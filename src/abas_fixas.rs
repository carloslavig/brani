//! Abas fixas da VM: quando o Chromium reinicia (recompilacao, /api/vm/ligar) as abas dos servicos somem (os logins ficam nos cookies).
//! Ao ligar a VM, e uma vez ao subir o programa, reabre cada aba que FALTA, com o nome de agente certo (whatsapp, airbnb, chatgpt,
//! gemini, grok, deepseek) mais as de `dados/abas_fixas.json` ({"extras":[{"agente":"telegram_web","url":"https://..."}],"desligadas":["grok"]}).
//! Aba que ja existe com aquele agente nunca e duplicada. So abre a pagina: NUNCA faz login, nunca digita nada.
use serde_json::Value;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

const PADRAO: &[(&str, &str)] = &[
    ("whatsapp", "https://web.whatsapp.com/"),
    ("airbnb", "https://www.airbnb.com.br/hosting/inbox"),
    ("chatgpt", "https://chatgpt.com/"),
    ("gemini", "https://gemini.google.com/app"),
    ("grok", "https://grok.com/"),
    ("deepseek", "https://chat.deepseek.com/"),
];

static OCUPADO: AtomicBool = AtomicBool::new(false);

/// A lista final (agente, url): padrao + extras - desligadas. Extra com o nome de um padrao troca o endereco dele.
pub fn lista(dados: &Path) -> Vec<(String, String)> {
    let cfg: Value = std::fs::read_to_string(dados.join("abas_fixas.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    let off: Vec<String> = cfg["desligadas"].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(|s| s.to_lowercase())).collect();
    let mut l: Vec<(String, String)> = PADRAO.iter().map(|(a, u)| (a.to_string(), u.to_string())).collect();
    for e in cfg["extras"].as_array().into_iter().flatten() {
        let (a, u) = (e["agente"].as_str().unwrap_or("").trim().to_lowercase(), e["url"].as_str().unwrap_or("").trim().to_string());
        if a.is_empty() || crate::vm::validar_url(&u).is_err() {
            continue;
        }
        match l.iter_mut().find(|(x, _)| *x == a) {
            Some(x) => x.1 = u,
            None => l.push((a, u)),
        }
    }
    l.retain(|(a, _)| !off.contains(a));
    l
}

/// As que faltam: nenhuma aba viva com aquele agente.
pub fn faltando(vivas: &Value, lista: &[(String, String)]) -> Vec<(String, String)> {
    let tem = |a: &str| vivas.as_array().into_iter().flatten().any(|x| x["agente"].as_str().is_some_and(|g| g.eq_ignore_ascii_case(a)));
    lista.iter().filter(|(a, _)| !tem(a)).cloned().collect()
}

/// Reconcilia: reusa o que ja existe (pela URL), fecha repetidas e abre so o que falta. Devolve os agentes abertos.
/// Com a VM fora do ar devolve erro (nada e aberto).
pub fn garantir(dados: &Path) -> Result<Vec<String>, String> {
    let r = crate::reconcilia_abas::passo(dados)?;
    Ok(r["abertas"].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(String::from)).collect())
}

/// Espera o navegador da VM responder (ate `espera_s`) e entao reconcilia as abas. Uma execucao por vez.
pub fn depois_de_ligar(espera_s: u64) {
    if OCUPADO.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        let dados = crate::pasta_dados();
        for _ in 0..(espera_s / 3).max(1) {
            if crate::vm::abas(&dados).is_ok() {
                std::thread::sleep(std::time::Duration::from_secs(4)); // deixa o Chromium terminar de subir
                let _ = garantir(&dados);
                break;
            }
            std::thread::sleep(std::time::Duration::from_secs(3));
        }
        OCUPADO.store(false, Ordering::SeqCst);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn so_abre_o_que_falta_e_nunca_duplica() {
        let l = lista(Path::new("pasta-que-nao-existe"));
        assert_eq!(l.len(), 6);
        let vivas = json!([{ "id": "1", "agente": "whatsapp" }, { "id": "2", "agente": "ChatGPT" }, { "id": "3" }]);
        let f: Vec<String> = faltando(&vivas, &l).into_iter().map(|x| x.0).collect();
        assert_eq!(f, vec!["airbnb", "gemini", "grok", "deepseek"]);
        assert!(faltando(&json!([]), &[]).is_empty());
    }

    #[test]
    fn lista_configuravel_extras_e_desligadas() {
        let d = std::env::temp_dir().join(format!("braniac-fixas-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("abas_fixas.json"), json!({ "extras": [{ "agente": "Telegram_Web", "url": "https://web.telegram.org/" }, { "agente": "x", "url": "javascript:1" }], "desligadas": ["grok"] }).to_string()).unwrap();
        let l = lista(&d);
        assert!(l.iter().any(|(a, _)| a == "telegram_web") && !l.iter().any(|(a, _)| a == "grok" || a == "x"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
