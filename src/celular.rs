//! Chamar o Jarvis pelo celular (braniac/celular.py): o app manda so os trechos com FALA; "<nome>, ta acordado?" acorda e o Jarvis
//! conversa por voz ate ouvir "pode descansar" (ou 3 min sem falar). A resposta volta em texto + audio (base64).
//! Dormindo, o que e dito NAO vai ao Jarvis. Transcrever, reconhecer acordar/descansar, responder e gerar a voz entram como closure
//! (`Voz`); nada do audio e guardado aqui.
#![allow(dead_code)]
use serde_json::{json, Value};

pub const IDLE_S: f64 = 180.0;

#[derive(Default, Debug)]
pub struct Estado {
    pub acordado: bool,
    pub ultima: f64,
}

pub struct Voz<'a> {
    /// WAV em base64 -> texto (transcricao local)
    pub transcrever: &'a dyn Fn(&str) -> String,
    pub quer_acordar: &'a dyn Fn(&str) -> bool,
    pub quer_descansar: &'a dyn Fn(&str) -> bool,
    /// o Jarvis responde a fala
    pub tratar: &'a dyn Fn(&str) -> String,
    /// texto -> WAV em base64; None = sem voz agora (o app mostra so o texto)
    pub falar: &'a dyn Fn(&str) -> Option<String>,
}

/// Um trecho de fala. Devolve {texto, estado, acordou?, resposta?, audio?}. `dono` = como o Jarvis chama quem fala.
pub fn fala(est: &mut Estado, wav_b64: &str, agora: f64, dono: &str, v: &Voz) -> Value {
    if est.acordado && agora - est.ultima > IDLE_S {
        est.acordado = false;
    }
    let texto = (v.transcrever)(wav_b64).trim().to_string();
    let audio = |r: &str| (v.falar)(&r.chars().take(600).collect::<String>()).unwrap_or_default();
    if texto.chars().count() < 2 {
        return json!({ "texto": "", "estado": if est.acordado { "acordado" } else { "dormindo" } });
    }
    if !est.acordado {
        if !(v.quer_acordar)(&texto) {
            return json!({ "texto": texto, "estado": "dormindo" });
        }
        *est = Estado { acordado: true, ultima: agora };
        let r = format!("Estou aqui, {dono}. Em que posso ajudar?");
        return json!({ "texto": texto, "estado": "acordado", "acordou": true, "resposta": r, "audio": audio(&r) });
    }
    est.ultima = agora;
    if (v.quer_descansar)(&texto) {
        est.acordado = false;
        let r = "Tudo bem, vou descansar. É só me chamar.";
        return json!({ "texto": texto, "estado": "dormindo", "resposta": r, "audio": audio(r) });
    }
    let resp = (v.tratar)(&texto);
    json!({ "texto": texto, "estado": "acordado", "resposta": resp, "audio": audio(&resp) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dormindo_nao_passa_nada_ao_jarvis_acorda_conversa_e_descansa() {
        let tratadas = std::cell::RefCell::new(vec![]);
        let v = Voz {
            transcrever: &|w| w.to_string(), // nos testes o "audio" ja e o texto
            quer_acordar: &|t| t.contains("acordado"),
            quer_descansar: &|t| t.contains("descansar"),
            tratar: &|t| {
                tratadas.borrow_mut().push(t.to_string());
                "x".repeat(700)
            },
            falar: &|t| Some(format!("wav:{}", t.chars().count())),
        };
        let mut e = Estado::default();
        assert_eq!(fala(&mut e, "apaga tudo do computador", 10.0, "Ana", &v), json!({"texto": "apaga tudo do computador", "estado": "dormindo"}));
        assert_eq!(fala(&mut e, " a ", 11.0, "Ana", &v), json!({"texto": "", "estado": "dormindo"}));
        assert!(tratadas.borrow().is_empty());
        let r = fala(&mut e, "Jarvis, tá acordado?", 20.0, "Ana", &v);
        assert_eq!(r, json!({"texto": "Jarvis, tá acordado?", "estado": "acordado", "acordou": true, "resposta": "Estou aqui, Ana. Em que posso ajudar?", "audio": "wav:37"}));
        assert!(tratadas.borrow().is_empty()); // a frase de acordar nao vira pedido
        let r = fala(&mut e, "que horas são", 30.0, "Ana", &v);
        assert_eq!((r["estado"].as_str(), r["audio"].as_str(), r["resposta"].as_str().map(str::len)), (Some("acordado"), Some("wav:600"), Some(700)));
        assert_eq!(fala(&mut e, "", 31.0, "Ana", &v), json!({"texto": "", "estado": "acordado"}));
        let r = fala(&mut e, "pode descansar", 40.0, "Ana", &v);
        assert_eq!((r["estado"].as_str(), r["resposta"].as_str()), (Some("dormindo"), Some("Tudo bem, vou descansar. É só me chamar.")));
        assert_eq!(fala(&mut e, "manda mensagem", 41.0, "Ana", &v)["estado"], "dormindo");
        assert_eq!(*tratadas.borrow(), ["que horas são"]);
    }

    #[test]
    fn tres_minutos_sem_falar_dorme_e_sem_voz_volta_so_o_texto() {
        let v = Voz { transcrever: &|w| w.to_string(), quer_acordar: &|t| t.contains("acordado"), quer_descansar: &|_| false, tratar: &|_| "ok".into(), falar: &|_| None };
        let mut e = Estado::default();
        assert_eq!(fala(&mut e, "tá acordado?", 0.0, "Ana", &v)["audio"], "");
        assert_eq!(fala(&mut e, "oi de novo", 170.0, "Ana", &v)["estado"], "acordado");
        assert_eq!(fala(&mut e, "ainda aí", 170.0 + IDLE_S + 1.0, "Ana", &v), json!({"texto": "ainda aí", "estado": "dormindo"}));
    }
}
