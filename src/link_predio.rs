//! Predio por predio com o par, pela fila do Link (link/predio.py). IDA: o par ve o NOSSO predio so por lista branca de rotas
//! (paginas e APIs do predio; a tela ao vivo do PC, qualquer POST e o resto ficam fora). VOLTA: o par sobe a tela dele
//! (PUT /predio-par/<arquivo>), guardada em dados/link/predio_par/<par>/ com nome seguro, extensao da lista branca e teto de tamanho;
//! o visor local serve esses arquivos com CSP so 'self'.
//! Ficou de fora: os servidores HTTP (a fila e o visor na porta 8774). Aqui estao as REGRAS: `rota` e `visor` devolvem
//! (status, content-type, corpo) e quem chama escreve na rede. Ler a pagina do disco e buscar a API local sao closures.
//! Diferenca do Python: "o dono esta olhando" recebe a hora por parametro (`marcar_olhar`), continua so em memoria.
#![allow(dead_code)]
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const PORTA_VISOR: u16 = 8774;
pub const EXT: [&str; 21] = [".html", ".htm", ".js", ".mjs", ".css", ".json", ".png", ".jpg", ".jpeg", ".webp", ".svg", ".gif", ".glb", ".gltf", ".bin", ".woff", ".woff2", ".ttf", ".mp3", ".ogg", ".txt"];
pub const MAX_ARQ: u64 = 20 << 20;
pub const MAX_TOTAL: u64 = 120 << 20;
pub const OLHANDO_S: f64 = 90.0;
pub const PAGINAS: [(&str, &str); 5] = [("/", "sauraki3d.html"), ("/sauraki", "sauraki3d.html"), ("/mapa", "mapa.html"), ("/pendencias", "pendencias.html"), ("/magno", "magno.html")];
// so GET e so estas rotas; /api/tela (abas do navegador, aceita clique) nunca entra
pub const APIS: [&str; 7] = ["/api/sauraki", "/api/braincenter", "/api/pendencias", "/api/par/conversa", "/api/par/link", "/api/par", "/api/magno"];
pub const CSP: &str = "default-src 'self' 'unsafe-inline' 'unsafe-eval' data: blob:; connect-src 'self'; style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; font-src 'self' data: https://fonts.gstatic.com; form-action 'none'; base-uri 'none'; frame-ancestors http://127.0.0.1:8765 http://localhost:8765";
pub const ESPERA: &str = "<!doctype html><meta charset=utf-8><title>Predio do par</title><body style='margin:0;display:grid;place-items:center;height:100vh;background:#0b0f17;color:#e6ebf5;font:15px system-ui;text-align:center'><div>O prédio do par ainda não chegou.<br><small style='color:#8b96ad'>Quando o computador dele mandar, ele aparece aqui sozinho.</small></div><script>setTimeout(function(){location.reload()},15000)</script>";
const JSON: &str = "application/json; charset=utf-8";

pub type Resposta = (u16, String, Vec<u8>);
static OLHAR: AtomicU64 = AtomicU64::new(0);

fn pasta(dados: &Path) -> PathBuf {
    dados.join("link").join("predio_par")
}

fn erro(st: u16, msg: &str) -> Resposta {
    (st, JSON.into(), serde_json::json!({ "erro": msg }).to_string().into_bytes())
}

/// GET do par no nosso predio. `pagina(arquivo)` le a tela do disco; `buscar(rota?consulta)` pergunta ao Braniac local.
pub fn nosso(caminho: &str, pagina: &dyn Fn(&str) -> Result<Vec<u8>, String>, buscar: &dyn Fn(&str) -> Result<Vec<u8>, String>) -> Resposta {
    let (antes, consulta) = caminho.find('?').map_or((caminho, ""), |i| caminho.split_at(i));
    let rota = if antes.trim_end_matches('/').is_empty() { "/" } else { antes.trim_end_matches('/') };
    if let Some((_, arq)) = PAGINAS.iter().find(|(r, _)| *r == rota) {
        return match pagina(arq) {
            // na visita, o botao "predio do par" some (la ele ja esta no dele)
            Ok(html) => (200, "text/html; charset=utf-8".into(), [b"<script>window.VISITA=1</script>".as_slice(), &html].concat()),
            Err(_) => erro(502, "Braniac fora do ar"),
        };
    }
    if APIS.contains(&rota) {
        return match buscar(&format!("{rota}{consulta}")) {
            Ok(corpo) => (200, JSON.into(), corpo),
            Err(_) => erro(502, "Braniac fora do ar"),
        };
    }
    erro(404, "fora do predio")
}

fn sem_porcento(p: &str) -> String {
    let b = p.as_bytes();
    let (mut out, mut i) = (vec![], 0);
    while i < b.len() {
        let hex = if b[i] == b'%' { b.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok()) } else { None };
        out.push(hex.unwrap_or(b[i]));
        i += if hex.is_some() { 3 } else { 1 };
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn extensao(nome: &str) -> String {
    match nome.rfind('.') {
        Some(i) if i > 0 && i + 1 < nome.len() => nome[i..].to_lowercase(),
        _ => String::new(),
    }
}

/// Segmentos do caminho -> caminho relativo seguro "a/b.js" (sem "..", sem oculto, extensao da lista branca, ate 4 niveis).
pub fn nome_seguro(partes: &[&str]) -> Result<String, String> {
    if partes.is_empty() || partes.len() > 4 {
        return Err("caminho invalido".into());
    }
    let mut limpos = vec![];
    for p in partes {
        let p: String = sem_porcento(p).chars().map(|c| if c.is_alphanumeric() || c == '_' || c == '.' || c == '-' { c } else { '_' }).take(80).collect();
        if p.is_empty() || p.starts_with('.') {
            return Err("caminho invalido".into());
        }
        limpos.push(p);
    }
    if !EXT.contains(&extensao(&limpos[limpos.len() - 1]).as_str()) {
        return Err("tipo de arquivo nao aceito no predio".into());
    }
    Ok(limpos.join("/"))
}

fn pasta_par(dados: &Path, pid: &str) -> PathBuf {
    pasta(dados).join(pid.chars().map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect::<String>())
}

fn tamanho_total(p: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(p) else { return 0 };
    rd.flatten().map(|e| if e.path().is_dir() { tamanho_total(&e.path()) } else { e.metadata().map(|m| m.len()).unwrap_or(0) }).sum()
}

/// Guarda um arquivo do predio do par (espelho: a versao nova substitui a velha). Devolve o nome relativo.
pub fn receber(dados: &Path, pid: &str, partes: &[&str], corpo_tmp: &Path) -> Result<String, String> {
    let rel = nome_seguro(partes)?;
    let tam = std::fs::metadata(corpo_tmp).map(|m| m.len()).unwrap_or(0);
    if tam == 0 || tam > MAX_ARQ {
        return Err("arquivo vazio ou maior que 20 MB".into());
    }
    let base = pasta_par(dados, pid);
    let destino = base.join(&rel);
    let antigo = std::fs::metadata(&destino).map(|m| m.len()).unwrap_or(0);
    if tamanho_total(&base) - antigo + tam > MAX_TOTAL {
        return Err("predio do par passou de 120 MB".into());
    }
    let e = |e: std::io::Error| e.to_string();
    std::fs::create_dir_all(destino.parent().unwrap_or(&base)).map_err(e)?;
    let tmp = PathBuf::from(format!("{}.parcial", destino.display()));
    std::fs::copy(corpo_tmp, &tmp).map_err(e)?;
    std::fs::rename(&tmp, &destino).map_err(e)?;
    Ok(rel)
}

/// O dono abriu o visor agora (o par so manda dados novos enquanto alguem olha).
pub fn marcar_olhar(agora: f64) {
    OLHAR.store(agora.to_bits(), Ordering::Relaxed);
}

pub fn olhando(agora: f64) -> bool {
    agora - f64::from_bits(OLHAR.load(Ordering::Relaxed)) <= OLHANDO_S
}

/// Rota da fila. `decisao` = o que `Pares::decidir(pid, "tarefa", "predio")` devolveu: so "aceitar" passa (render liberado nao basta).
/// `caminho` = caminho inteiro do pedido (com consulta); `partes` = os segmentos dele.
#[allow(clippy::too_many_arguments)]
pub fn rota(dados: &Path, decisao: &str, metodo: &str, caminho: &str, partes: &[&str], pid: &str, corpo_tmp: Option<&Path>, agora: f64, pagina: &dyn Fn(&str) -> Result<Vec<u8>, String>, buscar: &dyn Fn(&str) -> Result<Vec<u8>, String>) -> Resposta {
    if decisao != "aceitar" {
        return erro(403, "o dono deste Braniac nao liberou o predio");
    }
    match (partes.first().copied(), metodo) {
        (Some("predio"), "GET") => {
            let resto = caminho.strip_prefix("/predio").unwrap_or("");
            nosso(if resto.is_empty() { "/" } else { resto }, pagina, buscar)
        }
        (Some("predio-par"), "GET") if partes[1..] == ["olhando"] => (200, JSON.into(), serde_json::json!({ "olhando": olhando(agora) }).to_string().into_bytes()),
        (Some("predio-par"), "PUT") => match corpo_tmp.ok_or_else(|| "arquivo vazio ou maior que 20 MB".to_string()).and_then(|c| receber(dados, pid, &partes[1..], c)) {
            Ok(nome) => (200, JSON.into(), serde_json::json!({ "nome": nome }).to_string().into_bytes()),
            Err(e) => erro(400, &e),
        },
        _ => erro(404, "rota desconhecida"),
    }
}

// shortcut: um par so; com mais pares, escolher pelo nome na URL
fn pasta_atual(dados: &Path) -> Option<PathBuf> {
    let rd = std::fs::read_dir(pasta(dados)).ok()?;
    rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).max_by_key(|p| p.metadata().and_then(|m| m.modified()).ok())
}

fn tipo(ext: &str) -> &'static str {
    match ext {
        ".html" | ".htm" => "text/html",
        ".js" | ".mjs" => "text/javascript",
        ".css" => "text/css",
        ".txt" => "text/plain",
        ".json" | ".gltf" => "application/json",
        ".png" => "image/png",
        ".jpg" | ".jpeg" => "image/jpeg",
        ".webp" => "image/webp",
        ".svg" => "image/svg+xml",
        ".gif" => "image/gif",
        ".glb" => "model/gltf-binary",
        ".woff" => "font/woff",
        ".woff2" => "font/woff2",
        ".ttf" => "font/ttf",
        ".mp3" => "audio/mpeg",
        ".ogg" => "audio/ogg",
        _ => "application/octet-stream",
    }
}

/// Cabecalhos que o visor manda em TODA resposta (outra origem + CSP so 'self': o JS do par nao alcanca o Braniac).
pub fn cabecalhos_visor() -> [(&'static str, &'static str); 3] {
    [("Content-Security-Policy", CSP), ("X-Content-Type-Options", "nosniff"), ("Cache-Control", "no-store")]
}

/// GET no visor local do predio do par. Marca que o dono esta olhando.
pub fn visor(dados: &Path, caminho: &str, agora: f64) -> Resposta {
    marcar_olhar(agora);
    let nao = || (404, "text/plain; charset=utf-8".to_string(), b"nao encontrado".to_vec());
    let so = caminho.split('?').next().unwrap_or("");
    let mut partes: Vec<&str> = so.split('/').filter(|p| !p.is_empty()).collect();
    if partes.is_empty() {
        partes.push("index.html");
    }
    let Ok(rel) = nome_seguro(&partes) else { return nao() };
    let arq = pasta_atual(dados).map(|b| b.join(&rel)).filter(|a| a.is_file());
    let Some(corpo) = arq.and_then(|a| std::fs::read(a).ok()) else {
        return if rel == "index.html" { (200, "text/html; charset=utf-8".into(), ESPERA.as_bytes().to_vec()) } else { nao() };
    };
    let ct = tipo(&extensao(&rel));
    let charset = if ct.starts_with("text/") || ct.ends_with("json") { "; charset=utf-8" } else { "" };
    (200, format!("{ct}{charset}"), corpo)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(nome: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("braniac-link_predio-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn pagina(arq: &str) -> Result<Vec<u8>, String> {
        Ok(format!("<h1>{arq}</h1>").into_bytes())
    }

    fn local(rota: &str) -> Result<Vec<u8>, String> {
        Ok(serde_json::json!({ "rota": rota, "motivo": "saldo 1.234" }).to_string().into_bytes())
    }

    #[test]
    fn nosso_so_lista_branca() {
        let (st, _, html) = nosso("/sauraki?andar=terreo", &pagina, &local);
        assert!(st == 200 && html.starts_with(b"<script>window.VISITA=1</script><h1>sauraki3d.html"));
        assert_eq!(nosso("/mapa", &pagina, &local).0, 200);
        assert_eq!(nosso("/", &pagina, &local).0, 200);
        for fora in ["/api/tela?d=x", "/api/tela-acao", "/api/status", "/par/painel/", "/../server.py", "/api/banco?q=x", "/api/wpp_direto"] {
            assert_eq!(nosso(fora, &pagina, &local).0, 404, "{fora}");
        }
        let (st, _, corpo) = nosso("/api/sauraki", &pagina, &local);
        assert!(st == 200 && String::from_utf8(corpo).unwrap().contains("1.234")); // o mesmo dado que o dono ve
        let v: serde_json::Value = serde_json::from_slice(&nosso("/api/par/link?n=300", &pagina, &local).2).unwrap();
        assert_eq!(v["rota"], "/api/par/link?n=300");
        assert_eq!(nosso("/api/sauraki", &pagina, &|_| Err("fora".into())).0, 502);
    }

    #[test]
    fn nome_seguro_barra_caminho_e_executavel() {
        assert_eq!(nome_seguro(&["assets", "a b.js"]).unwrap(), "assets/a_b.js");
        let ruins: [&[&str]; 8] = [&["..", "x.js"], &[".env"], &["x.exe"], &["x.py"], &["a", "b", "c", "d", "e.js"], &["%2e%2e", "x.js"], &[], &["semext"]];
        for ruim in ruins {
            assert!(nome_seguro(ruim).is_err(), "{ruim:?}");
        }
        assert_eq!(nome_seguro(&["..%2f..%2fserver.py"]).unwrap_err(), "caminho invalido");
    }

    #[test]
    fn ida_e_volta_pela_rota() {
        let d = tmp("rota");
        assert!(String::from_utf8(visor(&d, "/", 5.0).2).unwrap().contains("ainda não chegou")); // sem predio: tela de espera
        let corpo = d.join("corpo.tmp");
        std::fs::write(&corpo, "<h1>Predio do par</h1>").unwrap();
        let r = |dec: &str, met: &str, cam: &str, c: Option<&Path>, agora: f64| {
            let partes: Vec<&str> = cam.split('?').next().unwrap().split('/').filter(|p| !p.is_empty()).collect();
            rota(&d, dec, met, cam, &partes, "par-1", c, agora, &pagina, &local)
        };
        let nao = r("perguntar", "GET", "/predio/api/sauraki", None, 1000.0);
        assert!(nao.0 == 403 && String::from_utf8(nao.2).unwrap().contains("nao liberou o predio"));
        assert_eq!(r("recusar", "PUT", "/predio-par/index.html", Some(&corpo), 1000.0).0, 403);
        assert_eq!(r("aceitar", "GET", "/predio/api/sauraki", None, 1000.0).0, 200);
        assert!(r("aceitar", "GET", "/predio/sauraki", None, 1000.0).2.starts_with(b"<script>window.VISITA=1"));
        assert_eq!(r("aceitar", "GET", "/predio/api/tela?d=x", None, 1000.0).0, 404);
        assert_eq!(r("aceitar", "POST", "/predio/api/sauraki", None, 1000.0).0, 404); // so GET
        // VOLTA
        let ruim = r("aceitar", "PUT", "/predio-par/rodar.sh", Some(&corpo), 1000.0);
        assert!(ruim.0 == 400 && String::from_utf8(ruim.2).unwrap().contains("tipo de arquivo"));
        assert_eq!(r("aceitar", "PUT", "/predio-par/index.html", None, 1000.0).0, 400);
        assert!(String::from_utf8(r("aceitar", "PUT", "/predio-par/index.html", Some(&corpo), 1000.0).2).unwrap().contains("index.html"));
        assert_eq!(receber(&d, "par-1", &["js", "app.js"], &corpo).unwrap(), "js/app.js");
        assert!(String::from_utf8(r("aceitar", "GET", "/predio-par/olhando", None, 1000.0).2).unwrap().contains("false"));
        let (st, ct, html) = visor(&d, "/", 1000.0);
        assert!(st == 200 && ct.starts_with("text/html") && html == b"<h1>Predio do par</h1>");
        assert!(String::from_utf8(r("aceitar", "GET", "/predio-par/olhando", None, 1050.0).2).unwrap().contains("true")); // o dono abriu
        assert!(!olhando(1091.0));
        assert!(visor(&d, "/js/app.js", 1000.0).1.starts_with("text/javascript"));
        assert_eq!(visor(&d, "/..%2f..%2fserver.py", 1000.0).0, 404);
        assert_eq!(visor(&d, "/nada.js", 1000.0).0, 404);
        assert!(cabecalhos_visor()[0].1.contains("connect-src 'self'"));
        // tetos
        std::fs::write(&corpo, "").unwrap();
        assert!(receber(&d, "par-1", &["v.txt"], &corpo).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
