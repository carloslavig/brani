//! Google Maps para o Jarvis (braniac/busca_maps.py): cartoes do Maps -> lugares com nome, nota, endereco e TELEFONE.
//! So o parse e as regras; abrir a pagina publica do Maps (navegador) entra como closure que devolve [{nome, url, texto}].
//! Travas: consulta curta ou com dado sensivel e recusada antes de abrir qualquer coisa; texto do Maps e DADO, nunca instrucao.
#![allow(dead_code)]
use regex::Regex;
use serde_json::{json, Value};

fn re(p: &str) -> Regex {
    Regex::new(p).expect("regex fixa")
}

fn digitos(fone: &str) -> String {
    fone.chars().filter(char::is_ascii_digit).collect()
}

/// Primeiro telefone do texto, no formato "(DD) NNNNN-NNNN".
pub fn telefone_em(texto: &str) -> Option<String> {
    re(r"\(?\b(\d{2})\)?\s?(9?\d{4})[-\s]?(\d{4})\b").captures(texto).map(|m| format!("({}) {}-{}", &m[1], &m[2], &m[3]))
}

/// (DD) 9NNNN-NNNN = celular (pode ter WhatsApp); (DD) NNNN-NNNN = fixo.
pub fn eh_celular(fone: &str) -> bool {
    let d = digitos(fone);
    d.len() == 11 && d.as_bytes()[2] == b'9'
}

/// Numero no formato que o WhatsApp mostra: +55 DD 9NNNN-NNNN. None se for fixo/invalido.
pub fn whatsapp_de(fone: &str) -> Option<String> {
    let d = digitos(fone);
    eh_celular(fone).then(|| format!("+55 {} {}-{}", &d[..2], &d[2..7], &d[7..]))
}

/// Cartao do Maps {nome, url, texto} -> {nome, url, nota, avaliacoes, endereco, telefone, site}. Campo ausente fica null.
pub fn interpretar(cartao: &Value) -> Value {
    let t = cartao["texto"].as_str().unwrap_or("");
    let nota = re(r"(\d,\d)\s*\((\d[\d.]*)\)").captures(t);
    let end = re(r"·\s*·?\s*([^\n·]{6,80})\n").captures(&format!("{t}\n")).map(|m| m[1].trim().to_string());
    json!({
        "nome": cartao["nome"].as_str().unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" "),
        "url": cartao["url"].as_str().unwrap_or(""),
        "nota": nota.as_ref().map(|n| n[1].to_string()),
        "avaliacoes": nota.as_ref().map(|n| n[2].to_string()),
        "endereco": end,
        "telefone": telefone_em(t),
        "site": t.contains("Website"),
    })
}

/// Endereco da busca publica do Maps para a consulta.
pub fn url_da_busca(consulta: &str) -> String {
    let q: String = consulta.bytes().map(|b| if b.is_ascii_alphanumeric() || b"_.-~/".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect();
    format!("https://www.google.com/maps/search/{q}?hl=pt-BR")
}

/// Lugares do Maps para a consulta. Err se a consulta for curta ou sensivel (nada e aberto). `abrir(consulta, n)` devolve os cartoes.
pub fn lugares(consulta: &str, n: usize, sensivel: &dyn Fn(&str) -> bool, abrir: &dyn Fn(&str, usize) -> Result<Vec<Value>, String>) -> Result<Vec<Value>, String> {
    let consulta = consulta.trim();
    if consulta.chars().count() < 4 || sensivel(consulta) {
        return Err("consulta invalida ou com dado sensivel".into());
    }
    Ok(abrir(consulta, n)?.iter().filter(|c| c["nome"].as_str().is_some_and(|x| !x.is_empty())).map(interpretar).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cartao() -> Value {
        json!({"nome": "Clínica Imagem Azul:  Ressonância,\n Vila Azul", "url": "https://www.google.com/maps/place/X/data=!4m7",
               "texto": "Clínica Imagem Azul\n4,9(420)\nCentro de diagnóstico médico por imagem ·  · R. das Flores, 200\nAberto · Fecha 23:00 · (11) 3043-0000\n\nWebsite\n\nRotas"})
    }

    #[test]
    fn interpreta_cartao_e_distingue_celular_de_fixo() {
        let c = interpretar(&cartao());
        assert_eq!(c, json!({"nome": "Clínica Imagem Azul: Ressonância, Vila Azul", "url": "https://www.google.com/maps/place/X/data=!4m7", "nota": "4,9", "avaliacoes": "420",
                             "endereco": "R. das Flores, 200", "telefone": "(11) 3043-0000", "site": true}));
        let vazio = interpretar(&json!({"nome": "Loja", "texto": "Loja\nSem avaliações"}));
        assert!(vazio["nota"].is_null() && vazio["endereco"].is_null() && vazio["telefone"].is_null() && vazio["site"] == false);
        assert_eq!(whatsapp_de("(11) 3043-0000"), None);
        assert_eq!(whatsapp_de("(11) 99876-5432").as_deref(), Some("+55 11 99876-5432"));
        assert_eq!(telefone_em("ligue 11 998765432 hoje").as_deref(), Some("(11) 99876-5432"));
        assert!(!eh_celular("") && !eh_celular("(11) 89876-54321"));
    }

    #[test]
    fn consulta_curta_ou_sensivel_nao_abre_nada() {
        let sens = |t: &str| t.contains("cpf");
        for ruim in ["", " ab ", "clinica cpf 123"] {
            assert_eq!(lugares(ruim, 5, &sens, &|_, _| panic!("nao abre")), Err("consulta invalida ou com dado sensivel".to_string()));
        }
        let abre = |c: &str, n: usize| -> Result<Vec<Value>, String> {
            assert_eq!((c, n), ("clinica Vila Azul", 5));
            Ok(vec![cartao(), json!({"nome": null, "texto": "anuncio"}), json!({"nome": "", "texto": "x"})])
        };
        let l = lugares("  clinica Vila Azul ", 5, &sens, &abre).unwrap();
        assert!(l.len() == 1 && l[0]["telefone"] == "(11) 3043-0000");
        assert_eq!(lugares("clinica boa", 5, &sens, &|_, _| Err("maps fora do ar".into())), Err("maps fora do ar".to_string()));
        assert_eq!(url_da_busca("clínica azul"), "https://www.google.com/maps/search/cl%C3%ADnica%20azul?hl=pt-BR");
    }
}
