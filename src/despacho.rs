//! Despachante da caixa de saida: entrega DE VERDADE o que o B.R.A.N.I produz sozinho (rotinas, funil e resumos, Alma, recados, avisos
//! da agenda/Compasso/bom dia, outbox do Claude), atras das mesmas chaves de `dados/troca.json` (todas nascem DESLIGADAS).
//! Fontes: `saida.jsonl` (canal, destino, texto, ts), `avisos.jsonl` (para, texto, ts) e `outbox_usuarios.txt` (blocos `---`, regras de
//! vigia.enviar_outbox do Python: [audio] [arquivo] [resumo] [voz] [alma] [jorge]; texto puro sai como "Claudinho (via Braniac): ...").
//! Regras: `whatsapp` ligada entrega a todos; `whatsapp_so_dono` so ao dono; `telegram` so ao dono (canal escuta-telegram).
//! NUNCA entrega o que nasceu antes de a chave ser vista ligada (`desde_*` em dados/despacho.json): a sombra acumulada fica para sempre.
//! Nao sao do despachante: canais de sombra (whatsapp*, dono*, agente, airbnb*, link*, alma-voz) -- o laco vivo responde na hora.
//! Canais `pedido*` = envios que o dono pediu ao Jarvis (ferramentas e comandos diretos); `cuidado_diario` = bom dia da familia (so com `whatsapp`).
//! Pai do dono: nunca texto (vira audio do Zelo). Falha = tenta de novo com espera crescente, no maximo 4 vezes, sem duplicar (cursor
//! so avanca depois de entregar ou desistir). Um envio por vez (a mesma TRAVA do laco do WhatsApp). So contagens em logs.
use crate::alma_cuida::Nomes;
use crate::contexto::{norm, Ctx};
use crate::tempo::Agora;
use crate::{funil, guia_pai, wpp_laco};
use serde_json::{json, Value};
use std::path::Path;

const MAX_TENTATIVAS: i64 = 4;
/// Quanto tempo uma mensagem espera a aba do WhatsApp voltar antes de contar tentativa normal.
const ESPERA_INFRA_S: f64 = 12.0 * 3600.0;
const POR_CICLO: usize = 4;

pub struct Portas<'a> {
    pub texto: &'a dyn Fn(&str, &str) -> String,
    /// (agente da voz, nome, texto)
    pub audio: &'a dyn Fn(&str, &str, &str) -> String,
    pub wav: &'a dyn Fn(&str, &[u8]) -> String,
    /// (nome, caminho, legenda)
    pub arquivo: &'a dyn Fn(&str, &str, &str) -> String,
    pub telegram: &'a dyn Fn(&str) -> Result<(), String>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Via {
    Wpp,
    Tg,
}

fn ler(dados: &Path) -> Value {
    std::fs::read_to_string(dados.join("despacho.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}))
}

fn gravar(dados: &Path, v: &Value) {
    let _ = std::fs::write(dados.join("despacho.json"), v.to_string());
}

fn linhas(arq: &Path) -> Vec<String> {
    std::fs::read_to_string(arq).unwrap_or_default().lines().map(String::from).collect()
}

/// Canais que o laco vivo / a sombra tratam por conta propria (ou que nao sao mensagem): nunca passam por aqui.
fn de_sombra(canal: &str) -> bool {
    let c = canal.to_lowercase();
    ["whatsapp", "dono", "agente", "airbnb", "link", "alma-voz"].iter().any(|p| c.starts_with(p)) || c.ends_with("-barrado") || c == "telegram"
}

fn eh_dono(ctx: &Ctx, dest: &str) -> bool {
    let n = norm(dest);
    n == "dono" || (!ctx.dono.is_empty() && n == norm(&ctx.dono)) || ctx.chat_do_dono().is_some_and(|c| norm(c) == n)
}

/// O nome que o WhatsApp conhece: o dono vira o chat dele.
fn nome_real(ctx: &Ctx, dest: &str) -> Option<String> {
    if eh_dono(ctx, dest) { ctx.chat_do_dono().map(String::from) } else { Some(dest.to_string()) }
}

struct Chaves {
    todos: bool,
    dono: bool,
    tg: bool,
}

fn chaves(dados: &Path) -> Chaves {
    let todos = wpp_laco::ligada(dados);
    Chaves { todos, dono: todos || wpp_laco::so_dono(dados), tg: wpp_laco::chave(dados, "telegram") }
}

/// Marca desde quando cada chave esta ligada (null = desligada). Devolve o estado.
fn atualizar_desde(est: &mut Value, k: &Chaves, ts: f64) {
    for (nome, on) in [("desde_todos", k.todos), ("desde_dono", k.dono), ("desde_tg", k.tg)] {
        est[nome] = if !on { Value::Null } else if est[nome].is_null() { json!(ts) } else { est[nome].clone() };
    }
}

enum Dec {
    Entregar(Via),
    Pular,
}

fn decidir(canal: &str, dono: bool, ts_item: Option<f64>, est: &Value, k: &Chaves) -> Dec {
    let Some(t) = ts_item else { return Dec::Pular };
    if canal == "escuta-telegram" {
        return match est["desde_tg"].as_f64() {
            Some(d) if k.tg && dono && t >= d => Dec::Entregar(Via::Tg),
            _ => Dec::Pular,
        };
    }
    if de_sombra(canal) {
        return Dec::Pular;
    }
    let (on, desde) = if dono { (k.dono, est["desde_dono"].as_f64()) } else { (k.todos, est["desde_todos"].as_f64()) };
    match desde {
        Some(d) if on && t >= d => Dec::Entregar(Via::Wpp),
        _ => Dec::Pular,
    }
}

fn agente_da_voz(canal: &str) -> &'static str {
    if canal.starts_with("alma") { "alma" } else if canal == "outbox-jorge" { "jorge" } else { "jarvis" }
}

/// Entrega UM item. Ok = entregue (ou nada a fazer); Err = falhou, tentar de novo.
fn entregar(via: Via, canal: &str, dest: &str, texto: &str, ctx: &Ctx, nomes: &Nomes, p: &Portas) -> Result<(), String> {
    if via == Via::Tg {
        return (p.telegram)(texto);
    }
    let nome = nome_real(ctx, dest).ok_or("sem o chat do dono configurado")?;
    let ok = |r: String| if r.starts_with('✅') { Ok(()) } else { Err(r) };
    if canal.starts_with("pedido") {
        // envio que o dono pediu ao Jarvis: as travas de quem pode pedir ja foram aplicadas na ferramenta; aqui so o canal certo.
        // Recusa definitiva comeca com "⛔" (nao tenta de novo e nao trava a fila). O destino e conferido no topo da conversa antes de enviar.
        let pai = guia_pai::eh_pai(&nome, nomes);
        return match canal {
            "pedido-arquivo" if pai => Err("⛔ o pai só recebe áudio".into()),
            "pedido-arquivo" => {
                let (arq, legenda) = texto.split_once('\n').unwrap_or((texto, ""));
                ok((p.arquivo)(&nome, arq.trim(), legenda.trim()))
            }
            "pedido-aviso" => ok((p.audio)("jarvis", &nome, texto)).or_else(|_| ok((p.texto)(&nome, texto))), // aviso do agente ao dono: audio, texto se falhar
            "pedido-audio" => ok((p.audio)(if pai { "zelo" } else { "jarvis" }, &nome, texto)),
            _ if pai => Err("⛔ o pai só recebe áudio".into()),
            _ => ok((p.texto)(&nome, texto)),
        };
    }
    if canal.ends_with("-audio") || guia_pai::eh_pai(&nome, nomes) {
        let agente = if guia_pai::eh_pai(&nome, nomes) { "zelo" } else { agente_da_voz(canal) };
        return ok((p.audio)(agente, &nome, texto)); // pai: so audio, nunca texto de reserva
    }
    ok((p.texto)(&nome, texto))
}

fn corta(t: &str, n: usize) -> String {
    t.chars().take(n).collect()
}

/// Falha de INFRAESTRUTURA (aba do WhatsApp nao achada, VM/CDP fora do ar): nao e culpa da mensagem. Nao conta tentativa nem
/// descarta: a mensagem espera a aba voltar (a reconciliacao repoe). Assim um bom dia nao se perde porque a VM estava confusa.
fn infra(e: &str) -> bool {
    let e = e.to_lowercase();
    ["nao tem aba", "não tem aba", "vm esta ligada", "vm está ligada", "nao foi aberto na vm", "nao conectei na aba", "aba caiu", "aba nao respondeu", "ler a lista do whatsapp", "cdp fora do ar"]
        .iter().any(|x| e.contains(x))
}

/// Uma fonte de linhas JSONL com cursor. Devolve (entregues, pulados, falhas).
#[allow(clippy::too_many_arguments)]
fn fonte(dados: &Path, arq: &str, cur: &str, est: &mut Value, ts: f64, k: &Chaves, ctx: &Ctx, nomes: &Nomes, p: &Portas, limite: &mut usize, cont: &mut [u32; 3]) {
    let ls = linhas(&dados.join(arq));
    let mut i = est[cur].as_u64().unwrap_or(ls.len() as u64) as usize; // sem estado: o que ja existe nunca sai
    while i < ls.len() {
        let v: Value = serde_json::from_str(&ls[i]).unwrap_or(Value::Null);
        let (canal, dest, texto) = if arq == "avisos.jsonl" {
            ("aviso", v["para"].as_str().unwrap_or(""), v["texto"].as_str().unwrap_or(""))
        } else {
            (v["canal"].as_str().unwrap_or(""), v["destino"].as_str().unwrap_or(""), v["texto"].as_str().unwrap_or(""))
        };
        if dest.is_empty() || texto.trim().is_empty() {
            cont[1] += 1;
            i += 1;
            continue;
        }
        let dec = decidir(canal, eh_dono(ctx, dest), v["ts"].as_f64(), est, k);
        let Dec::Entregar(via) = dec else {
            cont[1] += 1;
            i += 1;
            continue;
        };
        let chave = format!("{arq}:{i}");
        let (tent, prox) = (est["tent"][&chave]["n"].as_i64().unwrap_or(0), est["tent"][&chave]["prox"].as_f64().unwrap_or(0.0));
        if ts < prox || *limite == 0 {
            break; // espera a vez dele: nada passa na frente (ordem preservada)
        }
        *limite -= 1;
        match entregar(via, canal, dest, texto, ctx, nomes, p) {
            Ok(()) => {
                cont[0] += 1;
                if let Some(t) = est["tent"].as_object_mut() {
                    t.remove(&chave);
                }
                i += 1;
            }
            Err(e) if infra(&e) && !e.starts_with('⛔') && ts - v["ts"].as_f64().unwrap_or(ts) < ESPERA_INFRA_S => {
                est["tent"][&chave] = json!({ "n": tent, "prox": ts + 120.0 }); // sem gastar tentativa: volta daqui a 2 min
                break;
            }
            Err(e) if tent + 1 >= MAX_TENTATIVAS || e.starts_with('⛔') => {
                cont[2] += 1;
                if let Some(t) = est["tent"].as_object_mut() {
                    t.remove(&chave);
                }
                i += 1; // desistiu: nao fica preso para sempre
            }
            Err(_) => {
                est["tent"][&chave] = json!({ "n": tent + 1, "prox": ts + 60.0 * (tent + 1) as f64 });
                break;
            }
        }
    }
    est[cur] = json!(i);
}

/// O outbox do Claude. Ok(n entregues). Mesmas regras de vigia.enviar_outbox (Python).
fn outbox(dados: &Path, ts: f64, agora: &Agora, est: &mut Value, k: &Chaves, ctx: &Ctx, nomes: &Nomes, p: &Portas) -> usize {
    let arq = dados.join("outbox_usuarios.txt");
    let Ok(bruto) = std::fs::read_to_string(&arq) else { return 0 };
    let bruto = bruto.trim_start_matches('\u{feff}').trim().to_string();
    if bruto.is_empty() {
        let _ = std::fs::remove_file(&arq);
        return 0;
    }
    let Some(desde) = est["desde_dono"].as_f64().filter(|_| k.dono) else {
        // chave desligada: o que esta ai e anterior a ela e nunca sera enviado (guardado, nao apagado)
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dados.join("outbox_antigo.txt")) {
            let _ = writeln!(f, "{bruto}\n---");
        }
        let _ = std::fs::remove_file(&arq);
        return 0;
    };
    let mtime = std::fs::metadata(&arq).and_then(|m| m.modified()).ok().and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok()).map_or(ts, |d| d.as_secs_f64());
    if mtime < desde {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dados.join("outbox_antigo.txt")) {
            let _ = writeln!(f, "{bruto}\n---");
        }
        let _ = std::fs::remove_file(&arq);
        return 0;
    }
    let blocos = dividir(&bruto);
    let dono = if ctx.dono.is_empty() { "dono".to_string() } else { ctx.dono.clone() };
    let mut feitos = 0usize;
    for b in &blocos {
        let baixo = b.to_lowercase();
        let r: Result<(), String> = if let Some(t) = baixo.starts_with("[resumo]").then(|| b[8..].trim()) {
            // aviso automatico: passa pelo funil (agora ou resumo das 9h/13h/19h)
            let enviar = |_: &str, t: &str| match entregar(Via::Wpp, "outbox", &dono, t, ctx, nomes, p) { Ok(()) => "✅".to_string(), Err(e) => e };
            let r = funil::filtrar(dados, &dono, &dono, t, "outbox", agora, ts, &enviar);
            if r.starts_with('✅') { Ok(()) } else { Err(r) }
        } else if baixo.starts_with("[arquivo]") {
            let ls: Vec<&str> = b[9..].trim().lines().collect();
            let nome = nome_real(ctx, &dono).unwrap_or_default();
            let r = (p.arquivo)(&nome, ls.first().map_or("", |s| s.trim()), &corta(ls.iter().skip(1).copied().collect::<Vec<_>>().join("\n").trim(), 900));
            if r.starts_with('✅') { Ok(()) } else { Err(r) }
        } else if baixo.starts_with("[alma]") {
            let t = b[6..].trim();
            if t.is_empty() || t.starts_with("@novidades") {
                Ok(()) // a Alma compoe o proprio texto com o modelo local: ainda nao ligado aqui; nada enviado
            } else {
                entregar(Via::Wpp, "outbox", &dono, &corta(t, 1500), ctx, nomes, p).and_then(|()| entregar(Via::Wpp, "alma-audio", &dono, t, ctx, nomes, p).or(Ok(())))
            }
        } else if baixo.starts_with("[jorge]") {
            // so para a Mae; audio da voz dele e, se falhar, o texto. Nunca vai a hospede.
            match (k.todos, ctx.contato("mae")) {
                (true, Some(mae)) => {
                    let t = corta(b[7..].trim(), 900);
                    let r = (p.audio)("jorge", mae, &t);
                    if r.starts_with('✅') { Ok(()) } else { entregar(Via::Wpp, "outbox", mae, &t, ctx, nomes, p) }
                }
                _ => Err("[jorge] precisa da chave whatsapp ligada e do contato da mae".into()),
            }
        } else if baixo.starts_with("[voz]") {
            let c = b[5..].trim().lines().next().unwrap_or("").trim().trim_matches('"').to_string();
            match std::fs::read(&c).ok().filter(|_| c.to_lowercase().ends_with(".wav")) {
                Some(w) => { let r = (p.wav)(&nome_real(ctx, &dono).unwrap_or_default(), &w); if r.starts_with('✅') { Ok(()) } else { Err(r) } }
                None => Ok(()), // wav nao encontrado: ignorado (como no Python)
            }
        } else if baixo.starts_with("[audio]") {
            let t = corta(b[7..].trim(), 1200);
            entregar(Via::Wpp, "outbox-audio", &dono, &t, ctx, nomes, p).or_else(|_| entregar(Via::Wpp, "outbox", &dono, &t, ctx, nomes, p))
        } else {
            entregar(Via::Wpp, "outbox", &dono, &corta(&format!("Claudinho (via Braniac): {b}"), 1500), ctx, nomes, p)
        };
        let id = format!("{:x}", b.bytes().fold(14695981039346656037u64, |h, c| (h ^ c as u64).wrapping_mul(1099511628211)));
        match r {
            Ok(()) => { feitos += 1; if let Some(t) = est["tent_outbox"].as_object_mut() { t.remove(&id); } }
            Err(e) if infra(&e) && !e.starts_with('⛔') => break, // aba do WhatsApp fora: o bloco fica no arquivo para o proximo ciclo
            Err(_) => {
                let n = est["tent_outbox"][&id].as_i64().unwrap_or(0) + 1;
                if n >= MAX_TENTATIVAS { feitos += 1; } else { est["tent_outbox"][&id] = json!(n); break; }
            }
        }
    }
    // o que outro processo gravou enquanto entregavamos nao pode sumir
    let agora_txt = std::fs::read_to_string(&arq).unwrap_or_default().trim_start_matches('\u{feff}').trim().to_string();
    let novo = if agora_txt.starts_with(&bruto) { agora_txt[bruto.len()..].to_string() } else if agora_txt == bruto { String::new() } else { agora_txt.clone() };
    let mut resto: Vec<String> = blocos[feitos.min(blocos.len())..].to_vec();
    resto.extend(dividir(&novo).into_iter().filter(|x| !blocos.contains(x)));
    if resto.is_empty() { let _ = std::fs::remove_file(&arq); } else { let _ = std::fs::write(&arq, resto.join("\n---\n")); }
    feitos
}

fn dividir(t: &str) -> Vec<String> {
    let mut blocos = vec![];
    let mut atual: Vec<&str> = vec![];
    for l in t.lines() {
        if l.trim() == "---" {
            blocos.push(atual.join("\n").trim().to_string());
            atual.clear();
        } else {
            atual.push(l);
        }
    }
    blocos.push(atual.join("\n").trim().to_string());
    blocos.into_iter().filter(|b| !b.is_empty()).collect()
}

/// UM ciclo. Devolve so contagens.
pub fn ciclo(dados: &Path, agora: &Agora, ts: f64, p: &Portas) -> Value {
    let (k, ctx, nomes) = (chaves(dados), Ctx::ler(dados), Nomes::ler(dados));
    let mut est = ler(dados);
    atualizar_desde(&mut est, &k, ts);
    if !est["tent"].is_object() { est["tent"] = json!({}); }
    if !est["tent_outbox"].is_object() { est["tent_outbox"] = json!({}); }
    let (mut limite, mut cont) = (POR_CICLO, [0u32; 3]);
    for (arq, cur) in [("saida.jsonl", "cur_saida"), ("avisos.jsonl", "cur_avisos")] {
        fonte(dados, arq, cur, &mut est, ts, &k, &ctx, &nomes, p, &mut limite, &mut cont);
    }
    if k.todos && est["desde_todos"].is_number() {
        // familia: so com a chave geral (terceiro nunca recebe com a restrita)
        crate::cuidado_diario::passo(dados, agora, ts, &ctx, &crate::cuidado_diario::Canais { texto: p.texto, audio: p.audio });
    }
    let do_outbox = outbox(dados, ts, agora, &mut est, &k, &ctx, &nomes, p);
    gravar(dados, &est);
    json!({ "entregues": cont[0] as usize + do_outbox, "pulados": cont[1], "desistidos": cont[2], "outbox": do_outbox,
            "chaves": { "whatsapp": k.todos, "so_dono": wpp_laco::so_dono(dados), "telegram": k.tg } })
}

/// Ha algo que valha pegar a TRAVA do WhatsApp? (barato: so compara tamanhos)
fn ha_trabalho(dados: &Path) -> bool {
    let k = chaves(dados);
    let est = ler(dados);
    let n = |a: &str| linhas(&dados.join(a)).len() as u64;
    let novo = est["cur_saida"].as_u64().is_none_or(|c| n("saida.jsonl") > c) || est["cur_avisos"].as_u64().is_none_or(|c| n("avisos.jsonl") > c);
    // chave geral recem-ligada: marca o "desde" ja, senao o 1o envio nasce antes da marca e vira sombra
    (k.todos && est["desde_todos"].is_null()) || (k.dono || k.tg) && novo || (k.todos && crate::cuidado_diario::ha_pendente(dados, &crate::tempo::agora(), crate::fatos::agora_ts())) || dados.join("outbox_usuarios.txt").exists() || est["desde_dono"].is_null() != !k.dono
}

pub fn estado(dados: &Path) -> Value {
    let est = ler(dados);
    json!({ "cursor_saida": est["cur_saida"], "cursor_avisos": est["cur_avisos"], "em_tentativa": est["tent"].as_object().map_or(0, |t| t.len()),
            "desde_dono": est["desde_dono"], "desde_todos": est["desde_todos"], "desde_tg": est["desde_tg"] })
}

/// A thread de producao: de 10 em 10 s. So pega a trava do WhatsApp quando ha o que entregar.
pub fn iniciar(rot: std::sync::Arc<crate::roteador::Roteador>) {
    use crate::wpp_vivo as wv;
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(10));
        let dados = crate::pasta_dados();
        if !ha_trabalho(&dados) {
            continue;
        }
        let (agora, ts) = (crate::tempo::agora(), crate::fatos::agora_ts());
        let guarda = wpp_laco::TRAVA.try_lock().ok(); // o laco do atendimento esta usando a aba: fica para o proximo ciclo
        if guarda.is_none() {
            continue;
        }
        let (ctx, nomes) = (Ctx::ler(&dados), Nomes::ler(&dados));
        let pag = std::cell::RefCell::new(None::<wv::Real>);
        let com_pag = |f: &mut dyn FnMut(&mut wv::Real) -> String| -> String {
            let mut p = pag.borrow_mut();
            if p.is_none() {
                match wv::Real::conectar(&dados) { Ok(r) => *p = Some(r), Err(e) => return format!("sem a aba do WhatsApp: {e}") }
            }
            f(p.as_mut().expect("aba"))
        };
        let falar = |agente: &str, t: &str| -> Result<Vec<u8>, String> {
            let nuvem = !rot.sensivel(t);
            let estilo = crate::vozes_agentes::escolha(&dados, agente)["ajustes"]["estilo"].as_str().unwrap_or("").to_string();
            crate::vozes_agentes::falar(&dados, &agora, agente, t, nuvem, &|m, v, tx| crate::voz::gerar(&*rot.ler_chave, m, v, tx, &estilo)).map(|f| f.wav)
                .or_else(|_| crate::voz::falar_wav(&dados, &*rot.ler_chave, t, nuvem, agente).map(|(w, _)| w))
        };
        // falha de envio nunca fica muda: etapa e motivo (sem o texto da mensagem) vao para a saida, canal "*-barrado"
        let anota = |canal: &str, n: &str, r: String| { if !r.starts_with('✅') { crate::saida::registrar(&dados, canal, n, &corta(&r, 240)); } r };
        let texto = |n: &str, t: &str| anota("despacho-texto-barrado", n, com_pag(&mut |p| wv::enviar_texto(p, &dados, &ctx, &nomes, n, t)));
        let wav = |n: &str, w: &[u8]| anota("despacho-audio-barrado", n, com_pag(&mut |p| crate::wpp_midia::enviar_audio(p, &dados, &ctx, &nomes, n, w)));
        let audio = |ag: &str, n: &str, t: &str| match falar(ag, t) { Ok(w) => wav(n, &w), Err(e) => anota("despacho-voz-barrado", n, format!("áudio não gerado ({e})")) };
        let arquivo = |n: &str, c: &str, l: &str| anota("despacho-arquivo-barrado", n, com_pag(&mut |p| crate::wpp_midia::enviar_arquivo(p, &ctx, &nomes, n, c, l, false)));
        let tg = |t: &str| -> Result<(), String> {
            let (token, id) = ((rot.ler_chave)("TELEGRAM_BOT_TOKEN"), (rot.ler_chave)("TELEGRAM_USER_ID"));
            let dono = crate::telegram_bot::dono_configurado(&token, &id).ok_or("falta o token ou o id do Telegram")?;
            crate::telegram_laco::Api::mandar(&crate::telegram_laco::Real::nova(&token), dono, t)
        };
        let _ = ciclo(&dados, &agora, ts, &Portas { texto: &texto, audio: &audio, wav: &wav, arquivo: &arquivo, telegram: &tg });
        drop(guarda);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::saida;
    use crate::tempo::Data;
    use std::cell::RefCell;

    fn agora() -> Agora {
        Agora { data: Data::civil(2026, 11, 21).unwrap(), hora: 10, min: 0, seg: 0 }
    }

    fn pasta(n: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-desp-{n}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("pessoal.json"), json!({ "nome": "Dono", "contatos": { "dono": "Eu(D)", "mae": "Mae", "pai": "Pai" },
            "alma": { "contato_pai": ["Pai"] } }).to_string()).unwrap();
        d
    }

    fn chaves_json(d: &Path, v: Value) {
        std::fs::write(d.join("troca.json"), v.to_string()).unwrap();
    }

    #[derive(Default)]
    struct Reg {
        v: RefCell<Vec<String>>,
        falhar: RefCell<bool>,
    }

    fn rodar(d: &Path, r: &Reg, ts: f64) -> Value {
        let res = |s: String| if *r.falhar.borrow() { "falhou".to_string() } else { s };
        let texto = |n: &str, t: &str| { r.v.borrow_mut().push(format!("texto|{n}|{t}")); res("✅".into()) };
        let audio = |a: &str, n: &str, t: &str| { r.v.borrow_mut().push(format!("audio|{a}|{n}|{t}")); res("✅".into()) };
        let wav = |n: &str, _: &[u8]| { r.v.borrow_mut().push(format!("wav|{n}")); "✅".to_string() };
        let arq = |n: &str, c: &str, l: &str| { r.v.borrow_mut().push(format!("arquivo|{n}|{c}|{l}")); "✅".to_string() };
        let tg = |t: &str| { r.v.borrow_mut().push(format!("tg|{t}")); Ok(()) };
        ciclo(d, &agora(), ts, &Portas { texto: &texto, audio: &audio, wav: &wav, arquivo: &arq, telegram: &tg })
    }

    fn escrever(d: &Path, canal: &str, dest: &str, t: &str, ts: f64) {
        saida::registrar(d, canal, dest, t);
        // o registrar usa a hora real: reescreve a ultima linha com o ts do teste
        let mut ls = linhas(&d.join("saida.jsonl"));
        let u = ls.pop().unwrap();
        let mut v: Value = serde_json::from_str(&u).unwrap();
        v["ts"] = json!(ts);
        ls.push(v.to_string());
        std::fs::write(d.join("saida.jsonl"), ls.join("\n") + "\n").unwrap();
    }

    #[test]
    fn item_anterior_a_chave_nunca_sai() {
        let d = pasta("antigo");
        let r = Reg::default();
        escrever(&d, "alma", "Dono", "velho", 100.0);
        rodar(&d, &r, 200.0); // grava o cursor (chave desligada)
        escrever(&d, "alma", "Dono", "da sombra", 250.0);
        rodar(&d, &r, 300.0);
        chaves_json(&d, json!({ "whatsapp_so_dono": true }));
        rodar(&d, &r, 400.0); // ligou agora: "da sombra" nasceu antes
        assert!(r.v.borrow().is_empty());
        escrever(&d, "alma", "Dono", "novo", 450.0);
        rodar(&d, &r, 460.0);
        assert_eq!(*r.v.borrow(), vec!["texto|Eu(D)|novo"]);
        rodar(&d, &r, 470.0);
        assert_eq!(r.v.borrow().len(), 1, "nao duplica");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn so_dono_nao_entrega_a_terceiro_e_sombra_nao_e_do_despachante() {
        let d = pasta("terceiro");
        let r = Reg::default();
        chaves_json(&d, json!({ "whatsapp_so_dono": true }));
        rodar(&d, &r, 100.0);
        escrever(&d, "alma", "Ana", "oi Ana", 110.0);
        escrever(&d, "whatsapp", "Eu(D)", "o que eu responderia", 111.0);
        escrever(&d, "dono", "Eu(D)", "sombra do dono", 112.0);
        escrever(&d, "funil", "Eu(D)", "resumo", 113.0);
        rodar(&d, &r, 120.0);
        assert_eq!(*r.v.borrow(), vec!["texto|Eu(D)|resumo"]);
        chaves_json(&d, json!({ "whatsapp": true, "whatsapp_so_dono": true }));
        rodar(&d, &r, 130.0); // "oi Ana" nasceu antes de a chave geral ser vista ligada
        assert_eq!(r.v.borrow().len(), 1);
        escrever(&d, "recado", "Ana", "agora sim", 140.0);
        rodar(&d, &r, 150.0);
        assert_eq!(r.v.borrow()[1], "texto|Ana|agora sim");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn pai_so_recebe_audio_e_falha_nao_duplica() {
        let d = pasta("pai");
        let r = Reg::default();
        chaves_json(&d, json!({ "whatsapp": true }));
        rodar(&d, &r, 100.0);
        escrever(&d, "recado", "Pai", "filho, tudo bem?", 110.0);
        rodar(&d, &r, 120.0);
        assert_eq!(*r.v.borrow(), vec!["audio|zelo|Pai|filho, tudo bem?"]);
        // falha: tenta de novo depois da espera, sem entregar duas vezes
        escrever(&d, "funil-audio", "Eu(D)", "resumo falado", 130.0);
        *r.falhar.borrow_mut() = true;
        rodar(&d, &r, 140.0);
        rodar(&d, &r, 141.0); // ainda na espera: nem tenta
        assert_eq!(r.v.borrow().len(), 2);
        *r.falhar.borrow_mut() = false;
        rodar(&d, &r, 210.0);
        assert_eq!(r.v.borrow().iter().filter(|x| x.contains("resumo falado")).count(), 2); // 1 falha + 1 sucesso
        rodar(&d, &r, 300.0);
        assert_eq!(r.v.borrow().len(), 3);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn outbox_segue_as_regras_do_python() {
        let d = pasta("outbox");
        let r = Reg::default();
        std::fs::write(d.join("outbox_usuarios.txt"), "recado velho").unwrap(); // chave desligada: nunca sai
        rodar(&d, &r, 100.0);
        assert!(r.v.borrow().is_empty() && !d.join("outbox_usuarios.txt").exists() && d.join("outbox_antigo.txt").exists());
        chaves_json(&d, json!({ "whatsapp_so_dono": true }));
        rodar(&d, &r, 200.0);
        let arq = d.join("a.bin");
        std::fs::write(&arq, "x").unwrap();
        std::fs::write(d.join("outbox_usuarios.txt"), format!("achei 3 vagas\n---\n[audio] bom dia\n---\n[arquivo] {}\nlegenda\n---\n[jorge] oi tia\n---\n[resumo] aviso comum", arq.display())).unwrap();
        rodar(&d, &r, 1.0e10);
        let v = r.v.borrow().clone();
        assert_eq!(v[0], "texto|Eu(D)|Claudinho (via Braniac): achei 3 vagas");
        assert_eq!(v[1], "audio|jarvis|Eu(D)|bom dia");
        assert!(v[2].starts_with("arquivo|Eu(D)|") && v[2].ends_with("|legenda"));
        // [jorge] vai a Mae: precisa da chave geral; falha e fica para depois
        assert!(!v.iter().any(|x| x.contains("oi tia")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn envio_pedido_ao_jarvis_sai_so_com_a_chave_e_respeita_o_dono_e_o_pai() {
        let d = pasta("pedido");
        let r = Reg::default();
        rodar(&d, &r, 100.0); // chaves desligadas: grava o cursor
        escrever(&d, "pedido", "Eu(D)", "antes da chave", 110.0);
        rodar(&d, &r, 120.0);
        assert!(r.v.borrow().is_empty(), "chave desligada: nada sai");
        chaves_json(&d, json!({ "whatsapp_so_dono": true }));
        rodar(&d, &r, 200.0);
        escrever(&d, "pedido", "Eu(D)", "oi dono", 210.0);
        escrever(&d, "pedido", "Ana", "oi Ana", 211.0);
        escrever(&d, "pedido-audio", "Eu(D)", "fala isso", 212.0);
        escrever(&d, "pedido-arquivo", "Eu(D)", "C:/x/jogo.html\nPronto", 213.0);
        rodar(&d, &r, 220.0);
        assert_eq!(*r.v.borrow(), vec!["texto|Eu(D)|oi dono", "audio|jarvis|Eu(D)|fala isso", "arquivo|Eu(D)|C:/x/jogo.html|Pronto"]);
        // chave geral: terceiro recebe; pai nunca recebe texto nem arquivo (recusa final, a fila segue), audio sai pela voz do Zelo
        chaves_json(&d, json!({ "whatsapp": true }));
        rodar(&d, &r, 300.0);
        r.v.borrow_mut().clear();
        escrever(&d, "pedido", "Pai", "texto ao pai", 310.0);
        escrever(&d, "pedido-arquivo", "Pai", "C:/x/a.pdf", 311.0);
        escrever(&d, "pedido-audio", "Pai", "audio ao pai", 312.0);
        escrever(&d, "pedido", "Ana", "agora vai", 313.0);
        rodar(&d, &r, 320.0);
        assert_eq!(*r.v.borrow(), vec!["audio|zelo|Pai|audio ao pai", "texto|Ana|agora vai"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn aba_do_whatsapp_fora_nao_descarta_o_bom_dia() {
        // regressao 24/11/2026: com a aba do WhatsApp nao achada a mensagem gastava as 4 tentativas e era descartada para sempre
        let d = pasta("infra");
        let r = Reg::default();
        chaves_json(&d, json!({ "whatsapp_so_dono": true }));
        rodar(&d, &r, 100.0); // grava o cursor
        escrever(&d, "alma", "Dono", "bom dia", 450.0);
        let enviados = RefCell::new(0u32);
        let tenta = |ts: f64, fora: bool| {
            let texto = |_: &str, _: &str| if fora { "Não consegui ler a lista do WhatsApp (a VM esta ligada? HTTP GET abas). Nada foi enviado.".to_string() } else { *enviados.borrow_mut() += 1; "✅".to_string() };
            let audio = |_: &str, _: &str, _: &str| "falhou".to_string();
            let wav = |_: &str, _: &[u8]| "✅".to_string();
            let arq = |_: &str, _: &str, _: &str| "✅".to_string();
            let tg = |_: &str| Ok(());
            ciclo(&d, &agora(), ts, &Portas { texto: &texto, audio: &audio, wav: &wav, arquivo: &arq, telegram: &tg })
        };
        for k in 0..10 {
            let v = tenta(460.0 + 200.0 * k as f64, true);
            assert_eq!(v["desistidos"], 0, "falha de infraestrutura nunca desiste");
        }
        assert_eq!(*enviados.borrow(), 0);
        tenta(3000.0, false); // a aba voltou
        assert_eq!(*enviados.borrow(), 1, "entrega uma vez, sem perder nem duplicar");
        tenta(3100.0, false);
        assert_eq!(*enviados.borrow(), 1);
        assert!(infra("o agente 'whatsapp' nao tem aba aberta") && !infra("O WhatsApp diz que o número é inválido"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
