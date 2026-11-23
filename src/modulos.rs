//! Modulos opcionais: a base sempre liga, o resto so roda se a pessoa escolheu.
//! Porte de braniac/modulos.py. Sem dados/modulos.json (instalacao antiga) = tudo ligado.
use serde_json::{json, Value};
use std::{fs, path::Path};

pub fn catalogo() -> Vec<Value> {
    serde_json::from_str(include_str!("../padrao/modulos.json")).expect("padrao/modulos.json invalido")
}

fn ids() -> Vec<String> {
    catalogo().iter().map(|m| m["id"].as_str().unwrap_or("").to_string()).collect()
}

fn ler(dados: &Path) -> Option<Value> {
    serde_json::from_str(&fs::read_to_string(dados.join("modulos.json")).ok()?).ok()
}

pub fn ativos(dados: &Path) -> Vec<String> {
    let ids = ids();
    match ler(dados) {
        None => ids,
        Some(d) => d["ativos"].as_array().into_iter().flatten()
            .filter_map(|v| v.as_str().map(String::from))
            .filter(|i| ids.contains(i)).collect(),
    }
}

/// Grava a escolha; ignora id desconhecido e repetido. Devolve o que ficou.
pub fn definir(dados: &Path, pedidos: &[String]) -> std::io::Result<Vec<String>> {
    let ids = ids();
    let mut ok: Vec<String> = Vec::new();
    for p in pedidos {
        if ids.contains(p) && !ok.contains(p) {
            ok.push(p.clone());
        }
    }
    fs::create_dir_all(dados)?;
    fs::write(dados.join("modulos.json"), json!({ "ativos": ok }).to_string())?;
    Ok(ok)
}

pub fn estado(dados: &Path) -> Value {
    let on = ativos(dados);
    let lista: Vec<Value> = catalogo().into_iter().map(|mut m| {
        m["ativo"] = json!(on.iter().any(|i| Some(i.as_str()) == m["id"].as_str()));
        m
    }).collect();
    json!({ "configurado": ler(dados).is_some(), "modulos": lista })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sem_arquivo_tudo_ligado_e_definir_filtra() {
        let d = std::env::temp_dir().join(format!("braniac-rs-teste-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        assert_eq!(ativos(&d).len(), catalogo().len());
        assert_eq!(estado(&d)["configurado"], false);
        let pedido = ["airbnb".to_string(), "nao-existe".into(), "airbnb".into()];
        assert_eq!(definir(&d, &pedido).unwrap(), vec!["airbnb"]);
        assert_eq!(ativos(&d), vec!["airbnb"]);
        assert_eq!(estado(&d)["configurado"], true);
        let _ = fs::remove_dir_all(&d);
    }
}
