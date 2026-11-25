//! Voz: transforma texto em audio (WAV). Configuracao em dados/voz.json: {"motor": "gemini" | "local", "voz": "Umbriel", "estilo": "..."}.
//! Cada agente pode ter a sua voz: "agentes": {"alma": {"voz": "Leda", "estilo": "...", "genero": "F"}} (chave = nome do agente em
//! minusculas e sem acento); o que o agente nao define vem da voz geral. "genero" escolhe a voz local (F = feminina).
//! Sem o arquivo, o motor e "local" (voz do Windows: nada sai do PC). O Gemini e a voz natural; se falhar (cota, rede, sem chave)
//! ou se o texto nao puder ir a nuvem, fala a voz local. Chave: GEMINI_TTS_KEY* / GEMINI_API_KEY*, lidas pelo leitor de chaves.
use crate::texto_pt::sem_acento;
use crate::{base64, wav};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const MODELOS: [&str; 3] = ["gemini-3.1-flash-tts-preview", "gemini-2.5-flash-preview-tts", "gemini-2.5-pro-preview-tts"];
const CHAVES: [&str; 7] = ["GEMINI_TTS_KEY", "GEMINI_TTS_KEY_2", "GEMINI_TTS_KEY_3", "GEMINI_TTS_KEY_4", "GEMINI_TTS_KEY_5", "GEMINI_API_KEY", "GEMINI_API_KEY_2"];
const ESTILO: &str = "Fale em português do Brasil em tom sério, calmo e sóbrio, como um assistente profissional e seguro de si: \
                      voz firme, ritmo pausado e natural, sem empolgação, sem entusiasmo exagerado e sem soar sorridente";
// shortcut: fala so os primeiros 1500 caracteres (uns 90 s); texto maior precisa ser dividido em partes e emendado
const MAXIMO: usize = 1500;

static DESCANSO: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None); // modelo -> ate quando evitar (sem cota ou erro)

/// Tira o que nao se fala: links, marcas de markdown, emojis, marcadores de lista. Exclamacao vira ponto (voz mais seria).
pub fn limpar(texto: &str) -> String {
    let linhas: Vec<String> = texto.lines().map(|l| {
        let l = l.trim_start();
        let l = ["- ", "• ", "* "].iter().find_map(|m| l.strip_prefix(m)).unwrap_or(l);
        l.split_whitespace().filter(|w| !(w.starts_with("http://") || w.starts_with("https://") || w.starts_with("www."))).collect::<Vec<_>>().join(" ")
    }).collect();
    let t = linhas.join(" ").replace("**", "").replace("__", "").replace(['`', '«', '»'], "");
    let t: String = t.chars().filter(|c| c.is_ascii() || c.is_alphanumeric() || matches!(c, '–' | '—' | '“' | '”' | '…' | 'º' | 'ª')).collect();
    let mut limpo = String::new();
    for c in t.chars().map(|c| if c == '!' { '.' } else { c }) {
        if !(c == '.' && limpo.ends_with('.')) { // "!!!" vira um ponto so
            limpo.push(c);
        }
    }
    let palavras: Vec<&str> = limpo.split_whitespace().collect();
    let c: Vec<char> = palavras.join(" ").chars().collect();
    if c.len() <= MAXIMO {
        return c.iter().collect();
    }
    let fim = (0..MAXIMO).rev().find(|&i| matches!(c[i], '.' | '?')).map_or(MAXIMO, |i| i + 1); // corta no fim de uma frase
    c[..fim].iter().collect()
}

/// Modelo que LEU o estilo em voz alta devolve audio longo demais para o texto (a fala normal tem ~2,5 palavras por segundo).
fn plausivel(texto: &str, segundos: f64) -> bool {
    segundos <= texto.split_whitespace().count().max(1) as f64 / 1.3 + 3.5
}

fn gemini(ler_chave: &dyn Fn(&str) -> String, texto: &str, voz: &str, estilo: &str) -> Result<(Vec<u8>, String), String> {
    let mut chaves: Vec<String> = CHAVES.iter().map(|n| ler_chave(n).trim().to_string()).filter(|k| !k.is_empty()).collect();
    chaves.dedup();
    if chaves.is_empty() {
        return Err("sem chave do Gemini".into());
    }
    let corpo = json!({ "contents": [{ "parts": [{ "text": format!("{estilo}: {texto}") }] }],
        "generationConfig": { "responseModalities": ["AUDIO"], "speechConfig": { "voiceConfig": { "prebuiltVoiceConfig": { "voiceName": voz } } } } });
    let descansa = |modelo: &str, s: u64| {
        if let Ok(mut d) = DESCANSO.lock() {
            d.get_or_insert_with(HashMap::new).insert(modelo.to_string(), Instant::now() + Duration::from_secs(s));
        }
    };
    let mut erros = Vec::new();
    for modelo in MODELOS {
        if DESCANSO.lock().ok().and_then(|d| d.as_ref().and_then(|m| m.get(modelo).copied())).is_some_and(|ate| ate > Instant::now()) {
            erros.push(format!("{modelo}: em descanso"));
            continue;
        }
        let url = format!("https://generativelanguage.googleapis.com/v1beta/models/{modelo}:generateContent");
        let mut r = Err("sem tentativa".to_string());
        for chave in &chaves { // a chave seguinte (outro projeto) tem cota propria: so entra se a anterior estourar
            r = match ureq::post(&url).set("x-goog-api-key", chave).timeout(Duration::from_secs(60)).send_json(corpo.clone()) {
                Ok(resp) => resp.into_json::<Value>().map_err(|_| "resposta ilegivel".to_string()),
                Err(ureq::Error::Status(429, _)) => { r = Err("HTTP 429".into()); continue }
                Err(ureq::Error::Status(codigo, _)) => Err(format!("HTTP {codigo}")), // o corpo do erro nao vai para o registro
                Err(_) => Err("rede".into()),
            };
            break;
        }
        let v = match r {
            Ok(v) => v,
            Err(e) => {
                descansa(modelo, if e == "HTTP 429" { 900 } else { 120 });
                erros.push(format!("{modelo}: {e}"));
                continue;
            }
        };
        let Some(pcm) = v["candidates"][0]["content"]["parts"][0]["inlineData"]["data"].as_str().and_then(|d| base64::decodificar(d).ok()).filter(|p| !p.is_empty()) else {
            erros.push(format!("{modelo}: resposta vazia"));
            continue;
        };
        let audio = wav::wav(&pcm, 24000);
        if !plausivel(texto, wav::segundos(&audio)) {
            erros.push(format!("{modelo}: audio longo demais (leu o estilo?)"));
            continue;
        }
        return Ok((audio, format!("gemini:{modelo}:{voz}")));
    }
    Err(erros.join("; "))
}

/// Voz do proprio Windows (SAPI), em pt-BR quando houver uma instalada. Tudo local.
fn local(texto: &str, feminina: bool) -> Result<(Vec<u8>, String), String> {
    const SCRIPT: &str = "[Console]::InputEncoding=[Text.Encoding]::UTF8; Add-Type -AssemblyName System.Speech; \
        $s=New-Object System.Speech.Synthesis.SpeechSynthesizer; \
        try{$s.SelectVoiceByHints($env:BRANIAC_GENERO,'NotSet',0,[Globalization.CultureInfo]'pt-BR')}catch{}; \
        $s.SetOutputToWaveFile($env:BRANIAC_WAV); $s.Speak([Console]::In.ReadToEnd()); $s.Dispose()";
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let arq = std::env::temp_dir().join(format!("braniac-voz-{}-{}.wav", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    let mut c = crate::oculto::cmd("powershell");
    c.args(["-NoProfile", "-NonInteractive", "-Command", SCRIPT]).env("BRANIAC_WAV", &arq).env("BRANIAC_GENERO", if feminina { "Female" } else { "Male" }).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let mut filho = c.spawn().map_err(|e| format!("voz local: {e}"))?;
    if let Some(mut entrada) = filho.stdin.take() {
        let _ = entrada.write_all(texto.as_bytes());
    }
    let ok = filho.wait().map(|s| s.success()).unwrap_or(false);
    let audio = std::fs::read(&arq);
    let _ = std::fs::remove_file(&arq);
    match audio {
        Ok(a) if ok && a.len() > 44 => Ok((a, "local:windows".into())),
        _ => Err("a voz do Windows não respondeu".into()),
    }
}

/// A voz de um agente: o que ele define em "agentes" ganha; o resto vem da voz geral. "" = Jarvis (a voz geral).
fn escolha(cfg: &Value, agente: &str) -> (String, String, bool) {
    let ag = &cfg["agentes"][sem_acento(agente).as_str()];
    let pega = |k: &str| ag[k].as_str().or(cfg[k].as_str());
    (pega("voz").unwrap_or("Achird").to_string(), pega("estilo").unwrap_or(ESTILO).to_string(), pega("genero") == Some("F"))
}

/// Um motor pelo nome, para `vozes_agentes::falar` (voz escolhida por agente). So o Gemini existe aqui; Kokoro, Piper e Edge
/// devolvem erro e a fala cai na reserva ou na ordem de sempre. `estilo` vazio = o estilo padrao.
pub fn gerar(ler_chave: &dyn Fn(&str) -> String, motor: &str, voz: &str, texto: &str, estilo: &str) -> Result<Vec<u8>, String> {
    let texto = limpar(texto);
    match motor {
        _ if texto.is_empty() => Err("nada para falar".into()),
        "gemini" => gemini(ler_chave, &texto, voz, if estilo.is_empty() { ESTILO } else { estilo }).map(|(audio, _)| audio),
        outro => Err(format!("o motor {outro} ainda não foi ligado no Braniac em Rust")),
    }
}

/// (WAV, motor que falou). `permitir_nuvem` = false quando o texto nao pode sair do PC: so a voz local fala.
pub fn falar_wav(dados: &Path, ler_chave: &dyn Fn(&str) -> String, texto: &str, permitir_nuvem: bool, agente: &str) -> Result<(Vec<u8>, String), String> {
    let texto = limpar(texto);
    if texto.is_empty() {
        return Err("nada para falar".into());
    }
    let cfg: Value = std::fs::read_to_string(dados.join("voz.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let (voz, estilo, feminina) = escolha(&cfg, agente);
    if permitir_nuvem && cfg["motor"] == "gemini" {
        match gemini(ler_chave, &texto, &voz, &estilo) {
            Ok(r) => return Ok(r),
            Err(e) => eprintln!("voz: gemini falhou ({e}); falando com a voz local"),
        }
    }
    local(&texto, feminina)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limpa_o_que_nao_se_fala_e_corta_no_fim_da_frase() {
        assert_eq!(limpar("☀️ **Bom dia!!!** Veja https://x.com/y agora.\n- item `um`\n• item «dois»"), "Bom dia. Veja agora. item um item dois");
        assert_eq!(limpar("Ação às 9h — tudo certo?"), "Ação às 9h — tudo certo?");
        assert_eq!(limpar("  🙂 "), "");
        let longo = format!("{}. {}", "a".repeat(1000), "b".repeat(1000));
        assert_eq!(limpar(&longo).chars().count(), 1001); // para no ponto, nao no meio da palavra
        assert!(plausivel("uma frase curta de teste", 4.0) && !plausivel("uma frase curta de teste", 30.0));
    }

    #[test]
    fn sem_chave_ou_sem_nuvem_nao_chama_o_gemini() {
        assert_eq!(gemini(&|_| String::new(), "oi", "Achird", ESTILO).unwrap_err(), "sem chave do Gemini");
        assert_eq!(falar_wav(Path::new("."), &|_| panic!("leu chave"), " 🙂 ", true, "").unwrap_err(), "nada para falar");
        let cfg = json!({ "voz": "Umbriel", "agentes": { "alma": { "voz": "Leda", "genero": "F" }, "cabo tito": { "estilo": "Firme." } } });
        assert_eq!(escolha(&cfg, "Alma"), ("Leda".into(), ESTILO.into(), true));
        assert_eq!(escolha(&cfg, "Cabo Tito"), ("Umbriel".into(), "Firme.".into(), false)); // o que falta vem da voz geral
        assert_eq!(escolha(&cfg, ""), ("Umbriel".into(), ESTILO.into(), false));
        assert_eq!(escolha(&cfg, "Desconhecido").0, "Umbriel");
    }

    #[cfg(windows)]
    #[test]
    fn a_voz_local_devolve_um_wav() {
        let d = std::env::temp_dir(); // sem voz.json: motor local, a chave nem e lida
        let (a, motor) = falar_wav(&d, &|_| panic!("leu chave"), "Teste de voz.", true, "").unwrap();
        assert!(a.starts_with(b"RIFF") && a.len() > 1000 && motor == "local:windows");
    }

    #[test]
    fn motor_que_nao_existe_aqui_devolve_erro_sem_chamar_nada() {
        let sem_chave = |_: &str| String::new();
        for m in ["kokoro", "piper", "edge"] {
            assert!(gerar(&sem_chave, m, "x", "olá", "").unwrap_err().contains("ainda não foi ligado"), "{m}");
        }
        assert_eq!(gerar(&sem_chave, "gemini", "Leda", "olá", "").unwrap_err(), "sem chave do Gemini");
        assert_eq!(gerar(&sem_chave, "gemini", "Leda", " 🙂 ", "").unwrap_err(), "nada para falar");
    }
}
