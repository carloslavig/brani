//! Claude como IA de confianca, chamado pela linha de comando (`claude -p`, assinatura da pessoa).
//! Porte de providers.chamar_claude. Ele nao tem "function calling" por aqui: com ferramentas, o pedido vai em
//! texto e ele responde com um JSON de chamada, que o laco do agente ja sabe executar (texto_ferramenta.rs).
use crate::provedores::ErroProvedor;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn erro(msg: impl Into<String>, descanso: u64) -> ErroProvedor {
    ErroProvedor { msg: msg.into(), descanso }
}

/// Executavel do Claude: BRANIAC_CLAUDE ou o primeiro `claude` do PATH.
pub fn localizar() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("BRANIAC_CLAUDE").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    let pastas = std::env::var_os("PATH")?;
    std::env::split_paths(&pastas).flat_map(|d| ["claude.exe", "claude.cmd", "claude"].map(|n| d.join(n))).find(|p| p.is_file())
}

fn corta(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// A conversa em texto corrido, do jeito que o Claude le pela entrada padrao.
pub fn prompt_de(msgs: &Value) -> String {
    let mut linhas = Vec::new();
    for m in msgs.as_array().into_iter().flatten() {
        let texto = m["content"].as_str().unwrap_or("");
        for c in m["tool_calls"].as_array().into_iter().flatten() {
            linhas.push(format!("ASSISTENTE pediu a ferramenta {} com {}", c["function"]["name"].as_str().unwrap_or("?"), c["function"]["arguments"].as_str().unwrap_or("{}")));
        }
        if texto.is_empty() {
            continue;
        }
        let quem = match m["role"].as_str().unwrap_or("") {
            "system" => "INSTRUCOES",
            "assistant" => "ASSISTENTE",
            "tool" => "RESULTADO DE FERRAMENTA",
            _ => "USUARIO",
        };
        linhas.push(format!("{quem}: {texto}"));
    }
    linhas.join("\n\n")
}

/// Como pedir uma ferramenta por texto: lista o que existe e o formato exato da resposta.
pub fn instrucao_de_ferramentas(tools: &Value) -> String {
    let lista: Vec<String> = tools.as_array().into_iter().flatten().map(|t| {
        let f = &t["function"];
        format!("- {}: {} Parametros: {}", f["name"].as_str().unwrap_or(""), corta(f["description"].as_str().unwrap_or(""), 300), f["parameters"]["properties"])
    }).collect();
    format!("FERRAMENTAS DISPONIVEIS (so estas):\n{}\n\nSe precisar de uma ferramenta para responder, responda SOMENTE com um JSON nesta forma, sem mais nada: \
{{\"tool\": \"nome\", \"arguments\": {{...}}}}\nSe ja tem o que precisa (inclusive um RESULTADO DE FERRAMENTA acima), responda ao USUARIO em portugues, curto, sem JSON. \
So diga que fez algo depois de o RESULTADO DE FERRAMENTA confirmar.", lista.join("\n"))
}

/// Roda `claude -p` com o pedido pela entrada padrao. Nao edita nada nem recebe permissao de ferramenta.
fn rodar(exe: &Path, prompt: &str, pasta: &Path, timeout_s: u64) -> Result<String, ErroProvedor> {
    let mut c = crate::oculto::cmd(exe);
    // se o Braniac foi ligado de dentro de uma sessao do Claude Code, o filho nao pode achar que esta aninhado nela
    c.env_remove("CLAUDECODE").env_remove("CLAUDE_CODE_ENTRYPOINT");
    c.arg("-p").current_dir(pasta).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let mut filho = c.spawn().map_err(|e| erro(format!("nao consegui acionar o Claude: {e}"), 3600))?;
    if let Some(mut entrada) = filho.stdin.take() {
        let _ = entrada.write_all(prompt.as_bytes());
    }
    let mut saida = filho.stdout.take();
    let leitor = std::thread::spawn(move || {
        let mut t = String::new();
        if let Some(s) = saida.as_mut() {
            let _ = s.read_to_string(&mut t);
        }
        t
    });
    let limite = Instant::now() + Duration::from_secs(timeout_s);
    let status = loop {
        match filho.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if Instant::now() < limite => std::thread::sleep(Duration::from_millis(200)),
            _ => {
                let _ = filho.kill();
                let _ = filho.wait();
                return Err(erro("o Claude demorou demais", 300));
            }
        }
    };
    let texto = leitor.join().unwrap_or_default().trim().to_string();
    if !status.success() || texto.is_empty() {
        return Err(erro(format!("o Claude falhou: {}", corta(&texto, 150)), 3600));
    }
    Ok(texto)
}

fn pasta_de_trabalho() -> PathBuf {
    // pasta dos dados da pessoa: e la que ficam as instrucoes dela para o Claude, se houver
    std::env::var_os("BRANIAC_DADOS").map(PathBuf::from).filter(|p| p.is_dir()).unwrap_or_else(std::env::temp_dir)
}

pub fn chamar(msgs: &Value, timeout_s: u64) -> Result<String, ErroProvedor> {
    let exe = localizar().ok_or_else(|| erro("Claude nao encontrado neste computador", 3600))?;
    let prompt = format!("{}\n\nResponda apenas a ultima mensagem do USUARIO, sem usar ferramentas.", prompt_de(msgs));
    rodar(&exe, &prompt, &pasta_de_trabalho(), timeout_s.max(300))
}

/// Passo do agente: devolve a resposta como mensagem de assistente (o JSON de chamada, se houver, vem no texto).
pub fn chamar_tools(msgs: &Value, timeout_s: u64, tools: &Value) -> Result<Value, ErroProvedor> {
    let exe = localizar().ok_or_else(|| erro("Claude nao encontrado neste computador", 3600))?;
    let prompt = format!("{}\n\n{}", prompt_de(msgs), instrucao_de_ferramentas(tools));
    Ok(json!({ "role": "assistant", "content": rodar(&exe, &prompt, &pasta_de_trabalho(), timeout_s.max(300))? }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversa_vira_texto_com_ferramentas_e_resultados() {
        let msgs = json!([
            {"role": "system", "content": "S"}, {"role": "user", "content": "quanto?"},
            {"role": "assistant", "content": null, "tool_calls": [{"function": {"name": "saldo", "arguments": "{\"a\":1}"}}]},
            {"role": "tool", "tool_call_id": "1", "content": "150"}, {"role": "assistant", "content": "Deu 150."}]);
        assert_eq!(prompt_de(&msgs), "INSTRUCOES: S\n\nUSUARIO: quanto?\n\nASSISTENTE pediu a ferramenta saldo com {\"a\":1}\n\nRESULTADO DE FERRAMENTA: 150\n\nASSISTENTE: Deu 150.");
    }

    #[test]
    fn instrucao_lista_as_ferramentas_e_o_formato_que_o_laco_entende() {
        let t = instrucao_de_ferramentas(&json!([{"function": {"name": "consultar_banco", "description": "Consulta.", "parameters": {"properties": {"busca": {"type": "string"}}}}}]));
        assert!(t.contains("- consultar_banco: Consulta. Parametros: {\"busca\":{\"type\":\"string\"}}"));
        // o exemplo de resposta e exatamente o que texto_ferramenta::chamada_em_texto aceita
        assert!(crate::texto_ferramenta::chamada_em_texto("{\"tool\": \"consultar_banco\", \"arguments\": {\"busca\": \"x\"}}", Some(&["consultar_banco"])).is_some());
        assert!(t.contains("{\"tool\": \"nome\", \"arguments\": {...}}"));
    }
}
