// B.R.A.N.I - miniatura de voz: UMA janelinha (translucida, sempre por cima, arrastavel) para Jarvis e Alma, mais o unico icone da bandeja.
// Processo proprio e leve: fala com o servidor so por HTTP (127.0.0.1) e funciona com o app fechado e com o servidor parado ou ocupado.
// Um erro aqui nunca derruba o app nem o servidor; sem servidor mostra "sem conexao" e some sozinha. A janela so aparece quando alguem
// e chamado (regras em src/miniatura_logica.rs, a mesma que o servidor testa).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#[path = "../../src/miniatura_logica.rs"]
mod logica;

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::thread::{self, sleep};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

const LARG: f64 = 308.0;
const ALT: f64 = 148.0;
const EXP_LARG: f64 = 358.0; // 350 de conteudo + 4 de folga de cada lado
const EXP_ALT: f64 = 488.0; // 480 de conteudo

static INICIO: OnceLock<Instant> = OnceLock::new();
fn agora() -> f64 {
    INICIO.get_or_init(Instant::now).elapsed().as_secs_f64()
}

/// Diario curto em dados/miniatura.log (so o que os botoes fazem), para saber depois por que um clique nao funcionou.
fn log(msg: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dados().join("miniatura.log")) {
        let _ = writeln!(f, "{:.1}s {msg}", agora());
    }
}

struct Est {
    mini: logica::Mini,
    quadro: Value,
    pos: Option<(i32, i32)>,
    ancora: (bool, bool),
}
type Ctx = Arc<Mutex<Est>>;

fn trava(c: &Ctx) -> std::sync::MutexGuard<'_, Est> {
    c.lock().unwrap_or_else(|e| e.into_inner())
}

fn porta() -> String {
    std::env::var("BRANIAC_PORTA").unwrap_or_else(|_| "8865".into())
}
fn dados() -> std::path::PathBuf {
    std::env::var_os("BRANIAC_DADOS").map(Into::into).unwrap_or_else(|| r"E:\braniac-rs\dados".into())
}

/// Uma chamada HTTP simples (sem biblioteca): corpo da resposta, ou None se o servidor nao atende.
fn http(metodo: &str, caminho: &str, corpo: &str) -> Option<String> {
    http_t(metodo, caminho, corpo, 3)
}

/// Igual, com tempo de espera da resposta (o chat do agente pode levar dezenas de segundos).
fn http_t(metodo: &str, caminho: &str, corpo: &str, espera: u64) -> Option<String> {
    let end = format!("127.0.0.1:{}", porta());
    let mut s = TcpStream::connect_timeout(&end.parse().ok()?, Duration::from_millis(700)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(espera)));
    let req = format!("{metodo} {caminho} HTTP/1.0\r\nHost: {end}\r\nX-Braniac: mini\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{corpo}", corpo.len());
    s.write_all(req.as_bytes()).ok()?;
    // le ate fechar OU ate o tempo acabar: o que ja veio vale (a conexao nem sempre fecha na hora e perdia a resposta toda)
    let (mut buf, mut tmp) = (Vec::new(), [0u8; 4096]);
    while let Ok(n) = s.read(&mut tmp) {
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
    }
    logica::corpo_http(&String::from_utf8_lossy(&buf))
}
fn pegar(caminho: &str) -> Option<Value> {
    serde_json::from_str(&http("GET", caminho, "")?).ok()
}
fn mandar(caminho: &str, v: Value) -> Option<Value> {
    serde_json::from_str(&http("POST", caminho, &v.to_string())?).ok()
}
fn escuta(acao: &str) {
    mandar("/api/escuta-acao", json!({ "acao": acao }));
}

/// Abre o app completo ou, se ja estiver aberto, so traz a janela dele (o servidor guarda a instancia unica). Sem servidor, abre o exe.
fn abrir_app() {
    let r = mandar("/api/app/abrir", json!({ "rota": "/app" }));
    log(&format!("abrir app: servidor respondeu {r:?}"));
    if r.is_some_and(|r| r["ok"] == true) {
        // o servidor jura que o app esta de pe ou foi lancado: confere ate 6 s; se nao aparecer, abre o exe por conta propria
        for _ in 0..12 {
            sleep(Duration::from_millis(500));
            if pegar("/api/app/vivo").is_some_and(|v| v["vivo"] == true) {
                return;
            }
        }
        log("abrir app: o app nao apareceu em 6 s, abrindo o exe");
    }
    let exe = std::env::var_os("BRANIAC_APP").map(std::path::PathBuf::from).unwrap_or_else(|| r"E:\braniac-rs\brani-app\B.R.A.N.I.exe".into());
    let mut c = std::process::Command::new(exe);
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut c, 0x0800_0000); // sem console
    log(&format!("abrir app: exe lancado = {}", c.spawn().is_ok()));
}

fn chamar(agente: &str) {
    escuta("acordar");
    if agente == "alma" {
        sleep(Duration::from_millis(1500));
        mandar("/api/escuta-acao", json!({ "acao": "frase", "texto": "quero falar com a Alma" }));
    }
}

fn tratar(app: &AppHandle, ctx: &Ctx, nome: &str) {
    let q = trava(ctx).quadro.clone();
    log(&format!("acao: {nome}"));
    match nome {
        "expandir" => alternar(app, ctx, None),
        "dormir" => escuta("descansar"),
        "mic" => escuta(if q["mic"] == true { "desligar" } else { "ligar" }),
        "mudo" => drop(mandar("/api/mudo", if q["mudo"] == true { json!({ "mudo": false }) } else { json!({ "minutos": 60 }) })),
        "app" => abrir_app(),
        "jarvis" | "alma" => chamar(nome),
        "servidor" => {
            let ligado = pegar("/api/servidor").is_some_and(|s| s["estado"] == "ligado");
            mandar(if ligado { "/api/servidor/pausa" } else { "/api/servidor/retomar" }, json!({}));
        }
        "fechar" => {
            trava(ctx).mini.dispensar();
            if let Some(j) = app.get_webview_window("mini") {
                let _ = j.hide();
            }
        }
        _ => {}
    }
}

/// Expande (350x480) ou recolhe (tamanho de hoje): a largura passa a caber 350, a janela cresce para o lado mais livre e nunca sai da tela.
fn alternar(app: &AppHandle, ctx: &Ctx, quer: Option<bool>) {
    let Some(j) = app.get_webview_window("mini") else { return };
    let ligar = {
        let mut e = trava(ctx);
        let ligar = quer.unwrap_or(!e.mini.expandida());
        if ligar == e.mini.expandida() {
            return;
        }
        e.mini.expandir(ligar, agora());
        ligar
    };
    ajustar(&j, ctx, true);
    gravar(trava(ctx).pos, ligar);
}

/// Poe tamanho, canto e foco da janela de acordo com o estado (expandida ou nao). Roda ao clicar e TAMBEM a cada ciclo com a janela
/// na tela: se o tamanho real nao bate com o estado (ex.: abriu expandida enquanto estava escondida e o Windows ignorou), conserta sozinha.
/// O canto usa a AREA UTIL do monitor (sem a barra de tarefas): a janela nunca sai da tela nem fica por baixo da barra.
fn ajustar(j: &tauri::WebviewWindow, ctx: &Ctx, agora_mesmo: bool) {
    let (Ok(p), Ok(tam), Ok(Some(m))) = (j.outer_position(), j.outer_size(), j.current_monitor().or_else(|_| j.primary_monitor())) else { return };
    let (esc, wa) = (m.scale_factor(), m.work_area());
    let (tela, rel) = ((wa.size.width as i64, wa.size.height as i64), (p.x as i64 - wa.position.x as i64, p.y as i64 - wa.position.y as i64));
    let (n, x) = (((LARG * esc).round() as i64, (ALT * esc).round() as i64), ((EXP_LARG * esc).round() as i64, (EXP_ALT * esc).round() as i64));
    let atual = (tam.width as i64, tam.height as i64);
    let (exp, quer) = {
        let e = trava(ctx);
        (e.mini.expandida(), if e.mini.expandida() { x } else { n })
    };
    if !agora_mesmo && (atual.0 - quer.0).abs() <= 2 && (atual.1 - quer.1).abs() <= 2 {
        return;
    }
    let nova = {
        let mut e = trava(ctx);
        if exp {
            let (np, anc) = logica::expandir(rel, tela, atual, x);
            e.ancora = anc;
            np
        } else {
            logica::recolher(rel, tela, atual, n, e.ancora)
        }
    };
    let _ = j.set_focusable(exp); // expandida precisa de foco para digitar; recolhida nunca rouba o foco
    let (w, h) = if exp { (EXP_LARG, EXP_ALT) } else { (LARG, ALT) };
    let _ = j.set_size(tauri::LogicalSize::new(w, h));
    let _ = j.set_position(tauri::PhysicalPosition::new((nova.0 + wa.position.x as i64) as i32, (nova.1 + wa.position.y as i64) as i32));
    if exp && agora_mesmo {
        let _ = j.set_focus();
    }
}

fn gravar(pos: Option<(i32, i32)>, exp: bool) {
    let mut v = json!({ "exp": exp });
    if let Some((x, y)) = pos.or_else(|| ler_pos().map(|(x, y)| (x as i32, y as i32))) {
        v["x"] = json!(x);
        v["y"] = json!(y);
    }
    let _ = std::fs::write(dados().join("miniatura.json"), v.to_string());
}

#[tauri::command]
fn toque(ctx: tauri::State<'_, Ctx>) {
    trava(&ctx).mini.tocar(agora());
}

/// O que o dono digitou vai ao agente ativo: Jarvis pelo chat de sempre; Alma pela conversa dela (a sessao de voz, que responde na voz dela).
#[tauri::command]
fn enviar(texto: String, ctx: tauri::State<'_, Ctx>) {
    let ctx = ctx.inner().clone();
    thread::spawn(move || {
        let t: String = texto.trim().chars().take(1500).collect();
        if t.is_empty() {
            return;
        }
        let ag = {
            let mut e = trava(&ctx);
            e.mini.enviado(&t, agora());
            e.quadro["agente"].as_str().unwrap_or("jarvis").to_string()
        };
        log(&format!("enviar ao {ag}: {} caracteres", t.chars().count()));
        let r = if ag == "alma" {
            http_t("POST", "/api/escuta/frase", &json!({ "texto": t }).to_string(), 150).and_then(|c| serde_json::from_str::<Value>(&c).ok()).and_then(|v| v["texto"].as_str().map(str::to_string))
        } else {
            http_t("POST", "/api/chat", &json!({ "texto": t }).to_string(), 150).and_then(|c| serde_json::from_str::<Value>(&c).ok()).and_then(|v| v["resposta"].as_str().map(str::to_string))
        };
        let mut e = trava(&ctx);
        match r {
            Some(r) => e.mini.resposta(&ag, &r, agora()),
            None => e.mini.aviso("sem resposta do servidor", agora()),
        }
    });
}

#[tauri::command]
fn acao(nome: String, app: AppHandle, ctx: tauri::State<'_, Ctx>) {
    let ctx = ctx.inner().clone();
    thread::spawn(move || tratar(&app, &ctx, &nome)); // nunca trava a pagina
}

#[tauri::command]
fn arrastar(janela: tauri::WebviewWindow) {
    let _ = janela.start_dragging();
}

fn ler_exp() -> bool {
    std::fs::read_to_string(dados().join("miniatura.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).is_some_and(|v| v["exp"] == true)
}

fn ler_pos() -> Option<(i64, i64)> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(dados().join("miniatura.json")).ok()?).ok()?;
    Some((v["x"].as_i64()?, v["y"].as_i64()?))
}

/// Anel coral desenhado na mao (nada de arquivo): o unico icone da bandeja.
fn icone() -> tauri::image::Image<'static> {
    let mut px = Vec::with_capacity(32 * 32 * 4);
    for y in 0..32 {
        for x in 0..32 {
            let d = ((x as f32 - 15.5).powi(2) + (y as f32 - 15.5).powi(2)).sqrt();
            let a = (1.0 - ((d - 10.5).abs() - 2.2).max(0.0)).clamp(0.0, 1.0);
            px.extend_from_slice(&[240, 138, 75, (a * 255.0) as u8]);
        }
    }
    tauri::image::Image::new_owned(px, 32, 32)
}

fn main() {
    // uma copia so: a segunda sai calada
    let Some(_trava) = logica::trava(logica::PORTA_TRAVA) else { return };
    let ctx: Ctx = Arc::new(Mutex::new(Est { mini: logica::Mini::default(), quadro: Value::Null, pos: None, ancora: (false, false) }));
    let ctx2 = ctx.clone();
    let r = tauri::Builder::default()
        .manage(ctx.clone())
        .invoke_handler(tauri::generate_handler![acao, arrastar, toque, enviar])
        .setup(move |app| {
            let h = app.handle().clone();
            let j = WebviewWindowBuilder::new(app, "mini", WebviewUrl::App("index.html".into()))
                .title("B.R.A.N.I - miniatura")
                .inner_size(LARG, ALT)
                .transparent(true)
                .decorations(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .resizable(false)
                .shadow(false)
                .visible(false)
                .focusable(false)
                .build()?;
            if let Ok(Some(m)) = j.primary_monitor() {
                let (e, t) = (m.scale_factor(), m.work_area().size);
                let tam = ((LARG * e) as i64, (ALT * e) as i64);
                let (x, y) = logica::posicao(ler_pos(), (t.width as i64, t.height as i64), tam);
                let _ = j.set_position(tauri::PhysicalPosition::new(x as i32, y as i32));
            }
            let c3 = ctx2.clone();
            j.on_window_event(move |e| {
                if let WindowEvent::Moved(p) = e {
                    let mut e = trava(&c3);
                    if !e.mini.expandida() {
                        e.pos = Some((p.x, p.y)); // expandida o canto e outro: so o tamanho de hoje e guardado
                    }
                }
            });
            if ler_exp() {
                trava(&ctx2).mini.expandir(true, 0.0); // lembra que ficou expandida; o laco ajusta o tamanho quando a janela aparecer
            }
            // bandeja: um icone so, com tudo que o Dono precisa
            let it = |id: &str, t: &str| MenuItem::with_id(app, id, t, true, None::<&str>);
            let menu = Menu::with_items(
                app,
                &[
                    &it("jarvis", "Chamar Jarvis")?,
                    &it("alma", "Chamar Alma")?,
                    &it("app", "Abrir aplicativo")?,
                    &it("mic", "Ligar ou desligar o microfone")?,
                    &it("mudo", "Silenciar por 1 hora")?,
                    &it("servidor", "Pausar ou retomar o servidor")?,
                ],
            )?;
            let (c4, h4, c5, h5) = (ctx2.clone(), h.clone(), ctx2.clone(), h.clone());
            TrayIconBuilder::new()
                .icon(icone())
                .tooltip("B.R.A.N.I")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(move |_, e| {
                    let (id, c, a) = (e.id.as_ref().to_string(), c4.clone(), h4.clone());
                    thread::spawn(move || tratar(&a, &c, &id));
                })
                .on_tray_icon_event(move |_, e| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                        let (c, a) = (c5.clone(), h5.clone());
                        thread::spawn(move || tratar(&a, &c, "app"));
                    }
                })
                .build(app)?;
            // laco: le o estado do servidor, decide, mostra ou esconde, empurra o quadro para a pagina e guarda a posicao
            let ctx = ctx2.clone();
            thread::spawn(move || {
                let (mut visivel, mut gravada) = (false, None);
                loop {
                    let d = pegar("/api/escuta-estado");
                    let agora = agora();
                    let (q, pos) = {
                        let mut e = trava(&ctx);
                        let q = e.mini.passo(d.as_ref(), agora);
                        e.quadro = q.clone();
                        (q, e.pos)
                    };
                    let quer = q["visivel"] == true;
                    if let Some(j) = h.get_webview_window("mini") {
                        if quer != visivel {
                            let _ = if quer { j.show() } else { j.hide() };
                            visivel = quer;
                        }
                        if quer {
                            ajustar(&j, &ctx, false);
                            let _ = j.eval(&format!("window.estado&&window.estado({q})"));
                        }
                    }
                    if pos != gravada {
                        if let Some((x, y)) = pos {
                            gravar(Some((x, y)), trava(&ctx).mini.expandida());
                        }
                        gravada = pos;
                    }
                    sleep(Duration::from_millis(if visivel { 300 } else { 900 }));
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!());
    if let Err(e) = r {
        let _ = std::fs::write(dados().join("miniatura.erro.log"), e.to_string());
    }
}
