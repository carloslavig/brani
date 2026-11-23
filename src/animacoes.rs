//! Animacoes prontas para reels (braniac/animacoes.py): graph, minimalistic-graphics, map-route e breakdown, geradas de um JSON, sem IA.
//! Travas: so animacao da lista; a especificacao tem de ser um objeto; palavra de segredo/dado pessoal na especificacao = recusado; teto de 8
//! por dia (dados/animacoes_uso.json). De fora (closure): rodar o gerador (processo externo) na pasta do pedido.
#![allow(dead_code)]
use crate::tempo::Agora;
use regex::Regex;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const ATIVAS: [&str; 4] = ["breakdown", "graph", "map-route", "minimalistic-graphics"];
pub const MAX_DIA: i64 = 8;
const SEGREDO: &str = r"(?i)senha|token|cpf|cart[aã]o|pix|chave de acesso|c[oó]digo de verifica";

fn usos_hoje(dados: &Path, hoje: &str) -> i64 {
    let d: Value = std::fs::read_to_string(dados.join("animacoes_uso.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    if d["dia"] == hoje { d["n"].as_i64().unwrap_or(0) } else { 0 }
}

/// Nome do arquivo do pedido: o gerador reconhece o tipo pelo meio do nome.
pub fn arquivo_do_pedido(skill: &str) -> String {
    format!("pedido.{}.json", if skill == "minimalistic-graphics" { "minimal" } else { skill.split('-').next().unwrap_or(skill) })
}

/// Valida, conta o uso do dia, grava o pedido em `saida/<carimbo>-<skill>/` e chama `rodar(skill, arquivo do pedido)`, que devolve o mp4.
/// `instalado` = o gerador dessa animacao existe no PC.
pub fn gerar(dados: &Path, saida: &Path, agora: &Agora, skill: &str, spec: &Value, instalado: bool, rodar: &dyn Fn(&str, &Path) -> Result<PathBuf, String>) -> Result<PathBuf, String> {
    if !ATIVAS.contains(&skill) {
        return Err(format!("animacao desconhecida: {skill}. Disponiveis aqui: {} (zach-d-films e stick-figure: peca ao Claude)", ATIVAS.join(", ")));
    }
    if !spec.is_object() {
        return Err("spec precisa ser um objeto JSON".into());
    }
    if Regex::new(SEGREDO).expect("regex fixa").is_match(&spec.to_string()) {
        return Err("spec com palavra de segredo/dado pessoal: recusado".into());
    }
    if !instalado {
        return Err("animacoes nao instaladas (falta o gerador ou o kit)".into());
    }
    let (hoje, n) = (agora.data.iso(), usos_hoje(dados, &agora.data.iso()));
    if n >= MAX_DIA {
        return Err(format!("teto de {MAX_DIA} animacoes por dia"));
    }
    std::fs::create_dir_all(dados).map_err(|e| e.to_string())?;
    std::fs::write(dados.join("animacoes_uso.json"), json!({ "dia": hoje, "n": n + 1 }).to_string()).map_err(|e| e.to_string())?;
    let pasta = saida.join(format!("{}-{skill}", agora.carimbo()));
    std::fs::create_dir_all(&pasta).map_err(|e| e.to_string())?;
    let arq = pasta.join(arquivo_do_pedido(skill));
    std::fs::write(&arq, serde_json::to_string_pretty(spec).unwrap_or_default()).map_err(|e| e.to_string())?;
    rodar(skill, &arq).map_err(|e| format!("a animacao nao saiu: {}", e.chars().rev().take(300).collect::<Vec<_>>().into_iter().rev().collect::<String>()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;
    use std::cell::RefCell;

    #[test]
    fn so_animacao_conhecida_sem_segredo_e_ate_8_por_dia() {
        let d = std::env::temp_dir().join(format!("braniac-animacoes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let (saida, agora) = (d.join("saida"), Agora { data: Data::civil(2026, 11, 16).unwrap(), hora: 9, min: 0, seg: 0 });
        let rodou = RefCell::new(Vec::new());
        let rodar = |s: &str, arq: &Path| {
            rodou.borrow_mut().push(format!("{s}|{}", arq.file_name().unwrap().to_string_lossy()));
            assert!(arq.exists());
            Ok(arq.with_extension("mp4"))
        };
        let spec = json!({ "titulo": "Crescimento", "pontos": [1, 2, 3] });
        assert!(gerar(&d, &saida, &agora, "zach-d-films", &spec, true, &rodar).unwrap_err().contains("animacao desconhecida"));
        assert!(gerar(&d, &saida, &agora, "../graph", &spec, true, &rodar).is_err());
        assert_eq!(gerar(&d, &saida, &agora, "graph", &json!([1]), true, &rodar).unwrap_err(), "spec precisa ser um objeto JSON");
        for ruim in [json!({ "titulo": "minha senha nova" }), json!({ "a": { "b": ["chave PIX do cliente"] } }), json!({ "Cartão": 1 })] {
            assert!(gerar(&d, &saida, &agora, "graph", &ruim, true, &rodar).unwrap_err().contains("recusado"));
        }
        assert!(gerar(&d, &saida, &agora, "graph", &spec, false, &rodar).unwrap_err().contains("nao instaladas"));
        assert!(rodou.borrow().is_empty() && !d.join("animacoes_uso.json").exists()); // recusa nao gasta o teto
        for (i, s) in ATIVAS.iter().cycle().take(8).enumerate() {
            let a = Agora { seg: i as u32, ..agora };
            assert!(gerar(&d, &saida, &a, s, &spec, true, &rodar).unwrap().to_string_lossy().ends_with(".mp4"));
        }
        assert!(rodou.borrow()[1] == "graph|pedido.graph.json" && rodou.borrow()[2] == "map-route|pedido.map.json" && rodou.borrow()[3] == "minimalistic-graphics|pedido.minimal.json");
        assert_eq!(gerar(&d, &saida, &agora, "graph", &spec, true, &rodar).unwrap_err(), "teto de 8 animacoes por dia");
        let amanha = Agora { data: Data(agora.data.0 + 1), ..agora };
        assert!(gerar(&d, &saida, &amanha, "graph", &spec, true, &|_, _| Err("erro do gerador".into())).unwrap_err().ends_with("erro do gerador")); // outro dia: o teto zera
        let _ = std::fs::remove_dir_all(&d);
    }
}
