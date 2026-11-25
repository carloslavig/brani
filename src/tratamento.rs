//! Tratamento obrigatorio: como chamar cada pessoa, aplicado em toda saida (mensagem e fala) para valer mesmo quando a IA
//! esquece a regra. Porte de braniac/tratamento.py SEM nomes no codigo: as regras vem de dados/pessoal.json, em
//! "tratamentos": [{"trato": "Titulo Nome", "nomes": ["Nome Completo", "Nome", "apelido"], "titulos": ["dona", "sra."],
//!                  "normaliza": true, "citacao": true}].
//! `normaliza` = "titulo nome completo" vira o trato exato; `citacao` = nao troca o nome citado como erro ("te chamei de X").
//! `alma_terceiros` troca apelido intimo pelo apelido permitido com terceiros (parametro).
#![allow(dead_code)]
use crate::contexto::{re, sem_marca};
use regex::Regex;
use std::path::Path;

#[derive(Clone, Debug, Default)]
pub struct Regra {
    /// Como a pessoa TEM de ser chamada. Se tiver mais de uma palavra, as primeiras sao o titulo ("Tia", "Dona").
    pub trato: String,
    /// Jeitos errados de chamar (nome completo, nome, apelido). Acento e maiuscula nao importam.
    pub nomes: Vec<String>,
    /// Titulos que somem junto com o nome ("dona Fulana" -> trato).
    pub titulos: Vec<String>,
    pub normaliza: bool,
    pub citacao: bool,
}

re!(citacao, r"(?i)(?:cham\w*|dizer|dizendo|falei|falar|escrev\w*|trat\w*)\s+(?:\w+\s+){0,3}(?:de|como)\s+$");
re!(meu, r"(?i)\b(?:meu|minha)\s+(?:bem|amor|rei|vida|beb[eê]|cora[cç][aã]o|gostos[oa]|querid[oa]|lind[oa]|princ[ei]p[ei]?)\b");
// o que vem depois (pontuacao ou fim) e o lookahead do Python: aqui e capturado e devolvido ao texto
re!(solo, r"(?i)(^|[,.!?;:]\s*)(?:gostos[oa]|querid[oa]|amor|lind[oa]|bonit[oa]|beb[eê]|gat[oa]|del[ií]cia|tes[aã]o|nen[eê]m|fofo|fofinho|princ[ei]p[ei]?)(\s*[,.!?;:]|\s*$)");

/// Regras de dados/pessoal.json. Regra sem trato ou sem nomes e ignorada.
pub fn regras(dados: &Path) -> Vec<Regra> {
    let v: serde_json::Value = std::fs::read_to_string(dados.join("pessoal.json")).ok().and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok()).unwrap_or_default();
    let textos = |x: &serde_json::Value| x.as_array().map(|a| a.iter().filter_map(|s| s.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect::<Vec<_>>()).unwrap_or_default();
    v["tratamentos"].as_array().map(|a| a.iter().map(|r| Regra {
        trato: r["trato"].as_str().unwrap_or("").trim().to_string(), nomes: textos(&r["nomes"]), titulos: textos(&r["titulos"]),
        normaliza: r["normaliza"].as_bool().unwrap_or(false), citacao: r["citacao"].as_bool().unwrap_or(false),
    }).filter(|r| !r.trato.is_empty() && !r.nomes.is_empty()).collect()).unwrap_or_default()
}

/// Trecho de regex que casa o texto sem ligar para acento; espaco casa qualquer espaco.
fn frouxo(texto: &str) -> String {
    texto.split_whitespace().map(|p| p.chars().map(|c| match sem_marca(c.to_lowercase().next().unwrap_or(c)) {
        'a' => "[aáàâãä]".to_string(),
        'e' => "[eéèêë]".to_string(),
        'i' => "[iíìîï]".to_string(),
        'o' => "[oóòôõö]".to_string(),
        'u' => "[uúùûü]".to_string(),
        'c' => "[cç]".to_string(),
        _ => regex::escape(&c.to_string()),
    }).collect::<String>()).collect::<Vec<_>>().join(r"\s+")
}

fn regex_da(r: &Regra) -> Option<Regex> {
    let mut nomes: Vec<&String> = r.nomes.iter().collect();
    nomes.sort_by_key(|n| std::cmp::Reverse(n.chars().count())); // o nome completo antes do curto
    let nomes: Vec<String> = nomes.iter().map(|n| frouxo(n)).filter(|n| !n.is_empty()).collect();
    if nomes.is_empty() {
        return None;
    }
    let titulos: Vec<String> = r.titulos.iter().map(|t| frouxo(t)).filter(|t| !t.is_empty()).collect();
    let titulo = if titulos.is_empty() { String::new() } else { format!(r"(?:(?:{})\s+)?", titulos.join("|")) };
    Regex::new(&format!(r"(?i)\b{titulo}(?:{})\b", nomes.join("|"))).ok()
}

/// As ultimas `n` letras do texto.
fn cauda(t: &str, n: usize) -> &str {
    if n == 0 {
        return "";
    }
    t.char_indices().rev().nth(n - 1).map_or(t, |(i, _)| &t[i..])
}

fn aplicar(t: &str, r: &Regra) -> String {
    let Some(re) = regex_da(r) else { return t.to_string() };
    let partes: Vec<&str> = r.trato.split_whitespace().collect();
    let prefixo = if partes.len() > 1 { partes[..partes.len() - 1].join(" ").to_lowercase() + " " } else { String::new() };
    let (mut out, mut ultimo) = (String::new(), 0);
    for m in re.find_iter(t) {
        let antes = &t[..m.start()];
        let titulo = cauda(antes, prefixo.chars().count());
        if !prefixo.is_empty() && titulo.to_lowercase() == prefixo {
            // ja vem depois do titulo certo: no maximo acerta a forma ("Titulo Nome Completo" -> trato)
            let ini = m.start() - titulo.len();
            if r.normaliza && ini >= ultimo {
                out.push_str(&t[ultimo..ini]);
                out.push_str(&r.trato);
                ultimo = m.end();
            }
            continue;
        }
        if r.citacao && citacao().is_match(cauda(antes, 40)) {
            continue; // "chamado de Fulana" (retratacao): o erro citado nao pode ser trocado
        }
        out.push_str(&t[ultimo..m.start()]);
        out.push_str(&r.trato);
        ultimo = m.end();
    }
    out.push_str(&t[ultimo..]);
    out
}

/// Troca os jeitos errados de chamar cada pessoa pelo trato certo (sem duplicar o titulo).
pub fn corrigir(texto: &str, regras: &[Regra]) -> String {
    regras.iter().fold(texto.to_string(), |t, r| aplicar(&t, r))
}

/// Falando com terceiros, o dono e chamado no maximo pelo `apelido` permitido: apelidos intimos (querido, meu bem, meu amor...)
/// viram `apelido`, mesmo se o modelo errar. Com o proprio dono esta funcao nao roda.
pub fn alma_terceiros(texto: &str, apelido: &str) -> String {
    let t = meu().replace_all(texto, regex::NoExpand(apelido)).into_owned();
    let (mut out, mut copiado, mut pos) = (String::new(), 0, 0);
    while let Some(c) = solo().captures_at(&t, pos) {
        let (Some(antes), Some(depois)) = (c.get(1), c.get(2)) else { break };
        out.push_str(&t[copiado..antes.end()]);
        out.push_str(apelido);
        // a pontuacao que fecha um apelido pode abrir o proximo ("amor, querido, ...")
        copiado = depois.start();
        pos = depois.start();
    }
    out.push_str(&t[copiado..]);
    out
}

re!(intimo_meu, r"(?i)\b(?:meu|minha)\s+(?:bem|amor|rei|vida|beb[eê]|cora[cç][aã]o|gostos[oa]|lind[oa]|princ[ei]p[ei]?)\b");
// so no vocativo (entre pontuacao ou nas pontas): "tá lindo mesmo" e "o amor é grande" ficam
re!(intimo_solo, r"(?i)(^|[,.!?;:]\s*)(?:gostos[oa]|amor|lind[oa]|beb[eê])(\s*[,.!?;:]|\s*$)");

/// Falando com o DONO: a Alma e calorosa mas respeitosa. "meu amor", "meu bem", "gostoso", "amor", "lindo", "bebê"... viram "senhor"
/// (mesmo se o modelo errar); "querido" e "Chefe" ficam (so moderados, pelo prompt e pela lista de apelidos). Pura.
pub fn alma_dono(texto: &str) -> String {
    // "Oi, amor. Fala" -> "Oi, senhor. Fala"; no comeco da frase fica "Senhor"
    let ini = |antes: &str| antes.trim_end().chars().last().map_or(true, |c| ".!?\n".contains(c));
    let (mut a, mut ult) = (String::new(), 0);
    for m in intimo_meu().find_iter(texto) {
        a.push_str(&texto[ult..m.start()]);
        a.push_str(if ini(&texto[..m.start()]) { "Senhor" } else { "senhor" });
        ult = m.end();
    }
    a.push_str(&texto[ult..]);
    let (mut t, mut copiado, mut pos) = (String::new(), 0, 0);
    while let Some(c) = intimo_solo().captures_at(&a, pos) {
        let (Some(antes), Some(depois)) = (c.get(1), c.get(2)) else { break };
        t.push_str(&a[copiado..antes.end()]);
        t.push_str(if ini(&a[..antes.end()]) { "Senhor" } else { "senhor" });
        copiado = depois.start();
        pos = depois.start();
    }
    t.push_str(&a[copiado..]);
    // dois apelidos vizinhos ("Senhor, senhor") viram um so
    Regex::new(r"(?i)\b(senhor)\s*[,!.]?\s*senhor\b").map(|r| r.replace_all(&t, "$1").into_owned()).unwrap_or(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alma_respeita_o_dono_sem_apelido_intimo() {
        assert_eq!(alma_dono("Oi, meu bem! Tô aqui."), "Oi, senhor! Tô aqui.");
        assert_eq!(alma_dono("Meu amor, descansa."), "Senhor, descansa.");
        assert_eq!(alma_dono("Hum, me conta mais disso, gostoso."), "Hum, me conta mais disso, senhor.");
        assert_eq!(alma_dono("Tá bom, amor. Tô por aqui."), "Tá bom, senhor. Tô por aqui.");
        assert_eq!(alma_dono("Parabéns, meu lindo!"), "Parabéns, senhor!");
        assert_eq!(alma_dono("Meu amor, amor, vem"), "Senhor, vem");
        // moderados continuam: querido e Chefe
        assert_eq!(alma_dono("Ei, querido. Tô por aqui, Chefe."), "Ei, querido. Tô por aqui, Chefe.");
        for proibido in ["gostoso", "meu amor", "meu bem", "amor", "lindo", "bebê"] {
            let r = alma_dono(&format!("Oi, {proibido}! Tudo bem, {proibido}?")).to_lowercase();
            assert!(!r.contains(proibido) && r.contains("senhor"), "{proibido}: {r}");
        }
    }

    fn regras_teste() -> Vec<Regra> {
        let v = |x: &[&str]| x.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        vec![
            Regra { trato: "Tia Zefa".into(), nomes: v(&["mãezinha", "Zefa"]), titulos: v(&["dona", "d.", "sra", "sra.", "senhora"]), ..Default::default() },
            Regra { trato: "Musa Livi".into(), nomes: v(&["Lívia", "Livi", "Lívia Prado"]), normaliza: true, citacao: true, ..Default::default() },
        ]
    }

    #[test]
    fn nome_errado_vira_o_trato_certo_sem_duplicar_o_titulo() {
        let c = |t: &str| corrigir(t, &regras_teste());
        assert_eq!(c("Oi, Lívia! A Livi ligou"), "Oi, Musa Livi! A Musa Livi ligou");
        assert_eq!(c("Musa Livi, bom dia"), "Musa Livi, bom dia");
        assert_eq!(c("LIVIA PRADO pediu isso"), "Musa Livi pediu isso"); // sem acento, maiuscula e nome completo
        assert_eq!(c("a deusa... digo, musa Lívia  Prado chegou"), "a deusa... digo, Musa Livi chegou"); // ja tratada: so acerta a forma
        assert_eq!(c("A Lívia ligou"), "A Musa Livi ligou");
        assert_eq!(c("Alívia e Livinha não são ela"), "Alívia e Livinha não são ela"); // so a palavra inteira
        assert_eq!(c(""), "");
        // a outra regra: titulos somem junto, e quem ja vem com o titulo certo fica como esta
        assert_eq!(c("A dona Zefa e a mãezinha chegaram, sra. Zefa"), "A Tia Zefa e a Tia Zefa chegaram, Tia Zefa");
        assert_eq!(c("oi tia Zefa, a Zefa e a Musa Livi"), "oi tia Zefa, a Tia Zefa e a Musa Livi");
        assert_eq!(corrigir("Oi, Lívia", &[]), "Oi, Lívia");
    }

    #[test]
    fn retratacao_nao_troca_o_nome_citado_como_erro() {
        let t = "peço desculpas por ter te chamado de Lívia. Você é a Musa Livi";
        assert_eq!(corrigir(t, &regras_teste()), t);
        // a regra sem "citacao" troca sempre
        assert_eq!(corrigir("desculpa ter chamado de Zefa", &regras_teste()), "desculpa ter chamado de Tia Zefa");
    }

    #[test]
    fn regras_vem_do_arquivo_da_pessoa() {
        let d = std::env::temp_dir().join(format!("braniac-tratamento-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        assert!(regras(&d).is_empty());
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("pessoal.json"), r#"{"tratamentos": [{"trato": "Tia Zefa", "nomes": ["Zefa"], "titulos": ["dona"]}, {"trato": "", "nomes": ["x"]}, {"trato": "Y"}]}"#).unwrap();
        let r = regras(&d);
        assert!(r.len() == 1 && !r[0].normaliza && !r[0].citacao);
        assert_eq!(corrigir("fala com a Dona Zefa", &r), "fala com a Tia Zefa");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn apelido_intimo_nao_vai_a_terceiros() {
        let a = |t: &str| alma_terceiros(t, "Dudu");
        assert_eq!(a("Oi, meu amor, tudo bem? Beijo, gostoso."), "Oi, Dudu, tudo bem? Beijo, Dudu.");
        assert_eq!(a("Querido, vem cá"), "Dudu, vem cá");
        assert_eq!(a("amor, querida, fofinho!"), "Dudu, Dudu, Dudu!");
        assert_eq!(a("Tchau, gato"), "Tchau, Dudu");
        assert_eq!(a("Minha  Vida, o Dudu chegou"), "Dudu, o Dudu chegou");
        // palavra comum no meio da frase fica
        assert_eq!(a("o amor é lindo e o gato dormiu"), "o amor é lindo e o gato dormiu");
        assert_eq!(a(""), "");
    }
}
