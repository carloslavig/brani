//! Rede de apoio da Alma (braniac/rede_apoio.py). Nao cria canal novo: tudo sai por `alma_cuida::mandar` (closure de quem chama, tetos por
//! pessoa, filtro de segredo, apelidos intimos trocados). Aqui ficam: o HISTORICO local de todo acionamento (dados/alma/rede_apoio.jsonl),
//! a diferenca entre sobrecarga, sofrimento e emergencia (`situacao`) e o aviso DISCRETO de dia puxado a parceira, que nasce DESLIGADO
//! (`rede_parceira_sobrecarga`), sai no maximo 1x por dia, so das 17h as 21h e so se a sobrecarga foi elevada por 30 min ou mais.
//! Silencio do dono sozinho NUNCA aciona ninguem: nao existe caminho daqui que saia de "ele nao respondeu" para um contato.
//! Diferenca para o Python: o contato e o papel ("parceira"), nao o nome; o sorteio do texto usa a hora.
#![allow(dead_code)]
use crate::alma_cuida::{self as ac, anexar, cortar, jsonl, pasta, ts, Alma, Nomes};
use crate::alma_mente as am;
use crate::tempo::Agora;
use serde_json::{json, Value};
use std::path::Path;

pub const SOBRECARGA: &str = "sobrecarga";
pub const SOFRIMENTO: &str = "sofrimento";
pub const EMERGENCIA: &str = "emergencia";
/// Sem detalhe de projeto, sem frase dele, sem tom de alarme, sem pedir que ela pare o que esta fazendo.
const TEXTOS_PARCEIRA: [&str; 2] = [
    "Oi, {parceira}, é a Alma. O {dono} teve um dia bastante puxado hoje. Se puder, um pouco de carinho e tranquilidade quando vocês estiverem juntos pode ajudar. Nada de urgente 💗",
    "{parceira}, é a Alma. Hoje o {dono} acabou ficando bem sobrecarregado com o trabalho. Nada de urgente, só achei que seria bom você saber 💗",
];

/// Pura. Que tipo de situacao e esta, da mais grave para a mais leve ("" = nenhuma). c = alma_cuida::classificar(fala dele).
pub fn situacao(c: &Value, estado_sobrecarga: &str) -> &'static str {
    let urg = c["urgencia"].as_i64().unwrap_or(0);
    if c["crise"] == true || urg >= 3 {
        EMERGENCIA
    } else if urg >= 2 {
        SOFRIMENTO
    } else if ["ELEVATED", "HIGH"].contains(&estado_sobrecarga) {
        SOBRECARGA
    } else {
        ""
    }
}

/// Historico local de quem a Alma acionou, quando e com que texto (o texto nunca traz conversa dele).
pub fn registrar(dados: &Path, contato: &str, texto: &str, agora: &Agora, n: &Nomes) {
    let tipo = if texto.contains("URGENTE") { EMERGENCIA } else if TEXTOS_PARCEIRA.iter().any(|t| n.t(t) == texto) { SOBRECARGA } else { "apoio" };
    anexar(&pasta(dados).join("rede_apoio.jsonl"), &json!({ "ts": ts(agora), "contato": contato, "situacao": tipo, "texto": cortar(texto, 220) }));
}

pub fn historico(dados: &Path, n: usize) -> Vec<Value> {
    jsonl(&pasta(dados).join("rede_apoio.jsonl"), n)
}

/// A Alma (por qualquer motivo) ja falou com este contato nas ultimas `horas`? Evita mensagem em dobro.
pub fn ja_avisado(dados: &Path, contato: &str, horas: i64, agora: &Agora) -> bool {
    let corte = ts(agora) - horas * 3600;
    historico(dados, 60).iter().any(|x| x["contato"] == contato && x["ts"].as_i64().unwrap_or(0) >= corte)
}

/// Aviso gentil a parceira no fim de um dia pesado (av = sobrecarga::avaliar). So com permissao do dono, 1x por dia, das 17h as 21h, e so se a
/// sobrecarga foi ELEVADA por pelo menos 30 min (nao por um pico). true se enviou.
pub fn avisar_sobrecarga(a: &Alma, av: &Value) -> bool {
    let p = am::params(a.dados);
    if p["rede_parceira_sobrecarga"] != true || !(17..=21).contains(&a.agora.hora) {
        return false;
    }
    if !["ELEVATED", "HIGH"].contains(&av["pico"].as_str().unwrap_or("")) || av["min_elevado"].as_i64().unwrap_or(0) < 30 || ja_avisado(a.dados, "parceira", 20, &a.agora) {
        return false;
    }
    if !ac::pode(a.dados, "parceira", &a.agora) {
        return false;
    }
    ac::mandar(a, "parceira", &a.nomes.t(TEXTOS_PARCEIRA[ts(&a.agora).rem_euclid(2) as usize])).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alma_cuida::teste::*;

    #[test]
    fn parceira_so_com_permissao_uma_vez_no_fim_do_dia_e_sem_detalhes() {
        let (p, n, cx) = (dir("rede"), nomes(), Caixa::default());
        let env = anota(&cx);
        let mut a = Alma { dados: &p, nomes: &n, agora: em(16, 19, 0), enviar: &env, audio: None, llm: None };
        let pesado = json!({ "pico": "HIGH", "min_elevado": 45, "estado": "NORMAL" });
        assert!(!avisar_sobrecarga(&a, &pesado) && cx.borrow().is_empty()); // nasce DESLIGADO
        am::definir(&p, "rede_parceira_sobrecarga", &json!(true));
        a.agora = em(16, 11, 0);
        assert!(!avisar_sobrecarga(&a, &pesado)); // so no fim do dia
        a.agora = em(16, 22, 0);
        assert!(!avisar_sobrecarga(&a, &pesado));
        a.agora = em(16, 19, 0);
        assert!(!avisar_sobrecarga(&a, &json!({ "pico": "HIGH", "min_elevado": 5 }))); // pico curto nao e dia pesado
        assert!(!avisar_sobrecarga(&a, &json!({ "pico": "ATTENTION", "min_elevado": 90 })));
        assert!(avisar_sobrecarga(&a, &pesado));
        a.agora = em(16, 20, 0);
        assert!(!avisar_sobrecarga(&a, &pesado) && cx.borrow().len() == 1); // sem duplicar
        let (quem, texto) = cx.borrow()[0].clone();
        assert!(quem == "parceira" && texto.contains("Musa Bia") && texto.contains("Nada de urgente") && !texto.contains("URGENTE")); // sobrecarga nao e emergencia
        assert!(!["braniac", "site", "projeto", "disse", "querido", "amor"].iter().any(|x| texto.to_lowercase().contains(x))); // sem detalhe nem intimidade
        let h = historico(&p, 30);
        assert!(h.len() == 1 && h[0]["contato"] == "parceira" && h[0]["situacao"] == "sobrecarga");
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn situacao_da_mais_grave_para_a_mais_leve() {
        assert_eq!(situacao(&json!({ "crise": true }), "NORMAL"), EMERGENCIA);
        assert_eq!(situacao(&json!({ "urgencia": 3 }), "NORMAL"), EMERGENCIA);
        assert_eq!(situacao(&json!({ "tema": "familia", "urgencia": 2 }), "HIGH"), SOFRIMENTO);
        assert_eq!(situacao(&json!({}), "HIGH"), SOBRECARGA);
        assert_eq!(situacao(&json!({}), "ATTENTION"), "");
    }
}
