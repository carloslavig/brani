//! Portal remoto: a porta de entrada para usar o Braniac de fora, com o PC como servidor. Aqui ficam as REGRAS (o servidor HTTP e o
//! tunel sao de quem liga): senha so conferida por hash; trava por IP (5 falhas em 15 min) e global (20 em 1 h); dispositivo novo exige
//! codigo de 6 digitos enviado ao dono (vale 5 min, 3 tentativas); sessao de 24 h; dispositivo confiavel por 30 dias; cookies
//! HttpOnly+Secure+SameSite=Strict; Origin conferido em todo pedido que nao e GET; revogar dispositivo derruba a sessao; `/api/remoto*`
//! (pareamento) nunca passa pelo portal; depois do login o pedido e repassado sem cookies e com Host/Origin trocados.
//! NUNCA grava senha nem token: dados/portal.json guarda usuario, sal, hash e o RESUMO do token de sessao.
//! Ficou de fora (closures em `Cripto`, porque nao ha crate de scrypt/sha/aleatorio): conferir a senha, resumir o token, gerar token e
//! codigo. Tambem de fora: as paginas HTML, o repasse em si, o tunel, o `sleep(1)` depois de cada falha (quem serve aplica) e o registro
//! do proprio hash da senha (`definir_senha` recebe sal e hash ja calculados).
#![allow(dead_code)]
use crate::empresa_dev::{gravar, ler, rx};
use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::sync::OnceLock;

pub const PORTA: u16 = 8780;
pub const SESSAO_S: f64 = 24.0 * 3600.0;
pub const CONFIAVEL_S: f64 = 30.0 * 24.0 * 3600.0;
pub const CODIGO_S: f64 = 300.0;
/// Vao em toda resposta do portal.
pub const CABECALHOS: [(&str, &str); 5] = [("Cache-Control", "no-store"), ("X-Content-Type-Options", "nosniff"), ("X-Frame-Options", "DENY"), ("Referrer-Policy", "no-referrer"), ("Strict-Transport-Security", "max-age=31536000")];

pub struct Cripto<'a> {
    /// (senha digitada, sal, hash guardado) -> confere? Em tempo constante.
    pub confere_senha: &'a dyn Fn(&str, &str, &str) -> bool,
    /// Resumo de mao unica do token de sessao (o arquivo nunca guarda o token).
    pub resumo: &'a dyn Fn(&str) -> String,
    /// Token aleatorio seguro para URL com n bytes de entropia.
    pub token: &'a dyn Fn(usize) -> String,
    /// Codigo aleatorio de 6 digitos.
    pub codigo: &'a dyn Fn() -> String,
}

/// O que vive so na memoria do processo: falhas por IP, falhas de todos e codigos pendentes (codigo, expira, tentativas).
#[derive(Default)]
pub struct Memoria {
    falhas: HashMap<String, Vec<f64>>,
    falhas_g: Vec<f64>,
    codigos: HashMap<String, (String, f64, u32)>,
}

#[derive(Debug, PartialEq)]
pub struct Resposta {
    pub status: u16,
    pub corpo: Value,
    pub cookies: Vec<String>,
}

fn resp(status: u16, corpo: Value, cookies: Vec<String>) -> Resposta {
    Resposta { status, corpo, cookies }
}

fn arq(dados: &Path) -> std::path::PathBuf {
    dados.join("portal.json")
}

fn estado(dados: &Path) -> Value {
    let mut d = ler(&arq(dados)).filter(Value::is_object).unwrap_or_else(|| json!({}));
    for k in ["dispositivos", "sessoes"] {
        if !d[k].is_object() {
            d[k] = json!({});
        }
    }
    d
}

fn log(dados: &Path, quando: &str, msg: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dados.join("portal.log")) {
        let _ = writeln!(f, "{quando} {msg}");
    }
}

/// Comparacao em tempo constante (nao revela em que letra errou).
fn igual(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |x, (p, q)| x | (p ^ q)) == 0
}

/// Guarda usuario, sal e HASH (calculados fora). A senha nunca chega aqui.
pub fn definir_senha(dados: &Path, usuario: &str, sal: &str, hash: &str) -> Result<(), String> {
    let mut d = estado(dados);
    d["usuario"] = json!(usuario.to_lowercase());
    d["sal"] = json!(sal);
    d["hash"] = json!(hash);
    gravar(&arq(dados), &d)
}

/// 5 falhas do mesmo IP em 15 min ou 20 falhas de qualquer um em 1 h.
pub fn bloqueado(m: &mut Memoria, ip: &str, ts: f64) -> bool {
    m.falhas_g.retain(|t| ts - t < 3600.0);
    let r = m.falhas.entry(ip.to_string()).or_default();
    r.retain(|t| ts - t < 900.0);
    r.len() >= 5 || m.falhas_g.len() >= 20
}

fn falhou(dados: &Path, m: &mut Memoria, ip: &str, motivo: &str, ts: f64, quando: &str) {
    m.falhas.entry(ip.to_string()).or_default().push(ts);
    m.falhas_g.push(ts);
    log(dados, quando, &format!("FALHA {motivo} ip={ip}"));
}

pub fn nome_disp(ua: &str) -> String {
    let so = [("iPhone", "iPhone"), ("iPad", "iPad"), ("Android", "Android"), ("Windows", "Windows"), ("Mac OS", "Mac"), ("Linux", "Linux")].iter().find(|(k, _)| ua.contains(k)).map_or("?", |x| x.1);
    let nav = [("Edg/", "Edge"), ("Firefox", "Firefox"), ("Chrome", "Chrome"), ("Safari", "Safari")].iter().find(|(k, _)| ua.contains(k)).map_or("navegador", |x| x.1);
    format!("{so} · {nav}")
}

/// Id do dispositivo se a sessao existe, nao expirou e o dispositivo nao foi revogado.
pub fn sessao_valida(dados: &Path, c: &Cripto, tok: Option<&str>, ts: f64) -> Option<String> {
    let d = estado(dados);
    let s = &d["sessoes"][(c.resumo)(tok.filter(|t| !t.is_empty())?)];
    let disp = s["disp"].as_str()?;
    (s["exp"].as_f64()? >= ts && d["dispositivos"][disp]["status"] == "ativo").then(|| disp.to_string())
}

/// Valor de um cookie do cabecalho (so letras, numeros, `_` e `-`).
pub fn cookie(cabecalho: &str, nome: &str) -> Option<String> {
    Regex::new(&format!(r"(?:^|;\s*){}=([A-Za-z0-9_-]+)", regex::escape(nome))).ok()?.captures(cabecalho)?.get(1).map(|m| m.as_str().to_string())
}

/// Sem Origin (navegacao direta) passa; com Origin, tem de ser o proprio Host.
pub fn origem_ok(origin: Option<&str>, host: Option<&str>) -> bool {
    origin.is_none_or(|o| Some(o.split_once("://").map_or(o, |x| x.1)) == host)
}

fn cookie_seguro(nome: &str, valor: &str, max_age: f64) -> String {
    format!("{nome}={valor}; Max-Age={}; Path=/; HttpOnly; Secure; SameSite=Strict", max_age as i64)
}

pub struct Pedido<'a> {
    pub usuario: &'a str,
    pub senha: &'a str,
    pub codigo: &'a str,
    /// Cookie `bdev` (id do dispositivo), se o navegador ja tem.
    pub bdev: Option<&'a str>,
    pub ip: &'a str,
    pub ua: &'a str,
}

/// POST /portal/login. `quando` = "dd/mm/aaaa hh:mm" (vai no registro do dispositivo e no log). `avisar` manda o codigo ao DONO.
pub fn login(dados: &Path, m: &mut Memoria, c: &Cripto, p: &Pedido, ts: f64, quando: &str, avisar: &dyn Fn(&str)) -> Resposta {
    if bloqueado(m, p.ip, ts) {
        return resp(429, json!({ "erro": "Muitas tentativas. Aguarde alguns minutos." }), vec![]);
    }
    let d = estado(dados);
    let (Some(hash), Some(sal), Some(usuario)) = (d["hash"].as_str().filter(|h| !h.is_empty()), d["sal"].as_str(), d["usuario"].as_str()) else {
        return resp(503, json!({ "erro": "Portal sem senha definida." }), vec![]);
    };
    // as duas conferencias rodam sempre: o tempo de resposta nao diz se o usuario existe
    let ok_u = igual(&p.usuario.to_lowercase(), usuario);
    let ok_s = (c.confere_senha)(p.senha, sal, hash);
    if !(ok_u && ok_s) {
        falhou(dados, m, p.ip, "senha", ts, quando);
        return resp(401, json!({ "erro": "Usuário ou senha incorretos." }), vec![]);
    }
    let mut cookies = Vec::new();
    let did = match p.bdev.filter(|x| !x.is_empty()) {
        Some(x) => x.to_string(),
        None => {
            let novo = (c.token)(24);
            cookies.push(cookie_seguro("bdev", &novo, 10.0 * 365.0 * 86400.0));
            novo
        }
    };
    let mut disp = d["dispositivos"][&did].clone();
    if disp["status"] != "ativo" || disp["confianca"].as_f64().unwrap_or(0.0) < ts {
        let cod = p.codigo.trim();
        let reg = m.codigos.get(&did).cloned().filter(|r| r.1 >= ts);
        let Some((certo, expira, tentativas)) = reg.filter(|_| !cod.is_empty()) else {
            let novo = (c.codigo)();
            avisar(&format!("Braniac remoto: dispositivo novo ({}, IP {}) tentando entrar. Código: {novo} (vale 5 min). Se não foi você, ignore e use 'Sair de todos' nos dispositivos.", nome_disp(p.ua), p.ip));
            m.codigos.insert(did, (novo, ts + CODIGO_S, 0));
            log(dados, quando, &format!("CODIGO enviado ip={}", p.ip));
            return resp(200, json!({ "precisa_codigo": true }), cookies);
        };
        if tentativas >= 3 || !igual(cod, &certo) {
            m.codigos.insert(did, (certo, expira, tentativas + 1));
            falhou(dados, m, p.ip, "codigo", ts, quando);
            return resp(401, json!({ "erro": "Código incorreto." }), cookies);
        }
        m.codigos.remove(&did);
        disp = json!({ "nome": nome_disp(p.ua), "primeiro": quando, "logins": 0, "status": "ativo", "confianca": ts + CONFIAVEL_S });
    }
    let tok = (c.token)(32);
    let mut d = estado(dados);
    disp["ip"] = json!(p.ip);
    disp["ultimo"] = json!(quando);
    disp["logins"] = json!(disp["logins"].as_i64().unwrap_or(0) + 1);
    let nome = disp["nome"].as_str().unwrap_or("?").to_string();
    d["dispositivos"][&did] = disp;
    if let Some(s) = d["sessoes"].as_object_mut() {
        s.retain(|_, v| v["exp"].as_f64().unwrap_or(0.0) > ts);
        s.insert((c.resumo)(&tok), json!({ "disp": did, "exp": ts + SESSAO_S }));
    }
    if let Err(e) = gravar(&arq(dados), &d) {
        return resp(500, json!({ "erro": e }), cookies);
    }
    log(dados, quando, &format!("LOGIN ok ip={} disp={nome}", p.ip));
    cookies.push(cookie_seguro("bsess", &tok, SESSAO_S));
    resp(200, json!({ "ok": true }), cookies)
}

#[derive(Debug, PartialEq)]
pub enum Destino {
    /// POST de outra origem.
    OrigemNegada,
    PaginaLogin,
    Login,
    /// Navegador sem sessao pedindo pagina: vai para o login.
    IrParaLogin,
    NaoAutenticado,
    /// Rotas /portal/* de quem ja entrou (id do dispositivo).
    Admin(String),
    /// A raiz leva a interface principal.
    IrParaInterface,
    /// /api/remoto*: pareamento nunca pelo portal.
    Bloqueado,
    /// Repassa ao Braniac local.
    Repassar(String),
}

pub struct Req<'a> {
    pub metodo: &'a str,
    pub caminho: &'a str,
    pub origin: Option<&'a str>,
    pub host: Option<&'a str>,
    pub cookie: &'a str,
    pub accept: &'a str,
}

/// O que fazer com um pedido. Sem sessao valida, nada alem do login passa.
pub fn decidir(dados: &Path, c: &Cripto, r: &Req, ts: f64) -> Destino {
    let (p, post) = (r.caminho.split('?').next().unwrap_or(""), r.metodo != "GET");
    if post && !origem_ok(r.origin, r.host) {
        return Destino::OrigemNegada;
    }
    if p == "/portal/login" {
        return if post { Destino::Login } else { Destino::PaginaLogin };
    }
    let Some(disp) = sessao_valida(dados, c, cookie(r.cookie, "bsess").as_deref(), ts) else {
        return if !post && r.accept.contains("text/html") { Destino::IrParaLogin } else { Destino::NaoAutenticado };
    };
    if p.starts_with("/portal/") {
        Destino::Admin(disp)
    } else if p == "/" || p == "/index.html" {
        Destino::IrParaInterface
    } else if p.starts_with("/api/remoto") {
        Destino::Bloqueado
    } else {
        Destino::Repassar(disp)
    }
}

/// Rotas /portal/api/* de quem ja entrou: listar dispositivos, revogar um, revogar todos, sair. Mudanca so por POST.
pub fn admin(dados: &Path, caminho: &str, atual: &str, post: bool, corpo: &Value, quando: &str) -> (u16, Value) {
    let mut d = estado(dados);
    let p = caminho.split('?').next().unwrap_or("");
    if p == "/portal/api/dispositivos" {
        let lista: Vec<Value> = d["dispositivos"].as_object().into_iter().flatten().map(|(k, v)| {
            let mut v = v.clone();
            v["id"] = json!(k);
            v["atual"] = json!(k == atual);
            v
        }).collect();
        return (200, json!(lista));
    }
    if !post {
        return (404, json!({ "erro": "nao encontrado" }));
    }
    match p {
        "/portal/api/revogar" => {
            if let Some(x) = corpo["id"].as_str().filter(|i| d["dispositivos"][*i].is_object()) {
                d["dispositivos"][x]["status"] = json!("revogado");
            }
        }
        "/portal/api/revogar-todos" => {
            for v in d["dispositivos"].as_object_mut().into_iter().flatten() {
                v.1["status"] = json!("revogado");
            }
            d["sessoes"] = json!({});
        }
        "/portal/logout" => {
            if let Some(s) = d["sessoes"].as_object_mut() {
                s.retain(|_, v| v["disp"] != atual);
            }
        }
        _ => return (404, json!({ "erro": "nao encontrado" })),
    }
    if let Err(e) = gravar(&arq(dados), &d) {
        return (500, json!({ "erro": e }));
    }
    log(dados, quando, &format!("ADMIN {p} disp={atual}"));
    (200, json!({ "ok": true }))
}

/// Cabecalhos que seguem para o Braniac local: sem cookie (a sessao do portal nao vaza), com Host e Origin locais.
pub fn cabecalhos_de_ida(cab: &[(String, String)], porta_destino: u16) -> Vec<(String, String)> {
    const FORA: [&str; 7] = ["host", "origin", "cookie", "connection", "content-length", "accept-encoding", "cf-connecting-ip"];
    let mut h: Vec<(String, String)> = cab.iter().filter(|(k, _)| !FORA.contains(&k.to_lowercase().as_str())).cloned().collect();
    h.push(("Host".into(), format!("127.0.0.1:{porta_destino}")));
    if cab.iter().any(|(k, _)| k.eq_ignore_ascii_case("origin")) {
        h.push(("Origin".into(), format!("http://127.0.0.1:{porta_destino}")));
    }
    h
}

/// Cabecalhos da resposta do Braniac que voltam a quem esta fora (nunca Set-Cookie do servidor de dentro).
pub fn cabecalhos_de_volta(cab: &[(String, String)]) -> Vec<(String, String)> {
    const FORA: [&str; 5] = ["connection", "transfer-encoding", "content-length", "strict-transport-security", "set-cookie"];
    let mut h: Vec<(String, String)> = cab.iter().filter(|(k, _)| !FORA.contains(&k.to_lowercase().as_str())).cloned().collect();
    h.push(("X-Frame-Options".into(), "DENY".into()));
    h.push(("Connection".into(), "close".into()));
    h
}

fn url_tunel() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    rx(&R, r"https://[a-z0-9-]+\.trycloudflare\.com")
}

/// Linha da saida do tunel: endereco novo e SO guardado em dados/portal_url.txt; nunca e enviado a ninguem.
pub fn anotar_tunel(dados: &Path, linha: &str) -> Option<String> {
    let url = url_tunel().find(linha)?.as_str().to_string();
    std::fs::write(dados.join("portal_url.txt"), &url).ok()?;
    Some(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::path::PathBuf;

    const SENHA: &str = "senha-de-teste";
    const Q: &str = "18/11/2026 10:00";

    fn pasta(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-portal-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn valor(cookie: &str) -> String {
        cookie.split(';').next().unwrap().split_once('=').unwrap().1.to_string()
    }

    #[test]
    fn fluxo_completo_do_portal() {
        let d = pasta("fluxo");
        let n = Cell::new(0);
        let c = Cripto { confere_senha: &|s, sal, h| h == format!("H({sal}{})", s.chars().rev().collect::<String>()), resumo: &|t| format!("R({t})"), token: &|b| { n.set(n.get() + 1); format!("tok{}-{b}", n.get()) }, codigo: &|| "123456".into() };
        let mut m = Memoria::default();
        let avisos = RefCell::new(Vec::new());
        let avisar = |t: &str| avisos.borrow_mut().push(t.to_string());
        let ua = "Mozilla/5.0 (Linux; Android 14) Chrome/120";
        let ped = |senha: &'static str, codigo: &'static str, bdev: Option<&'static str>| Pedido { usuario: "DONA", senha, codigo, bdev, ip: "203.0.113.9", ua };
        let req = |metodo: &'static str, caminho: &'static str, cookie: &'static str| Req { metodo, caminho, origin: None, host: Some("x.test"), cookie, accept: "application/json" };

        assert_eq!(login(&d, &mut m, &c, &ped(SENHA, "", None), 1000.0, Q, &avisar).status, 503); // sem senha definida
        definir_senha(&d, "Dona", "sal1", &format!("H(sal1{})", SENHA.chars().rev().collect::<String>())).unwrap(); // "hash" de mentira: a senha ao contrario
        assert!(!std::fs::read_to_string(d.join("portal.json")).unwrap().contains(SENHA));
        assert_eq!(decidir(&d, &c, &req("GET", "/api/saude", ""), 1000.0), Destino::NaoAutenticado); // sem login nada passa
        assert_eq!(decidir(&d, &c, &Req { accept: "text/html", ..req("GET", "/b2", "") }, 1000.0), Destino::IrParaLogin);
        assert_eq!(decidir(&d, &c, &req("GET", "/portal/login", ""), 1000.0), Destino::PaginaLogin);
        assert_eq!(decidir(&d, &c, &req("POST", "/portal/login", ""), 1000.0), Destino::Login);
        assert_eq!(decidir(&d, &c, &Req { origin: Some("https://mal.test"), ..req("POST", "/portal/login", "") }, 1000.0), Destino::OrigemNegada);
        assert_eq!(decidir(&d, &c, &Req { origin: Some("https://x.test"), ..req("POST", "/portal/login", "") }, 1000.0), Destino::Login);

        assert_eq!(login(&d, &mut m, &c, &ped("errada", "", None), 1000.0, Q, &avisar).status, 401);
        assert_eq!(login(&d, &mut m, &c, &Pedido { usuario: "outra", ..ped(SENHA, "", None) }, 1000.0, Q, &avisar).status, 401);
        let r = login(&d, &mut m, &c, &ped(SENHA, "", None), 1000.0, Q, &avisar);
        assert!(r.status == 200 && r.corpo == json!({ "precisa_codigo": true }) && r.cookies.len() == 1); // dispositivo novo pede codigo
        let bdev: &'static str = Box::leak(valor(&r.cookies[0]).into_boxed_str());
        assert!(avisos.borrow()[0].contains("Código: 123456") && avisos.borrow()[0].contains("Android · Chrome") && avisos.borrow()[0].contains("203.0.113.9"));
        assert_eq!(login(&d, &mut m, &c, &ped(SENHA, "000000", Some(bdev)), 1001.0, Q, &avisar).status, 401);
        let r = login(&d, &mut m, &c, &ped(SENHA, " 123456 ", Some(bdev)), 1002.0, Q, &avisar);
        assert!(r.corpo == json!({ "ok": true }) && r.cookies.len() == 1 && ["bsess=", "HttpOnly", "Secure", "SameSite=Strict", "Max-Age=86400"].iter().all(|f| r.cookies[0].contains(f)));
        let tok = valor(&r.cookies[0]);
        let salvo = std::fs::read_to_string(d.join("portal.json")).unwrap();
        assert!(!salvo.contains(&format!("\"{tok}\"")) && salvo.contains(&format!("R({tok})"))); // so o resumo do token e guardado
        let ck: &'static str = Box::leak(format!("bdev={bdev}; bsess={tok}").into_boxed_str());

        assert_eq!(decidir(&d, &c, &req("GET", "/api/status?x=1", ck), 1003.0), Destino::Repassar(bdev.to_string()));
        assert_eq!(decidir(&d, &c, &req("GET", "/", ck), 1003.0), Destino::IrParaInterface);
        assert_eq!(decidir(&d, &c, &req("GET", "/api/remoto/parear", ck), 1003.0), Destino::Bloqueado); // pareamento nunca pelo portal
        assert_eq!(decidir(&d, &c, &req("GET", "/api/status", ck), 1003.0 + SESSAO_S), Destino::NaoAutenticado); // sessao vence em 24 h
        assert_eq!(decidir(&d, &c, &req("GET", "/api/status", "bsess=inventado"), 1003.0), Destino::NaoAutenticado);
        assert_eq!(decidir(&d, &c, &req("GET", "/portal/api/dispositivos", ck), 1003.0), Destino::Admin(bdev.to_string()));
        let (st, lista) = admin(&d, "/portal/api/dispositivos", bdev, false, &json!({}), Q);
        assert!(st == 200 && lista.as_array().unwrap().len() == 1 && lista[0]["atual"] == true && lista[0]["logins"] == 1 && lista[0]["nome"] == "Android · Chrome");
        // dispositivo confiavel entra de novo sem codigo; depois de 30 dias pede codigo outra vez
        assert_eq!(login(&d, &mut m, &c, &ped(SENHA, "", Some(bdev)), 2000.0, Q, &avisar).corpo, json!({ "ok": true }));
        assert_eq!(login(&d, &mut m, &c, &ped(SENHA, "", Some(bdev)), 1002.0 + CONFIAVEL_S + 1.0, Q, &avisar).corpo, json!({ "precisa_codigo": true }));

        assert_eq!(admin(&d, "/portal/api/revogar", bdev, false, &json!({ "id": bdev }), Q).0, 404); // mudar so por POST
        assert_eq!(admin(&d, "/portal/api/apagar-tudo", bdev, true, &json!({}), Q).0, 404);
        assert_eq!(admin(&d, "/portal/api/revogar", bdev, true, &json!({ "id": bdev }), Q), (200, json!({ "ok": true })));
        assert_eq!(decidir(&d, &c, &req("GET", "/portal/api/dispositivos", ck), 1004.0), Destino::NaoAutenticado); // revogado derruba a sessao
        let log = std::fs::read_to_string(d.join("portal.log")).unwrap();
        assert!(log.contains("LOGIN ok") && log.contains("FALHA senha") && log.contains("FALHA codigo") && log.contains("ADMIN /portal/api/revogar") && !log.contains(SENHA) && !log.contains("123456"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn travas_de_tentativas_codigo_e_saida() {
        let d = pasta("travas");
        let n = Cell::new(0);
        let c = Cripto { confere_senha: &|s, _, _| s == SENHA, resumo: &|t| format!("R({t})"), token: &|_| { n.set(n.get() + 1); format!("t{}", n.get()) }, codigo: &|| "654321".into() };
        let mut m = Memoria::default();
        definir_senha(&d, "dona", "s", "h").unwrap();
        let ped = |ip: &'static str, senha: &'static str, codigo: &'static str, bdev: Option<&'static str>| Pedido { usuario: "dona", senha, codigo, bdev, ip, ua: "" };
        let nada = |_: &str| {};
        for _ in 0..5 {
            assert_eq!(login(&d, &mut m, &c, &ped("198.51.100.1", "x", "", None), 100.0, Q, &nada).status, 401);
        }
        assert_eq!(login(&d, &mut m, &c, &ped("198.51.100.1", SENHA, "", None), 101.0, Q, &nada).status, 429); // trava por IP, mesmo com a senha certa
        assert_eq!(login(&d, &mut m, &c, &ped("198.51.100.2", SENHA, "", None), 101.0, Q, &nada).status, 200); // outro IP segue
        assert_eq!(login(&d, &mut m, &c, &ped("198.51.100.1", SENHA, "", None), 100.0 + 901.0, Q, &nada).status, 200); // 15 min depois libera
        // codigo: 3 erros queimam o codigo, mesmo acertando depois; vencido (5 min) gera outro
        let mut m = Memoria::default();
        assert_eq!(login(&d, &mut m, &c, &ped("192.0.2.1", SENHA, "", Some("disp1")), 5000.0, Q, &nada).corpo, json!({ "precisa_codigo": true }));
        for _ in 0..3 {
            assert_eq!(login(&d, &mut m, &c, &ped("192.0.2.1", SENHA, "111111", Some("disp1")), 5001.0, Q, &nada).status, 401);
        }
        assert_eq!(login(&d, &mut m, &c, &ped("192.0.2.1", SENHA, "654321", Some("disp1")), 5002.0, Q, &nada).status, 401);
        let mut m = Memoria::default();
        login(&d, &mut m, &c, &ped("192.0.2.2", SENHA, "", Some("disp2")), 6000.0, Q, &nada);
        assert_eq!(login(&d, &mut m, &c, &ped("192.0.2.2", SENHA, "654321", Some("disp2")), 6000.0 + CODIGO_S + 1.0, Q, &nada).corpo, json!({ "precisa_codigo": true }));
        let r = login(&d, &mut m, &c, &ped("192.0.2.2", SENHA, "654321", Some("disp2")), 6400.0, Q, &nada);
        assert_eq!(r.corpo, json!({ "ok": true }));
        let tok = valor(&r.cookies[0]);
        // trava global: 20 falhas de IPs diferentes em 1 h fecham a porta para todos
        let mut m = Memoria::default();
        for i in 0..20 {
            login(&d, &mut m, &c, &Pedido { ip: &format!("10.0.0.{i}"), ..ped("", "x", "", None) }, 7000.0, Q, &nada);
        }
        assert_eq!(login(&d, &mut m, &c, &ped("192.0.2.9", SENHA, "", Some("disp2")), 7001.0, Q, &nada).status, 429);
        // sair derruba so a sessao deste dispositivo; "sair de todos" revoga todos
        assert!(sessao_valida(&d, &c, Some(&tok), 6500.0).is_some());
        assert_eq!(admin(&d, "/portal/logout", "disp2", true, &json!({}), Q).0, 200);
        assert!(sessao_valida(&d, &c, Some(&tok), 6500.0).is_none() && sessao_valida(&d, &c, None, 6500.0).is_none() && sessao_valida(&d, &c, Some(""), 6500.0).is_none());
        admin(&d, "/portal/api/revogar-todos", "disp2", true, &json!({}), Q);
        assert!(admin(&d, "/portal/api/dispositivos", "disp2", false, &json!({}), Q).1.as_array().unwrap().iter().all(|x| x["status"] == "revogado"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn cookie_origem_cabecalhos_e_tunel() {
        assert_eq!(cookie("a=1; bsess=Ab_c-9; x=y", "bsess").as_deref(), Some("Ab_c-9"));
        assert!(cookie("xbsess=1", "bsess").is_none() && cookie("", "bsess").is_none() && cookie("bsess=", "bsess").is_none());
        assert!(origem_ok(None, Some("h")) && origem_ok(Some("https://h"), Some("h")) && !origem_ok(Some("https://h.mal"), Some("h")) && !origem_ok(Some("https://h"), None));
        assert!(igual("abc", "abc") && !igual("abc", "abd") && !igual("abc", "abcd") && !igual("", "a"));
        let par = |k: &str, v: &str| (k.to_string(), v.to_string());
        let ida = cabecalhos_de_ida(&[par("Cookie", "bsess=segredo"), par("Origin", "https://fora"), par("HOST", "fora"), par("Accept", "*/*"), par("Cf-Connecting-Ip", "1.2.3.4")], 8765);
        assert_eq!(ida, [par("Accept", "*/*"), par("Host", "127.0.0.1:8765"), par("Origin", "http://127.0.0.1:8765")]);
        let volta = cabecalhos_de_volta(&[par("Set-Cookie", "x=1"), par("Content-Type", "text/html"), par("Transfer-Encoding", "chunked")]);
        assert_eq!(volta, [par("Content-Type", "text/html"), par("X-Frame-Options", "DENY"), par("Connection", "close")]);
        assert!(nome_disp("Mozilla (iPhone) Safari") == "iPhone · Safari" && nome_disp("curl") == "? · navegador" && nome_disp("Windows NT Edg/120 Chrome") == "Windows · Edge");
        let d = pasta("tunel");
        assert!(anotar_tunel(&d, "nada aqui").is_none());
        assert_eq!(anotar_tunel(&d, "| https://um-dois.trycloudflare.com |").as_deref(), Some("https://um-dois.trycloudflare.com"));
        assert_eq!(std::fs::read_to_string(d.join("portal_url.txt")).unwrap(), "https://um-dois.trycloudflare.com");
        let _ = std::fs::remove_dir_all(&d);
    }
}
