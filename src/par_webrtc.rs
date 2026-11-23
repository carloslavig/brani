//! PAR (par_webrtc.py): dois Braniacs conversam a distancia por um codigo de pareamento. So quem tem o codigo fala com este Braniac,
//! e o DONO decide: ninguem recebe resposta ate `permitir(true)`; o painel local so e mostrado ao par com `permitir_painel(true)`,
//! so por GET e so em caminho simples. Mensagem recebida e SO TEXTO de outra IA: o `responder` de quem hospeda responde sem
//! ferramentas e sem dados pessoais; aqui nada e executado. Limites: 20 mensagens/min, 2000 caracteres, 240 GETs de painel/min.
//! Ficou de fora (closure de quem chama): WebRTC/aiortc, a sinalizacao (ntfy), a cifra AES-GCM/HKDF do "ola", o sorteio do codigo
//! (quem chama passa 16 bytes aleatorios) e o GET no painel local.
#![allow(dead_code)]
use crate::base64;
use crate::link_identidade::{hex, sha256};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub const ALFA: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
pub const MAX_TEXTO: usize = 2000;
pub const LIMITE_MIN: usize = 20;
pub const JANELA_S: f64 = 120.0;
pub const PARTE: usize = 30000; // pedaco (base64) por mensagem do canal
pub const MAX_PAINEL: usize = 3_000_000;
pub const LIMITE_PAINEL: usize = 240;
const SEM_PAR: &str = "Ainda nao ha par: gere ou cole um codigo antes.";

fn corta(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Codigo sem tracos nem espacos, em maiusculas.
pub fn norm(c: &str) -> String {
    c.replace(['-', ' '], "").to_uppercase()
}

/// 16 letras/numeros a partir de 16 bytes aleatorios (32 simbolos: cada byte escolhe um, sem vies).
pub fn codigo_de(bruto: &[u8; 16]) -> String {
    bruto.iter().map(|b| ALFA[(*b % 32) as usize] as char).collect()
}

pub fn formatar(c: &str) -> String {
    c.as_bytes().chunks(4).map(|p| String::from_utf8_lossy(p).to_string()).collect::<Vec<_>>().join("-")
}

/// Cada lado ouve um topico proprio (codigo + nome de quem recebe): nao da para adivinhar sem o codigo.
pub fn topico(cod: &str, quem_recebe: &str) -> String {
    format!("braniac-par-{}", &hex(&sha256(format!("{}|para|{}", norm(cod), quem_recebe.trim().to_lowercase()).as_bytes()))[..32])
}

/// A oferta decifrada serve? So "offer", sessao ainda nao vista e com data dentro da janela (2 min).
pub fn oferta_serve(o: &Value, vistos: &mut HashSet<String>, agora: f64) -> bool {
    let Some(sid) = o["sid"].as_str() else { return false };
    if o["t"] != "offer" || vistos.contains(sid) || (agora - o["ts"].as_f64().unwrap_or(0.0)).abs() > JANELA_S {
        return false;
    }
    vistos.insert(sid.to_string())
}

/// Base64 em pedacos de `parte` caracteres (pelo menos um, mesmo vazio).
pub fn pedacos(corpo: &[u8], parte: usize) -> Vec<String> {
    let b = base64::codificar(corpo);
    if b.is_empty() {
        return vec![String::new()];
    }
    b.as_bytes().chunks(parte.max(1)).map(|p| String::from_utf8_lossy(p).to_string()).collect()
}

pub struct Par {
    arq: PathBuf,
    /// Como este Braniac se chama.
    pub nome: String,
    /// URL do painel LOCAL que o par autorizado pode ver (vazio = nao oferece painel).
    pub painel_base: String,
    tempos: RefCell<Vec<f64>>,
    tp: RefCell<Vec<f64>>,
}

impl Par {
    pub fn new(pasta: &Path, nome: &str, painel_base: &str) -> Par {
        Par { arq: pasta.join("par.json"), nome: nome.to_string(), painel_base: painel_base.trim_end_matches('/').to_string(), tempos: RefCell::default(), tp: RefCell::default() }
    }

    fn ler(&self) -> Value {
        std::fs::read_to_string(&self.arq).ok().and_then(|t| serde_json::from_str(&t).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}))
    }

    fn gravar(&self, d: &Value) -> Result<(), String> {
        if let Some(p) = self.arq.parent() {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        std::fs::write(&self.arq, d.to_string()).map_err(|e| e.to_string())
    }

    fn com(&self, campos: &[(&str, Value)], exige_par: bool) -> Result<(), String> {
        let mut d = self.ler();
        if exige_par && d["codigo"].as_str().is_none_or(str::is_empty) {
            return Err(SEM_PAR.into());
        }
        for (k, v) in campos {
            d[*k] = v.clone();
        }
        self.gravar(&d)
    }

    /// Gera o codigo para entregar ao outro Braniac. O outro lado ainda comeca SEM permissao.
    pub fn novo_codigo(&self, nome_do_par: &str, bruto: &[u8; 16]) -> Result<String, String> {
        let cod = codigo_de(bruto);
        self.com(&[("codigo", json!(cod)), ("par", json!(nome_do_par.trim())), ("recebe", json!(false))], false)?;
        Ok(formatar(&cod))
    }

    /// Cola o codigo recebido do outro lado. Tambem comeca sem permissao.
    pub fn parear(&self, codigo: &str, nome_do_par: &str) -> Result<(), String> {
        let c = norm(codigo);
        if c.chars().count() != 16 {
            return Err("Codigo invalido (sao 16 letras/numeros).".into());
        }
        self.com(&[("codigo", json!(c)), ("par", json!(nome_do_par.trim())), ("recebe", json!(false))], false)
    }

    pub fn permitir(&self, sim: bool) -> Result<(), String> {
        self.com(&[("recebe", json!(sim))], true)
    }

    /// O dono libera (ou nao) o par para VER o painel deste Braniac (so leitura).
    pub fn permitir_painel(&self, sim: bool) -> Result<(), String> {
        self.com(&[("painel", json!(sim))], true)
    }

    pub fn definir_painel_porta(&self, porta: u16) -> Result<(), String> {
        self.com(&[("painel_porta", json!(porta))], false)
    }

    /// {par, pareado, recebe, painel} + o estado da conexao que quem chama informa ({ouvindo, conectado, erro, ultimo}).
    pub fn info(&self, estado: &Value) -> Value {
        let d = self.ler();
        let mut i = if estado.is_object() { estado.clone() } else { json!({}) };
        for (k, v) in [("par", json!(d["par"].as_str().unwrap_or(""))), ("pareado", json!(d["codigo"].as_str().is_some_and(|c| !c.is_empty()))), ("recebe", json!(d["recebe"] == true)), ("painel", json!(d["painel"] == true))] {
            i[k] = v;
        }
        i
    }

    /// (codigo, nome do par) para quem abre a conexao; Err sem par.
    pub fn codigo(&self) -> Result<(String, String), String> {
        let d = self.ler();
        match d["codigo"].as_str().filter(|c| !c.is_empty()) {
            Some(c) => Ok((c.to_string(), d["par"].as_str().unwrap_or("").to_string())),
            None => Err("Sem par: gere ou cole um codigo.".into()),
        }
    }

    fn limite_ok(&self, agora: f64) -> bool {
        let mut t = self.tempos.borrow_mut();
        t.retain(|x| agora - *x < 60.0);
        if t.len() >= LIMITE_MIN {
            return false;
        }
        t.push(agora);
        true
    }

    /// URL do GET no painel local para o caminho pedido; None = 404 (sem painel, caminho estranho ou comprido demais).
    pub fn painel_url(&self, path: &str) -> Option<String> {
        let base = match self.ler()["painel_porta"].as_u64() {
            Some(p) => format!("http://127.0.0.1:{p}"), // o dono escolhe qual tela o par ve
            None => self.painel_base.clone(),
        };
        (!base.is_empty() && path.starts_with('/') && !path.contains("..") && !path.contains("//") && !path.contains('\\') && path.chars().count() <= 300).then(|| format!("{base}{path}"))
    }

    fn tratar_painel(&self, req: &Value, agora: f64, get: &dyn Fn(&str) -> (u16, String, Vec<u8>)) -> Vec<Value> {
        let i = &req["i"];
        let um = |st: u16, corpo: &[u8]| vec![json!({ "i": i, "k": 0, "n": 1, "st": st, "ct": "text/plain", "b64": base64::codificar(corpo) })];
        let mut tp = self.tp.borrow_mut();
        tp.retain(|x| agora - *x < 60.0);
        if self.ler()["painel"] != true || self.painel_base.is_empty() {
            return um(403, b"O dono deste Braniac nao liberou o painel.");
        }
        if tp.len() >= LIMITE_PAINEL {
            return um(429, b"");
        }
        tp.push(agora);
        let (st, ct, corpo) = match self.painel_url(req["path"].as_str().unwrap_or("/")) {
            Some(url) => get(&url),
            None => (404, "text/plain".into(), b"nao encontrado".to_vec()),
        };
        let partes = pedacos(&corpo[..corpo.len().min(MAX_PAINEL)], PARTE);
        let n = partes.len();
        partes.into_iter().enumerate().map(|(k, p)| json!({ "i": i, "k": k, "n": n, "st": st, "ct": ct, "b64": p })).collect()
    }

    /// O que responder a uma mensagem do canal. "ping" tem resposta fixa (nao gasta IA); "painel" so com a liberacao do dono;
    /// texto so com `permitir(true)` e dentro do limite. `responder(nome do par, texto)` e a IA neutra de quem hospeda.
    pub fn tratar(&self, req: &Value, agora: f64, responder: &dyn Fn(&str, &str) -> Result<String, String>, get: &dyn Fn(&str) -> (u16, String, Vec<u8>)) -> Vec<Value> {
        let (d, i) = (self.ler(), &req["i"]);
        if req["t"] == "ping" {
            return vec![json!({ "i": i, "texto": "pong", "v": 2, "nome": self.nome })];
        }
        if req["t"] == "painel" {
            return self.tratar_painel(req, agora, get);
        }
        let texto = corta(req["texto"].as_str().unwrap_or(""), MAX_TEXTO);
        let resp = if d["recebe"] != true {
            "O dono deste Braniac ainda nao permitiu conversas.".to_string()
        } else if !self.limite_ok(agora) {
            "Muitas mensagens; espere um minuto.".to_string()
        } else {
            responder(d["par"].as_str().unwrap_or("outro Braniac"), &texto).map(|r| corta(&r, MAX_TEXTO)).unwrap_or_else(|_| "Nao consegui responder agora.".into())
        };
        vec![json!({ "i": i, "texto": resp })]
    }
}

/// True se a resposta ao "ping" veio certa.
pub fn pong(respostas: &[Value]) -> bool {
    respostas.first().is_some_and(|r| r["texto"] == "pong")
}

/// (status, content-type, bytes) de um GET no painel do outro Braniac, juntando os pedacos.
pub fn painel_juntar(respostas: &[Value]) -> (u16, String, Vec<u8>) {
    let Some(p) = respostas.first().filter(|r| r["b64"].is_string()) else { return (502, "text/plain".into(), b"o outro Braniac e de uma versao sem painel".to_vec()) };
    let mut ord: Vec<&Value> = respostas.iter().collect();
    ord.sort_by_key(|r| r["k"].as_u64().unwrap_or(0));
    match base64::decodificar(&ord.iter().map(|r| r["b64"].as_str().unwrap_or("")).collect::<String>()) {
        Ok(b) => (p["st"].as_u64().and_then(|s| u16::try_from(s).ok()).unwrap_or(502), p["ct"].as_str().unwrap_or("application/octet-stream").to_string(), b),
        Err(_) => (502, "text/plain".into(), b"resposta do painel ilegivel".to_vec()),
    }
}

/// Conversa sozinha, sem humano: manda `abertura`, le a resposta e usa `gerar(historico)` para a proxima fala (vazia = acabou).
/// Para no limite (no maximo 6 turnos). Historico: [("eu" | "par", fala)].
pub fn conversar(abertura: &str, falar: &dyn Fn(&str) -> Result<String, String>, gerar: &dyn Fn(&[(String, String)]) -> String, turnos: usize) -> Result<Vec<(String, String)>, String> {
    let (mut hist, mut fala) = (Vec::new(), abertura.to_string());
    for _ in 0..turnos.min(6) {
        let resp = falar(&corta(&fala, MAX_TEXTO))?;
        hist.push(("eu".to_string(), fala));
        hist.push(("par".to_string(), corta(&resp, MAX_TEXTO)));
        fala = gerar(&hist);
        if fala.is_empty() {
            break;
        }
    }
    Ok(hist)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn par(nome: &str) -> (PathBuf, Par) {
        let d = std::env::temp_dir().join(format!("braniac-par_webrtc-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let p = Par::new(&d, "jarvis", "http://127.0.0.1:8770/");
        (d, p)
    }

    fn sem_get(_: &str) -> (u16, String, Vec<u8>) {
        panic!("nao devia buscar o painel")
    }

    #[test]
    fn codigo_topico_e_oferta() {
        assert_eq!(codigo_de(&[0, 1, 25, 26, 31, 32, 255, 64, 0, 0, 0, 0, 0, 0, 0, 0]), "ABZ27A7AAAAAAAAA");
        assert_eq!(formatar("ABCD2345ABCD2345"), "ABCD-2345-ABCD-2345");
        assert_eq!(norm("abcd-2345 abcd-2345"), "ABCD2345ABCD2345");
        let t = topico("abcd-2345-abcd-2345", " Loja ");
        assert!(t.starts_with("braniac-par-") && t.len() == 44 && t == topico("ABCD2345ABCD2345", "loja") && t != topico("ABCD2345ABCD2345", "jarvis"));
        let mut v = HashSet::new();
        assert!(oferta_serve(&json!({"t": "offer", "sid": "s1", "ts": 1000.0}), &mut v, 1050.0));
        assert!(!oferta_serve(&json!({"t": "offer", "sid": "s1", "ts": 1000.0}), &mut v, 1050.0)); // repetida
        assert!(!oferta_serve(&json!({"t": "offer", "sid": "s2", "ts": 1000.0}), &mut v, 1200.0)); // velha
        assert!(!oferta_serve(&json!({"t": "answer", "sid": "s3", "ts": 1000.0}), &mut v, 1000.0) && !oferta_serve(&json!({"t": "offer"}), &mut v, 0.0));
    }

    #[test]
    fn ninguem_recebe_resposta_antes_do_dono_permitir() {
        let (d, p) = par("permitir");
        let chamou = Cell::new(0);
        let ia = |de: &str, t: &str| -> Result<String, String> {
            chamou.set(chamou.get() + 1);
            Ok(format!("{de}: {t}"))
        };
        assert_eq!(p.permitir(true).unwrap_err(), SEM_PAR);
        assert!(p.codigo().is_err() && p.parear("curto", "loja").is_err());
        let cod = p.novo_codigo(" Loja ", &[7; 16]).unwrap();
        assert_eq!(cod, "HHHH-HHHH-HHHH-HHHH");
        assert_eq!(p.info(&json!({"ouvindo": true})), json!({"ouvindo": true, "par": "Loja", "pareado": true, "recebe": false, "painel": false}));
        assert_eq!(p.codigo().unwrap(), ("HHHHHHHHHHHHHHHH".to_string(), "Loja".to_string()));
        let fala = |t: &str, agora: f64| p.tratar(&json!({"i": "x1", "texto": t}), agora, &ia, &sem_get);
        assert_eq!(fala("oi", 100.0), vec![json!({"i": "x1", "texto": "O dono deste Braniac ainda nao permitiu conversas."})]);
        assert_eq!(chamou.get(), 0);
        assert_eq!(p.tratar(&json!({"i": 1, "t": "ping"}), 100.0, &ia, &sem_get), vec![json!({"i": 1, "texto": "pong", "v": 2, "nome": "jarvis"})]); // ping nao depende de permissao nem gasta IA
        p.permitir(true).unwrap();
        assert_eq!(fala("oi", 100.0)[0]["texto"], "Loja: oi");
        assert_eq!(fala(&"x".repeat(5000), 100.0)[0]["texto"].as_str().map(|t| t.chars().count()), Some(MAX_TEXTO));
        (0..LIMITE_MIN).for_each(|_| drop(fala("mais", 101.0)));
        assert_eq!(fala("mais", 102.0)[0]["texto"], "Muitas mensagens; espere um minuto.");
        assert_eq!(fala("mais", 170.0)[0]["texto"], "Loja: mais"); // passou o minuto
        assert_eq!(p.tratar(&json!({"i": 2, "texto": "oi"}), 300.0, &|_, _| Err("sem IA".into()), &sem_get)[0]["texto"], "Nao consegui responder agora.");
        // parear de novo (codigo novo) zera a permissao
        p.parear("abcd-2345-abcd-2345", "outro").unwrap();
        assert_eq!(p.info(&Value::Null)["recebe"], false);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn painel_so_com_liberacao_so_caminho_simples_e_em_pedacos() {
        let (d, p) = par("painel");
        p.novo_codigo("loja", &[1; 16]).unwrap();
        let pedido = |path: &str, agora: f64, get: &dyn Fn(&str) -> (u16, String, Vec<u8>)| p.tratar(&json!({"i": "p", "t": "painel", "path": path}), agora, &|_, _| Ok(String::new()), get);
        let r = pedido("/", 10.0, &sem_get);
        assert_eq!((r[0]["st"].as_u64(), painel_juntar(&r).2), (Some(403), b"O dono deste Braniac nao liberou o painel.".to_vec()));
        p.permitir_painel(true).unwrap();
        let visto = RefCell::new(Vec::new());
        let get = |url: &str| {
            visto.borrow_mut().push(url.to_string());
            (200, "text/html".to_string(), vec![b'a'; 50_000])
        };
        let r = pedido("/vendas?dia=1", 10.0, &get);
        assert_eq!((r.len(), r[0]["n"].as_u64(), r[2]["k"].as_u64()), (3, Some(3), Some(2))); // 50 KB -> 3 pedacos de base64
        assert_eq!(painel_juntar(&r), (200, "text/html".to_string(), vec![b'a'; 50_000]));
        assert_eq!(*visto.borrow(), ["http://127.0.0.1:8770/vendas?dia=1"]);
        for ruim in ["../segredo", "/a/../b", "//outro.host/x", "/a\\b", "sem-barra", &format!("/{}", "a".repeat(300))] {
            assert_eq!(pedido(ruim, 11.0, &sem_get)[0]["st"], 404, "{ruim}");
        }
        p.definir_painel_porta(9001).unwrap();
        assert_eq!(p.painel_url("/x").as_deref(), Some("http://127.0.0.1:9001/x"));
        // limite de GETs por minuto
        let vazio = |_: &str| (200, "text/plain".to_string(), Vec::new());
        let ultimo = (0..LIMITE_PAINEL + 1).map(|_| pedido("/", 12.0, &vazio)).last().unwrap();
        assert_eq!((ultimo[0]["st"].as_u64(), ultimo[0]["b64"].as_str()), (Some(429), Some("")));
        assert_eq!(pedido("/", 80.0, &vazio)[0]["st"], 200);
        // sem painel_base nao ha painel, mesmo liberado
        let q = Par::new(&d, "jarvis", "");
        assert_eq!(q.tratar(&json!({"t": "painel", "path": "/"}), 1.0, &|_, _| Ok(String::new()), &sem_get)[0]["st"], 403);
        assert_eq!(painel_juntar(&[json!({"i": 1, "texto": "nao entendi"})]).0, 502); // outro lado sem painel
        assert!(pong(&[json!({"texto": "pong"})]) && !pong(&[]) && !pong(&[json!({"texto": "oi"})]));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn conversa_sozinha_para_no_limite() {
        let n = Cell::new(0);
        let falar = |t: &str| -> Result<String, String> {
            n.set(n.get() + 1);
            Ok(format!("resposta a {t}"))
        };
        let h = conversar("oi", &falar, &|h| format!("fala {}", h.len()), 50).unwrap();
        assert_eq!((h.len(), n.get(), h[0].clone(), h[1].1.as_str()), (12, 6, ("eu".to_string(), "oi".to_string()), "resposta a oi")); // no maximo 6 turnos
        let h = conversar("oi", &falar, &|_| String::new(), 4).unwrap();
        assert_eq!(h.len(), 2); // assunto acabou
        assert!(conversar("oi", &|_| Err("fora".into()), &|_| "x".into(), 4).is_err());
    }
}
