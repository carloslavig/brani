//! Cliente da porta LAN do Braniac (porta_lan_cliente.py): assina cada pedido com a chave combinada (HMAC-SHA256 de
//! "ts|nonce|metodo|caminho|sha") e manda arquivos para o PC na rede local. A chave nunca e impressa nem vai em log.
//! Ficou de fora (closure de quem chama): a conexao HTTP, o sorteio do nonce e descobrir o IP local.
#![allow(dead_code)]
use crate::link_fila_cliente::quote;
use crate::link_identidade::{hex, hmac_sha256, sha256};
use crate::porta_lan::{texto_assinado, PORTA};
use serde_json::Value;
use std::path::Path;

/// Cabecalhos de assinatura. `nonce`: 24 hex sorteados por quem chama (cada pedido um novo).
pub fn assinar(chave: &str, metodo: &str, caminho: &str, sha: &str, ts: i64, nonce: &str) -> Vec<(String, String)> {
    let sig = hex(&hmac_sha256(chave.as_bytes(), texto_assinado(&ts.to_string(), nonce, metodo, caminho, sha).as_bytes()));
    vec![("X-Braniac-Ts".into(), ts.to_string()), ("X-Braniac-Nonce".into(), nonce.into()), ("X-Braniac-Sig".into(), sig)]
}

pub fn caminho_arquivo(arquivo: &Path) -> String {
    format!("/arquivo?nome={}", quote(&arquivo.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()).replace('/', "%2F"))
}

/// Pedido assinado. Com `arquivo`, o corpo e o arquivo (com o SHA-256 dele no cabecalho). `http(ip, porta, metodo, caminho,
/// cabecalhos, corpo)` devolve (status, corpo).
#[allow(clippy::too_many_arguments)]
pub fn pedido(ip: &str, chave: &str, metodo: &str, caminho: &str, arquivo: Option<&Path>, ts: i64, nonce: &str, http: &dyn Fn(&str, u16, &str, &str, &[(String, String)], Option<&[u8]>) -> Result<(u16, Vec<u8>), String>) -> Result<(u16, Value), String> {
    let corpo = arquivo.map(|a| std::fs::read(a).map_err(|e| format!("nao li {}: {e}", a.display()))).transpose()?;
    let sha = corpo.as_ref().map(|c| hex(&sha256(c))).unwrap_or_default();
    let mut h = assinar(chave, metodo, caminho, &sha, ts, nonce);
    if let Some(c) = &corpo {
        h.push(("X-Body-Sha256".into(), sha));
        h.push(("Content-Length".into(), c.len().to_string()));
    }
    let (st, resp) = http(ip, PORTA, metodo, caminho, &h, corpo.as_deref())?;
    Ok((st, if resp.is_empty() { serde_json::json!({}) } else { serde_json::from_slice(&resp).map_err(|_| "resposta ilegivel".to_string())? }))
}

/// Envia um arquivo ao PC.
pub fn enviar(ip: &str, chave: &str, arquivo: &Path, ts: i64, nonce: &str, http: &dyn Fn(&str, u16, &str, &str, &[(String, String)], Option<&[u8]>) -> Result<(u16, Vec<u8>), String>) -> Result<(u16, Value), String> {
    pedido(ip, chave, "POST", &caminho_arquivo(arquivo), Some(arquivo), ts, nonce, http)
}

/// Os 254 enderecos da mesma rede do IP local, para procurar o PC.
pub fn candidatos(ip_local: &str) -> Vec<String> {
    match ip_local.rsplit_once('.') {
        Some((base, _)) if base.split('.').count() == 3 => (1..255).map(|n| format!("{base}.{n}")).collect(),
        _ => Vec::new(),
    }
}

/// A resposta do GET /ola e mesmo da porta LAN do Braniac?
pub fn eh_porta_lan(resposta: &[u8]) -> bool {
    resposta.windows(9).any(|w| w == b"porta-lan")
}

/// Procura o PC na rede: `ola(ip)` faz o GET /ola (None = nao respondeu).
pub fn achar(ip_local: &str, ola: &dyn Fn(&str) -> Option<Vec<u8>>) -> Vec<String> {
    candidatos(ip_local).into_iter().filter(|ip| ola(ip).is_some_and(|r| eh_porta_lan(&r))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::porta_lan::{autorizar, Memoria, Pedido};
    use std::cell::RefCell;

    #[test]
    fn o_servidor_aceita_o_que_o_cliente_assina() {
        let d = std::env::temp_dir().join(format!("braniac-porta_lan_cliente-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let arq = d.join("meu video.mp4");
        std::fs::write(&arq, b"conteudo").unwrap();
        let chave = "c".repeat(64);
        let visto = RefCell::new(Vec::new());
        let http = |ip: &str, porta: u16, m: &str, c: &str, h: &[(String, String)], corpo: Option<&[u8]>| -> Result<(u16, Vec<u8>), String> {
            visto.borrow_mut().push((format!("{ip}:{porta}"), m.to_string(), c.to_string(), h.to_vec(), corpo.map(<[u8]>::to_vec)));
            Ok((200, br#"{"ok":true}"#.to_vec()))
        };
        assert_eq!(enviar("192.168.0.10", &chave, &arq, 1000, "aabbccddeeff001122334455", &http).unwrap(), (200, serde_json::json!({"ok": true})));
        let v = visto.borrow();
        let (onde, m, c, h, corpo) = &v[0];
        assert_eq!((onde.as_str(), m.as_str(), c.as_str(), corpo.as_deref()), ("192.168.0.10:8766", "POST", "/arquivo?nome=meu%20video.mp4", Some(b"conteudo".as_slice())));
        let cab = |k: &str| h.iter().find(|x| x.0 == k).map(|x| x.1.clone()).unwrap_or_default();
        assert_eq!((cab("X-Body-Sha256"), cab("Content-Length")), (hex(&sha256(b"conteudo")), "8".to_string()));
        // a assinatura do cliente passa na conferencia do servidor (porta_lan::autorizar) e outra chave nao
        let hm = |k: &str, t: &str| hex(&hmac_sha256(k.as_bytes(), t.as_bytes()));
        let (ts, nonce, sig, sha) = (cab("X-Braniac-Ts"), cab("X-Braniac-Nonce"), cab("X-Braniac-Sig"), cab("X-Body-Sha256"));
        let p = Pedido { ip: "192.168.0.20", ts: &ts, nonce: &nonce, sig: &sig, metodo: "POST", caminho: c, sha: &sha };
        assert_eq!(autorizar(&mut Memoria::default(), &chave, &p, 1001.0, &hm), Ok(()));
        assert!(autorizar(&mut Memoria::default(), &"d".repeat(64), &p, 1001.0, &hm).is_err());
        // ping sem corpo: so os tres cabecalhos
        assert_eq!(pedido("192.168.0.10", &chave, "GET", "/ping", None, 1000, "00", &|_, _, _, _, h, c| Ok((if h.len() == 3 && c.is_none() { 200 } else { 500 }, Vec::new()))).unwrap(), (200, serde_json::json!({})));
        assert!(pedido("x", &chave, "POST", "/arquivo", Some(&d.join("nao-existe")), 1, "00", &http).is_err());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn procura_na_rede_local() {
        assert_eq!((candidatos("192.168.0.57").len(), candidatos("192.168.0.57")[0].as_str(), candidatos("192.168.0.57")[253].as_str()), (254, "192.168.0.1", "192.168.0.254"));
        assert!(candidatos("localhost").is_empty());
        let ola = |ip: &str| match ip {
            "10.0.0.7" => Some(br#"{"servico":"porta-lan"}"#.to_vec()),
            "10.0.0.9" => Some(b"<html>roteador</html>".to_vec()),
            _ => None,
        };
        assert_eq!(achar("10.0.0.3", &ola), ["10.0.0.7"]);
    }
}
