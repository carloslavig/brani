//! Regras PURAS do atendimento do Airbnb (porte de braniac/airbnb_vigia.py e das leituras puras de braniac/airbnb.py):
//! o que espera resposta, o que e pedido de reserva, o que uma resposta a hospede nunca pode conter, e o que vai para
//! uma pessoa decidir.
//! TRAVAS QUE NAO AFROUXAM:
//! - este codigo NUNCA aceita nem pre-aprova reserva: nao existe aqui funcao que aja sobre reserva. `regra_reserva`
//!   so devolve o MOTIVO da consulta (no Python existia o caminho "auto"; ele foi tirado de proposito);
//! - este codigo NUNCA altera anuncio: so interpreta texto que alguem leu;
//! - o atendimento automatico nasce DESLIGADO (`ligado` so e true com o modulo ativo E a chave gravada por `ligar`);
//! - resposta a hospede nunca leva link, numero longo, valor em reais, reembolso/multa/cancelamento, nem dado sensivel.
//! Ficou de fora: o `ciclo` (navegador + IA + envio), as ferramentas manuais e as threads. Quem le a caixa e quem envia
//! sao de fora; "anuncio protegido" (so atendimento) chega pronto em `Linha.protegido`, calculado por quem le a caixa.
//! Diferenca do Python: `validar_resposta` aqui nao abre excecao para nenhum link (la havia sites oficiais e proprios).
#![allow(dead_code)]
use crate::tempo::{Agora, Data};
use regex::Regex;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::OnceLock;

macro_rules! rx {
    ($p:expr) => {{
        static R: OnceLock<Regex> = OnceLock::new();
        R.get_or_init(|| Regex::new($p).expect("regex fixa do modulo"))
    }};
}

/// Segundos "locais" desde 1970 (data e hora do relogio da casa): serve para medir intervalo e separar por dia.
pub fn seg_local(a: &Agora) -> i64 {
    a.data.0 * 86400 + a.segundos()
}

/// Uma linha da caixa de entrada, ja interpretada por quem leu a pagina.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Linha {
    pub id: String,
    pub meta: String,
    pub anuncio: String,
    pub hospede: String,
    pub quando: String,
    /// "hospede" | "voce" | "outro"
    pub ultima_de: String,
    pub previa: String,
    pub suporte_airbnb: bool,
    /// anuncio em que so se atende hospede (nunca reserva, nunca alteracao)
    pub protegido: bool,
}

impl Linha {
    pub fn de_json(v: &Value) -> Linha {
        let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
        Linha { id: s("id"), meta: s("meta"), anuncio: s("anuncio"), hospede: s("hospede"), quando: s("quando"), ultima_de: s("ultima_de"), previa: s("previa"),
            suporte_airbnb: v["suporte_airbnb"].as_bool().unwrap_or(false), protegido: v["protegido"].as_bool().unwrap_or(false) }
    }
}

/// Uma mensagem da conversa: de = "hospede" | "voce" | "anfitriao" | "sistema".
#[derive(Clone, Debug, PartialEq)]
pub struct Msg {
    pub de: String,
    pub texto: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cfg {
    pub intervalo_s: i64,
    pub janela: (String, String),
    pub max_por_conversa_hora: usize,
    pub max_por_hora: usize,
    pub max_por_dia: usize,
    pub max_idade_h: f64,
    pub max_por_ciclo: usize,
    pub max_hospedes: i64,
    pub max_noites: i64,
    pub res_revisita_min: i64,
    pub consulta_reenvio_h: i64,
}

impl Default for Cfg {
    fn default() -> Self {
        Cfg { intervalo_s: 300, janela: ("07:00".into(), "22:30".into()), max_por_conversa_hora: 2, max_por_hora: 8, max_por_dia: 30, max_idade_h: 36.0,
            max_por_ciclo: 3, max_hospedes: 4, max_noites: 7, res_revisita_min: 90, consulta_reenvio_h: 5 }
    }
}

impl Cfg {
    /// Padrao + o que vier em {"airbnb_auto": {...}} da configuracao. Valor de tipo errado fica no padrao.
    pub fn de(cfg_router: &Value) -> Cfg {
        let (mut c, a) = (Cfg::default(), &cfg_router["airbnb_auto"]);
        let n = |k: &str, padrao: i64| a[k].as_i64().filter(|x| *x >= 0).unwrap_or(padrao);
        c.intervalo_s = n("intervalo_s", c.intervalo_s);
        c.max_por_conversa_hora = n("max_por_conversa_hora", c.max_por_conversa_hora as i64) as usize;
        c.max_por_hora = n("max_por_hora", c.max_por_hora as i64) as usize;
        c.max_por_dia = n("max_por_dia", c.max_por_dia as i64) as usize;
        c.max_idade_h = a["max_idade_h"].as_f64().filter(|x| *x >= 0.0).unwrap_or(c.max_idade_h);
        c.max_por_ciclo = n("max_por_ciclo", c.max_por_ciclo as i64) as usize;
        c.max_hospedes = n("max_hospedes", c.max_hospedes);
        c.max_noites = n("max_noites", c.max_noites);
        c.res_revisita_min = n("res_revisita_min", c.res_revisita_min);
        c.consulta_reenvio_h = n("consulta_reenvio_h", c.consulta_reenvio_h);
        if let (Some(i), Some(f)) = (a["janela"][0].as_str(), a["janela"][1].as_str()) {
            c.janela = (i.into(), f.into());
        }
        c
    }
}

// ---------- chave liga/desliga (nasce desligado) ----------

fn estado(dados: &Path) -> Value {
    std::fs::read_to_string(dados.join("airbnb_estado.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}))
}

fn modulo_ativo(dados: &Path) -> bool {
    crate::modulos::ativos(dados).iter().any(|m| m == "airbnb")
}

/// So true com o modulo "airbnb" ativo E a chave ligada por `ligar`. Sem arquivo, arquivo ilegivel ou qualquer duvida: desligado.
pub fn ligado(dados: &Path) -> bool {
    modulo_ativo(dados) && estado(dados)["ligado"] == true
}

fn gravar_chave(dados: &Path, v: bool) -> Result<(), String> {
    let mut e = estado(dados);
    e["ligado"] = json!(v);
    std::fs::create_dir_all(dados).and_then(|_| std::fs::write(dados.join("airbnb_estado.json"), e.to_string())).map_err(|x| format!("não gravei a chave do Airbnb: {x}"))
}

/// SO a interface local chama isto. Religar nao apaga o que ja foi tratado. Modulo desligado: recusa.
pub fn ligar(dados: &Path) -> Result<(), String> {
    if !modulo_ativo(dados) {
        return Err("o módulo airbnb está desligado".into());
    }
    gravar_chave(dados, true)
}

pub fn desligar(dados: &Path) -> Result<(), String> {
    gravar_chave(dados, false)
}

// ---------- horario, idade e limites ----------

fn hm(s: &str) -> Option<(u32, u32)> {
    let (h, m) = s.trim().split_once(':')?;
    Some((h.parse().ok()?, m.parse().ok()?))
}

/// Janela ("07:00", "22:30"): inicio incluso, fim excluso. Janela ilegivel = fora (nao responde).
pub fn dentro_da_janela(janela: &(String, String), agora: &Agora) -> bool {
    matches!((hm(&janela.0), hm(&janela.1)), (Some(i), Some(f)) if i <= (agora.hora, agora.min) && (agora.hora, agora.min) < f)
}

/// "22:50" (hoje) -> horas desde entao; "ontem" -> 30; outra coisa (dia da semana, data) -> None (antiga demais para responder de repente).
pub fn idade_horas(quando: &str, agora: &Agora) -> Option<f64> {
    let q = quando.trim().to_lowercase();
    if let Some(c) = rx!(r"^(\d{1,2}):(\d{2})$").captures(&q) {
        let (h, m) = (c[1].parse::<i64>().ok()?, c[2].parse::<i64>().ok()?);
        if h > 23 || m > 59 {
            return None;
        }
        let d = (agora.segundos() - (h * 3600 + m * 60)) as f64 / 3600.0;
        return Some(if d >= 0.0 { d } else { d + 24.0 });
    }
    (q == "ontem").then_some(30.0)
}

/// envios = (conversa, instante em `seg_local`). Cabe mais um envio a esta conversa? Olha a conversa na ultima hora, o total da hora e o total do dia.
pub fn limite_ok(envios: &[(String, i64)], conversa: &str, agora: &Agora, cfg: &Cfg) -> bool {
    let t = seg_local(agora);
    let hora: Vec<&(String, i64)> = envios.iter().filter(|x| t - x.1 < 3600).collect();
    let hoje = envios.iter().filter(|x| x.1.div_euclid(86400) == agora.data.0).count();
    hora.iter().filter(|x| x.0 == conversa).count() < cfg.max_por_conversa_hora && hora.len() < cfg.max_por_hora && hoje < cfg.max_por_dia
}

// ---------- o que espera resposta ----------

/// A conversa espera resposta NOSSA? (ultima mensagem e do hospede, nao e o suporte do Airbnb).
pub fn pendente_de_resposta(l: &Linha) -> bool {
    l.ultima_de == "hospede" && !l.suporte_airbnb
}

/// Mensagem so de agradecimento/confirmacao (sem pergunta nem pedido): nao precisa de resposta, senao vira laco infinito.
pub fn encerramento(texto: &str) -> bool {
    let t = texto.trim();
    !t.is_empty() && !t.contains('?') && t.chars().count() <= 60
        && rx!(r"(?i)^(?:(?:ok|okay|okk|certo|cerexemplo|beleza|blz|show|perfeito|entendi|entendido|combinado|fechado|valeu|vlw|obrigad[oa]|obg|tmj|tudo bem|tudo certo|t[aá] bom|t[aá] certo|t[aá] [oó]timo|[oó]timo|legal|massa|top|boa|de nada|por nada|imagina|disponha|sim|uhum|isso|j[aá] (?:vi|anotei|entendi)|anotado|recebido|bom dia|boa tarde|boa noite)[\s,.!👍🙏😊🙂😄❤️✅]*)+$").is_match(t)
}

/// Mensagens do HOSPEDE e cartoes do sistema depois da ultima resposta nossa -> (textos do hospede, cartoes).
pub fn finais_do_hospede(msgs: &[Msg]) -> (Vec<String>, Vec<String>) {
    let (mut textos, mut cartoes) = (Vec::new(), Vec::new());
    for m in msgs.iter().rev() {
        if m.de == "voce" || m.de == "anfitriao" {
            break;
        }
        if m.de == "sistema" { cartoes.push(m.texto.clone()) } else { textos.push(m.texto.clone()) }
    }
    textos.reverse();
    cartoes.reverse();
    (textos, cartoes)
}

pub fn textos_de_hospede(msgs: &[Msg]) -> Vec<String> {
    msgs.iter().filter(|m| m.de == "hospede").map(|m| m.texto.clone()).collect()
}

// ---------- reservas: so leitura e motivo da consulta ----------

fn norm(t: &str) -> String {
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn ini(t: &str, n: usize) -> String {
    t.chars().take(n).collect()
}

/// Ha cartao de consulta/pedido de reserva ainda sem desfecho?
pub fn reserva_pendente(cartoes: &[String]) -> bool {
    cartoes.iter().any(|c| rx!(r"(?i)Consulta de reserva|Pedido de reserva").is_match(c)
        && !rx!(r"(?i)recusad|aceit|expirad|cancelad|pr[eé]-aprov|confirmad").is_match(&ini(c.split("Você tem até").next().unwrap_or(""), 120)))
}

/// Olha o ULTIMO cartao de status -> Some("consulta") | Some("pedido") | None (nada pendente). So informa: ninguem aqui age sobre a reserva.
pub fn estado_reserva(cartoes: &[String]) -> Option<&'static str> {
    let status = rx!(r"^(Consulta de reserva|Pedido de reserva|Reserva pendente|Convite enviado|Pedido recusado|Pedido aceito|Pedido cancelado|Confirmado|Alteração solicitada|Reserva cancelada)");
    let ultimo = cartoes.iter().map(|c| norm(c)).filter(|t| status.is_match(t)).last()?;
    if ultimo.contains("%{") || !ultimo.contains("Você tem até") {
        return None;
    }
    if ultimo.starts_with("Consulta de reserva") {
        Some("consulta")
    } else if ultimo.starts_with("Pedido de reserva") || ultimo.starts_with("Reserva pendente") {
        Some("pedido")
    } else {
        None
    }
}

pub fn ultimo_cartao(cartoes: &[String]) -> String {
    cartoes.iter().rev().find(|c| rx!(r"^(Consulta de reserva|Pedido de reserva|Reserva pendente)").is_match(c)).cloned().unwrap_or_default()
}

pub fn sig_cartao(cartao: &str) -> String {
    ini(&norm(cartao), 120)
}

fn mes(m: &str) -> Option<i64> {
    ["jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez"].iter().position(|x| *x == m).map(|i| i as i64 + 1)
}

/// "8 – 9 de out." -> 1; "31 de dez. de 2026 – 3 de jan. de 2027" -> 3; nao entendeu ou nao e positivo -> None.
pub fn parse_noites(meta: &str, ano: i64) -> Option<i64> {
    let t = meta.to_lowercase();
    let c = rx!(r"(\d{1,2})(?:\s+de\s+([a-zç]{3})\.?)?(?:\s+de\s+(\d{4}))?\s*[–-]\s*(\d{1,2})\s+de\s+([a-zç]{3})\.?(?:\s+de\s+(\d{4}))?").captures(&t)?;
    let num = |i: usize| c.get(i).and_then(|x| x.as_str().parse::<i64>().ok());
    let m2 = mes(&c[5])?;
    let m1 = match c.get(2) { Some(x) => mes(x.as_str())?, None => m2 };
    let a2 = num(6);
    let fim = Data::civil(a2.unwrap_or(ano), m2, num(4)?)?;
    let comeco = Data::civil(num(3).unwrap_or(a2.unwrap_or(ano) - (m1 > m2) as i64), m1, num(1)?)?;
    Some(fim.0 - comeco.0).filter(|n| *n > 0)
}

pub fn parse_hospedes(texto: &str) -> Option<i64> {
    rx!(r"(\d+)\s+h[oó]spedes?").captures(texto)?[1].parse().ok()
}

/// "Você tem até 8 de out. 21:46 AMT para responder" -> true se o prazo ja passou (o cartao fica na conversa depois de expirar).
pub fn prazo_vencido(cartao: &str, agora: &Agora) -> bool {
    let Some(c) = rx!(r"(?i)tem até (\d{1,2}) de ([a-zç]{3})\.?(?:\s+(\d{1,2}):(\d{2}))?").captures(cartao) else { return false };
    let num = |i: usize, padrao: i64| c.get(i).and_then(|x| x.as_str().parse::<i64>().ok()).unwrap_or(padrao);
    let Some(fim) = mes(&c[2].to_lowercase()).and_then(|m| Data::civil(agora.data.partes().0, m, num(1, 0))) else { return false };
    let (h, m) = (num(3, 23), num(4, 59));
    h <= 23 && m <= 59 && fim.0 * 86400 + h * 3600 + m * 60 < seg_local(agora)
}

pub fn eh_quarto(anuncio: &str) -> bool {
    rx!(r"(?i)^\s*(quarto|su[ií]te)\b").is_match(anuncio)
}

/// Palavra de risco dita pelo hospede (festa, evento, som, pet, visitas...).
pub fn risco(textos: &[String]) -> Option<String> {
    rx!(r"(?i)\b(festa|evento|anivers[aá]rio|comemora\w*|churrasco|som\s+alto|caixa\s+de\s+som|pet|cachorro|cadela|gato|animais|animal|visitas?|casamento|confraterniza\w*|filmagem|ensaio|DJ|open\s+bar)\b")
        .find(&textos.join(" ")).map(|m| m.as_str().to_string())
}

/// Motivo da consulta a quem decide. TODA reserva vai para uma pessoa: nao existe resposta "pode aceitar sozinho".
/// O motivo so diz o que chamou atencao (anuncio protegido, casa inteira, muita gente, muitas noites, risco), ou a regra geral.
pub fn regra_reserva(l: &Linha, cartao: &str, textos_hospede: &[String], cfg: &Cfg, ano: i64) -> String {
    if l.protegido {
        return "é um anúncio protegido (lá eu só respondo hóspede)".into();
    }
    if !(eh_quarto(&l.anuncio) || rx!(r"(?i)^\s*studio\b").is_match(&l.anuncio)) {
        return "não é quarto/studio (casa inteira ou outro anúncio)".into();
    }
    let ou = |n: Option<i64>| n.map_or("?".to_string(), |x| x.to_string());
    let hosp = parse_hospedes(cartao);
    if hosp.map_or(true, |h| h > cfg.max_hospedes) {
        return format!("são {} hóspedes (acima de {} eu chamo atenção)", ou(hosp), cfg.max_hospedes);
    }
    let noites = parse_noites(&l.meta, ano).or_else(|| parse_noites(cartao.splitn(2, "hóspedes").last().unwrap_or(""), ano));
    if noites.map_or(true, |n| n > cfg.max_noites) {
        return format!("são {} noites (acima de {} eu chamo atenção)", ou(noites), cfg.max_noites);
    }
    if let Some(r) = risco(textos_hospede) {
        return format!("o hóspede falou de '{r}'");
    }
    "toda reserva precisa do OK de vocês".into()
}

/// Texto da consulta a uma pessoa. `vocativo` = como chamar quem recebe (vem da configuracao); None = formato seco.
/// Diferenca do Python: la o texto perguntava "Posso aceitar?"; aqui diz que quem aceita e a pessoa, la no Airbnb.
pub fn pergunta_familia(l: &Linha, cartao: &str, motivo: &str, estado: &str, vocativo: Option<&str>) -> String {
    let acao = if estado == "consulta" { "pré-aprovar a consulta" } else { "aceitar o pedido" };
    let valor = rx!(r"R\$\s*[\d.,]+").find(cartao).map(|m| format!(", {}", m.as_str())).unwrap_or_default();
    let primeiro = l.hospede.split_whitespace().next().unwrap_or("");
    let abre = vocativo.map_or("Airbnb | reserva: ".to_string(), |v| format!("{v}, reserva no Airbnb: "));
    format!("{abre}{} ({}, {}, {} hóspedes{valor}). Atenção: {motivo}. Eu não aceito nem pré-aprovo reserva: quem decide {acao} é você, lá no Airbnb. \
Responde 'sim {primeiro}' ou 'não {primeiro}' para eu anotar. Quem responder primeiro vale.",
        l.hospede, ini(&l.anuncio, 34), ini(&l.meta, 30), parse_hospedes(cartao).map_or("?".to_string(), |x| x.to_string()))
}

/// mensagens = (quem, texto) enviadas depois da consulta, em ordem -> Some((true = sim / false = nao, quem)).
/// "nao" vence "sim". Com varias consultas abertas exige o primeiro nome do hospede. Texto longo nao e decisao.
pub fn ler_decisao(nome_hospede: &str, varias_abertas: bool, mensagens: &[(String, String)]) -> Option<(bool, String)> {
    let primeiro = nome_hospede.split_whitespace().next().unwrap_or("").to_lowercase();
    let mut achou = None;
    for (quem, txt) in mensagens {
        let t = txt.trim();
        if t.chars().count() > 80 || (varias_abertas && !t.to_lowercase().contains(&primeiro)) {
            continue;
        }
        if rx!(r"(?i)^\W*(n[aã]o|recusa|recuso|rejeita|negativo)\b").is_match(t) {
            return Some((false, quem.clone()));
        }
        if achou.is_none() && rx!(r"(?i)^\W*(sim|pode|aceita|aceito|aprova|aprovo|pr[eé]-?aprova|autorizo|manda ver|bora|isso)\b").is_match(t) {
            achou = Some((true, quem.clone()));
        }
    }
    achou
}

/// O que dizer ao dono depois da decisao lida. Nos dois casos quem age no Airbnb e a pessoa.
pub fn aviso_decisao(hospede: &str, sim: bool, por: &str, estado: &str) -> String {
    if sim {
        format!("Airbnb | {hospede}: {por} disse SIM. Eu não {} sozinho: confirma lá no Airbnb (o prazo é de 24 h).", if estado == "consulta" { "pré-aprovo" } else { "aceito" })
    } else {
        format!("Airbnb | {hospede}: {por} mandou RECUSAR. Eu não recuso sozinho: recusa lá no Airbnb (o prazo é de 24 h).")
    }
}

/// A resposta rapida "firstcontact" (quarto privativo, areas compartilhadas) vai antes? So em quarto, nunca em anuncio protegido,
/// so se ainda nao ha mensagem nossa na conversa e so uma vez (`ja_enviada`).
pub fn precisa_firstcontact(l: &Linha, msgs: &[Msg], tem_reserva: bool, ja_enviada: bool) -> bool {
    let sem_nossa = !msgs.iter().any(|m| m.de == "voce" || m.de == "anfitriao");
    (!finais_do_hospede(msgs).0.is_empty() || tem_reserva) && eh_quarto(&l.anuncio) && !l.protegido && sem_nossa && !ja_enviada && msgs.len() <= 24
}

// ---------- o que uma resposta a hospede nunca leva ----------

/// Trava geral de resposta a terceiros (a mesma do WhatsApp): vazia, longa, link/numero longo, "o dono esta ocupado", dado sensivel.
pub fn validar_resposta(texto: &str) -> Result<(), String> {
    if texto.trim().is_empty() {
        return Err("vazia".into());
    }
    if texto.chars().count() > 500 {
        return Err("longa demais".into());
    }
    if rx!(r"(?i)https?://|www\.|\.com\b|\.br\b|\d{6,}").is_match(texto) {
        return Err("contem link/numero".into());
    }
    if rx!(r"(?i)ocupad[oa]|n[aã]o\s+est[aá]\s+dispon[ií]vel|indispon[ií]vel|(?:est[aá]|t[aá])\s+off|sem\s+tempo|n[aã]o\s+pode\s+atender").is_match(texto) {
        return Err("diz que o dono esta ocupado/indisponivel (nao pode; explique quem e o assistente)".into());
    }
    let dado = rx!(r"(?i)\b\d{4}[ .-]?\d{4}[ .-]?\d{4}[ .-]?\d{4}\b|\b\d{3}\.?\d{3}\.?\d{3}-?\d{2}\b|\b(?:senha|cvv|token|c[oó]digo\s+de\s+(?:verifica\w+|seguran\w+))\b\s*(?:[ée]|:|=)\s*\S{3,}|ag[eê]ncia\s*\d|conta\s*(?:corrente\s*)?\d");
    let pede = rx!(r"(?i)\b(?:me\s+)?(?:mande|manda|envie|envia|passe|passa|informe|informa|digite|digita)\b[^.!?\n]{0,30}\b(?:cart[aã]o|senha|cvv|cpf|c[oó]digo|token|dados\s+banc\w+)\b");
    if dado.is_match(texto) || pede.is_match(texto) {
        return Err("contem dado sensivel (cartao/CPF/senha) ou pede esses dados".into());
    }
    Ok(())
}

/// Trava do Airbnb por cima da geral: sem link, sem reembolso/multa/cancelamento e sem valor em reais (dinheiro e so com o dono).
pub fn validar_airbnb(texto: &str) -> Result<(), String> {
    validar_resposta(texto)?;
    if rx!(r"(?i)https?://|www\.|github").is_match(texto) {
        return Err("link (hospede do Airbnb nao recebe link por aqui)".into());
    }
    if rx!(r"(?i)reembols|estorn|multa|devolu[cç][aã]o do (valor|dinheiro)|cancel(ar|amos|ei)").is_match(texto) {
        return Err("fala de reembolso/multa/cancelamento (dinheiro: so o dono)".into());
    }
    if rx!(r"(?i)R\$\s*\d|\d+\s*reais|\d+[.,]\d{2}\s*(?:por|a)\s+noite").is_match(texto) {
        return Err("fala de valor em reais (decisao de dinheiro: so o dono)".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ag(dia: i64, h: u32, m: u32) -> Agora { Agora { data: Data::civil(2030, 10, dia).unwrap(), hora: h, min: m, seg: 0 } }
    fn linha() -> Linha {
        Linha { id: "10".into(), meta: "8 – 9 de out.".into(), anuncio: "Quarto Azul · casa com piscina".into(), hospede: "Ana Souza".into(), quando: "11:00".into(),
            ultima_de: "hospede".into(), previa: "Tem vaga sábado?".into(), suporte_airbnb: false, protegido: false }
    }
    fn msg(de: &str, t: &str) -> Msg { Msg { de: de.into(), texto: t.into() } }
    fn s(v: &[&str]) -> Vec<String> { v.iter().map(|x| x.to_string()).collect() }
    const CARTAO: &str = "Consulta de reserva R$ 166,03 · 3 hóspedes 8 – 9 de out. de 2030 · Quarto Azul Você tem até 7 de out. 21:46 AMT para responder Responder";

    #[test]
    fn desligado_por_padrao_e_so_liga_com_o_modulo_ativo() {
        let p = std::env::temp_dir().join(format!("braniac-airbnb_vigia-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        assert!(!ligado(&p));
        std::fs::write(p.join("airbnb_estado.json"), "lixo").unwrap();
        assert!(!ligado(&p)); // arquivo ilegivel = desligado
        std::fs::write(p.join("airbnb_estado.json"), r#"{"ligado": "sim", "tratados": {"1": "x"}}"#).unwrap();
        assert!(!ligado(&p)); // so o booleano true liga
        ligar(&p).unwrap();
        assert!(ligado(&p) && estado(&p)["tratados"]["1"] == "x"); // religar nao apaga o que ja foi tratado
        desligar(&p).unwrap();
        assert!(!ligado(&p));
        std::fs::write(p.join("modulos.json"), r#"{"ativos": []}"#).unwrap();
        assert!(ligar(&p).is_err() && !ligado(&p)); // modulo desligado: nem liga
        std::fs::write(p.join("airbnb_estado.json"), r#"{"ligado": true}"#).unwrap();
        assert!(!ligado(&p));
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn janela_idade_e_limites() {
        let j = Cfg::default().janela;
        assert!(dentro_da_janela(&j, &ag(7, 12, 0)) && dentro_da_janela(&j, &ag(7, 7, 0)) && !dentro_da_janela(&j, &ag(7, 23, 0)) && !dentro_da_janela(&j, &ag(7, 22, 30)) && !dentro_da_janela(&j, &ag(7, 6, 59)));
        assert!(!dentro_da_janela(&("sete".into(), "22:30".into()), &ag(7, 12, 0)));
        let agora = ag(7, 12, 0);
        assert_eq!(idade_horas("11:00", &agora), Some(1.0));
        assert_eq!(idade_horas("13:00", &agora), Some(23.0)); // hora "no futuro" e de ontem
        assert!(idade_horas("ontem", &agora).unwrap() > 24.0);
        assert_eq!((idade_horas("seg", &agora), idade_horas("25:00", &agora), idade_horas("", &agora)), (None, None, None));
        let cfg = Cfg::default();
        let t = seg_local(&agora);
        let envios = vec![("10".to_string(), t - 60), ("10".to_string(), t - 120)];
        assert!(!limite_ok(&envios, "10", &agora, &cfg) && limite_ok(&envios, "99", &agora, &cfg));
        assert!(limite_ok(&[("10".into(), t - 3700), ("10".into(), t - 60)], "10", &agora, &cfg)); // o de mais de 1 h nao conta
        let hora: Vec<(String, i64)> = (0..8).map(|i| (i.to_string(), t - 10)).collect();
        assert!(!limite_ok(&hora, "99", &agora, &cfg)); // total por hora
        let dia: Vec<(String, i64)> = (0..30).map(|i| (i.to_string(), t - 4 * 3600)).collect();
        assert!(!limite_ok(&dia, "99", &agora, &cfg)); // total por dia
        let ontem: Vec<(String, i64)> = (0..30).map(|i| (i.to_string(), t - 13 * 3600)).collect();
        assert!(limite_ok(&ontem, "99", &agora, &cfg));
        let c = Cfg::de(&json!({"airbnb_auto": {"max_por_dia": 5, "janela": ["08:00", "20:00"], "max_noites": "muitas"}}));
        assert_eq!((c.max_por_dia, c.janela.0.as_str(), c.max_noites, c.max_por_hora), (5, "08:00", 7, 8));
    }

    #[test]
    fn resposta_com_link_valor_reembolso_ou_dado_e_barrada() {
        for ruim in ["A diária é R$ 250 por noite", "fica 250 reais", "sai 199,90 por noite", "Diária boa, veja https://x.example", "veja em www.exemplo", "Já solicitei o reembolso pelo Airbnb.",
            "Vamos cancelar sem multa.", "me liga no 11987654321", "", "   ", "Ele está ocupado agora", "me manda o número do cartão", "a senha é abc123", "olha no site exemplo.com"] {
            assert!(validar_airbnb(ruim).is_err(), "{ruim}");
        }
        assert!(validar_airbnb(&"a".repeat(501)).is_err());
        assert!(validar_airbnb("Temos vaga sim, qual o período?").is_ok() && validar_airbnb("O check-in é a partir das 14h, tudo certo!").is_ok());
        assert_eq!(validar_airbnb("fica 250 reais").unwrap_err(), "fala de valor em reais (decisao de dinheiro: so o dono)");
    }

    #[test]
    fn o_que_espera_resposta() {
        assert!(pendente_de_resposta(&linha()));
        assert!(!pendente_de_resposta(&Linha { ultima_de: "voce".into(), ..linha() }) && !pendente_de_resposta(&Linha { suporte_airbnb: true, ..linha() }));
        for t in ["Ok, obrigado!", "valeu 👍", "Bom dia", "tá bom, combinado"] {
            assert!(encerramento(t), "{t}");
        }
        for t in ["ok, e o wifi?", "obrigado, mas preciso da chave", "", "Tem vaga sábado?"] {
            assert!(!encerramento(t), "{t}");
        }
        let msgs = [msg("hospede", "oi"), msg("voce", "Bem vindo(a)!"), msg("hospede", "Tem vaga sábado?"), msg("sistema", CARTAO), msg("hospede", "e pet?")];
        assert_eq!(finais_do_hospede(&msgs), (s(&["Tem vaga sábado?", "e pet?"]), s(&[CARTAO])));
        assert_eq!(textos_de_hospede(&msgs).len(), 3);
        assert_eq!(Linha::de_json(&json!({"id": "7", "hospede": "Bia", "ultima_de": "hospede", "protegido": true})).protegido, true);
    }

    #[test]
    fn le_o_estado_da_reserva_sem_agir() {
        assert_eq!(estado_reserva(&s(&[CARTAO])), Some("consulta"));
        assert_eq!(estado_reserva(&s(&["Reserva pendente R$440.01 para 3 hóspedes 7 – 11 de out. Você tem até 8 de out. para responder"])), Some("pedido"));
        assert_eq!(estado_reserva(&s(&[CARTAO, "Pedido recusado"])), None);
        assert_eq!(estado_reserva(&s(&["Consulta de reserva sem prazo"])), None);
        assert_eq!(estado_reserva(&s(&["Consulta de reserva %{x} Você tem até"])), None);
        assert!(reserva_pendente(&s(&[CARTAO])) && !reserva_pendente(&s(&["Pedido de reserva aceito"])) && !reserva_pendente(&[]));
        assert_eq!(ultimo_cartao(&s(&["Aviso", CARTAO, "Convite enviado"])), CARTAO);
        assert_eq!(sig_cartao("Consulta   de\nreserva"), "Consulta de reserva");
        assert_eq!((parse_noites("8 – 9 de out.", 2030), parse_noites("31 de dez. de 2030 – 3 de jan. de 2031", 2030), parse_noites("28 de dez. – 2 de jan.", 2031)), (Some(1), Some(3), Some(5)));
        assert_eq!((parse_noites("Check-in hoje", 2030), parse_noites("9 – 8 de out.", 2030), parse_noites("30 – 31 de fev.", 2030), parse_noites("1 – 20 de out.", 2030)), (None, None, None, Some(19)));
        assert_eq!((parse_hospedes(CARTAO), parse_hospedes("1 hóspede"), parse_hospedes("sem número")), (Some(3), Some(1), None));
        assert!(prazo_vencido(&CARTAO.replace("7 de out. 21:46", "1 de jan. 10:00"), &ag(7, 0, 0)));
        assert!(!prazo_vencido(CARTAO, &ag(7, 12, 0)) && prazo_vencido(CARTAO, &ag(7, 21, 47)) && !prazo_vencido("sem prazo", &ag(7, 12, 0)) && !prazo_vencido("tem até 31 de fev.", &ag(7, 12, 0)));
        assert!(!prazo_vencido("Você tem até 7 de out. para responder", &ag(7, 23, 0)) && prazo_vencido("Você tem até 7 de out. para responder", &ag(8, 0, 0)));
    }

    #[test]
    fn toda_reserva_vai_para_uma_pessoa_decidir() {
        let cfg = Cfg::default();
        // dentro do padrao (quarto, 3 hospedes, 1 noite, sem risco): AINDA ASSIM e consulta, nunca aceite
        assert_eq!(regra_reserva(&linha(), CARTAO, &s(&["Tem vaga sábado?"]), &cfg, 2030), "toda reserva precisa do OK de vocês");
        let casos: [(Linha, String, &str); 5] = [
            (Linha { anuncio: "Casa do Lago · 3 quartos".into(), ..linha() }, CARTAO.into(), "quarto/studio"),
            (Linha { protegido: true, ..linha() }, CARTAO.into(), "protegido"),
            (linha(), CARTAO.replace("3 hóspedes", "6 hóspedes"), "6 hóspedes"),
            (Linha { meta: "1 – 20 de out.".into(), ..linha() }, CARTAO.into(), "19 noites"),
            (Linha { meta: "Check-in hoje".into(), ..linha() }, CARTAO.replace("8 – 9 de out. de 2030", ""), "? noites"),
        ];
        for (l, cartao, motivo) in casos {
            assert!(regra_reserva(&l, &cartao, &[], &cfg, 2030).contains(motivo), "{motivo}");
        }
        assert!(regra_reserva(&linha(), "Consulta de reserva sem contagem", &[], &cfg, 2030).contains("? hóspedes"));
        assert_eq!(regra_reserva(&linha(), CARTAO, &s(&["Vamos fazer uma festa de aniversário?"]), &cfg, 2030), "o hóspede falou de 'festa'");
        assert!(regra_reserva(&Linha { anuncio: "Studio Verde · 500m".into(), ..linha() }, CARTAO, &[], &cfg, 2030).starts_with("toda reserva"));
        assert_eq!((risco(&s(&["posso levar meu cachorro?"])), risco(&s(&["petisco e gatorade"]))), (Some("cachorro".into()), None));
        let q = pergunta_familia(&linha(), CARTAO, "o hóspede falou de 'festa'", "consulta", Some("Oi Bia"));
        assert!(q.starts_with("Oi Bia, reserva no Airbnb: Ana Souza (Quarto Azul") && q.contains("3 hóspedes, R$ 166,03") && q.contains("festa")
            && q.contains("Eu não aceito nem pré-aprovo reserva") && q.contains("'sim Ana' ou 'não Ana'") && !q.contains("Posso"), "{q}");
        assert!(pergunta_familia(&linha(), "Pedido de reserva", "x", "pedido", None).starts_with("Airbnb | reserva: Ana Souza") );
        assert!(aviso_decisao("Ana", true, "Bia", "pedido").contains("Eu não aceito sozinho") && aviso_decisao("Ana", false, "Bia", "pedido").contains("RECUSAR"));
    }

    #[test]
    fn ler_decisao_nao_vence_sim_e_exige_nome_com_varias_abertas() {
        let m = |v: &[(&str, &str)]| v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<Vec<_>>();
        assert_eq!(ler_decisao("Ana Souza", false, &m(&[("Bia", "sim")])), Some((true, "Bia".into())));
        assert_eq!(ler_decisao("Ana Souza", false, &m(&[("Bia", "sim"), ("Caio", "não")])), Some((false, "Caio".into())));
        assert_eq!(ler_decisao("Ana Souza", true, &m(&[("Bia", "sim")])), None); // varias abertas: precisa do nome
        assert_eq!(ler_decisao("Ana Souza", true, &m(&[("Bia", "sim ana")])), Some((true, "Bia".into())));
        assert_eq!(ler_decisao("Ana", false, &m(&[("Bia", "bom dia, tudo bem?")])), None);
        assert_eq!(ler_decisao("Ana", false, &m(&[("Bia", &format!("sim {}", "x".repeat(100)))])), None);
        assert_eq!(ler_decisao("Ana", false, &m(&[("Bia", "simples assim"), ("Caio", "nada disso")])), None); // palavra inteira
    }

    #[test]
    fn firstcontact_so_em_quarto_sem_mensagem_nossa_e_uma_vez() {
        let primeira = [msg("hospede", "Tem vaga sábado?")];
        assert!(precisa_firstcontact(&linha(), &primeira, false, false));
        assert!(!precisa_firstcontact(&linha(), &primeira, false, true));
        assert!(!precisa_firstcontact(&linha(), &[msg("voce", "Bem vindo(a)!"), msg("hospede", "Tem vaga?")], false, false));
        for anuncio in ["Casa do Lago · 3 quartos", "Studio Verde · 500m", "Apartamento completo - centro"] {
            assert!(!precisa_firstcontact(&Linha { anuncio: anuncio.into(), ..linha() }, &primeira, true, false), "{anuncio}");
        }
        assert!(!precisa_firstcontact(&Linha { protegido: true, ..linha() }, &primeira, false, false));
        assert!(!precisa_firstcontact(&linha(), &[], false, false) && precisa_firstcontact(&linha(), &[], true, false));
    }
}
