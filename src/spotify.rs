//! Spotify no PC do dono: as REGRAS (o que tocar a partir da busca, qual dispositivo, o que fazer quando a API recusa, volume, teclas de midia).
//! Ficou de fora, por closure em `Mundo`: a API oficial (ja com o token; este modulo NAO guarda Client ID, token nem verificador, que o Python
//! guardava em data/spotify.json), as teclas de midia, abrir o app, o mixer de volume do Windows e o clique no play pela tela (aqui fica so
//! `achar_botao_play`, que acha o botao verde numa imagem RGB). Login: so a montagem do endereco (PKCE, sem segredo) e a conferencia do retorno.
//! Nao portado: o parse das frases ("toca X", "volume em 40"), que no Python mora no jarvis.py, nao neste modulo.
#![allow(dead_code)]
use crate::texto_pt::sem_acento;
use serde_json::{json, Value};

pub const REDIRECT: &str = "http://127.0.0.1:8765/spotify/callback";
pub const ESCOPOS: &str = "user-modify-playback-state user-read-playback-state user-read-currently-playing";
/// Teclas de midia do Windows.
pub const VK_MIDIA: [(&str, u32); 6] = [("playpause", 0xB3), ("proxima", 0xB0), ("anterior", 0xB1), ("volume_mais", 0xAF), ("volume_menos", 0xAE), ("mudo", 0xAD)];

pub fn norm(x: &str) -> String {
    sem_acento(&x.to_lowercase()).chars().map(|c| if c.is_ascii_lowercase() || c.is_ascii_digit() { c } else { ' ' }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

fn pct(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// Endereco do login (PKCE: `desafio` = SHA-256 do verificador em base64url; `estado` sorteado por quem chama).
pub fn url_login(client_id: &str, desafio: &str, estado: &str) -> Result<String, String> {
    if client_id.trim().is_empty() {
        return Err("falta o Client ID do app do Spotify (developer.spotify.com)".into());
    }
    let q = [("client_id", client_id.trim()), ("response_type", "code"), ("redirect_uri", REDIRECT), ("scope", ESCOPOS), ("code_challenge_method", "S256"), ("code_challenge", desafio), ("state", estado)];
    Ok(format!("https://accounts.spotify.com/authorize?{}", q.iter().map(|(k, v)| format!("{k}={}", pct(v))).collect::<Vec<_>>().join("&")))
}

pub fn retorno_confere(codigo: &str, estado_recebido: &str, estado_guardado: &str) -> Result<(), String> {
    if codigo.is_empty() || estado_guardado.is_empty() || estado_recebido != estado_guardado { Err("retorno do login invalido".into()) } else { Ok(()) }
}

fn itens<'a>(r: &'a Value, k: &str) -> &'a [Value] {
    r[k]["items"].as_array().map(|a| a.as_slice()).unwrap_or(&[])
}

fn nome(v: &Value) -> &str {
    v["name"].as_str().unwrap_or("")
}

fn faixa(t: &Value) -> (String, Value) {
    (format!("{}, de {}", nome(t), nome(&t["artists"][0])), json!({"uris": [t["uri"]]}))
}

/// Dado o resultado da busca, decide o que tocar: (descricao, corpo do play). Playlist so se o pedido disser "playlist";
/// artista inteiro se o nome do artista aparece no pedido e nenhuma faixa bate melhor.
pub fn escolher(consulta: &str, resultados: &Value) -> Option<(String, Value)> {
    let q = norm(consulta);
    let palavras: Vec<&str> = q.split(' ').filter(|p| !p.is_empty()).collect();
    let tracks = itens(resultados, "tracks");
    if palavras.contains(&"playlist") {
        if let Some(p) = itens(resultados, "playlists").iter().find(|p| !p.is_null()) {
            return Some((format!("a playlist {}", nome(p)), json!({"context_uri": p["uri"]})));
        }
    }
    let (mut melhor, mut melhor_s) = (None, 0.0);
    for t in tracks {
        let n = format!("{} {}", norm(nome(t)), t["artists"].as_array().map(|a| a.iter().map(|x| norm(nome(x))).collect::<Vec<_>>().join(" ")).unwrap_or_default());
        let do_nome: Vec<&str> = n.split(' ').collect();
        let mut unicas = palavras.clone();
        unicas.sort();
        unicas.dedup();
        let s = unicas.iter().filter(|p| do_nome.contains(p)).count() as f64 / unicas.len().max(1) as f64;
        if s > melhor_s {
            (melhor, melhor_s) = (Some(t), s);
        }
    }
    for a in itens(resultados, "artists") {
        let na = norm(nome(a));
        if !na.is_empty() && q.contains(&na) && (melhor.is_none() || na == q || melhor_s < 0.75) {
            return Some((format!("músicas de {}", nome(a)), json!({"context_uri": a["uri"]})));
        }
    }
    melhor.or(tracks.first()).map(faixa)
}

/// Id do dispositivo deste PC na lista da API: computador ativo, senao computador, senao qualquer um.
pub fn dispositivo_de(devices: &Value) -> Option<String> {
    let todos = devices["devices"].as_array()?;
    let pc: Vec<&Value> = todos.iter().filter(|d| d["type"] == "Computer").collect();
    let pc = if pc.is_empty() { todos.iter().collect() } else { pc };
    pc.iter().find(|d| d["is_active"] == true).or(pc.first()).and_then(|d| d["id"].as_str().map(String::from))
}

/// Centro (x, y) do botao verde de play do "Resultado principal" numa imagem RGB (3 bytes por ponto) da janela do app, ou None.
/// O botao e o unico grande bloco verde Spotify na parte de cima da area de resultados.
pub fn achar_botao_play(rgb: &[u8], w: usize, h: usize) -> Option<(usize, usize)> {
    if w == 0 || rgb.len() < w * h * 3 {
        return None;
    }
    let (y0, y1, x0, x1) = (h / 10, h * 45 / 100, w * 28 / 100, w * 92 / 100);
    let (mut n, mut sx, mut sy) = (0usize, 0usize, 0usize);
    for y in y0..y1 {
        for x in x0..x1 {
            let i = (y * w + x) * 3;
            let (r, g, b) = (rgb[i] as i32, rgb[i + 1] as i32, rgb[i + 2] as i32);
            if g > 185 && r < 110 && b > 50 && b < 150 && g - r > 90 {
                n += 1;
                sx += x;
                sy += y;
            }
        }
    }
    (n >= 500).then(|| (sx / n, sy / n))
}

pub struct Mundo<'a> {
    /// Ha login na API (quem chama sabe; sem login, tudo vai pelas teclas e pela tela).
    pub logado: bool,
    /// (metodo, caminho, parametros da URL, corpo) -> (status HTTP, JSON). O token fica com quem chama.
    pub api: &'a dyn Fn(&str, &str, &[(&str, String)], &Value) -> Result<(u16, Value), String>,
    /// (nome em VK_MIDIA, vezes)
    pub tecla: &'a dyn Fn(&str, usize),
    pub app_rodando: &'a dyn Fn() -> bool,
    /// Abre o app (uri "spotify:" ou "spotify:search:...").
    pub abrir_app: &'a dyn Fn(&str),
    /// Abre a busca no app e clica no play do resultado principal; true se clicou.
    pub tocar_pela_tela: &'a dyn Fn(&str) -> bool,
    /// Volume (0-100) do Spotify no mixer, ou None se o app ainda nao tem sessao de audio.
    pub volume_atual: &'a dyn Fn() -> Option<i64>,
    /// Poe o volume do Spotify (so o app; a voz do Jarvis fica como esta); None se ainda nao ha sessao de audio.
    pub definir_volume: &'a dyn Fn(i64) -> Option<i64>,
    pub esperar: &'a dyn Fn(f64),
}

fn dispositivo(m: &Mundo) -> Result<String, String> {
    // abre o app se preciso e espera aparecer (ate ~25 s)
    for volta in 0..18 {
        let (status, j) = (m.api)("GET", "/me/player/devices", &[], &Value::Null)?;
        if let Some(id) = Some(&j).filter(|_| status == 200).and_then(dispositivo_de) {
            return Ok(id);
        }
        if volta == 0 {
            (m.abrir_app)("spotify:");
        }
        (m.esperar)(1.5);
    }
    Err("o Spotify nao apareceu como dispositivo; abra o app e tente de novo".into())
}

fn tocar_uri(m: &Mundo, device_id: &str, corpo: &Value) -> Result<(), String> {
    let play = || (m.api)("PUT", "/me/player/play", &[("device_id", device_id.to_string())], corpo).map(|r| r.0);
    let mut status = play()?;
    if status == 404 {
        // sem dispositivo ativo: transfere para este e tenta de novo
        (m.api)("PUT", "/me/player", &[], &json!({"device_ids": [device_id], "play": false}))?;
        (m.esperar)(1.2);
        status = play()?;
    }
    match status {
        200 | 202 | 204 => Ok(()),
        403 => Err("o Spotify so deixa o controle de reproducao para contas Premium".into()),
        outro => Err(format!("o Spotify recusou o pedido ({outro})")),
    }
}

/// Procura e toca. Com login: escolhe a melhor faixa/artista/playlist. Sem login: clica no play do resultado principal na tela do app.
pub fn tocar(consulta: &str, m: &Mundo) -> Result<String, String> {
    let consulta = consulta.trim();
    if consulta.is_empty() {
        return Err("diga o que quer ouvir".into());
    }
    if !m.logado {
        return Ok(if (m.tocar_pela_tela)(consulta) {
            format!("Tocando {consulta} no Spotify.")
        } else {
            format!("Abri a busca de {consulta} no Spotify, mas não consegui apertar o play sozinho (a janela precisa estar visível). Com o login da API do Spotify no Braniac isso fica garantido.")
        });
    }
    let (status, j) = (m.api)("GET", "/search", &[("q", consulta.to_string()), ("type", "track,artist,playlist".into()), ("limit", "8".into()), ("market", "BR".into())], &Value::Null)?;
    if status != 200 {
        return Err(format!("a busca no Spotify falhou ({status})"));
    }
    let Some((desc, corpo)) = escolher(consulta, &j) else { return Ok(format!("Não achei nada no Spotify para {consulta}.")) };
    tocar_uri(m, &dispositivo(m)?, &corpo)?;
    Ok(format!("Tocando {desc}."))
}

/// Sobe/desce o volume do Spotify em `delta` pontos. Sem sessao de audio, usa as teclas de volume do Windows.
pub fn ajustar_volume(delta: i64, m: &Mundo) -> Option<i64> {
    match (m.volume_atual)() {
        Some(atual) => (m.definir_volume)((atual + delta).clamp(0, 100)),
        None => {
            (m.tecla)(if delta > 0 { "volume_mais" } else { "volume_menos" }, (delta.unsigned_abs() as usize / 2).max(1));
            None
        }
    }
}

/// pausar | continuar | proxima | anterior | volume (0-100) | volume_mais | volume_menos | tocando. Com login usa a API; sem login, as teclas de midia.
pub fn controle(acao: &str, valor: Option<i64>, m: &Mundo) -> Result<String, String> {
    let acao = acao.to_lowercase();
    let falas = [("pausar", "Pausei.", "PUT", "/me/player/pause", "playpause"), ("continuar", "Continuando.", "PUT", "/me/player/play", "playpause"), ("proxima", "Próxima.", "POST", "/me/player/next", "proxima"), ("anterior", "Voltando.", "POST", "/me/player/previous", "anterior")];
    match acao.as_str() {
        "volume" => {
            let v = valor.ok_or("diga o volume (0 a 100)")?.clamp(0, 100);
            return Ok((m.definir_volume)(v).map(|r| format!("Volume do Spotify em {r}%.")).unwrap_or_else(|| "O Spotify ainda não está tocando; o volume vale quando a música começar.".into()));
        }
        "volume_mais" | "volume_menos" => return Ok(ajustar_volume(if acao == "volume_mais" { 15 } else { -15 }, m).map(|r| format!("Volume em {r}%.")).unwrap_or_else(|| "Ajustei o volume.".into())),
        "tocando" if m.logado => {
            let (status, j) = (m.api)("GET", "/me/player/currently-playing", &[], &Value::Null)?;
            let i = &j["item"];
            return Ok(if status == 200 && i.is_object() { format!("Está tocando {}, de {}.", nome(i), nome(&i["artists"][0])) } else { "Não há nada tocando agora.".into() });
        }
        _ => {}
    }
    let Some((_, fala, metodo, caminho, tecla)) = falas.iter().find(|f| f.0 == acao) else { return Err("comando de Spotify desconhecido".into()) };
    if m.logado {
        (m.api)(metodo, caminho, &[], &Value::Null)?;
    } else {
        if !(m.app_rodando)() {
            (m.abrir_app)("spotify:");
        }
        (m.tecla)(tecla, 1);
    }
    Ok(fala.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn result() -> Value {
        json!({"tracks": {"items": [{"name": "Pais e Filhos", "uri": "spotify:track:1", "artists": [{"name": "Legião Urbana"}]}, {"name": "Tempo Perdido", "uri": "spotify:track:2", "artists": [{"name": "Legião Urbana"}]}]},
               "artists": {"items": [{"name": "Legião Urbana", "uri": "spotify:artist:9"}]}, "playlists": {"items": [null, {"name": "Rock Nacional", "uri": "spotify:playlist:7"}]}})
    }

    struct Falso {
        feito: RefCell<Vec<String>>,
        play: RefCell<Vec<u16>>,
        devices: Value,
        volume: RefCell<Option<i64>>,
    }

    impl Falso {
        fn novo(play: &[u16], devices: Value, volume: Option<i64>) -> Self {
            Falso { feito: RefCell::new(vec![]), play: RefCell::new(play.to_vec()), devices, volume: RefCell::new(volume) }
        }

        fn usar<T>(&self, logado: bool, clica: bool, f: impl FnOnce(&Mundo) -> T) -> T {
            let nota = |x: String| self.feito.borrow_mut().push(x);
            f(&Mundo {
                logado,
                api: &|metodo, caminho, _, corpo| {
                    nota(format!("{metodo} {caminho} {corpo}"));
                    Ok(match caminho {
                        "/search" => (200, result()),
                        "/me/player/devices" => (200, self.devices.clone()),
                        "/me/player/play" if metodo == "PUT" && !corpo.is_null() => (if self.play.borrow().len() > 1 { self.play.borrow_mut().remove(0) } else { self.play.borrow()[0] }, Value::Null),
                        "/me/player/currently-playing" => (200, json!({"item": {"name": "Tempo Perdido", "artists": [{"name": "Legião Urbana"}]}})),
                        _ => (204, Value::Null),
                    })
                },
                tecla: &|n, v| nota(format!("tecla {n} x{v}")),
                app_rodando: &|| false,
                abrir_app: &|u| nota(format!("abrir {u}")),
                tocar_pela_tela: &|q| {
                    nota(format!("tela {q}"));
                    clica
                },
                volume_atual: &|| *self.volume.borrow(),
                definir_volume: &|v| {
                    let tem = self.volume.borrow().is_some();
                    if tem {
                        *self.volume.borrow_mut() = Some(v);
                    }
                    tem.then_some(v)
                },
                esperar: &|_| {},
            })
        }
    }

    fn pc() -> Value {
        json!({"devices": [{"id": "cel", "type": "Smartphone", "is_active": true}, {"id": "pc1", "type": "Computer"}]})
    }

    #[test]
    fn escolhe_faixa_artista_ou_playlist() {
        let r = result();
        let (desc, corpo) = escolher("Pais e Filhos", &r).unwrap();
        assert!(corpo == json!({"uris": ["spotify:track:1"]}) && desc == "Pais e Filhos, de Legião Urbana");
        assert_eq!(escolher("Legião Urbana", &r).unwrap(), ("músicas de Legião Urbana".to_string(), json!({"context_uri": "spotify:artist:9"})));
        assert_eq!(escolher("playlist rock nacional", &r).unwrap().1, json!({"context_uri": "spotify:playlist:7"}));
        assert_eq!(escolher("tempo perdido legiao urbana", &r).unwrap().1, json!({"uris": ["spotify:track:2"]})); // a faixa bate inteira: nao vira "o artista"
        assert_eq!(escolher("qualquer coisa", &r).unwrap().1, json!({"uris": ["spotify:track:1"]})); // nada bate: a primeira da busca
        assert_eq!(escolher("zzz", &json!({"tracks": {"items": []}, "artists": {"items": []}, "playlists": {"items": []}})), None);
        assert_eq!(norm("  Legião   Urbana!! "), "legiao urbana");
    }

    #[test]
    fn sem_login_vai_pela_tela_e_pelas_teclas() {
        let f = Falso::novo(&[204], pc(), None);
        assert_eq!(f.usar(false, true, |m| tocar("Djavan", m)), Ok("Tocando Djavan no Spotify.".into()));
        let r = f.usar(false, false, |m| tocar("Djavan", m)).unwrap();
        assert!(r.contains("não consegui apertar o play") && r.contains("janela precisa estar visível"));
        assert_eq!(f.usar(false, true, |m| (controle("pausar", None, m), controle("Proxima", None, m))), (Ok("Pausei.".into()), Ok("Próxima.".into())));
        assert_eq!(f.feito.borrow()[2..], ["abrir spotify:", "tecla playpause x1", "abrir spotify:", "tecla proxima x1"]);
        assert!(f.usar(false, true, |m| tocar("  ", m)).is_err() && f.usar(false, true, |m| controle("tocando", None, m)).is_err() && f.usar(false, true, |m| controle("formatar", None, m)).is_err());
        assert!(!f.feito.borrow().iter().any(|x| x.starts_with("GET") || x.starts_with("PUT"))); // sem login nao chama a API
    }

    #[test]
    fn com_login_procura_escolhe_o_pc_e_trata_recusas() {
        let f = Falso::novo(&[204], pc(), None);
        assert_eq!(f.usar(true, false, |m| tocar("Pais e Filhos", m)), Ok("Tocando Pais e Filhos, de Legião Urbana.".into()));
        assert!(f.feito.borrow().contains(&r#"PUT /me/player/play {"uris":["spotify:track:1"]}"#.to_string()));
        assert_eq!(dispositivo_de(&pc()), Some("pc1".into())); // o computador, mesmo com o celular ativo
        assert_eq!(dispositivo_de(&json!({"devices": [{"id": "cel", "type": "Smartphone"}]})), Some("cel".into()));
        // conta gratis; 404 transfere para o PC e tenta de novo; sem dispositivo abre o app e desiste com aviso
        assert!(Falso::novo(&[403], pc(), None).usar(true, false, |m| tocar("Pais e Filhos", m)).unwrap_err().contains("Premium"));
        let t = Falso::novo(&[404, 204], pc(), None);
        assert!(t.usar(true, false, |m| tocar("Pais e Filhos", m)).is_ok());
        assert!(t.feito.borrow().contains(&r#"PUT /me/player {"device_ids":["pc1"],"play":false}"#.to_string()));
        assert!(Falso::novo(&[500], pc(), None).usar(true, false, |m| tocar("Pais e Filhos", m)).unwrap_err().contains("(500)"));
        let s = Falso::novo(&[204], json!({"devices": []}), None);
        assert!(s.usar(true, false, |m| tocar("Pais e Filhos", m)).unwrap_err().contains("nao apareceu como dispositivo"));
        assert_eq!(s.feito.borrow().iter().filter(|x| x.starts_with("abrir")).count(), 1);
        assert_eq!(f.usar(true, false, |m| (controle("tocando", None, m), controle("anterior", None, m))), (Ok("Está tocando Tempo Perdido, de Legião Urbana.".into()), Ok("Voltando.".into())));
        assert_eq!(f.feito.borrow().last().map(String::as_str), Some("POST /me/player/previous null"));
    }

    #[test]
    fn volume_exato_com_limites_e_sem_sessao_usa_as_teclas() {
        let f = Falso::novo(&[204], pc(), Some(95));
        assert_eq!(f.usar(false, false, |m| controle("volume", Some(250), m)), Ok("Volume do Spotify em 100%.".into()));
        assert_eq!(f.usar(false, false, |m| controle("volume_menos", None, m)), Ok("Volume em 85%.".into()));
        assert_eq!(f.usar(false, false, |m| (controle("volume_mais", None, m), controle("volume_mais", None, m))).1, Ok("Volume em 100%.".into()));
        assert!(f.usar(false, false, |m| controle("volume", None, m)).is_err());
        let mudo = Falso::novo(&[204], pc(), None);
        assert!(mudo.usar(false, false, |m| controle("volume", Some(40), m)).unwrap().contains("ainda não está tocando"));
        assert_eq!(mudo.usar(false, false, |m| controle("volume_menos", None, m)), Ok("Ajustei o volume.".into()));
        assert_eq!(*mudo.feito.borrow(), vec!["tecla volume_menos x7"]);
    }

    #[test]
    fn login_pkce_e_botao_verde() {
        let u = url_login("meu_client", "DESAFIO", "ESTADO").unwrap();
        assert!(u.contains("client_id=meu_client") && u.contains("code_challenge_method=S256") && u.contains("127.0.0.1%3A8765%2Fspotify%2Fcallback") && u.contains("state=ESTADO") && !u.contains("secret"));
        assert!(url_login("", "d", "e").is_err() && retorno_confere("c", "e", "e").is_ok() && retorno_confere("c", "x", "e").is_err() && retorno_confere("", "e", "e").is_err());
        let (w, h) = (800, 450);
        let mut img = vec![18u8; w * h * 3];
        assert_eq!(achar_botao_play(&img, w, h), None);
        for y in 78..103 {
            for x in 600..625 {
                img[(y * w + x) * 3..(y * w + x) * 3 + 3].copy_from_slice(&[30, 215, 96]);
            }
        }
        for y in 400..440 {
            for x in 600..700 {
                img[(y * w + x) * 3..(y * w + x) * 3 + 3].copy_from_slice(&[30, 215, 96]); // verde fora da area de resultados nao conta
            }
        }
        assert_eq!(achar_botao_play(&img, w, h), Some((612, 90)));
        assert_eq!(achar_botao_play(&img[..100], w, h), None);
        assert_eq!(VK_MIDIA[0], ("playpause", 0xB3));
    }
}
