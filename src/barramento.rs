//! Barramento dos agentes: caixa de recados local (dados/agentes.db, mesmo formato do Braniac em Python), sem IA e sem rede.
//! So entrega recados: NUNCA executa nada. Quem age continua sendo cada agente, dentro das proprias travas.
use crate::agentes;
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static TRAVA: Mutex<()> = Mutex::new(());

fn db(dados: &Path) -> Result<Connection, String> {
    let c = Connection::open(dados.join("agentes.db")).map_err(|e| e.to_string())?;
    c.execute("CREATE TABLE IF NOT EXISTS msg(id INTEGER PRIMARY KEY, ts REAL, de TEXT, para TEXT, tipo TEXT, texto TEXT, urgencia INTEGER DEFAULT 0, lida INTEGER DEFAULT 0, resp_de INTEGER)", [])
        .map_err(|e| e.to_string())?;
    Ok(c)
}

/// Posta um recado e devolve o id. Remetente ou destinatario desconhecido e recusado.
pub fn enviar(dados: &Path, de: &str, para: &str, tipo: &str, texto: &str, urgencia: i64, resp_de: Option<i64>) -> Result<i64, String> {
    let conhecidos = agentes::nomes(dados);
    if !conhecidos.iter().any(|n| n == de) || !conhecidos.iter().any(|n| n == para) {
        return Err(format!("agente desconhecido: {de:?} -> {para:?}"));
    }
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
    let texto: String = texto.chars().take(1500).collect();
    let _t = TRAVA.lock().map_err(|e| e.to_string())?;
    let c = db(dados)?;
    c.execute("INSERT INTO msg(ts,de,para,tipo,texto,urgencia,resp_de) VALUES(?,?,?,?,?,?,?)", params![ts, de, para, tipo, texto, urgencia, resp_de])
        .map_err(|e| e.to_string())?;
    Ok(c.last_insert_rowid())
}

fn linhas(dados: &Path, sql: &str, p: &[&dyn rusqlite::ToSql]) -> Vec<Value> {
    let Ok(_t) = TRAVA.lock() else { return vec![] };
    let Ok(c) = db(dados) else { return vec![] };
    let Ok(mut s) = c.prepare(sql) else { return vec![] };
    let r = s.query_map(p, |r| {
        Ok(json!({ "id": r.get::<_, i64>(0)?, "ts": r.get::<_, f64>(1)?, "de": r.get::<_, String>(2)?, "para": r.get::<_, String>(3)?,
                   "tipo": r.get::<_, String>(4)?, "texto": r.get::<_, String>(5)?, "urgencia": r.get::<_, i64>(6)?, "lida": r.get::<_, i64>(7)? }))
    });
    r.map(|it| it.flatten().collect()).unwrap_or_default()
}

const CAMPOS: &str = "SELECT id,ts,de,para,tipo,texto,urgencia,lida FROM msg";

/// Recados de um agente, do mais antigo ao mais novo.
#[allow(dead_code)] // quem usa: os agentes que lerem a propria caixa (a portar)
pub fn caixa(dados: &Path, para: &str, so_novas: bool, n: i64) -> Vec<Value> {
    let filtro = if so_novas { " AND lida=0" } else { "" };
    linhas(dados, &format!("{CAMPOS} WHERE para=?{filtro} ORDER BY id LIMIT ?"), &[&para, &n])
}

#[allow(dead_code)] // quem usa: os agentes que lerem a propria caixa (a portar)
pub fn marcar_lida(dados: &Path, id: i64) -> Result<(), String> {
    let _t = TRAVA.lock().map_err(|e| e.to_string())?;
    db(dados)?.execute("UPDATE msg SET lida=1 WHERE id=?", [id]).map(|_| ()).map_err(|e| e.to_string())
}

/// Responde a um recado (vai para quem enviou) e marca o original como lido.
#[allow(dead_code)] // quem usa: os agentes que lerem a propria caixa (a portar)
pub fn responder(dados: &Path, id: i64, de: &str, texto: &str) -> Result<i64, String> {
    let para = linhas(dados, &format!("{CAMPOS} WHERE id=?"), &[&id]).first().and_then(|m| m["de"].as_str().map(String::from)).ok_or("recado não encontrado")?;
    marcar_lida(dados, id)?;
    enviar(dados, de, &para, "resposta", texto, 0, Some(id))
}

/// Conversa entre os agentes, os n ultimos, do mais antigo ao mais novo (para a tela).
pub fn historico(dados: &Path, n: i64) -> Vec<Value> {
    let mut v = linhas(dados, &format!("{CAMPOS} ORDER BY id DESC LIMIT ?"), &[&n]);
    v.reverse();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recado_vai_volta_e_desconhecido_e_recusado() {
        let d = std::env::temp_dir().join(format!("braniac-barramento-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        assert!(enviar(&d, "Jarvis", "Fulano", "pedido", "oi", 0, None).is_err());
        assert!(enviar(&d, "Fulano", "Jarvis", "pedido", "oi", 0, None).is_err());
        let id = enviar(&d, "Jarvis", "Compasso", "pedido", &"x".repeat(3000), 1, None).unwrap();
        let cx = caixa(&d, "Compasso", true, 20);
        assert_eq!((cx.len(), cx[0]["texto"].as_str().unwrap().len(), cx[0]["urgencia"].as_i64()), (1, 1500, Some(1)));
        responder(&d, id, "Compasso", "feito").unwrap();
        assert!(caixa(&d, "Compasso", true, 20).is_empty()); // o original ficou lido
        assert_eq!(caixa(&d, "Compasso", false, 20).len(), 1);
        let h = historico(&d, 50);
        assert_eq!((h.len(), h[1]["para"].as_str(), h[1]["tipo"].as_str()), (2, Some("Jarvis"), Some("resposta")));
        assert!(responder(&d, 999, "Compasso", "x").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
