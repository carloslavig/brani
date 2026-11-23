//! Render vertical 9:16 (1080x1920): so os ARGUMENTOS do ffmpeg/ffprobe (corte + enquadramento + legenda queimada + volume normalizado).
//! Ficou de fora: executar. `renderizar` chama a closure `rodar` com a NVENC e, se ela falhar, com o libx264 (CPU), como no Python.
#![allow(dead_code)]

fn v(a: &[&str]) -> Vec<String> {
    a.iter().map(|x| x.to_string()).collect()
}

pub fn filtro_video(modo: &str, ass_nome: Option<&str>) -> String {
    // centro: recorte central (camera, conversa). desfocado: video inteiro no meio e o mesmo video borrado ao fundo (nao perde HUD/placar)
    let f = if modo == "centro" {
        "[0:v]scale=-2:1920,crop=1080:1920,setsar=1[v0]"
    } else {
        "[0:v]split=2[a][b];[a]scale=1080:1920:force_original_aspect_ratio=increase,crop=1080:1920,boxblur=24:2,eq=brightness=-0.08[bg];[b]scale=1080:-2[fg];[bg][fg]overlay=(W-w)/2:(H-h)/2,setsar=1[v0]"
    };
    match ass_nome {
        Some(a) => format!("{f};[v0]ass={a}[v]"),
        None => format!("{f};[v0]null[v]"),
    }
}

/// `codec`: "nvenc" (placa de video) ou qualquer outro valor = libx264.
pub fn args_render(video: &str, inicio: f64, dur: f64, saida: &str, filtro: &str, codec: &str) -> Vec<String> {
    let mut a = v(&["ffmpeg", "-nostdin", "-v", "error", "-y", "-ss", &format!("{inicio:.2}"), "-i", video, "-t", &format!("{dur:.2}"), "-filter_complex", filtro, "-map", "[v]", "-map", "0:a:0?"]);
    a.extend(if codec == "nvenc" {
        v(&["-c:v", "h264_nvenc", "-preset", "p5", "-rc", "vbr", "-cq", "23", "-b:v", "8M", "-maxrate", "12M", "-bufsize", "16M"])
    } else {
        v(&["-c:v", "libx264", "-preset", "veryfast", "-crf", "21"])
    });
    a.extend(v(&["-pix_fmt", "yuv420p", "-r", "30", "-af", "loudnorm=I=-14:TP=-1.5:LRA=11", "-c:a", "aac", "-b:a", "160k", "-ar", "48000", "-movflags", "+faststart", saida]));
    a
}

/// `ass`: NOME do arquivo de legenda; o ffmpeg tem de rodar na pasta dele (evita escapar "C:" no filtro).
/// `rodar(args)` so devolve Ok se o mp4 saiu inteiro. Devolve o codec que funcionou.
pub fn renderizar(video: &str, inicio: f64, fim: f64, saida: &str, ass: Option<&str>, modo: &str, rodar: &dyn Fn(&[String]) -> Result<(), String>) -> Result<&'static str, String> {
    let filtro = filtro_video(modo, ass);
    let mut erro = String::new();
    for codec in ["nvenc", "x264"] {
        match rodar(&args_render(video, inicio, fim - inicio, saida, &filtro, codec)) {
            Ok(()) => return Ok(codec),
            Err(e) => erro = crate::empresa_dev::cauda(e.trim(), 400),
        }
    }
    Err(format!("ffmpeg falhou: {erro}"))
}

pub fn args_miniatura(video: &str, destino: &str, t: f64) -> Vec<String> {
    v(&["ffmpeg", "-nostdin", "-v", "error", "-y", "-ss", &format!("{t:.2}"), "-i", video, "-frames:v", "1", "-vf", "scale=540:-2", destino])
}

pub fn args_duracao(video: &str) -> Vec<String> {
    v(&["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "default=nw=1:nk=1", video])
}

/// Saida do ffprobe -> segundos; ilegivel = 0.
pub fn duracao(saida_ffprobe: &str) -> f64 {
    saida_ffprobe.trim().parse().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn filtro_e_argumentos() {
        assert!(filtro_video("centro", None).ends_with("setsar=1[v0];[v0]null[v]"));
        let f = filtro_video("desfocado", Some("legenda.ass"));
        assert!(f.contains("boxblur=24:2") && f.ends_with(";[v0]ass=legenda.ass[v]"));
        let a = args_render("v.ts", 12.5, 30.0, "s.mp4", &f, "nvenc");
        assert_eq!(a[5..11].join(" "), "-ss 12.50 -i v.ts -t 30.00");
        assert!(a.contains(&"h264_nvenc".to_string()) && a.last().map(String::as_str) == Some("s.mp4") && a.contains(&"loudnorm=I=-14:TP=-1.5:LRA=11".to_string()));
        assert!(args_render("v", 0.0, 1.0, "s", "f", "x264").contains(&"libx264".to_string()));
        assert_eq!((duracao(" 31.5\n"), duracao("N/A")), (31.5, 0.0));
        assert_eq!(args_miniatura("v", "m.jpg", 1.0)[6], "1.00");
        assert_eq!(args_duracao("v").len(), 8);
    }

    #[test]
    fn placa_falhou_cai_na_cpu_e_as_duas_falhando_da_erro() {
        let vistos = RefCell::new(vec![]);
        let so_cpu = |a: &[String]| {
            let nv = a.contains(&"h264_nvenc".to_string());
            vistos.borrow_mut().push(nv);
            if nv { Err("sem placa".to_string()) } else { Ok(()) }
        };
        assert_eq!(renderizar("v", 10.0, 40.0, "s.mp4", None, "desfocado", &so_cpu), Ok("x264"));
        assert_eq!(*vistos.borrow(), vec![true, false]);
        assert_eq!(renderizar("v", 0.0, 1.0, "s", None, "centro", &|_| Err(" quebrou \n".into())), Err("ffmpeg falhou: quebrou".into()));
    }
}
