//! TELA VIVA: mostra no escritorio a aba do navegador do Braniac e repassa clique/tecla do dono.
//! Travas: so dominios da lista; nunca digita em campo de senha; o WhatsApp fica SOMENTE LEITURA enquanto o Braniac esta mexendo nele;
//! so as teclas da lista; texto ate 500 letras.
//! Ficou de fora: a conexao com o navegador (so 127.0.0.1). `cmd(metodo, parametros)` e a closure que manda o comando a aba ja escolhida
//! por `aba_de`; `agente_agindo` e quem chama que sabe (a trava do WhatsApp esta ocupada).
#![allow(dead_code)]
use crate::empresa_dev::corta;
use serde_json::{json, Value};

pub const DOMINIOS: [&str; 4] = ["web.whatsapp.com", "airbnb.com.br", "chatgpt.com", "gemini.google.com"];
pub const TECLAS: [(&str, u32); 8] = [("Enter", 13), ("Backspace", 8), ("Tab", 9), ("Escape", 27), ("ArrowUp", 38), ("ArrowDown", 40), ("ArrowLeft", 37), ("ArrowRight", 39)];

pub type Cmd<'a> = &'a dyn Fn(&str, &Value) -> Result<Value, String>;

fn permitido(dominio: &str) -> Result<(), String> {
    if DOMINIOS.contains(&dominio) { Ok(()) } else { Err("dominio nao permitido".into()) }
}

/// A aba daquele dominio na lista de abas do navegador (GET /json).
pub fn aba_de<'a>(dominio: &str, abas: &'a [Value]) -> Result<&'a Value, String> {
    permitido(dominio)?;
    abas.iter().find(|a| a["type"] == "page" && a["url"].as_str().is_some_and(|u| u.contains(dominio))).ok_or_else(|| "aba nao esta aberta".to_string())
}

/// {img: jpeg em base64, w, h}. Nao traz a aba para a frente.
pub fn captura(dominio: &str, cmd: Cmd) -> Result<Value, String> {
    permitido(dominio)?;
    let v = cmd("Page.getLayoutMetrics", &json!({}))?["cssVisualViewport"].clone();
    let img = cmd("Page.captureScreenshot", &json!({"format": "jpeg", "quality": 55}))?["data"].clone();
    Ok(json!({"img": img, "w": v["clientWidth"], "h": v["clientHeight"]}))
}

#[derive(Debug, Clone, PartialEq)]
pub enum Toque<'a> {
    /// Em pontos css da pagina.
    Clique(f64, f64),
    Texto(&'a str),
    Tecla(&'a str),
}

/// Repassa um toque do dono para a aba.
pub fn acao(dominio: &str, toque: &Toque, agente_agindo: bool, cmd: Cmd) -> Result<&'static str, String> {
    permitido(dominio)?;
    if dominio == "web.whatsapp.com" && agente_agindo {
        return Err("o Jarvis esta mexendo no WhatsApp agora; tente em instantes".into());
    }
    if let Toque::Clique(x, y) = toque {
        for t in ["mousePressed", "mouseReleased"] {
            cmd("Input.dispatchMouseEvent", &json!({"type": t, "x": x, "y": y, "button": "left", "clickCount": 1}))?;
        }
        return Ok("ok");
    }
    let senha = cmd("Runtime.evaluate", &json!({"expression": "!!(document.activeElement&&document.activeElement.type==='password')", "returnByValue": true}))?;
    if senha["result"]["value"] != false {
        // sem resposta clara da pagina tambem nao digita
        return Err("campo de senha: digite direto no Chrome, nao pelo painel".into());
    }
    match toque {
        Toque::Texto(t) => {
            cmd("Input.insertText", &json!({"text": corta(t, 500)}))?;
        }
        Toque::Tecla(k) => {
            let (nome, codigo) = TECLAS.iter().find(|(n, _)| n == k).ok_or("tecla nao permitida")?;
            for t in ["rawKeyDown", "keyUp"] {
                cmd("Input.dispatchKeyEvent", &json!({"type": t, "key": nome, "windowsVirtualKeyCode": codigo}))?;
            }
        }
        Toque::Clique(..) => {}
    }
    Ok("ok")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn pagina<'a>(senha: Value, visto: &'a RefCell<Vec<String>>) -> impl Fn(&str, &Value) -> Result<Value, String> + 'a {
        move |m, p| {
            visto.borrow_mut().push(format!("{m} {}", p["type"].as_str().or(p["text"].as_str()).unwrap_or("")));
            Ok(match m {
                "Runtime.evaluate" => json!({"result": {"value": senha}}),
                "Page.getLayoutMetrics" => json!({"cssVisualViewport": {"clientWidth": 1280, "clientHeight": 720}}),
                "Page.captureScreenshot" => json!({"data": "QUJD"}),
                _ => json!({}),
            })
        }
    }

    #[test]
    fn dominio_fora_da_lista_e_recusado() {
        let nunca = |_: &str, _: &Value| -> Result<Value, String> { panic!("nao pode chegar no navegador") };
        assert_eq!(captura("banco.exemplo.com", &nunca), Err("dominio nao permitido".into()));
        assert!(acao("banco.exemplo.com", &Toque::Clique(1.0, 1.0), false, &nunca).is_err());
        let abas = [json!({"type": "page", "url": "https://www.airbnb.com.br/hosting"}), json!({"type": "service_worker", "url": "https://chatgpt.com/sw.js"})];
        assert_eq!(aba_de("airbnb.com.br", &abas).unwrap()["url"], "https://www.airbnb.com.br/hosting");
        assert_eq!(aba_de("chatgpt.com", &abas), Err("aba nao esta aberta".into()));
        assert!(aba_de("banco.exemplo.com", &abas).is_err());
    }

    #[test]
    fn whatsapp_fica_so_leitura_enquanto_o_braniac_mexe() {
        let nunca = |_: &str, _: &Value| -> Result<Value, String> { panic!("so leitura") };
        for toque in [Toque::Clique(5.0, 5.0), Toque::Texto("oi"), Toque::Tecla("Enter")] {
            assert!(acao("web.whatsapp.com", &toque, true, &nunca).unwrap_err().contains("mexendo"));
        }
        let visto = RefCell::new(vec![]);
        assert_eq!(captura("web.whatsapp.com", &pagina(json!(false), &visto)).unwrap(), json!({"img": "QUJD", "w": 1280, "h": 720})); // olhar pode
        assert_eq!(acao("airbnb.com.br", &Toque::Clique(5.0, 7.0), true, &pagina(json!(false), &visto)), Ok("ok")); // outro site nao depende da trava do WhatsApp
        assert_eq!(visto.borrow()[2..], ["Input.dispatchMouseEvent mousePressed", "Input.dispatchMouseEvent mouseReleased"]);
    }

    #[test]
    fn campo_de_senha_nao_recebe_texto_e_so_teclas_da_lista() {
        for senha in [json!(true), Value::Null] {
            let visto = RefCell::new(vec![]);
            let p = pagina(senha, &visto);
            assert!(acao("airbnb.com.br", &Toque::Texto("x"), false, &p).unwrap_err().contains("senha"));
            assert!(acao("airbnb.com.br", &Toque::Tecla("Enter"), false, &p).unwrap_err().contains("senha"));
            assert!(visto.borrow().iter().all(|v| v.starts_with("Runtime.evaluate")));
        }
        let visto = RefCell::new(vec![]);
        let p = pagina(json!(false), &visto);
        assert_eq!(acao("chatgpt.com", &Toque::Texto(&"a".repeat(900)), false, &p), Ok("ok"));
        assert_eq!(visto.borrow()[1].len(), "Input.insertText ".len() + 500);
        assert_eq!(acao("chatgpt.com", &Toque::Tecla("Enter"), false, &p), Ok("ok"));
        assert_eq!(visto.borrow()[3..], ["Input.dispatchKeyEvent rawKeyDown", "Input.dispatchKeyEvent keyUp"]);
        assert_eq!(acao("chatgpt.com", &Toque::Tecla("F5"), false, &p), Err("tecla nao permitida".into()));
        assert_eq!(acao("chatgpt.com", &Toque::Texto("x"), false, &|_, _| Err("Chrome do Braniac fechado".into())), Err("Chrome do Braniac fechado".into()));
    }
}
