//! Assuntos e agentes: o Jarvis continua sendo a entrada de toda conversa; cada ASSUNTO tem um AGENTE (nome + instrucao + palavras-chave).
//! O codigo traz so os agentes gerais. Os da pessoa (loja, familia, trabalho...) ficam em dados/agentes.json:
//! {"agentes": [{"id", "nome", "agente", "descricao", "instrucao", "palavras": ["..."]}]}; mesmo id = o da pessoa vale.
use crate::{banco, barramento, fatos};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

/// id, assunto, agente, descricao, instrucao, palavras-chave (sem acento)
const GERAIS: &[[&str; 6]] = &[
    ["braniac", "Braniac e Jarvis (desenvolvimento)", "Forja", "recursos, servidor, IAs e o funcionamento do próprio Braniac e do Jarvis",
     "Você é a Forja, agente do BRANIAC. Diga o que foi feito e o que falta, com honestidade; não afirme teste que não foi rodado.",
     "braniac jarvis conselho ollama gemini servidor aplicativo autonomia"],
    ["programacao", "Programação (código, bugs, repositórios)", "Ada", "programar, corrigir bugs, escolher tecnologia, revisar código e repositórios",
     "Você é a Ada, agente de PROGRAMAÇÃO. Leia o código antes de mexer, faça mudanças pequenas e testáveis, diga exatamente o que rodou e o que NÃO rodou. Não coloque segredos nem dados pessoais em código.",
     "programar programacao programa codigo python javascript typescript rust java html css react node npm api funcao classe erro compilar teste testes repositorio github commit deploy bug debug script sql framework biblioteca refatorar"],
    ["rotina", "Rotina e agenda", "Compasso", "rotinas, lembretes, agenda e o que cada agente fez e quando",
     "Você é o Compasso, agente de ROTINA. Guarda as rotinas e a agenda da pessoa e aciona o Jarvis na hora certa. Só executa ações da lista permitida, dentro do horário e dos tetos.",
     "rotina rotinas compasso agenda lembrete lembrar horario agendar"],
];

/// Agentes ativos: os da pessoa primeiro (ganham no id repetido), depois os gerais. Arquivo ilegivel = so os gerais.
pub fn listar(dados: &Path) -> Vec<Value> {
    let arq: Value = std::fs::read_to_string(dados.join("agentes.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let mut v: Vec<Value> = arq["agentes"].as_array().cloned().unwrap_or_default().into_iter().filter(|a| a["id"].is_string() && a["agente"].is_string()).collect();
    for g in GERAIS {
        if !v.iter().any(|a| a["id"] == g[0]) {
            v.push(json!({ "id": g[0], "nome": g[1], "agente": g[2], "descricao": g[3], "instrucao": g[4], "palavras": g[5].split(' ').collect::<Vec<_>>(), "local": g[0] == "rotina" }));
        }
    }
    v
}

/// Quem pode falar no barramento.
pub fn nomes(dados: &Path) -> Vec<String> {
    let mut n: Vec<String> = vec!["Jarvis".into(), "Conselho".into()];
    n.extend(listar(dados).iter().filter_map(|a| a["agente"].as_str().map(String::from)));
    n
}

/// Assuntos da mensagem, do mais forte ao mais fraco (palavras-chave iguais; 2 valem mais que 1).
pub fn detectar(dados: &Path, texto: &str) -> Vec<(Value, usize)> {
    let alvo: HashSet<String> = banco::palavras(texto).into_iter().collect();
    let mut out: Vec<(Value, usize)> = listar(dados).into_iter()
        .map(|a| { let p = a["palavras"].as_array().into_iter().flatten().filter(|w| w.as_str().is_some_and(|w| alvo.contains(w))).count(); (a, p) })
        .filter(|(_, p)| *p > 0).collect();
    out.sort_by(|a, b| b.1.cmp(&a.1));
    out
}

static ULTIMA: Mutex<Option<(String, Instant)>> = Mutex::new(None);

/// Deixa no barramento que o Jarvis consultou o agente (so o nome do assunto, nunca o texto da conversa). 1 vez a cada 30 s por combinacao.
fn registrar_consulta(dados: &Path, usados: &[&Value]) {
    let nome = |a: &Value, k: &str| a[k].as_str().unwrap_or("").to_string();
    let chave = usados.iter().map(|a| nome(a, "id")).collect::<Vec<_>>().join("+");
    let Ok(mut u) = ULTIMA.lock() else { return };
    if u.as_ref().is_some_and(|(c, t)| *c == chave && t.elapsed().as_secs() < 30) {
        return;
    }
    *u = Some((chave, Instant::now()));
    let (a1, n1) = (nome(usados[0], "agente"), nome(usados[0], "nome"));
    let _ = barramento::enviar(dados, "Jarvis", &a1, "pedido", &format!("consulta sobre {n1}"), 0, None);
    if let Some(b) = usados.get(1) {
        let (a2, n2) = (nome(b, "agente"), nome(b, "nome"));
        let _ = barramento::enviar(dados, &a1, &a2, "pedido", &format!("o assunto também envolve {n2}: o que você tem?"), 0, None);
        let _ = barramento::enviar(dados, &a2, &a1, "resposta", &format!("passei as regras de {n2}"), 0, None);
        let _ = barramento::enviar(dados, &a1, "Jarvis", "resposta", &format!("juntei as regras de {n1} e {n2}"), 0, None);
    } else {
        let _ = barramento::enviar(dados, &a1, "Jarvis", "resposta", &format!("passei as regras de {n1}"), 0, None);
    }
}

/// Bloco do agente certo para anexar ao prompt (ate 2 assuntos): instrucao + fatos validos do assunto. "" se nenhum assunto casar.
/// `so_publico` = a IA e online: so recebe fato publico.
pub fn contexto_para(dados: &Path, texto: &str, so_publico: bool) -> String {
    let d = detectar(dados, texto);
    let usados: Vec<&Value> = d.iter().take(2).map(|(a, _)| a).collect();
    if usados.is_empty() {
        return String::new();
    }
    registrar_consulta(dados, &usados);
    let partes: Vec<String> = usados.iter().map(|a| format!("AGENTE {} (assunto: {}). {}\nFATOS VÁLIDOS DESTE ASSUNTO:\n{}", a["agente"].as_str().unwrap_or(""), a["nome"].as_str().unwrap_or(""), a["instrucao"].as_str().unwrap_or(""), fatos::bloco(dados, a["id"].as_str().unwrap_or(""), texto, so_publico))).collect();
    let bloco = format!("\n\nASSUNTO DETECTADO: responda como o agente abaixo, só com estes fatos, o banco e a conversa:\n{}", partes.join("\n\n"));
    bloco.chars().take(1500).collect()
}

/// Lista dos agentes, para o Jarvis responder "quais sao os agentes?" sem inventar.
pub fn resumo(dados: &Path) -> String {
    let l: Vec<String> = listar(dados).iter().map(|a| format!("• {} — {}: {}", a["agente"].as_str().unwrap_or(""), a["nome"].as_str().unwrap_or(""), a["descricao"].as_str().unwrap_or(""))).collect();
    format!("Agentes do Braniac, um por assunto (o Jarvis detecta o assunto e passa para o agente certo):\n{}", l.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detecta_usa_o_da_pessoa_e_registra_a_consulta() {
        let d = std::env::temp_dir().join(format!("braniac-agentes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        assert_eq!(listar(&d).len(), GERAIS.len());
        assert_eq!(contexto_para(&d, "bom dia, tudo bem?", true), "");
        std::fs::write(d.join("agentes.json"), r#"{"agentes": [
            {"id": "loja", "nome": "Loja", "agente": "Balcão", "descricao": "pedidos", "instrucao": "Regra da loja.", "palavras": ["pedido", "cardapio"]},
            {"id": "rotina", "nome": "Minha rotina", "agente": "Compasso", "descricao": "x", "instrucao": "Regra minha.", "palavras": ["rotina"]},
            {"sem": "id"}]}"#).unwrap();
        assert_eq!(listar(&d).len(), GERAIS.len() + 1); // rotina da pessoa substitui a geral; item sem id e ignorado
        let det = detectar(&d, "Qual o CARDÁPIO do pedido? E a rotina?");
        assert_eq!((det[0].0["id"].as_str(), det[0].1, det.len()), (Some("loja"), 2, 2));
        let c = contexto_para(&d, "Qual o cardápio do pedido? E a rotina?", true);
        assert!(c.contains("AGENTE Balcão") && c.contains("Regra da loja.") && c.contains("Regra minha."));
        let h = barramento::historico(&d, 50);
        assert_eq!((h.len(), h[0]["de"].as_str(), h[0]["para"].as_str(), h[3]["para"].as_str()), (4, Some("Jarvis"), Some("Balcão"), Some("Jarvis")));
        assert!(resumo(&d).contains("• Balcão — Loja: pedidos"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
