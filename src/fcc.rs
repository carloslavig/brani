//! fcc: ponte local (Anthropic Messages -> provedores gratis) feita em Rust na pasta fcc-rs/. Aqui so a regra de subir: se ja esta no ar,
//! nao sobe outra; sem o executavel compilado, nao sobe. Ficou de fora: testar a porta e abrir o processo (closures). As chaves do .env sao
//! abertas por quem chama, so na memoria, e entregues ao processo por variavel de ambiente: este modulo nunca ve nem grava chave.
#![allow(dead_code)]
use std::path::{Path, PathBuf};

pub const PORTA: u16 = 8791;

pub fn exe(raiz: &Path) -> PathBuf {
    raiz.join("fcc-rs").join("target").join("release").join("fcc.exe")
}

/// Sobe a ponte se ainda nao estiver no ar. `iniciar(exe, argumentos)` abre o processo sem janela. Devolve true se esta (ou vai estar) respondendo.
pub fn subir(raiz: &Path, porta: u16, no_ar: &dyn Fn(u16) -> bool, iniciar: &dyn Fn(&Path, &[String]) -> Result<(), String>) -> bool {
    if no_ar(porta) {
        return true;
    }
    let e = exe(raiz);
    e.exists() && iniciar(&e, &[raiz.to_string_lossy().into_owned(), porta.to_string()]).is_ok()
}

/// Variaveis para rodar o Claude Code pela ponte (o "token" e so a palavra local: a ponte fica em 127.0.0.1).
pub fn variaveis(porta: u16) -> [(&'static str, String); 2] {
    [("ANTHROPIC_BASE_URL", format!("http://127.0.0.1:{porta}")), ("ANTHROPIC_AUTH_TOKEN", "local".into())]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn so_sobe_uma_e_so_com_o_executavel() {
        let raiz = std::env::temp_dir().join(format!("braniac-fcc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&raiz);
        let abertos = RefCell::new(vec![]);
        let iniciar = |e: &Path, a: &[String]| {
            abertos.borrow_mut().push(format!("{} {}", e.file_name().unwrap().to_string_lossy(), a[1]));
            Ok(())
        };
        assert!(subir(&raiz, PORTA, &|_| true, &iniciar)); // ja no ar
        assert!(!subir(&raiz, PORTA, &|_| false, &iniciar)); // nao compilado
        assert!(abertos.borrow().is_empty());
        std::fs::create_dir_all(exe(&raiz).parent().unwrap()).unwrap();
        std::fs::write(exe(&raiz), "").unwrap();
        assert!(subir(&raiz, 9000, &|_| false, &iniciar));
        assert!(!subir(&raiz, 9000, &|_| false, &|_, _| Err("bloqueado".into())));
        assert_eq!(*abertos.borrow(), vec!["fcc.exe 9000"]);
        assert_eq!(variaveis(8791), [("ANTHROPIC_BASE_URL", "http://127.0.0.1:8791".to_string()), ("ANTHROPIC_AUTH_TOKEN", "local".to_string())]);
    }
}
