//! NexoDev ligado ao Braniac em Rust: o que faltava para o Claude (senior) passar tarefas ao time e receber o resultado.
//! Motores de geracao em ordem (`motores` do equipe.json): ChatGPT (projeto NexoDev) e DeepSeek pelas abas logadas da VM, depois o roteador
//! de IAs gratis. Quem gerou fica no marcador `<!-- motor: ... -->` da entrega, em `motores` do ciclo e no quadro: nunca falha calado.
//! Cada ticket tem a branch `tNNN` no git da pasta do projeto (dados/empresa_dev/projetos/<proj>/); o QA exporta a branch para uma pasta
//! temporaria e roda cargo build+test (ou go); `aprovar` faz o merge na main DO PROJETO (nada vai para o codigo do Brani sozinho).
//! Rotas (POST exige X-Braniac, conferido em main.rs): GET /nexodev, /api/nexodev/quadro|motores|ticket/<id>;
//! POST /api/nexodev/abrir|trabalhar|aprovar|devolver|parecer|testar.
use crate::empresa_dev::{self as ed, cauda, corta};
use crate::{abas, barramento, cdp, fatos, ia_web, oculto, tempo, vm, Resp};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

type R = Result<String, String>;
type Ia<'a> = &'a dyn Fn(&str, &str) -> R;

/// Motores que responderam no ciclo em andamento (o ciclo inteiro roda numa thread so).
thread_local! {
    static MOTORES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}

fn pasta(dados: &Path) -> PathBuf {
    dados.join("empresa_dev")
}

fn nota_motor(txt: String) {
    MOTORES.with(|m| m.borrow_mut().push(txt));
}

// ---------------------------------------------------------------- motores de geracao

fn agora_s() -> f64 {
    fatos::agora_ts()
}

/// Pergunta ao site de IA pela aba logada dele na VM. Erro claro (deslogado, captcha, sem aba, tempo) em vez de resposta vazia.
fn web(dados: &Path, nome: &str, texto: &str, projeto_url: Option<&str>) -> R {
    if ia_web::site(nome).is_none() {
        return Err(format!("IA desconhecida: {nome}"));
    }
    let id = abas::aba_do_agente(dados, nome).map_err(|e| format!("sem aba do {nome} na VM: {e}"))?;
    let aba = RefCell::new(cdp::Aba::conectar(vm::CDP.trim_start_matches("http://"), &id)?);
    let js = |e: &str| aba.borrow_mut().comando("Runtime.evaluate", json!({ "expression": e, "returnByValue": true })).and_then(|r| cdp::valor_avaliado(&r));
    let navegar = |u: &str| aba.borrow_mut().navegar(u);
    let frente = || drop(aba.borrow_mut().comando("Page.bringToFront", json!({})));
    let digitar = |t: &str| aba.borrow_mut().digitar(t, false);
    let enter = || -> Result<(), String> {
        let tecla = json!({ "key": "Enter", "code": "Enter", "windowsVirtualKeyCode": 13, "nativeVirtualKeyCode": 13 });
        for (tipo, txt) in [("keyDown", "\r"), ("keyUp", "")] {
            let mut t = tecla.clone();
            t["type"] = json!(tipo);
            if !txt.is_empty() {
                t["text"] = json!(txt);
            }
            aba.borrow_mut().comando("Input.dispatchKeyEvent", t)?;
        }
        Ok(())
    };
    let esperar = |seg: f64| std::thread::sleep(Duration::from_secs_f64(seg));
    let sem_foco = || {};
    let a = ia_web::Aba { js: &js, navegar: &navegar, frente: &frente, digitar: &digitar, enter: &enter, esperar: &esperar, relogio: &agora_s, devolver_foco: &sem_foco };
    let o = ia_web::Opcoes { timeout_s: 240.0, nova_conversa: true, projeto_url };
    ia_web::perguntar(nome, texto, &o, &|_| false, &a)
}

/// ChatGPT -> DeepSeek -> roteador gratis (`ia`). Cada falha entra no marcador da entrega.
fn motor(dados: &Path, ia: Ia, sistema: &str, prompt: &str) -> R {
    let cfg = ed::config(dados);
    let texto = format!("{sistema}\n\n{prompt}");
    let mut falhas: Vec<String> = vec![];
    for m in cfg["motores"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        let url = if m == "chatgpt" { cfg["gpt_projeto"].as_str() } else { None };
        match web(dados, m, &texto, url) {
            Ok(r) if !r.trim().is_empty() => return Ok(marcar(r, m, &falhas)),
            Ok(_) => falhas.push(format!("{m}: resposta vazia")),
            Err(e) => falhas.push(format!("{m}: {}", corta(&e, 140))),
        }
    }
    match ia(sistema, prompt) {
        Ok(r) => Ok(marcar(r, "roteador gratis", &falhas)),
        Err(e) => Err(format!("nenhum motor respondeu ({}; roteador gratis: {})", falhas.join("; "), corta(&e, 140))),
    }
}

fn marcar(texto: String, motor: &str, falhas: &[String]) -> String {
    let extra = if falhas.is_empty() { String::new() } else { format!(" (antes falharam: {})", falhas.join("; ")) };
    nota_motor(format!("{motor}{extra}"));
    format!("{texto}\n<!-- motor: {motor}{extra} -->")
}

// ---------------------------------------------------------------- git por ticket, QA e merge

fn git(dir: &Path, a: &[&str]) -> (i32, String) {
    let r = oculto::cmd("git").current_dir(dir).args(["-c", "user.name=NexoDev", "-c", "user.email=nexodev@braniac.local", "-c", "core.autocrlf=false"]).args(a).output();
    match r {
        Ok(o) => (o.status.code().unwrap_or(-1), format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))),
        Err(e) => (-1, format!("git indisponível: {e}")),
    }
}

fn ramo(t: &Value) -> String {
    format!("t{}", s(t, "id").to_lowercase())
}

fn preparar(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    if !dir.join(".git").exists() {
        let (c, o) = git(dir, &["init", "-q", "-b", "main"]);
        if c != 0 {
            return Err(format!("git init falhou: {}", cauda(&o, 200)));
        }
    }
    let _ = std::fs::write(dir.join(".git").join("info").join("exclude"), "T[0-9]*-*.md\n.commit_erro\n");
    if git(dir, &["rev-parse", "--verify", "-q", "HEAD"]).0 != 0 {
        git(dir, &["commit", "-q", "--allow-empty", "-m", "inicio"]);
    }
    Ok(())
}

/// Grava a entrega na branch do ticket. Falha vira `.commit_erro` (o QA le e reprova com o motivo).
fn commit(dados: &Path, t: &Value, arqs: &BTreeMap<String, String>) -> bool {
    let dir = ed::pasta_projeto(dados, t);
    let erro = dir.join(".commit_erro");
    let feito = (|| -> Result<(), String> {
        preparar(&dir)?;
        let b = ramo(t);
        let existe = git(&dir, &["rev-parse", "--verify", "-q", &b]).0 == 0;
        let (c, o) = if existe { git(&dir, &["checkout", "-q", &b]) } else { git(&dir, &["checkout", "-q", "-b", &b, "main"]) };
        if c != 0 {
            return Err(format!("checkout {b}: {}", cauda(&o, 200)));
        }
        for (nome, cont) in arqs {
            let p = dir.join(nome);
            if let Some(d) = p.parent() {
                std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
            }
            std::fs::write(&p, cont).map_err(|e| format!("{nome}: {e}"))?;
        }
        git(&dir, &["add", "-A"]);
        let (c, o) = git(&dir, &["commit", "-q", "--allow-empty", "-m", &format!("{} tentativa", s(t, "id"))]);
        if c != 0 {
            return Err(format!("commit: {}", cauda(&o, 200)));
        }
        Ok(())
    })();
    match feito {
        Ok(()) => {
            let _ = std::fs::remove_file(&erro);
            true
        }
        Err(e) => {
            let _ = std::fs::write(&erro, e);
            false
        }
    }
}

/// Roda o programa com teto de tempo; saida (stdout+stderr) num arquivo para nunca travar no pipe cheio.
fn rodar(dir: &Path, log: &Path, prog: &str, args: &[&str], alvo: Option<&Path>, seg: u64) -> (i32, String) {
    let Ok(f) = std::fs::File::create(log) else { return (-1, "sem pasta temporária".into()) };
    let (Ok(f2), mut c) = (f.try_clone(), oculto::cmd(prog)) else { return (-1, "sem pasta temporária".into()) };
    c.current_dir(dir).args(args).stdout(f).stderr(f2);
    if let Some(a) = alvo {
        c.env("CARGO_TARGET_DIR", a);
    }
    let mut filho = match c.spawn() {
        Ok(x) => x,
        Err(e) => return (-1, format!("não consegui rodar `{prog}` (está no PATH do Braniac?): {e}")),
    };
    let limite = Instant::now() + Duration::from_secs(seg);
    let codigo = loop {
        match filho.try_wait() {
            Ok(Some(st)) => break st.code().unwrap_or(-1),
            Ok(None) if Instant::now() < limite => std::thread::sleep(Duration::from_millis(400)),
            _ => {
                let _ = filho.kill();
                let _ = filho.wait();
                break 124;
            }
        }
    };
    let txt = std::fs::read_to_string(log).unwrap_or_default();
    (codigo, if codigo == 124 { format!("tempo esgotado ({seg}s) em `{prog}`. {}", cauda(&txt, 300)) } else { txt })
}

/// QA de codigo: exporta a branch para uma pasta temporaria e compila/testa. (Some(true)/(Some(false)/None = nada compilavel), log).
fn build(dados: &Path, t: &Value) -> (Option<bool>, String) {
    let dir = ed::pasta_projeto(dados, t);
    if let Ok(e) = std::fs::read_to_string(dir.join(".commit_erro")) {
        return (Some(false), format!("não consegui gravar a entrega no git: {e}"));
    }
    let b = ramo(t);
    if git(&dir, &["rev-parse", "--verify", "-q", &b]).0 != 0 {
        return (Some(false), "a entrega não trouxe nenhum arquivo no formato `=== ARQUIVO: caminho ===` ... `=== FIM ===`; nada foi gravado".into());
    }
    if git(&dir, &["diff", "--name-only", &format!("main...{b}")]).1.trim().is_empty() {
        return (Some(false), "a entrega não trouxe nenhum arquivo no formato `=== ARQUIVO: caminho ===` ... `=== FIM ===` (nada para compilar)".into());
    }
    let tmp = pasta(dados).join(format!(".qa-{}", s(t, "id")));
    let _ = std::fs::remove_dir_all(&tmp);
    let codigo = tmp.join("code");
    let (tar, log) = (tmp.join("src.tar"), tmp.join("log.txt"));
    let r = (|| {
        std::fs::create_dir_all(&codigo).map_err(|e| e.to_string())?;
        let (c, o) = git(&dir, &["archive", "--format=tar", "-o", &tar.to_string_lossy(), &b]);
        if c != 0 {
            return Err(format!("git archive: {}", cauda(&o, 200)));
        }
        let (c, o) = rodar(&tmp, &log, "tar", &["-xf", "src.tar", "-C", "code"], None, 60);
        if c != 0 {
            return Err(format!("tar: {}", cauda(&o, 200)));
        }
        Ok(())
    })();
    let saida = match r {
        Err(e) => (Some(false), e),
        Ok(()) if codigo.join("Cargo.toml").exists() => {
            let alvo = pasta(dados).join(".qa-target");
            let (c, o) = rodar(&codigo, &log, "cargo", &["build", "--quiet", "--color", "never"], Some(&alvo), 420);
            if c != 0 {
                (Some(false), format!("cargo build falhou: {}", cauda(&o, 1200)))
            } else {
                let (c, o) = rodar(&codigo, &log, "cargo", &["test", "--quiet", "--color", "never"], Some(&alvo), 420);
                (Some(c == 0), if c == 0 { format!("cargo build e cargo test passaram. {}", cauda(&o, 300)) } else { format!("cargo test falhou: {}", cauda(&o, 1200)) })
            }
        }
        Ok(()) if codigo.join("go.mod").exists() => {
            let (c, o) = rodar(&codigo, &log, "go", &["build", "./..."], None, 420);
            if c != 0 {
                (Some(false), format!("go build falhou: {}", cauda(&o, 1200)))
            } else {
                let (c, o) = rodar(&codigo, &log, "go", &["test", "./..."], None, 420);
                (Some(c == 0), if c == 0 { "go build e go test passaram.".into() } else { format!("go test falhou: {}", cauda(&o, 1200)) })
            }
        }
        Ok(()) => (None, "sem Cargo.toml nem go.mod: nada para compilar (revisão e QA por IA)".into()),
    };
    let _ = std::fs::remove_dir_all(&tmp);
    saida
}

/// Merge da branch do ticket na main DO PROJETO.
fn mesclar(dados: &Path, t: &Value) -> bool {
    let dir = ed::pasta_projeto(dados, t);
    if preparar(&dir).is_err() || git(&dir, &["checkout", "-q", "main"]).0 != 0 {
        return false;
    }
    let ok = git(&dir, &["merge", "--no-ff", "-q", "-m", &format!("{} aprovado", s(t, "id")), &ramo(t)]).0 == 0;
    if !ok {
        git(&dir, &["merge", "--abort"]);
    }
    ok
}

/// Arquivos que o ticket ja tem (branch) ou que o projeto ja tem (main), para o dev nao reescrever do zero.
/// Arquivos do projeto com os citados no ticket (caminho ou só o nome) na frente, na ordem do git.
fn citados_primeiro<'a>(nomes: &'a str, pedido: &str) -> Vec<&'a str> {
    let (mut c, r): (Vec<&str>, Vec<&str>) = nomes.lines().partition(|n| {
        let n = n.to_lowercase();
        pedido.contains(&n) || n.rsplit('/').next().is_some_and(|b| pedido.contains(b))
    });
    c.extend(r);
    c
}

fn contexto(dados: &Path, t: &Value) -> String {
    let dir = ed::pasta_projeto(dados, t);
    let b = ramo(t);
    let alvo = if git(&dir, &["rev-parse", "--verify", "-q", &b]).0 == 0 { b } else { "main".into() };
    let (c, nomes) = git(&dir, &["ls-tree", "-r", "--name-only", &alvo]);
    if c != 0 || nomes.trim().is_empty() {
        return String::new();
    }
    let mut out = String::from("ARQUIVOS QUE JÁ EXISTEM NO PROJETO (devolva o arquivo inteiro só do que mudar):\n");
    // projeto grande: primeiro os arquivos citados no ticket, inteiros; os outros so pelo nome (arquivo cortado vira arquivo quebrado)
    let pedido = format!("{} {}", t["titulo"].as_str().unwrap_or(""), t["descricao"].as_str().unwrap_or("")).to_lowercase();
    let mut so_nome = Vec::new();
    for n in &citados_primeiro(&nomes, &pedido) {
        let (_, cont) = git(&dir, &["show", &format!("{alvo}:{n}")]);
        // citado no ticket entra inteiro mesmo grande; os demais so se forem pequenos
        let citado = pedido.contains(&n.to_lowercase()) || n.rsplit('/').next().is_some_and(|b| pedido.contains(&b.to_lowercase()));
        if out.len() + cont.len() > 60_000 || (!citado && cont.len() > 16_000) {
            so_nome.push(*n);
            continue;
        }
        out.push_str(&format!("=== {n} ===\n{cont}\n"));
    }
    if !so_nome.is_empty() {
        out.push_str(&format!("OUTROS ARQUIVOS (conteúdo não enviado; não reescreva estes): {}\n", so_nome.join(", ")));
    }
    out
}

/// Monta as ferramentas reais do NexoDev e roda `f`. `ia` = IA gratis (roteador) para revisao, QA e reserva da geracao.
pub fn com_motor<T>(dados: &Path, ia: Ia, f: impl FnOnce(&ed::Ferramentas) -> T) -> T {
    let gerar = |sis: &str, p: &str| motor(dados, ia, sis, p);
    let ferr = ed::Ferramentas {
        gerar: &gerar, ia,
        contexto: &|t| contexto(dados, t),
        commit: &|t, _, _, arqs| commit(dados, t, arqs),
        build: &|t| build(dados, t),
        // o senior e o Claude por fora: o ticket espera em "bloqueado" e o parecer chega por POST /api/nexodev/parecer
        senior: &|_, _| Ok(()),
        recado: &|de, para, tipo, txt| drop(barramento::enviar(dados, de, para, tipo, txt, 0, None)),
    };
    f(&ferr)
}

pub fn aprovar(dados: &Path, id: &str, ts: f64) -> String {
    let r = ed::aprovar(dados, id, true, ts, &|t| mesclar(dados, t));
    if r.contains("entregue.") && !r.contains("mesclado") {
        return format!("{r} ATENÇÃO: o merge na main do projeto NÃO foi feito (veja o git em dados/empresa_dev/projetos/).");
    }
    r
}

// ---------------------------------------------------------------- ciclo em segundo plano

fn ciclo_andando(dados: &Path) -> bool {
    std::fs::read_to_string(pasta(dados).join(".trabalhando")).ok().and_then(|t| t.trim().parse::<f64>().ok()).is_some_and(|t| agora_s() - t < 900.0)
}

/// Um ciclo completo (varios passos). Chamado numa thread; o resultado fica em ultimo_ciclo.json.
pub fn ciclo(dados: &Path, ia: Ia, maximo: i64, ignorar: bool) -> Vec<String> {
    let (agora, ts) = (tempo::agora(), agora_s());
    MOTORES.with(|m| m.borrow_mut().clear());
    let arq = pasta(dados).join("ultimo_ciclo.json");
    let _ = ed::gravar(&arq, &json!({ "rodando": true, "inicio": ts, "maximo": maximo }));
    let feitos = com_motor(dados, ia, |f| ed::trabalhar(dados, f, &agora, ts, Some(maximo), 1, ignorar));
    let motores = MOTORES.with(|m| m.borrow().clone());
    let _ = ed::gravar(&arq, &json!({ "rodando": false, "inicio": ts, "fim": agora_s(), "feitos": feitos, "motores": motores }));
    feitos
}

// ---------------------------------------------------------------- rotas

fn resumo(t: &Value) -> Value {
    let ult = t["historico"].as_array().and_then(|h| h.last()).map(|h| format!("{}: {}", s(h, "quem"), corta(s(h, "txt"), 200))).unwrap_or_default();
    json!({ "id": t["id"], "projeto": t["projeto"], "titulo": t["titulo"], "estado": t["estado"], "responsavel": t["responsavel"], "tentativas": t["tentativas"],
            "critico": t["critico"], "parado": t["parado"], "ultimo": ult })
}

/// Cada motor da lista: aba aberta na VM e se esta logada (sem navegar, so olha a pagina que ja esta na aba).
fn estado_motores(dados: &Path) -> Value {
    let cfg = ed::config(dados);
    let abertas = vm::abas(dados).ok().and_then(|v| v.as_array().cloned());
    let mut out = vec![];
    for m in cfg["motores"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        let id = abertas.iter().flatten().find(|a| s(a, "agente") == m).map(|a| s(a, "id").to_string());
        let situacao = match (&abertas, id, ia_web::site(m)) {
            (None, _, _) => "VM fora do ar: não deu para olhar as abas".to_string(),
            (_, None, _) => "sem aba aberta na VM".into(),
            (_, _, None) => "IA desconhecida".into(),
            (_, Some(id), Some(site)) => match cdp::Aba::conectar(vm::CDP.trim_start_matches("http://"), &id).and_then(|mut a| a.comando("Runtime.evaluate", json!({ "expression": site.deslogado, "returnByValue": true })).and_then(|r| cdp::valor_avaliado(&r))) {
                Ok(v) if v == true => "aba aberta, mas DESLOGADA (o Dono precisa entrar nela)".into(),
                Ok(_) => "aba aberta e logada".into(),
                Err(e) => format!("aba aberta, não consegui olhar: {}", corta(&e, 100)),
            },
        };
        out.push(json!({ "motor": m, "situacao": situacao }));
    }
    out.push(json!({ "motor": "roteador gratis", "situacao": "sempre a reserva final (IAs gratuitas do roteador)" }));
    json!(out)
}

fn ticket_completo(dados: &Path, id: &str) -> Result<Value, String> {
    let t = ed::tickets(dados).into_iter().find(|t| s(t, "id").eq_ignore_ascii_case(id.trim())).ok_or("ticket não encontrado")?;
    let dir = ed::pasta_projeto(dados, &t);
    let b = ramo(&t);
    let tem_ramo = dir.join(".git").exists() && git(&dir, &["rev-parse", "--verify", "-q", &b]).0 == 0;
    let mut arquivos = serde_json::Map::new();
    let mut diff = String::new();
    if tem_ramo {
        for n in git(&dir, &["ls-tree", "-r", "--name-only", &b]).1.lines().take(30) {
            arquivos.insert(n.to_string(), json!(corta(&git(&dir, &["show", &format!("{b}:{n}")]).1, 12000)));
        }
        diff = corta(&git(&dir, &["diff", "--stat", &format!("main...{b}")]).1, 1500);
    }
    let entrega = std::fs::read_dir(&dir).into_iter().flatten().flatten().filter(|e| e.file_name().to_str().is_some_and(|n| n.starts_with(&format!("{}-", s(&t, "id"))) && n.ends_with(".md")))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path()))).max().and_then(|(_, p)| std::fs::read_to_string(p).ok()).unwrap_or_default();
    Ok(json!({ "ticket": t, "ramo": if tem_ramo { json!(b) } else { Value::Null }, "arquivos": arquivos, "diff": diff, "entrega_texto": corta(&entrega, 20000) }))
}

pub fn get(dados: &Path, url: &str) -> Option<Resp> {
    let caminho = url.split('?').next().unwrap_or("");
    Some(match caminho {
        "/nexodev" => crate::pagina(include_str!("nexodev.html")),
        "/api/nexodev/quadro" => {
            let (agora, todos) = (tempo::agora(), ed::tickets(dados));
            crate::dado(json!({ "texto": ed::quadro(dados, &agora), "situacao": ed::situacao(dados, &agora), "tickets": todos.iter().map(resumo).collect::<Vec<_>>(),
                "rodando": ciclo_andando(dados), "ciclo": ed::ler(&pasta(dados).join("ultimo_ciclo.json")) }))
        }
        "/api/nexodev/motores" => crate::dado(json!({ "motores": estado_motores(dados) })),
        c if c.starts_with("/api/nexodev/ticket/") => match ticket_completo(dados, &c["/api/nexodev/ticket/".len()..]) {
            Ok(v) => crate::dado(v),
            Err(e) => crate::erro(e, 404),
        },
        _ => return None,
    })
}

/// POST deste grupo (ja passou pelo cabecalho X-Braniac); None = nao e minha. Quem chama passa a IA gratis do roteador.
pub fn post(dados: &Path, ia: std::sync::Arc<dyn Fn(&str, &str) -> R + Send + Sync>, caminho: &str, v: &Value) -> Option<Resp> {
    let ts = agora_s();
    let t = |k: &str| v[k].as_str().unwrap_or("").trim().to_string();
    Some(match caminho {
        "/api/nexodev/abrir" => match ed::abrir(dados, &if t("projeto").is_empty() { "geral".to_string() } else { t("projeto") }, &t("titulo"), &t("descricao"), &t("area"), v["critico"] == true, ts) {
            Ok((tk, msg)) => crate::dado(json!({ "ok": true, "mensagem": msg, "ticket": resumo(&tk) })),
            Err(e) => crate::erro(e, 400),
        },
        "/api/nexodev/trabalhar" => {
            if ciclo_andando(dados) {
                return Some(crate::erro("já há um ciclo em andamento; olhe GET /api/nexodev/quadro (rodando)", 409));
            }
            let maximo = v["maximo"].as_i64().filter(|n| *n >= 1).unwrap_or(8).min(30);
            let (d, ignorar) = (dados.to_path_buf(), v["ignorar_teto"] == true);
            std::thread::spawn(move || {
                ciclo(&d, &*ia, maximo, ignorar);
            });
            crate::dado(json!({ "iniciado": true, "maximo": maximo, "veja": "GET /api/nexodev/quadro (rodando=false e ciclo.feitos quando acabar)" }))
        }
        "/api/nexodev/aprovar" => {
            let r = aprovar(dados, &t("id"), ts);
            crate::dado(json!({ "resultado": r, "mesclado": r.contains("mesclado na main") }))
        }
        "/api/nexodev/devolver" => match ed::devolver(dados, &t("id"), &t("motivo"), ts) {
            Ok(m) => crate::dado(json!({ "ok": true, "resultado": m })),
            Err(e) => crate::erro(e, 400),
        },
        "/api/nexodev/parecer" => {
            if t("texto").is_empty() {
                return Some(crate::erro("mande {id, texto} com o texto começando por APROVADO ou REPROVADO", 400));
            }
            crate::feito(ed::senior_resposta(dados, &t("id"), &t("texto"), ts).map(|_| json!({ "ok": true })))
        }
        // roda so o QA (build/test) da branch do ticket, para o senior conferir sem gastar IA
        "/api/nexodev/testar" => match ed::tickets(dados).into_iter().find(|x| s(x, "id").eq_ignore_ascii_case(&t("id"))) {
            Some(tk) => {
                let (ok, log) = build(dados, &tk);
                crate::dado(json!({ "passou": ok, "log": log }))
            }
            None => crate::erro("ticket não encontrado", 404),
        },
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn contexto_poe_arquivo_citado_no_ticket_na_frente() {
        // bug: so os 20 primeiros arquivos, cortados em 3000 caracteres; o dev reescrevia o arquivo citado pela metade
        let nomes = "Cargo.toml\nsrc/banco.rs\nsrc/relatorios.rs\nweb/app.js";
        assert_eq!(citados_primeiro(nomes, "ajustar a curva abc em src/relatorios.rs"), ["src/relatorios.rs", "Cargo.toml", "src/banco.rs", "web/app.js"]);
        assert_eq!(citados_primeiro(nomes, "botao novo no app.js"), ["web/app.js", "Cargo.toml", "src/banco.rs", "src/relatorios.rs"]);
        assert_eq!(citados_primeiro(nomes, "sem arquivo citado"), ["Cargo.toml", "src/banco.rs", "src/relatorios.rs", "web/app.js"]);
    }

    fn dir(nome: &str, motores: Value) -> PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-nexodev-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let a = |n: &str, area: &str, nivel: &str| json!({ "nome": n, "papel": area, "area": area, "nivel": nivel, "modelo": "gratis", "instrucao": format!("Você é {n}.") });
        ed::gravar(&d.join("empresa_dev/equipe.json"), &json!({ "empresa": "Oficina", "motores": motores, "limites": { "passos_dia": 100, "claude_dia": 4 },
            "agentes": [a("Ana", "gestao", "gestao"), a("Edu", "junior", "junior"), a("Eva", "qa", "pleno"), a("Bia", "front", "pleno")] })).unwrap();
        d
    }

    /// IA falsa: a geracao (sistema com "=== ARQUIVO") devolve `entrega`; revisao e QA aprovam.
    fn rodar_ciclo(d: &Path, entrega: &'static str) -> Vec<String> {
        let ia = |sis: &str, _: &str| -> R { Ok(if sis.contains("=== ARQUIVO") { entrega.to_string() } else { "APROVADO ok".to_string() }) };
        ciclo(d, &ia, 8, false)
    }

    const HTML: &str = "=== ARQUIVO: index.html ===\n```html\n<p>oi</p>\n```\n=== FIM ===";
    const CRATE_OK: &str = "=== ARQUIVO: Cargo.toml ===\n```toml\n[package]\nname = \"soma\"\nversion = \"0.1.0\"\nedition = \"2021\"\n```\n=== FIM ===\n=== ARQUIVO: src/lib.rs ===\n```rust\npub fn soma(a: i64, b: i64) -> i64 { a + b }\n#[cfg(test)]\nmod t { #[test] fn soma_ok() { assert_eq!(super::soma(2, 3), 5); } }\n```\n=== FIM ===";
    const CRATE_QUEBRADO: &str = "=== ARQUIVO: Cargo.toml ===\n```toml\n[package]\nname = \"ruim\"\nversion = \"0.1.0\"\nedition = \"2021\"\n```\n=== FIM ===\n=== ARQUIVO: src/lib.rs ===\n```rust\npub fn ruim( -> i64 { x }\n```\n=== FIM ===";

    fn estado(d: &Path) -> String {
        s(&ed::tickets(d)[0], "estado").to_string()
    }

    #[test]
    fn html_vai_ate_pronto_e_aprovar_mescla_na_main_do_projeto() {
        let d = dir("html", json!([]));
        ed::abrir(&d, "pagina", "Página de boas-vindas", "html css botao", "", false, 1.0).unwrap();
        let feitos = rodar_ciclo(&d, HTML);
        assert_eq!(estado(&d), "pronto", "{feitos:?}");
        let proj = d.join("empresa_dev/projetos/pagina");
        assert_ne!(git(&proj, &["cat-file", "-e", "main:index.html"]).0, 0, "ainda na branch tNNN, nao na main");
        assert!(aprovar(&d, "t001", 1.0).contains("mesclado na main"));
        assert_eq!(estado(&d), "entregue");
        assert_eq!(git(&proj, &["cat-file", "-e", "main:index.html"]).0, 0);
        assert!(git(&proj, &["log", "--oneline", "main"]).1.contains("aprovado"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn crate_que_compila_e_testa_passa_no_qa() {
        let d = dir("ok", json!([]));
        ed::abrir(&d, "soma", "Função soma de dois inteiros", "rust puro com teste", "", false, 1.0).unwrap();
        let feitos = rodar_ciclo(&d, CRATE_OK);
        let t = &ed::tickets(&d)[0];
        assert_eq!(s(t, "estado"), "pronto", "{feitos:?} {t}");
        assert!(t["historico"].to_string().contains("APROVADO"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn build_quebrado_reprova_sem_gastar_ia_do_qa() {
        let d = dir("quebrado", json!([]));
        ed::abrir(&d, "ruim", "Função que não compila", "rust", "", false, 1.0).unwrap();
        let chamadas = AtomicUsize::new(0);
        let ia = |sis: &str, _: &str| -> R {
            chamadas.fetch_add(1, Ordering::SeqCst);
            Ok(if sis.contains("=== ARQUIVO") { CRATE_QUEBRADO.to_string() } else { "APROVADO ok".to_string() })
        };
        ciclo(&d, &ia, 3, false); // dev, revisao, qa
        let t = &ed::tickets(&d)[0];
        assert_eq!(s(t, "estado"), "desenvolvimento", "{t}");
        assert!(t["historico"].to_string().contains("o build falhou"), "{t}");
        assert_eq!(chamadas.load(Ordering::SeqCst), 2, "dev + revisao; o QA reprovou pelo build, sem IA");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn entrega_sem_arquivo_no_formato_reprova_e_nao_vira_pronto() {
        let d = dir("prosa", json!([]));
        ed::abrir(&d, "x", "Tela de login simples", "html", "", false, 1.0).unwrap();
        rodar_ciclo(&d, "Claro! Aqui vai uma explicação, sem arquivo nenhum.");
        let t = &ed::tickets(&d)[0];
        assert_ne!(s(t, "estado"), "pronto");
        assert!(t["historico"].to_string().contains("nenhum arquivo"), "{t}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn motor_web_que_falha_cai_no_roteador_e_diz_qual_falhou() {
        let d = dir("reserva", json!(["site-que-nao-existe"])); // nao toca na VM
        ed::abrir(&d, "pagina", "Página de boas-vindas", "html", "", false, 1.0).unwrap();
        rodar_ciclo(&d, HTML);
        let ciclo = ed::ler(&d.join("empresa_dev/ultimo_ciclo.json")).unwrap();
        let m = ciclo["motores"][0].as_str().unwrap();
        assert!(m.starts_with("roteador gratis") && m.contains("site-que-nao-existe: IA desconhecida"), "{m}");
        let md = std::fs::read_dir(d.join("empresa_dev/projetos/pagina")).unwrap().flatten().find(|e| e.file_name().to_string_lossy().ends_with(".md")).unwrap();
        assert!(std::fs::read_to_string(md.path()).unwrap().contains("<!-- motor: roteador gratis"));
        // sem nenhum motor, o erro e dito
        let ia = |_: &str, _: &str| -> R { Err("sem provedor".into()) };
        let e = motor(&d, &ia, "sis", "p").unwrap_err();
        assert!(e.contains("nenhum motor respondeu") && e.contains("sem provedor"), "{e}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn devolver_volta_ao_dev_com_o_motivo_e_aprovar_so_do_pronto() {
        let d = dir("devolver", json!([]));
        ed::abrir(&d, "pagina", "Página de boas-vindas", "html", "", false, 1.0).unwrap();
        assert!(aprovar(&d, "T001", 1.0).contains("ainda não está pronto"));
        rodar_ciclo(&d, HTML);
        assert_eq!(estado(&d), "pronto");
        assert!(ed::devolver(&d, "T001", " ", 2.0).is_err());
        ed::devolver(&d, "T001", "falta o botão de contato", 2.0).unwrap();
        let t = &ed::tickets(&d)[0];
        assert_eq!((s(t, "estado"), t["tentativas"].as_i64()), ("desenvolvimento", Some(0)));
        assert!(t["historico"].to_string().contains("falta o botão de contato"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn ticket_critico_espera_o_parecer_do_claude_e_o_parecer_destrava() {
        let d = dir("critico", json!([]));
        ed::abrir(&d, "pagina", "Página crítica do pagamento", "html", "", true, 1.0).unwrap();
        rodar_ciclo(&d, HTML);
        assert_eq!(estado(&d), "bloqueado");
        ed::senior_resposta(&d, "T001", "APROVADO: ok", 2.0).unwrap();
        assert_eq!(estado(&d), "qa");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Regressao: rota POST do NexoDev sem X-Braniac e recusada (403) e nao abre ticket.
    #[test]
    fn rota_post_recusa_sem_x_braniac() {
        let servidor = std::sync::Arc::new(tiny_http::Server::http("127.0.0.1:0").unwrap());
        let porta = servidor.server_addr().to_ip().unwrap().port();
        let s2 = servidor.clone();
        let rot = std::sync::Arc::new(crate::Roteador {
            cfg: json!({ "system": "S", "cadeia": [] }), store: crate::store::Store::em_memoria().unwrap(), banco: crate::banco::Banco::em_memoria().unwrap(),
            chamar: Box::new(|_, _, _, _| Ok("x".into())), chamar_tools: Box::new(|_, _, _, _, _| Ok(json!({ "content": "x" }))),
            contexto: Box::new(|_, _| String::new()), vigia: Box::new(|_, _| {}), ler_chave: Box::new(|_| "k".into()),
        });
        let fio = std::thread::spawn(move || {
            if let Ok(mut req) = s2.recv() {
                let r = crate::atender(&mut req, &rot);
                let _ = req.respond(r);
            }
        });
        let antes = ed::tickets(&crate::pasta_dados()).len();
        let r = ureq::post(&format!("http://127.0.0.1:{porta}/api/nexodev/abrir")).send_string(r#"{"projeto":"x","titulo":"Ticket sem cabeçalho","descricao":"y"}"#);
        assert!(matches!(r, Err(ureq::Error::Status(403, _))), "{r:?}");
        fio.join().unwrap();
        assert_eq!(ed::tickets(&crate::pasta_dados()).len(), antes);
    }
}
