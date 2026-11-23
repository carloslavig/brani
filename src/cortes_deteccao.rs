//! Deteccao de momentos virais SEM IA: pico de volume + pico de mensagens no chat (meme de hype conta em dobro).
//! Cada sinal vira um z-score robusto em relacao a vizinhanca (5 min): live sempre barulhenta nao vira "tudo pico".
//! Ficou de fora: rodar o ffmpeg. `args_energia` monta o comando e `energia_de_pcm` le o audio cru (s16le mono) que ele devolve.
//! Diferenca do Python: empate de score sai em ordem de tempo (o numpy nao garante a ordem do empate).
#![allow(dead_code)]
use crate::empresa_dev::rx;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::OnceLock;

pub const IGNORAR_INICIO_S: usize = 60; // tela de "comecando" nao e momento viral

pub fn hype(msg: &str) -> bool {
    static R: OnceLock<Regex> = OnceLock::new();
    rx(&R, r"(?i)\b(kekw|lul|lmao|omegalul|pog|pogchamp|poggers|pogu|monkas|pepelaugh|lol|clip|clipa|clipou|w+|gg|aaa+|nossa|caraca|meu deus|mds|socorro)\b|k{4,}|\?{3,}|!{3,}|hah(a)+|rsrs").is_match(msg)
}

pub fn args_energia(arquivo: &str, sr: u32) -> Vec<String> {
    ["ffmpeg", "-nostdin", "-v", "error", "-i", arquivo, "-vn", "-ac", "1", "-ar", &sr.to_string(), "-f", "s16le", "-"].iter().map(|x| x.to_string()).collect()
}

/// Volume (dBFS) de cada segundo inteiro do audio cru s16le mono; o resto que nao fecha 1 s e ignorado.
pub fn energia_de_pcm(pcm: &[u8], sr: usize) -> Vec<f64> {
    if sr == 0 {
        return vec![];
    }
    pcm.chunks_exact(sr * 2)
        .map(|seg| {
            let soma: f64 = seg.chunks_exact(2).map(|b| (i16::from_le_bytes([b[0], b[1]]) as f64 / 32768.0).powi(2)).sum();
            20.0 * ((soma / sr as f64).sqrt() + 1e-6).log10()
        })
        .collect()
}

/// Mensagens por segundo a partir do .jsonl do chat ({"t": segundos, "msg": texto}), deslocadas `atraso_s` para tras:
/// o chat reage depois do lance. Chat quase vazio (menos de 30) nao e sinal confiavel: None.
pub fn chat_por_segundo(jsonl: &str, total_s: usize, atraso_s: f64) -> (Option<Vec<f64>>, BTreeMap<usize, u32>) {
    let mut v = vec![0.0; total_s + 1];
    let mut hy = BTreeMap::new();
    for ln in jsonl.lines() {
        let Ok(d) = serde_json::from_str::<Value>(ln) else { continue };
        let t = d["t"].as_f64().or_else(|| d["t"].as_str().and_then(|x| x.parse().ok())).unwrap_or(0.0);
        let i = (t - atraso_s) as i64; // trunca para zero, como o int() do Python
        if i < 0 || i as usize >= v.len() {
            continue;
        }
        let achou = hype(d["msg"].as_str().unwrap_or(""));
        v[i as usize] += if achou { 2.0 } else { 1.0 };
        if achou {
            *hy.entry(i as usize).or_insert(0) += 1;
        }
    }
    (if v.iter().sum::<f64>() >= 30.0 { Some(v) } else { None }, hy)
}

/// Media de `n` vizinhos com a borda repetida (nao zeros: em dB, zero seria "volume maximo" e o fim do arquivo viraria pico falso).
pub fn media_movel(x: &[f64], n: usize) -> Vec<f64> {
    if n <= 1 || x.len() < n {
        return x.to_vec();
    }
    let esq = (n / 2) as i64;
    let ult = x.len() as i64 - 1;
    (0..x.len() as i64).map(|i| (0..n as i64).map(|k| x[(i + k - esq).clamp(0, ult) as usize]).sum::<f64>() / n as f64).collect()
}

fn mediana(w: &[f64]) -> f64 {
    let mut o = w.to_vec();
    o.sort_by(|a, b| a.total_cmp(b));
    let n = o.len();
    if n == 0 {
        0.0
    } else if n % 2 == 1 {
        o[n / 2]
    } else {
        (o[n / 2 - 1] + o[n / 2]) / 2.0
    }
}

fn interp(i: usize, centros: &[usize], ys: &[f64]) -> f64 {
    let k = centros.partition_point(|&c| c <= i);
    if k == 0 {
        return ys[0];
    }
    if k == centros.len() {
        return ys[k - 1];
    }
    let (a, b) = (centros[k - 1], centros[k]);
    ys[k - 1] + (ys[k] - ys[k - 1]) * (i - a) as f64 / (b - a) as f64
}

/// (x - mediana local) / max(1,4826 * MAD local, piso), mediana e MAD a cada `passo` s e interpoladas.
/// O piso evita que um trecho muito constante (ruido parado, chat morto) transforme qualquer variacao minima em "pico".
pub fn z_robusto(x: &[f64], janela: usize, passo: usize, piso: f64) -> Vec<f64> {
    let n = x.len();
    if n == 0 || passo == 0 {
        return x.to_vec();
    }
    let (mut centros, mut meds, mut mads) = (vec![], vec![], vec![]);
    for c in (0..n).step_by(passo) {
        let w = &x[c.saturating_sub(janela / 2)..(c + janela / 2 + 1).min(n)];
        let m = mediana(w);
        let desvios: Vec<f64> = w.iter().map(|v| (v - m).abs()).collect();
        centros.push(c);
        meds.push(m);
        mads.push((mediana(&desvios) * 1.4826).max(piso));
    }
    (0..n).map(|i| (x[i] - interp(i, &centros, &meds)) / interp(i, &centros, &mads)).collect()
}

/// (score, z do audio, z do chat) por segundo. Audio: media de 4 s (grito sustentado, nao um estalo). Chat: soma de 10 s.
pub fn pontuar(audio_db: &[f64], chat: Option<&[f64]>, peso_audio: f64, peso_chat: f64) -> (Vec<f64>, Vec<f64>, Option<Vec<f64>>) {
    // piso 2 dB: variacao menor que isso nao e "momento"
    let mut za: Vec<f64> = z_robusto(&media_movel(audio_db, 4), 300, 30, 2.0).iter().map(|z| z.clamp(-3.0, 8.0)).collect();
    let Some(chat) = chat else { return (za.iter().map(|z| z * peso_audio).collect(), za, None) };
    let n = za.len().min(chat.len());
    za.truncate(n);
    let soma10: Vec<f64> = media_movel(&chat[..n], 10).iter().map(|v| v * 10.0).collect();
    let zc: Vec<f64> = z_robusto(&soma10, 300, 30, 3.0).iter().map(|z| z.clamp(-3.0, 10.0)).collect(); // piso: 3 msgs em 10 s
    ((0..n).map(|i| peso_audio * za[i] + peso_chat * zc[i]).collect(), za, Some(zc))
}

#[derive(Clone, Copy, Debug)]
pub struct Regras {
    pub quantos: usize,
    pub minimo: f64,
    pub antes: i64,
    pub depois: i64,
    pub dur_min: i64,
    pub dur_max: i64,
}

impl Default for Regras {
    fn default() -> Self {
        Regras { quantos: 6, minimo: 2.0, antes: 22, depois: 10, dur_min: 25, dur_max: 58 }
    }
}

impl Regras {
    pub fn de_cfg(cfg: &Value) -> Regras {
        let p = Regras::default();
        let i = |k: &str, d: i64| cfg[k].as_f64().map(|x| x as i64).unwrap_or(d);
        Regras { quantos: i("cortes_por_live", p.quantos as i64).max(0) as usize, minimo: cfg["score_minimo"].as_f64().unwrap_or(p.minimo), antes: i("antes_do_pico_s", p.antes),
                 depois: i("depois_do_pico_s", p.depois), dur_min: i("duracao_min_s", p.dur_min), dur_max: i("duracao_max_s", p.dur_max) }
    }
}

/// Os maiores picos, sem sobreposicao, cada um virando (inicio, fim, score, segundo do pico), em ordem de tempo.
pub fn escolher_janelas(score: &[f64], r: &Regras, total: Option<usize>) -> Vec<(f64, f64, f64, usize)> {
    let total = total.filter(|t| *t > 0).unwrap_or(score.len()) as i64;
    let mut ordem: Vec<usize> = (IGNORAR_INICIO_S.min(score.len())..score.len()).collect();
    ordem.sort_by(|a, b| score[*b].total_cmp(&score[*a]));
    let mut js: Vec<(i64, i64, f64, usize)> = vec![];
    for pico in ordem {
        if js.len() >= r.quantos || score[pico] < r.minimo {
            break;
        }
        let ini = pico as i64 - r.antes;
        let mut fim = pico as i64 + r.depois;
        if fim - ini < r.dur_min {
            fim = ini + r.dur_min;
        }
        fim = fim.min(ini + r.dur_max);
        let (ini, fim) = (ini.max(0), fim.min(total));
        if ((fim - ini) as f64) < r.dur_min as f64 * 0.6 || js.iter().any(|j| ini < j.1 + 5 && fim > j.0 - 5) {
            continue;
        }
        js.push((ini, fim, score[pico], pico));
    }
    js.sort_by_key(|j| j.0);
    js.into_iter().map(|j| (j.0 as f64, j.1 as f64, j.2, j.3)).collect()
}

/// Lista de {inicio, fim, score, motivo} a partir do volume por segundo e, se houver, do texto do chat (.jsonl).
pub fn detectar(audio_db: &[f64], chat_jsonl: Option<&str>, cfg: &Value) -> Vec<Value> {
    let (chat, hy) = match chat_jsonl {
        Some(t) => chat_por_segundo(t, audio_db.len(), cfg["atraso_chat_s"].as_f64().unwrap_or(6.0)),
        None => (None, BTreeMap::new()),
    };
    let (score, za, zc) = pontuar(audio_db, chat.as_deref(), cfg["peso_audio"].as_f64().unwrap_or(1.0), cfg["peso_chat"].as_f64().unwrap_or(1.4));
    escolher_janelas(&score, &Regras::de_cfg(cfg), Some(score.len()))
        .into_iter()
        .map(|(ini, fim, s, pico)| {
            let mut motivo = format!("audio {:+.1}σ", za[pico]);
            if let Some(zc) = &zc {
                let nh: u32 = hy.range(pico.saturating_sub(5)..pico + 15).map(|(_, n)| *n).sum();
                motivo += &format!(", chat {:+.1}σ", zc[pico]);
                if nh > 0 {
                    motivo += &format!(" ({nh} msgs de hype)");
                }
            }
            json!({"inicio": ini, "fim": fim, "score": (s * 100.0).round() / 100.0, "motivo": motivo})
        })
        .collect()
}

#[cfg(test)]
pub(crate) fn audio_teste(total: usize, picos: &[usize]) -> Vec<f64> {
    // ruido de mentira (gerador linear simples), +-2,5 dB em volta de -40
    let mut s: u64 = 1;
    let mut db: Vec<f64> = (0..total)
        .map(|_| {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            -40.0 + ((s >> 33) as f64 / (1u64 << 31) as f64 - 0.5) * 5.0
        })
        .collect();
    for p in picos {
        for v in db.iter_mut().skip(*p).take(5) {
            *v = -12.0;
        }
    }
    db
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acha_os_picos_de_audio_sem_sobreposicao() {
        let (score, _, _) = pontuar(&audio_teste(600, &[200, 210, 450]), None, 1.0, 1.4);
        let js = escolher_janelas(&score, &Regras { quantos: 5, ..Regras::default() }, None);
        assert_eq!(js.len(), 2, "{js:?}"); // 200 e 210 viram um corte so
        assert!(js[0].0 <= 200.0 && 200.0 <= js[0].1 && js[1].0 <= 450.0 && 450.0 <= js[1].1);
        assert!(js.iter().all(|j| (25.0..=58.0).contains(&(j.1 - j.0))));
    }

    #[test]
    fn fim_do_arquivo_ruido_constante_e_inicio_da_live_nao_viram_pico() {
        let mut db = vec![-46.3; 300];
        for v in db.iter_mut().skip(295) {
            *v += 0.3;
        }
        assert!(escolher_janelas(&pontuar(&db, None, 1.0, 1.4).0, &Regras::default(), None).is_empty());
        assert!(escolher_janelas(&pontuar(&audio_teste(600, &[10]), None, 1.0, 1.4).0, &Regras::default(), None).is_empty());
        assert!(escolher_janelas(&[], &Regras::default(), None).is_empty());
    }

    #[test]
    fn chat_com_hype_reforca_e_e_adiantado() {
        let mut linhas: Vec<String> = (0..600).step_by(4).map(|t| json!({"t": t, "msg": "oi"}).to_string()).collect();
        linhas.extend((0..60).map(|i| json!({"t": 306.0 + i as f64 * 0.1, "msg": "KEKW kkkkk"}).to_string()));
        linhas.push("lixo que nao e json".into());
        let (chat, hy) = chat_por_segundo(&linhas.join("\n"), 600, 6.0);
        let chat = chat.unwrap();
        assert!(chat[300..306].iter().sum::<f64>() > 100.0 && hy.values().sum::<u32>() == 60);
        let audio = audio_teste(600, &[]);
        let (score, _, zc) = pontuar(&audio, Some(&chat), 1.0, 1.4);
        assert!(zc.is_some());
        let js = escolher_janelas(&score, &Regras::default(), None);
        assert!(!js.is_empty() && js[0].0 <= 300.0 && 300.0 <= js[0].1, "{js:?}");
        let d = detectar(&audio, Some(&linhas.join("\n")), &json!({}));
        assert!(d[0]["motivo"].as_str().unwrap().contains("msgs de hype") && d[0]["motivo"].as_str().unwrap().starts_with("audio "));
    }

    #[test]
    fn chat_quase_vazio_e_ignorado_e_hype_reconhece_memes() {
        assert!(chat_por_segundo(&json!({"t": 5, "msg": "oi"}).to_string(), 100, 6.0).0.is_none());
        for sim in ["KEKW", "kkkkk", "???", "hahaha", "meu deus", "WWW", "gg"] {
            assert!(hype(sim), "{sim}");
        }
        for nao in ["oi", "kk", "boa noite", "ggwp123"] {
            assert!(!hype(nao), "{nao}");
        }
    }

    #[test]
    fn energia_do_pcm_e_media_movel_de_borda() {
        let mut pcm = vec![];
        for _ in 0..4 {
            pcm.extend_from_slice(&16384i16.to_le_bytes()); // metade do fundo de escala = -6 dB
        }
        pcm.extend_from_slice(&[0, 0, 0, 0, 9]); // 1 s de silencio e um byte solto
        let db = energia_de_pcm(&pcm, 2);
        assert_eq!(db.len(), 3);
        assert!((db[0] + 6.02).abs() < 0.01 && db[2] < -100.0);
        assert_eq!(media_movel(&[1.0, 1.0, 1.0, 5.0], 2), vec![1.0, 1.0, 1.0, 3.0]);
        assert_eq!(media_movel(&[1.0, 2.0], 4), vec![1.0, 2.0]);
        assert_eq!(args_energia("a.ts", 8000)[10], "8000");
    }
}
