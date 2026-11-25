//! Janelas do Jarvis (/hud) e da Alma (/alma) no Windows. Antes abriam pelo explorer.exe: caiam numa aba do navegador padrao, muitas vezes
//! escondida ou fora da tela. Agora e uma janela propria (Chrome/Edge `--app`, perfil separado) trazida para a frente a forca.
//! Tudo roda em segundo plano: quem chama nunca espera (a trava da voz fica livre).
#![allow(dead_code)] // roteiro/argumentos ficaram so para o teste e para a volta ao navegador, se um dia for preciso
use std::path::Path;

pub struct Janela {
    pub url: String,
    pub perfil: &'static str,
    pub titulo: &'static str, // trecho do titulo da pagina, para achar a janela
    pub largura: i64,
    pub altura: i64,
    pub canto: bool, // true = embaixo a direita (esfera da Alma)
    pub rota_app: &'static str, // rota que o aplicativo (brani-app) abre no lugar da janela Chrome/Edge
}

pub fn jarvis(porta: u16) -> Janela {
    Janela { url: format!("http://127.0.0.1:{porta}/hud"), perfil: "janela-jarvis", titulo: "Jarvis*Voz", largura: 520, altura: 780, canto: false, rota_app: "/jarvis" }
}

pub fn alma(porta: u16) -> Janela {
    Janela { url: format!("http://127.0.0.1:{porta}/alma"), perfil: "janela-alma", titulo: "Alma", largura: 420, altura: 540, canto: true, rota_app: "/alma" }
}

/// Argumentos do navegador: janela de aplicativo, sem barra, num perfil so dela (assim e uma janela NOVA, nunca uma aba de outro Chrome).
pub fn argumentos(j: &Janela, dados: &Path) -> Vec<String> {
    vec![
        format!("--app={}", j.url),
        format!("--user-data-dir={}", dados.join(j.perfil).display()),
        format!("--window-size={},{}", j.largura, j.altura),
        "--window-position=60,60".into(),
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
    ]
}

/// PowerShell que acha a janela pelo titulo e a traz para a frente (restaura se minimizada, poe no topo e no lugar) ou a minimiza.
/// Imprime "achou" ou "nao". Pura: so monta o texto.
pub fn roteiro(titulo: &str, minimizar: bool, largura: i64, altura: i64, canto: bool) -> String {
    let acao = if minimizar {
        "[W]::ShowWindow($h,6)|Out-Null".to_string()
    } else {
        let pos = if canto { format!("$x=[W]::GetSystemMetrics(0)-{largura}-24;$y=[W]::GetSystemMetrics(1)-{altura}-70") } else { "$x=60;$y=60".into() };
        format!("[W]::ShowWindow($h,9)|Out-Null;{pos};[W]::SetWindowPos($h,[IntPtr](-1),$x,$y,{largura},{altura},0x0040)|Out-Null;[W]::SetForegroundWindow($h)|Out-Null")
    };
    format!(
        "Add-Type 'using System;using System.Runtime.InteropServices;public class W{{[DllImport(\"user32.dll\")]public static extern bool ShowWindow(IntPtr h,int c);\
[DllImport(\"user32.dll\")]public static extern bool SetForegroundWindow(IntPtr h);[DllImport(\"user32.dll\")]public static extern int GetSystemMetrics(int i);\
[DllImport(\"user32.dll\")]public static extern bool SetWindowPos(IntPtr h,IntPtr a,int x,int y,int w,int d,uint f);}}';\
$p=Get-Process chrome,msedge -ErrorAction SilentlyContinue|Where-Object {{$_.MainWindowHandle -ne 0 -and $_.MainWindowTitle -like '{titulo}*'}}|Select-Object -First 1;\
if($p){{$h=$p.MainWindowHandle;{acao};'achou'}}else{{'nao'}}"
    )
}

#[cfg(windows)]
fn powershell(script: &str) -> String {
    use std::os::windows::process::CommandExt;
    crate::oculto::cmd("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", script])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

#[cfg(not(windows))]
fn powershell(_: &str) -> String {
    String::new()
}

fn navegador() -> Option<&'static str> {
    ["C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe", "C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe",
     "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe", "C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe"]
        .into_iter().find(|c| Path::new(c).exists())
}

/// "Chamar" Jarvis/Alma: a janela e a MINIATURA (processo proprio, aparece sozinha quando o agente acorda). Aqui so garante que ela esta de pe.
pub fn mostrar(_dados: &Path, _j: Janela) {
    crate::miniatura::garantir();
}

/// Descansar: a miniatura some sozinha quando o agente dorme; nada a fazer aqui.
pub fn minimizar(_j: Janela) {}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn janela_abre_como_aplicativo_proprio_e_vem_para_a_frente() {
        let j = jarvis(8865);
        let a = argumentos(&j, Path::new("D:/dados"));
        assert!(a[0] == "--app=http://127.0.0.1:8865/hud" && a.iter().any(|x| x.starts_with("--user-data-dir=") && x.contains("janela-jarvis")));
        let r = roteiro(j.titulo, false, 520, 780, false);
        assert!(r.contains("SetForegroundWindow") && r.contains("ShowWindow($h,9)") && r.contains("'Jarvis*Voz*'"));
        let c = roteiro("Alma", false, 420, 540, true);
        assert!(c.contains("GetSystemMetrics(0)-420") && roteiro("Alma", true, 420, 540, true).contains("ShowWindow($h,6)"));
        assert!(alma(8865).url.ends_with("/alma") && alma(8865).perfil != j.perfil);
    }
}
