//! Twitch: leitura das respostas da API oficial (Helix) e das listas do yt-dlp para live, clipes e VODs.
//! Ficou de fora: a rede e as chaves. `get(caminho, parametros)` e a closure que fala com a Helix (token de aplicativo, sem login de ninguem)
//! e devolve a lista "data"; a lista do yt-dlp (`--flat-playlist -J`, campo "entries") chega pronta. Streamlink ficou em `cortes_captura`.
#![allow(dead_code)]
use crate::tempo::Data;
use serde_json::{json, Value};

pub type Get<'a> = &'a dyn Fn(&str, &[(&str, String)]) -> Result<Vec<Value>, String>;

fn s(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or("").to_string()
}

/// Carimbo -> "AAAA-MM-DDTHH:MM:SSZ" (UTC).
pub fn iso_utc(ts: f64) -> String {
    let t = ts.floor() as i64;
    let r = t.rem_euclid(86400);
    format!("{}T{:02}:{:02}:{:02}Z", Data(t.div_euclid(86400)).iso(), r / 3600, r / 60 % 60, r % 60)
}

/// {titulo, jogo, inicio, id} se o canal esta ao vivo; None se offline.
pub fn ao_vivo(login: &str, get: Get) -> Result<Option<Value>, String> {
    Ok(get("/streams", &[("user_login", login.to_string())])?.first().map(|x| json!({"titulo": s(x, "title"), "jogo": s(x, "game_name"), "inicio": s(x, "started_at"), "id": x["id"]})))
}

fn id_usuario(login: &str, get: Get) -> Result<Option<String>, String> {
    Ok(get("/users", &[("login", login.to_string())])?.first().and_then(|u| u["id"].as_str().map(String::from)))
}

pub fn url_clipes(login: &str, janela_h: f64) -> String {
    format!("https://www.twitch.tv/{login}/clips?filter=clips&range={}", if janela_h <= 168.0 { "7d" } else { "30d" })
}

pub fn url_vods(login: &str) -> String {
    format!("https://www.twitch.tv/{login}/videos?filter=archives&sort=time")
}

pub fn clipes_de_helix(data: &[Value]) -> Vec<Value> {
    data.iter().map(|c| json!({"id": c["id"], "url": c["url"], "titulo": s(c, "title"), "views": c["view_count"].as_f64().unwrap_or(0.0), "duracao": c["duration"].as_f64().unwrap_or(0.0)})).collect()
}

fn ou(e: &Value, a: &str, b: &str) -> Value {
    if e[a].as_str().is_some_and(|x| !x.is_empty()) { e[a].clone() } else { e[b].clone() }
}

/// Entradas do yt-dlp dentro da janela (entrada sem data conta como de agora).
pub fn clipes_de_ytdlp(entradas: &[Value], janela_h: f64, ts: f64) -> Vec<Value> {
    entradas.iter().filter(|e| e["timestamp"].as_f64().filter(|t| *t != 0.0).unwrap_or(ts) >= ts - janela_h * 3600.0)
        .map(|e| json!({"id": ou(e, "id", "url"), "url": ou(e, "url", "webpage_url"), "titulo": s(e, "title"), "views": e["view_count"].as_f64().unwrap_or(0.0), "duracao": e["duration"].as_f64().unwrap_or(0.0)}))
        .collect()
}

/// Tira clipe sem link, com poucas views, curto demais (5 s nao vira Short) ou reprise (REPLAY/RERUN nao e momento novo); do mais visto ao menos.
pub fn filtrar_clipes(cs: Vec<Value>, min_views: f64, ignorar: &[String], maximo: usize) -> Vec<Value> {
    let mut cs: Vec<Value> = cs.into_iter().filter(|c| {
        let dur = c["duracao"].as_f64().filter(|d| *d != 0.0).unwrap_or(30.0);
        let titulo = c["titulo"].as_str().unwrap_or("").to_lowercase();
        c["url"].as_str().is_some_and(|u| !u.is_empty()) && c["views"].as_f64().unwrap_or(0.0) >= min_views && dur >= 8.0 && !ignorar.iter().any(|t| titulo.contains(&t.to_lowercase()))
    }).collect();
    cs.sort_by(|a, b| b["views"].as_f64().unwrap_or(0.0).total_cmp(&a["views"].as_f64().unwrap_or(0.0)));
    cs.truncate(maximo);
    cs
}

/// Clipes mais vistos da janela pela API oficial.
pub fn clipes_populares(login: &str, janela_h: f64, maximo: usize, min_views: f64, ignorar: &[String], ts: f64, get: Get) -> Result<Vec<Value>, String> {
    let Some(uid) = id_usuario(login, get)? else { return Ok(vec![]) };
    let cs = get("/clips", &[("broadcaster_id", uid), ("started_at", iso_utc(ts - janela_h * 3600.0)), ("first", (maximo * 3).max(20).to_string())])?;
    Ok(filtrar_clipes(clipes_de_helix(&cs), min_views, ignorar, maximo))
}

/// O VOD (transmissao arquivada) mais recente: {id, url, titulo}.
pub fn ultimo_vod(login: &str, get: Get) -> Result<Option<Value>, String> {
    let Some(uid) = id_usuario(login, get)? else { return Ok(None) };
    Ok(get("/videos", &[("user_id", uid), ("type", "archive".into()), ("first", "1".into())])?.first().map(|v| json!({"id": v["id"], "url": v["url"], "titulo": s(v, "title")})))
}

pub fn vod_de_ytdlp(entradas: &[Value]) -> Option<Value> {
    entradas.first().map(|e| json!({"id": e["id"], "url": ou(e, "url", "webpage_url"), "titulo": s(e, "title")}))
}

/// Maiores lives AGORA no idioma, para ajudar a escolher canais. NAO diz nada sobre a permissao de cortes de cada um:
/// isso o dono confere na regra do streamer antes de por "autorizado": true.
pub fn descobrir(idioma: &str, n: usize, get: Get) -> Result<Vec<Value>, String> {
    let mut out: Vec<Value> = vec![];
    for x in get("/streams", &[("language", idioma.to_string()), ("first", "100".into())])? {
        let login = s(&x, "user_login");
        if !login.is_empty() && !out.iter().any(|o| o["login"] == login.as_str()) {
            out.push(json!({"login": login, "nome": s(&x, "user_name"), "espectadores": x["viewer_count"].as_i64().unwrap_or(0), "jogo": s(&x, "game_name")}));
        }
    }
    out.truncate(n);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn live_vod_e_descobrir_pela_api() {
        let pedidos = RefCell::new(vec![]);
        let get = |c: &str, p: &[(&str, String)]| -> Result<Vec<Value>, String> {
            pedidos.borrow_mut().push(format!("{c} {}", p.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&")));
            Ok(match c {
                "/streams" if p[0].0 == "language" => vec![json!({"user_login": "ana", "user_name": "Ana", "viewer_count": 900, "game_name": "X"}), json!({"user_login": "ana"}), json!({"user_login": "bia"})],
                "/streams" if p[0].1 == "ana" => vec![json!({"title": "T", "game_name": "X", "started_at": "2026-01-01T00:00:00Z", "id": "9"})],
                "/users" if p[0].1 == "ana" => vec![json!({"id": "77"})],
                "/videos" => vec![json!({"id": "v1", "url": "https://www.twitch.tv/videos/v1", "title": "Live"})],
                "/clips" => vec![json!({"id": "c1", "url": "u1", "title": "bom", "view_count": 50, "duration": 20.0}), json!({"id": "c2", "url": "u2", "title": "REPLAY da live", "view_count": 900, "duration": 20.0}),
                                  json!({"id": "c3", "url": "u3", "title": "otimo", "view_count": 300, "duration": 30.0}), json!({"id": "c4", "url": "u4", "title": "curto", "view_count": 999, "duration": 5.0}),
                                  json!({"id": "c5", "url": "u5", "title": "fraco", "view_count": 3, "duration": 30.0})],
                _ => vec![],
            })
        };
        assert_eq!(ao_vivo("ana", &get).unwrap().unwrap()["jogo"], "X");
        assert_eq!(ao_vivo("bia", &get).unwrap(), None);
        assert_eq!(ultimo_vod("ana", &get).unwrap().unwrap()["id"], "v1");
        assert_eq!(ultimo_vod("bia", &get).unwrap(), None);
        let d = descobrir("en", 20, &get).unwrap();
        assert_eq!(d.iter().map(|x| x["login"].as_str().unwrap()).collect::<Vec<_>>(), vec!["ana", "bia"]);
        let cs = clipes_populares("ana", 48.0, 5, 20.0, &["replay".into()], 86400.0 * 3.0, &get).unwrap();
        assert_eq!(cs.iter().map(|c| c["id"].as_str().unwrap()).collect::<Vec<_>>(), vec!["c3", "c1"]);
        assert!(pedidos.borrow().iter().any(|p| p == "/clips broadcaster_id=77&started_at=1970-01-02T00:00:00Z&first=20"));
        assert!(clipes_populares("bia", 48.0, 5, 0.0, &[], 0.0, &get).unwrap().is_empty());
        assert_eq!(ao_vivo("ana", &|_, _| Err("HTTP 401".into())), Err("HTTP 401".into()));
    }

    #[test]
    fn lista_do_ytdlp_e_enderecos() {
        let es = [json!({"id": "a", "url": "ua", "title": "A", "view_count": 10, "timestamp": 1000.0}), json!({"url": "ub", "webpage_url": "wb", "title": "B"}),
                  json!({"id": "velho", "url": "uv", "timestamp": 1.0}), json!({"id": "d", "webpage_url": "wd", "view_count": 99, "duration": 12})];
        let cs = clipes_de_ytdlp(&es, 1.0, 4000.0);
        assert_eq!(cs.iter().map(|c| (c["id"].as_str().unwrap(), c["url"].as_str().unwrap())).collect::<Vec<_>>(), vec![("a", "ua"), ("ub", "ub"), ("d", "wd")]);
        assert_eq!(filtrar_clipes(cs, 0.0, &[], 1)[0]["id"], "d");
        assert_eq!(vod_de_ytdlp(&es).unwrap()["url"], "ua");
        assert_eq!(vod_de_ytdlp(&[]), None);
        assert!(url_clipes("ana", 48.0).ends_with("range=7d") && url_clipes("ana", 400.0).ends_with("range=30d") && url_vods("ana").contains("/ana/videos?filter=archives"));
        assert_eq!(iso_utc(1767225600.0 + 3661.0), "2026-01-01T01:01:01Z");
    }
}
