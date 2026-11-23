//! Recados para o dono (porte de braniac/recados.py), SEM IA no caminho: quando o Jarvis promete "vou passar pra ele",
//! o recado entra numa fila em disco (dados/recados.json) com as palavras LITERAIS da pessoa e e entregue pela closure de envio;
//! se o envio falhar, a fila tenta de novo a cada ciclo (ate 6 vezes), mantendo a ordem. Um recado por pessoa e mensagem.
//! Diferencas do Python: o nome do dono entra como parametro (la esta fixo nas regex) e a chave do recado e FNV-1a de 64 bits
//! (la e SHA-1 cortado; nao ha crate de SHA-1 aqui e a chave so serve para nao repetir).
#![allow(dead_code)]
use regex::Regex;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::OnceLock;

const ARQ: &str = "recados.json";
pub const MAX_TENTATIVAS: i64 = 6;
pub const MAX_CHARS: usize = 700;
const MAX_FILA: usize = 300;

fn re(cel: &'static OnceLock<Regex>, padrao: &str) -> &'static Regex {
    cel.get_or_init(|| Regex::new(padrao).expect("regex fixa"))
}

fn corta(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// True se a resposta PROMETE repassar recado ou diz que o dono vai retornar.
/// Por frase: (1a pessoa/futuro + verbo de repassar + alvo) ou "ele/a gente ... retorna". `dono` = primeiro nome do dono.
pub fn promete_repassar(resposta: &str, dono: &str) -> bool {
    static CUE: OnceLock<Regex> = OnceLock::new();
    static VERBO: OnceLock<Regex> = OnceLock::new();
    static RETORNO2: OnceLock<Regex> = OnceLock::new();
    let d = regex::escape(&dono.trim().to_lowercase());
    let d = if d.is_empty() { "ele".to_string() } else { d }; // sem nome configurado: so "ele" vale como alvo
    // shortcut: compila a cada chamada porque o nome vem de fora; guardar por nome se isto entrar em laco quente
    let monta = |p: String| Regex::new(&p).ok();
    let promessa = monta(format!(concat!(
        r"(?i)(?:vou|vamos|irei|iremos|j[aá]\s+vou|posso)\s+(?:logo\s+)?(?:passar|repassar|avisar|informar|comunicar|encaminhar|levar|contar|dizer|falar|deixar\s+(?:o\s+)?recado|anotar|registrar|transmitir|mandar|enviar)\b[^.!?\n]{{0,40}}\b(?:{d}|ele|pra\s+ele|para\s+ele)\b|",
        r"(?:passei|passo|repassei|repasso|avisei|aviso|informei|comuniquei|encaminhei|anotei|registrei|deixei|transmiti|mandei)\b[^.!?\n]{{0,40}}\b(?:{d}|ele|recado|mensagem|pedido)\b|",
        r"(?:anotei|registrei|deixei)\s+(?:o\s+|seu\s+|esse\s+|este\s+)?(?:recado|pedido|aviso)|",
        r"(?:o\s+)?{d}\s+(?:vai|ir[aá]|j[aá]|logo)\s+(?:ver|saber|retornar|te\s+(?:responder|ligar|chamar|retornar|procurar)|entrar\s+em\s+contato|falar\s+com\s+voc[eê])|",
        r"ele\s+(?:vai|ir[aá])\s+(?:ver|saber|retornar|te\s+(?:responder|ligar|chamar|retornar|procurar)|entrar\s+em\s+contato)|",
        r"pode\s+deixar\s+que\s+(?:eu\s+)?(?:aviso|passo|falo|aviso\s+o|repasso|digo)|",
        r"(?:avisarei|repassarei|informarei|comunicarei|encaminharei)\b|",
        r"(?:aviso|digo|repasso|falo)\s+(?:pra|para)\s+(?:ele|o\s+{d})|",
        r"recado\s+(?:anotado|registrado|repassado|enviado)"), d = d));
    let alvo = monta(format!(r"(?i)\b(?:{d}|ele|ela|chefe|dono|recado|mensagem|msg|pedido|aviso)\b"));
    let retorno = monta(format!(r"(?i)\b(?:ele|ela|{d}|a\s+gente|agente|gente)\b[^.!?\n]{{0,25}}\b(?:retorn|respond|cham[ae]|lig[ae]|entra\s+em\s+contato|procur|volta|fala\s+com\s+voc)"));
    let (Some(promessa), Some(alvo), Some(retorno)) = (promessa, alvo, retorno) else { return false };
    if promessa.is_match(resposta) {
        return true;
    }
    let cue = re(&CUE, r"(?i)\b(?:vou|vo|vamos|vamo|irei|iremos|ja|j[aá]|jah|pode\s+dex\w*|pode\s+deixa\w*|deixa|dexa|a\s+gente|agente|tamo|estou|to|t[aá]|ta|passei|passo|avisei|aviso|anotei|anotado|notei|registrei|repasso|repassei|informo|encaminho|mando|envio)\b");
    let verbo = re(&VERBO, r"(?i)(?:pass[aoe]|repass|avis|inform|comunic|encaminh|dar\s+(?:o|um)\s+(?:recado|toque)|dou\s+(?:o|um)\s+(?:recado|toque)|mand[aeo]|envi[aeo]|anot|regist|notei|dexa|deix|fal[ao]|fald|dig[ao]|diz|cont[ao]|transmit|entreg|pedir|pe[cç]o)");
    let retorno2 = re(&RETORNO2, r"(?i)\b(?:retorn|respond|cham|lig)\w*\b[^.!?\n]{0,15}\b(?:assim\s+que|logo\s+que)\b");
    resposta.split(['.', '!', '?', '\n']).any(|f| (cue.is_match(f) && verbo.is_match(f) && alvo.is_match(f)) || retorno.is_match(f) || retorno2.is_match(f))
}

fn ler(dados: &Path) -> Vec<Value> {
    std::fs::read_to_string(dados.join(ARQ)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn gravar(dados: &Path, fila: &[Value]) {
    let _ = std::fs::create_dir_all(dados);
    let ultimos = &fila[fila.len().saturating_sub(MAX_FILA)..];
    let _ = std::fs::write(dados.join(ARQ), serde_json::to_string_pretty(ultimos).unwrap_or_else(|_| "[]".into()));
}

fn chave(nome: &str, entrada: &str) -> String {
    let base = format!("{nome}|{}", entrada.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase());
    let h = base.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
    format!("{h:016x}")
}

/// Texto do recado: as palavras da pessoa entre « », cortadas em 700 caracteres sem perder o inicio.
pub fn formatar(nome: &str, entrada: &str, resposta: &str, nota: &str) -> String {
    static ESP: OnceLock<Regex> = OnceLock::new();
    let mut corpo = re(&ESP, r"[ \t]+").replace_all(entrada, " ").trim().to_string();
    if corpo.chars().count() > MAX_CHARS {
        corpo = corta(&corpo, MAX_CHARS).trim_end().to_string() + "…";
    }
    let mut linhas = vec![format!("📩 Recado de {nome}:"), format!("«{corpo}»")];
    if !nota.is_empty() {
        linhas.push(format!("Obs. do Jarvis: {}", corta(nota.trim(), 160)));
    } else if !resposta.is_empty() {
        linhas.push(format!("Jarvis respondeu: «{}»", corta(resposta.trim(), 140)));
    }
    linhas.join("\n")
}

/// Guarda o recado (uma vez por pessoa+mensagem). Devolve true se entrou na fila agora.
pub fn enfileirar(dados: &Path, nome: &str, entrada: &str, resposta: &str, nota: &str, ts: f64) -> bool {
    let entrada = entrada.trim();
    if nome.is_empty() || entrada.is_empty() {
        return false;
    }
    let k = chave(nome, entrada);
    let mut fila = ler(dados);
    if fila.iter().any(|r| r["k"] == k.as_str()) {
        return false;
    }
    fila.push(json!({ "k": k, "nome": nome, "texto": formatar(nome, entrada, resposta, nota), "entrada": corta(entrada, MAX_CHARS),
                      "resposta": corta(resposta, 300), "ts": ts, "estado": "pendente", "tentativas": 0 }));
    gravar(dados, &fila);
    true
}

pub fn pendentes(dados: &Path) -> Vec<Value> {
    ler(dados).into_iter().filter(|r| r["estado"] == "pendente").collect()
}

/// Entrega em audio: `roteiro(nome, falas, respostas)` monta a fala (recado_falado::roteiro) e `falar(destino, fala)` manda.
pub struct Audio<'a> {
    pub falar: &'a dyn Fn(&str, &str) -> String,
    pub roteiro: &'a dyn Fn(&str, &[String], &[String]) -> String,
}

/// Envia os recados pendentes ao dono. `enviar(destino, texto)` devolve "✅..." se enviou.
/// Com `audio`: UM audio por pessoa; se o audio falhar, cai no texto de sempre. Devolve quantos recados foram entregues.
pub fn entregar(dados: &Path, destino: &str, ts: f64, enviar: &dyn Fn(&str, &str) -> String, audio: Option<&Audio>) -> usize {
    let mut fila = ler(dados);
    let pendente = |r: &Value| r["estado"] == "pendente";
    let texto = |r: &Value, c: &str| r[c].as_str().unwrap_or("").to_string();
    let mut n = 0;
    if let Some(a) = audio {
        let mut nomes: Vec<String> = Vec::new();
        for r in fila.iter().filter(|r| pendente(r)) {
            if !nomes.contains(&texto(r, "nome")) {
                nomes.push(texto(r, "nome"));
            }
        }
        for nome in nomes {
            let dele = |r: &Value| pendente(r) && r["nome"] == nome.as_str();
            let falas: Vec<String> = fila.iter().filter(|r| dele(r)).map(|r| if texto(r, "entrada").is_empty() { texto(r, "texto") } else { texto(r, "entrada") }).collect();
            let respostas: Vec<String> = fila.iter().filter(|r| dele(r)).map(|r| texto(r, "resposta")).filter(|x| !x.is_empty()).collect();
            if (a.falar)(destino, &(a.roteiro)(&nome, &falas, &respostas)).starts_with('✅') {
                for r in fila.iter_mut().filter(|r| r["estado"] == "pendente" && r["nome"] == nome.as_str()) {
                    r["estado"] = json!("entregue");
                    r["entregue_em"] = json!(ts);
                    n += 1;
                }
            }
        }
    }
    for r in fila.iter_mut().filter(|r| r["estado"] == "pendente") {
        let ok = enviar(destino, r["texto"].as_str().unwrap_or("")).starts_with('✅');
        let tentativas = r["tentativas"].as_i64().unwrap_or(0) + 1;
        r["tentativas"] = json!(tentativas);
        if ok {
            r["estado"] = json!("entregue");
            r["entregue_em"] = json!(ts);
            n += 1;
        } else if tentativas >= MAX_TENTATIVAS {
            r["estado"] = json!("falhou");
        } else {
            break; // canal indisponivel: para e tenta no proximo ciclo (mantem a ordem)
        }
    }
    gravar(dados, &fila);
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn pasta(nome: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("braniac-recados-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn promessa_de_repassar_e_detectada() {
        for f in ["Vou passar pro Bruno, pode deixar.", "Anotei seu recado!", "Ele te retorna assim que puder.", "A gente avisa ele assim que possível.",
                  "Blz vo passa pro bruno", "Já informo ele, tá?", "Vou pedir pro Bruno te chamar."] {
            assert!(promete_repassar(f, "Bruno"), "{f}");
        }
    }

    #[test]
    fn conversa_normal_nao_vira_recado() {
        for f in ["Vou te ajudar com isso", "Oi! Aqui é o Jarvis, assistente do Bruno", "Ele disse que o preço é R$ 150", "Recebi sua mensagem, já te respondo",
                  "Já te passo o valor do combo", "A gente atende de quinta a terça", "Posso te enviar o link do site", ""] {
            assert!(!promete_repassar(f, "Bruno"), "{f}");
        }
        assert!(!promete_repassar("Vou passar pro Bruno", "") && promete_repassar("Vou passar pra ele", ""));
    }

    #[test]
    fn recado_e_literal_sem_duplicar_e_entregue_com_nova_tentativa() {
        let p = pasta("literal");
        let msg = "Oi, preciso falar com ele sobre o orçamento do casamento, é urgente 🙏";
        assert!(enfileirar(&p, "Ana", msg, "Vou passar pra ele!", "", 1.0));
        assert!(!enfileirar(&p, "Ana", &format!("  {}  ", msg.to_uppercase().replace(' ', "  ")), "", "", 2.0)); // mesma mensagem: nao repete
        assert!(!enfileirar(&p, "", msg, "", "", 2.0) && !enfileirar(&p, "Ana", "  ", "", "", 2.0));
        let t = pendentes(&p)[0]["texto"].as_str().unwrap().to_string();
        assert!(t.contains("orçamento do casamento, é urgente") && t.ends_with("Jarvis respondeu: «Vou passar pra ele!»"));
        assert!(entregar(&p, "Dono", 3.0, &|_, _| "falhou".into(), None) == 0 && pendentes(&p).len() == 1); // falhou: continua na fila
        let enviados = RefCell::new(Vec::new());
        let n = entregar(&p, "Dono", 4.0, &|d, t| {
            enviados.borrow_mut().push((d.to_string(), t.to_string()));
            "✅ ok".into()
        }, None);
        assert!(n == 1 && pendentes(&p).is_empty());
        assert!(enviados.borrow()[0].0 == "Dono" && enviados.borrow()[0].1.starts_with("📩 Recado de Ana:"));
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn recado_longo_e_cortado_e_falha_seis_vezes_desiste_sem_travar_a_fila() {
        let p = pasta("longo");
        enfileirar(&p, "Zé", &"pedido ".repeat(300), "", "veio por áudio", 1.0);
        let t = pendentes(&p)[0]["texto"].as_str().unwrap().to_string();
        assert!(t.matches("pedido").count() < 300 && t.contains("Recado de Zé") && t.contains("…»\nObs. do Jarvis: veio por áudio"));
        enfileirar(&p, "Bia", "segundo", "", "", 2.0);
        let chamadas = RefCell::new(0);
        let falha = |_: &str, _: &str| {
            *chamadas.borrow_mut() += 1;
            "erro".to_string()
        };
        for _ in 0..5 {
            assert_eq!(entregar(&p, "Dono", 3.0, &falha, None), 0);
        }
        assert_eq!(*chamadas.borrow(), 5); // parou no primeiro: a ordem se mantem
        entregar(&p, "Dono", 3.0, &falha, None); // 6a falha do primeiro: desiste dele e tenta o segundo
        let f = ler(&p);
        assert!(f[0]["estado"] == "falhou" && f[1]["estado"] == "pendente" && f[1]["tentativas"] == 1);
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn entrega_em_audio_agrupa_por_pessoa_e_cai_no_texto() {
        let p = pasta("audio");
        enfileirar(&p, "Ana", "tô melhor hoje", "Que bom!", "", 1.0);
        enfileirar(&p, "Ana", "obrigada por perguntar", "", "", 2.0);
        let falas = RefCell::new(Vec::new());
        let roteiro = |nome: &str, f: &[String], r: &[String]| format!("{nome}: {} / {}", f.join(" + "), r.join(" + "));
        let falar = |_: &str, f: &str| {
            falas.borrow_mut().push(f.to_string());
            "✅ Áudio".to_string()
        };
        assert_eq!(entregar(&p, "Dono", 3.0, &|_, _| "falhou".into(), Some(&Audio { falar: &falar, roteiro: &roteiro })), 2);
        assert!(pendentes(&p).is_empty());
        assert_eq!(*falas.borrow(), vec!["Ana: tô melhor hoje + obrigada por perguntar / Que bom!"]); // 2 recados da Ana = 1 audio
        enfileirar(&p, "Bia", "preciso de você", "", "", 4.0);
        let mudo = |_: &str, _: &str| "erro".to_string();
        assert_eq!(entregar(&p, "Dono", 5.0, &|_, _| "✅ ok".into(), Some(&Audio { falar: &mudo, roteiro: &roteiro })), 1); // audio falhou: texto de sempre
        let _ = std::fs::remove_dir_all(&p);
    }
}
