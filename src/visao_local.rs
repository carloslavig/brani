//! Ver imagem recebida no WhatsApp, SO LOCAL (porte de wpp_img.py + imagem.py sem a nuvem): le a miniatura da ultima imagem RECEBIDA
//! na conversa aberta (nunca as nossas, pelo menos 80 px) e descreve com o modelo de visao do Ollama NESTE computador (qwen2.5vl:3b, depois
//! moondream; regras em `imagem.rs`). Nao existe plano B online: sem modelo ou sem Ollama, devolve o erro e a imagem nao sai do PC.
//! A conversa aberta e conferida por quem chama (`wpp_vivo::abrir_destino`). A leitura de imagem so ocorre em conversa que o atendimento
//! ja vai tratar, e so com a chave ligada (em sombra nada abre).
use crate::imagem;
use crate::wpp_vivo::Pagina;
use serde_json::{json, Value};

const OLLAMA: &str = "http://127.0.0.1:11434";

/// Dispara o download do blob (assincrono) e guarda em window.__img; a leitura vem logo depois.
const JS_INICIAR: &str = r##"(function(){window.__img=null;
  var main=document.querySelector('#main'); if(!main) return false;
  var imgs=[].slice.call(main.querySelectorAll('[role="row"] img[src^="blob:"]')).filter(function(i){
    return !i.closest('[class*="message-out"]') && (i.naturalWidth||i.width)>=80; });
  if(!imgs.length) return false;
  var alvo=imgs[imgs.length-1]; alvo.scrollIntoView({block:'nearest'});
  (async function(){ try{ var b=await (await fetch(alvo.src)).blob(); var buf=new Uint8Array(await b.arrayBuffer()); var bin='';
    for (var i=0;i<buf.length;i+=0x8000) bin+=String.fromCharCode.apply(null,buf.subarray(i,i+0x8000));
    window.__img=JSON.stringify({tipo:b.type,b64:btoa(bin)}); }catch(e){ window.__img='erro'; } })();
  return true; })()"##;
const JS_LER: &str = "window.__img||null";

/// Bytes da ultima imagem recebida na conversa JA ABERTA (ate 8 tentativas, 1 s entre elas).
pub fn ler_ultima_imagem(p: &mut dyn Pagina) -> Result<Vec<u8>, String> {
    for _ in 0..8 {
        p.dormir(1000);
        if p.js(JS_INICIAR).ok().and_then(|v| v.as_bool()) != Some(true) {
            continue;
        }
        for _ in 0..10 {
            p.dormir(500);
            if let Some(t) = p.js(JS_LER).ok().and_then(|v| v.as_str().map(String::from)) {
                let b64 = serde_json::from_str::<Value>(&t).ok().and_then(|v| v["b64"].as_str().map(String::from));
                return b64.ok_or_else(|| "não consegui ler a imagem".to_string()).and_then(|b| crate::base64::decodificar(&b));
            }
        }
    }
    Err("Não achei imagem recebida nessa conversa.".into())
}

fn ollama(dados: &[u8], instrucao: &str) -> Result<String, String> {
    let instalados: Vec<String> = ureq::get(&format!("{OLLAMA}/api/tags")).timeout(std::time::Duration::from_secs(3)).call().ok()
        .and_then(|r| r.into_json::<Value>().ok()).map(|v| v["models"].as_array().into_iter().flatten().filter_map(|m| m["name"].as_str().map(String::from)).collect()).unwrap_or_default();
    let (modelo, prompt, prefixo) = imagem::modelo_local(&instalados, instrucao).ok_or("ollama sem modelo de visao (qwen2.5vl:3b ou moondream)")?;
    let tempo = if modelo.starts_with("qwen") { 150 } else { 60 };
    let corpo = json!({ "model": modelo, "prompt": prompt, "stream": false, "images": [crate::base64::codificar(dados)], "options": { "temperature": 0.2 } });
    let r = ureq::post(&format!("{OLLAMA}/api/generate")).timeout(std::time::Duration::from_secs(tempo)).send_json(corpo).map_err(|e| format!("ollama indisponivel: {e}"))?;
    let txt = r.into_json::<Value>().map_err(|e| e.to_string())?["response"].as_str().unwrap_or("").trim().to_string();
    if txt.is_empty() { Err("ollama: resposta vazia".into()) } else { Ok(format!("{prefixo}{txt}")) }
}

/// Descreve os bytes so com o modelo local. Sem nuvem de reserva.
pub fn descrever(dados: &[u8]) -> Result<String, String> {
    descrever_com(dados, &ollama)
}

fn descrever_com(dados: &[u8], local: &dyn Fn(&[u8], &str) -> Result<String, String>) -> Result<String, String> {
    imagem::descrever(dados, None, true, local, &|_, _| Err("sem nuvem: a imagem fica neste computador".into()))
}

/// Le e descreve a imagem da conversa aberta; erro curto (120 letras), como no Python.
pub fn ver(p: &mut dyn Pagina) -> Result<String, String> {
    let b = ler_ultima_imagem(p)?;
    descrever(&b).map_err(|e| e.chars().take(120).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn so_local_sem_plano_b_online() {
        let ok = descrever_com(&[1, 2, 3], &|_, i| Ok(format!("local:{}", i.len() > 10)));
        assert_eq!(ok.unwrap(), "local:true");
        let e = descrever_com(&[1], &|_, _| Err("ollama indisponivel".into())).unwrap_err();
        assert!(e.contains("ollama indisponivel") && e.contains("sem nuvem"), "{e}");
        assert!(descrever_com(&[], &|_, _| Ok("x".into())).is_err());
    }

    #[test]
    fn instrucao_pede_para_nao_repetir_dado_sensivel() {
        assert!(imagem::PROMPT_LER.contains("NAO escreva numeros de cartao"));
        assert!(OLLAMA.starts_with("http://127.0.0.1"));
    }
}
