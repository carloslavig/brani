//! O Braniac PEDE ao par o proximo video quando esta livre (link/puxar.py). So pede se: todos os pedidos de producao estao
//! entregues/erro/esperando, nenhum render na fila, placa de video abaixo de 80 C, fora da madrugada (7h-22h), 2 h desde o
//! ultimo pedido e no maximo 3 por dia. Pedido que nao foi entregue nao conta. Estado em dados/link/puxar.json.
//! Daqui nada e enviado: `enviar(texto) -> entregue?` e de quem chama. A temperatura da placa e o tempo medio de render tambem
//! entram por parametro (aqui nao se roda programa). Diferenca do Python: cada pedido guarda tambem o "dia" local.
#![allow(dead_code)]
use crate::link_identidade::id_novo;
use crate::tempo::{Agora, Data};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const INTERVALO_S: f64 = 2.0 * 3600.0;
pub const MAX_DIA: usize = 3;
pub const TEMP_MAX: i64 = 80;

fn arq(dados: &Path) -> PathBuf {
    dados.join("link").join("puxar.json")
}

fn pedidos(dados: &Path) -> Vec<Value> {
    let d: Value = std::fs::read_to_string(arq(dados)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    d["pedidos"].as_array().cloned().unwrap_or_default()
}

fn ts(a: &Agora) -> f64 {
    (a.data.0 * 86400 + a.segundos()) as f64
}

fn dia_de(p: &Value) -> String {
    // shortcut: pedido antigo (do Python) so tem "ts" em UTC; perto da meia-noite pode cair no dia vizinho
    p["dia"].as_str().map(String::from).unwrap_or_else(|| Data((p["ts"].as_f64().unwrap_or(0.0) as i64).div_euclid(86400)).iso())
}

/// Media dos 5 ultimos renders ("minutos" de cada linha do tempos.jsonl), ou None.
pub fn tempo_medio(tempos_jsonl: &str) -> Option<i64> {
    let ts: Option<Vec<f64>> = tempos_jsonl.lines().filter(|l| !l.trim().is_empty()).map(|l| serde_json::from_str::<Value>(l).ok()?["minutos"].as_f64()).collect();
    let ts = ts.filter(|t| !t.is_empty())?;
    let ultimos = &ts[ts.len().saturating_sub(5)..];
    Some((ultimos.iter().sum::<f64>() / ultimos.len() as f64).round_ties_even() as i64)
}

/// Motivo para NAO pedir agora, ou None se pode pedir.
pub fn livre(dados: &Path, producao_pedidos: &[Value], fila_ocupada: bool, temp: Option<i64>, agora: &Agora) -> Option<String> {
    if !(7..22).contains(&agora.hora) {
        return Some("madrugada".into());
    }
    if producao_pedidos.iter().any(|x| !matches!(x["etapa"].as_str(), Some("entregue" | "erro" | "esperando"))) {
        return Some("ainda ha pedido em andamento".into()); // parado (esperando) nao segura a fila
    }
    if fila_ocupada {
        return Some("render na fila".into());
    }
    if let Some(t) = temp.filter(|t| *t >= TEMP_MAX) {
        return Some(format!("GPU a {t} C"));
    }
    let feitos = pedidos(dados);
    if feitos.iter().filter(|p| dia_de(p) == agora.data.iso()).count() >= MAX_DIA {
        return Some("limite do dia".into());
    }
    if feitos.last().is_some_and(|p| ts(agora) - p["ts"].as_f64().unwrap_or(0.0) < INTERVALO_S) {
        return Some("pedido recente".into());
    }
    None
}

/// Pede o proximo video se estiver livre. Devolve o id do pedido enviado, ou None (nao podia, ou o envio nao foi entregue).
pub fn talvez_pedir(dados: &Path, enviar: &dyn Fn(&str) -> bool, producao_pedidos: &[Value], fila_ocupada: bool, temp: Option<i64>, medio: Option<i64>, agora: &Agora) -> Option<String> {
    if livre(dados, producao_pedidos, fila_ocupada, temp, agora).is_some() {
        return None;
    }
    let pid = id_novo(10);
    let txt = format!(
        "PEDIDO PROXIMO v1 · id={pid} · O Braniac está livre (fila vazia{}). Pode mandar o próximo vídeo da sua fila: produção completa (zip com LEIA-PRIMEIRO, como o 143) \
         ou só render (video-NNN.zip, ou meta.json + audio.mp3). {}Se não houver vídeo, responda SEM PEDIDO {pid}.",
        temp.map(|t| format!(", GPU {t} °C")).unwrap_or_default(),
        medio.filter(|m| *m != 0).map(|m| format!("Tempo médio de render aqui: {m} min. ")).unwrap_or_default()
    );
    if !enviar(&txt) {
        return None;
    }
    let mut e = pedidos(dados);
    e.push(json!({ "id": pid, "ts": ts(agora), "dia": agora.data.iso() }));
    let corte = e.len().saturating_sub(50);
    std::fs::create_dir_all(dados.join("link")).ok()?;
    std::fs::write(arq(dados), json!({ "pedidos": e[corte..] }).to_string()).ok()?;
    Some(pid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn pasta(nome: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("braniac-link_puxar-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn as_(hora: u32) -> Agora {
        Agora { data: Data::civil(2026, 11, 13).unwrap(), hora, min: 0, seg: 0 }
    }

    #[test]
    fn pede_quando_livre_e_grava_o_id() {
        let d = pasta("pede");
        let enviados = RefCell::new(vec![]);
        let enviar = |t: &str| { enviados.borrow_mut().push(t.to_string()); true };
        let medio = tempo_medio("{\"minutos\": 99}\n{\"minutos\": 15}\n{\"minutos\": 17}\n\n{\"minutos\": 19}\n{\"minutos\": 16}\n{\"minutos\": 18}\n");
        assert_eq!((medio, tempo_medio(""), tempo_medio("lixo")), (Some(17), None, None));
        let pid = talvez_pedir(&d, &enviar, &[json!({ "etapa": "entregue" })], false, Some(55), medio, &as_(10)).unwrap();
        let t = enviados.borrow()[0].clone();
        assert!(t.contains(&pid) && t.contains("17 min") && t.contains("GPU 55 °C") && t.starts_with("PEDIDO PROXIMO v1 · id="), "{t}");
        assert_eq!(talvez_pedir(&d, &enviar, &[], false, None, None, &Agora { min: 1, ..as_(10) }), None); // 2 h entre pedidos
        assert_eq!(livre(&d, &[], false, None, &as_(11)).as_deref(), Some("pedido recente"));
        assert_eq!(enviados.borrow().len(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn nao_pede() {
        let d = pasta("nao");
        let casos: [(Vec<Value>, bool, i64, u32, &str); 6] = [
            (vec![json!({ "etapa": "pesquisa" })], false, 55, 10, "andamento"),
            (vec![], true, 55, 10, "fila"),
            (vec![], false, 85, 10, "GPU"),
            (vec![], false, 80, 10, "GPU"),
            (vec![], false, 55, 3, "madrugada"),
            (vec![], false, 55, 22, "madrugada"),
        ];
        for (ped, ocupada, temp, hora, motivo) in casos {
            assert!(livre(&d, &ped, ocupada, Some(temp), &as_(hora)).unwrap().contains(motivo), "{motivo}");
            assert_eq!(talvez_pedir(&d, &|_| panic!("pediu sem poder"), &ped, ocupada, Some(temp), None, &as_(hora)), None);
        }
        // pedido parado nao segura a fila
        assert_eq!(livre(&d, &[json!({ "etapa": "esperando" }), json!({ "etapa": "entregue" })], false, Some(55), &as_(10)), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn limite_do_dia_e_falha_de_envio_nao_conta() {
        let d = pasta("limite");
        assert_eq!(talvez_pedir(&d, &|_| false, &[], false, None, None, &as_(8)), None); // nao entregue: nao grava
        for k in 0..3 {
            assert!(talvez_pedir(&d, &|_| true, &[], false, None, None, &as_(8 + 3 * k)).is_some());
        }
        assert_eq!(livre(&d, &[], false, Some(55), &Agora { min: 30, ..as_(17) }).as_deref(), Some("limite do dia"));
        assert_eq!(livre(&d, &[], false, Some(55), &Agora { data: Data(as_(8).data.0 + 1), ..as_(8) }), None); // no outro dia volta
        let _ = std::fs::remove_dir_all(&d);
    }
}
