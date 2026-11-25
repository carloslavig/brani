//! Ouvir: audio -> texto. O Braniac nao embute um reconhecedor de fala: chama o transcritor LOCAL configurado em dados/ouvir.json,
//! {"comando": ["programa", "arg", "{audio}"]}: o programa recebe o caminho do arquivo no lugar de {audio} e escreve o texto (UTF-8) na saida.
//! Ponte pronta para o Whisper local: ferramentas/ouvir_whisper.py. Sem o arquivo de configuracao, ouvir fica desligado. O audio nunca sai do PC.
use serde_json::Value;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const MAXIMO: usize = 20 * 1024 * 1024;
const ESPERA_S: u64 = 180;
static TRAVA: Mutex<()> = Mutex::new(()); // uma transcricao por vez: o modelo pesa

fn extensao(tipo: &str) -> &'static str {
    ["webm", "ogg", "wav", "mp4", "mpeg"].into_iter().find(|e| tipo.contains(e)).unwrap_or("webm")
}

pub fn transcrever(dados: &Path, audio: &[u8], tipo: &str) -> Result<String, String> {
    if audio.is_empty() || audio.len() > MAXIMO {
        return Err("áudio vazio ou grande demais (até 20 MB)".into());
    }
    let cfg: Value = std::fs::read_to_string(dados.join("ouvir.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let partes: Vec<&str> = cfg["comando"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    if partes.is_empty() {
        return Err("ouvir está desligado: falta dados/ouvir.json com o transcritor local".into());
    }
    let _t = TRAVA.lock().map_err(|e| e.to_string())?;
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let arq = std::env::temp_dir().join(format!("braniac-ouvir-{}-{}.{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed), extensao(tipo)));
    std::fs::write(&arq, audio).map_err(|e| e.to_string())?;
    let caminho = arq.to_string_lossy().to_string();
    let mut c = crate::oculto::cmd(partes[0]);
    c.args(partes[1..].iter().map(|p| p.replace("{audio}", &caminho))).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let r = (|| {
        let mut filho = c.spawn().map_err(|e| format!("o transcritor não abriu: {e}"))?;
        let limite = Instant::now() + Duration::from_secs(ESPERA_S);
        // a saida e uma frase curta (cabe no buffer do pipe): da para esperar o fim antes de ler
        let ok = loop {
            match filho.try_wait() {
                Ok(Some(s)) => break s.success(),
                Ok(None) if Instant::now() < limite => std::thread::sleep(Duration::from_millis(150)),
                _ => {
                    let _ = filho.kill();
                    let _ = filho.wait();
                    return Err("o transcritor demorou demais".to_string());
                }
            }
        };
        let mut bytes = Vec::new();
        if let Some(mut s) = filho.stdout.take() {
            let _ = s.read_to_end(&mut bytes);
        }
        let texto = String::from_utf8_lossy(&bytes).split_whitespace().collect::<Vec<_>>().join(" ");
        match (ok, texto.is_empty()) {
            (false, _) => Err("o transcritor falhou".into()),
            (true, true) => Err("não entendi o áudio".into()),
            (true, false) => Ok(texto),
        }
    })();
    let _ = std::fs::remove_file(&arq); // o audio nao fica guardado
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sem_configuracao_fica_desligado_e_recusa_audio_vazio() {
        let d = std::env::temp_dir().join(format!("braniac-ouvir-a-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        assert!(transcrever(&d, b"x", "audio/webm").unwrap_err().contains("desligado"));
        assert!(transcrever(&d, b"", "audio/webm").unwrap_err().contains("vazio"));
        assert_eq!((extensao("audio/webm;codecs=opus"), extensao("audio/wav"), extensao("")), ("webm", "wav", "webm"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(windows)]
    #[test]
    fn chama_o_transcritor_com_o_arquivo_e_devolve_o_texto() {
        let d = std::env::temp_dir().join(format!("braniac-ouvir-b-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        // transcritor de mentira: escreve o proprio conteudo do "audio"
        std::fs::write(d.join("ouvir.json"), r#"{"comando": ["cmd", "/C", "type", "{audio}"]}"#).unwrap();
        assert_eq!(transcrever(&d, b"  bom dia\r\n  jarvis ", "audio/wav").unwrap(), "bom dia jarvis");
        std::fs::write(d.join("ouvir.json"), r#"{"comando": ["cmd", "/C", "exit", "3"]}"#).unwrap();
        assert_eq!(transcrever(&d, b"x", "audio/wav").unwrap_err(), "o transcritor falhou");
        std::fs::write(d.join("ouvir.json"), r#"{"comando": ["programa-que-nao-existe-xyz"]}"#).unwrap();
        assert!(transcrever(&d, b"x", "audio/wav").unwrap_err().contains("não abriu"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
