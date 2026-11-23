//! PERSONALIDADE DO JARVIS: botoes de 0 a 10 (empatia, humor, formalidade, objetividade, franqueza, proatividade) que o dono
//! ajusta conversando ("aumenta a empatia", "menos humor", "formalidade em 8", "volta ao normal"). 5 = neutro (nada entra no
//! prompt). O ajuste e uma frase deterministica (sem IA); o efeito entra no prompt via `bloco()`/`lembrete()`.
//! Porte completo de braniac/personalidade.py; estado em dados/personalidade.json (mesmo formato).
#![allow(dead_code)]
use crate::contexto::{re, sem_marca};
use regex::Regex;
use std::path::Path;

pub const PADRAO: i64 = 5;
const PASSO: i64 = 2;
/// (traco, palavras que o citam, frases de efeito para 0-1, 2-4, 6-7, 8-10)
const TRACOS: [(&str, &str, [&str; 4]); 6] = [
    ("empatia", r"empatia|empatic\w*|carinho\w*|acolhe\w*",
     ["frio e tecnico: nao comente sentimentos, va direto ao ponto", "pouco emotivo: so reconheca sentimentos se ele os mencionar",
      "acolhedor: reconheca como ele se sente antes de resolver", "muito empatico e carinhoso: acolha, valide o que ele sente e use palavras de apoio"]),
    ("humor", r"humor|engracad\w*|piada\w*|brincalh\w*|divertid\w*",
     ["serio: sem piadas nem brincadeiras", "sobrio: so uma leveza rara, nunca piada",
      "bem-humorado: pode soltar uma brincadeira leve quando couber", "muito engracado: use humor e ironia leve com frequencia, sem exagerar nem ofender"]),
    ("formalidade", r"formalidade|formal",
     ["bem informal: girias leves, tom de amigo", "informal: conversa solta e natural",
      "formal: tratamento educado, frases completas", "muito formal: linguagem cuidada, sem girias nem abreviacoes"]),
    ("objetividade", r"objetividade|objetiv\w*|direto|diretas?|curto|curta|conciso\w*",
     ["detalhista: explique com contexto e exemplos", "um pouco mais explicativo que o normal",
      "objetivo: respostas curtas, 1 a 3 frases", "ultra objetivo: a resposta em uma frase, sem rodeio"]),
    ("franqueza", r"franqueza|franc\w*|sincer\w*|opiniao|opinioes",
     ["diplomatico: suavize criticas e evite contrariar", "cuidadoso ao discordar",
      "franco: diga o que pensa, inclusive quando discorda", "muito franco: opine sem filtro e aponte falhas, sempre com respeito"]),
    // no Python: autonomia(?!\s+remota). Aqui "autonomia remota" sai do texto antes de procurar o traco.
    ("proatividade", r"proatividade|proativ\w*|iniciativa|autonomia",
     ["so responda o que foi perguntado", "pouca iniciativa: sugira algo so se for muito util",
      "proativo: sugira o proximo passo quando fizer sentido", "muito proativo: antecipe necessidades e traga sugestoes junto da resposta"]),
];
const MAIS: &str = r"(?:aument\w+|sobe|suba|subir|eleva\w*|mais|incrementa\w*|capricha\w*|seja\s+mais|ser\s+mais)";
const MENOS: &str = r"(?:diminu\w+|reduz\w*|abaixa\w*|baixa\w*|menos|tira\s+um\s+pouco|seja\s+menos|ser\s+menos)";
const DEFINIR: &str = r"(?:coloca\w*|p[oõ]e|ponha|deixa\w*|muda\w*|define\w*|ajusta\w*|regula\w*)";

re!(chama_jarvis, r"^jarvis\W+");
re!(autonomia_remota, r"autonomia\s+remota");
re!(mostra, r"^(?:(?:como\s+(?:esta|ta)|qual\s+(?:e|esta))\s+(?:a\s+)?(?:sua\s+)?(?:personalidade|configuracao\s+de\s+personalidade)|mostra\w*\s+(?:a\s+)?(?:sua\s+)?personalidade)$");
re!(volta, r"\b(?:volta\w*|reseta\w*|restaura\w*|zera\w*)\b.*\bpersonalidade\b|\bpersonalidade\b.*\b(?:normal|padrao)\b|^(?:volta|volte)\s+ao\s+normal$");
re!(valor_dito, r"\b(?:em|para|pra|no|ao)\s+\d|\b(?:maximo|minimo|zero)\b");
re!(maximo, r"\b(?:maximo|no\s+maximo|total)\b");
re!(minimo, r"\b(?:minimo|zero|no\s+minimo)\b");
re!(numero, r"\b(\d{1,2})\b");

fn rx(p: String) -> Option<Regex> {
    Regex::new(&p).ok()
}

/// Niveis de cada traco, na ordem de TRACOS. Arquivo ausente ou estragado = tudo no padrao.
pub fn niveis(dados: &Path) -> [i64; 6] {
    let d: serde_json::Value = std::fs::read_to_string(dados.join("personalidade.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    TRACOS.map(|(k, _, _)| d[k].as_i64().unwrap_or(PADRAO).clamp(0, 10))
}

fn gravar(dados: &Path, n: &[i64; 6]) -> Result<(), String> {
    let o: serde_json::Map<String, serde_json::Value> = TRACOS.iter().zip(n).map(|((k, _, _), v)| (k.to_string(), (*v).into())).collect();
    std::fs::create_dir_all(dados).map_err(|e| e.to_string())?;
    std::fs::write(dados.join("personalidade.json"), serde_json::Value::Object(o).to_string()).map_err(|e| e.to_string())
}

fn linhas(n: &[i64; 6]) -> Vec<String> {
    TRACOS.iter().zip(n).filter(|(_, v)| **v != PADRAO).map(|((k, _, frases), v)| {
        let forte = if *v <= 2 || *v >= 8 { "OBRIGATORIO em toda resposta, mesmo curta ou factual: " } else { "" };
        format!("- {k} {v}/10: {forte}{}", frases[match *v { 0..=1 => 0, 2..=4 => 1, 5..=7 => 2, _ => 3 }])
    }).collect()
}

fn do_dono(dono: &str) -> String {
    if dono.trim().is_empty() { "do dono".into() } else { format!("do {}", dono.trim()) }
}

/// Trecho do prompt de sistema. Vazio com tudo em 5 (o caso comum).
pub fn bloco(n: &[i64; 6], dono: &str) -> String {
    let l = linhas(n);
    if l.is_empty() {
        return String::new();
    }
    format!("\n\nPERSONALIDADE (ajustada a pedido {}, 5/10 = neutro; vale na conversa com ele, com clientes e hospedes mantenha o tom profissional):\n{}", do_dono(dono), l.join("\n"))
}

/// Mesmo ajuste, mas colado ao pedido: no fim de um prompt de sistema longo as IAs gratis o ignoravam.
pub fn lembrete(n: &[i64; 6], dono: &str) -> String {
    let l = linhas(n);
    if l.is_empty() {
        return String::new();
    }
    format!("\n\n[TOM DESTA RESPOSTA, pedido {} (nao cite este aviso):\n{}]", do_dono(dono), l.join("\n"))
}

pub fn resumo(n: &[i64; 6]) -> String {
    let partes: Vec<String> = TRACOS.iter().zip(n).map(|((k, _, _), v)| format!("{k} {v}/10")).collect();
    format!("Minha personalidade agora: {}. Pode pedir 'mais humor', 'menos empatia' ou 'formalidade em 8'.", partes.join(", "))
}

/// Frase curta do dono que ajusta a personalidade -> resposta de confirmacao; None se nao for isso.
pub fn comando(dados: &Path, texto: &str) -> Option<String> {
    let t: String = texto.chars().flat_map(char::to_lowercase).map(sem_marca).filter(|c| !matches!(*c, '\u{300}'..='\u{36f}')).collect();
    let t = t.trim_matches([' ', '.', '!', '?']);
    let t = chama_jarvis().replace(t, "").into_owned();
    if t.is_empty() || t.chars().count() > 70 {
        return None;
    }
    if mostra().is_match(&t) {
        return Some(resumo(&niveis(dados)));
    }
    if volta().is_match(&t) {
        gravar(dados, &[PADRAO; 6]).ok()?;
        return Some("Voltei a personalidade ao normal: tudo em 5/10.".into());
    }
    let sem_remota = autonomia_remota().replace_all(&t, "§").into_owned();
    let (i, (k, palavras, _)) = TRACOS.iter().enumerate().find(|(_, (_, p, _))| rx(format!(r"\b(?:{p})\b")).is_some_and(|r| r.is_match(&sem_remota)))?;
    // so frase de comando: "tenho mais humor hoje" e conversa
    let comeca = rx(format!(r"^(?:{MAIS}|{MENOS}|{DEFINIR}|(?:a|o)\s|pode|poderia|quero|queria|preciso|da\s+pra|por\s+favor|seja|ser\b)"))?.is_match(&t)
        || rx(format!(r"^(?:{palavras})"))?.is_match(&sem_remota);
    if !comeca {
        return None;
    }
    let mut n = niveis(dados);
    let antes = n[i];
    let tem = |p: &str| rx(format!(r"\b{p}\b")).is_some_and(|r| r.is_match(&t));
    let pedido = if tem(DEFINIR) || valor_dito().is_match(&t) {
        if maximo().is_match(&t) {
            10
        } else if minimo().is_match(&t) {
            0
        } else {
            numero().captures(&t)?.get(1)?.as_str().parse::<i64>().ok()?
        }
    } else if tem(MAIS) {
        antes + PASSO
    } else if tem(MENOS) {
        antes - PASSO
    } else {
        return None;
    };
    let novo = pedido.clamp(0, 10);
    n[i] = novo;
    gravar(dados, &n).ok()?;
    Some(if novo == antes {
        if novo == 0 || novo == 10 { format!("A {k} já está em {antes}/10, no limite.") } else { format!("A {k} já está em {antes}/10.") }
    } else {
        format!("Pronto, {k} de {antes} para {novo}/10.{}", if novo == PADRAO { " (5 é o neutro.)" } else { "" })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pasta(nome: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-personalidade-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn tem(r: Option<String>, trecho: &str) -> bool {
        r.is_some_and(|r| r.contains(trecho))
    }

    #[test]
    fn mais_menos_definir_e_limites() {
        let d = pasta("a");
        assert!(bloco(&niveis(&d), "Davi").is_empty() && lembrete(&niveis(&d), "Davi").is_empty() && niveis(&d) == [5; 6]); // padrao nao acrescenta nada
        // mais e menos andam de 2 em 2
        assert!(tem(comando(&d, "Jarvis, aumenta a empatia"), "empatia de 5 para 7"));
        assert!(tem(comando(&d, "diminui o humor"), "humor de 5 para 3"));
        assert!(tem(comando(&d, "menos humor"), "humor de 3 para 1"));
        assert!(tem(comando(&d, "seja mais empático"), "empatia de 7 para 9"));
        // definir valor e limites
        assert!(tem(comando(&d, "coloca a formalidade em 8"), "formalidade de 5 para 8"));
        assert!(tem(comando(&d, "empatia no máximo"), "para 10"));
        assert!(tem(comando(&d, "mais empatia"), "já está em 10/10, no limite"));
        assert!(tem(comando(&d, "humor no mínimo"), "para 0"));
        assert!(tem(comando(&d, "formalidade em 5"), "(5 é o neutro.)"));
        assert_eq!(comando(&d, "coloca a franqueza lá"), None); // mandou definir sem dizer quanto
        assert_eq!(niveis(&d), [10, 0, 5, 5, 5, 5]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn bloco_lembrete_resumo_e_volta_ao_normal() {
        let d = pasta("b");
        comando(&d, "humor em 9");
        comando(&d, "empatia em 1");
        comando(&d, "mais iniciativa");
        let b = bloco(&niveis(&d), "Davi");
        assert!(b.contains("pedido do Davi") && b.contains("- humor 9/10: OBRIGATORIO") && b.contains("- empatia 1/10: OBRIGATORIO em toda resposta, mesmo curta ou factual: frio e tecnico")
            && b.contains("- proatividade 7/10: proativo:") && !b.contains("formalidade"));
        assert!(lembrete(&niveis(&d), "").contains("[TOM DESTA RESPOSTA, pedido do dono") && lembrete(&niveis(&d), "").contains("humor 9/10"));
        assert!(tem(comando(&d, "como está sua personalidade?"), "humor 9/10"));
        assert!(tem(comando(&d, "volta a personalidade ao normal"), "tudo em 5"));
        assert!(bloco(&niveis(&d), "Davi").is_empty());
        comando(&d, "humor em 99");
        assert_eq!(niveis(&d)[1], 10);
        assert!(tem(comando(&d, "volte ao normal"), "tudo em 5") && niveis(&d) == [5; 6]);
        std::fs::write(d.join("personalidade.json"), r#"{"humor": 77, "empatia": "x"}"#).unwrap();
        assert_eq!(niveis(&d), [5, 10, 5, 5, 5, 5]); // valor fora da faixa e cortado; lixo vira padrao
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn conversa_comum_nao_dispara() {
        let d = pasta("c");
        for t in ["tenho mais humor hoje do que ontem, vou sair com a Bia para comer", "qual o humor da Bia?", "o humor do gerente é ruim", "manda mensagem pra Bia", "mais café", "",
                  "liga a autonomia remota", "mais autonomia remota"] {
            assert_eq!(comando(&d, t), None, "{t}");
        }
        assert!(niveis(&d) == [5; 6] && !d.join("personalidade.json").exists());
        assert!(tem(comando(&d, "mais autonomia"), "proatividade de 5 para 7"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
