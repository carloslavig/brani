//! WhatsApp de verdade no Rust: o APARELHO (ler a conversa aberta, abrir pelo nome, digitar e enviar) na aba reservada `whatsapp`
//! da VM. Seletores e logica do JavaScript vieram de local-ia/wpp_local.py, braniac/whatsapp.py e braniac/dono.py (nada inventado).
//! Tudo passa pelo trait `Pagina`: em producao e a aba por CDP (`Real`); nos testes, uma pagina de mentira que anota cada gesto.
//! Travas portadas: nome no topo CONFERIDO antes de digitar (`jarvis_travas::trava_destino`), texto vazio/defeituoso barrado, pai do dono
//! so recebe AUDIO (texto barrado), eco do que NOS enviamos (o chat do dono e ele consigo mesmo: o envio volta como se fosse fala dele).
//! Audio e arquivo (enviar e ouvir) moram em wpp_midia.rs. Ficou de fora: imagem (ver), busca na agenda para contato fora da lista, envio direto pelo Baileys.
use crate::abas;
use crate::alma_cuida::Nomes;
use crate::atender::{self, Msg};
use crate::atender_laco::Conversa;
use crate::cdp::{self, Aba};
use crate::contexto::Ctx;
use crate::{guia_pai, jarvis_travas as jt, tratamento};
use serde_json::{json, Value};
use std::path::Path;

pub const AGENTE: &str = crate::whatsapp::AGENTE;

/// Lista de conversas com o resumo ("Nome | 17:47 | texto") e se a ultima mensagem e NOSSA (tique de entrega). Le o texto folha a folha:
/// com a aba escondida o innerText vem vazio, o textContent nao. Nao clica, nao abre, nao marca como lida.
const JS_LINHAS: &str = r##"(function(){
  var out=[];
  document.querySelectorAll('#pane-side [role="row"], #pane-side [role="listitem"]').forEach(function(r){
    var t=r.querySelector('span[title]'); if(!t) return;
    var badge=r.querySelector('[aria-label*="não lida" i],[aria-label*="unread" i]');
    var n=0; if(badge){var m=(badge.getAttribute('aria-label')||'').match(/\d+/); n=m?parseInt(m[0],10):1;}
    var linhas=[]; r.querySelectorAll('*').forEach(function(e){ if(e.children.length===0 && !e.closest('svg')){ var s=(e.textContent||'').trim(); if(s) linhas.push(s); } });
    var ts=[].slice.call(r.querySelectorAll('svg title')).map(function(x){return x.textContent});
    var nossa=ts.some(function(x){return /^wds-ic-(read|delivered|sent|pending)|phone-call-made/i.test(x)});
    out.push({nome:t.getAttribute('title'), nl:n, nossa:nossa, resumo:linhas.slice(0,4).join(' | ')});
  });
  return JSON.stringify(out);
})()"##;

const JS_RECT: &str = r##"(function(nome){var rows=document.querySelectorAll('#pane-side [role="row"], #pane-side [role="listitem"]');
 for(var r of rows){var t=r.querySelector('span[title]'); if(t&&t.getAttribute('title')===nome){r.scrollIntoView({block:'nearest'}); var b=r.getBoundingClientRect(); return JSON.stringify({x:b.x+b.width/2,y:b.y+b.height/2});}} return null;})(@NOME@)"##;

const JS_ABERTA: &str = r##"[...document.querySelectorAll('header')].some(h=>(h.innerText||h.textContent).indexOf(@CHAVE@)>=0 && h.closest('#pane-side')==null && !h.closest('#side'))"##;

const JS_TITULO: &str = r##"(function(){
  var h = document.querySelector('#main header');
  if (!h) return '';
  var s = h.querySelector('span[dir=auto]');
  return (s && (s.innerText||s.textContent)) || (h.innerText||h.textContent).split('\n')[0] || '';
})()"##;

const JS_CABECALHO: &str = "(function(){var h=document.querySelector('#main header'); return h ? (h.innerText||h.textContent||'') : '';})()";

const JS_CANCELA_GRAVACAO: &str = r##"(function(){var f=document.querySelector('footer'); if(!f||!f.querySelector('[aria-label="Pausar gravação"],[aria-label="Retomar gravação"]')) return false;
  var c=f.querySelector('[aria-label="Cancelar"]'); if(!c) return false; var r=c.getBoundingClientRect(); return JSON.stringify({x:r.x+r.width/2,y:r.y+r.height/2})})()"##;

const JS_FOCO: &str = r##"(function(){var b=document.querySelector('footer [contenteditable="true"], [contenteditable="true"][data-tab="10"]'); if(!b) return false; b.focus(); return true})()"##;

const JS_MSGS: &str = include_str!("wpp_msgs.js");

/// O que o aparelho sabe fazer. Em producao: a aba do WhatsApp na VM.
pub trait Pagina {
    fn js(&mut self, expr: &str) -> Result<Value, String>;
    fn mouse(&mut self, tipo: &str, x: f64, y: f64) -> Result<(), String>;
    fn inserir(&mut self, texto: &str) -> Result<(), String>;
    fn enter(&mut self) -> Result<(), String>;
    fn frente(&mut self) -> Result<(), String>;
    /// Clica em (x, y) esperando o seletor de arquivo do navegador e entrega a ele `caminho` (caminho VISTO PELO NAVEGADOR, o da VM).
    /// Nenhuma janela do Windows abre. Pagina sem essa capacidade: erro.
    fn escolher_arquivo(&mut self, _x: f64, _y: f64, _caminho: &str) -> Result<(), String> {
        Err("esta pagina nao sabe escolher arquivo".into())
    }
    fn dormir(&mut self, ms: u64) {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
}

pub struct Real(pub Aba);

impl Real {
    pub fn conectar(dados: &Path) -> Result<Real, String> {
        abas::aba_aberta(dados, AGENTE).map(Real)
    }
}

impl Pagina for Real {
    fn js(&mut self, expr: &str) -> Result<Value, String> {
        cdp::valor_avaliado(&self.0.comando("Runtime.evaluate", json!({ "expression": expr, "returnByValue": true }))?)
    }
    fn mouse(&mut self, tipo: &str, x: f64, y: f64) -> Result<(), String> {
        let botoes = if tipo == "mousePressed" { 1 } else { 0 };
        self.0.comando("Input.dispatchMouseEvent", json!({ "type": tipo, "x": x, "y": y, "button": "left", "buttons": botoes, "clickCount": 1 })).map(|_| ())
    }
    fn inserir(&mut self, texto: &str) -> Result<(), String> {
        self.0.comando("Input.insertText", json!({ "text": texto })).map(|_| ())
    }
    fn enter(&mut self) -> Result<(), String> {
        for tipo in ["keyDown", "keyUp"] {
            self.0.comando("Input.dispatchKeyEvent", json!({ "type": tipo, "key": "Enter", "code": "Enter", "windowsVirtualKeyCode": 13,
                "nativeVirtualKeyCode": 13, "text": if tipo == "keyDown" { "\r" } else { "" } }))?;
        }
        Ok(())
    }
    fn escolher_arquivo(&mut self, x: f64, y: f64, caminho: &str) -> Result<(), String> {
        self.0.comando("Page.enable", json!({}))?;
        self.0.comando("Page.setInterceptFileChooserDialog", json!({ "enabled": true }))?;
        let r = (|| {
            self.mouse("mouseMoved", x, y)?;
            std::thread::sleep(std::time::Duration::from_millis(200));
            self.mouse("mousePressed", x, y)?;
            self.mouse("mouseReleased", x, y)?;
            let ev = self.0.esperar_evento("Page.fileChooserOpened", 8)?;
            let no = ev["backendNodeId"].as_i64().ok_or("o seletor de arquivo veio sem no")?;
            self.0.comando("DOM.setFileInputFiles", json!({ "files": [caminho], "backendNodeId": no })).map(|_| ())
        })();
        let _ = self.0.comando("Page.setInterceptFileChooserDialog", json!({ "enabled": false }));
        r
    }
    /// Aba escondida: innerText vazio e clique que nao pega. So o modo VIVO chama isto (a leitura da lista nao precisa).
    fn frente(&mut self) -> Result<(), String> {
        self.0.comando("Page.bringToFront", json!({})).map(|_| ())
    }
}

/// Linha da lista: nome, nao lidas, se a ultima e nossa, resumo "Nome | hora | texto".
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Item {
    pub nome: String,
    pub nl: i64,
    pub nossa: bool,
    pub resumo: String,
}

impl Item {
    pub fn linha(&self) -> atender::Linha {
        atender::Linha { nome: self.nome.clone(), resumo: self.resumo.clone(), nossa: self.nossa }
    }
}

pub(crate) fn json_de(v: Value) -> Value {
    v.as_str().and_then(|s| serde_json::from_str(s).ok()).unwrap_or(Value::Null)
}

/// Pura. Lista de conversas a partir do JSON do JS_LINHAS; repetida (mesmo nome) sai.
pub fn itens_de(v: &Value) -> Vec<Item> {
    let mut vistos = std::collections::HashSet::new();
    v.as_array().into_iter().flatten()
        .filter_map(|c| Some(Item { nome: c["nome"].as_str()?.to_string(), nl: c["nl"].as_i64().unwrap_or(0), nossa: c["nossa"] == true,
            resumo: c["resumo"].as_str().unwrap_or("").to_string() }))
        .filter(|i| !i.nome.trim().is_empty() && vistos.insert(i.nome.clone())).collect()
}

/// So le a lista (nao abre conversa, nao marca como lida).
pub fn linhas(p: &mut dyn Pagina) -> Result<Vec<Item>, String> {
    Ok(itens_de(&json_de(p.js(JS_LINHAS)?)))
}

/// Abre a conversa pelo nome EXATO da lista, com clique de verdade, e confere que o topo mostra esse nome. ATENCAO: marca como lida.
pub fn abrir(p: &mut dyn Pagina, nome: &str) -> bool {
    let _ = p.frente();
    let Ok(rc) = p.js(&JS_RECT.replace("@NOME@", &serde_json::to_string(nome).unwrap_or_default())) else { return false };
    let rc = json_de(rc);
    let (Some(x), Some(y)) = (rc["x"].as_f64(), rc["y"].as_f64()) else { return false };
    let _ = p.mouse("mouseMoved", x, y);
    p.dormir(300);
    let _ = p.mouse("mousePressed", x, y);
    let _ = p.mouse("mouseReleased", x, y);
    p.dormir(1500);
    // emoji vira <img> no cabecalho e nao entra no texto: confere pela parte do nome sem emoji
    let chave: String = jt::chave_busca(nome).chars().take(12).collect();
    p.js(&JS_ABERTA.replace("@CHAVE@", &serde_json::to_string(&chave).unwrap_or_default())).is_ok_and(|v| v == true)
}

pub fn titulo_aberto(p: &mut dyn Pagina) -> String {
    p.js(JS_TITULO).ok().and_then(|v| v.as_str().map(|s| s.trim().to_string())).unwrap_or_default()
}

/// Pura. {"msgs": [...], "midias": [...]} do JS_MSGS -> a conversa e quantos audios dele ha entre as marcas. So midia que veio DELE entra;
/// o ultimo item ser um audio dele vira `audio_dele`.
pub fn conversa_de(v: &Value, cabecalho: &str) -> (Conversa, i64) {
    let msgs: Vec<Msg> = v["msgs"].as_array().into_iter().flatten().map(|m| Msg {
        d: m["d"].as_str().unwrap_or("ELE").to_string(), t: m["t"].as_str().unwrap_or("").to_string(), c: m["c"].as_str().unwrap_or("").to_string() }).collect();
    let extras: Vec<&Value> = v["midias"].as_array().into_iter().flatten().filter(|x| x["d"] == "ELE").collect();
    let n_audios = extras.iter().filter(|x| x["t"] == "[AUDIO]").count() as i64;
    let audio_dele = extras.last().is_some_and(|x| x["t"] == "[AUDIO]");
    let midias = extras.iter().filter_map(|x| x["t"].as_str()).filter(|t| *t != "[AUDIO]").map(String::from).collect();
    (Conversa { msgs, midias, cabecalho: cabecalho.to_string(), audio_dele }, n_audios)
}

/// Le as ultimas `max` mensagens da conversa JA ABERTA. `eu` = como o WhatsApp assina as mensagens do dono (pessoal.json "nome_whatsapp").
pub fn ler_conversa(p: &mut dyn Pagina, max: usize, eu: &str) -> Result<(Conversa, i64), String> {
    let js = JS_MSGS.replace("@MAX@", &max.to_string()).replace("@EU@", &serde_json::to_string(eu).unwrap_or_else(|_| "\"\"".into()));
    let v = json_de(p.js(&js)?);
    if v.is_null() {
        return Err("nao consegui ler as mensagens da conversa aberta".into());
    }
    let cab = p.js(JS_CABECALHO).ok().and_then(|c| c.as_str().map(String::from)).unwrap_or_default();
    Ok(conversa_de(&v, &cab))
}

/// Modo SOMBRA: a conversa montada so com a previa da lista (nada foi aberto).
pub fn conversa_da_previa(it: &Item, agora: &crate::tempo::Agora) -> Conversa {
    let partes: Vec<&str> = it.resumo.split(" | ").collect();
    let texto = partes.iter().skip(2).copied().collect::<Vec<_>>().join(" | ");
    let hora = partes.get(1).copied().filter(|h| h.len() <= 5 && h.contains(':')).unwrap_or("");
    let (a, m, d) = agora.data.partes();
    let c = if hora.is_empty() { String::new() } else { format!("[{hora}, {d:02}/{m:02}/{a}] {}: ", it.nome) };
    let msgs = if texto.trim().is_empty() { vec![] } else { vec![Msg { d: if it.nossa { "EU" } else { "ELE" }.into(), t: texto, c }] };
    Conversa { msgs, ..Default::default() }
}

// ---------- eco do que nos mesmos enviamos ----------

const ECO_MIN: usize = 25;

fn chave(t: &str, n: usize) -> String {
    t.to_lowercase().chars().filter(|c| c.is_alphanumeric() || *c == '_').take(n).collect()
}

/// Guarda o texto enviado (ate 1500 letras, os 60 ultimos) para `foi_nosso` reconhecer o eco.
pub fn lembrar_envio(dados: &Path, texto: &str) {
    let arq = dados.join("enviados_recentes.json");
    let mut lista: Vec<String> = std::fs::read_to_string(&arq).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    lista.push(chave(texto, 1500));
    let n = lista.len().saturating_sub(60);
    let _ = std::fs::write(&arq, json!(lista[n..]).to_string());
}

/// O texto e o que NOS enviamos? Comeco igual (60 letras) ou trecho de ECO_MIN+ letras dentro de algo enviado.
pub fn foi_nosso(dados: &Path, texto: &str) -> bool {
    let k = chave(texto, 60);
    let lista: Vec<String> = std::fs::read_to_string(dados.join("enviados_recentes.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    !k.is_empty() && lista.iter().any(|s| s.chars().take(60).collect::<String>() == k || (k.chars().count() >= ECO_MIN && s.contains(&k)))
}

// ---------- envio de texto ----------

fn defeituoso(t: &str) -> bool {
    let t = t.trim().to_lowercase();
    t.is_empty() || ["<none>", "none", "null", "undefined", "n/a"].contains(&t.as_str()) || t.starts_with("{\"tool\"") || t.starts_with("{\"name\"")
        || t.starts_with("não consegui concluir isso agora")
}

/// "+55 11 90000-0000" -> Some("5511900000000"). So digitos/+/espaco/-/() com 10-13 digitos; 10-11 ganham o 55.
pub fn numero_telefone(nome: &str) -> Option<String> {
    let t = nome.trim();
    if t.is_empty() || !t.chars().all(|c| c.is_ascii_digit() || " +-()".contains(c)) {
        return None;
    }
    let d: String = t.chars().filter(|c| c.is_ascii_digit()).collect();
    match d.len() {
        10 | 11 => Some(format!("55{d}")),
        12 | 13 => Some(d),
        _ => None,
    }
}

/// Abre o chat pelo numero (web.whatsapp.com/send) e espera a caixa de texto; popup de numero invalido = Err.
fn abrir_por_numero(p: &mut dyn Pagina, num: &str) -> Result<String, String> {
    p.js(&format!("location.href='https://web.whatsapp.com/send?phone={num}'")).map_err(|e| format!("Não consegui abrir o número {num} ({e}). Nada foi enviado."))?;
    for _ in 0..25 {
        p.dormir(1000);
        // durante o carregamento o js pode falhar: so tenta de novo
        if p.js(JS_FOCO).is_ok_and(|v| v == true) {
            return Ok(num.to_string());
        }
        let invalido = p.js(r#"/n[úu]mero de telefone compartilhado por url|phone number shared via url/i.test(document.body.innerText)"#);
        if invalido.is_ok_and(|v| v == true) {
            return Err(format!("O WhatsApp diz que o número {num} é inválido ou não tem WhatsApp. Nada foi enviado."));
        }
    }
    Err(format!("A conversa com o número {num} não abriu a tempo. Nada foi enviado."))
}

/// Acha a conversa na lista, abre e CONFERE o nome no topo. Ok(nome na lista) = pode digitar/gravar/anexar; Err(aviso) = nada saiu.
/// `pode_pai` = false barra o pai do dono (so recebe audio); o envio de audio chama com true.
pub fn abrir_destino(p: &mut dyn Pagina, ctx: &Ctx, nomes: &Nomes, nome: &str, pode_pai: bool) -> Result<String, String> {
    if let Some(num) = numero_telefone(nome) {
        if !pode_pai && guia_pai::eh_pai(&num, nomes) {
            return Err("⛔ Texto barrado: o pai do dono só recebe áudio.".into());
        }
        return abrir_por_numero(p, &num);
    }
    let lista = linhas(p).map_err(|e| format!("Não consegui ler a lista do WhatsApp ({e}). Nada foi enviado."))?;
    let visiveis: Vec<String> = lista.iter().map(|i| i.nome.clone()).collect();
    let mut alvos = jt::resolver_nome(ctx, nome, &visiveis);
    if alvos.len() > 1 {
        alvos = jt::melhor_candidato(ctx, nome, &alvos).into_iter().collect();
    }
    let Some(alvo) = alvos.into_iter().next() else { return Err(format!("Não achei a conversa '{nome}' na lista do WhatsApp. Nada foi enviado.")) };
    if !pode_pai && guia_pai::eh_pai(&alvo, nomes) {
        return Err("⛔ Texto barrado: o pai do dono só recebe áudio.".into());
    }
    if !abrir(p, &alvo) {
        return Err(format!("Falhou ao abrir a conversa com {alvo}. Nada foi enviado."));
    }
    p.dormir(1000);
    let aberto = titulo_aberto(p);
    if let Some(bloqueio) = jt::trava_destino(ctx, nome, &aberto) {
        return Err(bloqueio);
    }
    Ok(alvo)
}

/// Envia texto a uma conversa que ESTA NA LISTA. Barra: texto vazio/defeituoso, o pai (so audio), nome ambiguo/ausente e, principalmente,
/// a conversa aberta que nao e a pedida (confere o nome no topo ANTES de digitar). Devolve "✅ ..." quando enviou; qualquer outra coisa = nada saiu.
pub fn enviar_texto(p: &mut dyn Pagina, dados: &Path, ctx: &Ctx, nomes: &Nomes, nome: &str, texto: &str) -> String {
    if defeituoso(texto) {
        return "⛔ Mensagem barrada: resposta vazia ou defeituosa.".into();
    }
    if guia_pai::eh_pai(nome, nomes) {
        return "⛔ Texto barrado: o pai do dono só recebe áudio.".into();
    }
    let texto = tratamento::corrigir(texto, &tratamento::regras(dados));
    let alvo = match abrir_destino(p, ctx, nomes, nome, false) {
        Ok(a) => a,
        Err(m) => return m,
    };
    lembrar_envio(dados, &texto);
    if !digitar_e_enviar(p, &texto) {
        return format!("Não achei a caixa de mensagem da conversa com {alvo}. Nada foi enviado.");
    }
    format!("✅ Mensagem enviada para {alvo}.")
}

/// Se o rodape ficou no modo gravacao (gravacao presa), clica em Cancelar: nada e enviado.
pub fn cancelar_gravacao_presa(p: &mut dyn Pagina) {
    if let Ok(v) = p.js(JS_CANCELA_GRAVACAO) {
        let pos = json_de(v);
        if let (Some(x), Some(y)) = (pos["x"].as_f64(), pos["y"].as_f64()) {
            let _ = p.mouse("mousePressed", x, y);
            let _ = p.mouse("mouseReleased", x, y);
            p.dormir(1000);
        }
    }
}

/// Cancela gravacao de audio presa (nada e enviado), foca a caixa, digita e aperta Enter.
fn digitar_e_enviar(p: &mut dyn Pagina, texto: &str) -> bool {
    cancelar_gravacao_presa(p);
    if !p.js(JS_FOCO).is_ok_and(|v| v == true) {
        return false;
    }
    if p.inserir(texto).is_err() {
        return false;
    }
    p.dormir(400);
    let ok = p.enter().is_ok();
    p.dormir(800);
    ok
}

/// O nome com que o WhatsApp assina as mensagens do dono (pessoal.json "nome_whatsapp"); vazio = so a classe da mensagem decide.
pub fn nome_whatsapp(dados: &Path) -> String {
    crate::alma_cuida::ler_json(&dados.join("pessoal.json"))["nome_whatsapp"].as_str().unwrap_or("").trim().to_string()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn detecta_numero_de_telefone() {
        assert_eq!(numero_telefone("+55 11 90000-0000").as_deref(), Some("5511900000000"));
        assert_eq!(numero_telefone("(11) 90000-0000").as_deref(), Some("5511900000000"));
        assert_eq!(numero_telefone("2164993321").as_deref(), Some("552164993321"));
        assert_eq!(numero_telefone("Maria 11 90000-0000"), None);
        assert_eq!(numero_telefone("12345"), None);
    }

    /// Pagina de mentira: anota cada gesto; `titulo` e o que o topo da conversa mostra depois do clique.
    #[derive(Default)]
    pub struct Falsa {
        pub lista: String,
        pub titulo: String,
        pub gestos: Vec<String>,
        pub caixa: bool,
        pub msgs: String,
    }

    impl Pagina for Falsa {
        fn js(&mut self, expr: &str) -> Result<Value, String> {
            if expr.contains("#pane-side [role=\"row\"], #pane-side [role=\"listitem\"]') .forEach") || expr.contains("out.push({nome:t.getAttribute('title'), nl:n") {
                return Ok(json!(self.lista));
            }
            if expr.contains("getBoundingClientRect(); return JSON.stringify({x:b.x") {
                return Ok(json!(r#"{"x":10,"y":20}"#));
            }
            if expr.contains("indexOf(") && expr.starts_with("[...document.querySelectorAll('header')]") {
                return Ok(json!(true));
            }
            if expr.contains("data-pre-plain-text") {
                return Ok(json!(self.msgs));
            }
            if expr.contains("return h ? (h.innerText") {
                return Ok(json!("Ana"));
            }
            if expr.contains("span[dir=auto]") {
                return Ok(json!(self.titulo));
            }
            if expr.contains("Pausar gravação") {
                return Ok(json!(false));
            }
            if expr.contains("contenteditable") {
                return Ok(json!(self.caixa));
            }
            self.gestos.push(format!("js-desconhecido:{}", expr.chars().take(30).collect::<String>()));
            Err("fora do roteiro do teste".into())
        }
        fn mouse(&mut self, tipo: &str, _: f64, _: f64) -> Result<(), String> {
            self.gestos.push(format!("mouse:{tipo}"));
            Ok(())
        }
        fn inserir(&mut self, t: &str) -> Result<(), String> {
            self.gestos.push(format!("inserir:{t}"));
            Ok(())
        }
        fn enter(&mut self) -> Result<(), String> {
            self.gestos.push("enter".into());
            Ok(())
        }
        fn frente(&mut self) -> Result<(), String> {
            self.gestos.push("frente".into());
            Ok(())
        }
        fn dormir(&mut self, _: u64) {}
    }

    pub fn tmp(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-wpp_vivo-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("pessoal.json"), r#"{"nome": "Davi", "contatos": {"davi": "Eu(D)", "mae": "Mainha"}, "de_casa": ["mae"],
            "alma": {"contato_mae": ["mainha"], "contato_pai": ["Pai"]}}"#).unwrap();
        d
    }

    pub fn lista_de(nomes: &[&str]) -> String {
        json!(nomes.iter().map(|n| json!({"nome": n, "nl": 1, "nossa": false, "resumo": format!("{n} | 10:00 | oi")})).collect::<Vec<_>>()).to_string()
    }

    #[test]
    fn nome_errado_no_topo_nao_envia() {
        let d = tmp("destino");
        let (ctx, nomes) = (Ctx::ler(&d), Nomes::ler(&d));
        let mut p = Falsa { lista: lista_de(&["Ana"]), titulo: "Outra Pessoa".into(), caixa: true, ..Default::default() };
        let r = enviar_texto(&mut p, &d, &ctx, &nomes, "Ana", "oi, tudo bem?");
        assert!(r.starts_with("🔒"), "{r}");
        assert!(!p.gestos.iter().any(|g| g.starts_with("inserir") || g == "enter"), "{:?}", p.gestos);
        assert!(!foi_nosso(&d, "oi, tudo bem?")); // nada saiu, nada a lembrar
        // topo certo: envia
        p.titulo = "Ana".into();
        assert!(enviar_texto(&mut p, &d, &ctx, &nomes, "Ana", "oi, tudo bem?").starts_with('✅'));
        assert!(p.gestos.contains(&"enter".to_string()));
    }

    #[test]
    fn titulo_vazio_nao_envia_e_conversa_fora_da_lista_nao_envia() {
        let d = tmp("vazio");
        let (ctx, nomes) = (Ctx::ler(&d), Nomes::ler(&d));
        let mut p = Falsa { lista: lista_de(&["Ana"]), titulo: String::new(), caixa: true, ..Default::default() };
        assert!(enviar_texto(&mut p, &d, &ctx, &nomes, "Ana", "oi pessoal").starts_with("🔒"));
        assert!(enviar_texto(&mut p, &d, &ctx, &nomes, "Zeca", "oi pessoal").contains("Nada foi enviado"));
        assert!(!p.gestos.iter().any(|g| g == "enter"));
    }

    #[test]
    fn eco_do_proprio_envio_e_reconhecido() {
        let d = tmp("eco");
        let (ctx, nomes) = (Ctx::ler(&d), Nomes::ler(&d));
        let mut p = Falsa { lista: lista_de(&["Eu(D)"]), titulo: "Eu(D)".into(), caixa: true, ..Default::default() };
        let t = "Senhor, o resumo do dia ficou pronto e a página do cliente já está no ar.";
        assert!(!foi_nosso(&d, t));
        assert!(enviar_texto(&mut p, &d, &ctx, &nomes, "Eu(D)", t).starts_with('✅'));
        assert!(foi_nosso(&d, t));
        assert!(foi_nosso(&d, "o resumo do dia ficou pronto e a página do cliente")); // trecho de eco
        assert!(!foi_nosso(&d, "ok")); // curto demais para casar no meio
        assert!(!foi_nosso(&d, "outra coisa totalmente diferente que ele escreveu agora"));
    }

    #[test]
    fn pai_so_recebe_audio_texto_nunca() {
        let d = tmp("pai");
        let (ctx, nomes) = (Ctx::ler(&d), Nomes::ler(&d));
        let mut p = Falsa { lista: lista_de(&["Pai"]), titulo: "Pai".into(), caixa: true, ..Default::default() };
        let r = enviar_texto(&mut p, &d, &ctx, &nomes, "Pai", "Oi pai, tudo bem?");
        assert!(r.starts_with("⛔") && !r.starts_with('✅'), "{r}");
        assert!(p.gestos.is_empty(), "{:?}", p.gestos); // nem a lista foi lida
    }

    #[test]
    fn texto_defeituoso_e_barrado() {
        let d = tmp("lixo");
        let (ctx, nomes) = (Ctx::ler(&d), Nomes::ler(&d));
        let mut p = Falsa { lista: lista_de(&["Ana"]), titulo: "Ana".into(), caixa: true, ..Default::default() };
        for t in ["", "  ", "<NONE>", r#"{"tool": "x"}"#] {
            assert!(enviar_texto(&mut p, &d, &ctx, &nomes, "Ana", t).starts_with('⛔'), "{t}");
        }
        assert!(p.gestos.is_empty());
    }

    #[test]
    fn lista_e_conversa_sao_lidas_do_json() {
        let v = json!([{"nome": "Ana", "nl": 2, "nossa": false, "resumo": "Ana | 10:00 | oi"}, {"nome": "Ana", "nl": 2}, {"nome": " "}]);
        assert_eq!(itens_de(&v).len(), 1);
        let m = json!({"msgs": [{"d": "ELE", "c": "[10:00, 01/10/2026] Ana: ", "t": "oi"}], "midias": [{"d": "ELE", "t": "[AUDIO]"}, {"d": "ELE", "t": "[IMAGEM]"}, {"d": "EU", "t": "[IMAGEM]"}]});
        let (c, n) = conversa_de(&m, "Ana");
        assert_eq!((c.msgs.len(), c.midias, c.audio_dele, n), (1, vec!["[IMAGEM]".to_string()], false, 1));
        let (c, _) = conversa_de(&json!({"msgs": [], "midias": [{"d": "ELE", "t": "[IMAGEM]"}, {"d": "ELE", "t": "[AUDIO]"}]}), "");
        assert!(c.audio_dele);
        assert!(JS_MSGS.contains("@MAX@") && JS_MSGS.contains("@EU@") && !JS_MSGS.contains("Dono"));
    }
}
