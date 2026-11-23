//! TikTok: SEM robo de navegador (contra os termos, risco de perder a conta). O corte aprovado vai para dados/cortes/tiktok/
//! (video + legenda .txt pronta para colar) e o dono posta pelo celular. Opcional (`tiktok_rascunho_api`, depois do app aprovado): manda como
//! RASCUNHO para a caixa de entrada do app pela API oficial; o dono revisa e publica. Ficou de fora: a rede e o token (closures `iniciar` e `subir`).
#![allow(dead_code)]
use crate::cortes_config::sub;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const INIT: &str = "https://open.tiktokapis.com/v2/post/publish/inbox/video/init/";
pub const MAX_BYTES: u64 = 64 * 1024 * 1024;

pub fn legenda(corte: &Value) -> String {
    format!("{} {}", corte["titulo"].as_str().unwrap_or(""), corte["hashtags"].as_str().unwrap_or("")).trim().replace("#shorts", "#fyp")
}

/// Copia o video para a pasta do TikTok (por perfil) e grava a legenda ao lado. Devolve o destino.
pub fn preparar(dados: &Path, corte: &Value, perfil: &str) -> Result<PathBuf, String> {
    let pasta = sub(dados, &if perfil.is_empty() { "tiktok".to_string() } else { format!("tiktok/{perfil}") });
    let nome = format!("corte_{:04}", corte["id"].as_i64().ok_or("corte sem id")?);
    let destino = pasta.join(format!("{nome}.mp4"));
    std::fs::copy(corte["arquivo"].as_str().unwrap_or(""), &destino).map_err(|e| e.to_string())?;
    std::fs::write(pasta.join(format!("{nome}.txt")), legenda(corte)).map_err(|e| e.to_string())?;
    Ok(destino)
}

pub fn pedido_rascunho(tamanho: u64) -> Result<Value, String> {
    if tamanho > MAX_BYTES {
        return Err("video acima de 64 MB: envio em pedaco unico nao suportado aqui".into());
    }
    Ok(json!({"source_info": {"source": "FILE_UPLOAD", "video_size": tamanho, "chunk_size": tamanho, "total_chunk_count": 1}}))
}

pub fn cabecalhos_upload(tamanho: u64) -> Vec<(String, String)> {
    vec![("Content-Type".into(), "video/mp4".into()), ("Content-Length".into(), tamanho.to_string()), ("Content-Range".into(), format!("bytes 0-{}/{tamanho}", tamanho.saturating_sub(1)))]
}

/// Rascunho na caixa de entrada do app (Content Posting API). `iniciar(corpo)` -> (status HTTP, resposta); `subir(url, cabecalhos)` -> status HTTP.
/// Devolve o publish_id. Sem token nada sai.
pub fn enviar_rascunho(tamanho: u64, tem_token: bool, iniciar: &dyn Fn(&Value) -> Result<(u16, Value), String>, subir: &dyn Fn(&str, &[(String, String)]) -> Result<u16, String>) -> Result<String, String> {
    if !tem_token {
        return Err("sem TIKTOK_ACCESS_TOKEN no .env".into());
    }
    let (status, d) = iniciar(&pedido_rascunho(tamanho)?)?;
    if status != 200 || d["error"]["code"].as_str().unwrap_or("ok") != "ok" {
        return Err(format!("TikTok recusou: {}", d["error"]));
    }
    let (Some(url), Some(id)) = (d["data"]["upload_url"].as_str(), d["data"]["publish_id"].as_str()) else { return Err("TikTok respondeu sem upload_url".into()) };
    match subir(url, &cabecalhos_upload(tamanho))? {
        200 | 201 => Ok(id.to_string()),
        outro => Err(format!("upload TikTok HTTP {outro}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepara_pasta_por_perfil_com_legenda() {
        let d = crate::cortes_fila::tmp("tiktok");
        std::fs::create_dir_all(&d).unwrap();
        let v = d.join("v.mp4");
        std::fs::write(&v, vec![b'0'; 100]).unwrap();
        let c = json!({"id": 7, "arquivo": v.to_string_lossy(), "titulo": "T", "hashtags": "#shorts #x"});
        let dest = preparar(&d, &c, "").unwrap();
        assert!(dest.ends_with("cortes/tiktok/corte_0007.mp4") && dest.exists());
        assert_eq!(std::fs::read_to_string(d.join("cortes/tiktok/corte_0007.txt")).unwrap(), "T #fyp #x");
        assert!(preparar(&d, &c, "gringo").unwrap().ends_with("cortes/tiktok/gringo/corte_0007.mp4"));
        assert!(preparar(&d, &json!({"id": 8, "arquivo": "nao-existe.mp4"}), "").is_err());
    }

    #[test]
    fn rascunho_so_com_token_tamanho_e_resposta_boa() {
        let nunca = |_: &Value| -> Result<(u16, Value), String> { panic!("sem token nao chama a rede") };
        assert!(enviar_rascunho(10, false, &nunca, &|_, _| Ok(200)).is_err());
        assert!(enviar_rascunho(MAX_BYTES + 1, true, &nunca, &|_, _| Ok(200)).unwrap_err().contains("64 MB"));
        let ok = |c: &Value| {
            assert_eq!(c["source_info"]["video_size"], 10);
            Ok((200, json!({"error": {"code": "ok"}, "data": {"upload_url": "https://up", "publish_id": "p1"}})))
        };
        let subir = |u: &str, h: &[(String, String)]| {
            assert_eq!((u, h[2].1.as_str()), ("https://up", "bytes 0-9/10"));
            Ok(201)
        };
        assert_eq!(enviar_rascunho(10, true, &ok, &subir), Ok("p1".into()));
        assert!(enviar_rascunho(10, true, &ok, &|_, _| Ok(500)).unwrap_err().contains("HTTP 500"));
        assert!(enviar_rascunho(10, true, &|_| Ok((200, json!({"error": {"code": "spam_risk"}}))), &subir).unwrap_err().contains("recusou"));
        assert!(enviar_rascunho(10, true, &|_| Ok((401, json!({}))), &subir).is_err());
    }
}
