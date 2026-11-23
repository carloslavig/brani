//! No do BRANIAC LINK (link/no.py): as regras da conexao e o despachante das mensagens. Pedido do outro lado e DADO: tarefa so roda se a
//! permissao do dono for "aceitar" (ou depois de `aprovar` em "perguntar"); par pendente so faz ping; arquivo so entra se liberado.
//! Ficou de fora (entra por closure de quem chama): WebRTC/aiortc, a sinalizacao pelo ntfy, a cifra do "ola" (HKDF + AES-GCM + zlib),
//! as threads e o laco de escuta. Aqui `tratar` devolve a resposta (tipo, corpo, ref) e quem chama monta o envelope e envia.
#![allow(dead_code)]
use crate::link_arquivos::{Envio, Recebimento};
use crate::link_confianca::Pares;
use crate::link_identidade::{hex, sha256};
use crate::link_protocolo::{self as protocolo, Antirepeticao};
use regex::Regex;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const STUN: &str = "stun:stun.l.google.com:19302";
pub const MAX_TEXTO: usize = 8000;
pub const LIMITE_MIN: usize = 120;
pub const MAX_SINAL: usize = 3900;
pub const ESPERA_P2P_S: f64 = 300.0;
const CRLF: &str = "\r\n";

fn corta(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn texto(v: &Value) -> String {
    v.as_str().map(str::to_string).unwrap_or_else(|| if v.is_null() { String::new() } else { v.to_string() })
}

/// Topico da sinalizacao: nao da para adivinhar sem o segredo do par.
pub fn topico(seg: &str, para_id: &str) -> String {
    format!("blink-{}", &hex(&sha256(format!("{seg}|para|{para_id}").as_bytes()))[..32])
}

/// Tira do SDP os candidatos de placas virtuais (VirtualBox, WSL/Docker, Radmin/Hamachi, Teredo): so atrapalham a conexao direta.
pub fn sem_virtuais(sdp: &str) -> String {
    static R: OnceLock<Regex> = OnceLock::new();
    let r = R.get_or_init(|| Regex::new(r"(?i)^(192\.168\.56\.|172\.(1[6-9]|2\d|3[01])\.|169\.254\.|26\.|25\.|10\.0\.2\.|fe80:|fd|2001:0?:)").expect("regex fixa"));
    sdp.split(CRLF).filter(|l| !(l.starts_with("a=candidate:") && l.split(' ').nth(4).is_some_and(|ip| r.is_match(ip)))).collect::<Vec<_>>().join(CRLF)
}

/// Fingerprint DTLS do SDP (o que o "hello" assinado cobre).
pub fn fp(sdp: &str) -> String {
    static R: OnceLock<Regex> = OnceLock::new();
    let r = R.get_or_init(|| Regex::new(r"a=fingerprint:sha-256 ([0-9A-Fa-f:]{95})").expect("regex fixa"));
    r.captures(sdp).map(|c| c[1].to_uppercase()).unwrap_or_default()
}

/// Corpo do "hello": prende a assinatura do dispositivo a ESTE SDP (quem controla a sinalizacao nao consegue se passar pelo par).
pub fn corpo_hello(sdp: &str, sid: &str, nome: &str, prova: Option<&str>) -> Value {
    let mut c = json!({ "sid": sid, "sdp": hex(&sha256(sdp.as_bytes())), "fp": fp(sdp), "nome": nome });
    if let Some(p) = prova {
        c["prova"] = json!(p);
    }
    c
}

/// `env` ja validado pelo protocolo. Err se o hello nao cobre este SDP e esta sessao.
pub fn conferir_hello(env: &Value, sdp: &str, sid: &str) -> Result<(), String> {
    let c = &env["corpo"];
    let f = c["fp"].as_str().unwrap_or("");
    if env["tipo"] != "hello" || c["sid"] != sid || c["sdp"] != hex(&sha256(sdp.as_bytes())).as_str() || f.is_empty() || f != fp(sdp) {
        return Err("hello nao cobre este SDP".into());
    }
    Ok(())
}

#[derive(Debug, PartialEq)]
pub enum Origem {
    /// Par ja salvo, com a mesma chave e o mesmo segredo.
    Conhecido,
    /// Primeiro contato com prova de convite: quem chama consome o convite (`Pares::consumir_convite`) e o par nasce PENDENTE.
    ConviteNovo,
}

/// Quem mandou a oferta pode conectar? Par revogado, chave trocada ou segredo de outro par: nao.
pub fn origem_da_oferta(par: Option<&Value>, env: &Value, seg: &str) -> Result<Origem, String> {
    match par {
        Some(p) if p["pub"] == env["pub"] && p["seg"] == seg && p["estado"] != "revogado" => Ok(Origem::Conhecido),
        None if env["corpo"]["prova"].as_str().is_some_and(|x| !x.is_empty()) => Ok(Origem::ConviteNovo),
        _ => Err("par desconhecido ou revogado".into()),
    }
}

/// A resposta ("answer") da sinalizacao serve para a oferta que eu fiz? Mesma sessao, mesmo par, mesma chave.
pub fn resposta_serve(m: &Value, sid: &str, env: &Value, par_id: &str, par_pub: &str) -> bool {
    m["t"] == "answer" && m["sid"] == sid && env["de"] == par_id && env["pub"] == par_pub
}

/// Segredos que valem ouvir: pares nao revogados + convites abertos.
pub fn segredos(pares: &Pares, pasta_link: &Path, agora: i64) -> HashSet<String> {
    let d: Value = fs::read_to_string(pasta_link.join("pares.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    let mut s: HashSet<String> = d["pares"].as_object().into_iter().flatten().filter(|(_, p)| p["estado"] != "revogado").filter_map(|(_, p)| p["seg"].as_str().filter(|x| !x.is_empty()).map(str::to_string)).collect();
    s.extend(pares.convites_abertos(agora).keys().cloned());
    s
}

/// No maximo LIMITE_MIN mensagens por minuto por sessao. Anota esta chegada.
pub fn limite_ok(tempos: &mut Vec<f64>, agora: f64) -> bool {
    tempos.retain(|t| agora - *t < 60.0);
    tempos.push(agora);
    tempos.len() <= LIMITE_MIN
}

pub fn cabe_no_sinal(texto: &str) -> Result<(), String> {
    if texto.len() > MAX_SINAL { Err("sinalizacao grande demais para o canal".into()) } else { Ok(()) }
}

/// Resposta 429 com cota diaria: insistir antes da 0h UTC so gasta o limite de pedidos.
pub fn cota_diaria(status: u16, corpo: &str) -> bool {
    status == 429 && corpo.contains("daily message quota")
}

/// Ate quando ficar sem publicar depois da cota diaria estourar (0h UTC do dia seguinte + 60 s).
pub fn bloqueado_ate(agora: f64) -> f64 {
    ((agora / 86400.0).floor() + 1.0) * 86400.0 + 60.0
}

/// Espera progressiva depois de erro na escuta (429 do canal): dobra ate 60 s.
pub fn espera(intervalo: f64, anterior: f64) -> f64 {
    (anterior * 2.0).max(intervalo).min(60.0)
}

#[derive(Debug, PartialEq)]
pub enum Caminho {
    /// Canal direto aberto: usa.
    Direta,
    /// Tenta abrir a direta; se nao abrir, anota a falha e cai no plano B.
    TentarDireta,
    /// Plano B: so TEXTO pelo canal de sinalizacao (cifrado e assinado), sem arquivos.
    Relay,
}

/// Maquina de estados da conexao com um par: direta aberta > tentar direta (nao antes de 5 min de uma falha) > plano B.
pub fn caminho(direta_aberta: bool, p2p: bool, ultima_falha: f64, agora: f64) -> Caminho {
    if direta_aberta {
        Caminho::Direta
    } else if p2p && agora - ultima_falha > ESPERA_P2P_S {
        Caminho::TentarDireta
    } else {
        Caminho::Relay
    }
}

/// Estado do WebRTC que encerra a sessao.
pub fn sessao_acabou(estado: &str) -> bool {
    ["failed", "closed"].contains(&estado)
}

/// Pelo plano B nao passa arquivo; e com a cota do canal esgotada nem texto (falha na hora em vez de esperar o pong).
pub fn pode_enviar_msg(direta_aberta: bool, bloqueado_ate: f64, agora: f64) -> Result<(), String> {
    if agora < bloqueado_ate && !direta_aberta { Err("cota diaria do ntfy esgotada".into()) } else { Ok(()) }
}

pub struct Sessao {
    pub par_id: String,
    pub tempos: Vec<f64>,
    pub anti: Antirepeticao,
    pub receb: HashMap<String, Recebimento>,
    /// ids de pedidos meus esperando resposta
    pub pend: HashSet<String>,
}

impl Sessao {
    pub fn new(par_id: &str) -> Sessao {
        Sessao { par_id: par_id.to_string(), tempos: Vec::new(), anti: Antirepeticao::default(), receb: HashMap::new(), pend: HashSet::new() }
    }
}

/// O que o Braniac hospedeiro oferece ao no. Sem `executor`, toda tarefa e recusada; sem `ao_msg`, toda mensagem.
pub struct Ganchos<'a> {
    /// executor(agente, pedido, par) -> texto
    pub executor: Option<&'a dyn Fn(&str, &str, &str) -> Result<String, String>>,
    pub agentes: &'a dyn Fn() -> Vec<Value>,
    pub ao_msg: Option<&'a dyn Fn(&str, &str)>,
}

#[derive(Debug, PartialEq)]
pub struct Resposta {
    pub tipo: &'static str,
    pub corpo: Value,
    pub ref_: String,
}

#[derive(Debug, PartialEq)]
pub enum Entrada {
    /// Rejeitada ou sem resposta (ja foi para o registro).
    Nada,
    /// Resposta a um pedido meu que estava esperando.
    RespostaDe(Value),
    Responder(Resposta),
}

pub struct No {
    pub id: String,
    pub nome: String,
    pub pasta: PathBuf,
}

impl No {
    pub fn new(id: &str, nome: &str, pasta: &Path) -> Result<No, String> {
        fs::create_dir_all(pasta).map_err(|e| e.to_string())?;
        Ok(No { id: id.to_string(), nome: nome.to_string(), pasta: pasta.to_path_buf() })
    }

    pub fn log(&self, par: &str, evento: &str, detalhe: &str, agora: f64) {
        if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(self.pasta.join("registro.jsonl")) {
            let _ = writeln!(f, "{}", json!({ "ts": agora as i64, "par": par, "evento": evento, "detalhe": corta(detalhe, 200) }));
        }
    }

    pub fn pendencias(&self) -> Vec<Value> {
        fs::read_to_string(self.pasta.join("pendencias.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    fn gravar_pend(&self, p: &[Value]) -> Result<(), String> {
        fs::write(self.pasta.join("pendencias.json"), json!(p).to_string()).map_err(|e| e.to_string())
    }

    fn add_pend(&self, item: Value) -> Result<(), String> {
        let mut p = self.pendencias();
        let sobra = p.len().saturating_sub(49);
        p.drain(..sobra);
        p.push(item);
        self.gravar_pend(&p)
    }

    pub fn recusar(&self, id_item: &str, agora: f64) -> Result<(), String> {
        self.gravar_pend(&self.pendencias().into_iter().filter(|p| p["id"] != id_item).collect::<Vec<_>>())?;
        self.log("", "pendencia_recusada", id_item, agora);
        Ok(())
    }

    /// O dono aprovou um pedido que estava em "perguntar": roda o executor e devolve (par, resposta a entregar, texto).
    pub fn aprovar(&self, id_item: &str, g: &Ganchos, agora: f64) -> Result<(String, Resposta, String), String> {
        let item = self.pendencias().into_iter().find(|p| p["id"] == id_item).ok_or("pendencia nao encontrada")?;
        self.recusar(id_item, agora)?;
        let par = texto(&item["par"]);
        let txt = executar(g, &par, &texto(&item["agente"]), &texto(&item["pedido"]));
        Ok((par, Resposta { tipo: "tarefa.resultado", corpo: json!({ "ok": true, "texto": txt }), ref_: id_item.to_string() }, txt))
    }

    /// Texto que chegou pelo canal: limite por minuto, envelope valido, remetente = par da sessao, destino = eu, par nao revogado.
    pub fn receber(&self, sess: &mut Sessao, pares: &Pares, m: &str, verificar: &dyn Fn(&str, &[u8], &str) -> bool, g: &Ganchos, agora: f64) -> Entrada {
        if !limite_ok(&mut sess.tempos, agora) {
            return Entrada::Nada;
        }
        let r = (|| {
            let env = protocolo::validar(m, verificar, Some(&mut sess.anti), agora)?;
            if env["de"] != sess.par_id.as_str() || !(env["para"] == "" || env["para"] == self.id.as_str()) {
                return Err("remetente ou destino inesperado".to_string());
            }
            if pares.get(&sess.par_id).is_none_or(|p| p["estado"] == "revogado") {
                return Err("par revogado".into());
            }
            Ok(env)
        })();
        match r {
            Err(e) => {
                self.log(&sess.par_id, "rejeitado", &e, agora);
                Entrada::Nada
            }
            Ok(env) => {
                if env["ref"].as_str().is_some_and(|r| !r.is_empty() && sess.pend.remove(r)) {
                    return Entrada::RespostaDe(env);
                }
                self.tratar(sess, pares, &env, g, agora).map_or(Entrada::Nada, Entrada::Responder)
            }
        }
    }

    /// Quadro binario de arquivo: so entra em recebimento ja aceito nesta sessao.
    pub fn quadro(&self, sess: &mut Sessao, q: &[u8], agora: f64) {
        if q.len() < 8 {
            return;
        }
        if let Some(Err(e)) = sess.receb.get_mut(&hex(&q[..8])).map(|rec| rec.receber(q)) {
            self.log(&sess.par_id, "rejeitado", &e, agora);
        }
    }

    /// O despachante: o que cada tipo de mensagem de um par pode fazer.
    pub fn tratar(&self, sess: &mut Sessao, pares: &Pares, env: &Value, g: &Ganchos, agora: f64) -> Option<Resposta> {
        let (t, c, pid) = (env["tipo"].as_str().unwrap_or(""), &env["corpo"], sess.par_id.clone());
        let resp = |tipo: &'static str, corpo: Value| Some(Resposta { tipo, corpo, ref_: texto(&env["id"]) });
        if t == "ping" {
            return resp("pong", json!({ "nome": self.nome }));
        }
        if pares.get(&pid).is_none_or(|p| p["estado"] != "confiavel") {
            self.log(&pid, "ignorado_pendente", t, agora); // pendente so faz ping: ninguem conversa antes do dono conferir o SAS
            return None;
        }
        match t {
            "msg" => match g.ao_msg {
                Some(ao_msg) if pares.decidir(&pid, "msg", "*") == "aceitar" => {
                    let txt = texto(&c["texto"]);
                    self.log(&pid, "msg_recebida", &corta(&txt, 300), agora);
                    ao_msg(&pid, &corta(&txt, MAX_TEXTO));
                    resp("pong", json!({}))
                }
                _ => resp("tarefa.recusada", json!({ "motivo": "mensagens nao liberadas" })),
            },
            "caps.pedir" => resp("caps", json!({ "agentes": (g.agentes)().into_iter().take(50).collect::<Vec<_>>() })),
            "tarefa.pedido" => {
                let (agente, pedido) = (corta(&texto(&c["agente"]), 60), corta(&texto(&c["pedido"]), MAX_TEXTO));
                let nivel = pares.decidir(&pid, "tarefa", &agente);
                self.log(&pid, &format!("tarefa_pedido:{nivel}"), &format!("{agente}: {}", corta(&pedido, 60)), agora);
                if nivel == "recusar" || g.executor.is_none() {
                    return resp("tarefa.recusada", json!({ "motivo": "sem permissao para este agente" }));
                }
                if nivel == "perguntar" {
                    if let Err(e) = self.add_pend(json!({ "id": env["id"], "par": pid, "agente": agente, "pedido": pedido, "ts": agora as i64 })) {
                        self.log(&pid, "erro", &e, agora);
                        return resp("tarefa.recusada", json!({ "motivo": "nao consegui guardar o pedido" }));
                    }
                    return resp("tarefa.pendente", json!({ "motivo": "aguardando o dono aprovar" }));
                }
                resp("tarefa.resultado", json!({ "ok": true, "texto": executar(g, &pid, &agente, &pedido) }))
            }
            "arquivo.oferta" => {
                if pares.decidir(&pid, "arquivo", "*") != "aceitar" {
                    self.log(&pid, "arquivo_oferta_recusada", &texto(&c["nome"]), agora);
                    return resp("arquivo.recusa", json!({ "motivo": "o dono nao liberou arquivos deste par" }));
                }
                match Recebimento::new(&self.pasta.join("recebidos").join(&pid), c) {
                    Err(e) => {
                        self.log(&pid, "rejeitado", &e, agora);
                        None
                    }
                    Ok(rec) => {
                        self.log(&pid, "arquivo_aceito", &format!("{} {}", rec.nome, rec.tam), agora);
                        let r = resp("arquivo.aceita", json!({ "fid": rec.fid, "faltando": rec.faltando() }));
                        sess.receb.insert(rec.fid.clone(), rec);
                        r
                    }
                }
            }
            "arquivo.fim" => {
                let Some(rec) = sess.receb.get_mut(&texto(&c["fid"])) else { return resp("arquivo.falta", json!({ "fid": c["fid"], "faltando": [] })) };
                let falta = rec.faltando();
                if !falta.is_empty() {
                    return resp("arquivo.falta", json!({ "fid": rec.fid, "faltando": falta.into_iter().take(5000).collect::<Vec<_>>() }));
                }
                let fid = rec.fid.clone();
                match rec.finalizar() {
                    Ok(destino) => {
                        self.log(&pid, "arquivo_recebido", &destino.to_string_lossy(), agora);
                        resp("arquivo.ok", json!({ "fid": fid, "ok": true }))
                    }
                    Err(e) => resp("arquivo.ok", json!({ "fid": fid, "ok": false, "motivo": e })),
                }
            }
            _ => None,
        }
    }
}

fn executar(g: &Ganchos, par: &str, agente: &str, pedido: &str) -> String {
    match g.executor.map(|f| f(agente, pedido, par)) {
        Some(Ok(t)) => corta(&t, MAX_TEXTO),
        Some(Err(e)) => format!("Nao consegui atender: {}", corta(&e, 100)),
        None => "Nao consegui atender: sem executor".into(),
    }
}

// ---------- lado de quem pede: como ler a resposta do par ----------

/// Resposta a um "msg": so "pong" e entrega.
pub fn leitura_msg(r: &Value) -> Result<(), String> {
    if r["tipo"] == "pong" { Ok(()) } else { Err(r["corpo"]["motivo"].as_str().unwrap_or("recusado").to_string()) }
}

pub fn leitura_caps(r: &Value) -> Vec<Value> {
    if r["tipo"] == "caps" { r["corpo"]["agentes"].as_array().cloned().unwrap_or_default() } else { Vec::new() }
}

/// (estado, texto): "resultado" | "pendente" | "recusada".
pub fn leitura_tarefa(r: &Value) -> (String, String) {
    let c = &r["corpo"];
    let txt = [&c["texto"], &c["motivo"]].into_iter().filter_map(|v| v.as_str()).find(|s| !s.is_empty()).unwrap_or("");
    (r["tipo"].as_str().and_then(|t| t.split('.').nth(1)).unwrap_or("").to_string(), txt.to_string())
}

/// Envia um arquivo: oferta, os blocos que faltam, "fim"; repete o que faltou ate 4 vezes. `pedir(tipo, corpo)` devolve o envelope de
/// resposta; `mandar` envia um quadro binario pelo canal DIRETO (pelo plano B arquivo nao passa).
pub fn enviar_arquivo(envio: &Envio, pedir: &dyn Fn(&str, Value) -> Result<Value, String>, mandar: &dyn Fn(Vec<u8>) -> Result<(), String>) -> Result<(), String> {
    let r = pedir("arquivo.oferta", envio.oferta())?;
    if r["tipo"] != "arquivo.aceita" {
        return Err(r["corpo"]["motivo"].as_str().unwrap_or("recusado").to_string());
    }
    let mut falta = r["corpo"]["faltando"].as_array().cloned().unwrap_or_default();
    for _ in 0..4 {
        for i in &falta {
            mandar(envio.quadro(i.as_u64().and_then(|x| u32::try_from(x).ok()).ok_or("indice invalido na resposta")?)?)?;
        }
        let r = pedir("arquivo.fim", json!({ "fid": envio.fid }))?;
        if r["tipo"] == "arquivo.ok" {
            return if r["corpo"]["ok"] == true { Ok(()) } else { Err(r["corpo"]["motivo"].as_str().unwrap_or("o outro lado nao validou o arquivo").to_string()) };
        }
        falta = r["corpo"]["faltando"].as_array().cloned().unwrap_or_default();
    }
    Err("arquivo nao completou apos 4 tentativas".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link_identidade::{falso, Ident};
    use std::cell::RefCell;

    struct Mundo {
        dir: PathBuf,
        eu: Ident,
        ele: Ident,
        pares: Pares,
        no: No,
    }

    fn mundo(nome: &str, estado: &str) -> Mundo {
        let dir = std::env::temp_dir().join(format!("braniac-link_no-{}-{nome}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let (eu, ele) = (falso::ident("Eu"), falso::ident("Bia"));
        let pares = Pares::new(&dir).unwrap();
        pares.salvar_par(&ele.id, "Bia", &ele.chave_pub, "c2VncmVkbw", 100).unwrap();
        if estado == "confiavel" {
            pares.confirmar(&ele.id).unwrap();
        }
        let no = No::new(&eu.id, "Eu", &dir.join("no")).unwrap();
        Mundo { dir, eu, ele, pares, no }
    }

    fn env(m: &Mundo, tipo: &str, corpo: Value) -> String {
        protocolo::criar(&m.ele, &|d| falso::assinar(&m.ele, d), tipo, Some(corpo), &m.eu.id, "", 1000).unwrap()
    }

    fn sem_ganchos<'a>(agentes: &'a dyn Fn() -> Vec<Value>) -> Ganchos<'a> {
        Ganchos { executor: None, agentes, ao_msg: None }
    }

    fn resposta(e: Entrada) -> Resposta {
        match e {
            Entrada::Responder(r) => r,
            outro => panic!("esperava resposta, veio {outro:?}"),
        }
    }

    #[test]
    fn topico_sdp_e_hello() {
        assert!(topico("seg", "bl1-x").starts_with("blink-") && topico("seg", "bl1-x").len() == 38 && topico("seg", "a") != topico("seg", "b"));
        let f = (0..32).map(|i| format!("{i:02x}")).collect::<Vec<_>>().join(":");
        let sdp = format!("v=0\r\na=fingerprint:sha-256 {f}\r\na=candidate:1 1 udp 2 192.168.56.1 5000 typ host\r\na=candidate:2 1 udp 2 192.168.0.7 5000 typ host\r\na=candidate:3 1 udp 2 26.1.2.3 5 typ host\r\na=candidate:4 1 udp 2 FE80::1 5 typ host");
        let limpo = sem_virtuais(&sdp);
        assert!(limpo.contains("192.168.0.7") && !limpo.contains("192.168.56.1") && !limpo.contains("26.1.2.3") && !limpo.contains("FE80"));
        assert_eq!(fp(&sdp), f.to_uppercase());
        let m = mundo("hello", "confiavel");
        let txt = protocolo::criar(&m.ele, &|d| falso::assinar(&m.ele, d), "hello", Some(corpo_hello(&sdp, "s1", "Bia", Some("prova"))), "", "", 1000).unwrap();
        let e: Value = serde_json::from_str(&txt).unwrap();
        assert!(conferir_hello(&e, &sdp, "s1").is_ok());
        assert!(conferir_hello(&e, &sdp, "outra").is_err());
        assert!(conferir_hello(&e, &sdp.replace("00:01", "ff:01"), "s1").is_err()); // SDP trocado no caminho
        assert!(conferir_hello(&e, "v=0", "s1").is_err()); // sem fingerprint nunca passa
        // quem pode conectar
        let par = m.pares.get(&m.ele.id).unwrap();
        // o par guardado tem o segredo proprio
        let seg = par["seg"].as_str().unwrap().to_string();
        assert_eq!(origem_da_oferta(Some(&par), &e, &seg), Ok(Origem::Conhecido));
        assert!(origem_da_oferta(Some(&par), &e, "outro-segredo").is_err());
        assert_eq!(origem_da_oferta(None, &e, "x"), Ok(Origem::ConviteNovo));
        let mut sem_prova = e.clone();
        sem_prova["corpo"]["prova"] = json!("");
        assert!(origem_da_oferta(None, &sem_prova, "x").is_err());
        m.pares.revogar(&m.ele.id).unwrap();
        assert!(origem_da_oferta(m.pares.get(&m.ele.id).as_ref(), &e, &seg).is_err());
        assert!(!segredos(&m.pares, &m.dir, 100).contains(&seg));
        assert!(resposta_serve(&json!({"t": "answer", "sid": "s1"}), "s1", &e, &m.ele.id, &m.ele.chave_pub));
        assert!(!resposta_serve(&json!({"t": "answer", "sid": "s1"}), "s1", &e, &m.ele.id, "outra-chave"));
        let _ = fs::remove_dir_all(&m.dir);
    }

    #[test]
    fn estados_da_conexao_e_limites() {
        assert_eq!(caminho(true, true, 0.0, 10.0), Caminho::Direta);
        assert_eq!(caminho(false, true, 0.0, 1000.0), Caminho::TentarDireta);
        assert_eq!(caminho(false, true, 900.0, 1000.0), Caminho::Relay); // falhou ha menos de 5 min
        assert_eq!(caminho(false, false, 0.0, 1000.0), Caminho::Relay);
        assert!(sessao_acabou("failed") && sessao_acabou("closed") && !sessao_acabou("connected"));
        assert!(pode_enviar_msg(false, 2000.0, 1000.0).is_err() && pode_enviar_msg(true, 2000.0, 1000.0).is_ok() && pode_enviar_msg(false, 0.0, 1000.0).is_ok());
        assert_eq!(bloqueado_ate(86400.0 * 3.0 + 5.0), 86400.0 * 4.0 + 60.0);
        assert!(cota_diaria(429, "daily message quota reached") && !cota_diaria(429, "rate") && !cota_diaria(500, "daily message quota"));
        assert_eq!((espera(6.0, 0.0), espera(6.0, 6.0), espera(6.0, 48.0)), (6.0, 12.0, 60.0));
        assert!(cabe_no_sinal(&"a".repeat(3900)).is_ok() && cabe_no_sinal(&"ã".repeat(2000)).is_err());
        let mut t = Vec::new();
        assert!((0..LIMITE_MIN).all(|i| limite_ok(&mut t, i as f64 * 0.1)));
        assert!(!limite_ok(&mut t, 13.0));
        assert!(limite_ok(&mut t, 200.0)); // passou o minuto
    }

    #[test]
    fn pendente_so_faz_ping_e_desconhecido_e_rejeitado() {
        let m = mundo("pendente", "pendente");
        let ag = || vec![];
        let g = sem_ganchos(&ag);
        let mut s = Sessao::new(&m.ele.id);
        let r = resposta(m.no.receber(&mut s, &m.pares, &env(&m, "ping", json!({})), &falso::verificar, &g, 1000.0));
        assert_eq!((r.tipo, r.corpo["nome"].as_str()), ("pong", Some("Eu")));
        for (t, c) in [("msg", json!({"texto": "oi"})), ("tarefa.pedido", json!({"agente": "conversa", "pedido": "x"})), ("caps.pedir", json!({})), ("arquivo.oferta", json!({}))] {
            assert_eq!(m.no.receber(&mut s, &m.pares, &env(&m, t, c), &falso::verificar, &g, 1000.0), Entrada::Nada, "{t}");
        }
        // envelope de outro remetente nesta sessao, repetido ou para outro destino: nada
        let outro = falso::ident("Caio");
        let de_outro = protocolo::criar(&outro, &|d| falso::assinar(&outro, d), "ping", None, &m.eu.id, "", 1000).unwrap();
        assert_eq!(m.no.receber(&mut s, &m.pares, &de_outro, &falso::verificar, &g, 1000.0), Entrada::Nada);
        let p = env(&m, "ping", json!({}));
        assert!(matches!(m.no.receber(&mut s, &m.pares, &p, &falso::verificar, &g, 1000.0), Entrada::Responder(_)));
        assert_eq!(m.no.receber(&mut s, &m.pares, &p, &falso::verificar, &g, 1000.0), Entrada::Nada);
        let para_outro = protocolo::criar(&m.ele, &|d| falso::assinar(&m.ele, d), "ping", None, "bl1-outro", "", 1000).unwrap();
        assert_eq!(m.no.receber(&mut s, &m.pares, &para_outro, &falso::verificar, &g, 1000.0), Entrada::Nada);
        m.pares.revogar(&m.ele.id).unwrap();
        assert_eq!(m.no.receber(&mut s, &m.pares, &env(&m, "ping", json!({})), &falso::verificar, &g, 1000.0), Entrada::Nada);
        let log = fs::read_to_string(m.dir.join("no/registro.jsonl")).unwrap();
        assert!(log.contains("ignorado_pendente") && log.contains("par revogado") && log.contains("remetente ou destino inesperado"));
        let _ = fs::remove_dir_all(&m.dir);
    }

    #[test]
    fn tarefa_segue_a_permissao_do_dono() {
        let m = mundo("tarefa", "confiavel");
        let rodou = RefCell::new(Vec::new());
        let exec = |agente: &str, pedido: &str, _par: &str| -> Result<String, String> {
            rodou.borrow_mut().push(format!("{agente}:{pedido}"));
            if pedido == "quebra" { Err("falhou".into()) } else { Ok(format!("feito {pedido}")) }
        };
        let ag = || vec![json!({"nome": "conversa"})];
        let recebidas = RefCell::new(Vec::new());
        let ao_msg = |p: &str, t: &str| recebidas.borrow_mut().push(format!("{p}|{t}"));
        let g = Ganchos { executor: Some(&exec), agentes: &ag, ao_msg: Some(&ao_msg) };
        let mut s = Sessao::new(&m.ele.id);
        let pede = |s: &mut Sessao, c: Value| resposta(m.no.receber(s, &m.pares, &env(&m, "tarefa.pedido", c), &falso::verificar, &g, 1000.0));
        // padrao do par confiavel para tarefa
        let padrao = m.pares.decidir(&m.ele.id, "tarefa", "conversa");
        m.pares.permitir(&m.ele.id, "conversa", "tarefa", "recusar").unwrap();
        assert_eq!(pede(&mut s, json!({"agente": "conversa", "pedido": "a"})).tipo, "tarefa.recusada");
        assert!(rodou.borrow().is_empty(), "padrao era {padrao}");
        m.pares.permitir(&m.ele.id, "conversa", "tarefa", "perguntar").unwrap();
        let r = pede(&mut s, json!({"agente": "conversa", "pedido": "b"}));
        assert_eq!(r.tipo, "tarefa.pendente");
        assert!(rodou.borrow().is_empty());
        let pend = m.no.pendencias();
        assert_eq!((pend.len(), pend[0]["pedido"].as_str(), pend[0]["id"].as_str()), (1, Some("b"), Some(r.ref_.as_str())));
        let (par, resp, txt) = m.no.aprovar(&r.ref_, &g, 1001.0).unwrap();
        assert_eq!((par.as_str(), resp.tipo, resp.ref_.as_str(), txt.as_str()), (m.ele.id.as_str(), "tarefa.resultado", r.ref_.as_str(), "feito b"));
        assert!(m.no.pendencias().is_empty() && m.no.aprovar(&r.ref_, &g, 1001.0).is_err());
        m.pares.permitir(&m.ele.id, "conversa", "tarefa", "aceitar").unwrap();
        assert_eq!(pede(&mut s, json!({"agente": "conversa", "pedido": "c"})).corpo["texto"], "feito c");
        assert!(pede(&mut s, json!({"agente": "conversa", "pedido": "quebra"})).corpo["texto"].as_str().unwrap().starts_with("Nao consegui atender"));
        // sem executor, mesmo "aceitar" recusa
        let g2 = Ganchos { executor: None, agentes: &ag, ao_msg: None };
        assert_eq!(resposta(m.no.receber(&mut s, &m.pares, &env(&m, "tarefa.pedido", json!({"agente": "conversa", "pedido": "d"})), &falso::verificar, &g2, 1000.0)).tipo, "tarefa.recusada");
        // mensagem: so com permissao e gancho; caps lista os agentes
        m.pares.permitir(&m.ele.id, "*", "msg", "aceitar").unwrap();
        assert_eq!(resposta(m.no.receber(&mut s, &m.pares, &env(&m, "msg", json!({"texto": "ola"})), &falso::verificar, &g, 1000.0)).tipo, "pong");
        assert_eq!(*recebidas.borrow(), vec![format!("{}|ola", m.ele.id)]);
        assert_eq!(resposta(m.no.receber(&mut s, &m.pares, &env(&m, "msg", json!({"texto": "ola"})), &falso::verificar, &g2, 1000.0)).tipo, "tarefa.recusada");
        m.pares.permitir(&m.ele.id, "*", "msg", "recusar").unwrap();
        assert_eq!(resposta(m.no.receber(&mut s, &m.pares, &env(&m, "msg", json!({"texto": "de novo"})), &falso::verificar, &g, 1000.0)).tipo, "tarefa.recusada");
        assert_eq!(recebidas.borrow().len(), 1);
        assert_eq!(resposta(m.no.receber(&mut s, &m.pares, &env(&m, "caps.pedir", json!({})), &falso::verificar, &g, 1000.0)).corpo["agentes"][0]["nome"], "conversa");
        // resposta a pedido meu volta como RespostaDe e nao e tratada como pedido
        s.pend.insert("meu-pedido".into());
        let volta = protocolo::criar(&m.ele, &|d| falso::assinar(&m.ele, d), "tarefa.resultado", Some(json!({"ok": true, "texto": "pronto"})), &m.eu.id, "meu-pedido", 1000).unwrap();
        let Entrada::RespostaDe(e) = m.no.receber(&mut s, &m.pares, &volta, &falso::verificar, &g, 1000.0) else { panic!("devia casar com o pedido") };
        assert_eq!(leitura_tarefa(&e), ("resultado".to_string(), "pronto".to_string()));
        assert!(leitura_msg(&e).is_err() && leitura_msg(&json!({"tipo": "pong"})).is_ok() && leitura_caps(&e).is_empty());
        let _ = fs::remove_dir_all(&m.dir);
    }

    #[test]
    fn arquivo_so_entra_liberado_e_completa_com_retomada() {
        let m = mundo("arquivo", "confiavel");
        let ag = || vec![];
        let g = sem_ganchos(&ag);
        let s = RefCell::new(Sessao::new(&m.ele.id));
        let origem = m.dir.join("relatorio.txt");
        fs::write(&origem, vec![7u8; 3000]).unwrap();
        let envio = Envio::new(&origem, 1024, "00000000000000aa").unwrap();
        let pedir = |tipo: &str, corpo: Value| -> Result<Value, String> {
            let r = resposta(m.no.receber(&mut s.borrow_mut(), &m.pares, &env(&m, tipo, corpo), &falso::verificar, &g, 1000.0));
            Ok(json!({ "tipo": r.tipo, "corpo": r.corpo }))
        };
        let perdeu = RefCell::new(true);
        let mandar = |q: Vec<u8>| -> Result<(), String> {
            if q[11] == 1 && perdeu.replace(false) {
                return Ok(()); // o bloco 1 se perde na primeira vez
            }
            m.no.quadro(&mut s.borrow_mut(), &q, 1000.0);
            Ok(())
        };
        m.pares.permitir(&m.ele.id, "*", "arquivo", "recusar").unwrap();
        assert!(enviar_arquivo(&envio, &pedir, &mandar).unwrap_err().contains("nao liberou arquivos"));
        m.pares.permitir(&m.ele.id, "*", "arquivo", "aceitar").unwrap();
        enviar_arquivo(&envio, &pedir, &mandar).unwrap();
        assert_eq!(fs::read(m.dir.join("no/recebidos").join(&m.ele.id).join("relatorio.txt")).unwrap(), vec![7u8; 3000]);
        // "fim" de arquivo que nao foi oferecido
        assert_eq!(pedir("arquivo.fim", json!({"fid": "ffffffffffffffff"})).unwrap()["tipo"], "arquivo.falta");
        let _ = fs::remove_dir_all(&m.dir);
    }
}
