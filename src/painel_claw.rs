//! Dados agregados da interface `/painel`. So LEITURA e so dado local que ja existe: agenda de hoje, rotinas, assuntos com a contagem
//! de fatos validos, manutencao, propostas pendentes, resumo do financeiro e da vaquinha, apps conectados. Nenhuma IA, nenhum segredo:
//! dos apps saem so nome e descricao (nunca o comando, que tem caminho e pode ter chave).
//! Ficou de fora: o texto da agenda de hoje (quem chama entrega pronto). O que e pessoal (agenda, financeiro) so deve ser servido na
//! interface local ou no tunel pareado do proprio dono: essa trava e de quem liga a rota.
#![allow(dead_code)]
use crate::empresa_dev::ler;
use crate::{agentes, compasso, fatos, manutencao, sauraki};
use serde_json::{json, Value};
use std::path::Path;

const APPS: [(&str, &str); 12] = [("photocraft", "Editor de imagens (camadas, PSD)"), ("vectorcraft", "Vetores, logos e cartazes"), ("wordcraft", "Documentos Word"), ("soundcraft", "Áudio (estilo Pro Tools)"),
    ("effectcraft", "Motion graphics"), ("pdfcraft", "PDF: editar, assinar, OCR"), ("lightcraft", "Revelar e organizar fotos (RAW)"), ("gridcraft", "Planilhas"),
    ("deckcraft", "Slides"), ("filmcraft", "Edição de vídeo"), ("designcraft", "Diagramação de páginas"), ("cadcraft", "Desenho técnico (CAD)")];

/// Assuntos do registro com o agente e quantos fatos validos cada um tem. Assunto sem banco = 0 (o banco nao e criado so por olhar).
pub fn assuntos_com_fatos(dados: &Path, ts: f64) -> Vec<Value> {
    agentes::listar(dados).iter().map(|a| {
        let id = a["id"].as_str().unwrap_or("");
        let n = if dados.join("assuntos").join(format!("{id}.db")).exists() { fatos::validos(dados, id, false, ts).len() } else { 0 };
        json!({ "id": id, "nome": a["nome"], "agente": a["agente"], "descricao": a["descricao"], "fatos": n })
    }).collect()
}

pub fn rotinas(dados: &Path) -> Vec<Value> {
    let t = |r: &Value, k: &str| r[k].as_str().unwrap_or("").to_string();
    compasso::listar(dados).iter().map(|r| json!({ "id": r["id"], "titulo": r["titulo"], "horario": t(r, "horario"), "dias": r["dias"].as_array().cloned().unwrap_or_default(),
        "ativa": r["ativa"].as_bool().unwrap_or(false), "para": t(r, "para"), "ate": t(r, "ate"), "ultima": t(r, "ultima") })).collect()
}

/// Apps conectados por MCP (`.mcp.json` na raiz do projeto). So nome e descricao.
pub fn plugins(raiz: &Path) -> Vec<Value> {
    let mcp = ler(&raiz.join(".mcp.json")).unwrap_or(Value::Null);
    mcp["mcpServers"].as_object().into_iter().flatten()
        .map(|(n, _)| json!({ "nome": n, "tipo": "App ArtCraft (MCP)", "descricao": APPS.iter().find(|(a, _)| a == n).map_or("Servidor MCP", |x| x.1) })).collect()
}

pub fn dados(dados: &Path, raiz: &Path, ts: f64, agenda_hoje: &str) -> Value {
    let fin = ler(&dados.join("financeiro.json")).filter(|v| v.as_object().is_some_and(|o| !o.is_empty()));
    let vq = ler(&dados.join("vaquinha.json")).filter(|v| v.as_object().is_some_and(|o| !o.is_empty()));
    json!({
        "ts": ts, "plugins": plugins(raiz), "agenda_hoje": agenda_hoje, "rotinas": rotinas(dados), "assuntos": assuntos_com_fatos(dados, ts),
        "manutencao": manutencao::ultimo(dados), "propostas_pendentes": sauraki::propostas_pendentes(dados),
        "financeiro": fin.map(|f| { let av = &f["avaliacao"]; json!({ "saldo": f["saldo"], "nivel": av["nivel"], "falta": av["falta"], "meta": av["meta"], "prazo": av["prazo"], "lido_em": f["saldo_ts"] }) }),
        "vaquinha": vq.map(|v| json!({ "ativa": v["ativo"].as_bool().unwrap_or(false), "recebido": v["arrecadado"], "gasto": v["gasto"], "meta": v["meta"], "informado_em": v["informado_em"] })),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tem_todas_as_secoes_conta_fatos_e_nao_vaza_comando_nem_extrato() {
        let d = std::env::temp_dir().join(format!("braniac-painel_claw-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let vazio = dados(&d, &d, 1000.0, "");
        for k in ["agenda_hoje", "rotinas", "assuntos", "manutencao", "propostas_pendentes", "plugins", "financeiro", "vaquinha"] {
            assert!(vazio.get(k).is_some(), "{k}");
        }
        assert!(vazio["plugins"] == json!([]) && vazio["financeiro"].is_null() && vazio["vaquinha"].is_null());
        assert!(!d.join("assuntos").exists()); // olhar nao cria banco
        std::fs::write(d.join(".mcp.json"), r#"{"mcpServers": {"photocraft": {"command": "C:/Users/x/app.exe", "env": {"CHAVE": "cfut_segredo"}}, "outro": {}}}"#).unwrap();
        std::fs::write(d.join("financeiro.json"), r#"{"saldo": 10.5, "saldo_ts": 9, "avaliacao": {"nivel": "ok", "meta": 100}, "extrato": [{"de": "Fulano de Tal", "valor": 7}]}"#).unwrap();
        std::fs::write(d.join("vaquinha.json"), r#"{"ativo": true, "arrecadado": 5, "meta": 50, "chave_pix": "000111"}"#).unwrap();
        fatos::registrar(&d, "braniac", "porta", "o servidor usa a porta de sempre", "publico", "teste", None, 900.0).unwrap();
        fatos::registrar(&d, "braniac", "velho", "valia so ontem", "publico", "teste", Some(950.0), 900.0).unwrap();
        let p = dados(&d, &d, 1000.0, "09:00 reunião");
        let txt = p.to_string();
        assert!(!txt.contains("cfut_") && !txt.contains("Users") && !txt.contains("Fulano") && !txt.contains("000111")); // nunca caminho, chave nem extrato
        assert_eq!(p["plugins"], json!([{ "nome": "outro", "tipo": "App ArtCraft (MCP)", "descricao": "Servidor MCP" }, { "nome": "photocraft", "tipo": "App ArtCraft (MCP)", "descricao": "Editor de imagens (camadas, PSD)" }]));
        assert!(p["financeiro"]["saldo"] == 10.5 && p["financeiro"]["nivel"] == "ok" && p["vaquinha"]["ativa"] == true && p["agenda_hoje"] == "09:00 reunião");
        let forja = p["assuntos"].as_array().unwrap().iter().find(|a| a["agente"] == "Forja").unwrap().clone();
        assert_eq!(forja["fatos"], 1); // o vencido nao conta
        assert!(p["assuntos"].as_array().unwrap().iter().all(|a| a["fatos"].is_u64()));
        let _ = std::fs::remove_dir_all(&d);
    }
}
