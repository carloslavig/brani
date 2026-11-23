//! Fila dos cortes (SQLite em dados/cortes/cortes.db, mesmo esquema do Python): renderizado -> aprovado/rejeitado -> publicado.
//! Nome de coluna vem de quem chama: so passa o que esta em COLUNAS (o resto e recusado, nunca vira SQL).
#![allow(dead_code)]
use crate::cortes_config::pasta;
use rusqlite::{types::ValueRef, Connection, ToSql};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::Path;

const ESQUEMA: &str = "CREATE TABLE IF NOT EXISTS cortes(
  id INTEGER PRIMARY KEY AUTOINCREMENT, canal TEXT, origem TEXT, fonte TEXT, inicio REAL, fim REAL,
  score REAL, motivo TEXT, arquivo TEXT, transcricao TEXT, titulo TEXT, descricao TEXT, hashtags TEXT,
  status TEXT DEFAULT 'renderizado', youtube_id TEXT, tiktok TEXT, erro TEXT, criado REAL, atualizado REAL);
CREATE TABLE IF NOT EXISTS vistos(chave TEXT PRIMARY KEY, quando REAL);";
const COLUNAS: [&str; 19] = ["canal", "origem", "fonte", "inicio", "fim", "score", "motivo", "arquivo", "transcricao", "titulo", "descricao", "hashtags", "status",
    "youtube_id", "tiktok", "erro", "criado", "atualizado", "perfil"];

fn e(x: rusqlite::Error) -> String {
    x.to_string()
}

fn con(dados: &Path) -> Result<Connection, String> {
    let p = pasta(dados);
    std::fs::create_dir_all(&p).map_err(|x| x.to_string())?;
    let c = Connection::open(p.join("cortes.db")).map_err(e)?;
    c.busy_timeout(std::time::Duration::from_secs(30)).map_err(e)?;
    c.execute_batch(ESQUEMA).map_err(e)?;
    let tem: Vec<String> = c.prepare("PRAGMA table_info(cortes)").map_err(e)?.query_map([], |r| r.get::<_, String>(1)).map_err(e)?.flatten().collect();
    // bancos antigos: perfil vazio = canal PT; agendado = horario de publicacao no YouTube
    for (col, sql) in [("perfil", "ALTER TABLE cortes ADD COLUMN perfil TEXT DEFAULT ''"), ("agendado", "ALTER TABLE cortes ADD COLUMN agendado REAL")] {
        if !tem.iter().any(|t| t == col) {
            c.execute(sql, []).map_err(e)?;
        }
    }
    Ok(c)
}

fn para_sql(v: &Value) -> Box<dyn ToSql> {
    match v {
        Value::Null => Box::new(None::<String>),
        Value::Bool(b) => Box::new(*b as i64),
        Value::Number(n) => match n.as_i64() {
            Some(i) => Box::new(i),
            None => Box::new(n.as_f64().unwrap_or(0.0)),
        },
        Value::String(s) => Box::new(s.clone()),
        outro => Box::new(outro.to_string()),
    }
}

fn campos_validos(campos: &Value, extra: &[&str]) -> Result<Vec<(String, Value)>, String> {
    let o = campos.as_object().ok_or("campos precisam ser um objeto")?;
    match o.keys().find(|k| !COLUNAS.contains(&k.as_str()) && !extra.contains(&k.as_str())) {
        Some(k) => Err(format!("coluna desconhecida: {k}")),
        None => Ok(o.iter().map(|(k, v)| (k.clone(), v.clone())).collect()),
    }
}

fn linha(r: &rusqlite::Row) -> Value {
    let mut m = Map::new();
    for (i, nome) in r.as_ref().column_names().iter().enumerate() {
        let v = match r.get_ref(i) {
            Ok(ValueRef::Integer(n)) => json!(n),
            Ok(ValueRef::Real(f)) => json!(f),
            Ok(ValueRef::Text(t)) => json!(String::from_utf8_lossy(t)),
            _ => Value::Null,
        };
        m.insert(nome.to_string(), v);
    }
    Value::Object(m)
}

fn varias(dados: &Path, sql: &str, ps: &[&dyn ToSql]) -> Vec<Value> {
    let Ok(c) = con(dados) else { return vec![] };
    let Ok(mut st) = c.prepare(sql) else { return vec![] };
    let r = st.query_map(ps, |r| Ok(linha(r))).map(|it| it.flatten().collect()).unwrap_or_default();
    r
}

fn conta(dados: &Path, sql: &str, ps: &[&dyn ToSql]) -> i64 {
    con(dados).ok().and_then(|c| c.query_row(sql, ps, |r| r.get(0)).ok()).unwrap_or(0)
}

pub fn ja_visto(dados: &Path, chave: &str) -> bool {
    conta(dados, "SELECT COUNT(*) FROM vistos WHERE chave=?", &[&chave]) > 0
}

pub fn marcar_visto(dados: &Path, chave: &str, ts: f64) -> Result<(), String> {
    con(dados)?.execute("INSERT OR IGNORE INTO vistos VALUES(?,?)", (chave, ts)).map(|_| ()).map_err(e)
}

/// Poe um corte na fila (status padrao "renderizado") e devolve o id.
pub fn adicionar(dados: &Path, campos: &Value, ts: f64) -> Result<i64, String> {
    let mut cs = campos_validos(campos, &[])?;
    cs.retain(|(k, _)| k != "criado" && k != "atualizado");
    if !cs.iter().any(|(k, _)| k == "status") {
        cs.push(("status".into(), json!("renderizado")));
    }
    cs.push(("criado".into(), json!(ts)));
    cs.push(("atualizado".into(), json!(ts)));
    let cols: Vec<&str> = cs.iter().map(|(k, _)| k.as_str()).collect();
    let vals: Vec<Box<dyn ToSql>> = cs.iter().map(|(_, v)| para_sql(v)).collect();
    let c = con(dados)?;
    c.execute(&format!("INSERT INTO cortes({}) VALUES({})", cols.join(","), vec!["?"; cols.len()].join(",")), rusqlite::params_from_iter(vals.iter())).map_err(e)?;
    Ok(c.last_insert_rowid())
}

pub fn atualizar(dados: &Path, id: i64, campos: &Value, ts: f64) -> Result<(), String> {
    let mut cs = campos_validos(campos, &["agendado"])?;
    cs.retain(|(k, _)| k != "atualizado");
    cs.push(("atualizado".into(), json!(ts)));
    let sets: Vec<String> = cs.iter().map(|(k, _)| format!("{k}=?")).collect();
    let mut vals: Vec<Box<dyn ToSql>> = cs.iter().map(|(_, v)| para_sql(v)).collect();
    vals.push(Box::new(id));
    con(dados)?.execute(&format!("UPDATE cortes SET {} WHERE id=?", sets.join(",")), rusqlite::params_from_iter(vals.iter())).map(|_| ()).map_err(e)
}

pub fn pegar(dados: &Path, id: i64) -> Option<Value> {
    varias(dados, "SELECT * FROM cortes WHERE id=?", &[&id]).into_iter().next()
}

/// Do mais novo ao mais antigo.
pub fn listar(dados: &Path, status: Option<&str>, limite: i64) -> Vec<Value> {
    match status.filter(|s| !s.is_empty()) {
        Some(s) => varias(dados, "SELECT * FROM cortes WHERE status=? ORDER BY id DESC LIMIT ?", &[&s, &limite]),
        None => varias(dados, "SELECT * FROM cortes ORDER BY id DESC LIMIT ?", &[&limite]),
    }
}

/// `inicio_do_dia`: carimbo da meia-noite local de hoje (quem chama calcula com `Agora`).
pub fn publicados_hoje(dados: &Path, perfil: &str, inicio_do_dia: f64) -> i64 {
    conta(dados, "SELECT COUNT(*) FROM cortes WHERE status='publicado' AND atualizado>=? AND COALESCE(perfil,'')=?", &[&inicio_do_dia, &perfil])
}

pub fn horarios_ocupados(dados: &Path, perfil: &str) -> Vec<f64> {
    varias(dados, "SELECT agendado FROM cortes WHERE status='publicado' AND agendado IS NOT NULL AND COALESCE(perfil,'')=?", &[&perfil]).iter().filter_map(|v| v["agendado"].as_f64()).collect()
}

pub fn agenda(dados: &Path, perfil: &str, desde: f64) -> Vec<Value> {
    varias(dados, "SELECT * FROM cortes WHERE status='publicado' AND agendado>=? AND COALESCE(perfil,'')=? ORDER BY agendado", &[&desde, &perfil])
}

/// Cortes aprovados que ainda nao subiram para o YouTube.
pub fn estoque(dados: &Path, perfil: &str) -> i64 {
    conta(dados, "SELECT COUNT(*) FROM cortes WHERE status='aprovado' AND COALESCE(perfil,'')=?", &[&perfil])
}

pub fn resumo(dados: &Path) -> BTreeMap<String, i64> {
    varias(dados, "SELECT status, COUNT(*) n FROM cortes GROUP BY status", &[]).iter().filter_map(|v| Some((v["status"].as_str()?.to_string(), v["n"].as_i64()?))).collect()
}

#[cfg(test)]
pub(crate) fn tmp(n: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("braniac-cortes-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fila_guarda_lista_atualiza_e_conta() {
        let d = tmp("fila");
        let a = adicionar(&d, &json!({"canal": "c", "origem": "live", "fonte": "x", "inicio": 0, "fim": 30, "arquivo": "a.mp4", "titulo": "A", "motivo": "m"}), 100.0).unwrap();
        let b = adicionar(&d, &json!({"canal": "c", "titulo": "B", "status": "aprovado", "perfil": "gringo"}), 101.0).unwrap();
        assert_eq!((a, b), (1, 2));
        let ca = pegar(&d, a).unwrap();
        assert_eq!((ca["status"].as_str(), ca["perfil"].as_str(), ca["fim"].as_f64(), ca["criado"].as_f64()), (Some("renderizado"), Some(""), Some(30.0), Some(100.0)));
        assert_eq!(listar(&d, None, 30).iter().map(|c| c["id"].as_i64().unwrap()).collect::<Vec<_>>(), vec![2, 1]);
        assert_eq!(listar(&d, Some("aprovado"), 30).len(), 1);
        assert_eq!((estoque(&d, "gringo"), estoque(&d, "")), (1, 0));
        atualizar(&d, b, &json!({"status": "publicado", "youtube_id": "v1", "erro": null, "agendado": 5000.0}), 200.0).unwrap();
        assert_eq!((publicados_hoje(&d, "gringo", 150.0), publicados_hoje(&d, "gringo", 250.0), publicados_hoje(&d, "", 0.0)), (1, 0, 0));
        assert_eq!(horarios_ocupados(&d, "gringo"), vec![5000.0]);
        assert_eq!((agenda(&d, "gringo", 4000.0).len(), agenda(&d, "gringo", 6000.0).len()), (1, 0));
        assert_eq!(resumo(&d), BTreeMap::from([("publicado".to_string(), 1), ("renderizado".to_string(), 1)]));
        assert_eq!(pegar(&d, 99), None);
    }

    #[test]
    fn vistos_e_coluna_estranha_recusada() {
        let d = tmp("vistos");
        marcar_visto(&d, "clipe:1", 1.0).unwrap();
        marcar_visto(&d, "clipe:1", 2.0).unwrap();
        assert!(ja_visto(&d, "clipe:1") && !ja_visto(&d, "clipe:2"));
        assert!(adicionar(&d, &json!({"titulo": "x", "id; DROP TABLE cortes": 1}), 1.0).is_err());
        assert!(atualizar(&d, 1, &json!({"status=''--": 1}), 1.0).is_err());
        assert!(adicionar(&d, &json!([1]), 1.0).is_err());
    }
}
