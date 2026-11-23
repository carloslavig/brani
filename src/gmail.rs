//! Gmail do dono, SOMENTE LEITURA (escopo gmail.readonly): ver nao lidas, buscar, ler um e-mail e filtrar vagas. NAO envia, apaga nem altera.
//! PRIVACIDADE: o conteudo dos e-mails NUNCA vai para IA online. Estas funcoes devolvem texto para o PROPRIO dono (resposta direta);
//! nada aqui passa pelo roteador de IAs, e o filtro de vagas e 100% local (regex).
//! Ficou de fora: a rede e o token. `get(caminho, parametros)` e a closure que chama a API oficial ja autenticada e devolve o JSON
//! (`erro_de_status` traduz a recusa). Este modulo NAO guarda Client secret, token nem senha (o Python guardava em data/gmail.json):
//! o login OAuth fica com quem integra; aqui so ha a montagem do endereco de login (`url_login`, com PKCE e state) e a conferencia do retorno.
//! A "ultima lista" (para «le o e-mail 2») volta em `Lista.ids` e quem chama guarda; a data sai no fuso passado em `fuso_min`.
#![allow(dead_code)]
use crate::empresa_dev::{corta, rx};
use crate::tempo::Data;
use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

pub const REDIRECT: &str = "http://127.0.0.1:8765/email/callback";
pub const ESCOPO: &str = "https://www.googleapis.com/auth/gmail.readonly";
pub const COMO_CONECTAR: &str = "Falta conectar o Gmail (leitura). Abra http://127.0.0.1:8765/email no PC: tem o passo a passo e leva uns 5 minutos.";
pub const SALARIO_MIN_BRL: f64 = 5000.0;

pub type Get<'a> = &'a dyn Fn(&str, &[(&str, String)]) -> Result<Value, String>;

pub fn validar_cliente(client_id: &str, client_secret: &str) -> Result<(), String> {
    if !client_id.trim().ends_with(".apps.googleusercontent.com") || client_secret.trim().chars().count() < 10 {
        return Err("o Client ID deve terminar com .apps.googleusercontent.com e o segredo (Client secret) precisa ser colado inteiro".into());
    }
    Ok(())
}

fn pct(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// Endereco do login no Google: so leitura, com PKCE (`desafio` = SHA-256 do verificador em base64url) e `estado` sorteado por quem chama.
pub fn url_login(client_id: &str, desafio: &str, estado: &str) -> Result<String, String> {
    if client_id.trim().is_empty() {
        return Err("antes, cole o Client ID e o segredo do Google em http://127.0.0.1:8765/email".into());
    }
    let q = [("client_id", client_id.trim()), ("redirect_uri", REDIRECT), ("response_type", "code"), ("scope", ESCOPO), ("access_type", "offline"), ("prompt", "consent"),
             ("code_challenge_method", "S256"), ("code_challenge", desafio), ("state", estado)];
    Ok(format!("https://accounts.google.com/o/oauth2/v2/auth?{}", q.iter().map(|(k, v)| format!("{k}={}", pct(v))).collect::<Vec<_>>().join("&")))
}

/// Outro site nao consegue concluir o login: o state tem de ser o que foi sorteado.
pub fn retorno_confere(codigo: &str, estado_recebido: &str, estado_guardado: &str) -> Result<(), String> {
    if codigo.is_empty() || estado_guardado.is_empty() || estado_recebido != estado_guardado { Err("retorno do login inválido".into()) } else { Ok(()) }
}

/// Recusa da API em frase clara; None = 200.
pub fn erro_de_status(status: u16, corpo: &str) -> Option<String> {
    match status {
        200 => None,
        403 if corpo.to_lowercase().contains("insufficient") => Some("o Gmail recusou: falta a permissão de leitura. Entre de novo em /email/login e marque a permissão.".into()),
        403 | 429 => Some("o Gmail limitou o acesso agora (cota). Tente de novo em alguns minutos.".into()),
        outro => Some(format!("o Gmail respondeu {outro}")),
    }
}

fn bytes_para_texto(b: &[u8], charset: &str) -> String {
    if charset.to_lowercase().starts_with("utf") { String::from_utf8_lossy(b).into_owned() } else { b.iter().map(|&c| c as char).collect() } // o resto e lido como latin-1
}

fn b64(dados: &str) -> Vec<u8> {
    crate::base64::decodificar(&dados.replace('-', "+").replace('_', "/")).unwrap_or_default()
}

/// Cabecalho com palavras codificadas (=?UTF-8?B?...?= ou ?Q?) em texto.
pub fn decodificar_cabecalho(v: &str) -> String {
    static R: OnceLock<Regex> = OnceLock::new();
    static ENTRE: OnceLock<Regex> = OnceLock::new();
    let v = rx(&ENTRE, r"(\?=)\s+(=\?)").replace_all(v, "$1$2");
    rx(&R, r"=\?([^?\s]+)\?([bBqQ])\?([^?]*)\?=").replace_all(&v, |c: &regex::Captures| {
        let bytes = if c[2].eq_ignore_ascii_case("b") {
            b64(&c[3])
        } else {
            let (mut out, q) = (vec![], c[3].as_bytes());
            let mut i = 0;
            while i < q.len() {
                match (q[i], q.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok())) {
                    (b'=', Some(x)) => {
                        out.push(x);
                        i += 2;
                    }
                    (b'_', _) => out.push(b' '),
                    (x, _) => out.push(x),
                }
                i += 1;
            }
            out
        };
        bytes_para_texto(&bytes, &c[1])
    }).trim().to_string()
}

pub fn cabecalho(msg: &Value, nome: &str) -> String {
    msg["payload"]["headers"].as_array().and_then(|hs| hs.iter().find(|h| h["name"].as_str().unwrap_or("").eq_ignore_ascii_case(nome))).map(|h| decodificar_cabecalho(h["value"].as_str().unwrap_or(""))).unwrap_or_default()
}

/// "Mon, 05 Oct 2026 10:30:00 -0300" -> "05/10 10:30" no fuso local (`fuso_min` = minutos a leste de UTC). Ilegivel = "".
pub fn data_local(valor: &str, fuso_min: i64) -> String {
    static R: OnceLock<Regex> = OnceLock::new();
    let Some(c) = rx(&R, r"(\d{1,2})\s+([A-Za-z]{3})\w*\s+(\d{4})\s+(\d{1,2}):(\d{2})(?::(\d{2}))?\s*([+-]\d{4})?").captures(valor) else { return String::new() };
    let n = |i: usize| c.get(i).and_then(|m| m.as_str().parse::<i64>().ok()).unwrap_or(0);
    let Some(mes) = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"].iter().position(|m| c[2].eq_ignore_ascii_case(m)) else { return String::new() };
    let Some(dia) = Data::civil(n(3), mes as i64 + 1, n(1)) else { return String::new() };
    let fuso = c.get(7).map(|m| m.as_str()).map(|z| (if z.starts_with('-') { -1 } else { 1 }) * (z[1..3].parse::<i64>().unwrap_or(0) * 60 + z[3..5].parse::<i64>().unwrap_or(0))).unwrap_or(0);
    let t = dia.0 * 1440 + n(4) * 60 + n(5) - fuso + fuso_min;
    format!("{} {:02}:{:02}", Data(t.div_euclid(1440)).dia_mes(), t.rem_euclid(1440) / 60, t.rem_euclid(60))
}

/// Nome de quem mandou; sem nome, o endereco.
pub fn remetente(msg: &Value) -> String {
    let de = cabecalho(msg, "From");
    let (nome, end) = match de.split_once('<') {
        Some((n, e)) => (n.trim().trim_matches('"').trim().to_string(), e.trim_end_matches('>').trim().to_string()),
        None => (String::new(), de.trim().to_string()),
    };
    [nome, end].into_iter().find(|x| !x.is_empty()).unwrap_or_else(|| "?".into())
}

fn sem_entidades(t: &str) -> String {
    static R: OnceLock<Regex> = OnceLock::new();
    rx(&R, r"&(#x?[0-9a-fA-F]+|[a-zA-Z]+);").replace_all(t, |c: &regex::Captures| {
        let e = &c[1];
        let num = if let Some(h) = e.strip_prefix("#x").or_else(|| e.strip_prefix("#X")) { u32::from_str_radix(h, 16).ok() } else if let Some(d) = e.strip_prefix('#') { d.parse().ok() } else { None };
        match (e, num.and_then(char::from_u32)) {
            (_, Some(ch)) => ch.to_string(),
            ("amp", _) => "&".into(),
            ("lt", _) => "<".into(),
            ("gt", _) => ">".into(),
            ("quot", _) => "\"".into(),
            ("apos", _) => "'".into(),
            ("nbsp", _) => "\u{a0}".into(),
            _ => c[0].to_string(),
        }
    }).into_owned()
}

fn html_para_texto(h: &str) -> String {
    static SCRIPT: OnceLock<Regex> = OnceLock::new();
    static STYLE: OnceLock<Regex> = OnceLock::new();
    static QUEBRA: OnceLock<Regex> = OnceLock::new();
    static TAG: OnceLock<Regex> = OnceLock::new();
    static ESP: OnceLock<Regex> = OnceLock::new();
    static LINHA: OnceLock<Regex> = OnceLock::new();
    let t = rx(&SCRIPT, r"(?is)<script.*?</script>").replace_all(h, " ");
    let t = rx(&STYLE, r"(?is)<style.*?</style>").replace_all(&t, " ");
    let t = rx(&QUEBRA, r"(?i)<br\s*/?>|</p>").replace_all(&t, "\n");
    let t = sem_entidades(&rx(&TAG, r"<[^>]+>").replace_all(&t, " "));
    let t = rx(&ESP, "[ \t\u{a0}]+").replace_all(&t, " ");
    rx(&LINHA, r" *\n *").replace_all(&t, "\n").into_owned()
}

/// Texto do e-mail: prefere text/plain; senao tira as tags do HTML.
pub fn texto(parte: &Value) -> String {
    let tipo = parte["mimeType"].as_str().unwrap_or("");
    if let Some(dados) = parte["body"]["data"].as_str().filter(|d| !d.is_empty() && (tipo == "text/plain" || tipo == "text/html")) {
        let bruto = String::from_utf8_lossy(&b64(dados)).into_owned();
        return if tipo == "text/html" { html_para_texto(&bruto) } else { bruto };
    }
    let partes = parte["parts"].as_array().map(|a| a.as_slice()).unwrap_or(&[]);
    let plano: Vec<&Value> = partes.iter().filter(|p| p["mimeType"] == "text/plain").collect();
    let ordem: Vec<&Value> = if plano.is_empty() { partes.iter().collect() } else { plano };
    ordem.into_iter().map(texto).find(|t| !t.trim().is_empty()).unwrap_or_default()
}

fn assunto(msg: &Value) -> String {
    Some(cabecalho(msg, "Subject")).filter(|a| !a.is_empty()).unwrap_or_else(|| "(sem assunto)".into())
}

/// Uma linha so com metadados: data, quem mandou e assunto.
pub fn resumo(msg: &Value, i: Option<usize>, fuso_min: i64) -> String {
    let pref = i.map(|i| format!("{i}) ")).unwrap_or_else(|| "• ".into());
    let nao_lida = if msg["labelIds"].as_array().is_some_and(|l| l.iter().any(|x| x == "UNREAD")) { "🔵 " } else { "" };
    format!("{pref}{nao_lida}{} · {} — {}", data_local(&cabecalho(msg, "Date"), fuso_min), corta(&remetente(msg), 40), corta(&assunto(msg), 90))
}

#[derive(Debug, PartialEq)]
pub struct Lista {
    pub texto: String,
    /// Ids na ordem mostrada: «le o e-mail 2» abre o segundo.
    pub ids: Vec<String>,
}

fn ids_da_busca(consulta: &str, n: usize, get: Get) -> Result<Vec<String>, String> {
    Ok(get("/messages", &[("q", consulta.to_string()), ("maxResults", n.to_string())])?["messages"].as_array().map(|a| a.iter().filter_map(|m| m["id"].as_str().map(String::from)).collect()).unwrap_or_default())
}

fn listar(consulta: &str, n: usize, titulo: &str, fuso_min: i64, get: Get) -> Result<Lista, String> {
    let ids = ids_da_busca(consulta, n, get)?;
    if ids.is_empty() {
        return Ok(Lista { texto: format!("{titulo}: nada encontrado."), ids });
    }
    let mut linhas = vec![];
    for (i, id) in ids.iter().enumerate() {
        // so metadados: o corpo nem e pedido
        let m = get(&format!("/messages/{id}"), &[("format", "metadata".into()), ("metadataHeaders", "From".into()), ("metadataHeaders", "Subject".into()), ("metadataHeaders", "Date".into())])?;
        linhas.push(resumo(&m, Some(i + 1), fuso_min));
    }
    Ok(Lista { texto: format!("{titulo} ({}):\n{}\n\nPara abrir um: «lê o e-mail 2».", ids.len(), linhas.join("\n")), ids })
}

pub fn nao_lidas(n: usize, fuso_min: i64, get: Get) -> Result<Lista, String> {
    listar("is:unread in:inbox", n, "E-mails não lidos", fuso_min, get)
}

/// Aceita a sintaxe do Gmail (from:, subject:, after:...) ou texto livre.
pub fn buscar(consulta: &str, n: usize, fuso_min: i64, get: Get) -> Result<Lista, String> {
    let consulta = consulta.trim();
    if consulta.is_empty() {
        return Err("o que devo procurar no e-mail?".into());
    }
    listar(consulta, n, &format!("E-mails sobre «{}»", corta(consulta, 50)), fuso_min, get)
}

/// `referencia` = numero da ultima lista (1, 2...) ou id da mensagem. O texto volta SO para o dono.
pub fn ler(referencia: &str, ultima: &[String], max_chars: usize, fuso_min: i64, get: Get) -> Result<String, String> {
    static VAZIO: OnceLock<Regex> = OnceLock::new();
    let r = referencia.trim();
    let id = match r.parse::<usize>() {
        Ok(n) if r.bytes().all(|b| b.is_ascii_digit()) => ultima.get(n.wrapping_sub(1)).ok_or("esse número não está na última lista; peça os e-mails de novo")?.as_str(),
        _ => r,
    };
    let m = get(&format!("/messages/{id}"), &[("format", "full".into())])?;
    let corpo = rx(&VAZIO, r"\n{3,}").replace_all(texto(&m["payload"]).trim(), "\n\n").into_owned();
    let anexos: Vec<&str> = m["payload"]["parts"].as_array().map(|a| a.iter().filter_map(|p| p["filename"].as_str().filter(|f| !f.is_empty())).collect()).unwrap_or_default();
    let cortado = corta(&corpo, max_chars);
    Ok(format!("De: {}\nAssunto: {}\nData: {}\n\n{}{}{}", remetente(&m), assunto(&m), data_local(&cabecalho(&m, "Date"), fuso_min), if cortado.is_empty() { "(sem texto)" } else { &cortado },
        if anexos.is_empty() { String::new() } else { format!("\n\n[anexos: {}]", anexos.join(", ")) }, if corpo.chars().count() > max_chars { "\n…(cortado)" } else { "" }))
}

// ---------- vagas de emprego (criterio do dono: SO remoto e salario >= R$ 5 mil/mes), filtro local sem IA ----------

pub fn consulta_vagas(dias: i64) -> String {
    format!("newer_than:{dias}d (vaga OR vagas OR emprego OR job OR jobs OR hiring OR oportunidade OR position OR remote OR remoto OR from:linkedin.com OR from:indeed.com OR from:gupy.io OR from:glassdoor.com OR from:catho.com.br OR from:infojobs.com.br OR from:remoteok.com OR from:weworkremotely.com OR from:wellfound.com OR from:vagas.com.br) -in:sent -in:spam")
}

fn milhar(v: f64) -> String {
    let d = format!("{:.0}", v);
    let mut s = String::new();
    for (i, ch) in d.chars().enumerate() {
        if i > 0 && (d.len() - i) % 3 == 0 {
            s.push('.');
        }
        s.push(ch);
    }
    s
}

fn numero(s: &str) -> Option<f64> {
    static MIL: OnceLock<Regex> = OnceLock::new();
    let s = s.trim();
    if rx(&MIL, r"^\d{1,3}(?:[.,]\d{3})+$").is_match(s) { s.replace(['.', ','], "").parse().ok() } else { s.replace(',', ".").parse().ok() }
}

/// Valores de salario achados no texto, convertidos (cambio aproximado, so para comparar) para R$/mes. Vazio = salario nao informado.
pub fn salarios_mensais_brl(t: &str) -> Vec<f64> {
    static V: OnceLock<Regex> = OnceLock::new();
    static HORA: OnceLock<Regex> = OnceLock::new();
    static ANO: OnceLock<Regex> = OnceLock::new();
    let r = rx(&V, r"(?i)(?P<moeda>R\$|US\$|USD|EUR|GBP|€|£|\$)\s?(?P<num>\d{1,3}(?:[.,]\d{3})+|\d+(?:[.,]\d{1,2})?)\s*(?P<k>k|mil)?(?:\s*(?:-|–|a|to|até)\s*(?:R\$|US\$|USD|EUR|€|£|\$)?\s?\d[\d.,]*\s*(?:k|mil)?)?(?P<per>\s*(?:/|por|per|an?)\s*(?:h|hr|hora|hour|m[êe]s|month|mo|ano|year|yr|annum))?");
    let mut out = vec![];
    for m in r.captures_iter(t) {
        let Some(mut v) = numero(&m["num"]) else { continue };
        if m.name("k").is_some() {
            v *= 1000.0;
        }
        let per = m.name("per").map(|p| p.as_str().to_lowercase()).unwrap_or_default();
        if rx(&HORA, r"\b(?:h|hr|hora|hour)\b").is_match(&per) {
            v *= 160.0;
        } else if rx(&ANO, "ano|year|yr|annum").is_match(&per) || (per.is_empty() && v >= 40000.0) {
            v /= 12.0; // valor grande sem periodo: provavelmente anual
        }
        v *= match m["moeda"].to_lowercase().as_str() {
            "usd" | "us$" | "$" => 5.0,
            "eur" | "€" => 5.5,
            "gbp" | "£" => 6.5,
            _ => 1.0,
        };
        if v >= 300.0 {
            out.push(v.round()); // menos que isso e preco pequeno (curso, assinatura)
        }
    }
    out
}

/// ("ok" | "sem_salario" | "fora", motivo). Remoto obrigatorio; salario >= 5 mil/mes (quando informado).
pub fn avaliar_vaga(t: &str) -> (&'static str, String) {
    static REMOTO: OnceLock<Regex> = OnceLock::new();
    static NAO: OnceLock<Regex> = OnceLock::new();
    static TOTAL: OnceLock<Regex> = OnceLock::new();
    if !rx(&REMOTO, r"(?i)\b(?:remot[oa]s?|remote|100\s*%\s*(?:remot\w*|home\s*office)|home[\s-]*office|trabalho\s+a\s+dist[âa]ncia|work\s+from\s+(?:home|anywhere)|anywhere|teletrabalho|wfh)\b").is_match(t) {
        return ("fora", "não diz que é remota".into());
    }
    if rx(&NAO, r"(?i)\b(?:presencial|on[\s-]?site|in[\s-]?office|h[íi]brid[oa]|hybrid)\b").is_match(t) && !rx(&TOTAL, r"(?i)100\s*%\s*remot|fully\s+remote|totalmente\s+remot").is_match(t) {
        return ("fora", "presencial/híbrida".into());
    }
    match salarios_mensais_brl(t).into_iter().reduce(f64::max) {
        None => ("sem_salario", "remota, salário não informado".into()),
        Some(m) if m >= SALARIO_MIN_BRL => ("ok", format!("remota, ~R$ {}/mês", milhar(m))),
        Some(m) => ("fora", format!("salário abaixo de 5 mil (~R$ {}/mês)", milhar(m))),
    }
}

/// E-mails de vaga dos ultimos dias filtrados pelo criterio do dono. O conteudo e lido e julgado aqui mesmo, sem IA.
pub fn vagas(dias: i64, n: usize, fuso_min: i64, get: Get) -> Result<Lista, String> {
    let ids = ids_da_busca(&consulta_vagas(dias), n, get)?;
    if ids.is_empty() {
        return Ok(Lista { texto: format!("Nenhum e-mail de vaga nos últimos {dias} dias."), ids });
    }
    let (mut boas, mut sem_sal, mut fora) = (vec![], vec![], 0);
    for id in &ids {
        let m = get(&format!("/messages/{id}"), &[("format", "full".into())])?;
        match avaliar_vaga(&format!("{}\n{}", cabecalho(&m, "Subject"), texto(&m["payload"]))) {
            ("ok", motivo) => boas.push((id.clone(), m, motivo)),
            ("sem_salario", motivo) => sem_sal.push((id.clone(), m, motivo)),
            _ => fora += 1,
        }
    }
    let mut linhas = vec![];
    if !boas.is_empty() {
        linhas.push("✅ Remotas com salário ≥ R$ 5 mil:".to_string());
        linhas.extend(boas.iter().enumerate().map(|(i, (_, m, motivo))| format!("{}  [{motivo}]", resumo(m, Some(i + 1), fuso_min))));
    }
    if !sem_sal.is_empty() {
        linhas.push(format!("{}🟡 Remotas sem salário no e-mail (vale conferir):", if boas.is_empty() { "" } else { "\n" }));
        linhas.extend(sem_sal.iter().enumerate().map(|(i, (_, m, _))| resumo(m, Some(boas.len() + i + 1), fuso_min)));
    }
    let alguma = !boas.is_empty() || !sem_sal.is_empty();
    if !alguma {
        linhas.push("Nenhuma vaga remota com salário ≥ R$ 5 mil.".into());
    }
    linhas.push(format!("\n({} e-mails de vaga analisados nos últimos {dias} dias; {fora} descartados: presencial/híbrida, sem ser remota ou salário baixo.)", ids.len()));
    if alguma {
        linhas.push("Para abrir um: «lê o e-mail 2».".into());
    }
    Ok(Lista { texto: format!("Vagas no e-mail (critério: só remoto, salário ≥ 5 mil)\n{}", linhas.join("\n")), ids: boas.into_iter().chain(sem_sal).map(|x| x.0).collect() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    fn b64u(t: &str) -> String {
        crate::base64::codificar(t.as_bytes()).replace('+', "-").replace('/', "_").trim_end_matches('=').to_string()
    }

    fn msg(id: &str, de: &str, assunto: &str, rotulos: &[&str], extra: Value) -> Value {
        let mut p = json!({"headers": [{"name": "From", "value": de}, {"name": "Subject", "value": assunto}, {"name": "Date", "value": "Mon, 05 Oct 2026 10:30:00 -0300"}]});
        for (k, v) in extra.as_object().into_iter().flatten() {
            p[k] = v.clone();
        }
        json!({"id": id, "labelIds": rotulos, "payload": p})
    }

    fn corpo(t: &str) -> Value {
        json!({"mimeType": "text/plain", "body": {"data": b64u(t)}})
    }

    struct Falso {
        lista: Vec<&'static str>,
        msgs: BTreeMap<&'static str, Value>,
        chamadas: RefCell<Vec<String>>,
    }

    impl Falso {
        fn get(&self, caminho: &str, ps: &[(&str, String)]) -> Result<Value, String> {
            self.chamadas.borrow_mut().push(format!("{caminho} {}", ps.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&")));
            if caminho == "/messages" {
                return Ok(json!({"messages": self.lista.iter().map(|i| json!({"id": i})).collect::<Vec<_>>()}));
            }
            self.msgs.get(caminho.rsplit('/').next().unwrap_or("")).cloned().ok_or_else(|| "o Gmail respondeu 404".to_string())
        }
    }

    fn falso(lista: &[&'static str], msgs: Vec<(&'static str, Value)>) -> Falso {
        Falso { lista: lista.to_vec(), msgs: msgs.into_iter().collect(), chamadas: RefCell::new(vec![]) }
    }

    #[test]
    fn login_e_so_leitura_com_pkce_e_state() {
        assert!(validar_cliente("abc", "curto").is_err() && validar_cliente("123-abc.apps.googleusercontent.com", "segredo-bem-longo").is_ok());
        assert!(url_login(" ", "d", "e").is_err());
        let u = url_login("123-abc.apps.googleusercontent.com", "DESAFIO", "ESTADO").unwrap();
        assert!(u.contains("gmail.readonly") && u.contains("code_challenge_method=S256") && u.contains("state=ESTADO") && u.contains("access_type=offline") && u.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A8765%2Femail%2Fcallback"));
        assert!(!u.contains("gmail.send") && !u.contains("gmail.modify") && !u.contains("mail.google.com"));
        assert!(retorno_confere("codigo", "e1", "e1").is_ok());
        for (c, r, g) in [("codigo", "estado-errado", "e1"), ("", "e1", "e1"), ("codigo", "", "")] {
            assert!(retorno_confere(c, r, g).is_err());
        }
        assert_eq!(erro_de_status(200, ""), None);
        assert!(erro_de_status(403, "Insufficient Permission").unwrap().contains("permissão de leitura") && erro_de_status(429, "").unwrap().contains("cota") && erro_de_status(500, "").unwrap().ends_with("500"));
        // o codigo so conhece o escopo de leitura e nenhuma rota de envio
        let fonte = include_str!("gmail.rs");
        let proibidos = [format!("gmail.{}", "compose"), format!("/messages/{}", "send"), format!("/{}", "trash"), format!("/{}", "modify")];
        assert!(proibidos.iter().all(|p| !fonte.contains(p.as_str())));
    }

    #[test]
    fn nao_lidas_mostra_remetente_assunto_e_numera_so_com_metadados() {
        let f = falso(&["a1", "a2"], vec![("a1", msg("a1", "=?UTF-8?B?Q29uY2VudHJpeA==?= <rh@concentrix.example>", "=?UTF-8?Q?Proposta_de_vaga?=", &["UNREAD"], json!({}))),
                                           ("a2", msg("a2", "Escola <no-reply@escola.example>", "Curso gratuito", &[], json!({})))]);
        let l = nao_lidas(8, -180, &|c, p| f.get(c, p)).unwrap();
        assert!(l.texto.starts_with("E-mails não lidos (2):\n1) 🔵 05/10 10:30 · Concentrix — Proposta de vaga\n2) 05/10 10:30 · Escola — Curso gratuito"), "{}", l.texto);
        assert_eq!(l.ids, vec!["a1", "a2"]);
        let ch = f.chamadas.borrow();
        assert_eq!(ch[0], "/messages q=is:unread in:inbox&maxResults=8");
        assert!(ch[1..].iter().all(|c| c.contains("format=metadata") && !c.contains("format=full")));
    }

    #[test]
    fn ler_pelo_numero_html_anexos_e_corte() {
        let html = "<html><style>x{}</style><body><p>Oi <b>Ana</b> &amp; cia</p><br>Segue o&nbsp;contrato.<script>alert(1)</script></body></html>";
        let f = falso(&["a1"], vec![("a1", msg("a1", "RH <rh@x.example>", "Vaga", &[], corpo("Olá, temos uma vaga."))),
            ("m9", msg("m9", "Fulano <f@x.example>", "Contrato", &[], json!({"mimeType": "multipart/mixed", "parts": [{"mimeType": "text/html", "body": {"data": b64u(html)}}, {"mimeType": "application/pdf", "filename": "contrato.pdf", "body": {}}]}))),
            ("m1", msg("m1", "a@x.example", "", &[], corpo(&"palavra ".repeat(1000))))]);
        let get = |c: &str, p: &[(&str, String)]| f.get(c, p);
        let ultima = nao_lidas(8, 0, &get).unwrap().ids;
        let t = ler("1", &ultima, 2500, -180, &get).unwrap();
        assert!(t.starts_with("De: RH\nAssunto: Vaga\nData: 05/10 10:30\n\nOlá, temos uma vaga."), "{t}");
        assert!(ler("7", &ultima, 2500, 0, &get).unwrap_err().contains("última lista") && ler("0", &ultima, 2500, 0, &get).is_err());
        let h = ler("m9", &[], 2500, 0, &get).unwrap();
        assert!(h.contains("Oi Ana & cia") && h.contains("Segue o contrato.") && !h.contains("<b>") && !h.contains("alert") && !h.contains("x{}") && h.ends_with("[anexos: contrato.pdf]"), "{h}");
        let longo = ler("m1", &[], 200, 0, &get).unwrap();
        assert!(longo.ends_with("…(cortado)") && longo.starts_with("De: a@x.example\nAssunto: (sem assunto)\nData: 05/10 13:30"));
        assert!(ler("nao-existe", &[], 100, 0, &get).is_err());
    }

    #[test]
    fn buscar_exige_consulta_sem_resultado_avisa_e_erro_da_rede_sobe() {
        let f = falso(&[], vec![]);
        assert!(buscar("  ", 8, 0, &|c, p| f.get(c, p)).is_err());
        assert!(buscar("from:ninguem", 8, 0, &|c, p| f.get(c, p)).unwrap().texto.contains("nada encontrado"));
        assert_eq!(nao_lidas(8, 0, &|_, _| Err(COMO_CONECTAR.into())).unwrap_err(), COMO_CONECTAR);
    }

    #[test]
    fn cabecalhos_datas_e_remetente() {
        assert_eq!(decodificar_cabecalho("=?ISO-8859-1?Q?Reuni=E3o_amanh=E3?= =?UTF-8?B?IMOgcyAxMGg=?="), "Reunião amanhã às 10h");
        assert_eq!(decodificar_cabecalho("Texto simples"), "Texto simples");
        assert_eq!((data_local("Mon, 05 Oct 2026 23:50:00 -0300", 60), data_local("31 Dec 2026 23:30 +0000", -180), data_local("ontem", 0)), ("06/10 03:50".to_string(), "31/12 20:30".to_string(), String::new()));
        assert_eq!(remetente(&msg("x", "\"Bia Souza\" <bia@x.example>", "", &[], json!({}))), "Bia Souza");
        assert_eq!(remetente(&json!({})), "?");
    }

    #[test]
    fn avaliar_vaga_criterio_do_dono() {
        for ok in ["Vaga 100% remota - Dev Python - Salário: R$ 7.500,00", "Remote Support Engineer - USD 2,000/month", "Fully remote, $60,000 per year", "Remote contractor $30/hour", "Home office - R$ 5k a 6k"] {
            assert_eq!(avaliar_vaga(ok).0, "ok", "{ok}");
        }
        for fora in ["Vaga remota de suporte, R$ 2.500", "Vaga presencial em Cuiabá, R$ 9.000", "Analista híbrido (remoto 2x por semana) R$ 8.000", "Vaga de TI na cidade, R$ 6.000"] {
            assert_eq!(avaliar_vaga(fora).0, "fora", "{fora}");
        }
        assert_eq!(avaliar_vaga("Vaga remota de analista de dados"), ("sem_salario", "remota, salário não informado".into()));
        assert_eq!(avaliar_vaga("Vaga 100% remota, híbrido opcional, R$ 12.000 por mês").1, "remota, ~R$ 12.000/mês");
        assert_eq!(salarios_mensais_brl("curso por R$ 49,90 e bolsa de R$ 1.200"), vec![1200.0]);
    }

    #[test]
    fn vagas_lista_so_as_boas_e_as_sem_salario() {
        let f = falso(&["a", "b", "c", "d"], vec![("a", msg("a", "Site A <jobs@a.example>", "Vaga remota: Dev Python", &[], corpo("100% remoto. Salário R$ 8.000"))),
            ("b", msg("b", "Site B <x@b.example>", "Vaga presencial", &[], corpo("Presencial em Cuiabá. R$ 9.000"))),
            ("c", msg("c", "Site C <x@c.example>", "Remote job", &[], corpo("Remote analyst. Apply now"))), ("d", msg("d", "Site D <x@d.example>", "Vaga remota", &[], corpo("Remoto, R$ 2.000")))]);
        let l = vagas(7, 25, -180, &|c, p| f.get(c, p)).unwrap();
        let r = &l.texto;
        assert!(r.contains("1) 05/10 10:30 · Site A — Vaga remota: Dev Python  [remota, ~R$ 8.000/mês]") && r.contains("2) 05/10 10:30 · Site C — Remote job") && !r.contains("Vaga presencial"), "{r}");
        assert!(r.find("Dev Python") < r.find("Remote job") && r.contains("(4 e-mails de vaga analisados nos últimos 7 dias; 2 descartados"));
        assert_eq!(l.ids, vec!["a", "c"]);
        assert!(f.chamadas.borrow()[0].contains("newer_than:7d") && f.chamadas.borrow()[0].contains("-in:sent"));
        let vazio = falso(&[], vec![]);
        assert_eq!(vagas(3, 25, 0, &|c, p| vazio.get(c, p)).unwrap().texto, "Nenhum e-mail de vaga nos últimos 3 dias.");
    }
}
