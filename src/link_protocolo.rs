//! Envelope do BRANIAC LINK (link/protocolo.py): JSON assinado pelo dispositivo. Tudo que chega e dado nao confiavel: `validar`
//! antes de qualquer uso (tamanho, versao, tipo conhecido, identidade = chave, assinatura, data dentro da tolerancia, sem repeticao).
//! A assinatura Ed25519 nao mora aqui (nao ha crate de criptografia): `assinar` e `verificar` entram como closure.
//! Limite conhecido: a forma canonica bate com a do Python para texto, inteiro, lista e objeto; numero quebrado no corpo pode
//! sair escrito diferente nos dois lados (a assinatura nao confere e o envelope e recusado, nunca aceito por engano).
#![allow(dead_code)]
use crate::link_identidade::{id_de, id_novo, Ident};
use serde_json::{json, Value};
use std::collections::HashMap;

pub const VERSAO: i64 = 1;
pub const TIPOS: [&str; 18] = ["hello", "ping", "pong", "msg", "caps.pedir", "caps", "tarefa.pedido", "tarefa.aceita", "tarefa.pendente", "tarefa.recusada",
    "tarefa.resultado", "arquivo.oferta", "arquivo.aceita", "arquivo.recusa", "arquivo.fim", "arquivo.ok", "arquivo.falta", "bye"];
pub const MAX_ENVELOPE: usize = 64 * 1024;
pub const TOLERANCIA_S: f64 = 300.0;

/// O que e assinado: o envelope sem "sig", chaves em ordem, sem espacos.
pub fn canonico(env: &Value) -> Vec<u8> {
    let mut e = env.clone();
    if let Some(o) = e.as_object_mut() {
        o.remove("sig");
    }
    ordenado(&e).into_bytes()
}

// escrito a mao para a ordem das chaves nao depender de como o serde_json foi compilado
fn ordenado(v: &Value) -> String {
    match v {
        Value::Object(o) => {
            let mut ks: Vec<&String> = o.keys().collect();
            ks.sort();
            format!("{{{}}}", ks.iter().map(|k| format!("{}:{}", Value::String((*k).clone()), ordenado(&o[*k]))).collect::<Vec<_>>().join(","))
        }
        Value::Array(a) => format!("[{}]", a.iter().map(ordenado).collect::<Vec<_>>().join(",")),
        outro => outro.to_string(),
    }
}

/// Monta e assina um envelope. Tipo fora da lista e recusado (nao existe "executar comando").
pub fn criar(ident: &Ident, assinar: &dyn Fn(&[u8]) -> String, tipo: &str, corpo: Option<Value>, para: &str, ref_: &str, agora: i64) -> Result<String, String> {
    if !TIPOS.contains(&tipo) {
        return Err(format!("tipo desconhecido: {tipo}"));
    }
    let corpo = corpo.unwrap_or_else(|| json!({}));
    if !corpo.is_object() {
        return Err("tipo ou corpo invalido".into());
    }
    let mut env = json!({ "link": VERSAO, "tipo": tipo, "id": id_novo(16), "de": ident.id, "pub": ident.chave_pub, "para": para, "ref": ref_, "ts": agora, "corpo": corpo });
    env["sig"] = json!(assinar(&canonico(&env)));
    Ok(env.to_string())
}

/// Lembra os ids vistos na janela de tolerancia: reenviar o mesmo envelope e rejeitado.
#[derive(Default)]
pub struct Antirepeticao {
    vistos: HashMap<String, f64>,
}

impl Antirepeticao {
    pub fn novo(&mut self, env_id: &str, agora: f64) -> bool {
        self.vistos.retain(|_, t| agora - *t < 2.0 * TOLERANCIA_S);
        self.vistos.insert(env_id.to_string(), agora).is_none()
    }
}

/// Texto JSON -> envelope, com a assinatura, o id e a data conferidos. Err se algo nao fecha.
/// `verificar(chave publica, dados, assinatura)`.
pub fn validar(texto: &str, verificar: &dyn Fn(&str, &[u8], &str) -> bool, anti: Option<&mut Antirepeticao>, agora: f64) -> Result<Value, String> {
    if texto.len() > MAX_ENVELOPE {
        return Err("envelope grande demais ou invalido".into());
    }
    let env: Value = serde_json::from_str(texto).map_err(|_| "nao e JSON")?;
    if !env.is_object() || env["link"].as_i64() != Some(VERSAO) {
        return Err("versao do protocolo nao suportada".into());
    }
    if !env["tipo"].as_str().is_some_and(|t| TIPOS.contains(&t)) || !env["corpo"].is_object() {
        return Err("tipo ou corpo invalido".into());
    }
    for campo in ["id", "de", "pub", "sig"] {
        if env[campo].as_str().is_none_or(str::is_empty) {
            return Err(format!("falta {campo}"));
        }
    }
    let (de, chave, sig, id) = (env["de"].as_str().unwrap_or(""), env["pub"].as_str().unwrap_or(""), env["sig"].as_str().unwrap_or(""), env["id"].as_str().unwrap_or(""));
    if id_de(chave).map_err(|_| "chave invalida")? != de || !verificar(chave, &canonico(&env), sig) {
        return Err("assinatura ou identidade nao confere".into());
    }
    if !env["ts"].as_i64().is_some_and(|ts| (agora - ts as f64).abs() <= TOLERANCIA_S) {
        return Err("data fora da tolerancia".into());
    }
    if let Some(a) = anti {
        if !a.novo(id, agora) {
            return Err("envelope repetido".into());
        }
    }
    Ok(env)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link_identidade::falso;

    fn novo(i: &Ident, tipo: &str, corpo: Option<Value>, agora: i64) -> Result<String, String> {
        criar(i, &|d| falso::assinar(i, d), tipo, corpo, "", "", agora)
    }

    #[test]
    fn envelope_valido_e_adulterado() {
        let (a, b) = (falso::ident("A"), falso::ident("B"));
        let txt = novo(&a, "msg", Some(json!({ "texto": "olá", "z": [1, { "b": 2, "a": 1 }] })), 1000).unwrap();
        let env = validar(&txt, &falso::verificar, None, 1000.0).unwrap();
        assert_eq!(env["corpo"]["texto"], "olá");
        assert!(String::from_utf8(canonico(&env)).unwrap().starts_with("{\"corpo\":{\"texto\":\"olá\",\"z\":[1,{\"a\":1,\"b\":2}]},\"de\":\"bl1-"));
        let mut adulterado = env.clone();
        adulterado["corpo"]["texto"] = json!("outro");
        assert_eq!(validar(&adulterado.to_string(), &falso::verificar, None, 1000.0).unwrap_err(), "assinatura ou identidade nao confere");
        let mut trocado = env.clone(); // outra identidade com a assinatura de A
        trocado["de"] = json!(b.id);
        trocado["pub"] = json!(b.chave_pub);
        assert!(validar(&trocado.to_string(), &falso::verificar, None, 1000.0).is_err());
        let mut so_o_id = env.clone(); // chave de A dizendo ser B
        so_o_id["de"] = json!(b.id);
        assert!(validar(&so_o_id.to_string(), &|_, _, _| true, None, 1000.0).is_err());
        assert!(validar(&txt, &|_, _, _| false, None, 1000.0).is_err());
    }

    #[test]
    fn envelope_repetido_antigo_grande_e_tipo_desconhecido() {
        let a = falso::ident("A");
        let txt = novo(&a, "ping", None, 1000).unwrap();
        let mut anti = Antirepeticao::default();
        validar(&txt, &falso::verificar, Some(&mut anti), 1000.0).unwrap();
        assert_eq!(validar(&txt, &falso::verificar, Some(&mut anti), 1001.0).unwrap_err(), "envelope repetido");
        assert_eq!(validar(&txt, &falso::verificar, None, 5000.0).unwrap_err(), "data fora da tolerancia");
        assert_eq!(validar(&txt, &falso::verificar, None, 600.0).unwrap_err(), "data fora da tolerancia"); // do futuro tambem
        assert!(validar(&"x".repeat(70000), &falso::verificar, None, 1000.0).is_err());
        assert_eq!(novo(&a, "executar_comando", None, 1000).unwrap_err(), "tipo desconhecido: executar_comando");
        assert!(novo(&a, "msg", Some(json!("texto solto")), 1000).is_err());
        let env: Value = serde_json::from_str(&txt).unwrap();
        for (campo, valor) in [("link", json!(2)), ("tipo", json!("executar_comando")), ("corpo", json!("x")), ("sig", json!("")), ("ts", json!("1000")), ("pub", json!("!!"))] {
            let mut e = env.clone();
            e[campo] = valor;
            assert!(validar(&e.to_string(), &|_, _, _| true, None, 1000.0).is_err(), "{campo}");
        }
        assert!(validar("[1]", &falso::verificar, None, 1000.0).is_err() && validar("oi", &falso::verificar, None, 1000.0).is_err());
        assert!(anti.novo("x", 1.0) && !anti.novo("x", 2.0) && anti.novo("x", 2.0 + 2.0 * TOLERANCIA_S)); // depois da janela esquece
    }
}
