//! WhatsApp sem aba do navegador (sidecar local em 127.0.0.1): aqui ficam SO as regras.
//! Trava de destino: `jid_de` so devolve numero para nome IGUAL ao conhecido (sem tela para conferir o titulo, parcial nao vale: None e o
//! envio cai no caminho antigo, que confere). `aprender` guarda nome -> numero de quem mandou mensagem e nunca sobrescreve nome ja conhecido.
//! Ficou de fora: falar com o sidecar e subir o processo. Este modulo NAO envia nada: `corpo_enviar`/`corpo_arquivo` so montam o pedido, e
//! quem manda (com as travas de envio do Braniac) e quem integra. Os contatos ficam em dados/wpp_contatos.json (so no PC).
#![allow(dead_code)]
use crate::empresa_dev::{gravar, ler};
use crate::texto_pt::sem_acento;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

pub const PORTA: u16 = 8766;

fn arq(dados: &Path) -> PathBuf {
    dados.join("wpp_contatos.json")
}

/// Resposta do /status; sem resposta do sidecar = "parado" ("ocioso" = no ar, sem QR).
pub fn estado(status: Option<&Value>) -> &str {
    status.and_then(|s| s["estado"].as_str()).unwrap_or("parado")
}

pub fn conectado(status: Option<&Value>) -> bool {
    estado(status) == "conectado"
}

/// So sobe o sidecar se ele esta parado e as dependencias ja foram instaladas (pasta node_modules).
pub fn pode_subir(status: Option<&Value>, pasta_sidecar: &Path) -> bool {
    estado(status) == "parado" && pasta_sidecar.join("node_modules").exists()
}

pub fn corpo_enviar(para: &str, texto: &str) -> Value {
    json!({"para": para, "texto": texto})
}

pub fn corpo_arquivo(para: &str, caminho: &str, legenda: &str, voz: bool) -> Value {
    json!({"para": para, "caminho": caminho, "legenda": legenda, "voz": voz})
}

/// Resposta do /inbox: so vale se for lista.
pub fn novas(resposta: &Value) -> Vec<Value> {
    resposta.as_array().cloned().unwrap_or_default()
}

fn chave(n: &str) -> String {
    sem_acento(n).split_whitespace().collect::<Vec<_>>().join(" ")
}

fn contatos(dados: &Path) -> Map<String, Value> {
    ler(&arq(dados)).and_then(|v| v.as_object().cloned()).unwrap_or_default()
}

/// Aprende nome -> numero com quem mandou mensagem (nome de exibicao) e devolve o maior seq visto.
pub fn aprender(dados: &Path, mensagens: &[Value], desde: i64) -> Result<i64, String> {
    let (mut c, mut maior) = (contatos(dados), desde);
    for m in mensagens {
        maior = maior.max(m["seq"].as_i64().unwrap_or(desde));
        let (nome, de) = (m["nome"].as_str().unwrap_or(""), m["de"].as_str().unwrap_or(""));
        // so conversa de pessoa (grupo e transmissao tem outro sufixo) e so mensagem recebida
        if !chave(nome).is_empty() && m["minha"] != true && de.ends_with("@s.whatsapp.net") {
            c.entry(chave(nome)).or_insert_with(|| json!(de));
        }
    }
    gravar(&arq(dados), &Value::Object(c)).map(|_| maior)
}

pub fn jid_de(dados: &Path, nome: &str) -> Option<String> {
    contatos(dados).get(&chave(nome)).and_then(|v| v.as_str().map(String::from))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jid_so_para_nome_conhecido() {
        let d = std::env::temp_dir().join(format!("braniac-wpp_direto-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let msgs = [json!({"seq": 3, "de": "550000001@s.whatsapp.net", "nome": "Dona  Aná", "minha": false}), json!({"seq": 5, "de": "550000002@s.whatsapp.net", "nome": "Eu Mesmo", "minha": true}),
                    json!({"seq": 4, "de": "1203630@g.us", "nome": "Grupo da Rua"}), json!({"seq": 2, "de": "550000003@s.whatsapp.net", "nome": " "})];
        assert_eq!(aprender(&d, &msgs, 0), Ok(5));
        assert_eq!(jid_de(&d, "dona ana"), Some("550000001@s.whatsapp.net".into()));
        assert_eq!(jid_de(&d, "ana"), None); // parcial nao vale
        assert_eq!((jid_de(&d, "Eu Mesmo"), jid_de(&d, "grupo da rua"), jid_de(&d, "")), (None, None, None));
        // outro numero dizendo o mesmo nome nao toma o lugar de quem ja era conhecido
        assert_eq!(aprender(&d, &[json!({"seq": 6, "de": "559999999@s.whatsapp.net", "nome": "DONA ANA"})], 5), Ok(6));
        assert_eq!(jid_de(&d, "Dona Ana"), Some("550000001@s.whatsapp.net".into()));
        assert_eq!(aprender(&d, &[], 6), Ok(6));
    }

    #[test]
    fn parado_sem_sidecar() {
        assert_eq!((estado(None), conectado(None), conectado(Some(&json!({"estado": "conectado"}))), estado(Some(&json!({"estado": "ocioso"})))), ("parado", false, true, "ocioso"));
        let pasta = std::env::temp_dir().join(format!("braniac-wpp_direto-sidecar-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&pasta);
        assert!(!pode_subir(None, &pasta)); // sem as dependencias instaladas nao sobe
        std::fs::create_dir_all(pasta.join("node_modules")).unwrap();
        assert!(pode_subir(None, &pasta) && !pode_subir(Some(&json!({"estado": "ocioso"})), &pasta));
        assert!(novas(&json!({"estado": "parado"})).is_empty() && novas(&json!([{"seq": 1}])).len() == 1);
        assert_eq!(corpo_arquivo("n", "a.ogg", "", true)["voz"], true);
        assert_eq!(corpo_enviar("n", "oi"), json!({"para": "n", "texto": "oi"}));
    }
}
