//! Executor, trabalhos LONGOS (no Python rodam em thread): delegar_ao_claude, melhorar_braniac, trabalho_profundo, whatsapp_atender,
//! gerar_imagem, criar_video e estudio_3d. `iniciar` passa pelos porteiros (teto do dia, um por vez), poe na fila pela closure
//! `segundo_plano` e responde na hora; `rodar` e o trabalho em si (bloqueante), para quem integra chamar numa thread.
//! "Não ligado ainda" em `rodar`: gerar_imagem (os motores de imagem) e whatsapp_atender (o laco do atendimento).
#![allow(dead_code)]
use crate::executor::{arg, com_automelhoria, corta, nao_ligada, Ambiente};
use crate::jarvis_travas::Pedido;
use crate::{automelhoria, claude_agent, estudio3d, profundo};
use serde_json::{json, Value};

pub const LONGAS: [&str; 7] = ["criar_video", "delegar_ao_claude", "estudio_3d", "gerar_imagem", "melhorar_braniac", "trabalho_profundo", "whatsapp_atender"];

fn tema<'a>(nome: &str, args: &'a Value) -> &'a str {
    arg(args, if nome == "criar_video" { "descricao" } else { "tema" })
}

fn trava<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Resposta imediata para a IA. None = a ferramenta nao e longa.
pub fn iniciar(amb: &Ambiente, nome: &str, args: &Value) -> Option<String> {
    let fila = || (amb.segundo_plano)(nome, args).map_err(|e| format!("Não comecei: o trabalho em segundo plano está {e}."));
    Some(match nome {
        "delegar_ao_claude" => {
            if arg(args, "tarefa").is_empty() {
                return Some("tarefa vazia".into());
            }
            if (amb.uso_claude)() >= amb.teto_claude {
                return Some("teto diario do Claude atingido; diga isso ao dono".into());
            }
            match fila() {
                Ok(_) => {
                    (amb.add_uso_claude)();
                    "Claude acionado em segundo plano; o resultado chega ao dono quando terminar.".into()
                }
                Err(e) => e,
            }
        }
        "melhorar_braniac" => {
            if arg(args, "pedido").chars().count() < 8 {
                return Some("Qual melhoria você quer? Me diga o que mudar no Braniac/Jarvis.".into());
            }
            fila().map_or_else(|e| e, |_| "Automelhoria iniciada em segundo plano (10 a 25 min): o conselho planeja, o agente implementa e eu só aplico se os testes e as travas passarem. Aviso o dono a cada etapa.".into())
        }
        "trabalho_profundo" => {
            let obj = arg(args, "objetivo");
            if obj.is_empty() {
                return Some("Qual o objetivo do trabalho?".into());
            }
            if let Err(e) = amb.profundo.comecar(obj) {
                return Some(e);
            }
            fila().map_or_else(|e| { amb.profundo.terminar(); e }, |_| "Modo profundo iniciado em segundo plano: planejo, executo cada etapa e confiro antes de entregar. O resultado chega ao dono.".into())
        }
        "whatsapp_atender" => fila().map_or_else(|e| e, |_| "(atendimento iniciado em segundo plano; o relatorio chega ao dono)".into()),
        "gerar_imagem" => {
            let fala = match trava(&amb.criacao).comecar(arg(args, "descricao")) {
                Ok(f) => f.to_string(),
                Err(e) => return Some(e),
            };
            fila().map_or_else(|e| { *trava(&amb.criacao) = Default::default(); e }, |_| fala)
        }
        "criar_video" | "estudio_3d" => {
            let mut est = trava(&amb.estudio);
            let fala = match estudio3d::comecar(&amb.dados, &amb.agora, &mut est, tema(nome, args), args["duracao_s"].as_f64(), amb.estudio_instalado) {
                Ok((_, f)) => f,
                Err(e) => return Some(e),
            };
            fila().map_or_else(|e| { est.rodando = false; e }, |_| fala)
        }
        _ => return None,
    })
}

/// O trabalho em si (bloqueante). Avisos e entregas vao para `saida_direta`; o que e para o WhatsApp do dono sai pela closure de envio.
pub fn rodar(amb: &Ambiente, p: &Pedido, nome: &str, args: &Value) -> String {
    let direto = |t: &str| amb.saida_direta.borrow_mut().push(t.to_string());
    match nome {
        "delegar_ao_claude" => claude_agent::executar(&amb.dados, arg(args, "tarefa"), &amb.agora, "Claude", &*amb.agente, &|r| direto(&format!("🤖 Claude terminou:\n\n{r}"))),
        "melhorar_braniac" => com_automelhoria(amb, |a| automelhoria::melhorar(&amb.dados, a, arg(args, "pedido"), None, &amb.agora.data.iso())),
        "trabalho_profundo" => {
            let entregar = |f: &str| {
                direto(&format!("🧭 {f}"));
                if let Some(dono) = p.ctx.chat_do_dono() {
                    let _ = (amb.wpp_texto)(dono, f);
                    let _ = (amb.wpp_audio)(dono, &profundo::texto_para_audio(f));
                }
            };
            profundo::rodar_marcado(&amb.profundo, arg(args, "objetivo"), &*amb.jarvis, &|t| (amb.ia)("profundo-plano", t).unwrap_or_default(), &direto, &entregar);
            "Modo profundo encerrado.".into()
        }
        "criar_video" | "estudio_3d" => {
            let t = tema(nome, args);
            let dur = estudio3d::duracao(t, args["duracao_s"].as_f64());
            let r = estudio3d::roteiro(t, dur, &*amb.ia).and_then(|(titulo, narracao, cenas)| {
                let pasta = amb.dados.join("estudio3d").join("pedidos");
                let arq = pasta.join(format!("{}-{}.json", amb.agora.carimbo(), estudio3d::slug(&titulo)));
                std::fs::create_dir_all(&pasta).and_then(|_| std::fs::write(&arq, json!({ "titulo": titulo, "narracao": narracao, "cenas": cenas, "duracao": dur }).to_string())).map_err(|e| e.to_string())?;
                let video = (amb.render)("estudio3d", &arq)?;
                Ok((titulo, video.to_string_lossy().to_string()))
            });
            // rascunho: so entrega ao dono, nunca publica
            let blocos = estudio3d::terminar(&mut trava(&amb.estudio), t, &r, "Senhor");
            blocos.iter().for_each(|b| direto(b));
            r.map_or_else(|e| format!("O vídeo não saiu: {}", corta(&e, 150)), |(titulo, _)| format!("Vídeo «{titulo}» pronto (rascunho)."))
        }
        _ => nao_ligada(nome),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::tests::{amb, ctx, pasta, ped, saida};
    use crate::executor::{executar, NAO_LIGADO};
    use crate::jarvis_travas::{BLOQUEADO, DONO_CONV};
    use std::cell::RefCell;
    use std::rc::Rc;

    fn com_fila(d: &std::path::Path) -> (Ambiente, Rc<RefCell<Vec<String>>>) {
        let fila = Rc::new(RefCell::new(Vec::new()));
        let f2 = fila.clone();
        (Ambiente { segundo_plano: Box::new(move |n, _| { f2.borrow_mut().push(n.to_string()); Ok(String::new()) }), ..amb(d) }, fila)
    }

    #[test]
    fn trabalho_longo_de_fora_so_com_autonomia_ou_no_chat_do_dono() {
        let (d, c) = (pasta("longas-travas"), ctx());
        let (a, fila) = com_fila(&d);
        let fora = ped("whatsapp", "jarvis", "pede pro claude arrumar a fundo", false, &c, &[]);
        for n in ["delegar_ao_claude", "trabalho_profundo", "whatsapp_atender"] {
            assert_eq!(executar(&a, &fora, n, &json!({ "tarefa": "arruma a planilha", "objetivo": "comparar preços" })).unwrap().0, BLOQUEADO, "{n}");
        }
        for n in ["gerar_imagem", "criar_video"] {
            assert!(executar(&a, &fora, n, &json!({ "descricao": "um farol ao por do sol" })).unwrap().0.starts_with("🔒"), "{n}");
        }
        // o proprio codigo: so o Telegram sem autonomia e barrado
        let tg = ped("telegram", "telegram", "x", false, &c, &[]);
        assert_eq!(executar(&a, &tg, "melhorar_braniac", &json!({ "pedido": "responda mais curto" })).unwrap().0, BLOQUEADO);
        assert!(fila.borrow().is_empty());
        // chat verificado do dono libera o Claude; pergunta simples nao abre o modo profundo
        let dono = ped("whatsapp", DONO_CONV, "pede pro claude arrumar", false, &c, &[]);
        assert!(executar(&a, &dono, "delegar_ao_claude", &json!({ "tarefa": "arruma a planilha" })).unwrap().0.starts_with("Claude acionado"));
        let simples = ped("pc", "claude", "qual a capital da França?", false, &c, &[]);
        assert!(executar(&a, &simples, "trabalho_profundo", &json!({ "objetivo": "x" })).unwrap().0.starts_with("(nao abra o modo profundo"));
        assert_eq!(*fila.borrow(), ["delegar_ao_claude"]);
    }

    #[test]
    fn porteiros_teto_do_claude_um_profundo_por_vez_e_nada_ligado_no_seguro() {
        let (d, c) = (pasta("longas-porteiros"), ctx());
        let p = ped("pc", "jarvis", "pesquisa a fundo o mercado", false, &c, &[]);
        // ambiente seguro: a fila nao esta ligada, e o porteiro solta a vez
        let seguro = amb(&d);
        for (n, v) in [("delegar_ao_claude", json!({ "tarefa": "x y z" })), ("trabalho_profundo", json!({ "objetivo": "comparar" })), ("gerar_imagem", json!({ "descricao": "um farol" })),
                       ("melhorar_braniac", json!({ "pedido": "responda mais curto" })), ("whatsapp_atender", json!({}))] {
            assert!(executar(&seguro, &p, n, &v).unwrap().0.contains(NAO_LIGADO), "{n}");
        }
        assert!(seguro.profundo.comecar("outro").is_ok() && !trava(&seguro.criacao).rodando);
        // estudio nao instalado: recusa sem gastar o teto do dia
        assert_eq!(executar(&seguro, &p, "criar_video", &json!({ "descricao": "como funciona um farol" })).unwrap().0, "Não achei o Estúdio 3D neste PC.");
        assert!(!d.join("estudio3d_uso.json").exists());
        let (mut a, fila) = com_fila(&d);
        a.estudio_instalado = true;
        assert_eq!(executar(&a, &p, "delegar_ao_claude", &json!({ "tarefa": " " })).unwrap().0, "tarefa vazia");
        assert!(executar(&a, &p, "trabalho_profundo", &json!({ "objetivo": "comparar fornecedores" })).unwrap().0.starts_with("Modo profundo iniciado"));
        assert!(executar(&a, &p, "trabalho_profundo", &json!({ "objetivo": "outro" })).unwrap().0.starts_with("Ja estou num trabalho profundo"));
        assert!(executar(&a, &p, "estudio_3d", &json!({ "tema": "como funciona um farol" })).unwrap().0.starts_with("Começando o vídeo"));
        assert!(executar(&a, &p, "criar_video", &json!({ "descricao": "como nasce uma estrela" })).unwrap().0.contains("um por vez"));
        assert!(executar(&a, &p, "criar_video", &json!({ "descricao": "curto" })).unwrap().0.starts_with("Qual o tema"));
        assert!(executar(&a, &p, "gerar_imagem", &json!({ "descricao": "um farol" })).unwrap().0.starts_with("Comecei"));
        assert!(executar(&a, &p, "gerar_imagem", &json!({ "descricao": "outro farol" })).unwrap().0.starts_with("Já estou criando"));
        assert_eq!(*fila.borrow(), ["trabalho_profundo", "estudio_3d", "gerar_imagem"]);
        let (mut cheio, fila) = com_fila(&d);
        cheio.uso_claude = Box::new(|| 20);
        assert!(executar(&cheio, &p, "delegar_ao_claude", &json!({ "tarefa": "arruma" })).unwrap().0.starts_with("teto diario"));
        assert!(fila.borrow().is_empty());
    }

    #[test]
    fn rodar_chama_os_modulos_e_a_entrega_vai_para_a_caixa_de_saida() {
        let (d, c) = (pasta("longas-rodar"), ctx());
        let p = ped("pc", "jarvis", "x", false, &c, &[]);
        let a = Ambiente {
            agente: Box::new(|prompt| { assert!(prompt.contains("arruma a planilha")); Ok((0, "feito".into())) }),
            jarvis: Box::new(|_| Ok("etapa feita".into())),
            ia: Box::new(|_, _| Ok("resposta".into())),
            ..amb(&d)
        };
        assert_eq!(rodar(&a, &p, "delegar_ao_claude", &json!({ "tarefa": "arruma a planilha" })), "feito");
        assert_eq!(a.saida_direta.borrow()[0], "🤖 Claude terminou:\n\nfeito");
        // modo profundo: o resultado vai ao dono (texto e audio) pela closure de envio, que aqui so guarda
        a.profundo.comecar("comparar fornecedores").unwrap();
        rodar(&a, &p, "trabalho_profundo", &json!({ "objetivo": "comparar fornecedores" }));
        let s = saida(&d);
        assert!(s.iter().any(|l| l["canal"] == "pedido" && l["destino"] == "Eu(SP)") && s.iter().any(|l| l["canal"] == "pedido-audio"), "{s:?}");
        assert!(a.profundo.comecar("outro").is_ok()); // a vez foi solta
        // automelhoria sem git ligado nao aplica nada; imagem e atendimento ainda nao tem motor
        let seguro = amb(&d);
        assert!(!rodar(&seguro, &p, "melhorar_braniac", &json!({ "pedido": "responda mais curto sempre" })).starts_with("✅"));
        assert!(rodar(&seguro, &p, "gerar_imagem", &json!({})).contains("ainda não foi ligada") && rodar(&seguro, &p, "whatsapp_atender", &json!({})).contains("ainda não foi ligada"));
        // video: sem IA nao ha roteiro, a vez e solta e o dono fica sabendo
        trava(&seguro.estudio).rodando = true;
        assert!(rodar(&seguro, &p, "criar_video", &json!({ "descricao": "como funciona um farol" })).starts_with("O vídeo não saiu"));
        assert!(!trava(&seguro.estudio).rodando && seguro.saida_direta.borrow().last().unwrap().starts_with("Não consegui fazer o vídeo"));
    }
}
