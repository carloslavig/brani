// B.R.A.N.I: janela nativa (Tauri 2 / WebView2) do Jarvis. A tela vem do servidor local do Braniac (127.0.0.1:8865/b2),
// que guarda tudo no PC. Se o servidor nao estiver de pe, liga o Braniac (Chrome do agente + servidor) sem abrir o navegador.
// Os logins das IAs ficam no Chrome do Braniac (decisao do conselho de IAs: o Google bloqueia login dentro do WebView2).
// O aplicativo e o PRINCIPAL: so existe uma copia (uma segunda chamada manda a rota para a primeira e sai), o servidor pede
// "abra /, /hud, /alma ou /jarvis" por /api/app/sinal (1 s) e o botao "Servidor: ligado/pausado" fica no canto da tela.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

const BASE: &str = "http://127.0.0.1:8865";
const TELA: &str = "http://127.0.0.1:8865/b2";

fn servidor_ok() -> bool {
    TcpStream::connect_timeout(&"127.0.0.1:8865".parse().unwrap(), Duration::from_millis(800)).is_ok()
}

/// Uma chamada HTTP simples ao servidor local (sem biblioteca): devolve o corpo da resposta.
fn http(metodo: &str, caminho: &str, corpo: &str) -> String {
    let Ok(mut s) = TcpStream::connect_timeout(&"127.0.0.1:8865".parse().unwrap(), Duration::from_millis(800)) else {
        return String::new();
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
    let req = format!("{metodo} {caminho} HTTP/1.0\r\nHost: 127.0.0.1:8865\r\nX-Braniac: app\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{corpo}", corpo.len());
    let _ = s.write_all(req.as_bytes());
    let mut r = String::new();
    let _ = s.read_to_string(&mut r);
    r
}

/// Valor de um campo simples do JSON do servidor (`"chave":valor` ou `"chave":"valor"`), sem biblioteca de JSON.
fn campo(corpo: &str, chave: &str) -> String {
    let alvo = format!("\"{chave}\":");
    corpo.find(&alvo).map(|i| corpo[i + alvo.len()..].trim_start_matches('"').chars().take_while(|c| !matches!(c, '"' | ',' | '}')).collect()).unwrap_or_default()
}

fn rota_dos_argumentos() -> String {
    std::env::args().find_map(|a| a.strip_prefix("--rota=").map(str::to_string)).unwrap_or_default()
}

fn ligar_braniac() {
    let mut c = Command::new("cmd");
    c.args(["/c", r"E:\braniac-rs\ligar-brani.bat"]).current_dir(r"E:\braniac-rs");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // sem janela de console
    }
    let _ = c.spawn();
}

/// Traz a janela para a frente de verdade: restaura, mostra, poe no lugar se estava fora da tela e vence a trava do Windows
/// (que nao deixa programa em segundo plano roubar o foco) ligando e desligando "sempre no topo".
fn frente(janela: &tauri::WebviewWindow) {
    let _ = janela.unminimize();
    let _ = janela.show();
    if !matches!(janela.current_monitor(), Ok(Some(_))) {
        let _ = janela.center(); // janela que ficou fora da tela volta para ela
    }
    let _ = janela.set_always_on_top(true);
    let _ = janela.set_focus();
    let _ = janela.set_always_on_top(false);
}

/// "/hud" e "/alma" sao janelas do proprio aplicativo (uma de cada, nunca duas); "/" e "/jarvis" usam a janela principal.
fn abrir_rota(app: &AppHandle, rota: &str) {
    match rota {
        "/hud" | "/alma" => {
            let (rotulo, titulo, w, h) = if rota == "/hud" { ("hud", "B.R.A.N.I - Jarvis", 520.0, 780.0) } else { ("alma", "B.R.A.N.I - Alma", 420.0, 540.0) };
            if let Some(j) = app.get_webview_window(rotulo) {
                frente(&j);
                return;
            }
            let Ok(url) = format!("{BASE}{rota}").parse() else { return };
            // a Alma e translucida de verdade: janela transparente e sem moldura (o fundo vidro vem do CSS da pagina)
            let b = WebviewWindowBuilder::new(app, rotulo, WebviewUrl::External(url)).title(titulo).inner_size(w, h);
            let b = if rota == "/alma" { b.transparent(true).decorations(false).always_on_top(true) } else { b };
            if let Ok(j) = b.build() {
                if rota == "/alma" {
                    if let Ok(Some(m)) = j.primary_monitor() {
                        let (esc, t) = (m.scale_factor(), m.size());
                        let _ = j.set_position(tauri::PhysicalPosition::new(t.width as i32 - (w * esc) as i32 - 24, t.height as i32 - (h * esc) as i32 - 70));
                    }
                } else {
                    let _ = j.set_position(tauri::LogicalPosition::new(60.0, 60.0));
                }
                frente(&j);
            }
        }
        _ => {
            if rota == "/app" {
                for r in ["hud", "alma"] {
                    if let Some(j) = app.get_webview_window(r) {
                        let _ = j.close();
                    }
                }
            }
            if let Some(j) = app.get_webview_window("principal") {
                frente(&j);
                if rota == "/jarvis" {
                    let _ = j.maximize();
                    let _ = j.eval("window.abrirAba && abrirAba('jarvis')");
                }
            }
        }
    }
}

fn main() {
    // uma copia so: se o app ja esta aberto, so manda a rota para ele e sai
    if servidor_ok() && campo(&http("GET", "/api/app/vivo", ""), "vivo") == "true" {
        let rota = rota_dos_argumentos();
        let rota = if rota.is_empty() { if std::env::args().any(|a| a == "--jarvis") { "/jarvis".to_string() } else { "/".to_string() } } else { rota };
        http("POST", "/api/app/abrir", &format!("{{\"rota\":\"{rota}\"}}"));
        return;
    }
    tauri::Builder::default()
        .setup(|app| {
            let janela = WebviewWindowBuilder::new(app, "principal", WebviewUrl::App("index.html".into()))
                .title("B.R.A.N.I")
                .inner_size(1280.0, 820.0)
                .min_inner_size(820.0, 560.0)
                .build()?;
            let app = app.handle().clone();
            thread::spawn(move || {
                // tela de abertura estilo J.A.R.V.I.S (ui/index.html): a barra mostra o progresso real de ligar o Braniac
                let p = |n: u32, t: &str| {
                    let _ = janela.eval(&format!("window.progresso && progresso({n}, '{t}')"));
                };
                thread::sleep(Duration::from_millis(700));
                // nome e cor que o Dono escolheu (arquivo local, gravado pelo proprio Braniac e ja validado la)
                if let Ok(json) = std::fs::read_to_string("E:/braniac-rs/dados/aparencia.json") {
                    if json.trim_start().starts_with('{') {
                        let _ = janela.eval(&format!("window.tema && tema({json})"));
                    }
                }
                p(12, "Verificando sistemas");
                let ja_estava = servidor_ok();
                if !ja_estava {
                    p(25, "Ligando o B.R.A.N.I");
                    ligar_braniac();
                }
                let inicio = Instant::now();
                let fim = inicio + Duration::from_secs(150);
                while !servidor_ok() && Instant::now() < fim {
                    thread::sleep(Duration::from_secs(2));
                    let seg = inicio.elapsed().as_secs() as u32;
                    p((25 + seg).min(85), if seg < 20 { "Calibrating" } else { "Conectando as IAs" });
                }
                if servidor_ok() {
                    // o Jarvis fala ao abrir (so aqui, na abertura do app; o servidor decide se cala por silencio, pausa ou intervalo)
                    http("POST", "/api/saudacao/abrir", &format!("{{\"ja_estava\":{ja_estava},\"esperou_s\":{}}}", inicio.elapsed().as_secs()));
                    p(if ja_estava { 70 } else { 92 }, "Conectando as IAs");
                    thread::sleep(Duration::from_millis(if ja_estava { 1600 } else { 700 }));   // a abertura aparece mesmo com o Braniac ja ligado
                    p(100, "Online");
                    http("POST", "/api/saudacao/pronto", "{}");   // barra em 100%: o Jarvis resume o PC (depois da saudacao, um audio por vez)
                    thread::sleep(Duration::from_millis(900));
                    let rota = rota_dos_argumentos();
                    let jarvis = std::env::args().any(|a| a == "--jarvis") || rota == "/jarvis";   // aberto pela voz: ja abre na aba do Jarvis
                    let escritorio = std::env::args().any(|a| a == "--escritorio");   // aberto com --escritorio: ja abre na aba do Escritorio
                    let url = if jarvis { format!("{TELA}#jarvis") } else if escritorio { format!("{TELA}#escritorio") } else { TELA.to_string() };
                    let _ = janela.navigate(url.parse().unwrap());
                    if jarvis {
                        let _ = janela.maximize();
                        frente(&janela);
                    }
                    if rota == "/hud" || rota == "/alma" {
                        abrir_rota(&app, &rota);
                    }
                    let (mut ultimo, mut primeira) = (String::new(), true);
                    loop {
                        // o servidor manda a rota ("Jarvis, ta acordado?", bandeja, alerta): vem para a frente e abre a tela pedida
                        thread::sleep(Duration::from_secs(1));
                        let s = http("GET", "/api/app/sinal", "");
                        let seq = campo(&s, "seq");
                        let recente = campo(&s, "idade_s").parse::<u64>().unwrap_or(9999) < 30;
                        if !seq.is_empty() && seq != ultimo && (!primeira || recente) {
                            abrir_rota(&app, &campo(&s, "rota"));
                        }
                        if !seq.is_empty() {
                            ultimo = seq;
                            primeira = false;
                        }
                    }
                } else {
                    p(100, "O B.R.A.N.I nao ligou: abra de novo");
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("erro ao abrir o B.R.A.N.I");
}
