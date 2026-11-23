//! Mudo do Jarvis (porte de braniac/mudo.py): mudo = nao FALA em voz alta; o texto continua aparecendo.
//! Opcoes: N minutos (expira sozinho) ou ate desmutar. Interruptor separado (padrao DESLIGADO): com o mudo ativo, tambem nao
//! mandar AUDIO no aplicativo de mensagem para o DONO (vai por escrito). Audio para cliente e hospede nunca e afetado aqui.
//! Estado em dados/mudo.json ({"ate": 0 | -1 | ts, "whatsapp_dono": bool}; no Python a chave leva o nome do dono).
//! Ficou de fora: cortar o som que esta tocando (aqui so a bandeira CORTAR, que quem toca o audio observa).
#![allow(dead_code)]
use crate::texto_pt::sem_acento;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// Levantada ao mutar: a fala em andamento deve parar na hora. Baixada ao desmutar.
pub static CORTAR: AtomicBool = AtomicBool::new(false);
static LK: Mutex<()> = Mutex::new(());
const SEMPRE: f64 = -1.0; // "ate" = -1: mudo ate desmutar

fn ler(dados: &Path) -> Value {
    std::fs::read_to_string(dados.join("mudo.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}))
}

fn mudar(dados: &Path, campo: &str, valor: Value) {
    let _g = LK.lock().unwrap_or_else(|e| e.into_inner());
    let mut d = ler(dados);
    d[campo] = valor;
    let _ = std::fs::create_dir_all(dados);
    let _ = std::fs::write(dados.join("mudo.json"), d.to_string());
}

fn ate(dados: &Path) -> f64 {
    ler(dados)["ate"].as_f64().unwrap_or(0.0)
}

/// True se o Jarvis esta mudo agora (o prazo em minutos expira sozinho). `ts` = segundos desde 1970.
pub fn ativo(dados: &Path, ts: f64) -> bool {
    let a = ate(dados);
    a == SEMPRE || (a > 0.0 && ts < a)
}

/// `minutos` None (ou 0): ate desmutar. Levanta CORTAR para a fala que estiver tocando parar.
pub fn mutar(dados: &Path, minutos: Option<f64>, ts: f64) -> Value {
    let m = minutos.filter(|m| m.is_finite() && *m != 0.0);
    mudar(dados, "ate", json!(m.map_or(SEMPRE, |m| ts + m * 60.0)));
    CORTAR.store(true, Ordering::SeqCst);
    estado(dados, ts)
}

pub fn desmutar(dados: &Path, ts: f64) -> Value {
    mudar(dados, "ate", json!(0));
    CORTAR.store(false, Ordering::SeqCst);
    estado(dados, ts)
}

/// Interruptor: com o mudo ativo, tambem nao manda AUDIO no aplicativo de mensagem para o dono (manda texto).
pub fn definir_whatsapp_dono(dados: &Path, ligado: bool, ts: f64) -> Value {
    mudar(dados, "whatsapp_dono", json!(ligado));
    estado(dados, ts)
}

/// Audio para o DONO deve virar texto agora? So se mudo ativo E o interruptor ligado.
pub fn bloqueia_audio_dono(dados: &Path, ts: f64) -> bool {
    ativo(dados, ts) && ler(dados)["whatsapp_dono"] == true
}

pub fn estado(dados: &Path, ts: f64) -> Value {
    let (a, mudo) = (ate(dados), ativo(dados, ts));
    let restante = (mudo && a != SEMPRE).then(|| (((a - ts) / 60.0 + 0.99) as i64).max(1));
    json!({ "mudo": mudo, "ate_desmutar": mudo && a == SEMPRE, "restante_min": restante, "whatsapp_dono": ler(dados)["whatsapp_dono"] == true })
}

fn norm(x: &str) -> String {
    let s: String = sem_acento(x).chars().map(|c| if c.is_ascii_lowercase() || c.is_ascii_digit() { c } else { ' ' }).collect();
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

const MUTAR: [&str; 11] = ["silencio", "cala a boca", "cala boca", "fica quieto", "fica calado", "mudo", "modo mudo", "muta", "mutar", "para de falar", "pare de falar"];
const DESMUTAR: [&str; 7] = ["desmuta", "desmutar", "tira do mudo", "sai do mudo", "pode falar", "pode voltar a falar", "volta a falar"];

/// "mutar" / "desmutar" / None. So frase CURTA que e so o comando ("Jarvis, silencio", "cala a boca", "mudo").
/// "pode falar" so desmuta se estiver mudo (`mudo_agora`); senao e conversa normal ("pode falar, sou todo ouvidos").
pub fn comando(texto: &str, mudo_agora: bool) -> Option<&'static str> {
    let n = norm(texto);
    if n.is_empty() || n.len() > 40 {
        return None;
    }
    // mesma forma da regex do Python: ^(jarvis )?(comando)( jarvis)?( por favor)?$
    let n = n.strip_suffix(" por favor").unwrap_or(&n);
    let n = n.strip_suffix(" jarvis").unwrap_or(n);
    let n = n.strip_prefix("jarvis ").unwrap_or(n);
    if MUTAR.contains(&n) {
        Some("mutar")
    } else if DESMUTAR.contains(&n) && mudo_agora {
        Some("desmutar")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TS: f64 = 1_800_000_000.0;

    fn pasta(nome: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("braniac-mudo-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    // um teste so para tudo que mexe na bandeira CORTAR (ela e global)
    #[test]
    fn mutar_desmutar_prazo_e_interruptor() {
        let p = pasta("estado");
        let e = estado(&p, TS);
        assert!(e["mudo"] == false && e["whatsapp_dono"] == false && e["restante_min"].is_null() && !bloqueia_audio_dono(&p, TS));
        // ate desmutar
        let e = mutar(&p, None, TS);
        assert!(ativo(&p, TS + 1e9) && e["ate_desmutar"] == true && e["restante_min"].is_null() && CORTAR.load(Ordering::SeqCst));
        desmutar(&p, TS);
        assert!(!ativo(&p, TS) && !CORTAR.load(Ordering::SeqCst));
        // por 10 minutos: expira sozinho
        let e = mutar(&p, Some(10.0), TS);
        assert!(e["mudo"] == true && e["ate_desmutar"] == false && e["restante_min"] == 10);
        assert_eq!(estado(&p, TS + 9.5 * 60.0)["restante_min"], 1);
        assert!(ativo(&p, TS + 599.0) && !ativo(&p, TS + 11.0 * 60.0) && estado(&p, TS + 660.0)["mudo"] == false);
        assert!(mutar(&p, Some(0.0), TS)["ate_desmutar"] == true); // 0 = ate desmutar
        desmutar(&p, TS);
        // o interruptor do audio ao dono so vale com o mudo ativo
        definir_whatsapp_dono(&p, true, TS);
        assert!(!bloqueia_audio_dono(&p, TS));
        mutar(&p, None, TS);
        assert!(bloqueia_audio_dono(&p, TS) && estado(&p, TS)["whatsapp_dono"] == true);
        definir_whatsapp_dono(&p, false, TS);
        assert!(!bloqueia_audio_dono(&p, TS) && ativo(&p, TS));
        desmutar(&p, TS);
        std::fs::write(p.join("mudo.json"), "[1, 2]").unwrap(); // arquivo torto = nao esta mudo
        assert!(!ativo(&p, TS));
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn comandos_de_voz() {
        for f in ["Jarvis, silêncio", "cala a boca", "Mudo.", "para de falar", "Jarvis, fica quieto, Jarvis, por favor", "silencio"] {
            assert_eq!(comando(f, false), Some("mutar"), "{f}");
        }
        assert_eq!(comando("pode falar", false), None); // sem estar mudo e conversa normal
        assert_eq!(comando("pode falar", true), Some("desmutar"));
        assert_eq!(comando("Jarvis, pode voltar a falar por favor", true), Some("desmutar"));
        for f in ["hoje o silencio da casa estava estranho, me conta uma historia", "pode falar, sou todo ouvidos", "silencio agora", "por favor silencio", "", "jarvis"] {
            assert_eq!(comando(f, true), None, "{f}");
        }
    }
}
