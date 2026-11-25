//! Ligacao (liga_b2): a tela principal /b2, com projetos e conversas separadas. Chamado por main.rs nos tres ganchos (get, post, tick).
//! O estado e as acoes (criar, renomear, mover, arquivar, projeto) continuam em GET/POST /api/b2, do main.rs. Aqui entram so o
//! historico de uma conversa, o turno de conversa de projeto e o relatorio da noite.
//! Cada conversa e a "b2:<id>" do historico (nao se mistura com a conversa unica do Jarvis) e as instrucoes do projeto vao coladas
//! no fim da fala, como o `b2::mensagens` espera (ele tira esse trecho antes de mostrar).
//! Ficou de fora: os agentes liberados por conversa (`b2::modulos_da_conversa`): o `conversar` do main.rs nao recebe essa lista.
use crate::roteador::Roteador;
use crate::tempo::Agora;
use crate::{b2, fatos, power, tempo, Resp};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;

/// Id de conversa: numero inteiro positivo, ou texto so com digitos. Qualquer outro caractere e recusado (o id vira o nome do
/// historico, "b2:<id>").
fn conv_id(v: &Value) -> Option<i64> {
    let n = match v {
        Value::String(s) if !s.is_empty() && s.len() <= 15 && s.bytes().all(|b| b.is_ascii_digit()) => s.parse().ok()?,
        _ => v.as_i64()?,
    };
    (n > 0).then_some(n)
}

/// Um turno numa conversa de projeto. `conversar` = (conversa "b2:<id>", fala ja com o contexto do projeto) -> corpo da resposta.
/// Erro = (mensagem, codigo HTTP); nesse caso `conversar` nem e chamado.
fn chat(dados: &Path, v: &Value, ts: f64, conversar: &dyn Fn(&str, &str) -> Value) -> Result<Value, (String, u16)> {
    let id = conv_id(&v["conv"]).or_else(|| conv_id(&v["conversa"])).ok_or_else(|| ("conversa inválida".to_string(), 400))?;
    let texto = v["texto"].as_str().unwrap_or("").trim();
    if texto.is_empty() {
        return Err(("mande {\"conv\": id, \"texto\": \"...\"}".into(), 400));
    }
    let ctx = b2::contexto_da_conversa(dados, id, ts).map_err(|_| ("essa conversa não existe".to_string(), 404))?;
    let _ = b2::titulo_inicial(dados, id, texto, ts);
    // comando ("/status") nao vai a IA: segue limpo, sem o contexto do projeto
    let fala = if texto.starts_with('/') { texto.to_string() } else { format!("{texto}{ctx}") };
    let mut r = conversar(&format!("b2:{id}"), &fala);
    r["conv"] = json!(id);
    Ok(r)
}

/// Rota GET deste grupo; None = nao e minha.
pub fn get(dados: &Path, rot: &Arc<Roteador>, url: &str) -> Option<Resp> {
    Some(match url.split('?').next().unwrap_or("") {
        "/b2" => crate::pagina(include_str!("b2.html")),
        "/api/b2/mensagens" => match crate::parametro(url, "conv").and_then(|c| conv_id(&json!(c))) {
            Some(id) => crate::dado(json!({ "conv": id, "mensagens": b2::mensagens(&rot.store, id, 60) })),
            None => crate::erro("conversa inválida", 400),
        },
        "/api/b2/relatorio" => crate::dado(json!({ "relatorio": b2::relatorio(dados) })),
        _ => return None,
    })
}

/// Rota POST deste grupo (ja passou pelo cabecalho X-Braniac); None = nao e minha.
pub fn post(dados: &Path, rot: &Arc<Roteador>, caminho: &str, v: &Value) -> Option<Resp> {
    if caminho != "/api/b2/chat" {
        return None;
    }
    let (agora, ts) = (tempo::agora(), fatos::agora_ts());
    // igual ao /api/chat: desligado, so os comandos sao atendidos e nada e observado
    let ligado = !power::pausado(dados);
    let r = chat(dados, v, ts, &|conv, fala| crate::conversa_cmd::tentar(dados, rot, conv, fala).unwrap_or_else(|| crate::conversar(dados, rot, conv, fala, v["ferramentas"] != false, &agora, ts)));
    Some(match r {
        Ok(r) => {
            if ligado {
                // memoria e Alma fora do pedido (ela pode chamar o modelo local); recebem so a fala, sem as instrucoes do projeto
                let (rot, d, conv) = (rot.clone(), dados.to_path_buf(), format!("b2:{}", r["conv"]));
                let (texto, resposta) = (v["texto"].as_str().unwrap_or("").trim().to_string(), r["resposta"].as_str().unwrap_or("").to_string());
                std::thread::spawn(move || crate::depois(&d, &rot, &conv, &texto, &resposta, &agora, ts));
            }
            crate::dado(r)
        }
        Err((e, codigo)) => crate::erro(e, codigo),
    })
}

/// Relogio de 60 s: a tela nao precisa de nada aqui.
pub fn tick(_dados: &Path, _rot: &Roteador, _agora: &Agora, _ts: f64, _minuto: u32) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::sync::Mutex;

    fn dir(nome: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("braniac-liga-b2-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(p.join("pessoal.json"), r#"{"nome": "Zuleica"}"#).unwrap();
        p
    }

    /// Projeto "Sites" com instrucoes e uma conversa nele. Devolve o id da conversa.
    fn conversa_em_projeto(d: &Path) -> i64 {
        let a = |v: Value| b2::acao(d, &d.join("skills"), &v, &[], 10.0, "18/11/2026").unwrap()["novo"].as_i64().unwrap();
        let p = a(json!({ "acao": "criar_projeto", "nome": "Sites", "instrucoes": "Responda curto." }));
        a(json!({ "acao": "criar_conversa", "projeto": p }))
    }

    #[test]
    fn conv_com_caractere_estranho_e_recusada() {
        assert_eq!((conv_id(&json!(3)), conv_id(&json!("12"))), (Some(3), Some(12)));
        for ruim in [json!("b2:1"), json!("1 "), json!("-1"), json!("1a"), json!("../1"), json!("1;2"), json!("١"), json!(""), json!("9999999999999999"),
                     json!(0), json!(-4), json!(1.5), json!(null), json!([1]), json!(true)] {
            assert_eq!(conv_id(&ruim), None, "{ruim}");
        }
    }

    #[test]
    fn chat_leva_as_instrucoes_da_titulo_e_recusa_sem_chamar_a_ia() {
        let d = dir("chat");
        let c = conversa_em_projeto(&d);
        let visto = RefCell::new(Vec::<(String, String)>::new());
        let conversar = |conv: &str, fala: &str| {
            visto.borrow_mut().push((conv.to_string(), fala.to_string()));
            json!({ "resposta": "ok", "provedor": "x", "agentes": [] })
        };
        let r = chat(&d, &json!({ "conv": c, "texto": "  Arrumar o site  " }), 11.0, &conversar).unwrap();
        assert!(r["resposta"] == "ok" && r["conv"] == c);
        assert_eq!(visto.borrow()[0], (format!("b2:{c}"), "Arrumar o site\n\n[Projeto: Sites. Instrucoes do projeto: Responda curto.]".to_string()));
        let titulo = || b2::estado(&d, &[], 12.0).unwrap()["conversas"][0]["titulo"].as_str().unwrap().to_string();
        assert_eq!(titulo(), "Arrumar o site");
        // o id pode vir como texto; a segunda mensagem nao muda o titulo; comando segue sem o contexto
        chat(&d, &json!({ "conv": c.to_string(), "texto": "/status" }), 12.0, &conversar).unwrap();
        assert!(visto.borrow()[1].1 == "/status" && titulo() == "Arrumar o site");
        // recusas: nada chega ao `conversar`
        for (ruim, codigo) in [(json!({ "conv": "b2:1", "texto": "oi" }), 400), (json!({ "conv": format!("{c} OR 1=1"), "texto": "oi" }), 400), (json!({ "texto": "oi" }), 400),
                               (json!({ "conv": c, "texto": "   " }), 400), (json!({ "conv": c }), 400), (json!({ "conv": 999, "texto": "oi" }), 404)] {
            assert_eq!(chat(&d, &ruim, 13.0, &conversar).unwrap_err().1, codigo, "{ruim}");
        }
        assert_eq!(visto.borrow().len(), 2);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn conversa_b2_nao_se_mistura_com_a_principal_e_a_ia_recebe_as_instrucoes() {
        let d = dir("separada");
        let c = conversa_em_projeto(&d);
        let recebido = Arc::new(Mutex::new(String::new()));
        let (r1, r2) = (recebido.clone(), recebido.clone());
        let rot = Arc::new(Roteador {
            cfg: json!({ "system": "S", "cadeia": [{ "nome": "b", "tipo": "online", "api": "openai", "chave_env": "TEM" }] }),
            store: crate::store::Store::em_memoria().unwrap(),
            banco: crate::banco::Banco::em_memoria().unwrap(),
            chamar: Box::new(move |_, msgs, _, _| { r1.lock().unwrap().push_str(&msgs.to_string()); Ok("x".into()) }),
            chamar_tools: Box::new(move |_, msgs, _, _, _| { r2.lock().unwrap().push_str(&msgs.to_string()); Ok(json!({ "content": "x" })) }),
            contexto: Box::new(|_, _| String::new()),
            vigia: Box::new(|_, _| {}),
            ler_chave: Box::new(|_| "k".into()),
        });
        let (agora, ts) = (Agora { data: Data::civil(2026, 11, 18).unwrap(), hora: 10, min: 0, seg: 0 }, fatos::agora_ts());
        let real = |conv: &str, fala: &str| crate::conversar(&d, &rot, conv, fala, true, &agora, ts);
        let r = chat(&d, &json!({ "conv": c, "texto": "oi, tudo bem com você?" }), ts, &real).unwrap();
        assert!(r["provedor"] == "b", "{r}");
        assert!(recebido.lock().unwrap().contains("Instrucoes do projeto: Responda curto."), "a IA tem de receber as instrucoes do projeto");
        let (h, principal) = (rot.store.historico(&format!("b2:{c}"), 99), || rot.store.historico(crate::jarvis::CONV_UNICA, 99));
        assert!(h.len() == 2 && principal().is_empty(), "{h:?}");
        // a tela ve a fala sem o contexto do projeto
        let m = b2::mensagens(&rot.store, c, 60);
        assert!(m[0]["role"] == "user" && m[0]["content"] == "oi, tudo bem com você?" && m[1]["role"] == "assistant", "{m:?}");
        // a conversa principal nao entra na de projeto, nem a de outro projeto
        real("jarvis", "oi, tudo bem com você?");
        assert!(principal().len() == 2 && rot.store.historico(&format!("b2:{c}"), 99).len() == 2 && b2::mensagens(&rot.store, c + 1, 60).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
}
