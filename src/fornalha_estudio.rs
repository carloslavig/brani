//! Oficina local da Fornalha: livro de estilo por cliente, placar de qualidade 0-100 (Cris), filtro de promessas na legenda,
//! biblioteca de prompts vencedores e registro de desperdicio (biblioteca.jsonl e desperdicio.jsonl, mesmo formato do Python).
//! Ficou de fora: abrir imagem (Pillow) e rodar ffmpeg/ffprobe. As medidas da imagem e a sonda do video chegam por parametro;
//! os comandos do ffmpeg saem prontos como `Vec<String>` e quem chama decide se executa.
#![allow(dead_code)]
use crate::empresa_dev::{corta, rx};
use regex::Regex;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::OnceLock;

/// Um plano = um movimento de camera; gancho nos 2 primeiros segundos.
pub const ROTEIRO_VIDEO: &str = "Estruture o vídeo em 3 planos com tempo, no formato:\n0-2s GANCHO: ... | 2-5s DESENVOLVIMENTO: ... | 5-8s FECHAMENTO: ...\nEm cada plano diga enquadramento, lente (ex.: 35mm, macro), UM único movimento de câmera, luz e o que se move na cena. Mantenha a mesma luz, cores e direção da câmera nos 3 planos (continuidade).";

const PROMESSAS: [&str; 12] = [r"o melhor d[aeo]s? [\wçãõáéíóúâêô ]{2,25}", r"melhor d[aeo]s? (?:cidade|regi[aã]o|brasil)", r"\b100 ?% ?\w+", r"sem conservantes?", r"saud[aá]vel",
    r"emagrec\w+", r"\bcura\w*", r"garantid\w+", r"\bn[ºo°]\.? ?1\b", r"\bnúmero 1\b", r"zero (?:gl[uú]ten|lactose|a[cç][uú]car)", "artesanal"];

/// Livro de estilo do cliente (estilo/<cliente>.md dentro da pasta da empresa). Vazio se nao existir.
pub fn estilo(pasta: &Path, cliente: &str) -> String {
    std::fs::read_to_string(pasta.join("estilo").join(format!("{}.md", cliente.replace(' ', "-")))).map(|t| corta(&t, 900)).unwrap_or_default()
}

pub fn alvo_imagem(tipo: &str) -> (u32, u32) {
    if tipo == "stories" { (1080, 1920) } else { (1080, 1350) }
}

/// Caixa de corte central (x0, y0, x1, y1) para 4:5 (stories 9:16).
pub fn corte_imagem(w: u32, h: u32, tipo: &str) -> (u32, u32, u32, u32) {
    let (lw, lh) = alvo_imagem(tipo);
    let ratio = lw as f64 / lh as f64;
    if w as f64 / h as f64 > ratio {
        let nw = (h as f64 * ratio) as u32;
        ((w - nw) / 2, 0, (w - nw) / 2 + nw, h)
    } else {
        let nh = (w as f64 / ratio) as u32;
        (0, (h - nh) / 2, w, (h - nh) / 2 + nh)
    }
}

/// O que quem abre a imagem mede: tamanho, media e desvio do cinza (0-255) e variancia do filtro de bordas.
#[derive(Clone, Copy, Debug)]
pub struct MedidaImagem {
    pub w: u32,
    pub h: u32,
    pub media: f64,
    pub desvio: f64,
    pub borda: f64,
}

/// (nota 0-100, avisos). Resolucao 30 + formato 15 + brilho 20 + nitidez 20 + contraste 15. Imagem em branco reprova direto.
pub fn placar_imagem(m: &MedidaImagem, tipo: &str) -> (i64, Vec<String>) {
    if m.desvio < 8.0 || m.h == 0 {
        return (0, vec!["imagem praticamente em branco".into()]);
    }
    let (lw, lh) = alvo_imagem(tipo);
    let mn = m.w.min(m.h);
    let fora = (m.w as f64 / m.h as f64 - lw as f64 / lh as f64).abs();
    let res = if mn >= 1000 { 30 } else if mn >= 720 { 25 } else if mn >= 512 { 20 } else { 0 };
    let fmt = if fora < 0.02 { 15 } else if fora < 0.15 { 6 } else { 0 };
    let bri = if (50.0..=205.0).contains(&m.media) { 20 } else if (30.0..=225.0).contains(&m.media) { 10 } else { 0 };
    let nit = if m.borda >= 80.0 { 20 } else if m.borda >= 25.0 { 10 } else { 0 };
    let con = if m.desvio >= 30.0 { 15 } else if m.desvio >= 15.0 { 8 } else { 0 };
    let mut avisos = vec![];
    if res < 20 {
        avisos.push(format!("resolução baixa ({}x{})", m.w, m.h));
    }
    if fmt < 15 {
        avisos.push(format!("formato fora de {lw}:{lh}"));
    }
    if bri < 20 {
        avisos.push(format!("muito {}", if m.media < 50.0 { "escura" } else { "clara" }));
    }
    if nit < 20 {
        avisos.push("sem nitidez (borrada)".into());
    }
    if con < 15 {
        avisos.push("pouco contraste".into());
    }
    (res + fmt + bri + nit + con, avisos)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sonda {
    pub w: u32,
    pub h: u32,
    pub dur: f64,
    pub audio: bool,
}

pub fn args_sondar(f: &str) -> Vec<String> {
    ["ffprobe", "-v", "error", "-show_streams", "-show_format", "-of", "json", f].map(String::from).to_vec()
}

/// Le a saida JSON do ffprobe. None se nao tem faixa de video ou nao deu para ler.
pub fn sonda_de_json(saida: &str) -> Option<Sonda> {
    let d: Value = serde_json::from_str(saida).ok()?;
    let faixas = d["streams"].as_array()?;
    let v = faixas.iter().find(|s| s["codec_type"] == "video")?;
    let dur = &d["format"]["duration"];
    Some(Sonda { w: v["width"].as_u64()? as u32, h: v["height"].as_u64()? as u32, dur: dur.as_f64().or_else(|| dur.as_str()?.parse().ok()).unwrap_or(0.0),
        audio: faixas.iter().any(|s| s["codec_type"] == "audio") })
}

/// ffmpeg do acabamento: 9:16 1080x1920, cor e nitidez leves, teto de 6 Mbps, loudness -16 LUFS se tiver audio.
pub fn args_acabar_video(f: &str, saida: &str, audio: bool) -> Vec<String> {
    let mut c: Vec<&str> = vec!["ffmpeg", "-y", "-v", "error", "-i", f, "-vf", "scale=1080:1920:force_original_aspect_ratio=increase,crop=1080:1920,eq=contrast=1.04:saturation=1.08,unsharp=3:3:0.4",
        "-c:v", "libx264", "-preset", "veryfast", "-crf", "20", "-maxrate", "6M", "-bufsize", "12M", "-pix_fmt", "yuv420p", "-movflags", "+faststart"];
    c.extend(if audio { vec!["-af", "loudnorm=I=-16:TP=-1.5:LRA=11", "-c:a", "aac", "-b:a", "128k"] } else { vec!["-an"] });
    c.push(saida);
    c.into_iter().map(String::from).collect()
}

/// ffmpeg da capa: um quadro JPG do meio do video.
pub fn args_capa(f: &str, capa: &str, dur: f64) -> Vec<String> {
    let meio = (dur / 2.0).max(0.5).to_string();
    ["ffmpeg", "-y", "-v", "error", "-ss", &meio, "-i", f, "-frames:v", "1", "-q:v", "3", capa].map(String::from).to_vec()
}

/// (nota, avisos): duracao 3-20s (30), altura >=1280 (25), formato 9:16 (20), tem faixa de video (10), tamanho (15).
/// Sem sonda (ffprobe nao leu): so o tamanho do arquivo conta.
pub fn placar_video(sonda: Option<&Sonda>, bytes: u64) -> (i64, Vec<String>) {
    let Some(i) = sonda.filter(|i| i.h > 0) else {
        return if bytes > 100_000 { (60, vec!["sem ffprobe: só conferi o tamanho".into()]) } else { (0, vec!["vídeo ilegível".into()]) };
    };
    let dur = if (3.0..=20.0).contains(&i.dur) { 30 } else if i.dur >= 1.5 { 12 } else { 0 };
    let alt = if i.h >= 1280 { 25 } else if i.h >= 720 { 15 } else { 0 };
    let fmt = if (i.w as f64 / i.h as f64 - 9.0 / 16.0).abs() < 0.02 { 20 } else { 0 };
    let tam = if bytes >= 100_000 { 15 } else { 0 };
    let avisos = [(dur == 30, format!("duração {:.1}s fora de 3-20s", i.dur)), (alt >= 25, format!("altura {}px", i.h)), (fmt == 20, "formato não é 9:16".to_string()), (tam == 15, "arquivo pequeno".to_string())];
    (dur + alt + fmt + 10 + tam, avisos.into_iter().filter(|(ok, _)| !ok).map(|(_, t)| t).collect())
}

/// Tira da legenda promessas e superlativos que o briefing nao autorizou. Devolve (texto, removidos em ordem alfabetica).
pub fn tirar_promessas(legenda: &str, briefing: &str) -> (String, Vec<String>) {
    static P: OnceLock<Vec<Regex>> = OnceLock::new();
    static E: OnceLock<Regex> = OnceLock::new();
    let bl = briefing.to_lowercase();
    let regras = P.get_or_init(|| PROMESSAS.iter().map(|p| Regex::new(&format!("(?i){p}")).expect("regex fixa")).collect());
    let achados: BTreeSet<String> = regras.iter().flat_map(|r| r.find_iter(legenda)).map(|m| m.as_str().to_string()).filter(|a| !bl.contains(&a.to_lowercase())).collect();
    let mut maiores: Vec<&String> = achados.iter().collect();
    maiores.sort_by_key(|a| std::cmp::Reverse(a.chars().count()));
    let mut txt = legenda.to_string();
    for a in maiores {
        if let Ok(r) = Regex::new(&format!(r"(?i){}[,.!]?\s*", regex::escape(a))) {
            txt = r.replace_all(&txt, "").into_owned();
        }
    }
    (rx(&E, r"[ \t]{2,}").replace_all(&txt, " ").trim().to_string(), achados.into_iter().collect())
}

fn anexar(arq: &Path, obj: &Value) -> Result<(), String> {
    use std::io::Write;
    if let Some(d) = arq.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(arq).map_err(|e| e.to_string())?;
    writeln!(f, "{obj}").map_err(|e| e.to_string())
}

fn linhas(arq: &Path) -> Vec<Value> {
    std::fs::read_to_string(arq).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

pub fn guardar_vencedor(pasta: &Path, p: &Value, nota: i64, ts: f64) -> Result<(), String> {
    anexar(&pasta.join("biblioteca.jsonl"), &json!({ "ts": ts, "cliente": p["cliente"], "tipo": p["tipo"], "nota": nota, "prompt": corta(p["prompt"].as_str().unwrap_or(""), 1500) }))
}

/// Os `n` prompts de maior nota (empate: o mais novo) daquele cliente e tipo.
pub fn melhores(pasta: &Path, cliente: &str, tipo: &str, n: usize) -> Vec<String> {
    let mut l: Vec<Value> = linhas(&pasta.join("biblioteca.jsonl")).into_iter().filter(|x| x["cliente"] == cliente && x["tipo"] == tipo && x["prompt"].as_str().is_some_and(|p| !p.is_empty())).collect();
    let chave = |x: &Value| (x["nota"].as_f64().unwrap_or(0.0), x["ts"].as_f64().unwrap_or(0.0));
    l.sort_by(|a, b| chave(b).partial_cmp(&chave(a)).unwrap_or(std::cmp::Ordering::Equal));
    l.iter().take(n).filter_map(|x| x["prompt"].as_str().map(String::from)).collect()
}

pub fn registrar_desperdicio(pasta: &Path, p: &Value, causa: &str, ts: f64) -> Result<(), String> {
    anexar(&pasta.join("desperdicio.jsonl"), &json!({ "ts": ts, "pedido": p["id"], "cliente": p["cliente"], "tipo": p["tipo"], "causa": corta(causa, 200) }))
}

/// Reprovacoes dos ultimos `dias`.
pub fn desperdicio(pasta: &Path, dias: f64, ts: f64) -> usize {
    linhas(&pasta.join("desperdicio.jsonl")).iter().filter(|x| ts - x["ts"].as_f64().unwrap_or(0.0) < dias * 86400.0).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placar_de_imagem_aprova_boa_e_reprova_em_branco() {
        let boa = MedidaImagem { w: 1080, h: 1350, media: 120.0, desvio: 45.0, borda: 300.0 };
        assert_eq!(placar_imagem(&boa, "imagem"), (100, vec![]));
        assert_eq!(placar_imagem(&MedidaImagem { desvio: 2.0, media: 250.0, ..boa }, "imagem"), (0, vec!["imagem praticamente em branco".to_string()]));
        let (nota, avisos) = placar_imagem(&MedidaImagem { w: 600, h: 600, media: 40.0, desvio: 20.0, borda: 30.0 }, "imagem");
        assert_eq!((nota, avisos.len()), (20 + 0 + 10 + 10 + 8, 4));
        assert!(avisos.contains(&"muito escura".to_string()) && avisos.contains(&"formato fora de 1080:1350".to_string()));
        assert_eq!(placar_imagem(&boa, "stories").0, 85); // 4:5 num stories perde o formato
        assert_eq!(placar_imagem(&MedidaImagem { w: 400, h: 500, ..boa }, "imagem").1, vec!["resolução baixa (400x500)".to_string()]);
        assert_eq!((corte_imagem(2000, 1000, "imagem"), corte_imagem(1000, 2000, "imagem"), corte_imagem(1080, 1920, "stories")), ((600, 0, 1400, 1000), (0, 375, 1000, 1625), (0, 0, 1080, 1920)));
    }

    #[test]
    fn placar_de_video_e_sonda() {
        let s = sonda_de_json(r#"{"streams":[{"codec_type":"audio"},{"codec_type":"video","width":1080,"height":1920}],"format":{"duration":"7.5"}}"#).unwrap();
        assert_eq!(s, Sonda { w: 1080, h: 1920, dur: 7.5, audio: true });
        assert_eq!(placar_video(Some(&s), 500_000), (100, vec![]));
        let (nota, avisos) = placar_video(Some(&Sonda { w: 1920, h: 1080, dur: 40.0, audio: false }), 50_000);
        assert_eq!((nota, avisos.len()), (12 + 15 + 10, 4));
        assert_eq!((placar_video(None, 200_000).0, placar_video(None, 10).0), (60, 0));
        assert_eq!((sonda_de_json("{}"), sonda_de_json(r#"{"streams":[{"codec_type":"audio"}]}"#), sonda_de_json("lixo")), (None, None, None));
    }

    #[test]
    fn comandos_do_ffmpeg_saem_montados_sem_executar() {
        let com = args_acabar_video("a.mp4", "a_acab.mp4", true);
        assert_eq!((com[0].as_str(), com.last().map(String::as_str)), ("ffmpeg", Some("a_acab.mp4")));
        assert!(com.windows(2).any(|w| w == ["-af", "loudnorm=I=-16:TP=-1.5:LRA=11"]) && com.windows(2).any(|w| w == ["-maxrate", "6M"]) && !com.contains(&"-an".to_string()));
        let sem = args_acabar_video("a.mp4", "b.mp4", false);
        assert!(sem.contains(&"-an".to_string()) && !sem.contains(&"-af".to_string()) && sem.iter().any(|x| x.contains("crop=1080:1920")));
        assert_eq!(args_capa("a.mp4", "c.jpg", 8.0)[5], "4");
        assert_eq!(args_capa("a.mp4", "c.jpg", 0.2)[5], "0.5");
        assert_eq!(args_sondar("a.mp4").last().map(String::as_str), Some("a.mp4"));
    }

    #[test]
    fn promessas_sem_autorizacao_saem_da_legenda() {
        let (txt, tirados) = tirar_promessas("A esfiha mais saudável, 100% natural! Venha provar.", "esfiha gratinada");
        assert_eq!((txt.as_str(), tirados), ("A esfiha mais Venha provar.", vec!["100% natural".to_string(), "saudável".to_string()]));
        assert!(tirar_promessas("Feita com ingredientes frescos.", "").1.is_empty());
        assert!(tirar_promessas("Artesanal de verdade.", "esfiha artesanal").1.is_empty()); // o briefing autorizou
        let (txt, tirados) = tirar_promessas("O melhor da cidade, sem conservantes. Resultado garantido e zero açúcar!", "");
        assert_eq!((txt.as_str(), tirados.len()), ("Resultado e", 5));
    }

    #[test]
    fn biblioteca_e_desperdicio() {
        let d = std::env::temp_dir().join(format!("braniac-fornalha_estudio-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let p = json!({ "id": "P1", "cliente": "x", "tipo": "imagem", "prompt": "prompt bom" });
        guardar_vencedor(&d, &p, 92, 10.0).unwrap();
        guardar_vencedor(&d, &json!({ "id": "P2", "cliente": "x", "tipo": "imagem", "prompt": "prompt medio" }), 80, 20.0).unwrap();
        assert_eq!((melhores(&d, "x", "imagem", 1), melhores(&d, "x", "video", 2), melhores(&d, "x", "imagem", 2).len()), (vec!["prompt bom".to_string()], vec![], 2));
        registrar_desperdicio(&d, &p, "borrada", 1000.0).unwrap();
        assert_eq!((desperdicio(&d, 7.0, 2000.0), desperdicio(&d, 7.0, 1000.0 + 8.0 * 86400.0)), (1, 0));
        assert_eq!(estilo(&d, "x"), "");
        std::fs::create_dir_all(d.join("estilo")).unwrap();
        std::fs::write(d.join("estilo/casa-da-ana.md"), "luz quente").unwrap();
        assert_eq!(estilo(&d, "casa da ana"), "luz quente");
    }
}
