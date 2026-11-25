//! O LACO do atendimento do WhatsApp (atender() e coletar() de braniac/atender.py), na mesma ordem e com as mesmas travas:
//! quem nunca e respondido, grupo, pedido sobre o que entregamos, agradecimento/elogio, regra aprendida, decisao, contato novo
//! sempre se apresenta, familia e prioritario nunca ficam sem resposta, revisao antes de agir, validar_resposta, resposta longa
//! refeita uma vez, recado enfileirado, registro na memoria e espaco entre envios. As regras puras vem de atender.rs.
//! Tudo que sai para fora (ler conversa, decidir, revisar, enviar texto/audio, ouvir audio, ver imagem, baixar documento, esperar)
//! entra por closure em `Portas`. Este arquivo nao envia nada sozinho.
//! Ficou de fora (entra pela closure `atalho`, em 3 pontos do laco): Zelo/guia do pai e Alma (fase Antes); conselho de IAs, busca
//! de preco/lugar e corrida (fase Meio); checklist/prazos, financeiro e vaquinha (fase Fim). Tambem de fora: o recado por
//! Telegram de um contato especifico. Nomes de pessoa do Python viraram configuracao: familia e namorada = `ctx.de_casa`, o pai =
//! contato "pai", a mae = contato "mae", a atendente da loja = `ctx.prioritario` (as respostas de reserva nao citam nome).
//! Estado que no Python fica na memoria do processo (pausa de 20 min apos falha ao ouvir audio; um agente por pessoa a cada
//! 15 min no pedido de entrega) aqui fica em dados/atender_pausas.json.
#![allow(dead_code)]
use crate::atender::{self, Item, Linha, Msg, ANOTADO_ENTREGA, PRIMEIRO_CONTATO, VOZ_ATENDER_MAX};
use crate::contexto::{self, re, Ctx};
use crate::tempo::Agora;
use crate::{clareza, gibberlink, memoria, recados, revisao, wpp_doc};
use serde_json::{json, Value};
use std::path::Path;

/// Ouvir audio recarrega a pagina do WhatsApp; se falhou, nao repete por 20 min para a mesma pessoa.
pub const AUDIO_PAUSA_S: f64 = 1200.0;
/// Um agente por pessoa a cada 15 min no pedido sobre a entrega (a rajada de 3 mensagens nao dispara 3).
pub const ENTREGA_PAUSA_S: f64 = 900.0;
pub const RESERVA_PRIORITARIO: &str = "Recebi sua mensagem. Já estou verificando e te respondo.";
pub const RETORNO_PRIORITARIO: &str = "Entendido. Já estou vendo isso e te retorno.";
pub const CORRECAO: &str = "Desculpa, entendi errado. Me conta de novo que agora eu presto atenção.";

// o prioritario perguntou/pediu algo: nunca fica sem resposta
re!(pergunta_prio, r"(?i)\?|\b(?:preciso|precisa|ajuda|ajudar|erro|problema|como|quando|qual|quais|onde|por\s+que|pode|consegue|d[aá]\s+pra)\b");

/// Conversa esperando decisao + se o ultimo item dela e um audio da pessoa.
#[derive(Default, Clone, Debug)]
pub struct Pendente {
    pub it: Item,
    pub audio_dele: bool,
}

/// O que a tela mostra de uma conversa aberta.
#[derive(Default, Clone, Debug)]
pub struct Conversa {
    pub msgs: Vec<Msg>,
    /// Marcas de midia sem texto: "[AUDIO]", "[IMAGEM]", "[DOC:nome]".
    pub midias: Vec<String>,
    pub cabecalho: String,
    pub audio_dele: bool,
}

/// De onde saem os candidatos a atendimento.
#[derive(Default)]
pub struct Fontes<'a> {
    /// Lista de conversas na ordem da tela: (nome, nao lidas).
    pub lista: &'a [(String, i64)],
    /// Conversas ja vistas e ainda sem resposta (olhar marca como lida, mas continua pendente).
    pub pendentes: &'a [String],
    /// Linhas da lista com o resumo (para achar as LIDAS e sem resposta).
    pub linhas: &'a [Linha],
    /// Vigia automatico: so as conversas que ele escolheu.
    pub somente: Option<&'a [String]>,
}

fn corta(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn ok(r: &str) -> bool {
    r.starts_with('✅')
}

/// Nomes a abrir, na ordem: nao lidas, pendentes, lidas e sem resposta (ultima mensagem e dela, ate 12 h). O chat do dono fica de fora.
pub fn candidatos(ctx: &Ctx, f: &Fontes, agora: &Agora) -> Vec<String> {
    let na_lista = |n: &str| f.lista.iter().any(|(x, _)| x == n);
    let mut c: Vec<String> = vec![];
    match f.somente {
        Some(s) => c.extend(s.iter().filter(|n| na_lista(n)).cloned()),
        None => {
            c.extend(f.lista.iter().filter(|(_, nl)| *nl > 0).map(|(n, _)| n.clone()));
            for n in f.pendentes.iter().cloned().chain(atender::esperando_resposta(ctx, f.linhas, agora, 12.0)) {
                if na_lista(&n) && !c.contains(&n) {
                    c.push(n);
                }
            }
        }
    }
    let dono = ctx.chat_do_dono().unwrap_or("");
    c.retain(|n| n != dono);
    c
}

#[derive(Debug)]
pub enum Montagem {
    Pronta(Pendente),
    /// So texto de dias atras: nao reabre a conversa (e continua pendente).
    Velha,
    /// Ja respondida por alguem: sai da lista de pendentes.
    Respondida,
}

/// Transforma a conversa aberta no item a decidir.
pub fn montar(nome: &str, conv: &Conversa, agora: &Agora, max_msgs: usize, max_idade_h: Option<f64>) -> Montagem {
    let mut novas = atender::finais_deles(&conv.msgs);
    let tem_midia = !conv.midias.is_empty();
    let idade = |m: &Msg| atender::idade_horas(&m.c, agora).unwrap_or(0.0);
    if let Some(max) = max_idade_h {
        if !novas.is_empty() && !tem_midia && !conv.audio_dele && novas.iter().all(|m| idade(m) > max) {
            return Montagem::Velha;
        }
        if tem_midia || conv.audio_dele { // texto de dias atras nao entra junto do audio novo
            novas.retain(|m| idade(m) <= max);
        }
    }
    if novas.is_empty() && !tem_midia && !conv.audio_dele {
        return Montagem::Respondida;
    }
    let mut msgs: Vec<String> = novas[novas.len().saturating_sub(max_msgs)..].iter().filter(|m| !m.t.is_empty()).map(|m| m.t.clone()).collect();
    msgs.extend(conv.midias.iter().cloned());
    let mut autores: Vec<String> = novas.iter().map(|m| atender::autor(&m.c)).filter(|a| !a.is_empty()).collect();
    autores.sort();
    autores.dedup();
    let com_texto: Vec<&Msg> = conv.msgs.iter().filter(|m| !m.t.is_empty()).collect();
    let ctx = com_texto[com_texto.len().saturating_sub(8)..].iter().map(|m| format!("{}{}", if m.d == "EU" { "C: " } else { "E: " }, corta(&m.t, 100))).collect();
    Montagem::Pronta(Pendente {
        it: Item { nome: nome.to_string(), msgs, ctx, autores, midia: tem_midia || !novas.iter().any(|m| !m.t.is_empty()), grupo: atender::eh_grupo(&conv.cabecalho), gl_ativo: false },
        audio_dele: conv.audio_dele,
    })
}

/// Conversas com nao lidas OU ainda sem resposta. `abrir(nome)` abre a conversa e devolve o que a tela mostra (None = nao abriu).
#[allow(clippy::too_many_arguments)]
pub fn coletar(ctx: &Ctx, agora: &Agora, f: &Fontes, max_chats: usize, max_msgs: usize, max_idade_h: Option<f64>,
               abrir: &dyn Fn(&str) -> Option<Conversa>, pendente_remover: &dyn Fn(&str)) -> Result<Vec<Pendente>, String> {
    if f.lista.is_empty() {
        return Err("A lista de conversas do WhatsApp veio vazia (carregando ou deslogado).".into());
    }
    let mut itens = vec![];
    for nome in candidatos(ctx, f, agora) {
        if itens.len() >= max_chats {
            break;
        }
        let Some(conv) = abrir(&nome) else { continue };
        match montar(&nome, &conv, agora, max_msgs, max_idade_h) {
            Montagem::Pronta(p) => itens.push(p),
            Montagem::Velha => {}
            Montagem::Respondida => pendente_remover(&nome),
        }
    }
    Ok(itens)
}

/// Ponto do laco em que a closure `atalho` e consultada.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fase {
    /// Antes de registrar e decidir (so conversa 1:1): agentes que cuidam da familia e fazem o proprio envio.
    Antes,
    /// Depois das travas de grupo/conta automatica/encerramento: conselho, busca de lugar, corrida.
    Meio,
    /// Depois de agradecimento/elogio: respostas locais (prazos, financeiro, vaquinha).
    Fim,
}

/// Resposta pronta de um agente local, sem passar pelo decisor.
#[derive(Default, Clone, Debug)]
pub struct Atalho {
    /// Linha do relatorio ("" = nada a relatar). Na fase Antes, comecar com "✓" quer dizer que o agente ja enviou.
    pub linha: String,
    /// Texto a enviar a pessoa (fases Meio e Fim); None = o agente ja fez o que tinha de fazer.
    pub resposta: Option<String>,
    /// A resposta leva a apresentacao de 1o contato.
    pub apresentar: bool,
}

pub struct Portas<'a> {
    /// (a) leitura das conversas pendentes (ver `coletar`); Err = texto do erro.
    pub coletar: &'a dyn Fn() -> Result<Vec<Pendente>, String>,
    /// (b) decisor: (item, flags) -> (codigo E/I/A/P, texto). None = ninguem decidiu a tempo.
    pub decidir: &'a dyn Fn(&Item, &str) -> Option<(String, String)>,
    /// (c) revisor: recebe a linha V do protocolo e devolve a decisao revista, ou None.
    pub revisar: &'a dyn Fn(&str) -> Option<(String, String)>,
    /// (d) envios: (contato, texto) -> resultado; comeca com "✅" se enviou.
    pub enviar_texto: &'a dyn Fn(&str, &str) -> String,
    pub enviar_audio: &'a dyn Fn(&str, &str) -> String,
    /// Espaco entre envios (o WhatsApp limita quem dispara em sequencia).
    pub esperar: &'a dyn Fn(),
    /// Transcreve o audio mais recente da pessoa.
    pub ouvir_audio: &'a dyn Fn(&str) -> Result<String, String>,
    /// Descreve a imagem mais recente da pessoa.
    pub ver_imagem: &'a dyn Fn(&str) -> Result<String, String>,
    /// Documento recebido: (item, marcas [DOC:..], avisar a pessoa) -> linha do relatorio (ver wpp_doc::tratar_item).
    pub documento: &'a dyn Fn(&Item, &[String], &dyn Fn(&str)) -> String,
    pub atalho: &'a dyn Fn(Fase, &Item, &str) -> Option<Atalho>,
    /// Resposta curta e carinhosa para gente de casa (nome, o que ela disse); None = usa o acolhimento fixo.
    pub familia: &'a dyn Fn(&str, &str) -> Option<String>,
    /// Aciona o agente no pedido sobre o que entregamos: (contato, arquivo entregue, pedido).
    pub tarefa_entrega: &'a dyn Fn(&str, &str, &str),
    pub pendente_remover: &'a dyn Fn(&str),
    /// Recado ao dono em audio (None = texto).
    pub recado_audio: Option<&'a recados::Audio<'a>>,
}

#[derive(Default, Clone, Debug)]
pub struct Opcoes {
    /// Copia o relatorio ao dono quando ha algo para ele.
    pub relatar: bool,
    /// Modo automatico: grupo nunca recebe resposta sozinho.
    pub pular_grupos: bool,
    /// Enderecos do dono que podem ser citados (pessoal.json "links_proprios").
    pub links_proprios: Vec<String>,
}

/// Resultado do ultimo atendimento (o vigia automatico le daqui).
#[derive(Default, Clone, Debug, PartialEq)]
pub struct Ultimo {
    pub enviados: Vec<String>,
    pub linhas: Vec<String>,
    /// Conversas com decisao tomada (enviar/ignorar/avisar/perguntar).
    pub tratados: Vec<String>,
    pub grupos: Vec<String>,
}

fn log(dados: &Path, agora: &Agora, linha: &str) {
    use std::io::Write;
    let _ = std::fs::create_dir_all(dados);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dados.join("atendimentos.log")) {
        let _ = writeln!(f, "{} {}:{:02} | {}", agora.data.iso(), agora.hm(), agora.seg, linha.replace(['\r', '\n'], " "));
    }
}

fn pausas(dados: &Path) -> Value {
    std::fs::read_to_string(dados.join("atender_pausas.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(|| json!({}))
}

pub(crate) fn pausa(dados: &Path, tipo: &str, nome: &str) -> f64 {
    pausas(dados)[tipo][nome].as_f64().unwrap_or(0.0)
}

pub(crate) fn marcar_pausa(dados: &Path, tipo: &str, nome: &str, ts: f64) {
    let mut p = pausas(dados);
    if !p.is_object() {
        p = json!({});
    }
    if !p[tipo].is_object() {
        p[tipo] = json!({});
    }
    p[tipo][nome] = json!(ts);
    let _ = std::fs::create_dir_all(dados);
    let _ = std::fs::write(dados.join("atender_pausas.json"), p.to_string());
}

fn apresentar(dados: &Path, ctx: &Ctx, nome: &str, texto: &str, pergunta: &str) -> String {
    atender::com_apresentacao(ctx, nome, texto, pergunta, &memoria::historico(dados, nome, 30))
}

/// PADRAO: audio. Texto se a pessoa pediu por escrito (a preferencia fica salva), se a resposta e longa ou tem que ser lida, e
/// tambem se o audio falhar. Do outro lado e IA em GL1: sempre texto. O pai (contato "pai") so recebe audio, nunca texto.
/// Antes de sair passa pelo ultimo filtro (repeticao, vazamento, textao). Devolve (resultado, foi em audio); "silencio" = nada saiu.
fn enviar_resposta(dados: &Path, ctx: &Ctx, p: &Portas, nome: &str, texto: &str, it: &Item, ts: f64) -> (String, bool) {
    if it.gl_ativo {
        return ((p.enviar_texto)(nome, texto), false);
    }
    let mut texto = texto.to_string();
    if !it.msgs.is_empty() {
        let saidas: Vec<String> = memoria::historico(dados, nome, 12).into_iter().filter(|(d, _)| d == "C").map(|(_, t)| t).collect();
        match clareza::polir(ctx, nome, &texto, &it.msgs.join(" "), &saidas, &|tipo, det| clareza::evento(dados, ts, tipo, det)) {
            Some(t) => texto = t,
            None => return ("silencio".into(), false),
        }
    }
    if ctx.contato("pai") == Some(nome) {
        let r = (p.enviar_audio)(nome, &texto);
        return (if ok(&r) { r } else { format!("⚠️ audio ao pai falhou ({}); texto bloqueado", corta(&r, 60)) }, true);
    }
    contexto::atualizar_preferencia(dados, nome, &it.msgs.join(" "));
    if Ctx::ler(dados).deve_falar(nome, &texto) && texto.chars().count() <= VOZ_ATENDER_MAX {
        let r = (p.enviar_audio)(nome, &texto);
        if ok(&r) {
            return (r, true);
        }
    }
    ((p.enviar_texto)(nome, &texto), false)
}

/// Um atendimento: le as conversas pendentes e trata cada uma. Devolve (relatorio, resultado estruturado).
/// `ts` = segundos desde 1970; `agora` = hora local (log e teto diario).
pub fn atender(dados: &Path, agora: &Agora, ts: f64, op: &Opcoes, p: &Portas) -> (String, Ultimo) {
    let mut u = Ultimo::default();
    let itens = match (p.coletar)() {
        Ok(i) => i,
        Err(e) => return (e, u),
    };
    if itens.is_empty() {
        return ("Nenhuma mensagem nova para atender no WhatsApp. ✅".into(), u);
    }
    let lg = |l: &str| log(dados, agora, l);
    let registrar = |nome: &str, d: &str, t: &str, audio: bool| {
        let _ = memoria::registrar_msg(dados, nome, d, t, "whatsapp", audio, ts);
    };
    let mut linhas: Vec<String> = vec![];
    let mut enviados = 0usize;
    for Pendente { mut it, audio_dele } in itens {
        let ctx = Ctx::ler(dados); // relido a cada conversa: apresentados e preferencias mudam durante o atendimento
        let nome = it.nome.clone();
        let calado = ctx.nao_responder(&nome);
        if audio_dele && !calado { // OUVE o audio (local) antes de decidir
            let (txt, erro) = if ts - pausa(dados, "audio", &nome) < AUDIO_PAUSA_S {
                (String::new(), "ouvir falhou ha pouco: sem recarregar o WhatsApp de novo".to_string())
            } else {
                match (p.ouvir_audio)(&nome) {
                    Ok(t) => (t.trim().to_string(), String::new()),
                    Err(e) => {
                        marcar_pausa(dados, "audio", &nome, ts);
                        (String::new(), corta(&e, 80))
                    }
                }
            };
            it.msgs.retain(|m| m != "[AUDIO]");
            it.msgs.push(if txt.is_empty() { "[ÁUDIO não compreendido]".into() } else { format!("🎙 {txt}") });
            it.midia = txt.is_empty();
            lg(&format!("{nome} | audio ouvido | {:?}", corta(if txt.is_empty() { &erro } else { &txt }, 100)));
        }
        if it.msgs.iter().any(|m| m == "[IMAGEM]") && !calado && !(op.pular_grupos && it.grupo) { // grupo ignorado: nem gasta visao
            let (desc, erro) = match (p.ver_imagem)(&nome) {
                Ok(t) => (t.trim().to_string(), String::new()),
                Err(e) => (String::new(), corta(&e, 80)),
            };
            it.msgs.retain(|m| m != "[IMAGEM]");
            it.msgs.push(if desc.is_empty() { "[IMAGEM não compreendida]".into() } else { format!("🖼 {desc}") });
            it.midia = it.msgs.iter().any(|m| m.starts_with('[') && m.ends_with(']')); // sobrou midia que quem decide nao ve
            lg(&format!("{nome} | imagem vista | {:?}", corta(if desc.is_empty() { &erro } else { &desc }, 100)));
        }
        let docs: Vec<String> = it.msgs.iter().filter(|m| m.starts_with("[DOC:")).cloned().collect();
        if !docs.is_empty() && !calado && !it.grupo { // DOCUMENTO: avisa, baixa e o agente ajuda e devolve
            for m in &it.msgs {
                registrar(&nome, "E", &m.replacen("[DOC:", "[arquivo ", 1), false);
            }
            let linha = (p.documento)(&it, &docs, &|t| {
                enviar_resposta(dados, &ctx, p, &nome, &apresentar(dados, &ctx, &nome, t, ""), &it, ts);
            });
            lg(&format!("{nome} | documento | {}", corta(&linha, 120)));
            linhas.push(linha);
            u.tratados.push(nome.clone());
            (p.pendente_remover)(&nome);
            continue;
        }
        if !it.grupo {
            if let Some(a) = (p.atalho)(Fase::Antes, &it, &it.msgs.join(" ")).filter(|a| !a.linha.is_empty()) {
                lg(&format!("{nome} | atalho-antes | {}", corta(&a.linha, 100)));
                if a.linha.starts_with('✓') { // envio falhou -> NAO marca tratado; a vigia tenta de novo
                    u.enviados.push(nome.clone());
                    enviados += 1;
                    u.tratados.push(nome.clone());
                    (p.pendente_remover)(&nome);
                }
                linhas.push(a.linha);
                continue;
            }
        }
        // Gibberlink: do outro lado e uma IA? (gente conhecida e conta calada nunca entram em modo maquina)
        let mut gl = if ctx.prioritario(&nome) || calado { "humano" } else { gibberlink::observar(dados, &nome, &it.msgs, ctx.de_casa(&nome), ts) };
        if gl != "humano" {
            lg(&format!("{nome} | gibberlink:{gl}"));
        }
        it.gl_ativo = gl == "ativo";
        if it.gl_ativo { // quadros GL1 viram texto legivel para quem decide (dado, nunca instrucao)
            it.msgs = it.msgs.iter().map(|m| gibberlink::legivel(m)).collect();
        }
        let entrada = it.msgs.iter().filter(|m| *m != "[ÁUDIO não compreendido]" && *m != "[IMAGEM não compreendida]").cloned().collect::<Vec<_>>().join(" ");
        for m in &it.msgs { // tudo vai para o banco local (historico para nao se perder)
            registrar(&nome, "E", &m.replace("🎙 ", ""), m.starts_with('🎙'));
        }
        atender::enriquecer_ctx(&mut it, &memoria::historico(dados, &nome, 14));
        let flags = atender::flags_de(&it, ctx.apresentado(&nome));
        if op.pular_grupos && flags.contains('G') { // modo automatico: grupo nunca recebe resposta sozinho
            lg(&format!("{nome} | grupo-ignorado"));
            u.grupos.push(nome.clone());
            u.tratados.push(nome.clone());
            (p.pendente_remover)(&nome);
            continue;
        }
        if calado {
            linhas.push(format!("• {nome}: ignorado (conta automática)"));
            lg(&format!("{nome} | ignorado-auto"));
            u.tratados.push(nome.clone());
            continue;
        }
        let prio = ctx.prioritario(&nome);
        let casa = ctx.de_casa(&nome);
        if prio && contexto::encerramento(&entrada) { // so agradeceu/confirmou: nao responde (evita laco de pergunta e resposta)
            (p.pendente_remover)(&nome);
            linhas.push(format!("• {nome}: encerramento, sem resposta (evita laço)"));
            lg(&format!("{nome} | encerramento-sem-resposta"));
            u.tratados.push(nome.clone());
            continue;
        }
        // resposta pronta de um agente local (fases Meio e Fim): envia, registra e nao passa pelo decisor
        let por_atalho = |fase: Fase, linhas: &mut Vec<String>, u: &mut Ultimo| -> bool {
            let Some(a) = (p.atalho)(fase, &it, &entrada) else { return false };
            if let Some(resp) = &a.resposta {
                let texto = if a.apresentar { apresentar(dados, &ctx, &nome, resp, "") } else { resp.clone() };
                let (r, audio) = enviar_resposta(dados, &ctx, p, &nome, &texto, &it, ts);
                if ok(&r) {
                    if a.apresentar {
                        let _ = contexto::marcar_apresentado(dados, &nome);
                    }
                    registrar(&nome, "C", &texto, audio);
                    u.enviados.push(nome.clone());
                    if !a.linha.is_empty() {
                        linhas.push(a.linha.clone());
                    }
                }
            } else if !a.linha.is_empty() {
                linhas.push(a.linha.clone());
            }
            lg(&format!("{nome} | atalho | {}", corta(&a.linha, 100)));
            u.tratados.push(nome.clone());
            (p.pendente_remover)(&nome);
            true
        };
        if por_atalho(Fase::Meio, &mut linhas, &mut u) {
            continue;
        }
        let ent = if casa || prio || it.grupo { String::new() } else { atender::entrega_a(&memoria::historico(dados, &nome, 40)) };
        if !ent.is_empty() && atender::pedido_sobre_entrega(&entrada, &memoria::historico(dados, &nome, 6)) {
            // pedido sobre o que entregamos: nao inventa; anota por escrito (uma vez) e passa ao dono
            let ja = memoria::historico(dados, &nome, 8).iter().any(|(d, t)| d == "C" && t == ANOTADO_ENTREGA);
            if !ja && ok(&(p.enviar_texto)(&nome, ANOTADO_ENTREGA)) {
                registrar(&nome, "C", ANOTADO_ENTREGA, false);
                u.enviados.push(nome.clone());
            }
            let mut fazendo = ts - pausa(dados, "entrega", &nome) < ENTREGA_PAUSA_S; // ja ha um agente neste pedido
            if !fazendo && !crate::dono::proibida(&entrada) && wpp_doc::pode_hoje(dados, agora.data) { // mesmo teto diario dos documentos
                marcar_pausa(dados, "entrega", &nome, ts);
                wpp_doc::usar(dados, agora.data);
                (p.tarefa_entrega)(&nome, &ent, &entrada);
                fazendo = true;
            }
            recados::enfileirar(dados, &nome, &entrada, "", &format!("pediu ajuste ou texto sobre o que entregamos ({ent}); {}",
                if fazendo { "o agente está fazendo e devolve a ele" } else { "não inventei resposta: falta fazer e mandar para ele" }), ts);
            linhas.push(format!("📩 {nome}: pedido sobre a entrega {ent}: «{}» ({})", corta(&entrada, 90), if fazendo { "agente fazendo" } else { "anotei e passei ao dono" }));
            lg(&format!("{nome} | pedido-entrega{} | {}", if fazendo { " | agente" } else { "" }, corta(&entrada, 100)));
            u.tratados.push(nome.clone());
            (p.pendente_remover)(&nome);
            continue;
        }
        if let Some(por_nada) = contexto::agradecimento(&entrada).filter(|_| (ctx.apresentado(&nome) || memoria::ja_falamos(dados, &nome)) && !prio) {
            // so "valeu": nao se reapresenta. Gente de casa recebe um "por nada"; os demais ficam sem resposta (evita laco)
            let enviado = casa.then(|| enviar_resposta(dados, &ctx, p, &nome, por_nada, &it, ts));
            if enviado.as_ref().is_none_or(|(r, _)| ok(r)) {
                if let Some((_, audio)) = enviado {
                    registrar(&nome, "C", por_nada, audio);
                    u.enviados.push(nome.clone());
                }
                linhas.push(format!("• {nome}: agradeceu{}", if casa { " (respondi curto)" } else { " (sem resposta necessária)" }));
                lg(&format!("{nome} | agradecimento | {}", if casa { por_nada } else { "sem resposta" }));
                u.tratados.push(nome.clone());
                (p.pendente_remover)(&nome);
                continue;
            }
        }
        if let Some(obrigado) = contexto::elogio(&entrada).filter(|_| ctx.apresentado(&nome)) { // so elogio: agradece curto, sem repetir quem e
            let (r, audio) = enviar_resposta(dados, &ctx, p, &nome, obrigado, &it, ts);
            if ok(&r) {
                registrar(&nome, "C", obrigado, audio);
                u.enviados.push(nome.clone());
                linhas.push(format!("✓ {nome} (elogio): agradeci"));
                lg(&format!("{nome} | elogio | {obrigado}"));
            }
            u.tratados.push(nome.clone());
            (p.pendente_remover)(&nome);
            continue;
        }
        if por_atalho(Fase::Fim, &mut linhas, &mut u) {
            continue;
        }
        let regra = if prio || casa { None } else {
            memoria::decisao_repetida(dados, &nome, &entrada, &["I"], 30.0, ts).or_else(|| memoria::decisao_repetida(dados, &nome, &entrada, &["A"], 1.0, ts))
        };
        if let Some(regra) = regra { // ja foi decidido EXATAMENTE isto antes: nao gasta pergunta
            (p.pendente_remover)(&nome);
            linhas.push(format!("• {nome}: igual a uma mensagem já tratada ({}), sem nova pergunta", if regra == "I" { "ignorada" } else { "avisada" }));
            lg(&format!("{nome} | regra-aprendida:{regra}"));
            u.tratados.push(nome.clone());
            continue;
        }
        let mut dec = (p.decidir)(&it, &flags);
        if dec.is_none() && prio && pergunta_prio().is_match(&entrada) { // ninguem decidiu a tempo: so cobre se ela perguntou/pediu algo
            dec = Some(("E".into(), RESERVA_PRIORITARIO.into()));
            lg(&format!("{nome} | sem-decisao: resposta de reserva para o prioritario"));
        }
        // 1o contato (contas automaticas e grupos ficam de fora)
        let novo = flags.contains('N') && !flags.contains('G') && !memoria::ja_falamos(dados, &nome);
        if dec.is_none() && !atender::nao_responder_ou_grupo(&ctx, &nome, &flags) {
            // ninguem fica sem resposta: contato novo = se apresenta e pergunta como ajudar; os demais = resposta neutra; o dono ve no relatorio
            dec = Some(("E".into(), if novo { PRIMEIRO_CONTATO } else { revisao::NEUTRA }.to_string()));
            linhas.push(format!("⏸ {nome}: ninguém decidiu a tempo; respondi com {} e deixei a mensagem pra você: «{}»", if novo { "a apresentação" } else { "um recebimento" }, corta(&entrada, 100)));
            lg(&format!("{nome} | sem-decisao: resposta imediata ({})", if novo { "apresentacao" } else { "neutra" }));
        }
        let Some((cod, texto)) = dec else {
            linhas.push(format!("⏸ {nome}: ninguém decidiu a tempo, segurei"));
            lg(&format!("{nome} | sem-decisao"));
            continue;
        };
        // NADA sai sem passar pela revisao: o auditor barra rotular a pessoa, repetir o que ja foi dito e ignorar pedido de cliente
        let saidas = memoria::saidas_recentes(dados, &nome, 24.0, ts);
        let id = format!("{:06x}", (ts as u64).wrapping_mul(31).wrapping_add(nome.len() as u64 + linhas.len() as u64) & 0xff_ffff);
        let (mut cod, mut texto, _) = revisao::decidir_com_revisao(&it, &flags, &cod, &texto, &saidas, false, &id, p.revisar, &|l| revisao::anotar(dados, agora, l));
        if cod == "E" && contexto::repete_negado(&entrada, &texto).is_some() { // a pessoa corrigiu e a resposta repetiu o erro
            (cod, texto) = ("E".into(), CORRECAO.into());
            lg(&format!("{nome} | correcao: resposta repetia o que a pessoa negou"));
        }
        if ["I", "A", "P"].contains(&cod.as_str()) && casa { // gente de casa nunca fica sem resposta
            let acolhe = (p.familia)(&nome, &entrada).filter(|r| !r.trim().is_empty() && r.chars().count() < 600).map(|r| r.trim().to_string());
            texto = acolhe.unwrap_or_else(|| format!("Oi{}! Recebi sua mensagem e já avisei{}. Cuida bem de você, tá?",
                contexto::primeiro_nome(&nome).map_or(String::new(), |n| format!(", {n}")), if ctx.dono.is_empty() { String::new() } else { format!(" o {}", ctx.dono) }));
            cod = "E".into();
            lg(&format!("{nome} | familia: nao ignora, respondeu"));
        }
        if prio && ["I", "A", "P"].contains(&cod.as_str()) && pergunta_prio().is_match(&entrada) {
            (cod, texto) = ("E".into(), RETORNO_PRIORITARIO.into()); // ela perguntou/pediu algo: nunca fica sem resposta
        }
        let mut aviso_novo = None;
        if novo && !prio {
            let (cod0, texto0) = (cod.clone(), texto.clone());
            if cod != "E" { // contato novo NUNCA e ignorado, segurado ou so avisado: responde se apresentando e pergunta como ajudar
                (cod, texto) = ("E".into(), PRIMEIRO_CONTATO.into());
                lg(&format!("{nome} | contato novo: decisao {cod0} trocada por apresentacao"));
            }
            let avisar = cod0 == "A" || cod0 == "P";
            if atender::importante(&entrada, &ctx.dono) || avisar {
                let nota = corta(if avisar && !texto0.is_empty() { &texto0 } else { &entrada }, 140);
                aviso_novo = Some(format!("⚠️ {nome} (contato novo, parece importante): «{nota}» — já me apresentei e respondi"));
            }
        }
        let _ = memoria::registrar_decisao(dados, &nome, &entrada, &cod, &texto, ts); // a decisao (ja revisada) fica no banco
        let mut recado_na_fila = false;
        let avisa = cod == "A" || cod == "P";
        if !prio && !flags.contains('G') && (avisa || (aviso_novo.is_some() && cod == "E")) { // RECADOS: literal, em fila, com nova tentativa
            recado_na_fila = recados::enfileirar(dados, &nome, &entrada, "", if avisa { &texto } else { "" }, ts);
        }
        u.tratados.push(nome.clone());
        if cod != "P" {
            (p.pendente_remover)(&nome); // tratada; "P" (segurar) continua pendente
        }
        match cod.as_str() {
            "E" => {
                let mut valida = atender::validar_resposta(&texto, &op.links_proprios);
                if valida.is_err_and(|m| m.starts_with("longa")) { // nao descarta: pede de novo, mais curta (flag L no protocolo)
                    if let Some((c2, t2)) = (p.decidir)(&it, &format!("{flags}L")) {
                        if c2 == "E" {
                            texto = t2;
                            valida = atender::validar_resposta(&texto, &op.links_proprios);
                            lg(&format!("{nome} | resposta longa refeita ({} chars)", texto.chars().count()));
                        }
                    }
                }
                if let Err(motivo) = valida {
                    linhas.push(format!("⚠️ {nome}: resposta barrada ({motivo}), segurei"));
                    lg(&format!("{nome} | barrada:{motivo} | {}", corta(&texto, 80)));
                    continue;
                }
                if enviados > 0 { // espaca os envios: evita o WhatsApp limitar o numero
                    (p.esperar)();
                }
                let natural = apresentar(dados, &ctx, &nome, &texto, "");
                let mut final_ = if gl == "ativo" { // IA <-> IA: quadro compacto, sem cortesia (1o contato leva quem somos num campo)
                    let quem = gibberlink::quem_somos(&ctx.dono);
                    gibberlink::compactar(&texto, (!ctx.apresentado(&nome)).then_some(quem.as_str()), &ctx.dono)
                } else {
                    apresentar(dados, &ctx, &nome, &texto, &entrada)
                };
                if gl == "ativo" && atender::validar_resposta(&final_, &op.links_proprios).is_err() { // quadro estranho: volta para a fala normal
                    lg(&format!("{nome} | gibberlink: quadro barrado, enviado em texto normal"));
                    (gl, it.gl_ativo, final_) = ("humano", false, natural.clone());
                }
                if gl == "oferecer" {
                    final_ = gibberlink::com_convite(&final_);
                }
                let (r, audio) = enviar_resposta(dados, &ctx, p, &nome, &final_, &it, ts);
                if ok(&r) {
                    if gl == "oferecer" {
                        gibberlink::marcar_oferecido(dados, &nome, ts);
                    } else if gl == "ativo" { // mede a economia antes de espalhar o GL1
                        gibberlink::medir(dados, &nome, &natural, &final_, ts);
                    }
                    let como = if audio { "áudio" } else if gl == "ativo" { "GL1" } else { "texto" };
                    let _ = contexto::marcar_apresentado(dados, &nome);
                    registrar(&nome, "C", &final_, audio);
                    enviados += 1;
                    u.enviados.push(nome.clone());
                    linhas.push(format!("✓ {nome} ({como}): «{}»", corta(&final_, 90)));
                    lg(&format!("{nome} | enviado({como}) | {final_}"));
                    let dos_pais = [ctx.contato("mae"), ctx.contato("pai")].contains(&Some(nome.as_str()));
                    if !prio && !recado_na_fila && dos_pais && entrada.split_whitespace().count() >= 4 {
                        recado_na_fila = recados::enfileirar(dados, &nome, &entrada, &texto, "", ts); // o que a mae/o pai disseram o dono sempre ouve
                    }
                    if !prio && !recado_na_fila && !flags.contains('G') && recados::promete_repassar(&texto, &ctx.dono) {
                        recado_na_fila = recados::enfileirar(dados, &nome, &entrada, &texto, "", ts); // prometeu repassar: o recado vai de verdade
                        if recado_na_fila {
                            lg(&format!("{nome} | recado enfileirado (promessa na resposta)"));
                        }
                    }
                    if let Some(a) = aviso_novo { // linha com ⚠️ => o relatorio vai para o WhatsApp do dono
                        linhas.push(if recado_na_fila { format!("📩 {}", a.trim_start_matches(['⚠', '\u{fe0f}', ' '])) } else { a });
                    }
                } else {
                    linhas.push(format!("⚠️ {nome}: falhou o envio ({})", corta(&r, 60)));
                    lg(&format!("{nome} | falha-envio | {}", corta(&r, 80)));
                }
            }
            "I" => {
                linhas.push(format!("• {nome}: sem resposta necessária"));
                lg(&format!("{nome} | ignorado"));
            }
            "A" => {
                linhas.push(format!("{} {nome}: {}", if recado_na_fila { "📩" } else { "⚠️" }, corta(&texto, 140)));
                lg(&format!("{nome} | avisar | {}", corta(&texto, 100)));
            }
            _ => { // P: depende do dono; o texto e a pergunta exata
                linhas.push(if texto.is_empty() { format!("⏸ {nome}: segurei, aguardando você") } else { format!("{} {nome}: {}", if recado_na_fila { "📩" } else { "❓" }, corta(&texto, 160)) });
                lg(&format!("{nome} | segurado"));
            }
        }
    }
    // entrega os recados da fila ao dono (literais; se o WhatsApp falhar, a rotina tenta de novo)
    let ctx = Ctx::ler(dados);
    let dono = ctx.chat_do_dono();
    if let Some(d) = dono.filter(|_| !recados::pendentes(dados).is_empty()) {
        recados::entregar(dados, d, ts, p.enviar_texto, p.recado_audio);
    }
    u.linhas = linhas.clone();
    if linhas.is_empty() { // tudo foi ignorado em silencio (grupos etc.): nao ha o que relatar
        return ("Nenhuma conversa para atender (ignoradas).".into(), u);
    }
    let relatorio = format!("Jarvis ✓ atendimento: {}", linhas.join(" | "));
    // o dono so recebe relatorio quando ha algo para ele (aviso, pergunta ou problema); respostas normais nao viram ruido
    let precisa_dele = linhas.iter().any(|l| l.starts_with("⚠️") || l.starts_with('❓') || l.starts_with('⏸'));
    if let Some(d) = dono.filter(|_| op.relatar && precisa_dele) {
        (p.enviar_texto)(d, &corta(&relatorio, 900));
    }
    (relatorio, u)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;
    use std::cell::{Cell, RefCell};
    use std::path::PathBuf;

    const TS: f64 = 1.8e9;

    fn agora() -> Agora {
        Agora { data: Data::civil(2026, 10, 1).unwrap(), hora: 18, min: 0, seg: 0 }
    }

    fn tmp(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-atender_laco-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("pessoal.json"), r#"{"nome": "Davi", "contatos": {"davi": "Eu(D)", "mae": "Mainha", "pai": "Painho"},
            "nao_responder": ["cobranca"], "prioritarios": ["loja"], "de_casa": ["mae", "pai"]}"#).unwrap();
        d
    }

    fn v(m: &[&str]) -> Vec<String> {
        m.iter().map(|x| x.to_string()).collect()
    }

    fn item(nome: &str, msgs: &[&str]) -> Pendente {
        Pendente { it: Item { nome: nome.into(), msgs: v(msgs), autores: v(&["x"]), ..Default::default() }, audio_dele: false }
    }

    type Env = RefCell<Vec<(String, String)>>;

    fn anota(env: &Env) -> impl Fn(&str, &str) -> String + '_ {
        move |n, t| {
            env.borrow_mut().push((n.to_string(), t.to_string()));
            format!("✅ Mensagem enviada para {n}.")
        }
    }

    fn para(env: &Env, nome: &str) -> Vec<String> {
        env.borrow().iter().filter(|(n, _)| n == nome).map(|(_, t)| t.clone()).collect()
    }

    fn portas<'a>(coletar: &'a dyn Fn() -> Result<Vec<Pendente>, String>, decidir: &'a dyn Fn(&Item, &str) -> Option<(String, String)>,
                  enviar: &'a dyn Fn(&str, &str) -> String) -> Portas<'a> {
        Portas { coletar, decidir, revisar: &|_| None, enviar_texto: enviar, enviar_audio: &|_, _| "sem voz no teste".into(), esperar: &|| (),
            ouvir_audio: &|_| Err("sem ouvido".into()), ver_imagem: &|_| Err("sem visao".into()), documento: &|_, _, _| panic!("documento inesperado"),
            atalho: &|_, _, _| None, familia: &|_, _| None, tarefa_entrega: &|_, _, _| (), pendente_remover: &|_| (), recado_audio: None }
    }

    /// Roda um atendimento com decisoes fixas por nome; devolve (relatorio, resultado).
    fn rodar(d: &Path, env: &Env, itens: Vec<Pendente>, decisoes: &[(&str, &str, &str)]) -> (String, Ultimo) {
        let enviar = anota(env);
        let coletar = || Ok(itens.clone());
        let decidir = |it: &Item, _: &str| decisoes.iter().find(|(n, _, _)| *n == it.nome).map(|(_, c, t)| (c.to_string(), t.to_string()));
        atender(d, &agora(), TS, &Opcoes { relatar: true, ..Default::default() }, &portas(&coletar, &decidir, &enviar))
    }

    fn conhecido(d: &Path, nome: &str) {
        contexto::marcar_apresentado(d, nome).unwrap();
        memoria::registrar_msg(d, nome, "C", "Oi! Eu sou o Jarvis.", "whatsapp", false, TS - 90000.0).unwrap();
    }

    #[test]
    fn envia_apresenta_registra_e_nao_incomoda_o_dono() {
        let (d, env) = (tmp("envia"), Env::default());
        let (rel, u) = rodar(&d, &env, vec![item("Maria", &["oi, tem vaga?"])], &[("Maria", "E", "Oi! Posso ajudar sim, qual a data?")]);
        let ctx = Ctx::ler(&d);
        assert_eq!(env.borrow()[0], ("Maria".to_string(), format!("{}Oi! Posso ajudar sim, qual a data?", Ctx::ler(&tmp("envia-b")).abertura_padrao("Maria"))));
        assert!(ctx.apresentado("Maria") && rel.contains("✓ Maria (texto)"));
        assert!(para(&env, "Eu(D)").is_empty()); // resposta normal NAO vira relatorio para o dono
        assert_eq!((u.enviados.clone(), u.tratados.clone()), (v(&["Maria"]), v(&["Maria"])));
        assert!(u.linhas[0].starts_with("✓ Maria"));
        let h = memoria::historico(&d, "Maria", 5);
        assert_eq!((h[0].0.as_str(), h[0].1.as_str(), h[1].0.as_str()), ("E", "oi, tem vaga?", "C")); // os dois lados ficam no banco
        assert!(std::fs::read_to_string(d.join("atendimentos.log")).unwrap().contains("Maria | enviado(texto)"));
        // so se apresenta uma vez
        env.borrow_mut().clear();
        rodar(&d, &env, vec![item("Maria", &["e o horario?"])], &[("Maria", "E", "As 14h.")]);
        assert_eq!(para(&env, "Maria"), v(&["As 14h."]));
    }

    #[test]
    fn audio_e_o_padrao_e_o_espaco_entre_envios_vale_a_partir_do_segundo() {
        let (d, env, voz, esperas) = (tmp("audio"), Env::default(), Env::default(), Cell::new(0));
        let (enviar, falar) = (anota(&env), anota(&voz));
        let coletar = || Ok(vec![item("Ana", &["tem vaga?"]), item("Bia", &["me responde por escrito, tem vaga?"]), item("Caio", &["oi?"])]);
        let decidir = |_: &Item, _: &str| Some(("E".to_string(), "Tem sim, qual a data?".to_string()));
        let espera = || esperas.set(esperas.get() + 1);
        let (rel, u) = atender(&d, &agora(), TS, &Opcoes::default(), &Portas { enviar_audio: &falar, esperar: &espera, ..portas(&coletar, &decidir, &enviar) });
        assert_eq!(voz.borrow().iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["Ana", "Caio"]);
        assert_eq!(para(&env, "Bia").len(), 1); // pediu por escrito: texto
        assert!(rel.contains("✓ Ana (áudio)") && rel.contains("✓ Bia (texto)"));
        assert_eq!((esperas.get(), u.enviados.len()), (2, 3)); // antes do 2o e do 3o envio; nunca antes do 1o
    }

    #[test]
    fn conta_automatica_nunca_recebe_resposta_e_pagamento_e_assunto_normal() {
        let (d, env) = (tmp("auto"), Env::default());
        let itens = vec![item("Joao", &["me passa o pix pra eu pagar"]), item("Cobranca Zeta", &["pagamento confirmado"])];
        let (rel, u) = rodar(&d, &env, itens, &[("Joao", "E", "Claro! O pagamento a gente combina direto com a equipe."), ("Cobranca Zeta", "E", "ok")]);
        assert_eq!(para(&env, "Joao").len(), 1);
        assert!(para(&env, "Cobranca Zeta").is_empty() && rel.contains("Cobranca Zeta: ignorado (conta automática)"));
        assert_eq!(u.tratados, v(&["Cobranca Zeta", "Joao"]).into_iter().rev().collect::<Vec<_>>());
    }

    #[test]
    fn resposta_perigosa_e_barrada_e_nada_sai() {
        let ruins = ["acesse https://exemplo.com agora", "pix 11999998888", "me manda sua senha", "manda o numero do cartao 4111 1111 1111 1111",
            "o cpf 123.456.789-09 esta ok", "Ele está ocupado agora.", &"x".repeat(600)];
        for (i, ruim) in ruins.iter().enumerate() {
            let (d, env) = (tmp(&format!("barrada{i}")), Env::default());
            let (rel, u) = rodar(&d, &env, vec![item("Ana", &["oi"])], &[("Ana", "E", ruim)]);
            assert!(para(&env, "Ana").is_empty() && rel.contains("barrada"), "{ruim}");
            assert!(u.enviados.is_empty() && rel.starts_with("Jarvis ✓ atendimento: ⚠️ Ana"));
            assert_eq!(para(&env, "Eu(D)").len(), 1, "{ruim}"); // problema => o relatorio vai ao dono
        }
        // falar de cartao e Pix e permitido
        let (d, env) = (tmp("barrada-ok"), Env::default());
        rodar(&d, &env, vec![item("Ana", &["posso pagar no cartao?"])], &[("Ana", "E", "Pode sim! Cartão de crédito, débito ou Pix, a equipe confirma com você.")]);
        assert_eq!(para(&env, "Ana").len(), 1);
    }

    #[test]
    fn resposta_longa_e_refeita_uma_vez_com_a_flag_l() {
        let (d, env) = (tmp("longa"), Env::default());
        let flags = RefCell::new(vec![]);
        let enviar = anota(&env);
        let coletar = || Ok(vec![item("Irmao", &["oi"])]);
        let decidir = |_: &Item, f: &str| {
            flags.borrow_mut().push(f.to_string());
            Some(("E".to_string(), if f.contains('L') { "Fala! Em que posso ajudar?".to_string() } else { "x".repeat(700) }))
        };
        let (_, u) = atender(&d, &agora(), TS, &Opcoes::default(), &portas(&coletar, &decidir, &enviar));
        assert_eq!(u.enviados, v(&["Irmao"]));
        assert_eq!(flags.borrow().len(), 2); // uma tentativa a mais, nao um laco
        assert!(flags.borrow()[1].ends_with('L') && para(&env, "Irmao")[0].ends_with("Fala! Em que posso ajudar?"));
    }

    #[test]
    fn codigos_ignorar_avisar_segurar_e_sem_decisao() {
        let (d, env) = (tmp("codigos"), Env::default());
        for n in ["A1", "B1", "C1", "D1"] {
            conhecido(&d, n);
        }
        let itens = ["A1", "B1", "C1", "D1"].iter().map(|n| item(n, &["x"])).collect();
        let (rel, u) = rodar(&d, &env, itens, &[("A1", "I", ""), ("B1", "A", "hospede pediu desconto"), ("C1", "P", "")]);
        assert_eq!(env.borrow().iter().filter(|(n, _)| n != "Eu(D)").map(|(n, t)| (n.as_str(), t.as_str())).collect::<Vec<_>>(), [("D1", revisao::NEUTRA)]);
        assert!(rel.contains("A1: sem resposta necessária") && rel.contains("B1: hospede pediu desconto"), "{rel}");
        assert!(rel.contains("C1: segurei") && rel.contains("D1: ninguém decidiu a tempo; respondi com um recebimento"));
        assert_eq!(u.tratados, v(&["A1", "B1", "C1", "D1"]));
        let ao_dono = para(&env, "Eu(D)");
        assert!(ao_dono.iter().any(|t| t.contains("Recado de B1")) && ao_dono.last().unwrap().starts_with("Jarvis ✓ atendimento")); // recado literal + relatorio
    }

    #[test]
    fn sem_mensagens_erro_de_leitura_e_tudo_ignorado() {
        let (d, env) = (tmp("vazio"), Env::default());
        assert!(rodar(&d, &env, vec![], &[]).0.contains("Nenhuma mensagem nova"));
        let enviar = anota(&env);
        let (erro, nada) = (|| Err("A lista veio vazia".to_string()), |_: &Item, _: &str| -> Option<(String, String)> { panic!("sem conversa nao ha decisao") });
        assert_eq!(atender(&d, &agora(), TS, &Opcoes::default(), &portas(&erro, &nada, &enviar)).0, "A lista veio vazia");
        assert!(env.borrow().is_empty()); // nem relatorio
    }

    #[test]
    fn modo_automatico_pula_grupos_sem_perguntar_a_ninguem() {
        let (d, env) = (tmp("grupo"), Env::default());
        let enviar = anota(&env);
        let removidos = RefCell::new(vec![]);
        let grupo = Pendente { it: Item { nome: "Turma".into(), msgs: v(&["oi pessoal", "[IMAGEM]"]), autores: v(&["A", "B"]), grupo: true, ..Default::default() }, audio_dele: false };
        let coletar = || Ok(vec![grupo.clone()]);
        let decidir = |_: &Item, _: &str| -> Option<(String, String)> { panic!("grupo nao e perguntado a ninguem") };
        let remover = |n: &str| removidos.borrow_mut().push(n.to_string());
        let p = Portas { pendente_remover: &remover, ver_imagem: &|_| panic!("grupo ignorado nem gasta visao"), ..portas(&coletar, &decidir, &enviar) };
        let (rel, u) = atender(&d, &agora(), TS, &Opcoes { pular_grupos: true, ..Default::default() }, &p);
        assert!(rel.contains("ignoradas") && u.enviados.is_empty() && env.borrow().is_empty());
        assert_eq!((u.tratados, u.grupos, removidos.into_inner()), (v(&["Turma"]), v(&["Turma"]), v(&["Turma"]))); // o vigia nao volta a ela
    }

    #[test]
    fn grupo_sem_decisao_fica_segurado_e_nunca_recebe_a_neutra() {
        let (d, env) = (tmp("grupo2"), Env::default());
        let grupo = Pendente { it: Item { nome: "Turma".into(), msgs: v(&["oi pessoal"]), autores: v(&["A", "B"]), ..Default::default() }, audio_dele: false };
        let (rel, _) = rodar(&d, &env, vec![grupo], &[]);
        assert!(rel.contains("Turma: ninguém decidiu a tempo, segurei") && para(&env, "Turma").is_empty());
    }

    #[test]
    fn contato_novo_sempre_se_apresenta_e_o_importante_chega_ao_dono() {
        // decisao de ignorar: ainda assim se apresenta
        let (d, env) = (tmp("novo1"), Env::default());
        rodar(&d, &env, vec![item("Fulano Novo", &["oi"])], &[("Fulano Novo", "I", "")]);
        let e = para(&env, "Fulano Novo");
        assert!(e[0].contains("Jarvis") && e[0].contains("inteligência artificial") && e[0].ends_with(PRIMEIRO_CONTATO));
        assert!(Ctx::ler(&d).apresentado("Fulano Novo") && para(&env, "Eu(D)").is_empty()); // saudacao simples nao incomoda o dono
        // ninguem decidiu: nao fica calado
        let (d, env) = (tmp("novo2"), Env::default());
        let (rel, _) = rodar(&d, &env, vec![item("Sicrano Novo", &["bom dia, queria uma informacao"])], &[]);
        assert!(para(&env, "Sicrano Novo")[0].contains("Jarvis") && rel.contains("respondi com a apresentação"));
        // decisao de segurar: responde se apresentando E o recado LITERAL vai ao dono
        let (d, env) = (tmp("novo3"), Env::default());
        let (rel, _) = rodar(&d, &env, vec![item("Beltrano Novo", &["preciso falar com o Davi sobre uma proposta"])], &[("Beltrano Novo", "P", "Beltrano quer falar com voce")]);
        assert_eq!(para(&env, "Beltrano Novo").len(), 1);
        let aviso = para(&env, "Eu(D)");
        assert!(aviso.iter().any(|t| t.contains("Recado de Beltrano Novo") && t.contains("preciso falar com o Davi sobre uma proposta")), "{aviso:?}");
        assert!(rel.contains("📩 Beltrano Novo (contato novo, parece importante): «Beltrano quer falar com voce»"), "{rel}");
        // resposta normal, mas a mensagem e importante
        let (d, env) = (tmp("novo4"), Env::default());
        rodar(&d, &env, vec![item("Novo Urgente", &["Oi, é urgente, me liga por favor"])], &[("Novo Urgente", "E", "Oi! Já vi, me diga o que aconteceu.")]);
        assert!(para(&env, "Novo Urgente").len() == 1 && para(&env, "Eu(D)").iter().any(|t| t.contains("Novo Urgente")));
        // conta automatica nova continua sem resposta
        let (d, env) = (tmp("novo5"), Env::default());
        rodar(&d, &env, vec![item("Cobranca Nova", &["Opa! Nao entendi o que voce digitou"])], &[("Cobranca Nova", "E", "oi")]);
        assert!(env.borrow().is_empty());
    }

    #[test]
    fn regra_aprendida_nao_pergunta_de_novo_a_mesma_mensagem() {
        let (d, env) = (tmp("regra"), Env::default());
        conhecido(&d, "Ana");
        let (enviar, n) = (anota(&env), Cell::new(0));
        let coletar = || Ok(vec![item("Ana", &["promoção imperdível só hoje"])]);
        let decidir = |_: &Item, _: &str| {
            n.set(n.get() + 1);
            Some(("I".to_string(), String::new()))
        };
        let p = portas(&coletar, &decidir, &enviar);
        atender(&d, &agora(), TS, &Opcoes::default(), &p);
        let (rel, u) = atender(&d, &agora(), TS + 60.0, &Opcoes::default(), &p);
        assert_eq!(n.get(), 1);
        assert!(rel.contains("igual a uma mensagem já tratada (ignorada)") && u.tratados == v(&["Ana"]) && env.borrow().is_empty());
    }

    #[test]
    fn gente_de_casa_e_prioritario_nunca_ficam_sem_resposta() {
        let (d, env) = (tmp("casa"), Env::default());
        conhecido(&d, "Mainha");
        conhecido(&d, "Loja Sol");
        let itens = vec![item("Mainha", &["meu filho, a chuva derrubou o muro aqui de casa"]), item("Loja Sol", &["deu erro no caixa, me ajuda"])];
        let (rel, _) = rodar(&d, &env, itens, &[("Mainha", "I", ""), ("Loja Sol", "A", "pergunta do caixa")]);
        assert_eq!(para(&env, "Mainha"), v(&["Oi, Mainha! Recebi sua mensagem e já avisei o Davi. Cuida bem de você, tá?"]));
        assert_eq!(para(&env, "Loja Sol"), v(&[RETORNO_PRIORITARIO]));
        assert!(para(&env, "Eu(D)").iter().any(|t| t.contains("Recado de Mainha")), "{rel}"); // o que a mae disse o dono sempre ouve
        // a closure da familia escreve a resposta; a regra aprendida NAO vale para gente de casa
        let (enviar, n) = (anota(&env), Cell::new(0));
        let coletar = || Ok(vec![item("Mainha", &["meu filho, a chuva derrubou o muro aqui de casa"])]);
        let decidir = |_: &Item, _: &str| {
            n.set(n.get() + 1);
            Some(("I".to_string(), String::new()))
        };
        let p = Portas { familia: &|_, _| Some(" Que susto, minha tia! Já falo com ele. ".into()), ..portas(&coletar, &decidir, &enviar) };
        atender(&d, &agora(), TS + 7.0 * 3600.0, &Opcoes::default(), &p);
        assert_eq!((n.get(), para(&env, "Mainha").last().unwrap().as_str()), (1, "Que susto, minha tia! Já falo com ele."));
        // prioritario so agradeceu: sem resposta (evita laco); sem decisao e com pergunta: resposta de reserva
        let (d, env) = (tmp("casa2"), Env::default());
        conhecido(&d, "Loja Sol");
        let (rel, _) = rodar(&d, &env, vec![item("Loja Sol", &["ok, obrigada"])], &[("Loja Sol", "E", "de nada")]);
        assert!(env.borrow().is_empty() && rel.contains("encerramento"));
        rodar(&d, &env, vec![item("Loja Sol", &["qual o preço da esfirra?"])], &[]);
        assert_eq!(para(&env, "Loja Sol"), v(&[RESERVA_PRIORITARIO]));
    }

    #[test]
    fn revisao_antes_de_agir_repeticao_cala_e_rotulo_vai_ao_revisor() {
        // repete o que ja foi dito e nao ha revisor: nada e enviado e o dono nao e incomodado
        let (d, env) = (tmp("revisao1"), Env::default());
        conhecido(&d, "Teo");
        memoria::registrar_msg(&d, "Teo", "C", "Teo, 31 de fevereiro não existe, rs. Me passa a data certinha e o horário do casamento", "whatsapp", false, TS - 600.0).unwrap();
        let (rel, u) = rodar(&d, &env, vec![item("Teo", &["então dia 31"])], &[("Teo", "E", "Opa, Teo! Só um detalhe: 31 de fevereiro não existe, rs. Me passa a data certinha e o horário do casamento")]);
        assert!(env.borrow().is_empty() && u.enviados.is_empty() && rel.contains("Teo: sem resposta necessária"), "{rel}");
        assert!(std::fs::read_to_string(d.join("revisoes.log")).unwrap().contains("AUTO-IGNORADA"));
        // nota que rotula a pessoa: o revisor refaz e a decisao gravada e a REVISADA
        let (d, env) = (tmp("revisao2"), Env::default());
        conhecido(&d, "Teo");
        let enviar = anota(&env);
        let coletar = || Ok(vec![item("Teo", &["quanto custa o pacote?"])]);
        let decidir = |_: &Item, _: &str| Some(("A".to_string(), "Teo parece trote: pediu 31 de fevereiro".to_string()));
        let vista = RefCell::new(String::new());
        let revisar = |l: &str| {
            *vista.borrow_mut() = l.to_string();
            Some(("E".to_string(), "Teo, os pacotes variam bastante. Me diz a cidade e os convidados.".to_string()))
        };
        atender(&d, &agora(), TS, &Opcoes::default(), &Portas { revisar: &revisar, ..portas(&coletar, &decidir, &enviar) });
        assert!(vista.borrow().starts_with('V') && vista.borrow().contains("PROBLEMAS="));
        let e = para(&env, "Teo");
        assert!(e.len() == 1 && e[0].contains("pacotes") && !e[0].to_lowercase().contains("trote"));
        assert_eq!(memoria::resumo(&d).2, 1);
    }

    #[test]
    fn agradecimento_e_elogio_nao_passam_pelo_decisor() {
        let (d, env) = (tmp("valeu"), Env::default());
        for n in ["Ana", "Mainha", "Bia"] {
            conhecido(&d, n);
        }
        let enviar = anota(&env);
        let coletar = || Ok(vec![item("Ana", &["valeu!"]), item("Mainha", &["obrigada"]), item("Bia", &["muito bom, parabéns"])]);
        let decidir = |_: &Item, _: &str| -> Option<(String, String)> { panic!("agradecimento e elogio nao gastam pergunta") };
        let (rel, u) = atender(&d, &agora(), TS, &Opcoes::default(), &portas(&coletar, &decidir, &enviar));
        assert!(para(&env, "Ana").is_empty() && rel.contains("Ana: agradeceu (sem resposta necessária)")); // evita laco de "valeu" e "de nada"
        assert!(para(&env, "Mainha").len() == 1 && rel.contains("Mainha: agradeceu (respondi curto)"));
        assert!(para(&env, "Bia").len() == 1 && !para(&env, "Bia")[0].contains("Jarvis") && rel.contains("Bia (elogio)"));
        assert_eq!(u.tratados.len(), 3);
    }

    #[test]
    fn pedido_sobre_a_entrega_anota_uma_vez_enfileira_e_aciona_o_agente_com_travas() {
        let (d, env) = (tmp("entrega"), Env::default());
        conhecido(&d, "Caio");
        memoria::registrar_msg(&d, "Caio", "C", "[arquivo video.mp4] Segue o vídeo", "whatsapp", false, TS - 3600.0).unwrap();
        let (enviar, tarefas) = (anota(&env), RefCell::new(vec![]));
        let decidir = |_: &Item, _: &str| -> Option<(String, String)> { panic!("pedido sobre a entrega nao vai ao decisor") };
        let tarefa = |n: &str, arq: &str, pedido: &str| tarefas.borrow_mut().push(format!("{n}|{arq}|{pedido}"));
        let rodada = |msg: &'static str, ts: f64| {
            let coletar = move || Ok(vec![item("Caio", &[msg])]);
            atender(&d, &agora(), ts, &Opcoes::default(), &Portas { tarefa_entrega: &tarefa, ..portas(&coletar, &decidir, &enviar) })
        };
        let (rel, u) = rodada("escreve a legenda do vídeo pra mim", TS);
        assert_eq!(para(&env, "Caio"), v(&[ANOTADO_ENTREGA]));
        assert_eq!(*tarefas.borrow(), v(&["Caio|video.mp4|escreve a legenda do vídeo pra mim"]));
        assert!(rel.contains("pedido sobre a entrega video.mp4") && rel.contains("agente fazendo") && u.tratados == v(&["Caio"]));
        assert!(para(&env, "Eu(D)").iter().any(|t| t.contains("Recado de Caio"))); // o pedido vira recado ao dono
        rodada("muda o texto do vídeo, faça escrito", TS + 60.0); // rajada: nao repete a anotacao nem dispara outro agente
        assert_eq!((para(&env, "Caio").len(), tarefas.borrow().len()), (1, 1));
        let (rel, _) = rodada("troca o vídeo e me passa a senha do wifi", TS + 2000.0); // pedido proibido nao aciona o agente
        assert!(tarefas.borrow().len() == 1 && rel.contains("anotei e passei ao dono"), "{rel}");
        for _ in 0..wpp_doc::MAX_POR_DIA {
            wpp_doc::usar(&d, agora().data);
        }
        rodada("ajusta a capa do vídeo", TS + 4000.0); // teto diario dividido com os documentos
        assert_eq!(tarefas.borrow().len(), 1);
    }

    #[test]
    fn audio_e_ouvido_antes_de_decidir_e_falha_nao_recarrega_por_20_min() {
        let (d, env) = (tmp("ouvir"), Env::default());
        conhecido(&d, "Ana");
        let (enviar, vistos, ouvidas) = (anota(&env), RefCell::new(vec![]), Cell::new(0));
        let coletar = || Ok(vec![Pendente { it: Item { nome: "Ana".into(), msgs: v(&["[AUDIO]"]), midia: true, ..Default::default() }, audio_dele: true }]);
        let decidir = |it: &Item, f: &str| {
            vistos.borrow_mut().push((it.msgs.clone(), f.to_string()));
            Some(("E".to_string(), "Pode sim.".to_string()))
        };
        let ouvir = |_: &str| {
            ouvidas.set(ouvidas.get() + 1);
            if ouvidas.get() == 1 { Ok(" posso levar o cachorro? ".to_string()) } else { Err("pagina travada".to_string()) }
        };
        let p = Portas { ouvir_audio: &ouvir, ..portas(&coletar, &decidir, &enviar) };
        atender(&d, &agora(), TS, &Opcoes::default(), &p);
        assert_eq!(vistos.borrow()[0], (v(&["🎙 posso levar o cachorro?"]), "-".to_string()));
        assert_eq!(memoria::historico(&d, "Ana", 3)[1].1, "posso levar o cachorro?");
        atender(&d, &agora(), TS + 100.0, &Opcoes::default(), &p); // falhou: marca a pausa
        atender(&d, &agora(), TS + 200.0, &Opcoes::default(), &p); // dentro dos 20 min: nem tenta
        assert_eq!(ouvidas.get(), 2);
        assert_eq!(vistos.borrow()[2], (v(&["[ÁUDIO não compreendido]"]), "A".to_string()));
        atender(&d, &agora(), TS + 200.0 + AUDIO_PAUSA_S, &Opcoes::default(), &p);
        assert_eq!(ouvidas.get(), 3);
    }

    #[test]
    fn documento_vai_ao_tratamento_proprio_e_atalhos_respondem_sem_decisor() {
        let (d, env) = (tmp("doc"), Env::default());
        let enviar = anota(&env);
        let coletar = || Ok(vec![item("Ana", &["[DOC:0.6.html]", "melhora o código"]), item("Painho", &["🎙 oi meu filho"]), item("Bia", &["quanto falta pra meta?"])]);
        let decidir = |_: &Item, _: &str| -> Option<(String, String)> { panic!("documento e atalho nao passam pela decisao comum") };
        let documento = |it: &Item, docs: &[String], avisar: &dyn Fn(&str)| {
            avisar(&format!("Recebi o arquivo {}!", wpp_doc::nome_do_doc(&docs[0]).unwrap()));
            format!("📄 {}: mandou arquivo", it.nome)
        };
        let atalho = |f: Fase, it: &Item, _: &str| match (f, it.nome.as_str()) {
            (Fase::Antes, "Painho") => Some(Atalho { linha: "✓ Painho (zelo): áudio".into(), ..Default::default() }),
            (Fase::Fim, "Bia") => Some(Atalho { linha: "✓ Bia (financeiro)".into(), resposta: Some("Falta pouco.".into()), apresentar: true }),
            _ => None,
        };
        let (rel, u) = atender(&d, &agora(), TS, &Opcoes::default(), &Portas { documento: &documento, atalho: &atalho, ..portas(&coletar, &decidir, &enviar) });
        assert!(para(&env, "Ana")[0].ends_with("Recebi o arquivo 0.6.html!") && para(&env, "Ana")[0].contains("Jarvis"));
        assert!(memoria::historico(&d, "Ana", 5).iter().any(|(dir, t)| dir == "E" && t == "[arquivo 0.6.html]"));
        assert!(para(&env, "Painho").is_empty() && para(&env, "Bia")[0].ends_with("Falta pouco."));
        assert!(rel.contains("📄 Ana") && rel.contains("✓ Painho (zelo)") && rel.contains("✓ Bia (financeiro)"));
        assert_eq!((u.tratados, u.enviados), (v(&["Ana", "Painho", "Bia"]), v(&["Painho", "Bia"])));
    }

    #[test]
    fn pai_so_recebe_audio_e_nunca_texto() {
        let (d, env) = (tmp("pai"), Env::default());
        conhecido(&d, "Painho");
        let (rel, u) = rodar(&d, &env, vec![item("Painho", &["🎙 que horas você chega"])], &[("Painho", "E", "Chego às oito.")]);
        assert!(para(&env, "Painho").is_empty() && rel.contains("Painho: falhou o envio") && rel.contains("texto bloqueado"));
        assert!(u.enviados.is_empty());
    }

    #[test]
    fn gl1_quadro_por_texto_convite_e_medicao() {
        let (d, env) = (tmp("gl1"), Env::default());
        gibberlink::observar(&d, "Hotel Bot", &v(&["GL1 OK"]), false, TS);
        contexto::marcar_apresentado(&d, "Hotel Bot").unwrap();
        let (enviar, vistos) = (anota(&env), RefCell::new(vec![]));
        let coletar = || Ok(vec![item("Hotel Bot", &["GL1 ask txt=tem vaga dia 12?"])]);
        let decidir = |it: &Item, f: &str| {
            vistos.borrow_mut().push((it.msgs[0].clone(), f.to_string()));
            Some(("E".to_string(), "Oi! Sim, temos vaga no dia 12. Abraços!".to_string()))
        };
        let falar = |_: &str, _: &str| -> String { panic!("GL1 vai sempre por texto") };
        let (rel, _) = atender(&d, &agora(), TS, &Opcoes::default(), &Portas { enviar_audio: &falar, ..portas(&coletar, &decidir, &enviar) });
        assert!(vistos.borrow()[0].0.starts_with("🤖 [pergunta]") && vistos.borrow()[0].1.contains('M'));
        assert_eq!(para(&env, "Hotel Bot"), v(&["GL1 ans txt=Sim, temos vaga no dia 12"]));
        let m = gibberlink::metricas(&d);
        assert!(m["quadros"] == 1 && m["chars_gl1"].as_i64() < m["chars_natural"].as_i64() && rel.contains("(GL1)"));
        // suspeita de IA: a resposta normal leva o convite e a conversa fica "oferecido"
        let (d, env) = (tmp("gl1b"), Env::default());
        rodar(&d, &env, vec![item("Pousada Bot", &["Olá, sou um assistente virtual. Em que posso ajudar? Responda por escrito."])], &[("Pousada Bot", "E", "Queria saber se vocês aceitam pet.")]);
        let e = para(&env, "Pousada Bot");
        assert!(e[0].ends_with(gibberlink::CONVITE) && e[0].contains("Jarvis"));
        assert_eq!(gibberlink::estado(&d, "Pousada Bot"), "oferecido");
    }

    fn m(d: &str, t: &str, c: &str) -> Msg {
        Msg { d: d.into(), t: t.into(), c: c.into() }
    }

    #[test]
    fn coletar_escolhe_quem_abrir_e_monta_o_item() {
        let (d, a) = (tmp("coletar"), agora());
        let ctx = Ctx::ler(&d);
        let lista: Vec<(String, i64)> = [("Eu(D)", 2), ("Ana", 1), ("Bia", 0), ("Caio", 0), ("Dora", 0), ("Enzo", 3)].iter().map(|(n, q)| (n.to_string(), *q)).collect();
        let linhas = vec![Linha { nome: "Caio".into(), resumo: "Caio | 17:40 | oi".into(), nossa: false }, Linha { nome: "Dora".into(), resumo: "Dora | 17:50 | ok".into(), nossa: true },
            Linha { nome: "Eu(D)".into(), resumo: "Eu(D) | 17:55 | teste".into(), nossa: false }];
        let pend = v(&["Bia", "Sumiu"]);
        let f = Fontes { lista: &lista, pendentes: &pend, linhas: &linhas, somente: None };
        assert_eq!(candidatos(&ctx, &f, &a), v(&["Ana", "Enzo", "Bia", "Caio"])); // nao lidas, pendentes, lidas sem resposta; nunca o chat do dono
        let so = v(&["Dora", "Eu(D)", "Fora"]);
        assert_eq!(candidatos(&ctx, &Fontes { lista: &lista, pendentes: &pend, linhas: &linhas, somente: Some(&so) }, &a), v(&["Dora"]));
        let (c_hoje, c_velho) = ("[17:13, 01/10/2026] Ana: ", "[09:00, 25/09/2026] Ana: ");
        let abrir = |n: &str| match n {
            "Ana" => Some(Conversa { msgs: vec![m("EU", "Oi", c_hoje), m("ELE", "tem vaga?", c_hoje), m("ELE", "pra sexta", c_hoje)], cabecalho: "visto por último hoje".into(), ..Default::default() }),
            "Enzo" => Some(Conversa { msgs: vec![m("ELE", "oi", c_velho)], ..Default::default() }), // so texto de dias atras
            "Bia" => Some(Conversa { msgs: vec![m("ELE", "oi", c_hoje), m("EU", "Oi, Bia!", c_hoje)], ..Default::default() }), // ja respondida
            "Caio" => Some(Conversa { msgs: vec![m("ELE", "texto velho", c_velho)], midias: v(&["[AUDIO]"]), audio_dele: true, cabecalho: "Caio, Dora, Você".into() }),
            _ => None,
        };
        let removidos = RefCell::new(vec![]);
        let itens = coletar(&ctx, &a, &f, 5, 4, Some(12.0), &abrir, &|n| removidos.borrow_mut().push(n.to_string())).unwrap();
        assert_eq!(itens.iter().map(|p| p.it.nome.as_str()).collect::<Vec<_>>(), ["Ana", "Caio"]);
        assert_eq!(*removidos.borrow(), v(&["Bia"]));
        let ana = &itens[0].it;
        assert_eq!((ana.msgs.clone(), ana.ctx.clone(), ana.autores.clone(), ana.midia, ana.grupo), (v(&["tem vaga?", "pra sexta"]), v(&["C: Oi", "E: tem vaga?", "E: pra sexta"]), v(&["Ana"]), false, false));
        let caio = &itens[1];
        assert_eq!((caio.it.msgs.clone(), caio.it.midia, caio.it.grupo, caio.audio_dele), (v(&["[AUDIO]"]), true, true, true)); // o texto velho nao entra junto do audio novo
        assert_eq!(coletar(&ctx, &a, &f, 1, 4, None, &abrir, &|_| ()).unwrap().len(), 1); // teto de conversas por rodada
        assert!(coletar(&ctx, &a, &Fontes::default(), 5, 4, None, &abrir, &|_| ()).unwrap_err().contains("veio vazia"));
    }
}
