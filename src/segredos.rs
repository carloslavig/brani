//! Chaves criptografadas no .env. Porte de braniac/segredos.py, mesmo formato: NOME=dpapi:<base64>.
//! Usa o DPAPI do Windows: so a conta do Windows da pessoa, naquele PC, consegue abrir. Sem senha mestra.
//! Chave colada em texto puro e criptografada sozinha na proxima leitura. Nenhuma funcao aqui imprime valor.
use crate::base64;
use std::collections::HashMap;
use std::path::Path;

pub const PREFIXO: &str = "dpapi:";
/// Amarra o segredo a este programa. E a mesma do Braniac em Python, para os dois lerem o mesmo arquivo.
const ENTROPIA: &[u8] = b"braniac-dono";
const NOMES_DE_SEGREDO: [&str; 7] = ["KEY", "TOKEN", "SECRET", "SENHA", "PASSWORD", "CHAVE", "USER_ID"];

#[cfg(windows)]
mod dpapi {
    use std::ffi::c_void;

    #[repr(C)]
    struct Blob {
        tamanho: u32,
        dados: *mut u8,
    }

    type Funcao = unsafe extern "system" fn(*const Blob, *const u16, *const Blob, *const c_void, *const c_void, u32, *mut Blob) -> i32;

    #[link(name = "crypt32")]
    extern "system" {
        fn CryptProtectData(entrada: *const Blob, descricao: *const u16, entropia: *const Blob, reservado: *const c_void, prompt: *const c_void, flags: u32, saida: *mut Blob) -> i32;
        fn CryptUnprotectData(entrada: *const Blob, descricao: *const u16, entropia: *const Blob, reservado: *const c_void, prompt: *const c_void, flags: u32, saida: *mut Blob) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn LocalFree(p: *mut c_void) -> *mut c_void;
    }

    fn chamar(f: Funcao, dados: &[u8], entropia: &[u8]) -> Result<Vec<u8>, String> {
        let entrada = Blob { tamanho: dados.len() as u32, dados: dados.as_ptr() as *mut u8 };
        let entropia = Blob { tamanho: entropia.len() as u32, dados: entropia.as_ptr() as *mut u8 };
        let mut saida = Blob { tamanho: 0, dados: std::ptr::null_mut() };
        // SAFETY: os blobs de entrada apontam para fatias vivas durante a chamada e a API so le deles;
        // 0x1 = sem janela (UI_FORBIDDEN). Em caso de sucesso o Windows aloca `saida.dados`, que copiamos e liberamos.
        unsafe {
            if f(&entrada, std::ptr::null(), &entropia, std::ptr::null(), std::ptr::null(), 0x1, &mut saida) == 0 || saida.dados.is_null() {
                return Err("o Windows nao abriu o segredo (outra conta ou outro computador?)".into());
            }
            let copia = std::slice::from_raw_parts(saida.dados, saida.tamanho as usize).to_vec();
            LocalFree(saida.dados as *mut c_void);
            Ok(copia)
        }
    }

    pub fn proteger(dados: &[u8], entropia: &[u8]) -> Result<Vec<u8>, String> {
        chamar(CryptProtectData, dados, entropia)
    }

    pub fn revelar(dados: &[u8], entropia: &[u8]) -> Result<Vec<u8>, String> {
        chamar(CryptUnprotectData, dados, entropia)
    }
}

#[cfg(not(windows))]
mod dpapi {
    pub fn proteger(_: &[u8], _: &[u8]) -> Result<Vec<u8>, String> {
        Err("criptografia de chaves so existe no Windows por enquanto".into())
    }

    pub fn revelar(_: &[u8], _: &[u8]) -> Result<Vec<u8>, String> {
        Err("criptografia de chaves so existe no Windows por enquanto".into())
    }
}

pub fn proteger(texto: &str) -> Result<String, String> {
    if texto.starts_with(PREFIXO) {
        return Ok(texto.to_string());
    }
    Ok(format!("{PREFIXO}{}", base64::codificar(&dpapi::proteger(texto.as_bytes(), ENTROPIA)?)))
}

pub fn revelar(valor: &str) -> Result<String, String> {
    match valor.strip_prefix(PREFIXO) {
        None => Ok(valor.to_string()),
        Some(b64) => String::from_utf8(dpapi::revelar(&base64::decodificar(b64)?, ENTROPIA)?).map_err(|_| "segredo ilegivel".to_string()),
    }
}

fn nome_de_segredo(nome: &str) -> bool {
    let n = nome.to_uppercase();
    NOMES_DE_SEGREDO.iter().any(|p| n.contains(p))
}

/// (nome, valor como esta no arquivo) de uma linha NOME=valor; comentario e linha sem "=" dao None.
fn linha(l: &str) -> Option<(String, String)> {
    let s = l.trim();
    if s.is_empty() || s.starts_with('#') {
        return None;
    }
    let (k, v) = s.split_once('=')?;
    Some((k.trim().to_string(), v.trim().trim_matches('"').trim_matches('\'').to_string()))
}

/// {nome: valor ABERTO}. Com `criptografar`, segredo em texto puro e trocado por dpapi: no arquivo
/// (so depois de conferir que abre de volta igual). Valor que nao abre fica de fora, sem derrubar o resto.
pub fn ler_env(caminho: &Path, criptografar: bool) -> HashMap<String, String> {
    let Ok(texto) = std::fs::read_to_string(caminho) else { return HashMap::new() };
    let (mut valores, mut novas, mut mudou) = (HashMap::new(), Vec::new(), false);
    for l in texto.trim_start_matches('\u{feff}').lines() {
        let Some((k, v)) = linha(l) else {
            novas.push(l.to_string());
            continue;
        };
        if let Ok(aberto) = revelar(&v) {
            valores.insert(k.clone(), aberto);
        }
        if criptografar && !v.is_empty() && !v.starts_with(PREFIXO) && nome_de_segredo(&k) {
            if let Ok(cifrado) = proteger(&v) {
                if revelar(&cifrado).as_deref() == Ok(v.as_str()) {
                    novas.push(format!("{k}={cifrado}"));
                    mudou = true;
                    continue;
                }
            }
        }
        novas.push(l.to_string());
    }
    if mudou {
        // grava ao lado e troca de uma vez: queda de energia no meio nao deixa o arquivo pela metade
        let tmp = caminho.with_extension("env.tmp");
        if std::fs::write(&tmp, novas.join("\n") + "\n").is_ok() {
            let _ = std::fs::rename(&tmp, caminho);
        }
    }
    valores
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linhas_do_env() {
        assert_eq!(linha(" GROQ_API_KEY = \"abc\" "), Some(("GROQ_API_KEY".into(), "abc".into())));
        assert_eq!(linha("A=b=c"), Some(("A".into(), "b=c".into())));
        assert!(linha("# comentario").is_none() && linha("sem igual").is_none() && linha("  ").is_none());
        assert!(nome_de_segredo("groq_api_key") && nome_de_segredo("TELEGRAM_USER_ID") && !nome_de_segredo("PORTA"));
        assert_eq!(revelar("texto puro").unwrap(), "texto puro");
        assert!(revelar("dpapi:@@@").is_err());
    }

    #[cfg(windows)]
    #[test]
    fn protege_abre_e_criptografa_o_arquivo_sozinho() {
        let cifrado = proteger("valor-de-teste-ção").unwrap();
        assert!(cifrado.starts_with(PREFIXO) && !cifrado.contains("valor-de-teste"));
        assert_eq!(revelar(&cifrado).unwrap(), "valor-de-teste-ção");
        assert_eq!(proteger(&cifrado).unwrap(), cifrado);

        let arq = std::env::temp_dir().join(format!("braniac-rs-env-{}.env", std::process::id()));
        std::fs::write(&arq, "# chaves\nX_API_KEY=abc123\nPORTA=8865\nY_TOKEN=\nRUIM_KEY=dpapi:AAAA\n").unwrap();
        // sem criptografar: le e nao mexe no arquivo
        assert_eq!(ler_env(&arq, false).get("X_API_KEY").map(String::as_str), Some("abc123"));
        assert!(std::fs::read_to_string(&arq).unwrap().contains("X_API_KEY=abc123"));
        // criptografando: o segredo some do arquivo, o resto fica igual, e a leitura continua dando o valor aberto
        let v = ler_env(&arq, true);
        let depois = std::fs::read_to_string(&arq).unwrap();
        assert!(depois.contains("X_API_KEY=dpapi:") && !depois.contains("abc123"));
        assert!(depois.contains("# chaves") && depois.contains("PORTA=8865") && depois.contains("Y_TOKEN=") && depois.contains("RUIM_KEY=dpapi:AAAA"));
        assert_eq!(v.get("X_API_KEY").map(String::as_str), Some("abc123"));
        assert_eq!(ler_env(&arq, true).get("X_API_KEY").map(String::as_str), Some("abc123"));
        assert_eq!(v.get("PORTA").map(String::as_str), Some("8865"));
        assert!(!v.contains_key("RUIM_KEY"));
        let _ = std::fs::remove_file(&arq);
    }
}
