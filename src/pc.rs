//! O Jarvis mexe no PC do dono: abrir pagina/pasta/documento, procurar e listar arquivos (braniac/pc.py).
//! Trava: NUNCA executa programa/script (PERIGOSOS), nunca apaga, move ou altera arquivo. Este modulo so LE o disco;
//! abrir de verdade (navegador, programa padrao) entra como closure de quem chama.
//! De fora: expansao de variaveis de ambiente (so `~` e `%USERPROFILE%` viram a pasta do usuario).
#![allow(dead_code)]
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const PERIGOSOS: [&str; 26] = [
    ".exe", ".msi", ".bat", ".cmd", ".ps1", ".vbs", ".vbe", ".js", ".jse", ".wsf", ".scr", ".com", ".pif", ".lnk", ".jar", ".reg", ".hta", ".cpl",
    ".dll", ".sys", ".py", ".pyw", ".sh", ".appx", ".msix", ".url",
];
pub const RAIZES: [&str; 9] = ["Desktop", "Documents", "Downloads", "Pictures", "Music", "Videos", "OneDrive", "Projetos", "ArtCraft-Trabalho"];
const PULAR: [&str; 11] =
    ["AppData", "node_modules", ".git", "__pycache__", "target", ".venv", "venv", "$Recycle.Bin", "Windows", "Program Files", "Program Files (x86)"];

fn caminho(alvo: &str, casa: &Path) -> PathBuf {
    let a = alvo.trim().trim_matches('"');
    let resto = a.strip_prefix('~').or_else(|| a.get(..13).filter(|p| p.eq_ignore_ascii_case("%userprofile%")).map(|_| &a[13..]));
    match resto {
        Some(r) => casa.join(r.trim_start_matches(['/', '\\'])),
        None => PathBuf::from(a),
    }
}

/// True para arquivo que o Jarvis nao abre (programa, script, atalho).
pub fn perigoso(p: &Path) -> bool {
    p.extension().map(|e| format!(".{}", e.to_string_lossy().to_lowercase())).is_some_and(|e| PERIGOSOS.contains(&e.as_str()))
}

pub fn abrir(alvo: &str, casa: &Path, abrir_url: &dyn Fn(&str) -> Result<(), String>, abrir_caminho: &dyn Fn(&Path) -> Result<(), String>) -> String {
    let alvo = alvo.trim().trim_matches('"');
    let min = alvo.to_lowercase();
    if min.starts_with("http://") || min.starts_with("https://") {
        return match abrir_url(alvo) {
            Ok(()) => format!("Abri a pagina {alvo} no navegador."),
            Err(e) => format!("Nao consegui abrir a pagina {alvo}: {e}"),
        };
    }
    let p = caminho(alvo, casa);
    // alvo vazio no Python virava a pasta corrente; aqui nao abre nada
    if alvo.is_empty() || !p.exists() {
        return format!("Nao achei '{alvo}'. Use a acao procurar para achar o caminho certo.");
    }
    if p.is_file() && perigoso(&p) {
        let ext = p.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
        return format!("Nao abro .{ext} (programa/script pode ser perigoso). O dono pode abrir ele mesmo: {}", p.display());
    }
    match abrir_caminho(&p) {
        Ok(()) => format!("Abri {} {}.", if p.is_dir() { "a pasta" } else { "o arquivo" }, p.display()),
        Err(e) => format!("Nao consegui abrir {}: {e}", p.display()),
    }
}

pub fn listar(pasta: &str, casa: &Path, n: usize) -> String {
    let p = if pasta.trim().is_empty() { casa.to_path_buf() } else { caminho(pasta, casa) };
    let Ok(rd) = p.read_dir() else { return format!("'{pasta}' nao e uma pasta.") };
    let mut itens: Vec<(bool, String, u64)> = rd
        .flatten()
        .map(|e| {
            let m = e.metadata().ok();
            (!m.as_ref().is_some_and(|m| m.is_dir()), e.file_name().to_string_lossy().to_string(), m.map(|m| m.len()).unwrap_or(0))
        })
        .collect();
    itens.sort_by_key(|i| (i.0, i.1.to_lowercase()));
    let linhas: Vec<String> = itens
        .iter()
        .take(n)
        .filter(|i| !i.1.starts_with("desktop.ini"))
        .map(|(arq, nome, tam)| if *arq { format!("{nome} ({} KB)", tam / 1024) } else { format!("[pasta] {nome}") })
        .collect();
    if linhas.is_empty() { format!("{} esta vazia.", p.display()) } else { format!("{}:\n{}", p.display(), linhas.join("\n")) }
}

fn andar(pasta: &Path, termo: &str, n: usize, fim: Instant, achados: &mut Vec<String>) {
    let Ok(rd) = pasta.read_dir() else { return };
    let (mut dirs, mut arqs) = (vec![], vec![]);
    for e in rd.flatten() {
        let nome = e.file_name().to_string_lossy().to_string();
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            if !PULAR.contains(&nome.as_str()) && !nome.starts_with('.') {
                dirs.push(nome);
            }
        } else {
            arqs.push(nome);
        }
    }
    dirs.sort();
    arqs.sort();
    achados.extend(dirs.iter().chain(&arqs).filter(|x| x.to_lowercase().contains(termo)).map(|x| pasta.join(x).display().to_string()));
    for d in dirs {
        if achados.len() >= n || Instant::now() > fim {
            return;
        }
        andar(&pasta.join(d), termo, n, fim, achados);
    }
}

/// Arquivos/pastas cujo NOME contem o termo, dentro das `raizes` (as pastas do usuario).
pub fn procurar(termo: &str, raizes: &[PathBuf], n: usize, limite: Duration) -> String {
    let termo = termo.trim().to_lowercase();
    if termo.chars().count() < 2 {
        return "Diga pelo menos 2 letras do nome.".into();
    }
    let fim = Instant::now() + limite;
    let mut achados = vec![];
    for r in raizes.iter().filter(|r| r.exists()) {
        if achados.len() >= n || Instant::now() > fim {
            break;
        }
        andar(r, &termo, n, fim, &mut achados);
    }
    if achados.is_empty() {
        return format!("Nada com '{termo}' no nome nas pastas do usuario.");
    }
    achados.truncate(n);
    achados.join("\n") + if Instant::now() < fim { "" } else { "\n(busca parou no tempo limite; seja mais especifico)" }
}

pub fn raizes(casa: &Path) -> Vec<PathBuf> {
    RAIZES.iter().map(|n| casa.join(n)).collect()
}

/// So existem tres acoes; qualquer outra (executar, apagar, mover...) e recusada.
pub fn executar(acao: &str, alvo: &str, casa: &Path, abrir_url: &dyn Fn(&str) -> Result<(), String>, abrir_caminho: &dyn Fn(&Path) -> Result<(), String>) -> String {
    match acao {
        "abrir" => abrir(alvo, casa, abrir_url, abrir_caminho),
        "listar" => listar(alvo, casa, 60),
        "procurar" => procurar(alvo, &raizes(casa), 20, Duration::from_secs(15)),
        _ => "acao desconhecida: use abrir, listar ou procurar".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn casa(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-pc-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("Documents").join("contas")).unwrap();
        std::fs::create_dir_all(d.join("Documents").join("node_modules")).unwrap();
        for a in ["Documents/boleto.pdf", "Documents/instala.EXE", "Documents/rodar.ps1", "Documents/atalho.lnk", "Documents/contas/boleto-velho.txt", "Documents/node_modules/boleto.js"] {
            std::fs::write(d.join(a), "x").unwrap();
        }
        d
    }

    #[test]
    fn nunca_abre_programa_nem_script_e_nao_tem_outra_acao() {
        let c = casa("trava");
        let abertos = RefCell::new(vec![]);
        let url = |u: &str| -> Result<(), String> { abertos.borrow_mut().push(u.to_string()); Ok(()) };
        let arq = |p: &Path| -> Result<(), String> { abertos.borrow_mut().push(p.display().to_string()); Ok(()) };
        for ruim in ["instala.EXE", "rodar.ps1", "atalho.lnk"] {
            let r = abrir(&c.join("Documents").join(ruim).display().to_string(), &c, &url, &arq);
            assert!(r.starts_with("Nao abro ."), "{r}");
        }
        for acao in ["executar", "apagar", "mover", "rodar", ""] {
            assert!(executar(acao, &c.join("Documents/boleto.pdf").display().to_string(), &c, &url, &arq).starts_with("acao desconhecida"));
        }
        assert!(abertos.borrow().is_empty());
        assert!(abrir("", &c, &url, &arq).starts_with("Nao achei") && abrir("~/Documents/sumiu.pdf", &c, &url, &arq).starts_with("Nao achei"));
        assert!(abertos.borrow().is_empty());
        // tudo continua no lugar: o modulo so le
        assert!(c.join("Documents/instala.EXE").exists() && c.join("Documents/boleto.pdf").exists());
        assert!(PERIGOSOS.iter().all(|e| perigoso(Path::new(&format!("x{}", e.to_uppercase())))) && !perigoso(Path::new("foto.jpg")));
    }

    #[test]
    fn abre_documento_pasta_e_pagina_pela_closure() {
        let c = casa("abre");
        let abertos = RefCell::new(vec![]);
        let url = |u: &str| -> Result<(), String> { abertos.borrow_mut().push(u.to_string()); Ok(()) };
        let arq = |p: &Path| -> Result<(), String> { abertos.borrow_mut().push(p.display().to_string()); Ok(()) };
        assert!(executar("abrir", "\"~/Documents/boleto.pdf\"", &c, &url, &arq).starts_with("Abri o arquivo"));
        assert!(abrir("%UserProfile%\\Documents", &c, &url, &arq).starts_with("Abri a pasta"));
        assert_eq!(abrir("HTTPS://exemplo.test/a", &c, &url, &arq), "Abri a pagina HTTPS://exemplo.test/a no navegador.");
        assert_eq!(abertos.borrow().len(), 3);
        let falha = |_: &Path| -> Result<(), String> { Err("sem programa".into()) };
        assert!(abrir("~/Documents/boleto.pdf", &c, &url, &falha).starts_with("Nao consegui abrir"));
    }

    #[test]
    fn lista_e_procura_pelo_nome() {
        let c = casa("lista");
        let l = listar("~/Documents", &c, 60);
        let linhas: Vec<&str> = l.lines().collect();
        assert_eq!(&linhas[1..3], ["[pasta] contas", "[pasta] node_modules"]);
        assert!(l.contains("boleto.pdf (0 KB)"));
        assert_eq!(listar("~/nada", &c, 60), "'~/nada' nao e uma pasta.");
        assert!(listar("~/Documents/contas", &c, 0).ends_with("esta vazia."));
        let r = procurar("BOLETO", &raizes(&c), 20, Duration::from_secs(15));
        assert!(r.contains("boleto.pdf") && r.contains("boleto-velho.txt") && !r.contains("node_modules"), "{r}");
        assert_eq!(procurar("b", &raizes(&c), 20, Duration::from_secs(15)), "Diga pelo menos 2 letras do nome.");
        assert!(procurar("zzzz", &raizes(&c), 20, Duration::from_secs(15)).starts_with("Nada com 'zzzz'"));
        assert_eq!(procurar("boleto", &raizes(&c), 1, Duration::from_secs(15)).lines().count(), 1);
    }
}
