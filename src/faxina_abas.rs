//! Faxina de abas (braniac/faxina_abas.py): qual aba do navegador do Braniac fechar. Fecha a que NAO e essencial e esta ha mais de
//! PARADA_S na mesma URL. Listar e fechar abas entram como dado/closure; aqui fica so a regra.
//! Travas: nao fecha nada com a faxina desligada nem enquanto um audio esta sendo gravado (sairia cortado).
#![allow(dead_code)]
use std::collections::HashMap;

pub const MANTER: [&str; 8] = ["web.whatsapp.com", "airbnb.com", "web.telegram.org", "infinitepay.io", "127.0.0.1", "localhost", "chrome://", "devtools://"];
pub const PARADA_S: f64 = 3600.0;

/// id da aba -> (url, desde quando esta nessa url)
pub type Vistas = HashMap<String, (String, f64)>;

/// Recebe [(id, url)], atualiza `vistas` e devolve os ids a fechar (nao essenciais, parados ha `parada_s`).
pub fn escolher(abas: &[(String, String)], vistas: &mut Vistas, agora: f64, parada_s: f64) -> Vec<String> {
    let mut fechar = vec![];
    for (id, url) in abas {
        match vistas.get(id) {
            Some(ant) if ant.0 == *url => {
                if agora - ant.1 >= parada_s && !MANTER.iter().any(|m| url.contains(m)) {
                    fechar.push(id.clone());
                }
            }
            _ => {
                vistas.insert(id.clone(), (url.clone(), agora)); // aba nova ou navegou: zera o relogio
            }
        }
    }
    vistas.retain(|i, _| abas.iter().any(|a| a.0 == *i) && !fechar.contains(i));
    fechar
}

/// Devolve quantas abas mandou fechar. `ligada` = BRANIAC_FAXINA_ABAS != "0"; `gravando_audio` = trava do envio de voz.
pub fn tick(ligada: bool, gravando_audio: bool, abas: &[(String, String)], vistas: &mut Vistas, agora: f64, fechar_aba: &dyn Fn(&str)) -> usize {
    if !ligada || gravando_audio {
        return 0;
    }
    let fechar = escolher(abas, vistas, agora, PARADA_S);
    fechar.iter().for_each(|i| fechar_aba(i));
    fechar.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn abas() -> Vec<(String, String)> {
        [("w", "https://web.whatsapp.com/"), ("g", "https://gemini.google.com/app"), ("t", "https://www.tiktok.com/foryou")].iter().map(|a| (a.0.to_string(), a.1.to_string())).collect()
    }

    #[test]
    fn fecha_so_a_parada_e_nao_essencial() {
        let (mut v, mut a) = (Vistas::new(), abas());
        assert!(escolher(&a, &mut v, 0.0, PARADA_S).is_empty()); // primeira vez: so anota
        a[2].1 = "https://www.tiktok.com/upload".into(); // navegou: relogio zera
        assert!(escolher(&a, &mut v, 3000.0, PARADA_S).is_empty());
        assert_eq!(escolher(&a, &mut v, 3700.0, PARADA_S), ["g"]); // gemini parado ha mais de 1h; whatsapp nunca; tiktok ainda nao
        assert!(!v.contains_key("g") && v.contains_key("w"));
        a.remove(1);
        assert_eq!(escolher(&a, &mut v, 6700.0, PARADA_S), ["t"]);
    }

    #[test]
    fn desligada_ou_gravando_audio_nao_fecha_nada() {
        let fechadas = std::cell::RefCell::new(vec![]);
        let fechar = |i: &str| fechadas.borrow_mut().push(i.to_string());
        let mut v: Vistas = abas().into_iter().map(|a| (a.0, (a.1, 0.0))).collect();
        assert_eq!(tick(false, false, &abas(), &mut v, 9999.0, &fechar), 0);
        assert_eq!(tick(true, true, &abas(), &mut v, 9999.0, &fechar), 0);
        assert!(fechadas.borrow().is_empty() && v.len() == 3);
        assert_eq!(tick(true, false, &abas(), &mut v, 9999.0, &fechar), 2);
        assert_eq!(fechadas.borrow().len(), 2);
        assert!(!fechadas.borrow().contains(&"w".to_string()));
    }
}
