//! Garimpeiro: uma vez por dia, a partir das 8h, junta vagas REMOTAS de uma lista publica e manda as novas (no maximo 5).
//! Porte de braniac/vagas.py. Nao se candidata e nao manda dado pessoal. O texto das vagas e dado de terceiros.
//! Ficou de fora: a leitura da lista publica (closure `buscar(termo)`) e o envio (closure `enviar`); este modulo nao fala com a rede.
//! As novas tambem entram na fila do Nando (`candidaturas::receber`). O recado ao barramento fica para quem chama.
#![allow(dead_code)]
use crate::candidaturas;
use crate::tempo::Agora;
use regex::Regex;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::OnceLock;

pub const BUSCAS: [&str; 3] = ["IT support", "technical support", "AI automation"];
const HORA: u32 = 8;
const MAX_DIA: usize = 5;

fn id(j: &Value) -> String {
    j["id"].as_str().map_or_else(|| j["id"].to_string(), String::from)
}

/// Vagas novas com local compativel, sem repetir; as que informam salario vem primeiro. Pura.
pub fn filtrar(jobs: &[Value], vistos: &[String]) -> Vec<Value> {
    static R: OnceLock<Regex> = OnceLock::new();
    let local_ok = R.get_or_init(|| Regex::new(r"(?i)worldwide|anywhere|global|brazil|brasil|latin|latam|americas|south america").expect("regex fixa"));
    let mut ids: Vec<String> = Vec::new();
    let mut out: Vec<Value> = Vec::new();
    for j in jobs {
        let i = id(j);
        if vistos.contains(&i) || ids.contains(&i) || !local_ok.is_match(j["candidate_required_location"].as_str().unwrap_or("")) {
            continue;
        }
        ids.push(i);
        out.push(j.clone());
    }
    let sem_salario = |j: &Value| j["salary"].as_str().map_or(true, str::is_empty);
    out.sort_by_key(sem_salario); // estavel: mantem a ordem de chegada dentro de cada grupo
    out
}

/// No maximo uma vez por dia, a partir das 8h. Devolve quantas vagas mandou. `ts` = instante para a fila do Nando.
/// Busca que falha e pulada; envio que falha nao desfaz o dia (as vagas ficam na fila do Nando e saem no relato).
pub fn verificar(dados: &Path, agora: &Agora, ts: f64, buscar: &dyn Fn(&str) -> Result<Vec<Value>, String>, enviar: &dyn Fn(&str) -> Result<(), String>) -> Result<usize, String> {
    let arq = dados.join("vagas.json");
    let mut est: Value = std::fs::read_to_string(&arq).ok().and_then(|t| serde_json::from_str(&t).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}));
    let hoje = agora.data.iso();
    if agora.hora < HORA || est["dia"] == hoje.as_str() {
        return Ok(0);
    }
    let mut vistos: Vec<String> = est["vistos"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect();
    let jobs: Vec<Value> = BUSCAS.iter().filter_map(|b| buscar(b).ok()).flatten().collect();
    let novas: Vec<Value> = filtrar(&jobs, &vistos).into_iter().take(MAX_DIA).collect();
    vistos.extend(novas.iter().map(id));
    est["dia"] = json!(hoje);
    est["vistos"] = json!(vistos[vistos.len().saturating_sub(800)..]);
    std::fs::create_dir_all(dados).and_then(|_| std::fs::write(&arq, est.to_string())).map_err(|e| format!("não gravei o estado das vagas: {e}"))?;
    let s = |j: &Value, k: &str| j[k].as_str().unwrap_or("").to_string();
    for j in &novas {
        let _ = candidaturas::receber(dados, "Garimpeiro", &s(j, "title"), &s(j, "company_name"), &s(j, "url"), "", ts);
    }
    if !novas.is_empty() {
        let linhas: Vec<String> = novas.iter().map(|j| format!("• {} — {} ({}) {}", s(j, "title").chars().take(70).collect::<String>(), s(j, "company_name").chars().take(30).collect::<String>(),
            Some(s(j, "salary")).filter(|x| !x.is_empty()).unwrap_or_else(|| "salário não informado".into()), s(j, "url"))).collect();
        let _ = enviar(&format!("🔎 Vagas remotas novas de hoje (Garimpeiro):\n{}", linhas.join("\n")));
    }
    Ok(novas.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;
    use std::cell::RefCell;

    fn vaga(id: i64, local: &str, salario: &str) -> Value {
        json!({ "id": id, "title": format!("Vaga {id}"), "company_name": "Empresa Exemplo", "url": format!("https://vagas.example/{id}"), "candidate_required_location": local, "salary": salario })
    }
    fn ag(dia: i64, hora: u32) -> Agora { Agora { data: Data::civil(2030, 5, dia).unwrap(), hora, min: 0, seg: 0 } }

    #[test]
    fn filtra_local_repetida_vista_e_poe_salario_primeiro() {
        let jobs = [vaga(1, "USA only", "$1"), vaga(2, "Worldwide", ""), vaga(3, "Brazil", "$3k"), vaga(2, "Worldwide", ""), vaga(4, "LATAM", ""), vaga(5, "Anywhere", "x")];
        let f = filtrar(&jobs, &["5".to_string()]);
        assert_eq!(f.iter().map(|j| j["id"].as_i64().unwrap()).collect::<Vec<_>>(), [3, 2, 4]);
        assert!(filtrar(&[json!({"id": 9})], &[]).is_empty()); // sem local informado nao entra
    }

    #[test]
    fn uma_vez_por_dia_depois_das_8_no_maximo_5() {
        let p = std::env::temp_dir().join(format!("braniac-vagas-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        let env = RefCell::new(Vec::<String>::new());
        let enviar = |t: &str| { env.borrow_mut().push(t.to_string()); Ok(()) };
        let buscas = RefCell::new(0);
        let buscar = |termo: &str| { *buscas.borrow_mut() += 1; if termo == "technical support" { Err("fora do ar".into()) } else { Ok((1..=7).map(|i| vaga(i, "Worldwide", if i == 7 { "$5k" } else { "" })).collect()) } };
        assert_eq!(verificar(&p, &ag(2, 7), 0.0, &buscar, &enviar).unwrap(), 0); // antes das 8h nem busca
        assert_eq!(*buscas.borrow(), 0);
        assert_eq!(verificar(&p, &ag(2, 8), 0.0, &buscar, &enviar).unwrap(), 5);
        assert!(env.borrow().len() == 1 && env.borrow()[0].lines().count() == 6 && env.borrow()[0].lines().nth(1).unwrap().contains("Vaga 7 — Empresa Exemplo ($5k) https://vagas.example/7")
            && env.borrow()[0].contains("salário não informado"));
        assert_eq!(verificar(&p, &ag(2, 15), 0.0, &buscar, &enviar).unwrap(), 0); // ja rodou hoje
        assert_eq!(candidaturas::resumo(&p).get("triagem"), Some(&5)); // as novas entraram na fila do Nando
        assert_eq!(verificar(&p, &ag(3, 9), 0.0, &buscar, &enviar).unwrap(), 2); // no dia seguinte so as que faltavam
        assert_eq!(verificar(&p, &ag(4, 9), 0.0, &buscar, &|_| Err("sem conexão".into())).unwrap(), 0);
        assert_eq!(env.borrow().len(), 2);
        let _ = std::fs::remove_dir_all(&p);
    }
}
