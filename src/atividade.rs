//! Monitor de atividades da Alma (braniac/atividade.py): uma amostra por minuto, SO NUMEROS, para saber QUEM esta trabalhando:
//! HUMAN_ACTIVITY (o dono mexendo), AUTONOMOUS_ACTIVITY (agentes sozinhos) ou SUPERVISED_ACTIVITY (os dois juntos).
//! Guarda em dados/alma/atividade.jsonl: segundos sem tocar no PC, quantos comandos ele deu e em quais conversas (so o id, nunca o texto),
//! quantos recados os agentes trocaram e quais agentes. Tarefa autonoma nunca conta como esforco dele.
//! Diferencas para o Python: nada de leitura do sistema nem estado em memoria. A leitura do teclado/mouse (`ocioso`, em segundos; -1 = nao sei),
//! os comandos e os recados NOVOS desde a ultima amostra entram por parametro (`Leitura`); `ts` e hora local (`alma_cuida::ts`).
#![allow(dead_code)]
use crate::alma_cuida::{anexar, hm_de, jsonl, pasta, ts};
use crate::alma_mente as am;
use crate::tempo::Agora;
use serde_json::{json, Value};
use std::path::Path;

pub const HUMANA: &str = "HUMAN_ACTIVITY";
pub const AUTONOMA: &str = "AUTONOMOUS_ACTIVITY";
pub const SUPERVISIONADA: &str = "SUPERVISED_ACTIVITY";
pub const PARADA: &str = "IDLE";
/// Tocou no PC nos ultimos 3 min = esta ali.
pub const PRESENTE_S: i64 = 180;
/// 10 min seguidos sem atividade humana = uma pausa de verdade.
const PAUSA_MIN: i64 = 10;
/// 4 h sem atividade humana = acabou a jornada (dormiu, saiu).
const JORNADA_H: i64 = 4;

/// O que quem liga mediu desde a ultima amostra.
pub struct Leitura<'a> {
    /// Segundos desde o ultimo toque no teclado/mouse; -1 = desconhecido.
    pub ocioso: i64,
    /// Id da conversa/projeto de cada comando que ele deu (um item por comando; nunca o texto).
    pub convs: &'a [String],
    /// Recados NOVOS do barramento ({de, para, tipo}); o conteudo nao e lido.
    pub recados: &'a [Value],
    /// Houve "errou" (retrabalho) desde a ultima amostra.
    pub errou: bool,
    /// Ele mandou pausar a Alma.
    pub pausado: bool,
}

fn n(x: &Value, k: &str) -> i64 {
    x[k].as_f64().unwrap_or(0.0) as i64
}

fn presente(x: &Value) -> bool {
    x["ocioso"].as_i64().is_some_and(|o| (0..PRESENTE_S).contains(&o))
}

/// Pura. Categoria de UMA amostra. Tarefa automatica rodando nunca conta como esforco dele so por estar rodando.
pub fn classificar(a: &Value) -> &'static str {
    let (auto, cmds) = (n(a, "auto") > 0, n(a, "cmds") > 0);
    if auto && (cmds || (presente(a) && n(a, "pedem") > 0)) {
        SUPERVISIONADA
    } else if presente(a) || cmds {
        HUMANA
    } else if auto {
        AUTONOMA
    } else {
        PARADA
    }
}

/// Pura. (recados, agentes, pedidos de atencao). O que o dono disse e o eco da Alma nao contam como trabalho autonomo.
pub fn do_barramento(recados: &[Value]) -> (i64, Vec<String>, i64) {
    let (mut auto, mut agentes, mut pedem) = (0, Vec::new(), 0);
    for m in recados {
        let tipo = m["tipo"].as_str().unwrap_or("");
        // "usuarios_disse" e o tipo que o barramento do Braniac em Python grava; "dono_disse" e o nome neutro
        if matches!(tipo, "dono_disse" | "usuarios_disse") || m["de"] == "Alma" {
            continue;
        }
        auto += 1;
        if let Some(de) = m["de"].as_str() {
            agentes.push(de.to_string());
        }
        pedem += (m["para"] == "Jarvis" && matches!(tipo, "alerta" | "pedido")) as i64;
    }
    agentes.sort();
    agentes.dedup();
    agentes.truncate(8);
    (auto, agentes, pedem)
}

/// Monta e grava a amostra deste minuto. None se o dono pausou a Alma ou desligou o monitor. 1x por dia apaga o que passou da retencao.
pub fn amostrar(dados: &Path, agora: &Agora, l: &Leitura) -> Option<Value> {
    let p = am::params(dados);
    if p["monitor_ligado"] != true || l.pausado {
        return None;
    }
    let (t, arq) = (ts(agora), pasta(dados).join("atividade.jsonl"));
    let mut convs: Vec<String> = l.convs.iter().map(|c| c.chars().take(40).collect()).collect();
    convs.sort();
    convs.dedup();
    convs.truncate(6);
    let (auto, agentes, pedem) = do_barramento(l.recados);
    let mut a = json!({ "ts": t, "ocioso": l.ocioso, "cmds": l.convs.len(), "convs": convs, "auto": auto, "agentes": agentes, "pedem": pedem, "erros": l.errou as i64 });
    a["cat"] = json!(classificar(&a));
    let ultima = jsonl(&arq, 1).first().map(|x| n(x, "ts"));
    anexar(&arq, &a);
    if ultima.is_some_and(|u| u.div_euclid(86400) != t.div_euclid(86400)) {
        let corte = t - n(&p, "atividade_retencao_dias") * 86400;
        let vivas: String = std::fs::read_to_string(&arq).unwrap_or_default().lines()
            .filter(|l| serde_json::from_str::<Value>(l).is_ok_and(|x| n(&x, "ts") >= corte)).map(|l| format!("{l}\n")).collect();
        let _ = std::fs::write(&arq, vivas);
    }
    Some(a)
}

/// Amostras das ultimas `horas`.
pub fn linhas(dados: &Path, agora: &Agora, horas: i64) -> Vec<Value> {
    let corte = ts(agora) - horas * 3600;
    jsonl(&pasta(dados).join("atividade.jsonl"), (horas * 60 + 5) as usize).into_iter().filter(|x| n(x, "ts") >= corte).collect()
}

/// Pura. Indicadores da JORNADA atual (desde o ultimo intervalo de 4 h sem atividade humana). Cada amostra vale 1 minuto.
/// Minutos de tarefa autonoma sao contados A PARTE e nunca entram no esforco humano. `agora` = `alma_cuida::ts`.
pub fn resumir(amostras: &[Value], agora: i64) -> Value {
    fn t(x: &Value) -> i64 {
        n(x, "ts")
    }
    fn cat(x: &Value) -> &str {
        x["cat"].as_str().unwrap_or("")
    }
    fn lista<'a>(xs: &[&'a Value], k: &str) -> Vec<&'a str> {
        xs.iter().flat_map(|x| x[k].as_array().into_iter().flatten().filter_map(|c| c.as_str())).collect()
    }
    let mut hum: Vec<&Value> = amostras.iter().filter(|x| [HUMANA, SUPERVISIONADA].contains(&cat(x))).collect();
    let ini = (1..hum.len()).filter(|&i| t(hum[i]) - t(hum[i - 1]) >= JORNADA_H * 3600).last().unwrap_or(0);
    hum.drain(..ini);
    if hum.last().is_some_and(|u| agora - t(u) >= JORNADA_H * 3600) {
        hum.clear();
    }
    let t0 = hum.first().copied().or(amostras.first()).map_or(agora, t); // sem ele: o que os agentes fizeram sozinhos na janela
    let jornada: Vec<&Value> = amostras.iter().filter(|x| t(x) >= t0).collect();
    let (mut pausas, mut desde) = (0, t0);
    for i in 1..hum.len() {
        if t(hum[i]) - t(hum[i - 1]) >= PAUSA_MIN * 60 {
            pausas += 1;
            desde = t(hum[i]);
        }
    }
    let em_pausa = hum.last().is_some_and(|u| agora - t(u) >= PAUSA_MIN * 60);
    let h2: Vec<&Value> = jornada.iter().copied().filter(|x| t(x) >= agora - 7200).collect();
    let h1: Vec<&Value> = jornada.iter().copied().filter(|x| t(x) >= agora - 3600).collect();
    let seq = lista(&h2, "convs");
    let mut projetos = seq.clone();
    projetos.sort();
    projetos.dedup();
    let mut agentes = lista(&h1, "agentes");
    agentes.sort();
    agentes.dedup();
    let hora = |x: &Value| t(x).rem_euclid(86400) / 3600;
    let conta = |c: &str| hum.iter().filter(|x| cat(x) == c).count();
    json!({
        "min_humano": conta(HUMANA), "min_supervisionado": conta(SUPERVISIONADA),
        "min_autonomo": jornada.iter().filter(|x| cat(x) == AUTONOMA).count(),
        "inicio": if hum.is_empty() { Value::Null } else { json!(t0) },
        "sem_pausa_min": if em_pausa || hum.is_empty() { 0 } else { (agora - desde) / 60 }, "pausas": pausas + em_pausa as i64,
        "trocas_2h": seq.windows(2).filter(|w| w[0] != w[1]).count(), "projetos_2h": projetos.len(),
        "cmds_1h": h1.iter().map(|x| n(x, "cmds")).sum::<i64>(), "erros_2h": h2.iter().map(|x| n(x, "erros")).sum::<i64>(),
        "recados_agentes_1h": h1.iter().map(|x| n(x, "auto")).sum::<i64>(), "agentes_1h": agentes,
        "fora_de_hora_min": hum.iter().filter(|x| hora(x) >= 23 || hora(x) < 6).count(),
        // so teclado/mouse: PC ligado rodando coisa nao conta
        "presente_30min": amostras.iter().filter(|x| t(x) >= agora - 1800 && presente(x)).count(),
        "agora": jornada.last().filter(|u| agora - t(u) < 180).map_or(PARADA, |u| cat(u)),
    })
}

/// Pura. Maior intervalo SEM teclado/mouse (3 h ou mais) que terminou hoje de manha (4h-13h): e o teto do sono dele (pode ter ficado acordado
/// longe do PC). {"min", "parou", "voltou"} com horas HH:MM, ou None se nao da para saber (PC desligado conta: sem amostra = sem uso).
pub fn sono(amostras: &[Value], agora: &Agora) -> Option<Value> {
    let pres: Vec<i64> = amostras.iter().filter(|x| presente(x)).map(|x| n(x, "ts")).collect();
    let mut melhor: Option<(i64, i64)> = None;
    for w in pres.windows(2) {
        let (a, b) = (w[0], w[1]);
        let hora = b.rem_euclid(86400) / 3600;
        if b - a >= 3 * 3600 && b.div_euclid(86400) == agora.data.0 && (4..=13).contains(&hora) && melhor.is_none_or(|m| b - a > m.1 - m.0) {
            melhor = Some((a, b));
        }
    }
    melhor.map(|(a, b)| json!({ "min": (b - a) / 60, "parou": hm_de(a), "voltou": hm_de(b) }))
}

pub fn resumo(dados: &Path, agora: &Agora) -> Value {
    resumir(&linhas(dados, agora, 18), ts(agora))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alma_cuida::teste::*;

    const AGORA: i64 = 20773 * 86400 + 15 * 3600; // um dia qualquer, 15h

    fn am_(minuto: i64, ocioso: i64, auto: i64) -> Value {
        let mut a = json!({ "ts": AGORA - minuto * 60, "ocioso": ocioso, "cmds": 0, "convs": [], "auto": auto, "agentes": [], "pedem": 0, "erros": 0 });
        a["cat"] = json!(classificar(&a));
        a
    }

    #[test]
    fn classifica_humana_autonoma_e_supervisionada() {
        assert_eq!(classificar(&json!({ "ocioso": 5, "cmds": 0, "auto": 0 })), HUMANA);
        assert_eq!(classificar(&json!({ "ocioso": 999, "cmds": 0, "auto": 7 })), AUTONOMA);
        assert_eq!(classificar(&json!({ "ocioso": 5, "cmds": 2, "auto": 3 })), SUPERVISIONADA); // ele comanda enquanto os agentes trabalham
        assert_eq!(classificar(&json!({ "ocioso": 5, "cmds": 0, "auto": 3, "pedem": 1 })), SUPERVISIONADA); // agente pediu a atencao dele
        assert_eq!(classificar(&json!({ "ocioso": 5, "cmds": 0, "auto": 3 })), HUMANA); // agentes sozinhos + ele em outra coisa
        assert_eq!(classificar(&json!({ "ocioso": 999, "cmds": 0, "auto": 0 })), PARADA);
        assert_eq!(classificar(&json!({ "ocioso": -1, "cmds": 0, "auto": 0 })), PARADA); // presenca desconhecida nao vira trabalho
        assert_eq!(classificar(&json!({})), PARADA);
    }

    #[test]
    fn autonoma_nao_conta_como_esforco_e_pausa_zera_o_contador() {
        let noite: Vec<Value> = (1..=300).rev().map(|m| am_(m, 999, 5)).collect(); // 5 h de agentes sozinhos
        let r = resumir(&noite, AGORA);
        assert!(r["min_autonomo"] == 300 && r["min_humano"] == 0 && r["sem_pausa_min"] == 0 && r["inicio"].is_null() && r["presente_30min"] == 0);
        let direto: Vec<Value> = (1..=200).rev().map(|m| am_(m, 10, 0)).collect();
        let r = resumir(&direto, AGORA);
        assert!(r["min_humano"] == 200 && r["sem_pausa_min"].as_i64().unwrap() >= 199 && r["pausas"] == 0 && r["agora"] == HUMANA && r["presente_30min"] == 30);
        let com_pausa: Vec<Value> = (41..=200).rev().chain((1..=20).rev()).map(|m| am_(m, 10, 0)).collect();
        let r2 = resumir(&com_pausa, AGORA);
        assert!(r2["pausas"] == 1 && r2["sem_pausa_min"].as_i64().unwrap() <= 21);
        let antiga: Vec<Value> = (401..=600).rev().map(|m| am_(m, 10, 0)).collect();
        assert_eq!(resumir(&antiga, AGORA)["min_humano"], 0); // jornada antiga (parou ha mais de 4 h) nao conta
        assert_eq!(resumir(&[], AGORA)["agora"], PARADA);
    }

    #[test]
    fn amostra_grava_so_numeros_respeita_pausa_e_poda() {
        let p = dir("atividade");
        let recados = [json!({ "de": "Forja", "para": "Jarvis", "tipo": "alerta", "texto": "temperatura alta" }),
                       json!({ "de": "Jarvis", "para": "Alma", "tipo": "dono_disse", "texto": "frase particular" })];
        let convs = ["b2:7".to_string()];
        let mut l = Leitura { ocioso: 4, convs: &convs, recados: &recados, errou: false, pausado: false };
        let a = amostrar(&p, &em(1, 15, 0), &l).unwrap();
        assert!(a["cat"] == SUPERVISIONADA && a["cmds"] == 1 && a["convs"] == json!(["b2:7"]) && a["auto"] == 1 && a["pedem"] == 1 && a["agentes"] == json!(["Forja"]));
        let bruto = std::fs::read_to_string(pasta(&p).join("atividade.jsonl")).unwrap();
        assert!(!bruto.contains("particular") && !bruto.contains("temperatura")); // nenhum conteudo, so contagem
        l.pausado = true;
        assert!(amostrar(&p, &em(1, 15, 1), &l).is_none()); // PAUSAR tudo para o monitor tambem
        l.pausado = false;
        amostrar(&p, &em(20, 9, 0), &l).unwrap(); // 19 dias depois: a amostra velha passa da retencao (14 dias) e sai
        assert_eq!(std::fs::read_to_string(pasta(&p).join("atividade.jsonl")).unwrap().lines().count(), 1);
        assert_eq!(linhas(&p, &em(20, 9, 30), 18).len(), 1);
        am::definir(&p, "monitor_ligado", &json!(false));
        assert!(amostrar(&p, &em(20, 9, 1), &l).is_none());
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn sono_pelo_tempo_longe_do_pc() {
        let agora = em(17, 6, 5);
        let base = agora.data.0 * 86400;
        let a = |h: i64, m: i64, ocioso: i64| json!({ "ts": base + h * 3600 + m * 60, "ocioso": ocioso });
        // PC ligado a noite toda, ele so voltou as 6h
        let noite: Vec<Value> = [a(-1, 0, 5), a(0, 0, 5)].into_iter().chain((1..6).map(|h| a(h, 0, 9999))).chain([a(6, 0, 5), a(6, 1, 5)]).collect();
        assert_eq!(sono(&noite, &agora), Some(json!({ "min": 360, "parou": "00:00", "voltou": "06:00" })));
        assert_eq!(sono(&[a(-3, 0, 5), a(5, 30, 5)], &agora).unwrap()["min"], 510);
        assert_eq!(sono(&[a(8, 0, 5), a(8, 30, 5)], &agora), None); // sem intervalo longo: ela so pergunta
        assert_eq!(sono(&[a(9, 0, 5), a(15, 0, 5)], &agora), None); // sumico de tarde nao e sono
    }
}
