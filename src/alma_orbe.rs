//! Esfera da Alma (braniac/alma_orbe.py): so o calculo de nivel -> tamanho e lugar da janela, o canto guardado, o rotulo de cada estado e o
//! ritmo de consulta. A janela, o desenho do anel e do painel ficaram de fora (sao interface).
#![allow(dead_code)]
use serde_json::{json, Value};

pub const PORTA_TRAVA: u16 = 8769;

/// O que a esfera mostra sem ligacao com o Braniac.
pub fn offline() -> Value {
    json!({ "ativa": false, "nivel": 0, "estado": "offline", "legenda": "", "historico": [], "entrada": 0.0, "fala": 0.0 })
}

pub fn rotulo(estado: &str) -> &'static str {
    match estado {
        "ouvindo" => "ouvindo você",
        "pensando" => "pensando",
        "falando" => "falando",
        "interrompida" => "parei de falar",
        "offline" => "sem ligação com o Braniac",
        _ => "",
    }
}

/// Segundos entre consultas do estado: em repouso quase nao consulta.
pub fn intervalo(estado: &Value) -> f64 {
    if estado["ativa"] == true { 0.1 } else { 1.0 }
}

fn r(x: f64) -> i64 {
    x.round() as i64
}

/// Canto da esfera em repouso: o lugar guardado (se ainda cabe na tela) ou o padrao, embaixo a direita.
pub fn canto(pos: &Value, sw: i64, sh: i64, k: f64) -> (i64, i64) {
    match (pos["x"].as_f64(), pos["y"].as_f64()) {
        (Some(x), Some(y)) if (0.0..(sw - 20) as f64).contains(&x) && (0.0..(sh - 20) as f64).contains(&y) => (x as i64, y as i64),
        _ => (sw - r(96.0 * k), sh - r(150.0 * k)),
    }
}

/// (largura, altura, x, y) da janela. Nivel 0/1 = canexemplo (a legenda cresce para a ESQUERDA; a esfera fica onde estava); 2 = conversa,
/// embaixo a direita; 3 = atencao, no centro (grande, mas e so uma janela: nunca a tela toda).
pub fn geometria(nivel: i64, tem_legenda: bool, sw: i64, sh: i64, k: f64, pos: &Value) -> (i64, i64, i64, i64) {
    let lado = r(64.0 * k);
    let (w, h, x, y) = match nivel {
        ..=1 => {
            let w = if nivel == 1 && tem_legenda { r(360.0 * k) } else { lado };
            let (x, y) = canto(pos, sw, sh, k);
            (w, lado, x - (w - lado), y)
        }
        2 => {
            let (w, h) = (r(420.0 * k), r(540.0 * k));
            (w, h, sw - w - r(24.0 * k), sh - h - r(70.0 * k))
        }
        _ => {
            let (w, h) = (r(sw as f64 * 0.60), r(sh as f64 * 0.56));
            (w, h, (sw - w) / 2, (sh - h) / 2)
        }
    };
    (w, h, x.max(0), y.max(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nivel_decide_tamanho_e_lugar() {
        let n = Value::Null;
        assert_eq!(geometria(0, true, 1920, 1080, 1.0, &n), (64, 64, 1824, 930));
        assert_eq!(geometria(1, false, 1920, 1080, 1.0, &n), (64, 64, 1824, 930));
        assert_eq!(geometria(1, true, 1920, 1080, 1.0, &n), (360, 64, 1824 - 296, 930)); // cresce para a esquerda
        assert_eq!(geometria(2, true, 1920, 1080, 1.0, &n), (420, 540, 1476, 470));
        let (w, h, x, y) = geometria(3, true, 1920, 1080, 1.0, &n);
        assert!((w, h) == (1152, 605) && x == 384 && y == 237 && w < 1920 && h < 1080); // atencao nunca toma a tela toda
        assert_eq!(canto(&json!({ "x": 100, "y": 200 }), 1920, 1080, 1.0), (100, 200));
        assert_eq!(canto(&json!({ "x": 5000, "y": 200 }), 1920, 1080, 1.0), (1824, 930)); // guardado fora da tela: volta ao padrao
        assert_eq!(geometria(1, true, 1920, 1080, 1.0, &json!({ "x": 10, "y": 10 })).2, 0);
        assert!(rotulo("ouvindo") == "ouvindo você" && rotulo("idle").is_empty());
        assert_eq!((intervalo(&offline()), intervalo(&json!({ "ativa": true }))), (1.0, 0.1));
    }
}
