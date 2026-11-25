//! Ponte Brani -> agentes externos (claude/codex/antigravity). Esqueleto seguro: DESLIGADA por padrao
//! (dados/ponte_agentes.json {"ligado": false}). Ver PONTE-AGENTES.md.
use serde_json::{json, Value};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

static FILA: Mutex<()> = Mutex::new(()); // uma tarefa por vez
const ALVOS: [&str; 3] = ["claude", "codex", "antigravity"];
const BARRADOS: [&str; 10] = ["whatsapp", "senha", "token", "api_key", "apikey", "cpf", "pix", "transfer", "pagamento", "login"];

fn config(dados: &Path) -> Value {
    std::fs::read_to_string(dados.join("ponte_agentes.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(json!({}))
}

/// Executavel do alvo: config "executaveis", variavel BRANIAC_<ALVO> ou PATH.
pub fn localizar(dados: &Path, alvo: &str) -> Option<PathBuf> {
    if let Some(p) = config(dados)["executaveis"][alvo].as_str().map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    if let Some(p) = std::env::var_os(format!("BRANIAC_{}", alvo.to_uppercase())).map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    std::env::split_paths(&std::env::var_os("PATH")?).flat_map(|d| [".exe", ".cmd", ""].map(|e| d.join(format!("{alvo}{e}")))).find(|p| p.is_file())
}

fn uso_hoje(dados: &Path, hoje: &str) -> u64 {
    let v: Value = std::fs::read_to_string(dados.join("ponte_agentes_uso.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(json!({}));
    if v["dia"] == hoje { v["n"].as_u64().unwrap_or(0) } else { 0 }
}

/// Estado para GET /api/ponte (sem executar nada).
pub fn estado(dados: &Path, hoje: &str) -> Value {
    let c = config(dados);
    json!({
        "ligado": c["ligado"].as_bool().unwrap_or(false),
        "teto_diario": c["teto_diario"].as_u64().unwrap_or(10),
        "usados_hoje": uso_hoje(dados, hoje),
        "alvos": ALVOS.iter().map(|a| json!({"alvo": a, "achado": localizar(dados, a).map(|p| p.display().to_string()), "sem_interface": *a != "antigravity"})).collect::<Vec<_>>(),
    })
}

/// Roda o pedido no CLI do alvo, na pasta explicita, com timeout; saida capturada (e dado, nunca instrucao).
pub fn executar(dados: &Path, hoje: &str, alvo: &str, pedido: &str, pasta: &Path, timeout: Duration) -> Result<String, String> {
    let c = config(dados);
    if !c["ligado"].as_bool().unwrap_or(false) {
        return Err("ponte desligada (dados/ponte_agentes.json)".into());
    }
    if !ALVOS.contains(&alvo) {
        return Err(format!("alvo desconhecido: {alvo}"));
    }
    let baixo = pedido.to_lowercase();
    if let Some(b) = BARRADOS.iter().find(|b| baixo.contains(**b)) {
        return Err(format!("pedido barrado pela trava (menciona '{b}')"));
    }
    if !pasta.is_dir() {
        return Err("pasta de trabalho inexistente".into());
    }
    let args: Vec<&str> = match alvo {
        "claude" => vec!["-p", pedido],
        "codex" => vec!["exec", "--sandbox", "workspace-write", pedido],
        _ => return Err("antigravity nao tem modo sem interface confirmado (ver PONTE-AGENTES.md)".into()),
    };
    let exe = localizar(dados, alvo).ok_or_else(|| format!("{alvo} nao encontrado"))?;
    let _vez = FILA.lock().unwrap_or_else(|e| e.into_inner());
    let n = uso_hoje(dados, hoje);
    if n >= c["teto_diario"].as_u64().unwrap_or(10) {
        return Err("teto diario atingido".into());
    }
    let _ = std::fs::write(dados.join("ponte_agentes_uso.json"), json!({"dia": hoje, "n": n + 1}).to_string());
    let mut filho = crate::oculto::cmd(exe).args(args).current_dir(pasta).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|e| e.to_string())?;
    let mut saida = filho.stdout.take().ok_or("sem saida")?;
    let leitor = std::thread::spawn(move || { let mut s = String::new(); let _ = saida.read_to_string(&mut s); s });
    let ini = Instant::now();
    loop {
        if filho.try_wait().map_err(|e| e.to_string())?.is_some() { break; }
        if ini.elapsed() > timeout {
            let _ = filho.kill();
            let _ = filho.wait();
            return Err("tempo esgotado".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(leitor.join().unwrap_or_default().chars().take(20000).collect())
}

#[cfg(test)]
mod testes {
    use super::*;

    fn pasta(nome: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("ponte_{nome}_{}", std::process::id()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn ponte_desligada_por_padrao_e_barra_segredo() {
        let d = pasta("a");
        assert!(executar(&d, "2026-11-23", "codex", "oi", &d, Duration::from_secs(1)).unwrap_err().contains("desligada"));
        std::fs::write(d.join("ponte_agentes.json"), r#"{"ligado":true}"#).unwrap();
        assert!(executar(&d, "2026-11-23", "codex", "mande no WhatsApp", &d, Duration::from_secs(1)).unwrap_err().contains("barrado"));
        assert!(executar(&d, "2026-11-23", "antigravity", "oi", &d, Duration::from_secs(1)).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn cli_falso_roda_em_fila_com_teto_e_timeout() {
        let d = pasta("b");
        let falso = d.join("codex.cmd");
        std::fs::write(&falso, "@echo off\r\necho FEITO %*\r\n").unwrap();
        std::fs::write(d.join("ponte_agentes.json"), json!({"ligado": true, "teto_diario": 1, "executaveis": {"codex": falso.display().to_string()}}).to_string()).unwrap();
        let s = executar(&d, "2026-11-23", "codex", "faca algo", &d, Duration::from_secs(10)).unwrap();
        assert!(s.contains("FEITO exec"), "{s}");
        assert!(executar(&d, "2026-11-23", "codex", "de novo", &d, Duration::from_secs(10)).unwrap_err().contains("teto"));
        assert_eq!(estado(&d, "2026-11-23")["usados_hoje"], 1);
        let lento = d.join("claude.cmd");
        std::fs::write(&lento, "@echo off\r\nping -n 6 127.0.0.1 >nul\r\n").unwrap();
        std::fs::write(d.join("ponte_agentes.json"), json!({"ligado": true, "executaveis": {"claude": lento.display().to_string()}}).to_string()).unwrap();
        assert!(executar(&d, "2026-11-24", "claude", "x", &d, Duration::from_millis(500)).unwrap_err().contains("tempo"));
    }
}
