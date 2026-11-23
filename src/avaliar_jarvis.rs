//! Avaliacao de eficacia do Jarvis: manda perguntas tipicas (so de LEITURA) e confere se a resposta tem o esperado e nao tem o proibido.
//! Os erros que o dono marcou ("errou") viram caso: passa quando a resposta NAO repete a resposta errada de antes. So pedido de CONSULTA
//! roda sozinho; pedido com acao (efeito colateral) fica para revisao manual.
//! Ficou de fora: a chamada HTTP ao Jarvis (closure `perguntar`) e a regra "isto e consulta?" (closure; no Python e delegacao.eh_consulta).
//! Diferenca do Python: os casos com nome, endereco ou valor de pessoa real nao estao no codigo; vem de dados/avaliacao_casos.json
//! ([[nome, mensagem, regex que deve ter, regex que nao pode ter], ...]). Aqui ficam so os gerais.
#![allow(dead_code)]
use crate::empresa_dev::{corta, ler};
use regex::Regex;
use serde_json::Value;
use std::path::Path;

/// Vai na frente de toda pergunta: quem observa a conversa (Alma) ignora mensagem de avaliacao.
pub const PREFIXO: &str = "[AVALIACAO-AUTOMATICA] ";

#[derive(Clone, Debug, PartialEq)]
pub struct Caso {
    pub nome: String,
    pub msg: String,
    pub deve: String,
    pub nao: String,
}

fn caso(nome: &str, msg: &str, deve: &str, nao: &str) -> Caso {
    Caso { nome: nome.into(), msg: msg.into(), deve: deve.into(), nao: nao.into() }
}

/// Casos gerais + os da pessoa (dados/avaliacao_casos.json).
pub fn casos(dados: &Path) -> Vec<Caso> {
    let mut v = vec![
        caso("lembrete-vago", "Me lembre amanhã", r"(?i)de que|o que|qual|quê|que horas|hor[aá]rio|sobre", r"anotado|09:00"),
        caso("geral", "me explica em 3 linhas o que é um Pix copia e cola", r".{80,}", r"RESULTADO DE FERRAMENTA"),
    ];
    for c in ler(&dados.join("avaliacao_casos.json")).as_ref().and_then(Value::as_array).into_iter().flatten() {
        if let [n, m, d, x] = c.as_array().map(Vec::as_slice).unwrap_or(&[]) {
            if let (Some(n), Some(m), Some(d)) = (n.as_str(), m.as_str(), d.as_str()) {
                v.push(caso(n, m, d, x.as_str().unwrap_or("")));
            }
        }
    }
    v
}

/// (automaticos, manuais) a partir de erros_jarvis.jsonl (as 60 ultimas linhas).
pub fn casos_dos_erros(arquivo: &Path, eh_consulta: &dyn Fn(&str) -> bool) -> (Vec<Caso>, Vec<Caso>) {
    let (mut auto, mut manual) = (Vec::new(), Vec::new());
    let txt = std::fs::read_to_string(arquivo).unwrap_or_default();
    let linhas: Vec<&str> = txt.lines().collect();
    for l in &linhas[linhas.len().saturating_sub(60)..] {
        let Ok(e) = serde_json::from_str::<Value>(l) else { continue };
        let (ped, ruim) = (e["pedido"].as_str().unwrap_or("").trim(), e["resposta_errada"].as_str().unwrap_or("").trim());
        if ped.is_empty() || ruim.is_empty() {
            continue;
        }
        let trecho = corta(&ruim.split_whitespace().collect::<Vec<_>>().join(" "), 70);
        let c = caso(&format!("erro-{}", e["ts"].as_f64().unwrap_or(0.0) as i64), ped, r".{10,}", &regex::escape(&trecho).replace(' ', r"\s+"));
        if eh_consulta(ped) { auto.push(c) } else { manual.push(c) }
    }
    (auto, manual)
}

/// [(nome, ok, motivo, resposta)]; ok = false se faltou o esperado, apareceu o proibido ou a pergunta falhou.
pub fn avaliar(perguntar: &dyn Fn(&str, &str) -> Result<String, String>, casos: &[Caso], ts: f64) -> Vec<(String, bool, String, String)> {
    casos.iter().map(|c| {
        let r = match perguntar(&format!("{PREFIXO}{}", c.msg), &format!("aval-{}-{}", c.nome, ts as i64)) {
            Ok(r) => r,
            Err(e) => return (c.nome.clone(), false, format!("erro: {}", corta(&e, 80)), String::new()),
        };
        let (deve, nao) = (Regex::new(&format!("(?s){}", c.deve)), Regex::new(&format!("(?sm){}", c.nao)));
        let motivo = match (deve, nao) {
            (Err(_), _) | (_, Err(_)) => "regex do caso inválida",
            (Ok(d), _) if !d.is_match(&r) => "faltou o esperado",
            (_, Ok(n)) if !c.nao.is_empty() && n.is_match(&r) => "apareceu algo proibido",
            _ => "",
        };
        (c.nome.clone(), motivo.is_empty(), motivo.to_string(), r)
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pega_eco_aceita_resposta_boa_e_erro_de_rede_reprova() {
        let eco = [caso("eco", "x", r".{20,}", r"^Fulano disse em")];
        assert!(!avaliar(&|_, _| Ok("ok.\nFulano disse em 27/10 12:50 (familia): vou mandar".into()), &eco, 0.0)[0].1);
        let visto = std::cell::RefCell::new(String::new());
        let bom = avaliar(&|m, c| { *visto.borrow_mut() = format!("{m}|{c}"); Ok("O site é https://exemplo.test e funciona bem.".into()) }, &[caso("site", "qual o site?", r"exemplo\.test", r"Fulano disse em")], 99.0);
        assert!(bom[0].1 && visto.borrow().as_str() == "[AVALIACAO-AUTOMATICA] qual o site?|aval-site-99");
        assert_eq!(avaliar(&|_, _| Ok("curta".into()), &eco, 0.0)[0].2, "faltou o esperado");
        assert!(avaliar(&|_, _| Err("fora do ar".into()), &eco, 0.0)[0].2.starts_with("erro: fora"));
        assert!(!avaliar(&|_, _| Ok("qualquer coisa bem comprida aqui".into()), &[caso("r", "x", "(", "")], 0.0)[0].1);
    }

    #[test]
    fn erro_marcado_vira_caso_e_acao_fica_para_revisao_manual() {
        let d = std::env::temp_dir().join(format!("braniac-avaliar_jarvis-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let arq = d.join("erros_jarvis.jsonl");
        std::fs::write(&arq, "{\"ts\": 10.5, \"pedido\": \"como está a rifa?\", \"resposta_errada\": \"Tocando  rifa (ao vivo) no som.\"}\nlixo\n{\"ts\": 11, \"pedido\": \"manda mensagem pra Bia dizendo oi\", \"resposta_errada\": \"Mandei.\"}\n{\"ts\": 12, \"pedido\": \"\", \"resposta_errada\": \"x\"}\n").unwrap();
        let (auto, manual) = casos_dos_erros(&arq, &|p| p.ends_with('?'));
        assert!(auto.len() == 1 && manual.len() == 1 && auto[0].nome == "erro-10" && manual[0].msg.starts_with("manda")); // acao nunca roda sozinha
        assert!(avaliar(&|_, _| Ok("A rifa está em 40 números vendidos.".into()), &auto, 0.0)[0].1);
        let ruim = avaliar(&|_, _| Ok("Tocando rifa\n(ao vivo) no som.".into()), &auto, 0.0);
        assert!(!ruim[0].1 && ruim[0].2 == "apareceu algo proibido"); // repetir a resposta errada reprova
        assert_eq!(casos_dos_erros(&d.join("nao-existe"), &|_| true), (vec![], vec![]));
        std::fs::write(d.join("avaliacao_casos.json"), r#"[["meu", "pergunta?", "sim", ""], ["torto"]]"#).unwrap();
        let c = casos(&d);
        assert!(c.len() == 3 && c[2].nome == "meu" && c.iter().all(|x| Regex::new(&x.deve).is_ok() && Regex::new(&x.nao).is_ok()));
        let _ = std::fs::remove_dir_all(&d);
    }
}
