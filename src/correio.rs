//! Correio: olha SO os metadados da caixa de entrada (id, nome, e-mail, assunto, lida ou nao) e avisa de e-mail de pessoa ou de possivel
//! oferta de trabalho. NUNCA o corpo: `verificar` nem le um campo de corpo, mesmo que venha. Estado em dados/correio.json, como no Python.
//! Aviso na hora so do que e extremamente importante (no maximo 2 por rodada); o resto fica no relato da manha. Oferta de trabalho tambem
//! entra na fila do Nando (`candidaturas::receber`). A primeira rodada so marca a caixa como vista, para nao inundar ninguem.
//! Ficou de fora: ler a caixa (chega pronta em `caixa`), o envio (closure) e o recado de status no barramento (fica para quem chama).
#![allow(dead_code)]
use crate::candidaturas;
use crate::empresa_dev::{corta, ler, rx};
use regex::Regex;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

const CATEGORIAS_RUINS: [&str; 5] = ["CATEGORY_PROMOTIONS", "CATEGORY_SOCIAL", "CATEGORY_UPDATES", "CATEGORY_FORUMS", "SPAM"];
const MAX_AVISOS: usize = 2;

fn robo(end: &str) -> bool {
    static R: OnceLock<Regex> = OnceLock::new();
    rx(&R, r"(?i)no-?reply|do-?not-?reply|nao-?responda|notific|newsletter|mailer|marketing|news@|info@|contato@|suporte@|support@|alerts?@|updates?@|@(?:.+\.)?(?:facebookmail|linkedin|google|youtube|amazon|mercadolivre|nubank|ifood|airbnb|samsung|spotify|steampowered|epicgames|discord)\.").is_match(end)
}

/// So o que e extremamente importante vira aviso na hora.
pub fn importante(assunto: &str) -> bool {
    static R: OnceLock<Regex> = OnceLock::new();
    rx(&R, r"(?i)urgente|convoca|entrevista|aprovad|contrata|proposta de emprego|ex[eé]rcito|11.?rm|\bSTT\b|intima|cita[cç][aã]o|div[oó]rcio|cart[oó]rio|prazo (?:final|hoje|amanh)|[uú]ltimo dia|vence hoje|bloquead|invadid|acesso n[aã]o autorizado").is_match(assunto)
}

/// "pessoa" | "trabalho" | "". Pura. `cabecalhos` com chaves em minusculas.
pub fn classificar(nome: &str, end: &str, assunto: &str, cabecalhos: &BTreeMap<String, String>, labels: &[&str]) -> &'static str {
    static T: OnceLock<Regex> = OnceLock::new();
    static U: OnceLock<Regex> = OnceLock::new();
    static A: OnceLock<Regex> = OnceLock::new();
    let trabalho = rx(&T, r"(?i)\b(?:vaga|vagas|emprego|contrata[cç][ãa]o|processo\s+seletivo|entrevista|oportunidade|proposta|freelance|freela|job|hiring|position|recrutador|recruiter|candidatura|curr[ií]culo|projeto|or[çc]amento|contrato)\b");
    let ruido = rx(&U, r"(?i)\b(?:unsubscribe|descadastr|newsletter|promo[cç][ãa]o|cupom|desconto|black\s*friday|oferta)\b");
    if labels.contains(&"SPAM") || labels.contains(&"CATEGORY_PROMOTIONS") {
        return "";
    }
    if trabalho.is_match(assunto) && !ruido.is_match(assunto) {
        return "trabalho";
    }
    if labels.iter().any(|l| CATEGORIAS_RUINS.contains(l)) {
        return "";
    }
    let cab = |k: &str| cabecalhos.get(k).map_or("", String::as_str);
    let auto = cabecalhos.get("auto-submitted").map_or("no", String::as_str).to_lowercase();
    let automatico = !cab("list-unsubscribe").is_empty() || rx(&A, r"(?i)bulk|list|junk").is_match(cab("precedence")) || !["", "no"].contains(&auto.as_str());
    if !nome.is_empty() && !automatico && !robo(end) { "pessoa" } else { "" }
}

/// Confere a caixa ja lida (so metadados: {id, nome, email, assunto, nao_lida}) e devolve quantos avisos mandou.
pub fn verificar(dados: &Path, caixa: &[Value], ts: f64, enviar: &dyn Fn(&str)) -> Result<usize, String> {
    let arq = dados.join("correio.json");
    let mut est = ler(&arq).filter(Value::is_object).unwrap_or_else(|| json!({}));
    let primeira = est["fonte"] != "web"; // ids de outra fonte nao servem: marca a caixa atual como vista
    let mut vistos: Vec<String> = if primeira { vec![] } else { est["vistos"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect() };
    let mut relato = est["relato"].as_array().cloned().unwrap_or_default();
    let mut avisos = 0;
    let s = |m: &Value, k: &str| m[k].as_str().unwrap_or("").to_string();
    for m in caixa {
        let id = m["id"].as_str().map_or_else(|| m["id"].to_string(), String::from);
        if vistos.contains(&id) {
            continue;
        }
        vistos.push(id);
        if primeira || m["nao_lida"] != true {
            continue;
        }
        let (nome, end) = (s(m, "nome").trim().to_string(), s(m, "email"));
        let assunto = corta(if s(m, "assunto").is_empty() { "(sem assunto)" } else { m["assunto"].as_str().unwrap_or("") }, 90);
        let tipo = classificar(&nome, &end, &assunto, &BTreeMap::new(), &[]);
        if tipo.is_empty() || avisos >= MAX_AVISOS {
            continue;
        }
        let quem = corta(if !nome.is_empty() { &nome } else if !end.is_empty() { &end } else { "?" }, 40);
        if tipo == "trabalho" {
            let _ = candidaturas::receber(dados, "Correio", &assunto, &quem, "", "", ts); // vaga vai para a triagem do Nando; o Correio continua so lendo
        }
        relato.push(json!({ "tipo": tipo, "quem": quem, "assunto": assunto, "ts": ts })); // entra na reuniao da manha
        if !importante(&assunto) {
            continue;
        }
        enviar(&if tipo == "pessoa" { format!("📬 E-mail de pessoa: {quem} — {assunto}") } else { format!("💼 Possível oferta de trabalho no e-mail: {quem} — {assunto}") });
        avisos += 1;
    }
    est["vistos"] = json!(vistos[vistos.len().saturating_sub(500)..]);
    est["relato"] = json!(relato);
    est["ts"] = json!(ts);
    est["fonte"] = json!("web");
    std::fs::create_dir_all(dados).and_then(|_| std::fs::write(&arq, est.to_string())).map_err(|e| format!("não gravei o estado do correio: {e}"))?;
    Ok(avisos)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn classifica_email() {
        let cab = |k: &str, v: &str| BTreeMap::from([(k.to_string(), v.to_string())]);
        let vazio = BTreeMap::new();
        assert_eq!(classificar("Ana Souza", "ana@exemplo.test", "Oi, tudo bem?", &vazio, &["INBOX"]), "pessoa");
        assert_eq!(classificar("Loja X", "no-reply@loja.test", "Seu pedido", &vazio, &["INBOX"]), "");
        assert_eq!(classificar("Ana", "ana@x.test", "Oi", &cab("list-unsubscribe", "<x>"), &[]), "");
        assert_eq!(classificar("Recrutador", "rh@empresa.test", "Vaga de suporte remoto", &cab("list-unsubscribe", "<x>"), &["INBOX"]), "trabalho");
        assert_eq!(classificar("Cupom", "a@b.test", "Vaga com desconto black friday", &vazio, &["CATEGORY_PROMOTIONS"]), "");
        assert_eq!(classificar("Rede", "ana@x.test", "Oi", &vazio, &["CATEGORY_SOCIAL"]), "");
        assert_eq!(classificar("Ana", "ana@x.test", "Oi", &cab("precedence", "bulk"), &[]), "");
        assert_eq!(classificar("Ana", "ana@x.test", "Oi", &cab("auto-submitted", "auto-generated"), &[]), "");
        assert_eq!(classificar("", "ana@x.test", "Oi", &vazio, &[]), "");
        assert_eq!(classificar("Bia", "bia@mail.linkedin.com", "Oi", &vazio, &[]), "");
        assert_eq!(classificar("Bia", "bia@x.test", "Vaga com cupom", &vazio, &[]), "pessoa"); // oferta com ruido nao e trabalho
    }

    #[test]
    fn primeira_rodada_so_marca_depois_avisa_so_o_importante_e_nunca_le_corpo() {
        let d = std::env::temp_dir().join(format!("braniac-correio-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let avisos = RefCell::new(vec![]);
        let enviar = |m: &str| avisos.borrow_mut().push(m.to_string());
        let m = |id: &str, nome: &str, assunto: &str, nao_lida: bool| json!({ "id": id, "nome": nome, "email": format!("{id}@exemplo.test"), "assunto": assunto, "nao_lida": nao_lida, "corpo": "SEGREDO-DO-CORPO" });
        let velha = vec![m("v1", "Ana", "URGENTE: convocação", true)];
        assert_eq!(verificar(&d, &velha, 1.0, &enviar).unwrap(), 0); // primeira rodada: nada de aviso
        let caixa = vec![velha[0].clone(), m("n1", "Bia", "Oi, tudo bem?", true), m("n2", "Caio", "URGENTE: entrevista amanhã", true), m("n3", "Duda", "Convocação para a prova", false),
            m("n4", "Eva", "Vaga de suporte remoto", true), m("n5", "Loja", "Vaga urgente com cupom de desconto", true), m("n6", "Gil", "Resultado: aprovado", true), m("n7", "Hugo", "Prazo final hoje", true)];
        assert_eq!(verificar(&d, &caixa, 2.0, &enviar).unwrap(), 2);
        assert_eq!(avisos.borrow().clone(), vec!["💼 Possível oferta de trabalho no e-mail: Caio — URGENTE: entrevista amanhã".to_string(), "📬 E-mail de pessoa: Loja — Vaga urgente com cupom de desconto".to_string()]);
        assert_eq!(verificar(&d, &caixa, 3.0, &enviar).unwrap(), 0); // nao reavisa
        let gravado = std::fs::read_to_string(d.join("correio.json")).unwrap();
        assert!(!gravado.contains("SEGREDO-DO-CORPO") && !avisos.borrow().iter().any(|a| a.contains("SEGREDO")));
        let est: Value = serde_json::from_str(&gravado).unwrap();
        assert_eq!((est["vistos"].as_array().unwrap().len(), est["relato"].as_array().unwrap().len(), est["fonte"].as_str()), (8, 4, Some("web"))); // Bia, Caio, Eva e Loja no relato da manha
        assert_eq!(candidaturas::resumo(&d).values().sum::<usize>(), 2); // Caio e Eva na fila do Nando
        assert!(importante("Intimação do cartório") && importante("Seu acesso foi bloqueado") && !importante("Oi, tudo bem?"));
    }
}
