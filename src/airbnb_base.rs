//! Base de conhecimento POR ANUNCIO do Airbnb (fatos no banco unificado, escopo "airbnb") + o questionario ao dono +
//! o texto "Info Casa". Fatos entram quando o dono responde "<anuncio>: texto"; "<hospede>: texto" e resposta a duvida aberta.
//! Diferenca do Python: a lista de anuncios (slug, regex do titulo, tipo) nao fica no codigo, vem de
//! dados/airbnb_anuncios.json: [["slug", "regex do titulo", "quarto|casa|apto"], ...] (a ordem vale: o primeiro que casar).
//! Este modulo so guarda e le texto: nunca altera anuncio nem fala com o Airbnb.
#![allow(dead_code)]
use crate::banco::Banco;
use regex::{Regex, RegexBuilder};
use rusqlite::{params, Connection};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::Path;

pub const INFO_CASA: &str = "info-casa";

/// (slug, regex do titulo, tipo).
pub struct Anuncios(pub Vec<(String, Regex, String)>);

impl Anuncios {
    /// Entrada com regex invalida ou tipo desconhecido e descartada.
    pub fn de_json(v: &Value) -> Anuncios {
        Anuncios(v.as_array().into_iter().flatten().filter_map(|a| {
            let (s, rx, tipo) = (a[0].as_str()?, a[1].as_str()?, a[2].as_str()?);
            let ok = !s.is_empty() && perguntas(tipo).is_some();
            ok.then(|| RegexBuilder::new(rx).case_insensitive(true).build().ok()).flatten().map(|r| (s.to_lowercase(), r, tipo.to_string()))
        }).collect())
    }

    /// Sem o arquivo = nenhum anuncio conhecido (slug None: nada e guardado no lugar errado).
    pub fn ler(dados: &Path) -> Anuncios {
        let v = std::fs::read_to_string(dados.join("airbnb_anuncios.json")).ok().and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok()).unwrap_or(Value::Null);
        Anuncios::de_json(&v)
    }

    pub fn slug(&self, anuncio: &str) -> Option<&str> {
        self.0.iter().find(|a| a.1.is_match(anuncio)).map(|a| a.0.as_str())
    }

    pub fn tipo(&self, anuncio: &str) -> Option<&str> {
        self.0.iter().find(|a| a.1.is_match(anuncio)).map(|a| a.2.as_str())
    }

    fn tem(&self, slug: &str) -> bool {
        self.0.iter().any(|a| a.0 == slug)
    }

    /// Pedido ao dono para montar a base de um anuncio. None = slug desconhecido.
    pub fn questionario(&self, s: &str) -> Option<String> {
        let tipo = &self.0.iter().find(|a| a.0 == s)?.2;
        Some(format!("Airbnb | montando a base do anúncio '{s}'. Me responde começando com '{s}:' e o que souber de: {}. Pode mandar em várias mensagens (cada uma começando com '{s}:').", perguntas(tipo)?))
    }

    /// msgs = textos do dono. -> (fatos de anuncio [(slug ou "casa:info-casa", texto)], respostas a hospede [(primeiro nome minusculo, texto)]).
    /// Texto que nao comeca com um anuncio conhecido nem com hospede de duvida aberta e ignorado.
    pub fn ler_respostas(&self, msgs: &[&str], hospedes_abertos: &BTreeSet<String>) -> (Vec<(String, String)>, Vec<(String, String)>) {
        let Ok(r) = Regex::new(r"(?s)^\s*([\w-]{3,20}(?: casa)?)\s*:\s*(.{4,})$") else { return (Vec::new(), Vec::new()) };
        let (mut fa, mut rh) = (Vec::new(), Vec::new());
        for t in msgs {
            let Some(m) = r.captures(t.trim()) else { continue };
            let (k, txt) = (m[1].to_lowercase().replace(' ', "-"), m[2].to_string());
            if k == INFO_CASA || k == "info" || k == "infocasa" {
                fa.push((format!("casa:{INFO_CASA}"), txt));
            } else if self.tem(&k) {
                fa.push((k, txt));
            } else if hospedes_abertos.contains(&k) {
                rh.push((k, txt));
            }
        }
        (fa, rh)
    }
}

fn perguntas(tipo: &str) -> Option<&'static str> {
    Some(match tipo {
        "quarto" => "wifi (nome/senha podem ir ao hóspede?), como chegar/endereço, estacionamento, horário e regra de uso da piscina e áreas comuns, barulho/visitas, ar-condicionado/toalhas",
        "casa" => "wifi, estacionamento, quem mora nos fundos e o que o hóspede pode usar, regras de barulho/visitas, o texto da 'Info Casa'",
        "apto" => "wifi, estacionamento/portaria, como chegar, regras do prédio, ar-condicionado/utensílios",
        _ => return None,
    })
}

/// Guarda um fato do anuncio (camada LOCAL: pode ter endereco e senha de wifi). Sem tema, a chave leva o instante.
pub fn guardar(banco: &Banco, s: &str, texto: &str, tema: Option<&str>, ts: f64) -> bool {
    let chave = format!("{s}:{}", tema.map_or((ts as i64).to_string(), String::from));
    banco.gravar_fato("airbnb", &chave, &texto.trim().chars().take(600).collect::<String>(), "local", "dono", None)
}

fn palavras4(t: &str) -> BTreeSet<String> {
    let t = t.to_lowercase();
    t.split(|c: char| !(c.is_alphanumeric() || c == '_')).filter(|w| w.chars().count() >= 4).map(String::from).collect()
}

/// Os n textos que mais dividem palavras (4+ letras) com a pergunta; empate fica na ordem dada. Pura.
pub fn mais_parecidos(textos: Vec<String>, pergunta: &str, n: usize) -> Vec<String> {
    let pal = palavras4(pergunta);
    let mut v: Vec<(usize, String)> = textos.into_iter().map(|t| (pal.intersection(&palavras4(&t)).count(), t)).collect();
    v.sort_by(|a, b| b.0.cmp(&a.0)); // estavel: empate nao embaralha
    v.into_iter().take(n).map(|x| x.1).collect()
}

fn ler_fatos(dados: &Path, sql: &str, args: &[&dyn rusqlite::ToSql]) -> Vec<(String, String)> {
    // banco ainda nao criado ou ocupado = nada sabido (quem chama pergunta ao dono em vez de inventar)
    let Ok(c) = Connection::open_with_flags(dados.join("jarvis.db"), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY) else { return Vec::new() };
    let Ok(mut st) = c.prepare(sql) else { return Vec::new() };
    let r = st.query_map(args, |r| Ok((r.get(0)?, r.get(1)?))).map(|it| it.flatten().collect()).unwrap_or_default();
    r
}

/// Fatos validos do anuncio (fora a Info Casa), os que mais batem com a pergunta primeiro.
pub fn fatos(dados: &Path, s: &str, pergunta: &str, n: usize, ts: f64) -> Vec<String> {
    let rows = ler_fatos(dados, "SELECT chave, texto FROM fatos WHERE escopo='airbnb' AND substr(chave, 1, ?)=? AND (validade IS NULL OR validade>?) ORDER BY id",
        params![s.chars().count() as i64 + 1, format!("{s}:"), ts]);
    mais_parecidos(rows.into_iter().filter(|r| !r.0.ends_with(&format!(":{INFO_CASA}"))).map(|r| r.1).collect(), pergunta, n)
}

/// Texto da "Info Casa" (outros espacos no mesmo terreno), se o dono ja mandou.
pub fn info_casa(dados: &Path) -> Option<String> {
    ler_fatos(dados, "SELECT chave, texto FROM fatos WHERE escopo='airbnb' AND chave=?", params![format!("casa:{INFO_CASA}")]).into_iter().next().map(|r| r.1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // anuncios inventados
    fn anuncios() -> Anuncios {
        Anuncios::de_json(&json!([["azul", "quarto azul", "quarto"], ["casa", "casa do lago", "casa"], ["centro", "apartamento completo", "apto"],
            ["loft", r"^\s*loft", "apto"], ["ruim", "(", "apto"], ["outro", "x", "barco"]]))
    }

    #[test]
    fn slug_e_tipo() {
        let a = anuncios();
        assert_eq!(a.0.len(), 4); // regex invalida e tipo desconhecido ficam de fora
        assert_eq!(a.slug("Quarto Azul · casa com piscina"), Some("azul"));
        assert_eq!(a.tipo("Casa do Lago · 3 quartos"), Some("casa"));
        assert_eq!(a.tipo("Apartamento completo - Perto da praia"), Some("apto"));
        assert_eq!((a.slug("Pousada qualquer"), a.tipo(""), a.slug("Um loft no centro")), (None, None, None));
        assert!(a.questionario("azul").unwrap().contains("'azul:'") && a.questionario("casa").unwrap().contains("Info Casa") && a.questionario("nada").is_none());
    }

    #[test]
    fn ler_respostas_separa_fato_de_anuncio_e_resposta_a_hospede() {
        let (fa, rh) = anuncios().ler_respostas(&["azul: wifi é X", "Elvira: Rua A, 10", "info casa: outros espaços", "oi", "Zé: não aberto", "Bia: sem dúvida aberta", "AZUL:\nlinha um\nlinha dois"],
            &BTreeSet::from(["elvira".to_string()]));
        assert!(fa.contains(&("azul".into(), "wifi é X".into())) && fa.contains(&("casa:info-casa".into(), "outros espaços".into())) && fa.contains(&("azul".into(), "linha um\nlinha dois".into())));
        assert_eq!(rh, vec![("elvira".to_string(), "Rua A, 10".to_string())]);
    }

    #[test]
    fn fatos_do_anuncio_no_banco_e_info_casa() {
        let p = std::env::temp_dir().join(format!("braniac-airbnb_base-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        assert!(fatos(&p, "azul", "wifi", 4, 0.0).is_empty() && info_casa(&p).is_none()); // sem banco: nada sabido
        let b = Banco::abrir(&p.join("jarvis.db")).unwrap();
        assert!(guardar(&b, "azul", "  estacionamento na rua  ", None, 1000.0) && guardar(&b, "azul", "a rede wifi se chama Exemplo", Some("wifi"), 1001.0));
        assert!(guardar(&b, "azulado", "de outro anúncio", Some("x"), 1002.0) && guardar(&b, "casa", "nos fundos há outra moradia", Some(INFO_CASA), 1003.0));
        assert!(!guardar(&b, "azul", "   ", None, 1004.0));
        b.gravar_fato("airbnb", "azul:velho", "promoção vencida do wifi", "local", "dono", Some(5.0));
        assert_eq!(fatos(&p, "azul", "qual a senha do wifi?", 4, 2000.0), vec!["a rede wifi se chama Exemplo", "estacionamento na rua"]);
        assert_eq!(fatos(&p, "azul", "", 1, 2000.0), vec!["estacionamento na rua"]);
        assert!(fatos(&p, "casa", "", 4, 2000.0).is_empty()); // a Info Casa nao entra como fato comum
        assert_eq!(info_casa(&p).as_deref(), Some("nos fundos há outra moradia"));
        drop(b);
        let _ = std::fs::remove_dir_all(&p);
    }
}
