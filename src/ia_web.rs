//! IAs de nuvem pelas abas logadas do navegador (ChatGPT, Gemini, Grok, DeepSeek, Kimi, Meta): digita, envia, espera a resposta
//! estabilizar e le o texto. Travas: UMA pergunta por vez (`VEZ`); nunca resolve captcha nem login (vira erro e quem chama pula para o proximo
//! provedor); o que ja estava na tela e o eco da propria pergunta nunca valem como resposta; e NADA sensivel sai: `barrado` recusa segredo
//! (senha, token, CPF, cartao, Pix...) e tudo que a closure `sensivel` de quem chama (ex.: `Roteador::sensivel`) apontar, como conteudo de
//! WhatsApp e dado pessoal de terceiros. Essa recusa no Python ficava em quem chamava (jarvis/roteador); aqui vale tambem dentro do modulo.
//! Ficou de fora: o navegador (closures em `Aba`: avaliar JS, navegar, trazer para a frente, digitar, Enter, esperar, relogio, devolver o foco).
//! Tetos diarios e descanso por limite de uso sao do roteador (`roteador.rs`), nao deste modulo.
#![allow(dead_code)]
use crate::empresa_dev::{corta, segredo};
use serde_json::Value;
use std::sync::Mutex;

/// So uma aba fica na frente por vez.
static VEZ: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Copy)]
pub struct Site {
    pub nome: &'static str,
    pub dominio: &'static str,
    /// URL de conversa nova.
    pub nova: &'static str,
    /// Seletor da caixa de mensagem.
    pub entrada: &'static str,
    /// Seletor do botao; vazio = tecla Enter.
    pub enviar: &'static str,
    /// Seletor das respostas (a ultima vale).
    pub resposta: &'static str,
    /// JS verdadeiro enquanto ainda esta respondendo.
    pub parar: &'static str,
    pub deslogado: &'static str,
}

const BOTAO_PARAR: &str = "[...document.querySelectorAll('button')].some(b=>/parar|stop/i.test(b.getAttribute('aria-label')||''))";

pub const SITES: [Site; 6] = [
    Site { nome: "chatgpt", dominio: "chatgpt.com", nova: "https://chatgpt.com/", entrada: "#prompt-textarea, div.ProseMirror[contenteditable=true]", enviar: "[data-testid=\"send-button\"], button[aria-label*=\"Enviar\"], button[aria-label=\"Send\"], button[aria-label*=\"Send prompt\"]",
           resposta: "[data-message-author-role=\"assistant\"], [data-markdown-text-style=\"assistant-message\"]", parar: "!!document.querySelector('[data-testid=\"stop-button\"]')", deslogado: "!!document.querySelector('[data-testid=\"login-button\"]')" },
    Site { nome: "gemini", dominio: "gemini.google.com", nova: "https://gemini.google.com/app", entrada: "rich-textarea .ql-editor", enviar: "button[aria-label=\"Enviar mensagem\"]", resposta: "model-response .model-response-text, model-response message-content",
           parar: "[...document.querySelectorAll('button')].some(b=>/parar|stop|interromper/i.test(b.getAttribute('aria-label')||''))", deslogado: "[...document.querySelectorAll('a,button')].some(e=>/fazer login|sign in/i.test(e.innerText||''))" },
    Site { nome: "grok", dominio: "grok.com", nova: "https://grok.com/", entrada: ".ProseMirror", enviar: "[data-testid=\"chat-submit\"]", resposta: ".response-content-markdown", parar: BOTAO_PARAR,
           deslogado: "[...document.querySelectorAll('a,button')].some(e=>/^(sign in|log in|entrar)$/i.test((e.innerText||'').trim()))" },
    // Enter nao envia la: vai pelo botao redondo
    Site { nome: "deepseek", dominio: "chat.deepseek.com", nova: "https://chat.deepseek.com/", entrada: "textarea", enviar: ".ds-button--primary.ds-button--filled.ds-button--circle", resposta: ".ds-markdown",
           parar: "[...document.querySelectorAll('[role=button],button')].some(b=>/parar|stop/i.test(b.getAttribute('aria-label')||''))", deslogado: "!!document.querySelector('input[type=password]') || /sign_in/.test(location.pathname)" },
    Site { nome: "kimi", dominio: "kimi.com", nova: "https://www.kimi.com/", entrada: ".chat-input-editor", enviar: ".send-button-container", resposta: ".segment-content-box, .markdown-container, .markdown",
           parar: "!!document.querySelector('.send-button-container.stop, [class*=stop]')", deslogado: "!!document.querySelector('.login-button-text')" },
    // uso principal = imagens; o seletor da resposta de texto nao foi conferido logado
    Site { nome: "meta", dominio: "meta.ai", nova: "https://www.meta.ai/", entrada: "textarea[placeholder^='Pergunte'], input[placeholder^='Pergunte'], input[placeholder^='Descreva']", enviar: "button[aria-label=\"Enviar\"]",
           resposta: "[data-testid*=message], [class*=markdown], [class*=response]", parar: BOTAO_PARAR, deslogado: "false" },
];

pub fn site(nome: &str) -> Option<&'static Site> {
    SITES.iter().find(|s| s.nome == nome)
}

fn q(seletor: &str) -> String {
    Value::from(seletor).to_string()
}

pub fn estado_js(s: &Site) -> String {
    format!("JSON.stringify({{caixa: !!document.querySelector({}), parar: {}, deslogado: {}, captcha: /verify you are human|confirme que voc|captcha/i.test(document.body.innerText.slice(0,3000)), n: document.querySelectorAll({}).length}})",
        q(s.entrada), s.parar, s.deslogado, q(s.resposta))
}

pub fn ultima_js(s: &Site) -> String {
    format!("(function(){{var m=document.querySelectorAll({});return m.length? m[m.length-1].innerText : ''}})()", q(s.resposta))
}

pub const IMGS_JS: &str = "JSON.stringify([...document.querySelectorAll('img')].filter(i=>i.naturalWidth>=512).map(i=>i.currentSrc||i.src))";

pub struct Aba<'a> {
    /// Avalia na pagina e devolve o valor.
    pub js: &'a dyn Fn(&str) -> Result<Value, String>,
    pub navegar: &'a dyn Fn(&str) -> Result<(), String>,
    /// Traz a aba para a frente (aba escondida e "congelada": a resposta para de aparecer). Falha aqui nao derruba a pergunta.
    pub frente: &'a dyn Fn(),
    /// Insere o texto na caixa em foco.
    pub digitar: &'a dyn Fn(&str) -> Result<(), String>,
    pub enter: &'a dyn Fn() -> Result<(), String>,
    /// Espera tantos segundos.
    pub esperar: &'a dyn Fn(f64),
    /// Segundos (qualquer origem), so para medir tempo decorrido.
    pub relogio: &'a dyn Fn() -> f64,
    /// Ao terminar (com erro ou nao), a aba que estava na frente volta.
    pub devolver_foco: &'a dyn Fn(),
}

/// Some(motivo) se o texto NAO pode ir a uma IA online.
pub fn barrado(texto: &str, sensivel: &dyn Fn(&str) -> bool) -> Option<&'static str> {
    if segredo(texto) {
        Some("pergunta barrada: tem segredo ou dado de acesso (senha, token, CPF, cartão, Pix) e isso nunca vai a IA online")
    } else if sensivel(texto) {
        Some("pergunta barrada: tem dado pessoal ou conteúdo de conversa, que não sai deste PC")
    } else {
        None
    }
}

#[derive(Clone, Copy)]
pub struct Opcoes<'a> {
    pub timeout_s: f64,
    pub nova_conversa: bool,
    /// Abre a conversa dentro deste endereco (o projeto das reunioes do conselho) em vez da conversa nova do site.
    pub projeto_url: Option<&'a str>,
}

impl Default for Opcoes<'_> {
    fn default() -> Self {
        Opcoes { timeout_s: 150.0, nova_conversa: true, projeto_url: None }
    }
}

fn estado(s: &Site, a: &Aba) -> Result<Value, String> {
    let v = (a.js)(&estado_js(s))?;
    serde_json::from_str(v.as_str().unwrap_or("")).map_err(|_| format!("{}: a pagina nao respondeu o estado", s.nome))
}

fn ultima(s: &Site, a: &Aba) -> Result<String, String> {
    Ok((a.js)(&ultima_js(s))?.as_str().unwrap_or("").to_string())
}

fn eco(t: &str) -> String {
    corta(&t.split_whitespace().collect::<Vec<_>>().join(" "), 80)
}

fn clicar_enviar(s: &Site, a: &Aba) -> Result<(), String> {
    if s.enviar.is_empty() {
        return (a.enter)();
    }
    (a.js)(&format!("(function(){{var b=document.querySelector({}); if(b) b.click()}})()", q(s.enviar))).map(|_| ())
}

fn perguntar_na_aba(s: &Site, texto: &str, o: &Opcoes, a: &Aba) -> Result<String, String> {
    let captcha = || Err::<String, String>("captcha/verificacao: precisa do dono".into());
    (a.frente)();
    (a.esperar)(0.8);
    if o.nova_conversa {
        (a.navegar)(o.projeto_url.unwrap_or(s.nova))?;
        (a.esperar)(4.0);
        for _ in 0..10 {
            // espera a conversa NOVA carregar (sem respostas antigas na tela) antes de digitar
            let e = estado(s, a)?;
            if e["caixa"] == true && e["n"] == 0 {
                break;
            }
            (a.esperar)(1.0);
        }
    }
    let e = estado(s, a)?;
    if e["captcha"] == true {
        return captcha();
    }
    if e["deslogado"] == true || e["caixa"] != true {
        return Err(format!("{}: deslogado ou caixa de mensagem indisponivel", s.nome));
    }
    let n0 = e["n"].as_i64().unwrap_or(0);
    let texto_antes = ultima(s, a)?; // o que ja estava na tela: nunca vale como resposta
    (a.js)(&format!("document.querySelector({}).focus()", q(s.entrada)))?;
    (a.digitar)(texto)?;
    (a.esperar)(0.8);
    clicar_enviar(s, a)?;
    let fim = (a.relogio)() + o.timeout_s;
    let (mut anterior, mut estavel_desde) = (String::new(), None::<f64>);
    while (a.relogio)() < fim {
        (a.esperar)(1.5);
        (a.frente)();
        let e = estado(s, a)?;
        if e["captcha"] == true {
            return captcha();
        }
        if e["n"].as_i64().unwrap_or(0) <= n0 {
            continue;
        }
        let atual = ultima(s, a)?;
        // em alguns sites a bolha da PERGUNTA usa o mesmo seletor: isso e eco, ainda nao e a resposta
        if (!texto_antes.is_empty() && atual == texto_antes) || eco(&atual) == eco(texto) {
            continue;
        }
        if e["parar"] == true || atual != anterior {
            anterior = atual;
            estavel_desde = None;
            continue;
        }
        let desde = *estavel_desde.get_or_insert((a.relogio)());
        if (a.relogio)() - desde >= 8.0 && !atual.trim().is_empty() {
            return Ok(atual.trim().to_string());
        }
    }
    Err(format!("{}: tempo esgotado esperando a resposta", s.nome))
}

fn na_vez<T>(a: &Aba, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let _vez = VEZ.lock().unwrap_or_else(|e| e.into_inner());
    let r = f();
    (a.devolver_foco)();
    r
}

/// Envia `texto` ao site e devolve a resposta. Erro se algo impedir (texto barrado, site desconhecido, captcha, login, tempo).
pub fn perguntar(nome_site: &str, texto: &str, o: &Opcoes, sensivel: &dyn Fn(&str) -> bool, a: &Aba) -> Result<String, String> {
    let s = site(nome_site).ok_or_else(|| format!("IA desconhecida: {nome_site}"))?;
    if texto.trim().is_empty() {
        return Err("pergunta vazia".into());
    }
    if let Some(motivo) = barrado(texto, sensivel) {
        return Err(motivo.into());
    }
    na_vez(a, || perguntar_na_aba(s, texto, o, a))
}

fn imagens(a: &Aba) -> Result<Vec<String>, String> {
    let v = (a.js)(IMGS_JS)?;
    Ok(serde_json::from_str::<Vec<String>>(v.as_str().unwrap_or("[]")).unwrap_or_default())
}

/// Pede uma imagem ao Meta AI e devolve os enderecos (ate 4) das imagens novas, depois de 6 s sem mudar. So prompt de imagem: nada pessoal.
/// Baixar e salvar fica com quem chama.
pub fn gerar_imagem(prompt: &str, timeout_s: f64, sensivel: &dyn Fn(&str) -> bool, a: &Aba) -> Result<Vec<String>, String> {
    let s = site("meta").ok_or("meta: site nao cadastrado")?;
    if let Some(motivo) = barrado(prompt, sensivel) {
        return Err(motivo.into());
    }
    na_vez(a, || {
        (a.frente)();
        (a.navegar)(s.nova)?;
        (a.esperar)(5.0);
        if (a.js)("!!document.querySelector('input[type=password]')")? == true {
            return Err("meta: precisa de login (o dono entra na aba do Meta AI)".into());
        }
        if (a.js)(&format!("!!document.querySelector({})", q(s.entrada)))? != true {
            return Err("meta: caixa de mensagem indisponivel".into());
        }
        let antes = imagens(a)?;
        (a.js)(&format!("document.querySelector({}).focus()", q(s.entrada)))?;
        (a.digitar)(&format!("Gere uma imagem realista: {prompt}"))?;
        (a.esperar)(0.8);
        clicar_enviar(s, a)?;
        let fim = (a.relogio)() + timeout_s;
        let (mut ult, mut desde): (Vec<String>, Option<f64>) = (vec![], None);
        while (a.relogio)() < fim {
            (a.esperar)(2.0);
            let novas: Vec<String> = imagens(a)?.into_iter().filter(|u| !antes.contains(u)).collect();
            if !novas.is_empty() && novas == ult {
                let d = *desde.get_or_insert((a.relogio)());
                if (a.relogio)() - d >= 6.0 {
                    return Ok(ult.into_iter().take(4).collect());
                }
            } else {
                ult = novas;
                desde = None;
            }
        }
        Err("meta: nao apareceu imagem (login/limite?)".into())
    })
}

#[cfg(test)]
pub(crate) mod falso {
    use super::*;
    use std::cell::RefCell;

    /// Pagina de mentira: `roteiro(expressao, segundos)` responde a cada JS; o relogio so anda quando alguem espera.
    pub struct Pagina<F: Fn(&str, f64) -> Result<Value, String>> {
        pub roteiro: F,
        pub t: RefCell<f64>,
        pub feito: RefCell<Vec<String>>,
    }

    impl<F: Fn(&str, f64) -> Result<Value, String>> Pagina<F> {
        pub fn nova(roteiro: F) -> Self {
            Pagina { roteiro, t: RefCell::new(0.0), feito: RefCell::new(vec![]) }
        }

        pub fn usar<T>(&self, f: impl FnOnce(&Aba) -> T) -> T {
            let nota = |x: String| self.feito.borrow_mut().push(x);
            f(&Aba {
                js: &|e| (self.roteiro)(e, *self.t.borrow()),
                navegar: &|u| {
                    nota(format!("navegar {u}"));
                    Ok(())
                },
                frente: &|| {},
                digitar: &|t| {
                    nota(format!("digitar {t}"));
                    Ok(())
                },
                enter: &|| {
                    nota("enter".into());
                    Ok(())
                },
                esperar: &|s| *self.t.borrow_mut() += s,
                relogio: &|| *self.t.borrow(),
                devolver_foco: &|| nota("foco".into()),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::falso::Pagina;
    use super::*;
    use serde_json::json;

    fn livre(_: &str) -> bool {
        false
    }

    fn est(n: i64, parar: bool, extra: Value) -> Result<Value, String> {
        let mut e = json!({"caixa": true, "parar": parar, "deslogado": false, "captcha": false, "n": n});
        for (k, v) in extra.as_object().into_iter().flatten() {
            e[k] = v.clone();
        }
        Ok(json!(e.to_string()))
    }

    #[test]
    fn pergunta_espera_estabilizar_e_ignora_eco() {
        // a resposta comeca aos 10 s, cresce ate os 20 s e para; antes disso a bolha nova e so o eco da pergunta
        let p = Pagina::nova(|e, t| {
            if e.starts_with("JSON.stringify({caixa") {
                return est(if t < 7.0 { 0 } else { 1 }, (10.0..20.0).contains(&t), json!({}));
            }
            Ok(json!(if !e.contains("innerText") { "" } else if t < 7.0 { "" } else if t < 10.0 { "Qual  a capital?" } else if t < 20.0 { "Can" } else { " Canberra \n" }))
        });
        let r = p.usar(|a| perguntar("grok", "Qual a capital?", &Opcoes::default(), &livre, a));
        assert_eq!(r, Ok("Canberra".to_string()));
        assert!(*p.t.borrow() >= 28.0 && *p.t.borrow() < 35.0, "{}", p.t.borrow());
        assert_eq!(*p.feito.borrow(), vec!["navegar https://grok.com/", "digitar Qual a capital?", "foco"]);
    }

    #[test]
    fn segredo_e_dado_sensivel_nunca_vao_para_a_nuvem() {
        let p = Pagina::nova(|_, _| panic!("nao pode ir para a IA online"));
        for (texto, sens) in [("minha senha do banco é 1234, o que faço?", false), ("meu token de acesso expirou", false), ("resume essa conversa do contato", true)] {
            let r = p.usar(|a| perguntar("chatgpt", texto, &Opcoes::default(), &|_| sens, a));
            assert!(r.unwrap_err().contains("barrada"), "{texto}");
            assert!(p.usar(|a| gerar_imagem(texto, 10.0, &|_| sens, a)).unwrap_err().contains("barrada"));
        }
        assert_eq!(p.usar(|a| perguntar("bard", "oi", &Opcoes::default(), &livre, a)), Err("IA desconhecida: bard".into()));
        assert_eq!(p.usar(|a| perguntar("gemini", "  ", &Opcoes::default(), &livre, a)), Err("pergunta vazia".into()));
        assert!(p.feito.borrow().is_empty()); // nem chegou a abrir a aba
    }

    #[test]
    fn captcha_login_e_tempo_viram_erro_e_o_foco_volta() {
        for (extra, erro) in [(json!({"captcha": true}), "captcha"), (json!({"deslogado": true}), "deslogado"), (json!({"caixa": false}), "caixa de mensagem indisponivel")] {
            let p = Pagina::nova(|e, _| if e.starts_with("JSON.stringify({caixa") { est(0, false, extra.clone()) } else { Ok(json!("")) });
            let r = p.usar(|a| perguntar("gemini", "ideia de promoção", &Opcoes::default(), &livre, a));
            assert!(r.unwrap_err().contains(erro), "{erro}");
            assert_eq!(p.feito.borrow().last().map(String::as_str), Some("foco"));
            assert!(!p.feito.borrow().iter().any(|f| f.starts_with("digitar"))); // nada e digitado em pagina com captcha ou login
        }
        // captcha no meio da espera tambem para; resposta que nunca chega = tempo esgotado
        let p = Pagina::nova(|e, t| if e.starts_with("JSON.stringify({caixa") { est(0, false, json!({"captcha": t > 20.0})) } else { Ok(json!("")) });
        assert!(p.usar(|a| perguntar("grok", "oi tudo bem", &Opcoes::default(), &livre, a)).unwrap_err().contains("captcha"));
        let p = Pagina::nova(|e, _| if e.starts_with("JSON.stringify({caixa") { est(0, false, json!({})) } else { Ok(json!("")) });
        let o = Opcoes { timeout_s: 30.0, nova_conversa: false, projeto_url: None };
        assert_eq!(p.usar(|a| perguntar("grok", "oi tudo bem", &o, &livre, a)), Err("grok: tempo esgotado esperando a resposta".into()));
        assert!(!p.feito.borrow().iter().any(|f| f.starts_with("navegar")));
    }

    #[test]
    fn texto_antigo_na_tela_nao_vale_e_reuniao_abre_no_projeto() {
        // ja havia uma resposta antiga; a nova so aparece aos 15 s
        let p = Pagina::nova(|e, t| {
            if e.starts_with("JSON.stringify({caixa") {
                return est(if t < 15.0 { 1 } else { 2 }, false, json!({}));
            }
            Ok(json!(if !e.contains("innerText") { "" } else if t < 15.0 { "resposta velha" } else { "resposta nova" }))
        });
        let o = Opcoes { projeto_url: Some("https://chatgpt.com/g/projeto-de-teste/project"), ..Opcoes::default() };
        assert_eq!(p.usar(|a| perguntar("chatgpt", "pauta da reunião", &o, &livre, a)), Ok("resposta nova".to_string()));
        assert_eq!(p.feito.borrow()[0], "navegar https://chatgpt.com/g/projeto-de-teste/project");
        assert!(estado_js(site("deepseek").unwrap()).contains("document.querySelector(\"textarea\")") && ultima_js(site("kimi").unwrap()).contains("m[m.length-1].innerText"));
        assert!(site("meta").is_some() && site("claude").is_none());
    }

    #[test]
    fn imagem_do_meta_so_as_novas_e_estaveis() {
        let p = Pagina::nova(|e, t| {
            Ok(if e == IMGS_JS {
                json!(if t < 12.0 { json!(["velha"]) } else if t < 16.0 { json!(["velha", "n1"]) } else { json!(["velha", "n1", "n2", "n3", "n4", "n5"]) }.to_string())
            } else {
                json!(!e.contains("password"))
            })
        });
        let r = p.usar(|a| gerar_imagem("um gato de chapéu", 180.0, &livre, a)).unwrap();
        assert_eq!(r, vec!["n1", "n2", "n3", "n4"]);
        assert!(p.feito.borrow().contains(&"digitar Gere uma imagem realista: um gato de chapéu".to_string()));
        let login = Pagina::nova(|_, _| Ok(json!(true)));
        assert!(login.usar(|a| gerar_imagem("gato", 10.0, &livre, a)).unwrap_err().contains("precisa de login"));
        let nada = Pagina::nova(|e, _| Ok(if e == IMGS_JS { json!("[]") } else { json!(!e.contains("password")) }));
        assert!(nada.usar(|a| gerar_imagem("gato", 20.0, &livre, a)).unwrap_err().contains("nao apareceu imagem"));
    }
}
