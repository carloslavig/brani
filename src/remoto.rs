//! ACESSO REMOTO do celular (remoto.py): o app do celular conversa com o Braniac deste PC por um canal direto, pareado por CODIGO.
//! Desligado de fabrica: so existe com o botao "Acesso remoto" ligado no PC. Regras que ficam aqui: o codigo e o topico, a oferta
//! que vale (so "offer", sessao nova, dentro de 2 min), o que o celular NAO alcanca (nada que arme ou ligue alguma coisa:
//! /par, /api/par, /api/remoto, /api/autonomia, /api/atendimento-auto, /api/airbnb-auto) e a resposta em quadros de 30 mil caracteres.
//! Ficou de fora (closure de quem chama): WebRTC/aiortc, a sinalizacao (ntfy), a cifra AES-GCM/HKDF, o sorteio do codigo (16 bytes
//! aleatorios de quem chama) e o pedido HTTP ao servidor local.
#![allow(dead_code)]
use crate::link_identidade::{hex, sha256};
use crate::par_webrtc::{codigo_de, formatar, norm, pedacos};
use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::io::Write;
use std::path::Path;

pub const PORTA: u16 = 8765;
pub const FRAME: usize = 30000; // caracteres base64 por mensagem do canal (limite seguro)
pub const JANELA_S: f64 = 120.0;
/// So no PC: nada que arme ou ligue alguma coisa pode vir pelo celular.
pub const SO_NO_PC: [&str; 6] = ["/par", "/api/par", "/api/remoto", "/api/autonomia", "/api/atendimento-auto", "/api/airbnb-auto"];

fn ler(dados: &Path) -> Value {
    std::fs::read_to_string(dados.join("remoto.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}))
}

fn salvar(dados: &Path, d: &Value) -> Result<(), String> {
    std::fs::create_dir_all(dados).map_err(|e| e.to_string())?;
    std::fs::write(dados.join("remoto.json"), d.to_string()).map_err(|e| e.to_string())
}

/// Troca o codigo de pareamento (o antigo para de valer). Devolve no formato XXXX-XXXX-XXXX-XXXX.
pub fn novo_codigo(dados: &Path, bruto: &[u8; 16]) -> Result<String, String> {
    let (cod, mut d) = (codigo_de(bruto), ler(dados));
    d["codigo"] = json!(cod);
    salvar(dados, &d)?;
    Ok(formatar(&cod))
}

/// O codigo atual; se ainda nao existe, cria um com `bruto`.
pub fn codigo(dados: &Path, bruto: &[u8; 16]) -> Result<String, String> {
    match ler(dados)["codigo"].as_str().filter(|c| !c.is_empty()) {
        Some(c) => Ok(c.to_string()),
        None => novo_codigo(dados, bruto).map(|c| norm(&c)),
    }
}

/// Topico da sinalizacao derivado do codigo (igual no celular).
pub fn topico(cod: &str) -> String {
    format!("braniac-{}", &hex(&sha256(format!("{}|topico", norm(cod)).as_bytes()))[..32])
}

pub fn barrado(path: &str) -> bool {
    SO_NO_PC.iter().any(|p| path.starts_with(p))
}

/// A oferta decifrada serve? So "offer", sessao ainda nao vista e com data dentro de 2 min.
pub fn oferta_serve(o: &Value, vistos: &mut HashSet<String>, agora: f64) -> bool {
    crate::par_webrtc::oferta_serve(o, vistos, agora)
}

pub struct Pedido {
    pub metodo: String,
    pub url: String,
    pub origem: String,
    pub tipo: Option<String>,
    pub corpo: Option<Vec<u8>>,
}

/// Executa a requisicao do celular no servidor local: (status, content-type, corpo). Caminho so do PC = 403 sem nem chamar.
/// `http` devolve Err quando o servidor local nao respondeu (vira 502).
pub fn local(req: &Value, http: &dyn Fn(&Pedido) -> Result<(u16, String, Vec<u8>), String>) -> (u16, String, Vec<u8>) {
    let json_ = || "application/json".to_string();
    let path = req["p"].as_str().unwrap_or("/");
    if barrado(path) || !path.starts_with('/') {
        return (403, json_(), br#"{"erro":"so no PC"}"#.to_vec());
    }
    let corpo = match req["b"].as_str().filter(|b| !b.is_empty()).map(crate::base64::decodificar) {
        Some(Err(_)) => return (400, json_(), br#"{"erro":"corpo invalido"}"#.to_vec()),
        Some(Ok(b)) => Some(b),
        None => None,
    };
    let p = Pedido { metodo: req["m"].as_str().unwrap_or("GET").to_string(), url: format!("http://127.0.0.1:{PORTA}{path}"), origem: format!("http://127.0.0.1:{PORTA}"), tipo: req["c"].as_str().filter(|c| !c.is_empty()).map(str::to_string), corpo };
    http(&p).unwrap_or_else(|e| (502, json_(), json!({ "erro": e.chars().take(100).collect::<String>() }).to_string().into_bytes()))
}

/// A resposta ao celular, em quadros: {i, s, c, n, x, b}.
pub fn responder(msg: &Value, http: &dyn Fn(&Pedido) -> Result<(u16, String, Vec<u8>), String>) -> Vec<Value> {
    let (st, ct, corpo) = local(msg, http);
    let partes = pedacos(&corpo, FRAME);
    let n = partes.len();
    partes.into_iter().enumerate().map(|(x, b)| json!({ "i": msg["i"], "s": st, "c": ct, "n": n, "x": x, "b": b })).collect()
}

/// Diagnostico da oferta: quantos caminhos de cada tipo o celular ofereceu (so estado de rede, nunca codigo nem conteudo).
pub fn caminhos(sdp: &str) -> String {
    let r = Regex::new(r"typ (host|srflx|relay)").expect("regex fixa");
    let tipos: Vec<&str> = r.captures_iter(sdp).filter_map(|m| m.get(1).map(|x| x.as_str())).collect();
    format!("celular ofereceu caminhos: {}", ["host", "srflx", "relay"].iter().map(|t| format!("{t}={}", tipos.iter().filter(|x| *x == t).count())).collect::<Vec<_>>().join(", "))
}

#[derive(Default, Debug, PartialEq)]
pub struct Estado {
    pub ligado: bool,
    pub conectado: bool,
    pub ultimo: String,
    pub erro: String,
    pub tentativas: Vec<String>,
}

impl Estado {
    /// Guarda as 6 ultimas tentativas e grava no remoto.log (sobrevive a reinicio). `quando` = "AAAA-MM-DD HH:MM:SS".
    pub fn tentou(&mut self, dados: &Path, quando: &str, msg: &str) {
        self.tentativas.push(format!("{} {msg}", quando.rsplit(' ').next().unwrap_or("")));
        let sobra = self.tentativas.len().saturating_sub(6);
        self.tentativas.drain(..sobra);
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dados.join("remoto.log")) {
            let _ = writeln!(f, "{quando} {msg}");
        }
    }

    /// Liga (devolve true se quem chama deve subir a escuta agora). `salvar` = a escolha vale depois de reiniciar.
    pub fn ligar(&mut self, dados: &Path, salvar_escolha: bool) -> Result<bool, String> {
        if salvar_escolha {
            let mut d = ler(dados);
            d["ligado"] = json!(true);
            salvar(dados, &d)?;
        }
        Ok(!std::mem::replace(&mut self.ligado, true))
    }

    pub fn desligar(&mut self, dados: &Path, salvar_escolha: bool) -> Result<(), String> {
        if salvar_escolha {
            let mut d = ler(dados);
            d["ligado"] = json!(false);
            salvar(dados, &d)?;
        }
        (self.ligado, self.conectado) = (false, false);
        Ok(())
    }

    pub fn json(&self, codigo: &str) -> Value {
        json!({ "ligado": self.ligado, "conectado": self.conectado, "ultimo": self.ultimo, "erro": self.erro, "tentativas": self.tentativas, "codigo": formatar(&norm(codigo)) })
    }
}

/// Ao ligar o Braniac: so volta a ouvir se o dono tinha deixado ligado.
pub fn deve_retomar(dados: &Path) -> bool {
    ler(dados)["ligado"] == true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn tmp(nome: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-remoto-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn codigo_de_16_caracteres_e_topico_igual_ao_do_python() {
        let d = tmp("codigo");
        let c = novo_codigo(&d, &[3; 16]).unwrap();
        assert_eq!((c.as_str(), c.replace('-', "").len()), ("DDDD-DDDD-DDDD-DDDD", 16));
        assert_eq!(codigo(&d, &[9; 16]).unwrap(), "DDDDDDDDDDDDDDDD"); // ja existe: nao troca
        assert_eq!(codigo(&tmp("codigo2"), &[9; 16]).unwrap(), "JJJJJJJJJJJJJJJJ");
        assert_eq!(topico("abcd-2345 abcd-2345"), "braniac-5d854f9eb5f75f3083e9a967eaccb599");
        assert_eq!(topico("ABCD2345ABCD2345"), topico("abcd-2345-abcd-2345"));
    }

    #[test]
    fn celular_nao_alcanca_o_que_liga_ou_arma() {
        let chamadas = RefCell::new(Vec::new());
        let http = |p: &Pedido| -> Result<(u16, String, Vec<u8>), String> {
            chamadas.borrow_mut().push((p.metodo.clone(), p.url.clone(), p.tipo.clone(), p.corpo.clone(), p.origem.clone()));
            Ok((200, "text/plain".into(), b"ok".to_vec()))
        };
        for caminho in ["/api/remoto", "/api/remoto/ligar", "/api/par/link/tunel", "/par/painel/", "/api/autonomia", "/api/atendimento-auto", "/api/airbnb-auto", "sem-barra", "//evil"] {
            let (st, _, corpo) = local(&json!({"p": caminho, "m": "POST"}), &http);
            if caminho != "//evil" {
                assert!(st == 403 && String::from_utf8_lossy(&corpo).contains("so no PC"), "{caminho}");
            }
        }
        chamadas.borrow_mut().clear();
        assert_eq!(local(&json!({"p": "/api/status"}), &http), (200, "text/plain".to_string(), b"ok".to_vec()));
        assert_eq!(local(&json!({"p": "/api/chat", "m": "POST", "c": "application/json", "b": "e30="}), &http).0, 200);
        let c = chamadas.borrow();
        assert_eq!(c[0], ("GET".to_string(), "http://127.0.0.1:8765/api/status".to_string(), None, None, "http://127.0.0.1:8765".to_string()));
        assert_eq!((c[1].0.as_str(), c[1].2.as_deref(), c[1].3.as_deref()), ("POST", Some("application/json"), Some(b"{}".as_slice())));
        assert_eq!(local(&json!({"p": "/x", "b": "!!!"}), &http).0, 400);
        assert_eq!(local(&json!({"p": "/x"}), &|_| Err("servidor parado".into())).0, 502);
    }

    #[test]
    fn resposta_em_quadros_oferta_e_estado() {
        let grande = |_: &Pedido| -> Result<(u16, String, Vec<u8>), String> { Ok((200, "text/html".into(), vec![b'z'; 45_000])) };
        let q = responder(&json!({"i": 7, "p": "/b2"}), &grande);
        assert_eq!((q.len(), q[0]["n"].as_u64(), q[1]["x"].as_u64(), q[0]["b"].as_str().map(str::len), q[0]["i"].as_u64()), (2, Some(2), Some(1), Some(FRAME), Some(7)));
        let q = responder(&json!({"i": 8, "p": "/api/remoto"}), &grande);
        assert_eq!((q.len(), q[0]["s"].as_u64()), (1, Some(403)));
        let mut v = HashSet::new();
        assert!(oferta_serve(&json!({"t": "offer", "sid": "a", "ts": 500.0}), &mut v, 600.0) && !oferta_serve(&json!({"t": "offer", "sid": "a", "ts": 500.0}), &mut v, 600.0));
        assert!(!oferta_serve(&json!({"t": "offer", "sid": "b", "ts": 500.0}), &mut v, 700.0));
        assert_eq!(caminhos("a=candidate:1 typ host\r\na=candidate:2 typ host\r\na=candidate:3 typ srflx"), "celular ofereceu caminhos: host=2, srflx=1, relay=0");
        let d = tmp("estado");
        std::fs::create_dir_all(&d).unwrap();
        let mut e = Estado::default();
        assert!(!deve_retomar(&d)); // desligado de fabrica
        assert!(e.ligar(&d, true).unwrap() && !e.ligar(&d, false).unwrap()); // ja ligado: nao sobe duas escutas
        assert!(deve_retomar(&d));
        (0..8).for_each(|i| e.tentou(&d, "2026-11-18 09:05:00", &format!("rede: {i}")));
        assert_eq!((e.tentativas.len(), e.tentativas[5].as_str()), (6, "09:05:00 rede: 7"));
        assert_eq!(std::fs::read_to_string(d.join("remoto.log")).unwrap().lines().count(), 8);
        e.conectado = true;
        e.desligar(&d, true).unwrap();
        assert!(!deve_retomar(&d) && !e.ligado && !e.conectado);
        assert_eq!(e.json("dddddddddddddddd")["codigo"], "DDDD-DDDD-DDDD-DDDD");
        let _ = std::fs::remove_dir_all(d);
    }
}
