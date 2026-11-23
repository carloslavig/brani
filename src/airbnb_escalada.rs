//! Escalada do Airbnb (porte de braniac/airbnb_escalada.py). Funciona com o atendimento automatico desligado: so recebe
//! as linhas da caixa que alguem leu e decide QUEM avisar e QUANDO. Nunca responde hospede, nunca aceita reserva.
//! - mensagem de hospede sem resposta ha 30 min -> avisa a primeira pessoa e o dono juntos; UM lembrete 30 min depois e para;
//! - pedido/consulta de reserva -> os dois na hora, com a sintese; 15 min depois a terceira pessoa; depois repete os dois
//!   a cada 15 min; passada 1 h da primeira pendencia, para de avisar;
//! - numero longo que o hospede digitou (cartao, CPF, conta) vira "[número]" antes de ir a qualquer pessoa;
//! - "ja respondi" silencia ate chegar mensagem NOVA daquele hospede.
//! A leitura da caixa e o envio entram de fora (`enviar` e closure). Quem e avisado vem de dados/pessoal.json:
//! {"airbnb_escalada": {"primeira": {"contato", "vocativo", "citado"}, "dono": {...}, "terceira": {...}}}
//! (contato = nome no WhatsApp; vocativo = como abrir a frase; citado = como a pessoa e citada a outra, com artigo).
//! TETO por hora (dados/airbnb_config.json {"avisos_por_hora": 2}; 0 = sem teto), POR DESTINATARIO, janela deslizante de 60 min (estado em
//! dados/airbnb_teto.json). Estourou: o aviso fica na fila e, quando abre vaga, sai UM aviso juntando o que esperava (reservas primeiro);
//! quem ja foi respondido sai da fila.
//! Diferencas do Python: os nomes e tratamentos sairam do codigo; o estado (dados/airbnb_escalada.json, mesmo formato)
//! guarda segundos do relogio local em vez de time.time(); a thread que le a caixa a cada 60 s nao foi portada.
#![allow(dead_code)]
use crate::airbnb_vigia::{encerramento, idade_horas, pendente_de_resposta, seg_local, Linha};
use crate::tempo::Agora;
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;
use std::sync::OnceLock;

const INTERVALO_S: i64 = 15 * 60; // no maximo 1 aviso a cada 15 min por hospede
const ESPERA_MENSAGEM_S: i64 = 1800;
const LEMBRETE_MENSAGEM_S: i64 = 1800;
const JANELA_S: i64 = 3600; // passada 1 h a escalada de reserva para (aviso sem fim na madrugada = risco de bloqueio do numero)

macro_rules! rx {
    ($p:expr) => {{
        static R: OnceLock<Regex> = OnceLock::new();
        R.get_or_init(|| Regex::new($p).expect("regex fixa do modulo"))
    }};
}

fn reserva_pendente() -> &'static Regex {
    rx!(r"(?i)Pedido de reserva|Consulta de reserva|Reserva pendente")
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pessoa {
    pub contato: String,
    pub vocativo: String,
    pub citado: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Destinos {
    pub primeira: Option<Pessoa>,
    pub dono: Option<Pessoa>,
    pub terceira: Option<Pessoa>,
}

impl Destinos {
    pub fn de_json(v: &Value) -> Destinos {
        let p = |k: &str| {
            let s = |c: &str| v[k][c].as_str().unwrap_or("").trim().to_string();
            (!s("contato").is_empty()).then(|| Pessoa { contato: s("contato"), vocativo: s("vocativo"), citado: s("citado") })
        };
        Destinos { primeira: p("primeira"), dono: p("dono"), terceira: p("terceira") }
    }

    /// Sem configuracao ninguem e avisado (nenhum nome fica no codigo).
    pub fn ler(dados: &Path) -> Destinos {
        let v: Value = std::fs::read_to_string(dados.join("pessoal.json")).ok().and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok()).unwrap_or(Value::Null);
        Destinos::de_json(&v["airbnb_escalada"])
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pendencia {
    /// "reserva" | "mensagem"
    pub tipo: String,
    pub consulta: bool,
    pub hospede: String,
    pub anuncio: String,
    pub datas: String,
    pub previa: String,
    /// assinatura da mensagem que esta na caixa (previa|quando); vazia = sem assinatura
    pub sig: String,
}

/// Aviso automatico do Airbnb ou reserva ja decidida (expirou, cancelou, aceita, pre-aprovacao enviada) NAO espera resposta.
/// Texto livre do hospede ("aceita pet?", "quero cancelar") nao cai aqui: so frases de sistema e o status da conversa.
pub fn ja_resolvido(meta: &str, previa: &str) -> bool {
    rx!(r"(?i)pr[eé]-?aprova[cç][aã]o (?:expirou|enviada|foi enviada)|(?:pedido|convite|reserva|solicita[cç][aã]o)[^.]{0,30}(?:expirou|foi (?:cancelad|aceit|recusad|confirmad|retirad|removid))|reserva (?:confirmada|aceita)|mensagem foi cancelada|uma mensagem foi").is_match(previa)
        || rx!(r"(?i)^\s*(?:cancelad|expirad|recusad|negad|confirmad|conclu[ií]d|declinad)").is_match(meta)
}

/// (id, pendencia) a partir das linhas da caixa. Fica de fora: suporte do Airbnb, conversa antiga (mais de 24 h ou sem hora),
/// so "ok/obrigado" e o que ja foi resolvido: nada disso acorda a familia.
pub fn pendencias(linhas: &[Linha], agora: &Agora) -> Vec<(String, Pendencia)> {
    let mut p = Vec::new();
    for l in linhas {
        if l.suporte_airbnb || !idade_horas(&l.quando, agora).map_or(false, |h| h <= 24.0) || encerramento(&l.previa) || ja_resolvido(&l.meta, &l.previa) {
            continue;
        }
        let base = Pendencia { hospede: l.hospede.clone(), anuncio: l.anuncio.clone(), previa: l.previa.clone(), sig: format!("{}|{}", l.previa, l.quando), ..Default::default() };
        if reserva_pendente().is_match(&l.previa) && !l.previa.to_lowercase().contains("aceit") {
            p.push((l.id.clone(), Pendencia { tipo: "reserva".into(), datas: l.meta.clone(), ..base }));
        } else if pendente_de_resposta(l) {
            // quem esta CONSULTANDO para reservar nao espera 30 min: vai na hora
            let consulta = l.meta.trim().to_lowercase().starts_with("consulta");
            p.push((l.id.clone(), Pendencia { tipo: if consulta { "reserva" } else { "mensagem" }.into(), consulta, datas: if consulta { l.meta.clone() } else { String::new() }, ..base }));
        }
    }
    p
}

/// Uma linha so, com numero longo trocado por "[número]" (nunca repassa cartao/CPF/conta que o hospede digitou) e no maximo n letras.
pub fn resumo(t: &str, n: usize) -> String {
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    let t = rx!(r"\d[\d .\-/]{9,}\d").replace_all(&t, "[número]").to_string();
    if t.chars().count() <= n {
        return t;
    }
    let corte: String = t.chars().take(n).collect();
    format!("{}…", corte.rsplit_once(' ').map_or(corte.as_str(), |x| x.0))
}

fn citar(p: &Option<Pessoa>) -> Option<&str> {
    p.as_ref().map(|x| x.citado.as_str()).filter(|c| !c.is_empty())
}

#[derive(Clone, Copy, PartialEq)]
enum Quem {
    Primeira,
    Dono,
    Terceira,
}

fn texto_mensagem(quem: Quem, d: &Destinos, voc: &str, p: &Pendencia, minutos: i64, lembrete: bool) -> String {
    let q = resumo(&p.previa, 140);
    let onde = if p.anuncio.is_empty() { String::new() } else { format!(" ({})", p.anuncio) };
    let perg = if q.is_empty() { String::new() } else { format!(" Ele perguntou: \"{q}\"") };
    if quem == Quem::Primeira {
        return format!("{voc}, {}ninguém respondeu o hóspede {} no Airbnb{onde}, já faz {minutos} min.{perg} Vê se consegue olhar pra mim?", if lembrete { "ainda " } else { "" }, p.hospede);
    }
    let tambem = citar(&d.primeira).map(|c| format!(" Avisei {c} também.")).unwrap_or_default();
    format!("{voc}, {}o hóspede {} está há {minutos} min sem resposta no Airbnb{onde}.{perg}{tambem}", if lembrete { "lembrete: " } else { "" }, p.hospede)
}

fn texto(quem: Quem, d: &Destinos, voc: &str, p: &Pendencia, minutos: i64, hora: u32) -> String {
    let (h, reserva) = (&p.hospede, p.tipo == "reserva");
    if reserva && quem != Quem::Terceira && minutos == 0 {
        // sintese do pedido para os dois juntos; quem aceita sao eles, nunca o sistema
        let oi = if hora < 12 { "bom dia" } else if hora < 18 { "boa tarde" } else { "boa noite" };
        let an = if p.anuncio.is_empty() { String::new() } else { format!(" para o {}", p.anuncio) };
        let datas = if p.datas.is_empty() { String::new() } else { format!(", {}", p.datas) };
        let q = resumo(&p.previa, 140);
        let perg = if !q.is_empty() && !reserva_pendente().is_match(&q) { format!(" Ele escreveu: \"{q}\"") } else { String::new() };
        let chegou = format!("hegou {} no Airbnb{an}. O hóspede é {h}{datas}.{perg}", if p.consulta { "uma consulta para reservar" } else { "um pedido de reserva" });
        return if quem == Quem::Primeira {
            format!("{voc}, {oi}! c{chegou} Você {} lá no Airbnb?", if p.consulta { "responde" } else { "aceita" })
        } else {
            format!("{voc}, {oi}. C{chegou}{} Eu não aceito reserva.", citar(&d.primeira).map(|c| format!(" Avisei {c} também.")).unwrap_or_default())
        };
    }
    match quem {
        Quem::Primeira if reserva => format!("{voc}, tem um pedido de reserva do {h} no Airbnb esperando resposta, vê se consegue olhar pra mim?"),
        Quem::Primeira => format!("{voc}, o hóspede {h} mandou mensagem no Airbnb e ainda não foi respondido, vê se consegue olhar pra mim?"),
        Quem::Terceira => {
            let nem = match (citar(&d.primeira), citar(&d.dono)) { (Some(a), Some(b)) => format!(", nem {a} nem {b}"), _ => String::new() };
            format!("{voc}, aqui é o Jarvis. Tem {} do {h} no Airbnb parada há {minutos} min e ninguém respondeu{nem}. Consegue olhar?", if reserva { "um pedido de reserva" } else { "uma mensagem" })
        }
        Quem::Dono => format!("{voc}, {} do {h} no Airbnb sem resposta há {minutos} min{}. Preciso do seu OK/resposta.", if reserva { "pedido de reserva" } else { "mensagem" },
            citar(&d.primeira).map(|c| format!(" (já avisei {c})")).unwrap_or_default()),
    }
}

const PADRAO_POR_HORA: usize = 2;

/// Avisos por hora e por destinatario (0 = sem teto). Sem arquivo: 2.
pub fn avisos_por_hora(dados: &Path) -> usize {
    std::fs::read_to_string(dados.join("airbnb_config.json")).ok().and_then(|t| serde_json::from_str::<Value>(t.trim_start_matches('\u{feff}')).ok())
        .and_then(|v| v["avisos_por_hora"].as_u64()).map_or(PADRAO_POR_HORA, |n| n as usize)
}

fn carregar_teto(dados: &Path) -> Value {
    std::fs::read_to_string(dados.join("airbnb_teto.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}))
}

/// Registra um envio feito agora e poda os de mais de 1 h.
fn vagas(teto: &mut Value, contato: &str, t: i64) -> usize {
    let e = &mut teto["envios"][contato];
    let mut v: Vec<i64> = e.as_array().map(|a| a.iter().filter_map(Value::as_i64).filter(|x| *x > t - JANELA_S).collect()).unwrap_or_default();
    v.sort();
    *e = json!(v);
    v.len()
}

fn registrar_envio(teto: &mut Value, contato: &str, t: i64) {
    vagas(teto, contato, t);
    if let Some(a) = teto["envios"][contato].as_array_mut() {
        a.push(json!(t));
    }
}

fn enfileirar(teto: &mut Value, contato: &str, quem: &str, voc: &str, tid: &str, p: &Pendencia) {
    let f = &mut teto["fila"][contato];
    f["quem"] = json!(quem);
    f["voc"] = json!(voc);
    if !f["itens"].is_array() {
        f["itens"] = json!([]);
    }
    let itens = f["itens"].as_array_mut().expect("itens e lista");
    if !itens.iter().any(|i| i["tid"] == tid) {
        itens.push(json!({ "tid": tid, "hospede": p.hospede, "anuncio": p.anuncio, "reserva": p.tipo == "reserva" }));
    }
}

/// O aviso unico que junta a fila de um destinatario (reservas primeiro). Nada do conteudo da mensagem: so hospede e anuncio.
fn texto_agrupado(quem: &str, voc: &str, itens: &[Value]) -> String {
    let nome = |i: &Value| {
        let (h, a) = (i["hospede"].as_str().unwrap_or("?"), i["anuncio"].as_str().unwrap_or(""));
        if a.is_empty() { h.to_string() } else { format!("{h} ({a})") }
    };
    let (res, com): (Vec<&Value>, Vec<&Value>) = itens.iter().partition(|i| i["reserva"] == true);
    let lista = |v: &[&Value]| v.iter().map(|i| nome(i)).collect::<Vec<_>>().join(", ");
    let mut partes = Vec::new();
    if !res.is_empty() {
        partes.push(format!("{} {} de reserva: {}", res.len(), if res.len() == 1 { "pedido" } else { "pedidos" }, lista(&res)));
    }
    if !com.is_empty() {
        partes.push(format!("{} {} esperando resposta: {}", com.len(), if com.len() == 1 { "hóspede" } else { "hóspedes" }, lista(&com)));
    }
    let corpo = format!("{voc}, {}tem no Airbnb {}.", if quem == "terceira" { "aqui é o Jarvis. " } else { "" }, partes.join("; e "));
    match quem {
        "primeira" => format!("{corpo} Vê se consegue olhar pra mim?"),
        "dono" => format!("{corpo} Preciso do seu OK/resposta.{}", if res.is_empty() { "" } else { " Eu não aceito reserva." }),
        _ => format!("{corpo} Consegue olhar?"),
    }
}

/// Depois do ciclo: tira da fila quem ja foi respondido (`vivos` = pendencias ainda no estado) e, havendo vaga, manda UM aviso agrupado.
fn despachar_fila(dados: &Path, teto: &mut Value, vivos: &BTreeSet<String>, t: i64, agora: &Agora, enviar: &dyn Fn(&str, &str) -> Result<(), String>) {
    let limite = avisos_por_hora(dados);
    let contatos: Vec<String> = teto["fila"].as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
    for c in contatos {
        if let Some(a) = teto["fila"][&c]["itens"].as_array_mut() {
            a.retain(|i| vivos.contains(i["tid"].as_str().unwrap_or("")));
        }
        let itens = teto["fila"][&c]["itens"].as_array().cloned().unwrap_or_default();
        if itens.is_empty() {
            teto["fila"].as_object_mut().map(|o| o.remove(&c));
            continue;
        }
        if limite > 0 && vagas(teto, &c, t) >= limite {
            continue;
        }
        let (quem, voc) = (teto["fila"][&c]["quem"].as_str().unwrap_or("").to_string(), teto["fila"][&c]["voc"].as_str().unwrap_or("Oi").to_string());
        match enviar(&c, &texto_agrupado(&quem, &voc, &itens)) {
            Ok(()) => {
                registrar_envio(teto, &c, t);
                teto["fila"].as_object_mut().map(|o| o.remove(&c));
                log(dados, agora, &format!("aviso agrupado ({} em espera) -> {quem}", itens.len()));
            }
            Err(x) => log(dados, agora, &format!("falha ao mandar o agrupado para {quem}: {x}")),
        }
    }
}

fn log(dados: &Path, agora: &Agora, t: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dados.join("airbnb.log")) {
        let _ = writeln!(f, "{} {}:{:02} | escalada | {t}", agora.data.dia_mes(), agora.hm(), agora.seg);
    }
}

fn carregar(dados: &Path) -> Map<String, Value> {
    std::fs::read_to_string(dados.join("airbnb_escalada.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.as_object().cloned()).unwrap_or_default()
}

fn gravar(dados: &Path, st: &Map<String, Value>) -> Result<(), String> {
    std::fs::create_dir_all(dados).and_then(|_| std::fs::write(dados.join("airbnb_escalada.json"), Value::Object(st.clone()).to_string()))
        .map_err(|e| format!("não gravei a escalada: {e}"))
}

/// Avanca a escalada. `pend` = pendencias lidas agora; `vistos` = ids lidos agora (so estes podem ser dados como resolvidos);
/// `enviar(contato, texto)` e de quem chama. Falha de envio fica no log e a escalada segue. Devolve o estado gravado.
pub fn passo(dados: &Path, pend: &[(String, Pendencia)], vistos: &BTreeSet<String>, agora: &Agora, enviar: &dyn Fn(&str, &str) -> Result<(), String>, destinos: &Destinos) -> Result<Value, String> {
    let t = seg_local(agora);
    let mut st = carregar(dados);
    let mut ok = st.remove("_ok").and_then(|v| v.as_object().cloned()).unwrap_or_default();
    // "ja respondi": vale para a mensagem que esta na caixa agora; mensagem nova (outra assinatura) volta a escalar
    let mut ativas: Vec<&(String, Pendencia)> = Vec::new();
    for item in pend {
        let (tid, p) = item;
        if ok.get(tid).and_then(Value::as_str) == Some("*") {
            ok.insert(tid.clone(), if p.sig.is_empty() { Value::Null } else { json!(p.sig) });
        }
        if !p.sig.is_empty() && ok.get(tid).and_then(Value::as_str) == Some(p.sig.as_str()) {
            st.remove(tid);
        } else {
            ativas.push(item);
        }
    }
    let resolvidas: Vec<String> = st.keys().filter(|k| vistos.contains(*k) && !ativas.iter().any(|a| a.0 == **k)).cloned().collect();
    for tid in resolvidas {
        if let Some(e) = st.remove(&tid) {
            log(dados, agora, &format!("{} resolvida", e["hospede"].as_str().unwrap_or("?")));
        }
    }
    let limite = avisos_por_hora(dados);
    let teto = std::cell::RefCell::new(carregar_teto(dados));
    // todo aviso passa por aqui: dentro do teto sai; estourou (ou ja ha fila) espera para sair agrupado
    let gate = |contato: &str, quem: &str, voc: &str, tid: &str, p: &Pendencia, msg: &str| -> Result<(), String> {
        if limite == 0 {
            return enviar(contato, msg);
        }
        let mut tt = teto.borrow_mut();
        if vagas(&mut tt, contato, t) < limite && tt["fila"][contato]["itens"].as_array().map_or(true, |a| a.is_empty()) {
            registrar_envio(&mut tt, contato, t);
            return enviar(contato, msg);
        }
        enfileirar(&mut tt, contato, quem, voc, tid, p);
        log(dados, agora, &format!("{} -> {quem}: segurado pelo teto de {limite} por hora", p.hospede));
        Ok(())
    };
    for (tid, p) in ativas {
        let e = st.entry(tid.clone()).or_insert_with(|| json!({ "t0": t, "etapa": 0, "ult": 0, "tipo": p.tipo, "consulta": p.consulta, "hospede": p.hospede,
            "anuncio": p.anuncio, "datas": p.datas, "previa": p.previa, "sig": p.sig }));
        let (t0, etapa, ult) = (e["t0"].as_i64().unwrap_or(t), e["etapa"].as_i64().unwrap_or(0), e["ult"].as_i64().unwrap_or(0));
        let mins = (t - t0) / 60;
        if etapa == 4 {
            continue;
        }
        let mandar = |quem: Quem, lembrete: Option<bool>| {
            let (pessoa, nome) = match quem { Quem::Primeira => (&destinos.primeira, "primeira"), Quem::Dono => (&destinos.dono, "dono"), Quem::Terceira => (&destinos.terceira, "terceira") };
            let Some(pessoa) = pessoa else { return };
            let voc = if pessoa.vocativo.is_empty() { "Oi" } else { pessoa.vocativo.as_str() };
            let msg = match lembrete { Some(l) => texto_mensagem(quem, destinos, voc, p, mins, l), None => texto(quem, destinos, voc, p, mins, agora.hora) };
            match gate(&pessoa.contato, nome, voc, tid, p, &msg) {
                Ok(()) => log(dados, agora, &format!("{} -> {nome}{}", p.hospede, if lembrete == Some(true) { " (lembrete)" } else { "" })),
                Err(x) => log(dados, agora, &format!("falha ao avisar {nome}: {x}")),
            }
        };
        let (mut nova, mut novo_ult) = (etapa, ult);
        if p.tipo == "mensagem" {
            let lembrete = if etapa == 0 && t - t0 >= ESPERA_MENSAGEM_S { false } else if etapa == 1 && t - ult >= LEMBRETE_MENSAGEM_S { true } else { continue };
            mandar(Quem::Primeira, Some(lembrete));
            mandar(Quem::Dono, Some(lembrete));
            (nova, novo_ult) = (if lembrete { 4 } else { 1 }, t);
        } else if t - t0 > JANELA_S {
            nova = 4; // encerrada: o registro fica ate a pendencia sumir, para nao recomecar do zero
            log(dados, agora, &format!("{} sem resposta ha mais de 1 h: parei de avisar", p.hospede));
        } else if etapa == 0 {
            mandar(Quem::Primeira, None);
            mandar(Quem::Dono, None);
            (nova, novo_ult) = (2, t);
        } else if etapa == 1 && t - ult >= INTERVALO_S {
            mandar(Quem::Dono, None);
            (nova, novo_ult) = (2, t);
        } else if etapa == 2 && t - ult >= INTERVALO_S {
            mandar(Quem::Terceira, None);
            (nova, novo_ult) = (3, t);
        } else if etapa == 3 && t - ult >= INTERVALO_S {
            mandar(Quem::Primeira, None);
            mandar(Quem::Dono, None);
            novo_ult = t;
        }
        e["etapa"] = json!(nova);
        e["ult"] = json!(novo_ult);
    }
    st.insert("_ok".into(), Value::Object(ok));
    if limite > 0 {
        let vivos: BTreeSet<String> = st.keys().filter(|k| *k != "_ok").cloned().collect();
        let mut tt = teto.into_inner();
        despachar_fila(dados, &mut tt, &vivos, t, agora, enviar);
        let _ = std::fs::write(dados.join("airbnb_teto.json"), tt.to_string());
    }
    gravar(dados, &st)?;
    Ok(Value::Object(st))
}

/// O dono disse "ja respondi". Marca como resolvidas as pendencias citadas pelo primeiro nome (ou todas as ativas, se nao citou ninguem)
/// ate chegar mensagem NOVA do hospede. Devolve a resposta, ou None se nao ha nada ativo (a frase segue o caminho normal).
pub fn reconhecer(dados: &Path, texto: &str, agora: &Agora) -> Option<String> {
    let mut st = carregar(dados);
    let t = texto.to_lowercase();
    let nome = |e: &Value| e["hospede"].as_str().unwrap_or("").to_string();
    let ativos: Vec<(String, String)> = st.iter().filter(|(k, _)| *k != "_ok").map(|(k, e)| (k.clone(), nome(e))).collect();
    if ativos.is_empty() {
        return None;
    }
    let citados: Vec<&(String, String)> = ativos.iter().filter(|(_, h)| h.split_whitespace().next().map_or(false, |p| t.contains(&p.to_lowercase()))).collect();
    let alvo: Vec<&(String, String)> = if citados.is_empty() { ativos.iter().collect() } else { citados };
    let mut ok = st.remove("_ok").and_then(|v| v.as_object().cloned()).unwrap_or_default();
    for (id, _) in &alvo {
        ok.insert(id.clone(), json!("*"));
        st.remove(id);
    }
    st.insert("_ok".into(), Value::Object(ok));
    gravar(dados, &st).ok()?;
    let nomes = alvo.iter().map(|a| a.1.as_str()).collect::<Vec<_>>().join(", ");
    log(dados, agora, &format!("o dono disse que ja respondeu: {nomes}"));
    Some(format!("Anotado: marquei {nomes} como já respondido(s) no Airbnb. Não aviso mais até chegar mensagem nova."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;
    use std::cell::RefCell;

    fn pasta(nome: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("braniac-airbnb_escalada-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }
    // nomes inventados
    fn dest() -> Destinos {
        Destinos::de_json(&json!({ "primeira": {"contato": "Tia", "vocativo": "oh minha tia", "citado": "a Tia Bia"}, "dono": {"contato": "Davi", "vocativo": "Senhor Davi", "citado": "o Davi"},
            "terceira": {"contato": "Lia", "vocativo": "Lia", "citado": "a Lia"} }))
    }
    /// instante t (segundos) num dia fixo; t = 0 cai as 09:00
    fn em(t: i64) -> Agora {
        let s = 9 * 3600 + t;
        Agora { data: Data(22000 + s / 86400), hora: (s % 86400 / 3600) as u32, min: (s % 3600 / 60) as u32, seg: (s % 60) as u32 }
    }
    fn ids(v: &[&str]) -> BTreeSet<String> { v.iter().map(|x| x.to_string()).collect() }
    fn ativos(st: &Value) -> Vec<String> { st.as_object().unwrap().keys().filter(|k| *k != "_ok").cloned().collect() }

    #[test]
    fn escada_reserva_e_resolucao() {
        let p = pasta("reserva");
        let env = RefCell::new(Vec::<(String, String)>::new());
        let f = |n: &str, t: &str| { env.borrow_mut().push((n.to_string(), t.to_string())); Ok(()) };
        let pend = vec![("1".to_string(), Pendencia { tipo: "reserva".into(), hospede: "Ana".into(), anuncio: "Quarto Azul".into(), datas: "18 – 19 de nov.".into(), previa: "posso chegar mais cedo?".into(), ..Default::default() })];
        let nomes = || env.borrow().iter().map(|e| e.0.clone()).collect::<Vec<_>>();
        passo(&p, &pend, &ids(&["1"]), &em(0), &f, &dest()).unwrap(); // reserva vai aos dois JUNTOS, na hora, com a sintese
        assert_eq!(nomes(), ["Tia", "Davi"]);
        assert!(env.borrow().iter().all(|(_, t)| t.contains("Quarto Azul") && t.contains("Ana") && t.contains("18 – 19") && t.contains("mais cedo")));
        let (a, b) = (env.borrow()[0].1.clone(), env.borrow()[1].1.clone());
        assert!(a.starts_with("oh minha tia, bom dia! chegou um pedido de reserva") && a.ends_with("Você aceita lá no Airbnb?"), "{a}");
        assert!(b.starts_with("Senhor Davi, bom dia. Chegou um pedido") && b.contains("Avisei a Tia Bia também.") && b.contains("não aceito reserva"), "{b}");
        passo(&p, &pend, &ids(&["1"]), &em(899), &f, &dest()).unwrap(); // antes de 15 min: nada
        assert_eq!(nomes().len(), 2);
        passo(&p, &pend, &ids(&["1"]), &em(901), &f, &dest()).unwrap(); // 15 min: a terceira pessoa
        assert!(env.borrow()[2].1.contains("aqui é o Jarvis") && env.borrow()[2].1.contains("nem a Tia Bia nem o Davi") && !env.borrow()[2].1.contains("Claudinho"));
        passo(&p, &pend, &ids(&["1"]), &em(1802), &f, &dest()).unwrap(); // ciclo a cada 15 min: os dois de novo
        assert_eq!(nomes(), ["Tia", "Davi", "Lia", "Tia", "Davi"]);
        for t in [3601, 5000, 9000] { // passada 1 h: para de avisar
            passo(&p, &pend, &ids(&["1"]), &em(t), &f, &dest()).unwrap();
        }
        assert_eq!(nomes().len(), 5);
        assert!(ativos(&passo(&p, &[], &ids(&["1"]), &em(9100), &f, &dest()).unwrap()).is_empty()); // alguem respondeu: some
        assert!(std::fs::read_to_string(p.join("airbnb.log")).unwrap().contains("Ana resolvida"));
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn mensagem_30_min_os_dois_juntos_um_lembrete_e_para() {
        let p = pasta("mensagem");
        let env = RefCell::new(Vec::<(String, String)>::new());
        let f = |n: &str, t: &str| { env.borrow_mut().push((n.to_string(), t.to_string())); Ok(()) };
        let pend = vec![("2".to_string(), Pendencia { tipo: "mensagem".into(), hospede: "Bia".into(), anuncio: "Quarto Azul".into(), previa: "Tem toalha? meu cartao 4111 1111 1111 1111".into(), sig: "x".into(), ..Default::default() })];
        let nomes = || env.borrow().iter().map(|e| e.0.clone()).collect::<Vec<_>>();
        for t in [0, 60, 900, 1799] {
            passo(&p, &pend, &ids(&["2"]), &em(t), &f, &dest()).unwrap();
        }
        assert!(nomes().is_empty()); // menos de 30 min: ninguem
        passo(&p, &pend, &ids(&["2"]), &em(1800), &f, &dest()).unwrap();
        passo(&p, &pend, &ids(&["2"]), &em(1830), &f, &dest()).unwrap(); // ciclo seguinte nao duplica
        assert_eq!(nomes(), ["Tia", "Davi"]);
        let (a, b) = (env.borrow()[0].1.clone(), env.borrow()[1].1.clone());
        assert!(a.starts_with("oh minha tia") && a.contains("Bia") && a.contains("Quarto Azul") && a.contains("toalha") && a.contains("30 min"));
        assert!(b.starts_with("Senhor Davi") && !format!("{a}{b}").contains("4111") && a.contains("[número]"));
        for t in [2000, 3599] {
            passo(&p, &pend, &ids(&["2"]), &em(t), &f, &dest()).unwrap();
        }
        assert_eq!(nomes().len(), 2);
        passo(&p, &pend, &ids(&["2"]), &em(3600), &f, &dest()).unwrap(); // 1 lembrete 30 min depois
        assert!(nomes() == ["Tia", "Davi", "Tia", "Davi"] && env.borrow()[3].1.contains("lembrete") && env.borrow()[2].1.contains("ainda ninguém"));
        for t in [5400, 7200, 20000] {
            passo(&p, &pend, &ids(&["2"]), &em(t), &f, &dest()).unwrap();
        }
        assert_eq!(nomes().len(), 4); // e para
        // conversa que nao foi lida neste ciclo nao e dada como resolvida
        assert_eq!(ativos(&passo(&p, &[], &ids(&["outra"]), &em(20050), &f, &dest()).unwrap()), ["2"]);
        assert!(ativos(&passo(&p, &[], &ids(&["2"]), &em(20100), &f, &dest()).unwrap()).is_empty()); // respondido: some
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn ja_respondi_silencia_ate_mensagem_nova() {
        let p = pasta("jarespondi");
        assert_eq!(reconhecer(&p, "ja respondi", &em(0)), None); // nada ativo: a frase segue o caminho normal
        let env = RefCell::new(Vec::<String>::new());
        let f = |n: &str, _: &str| { env.borrow_mut().push(n.to_string()); Ok(()) };
        let m = |sig: &str| vec![("9".to_string(), Pendencia { tipo: "mensagem".into(), hospede: "Leonardo Reis".into(), sig: sig.into(), ..Default::default() })];
        passo(&p, &m("oi|09:42"), &ids(&["9"]), &em(0), &f, &dest()).unwrap();
        passo(&p, &m("oi|09:42"), &ids(&["9"]), &em(1800), &f, &dest()).unwrap(); // os dois avisados
        assert!(reconhecer(&p, "ja respondi o Leonardo no airbnb", &em(1900)).unwrap().contains("Leonardo Reis"));
        passo(&p, &m("oi|09:42"), &ids(&["9"]), &em(3700), &f, &dest()).unwrap();
        passo(&p, &m("oi|09:42"), &ids(&["9"]), &em(5000), &f, &dest()).unwrap();
        assert_eq!(*env.borrow(), ["Tia", "Davi"]); // nada mais enquanto a mensagem e a mesma
        passo(&p, &m("e ai?|10:50"), &ids(&["9"]), &em(6000), &f, &dest()).unwrap();
        passo(&p, &m("e ai?|10:50"), &ids(&["9"]), &em(7800), &f, &dest()).unwrap();
        assert_eq!(*env.borrow(), ["Tia", "Davi", "Tia", "Davi"]); // mensagem NOVA volta a escalar
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn filtra_avisos_do_airbnb_e_reservas_ja_decididas() {
        for (meta, previa) in [("5 – 14 de jan. de 2027", "A pré-aprovação expirou"), ("Pendente. 14 – 16 de nov.", "O pedido de reserva expirou"), ("Cancelada. 20 – 22 de nov.", "O pedido de reserva foi cancelado"),
            ("6 – 9 de nov.", "Pré-aprovação enviada"), ("Consulta. 7 – 9 de nov.", "Uma mensagem foi cancelada"), ("Confirmada. 1 – 3 de dez.", "Reserva confirmada")] {
            assert!(ja_resolvido(meta, previa), "{meta} {previa}");
        }
        for (meta, previa) in [("Consulta. 7 – 9 de nov.", "Vocês aceitam pet?"), ("Pendente. 14 – 16 de nov.", "Pedido de reserva"), ("Consulta. 5 – 7 de nov.", "Preciso cancelar, posso?"), ("11 – 15 de dez.", "Tem vaga na garagem?")] {
            assert!(!ja_resolvido(meta, previa), "{meta} {previa}");
        }
    }

    #[test]
    fn pendencias_da_caixa_e_consulta_avisa_na_hora() {
        let p = pasta("consulta");
        let agora = em(3600); // 10:00
        let l = |id: &str, meta: &str, previa: &str, quando: &str, de: &str| Linha { id: id.into(), hospede: "Hugo".into(), anuncio: "Apartamento aconchegante".into(), meta: meta.into(),
            previa: previa.into(), quando: quando.into(), ultima_de: de.into(), ..Default::default() };
        let caixa = [l("9", "Consulta. 2 – 9 de jan.", "Olá, procuro um lugar para as férias", "09:55", "hospede"), l("3", "2 – 9 de jan.", "Tem garagem?", "09:10", "hospede"),
            l("4", "Pendente. 5 – 6 de jan.", "Pedido de reserva", "09:00", "outro"), l("5", "x", "Tem garagem?", "seg", "hospede"), l("6", "x", "Ok, obrigado!", "09:50", "hospede"),
            l("7", "x", "Reserva confirmada", "09:50", "hospede"), l("8", "x", "já respondemos", "09:50", "voce"), Linha { suporte_airbnb: true, ..l("2", "x", "oi?", "09:50", "hospede") }];
        let pd = pendencias(&caixa, &agora);
        assert_eq!(pd.iter().map(|x| (x.0.as_str(), x.1.tipo.as_str(), x.1.consulta)).collect::<Vec<_>>(), [("9", "reserva", true), ("3", "mensagem", false), ("4", "reserva", false)]);
        assert_eq!((pd[0].1.sig.as_str(), pd[0].1.datas.as_str(), pd[1].1.datas.as_str()), ("Olá, procuro um lugar para as férias|09:55", "Consulta. 2 – 9 de jan.", ""));
        let env = RefCell::new(Vec::<(String, String)>::new());
        let f = |n: &str, t: &str| { env.borrow_mut().push((n.to_string(), t.to_string())); Ok(()) };
        let so_dois = Destinos { terceira: None, ..dest() };
        passo(&p, &pd[..1], &ids(&["9"]), &agora, &f, &so_dois).unwrap();
        assert!(env.borrow().len() == 2 && env.borrow()[0].1.contains("consulta para reservar") && env.borrow()[0].1.ends_with("Você responde lá no Airbnb?") && env.borrow()[1].1.contains("procuro um lugar"));
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn resumo_esconde_numero_longo_e_envio_que_falha_nao_derruba() {
        assert_eq!(resumo("meu cpf 123.456.789-09 e o cartão 4111-1111-1111-1111, ok?", 140), "meu cpf [número] e o cartão [número], ok?");
        assert_eq!(resumo("quarto 12, dia 5", 140), "quarto 12, dia 5");
        let longo = resumo(&"palavra ".repeat(40), 140);
        assert!(longo.ends_with("palavra…") && longo.chars().count() <= 141);
        let p = pasta("falha");
        let pend = vec![("1".to_string(), Pendencia { tipo: "reserva".into(), hospede: "Ana".into(), ..Default::default() })];
        let st = passo(&p, &pend, &ids(&["1"]), &em(0), &|_, _| Err("sem conexão".into()), &dest()).unwrap();
        assert_eq!(st["1"]["etapa"], 2);
        assert!(std::fs::read_to_string(p.join("airbnb.log")).unwrap().contains("falha ao avisar primeira: sem conexão"));
        // sem configuracao ninguem e avisado e nada quebra
        let n = RefCell::new(0);
        passo(&p, &pend, &ids(&["1"]), &em(1000), &|_, _| { *n.borrow_mut() += 1; Ok(()) }, &Destinos::ler(&p)).unwrap();
        assert_eq!(*n.borrow(), 0);
        let _ = std::fs::remove_dir_all(&p);
    }

    fn msgs(p: &Pendencia) -> Vec<(String, Pendencia)> { vec![(p.sig.clone(), p.clone())] }

    #[test]
    fn teto_por_hora_agrupa_prioriza_reserva_e_destinatarios_nao_dividem() {
        let p = pasta("teto");
        let env = RefCell::new(Vec::<(String, String)>::new());
        let f = |n: &str, t: &str| { env.borrow_mut().push((n.to_string(), t.to_string())); Ok(()) };
        let m = |id: &str, h: &str| (id.to_string(), Pendencia { tipo: "mensagem".into(), hospede: h.into(), anuncio: "Quarto Azul".into(), sig: id.into(), ..Default::default() });
        // 5 mensagens comuns, 1 a cada 2 min, todas vencem os 30 min juntas depois de t0: abrimos cada uma em t = 0, 120, ... e passamos a 1800+
        let tudo: Vec<(String, Pendencia)> = ["A", "B", "C", "D", "E"].iter().map(|h| m(h, h)).collect();
        let vistos = ids(&["A", "B", "C", "D", "E"]);
        passo(&p, &tudo[..1], &vistos, &em(0), &f, &dest()).unwrap();
        passo(&p, &tudo[..2], &vistos, &em(120), &f, &dest()).unwrap();
        passo(&p, &tudo[..3], &vistos, &em(240), &f, &dest()).unwrap();
        passo(&p, &tudo[..4], &vistos, &em(360), &f, &dest()).unwrap();
        passo(&p, &tudo, &vistos, &em(480), &f, &dest()).unwrap();
        for t in [1800, 1920, 2040, 2160, 2280] { passo(&p, &tudo, &vistos, &em(t), &f, &dest()).unwrap(); }
        let para = |c: &str| env.borrow().iter().filter(|e| e.0 == c).count();
        // destinatarios nao dividem: cada um recebeu 2 (e so 2) dentro da hora
        assert_eq!((para("Tia"), para("Davi")), (2, 2), "{:?}", env.borrow());
        // quem ja foi respondido sai da lista: A e B sumem (ja avisados), C e D vao, E responde antes da vaga
        let resto: Vec<(String, Pendencia)> = ["C", "D"].iter().map(|h| m(h, h)).collect();
        // a janela de 60 min abre depois de t=1800+3600
        passo(&p, &resto, &ids(&["A", "B", "C", "D", "E"]), &em(1800 + 3601), &f, &dest()).unwrap();
        let (a, b) = (env.borrow()[4].clone(), env.borrow()[5].clone());
        let agrup: Vec<_> = env.borrow().iter().skip(4).cloned().collect();
        assert_eq!(agrup.len(), 2, "{:?}", env.borrow());
        let dono = agrup.iter().find(|x| x.0 == "Davi").unwrap();
        assert!(dono.1.starts_with("Senhor Davi, tem no Airbnb 2 hóspedes esperando resposta: C (Quarto Azul), D (Quarto Azul).") && !dono.1.contains('E'), "{} {}", a.1, b.1);
        assert!(!dono.1.contains("Ele perguntou"));
        let _ = msgs(&resto[0].1);
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn teto_reserva_vai_primeiro_e_teto_zero_e_como_antes() {
        let p = pasta("teto2");
        std::fs::write(p.join("airbnb_config.json"), r#"{"avisos_por_hora": 1}"#).unwrap();
        let env = RefCell::new(Vec::<(String, String)>::new());
        let f = |n: &str, t: &str| { env.borrow_mut().push((n.to_string(), t.to_string())); Ok(()) };
        let c = |id: &str, h: &str, tipo: &str| (id.to_string(), Pendencia { tipo: tipo.into(), hospede: h.into(), anuncio: "Quarto Azul".into(), sig: id.into(), ..Default::default() });
        let v = ids(&["1", "2", "3"]);
        passo(&p, &[c("1", "Ana", "reserva")], &v, &em(0), &f, &dest()).unwrap(); // sai (vaga 1/1)
        passo(&p, &[c("1", "Ana", "reserva"), c("2", "Bia", "mensagem"), c("3", "Caio", "reserva")], &v, &em(100), &f, &dest()).unwrap();
        assert_eq!(env.borrow().len(), 2); // so o primeiro de cada um
        passo(&p, &[c("1", "Ana", "reserva"), c("2", "Bia", "mensagem"), c("3", "Caio", "reserva")], &v, &em(3601), &f, &dest()).unwrap();
        let dono = env.borrow().iter().find(|x| x.0 == "Davi" && x.1.contains("Caio")).cloned().unwrap();
        assert!(dono.1.contains("pedido de reserva: Caio") || dono.1.contains("1 pedido de reserva"), "{}", dono.1);
        assert!(dono.1.find("reserva").unwrap() < dono.1.find("Bia").unwrap_or(usize::MAX), "{}", dono.1);
        // teto 0 = sem teto
        let q = pasta("teto0");
        std::fs::write(q.join("airbnb_config.json"), r#"{"avisos_por_hora": 0}"#).unwrap();
        let n = RefCell::new(0);
        let g = |_: &str, _: &str| { *n.borrow_mut() += 1; Ok(()) };
        for (i, t) in [0, 60, 120, 180, 240].iter().enumerate() {
            let id = i.to_string();
            passo(&q, &[c(&id, "X", "reserva")], &ids(&[id.as_str()]), &em(*t), &g, &dest()).unwrap();
        }
        assert_eq!(*n.borrow(), 10);
        let _ = std::fs::remove_dir_all(&p);
        let _ = std::fs::remove_dir_all(&q);
    }
}
