//! YouTube (Shorts): montagem do pedido de upload da API oficial (videos.insert) e do kit do canal de um perfil.
//! Ficou de fora: login OAuth (so o dono faz), token e a rede. `subir(arquivo, corpo, conta)` e a closure que envia e devolve o id do video.
//! TRAVA (a mais no Rust): `enviar` so chama `subir` para corte com status "aprovado"; e nada aqui e chamado sem passar pela fila.
#![allow(dead_code)]
use crate::cortes_config::pasta;
use crate::cortes_twitch::iso_utc;
use crate::empresa_dev::corta;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const ESCOPOS: [&str; 1] = ["https://www.googleapis.com/auth/youtube.upload"];
pub const ESCOPO_CANAL: &str = "https://www.googleapis.com/auth/youtube"; // perfis: tambem edita o "Sobre" e cria playlists

pub type Subir<'a> = &'a dyn Fn(&str, &Value, &str) -> Result<String, String>;

/// (segredo do cliente OAuth, token da conta). Cada perfil com canal proprio tem o seu token.
pub fn arqs(dados: &Path, conta: &str) -> (PathBuf, PathBuf) {
    let p = pasta(dados);
    (p.join("youtube_client_secret.json"), p.join(if conta.is_empty() { "youtube_token.json".to_string() } else { format!("youtube_token_{conta}.json") }))
}

pub fn escopos(conta: &str) -> Vec<&'static str> {
    if conta.is_empty() { ESCOPOS.to_vec() } else { vec![ESCOPOS[0], ESCOPO_CANAL] }
}

/// So olha se o arquivo do token existe; nunca le o conteudo.
pub fn configurado(dados: &Path, conta: &str) -> bool {
    arqs(dados, conta).1.exists()
}

pub fn falta_login(conta: &str) -> String {
    format!("YouTube nao configurado: rode 'python -m braniac.cortes youtube-login{}'", if conta.is_empty() { String::new() } else { format!(" --perfil {conta}") })
}

/// Corpo do videos.insert. `publicar_em` (carimbo): sobe PRIVADO e o YouTube publica sozinho nessa hora.
pub fn corpo(titulo: &str, descricao: &str, tags: &[String], privacidade: &str, categoria: &str, publicar_em: Option<f64>) -> Value {
    let descricao = if format!("{titulo}{descricao}").to_lowercase().contains("#shorts") { descricao.to_string() } else { format!("{descricao}\n\n#shorts").trim().to_string() };
    let mut c = json!({"snippet": {"title": corta(titulo, 100), "description": corta(&descricao, 4900), "tags": tags.iter().take(15).map(|t| t.trim_start_matches('#')).collect::<Vec<_>>(), "categoryId": categoria},
                       "status": {"privacyStatus": privacidade, "selfDeclaredMadeForKids": false}});
    if let Some(t) = publicar_em.filter(|t| *t != 0.0) {
        c["status"]["privacyStatus"] = json!("private");
        c["status"]["publishAt"] = json!(iso_utc(t).replace('Z', ".000Z"));
    }
    c
}

/// Sobe um corte APROVADO e devolve o id do YouTube.
pub fn enviar(corte: &Value, privacidade: &str, categoria: &str, conta: &str, publicar_em: Option<f64>, subir: Subir) -> Result<String, String> {
    if corte["status"] != "aprovado" {
        return Err(format!("corte #{} nao esta aprovado: nao sobe", corte["id"]));
    }
    let tags: Vec<String> = corte["hashtags"].as_str().unwrap_or("").split_whitespace().map(String::from).collect();
    subir(corte["arquivo"].as_str().unwrap_or(""), &corpo(corte["titulo"].as_str().unwrap_or(""), corte["descricao"].as_str().unwrap_or(""), &tags, privacidade, categoria, publicar_em), conta)
}

/// brandingSettings do canal com o kit de dados/cortes/canal_<perfil>.json por cima (Sobre, palavras-chave, idioma, pais).
/// Nao mexe em nome/foto/banner: isso o dono faz no YouTube Studio.
pub fn marca_do_kit(marca: &Value, kit: &Value) -> Value {
    let mut m = if marca.is_object() { marca.clone() } else { json!({}) };
    if !m["channel"].is_object() {
        m["channel"] = json!({});
    }
    let chaves: Vec<String> = kit["palavras_chave"].as_array().map(|a| a.iter().filter_map(|k| k.as_str()).map(|k| if k.contains(' ') { format!("\"{k}\"") } else { k.to_string() }).collect()).unwrap_or_default();
    m["channel"]["description"] = json!(corta(kit["sobre"].as_str().unwrap_or(""), 1000));
    m["channel"]["keywords"] = json!(corta(&chaves.join(" "), 500));
    m["channel"]["defaultLanguage"] = json!(kit["idioma"].as_str().unwrap_or("en"));
    m["channel"]["country"] = json!(kit["pais"].as_str().unwrap_or("US"));
    m
}

/// Corpos das playlists do kit que ainda nao existem no canal.
pub fn playlists_que_faltam(kit: &Value, existentes: &[String]) -> Vec<Value> {
    kit["playlists"].as_array().map(|a| a.as_slice()).unwrap_or(&[]).iter().filter(|p| p["titulo"].as_str().is_some_and(|t| !existentes.iter().any(|e| e == t)))
        .map(|p| json!({"snippet": {"title": p["titulo"], "description": p["descricao"].as_str().unwrap_or("")}, "status": {"privacyStatus": "public"}})).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn token_por_conta_e_corpo_do_upload() {
        let d = crate::cortes_fila::tmp("yt");
        assert_eq!(arqs(&d, "gringo").1.file_name().unwrap(), "youtube_token_gringo.json");
        assert_eq!(arqs(&d, "").1.file_name().unwrap(), "youtube_token.json");
        assert!(!configurado(&d, ""));
        assert_eq!((escopos("").len(), escopos("gringo").len()), (1, 2));
        assert!(falta_login("gringo").ends_with("youtube-login --perfil gringo'"));
        let tags: Vec<String> = (0..20).map(|i| format!("#t{i}")).collect();
        let c = corpo(&"T".repeat(150), "desc", &tags, "public", "20", None);
        assert_eq!(c["snippet"]["title"].as_str().unwrap().len(), 100);
        assert_eq!((c["snippet"]["description"].as_str(), c["snippet"]["tags"].as_array().unwrap().len(), c["snippet"]["tags"][0].as_str()), (Some("desc\n\n#shorts"), 15, Some("t0")));
        assert_eq!((c["status"]["privacyStatus"].as_str(), c["status"]["selfDeclaredMadeForKids"].as_bool(), c["status"].get("publishAt")), (Some("public"), Some(false), None));
        // agendado sobe sempre privado; quem publica na hora e o YouTube
        let a = corpo("Titulo #Shorts", "d", &[], "public", "20", Some(1767225600.0));
        assert_eq!((a["status"]["privacyStatus"].as_str(), a["status"]["publishAt"].as_str(), a["snippet"]["description"].as_str()), (Some("private"), Some("2026-01-01T00:00:00.000Z"), Some("d")));
    }

    #[test]
    fn so_corte_aprovado_sobe() {
        let enviados = RefCell::new(vec![]);
        let subir = |arq: &str, corpo: &Value, conta: &str| {
            enviados.borrow_mut().push(format!("{arq}|{}|{conta}", corpo["snippet"]["tags"]));
            Ok("vid1".to_string())
        };
        for status in ["renderizado", "rejeitado", "publicado", "erro_youtube"] {
            assert!(enviar(&json!({"id": 3, "status": status, "arquivo": "a.mp4"}), "private", "20", "", None, &subir).is_err());
        }
        assert!(enviados.borrow().is_empty());
        let ok = enviar(&json!({"id": 3, "status": "aprovado", "arquivo": "a.mp4", "titulo": "T", "descricao": "D", "hashtags": "#shorts #x"}), "private", "20", "gringo", None, &subir);
        assert_eq!((ok, enviados.borrow().clone()), (Ok("vid1".to_string()), vec!["a.mp4|[\"shorts\",\"x\"]|gringo".to_string()]));
    }

    #[test]
    fn kit_do_canal() {
        let kit = json!({"sobre": "Sobre o canal", "palavras_chave": ["twitch", "best clips"], "playlists": [{"titulo": "A"}, {"titulo": "B", "descricao": "db"}]});
        let m = marca_do_kit(&json!({"channel": {"title": "Nome"}, "image": {"x": 1}}), &kit);
        assert_eq!((m["channel"]["title"].as_str(), m["channel"]["keywords"].as_str(), m["channel"]["country"].as_str(), m["image"]["x"].as_i64()), (Some("Nome"), Some("twitch \"best clips\""), Some("US"), Some(1)));
        assert_eq!(marca_do_kit(&Value::Null, &kit)["channel"]["description"], "Sobre o canal");
        let f = playlists_que_faltam(&kit, &["A".to_string()]);
        assert_eq!((f.len(), f[0]["snippet"]["title"].as_str(), f[0]["snippet"]["description"].as_str()), (1, Some("B"), Some("db")));
    }
}
