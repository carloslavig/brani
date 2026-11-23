//! Servico do portal remoto (portal.py, porta 8780) atras da chave "portal" de dados/troca.json, que nasce DESLIGADA.
//! Chave desligada = a porta 8780 NAO e aberta (o Braniac em Python a usa). Ligada: login do dono (usuario + scrypt, o MESMO
//! dados/portal.json copiado do Python), codigo de 6 digitos para dispositivo novo (vai ao dono pela caixa de saida, nunca ao log),
//! sessao por cookie e repasse ao proprio B.R.A.N.I (porta 8865). Regras em `portal.rs`; aqui so o servidor, o scrypt e o sorteio.
//! Nunca imprime nem registra senha, hash, token ou codigo. Nao mexe em Tailscale nem cria tunel (o Funnel do dono ja aponta para 8780).
//! O portal NAO deixa quem esta de fora mexer nas chaves de troca (POST /api/troca*) nem importar o antigo.
use crate::link_identidade::{b64, hex, hmac_sha256, iguais, sha256};
use crate::portal::{self, Cripto, Destino, Memoria, Pedido, Req};
use serde_json::{json, Value};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const PORTA: u16 = 8780;
const DESTINO: u16 = 8865;
const LOGIN: &str = include_str!("portal_login.html");
const DISP: &str = include_str!("portal_disp.html");

// ---------- sorteio do sistema ----------
#[cfg(windows)]
pub fn aleatorio(n: usize) -> Vec<u8> {
    #[link(name = "bcrypt")]
    extern "system" {
        fn BCryptGenRandom(alg: *mut u8, buf: *mut u8, tam: u32, flags: u32) -> i32;
    }
    let mut b = vec![0u8; n];
    // 2 = BCRYPT_USE_SYSTEM_PREFERRED_RNG
    let r = unsafe { BCryptGenRandom(std::ptr::null_mut(), b.as_mut_ptr(), n as u32, 2) };
    assert!(r == 0, "sem sorteio seguro do sistema");
    b
}
#[cfg(not(windows))]
pub fn aleatorio(n: usize) -> Vec<u8> {
    let mut b = vec![0u8; n];
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b)).expect("sem /dev/urandom");
    b
}

// ---------- scrypt (RFC 7914), so para conferir o hash que o Python gravou ----------
fn pbkdf2_1(senha: &[u8], sal: &[u8], tam: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(tam + 32);
    let mut i = 1u32;
    while out.len() < tam {
        let mut m = sal.to_vec();
        m.extend_from_slice(&i.to_be_bytes());
        out.extend_from_slice(&hmac_sha256(senha, &m));
        i += 1;
    }
    out.truncate(tam);
    out
}

fn salsa8(b: &mut [u32; 16]) {
    let mut x = *b;
    for _ in 0..4 {
        macro_rules! q { ($a:expr,$b:expr,$c:expr,$r:expr) => { x[$a] ^= x[$b].wrapping_add(x[$c]).rotate_left($r); }; }
        q!(4, 0, 12, 7); q!(8, 4, 0, 9); q!(12, 8, 4, 13); q!(0, 12, 8, 18);
        q!(9, 5, 1, 7); q!(13, 9, 5, 9); q!(1, 13, 9, 13); q!(5, 1, 13, 18);
        q!(14, 10, 6, 7); q!(2, 14, 10, 9); q!(6, 2, 14, 13); q!(10, 6, 2, 18);
        q!(3, 15, 11, 7); q!(7, 3, 15, 9); q!(11, 7, 3, 13); q!(15, 11, 7, 18);
        q!(1, 0, 3, 7); q!(2, 1, 0, 9); q!(3, 2, 1, 13); q!(0, 3, 2, 18);
        q!(6, 5, 4, 7); q!(7, 6, 5, 9); q!(4, 7, 6, 13); q!(5, 4, 7, 18);
        q!(11, 10, 9, 7); q!(8, 11, 10, 9); q!(9, 8, 11, 13); q!(10, 9, 8, 18);
        q!(12, 15, 14, 7); q!(13, 12, 15, 9); q!(14, 13, 12, 13); q!(15, 14, 13, 18);
    }
    for i in 0..16 {
        b[i] = b[i].wrapping_add(x[i]);
    }
}

fn block_mix(b: &[u32], r: usize) -> Vec<u32> {
    let mut x = [0u32; 16];
    x.copy_from_slice(&b[(2 * r - 1) * 16..2 * r * 16]);
    let mut y = vec![0u32; 32 * r];
    for i in 0..2 * r {
        for k in 0..16 {
            x[k] ^= b[i * 16 + k];
        }
        salsa8(&mut x);
        let dest = if i % 2 == 0 { i / 2 } else { r + i / 2 };
        y[dest * 16..dest * 16 + 16].copy_from_slice(&x);
    }
    y
}

pub fn scrypt(senha: &[u8], sal: &[u8], n: usize, r: usize, p: usize, tam: usize) -> Vec<u8> {
    let mut b = pbkdf2_1(senha, sal, p * 128 * r);
    for bloco in b.chunks_mut(128 * r) {
        let mut x: Vec<u32> = bloco.chunks(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        let mut v = Vec::with_capacity(n * 32 * r);
        for _ in 0..n {
            v.extend_from_slice(&x);
            x = block_mix(&x, r);
        }
        for _ in 0..n {
            let j = (x[(2 * r - 1) * 16] as usize) & (n - 1);
            for k in 0..32 * r {
                x[k] ^= v[j * 32 * r + k];
            }
            x = block_mix(&x, r);
        }
        for (c, w) in bloco.chunks_mut(4).zip(x) {
            c.copy_from_slice(&w.to_le_bytes());
        }
    }
    pbkdf2_1(senha, &b, tam)
}

fn de_hex(s: &str) -> Option<Vec<u8>> {
    (s.len() % 2 == 0 && s.is_ascii()).then_some(())?;
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

/// Mesmos parametros do portal.py: n=2**14, r=8, p=1, 32 bytes, sal em hex.
fn confere_senha(senha: &str, sal: &str, hash: &str) -> bool {
    let Some(sal) = de_hex(sal) else { return false };
    iguais(hex(&scrypt(senha.as_bytes(), &sal, 1 << 14, 8, 1, 32)).as_bytes(), hash.as_bytes())
}

// ---------- o servidor ----------
pub struct Entrada {
    pub metodo: String,
    pub url: String,
    pub cab: Vec<(String, String)>,
    pub corpo: Vec<u8>,
    pub ip: String,
}

pub enum Saida {
    Pronta(u16, Vec<(String, String)>, Vec<u8>),
    Repassar,
}

fn h<'a>(cab: &'a [(String, String)], nome: &str) -> Option<&'a str> {
    cab.iter().find(|(k, _)| k.eq_ignore_ascii_case(nome)).map(|(_, v)| v.as_str())
}

fn pronta(status: u16, tipo: &str, corpo: Vec<u8>, extra: Vec<(String, String)>) -> Saida {
    let mut c: Vec<(String, String)> = portal::CABECALHOS.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    c.push(("Content-Type".into(), format!("{tipo}; charset=utf-8")));
    c.extend(extra);
    Saida::Pronta(status, c, corpo)
}

fn json_(status: u16, v: &Value, extra: Vec<(String, String)>) -> Saida {
    pronta(status, "application/json", v.to_string().into_bytes(), extra)
}

fn ir(para: &str) -> Saida {
    pronta(302, "text/plain", vec![], vec![("Location".into(), para.into())])
}

fn cripto_de_verdade<T>(f: impl FnOnce(&Cripto) -> T) -> T {
    let confere = |s: &str, a: &str, b: &str| confere_senha(s, a, b);
    let resumo = |t: &str| hex(&sha256(t.as_bytes()));
    let token = |n: usize| b64(&aleatorio(n));
    let codigo = || {
        let b = aleatorio(4);
        format!("{:06}", u32::from_le_bytes([b[0], b[1], b[2], b[3]]) % 1_000_000)
    };
    f(&Cripto { confere_senha: &confere, resumo: &resumo, token: &token, codigo: &codigo })
}

/// Decide o pedido. `Repassar` = segue para o B.R.A.N.I (quem chama faz o repasse).
pub fn tratar(dados: &Path, mem: &Mutex<Memoria>, c: &Cripto, e: &Entrada, ts: f64, quando: &str) -> Saida {
    let r = Req { metodo: &e.metodo, caminho: &e.url, origin: h(&e.cab, "origin"), host: h(&e.cab, "host"), cookie: h(&e.cab, "cookie").unwrap_or(""), accept: h(&e.cab, "accept").unwrap_or("") };
    let caminho = e.url.split('?').next().unwrap_or("");
    let post = e.metodo != "GET";
    match portal::decidir(dados, c, &r, ts) {
        Destino::OrigemNegada => json_(403, &json!({ "erro": "origem negada" }), vec![]),
        Destino::PaginaLogin => pronta(200, "text/html", LOGIN.as_bytes().to_vec(), vec![]),
        Destino::IrParaLogin => ir("/portal/login"),
        Destino::NaoAutenticado => json_(401, &json!({ "erro": "nao autenticado" }), vec![]),
        Destino::IrParaInterface => ir("/b2"),
        Destino::Bloqueado => json_(403, &json!({ "erro": "bloqueado no acesso remoto" }), vec![]),
        Destino::Login => {
            let v: Value = serde_json::from_slice(&e.corpo[..e.corpo.len().min(20000)]).unwrap_or_else(|_| json!({}));
            let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
            let (usuario, senha, codigo) = (s("usuario"), s("senha"), s("codigo"));
            let bdev = portal::cookie(h(&e.cab, "cookie").unwrap_or(""), "bdev");
            let ua = h(&e.cab, "user-agent").unwrap_or("");
            let p = Pedido { usuario: &usuario, senha: &senha, codigo: codigo.trim(), bdev: bdev.as_deref(), ip: &e.ip, ua };
            let avisar = |t: &str| {
                use std::io::Write;
                let arq = dados.join("outbox_carlos.txt");
                let sep = if std::fs::metadata(&arq).map(|m| m.len() > 0).unwrap_or(false) { "\n---\n" } else { "" };
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&arq) {
                    let _ = write!(f, "{sep}{t}");
                }
            };
            let resp = portal::login(dados, &mut mem.lock().unwrap(), c, &p, ts, quando, &avisar);
            if resp.status == 401 {
                std::thread::sleep(Duration::from_secs(1)); // custo por tentativa errada
            }
            json_(resp.status, &resp.corpo, resp.cookies.into_iter().map(|x| ("Set-Cookie".to_string(), x)).collect())
        }
        Destino::Admin(disp) => {
            if caminho == "/portal/dispositivos" {
                return pronta(200, "text/html", DISP.as_bytes().to_vec(), vec![]);
            }
            let corpo: Value = serde_json::from_slice(&e.corpo[..e.corpo.len().min(20000)]).unwrap_or_else(|_| json!({}));
            let (st, v) = portal::admin(dados, &e.url, &disp, post, &corpo, quando);
            json_(st, &v, vec![])
        }
        Destino::Repassar(_) => {
            if (post && caminho.starts_with("/api/troca")) || caminho.starts_with("/api/importar-antigo") || caminho.starts_with("/api/ponte") {
                return json_(403, &json!({ "erro": "as chaves de troca so se mexem no proprio PC" }), vec![]);
            }
            Saida::Repassar
        }
    }
}

type Repasse = (u16, Vec<(String, String)>, Box<dyn Read + Send>);

fn repassar(e: &Entrada) -> Repasse {
    let agente = ureq::AgentBuilder::new().redirects(0).timeout_connect(Duration::from_secs(5)).timeout(Duration::from_secs(300)).build();
    let mut rq = agente.request(&e.metodo, &format!("http://127.0.0.1:{DESTINO}{}", e.url));
    for (k, v) in portal::cabecalhos_de_ida(&e.cab, DESTINO) {
        rq = rq.set(&k, &v);
    }
    let res = if e.corpo.is_empty() { rq.call() } else { rq.send_bytes(&e.corpo) };
    match res {
        Ok(r) | Err(ureq::Error::Status(_, r)) => {
            let cab: Vec<(String, String)> = r.headers_names().iter().filter_map(|n| r.header(n).map(|v| (n.clone(), v.to_string()))).collect();
            (r.status(), portal::cabecalhos_de_volta(&cab), Box::new(r.into_reader()))
        }
        Err(_) => (502, vec![("Content-Type".into(), "application/json".into())], Box::new(std::io::Cursor::new(br#"{"erro":"B.R.A.N.I fora do ar no PC"}"#.to_vec()))),
    }
}

fn servir_um(dados: PathBuf, mem: Arc<Mutex<Memoria>>, mut req: tiny_http::Request) {
    let mut corpo = Vec::new();
    let _ = req.as_reader().take(50_000_000).read_to_end(&mut corpo);
    let cab: Vec<(String, String)> = req.headers().iter().map(|x| (x.field.to_string(), x.value.to_string())).collect();
    let ip = h(&cab, "cf-connecting-ip").map(String::from).or_else(|| h(&cab, "x-forwarded-for").and_then(|x| x.split(',').next()).map(|x| x.trim().to_string()))
        .unwrap_or_else(|| req.remote_addr().map_or("?".into(), |a| a.ip().to_string()));
    let e = Entrada { metodo: req.method().as_str().to_string(), url: req.url().to_string(), cab, corpo, ip };
    let agora = crate::tempo::agora();
    let quando = format!("{} {}", agora.data.iso(), agora.hm());
    let s = cripto_de_verdade(|c| tratar(&dados, &mem, c, &e, crate::fatos::agora_ts(), &quando));
    let hd = |v: &[(String, String)]| v.iter().filter_map(|(k, x)| tiny_http::Header::from_bytes(k.as_bytes(), x.as_bytes()).ok()).collect::<Vec<_>>();
    let _ = match s {
        Saida::Pronta(st, c, b) => req.respond(tiny_http::Response::new(tiny_http::StatusCode(st), hd(&c), std::io::Cursor::new(b), None, None)),
        Saida::Repassar => {
            let (st, c, leitor) = repassar(&e);
            req.respond(tiny_http::Response::new(tiny_http::StatusCode(st), hd(&c), leitor, None, None))
        }
    };
}

/// Um passo do laco: chave ligada e porta fechada -> abre; chave desligada -> fecha. Chave desligada nunca abre porta.
pub fn passo(dados: &Path, porta: u16, srv: &mut Option<Arc<tiny_http::Server>>) {
    if !crate::wpp_laco::chave(dados, "portal") {
        *srv = None;
        return;
    }
    if srv.is_none() {
        // 127.0.0.1: so o Funnel local alcanca; nunca a rede de casa
        *srv = tiny_http::Server::http(("127.0.0.1", porta)).ok().map(Arc::new);
    }
}

pub fn iniciar() {
    let dados = crate::pasta_dados();
    std::thread::spawn(move || {
        let mem = Arc::new(Mutex::new(Memoria::default()));
        let mut srv: Option<Arc<tiny_http::Server>> = None;
        loop {
            passo(&dados, PORTA, &mut srv);
            if let Some(s) = srv.clone() {
                if let Ok(Some(req)) = s.recv_timeout(Duration::from_secs(2)) {
                    let (d, m) = (dados.clone(), mem.clone());
                    std::thread::spawn(move || servir_um(d, m, req));
                }
            } else {
                std::thread::sleep(Duration::from_secs(2));
            }
        }
    });
}

pub fn estado(dados: &Path) -> Value {
    let ligada = crate::wpp_laco::chave(dados, "portal");
    let tem = std::fs::read_to_string(dados.join("portal.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).is_some_and(|v| v["hash"].as_str().is_some_and(|x| !x.is_empty()));
    json!({ "chave": ligada, "porta": PORTA, "porta_aberta": ligada, "login_copiado": tem,
        "obs": if tem { "login do dono ja esta em dados/portal.json" } else { "falta copiar o login: POST /api/troca/copiar" } })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pasta(n: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-portal-{n}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn scrypt_bate_com_o_vetor_da_rfc_7914() {
        let r = hex(&scrypt(b"", b"", 16, 1, 1, 64));
        assert!(r.starts_with("77d6576238657b203b19ca42c18a0497f16b4844e3074ae8dfdffa3fede21442"), "{r}");
    }

    fn entrada(m: &str, url: &str, cab: &[(&str, &str)], corpo: &str) -> Entrada {
        let mut c: Vec<(String, String)> = cab.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        c.push(("Host".into(), "x.ts.net".into()));
        Entrada { metodo: m.into(), url: url.into(), cab: c, corpo: corpo.as_bytes().to_vec(), ip: "1.2.3.4".into() }
    }

    fn senha_de_teste(d: &Path) {
        let sal = "00112233445566778899aabbccddeeff";
        let hash = hex(&scrypt(b"s3nha-de-teste", &de_hex(sal).unwrap(), 1 << 14, 8, 1, 32));
        portal::definir_senha(d, "dono", sal, &hash).unwrap();
    }

    #[test]
    fn login_errado_nao_entra_e_sem_sessao_nada_passa() {
        let d = pasta("login");
        senha_de_teste(&d);
        let mem = Mutex::new(Memoria::default());
        let s = cripto_de_verdade(|c| tratar(&d, &mem, c, &entrada("POST", "/portal/login", &[], r#"{"usuario":"dono","senha":"errada"}"#), 1000.0, "q"));
        assert!(matches!(s, Saida::Pronta(401, _, _)));
        let s = cripto_de_verdade(|c| tratar(&d, &mem, c, &entrada("GET", "/api/status", &[], ""), 1000.0, "q"));
        assert!(matches!(s, Saida::Pronta(401, _, _)));
        let s = cripto_de_verdade(|c| tratar(&d, &mem, c, &entrada("GET", "/api/status", &[("Accept", "text/html")], ""), 1000.0, "q"));
        assert!(matches!(s, Saida::Pronta(302, _, _)));
    }

    #[test]
    fn dispositivo_novo_pede_codigo_e_o_codigo_vai_so_para_a_caixa_do_dono() {
        let d = pasta("codigo");
        senha_de_teste(&d);
        let mem = Mutex::new(Memoria::default());
        let s = cripto_de_verdade(|c| tratar(&d, &mem, c, &entrada("POST", "/portal/login", &[], r#"{"usuario":"dono","senha":"s3nha-de-teste"}"#), 1000.0, "q"));
        let Saida::Pronta(200, cab, corpo) = s else { panic!("esperava 200") };
        assert!(String::from_utf8_lossy(&corpo).contains("precisa_codigo"));
        assert!(!cab.iter().any(|(k, v)| k == "Set-Cookie" && v.starts_with("bsess")), "nao pode haver sessao antes do codigo");
        assert!(std::fs::read_to_string(d.join("outbox_carlos.txt")).unwrap().contains("Código"));
    }

    #[test]
    fn chave_desligada_nao_abre_porta() {
        let d = pasta("chave");
        let mut srv = None;
        passo(&d, 0, &mut srv);
        assert!(srv.is_none(), "chave desligada nao pode abrir porta");
        std::fs::write(d.join("troca.json"), r#"{"portal": true}"#).unwrap();
        passo(&d, 0, &mut srv);
        assert!(srv.is_some());
        std::fs::write(d.join("troca.json"), r#"{"portal": false}"#).unwrap();
        passo(&d, 0, &mut srv);
        assert!(srv.is_none());
    }
}
