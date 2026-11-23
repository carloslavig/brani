//! Titulo + hashtags do corte. A IA (LOCAL, nunca paga) entra por closure; sem ela, ou se ela responder torto, vale a reserva sem IA:
//! titulo do clipe dado pela comunidade, senao a 1a frase falada, senao um texto fixo. Modo jogo (`pausado`) nao acorda a IA.
#![allow(dead_code)]
use crate::empresa_dev::corta;
use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct Titulo {
    pub titulo: String,
    pub hashtags: Vec<String>,
}

fn espacos(t: &str) -> String {
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn primeira_frase(t: &str) -> String {
    let t = t.trim();
    let mut ant = ' ';
    for (i, c) in t.char_indices() {
        if c.is_whitespace() && matches!(ant, '.' | '!' | '?') {
            return t[..i].to_string();
        }
        ant = c;
    }
    t.to_string()
}

fn capitalizar(t: &str) -> String {
    let mut cs = t.chars();
    cs.next().map(|c| c.to_uppercase().chain(cs.flat_map(char::to_lowercase)).collect()).unwrap_or_default()
}

pub(crate) fn sem_repetir(v: Vec<String>) -> Vec<String> {
    let mut o: Vec<String> = vec![];
    for x in v {
        if !o.contains(&x) {
            o.push(x);
        }
    }
    o
}

/// Sem IA.
pub fn padrao(canal: &str, transcricao: &str, titulo_base: &str, idioma: &str, nome: &str) -> Titulo {
    let base = espacos(titulo_base);
    let frase = corta(&primeira_frase(transcricao), 70).trim_matches([' ', '.', ',']).to_string();
    let nome = if nome.is_empty() { capitalizar(canal) } else { nome.to_string() };
    let tag_canal = format!("#{}", canal.to_lowercase().chars().filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit()).collect::<String>());
    let en = idioma == "en";
    let titulo = if base.chars().filter(|c| c.is_alphanumeric() || *c == '_').count() >= 8 {
        format!("{} | {nome}", corta(&base, 70))
    } else if frase.chars().count() >= 12 {
        let mut cs = frase.chars();
        let t: String = cs.next().map(|c| c.to_uppercase().chain(cs).collect()).unwrap_or_default();
        if en { format!("{t} | {nome}") } else { t }
    } else if en {
        format!("{nome} stream highlight")
    } else {
        format!("Melhor momento da live do {nome}")
    };
    let tags = if en { vec!["#shorts", "#twitch", &tag_canal, "#twitchclips", "#streamer"] } else { vec!["#shorts", "#twitch", &tag_canal, "#cortes"] };
    Titulo { titulo, hashtags: tags.iter().map(|t| t.to_string()).collect() }
}

pub fn prompt_en(canal: &str, nome: &str, transcricao: &str, jogo: &str, titulo_base: &str) -> String {
    format!("You write titles for YouTube Shorts made from Twitch clips of the streamer {nome} (twitch.tv/{canal}){}.\nOriginal clip title (given by the viewers): \"{}\"\nTranscript:\n\"\"\"{}\"\"\"\nReply ONLY with JSON: {{\"titulo\": \"English, max 70 characters, include the streamer name, curiosity-driven and searchable, never claim something that is not in the clip, no ALL CAPS\", \"hashtags\": [\"#...\", 4 to 6 relevant English hashtags]}}",
        if jogo.is_empty() { String::new() } else { format!(", game {jogo}") }, corta(titulo_base, 100), corta(transcricao, 1500))
}

pub fn prompt_pt(canal: &str, transcricao: &str, jogo: &str) -> String {
    format!("Voce cria titulos de cortes (YouTube Shorts/TikTok) de live da Twitch do canal {canal}{}.\nTranscricao do corte:\n\"\"\"{}\"\"\"\nResponda SO um JSON: {{\"titulo\": \"ate 70 caracteres, portugues do Brasil, chamativo, sem mentir sobre o conteudo\", \"hashtags\": [\"#...\", 4 a 6 hashtags]}}",
        if jogo.is_empty() { String::new() } else { format!(", jogo {jogo}") }, corta(transcricao, 1500))
}

pub struct Pedido<'a> {
    pub canal: &'a str,
    pub transcricao: &'a str,
    pub jogo: &'a str,
    pub titulo_base: &'a str,
    pub idioma: &'a str,
    pub nome: &'a str,
}

/// `ia(prompt)` devolve o texto JSON da IA local. A transcricao e DADO: entra entre aspas triplas e a resposta so vale se for o JSON pedido.
pub fn gerar(p: &Pedido, usar_ia: bool, pausado: bool, ia: &dyn Fn(&str) -> Result<String, String>) -> Titulo {
    let base = padrao(p.canal, p.transcricao, p.titulo_base, p.idioma, p.nome);
    if !usar_ia || p.transcricao.is_empty() || pausado {
        return base;
    }
    let prompt = if p.idioma == "en" { prompt_en(p.canal, if p.nome.is_empty() { p.canal } else { p.nome }, p.transcricao, p.jogo, p.titulo_base) } else { prompt_pt(p.canal, p.transcricao, p.jogo) };
    let Some(d) = ia(&prompt).ok().and_then(|r| serde_json::from_str::<Value>(&r).ok()) else { return base };
    let titulo = corta(d["titulo"].as_str().unwrap_or("").trim().trim_matches('"'), 90);
    if titulo.chars().count() < 8 {
        return base;
    }
    let mut tags = vec!["#shorts".to_string()];
    for t in d["hashtags"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
        if let Some(t) = t.as_str().filter(|t| !t.trim().is_empty()) {
            let t: String = t.split_whitespace().collect();
            tags.push(if t.starts_with('#') { t } else { format!("#{t}") });
        }
    }
    Titulo { titulo, hashtags: sem_repetir(tags).into_iter().take(7).collect() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ped<'a>(transcricao: &'a str, base: &'a str, idioma: &'a str) -> Pedido<'a> {
        Pedido { canal: "fulano", transcricao, jogo: "", titulo_base: base, idioma, nome: "Fulano" }
    }

    #[test]
    fn reserva_sem_ia_usa_titulo_do_clipe_frase_ou_texto_fixo() {
        let nunca = |_: &str| -> Result<String, String> { panic!("IA nao pode ser chamada") };
        let t = gerar(&ped("", "crazy clutch in ranked", "en"), false, false, &nunca);
        assert_eq!(t.titulo, "crazy clutch in ranked | Fulano");
        assert!(t.hashtags.contains(&"#twitchclips".to_string()));
        assert_eq!(padrao("fulano_tv", "olha isso que jogada absurda! depois tem mais.", "gg", "pt", "").titulo, "Olha isso que jogada absurda!");
        assert_eq!(padrao("fulano_tv", "eita", "", "pt", ""), Titulo { titulo: "Melhor momento da live do Fulano_tv".into(), hashtags: vec!["#shorts".into(), "#twitch".into(), "#fulanotv".into(), "#cortes".into()] });
        assert_eq!(padrao("x", "", "", "en", "Bia").titulo, "Bia stream highlight");
        // modo jogo e transcricao vazia tambem nao acordam a IA
        assert_eq!(gerar(&ped("fala longa aqui", "", "pt"), true, true, &nunca), padrao("fulano", "fala longa aqui", "", "pt", "Fulano"));
        assert_eq!(gerar(&ped("", "", "pt"), true, false, &nunca).titulo, "Melhor momento da live do Fulano");
    }

    #[test]
    fn ia_local_so_vale_com_json_certo() {
        let boa = |p: &str| -> Result<String, String> {
            assert!(p.contains("\"\"\"ele ganhou a partida\"\"\""));
            Ok(r##"{"titulo": "\"A virada impossivel\"", "hashtags": ["jogo", "#virada top", "#shorts", "", 7]}"##.into())
        };
        let t = gerar(&ped("ele ganhou a partida", "", "pt"), true, false, &boa);
        assert_eq!(t, Titulo { titulo: "A virada impossivel".into(), hashtags: vec!["#shorts".into(), "#jogo".into(), "#viradatop".into()] });
        let base = padrao("fulano", "ele ganhou a partida", "", "pt", "Fulano");
        for ruim in [Ok("nao sou json".to_string()), Ok(r#"{"titulo": "curto"}"#.to_string()), Err("fora do ar".to_string())] {
            assert_eq!(gerar(&ped("ele ganhou a partida", "", "pt"), true, false, &|_| ruim.clone()), base);
        }
        assert!(prompt_en("c", "N", "t", "Jogo X", "b").contains(", game Jogo X."));
    }
}
