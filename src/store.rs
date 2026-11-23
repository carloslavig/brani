//! Historico por conversa, descanso de provedores e contagem de uso (SQLite). Porte de braniac/store.py.
//! Escrito pelo Otto (NexoDev, T057), revisado e ajustado pelo Claude.
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::json;
use std::path::Path;
use std::sync::Mutex;

/// Camada de armazenamento persistente.
pub struct Store {
    con: Mutex<Connection>,
}

impl Store {
    /// Abre (ou cria) o banco no caminho informado.
    pub fn abrir(caminho: &Path) -> Result<Self, String> {
        let conn = Connection::open(caminho).map_err(|e| e.to_string())?;
        Self::init(&conn)?;
        Ok(Self {
            con: Mutex::new(conn),
        })
    }

    /// Cria um banco em memória (para testes).
    #[cfg(test)]
    pub fn em_memoria() -> Result<Self, String> {
        let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
        Self::init(&conn)?;
        Ok(Self {
            con: Mutex::new(conn),
        })
    }

    /// Cria as tabelas caso ainda não existam.
    fn init(conn: &Connection) -> Result<(), String> {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS msgs(
                id INTEGER PRIMARY KEY,
                conv TEXT,
                role TEXT,
                content TEXT,
                provider TEXT,
                ts REAL
            );
            CREATE TABLE IF NOT EXISTS estado(
                nome TEXT PRIMARY KEY,
                ate REAL,
                erro TEXT
            );
            CREATE TABLE IF NOT EXISTS uso(
                dia TEXT,
                nome TEXT,
                n INTEGER,
                PRIMARY KEY(dia, nome)
            );
            ",
        )
        .map_err(|e| e.to_string())
    }

    /// Insere uma mensagem no histórico.
    pub fn add_msg(
        &self,
        conv: &str,
        role: &str,
        content: &str,
        provider: Option<&str>,
    ) -> Result<(), String> {
        let db = self.con.lock().map_err(|_| "lock falhou".to_string())?;
        db.execute(
            "INSERT INTO msgs (conv, role, content, provider, ts)
             VALUES (?, ?, ?, ?, strftime('%s','now'))",
            params![conv, role, content, provider],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    /// As n ultimas mensagens em ordem cronologica: {"role","content","provider"}.
    /// `provider` comecando com "local/" marca turno que nao pode ir a IA online.
    pub fn historico(&self, conv: &str, n: usize) -> Vec<serde_json::Value> {
        let Ok(db) = self.con.lock() else { return Vec::new() };
        let Ok(mut stmt) = db.prepare("SELECT role, content, provider FROM msgs WHERE conv = ? ORDER BY id DESC LIMIT ?") else {
            return Vec::new();
        };
        let Ok(rows) = stmt.query_map(params![conv, n as i64], |r| {
            Ok(json!({ "role": r.get::<_, String>(0)?, "content": r.get::<_, String>(1)?, "provider": r.get::<_, Option<String>>(2)? }))
        }) else {
            return Vec::new();
        };
        let mut msgs: Vec<_> = rows.filter_map(Result::ok).collect();
        msgs.reverse();
        msgs
    }

    /// Ano, dia, mes, hora e minuto de agora (com e sem zero a esquerda): numeros que qualquer resposta pode citar.
    pub fn agora_partes(&self) -> Vec<String> {
        let Ok(db) = self.con.lock() else { return Vec::new() };
        let t: String = db.query_row("SELECT strftime('%Y %d %m %H %M','now','localtime')", [], |r| r.get(0)).unwrap_or_default();
        let mut partes: Vec<String> = t.split_whitespace().map(String::from).collect();
        let sem_zero: Vec<String> = partes.iter().map(|x| x.trim_start_matches('0').to_string()).filter(|x| !x.is_empty()).collect();
        partes.extend(sem_zero);
        partes
    }

    /// Segundos que ainda faltam no cooldown de *nome*.
    pub fn cooldown(&self, nome: &str) -> f64 {
        let Ok(db) = self.con.lock() else { return 0.0 };
        let sql = "SELECT ate - strftime('%s','now') FROM estado WHERE nome = ? AND ate IS NOT NULL";
        let diff: Option<f64> = db
            .query_row(sql, params![nome], |r| r.get(0))
            .optional()
            .unwrap_or(None);
        diff.unwrap_or(0.0).max(0.0)
    }

    /// Define/atualiza o cooldown de *nome* por *segundos* a partir de agora.
    /// O campo `erro` é truncado em 200 caracteres.
    pub fn set_cooldown(&self, nome: &str, segundos: f64, erro: &str) -> Result<(), String> {
        let db = self.con.lock().map_err(|_| "lock falhou".to_string())?;
        // corte por caractere: cortar por byte no meio de um acento derruba o programa
        let erro_trunc: String = erro.chars().take(200).collect();
        db.execute(
            "INSERT OR REPLACE INTO estado (nome, ate, erro) VALUES (?, strftime('%s','now') + ?, ?)",
            params![nome, segundos, erro_trunc],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    /// Remove o cooldown de *nome*.
    #[allow(dead_code)] // usado quando o painel de IAs for portado
    pub fn clear_cooldown(&self, nome: &str) -> Result<(), String> {
        let db = self.con.lock().map_err(|_| "lock falhou".to_string())?;
        db.execute("DELETE FROM estado WHERE nome = ?", params![nome])
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Retorna a mensagem de erro associada ao cooldown (ou string vazia).
    pub fn erro(&self, nome: &str) -> String {
        let Ok(db) = self.con.lock() else { return String::new() };
        let sql = "SELECT erro FROM estado WHERE nome = ?1";
        db.query_row(sql, params![nome], |r| r.get(0))
            .unwrap_or_else(|_| String::new())
    }

    /// Quantas vezes *nome* foi usado hoje.
    pub fn uso_hoje(&self, nome: &str) -> i64 {
        let Ok(db) = self.con.lock() else { return 0 };
        let sql = "SELECT n FROM uso WHERE dia = date('now','localtime') AND nome = ?";
        db.query_row(sql, params![nome], |r| r.get(0))
            .unwrap_or(0)
    }

    /// Incrementa o contador de uso diário de *nome*.
    pub fn add_uso(&self, nome: &str) -> Result<(), String> {
        let db = self.con.lock().map_err(|_| "lock falhou".to_string())?;
        db.execute(
            "INSERT INTO uso (dia, nome, n) VALUES (date('now','localtime'), ?, 1)
             ON CONFLICT(dia, nome) DO UPDATE SET n = n + 1",
            params![nome],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn test_historico() {
        let store = Store::em_memoria().unwrap();
        store
            .add_msg("c1", "user", "Oi", None)
            .unwrap();
        store
            .add_msg("c1", "bot", "Olá", None)
            .unwrap();
        store
            .add_msg("c1", "user", "Tudo bem?", None)
            .unwrap();

        // pede 2 últimas
        let hist: Vec<Value> = store.historico("c1", 2);
        assert_eq!(hist.len(), 2);
        assert_eq!(hist[0]["role"], "bot");
        assert_eq!(hist[1]["role"], "user");
    }

    #[test]
    fn test_cooldown() {
        let store = Store::em_memoria().unwrap();
        assert_eq!(store.cooldown("api"), 0.0);
        store.set_cooldown("api", 5.0, "tempo curto").unwrap();
        let cd = store.cooldown("api");
        assert!(cd > 4.0 && cd <= 5.0);
        store.clear_cooldown("api").unwrap();
        assert_eq!(store.cooldown("api"), 0.0);
        assert_eq!(store.erro("api"), "");
    }

    #[test]
    fn test_erro_truncado() {
        let store = Store::em_memoria().unwrap();
        let long = "á".repeat(250);
        store.set_cooldown("svc", 1.0, &long).unwrap();
        let e = store.erro("svc");
        assert_eq!(e.chars().count(), 200);
    }

    #[test]
    fn test_uso() {
        let store = Store::em_memoria().unwrap();
        assert_eq!(store.uso_hoje("mod"), 0);
        store.add_uso("mod").unwrap();
        store.add_uso("mod").unwrap();
        assert_eq!(store.uso_hoje("mod"), 2);
    }
}
