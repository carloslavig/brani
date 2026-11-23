//! Andares do predio (/sauraki) que estao construidos: dados/predio.json {"andares": [...]}. Sem o arquivo = predio inteiro.
//! Construir so MOSTRA o andar; nao liga nem desliga rotina. Ficou de fora: descobrir sozinho quais modulos nao vieram na instalacao
//! (no Python e um import); aqui quem chama passa `indisponiveis`. O teste que confere a ordem com a tela (HTML) tambem ficou de fora.
#![allow(dead_code)]
use crate::empresa_dev::{gravar, ler};
use serde_json::{json, Value};
use std::path::Path;

/// Da base ao topo; a tela (ORDEM_ANDAR) usa a mesma lista invertida.
pub const TODOS: [&str; 13] = ["terreo", "servidores", "dormitorio2", "dormitorio1", "braincenter", "compras", "esteira", "cortes", "investimentos", "marketing", "nexodev", "escritorio", "cobertura"];
pub const FIXOS: [&str; 3] = ["terreo", "escritorio", "cobertura"];

fn construidos(dados: &Path) -> Option<Vec<String>> {
    ler(&dados.join("predio.json"))?["andares"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
}

pub fn estado(dados: &Path, indisponiveis: &[&str]) -> Value {
    json!({ "andares": construidos(dados), "todos": TODOS, "indisponiveis": indisponiveis })
}

/// Acrescenta o andar na ordem do predio. Andar desconhecido ou indisponivel e recusado.
pub fn construir(dados: &Path, andar: &str, indisponiveis: &[&str]) -> Result<Value, String> {
    if !TODOS.contains(&andar) || indisponiveis.contains(&andar) {
        return Err("andar desconhecido".into());
    }
    let atual = construidos(dados).unwrap_or_else(|| TODOS.iter().map(|s| s.to_string()).collect());
    let novo: Vec<&str> = TODOS.iter().copied().filter(|k| atual.iter().any(|a| a == k) || *k == andar || FIXOS.contains(k)).collect();
    gravar(&dados.join("predio.json"), &json!({ "andares": novo }))?;
    Ok(estado(dados, indisponiveis))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sem_arquivo_e_o_predio_todo_e_construir_acrescenta_na_ordem() {
        let d = std::env::temp_dir().join(format!("braniac-andares-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        assert!(estado(&d, &["esteira"])["andares"].is_null());
        gravar(&d.join("predio.json"), &json!({ "andares": FIXOS })).unwrap();
        assert_eq!(construir(&d, "nexodev", &["esteira"]).unwrap()["andares"], json!(["terreo", "nexodev", "escritorio", "cobertura"]));
        assert!(construir(&d, "esteira", &["esteira"]).is_err());
        assert!(construir(&d, "<script>", &[]).is_err());
        assert!(FIXOS.iter().all(|f| TODOS.contains(f)));
        let _ = std::fs::remove_dir_all(&d);
    }
}
