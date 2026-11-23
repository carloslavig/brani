//! Captura: regras da gravacao da live (streamlink) + chat (IRC anonimo, so leitura) e do download de VOD/clipe (yt-dlp).
//! Ficou de fora: abrir processo e socket. Aqui ficam os argumentos, os nomes dos arquivos, a leitura das linhas do IRC e a faxina das
//! gravacoes velhas (so arquivos que o proprio sistema criou em dados/cortes/gravacoes e dados/cortes/tmp; os cortes prontos ficam).
#![allow(dead_code)]
use crate::cortes_config::sub;
use crate::empresa_dev::corta;
use crate::tempo::Agora;
use serde_json::json;
use std::path::{Path, PathBuf};

fn v(a: &[&str]) -> Vec<String> {
    a.iter().map(|x| x.to_string()).collect()
}

/// (video .ts, chat .jsonl) da gravacao que comeca agora.
pub fn nomes_gravacao(pasta: &Path, login: &str, agora: &Agora) -> (PathBuf, PathBuf) {
    let carimbo = format!("{}_{:02}{:02}", agora.data.iso().replace('-', ""), agora.hora, agora.min);
    (pasta.join(format!("{login}_{carimbo}.ts")), pasta.join(format!("{login}_{carimbo}.chat.jsonl")))
}

pub fn args_streamlink(login: &str, video: &str) -> Vec<String> {
    v(&["streamlink", "--twitch-disable-ads", "--retry-streams", "10", "--retry-max", "3", "--stream-segment-threads", "2", "-o", video, &format!("https://www.twitch.tv/{login}"), "720p60,720p,best"])
}

/// Entrada no IRC da Twitch com usuario anonimo "justinfan" (somente leitura, sem conta e sem senha).
pub fn entrada_irc(login: &str, n: u32) -> String {
    format!("NICK justinfan{}\r\nJOIN #{login}\r\n", 10000 + n % 90000)
}

#[derive(Debug, PartialEq)]
pub enum Irc {
    /// Responder "PONG :tmi.twitch.tv\r\n".
    Ping,
    /// Linha pronta para o .jsonl do chat.
    Msg(String),
    Nada,
}

/// Le uma linha do IRC. `t` = segundos desde o inicio da gravacao. O texto do chat e DADO: so e guardado (ate 200 letras).
pub fn linha_irc(linha: &str, t: f64) -> Irc {
    if linha.starts_with("PING") {
        return Irc::Ping;
    }
    let Some((_, resto)) = linha.split_once(" PRIVMSG #") else { return Irc::Nada };
    let msg = resto.split_once(" :").map(|x| x.1).unwrap_or("");
    Irc::Msg(json!({"t": (t * 10.0).round() / 10.0, "msg": corta(msg, 200)}).to_string())
}

/// yt-dlp: video ate 1080p (ou so o audio). `trecho` = (ini, fim) em segundos baixa so aquele pedaco do VOD.
pub fn args_baixar(url: &str, destino_sem_ext: &str, so_audio: bool, trecho: Option<(f64, f64)>) -> Vec<String> {
    let fmt = if so_audio { "bestaudio/Audio_Only/worst" } else { "bestvideo[height<=1080]+bestaudio/best[height<=1080]/best" };
    let mut a = v(&["yt-dlp", "--no-progress", "--no-warnings", "-f", fmt, "-o", &format!("{destino_sem_ext}.%(ext)s"), "--print", "after_move:filepath"]);
    if let Some((i, f)) = trecho {
        a.extend(v(&["--download-sections", &format!("*{i:.1}-{f:.1}"), "--force-keyframes-at-cuts"]));
    }
    a.push(url.to_string());
    a
}

/// Caminho do arquivo baixado: a ultima linha da saida do yt-dlp se existir; senao o ultimo `destino.*` da pasta.
pub fn arquivo_baixado(saida: &str, destino_sem_ext: &Path) -> Result<PathBuf, String> {
    if let Some(p) = saida.lines().filter(|l| !l.trim().is_empty()).next_back().map(PathBuf::from).filter(|p| p.exists()) {
        return Ok(p);
    }
    let prefixo = format!("{}.", destino_sem_ext.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
    let mut achados: Vec<PathBuf> = destino_sem_ext.parent().and_then(|d| std::fs::read_dir(d).ok()).map(|it| it.flatten().map(|e| e.path()).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(&prefixo))).collect()).unwrap_or_default();
    achados.sort();
    achados.pop().ok_or_else(|| "download terminou sem arquivo".to_string())
}

fn velhos(pasta: &Path, limite: f64, apagados: &mut usize) {
    for e in std::fs::read_dir(pasta).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            velhos(&p, limite, apagados);
        } else if e.metadata().ok().and_then(|m| m.modified().ok()).and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok()).is_some_and(|m| m.as_secs_f64() < limite) && std::fs::remove_file(&p).is_ok() {
            *apagados += 1;
        }
    }
}

/// Apaga gravacoes/downloads temporarios mais velhos que `dias`. Devolve quantos arquivos sairam.
pub fn limpar_gravacoes(dados: &Path, dias: f64, ts: f64) -> usize {
    let mut n = 0;
    for nome in ["gravacoes", "tmp"] {
        velhos(&sub(dados, nome), ts - dias * 86400.0, &mut n);
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;

    #[test]
    fn nomes_argumentos_e_irc() {
        let agora = Agora { data: Data::civil(2026, 10, 10).unwrap(), hora: 9, min: 5, seg: 0 };
        let (video, chat) = nomes_gravacao(Path::new("g"), "ana", &agora);
        assert!(video.ends_with("ana_20261010_0905.ts") && chat.ends_with("ana_20261010_0905.chat.jsonl"));
        let a = args_streamlink("ana", "v.ts");
        assert_eq!((a[9].as_str(), a[10].as_str(), a[11].as_str()), ("v.ts", "https://www.twitch.tv/ana", "720p60,720p,best"));
        assert_eq!(entrada_irc("ana", 123456), "NICK justinfan43456\r\nJOIN #ana\r\n");
        assert_eq!(linha_irc("PING :tmi.twitch.tv", 1.0), Irc::Ping);
        assert_eq!(linha_irc(":bia!bia@bia.tmi.twitch.tv PRIVMSG #ana :KEKW : olha", 12.34), Irc::Msg(r#"{"msg":"KEKW : olha","t":12.3}"#.into()));
        assert_eq!(linha_irc(":x PRIVMSG #ana", 1.0), Irc::Msg(r#"{"msg":"","t":1.0}"#.into()));
        assert_eq!(linha_irc(":tmi.twitch.tv 001 justinfan1 :Welcome", 1.0), Irc::Nada);
        let Irc::Msg(longa) = linha_irc(&format!(":x PRIVMSG #ana :{}", "a".repeat(500)), 0.0) else { panic!() };
        assert!(longa.len() < 230);
        let b = args_baixar("https://u", "tmp/trecho0", false, Some((10.0, 40.25)));
        assert_eq!(b[b.len() - 3..].join(" "), "*10.0-40.2 --force-keyframes-at-cuts https://u".replace("40.2", &format!("{:.1}", 40.25)));
        assert!(args_baixar("u", "d", true, None).contains(&"bestaudio/Audio_Only/worst".to_string()));
    }

    #[test]
    fn acha_o_baixado_e_limpa_so_o_velho_das_pastas_proprias() {
        let d = crate::cortes_fila::tmp("captura");
        let tmp = sub(&d, "tmp");
        let alvo = tmp.join("clipe_x");
        assert!(arquivo_baixado("", &alvo).is_err());
        std::fs::write(tmp.join("clipe_x.mp4"), "v").unwrap();
        assert_eq!(arquivo_baixado("aviso\n\n", &alvo).unwrap(), tmp.join("clipe_x.mp4"));
        let outro = tmp.join("outro.mkv");
        std::fs::write(&outro, "v").unwrap();
        assert_eq!(arquivo_baixado(&format!("x\n{}\n", outro.display()), &alvo).unwrap(), outro);
        let pronto = sub(&d, "saida").join("corte.mp4");
        std::fs::write(&pronto, "v").unwrap();
        std::fs::create_dir_all(sub(&d, "gravacoes").join("sub")).unwrap();
        std::fs::write(sub(&d, "gravacoes").join("sub").join("live.ts"), "v").unwrap();
        let agora = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64();
        assert_eq!(limpar_gravacoes(&d, 3.0, agora), 0);
        assert_eq!(limpar_gravacoes(&d, 3.0, agora + 4.0 * 86400.0), 3);
        assert!(pronto.exists() && !outro.exists());
    }
}
