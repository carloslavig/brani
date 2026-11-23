//! WhatsApp no Rust, 1a parte: SO LEITURA da lista de conversas (nome e nao lidas), na aba "whatsapp" do navegador da VM.
//! Nao abre conversa, nao marca como lida, nao envia nada. O login (QR code) e sempre do Dono, na tela da VM.
//! A aba e reservada: as ferramentas das IAs nao leem nem mexem nela (conteudo de WhatsApp nao vai a IA online).
use crate::abas;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::Path;

pub const AGENTE: &str = "whatsapp";
const URL: &str = "https://web.whatsapp.com/";

pub fn reservado(agente: &str) -> bool {
    agente.trim().eq_ignore_ascii_case(AGENTE)
}

/// Abre o WhatsApp Web na aba reservada (quem le o QR e o Dono) e devolve o estado.
pub fn abrir(dados: &Path) -> Result<Value, String> {
    abas::navegar(dados, AGENTE, URL)?;
    estado(dados)
}

pub fn estado(dados: &Path) -> Result<Value, String> {
    abas::whatsapp(dados).map(|v| resumo(&v)).map_err(|e| if e.contains("nao tem aba") { "o WhatsApp ainda nao foi aberto na VM (botao Abrir)".into() } else { e })
}

/// Pura. Tira linha repetida (a pagina lista a mesma conversa em dois elementos), soma as nao lidas e explica o estado.
pub fn resumo(v: &Value) -> Value {
    let estado = v["estado"].as_str().unwrap_or("carregando");
    let mut vistos = HashSet::new();
    let conversas: Vec<Value> = v["conversas"].as_array().into_iter().flatten()
        .filter(|c| c["nome"].as_str().is_some_and(|n| !n.trim().is_empty() && vistos.insert(n.to_string())))
        .map(|c| json!({ "nome": c["nome"], "nao_lidas": c["nao_lidas"].as_u64().unwrap_or(0) }))
        .collect();
    let nao_lidas: u64 = conversas.iter().map(|c| c["nao_lidas"].as_u64().unwrap_or(0)).sum();
    let aviso = match estado {
        "logado" => "",
        "qr" => "Falta ler o QR code na tela da VM (so o Dono faz o login).",
        _ => "A pagina ainda esta carregando.",
    };
    json!({ "estado": estado, "aviso": aviso, "nao_lidas": nao_lidas, "conversas": conversas })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resumo_tira_repetida_soma_e_explica() {
        let r = resumo(&json!({"estado": "logado", "conversas": [
            {"nome": "Ana", "nao_lidas": 2}, {"nome": "Ana", "nao_lidas": 2}, {"nome": " ", "nao_lidas": 9}, {"nome": "Bia"}]}));
        assert_eq!(r["conversas"].as_array().unwrap().len(), 2);
        assert_eq!(r["nao_lidas"], 2);
        assert_eq!(r["aviso"], "");
        assert!(resumo(&json!({"estado": "qr", "conversas": []}))["aviso"].as_str().unwrap().contains("QR"));
        assert_eq!(resumo(&json!(null))["estado"], "carregando");
    }

    #[test]
    fn aba_e_reservada() {
        assert!(reservado(" WhatsApp ") && !reservado("Jarvis"));
    }
}
