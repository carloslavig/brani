//! Vigia de voz (braniac/sentinela.py): com o Braniac FECHADO, escuta SO a frase de acordar. Ao ouvir: bip, sobe o Braniac e pede que ele
//! acorde (quem cumprimenta e o servidor). Com o Braniac no ar o vigia solta o microfone.
//! De fora (closures): subir processos, bip, pedido HTTP de acordar, microfone, checagem de porta. Aqui so as regras.
#![allow(dead_code)]
use crate::escuta;

pub const PORTA_TRAVA: u16 = 8767; // uma instancia so

/// A unica frase que o vigia atende (a mesma regra da escuta; lixo do Whisper nao conta).
pub fn frase_de_acordar(texto: &str) -> bool {
    !escuta::lixo(texto) && escuta::quer_acordar(texto)
}

/// Ouviu a frase: bip, sobe o Braniac e pede para acordar. Nao fala nada aqui. Uma subida por vez (`subindo`). Devolve o que registrar.
pub fn acordar(subindo: &mut bool, bip: &dyn Fn(), subir: &dyn Fn() -> Result<bool, String>, acordar_servidor: &dyn Fn() -> Result<(), String>) -> Option<String> {
    if *subindo {
        return None;
    }
    *subindo = true;
    bip();
    let r = match subir().and_then(|ok| if ok { acordar_servidor().map(|_| true) } else { Ok(false) }) {
        Ok(true) => "Braniac no ar e acordado".to_string(),
        Ok(false) => "o Braniac nao subiu a tempo".to_string(),
        Err(e) => format!("ERRO ao subir/acordar: {e}"),
    };
    *subindo = false;
    Some(r)
}

/// Guarda (um passo a cada 5 s): navegador do agente fora ha 12 passos (1 min) ou servidor fora ha 18 (1,5 min) = religar tudo.
#[derive(Default)]
pub struct Guarda {
    pub chrome_fora: u32,
    pub servidor_fora: u32,
}

impl Guarda {
    /// true = hora de religar tudo (e a contagem recomeca).
    pub fn passo(&mut self, chrome_aberto: bool, servidor_aberto: bool) -> bool {
        self.chrome_fora = if chrome_aberto { 0 } else { self.chrome_fora + 1 };
        self.servidor_fora = if servidor_aberto { 0 } else { self.servidor_fora + 1 };
        let religar = self.chrome_fora == 12 || self.servidor_fora == 18;
        if religar {
            *self = Guarda::default();
        }
        religar
    }
}

/// O que fazer com o microfone do vigia: Some("desligar") com o Braniac no ar, Some("ligar") com ele fechado, None se ja esta certo.
pub fn microfone(braniac_no_ar: bool, estado: &str) -> Option<&'static str> {
    match (braniac_no_ar, estado == "parada") {
        (true, false) => Some("desligar"),
        (false, true) => Some("ligar"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn so_a_frase_de_acordar_sobe_o_braniac_e_uma_vez_por_vez() {
        assert!(frase_de_acordar("Jarvis, tá acordado?") && frase_de_acordar("oi Jarvis"));
        for f in ["jarvis qual a capital da França", "tá acordado", "Legendas pela comunidade Jarvis acorda", "", "o jarvis é legal"] {
            assert!(!frase_de_acordar(f), "{f}");
        }
        let passos = RefCell::new(Vec::new());
        let bip = || passos.borrow_mut().push("bip");
        let pede = || {
            passos.borrow_mut().push("acordar");
            Ok(())
        };
        let mut subindo = false;
        assert_eq!(acordar(&mut subindo, &bip, &|| Ok(true), &pede).unwrap(), "Braniac no ar e acordado");
        assert!(*passos.borrow() == ["bip", "acordar"] && !subindo);
        assert!(acordar(&mut subindo, &bip, &|| Ok(false), &pede).unwrap().contains("nao subiu")); // nao subiu: nao pede para acordar
        assert!(acordar(&mut subindo, &bip, &|| Err("x".into()), &pede).unwrap().starts_with("ERRO") && !subindo);
        assert_eq!(passos.borrow().iter().filter(|p| **p == "acordar").count(), 1);
        subindo = true; // ja ha uma subida em curso: a segunda frase nao faz nada
        assert!(acordar(&mut subindo, &|| panic!("bip"), &|| panic!("subir"), &pede).is_none());
    }

    #[test]
    fn guarda_religa_depois_de_minutos_fora_e_o_microfone_segue_o_braniac() {
        let mut g = Guarda::default();
        assert!((0..11).all(|_| !g.passo(false, true)) && g.passo(false, true) && g.chrome_fora == 0);
        assert!((0..17).all(|_| !g.passo(true, false)) && g.passo(true, false));
        assert!(!g.passo(false, false) && !g.passo(true, true) && g.chrome_fora == 0); // voltou: zera
        assert_eq!((microfone(true, "dormindo"), microfone(false, "parada"), microfone(true, "parada"), microfone(false, "dormindo")), (Some("desligar"), Some("ligar"), None, None));
    }
}
