//! Vozes por agente (braniac/vozes_agentes.py): cada agente tem a voz PRIMARIA (formato antigo: motor/genero/vozes) e ate 2 RESERVAS
//! (secundaria e terciaria) em dados/voz_escolhida.json; o Jarvis fica no topo do arquivo, os outros em "agentes". Ajustes (velocidade, tom,
//! calor, estilo) com faixa. `falar` e a regra de queda: primaria -> reservas na ordem, com o motivo registrado em dados/voz.log.
//! De fora: os motores de voz (closure `gerar`). O `voz.rs` que ja existe usa outro arquivo (voz.json: gemini ou voz do Windows) e nao conhece
//! reservas: quando `falar` devolve Err, quem liga cai na "ordem de sempre" (`voz::falar_wav`).
#![allow(dead_code)]
use crate::tempo::Agora;
use serde_json::{json, Map, Value};
use std::io::Write;
use std::path::Path;

const ARQ: &str = "voz_escolhida.json";
pub const MOTORES: [&str; 4] = ["kokoro", "piper", "edge", "gemini"];
const ONLINE: [&str; 2] = ["edge", "gemini"];

fn padrao() -> Vec<Value> {
    vec![json!({ "id": "jarvis", "nome": "Jarvis" }), json!({ "id": "alma", "nome": "Alma", "so_genero": "F", "combinacoes": true }),
        json!({ "id": "zelo", "nome": "Zelo (fala como Jarvis)" }), json!({ "id": "jorge", "nome": "Jorge (Airbnb)" })]
}

fn ler_json(arq: &Path) -> Value {
    std::fs::read_to_string(arq).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}))
}

/// Lista de agentes com voz. dados/vozes_agentes.json pode renomear ou acrescentar.
pub fn agentes(dados: &Path) -> Vec<Value> {
    let mut lista = padrao();
    for a in ler_json(&dados.join("vozes_agentes.json"))["agentes"].as_array().into_iter().flatten() {
        let Some(id) = a["id"].as_str().filter(|i| !i.is_empty()) else { continue };
        let mut novo = lista.iter().find(|x| x["id"] == id).cloned().unwrap_or_else(|| json!({}));
        for k in ["so_genero", "combinacoes"] {
            if !a[k].is_null() {
                novo[k] = a[k].clone();
            }
        }
        novo["id"] = json!(id);
        novo["nome"] = json!(a["nome"].as_str().filter(|n| !n.is_empty()).unwrap_or(id));
        match lista.iter_mut().find(|x| x["id"] == id) {
            Some(x) => *x = novo,
            None => lista.push(novo),
        }
    }
    lista
}

/// "F" / "M" se o agente so pode usar voz de um genero.
pub fn so_genero(dados: &Path, agente: &str) -> Option<String> {
    agentes(dados).iter().find(|a| a["id"] == agente).and_then(|a| a["so_genero"].as_str().map(String::from))
}

pub fn escolha(dados: &Path, agente: &str) -> Value {
    let d = ler_json(&dados.join(ARQ));
    if agente.is_empty() || agente == "jarvis" { d } else { d["agentes"].get(agente).cloned().unwrap_or_else(|| json!({})) }
}

pub fn todas(dados: &Path) -> Value {
    Value::Object(agentes(dados).iter().filter_map(|a| a["id"].as_str()).map(|id| (id.to_string(), escolha(dados, id))).collect())
}

fn valido(dados: &Path, agente: &str) -> Result<(), String> {
    if agentes(dados).iter().any(|a| a["id"] == agente) { Ok(()) } else { Err("agente desconhecido".into()) }
}

/// Le o arquivo, entrega o pedaco do agente para mudar e grava.
fn mudar(dados: &Path, agente: &str, f: impl FnOnce(&mut Map<String, Value>)) -> Value {
    let mut d = ler_json(&dados.join(ARQ));
    {
        let alvo = if agente == "jarvis" {
            &mut d
        } else {
            if !d["agentes"].is_object() {
                d["agentes"] = json!({});
            }
            if !d["agentes"][agente].is_object() {
                d["agentes"][agente] = json!({});
            }
            &mut d["agentes"][agente]
        };
        if let Some(o) = alvo.as_object_mut() {
            f(o);
        }
    }
    let _ = std::fs::create_dir_all(dados);
    let _ = std::fs::write(dados.join(ARQ), serde_json::to_string_pretty(&d).unwrap_or_default());
    escolha(dados, agente)
}

fn reservas_de(alvo: &Value, com_voz: bool) -> Vec<Value> {
    let tem = |r: &Value, k: &str| r[k].as_str().is_some_and(|x| !x.is_empty());
    alvo["reservas"].as_array().into_iter().flatten().filter(|r| r.is_object() && tem(r, "motor") && (!com_voz || tem(r, "voz"))).take(2).cloned().collect()
}

/// Grava a voz do agente (a validacao do motor/voz e do catalogo). posicao 1 = PRIMARIA; 2 e 3 = SECUNDARIA e TERCIARIA.
pub fn escolher(dados: &Path, agente: &str, motor: &str, voz_id: &str, genero: &str, posicao: i64) -> Result<Value, String> {
    valido(dados, agente)?;
    if !(1..=3).contains(&posicao) {
        return Err("posicao deve ser 1, 2 ou 3".into());
    }
    Ok(mudar(dados, agente, |alvo| {
        if posicao == 1 {
            let mut vozes = alvo.get("vozes").and_then(Value::as_object).cloned().unwrap_or_default();
            vozes.insert(motor.to_string(), json!(voz_id));
            alvo.insert("motor".into(), json!(motor));
            alvo.insert("genero".into(), json!(genero));
            alvo.insert("vozes".into(), Value::Object(vozes));
        } else {
            let mut res = reservas_de(&Value::Object(alvo.clone()), false);
            let (item, i) = (json!({ "motor": motor, "voz": voz_id, "genero": genero }), (posicao - 2) as usize);
            if i < res.len() {
                res[i] = item;
            } else {
                res.push(item); // terciaria sem secundaria vira a secundaria
            }
            alvo.insert("reservas".into(), json!(res));
        }
    }))
}

/// Vozes de reserva do agente, na ordem (secundaria, terciaria).
pub fn reservas(dados: &Path, agente: &str) -> Vec<Value> {
    reservas_de(&escolha(dados, agente), true)
}

/// Tira a secundaria (2) ou a terciaria (3); a terciaria sobe se a secundaria sair.
pub fn tirar_reserva(dados: &Path, agente: &str, posicao: i64) -> Result<Value, String> {
    valido(dados, agente)?;
    if agente != "jarvis" && !ler_json(&dados.join(ARQ))["agentes"][agente].is_object() {
        return Ok(json!({}));
    }
    Ok(mudar(dados, agente, |alvo| {
        let mut res = alvo.get("reservas").and_then(Value::as_array).cloned().unwrap_or_default();
        if posicao >= 2 && ((posicao - 2) as usize) < res.len() {
            res.remove((posicao - 2) as usize);
        }
        if res.is_empty() {
            alvo.remove("reservas");
        } else {
            alvo.insert("reservas".into(), json!(res));
        }
    }))
}

/// Volta o agente para a ordem de sempre.
pub fn limpar(dados: &Path, agente: &str) -> Result<(), String> {
    valido(dados, agente)?;
    let mut d = ler_json(&dados.join(ARQ));
    if agente == "jarvis" {
        for k in ["motor", "genero", "vozes", "ajustes", "reservas"] {
            d.as_object_mut().map(|o| o.remove(k));
        }
    } else if let Some(a) = d["agentes"].as_object_mut() {
        a.remove(agente);
    }
    std::fs::write(dados.join(ARQ), serde_json::to_string_pretty(&d).unwrap_or_default()).map_err(|e| e.to_string())
}

/// Pontos de partida; o dono ajusta os controles depois. "Sedutora" e sensualidade em volume normal: nunca sussurro.
pub fn presets() -> Value {
    json!({
        "natural": { "nome": "Natural", "ajustes": {} },
        "acolhedora": { "nome": "Acolhedora", "ajustes": { "estilo": "Fale em português do Brasil com voz suave, calorosa e acolhedora, em volume normal, sem sussurrar, ritmo calmo, como quem cuida de alguém querido",
            "velocidade": 0.95, "tom": 0, "calor": 40 } },
        "sedutora": { "nome": "Sedutora", "ajustes": { "estilo": "Fale em português do Brasil com voz grave, quente e envolvente, em volume normal e bem projetada, sem sussurrar, em ritmo lento e seguro, com sensualidade e confiança, com um sorriso discreto na voz",
            "velocidade": 0.92, "tom": -1, "calor": 55 } },
    })
}

/// Grava os ajustes de voz do agente (velocidade, tom, calor, estilo). Valores fora da faixa sao cortados.
pub fn ajustar(dados: &Path, agente: &str, ajustes: &Value) -> Result<Value, String> {
    valido(dados, agente)?;
    let mut a = Map::new();
    let faixa = |k: &str, min: f64, max: f64, casas: f64| ajustes[k].as_f64().map(|v| (v.clamp(min, max) * casas).round() / casas);
    if let Some(v) = faixa("velocidade", 0.7, 1.3, 100.0) {
        a.insert("velocidade".into(), json!(v));
    }
    if let Some(v) = faixa("tom", -6.0, 3.0, 10.0) {
        a.insert("tom".into(), json!(v));
    }
    if let Some(v) = faixa("calor", 0.0, 100.0, 1.0) {
        a.insert("calor".into(), json!(v as i64));
    }
    if let Some(e) = ajustes["estilo"].as_str().map(str::trim).filter(|e| !e.is_empty()) {
        a.insert("estilo".into(), json!(e.chars().take(300).collect::<String>()));
    }
    Ok(mudar(dados, agente, |alvo| {
        if a.is_empty() {
            alvo.remove("ajustes");
        } else {
            alvo.insert("ajustes".into(), Value::Object(a));
        }
    }))
}

fn registrar(dados: &Path, agora: &Agora, agente: &str, linha: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dados.join("voz.log")) {
        let _ = writeln!(f, "{} {}:{:02} | {} | {linha}", agora.data.iso(), agora.hm(), agora.seg, if agente.is_empty() { "jarvis" } else { agente });
    }
}

/// WAV so com o cabecalho (~0,02 s de som) conta como falha.
fn cheio(wav: Vec<u8>) -> Result<Vec<u8>, String> {
    if wav.is_empty() || (wav.starts_with(b"RIFF") && wav.len() < 1000) { Err("audio vazio".into()) } else { Ok(wav) }
}

fn curto(e: &str, n: usize) -> String {
    e.chars().take(n).collect()
}

pub struct Fala {
    pub wav: Vec<u8>,
    /// "motor:voz" que falou.
    pub motor: String,
    /// Genero da RESERVA que falou (None = falou a primaria).
    pub genero_reserva: Option<String>,
}

/// Fala com a voz escolhida para o agente: a primaria; se ela cair (cota, fora do ar, erro, audio vazio), a secundaria e depois a terciaria.
/// Motor online (edge, gemini) nao entra se `permitir_nuvem` e falso (texto que nao pode sair do PC). Agente de um genero so nao usa reserva
/// de outro. Err(motivo) = sem escolha ou tudo caiu: quem liga usa a ordem de sempre. `gerar(motor, voz, texto)`.
pub fn falar(dados: &Path, agora: &Agora, agente: &str, texto: &str, permitir_nuvem: bool, gerar: &dyn Fn(&str, &str, &str) -> Result<Vec<u8>, String>) -> Result<Fala, String> {
    let e = escolha(dados, agente);
    let Some(pref) = e["motor"].as_str().filter(|m| !m.is_empty()) else { return Err("sem voz escolhida".into()) };
    let pode = |m: &str| MOTORES.contains(&m) && (permitir_nuvem || !ONLINE.contains(&m));
    let mut motivo = "texto sensivel (fica local)".to_string();
    if pode(pref) {
        let v = e["vozes"][pref].as_str().unwrap_or("");
        match gerar(pref, v, texto).and_then(cheio) {
            Ok(wav) => return Ok(Fala { wav, motor: format!("{pref}:{v}"), genero_reserva: None }),
            Err(err) => motivo = format!("{pref}: {}", curto(&err, 80)),
        }
    }
    let so = so_genero(dados, agente);
    for r in reservas_de(&e, true) {
        let (m, v) = (r["motor"].as_str().unwrap_or(""), r["voz"].as_str().unwrap_or(""));
        if !pode(m) || so.as_deref().is_some_and(|g| r["genero"] != g) {
            continue;
        }
        match gerar(m, v, texto).and_then(cheio) {
            Ok(wav) => {
                registrar(dados, agora, agente, &format!("RESERVA {m}:{v} | caiu: {motivo}"));
                return Ok(Fala { wav, motor: format!("{m}:{v}"), genero_reserva: r["genero"].as_str().map(String::from) });
            }
            Err(err) => motivo += &format!("; reserva {m}:{v}: {}", curto(&err, 60)),
        }
    }
    registrar(dados, agora, agente, &format!("PRIMARIA E RESERVAS caíram, ordem de sempre | {motivo}"));
    Err(motivo)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;
    use std::cell::RefCell;
    use std::path::PathBuf;

    fn dir(nome: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("braniac-vozes_agentes-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn agora() -> Agora {
        Agora { data: Data::civil(2026, 11, 16).unwrap(), hora: 9, min: 5, seg: 7 }
    }

    #[test]
    fn primaria_cai_por_cota_e_a_secundaria_fala_com_registro() {
        let d = dir("cota");
        escolher(&d, "alma", "gemini", "Kore", "F", 1).unwrap();
        escolher(&d, "alma", "edge", "Francisca", "F", 2).unwrap();
        escolher(&d, "alma", "kokoro", "pf_dora", "F", 3).unwrap();
        assert_eq!(reservas(&d, "alma").iter().map(|r| r["motor"].as_str().unwrap()).collect::<Vec<_>>(), ["edge", "kokoro"]);
        assert!(escolha(&d, "alma")["motor"] == "gemini" && escolha(&d, "alma")["vozes"]["gemini"] == "Kore"); // primaria no formato antigo
        assert!(escolha(&d, "jarvis")["motor"].is_null()); // a escolha da Alma nao mexe na do Jarvis
        let usadas = RefCell::new(Vec::new());
        let gerar = |m: &str, v: &str, _: &str| {
            usadas.borrow_mut().push(format!("{m}:{v}"));
            if m == "gemini" { Err("429 cota esgotada".to_string()) } else { Ok(vec![7u8; 2000]) }
        };
        let f = falar(&d, &agora(), "alma", "Oi, tudo certo?", true, &gerar).unwrap();
        assert!(f.motor == "edge:Francisca" && f.genero_reserva.as_deref() == Some("F") && *usadas.borrow() == ["gemini:Kore", "edge:Francisca"]);
        let log = std::fs::read_to_string(d.join("voz.log")).unwrap();
        assert!(log.starts_with("2026-11-16 09:05:07 | alma | RESERVA edge:Francisca | caiu: gemini: 429 cota"));
        // texto que nao pode sair do PC: nem a primaria nem a reserva online entram; fala a local
        usadas.borrow_mut().clear();
        assert_eq!(falar(&d, &agora(), "alma", "segredo", false, &gerar).unwrap().motor, "kokoro:pf_dora");
        assert_eq!(*usadas.borrow(), ["kokoro:pf_dora"]);
        // tudo caiu: erro com o motivo de cada um, registrado
        let e = falar(&d, &agora(), "alma", "oi", true, &|_, _, _| Err("fora do ar".into())).err().unwrap();
        assert!(e.contains("gemini: fora do ar") && e.contains("reserva kokoro:pf_dora") && std::fs::read_to_string(d.join("voz.log")).unwrap().contains("PRIMARIA E RESERVAS"));
        assert_eq!(falar(&d, &agora(), "zelo", "oi", true, &gerar).err().unwrap(), "sem voz escolhida");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn audio_vazio_passa_para_a_terciaria_e_genero_errado_nao_fala() {
        let d = dir("vazio");
        escolher(&d, "jarvis", "gemini", "Charon", "M", 1).unwrap();
        escolher(&d, "jarvis", "edge", "Antonio", "M", 2).unwrap();
        escolher(&d, "jarvis", "kokoro", "pm_alex", "M", 3).unwrap();
        let gerar = |m: &str, v: &str, _: &str| match m {
            "gemini" => Err("fora do ar".to_string()),
            "edge" => Ok([b"RIFF".as_slice(), &[0u8; 40]].concat()), // so cabecalho = vazio
            _ => Ok(format!("TERCIARIA:{v}{}", " ".repeat(1000)).into_bytes()),
        };
        assert!(falar(&d, &agora(), "jarvis", "Bom dia.", true, &gerar).unwrap().wav.starts_with(b"TERCIARIA:pm_alex"));
        // reserva masculina gravada a mao no arquivo de um agente so-feminino: e pulada
        escolher(&d, "alma", "gemini", "Kore", "F", 1).unwrap();
        escolher(&d, "alma", "kokoro", "pm_alex", "M", 2).unwrap();
        assert!(falar(&d, &agora(), "alma", "oi", true, &gerar).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn reservas_sobem_limpar_ajustes_e_agentes() {
        let d = dir("reservas");
        escolher(&d, "alma", "edge", "Francisca", "F", 3).unwrap(); // terciaria sem secundaria vira secundaria
        escolher(&d, "alma", "kokoro", "pf_dora", "F", 3).unwrap();
        let vozes = |ag: &str| reservas(&d, ag).iter().map(|r| r["voz"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        assert_eq!(vozes("alma"), ["Francisca", "pf_dora"]);
        escolher(&d, "alma", "piper", "faber/medium", "M", 2).unwrap(); // troca a secundaria no lugar
        assert_eq!(vozes("alma"), ["faber/medium", "pf_dora"]);
        tirar_reserva(&d, "alma", 2).unwrap();
        assert_eq!(vozes("alma"), ["pf_dora"]); // a terciaria sobe
        tirar_reserva(&d, "alma", 2).unwrap();
        assert!(escolha(&d, "alma").get("reservas").is_none());
        assert!(escolher(&d, "ninguem", "edge", "x", "F", 1).is_err() && escolher(&d, "alma", "edge", "x", "F", 4).is_err() && tirar_reserva(&d, "x", 2).is_err());
        escolher(&d, "jarvis", "edge", "Antonio", "M", 1).unwrap();
        escolher(&d, "jarvis", "kokoro", "pm_alex", "M", 2).unwrap();
        limpar(&d, "jarvis").unwrap();
        assert!(reservas(&d, "jarvis").is_empty() && escolha(&d, "jarvis")["motor"].is_null() && !vozes("alma").contains(&"x".to_string()));
        let a = ajustar(&d, "alma", &json!({ "velocidade": 9, "tom": -40, "calor": 55.4, "estilo": "x".repeat(400) })).unwrap();
        assert!(a["ajustes"]["velocidade"] == 1.3 && a["ajustes"]["tom"] == -6.0 && a["ajustes"]["calor"] == 55 && a["ajustes"]["estilo"].as_str().unwrap().len() == 300);
        assert!(ajustar(&d, "alma", &json!({})).unwrap().get("ajustes").is_none());
        limpar(&d, "alma").unwrap();
        assert_eq!(escolha(&d, "alma"), json!({}));
        // lista de agentes: padrao + o que o arquivo acrescenta ou renomeia
        assert!(so_genero(&d, "alma").as_deref() == Some("F") && so_genero(&d, "jarvis").is_none());
        std::fs::write(d.join("vozes_agentes.json"), r#"{"agentes": [{"id": "alma", "nome": "Alminha"}, {"id": "novo"}, {"nome": "sem id"}]}"#).unwrap();
        let ags = agentes(&d);
        assert!(ags.len() == 5 && ags[1]["nome"] == "Alminha" && ags[1]["so_genero"] == "F" && ags[4]["nome"] == "novo" && todas(&d)["novo"] == json!({}));
        for p in ["acolhedora", "sedutora"] {
            assert!(presets()[p]["ajustes"]["estilo"].as_str().unwrap().contains("sem sussurrar"));
        }
        let _ = std::fs::remove_dir_all(&d);
    }
}
