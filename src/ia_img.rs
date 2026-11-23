//! Gera imagem pelo GEMINI (aba logada do navegador). Mesmo cuidado do `ia_web`: uma aba na frente por vez e o foco volta no fim.
//! O prompt vai para o Google: nunca dado pessoal, de terceiros ou segredo (`ia_web::barrado` recusa antes de abrir a aba).
//! Foto de referencia so com autorizacao de quem esta na foto (decisao de quem chama; aqui ela so e repassada a closure `anexar`).
//! Ficou de fora: o navegador (`ia_web::Aba`), anexar a foto e baixar a imagem (closures); o arquivo e gravado por quem chama.
#![allow(dead_code)]
use crate::empresa_dev::rx;
use crate::ia_web::{barrado, site, Aba};
use regex::Regex;
use serde_json::Value;
use std::sync::{Mutex, OnceLock};

static VEZ: Mutex<()> = Mutex::new(());

pub const JS_IMGS: &str = "(function(){ var r=[...document.querySelectorAll('model-response, .model-response-text, message-content')]; var alvo=r[r.length-1]||document;\n  var imgs=[...alvo.querySelectorAll('img')].filter(i=>i.naturalWidth>=256 && !/avatar|logo|icon/i.test(i.className+i.alt)).map(i=>({src:i.currentSrc||i.src,w:i.naturalWidth,h:i.naturalHeight}));\n  return JSON.stringify(imgs); })()";
/// O fetch da pagina do Gemini e bloqueado (CORS): a IMAGEM e aberta como documento proprio em aba nova e lida pelo canvas. null = ainda em branco.
pub const JS_CANVAS: &str = "(async function(){ var i=document.images[0]; if(!i || !i.complete || !i.naturalWidth) return null; try { await i.decode(); } catch(e) {}\n  var c=document.createElement('canvas'); c.width=i.naturalWidth; c.height=i.naturalHeight; var x=c.getContext('2d'); x.drawImage(i,0,0);\n  var px=x.getImageData(0,0,c.width,c.height).data, nb=0; for (var k=0;k<px.length;k+=4000){ if (px[k]<245||px[k+1]<245||px[k+2]<245) nb++; }\n  if (!nb) return null;\n  return JSON.stringify({w:c.width,h:c.height,b64:c.toDataURL('image/png').split(',')[1]}); })()";

/// Pede o tamanho 2048 (capa do YouTube exige pelo menos 2048x1152).
pub fn url_2048(src: &str) -> String {
    static FIM: OnceLock<Regex> = OnceLock::new();
    static TEM: OnceLock<Regex> = OnceLock::new();
    if rx(&TEM, r"=s\d+").is_match(src) { rx(&FIM, r"=s\d+(-rj)?$").replace(src, "=s2048-rj").into_owned() } else { src.to_string() }
}

/// A maior imagem da resposta.
pub fn maior(imgs: &[Value]) -> Option<&Value> {
    imgs.iter().max_by_key(|i| i["w"].as_i64().unwrap_or(0) * i["h"].as_i64().unwrap_or(0))
}

fn imagens(a: &Aba) -> Result<Vec<Value>, String> {
    Ok(serde_json::from_str((a.js)(JS_IMGS)?.as_str().unwrap_or("[]")).unwrap_or_default())
}

/// Pede a imagem ao Gemini e devolve os bytes. `baixar(endereco)` busca a imagem (ja com o tamanho 2048 pedido).
pub fn gerar(prompt: &str, timeout_s: f64, referencia: Option<&str>, sensivel: &dyn Fn(&str) -> bool, a: &Aba, anexar: &dyn Fn(&str) -> Result<(), String>, baixar: &dyn Fn(&str) -> Result<Vec<u8>, String>) -> Result<Vec<u8>, String> {
    let s = site("gemini").ok_or("gemini: site nao cadastrado")?;
    if let Some(motivo) = barrado(prompt, sensivel) {
        return Err(motivo.into());
    }
    let _vez = VEZ.lock().unwrap_or_else(|e| e.into_inner());
    let feito = (|| {
        (a.frente)();
        (a.navegar)(s.nova)?;
        (a.esperar)(5.0);
        if let Some(r) = referencia {
            anexar(r)?;
        }
        (a.js)(&format!("document.querySelector({}).focus()", Value::from(s.entrada)))?;
        (a.digitar)(prompt)?;
        (a.esperar)(1.0);
        (a.js)(&format!("(function(){{var b=document.querySelector({}); if(b) b.click()}})()", Value::from(s.enviar)))?;
        let fim = (a.relogio)() + timeout_s;
        while (a.relogio)() < fim {
            (a.esperar)(3.0);
            (a.frente)();
            let imgs = imagens(a)?;
            if imgs.is_empty() {
                continue;
            }
            (a.esperar)(4.0); // deixa terminar de renderizar a versao final
            let finais = Some(imagens(a)?).filter(|f| !f.is_empty()).unwrap_or(imgs);
            let src = maior(&finais).and_then(|i| i["src"].as_str()).ok_or("o Gemini devolveu imagem sem endereço")?;
            return baixar(&url_2048(src));
        }
        Err("o Gemini não devolveu imagem a tempo (pode ter recusado o pedido ou estar sem cota)".to_string())
    })();
    (a.devolver_foco)();
    feito
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ia_web::falso::Pagina;
    use serde_json::json;
    use std::cell::RefCell;

    #[test]
    fn gera_pega_a_maior_em_2048_e_anexa_a_referencia() {
        let p = Pagina::nova(|e, t| Ok(json!(if e != JS_IMGS { String::new() } else if t < 20.0 { "[]".to_string() } else { json!([{"src": "https://img/a=s512-rj", "w": 512, "h": 512}, {"src": "https://img/b=s1024", "w": 1024, "h": 1024}]).to_string() })));
        let pedidos = RefCell::new(vec![]);
        let r = p.usar(|a| gerar("capa do canal, estilo neon", 180.0, Some("ref.jpg"), &|_| false, a, &|f| {
            pedidos.borrow_mut().push(format!("anexar {f}"));
            Ok(())
        }, &|u| {
            pedidos.borrow_mut().push(format!("baixar {u}"));
            Ok(vec![1, 2, 3])
        }));
        assert_eq!(r, Ok(vec![1, 2, 3]));
        assert_eq!(*pedidos.borrow(), vec!["anexar ref.jpg", "baixar https://img/b=s2048-rj"]);
        assert_eq!(*p.feito.borrow(), vec!["navegar https://gemini.google.com/app", "digitar capa do canal, estilo neon", "foco"]);
    }

    #[test]
    fn sem_imagem_da_erro_e_prompt_com_segredo_nem_abre_a_aba() {
        let p = Pagina::nova(|e, _| Ok(json!(if e == JS_IMGS { "[]" } else { "" })));
        let r = p.usar(|a| gerar("um gato", 20.0, None, &|_| false, a, &|_| panic!("sem referencia"), &|_| panic!("sem imagem")));
        assert!(r.unwrap_err().contains("não devolveu imagem a tempo") && p.feito.borrow().last().map(String::as_str) == Some("foco"));
        let q = Pagina::nova(|_, _| panic!("nao pode abrir"));
        assert!(q.usar(|a| gerar("cartaz com o cpf do cliente", 20.0, None, &|_| false, a, &|_| Ok(()), &|_| Ok(vec![]))).unwrap_err().contains("barrada"));
        assert!(q.usar(|a| gerar("retrato da vizinha", 20.0, None, &|_| true, a, &|_| Ok(()), &|_| Ok(vec![]))).unwrap_err().contains("barrada"));
        assert!(q.feito.borrow().is_empty());
        assert_eq!((url_2048("https://x/img=s512"), url_2048("https://x/img=s512-rj"), url_2048("https://x/img.png"), url_2048("https://x/a=s64/b")), ("https://x/img=s2048-rj".to_string(), "https://x/img=s2048-rj".to_string(), "https://x/img.png".to_string(), "https://x/a=s64/b".to_string()));
        assert_eq!(maior(&[]), None);
    }
}
