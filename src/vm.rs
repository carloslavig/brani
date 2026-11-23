//! Controle da VM Linux (WSL) e das abas do navegador dela: cada aba e um agente.
//! Escrito pelo Otto (NexoDev, T054), revisado e ajustado pelo Claude.
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{json, Value};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

// constantes do projeto
const DISTRO: &str = "BraniacVM";
pub const CDP: &str = "http://127.0.0.1:9333";
pub const TELA: &str = "http://127.0.0.1:6090";

/// Converte saída UTF‑16LE (Windows) para String UTF‑8.
fn utf16le_to_string(buf: &[u8]) -> Result<String, String> {
    if buf.len() % 2 != 0 {
        return Err("Tamanho inválido para UTF-16LE".into());
    }
    let utf16: Vec<u16> = buf
        .chunks(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16(&utf16).map_err(|e| format!("Erro UTF‑16: {}", e))
}

/// Interpreta a saída de `wsl.exe -l -v` e devolve (existe, ligada).
fn parse_wsl_status(output: &str) -> (bool, bool) {
    for line in output.lines() {
        if line.contains(DISTRO) {
            let exists = true;
            let running = line.contains("Running");
            return (exists, running);
        }
    }
    (false, false)
}

/// Filtra a lista de abas retornada pelo CDP, mantendo apenas as do tipo "page".
fn filtrar_abas(json: &Value) -> Value {
    let mut res = Vec::new();
    if let Some(arr) = json.as_array() {
        for aba in arr {
            if aba.get("type") == Some(&json!("page")) {
                let id = aba.get("id").cloned().unwrap_or_default();
                let titulo = aba.get("title").cloned().unwrap_or_default();
                let url = aba.get("url").cloned().unwrap_or_default();
                let agente = json!(""); // será preenchido depois
                res.push(json!({ "id": id, "titulo": titulo, "url": url, "agente": agente }));
            }
        }
    }
    json!(res)
}

/// Valida se a URL possui esquema http ou https.
pub fn validar_url(url: &str) -> Result<(), String> {
    let url = url.trim();
    if url.starts_with("http://") || url.starts_with("https://") {
        Ok(())
    } else {
        Err("URL deve iniciar com http:// ou https://".into())
    }
}

/// Valida se o ID contém apenas letras e números.
fn validar_id(id: &str) -> Result<(), String> {
    if id.chars().all(|c| c.is_ascii_alphanumeric()) && !id.is_empty() {
        Ok(())
    } else {
        Err("ID inválido: deve conter apenas A‑Z a‑z 0‑9".into())
    }
}

/// Carrega o mapa id → agente do arquivo JSON.
fn carregar_mapa(dados: &Path) -> Result<HashMap<String, String>, String> {
    let dados = &dados.join("vm_abas.json");
    if !dados.exists() {
        return Ok(HashMap::new());
    }
    let mut f = fs::File::open(dados).map_err(|e| format!("Abrir {}: {}", dados.display(), e))?;
    let mut txt = String::new();
    f.read_to_string(&mut txt)
        .map_err(|e| format!("Ler {}: {}", dados.display(), e))?;
    serde_json::from_str(&txt).map_err(|e| format!("Parse JSON {}: {}", dados.display(), e))
}

/// Salva o mapa id → agente no arquivo JSON.
fn salvar_mapa(dados: &Path, mapa: &HashMap<String, String>) -> Result<(), String> {
    fs::create_dir_all(dados).map_err(|e| format!("Criar {}: {}", dados.display(), e))?;
    let dados = &dados.join("vm_abas.json");
    let txt = serde_json::to_string_pretty(mapa)
        .map_err(|e| format!("Serializar mapa: {}", e))?;
    let mut f = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(dados)
        .map_err(|e| format!("Abrir {}: {}", dados.display(), e))?;
    f.write_all(txt.as_bytes())
        .map_err(|e| format!("Escrever {}: {}", dados.display(), e))
}

/// Estado da VM.
pub fn estado() -> Result<Value, String> {
    let out = Command::new("wsl.exe")
        .arg("-l")
        .arg("-v")
        .output()
        .map_err(|e| format!("Exec wsl: {}", e))?;

    // wsl.exe retorna UTF‑16LE
    let txt = utf16le_to_string(&out.stdout)?;
    let (existe, ligada) = parse_wsl_status(&txt);
    Ok(json!({ "existe": existe, "ligada": ligada }))
}

/// Liga a VM sem janela. O ligar.sh fica em primeiro plano de proposito (o WSL desliga a
/// distro quando o ultimo processo iniciado pelo Windows termina), entao nao esperamos ele.
pub fn ligar() -> Result<Value, String> {
    let mut c = Command::new("wsl.exe");
    c.args(["-d", DISTRO, "-u", "root", "--", "/opt/braniac/ligar.sh"])
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    c.creation_flags(0x08000000); // CREATE_NO_WINDOW
    c.spawn().map_err(|e| format!("Exec wsl ligar: {}", e))?;
    crate::abas_fixas::depois_de_ligar(120); // o Chromium reiniciou: as abas dos servicos voltam sozinhas
    Ok(json!({ "ok": true }))
}

/// Desligar a VM.
pub fn desligar() -> Result<Value, String> {
    let status = Command::new("wsl.exe")
        .arg("--terminate")
        .arg(DISTRO)
        .status()
        .map_err(|e| format!("Exec wsl desligar: {}", e))?;

    if status.success() {
        Ok(json!({ "ok": true }))
    } else {
        Err(format!("wsl retornou código {}", status))
    }
}

/// Lista abas abertas, incluindo agente.
pub fn abas(dados: &Path) -> Result<Value, String> {
    let resp = ureq::get(&format!("{}/json/list", CDP))
        .timeout(std::time::Duration::from_secs(3))
        .call()
        .map_err(|e| format!("HTTP GET abas: {}", e))?;

    let txt = resp
        .into_string()
        .map_err(|e| format!("Ler corpo HTTP: {}", e))?;
    let json: Value = serde_json::from_str(&txt).map_err(|e| format!("Parse JSON: {}", e))?;

    let mut lista = filtrar_abas(&json);
    // carrega mapa agentes
    let mapa = carregar_mapa(dados)?;
    if let Some(arr) = lista.as_array_mut() {
        for aba in arr.iter_mut() {
            if let Some(id) = aba.get("id").and_then(|v| v.as_str()) {
                if let Some(ag) = mapa.get(id) {
                    aba["agente"] = json!(ag);
                }
            }
        }
    }
    Ok(lista)
}

/// Abre nova aba e persiste agente.
pub fn abrir_aba(dados: &Path, agente: &str, url: &str) -> Result<Value, String> {
    validar_url(url)?;
    criar_aba(dados, agente, url)
}

/// Aba em branco para o agente (ele navega depois, ja com o endereco validado).
pub fn aba_em_branco(dados: &Path, agente: &str) -> Result<Value, String> {
    criar_aba(dados, agente, "about:blank")
}

fn criar_aba(dados: &Path, agente: &str, url: &str) -> Result<Value, String> {
    if crate::wpp_midia::gravando() {
        return Err("gravando um audio no WhatsApp: abrir aba agora corta o audio; tente de novo em instantes".into());
    }

    let resp = ureq::put(&format!("{}/json/new?{}", CDP, url))
        .timeout(std::time::Duration::from_secs(3))
        .call()
        .map_err(|e| format!("HTTP PUT abrir aba: {}", e))?;

    let txt = resp
        .into_string()
        .map_err(|e| format!("Ler corpo HTTP: {}", e))?;
    let json: Value = serde_json::from_str(&txt).map_err(|e| format!("Parse JSON: {}", e))?;
    let id = json
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or("Resposta sem campo id")?;

    // persiste agente
    let mut mapa = carregar_mapa(dados)?;
    mapa.insert(id.to_string(), agente.to_string());
    salvar_mapa(dados, &mapa)?;

    Ok(json)
}

/// Traz a aba para a frente dentro da tela da VM (para a pessoa ver o que o agente esta fazendo).
pub fn mostrar_aba(id: &str) -> Result<Value, String> {
    validar_id(id)?;
    ureq::get(&format!("{}/json/activate/{}", CDP, id))
        .timeout(std::time::Duration::from_secs(3))
        .call()
        .map_err(|e| format!("HTTP GET mostrar aba: {}", e))?;
    Ok(json!({ "ok": true }))
}

/// Fecha aba e remove agente do mapa.
pub fn fechar_aba(dados: &Path, id: &str) -> Result<Value, String> {
    validar_id(id)?;

    let resp = ureq::get(&format!("{}/json/close/{}", CDP, id))
        .timeout(std::time::Duration::from_secs(3))
        .call()
        .map_err(|e| format!("HTTP GET fechar aba: {}", e))?;

    // mesmo que não haja corpo, consideramos sucesso se código 200
    if resp.status() == 200 {
        let mut mapa = carregar_mapa(dados)?;
        mapa.remove(id);
        salvar_mapa(dados, &mapa)?;
        Ok(json!({ "ok": true }))
    } else {
        Err(format!("HTTP {} ao fechar aba", resp.status()))
    }
}

/* ==================== Funções puras (testáveis) ==================== */

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_utf16le_to_string() {
        // "ABC" em UTF‑16LE = 41 00 42 00 43 00
        let data = [0x41, 0x00, 0x42, 0x00, 0x43, 0x00];
        let s = utf16le_to_string(&data).unwrap();
        assert_eq!(s, "ABC");
    }

    #[test]
    fn test_parse_wsl_status() {
        let txt = "  Debian    Running   2";
        let (ex, lig) = parse_wsl_status(txt);
        assert!(!ex);
        assert!(!lig);

        let txt = "  BraniacVM  Stopped   2\n  Other  Running   2";
        let (ex, lig) = parse_wsl_status(txt);
        assert!(ex);
        assert!(!lig);

        let txt = "  BraniacVM  Running   2";
        let (ex, lig) = parse_wsl_status(txt);
        assert!(ex);
        assert!(lig);
    }

    #[test]
    fn test_filtrar_abas() {
        let input = json!([
            { "id": "1", "type": "page", "title": "A", "url": "http://a" },
            { "id": "2", "type": "other", "title": "B", "url": "http://b" }
        ]);
        let out = filtrar_abas(&input);
        let arr = out.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["id"], "1");
        assert_eq!(arr[0]["agente"], "");
    }

    #[test]
    fn test_validar_url() {
        assert!(validar_url("http://ex.com").is_ok());
        assert!(validar_url("https://ex.com/path").is_ok());
        assert!(validar_url("ftp://ex.com").is_err());
        assert!(validar_url("ex.com").is_err());
    }

    #[test]
    fn test_validar_id() {
        assert!(validar_id("abc123").is_ok());
        assert!(validar_id("ABCdef").is_ok());
        assert!(validar_id("").is_err());
        assert!(validar_id("abc-123").is_err());
    }
}
