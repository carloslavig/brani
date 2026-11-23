//! Modo profundo (braniac/profundo.py): em vez de responder de primeira, PLANEJA, executa cada etapa, CONFERE o resultado contra o
//! objetivo, corrige o que faltar e so entao entrega. Nunca inventa: etapa sem evidencia vira "nao confirmei".
//! A IA (`perguntar`, sem ferramentas), o agente com ferramentas, os avisos e a entrega entram como closure.
//! De fora: a thread de segundo plano (aqui fica so a trava `Ativo`: um trabalho profundo por vez) e o envio do audio no WhatsApp.
#![allow(dead_code)]
use regex::Regex;
use serde_json::Value;
use std::sync::Mutex;

pub const MAX_ETAPAS: usize = 6;
pub const RODADAS_CONFERENCIA: usize = 2;
pub const COMECEI: &str = "Comecei a trabalhar nisso a fundo. Pode demorar; te aviso e mando o resultado em audio no WhatsApp quando terminar.";

fn ini(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn fim(s: &str, n: usize) -> String {
    let t = s.chars().count();
    s.chars().skip(t.saturating_sub(n)).collect()
}

fn vdd(v: &Value) -> bool {
    !matches!(v, Value::Null | Value::Bool(false)) && v.as_f64() != Some(0.0) && v.as_str() != Some("") && !v.as_array().is_some_and(|a| a.is_empty()) && !v.as_object().is_some_and(|o| o.is_empty())
}

/// O primeiro objeto JSON dentro do texto da IA (do primeiro `{` ao ultimo `}`); ilegivel = objeto vazio.
fn json_de(txt: &str) -> Value {
    let (Some(a), Some(b)) = (txt.find('{'), txt.rfind('}')) else { return Value::Object(Default::default()) };
    if b < a {
        return Value::Object(Default::default());
    }
    let cru = &txt[a..=b];
    // a IA quebra linha dentro de texto: o Python aceitava (strict=False)
    serde_json::from_str::<Value>(cru)
        .or_else(|_| serde_json::from_str::<Value>(&cru.replace(|c: char| c.is_control(), " ")))
        .ok()
        .filter(Value::is_object)
        .unwrap_or_else(|| Value::Object(Default::default()))
}

/// (etapas, pronto_quando). Plano ilegivel vira uma etapa so: o proprio objetivo.
pub fn planejar(perguntar: &dyn Fn(&str) -> String, objetivo: &str) -> (Vec<String>, String) {
    let d = json_de(&perguntar(&format!(
        "Planeje como cumprir o OBJETIVO do dono do JEITO MAIS COMPLETO E CONFIAVEL, sem pressa. Responda SO um JSON: \
         {{\"etapas\":[\"etapa 1 concreta\",\"etapa 2\"],\"pronto_quando\":\"como saber que esta bem feito\"}} com no maximo {MAX_ETAPAS} etapas, cada uma uma acao que \
         da para fazer com ferramentas (pesquisar na web, ler paginas, consultar IAs/conselho, procurar arquivos, usar o notebook, consultar leis/banco). \
         Descreva cada etapa pelo QUE descobrir/fazer (nao cite nomes de ferramentas) e NUNCA use o Claude. Evite etapas que dependam so de opiniao sua: busque evidencia.\nOBJETIVO: {objetivo}"
    )));
    let etapas: Vec<String> = d["etapas"]
        .as_array()
        .map(|e| e.iter().map(|x| x.as_str().map(str::to_string).unwrap_or_else(|| x.to_string())).filter(|x| !x.trim().is_empty()).take(MAX_ETAPAS).collect())
        .unwrap_or_default();
    let pronto = d["pronto_quando"].as_str().filter(|p| !p.is_empty()).unwrap_or("o objetivo foi atendido com evidencias").to_string();
    (if etapas.is_empty() { vec![objetivo.to_string()] } else { etapas }, pronto)
}

/// `agente(texto)` = uma etapa com ferramentas; `perguntar(texto)` = IA sem ferramentas. Devolve o texto final (ja entregue por `entregar`).
/// Etapa que falha vira nota "(etapa falhou)" e segue; falha na correcao para o trabalho (Err).
pub fn executar(objetivo: &str, agente: &dyn Fn(&str) -> Result<String, String>, perguntar: &dyn Fn(&str) -> String, avisar: &dyn Fn(&str), entregar: &dyn Fn(&str)) -> Result<String, String> {
    let t0 = std::time::Instant::now();
    let (etapas, pronto) = planejar(perguntar, objetivo);
    let n = etapas.len();
    avisar(&format!("🧭 Modo profundo: {n} etapa(s): {}", etapas.iter().map(|e| ini(e, 50)).collect::<Vec<_>>().join(" → ")));
    let mut notas: Vec<String> = vec![];
    for (i, e) in etapas.iter().enumerate() {
        let apurado = fim(&notas.join("\n"), 3000);
        let r = agente(&format!(
            "OBJETIVO GERAL: {objetivo}\nETAPA {}/{n}: {e}\nJA APURADO:\n{}\nFaca SO esta etapa, use as ferramentas, e responda com os FATOS achados (e o que nao conseguiu confirmar).",
            i + 1,
            if apurado.is_empty() { "nada ainda" } else { &apurado }
        ))
        .unwrap_or_else(|ex| format!("(etapa falhou: {})", ini(&ex, 100)));
        notas.push(format!("[{}] {e} -> {}", i + 1, ini(&r, 1200)));
        avisar(&format!("✔ etapa {}/{n} feita", i + 1));
    }
    let mut final_ = perguntar(&format!(
        "OBJETIVO: {objetivo}\nPRONTO QUANDO: {pronto}\nRESULTADOS DAS ETAPAS:\n{}\n\nMonte a MELHOR resposta final para o dono, em portugues simples, curta e direta, so com o que as etapas confirmaram (o que nao foi confirmado, diga).",
        notas.join("\n")
    ));
    for _ in 0..RODADAS_CONFERENCIA {
        let v = json_de(&perguntar(&format!(
            "Confira com rigor se a RESPOSTA atende o OBJETIVO e se nao tem nada inventado. Responda SO JSON: {{\"ok\":true,\"falta\":\"o que falta ou esta sem evidencia\"}}.\nOBJETIVO: {objetivo}\nRESPOSTA:\n{final_}\nEVIDENCIAS:\n{}",
            fim(&notas.join("\n"), 6000)
        )));
        if v.get("ok").map(vdd).unwrap_or(true) || !vdd(&v["falta"]) {
            break;
        }
        let falta = v["falta"].as_str().map(str::to_string).unwrap_or_else(|| v["falta"].to_string());
        avisar(&format!("🔁 corrigindo: {}", ini(&falta, 100)));
        let extra = agente(&format!("OBJETIVO GERAL: {objetivo}\nCORRIJA/COMPLETE: {falta}\nJA APURADO:\n{}", fim(&notas.join("\n"), 3000)))?;
        notas.push(format!("[correcao] {}", ini(&extra, 1200)));
        final_ = perguntar(&format!("OBJETIVO: {objetivo}\nEVIDENCIAS:\n{}\nRefaca a melhor resposta final, curta, so com fatos confirmados.", notas.join("\n")));
    }
    let mut final_ = final_.trim().to_string();
    if final_.is_empty() {
        final_ = "Nao consegui concluir com confianca.".into();
    }
    final_ += &format!("\n\n(feito no modo profundo, {} min)", (t0.elapsed().as_secs() / 60).max(1));
    entregar(&final_);
    Ok(final_)
}

/// Um trabalho profundo por vez.
#[derive(Default)]
pub struct Ativo(Mutex<Option<String>>);

impl Ativo {
    /// Ok = pode comecar (ja ficou marcado); Err = a frase para quem pediu outro no meio.
    pub fn comecar(&self, objetivo: &str) -> Result<(), String> {
        let mut a = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(atual) = a.as_ref() {
            return Err(format!("Ja estou num trabalho profundo: {}. Aguarde terminar.", ini(atual, 80)));
        }
        *a = Some(objetivo.to_string());
        Ok(())
    }

    pub fn terminar(&self) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

/// Roda dentro da trava: quem chama poe isto numa thread depois de `ativo.comecar` dar Ok. A trava sempre solta no fim.
pub fn rodar_marcado(ativo: &Ativo, objetivo: &str, agente: &dyn Fn(&str) -> Result<String, String>, perguntar: &dyn Fn(&str) -> String, avisar: &dyn Fn(&str), entregar: &dyn Fn(&str)) {
    if let Err(e) = executar(objetivo, agente, perguntar, avisar, entregar) {
        avisar(&format!("⚠️ Modo profundo parou: {}", ini(&e, 150)));
    }
    ativo.terminar();
}

/// O texto final sem o rodape "(feito no modo profundo, N min)", para virar audio.
pub fn texto_para_audio(texto: &str) -> String {
    Regex::new(r"\(feito no modo profundo.*?\)").expect("regex fixa").replace_all(texto, "").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn planeja_executa_confere_e_corrige() {
        let (chamadas, avisos, entregue) = (RefCell::new(Vec::<String>::new()), RefCell::new(vec![]), RefCell::new(vec![]));
        let perguntar = |t: &str| -> String {
            if t.starts_with("Planeje") {
                "claro:\n{\"etapas\":[\"achar A\",\"achar B\"],\"pronto_quando\":\"A e B\"}".into()
            } else if t.starts_with("Confira") {
                if chamadas.borrow().iter().any(|c| c.contains("CORRIJA")) { "{\"ok\": true}".into() } else { "{\"ok\": false, \"falta\": \"faltou C\"}".into() }
            } else {
                "RESPOSTA FINAL".into()
            }
        };
        let agente = |t: &str| -> Result<String, String> {
            chamadas.borrow_mut().push(t.to_string());
            Ok("fato".into())
        };
        let out = executar("descobrir tudo", &agente, &perguntar, &|a| avisos.borrow_mut().push(a.to_string()), &|e| entregue.borrow_mut().push(e.to_string())).unwrap();
        assert!(out.starts_with("RESPOSTA FINAL") && out.ends_with("(feito no modo profundo, 1 min)") && *entregue.borrow() == [out.clone()]);
        assert_eq!(chamadas.borrow().iter().filter(|c| c.contains("ETAPA")).count(), 2);
        assert_eq!(chamadas.borrow().iter().filter(|c| c.contains("CORRIJA/COMPLETE: faltou C")).count(), 1);
        assert!(chamadas.borrow()[0].contains("JA APURADO:\nnada ainda") && chamadas.borrow()[1].contains("[1] achar A -> fato"));
        assert!(avisos.borrow().iter().any(|a| a.contains("corrigindo")) && avisos.borrow()[0].contains("2 etapa(s): achar A → achar B"));
        assert_eq!(texto_para_audio(&out), "RESPOSTA FINAL\n\n");
    }

    #[test]
    fn plano_invalido_vira_uma_etapa_e_etapa_que_falha_nao_derruba() {
        let notas = RefCell::new(String::new());
        let perguntar = |t: &str| -> String {
            if t.starts_with("OBJETIVO") {
                *notas.borrow_mut() = t.to_string();
            }
            "nada json".into()
        };
        let out = executar("x", &|_| Err("caiu".into()), &perguntar, &|_| {}, &|_| {}).unwrap();
        assert!(out.starts_with("nada json"));
        assert!(notas.borrow().contains("[1] x -> (etapa falhou: caiu)"));
        assert_eq!(executar("x", &|_| Ok(String::new()), &|_| String::new(), &|_| {}, &|_| {}).unwrap(), "Nao consegui concluir com confianca.\n\n(feito no modo profundo, 1 min)");
    }

    #[test]
    fn plano_tem_teto_de_etapas_e_confere_no_maximo_duas_vezes() {
        let muitas = format!("{{\"etapas\":[{}, \"  \", 7],\"pronto_quando\":\"\"}}", (1..=9).map(|i| format!("\"e{i}\"")).collect::<Vec<_>>().join(","));
        let (etapas, pronto) = planejar(&|_| muitas.clone(), "obj");
        assert_eq!((etapas.len(), pronto.as_str()), (MAX_ETAPAS, "o objetivo foi atendido com evidencias"));
        assert_eq!(planejar(&|_| "{\"etapas\": [\"linha\nquebrada\"]}".into(), "obj").0, ["linha quebrada"]);
        let conferencias = RefCell::new(0);
        let perguntar = |t: &str| -> String {
            if t.starts_with("Confira") {
                *conferencias.borrow_mut() += 1;
                return "{\"ok\": false, \"falta\": \"sempre falta\"}".into();
            }
            "r".into()
        };
        executar("x", &|_| Ok("f".into()), &perguntar, &|_| {}, &|_| {}).unwrap();
        assert_eq!(*conferencias.borrow(), RODADAS_CONFERENCIA);
    }

    #[test]
    fn um_trabalho_profundo_por_vez_e_falha_na_correcao_solta_a_trava() {
        let ativo = Ativo::default();
        assert!(ativo.comecar("levantar os precos").is_ok());
        assert_eq!(ativo.comecar("outro").unwrap_err(), "Ja estou num trabalho profundo: levantar os precos. Aguarde terminar.");
        let avisos = RefCell::new(vec![]);
        let vez = RefCell::new(0);
        let agente = |_: &str| -> Result<String, String> {
            *vez.borrow_mut() += 1;
            if *vez.borrow() > 1 { Err("sem rede".into()) } else { Ok("f".into()) }
        };
        let perguntar = |t: &str| -> String { if t.starts_with("Confira") { "{\"ok\": false, \"falta\": \"y\"}".into() } else { "r".into() } };
        rodar_marcado(&ativo, "levantar os precos", &agente, &perguntar, &|a| avisos.borrow_mut().push(a.to_string()), &|_| panic!("nao entrega trabalho que parou"));
        assert_eq!(avisos.borrow().last().map(String::as_str), Some("⚠️ Modo profundo parou: sem rede"));
        assert!(ativo.comecar("outro").is_ok());
    }
}
