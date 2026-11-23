//! Analise LOCAL de video (braniac/video_analise.py): quadros (ffmpeg) + descricao de cada quadro (modelo de visao local) + transcricao.
//! Aqui so a MONTAGEM: os argumentos do ffprobe/ffmpeg (nada e executado), em que segundos tirar os quadros, a leitura da duracao e o
//! resumo em texto. Executar ffmpeg, descrever o quadro e transcrever ficam com quem liga.
#![allow(dead_code)]
use serde_json::Value;
use std::path::Path;

pub const MODELO_VISAO: &str = "moondream:latest"; // cabe numa placa de 4 GB
pub const PROMPT: &str = "Describe this vertical video frame in detail: layout, every text you can read on screen (titles, labels, captions, watermark), colors, 3D objects and their position.";

fn s(p: &Path) -> String {
    p.to_string_lossy().to_string()
}

fn lista(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

pub fn args_ffprobe(video: &Path) -> Vec<String> {
    lista(&["ffprobe", "-v", "error", "-show_entries", "format=duration:stream=codec_name,width,height,r_frame_rate", "-of", "json", &s(video)])
}

/// Duracao em segundos, da saida JSON do ffprobe (0 se nao der para ler).
pub fn duracao(saida_ffprobe: &str) -> f64 {
    let v: Value = serde_json::from_str(saida_ffprobe).unwrap_or_default();
    let d = &v["format"]["duration"];
    d.as_f64().or_else(|| d.as_str().and_then(|x| x.parse().ok())).filter(|x| x.is_finite() && *x > 0.0).unwrap_or(0.0)
}

/// Os `n` instantes (no meio de cada fatia do video) e o nome do arquivo de cada quadro.
pub fn quadros(n: usize, duracao: f64) -> Vec<(f64, String)> {
    let d = duracao.max(1.0);
    (0..n).map(|i| d * (i as f64 + 0.5) / n as f64).map(|t| (t, format!("q{:03}s.jpg", t as i64))).collect()
}

/// Um quadro no instante `t`, com 540 px de largura.
pub fn args_quadro(video: &Path, t: f64, arq: &Path) -> Vec<String> {
    lista(&["ffmpeg", "-loglevel", "error", "-y", "-ss", &t.to_string(), "-i", &s(video), "-frames:v", "1", "-vf", "scale=540:-2", &s(arq)])
}

/// O audio em mono, 16 kHz (o que o transcritor local espera).
pub fn args_audio(video: &Path, wav: &Path) -> Vec<String> {
    lista(&["ffmpeg", "-loglevel", "error", "-y", "-i", &s(video), "-ac", "1", "-ar", "16000", &s(wav)])
}

/// Nome da pasta de resultado: o nome do video sem espacos, ate 60 caracteres.
pub fn pasta(video: &Path) -> String {
    video.file_stem().map(|x| x.to_string_lossy().replace(' ', "-")).unwrap_or_default().chars().take(60).collect()
}

/// resumo.md: a fala [(inicio em s, texto)] e a descricao de cada quadro [(arquivo, descricao)].
pub fn resumo_md(nome: &str, duracao: f64, fala: &[(f64, String)], quadros: &[(String, String)]) -> String {
    let mut md = vec![format!("# {nome}"), format!("Duracao: {duracao:.0} s"), String::new(), "## Fala (Whisper local)".to_string()];
    if fala.is_empty() {
        md.push("(sem fala reconhecida)".into());
    }
    md.extend(fala.iter().map(|(t, x)| format!("- [{t:.0}s] {x}")));
    md.extend([String::new(), "## Quadros (moondream local)".to_string()]);
    md.extend(quadros.iter().map(|(q, d)| format!("### {q}\n{d}")));
    md.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monta_os_argumentos_sem_executar_nada() {
        let (v, q) = (Path::new("C:/videos/meu video; rm -rf.mp4"), Path::new("C:/saida/frames/q002s.jpg"));
        let a = args_quadro(v, 2.5, q);
        assert_eq!(a, ["ffmpeg", "-loglevel", "error", "-y", "-ss", "2.5", "-i", "C:/videos/meu video; rm -rf.mp4", "-frames:v", "1", "-vf", "scale=540:-2", "C:/saida/frames/q002s.jpg"]);
        assert!(a.iter().filter(|x| x.contains("rm -rf")).count() == 1); // o caminho e UM argumento: nunca passa por shell
        assert!(args_ffprobe(v)[0] == "ffprobe" && args_ffprobe(v).last().unwrap().ends_with(".mp4"));
        assert_eq!(args_audio(v, Path::new("a.wav"))[6..], ["-ac", "1", "-ar", "16000", "a.wav"]);
        assert_eq!(quadros(4, 40.0), [(5.0, "q005s.jpg".to_string()), (15.0, "q015s.jpg".to_string()), (25.0, "q025s.jpg".to_string()), (35.0, "q035s.jpg".to_string())]);
        assert!(quadros(2, 0.0) == [(0.25, "q000s.jpg".to_string()), (0.75, "q000s.jpg".to_string())] && quadros(0, 10.0).is_empty());
        assert_eq!((duracao(r#"{"format": {"duration": "12.5"}}"#), duracao(r#"{"format": {"duration": 3}}"#), duracao("lixo"), duracao(r#"{"format": {"duration": "nan"}}"#)), (12.5, 3.0, 0.0, 0.0));
        assert_eq!(pasta(v), "meu-video;-rm--rf");
        assert_eq!(pasta(Path::new(&format!("{}.mp4", "a".repeat(80)))).len(), 60);
        let md = resumo_md("v.mp4", 12.4, &[(3.6, "bom dia".into())], &[("q005s.jpg".into(), "a chart".into())]);
        assert_eq!(md, "# v.mp4\nDuracao: 12 s\n\n## Fala (Whisper local)\n- [4s] bom dia\n\n## Quadros (moondream local)\n### q005s.jpg\na chart");
        assert!(resumo_md("v.mp4", 0.0, &[], &[]).contains("(sem fala reconhecida)"));
    }
}
