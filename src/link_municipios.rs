//! Situacao das cidades na fila dos videos de prefeitura (link/municipios.py), para os dois lados do Link falarem a mesma lingua.
//! Fonte: o texto do arquivo da fila (secoes "Prontos", "Em produção", "Próximos"; itens "NNN Cidade" separados por " · ") entra
//! como PARAMETRO. Por cima dele, as RESERVAS (dados/link/producao/checklist.db, mesmo formato do Python): reservar() e atomica e
//! falha se a cidade ja esta pronta ou reservada por outro; reserva "fazendo" sem sinal por 7 dias vence sozinha.
//! Estados: falta -> producao -> entregue -> finalizado -> postado. Sem IA e sem rede. Ficou de fora: a linha de comando.
#![allow(dead_code)]
use crate::texto_pt::sem_acento;
use regex::Regex;
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

pub const VALIDADE_S: f64 = 7.0 * 24.0 * 3600.0; // pedido parado esperando a aprovacao das fontes nao pode perder a cidade
pub const ESTADOS: [&str; 5] = ["falta", "producao", "entregue", "finalizado", "postado"];
/// Motivo de quem pede cidade que nao esta no arquivo da fila (no Python o texto levava o nome do dono da fila).
pub const FORA_DA_FILA: &str = "cidade nao esta na fila dos videos";
static TRAVA: Mutex<()> = Mutex::new(());

pub fn chave(nome: &str) -> String {
    sem_acento(&nome.to_lowercase()).chars().filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit()).collect()
}

fn status_db(estado: &str) -> Option<&'static str> {
    match estado {
        "fazendo" => Some("producao"),
        "entregue" => Some("entregue"),
        "finalizado" => Some("finalizado"),
        "postado" => Some("postado"),
        _ => None,
    }
}

/// Lista de {numero, cidade, status, nota} na ordem do arquivo.
pub fn ler(texto: &str) -> Vec<Value> {
    static ITEM: OnceLock<Regex> = OnceLock::new();
    let item = ITEM.get_or_init(|| Regex::new(r"^\s*(\d{3})\s+([^(]+?)\s*(?:\((.*)\))?\s*$").expect("regex fixa"));
    let (mut out, mut status) = (vec![], None);
    for linha in texto.lines() {
        if let Some(titulo) = linha.strip_prefix("## ") {
            let t = titulo.to_lowercase();
            status = [("prontos", "finalizado"), ("em produção", "producao"), ("próximos", "falta")].iter().find(|(k, _)| t.starts_with(k)).map(|(_, v)| *v);
        } else if let (Some(s), false) = (status, linha.trim().is_empty()) {
            for pedaco in linha.split(" · ") {
                if let Some(m) = item.captures(pedaco) {
                    out.push(json!({ "numero": &m[1], "cidade": &m[2], "status": s, "nota": m.get(3).map_or("", |x| x.as_str()) }));
                }
            }
        }
    }
    out
}

fn achar(fila: &str, nome_ou_numero: &str) -> Option<Value> {
    let (k, num) = (chave(nome_ou_numero), format!("{:0>3}", nome_ou_numero.trim()));
    ler(fila).into_iter().find(|i| i["numero"] == num.as_str() || chave(i["cidade"].as_str().unwrap_or("")) == k)
}

fn db(dados: &Path) -> Result<Connection, String> {
    let p = dados.join("link").join("producao");
    std::fs::create_dir_all(&p).map_err(|e| e.to_string())?;
    let c = Connection::open(p.join("checklist.db")).map_err(|e| e.to_string())?;
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS reservas (chave TEXT PRIMARY KEY, numero TEXT, cidade TEXT, quem TEXT, estado TEXT, desde REAL, sinal REAL, nota TEXT);
         UPDATE reservas SET estado='entregue' WHERE estado='feito';
         CREATE TABLE IF NOT EXISTS historico (quando REAL, chave TEXT, quem TEXT, acao TEXT, nota TEXT);",
    )
    .map_err(|e| e.to_string())?;
    Ok(c)
}

struct Reserva {
    quem: String,
    estado: String,
    desde: f64,
}

/// Reservas que valem: so "fazendo" vence (7 dias sem sinal); entregue/finalizado/postado ficam.
fn vivas(c: &Connection, agora: f64) -> Result<HashMap<String, Reserva>, String> {
    let mut s = c.prepare("SELECT chave, quem, estado, desde, sinal FROM reservas").map_err(|e| e.to_string())?;
    let linhas = s
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, Option<f64>>(3)?, r.get::<_, Option<f64>>(4)?)))
        .map_err(|e| e.to_string())?;
    let mut out = HashMap::new();
    for (k, quem, estado, desde, sinal) in linhas.flatten() {
        if estado != "fazendo" || agora - sinal.unwrap_or(0.0) < VALIDADE_S {
            out.insert(k, Reserva { quem, estado, desde: desde.unwrap_or(0.0) });
        }
    }
    Ok(out)
}

/// Junta o arquivo com as reservas: o estado da reserva vale por cima do arquivo, com "quem" fez.
fn aplicar(dados: &Path, mut itens: Vec<Value>, agora: f64) -> Vec<Value> {
    let Ok(v) = db(dados).and_then(|c| vivas(&c, agora)) else { return itens };
    for i in itens.iter_mut() {
        if let Some(r) = v.get(&chave(i["cidade"].as_str().unwrap_or(""))) {
            i["status"] = json!(status_db(&r.estado).unwrap_or("producao"));
            i["quem"] = json!(r.quem);
            i["desde"] = json!(r.desde);
        }
    }
    itens
}

fn com(mut i: Value, campos: Value) -> Value {
    if let (Some(a), Some(b)) = (i.as_object_mut(), campos.as_object()) {
        a.extend(b.clone());
    }
    i
}

fn nao(i: Option<Value>, motivo: String, quem: Option<&str>) -> Value {
    let mut r = com(i.unwrap_or_else(|| json!({})), json!({ "ok": false, "motivo": motivo }));
    if let Some(q) = quem {
        r["quem"] = json!(q);
    }
    r
}

fn curta(nota: &str) -> String {
    nota.chars().take(200).collect()
}

fn gravar(c: &Connection, k: &str, i: &Value, quem_reserva: &str, estado: &str, desde: f64, agora: f64, nota: &str) -> Result<(), String> {
    c.execute("INSERT OR REPLACE INTO reservas VALUES (?,?,?,?,?,?,?,?)", params![k, i["numero"].as_str(), i["cidade"].as_str(), quem_reserva, estado, desde, agora, curta(nota)])
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn historico(c: &Connection, agora: f64, k: &str, quem: &str, acao: &str, nota: &str) -> Result<(), String> {
    c.execute("INSERT INTO historico VALUES (?,?,?,?,?)", params![agora, k, quem, acao, curta(nota)]).map(|_| ()).map_err(|e| e.to_string())
}

/// Pega a cidade para "quem". Ja feita ou reservada por outro -> ok false com o motivo. A mesma pessoa renova o sinal.
pub fn reservar(dados: &Path, fila: &str, nome_ou_numero: &str, quem: &str, nota: &str, agora: f64) -> Value {
    let Some(i) = achar(fila, nome_ou_numero) else { return nao(None, FORA_DA_FILA.into(), None) };
    let k = chave(i["cidade"].as_str().unwrap_or(""));
    let feito = || -> Result<Value, String> {
        let _t = TRAVA.lock().map_err(|e| e.to_string())?;
        let c = db(dados)?;
        let v = vivas(&c, agora)?;
        let r = v.get(&k);
        match r {
            Some(r) if r.estado != "fazendo" => return Ok(nao(Some(i.clone()), format!("ja esta {}", status_db(&r.estado).unwrap_or("None")), None)),
            None if i["status"] == "finalizado" => return Ok(nao(Some(i.clone()), "ja esta finalizado".into(), None)),
            Some(r) if r.quem != quem => return Ok(nao(Some(i.clone()), format!("reservada por {}", r.quem), Some(&r.quem))),
            _ => {}
        }
        gravar(&c, &k, &i, quem, "fazendo", r.map_or(agora, |r| r.desde), agora, nota)?;
        historico(&c, agora, &k, quem, "reservar", nota)?;
        Ok(com(i.clone(), json!({ "ok": true, "status": "producao", "quem": quem })))
    };
    feito().unwrap_or_else(|e| nao(Some(i.clone()), format!("banco das reservas falhou: {e}"), None))
}

/// acao -> (estados de onde pode sair, estado novo, so quem fez pode?). Estado novo None = apaga a reserva (volta a "falta").
/// "arquivo" = cidade que o arquivo da fila ja da como pronta; "" = sem reserva.
fn passo_de(acao: &str) -> Option<(&'static [&'static str], Option<&'static str>, bool)> {
    Some(match acao {
        "concluir" => (&["fazendo", ""], Some("entregue"), true), // rascunho entregue, esperando aprovacao
        "aprovar" => (&["entregue"], Some("finalizado"), false),  // aprovado: pronto para postar
        "reprovar" => (&["entregue"], Some("fazendo"), false),    // volta para quem fez refazer
        "postar" => (&["finalizado", "arquivo"], Some("postado"), false),
        "liberar" => (&["fazendo", ""], None, true),
        _ => return None,
    })
}

/// concluir | aprovar | reprovar | postar | liberar. Acao desconhecida e recusada.
pub fn passo(dados: &Path, fila: &str, nome_ou_numero: &str, quem: &str, acao: &str, nota: &str, agora: f64) -> Value {
    let Some((de, para, so_dono)) = passo_de(acao) else { return nao(None, format!("acao desconhecida: {acao}"), None) };
    let Some(i) = achar(fila, nome_ou_numero) else { return nao(None, FORA_DA_FILA.into(), None) };
    let k = chave(i["cidade"].as_str().unwrap_or(""));
    let feito = || -> Result<Value, String> {
        let _t = TRAVA.lock().map_err(|e| e.to_string())?;
        let c = db(dados)?;
        let v = vivas(&c, agora)?;
        let r = v.get(&k);
        let atual = r.map_or(if i["status"] == "finalizado" { "arquivo" } else { "" }, |r| r.estado.as_str());
        if !de.contains(&atual) {
            let esta = status_db(atual).unwrap_or(if atual.is_empty() { i["status"].as_str().unwrap_or("") } else { "finalizado" });
            return Ok(nao(Some(i.clone()), format!("nao da para {acao}: esta {esta}"), None));
        }
        if let Some(r) = r.filter(|r| so_dono && r.quem != quem) {
            return Ok(nao(Some(i.clone()), format!("reservada por {}", r.quem), Some(&r.quem)));
        }
        let dono = r.filter(|_| acao != "concluir").map_or(quem, |r| r.quem.as_str());
        match para {
            Some(p) => gravar(&c, &k, &i, dono, p, r.map_or(agora, |r| r.desde), agora, nota)?,
            None => c.execute("DELETE FROM reservas WHERE chave=?", [&k]).map(|_| ()).map_err(|e| e.to_string())?,
        }
        historico(&c, agora, &k, quem, acao, nota)?;
        Ok(com(i.clone(), json!({ "ok": true, "status": para.and_then(status_db).unwrap_or("falta"), "quem": dono })))
    };
    feito().unwrap_or_else(|e| nao(Some(i.clone()), format!("banco das reservas falhou: {e}"), None))
}

/// A proxima cidade "falta" sem reserva, na ordem da fila.
pub fn proxima_livre(dados: &Path, fila: &str, agora: f64) -> Option<Value> {
    aplicar(dados, ler(fila), agora).into_iter().find(|i| i["status"] == "falta")
}

/// Contagem + nomes por estado (com quem esta fazendo).
pub fn resumo(dados: &Path, fila: &str, agora: f64) -> Value {
    let itens = aplicar(dados, ler(fila), agora);
    let mut r = json!({ "total": itens.len() });
    for s in ESTADOS {
        let nomes: Vec<String> = itens
            .iter()
            .filter(|i| i["status"] == s)
            .map(|i| {
                let quem = i["quem"].as_str().filter(|q| !q.is_empty() && (s == "producao" || s == "entregue"));
                format!("{} {}{}", i["numero"].as_str().unwrap_or(""), i["cidade"].as_str().unwrap_or(""), quem.map(|q| format!(" ({q})")).unwrap_or_default())
            })
            .collect();
        r[format!("n_{s}")] = json!(nomes.len());
        r[s] = json!(nomes);
    }
    r
}

/// Situacao de uma cidade (acento e caixa nao importam). Nao achou -> status "desconhecida" (nunca inventa).
pub fn cidade(dados: &Path, fila: &str, nome: &str, agora: f64) -> Value {
    match achar(fila, nome) {
        Some(i) => aplicar(dados, vec![i], agora).remove(0),
        None => json!({ "numero": "", "cidade": nome, "status": "desconhecida", "nota": "nao esta na fila dos videos" }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILA: &str = "# Fila\n## Prontos (com vídeo)\n017 Itabuna · 150 Salvador\n## Em produção / refazer\n143 Itatim (lado A: rascunho)\n## Próximos (na ordem)\n152 Cidade B · 153 Barra do Rocha\n";

    fn pasta(nome: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("braniac-link_municipios-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn leitura_e_busca() {
        let d = pasta("ler");
        let r = resumo(&d, FILA, 1000.0);
        assert_eq!((r["total"].as_i64(), r["n_finalizado"].as_i64(), r["n_producao"].as_i64(), r["n_falta"].as_i64()), (Some(5), Some(2), Some(1), Some(2)));
        assert_eq!(cidade(&d, FILA, "ITABUNA", 1000.0)["status"], "finalizado");
        assert_eq!(cidade(&d, FILA, "itatim", 1000.0)["nota"], "lado A: rascunho");
        assert_eq!(cidade(&d, FILA, "17", 1000.0)["cidade"], "Itabuna");
        assert_eq!(cidade(&d, FILA, "Gotham", 1000.0)["status"], "desconhecida");
        assert_eq!(chave("São João d'Aliança"), "saojoaodalianca");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn reserva_exclusiva() {
        let d = pasta("reserva");
        let t = 5000.0;
        assert_eq!(reservar(&d, FILA, "Itabuna", "ana", "", t)["motivo"], "ja esta finalizado");
        assert_eq!(proxima_livre(&d, FILA, t).unwrap()["numero"], "152");
        assert_eq!(reservar(&d, FILA, "152", "bia", "", t)["ok"], true);
        let r = reservar(&d, FILA, "cidade b", "ana", "", t);
        assert!(r["ok"] == false && r["quem"] == "bia");
        assert_eq!(reservar(&d, FILA, "Cidade B", "bia", "", t)["ok"], true); // o mesmo lado renova
        assert_eq!(proxima_livre(&d, FILA, t).unwrap()["numero"], "153");
        assert_eq!(passo(&d, FILA, "152", "ana", "concluir", "", t)["ok"], false); // o outro lado nao fecha a reserva alheia
        assert_eq!(passo(&d, FILA, "152", "bia", "concluir", "", t)["ok"], true);
        assert_eq!(cidade(&d, FILA, "Cidade B", t)["status"], "entregue");
        assert_eq!(reservar(&d, FILA, "152", "ana", "", t)["motivo"], "ja esta entregue");
        assert_eq!(reservar(&d, FILA, "Gotham", "ana", "", t)["ok"], false);
        assert_eq!(passo(&d, FILA, "152", "ana", "apagar tudo", "", t)["ok"], false);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn reserva_vence_e_libera() {
        let d = pasta("vence");
        assert_eq!(reservar(&d, FILA, "153", "bia", "", 1000.0)["ok"], true);
        assert_eq!(reservar(&d, FILA, "153", "ana", "", 1000.0 + VALIDADE_S - 5.0)["ok"], false); // ainda vale
        let depois = 1000.0 + VALIDADE_S + 1.0;
        assert_eq!(reservar(&d, FILA, "153", "ana", "", depois)["ok"], true); // sem sinal: venceu
        assert_eq!(passo(&d, FILA, "153", "ana", "liberar", "", depois)["ok"], true);
        assert_eq!(cidade(&d, FILA, "153", depois)["status"], "falta");
        assert_eq!(resumo(&d, FILA, depois)["n_falta"], 2);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn entregue_aprovado_postado() {
        let d = pasta("postado");
        let t = 9000.0;
        let p = |n: &str, quem: &str, acao: &str| passo(&d, FILA, n, quem, acao, "nota", t);
        assert_eq!(p("152", "dono", "aprovar")["ok"], false); // nada entregue ainda
        assert_eq!(reservar(&d, FILA, "152", "ana", "", t)["ok"], true);
        assert_eq!(p("152", "ana", "concluir")["status"], "entregue");
        let r = p("152", "dono", "reprovar");
        assert!(r["status"] == "producao" && r["quem"] == "ana");
        assert_eq!(reservar(&d, FILA, "152", "bia", "", t)["ok"], false); // reprovada continua com quem fez
        assert_eq!(p("152", "ana", "concluir")["ok"], true);
        assert_eq!(p("152", "dono", "postar")["ok"], false); // sem aprovacao nao posta
        assert_eq!(p("152", "dono", "aprovar")["status"], "finalizado");
        assert_eq!(p("152", "dono", "postar")["status"], "postado");
        assert_eq!(p("Itabuna", "dono", "postar")["ok"], true); // os prontos do arquivo tambem podem ser marcados
        let r = resumo(&d, FILA, t);
        assert_eq!((r["n_postado"].as_i64(), r["n_finalizado"].as_i64(), r["n_entregue"].as_i64()), (Some(2), Some(1), Some(0)));
        assert_eq!(p("152", "ana", "liberar")["ok"], false); // postado nao volta para a fila
        let _ = std::fs::remove_dir_all(&d);
    }
}
