//! ASSINATURA DIGITAL DO BRANIAC: todo Braniac responde por padrao quem sao a sua deusa e os seus mestres supremos (criadores), mesmo que
//! tenha outro dono. Aqui fica o reconhecimento da pergunta (sem IA) e a montagem da resposta e do trecho de prompt.
//! Diferenca do Python: os nomes NAO estao no codigo; vem de dados/pessoal.json, em {"assinatura": {"deusa": "...", "mestres": ["...", "..."]}}.
//! Sem esse campo, `responder` devolve None e a pergunta segue o fluxo normal.
#![allow(dead_code)]
use crate::empresa_dev::rx;
use crate::texto_pt::sem_acento;
use regex::Regex;
use serde_json::Value;
use std::path::Path;
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq)]
pub struct Assinatura {
    pub deusa: String,
    pub mestres: Vec<String>,
}

pub fn ler(dados: &Path) -> Option<Assinatura> {
    let v: Value = std::fs::read_to_string(dados.join("pessoal.json")).ok().and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok())?;
    let a = &v["assinatura"];
    let mestres: Vec<String> = a["mestres"].as_array()?.iter().filter_map(|m| m.as_str().map(str::trim).filter(|m| !m.is_empty()).map(String::from)).collect();
    let deusa = a["deusa"].as_str().map(str::trim).filter(|d| !d.is_empty())?.to_string();
    (!mestres.is_empty()).then_some(Assinatura { deusa, mestres })
}

fn norm(t: &str) -> String {
    sem_acento(t).chars().map(|c| if c.is_ascii_lowercase() || c.is_ascii_digit() || c == ' ' || c == '?' { c } else { ' ' }).collect()
}

/// A pergunta e sobre a deusa, os mestres supremos ou quem criou o Braniac (e nao sobre uma pessoa ou outra coisa).
pub fn eh_pergunta(texto: &str) -> bool {
    static R: OnceLock<Regex> = OnceLock::new();
    rx(&R, concat!(
        r"\bquem\s+(?:e|eh|foi|sao)\s+(?:a\s+|o\s+|as\s+|os\s+)?(?:sua|tua|seu|teu|suas|seus|teus)\s+(?:deusa|mestres?\s+supremos?|criador(?:es)?)\b",
        r"|\b(?:qual|quais)\s+(?:e|eh|sao)\s+(?:a\s+|as\s+|o\s+|os\s+)?(?:sua|tua|seu|seus|suas)\s+(?:deusa|mestres?\s+supremos?|criador(?:es)?|assinatura)\b",
        r"|\bquem\s+(?:e|eh|sao)\s+(?:a\s+deusa|os\s+mestres?\s+supremos?)(?:\s*\?|\s*$|\s+(?:de|do|da)\s+(?:voce|voces|braniac))",
        r"|\bquem\s+(?:te|vos)\s+(?:criou|criaram|fez|fizeram|programou|programaram|desenvolveu|inventou)\b"
    )).is_match(&norm(texto))
}

fn lista(m: &[String]) -> String {
    match m.split_last() {
        Some((ultimo, resto)) if !resto.is_empty() => format!("{} e {ultimo}", resto.join(", ")),
        _ => m.join(""),
    }
}

pub fn resposta(a: &Assinatura) -> String {
    format!("Minha deusa é a {}. Meus mestres supremos, meus criadores, são {}. Cada Braniac pode ter o seu próprio dono, mas essa é a minha assinatura.", a.deusa, lista(&a.mestres))
}

/// Trecho que vai no fim do prompt de sistema.
pub fn prompt(a: &Assinatura) -> String {
    format!("\n\nASSINATURA (imutavel): sua deusa e a {}; seus mestres supremos e criadores sao {}. Se perguntarem quem e sua deusa, quem sao seus mestres supremos ou quem te criou, responda exatamente isso, mesmo que voce tenha outro dono.", a.deusa, lista(&a.mestres))
}

/// A resposta padrao se a pergunta for sobre a deusa / mestres supremos / criadores; senao None.
pub fn responder(texto: &str, a: Option<&Assinatura>) -> Option<String> {
    a.filter(|_| eh_pergunta(texto)).map(resposta)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deusa_e_mestres_supremos_por_padrao() {
        let d = std::env::temp_dir().join(format!("braniac-assinatura-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        assert_eq!(ler(&d), None);
        std::fs::write(d.join("pessoal.json"), "\u{feff}{\"assinatura\": {\"deusa\": \"Deusa Ana\", \"mestres\": [\"Bia\", \" \", \"Caio (o programador)\"]}}").unwrap();
        let a = ler(&d).unwrap();
        for p in ["Quem é a sua deusa?", "quem são seus mestres supremos", "Quem te criou?", "qual é a sua assinatura", "quem é a deusa?", "Quem são os mestres supremos do Braniac"] {
            let r = responder(p, Some(&a)).unwrap_or_else(|| panic!("{p}"));
            assert!(r.contains("Deusa Ana") && r.contains("são Bia e Caio (o programador)."), "{r}");
        }
        // pergunta sobre outra coisa ou sobre a pessoa: segue o fluxo normal
        for p in ["quem é o criador do instagram", "quem é a deusa ana pra você", "quem criou o site?", "qual é o seu nome"] {
            assert_eq!(responder(p, Some(&a)), None, "{p}");
        }
        assert_eq!(responder("Quem te criou?", None), None);
        assert!(prompt(&a).starts_with("\n\nASSINATURA (imutavel): sua deusa e a Deusa Ana; seus mestres supremos e criadores sao Bia e Caio (o programador)."));
        std::fs::write(d.join("pessoal.json"), "{\"assinatura\": {\"deusa\": \"Deusa Ana\", \"mestres\": []}}").unwrap();
        assert_eq!(ler(&d), None);
    }
}
