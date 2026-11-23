//! Porta LAN TEMPORARIA: uma porta so na rede local para outro computador da casa mandar ARQUIVOS ao PC. Aqui ficam as REGRAS.
//! A prova e a CHAVE (dados/porta_lan.key, nova a cada subida): cada pedido leva HMAC(chave, "ts|nonce|metodo|caminho|sha256-do-corpo").
//! Sem a chave nao entra; pedido velho (mais de 60 s) ou repetido (mesmo nonce) e recusado; so IP de rede privada; 5 erros bloqueiam o IP
//! por 10 min; a porta se desliga sozinha no prazo. O arquivo vai para a quarentena (dados/recebidos_lan): nunca e aberto nem executado,
//! executavel e recusado, o nome e limpo, o tamanho tem teto (200 MB) e o hash declarado tem de bater com o conteudo.
//! Ficou de fora: o servidor HTTP, o temporizador e descobrir o IP da maquina; o HMAC, o SHA-256 e o sorteio da chave entram como closure
//! (nao ha crate para eles).
#![allow(dead_code)]
use crate::empresa_dev::{corta, rx};
use crate::tempo::Agora;
use regex::Regex;
use std::collections::HashMap;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const PORTA: u16 = 8766;
pub const LIMITE: u64 = 200 * 1024 * 1024;
pub const JANELA: f64 = 60.0;
pub const EXEC: [&str; 13] = ["exe", "bat", "cmd", "ps1", "msi", "scr", "vbs", "lnk", "dll", "com", "jar", "reg", "hta"];

/// Erros por IP e nonces ja usados (so na memoria do processo).
#[derive(Default)]
pub struct Memoria {
    erros: HashMap<String, Vec<f64>>,
    nonces: HashMap<String, f64>,
}

/// Rede privada, enlace local ou a propria maquina.
pub fn ip_privado(ip: &str) -> bool {
    match ip.parse::<IpAddr>() {
        Ok(IpAddr::V4(a)) => a.is_private() || a.is_loopback() || a.is_link_local(),
        Ok(IpAddr::V6(a)) => a.is_loopback() || (a.segments()[0] & 0xfe00) == 0xfc00 || (a.segments()[0] & 0xffc0) == 0xfe80,
        Err(_) => false,
    }
}

/// 5 erros em 10 min.
pub fn bloqueado(m: &mut Memoria, ip: &str, ts: f64) -> bool {
    let e = m.erros.entry(ip.to_string()).or_default();
    e.retain(|t| ts - t < 600.0);
    e.len() >= 5
}

/// O texto que a chave assina.
pub fn texto_assinado(ts: &str, nonce: &str, metodo: &str, caminho: &str, sha: &str) -> String {
    format!("{ts}|{nonce}|{metodo}|{caminho}|{sha}")
}

fn igual(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |x, (p, q)| x | (p ^ q)) == 0
}

pub struct Pedido<'a> {
    pub ip: &'a str,
    pub metodo: &'a str,
    pub caminho: &'a str,
    /// Cabecalhos X-Braniac-Ts, X-Braniac-Nonce, X-Braniac-Sig.
    pub ts: &'a str,
    pub nonce: &'a str,
    pub sig: &'a str,
    /// Hash declarado do corpo ("" em pedido sem corpo): a assinatura cobre ele, entao o corpo so e lido depois de autorizar.
    pub sha: &'a str,
}

/// Ok ou (codigo HTTP, motivo). `hmac(chave, texto)` devolve o HMAC-SHA256 em hexadecimal.
pub fn autorizar(m: &mut Memoria, chave: &str, p: &Pedido, agora_ts: f64, hmac: &dyn Fn(&str, &str) -> String) -> Result<(), (u16, &'static str)> {
    if !ip_privado(p.ip) {
        return Err((403, "ip fora da rede local"));
    }
    if bloqueado(m, p.ip, agora_ts) {
        return Err((429, "bloqueado por excesso de erros"));
    }
    let no_prazo = !p.ts.is_empty() && p.ts.bytes().all(|b| b.is_ascii_digit()) && p.ts.parse::<f64>().is_ok_and(|t| (agora_ts - t).abs() <= JANELA);
    let mut ok = no_prazo && (8..=64).contains(&p.nonce.len()) && !chave.is_empty() && igual(&hmac(chave, &texto_assinado(p.ts, p.nonce, p.metodo, p.caminho, p.sha)), p.sig);
    if ok {
        m.nonces.retain(|_, t| agora_ts - *t <= JANELA * 2.0);
        ok = m.nonces.insert(p.nonce.to_string(), agora_ts).is_none(); // pedido repetido nao vale
    }
    if !ok {
        m.erros.entry(p.ip.to_string()).or_default().push(agora_ts);
        return Err((401, "nao autorizado"));
    }
    Ok(())
}

/// So o nome do arquivo, sem pasta e sem caractere estranho.
pub fn nome_seguro(nome: &str) -> String {
    static R: OnceLock<Regex> = OnceLock::new();
    let n = nome.replace('\\', "/");
    let n = rx(&R, r"[^\w.\- ]").replace_all(n.rsplit('/').next().unwrap_or(""), "_");
    let n = corta(n.trim_matches([' ', '.']), 120);
    if n.is_empty() { "arquivo".into() } else { n }
}

fn decodificar(s: &str) -> String {
    let (b, mut out, mut i) = (s.as_bytes(), Vec::new(), 0);
    while i < b.len() {
        let hex = (b[i] == b'%').then(|| s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())).flatten();
        match hex {
            Some(x) => { out.push(x); i += 3 }
            None => { out.push(if b[i] == b'+' { b' ' } else { b[i] }); i += 1 }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Confere o POST de arquivo ANTES de ler o corpo: rota, hash com cara de hash, tamanho e tipo. Devolve o nome limpo.
pub fn checar_arquivo(caminho: &str, sha: &str, tamanho: u64) -> Result<String, (u16, &'static str)> {
    let Some(q) = caminho.strip_prefix("/arquivo?nome=") else { return Err((404, "nao existe")) };
    if sha.len() != 64 || !sha.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) || tamanho > LIMITE {
        return Err((413, "hash invalido ou arquivo acima de 200 MB"));
    }
    let nome = nome_seguro(&decodificar(q.split('&').next().unwrap_or("")));
    let ext = nome.rsplit_once('.').map(|x| x.1.to_lowercase()).unwrap_or_default();
    if EXEC.contains(&ext.as_str()) {
        return Err((415, "executavel recusado"));
    }
    Ok(nome)
}

/// Depois de gravar o parcial: so fica se veio inteiro e o hash do conteudo e o declarado.
pub fn conteudo_confere(faltou: u64, sha_real: &str, sha_declarado: &str) -> Result<(), (u16, &'static str)> {
    if faltou == 0 && igual(sha_real, sha_declarado) { Ok(()) } else { Err((400, "arquivo incompleto ou corrompido")) }
}

pub fn pasta(dados: &Path) -> PathBuf {
    dados.join("recebidos_lan")
}

/// Onde o arquivo aceito fica na quarentena: "AAAAMMDD-HHMMSS-nome".
pub fn destino(dados: &Path, agora: &Agora, nome: &str) -> PathBuf {
    pasta(dados).join(format!("{}-{}", agora.carimbo(), nome_seguro(nome)))
}

/// Chave nova a cada subida, gravada em dados/porta_lan.key. `sorteio(n)` = n bytes aleatorios em hexadecimal.
pub fn nova_chave(dados: &Path, sorteio: &dyn Fn(usize) -> String) -> Result<String, String> {
    std::fs::create_dir_all(pasta(dados)).map_err(|e| e.to_string())?;
    let k = sorteio(32);
    if k.len() < 64 {
        return Err("chave curta demais".into());
    }
    std::fs::write(dados.join("porta_lan.key"), &k).map_err(|e| e.to_string())?;
    Ok(k)
}

/// A porta e temporaria: passou do prazo, desliga.
pub fn vencida(subiu_ts: f64, minutos: f64, agora_ts: f64) -> bool {
    agora_ts - subiu_ts >= minutos * 60.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;

    fn hmac(chave: &str, texto: &str) -> String {
        format!("mac[{chave}]({texto})")
    }

    fn ped<'a>(ip: &'a str, ts: &'a str, nonce: &'a str, sig: &'a str) -> Pedido<'a> {
        Pedido { ip, metodo: "GET", caminho: "/ping", ts, nonce, sig, sha: "" }
    }

    #[test]
    fn so_entra_com_chave_no_prazo_da_rede_local_e_uma_vez() {
        let (mut m, k) = (Memoria::default(), "chave-certa");
        let sig = hmac(k, &texto_assinado("1000", "nonce-0001", "GET", "/ping", ""));
        assert_eq!(autorizar(&mut m, k, &ped("192.168.0.7", "1000", "nonce-0001", &sig), 1010.0, &hmac), Ok(()));
        assert_eq!(autorizar(&mut m, k, &ped("192.168.0.7", "1000", "nonce-0001", &sig), 1011.0, &hmac), Err((401, "nao autorizado"))); // repetido
        assert_eq!(autorizar(&mut m, k, &ped("8.8.8.8", "1000", "nonce-0002", &sig), 1010.0, &hmac), Err((403, "ip fora da rede local")));
        assert_eq!(autorizar(&mut m, k, &ped("nao-e-ip", "1000", "nonce-0002", &sig), 1010.0, &hmac).unwrap_err().0, 403);
        let mut m = Memoria::default();
        let s2 = hmac(k, &texto_assinado("1000", "nonce-0002", "GET", "/ping", ""));
        assert_eq!(autorizar(&mut m, k, &ped("10.0.0.2", "1000", "nonce-0002", &s2), 1061.0, &hmac).unwrap_err().0, 401); // velho
        assert_eq!(autorizar(&mut m, "outra-chave", &ped("10.0.0.2", "1000", "nonce-0002", &s2), 1010.0, &hmac).unwrap_err().0, 401); // chave errada
        assert_eq!(autorizar(&mut m, "", &ped("10.0.0.2", "1000", "nonce-0002", &hmac("", "1000|nonce-0002|GET|/ping|")), 1010.0, &hmac).unwrap_err().0, 401); // sem chave nao ha porta
        let curto = hmac(k, &texto_assinado("1000", "curto", "GET", "/ping", ""));
        assert_eq!(autorizar(&mut m, k, &ped("10.0.0.2", "1000", "curto", &curto), 1010.0, &hmac).unwrap_err().0, 401);
        assert_eq!(autorizar(&mut m, k, &ped("10.0.0.2", "-1000", "nonce-0002", &s2), 1010.0, &hmac).unwrap_err().0, 401);
        // a assinatura cobre metodo, caminho e hash do corpo: trocar qualquer um invalida
        let outro = Pedido { metodo: "POST", caminho: "/arquivo?nome=a.txt", sha: "ab", ..ped("10.0.0.3", "1000", "nonce-0003", &s2) };
        assert_eq!(autorizar(&mut m, k, &outro, 1010.0, &hmac).unwrap_err().0, 401);
        assert!(ip_privado("127.0.0.1") && ip_privado("172.16.5.5") && ip_privado("169.254.1.1") && ip_privado("::1") && ip_privado("fd00::1") && !ip_privado("172.32.0.1") && !ip_privado("2001:db8::1"));
    }

    #[test]
    fn cinco_erros_bloqueiam_o_ip_por_dez_minutos() {
        let (mut m, k) = (Memoria::default(), "k");
        for i in 0..5 {
            assert_eq!(autorizar(&mut m, k, &ped("192.168.0.9", "1000", &format!("nonce-ruim-{i}"), "errada"), 1000.0, &hmac).unwrap_err().0, 401);
        }
        let sig = hmac(k, &texto_assinado("1000", "nonce-bom-1", "GET", "/ping", ""));
        assert_eq!(autorizar(&mut m, k, &ped("192.168.0.9", "1000", "nonce-bom-1", &sig), 1001.0, &hmac), Err((429, "bloqueado por excesso de erros")));
        assert_eq!(autorizar(&mut m, k, &ped("192.168.0.10", "1000", "nonce-bom-1", &sig), 1001.0, &hmac), Ok(())); // outro IP segue
        let s2 = hmac(k, &texto_assinado("1600", "nonce-bom-2", "GET", "/ping", ""));
        assert_eq!(autorizar(&mut m, k, &ped("192.168.0.9", "1600", "nonce-bom-2", &s2), 1601.0, &hmac), Ok(())); // 10 min depois
    }

    #[test]
    fn nome_limpo_executavel_recusado_tamanho_hash_e_prazo() {
        assert_eq!(nome_seguro("../../etc/passwd"), "passwd");
        assert_eq!(nome_seguro("..\\..\\a b.rs"), "a b.rs");
        assert!(nome_seguro("") == "arquivo" && nome_seguro("...") == "arquivo" && nome_seguro("relatório<1>.pdf") == "relatório_1_.pdf");
        assert_eq!(nome_seguro(&"x".repeat(300)).chars().count(), 120);
        let sha = "a".repeat(64);
        assert_eq!(checar_arquivo("/arquivo?nome=a%20b.txt", &sha, 10).as_deref(), Ok("a b.txt"));
        assert_eq!(checar_arquivo("/arquivo?nome=..%2F..%2Fnota+1.md&x=1", &sha, 10).as_deref(), Ok("nota 1.md"));
        for exe in ["x.exe", "x.EXE", "virus.bat", "a.b.ps1", "x.exe.", "x.exe%20", "atalho.lnk", "pasta%2Fx.dll"] {
            assert_eq!(checar_arquivo(&format!("/arquivo?nome={exe}"), &sha, 10), Err((415, "executavel recusado")), "{exe}");
        }
        assert_eq!(checar_arquivo("/arquivo?nome=x.exe:fluxo", &sha, 10).as_deref(), Ok("x.exe_fluxo")); // sem fluxo alternativo: vira nome comum
        assert_eq!(checar_arquivo("/arquivo?nome=a.txt", &sha, LIMITE + 1).unwrap_err().0, 413);
        assert_eq!(checar_arquivo("/arquivo?nome=a.txt", "0", 1).unwrap_err().0, 413);
        assert_eq!(checar_arquivo("/arquivo?nome=a.txt", &"G".repeat(64), 1).unwrap_err().0, 413);
        assert_eq!(checar_arquivo("/outra?nome=a.txt", &sha, 1).unwrap_err().0, 404);
        assert!(conteudo_confere(0, &sha, &sha).is_ok() && conteudo_confere(1, &sha, &sha).is_err() && conteudo_confere(0, &"b".repeat(64), &sha).is_err());
        let d = std::env::temp_dir().join(format!("braniac-porta_lan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let agora = Agora { data: Data::civil(2026, 11, 18).unwrap(), hora: 9, min: 8, seg: 7 };
        assert!(destino(&d, &agora, "../a.txt").ends_with("recebidos_lan/20261118-090807-a.txt"));
        assert!(nova_chave(&d, &|_| "curta".into()).is_err());
        let k = nova_chave(&d, &|n| "ab".repeat(n)).unwrap();
        assert!(k.len() == 64 && std::fs::read_to_string(d.join("porta_lan.key")).unwrap() == k && pasta(&d).is_dir());
        assert!(!vencida(1000.0, 120.0, 1000.0 + 7199.0) && vencida(1000.0, 120.0, 1000.0 + 7200.0));
        let _ = std::fs::remove_dir_all(&d);
    }
}
