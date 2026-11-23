//! Caixa de saida LOCAL: o Braniac em Rust roda em paralelo ao sistema real e nao fala com ninguem. Tudo que um modulo "enviaria"
//! (mensagem, audio, aviso, resumo) vira uma linha em dados/saida.jsonl e fica so na tela /saida. Nada daqui sai do PC.
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;

/// Anota o que SERIA enviado. Devolve no formato de um envio: comeca com "✅" quando gravou (os modulos conferem isso).
pub fn registrar(dados: &Path, canal: &str, destino: &str, texto: &str) -> String {
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let linha = json!({ "ts": ts, "canal": canal, "destino": destino, "texto": texto });
    let gravou = std::fs::create_dir_all(dados)
        .and_then(|_| std::fs::OpenOptions::new().create(true).append(true).open(dados.join("saida.jsonl")))
        .and_then(|mut f| writeln!(f, "{linha}"));
    match gravou {
        Ok(()) => "✅ na caixa de saída (nada foi enviado)".into(),
        Err(e) => format!("não anotei na caixa de saída: {e}"),
    }
}

/// As ultimas n linhas, da mais nova para a mais antiga.
pub fn ultimas(dados: &Path, n: usize) -> Value {
    let t = std::fs::read_to_string(dados.join("saida.jsonl")).unwrap_or_default();
    json!(t.lines().rev().filter_map(|l| serde_json::from_str::<Value>(l).ok()).take(n).collect::<Vec<_>>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anota_no_formato_de_envio_e_lista_da_mais_nova() {
        let d = std::env::temp_dir().join(format!("braniac-saida-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        assert_eq!(ultimas(&d, 5), json!([]));
        assert!(registrar(&d, "alma", "Ana", "oi \"Ana\"\nlinha 2").starts_with('✅'));
        assert!(registrar(&d, "funil", "Bia", "resumo").starts_with('✅'));
        let u = ultimas(&d, 1);
        assert_eq!((u[0]["canal"].as_str(), u[0]["destino"].as_str(), u[0]["texto"].as_str()), (Some("funil"), Some("Bia"), Some("resumo")));
        assert_eq!(ultimas(&d, 9)[1]["texto"], "oi \"Ana\"\nlinha 2");
        assert!(ultimas(&d, 9)[1]["ts"].as_u64().unwrap() > 0);
        let _ = std::fs::remove_dir_all(&d);
    }
}
