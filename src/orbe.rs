//! Orbe mini do Jarvis (orbe.py): SO o calculo do estado de cada quadro (o que a janelinha mostra a partir do /api/escuta-estado).
//! Ficou de fora: a janela (tkinter), o desenho do anel, arrastar/menu, a trava de uma instancia so e as chamadas ao servidor
//! (aqui so sai QUAL acao pedir).
#![allow(dead_code)]
use serde_json::Value;

pub const TAM_ICONE: f64 = 16.0; // icone de rede da bandeja a 100%
pub const EXTRA: f64 = 4.0; // o orbe fica 4 px maior

/// Lado da janela em pixels para a escala da tela (20 px a 100%, nunca menos de 16).
pub fn tamanho(escala: f64) -> u32 {
    (((TAM_ICONE + EXTRA) * escala).round() as u32).max(16)
}

pub fn mix(c1: (f64, f64, f64), c2: (f64, f64, f64), t: f64) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    let m = |a: f64, b: f64| (a + (b - a) * t) as u8;
    (m(c1.0, c2.0), m(c1.1, c2.1), m(c1.2, c2.2))
}

pub fn hex(c: (u8, u8, u8)) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

/// Sem resposta do servidor: parado.
pub fn dados_padrao() -> Value {
    serde_json::json!({ "estado": "parada", "atividade": "idle", "entrada": 0.0, "fala": 0.0 })
}

#[derive(Default, Debug, Clone, PartialEq)]
pub struct Vivo {
    /// energia suavizada (voz dele ouvindo, voz do Jarvis falando)
    pub e: f64,
    pub acordado: f64,
    pub fase: f64,
}

#[derive(Debug, PartialEq)]
pub struct Quadro {
    /// offline | ouvindo | pensando | falando | idle
    pub est: &'static str,
    pub e: f64,
    pub fase: f64,
    pub forca: f64,
    pub brilho: f64,
    /// milissegundos ate o proximo quadro (dormindo: 4 quadros/s, para nao gastar processador)
    pub proximo_ms: u32,
}

/// Um quadro: sobe rapido com a voz e desce devagar; acordado gira mais depressa. `agora` e `dt` em segundos (dt ate 0,1).
pub fn passo(v: &mut Vivo, d: &Value, agora: f64, dt: f64) -> Quadro {
    let dt = dt.clamp(0.0, 0.1);
    let ativ = d["atividade"].as_str().unwrap_or("idle");
    let reativo = match ativ {
        "ouvindo" => d["entrada"].as_f64().unwrap_or(0.0),
        "falando" => d["fala"].as_f64().unwrap_or(0.0),
        "pensando" => 0.25 + 0.2 * (agora * 7.0).sin(),
        _ => 0.0,
    };
    let (parada, acordado) = (d["estado"] == "parada", if d["estado"] == "acordado" { 1.0 } else { 0.0 });
    v.e += (reativo - v.e) * (dt * if reativo > v.e { 14.0 } else { 6.0 }).min(1.0);
    v.acordado += (acordado - v.acordado) * (dt * 3.0).min(1.0);
    let respira = 0.5 + 0.5 * (agora * if v.acordado > 0.5 { 1.8 } else { 0.9 }).sin();
    v.fase += dt * (0.6 + 2.2 * v.acordado + 4.0 * v.e);
    let est = if parada { "offline" } else { ["ouvindo", "pensando", "falando"].into_iter().find(|x| *x == ativ).unwrap_or("idle") };
    Quadro { est, e: v.e, fase: v.fase, forca: 0.75 + 0.6 * v.acordado + 0.15 * respira, brilho: if parada { 0.15 } else { 0.30 + 0.30 * v.acordado + 0.40 * v.e + 0.08 * respira }, proximo_ms: if ativ != "idle" || v.acordado > 0.5 { 50 } else { 250 } }
}

/// Menu "Acordar / Descansar": a acao a pedir em /api/escuta-acao.
pub fn acao_alternar(d: &Value) -> &'static str {
    if d["estado"] == "acordado" { "descansar" } else { "acordar" }
}

/// Menu "Desligar / ligar microfone".
pub fn acao_microfone(d: &Value) -> &'static str {
    if d["estado"] == "parada" { "ligar" } else { "desligar" }
}

/// A posicao salva vale se ainda cabe na tela; senao, o canto de baixo a direita (perto da bandeja).
pub fn posicao(salva: Option<(i64, i64)>, tela: (i64, i64), escala: f64) -> (i64, i64) {
    match salva {
        Some((x, y)) if (0..tela.0 - 4).contains(&x) && (0..tela.1 - 4).contains(&y) => (x, y),
        _ => (tela.0 - (310.0 * escala).round() as i64, tela.1 - (34.0 * escala).round() as i64),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn estado_do_quadro() {
        let mut v = Vivo::default();
        let q = passo(&mut v, &dados_padrao(), 0.0, 0.05);
        assert_eq!((q.est, q.brilho, q.proximo_ms, q.e), ("offline", 0.15, 250, 0.0));
        let ouvindo = json!({"estado": "acordado", "atividade": "ouvindo", "entrada": 1.0});
        let q = passo(&mut v, &ouvindo, 1.0, 5.0); // dt e limitado a 0,1
        assert_eq!((q.est, q.proximo_ms), ("ouvindo", 50));
        assert!((v.e - 1.0).abs() < 1e-9 && (v.acordado - 0.3).abs() < 1e-9); // sobe de uma vez com a voz; acorda aos poucos
        let fase = v.fase;
        let q = passo(&mut v, &json!({"estado": "acordado", "atividade": "idle"}), 1.1, 0.1);
        assert!((v.e - 0.4).abs() < 1e-9 && v.fase > fase && q.est == "idle"); // desce devagar
        assert_eq!(passo(&mut v, &json!({"estado": "dormindo", "atividade": "dancando"}), 2.0, 0.1).est, "idle");
        let q = passo(&mut Vivo::default(), &json!({"estado": "dormindo", "atividade": "pensando"}), 0.0, 0.1);
        assert!(q.est == "pensando" && q.e > 0.0 && (0.0..=1.05).contains(&q.forca));
        let q = passo(&mut Vivo::default(), &json!({"estado": "acordado", "atividade": "falando", "fala": 0.5}), 0.0, 0.01);
        assert!((q.e - 0.07).abs() < 1e-9);
    }

    #[test]
    fn tamanho_cores_acoes_e_posicao() {
        assert_eq!((tamanho(1.0), tamanho(1.5), tamanho(0.5)), (20, 30, 16));
        assert_eq!(hex(mix((125.0, 82.0, 14.0), (255.0, 232.0, 140.0), 2.0)), "#ffe88c");
        assert_eq!(hex(mix((0.0, 0.0, 0.0), (255.0, 255.0, 255.0), 0.5)), "#7f7f7f");
        assert_eq!((acao_alternar(&json!({"estado": "acordado"})), acao_alternar(&json!({"estado": "dormindo"}))), ("descansar", "acordar"));
        assert_eq!((acao_microfone(&json!({"estado": "parada"})), acao_microfone(&json!({"estado": "dormindo"}))), ("ligar", "desligar"));
        assert_eq!(posicao(Some((100, 200)), (1920, 1080), 1.0), (100, 200));
        assert_eq!(posicao(Some((5000, 200)), (1920, 1080), 1.0), (1610, 1046)); // monitor que nao existe mais
        assert_eq!(posicao(None, (1920, 1080), 1.5), (1455, 1029));
    }
}
