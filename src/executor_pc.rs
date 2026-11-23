//! Executor, grupo PC: pc, usar_skill, criar_habilidade, consultar_lei, leads_maps, animacao, temperatura_notebook, organizar_sessoes,
//! artcraft (so as travas de entrada/saida; o aplicativo e o processo "artcraft"), ver_imagem_arquivo, melhorar_alma,
//! desfazer_melhoria_alma e desfazer_automelhoria. As travas ja passaram em `executor::executar`.
#![allow(dead_code)]
use crate::executor::{arg, com_automelhoria, mostrar, Ambiente, NAO_LIGADO};
use crate::{alma_evolucao, animacoes, artcraft, automelhoria, b2, imagem, leads_maps, leis, organizar_sessoes, pc, sala, skills, termico, wpp_arquivo};
use serde_json::Value;

/// Acoes do ArtCraft em que `entrada` e texto, nao arquivo.
const ARTCRAFT_TEXTO: [&str; 2] = ["cartaz", "word_criar"];

pub fn executar(amb: &Ambiente, nome: &str, args: &Value) -> Option<(String, bool)> {
    let a = |k: &str| arg(args, k);
    let (d, agora) = (amb.dados.as_path(), &amb.agora);
    let l = &amb.locais;
    let local = |t: String| Some((t, true));
    let publico = |t: String| Some((t, false));
    match nome {
        // nomes de arquivo do PC do dono: resultado local
        "pc" => local(pc::executar(a("acao"), a("alvo"), &l.casa, &|u| (amb.abrir)(u), &|c| (amb.abrir)(&c.to_string_lossy()))),
        "usar_skill" => publico(skills::usar(a("nome"), a("arquivo"), &l.skills)),
        "criar_habilidade" => publico(match b2::criar_skill(&l.skills, a("nome"), a("descricao"), a("passos"), a("ferramentas"), &agora.data.iso()) {
            Ok(s) => format!("Habilidade criada: {s}"),
            Err(e) => format!("Nao criei: {e}"),
        }),
        "consultar_lei" => {
            let a_partir = args["a_partir"].as_u64().unwrap_or(0) as usize;
            let numero = args["numero"].as_str().map(str::to_string).or_else(|| args["numero"].as_i64().map(|n| n.to_string())).unwrap_or_default();
            let ano = args["ano"].as_str().map(str::to_string).or_else(|| args["ano"].as_i64().map(|n| n.to_string())).unwrap_or_default();
            publico(if !numero.trim().is_empty() {
                leis::planalto(a("tipo"), &numero, &ano, 6000, a_partir, &*amb.baixar)
            } else if !a("id").is_empty() {
                leis::ler(&l.leis, a("id"), 6000, a_partir)
            } else {
                leis::buscar(&l.leis_db, a("termo"), 8)
            })
        }
        "leads_maps" => {
            let r = leads_maps::buscar(a("tipo"), a("cidade"), args["limite"].as_i64().unwrap_or(20), d, &l.kit_leads, &l.leads, agora, &|argv| (amb.processo)("leads_maps", argv).map(drop));
            publico(match r {
                Ok((csv, linhas)) => {
                    let campo = |x: &serde_json::Map<String, Value>, k: &str| x.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                    let res: Vec<String> = linhas.iter().take(10).map(|x| format!("{} {}", campo(x, "title"), campo(x, "phone")).trim().to_string()).collect();
                    format!("Achei {} empresas (rascunho para o dono decidir, não contatei ninguém). CSV: {}. Primeiras: {}", linhas.len(), csv.display(), res.join("; "))
                }
                Err(e) => format!("Não consegui a lista: {e}"),
            })
        }
        "animacao" => publico(match animacoes::gerar(d, &l.animacoes, agora, a("tipo"), &args["spec"], amb.animacoes_instaladas, &|skill, arq| (amb.render)(skill, arq)) {
            Ok(arq) => format!("Animação pronta (rascunho, não publiquei): {}", arq.display()),
            Err(e) => format!("Não consegui gerar a animação: {e}"),
        }),
        "temperatura_notebook" => {
            let (gpu, cpu) = ((amb.processo)("termico_gpu", &[]), (amb.processo)("termico_cpu", &[]));
            publico(if gpu.is_err() && cpu.is_err() {
                format!("A leitura de temperatura {NAO_LIGADO} no Braniac em Rust; não medi nada.")
            } else {
                // so o que foi medido de verdade: uso de CPU/memoria nao tem leitor no Rust, entao nao aparece (zero inventado nao e dado)
                let grau = |v: Option<f64>, o_que: &str| v.filter(|x| *x != 0.0).map_or(format!("{o_que} sem leitura"), |x| format!("{o_que} {x:.0} graus"));
                format!("{}, {}. Uso de processador e memória: não medido aqui.",
                    grau(gpu.ok().and_then(|s| termico::parse_gpu(&s)), "placa de vídeo"), grau(cpu.ok().and_then(|s| termico::parse_cpu(&s)), "processador"))
            })
        }
        "organizar_sessoes" => {
            let db = d.join("b2.db");
            publico(if !db.exists() {
                "Nada a organizar: ainda não há conversas guardadas.".into()
            } else {
                rusqlite::Connection::open(&db).map_err(|e| e.to_string()).and_then(|c| organizar_sessoes::resumo(&c, &organizar_sessoes::REGRAS_GERAIS, true)).unwrap_or_else(|e| format!("Não consegui organizar: {e}"))
            })
        }
        "artcraft" => publico(artcraft_travas(amb, a("acao"), a("entrada"), a("saida"), a("extra")).unwrap_or_else(|e| format!("não consegui: {e}"))),
        "ver_imagem_arquivo" => {
            let r = wpp_arquivo::resolver_caminho(a("arquivo"), &amb.pastas)
                .and_then(|c| std::fs::read(&c).map_err(|e| e.to_string()))
                // imagem do PC do dono: so a visao LOCAL
                .and_then(|b| imagem::descrever(&b, Some(a("pergunta")).filter(|p| !p.is_empty()), true, &*amb.ver_imagem, &|_, _| Err("visão online não é usada para arquivo do PC".into())));
            local(r.unwrap_or_else(|e| format!("não consegui ler a imagem: {}", e.chars().take(150).collect::<String>())))
        }
        "melhorar_alma" => {
            // IA LOCAL e sem o contexto do Jarvis: nada vaza para a persona da A.L.M.A.
            let msg = match alma_evolucao::melhorar(d, agora, a("pedido"), &|p| (amb.ia_local)(p).ok(), "Jarvis") {
                Ok(v) => {
                    sala::registrar(d, agora, "sistema", &format!("A A.L.M.A. foi melhorada pelo Jarvis (versão {v}): {}. Dá para desfazer.", a("pedido").chars().take(120).collect::<String>()), "aviso");
                    format!("✅ Melhorei a A.L.M.A. (versão {v}). Texto novo dos ajustes:\n\n{}\n\nSe não gostar, é só dizer «desfaz a melhoria da A.L.M.A.».", alma_evolucao::ler_ajustes(d))
                }
                Err(e) => format!("🚫 Não apliquei: {e}"),
            };
            Some((mostrar(amb, msg, "(resultado mostrado ao dono)").0, false))
        }
        "desfazer_melhoria_alma" => {
            let msg = match alma_evolucao::reverter(d, agora) {
                Ok(v) => {
                    sala::registrar(d, agora, "sistema", &format!("A melhoria v{v} da A.L.M.A. foi desfeita."), "aviso");
                    format!("↩️ Desfiz a melhoria v{v} da A.L.M.A. Voltou ao ajuste anterior.")
                }
                Err(e) => format!("🚫 Não apliquei: {e}"),
            };
            Some((mostrar(amb, msg, "(resultado mostrado ao dono)").0, false))
        }
        "desfazer_automelhoria" => publico(com_automelhoria(amb, |a| automelhoria::desfazer_ultima(d, a))),
        _ => None,
    }
}

/// Entrada tem de existir fora de local protegido; a saida fica sempre na pasta de trabalho (o original nunca muda). So depois o aplicativo roda.
fn artcraft_travas(amb: &Ambiente, acao: &str, entrada: &str, saida: &str, extra: &str) -> Result<String, String> {
    if acao.is_empty() || entrada.is_empty() {
        return Err("faltou a ação ou a entrada".into());
    }
    let mut argv = vec![acao.to_string()];
    let mut ext = "txt".to_string();
    if ARTCRAFT_TEXTO.contains(&acao) {
        argv.push(entrada.to_string());
    } else {
        for item in artcraft::lista(entrada) {
            let real = artcraft::entrada(&item)?;
            ext = real.extension().map_or(ext, |e| e.to_string_lossy().to_lowercase());
            argv.push(real.to_string_lossy().to_string());
        }
    }
    let alvo = artcraft::saida(&amb.locais.trabalho, if saida.is_empty() { acao } else { saida }, &ext);
    argv.extend([alvo.to_string_lossy().to_string(), extra.to_string()]);
    (amb.processo)("artcraft", &argv).map(|_| artcraft::pronto(&alvo))
}

#[cfg(test)]
mod tests {
    use crate::executor::tests::{amb, ctx, pasta, ped, saida};
    use crate::executor::{executar, Ambiente, NAO_LIGADO};
    use crate::jarvis_travas::{BLOQUEADO, DONO_CONV};
    use serde_json::json;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn de_fora_sem_autonomia_o_pc_fica_fechado() {
        let (d, c) = (pasta("pc-travas"), ctx());
        let abertos = Rc::new(RefCell::new(Vec::new()));
        let ab = abertos.clone();
        let a = Ambiente { abrir: Box::new(move |x| { ab.borrow_mut().push(x.to_string()); Ok(()) }), ..amb(&d) };
        let fora = ped("telegram", "telegram", "abre o site", false, &c, &[]);
        for n in ["pc", "criar_habilidade", "melhorar_alma", "desfazer_melhoria_alma", "desfazer_automelhoria"] {
            assert_eq!(executar(&a, &fora, n, &json!({ "acao": "abrir", "alvo": "https://exemplo.com" })).unwrap().0, BLOQUEADO, "{n}");
        }
        // ArtCraft: o texto da trava volta para a IA repassar
        assert!(executar(&a, &fora, "artcraft", &json!({ "acao": "pdf_juntar", "entrada": "a.pdf" })).unwrap().0.starts_with("🔒"));
        assert!(abertos.borrow().is_empty());
        // chat verificado do dono: 'pc' passa sem autonomia; com a autonomia armada tambem
        let dono = ped("whatsapp", DONO_CONV, "abre o site", false, &c, &[]);
        let (r, local) = executar(&a, &dono, "pc", &json!({ "acao": "abrir", "alvo": "https://exemplo.com" })).unwrap();
        assert!(local && *abertos.borrow() == ["https://exemplo.com"], "{r}");
        // so tres acoes existem: nada de executar ou apagar
        assert!(executar(&a, &dono, "pc", &json!({ "acao": "apagar", "alvo": "x" })).unwrap().0.starts_with("acao desconhecida"));
    }

    #[test]
    fn caminho_feliz_das_ferramentas_locais() {
        let (d, c) = (pasta("pc-feliz"), ctx());
        let dados = d.join("dados");
        std::fs::create_dir_all(&dados).unwrap();
        let baixados = Rc::new(RefCell::new(Vec::new()));
        let b2 = baixados.clone();
        let a = Ambiente { baixar: Box::new(move |u| { b2.borrow_mut().push(u.to_string()); Err("fora do ar".into()) }), ..amb(&dados) };
        let p = ped("pc", "jarvis", "x", false, &c, &[]);
        let ex = |n: &str, v: serde_json::Value| executar(&a, &p, n, &v).unwrap().0;
        // habilidade nova vira skill e ja pode ser usada
        assert_eq!(ex("criar_habilidade", json!({ "nome": "Fechar o caixa", "descricao": "quando fechar o dia", "passos": "1. somar\n2. conferir", "ferramentas": "agenda consultar_banco" })), "Habilidade criada: fechar-o-caixa");
        assert!(ex("criar_habilidade", json!({ "nome": "Fechar o caixa", "descricao": "x", "passos": "y" })).starts_with("Nao criei"));
        assert!(ex("usar_skill", json!({ "nome": "fechar-o-caixa" })).contains("somar"));
        assert!(ex("usar_skill", json!({ "nome": "nao-existe" })).contains("não existe"));
        // leis: indice local; texto oficial pela closure de baixar
        assert!(ex("consultar_lei", json!({ "termo": "inquilinato" })).contains("indice de leis ainda nao foi montado"));
        assert_eq!(ex("consultar_lei", json!({ "id": "../segredo" })), "id invalido");
        ex("consultar_lei", json!({ "tipo": "lei", "numero": "8245", "ano": "1991" }));
        assert!(baixados.borrow().iter().all(|u| u.contains("planalto")) && !baixados.borrow().is_empty());
        // o que depende de processo diz que nao esta ligado, sem fingir
        assert!(ex("temperatura_notebook", json!({})).contains(NAO_LIGADO));
        assert!(ex("leads_maps", json!({ "tipo": "padarias", "cidade": "Cidade Exemplo, UF" })).starts_with("Não consegui a lista"));
        assert!(ex("animacao", json!({ "tipo": "graph", "spec": { "label": "x" } })).starts_with("Não consegui gerar a animação"));
        assert!(ex("organizar_sessoes", json!({})).starts_with("Nada a organizar"));
        assert!(ex("desfazer_automelhoria", json!({})).starts_with("Não achei nenhuma automelhoria"));
        assert!(saida(&dados).is_empty());
    }

    #[test]
    fn temperatura_le_as_saidas_dos_sensores() {
        let (d, c) = (pasta("pc-termico"), ctx());
        let a = Ambiente { processo: Box::new(|n, _| if n == "termico_gpu" { Ok("67\n".into()) } else { Err("sem sensor".into()) }), ..amb(&d) };
        let r = executar(&a, &ped("pc", "jarvis", "x", false, &c, &[]), "temperatura_notebook", &json!({})).unwrap().0;
        assert!(r.contains("placa de vídeo 67 graus") && r.contains("processador sem leitura") && r.contains("não medido") && !r.contains("0%"), "{r}");
    }

    #[test]
    fn artcraft_so_roda_com_entrada_e_saida_conferidas() {
        let (d, c) = (pasta("pc-artcraft"), ctx());
        std::fs::write(d.join("foto.png"), b"png").unwrap();
        std::fs::write(d.join(".env"), b"CHAVE=1").unwrap();
        let rodou = Rc::new(RefCell::new(Vec::<Vec<String>>::new()));
        let r2 = rodou.clone();
        let mut a = Ambiente { processo: Box::new(move |n, argv| { assert_eq!(n, "artcraft"); r2.borrow_mut().push(argv.to_vec()); Ok(String::new()) }), ..amb(&d) };
        a.locais.trabalho = d.join("trabalho");
        let p = ped("pc", "jarvis", "x", false, &c, &[]);
        let ex = |v: serde_json::Value| executar(&a, &p, "artcraft", &v).unwrap().0;
        assert!(ex(json!({ "acao": "foto_pb", "entrada": d.join(".env").to_string_lossy() })).contains("protegido"));
        assert!(ex(json!({ "acao": "foto_pb", "entrada": d.join("nao-existe.png").to_string_lossy() })).starts_with("não consegui: Não achei o arquivo"));
        assert!(rodou.borrow().is_empty());
        let r = ex(json!({ "acao": "foto_pb", "entrada": d.join("foto.png").to_string_lossy(), "saida": "..\\..\\fora.png" }));
        assert!(r.starts_with("Pronto.") && r.contains("o original não foi alterado"), "{r}");
        let argv = rodou.borrow()[0].clone();
        // a saida ficou dentro da pasta de trabalho, mesmo com ".." no nome
        assert!(argv[0] == "foto_pb" && std::path::Path::new(&argv[2]).starts_with(d.join("trabalho")), "{argv:?}");
        // ambiente seguro: o aplicativo nao esta ligado
        let seguro = amb(&d);
        assert!(executar(&seguro, &p, "artcraft", &json!({ "acao": "cartaz", "entrada": "Festa junina" })).unwrap().0.contains(NAO_LIGADO));
    }

    #[test]
    fn alma_melhora_com_ia_local_e_imagem_do_pc_so_na_visao_local() {
        let (d, c) = (pasta("pc-alma"), ctx());
        let dados = d.join("dados");
        let fotos = dados.join("imagens"); // subpasta liberada da pasta protegida (a temporaria fica debaixo de AppData)
        std::fs::create_dir_all(&fotos).unwrap();
        std::fs::write(fotos.join("recibo.png"), [0x89, b'P', b'N', b'G', 1, 2, 3]).unwrap();
        let mut a = Ambiente { ia_local: Box::new(|_| Ok("Fale em frases curtas e pergunte como foi o dia.".into())), ver_imagem: Box::new(|_, inst| Ok(format!("um recibo ({inst})"))), ..amb(&dados) };
        a.pastas = crate::wpp_arquivo::Pastas { atalhos: vec![], raizes: vec![fotos.clone()], protegidas: vec![dados.clone()] };
        let p = ped("pc", "jarvis", "x", false, &c, &[]);
        let (nota, _) = executar(&a, &p, "melhorar_alma", &json!({ "pedido": "fale mais curto" })).unwrap();
        assert_eq!(nota, "(resultado mostrado ao dono)");
        assert!(a.saida_direta.borrow()[0].starts_with("✅ Melhorei a A.L.M.A. (versão 1)"), "{:?}", a.saida_direta.borrow());
        executar(&a, &p, "desfazer_melhoria_alma", &json!({})).unwrap();
        assert!(a.saida_direta.borrow()[1].starts_with("↩️"));
        // sem IA local nada muda
        let seguro = amb(&dados);
        executar(&seguro, &p, "melhorar_alma", &json!({ "pedido": "fale mais curto" })).unwrap();
        assert!(seguro.saida_direta.borrow()[0].starts_with("🚫"));
        let (r, local) = executar(&a, &p, "ver_imagem_arquivo", &json!({ "arquivo": fotos.join("recibo.png").to_string_lossy(), "pergunta": "qual o valor?" })).unwrap();
        assert!(local && r.contains("um recibo (qual o valor?)"), "{r}");
        assert!(executar(&a, &p, "ver_imagem_arquivo", &json!({ "arquivo": "nao-existe-xyz.png" })).unwrap().0.starts_with("não consegui ler a imagem"));
    }
}
