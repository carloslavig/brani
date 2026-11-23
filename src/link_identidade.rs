//! Parte SEM chave do link/identidade.py: id do dispositivo ("bl1-" + 20 hex do SHA-256 da chave publica), base64 de URL sem "=",
//! SHA-256 e HMAC-SHA-256 escritos aqui (nao ha crate de criptografia). Ficou de fora: gerar/guardar a chave Ed25519, assinar e
//! verificar assinatura. Quem chama os modulos do Link passa essas duas operacoes como closure.
#![allow(dead_code)]
use std::sync::atomic::{AtomicU64, Ordering};

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
    0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
    0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

pub fn sha256(dados: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut m = dados.to_vec();
    m.push(0x80);
    while m.len() % 64 != 56 {
        m.push(0);
    }
    m.extend_from_slice(&((dados.len() as u64) * 8).to_be_bytes());
    for bloco in m.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([bloco[4 * i], bloco[4 * i + 1], bloco[4 * i + 2], bloco[4 * i + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let t2 = s0.wrapping_add((v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]));
            v = [t1.wrapping_add(t2), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]];
        }
        for i in 0..8 {
            h[i] = h[i].wrapping_add(v[i]);
        }
    }
    let mut out = [0u8; 32];
    for (i, x) in h.iter().enumerate() {
        out[4 * i..4 * i + 4].copy_from_slice(&x.to_be_bytes());
    }
    out
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn hmac_sha256(chave: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut k = if chave.len() > 64 { sha256(chave).to_vec() } else { chave.to_vec() };
    k.resize(64, 0);
    let mut dentro: Vec<u8> = k.iter().map(|b| b ^ 0x36).collect();
    dentro.extend_from_slice(msg);
    let mut fora: Vec<u8> = k.iter().map(|b| b ^ 0x5c).collect();
    fora.extend_from_slice(&sha256(&dentro));
    sha256(&fora)
}

/// Comparacao em tempo constante (como hmac.compare_digest).
pub fn iguais(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

/// Base64 de URL sem "=" no fim.
pub fn b64(b: &[u8]) -> String {
    crate::base64::codificar(b).trim_end_matches('=').replace('+', "-").replace('/', "_")
}

pub fn unb64(s: &str) -> Result<Vec<u8>, String> {
    if s.contains(['+', '/']) {
        return Err("base64 invalido".into());
    }
    crate::base64::decodificar(&s.replace('-', "+").replace('_', "/"))
}

pub fn id_de(pub_b64: &str) -> Result<String, String> {
    Ok(format!("bl1-{}", &hex(&sha256(&unb64(pub_b64)?))[..20]))
}

static CONTADOR: AtomicU64 = AtomicU64::new(0);

/// Id novo com `n` caracteres hex (ate 64). Serve para id de envelope e de job: nao e segredo, so nao pode repetir.
// shortcut: sai do relogio + contador + pid, nao de sorteio do sistema; o que e SEGREDO (o do convite) vem de fora, por parametro
pub fn id_novo(n: usize) -> String {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let semente = format!("{t}|{}|{}", CONTADOR.fetch_add(1, Ordering::Relaxed), std::process::id());
    hex(&sha256(semente.as_bytes()))[..n.min(64)].to_string()
}

/// A parte publica de um dispositivo: quem ele diz que e. A chave privada nunca passa por aqui.
#[derive(Clone, Debug)]
pub struct Ident {
    pub id: String,
    pub chave_pub: String,
    pub nome: String,
}

/// Assinatura de MENTIRA, so para os testes dos modulos do Link (a de verdade e Ed25519 e vem de fora, por closure).
#[cfg(test)]
pub mod falso {
    use super::*;

    pub fn ident(nome: &str) -> Ident {
        let chave_pub = b64(&sha256(nome.as_bytes()));
        Ident { id: id_de(&chave_pub).unwrap(), chave_pub, nome: nome.to_string() }
    }

    pub fn assinar(i: &Ident, dados: &[u8]) -> String {
        b64(&hmac_sha256(i.chave_pub.as_bytes(), dados))
    }

    pub fn verificar(chave_pub: &str, dados: &[u8], sig: &str) -> bool {
        b64(&hmac_sha256(chave_pub.as_bytes(), dados)) == sig
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha_e_hmac_batem_com_os_vetores_conhecidos() {
        assert_eq!(hex(&sha256(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(hex(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(hex(&sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")), "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1");
        assert_eq!(hex(&sha256(&[b'a'; 1000])), "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3");
        // RFC 4231, caso 2
        assert_eq!(hex(&hmac_sha256(b"Jefe", b"what do ya want for nothing?")), "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
        assert!(iguais(b"abc", b"abc") && !iguais(b"abc", b"abd") && !iguais(b"abc", b"ab"));
    }

    #[test]
    fn base64_de_url_e_id() {
        assert_eq!(b64(&[0xfb, 0xff, 0xfe]), "-__-");
        assert_eq!(unb64("-__-").unwrap(), vec![0xfb, 0xff, 0xfe]);
        assert_eq!(unb64(&b64(b"oi")).unwrap(), b"oi");
        assert!(unb64("+/+/").is_err() && unb64("!!").is_err());
        let id = id_de(&b64(&[7u8; 32])).unwrap();
        assert!(id.starts_with("bl1-") && id.len() == 24);
        assert!(id_de("!").is_err());
        assert!(id_novo(16) != id_novo(16) && id_novo(12).len() == 12);
    }
}
