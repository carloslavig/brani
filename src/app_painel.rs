//! O APLICATIVO (brani-app, Tauri) e a tela principal. Aqui o servidor:
//! - recebe "abra a rota X" (`pedir`): "/" painel, "/hud", "/alma", "/jarvis" (painel na aba do Jarvis). O app, que consulta
//!   `/api/app/sinal` de 1 em 1 s, traz a janela para a frente. App fechado = o servidor lanca o app (uma instancia so);
//!   sem o arquivo do app, devolve false e quem chamou cai na janela de antes (Chrome/Edge `--app`). Nunca uma aba web.
//! - guarda o interruptor "Servidor: ligado/pausado" (por cima de `power`, motivo "servidor"): pausar nao mata nada em curso.
use crate::power;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

struct Estado {
    seq: u64,
    rota: String,
    pedido: Option<Instant>,
    visto: Option<Instant>,
    lancado: Option<Instant>,
}
static E: Mutex<Estado> = Mutex::new(Estado { seq: 0, rota: String::new(), pedido: None, visto: None, lancado: None });
const VIVO_S: u64 = 5; // o app consulta a cada 1 s: sem sinal por 5 s = fechado
const LANCAR_S: u64 = 25; // depois de lancar, espera o app subir antes de tentar de novo

pub fn rota_valida(r: &str) -> &'static str {
    match r.trim() {
        "/hud" => "/hud",
        "/alma" => "/alma",
        "/jarvis" => "/jarvis",
        "/app" => "/app", // botao "Abrir aplicativo completo": painel inteiro e fecha as janelas de voz
        _ => "/",
    }
}

pub fn exe_app() -> Option<PathBuf> {
    let c = std::env::var_os("BRANIAC_APP").map(PathBuf::from).into_iter()
        .chain(std::env::current_exe().ok().into_iter().flat_map(|e| e.ancestors().map(|p| p.join("brani-app").join("B.R.A.N.I.exe")).collect::<Vec<_>>()))
        .chain([PathBuf::from(r"E:\braniac-rs\brani-app\B.R.A.N.I.exe")]);
    c.into_iter().find(|p| p.is_file())
}

fn vivo_em(e: &Estado) -> bool {
    e.visto.is_some_and(|v| v.elapsed() < Duration::from_secs(VIVO_S))
}

pub fn vivo() -> bool {
    vivo_em(&crate::travar(&E))
}

/// Registra o pedido e garante o app: vivo = ele ve o pedido em ate 1 s; fechado = lanca UMA copia. false = nao ha app.
pub fn pedir(rota: &str) -> bool {
    // a Alma chamada e a miniatura; o app nunca recebe /alma nem abre janela grande
    if rota.trim() == "/alma" {
        return crate::miniatura::garantir();
    }
    // /hud = a aba Voz DENTRO do app (o app abre a aba pela rota /jarvis); nunca janela grande separada
    let rota = if rota.trim() == "/hud" { "/jarvis" } else { rota_valida(rota) };
    let mut e = crate::travar(&E);
    e.seq += 1;
    e.rota = rota.to_string();
    e.pedido = Some(Instant::now());
    if vivo_em(&e) || e.lancado.is_some_and(|l| l.elapsed() < Duration::from_secs(LANCAR_S)) {
        return true;
    }
    let Some(exe) = exe_app() else { return false };
    if cfg!(test) {
        return true; // teste nunca abre janela
    }
    e.lancado = Some(Instant::now());
    let mut c = crate::oculto::cmd(exe);
    c.arg(format!("--rota={rota}"));
    c.spawn().is_ok()
}

/// O que o app le a cada segundo. Cada leitura e a prova de que ele esta aberto.
pub fn sinal() -> Value {
    let mut e = crate::travar(&E);
    e.visto = Some(Instant::now());
    let (seq, rota, idade) = (e.seq, e.rota.clone(), e.pedido.map_or(9999, |p| p.elapsed().as_secs()));
    drop(e);
    json!({ "seq": seq, "rota": rota, "idade_s": idade, "servidor": servidor(&crate::pasta_dados())["estado"] })
}

/// Rota antiga do app ja instalado: so o `"jarvis"` com aspas (o app procura esse trecho) e so por 3 s depois do pedido.
pub fn sinal_antigo() -> Value {
    let mut e = crate::travar(&E);
    e.visto = Some(Instant::now());
    if e.rota == "/jarvis" && e.pedido.is_some_and(|p| p.elapsed() < Duration::from_secs(3)) { json!({ "jarvis": true }) } else { json!({}) }
}

// ---------- servidor ligado / pausado ----------

/// "ligado", "parando" (pausado, mas o microfone ou um trabalho de fundo ainda esta fechando) ou "pausado".
pub fn servidor(dados: &Path) -> Value {
    let e = power::estado(dados);
    let pausado = e["pausado"] == true;
    let parando = pausado && (crate::aparelho::microfone_aberto() || crate::liga_casa::fundo_em_curso());
    let estado = if !pausado { "ligado" } else if parando { "parando" } else { "pausado" };
    json!({ "estado": estado, "pausado": pausado, "motivo": e["motivo"], "jogo": e["jogo"], "desde": e["desde"], "app_aberto": vivo() })
}

/// Pausa segura: novos passos, escuta/microfone, vigias e relogios param (cada laco olha `power::pausado`); o que ja esta rodando
/// (render, envio) termina sozinho. O servidor HTTP e o app seguem vivos para religar com um clique.
pub fn pausar(dados: &Path) -> Value {
    power::desligar(dados, "servidor", "");
    servidor(dados)
}

pub fn retomar(dados: &Path) -> Value {
    power::ligar(dados);
    servidor(dados)
}

pub fn get(dados: &Path, url: &str) -> Option<crate::Resp> {
    Some(match url.split('?').next()? {
        "/api/app/sinal" => crate::dado(sinal()),
        "/api/b2/sinal" => crate::dado(sinal_antigo()),
        "/api/app/vivo" => crate::dado(json!({ "vivo": vivo() })),
        "/api/servidor" => crate::dado(servidor(dados)),
        _ => return None,
    })
}

pub fn post(dados: &Path, caminho: &str, v: &Value) -> Option<crate::Resp> {
    Some(match caminho {
        "/api/app/abrir" => {
            let rota = rota_valida(v["rota"].as_str().unwrap_or("/"));
            crate::dado(json!({ "ok": pedir(rota), "rota": rota }))
        }
        "/api/servidor/pausa" => crate::dado(pausar(dados)),
        "/api/servidor/retomar" => crate::dado(retomar(dados)),
        _ => return None,
    })
}

#[cfg(test)]
mod testes {
    use super::*;

    fn pasta(n: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-app-{n}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn interruptor_do_servidor_persiste_pausa_e_retoma() {
        let d = pasta("sw");
        assert_eq!(servidor(&d)["estado"], "ligado");
        assert_eq!(pausar(&d)["pausado"], true);
        assert!(power::pausado(&d)); // os lacos leem o mesmo arquivo: e isso que faz o tick parar
        assert_eq!(servidor(&d)["motivo"], "servidor"); // "gravou o estado": sobrevive a reinicio
        assert!(servidor(&d)["desde"].as_f64().unwrap_or(0.0) > 1.0e9);
        assert!(["parando", "pausado"].contains(&servidor(&d)["estado"].as_str().unwrap()));
        assert_eq!(retomar(&d)["estado"], "ligado");
        assert!(!power::pausado(&d));
    }

    #[test]
    fn pausa_manual_nao_e_desfeita_pelo_modo_jogo() {
        let d = pasta("jogo");
        pausar(&d);
        // jogo fechado: o vigia so religa o que ele mesmo pausou
        assert_eq!(power::vigiar_passo(&d, &["jogo".into()], &|| Default::default()), "");
        assert!(power::pausado(&d));
    }

    #[test]
    fn rota_do_app_so_aceita_as_conhecidas() {
        assert_eq!(rota_valida("/hud"), "/hud"); // so por compatibilidade; `pedir` desvia /hud e /alma para a miniatura
        assert_eq!(rota_valida("https://site.com"), "/");
        assert_eq!(rota_valida("/jarvis"), "/jarvis");
    }

    #[test]
    fn pedido_vira_sinal_e_a_leitura_prova_que_o_app_esta_aberto() {
        assert!(pedir("/alma")); // vai para a miniatura: o app nao recebe pedido de tela grande
        pedir("/app");
        let s = sinal();
        assert_eq!(s["rota"], "/app");
        assert!(vivo()); // quem le o sinal esta vivo
        assert!(!sinal().to_string().contains("\"jarvis\"")); // o app antigo nunca confunde com o pedido do Jarvis
        pedir("/jarvis");
        assert!(sinal_antigo().to_string().contains("\"jarvis\""));
    }
}
