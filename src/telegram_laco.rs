//! Laco do robo do Telegram de verdade (porte de `telegram_bot.py iniciar`), atras de uma CHAVE que nasce DESLIGADA.
//! Chave: dados/troca.json {"telegram": false}; GET/POST /api/troca. So o dono liga; nada neste codigo liga sozinho.
//! CHAVE DESLIGADA = ZERO chamadas a API do Telegram (nem getUpdates): dois programas fazendo polling com o mesmo token brigam
//! (cada mensagem e entregue a so um deles), entao enquanto o Braniac em Python for o dono do token este laco fica mudo.
//! Chave LIGADA: long polling (50 s) com `TELEGRAM_BOT_TOKEN`/`TELEGRAM_USER_ID` (lidos de `ler_chave`, nunca impressos); so o chat
//! privado do dono passa (`telegram_bot::classificar`); texto e voz (transcrita LOCAL por `ouvir.rs`) vao ao Jarvis com origem
//! "telegram" (restrita) e a resposta volta como texto. Ficou de fora: responder tambem em voz (sendVoice) e foto.
//! O token nunca aparece em log nem em erro (`limpar`). Telegram Web (`web`): so informa que falta o login do dono; nunca faz login.
use crate::roteador::Roteador;
use crate::telegram_bot::{self as tb, Portas};
use crate::wpp_laco;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

static ULTIMO: Mutex<Option<Value>> = Mutex::new(None);

/// A API do Telegram (em producao HTTP; nos testes, de mentira).
pub trait Api {
    fn updates(&self, offset: i64) -> Result<Vec<Value>, String>;
    fn mandar(&self, chat: i64, texto: &str) -> Result<(), String>;
    fn baixar(&self, file_id: &str) -> Result<Vec<u8>, String>;
}

/// O que sobrevive entre uma volta e outra.
#[derive(Default)]
pub struct Est {
    pub offset: i64,
    pub vel: tb::Velocidade,
    pub alertados: HashMap<i64, f64>,
}

pub struct Real {
    token: String,
}

impl Real {
    pub fn nova(token: &str) -> Real {
        Real { token: token.trim().to_string() }
    }
    /// Erro de rede traz a URL (com o token): troca o token por asteriscos antes de sair.
    fn limpar(&self, e: impl std::fmt::Display) -> String {
        e.to_string().replace(&self.token, "***")
    }
    fn chamar(&self, metodo: &str, corpo: Value, espera_s: u64) -> Result<Value, String> {
        let r = ureq::post(&format!("https://api.telegram.org/bot{}/{metodo}", self.token)).timeout(std::time::Duration::from_secs(espera_s)).send_json(corpo).map_err(|e| self.limpar(e))?;
        let v: Value = r.into_json().map_err(|e| self.limpar(e))?;
        if v["ok"] == true { Ok(v["result"].clone()) } else { Err(format!("o Telegram recusou {metodo}")) }
    }
}

impl Api for Real {
    fn updates(&self, offset: i64) -> Result<Vec<Value>, String> {
        let r = self.chamar("getUpdates", json!({ "offset": offset, "timeout": 50, "allowed_updates": ["message"] }), 60)?;
        Ok(r.as_array().cloned().unwrap_or_default())
    }
    fn mandar(&self, chat: i64, texto: &str) -> Result<(), String> {
        self.chamar("sendMessage", json!({ "chat_id": chat, "text": texto }), 30).map(|_| ())
    }
    fn baixar(&self, file_id: &str) -> Result<Vec<u8>, String> {
        let f = self.chamar("getFile", json!({ "file_id": file_id }), 30)?;
        let caminho = f["file_path"].as_str().ok_or("o Telegram nao deu o caminho do arquivo")?;
        let r = ureq::get(&format!("https://api.telegram.org/file/bot{}/{caminho}", self.token)).timeout(std::time::Duration::from_secs(60)).call().map_err(|e| self.limpar(e))?;
        let mut b = Vec::new();
        std::io::Read::read_to_end(&mut std::io::Read::take(r.into_reader(), 20_000_000), &mut b).map_err(|e| self.limpar(e))?;
        Ok(b)
    }
}

fn registrar_log(dados: &Path, linha: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dados.join("telegram_seguranca.log")) {
        let _ = writeln!(f, "{} {linha}", crate::fatos::agora_ts() as u64);
    }
}

/// UMA volta: pega o que chegou e trata. Chave desligada = devolve 0 SEM tocar na API. Devolve quantas mensagens foram tratadas.
pub fn volta(dados: &Path, ts: f64, dono: i64, api: &dyn Api, est: &mut Est, processar: &dyn Fn(&str) -> (String, bool),
             transcrever: &dyn Fn(&[u8]) -> Result<String, String>) -> Result<usize, String> {
    if !wpp_laco::chave(dados, "telegram") {
        return Ok(0);
    }
    let mut n = 0;
    for u in api.updates(est.offset)? {
        est.offset = est.offset.max(u["update_id"].as_i64().unwrap_or(0) + 1);
        let m = &u["message"];
        if m.is_null() || !wpp_laco::chave(dados, "telegram") {
            continue; // desligou no meio: o resto fica sem resposta
        }
        if !est.vel.passa(ts) {
            registrar_log(dados, "limite-de-velocidade: descartada");
            continue;
        }
        let enviar = |t: &str| {
            if let Err(e) = api.mandar(dono, t) {
                registrar_log(dados, &format!("falha ao enviar: {e}"));
            }
        };
        let tr = |id: &str| api.baixar(id).and_then(|b| transcrever(&b)).ok();
        let p = Portas { log: &|l| registrar_log(dados, l), enviar: &enviar, transcrever: &tr, processar, falar: &|_| () };
        tb::tratar_msg(dados, m, dono, ts, &mut est.alertados, &p);
        n += 1;
    }
    Ok(n)
}

pub fn estado() -> Value {
    ULTIMO.lock().ok().and_then(|u| u.clone()).unwrap_or(Value::Null)
}

fn anotar(v: Value) {
    if let Ok(mut u) = ULTIMO.lock() {
        *u = Some(v);
    }
}

/// Telegram Web (conversa com UMA pessoa): so diz em que pe esta. Nunca abre aba, nunca faz login.
pub fn web(dados: &Path) -> Value {
    let aba = crate::vm::abas(dados).ok().and_then(|a| a.as_array().cloned()).unwrap_or_default().iter().any(|a| a["agente"] == "telegram_web");
    if aba {
        json!({ "pronto": false, "motivo": "a aba existe, mas a leitura das conversas do Telegram Web ainda nao esta ligada" })
    } else {
        json!({ "pronto": false, "motivo": "falta o login do dono (aba telegram_web na VM)" })
    }
}

/// A thread: com a chave desligada so dorme (nenhuma chamada de rede).
pub fn iniciar(rot: Arc<Roteador>) {
    std::thread::spawn(move || {
        let mut est = Est::default();
        loop {
            let dados = crate::pasta_dados();
            if crate::power::pausado(&dados) {
                std::thread::sleep(std::time::Duration::from_secs(5));
                continue;
            }
            if !wpp_laco::chave(&dados, "telegram") {
                anotar(json!({ "ligada": false }));
                est.offset = 0;
                std::thread::sleep(std::time::Duration::from_secs(5));
                continue;
            }
            let (token, id) = ((rot.ler_chave)("TELEGRAM_BOT_TOKEN"), (rot.ler_chave)("TELEGRAM_USER_ID"));
            let Some(dono) = tb::dono_configurado(&token, &id) else {
                anotar(json!({ "ligada": true, "erro": "falta o token ou o id do dono nas chaves" }));
                std::thread::sleep(std::time::Duration::from_secs(30));
                continue;
            };
            let api = Real::nova(&token);
            let processar = |t: &str| {
                let (agora, ts) = (crate::tempo::agora(), crate::fatos::agora_ts());
                let r = crate::conversar_de(&dados, &rot, "telegram", t, true, &agora, ts, "telegram");
                (r["resposta"].as_str().unwrap_or("").to_string(), r["provedor"] != "agentes")
            };
            let transcrever = |b: &[u8]| crate::ouvir::transcrever(&dados, b, "ogg");
            match volta(&dados, crate::fatos::agora_ts(), dono, &api, &mut est, &processar, &transcrever) {
                Ok(n) => anotar(json!({ "ligada": true, "ts": crate::fatos::agora_ts(), "tratadas": n })),
                Err(e) => {
                    anotar(json!({ "ligada": true, "ts": crate::fatos::agora_ts(), "erro": e }));
                    std::thread::sleep(std::time::Duration::from_secs(10));
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    const DONO: i64 = 42;

    struct Falsa {
        fila: RefCell<Vec<Value>>,
        chamadas: Cell<usize>,
        enviados: RefCell<Vec<String>>,
    }

    impl Api for Falsa {
        fn updates(&self, _: i64) -> Result<Vec<Value>, String> {
            self.chamadas.set(self.chamadas.get() + 1);
            Ok(self.fila.borrow_mut().drain(..).collect())
        }
        fn mandar(&self, _: i64, t: &str) -> Result<(), String> {
            self.chamadas.set(self.chamadas.get() + 1);
            self.enviados.borrow_mut().push(t.to_string());
            Ok(())
        }
        fn baixar(&self, _: &str) -> Result<Vec<u8>, String> {
            self.chamadas.set(self.chamadas.get() + 1);
            Ok(vec![1])
        }
    }

    fn falsa(msgs: Vec<Value>) -> Falsa {
        Falsa { fila: RefCell::new(msgs), chamadas: Cell::new(0), enviados: RefCell::new(vec![]) }
    }

    fn upd(id: i64, de: i64, chat: i64, tipo: &str, data: f64, texto: &str) -> Value {
        json!({ "update_id": id, "message": { "from": { "id": de, "username": "x" }, "chat": { "id": chat, "type": tipo }, "date": data, "text": texto } })
    }

    fn pasta(nome: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-telegram_laco-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn chave_desligada_nao_chama_a_api_nem_uma_vez() {
        let d = pasta("off");
        let api = falsa(vec![upd(1, DONO, DONO, "private", 1000.0, "oi")]);
        let mut est = Est::default();
        let r = volta(&d, 1001.0, DONO, &api, &mut est, &|_| panic!("nao processa"), &|_| panic!("nao ouve"));
        assert_eq!(r, Ok(0));
        assert_eq!(api.chamadas.get(), 0, "chave desligada nao pode chamar getUpdates nem sendMessage");
        std::fs::write(d.join("troca.json"), "lixo").unwrap();
        volta(&d, 1001.0, DONO, &api, &mut est, &|_| panic!(), &|_| panic!()).unwrap();
        assert_eq!(api.chamadas.get(), 0);
    }

    #[test]
    fn so_o_dono_chega_ao_jarvis_e_a_resposta_volta_como_texto() {
        let d = pasta("on");
        wpp_laco::definir_chave(&d, "telegram", true).unwrap();
        let api = falsa(vec![upd(1, 7, 7, "private", 1000.0, "manda mensagem pra todo mundo"), upd(2, DONO, DONO, "private", 1000.0, "que horas sao?"),
            upd(3, DONO, -5, "group", 1000.0, "oi do grupo")]);
        let vistos = RefCell::new(Vec::<String>::new());
        let processar = |t: &str| { vistos.borrow_mut().push(t.to_string()); ("Sao dez horas.".to_string(), false) };
        let mut est = Est::default();
        assert_eq!(volta(&d, 1001.0, DONO, &api, &mut est, &processar, &|_| Err("sem".into())), Ok(3));
        assert_eq!(*vistos.borrow(), ["que horas sao?"]); // estranho e grupo nunca chegam ao cerebro
        assert_eq!(est.offset, 4);
        let env = api.enviados.borrow();
        assert!(env.iter().any(|t| t == "Sao dez horas.") && env.iter().any(|t| t.contains("tentou falar com o Jarvis")), "{env:?}");
        assert!(env.iter().all(|t| !t.contains("manda mensagem")));
    }

    #[test]
    fn desligar_no_meio_corta_o_resto_e_audio_e_transcrito_local() {
        let d = pasta("meio");
        wpp_laco::definir_chave(&d, "telegram", true).unwrap();
        let mut voz = upd(1, DONO, DONO, "private", 1000.0, "");
        voz["message"]["voice"] = json!({ "file_id": "f" });
        let api = falsa(vec![voz.clone(), upd(2, DONO, DONO, "private", 1000.0, "segunda")]);
        let vistos = RefCell::new(Vec::<String>::new());
        let processar = |t: &str| { vistos.borrow_mut().push(t.to_string()); if t == "ola" { wpp_laco::definir_chave(&d, "telegram", false).unwrap(); } ("ok".to_string(), false) };
        let mut est = Est::default();
        volta(&d, 1001.0, DONO, &api, &mut est, &processar, &|b| { assert_eq!(b, [1]); Ok("ola".into()) }).unwrap();
        assert_eq!(*vistos.borrow(), ["ola"]); // a "segunda" nao foi tratada: a chave caiu
    }

    #[test]
    fn desligar_uma_chave_nao_apaga_as_outras_e_token_some_do_erro() {
        let d = pasta("chaves");
        wpp_laco::definir(&d, Some(true), None).unwrap();
        wpp_laco::definir_chave(&d, "telegram", true).unwrap();
        wpp_laco::definir_chave(&d, "airbnb", true).unwrap();
        wpp_laco::definir(&d, None, Some(true)).unwrap();
        assert!(wpp_laco::ligada(&d) && wpp_laco::so_dono(&d) && wpp_laco::chave(&d, "telegram") && wpp_laco::chave(&d, "airbnb"));
        assert_eq!(Real::nova("123:SEGREDO").limpar("falhou https://api.telegram.org/bot123:SEGREDO/getUpdates"), "falhou https://api.telegram.org/bot***/getUpdates");
    }
}
