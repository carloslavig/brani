//! Conselho de IAs: o mesmo tema vai a varias IAs, cada uma com um papel, e sai uma sintese.
//! Porte de braniac/conselho.py. Seguranca: so vai o TEMA; tema com dado sensivel cancela a reuniao.
//! Hoje os membros sao IAs com chave (uma por familia). Membro por aba logada na VM (ChatGPT, Gemini, Grok,
//! DeepSeek) entra trocando a funcao `consultar`, quando a pessoa fizer os logins.
use crate::roteador::Roteador;
use serde_json::{json, Value};
use std::sync::Mutex;

pub const PAPEIS: [&str; 4] = ["raciocínio e ideias", "criatividade e alternativas", "checagem de fatos", "análise técnica e lógica passo a passo"];

/// Ate 4 IAs online prontas, uma por familia (groq, gemini, mistral...), cada uma com um papel.
pub fn membros(rot: &Roteador) -> Vec<(String, String)> {
    let mut familias: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for p in rot.cfg["cadeia"].as_array().into_iter().flatten() {
        let nome = p["nome"].as_str().unwrap_or("");
        let familia = nome.split('-').next().unwrap_or("").to_string();
        if p["tipo"] != "online" || familias.contains(&familia) || !rot.disponivel(p).0 {
            continue;
        }
        out.push((nome.to_string(), PAPEIS[out.len()].to_string()));
        familias.push(familia);
        if out.len() == PAPEIS.len() {
            break;
        }
    }
    out
}

/// Reuniao: `consultar(ia, pergunta)` devolve (quem respondeu de fato, texto); ele e `sintetizar(texto)` devolvem None quando falham. As IAs opinam ao mesmo tempo.
pub fn reunir(tema: &str, membros: &[(String, String)], consultar: &(dyn Fn(&str, &str) -> Option<(String, String)> + Sync), sintetizar: &dyn Fn(&str) -> Option<String>) -> Result<Value, String> {
    let tema = tema.trim();
    if tema.chars().count() < 12 {
        return Err("Qual o assunto do conselho de IAs? Me passa o tema completo.".into());
    }
    let respostas: Vec<Option<(String, String)>> = std::thread::scope(|s| {
        let fios: Vec<_> = membros.iter().map(|(ia, papel)| {
            s.spawn(move || {
                let pergunta = format!("Você participa de um conselho de IAs. Dê sua opinião objetiva (até 8 linhas, em português) sobre: {tema}. Seu papel no conselho: {papel}.");
                consultar(ia, &pergunta).map(|(quem, r)| (quem, r.trim().to_string())).filter(|(_, r)| !r.is_empty())
            })
        }).collect();
        fios.into_iter().map(|f| f.join().unwrap_or(None)).collect()
    });
    let (mut opinioes, mut ausentes) = (Vec::new(), Vec::new());
    for ((ia, papel), r) in membros.iter().zip(respostas) {
        match r {
            Some((quem, texto)) => opinioes.push(json!({ "ia": quem, "papel": papel, "texto": texto })),
            None => ausentes.push(ia.clone()),
        }
    }
    if opinioes.is_empty() {
        return Err("O conselho de IAs não conseguiu se reunir agora (nenhuma IA respondeu).".into());
    }
    let corpo = opinioes.iter().map(|o| format!("• {}: {}", o["ia"].as_str().unwrap_or(""), o["texto"].as_str().unwrap_or(""))).collect::<Vec<_>>().join("\n\n");
    let sintese = sintetizar(&format!("Tema: {tema}\n\nOpiniões do conselho:\n{corpo}\n\nFaça uma síntese curta (até 6 linhas) com a decisão recomendada e os pontos de divergência."))
        .map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let mut relatorio = format!("Conselho de IAs do Braniac\n\n{corpo}");
    if !ausentes.is_empty() {
        relatorio += &format!("\n\n(não responderam: {})", ausentes.join(", "));
    }
    if let Some(s) = &sintese {
        relatorio += &format!("\n\nSíntese: {s}");
    }
    Ok(json!({ "tema": tema, "opinioes": opinioes, "ausentes": ausentes, "sintese": sintese, "relatorio": relatorio }))
}

/// Reuniao de verdade com as IAs do roteador.
pub fn convocar(rot: &Roteador, tema: &str) -> Result<Value, String> {
    if rot.sensivel(tema) {
        return Err("Conselho cancelado: o tema tem dado sensível, e isso não pode ir para IA de fora.".into());
    }
    let m = membros(rot);
    if m.is_empty() {
        return Err("O conselho não tem nenhuma IA pronta agora.".into());
    }
    // reservas: as outras IAs online prontas; membro que falha (limite de uso, fora do ar) cede a cadeira a uma delas
    let reservas: Vec<String> = rot.cfg["cadeia"].as_array().into_iter().flatten()
        .filter(|p| p["tipo"] == "online" && rot.disponivel(p).0).filter_map(|p| p["nome"].as_str().map(String::from))
        .filter(|n| !m.iter().any(|(nome, _)| nome == n)).rev().collect();
    let reservas = Mutex::new(reservas);
    let sistema = "Você é um membro de um conselho de IAs. Responda em português do Brasil, direto ao ponto.";
    let consultar = |ia: &str, pergunta: &str| {
        let mut quem = ia.to_string();
        loop {
            if let Ok(r) = rot.perguntar_a(&quem, sistema, pergunta) {
                return Some((quem, r));
            }
            quem = reservas.lock().ok()?.pop()?;
        }
    };
    reunir(tema, &m, &consultar,
           &|texto| rot.neutro("Você redige a síntese de um conselho de IAs. Use só as opiniões dadas; não invente fato nem número.", texto).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{banco::Banco, store::Store};

    fn m(nomes: &[&str]) -> Vec<(String, String)> {
        nomes.iter().enumerate().map(|(i, n)| (n.to_string(), PAPEIS[i].to_string())).collect()
    }

    #[test]
    fn opinioes_ausentes_e_sintese() {
        let consultar = |ia: &str, p: &str| (ia != "b").then(|| (ia.to_string(), format!("opiniao de {ia} ({})", p.contains("Seu papel no conselho"))));
        let r = reunir("vale a pena aprender Rust?", &m(&["a", "b", "c"]), &consultar, &|t| Some(format!(" resumo com {} ", t.matches("• ").count()))).unwrap();
        assert_eq!(r["opinioes"].as_array().unwrap().len(), 2);
        assert_eq!(r["opinioes"][0], json!({"ia": "a", "papel": PAPEIS[0], "texto": "opiniao de a (true)"}));
        assert_eq!(r["ausentes"], json!(["b"]));
        assert_eq!(r["sintese"], "resumo com 2");
        let rel = r["relatorio"].as_str().unwrap();
        assert!(rel.contains("• c: opiniao de c") && rel.contains("(não responderam: b)") && rel.ends_with("Síntese: resumo com 2"));
    }

    #[test]
    fn tema_curto_ninguem_responde_e_sintese_que_falha() {
        let sim = |ia: &str, _: &str| Some((ia.to_string(), "ok".to_string()));
        assert!(reunir("curto", &m(&["a"]), &sim, &|_| None).unwrap_err().starts_with("Qual o assunto"));
        assert!(reunir("um tema bem completo", &m(&["a", "b"]), &|_, _| None, &|_| None).unwrap_err().contains("nenhuma IA respondeu"));
        let r = reunir("um tema bem completo", &m(&["a"]), &sim, &|_| None).unwrap();
        assert!(r["sintese"].is_null() && !r["relatorio"].as_str().unwrap().contains("Síntese"));
    }

    fn rot() -> Roteador {
        Roteador {
            cfg: json!({ "sensivel": ["senha"], "cadeia": [
                {"nome": "groq", "tipo": "online", "api": "openai", "chave_env": "TEM"},
                {"nome": "groq-20b", "tipo": "online", "api": "openai", "chave_env": "TEM"},
                {"nome": "gemini", "tipo": "online", "api": "openai", "chave_env": "FALTA"},
                {"nome": "mistral", "tipo": "online", "api": "openai", "chave_env": "TEM"},
                {"nome": "ollama", "tipo": "local", "api": "ollama"}]}),
            store: Store::em_memoria().unwrap(),
            banco: Banco::em_memoria().unwrap(),
            chamar: Box::new(|p, msgs, _, _| Ok(format!("{} disse algo sobre {}", p["nome"].as_str().unwrap(), msgs.as_array().unwrap().len()))),
            chamar_tools: Box::new(|_, _, _, _, _| Ok(json!({}))),
            contexto: Box::new(|_, _| String::new()),
            vigia: Box::new(|_, _| {}),
            ler_chave: Box::new(|k| if k == "TEM" { "k".into() } else { String::new() }),
        }
    }

    #[test]
    fn membros_sao_um_por_familia_so_online_e_so_quem_esta_pronto() {
        assert_eq!(membros(&rot()), vec![("groq".to_string(), PAPEIS[0].to_string()), ("mistral".to_string(), PAPEIS[1].to_string())]);
    }

    #[test]
    fn membro_que_falha_cede_a_cadeira_a_uma_reserva() {
        let mut r = rot();
        // groq (membro) fora do ar; groq-20b e a unica reserva pronta
        r.chamar = Box::new(|p, _, _, _| if p["nome"] == "groq" { Err(crate::provedores::ErroProvedor { msg: "fora".into(), descanso: 0 }) } else { Ok(format!("opiniao de {}", p["nome"].as_str().unwrap())) });
        let v = convocar(&r, "qual a melhor linguagem para um servidor leve?").unwrap();
        assert_eq!(v["opinioes"][0]["ia"], "groq-20b");
        assert_eq!(v["opinioes"][0]["papel"], PAPEIS[0]);
        assert_eq!(v["opinioes"][1]["ia"], "mistral");
        assert_eq!(v["ausentes"], json!([]));
    }

    #[test]
    fn convocar_de_verdade_e_tema_sensivel_cancela() {
        let r = rot();
        r.banco.gravar_fato("pessoa", "x", "fato privado", "local", "t", None);
        let v = convocar(&r, "qual a melhor linguagem para um servidor leve?").unwrap();
        // cada membro recebe so sistema + pergunta: nem banco nem historico vao junto
        assert_eq!(v["opinioes"][0]["texto"], "groq disse algo sobre 2");
        assert_eq!(v["opinioes"][1]["ia"], "mistral");
        assert!(v["sintese"].as_str().unwrap().contains("disse algo sobre 2"));
        assert!(r.store.historico("jarvis", 10).is_empty());
        assert!(convocar(&r, "o que fazer com a minha senha do banco?").unwrap_err().starts_with("Conselho cancelado"));
    }
}
