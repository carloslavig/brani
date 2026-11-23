//! Brain Center: memoria central que se consolida so quando a MAIORIA dos agentes esta ociosa (dados/braincenter.db, mesmo formato do Python).
//! Partes: memoria de trabalho, episodica, semantica, associacao, consolidacao e recuperacao.
//! Travas: % de ociosos entre os DISPONIVEIS sustentada por `sustentado_s` (indisponivel e "sem dado" nao contam como ocioso); CPU/RAM/GPU
//! sao a segunda trava; um ciclo por vez por pasta de dados; um lote = uma transacao; checkpoint no banco (pausa e retoma).
//! NADA e apagado: episodio original fica, memoria velha vira "arquivada", contradicao fica aberta ate uma pessoa resolver; memoria privada
//! so volta para o dono; associacao e consulta nunca cruzam projetos. Nao chama agente, nao envia mensagem, nao usa internet.
//! Ficou de fora: `ingerir` (trazer os fatos do banco unificado; quem chama usa `registrar`), a thread do tick (quem chama decide) e a
//! leitura termica (parametro). Diferencas do Python: o estado da maquina (`Estado`) e passado por parametro; a `origem` padrao do episodio
//! e um hash FNV (la, sha1); "ja rodando" nao apaga a marca do ciclo que esta rodando (la o `finally` apagava).
#![allow(dead_code)]
use crate::empresa_dev::{corta, ler, rx};
use crate::texto_pt::sem_acento;
use regex::Regex;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub const ESTADOS: [&str; 8] = ["MONITORANDO", "AGUARDANDO OCIOSIDADE", "OCIOSIDADE COLETIVA ATINGIDA", "CONSOLIDANDO", "PAUSADO POR DEMANDA", "PAUSADO POR RECURSOS", "CONCLUÍDO", "ERRO"];
const PARADAS: &str = " de da do das dos e o a os as um uma que para por com em no na nos nas se ao the is of to and ";
const NEGA: [&str; 4] = ["nao", "nunca", "sem", "jamais"];
const COLS: &str = "id,projeto,assunto,chave,texto,tipo,confianca,importancia,estado,privado,dono,agente_origem,uso,n_fontes,atualizado,ultimo_uso,score,norm";

/// Estado da maquina de disponibilidade coletiva (no Python, um dicionario global).
#[derive(Clone, Debug)]
pub struct Estado {
    pub desde: Option<f64>,
    pub estado: String,
    pub motivo: String,
    pub medicao: Value,
    pub recursos: Value,
}

impl Default for Estado {
    fn default() -> Self {
        Estado { desde: None, estado: "MONITORANDO".into(), motivo: "ainda não avaliado".into(), medicao: Value::Null, recursos: Value::Null }
    }
}

pub fn padrao() -> Value {
    json!({
        "limite_ociosidade_pct": 85.0, "sustentado_s": 300,
        "ignorar_agentes": ["BrainCenter", "Brain Center"],
        "estados_trabalhando": ["analisando", "planejando", "executando", "consultando"],
        "estados_ociosos": ["ocioso", "concluido", "aguardando_autorizacao", "aguardando_servico"],
        "estados_indisponiveis": ["desconectado", "erro"],
        "cpu_max_pct": 70, "ram_max_pct": 85, "gpu_max_c": 80,
        "lote": 200, "max_lotes_por_tick": 3,
        "similaridade_duplicata": 0.85, "similaridade_conflito": 0.6,
        "arquivar_apos_dias": 90, "arquivar_score_abaixo": 0.25, "importante_a_partir_de": 0.8, "meia_vida_dias": 30,
        "pesos": { "frequencia": 0.2, "recencia": 0.2, "projeto_ativo": 0.15, "importancia": 0.25, "confianca": 0.1, "repeticao": 0.1 },
        "confianca_fonte": { "dono": 1.0, "dono": 1.0, "dossie": 0.9, "usuario": 1.0, "jarvis": 0.6, "agente": 0.6, "ia": 0.5, "desconhecida": 0.4 },
        "projetos_ativos": ["global"],
        "permissoes": { "*": { "projetos": ["global"] } },
    })
}

/// dados/braincenter/config.json sobrepoe o padrao (so chaves conhecidas; `pesos` e mesclado).
pub fn config(dados: &Path) -> Value {
    let mut c = padrao();
    let extra = ler(&dados.join("braincenter").join("config.json")).unwrap_or(Value::Null);
    for (k, v) in extra.as_object().into_iter().flatten() {
        if k == "pesos" {
            for (p, x) in v.as_object().into_iter().flatten() {
                c["pesos"][p] = x.clone();
            }
        } else if !c[k].is_null() {
            c[k] = v.clone();
        }
    }
    c
}

fn num(cfg: &Value, k: &str) -> f64 {
    cfg[k].as_f64().unwrap_or(0.0)
}

fn tem(cfg: &Value, k: &str, x: &str) -> bool {
    cfg[k].as_array().is_some_and(|a| a.iter().any(|v| v == x))
}

fn norm(x: &str) -> String {
    sem_acento(x).chars().map(|c| if c.is_ascii_lowercase() || c.is_ascii_digit() { c } else { ' ' }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

fn pal(x: &str) -> BTreeSet<String> {
    norm(x).split(' ').filter(|w| (w.len() > 2 || (!w.is_empty() && w.bytes().all(|b| b.is_ascii_digit()))) && !PARADAS.contains(&format!(" {w} "))).map(String::from).collect()
}

fn jac(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
    if a.is_empty() || b.is_empty() { 0.0 } else { a.intersection(b).count() as f64 / a.union(b).count() as f64 }
}

fn nega(norm: &str) -> bool {
    norm.split(' ').any(|w| NEGA.contains(&w))
}

fn e(x: impl ToString) -> String {
    x.to_string()
}

// ---------- banco ----------
fn db(dados: &Path) -> Result<Connection, String> {
    std::fs::create_dir_all(dados).map_err(e)?;
    let c = Connection::open(dados.join("braincenter.db")).map_err(e)?;
    c.busy_timeout(std::time::Duration::from_secs(15)).map_err(e)?;
    c.execute_batch(
        "PRAGMA journal_mode=WAL;
        CREATE TABLE IF NOT EXISTS trabalho(id INTEGER PRIMARY KEY, agente TEXT, contexto TEXT, conteudo TEXT, ts REAL, expira REAL, UNIQUE(agente,contexto));
        CREATE TABLE IF NOT EXISTS episodica(id INTEGER PRIMARY KEY, origem TEXT UNIQUE, ts REAL, agente TEXT, projeto TEXT DEFAULT 'global', assunto TEXT DEFAULT 'geral',
            tipo TEXT DEFAULT 'evento', chave TEXT, texto TEXT, privado INTEGER DEFAULT 0, importancia REAL DEFAULT 0.5, fonte TEXT DEFAULT 'agente', consolidado INTEGER DEFAULT 0);
        CREATE INDEX IF NOT EXISTS ep_cons ON episodica(consolidado,id);
        CREATE TABLE IF NOT EXISTS semantica(id INTEGER PRIMARY KEY, projeto TEXT, assunto TEXT, chave TEXT, texto TEXT, tipo TEXT DEFAULT 'fato', confianca REAL, importancia REAL DEFAULT 0.5,
            estado TEXT DEFAULT 'ativa', privado INTEGER DEFAULT 0, dono TEXT, agente_origem TEXT, uso INTEGER DEFAULT 0, n_fontes INTEGER DEFAULT 1, criado REAL, atualizado REAL, ultimo_uso REAL,
            score REAL DEFAULT 0, norm TEXT);
        CREATE INDEX IF NOT EXISTS se_chave ON semantica(projeto,assunto,chave);
        CREATE TABLE IF NOT EXISTS historico(id INTEGER PRIMARY KEY, sem_id INTEGER, texto_anterior TEXT, ts REAL, motivo TEXT);
        CREATE TABLE IF NOT EXISTS origem(sem_id INTEGER, ep_id INTEGER, UNIQUE(sem_id,ep_id));
        CREATE TABLE IF NOT EXISTS conflitos(id INTEGER PRIMARY KEY, sem_a INTEGER, sem_b INTEGER, motivo TEXT, aberto INTEGER DEFAULT 1, ts REAL, UNIQUE(sem_a,sem_b));
        CREATE TABLE IF NOT EXISTS associacoes(a INTEGER, b INTEGER, peso REAL, UNIQUE(a,b));
        CREATE TABLE IF NOT EXISTS indice(palavra TEXT, sem_id INTEGER, UNIQUE(palavra,sem_id));
        CREATE INDEX IF NOT EXISTS ix_p ON indice(palavra);
        CREATE TABLE IF NOT EXISTS ciclos(id INTEGER PRIMARY KEY, inicio REAL, fim REAL, estado TEXT, lotes INTEGER DEFAULT 0, novos INTEGER DEFAULT 0, criadas INTEGER DEFAULT 0,
            atualizadas INTEGER DEFAULT 0, duplicadas INTEGER DEFAULT 0, conflitos INTEGER DEFAULT 0, arquivadas INTEGER DEFAULT 0, erro TEXT, duracao_s REAL DEFAULT 0);
        CREATE TABLE IF NOT EXISTS meta(k TEXT PRIMARY KEY, v TEXT);",
    ).map_err(e)?;
    Ok(c)
}

fn meta(c: &Connection, k: &str) -> String {
    c.query_row("SELECT v FROM meta WHERE k=?", [k], |r| r.get::<_, Option<String>>(0)).ok().flatten().unwrap_or_default()
}

fn setmeta(c: &Connection, k: &str, v: &str) -> rusqlite::Result<usize> {
    c.execute("INSERT INTO meta(k,v) VALUES(?,?) ON CONFLICT(k) DO UPDATE SET v=excluded.v", [k, v])
}

fn conta(c: &Connection, sql: &str) -> i64 {
    c.query_row(sql, [], |r| r.get(0)).unwrap_or(0)
}

// ---------- memoria de trabalho ----------
pub fn trabalho_set(dados: &Path, agente: &str, contexto: &str, conteudo: &str, ttl_s: f64, ts: f64) -> Result<(), String> {
    db(dados)?.execute("INSERT INTO trabalho(agente,contexto,conteudo,ts,expira) VALUES(?,?,?,?,?) ON CONFLICT(agente,contexto) DO UPDATE SET conteudo=excluded.conteudo,ts=excluded.ts,expira=excluded.expira",
                       params![agente, contexto, corta(conteudo, 4000), ts, ts + ttl_s]).map(|_| ()).map_err(e)
}

pub fn trabalho_get(dados: &Path, agente: &str, contexto: &str, ts: f64) -> Option<String> {
    db(dados).ok()?.query_row("SELECT conteudo FROM trabalho WHERE agente=? AND contexto=? AND expira>?", params![agente, contexto, ts], |r| r.get(0)).ok()
}

// ---------- memoria episodica ----------
pub struct Ep<'a> {
    pub agente: &'a str,
    pub texto: &'a str,
    pub projeto: &'a str,
    pub assunto: &'a str,
    pub tipo: &'a str,
    pub chave: Option<&'a str>,
    pub privado: bool,
    pub importancia: f64,
    pub fonte: &'a str,
    /// Unica: evita duplicar reenvios. None = hash do conteudo.
    pub origem: Option<&'a str>,
}

impl<'a> Ep<'a> {
    pub fn novo(agente: &'a str, texto: &'a str) -> Self {
        Ep { agente, texto, projeto: "global", assunto: "geral", tipo: "evento", chave: None, privado: false, importancia: 0.5, fonte: "agente", origem: None }
    }
}

fn fnv(s: &str, semente: u64) -> u64 {
    s.bytes().fold(semente, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

/// Grava um episodio (nunca e apagado nem alterado). false = vazio, repetido ou banco fora do ar.
pub fn registrar(dados: &Path, ep: &Ep, ts: f64) -> bool {
    let texto = ep.texto.trim();
    if texto.is_empty() {
        return false;
    }
    let base = format!("{}|{}|{}|{}|{texto}", ep.agente, ep.projeto, ep.assunto, ep.chave.unwrap_or("None"));
    let origem = ep.origem.map(String::from).unwrap_or_else(|| format!("{:016x}{:016x}", fnv(&base, 0xcbf29ce484222325), fnv(&base, 0x84222325cbf29ce4)));
    let Ok(c) = db(dados) else { return false };
    c.execute("INSERT OR IGNORE INTO episodica(origem,ts,agente,projeto,assunto,tipo,chave,texto,privado,importancia,fonte) VALUES(?,?,?,?,?,?,?,?,?,?,?)",
              params![origem, ts, ep.agente, ep.projeto, ep.assunto, ep.tipo, ep.chave, corta(texto, 2000), ep.privado as i64, ep.importancia, ep.fonte]).is_ok_and(|n| n == 1)
}

// ---------- disponibilidade coletiva ----------
/// Funcao pura. agentes = [{"nome", "estado"}]. Indisponivel (desconectado, erro, sem dado, estado desconhecido) fica fora e nao conta como ocioso.
pub fn medir(agentes: &[Value], cfg: &Value) -> Value {
    let ign: HashSet<String> = cfg["ignorar_agentes"].as_array().into_iter().flatten().filter_map(Value::as_str).map(norm).collect();
    let (mut tr, mut oc, mut ind) = (0i64, 0i64, 0i64);
    for a in agentes {
        if ign.contains(&norm(a["nome"].as_str().unwrap_or(""))) {
            continue;
        }
        match a["estado"].as_str() {
            Some(x) if tem(cfg, "estados_trabalhando", x) => tr += 1,
            Some(x) if tem(cfg, "estados_ociosos", x) => oc += 1,
            _ => ind += 1,
        }
    }
    let disp = tr + oc;
    let pct = if disp > 0 { (1000.0 * oc as f64 / disp as f64).round() / 10.0 } else { 0.0 };
    json!({ "total": tr + oc + ind, "disponiveis": disp, "trabalhando": tr, "ociosos": oc, "indisponiveis": ind, "pct_ociosos": pct })
}

/// (ok, motivo, dados). Sem leitura disponivel nao bloqueia, mas diz isso.
pub fn recursos_ok(cfg: &Value, leitura: &Value) -> (bool, String, Value) {
    if leitura.as_object().is_none_or(|o| o.is_empty()) || leitura["pronto"] == false {
        return (true, "sem leitura de recursos (liberado)".into(), json!({}));
    }
    let d = json!({ "cpu_pct": leitura["cpu_pct"], "ram_pct": leitura["ram_pct"], "gpu_c": leitura["gpu_c"] });
    for (k, max, txt) in [("cpu_pct", "cpu_max_pct", "CPU em"), ("ram_pct", "ram_max_pct", "RAM em"), ("gpu_c", "gpu_max_c", "GPU a")] {
        if d[k].as_f64().unwrap_or(0.0) > num(cfg, max) {
            let un = if k == "gpu_c" { " °C" } else { "%" };
            return (false, format!("{txt} {}{un} (máx {}{un})", d[k], cfg[max]), d);
        }
    }
    (true, "recursos dentro dos limites".into(), d)
}

/// Maquina de estados da disponibilidade coletiva: (autorizado, estado, motivo, medicao).
pub fn avaliar(est: &mut Estado, agentes: &[Value], ts: f64, cfg: &Value, leitura: &Value, em_ciclo: bool) -> (bool, &'static str, String, Value) {
    let m = medir(agentes, cfg);
    let (rok, rmot, rdados) = recursos_ok(cfg, leitura);
    est.medicao = m.clone();
    est.recursos = rdados;
    let (pct, disp) = (m["pct_ociosos"].as_f64().unwrap_or(0.0), m["disponiveis"].as_i64().unwrap_or(0));
    let (ok, nome, mot) = if !(disp > 0 && pct >= num(cfg, "limite_ociosidade_pct")) {
        est.desde = None;
        (false, if em_ciclo { "PAUSADO POR DEMANDA" } else { "AGUARDANDO OCIOSIDADE" }, format!("{pct:.1}% ociosos de {disp} disponíveis (precisa {}%)", cfg["limite_ociosidade_pct"]))
    } else {
        let falta = num(cfg, "sustentado_s") - (ts - *est.desde.get_or_insert(ts));
        if falta > 0.0 && !em_ciclo {
            (false, "OCIOSIDADE COLETIVA ATINGIDA", format!("{pct:.1}% ociosos; aguardando {} s sustentados", falta as i64))
        } else if !rok {
            (false, "PAUSADO POR RECURSOS", rmot)
        } else {
            (true, "CONSOLIDANDO", format!("{pct:.1}% ociosos e {rmot}"))
        }
    };
    est.estado = nome.into();
    est.motivo = mot.clone();
    (ok, nome, mot, m)
}

// ---------- consolidacao ----------
fn tipo(texto: &str) -> &'static str {
    static T: OnceLock<Regex> = OnceLock::new();
    static H: OnceLock<Regex> = OnceLock::new();
    static O: OnceLock<Regex> = OnceLock::new();
    let t = norm(texto);
    if rx(&T, r"\b(so hoje|hoje|agora|temporari\w*|neste momento|por enquanto|amanha)\b").is_match(&t) {
        "temporario"
    } else if rx(&H, r"\b(acho|talvez|provavelmente|parece|suponho|possivelmente|hipotese)\b").is_match(&t) {
        "hipotese"
    } else if rx(&O, r"\b(prefiro|gosto|na minha opiniao|opiniao|detesto|adoro)\b").is_match(&t) {
        "opiniao"
    } else {
        "fato"
    }
}

struct Mem {
    id: i64,
    projeto: String,
    assunto: String,
    chave: Option<String>,
    texto: String,
    tipo: String,
    confianca: f64,
    importancia: f64,
    estado: String,
    privado: bool,
    dono: Option<String>,
    agente_origem: String,
    uso: i64,
    n_fontes: i64,
    atualizado: f64,
    ultimo_uso: f64,
    score: f64,
    norm: String,
}

fn mem(r: &Row) -> rusqlite::Result<Mem> {
    let t = |i: usize| r.get::<_, Option<String>>(i).map(Option::unwrap_or_default);
    let f = |i: usize| r.get::<_, Option<f64>>(i).map(Option::unwrap_or_default);
    Ok(Mem { id: r.get(0)?, projeto: t(1)?, assunto: t(2)?, chave: r.get(3)?, texto: t(4)?, tipo: t(5)?, confianca: f(6)?, importancia: f(7)?, estado: t(8)?,
             privado: r.get::<_, Option<i64>>(9)?.unwrap_or(0) != 0, dono: r.get(10)?, agente_origem: t(11)?, uso: r.get::<_, Option<i64>>(12)?.unwrap_or(0),
             n_fontes: r.get::<_, Option<i64>>(13)?.unwrap_or(1), atualizado: f(14)?, ultimo_uso: f(15)?, score: f(16)?, norm: t(17)? })
}

fn mems(c: &Connection, onde: &str, p: &[&dyn rusqlite::ToSql]) -> rusqlite::Result<Vec<Mem>> {
    let mut st = c.prepare(&format!("SELECT {COLS} FROM semantica WHERE {onde}"))?;
    let v = st.query_map(p, mem)?.collect();
    v
}

/// Score 0..1 (nao e verdade: confianca vem da FONTE). Uso e repeticao so contam com teto.
fn pontuar(m: &Mem, ts: f64, cfg: &Value) -> f64 {
    let p = &cfg["pesos"];
    let ref_ts = [m.ultimo_uso, m.atualizado].into_iter().find(|x| *x != 0.0).unwrap_or(ts);
    let dias = ((ts - ref_ts) / 86400.0).max(0.0);
    let rec = (-std::f64::consts::LN_2 * dias / num(cfg, "meia_vida_dias")).exp();
    let freq = ((m.uso as f64).ln_1p() / 11f64.ln()).min(1.0);
    let rep = ((m.n_fontes.max(1) - 1) as f64 / 4.0).min(1.0);
    let proj = if tem(cfg, "projetos_ativos", &m.projeto) { 1.0 } else { 0.0 };
    let s = num(p, "frequencia") * freq + num(p, "recencia") * rec + num(p, "projeto_ativo") * proj + num(p, "importancia") * m.importancia + num(p, "confianca") * m.confianca + num(p, "repeticao") * rep;
    let total: f64 = p.as_object().map_or(1.0, |o| o.values().filter_map(Value::as_f64).sum());
    (s / total * 10000.0).round() / 10000.0
}

fn indexar(c: &Connection, sid: i64, texto: &str) -> rusqlite::Result<()> {
    c.execute("DELETE FROM indice WHERE sem_id=?", [sid])?;
    for w in pal(texto) {
        c.execute("INSERT OR IGNORE INTO indice(palavra,sem_id) VALUES(?,?)", params![w, sid])?;
    }
    Ok(())
}

fn candidatos(c: &Connection, palavras: &BTreeSet<String>) -> rusqlite::Result<Vec<(i64, i64)>> {
    let mut cand: Vec<(i64, i64)> = Vec::new();
    let mut st = c.prepare("SELECT sem_id FROM indice WHERE palavra=?")?;
    for w in palavras {
        for sid in st.query_map([w], |r| r.get::<_, i64>(0))?.flatten() {
            match cand.iter_mut().find(|(i, _)| *i == sid) {
                Some(x) => x.1 += 1,
                None => cand.push((sid, 1)),
            }
        }
    }
    Ok(cand)
}

/// Liga a memoria as 5 mais parecidas do MESMO projeto (associacao nunca cruza projetos).
fn associar(c: &Connection, sid: i64, projeto: &str, texto: &str) -> rusqlite::Result<()> {
    let p = pal(texto);
    let mut pontos: Vec<(f64, i64)> = Vec::new();
    for (o, _) in candidatos(c, &p)?.into_iter().filter(|(o, _)| *o != sid) {
        let r: Option<(String, String)> = c.query_row("SELECT texto,projeto FROM semantica WHERE id=? AND estado!='arquivada'", [o], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
        if let Some((t, pj)) = r {
            let j = jac(&p, &pal(&t));
            if pj == projeto && j >= 0.25 {
                pontos.push((j, o));
            }
        }
    }
    pontos.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    for (j, o) in pontos.into_iter().take(5) {
        c.execute("INSERT INTO associacoes(a,b,peso) VALUES(?,?,?) ON CONFLICT(a,b) DO UPDATE SET peso=excluded.peso", params![sid.min(o), sid.max(o), (j * 1000.0).round() / 1000.0])?;
    }
    Ok(())
}

struct Episodio {
    id: i64,
    agente: String,
    projeto: String,
    assunto: String,
    chave: Option<String>,
    texto: String,
    privado: bool,
    importancia: f64,
    fonte: String,
}

#[derive(Default)]
struct Conta {
    criadas: i64,
    atualizadas: i64,
    duplicadas: i64,
    conflitos: i64,
    temporarios: i64,
}

fn criar(c: &Connection, ep: &Episodio, tipo: &str, norm: &str, chave: &Option<String>, conf: f64, dono: Option<&str>, estado: &str, ts: f64) -> rusqlite::Result<i64> {
    c.execute("INSERT INTO semantica(projeto,assunto,chave,texto,tipo,confianca,importancia,estado,privado,dono,agente_origem,criado,atualizado,norm) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
              params![ep.projeto, ep.assunto, chave, ep.texto, tipo, conf, ep.importancia, estado, ep.privado as i64, dono, ep.agente, ts, ts, norm])?;
    let sid = c.last_insert_rowid();
    c.execute("INSERT INTO origem(sem_id,ep_id) VALUES(?,?)", [sid, ep.id])?;
    indexar(c, sid, &ep.texto)?;
    associar(c, sid, &ep.projeto, &ep.texto)?;
    Ok(sid)
}

fn conflito(c: &Connection, ep: &Episodio, m: &Mem, tp: &str, nrm: &str, chave: &Option<String>, conf: f64, dono: Option<&str>, motivo: &str, ts: f64) -> rusqlite::Result<()> {
    let novo = criar(c, ep, tp, nrm, chave, conf, dono, "conflito", ts)?;
    c.execute("UPDATE semantica SET estado='conflito' WHERE id=?", [m.id])?;
    c.execute("INSERT OR IGNORE INTO conflitos(sem_a,sem_b,motivo,ts) VALUES(?,?,?,?)", params![m.id, novo, motivo, ts])?;
    Ok(())
}

fn consolidar_episodio(c: &Connection, ep: &Episodio, cfg: &Value, st: &mut Conta, ts: f64) -> rusqlite::Result<()> {
    let tp = tipo(&ep.texto);
    if tp == "temporario" {
        st.temporarios += 1; // fica so no episodico; nao vira conhecimento estavel
        return Ok(());
    }
    let (nrm, p) = (norm(&ep.texto), pal(&ep.texto));
    let chave = ep.chave.as_deref().map(norm).filter(|k| !k.is_empty());
    let fontes = &cfg["confianca_fonte"];
    let conf = fontes[ep.fonte.as_str()].as_f64().or_else(|| fontes["desconhecida"].as_f64()).unwrap_or(0.4);
    let dono = ep.privado.then_some(ep.agente.as_str());
    let linhas = mems(c, "projeto=? AND assunto=? AND estado!='arquivada' AND IFNULL(dono,'')=IFNULL(?,'')", &[&ep.projeto, &ep.assunto, &dono])?;
    let mesma_chave = |m: &Mem| chave.is_some() && m.chave == chave;
    // 1) duplicata
    if let Some(m) = linhas.iter().find(|m| m.norm == nrm || (!mesma_chave(m) && jac(&p, &pal(&m.texto)) >= num(cfg, "similaridade_duplicata") && nega(&nrm) == nega(&m.norm))) {
        c.execute("INSERT OR IGNORE INTO origem(sem_id,ep_id) VALUES(?,?)", [m.id, ep.id])?;
        let n: i64 = c.query_row("SELECT COUNT(DISTINCT e.agente) FROM origem o JOIN episodica e ON e.id=o.ep_id WHERE o.sem_id=?", [m.id], |r| r.get(0))?;
        c.execute("UPDATE semantica SET n_fontes=?, atualizado=? WHERE id=?", params![n, ts, m.id])?;
        st.duplicadas += 1;
        return Ok(());
    }
    // 2) mesma chave, texto diferente: atualizacao (mesma origem, memoria comum) ou conflito
    if let Some(m) = linhas.iter().find(|m| mesma_chave(m)) {
        if m.agente_origem == ep.agente && m.importancia < num(cfg, "importante_a_partir_de") && m.estado == "ativa" {
            c.execute("INSERT INTO historico(sem_id,texto_anterior,ts,motivo) VALUES(?,?,?,?)", params![m.id, m.texto, ts, format!("atualizado por {}", ep.agente)])?;
            c.execute("UPDATE semantica SET texto=?,norm=?,tipo=?,confianca=?,atualizado=? WHERE id=?", params![ep.texto, nrm, tp, conf.max(m.confianca), ts, m.id])?;
            c.execute("INSERT OR IGNORE INTO origem(sem_id,ep_id) VALUES(?,?)", [m.id, ep.id])?;
            indexar(c, m.id, &ep.texto)?;
            associar(c, m.id, &ep.projeto, &ep.texto)?;
            st.atualizadas += 1;
            return Ok(());
        }
        conflito(c, ep, m, tp, &nrm, &chave, conf, dono, "mesma chave, texto diferente, outra origem ou memória importante", ts)?;
        st.conflitos += 1;
        return Ok(());
    }
    // 3) contradicao por negacao em texto muito parecido
    if let Some(m) = linhas.iter().find(|m| jac(&p, &pal(&m.texto)) >= num(cfg, "similaridade_conflito") && nega(&nrm) != nega(&m.norm)) {
        conflito(c, ep, m, tp, &nrm, &chave, conf, dono, "afirmação e negação sobre o mesmo assunto", ts)?;
        st.conflitos += 1;
        return Ok(());
    }
    criar(c, ep, tp, &nrm, &chave, conf, dono, "ativa", ts)?;
    st.criadas += 1;
    Ok(())
}

/// Memoria velha, pouco pontuada e nao importante vira "arquivada" (continua no banco).
fn arquivar(c: &Connection, cfg: &Value, ts: f64) -> rusqlite::Result<i64> {
    let mut n = 0;
    for m in mems(c, "estado='ativa'", &[])? {
        let s = pontuar(&m, ts, cfg);
        let base = if m.atualizado != 0.0 { m.atualizado } else { ts };
        let arq = (ts - base) / 86400.0 >= num(cfg, "arquivar_apos_dias") && s < num(cfg, "arquivar_score_abaixo") && m.importancia < num(cfg, "importante_a_partir_de");
        c.execute("UPDATE semantica SET score=?, estado=? WHERE id=?", params![s, if arq { "arquivada" } else { "ativa" }, m.id])?;
        n += arq as i64;
    }
    Ok(n)
}

static RODANDO: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

struct Vez(PathBuf);

impl Drop for Vez {
    fn drop(&mut self) {
        if let Ok(mut r) = RODANDO.lock() {
            r.retain(|p| *p != self.0);
        }
    }
}

fn pendentes(c: &Connection, lote: i64) -> rusqlite::Result<Vec<Episodio>> {
    let mut st = c.prepare("SELECT id,agente,projeto,assunto,chave,texto,privado,importancia,fonte FROM episodica WHERE consolidado=0 ORDER BY id LIMIT ?")?;
    let t = |r: &Row, i: usize| r.get::<_, Option<String>>(i).map(Option::unwrap_or_default);
    let v = st.query_map([lote], |r| Ok(Episodio { id: r.get(0)?, agente: t(r, 1)?, projeto: t(r, 2)?, assunto: t(r, 3)?, chave: r.get(4)?, texto: t(r, 5)?,
                                                   privado: r.get::<_, Option<i64>>(6)?.unwrap_or(0) != 0, importancia: r.get::<_, Option<f64>>(7)?.unwrap_or(0.5), fonte: t(r, 8)? }))?.collect();
    v
}

fn ciclo(c: &Connection, cfg: &Value, ts: f64, avaliador: Option<&dyn Fn() -> bool>) -> rusqlite::Result<Value> {
    let aberto = meta(c, "ciclo_aberto").parse::<i64>().ok().filter(|id| c.query_row("SELECT 1 FROM ciclos WHERE id=? AND estado='pausado'", [id], |_| Ok(())).is_ok());
    let rid = match aberto {
        Some(id) => {
            c.execute("UPDATE ciclos SET estado='consolidando' WHERE id=?", [id])?;
            id
        }
        None => {
            c.execute("INSERT INTO ciclos(inicio,estado) VALUES(?,?)", params![ts, "consolidando"])?;
            let id = c.last_insert_rowid();
            setmeta(c, "ciclo_aberto", &id.to_string())?;
            id
        }
    };
    let t0 = std::time::Instant::now();
    let (lote, max) = (cfg["lote"].as_i64().unwrap_or(200), cfg["max_lotes_por_tick"].as_i64().unwrap_or(3));
    let (mut st, mut lotes, mut novos, mut estado) = (Conta::default(), 0i64, 0i64, None);
    while lotes < max {
        // a ociosidade e conferida ENTRE lotes: se cair, para no checkpoint
        if lotes > 0 && avaliador.is_some_and(|a| !a()) {
            estado = Some("pausado");
            break;
        }
        let pend = pendentes(c, lote)?;
        let Some(ultimo) = pend.last().map(|e| e.id) else {
            estado = Some("concluido");
            break;
        };
        let tx = c.unchecked_transaction()?; // 1 transacao por lote: falha = nada do lote e aplicado
        let feito = (|| {
            for ep in &pend {
                consolidar_episodio(&tx, ep, cfg, &mut st, ts)?;
                tx.execute("UPDATE episodica SET consolidado=1 WHERE id=?", [ep.id])?;
            }
            setmeta(&tx, "checkpoint", &ultimo.to_string())
        })();
        feito?; // erro: a transacao e desfeita ao sair
        tx.commit()?;
        lotes += 1;
        novos += pend.len() as i64;
    }
    let estado = estado.unwrap_or_else(|| if conta(c, "SELECT COUNT(*) FROM (SELECT 1 FROM episodica WHERE consolidado=0 LIMIT 1)") > 0 { "pausado" } else { "concluido" });
    let arq = if estado == "concluido" { arquivar(c, cfg, ts)? } else { 0 };
    c.execute("UPDATE ciclos SET fim=?,estado=?,lotes=lotes+?,novos=novos+?,criadas=criadas+?,atualizadas=atualizadas+?,duplicadas=duplicadas+?,conflitos=conflitos+?,arquivadas=arquivadas+?,duracao_s=duracao_s+? WHERE id=?",
              params![ts, estado, lotes, novos, st.criadas, st.atualizadas, st.duplicadas, st.conflitos, arq, (t0.elapsed().as_secs_f64() * 100.0).round() / 100.0, rid])?;
    if estado == "concluido" {
        setmeta(c, "ciclo_aberto", "")?;
    }
    Ok(json!({ "estado": estado, "lotes": lotes, "criadas": st.criadas, "atualizadas": st.atualizadas, "duplicadas": st.duplicadas, "conflitos": st.conflitos,
               "temporarios": st.temporarios, "novos": novos, "arquivadas": arq }))
}

/// Roda ate `max_lotes_por_tick` lotes. `avaliador()` (continua autorizado?) e conferido ENTRE lotes. O proximo ciclo retoma do checkpoint.
/// Nao permite 2 ciclos juntos (nem no mesmo processo, nem em outro: marca `ciclo_ativo` no banco por 10 min).
pub fn consolidar(dados: &Path, est: &mut Estado, cfg: &Value, ts: f64, avaliador: Option<&dyn Fn() -> bool>) -> Value {
    let ja = json!({ "estado": "ja_rodando" });
    let _vez = {
        let Ok(mut r) = RODANDO.lock() else { return ja };
        if r.iter().any(|p| p == dados) {
            return ja;
        }
        r.push(dados.to_path_buf());
        Vez(dados.to_path_buf())
    };
    let falha = |est: &mut Estado, c: Option<&Connection>, x: String| {
        if let Some(c) = c {
            let _ = c.execute("INSERT INTO ciclos(inicio,fim,estado,erro) VALUES(?,?,?,?)", params![ts, ts, "erro", corta(&x, 300)]);
            let _ = setmeta(c, "ciclo_ativo", "");
        }
        est.estado = "ERRO".into();
        est.motivo = corta(&x, 200);
        json!({ "estado": "erro", "erro": corta(&x, 200) })
    };
    let c = match db(dados) {
        Ok(c) => c,
        Err(x) => return falha(est, None, x),
    };
    if meta(&c, "ciclo_ativo").parse::<f64>().is_ok_and(|a| ts - a < 600.0) {
        return ja;
    }
    if let Err(x) = setmeta(&c, "ciclo_ativo", &ts.to_string()) {
        return falha(est, Some(&c), x.to_string());
    }
    match ciclo(&c, cfg, ts, avaliador) {
        Ok(r) => {
            let _ = setmeta(&c, "ciclo_ativo", "");
            est.estado = if r["estado"] == "concluido" { "CONCLUÍDO" } else { "PAUSADO POR DEMANDA" }.into();
            r
        }
        Err(x) => falha(est, Some(&c), x.to_string()),
    }
}

// ---------- recuperacao ----------
pub fn permitido(agente: &str, projeto: &str, cfg: &Value) -> bool {
    let p = &cfg["permissoes"];
    let lista = if p[agente].is_object() { &p[agente]["projetos"] } else { &p["*"]["projetos"] };
    projeto == "global" || lista.as_array().is_some_and(|l| l.iter().any(|x| x == "*" || x == projeto))
}

/// Memorias relevantes. Privada so volta ao dono; projeto sem permissao e recusado; conflito aparece marcado; arquivada perde pontos.
pub fn consultar(dados: &Path, agente: &str, texto: &str, projeto: &str, n: usize, cfg: &Value, ts: f64) -> Vec<Value> {
    let p = pal(texto);
    if !permitido(agente, projeto, cfg) || p.is_empty() {
        return vec![];
    }
    let Ok(c) = db(dados) else { return vec![] };
    let mut out: Vec<(f64, Mem)> = Vec::new();
    for (sid, hit) in candidatos(&c, &p).unwrap_or_default() {
        let Some(m) = mems(&c, "id=?", &[&sid]).ok().and_then(|v| v.into_iter().next()) else { continue };
        if (m.projeto != projeto && m.projeto != "global") || (m.privado && m.dono.as_deref() != Some(agente)) {
            continue;
        }
        let rel = hit as f64 / p.len() as f64;
        out.push((rel * 0.6 + m.score * 0.2 + m.confianca * 0.2 - if m.estado == "arquivada" { 0.3 } else { 0.0 }, m));
    }
    out.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    out.into_iter().take(n).map(|(_, m)| {
        let _ = c.execute("UPDATE semantica SET uso=uso+1, ultimo_uso=? WHERE id=?", params![ts, m.id]);
        json!({ "id": m.id, "texto": m.texto, "tipo": m.tipo, "estado": m.estado, "assunto": m.assunto, "projeto": m.projeto, "confianca": m.confianca,
                "origem": m.agente_origem, "conflito": m.estado == "conflito" })
    }).collect()
}

/// So uma pessoa valida: mantem uma memoria, arquiva a outra (preservada) e fecha o conflito.
pub fn resolver_conflito(dados: &Path, conflito_id: i64, manter: i64, quem: &str, ts: f64) -> bool {
    let Ok(c) = db(dados) else { return false };
    let Ok((a, b)) = c.query_row("SELECT sem_a,sem_b FROM conflitos WHERE id=? AND aberto=1", [conflito_id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))) else { return false };
    if manter != a && manter != b {
        return false;
    }
    let outro = if manter == a { b } else { a };
    let feito = (|| {
        let tx = c.unchecked_transaction()?;
        tx.execute("UPDATE semantica SET estado='ativa' WHERE id=?", [manter])?;
        tx.execute("UPDATE semantica SET estado='arquivada' WHERE id=?", [outro])?;
        tx.execute("INSERT INTO historico(sem_id,texto_anterior,ts,motivo) VALUES(?,?,?,?)", params![outro, "(arquivada ao resolver conflito)", ts, format!("resolvido por {quem}")])?;
        tx.execute("UPDATE conflitos SET aberto=0 WHERE id=?", [conflito_id])?;
        tx.commit()
    })();
    feito.is_ok()
}

// ---------- orquestracao e painel ----------
/// Chamado pela rotina; barato quando nao autorizado (nao toca em agente nenhum). Autorizado, consolida AGORA (quem chama decide a thread).
/// `reavaliar` devolve o retrato novo dos agentes entre os lotes (None = nao deu para ler: para).
pub fn tick(dados: &Path, est: &mut Estado, agentes: &[Value], ts: f64, leitura: &Value, reavaliar: &dyn Fn() -> Option<Vec<Value>>) -> String {
    let cfg = config(dados);
    let (ok, nome, _, _) = avaliar(est, agentes, ts, &cfg, leitura, false);
    if ok {
        let copia = RefCell::new(est.clone());
        let ainda = || reavaliar().is_some_and(|a| avaliar(&mut copia.borrow_mut(), &a, ts, &cfg, leitura, true).0);
        consolidar(dados, est, &cfg, ts, Some(&ainda));
    }
    nome.to_string()
}

pub fn painel(dados: &Path, est: &Estado, ts: f64) -> Result<Value, String> {
    let (cfg, c) = (config(dados), db(dados)?);
    let ult: Option<(Option<f64>, Option<f64>)> = c.query_row("SELECT fim,duracao_s FROM ciclos WHERE estado IN ('concluido','pausado','erro') ORDER BY id DESC LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?))).ok();
    let mut st = c.prepare("SELECT id,inicio,fim,estado,lotes,novos,criadas,atualizadas,duplicadas,conflitos,arquivadas,duracao_s,erro FROM ciclos ORDER BY id DESC LIMIT 10").map_err(e)?;
    let hist: Vec<Value> = st.query_map([], |r| {
        let i = |k: usize| r.get::<_, Option<i64>>(k);
        Ok(json!({ "id": i(0)?, "inicio": r.get::<_, Option<f64>>(1)?, "fim": r.get::<_, Option<f64>>(2)?, "estado": r.get::<_, Option<String>>(3)?, "lotes": i(4)?, "novos": i(5)?, "criadas": i(6)?,
                   "atualizadas": i(7)?, "duplicadas": i(8)?, "conflitos": i(9)?, "arquivadas": i(10)?, "duracao_s": r.get::<_, Option<f64>>(11)?, "erro": r.get::<_, Option<String>>(12)? }))
    }).map_err(e)?.flatten().collect();
    let trabalho: i64 = c.query_row("SELECT COUNT(*) FROM trabalho WHERE expira>?", [ts], |r| r.get(0)).unwrap_or(0);
    Ok(json!({
        "estado": est.estado, "motivo": est.motivo, "medicao": est.medicao, "recursos": est.recursos,
        "limite_pct": cfg["limite_ociosidade_pct"], "sustentado_s": cfg["sustentado_s"],
        "pendentes": conta(&c, "SELECT COUNT(*) FROM episodica WHERE consolidado=0"), "episodios": conta(&c, "SELECT COUNT(*) FROM episodica"),
        "memorias": conta(&c, "SELECT COUNT(*) FROM semantica WHERE estado!='arquivada'"), "arquivadas": conta(&c, "SELECT COUNT(*) FROM semantica WHERE estado='arquivada'"),
        "conflitos_abertos": conta(&c, "SELECT COUNT(*) FROM conflitos WHERE aberto=1"), "associacoes": conta(&c, "SELECT COUNT(*) FROM associacoes"),
        "trabalho": trabalho, "ultima_consolidacao": ult.and_then(|u| u.0), "ultimo_tempo_s": ult.and_then(|u| u.1), "historico": hist,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pasta(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-braincenter-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn grupo(oc: usize, tr: usize, ind: usize) -> Vec<Value> {
        let g = |n: usize, p: &str, e: &str| (0..n).map(|i| json!({ "nome": format!("{p}{i}"), "estado": e })).collect::<Vec<_>>();
        [g(oc, "o", "ocioso"), g(tr, "t", "executando"), g(ind, "i", "desconectado")].concat()
    }

    fn cfg0() -> Value {
        let mut c = padrao();
        c["sustentado_s"] = json!(0);
        c
    }

    fn sem_leitura() -> Value {
        json!({ "pronto": false })
    }

    fn aut(ag: &[Value], em_ciclo: bool) -> (bool, Estado) {
        let mut e = Estado::default();
        let ok = avaliar(&mut e, ag, 1000.0, &cfg0(), &sem_leitura(), em_ciclo).0;
        (ok, e)
    }

    fn reg(d: &Path, agente: &str, texto: &str, assunto: &str, chave: Option<&str>) -> bool {
        registrar(d, &Ep { assunto, chave, ..Ep::novo(agente, texto) }, 1000.0)
    }

    fn cons(d: &Path, cfg: &Value, ts: f64, av: Option<&dyn Fn() -> bool>) -> Value {
        consolidar(d, &mut Estado::default(), cfg, ts, av)
    }

    fn p(d: &Path, k: &str) -> i64 {
        painel(d, &Estado::default(), 2000.0).unwrap()[k].as_i64().unwrap()
    }

    #[test]
    fn limites_de_ociosidade_e_quem_fica_fora_da_conta() {
        assert!(aut(&grupo(12, 0, 0), false).0 && aut(&grupo(9, 1, 0), false).0);
        assert!(aut(&grupo(17, 3, 0), false).0); // 85% exato: limite inclusivo
        assert!(!aut(&grupo(7, 3, 0), false).0 && !aut(&grupo(2, 10, 0), false).0);
        assert_eq!(aut(&grupo(7, 3, 0), true).1.estado, "PAUSADO POR DEMANDA");
        assert_eq!(aut(&grupo(7, 3, 0), false).1.estado, "AGUARDANDO OCIOSIDADE");
        let mut g = grupo(10, 2, 0);
        g.push(json!({ "nome": "BrainCenter", "estado": "executando" }));
        let m = medir(&g, &padrao());
        assert!(m["total"] == 12 && m["pct_ociosos"] == 83.3); // o proprio Brain Center fica fora
        let m = medir(&grupo(10, 0, 10), &padrao());
        assert!(m["ociosos"] == 10 && m["disponiveis"] == 10 && m["indisponiveis"] == 10); // indisponivel nao e ocioso
        let sem_dado = [json!({ "nome": "x", "estado": null }), json!({ "nome": "y", "estado": "inventado" })];
        assert!(medir(&sem_dado, &padrao())["pct_ociosos"] == 0.0 && !aut(&sem_dado, false).0);
    }

    #[test]
    fn sustentado_e_recursos() {
        let (cfg, ag, mut e) = (padrao(), grupo(10, 0, 0), Estado::default());
        assert!(!avaliar(&mut e, &ag, 1000.0, &cfg, &sem_leitura(), false).0);
        assert_eq!(avaliar(&mut e, &ag, 1200.0, &cfg, &sem_leitura(), false).1, "OCIOSIDADE COLETIVA ATINGIDA");
        assert!(avaliar(&mut e, &ag, 1301.0, &cfg, &sem_leitura(), false).0);
        avaliar(&mut e, &grupo(1, 9, 0), 1400.0, &cfg, &sem_leitura(), false); // caiu: zera a contagem
        assert!(!avaliar(&mut e, &ag, 1500.0, &cfg, &sem_leitura(), false).0);
        let r = avaliar(&mut e, &ag, 1900.0, &cfg, &json!({ "pronto": true, "cpu_pct": 95, "ram_pct": 10, "gpu_c": 40 }), false);
        assert!(!r.0 && r.1 == "PAUSADO POR RECURSOS" && r.2.contains("CPU em 95%"));
        assert!(recursos_ok(&cfg, &Value::Null).0 && recursos_ok(&cfg, &json!({ "cpu_pct": 10, "ram_pct": 10, "gpu_c": 81 })).1.contains("GPU"));
    }

    #[test]
    fn tick_sem_ociosidade_nao_consolida_e_com_ociosidade_consolida() {
        let d = pasta("tick");
        std::fs::create_dir_all(d.join("braincenter")).unwrap();
        std::fs::write(d.join("braincenter/config.json"), r#"{"sustentado_s": 0, "inventado": 1, "pesos": {"recencia": 0.3}}"#).unwrap();
        let c = config(&d);
        assert!(c["sustentado_s"] == 0 && c["inventado"].is_null() && c["pesos"]["recencia"] == 0.3 && c["pesos"]["confianca"] == 0.1);
        reg(&d, "a", "O painel novo da loja mostra os pedidos", "loja", None);
        let mut e = Estado::default();
        assert_eq!(tick(&d, &mut e, &grupo(3, 9, 0), 1000.0, &sem_leitura(), &|| None), "AGUARDANDO OCIOSIDADE");
        assert_eq!(p(&d, "pendentes"), 1);
        assert_eq!(tick(&d, &mut e, &grupo(9, 1, 0), 1000.0, &sem_leitura(), &|| Some(grupo(9, 1, 0))), "CONSOLIDANDO");
        assert!(p(&d, "pendentes") == 0 && e.estado == "CONCLUÍDO");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn duplicata_e_repeticao_nao_duplicam() {
        let d = pasta("dup");
        for ag in ["a", "b"] {
            assert!(reg(&d, ag, "O site da loja usa o painel novo de pedidos", "loja", Some("painel")));
        }
        assert!(!reg(&d, "a", "O site da loja usa o painel novo de pedidos", "loja", Some("painel"))); // reenvio nao entra
        assert!(!reg(&d, "a", "   ", "loja", None));
        let r = cons(&d, &cfg0(), 1000.0, None);
        assert!(r["criadas"] == 1 && r["duplicadas"] == 1);
        let r2 = cons(&d, &cfg0(), 1001.0, None);
        assert!(r2["novos"] == 0 && r2["criadas"] == 0);
        assert!(p(&d, "memorias") == 1 && p(&d, "episodios") == 2);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn contradicao_preserva_as_fontes_sinaliza_e_so_pessoa_resolve() {
        let d = pasta("conflito");
        reg(&d, "a", "O horário da loja é 18h", "loja", Some("horario"));
        reg(&d, "b", "O horário da loja é 19h", "loja", Some("horario"));
        assert_eq!(cons(&d, &cfg0(), 1000.0, None)["conflitos"], 1);
        assert!(p(&d, "conflitos_abertos") == 1 && p(&d, "memorias") == 2);
        let res = consultar(&d, "a", "horário da loja", "global", 5, &cfg0(), 1000.0);
        assert!(res.len() == 2 && res.iter().all(|x| x["conflito"] == true));
        assert!(!resolver_conflito(&d, 1, 999, "dono", 1000.0));
        assert!(resolver_conflito(&d, 1, res[0]["id"].as_i64().unwrap(), "dono", 1000.0) && p(&d, "conflitos_abertos") == 0);
        assert!(p(&d, "arquivadas") == 1 && p(&d, "episodios") == 2); // a outra foi arquivada, nao apagada
        let d2 = pasta("negacao");
        reg(&d2, "a", "A loja aceita pagamento no cartão", "loja", None);
        reg(&d2, "b", "A loja não aceita pagamento no cartão", "loja", None);
        assert_eq!(cons(&d2, &cfg0(), 1000.0, None)["conflitos"], 1);
        for x in [d, d2] {
            let _ = std::fs::remove_dir_all(&x);
        }
    }

    #[test]
    fn atualizacao_da_mesma_origem_guarda_historico() {
        let d = pasta("atualiza");
        reg(&d, "a", "Preço da esfiha é 8 reais", "loja", Some("preco"));
        cons(&d, &cfg0(), 1000.0, None);
        reg(&d, "a", "Preço da esfiha é 9 reais", "loja", Some("preco"));
        let r = cons(&d, &cfg0(), 1001.0, None);
        assert!(r["atualizadas"] == 1 && r["conflitos"] == 0);
        assert_eq!(conta(&db(&d).unwrap(), "SELECT COUNT(*) FROM historico"), 1);
        assert!(consultar(&d, "a", "preço esfiha", "global", 5, &cfg0(), 1002.0)[0]["texto"].as_str().unwrap().contains("9 reais"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn checkpoint_pausa_retoma_e_um_ciclo_por_vez() {
        let d = pasta("checkpoint");
        for i in 0..5 {
            reg(&d, "a", &format!("fato único número {i} sobre assunto{i}"), &format!("s{i}"), Some(&format!("k{i}")));
        }
        let mut cfg = cfg0();
        cfg["lote"] = json!(2);
        cfg["max_lotes_por_tick"] = json!(5);
        let r = cons(&d, &cfg, 1000.0, Some(&|| false)); // ociosidade caiu depois do 1o lote
        assert!(r["estado"] == "pausado" && r["lotes"] == 1 && p(&d, "pendentes") == 3);
        setmeta(&db(&d).unwrap(), "ciclo_ativo", "1000").unwrap(); // outro processo no meio de um ciclo
        assert_eq!(cons(&d, &cfg, 1100.0, None)["estado"], "ja_rodando");
        assert_eq!(meta(&db(&d).unwrap(), "ciclo_ativo"), "1000");
        let r = cons(&d, &cfg, 1700.0, Some(&|| true)); // marca velha (10 min) nao segura; retoma do checkpoint
        assert!(r["estado"] == "concluido" && p(&d, "pendentes") == 0 && p(&d, "memorias") == 5);
        assert_eq!(conta(&db(&d).unwrap(), "SELECT COUNT(*) FROM ciclos"), 1); // mesmo ciclo retomado
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn banco_indisponivel_vira_erro_sem_perder_registro() {
        let d = pasta("erro");
        let arq = d.join("nao-e-pasta");
        std::fs::write(&arq, "x").unwrap();
        let mut e = Estado::default();
        assert!(consolidar(&arq, &mut e, &cfg0(), 1000.0, None)["estado"] == "erro" && e.estado == "ERRO");
        assert!(!registrar(&arq, &Ep::novo("a", "texto"), 1000.0) && painel(&arq, &e, 1000.0).is_err());
        reg(&d, "a", "registro importante guardado", "x", None);
        assert!(p(&d, "pendentes") == 1 && cons(&d, &cfg0(), 1000.0, None)["criadas"] == 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn temporario_privado_e_projeto_nao_vazam() {
        let d = pasta("privado");
        reg(&d, "a", "Hoje o refrigerante zero acabou", "loja", None);
        registrar(&d, &Ep { assunto: "x", privado: true, ..Ep::novo("a", "Combinação de teste do ambiente local") }, 1000.0);
        registrar(&d, &Ep { projeto: "alfa", assunto: "stack", ..Ep::novo("a", "Projeto Alfa usa Rust no backend") }, 1000.0);
        let r = cons(&d, &cfg0(), 1000.0, None);
        assert!(r["criadas"] == 2 && r["temporarios"] == 1);
        let q = |ag: &str, t: &str, pj: &str, cfg: &Value| consultar(&d, ag, t, pj, 5, cfg, 1001.0).len();
        assert_eq!(q("b", "combinação ambiente local", "global", &cfg0()), 0); // privado nao vaza para outro agente
        assert_eq!(q("a", "combinação ambiente local", "global", &cfg0()), 1);
        assert_eq!(q("b", "Rust backend", "alfa", &cfg0()), 0); // sem permissao no projeto
        assert_eq!(q("b", "Rust backend", "global", &cfg0()), 0); // nao contamina o global
        let mut cfg = cfg0();
        cfg["permissoes"]["b"] = json!({ "projetos": ["alfa"] });
        assert_eq!(q("b", "Rust backend", "alfa", &cfg), 1);
        assert!(trabalho_set(&d, "a", "ctx", "rascunho", 60.0, 1000.0).is_ok());
        assert_eq!(trabalho_get(&d, "a", "ctx", 1030.0).as_deref(), Some("rascunho"));
        assert_eq!(trabalho_get(&d, "a", "ctx", 1061.0), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn nao_apaga_originais_e_arquiva_sem_perder() {
        let d = pasta("arquiva");
        reg(&d, "a", "Fato antigo e pouco usado sobre arquivo morto", "z", Some("k"));
        let agora = 200.0 * 86400.0;
        cons(&d, &cfg0(), agora, None);
        db(&d).unwrap().execute("UPDATE semantica SET atualizado=1, ultimo_uso=1, importancia=0.1, confianca=0.1", []).unwrap();
        let r = cons(&d, &cfg0(), agora + 700.0, None);
        assert!(p(&d, "arquivadas") == 1 && p(&d, "episodios") == 1 && r["arquivadas"] == 1);
        let _ = std::fs::remove_dir_all(&d);
    }
}
