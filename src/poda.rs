//! Poda de resultados antigos de ferramenta no laço do agente (ideia do "microcompact" do OpenClaude, reescrita aqui).
//! Só encurta o que vai para a IA: o `turno` guardado (usado pelo vigia contra invenção) não muda.
//! Desligada por padrão: ligue com `"poda_contexto": true` na config do roteador.
use serde_json::{json, Value};

/// Mantém inteiros os `manter` resultados de ferramenta mais recentes; os mais antigos e maiores que `limite` caracteres são cortados.
pub fn podar(turno: &[Value], manter: usize, limite: usize) -> Vec<Value> {
    let total = turno.iter().filter(|m| m["role"] == "tool").count();
    let mut vistos = 0;
    turno.iter().map(|m| {
        if m["role"] != "tool" { return m.clone(); }
        vistos += 1;
        let t = m["content"].as_str().unwrap_or("");
        if total - vistos < manter || t.chars().count() <= limite { return m.clone(); }
        let mut novo = m.clone();
        novo["content"] = json!(format!("{}\n[…resultado antigo cortado para economizar…]", t.chars().take(limite).collect::<String>()));
        novo
    }).collect()
}

#[cfg(test)]
mod testes {
    use super::*;

    fn tool(t: &str) -> Value { json!({ "role": "tool", "content": t }) }

    #[test]
    fn poda_so_os_resultados_antigos_e_grandes() {
        let grande = "x".repeat(1000);
        let turno = vec![json!({ "role": "assistant", "content": "oi" }), tool(&grande), tool(&grande), tool(&grande), tool("curto")];
        let p = podar(&turno, 2, 100);
        assert!(p[1]["content"].as_str().unwrap().contains("cortado"));
        assert_eq!(p[1]["content"].as_str().unwrap().chars().filter(|c| *c == 'x').count(), 100);
        assert_eq!(p[3]["content"].as_str().unwrap().len(), 1000);
        assert_eq!(p[4]["content"], "curto");
        assert_eq!(p[0], turno[0]);
        assert_eq!(turno[1]["content"].as_str().unwrap().len(), 1000);
    }

    #[test]
    fn poda_sem_ferramentas_nao_faz_nada() {
        let turno = vec![json!({ "role": "user", "content": "a".repeat(500) })];
        assert_eq!(podar(&turno, 2, 10), turno);
    }
}
