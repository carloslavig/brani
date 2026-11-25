//! ATENDIMENTO AUTOMATICO do WhatsApp: quando olhar a lista de conversas e quem atender sozinho. TRAVAS (todas ativas):
//! - liga/desliga pelo interruptor local (padrao: DESLIGADO); para sozinho com o Braniac pausado;
//! - so entra conversa cuja ULTIMA mensagem e DELA, de hoje e dentro da janela de idade; se a ultima e nossa (alguem ja respondeu),
//!   nao ha o que responder; chat do dono, grupos e contas automaticas ficam de fora; cada situacao e tratada uma vez;
//! - espera a pessoa terminar de escrever (a conversa precisa ficar estavel alguns segundos);
//! - tetos por pessoa/hora, total/hora e total/dia; janela de horario (fora dela so o prioritario); anti-laco (8 por hora na mesma
//!   conversa); desiste depois de 2 tentativas; quem espera mais que o teto de espera fura os limites UMA vez por mensagem.
//! Tambem aqui: a entrega do outbox (recados que outras rotinas gravam para o dono) e o controle de queda do navegador.
//! Ficou de fora: ler a lista do WhatsApp (entra `linhas`), rodar o atendimento (closure `rodar`, ver atender_laco), os comandos
//! do dono dentro da conversa de terceiros (monitor_dono: apresentar, silenciar, repassar por voz), reabrir o navegador e a thread.
//! O arquivo do outbox leva o nome do dono (dados/outbox_<nome>.txt), que vem da configuracao.
#![allow(dead_code)]
use crate::atender::{self, Linha};
use crate::atender_laco::Ultimo;
use crate::contexto::{self, re, Ctx};
use crate::tempo::Agora;
use crate::{funil, power, wpp_doc};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct Cfg {
    /// De quanto em quanto tempo olha a lista.
    pub intervalo_s: f64,
    /// A conversa precisa ficar sem mudar por isso antes de responder.
    pub estavel_s: f64,
    /// Ninguem espera mais que isso: passou, o atendimento fura os limites por hora (1x por mensagem).
    pub teto_espera_s: f64,
    /// ((hora, minuto) de inicio, (hora, minuto) de fim).
    pub janela: ((u32, u32), (u32, u32)),
    pub max_por_chat_hora: usize,
    pub max_por_hora: usize,
    pub max_por_dia: usize,
    pub max_idade_h: f64,
}

impl Default for Cfg {
    fn default() -> Self {
        Cfg { intervalo_s: 10.0, estavel_s: 8.0, teto_espera_s: 300.0, janela: ((7, 0), (23, 59)), max_por_chat_hora: 3, max_por_hora: 15, max_por_dia: 60, max_idade_h: 12.0 }
    }
}

fn hm(v: &Value) -> Option<(u32, u32)> {
    let (h, m) = v.as_str()?.split_once(':')?;
    Some((h.trim().parse().ok()?, m.trim().parse().ok()?))
}

/// Padrao + o que vier em `atendimento_auto` da configuracao do roteador. Valor invalido fica no padrao.
pub fn cfg_de(v: &Value) -> Cfg {
    let p = Cfg::default();
    let f = |k: &str, d: f64| v[k].as_f64().filter(|x| *x >= 0.0).unwrap_or(d);
    let n = |k: &str, d: usize| v[k].as_u64().map_or(d, |x| x as usize);
    Cfg {
        intervalo_s: f("intervalo_s", p.intervalo_s), estavel_s: f("estavel_s", p.estavel_s), teto_espera_s: f("teto_espera_s", p.teto_espera_s),
        janela: hm(&v["janela"][0]).zip(hm(&v["janela"][1])).unwrap_or(p.janela),
        max_por_chat_hora: n("max_por_chat_hora", p.max_por_chat_hora), max_por_hora: n("max_por_hora", p.max_por_hora), max_por_dia: n("max_por_dia", p.max_por_dia),
        max_idade_h: f("max_idade_h", p.max_idade_h),
    }
}

fn log(dados: &Path, agora: &Agora, txt: &str) {
    use std::io::Write;
    let _ = std::fs::create_dir_all(dados);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dados.join("vigia.log")) {
        let _ = writeln!(f, "{} {}:{:02} | {}", agora.data.iso(), agora.hm(), agora.seg, txt.replace(['\r', '\n'], " "));
    }
}

// ---------- estado (dados/vigia_estado.json, mesmo formato do Python) ----------

#[derive(Default, Debug)]
pub struct Est {
    /// Respostas enviadas: (nome, quando).
    pub envios: Vec<(String, f64)>,
    /// nome -> assinatura (linha da lista) ja tratada.
    pub tratados: BTreeMap<String, String>,
    /// nome -> (assinatura, desde quando esta igual).
    pub estavel: BTreeMap<String, (String, f64)>,
    /// "nome|assinatura" -> tentativas sem conseguir tratar.
    pub tentativas: BTreeMap<String, i64>,
    pub grupos: Vec<String>,
    pub urgente_feito: BTreeMap<String, String>,
    pub limite_logado: BTreeMap<String, String>,
    pub laco_logado: BTreeMap<String, String>,
    /// Tentativas de atendimento: (nome, quando).
    pub atendidos: Vec<(String, f64)>,
    /// O arquivo como foi lido (chaves que este modulo nao conhece sao preservadas).
    bruto: Value,
}

fn textos(v: &Value) -> BTreeMap<String, String> {
    v.as_object().into_iter().flatten().filter_map(|(k, x)| Some((k.clone(), x.as_str()?.to_string()))).collect()
}

fn pares(v: &Value) -> Vec<(String, f64)> {
    v.as_array().into_iter().flatten().filter_map(|x| Some((x[0].as_str()?.to_string(), x[1].as_f64()?))).collect()
}

pub fn estado(dados: &Path) -> Est {
    let b = std::fs::read_to_string(dados.join("vigia_estado.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}));
    Est {
        envios: pares(&b["envios"]), tratados: textos(&b["tratados"]),
        estavel: b["estavel"].as_object().into_iter().flatten().filter_map(|(k, x)| Some((k.clone(), (x["sig"].as_str()?.to_string(), x["desde"].as_f64()?)))).collect(),
        tentativas: b["tentativas"].as_object().into_iter().flatten().filter_map(|(k, x)| Some((k.clone(), x.as_i64()?))).collect(),
        grupos: b["grupos"].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(str::to_string)).collect(),
        urgente_feito: textos(&b["urgente_feito"]), limite_logado: textos(&b["limite_logado"]), laco_logado: textos(&b["laco_logado"]),
        atendidos: pares(&b["atendidos"]), bruto: b,
    }
}

fn salvar(dados: &Path, e: &Est) {
    let mut b = if e.bruto.is_object() { e.bruto.clone() } else { json!({}) };
    let lista = |v: &[(String, f64)]| json!(v.iter().map(|(n, t)| json!([n, t])).collect::<Vec<_>>());
    b["envios"] = lista(&e.envios);
    b["atendidos"] = lista(&e.atendidos);
    b["tratados"] = json!(e.tratados);
    b["estavel"] = e.estavel.iter().map(|(k, (s, d))| (k.clone(), json!({"sig": s, "desde": d}))).collect::<serde_json::Map<_, _>>().into();
    b["tentativas"] = json!(e.tentativas);
    b["grupos"] = json!(e.grupos);
    b["urgente_feito"] = json!(e.urgente_feito);
    b["limite_logado"] = json!(e.limite_logado);
    b["laco_logado"] = json!(e.laco_logado);
    let _ = std::fs::create_dir_all(dados);
    let _ = std::fs::write(dados.join("vigia_estado.json"), b.to_string());
}

// ---------- liga / desliga ----------

/// Liga o atendimento automatico. SO a interface local chama isto. Nao apaga o que ja foi tratado (religar nao reenvia nada).
pub fn ligar(dados: &Path, agora: &Agora) {
    power::set_atendimento_auto(dados, true);
    let mut e = estado(dados);
    e.estavel.clear();
    salvar(dados, &e);
    log(dados, agora, "LIGADO");
}

pub fn desligar(dados: &Path, agora: &Agora) {
    power::set_atendimento_auto(dados, false);
    log(dados, agora, "DESLIGADO");
}

/// (ligado, respostas na ultima hora, conversas tratadas).
pub fn resumo(dados: &Path, ts: f64) -> (bool, usize, usize) {
    let e = estado(dados);
    (power::atendimento_auto(dados), e.envios.iter().filter(|(_, t)| ts - t < 3600.0).count(), e.tratados.len())
}

// ---------- regras ----------

pub fn dentro_da_janela(cfg: &Cfg, agora: &Agora) -> bool {
    let (ini, fim) = cfg.janela;
    ini <= (agora.hora, agora.min) && (agora.hora, agora.min) < fim
}

/// Este ciclo deve rodar? None = nao (atendimento automatico desligado ou Braniac pausado). Some(true) = fora da janela de
/// horario: so o prioritario e atendido. Quem chama ainda confere se ha um atendimento manual em andamento.
pub fn modo(dados: &Path, cfg: &Cfg, agora: &Agora) -> Option<bool> {
    (power::atendimento_auto(dados) && !power::pausado(dados)).then(|| !dentro_da_janela(cfg, agora))
}

/// Respeita os limites por pessoa/hora, total/hora e total/dia. `ts` = agora; `agora` = hora local (para saber onde o dia comecou).
pub fn limite_ok(est: &mut Est, nome: &str, ts: f64, agora: &Agora, cfg: &Cfg) -> bool {
    est.envios.retain(|(_, t)| ts - t < 86400.0);
    let inicio_do_dia = ts - agora.segundos() as f64;
    let hora: Vec<&(String, f64)> = est.envios.iter().filter(|(_, t)| ts - t < 3600.0).collect();
    hora.iter().filter(|(n, _)| n == nome).count() < cfg.max_por_chat_hora && hora.len() < cfg.max_por_hora
        && est.envios.iter().filter(|(_, t)| *t >= inicio_do_dia).count() < cfg.max_por_dia
}

// "gravando audio..."/"digitando..." nao e mensagem nova
re!(digitando, r"(?i)\| (gravando [aá]udio|digitando|recording audio|typing)\.*$");

#[derive(Default, Debug, PartialEq)]
pub struct Feito {
    pub candidatos: Vec<String>,
    pub enviados: Vec<String>,
}

/// Um ciclo do vigia. `linhas` = a lista de conversas agora; `rodar(nomes)` faz o atendimento dessas conversas e devolve o resultado.
/// Criterio: a ULTIMA mensagem da conversa e DELA e e de hoje, dentro da janela de idade. Ler a conversa nao a tira da fila: o que
/// conta e ninguem ter respondido.
pub fn ciclo(dados: &Path, cfg: &Cfg, linhas: &[Linha], agora: &Agora, ts: f64, so_prioritarios: bool, rodar: &dyn Fn(&[String]) -> Ultimo) -> Feito {
    let ctx = Ctx::ler(dados);
    let mut est = estado(dados);
    let dono = ctx.chat_do_dono().unwrap_or("");
    let mut candidatos: Vec<(String, String)> = vec![];
    for c in linhas {
        let (nome, sig) = (c.nome.as_str(), c.resumo.as_str()); // sem o contador de nao lidas: ler nao muda o que esta sem resposta
        if nome == dono || ctx.nao_responder(nome) || est.grupos.iter().any(|g| g == nome) {
            continue;
        }
        let prio = ctx.prioritario(nome);
        if (so_prioritarios && !prio) || digitando().is_match(sig) {
            continue;
        }
        if c.nossa && !contexto::saudacao_automatica(sig) { // a ultima e nossa: nada a responder (a saudacao automatica NAO conta)
            est.estavel.remove(nome); // nao vira "tratada": se a leitura do icone mudar, a conversa continua elegivel
            est.tratados.remove(nome);
            continue;
        }
        if est.tratados.get(nome).is_some_and(|s| s == sig) {
            continue;
        }
        if atender::idade_linha(sig, agora).is_none_or(|h| h > cfg.max_idade_h) { // "Ontem", dia da semana, data ou muito antiga
            est.tratados.insert(nome.into(), sig.into());
            continue;
        }
        let desde = match est.estavel.get(nome) {
            Some((s, d)) if s == sig => *d,
            _ => { // mudou de novo: reinicia a espera (a pessoa pode estar digitando)
                est.estavel.insert(nome.into(), (sig.into(), ts));
                continue;
            }
        };
        if ts - desde < cfg.estavel_s {
            continue;
        }
        // ninguem espera mais que o teto: passou -> fura limites e desistencia UMA vez por mensagem
        let urgente = ts - desde >= cfg.teto_espera_s && est.urgente_feito.get(nome).is_none_or(|s| s != sig);
        let chave = format!("{nome}|{sig}");
        let livre = prio || urgente;
        if !livre && est.tentativas.get(&chave).copied().unwrap_or(0) >= 2 { // tentou 2x sem conseguir tratar: desiste desta (nao entra em laco)
            est.tratados.insert(nome.into(), sig.into());
            log(dados, agora, &format!("{nome} | desistiu apos 2 tentativas"));
            continue;
        }
        if !livre && !limite_ok(&mut est, nome, ts, agora, cfg) { // limite atingido: NAO marca como tratado; tenta de novo quando liberar
            if est.limite_logado.get(nome).is_none_or(|s| s != sig) { // loga so uma vez por conversa
                est.limite_logado.insert(nome.into(), sig.into());
                log(dados, agora, &format!("{nome} | limite atingido: aguardando"));
            }
            continue;
        }
        est.atendidos.retain(|(_, t)| ts - t < 3600.0);
        if !livre && est.atendidos.iter().filter(|(n, _)| n == nome).count() >= 8 { // anti-laco (o do prioritario e cortado pelo encerramento)
            if est.laco_logado.get(nome).is_none_or(|s| s != sig) { // nunca em silencio: fica no log
                est.laco_logado.insert(nome.into(), sig.into());
                log(dados, agora, &format!("{nome} | anti-laco: 8 tentativas na ultima hora, aguardando"));
            }
            continue;
        }
        if urgente {
            est.urgente_feito.insert(nome.into(), sig.into());
            log(dados, agora, &format!("{nome} | esperou mais de {} min: atendimento forcado", (cfg.teto_espera_s / 60.0) as i64));
        }
        candidatos.push((nome.into(), sig.into()));
    }
    let mut feito = Feito { candidatos: candidatos.iter().map(|(n, _)| n.clone()).collect(), enviados: vec![] };
    if !candidatos.is_empty() {
        est.atendidos.extend(feito.candidatos.iter().map(|n| (n.clone(), ts)));
        log(dados, agora, &format!("atendendo: {:?}", feito.candidatos));
        for (n, sig) in &candidatos {
            *est.tentativas.entry(format!("{n}|{sig}")).or_insert(0) += 1;
        }
        salvar(dados, &est); // antes de atender: se cair no meio, a tentativa ja esta contada
        let u = rodar(&feito.candidatos);
        for g in u.grupos { // grupo nunca recebe resposta sozinho: a vigia nem tenta mais
            if !est.grupos.contains(&g) {
                est.grupos.push(g);
            }
        }
        est.envios.extend(u.enviados.iter().map(|n| (n.clone(), ts)));
        for (n, sig) in candidatos {
            if u.tratados.contains(&n) || u.enviados.contains(&n) { // decidido (enviou, ignorou, avisou ou perguntou ao dono): nao repete
                est.estavel.remove(&n);
                est.tentativas.remove(&format!("{n}|{sig}"));
                est.tratados.insert(n, sig);
            }
        }
        feito.enviados = u.enviados;
    }
    salvar(dados, &est);
    feito
}

// ---------- outbox: recados que outras rotinas gravam para o dono ----------

pub fn outbox(dados: &Path, ctx: &Ctx) -> PathBuf {
    dados.join(format!("outbox_{}.txt", if ctx.dono.trim().is_empty() { "dono".to_string() } else { wpp_doc::slug(&ctx.dono) }))
}

/// Como cada tipo de bloco sai. Todo resultado comeca com "✅" se enviou.
pub struct Saidas<'a> {
    /// (destino, texto).
    pub texto: &'a dyn Fn(&str, &str) -> String,
    /// (destino, fala) na voz do Jarvis.
    pub audio: &'a dyn Fn(&str, &str) -> String,
    /// (destino, caminho, legenda): manda como documento.
    pub arquivo: &'a dyn Fn(&str, &str, &str) -> String,
    /// (destino, caminho do .wav): manda ESSE wav como mensagem de voz.
    pub voz: &'a dyn Fn(&str, &Path) -> String,
    /// (agente, destino, texto): audio na voz de um agente (ex.: "jorge").
    pub voz_agente: &'a dyn Fn(&str, &str, &str) -> String,
    /// A Alma compoe a propria mensagem ("@novidades" ou "" = livre); None = nao acertou, nada a enviar.
    pub alma: &'a dyn Fn(&str) -> Option<String>,
    /// Depois do texto da Alma, o audio na voz dela.
    pub alma_audio: &'a dyn Fn(&str),
}

re!(separador, r"(?m)^\s*---\s*$");

fn corta(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn blocos(texto: &str) -> Vec<String> {
    separador().split(texto).map(|b| b.trim().to_string()).filter(|b| !b.is_empty()).collect()
}

fn ler_outbox(arq: &Path) -> Option<String> {
    Some(std::fs::read_to_string(arq).ok()?.trim_start_matches('\u{feff}').trim().to_string())
}

/// Marca no comeco do bloco, sem diferenciar maiusculas ("[audio]", "[Audio]").
fn marca<'b>(b: &'b str, m: &str) -> Option<&'b str> {
    b.get(..m.len()).filter(|i| i.eq_ignore_ascii_case(m)).map(|_| b[m.len()..].trim())
}

/// Entrega os blocos do outbox ao dono (separados por uma linha "---"). Devolve quantos saíram da fila.
/// [resumo] vai para o funil (nao sai na hora); [arquivo] caminho + legenda; [audio] em audio; [voz] um .wav; [alma] texto + audio
/// dela; [jorge] audio do agente para a mae do dono (texto se o audio falhar); o resto vai em texto com o prefixo do agente.
/// Falhou o envio: para ali e tenta de novo no proximo ciclo. O que outro processo gravou durante a entrega nao se perde.
pub fn enviar_outbox(dados: &Path, agora: &Agora, ts: f64, s: &Saidas) -> usize {
    let ctx = Ctx::ler(dados);
    let arq = outbox(dados, &ctx);
    let (Some(bruto), Some(dono)) = (ler_outbox(&arq).filter(|t| !t.is_empty()), ctx.chat_do_dono()) else { return 0 };
    let lista = blocos(&bruto);
    let lg = |t: &str| log(dados, agora, t);
    let mut feitos = 0;
    for b in &lista {
        let r = if let Some(t) = marca(b, "[resumo]") { // aviso automatico: vai para o resumo do funil
            funil::passar(dados, t, "outbox", agora, ts);
            "✅ no funil".to_string()
        } else if let Some(t) = marca(b, "[arquivo]") { // caminho na 1a linha, legenda no resto
            let mut l = t.lines();
            let caminho = l.next().unwrap_or("").trim().to_string();
            (s.arquivo)(dono, &caminho, &corta(l.collect::<Vec<_>>().join("\n").trim(), 900))
        } else if let Some(t) = marca(b, "[alma]") {
            match if t.is_empty() || t == "@novidades" { (s.alma)(t) } else { Some(t.to_string()) }.filter(|x| !x.trim().is_empty()) {
                None => {
                    lg("outbox: [alma]: o modelo local nao acertou, nada enviado");
                    "✅ nada a enviar".to_string()
                }
                Some(texto) => {
                    let r = (s.texto)(dono, &corta(&texto, 1500));
                    if r.starts_with('✅') {
                        (s.alma_audio)(&texto);
                    }
                    r
                }
            }
        } else if let Some(t) = marca(b, "[jorge]") { // audio na voz do agente; nunca vai a hospede
            let t = corta(t, 900);
            match ctx.contato("mae") {
                None => {
                    lg("outbox: [jorge] ignorado, contato 'mae' nao configurado");
                    "✅ sem destino".to_string()
                }
                Some(mae) => {
                    let r = (s.voz_agente)("jorge", mae, &t);
                    if r.starts_with('✅') || r.starts_with("Cliquei em enviar") { r } else {
                        lg(&format!("outbox: [jorge] audio nao saiu ({}), mandando texto", corta(&r, 60)));
                        (s.texto)(mae, &t)
                    }
                }
            }
        } else if let Some(t) = marca(b, "[voz]") {
            let caminho = PathBuf::from(t.lines().next().unwrap_or("").trim().trim_matches('"'));
            if wpp_doc::extensao(&caminho.to_string_lossy()) != ".wav" || !caminho.is_file() {
                lg(&format!("outbox: [voz] ignorado, wav nao encontrado ({})", caminho.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
                "✅ ignorado".to_string()
            } else {
                (s.voz)(dono, &caminho)
            }
        } else if let Some(t) = marca(b, "[audio]") {
            (s.audio)(dono, &corta(t, 1200))
        } else {
            (s.texto)(dono, &corta(&format!("Claudinho (via Braniac): {b}"), 1500))
        };
        if r.starts_with("Cliquei em enviar") { // o clique saiu, so a confirmacao na tela falhou; reenviar mandava o MESMO audio varias vezes
            lg(&format!("outbox: enviado sem confirmação na tela ({}); não reenvia", corta(&r, 60)));
        } else if !r.starts_with('✅') {
            lg(&format!("outbox: falhou ({}); tenta de novo no proximo ciclo", corta(&r, 80)));
            break;
        }
        feitos += 1;
    }
    let mut restante: Vec<String> = lista[feitos..].to_vec();
    if let Some(agora_txt) = ler_outbox(&arq).filter(|t| *t != bruto) { // o que outro processo gravou enquanto entregavamos nao pode sumir
        let novo = agora_txt.strip_prefix(bruto.as_str()).unwrap_or(&agora_txt);
        restante.extend(blocos(novo).into_iter().filter(|b| !lista.contains(b)));
    }
    if restante.is_empty() {
        let _ = std::fs::remove_file(&arq);
    } else {
        let _ = std::fs::write(&arq, restante.join("\n---\n"));
    }
    if feitos > 0 {
        lg(&format!("outbox: {feitos} recado(s) entregue(s) ao dono"));
    }
    feitos
}

// ---------- queda do navegador ----------

/// Navegador do agente fora do ar: loga o 1o erro e depois 1 vez por hora; com a porta recusada, manda reabrir no maximo 1 vez por hora.
#[derive(Default, Debug)]
pub struct Queda {
    desde: Option<f64>,
    ult_log: f64,
    ult_reabrir: f64,
}

impl Queda {
    /// Devolve (logar agora?, reabrir o navegador agora?).
    pub fn registrar(&mut self, conexao_recusada: bool, ts: f64) -> (bool, bool) {
        self.desde.get_or_insert(ts);
        let reabrir = conexao_recusada && ts - self.ult_reabrir >= 3600.0;
        if reabrir {
            self.ult_reabrir = ts;
        }
        let logar = ts - self.ult_log >= 3600.0;
        if logar {
            self.ult_log = ts;
        }
        (logar, reabrir)
    }

    /// True se estava caido (para logar que voltou).
    pub fn voltou(&mut self) -> bool {
        std::mem::take(self).desde.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;
    use std::cell::RefCell;

    const T0: f64 = 1.8e9;

    fn agora() -> Agora {
        Agora { data: Data::civil(2026, 10, 1).unwrap(), hora: 18, min: 0, seg: 0 }
    }

    fn tmp(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-vigia-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("pessoal.json"), r#"{"nome": "Davi", "contatos": {"davi": "Eu(D)", "mae": "Mainha"},
            "nao_responder": ["cobranca", "suporte"], "prioritarios": ["loja"]}"#).unwrap();
        d
    }

    fn l(nome: &str, hora: &str, texto: &str) -> Linha {
        Linha { nome: nome.into(), resumo: format!("{nome} | {hora} | {texto}"), nossa: false }
    }

    fn nossa(nome: &str, hora: &str, texto: &str) -> Linha {
        Linha { nossa: true, ..l(nome, hora, texto) }
    }

    /// Roda ciclos com o "WhatsApp" simulado; `resolve` diz se o atendimento conseguiu tratar quem recebeu.
    struct Sim {
        d: PathBuf,
        t: f64,
        chamadas: RefCell<Vec<Vec<String>>>,
        resolve: bool,
        cfg: Cfg,
    }

    impl Sim {
        fn novo(nome: &str) -> Sim {
            Sim { d: tmp(nome), t: T0, chamadas: RefCell::default(), resolve: true, cfg: Cfg { estavel_s: 15.0, ..Cfg::default() } }
        }

        fn ciclo_em(&mut self, linhas: &[Linha], avanca: f64, so_prio: bool) -> Feito {
            self.t += avanca;
            let rodar = |nomes: &[String]| {
                self.chamadas.borrow_mut().push(nomes.to_vec());
                let feitos = if self.resolve { nomes.to_vec() } else { vec![] };
                Ultimo { enviados: feitos.clone(), tratados: feitos, ..Default::default() }
            };
            ciclo(&self.d, &self.cfg, linhas, &agora(), self.t, so_prio, &rodar)
        }

        fn ciclo(&mut self, linhas: &[Linha], avanca: f64) -> Feito {
            self.ciclo_em(linhas, avanca, false)
        }

        /// Ve a conversa e, 20 s depois (estavel ha mais de 15 s), atende.
        fn espera_e_atende(&mut self, linhas: &[Linha]) -> Feito {
            self.ciclo(linhas, 0.0);
            self.ciclo(linhas, 20.0)
        }

        fn n(&self) -> usize {
            self.chamadas.borrow().len()
        }
    }

    fn v(m: &[&str]) -> Vec<String> {
        m.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn mensagem_lida_e_sem_resposta_e_respondida_e_a_nossa_nao_entra() {
        let mut s = Sim::novo("lida");
        let r = s.espera_e_atende(&[l("Irmao", "17:47", "me explica uma coisa"), l("Bia", "17:55", "oi")]);
        assert_eq!((r.candidatos.clone(), r.enviados), (v(&["Irmao", "Bia"]), v(&["Irmao", "Bia"])));
        assert_eq!(*s.chamadas.borrow(), [v(&["Irmao", "Bia"])]);
        let mut s = Sim::novo("nossa");
        s.espera_e_atende(&[nossa("Ana", "17:50", "Tem sim!")]); // alguem ja respondeu: nada a fazer
        assert_eq!(s.n(), 0);
    }

    #[test]
    fn conversas_antigas_nao_sao_respondidas_de_repente() {
        let mut s = Sim::novo("antigas");
        let r = s.espera_e_atende(&[l("Gran", "Ontem", "oi"), l("Zeca", "terça-feira", "oi"), l("Rui", "03:00", "oi"), l("Eva", "17:00", "oi")]);
        assert_eq!(r.candidatos, v(&["Eva"])); // so a de hoje, dentro de 12 h
    }

    #[test]
    fn chat_do_dono_contas_automaticas_e_grupos_sao_ignorados() {
        let mut s = Sim::novo("ignora");
        std::fs::write(s.d.join("vigia_estado.json"), r#"{"grupos": ["Turma"], "apres_dono": {"X": "y"}}"#).unwrap();
        let linhas: Vec<Linha> = ["Eu(D)", "Cobranca Zeta", "Suporte Cadastro", "Turma"].iter().map(|n| l(n, "17:50", "mensagem nova")).collect();
        s.espera_e_atende(&linhas);
        assert_eq!(s.n(), 0);
        let bruto: Value = serde_json::from_str(&std::fs::read_to_string(s.d.join("vigia_estado.json")).unwrap()).unwrap();
        assert_eq!(bruto["apres_dono"]["X"], "y"); // chave de outro modulo e preservada
    }

    #[test]
    fn grupo_descoberto_no_atendimento_nao_volta() {
        let (d, chamadas) = (tmp("grupo"), RefCell::new(0));
        let rodar = |nomes: &[String]| {
            *chamadas.borrow_mut() += 1;
            Ultimo { grupos: nomes.to_vec(), tratados: nomes.to_vec(), ..Default::default() }
        };
        let cfg = Cfg::default();
        ciclo(&d, &cfg, &[l("Turma", "17:50", "oi")], &agora(), T0, false, &rodar);
        ciclo(&d, &cfg, &[l("Turma", "17:50", "oi")], &agora(), T0 + 20.0, false, &rodar);
        for i in 0..3 {
            let linha = l("Turma", "17:55", &format!("mensagem {i}"));
            ciclo(&d, &cfg, std::slice::from_ref(&linha), &agora(), T0 + 100.0 + 30.0 * i as f64, false, &rodar);
        }
        assert_eq!((*chamadas.borrow(), estado(&d).grupos), (1, v(&["Turma"])));
    }

    #[test]
    fn depois_de_tratada_nao_repete_e_mensagem_nova_e_tratada_de_novo() {
        let mut s = Sim::novo("repete");
        let linhas = [l("Ana", "17:50", "tem vaga?")];
        s.espera_e_atende(&linhas);
        for _ in 0..3 {
            s.ciclo(&linhas, 30.0);
        }
        assert_eq!(s.n(), 1);
        s.ciclo(&[nossa("Ana", "17:51", "Tem sim")], 20.0); // a lista mostra a nossa resposta
        let r = s.espera_e_atende(&[l("Ana", "17:58", "e dia 5?")]);
        assert_eq!((r.candidatos, s.n()), (v(&["Ana"]), 2));
    }

    #[test]
    fn se_o_atendimento_nao_conseguiu_tenta_de_novo_ate_2_vezes_e_desiste() {
        let mut s = Sim::novo("desiste");
        s.resolve = false;
        let linhas = [l("Ana", "17:50", "tem vaga?")];
        s.ciclo(&linhas, 0.0);
        for _ in 0..5 {
            s.ciclo(&linhas, 20.0);
        }
        assert_eq!(s.n(), 2); // 2 tentativas e para (sem laco)
        assert!(std::fs::read_to_string(s.d.join("vigia.log")).unwrap().contains("Ana | desistiu apos 2 tentativas"));
    }

    #[test]
    fn a_pessoa_continua_digitando_a_espera_recomeca() {
        let mut s = Sim::novo("digitando");
        s.ciclo(&[l("Ana", "17:50", "oi")], 0.0);
        s.ciclo(&[l("Ana", "17:51", "tem vaga?")], 10.0); // chegou outra: recomeca
        assert!(s.ciclo(&[l("Ana", "17:51", "tem vaga?")], 10.0).candidatos.is_empty());
        assert_eq!(s.ciclo(&[l("Ana", "17:51", "tem vaga?")], 10.0).candidatos, v(&["Ana"]));
        // "gravando audio..."/"digitando..." nao e mensagem nova
        let mut s = Sim::novo("gravando");
        s.espera_e_atende(&[l("Bia", "17:50", "gravando áudio...")]);
        s.espera_e_atende(&[l("Bia", "17:50", "digitando...")]);
        assert_eq!(s.n(), 0);
    }

    #[test]
    fn limites_por_pessoa_total_e_por_dia() {
        let mut s = Sim::novo("limite");
        s.cfg.max_por_chat_hora = 1;
        s.espera_e_atende(&[l("Ana", "17:50", "oi")]); // 1a: atende
        s.ciclo(&[nossa("Ana", "17:51", "ok")], 5.0);
        let r = s.espera_e_atende(&[l("Ana", "17:56", "e sobre o preco?")]); // 2a na mesma hora: limite
        assert!(r.candidatos.is_empty() && s.n() == 1);
        assert!(!estado(&s.d).tratados.contains_key("Ana")); // NAO marca como tratada: tenta de novo quando liberar
        // quem espera mais de 5 min fura o limite UMA vez
        assert_eq!(s.ciclo(&[l("Ana", "17:56", "e sobre o preco?")], 300.0).candidatos, v(&["Ana"]));
        assert_eq!(s.n(), 2);
        assert_eq!(std::fs::read_to_string(s.d.join("vigia.log")).unwrap().matches("limite atingido").count(), 1); // loga so uma vez
        let cfg = Cfg::default();
        let mut cheio = Est { envios: (0..15).map(|i| (format!("p{i}"), T0 - i as f64)).collect(), ..Default::default() };
        assert!(!limite_ok(&mut cheio, "novo", T0, &agora(), &cfg) && limite_ok(&mut Est::default(), "novo", T0, &agora(), &cfg));
        let mut dia = Est { envios: (0..60).map(|i| (format!("p{i}"), T0 - 4000.0 - i as f64)).collect(), ..Default::default() };
        assert!(!limite_ok(&mut dia, "novo", T0, &agora(), &cfg)); // 60 hoje, todos ha mais de 1 h
        let mut ontem = Est { envios: (0..60).map(|i| (format!("p{i}"), T0 - 70000.0 - i as f64)).collect(), ..Default::default() };
        assert!(limite_ok(&mut ontem, "novo", T0, &agora(), &cfg)); // os de ontem nao contam no dia
    }

    #[test]
    fn anti_laco_no_maximo_8_tentativas_por_hora_na_mesma_conversa() {
        let mut s = Sim::novo("laco");
        s.cfg.max_por_chat_hora = 100;
        s.cfg.max_por_hora = 100;
        for i in 0..12 {
            s.espera_e_atende(&[l("Ana", "17:50", &format!("mensagem {i}"))]);
        }
        assert_eq!(s.n(), 8);
        assert!(std::fs::read_to_string(s.d.join("vigia.log")).unwrap().contains("anti-laco")); // nunca em silencio
    }

    #[test]
    fn janela_de_horario_e_interruptor() {
        let cfg = Cfg::default();
        let h = |hora, min| Agora { hora, min, ..agora() };
        assert!(dentro_da_janela(&cfg, &h(7, 0)) && dentro_da_janela(&cfg, &h(23, 58)));
        assert!(!dentro_da_janela(&cfg, &h(6, 59)) && !dentro_da_janela(&cfg, &h(23, 59)) && !dentro_da_janela(&cfg, &h(3, 0)));
        let d = tmp("janela");
        assert_eq!(modo(&d, &cfg, &agora()), None); // padrao: DESLIGADO
        ligar(&d, &agora());
        assert_eq!((modo(&d, &cfg, &agora()), modo(&d, &cfg, &h(3, 0))), (Some(false), Some(true)));
        power::desligar(&d, "teste", "");
        assert_eq!(modo(&d, &cfg, &agora()), None); // Braniac pausado: o vigia para sozinho
        power::ligar(&d);
        desligar(&d, &agora());
        assert_eq!((modo(&d, &cfg, &agora()), resumo(&d, T0).0), (None, false));
        let c = cfg_de(&json!({"estavel_s": 15, "janela": ["08:30", "20:00"], "max_por_dia": 5, "max_por_hora": "muitos"}));
        assert_eq!(c, Cfg { estavel_s: 15.0, janela: ((8, 30), (20, 0)), max_por_dia: 5, ..Cfg::default() });
        assert_eq!(cfg_de(&json!({"janela": ["sempre"]})), Cfg::default());
    }

    #[test]
    fn religar_nao_reenvia_o_que_ja_foi_tratado() {
        let mut s = Sim::novo("religar");
        let linhas = [l("Ana", "17:50", "tem vaga?")];
        s.espera_e_atende(&linhas);
        desligar(&s.d, &agora());
        ligar(&s.d, &agora());
        s.espera_e_atende(&linhas);
        assert_eq!((s.n(), resumo(&s.d, s.t)), (1, (true, 1, 1)));
    }

    #[test]
    fn prioritario_e_sempre_atendido_mesmo_com_limites_e_fora_do_horario() {
        let mut s = Sim::novo("prio");
        s.cfg.max_por_hora = 0;
        s.resolve = false;
        let linhas = [l("Loja Sol", "17:50", "como fecho o caixa?"), l("Ana", "17:50", "oi")];
        s.ciclo(&linhas, 0.0);
        for _ in 0..4 {
            assert_eq!(s.ciclo(&linhas, 20.0).candidatos, v(&["Loja Sol"])); // sem limite, sem desistencia
        }
        let mut s = Sim::novo("prio2");
        let linhas = [l("Loja Sol", "17:50", "oi"), l("Ana", "17:50", "oi")];
        s.ciclo_em(&linhas, 0.0, true);
        assert_eq!(s.ciclo_em(&linhas, 20.0, true).candidatos, v(&["Loja Sol"])); // fora do horario so o prioritario
    }

    #[test]
    fn saudacao_automatica_nao_conta_como_resposta_nossa() {
        let mut s = Sim::novo("saudacao");
        let r = s.espera_e_atende(&[nossa("Ana", "17:50", "A empresa agradece seu contato. Como podemos ajudar?")]);
        assert_eq!(r.candidatos, v(&["Ana"]));
    }

    #[test]
    fn linha_nossa_nunca_vira_tratada() {
        let mut s = Sim::novo("icone");
        s.espera_e_atende(&[l("Ana", "17:50", "oi")]);
        s.ciclo(&[nossa("Ana", "17:50", "oi")], 20.0); // o icone mudou: sai de "tratada"
        assert!(estado(&s.d).tratados.is_empty());
        assert_eq!(s.espera_e_atende(&[l("Ana", "17:50", "oi")]).candidatos, v(&["Ana"])); // continua elegivel
    }

    type Env = RefCell<Vec<String>>;

    /// Saidas falsas: tudo e anotado como "tipo|destino|conteudo"; o que contem "FALHA" nao sai.
    fn rodar_outbox(d: &Path, env: &Env, jorge_ok: bool) -> usize {
        let anota = |tipo: &str, dest: &str, c: &str| {
            env.borrow_mut().push(format!("{tipo}|{dest}|{c}"));
            if c.contains("FALHA") { "erro no envio".to_string() } else { "✅ ok".to_string() }
        };
        enviar_outbox(d, &agora(), T0, &Saidas {
            texto: &|a, b| anota("texto", a, b), audio: &|a, b| anota("audio", a, b), arquivo: &|a, b, c| anota("arquivo", a, &format!("{b}|{c}")),
            voz: &|a, p| anota("voz", a, &p.file_name().unwrap().to_string_lossy()),
            voz_agente: &|ag, a, b| if jorge_ok { anota(ag, a, b) } else { "⚠️ audio falhou".into() },
            alma: &|t| (t.is_empty()).then(|| "Oi, meu bem.".to_string()), alma_audio: &|t| env.borrow_mut().push(format!("alma_audio|{t}")),
        })
    }

    #[test]
    fn outbox_entrega_cada_tipo_de_bloco_e_limpa() {
        let (d, env) = (tmp("outbox"), Env::default());
        let arq = d.join("outbox_davi.txt");
        assert_eq!(outbox(&d, &Ctx::ler(&d)), arq);
        assert_eq!(rodar_outbox(&d, &env, true), 0); // inexistente
        std::fs::write(&arq, "  \n").unwrap();
        assert_eq!(rodar_outbox(&d, &env, true), 0); // vazio
        let wav = d.join("teste.wav");
        std::fs::write(&wav, "RIFF").unwrap();
        std::fs::write(&arq, format!("\u{feff}achei 3 vagas\n---\n[audio] Terminei o site, senhor.\n ---  \n[Arquivo] C:/x/video.mp4\nTeste do estúdio\nlinha 2\n---\n\
[voz] \"{}\"\n---\n[voz] nao_existe.wav\n---\n[alma]\n---\n[alma] @novidades\n---\n[alma] Dormiu bem?\n---\n[jorge] Tia, o hóspede chega hoje.\n---\n[resumo] esteira: 2 vídeos prontos", wav.display())).unwrap();
        assert_eq!(rodar_outbox(&d, &env, true), 10);
        assert_eq!(*env.borrow(), ["texto|Eu(D)|Claudinho (via Braniac): achei 3 vagas", "audio|Eu(D)|Terminei o site, senhor.", "arquivo|Eu(D)|C:/x/video.mp4|Teste do estúdio\nlinha 2",
            "voz|Eu(D)|teste.wav", "texto|Eu(D)|Oi, meu bem.", "alma_audio|Oi, meu bem.", "texto|Eu(D)|Dormiu bem?", "alma_audio|Dormiu bem?", "jorge|Mainha|Tia, o hóspede chega hoje."]);
        assert!(!arq.exists()); // tudo entregue: o arquivo some
        assert!(std::fs::read_to_string(d.join("funil_dono.json")).unwrap().contains("esteira: 2 vídeos prontos")); // [resumo] nao sai na hora
        // audio do agente falhou: cai no texto para a mesma pessoa
        env.borrow_mut().clear();
        std::fs::write(&arq, "[jorge] Tia, chegou.").unwrap();
        assert_eq!(rodar_outbox(&d, &env, false), 1);
        assert_eq!(*env.borrow(), ["texto|Mainha|Tia, chegou."]);
    }

    #[test]
    fn outbox_mantem_o_que_nao_conseguiu_enviar_e_o_que_chegou_durante_a_entrega() {
        let (d, env) = (tmp("outbox2"), Env::default());
        let arq = d.join("outbox_davi.txt");
        std::fs::write(&arq, "um\n---\ndois FALHA\n---\ntres").unwrap();
        assert_eq!(rodar_outbox(&d, &env, true), 1);
        assert_eq!(std::fs::read_to_string(&arq).unwrap(), "dois FALHA\n---\ntres"); // parou no que falhou; a ordem fica
        // sem confirmacao na tela: nao reenvia (reenviar mandava o mesmo audio varias vezes)
        std::fs::write(&arq, "[audio] oi\n---\nfim").unwrap();
        let n = enviar_outbox(&d, &agora(), T0, &Saidas {
            texto: &|_, _| "✅".into(), audio: &|_, _| "Cliquei em enviar, mas não vi a mensagem".into(), arquivo: &|_, _, _| "✅".into(), voz: &|_, _| "✅".into(),
            voz_agente: &|_, _, _| "✅".into(), alma: &|_| None, alma_audio: &|_| (),
        });
        assert!(n == 2 && !arq.exists());
        // outro processo gravou um bloco enquanto entregavamos: ele fica para o proximo ciclo
        std::fs::write(&arq, "primeiro").unwrap();
        let n = enviar_outbox(&d, &agora(), T0, &Saidas {
            texto: &|_, _| {
                std::fs::write(&arq, "primeiro\n---\nchegou depois").unwrap();
                "✅".into()
            },
            audio: &|_, _| "✅".into(), arquivo: &|_, _, _| "✅".into(), voz: &|_, _| "✅".into(), voz_agente: &|_, _, _| "✅".into(), alma: &|_| None, alma_audio: &|_| (),
        });
        assert_eq!((n, std::fs::read_to_string(&arq).unwrap().as_str()), (1, "chegou depois"));
    }

    #[test]
    fn queda_loga_e_reabre_no_maximo_uma_vez_por_hora() {
        let mut q = Queda::default();
        assert_eq!(q.registrar(true, T0), (true, true));
        assert_eq!(q.registrar(true, T0 + 60.0), (false, false));
        assert_eq!(q.registrar(false, T0 + 3600.0), (true, false)); // outro erro: loga, mas nao reabre
        assert_eq!(q.registrar(true, T0 + 3700.0), (false, true));
        assert!(q.voltou() && !q.voltou());
        assert_eq!(q.registrar(true, T0 + 3800.0), (true, true)); // caiu de novo: comeca do zero
    }
}
