//! Artes da loja para o Instagram (artes_loja.py): as REGRAS e a montagem do pedido. Legenda escrita por IA so vale se passar na
//! validacao: nenhum preco fora da lista permitida (dossie da loja), nada de horario nem telefone, ate 700 caracteres; a que nao
//! passa e trocada pela legenda padrao. Fotos originais nunca sao alteradas (o desenho trabalha em copias).
//! Ficou de fora (closure/argumentos para quem chama): desenhar as artes (Pillow), rodar o ffmpeg do reel, perguntar ao conselho de
//! IAs e enviar pelo WhatsApp. Diferenca: nome da loja, site, cidade, precos permitidos e conceitos vem de configuracao.
#![allow(dead_code)]
use regex::Regex;
use std::collections::HashMap;

pub struct Conceito {
    pub id: String,
    /// "feed" (1080x1350) ou "story" (1080x1920)
    pub tipo: String,
    pub foto: String,
    pub titulo: String,
    pub sub: String,
    pub ideia: String,
}

pub struct Loja<'a> {
    pub nome: &'a str,
    pub site: &'a str,
    /// Uma frase para a IA: o que e a loja e onde fica.
    pub descricao: &'a str,
    pub hashtags: &'a str,
    /// Precos que existem no dossie, no formato "21,90". Qualquer "R$ x" fora daqui barra a legenda.
    pub precos_ok: &'a [&'a str],
}

fn re(p: &str) -> Regex {
    Regex::new(p).expect("regex fixa")
}

fn precos(txt: &str) -> Vec<String> {
    re(r"R\$\s*(\d+[.,]\d{2})").captures_iter(txt).map(|m| m[1].replace('.', ",")).collect()
}

/// Barra preco que nao existe no dossie, horario, telefone (4+ digitos seguidos) e texto vazio ou longo demais.
pub fn validar_legenda(txt: &str, precos_ok: &[&str]) -> bool {
    !txt.is_empty() && txt.chars().count() <= 700 && precos(txt).iter().all(|v| precos_ok.contains(&v.as_str())) && !re(r"\d{4,}").is_match(txt) && !re(r"(?i)\b\d{1,2}\s*h\b|\d{1,2}h\d{0,2}").is_match(txt)
}

pub fn legenda_padrao(titulo: &str, sub: &str, loja: &Loja) -> String {
    format!("{titulo}. {sub}.\n\nPeça pelo WhatsApp ou pelo site {} 🧡\n\n{}", loja.site, loja.hashtags)
}

/// Conceitos que citam preco fora da lista ou foto que nao existe: (id, motivo). Vazio = pode gerar.
pub fn conceitos_ruins(conceitos: &[Conceito], fotos: &[&str], precos_ok: &[&str]) -> Vec<(String, String)> {
    let mut r = Vec::new();
    for c in conceitos {
        if !fotos.contains(&c.foto.as_str()) {
            r.push((c.id.clone(), format!("foto nao encontrada: {}", c.foto)));
        }
        r.extend(precos(&format!("{} {}", c.titulo, c.sub)).into_iter().filter(|v| !precos_ok.contains(&v.as_str())).map(|v| (c.id.clone(), format!("preco fora do dossie: {v}"))));
    }
    r
}

/// A pergunta unica ao conselho: todas as legendas de uma vez, so com os precos e dias do resumo de cada post.
pub fn pergunta(conceitos: &[Conceito], loja: &Loja, resumo_reel: &str) -> String {
    let briefs: Vec<String> = conceitos.iter().map(|c| format!("### {} | {} | {} | ideia: {}", c.id, c.titulo, c.sub, c.ideia)).collect();
    format!(
        "Você é social media de {}. Escreva a LEGENDA de Instagram de cada post abaixo: tom caloroso e apetitoso, 2 a 4 linhas, até 3 emojis, uma chamada para pedir pelo WhatsApp ou pelo site {}, \
e 5 a 7 hashtags no fim. REGRAS: use SOMENTE os preços e dias que aparecem no resumo de cada post, NÃO invente preço, horário, telefone, sabor ou promoção. \
Responda exatamente neste formato, sem comentários extras:\n### id\ntexto da legenda\n(repita para cada id)\n\nPOSTS:\n{}\n### reel | {resumo_reel}",
        loja.descricao,
        loja.site,
        briefs.join("\n")
    )
}

/// Separa a resposta em {id: legenda}, so com as VALIDAS (o site do chat tira os "###": o id pode vir sozinho na linha).
pub fn separar_legendas(bruto: &str, precos_ok: &[&str]) -> HashMap<String, String> {
    let (cab, lixo) = (re(r"(?mi)^[ \t]*(?:#{1,4}[ \t]*)?(feed\d+|story\d+|reel)\b[^\n]*\n"), re(r"(?mi)^\s*(escrita|texto|legenda)\s*$"));
    let marcas: Vec<(usize, usize, String)> = cab.captures_iter(bruto).filter_map(|m| Some((m.get(0)?.start(), m.get(0)?.end(), m[1].to_lowercase()))).collect();
    let mut legs = HashMap::new();
    for (i, (_, fim, id)) in marcas.iter().enumerate() {
        let corpo = lixo.replace_all(&bruto[*fim..marcas.get(i + 1).map_or(bruto.len(), |p| p.0)], "").trim().to_string();
        if validar_legenda(&corpo, precos_ok) {
            legs.insert(id.clone(), corpo);
        }
    }
    legs
}

/// Pede as legendas ao conselho (`perguntar(ia, pergunta)`): primeira IA que devolver alguma valida ganha. Nenhuma = vazio
/// (todas as artes saem com a legenda padrao).
pub fn pedir_legendas(conceitos: &[Conceito], loja: &Loja, resumo_reel: &str, perguntar: &dyn Fn(&str, &str) -> Result<String, String>) -> HashMap<String, String> {
    let q = pergunta(conceitos, loja, resumo_reel);
    ["gemini", "chatgpt"].iter().filter_map(|ia| perguntar(ia, &q).ok()).map(|b| separar_legendas(&b, loja.precos_ok)).find(|l| !l.is_empty()).unwrap_or_default()
}

/// O legendas.txt que vai junto: a do conselho quando valida, senao a padrao. `reel`: (titulo, sub) da legenda padrao do video.
pub fn legendas_txt(conceitos: &[Conceito], legs: &HashMap<String, String>, loja: &Loja, reel: (&str, &str)) -> String {
    let mut blocos: Vec<String> = conceitos.iter().map(|c| format!("[{}] {}\n{}\n", c.id, c.titulo, legs.get(&c.id).cloned().unwrap_or_else(|| legenda_padrao(&c.titulo, &c.sub, loja)))).collect();
    blocos.push(format!("[reel] Vídeo (sem música: coloque uma música do Instagram)\n{}\n", legs.get("reel").cloned().unwrap_or_else(|| legenda_padrao(reel.0, reel.1, loja))));
    blocos.join("\n")
}

/// Quebra o texto em linhas que cabem na largura (`medir(texto)` = largura em pixels na fonte usada).
pub fn quebrar(texto: &str, largura: f64, medir: &dyn Fn(&str) -> f64) -> Vec<String> {
    let (mut linhas, mut atual) = (Vec::new(), String::new());
    for palavra in texto.split_whitespace() {
        let teste = format!("{atual} {palavra}").trim().to_string();
        if medir(&teste) <= largura {
            atual = teste;
        } else {
            linhas.push(std::mem::replace(&mut atual, palavra.to_string()));
        }
    }
    if !atual.is_empty() {
        linhas.push(atual);
    }
    linhas
}

/// Recorte para preencher w x h com ponto focal (fx, fy): (nova largura, nova altura, x, y do recorte).
pub fn cobrir(iw: u32, ih: u32, w: u32, h: u32, fx: f64, fy: f64) -> (u32, u32, u32, u32) {
    let r = (w as f64 / iw.max(1) as f64).max(h as f64 / ih.max(1) as f64);
    let (nw, nh) = (((iw as f64 * r + 0.5) as u32).max(w), ((ih as f64 * r + 0.5) as u32).max(h));
    (nw, nh, ((nw - w) as f64 * fx.clamp(0.0, 1.0)) as u32, ((nh - h) as f64 * fy.clamp(0.0, 1.0)) as u32)
}

/// Argumentos do ffmpeg do reel 1080x1920 (sem audio): slides com zoom suave e transicao. Quem chama executa.
pub fn args_reel(quadros: &[String], destino: &str, dur: f64, fade: f64) -> Result<Vec<String>, String> {
    if quadros.is_empty() {
        return Err("reel sem quadros".into());
    }
    let mut cmd: Vec<String> = ["ffmpeg", "-y", "-loglevel", "error"].iter().map(|s| s.to_string()).collect();
    cmd.extend(quadros.iter().flat_map(|q| ["-i".to_string(), q.clone()]));
    let mut filtros: Vec<String> = (0..quadros.len()).map(|i| format!("[{i}:v]scale=1188:2112,zoompan=z='1+0.00027*on':x='iw/2-(iw/zoom/2)':y='ih/2-(ih/zoom/2)':d={}:s=1080x1920:fps=30,setsar=1[v{i}]", ((dur + fade) * 30.0) as i64)).collect();
    let mut ult = "v0".to_string();
    for i in 1..quadros.len() {
        filtros.push(format!("[{ult}][v{i}]xfade=transition=fade:duration={fade}:offset={:.2}[x{i}]", i as f64 * dur));
        ult = format!("x{i}");
    }
    cmd.extend(["-filter_complex".to_string(), filtros.join(";"), "-map".into(), format!("[{ult}]")]);
    cmd.extend(["-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "20", "-movflags", "+faststart", destino].iter().map(|s| s.to_string()));
    Ok(cmd)
}

/// Envia tudo ao contato da loja: a abertura, depois cada arquivo (reel primeiro) e as legendas, com `esperar()` entre eles.
/// O envio e de quem chama (`texto`, `arquivo` devolvem o resultado); aqui so a ordem. Devolve (nome do arquivo, resultado).
pub fn enviar(arquivos: &[String], legendas: &str, abertura: &str, texto: &dyn Fn(&str) -> String, arquivo: &dyn Fn(&str) -> String, esperar: &dyn Fn()) -> Vec<(String, String)> {
    texto(abertura);
    arquivos.iter().map(String::as_str).chain([legendas]).map(|a| {
        esperar();
        (a.replace('\\', "/").rsplit('/').next().unwrap_or(a).to_string(), arquivo(a).chars().take(60).collect())
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const PRECOS: [&str; 2] = ["21,90", "13,50"];
    const LOJA: Loja = Loja { nome: "Loja Teste", site: "loja.exemplo", descricao: "uma loja de teste de delivery", hashtags: "#teste #delivery", precos_ok: &PRECOS };

    fn conceitos() -> Vec<Conceito> {
        [("feed1", "feed", "F1", "Doce", "Chocolate com morango", "doce"), ("feed5", "feed", "F2", "Promo", "2 por R$ 21,90 · só na segunda", "promo do dia")]
            .iter()
            .map(|c| Conceito { id: c.0.into(), tipo: c.1.into(), foto: c.2.into(), titulo: c.3.into(), sub: c.4.into(), ideia: c.5.into() })
            .collect()
    }

    #[test]
    fn legenda_barra_preco_horario_e_telefone_inventados() {
        assert!(validar_legenda("Segunda em Dobro: 2 esfihas por R$ 21,90! Peça no site.", &PRECOS));
        assert!(validar_legenda("Doce por R$ 13.50", &PRECOS)); // ponto ou virgula
        assert!(!validar_legenda("Promoção por R$ 19,90 hoje!", &PRECOS));
        assert!(!validar_legenda("Aberto das 18h às 23h30", &PRECOS));
        assert!(!validar_legenda("Abre 18 h", &PRECOS));
        assert!(!validar_legenda("Chama no 66 99999 9999", &PRECOS));
        assert!(!validar_legenda(&"x".repeat(800), &PRECOS) && !validar_legenda("", &PRECOS));
    }

    #[test]
    fn parser_aceita_id_sozinho_na_linha_e_descarta_invalidas() {
        let bruto = "feed1\n\nEscrita\n\nChocolate cremoso! 🍓\n\n#esfiha\n\nfeed5\nPor R$ 12,34 hoje\nreel\nVídeo lindo!\n";
        let visto = RefCell::new(Vec::new());
        let legs = pedir_legendas(&conceitos(), &LOJA, "vídeo de 20 s", &|ia, q| {
            visto.borrow_mut().push((ia.to_string(), q.to_string()));
            Ok(bruto.to_string())
        });
        let mut ids: Vec<&String> = legs.keys().collect();
        ids.sort();
        assert_eq!(ids, ["feed1", "reel"]);
        assert!(legs["feed1"].starts_with("Chocolate") && legs["feed1"].ends_with("#esfiha"));
        assert_eq!(visto.borrow().len(), 1); // a primeira IA ja serviu
        let q = &visto.borrow()[0].1;
        assert!(q.contains("NÃO invente preço") && q.contains("### feed5 | Promo | 2 por R$ 21,90") && q.ends_with("### reel | vídeo de 20 s"));
        assert_eq!(separar_legendas("### Feed1 | titulo\nOi!\n### STORY2\nTchau!", &PRECOS).len(), 2);
        // primeira IA fora, segunda so com lixo: nenhuma legenda (vale a padrao)
        let ordem = RefCell::new(Vec::new());
        let legs = pedir_legendas(&conceitos(), &LOJA, "r", &|ia, _| {
            ordem.borrow_mut().push(ia.to_string());
            if ia == "gemini" { Err("fora".into()) } else { Ok("feed1\nLigue 99999 9999".into()) }
        });
        assert!(legs.is_empty() && *ordem.borrow() == ["gemini", "chatgpt"]);
        let txt = legendas_txt(&conceitos(), &legs, &LOJA, ("Video", "da loja"));
        assert!(txt.contains("[feed1] Doce\nDoce. Chocolate com morango.\n\nPeça pelo WhatsApp ou pelo site loja.exemplo 🧡\n\n#teste #delivery\n") && txt.contains("[reel] Vídeo") && txt.contains("Video. da loja."));
    }

    #[test]
    fn conceitos_so_usam_fotos_e_precos_conhecidos() {
        assert!(conceitos_ruins(&conceitos(), &["F1", "F2"], &PRECOS).is_empty());
        let r = conceitos_ruins(&conceitos(), &["F1"], &["13,50"]);
        assert_eq!(r, [("feed5".to_string(), "foto nao encontrada: F2".to_string()), ("feed5".to_string(), "preco fora do dossie: 21,90".to_string())]);
    }

    #[test]
    fn quebra_recorte_reel_e_ordem_do_envio() {
        let medir = |t: &str| t.chars().count() as f64 * 10.0;
        assert_eq!(quebrar("um texto bem comprido aqui", 100.0, &medir), ["um texto", "bem", "comprido", "aqui"]);
        assert_eq!(cobrir(4000, 3000, 1080, 1350, 0.5, 0.5), (1800, 1350, 360, 0));
        assert_eq!(cobrir(1000, 4000, 1080, 1350, 0.5, 1.0), (1080, 4320, 0, 2970));
        let a = args_reel(&["q0.png".into(), "q1.png".into(), "q2.png".into()], "reel.mp4", 3.2, 0.5).unwrap();
        let f = &a[a.iter().position(|x| x == "-filter_complex").unwrap() + 1];
        assert!(f.contains(":d=111:s=1080x1920") && f.contains("[v0][v1]xfade=transition=fade:duration=0.5:offset=3.20[x1]") && f.ends_with("[x1][v2]xfade=transition=fade:duration=0.5:offset=6.40[x2]"));
        assert!(a.contains(&"[x2]".to_string()) && a.last().map(String::as_str) == Some("reel.mp4") && args_reel(&[], "r.mp4", 3.2, 0.5).is_err());
        assert!(args_reel(&["q0.png".into()], "r.mp4", 3.2, 0.5).unwrap().contains(&"[v0]".to_string()));
        let log = RefCell::new(Vec::new());
        let r = enviar(&["C:\\saida\\reel.mp4".into(), "feed1.jpg".into()], "saida/legendas.txt", "Seguem as artes", &|t| { log.borrow_mut().push(format!("texto:{t}")); String::new() }, &|a| { log.borrow_mut().push(format!("arq:{a}")); "✅ enviado".into() }, &|| log.borrow_mut().push("espera".into()));
        assert_eq!(*log.borrow(), ["texto:Seguem as artes", "espera", "arq:C:\\saida\\reel.mp4", "espera", "arq:feed1.jpg", "espera", "arq:saida/legendas.txt"]);
        assert_eq!(r[0], ("reel.mp4".to_string(), "✅ enviado".to_string()));
        assert_eq!(r[2].0, "legendas.txt");
    }
}
