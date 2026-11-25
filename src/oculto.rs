//! Unico ponto que cria processo filho: nunca abre janela de console (CREATE_NO_WINDOW).
//! Regra do Dono (23/11/2026): ver AGENTS.md. O teste `sem_command_fora_do_helper` varre o codigo.
use std::ffi::OsStr;
use std::process::Command;

pub const SEM_JANELA: u32 = 0x0800_0000;

/// Como `Command::new`, mas oculto. Janela que o Dono pede (app, HUD, orbe) abre por GUI, nao por console.
pub fn cmd(programa: impl AsRef<OsStr>) -> Command {
    let mut c = Command::new(programa);
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut c, SEM_JANELA);
    c
}

/// O exe e do subsistema "windows" (sem console): sem stdout/stderr herdados, println!/panic iriam para o vazio.
/// Se nao veio redirecionado (atalho, tarefa agendada), manda os dois para `<dados>/brani.log`.
#[cfg(windows)]
pub fn log_em_arquivo(dados: &std::path::Path) {
    use std::os::windows::io::IntoRawHandle;
    extern "system" {
        fn GetStdHandle(n: u32) -> isize;
        fn SetStdHandle(n: u32, h: isize) -> i32;
    }
    const SAIDA: u32 = -11i32 as u32;
    const ERRO: u32 = -12i32 as u32;
    unsafe {
        let h = GetStdHandle(SAIDA);
        if h != 0 && h != -1 {
            return; // ja redirecionado (ligar-brani.bat)
        }
        let _ = std::fs::create_dir_all(dados);
        if let Ok(f) = std::fs::OpenOptions::new().create(true).append(true).open(dados.join("brani.log")) {
            let h = f.into_raw_handle() as isize;
            SetStdHandle(SAIDA, h);
            SetStdHandle(ERRO, h);
        }
    }
}
#[cfg(not(windows))]
pub fn log_em_arquivo(_: &std::path::Path) {}

#[cfg(test)]
mod testes {
    use std::path::Path;

    fn varrer(d: &Path, achados: &mut Vec<String>) {
        for e in std::fs::read_dir(d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                varrer(&p, achados);
            } else if p.extension().is_some_and(|x| x == "rs") && !p.ends_with("oculto.rs") {
                let t = std::fs::read_to_string(&p).unwrap_or_default();
                if t.contains("Command::new") {
                    achados.push(p.to_string_lossy().replace("\\", "/"));
                }
            }
        }
    }

    /// Regressao: janelas de console pretas piscando (23/11/2026). Todo processo filho passa por `oculto::cmd`.
    #[test]
    fn sem_command_fora_do_helper() {
        let raiz = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut achados = vec![];
        varrer(&raiz.join("src"), &mut achados);
        varrer(&raiz.join("brani-app/src"), &mut achados);
        varrer(&raiz.join("brani-mini/src"), &mut achados);
        // Excecao unica: o app Tauri e outro crate (nao enxerga `oculto`); o teste exige que ele ponha CREATE_NO_WINDOW.
        let permitidos = ["brani-app/src/main.rs", "brani-mini/src/main.rs"];
        let fora: Vec<_> = achados.iter().filter(|a| !permitidos.iter().any(|p| a.ends_with(p))).collect();
        assert!(fora.is_empty(), "Command::new fora de oculto.rs (use crate::oculto::cmd): {fora:?}");
        let app = std::fs::read_to_string(raiz.join("brani-app/src/main.rs")).unwrap();
        assert!(app.contains("creation_flags(0x0800_0000)"), "o app precisa criar o filho sem console");
        let mini = std::fs::read_to_string(raiz.join("brani-mini/src/main.rs")).unwrap();
        assert!(mini.contains("creation_flags(&mut c, 0x0800_0000)"), "a miniatura precisa criar o filho sem console");
    }

    #[test]
    fn exe_do_servidor_nao_abre_console() {
        let m = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs")).unwrap();
        assert!(m.contains("windows_subsystem = \"windows\""));
        assert!(m.contains("oculto::log_em_arquivo"));
    }
}
