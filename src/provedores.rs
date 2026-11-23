//! Chamadas HTTP as IAs (formato OpenAI e Ollama), com e sem ferramentas. Porte de braniac/providers.py.
//! Rascunho do Otto (NexoDev, T058, veio cortado); fechado e ajustado pelo Claude.
use serde_json::{json, Value};
use std::time::Duration;

/// Falha de um provedor; `descanso` = segundos sem tentar de novo.
#[derive(Debug, Clone, PartialEq)]
pub struct ErroProvedor {
    pub msg: String,
    pub descanso: u64,
}

fn erro(msg: impl Into<String>, descanso: u64) -> ErroProvedor {
    ErroProvedor { msg: msg.into(), descanso }
}

fn inicio(s: &str) -> String {
    s.chars().take(120).collect() // por caractere: corte por byte quebra no meio de acento
}

pub fn erro_de_status(status: u16, retry_after: Option<&str>, corpo: &str) -> ErroProvedor {
    match status {
        // cota estourada: sem cabecalho valido, tenta de novo em 15 min
        429 => erro("429 limite de uso", retry_after.and_then(|x| x.trim().parse::<f64>().ok()).map_or(900, |n| n as u64).max(30)),
        401..=404 => erro(format!("{status} acesso: {}", inicio(corpo)), 6 * 3600),
        500.. => erro(format!("{status} servidor fora"), 300),
        _ => erro(format!("{status} {}", inicio(corpo)), 0),
    }
}

pub fn texto_openai(v: &Value) -> Result<String, ErroProvedor> {
    let t = v["choices"][0]["message"]["content"].as_str().ok_or_else(|| erro("resposta sem conteudo", 0))?;
    if t.trim().is_empty() {
        return Err(erro("resposta vazia", 0));
    }
    Ok(t.trim().to_string())
}

pub fn texto_ollama(v: &Value) -> Result<String, ErroProvedor> {
    let t = v["message"]["content"].as_str().unwrap_or("").trim();
    if t.is_empty() {
        return Err(erro("resposta vazia", 0));
    }
    Ok(t.to_string())
}

fn vazia(m: &Value) -> bool {
    m["content"].as_str().unwrap_or("").trim().is_empty() && m["tool_calls"].as_array().is_none_or(|c| c.is_empty())
}

/// Mensagem do assistente (formato OpenAI) so com o que o proximo passo precisa:
/// alguns provedores rejeitam campos extras como 'reasoning'.
pub fn msg_openai(v: &Value) -> Result<Value, ErroProvedor> {
    let m = &v["choices"][0]["message"];
    if !m.is_object() {
        return Err(erro("resposta sem conteudo", 0));
    }
    if vazia(m) {
        return Err(erro("resposta vazia", 0));
    }
    let mut out = json!({ "role": "assistant", "content": m["content"] });
    for campo in ["tool_calls", "extra_content"] {
        if !m[campo].is_null() {
            out[campo] = m[campo].clone();
        }
    }
    Ok(out)
}

/// O Ollama quer os argumentos das chamadas como objeto (o formato OpenAI guarda como texto JSON).
pub fn msgs_para_ollama(msgs: &Value) -> Value {
    msgs.as_array().into_iter().flatten().map(|m| {
        let mut m = m.clone();
        if let Some(calls) = m["tool_calls"].as_array() {
            m["tool_calls"] = calls.iter().map(|c| {
                let args: Value = c["function"]["arguments"].as_str().and_then(|a| serde_json::from_str(a).ok()).unwrap_or_else(|| json!({}));
                json!({ "function": { "name": c["function"]["name"], "arguments": args } })
            }).collect();
        }
        m
    }).collect()
}

/// Resposta do Ollama (argumentos como objeto e sem id) no formato OpenAI.
pub fn msg_ollama(v: &Value) -> Result<Value, ErroProvedor> {
    let m = &v["message"];
    if vazia(m) {
        return Err(erro("resposta vazia", 0));
    }
    let mut out = json!({ "role": "assistant", "content": m["content"].as_str().unwrap_or("") });
    let calls: Vec<Value> = m["tool_calls"].as_array().into_iter().flatten().enumerate().map(|(i, c)| {
        let args = if c["function"]["arguments"].is_object() { c["function"]["arguments"].to_string() } else { "{}".into() };
        json!({ "id": format!("call_{i}"), "type": "function", "function": { "name": c["function"]["name"], "arguments": args } })
    }).collect();
    if !calls.is_empty() {
        out["tool_calls"] = Value::Array(calls);
    }
    Ok(out)
}

fn postar(req: ureq::Request, corpo: Value, rede: &str) -> Result<Value, ErroProvedor> {
    match req.send_json(corpo) {
        Ok(r) => r.into_json().map_err(|e| erro(format!("resposta ilegivel: {e}"), 0)),
        Err(ureq::Error::Status(codigo, r)) => {
            let espera = r.header("retry-after").map(String::from);
            Err(erro_de_status(codigo, espera.as_deref(), &r.into_string().unwrap_or_default()))
        }
        Err(e) => Err(erro(format!("{rede}: {e}"), 60)),
    }
}

fn req_openai(p: &Value, chave: &str, timeout_s: u64) -> ureq::Request {
    ureq::post(p["url"].as_str().unwrap_or("")).set("Authorization", &format!("Bearer {chave}")).timeout(Duration::from_secs(timeout_s))
}

fn req_ollama(p: &Value, timeout_s: u64, minimo: u64) -> ureq::Request {
    // CPU/GPU fraca: a primeira resposta do modelo local demora
    ureq::post(p["url"].as_str().unwrap_or("")).timeout(Duration::from_secs(timeout_s.max(minimo)))
}

/// Conversa simples: devolve o texto. Escolhe a chamada pelo campo "api" do provedor.
pub fn chamar(p: &Value, msgs: &Value, chave: &str, timeout_s: u64) -> Result<String, ErroProvedor> {
    match p["api"].as_str() {
        Some("openai") => texto_openai(&postar(req_openai(p, chave, timeout_s), json!({ "model": p["modelo"], "messages": msgs }), "rede")?),
        Some("ollama") => {
            let corpo = json!({ "model": p["modelo"], "messages": msgs, "stream": false, "keep_alive": p["keep_alive"].as_str().unwrap_or("2m") });
            texto_ollama(&postar(req_ollama(p, timeout_s, 120), corpo, "ollama indisponivel")?)
        }
        Some("cli") => crate::claude_cli::chamar(msgs, timeout_s),
        outro => Err(erro(format!("api desconhecida: {outro:?}"), 0)),
    }
}

/// Um passo do agente: devolve a mensagem do assistente (content e/ou tool_calls), sempre no formato OpenAI.
pub fn chamar_tools(p: &Value, msgs: &Value, chave: &str, timeout_s: u64, tools: &Value) -> Result<Value, ErroProvedor> {
    match p["api"].as_str() {
        Some("openai") => {
            let corpo = json!({ "model": p["modelo"], "messages": msgs, "tools": tools, "tool_choice": "auto" });
            msg_openai(&postar(req_openai(p, chave, timeout_s), corpo, "rede")?)
        }
        Some("ollama") => {
            let corpo = json!({ "model": p["modelo"], "messages": msgs_para_ollama(msgs), "tools": tools, "stream": false,
                                "keep_alive": p["keep_alive"].as_str().unwrap_or("2m") });
            msg_ollama(&postar(req_ollama(p, timeout_s, 180), corpo, "ollama indisponivel")?)
        }
        Some("cli") => crate::claude_cli::chamar_tools(msgs, timeout_s, tools),
        outro => Err(erro(format!("api sem ferramentas: {outro:?}"), 0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_vira_descanso() {
        assert_eq!(erro_de_status(429, Some("50"), ""), erro("429 limite de uso", 50));
        assert_eq!(erro_de_status(429, Some("5"), "").descanso, 30);
        assert_eq!(erro_de_status(429, None, "").descanso, 900);
        assert_eq!(erro_de_status(403, None, &"é".repeat(300)).msg.chars().count(), "403 acesso: ".len() + 120);
        assert_eq!(erro_de_status(401, None, "x").descanso, 21600);
        assert_eq!(erro_de_status(503, None, "x"), erro("503 servidor fora", 300));
        assert_eq!(erro_de_status(400, None, "ruim"), erro("400 ruim", 0));
    }

    #[test]
    fn texto_das_respostas() {
        assert_eq!(texto_openai(&json!({"choices": [{"message": {"content": " oi "}}]})).unwrap(), "oi");
        assert_eq!(texto_openai(&json!({"choices": []})).unwrap_err().msg, "resposta sem conteudo");
        assert_eq!(texto_openai(&json!({"choices": [{"message": {"content": "  "}}]})).unwrap_err().msg, "resposta vazia");
        assert_eq!(texto_ollama(&json!({"message": {"content": "oi"}})).unwrap(), "oi");
        assert_eq!(texto_ollama(&json!({})).unwrap_err().msg, "resposta vazia");
    }

    #[test]
    fn api_desconhecida_nao_chama_nada() {
        assert_eq!(chamar(&json!({"api": "fax"}), &json!([]), "", 1).unwrap_err().descanso, 0);
        assert_eq!(chamar_tools(&json!({"api": "fax"}), &json!([]), "", 1, &json!([])).unwrap_err().descanso, 0);
    }

    #[test]
    fn mensagem_com_ferramenta_openai() {
        let call = json!([{"id": "a1", "type": "function", "function": {"name": "f", "arguments": "{\"x\":1}"}}]);
        let m = msg_openai(&json!({"choices": [{"message": {"content": null, "tool_calls": call, "reasoning": "segredo"}}]})).unwrap();
        assert_eq!(m, json!({"role": "assistant", "content": null, "tool_calls": call}));
        assert_eq!(msg_openai(&json!({"choices": [{"message": {"content": ""}}]})).unwrap_err().msg, "resposta vazia");
        assert_eq!(msg_openai(&json!({})).unwrap_err().msg, "resposta sem conteudo");
    }

    #[test]
    fn ollama_ida_e_volta() {
        let m = msg_ollama(&json!({"message": {"content": "", "tool_calls": [{"function": {"name": "f", "arguments": {"x": 1}}}]}})).unwrap();
        assert_eq!(m["tool_calls"][0], json!({"id": "call_0", "type": "function", "function": {"name": "f", "arguments": "{\"x\":1}"}}));
        let ida = msgs_para_ollama(&json!([{"role": "user", "content": "oi"}, m]));
        assert_eq!(ida[0], json!({"role": "user", "content": "oi"}));
        assert_eq!(ida[1]["tool_calls"][0], json!({"function": {"name": "f", "arguments": {"x": 1}}}));
        assert_eq!(msg_ollama(&json!({"message": {"content": " "}})).unwrap_err().msg, "resposta vazia");
    }
}
