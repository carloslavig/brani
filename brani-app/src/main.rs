// B.R.A.N.I: janela nativa (Tauri 2 / WebView2) do Jarvis. A tela vem do servidor local do Braniac (127.0.0.1:8865/b2),
// que guarda tudo no PC. Se o servidor nao estiver de pe, liga o Braniac (Chrome do agente + servidor) sem abrir o navegador.
// Os logins das IAs ficam no Chrome do Braniac (decisao do conselho de IAs: o Google bloqueia login dentro do WebView2).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};
use tauri::{WebviewUrl, WebviewWindowBuilder};

const TELA: &str = "http://127.0.0.1:8865/b2";

fn servidor_ok() -> bool {
    TcpStream::connect_timeout(&"127.0.0.1:8865".parse().unwrap(), Duration::from_millis(800)).is_ok()
}

/// Pergunta ao servidor se deve abrir alguma aba (o Jarvis foi chamado por voz). Devolve o corpo da resposta.
fn sinal() -> String {
    let Ok(mut s) = TcpStream::connect_timeout(&"127.0.0.1:8865".parse().unwrap(), Duration::from_millis(800)) else {
        return String::new();
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
    let _ = s.write_all(b"GET /api/b2/sinal HTTP/1.0\r\nHost: 127.0.0.1:8865\r\n\r\n");
    let mut r = String::new();
    let _ = s.read_to_string(&mut r);
    r
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

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let janela = WebviewWindowBuilder::new(app, "principal", WebviewUrl::App("index.html".into()))
                .title("B.R.A.N.I")
                .inner_size(1280.0, 820.0)
                .min_inner_size(820.0, 560.0)
                .build()?;
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
                    p(if ja_estava { 70 } else { 92 }, "Conectando as IAs");
                    thread::sleep(Duration::from_millis(if ja_estava { 1600 } else { 700 }));   // a abertura aparece mesmo com o Braniac ja ligado
                    p(100, "Online");
                    thread::sleep(Duration::from_millis(900));
                    let jarvis = std::env::args().any(|a| a == "--jarvis");   // aberto pela voz: ja abre na aba do Jarvis
                    let escritorio = std::env::args().any(|a| a == "--escritorio");   // aberto com --escritorio: ja abre na aba do Escritorio
                    let url = if jarvis { format!("{TELA}#jarvis") } else if escritorio { format!("{TELA}#escritorio") } else { TELA.to_string() };
                    let _ = janela.navigate(url.parse().unwrap());
                    if jarvis {
                        let _ = janela.maximize();
                    }
                    loop {
                        // 'Jarvis, ta acordado?' com o programa aberto: vem para a frente, maximiza e abre a aba do Jarvis
                        thread::sleep(Duration::from_secs(1));
                        if sinal().contains("\"jarvis\"") {
                            let _ = janela.unminimize();
                            let _ = janela.show();
                            let _ = janela.maximize();
                            let _ = janela.set_focus();
                            let _ = janela.eval("window.abrirAba && abrirAba('jarvis')");
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
