//! Laco do Airbnb de verdade: le a LISTA da caixa de mensagens do anfitriao na aba `airbnb` da VM (ja logada pelo dono) e entrega as
//! linhas a `airbnb_escalada` (30 min sem resposta avisa a primeira pessoa e o dono; pedido/consulta de reserva avisa os dois na hora;
//! 15 min depois a terceira; lembrete e parada pelas regras portadas). Seletor e JS copiados de braniac/airbnb.py (`JS_LINHAS`).
//! REGRAS FIXAS: este modulo NUNCA responde hospede, NUNCA aceita, pre-aprova ou recusa reserva, NUNCA mexe em preco ou anuncio, NUNCA faz
//! login: so le a lista (um JS de leitura) e, se a aba nao estiver na caixa de mensagens, volta para a caixa. Nao abre conversa de hospede
//! (a conferencia "quem escreveu" do Python, que abre ate 4 conversas por ciclo, nao foi portada: usa so o que a lista mostra).
//! Chave: dados/troca.json {"airbnb": false}. DESLIGADA = SOMBRA: le a lista, decide e grava em `crate::saida` (canal "airbnb-aviso"); nada sai.
//! LIGADA: os avisos saem pelo WhatsApp do Braniac, que respeita as chaves do WhatsApp (`whatsapp`, `whatsapp_so_dono`): chat que
//! nao esta liberado la vira registro na caixa de saida, nunca mensagem.
use crate::airbnb::{parse_linha, protegido_extra};
use crate::airbnb_escalada::{pendencias, Destinos};
use crate::airbnb_vigia::Linha;
use crate::contexto::Ctx;
use crate::tempo::Agora;
use crate::wpp_vivo::{self as wv, Pagina};
use crate::{saida, wpp_laco};
use regex::Regex;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Mutex;

pub const AGENTE: &str = "airbnb";
const URL_INBOX: &str = "https://www.airbnb.com.br/hosting/messages";
const JS_LINHAS: &str = "JSON.stringify([...document.querySelectorAll('[data-testid^=inbox_list_]')].slice(0,%d).map(r=>({id:r.getAttribute('data-testid').slice(11),t:(r.innerText||'')})))";
const JS_TEM_LISTA: &str = "!!document.querySelector('[data-testid^=inbox_list_]')";

static ULTIMO: Mutex<Option<Value>> = Mutex::new(None);

/// As linhas da caixa (mais recentes primeiro), ja interpretadas. So leitura.
pub fn ler_caixa(pag: &mut dyn Pagina, extra: Option<&Regex>, max: usize) -> Result<Vec<Linha>, String> {
    if !pag.js("location.pathname")?.as_str().unwrap_or("").contains("/hosting/messages") {
        pag.js(&format!("location.href='{URL_INBOX}'"))?;
        pag.dormir(3000);
    }
    let mut tem = false;
    for _ in 0..15 {
        if pag.js(JS_TEM_LISTA)? == true {
            tem = true;
            break;
        }
        pag.dormir(1000);
    }
    if !tem {
        return Err("a caixa de mensagens do Airbnb nao carregou (aba deslogada ou fora do ar?)".into());
    }
    let bruto = pag.js(&JS_LINHAS.replace("%d", &max.to_string()))?;
    let itens: Vec<Value> = serde_json::from_str(bruto.as_str().unwrap_or("[]")).map_err(|e| format!("lista ilegivel: {e}"))?;
    Ok(itens.iter().filter_map(|b| parse_linha(b["id"].as_str()?, b["t"].as_str()?, extra)).map(|v| Linha::de_json(&v)).collect())
}

/// Entrega um aviso a uma PESSOA DA FAMILIA/dono (nunca ao hospede). Chave `airbnb` desligada = so a caixa de saida.
/// Ligada: pelo WhatsApp do Braniac, e so de verdade se o chat esta liberado la.
pub fn aviso(dados: &Path, contato: &str, texto: &str) -> Result<(), String> {
    if !wpp_laco::chave(dados, "airbnb") {
        saida::registrar(dados, "airbnb-aviso", contato, texto);
        return Ok(());
    }
    let (ctx, nomes) = (Ctx::ler(dados), crate::alma_cuida::Nomes::ler(dados));
    let real = wpp_laco::ligada(dados) || (wpp_laco::so_dono(dados) && ctx.chat_do_dono() == Some(contato));
    if !real {
        saida::registrar(dados, "whatsapp", contato, texto);
        return Ok(());
    }
    let _g = wpp_laco::TRAVA.lock().map_err(|_| "o WhatsApp esta ocupado".to_string())?;
    let mut pag = wv::Real::conectar(dados)?;
    let r = wv::enviar_texto(&mut pag, dados, &ctx, &nomes, contato, texto);
    if r.starts_with('✅') { Ok(()) } else { Err(r) }
}

/// UM ciclo: le a caixa, acha as pendencias e avanca a escalada. `enviar(contato, texto)` entra de fora.
pub fn ciclo(dados: &Path, agora: &Agora, pag: &mut dyn Pagina, enviar: &dyn Fn(&str, &str) -> Result<(), String>) -> Result<Value, String> {
    let linhas = ler_caixa(pag, protegido_extra(dados).as_ref(), 25)?;
    let pend = pendencias(&linhas, agora);
    let vistos: BTreeSet<String> = linhas.iter().map(|l| l.id.clone()).collect();
    let st = crate::airbnb_escalada::passo(dados, &pend, &vistos, agora, enviar, &Destinos::ler(dados))?;
    let ativas = st.as_object().map_or(0, |o| o.keys().filter(|k| *k != "_ok").count());
    Ok(json!({ "conversas": linhas.len(), "pendencias": pend.len(), "ativas": ativas }))
}

pub fn estado() -> Value {
    ULTIMO.lock().ok().and_then(|u| u.clone()).unwrap_or(Value::Null)
}

/// A thread: sempre roda (sombra ou viva); a chave so decide se o aviso sai. 60 s entre ciclos (30 s com pendencia).
pub fn iniciar() {
    std::thread::spawn(|| {
        let mut espera = 60;
        loop {
            std::thread::sleep(std::time::Duration::from_secs(espera));
            let dados = crate::pasta_dados();
            if crate::power::pausado(&dados) {
                continue;
            }
            let r = crate::abas::aba_aberta(&dados, AGENTE).map(wv::Real).and_then(|mut p| ciclo(&dados, &crate::tempo::agora(), &mut p, &|c, t| aviso(&dados, c, t)));
            espera = match &r { Ok(v) if v["ativas"].as_u64().unwrap_or(0) > 0 => 30, Err(_) => 120, _ => 60 };
            if let Ok(mut u) = ULTIMO.lock() {
                *u = Some(match r {
                    Ok(v) => json!({ "ts": crate::fatos::agora_ts(), "modo": if wpp_laco::chave(&dados, "airbnb") { "vivo" } else { "sombra" }, "ok": v }),
                    Err(e) => json!({ "ts": crate::fatos::agora_ts(), "erro": e }),
                });
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;
    use std::cell::RefCell;

    struct Falsa {
        lista: String,
        gestos: RefCell<Vec<String>>,
        no_inbox: bool,
    }

    impl Pagina for Falsa {
        fn js(&mut self, e: &str) -> Result<Value, String> {
            if e == "location.pathname" {
                return Ok(json!(if self.no_inbox { "/hosting/messages/123" } else { "/hosting" }));
            }
            if e == JS_TEM_LISTA {
                return Ok(json!(true));
            }
            if e.starts_with("JSON.stringify([...document.querySelectorAll('[data-testid^=inbox_list_]')]") {
                return Ok(json!(self.lista));
            }
            self.gestos.borrow_mut().push(format!("js:{}", e.chars().take(40).collect::<String>()));
            if e.starts_with("location.href='https://www.airbnb.com.br/hosting/messages'") { Ok(Value::Null) } else { Err("fora do roteiro".into()) }
        }
        fn mouse(&mut self, t: &str, _: f64, _: f64) -> Result<(), String> { self.gestos.borrow_mut().push(format!("mouse:{t}")); Ok(()) }
        fn inserir(&mut self, t: &str) -> Result<(), String> { self.gestos.borrow_mut().push(format!("inserir:{t}")); Ok(()) }
        fn enter(&mut self) -> Result<(), String> { self.gestos.borrow_mut().push("enter".into()); Ok(()) }
        fn frente(&mut self) -> Result<(), String> { self.gestos.borrow_mut().push("frente".into()); Ok(()) }
        fn dormir(&mut self, _: u64) {}
    }

    fn pasta(nome: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-airbnb_laco-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("pessoal.json"), r#"{"airbnb_escalada": {"primeira": {"contato": "Tia", "vocativo": "oh minha tia", "citado": "a Tia Bia"},
            "dono": {"contato": "Davi", "vocativo": "Senhor Davi", "citado": "o Davi"}, "terceira": {"contato": "Lia", "vocativo": "Lia", "citado": "a Lia"}}}"#).unwrap();
        d
    }

    fn agora(min: u32) -> Agora {
        Agora { data: Data::civil(2026, 10, 1).unwrap(), hora: 10, min, seg: 0 }
    }

    fn caixa() -> String {
        let itens = [("t1", "Pendente. 5 – 6 de jan.. Quarto Azul. Bia. A última mensagem 09:55 enviada é: Pedido de reserva"),
            ("t2", "Lida. Quarto Azul. Ana. A última mensagem 09:58 enviada é: Ana: Tem garagem?"),
            ("t3", "Lida. Quarto Azul. Caio. A última mensagem 09:50 enviada é: Você: Bem-vindo!"),
            ("t4", "texto que nao e linha de caixa")];
        json!(itens.iter().map(|(id, t)| json!({ "id": id, "t": t })).collect::<Vec<_>>()).to_string()
    }

    #[test]
    fn le_a_lista_com_o_seletor_do_python_e_so_com_js_de_leitura() {
        let mut p = Falsa { lista: caixa(), gestos: RefCell::new(vec![]), no_inbox: true };
        let l = ler_caixa(&mut p, None, 25).unwrap();
        assert_eq!(l.len(), 3); // a linha ilegivel some
        assert_eq!((l[0].hospede.as_str(), l[0].anuncio.as_str(), l[0].ultima_de.as_str()), ("Bia", "Quarto Azul", "outro"));
        assert_eq!((l[1].ultima_de.as_str(), l[2].ultima_de.as_str()), ("hospede", "voce"));
        assert!(p.gestos.borrow().is_empty(), "{:?}", p.gestos);
        // aba fora da caixa: so navega de volta para a caixa de mensagens (nada de clique)
        let mut q = Falsa { lista: caixa(), gestos: RefCell::new(vec![]), no_inbox: false };
        ler_caixa(&mut q, None, 25).unwrap();
        assert_eq!(q.gestos.borrow().len(), 1);
    }

    #[test]
    fn sombra_nao_abre_conversa_nao_envia_e_decide_na_caixa_de_saida() {
        let d = pasta("sombra");
        assert!(!wpp_laco::chave(&d, "airbnb"));
        let mut p = Falsa { lista: caixa(), gestos: RefCell::new(vec![]), no_inbox: true };
        let r = ciclo(&d, &agora(0), &mut p, &|c, t| aviso(&d, c, t)).unwrap();
        assert_eq!((r["conversas"].as_u64(), r["pendencias"].as_u64()), (Some(3), Some(2)));
        assert!(p.gestos.borrow().is_empty(), "sombra: so leitura da lista, {:?}", p.gestos);
        let s = saida::ultimas(&d, 20);
        let s = s.as_array().unwrap();
        // pedido de reserva: mae e dono juntos, na hora, com a sintese e a frase de que o sistema nao aceita
        let de = |c: &str| s.iter().find(|x| x["canal"] == "airbnb-aviso" && x["destino"] == c && x["texto"].as_str().unwrap().contains("pedido de reserva")).cloned();
        assert!(de("Tia").is_some() && de("Davi").is_some(), "{s:?}");
        assert!(de("Davi").unwrap()["texto"].as_str().unwrap().contains("Eu não aceito reserva"));
        assert!(s.iter().all(|x| x["canal"] != "whatsapp"), "sombra nao toca no canal do WhatsApp");
        // a mensagem comum (Ana) espera 30 min: nada ainda; depois de 30 min avisa os dois
        assert!(s.iter().all(|x| !x["texto"].as_str().unwrap().contains("Ana")));
        ciclo(&d, &agora(31), &mut p, &|c, t| aviso(&d, c, t)).unwrap();
        let n_ana = saida::ultimas(&d, 50).as_array().unwrap().iter().filter(|x| x["texto"].as_str().unwrap().contains("Ana")).count();
        assert_eq!(n_ana, 2);
        assert!(p.gestos.borrow().is_empty());
    }

    #[test]
    fn ligada_com_whatsapp_desligado_nunca_digita_so_registra() {
        let d = pasta("viva");
        wpp_laco::definir_chave(&d, "airbnb", true).unwrap();
        // WhatsApp desligado (nem "so o dono"): `aviso` nem tenta a aba (sem VM no teste, tentar daria erro)
        assert_eq!(aviso(&d, "Tia", "oi"), Ok(()));
        let s = saida::ultimas(&d, 5);
        assert!(s[0]["canal"] == "whatsapp" && s[0]["destino"] == "Tia");
    }

    #[test]
    fn este_modulo_nao_tem_como_responder_hospede() {
        let fonte = include_str!("airbnb_laco.rs");
        let codigo = fonte.split("#[cfg(test)]").next().unwrap();
        for proibido in [".inserir(", ".enter(", ".mouse(", ".frente(", "escolher_arquivo", "enviar_audio", "enviar_arquivo", "location.href=' + ", "Input.", "navegar("] {
            assert!(!codigo.contains(proibido), "{proibido}");
        }
        // o unico envio e `aviso`, que so fala com a lista de pessoas de `Destinos` (nunca com o hospede)
        assert_eq!(codigo.matches("wv::enviar_texto(").count(), 1);
    }
}
