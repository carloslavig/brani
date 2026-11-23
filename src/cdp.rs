//! Controle de UMA aba do navegador da VM pelo protocolo de depuracao do Chrome (CDP, WebSocket).
//! E o que deixa um agente navegar e ler a propria aba. Nada aqui executa script vindo de fora:
//! as unicas expressoes avaliadas sao as fixas deste arquivo.
use serde_json::{json, Value};
use std::net::TcpStream;
use std::time::{Duration, Instant};
use tungstenite::{stream::MaybeTlsStream, Message, WebSocket};

/// Marca os elementos em que da para clicar ou digitar com data-braniac="N" e devolve a lista.
/// O valor digitado nos campos NAO entra no rotulo (senao um segredo ja digitado vazava para a IA).
const JS_ELEMENTOS: &str = r#"(() => {
  const rotulo = e => ((e.tagName === 'INPUT' ? (['submit','button'].includes(e.type) ? e.value : '') : e.innerText)
    || e.getAttribute('aria-label') || e.getAttribute('placeholder') || e.getAttribute('title') || e.getAttribute('name') || '').trim().replace(/\s+/g, ' ').slice(0, 80);
  const visivel = e => { const r = e.getBoundingClientRect(), s = getComputedStyle(e); return r.width > 2 && r.height > 2 && s.visibility !== 'hidden' && s.display !== 'none'; };
  document.querySelectorAll('[data-braniac]').forEach(e => e.removeAttribute('data-braniac'));
  const out = [];
  for (const e of document.querySelectorAll('a[href],button,input:not([type=hidden]),textarea,select,[role=button],[role=link],[role=textbox],[contenteditable=true]')) {
    if (e.disabled || !visivel(e)) continue;
    const n = out.length + 1;
    e.setAttribute('data-braniac', n);
    out.push({ n, tag: e.tagName.toLowerCase(), tipo: (e.getAttribute('type') || e.getAttribute('role') || '').toLowerCase(), rotulo: rotulo(e) });
    if (n >= 80) break;
  }
  return out;
})()"#;

/// Acha o elemento N (o numero e trocado por digitos, nunca por texto de fora), rola ate ele e devolve o centro e o rotulo ATUAL.
const JS_ALVO: &str = r#"(() => {
  const e = document.querySelector('[data-braniac="@N@"]');
  if (!e) return null;
  e.scrollIntoView({ block: 'center' });
  const r = e.getBoundingClientRect();
  const rotulo = ((e.tagName === 'INPUT' ? (['submit','button'].includes(e.type) ? e.value : '') : e.innerText)
    || e.getAttribute('aria-label') || e.getAttribute('placeholder') || e.getAttribute('title') || e.getAttribute('name') || '').trim().replace(/\s+/g, ' ').slice(0, 80);
  if (@SEL@ && e.select) e.select();
  return { x: r.left + r.width / 2, y: r.top + r.height / 2, tag: e.tagName.toLowerCase(), tipo: (e.getAttribute('type') || e.getAttribute('role') || '').toLowerCase(), rotulo, url: location.href };
})()"#;

/// Lista de conversas do WhatsApp Web: so le o que ja esta na tela (nao clica, nao abre conversa, nao marca como lida).
const JS_WHATSAPP: &str = r#"(() => {
  const lista = document.querySelector('#pane-side');
  if (!lista) return { estado: document.querySelector('canvas') ? 'qr' : 'carregando', conversas: [] };
  const out = [];
  lista.querySelectorAll('[role="row"], [role="listitem"]').forEach(r => {
    const t = r.querySelector('span[title]');
    if (!t) return;
    const b = r.querySelector('[aria-label*="não lida" i],[aria-label*="unread" i]');
    const m = b ? (b.getAttribute('aria-label') || '').match(/\d+/) : null;
    out.push({ nome: t.getAttribute('title'), nao_lidas: b ? (m ? parseInt(m[0], 10) : 1) : 0 });
  });
  return { estado: 'logado', conversas: out };
})()"#;

pub struct Aba {
    ws: WebSocket<MaybeTlsStream<TcpStream>>,
    n: u64,
    /// Eventos que chegaram enquanto esperavamos a resposta de um comando (ate 100): `esperar_evento` os encontra depois.
    eventos: Vec<Value>,
}

/// A mensagem `m` e a resposta do comando `id`? Evento (sem id) e resposta de outro comando = None.
pub fn resposta_de(m: &Value, id: u64) -> Option<Result<Value, String>> {
    if m["id"].as_u64() != Some(id) {
        return None;
    }
    Some(match m["error"]["message"].as_str() {
        Some(e) => Err(format!("navegador recusou: {e}")),
        None => Ok(m["result"].clone()),
    })
}

/// Valor devolvido por Runtime.evaluate; excecao da pagina vira erro.
pub fn valor_avaliado(r: &Value) -> Result<Value, String> {
    if !r["exceptionDetails"].is_null() {
        return Err(format!("a pagina deu erro: {}", r["exceptionDetails"]["text"].as_str().unwrap_or("?")));
    }
    Ok(r["result"]["value"].clone())
}

impl Aba {
    /// `base` = "127.0.0.1:9333"; `id` = id da aba (ja validado por quem chama).
    pub fn conectar(base: &str, id: &str) -> Result<Aba, String> {
        let (ws, _) = tungstenite::connect(format!("ws://{base}/devtools/page/{id}")).map_err(|e| format!("nao conectei na aba: {e}"))?;
        if let MaybeTlsStream::Plain(s) = ws.get_ref() {
            // pagina travada nao pode segurar o Braniac para sempre
            let _ = s.set_read_timeout(Some(Duration::from_secs(20)));
        }
        Ok(Aba { ws, n: 0, eventos: vec![] })
    }

    pub fn comando(&mut self, metodo: &str, params: Value) -> Result<Value, String> {
        self.n += 1;
        self.ws.send(Message::Text(json!({ "id": self.n, "method": metodo, "params": params }).to_string())).map_err(|e| format!("aba caiu: {e}"))?;
        loop {
            let m = self.ws.read().map_err(|e| format!("aba nao respondeu: {e}"))?;
            if let Message::Text(t) = m {
                let Ok(v) = serde_json::from_str::<Value>(&t) else { continue };
                if let Some(r) = resposta_de(&v, self.n) {
                    return r;
                }
                if v["method"].is_string() {
                    self.eventos.push(v);
                    if self.eventos.len() > 100 {
                        self.eventos.remove(0);
                    }
                }
            }
        }
    }

    /// Espera um evento do navegador (ex.: "Page.fileChooserOpened") por ate `seg` segundos; devolve os `params`.
    pub fn esperar_evento(&mut self, metodo: &str, seg: u64) -> Result<Value, String> {
        let limite = Instant::now() + Duration::from_secs(seg);
        loop {
            if let Some(i) = self.eventos.iter().position(|e| e["method"] == metodo) {
                return Ok(self.eventos.remove(i)["params"].clone());
            }
            if Instant::now() >= limite {
                return Err(format!("o navegador nao mandou {metodo} a tempo"));
            }
            if let Message::Text(t) = self.ws.read().map_err(|e| format!("aba nao respondeu: {e}"))? {
                if let Ok(v) = serde_json::from_str::<Value>(&t) {
                    if v["method"].is_string() {
                        self.eventos.push(v);
                    }
                }
            }
        }
    }

    fn avaliar(&mut self, expressao: &str) -> Result<Value, String> {
        valor_avaliado(&self.comando("Runtime.evaluate", json!({ "expression": expressao, "returnByValue": true }))?)
    }

    /// Vai ao endereco e espera a pagina terminar de carregar (ate 20 s; passou disso, segue com o que tiver).
    pub fn navegar(&mut self, url: &str) -> Result<(), String> {
        let r = self.comando("Page.navigate", json!({ "url": url }))?;
        if let Some(e) = r["errorText"].as_str().filter(|e| !e.is_empty()) {
            return Err(format!("nao abri {url}: {e}"));
        }
        let limite = Instant::now() + Duration::from_secs(20);
        while Instant::now() < limite {
            std::thread::sleep(Duration::from_millis(300));
            if self.avaliar("document.readyState").is_ok_and(|v| v == "complete") {
                break;
            }
        }
        Ok(())
    }

    /// Lista numerada do que da para clicar ou digitar: [{"n","tag","tipo","rotulo"}].
    pub fn whatsapp(&mut self) -> Result<Value, String> {
        self.avaliar(JS_WHATSAPP)
    }

    pub fn elementos(&mut self) -> Result<Value, String> {
        self.avaliar(JS_ELEMENTOS)
    }

    /// {"x","y","tag","tipo","rotulo","url"} do elemento `n` da ultima lista; `selecionar` marca o texto que ja esta no campo.
    pub fn alvo(&mut self, n: u32, selecionar: bool) -> Result<Value, String> {
        let v = self.avaliar(&JS_ALVO.replace("@N@", &n.to_string()).replace("@SEL@", if selecionar { "true" } else { "false" }))?;
        if v.is_null() {
            return Err(format!("nao achei o elemento {n}: liste os elementos de novo (a pagina mudou)"));
        }
        Ok(v)
    }

    /// Clique de verdade (evento de mouse do navegador) no ponto dado.
    pub fn clicar_em(&mut self, x: f64, y: f64) -> Result<(), String> {
        for tipo in ["mouseMoved", "mousePressed", "mouseReleased"] {
            self.comando("Input.dispatchMouseEvent", json!({ "type": tipo, "x": x, "y": y, "button": "left", "clickCount": 1 }))?;
        }
        Ok(())
    }

    /// Digita no elemento que esta com o foco; `enter` aperta Enter no fim.
    pub fn digitar(&mut self, texto: &str, enter: bool) -> Result<(), String> {
        self.comando("Input.insertText", json!({ "text": texto }))?;
        if enter {
            let tecla = json!({ "key": "Enter", "code": "Enter", "windowsVirtualKeyCode": 13, "nativeVirtualKeyCode": 13 });
            let mut baixo = tecla.clone();
            baixo["type"] = json!("keyDown");
            baixo["text"] = json!("\r");
            self.comando("Input.dispatchKeyEvent", baixo)?;
            let mut cima = tecla;
            cima["type"] = json!("keyUp");
            self.comando("Input.dispatchKeyEvent", cima)?;
        }
        Ok(())
    }

    /// {"titulo","url","texto"}: o texto visivel da pagina, cortado em `max` caracteres.
    pub fn ler(&mut self, max: usize) -> Result<Value, String> {
        let v = self.avaliar("({t: document.title, u: location.href, x: document.body ? document.body.innerText : ''})")?;
        let texto: String = v["x"].as_str().unwrap_or("").chars().take(max).collect();
        Ok(json!({ "titulo": v["t"], "url": v["u"], "texto": texto }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn so_a_resposta_do_meu_comando_conta() {
        assert!(resposta_de(&json!({"method": "Page.loadEventFired", "params": {}}), 1).is_none());
        assert!(resposta_de(&json!({"id": 2, "result": {}}), 1).is_none());
        assert_eq!(resposta_de(&json!({"id": 1, "result": {"a": 1}}), 1), Some(Ok(json!({"a": 1}))));
        assert_eq!(resposta_de(&json!({"id": 1, "error": {"message": "x"}}), 1), Some(Err("navegador recusou: x".into())));
    }

    #[test]
    fn scripts_fixos_nao_leem_valor_de_campo_e_so_recebem_numero() {
        // o valor digitado num campo (pode ser segredo) nunca entra no rotulo
        for js in [JS_ELEMENTOS, JS_ALVO] {
            assert!(js.contains("['submit','button'].includes(e.type) ? e.value : ''"));
        }
        let pronto = JS_ALVO.replace("@N@", &7u32.to_string()).replace("@SEL@", "false");
        assert!(pronto.contains(r#"[data-braniac="7"]"#) && !pronto.contains('@'));
    }

    #[test]
    fn excecao_da_pagina_vira_erro() {
        assert_eq!(valor_avaliado(&json!({"result": {"type": "string", "value": "complete"}})).unwrap(), "complete");
        assert!(valor_avaliado(&json!({"result": {}, "exceptionDetails": {"text": "Uncaught"}})).unwrap_err().contains("Uncaught"));
    }
}
