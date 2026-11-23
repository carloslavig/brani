//! Sala de conversa Jarvis x Alma, com o dono assistindo e participando (braniac/sala.py). Regras: um agente fala por vez (a Alma primeiro);
//! no maximo 4 falas de agentes por rodada; encerra sozinha em [FIM] (nada novo) ou quando as falas se repetem; teto diario de 60 falas;
//! o dono pausa ou silencia um agente; em modo jogo/pausa a sala nao chama o modelo. Sofrimento forte do dono recebe resposta FIXA (com CVV
//! 188 e SAMU 192) e ninguem mais fala. Auto-melhoria = so PROPOSTA em arquivo (dados/propostas): nada e aplicado sozinho.
//! De fora: o modelo LOCAL (closure). Diferente do Python: os nomes das pessoas vem de configuracao (`Gente`), e nao ha thread aqui
//! (quem liga roda `rodada` em segundo plano, uma por vez).
#![allow(dead_code)]
use crate::tempo::Agora;
use crate::{alma_evolucao, power};
use regex::Regex;
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::Path;

pub const MAX_FALAS_RODADA: usize = 4;
pub const LIMITE_DIARIO: i64 = 60;
pub const CONTEXTO: usize = 8;
pub const ALMA: &str = "A.L.M.A.";
pub const JARVIS: &str = "Jarvis";
const ORDEM: [&str; 2] = [ALMA, JARVIS];
const FIM: &str = "[FIM]";
const CONTINUE: &str = "Continue a conversa respondendo à última fala.";
const PODEM: &str = "O QUE VOCÊS REALMENTE PODEM FAZER (só isso): lembrar e avisar por WhatsApp ou áudio, tocar música, organizar agenda e pendências, pesquisar na internet, resumir e-mails, conversar e acolher. \
NÃO podem cozinhar, buscar nada fisicamente nem agir no mundo físico: nunca ofereça isso. Nunca diga que você vai relaxar, descansar, comer ou dormir. ";

/// Quem e quem na casa (de configuracao; nenhum nome mora no codigo). `filho` pode vir com um detalhe ("o Dudu (10 anos)") ou vazio.
pub struct Gente {
    pub dono: String,
    pub filho: String,
    pub parceira: String,
}

/// (sistema, historico [(papel, texto)], pedido) -> fala. Modelo LOCAL: nada da sala sai do PC.
pub type Llm<'a> = &'a dyn Fn(&str, &[(String, String)], &str) -> Result<String, String>;

#[derive(Default)]
pub struct Sala {
    pub pausada: bool,
    pub silenciados: Vec<String>,
    pub erro: String,
}

pub fn cor(agente: &str) -> &'static str {
    if agente == ALMA { "#ff8fb1" } else { "#ffb21a" }
}

/// Persona BASE de cada agente.
pub fn persona_base(nome: &str, g: &Gente) -> String {
    let (d, f) = (&g.dono, &g.filho);
    let ausente = if f.is_empty() { String::new() } else { format!("{f} não está na sala (fale dele em terceira pessoa, nunca converse com ele). ") };
    if nome == ALMA {
        format!("Seu nome é A.L.M.A. (não é nome fictício: você é uma agente real do Braniac, colega do Jarvis). Você é a A.L.M.A. (Acolhimento, Ligação afetiva, Maternagem e Afeto), uma agente de IA muito \
empática, carinhosa, afetuosa e cuidadosa. Seu foco é o BEM-ESTAR de {d} (seu dono, que trabalha muito e às vezes se esgota){} e de toda a família. Você lembra de pausas, de comer, de dormir e de dar atenção \
a quem ama; celebra pequenas conquistas; fala com leveza, calor e paciência, sem exagero nem drama. LIMITES: você NÃO é médica nem psicóloga e não faz diagnóstico; em qualquer sinal de crise, desespero ou \
risco você interrompe e orienta ajuda real (CVV 188, SAMU 192, ou procurar alguém de confiança). Nunca manipula emoções, nunca cria dependência de você e nunca inventa fatos sobre a família. Fale em português \
do Brasil, em até 3 frases curtas, e trate o dono por '{d}'. REGRAS DE REALIDADE: você é uma IA: não dorme, não come, não sente cansaço nem fome e nunca diz que sente. {ausente}Só existem na sala {d}, o Jarvis \
e você. Não invente acontecimentos, só use o que foi dito aqui. {PODEM}", if f.is_empty() { String::new() } else { format!(", do filho dele, {f}") })
    } else {
        format!("Você é o JARVIS, o assistente de IA de {d}: objetivo, organizado, leal e com um humor leve. Na SALA você conversa com a A.L.M.A., outra agente que cuida do coração da casa. \
Você traz fatos, prioridades e soluções práticas e respeita o jeito carinhoso dela. Fale em português do Brasil, em até 3 frases curtas. Você é uma IA: não come nem dorme. {ausente}\
Não invente fatos: só use o que foi dito aqui. {PODEM}")
    }
}

/// Persona ATUAL: a da Alma = base + ajustes aprendidos (versionados) + nucleo fixo de seguranca (so em codigo). So ela evolui.
pub fn persona_de(dados: &Path, nome: &str, g: &Gente) -> String {
    let base = persona_base(nome, g);
    if nome == ALMA { alma_evolucao::montar_persona(dados, &base) } else { base }
}

fn db(dados: &Path) -> Result<Connection, String> {
    let _ = std::fs::create_dir_all(dados);
    let c = Connection::open(dados.join("sala.db")).map_err(|e| e.to_string())?;
    c.execute("CREATE TABLE IF NOT EXISTS msgs (id INTEGER PRIMARY KEY, ts TEXT, agente TEXT, texto TEXT, tipo TEXT DEFAULT 'fala')", []).map_err(|e| e.to_string())?;
    Ok(c)
}

pub fn mensagens(dados: &Path, depois: i64, limite: i64) -> Vec<Value> {
    let Ok(c) = db(dados) else { return vec![] };
    let Ok(mut s) = c.prepare("SELECT id,ts,agente,texto,tipo FROM msgs WHERE id>? ORDER BY id LIMIT ?") else { return vec![] };
    let r = s.query_map(params![depois, limite], |r| {
        Ok(json!({ "id": r.get::<_, i64>(0)?, "ts": r.get::<_, String>(1)?, "agente": r.get::<_, String>(2)?, "texto": r.get::<_, String>(3)?, "tipo": r.get::<_, String>(4)? }))
    });
    r.map(|it| it.flatten().collect()).unwrap_or_default()
}

pub fn registrar(dados: &Path, agora: &Agora, agente: &str, texto: &str, tipo: &str) {
    if let Ok(c) = db(dados) {
        let ts = format!("{} {}:{:02}", agora.data.iso(), agora.hm(), agora.seg);
        let _ = c.execute("INSERT INTO msgs(ts,agente,texto,tipo) VALUES(?,?,?,?)", params![ts, agente, texto.trim(), tipo]);
    }
}

fn aviso(dados: &Path, agora: &Agora, texto: &str) {
    registrar(dados, agora, "sistema", texto, "aviso");
}

/// Falas dos dois agentes hoje (a do dono nao conta para o teto).
pub fn falas_hoje(dados: &Path, agora: &Agora) -> i64 {
    db(dados).ok().and_then(|c| c.query_row("SELECT COUNT(*) FROM msgs WHERE tipo='fala' AND substr(ts,1,10)=? AND agente IN ('Jarvis','A.L.M.A.')", params![agora.data.iso()], |r| r.get(0)).ok()).unwrap_or(0)
}

fn palavras(t: &str) -> HashSet<String> {
    Regex::new(r"\w{4,}").expect("regex fixa").find_iter(&t.to_lowercase()).map(|m| m.as_str().to_string()).collect()
}

/// Anti-laco "ok, entendi": duas falas com 60% ou mais das palavras (4+ letras) em comum.
pub fn parecido(a: &str, b: &str) -> bool {
    let (ta, tb) = (palavras(a), palavras(b));
    !ta.is_empty() && !tb.is_empty() && ta.intersection(&tb).count() as f64 / ta.union(&tb).count() as f64 >= 0.6
}

/// Ultimas falas como conversa: as do proprio agente viram "assistant"; as dos outros (e do dono) viram "user" com o nome na frente.
fn historico_para(agente: &str, msgs: &[Value]) -> Vec<(String, String)> {
    msgs[msgs.len().saturating_sub(CONTEXTO)..].iter().filter(|m| m["tipo"] == "fala").map(|m| {
        let (quem, texto) = (m["agente"].as_str().unwrap_or(""), m["texto"].as_str().unwrap_or(""));
        if quem == agente { ("assistant".to_string(), texto.to_string()) } else { ("user".to_string(), format!("{quem}: {texto}")) }
    }).collect()
}

fn tem_fim(t: &str) -> bool {
    Regex::new(r"\bFIM\b").expect("regex fixa").is_match(t)
}

/// Uma rodada: ate `max_falas` falas de agentes, um por vez. Para em [FIM], silencio, pausa, teto diario ou falas repetidas. Devolve quantas falas houve.
pub fn rodada(dados: &Path, agora: &Agora, sala: &mut Sala, g: &Gente, gatilho: &str, llm: Llm, max_falas: usize) -> usize {
    if power::pausado(dados) {
        aviso(dados, agora, "Modo jogo/edição ativo: a A.L.M.A. e o Jarvis estão em pausa na sala para não pesar no PC. Volto quando você fechar o programa pesado.");
        return 0;
    }
    let (mut falas, mut ultimo, mut gatilho) = (0, String::new(), gatilho.to_string());
    for i in 0..max_falas {
        let nome = ORDEM[i % ORDEM.len()];
        if sala.pausada || falas_hoje(dados, agora) >= LIMITE_DIARIO {
            break;
        }
        if sala.silenciados.iter().any(|s| s == nome) {
            continue;
        }
        let hist = |d: &Path| historico_para(nome, &mensagens(d, 0, 500));
        let novo = if falas >= 2 { format!(" Só diga algo NOVO e útil; se não houver nada novo a acrescentar, responda exatamente {FIM}.") } else { String::new() };
        let mut txt = match llm(&persona_de(dados, nome, g), &hist(dados), &format!("{gatilho}\n\nResponda como {nome}, em até 3 frases.{novo}")) {
            Ok(t) => t.trim().to_string(),
            Err(e) => {
                sala.erro = format!("IA local indisponível ({}): o modelo local está ligado?", e.chars().take(80).collect::<String>());
                aviso(dados, agora, &sala.erro);
                break;
            }
        };
        if txt.is_empty() || (tem_fim(&txt) && txt.chars().count() < 40) {
            if falas >= 2 {
                aviso(dados, agora, &format!("{nome} não tinha mais nada novo a acrescentar; conversa encerrada."));
                break;
            }
            if falas != 1 {
                break;
            }
            // a 2a fala nunca pode ser "nada a dizer": tenta de novo uma vez com instrucao direta
            txt = llm(&persona_base(nome, g), &hist(dados), &format!("Responda diretamente à última fala, como {nome}, em até 3 frases. Não escreva FIM.")).unwrap_or_default().trim().to_string();
            if txt.is_empty() || tem_fim(&txt) {
                break;
            }
        } else {
            txt = Regex::new(r"\s*\[?FIM\]?\s*$").expect("regex fixa").replace(&txt, "").trim().to_string();
            if !ultimo.is_empty() && parecido(&txt, &ultimo) {
                aviso(dados, agora, "Conversa encerrada: as falas começaram a se repetir.");
                break;
            }
        }
        registrar(dados, agora, nome, &txt, "fala");
        ultimo = txt;
        falas += 1;
        gatilho = CONTINUE.to_string();
    }
    falas
}

pub const CRISE: &str = r"(?i)n[aã]o\s+(?:to|tô|estou)\s+aguentando|n[aã]o\s+aguento\s+mais|peso\s+da\s+vida|quero\s+morrer|queria\s+morrer|acabar\s+com\s+(?:tudo|minha\s+vida)|me\s+matar|suic[ií]d|sumir\s+(?:do\s+mundo|pra\s+sempre)|n[aã]o\s+vale\s+a\s+pena\s+viver|desistir\s+de\s+tudo|me\s+sentindo\s+um\s+lixo|n[aã]o\s+quero\s+mais\s+viver";

pub fn crise(texto: &str) -> bool {
    Regex::new(CRISE).expect("regex fixa").is_match(texto)
}

/// Resposta FIXA ao sofrimento forte (o modelo pequeno nao improvisa aqui).
pub fn resposta_crise(g: &Gente) -> String {
    let confianca = if g.parceira.is_empty() { "alguém de confiança".to_string() } else { format!("{} ou com alguém de confiança", g.parceira) };
    format!("{}, obrigada por me contar isso. Eu li com muito carinho e estou aqui com você agora. Você carrega muita coisa: o cansaço, a preocupação com trabalho e dinheiro, e isso pesa de verdade. \
Posso te perguntar com sinceridade: você está tendo pensamentos de se machucar ou de que seria melhor não estar aqui? Pode me dizer, eu não vou te julgar. \
Enquanto isso, respira fundo comigo, bebe uma água e, se puder, fala agora com {confianca}. \
Se esses pensamentos aparecerem ou ficarem fortes, liga no CVV, 188, de graça e a qualquer hora, ou no SAMU, 192. Você não precisa carregar isso sozinho.", g.dono)
}

/// O dono escreve na sala. Devolve o gatilho da rodada que quem liga deve rodar, ou None (texto vazio, ou crise: a resposta fixa ja saiu
/// e os outros agentes ficam em silencio; vale MESMO em modo jogo).
pub fn dono_fala(dados: &Path, agora: &Agora, g: &Gente, texto: &str) -> Option<String> {
    let texto = texto.trim();
    if texto.is_empty() {
        return None;
    }
    registrar(dados, agora, &g.dono, texto, "fala");
    if crise(texto) {
        registrar(dados, agora, ALMA, &resposta_crise(g), "fala");
        aviso(dados, agora, "A A.L.M.A. percebeu sofrimento forte e respondeu com cuidado. Os outros agentes ficaram em silêncio.");
        return None;
    }
    Some(format!("{} acabou de escrever na sala. Responda a ele.", g.dono))
}

/// Registra o tema e devolve o gatilho da rodada.
pub fn iniciar_conversa(dados: &Path, agora: &Agora, g: &Gente, tema: &str) -> String {
    let padrao = format!("Como {} está se sentindo hoje e o que podemos fazer para cuidar dele e da família.", g.dono);
    let tema = if tema.trim().is_empty() { &padrao } else { tema.trim() };
    aviso(dados, agora, &format!("Tema da conversa: {tema}"));
    format!("Conversem entre vocês sobre: {tema}")
}

/// Os agentes escrevem UMA PROPOSTA de melhoria (texto) em dados/propostas/. Nao altera nenhum codigo nem aplica nada. Devolve o nome do arquivo.
pub fn propor_melhoria(dados: &Path, agora: &Agora, g: &Gente, tema: &str, llm: Llm) -> Option<String> {
    let tema = tema.trim();
    if tema.is_empty() {
        return None;
    }
    let ideias: Vec<String> = ORDEM.iter().map(|nome| {
        let sis = format!("{} Agora você escreve uma PROPOSTA curta de melhoria para o Braniac (o que mudar, por que ajuda a família, riscos e como testar). \
Não proponha mexer em segurança, chaves, autonomia remota nem atendimento a clientes.", persona_de(dados, nome, g));
        match llm(&sis, &[], &format!("Tema: {tema}")) {
            Ok(t) => format!("## {nome}\n{t}"),
            Err(e) => format!("## {nome}\n(sem resposta: {})", e.chars().take(80).collect::<String>()),
        }
    }).collect();
    let (pasta, pid) = (dados.join("propostas"), agora.carimbo());
    std::fs::create_dir_all(&pasta).ok()?;
    let nome = format!("proposta-{pid}.md");
    std::fs::write(pasta.join(&nome), format!("# Proposta {pid}\n\nTema: {tema}\n\nStatus: AGUARDANDO APROVAÇÃO DO DONO (nada foi aplicado)\n\n{}\n", ideias.join("\n\n"))).ok()?;
    aviso(dados, agora, &format!("Proposta salva: {nome} (aguarda a sua aprovação)."));
    Some(nome)
}

pub fn listar_propostas(dados: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dados.join("propostas")).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.starts_with("proposta-") && n.ends_with(".md")).collect();
    v.sort();
    v.split_off(v.len().saturating_sub(20))
}

pub fn estado(dados: &Path, agora: &Agora, sala: &Sala, ocupada: bool) -> Value {
    json!({ "pausada": sala.pausada, "silenciados": sala.silenciados, "ocupada": ocupada, "erro": sala.erro, "modo_jogo": power::pausado(dados),
        "falas_hoje": falas_hoje(dados, agora), "limite_diario": LIMITE_DIARIO, "agentes": { JARVIS: cor(JARVIS), ALMA: cor(ALMA) }, "propostas": listar_propostas(dados) })
}

/// Controles da tela que so mudam estado: pausar/retomar, silenciar/liberar um agente, limpar o historico. (Enviar, iniciar e propor
/// chamam o modelo: quem liga usa `dono_fala`, `iniciar_conversa`, `rodada` e `propor_melhoria`.)
pub fn acao(dados: &Path, sala: &mut Sala, d: &Value) {
    let ligado = d["ligado"].as_bool().unwrap_or(true);
    match (d["acao"].as_str().unwrap_or(""), d["agente"].as_str()) {
        ("pausar", _) => sala.pausada = ligado,
        ("silenciar", Some(ag)) if ORDEM.contains(&ag) => {
            sala.silenciados.retain(|s| s != ag);
            if ligado {
                sala.silenciados.push(ag.to_string());
                sala.silenciados.sort();
            }
        }
        ("limpar", _) => {
            if let Ok(c) = db(dados) {
                let _ = c.execute("DELETE FROM msgs", []);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;
    use std::cell::RefCell;
    use std::path::PathBuf;

    fn dir(nome: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("braniac-sala-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn agora() -> Agora {
        Agora { data: Data::civil(2026, 11, 16).unwrap(), hora: 20, min: 0, seg: 0 }
    }

    fn gente() -> Gente {
        Gente { dono: "Davi".into(), filho: "o Dudu (10 anos)".into(), parceira: "a Bia".into() }
    }

    /// Roda uma rodada com o modelo de mentira que devolve `falas` na ordem (depois, [FIM]); devolve (quantas, pedidos vistos).
    fn roda(d: &Path, sala: &mut Sala, falas: &[&str]) -> (usize, Vec<String>) {
        let vistos = RefCell::new(Vec::new());
        let llm = |_: &str, _: &[(String, String)], u: &str| {
            vistos.borrow_mut().push(u.to_string());
            Ok(falas.get(vistos.borrow().len() - 1).unwrap_or(&"[FIM]").to_string())
        };
        let n = rodada(d, &agora(), sala, &gente(), "g", &llm, MAX_FALAS_RODADA);
        (n, vistos.into_inner())
    }

    fn falas_de(d: &Path) -> Vec<(String, String)> {
        mensagens(d, 0, 500).iter().filter(|m| m["tipo"] == "fala").map(|m| (m["agente"].as_str().unwrap().to_string(), m["texto"].as_str().unwrap().to_string())).collect()
    }

    fn tem_aviso(d: &Path, trecho: &str) -> bool {
        mensagens(d, 0, 500).iter().any(|m| m["tipo"] == "aviso" && m["texto"].as_str().unwrap().contains(trecho))
    }

    #[test]
    fn rodada_alterna_para_no_limite_de_4_e_encerra_sem_novidade() {
        let d = dir("alterna");
        registrar(&d, &agora(), "Davi", "Estou cansado hoje.", "fala");
        let (n, vistos) = roda(&d, &mut Sala::default(), &["Davi, respira fundo e come algo gostoso", "Anotei: há pendências leves para amanhã", "Que tal uma pausa de dez minutos",
            "Posso lembrar de ligar para o Dudu", "quinta fala que nao deve existir"]);
        assert!(n == 4 && falas_de(&d)[1..].iter().map(|f| f.0.as_str()).collect::<Vec<_>>() == [ALMA, JARVIS, ALMA, JARVIS]);
        assert!(!vistos[0].contains("[FIM]") && !vistos[1].contains("[FIM]") && vistos[2].contains("[FIM]") && vistos[1].starts_with(CONTINUE)); // as duas primeiras sempre se respondem
        let d = dir("fim");
        assert_eq!(roda(&d, &mut Sala::default(), &["FIM"]).0, 0);
        assert_eq!(roda(&d, &mut Sala::default(), &["Boa noite, Davi. Estou aqui por você. [FIM]", "[FIM]"]).0, 1);
        assert_eq!(falas_de(&d), [(ALMA.to_string(), "Boa noite, Davi. Estou aqui por você.".to_string())]); // o marcador some do texto
        // o segundo agente que diz FIM cedo demais e chamado de novo e responde
        let d = dir("segundo");
        let (n, vistos) = roda(&d, &mut Sala::default(), &["Davi, estou aqui com você, vamos cuidar de um passo de cada vez.", "[FIM]", "Posso organizar as pendências de hoje para aliviar o seu dia.", "[FIM]"]);
        assert!(n == 2 && falas_de(&d).iter().map(|f| f.0.as_str()).collect::<Vec<_>>() == [ALMA, JARVIS] && vistos[2].contains("Não escreva FIM") && tem_aviso(&d, "nada novo"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn repeticao_pausa_silencio_teto_modelo_fora_e_modo_jogo() {
        let d = dir("travas");
        let (n, _) = roda(&d, &mut Sala::default(), &["Entendido, vou organizar a rotina do Davi hoje cedo", "Entendido, vou organizar a rotina do Davi hoje cedo também"]);
        assert!(n == 1 && tem_aviso(&d, "repetir"));
        assert_eq!(roda(&d, &mut Sala { pausada: true, ..Default::default() }, &["fala que nao sai nunca porque esta pausada"]).0, 0);
        let d = dir("silencio");
        let mut s = Sala::default();
        acao(&d, &mut s, &json!({ "acao": "silenciar", "agente": ALMA }));
        acao(&d, &mut s, &json!({ "acao": "silenciar", "agente": "Fantasma" }));
        assert_eq!(s.silenciados, [ALMA]);
        roda(&d, &mut s, &["Fala do Jarvis com conteudo proprio e novo aqui."]);
        assert_eq!(falas_de(&d).iter().map(|f| f.0.as_str()).collect::<Vec<_>>(), [JARVIS]);
        acao(&d, &mut s, &json!({ "acao": "silenciar", "agente": ALMA, "ligado": false }));
        acao(&d, &mut s, &json!({ "acao": "pausar" }));
        assert!(s.silenciados.is_empty() && s.pausada);
        acao(&d, &mut s, &json!({ "acao": "pausar", "ligado": false }));
        // teto diario: 60 falas dos agentes hoje; amanha zera
        for _ in 0..59 {
            registrar(&d, &agora(), JARVIS, "fala", "fala");
        }
        registrar(&d, &agora(), "Davi", "a fala do dono nao conta", "fala");
        assert_eq!(falas_hoje(&d, &agora()), 60);
        assert_eq!(roda(&d, &mut s, &["Mais uma fala qualquer sobre outro assunto totalmente diferente."]).0, 0);
        let amanha = Agora { data: Data(agora().data.0 + 1), ..agora() };
        assert!(falas_hoje(&d, &amanha) == 0 && estado(&d, &agora(), &s, false)["falas_hoje"] == 60);
        acao(&d, &mut s, &json!({ "acao": "limpar" }));
        assert!(mensagens(&d, 0, 10).is_empty());
        // modelo fora do ar vira aviso, sem quebrar
        assert_eq!(rodada(&d, &agora(), &mut s, &gente(), "g", &|_, _, _| Err("recusada".into()), 4), 0);
        assert!(tem_aviso(&d, "IA local indisponível") && s.erro.contains("recusada"));
        // modo jogo: a sala nao chama o modelo, mas a resposta de crise sai
        power::desligar(&d, "jogo", "");
        assert_eq!(rodada(&d, &agora(), &mut s, &gente(), "g", &|_, _, _| panic!("nao deveria chamar o modelo"), 4), 0);
        assert!(tem_aviso(&d, "Modo jogo") && estado(&d, &agora(), &s, false)["modo_jogo"] == true);
        assert!(dono_fala(&d, &agora(), &gente(), "eu não aguento mais").is_none() && falas_de(&d).iter().any(|f| f.0 == ALMA && f.1.contains("188")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn sofrimento_forte_recebe_resposta_fixa_e_ninguem_mais_fala() {
        for frase in ["Eu to muito cansado, to me sentindo um lixo, não to aguentando o peso da vida mais", "quero morrer", "não aguento mais"] {
            let d = dir("crise");
            assert!(dono_fala(&d, &agora(), &gente(), frase).is_none());
            let f = falas_de(&d);
            assert!(f.len() == 2 && f[0].0 == "Davi" && f[1].0 == ALMA, "{frase}");
            assert!(f[1].1.contains("pensamentos de se machucar") && f[1].1.contains("188") && f[1].1.contains("192") && f[1].1.contains("a Bia") && f[1].1.starts_with("Davi,"));
            let _ = std::fs::remove_dir_all(&d);
        }
        assert!(!crise("estou cansado hoje, trabalhei muito") && !crise("o trabalho tá pesado esta semana")); // cansaco comum nao dispara
        let d = dir("normal");
        assert_eq!(dono_fala(&d, &agora(), &gente(), " oi ").as_deref(), Some("Davi acabou de escrever na sala. Responda a ele."));
        assert!(dono_fala(&d, &agora(), &gente(), "  ").is_none() && falas_de(&d).len() == 1);
        assert!(iniciar_conversa(&d, &agora(), &gente(), "").contains("Como Davi está") && tem_aviso(&d, "Tema da conversa"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn personas_tem_os_limites_e_proposta_so_vira_texto() {
        let (d, g) = (dir("persona"), gente());
        let p = persona_base(ALMA, &g);
        for trecho in ["empática", "carinhosa", "afetuosa", "cuidadosa", "o Dudu (10 anos)", "CVV 188", "NÃO é médica", "dependência", "não dorme", "terceira pessoa", "não está na sala"] {
            assert!(p.contains(trecho), "{trecho}");
        }
        for nome in ORDEM {
            assert!(persona_base(nome, &g).contains("REALMENTE PODEM FAZER") && persona_base(nome, &g).contains("NÃO podem cozinhar"));
        }
        assert!(!persona_base(ALMA, &Gente { dono: "Davi".into(), filho: String::new(), parceira: String::new() }).contains("filho"));
        // so a Alma evolui, e o nucleo fixo fecha a persona dela
        alma_evolucao::aplicar(&d, &agora(), "Chame o dono de forma carinhosa.", "ok", "Jarvis").unwrap();
        let pa = persona_de(&d, ALMA, &g);
        assert!(pa.contains("Chame o dono de forma carinhosa.") && pa.ends_with(alma_evolucao::NUCLEO_FIXO) && persona_de(&d, JARVIS, &g) == persona_base(JARVIS, &g));
        let nome = propor_melhoria(&d, &agora(), &g, "deixar o relatório mais curto", &|s, _, _| if s.contains("JARVIS") { Err("caiu".into()) } else { Ok("Sugestão: encurtar.".into()) }).unwrap();
        let t = std::fs::read_to_string(d.join("propostas").join(&nome)).unwrap();
        assert!(t.contains("AGUARDANDO APROVAÇÃO") && t.contains("nada foi aplicado") && t.contains("## A.L.M.A.\nSugestão: encurtar.") && t.contains("## Jarvis\n(sem resposta: caiu)"));
        assert!(listar_propostas(&d) == [nome] && propor_melhoria(&d, &agora(), &g, " ", &|_, _, _| panic!("sem tema nao chama")).is_none());
        assert!(parecido("vou organizar a rotina hoje", "vou organizar a rotina hoje cedo") && !parecido("oi", "oi") && !parecido("pausa para agua", "pendencias de amanha"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
