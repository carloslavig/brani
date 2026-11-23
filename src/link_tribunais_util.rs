//! Pecas pequenas que os moldes dos tribunais repetem (no Python cada molde tem a sua copia): origem da receita pelos 2 primeiros
//! digitos da natureza, numero no formato brasileiro, soma por nome na ordem em que apareceu, acoes por chave na ordem em que
//! apareceram e leitura estrita de JSON. Tudo que falta ou nao e numero e Err: nunca vira zero calado.
#![allow(dead_code)]
use crate::link_tribunais_base::{Acao, Ano};
use regex::Regex;
use serde_json::Value;
use std::collections::BTreeMap;

pub const INTRA: &str = "Receitas intraorçamentárias";

/// Origem da receita pelos 2 primeiros digitos da natureza (classificacao nacional).
pub fn origem(c2: &str) -> Option<&'static str> {
    Some(match c2 {
        "11" => "Impostos, taxas e contribuições de melhoria",
        "12" => "Contribuições",
        "13" => "Receita patrimonial",
        "14" => "Receita agropecuária",
        "15" => "Receita industrial",
        "16" => "Receita de serviços",
        "17" => "Transferências correntes",
        "19" => "Outras receitas correntes",
        "21" => "Operações de crédito",
        "22" => "Alienação de bens",
        "23" => "Amortização de empréstimos",
        "24" => "Transferências de capital",
        "29" => "Outras receitas de capital",
        _ => return None,
    })
}

/// Os `n` primeiros caracteres (como o `s[:n]` do Python).
pub fn ini(s: &str, n: usize) -> &str {
    s.char_indices().nth(n).map_or(s, |(i, _)| &s[..i])
}

/// "1.234,56" -> 1234.56; vazio = 0; o que nao e numero e erro.
pub fn n_br(s: &str) -> Result<f64, String> {
    let t = s.trim();
    if t.is_empty() { Ok(0.0) } else { t.replace('.', "").replace(',', ".").parse().map_err(|_| format!("valor que não é número: {t:?}")) }
}

/// Inteiro de um texto do tribunal (mes, remessa, funcao).
pub fn int(s: &str, o_que: &str) -> Result<i64, String> {
    s.trim().parse().map_err(|_| format!("{o_que} que não é número: {s:?}"))
}

/// Soma por nome, guardando a ordem em que cada nome apareceu.
pub fn soma(orig: &mut Vec<(String, f64)>, nome: &str, v: f64) {
    match orig.iter_mut().find(|(n, _)| n == nome) {
        Some(o) => o.1 += v,
        None => orig.push((nome.to_string(), v)),
    }
}

/// Acoes por chave, na ordem em que apareceram (o dict do Python).
#[derive(Default)]
pub struct Acoes {
    pub lista: Vec<Acao>,
    onde: BTreeMap<String, usize>,
}

impl Acoes {
    /// Posicao da acao dessa chave e se ela e nova.
    pub fn idx(&mut self, chave: &str) -> (usize, bool) {
        if let Some(i) = self.onde.get(chave) {
            return (*i, false);
        }
        self.lista.push(Acao::default());
        self.onde.insert(chave.to_string(), self.lista.len() - 1);
        (self.lista.len() - 1, true)
    }
    pub fn entra(&mut self, chave: &str) -> &mut Acao {
        let (i, _) = self.idx(chave);
        &mut self.lista[i]
    }
}

/// Fecha o ano: receita da maior para a menor e totais = soma do que foi lido.
pub fn ano(acoes: Vec<Acao>, mut receita: Vec<(String, f64)>, doc_despesa: String, doc_receita: String) -> Ano {
    receita.sort_by(|x, y| y.1.total_cmp(&x.1));
    Ano { arrecadado: receita.iter().map(|o| o.1).sum(), liquidado: acoes.iter().map(|x| x.liquidado).sum(), empenhado: acoes.iter().map(|x| x.empenhado).sum(), acoes, receita, doc_despesa, doc_receita }
}

pub fn json(texto: &str, o_que: &str) -> Result<Value, String> {
    serde_json::from_str(texto).map_err(|e| format!("{o_que}: resposta do tribunal que não é JSON ({e})"))
}

pub fn lista<'a>(v: &'a Value, o_que: &str) -> Result<&'a Vec<Value>, String> {
    v.as_array().ok_or_else(|| format!("{o_que}: o tribunal não devolveu uma lista"))
}

/// Campo de texto obrigatorio.
pub fn txt<'a>(v: &'a Value, k: &str) -> Result<&'a str, String> {
    v[k].as_str().ok_or_else(|| format!("resposta do tribunal sem o campo {k}"))
}

/// Campo numerico obrigatorio (numero mesmo, ou texto que e numero).
pub fn num(v: &Value, k: &str) -> Result<f64, String> {
    match &v[k] {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
    .ok_or_else(|| format!("resposta do tribunal sem o número {k}"))
}

/// Como o `float(v or 0)` do Python: nulo e vazio = 0; texto que nao e numero e erro.
pub fn num0(v: &Value) -> Result<f64, String> {
    match v {
        Value::Null => Ok(0.0),
        Value::Number(n) => n.as_f64().ok_or_else(|| "número fora da faixa".to_string()),
        Value::String(s) if s.trim().is_empty() => Ok(0.0),
        Value::String(s) => s.trim().parse().map_err(|_| format!("valor que não é número: {s:?}")),
        _ => Err(format!("valor que não é número: {v}")),
    }
}

/// O `str(v)` do Python para um valor do JSON.
pub fn pystr(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "None".into(),
        Value::Number(n) if n.is_f64() => n.as_f64().map(|f| if f.fract() == 0.0 { format!("{f:.1}") } else { f.to_string() }).unwrap_or_default(),
        _ => v.to_string(),
    }
}

/// Entidades do HTML. shortcut: so as numericas e as 6 basicas; pagina de tribunal com outra entidade com nome -> acrescentar aqui.
pub fn desescapa(s: &str) -> String {
    let re = Regex::new(r"&(#[xX][0-9a-fA-F]+|#[0-9]+|amp|lt|gt|quot|apos|nbsp);").expect("regex fixa");
    re.replace_all(s, |c: &regex::Captures| {
        let e = &c[1];
        let n = match e {
            "amp" => Some(38),
            "lt" => Some(60),
            "gt" => Some(62),
            "quot" => Some(34),
            "apos" => Some(39),
            "nbsp" => Some(160),
            _ => e.strip_prefix("#x").or_else(|| e.strip_prefix("#X")).map_or_else(|| e[1..].parse().ok(), |h| u32::from_str_radix(h, 16).ok()),
        };
        n.and_then(char::from_u32).map_or_else(|| c[0].to_string(), String::from)
    })
    .into_owned()
}

/// Tabela HTML -> linhas de celulas (texto sem tags, entidades resolvidas), SEM a primeira linha (cabecalho).
pub fn tabela(html: &str, celula: &dyn Fn(&str) -> String) -> Vec<Vec<String>> {
    let tr = Regex::new(r"(?s)<tr[^>]*>(.*?)</tr>").expect("regex fixa");
    let td = Regex::new(r"(?s)<td[^>]*>(.*?)</td>").expect("regex fixa");
    tr.captures_iter(html).skip(1).map(|r| td.captures_iter(&r[1]).map(|c| celula(&c[1])).collect()).collect()
}

pub fn sem_tags(s: &str) -> String {
    desescapa(&Regex::new(r"<[^>]+>").expect("regex fixa").replace_all(s, "")).trim().to_string()
}

/// CSV sem cabecalho: cada registro com as colunas na posicao (o csv.reader do Python). Aspas, aspas dobradas e quebra dentro de aspas.
pub fn linhas_csv(texto: &str, sep: char) -> Vec<Vec<String>> {
    let (mut regs, mut reg, mut campo, mut aspas): (Vec<Vec<String>>, Vec<String>, String, bool) = (vec![], vec![], String::new(), false);
    let mut it = texto.trim_start_matches('\u{feff}').chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '"' if aspas && it.peek() == Some(&'"') => {
                campo.push('"');
                it.next();
            }
            '"' => aspas = !aspas,
            c if c == sep && !aspas => reg.push(std::mem::take(&mut campo)),
            '\r' if !aspas => {}
            '\n' if !aspas => {
                reg.push(std::mem::take(&mut campo));
                regs.push(std::mem::take(&mut reg));
            }
            c => campo.push(c),
        }
    }
    if !campo.is_empty() || !reg.is_empty() {
        reg.push(campo);
        regs.push(reg);
    }
    regs.retain(|r| r.len() > 1 || r.iter().any(|c| !c.is_empty()));
    regs
}

/// Celula `i` da linha; linha curta e erro (tabela cortada).
pub fn cel(x: &[String], i: usize) -> Result<&str, String> {
    x.get(i).map(String::as_str).ok_or_else(|| format!("linha da planilha do tribunal com {} colunas (falta a {i})", x.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn numeros_estritos_e_texto() {
        assert_eq!((n_br("1.234,56"), n_br(" "), n_br("-0,5")), (Ok(1234.56), Ok(0.0), Ok(-0.5)));
        assert!(n_br("R$ 5").is_err() && int("x", "mês").unwrap_err().contains("mês"));
        assert_eq!((ini("ação", 2), ini("a", 3), origem("17"), origem("99")), ("aç", "a", Some("Transferências correntes"), None));
        assert_eq!((num0(&json!(null)), num0(&json!("")), num0(&json!("2.5")), num0(&json!(3))), (Ok(0.0), Ok(0.0), Ok(2.5), Ok(3.0)));
        assert!(num0(&json!("dois")).is_err() && num0(&json!([1])).is_err() && num(&json!({}), "v").is_err() && txt(&json!({ "a": 1 }), "a").is_err());
        assert_eq!((pystr(&json!(6)), pystr(&json!(6.0)), pystr(&json!("x")), pystr(&json!(1.5))), ("6".into(), "6.0".into(), "x".into(), "1.5".into()));
        assert_eq!(desescapa("A&amp;B &#231;&#xE7; &nbsp;&coisa;"), "A&B çç \u{a0}&coisa;");
        let t = tabela("<table><tr><td>cab</td></tr><tr class=a><td>1</td><td align=r><b>x&lt;y</b>\n</td></tr></table>", &sem_tags);
        assert_eq!(t, vec![vec!["1".to_string(), "x<y".to_string()]]);
        assert!(cel(&t[0], 1) == Ok("x<y") && cel(&t[0], 2).is_err());
        assert_eq!(linhas_csv("\u{feff}a,\"b,\"\"c\"\"\"\r\n\r\n1,2", ','), [vec!["a", "b,\"c\""], vec!["1", "2"]]);
    }

    #[test]
    fn soma_e_acoes_na_ordem() {
        let mut o = vec![];
        (soma(&mut o, "b", 1.0), soma(&mut o, "a", 5.0), soma(&mut o, "b", 2.0));
        assert_eq!(o, [("b".to_string(), 3.0), ("a".to_string(), 5.0)]);
        let mut ac = Acoes::default();
        ac.entra("z").liquidado += 2.0;
        ac.entra("a").empenhado += 1.0;
        ac.entra("z").liquidado += 3.0;
        assert_eq!((ac.idx("a"), ac.idx("n")), ((1, false), (2, true)));
        let a = ano(ac.lista, o, "d".into(), "r".into());
        assert_eq!((a.liquidado, a.empenhado, a.arrecadado, a.receita[0].0.as_str(), a.acoes.len()), (5.0, 1.0, 8.0, "a", 3));
    }
}
