//! Legenda do corte: o FORMATO do arquivo .ass no estilo Shorts/TikTok (palavras grandes, 2-3 por vez, a palavra falada em amarelo)
//! e os argumentos do ffmpeg que tira o audio do trecho. Ficou de fora: o Whisper (transcricao entra pronta, palavra por palavra, por closure
//! em quem orquestra) e gravar o arquivo (`gerar_ass` devolve o texto).
#![allow(dead_code)]

/// (inicio, fim, texto), tempos relativos ao comeco do trecho.
pub type Palavra = (f64, f64, String);

pub fn args_extrair_wav(video: &str, inicio: f64, fim: f64, destino: &str) -> Vec<String> {
    ["ffmpeg", "-nostdin", "-v", "error", "-y", "-ss", &format!("{inicio:.2}"), "-i", video, "-t", &format!("{:.2}", fim - inicio), "-vn", "-ac", "1", "-ar", "16000", destino].iter().map(|x| x.to_string()).collect()
}

pub fn tempo(s: f64) -> String {
    let s = s.max(0.0);
    format!("{}:{:02}:{:05.2}", (s / 3600.0) as i64, (s % 3600.0 / 60.0) as i64, s % 60.0)
}

// chave abre comando no .ass: texto falado nunca pode virar comando
fn escapar(t: &str) -> String {
    t.replace('\\', "").replace('{', "(").replace('}', ")")
}

/// Ate 3 palavras ou 16 letras por grupo; pausa maior que 0,6 s abre grupo novo.
pub fn agrupar(palavras: &[Palavra], max_palavras: usize, max_chars: usize) -> Vec<Vec<Palavra>> {
    let (mut grupos, mut atual): (Vec<Vec<Palavra>>, Vec<Palavra>) = (vec![], vec![]);
    for w in palavras {
        let letras = atual.iter().map(|x| x.2.chars().count() + 1).sum::<usize>() + w.2.chars().count();
        if atual.last().is_some_and(|u| atual.len() >= max_palavras || letras > max_chars || w.0 - u.1 > 0.6) {
            grupos.push(std::mem::take(&mut atual));
        }
        atual.push(w.clone());
    }
    if !atual.is_empty() {
        grupos.push(atual);
    }
    grupos
}

/// Texto do .ass: cada grupo fica na tela inteiro, com a palavra da vez em amarelo.
pub fn gerar_ass(palavras: &[Palavra], margem_v: i64, tamanho: i64) -> String {
    let mut t = format!("[Script Info]\nScriptType: v4.00+\nPlayResX: 1080\nPlayResY: 1920\nWrapStyle: 2\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Corte,Arial Black,{tamanho},&H00FFFFFF,&H00FFFFFF,&H00000000,&H80000000,-1,0,0,0,100,100,1,0,1,7,3,2,60,60,{margem_v},1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n");
    for g in agrupar(palavras, 3, 16) {
        for (i, (ini, fim, _)) in g.iter().enumerate() {
            let prox = g.get(i + 1).map(|p| p.0).unwrap_or(*fim);
            let partes: Vec<String> = g.iter().enumerate().map(|(j, w)| {
                let p = escapar(&w.2).to_uppercase();
                if j == i { format!("{{\\c&H00F0FF&}}{p}{{\\c&HFFFFFF&}}") } else { p }
            }).collect();
            t += &format!("Dialogue: 0,{},{},Corte,,0,0,0,,{}\n", tempo(*ini), tempo(prox.max(ini + 0.05)), partes.join(" "));
        }
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(a: f64, b: f64, t: &str) -> Palavra {
        (a, b, t.to_string())
    }

    #[test]
    fn agrupa_e_pausa_longa_abre_grupo_novo() {
        let gs = agrupar(&[p(0.0, 0.3, "eu"), p(0.3, 0.6, "nao"), p(0.6, 0.9, "acredito"), p(2.0, 2.4, "caraca")], 3, 16);
        assert_eq!(gs.iter().map(Vec::len).collect::<Vec<_>>(), vec![3, 1]); // "eu nao acredito" tem 15 letras
        assert_eq!(gs[1][0].2, "caraca");
        assert_eq!(agrupar(&[p(0.0, 0.1, "a"), p(0.1, 0.2, "b"), p(0.2, 0.3, "c"), p(0.3, 0.4, "d")], 3, 16).len(), 2);
        assert_eq!(agrupar(&[p(0.0, 0.1, "inacreditavelmente"), p(0.1, 0.2, "bom")], 3, 16).len(), 2);
    }

    #[test]
    fn ass_destaca_a_palavra_e_chave_nao_vira_comando() {
        let t = gerar_ass(&[p(0.0, 0.5, "{oi}"), p(0.5, 1.0, "gente\\N")], 470, 80);
        assert!(t.contains("PlayResY: 1920") && t.matches("Dialogue:").count() == 2 && !t.contains("{oi}") && t.contains("(OI)"));
        assert!(t.contains("Dialogue: 0,0:00:00.00,0:00:00.50,Corte,,0,0,0,,{\\c&H00F0FF&}(OI){\\c&HFFFFFF&} GENTEN\n"));
        assert!(t.contains(",60,60,470,1\n") && t.contains("Arial Black,80,"));
        assert_eq!((tempo(3725.5), tempo(-1.0), tempo(59.5)), ("1:02:05.50".to_string(), "0:00:00.00".to_string(), "0:00:59.50".to_string()));
        assert_eq!(args_extrair_wav("v.ts", 10.0, 40.5, "a.wav")[10], "30.50");
    }
}
