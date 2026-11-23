//! Conferencia visual contra o gabarito (link/conferir.py): folha lado a lado (referencia | nosso) nos mesmos 10 pontos do video.
//! Sem IA. Aqui ficam as regras e os argumentos do ffmpeg; rodar o ffmpeg e tirar os quadros e de quem chama. Quem confere a folha
//! e uma pessoa (ou o agente) com o gabarito ao lado.
#![allow(dead_code)]
use serde_json::Value;

/// Pontos do video, em % da duracao.
pub const PONTOS: [u32; 10] = [2, 8, 18, 30, 42, 55, 68, 80, 92, 98];

/// Duracao do roteiro: o maior "fim" entre cenas e legenda (minimo 1 s).
pub fn duracao(roteiro: &Value) -> f64 {
    ["cenas", "legenda"].iter().flat_map(|k| roteiro[*k].as_array().into_iter().flatten()).filter_map(|x| x["fim"].as_f64()).fold(1.0, f64::max)
}

/// (ponto, segundo do video, nome do nosso quadro, nome do quadro de referencia).
pub fn quadros(dur: f64, referencia: &str) -> Vec<(u32, f64, String, String)> {
    PONTOS.iter().map(|p| (*p, (dur * *p as f64).round() / 100.0, format!("conf-{p:02}.png"), format!("{referencia}_{p:02}.jpg"))).collect()
}

/// Tira um quadro do video pronto.
pub fn args_quadro(video: &str, segundo: f64, saida: &str) -> Vec<String> {
    vec!["ffmpeg".into(), "-v".into(), "error".into(), "-y".into(), "-ss".into(), segundo.to_string(), "-i".into(), video.into(), "-frames:v".into(), "1".into(), saida.into()]
}

/// Filtro do ffmpeg: cada imagem em 216x384, fileiras de ate 10, fileiras empilhadas.
pub fn filtro(n: usize) -> String {
    let escala: String = (0..n).map(|i| format!("[{i}]scale=216:384[v{i}];")).collect();
    let linhas: Vec<String> = (0..n).step_by(10).map(|k| format!("{}hstack={}[l{}]", (k..(k + 10).min(n)).map(|i| format!("[v{i}]")).collect::<String>(), (n - k).min(10), k / 10)).collect();
    if linhas.len() == 1 {
        return format!("{escala}{}", linhas[0].trim_end_matches("[l0]")); // uma fileira so: a saida fica sem rotulo
    }
    format!("{escala}{};{}vstack={}", linhas.join(";"), (0..linhas.len()).map(|j| format!("[l{j}]")).collect::<String>(), linhas.len())
}

/// Monta a folha a partir dos pares que existem (referencia, nosso, referencia, nosso...). Nenhum par = erro.
pub fn args_folha(pares: &[String], saida: &str) -> Result<Vec<String>, String> {
    if pares.is_empty() {
        return Err("nenhum quadro gerado".into());
    }
    let mut a: Vec<String> = ["ffmpeg", "-v", "error", "-y"].iter().map(|s| s.to_string()).collect();
    a.extend(pares.iter().flat_map(|p| ["-i".to_string(), p.clone()]));
    a.extend(["-filter_complex".to_string(), filtro(pares.len()), saida.to_string()]);
    Ok(a)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pontos_filtro_e_folha() {
        assert_eq!(duracao(&json!({"cenas": [{"fim": 30.0}, {"fim": 80.5}], "legenda": [{"fim": 81.2}]})), 81.2);
        assert_eq!(duracao(&json!({})), 1.0);
        let q = quadros(200.0, "ref01");
        assert_eq!((q.len(), q[0].clone(), q[9].1), (10, (2, 4.0, "conf-02.png".to_string(), "ref01_02.jpg".to_string()), 196.0));
        assert_eq!(quadros(81.2, "r")[1].1, 6.5); // 81.2 * 8 % = 6.496 -> 2 casas
        assert_eq!(filtro(2), "[0]scale=216:384[v0];[1]scale=216:384[v1];[v0][v1]hstack=2");
        let f = filtro(20);
        assert!(f.contains("hstack=10[l0];") && f.ends_with("hstack=10[l1];[l0][l1]vstack=2"));
        assert!(filtro(12).contains("[v10][v11]hstack=2[l1]"));
        assert!(args_folha(&[], "s.jpg").is_err());
        let a = args_folha(&["m.jpg".into(), "n.png".into()], "s.jpg").unwrap();
        assert_eq!(a[4..8], ["-i", "m.jpg", "-i", "n.png"]);
        assert_eq!(a.last().map(String::as_str), Some("s.jpg"));
        assert_eq!(args_quadro("v.mp4", 6.5, "q.png")[5], "6.5");
    }
}
