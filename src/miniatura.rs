//! Miniatura de voz (Jarvis e Alma): o servidor so oferece a pagina, o quadro para quem abre a pagina no navegador e a garantia de que o
//! processo proprio `brani-mini` esta de pe. A decisao de aparecer/sumir mora em `miniatura_logica` (a mesma que o processo usa).
use crate::miniatura_logica::Mini;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

static MINI: Mutex<Option<Mini>> = Mutex::new(None);
static LANCADO: Mutex<Option<Instant>> = Mutex::new(None);

pub fn pagina() -> crate::Resp {
    crate::pagina(include_str!("miniatura.html"))
}

/// /hud e /alma nao sao mais telas grandes: levam a miniatura.
pub fn redirecionar() -> crate::Resp {
    let r = crate::resposta(String::new(), "text/plain", 302);
    r.with_header(tiny_http::Header::from_bytes("Location", "/miniatura").unwrap())
}

/// Quadro para a pagina aberta no navegador (o processo da miniatura calcula o dele por conta propria).
pub fn quadro(d: Option<&Value>, agora: f64) -> Value {
    crate::travar(&MINI).get_or_insert_with(Mini::default).passo(d, agora)
}

pub fn exe_mini() -> Option<PathBuf> {
    std::env::var_os("BRANIAC_MINI").map(PathBuf::from).into_iter()
        .chain(std::env::current_exe().ok().into_iter().flat_map(|e| e.ancestors().map(|p| p.join("brani-mini").join("brani-mini.exe")).collect::<Vec<_>>()))
        .chain([PathBuf::from(r"E:\braniac-rs\brani-mini\brani-mini.exe")])
        .find(|p| p.is_file())
}

/// Garante o processo da miniatura (ele mesmo recusa uma segunda copia pela trava de porta). Nunca espera, nunca abre console.
pub fn garantir() -> bool {
    if cfg!(test) {
        return true;
    }
    let mut l = crate::travar(&LANCADO);
    if l.is_some_and(|t| t.elapsed() < Duration::from_secs(20)) {
        return true;
    }
    let Some(exe) = exe_mini() else { return false };
    *l = Some(Instant::now());
    crate::oculto::cmd(exe).spawn().is_ok()
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;

    #[test]
    fn alma_leva_a_miniatura_e_a_pagina_traz_os_botoes() {
        let r = redirecionar();
        assert_eq!(r.status_code().0, 302);
        assert!(r.headers().iter().any(|h| h.field.equiv("Location") && h.value.as_str() == "/miniatura"));
        let h = include_str!("miniatura.html");
        for chave in ["Abrir aplicativo", "bMic", "bMudo", "bDorme", "id=\"x\"", "--m-jarvis", "--m-alma", "prefers-reduced-motion"] {
            assert!(h.contains(chave), "{chave}");
        }
    }

    #[test]
    fn quadro_do_servidor_so_aparece_ao_ser_chamado() {
        let dorme = json!({ "estado": "dormindo", "agente": "jarvis", "alma": { "ativa": false }, "historico": [] });
        assert_eq!(quadro(Some(&dorme), 0.0)["visivel"], false);
        let acorda = json!({ "estado": "acordado", "agente": "jarvis", "alma": { "ativa": false }, "historico": [] });
        assert_eq!(quadro(Some(&acorda), 1.0)["visivel"], true);
        assert!(garantir()); // em teste nunca lanca processo
    }

    /// Regressao: a voz virou uma aba/janela grande "fora do sistema". O menu do app nao tem mais "Voz" e as telas grandes sumiram.
    #[test]
    fn menu_sem_aba_voz_e_sem_telas_grandes() {
        let p = include_str!("painel_b1.html");
        assert!(p.contains("[\"voz\",\"Voz\"]"), "a aba Voz continua no menu");
        let h = include_str!("hud_b1.html");
        assert!(!h.contains("Abrir aplicativo completo") && !h.contains("'←'"), "sem botoes de voltar/abrir app dentro da aba");
        let t = include_str!("liga_telas.rs");
        assert!(!t.contains("include_str!(\"alma_b1.html\")"));
    }

    /// Regressao: "Abrir aplicativo" nao abria. A miniatura le a resposta ate o fim do tempo (antes perdia tudo se a conexao nao fechava), confere
    /// se o app apareceu e, se nao, abre o exe sem console; o servidor entende "/app" e a rota nao vai para a tela grande.
    #[test]
    fn abrir_aplicativo_chama_a_rota_certa_e_confere() {
        let m = include_str!("../brani-mini/src/main.rs");
        assert!(m.contains("\"/api/app/abrir\"") && m.contains("\"rota\": \"/app\"") && m.contains("\"/api/app/vivo\"") && m.contains("B.R.A.N.I.exe"));
        assert!(m.contains("\"app\" => abrir_app()") && m.contains("0x0800_0000") && m.contains("corpo_http"));
        let h = include_str!("miniatura.html");
        assert!(h.contains("cmd(\"app\")") && h.contains("T.invoke(\"acao\""));
        // causa real do "nao funciona": o atalho do fetch derrubava o cabecalho Tauri-Invoke-Key do IPC; dentro do Tauri ele nao pode existir
        assert!(h.contains("if(!window.__TAURI_INTERNALS__){var _f=window.fetch"));
        assert!(crate::app_painel::pedir("/app")); // teste: nao abre nada
        assert_eq!(crate::app_painel::sinal()["rota"], "/app");
    }

    #[test]
    fn expandir_tem_botao_chat_e_envio_pelas_rotas_de_sempre() {
        let h = include_str!("miniatura.html");
        for chave in ["bEnv", "flex-wrap:nowrap", "bExp", "id=\"chat\"", "id=\"txt\"", "e.shiftKey", "/api/chat", "scrollbar-width:thin"] {
            assert!(h.contains(chave), "{chave}");
        }
        let m = include_str!("../brani-mini/src/main.rs");
        assert!(m.contains("EXP_LARG: f64 = 358.0") && m.contains("EXP_ALT: f64 = 488.0"));
        assert!(m.contains("\"/api/chat\"") && m.contains("\"/api/escuta/frase\"") && m.contains("set_focusable(exp)") && m.contains("work_area()") && m.contains("ajustar(&j, &ctx, false)"));
    }
}
