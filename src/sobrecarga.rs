//! Detector de sobrecarga da Alma (braniac/sobrecarga.py). Indice HEURISTICO de 0 a 100 a partir do resumo de atividade HUMANA
//! (`atividade::resumir`). Nao e diagnostico nem medida de saude: e so um palpite para a Alma saber quando vale oferecer ajuda.
//! Varios sinais (horas de trabalho, tempo sem pausa, troca de projeto, comandos, retrabalho, madrugada, humor que ELE declarou, pouco sono);
//! nenhum sinal sozinho tira o estado de NORMAL (precisa de pelo menos dois); tarefa autonoma dos agentes nao entra na conta.
//! Limites em dados/alma/parametros.json; historico so com numeros em dados/alma/sobrecarga.jsonl (1 linha a cada 5 min).
#![allow(dead_code)]
use crate::alma_cuida::{self as ac, anexar, jsonl, pasta, ts};
use crate::alma_mente as am;
use crate::atividade;
use crate::tempo::Agora;
use serde_json::{json, Value};
use std::path::Path;

pub const NORMAL: &str = "NORMAL";
pub const ATENCAO: &str = "ATTENTION";
pub const ELEVADO: &str = "ELEVATED";
pub const ALTO: &str = "HIGH";
const ORDEM: [&str; 4] = [NORMAL, ATENCAO, ELEVADO, ALTO];
const PESA: [&str; 3] = ["estresse", "cansaco", "tristeza"];

/// 0 ponto ate `de`, `pontos` a partir de `ate`, reta no meio.
fn rampa(v: f64, de: f64, ate: f64, pontos: f64) -> f64 {
    if v <= de { 0.0 } else if v >= ate { pontos } else { pontos * (v - de) / (ate - de) }
}

fn grau(estado: &str) -> usize {
    ORDEM.iter().position(|e| *e == estado).unwrap_or(0)
}

/// Pura. r = atividade::resumir(); humor_hoje = estados que ele declarou hoje; p = parametros (limites). Devolve {indice, estado, sinais}.
pub fn indice(r: &Value, humor_hoje: &[String], p: &Value) -> Value {
    let f = |k: &str| r[k].as_f64().unwrap_or(0.0);
    let lim = |k: &str, padrao: f64| p[k].as_f64().unwrap_or(padrao);
    let sono = f("sono_min");
    let brutos = [
        ("horas de trabalho", rampa((f("min_humano") + f("min_supervisionado")) / 60.0, 4.0, 10.0, 25.0)),
        ("tempo sem pausa", rampa(f("sem_pausa_min"), 60.0, 180.0, 25.0)),
        ("troca de projeto", rampa(f("trocas_2h"), 2.0, 8.0, 15.0)),
        ("comandos manuais", rampa(f("cmds_1h"), 10.0, 40.0, 10.0)),
        ("erros e retrabalho", (5.0 * f("erros_2h")).min(10.0)),
        ("horário incomum", rampa(f("fora_de_hora_min"), 0.0, 60.0, 10.0)),
        ("humor declarado", (5.0 * humor_hoje.iter().filter(|h| PESA.contains(&h.as_str())).count() as f64).min(10.0)),
        ("pouco sono", if sono > 0.0 { rampa(60.0 * lim("sono_minimo_h", 7.0) - sono, 0.0, 120.0, 10.0) } else { 0.0 }),
    ];
    let sinais: Vec<(&str, f64)> = brutos.into_iter().filter(|s| s.1 > 0.0).map(|(k, v)| (k, (v * 10.0).round() / 10.0)).collect();
    let n = sinais.iter().map(|s| s.1).sum::<f64>().round().min(100.0);
    let fortes = sinais.iter().filter(|s| s.1 >= 5.0).count();
    let mut est = if n >= lim("limite_alto", 75.0) { ALTO } else if n >= lim("limite_elevado", 55.0) { ELEVADO } else if n >= lim("limite_atencao", 35.0) { ATENCAO } else { NORMAL };
    if fortes < 2 {
        est = NORMAL; // um indicador sozinho nunca passa de NORMAL
    }
    json!({ "indice": n as i64, "estado": est, "sinais": sinais.into_iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<serde_json::Map<_, _>>() })
}

fn humor_hoje(dados: &Path, agora: &Agora) -> Vec<String> {
    let hoje = agora.data.iso();
    jsonl(&pasta(dados).join("humor_diario.jsonl"), 40).iter().filter(|x| x["dia"] == hoje.as_str()).filter_map(|x| x["estado"].as_str().map(String::from)).collect()
}

/// Indice de agora a partir de um resumo `r` + ha quanto tempo ele esta em cada estado ou acima (`desde`), o pico do dia e os minutos em
/// estado elevado. `gravar` guarda isso em cuida.json e, a cada 5 min, uma linha no historico.
pub fn avaliar_com(dados: &Path, agora: &Agora, mut r: Value, gravar: bool) -> Value {
    let (p, t, hoje) = (am::params(dados), ts(agora), agora.data.iso());
    let antes = ac::estado(dados);
    if antes["sono"]["dia"] == hoje.as_str() {
        r["sono_min"] = antes["sono"]["min"].clone(); // noite curta pesa o dia inteiro
    }
    let mut out = if p["sobrecarga_ligada"] == true { indice(&r, &humor_hoje(dados, agora), &p) } else { json!({ "indice": 0, "estado": NORMAL, "sinais": {} }) };
    out["resumo"] = r;
    let g = grau(out["estado"].as_str().unwrap_or(NORMAL));
    let calcular = |e: &mut Value| {
        let s = e["sobrec"].clone();
        let mut desde = s["desde"].as_object().cloned().unwrap_or_default();
        for (i, nome) in ORDEM.iter().enumerate().skip(1) {
            // 'desde' por estado: ATTENTION conta enquanto estiver em ATTENTION ou acima
            if g >= i {
                desde.entry(nome.to_string()).or_insert(json!(t));
            } else {
                desde.remove(*nome);
            }
        }
        let do_dia = s["dia"] == hoje.as_str();
        let mut pico = if do_dia { s["pico"].as_str().unwrap_or(NORMAL).to_string() } else { NORMAL.to_string() };
        let min_elev = if do_dia { s["min_elevado"].as_i64().unwrap_or(0) } else { 0 } + (g >= 2) as i64;
        if g > grau(&pico) {
            pico = ORDEM[g].to_string();
        }
        e["sobrec"] = json!({ "dia": hoje, "estado": ORDEM[g], "indice": out["indice"], "desde": desde, "pico": pico, "min_elevado": min_elev, "ts": t });
        (json!(desde), pico, min_elev)
    };
    let (desde, pico, min_elev) = if gravar { ac::mudar(dados, calcular) } else { calcular(&mut antes.clone()) };
    out["desde"] = desde;
    out["pico"] = json!(pico);
    out["min_elevado"] = json!(min_elev);
    if gravar && agora.min % 5 == 0 {
        let mut nomes: Vec<String> = out["sinais"].as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
        nomes.sort();
        anexar(&pasta(dados).join("sobrecarga.jsonl"), &json!({ "ts": t, "indice": out["indice"], "estado": out["estado"], "sinais": nomes }));
    }
    out
}

pub fn avaliar(dados: &Path, agora: &Agora, gravar: bool) -> Value {
    avaliar_com(dados, agora, atividade::resumo(dados, agora), gravar)
}

pub fn historico(dados: &Path, n: usize) -> Vec<Value> {
    jsonl(&pasta(dados).join("sobrecarga.jsonl"), n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alma_cuida::teste::*;

    fn pesado() -> Value {
        json!({ "min_humano": 540, "min_supervisionado": 60, "sem_pausa_min": 200, "trocas_2h": 9, "cmds_1h": 45, "erros_2h": 2, "fora_de_hora_min": 0 })
    }

    #[test]
    fn indice_precisa_de_mais_de_um_sinal() {
        let v = json!({});
        let i = indice(&pesado(), &[], &v);
        assert!(i["estado"] == ALTO && i["indice"].as_i64().unwrap() >= 75 && i["sinais"].as_object().unwrap().len() >= 4);
        let so_horas = indice(&json!({ "min_humano": 900 }), &[], &json!({ "limite_atencao": 10 }));
        assert!(so_horas["indice"] == 25 && so_horas["estado"] == NORMAL); // um indicador sozinho nunca passa de NORMAL
        assert!([NORMAL, ATENCAO].contains(&indice(&json!({ "min_humano": 420, "sem_pausa_min": 150 }), &[], &v)["estado"].as_str().unwrap()));
        assert_eq!(indice(&json!({ "min_humano": 120, "sem_pausa_min": 30 }), &[], &v)["indice"], 0); // dia comum
        assert_eq!(indice(&pesado(), &[], &json!({ "limite_alto": 99, "limite_elevado": 98, "limite_atencao": 97 }))["estado"], NORMAL); // limites configuraveis
        assert_eq!(indice(&json!({ "min_autonomo": 300, "min_humano": 0 }), &[], &v)["indice"], 0); // agentes sozinhos nao pesam
        assert!(indice(&json!({ "min_humano": 300, "sono_min": 300 }), &[], &v)["sinais"]["pouco sono"] == 10.0 && indice(&json!({ "sono_min": 480 }), &[], &v)["sinais"]["pouco sono"].is_null());
        assert_eq!(indice(&json!({}), &["estresse".to_string(), "alegria".to_string(), "cansaco".to_string(), "tristeza".to_string()], &v)["sinais"]["humor declarado"], 10.0);
    }

    #[test]
    fn avaliar_guarda_ha_quanto_tempo_e_o_pico() {
        let p = dir("sobrecarga");
        let a1 = avaliar_com(&p, &em(16, 15, 0), pesado(), true);
        let a2 = avaliar_com(&p, &em(16, 15, 40), pesado(), true);
        assert!(a2["desde"]["HIGH"] == a1["desde"]["HIGH"] && a1["desde"]["HIGH"] == ts(&em(16, 15, 0)) && a2["pico"] == ALTO && a2["min_elevado"] == 2);
        assert_eq!(avaliar_com(&p, &em(16, 15, 41), pesado(), false)["min_elevado"], 3); // sem gravar: so calcula
        let a3 = avaliar_com(&p, &em(16, 15, 41), json!({ "min_humano": 60 }), true);
        assert!(a3["desde"] == json!({}) && a3["pico"] == ALTO && a3["min_elevado"] == 2); // passou; o pico do dia fica (para o aviso discreto)
        assert_eq!(historico(&p, 48)[0]["estado"], "HIGH");
        assert_eq!(avaliar_com(&p, &em(17, 9, 0), json!({ "min_humano": 60 }), true)["pico"], NORMAL); // dia novo zera o pico
        am::definir(&p, "sobrecarga_ligada", &json!(false));
        assert_eq!(avaliar_com(&p, &em(17, 9, 1), pesado(), true)["indice"], 0);
        assert_eq!(avaliar(&p, &em(17, 9, 2), false)["estado"], NORMAL);
        let _ = std::fs::remove_dir_all(&p);
    }
}
