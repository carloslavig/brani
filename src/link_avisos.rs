//! Caixa de avisos do Link (link/avisos.py): tudo o que o Braniac diz ao par fica aqui, inteiro e numerado, e o par le o que
//! ainda nao viu (seq > desde). Enquanto o par ler a caixa (ultima leitura ha menos de 30 min), o aviso conta como entregue por
//! ela. O mesmo texto de novo em menos de 6 h e reenvio, nao aviso novo. Arquivo dados/link/avisos.json no formato do Python.
//! Daqui nada e enviado: a caixa so guarda; quem serve a leitura ao par (ja autenticado) e quem chama.
#![allow(dead_code)]
use crate::link_identidade::{hex, sha256};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const ATIVA_S: f64 = 30.0 * 60.0; // par que leu a caixa nos ultimos 30 min esta lendo por ela
pub const REPETIDO_S: f64 = 6.0 * 3600.0;
static TRAVA: Mutex<()> = Mutex::new(());

fn arq(dados: &Path) -> PathBuf {
    dados.join("link").join("avisos.json")
}

fn ler_arq(dados: &Path) -> Value {
    let mut d: Value = std::fs::read_to_string(arq(dados)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    if !d["seq"].is_i64() || !d["avisos"].is_array() || !d["leituras"].is_object() {
        d = json!({ "seq": d["seq"].as_i64().unwrap_or(0), "avisos": d["avisos"].as_array().cloned().unwrap_or_default(), "leituras": d["leituras"].as_object().cloned().unwrap_or_default() });
    }
    d
}

fn gravar(dados: &Path, d: &Value) -> Result<(), String> {
    let a = arq(dados);
    std::fs::create_dir_all(dados.join("link")).map_err(|e| e.to_string())?;
    let tmp = a.with_extension("tmp");
    std::fs::write(&tmp, d.to_string()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &a).map_err(|e| e.to_string())
}

/// Poe o aviso na caixa do par. Devolve o numero (o mesmo de antes se for reenvio do mesmo texto).
pub fn guardar(dados: &Path, par: &str, texto: &str, agora: f64) -> Result<i64, String> {
    let h = hex(&sha256(texto.as_bytes()))[..16].to_string();
    let _t = TRAVA.lock().map_err(|e| e.to_string())?;
    let mut d = ler_arq(dados);
    let mut avisos = d["avisos"].as_array().cloned().unwrap_or_default();
    if let Some(a) = avisos.iter().rev().find(|a| a["par"] == par && a["h"] == h.as_str() && agora - a["quando"].as_f64().unwrap_or(0.0) < REPETIDO_S) {
        return a["seq"].as_i64().ok_or_else(|| "aviso sem numero".to_string());
    }
    let seq = d["seq"].as_i64().unwrap_or(0) + 1;
    avisos.push(json!({ "seq": seq, "par": par, "quando": agora, "h": h, "texto": texto }));
    let corte = avisos.len().saturating_sub(500);
    d["seq"] = json!(seq);
    d["avisos"] = json!(avisos[corte..]);
    gravar(dados, &d)?;
    Ok(seq)
}

/// O que o par ainda nao viu (seq > desde), so dele. Marca a leitura: a partir dai a caixa vale como entrega.
pub fn ler(dados: &Path, par: &str, desde: i64, agora: f64) -> Result<Value, String> {
    let _t = TRAVA.lock().map_err(|e| e.to_string())?;
    let mut d = ler_arq(dados);
    d["leituras"][par] = json!(agora);
    gravar(dados, &d)?;
    let novos: Vec<Value> = d["avisos"].as_array().into_iter().flatten().filter(|a| a["par"] == par && a["seq"].as_i64().is_some_and(|s| s > desde))
        .map(|a| json!({ "seq": a["seq"], "quando": a["quando"], "texto": a["texto"] })).collect();
    Ok(json!({ "avisos": novos, "ultimo": d["seq"] }))
}

pub fn ativa(dados: &Path, par: &str, agora: f64) -> bool {
    agora - ler_arq(dados)["leituras"][par].as_f64().unwrap_or(0.0) < ATIVA_S
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caixa_numerada_por_par_sem_repetir() {
        let d = std::env::temp_dir().join(format!("braniac-link_avisos-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        assert!(!ativa(&d, "bia", 10_000.0));
        assert_eq!(guardar(&d, "bia", "video 150 pronto", 10_000.0), Ok(1));
        assert_eq!(guardar(&d, "bia", "video 150 pronto", 10_300.0), Ok(1)); // reenvio do mesmo texto
        assert_eq!(guardar(&d, "ana", "video 150 pronto", 10_300.0), Ok(2)); // outro par: aviso dele
        assert_eq!(guardar(&d, "bia", "video 151 pronto", 10_400.0), Ok(3));
        assert_eq!(guardar(&d, "bia", "video 150 pronto", 10_000.0 + REPETIDO_S + 1.0), Ok(4)); // depois de 6 h e aviso novo
        let r = ler(&d, "bia", 1, 40_000.0).unwrap();
        let seqs: Vec<i64> = r["avisos"].as_array().unwrap().iter().map(|a| a["seq"].as_i64().unwrap()).collect();
        assert_eq!((seqs, r["ultimo"].as_i64()), (vec![3, 4], Some(4))); // nunca ve o aviso do outro par
        assert!(r["avisos"][0].get("par").is_none() && r["avisos"][0]["texto"] == "video 151 pronto");
        assert!(ativa(&d, "bia", 40_000.0 + ATIVA_S - 1.0) && !ativa(&d, "bia", 40_000.0 + ATIVA_S + 1.0) && !ativa(&d, "ana", 40_000.0));
        for i in 0..510 {
            guardar(&d, "bia", &format!("aviso {i}"), 50_000.0).unwrap();
        }
        assert_eq!(ler_arq(&d)["avisos"].as_array().unwrap().len(), 500); // a caixa guarda os 500 ultimos
        std::fs::write(arq(&d), "lixo").unwrap();
        assert_eq!(guardar(&d, "bia", "depois do arquivo estragado", 60_000.0), Ok(1));
        let _ = std::fs::remove_dir_all(&d);
    }
}
