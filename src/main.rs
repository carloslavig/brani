//! Braniac em Rust: roda em paralelo ao Braniac atual (Python, porta 8765), sem tocar nele.
//! Codigo e padrao/ sao iguais em todo PC; tudo que e pessoal mora so na pasta de dados.
#![cfg_attr(not(test), windows_subsystem = "windows")]
mod abas;
mod oculto;
mod abas_fixas;
mod reconcilia_abas;
mod agenda;
mod agentes;
mod avisos;
mod banco;
mod despacho;
mod importar_antigo;
mod barramento;
mod bomdia;
mod cdp;
mod base64;
mod claude_cli;
mod ponte_agentes;
mod codigo;
mod cofre;
mod cuidado_diario;
mod compasso;
mod conselho;
mod pc_metricas;
mod consulta;
mod contradiz;
mod delegacao;
mod fatos;
mod ferramentas;
mod modulos;
mod ouvir;
mod aparelho;
mod app_painel;
mod saudacao;
mod para_fala;
mod parecer;
mod poda;
mod provedores;
mod roteador;
mod segredos;
mod store;
mod tempo;
mod texto_ferramenta;
mod texto_pt;
mod travas;
mod travas_pagina;
mod vm;
mod voz;
mod wav;
mod whatsapp;
mod funil;
mod latencia;
mod limpeza;
mod mudo;
mod pendencias;
mod cerebro;
mod projetos;
mod projetos_eventos;
mod power;
mod qualidade;
mod recado_falado;
mod recados;
mod rosto;
mod rosto_detector;
mod rosto_nucleo;
mod reuniao_matinal;
mod termico;
mod airbnb_base;
mod airbnb_conhecimento;
mod airbnb_escalada;
mod airbnb_vigia;
mod candidaturas;
mod exercito;
mod memoria;
mod monza;
mod vagas;
mod atender;
mod canal;
mod clareza;
mod confirmacao;
mod contexto;
mod personalidade;
mod revisao;
mod tratamento;
mod alma_cuida;
mod alma_mente;
mod atividade;
mod guia_pai;
mod rede_apoio;
mod sobrecarga;
mod zelo;
mod busca_maps;
mod busca_pedido;
mod celular;
mod entrelinhas;
mod faxina_abas;
mod guarda;
mod habilidades_terceiros;
mod ibge_cidades;
mod leads_maps;
mod leis;
mod monitor_dono;
mod mostrar;
mod miniatura;
#[path = "miniatura_logica.rs"]
mod miniatura_logica;
mod organizador;
mod organizar_sessoes;
mod pc;
mod profundo;
mod projeto_consulta;
mod seguranca;
mod skills;
mod candidatura_rotina;
mod correio;
mod empresa_compras;
mod empresa_cortes;
mod empresa_dev;
mod nexodev;
mod empresa_inv;
mod empresa_mkt;
mod fornalha_estudio;
mod freelas;
mod link_auditar;
mod link_avisos;
mod link_concluidos;
mod link_confianca;
mod link_fila;
mod link_identidade;
mod link_municipios;
mod link_pdfs_tcm;
mod link_processo;
mod link_producao;
mod link_protocolo;
mod link_puxar;
mod atender_laco;
mod dono;
mod dono_agente;
mod visao_local;
mod gibberlink;
mod grupo_ia;
mod revisor_claude;
mod vigia;
mod wpp_doc;
mod alma_aprende;
mod alma_evolucao;
mod alma_orbe;
mod janela;
mod alma_voz;
mod animacoes;
mod artcraft;
mod criacao;
mod escuta;
mod estudio3d;
mod imagem;
mod sala;
mod sentinela;
mod video_analise;
mod voz_do_usuarios;
mod vozes_agentes;
mod vozes_catalogo;
mod link_fonte_siconfi;
mod link_lote_parceiro;
mod link_predio;
mod link_radar_mt;
mod link_tribunais;
mod link_tribunais_base;
mod link_tribunais_ce;
mod link_tribunais_mg;
mod link_tribunais_rs;
mod agente_dev;
mod andares;
mod automelhoria;
mod avaliar_jarvis;
mod b2;
mod conversa_cmd;
mod braincenter;
mod claude_agent;
mod claudinho;
mod dev_livre;
mod manutencao;
mod painel;
mod painel_claw;
mod painel_rede;
mod porta_lan;
mod portal;
mod portal_servico;
mod troca_rede;
mod sauraki;
mod airbnb;
mod jarvis;
mod jarvis_comandos;
mod jarvis_ferramentas;
mod jarvis_travas;
mod telegram_bot;
mod telegram_laco;
mod airbnb_laco;
mod telegram_ex;
mod wpp_arquivo;
mod wpp_img;
mod wpp_voz;
mod saida;
mod cliente_exemplo;
mod liga_atendimento;
mod wpp_laco;
mod wpp_midia;
mod wpp_vivo;
mod esteira_orq;
mod esteira_vigias;
mod liga_esteira;
mod liga_casa;
mod liga_b2;
mod liga_painel;
mod liga_telas;
mod executor;
mod executor_empresas;
mod executor_longas;
mod executor_pc;
mod link_tribunais_ac;
mod link_tribunais_am;
mod link_tribunais_es;
mod link_tribunais_go;
mod link_tribunais_ms;
mod link_tribunais_pb;
mod link_tribunais_pr;
mod link_tribunais_rj;
mod link_tribunais_rn;
mod link_tribunais_ro;
mod link_tribunais_sc;
mod link_tribunais_sp;
mod link_tribunais_to;
mod link_tribunais_util;
mod assinatura;
mod cortes_captura;
mod cortes_comandos;
mod cortes_config;
mod cortes_deteccao;
mod cortes_fila;
mod cortes_legenda;
mod cortes_pipeline;
mod cortes_render;
mod cortes_tiktok;
mod cortes_titulos;
mod cortes_twitch;
mod cortes_vigia;
mod cortes_youtube;
mod drive_pasta;
mod fcc;
mod gmail;
mod gmail_web;
mod gratis;
mod ia_img;
mod ia_web;
mod mcp_servidor;
mod spotify;
mod taxa_sepique;
mod tela;
mod tela_viva;
mod wpp_direto;
mod artes_loja;
mod elo_loja;
mod ponte;
mod ligar_tudo;
mod link_arquivos;
mod link_conferir;
mod link_fila_cliente;
mod link_integracao;
mod link_no;
mod link_preparar_parceiro;
mod link_render_estudio;
mod link_render_parceiro;
mod orbe;
mod organiza_claude;
mod par_webrtc;
mod porta_lan_cliente;
mod rede;
mod rede_jarvis;
mod remoto;
mod link_conferir_capitais;
mod link_docs_cap;
mod link_docs_cap2;
mod link_docs_capitais;
mod link_etcm;
mod link_tribunais_ma;
mod link_tribunais_pe;
mod link_tribunais_pi;

use executor::Ambiente;
use jarvis_travas::Pedido;
use serde_json::{json, Value};
use roteador::Roteador;
use std::{cell::RefCell, io::Read, path::{Path, PathBuf}, sync::{Arc, LazyLock, Mutex}};
use tiny_http::{Header, Method, Request, Response, Server};

const PORTA: u16 = 8865;
/// BRANIAC_PORTA so para teste em copia de dados; sem ela, a de sempre.
fn porta() -> u16 { std::env::var("BRANIAC_PORTA").ok().and_then(|p| p.parse().ok()).unwrap_or(PORTA) }
const PARIDADE: &str = include_str!("../paridade.json");

/// Pasta dos dados pessoais: BRANIAC_DADOS ou ./dados. Nunca vai junto do codigo.
fn pasta_dados() -> PathBuf {
    std::env::var_os("BRANIAC_DADOS").map(PathBuf::from).unwrap_or_else(|| "dados".into())
}

/// Pasta que a tela de codigo mostra: BRANIAC_CODIGO ou <dados>/projetos.
fn pasta_codigo() -> PathBuf {
    std::env::var_os("BRANIAC_CODIGO").map(PathBuf::from).unwrap_or_else(|| pasta_dados().join("projetos"))
}

/// Configuracao das IAs: dados/provedores.json (da pessoa) ou o padrao que vem com o codigo.
/// Registro local de tudo que o verificador de respostas pegou (dados/verificacao.jsonl).
fn registrar_verificacao(dados: &std::path::Path, tipo: &str, detalhe: &str) {
    use std::io::Write;
    let a = tempo::agora();
    let linha = json!({ "quando": format!("{} {}", a.data.iso(), a.hm()), "tipo": tipo, "detalhe": detalhe });
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dados.join("verificacao.jsonl")) {
        let _ = writeln!(f, "{linha}");
    }
}

fn config_ias() -> Value {
    std::fs::read_to_string(pasta_dados().join("provedores.json")).ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| serde_json::from_str(include_str!("../padrao/provedores.json")).expect("padrao/provedores.json invalido"))
}

fn roteador() -> Result<Roteador, String> {
    let dados = pasta_dados();
    std::fs::create_dir_all(&dados).map_err(|e| e.to_string())?;
    // BRANIAC_ENV aponta para um .env de outro programa (ex.: o do Braniac em Python): so le, nunca regrava.
    // O .env da propria pasta de dados e criptografado sozinho quando alguem cola uma chave em texto puro.
    let chaves = match std::env::var_os("BRANIAC_ENV") {
        Some(outro) => segredos::ler_env(std::path::Path::new(&outro), false),
        None => segredos::ler_env(&dados.join(".env"), true),
    };
    println!("chaves carregadas: {}", chaves.len()); // so a quantidade: valor de chave nunca vai para a tela
    Ok(Roteador {
        cfg: config_ias(),
        store: store::Store::abrir(&dados.join("braniac.db"))?,
        banco: banco::Banco::abrir(&dados.join("jarvis.db"))?,
        chamar: Box::new(provedores::chamar),
        chamar_tools: Box::new(provedores::chamar_tools),
        // variavel de ambiente ganha; senao, o .env criptografado (aberto so na memoria deste processo)
        contexto: Box::new(move |t, tipo| {
            let (d, completo) = (pasta_dados(), banco::camada_completa(tipo));
            format!("{}{}", agentes::contexto_para(&d, t, !completo), prompt_extra(&d, completo, &tempo::agora()))
        }),
        vigia: Box::new(|tipo, detalhe| registrar_verificacao(&pasta_dados(), tipo, detalhe)),
        ler_chave: Box::new(move |nome| std::env::var(nome).ok().filter(|v| !v.is_empty()).or_else(|| chaves.get(nome).cloned()).unwrap_or_default()),
    })
}

// ---------- ligacoes: os modulos portados entram nas rotas, na conversa e no relogio ----------
// REGRA: este Braniac roda em paralelo ao sistema real e NAO envia nada. Toda closure de "enviar/avisar/audio" abaixo
// grava na caixa de saida local (saida.rs). IA para a Alma: so provedor de tipo "local"; sem ele, vale o texto pronto do modulo.

/// Estado do Brain Center entre um minuto e outro (no Python, um dicionario global).
static BRAIN: Mutex<Option<braincenter::Estado>> = Mutex::new(None);
/// Comandos que o dono deu desde a ultima amostra de atividade: (id da conversa de cada um, houve "errou"). Nunca o texto.
static COMANDOS: Mutex<(Vec<String>, bool)> = Mutex::new((Vec::new(), false));

/// O que a conversa deixou esperando a resposta do dono (duvida que a IA sinalizou).
static PENDENTE: Mutex<Option<jarvis::Pendente>> = Mutex::new(None);
/// A sala: pausada e quem esta silenciado. Vale ate o programa fechar.
static SALA: Mutex<sala::Sala> = Mutex::new(sala::Sala { pausada: false, silenciados: Vec::new(), erro: String::new() });
/// "Um por vez" dos trabalhos longos (video, imagem, modo profundo): tem de valer entre um pedido e outro.
static LONGAS: LazyLock<(Arc<Mutex<estudio3d::Estudio>>, Arc<Mutex<criacao::Estado>>, Arc<profundo::Ativo>)> = LazyLock::new(Default::default);

/// Passo de empresa a pedido: quem esta trabalhando agora e o que o ultimo passo de cada uma devolveu.
static PASSOS: Mutex<(Vec<String>, std::collections::BTreeMap<String, String>)> = Mutex::new((Vec::new(), std::collections::BTreeMap::new()));
const SEM_ORQUESTRADOR: &str = "O orquestrador da esteira não roda no Braniac em Rust: só a fila e a meta foram gravadas, nenhum vídeo é feito por aqui.";

fn travar<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Nome do dono (dados/pessoal.json); "dono" enquanto nao foi configurado.
fn dono_de(dados: &Path) -> String {
    Some(contexto::Ctx::ler(dados).dono).filter(|d| !d.is_empty()).unwrap_or_else(|| "dono".into())
}

/// O status que o retrato do escritorio e o painel esperam (a rota /api/status acrescenta a VM e a aba do WhatsApp).
/// Microfone e e-mail nao existem aqui: saem como desligados.
fn status(dados: &Path, rot: &Roteador, ts: f64) -> Value {
    let provedores: Vec<Value> = rot.status().as_array().into_iter().flatten().cloned().map(|mut p| {
        p["hoje"] = json!(rot.store.uso_hoje(p["nome"].as_str().unwrap_or("")));
        p
    }).collect();
    let pausado = power::pausado(dados);
    json!({ "ligado": !pausado, "pausado": pausado, "mudo": mudo::ativo(dados, ts),
            "atendimento_auto": { "ligado": wpp_laco::ligada(dados), "respostas_ultima_hora": 0 }, "airbnb_auto": { "ligado": wpp_laco::chave(dados, "airbnb") },
            "autonomia": { "armada": power::armada(dados, ts), "restante_min": power::armada_restante_min(dados, ts), "indefinida": power::armada_indefinida(dados, ts) },
            "escuta_voz": { "estado": "parada" }, "microfone": "não ligado", "email": { "logado": false, "estado": "não ligado" }, "provedores": provedores })
}

fn retrato(dados: &Path, rot: &Roteador, agora: &tempo::Agora, ts: f64) -> Value {
    let publico = link_lote_parceiro::publico(dados, &[], &|_| None);
    let esteira = publico["cidades"].as_object().is_some_and(|o| !o.is_empty()).then_some(publico);
    sauraki::snapshot(dados, &status(dados, rot, ts), ts, agora, &sauraki::Extras { esteira, recursos: liga_telas::recursos(),
        link_conversa: link_integracao::resumo(dados, ts, "Aliado"), loja_conversa: elo_loja::resumo(dados, ts), ..Default::default() })
}

/// Modelo LOCAL ("<sistema><SEP><pedido>", ou so o pedido). So provedor de tipo "local": o que a pessoa diz nunca vai a IA de fora.
fn ia_local(rot: &Roteador, pedido: &str) -> Option<String> {
    let (sis, user) = pedido.split_once(alma_mente::SEP).unwrap_or(("", pedido));
    rot.cfg["cadeia"].as_array()?.iter().filter(|p| p["tipo"] == "local").filter_map(|p| p["nome"].as_str())
        .find_map(|nome| rot.perguntar_a(nome, sis, user).ok())
}

/// Uma chamada da Alma com as saidas presas na caixa de saida.
fn com_alma<R>(dados: &Path, rot: &Roteador, agora: &tempo::Agora, f: impl FnOnce(&alma_cuida::Alma) -> R) -> R {
    let nomes = alma_cuida::Nomes::ler(dados);
    let enviar = |para: &str, t: &str| saida::registrar(dados, "alma", para, t);
    let audio = |para: &str, t: &str| saida::registrar(dados, "alma-audio", para, t);
    let llm = |p: &str| ia_local(rot, p);
    f(&alma_cuida::Alma { dados, nomes: &nomes, agora: *agora, enviar: &enviar, audio: Some(&audio), llm: Some(&llm) })
}

/// Bloco extra do prompt de sistema. IA online (`completo` = false) recebe so o que nao e pessoal: tom, humor e a hora.
/// O nome do dono, o guia do jeito dele falar e as regras do Claudinho so vao a quem ja ve a camada completa do banco.
fn prompt_extra(dados: &Path, completo: bool, agora: &tempo::Agora) -> String {
    let ctx = contexto::Ctx::ler(dados);
    let dono = if completo { ctx.dono.as_str() } else { "" };
    let mut s = personalidade::bloco(&personalidade::niveis(dados), dono);
    s += &format!("\n\nAGORA: {}\n{}", contexto::agora_texto(agora), contexto::diretriz_humor(ctx.humor()));
    if completo {
        let voz = voz_do_usuarios::guia(dados, dono);
        if !voz.is_empty() {
            s += &format!("\n\nJEITO DE FALAR DO DONO:\n{voz}");
        }
        s += &format!("\n\nREGRAS DE ATENDIMENTO:\n{}", claudinho::guia(dados, dono));
    }
    s
}

/// "tem alguma pendência?": so quando PERGUNTAM (regra do dono; por isso nao existe no relogio).
fn eh_pendencia(texto: &str) -> bool {
    let n = texto_pt::sem_acento(texto);
    (n.contains("pendenci") || n.contains("pendente")) && consulta::eh_consulta(texto)
}

/// Consulta aos agentes locais, sem IA, na ordem: pendencias, checklist e prazos do Exercito, agente do assunto.
/// Devolve (resposta, agentes consultados). Mudo e personalidade ficam com o `jarvis::processar`.
fn atalho(dados: &Path, texto: &str, agora: &tempo::Agora, ts: f64) -> Option<(String, Vec<String>)> {
    if eh_pendencia(texto) {
        return Some((pendencias::fala(&pendencias::painel(dados, ts), 5), vec!["Mapa".into()]));
    }
    // Cabo Tito: checklist, prazo, exame e EAF saem do edital guardado em dados/edital_stt, sem IA
    if exercito::ativo(dados) {
        if let Ok(ed) = exercito::Edital::ler(dados) {
            let r = if ed.eh_checklist(texto) { ed.marcar(dados, texto, agora.data).ok() } else { None };
            if let Some(r) = r.or_else(|| exercito::eh_pergunta(texto).then(|| ed.resposta(texto, agora.data).0)) {
                return Some((r, vec!["Cabo Tito".into()]));
            }
        }
    }
    // consulta a agente local: a resposta sai dos dados dele, sem IA
    delegacao::atender(dados, texto, agora).map(|r| (r.texto, r.agentes))
}

/// Segundos desde o ultimo toque no teclado ou no mouse. So esse numero: nenhuma tecla, tela ou audio e lido. -1 = nao sei.
#[cfg(windows)]
fn segundos_ocioso() -> i64 {
    #[repr(C)]
    struct UltimoToque {
        tamanho: u32,
        tique: u32,
    }
    #[link(name = "user32")]
    extern "system" {
        fn GetLastInputInfo(info: *mut UltimoToque) -> i32;
    }
    extern "system" {
        fn GetTickCount() -> u32;
    }
    let mut u = UltimoToque { tamanho: 8, tique: 0 };
    // SAFETY: a estrutura tem os 8 bytes que o Windows espera (tamanho informado nela) e a funcao so preenche `tique`.
    if unsafe { GetLastInputInfo(&mut u) } == 0 {
        return -1;
    }
    // SAFETY: nao recebe nada e so devolve um numero.
    ocioso_de(unsafe { GetTickCount() }, u.tique)
}

#[cfg(not(windows))]
fn segundos_ocioso() -> i64 {
    -1
}

/// O contador do Windows volta a zero a cada ~49 dias: a conta e feita com a volta.
#[cfg_attr(not(windows), allow(dead_code))]
fn ocioso_de(agora: u32, ultimo: u32) -> i64 {
    i64::from(agora.wrapping_sub(ultimo) / 1000)
}

/// Abre pagina, pasta ou arquivo no programa padrao. Quem chama e o `pc.rs`, que ja recusou programa e script; a conferencia se
/// repete aqui. O alvo e UM argumento do Explorer (sem shell): nunca vira comando.
fn abrir_no_pc(alvo: &str) -> Result<(), String> {
    let min = alvo.to_lowercase();
    let pagina = min.starts_with("http://") || min.starts_with("https://");
    let caminho = Path::new(alvo);
    if !pagina && (!caminho.exists() || (caminho.is_file() && pc::perigoso(caminho))) {
        return Err("só abro página http(s), pasta ou arquivo que não seja programa".into());
    }
    if !cfg!(windows) {
        return Err(executor::NAO_LIGADO.into());
    }
    crate::oculto::cmd("explorer.exe").arg(alvo).spawn().map(drop).map_err(|e| e.to_string())
}

/// Temperatura da placa de video pelo nvidia-smi (so leitura).
fn temperatura_gpu() -> Result<String, String> {
    let o = crate::oculto::cmd("nvidia-smi").args(["--query-gpu=temperature.gpu", "--format=csv,noheader,nounits"]).output().map_err(|e| e.to_string())?;
    if o.status.success() { Ok(String::from_utf8_lossy(&o.stdout).into_owned()) } else { Err("o nvidia-smi não respondeu".into()) }
}

/// Ambiente das ferramentas: o seguro (todo ENVIO vira linha na caixa de saida) com o que e LOCAL ligado de verdade: abrir
/// pagina/pasta/arquivo, temperatura da placa e IA pelo roteador. `de_fundo` = dentro de um trabalho longo: a IA de apoio ve o
/// resultado das etapas, entao so a local entra, e nenhum outro trabalho longo e aberto.
fn ambiente(dados: &Path, rot: &Arc<Roteador>, agora: &tempo::Agora, ts: f64, de_fundo: bool) -> Ambiente {
    let (r1, r2, r3, r4, r5, r6) = (rot.clone(), rot.clone(), rot.clone(), rot.clone(), rot.clone(), rot.clone());
    let (estudio, criacao, profundo) = LONGAS.clone();
    let d = dados.to_path_buf();
    Ambiente {
        agora: *agora, ts, estudio, criacao, profundo,
        teto_claude: rot.cfg["claude_max_por_dia"].as_i64().unwrap_or(20),
        uso_claude: Box::new(move || r1.store.uso_hoje("claude")),
        add_uso_claude: Box::new(move || drop(r2.store.add_uso("claude"))),
        sensivel: Box::new(move |t| jarvis_travas::tem_dado_real(t) || r3.sensivel(t)),
        // o primeiro argumento ora e o prompt de sistema, ora so o nome da tarefa ("profundo-plano")
        ia: Box::new(move |sis, pedido| r4.neutro_em(if sis.contains(' ') { sis } else { "Responda em portugues do Brasil, direto ao ponto." }, pedido, de_fundo)),
        ia_local: Box::new(move |pedido| ia_local(&r5, pedido).ok_or_else(|| "nenhum modelo local respondeu".to_string())),
        abrir: Box::new(abrir_no_pc),
        processo: Box::new(|nome, _| if nome == "termico_gpu" { temperatura_gpu() } else { Err(executor::NAO_LIGADO.into()) }),
        // so o modo profundo roda (IA pelo roteador); o resto depende de agente de programacao, render ou navegador
        segundo_plano: Box::new(move |nome, args| {
            if de_fundo || nome != "trabalho_profundo" {
                return Err(executor::NAO_LIGADO.into());
            }
            let (rot, dados, nome, args) = (r6.clone(), d.clone(), nome.to_string(), args.clone());
            std::thread::spawn(move || longa(&dados, &rot, &nome, &args));
            Ok(String::new())
        }),
        ..Ambiente::seguro(dados)
    }
}

/// Um trabalho longo na sua propria thread (`executor_longas::rodar`). Cada etapa e o proprio agente com as ferramentas, numa
/// conversa a parte. O que iria direto ao dono fica na caixa de saida (canal "jarvis"); nada sai do PC.
fn longa(dados: &Path, rot: &Arc<Roteador>, nome: &str, args: &Value) {
    let (agora, ts, dono) = (tempo::agora(), fatos::agora_ts(), dono_de(dados));
    let (ctx, familia) = (contexto::Ctx::ler(dados), jarvis_travas::familia_livre(dados));
    let (interno, r, d, c2, f2, dono2) = (ambiente(dados, rot, &agora, ts, true), rot.clone(), dados.to_path_buf(), ctx.clone(), familia.clone(), dono.clone());
    let etapa = move |t: &str| {
        let p = Pedido { origem: "pc", conv: "profundo", original: t, armada: false, ctx: &c2, familia: &f2 };
        let exec = |n: &str, a: &Value, tipo: &str| ferramentas::executar_tudo(&r, &interno, &p, n, a, tipo);
        let resp = r.responder_agente("profundo", t, &ferramentas::lista_para(&interno, t, &[]), &exec, 6).map(|(x, _)| x);
        // conteudo privado que uma ferramenta mostraria ao dono nao entra no resultado da etapa (a conferencia e feita por outra IA)
        for x in interno.saida_direta.borrow_mut().drain(..) {
            saida::registrar(&d, "jarvis", &dono2, &x);
        }
        resp
    };
    let amb = Ambiente { jarvis: Box::new(etapa), ..ambiente(dados, rot, &agora, ts, true) };
    let original = args["objetivo"].as_str().unwrap_or("");
    let p = Pedido { origem: "pc", conv: jarvis::CONV_UNICA, original, armada: false, ctx: &ctx, familia: &familia };
    let fim = executor_longas::rodar(&amb, &p, nome, args);
    amb.saida_direta.borrow_mut().push(fim);
    for x in amb.saida_direta.borrow_mut().drain(..) {
        saida::registrar(dados, "jarvis", &dono, &x);
    }
}

/// Nome da ferramenta de cada empresa nas rotas.
fn ferramenta_da_empresa(chave: &str) -> Option<&'static str> {
    Some(match chave {
        "dev" => "nexodev",
        "mkt" => "marketing",
        "inv" => "investimentos",
        "compras" => "compras",
        "cortes" => "cortes_video",
        _ => return None,
    })
}

/// UM passo de uma empresa, a pedido do dono (botao da tela): mesmas travas e tetos da ferramenta, IA pelo roteador, e git, cargo,
/// ffmpeg, navegador e Drive "não ligado" (o passo que depende deles para com o erro no historico do pedido). No NexoDev, a
/// entrega do dev nao segue para revisao sem commit e build: o ticket fica estacionado em "desenvolvimento".
fn passo_empresa(dados: &Path, rot: &Arc<Roteador>, chave: &str, agora: &tempo::Agora, ts: f64) -> String {
    let Some(nome) = ferramenta_da_empresa(chave).filter(|n| *n != "investimentos") else { return "Essa empresa não tem passo para fazer.".into() };
    let (amb, ctx) = (ambiente(dados, rot, agora, ts, false), contexto::Ctx::ler(dados));
    let p = Pedido { origem: "pc", conv: jarvis::CONV_UNICA, original: "", armada: false, ctx: &ctx, familia: &[] };
    let r = executor::executar(&amb, &p, nome, &json!({ "acao": "trabalhar", "passos": 1 })).map(|(r, _)| r).unwrap_or_default();
    let mut linhas: Vec<String> = amb.saida_direta.borrow_mut().drain(..).collect();
    if linhas.is_empty() {
        linhas.push(r);
    }
    linhas.join("\n")
}

/// O que espera o OK do dono em cada empresa (a tela mostra um botao Aprovar para cada): {empresa: [{id, titulo}]}.
fn esperando_ok(dados: &Path) -> Value {
    let itens = |lista: Vec<Value>, estado: &str, titulo: &dyn Fn(&Value) -> String| -> Vec<Value> {
        lista.iter().filter(|x| x["estado"] == estado).map(|x| json!({ "id": x["id"], "titulo": titulo(x) })).collect()
    };
    let t = |x: &Value, k: &str| x[k].as_str().unwrap_or("").to_string();
    let propostas = empresa_dev::ler(&dados.join("empresa_inv").join("propostas.json")).and_then(|v| v.as_array().cloned()).unwrap_or_default();
    json!({ "dev": itens(empresa_dev::tickets(dados), "pronto", &|x| t(x, "titulo")),
            "inv": itens(propostas, "aguardando_ok", &|x| format!("{} {}", t(x, "perfil"), t(x, "ticker"))),
            "cortes": itens(empresa_cortes::pedidos(dados), "aguardando_ok", &|x| format!("{} {}", t(x, "autor"), t(x, "titulo_fonte")).trim().to_string()) })
}

/// Arquivo com a fila das cidades (secoes Prontos / Em produção / Próximos): BRANIAC_FILA ou dados/link/fila.md.
fn arquivo_da_fila(dados: &Path) -> PathBuf {
    std::env::var_os("BRANIAC_FILA").map(PathBuf::from).unwrap_or_else(|| dados.join("link").join("fila.md"))
}

/// Comando de fila da esteira pela tela. Nenhum processo e derrubado nem religado aqui (o orquestrador nao roda no Rust), e a
/// resposta diz isso em vez de deixar "retoma de onde parou" passar por verdade.
fn comando_esteira(dados: &Path, acao: &str, quantos: i64, agora: &tempo::Agora, ts: f64) -> Result<String, String> {
    if !["status", "parar", "continuar", "mais"].contains(&acao) {
        return Err("acao: status, parar, continuar ou mais (quantos)".into());
    }
    let r = link_lote_parceiro::esteira(dados, acao, quantos, &|| 0, &|| {}, ts, agora)?;
    // tambem no status: sem isto "Esteira rodando" passava por verdade aqui, onde nenhuma esteira roda
    Ok(format!("{r}\n{SEM_ORQUESTRADOR}"))
}

/// Ultimo relatorio de seguranca ja gravado (dados/relatorios/seguranca-AAAA-MM-DD.md). Nenhuma coleta roda aqui.
fn ultimo_relatorio_seguranca(dados: &Path) -> Value {
    let mut arqs: Vec<PathBuf> = std::fs::read_dir(dados.join("relatorios")).into_iter().flatten().flatten().map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("seguranca-") && n.ends_with(".md"))).collect();
    arqs.sort();
    let ultimo = arqs.last();
    json!({ "arquivo": ultimo.and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()),
            "relatorio": ultimo.and_then(|p| std::fs::read_to_string(p).ok()), "guarda": guarda::resumo(dados) })
}

/// Pares da rede de Braniacs, so leitura. Sem rede.json com id nada e lido (a leitura criaria o arquivo); o segredo nunca sai.
fn pares_da_rede(dados: &Path) -> Value {
    if empresa_dev::ler(&dados.join("rede.json")).is_none_or(|d| !d["id"].is_string()) {
        return json!({ "pares": [], "registro": [] });
    }
    let (pares, registro) = rede::Rede::new(dados, "", "", 0).listar(&|_| String::new());
    json!({ "pares": pares, "registro": registro })
}

/// O que o `jarvis::processar` confirmaria como feito ("Escuta LIGADA", "Acionei o Claude") sem existir neste Braniac.
fn nao_ligado_aqui(fala: &str) -> Option<String> {
    let t = jarvis_comandos::sem_contexto(fala.trim());
    let o_que = match t.to_lowercase().as_str() {
        "/escuta on" | "/escuta ligar" => "A escuta por voz",
        "/auto on" | "/auto ligar" => "O atendimento automático",
        _ if jarvis_comandos::claude_tarefa(t).is_some() => "Acionar o Claude",
        _ => return None,
    };
    Some(format!("{o_que} ainda não foi ligado no Braniac em Rust. Nada foi feito."))
}

/// Um turno da conversa pelo `jarvis::processar`, com as pecas que ja existem aqui: comandos de envio (caixa de saida, com o
/// destino conferido), agentes locais (pendencias, Cabo Tito, delegacao), agenda, panico, status, resumo local e a IA com
/// ferramentas. O que nao existe responde "ainda não foi ligado". Esta porta so atende a propria maquina: a origem e sempre "pc".
fn conversar(dados: &Path, rot: &Arc<Roteador>, conv: &str, fala: &str, com_ferramentas: bool, agora: &tempo::Agora, ts: f64) -> Value {
    conversar_de(dados, rot, conv, fala, com_ferramentas, agora, ts, "pc")
}

/// Igual a `conversar`, mas diz de onde veio a fala: "telegram" e restrito (o que AGE so roda com origem "pc" ou com a autonomia armada).
#[allow(clippy::too_many_arguments)]
fn conversar_de(dados: &Path, rot: &Arc<Roteador>, conv: &str, fala: &str, com_ferramentas: bool, agora: &tempo::Agora, ts: f64, origem: &str) -> Value {
    let fala = fala.trim();
    // igual ao Python: fora das conversas de projeto ("b2:N"), tudo e UMA conversa so
    let conv_real = if conv.starts_with("b2:") { conv } else { jarvis::CONV_UNICA };
    let (provedor, consultados) = (RefCell::new(String::new()), RefCell::new(Vec::<String>::new()));
    let resposta = nao_ligado_aqui(fala).unwrap_or_else(|| {
        let amb = ambiente(dados, rot, agora, ts, false);
        let (ctx, familia) = (contexto::Ctx::ler(dados), jarvis_travas::familia_livre(dados));
        let conv_ped = if conv == jarvis_travas::DONO_CONV { conv } else { conv_real }; // o chat verificado do dono tem as liberacoes dele
        let ped = Pedido { origem, conv: conv_ped, original: fala, armada: power::armada(dados, ts), ctx: &ctx, familia: &familia };
        let nl = |o_que: &str| format!("{o_que} ainda não foi ligado no Braniac em Rust. Nada foi feito.");
        // ENVIO: so a caixa de saida, e so depois de conferir que a conversa "aberta" e a pedida (`trava_destino`).
        // Sem ler o WhatsApp, os contatos que existem sao os de dados/pessoal.json.
        let conferida = |nome: &str| (amb.wpp_titulo)(nome).and_then(|aberto| jarvis_travas::trava_destino(&ctx, nome, &aberto).map_or(Ok(aberto), Err));
        let listar = || {
            let mut l: Vec<String> = ctx.contatos.values().cloned().collect();
            // conversas recentes do WhatsApp logado (igual ao Python, que olha a lista da tela antes da agenda)
            if let Ok(v) = whatsapp::estado(dados) {
                l.extend(v["conversas"].as_array().into_iter().flatten().filter_map(|c| c["nome"].as_str().map(str::to_string)));
            }
            l.sort();
            l.dedup();
            Ok(l)
        };
        let enviar_texto = |n: &str, t: &str| conferida(n).and_then(|c| (amb.wpp_texto)(&c, t)).unwrap_or_else(|e| e);
        let enviar_audio = |n: &str, t: &str| conferida(n).and_then(|c| (amb.wpp_audio)(&c, t)).unwrap_or_else(|e| e);
        let resolver_arquivo = |c: &str| wpp_arquivo::resolver_caminho(c, &amb.pastas);
        let enviar_arquivo = |n: &str, arq: &Path, legenda: &str| match conferida(n) {
            Ok(c) => wpp_arquivo::enviar_arquivo(arq, legenda, &std::env::temp_dir(), &|a, l| (amb.wpp_arquivo)(&c, a, l).unwrap_or_else(|e| format!("Não enviei: {e}")), &|| {}),
            Err(e) => e,
        };
        // uma ferramenta do executor chamada sem IA: o que iria direto ao dono e a resposta
        let ferramenta = |nome: &str, args: Value| {
            let r = executor::executar(&amb, &ped, nome, &args).map(|(r, _)| r).unwrap_or_default();
            let direto: Vec<String> = amb.saida_direta.borrow_mut().drain(..).collect();
            if direto.is_empty() { r } else { direto.join("\n\n") }
        };
        // o tom ja vai no prompt de sistema (sem o nome do dono para IA online): o lembrete que o `processar` anexa sai
        let lembrete = personalidade::lembrete(&personalidade::niveis(dados), &ctx.dono);
        let sem_tom = |extra: &str| if lembrete.is_empty() { extra.to_string() } else { extra.replace(&lembrete, "") };

        let mut p = jarvis::Pecas::neutras(dados, ts);
        p.tem_agente = com_ferramentas;
        p.teto_claude = amb.teto_claude;
        p.ficha = jarvis::ficha_jarvis(""); // sem o nome do dono: a ficha pode ir a IA online
        p.saidas = jarvis_comandos::Saidas { listar_nomes: &listar, buscar_contatos: &|_| vec![], perguntar_claude: &|_| None, enviar_texto: &enviar_texto,
            enviar_audio: &enviar_audio, resolver_arquivo: &resolver_arquivo, enviar_arquivo: &enviar_arquivo };
        p.historico = Box::new(|c, n| rot.store.historico(c, n));
        p.sensivel = Box::new(|t| rot.sensivel(t));
        p.panico = Box::new(|origem| painel::panico(dados, origem, agora, ts)["texto"].as_str().unwrap_or("").to_string());
        p.status = Box::new(|| {
            let s = status(dados, rot, ts);
            let prontas: Vec<&str> = s["provedores"].as_array().into_iter().flatten().filter(|i| i["ok"] == true).filter_map(|i| i["nome"].as_str()).collect();
            let sn = |b: &Value| if b == true { "sim" } else { "não" };
            format!("Braniac {}. Mudo: {}. Atendimento automático: {}. Autonomia remota armada: {}. IAs prontas: {}.", if s["ligado"] == true { "ligado" } else { "desligado" },
                sn(&s["mudo"]), sn(&s["atendimento_auto"]["ligado"]), sn(&s["autonomia"]["armada"]), if prontas.is_empty() { "nenhuma".into() } else { prontas.join(", ") })
        });
        p.auto = Box::new(|acao| {
            if acao == "desligar" {
                power::set_atendimento_auto(dados, false);
            }
            format!("Atendimento automático: {}.", if power::atendimento_auto(dados) { "ligado" } else { "desligado" })
        });
        p.escuta = Box::new(|_| "Escuta por voz: não existe no Braniac em Rust (o microfone não é usado).".into());
        p.voz = Box::new(|_| "A voz de cada agente é escolhida na tela Vozes (/vozes).".into());
        p.nao_lidas = Box::new(|| nl("Ler o WhatsApp"));
        p.ler_conversa = Box::new(|_| nl("Ler conversa do WhatsApp"));
        p.atender = Box::new(|| nl("O atendimento do WhatsApp"));
        p.email = Box::new(|_| nl("A leitura de e-mail"));
        // "faz um jogo" no painel ou por voz: so aqui na propria maquina (o `processar` ja exclui o Telegram); teto diario e PROIBIDA moram em dono.rs
        p.tarefa_local = Box::new(|t| {
            let teto = rot.cfg["claude_max_por_dia"].as_i64().unwrap_or(20);
            let (pode, usar) = (|| rot.store.uso_hoje("claude") < teto, || drop(rot.store.add_uso("claude")));
            let (enviar, falar) = (|_: &str, _: &str| String::new(), |_: &str, _: &str| false);
            let agente = |_: &str, prompt: &str, pasta: Option<&Path>| if let Some(pasta) = pasta { dono_agente::disparar_painel(dados, prompt, pasta) };
            dono::tarefa_local(dados, agora, t, &dono_agente::portas(&pode, &usar, &agente, &enviar, &falar)).0
        });
        p.agenda_listar = Box::new(|dia, dias| agenda::ferramenta(&dados.join("agenda.json"), &json!({ "acao": "listar", "dia": dia, "dias": dias }), agora));
        p.resumo_wpp = Box::new(|horas| ferramenta("resumo_whatsapp", json!({ "horas": horas })));
        p.ultimas_do_contato = Box::new(|nome| memoria::historico(dados, nome, 6).into_iter().map(|(de, t)| format!("{}: {t}", if de == "E" { nome } else { "Jarvis" })).collect());
        p.profundo = Box::new(|t| ferramenta("trabalho_profundo", json!({ "objetivo": t })));
        // agente local respondeu: sai como esta, sem passar por IA nenhuma
        p.delegar = Box::new(|t| atalho(dados, t, agora, ts).map(|(texto, quem)| {
            *consultados.borrow_mut() = quem;
            jarvis::Delegado { texto, local_only: true, vazio: false, direto: false }
        }));
        p.agente = Box::new(|texto, conv, extra| {
            let falas: Vec<String> = rot.store.historico(conv, 2).iter().filter_map(|m| m["content"].as_str().map(str::to_string)).collect();
            let (diretas, feitos, chamadas) = (RefCell::new(Vec::new()), RefCell::new(Vec::new()), RefCell::new(Vec::new()));
            let exec = |nome: &str, args: &Value, tipo: &str| {
                let r = ferramentas::executar_tudo(rot, &amb, &ped, nome, args, tipo);
                diretas.borrow_mut().extend(amb.saida_direta.borrow_mut().drain(..));
                chamadas.borrow_mut().push(nome.to_string());
                feitos.borrow_mut().push((nome.to_string(), r.0.clone()));
                r
            };
            let r = rot.responder_agente_com(conv, texto, &sem_tom(extra), &ferramentas::lista_para(&amb, texto, &falas), &exec, 6);
            let r = r.map(|(t, quem)| {
                *provedor.borrow_mut() = quem;
                t
            }).map_err(jarvis::Falha::TodosFalharam);
            let fecho = jarvis::fecho_do_agente(r, &diretas.borrow(), &feitos.borrow(), &chamadas.borrow());
            fecho
        });
        p.responder = Box::new(|conv, t| {
            // o `processar` entrega a fala ja emendada no extra: separa de novo para o historico guardar so a fala
            let (dito, extra) = t.strip_prefix(fala).map_or((t, String::new()), |e| (fala, sem_tom(e)));
            rot.responder_com(conv, dito, &extra).map(|(r, quem)| {
                *provedor.borrow_mut() = quem;
                r
            }).map_err(jarvis::Falha::TodosFalharam)
        });
        // copia, conversa e devolve: a trava nao fica presa enquanto a IA pensa
        let mut pend = travar(&PENDENTE).clone();
        let (r, _) = jarvis::processar(&p, &mut pend, fala, conv, origem, "");
        *travar(&PENDENTE) = pend;
        r
    });
    let provedor = provedor.into_inner();
    if provedor.is_empty() {
        // respondeu sem IA: o turno fica marcado "local/" (nunca vai a IA online)
        let _ = rot.store.add_msg(conv_real, "user", fala, Some("local/agentes"));
        let _ = rot.store.add_msg(conv_real, "assistant", &resposta, Some("local/agentes"));
    }
    json!({ "resposta": resposta, "provedor": if provedor.is_empty() { "agentes" } else { provedor.as_str() }, "agentes": consultados.into_inner() })
}

/// Catalogo de vozes sem consultar a internet nem os motores: so as vozes de portugues conhecidas e nenhuma do Kokoro/Piper instalada.
fn com_fontes<R>(f: impl FnOnce(&vozes_catalogo::Fontes) -> R) -> R {
    let edge = vozes_catalogo::edge_minimo();
    f(&vozes_catalogo::Fontes { edge: &edge, kokoro: &[], piper_instalada: &|_| false })
}

/// Depois de cada turno do dono: memoria, entrelinhas (so observa), Alma (so se a fala e observavel) e a contagem de comandos.
/// Tudo local; o que a Alma mandaria fica na caixa de saida.
fn depois(dados: &Path, rot: &Roteador, conv: &str, texto: &str, resposta: &str, agora: &tempo::Agora, ts: f64) {
    let dono = dono_de(dados);
    let _ = memoria::registrar_msg(dados, &dono, "E", texto, "conversa-jarvis", false, ts);
    let _ = memoria::registrar_msg(dados, &dono, "C", resposta, "conversa-jarvis", false, ts);
    // sem modelo local na entrelinhas: valem as regras (camada 1); ela nunca age
    entrelinhas::observar(dados, texto, &[], None, ts);
    alma_aprende::registrar_sinais(dados, agora, texto); // so numeros, local, e so com o consentimento ligado
    {
        let mut c = travar(&COMANDOS);
        c.0.push(conv.to_string());
        c.1 |= qualidade::correcao(texto);
    }
    if alma_cuida::observavel(texto, &alma_cuida::Nomes::ler(dados)) {
        // conselho de IAs = None: nem a pergunta generica sai daqui
        com_alma(dados, rot, agora, |a| alma_cuida::fala(a, texto, None));
    }
}

/// O que roda a cada minuto alem do Compasso e do bom dia. `ultimo_recado` = maior id do barramento ja contado.
/// Nada aqui usa rede nem processo externo (vagas e freelas ficaram de fora por isso); pendencias tambem nao: so quando perguntam.
static SITES_EM_CURSO: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Codigo HTTP de um site, com 2 tentativas (um soluco de rede nao vira alarme). Resposta abaixo de 500 = no ar.
fn ping_http(url: &str) -> Result<u16, String> {
    let mut r = Err(String::new());
    for i in 0..2 {
        if i > 0 {
            std::thread::sleep(std::time::Duration::from_secs(10));
        }
        r = match ureq::get(url).set("User-Agent", "leitor-de-feed/1.0").timeout(std::time::Duration::from_secs(20)).call() {
            Ok(x) => Ok(x.status()),
            Err(ureq::Error::Status(c, _)) => Ok(c),
            Err(e) => Err(e.to_string()),
        };
        if matches!(r, Ok(c) if c < 500) {
            break;
        }
    }
    r
}

fn relogio(dados: &Path, rot: &Roteador, agora: &tempo::Agora, ts: f64, minuto: u32, ultimo_recado: &mut i64) {
    liga_atendimento::tick(dados, rot, agora, ts, minuto);
    liga_esteira::tick(dados, rot, agora, ts, minuto);
    liga_casa::tick(dados, rot, agora, ts, minuto);
    liga_b2::tick(dados, rot, agora, ts, minuto);
    let dono = dono_de(dados);
    // resumo dos avisos automaticos nas horas marcadas; tratamento vazio = sem vocativo
    funil::tick(dados, agora, ts, &dono, "", &|para, t| saida::registrar(dados, "funil", para, t), &|para, t| saida::registrar(dados, "funil-audio", para, t));
    recados::entregar(dados, &dono, ts, &|para, t| saida::registrar(dados, "recado", para, t), None);
    // atividade: os comandos que ele deu na conversa e ha quantos segundos o teclado/mouse esta parado (so esse numero)
    let (convs, errou) = std::mem::take(&mut *travar(&COMANDOS));
    let novos: Vec<Value> = barramento::historico(dados, 50).into_iter().filter(|r| r["id"].as_i64().unwrap_or(0) > *ultimo_recado).collect();
    *ultimo_recado = novos.iter().filter_map(|r| r["id"].as_i64()).fold(*ultimo_recado, i64::max);
    atividade::amostrar(dados, agora, &atividade::Leitura { ocioso: segundos_ocioso(), convs: &convs, recados: &novos, errou, pausado: false });
    sobrecarga::avaliar(dados, agora, true);
    if minuto % 5 == 0 {
        com_alma(dados, rot, agora, alma_mente::iniciativa);
    }
    if minuto % 10 == 0 {
        // sem destilar regras novas: isso chamaria o agente de programacao
        claudinho::tick(dados, ts, agora, &dono, &contexto::Ctx::ler(dados).casa, &|_| {});
    }
    // Brain Center: sem leitura de CPU/GPU (Null = "sem leitura, liberado"); trabalha numa copia para a rota do painel nao esperar
    let agentes = || retrato(dados, rot, agora, ts)["agentes"].as_array().cloned();
    let mut est = travar(&BRAIN).clone().unwrap_or_default();
    braincenter::tick(dados, &mut est, &agentes().unwrap_or_default(), ts, &Value::Null, &agentes);
    *travar(&BRAIN) = Some(est);
}

fn paridade() -> Value {
    let itens: Vec<Value> = serde_json::from_str(PARIDADE).expect("paridade.json invalido");
    let conta = |e: &str| itens.iter().filter(|i| i["estado"] == e).count();
    json!({ "total": itens.len(), "portados": conta("portado"),
            "em_andamento": conta("em andamento"), "itens": itens })
}

/// Valor de ?chave=... ja sem o %XX.
fn parametro(url: &str, chave: &str) -> Option<String> {
    let bruto = url.split_once('?')?.1.split('&').find_map(|p| p.strip_prefix(chave)?.strip_prefix('='))?;
    let (b, mut saida, mut i) = (bruto.as_bytes(), Vec::new(), 0);
    while i < b.len() {
        let hex = if b[i] == b'%' { bruto.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()) } else { None };
        match hex {
            Some(v) => {
                saida.push(v);
                i += 3;
            }
            None => {
                saida.push(if b[i] == b'+' { b' ' } else { b[i] });
                i += 1;
            }
        }
    }
    String::from_utf8(saida).ok()
}

type Resp = Response<std::io::Cursor<Vec<u8>>>;

fn resposta(corpo: String, tipo: &str, codigo: u16) -> Resp {
    let h = Header::from_bytes("Content-Type", format!("{tipo}; charset=utf-8")).unwrap();
    Response::from_string(corpo).with_header(h).with_status_code(codigo)
}

fn dado(v: Value) -> Resp {
    resposta(v.to_string(), "application/json", 200)
}

fn erro(msg: impl ToString, codigo: u16) -> Resp {
    resposta(json!({ "erro": msg.to_string() }).to_string(), "application/json", codigo)
}

fn feito(r: Result<Value, String>) -> Resp {
    r.map(dado).unwrap_or_else(|e| erro(e, 502))
}

fn pagina(html: &str) -> Resp {
    resposta(html.into(), "text/html", 200)
}

fn atender(req: &mut Request, rot: &Arc<Roteador>) -> Resp {
    let dados = pasta_dados();
    let (agora, ts) = (tempo::agora(), fatos::agora_ts());
    let url = req.url().to_string();
    let caminho = url.split('?').next().unwrap_or("");
    if caminho == "/api/ponte" || caminho.starts_with("/api/ponte/") {
        return ponte::atender(req, rot, &dados, caminho);
    }
    if *req.method() == Method::Get {
        if let Some(r) = projetos::get(&dados, &url).or_else(|| pc_metricas::get(&url)).or_else(|| cerebro::get(&url)) {
            return r;
        }
        if let Some(r) = liga_telas::get(&dados, rot, &url) {
            return r;
        }
        return match caminho {
            "/vm" => pagina(include_str!("vm.html")),
            "/whatsapp" => pagina(include_str!("whatsapp.html")),
            "/codigo" => pagina(include_str!("codigo.html")),
            "/conselho" => pagina(include_str!("conselho.html")),
            "/agenda" => pagina(include_str!("agenda.html")),
            "/conversa" => pagina(include_str!("conversa.html")),
            "/agentes" => pagina(include_str!("agentes.html")),
            "/saida" => pagina(include_str!("saida.html")),
            "/escritorio" => pagina(include_str!("escritorio.html")),
            "/pendencias" => pagina(include_str!("pendencias.html")),
            "/alma" => pagina(include_str!("alma.html")),
            "/empresas" => pagina(include_str!("empresas.html")),
            "/esteira" => pagina(include_str!("esteira.html")),
            "/exercito" => pagina(include_str!("exercito.html")),
            "/vozes" => pagina(include_str!("vozes.html")),
            "/sala" => pagina(include_str!("sala.html")),
            "/api/saida" => dado(saida::ultimas(&dados, 200)),
            "/api/despacho" => dado(despacho::estado(&dados)),
            "/api/sauraki" => {
                let snap = retrato(&dados, rot, &agora, ts);
                dado(if parametro(&url, "demo").is_some_and(|d| d == "1") { sauraki::demo(&snap) } else { snap })
            }
            "/api/predio" => dado(andares::estado(&dados, &[])),
            // agenda e financeiro sao pessoais: esta porta so atende a propria maquina
            "/api/painel" => {
                let hoje = agenda::ferramenta(&dados.join("agenda.json"), &json!({ "acao": "listar" }), &agora);
                dado(json!({ "painel": painel_claw::dados(&dados, dados.parent().unwrap_or(Path::new(".")), ts, &hoje), "bracos": painel::bracos(&dados, &status(&dados, rot, ts), &agora, ts),
                             "historico": painel::historico(&dados, 40, 200) }))
            }
            "/api/braincenter" => braincenter::painel(&dados, &travar(&BRAIN).clone().unwrap_or_default(), ts).map(dado).unwrap_or_else(|e| erro(e, 500)),
            "/api/alma" => dado(json!({ "params": alma_mente::params(&dados), "diario": alma_mente::diario(&dados, 7, &agora), "quieta": alma_mente::quieta(&dados, &agora) })),
            "/api/sobrecarga" => dado(json!({ "agora": sobrecarga::avaliar(&dados, &agora, false), "historico": sobrecarga::historico(&dados, 60) })),
            "/api/qualidade" => {
                let (trocas, correcoes, exemplos) = qualidade::medir(&dados.join("jarvis.db"), &contexto::Ctx::ler(&dados).dono, 7.0, ts);
                dado(json!({ "dias": 7, "trocas": trocas, "correcoes": correcoes, "exemplos": exemplos }))
            }
            "/api/vozes" => {
                // o catalogo vai no topo (formato do painel do Python) e tambem em "catalogo" (tela /vozes do B.R.A.N.I)
                let mut c = com_fontes(|f| vozes_catalogo::catalogo(&dados, f));
                c["catalogo"] = c.clone();
                c["escolhas"] = vozes_agentes::todas(&dados);
                dado(c)
            }
            "/api/status" => {
                let mut s = status(&dados, rot, ts);
                s["vm"] = vm::estado().unwrap_or_else(|e| json!({ "erro": e }));
                // so o estado da aba (logado, qr, carregando) e a contagem: nome de conversa nao entra aqui
                s["whatsapp"] = whatsapp::estado(&dados).map(|w| json!({ "estado": w["estado"], "aviso": w["aviso"], "nao_lidas": w["nao_lidas"] }))
                    .unwrap_or_else(|e| json!({ "estado": "fechado", "aviso": e }));
                dado(s)
            }
            "/api/empresas" => dado(json!({
                "dev": { "situacao": empresa_dev::situacao(&dados, &agora), "quadro": empresa_dev::quadro(&dados, &agora) },
                "mkt": { "situacao": empresa_mkt::situacao(&dados, &agora, ts), "quadro": empresa_mkt::quadro(&dados, &agora, ts) },
                "inv": { "situacao": empresa_inv::situacao(&dados), "quadro": empresa_inv::quadro(&dados) },
                "compras": { "situacao": empresa_compras::situacao(&dados, &agora), "quadro": empresa_compras::quadro(&dados, &agora) },
                "cortes": { "situacao": empresa_cortes::situacao(&dados), "quadro": empresa_cortes::quadro(&dados) },
                "esperando_ok": esperando_ok(&dados),
                "passos": { "trabalhando": travar(&PASSOS).0.clone(), "ultimo": travar(&PASSOS).1.clone() } })),
            // leituras do que ja esta gravado em dados: nada aqui roda coleta, rede ou processo
            "/api/esteira/municipios" => {
                let arq = arquivo_da_fila(&dados);
                match std::fs::read_to_string(&arq) {
                    Ok(fila) => dado(link_municipios::resumo(&dados, &fila, ts)),
                    Err(_) => erro(format!("não achei o arquivo da fila das cidades ({})", arq.display()), 404),
                }
            }
            "/api/esteira/concluidos" => dado(json!({ "concluidos": link_concluidos::lista(&dados) })),
            // so leitura: nao existe rota que liga o tunel nem que fala com o par
            "/api/link" => {
                let url = std::fs::read_to_string(dados.join("link").join("fila_url.txt")).ok().map(|u| u.trim().to_string()).filter(|u| !u.is_empty());
                feito(link_integracao::tunel(&dados, None, url.as_deref(), &|| {}).map(|t| json!({ "conversa": link_integracao::resumo(&dados, ts, "par"), "tunel": t })))
            }
            // sem dados/cortes/cortes.json nada e lido (carregar a configuracao criaria o arquivo)
            "/api/cortes" if !cortes_config::arq(&dados).exists() => dado(json!({ "status": null, "lista": null })),
            "/api/cortes" => {
                let twitch = !(rot.ler_chave)("TWITCH_CLIENT_ID").is_empty() && !(rot.ler_chave)("TWITCH_CLIENT_SECRET").is_empty();
                dado(json!({ "status": cortes_comandos::status(&dados, &cortes_config::carregar(&dados), &agora, ts, twitch),
                             "lista": cortes_comandos::listar(&dados, &parametro(&url, "status").unwrap_or_default()) }))
            }
            "/api/freelas" => dado(json!({ "freelas": freelas::listar(&dados, parametro(&url, "minimo").and_then(|m| m.parse().ok()).unwrap_or(0)) })),
            "/api/candidaturas" => dado(json!({ "resumo": candidaturas::resumo(&dados) })),
            "/api/leis" => match parametro(&url, "q").filter(|q| !q.trim().is_empty()) {
                Some(q) => dado(json!({ "texto": leis::buscar(&dados.join("legislacao.db"), &q, 8) })),
                None => erro("falta ?q=", 400),
            },
            "/api/skills" => {
                let base = Ambiente::seguro(&dados).locais.skills;
                dado(json!({ "catalogo": skills::catalogo(&base, 160), "skills": skills::listar(&base).into_iter().map(|(n, d, _)| json!({ "nome": n, "descricao": d })).collect::<Vec<_>>() }))
            }
            "/api/rede" => dado(pares_da_rede(&dados)),
            "/api/seguranca" => dado(ultimo_relatorio_seguranca(&dados)),
            "/api/esteira" => dado(json!({ "publico": link_lote_parceiro::publico(&dados, &[], &|_| None), "resumo": link_lote_parceiro::resumo(&dados) })),
            // o relatorio e remontado por codigo a partir da auditoria ja gravada (dados/parceiro/auditoria.json); nenhuma auditoria roda aqui
            "/api/auditoria" => match empresa_dev::ler(&dados.join("parceiro").join("auditoria.json")).and_then(|v| v.as_object().cloned()) {
                Some(conf) => link_auditar::relatorio(&dados, &conf, &agora, &[]).and_then(|p| std::fs::read_to_string(p).map_err(|e| e.to_string()))
                    .map(|t| dado(json!({ "relatorio": t, "cidades": conf.len() }))).unwrap_or_else(|e| erro(e, 500)),
                None => dado(json!({ "relatorio": null, "cidades": 0 })),
            },
            "/api/canal" => dado(json!({ "pendentes": canal::pendentes(&dados) })),
            "/api/b2" => b2::estado(&dados, &agentes::nomes(&dados), ts).map(dado).unwrap_or_else(|e| erro(e, 500)),
            "/api/sala" => {
                let depois = parametro(&url, "depois").and_then(|d| d.parse().ok()).unwrap_or(0);
                dado(json!({ "estado": sala::estado(&dados, &agora, &travar(&SALA), false), "mensagens": sala::mensagens(&dados, depois, 100) }))
            }
            "/api/exercito" => match exercito::Edital::ler(&dados) {
                Ok(ed) if exercito::ativo(&dados) => {
                    let feitos = exercito::estado(&dados);
                    dado(json!({ "checklist": ed.checklist(&feitos, agora.data), "feitos": feitos }))
                }
                _ => erro("o Cabo Tito está desligado ou não há edital em dados/edital_stt", 404),
            },
            "/api/memoria" => {
                let (mensagens, conhecimento, decisoes, fatos) = memoria::resumo(&dados);
                dado(json!({ "mensagens": mensagens, "conhecimento": conhecimento, "decisoes": decisoes, "fatos": fatos }))
            }
            "/api/guarda" => dado(json!({ "resumo": guarda::resumo(&dados) })),
            "/api/ponte-agentes" => dado(ponte_agentes::estado(&dados, &agora.data.iso())),
            "/api/agentes/fatos" => dado(json!({ "fatos": fatos::todos(&dados, &parametro(&url, "id").unwrap_or_default(), fatos::agora_ts()) })),
            "/api/agentes" => dado(json!({ "agentes": agentes::listar(&dados).into_iter().map(|mut a| {
                    a["fatos"] = json!(fatos::validos(&dados, a["id"].as_str().unwrap_or(""), false, fatos::agora_ts()).len());
                    a
                }).collect::<Vec<_>>(), "recados": barramento::historico(&dados, 50) })),
            "/api/agenda" => {
                let args = json!({ "acao": "listar", "dia": parametro(&url, "dia").unwrap_or_default(), "dias": parametro(&url, "dias").unwrap_or_default() });
                dado(json!({ "texto": agenda::ferramenta(&dados.join("agenda.json"), &args, &tempo::agora()) }))
            }
            "/api/rotinas" => dado(json!({ "rotinas": compasso::listar(&dados) })),
            "/api/bomdia" => dado(json!({ "fala": bomdia::fala(&dados, &tempo::agora()) })),
            "/api/cofre" => dado(cofre::situacao(&dados, tempo::agora().data)),
            "/api/pendencias" => dado(pendencias::painel(&dados, fatos::agora_ts() as f64)),
            "/api/power" => dado(power::estado(&dados)),
            "/api/mudo" => dado(mudo::estado(&dados, fatos::agora_ts() as f64)),
            "/api/latencia" => dado(json!({ "relatorio": latencia::relatorio(&dados) })),
            "/api/avisos" => dado(avisos::ultimos(&dados, 30)),
            "/api/saude" => dado(json!({"ok": true, "versao": env!("CARGO_PKG_VERSION")})),
            "/api/paridade" => dado(paridade()),
            "/api/modulos" => dado(modulos::estado(&dados)),
            "/api/vm" => feito(vm::estado().map(|mut e| {
                e["tela"] = json!(vm::TELA);
                e
            })),
            "/api/vm/abas" => feito(vm::abas(&dados)),
            "/api/whatsapp" => feito(whatsapp::estado(&dados)),
            "/api/vm/abas/elementos" => feito(abas::elementos(&dados, &parametro(&url, "agente").unwrap_or_default())),
            "/api/vm/abas/texto" => feito(abas::ler(&dados, &parametro(&url, "agente").unwrap_or_default())),
            "/api/ias" => dado(rot.status()),
            // consulta local (so esta maquina alcanca a porta): por padrao mostra a camada completa
            "/api/banco" => {
                let (texto, usou_local) = rot.banco.consultar(&parametro(&url, "q").unwrap_or_default(),
                    &parametro(&url, "tipo").unwrap_or_else(|| "local".into()), None, 8);
                dado(json!({ "texto": texto, "usou_local": usou_local }))
            }
            "/api/codigo/arquivos" => dado(codigo::listar(&pasta_codigo())),
            "/api/codigo/arquivo" => match parametro(&url, "caminho") {
                Some(c) => codigo::ler(&pasta_codigo(), &c).map(dado).unwrap_or_else(|e| erro(e, 400)),
                None => erro("falta ?caminho=", 400),
            },
            _ => nexodev::get(&dados, &url).or_else(|| app_painel::get(&dados, &url)).or_else(|| liga_painel::get(&dados, rot, &url)).or_else(|| liga_atendimento::get(&dados, rot, &url)).or_else(|| wpp_laco::get(&dados, &url)).or_else(|| troca_rede::get(&dados, &url)).or_else(|| liga_esteira::get(&dados, rot, &url)).or_else(|| liga_casa::get(&dados, rot, &url)).or_else(|| liga_b2::get(&dados, rot, &url)).unwrap_or_else(|| erro("nao existe", 404)),
        };
    }
    if *req.method() != Method::Post {
        return erro("nao existe", 404);
    }
    // Pagina de outro site nao consegue mandar este cabecalho: barra pedido forjado pelo navegador.
    if !req.headers().iter().any(|h| h.field.equiv("X-Braniac")) {
        return erro("falta o cabecalho X-Braniac", 403);
    }
    // audio -> texto: o corpo e o audio cru (nao e JSON); a transcricao e local
    if caminho == "/api/ouvir" {
        let tipo = req.headers().iter().find(|h| h.field.equiv("Content-Type")).map(|h| h.value.to_string()).unwrap_or_default();
        let mut audio = Vec::new();
        let _ = req.as_reader().take(20 * 1024 * 1024 + 1).read_to_end(&mut audio);
        return ouvir::transcrever(&dados, &audio, &tipo).map(|t| dado(json!({ "texto": t }))).unwrap_or_else(|e| erro(e, 503));
    }
    let mut corpo = String::new();
    let _ = req.as_reader().take(65536).read_to_string(&mut corpo);
    let v: Value = serde_json::from_str(&corpo).unwrap_or(Value::Null);
    if let Some(r) = projetos::post(&dados, &|s, p| rot.neutro_em(s, p, false), caminho, &v) {
        return r;
    }
    if let Some(r) = cerebro::post(caminho, &v) {
        return r;
    }
    if let Some(r) = liga_telas::post(&dados, rot, caminho, &v) {
        return r;
    }
    if let Some(r) = saudacao::post(&dados, rot, caminho, &v) {
        return r;
    }
    if caminho.starts_with("/api/nexodev/") {
        let rot_ia = rot.clone();
        let ia: Arc<dyn Fn(&str, &str) -> Result<String, String> + Send + Sync> = Arc::new(move |sis, p| rot_ia.neutro_em(if sis.contains(' ') { sis } else { "Responda em portugues do Brasil, direto ao ponto." }, p, false));
        if let Some(r) = nexodev::post(&dados, ia, caminho, &v) {
            return r;
        }
    }
    let texto = |k: &str| v[k].as_str().unwrap_or("").trim().to_string();
    match caminho {
        "/api/modulos" => match serde_json::from_value::<Vec<String>>(v["ativos"].clone()) {
            Ok(p) => modulos::definir(&dados, &p).map(|_| dado(modulos::estado(&dados))).unwrap_or_else(|e| erro(e, 500)),
            Err(_) => erro("mande {\"ativos\": [ids]}", 400),
        },
        "/api/power" => {
            if v["ligado"] == false { power::desligar(&dados, "manual", "") } else { power::ligar(&dados) }
            dado(power::estado(&dados))
        }
        "/api/mudo" => {
            let ts = fatos::agora_ts() as f64;
            // as telas mandam {"mudo": false} (painel) ou {"acao": "desmutar"} (HUD)
            dado(if v["mudo"] == false || v["acao"] == "desmutar" { mudo::desmutar(&dados, ts) } else { mudo::mutar(&dados, v["minutos"].as_f64(), ts) })
        }
        "/api/chat" if texto("texto").is_empty() => erro("mande {\"texto\": \"...\"}", 400),
        "/api/chat" => {
            let (conv, fala) = (if texto("conv").is_empty() { jarvis::CONV_UNICA.to_string() } else { texto("conv") }, texto("texto"));
            // desligado, o `processar` so atende os comandos (/ligar, /status...) e nada e observado
            let ligado = !power::pausado(&dados);
            // {"ferramentas": false} = conversa simples, sem agente
            let corpo = conversar(&dados, rot, &conv, &fala, v["ferramentas"] != false, &agora, ts);
            if ligado {
                // a Alma pode chamar o modelo local (segundos): roda fora do pedido para a resposta nao esperar
                let (rot, resposta) = (rot.clone(), corpo["resposta"].as_str().unwrap_or("").to_string());
                std::thread::spawn(move || depois(&pasta_dados(), &rot, &conv, &fala, &resposta, &agora, ts));
            }
            dado(corpo)
        }
        // pergunta solta a UMA IA (sem banco e sem historico); texto sensivel so vai a IA que nao e online
        "/api/ia" => {
            let nome = texto("nome");
            let online = rot.cfg["cadeia"].as_array().into_iter().flatten().any(|p| p["nome"] == nome.as_str() && p["tipo"] == "online");
            if online && rot.sensivel(&texto("texto")) {
                erro("texto com dado sensivel nao vai para IA de fora", 400)
            } else {
                rot.perguntar_a(&nome, "Responda em portugues do Brasil, direto ao ponto.", &texto("texto"))
                    .map(|r| dado(json!({ "resposta": r, "provedor": nome }))).unwrap_or_else(|e| erro(e, 503))
            }
        }
        "/api/agenda" => dado(json!({ "resultado": agenda::ferramenta(&dados.join("agenda.json"), &v, &tempo::agora()) })),
        "/api/rotinas" => match texto("acao").as_str() {
            "criar" => feito(compasso::criar(&dados, &v["rotina"], &tempo::agora())),
            "ativar" | "pausar" => feito(compasso::ativar(&dados, &texto("id"), texto("acao") == "ativar")),
            "remover" => feito(compasso::remover(&dados, &texto("id")).map(|_| json!({ "ok": true }))),
            _ => erro("acao: criar, ativar, pausar ou remover", 400),
        },
        // texto -> audio WAV. Primeiro a voz escolhida para o agente na tela Vozes (principal, depois as reservas); se nao houver
        // ou todas cairem, a ordem de sempre (voz.json). Texto sensivel nunca vai a voz da nuvem: fala a voz local
        // amostra da tela Vozes: fala uma frase fixa com o motor/voz pedidos (so grava a escolha depois, em /api/vozes/escolher)
        "/api/vozes/amostra" => {
            const FRASE: &str = "Olá, esta é a minha voz. Posso falar assim com você.";
            let (m, vz) = (texto("motor"), texto("voz"));
            let estilo = vozes_agentes::escolha(&dados, &texto("agente"))["ajustes"]["estilo"].as_str().unwrap_or("").to_string();
            match vozes_catalogo::amostra(&|| voz::gerar(&*rot.ler_chave, &m, &vz, FRASE, &estilo)) {
                Ok((audio, seg)) => {
                    let r = Response::from_data(audio).with_header(Header::from_bytes("Content-Type", "audio/wav").unwrap());
                    r.with_header(Header::from_bytes("X-Segundos", seg.to_string()).unwrap())
                }
                Err(e) => erro(e, 503),
            }
        }
        "/api/voz" => {
            let (t, agente) = (texto("texto"), texto("agente"));
            let nuvem = !rot.sensivel(&t);
            let estilo = vozes_agentes::escolha(&dados, &agente)["ajustes"]["estilo"].as_str().unwrap_or("").to_string();
            let escolhida = vozes_agentes::falar(&dados, &agora, &agente, &t, nuvem, &|motor, v, tx| voz::gerar(&*rot.ler_chave, motor, v, tx, &estilo));
            match escolhida.map(|f| (f.wav, f.motor)).or_else(|_| voz::falar_wav(&dados, &*rot.ler_chave, &t, nuvem, &agente)) {
                Ok((audio, motor)) => {
                    let r = Response::from_data(audio).with_header(Header::from_bytes("Content-Type", "audio/wav").unwrap());
                    // nome de voz com acento nao cabe em cabecalho: a fala sai sem ele
                    match Header::from_bytes("X-Voz", motor) {
                        Ok(h) => r.with_header(h),
                        Err(()) => r,
                    }
                }
                Err(e) => erro(e, 503),
            }
        }
        // escolher a voz de um agente (posicao 1 = principal, 2 e 3 = reservas) ou tirar uma reserva
        "/api/vozes" => {
            let (agente, pos) = (texto("agente"), v["posicao"].as_i64().unwrap_or(1));
            let r = match texto("acao").as_str() {
                "escolher" => com_fontes(|f| vozes_catalogo::escolher(&dados, f, &texto("motor"), &texto("voz"), &agente, pos)),
                "tirar_reserva" if pos == 2 || pos == 3 => vozes_agentes::tirar_reserva(&dados, &agente, pos),
                _ => Err("acao: escolher (motor, voz, posicao 1 a 3) ou tirar_reserva (posicao 2 ou 3)".into()),
            };
            r.map(|e| dado(json!({ "escolha": e }))).unwrap_or_else(|e| erro(e, 400))
        }
        "/api/sala" => {
            let mut s = travar(&SALA);
            match texto("acao").as_str() {
                "pausar" | "silenciar" => sala::acao(&dados, &mut s, &v),
                // so guarda a fala (e a resposta pronta de cuidado, se for crise): a rodada dos agentes usa IA e nao roda aqui
                "falar" if !texto("texto").is_empty() => {
                    sala::dono_fala(&dados, &agora, &sala::Gente { dono: dono_de(&dados), filho: String::new(), parceira: String::new() }, &texto("texto"));
                }
                _ => return erro("acao: pausar, silenciar (agente) ou falar (texto)", 400),
            }
            dado(json!({ "estado": sala::estado(&dados, &agora, &s, false) }))
        }
        // abrir pedido, aprovar e UM passo de trabalho, pelas mesmas travas das ferramentas (modulo ligado; aprovar so a pedido do
        // dono, que nesta porta local vale). O passo so roda por este pedido, nunca pelo relogio.
        "/api/empresas" => {
            let chave = texto("empresa");
            let Some(nome) = ferramenta_da_empresa(&chave) else { return erro("empresa: dev, mkt, inv, compras ou cortes", 400) };
            if texto("acao") == "trabalhar" {
                if nome == "investimentos" {
                    return erro("os investimentos (simulação) não têm passo: só propor e aprovar", 400);
                }
                if power::pausado(&dados) {
                    return erro("Braniac desligado: ligue no painel antes de pedir um passo", 409);
                }
                {
                    let mut p = travar(&PASSOS);
                    if p.0.contains(&chave) {
                        return dado(json!({ "resultado": "Já tem um passo dessa empresa em andamento." }));
                    }
                    p.0.push(chave.clone());
                }
                // a IA demora: o passo roda fora do pedido e o resultado aparece no quadro
                let rot = rot.clone();
                std::thread::spawn(move || {
                    let r = passo_empresa(&dados, &rot, &chave, &agora, ts);
                    let mut p = travar(&PASSOS);
                    p.0.retain(|c| *c != chave);
                    p.1.insert(chave, format!("{}: {r}", agora.hm()));
                });
                return dado(json!({ "resultado": "Comecei um passo. O resultado aparece no quadro em instantes." }));
            }
            if !matches!(texto("acao").as_str(), "abrir" | "propor" | "aprovar") {
                return erro("acao: abrir (propor, nos investimentos), aprovar ou trabalhar", 400);
            }
            let (amb, ctx) = (Ambiente::seguro(&dados), contexto::Ctx::ler(&dados));
            let p = Pedido { origem: "pc", conv: jarvis::CONV_UNICA, original: "", armada: false, ctx: &ctx, familia: &[] };
            executor::executar(&amb, &p, nome, &v).map(|(r, _)| dado(json!({ "resultado": r }))).unwrap_or_else(|| erro("nao existe", 404))
        }
        // comandos de FILA da esteira (meta, parar, continuar): nada e preparado, narrado, renderizado nem enviado ao site por aqui
        "/api/esteira" => comando_esteira(&dados, &texto("acao"), v["quantos"].as_i64().unwrap_or(0), &agora, ts).map(|r| dado(json!({ "resultado": r }))).unwrap_or_else(|e| erro(e, 400)),
        // uma linha no relatorio do proximo bom dia (quem termina uma tarefa anota aqui)
        "/api/cofre" => cofre::informar(&dados, tempo::agora().data, v["saldo"].as_f64(), v["meta"].as_f64()).map(dado).unwrap_or_else(|e| erro(e, 400)),
        "/api/relatorio" => {
            if bomdia::registrar(&dados, &tempo::agora(), &texto("texto")) { dado(json!({ "ok": true })) } else { erro("mande {\"texto\": \"...\"}", 400) }
        }
        // reconhecimento de rosto local: cadastrar (fotos -> 128 numeros), quem (olha a camera agora), pessoas (so nomes e quantidade)
        "/api/rosto/cadastrar" => {
            let fotos: Vec<String> = v["fotos"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
            (if v["camera"].as_bool().unwrap_or(false) { rosto::cadastrar_camera(&dados, &texto("nome")) } else { rosto::cadastrar(&dados, &texto("nome"), &fotos) }).map(dado).unwrap_or_else(|e| erro(e, 400))
        }
        "/api/rosto/quem" => rosto::quem(&dados).map(|r| dado(json!({ "visto": format!("{:?}", r), "fala": rosto::fala(&r, "") }))).unwrap_or_else(|e| erro(e, 500)),
        "/api/rosto/pessoas" => dado(json!({ "pessoas": rosto::pessoas(&dados).iter().map(|p| json!({ "nome": p.nome, "vetores": p.vetores.len() })).collect::<Vec<_>>(), "ligado": rosto::config(&dados).ligado })),
        "/api/conselho" => conselho::convocar(rot, &texto("tema")).map(dado).unwrap_or_else(|e| erro(e, 400)),
        "/api/banco/fato" => {
            let val = v["validade"].as_f64().or_else(|| fatos::validade_do_texto(&texto("texto"), &tempo::agora(), fatos::agora_ts()));
            let ok = rot.banco.gravar_fato(&texto("escopo"), &texto("chave"), &texto("texto"), &texto("nivel"), "api", val);
            if ok {
                fatos::espelhar(&dados, &texto("escopo"), &texto("chave"), &texto("texto"), &texto("nivel"), "api", val);
            }
            if ok { dado(json!({ "ok": true })) } else { erro("fato sem texto", 400) }
        }
        "/api/alma" => {
            let r = alma_mente::definir(&dados, &texto("chave"), &v["valor"]);
            if r["erro"].is_null() { dado(r) } else { erro(r["erro"].as_str().unwrap_or("erro"), 400) }
        }
        "/api/exercito" => match exercito::Edital::ler(&dados) {
            Ok(ed) if exercito::ativo(&dados) => ed.marcar(&dados, &texto("texto"), agora.data).map(|r| dado(json!({ "resposta": r }))).unwrap_or_else(|e| erro(e, 500)),
            _ => erro("o Cabo Tito está desligado ou não há edital em dados/edital_stt", 404),
        },
        "/api/b2" => {
            let (y, m, d) = agora.data.partes();
            b2::acao(&dados, &dados.join("skills"), &v, &agentes::nomes(&dados), ts, &format!("{d:02}/{m:02}/{y:04}")).map(dado).unwrap_or_else(|e| erro(e, 400))
        }
        // so desliga e desarma (direcao segura)
        "/api/panico" => dado(painel::panico(&dados, "pc", &agora, ts)),
        "/api/predio" => andares::construir(&dados, &texto("andar"), &[]).map(dado).unwrap_or_else(|e| erro(e, 400)),
        "/api/whatsapp/abrir" => feito(whatsapp::abrir(&dados)),
        "/api/vm/ligar" => feito(vm::ligar()),
        "/api/vm/desligar" => feito(vm::desligar()),
        "/api/vm/abas" if texto("agente").is_empty() => erro("diga qual agente usa a aba", 400),
        "/api/vm/abas" => feito(vm::abrir_aba(&dados, &texto("agente"), &texto("url"))),
        "/api/vm/abas/navegar" => feito(abas::navegar(&dados, &texto("agente"), &texto("url"))),
        "/api/vm/abas/clicar" => feito(abas::clicar(&dados, &texto("agente"), v["n"].as_u64().unwrap_or(0) as u32)),
        // mesma trava da ferramenta: texto sensivel nunca e digitado pelo Braniac
        "/api/vm/abas/digitar" if rot.sensivel(&texto("texto")) => erro("nao digitei: o texto tem dado sensivel, e isso e sempre a pessoa", 400),
        "/api/vm/abas/digitar" => feito(abas::digitar(&dados, &texto("agente"), v["n"].as_u64().unwrap_or(0) as u32, &texto("texto"), v["enter"] == true)),
        "/api/vm/abas/mostrar" => feito(vm::mostrar_aba(&texto("id"))),
        "/api/vm/abas-fixas" => feito(abas_fixas::garantir(&dados).map(|a| json!({ "abertas": a }))),
        "/api/vm/abas/fechar" => feito(vm::fechar_aba(&dados, &texto("id"))),
        _ => app_painel::post(&dados, caminho, &v).or_else(|| liga_painel::post(&dados, caminho, &v)).or_else(|| liga_atendimento::post(&dados, rot, caminho, &v)).or_else(|| troca_rede::post(&dados, caminho, &v)).or_else(|| wpp_laco::post(&dados, caminho, &v)).or_else(|| importar_antigo::post(&dados, caminho, &v)).or_else(|| liga_esteira::post(&dados, rot, caminho, &v)).or_else(|| liga_casa::post(&dados, rot, caminho, &v)).or_else(|| liga_b2::post(&dados, rot, caminho, &v)).unwrap_or_else(|| erro("nao existe", 404)),
    }
}

fn main() {
    oculto::log_em_arquivo(&pasta_dados());
    // so a propria maquina: nada de fora alcanca esta porta
    let servidor = Server::http(("127.0.0.1", porta()))
        .unwrap_or_else(|e| panic!("porta {} ocupada (ja tem um Braniac Rust rodando?): {e}", porta()));
    let rot = Arc::new(roteador().unwrap_or_else(|e| panic!("nao abri os bancos em {}: {e}", pasta_dados().display())));
    println!("Braniac Rust em http://127.0.0.1:{PORTA}  dados: {}", pasta_dados().display());
    // uma pergunta a IA demora segundos: sem isto as telas travavam enquanto ela pensava
    let servidor = Arc::new(servidor);
    saudacao::boot(&pasta_dados(), rot.clone()); // so fala se o Windows acabou de iniciar (dados/logon.flag novo)
    // relogio do Compasso: a cada minuto ve se ha lembrete da agenda ou rotina na hora
    let rot_relogio = rot.clone();
    std::thread::spawn(move || {
        let (mut minuto, mut avisadas) = (0u32, std::collections::HashSet::new());
        // o que ja esta no barramento antes de ligar nao conta como atividade nova
        let mut ultimo_recado = barramento::historico(&pasta_dados(), 1).first().and_then(|r| r["id"].as_i64()).unwrap_or(0);
        loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
        let (dados, agora) = (pasta_dados(), tempo::agora());
        if power::pausado(&dados) {
            continue; // servidor pausado: nenhuma rotina do relogio roda ate retomar
        }
        // curador dos fatos: a cada 30 min vence o que passou do prazo; cada contradicao entra UMA vez no relatorio do bom dia
        if minuto % 30 == 0 {
            for (ag, x, y) in fatos::curar(&dados, fatos::agora_ts()).1.into_iter().take(3) {
                let t = format!("Curador {ag}: possível contradição na memória: «{x}» × «{y}». Confira qual vale.");
                if avisadas.insert(t.clone()) {
                    bomdia::registrar(&dados, &agora, &t);
                }
            }
        }
        relogio(&dados, &rot_relogio, &agora, fatos::agora_ts(), minuto, &mut ultimo_recado);
        minuto += 1;
        // bom dia: uma vez por dia, a partir das 6h (por enquanto na tela de avisos)
        bomdia::tick(&dados, &agora, &|fala| avisos::registrar(&dados, &agora, "dono", fala));
        // o aviso passa pelo barramento (Compasso -> Jarvis) e e o Jarvis quem fala com a pessoa; no barramento vai so o tipo
        compasso::tick(&dados, &agora, &|para, texto| {
            let _ = barramento::enviar(&dados, "Compasso", "Jarvis", "alerta", "lembrete ou rotina na hora", 0, None);
            avisos::registrar(&dados, &agora, para, texto)
        });
        // servidor do celular: pinga os 3 sites 1x por dia (a partir das 9h); se algum nao responder, o Jarvis avisa o dono no WhatsApp
        if guarda::sites_pendente(&dados, agora.data.0, agora.hora) && !SITES_EM_CURSO.swap(true, std::sync::atomic::Ordering::SeqCst) {
            std::thread::spawn(|| {
                let (dados, agora, ts) = (pasta_dados(), tempo::agora(), fatos::agora_ts());
                let dono = dono_de(&dados);
                guarda::sites_do_dia(&dados, agora.data.0, agora.hora, &ping_http, &|t| {
                    drop(funil::filtrar(&dados, &dono, &dono, t, "guarda", &agora, ts, &|para, x| saida::registrar(&dados, "guarda", para, x)));
                });
                SITES_EM_CURSO.store(false, std::sync::atomic::Ordering::SeqCst);
            });
        }
        }
    });
    // atendimento do WhatsApp: sempre em sombra, a menos que o dono ligue a chave (dados/troca.json)
    wpp_laco::iniciar(rot.clone());
    telegram_laco::iniciar(rot.clone());
    airbnb_laco::iniciar();
    portal_servico::iniciar(); // so abre a porta 8780 com a chave "portal" ligada
    despacho::iniciar(rot.clone());
    ponte::iniciar(); // so sobe o filho Python com a chave "loja" ou "link" ligada
    aparelho::iniciar(rot.clone()); // microfone e alto-falante: so abrem com a chave "voz"; a bandeja sobe sempre
    reconcilia_abas::iniciar(); // reusa as abas que ja existem, fecha as repetidas e repoe as que faltam (na partida e a cada 2 min)
    let fios: Vec<_> = (0..4).map(|_| {
        let (servidor, rot) = (servidor.clone(), rot.clone());
        std::thread::spawn(move || {
            while let Ok(mut req) = servidor.recv() {
                let r = atender(&mut req, &rot);
                let _ = req.respond(r);
            }
        })
    }).collect();
    for f in fios {
        let _ = f.join();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parametro_tira_o_percentual() {
        assert_eq!(super::parametro("/x?a=1&caminho=src%2Fm%C3%A3o+1.rs", "caminho").unwrap(), "src/mão 1.rs");
        assert!(super::parametro("/x?caminhos=1", "caminho").is_none());
        assert!(super::parametro("/x", "caminho").is_none());
    }

    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn pasta(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-liga-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("pessoal.json"), r#"{"nome": "Zuleica", "de_casa": ["Bia"]}"#).unwrap();
        d
    }

    fn em(hora: u32) -> tempo::Agora {
        tempo::Agora { data: tempo::Data::civil(2026, 11, 18).unwrap(), hora, min: 0, seg: 0 }
    }

    /// Roteador so com IA online; `chamadas` conta cada vez que ela e chamada (nas ligacoes locais tem de ficar em zero).
    fn rot_so_online(chamadas: Arc<AtomicUsize>) -> Roteador {
        let c2 = chamadas.clone();
        Roteador {
            cfg: json!({ "system": "S", "cadeia": [{ "nome": "b", "tipo": "online", "api": "openai", "chave_env": "TEM" }] }),
            store: store::Store::em_memoria().unwrap(),
            banco: banco::Banco::em_memoria().unwrap(),
            chamar: Box::new(move |_, _, _, _| { chamadas.fetch_add(1, Ordering::SeqCst); Ok("x".into()) }),
            chamar_tools: Box::new(move |_, _, _, _, _| { c2.fetch_add(1, Ordering::SeqCst); Ok(json!({ "content": "x" })) }),
            contexto: Box::new(|_, _| String::new()),
            vigia: Box::new(|_, _| {}),
            ler_chave: Box::new(|_| "k".into()),
        }
    }

    fn canais(d: &Path) -> Vec<String> {
        saida::ultimas(d, 99).as_array().unwrap().iter().map(|l| l["canal"].as_str().unwrap().to_string()).collect()
    }

    #[test]
    fn pendencia_so_quando_perguntam() {
        assert!(eh_pendencia("Tem alguma pendência?") && eh_pendencia("Jarvis, quais as pendências") && eh_pendencia("o que está pendente?"));
        assert!(!eh_pendencia("anota uma pendência: ligar pro banco") && !eh_pendencia("resolvi a pendência") && !eh_pendencia("tem reunião hoje?"));
    }

    #[test]
    fn prompt_da_ia_online_nao_leva_nome_nem_guia_do_dono() {
        let d = pasta("prompt");
        let (fora, dentro) = (prompt_extra(&d, false, &em(10)), prompt_extra(&d, true, &em(10)));
        assert!(!fora.contains("Zuleica") && !fora.contains("REGRAS DE ATENDIMENTO") && !fora.contains("JEITO DE FALAR"), "{fora}");
        assert!(fora.contains("AGORA: quarta-feira, 18/11/2026, 10:00") && fora.contains("Humor 35%"), "{fora}");
        assert!(dentro.starts_with(&fora) && dentro.contains("REGRAS DE ATENDIMENTO"), "{dentro}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn conversa_resolve_sem_ia_o_que_e_local_e_envio_so_vai_para_a_caixa_de_saida() {
        let d = pasta("conversa");
        std::fs::write(d.join("pessoal.json"), r#"{"nome": "Zuleica", "contatos": {"bia": "Beatriz Lima"}}"#).unwrap();
        let chamadas = Arc::new(AtomicUsize::new(0));
        let rot = Arc::new(rot_so_online(chamadas.clone()));
        let ts = fatos::agora_ts();
        let diz = |t: &str| conversar(&d, &rot, "claude", t, true, &em(10), ts);
        let r = |t: &str| diz(t)["resposta"].as_str().unwrap().to_string();
        assert!(atalho(&d, "oi, tudo certo por aí", &em(10), ts).is_none());
        assert!(r("Jarvis, silêncio").contains("Mudo") && mudo::ativo(&d, ts));
        assert!(r("pode falar").contains("Som de volta") && !mudo::ativo(&d, ts));
        let p = diz("tem alguma pendência?");
        assert_eq!((p["resposta"].as_str(), p["provedor"].as_str(), &p["agentes"]), (Some("Nenhuma pendência esperando você agora."), Some("agentes"), &json!(["Mapa"])));
        // comandos de envio: caixa de saida, com o destino conferido; quem nao esta nos contatos nao recebe nada
        assert!(r("/enviar bia|cheguei bem").starts_with('✅'));
        assert!(r("manda um áudio para a Bia dizendo que já estou a caminho").starts_with('✅'));
        assert!(r("/enviar Caio Obras|oi").starts_with("Procurei"));
        assert!(r("/enviar bia|a senha é abc123").starts_with("Não mandei"));
        let caixa = saida::ultimas(&d, 9);
        assert_eq!(caixa.as_array().unwrap().len(), 2, "{caixa}");
        assert_eq!((caixa[1]["canal"].as_str(), caixa[1]["destino"].as_str(), caixa[1]["texto"].as_str()), (Some("pedido"), Some("Beatriz Lima"), Some("cheguei bem")));
        assert_eq!(caixa[0]["canal"], "pedido-audio");
        // o que nao existe aqui nao e confirmado como feito
        for t in ["/escuta on", "/auto ligar", "pede pro Claude arrumar o site"] {
            assert!(r(t).contains("ainda não foi ligado"), "{t}");
        }
        assert!(r("tem e-mail novo?").contains("ainda não foi ligado") && r("/status").starts_with("Braniac ligado. Mudo: não."));
        assert_eq!(chamadas.load(Ordering::SeqCst), 0, "nada acima pode chamar IA");
        // conversa comum vai a IA; qualquer conversa que nao e de projeto e a mesma ("jarvis") e o que saiu sem IA fica marcado local
        let c = diz("oi, tudo bem com você?");
        assert!(c["provedor"] == "b" && c["agentes"] == json!([]), "{c}");
        assert!(chamadas.load(Ordering::SeqCst) > 0);
        let h = rot.store.historico(jarvis::CONV_UNICA, 99);
        assert!(h.len() >= 20 && h[0]["provider"] == "local/agentes" && h.last().unwrap()["provider"] == "b" && rot.store.historico("claude", 9).is_empty(), "{h:?}");
        // desligado: so os comandos; /ligar religa pela propria conversa
        power::desligar(&d, "manual", "");
        assert!(r("oi").starts_with("Braniac desligado") && r("/ligar") == "Braniac ligado." && !power::pausado(&d));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn leitura_privada_de_ferramenta_vai_direto_ao_dono_e_tira_a_ia_online_do_turno() {
        let d = pasta("direto");
        let chamadas = Arc::new(AtomicUsize::new(0));
        let mut rot = rot_so_online(chamadas.clone());
        let c2 = chamadas.clone();
        // a IA online pede o resumo do WhatsApp (dado local); depois disso ela nao pode mais ser chamada
        rot.chamar_tools = Box::new(move |_, _, _, _, _| {
            c2.fetch_add(1, Ordering::SeqCst);
            Ok(json!({ "role": "assistant", "content": null, "tool_calls": [{ "id": "1", "type": "function", "function": { "name": "resumo_whatsapp", "arguments": "{}" } }] }))
        });
        let ts = fatos::agora_ts();
        memoria::registrar_msg(&d, "Ana", "E", "oi, o orçamento ficou pronto?", "whatsapp", false, ts - 60.0).unwrap();
        let c = conversar(&d, &Arc::new(rot), "jarvis", "me conta as novidades do dia", true, &em(10), ts);
        assert!(c["resposta"].as_str().unwrap().contains("Ana") && c["provedor"] == "agentes", "{c}");
        assert_eq!(chamadas.load(Ordering::SeqCst), 1);
        assert!(saida::ultimas(&d, 9).as_array().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn modo_profundo_roda_as_etapas_e_a_ia_de_apoio_nunca_e_online() {
        let d = pasta("profundo");
        let fora = Arc::new(AtomicUsize::new(0));
        let f2 = fora.clone();
        let mut rot = rot_so_online(Arc::new(AtomicUsize::new(0)));
        rot.cfg = json!({ "system": "S", "cadeia": [{ "nome": "b", "tipo": "online", "api": "openai", "chave_env": "TEM" }, { "nome": "c", "tipo": "local", "api": "ollama" }] });
        rot.chamar = Box::new(move |p, msgs, _, _| {
            if p["tipo"] == "online" {
                f2.fetch_add(1, Ordering::SeqCst);
            }
            let pedido = msgs[1]["content"].as_str().unwrap_or("");
            Ok(if pedido.starts_with("Planeje") { r#"{"etapas": ["ver o que está marcado"], "pronto_quando": "ok"}"#.into() }
                else if pedido.starts_with("Confira") { r#"{"ok": true}"#.into() } else { "Resposta final montada só com o que a etapa apurou.".into() })
        });
        rot.chamar_tools = Box::new(|_, _, _, _, _| Ok(json!({ "role": "assistant", "content": "A etapa foi feita e não achei nada marcado para a semana." })));
        longa(&d, &Arc::new(rot), "trabalho_profundo", &json!({ "objetivo": "veja a fundo como está a minha semana" }));
        let caixa = saida::ultimas(&d, 20);
        let textos: Vec<&str> = caixa.as_array().unwrap().iter().rev().map(|l| l["texto"].as_str().unwrap()).collect();
        assert!(caixa.as_array().unwrap().iter().all(|l| l["canal"] == "jarvis" && l["destino"] == "Zuleica"), "{caixa}");
        assert!(textos[0].starts_with("🧭 Modo profundo: 1 etapa(s)") && textos.iter().any(|t| t.starts_with("🧭 Resposta final montada")) && *textos.last().unwrap() == "Modo profundo encerrado.", "{textos:?}");
        assert_eq!(fora.load(Ordering::SeqCst), 0, "plano, resposta final e conferencia veem o resultado das etapas: so IA local");
        // a trava de "um por vez" ficou solta no fim
        assert!(LONGAS.2.comecar("outro").is_ok());
        LONGAS.2.terminar();
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn passo_de_empresa_a_pedido_usa_a_ia_e_para_no_que_nao_esta_ligado() {
        let d = pasta("passo");
        let chamadas = Arc::new(AtomicUsize::new(0));
        let (mut rot, c2) = (rot_so_online(Arc::new(AtomicUsize::new(0))), chamadas.clone());
        rot.chamar = Box::new(move |_, _, _, _| {
            c2.fetch_add(1, Ordering::SeqCst);
            Ok("Foto de pão quenexemplo saindo do forno, luz quente da manhã, balcão de madeira, sem texto na imagem.".into())
        });
        let rot = Arc::new(rot);
        let (ts, amb) = (fatos::agora_ts(), Ambiente::seguro(&d));
        let c = contexto::Ctx::ler(&d);
        let p = Pedido { origem: "pc", conv: jarvis::CONV_UNICA, original: "", armada: false, ctx: &c, familia: &[] };
        let abrir = |nome: &str, v: Value| executor::executar(&amb, &p, nome, &v).unwrap().0;
        assert!(passo_empresa(&d, &rot, "inv", &em(10), ts).contains("não tem passo") && passo_empresa(&d, &rot, "nada", &em(10), ts).contains("não tem passo"));
        // NexoDev: UM passo por vez; commit e build agora estao ligados (nexodev.rs), entao a entrega segue para a revisao
        assert!(abrir("nexodev", json!({ "acao": "abrir", "projeto": "loja", "titulo": "Tela de estoque", "descricao": "lista de itens" })).starts_with("T001"));
        let r = passo_empresa(&d, &rot, "dev", &em(10), ts);
        let t = empresa_dev::tickets(&d).remove(0);
        assert!(r.contains("T001") && t["estado"] == "revisao" && t["parado"].is_null(), "{r} {t}");
        let entregas = std::fs::read_dir(d.join("empresa_dev").join("projetos").join("loja")).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".md")).count();
        assert_eq!((entregas, chamadas.load(Ordering::SeqCst)), (1, 1));
        // o passo seguinte e a revisao (uma IA): resposta que nao comeca com APROVADO reprova e o ticket volta ao dev
        assert!(passo_empresa(&d, &rot, "dev", &em(10), ts).contains("reprovado na revisão"));
        assert_eq!(chamadas.load(Ordering::SeqCst), 2);
        assert_eq!(esperando_ok(&d), json!({ "dev": [], "inv": [], "cortes": [] }));
        // Fornalha: o passo do prompt usa a IA; o de gerar a arte depende do navegador e para com o motivo no historico
        std::fs::create_dir_all(d.join("empresa_mkt")).unwrap();
        std::fs::write(d.join("empresa_mkt").join("equipe.json"), r#"{"empresa": "Fornalha", "agentes": [], "limites": {}, "clientes": {"padaria": {"marca": "Padaria"}}}"#).unwrap();
        assert!(abrir("marketing", json!({ "acao": "abrir", "cliente": "Padaria", "tipo": "imagem", "briefing": "pão quenexemplo" })).contains("P001"));
        let (a, b) = (passo_empresa(&d, &rot, "mkt", &em(10), ts), passo_empresa(&d, &rot, "mkt", &em(10), ts));
        let ped = empresa_mkt::pedidos(&d).remove(0);
        assert!(a.contains("→ prompt") && b.contains(executor::NAO_LIGADO) && ped["estado"] == "prompt", "{a} | {b} | {ped}");
        assert!(ped["historico"].as_array().unwrap().iter().any(|h| h["txt"].as_str().unwrap().contains(executor::NAO_LIGADO)), "{ped}");
        // nada saiu: nem caixa de saida
        assert!(saida::ultimas(&d, 9).as_array().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn esteira_pela_tela_so_comanda_a_fila_e_as_leituras_nao_criam_arquivo() {
        let d = pasta("esteira-tela");
        let ts = fatos::agora_ts();
        for acao in ["mt", "refazer", "painel", "capitais", ""] {
            assert!(comando_esteira(&d, acao, 0, &em(10), ts).unwrap_err().starts_with("acao:"), "{acao}");
        }
        assert!(comando_esteira(&d, "status", 0, &em(10), ts).unwrap().starts_with("Esteira"));
        let r = comando_esteira(&d, "parar", 0, &em(10), ts).unwrap();
        assert!(r.starts_with('⏸') && r.ends_with(SEM_ORQUESTRADOR), "{r}");
        assert!(comando_esteira(&d, "continuar", 0, &em(10), ts).unwrap().ends_with(SEM_ORQUESTRADOR));
        // rede: sem arquivo nada e criado; com arquivo, o segredo nao aparece
        assert_eq!(pares_da_rede(&d), json!({ "pares": [], "registro": [] }));
        assert!(!d.join("rede.json").exists());
        std::fs::write(d.join("rede.json"), r#"{"id": "ab12", "pares": {"x": {"nome": "Bia", "ip": "26.0.0.2", "recebe": true, "segredo": "SEGREDO-XYZ"}}, "log": ["oi"]}"#).unwrap();
        let rede = pares_da_rede(&d);
        assert!(rede["pares"][0]["nome"] == "Bia" && rede["registro"] == json!(["oi"]) && !rede.to_string().contains("SEGREDO"), "{rede}");
        // seguranca: o relatorio mais novo ja gravado; sem nenhum, nulo
        assert!(ultimo_relatorio_seguranca(&d)["relatorio"].is_null());
        std::fs::create_dir_all(d.join("relatorios")).unwrap();
        for (nome, t) in [("seguranca-2026-11-10.md", "velho"), ("seguranca-2026-11-17.md", "novo"), ("zzz-notas.md", "outro")] {
            std::fs::write(d.join("relatorios").join(nome), t).unwrap();
        }
        let s = ultimo_relatorio_seguranca(&d);
        assert_eq!((s["arquivo"].as_str(), s["relatorio"].as_str()), (Some("seguranca-2026-11-17.md"), Some("novo")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn ocioso_com_a_volta_do_contador_e_abrir_recusa_programa() {
        assert_eq!((ocioso_de(65_000, 5_000), ocioso_de(1_000, u32::MAX - 999)), (60, 2));
        assert!(segundos_ocioso() >= -1);
        let d = pasta("abrir");
        std::fs::write(d.join("x.bat"), "echo oi").unwrap();
        for alvo in [d.join("x.bat"), d.join("nao-existe.txt")] {
            assert!(abrir_no_pc(&alvo.to_string_lossy()).unwrap_err().starts_with("só abro"), "{alvo:?}");
        }
        assert!(abrir_no_pc("ftp://exemplo.com/x").is_err() && abrir_no_pc("calc.exe").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn voz_por_agente_e_escolhida_pelo_catalogo_sem_internet() {
        let d = pasta("vozes");
        let cat = com_fontes(|f| vozes_catalogo::catalogo(&d, f));
        let gemini = cat["motores"].as_array().unwrap().iter().find(|m| m["id"] == "gemini").unwrap()["vozes"].as_array().unwrap().clone();
        let de = |g: &str| gemini.iter().find(|v| v["genero"] == g).unwrap()["id"].as_str().unwrap().to_string();
        let escolher = |agente: &str, voz: &str, pos| com_fontes(|f| vozes_catalogo::escolher(&d, f, "gemini", voz, agente, pos));
        assert_eq!(escolher("alma", &de("F"), 1).unwrap()["motor"], "gemini");
        assert!(escolher("alma", &de("M"), 2).unwrap_err().contains("feminina") && escolher("ninguem", &de("F"), 1).is_err());
        // Kokoro e Piper nao estao instalados aqui: nao da para escolher
        assert!(com_fontes(|f| vozes_catalogo::escolher(&d, f, "piper", "faber", "jarvis", 1)).is_err());
        assert_eq!(escolher("jarvis", &de("M"), 2).unwrap()["reservas"].as_array().unwrap().len(), 1);
        assert!(vozes_agentes::tirar_reserva(&d, "jarvis", 2).unwrap()["reservas"].is_null());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn status_tem_o_que_o_escritorio_e_o_painel_esperam() {
        let d = pasta("status");
        let rot = rot_so_online(Arc::new(AtomicUsize::new(0)));
        let ts = fatos::agora_ts();
        let s = status(&d, &rot, ts);
        assert!(s["ligado"] == true && s["pausado"] == false && s["mudo"] == false && s["atendimento_auto"]["ligado"] == false && s["autonomia"]["armada"] == false, "{s}");
        assert!(s["escuta_voz"]["estado"] == "parada" && s["microfone"] == "não ligado" && s["email"]["logado"] == false && s["provedores"][0]["hoje"] == 0, "{s}");
        power::desligar(&d, "manual", "");
        mudo::mutar(&d, None, ts);
        let s = status(&d, &rot, ts);
        assert!(s["ligado"] == false && s["pausado"] == true && s["mudo"] == true);
        // o painel le dai sem tropecar
        assert!(!painel::bracos(&d, &s, &em(10), ts).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn depois_do_turno_e_relogio_so_gravam_no_pc_e_na_caixa_de_saida() {
        let d = pasta("depois");
        let chamadas = Arc::new(AtomicUsize::new(0));
        let rot = rot_so_online(chamadas.clone());
        let ts = fatos::agora_ts();
        // conquista: a Alma reage; sem modelo local vale o texto pronto, e ele vai para a caixa de saida
        depois(&d, &rot, "conv-teste", "Consegui, fechei o contrato hoje!", "Que bom!", &em(10), ts);
        assert_eq!(memoria::resumo(&d).0, 2);
        assert!(canais(&d).iter().any(|c| c == "alma"), "{:?}", canais(&d));
        assert_eq!(travar(&COMANDOS).0.iter().filter(|c| *c == "conv-teste").count(), 1);
        // relogio na hora do resumo (13h): fila do funil e recado pendente saem para a caixa, nada mais
        assert_eq!(funil::passar(&d, "a rotina da madrugada terminou", "teste", &em(12), ts), "fila");
        assert!(recados::enfileirar(&d, "Ana", "oi", "já aviso", "", ts));
        let mut ultimo = 0;
        relogio(&d, &rot, &em(13), ts, 0, &mut ultimo);
        let c = canais(&d);
        assert!(c.iter().any(|x| x == "funil") && c.iter().any(|x| x == "funil-audio") && c.iter().any(|x| x == "recado"), "{c:?}");
        assert!(recados::pendentes(&d).is_empty() && d.join("alma").join("atividade.jsonl").exists());
        assert!(travar(&BRAIN).is_some());
        assert_eq!(chamadas.load(Ordering::SeqCst), 0, "nenhuma IA online pode ser chamada pelas ligacoes locais");
        let _ = std::fs::remove_dir_all(&d);
    }
}
