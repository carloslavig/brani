//! Base64 padrao (RFC 4648), sem crate. Usado para guardar as chaves criptografadas no .env.
//! `codificar` e do Otto (NexoDev, T066; a entrega veio cortada); `decodificar` e os testes sao do Claude.
/// Alfabeto Base64 padrão.
const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Tabela de inversão (valor -> índice). -1 indica caractere inválido.
static REVERSE: [i8; 256] = {
    let mut map = [ -1i8; 256];
    let mut i = 0;
    while i < 64 {
        map[ALPHABET[i] as usize] = i as i8;
        i += 1;
    }
    map
};

/// Codifica bytes em string Base64.
pub fn codificar(dados: &[u8]) -> String {
    let mut out = String::with_capacity((dados.len() + 2) / 3 * 4);
    let mut i = 0;
    while i < dados.len() {
        let b0 = dados[i];
        let b1 = *dados.get(i + 1).unwrap_or(&0);
        let b2 = *dados.get(i + 2).unwrap_or(&0);
        out.push(ALPHABET[(b0 >> 2) as usize] as char);
        out.push(ALPHABET[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if i + 1 < dados.len() {
            out.push(ALPHABET[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if i + 2 < dados.len() {
            out.push(ALPHABET[(b2 & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
        i += 3;
    }
    out
}

/// Decodifica base64. Ignora espacos e quebras de linha; aceita com ou sem "=" no fim.
pub fn decodificar(texto: &str) -> Result<Vec<u8>, String> {
    let ruim = || "base64 invalido".to_string();
    let limpo: Vec<u8> = texto.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    let fim = limpo.iter().position(|&b| b == b'=').unwrap_or(limpo.len());
    // depois do primeiro "=" so pode vir "=" (no maximo dois), e sobra de 1 caractere e tamanho impossivel
    if limpo.len() - fim > 2 || limpo[fim..].iter().any(|&b| b != b'=') || fim % 4 == 1 {
        return Err(ruim());
    }
    let mut out = Vec::with_capacity(fim * 3 / 4);
    for grupo in limpo[..fim].chunks(4) {
        let mut n: u32 = 0;
        for &b in grupo {
            let v = REVERSE[b as usize];
            if v < 0 {
                return Err(ruim());
            }
            n = (n << 6) | v as u32;
        }
        n <<= 6 * (4 - grupo.len());
        out.extend_from_slice(&n.to_be_bytes()[1..grupo.len()]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vetores_da_rfc_nos_dois_sentidos() {
        for (aberto, cod) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")] {
            assert_eq!(codificar(aberto.as_bytes()), cod);
            assert_eq!(decodificar(cod).unwrap(), aberto.as_bytes());
        }
    }

    #[test]
    fn todos_os_bytes_vao_e_voltam() {
        let todos: Vec<u8> = (0..=255).collect();
        for n in [0, 1, 2, 3, 254, 255, 256] {
            assert_eq!(decodificar(&codificar(&todos[..n])).unwrap(), &todos[..n]);
        }
    }

    #[test]
    fn tolera_quebra_de_linha_e_falta_de_preenchimento_mas_recusa_lixo() {
        assert_eq!(decodificar("Zm9v\nYmFy ").unwrap(), b"foobar");
        assert_eq!(decodificar("Zg").unwrap(), b"f");
        for ruim in ["Zm9*", "Z", "Zg==Zg", "Zg===", "=Zg=", "Zm9vé"] {
            assert!(decodificar(ruim).is_err(), "{ruim}");
        }
    }
}
