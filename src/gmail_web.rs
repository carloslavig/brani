//! Gmail SO LEITURA pela aba logada do navegador (sem OAuth): le remetente e assunto da caixa de entrada; nunca abre e-mail, nunca responde,
//! nunca clica. Ficou de fora: a aba. `js(expressao)` e a closure que avalia na pagina ja aberta em URL e `esperar()` da 1,5 s entre as tentativas.
//! Pedido de login vira erro (login e sempre do dono); o JS so LE o que esta na tela.
#![allow(dead_code)]
use serde_json::Value;

pub const URL: &str = "https://mail.google.com/mail/u/0/#inbox";
pub const TENTATIVAS: usize = 20;

pub fn js_caixa(n: usize) -> String {
    format!("(()=>{{const l=[...document.querySelectorAll('tr.zA')].slice(0,{n});\nreturn l.map(r=>{{const s=r.querySelector('.yX .yW span[email], .yP, .zF');const a=r.querySelector('.bog, .y6 span');\nconst t=r.querySelector('[data-legacy-thread-id]');\nreturn {{id:t?t.getAttribute('data-legacy-thread-id'):'', email:s?(s.getAttribute('email')||''):'', nome:s?(s.getAttribute('name')||s.textContent||''):'',\nassunto:a?a.textContent:'', nao_lida:r.classList.contains('zE')}}}})}})()")
}

/// [{id, email, nome, assunto, nao_lida}] da caixa de entrada (so linhas com id). Erro se o Gmail pedir login ou nao carregar.
pub fn caixa(n: usize, js: &dyn Fn(&str) -> Result<Value, String>, esperar: &dyn Fn()) -> Result<Vec<Value>, String> {
    for _ in 0..TENTATIVAS {
        if js("location.href")?.as_str().unwrap_or("").contains("accounts.google.com") {
            return Err("Gmail pediu login (sessão do Google caiu no Chrome do Braniac)".into());
        }
        let linhas = js(&js_caixa(n))?;
        if let Some(l) = linhas.as_array().filter(|l| !l.is_empty()) {
            return Ok(l.iter().filter(|x| x["id"].as_str().is_some_and(|i| !i.is_empty())).cloned().collect());
        }
        esperar();
    }
    Err("caixa de entrada do Gmail não carregou".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::RefCell;

    #[test]
    fn le_a_caixa_espera_carregar_e_login_vira_erro() {
        let (voltas, esperas) = (RefCell::new(0), RefCell::new(0));
        let js = |e: &str| -> Result<Value, String> {
            if e == "location.href" {
                return Ok(json!(URL));
            }
            assert!(e.contains(".slice(0,30)") && !e.contains(".click(") && !e.contains("fetch("));
            *voltas.borrow_mut() += 1;
            Ok(if *voltas.borrow() < 3 { json!([]) } else { json!([{"id": "t1", "nome": "Ana", "assunto": "Oi", "nao_lida": true}, {"id": "", "nome": "sem id"}]) })
        };
        let c = caixa(30, &js, &|| *esperas.borrow_mut() += 1).unwrap();
        assert_eq!((c.len(), c[0]["nome"].as_str(), *esperas.borrow()), (1, Some("Ana"), 2));
        assert!(caixa(30, &|_| Ok(json!("https://accounts.google.com/signin")), &|| {}).unwrap_err().contains("pediu login"));
        let n = RefCell::new(0);
        assert!(caixa(5, &|e| Ok(if e == "location.href" { json!(URL) } else { Value::Null }), &|| *n.borrow_mut() += 1).unwrap_err().contains("não carregou"));
        assert_eq!(*n.borrow(), TENTATIVAS);
        assert_eq!(caixa(5, &|_| Err("aba fechou".into()), &|| {}), Err("aba fechou".into()));
    }
}
