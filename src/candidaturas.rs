//! Nando: fila de candidaturas (dados/candidaturas.json, mesmo formato do braniac/candidaturas.py). So triagem e relato.
//! Ele NAO envia candidatura, NAO cria conta e NAO digita senha: o que exige isso fica "aguardando_voce".
//! Reuniao, entrevista, teste, login ou captcha viram "precisa de você". Nunca inventa experiencia.
//! Ficou de fora: o recado "nova vaga na fila" no barramento (quem chama `receber` decide se avisa).
#![allow(dead_code)]
use regex::Regex;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

const MAX: usize = 200;

fn remota() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)remot[oa]|home\s*office|trabalho\s+em\s+casa|anywhere|work\s+from\s+home|\bremote\b").expect("regex fixa"))
}

fn humano() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)entrevista|reuni[ãa]o|teste|desafio|case\b|v[ií]deo|agendar|call\b|interview|assessment|captcha|login|cadastro").expect("regex fixa"))
}

fn corta(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn ler(dados: &Path) -> Vec<Value> {
    std::fs::read_to_string(dados.join("candidaturas.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.as_array().cloned()).unwrap_or_default()
}

fn gravar(dados: &Path, itens: &[Value]) -> Result<(), String> {
    let corte = itens.len().saturating_sub(MAX);
    std::fs::create_dir_all(dados).and_then(|_| std::fs::write(dados.join("candidaturas.json"), json!(itens[corte..]).to_string())).map_err(|e| format!("não gravei a fila de vagas: {e}"))
}

fn chave(titulo: &str, empresa: &str, link: &str) -> String {
    if link.is_empty() { format!("{titulo}|{empresa}") } else { link.to_string() }.to_lowercase()
}

/// Entra uma vaga na fila. Ok(None) se ja estava (mesmo link, ou mesmo titulo+empresa).
pub fn receber(dados: &Path, origem: &str, titulo: &str, empresa: &str, link: &str, texto: &str, ts: f64) -> Result<Option<Value>, String> {
    let mut itens = ler(dados);
    let k = chave(titulo, empresa, link);
    if itens.iter().any(|i| i["chave"] == k.as_str()) {
        return Ok(None);
    }
    let alvo = format!("{titulo} {empresa} {texto}");
    let hum = humano().is_match(&alvo);
    let item = json!({ "chave": k, "origem": origem, "titulo": corta(titulo, 120), "empresa": corta(empresa, 60), "link": link, "ts": ts,
        "remota": if origem == "Garimpeiro" || remota().is_match(&alvo) { json!(true) } else { Value::Null },
        "status": if hum { "aguardando_voce" } else { "triagem" },
        "motivo": if hum { "o aviso fala de reunião/entrevista/teste/login: só você resolve" } else if link.is_empty() { "sem link da vaga: preciso abrir o e-mail original" } else { "tem link; falta você autorizar a inscrição" },
        "avisado": false });
    itens.push(item.clone());
    gravar(dados, &itens)?;
    Ok(Some(item))
}

/// Para a reuniao da manha: o que entrou desde o ultimo relato e o que depende de uma pessoa. Marca como avisado. Nada novo = "".
pub fn relato(dados: &Path) -> Result<String, String> {
    let mut itens = ler(dados);
    let novos: Vec<Value> = itens.iter().filter(|i| i["avisado"] != true).cloned().collect();
    if novos.is_empty() {
        return Ok(String::new());
    }
    for i in itens.iter_mut() {
        i["avisado"] = json!(true);
    }
    gravar(dados, &itens)?;
    let s = |i: &Value, k: &str| i[k].as_str().unwrap_or("").to_string();
    let nomes: Vec<String> = novos.iter().take(3).map(|i| format!("{}{}", corta(&s(i, "titulo"), 50), if s(i, "empresa").is_empty() { String::new() } else { format!(" na {}", corta(&s(i, "empresa"), 25)) })).collect();
    let mut f = vec![format!("Entraram {} vaga(s) na fila: {}.", novos.len(), nomes.join("; "))];
    let hum: Vec<String> = novos.iter().filter(|i| i["status"] == "aguardando_voce" && humano().is_match(&format!("{}{}", s(i, "motivo"), s(i, "titulo")))).take(3).map(|i| corta(&s(i, "titulo"), 50)).collect();
    if !hum.is_empty() {
        f.push(format!("Atenção, precisa de você: {} fala de reunião, entrevista, teste ou login.", hum.join("; ")));
    }
    f.push("Eu ainda não me inscrevo sozinho: me diga quais você quer e eu preparo a mensagem.".into());
    Ok(f.join(" "))
}

/// Quantas vagas em cada status.
pub fn resumo(dados: &Path) -> BTreeMap<String, usize> {
    let mut por = BTreeMap::new();
    for i in ler(dados) {
        *por.entry(i["status"].as_str().unwrap_or("").to_string()).or_insert(0) += 1;
    }
    por
}

/// Cria ou atualiza a vaga com o status final informado por uma PESSOA ou rotina ("enviada", "pulada", "aguardando_voce"); "enviada" guarda ts_envio.
/// So anota: este modulo nao envia candidatura.
pub fn registrar(dados: &Path, status: &str, titulo: &str, empresa: &str, link: &str, motivo: &str, ts: f64) -> Result<(), String> {
    receber(dados, "Rotina", titulo, empresa, link, "", ts)?;
    let k = chave(titulo, empresa, link);
    let mut itens = ler(dados);
    for i in itens.iter_mut().filter(|i| i["chave"] == k.as_str()) {
        i["status"] = json!(status);
        i["motivo"] = json!(corta(motivo, 160));
        i["avisado"] = json!(true);
        if status == "enviada" {
            i["ts_envio"] = json!(ts);
        }
    }
    gravar(dados, &itens)
}

pub fn enviadas_entre(dados: &Path, ini: f64, fim: f64) -> usize {
    ler(dados).iter().filter(|i| i["status"] == "enviada" && (ini..fim).contains(&i["ts_envio"].as_f64().unwrap_or(0.0))).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fila_triagem_relato_e_registro() {
        let p = std::env::temp_dir().join(format!("braniac-candidaturas-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        assert_eq!(relato(&p).unwrap(), "");
        let e = receber(&p, "Correio", "Convite para entrevista", "Acme", "", "", 1.0).unwrap().unwrap();
        assert!(e["status"] == "aguardando_voce" && e["remota"].is_null()); // o que pede uma pessoa nunca segue sozinho
        let v = receber(&p, "Garimpeiro", "Support Engineer", "Foo", "https://x.example/1", "", 2.0).unwrap().unwrap();
        assert!(v["status"] == "triagem" && v["remota"] == true && v["motivo"] == "tem link; falta você autorizar a inscrição");
        assert!(receber(&p, "Garimpeiro", "Support Engineer", "Foo", "HTTPS://x.example/1", "", 3.0).unwrap().is_none()); // nao duplica
        assert_eq!(receber(&p, "Correio", "Analista", "Bar", "", "vaga em home office", 3.0).unwrap().unwrap()["motivo"], "sem link da vaga: preciso abrir o e-mail original");
        let r = relato(&p).unwrap();
        assert!(r.contains("3 vaga") && r.contains("Convite para entrevista na Acme") && r.contains("precisa de você") && r.contains("não me inscrevo sozinho"), "{r}");
        assert_eq!(relato(&p).unwrap(), ""); // so relata o que e novo
        registrar(&p, "enviada", "Support Engineer", "Foo", "https://x.example/1", "o dono enviou", 50.0).unwrap();
        registrar(&p, "pulada", "Outra vaga", "Baz", "", "fora do perfil", 60.0).unwrap();
        assert_eq!((enviadas_entre(&p, 0.0, 100.0), enviadas_entre(&p, 51.0, 100.0)), (1, 0));
        assert_eq!(resumo(&p), BTreeMap::from([("aguardando_voce".to_string(), 1), ("enviada".to_string(), 1), ("pulada".to_string(), 1), ("triagem".to_string(), 1)]));
        assert_eq!(relato(&p).unwrap(), ""); // o que foi registrado ja nasce avisado
        let _ = std::fs::remove_dir_all(&p);
    }
}
