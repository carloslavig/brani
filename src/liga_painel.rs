//! Ligacao (liga_painel): a tela do B.R.A.N.I.A.C 1 (painel_b1.html, copia do painel.html do Braniac em Python) e as rotas que
//! ela chama com o nome do Python (/api/b2/estado, /api/b2/conversa, /api/b2/acao, /api/b2/chaves...). O armazenamento e o do
//! b2.rs; aqui so mudam nome e formato. O que o B.R.A.N.I ainda nao tem responde vazio com um aviso, nunca dado inventado.
//! Nada aqui envia para fora nem faz login: o WhatsApp direto fica desligado e a chave so grava no .env proprio do B.R.A.N.I.
use crate::executor::Ambiente;
use crate::roteador::Roteador;
use crate::{agenda, agentes, b2, dado, erro, fatos, painel_claw, skills, tempo, vozes_agentes, vozes_catalogo, Resp};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;

const AINDA_NAO: &str = "ainda não disponível no B.R.A.N.I";

/// Pagina curta, no fundo escuro da tela, para as telas embutidas que o B.R.A.N.I ainda nao tem.
fn ainda_nao(nome: &str) -> Resp {
    crate::pagina(&format!(
        "<!doctype html><meta charset=utf-8><title>{nome}</title><body style=\"margin:0;background:#0b0d12;color:#9aa3b2;font:15px system-ui;display:grid;place-items:center;height:100vh\"><div style=\"text-align:center\"><b style=\"color:#e6e9ef;font-size:18px\">{nome}</b><br>{AINDA_NAO}.</div>"))
}

fn skills_lista(dados: &Path) -> Value {
    let base = Ambiente::seguro(dados).locais.skills;
    json!(skills::listar(&base).into_iter().map(|(n, d, _)| json!({ "nome": n, "descricao": d })).collect::<Vec<_>>())
}

fn hoje() -> String {
    let (y, m, d) = tempo::agora().data.partes();
    format!("{d:02}/{m:02}/{y:04}")
}

fn fonte(bytes: &'static [u8]) -> Resp {
    tiny_http::Response::from_data(bytes.to_vec())
        .with_header(tiny_http::Header::from_bytes("Content-Type", "font/woff2").unwrap())
        .with_header(tiny_http::Header::from_bytes("Cache-Control", "max-age=604800").unwrap())
}

pub fn get(dados: &Path, rot: &Arc<Roteador>, url: &str) -> Option<Resp> {
    let ts = fatos::agora_ts();
    Some(match url.split('?').next().unwrap_or("") {
        "/" | "/painel" => crate::pagina(include_str!("painel_b1.html")),
        // a tela de projetos antiga continua em /b2?antigo=1 (o link esta no proprio painel)
        "/b2" if !url.contains("antigo=1") => crate::pagina(include_str!("painel_b1.html")),
        "/inicio" => crate::pagina(include_str!("inicio.html")),
        // 21/11: sem a fonte a tela caia na letra padrao do navegador e nao ficava igual ao B.R.A.N.I.A.C 1 (Instrument Sans, licenca SIL OFL em src/fontes)
        "/claw/fonts/instrument-sans-latin.woff2" => fonte(include_bytes!("fontes/instrument-sans-latin.woff2")),
        "/claw/fonts/instrument-sans-latin-ext.woff2" => fonte(include_bytes!("fontes/instrument-sans-latin-ext.woff2")),
        // telas embutidas: as que o B.R.A.N.I tem com outro nome apontam para ela, as outras avisam
        "/sauraki" => crate::pagina(include_str!("escritorio.html")),
        "/magno" => crate::pagina(include_str!("esteira.html")),
        "/hud" => ainda_nao("Voz"),
        "/email" => ainda_nao("E-mail"),
        "/api/b2/estado" => match b2::estado(dados, &agentes::nomes(dados), ts) {
            Ok(mut e) => {
                e["skills"] = skills_lista(dados);
                e["status"] = crate::status(dados, rot, ts);
                dado(e)
            }
            Err(x) => erro(x, 500),
        },
        "/api/b2/conversa" => match crate::parametro(url, "id").and_then(|i| i.parse::<i64>().ok()).filter(|i| *i > 0) {
            Some(id) => dado(json!({ "mensagens": b2::mensagens(&rot.store, id, 60) })),
            None => erro("id invalido", 400),
        },
        "/api/b2/chaves" => dado(json!({ "chaves": b2::chaves(&|k| Some((rot.ler_chave)(k)).filter(|v| !v.is_empty())) })),
        "/api/b2/aparencia" => dado(b2::aparencia(dados)),
        "/api/painel2" => {
            let agora = tempo::agora();
            let hoje = agenda::ferramenta(&dados.join("agenda.json"), &json!({ "acao": "listar" }), &agora);
            dado(painel_claw::dados(dados, Path::new("."), ts, &hoje))
        }
        "/api/termico" => dado(json!({ "pronto": false, "aviso": format!("temperatura {AINDA_NAO}") })),
        // perguntado=true: a tela nao abre a janela de instalar habilidades de terceiros
        "/api/habilidades-terceiros" => dado(json!({ "perguntado": true, "itens": [], "aviso": format!("habilidades de terceiros {AINDA_NAO}") })),
        "/api/wpp_direto" => dado(json!({ "estado": "parado", "aviso": format!("WhatsApp direto ainda desligado no B.R.A.N.I") })),
        _ => return None,
    })
}

pub fn post(dados: &Path, caminho: &str, v: &Value) -> Option<Resp> {
    let ts = fatos::agora_ts();
    let t = |k: &str| v[k].as_str().unwrap_or("").to_string();
    let agente = || { let a = t("agente"); if a.is_empty() { "jarvis".to_string() } else { a } };
    let pos = || v["posicao"].as_i64().unwrap_or(1);
    let ok = |r: Result<Value, String>| match r {
        Ok(e) => dado(json!({ "ok": true, "escolha": e })),
        Err(x) => erro(x, 400),
    };
    Some(match caminho {
        "/api/b2/acao" => b2::acao(dados, &dados.join("skills"), v, &agentes::nomes(dados), ts, &hoje()).map(dado).unwrap_or_else(|x| erro(x, 400)),
        "/api/b2/chave" => {
            // BRANIAC_ENV e o .env do Braniac em Python: so leitura
            if std::env::var_os("BRANIAC_ENV").is_some() {
                return Some(erro("o .env em uso é o do Braniac em Python (somente leitura); cole a chave lá", 409));
            }
            match b2::salvar_chave(&dados.join(".env"), &t("env"), &t("valor")) {
                Ok(_) => dado(json!({ "ok": true, "aviso": "chave guardada; vale depois de reiniciar o B.R.A.N.I" })),
                Err(x) => erro(x, 400),
            }
        }
        // a interface local arma ou desarma (minutos 0); o B.R.A.N.I nao envia nada de qualquer modo
        "/api/autonomia" => {
            crate::power::armar(dados, v["minutos"].as_i64().unwrap_or(0), ts);
            dado(crate::power::estado(dados))
        }
        "/api/habilidades-terceiros/recusar" => dado(json!({ "perguntado": true, "itens": [] })),
        "/api/habilidades-terceiros/instalar" => erro(format!("instalar habilidades de terceiros {AINDA_NAO}"), 501),
        "/api/wpp_direto/iniciar" => erro("WhatsApp direto ainda desligado no B.R.A.N.I: nada é iniciado nem enviado", 501),
        "/api/vozes/escolher" => {
            let r = crate::com_fontes(|f| vozes_catalogo::escolher(dados, f, &t("motor"), &t("voz"), &agente(), pos()));
            // combinacao pronta: voz e ajustes de uma vez (so na primaria)
            match r {
                Ok(_) if pos() == 1 && v["ajustes"].is_object() => ok(vozes_agentes::ajustar(dados, &agente(), &v["ajustes"])),
                r => ok(r),
            }
        }
        "/api/vozes/ajustar" => ok(vozes_agentes::ajustar(dados, &agente(), &v["ajustes"])),
        "/api/vozes/tirar_reserva" => ok(vozes_agentes::tirar_reserva(dados, &agente(), v["posicao"].as_i64().unwrap_or(2))),
        "/api/vozes/limpar" => match vozes_agentes::limpar(dados, &agente()) {
            Ok(()) => dado(json!({ "ok": true })),
            Err(x) => erro(x, 400),
        },
        "/api/vozes/amostra" => erro(format!("amostra falada {AINDA_NAO}"), 503),
        "/api/vozes/baixar" => erro(format!("baixar voz {AINDA_NAO}"), 503),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rota_que_falta_recusa_e_acao_cria_conversa() {
        let d = std::env::temp_dir().join(format!("braniac-liga-painel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let codigo = |c: &str, v: Value| post(&d, c, &v).map(|r| r.status_code().0);
        assert_eq!(codigo("/api/vozes/amostra", json!({})), Some(503));
        assert_eq!(codigo("/api/habilidades-terceiros/instalar", json!({})), Some(501));
        assert_eq!(codigo("/api/wpp_direto/iniciar", json!({})), Some(501));
        assert_eq!(codigo("/api/autonomia", json!({ "minutos": 0 })), Some(200));
        assert_eq!(codigo("/api/nao-existe", json!({})), None);
        assert_eq!(codigo("/api/b2/acao", json!({ "acao": "criar_conversa", "projeto": 1, "modulos": [] })), Some(200));
        assert_eq!(codigo("/api/b2/acao", json!({ "acao": "voa" })), Some(400));
    }
}
