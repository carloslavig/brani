//! Executor, grupo EMPRESAS: nexodev, marketing, investimentos, compras, cortes_video e esteira_magno (as travas ja passaram em
//! `executor::executar`). Aprovar/depositar/fechar levam `pedido_do_dono` ao modulo, que recusa de novo se nao for.
//! "Não ligado ainda" aqui: build/commit/merge do NexoDev, gerar/medir/subir da Fornalha, baixar/transcrever/montar da Claquete,
//! o checklist compartilhado da esteira e a acao 'capitais' (a lista de capitais nao esta no Rust).
#![allow(dead_code)]
use crate::executor::{arg, corta, do_dono, Ambiente, NAO_LIGADO};
use crate::jarvis_travas::Pedido;
use crate::{barramento, empresa_compras, empresa_cortes, empresa_dev, empresa_inv, empresa_mkt, link_concluidos, link_lote_magno, painel_rede};
use serde_json::{json, Value};

const FILA_VAZIA: &str = "Nada na fila (ou teto do dia atingido).";

fn nl<T>() -> Result<T, String> {
    Err(NAO_LIGADO.into())
}

fn fila(l: Vec<String>) -> String {
    if l.is_empty() { FILA_VAZIA.into() } else { l.join("\n") }
}

fn ou<T>(r: Result<(T, String), String>) -> String {
    r.map_or_else(|e| e, |(_, m)| m)
}

pub fn executar(amb: &Ambiente, p: &Pedido, nome: &str, args: &Value) -> Option<String> {
    let a = |k: &str| arg(args, k);
    let n = |k: &str| args[k].as_f64().unwrap_or(0.0);
    let (d, agora, ts, acao) = (amb.dados.as_path(), &amb.agora, amb.ts, a("acao"));
    let ignorar = args["ignorar_teto"] == true;
    // "passos" (so a tela usa): no maximo esse tanto, nunca mais que o padrao da ferramenta
    let passos = |padrao: i64| args["passos"].as_i64().filter(|n| *n >= 1).map_or(padrao, |n| n.min(padrao));
    Some(match nome {
        "nexodev" => match acao {
            "abrir" => ou(empresa_dev::abrir(d, if a("projeto").is_empty() { "geral" } else { a("projeto") }, a("titulo"), a("descricao"), a("area"), args["critico"] == true, ts)),
            "aprovar" => empresa_dev::aprovar(d, a("id"), do_dono(p), ts, &|t| (amb.processo)("nexodev_mesclar", &[t["id"].as_str().unwrap_or("").to_string()]).is_ok()),
            "trabalhar" => {
                let f = empresa_dev::Ferramentas {
                    gerar: &*amb.ia, ia: &*amb.ia, contexto: &|_| String::new(), commit: &|_, _, _, _| false, build: &|_| (None, String::new()),
                    senior: &|t, tarefa| (amb.segundo_plano)("nexodev_senior", &json!({ "id": t["id"], "tarefa": tarefa })).map(drop),
                    recado: &|de, para, tipo, txt| drop(barramento::enviar(d, de, para, tipo, txt, 0, None)),
                };
                fila(empresa_dev::trabalhar(d, &f, agora, ts, Some(passos(if ignorar { 6 } else { 2 })), 1, ignorar))
            }
            _ => empresa_dev::quadro(d, agora),
        },
        "marketing" => {
            let f = empresa_mkt::Oficina { ia: &*amb.ia, descrever: &|_| String::new(), gerar: &|_, _, _| nl(), acabar: &|_, _| nl(), medir: &|_| nl(), sondar: &|_| None, subir: &|_, _| nl() };
            match acao {
                "abrir" => {
                    let mut foto = String::new();
                    if !a("contato").is_empty() {
                        // a Ponte: a FOTO real vem da conversa de onde o pedido saiu
                        let pasta = d.join("empresa_mkt").join("recebidos");
                        let arq = pasta.join(format!("{}.jpg", agora.carimbo().replace('-', "_")));
                        let r = (amb.wpp_foto)(a("contato")).and_then(|b| std::fs::create_dir_all(&pasta).and_then(|_| std::fs::write(&arq, b)).map_err(|e| e.to_string()));
                        if let Err(e) = r {
                            return Some(format!("Não consegui pegar a foto do WhatsApp: {e}"));
                        }
                        foto = arq.to_string_lossy().to_string();
                    }
                    ou(empresa_mkt::abrir(d, a("cliente"), a("tipo"), a("briefing"), &foto, ts))
                }
                "planejar" => {
                    let l = empresa_mkt::planejar(d, &f, agora, ts);
                    format!("Pauta aberta: {}", if l.is_empty() { "já havia pauta hoje.".to_string() } else { l.join(", ") })
                }
                "trabalhar" => fila(empresa_mkt::trabalhar(d, &f, agora, ts, passos(if ignorar { 3 } else { 1 }) as usize, ignorar)),
                _ => empresa_mkt::quadro(d, agora, ts),
            }
        }
        "investimentos" => match acao {
            "depositar" if !do_dono(p) => "Só o dono mexe na carteira simulada.".into(),
            "fechar" if !do_dono(p) => "Só o dono mexe na carteira simulada.".into(),
            "depositar" => empresa_inv::depositar(d, a("perfil"), n("valor")),
            "propor" => empresa_inv::propor(d, a("perfil"), a("ticker"), n("entrada"), n("stop"), n("alvo"), a("tese"), agora, ts),
            "aprovar" => empresa_inv::aprovar(d, a("id"), do_dono(p), agora, ts),
            "fechar" => empresa_inv::fechar(d, a("id"), n("preco"), agora, ts),
            _ => empresa_inv::quadro(d),
        },
        "compras" => match acao {
            "abrir" => ou(empresa_compras::abrir(d, a("item"), a("uso"), a("orcamento"), if p.origem == "whatsapp" { "whatsapp" } else { "jarvis" }, ts)),
            "trabalhar" => {
                // so le: nao existe closure de compra
                let f = empresa_compras::Pesquisa { ia: &*amb.ia, pesquisar: &*amb.pesquisar, pagina: &*amb.pagina };
                fila(empresa_compras::trabalhar(d, &f, agora, ts, passos(3) as usize, false))
            }
            "resultado" => empresa_compras::resultado(d, a("id")),
            "responder" => empresa_compras::responder(d, a("id"), a("texto")),
            _ => empresa_compras::quadro(d, agora),
        },
        "cortes_video" => match acao {
            "aprovar" => empresa_cortes::aprovar(d, a("id"), do_dono(p), agora, ts),
            "confirmar_locutor" => empresa_cortes::confirmar_locutor(d, a("id"), do_dono(p), ts),
            "rejeitar" if !do_dono(p) => "Só o dono rejeita um corte.".into(),
            "rejeitar" => empresa_cortes::rejeitar(d, a("id"), ts),
            "abrir" => ou(empresa_cortes::abrir(d, a("fonte"), a("autor"), a("titulo_fonte"), a("gameplay"), false, agora, ts)),
            "trabalhar" => {
                let f = empresa_cortes::Estudio { ia: &*amb.ia, baixar: &|_, _| nl(), segmentos: &|_| nl(), montar: &|_, _, _, _, _| nl(), queimar: &|_, _, _| nl(), sonda: &|_| nl(), buscar: &|_| vec![] };
                fila(empresa_cortes::trabalhar(d, &f, agora, ts, passos(2) as usize, false))
            }
            _ => empresa_cortes::quadro(d),
        },
        "esteira_magno" => esteira(amb, p, args),
        _ => return None,
    })
}

/// Sem gate (o dono comanda a esteira de qualquer canal); 'refazer' ja passou pela trava de `trava_ferramenta`. Nao envia nada nem apaga video.
fn esteira(amb: &Ambiente, p: &Pedido, args: &Value) -> String {
    let (d, agora, ts) = (amb.dados.as_path(), &amb.agora, amb.ts);
    let religar = || drop((amb.processo)("esteira_religar", &[]));
    let matar = || (amb.processo)("esteira_matar", &[]).ok().and_then(|n| n.trim().parse().ok()).unwrap_or(0);
    let r = match arg(args, "acao") {
        // o endereco so sai quando o dono pede
        "painel" => Ok(painel_rede::endereco(&amb.locais.painel_fixo, d)),
        "capitais" => Ok(format!("A esteira das capitais {NAO_LIGADO} no Braniac em Rust (falta a lista de capitais). Nada foi feito.")),
        "refazer" => {
            let n = args["numero"].as_str().map(str::to_string).or_else(|| args["numero"].as_i64().map(|x| x.to_string())).unwrap_or_default();
            let n = n.trim();
            if n.is_empty() || !n.chars().all(|c| c.is_ascii_digit()) {
                return "Qual o número do vídeo para refazer?".into();
            }
            let fora = link_lote_magno::Fora { checklist: &|_, _, _| {}, barramento: &|de, para, tipo, txt| drop(barramento::enviar(d, de, para, tipo, txt, 0, None)) };
            link_concluidos::pedir_refazer(d, n, &format!("dono ({})", p.origem), &corta(p.original, 200), agora)
                .and_then(|n| link_lote_magno::resolver(d, &n, "pedido do dono para refazer", ts, agora, &fora).map(|_| n))
                .map(|n| format!("Pedido registrado: o vídeo {n} volta para a esteira como o próximo, mesmo já estando no site."))
        }
        "mt" if arg(args, "cidade").is_empty() => Ok("Qual cidade de Mato Grosso?".into()),
        "mt" => link_lote_magno::mt(d, arg(args, "cidade"), &religar, ts, agora),
        acao => link_lote_magno::esteira(d, if acao.is_empty() { "status" } else { acao }, args["quantos"].as_i64().unwrap_or(50), &matar, &religar, ts, agora),
    };
    r.unwrap_or_else(|e| format!("Não consegui na esteira: {e}"))
}

#[cfg(test)]
mod tests {
    use crate::executor::tests::{amb, ctx, pasta, ped, saida};
    use crate::executor::{executar, Ambiente};
    use crate::jarvis_travas::{BLOQUEADO, DONO_CONV};
    use serde_json::json;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn aprovar_e_mexer_na_carteira_so_com_pedido_do_dono() {
        let (d, c) = (pasta("emp-travas"), ctx());
        let a = amb(&d);
        let fora = ped("whatsapp", "jarvis", "aprova o T001", false, &c, &[]);
        for (n, acao) in [("nexodev", "aprovar"), ("investimentos", "aprovar"), ("investimentos", "depositar"), ("investimentos", "fechar"),
                          ("cortes_video", "aprovar"), ("cortes_video", "rejeitar"), ("cortes_video", "confirmar_locutor")] {
            assert_eq!(executar(&a, &fora, n, &json!({ "acao": acao, "id": "T001", "perfil": "swing", "valor": 100 })).unwrap().0, BLOQUEADO, "{n} {acao}");
        }
        assert_eq!(a.saida_direta.borrow().len(), 7);
        assert!(!d.join("empresa_inv").exists() && saida(&d).is_empty());
        // modulo desligado nem chega na acao
        let off = Ambiente { modulo_ativo: Box::new(|m| m != "compras"), ..amb(&d) };
        let pc = ped("pc", "jarvis", "x", false, &c, &[]);
        assert!(executar(&off, &pc, "compras", &json!({ "acao": "quadro" })).unwrap().0.contains("Pechincha") );
        // ler o quadro de fora e livre
        assert!(!executar(&a, &fora, "nexodev", &json!({ "acao": "quadro" })).unwrap().0.starts_with('('));
    }

    #[test]
    fn caminho_feliz_chama_cada_empresa() {
        let (d, c) = (pasta("emp-feliz"), ctx());
        let a = amb(&d);
        let pc = ped("pc", "jarvis", "abre um ticket", false, &c, &[]);
        let ex = |n: &str, v: serde_json::Value| executar(&a, &pc, n, &v).unwrap().0;
        // NexoDev
        let r = ex("nexodev", json!({ "acao": "abrir", "projeto": "Loja", "titulo": "Tela de estoque", "descricao": "lista de itens" }));
        assert!(r.starts_with("T001 aberto em loja"), "{r}");
        assert!(ex("nexodev", json!({ "acao": "abrir", "titulo": "Guardar a senha do banco", "descricao": "x" })).starts_with("Ticket recusado"));
        assert!(ex("nexodev", json!({ "acao": "quadro" })).contains("T001"));
        assert!(ex("nexodev", json!({ "acao": "aprovar", "id": "T001" })).contains("ainda não está pronto"));
        assert!(ex("nexodev", json!({ "acao": "aprovar", "id": "T999" })).contains("não encontrado"));
        // Quantix: so papel
        assert!(!ex("investimentos", json!({ "acao": "depositar", "perfil": "swing", "valor": 1000 })).contains("inválido"));
        assert!(ex("investimentos", json!({ "acao": "depositar", "perfil": "poupanca", "valor": 10 })).contains("inválido"));
        assert!(!ex("investimentos", json!({ "acao": "quadro" })).is_empty());
        // Pechincha
        assert!(ex("compras", json!({ "acao": "abrir", "item": "furadeira de impacto", "uso": "parede de tijolo" })).contains("C001"));
        assert!(ex("compras", json!({ "acao": "abrir", "item": "x" })).starts_with("O que você quer comprar"));
        assert!(ex("compras", json!({ "acao": "quadro" })).contains("C001"));
        // Claquete: sem autor nao abre
        assert!(ex("cortes_video", json!({ "acao": "abrir", "fonte": "video.mp4" })).starts_with("Sem autor"));
        assert!(!ex("cortes_video", json!({ "acao": "quadro" })).is_empty());
        // Fornalha: a foto do WhatsApp nao esta ligada no ambiente seguro; sem contato o pedido abre
        assert!(ex("marketing", json!({ "acao": "abrir", "cliente": "Padaria", "tipo": "imagem", "briefing": "pão quentinho", "contato": "Padaria Central" })).starts_with("Não consegui pegar a foto"));
        assert!(ex("marketing", json!({ "acao": "abrir", "cliente": "Padaria", "tipo": "filme", "briefing": "x" })).starts_with("Tipo inválido"));
        assert!(!ex("marketing", json!({ "acao": "quadro" })).is_empty());
        assert!(saida(&d).is_empty()); // nenhuma empresa envia nada
    }

    #[test]
    fn nexodev_trabalha_com_a_ia_do_ambiente() {
        let (d, c) = (pasta("emp-ia"), ctx());
        let visto = Rc::new(RefCell::new(0));
        let v2 = visto.clone();
        let a = Ambiente { ia: Box::new(move |_, _| { *v2.borrow_mut() += 1; Ok("resumo\n=== ARQUIVO: src/main.rs ===\n```rust\nfn main() {}\n```\n=== FIM ===".into()) }), ..amb(&d) };
        let pc = ped("pc", "jarvis", "x", false, &c, &[]);
        executar(&a, &pc, "nexodev", &json!({ "acao": "abrir", "projeto": "loja", "titulo": "Tela de estoque", "descricao": "lista" })).unwrap();
        let r = executar(&a, &pc, "nexodev", &json!({ "acao": "trabalhar" })).unwrap().0;
        assert!(*visto.borrow() > 0 && r.contains("T001"), "{r}");
    }

    #[test]
    fn esteira_comanda_sem_enviar_e_refazer_so_do_dono() {
        let (d, c) = (pasta("emp-esteira"), ctx());
        let chamados = Rc::new(RefCell::new(Vec::new()));
        let ch = chamados.clone();
        let a = Ambiente { processo: Box::new(move |n, _| { ch.borrow_mut().push(n.to_string()); Ok("1".into()) }), ..amb(&d) };
        let fora = ped("whatsapp", "jarvis", "para a esteira", false, &c, &[]);
        // o dono comanda de qualquer canal
        assert!(executar(&a, &fora, "esteira_magno", &json!({ "acao": "status" })).unwrap().0.starts_with("Esteira"));
        let r = executar(&a, &fora, "esteira_magno", &json!({ "acao": "parar" })).unwrap().0;
        assert!(r.starts_with("⏸") && r.contains("volta para a fila") && *chamados.borrow() == ["esteira_matar"]);
        assert!(executar(&a, &fora, "esteira_magno", &json!({ "acao": "painel" })).unwrap().0.contains("ainda não tem endereço"));
        assert_eq!(executar(&a, &fora, "esteira_magno", &json!({ "acao": "mt" })).unwrap().0, "Qual cidade de Mato Grosso?");
        assert!(executar(&a, &fora, "esteira_magno", &json!({ "acao": "capitais" })).unwrap().0.contains("não ligado ainda"));
        // refazer video publicado: de fora, so pelo chat verificado do dono
        assert!(executar(&a, &fora, "esteira_magno", &json!({ "acao": "refazer", "numero": "170" })).unwrap().0.starts_with("(refazer"));
        assert!(!crate::link_concluidos::pedido_humano(&d, "170"));
        let dono = ped("whatsapp", DONO_CONV, "refaz o 170", false, &c, &[]);
        assert_eq!(executar(&a, &dono, "esteira_magno", &json!({ "acao": "refazer", "numero": "abc" })).unwrap().0, "Qual o número do vídeo para refazer?");
        let r = executar(&a, &dono, "esteira_magno", &json!({ "acao": "refazer", "numero": "170" })).unwrap().0;
        assert!(r.starts_with("Pedido registrado: o vídeo 170"), "{r}");
        assert!(crate::link_concluidos::pedido_humano(&d, "170") && saida(&d).is_empty());
    }
}
