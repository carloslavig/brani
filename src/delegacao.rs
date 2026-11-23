//! Delegacao: so o Jarvis fala com a pessoa. Pergunta de consulta -> o Jarvis vai ao agente do assunto (barramento: pedido),
//! o agente responde com os dados que realmente tem (barramento: resposta) e o Jarvis repassa.
//! Agente LOCAL (campo "local" em dados/agentes.json; sem o campo = local): a resposta e montada aqui, sem nenhuma IA.
//! Agente nao local: quem compoe a resposta e a IA, com os fatos que agentes::contexto_para ja poe no prompt.
use crate::consulta::eh_consulta;
use crate::tempo::Agora;
use crate::{agenda, agentes, banco, barramento, cofre, fatos};
use serde_json::{json, Value};
use std::path::Path;

pub struct Resposta {
    pub texto: String,
    pub agentes: Vec<String>,
}

/// O que o agente tem para esta pergunta: (texto, resumo curto). None = nada registrado.
fn bloco(dados: &Path, a: &Value, pergunta: &str, agora: &Agora) -> Option<(String, String)> {
    let id = a["id"].as_str()?;
    if id == "rotina" {
        let dias = if banco::palavras(pergunta).iter().any(|w| w == "semana") { 7 } else { 1 };
        let lista = agenda::ferramenta(&dados.join("agenda.json"), &json!({ "acao": "listar", "dias": dias }), agora);
        return Some((lista, format!("agenda {}", if dias > 1 { "da semana" } else { "de hoje" })));
    }
    if a["agente"] == "Cofre" && banco::palavras(pergunta).iter().any(|w| ["saldo", "meta", "falta", "conta", "dinheiro"].contains(&w.as_str())) {
        let s = cofre::situacao(dados, agora.data); // so quando perguntam de dinheiro e ha saldo e meta informados; senao valem os fatos do assunto
        if s["tem"] == true {
            return Some((s["fala"].as_str().unwrap_or("").to_string(), "situação da meta".into()));
        }
    }
    let pal = banco::palavras(pergunta);
    let mut f = fatos::validos(dados, id, false, fatos::agora_ts());
    f.sort_by(|x, y| banco::pontua(&y.0, &pal).cmp(&banco::pontua(&x.0, &pal))); // estavel: empate fica no mais novo
    let l: Vec<String> = f.iter().take(5).map(|(t, _)| format!("- {}", t.chars().take(240).collect::<String>())).collect();
    (!l.is_empty()).then(|| (l.join("\n"), format!("{} fato(s) válido(s)", l.len())))
}

/// Consulta os agentes do assunto (ate 2). None = a pergunta segue para a IA (nao e consulta, ninguem tem dado, ou nenhum agente e local).
pub fn atender(dados: &Path, texto: &str, agora: &Agora) -> Option<Resposta> {
    if !eh_consulta(texto) {
        return None;
    }
    let com_dado: Vec<(Value, String, String)> = agentes::detectar(dados, texto).into_iter().take(2)
        .filter_map(|(a, _)| bloco(dados, &a, texto, agora).map(|(t, r)| (a, t, r))).collect();
    if !com_dado.iter().any(|(a, _, _)| a["local"] != false) {
        return None;
    }
    let nome = |a: &Value, k: &str| a[k].as_str().unwrap_or("").to_string();
    for (a, _, resumo) in &com_dado {
        // no barramento vai so o assunto e o resumo, nunca a pergunta nem o dado
        let _ = barramento::enviar(dados, "Jarvis", &nome(a, "agente"), "pedido", &format!("a pessoa quer saber de {}", nome(a, "nome")), 0, None);
        let _ = barramento::enviar(dados, &nome(a, "agente"), "Jarvis", "resposta", resumo, 0, None);
    }
    let nomes: Vec<String> = com_dado.iter().map(|(a, _, _)| nome(a, "agente")).collect();
    let blocos: Vec<String> = com_dado.iter().map(|(a, t, _)| format!("🔹 {}:\n{t}", nome(a, "agente"))).collect();
    let intro = format!("Falei com {}. {}", nomes.join(" e com "), if nomes.len() == 1 { "O que ele tem:" } else { "O que eles têm:" });
    Some(Resposta { texto: format!("{intro}\n\n{}", blocos.join("\n\n")), agentes: nomes })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;

    #[test]
    fn local_responde_sem_ia_e_o_resto_segue_para_a_ia() {
        let d = std::env::temp_dir().join(format!("braniac-delegacao-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("agentes.json"), r#"{"agentes": [
            {"id": "caixa", "nome": "Dinheiro", "agente": "Cofre", "descricao": "x", "instrucao": "R.", "palavras": ["saldo", "meta"]},
            {"id": "loja", "nome": "Loja", "agente": "Balcão", "descricao": "x", "instrucao": "R.", "palavras": ["esfiha"], "local": false}]}"#).unwrap();
        let agora = Agora { data: Data::civil(2026, 11, 17).unwrap(), hora: 10, min: 0, seg: 0 };
        let ts = fatos::agora_ts();
        assert!(atender(&d, "Quanto falta pra meta?", &agora).is_none()); // agente local sem nenhum fato: a IA responde
        fatos::registrar(&d, "caixa", "k", "A meta do mês é juntar 100", "local", "t", None, ts).unwrap();
        fatos::registrar(&d, "loja", "k", "A esfiha de carne custa 5", "publico", "t", None, ts).unwrap();
        let r = atender(&d, "Quanto falta pra meta?", &agora).unwrap();
        assert_eq!(r.agentes, vec!["Cofre"]);
        assert!(r.texto.starts_with("Falei com Cofre. O que ele tem:") && r.texto.contains("- A meta do mês é juntar 100"));
        let h = barramento::historico(&d, 10);
        assert_eq!((h.len(), h[0]["texto"].as_str(), h[1]["texto"].as_str()), (2, Some("a pessoa quer saber de Dinheiro"), Some("1 fato(s) válido(s)")));
        assert!(atender(&d, "Quanto custa a esfiha?", &agora).is_none()); // agente nao local: a IA compoe
        cofre::informar(&d, agora.data, Some(40.0), Some(100.0)).unwrap();
        assert!(atender(&d, "Quanto falta pra meta?", &agora).unwrap().texto.contains("faltam R$ 60,00")); // com saldo informado, a conta e do Cofre
        assert!(atender(&d, "Anota a meta de hoje?", &agora).is_none()); // pedido de acao nao e consulta
        assert!(atender(&d, "a meta esta boa", &agora).is_none()); // nao e pergunta
        let r = atender(&d, "O que tenho na agenda esta semana?", &agora).unwrap(); // Compasso (geral) responde com a agenda
        assert!(r.agentes == vec!["Compasso"] && r.texto.contains("🔹 Compasso:"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
