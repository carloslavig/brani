//! Maestro da esteira de videos de prefeitura do Cliente: porta do orquestrador_50.py (fases, vozes, render, site, preparacao continua).
//! Estado e log nos MESMOS arquivos do Python (orquestrador.json, orquestrador.log, comando_esteira.json), porque os paineis leem eles.
//! O que ainda e Python (ajudante passo_esteira.py, so importa funcoes que ja existem): preparar a cidade (e-TCM, levantamento, auditoria
//! e guia), escolher a proxima, gravar a voz, copiar PDFs do TCM, avisar o Xandao. Render, site, fases e estado sao daqui.
//! NASCE DESLIGADO: o laco so anda se existir `<dados>/esteira_ligada` (POST /api/esteira/orq {"ligar":true}).
#![allow(dead_code)]

use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

pub const RENDERS_POR_RODADA: i64 = 5; // Dono, 14/11: 5 renders, depois 10 vozes
pub const VOZES_POR_RODADA: i64 = 10;
pub const MT_INICIO: i64 = 900; // serie 900 = Mato Grosso: vai ao WhatsApp do Dono, nao ao site do Cliente
const MAX_TENTATIVAS_VOZ: i64 = 6; // reprovou 6 rodadas: sai da vez ate resolver()
const PEDIDO_IRECE: &str = "33821a57b3d9";
const SEM_JANELA: u32 = 0x0800_0000;

// ---------------------------------------------------------------- processos

pub struct Cmd {
    pub prog: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub limite: Duration,
    /// Some = saida e erro vao direto para este arquivo (sem cano): o filho sobrevive se o B.R.A.N.I reiniciar no meio.
    pub arquivo: Option<PathBuf>,
}

pub struct Saida {
    pub codigo: i32,
    pub texto: String, // stdout + stderr, como o `run` do Python
}

pub type Exec = dyn Fn(&Cmd) -> Saida + Send + Sync;

/// Roda sem janela, com tempo limite; le saida e erro em paralelo (cano cheio travaria o filho).
pub fn exec_real(c: &Cmd) -> Saida {
    use std::io::Read;
    use std::process::Stdio;
    let mut cmd = std::process::Command::new(&c.prog);
    cmd.args(&c.args).current_dir(&c.cwd).stdin(Stdio::null());
    match c.arquivo.as_ref().map(|a| std::fs::File::create(a).and_then(|f| Ok((f.try_clone()?, f)))) {
        Some(Ok((o, e))) => cmd.stdout(o).stderr(e),
        Some(Err(e)) => return Saida { codigo: -1, texto: format!("não consegui abrir o log de {}: {e}", c.prog) },
        None => cmd.stdout(Stdio::piped()).stderr(Stdio::piped()),
    };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(SEM_JANELA);
    }
    let mut filho = match cmd.spawn() {
        Ok(f) => f,
        Err(e) => return Saida { codigo: -1, texto: format!("não consegui rodar {}: {e}", c.prog) },
    };
    fn ler<R: Read + Send + 'static>(r: Option<R>) -> std::thread::JoinHandle<Vec<u8>> {
        std::thread::spawn(move || {
            let mut b = vec![];
            if let Some(mut r) = r {
                let _ = r.read_to_end(&mut b);
            }
            b
        })
    }
    let (o, e) = (ler(filho.stdout.take()), ler(filho.stderr.take()));
    let inicio = Instant::now();
    let codigo = loop {
        match filho.try_wait() {
            Ok(Some(s)) => break s.code().unwrap_or(-1),
            Ok(None) if inicio.elapsed() > c.limite => {
                let _ = filho.kill();
                let _ = filho.wait();
                break -1;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(_) => break -1,
        }
    };
    let mut texto = String::from_utf8_lossy(&o.join().unwrap_or_default()).into_owned();
    texto += &String::from_utf8_lossy(&e.join().unwrap_or_default());
    if let Some(a) = &c.arquivo {
        texto = std::fs::read(a).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    }
    Saida { codigo, texto }
}

// ---------------------------------------------------------------- funcoes puras

/// Arquivo mexido nos ultimos 5 min (render.log de um node vivo).
fn fresco(p: &Path) -> bool {
    std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|d| d < Duration::from_secs(300))
}

fn algum_log_fresco(videos: &Path) -> bool {
    std::fs::read_dir(videos).into_iter().flatten().flatten().any(|e| fresco(&e.path().join("render.log")))
}

/// Item de fila: [numero, cidade, pedido-do-Xandao|null], como no json do Python.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub n: String,
    pub nome: String,
    pub pid: Option<String>,
}

impl Item {
    pub fn de(v: &Value) -> Option<Item> {
        let a = v.as_array()?;
        let n = match a.first()? {
            Value::String(s) => s.clone(),
            Value::Number(x) => x.to_string(),
            _ => return None,
        };
        Some(Item { n, nome: a.get(1).and_then(|x| x.as_str()).unwrap_or("").into(), pid: a.get(2).and_then(|x| x.as_str()).map(String::from) })
    }
    pub fn json(&self) -> Value {
        json!([self.n, self.nome, self.pid])
    }
    pub fn numero(&self) -> i64 {
        num(&self.n)
    }
}

pub fn num(n: &str) -> i64 {
    n.trim().parse().unwrap_or(i64::MAX)
}

pub fn itens(d: &Value, chave: &str) -> Vec<Item> {
    d[chave].as_array().map(|a| a.iter().filter_map(Item::de).collect()).unwrap_or_default()
}

fn por_itens(v: &[Item]) -> Value {
    Value::Array(v.iter().map(Item::json).collect())
}

fn lista_de_numeros(d: &Value, chave: &str) -> Vec<String> {
    d[chave].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default()
}

fn retirar_item(d: &mut Value, chave: &str, n: &str) {
    if let Some(a) = d[chave].as_array_mut() {
        a.retain(|x| x.get(0).and_then(|y| y.as_str()) != Some(n));
    }
}

fn empurrar(d: &mut Value, chave: &str, v: Value) {
    match d[chave].as_array_mut() {
        Some(a) => a.push(v),
        None => d[chave] = json!([v]),
    }
}

/// ASCII como o `unicodedata NFKD` + ignore do Python, so para o portugues (acento some, o resto nao-ASCII cai).
pub fn ascii(s: &str) -> String {
    s.chars()
        .filter_map(|c| {
            Some(match c {
                'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
                'Á' | 'À' | 'Â' | 'Ã' | 'Ä' => 'A',
                'é' | 'è' | 'ê' | 'ë' => 'e',
                'É' | 'È' | 'Ê' | 'Ë' => 'E',
                'í' | 'ì' | 'î' | 'ï' => 'i',
                'Í' | 'Ì' | 'Î' | 'Ï' => 'I',
                'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
                'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ö' => 'O',
                'ú' | 'ù' | 'û' | 'ü' => 'u',
                'Ú' | 'Ù' | 'Û' | 'Ü' => 'U',
                'ç' => 'c',
                'Ç' => 'C',
                'ñ' => 'n',
                'Ñ' => 'N',
                c if c.is_ascii() => c,
                _ => return None,
            })
        })
        .collect()
}

/// Pasta de entrega no site: NNN-CIDADE (nome ascii, espaco vira traco).
pub fn nome_pasta_site(n: &str, nome: &str) -> String {
    format!("{n}-{}", ascii(nome).replace(' ', "-"))
}

pub fn meta_do_site(n: &str, cidade: &str, cfg: &Value) -> Value {
    json!({"numero": n, "cidade": cidade, "periodo": cfg["periodo"].as_str().unwrap_or("2024-2026"), "projeto": cfg["projeto"]})
}

/// (modo, feitos) da fase; sem fase: render se ha voz pronta, senao voz.
pub fn fase_de(d: &Value) -> (String, i64) {
    match d.get("fase").filter(|f| f.is_object()) {
        Some(f) => (f["modo"].as_str().unwrap_or("voz").to_string(), f["feitos"].as_i64().unwrap_or(0)),
        None => ((if d["voz_ok"].as_array().is_some_and(|a| !a.is_empty()) { "render" } else { "voz" }).to_string(), 0),
    }
}

/// Troca a fase e devolve a linha do log.
pub fn trocar_fase(d: &mut Value, modo: &str) -> String {
    d["fase"] = json!({"modo": modo, "feitos": 0});
    format!("FASE: {}", if modo == "render" { "5 renders" } else { "10 vozes" })
}

fn livres_de_voz(d: &Value, em_voz: &BTreeSet<String>) -> Vec<Item> {
    itens(d, "preparadas").into_iter().filter(|x| !em_voz.contains(&x.n) && d["voz_tentativas"][&x.n].as_i64().unwrap_or(0) < MAX_TENTATIVAS_VOZ).collect()
}

/// Proxima cidade para a voz: a de menor numero, ate fechar a rodada de 10 (contando as que estao gravando).
pub fn escolher_voz(d: &Value, em_voz: &BTreeSet<String>) -> Option<Item> {
    let feitos = d["fase"]["feitos"].as_i64().unwrap_or(0);
    if feitos + em_voz.len() as i64 >= VOZES_POR_RODADA {
        return None;
    }
    livres_de_voz(d, em_voz).into_iter().min_by_key(Item::numero)
}

/// Acabou a fila de voz (nada livre, nada gravando) e ha voz pronta: hora de renderizar.
pub fn trocar_para_render(d: &Value, em_voz: &BTreeSet<String>) -> bool {
    livres_de_voz(d, em_voz).is_empty() && em_voz.is_empty() && d["voz_ok"].as_array().is_some_and(|a| !a.is_empty())
}

pub fn escolher_render(d: &Value) -> Option<Item> {
    itens(d, "voz_ok").into_iter().min_by_key(Item::numero)
}

/// Estado depois de uma voz. `em_voz_n` = quantas gravam agora, contando esta. Devolve a linha de log se a fase trocou.
pub fn depois_da_voz(d: &mut Value, item: &Item, ok: bool, em_voz_n: usize) -> Option<String> {
    if ok {
        retirar_item(d, "preparadas", &item.n);
        if !itens(d, "voz_ok").iter().any(|x| x.n == item.n) {
            empurrar(d, "voz_ok", item.json());
        }
        let feitos = fase_de(d).1;
        let modo = d["fase"]["modo"].as_str().unwrap_or("voz").to_string();
        d["fase"] = json!({"modo": modo, "feitos": feitos + 1});
    } else {
        let t = d["voz_tentativas"][&item.n].as_i64().unwrap_or(0) + 1;
        if !d["voz_tentativas"].is_object() {
            d["voz_tentativas"] = json!({});
        }
        d["voz_tentativas"][&item.n] = json!(t);
    }
    if d["fase"]["feitos"].as_i64().unwrap_or(0) >= VOZES_POR_RODADA && em_voz_n <= 1 {
        return Some(trocar_fase(d, "render")); // a ultima voz da rodada terminou
    }
    None
}

/// Estado depois do render (r = true pronto, false pendencia). Devolve (ha lote para o site, linha de log da fase).
pub fn depois_do_render(d: &mut Value, item: &Item, r: bool) -> (bool, Option<String>) {
    retirar_item(d, "voz_ok", &item.n);
    empurrar(d, "feitas", json!(item.n));
    if r {
        empurrar(d, "prontos", json!(item.n));
        if item.numero() < MT_INICIO {
            empurrar(d, "lote_site", json!([item.n, item.nome])); // MT vai ao WhatsApp (renderizar), nao ao site
        }
    }
    let feitos = d["fase"]["feitos"].as_i64().unwrap_or(0) + 1;
    let modo = d["fase"]["modo"].as_str().unwrap_or("render").to_string();
    d["fase"] = json!({"modo": modo, "feitos": feitos});
    let sem_voz = d["voz_ok"].as_array().is_none_or(|a| a.is_empty());
    let log = (feitos >= RENDERS_POR_RODADA || sem_voz).then(|| trocar_fase(d, "voz"));
    (d["lote_site"].as_array().is_some_and(|a| !a.is_empty()), log)
}

/// Depois do preparo: sai de preparadas/voz_ok (refeita: voz antiga nao vale) e entra de novo se deu certo.
/// `prioridade` entra logo depois da que esta na voz (indice 1); as outras no fim.
pub fn depois_do_preparo(d: &mut Value, item: &Item, ok: bool, prioridade: bool) {
    d["atual"] = Value::Null;
    retirar_item(d, "preparadas", &item.n);
    retirar_item(d, "voz_ok", &item.n);
    if ok {
        if let Some(a) = d["feitas"].as_array_mut() {
            a.retain(|x| x.as_str() != Some(&item.n));
        }
        let mut p = itens(d, "preparadas");
        p.insert(if prioridade { 1.min(p.len()) } else { p.len() }, item.clone());
        d["preparadas"] = por_itens(&p);
    } else if !lista_de_numeros(d, "feitas").contains(&item.n) {
        empurrar(d, "feitas", json!(item.n));
    }
}

pub fn aviso_lote(ok: &[String], pendencias: &[String], revisar: &[String]) -> String {
    let mut t = format!("[Braniac · Jarvis] Parceiro, por favor avise o Cliente no Telegram: lote novo no site dele. Lote entregue pela caixa de entrada do site: {}. Mesmo padrão dos de hoje (seta no centro da cidade, mês certo nos selos, voz Algenib conferida número a número, acelerada só se passar de 3 min). Confira e aprove um por um: VIDEO APROVADO NNN (ou VIDEO REPROVADO NNN: motivo). PDFs do TCM de cada cidade ficam no PC do Dono (RECEITAS-DESPESAS-CIDADE).", ok.join(", "));
    if !pendencias.is_empty() {
        t += &format!(" Pendências desde o último lote (prefeitura sem documento no TCM ou soma que não fecha): {}.", pendencias.join(", "));
    }
    if !revisar.is_empty() {
        t += &format!(" PARA UMA PESSOA REVISAR (a máquina não reprovou nada; decida vocês): {}", revisar.join(" | "));
    }
    t
}

fn corta(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn corta_fim(s: &str, n: usize) -> String {
    let c: Vec<char> = s.chars().collect();
    c[c.len().saturating_sub(n)..].iter().collect()
}

fn achar_partes(raiz: &Path, saida: &mut Vec<PathBuf>) {
    if let Ok(rd) = std::fs::read_dir(raiz) {
        for e in rd.flatten() {
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                if e.file_name().to_string_lossy().starts_with(".partes") {
                    saida.push(e.path());
                } else {
                    achar_partes(&e.path(), saida);
                }
            }
        }
    }
}

// ---------------------------------------------------------------- configuracao

#[derive(Clone)]
pub struct Config {
    pub pasta: PathBuf, // orquestrador.json, .log, comando_esteira.json
    pub pacote: PathBuf,
    pub estudio: PathBuf,
    pub site: PathBuf,
    pub envia: PathBuf,
    pub raiz: PathBuf, // cwd do Python do Braniac
    pub ajudante: PathBuf,
    pub backup: PathBuf,
    pub ligada: PathBuf, // existe = laco liga
    pub python: String,
    pub segundo: Duration, // 1 s de verdade; nos testes, 1 ms
}

impl Config {
    pub fn real(dados: &Path) -> Config {
        let var = |k: &str, padrao: &str| std::env::var_os(k).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(padrao));
        let pacote = var("BRANIAC_PACOTE", r"C:\Cliente\PACOTE-PRODUCAO-COMPLETA");
        Config {
            pasta: var("BRANIAC_ESTEIRA_DADOS", r"%USERPROFILE%\Projetos\Agente\braniac\data\magno"),
            estudio: pacote.join("estudio-3d"),
            ajudante: pacote.join("scripts-braniac").join("passo_esteira.py"),
            pacote,
            site: PathBuf::from(r"E:\braniac-link\site"),
            envia: PathBuf::from(r"C:\Braniac\envio\envia_site.py"),
            raiz: PathBuf::from(r"%USERPROFILE%\Projetos\Agente"),
            backup: PathBuf::from(r"E:\BACKUP DO ESCRITORIO PREFEITURAS VIDEO"),
            ligada: dados.join("esteira_ligada"),
            python: "python".into(),
            segundo: Duration::from_secs(1),
        }
    }
}

pub struct Comando {
    pub meta: i64,
    pub parado: bool,
    pub so_fila: bool,
}

// ---------------------------------------------------------------- maestro

pub struct Maestro {
    pub cfg: Config,
    exec: Box<Exec>,
    trava_estado: Mutex<()>,
    trava_log: Mutex<()>,
    trava_site: Mutex<()>,
    em_voz: Mutex<BTreeSet<String>>,
    render_atual: Mutex<Option<String>>,
    pend_lote: Mutex<Vec<String>>,
    revisar: Mutex<Vec<String>>,
    bloqueios: Mutex<HashMap<String, (Instant, bool)>>,
    vivo: Mutex<Option<(Instant, bool)>>,
    avisado: Mutex<HashMap<&'static str, Instant>>,
    fila: Mutex<Vec<Item>>,
    fila_pronta: AtomicBool,
    fim_armado: AtomicBool,
}

fn travar<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Maestro {
    pub fn novo(cfg: Config, exec: Box<Exec>) -> Maestro {
        Maestro {
            cfg,
            exec,
            trava_estado: Mutex::new(()),
            trava_log: Mutex::new(()),
            trava_site: Mutex::new(()),
            em_voz: Mutex::new(BTreeSet::new()),
            render_atual: Mutex::new(None),
            pend_lote: Mutex::new(vec![]),
            revisar: Mutex::new(vec![]),
            bloqueios: Mutex::new(HashMap::new()),
            vivo: Mutex::new(None),
            avisado: Mutex::new(HashMap::new()),
            fila: Mutex::new(vec![]),
            fila_pronta: AtomicBool::new(false),
            fim_armado: AtomicBool::new(false),
        }
    }

    pub fn real(dados: &Path) -> Maestro {
        Maestro::novo(Config::real(dados), Box::new(exec_real))
    }

    fn dorme(&self, s: u64) {
        std::thread::sleep(self.cfg.segundo * s as u32);
    }

    // ---- log e estado

    fn arq_estado(&self) -> PathBuf {
        self.cfg.pasta.join("orquestrador.json")
    }

    fn arq_log(&self) -> PathBuf {
        self.cfg.pasta.join("orquestrador.log")
    }

    /// Linha `dd/mm HH:MM:SS texto`, igual a do Python.
    pub fn log(&self, texto: &str) {
        use std::io::Write;
        let a = crate::tempo::agora();
        let linha = format!("{} {:02}:{:02}:{:02} {texto}\n", a.data.dia_mes(), a.hora, a.min, a.seg);
        let _g = travar(&self.trava_log);
        let _ = std::fs::create_dir_all(&self.cfg.pasta);
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(self.arq_log()) {
            let _ = f.write_all(linha.as_bytes());
        }
    }

    pub fn estado(&self) -> Value {
        let mut d: Value = std::fs::read(self.arq_estado()).ok().and_then(|b| serde_json::from_slice(&b).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}));
        for k in ["prontos", "lote_site", "enviados", "pendencias", "feitas", "preparadas", "voz_ok"] {
            if !d[k].is_array() {
                d[k] = json!([]);
            }
        }
        d
    }

    /// Le, aplica f e grava, com a trava (as linhas de trabalho nunca se atropelam). Campos que nao conheco ficam como estao.
    pub fn mexer<R>(&self, f: impl FnOnce(&mut Value) -> R) -> R {
        let _g = travar(&self.trava_estado);
        let mut d = self.estado();
        let r = f(&mut d);
        let _ = std::fs::create_dir_all(&self.cfg.pasta);
        let tmp = self.arq_estado().with_extension("json.tmp");
        if let Ok(t) = serde_json::to_string_pretty(&d) {
            // escreve ao lado e troca: o painel nunca le o arquivo pela metade
            if std::fs::write(&tmp, t).is_ok() {
                let _ = std::fs::rename(&tmp, self.arq_estado());
            }
        }
        r
    }

    pub fn comando(&self) -> Comando {
        let c: Value = std::fs::read(self.cfg.pasta.join("comando_esteira.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
        Comando { meta: c["meta"].as_i64().unwrap_or(50), parado: c["parado"].as_bool().unwrap_or(false), so_fila: c["so_fila"].as_bool().unwrap_or(false) }
    }

    pub fn ligada(&self) -> bool {
        self.cfg.ligada.exists()
    }

    /// Pode comecar passo novo? Desligado ou parado: nao (o que esta no meio termina).
    pub fn ativo(&self) -> bool {
        self.ligada() && !self.comando().parado
    }

    // ---- ajudante Python

    fn py(&self, args: &[&str], limite_s: u64) -> Result<Value, String> {
        let mut a: Vec<String> = vec!["-X".into(), "utf8".into(), self.cfg.ajudante.to_string_lossy().into_owned()];
        a.extend(args.iter().map(|s| s.to_string()));
        let s = (self.exec)(&Cmd { prog: self.cfg.python.clone(), args: a, cwd: self.cfg.raiz.clone(), limite: self.cfg.segundo * limite_s as u32, arquivo: None });
        let v = s.texto.lines().rev().map(str::trim).filter(|l| l.starts_with('{')).find_map(|l| serde_json::from_str::<Value>(l).ok());
        match v {
            Some(v) if v["erro"].is_string() => Err(v["erro"].as_str().unwrap_or("").to_string()),
            Some(v) => Ok(v),
            None => Err(format!("ajudante sem resposta (código {}): {}", s.codigo, corta_fim(s.texto.trim(), 200))),
        }
    }

    fn etapa(&self, n: &str, nome: &str, etapa: &str, detalhe: &str) {
        if let Err(e) = self.py(&["etapa", n, nome, etapa, detalhe], 120) {
            self.log(&format!("{n} não gravei a etapa {etapa}: {}", corta(&e, 150)));
        }
    }

    fn pendencia(&self, n: &str, nome: &str, motivo: &str) {
        if let Err(e) = self.py(&["pendencia", n, nome, &corta(motivo, 380)], 120) {
            self.log(&format!("{n} não gravei a pendência no Lupa: {}", corta(&e, 150)));
        }
        travar(&self.pend_lote).push(format!("{n} {nome}"));
        self.log(&format!("PENDÊNCIA {n} {nome} - {}", corta(motivo, 200)));
    }

    /// Quantos processos python (ou o que for) tem esse trecho na linha de comando.
    fn existe_processo(&self, padrao: &str) -> bool {
        self.existe_de("python", padrao)
    }

    fn existe_de(&self, nome: &str, padrao: &str) -> bool {
        let filtro = format!("(Get-CimInstance Win32_Process -Filter \"Name like '{nome}%'\" | Where-Object {{ $_.CommandLine -like '*{padrao}*' }}).Count");
        let s = (self.exec)(&Cmd { prog: "powershell".into(), args: vec!["-NoProfile".into(), "-Command".into(), filtro], cwd: self.cfg.raiz.clone(), limite: Duration::from_secs(60), arquivo: None });
        !matches!(s.texto.trim(), "" | "0")
    }

    /// O orquestrador Python ainda esta vivo? (guardado por 30 s: o powershell custa ~1 s)
    pub fn python_vivo(&self) -> bool {
        let mut g = travar(&self.vivo);
        if let Some((t, v)) = *g {
            if t.elapsed() < self.cfg.segundo * 30 {
                return v;
            }
        }
        let v = self.existe_processo("orquestrador_50");
        *g = Some((Instant::now(), v));
        v
    }

    /// Mensagem de log no maximo uma vez a cada 5 minutos por motivo.
    fn avisar_uma_vez(&self, chave: &'static str, texto: &str) {
        let mut g = travar(&self.avisado);
        if g.get(chave).is_none_or(|t| t.elapsed() > self.cfg.segundo * 300) {
            g.insert(chave, Instant::now());
            self.log(texto);
        }
    }

    fn preparo(&self, n: &str) -> Result<Value, String> {
        let p = self.cfg.pacote.join("videos").join(n).join("preparo.json");
        serde_json::from_slice(&std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?).map_err(|e| format!("preparo.json de {n}: {e}"))
    }

    pub fn ja_no_backup(&self, n: &str) -> bool {
        self.preparo(n).ok().is_some_and(|c| self.cfg.backup.join(c["projeto"].as_str().unwrap_or("")).join(format!("{}.mp4", c["nome"].as_str().unwrap_or(""))).exists())
    }

    /// Cidade ja no site do Cliente (a esteira nao refaz sozinha). Pergunta ao Python, com cache de 10 min.
    fn bloqueada(&self, n: &str) -> bool {
        if let Some((t, v)) = travar(&self.bloqueios).get(n) {
            if t.elapsed() < self.cfg.segundo * 600 {
                return *v;
            }
        }
        let v = self.py(&["bloqueia", n], 120).ok().is_some_and(|r| r["bloqueia"][n].as_bool().unwrap_or(false));
        travar(&self.bloqueios).insert(n.to_string(), (Instant::now(), v));
        v
    }

    /// Tira da fila (voz e render) toda cidade que ja tem video no backup ou ja esta no site.
    pub fn pular_feitos(&self) {
        let d = self.estado();
        let prontos = lista_de_numeros(&d, "prontos");
        let mut alvo: Vec<(&str, Item, bool)> = vec![];
        for chave in ["preparadas", "voz_ok"] {
            for x in itens(&d, chave) {
                let backup = !prontos.contains(&x.n) && self.ja_no_backup(&x.n);
                if backup || self.bloqueada(&x.n) {
                    alvo.push((chave, x, backup));
                }
            }
        }
        if alvo.is_empty() {
            return;
        }
        self.mexer(|d| {
            for (chave, x, backup) in &alvo {
                if !itens(d, chave).iter().any(|y| y.n == x.n) {
                    continue;
                }
                retirar_item(d, chave, &x.n);
                if *backup {
                    empurrar(d, "prontos", json!(x.n));
                    empurrar(d, "feitas", json!(x.n));
                    empurrar(d, "lote_site", json!([x.n, x.nome]));
                    self.log(&format!("JÁ NO BACKUP {} {} - não refaz", x.n, x.nome));
                } else {
                    self.log(&format!("JÁ NO SITE {} {} - não refaz", x.n, x.nome));
                }
            }
        });
    }

    // ---- voz

    /// Um ciclo do trabalhador de voz. Devolve quantos segundos esperar antes do proximo.
    pub fn passo_voz(&self) -> u64 {
        if !self.ativo() {
            return 5;
        }
        let meta = self.comando().meta;
        let d0 = self.estado();
        if lista_de_numeros(&d0, "prontos").len() as i64 >= meta || fase_de(&d0).0 != "voz" {
            return 30;
        }
        if self.python_vivo() {
            self.avisar_uma_vez("py_vivo", "orquestrador Python ainda vivo: o laço do Braniac não pega passo novo");
            return 30;
        }
        self.pular_feitos();
        let item = {
            let mut em = travar(&self.em_voz);
            let escolhido = self.mexer(|d| {
                let e = escolher_voz(d, &em);
                if e.is_none() && trocar_para_render(d, &em) {
                    let l = trocar_fase(d, "render");
                    self.log(&l);
                }
                e
            });
            if let Some(i) = &escolhido {
                em.insert(i.n.clone());
            }
            escolhido
        };
        let Some(item) = item else { return 30 };
        // cota da voz do Cliente: sem cota espera 10 min (a cidade volta para a fila)
        if !self.py(&["cota"], 180).ok().is_some_and(|r| r["ok"].as_bool().unwrap_or(false)) {
            travar(&self.em_voz).remove(&item.n);
            return 600;
        }
        self.log(&format!("VOZ {} {}", item.n, item.nome));
        let r = self.py(&["voz", &item.n, &item.nome, item.pid.as_deref().unwrap_or("")], 6 * 3600);
        let ok = match &r {
            Ok(v) => v["r"].as_bool().unwrap_or(false),
            Err(e) => {
                self.log(&format!("{} erro inesperado na voz: {}", item.n, corta(e, 200)));
                false
            }
        };
        let em_n = travar(&self.em_voz).len();
        self.mexer(|d| {
            if let Some(l) = depois_da_voz(d, &item, ok, em_n) {
                self.log(&l);
            }
        });
        travar(&self.em_voz).remove(&item.n);
        if ok {
            self.log(&format!("VOZ PRONTA {} {}", item.n, item.nome));
            0
        } else {
            120
        }
    }

    // ---- render

    /// Render + limitador + cópias de fontes/roteiro. Ok(true) pronto, Ok(false) pendencia, Err = erro inesperado (tenta de novo depois).
    pub fn renderizar(&self, n: &str, nome: &str, pid: Option<&str>) -> Result<bool, String> {
        let cfg = self.preparo(n)?;
        let (projeto, nome_arq) = (cfg["projeto"].as_str().unwrap_or("").to_string(), cfg["nome"].as_str().unwrap_or("").to_string());
        let pj = self.cfg.estudio.join("projetos").join(&projeto);
        let ed = self.cfg.estudio.join("edicoes").join(&projeto);
        self.etapa(n, nome, "render", "Estúdio 3D");
        {
            // trava de diretorio (mkdir atomico): um render por vez, tambem contra o Python e a prévia
            let trava = self.cfg.pacote.join(".render-trava");
            let alvo = format!("videos/{n}/video.json");
            // render que sobreviveu a um reinicio do B.R.A.N.I: espera ele acabar em vez de comecar outro por cima
            let log_n = self.cfg.pacote.join("videos").join(n).join("render.log");
            // dois sinais: o processo na lista OU o render.log mexido ha pouco (o node escreve nele a cada poucos segundos)
            let vivo = |m: &Self| m.existe_de("node", &alvo) || fresco(&log_n);
            let adotado = vivo(self);
            if adotado {
                self.log(&format!("{n} render já rodando (de antes do reinício): esperando terminar"));
                while vivo(self) {
                    self.dorme(30);
                }
            }
            loop {
                match std::fs::create_dir(&trava) {
                    Ok(()) => break,
                    // trava sem nenhum render vivo = sobra de um B.R.A.N.I que morreu no meio
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && !self.existe_de("node", "renderizar.mjs") && !algum_log_fresco(&self.cfg.pacote.join("videos")) => {
                        let _ = std::fs::remove_dir(&trava);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => self.dorme(20),
                    Err(e) => return Err(format!("trava do render: {e}")),
                }
            }
            struct Solta(PathBuf);
            impl Drop for Solta {
                fn drop(&mut self) {
                    let _ = std::fs::remove_dir(&self.0);
                }
            }
            let _solta = Solta(trava);
            let mut partes = vec![];
            if !adotado {
                achar_partes(&pj, &mut partes);
            }
            for x in partes {
                let _ = std::fs::remove_dir_all(x);
            }
            if adotado {
                // o render.log ja foi gravado pelo proprio node
            } else if let Err(e) = self.py(&["satelite", cfg["comp"].as_str().unwrap_or("")], 1800) {
                self.log(&format!("{n} satélite: {}", corta(&e, 150)));
            }
            if !adotado {
                let log = self.cfg.pacote.join("videos").join(n).join("render.log");
                let s = (self.exec)(&Cmd {
                    prog: "node".into(),
                    args: ["renderizar.mjs", &format!("videos/{n}/video.json"), "--nucleos=6", "--gl=angle", "--quadros-por-parte=600"].iter().map(|s| s.to_string()).collect(),
                    cwd: self.cfg.pacote.clone(),
                    limite: self.cfg.segundo * 6 * 3600,
                    arquivo: Some(log.clone()), // sem cano: o node nao morre se o B.R.A.N.I reiniciar
                });
                // o renderizar.mjs nao grava log sozinho
                std::fs::write(&log, &s.texto).map_err(|e| format!("render.log: {e}"))?;
            }
        }
        let mp4 = ed.join(format!("{nome_arq}.mp4"));
        let logr = std::fs::read(self.cfg.pacote.join("videos").join(n).join("render.log")).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
        let avisos: Vec<String> = regex::Regex::new(r"(?m)^(?:ERRO|PAROU|ATENÇÃO).*$").map_err(|e| e.to_string())?.find_iter(&logr).map(|m| m.as_str().to_string()).collect();
        if !mp4.exists() {
            self.pendencia(n, nome, &format!("render não gerou o vídeo: {}", corta_fim(&avisos.join("; "), 200)));
            return Ok(false);
        }
        let rev = pj.join("audio").join("REVISAO-HUMANA.txt");
        if !avisos.is_empty() {
            // video existe = pronto; quem decide se esta ruim e uma pessoa
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&rev) {
                let _ = writeln!(f, "Conferência automática do render achou: {}", corta_fim(&avisos.join("; "), 400));
            }
        }
        let tmp = ed.join(format!("{nome_arq}.lim.mp4"));
        (self.exec)(&Cmd {
            prog: "ffmpeg".into(),
            args: ["-v", "error", "-y", "-i", &mp4.to_string_lossy(), "-c:v", "copy", "-af", "alimiter=limit=0.84:level=false", "-c:a", "aac", "-b:a", "192k", &tmp.to_string_lossy()].iter().map(|s| s.to_string()).collect(),
            cwd: self.cfg.raiz.clone(),
            limite: self.cfg.segundo * 6 * 3600,
            arquivo: None,
        });
        if tmp.exists() {
            let _ = std::fs::remove_file(&mp4);
            std::fs::rename(&tmp, &mp4).map_err(|e| format!("alimiter: {e}"))?;
        }
        for (de, para) in [("fontes.md", format!("{nome_arq} - fontes.md")), ("roteiro-falado.txt", format!("{nome_arq} - roteiro.txt"))] {
            std::fs::copy(pj.join("audio").join(de), ed.join(para)).map_err(|e| format!("{de}: {e}"))?;
        }
        let revisao = rev.exists();
        if revisao {
            let t = std::fs::read_to_string(&rev).unwrap_or_default();
            let _ = std::fs::write(ed.join(format!("{nome_arq} - REVISAR.txt")), format!("Para uma pessoa olhar (a máquina não reprova nem apaga):\n{t}"));
            travar(&self.revisar).push(format!("{n} {nome}: {}", corta(&t.replace('\n', " "), 200)));
            self.log(&format!("{n} PRONTO COM AVISO para revisão humana: {}", corta(&t, 150)));
        }
        self.etapa(n, nome, "pronto", &format!("voz Algenib conferida, seta do mapa certa{}", if revisao { " · REVISAR (ver arquivo REVISAR)" } else { "" }));
        if let Some(p) = pid.filter(|p| !p.is_empty()) {
            let _ = self.py(&["entregue", p, &mp4.to_string_lossy()], 120);
        }
        if num(n) >= MT_INICIO {
            if let Err(e) = self.py(&["mt_aviso", n, &nome_arq, &ed.to_string_lossy(), &mp4.to_string_lossy()], 1800) {
                self.log(&format!("{n} não avisei o Dono do vídeo de MT: {}", corta(&e, 150)));
            }
        }
        self.log(&format!("PRONTO {n} {nome} {}", mp4.display()));
        Ok(true)
    }

    fn cfg_de(&self, n: &str) -> Result<(Value, PathBuf, PathBuf), String> {
        let cfg = self.preparo(n)?;
        let p = cfg["projeto"].as_str().unwrap_or("").to_string();
        Ok((cfg, self.cfg.estudio.join("edicoes").join(&p), self.cfg.estudio.join("projetos").join(p)))
    }

    fn montar_pasta_site(&self, n: &str, nome: &str) -> Result<PathBuf, String> {
        let (cfg, ed, pj) = self.cfg_de(n)?;
        let nome_arq = cfg["nome"].as_str().unwrap_or("");
        let d = self.cfg.site.join(nome_pasta_site(n, nome_arq));
        std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
        for f in [format!("{nome_arq}.mp4"), format!("{nome_arq} - capa.jpg"), format!("{nome_arq} - legenda.txt")] {
            std::fs::copy(ed.join(&f), d.join(&f)).map_err(|e| format!("{f}: {e}"))?;
        }
        std::fs::copy(pj.join("audio").join("fontes.md"), d.join(format!("{} - fontes.md", cfg["projeto"].as_str().unwrap_or(""))))
            .map_err(|e| format!("fontes.md: {e}"))?;
        std::fs::write(d.join("meta.json"), serde_json::to_string_pretty(&meta_do_site(n, nome, &cfg)).unwrap_or_default()).map_err(|e| e.to_string())?;
        // os PDFs do TCM da cidade vao junto, na mesma pasta de entrega
        match self.py(&["pdfs", n, nome, &d.to_string_lossy()], 1800) {
            Ok(r) => self.log(&format!("PDFS-TCM {n} {nome} {} PDFs na pasta de entrega", r["qtd"])),
            Err(e) => self.log(&format!("PDFS-TCM {n} {nome} não copiei: {}", corta(&e, 150))),
        }
        Ok(d)
    }

    /// Monta a pasta NNN-CIDADE de cada video, envia pela caixa de entrada do site e avisa o Xandao. Devolve "NNN CIDADE" dos que chegaram.
    pub fn mandar_site(&self, lote: &[(String, String)]) -> Vec<String> {
        let mut ok = vec![];
        let envia = self.cfg.envia.to_string_lossy().into_owned();
        for (n, nome) in lote {
            let d = match self.montar_pasta_site(n, nome) {
                Ok(d) => d,
                Err(e) => {
                    self.log(&format!("SITE {n} {nome} FALHOU ao montar a pasta: {}", corta(&e, 150)));
                    continue;
                }
            };
            let dpath = d.to_string_lossy().into_owned();
            let rodar = |extra: Option<&str>| {
                let mut a = vec![envia.clone(), dpath.clone()];
                a.extend(extra.map(String::from));
                (self.exec)(&Cmd { prog: self.cfg.python.clone(), args: a, cwd: self.cfg.raiz.clone(), limite: self.cfg.segundo * 3600, arquivo: None })
            };
            let s = rodar(None);
            let chegou = s.codigo == 0 && s.texto.contains("Recebido");
            self.log(&format!("SITE {n} {nome} {}", if chegou { "ok".to_string() } else { format!("FALHOU {}", corta_fim(&s.texto, 150)) }));
            if chegou {
                let s2 = rodar(Some("--pdfs"));
                self.log(&format!("SITE-PDFS {n} {nome} {}", if s2.codigo == 0 { "ok" } else { "site ainda não aceita PDF (ficam na pasta pdfs-tcm)" }));
                ok.push(format!("{n} {nome}"));
            }
        }
        for (n, _) in lote {
            if ok.iter().any(|x| x.starts_with(&format!("{n} "))) {
                // nada e apagado depois do site; a pasta so ganha "CONCLUÍDO" na frente
                if let Ok((_, ed, _)) = self.cfg_de(n) {
                    if ed.exists() {
                        let novo = ed.with_file_name(format!("CONCLUÍDO {}", ed.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()));
                        if let Err(e) = std::fs::rename(&ed, novo) {
                            self.log(&format!("{n} não renomeei a pasta para CONCLUÍDO: {}", corta(&e.to_string(), 150)));
                        }
                    }
                }
            }
        }
        let aviso = aviso_lote(&ok, &travar(&self.pend_lote), &travar(&self.revisar));
        if let Err(e) = self.py(&["avisar_xandao", &aviso], 120) {
            self.log(&format!("não avisei o Parceiro: {}", corta(&e, 150)));
        }
        travar(&self.revisar).clear();
        travar(&self.pend_lote).clear();
        ok
    }

    /// Manda o que esta em lote_site e limpa; uma so linha de envio por vez.
    fn enviar_lote(&self) {
        let _g = travar(&self.trava_site);
        let lote: Vec<(String, String)> = itens(&self.estado(), "lote_site").into_iter().map(|x| (x.n, x.nome)).collect();
        if lote.is_empty() {
            return;
        }
        let ok = self.mandar_site(&lote);
        self.mexer(|d| {
            let atual = d["enviados"].as_array().cloned().unwrap_or_default();
            d["enviados"] = Value::Array(atual.into_iter().chain(ok.iter().map(|x| json!(x))).collect());
            if let Some(a) = d["lote_site"].as_array_mut() {
                a.retain(|x| !lote.iter().any(|(n, nm)| x[0].as_str() == Some(n) && x[1].as_str() == Some(nm)));
            }
        });
    }

    /// Um ciclo do trabalhador de render. Devolve quantos segundos esperar.
    pub fn passo_render(&self) -> u64 {
        if !self.ativo() {
            return 5;
        }
        let meta = self.comando().meta;
        let d0 = self.estado();
        if lista_de_numeros(&d0, "prontos").len() as i64 >= meta || fase_de(&d0).0 != "render" {
            return 30;
        }
        if self.python_vivo() {
            self.avisar_uma_vez("py_vivo", "orquestrador Python ainda vivo: o laço do Braniac não pega passo novo");
            return 30;
        }
        self.pular_feitos();
        if itens(&self.estado(), "voz_ok").is_empty() {
            self.mexer(|d| {
                let l = trocar_fase(d, "voz");
                self.log(&l);
            });
            return 0;
        }
        // a placa e uma so: espera a voz que estiver terminando
        while self.cfg.pacote.join(".whisper-trava").exists() || !travar(&self.em_voz).is_empty() {
            if !self.ativo() {
                return 5;
            }
            self.dorme(15);
        }
        let Some(item) = escolher_render(&self.estado()) else { return 0 };
        self.log(&format!("RENDER {} {}", item.n, item.nome));
        *travar(&self.render_atual) = Some(format!("{} {}", item.n, item.nome));
        let r = self.renderizar(&item.n, &item.nome, item.pid.as_deref());
        *travar(&self.render_atual) = None;
        let r = match r {
            Ok(r) => r,
            Err(e) => {
                self.log(&format!("{} erro inesperado no render: {}", item.n, corta(&e, 200)));
                return 300;
            }
        };
        let (ha_lote, fase_log) = self.mexer(|d| depois_do_render(d, &item, r));
        if let Some(l) = fase_log {
            self.log(&l);
        }
        if ha_lote {
            self.enviar_lote(); // cada video vai ao site assim que fica pronto
        }
        let d = self.estado();
        self.log(&format!("placar: {}/{meta} prontos, {} com voz esperando render, {} esperando voz", lista_de_numeros(&d, "prontos").len(), itens(&d, "voz_ok").len(), itens(&d, "preparadas").len()));
        0
    }

    // ---- preparacao continua (main() do Python)

    fn preparar_fila(&self) {
        let d = self.estado();
        let ja: BTreeSet<String> = lista_de_numeros(&d, "feitas").into_iter().chain(itens(&d, "preparadas").into_iter().map(|x| x.n)).chain(itens(&d, "voz_ok").into_iter().map(|x| x.n)).collect();
        let mut fila = vec![];
        if !ja.contains("141") {
            fila.push(Item { n: "141".into(), nome: "Irecê".into(), pid: Some(PEDIDO_IRECE.into()) });
        }
        if let Some(a) = Item::de(&d["atual"]).filter(|a| !ja.contains(&a.n)) {
            fila = vec![a]; // reiniciou no meio de uma cidade: retoma ela
        }
        *travar(&self.fila) = fila;
        self.mexer(|d| {
            if !d["preparadas"].is_array() {
                d["preparadas"] = json!([]);
            }
        });
        self.fila_pronta.store(true, Ordering::SeqCst);
        self.log("ESTEIRA: preparação contínua; rodadas de 5 renders e 10 vozes (2 por vez); lote ao site assim que cada vídeo fica pronto");
    }

    /// Um ciclo da preparacao. Devolve quantos segundos esperar.
    pub fn passo_preparar(&self) -> u64 {
        if !self.ativo() {
            self.fila_pronta.store(false, Ordering::SeqCst);
            return 5;
        }
        let meta = self.comando().meta;
        let d = self.estado();
        let prontos = lista_de_numeros(&d, "prontos");
        if (prontos.len() as i64) < meta {
            self.fim_armado.store(true, Ordering::SeqCst);
        } else {
            return self.fim();
        }
        if self.python_vivo() {
            self.avisar_uma_vez("py_vivo", "orquestrador Python ainda vivo: o laço do Braniac não pega passo novo");
            return 30;
        }
        if self.existe_processo("braniac.link.etcm") || self.existe_processo("passo_esteira.py preparar") {
            return 30; // dois baixadores no TCM se atropelam (o preparo de antes de um reinicio ainda roda)
        }
        if !self.fila_pronta.load(Ordering::SeqCst) {
            self.preparar_fila();
        }
        let usadas: Vec<String> = lista_de_numeros(&d, "feitas").into_iter().chain(itens(&d, "preparadas").into_iter().map(|x| x.n)).chain(itens(&d, "voz_ok").into_iter().map(|x| x.n)).collect();
        let abaixo = itens(&d, "preparadas").len() + itens(&d, "voz_ok").len() + prontos.len() < meta as usize;
        let (prontos_s, usadas_s) = (prontos.join(","), usadas.join(","));
        let cabeca = travar(&self.fila).first().cloned();
        let mut a: Vec<String> = ["proxima", "--prontos", &prontos_s, "--usadas", &usadas_s, "--abaixo", if abaixo { "1" } else { "0" }].iter().map(|s| s.to_string()).collect();
        if let Some(c) = &cabeca {
            a.extend(["--fila-n".into(), c.n.clone(), "--fila-nome".into(), c.nome.clone(), "--fila-pid".into(), c.pid.clone().unwrap_or_default()]);
        }
        let r = match self.py(&a.iter().map(String::as_str).collect::<Vec<_>>(), 600) {
            Ok(r) => r,
            Err(e) => {
                self.log(&format!("não consegui escolher a próxima cidade: {}", corta(&e, 150)));
                return 120;
            }
        };
        for l in r["log"].as_array().into_iter().flatten().filter_map(|l| l.as_str()) {
            self.log(l);
        }
        let Some(n) = r["n"].as_str().map(String::from) else { return 120 };
        let item = Item { n: n.clone(), nome: r["nome"].as_str().unwrap_or("").into(), pid: r["pid"].as_str().map(String::from) };
        let (prioridade, da_fila) = (r["prioridade"].as_bool().unwrap_or(false), r["da_fila"].as_bool().unwrap_or(false));
        if r["refazer"].as_bool().unwrap_or(false) {
            self.mexer(|d| {
                if let Some(a) = d["prontos"].as_array_mut() {
                    a.retain(|x| x.as_str() != Some(&n));
                }
            });
        }
        if prioridade {
            self.mexer(|d| {
                if let Some(a) = d["feitas"].as_array_mut() {
                    a.retain(|x| x.as_str() != Some(&n));
                }
            });
        }
        if da_fila {
            let mut f = travar(&self.fila);
            if !f.is_empty() {
                f.remove(0);
            }
        }
        if r["ja_no_site"].as_bool().unwrap_or(false) {
            self.log(&format!("JÁ NO SITE {} {} - não refaz", item.n, item.nome));
            return 0;
        }
        self.log(&format!("PREPARANDO {} {}", item.n, item.nome));
        self.mexer(|d| d["atual"] = item.json());
        let ok = match self.py(&["preparar", &item.n, &item.nome, item.pid.as_deref().unwrap_or("")], 4 * 3600 + 600) {
            Ok(p) => {
                let mut pl = travar(&self.pend_lote);
                for x in p["pendencias"].as_array().into_iter().flatten().filter_map(|x| x.as_str()) {
                    if !pl.iter().any(|y| y == x) {
                        pl.push(x.to_string());
                    }
                }
                p["ok"].as_bool().unwrap_or(false)
            }
            Err(e) => {
                self.pendencia(&item.n, &item.nome, &corta(&format!("erro inesperado: {e}"), 300));
                false
            }
        };
        self.mexer(|d| depois_do_preparo(d, &item, ok, prioridade));
        0
    }

    /// Meta batida: manda o que sobrou e avisa o Xandao, uma vez. So quando nada esta no meio.
    fn fim(&self) -> u64 {
        if !self.fim_armado.load(Ordering::SeqCst) {
            return 30;
        }
        if travar(&self.render_atual).is_some() || !travar(&self.em_voz).is_empty() {
            return 30;
        }
        self.enviar_lote();
        let n = lista_de_numeros(&self.estado(), "prontos").len();
        let _ = self.py(&["avisar_xandao", &format!("[Braniac · Claudinho] Esteira terminou: {n} vídeos prontos e entregues pelo site.")], 120);
        self.log(&format!("FIM {n} prontos"));
        self.fim_armado.store(false, Ordering::SeqCst);
        30
    }

    /// Estado para a rota GET: ligada?, fase, quem esta em voz, render atual, placar e as ultimas 20 linhas do log.
    pub fn resumo(&self) -> Value {
        let d = self.estado();
        let (modo, feitos) = fase_de(&d);
        let log = std::fs::read(self.arq_log()).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
        let linhas: Vec<&str> = log.lines().collect();
        let c = self.comando();
        json!({
            "ligada": self.ligada(),
            "parado": c.parado,
            "fase": {"modo": modo, "feitos": feitos},
            "em_voz": travar(&self.em_voz).iter().collect::<Vec<_>>(),
            "render_atual": *travar(&self.render_atual),
            "python_vivo": travar(&self.vivo).map(|(_, v)| v),
            "placar": {"prontos": lista_de_numeros(&d, "prontos").len(), "meta": c.meta, "voz_ok": itens(&d, "voz_ok").len(),
                       "preparadas": itens(&d, "preparadas").len(), "lote_site": itens(&d, "lote_site").len()},
            "log": linhas[linhas.len().saturating_sub(20)..],
        })
    }
}


// ---------------------------------------------------------------- ligacao com o servidor

static MAESTRO: OnceLock<Arc<Maestro>> = OnceLock::new();

/// Sobe as quatro linhas (2 de voz, render, preparacao), uma vez. Elas dormem enquanto `esteira_ligada` nao existir.
pub fn garantir(dados: &Path) -> Arc<Maestro> {
    MAESTRO
        .get_or_init(|| {
            let m = Arc::new(Maestro::real(dados));
            iniciar(&m);
            m
        })
        .clone()
}

pub fn iniciar(m: &Arc<Maestro>) {
    type Passo = fn(&Maestro) -> u64;
    let linhas: [(&str, u64, Passo); 4] = [("voz1", 20, Maestro::passo_voz), ("voz2", 40, Maestro::passo_voz), ("render", 0, Maestro::passo_render), ("preparar", 0, Maestro::passo_preparar)];
    for (nome, atraso, passo) in linhas {
        let m = m.clone();
        let _ = std::thread::Builder::new().name(format!("esteira-{nome}")).spawn(move || {
            m.dorme(atraso); // as duas vozes nao comecam juntas
            loop {
                // erro em um passo vira log e nunca derruba o laco
                let s = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| passo(&m))).unwrap_or_else(|_| {
                    m.log(&format!("erro inesperado na linha {nome}; segue em 60 s"));
                    60
                });
                m.dorme(s);
            }
        });
    }
}

/// Relogio de 60 s: so garante as linhas no ar quando a esteira esta ligada.
pub fn tick(dados: &Path) {
    if Config::real(dados).ligada.exists() {
        garantir(dados);
    }
}

pub fn estado_http(dados: &Path) -> Value {
    match MAESTRO.get() {
        Some(m) => m.resumo(),
        None => Maestro::real(dados).resumo(),
    }
}

/// POST {"ligar": bool}: grava ou apaga o arquivo. Desligar = nao pega passo novo (o que esta no meio termina).
pub fn ligar_http(dados: &Path, v: &Value) -> Result<Value, String> {
    let ligar = v["ligar"].as_bool().ok_or("faltou {\"ligar\": true|false}")?;
    let arq = Config::real(dados).ligada;
    if ligar {
        std::fs::write(&arq, "ligada\n").map_err(|e| e.to_string())?;
        garantir(dados).log("ESTEIRA do Braniac LIGADA (esteira_ligada criado)");
    } else {
        let _ = std::fs::remove_file(&arq);
        if let Some(m) = MAESTRO.get() {
            m.log("ESTEIRA do Braniac DESLIGADA: não pega passo novo; o que está no meio termina");
        }
    }
    Ok(estado_http(dados))
}

// ---------------------------------------------------------------- testes

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn tmp(nome: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("braniac-esteira_orq-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn cfg_teste(base: &Path) -> Config {
        Config {
            pasta: base.join("magno"),
            pacote: base.join("pacote"),
            estudio: base.join("pacote").join("estudio-3d"),
            site: base.join("site"),
            envia: base.join("envia_site.py"),
            raiz: base.to_path_buf(),
            ajudante: base.join("passo_esteira.py"),
            backup: base.join("backup"),
            ligada: base.join("esteira_ligada"),
            python: "python".into(),
            segundo: Duration::from_millis(1),
        }
    }

    type Gravado = Arc<Mutex<Vec<String>>>;

    /// Maestro com processos falsos: `resp` decide a saida; tudo que foi chamado fica em `gravado`.
    fn maestro(base: &Path, resp: impl Fn(&Cmd) -> Saida + Send + Sync + 'static) -> (Maestro, Gravado) {
        let g: Gravado = Arc::new(Mutex::new(vec![]));
        let g2 = g.clone();
        let m = Maestro::novo(cfg_teste(base), Box::new(move |c| {
            g2.lock().unwrap().push(format!("{} {}", c.prog, c.args.join(" ")));
            resp(c)
        }));
        (m, g)
    }

    fn ok_json(t: &str) -> Saida {
        Saida { codigo: 0, texto: t.into() }
    }

    fn preparar_cidade(base: &Path, n: &str, projeto: &str, nome: &str) {
        let c = cfg_teste(base);
        std::fs::create_dir_all(c.pacote.join("videos").join(n)).unwrap();
        std::fs::write(c.pacote.join("videos").join(n).join("preparo.json"), json!({"projeto": projeto, "nome": nome, "comp": "Comp", "periodo": "2024-2026"}).to_string()).unwrap();
        let audio = c.estudio.join("projetos").join(projeto).join("audio");
        std::fs::create_dir_all(&audio).unwrap();
        std::fs::write(audio.join("fontes.md"), "fontes").unwrap();
        std::fs::write(audio.join("roteiro-falado.txt"), "roteiro").unwrap();
        std::fs::create_dir_all(c.estudio.join("edicoes").join(projeto)).unwrap();
    }

    fn sub(c: &Cmd) -> String {
        // ajudante: -X utf8 <script> <subcomando> ...
        c.args.get(3).cloned().unwrap_or_default()
    }

    #[test]
    fn ascii_e_pasta_do_site() {
        assert_eq!(ascii("CORAÇÃO DE MARIA"), "CORACAO DE MARIA");
        assert_eq!(ascii("Irecê"), "Irece");
        assert_eq!(nome_pasta_site("254", "CORAÇÃO DE MARIA"), "254-CORACAO-DE-MARIA");
    }

    #[test]
    fn meta_do_site_traz_periodo_padrao() {
        let m = meta_do_site("254", "Coração", &json!({"projeto": "p"}));
        assert_eq!(m["periodo"], "2024-2026");
        assert_eq!(m["numero"], "254");
    }

    #[test]
    fn fase_padrao_e_troca() {
        assert_eq!(fase_de(&json!({})), ("voz".into(), 0));
        assert_eq!(fase_de(&json!({"voz_ok": [["1", "A", null]]})).0, "render");
        let mut d = json!({"fase": {"modo": "voz", "feitos": 7}});
        assert_eq!(trocar_fase(&mut d, "render"), "FASE: 5 renders");
        assert_eq!(d["fase"], json!({"modo": "render", "feitos": 0}));
        assert_eq!(trocar_fase(&mut d, "voz"), "FASE: 10 vozes");
    }

    #[test]
    fn voz_menor_numero_primeiro_e_respeita_tentativas() {
        let d = json!({"preparadas": [["30", "C", null], ["9", "B", null], ["12", "A", null]], "voz_tentativas": {"9": 6}});
        let mut em = BTreeSet::new();
        assert_eq!(escolher_voz(&d, &em).unwrap().n, "12"); // 9 reprovou 6x: fora da vez
        em.insert("12".to_string());
        assert_eq!(escolher_voz(&d, &em).unwrap().n, "30");
    }

    #[test]
    fn voz_para_quando_a_rodada_fecha() {
        let d = json!({"preparadas": [["1", "A", null]], "fase": {"modo": "voz", "feitos": 9}});
        let mut em = BTreeSet::new();
        assert!(escolher_voz(&d, &em).is_some());
        em.insert("2".to_string()); // 9 feitas + 1 gravando = 10
        assert!(escolher_voz(&d, &em).is_none());
    }

    #[test]
    fn troca_para_render_so_sem_fila_e_sem_voz_gravando() {
        let vazio = BTreeSet::new();
        let com_voz = json!({"preparadas": [], "voz_ok": [["1", "A", null]]});
        assert!(trocar_para_render(&com_voz, &vazio));
        assert!(!trocar_para_render(&json!({"preparadas": [], "voz_ok": []}), &vazio));
        assert!(!trocar_para_render(&json!({"preparadas": [["2", "B", null]], "voz_ok": [["1", "A", null]]}), &vazio));
        let mut em = BTreeSet::new();
        em.insert("2".to_string());
        assert!(!trocar_para_render(&com_voz, &em));
    }

    #[test]
    fn depois_da_voz_move_para_voz_ok_e_fecha_a_rodada() {
        let mut d = json!({"preparadas": [["5", "X", "p"]], "fase": {"modo": "voz", "feitos": 9}});
        let l = depois_da_voz(&mut d, &Item { n: "5".into(), nome: "X".into(), pid: Some("p".into()) }, true, 1);
        assert_eq!(l.as_deref(), Some("FASE: 5 renders"));
        assert_eq!(d["voz_ok"], json!([["5", "X", "p"]]));
        assert_eq!(d["preparadas"], json!([]));
        assert_eq!(d["fase"]["modo"], "render");
    }

    #[test]
    fn depois_da_voz_com_outra_gravando_nao_troca_e_falha_conta_tentativa() {
        let mut d = json!({"preparadas": [["5", "X", null]], "fase": {"modo": "voz", "feitos": 9}});
        let i = Item { n: "5".into(), nome: "X".into(), pid: None };
        assert!(depois_da_voz(&mut d, &i, true, 2).is_none()); // a outra voz ainda grava
        assert_eq!(d["fase"]["modo"], "voz");
        depois_da_voz(&mut d, &i, false, 1);
        assert_eq!(d["voz_tentativas"]["5"], 1);
    }

    #[test]
    fn depois_do_render_lote_mt_e_rodada() {
        let mut d = json!({"voz_ok": [["10", "A", null], ["901", "MT", null]], "fase": {"modo": "render", "feitos": 3}});
        let (lote, l) = depois_do_render(&mut d, &Item { n: "10".into(), nome: "A".into(), pid: None }, true);
        assert!(lote && l.is_none());
        assert_eq!(d["lote_site"], json!([["10", "A"]]));
        let (_, l) = depois_do_render(&mut d, &Item { n: "901".into(), nome: "MT".into(), pid: None }, true);
        assert_eq!(l.as_deref(), Some("FASE: 10 vozes")); // acabou a fila de voz pronta
        assert_eq!(d["lote_site"], json!([["10", "A"]])); // MT nao vai ao site
        assert_eq!(d["prontos"], json!(["10", "901"]));
    }

    #[test]
    fn depois_do_preparo_prioridade_entra_em_segundo() {
        let mut d = json!({"preparadas": [["1", "A", null], ["2", "B", null]], "feitas": ["7"], "atual": ["7", "Z", null]});
        depois_do_preparo(&mut d, &Item { n: "7".into(), nome: "Z".into(), pid: None }, true, true);
        assert_eq!(d["preparadas"][1][0], "7");
        assert_eq!(d["feitas"], json!([]));
        assert!(d["atual"].is_null());
        depois_do_preparo(&mut d, &Item { n: "8".into(), nome: "W".into(), pid: None }, false, false);
        assert_eq!(d["feitas"], json!(["8"]));
    }

    #[test]
    fn aviso_do_lote_tem_pendencias_e_revisao() {
        let t = aviso_lote(&["1 A".into(), "2 B".into()], &["3 C".into()], &["1 A: x".into(), "2 B: y".into()]);
        assert!(t.contains("1 A, 2 B."));
        assert!(t.contains("Pendências desde o último lote (prefeitura sem documento no TCM ou soma que não fecha): 3 C."));
        assert!(t.ends_with("1 A: x | 2 B: y"));
        assert!(!aviso_lote(&["1 A".into()], &[], &[]).contains("Pendências"));
    }

    #[test]
    fn mexer_guarda_campos_desconhecidos_e_normaliza() {
        let base = tmp("mexer");
        let (m, _) = maestro(&base, |_| ok_json("{}"));
        std::fs::create_dir_all(&m.cfg.pasta).unwrap();
        std::fs::write(m.arq_estado(), r#"{"prontos":["1"],"campo_do_python":{"a":1}}"#).unwrap();
        m.mexer(|d| empurrar(d, "prontos", json!("2")));
        let lido: Value = serde_json::from_slice(&std::fs::read(m.arq_estado()).unwrap()).unwrap();
        assert_eq!(lido["prontos"], json!(["1", "2"]));
        assert_eq!(lido["campo_do_python"]["a"], 1);
        assert_eq!(lido["lote_site"], json!([]));
    }

    #[test]
    fn log_tem_o_formato_do_python() {
        let base = tmp("log");
        let (m, _) = maestro(&base, |_| ok_json("{}"));
        m.log("FASE: 10 vozes");
        let t = std::fs::read_to_string(m.arq_log()).unwrap();
        assert!(regex::Regex::new(r"^\d\d/\d\d \d\d:\d\d:\d\d FASE: 10 vozes\n$").unwrap().is_match(&t), "{t}");
    }

    #[test]
    fn comando_le_meta_parado_e_so_fila() {
        let base = tmp("cmd");
        let (m, _) = maestro(&base, |_| ok_json("{}"));
        assert_eq!((m.comando().meta, m.comando().parado), (50, false));
        std::fs::create_dir_all(&m.cfg.pasta).unwrap();
        std::fs::write(m.cfg.pasta.join("comando_esteira.json"), r#"{"meta": 12, "parado": true, "so_fila": true}"#).unwrap();
        let c = m.comando();
        assert_eq!((c.meta, c.parado, c.so_fila), (12, true, true));
        std::fs::write(&m.cfg.ligada, "x").unwrap();
        assert!(m.ligada() && !m.ativo()); // parado = nao comeca passo novo
    }

    #[test]
    fn desligada_nao_faz_nada() {
        let base = tmp("desl");
        let (m, g) = maestro(&base, |_| ok_json("{}"));
        assert_eq!((m.passo_voz(), m.passo_render(), m.passo_preparar()), (5, 5, 5));
        assert!(g.lock().unwrap().is_empty());
    }

    #[test]
    fn python_vivo_trava_o_laco_e_loga_uma_vez() {
        let base = tmp("vivo");
        let (m, g) = maestro(&base, |c| if c.prog == "powershell" { ok_json("1\n") } else { ok_json("{}") });
        std::fs::write(&m.cfg.ligada, "x").unwrap();
        m.mexer(|d| d["fase"] = json!({"modo": "render", "feitos": 0}));
        assert_eq!(m.passo_render(), 30);
        assert_eq!(m.passo_render(), 30);
        let log = std::fs::read_to_string(m.arq_log()).unwrap();
        assert_eq!(log.matches("orquestrador Python ainda vivo").count(), 1);
        assert!(g.lock().unwrap().iter().all(|c| !c.starts_with("node")));
    }

    fn resposta_ajudante(c: &Cmd) -> Saida {
        match c.prog.as_str() {
            "powershell" => ok_json("0\n"),
            "python" if c.args.get(2).is_some_and(|a| a.ends_with("passo_esteira.py")) => match sub(c).as_str() {
                "cota" => ok_json("ruído\n{\"ok\": true}\n"),
                "voz" => ok_json("{\"r\": true}\n"),
                "bloqueia" => ok_json("{\"bloqueia\": {\"%\": false}}\n"),
                "pdfs" => ok_json("{\"qtd\": 3}\n"),
                _ => ok_json("{\"ok\": true}\n"),
            },
            _ => ok_json(""),
        }
    }

    #[test]
    fn passo_voz_grava_e_move_para_voz_ok() {
        let base = tmp("voz");
        let (m, g) = maestro(&base, resposta_ajudante);
        std::fs::write(&m.cfg.ligada, "x").unwrap();
        m.mexer(|d| d["preparadas"] = json!([["20", "B", null], ["10", "A", null]]));
        assert_eq!(m.passo_voz(), 0);
        let d = m.estado();
        assert_eq!(d["voz_ok"], json!([["10", "A", null]])); // menor numero primeiro
        assert_eq!(d["fase"]["feitos"], 1);
        assert!(g.lock().unwrap().iter().any(|c| c.contains("passo_esteira.py voz 10 A")));
        assert!(m.em_voz.lock().unwrap().is_empty());
    }

    #[test]
    fn passo_voz_sem_cota_espera_e_devolve_a_cidade() {
        let base = tmp("cota");
        let (m, _) = maestro(&base, |c| if c.args.get(3).is_some_and(|a| a == "cota") { ok_json("{\"ok\": false}") } else { resposta_ajudante(c) });
        std::fs::write(&m.cfg.ligada, "x").unwrap();
        m.mexer(|d| d["preparadas"] = json!([["10", "A", null]]));
        assert_eq!(m.passo_voz(), 600);
        assert_eq!(m.estado()["preparadas"], json!([["10", "A", null]]));
        assert!(m.em_voz.lock().unwrap().is_empty());
    }

    #[test]
    fn passo_voz_reprovada_conta_tentativa() {
        let base = tmp("rep");
        let (m, _) = maestro(&base, |c| if c.args.get(3).is_some_and(|a| a == "voz") { ok_json("{\"r\": null}") } else { resposta_ajudante(c) });
        std::fs::write(&m.cfg.ligada, "x").unwrap();
        m.mexer(|d| d["preparadas"] = json!([["10", "A", null]]));
        assert_eq!(m.passo_voz(), 120);
        assert_eq!(m.estado()["voz_tentativas"]["10"], 1);
        assert_eq!(m.estado()["preparadas"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn passo_voz_sem_fila_troca_para_render() {
        let base = tmp("troca");
        let (m, _) = maestro(&base, resposta_ajudante);
        std::fs::write(&m.cfg.ligada, "x").unwrap();
        m.mexer(|d| {
            d["fase"] = json!({"modo": "voz", "feitos": 2});
            d["voz_ok"] = json!([["10", "A", null]]);
        });
        assert_eq!(m.passo_voz(), 30);
        assert_eq!(m.estado()["fase"]["modo"], "render");
    }

    #[test]
    fn render_completo_com_processos_falsos() {
        let base = tmp("render");
        preparar_cidade(&base, "254", "254. orçamento de coração", "CORAÇÃO DE MARIA");
        let c = cfg_teste(&base);
        let (pacote, ed) = (c.pacote.clone(), c.estudio.join("edicoes").join("254. orçamento de coração"));
        let nodes = Arc::new(AtomicUsize::new(0));
        let (n2, ed2) = (nodes.clone(), ed.clone());
        let (m, g) = maestro(&base, move |cm| match cm.prog.as_str() {
            "node" => {
                n2.fetch_add(1, Ordering::SeqCst);
                assert!(cm.cwd.ends_with("pacote")); // renderizar.mjs roda com cwd no PACOTE
                assert!(cm.cwd.join(".render-trava").exists()); // com a trava de diretorio tomada
                std::fs::write(ed2.join("CORAÇÃO DE MARIA.mp4"), "mp4").unwrap();
                std::fs::write(ed2.join("CORAÇÃO DE MARIA - capa.jpg"), "capa").unwrap();
                std::fs::write(ed2.join("CORAÇÃO DE MARIA - legenda.txt"), "leg").unwrap();
                ok_json("ATENÇÃO: voz estourou\nquadros ok\nERRO: um quadro\n")
            }
            "ffmpeg" => {
                std::fs::write(cm.args.last().unwrap(), "mp4-limitado").unwrap();
                ok_json("")
            }
            "python" if cm.args.first().is_some_and(|a| a.ends_with("envia_site.py")) => ok_json("Recebido: ok"),
            _ => resposta_ajudante(cm),
        });
        // restos de render antigo e satelite
        std::fs::create_dir_all(m.cfg.estudio.join("projetos/254. orçamento de coração/a/.partes-1")).unwrap();
        std::fs::write(&m.cfg.ligada, "x").unwrap();
        m.mexer(|d| {
            d["fase"] = json!({"modo": "render", "feitos": 0});
            d["voz_ok"] = json!([["254", "CORAÇÃO DE MARIA", null]]);
        });
        assert_eq!(m.passo_render(), 0);
        assert_eq!(nodes.load(Ordering::SeqCst), 1);
        assert!(!pacote.join(".render-trava").exists(), "trava solta");
        assert!(!m.cfg.estudio.join("projetos/254. orçamento de coração/a/.partes-1").exists());
        let d = m.estado();
        assert_eq!(d["prontos"], json!(["254"]));
        assert_eq!(d["enviados"], json!(["254 CORAÇÃO DE MARIA"]));
        assert_eq!(d["lote_site"], json!([]));
        assert_eq!(d["voz_ok"], json!([]));
        // pasta do site completa
        let site = m.cfg.site.join("254-CORACAO-DE-MARIA");
        for f in ["CORAÇÃO DE MARIA.mp4", "CORAÇÃO DE MARIA - capa.jpg", "CORAÇÃO DE MARIA - legenda.txt", "254. orçamento de coração - fontes.md", "meta.json"] {
            assert!(site.join(f).exists(), "{f}");
        }
        assert_eq!(std::fs::read_to_string(site.join("CORAÇÃO DE MARIA.mp4")).unwrap(), "mp4-limitado");
        // pasta de edicao marcada CONCLUÍDO; avisos viraram REVISAR antes disso
        assert!(!ed.exists());
        let concl = m.cfg.estudio.join("edicoes").join("CONCLUÍDO 254. orçamento de coração");
        assert!(concl.join("CORAÇÃO DE MARIA - REVISAR.txt").exists());
        assert!(concl.join("CORAÇÃO DE MARIA - fontes.md").exists());
        let log = std::fs::read_to_string(m.arq_log()).unwrap();
        assert!(log.contains("PRONTO COM AVISO") && log.contains("SITE 254 CORAÇÃO DE MARIA ok") && log.contains("PRONTO 254"));
        let chamadas = g.lock().unwrap().join("\n");
        assert!(chamadas.contains("renderizar.mjs videos/254/video.json --nucleos=6 --gl=angle --quadros-por-parte=600"));
        assert!(chamadas.contains("envia_site.py") && chamadas.contains("--pdfs") && chamadas.contains("avisar_xandao"));
        assert!(std::fs::read_to_string(pacote.join("videos/254/render.log")).unwrap().contains("ERRO"));
    }

    #[test]
    fn render_sem_mp4_vira_pendencia_e_solta_a_trava() {
        let base = tmp("semmp4");
        preparar_cidade(&base, "9", "p9", "NOVE");
        let (m, g) = maestro(&base, |c| if c.prog == "node" { ok_json("ERRO: deu ruim\n") } else { resposta_ajudante(c) });
        let r = m.renderizar("9", "Nove", None).unwrap();
        assert!(!r);
        assert!(!m.cfg.pacote.join(".render-trava").exists());
        assert!(g.lock().unwrap().iter().any(|c| c.contains("passo_esteira.py pendencia 9 Nove render não gerou o vídeo: ERRO: deu ruim")));
        assert_eq!(*m.pend_lote.lock().unwrap(), vec!["9 Nove".to_string()]);
    }

    #[test]
    fn render_espera_a_trava_existente_e_nunca_dois() {
        let base = tmp("trava");
        preparar_cidade(&base, "9", "p9", "NOVE");
        let trava = base.join("pacote").join(".render-trava");
        let t1 = trava.clone();
        // trava alheia com um render vivo (previa, outro video): o powershell ve "renderizar.mjs" enquanto a trava existe
        let (m, _) = maestro(&base, move |c| match c.prog.as_str() {
            "node" => ok_json(""),
            "powershell" if c.args.join(" ").contains("*renderizar.mjs*") && t1.exists() => ok_json("1
"),
            "powershell" => ok_json("0
"),
            _ => resposta_ajudante(c),
        });
        assert_eq!(m.cfg.pacote.join(".render-trava"), trava);
        std::fs::create_dir_all(&trava).unwrap();
        let t2 = trava.clone();
        let h = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            std::fs::remove_dir(t2).unwrap();
        });
        let _ = m.renderizar("9", "Nove", None); // espera a trava alheia sumir (sem mp4 = pendencia, nao importa)
        h.join().unwrap();
        assert!(!trava.exists());
    }

    #[test]
    fn trava_sobrando_sem_render_vivo_e_tirada() {
        let base = tmp("trava_velha");
        preparar_cidade(&base, "9", "p9", "NOVE");
        let (m, g) = maestro(&base, |c| match c.prog.as_str() {
            "node" => ok_json(""),
            "powershell" => ok_json("0
"),
            _ => resposta_ajudante(c),
        });
        std::fs::create_dir_all(m.cfg.pacote.join(".render-trava")).unwrap(); // B.R.A.N.I morreu no meio do render
        let _ = m.renderizar("9", "Nove", None);
        assert!(g.lock().unwrap().iter().any(|c| c.starts_with("node")));
        assert!(!m.cfg.pacote.join(".render-trava").exists());
    }

    #[test]
    fn trava_com_log_fresco_de_outro_video_nao_e_tirada() {
        let base = tmp("trava_log");
        preparar_cidade(&base, "9", "p9", "NOVE");
        let (m, _) = maestro(&base, |c| match c.prog.as_str() {
            "node" => ok_json(""),
            "powershell" => ok_json("0
"), // a lista de processos falhou em ver o node
            _ => resposta_ajudante(c),
        });
        let outro = m.cfg.pacote.join("videos").join("8");
        std::fs::create_dir_all(&outro).unwrap();
        std::fs::write(outro.join("render.log"), "Rendered 10/600").unwrap(); // mas o log dele esta vivo
        let trava = m.cfg.pacote.join(".render-trava");
        std::fs::create_dir_all(&trava).unwrap();
        let t2 = trava.clone();
        let h = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            std::fs::remove_dir(t2).unwrap(); // so quem tomou a trava a solta
        });
        let _ = m.renderizar("9", "Nove", None);
        h.join().unwrap();
    }

    #[test]
    fn render_vivo_de_antes_do_reinicio_e_adotado() {
        let base = tmp("adota");
        preparar_cidade(&base, "9", "p9", "NOVE");
        let vezes = Arc::new(AtomicUsize::new(0));
        let v2 = vezes.clone();
        // o node do video 9 aparece vivo nas 2 primeiras olhadas e depois termina
        let (m, g) = maestro(&base, move |c| match c.prog.as_str() {
            "node" => ok_json(""),
            "powershell" if c.args.join(" ").contains("videos/9/video.json") => ok_json(if v2.fetch_add(1, Ordering::SeqCst) < 2 { "1
" } else { "0
" }),
            "powershell" => ok_json("0
"),
            _ => resposta_ajudante(c),
        });
        let _ = m.renderizar("9", "Nove", None);
        assert!(vezes.load(Ordering::SeqCst) >= 3);
        assert!(!g.lock().unwrap().iter().any(|c| c.starts_with("node"))); // nao comecou outro render por cima
        assert!(std::fs::read_to_string(m.arq_log()).unwrap().contains("render já rodando"));
    }

    #[test]
    fn erro_no_render_devolve_err_para_tentar_de_novo() {
        let base = tmp("erro");
        let (m, _) = maestro(&base, resposta_ajudante);
        std::fs::write(&m.cfg.ligada, "x").unwrap();
        m.mexer(|d| {
            d["fase"] = json!({"modo": "render", "feitos": 0});
            d["voz_ok"] = json!([["9", "N", null]]); // sem preparo.json
        });
        assert_eq!(m.passo_render(), 300);
        assert_eq!(m.estado()["voz_ok"].as_array().unwrap().len(), 1); // continua na fila
    }

    #[test]
    fn site_que_nao_recebeu_nao_entra_em_enviados() {
        let base = tmp("sitefalha");
        preparar_cidade(&base, "9", "p9", "NOVE");
        let ed = cfg_teste(&base).estudio.join("edicoes").join("p9");
        for f in ["NOVE.mp4", "NOVE - capa.jpg", "NOVE - legenda.txt"] {
            std::fs::write(ed.join(f), "x").unwrap();
        }
        let (m, _) = maestro(&base, |c| if c.args.first().is_some_and(|a| a.ends_with("envia_site.py")) { ok_json("erro 500") } else { resposta_ajudante(c) });
        assert!(m.mandar_site(&[("9".into(), "Nove".into())]).is_empty());
        assert!(ed.exists(), "sem confirmação do site a pasta não vira CONCLUÍDO");
    }

    #[test]
    fn pular_feitos_tira_quem_ja_esta_no_backup() {
        let base = tmp("backup");
        preparar_cidade(&base, "9", "p9", "NOVE");
        let (m, _) = maestro(&base, resposta_ajudante);
        std::fs::create_dir_all(m.cfg.backup.join("p9")).unwrap();
        std::fs::write(m.cfg.backup.join("p9").join("NOVE.mp4"), "x").unwrap();
        m.mexer(|d| d["preparadas"] = json!([["9", "Nove", null]]));
        m.pular_feitos();
        let d = m.estado();
        assert_eq!((d["preparadas"].clone(), d["prontos"].clone(), d["lote_site"].clone()), (json!([]), json!(["9"]), json!([["9", "Nove"]])));
    }

    #[test]
    fn preparacao_aplica_o_que_o_python_devolve() {
        let base = tmp("prep");
        let (m, g) = maestro(&base, |c| match sub(c).as_str() {
            "proxima" if c.prog == "python" => ok_json("{\"n\": \"300\", \"nome\": \"Cidade\", \"pid\": null, \"prioridade\": true, \"da_fila\": false, \"refazer\": true, \"ja_no_site\": false, \"log\": [\"PRIORIDADE 300 Cidade (pendência resolvida)\"]}"),
            "preparar" => ok_json("{\"ok\": true, \"pendencias\": [\"1 X\"]}"),
            _ => resposta_ajudante(c),
        });
        std::fs::write(&m.cfg.ligada, "x").unwrap();
        m.mexer(|d| {
            d["prontos"] = json!(["300", "5"]);
            d["feitas"] = json!(["300", "5", "141"]);
            d["preparadas"] = json!([["1", "A", null], ["2", "B", null]]);
        });
        assert_eq!(m.passo_preparar(), 0);
        let d = m.estado();
        assert_eq!(d["prontos"], json!(["5"])); // "refazer": sai de prontos
        assert_eq!(d["preparadas"][1][0], "300"); // prioridade: logo depois da que esta na voz
        assert!(!d["feitas"].as_array().unwrap().contains(&json!("300")));
        assert!(d["atual"].is_null());
        assert_eq!(*m.pend_lote.lock().unwrap(), vec!["1 X".to_string()]);
        let log = std::fs::read_to_string(m.arq_log()).unwrap();
        assert!(log.contains("PRIORIDADE 300 Cidade (pendência resolvida)") && log.contains("PREPARANDO 300 Cidade"));
        let chamadas = g.lock().unwrap().join("\n");
        assert!(chamadas.contains("--prontos 300,5") && chamadas.contains("--abaixo 1"));
    }

    #[test]
    fn preparacao_que_falha_vira_pendencia_e_feita() {
        let base = tmp("prepfalha");
        let (m, _) = maestro(&base, |c| match sub(c).as_str() {
            "proxima" if c.prog == "python" => ok_json("{\"n\": \"300\", \"nome\": \"C\", \"pid\": null, \"prioridade\": false, \"da_fila\": false, \"refazer\": false, \"ja_no_site\": false, \"log\": []}"),
            "preparar" => ok_json("{\"erro\": \"quebrou\"}"),
            _ => resposta_ajudante(c),
        });
        std::fs::write(&m.cfg.ligada, "x").unwrap();
        m.mexer(|d| d["feitas"] = json!(["141"]));
        assert_eq!(m.passo_preparar(), 0);
        assert_eq!(m.estado()["feitas"], json!(["141", "300"]));
        assert!(std::fs::read_to_string(m.arq_log()).unwrap().contains("PENDÊNCIA 300 C - erro inesperado: quebrou"));
    }

    #[test]
    fn preparacao_sem_cidade_espera_e_ja_no_site_pula() {
        let base = tmp("prepvazio");
        let (m, _) = maestro(&base, |c| match sub(c).as_str() {
            "proxima" if c.prog == "python" => ok_json("{\"log\": []}"),
            _ => resposta_ajudante(c),
        });
        std::fs::write(&m.cfg.ligada, "x").unwrap();
        m.mexer(|d| d["feitas"] = json!(["141"]));
        assert_eq!(m.passo_preparar(), 120);
        let (m2, _) = maestro(&tmp("prepsite"), |c| match sub(c).as_str() {
            "proxima" if c.prog == "python" => ok_json("{\"n\": \"7\", \"nome\": \"S\", \"pid\": null, \"prioridade\": false, \"da_fila\": false, \"refazer\": false, \"ja_no_site\": true, \"log\": []}"),
            _ => resposta_ajudante(c),
        });
        std::fs::write(&m2.cfg.ligada, "x").unwrap();
        m2.mexer(|d| d["feitas"] = json!(["141"]));
        assert_eq!(m2.passo_preparar(), 0);
        assert!(std::fs::read_to_string(m2.arq_log()).unwrap().contains("JÁ NO SITE 7 S - não refaz"));
        assert!(m2.estado()["atual"].is_null());
    }

    #[test]
    fn fila_inicial_tem_irece_e_retoma_o_atual() {
        let base = tmp("fila");
        let (m, _) = maestro(&base, resposta_ajudante);
        m.preparar_fila();
        assert_eq!(m.fila.lock().unwrap()[0].nome, "Irecê");
        m.mexer(|d| d["atual"] = json!(["77", "Meio", null]));
        m.preparar_fila();
        assert_eq!(*m.fila.lock().unwrap(), vec![Item { n: "77".into(), nome: "Meio".into(), pid: None }]);
    }

    #[test]
    fn meta_batida_manda_o_resto_e_avisa_uma_vez() {
        let base = tmp("fim");
        let (m, g) = maestro(&base, resposta_ajudante);
        std::fs::write(&m.cfg.ligada, "x").unwrap();
        std::fs::create_dir_all(&m.cfg.pasta).unwrap();
        std::fs::write(m.cfg.pasta.join("comando_esteira.json"), r#"{"meta": 1}"#).unwrap();
        m.mexer(|d| d["prontos"] = json!(["1"]));
        m.fim_armado.store(true, Ordering::SeqCst);
        assert_eq!(m.passo_preparar(), 30);
        assert_eq!(m.passo_preparar(), 30);
        let n = g.lock().unwrap().iter().filter(|c| c.contains("Esteira terminou")).count();
        assert_eq!(n, 1);
    }

    #[test]
    fn resumo_mostra_placar_e_ultimas_linhas() {
        let base = tmp("resumo");
        let (m, _) = maestro(&base, resposta_ajudante);
        for i in 0..25 {
            m.log(&format!("linha {i}"));
        }
        m.mexer(|d| d["prontos"] = json!(["1", "2"]));
        let r = m.resumo();
        assert_eq!(r["ligada"], false);
        assert_eq!(r["placar"]["prontos"], 2);
        assert_eq!(r["log"].as_array().unwrap().len(), 20);
        assert!(r["log"][19].as_str().unwrap().ends_with("linha 24"));
    }

    #[test]
    fn ajudante_le_a_ultima_linha_json_e_o_erro() {
        let base = tmp("py");
        let (m, _) = maestro(&base, |_| ok_json("lixo\n{\"a\": 1}\ntexto depois\n"));
        assert_eq!(m.py(&["x"], 1).unwrap()["a"], 1);
        let (m, _) = maestro(&base, |_| ok_json("nada de json"));
        assert!(m.py(&["x"], 1).is_err());
        let (m, _) = maestro(&base, |_| ok_json("{\"erro\": \"deu ruim\"}"));
        assert_eq!(m.py(&["x"], 1).unwrap_err(), "deu ruim");
    }
}
