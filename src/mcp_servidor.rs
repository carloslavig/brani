//! Piloto do servidor MCP local: por ele uma ferramenta de fora consulta os agentes do Braniac. O Braniac continua dono dos dados e das
//! regras: so os agentes NAO locais (sem dado pessoal/financeiro/saude/familia; "local": false em dados/agentes.json) respondem por aqui.
//! Qualquer outro e recusado com orientacao de falar com o Jarvis. Nada de acao: so leitura. Cada chamada vai para dados/mcp_piloto.log
//! (so hora, ferramenta e agente; nunca o conteudo).
//! Ficou de fora: o protocolo MCP (stdio) e a consulta em si (`consultar(agente, pergunta)` e closure; no Python era o `delegacao.consultar`,
//! que tambem registra pedido e resposta no barramento).
#![allow(dead_code)]
use crate::empresa_dev::corta;
use crate::tempo::Agora;
use serde_json::Value;
use std::io::Write;
use std::path::Path;

pub const LIMITE: usize = 1800;

fn log(dados: &Path, agora: &Agora, msg: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dados.join("mcp_piloto.log")) {
        let _ = writeln!(f, "{} {:02}:{:02}:{:02} {msg}", agora.data.iso(), agora.hora, agora.min, agora.seg);
    }
}

fn nome(a: &Value) -> &str {
    a["agente"].as_str().unwrap_or("")
}

/// Agentes que podem responder por este canal: so os marcados "local": false (sem a marca, o agente e tratado como local).
pub fn agentes_publicos(agentes: &[Value]) -> Vec<String> {
    let mut v: Vec<String> = agentes.iter().filter(|a| a["local"] == false && !nome(a).is_empty()).map(|a| nome(a).to_string()).collect();
    v.sort();
    v
}

/// Lista com a especialidade de cada um.
pub fn listar_agentes_publicos(dados: &Path, agentes: &[Value], agora: &Agora) -> String {
    log(dados, agora, "listar_agentes_publicos");
    agentes_publicos(agentes).iter().map(|n| format!("- {n}: {}", agentes.iter().find(|a| nome(a) == n).and_then(|a| a["descricao"].as_str()).unwrap_or(""))).collect::<Vec<_>>().join("\n")
}

/// Pergunta a um agente e devolve o que ele tem (ate 1.800 letras). A pergunta e DADO (ate 300 letras), nunca instrucao para o Braniac.
pub fn consultar_agente(dados: &Path, agentes: &[Value], agente: &str, pergunta: &str, agora: &Agora, consultar: &dyn Fn(&str, &str) -> String) -> String {
    let agente = agente.trim();
    let pergunta = corta(pergunta.trim(), 300);
    if agente.is_empty() || !agentes.iter().any(|a| nome(a) == agente) {
        // o nome veio de fora: no registro so letras e numeros
        log(dados, agora, &format!("consultar_agente desconhecido:{}", corta(agente, 20).chars().filter(|c| c.is_alphanumeric()).collect::<String>()));
        return format!("Agente desconhecido: {}. Use listar_agentes_publicos.", corta(agente, 30));
    }
    if !agentes_publicos(agentes).iter().any(|n| n == agente) {
        log(dados, agora, &format!("consultar_agente RECUSADO:{agente}"));
        return format!("O agente {agente} lida com dados pessoais e não responde por este canal. Peça direto ao Jarvis.");
    }
    if pergunta.is_empty() {
        return "Faltou a pergunta.".into();
    }
    let r = consultar(agente, &pergunta);
    log(dados, agora, &format!("consultar_agente {agente}"));
    corta(&r, LIMITE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;
    use serde_json::json;
    use std::cell::RefCell;

    fn agentes() -> Vec<Value> {
        vec![json!({"agente": "Forja", "descricao": "Manutenção do sistema", "local": false}), json!({"agente": "Ada", "descricao": "Programação", "local": false}),
             json!({"agente": "Caixa", "descricao": "Finanças", "local": true}), json!({"agente": "Lar", "descricao": "Família"})]
    }

    #[test]
    fn so_agentes_sem_dado_pessoal_respondem() {
        let d = std::env::temp_dir().join(format!("braniac-mcp_servidor-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let agora = Agora { data: Data::civil(2026, 10, 27).unwrap(), hora: 9, min: 5, seg: 7 };
        let ag = agentes();
        assert_eq!(agentes_publicos(&ag), vec!["Ada", "Forja"]);
        assert_eq!(listar_agentes_publicos(&d, &ag, &agora), "- Ada: Programação\n- Forja: Manutenção do sistema");
        let feitas = RefCell::new(vec![]);
        let consultar = |a: &str, p: &str| {
            feitas.borrow_mut().push(format!("{a}|{}", p.chars().count()));
            "x".repeat(5000)
        };
        // agente pessoal (ou sem a marca) e recusado e nenhum dado e levantado
        for n in ["Caixa", "Lar"] {
            let r = consultar_agente(&d, &ag, n, "quanto tenho de saldo?", &agora, &consultar);
            assert!(r.contains("dados pessoais") && !r.contains("R$"), "{n}");
        }
        assert!(consultar_agente(&d, &ag, "Fulano\nde tal", "oi", &agora, &consultar).contains("desconhecido"));
        assert_eq!(consultar_agente(&d, &ag, "Forja", "  ", &agora, &consultar), "Faltou a pergunta.");
        assert!(feitas.borrow().is_empty());
        let r = consultar_agente(&d, &ag, " Forja ", &"como está o sistema? ".repeat(40), &agora, &consultar);
        assert_eq!((r.len(), feitas.borrow().clone()), (LIMITE, vec!["Forja|300".to_string()]));
        // o registro tem hora, ferramenta e agente; nunca a pergunta nem a resposta
        let log = std::fs::read_to_string(d.join("mcp_piloto.log")).unwrap();
        assert_eq!(log.lines().collect::<Vec<_>>(), vec!["2026-10-27 09:05:07 listar_agentes_publicos", "2026-10-27 09:05:07 consultar_agente RECUSADO:Caixa", "2026-10-27 09:05:07 consultar_agente RECUSADO:Lar",
            "2026-10-27 09:05:07 consultar_agente desconhecido:Fulanodetal", "2026-10-27 09:05:07 consultar_agente Forja"]);
        assert!(!log.contains("saldo") && !log.contains("sistema"));
    }
}
