//! Avisos do Braniac para a pessoa (lembretes da agenda, rotinas do Compasso).
//! shortcut: so ficam na tela (dados/avisos.jsonl), nada sai do PC; trocar o corpo de `registrar` pelo envio de
//! verdade quando o modulo de mensagens for portado.
use crate::tempo::Agora;
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;

/// Mesmo contrato de um envio: resultado que comeca com "✅" quando deu certo.
pub fn registrar(dados: &Path, agora: &Agora, para: &str, texto: &str) -> String {
    let linha = json!({ "quando": format!("{} {}", agora.data.iso(), agora.hm()), "para": para, "texto": texto, "ts": crate::fatos::agora_ts() });
    match std::fs::OpenOptions::new().create(true).append(true).open(dados.join("avisos.jsonl")).and_then(|mut f| writeln!(f, "{linha}")) {
        Ok(()) => "✅ na tela de avisos".into(),
        Err(e) => format!("não registrei o aviso: {e}"),
    }
}

/// Os ultimos n avisos, do mais novo para o mais antigo.
pub fn ultimos(dados: &Path, n: usize) -> Value {
    let t = std::fs::read_to_string(dados.join("avisos.jsonl")).unwrap_or_default();
    json!(t.lines().rev().filter_map(|l| serde_json::from_str::<Value>(l).ok()).take(n).collect::<Vec<_>>())
}
