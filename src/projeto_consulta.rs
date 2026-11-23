//! Projeto sem registro (braniac/projeto_consulta.py): quando o dono fala de um projeto que o banco nao conhece, o Jarvis olha as pastas
//! do PC (aprende na hora, sem IA) e, se nao achar, monta a pergunta ao GPT e a tarefa SOMENTE LEITURA ao Claude. Cada projeto no
//! maximo 1x por 24 h (dados/projetos_consultados.json). Dado sensivel nunca vai a IA online.
//! De fora: as threads e as chamadas as IAs. `verificar` devolve o que perguntar; `volta_gpt`/`volta_claude` dizem o que gravar e avisar.
//! Quem e o dono e onde procurar vem de configuracao (`Quem`), nao fica no codigo.
#![allow(dead_code)]
use crate::texto_pt::sem_acento;
use regex::Regex;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const DIA: f64 = 24.0 * 3600.0;
const NAO_NOME: &[&str] = &[
    "novo", "nova", "novos", "novas", "que", "esse", "essa", "isso", "este", "esta", "meu", "minha", "nosso", "nossa", "seu", "sua", "dele", "dela", "atual", "anterior", "outro",
    "outra", "todo", "toda", "aquele", "aquela", "mesmo", "mesma", "pronto", "pronta", "bom", "boa", "legal", "bem", "mais", "menos", "tem", "para", "com", "sem", "uma", "uns",
    "umas", "seguinte", "aqui", "ali", "cliente", "clientes", "braniac", "jarvis", "airbnb", "whatsapp", "telegram", "ja", "ainda", "projetos", "apps", "sites", "sistemas", "jogos",
    "software", "tudo", "nada", "algum", "alguma", "qualquer", "funciona", "funcionando", "quebrado", "rodando",
];

/// Como o dono e descrito ao GPT e onde o Claude deve procurar (texto livre, vem de dados/pessoal.json).
pub struct Quem<'a> {
    pub nome: &'a str,
    pub descricao: &'a str,
    pub onde_procurar: &'a str,
}

#[derive(Debug, PartialEq)]
pub enum Decisao {
    /// Nao citou projeto, ou o banco ja conhece.
    Nada,
    /// So uma instrucao para o Jarvis incluir na resposta.
    Instrucao(String),
    /// Projeto desconhecido: perguntar ao GPT e passar a tarefa ao Claude (em segundo plano, por quem chama).
    Consultar { nome: String, instrucao: String, pergunta_gpt: String, tarefa_claude: String },
}

fn re(p: &str) -> Regex {
    Regex::new(p).expect("regex fixa")
}

fn so_letras(t: &str) -> String {
    sem_acento(t).chars().filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit()).collect()
}

fn ini(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Nome do projeto citado na frase ("o projeto Taximetro", "app SePique") ou None.
pub fn nome_do_projeto(texto: &str) -> Option<String> {
    let r = re(r"(?i)\b(?:projeto|app|aplicativo|jogo|site|sistema|plataforma|software)\s+(?:(?:d[aeo]s?|chamad[oa]|que\s+se\s+chama|o|a)\s+)?([A-Za-zÀ-ÿ0-9][\wÀ-ÿ-]{2,})");
    let achado = r
        .captures_iter(texto)
        .map(|m| m[1].trim_matches(['-', '_']).to_string())
        .find(|n| !n.is_empty() && !NAO_NOME.contains(&sem_acento(n).as_str()) && !n.chars().all(|c| c.is_ascii_digit()));
    achado
}

/// Pasta local parecida com o nome (SePique -> SePiqueRS, sepique-api...).
pub fn pasta(nome: &str, pastas: &[PathBuf]) -> Option<PathBuf> {
    let n = so_letras(nome);
    if n.len() < 3 {
        return None;
    }
    for base in pastas {
        let Ok(rd) = base.read_dir() else { continue };
        let mut filhos: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        filhos.sort();
        for p in filhos {
            let q = so_letras(&p.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default());
            if p.is_dir() && q.len() >= 3 && (q.contains(&n) || n.contains(&q)) {
                return Some(p);
            }
        }
    }
    None
}

pub fn resumo_da_pasta(p: &Path) -> String {
    for arq in ["README.md", "readme.md", "CLAUDE.md", "AGENTS.md", "package.json", "Cargo.toml"] {
        let Ok(b) = std::fs::read(p.join(arq)) else { continue };
        let txt = String::from_utf8_lossy(&b).split_whitespace().collect::<Vec<_>>().join(" ");
        if !txt.is_empty() {
            return ini(&txt, 420);
        }
    }
    let Ok(rd) = p.read_dir() else { return String::new() };
    let mut nomes: Vec<String> = rd.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
    nomes.sort();
    nomes.truncate(12);
    format!("contém: {}", nomes.join(", "))
}

/// `resultado` = texto que o banco devolveu para o nome. So conta como "conhecido" o fato que DESCREVE o projeto (nome seguido de
/// plataforma/app/projeto...); mencao solta, em curriculo ou em conversa, nao.
pub fn no_banco(nome: &str, resultado: &str) -> bool {
    let n = regex::escape(&sem_acento(nome));
    let desc = re(&format!(
        r"(?i){n}\W{{0,3}}(?:e\s+|era\s+)?(?:um|uma|o|a)?\s*(?:plataforma|app|aplicativo|sistema|site|jogo|projeto|software)\b|(?:projeto|app|aplicativo|sistema|jogo|plataforma|site)\s+{n}\W{{0,3}}(?:e|era|:|-)"
    ));
    resultado.lines().map(sem_acento).any(|l| desc.is_match(&l) && !l.contains("curriculo") && !l.starts_with("[conversa-jarvis]"))
}

fn consultados(dados: &Path) -> Value {
    std::fs::read_to_string(dados.join("projetos_consultados.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}))
}

/// Chamado a cada fala do dono. `resultado_banco` = o que o banco devolveu para o nome (closure: so e chamada se houver nome).
/// `gravar_fato(chave, texto, fonte)` so e usado quando a pasta do PC ensina o projeto.
pub fn verificar(texto: &str, dados: &Path, pastas: &[PathBuf], agora: f64, quem: &Quem, sensivel: Option<&Regex>, resultado_banco: &dyn Fn(&str) -> String, gravar_fato: &dyn Fn(&str, &str, &str)) -> Decisao {
    let Some(nome) = nome_do_projeto(texto) else { return Decisao::Nada };
    if no_banco(&nome, &resultado_banco(&nome)) {
        return Decisao::Nada;
    }
    let maiusc = nome.to_uppercase();
    if let Some(p) = pasta(&nome, pastas) {
        // existe no PC: o Jarvis aprende na hora, sem gastar IA
        let resumo = resumo_da_pasta(&p);
        if !resumo.is_empty() {
            let nome_pasta = p.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
            gravar_fato(&format!("projeto {nome}"), &format!("Projeto {nome}: pasta {nome_pasta} no PC do {}. {resumo}", quem.nome), "pasta");
            return Decisao::Instrucao(format!("[PROJETO {maiusc}: achei a pasta '{nome_pasta}' no PC e aprendi: {}. Use isso e diga o que sabe.]", ini(&resumo, 300)));
        }
    }
    let mut d = consultados(dados);
    let chave = sem_acento(&nome);
    if agora - d[&chave].as_f64().unwrap_or(0.0) < DIA {
        return Decisao::Instrucao(format!("[PROJETO {maiusc}: ja perguntei ao GPT e ao Claude nas ultimas 24h; diga que ainda nao tem resposta registrada se nao vier nada acima.]"));
    }
    d[&chave] = json!(agora);
    let _ = std::fs::create_dir_all(dados);
    let _ = std::fs::write(dados.join("projetos_consultados.json"), d.to_string());
    let mut frase = ini(texto.trim(), 250);
    if sensivel.is_some_and(|s| s.is_match(&frase)) {
        frase.clear(); // dado sensivel nunca vai a IA online; so o Claude local (le as pastas) e acionado
    }
    let pergunta_gpt = format!(
        "Meu dono, {} ({}), citou o projeto \"{nome}\" ao assistente pessoal de IA dele{}. O assistente nao tem nada registrado sobre esse projeto. \
         O que provavelmente e esse projeto e que 3 perguntas curtas o assistente deve fazer ao {} para entender? Resposta curta, em portugues.",
        quem.nome,
        quem.descricao,
        if frase.is_empty() { String::new() } else { format!(", assim: \"{frase}\"") },
        quem.nome
    );
    let tarefa_claude = format!(
        "SOMENTE LEITURA, nao altere arquivo nenhum. O {} falou do projeto \"{nome}\" e o Jarvis nao tem nada sobre ele. Procure em {} \
         quem e esse projeto (o que e, estado, pasta, ultimo trabalho). Se nao achar, diga isso. Responda em ate 8 linhas.",
        quem.nome, quem.onde_procurar
    );
    let instrucao = format!(
        "[PROJETO {maiusc}: NAO ha nada sobre ele no banco nem numa pasta do PC. Diga ao {}, em 1 frase natural, que ainda nao conhece esse projeto, \
         que ja perguntou ao GPT e ao Claude e que avisa quando voltarem. Nao invente nada sobre o projeto.]",
        quem.nome
    );
    Decisao::Consultar { nome, instrucao, pergunta_gpt, tarefa_claude }
}

/// Resposta do GPT (ou o erro dele) -> (fato a gravar: (chave, texto, fonte), aviso ao dono). Resposta de erro avisa, mas nao vira fato.
pub fn volta_gpt(nome: &str, resposta: Result<&str, &str>) -> (Option<(String, String, &'static str)>, String) {
    let r = match resposta {
        Ok(r) => r.trim().to_string(),
        Err(e) => format!("nao consegui falar com o GPT ({})", ini(e, 80)),
    };
    let min = r.to_lowercase();
    let erro = ["nao consegui", "pergunta barrada", "ia desconhecida"].iter().any(|p| min.starts_with(p));
    let fato = (!r.is_empty() && !erro).then(|| (format!("projeto {nome} (gpt)"), format!("Projeto {nome}: segundo o GPT, {}", ini(&r, 500)), "gpt"));
    (fato, format!("🔎 Projeto \"{nome}\" — o que o GPT disse:\n\n{}", ini(&r, 900)))
}

/// Resposta do Claude -> (fato a gravar, aviso ao dono). "Nao achei" e erro avisam, mas nao viram fato.
pub fn volta_claude(nome: &str, resposta: &str) -> (Option<(String, String, &'static str)>, String) {
    let r = resposta.trim();
    let vale = !r.is_empty() && !ini(&sem_acento(r), 80).contains("nao achei") && !r.starts_with('⚠') && !r.starts_with('❌');
    let fato = vale.then(|| (format!("projeto {nome} (claude)"), format!("Projeto {nome}: segundo o Claude (que olhou o PC), {}", ini(r, 600)), "claude"));
    (fato, format!("🔎 Projeto \"{nome}\" — o que o Claude achou:\n\n{}", ini(r, 1200)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const QUEM: Quem = Quem { nome: "Ana", descricao: "desenvolvedora", onde_procurar: "D:\\Projetos" };

    fn tmp(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-projeto_consulta-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("Projetos")).unwrap();
        d
    }

    fn roda(t: &Path, texto: &str, agora: f64, sens: Option<&Regex>) -> (Decisao, Vec<String>) {
        let fatos = RefCell::new(vec![]);
        let d = verificar(texto, &t.join("dados"), &[t.join("Projetos")], agora, &QUEM, sens, &|_| String::new(), &|_, _, fonte| fatos.borrow_mut().push(fonte.to_string()));
        (d, fatos.into_inner())
    }

    #[test]
    fn acha_o_nome_do_projeto() {
        assert_eq!(nome_do_projeto("como esta o projeto Taximetro?").as_deref(), Some("Taximetro"));
        assert_eq!(nome_do_projeto("abre o app SePique").as_deref(), Some("SePique"));
        assert_eq!(nome_do_projeto("o sistema chamado Órbita-2 caiu").as_deref(), Some("Órbita-2"));
        assert_eq!(nome_do_projeto("tenho um projeto novo, o app Zeta"), Some("Zeta".into()));
        for nada in ["tenho um projeto novo", "bom dia, tudo bem?", "o site 2026", "o app do WhatsApp"] {
            assert_eq!(nome_do_projeto(nada), None, "{nada}");
        }
    }

    #[test]
    fn projeto_desconhecido_monta_pergunta_ao_gpt_e_tarefa_so_leitura_ao_claude() {
        let t = tmp("a");
        let (d, fatos) = roda(&t, "como esta o projeto Zeta?", 1_800_000_000.0, None);
        let Decisao::Consultar { nome, instrucao, pergunta_gpt, tarefa_claude } = d else { panic!("devia consultar") };
        assert!(nome == "Zeta" && instrucao.contains("NAO ha nada") && instrucao.contains("Nao invente") && fatos.is_empty());
        assert!(tarefa_claude.starts_with("SOMENTE LEITURA, nao altere arquivo nenhum.") && tarefa_claude.contains("D:\\Projetos"));
        assert!(pergunta_gpt.contains("Ana (desenvolvedora)") && pergunta_gpt.contains(", assim: \"como esta o projeto Zeta?\""));
        let (fato, aviso) = volta_gpt("Zeta", Ok(" e um app de corridas "));
        assert_eq!(fato.map(|f| (f.1, f.2)), Some(("Projeto Zeta: segundo o GPT, e um app de corridas".into(), "gpt")));
        assert!(aviso.ends_with("o que o GPT disse:\n\ne um app de corridas"));
        let (fato, aviso) = volta_claude("Zeta", "achei a pasta X");
        assert!(fato.is_some_and(|f| f.2 == "claude") && aviso.contains("o que o Claude achou"));
    }

    #[test]
    fn so_uma_vez_por_dia() {
        let t = tmp("b");
        roda(&t, "projeto Zeta", 1_800_000_000.0, None);
        let (d, _) = roda(&t, "e o projeto Zêta de novo?", 1_800_000_000.0 + DIA - 1.0, None);
        assert!(matches!(&d, Decisao::Instrucao(i) if i.contains("ja perguntei")), "{d:?}");
        assert!(matches!(roda(&t, "projeto Zeta", 1_800_000_000.0 + DIA + 1.0, None).0, Decisao::Consultar { .. }));
    }

    #[test]
    fn pasta_local_ensina_sem_gastar_ia() {
        let t = tmp("c");
        std::fs::create_dir_all(t.join("Projetos").join("TaximetroApp")).unwrap();
        std::fs::write(t.join("Projetos").join("TaximetroApp").join("README.md"), "Taximetro para\n motoristas   de app").unwrap();
        let (d, fatos) = roda(&t, "fala do projeto Taximetro", 1_800_000_000.0, None);
        assert_eq!(d, Decisao::Instrucao("[PROJETO TAXIMETRO: achei a pasta 'TaximetroApp' no PC e aprendi: Taximetro para motoristas de app. Use isso e diga o que sabe.]".into()));
        assert_eq!(fatos, ["pasta"]);
        assert!(!t.join("dados").exists()); // nao gastou a consulta do dia
        std::fs::create_dir_all(t.join("Projetos").join("zeta-api")).unwrap();
        std::fs::write(t.join("Projetos").join("zeta-api").join("main.rs"), "").unwrap();
        assert_eq!(resumo_da_pasta(&t.join("Projetos").join("zeta-api")), "contém: main.rs");
        assert_eq!(pasta("ze", &[t.join("Projetos")]), None);
    }

    #[test]
    fn dado_sensivel_nao_vai_ao_gpt() {
        let t = tmp("d");
        let sens = Regex::new("(?i)senha").unwrap();
        let (d, _) = roda(&t, "projeto Zeta senha 1234", 1_800_000_000.0, Some(&sens));
        let Decisao::Consultar { pergunta_gpt, .. } = d else { panic!("devia consultar") };
        assert!(!pergunta_gpt.contains("senha") && !pergunta_gpt.contains("1234") && !pergunta_gpt.contains("assim:"));
    }

    #[test]
    fn mencao_solta_no_banco_nao_conta_como_conhecido() {
        assert!(!no_banco("Okarã", "[fato] Curriculo atualizado: Okara removido; Foundever em destaque"));
        assert!(no_banco("Okarã", "[fato] Projeto Okara: plataforma de hospedagens em Next.js"));
        assert!(no_banco("Zeta", "x\n[fato] Zeta é um app de corridas"));
        assert!(!no_banco("Zeta", "[conversa-jarvis] projeto Zeta: nao sei") && !no_banco("Zeta", "[fato] falei com o Zeta ontem"));
        let t = tmp("e");
        let d = verificar("e o projeto Zeta?", &t.join("dados"), &[], 1.0, &QUEM, None, &|_| "[fato] Projeto Zeta: site de receitas".into(), &|_, _, _| panic!("nao grava"));
        assert_eq!(d, Decisao::Nada);
    }

    #[test]
    fn resposta_de_erro_avisa_mas_nao_vira_fato() {
        for r in [Ok("Nao consegui abrir a aba"), Ok("pergunta barrada: dado pessoal"), Ok(""), Err("timeout")] {
            assert!(volta_gpt("Zeta", r).0.is_none(), "{r:?}");
        }
        assert!(volta_gpt("Zeta", Err("timeout")).1.contains("nao consegui falar com o GPT (timeout)"));
        for r in ["Não achei nada sobre esse projeto.", "⚠ limite", "❌ erro", "  "] {
            assert!(volta_claude("Zeta", r).0.is_none(), "{r}");
        }
    }
}
