//! Aparelho da conversa por voz (o que escuta.py/voz.py fazem com o hardware): microfone nativo e alto-falante. As REGRAS (maquina de
//! estados, escolha de voz, transcricao) ja moram em escuta.rs, vozes_agentes.rs e ouvir.rs; aqui so entra o som.
//! Chave: dados/troca.json {"voz": false} (POST /api/troca {"voz": true}); nasce DESLIGADA e nada liga sozinho. Chave desligada = o
//! microfone NUNCA e aberto e nada toca sozinho (so a rota explicita POST /api/voz/tocar, pedida pela tela).
//! Fluxo ligado: cpal capta o microfone padrao -> 16 kHz mono em blocos de 100 ms -> `escuta::Vad` corta frases -> `ouvir` (Whisper LOCAL,
//! o audio nao sai do PC e nao vai a disco daqui) -> POST local /api/escuta/frase (a mesma maquina da tela) -> a fala volta como WAV
//! (voz do agente certo) e toca no alto-falante (rodio), um som por vez. Surdo enquanto fala e 0,7 s depois, como no Python.
//! O que o microfone ouviu nao e gravado em log.
#![allow(dead_code)]
use crate::escuta::{amplificar, Vad, BLOCO, TAXA};
use crate::roteador::Roteador;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const CAUDA_MS: u64 = 700; // depois de falar, ainda "surdo" por 0,7 s (o eco da propria voz)
/// "Desligar microfone" do menu/tela. A chave `voz` continua mandando: sem ela nada abre.
pub static MIC_LIGADO: AtomicBool = AtomicBool::new(true);
static ABERTO: AtomicBool = AtomicBool::new(false);
static FALANDO: AtomicBool = AtomicBool::new(false);
static SURDO_ATE: AtomicU64 = AtomicU64::new(0);
static NIVEL: AtomicU32 = AtomicU32::new(0);
static ERRO: Mutex<String> = Mutex::new(String::new());
static TOCAR: Mutex<()> = Mutex::new(()); // a trava de voz (_voz_lock): um alto-falante, um som por vez

fn agora_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

pub fn chave(dados: &Path) -> bool {
    crate::wpp_laco::chave(dados, "voz")
}

/// O microfone so abre com a chave ligada E o microfone nao desligado pelo menu.
pub fn quer_microfone(chave: bool, mic_ligado: bool) -> bool {
    chave && mic_ligado
}

/// Sem ouvir enquanto o B.R.A.N.I fala (e um instante depois).
pub fn surdo(falando: bool, surdo_ate: u64, agora: u64) -> bool {
    falando || agora < surdo_ate
}

/// Um som por vez: quem chega espera o anterior terminar.
pub fn um_por_vez<R>(f: impl FnOnce() -> R) -> R {
    let _g = crate::travar(&TOCAR);
    f()
}

/// Mono float no ritmo do aparelho -> um bloco de 1600 amostras (16 kHz). `entrada.len()` = 100 ms no ritmo do aparelho.
pub fn reduzir(entrada: &[f32]) -> Vec<i16> {
    let n = entrada.len().max(1);
    (0..BLOCO)
        .map(|j| {
            let (a, b) = (j * n / BLOCO, ((j + 1) * n / BLOCO).max(j * n / BLOCO + 1).min(n));
            let m = entrada[a.min(n - 1)..b].iter().sum::<f32>() / (b - a.min(n - 1)).max(1) as f32;
            (m.clamp(-1.0, 1.0) * 32767.0) as i16
        })
        .collect()
}

/// WAV PCM 16 bits mono 16 kHz, em memoria (para o transcritor).
pub fn wav_bytes(pcm: &[i16]) -> Vec<u8> {
    let dados = (pcm.len() * 2) as u32;
    let mut w = Vec::with_capacity(44 + pcm.len() * 2);
    w.extend(b"RIFF");
    w.extend((36 + dados).to_le_bytes());
    w.extend(b"WAVEfmt ");
    w.extend(16u32.to_le_bytes());
    w.extend([1u8, 0, 1, 0]);
    w.extend((TAXA as u32).to_le_bytes());
    w.extend((TAXA as u32 * 2).to_le_bytes());
    w.extend([2u8, 0, 16, 0]);
    w.extend(b"data");
    w.extend(dados.to_le_bytes());
    pcm.iter().for_each(|s| w.extend(s.to_le_bytes()));
    w
}

/// Da resposta de /api/escuta/frase: (texto a falar, agente). None = nada a falar (mudo, dormindo...).
pub fn fala_da_resposta(r: &Value) -> Option<(String, String)> {
    let t = r["fala"].as_str().filter(|t| !t.trim().is_empty())?;
    Some((t.to_string(), if r["agente"] == "alma" { "alma" } else { "jarvis" }.to_string()))
}

pub fn estado() -> Value {
    json!({ "aberto": ABERTO.load(Ordering::SeqCst), "microfone_ligado": MIC_LIGADO.load(Ordering::SeqCst), "falando": FALANDO.load(Ordering::SeqCst),
            "nivel": f32::from_bits(NIVEL.load(Ordering::SeqCst)), "erro": crate::travar(&ERRO).clone() })
}

// ---------- alto-falante ----------

/// Toca um WAV (bloqueia ate acabar ou ate o mudo cortar). Volume: BRANIAC_VOLUME (0 a 1, padrao 1).
pub fn tocar_wav(wav: Vec<u8>) -> Result<(), String> {
    um_por_vez(|| {
        let vol = std::env::var("BRANIAC_VOLUME").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.0).clamp(0.0, 1.0);
        FALANDO.store(true, Ordering::SeqCst);
        let r = (|| {
            let (_fluxo, saida) = rodio::OutputStream::try_default().map_err(|e| format!("sem alto-falante: {e}"))?;
            let sink = rodio::Sink::try_new(&saida).map_err(|e| format!("alto-falante: {e}"))?;
            sink.set_volume(vol);
            sink.append(rodio::Decoder::new(std::io::Cursor::new(wav)).map_err(|e| format!("audio ilegivel: {e}"))?);
            while !sink.empty() {
                if crate::mudo::CORTAR.load(Ordering::SeqCst) {
                    sink.stop();
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Ok(())
        })();
        SURDO_ATE.store(agora_ms() + CAUDA_MS, Ordering::SeqCst);
        FALANDO.store(false, Ordering::SeqCst);
        r
    })
}

/// Gera a voz do agente (mesma regra de POST /api/voz: escolhida na tela Vozes e reservas, depois a ordem de sempre; texto sensivel so voz local) e toca.
pub fn falar(dados: &Path, rot: &Roteador, texto: &str, agente: &str) -> Result<(), String> {
    let (agora, nuvem) = (crate::tempo::agora(), !rot.sensivel(texto));
    let estilo = crate::vozes_agentes::escolha(dados, agente)["ajustes"]["estilo"].as_str().unwrap_or("").to_string();
    let wav = crate::vozes_agentes::falar(dados, &agora, agente, texto, nuvem, &|m, v, tx| crate::voz::gerar(&*rot.ler_chave, m, v, tx, &estilo))
        .map(|f| f.wav)
        .or_else(|_| crate::voz::falar_wav(dados, &*rot.ler_chave, texto, nuvem, agente).map(|(w, _)| w))?;
    tocar_wav(wav)
}

// ---------- microfone ----------

fn empurrar<T: Copy>(buf: &Mutex<Vec<f32>>, d: &[T], canais: usize, taxa: usize, conv: impl Fn(T) -> f32) {
    let mut b = crate::travar(buf);
    b.extend(d.chunks(canais.max(1)).map(|c| c.iter().map(|&x| conv(x)).sum::<f32>() / c.len() as f32));
    let teto = taxa * 30;
    if b.len() > teto {
        let sobra = b.len() - teto;
        b.drain(..sobra);
    }
}

fn abrir(buf: Arc<Mutex<Vec<f32>>>) -> Result<(cpal::Stream, usize), String> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{Sample, SampleFormat};
    let dev = cpal::default_host().default_input_device().ok_or("sem microfone")?;
    let cfg = dev.default_input_config().map_err(|e| format!("microfone: {e}"))?;
    let (taxa, canais, formato) = (cfg.sample_rate().0 as usize, cfg.channels() as usize, cfg.sample_format());
    let falha = |e| *crate::travar(&ERRO) = format!("microfone: {e}");
    let cfg = cfg.into();
    let s = match formato {
        SampleFormat::F32 => dev.build_input_stream(&cfg, move |d: &[f32], _| empurrar(&buf, d, canais, taxa, |x| x), falha, None),
        SampleFormat::I16 => dev.build_input_stream(&cfg, move |d: &[i16], _| empurrar(&buf, d, canais, taxa, f32::from_sample), falha, None),
        SampleFormat::U16 => dev.build_input_stream(&cfg, move |d: &[u16], _| empurrar(&buf, d, canais, taxa, f32::from_sample), falha, None),
        f => return Err(format!("formato de microfone nao suportado: {f:?}")),
    }
    .map_err(|e| format!("microfone: {e}"))?;
    s.play().map_err(|e| format!("microfone: {e}"))?;
    Ok((s, taxa))
}

fn proximo_bloco(buf: &Mutex<Vec<f32>>, taxa: usize) -> Option<Vec<i16>> {
    let mut b = crate::travar(buf);
    let n = taxa / 10;
    (b.len() >= n).then(|| reduzir(&b.drain(..n).collect::<Vec<_>>()))
}

fn rms(pcm: &[i16]) -> f32 {
    ((pcm.iter().map(|&v| (v as f32 / 32768.0).powi(2)).sum::<f32>()) / pcm.len().max(1) as f32).sqrt()
}

fn post_local(caminho: &str, corpo: &Value) -> Result<Value, String> {
    ureq::post(&format!("http://127.0.0.1:{}{caminho}", crate::PORTA)).set("X-Braniac", "aparelho").timeout(Duration::from_secs(180))
        .send_json(corpo.clone()).map_err(|e| e.to_string())?.into_json::<Value>().map_err(|e| e.to_string())
}

/// Uma frase captada: transcreve (local), passa pela maquina de estados e fala a resposta. Nada do que foi ouvido vai para log.
fn ouvida(dados: &Path, rot: &Roteador, seg: Vec<i16>) {
    let Ok(texto) = crate::ouvir::transcrever(dados, &wav_bytes(&amplificar(&seg)), "audio/wav") else { return };
    let texto: String = texto.trim().chars().take(600).collect();
    if texto.is_empty() {
        return;
    }
    let Ok(r) = post_local("/api/escuta/frase", &json!({ "texto": texto })) else { return };
    if let Some((fala, agente)) = fala_da_resposta(&r) {
        if let Err(e) = falar(dados, rot, &fala, &agente) {
            *crate::travar(&ERRO) = e;
        }
    }
}

fn laco(rot: Arc<Roteador>) {
    let buf = Arc::new(Mutex::new(Vec::<f32>::new()));
    let (mut aberto, mut vad) = (None::<(cpal::Stream, usize)>, Vad::default());
    loop {
        std::thread::sleep(Duration::from_millis(50));
        let dados = crate::pasta_dados();
        match (quer_microfone(chave(&dados), MIC_LIGADO.load(Ordering::SeqCst)), aberto.is_some()) {
            (true, false) => match abrir(buf.clone()) {
                Ok(s) => {
                    aberto = Some(s);
                    ABERTO.store(true, Ordering::SeqCst);
                    ERRO.lock().map(|mut e| e.clear()).ok();
                    if let Some(mut p) = crate::liga_casa::voz_livre() {
                        p.0.ligar();
                    }
                }
                Err(e) => {
                    *crate::travar(&ERRO) = e;
                    std::thread::sleep(Duration::from_secs(5));
                }
            },
            (false, true) => {
                aberto = None;
                ABERTO.store(false, Ordering::SeqCst);
                crate::travar(&buf).clear();
                if let Some(mut p) = crate::liga_casa::voz_livre() {
                    p.0.desligar();
                }
            }
            _ => {}
        }
        let Some((_, taxa)) = &aberto else { continue };
        while let Some(b) = proximo_bloco(&buf, *taxa) {
            NIVEL.store(rms(&b).to_bits(), Ordering::SeqCst);
            let falando = surdo(FALANDO.load(Ordering::SeqCst), SURDO_ATE.load(Ordering::SeqCst), agora_ms());
            if let Some(seg) = vad.bloco(&b, falando) {
                ouvida(&dados, &rot, seg);
                crate::travar(&buf).clear(); // como o Python: esvazia a fila depois de cada frase, para nao ouvir a propria voz
                break;
            }
        }
    }
}

/// Liga o laco do microfone (so abre o aparelho com a chave `voz` ligada) e o icone da bandeja.
pub fn iniciar(rot: Arc<Roteador>) {
    std::thread::spawn(move || laco(rot));
    bandeja();
}

/// Sobe o script da bandeja (icones do Jarvis e da Alma). Ele mesmo recusa uma segunda copia (mutex nomeado). Sem o arquivo, nao faz nada.
fn bandeja() {
    let script = std::env::var_os("BRANIAC_BANDEJA").map(std::path::PathBuf::from).or_else(|| {
        std::env::current_exe().ok()?.ancestors().map(|p| p.join("ferramentas").join("bandeja.ps1")).find(|p| p.exists())
    });
    let Some(script) = script else { return };
    let mut c = std::process::Command::new("powershell.exe");
    c.args(["-NoProfile", "-STA", "-WindowStyle", "Hidden", "-ExecutionPolicy", "Bypass", "-File"]).arg(script).arg(crate::PORTA.to_string());
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut c, 0x0800_0000);
    let _ = c.spawn();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn chave_desligada_nunca_abre_o_microfone() {
        assert!(!quer_microfone(false, true));
        assert!(!quer_microfone(true, false));
        assert!(quer_microfone(true, true));
        let d = std::env::temp_dir().join(format!("braniac-aparelho-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        assert!(!chave(&d)); // sem arquivo: desligada
        std::fs::write(d.join("troca.json"), r#"{"voz": "sim"}"#).unwrap();
        assert!(!chave(&d)); // so true liga
    }

    #[test]
    fn nao_escuta_enquanto_fala_e_na_cauda() {
        assert!(surdo(true, 0, 10_000));
        assert!(surdo(false, 10_500, 10_000));
        assert!(!surdo(false, 10_000, 10_000));
        // o corte em frases ignora o bloco quando esta surdo
        let mut v = Vad::default();
        let alto = vec![8000i16; BLOCO];
        for _ in 0..10 {
            assert!(v.bloco(&alto, true).is_none());
        }
        assert!(!v.ouvindo());
    }

    #[test]
    fn um_som_por_vez() {
        let (agora, maximo) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let fios: Vec<_> = (0..4)
            .map(|_| {
                let (a, m) = (agora.clone(), maximo.clone());
                std::thread::spawn(move || {
                    um_por_vez(|| {
                        let n = a.fetch_add(1, Ordering::SeqCst) + 1;
                        m.fetch_max(n, Ordering::SeqCst);
                        std::thread::sleep(Duration::from_millis(20));
                        a.fetch_sub(1, Ordering::SeqCst);
                    })
                })
            })
            .collect();
        fios.into_iter().for_each(|f| f.join().unwrap());
        assert_eq!(maximo.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn mudo_respeitado_e_voz_do_agente_certo() {
        // no mudo a maquina nao pede fala: a resposta vem sem "fala"
        assert_eq!(fala_da_resposta(&json!({ "texto": "oi", "fala": null, "agente": "jarvis" })), None);
        assert_eq!(fala_da_resposta(&json!({ "fala": "Oi, meu bem.", "agente": "alma" })), Some(("Oi, meu bem.".into(), "alma".into())));
        assert_eq!(fala_da_resposta(&json!({ "fala": "Pois não.", "agente": "outro" })).unwrap().1, "jarvis");
    }

    #[test]
    fn reduz_e_empacota_audio() {
        assert_eq!(reduzir(&vec![0.5f32; 4800]).len(), BLOCO); // 48 kHz -> 16 kHz
        assert_eq!(reduzir(&vec![0.5f32; 1600]).len(), BLOCO);
        assert!(reduzir(&vec![0.5f32; 4800]).iter().all(|&s| (s - 16383).abs() <= 1));
        let w = wav_bytes(&[1, 2, 3]);
        assert_eq!((&w[0..4], &w[8..12], w.len()), (&b"RIFF"[..], &b"WAVE"[..], 50));
    }
}
