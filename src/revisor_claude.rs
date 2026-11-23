//! Porteiro das respostas ao dono no WhatsApp: o Jarvis passa a conversa recente, a pergunta e a resposta que pretende dar; o
//! revisor diz se presta. Aprovou -> sai. Reprovou -> sai a versao corrigida (ou um aviso curto). Revisor fora do ar ou parecer
//! ilegivel -> `ok = None`: quem chama decide (para o dono, a resposta sai assim mesmo; ver dono.rs).
//! Ficou de fora: chamar o agente revisor (closure `perguntar`) e ler o historico do banco (entra por parametro).
//! Nota: no Python `_historico` le as tuplas do banco como dicionario e cai sempre em "(sem histórico)"; aqui o historico entra de verdade.
#![allow(dead_code)]
use crate::tempo::Agora;
use serde_json::Value;

pub const TIMEOUT_S: u64 = 120;

#[derive(Debug, Clone, PartialEq)]
pub struct Parecer {
    /// Some(true) aprovou, Some(false) reprovou, None = sem revisor.
    pub ok: Option<bool>,
    pub motivo: String,
    pub corrigida: String,
}

fn corta(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Conversa recente, da mais antiga para a mais nova. `historico` = (direcao "E" dono / "C" Jarvis, texto).
pub fn historico_texto(historico: &[(String, String)], dono: &str) -> String {
    let quem = if dono.trim().is_empty() { "Dono" } else { dono.trim() };
    let l: Vec<String> = historico.iter().map(|(d, t)| format!("{}: {}", if d == "E" { quem } else { "Jarvis" }, corta(t, 300))).collect();
    if l.is_empty() { "(sem histórico)".into() } else { l.join("\n") }
}

/// O pedido ao revisor. Pergunta, resposta e conversa entram como texto a julgar (dado), nunca como ordem.
pub fn prompt(pergunta: &str, resposta: &str, historico: &[(String, String)], por_audio: bool, agora: &Agora, dono: &str) -> String {
    let quem = if dono.trim().is_empty() { "dono" } else { dono.trim() };
    let (a, m, d) = agora.data.partes();
    format!(
        "Voce e o REVISOR das respostas do Jarvis (assistente do {quem}, no WhatsApp). Nada sai sem a sua aprovacao: se uma resposta ruim sair, a culpa e sua.\n\n\
Hoje: {d:02}/{m:02}/{a} {}. A mensagem do {quem} chegou por {}.\n\n\
CONVERSA RECENTE (mais antiga primeiro):\n{}\n\n\
PERGUNTA/PEDIDO DO {} AGORA:\n{}\n\n\
RESPOSTA QUE O JARVIS QUER ENVIAR:\n{}\n\n\
Decida se essa resposta presta. REPROVE se: nao responde o que foi perguntado; fala de outro assunto ou outro agente; \
e muito mais longa do que a pergunta pede (pergunta pontual = resposta curta e direta, sem despejar fichas, listas ou avisos); parece inventar dado; \
repete a conversa anterior sem necessidade; tem tom robotico, rotulos (\"Falei com...\", \"Fato:\") ou revela bastidores (agentes, IAs, banco).\n\
Se o pedido veio truncado/ininteligivel (transcricao de audio ruim), a resposta certa e pedir para repetir, em UMA frase.\n\
APROVE se responde direto e com bom senso.\n\n\
Responda SOMENTE com uma linha JSON, sem mais nada:\n\
{{\"aprovado\": true|false, \"motivo\": \"uma frase curta\", \"corrigida\": \"resposta melhor, curta, em portugues do Brasil, so com fatos que apareceram acima (ou vazio se aprovado ou se nao da para corrigir)\"}}",
        agora.hm(), if por_audio { "ÁUDIO (transcrito, pode ter erros)" } else { "texto" }, historico_texto(historico, dono),
        quem.to_uppercase(), corta(pergunta, 1200), corta(resposta, 3000))
}

/// O objeto JSON do parecer (do primeiro "{" ao ultimo "}"), so se tiver "aprovado" booleano.
fn parecer_json(txt: &str) -> Option<Value> {
    let (i, f) = (txt.find('{')?, txt.rfind('}')?);
    let d: Value = serde_json::from_str(txt.get(i..=f)?).ok()?;
    d["aprovado"].is_boolean().then_some(d)
}

fn campo(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        o => o.to_string(),
    }
}

/// `perguntar(prompt)` devolve a resposta bruta do revisor (None = fora do ar). `log` recebe o registro de auditoria.
#[allow(clippy::too_many_arguments)]
pub fn revisar(pergunta: &str, resposta: &str, historico: &[(String, String)], por_audio: bool, agora: &Agora, dono: &str,
               perguntar: &dyn Fn(&str) -> Option<String>, log: &dyn Fn(&str)) -> Parecer {
    let bruto = perguntar(&prompt(pergunta, resposta, historico, por_audio, agora, dono));
    let Some(d) = bruto.as_deref().and_then(parecer_json) else {
        log(&format!("SEM REVISAO | pergunta={:?} | bruto={:?}", corta(pergunta, 60), corta(bruto.as_deref().unwrap_or(""), 120)));
        return Parecer { ok: None, motivo: "revisor indisponível".into(), corrigida: String::new() };
    };
    let r = Parecer { ok: d["aprovado"].as_bool(), motivo: corta(&campo(&d["motivo"]), 200), corrigida: campo(&d["corrigida"]).trim().to_string() };
    log(&format!("{} | pergunta={:?} | resposta={:?} | motivo={:?} | corrigida={:?}", if r.ok == Some(true) { "APROVOU" } else { "REPROVOU" },
        corta(pergunta, 60), corta(resposta, 80), r.motivo, corta(&r.corrigida, 80)));
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;
    use std::cell::RefCell;

    fn agora() -> Agora {
        Agora { data: Data::civil(2026, 11, 8).unwrap(), hora: 9, min: 5, seg: 0 }
    }

    fn rev(resp: Option<&str>) -> Parecer {
        revisar("que horas é a prova?", "Amanhã às 8h.", &[], false, &agora(), "Davi", &|_| resp.map(str::to_string), &|_| ())
    }

    #[test]
    fn aprova_e_reprova() {
        assert_eq!(rev(Some(r#"{"aprovado": true, "motivo": "direto", "corrigida": ""}"#)).ok, Some(true));
        let no = rev(Some(r#"ok {"aprovado": false, "motivo": "assunto errado", "corrigida": " Amanhã às 8h. "} fim"#));
        assert_eq!((no.ok, no.corrigida.as_str(), no.motivo.as_str()), (Some(false), "Amanhã às 8h.", "assunto errado"));
        assert_eq!(rev(Some(r#"{"aprovado": false, "corrigida": null}"#)).corrigida, "");
    }

    #[test]
    fn sem_revisor_nao_aprova() {
        for ruim in [None, Some("lixo sem json"), Some(r#"{"aprovado": "sim"}"#), Some(r#"{"motivo": "x"}"#), Some("} {"), Some("[true]")] {
            let p = rev(ruim);
            assert_eq!((p.ok, p.corrigida.as_str()), (None, ""), "{ruim:?}");
        }
    }

    #[test]
    fn prompt_leva_conversa_pergunta_e_resposta_e_o_log_registra() {
        let h = vec![("E".to_string(), "e a esteira?".to_string()), ("C".to_string(), "Rodando.".to_string())];
        let (visto, logs) = (RefCell::new(String::new()), RefCell::new(vec![]));
        revisar("quanto falta?", "Faltam dois.", &h, true, &agora(), "Davi", &|p| { *visto.borrow_mut() = p.to_string(); None }, &|l| logs.borrow_mut().push(l.to_string()));
        let p = visto.borrow();
        for trecho in ["Davi: e a esteira?\nJarvis: Rodando.", "quanto falta?", "Faltam dois.", "ÁUDIO (transcrito", "08/11/2026 09:05", "PEDIDO DO DAVI AGORA"] {
            assert!(p.contains(trecho), "{trecho}");
        }
        assert!(logs.borrow()[0].starts_with("SEM REVISAO"));
        assert!(prompt("a", "b", &[], false, &agora(), "").contains("(sem histórico)"));
    }
}
