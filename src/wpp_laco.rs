//! Laco do atendimento do WhatsApp de verdade (o que vigia.py + atender.py + dono.py fazem juntos), atras de uma CHAVE que nasce DESLIGADA.
//! Chave: dados/troca.json {"whatsapp": false}; GET/POST /api/troca. So o dono liga (POST {"whatsapp": true}); nada neste codigo liga sozinho.
//! A cada 10 s, um ciclo: 1) le a LISTA de conversas; 2) chat do dono primeiro e sempre (`dono::tratar_dono`); 3) `vigia::ciclo` escolhe quem
//! atender (estavel, janela, tetos, anti-laco) e `liga_atendimento::rodar` decide pelas regras ja portadas.
//! Chave DESLIGADA = modo SOMBRA: so le a lista, NUNCA abre conversa (abrir marca como lida no WhatsApp real), decide pela previa e grava
//! em crate::saida o que responderia. Nada e enviado, nem ao chat do dono. Chave LIGADA = abre, le e responde de verdade (texto).
//! Audio (enviar e ouvir) vem de wpp_midia.rs; chave LIGADA = fala e ouve de verdade (o pai do dono so recebe audio). Enviar arquivo existe
//! em wpp_midia mas nenhuma regra do laco o chama ainda. Na SOMBRA "enviar audio" so vira registro na caixa de saida.
//! Chave restrita "so o dono": dados/troca.json {"whatsapp": false, "whatsapp_so_dono": true} atende DE VERDADE (texto e audio) so o chat do
//! proprio dono; todo o resto continua em sombra. Serve para o dono testar ao vivo sem risco para terceiros. Nasce false; nada liga sozinho.
use crate::alma_cuida::Nomes;
use crate::atender_laco::Conversa;
use crate::contexto::Ctx;
use crate::dono::{self, Lido};
use crate::roteador::Roteador;
use crate::wpp_vivo::{self as wv, Pagina};
use crate::{guia_pai, liga_atendimento, power, saida, vigia, wpp_midia as wm, Resp};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

pub(crate) static TRAVA: Mutex<()> = Mutex::new(());
static ULTIMO: Mutex<Option<Value>> = Mutex::new(None);
static LEU_EM: Mutex<Option<HashMap<String, f64>>> = Mutex::new(None);

const AVISO_AUDIO: &str = "[áudio NÃO enviado: modo sombra]";

/// Voz do laco: texto -> WAV (por agente) e audio -> texto (LOCAL). Em producao vem do roteador; nos testes, de mentira.
pub struct Voz<'a> {
    pub falar: &'a dyn Fn(&str, &str) -> Result<Vec<u8>, String>,
    pub transcrever: &'a dyn Fn(&[u8]) -> Result<String, String>,
    /// Teto diario do agente de programacao (pedido de construcao do dono, resposta tardia): ainda pode? / gastar um uso.
    pub pode_agente: &'a dyn Fn() -> bool,
    pub usar_agente: &'a dyn Fn(),
    /// (tipo, prompt, pasta de entrega): roda em segundo plano e entrega pela caixa de saida.
    pub agente: &'a dyn Fn(&str, &str, Option<&Path>),
}

/// A chave. Sem arquivo, ou com qualquer coisa diferente de true: DESLIGADA.
pub fn ligada(dados: &Path) -> bool {
    std::fs::read_to_string(dados.join("troca.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).is_some_and(|v| v["whatsapp"] == true)
}

/// Chave generica de `dados/troca.json` (telegram, airbnb...). Sem arquivo, ou com qualquer coisa diferente de true: DESLIGADA.
pub fn chave(dados: &Path, nome: &str) -> bool {
    std::fs::read_to_string(dados.join("troca.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).is_some_and(|v| v[nome] == true)
}

/// Liga/desliga UMA chave de canal sem apagar as outras (so o dono chama, pelo POST). Ao LIGAR apaga o estado que a sombra deixou.
pub fn definir_chave(dados: &Path, nome: &str, ligar: bool) -> Result<Value, String> {
    if ligar && !chave(dados, nome) {
        if let Some(f) = match nome { "airbnb" => Some("airbnb_escalada.json"), "telegram" => Some("telegram_estado.json"), _ => None } {
            let _ = std::fs::remove_file(dados.join(f));
        }
    }
    let mut v: Value = std::fs::read_to_string(dados.join("troca.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}));
    v[nome] = json!(ligar);
    std::fs::write(dados.join("troca.json"), v.to_string()).map_err(|e| format!("nao gravei a chave: {e}"))?;
    Ok(estado(dados))
}

/// A chave restrita: atende de verdade SO o chat do dono. Sem arquivo, ou com qualquer coisa diferente de true: DESLIGADA.
pub fn so_dono(dados: &Path) -> bool {
    std::fs::read_to_string(dados.join("troca.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).is_some_and(|v| v["whatsapp_so_dono"] == true)
}

/// Quem ja conhece o Jarvis no Braniac antigo: sem isto o B.R.A.N.I se apresentaria de novo a todo mundo. So le o arquivo do antigo.
pub fn importar_apresentados(dados: &Path, origem: &Path) -> usize {
    let lista = |v: &Value| -> Vec<String> { v["apresentados"].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(String::from)).collect() };
    let Some(antigos) = std::fs::read_to_string(origem).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).map(|v| lista(&v)) else { return 0 };
    let arq = dados.join("contexto.json");
    let mut c: Value = std::fs::read_to_string(&arq).ok().and_then(|t| serde_json::from_str(&t).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}));
    let mut atuais = lista(&c);
    let antes = atuais.len();
    for a in antigos {
        if !atuais.contains(&a) {
            atuais.push(a);
        }
    }
    c["apresentados"] = json!(atuais);
    let _ = std::fs::write(&arq, c.to_string());
    atuais.len() - antes
}

/// Liga ou desliga (cada chave e opcional: None = nao mexe). Ao LIGAR: apaga o estado do modo sombra (senao o que a sombra "tratou" nunca seria respondido de verdade) e traz
/// os apresentados do Braniac antigo (BRANIAC_ENV aponta para o .env dele; so leitura).
pub fn definir(dados: &Path, ligar: Option<bool>, so_dono_novo: Option<bool>) -> Result<Value, String> {
    if ligar == Some(true) && !ligada(dados) {
        for f in ["vigia_estado.json", "dono_estado.json"] {
            let _ = std::fs::remove_file(dados.join(f));
        }
        if let Some(origem) = std::env::var_os("BRANIAC_ENV").map(|e| Path::new(&e).with_file_name("data").join("contexto.json")) {
            importar_apresentados(dados, &origem);
        }
    }
    if so_dono_novo == Some(true) && !so_dono(dados) {
        let _ = std::fs::remove_file(dados.join("dono_estado.json")); // o que a sombra "tratou" no chat do dono tem de ser respondido de verdade
    }
    let mut chave: Value = std::fs::read_to_string(dados.join("troca.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}));
    chave["whatsapp"] = json!(ligar.unwrap_or_else(|| ligada(dados)));
    chave["whatsapp_so_dono"] = json!(so_dono_novo.unwrap_or_else(|| so_dono(dados)));
    std::fs::write(dados.join("troca.json"), chave.to_string()).map_err(|e| format!("nao gravei a chave: {e}"))?;
    Ok(estado(dados))
}

pub fn estado(dados: &Path) -> Value {
    let (vivo, dono) = (ligada(dados), so_dono(dados));
    json!({ "whatsapp": vivo, "whatsapp_so_dono": dono, "telegram": chave(dados, "telegram"), "airbnb": chave(dados, "airbnb"), "voz": chave(dados, "voz"), "portal": chave(dados, "portal"), "loja": chave(dados, "loja"), "link": chave(dados, "link"),
        "telegram_estado": crate::telegram_laco::estado(), "airbnb_estado": crate::airbnb_laco::estado(),
        "modo": if vivo { "vivo: abre conversas e responde de verdade (texto e audio)" } else if dono { "so o dono: atende de verdade o chat do dono; o resto em sombra" }
            else { "sombra: so le a lista e grava na caixa de saida" },
        "ultimo_ciclo": ULTIMO.lock().ok().and_then(|u| u.clone()).unwrap_or(Value::Null),
        "fora": ["ver imagem", "enviar arquivo pelo laco (a funcao existe em wpp_midia, nenhuma regra a chama)", "contato que nao esta na lista de conversas",
            "audio e arquivo: portados, ainda sem teste na pagina real do WhatsApp"] })
}

fn trava_de_envio(texto: &str, nome: &str, nomes: &Nomes) -> Option<String> {
    guia_pai::eh_pai(nome, nomes).then(|| format!("⛔ Texto barrado: o pai do dono só recebe áudio. ({} letras)", texto.chars().count()))
}

/// UM ciclo. `vivo` = a chave estava ligada quando o ciclo comecou. `ia` e `responder` vem de quem chama (o roteador em producao).
#[allow(clippy::too_many_arguments)]
pub fn ciclo(dados: &Path, agora: &crate::tempo::Agora, ts: f64, vivo: bool, cfg_rot: &Value, pag: &mut dyn Pagina, ia: liga_atendimento::Ia,
             voz: &Voz, responder: &dyn Fn(&str, &str) -> Result<(String, bool), String>) -> Result<Value, String> {
    let (ctx, nomes, eu) = (Ctx::ler(dados), Nomes::ler(dados), wv::nome_whatsapp(dados));
    let itens = wv::linhas(pag)?;
    if itens.is_empty() {
        return Err("a lista de conversas veio vazia (carregando ou deslogado)".into());
    }
    let lista: Vec<(String, i64)> = itens.iter().map(|i| (i.nome.clone(), i.nl)).collect();
    let linhas: Vec<crate::atender::Linha> = itens.iter().map(wv::Item::linha).collect();
    let pag = RefCell::new(pag);
    // a chave e conferida DE NOVO a cada envio: desligar no meio do ciclo corta o resto
    let real = || vivo && ligada(dados);
    let chat_dono = ctx.chat_do_dono().map(String::from);
    // "so o dono": o chat do dono e o UNICO que age de verdade enquanto a chave geral esta desligada
    let real_nome = |n: &str| real() || (so_dono(dados) && chat_dono.as_deref() == Some(n));
    let texto = |n: &str, t: &str| -> String {
        if let Some(b) = trava_de_envio(t, n, &nomes) {
            saida::registrar(dados, "whatsapp-barrado", n, &format!("{b} | {t}"));
            return b;
        }
        if !real_nome(n) {
            return saida::registrar(dados, "whatsapp", n, t);
        }
        let r = wv::enviar_texto(&mut **pag.borrow_mut(), dados, &ctx, &nomes, n, t);
        if !r.starts_with('✅') {
            saida::registrar(dados, "whatsapp-barrado", n, &format!("{r} | {t}"));
        }
        r
    };
    let audio = |n: &str, t: &str| -> String {
        if !real_nome(n) {
            saida::registrar(dados, "whatsapp-audio", n, &format!("{AVISO_AUDIO} {t}"));
            return format!("áudio não enviado: modo sombra ({n})");
        }
        let agente = if guia_pai::eh_pai(n, &nomes) { "zelo" } else { "jarvis" };
        let r = match (voz.falar)(agente, t) {
            Ok(wav) => wm::enviar_audio(&mut **pag.borrow_mut(), dados, &ctx, &nomes, n, &wav),
            Err(e) => format!("Áudio não gerado ({e}). Nada foi enviado."),
        };
        if !r.starts_with('✅') {
            saida::registrar(dados, "whatsapp-audio-barrado", n, &format!("{r} | {t}"));
        }
        r
    };
    let ao_dono = |t: &str| {
        if let Some(c) = &chat_dono {
            if real_nome(c) { texto(c, t); } else { saida::registrar(dados, "dono", c, t); }
        }
    };
    let (mut respondeu_dono, mut abriu) = (false, 0usize);

    // 1) o chat do dono, primeiro e sempre
    let ler_dono = |nome: &str, sig_ant: Option<&str>| -> Option<Lido> {
        let alvo = itens.iter().find(|i| i.nome == nome)?;
        let sig = format!("{}|{}", alvo.nl, alvo.resumo);
        let presa = regex::Regex::new(r"(?i)digitando|gravando").is_ok_and(|r| r.is_match(&alvo.resumo))
            && ts - LEU_EM.lock().ok().and_then(|m| m.as_ref().and_then(|m| m.get(nome).copied())).unwrap_or(0.0) > 45.0;
        if Some(sig.as_str()) == sig_ant && !presa {
            return None;
        }
        if let Ok(mut m) = LEU_EM.lock() {
            m.get_or_insert_with(HashMap::new).insert(nome.to_string(), ts);
        }
        if !real_nome(nome) {
            return Some(Lido { msgs: wv::conversa_da_previa(alvo, agora).msgs, lista_sig: sig, ..Default::default() });
        }
        let mut pg = pag.borrow_mut();
        if !wv::abrir(&mut **pg, nome) {
            return None;
        }
        pg.dormir(1500);
        let (c, n_audios) = wv::ler_conversa(&mut **pg, 14, &eu).ok()?;
        let alvo2 = wv::linhas(&mut **pg).ok().and_then(|l| l.into_iter().find(|i| i.nome == nome)).unwrap_or_else(|| alvo.clone());
        Some(Lido { msgs: c.msgs, midias: c.midias, lista_sig: format!("{}|{}", alvo2.nl, alvo2.resumo), audio_dele: c.audio_dele, n_audios })
    };
    let nao_ligado = |o: &str| format!("{o}: ainda não ligado no Braniac em Rust");
    let r = dono::tratar_dono(dados, agora, ts, &dono::Portas {
        ler: &ler_dono, responder, revisar: None,
        enviar: &|n, t| if real_nome(n) { texto(n, t) } else { saida::registrar(dados, "dono", n, t) },
        falar: &|n, t| real_nome(n) && audio(n, t).starts_with('✅'), // sombra: a regra cai no texto
        ouvir: &|n, k| if real_nome(n) { wm::ouvir_contato(&mut **pag.borrow_mut(), dados, &ctx, &nomes, n, k, ts, voz.transcrever) } else { Err("sombra: não ouvi o áudio".into()) },
        foi_nosso: &|t| wv::foi_nosso(dados, t),
        // so com o chat do dono REAL (chave ligada ou "so o dono"): em sombra nada aciona o agente
        pode_agente: &|| chat_dono.as_deref().is_some_and(real_nome) && (voz.pode_agente)(), usar_agente: voz.usar_agente, agente: voz.agente,
        baixar: &|_, _| Err(nao_ligado("baixar arquivo")), observar: &|_| (),
    });
    if r.is_some() {
        respondeu_dono = true;
    }

    // 2) os demais, pelo vigia
    let mut feito = vigia::Feito::default();
    if !power::pausado(dados) {
        let cfg = vigia::cfg_de(&cfg_rot["atendimento_auto"]);
        let so_prio = !vigia::dentro_da_janela(&cfg, agora);
        let abrir_conv = |nome: &str| -> Option<Conversa> {
            if !real_nome(nome) {
                return itens.iter().find(|i| i.nome == nome).map(|i| wv::conversa_da_previa(i, agora));
            }
            let mut pg = pag.borrow_mut();
            if !wv::abrir(&mut **pg, nome) {
                return None;
            }
            pg.dormir(1500);
            wv::ler_conversa(&mut **pg, 12, &eu).ok().map(|(c, _)| c)
        };
        let aberturas = std::cell::Cell::new(0usize);
        let rodar = |nomes_v: &[String]| {
            let abrir_contando = |n: &str| {
                // so conta abertura real: conversa lida pela previa em sombra nao abre nada no WhatsApp
                if real_nome(n) {
                    aberturas.set(aberturas.get() + 1);
                }
                abrir_conv(n)
            };
            liga_atendimento::rodar(dados, agora, ts, ia, &lista, &linhas, Some(nomes_v), &abrir_contando, &texto, &audio, &ao_dono,
                &|n| if real_nome(n) { wm::ouvir_contato(&mut **pag.borrow_mut(), dados, &ctx, &nomes, n, 1, ts, voz.transcrever) } else { Err("sombra: não ouvi o áudio".into()) },
                &|n| if real_nome(n) { wv::abrir_destino(&mut **pag.borrow_mut(), &ctx, &nomes, n, false).and_then(|_| crate::visao_local::ver(&mut **pag.borrow_mut())) } else { Err("sombra: não vi a imagem".into()) }, true).1
        };
        feito = vigia::ciclo(dados, &cfg, &linhas, agora, ts, so_prio, &rodar);
        abriu = aberturas.get();
    }
    Ok(json!({ "modo": if vivo { "vivo" } else { "sombra" }, "conversas": itens.len(), "nao_lidas": itens.iter().map(|i| i.nl).sum::<i64>(),
        "dono_respondido": respondeu_dono, "candidatos": feito.candidatos.len(), "enviados": feito.enviados.len(), "conversas_abertas": abriu + usize::from(respondeu_dono && vivo) }))
}

/// Uma volta com a aba de verdade e o roteador. Erro (VM fora, aba fechada, lista vazia) so fica no estado.
pub fn uma_volta(dados: &Path, rot: &Arc<Roteador>) -> Result<Value, String> {
    let _g = TRAVA.lock().map_err(|_| "travado".to_string())?;
    let vivo = ligada(dados);
    let mut pag = wv::Real::conectar(dados)?;
    let (agora, ts) = (crate::tempo::agora(), crate::fatos::agora_ts());
    let ia = |sis: &str, pedido: &str, so_local: bool| rot.neutro_em(sis, pedido, so_local).ok();
    let responder = |limpo: &str, _: &str| -> Result<(String, bool), String> {
        // origem "whatsapp" no chat verificado do dono (DONO_CONV): as travas de envio e de autonomia valem como no Python
        let r = crate::conversar_de(dados, rot, crate::jarvis_travas::DONO_CONV, limpo, true, &agora, ts, "whatsapp");
        Ok((r["resposta"].as_str().unwrap_or("").to_string(), r["provedor"] != "agentes"))
    };
    let falar = |agente: &str, t: &str| {
        let nuvem = !rot.sensivel(t); // texto sensivel nunca vai a voz da nuvem
        let estilo = crate::vozes_agentes::escolha(dados, agente)["ajustes"]["estilo"].as_str().unwrap_or("").to_string();
        crate::vozes_agentes::falar(dados, &agora, agente, t, nuvem, &|m, v, tx| crate::voz::gerar(&*rot.ler_chave, m, v, tx, &estilo)).map(|f| f.wav)
            .or_else(|_| crate::voz::falar_wav(dados, &*rot.ler_chave, t, nuvem, agente).map(|(w, _)| w))
    };
    let transcrever = |b: &[u8]| crate::ouvir::transcrever(dados, b, "ogg"); // LOCAL (Whisper); o audio nunca sai do PC
    let teto = rot.cfg["claude_max_por_dia"].as_i64().unwrap_or(20);
    let pode = || rot.store.uso_hoje("claude") < teto;
    let usar = || drop(rot.store.add_uso("claude"));
    let agente = |tipo: &str, prompt: &str, pasta: Option<&Path>| crate::dono_agente::disparar_whatsapp(dados, tipo, prompt, pasta);
    ciclo(dados, &agora, ts, vivo, &rot.cfg, &mut pag, &ia, &Voz { falar: &falar, transcrever: &transcrever, pode_agente: &pode, usar_agente: &usar, agente: &agente }, &responder)
}

/// A thread do laco: sempre roda (sombra ou viva), de 10 em 10 s. A chave decide se o que ela decide sai de verdade.
pub fn iniciar(rot: Arc<Roteador>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(10));
        let dados = crate::pasta_dados();
        let r = uma_volta(&dados, &rot);
        if let Ok(mut u) = ULTIMO.lock() {
            *u = Some(match r {
                Ok(v) => json!({ "ts": crate::fatos::agora_ts(), "ok": v }),
                Err(e) => json!({ "ts": crate::fatos::agora_ts(), "erro": e }),
            });
        }
    });
}

pub fn get(dados: &Path, url: &str) -> Option<Resp> {
    match url.split('?').next() {
        Some("/api/troca") => Some(crate::dado(estado(dados))),
        // os botoes "atendimento automatico" do painel: o que mostram e a chave de verdade em dados/troca.json
        Some("/api/atendimento-auto") => Some(crate::dado(json!({ "ligado": ligada(dados), "chave": "whatsapp" }))),
        Some("/api/airbnb-auto") => Some(crate::dado(json!({ "ligado": chave(dados, "airbnb"), "chave": "airbnb", "obs": AIRBNB_OBS }))),
        Some("/api/telegram") => Some(crate::dado(json!({ "robo": crate::telegram_laco::estado(), "web": crate::telegram_laco::web(dados) }))),
        _ => None,
    }
}

pub const AVISO_ANTIGO: &str = "O Braniac antigo ainda está ligado e atendendo. Desligue ele primeiro (passo a passo da troca) para não responderem em dobro.";
pub const AIRBNB_OBS: &str = "No B.R.A.N.I esta chave so LE a caixa do Airbnb e AVISA o dono e a Tia Mae; nunca responde hospede nem aceita reserva.";
/// Canais que o Braniac antigo tambem atende: ligar um deles com o antigo no ar faria os DOIS responderem as mesmas pessoas.
/// `whatsapp_so_dono` (teste do dono) e desligar qualquer chave ficam de fora da trava.
const CANAIS_DO_ANTIGO: [&str; 5] = ["whatsapp", "telegram", "airbnb", "voz", "portal"];

/// O Braniac antigo esta atendendo? (porta 8765 respondendo)
pub fn antigo_no_ar() -> bool {
    std::net::TcpStream::connect_timeout(&std::net::SocketAddr::from(([127, 0, 0, 1], 8765)), std::time::Duration::from_millis(800)).is_ok()
}

/// Some(mensagem) = recusado: o pedido liga um canal do antigo e o antigo esta no ar. `{"forcar": true}` (so para a hora da troca) passa.
pub fn trava_antigo(v: &Value, antigo: &dyn Fn() -> bool) -> Option<String> {
    (v["forcar"] != true && CANAIS_DO_ANTIGO.iter().any(|c| v[*c] == true) && antigo()).then(|| AVISO_ANTIGO.to_string())
}

/// POST do botao: corpo {"ligado": bool}. Liga/desliga a chave `canal` (whatsapp ou airbnb).
fn botao(dados: &Path, canal: &str, v: &Value, antigo: &dyn Fn() -> bool) -> Resp {
    let Some(lig) = v["ligado"].as_bool() else { return crate::erro("mande {\"ligado\": true|false}", 400) };
    if lig {
        if let Some(m) = trava_antigo(&json!({ canal: true, "forcar": v["forcar"] }), antigo) {
            return crate::erro(m, 409);
        }
    }
    let r = if canal == "whatsapp" { definir(dados, Some(lig), None) } else { definir_chave(dados, canal, lig) };
    r.map(|_| crate::dado(json!({ "ligado": chave(dados, canal) }))).unwrap_or_else(|e| crate::erro(e, 500))
}

pub fn post(dados: &Path, caminho: &str, v: &Value) -> Option<Resp> {
    post_com(dados, caminho, v, &antigo_no_ar)
}

pub fn post_com(dados: &Path, caminho: &str, v: &Value, antigo: &dyn Fn() -> bool) -> Option<Resp> {
    match caminho {
        "/api/atendimento-auto" => return Some(botao(dados, "whatsapp", v, antigo)),
        "/api/airbnb-auto" => return Some(botao(dados, "airbnb", v, antigo)),
        _ => {}
    }
    (caminho == "/api/troca").then(|| {
        if let Some(m) = trava_antigo(v, antigo) {
            return crate::erro(m, 409);
        }
        // loja e link ligam: quem faz a rede e a ponte temporaria em Python (ponte.rs); a trava e a do Python antigo no ar (a ponte se recusa a subir)
        let canais = ["telegram", "airbnb", "voz", "portal", "loja", "link"];
        if canais.iter().any(|c| !v[*c].is_null() && v[*c].as_bool().is_none()) {
            return crate::erro("os valores tem de ser true ou false", 400);
        }
        let achou_canal = canais.iter().any(|c| v[*c].is_boolean());
        for c in canais {
            if let Some(b) = v[c].as_bool() {
                if let Err(e) = definir_chave(dados, c, b) {
                    return crate::erro(e, 500);
                }
            }
        }
        match (v["whatsapp"].as_bool(), v["whatsapp_so_dono"].as_bool()) {
            (None, None) if achou_canal => crate::dado(estado(dados)),
            (None, None) => crate::erro("mande {\"whatsapp\": true|false} e/ou {\"whatsapp_so_dono\": true|false} (ou \"telegram\", \"airbnb\")", 400),
            (a, b) if (!v["whatsapp"].is_null() && a.is_none()) || (!v["whatsapp_so_dono"].is_null() && b.is_none()) => crate::erro("os valores tem de ser true ou false", 400),
            (a, b) => definir(dados, a, b).map(crate::dado).unwrap_or_else(|e| crate::erro(e, 500)),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::{Agora, Data};
    use crate::wpp_vivo::tests::{lista_de, tmp, Falsa};

    const TS: f64 = 1.8e9;

    /// Os testes antigos nao dependem de haver (ou nao) um Braniac antigo ouvindo na 8765 desta maquina: a trava tem teste proprio.
    fn post(d: &Path, c: &str, v: &Value) -> Option<Resp> {
        post_com(d, c, v, &|| false)
    }

    fn agora() -> Agora {
        Agora { data: Data::civil(2026, 10, 1).unwrap(), hora: 10, min: 0, seg: 0 }
    }

    fn lista_com_previas() -> String {
        json!([{"nome": "Ana", "nl": 2, "nossa": false, "resumo": "Ana | 09:55 | oi, tudo bem?"},
            {"nome": "Eu(D)", "nl": 1, "nossa": false, "resumo": "Eu(D) | 09:58 | me lembra de ligar para a loja"},
            {"nome": "Pai", "nl": 1, "nossa": false, "resumo": "Pai | 09:50 | filho, como estão as coisas?"}]).to_string()
    }

    fn decide(sis: &str, pedido: &str, _: bool) -> Option<String> {
        let _ = sis;
        let id = pedido.split('|').nth(1).unwrap_or("e1");
        Some(format!("R|{id}|E|Claro, já te respondo."))
    }

    fn sem_voz() -> Voz<'static> {
        Voz { falar: &|_, _| Err("sem voz".into()), transcrever: &|_| Err("sem ouvido".into()), pode_agente: &|| true, usar_agente: &|| (), agente: &|_, _, _| () }
    }

    fn sem_resposta_do_dono(_: &str, _: &str) -> Result<(String, bool), String> {
        Ok(("Anotado, senhor.".into(), true))
    }

    #[test]
    fn chave_nasce_desligada_e_so_liga_pelo_post() {
        let d = tmp("chave");
        assert!(!ligada(&d));
        assert_eq!(estado(&d)["whatsapp"], false);
        std::fs::write(d.join("troca.json"), "lixo").unwrap();
        assert!(!ligada(&d));
        assert_eq!(post(&d, "/api/troca", &json!({"whatsapp": "sim"})).unwrap().status_code().0, 400);
        definir(&d, Some(true), None).unwrap();
        assert!(ligada(&d));
        definir(&d, Some(false), None).unwrap();
        assert!(!ligada(&d));
    }

    #[test]
    fn ligar_apaga_o_estado_da_sombra() {
        let d = tmp("limpa");
        std::fs::write(d.join("vigia_estado.json"), "{}").unwrap();
        std::fs::write(d.join("dono_estado.json"), "{}").unwrap();
        definir(&d, Some(true), None).unwrap();
        assert!(!d.join("vigia_estado.json").exists() && !d.join("dono_estado.json").exists());
    }

    #[test]
    fn importa_apresentados_do_antigo_sem_duplicar() {
        let d = tmp("apres");
        std::fs::write(d.join("contexto.json"), r#"{"apresentados": ["ana"]}"#).unwrap();
        let o = d.join("antigo.json");
        std::fs::write(&o, r#"{"apresentados": ["ana", "bia", "caio"]}"#).unwrap();
        assert_eq!(importar_apresentados(&d, &o), 2);
        assert_eq!(importar_apresentados(&d, &o), 0);
        assert!(Ctx::ler(&d).apresentado("Caio"));
        assert_eq!(importar_apresentados(&d, &d.join("nao-existe.json")), 0);
    }

    #[test]
    fn trava_do_antigo_recusa_ligar_canal_com_ele_no_ar() {
        let (no_ar, fora) = (&|| true, &|| false);
        let codigo = |r: Option<Resp>| r.unwrap().status_code().0;
        for c in ["whatsapp", "telegram", "airbnb", "voz", "portal"] {
            assert_eq!(trava_antigo(&json!({ c: true }), no_ar).as_deref(), Some(AVISO_ANTIGO), "{c}");
            assert!(trava_antigo(&json!({ c: true }), fora).is_none(), "{c} com o antigo parado liga");
            assert!(trava_antigo(&json!({ c: false }), no_ar).is_none(), "desligar e sempre permitido: {c}");
        }
        assert!(trava_antigo(&json!({ "whatsapp_so_dono": true }), no_ar).is_none()); // teste do dono
        assert!(trava_antigo(&json!({ "whatsapp": true, "forcar": true }), no_ar).is_none()); // hora da troca
        assert!(AVISO_ANTIGO.contains("Braniac antigo") && AVISO_ANTIGO.contains("em dobro"));
        let d = tmp("trava_antigo");
        // POST /api/troca
        assert_eq!(codigo(post_com(&d, "/api/troca", &json!({ "whatsapp": true }), no_ar)), 409);
        assert!(!ligada(&d));
        assert_eq!(codigo(post_com(&d, "/api/troca", &json!({ "whatsapp_so_dono": true }), no_ar)), 200);
        assert!(so_dono(&d));
        // botoes do painel
        assert_eq!(codigo(post_com(&d, "/api/atendimento-auto", &json!({ "ligado": true }), no_ar)), 409);
        assert_eq!(codigo(post_com(&d, "/api/airbnb-auto", &json!({ "ligado": true }), no_ar)), 409);
        assert!(!ligada(&d) && !chave(&d, "airbnb"));
        assert_eq!(codigo(post_com(&d, "/api/atendimento-auto", &json!({}), no_ar)), 400);
        // com o antigo parado liga e desliga; o estado reflete a chave
        assert_eq!(codigo(post_com(&d, "/api/airbnb-auto", &json!({ "ligado": true }), fora)), 200);
        assert!(chave(&d, "airbnb"));
        assert_eq!(get(&d, "/api/airbnb-auto").unwrap().status_code().0, 200);
        assert_eq!(codigo(post_com(&d, "/api/atendimento-auto", &json!({ "ligado": true }), fora)), 200);
        assert!(ligada(&d) && so_dono(&d));
        // desligar com o antigo no ar passa; forcar so na hora da troca
        assert_eq!(codigo(post_com(&d, "/api/atendimento-auto", &json!({ "ligado": false }), no_ar)), 200);
        assert!(!ligada(&d) && so_dono(&d));
        assert_eq!(codigo(post_com(&d, "/api/airbnb-auto", &json!({ "ligado": false }), no_ar)), 200);
        assert_eq!(codigo(post_com(&d, "/api/atendimento-auto", &json!({ "ligado": true, "forcar": true }), no_ar)), 200);
        assert!(ligada(&d));
    }

    #[test]
    fn modo_sombra_nunca_abre_nem_digita_e_decide_na_caixa_de_saida() {
        let d = tmp("sombra");
        let mut p = Falsa { lista: lista_com_previas(), titulo: "Ana".into(), caixa: true, ..Default::default() };
        let cfg = json!({});
        let mut feitos = 0;
        for volta in 0..4 {
            let r = ciclo(&d, &agora(), TS + 30.0 * volta as f64, false, &cfg, &mut p, &decide, &sem_voz(), &sem_resposta_do_dono).unwrap();
            assert_eq!(r["modo"], "sombra");
            feitos += r["candidatos"].as_u64().unwrap();
            // conversa lida so pela previa em sombra nao e abertura: nada foi aberto no WhatsApp
            assert_eq!(r["conversas_abertas"], 0, "volta {volta}");
        }
        assert!(feitos > 0, "a sombra deveria ter decidido sobre alguma conversa");
        // nenhum gesto: nem clique, nem digitar, nem Enter, nem trazer a aba para a frente, nem JS fora da lista
        assert!(p.gestos.is_empty(), "{:?}", p.gestos);
        let saidas = saida::ultimas(&d, 50);
        assert!(!saidas.as_array().unwrap().is_empty());
        // o pai nao recebe texto nem na sombra
        assert!(saidas.as_array().unwrap().iter().all(|s| !(s["canal"] == "whatsapp" && s["destino"] == "Pai")));
    }

    #[test]
    fn chave_desligada_nada_sai_mesmo_com_o_ciclo_pedindo_vivo_depois_de_desligar() {
        let d = tmp("desliga");
        let mut p = Falsa { lista: lista_com_previas(), titulo: "Ana".into(), caixa: true, ..Default::default() };
        // o ciclo "comecou vivo", mas a chave no arquivo esta desligada: tem de valer o arquivo
        assert!(!ligada(&d));
        for volta in 0..4 {
            ciclo(&d, &agora(), TS + 30.0 * volta as f64, true, &json!({}), &mut p, &decide, &sem_voz(), &sem_resposta_do_dono).unwrap();
        }
        assert!(p.gestos.is_empty(), "{:?}", p.gestos);
    }

    #[test]
    fn lista_vazia_e_erro_e_nao_decide() {
        let d = tmp("vazia");
        let mut p = Falsa { lista: "[]".into(), ..Default::default() };
        assert!(ciclo(&d, &agora(), TS, false, &json!({}), &mut p, &decide, &sem_voz(), &sem_resposta_do_dono).is_err());
        let _ = lista_de(&[]);
    }

    #[test]
    fn modo_vivo_abre_le_e_responde_so_com_a_chave_ligada_e_o_nome_conferido() {
        let d = tmp("vivo");
        definir(&d, Some(true), None).unwrap();
        let msgs = json!({"msgs": [{"d": "ELE", "c": "[09:55, 01/10/2026] Ana: ", "t": "oi, tudo bem?", "links": []}], "midias": []}).to_string();
        let lista = json!([{"nome": "Ana", "nl": 1, "nossa": false, "resumo": "Ana | 09:55 | oi, tudo bem?"}]).to_string();
        let mut p = Falsa { lista, titulo: "Ana".into(), caixa: true, msgs: msgs.clone(), ..Default::default() };
        let mut enviados = 0;
        for volta in 0..4 {
            let r = ciclo(&d, &agora(), TS + 30.0 * volta as f64, true, &json!({}), &mut p, &decide, &sem_voz(), &sem_resposta_do_dono).unwrap();
            enviados += r["enviados"].as_u64().unwrap();
        }
        assert!(enviados >= 1, "{:?}", saida::ultimas(&d, 10));
        assert!(p.gestos.iter().any(|g| g == "mouse:mousePressed") && p.gestos.iter().any(|g| g == "enter"), "{:?}", p.gestos);
        // nome errado no topo: abre, mas NAO digita
        let d2 = tmp("vivo2");
        definir(&d2, Some(true), None).unwrap();
        let mut p2 = Falsa { lista: json!([{"nome": "Ana", "nl": 1, "nossa": false, "resumo": "Ana | 09:55 | oi, tudo bem?"}]).to_string(), titulo: "Bia".into(), caixa: true, msgs, ..Default::default() };
        for volta in 0..4 {
            let _ = ciclo(&d2, &agora(), TS + 30.0 * volta as f64, true, &json!({}), &mut p2, &decide, &sem_voz(), &sem_resposta_do_dono);
        }
        assert!(!p2.gestos.iter().any(|g| g.starts_with("inserir") || g == "enter"), "{:?}", p2.gestos);
    }

    #[test]
    fn chave_so_o_dono_nasce_desligada_e_convive_com_a_geral() {
        let d = tmp("so-dono-chave");
        assert!(!so_dono(&d) && estado(&d)["whatsapp_so_dono"] == false);
        post(&d, "/api/troca", &json!({"whatsapp_so_dono": true})).unwrap();
        assert!(so_dono(&d) && !ligada(&d)); // so a restrita; a geral continua desligada
        post(&d, "/api/troca", &json!({"whatsapp": true})).unwrap();
        assert!(so_dono(&d) && ligada(&d)); // mexer numa nao apaga a outra
        post(&d, "/api/troca", &json!({"whatsapp": false})).unwrap();
        assert!(so_dono(&d) && !ligada(&d));
        assert_eq!(post(&d, "/api/troca", &json!({"whatsapp_so_dono": "sim"})).unwrap().status_code().0, 400);
        assert_eq!(post(&d, "/api/troca", &json!({})).unwrap().status_code().0, 400);
        std::fs::write(d.join("troca.json"), "lixo").unwrap();
        assert!(!so_dono(&d));
    }

    #[test]
    fn so_o_dono_e_atendido_de_verdade_e_terceiro_continua_em_sombra() {
        let d = tmp("so-dono");
        definir(&d, None, Some(true)).unwrap();
        let msgs = json!({"msgs": [{"d": "ELE", "c": "[09:58, 01/10/2026] Davi: ", "t": "me lembra de ligar para a loja amanha", "links": []}], "midias": []}).to_string();
        let mut p = Falsa { lista: lista_com_previas(), titulo: "Eu(D)".into(), caixa: true, msgs, ..Default::default() };
        let mut dono_respondido = false;
        for volta in 0..4 {
            let r = ciclo(&d, &agora(), TS + 30.0 * volta as f64, false, &json!({}), &mut p, &decide, &sem_voz(), &sem_resposta_do_dono).unwrap();
            dono_respondido |= r["dono_respondido"] == true;
        }
        assert!(dono_respondido, "{:?}", saida::ultimas(&d, 10));
        let digitado: Vec<&String> = p.gestos.iter().filter(|g| g.starts_with("inserir:")).collect();
        // tudo que foi digitado foi no chat do dono: a resposta dele e os relatorios do atendimento a ele (que citam a Ana, mas nao a escrevem)
        assert!(digitado.iter().all(|g| g.starts_with("inserir:Anotado") || g.starts_with("inserir:Jarvis ✓")), "{digitado:?}");
        assert!(!p.gestos.iter().any(|g| g.starts_with("inserir:Oi, Ana")), "{digitado:?}");
        assert!(p.gestos.iter().any(|g| g == "enter"));
        let s = saida::ultimas(&d, 50);
        let s = s.as_array().unwrap();
        // terceiros: decididos em sombra (registrados), nunca digitados; o dono nao vai para a caixa de sombra
        assert!(s.iter().any(|x| x["canal"] == "whatsapp" && x["destino"] == "Ana"), "{s:?}");
        assert!(s.iter().all(|x| !(x["canal"] == "whatsapp" && x["destino"] == "Eu(D)")), "{s:?}");
        assert!(s.iter().all(|x| !(x["canal"] == "whatsapp" && x["destino"] == "Pai")));
    }
}
