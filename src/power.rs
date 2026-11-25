//! Interruptor do Braniac (porte de braniac/power.py): liga/desliga manual, modo jogo automatico, autonomia armada por prazo
//! e a chave do atendimento automatico. Estado em dados/estado.json, gravado por troca atomica.
//! Ficou de fora (entra como closure de quem chama): descarregar o modelo local da memoria, criar/apagar o arquivo PAUSE do
//! monitor e listar os processos do Windows (`tasklist`); aqui ficam a regra e o parse da saida do tasklist.
//! Travas mantidas: armar tem teto de 72 h (ou indefinida so com -1 explicito), desligar desarma, e o modo jogo so religa
//! o que ele mesmo pausou (motivo "jogo"), nunca um desligamento manual.
#![allow(dead_code)]
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;

static LK: Mutex<()> = Mutex::new(());
pub const MAX_ARMAR_MIN: i64 = 72 * 60;
/// Armar SEM prazo, ate o dono desarmar (armar(0)).
pub const INDEFINIDA: i64 = -1;
const DEZ_ANOS_S: f64 = 10.0 * 365.0 * 24.0 * 3600.0;

pub fn estado(dados: &Path) -> Value {
    std::fs::read_to_string(dados.join("estado.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).filter(Value::is_object)
        .unwrap_or_else(|| json!({ "pausado": false, "motivo": "", "modo_jogo_auto": true, "jogo": "", "arma_ate": 0, "atendimento_auto": false }))
}

/// Le, muda e grava debaixo da trava; a troca atomica evita que alguem leia o arquivo pela metade.
fn mudar(dados: &Path, f: impl FnOnce(&mut Value)) -> Value {
    let _g = LK.lock().unwrap_or_else(|e| e.into_inner());
    let mut e = estado(dados);
    f(&mut e);
    let _ = std::fs::create_dir_all(dados);
    let tmp = dados.join("estado.tmp");
    if std::fs::write(&tmp, e.to_string()).is_ok() {
        let _ = std::fs::rename(&tmp, dados.join("estado.json"));
    }
    e
}

pub fn pausado(dados: &Path) -> bool {
    estado(dados)["pausado"] == true
}

/// Desligar tambem desarma a autonomia. Quem chama descarrega o modelo local e pausa o monitor.
pub fn desligar(dados: &Path, motivo: &str, jogo: &str) {
    mudar(dados, |e| {
        e["pausado"] = json!(true);
        e["motivo"] = json!(motivo);
        e["jogo"] = json!(jogo);
        e["desde"] = json!(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64()));
        e["arma_ate"] = json!(0);
    });
}

pub fn ligar(dados: &Path) {
    mudar(dados, |e| {
        e["pausado"] = json!(false);
        e["motivo"] = json!("");
        e["jogo"] = json!("");
    });
}

/// Autonomia remota: libera acoes sensiveis vindas de fora por N minutos (teto 72 h) ou INDEFINIDA. 0 desarma.
/// So a interface LOCAL pode chamar isto. Devolve ate quando vale (ts) ou 0.
pub fn armar(dados: &Path, minutos: i64, ts: f64) -> f64 {
    let indefinida = minutos == INDEFINIDA;
    let m = minutos.clamp(0, MAX_ARMAR_MIN);
    let ate = if indefinida { ts + DEZ_ANOS_S } else if m > 0 { ts + m as f64 * 60.0 } else { 0.0 };
    mudar(dados, |e| {
        e["indefinida"] = json!(indefinida);
        e["arma_ate"] = json!(ate);
    });
    ate
}

fn arma_ate(dados: &Path) -> f64 {
    estado(dados)["arma_ate"].as_f64().unwrap_or(0.0)
}

pub fn armada(dados: &Path, ts: f64) -> bool {
    arma_ate(dados) > ts
}

pub fn armada_indefinida(dados: &Path, ts: f64) -> bool {
    let e = estado(dados);
    e["indefinida"] == true && e["arma_ate"].as_f64().unwrap_or(0.0) > ts
}

pub fn armada_restante_min(dados: &Path, ts: f64) -> i64 {
    (((arma_ate(dados) - ts) / 60.0) as i64).max(0)
}

/// Atendimento AUTOMATICO de mensagens. Padrao: desligado. So a interface local liga.
pub fn atendimento_auto(dados: &Path) -> bool {
    estado(dados)["atendimento_auto"] == true
}

pub fn set_atendimento_auto(dados: &Path, valor: bool) {
    mudar(dados, |e| e["atendimento_auto"] = json!(valor));
}

pub fn set_modo_jogo_auto(dados: &Path, valor: bool) {
    mudar(dados, |e| e["modo_jogo_auto"] = json!(valor));
}

/// Nomes (minusculos) dos processos na saida de `tasklist /FO CSV /NH`.
pub fn processos_do_tasklist(saida: &str) -> HashSet<String> {
    saida.lines().filter(|l| !l.is_empty()).map(|l| l.split("\",\"").next().unwrap_or("").trim_matches('"').to_lowercase()).collect()
}

/// Nome do primeiro processo da lista que esta rodando (ou "").
pub fn jogo_aberto(lista: &[String], rodando: &HashSet<String>) -> String {
    lista.iter().find(|nome| {
        let n = nome.to_lowercase();
        rodando.contains(&n) || rodando.contains(&format!("{n}.exe"))
    }).cloned().unwrap_or_default()
}

/// Um passo do vigia do modo jogo (o Python roda isto a cada 10 s): pausa sozinho quando um jogo/editor pesado abre e
/// retoma quando fecha. Devolve "desligou", "ligou" ou "". `rodando()` lista os processos; so e chamada com o modo jogo ligado.
pub fn vigiar_passo(dados: &Path, processos_cfg: &[String], rodando: &dyn Fn() -> HashSet<String>) -> &'static str {
    let e = estado(dados);
    if e["modo_jogo_auto"] == false {
        return "";
    }
    let achou = jogo_aberto(processos_cfg, &rodando());
    let pausado = e["pausado"] == true;
    if !achou.is_empty() && !pausado {
        desligar(dados, "jogo", &achou);
        "desligou"
    } else if achou.is_empty() && pausado && e["motivo"] == "jogo" {
        ligar(dados);
        "ligou"
    } else {
        ""
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TS: f64 = 1_800_000_000.0;

    fn pasta(nome: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("braniac-power-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn liga_desliga_e_padrao_seguro() {
        let p = pasta("liga");
        assert!(!pausado(&p) && !atendimento_auto(&p) && !armada(&p, TS) && estado(&p)["modo_jogo_auto"] == true);
        armar(&p, 30, TS);
        desligar(&p, "manual", "");
        assert!(pausado(&p) && estado(&p)["motivo"] == "manual" && !armada(&p, TS)); // desligar desarma
        ligar(&p);
        assert!(!pausado(&p) && estado(&p)["motivo"] == "");
        set_atendimento_auto(&p, true);
        assert!(atendimento_auto(&p) && !p.join("estado.tmp").exists());
        std::fs::write(p.join("estado.json"), "{meio arquivo").unwrap(); // ilegivel volta ao padrao: nada armado, nada automatico
        assert!(!atendimento_auto(&p) && !armada(&p, TS) && !pausado(&p));
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn armar_tem_teto_prazo_e_indefinida() {
        let p = pasta("armar");
        assert_eq!(armar(&p, 10, TS), TS + 600.0);
        assert!(armada(&p, TS + 599.0) && !armada(&p, TS + 601.0) && !armada_indefinida(&p, TS));
        assert_eq!(armada_restante_min(&p, TS + 90.0), 8);
        assert_eq!(armar(&p, 999_999, TS), TS + (MAX_ARMAR_MIN * 60) as f64); // teto de 72 h
        assert_eq!(armar(&p, -5, TS), 0.0); // negativo que nao e a INDEFINIDA desarma
        assert!(!armada(&p, TS) && armada_restante_min(&p, TS) == 0);
        armar(&p, INDEFINIDA, TS);
        assert!(armada_indefinida(&p, TS + 365.0 * 86400.0));
        assert_eq!(armar(&p, 0, TS), 0.0);
        assert!(!armada(&p, TS) && !armada_indefinida(&p, TS));
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn modo_jogo_so_religa_o_que_ele_pausou() {
        let p = pasta("jogo");
        let cfg = vec!["Jogo".to_string(), "editor.exe".to_string()];
        let saida = "\"System Idle Process\",\"0\",\"Services\",\"0\",\"8 K\"\r\n\"JOGO.exe\",\"4321\",\"Console\",\"1\",\"900.000 K\"\r\n\r\n";
        let com = || processos_do_tasklist(saida);
        let sem = || processos_do_tasklist("\"outro.exe\",\"1\",\"Console\",\"1\",\"1 K\"\n");
        assert!(com().contains("jogo.exe") && com().len() == 2);
        assert_eq!(jogo_aberto(&cfg, &com()), "Jogo");
        assert_eq!(jogo_aberto(&cfg, &sem()), "");
        assert_eq!(vigiar_passo(&p, &cfg, &sem), "");
        assert_eq!(vigiar_passo(&p, &cfg, &com), "desligou");
        assert!(pausado(&p) && estado(&p)["motivo"] == "jogo" && estado(&p)["jogo"] == "Jogo");
        assert_eq!(vigiar_passo(&p, &cfg, &com), ""); // ja pausado
        assert_eq!(vigiar_passo(&p, &cfg, &sem), "ligou");
        desligar(&p, "manual", "");
        assert_eq!(vigiar_passo(&p, &cfg, &sem), ""); // desligamento manual nunca e desfeito pelo vigia
        assert!(pausado(&p));
        ligar(&p);
        set_modo_jogo_auto(&p, false);
        assert_eq!(vigiar_passo(&p, &cfg, &|| panic!("nao deve listar processos")), "");
        let _ = std::fs::remove_dir_all(&p);
    }
}
