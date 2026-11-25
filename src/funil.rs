//! Funil unico dos avisos AUTOMATICOS ao dono (porte de braniac/funil.py).
//! - CRITICO (crise, CVV/SAMU, familia precisando) sai na hora, sempre: nem silencio nem teto seguram.
//! - URGENTE (hospede, reserva, venda, ataque...) sai na hora de dia e ate o teto diario; fora disso vai ao resumo.
//! - O resto vai para o RESUMO das horas de `horas_resumo` (um texto com os detalhes + um audio curto).
//! - Repetido some: o mesmo aviso (sem contar numeros e links) nao sai 2x em 6 h; na fila, o mais novo substitui o antigo.
//! Config em dados/funil_config.json (ligado, horas_resumo, silencio [inicio, fim], teto_dia); estado em dados/funil_dono.json.
//! Ficou de fora: o bloco [resumo] do outbox (e do vigia.py) e a busca do nome do dono (entra como parametro).
//! O envio entra como closure que devolve texto comecando com "✅" quando deu certo (mesma convencao do Python).
#![allow(dead_code)]
use crate::tempo::Agora;
use crate::texto_pt::sem_acento;
use regex::Regex;
use serde_json::{json, Map, Value};
use std::path::Path;
use std::sync::{Mutex, MutexGuard, OnceLock};

const ARQ: &str = "funil_dono.json";
const CFG: &str = "funil_config.json";
pub const REPETE_S: f64 = 6.0 * 3600.0;
const MAX_FILA: usize = 40;
static LK: Mutex<()> = Mutex::new(());

pub struct Cfg {
    pub ligado: bool,
    pub horas_resumo: Vec<i64>,
    pub silencio: (i64, i64),
    pub teto_dia: i64,
}

/// Campo ausente ou torto cai no padrao (o Python quebraria): config ruim nunca desliga o silencio nem o teto.
pub fn cfg(dados: &Path) -> Cfg {
    let v: Value = std::fs::read_to_string(dados.join(CFG)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    Cfg {
        ligado: match &v["ligado"] {
            Value::Bool(b) => *b,
            Value::Number(n) => n.as_f64() != Some(0.0),
            _ => true,
        },
        horas_resumo: v["horas_resumo"].as_array().map(|a| a.iter().filter_map(Value::as_i64).collect()).unwrap_or_else(|| vec![9, 13, 19]),
        silencio: match (v["silencio"][0].as_i64(), v["silencio"][1].as_i64()) {
            (Some(i), Some(f)) => (i, f),
            _ => (22, 7),
        },
        teto_dia: v["teto_dia"].as_i64().unwrap_or(8),
    }
}

fn trava() -> MutexGuard<'static, ()> {
    LK.lock().unwrap_or_else(|e| e.into_inner())
}

fn ler(dados: &Path) -> Value {
    std::fs::read_to_string(dados.join(ARQ)).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}))
}

fn gravar(dados: &Path, est: &Value) {
    let _ = std::fs::create_dir_all(dados);
    let _ = std::fs::write(dados.join(ARQ), est.to_string());
}

fn corta(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn re(cel: &'static OnceLock<Regex>, padrao: &str) -> &'static Regex {
    cel.get_or_init(|| Regex::new(padrao).expect("regex fixa"))
}

pub fn critico(texto: &str) -> bool {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"(?i)\bcrise\b|\bcvv\b|\bsamu\b|socorro|suic|emerg[eê]ncia|falou algo importante|precis[ao] de voc[eê] agora").is_match(texto)
}

pub fn urgente(texto: &str) -> bool {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"(?i)urgent|h[oó]spede|reserva|check-?in|airbnb|quer fechar|fechou|\bvenda\b|\blead\b|invas|ataque|senha errada|login suspeito").is_match(texto)
}

/// O mesmo aviso com outro numero/link conta como repetido ("saldo era X, agora Y", "link novo: ...").
pub fn chave(texto: &str) -> String {
    static A: OnceLock<Regex> = OnceLock::new();
    static B: OnceLock<Regex> = OnceLock::new();
    let t = re(&A, r"https?://\S+|\d+([.,:]\d+)*").replace_all(&texto.to_lowercase(), "").into_owned();
    corta(&re(&B, r"\W+").replace_all(&t, ""), 80)
}

pub fn silencio(h: i64, c: &Cfg) -> bool {
    let (ini, fim) = c.silencio;
    if ini > fim { h >= ini || h < fim } else { ini <= h && h < fim }
}

/// Decide um aviso automatico: "agora" = manda ja; "fila" = vai para o resumo; "repetido" = descartado.
/// `ts` = segundos desde 1970 (fatos::agora_ts()); `agora` = hora local.
pub fn passar(dados: &Path, texto: &str, origem: &str, agora: &Agora, ts: f64) -> &'static str {
    let c = cfg(dados);
    if !c.ligado {
        return "agora";
    }
    let (hoje, k) = (agora.data.iso(), chave(texto));
    let _g = trava();
    let mut est = ler(dados);
    let mut env: Map<String, Value> = est["enviados"].as_object().into_iter().flatten()
        .filter(|(_, t)| t.as_f64().is_some_and(|t| ts - t < REPETE_S)).map(|(k, t)| (k.clone(), t.clone())).collect();
    if !k.is_empty() && env.contains_key(&k) {
        est["enviados"] = Value::Object(env);
        gravar(dados, &est);
        return "repetido";
    }
    let n = est["contagem"][hoje.as_str()].as_i64().unwrap_or(0);
    // critico vem primeiro e sozinho: silencio e teto so pesam no urgente
    if critico(texto) || (urgente(texto) && !silencio(agora.hora as i64, &c) && n < c.teto_dia) {
        env.insert(k, json!(ts));
        est["enviados"] = Value::Object(env);
        est["contagem"] = json!({ hoje: n + 1 });
        gravar(dados, &est);
        if critico(texto) {
            let _ = crate::app_painel::pedir("/"); // precisa do Dono: o aplicativo vem para a frente
        }
        return "agora";
    }
    let mut fila: Vec<Value> = est["fila"].as_array().cloned().unwrap_or_default();
    fila.retain(|x| x["k"].as_str() != Some(k.as_str()));
    fila.push(json!({ "k": k, "texto": corta(texto, 900), "origem": origem, "ts": ts }));
    let sobra = fila.len().saturating_sub(MAX_FILA);
    est["enviados"] = Value::Object(env);
    est["fila"] = Value::Array(fila.split_off(sobra));
    gravar(dados, &est);
    "fila"
}

/// Porte do `envolver`: o que vai ao dono passa pelo funil; para qualquer outra pessoa o envio passa direto.
#[allow(clippy::too_many_arguments)]
pub fn filtrar(dados: &Path, dono: &str, nome: &str, texto: &str, origem: &str, agora: &Agora, ts: f64, enviar: &dyn Fn(&str, &str) -> String) -> String {
    if sem_acento(nome) != sem_acento(dono) {
        return enviar(nome, texto);
    }
    match passar(dados, texto, origem, agora, ts) {
        "agora" => enviar(nome, texto),
        d => format!("✅ Aviso {d} (funil do dono)."),
    }
}

fn fala_de(x: &str) -> String {
    static A: OnceLock<Regex> = OnceLock::new();
    static B: OnceLock<Regex> = OnceLock::new();
    let t = re(&A, r"https?://\S+|[A-Za-z]:[\\/]\S+|[\x{1F000}-\x{1FFFF}\x{2600}-\x{27BF}]").replace_all(x, "");
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    match re(&B, r"^(.{20,160}?[.!?:])(\s|$)").captures(&t) {
        Some(m) => m[1].trim().to_string(),
        None => corta(&t, 160).trim().to_string(),
    }
}

/// (fala curta, texto com detalhes) do resumo. `tratamento` = como chamar o dono na fala ("senhor Fulano"); vazio = sem vocativo.
pub fn resumo(fila: &[Value], tratamento: &str) -> (String, String) {
    let n = fila.len();
    let texto_de = |x: &Value| x["texto"].as_str().unwrap_or("").to_string();
    let voc = if tratamento.is_empty() { String::new() } else { format!(", {tratamento}") };
    let mut fala = format!("Resumo do Jarvis{voc}: {n} aviso{}. ", if n > 1 { "s" } else { "" })
        + &fila.iter().take(6).enumerate().map(|(i, x)| format!("{}. {}", i + 1, fala_de(&texto_de(x)))).collect::<Vec<_>>().join(" ");
    if n > 6 {
        fala += &format!(" E mais {} no texto.", n - 6);
    }
    let texto = "📋 Resumo do Jarvis\n\n".to_string() + &fila.iter().map(|x| format!("• {}", texto_de(x))).collect::<Vec<_>>().join("\n\n");
    (corta(&fala, 1100), corta(&texto, 3500))
}

/// Na hora de resumo (fora do silencio), manda a fila acumulada: texto + audio curto. Devolve quantos avisos entregou.
/// Um resumo por hora; se o envio falhar, tenta de novo no proximo tick da mesma hora.
#[allow(clippy::too_many_arguments)]
pub fn tick(dados: &Path, agora: &Agora, ts: f64, nome: &str, tratamento: &str, enviar: &dyn Fn(&str, &str) -> String, audio: &dyn Fn(&str, &str) -> String) -> usize {
    let (c, h) = (cfg(dados), agora.hora as i64);
    if !c.horas_resumo.contains(&h) || silencio(h, &c) {
        return 0;
    }
    let marca = format!("{} {:02}", agora.data.iso(), agora.hora);
    let fila = {
        let _g = trava();
        let mut est = ler(dados);
        let fila = est["fila"].as_array().cloned().unwrap_or_default();
        if fila.is_empty() || est["ultimo_resumo"] == marca.as_str() {
            return 0;
        }
        est["ultimo_resumo"] = json!(marca);
        gravar(dados, &est);
        fila
    };
    let (fala, texto) = resumo(&fila, tratamento);
    // o envio fica fora da trava: pode demorar, e avisos novos continuam entrando na fila
    let ok = enviar(nome, &texto).starts_with('✅');
    if ok {
        let _ = audio(nome, &fala);
    }
    let _g = trava();
    let mut est = ler(dados);
    if !ok {
        if let Some(o) = est.as_object_mut() {
            o.remove("ultimo_resumo");
        }
        gravar(dados, &est);
        return 0;
    }
    // o que chegou na fila durante o envio fica para o proximo resumo
    let mandado = |x: &Value| fila.iter().any(|m| m["k"] == x["k"] && (m["ts"].as_f64().unwrap_or(0.0) - x["ts"].as_f64().unwrap_or(-1.0)).abs() < 1e-3);
    let resto: Vec<Value> = est["fila"].as_array().into_iter().flatten().filter(|x| !mandado(x)).cloned().collect();
    est["fila"] = Value::Array(resto);
    let mut env = est["enviados"].as_object().cloned().unwrap_or_default();
    for m in &fila {
        env.insert(m["k"].as_str().unwrap_or("").to_string(), json!(ts));
    }
    est["enviados"] = Value::Object(env);
    gravar(dados, &est);
    fila.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;
    use std::cell::RefCell;

    const TS: f64 = 1_800_000_000.0;

    fn pasta(nome: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("braniac-funil-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn hora(h: u32, min: u32) -> Agora {
        Agora { data: Data::civil(2026, 11, 16).unwrap(), hora: h, min, seg: 0 }
    }

    #[test]
    fn comum_vai_para_fila_e_repetido_substitui() {
        let p = pasta("fila");
        assert_eq!(passar(&p, "🌡 o notebook está esquentando: 98 °C", "rotina", &hora(10, 0), TS), "fila");
        assert_eq!(passar(&p, "🌡 o notebook está esquentando: 99 °C", "rotina", &hora(10, 0), TS + 1.0), "fila");
        let fila = ler(&p)["fila"].as_array().cloned().unwrap();
        assert!(fila.len() == 1 && fila[0]["texto"].as_str().unwrap().contains("99"));
        assert_eq!(chave("Saldo era 10,50, agora 9. Veja https://x.y/z?a=1"), chave("saldo era 7, agora 1.234,00. veja http://outro"));
        // a fila guarda so os 40 mais novos
        for i in 0..45 {
            passar(&p, &format!("aviso {}", "x".repeat(i + 1)), "rotina", &hora(10, 0), TS);
        }
        assert_eq!(ler(&p)["fila"].as_array().unwrap().len(), 40);
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn urgente_sai_na_hora_e_nao_repete_por_6_horas() {
        let p = pasta("urgente");
        let t = "Hóspede Ana com problema na chave";
        assert_eq!(passar(&p, t, "rotina", &hora(10, 0), TS), "agora");
        assert_eq!(passar(&p, t, "rotina", &hora(10, 0), TS + 60.0), "repetido");
        assert_eq!(passar(&p, t, "rotina", &hora(15, 59), TS + REPETE_S - 1.0), "repetido");
        assert_eq!(passar(&p, t, "rotina", &hora(16, 0), TS + REPETE_S), "agora");
        assert_eq!(ler(&p)["contagem"]["2026-11-16"], 2);
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn madrugada_e_teto_seguram_urgente_mas_nunca_critico() {
        let p = pasta("noite");
        assert_eq!(passar(&p, "Reserva nova no Airbnb", "rotina", &hora(2, 0), TS), "fila");
        assert_eq!(passar(&p, "Reserva de outro hóspede", "rotina", &hora(22, 0), TS), "fila");
        assert_eq!(passar(&p, "Reserva de mais um hóspede", "rotina", &hora(6, 59), TS), "fila");
        assert_eq!(passar(&p, "Reserva da manhã, hóspede novo", "rotina", &hora(7, 0), TS), "agora");
        assert_eq!(passar(&p, "Crise: ligar CVV 188", "rotina", &hora(2, 0), TS), "agora");
        std::fs::write(p.join(CFG), r#"{"teto_dia": 0}"#).unwrap();
        assert_eq!(passar(&p, "Lead quer fechar a landing page", "rotina", &hora(10, 0), TS), "fila");
        assert_eq!(passar(&p, "Socorro, a Bia precisa de você agora", "rotina", &hora(3, 0), TS), "agora"); // teto 0 + madrugada
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn teto_diario_conta_por_dia() {
        let p = pasta("teto");
        std::fs::write(p.join(CFG), r#"{"teto_dia": 2}"#).unwrap();
        for (i, r) in ["agora", "agora", "fila"].iter().enumerate() {
            assert_eq!(passar(&p, &format!("venda {}", "a".repeat(i + 1)), "rotina", &hora(10, 0), TS), *r);
        }
        let amanha = Agora { data: Data::civil(2026, 11, 17).unwrap(), hora: 10, min: 0, seg: 0 };
        assert_eq!(passar(&p, "venda de amanhã", "rotina", &amanha, TS + 86400.0), "agora");
        assert!(ler(&p)["contagem"].as_object().unwrap().len() == 1);
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn silencio_sem_virar_a_noite_e_funil_desligado() {
        let c = Cfg { ligado: true, horas_resumo: vec![], silencio: (12, 14), teto_dia: 8 };
        assert!(silencio(12, &c) && silencio(13, &c) && !silencio(14, &c) && !silencio(11, &c));
        let p = pasta("desligado");
        std::fs::write(p.join(CFG), r#"{"ligado": false}"#).unwrap();
        assert_eq!(passar(&p, "pauta do dia pronta", "rotina", &hora(3, 0), TS), "agora");
        assert!(!p.join(ARQ).exists());
        std::fs::write(p.join(CFG), "lixo").unwrap();
        let c = cfg(&p);
        assert!(c.ligado && c.horas_resumo == vec![9, 13, 19] && c.silencio == (22, 7) && c.teto_dia == 8);
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn resumo_so_na_hora_certa_uma_vez_e_limpa_fila() {
        let p = pasta("resumo");
        passar(&p, "Cofre: saldo diminuiu R$ 34,98", "rotina", &hora(10, 0), TS);
        passar(&p, "Candidaturas da manhã: 0 enviadas", "rotina", &hora(10, 0), TS);
        let (env, aud) = (RefCell::new(Vec::new()), RefCell::new(Vec::new()));
        let e = |n: &str, t: &str| {
            env.borrow_mut().push((n.to_string(), t.to_string()));
            "✅ ok".to_string()
        };
        let a = |_: &str, t: &str| {
            aud.borrow_mut().push(t.to_string());
            "✅ ok".to_string()
        };
        assert_eq!(tick(&p, &hora(10, 5), TS, "Dono", "senhor Dono", &e, &a), 0); // 10h nao e hora de resumo
        assert_eq!(tick(&p, &hora(13, 1), TS + 100.0, "Dono", "senhor Dono", &e, &a), 2);
        assert_eq!(tick(&p, &hora(13, 11), TS + 200.0, "Dono", "senhor Dono", &e, &a), 0); // um resumo por hora
        let env = env.borrow();
        assert!(env.len() == 1 && env[0].0 == "Dono" && env[0].1.contains("saldo") && env[0].1.contains("Candidaturas"));
        assert!(aud.borrow()[0].contains("2 avisos") && aud.borrow()[0].starts_with("Resumo do Jarvis, senhor Dono: "));
        assert!(ler(&p)["fila"].as_array().unwrap().is_empty());
        // o que saiu no resumo conta como enviado: nao volta a fila nas proximas 6 h
        assert_eq!(passar(&p, "Cofre: saldo diminuiu R$ 1,00", "rotina", &hora(13, 20), TS + 300.0), "repetido");
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn resumo_que_falha_tenta_de_novo_e_nao_sai_no_silencio() {
        let p = pasta("falha");
        passar(&p, "Fornalha: pauta do dia pronta", "rotina", &hora(10, 0), TS);
        let nada = |_: &str, _: &str| String::new();
        assert_eq!(tick(&p, &hora(13, 0), TS, "Dono", "", &|_, _| "erro".into(), &nada), 0);
        assert_eq!(ler(&p)["fila"].as_array().unwrap().len(), 1);
        assert_eq!(tick(&p, &hora(13, 5), TS, "Dono", "", &|_, _| "✅".into(), &nada), 1);
        // hora de resumo dentro do silencio: nada sai
        passar(&p, "NexoDev: ticket pronto", "rotina", &hora(21, 0), TS + 7.0 * 3600.0);
        std::fs::write(p.join(CFG), r#"{"horas_resumo": [23], "silencio": [22, 7]}"#).unwrap();
        assert_eq!(tick(&p, &hora(23, 0), TS + 9.0 * 3600.0, "Dono", "", &|_, _| "✅".into(), &nada), 0);
        let (fala, texto) = resumo(&[json!({"texto": "Vídeo parado em C:\\x\\y.mp4 veja https://a.b 🎬 agora. Resto"})], "");
        assert!(fala.starts_with("Resumo do Jarvis: 1 aviso. 1. Vídeo parado em veja agora.") && texto.starts_with("📋 Resumo do Jarvis\n\n• Vídeo"));
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn filtrar_so_segura_o_que_vai_ao_dono() {
        let p = pasta("filtrar");
        let env = RefCell::new(Vec::new());
        let e = |n: &str, _: &str| {
            env.borrow_mut().push(n.to_string());
            "✅ ok".to_string()
        };
        let r = filtrar(&p, "Dono", "dono", "Fornalha: pauta do dia pronta", "rotina", &hora(10, 0), TS, &e);
        assert!(r.starts_with('✅') && r.contains("fila") && env.borrow().is_empty());
        filtrar(&p, "Dono", "Bia", "Fornalha: pauta do dia pronta", "rotina", &hora(10, 0), TS, &e);
        filtrar(&p, "Dono", "Dono", "Crise: ligar para o SAMU", "rotina", &hora(3, 0), TS, &e);
        assert_eq!(*env.borrow(), vec!["Bia", "Dono"]);
        let _ = std::fs::remove_dir_all(&p);
    }
}
