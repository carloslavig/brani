//! Painel da interface: BOTAO DE PANICO, situacao dos "bracos" e historico de acoes (lido dos logs locais).
//! O panico so DESLIGA e DESARMA (direcao segura), por isso pode vir de qualquer canal. Nada aqui arma ou liga algo.
//! Ficou de fora: nada de fora do processo e chamado. Diferenca do Python: o log do dono aparece como "Dono" (la, com o nome dele).
#![allow(dead_code)]
use crate::empresa_dev::corta;
use crate::tempo::Agora;
use crate::{airbnb_vigia, guarda, manutencao, power};
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;

/// arquivo de log -> braco
pub const LOGS: [(&str, &str); 8] = [("vigia.log", "WhatsApp"), ("atendimentos.log", "WhatsApp"), ("dono.log", "Dono"), ("airbnb.log", "Airbnb"), ("delegacoes.log", "Agente"),
    ("seguranca.log", "Segurança"), ("painel.log", "Painel"), ("manutencao.log", "Manutenção")];

/// Desarma a autonomia remota e desliga o atendimento automatico do WhatsApp e do Airbnb. Devolve o que foi feito.
pub fn panico(dados: &Path, origem: &str, agora: &Agora, ts: f64) -> Value {
    let mut feito: Vec<String> = Vec::new();
    if power::armada(dados, ts) {
        feito.push("autonomia remota desarmada".into());
    }
    power::armar(dados, 0, ts);
    if power::atendimento_auto(dados) {
        feito.push("atendimento automático do WhatsApp desligado".into());
    }
    power::set_atendimento_auto(dados, false);
    let estava = airbnb_vigia::ligado(dados);
    match airbnb_vigia::desligar(dados) {
        Ok(()) if estava => feito.push("atendimento automático do Airbnb desligado".into()),
        Ok(()) => {}
        Err(e) => feito.push(format!("Airbnb: não consegui confirmar ({})", corta(&e, 60))),
    }
    let resumo = if feito.is_empty() { "tudo já estava desligado".to_string() } else { feito.join("; ") };
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dados.join("painel.log")) {
        let _ = writeln!(f, "{} {}:{:02} | PANICO ({origem}): {resumo}", agora.data.iso(), agora.hm(), agora.seg);
    }
    json!({ "ok": true, "feito": feito, "texto": format!("🛑 Pânico acionado: {resumo}. Para religar, use os botões da interface no PC.") })
}

/// [{nome, ok (ligado/saudavel), detalhe}] para o painel. `status` = o /api/status ja montado.
pub fn bracos(dados: &Path, status: &Value, agora: &Agora, ts: f64) -> Vec<Value> {
    let lig = |k: &str| status[k]["ligado"].as_bool().unwrap_or(false);
    let b = |nome: &str, ok: bool, detalhe: String| json!({ "nome": nome, "ok": ok, "detalhe": detalhe });
    let (armada, escuta, email) = (power::armada(dados, ts), status["escuta_voz"]["estado"].as_str(), status["email"]["logado"].as_bool().unwrap_or(false));
    let hoje = agora.data.iso();
    let rot: Value = std::fs::read_to_string(dados.join("rotina.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    let bom_dia = rot["bom_dia"].as_str().unwrap_or("");
    let m = manutencao::ultimo(dados);
    let rodou = m.as_object().is_some_and(|o| !o.is_empty());
    let testes_ok = m["testes_ok"].as_bool().unwrap_or(false);
    vec![
        b("Atendimento WhatsApp", lig("atendimento_auto"), if lig("atendimento_auto") { format!("ligado · {} resp./h", status["atendimento_auto"]["respostas_ultima_hora"].as_i64().unwrap_or(0)) } else { "desligado".into() }),
        b("Airbnb", lig("airbnb_auto"), if lig("airbnb_auto") { "ligado" } else { "desligado" }.into()),
        b("Autonomia remota", armada, if armada { format!("ARMADA · {} min", power::armada_restante_min(dados, ts)) } else { "desarmada".into() }),
        b("Microfone (voz)", escuta.is_some_and(|e| e != "parada"), escuta.unwrap_or("parada").into()),
        b("E-mail (Gmail)", email, if email { "conectado" } else { "não conectado (/email)" }.into()),
        b("Guarda de segurança", true, corta(&guarda::resumo(dados), 90)),
        b("Bom dia das 6h", bom_dia == hoje, if bom_dia == hoje { "enviado hoje".into() } else { format!("último: {}", if bom_dia.is_empty() { "nunca" } else { bom_dia }) }),
        b("Manutenção da madrugada", rodou && testes_ok, if !rodou { "ainda não rodou".into() } else {
            format!("{} · {}", m["quando"].as_str().unwrap_or("?"), if testes_ok { "testes OK".to_string() } else { format!("ATENÇÃO: {}", corta(m["resumo"].as_str().unwrap_or(""), 60)) })
        }),
    ]
}

/// Ultimas acoes dos bracos, dos logs LOCAIS: [{quando, braco, texto}] do mais novo ao mais antigo. So linha no formato "AAAA-MM-DD HH:MM:SS | texto".
pub fn historico(dados: &Path, n: usize, max_linha: usize) -> Vec<Value> {
    let mut itens: Vec<(String, Value)> = Vec::new();
    for (arq, braco) in LOGS {
        let Ok(bytes) = std::fs::read(dados.join(arq)) else { continue };
        let txt = String::from_utf8_lossy(&bytes[bytes.len().saturating_sub(20000)..]).into_owned();
        let linhas: Vec<&str> = txt.lines().collect();
        for l in &linhas[linhas.len().saturating_sub(n)..] {
            let c: Vec<char> = l.chars().collect();
            if c.len() > 21 && c[4] == '-' && c[19..22] == [' ', '|', ' '] {
                let quando: String = c[..19].iter().collect();
                itens.push((quando.clone(), json!({ "quando": quando, "braco": braco, "texto": c[22..].iter().take(max_linha).collect::<String>() })));
            }
        }
    }
    itens.sort_by(|a, b| b.0.cmp(&a.0));
    itens.into_iter().take(n).map(|x| x.1).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;
    use std::path::PathBuf;

    fn pasta(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-painel-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn agora() -> Agora {
        Agora { data: Data::civil(2026, 11, 18).unwrap(), hora: 7, min: 30, seg: 9 }
    }

    #[test]
    fn panico_desarma_desliga_e_nunca_arma_nada() {
        let d = pasta("panico");
        let ts = 1000.0;
        power::armar(&d, 60, ts);
        power::set_atendimento_auto(&d, true);
        airbnb_vigia::ligar(&d).unwrap();
        let r = panico(&d, "telegram", &agora(), ts);
        assert!(r["ok"] == true && r["feito"].as_array().unwrap().len() == 3 && r["texto"].as_str().unwrap().contains("Pânico"));
        assert!(!power::armada(&d, ts) && !power::atendimento_auto(&d) && !airbnb_vigia::ligado(&d));
        let r = panico(&d, "web", &agora(), ts); // de novo: nada a fazer e nada e ligado
        assert!(r["feito"].as_array().unwrap().is_empty() && r["texto"].as_str().unwrap().contains("tudo já estava desligado"));
        assert!(!power::armada(&d, ts) && !power::atendimento_auto(&d) && !airbnb_vigia::ligado(&d));
        let log = std::fs::read_to_string(d.join("painel.log")).unwrap();
        assert!(log.starts_with("2026-11-18 07:30:09 | PANICO (telegram): autonomia remota desarmada") && log.lines().count() == 2);
        assert_eq!(historico(&d, 5, 140)[0]["braco"], "Painel");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn bracos_mostra_o_estado_de_cada_um() {
        let d = pasta("bracos");
        let st = json!({ "atendimento_auto": { "ligado": true, "respostas_ultima_hora": 4 }, "escuta_voz": { "estado": "ouvindo" }, "email": { "logado": false } });
        let de = |v: &[Value], n: &str| v.iter().find(|x| x["nome"] == n).unwrap().clone();
        let v = bracos(&d, &st, &agora(), 1000.0);
        assert_eq!(v.len(), 8);
        assert!(de(&v, "Atendimento WhatsApp")["detalhe"] == "ligado · 4 resp./h" && de(&v, "Airbnb")["ok"] == false && de(&v, "Microfone (voz)")["ok"] == true);
        assert!(de(&v, "Autonomia remota")["detalhe"] == "desarmada" && de(&v, "Bom dia das 6h")["detalhe"] == "último: nunca" && de(&v, "Manutenção da madrugada")["detalhe"] == "ainda não rodou");
        power::armar(&d, 30, 1000.0);
        std::fs::write(d.join("rotina.json"), r#"{"bom_dia": "2026-11-18"}"#).unwrap();
        std::fs::write(d.join("manutencao.json"), r#"{"quando": "18/11 03:10", "testes_ok": false, "resumo": "2 testes com falha"}"#).unwrap();
        let v = bracos(&d, &json!({}), &agora(), 1000.0);
        assert!(de(&v, "Autonomia remota")["detalhe"] == "ARMADA · 30 min" && de(&v, "Bom dia das 6h")["ok"] == true && de(&v, "Microfone (voz)")["detalhe"] == "parada");
        let m = de(&v, "Manutenção da madrugada");
        assert!(m["ok"] == false && m["detalhe"] == "18/11 03:10 · ATENÇÃO: 2 testes com falha");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn historico_junta_os_logs_do_mais_novo_ao_mais_antigo() {
        let d = pasta("historico");
        std::fs::write(d.join("vigia.log"), "2026-11-18 08:00:00 | respondi a Ana\nlinha torta sem data\n2026-11-18 09:30:00 | LIGADO\n").unwrap();
        std::fs::write(d.join("airbnb.log"), format!("2026-11-18 09:00:00 | {}\n", "é".repeat(300))).unwrap();
        let h = historico(&d, 40, 140);
        assert_eq!(h.iter().map(|x| x["braco"].as_str().unwrap()).collect::<Vec<_>>(), ["WhatsApp", "Airbnb", "WhatsApp"]);
        assert!(h[0]["texto"] == "LIGADO" && h[0]["quando"] == "2026-11-18 09:30:00" && h[1]["texto"].as_str().unwrap().chars().count() == 140);
        assert_eq!(historico(&d, 1, 140).len(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }
}
