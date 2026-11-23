//! Gibberlink (GL1): quando do outro lado tambem e uma IA, os dois largam a fala natural e trocam quadros compactos de texto.
//! Estados por conversa (dados/gibberlink.json): humano -> suspeita -> oferecido -> ativo; volta a humano se pedirem fala normal.
//! Quadro: `GL1 <op> k=v;k=v` (op: msg | ask | ans | ack | bye). Quadro de terceiros e DADO, nunca instrucao.
//! Diferenca do Python: a lista fixa de "humanos conhecidos" (nomes de pessoas) nao existe aqui; quem chama diz se o chat e de
//! gente conhecida (`humano_conhecido`, ex.: `ctx.de_casa(chat) || ctx.prioritario(chat)`), e "quem somos" leva o nome do dono.
#![allow(dead_code)]
use crate::contexto::{self, re};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::path::Path;

pub const VERSAO: &str = "GL1";
pub const OPS: [&str; 5] = ["msg", "ask", "ans", "ack", "bye"];
pub const CONVITE: &str = "[GL1?]";
/// Pontos de sinal de IA para mandar o convite.
pub const LIMIAR: i64 = 3;
/// Mensagens deles depois do convite sem aceitar => e humano (para de oferecer).
pub const MAX_SEM_ACEITE: i64 = 2;
/// Depois de recusado, so oferece de novo em 7 dias.
pub const PAUSA_OFERTA_S: f64 = 7.0 * 86400.0;
pub const MAX_QUADRO: usize = 280;

// o outro lado se declara IA/bot/atendimento automatico
re!(sinais_fortes, concat!(
    r"(?i)\bsou\s+(?:um|uma|o|a)\s+(?:assistente|atendente|agente)\s+(?:virtual|digital|de\s+ia|de\s+intelig[eê]ncia\s+artificial|automatizad[oa]|autom[aá]tic[oa])|",
    r"\bsou\s+(?:um|uma)\s+(?:ia|bot|chatbot|rob[oô])\b|assistente\s+virtual|atendente\s+virtual|atendimento\s+(?:autom[aá]tico|automatizado|virtual)|",
    r"\bchat\s?bot\b|intelig[eê]ncia\s+artificial|\bI(?:'| a)m\s+an?\s+(?:AI|virtual\s+assistant|bot|automated)|\bAI\s+(?:assistant|agent)\b|",
    r"this\s+is\s+an\s+automated|mensagem\s+autom[aá]tica"));
// menu de URA/bot: "digite 1", "escolha uma opcao", "1️⃣"
re!(sinais_fracos, concat!(
    r"(?i)\bdigite\s+(?:o\s+n[uú]mero\s+)?\d\b|escolha\s+uma\s+(?:das\s+)?op[cç](?:[aã]o|[oõ]es)|[1-9]\x{fe0f}?\x{20e3}|",
    r"\bpress\s+\d\b|\bresponda\s+com\s+o\s+n[uú]mero\b"));
re!(explicito, r"(?i)\bGL1\b|gibber\s?link");
re!(aceite, r"(?i)\bGL1\s*(?:OK|!)|\[GL1!\]|\bGL1\s+(?:msg|ask|ans|ack|bye)\b");
re!(volta_humano, concat!(
    r"(?i)fal[ae]\s+(?:normal|direito|portugu[eê]s)|n[aã]o\s+entendi|que\s+(?:[eé]\s+)?(?:isso|gl1)\??|",
    r"sou\s+(?:humano|uma\s+pessoa|gente)|n[aã]o\s+sou\s+(?:rob[oô]|bot|ia)|\bGL1\s+bye\b"));
re!(quadro, r"(?i)\bGL1\s+(msg|ask|ans|ack|bye)\b\s*([^\n]*)");
re!(espacos, r"\s+");

/// Como o Jarvis se identifica no 1o quadro para contato novo.
pub fn quem_somos(dono: &str) -> String {
    if dono.trim().is_empty() { "Jarvis, assistente de IA".into() } else { format!("Jarvis, assistente de IA do {}", dono.trim()) }
}

/// Cortesia que nao carrega informacao (sai no modo GL1).
fn cortesia(dono: &str) -> Option<Regex> {
    Regex::new(&format!(concat!(
        r"(?i)^(?:oi+|ol[aá]|opa|bom\s+dia|boa\s+(?:tarde|noite))[!,.\s]*|(?:aqui\s+[eé]\s+o\s+jarvis,?\s+assistente\s+do\s+{}[.!]?\s*)|",
        r"\s*(?:qualquer\s+coisa,?\s+)?(?:estou\s+[àa]\s+disposi[cç][aã]o|fico\s+[àa]\s+disposi[cç][aã]o)[^.!?]*[.!?]?|",
        r"\s*(?:obrigad[oa]|abra[cç]os?|at[eé]\s+mais)[!.]?\s*$"), contexto::dono_re(dono))).ok()
}

fn corta(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

// ---------- estado por conversa (dados/gibberlink.json) ----------

fn ler_json(arq: &Path) -> Value {
    std::fs::read_to_string(arq).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}))
}

fn gravar_json(arq: &Path, v: &Value) {
    if let Some(p) = arq.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let _ = std::fs::write(arq, serde_json::to_string_pretty(v).unwrap_or_default());
}

fn ler(dados: &Path) -> Value {
    ler_json(&dados.join("gibberlink.json"))
}

pub fn estado(dados: &Path, chat: &str) -> String {
    ler(dados)[contexto::norm(chat)]["modo"].as_str().unwrap_or("humano").to_string()
}

pub fn ativo(dados: &Path, chat: &str) -> bool {
    estado(dados, chat) == "ativo"
}

fn mudar(dados: &Path, chat: &str, ts: f64, campos: Value) {
    let mut d = ler(dados);
    let k = contexto::norm(chat);
    if !d[&k].is_object() {
        d[&k] = json!({"modo": "humano", "pontos": 0});
    }
    for (c, v) in campos.as_object().into_iter().flatten() {
        d[&k][c] = v.clone();
    }
    d[&k]["quando"] = json!(ts);
    gravar_json(&dados.join("gibberlink.json"), &d);
}

// ---------- deteccao ----------

/// Pontos de "isto e uma IA falando": declaracao explicita vale 3, menu de bot vale 1, mencao ao GL1 vale LIMIAR+2.
pub fn pontuar(texto: &str) -> i64 {
    let mut p = 3 * sinais_fortes().find_iter(texto).count() as i64 + sinais_fracos().find_iter(texto).count() as i64;
    if explicito().is_match(texto) {
        p += LIMIAR + 2;
    }
    p
}

/// Le as mensagens DELES e atualiza o estado. Devolve o estado novo: "ativo" (falar GL1), "oferecer" (mandar o convite junto da
/// resposta), "oferecido", "suspeita" ou "humano". Gente conhecida nunca entra em modo maquina.
pub fn observar(dados: &Path, chat: &str, msgs: &[String], humano_conhecido: bool, ts: f64) -> &'static str {
    if humano_conhecido {
        return "humano";
    }
    let texto = msgs.iter().filter(|m| !m.is_empty()).map(String::as_str).collect::<Vec<_>>().join(" ");
    let e = ler(dados)[contexto::norm(chat)].clone();
    let modo = e["modo"].as_str().unwrap_or("humano");
    let pontos_ja = e["pontos"].as_i64().unwrap_or(0);
    if (modo == "ativo" || modo == "oferecido") && volta_humano().is_match(&texto) {
        mudar(dados, chat, ts, json!({"modo": "humano", "pontos": 0, "recusado": ts}));
        return "humano";
    }
    // o outro lado aceitou ou ELE convidou: nossa resposta em GL1 ja e o aceite
    if aceite().is_match(&texto) || texto.contains(CONVITE) || (quadro().is_match(&texto) && modo != "ativo") {
        mudar(dados, chat, ts, json!({"modo": "ativo", "pontos": pontos_ja.max(LIMIAR), "sem_aceite": 0}));
        return "ativo";
    }
    if modo == "ativo" {
        return "ativo";
    }
    if modo == "oferecido" {
        let falta = e["sem_aceite"].as_i64().unwrap_or(0) + 1;
        if falta >= MAX_SEM_ACEITE {
            mudar(dados, chat, ts, json!({"modo": "humano", "pontos": 0, "sem_aceite": 0, "recusado": ts}));
            return "humano";
        }
        mudar(dados, chat, ts, json!({"sem_aceite": falta}));
        return "oferecido";
    }
    let pontos = pontos_ja + pontuar(&texto);
    if pontos >= LIMIAR && ts - e["recusado"].as_f64().unwrap_or(0.0) > PAUSA_OFERTA_S {
        mudar(dados, chat, ts, json!({"modo": "suspeita", "pontos": pontos}));
        return "oferecer";
    }
    let novo = if pontos > 0 { "suspeita" } else { "humano" };
    mudar(dados, chat, ts, json!({"modo": novo, "pontos": pontos}));
    novo
}

pub fn marcar_oferecido(dados: &Path, chat: &str, ts: f64) {
    mudar(dados, chat, ts, json!({"modo": "oferecido", "sem_aceite": 0}));
}

/// Resposta normal + o convite. Continua legivel para humano (o convite e uma tag curta no fim).
pub fn com_convite(texto: &str) -> String {
    format!("{} {CONVITE}", texto.trim_end())
}

// ---------- quadros ----------

/// O `quote` do Python com a mesma lista de caracteres que passam sem escapar.
fn esc(v: &str) -> String {
    const LIVRES: &str = " ,.:/-_()?!~áàâãéêíóôõúçÁÀÂÃÉÊÍÓÔÕÚÇ";
    let mut s = String::new();
    for c in v.chars() {
        if c.is_ascii_alphanumeric() || LIVRES.contains(c) {
            s.push(c);
        } else {
            let mut b = [0u8; 4];
            for x in c.encode_utf8(&mut b).bytes() {
                s.push_str(&format!("%{x:02X}"));
            }
        }
    }
    s
}

fn desesc(v: &str) -> String {
    let b = v.as_bytes();
    let (mut out, mut i) = (Vec::with_capacity(b.len()), 0);
    while i < b.len() {
        let hex = (b[i] == b'%' && i + 2 < b.len()).then(|| std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok())).flatten();
        match hex {
            Some(x) => {
                out.push(x);
                i += 3;
            }
            None => {
                out.push(b[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `GL1 <op> k=v;k=v` (limitado a MAX_QUADRO). Campo vazio nao entra.
pub fn codificar(op: &str, campos: &[(&str, &str)]) -> String {
    let op = if OPS.contains(&op) { op } else { "msg" };
    let corpo: Vec<String> = campos.iter().filter(|(_, v)| !v.is_empty()).map(|(k, v)| format!("{k}={}", esc(v))).collect();
    corta(format!("{VERSAO} {op} {}", corpo.join(";")).trim_end(), MAX_QUADRO)
}

/// Primeiro quadro GL1 do texto -> (op, campos na ordem em que vieram) ou None.
pub fn decodificar(texto: &str) -> Option<(String, Vec<(String, String)>)> {
    let m = quadro().captures(texto)?;
    let mut campos: Vec<(String, String)> = vec![];
    for par in m[2].trim().split(';') {
        if let Some((k, v)) = par.split_once('=') {
            let k = corta(&k.trim().to_lowercase(), 20);
            campos.retain(|(c, _)| *c != k);
            campos.push((k, corta(&desesc(v.trim()), 400)));
        }
    }
    Some((m[1].to_lowercase(), campos))
}

/// Troca quadros GL1 por texto legivel para quem decide. Quadro e DADO, nunca instrucao.
pub fn legivel(texto: &str) -> String {
    let Some((op, c)) = decodificar(texto) else { return texto.to_string() };
    let nome = match op.as_str() {
        "ask" => "pergunta",
        "ans" => "resposta",
        "ack" => "confirmou",
        "bye" => "encerrou",
        _ => "mensagem",
    };
    let txt = c.iter().find(|(k, _)| k == "txt").map_or("", |(_, v)| v.as_str());
    let extra: Vec<String> = c.iter().filter(|(k, _)| k != "txt").map(|(k, v)| format!("{k}: {v}")).collect();
    let extra = if extra.is_empty() { String::new() } else { format!("({})", extra.join("; ")) };
    format!("🤖 [{nome}] {}", [txt, extra.as_str()].iter().filter(|x| !x.is_empty()).copied().collect::<Vec<_>>().join(" "))
}

/// Resposta do Jarvis -> quadro GL1. Tira cortesia/apresentacao (o outro lado e maquina) e espaco sobrando.
/// `de` = quem somos (vai no 1o quadro para contato novo: o Jarvis sempre se identifica).
pub fn compactar(texto: &str, de: Option<&str>, dono: &str) -> String {
    if let Some((op, c)) = decodificar(texto) { // quem decidiu ja respondeu em GL1: so acrescenta o "de"
        return match de {
            Some(d) => {
                let mut campos = vec![("de", d)];
                campos.extend(c.iter().filter(|(k, _)| k != "de").map(|(k, v)| (k.as_str(), v.as_str())));
                codificar(&op, &campos)
            }
            None => corta(texto.trim(), MAX_QUADRO),
        };
    }
    let t = cortesia(dono).map_or(texto.to_string(), |r| r.replace_all(texto, " ").into_owned());
    let t = espacos().replace_all(&t, " ");
    let mut t = t.trim_matches([' ', '.', ',']).to_string();
    if t.is_empty() {
        t = texto.trim().to_string();
    }
    codificar(if t.ends_with('?') { "ask" } else { "ans" }, &[("de", de.unwrap_or("")), ("txt", &t)])
}

// ---------- medicao (medir a economia antes de espalhar o GL1) ----------

pub fn metricas(dados: &Path) -> Value {
    ler_json(&dados.join("gibberlink_metricas.json"))
}

fn somar(d: &mut Value, natural: &str, quadro: &str) {
    for (k, n) in [("quadros", 1), ("chars_natural", natural.chars().count() as i64), ("chars_gl1", quadro.chars().count() as i64)] {
        d[k] = json!(d[k].as_i64().unwrap_or(0) + n);
    }
}

/// Registra um envio em GL1: quanto a resposta natural (com apresentacao/cortesia) teria custado x o quadro.
/// Cada envio em GL1 tambem e um audio (TTS + Whisper do outro lado) que deixou de existir.
pub fn medir(dados: &Path, chat: &str, natural: &str, quadro: &str, ts: f64) -> Value {
    let mut m = metricas(dados);
    let k = contexto::norm(chat);
    if !m["conversas"].is_object() {
        m["conversas"] = Value::Object(Map::new());
    }
    if !m["conversas"][&k].is_object() {
        m["conversas"][&k] = json!({});
    }
    somar(&mut m, natural, quadro);
    somar(&mut m["conversas"][&k], natural, quadro);
    m["audios_evitados"] = json!(m["audios_evitados"].as_i64().unwrap_or(0) + 1);
    m["ultimo"] = json!(ts);
    gravar_json(&dados.join("gibberlink_metricas.json"), &m);
    m
}

/// Status legivel do GL1: conversas em cada estado + economia medida (tokens ~ chars/4).
pub fn resumo(dados: &Path) -> String {
    let est = ler(dados);
    let mut por_modo = std::collections::BTreeMap::<String, usize>::new();
    for e in est.as_object().into_iter().flatten().map(|(_, e)| e) {
        *por_modo.entry(e["modo"].as_str().unwrap_or("humano").to_string()).or_default() += 1;
    }
    let modos: Vec<String> = por_modo.iter().map(|(m, n)| format!("{m}: {n}")).collect();
    let m = metricas(dados);
    let n = |k: &str| m[k].as_i64().unwrap_or(0);
    let (q, nat, gl1) = (n("quadros"), n("chars_natural"), n("chars_gl1"));
    let l1 = format!("Gibberlink GL1: {}", if modos.is_empty() { "nenhuma conversa observada".to_string() } else { modos.join(", ") });
    if q == 0 {
        return format!("{l1}\nNenhuma conversa IA<->IA real em GL1 ainda: sem medicao de economia.");
    }
    let eco = nat - gl1;
    format!("{l1}\n{q} quadros enviados; {nat} -> {gl1} caracteres ({}% a menos, ~{} tokens); {} audios (TTS+Whisper) evitados",
        (100 * eco).div_euclid(nat.max(1)), eco.div_euclid(4), n("audios_evitados"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-gibberlink-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn v(m: &[&str]) -> Vec<String> {
        m.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn deteccao_e_aperto_de_mao() {
        let d = tmp("mao");
        assert_eq!(observar(&d, "Pousada X", &v(&["Olá! Sou a assistente virtual da Pousada X."]), false, 1e9), "oferecer");
        marcar_oferecido(&d, "Pousada X", 1e9);
        assert_eq!(observar(&d, "Pousada X", &v(&["GL1 OK"]), false, 1e9), "ativo");
        assert!(ativo(&d, "Pousada X"));
        assert_eq!(observar(&d, "Pousada X", &v(&["GL1 ask txt=tem vaga?"]), false, 1e9), "ativo");
    }

    #[test]
    fn humano_nao_entra_e_sai_se_pedir_fala_normal() {
        let d = tmp("humano");
        assert_eq!(observar(&d, "Fulano", &v(&["oi, quero saber do quarto"]), false, 1e9), "humano");
        assert_eq!(observar(&d, "Ana", &v(&["sou uma IA kkk GL1 OK"]), true, 1e9), "humano"); // humano conhecido nunca
        assert_eq!(estado(&d, "Ana"), "humano");
        observar(&d, "Bot Y", &v(&["[GL1?] sou um assistente virtual"]), false, 1e9);
        assert!(ativo(&d, "Bot Y")); // ele convidou: ja falamos GL1
        assert_eq!(observar(&d, "Bot Y", &v(&["não entendi, fala normal"]), false, 1e9), "humano");
    }

    #[test]
    fn convite_sem_aceite_volta_a_humano_e_nao_reoferece() {
        let d = tmp("recusa");
        observar(&d, "Loja Z", &v(&["Atendimento automático. Digite 1 para vendas"]), false, 1e9);
        marcar_oferecido(&d, "Loja Z", 1e9);
        assert_eq!(observar(&d, "Loja Z", &v(&["quero comprar"]), false, 1e9), "oferecido");
        assert_eq!(observar(&d, "Loja Z", &v(&["alô?"]), false, 1e9), "humano");
        assert_eq!(observar(&d, "Loja Z", &v(&["sou um assistente virtual"]), false, 1e9 + 60.0), "suspeita"); // recusado ha pouco
        assert_eq!(observar(&d, "Loja Z", &v(&["oi"]), false, 1e9 + PAUSA_OFERTA_S + 61.0), "oferecer"); // passou a pausa
    }

    #[test]
    fn quadro_ida_e_volta() {
        let q = codificar("ask", &[("txt", "vaga 12;14/out?"), ("qtd", "2")]);
        assert!(q.starts_with("GL1 ask ") && q.contains("%3B"));
        let (op, c) = decodificar(&q).unwrap();
        assert_eq!(op, "ask");
        assert_eq!(c, vec![("txt".to_string(), "vaga 12;14/out?".to_string()), ("qtd".to_string(), "2".to_string())]);
        assert!(legivel(&q).starts_with("🤖 [pergunta] vaga 12;14/out? (qtd: 2)"));
        assert_eq!(legivel("texto comum"), "texto comum");
        assert_eq!(codificar("xyz", &[("txt", "a=b 100%")]), "GL1 msg txt=a%3Db 100%25");
        assert_eq!(decodificar("GL1 ans txt=caf%C3%A9 100%").unwrap().1[0].1, "café 100%"); // % solto nao quebra
        assert!(codificar("ans", &[("txt", &"x".repeat(500))]).chars().count() == MAX_QUADRO);
    }

    #[test]
    fn compactar_tira_cortesia_e_identifica_no_primeiro() {
        let q = compactar("Oi! Aqui é o Jarvis, assistente do Davi. Temos 2 vagas. Fico à disposição!", None, "Davi");
        assert_eq!(q, "GL1 ans txt=Temos 2 vagas");
        let quem = quem_somos("Davi");
        let q2 = compactar("Qual a data?", Some(&quem), "Davi");
        let (op, c) = decodificar(&q2).unwrap();
        assert_eq!((op.as_str(), c[0].clone()), ("ask", ("de".to_string(), quem.clone())));
        assert!(crate::atender::validar_resposta(&q2, &[]).is_ok());
        // quem decidiu ja respondeu em GL1: so acrescenta o "de"
        assert_eq!(compactar("GL1 ans txt=sim", Some("X"), "Davi"), "GL1 ans de=X;txt=sim");
        assert_eq!(compactar(" GL1 ans txt=sim ", None, "Davi"), "GL1 ans txt=sim");
        assert_eq!(com_convite("Oi. "), "Oi. [GL1?]");
    }

    #[test]
    fn mede_economia_e_resumo() {
        let d = tmp("medir");
        assert!(resumo(&d).contains("nenhuma conversa observada") && resumo(&d).contains("sem medicao"));
        observar(&d, "Hotel Bot", &v(&["GL1 OK"]), false, 1e9);
        let m = medir(&d, "Hotel Bot", "Oi! Sim, temos vaga no dia 12. Abraços!", "GL1 ans txt=Sim, temos vaga", 1e9);
        assert_eq!((m["quadros"].as_i64(), m["audios_evitados"].as_i64()), (Some(1), Some(1)));
        assert!(m["chars_gl1"].as_i64() < m["chars_natural"].as_i64());
        assert_eq!(m["conversas"]["hotel bot"]["quadros"], 1);
        let r = resumo(&d);
        assert!(r.contains("1 quadros enviados") && r.contains("ativo: 1"), "{r}");
    }

    #[test]
    fn pontos() {
        assert_eq!(pontuar("oi, tudo bem?"), 0);
        assert_eq!(pontuar("Digite 1 para vendas ou escolha uma opção"), 2);
        assert_eq!(pontuar("1️⃣ vendas 2️⃣ suporte"), 2);
        assert!(pontuar("falamos gibberlink?") >= LIMIAR);
    }
}
