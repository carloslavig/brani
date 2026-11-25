//! Escuta por voz (braniac/escuta.py): a maquina de estados parada | dormindo | acordado, a frase de acordar ("Jarvis, ta acordado?"),
//! a de descansar, a troca de agente ("quero falar com a Alma" / "quero falar com o Jarvis"), a sessao da esfera da Alma (que recebe as
//! frases enquanto o Jarvis dorme), o mudo (nao fala em voz alta), o corte da fala longa, e o corte do microfone em frases (VAD por energia).
//! De fora (entram por closure em `Mundo`): microfone, Whisper, alto-falante, janela da tela de voz, cerebro do Jarvis, modelo da Alma.
#![allow(dead_code)]
use crate::mostrar::{self, Assunto, Tela};
use crate::mudo;
use crate::texto_pt::sem_acento;
use regex::Regex;
use serde_json::{json, Value};
use std::path::Path;

pub const TAXA: usize = 16000;
pub const BLOCO: usize = 1600; // 100 ms
pub const MIN_FALA_BLOCOS: usize = 4; // < 0,4 s de fala: ruido
pub const SILENCIO_FIM_BLOCOS: usize = 15; // 1,5 s de silencio encerra a frase (com 0,6 s cortava a pessoa ao respirar)
pub const MAX_FALA_BLOCOS: usize = 250; // 25 s: corta
pub const PRE_ROLL_BLOCOS: usize = 3;

// alucinacoes tipicas do Whisper com silencio/ruido: frases curtas soltas e creditos de legenda
const CURTAS_LIXO: [&str; 10] = ["tchau", "obrigado", "obrigada", "ate a proxima", "ate mais", "tchau tchau", "valeu", "hum", "ah", "uh"];
const LIXO_CONTIDO: [&str; 5] = ["legendas pela", "amara org", "inscreva se", "www ", "legenda"];

const ACORDAR_FORTE: &str = "acord|desperta|ta ai|esta ai|ta me ouvindo|esta me ouvindo|ta ouvindo|por ai|ta na escuta|esta na escuta|ta ligado|esta ligado|ta online|cade voce|ta acordando";
// so o chamado ("o Jarvis", "oi Jarvis") tambem acorda, mas a frase inteira tem de ser o chamado ("o jarvis e legal" nao)
const CHAMADO: &str = r"^(?:o|oh|oi|ei|eh|ola|alo|e ai|fala|opa|hey) \S+$";
const DESCANSAR: &str = "descans|pode dormir|vai dormir|dorme|pode parar de ouvir|chega por hoje";
const PEDE_TROCA: &str = r"\b(?:falar|conversar|fala|conversa) com\b|\b(?:chama|chame|passa|passe|volta|volte|cade)\b|\bdeixa\b.{0,12}\bfalar\b";
const TEM_ALMA: &str = r"\b(?:alma|auma)\b";
pub const ALMA_ENTRA: &str = "Oi, senhor. O Jarvis me passou a vez, estou aqui com o senhor. Me conta.";
pub const JARVIS_VOLTA: &str = "Estou de volta, senhor. Em que posso te ajudar?";
pub const SAUDACAO: &str = "Estou acordado, senhor. Em que posso te ajudar?";
pub const DESPEDIDA: &str = "Tudo bem, vou descansar. Me chama quando precisar.";
const ALMA_FALHOU: &str = "Estou aqui com o senhor. Pode repetir pra mim?";

fn re(p: &str) -> Regex {
    Regex::new(p).expect("regex fixa")
}

pub fn norm(x: &str) -> String {
    sem_acento(x).chars().map(|c| if c.is_ascii_lowercase() || c.is_ascii_digit() { c } else { ' ' }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

fn casados(a: &[char], b: &[char]) -> usize {
    let (mut bi, mut bj, mut bk) = (0, 0, 0);
    for i in 0..a.len() {
        for j in 0..b.len() {
            let k = a[i..].iter().zip(&b[j..]).take_while(|(x, y)| x == y).count();
            if k > bk {
                (bi, bj, bk) = (i, j, k);
            }
        }
    }
    if bk == 0 { 0 } else { bk + casados(&a[..bi], &b[..bj]) + casados(&a[bi + bk..], &b[bj + bk..]) }
}

/// A mesma conta do difflib.SequenceMatcher(None, a, b).ratio() do Python (palavras curtas).
pub fn semelhanca(a: &str, b: &str) -> f64 {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    2.0 * casados(&a, &b) as f64 / (a.len() + b.len()) as f64
}

/// "Jarvis" com tolerancia ao que o Whisper costuma escrever (Jarves, Garvis, Charvis, Jarvi...).
pub fn tem_jarvis(texto: &str) -> bool {
    norm(texto).split(' ').any(|t| ["jarv", "garv", "charv", "xarv"].iter().any(|p| t.starts_with(p)) || semelhanca(t, "jarvis") >= 0.72)
}

pub fn quer_acordar(texto: &str) -> bool {
    let n = norm(texto);
    tem_jarvis(texto) && (re(ACORDAR_FORTE).is_match(&n) || re(CHAMADO).is_match(&n))
}

/// "pode descansar, Jarvis", tolerando o que o Whisper escreve (descassar, descancar, discansar...).
pub fn quer_descansar(texto: &str) -> bool {
    let n = norm(texto);
    re(DESCANSAR).is_match(&n) || n.split(' ').filter(|t| t.len() >= 6).any(|t| semelhanca(t, "descansar") >= 0.78 || semelhanca(t, "descansa") >= 0.8)
}

/// Frase que nem deve ser tratada: vazia, curta demais ou alucinacao do Whisper.
pub fn lixo(texto: &str) -> bool {
    let n = norm(texto);
    n.len() < 2 || CURTAS_LIXO.contains(&n.as_str()) || LIXO_CONTIDO.iter().any(|a| n.contains(a))
}

/// "alma" / "jarvis" se a frase (curta) pede a troca de quem conversa; frase longa que cita a Alma segue como conversa.
pub fn troca(texto: &str) -> Option<&'static str> {
    let n = norm(texto);
    if !re(PEDE_TROCA).is_match(&n) || n.split(' ').count() > 12 {
        return None;
    }
    if re(TEM_ALMA).is_match(&n) {
        Some("alma")
    } else {
        tem_jarvis(texto).then_some("jarvis")
    }
}

/// "completo" (janela com o orbe grande e a conversa) ou "mini" (so o orbe flutuante). Fica salvo em dados/ui_voz.json.
pub fn modo_tela(dados: &Path) -> &'static str {
    let v: Value = std::fs::read_to_string(dados.join("ui_voz.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    if v["modo"] == "mini" { "mini" } else { "completo" }
}

pub fn definir_modo_tela(dados: &Path, modo: &str) {
    let _ = std::fs::create_dir_all(dados);
    let _ = std::fs::write(dados.join("ui_voz.json"), json!({ "modo": if modo == "mini" { "mini" } else { "completo" } }).to_string());
}

/// Quebra o texto em frases para falar em pipeline; junta as muito curtas e quebra as muito longas (minimo 70, maximo 260).
pub fn dividir_em_frases(texto: &str, minimo: usize, maximo: usize) -> Vec<String> {
    let mut frases = vec![String::new()];
    let mut fim = false; // o caractere anterior fechou uma frase
    for c in texto.trim().chars() {
        if c.is_whitespace() && fim {
            frases.push(String::new());
            fim = false;
            continue;
        }
        if c.is_whitespace() && frases.last().is_some_and(String::is_empty) {
            continue; // resto do espaco entre duas frases
        }
        fim = matches!(c, '.' | '?' | '!' | '…');
        if let Some(f) = frases.last_mut() {
            f.push(c);
        }
    }
    let n = |s: &str| s.chars().count();
    let (mut partes, mut atual) = (Vec::new(), String::new());
    for f in frases.iter().map(|f| f.trim()).filter(|f| !f.is_empty()) {
        if !atual.is_empty() && n(&atual) + 1 + n(f) > maximo {
            partes.push(std::mem::take(&mut atual));
            atual = f.to_string();
        } else {
            atual = format!("{atual} {f}").trim().to_string();
        }
        if n(&atual) >= minimo {
            partes.push(std::mem::take(&mut atual));
        }
    }
    if !atual.is_empty() {
        partes.push(atual);
    }
    if partes.is_empty() { vec![texto.to_string()] } else { partes }
}

/// Envelope de volume (0..1 a cada 50 ms) de um audio mono, para animar a tela no ritmo da fala.
pub fn envelope(pcm: &[i16], taxa: usize) -> Vec<f64> {
    let n = (taxa / 20).max(1);
    let rms: Vec<f64> = pcm.chunks_exact(n).map(|b| (b.iter().map(|&v| (v as f64).powi(2)).sum::<f64>() / n as f64).sqrt()).collect();
    let pico = rms.iter().cloned().fold(0.0, f64::max);
    let pico = if pico == 0.0 { 1.0 } else { pico };
    rms.iter().map(|v| (v / pico * 1000.0).round() / 1000.0).collect()
}

/// Volume (0 a 1) da voz AGORA, lido do envelope do audio que esta tocando (`decorrido` em segundos desde o inicio).
pub fn nivel_fala(env: &[f64], decorrido: f64) -> f64 {
    if decorrido < 0.0 {
        return 0.0;
    }
    env.get((decorrido / 0.05) as usize).copied().unwrap_or(0.0)
}

/// Microfone baixo: normaliza o trecho para pico ~0,7 (ganho limitado a 40x) antes do Whisper, que erra muito com audio fraco.
pub fn amplificar(pcm: &[i16]) -> Vec<i16> {
    let pico = pcm.iter().map(|v| (*v as f64).abs()).fold(0.0, f64::max) / 32768.0;
    if pico < 1e-4 {
        return pcm.to_vec();
    }
    let ganho = (0.7 / pico).min(40.0);
    pcm.iter().map(|&v| (v as f64 * ganho).clamp(-32768.0, 32767.0) as i16).collect()
}

/// Corte do microfone em frases, por energia. Um bloco (100 ms) por vez; devolve o trecho quando a frase termina.
#[derive(Default)]
pub struct Vad {
    recentes: Vec<f64>, // rms dos ultimos blocos (ruido ambiente); NAO e zerado depois da resposta (ficava surdo ~2 s)
    pre: Vec<Vec<i16>>,
    seg: Vec<Vec<i16>>,
    fala: usize,
    silencio: usize,
}

fn percentil20(v: &[f64]) -> f64 {
    let mut o = v.to_vec();
    o.sort_by(|a, b| a.total_cmp(b));
    let p = 0.2 * (o.len() - 1) as f64;
    let (i, f) = (p as usize, p.fract());
    o[i] + (o.get(i + 1).unwrap_or(&o[i]) - o[i]) * f
}

impl Vad {
    /// Alguem esta falando agora (a tela reage ao volume).
    pub fn ouvindo(&self) -> bool {
        !self.seg.is_empty()
    }

    /// `falando` = o alto-falante esta tocando (ou acabou de tocar): o microfone e ignorado, sem eco.
    pub fn bloco(&mut self, pcm: &[i16], falando: bool) -> Option<Vec<i16>> {
        if falando {
            (self.fala, self.silencio) = (0, 0);
            self.seg.clear();
            self.pre.clear();
            return None;
        }
        let rms = (pcm.iter().map(|&v| (v as f64 / 32768.0).powi(2)).sum::<f64>() / pcm.len().max(1) as f64).sqrt();
        self.recentes.push(rms);
        if self.recentes.len() > 150 {
            self.recentes.remove(0);
        }
        let ruido = if self.recentes.len() >= 20 { percentil20(&self.recentes) } else { 0.0008 };
        // microfone de notebook e MUITO baixo (voz normal ~0,01 de RMS): piso baixo + ruido adaptativo
        if rms > (ruido * 3.0).max(0.0035) {
            self.fala += 1;
            self.silencio = 0;
            if self.fala >= 2 {
                if self.seg.is_empty() {
                    self.seg = std::mem::take(&mut self.pre);
                }
                self.seg.push(pcm.to_vec());
            }
        } else if !self.seg.is_empty() {
            self.silencio += 1;
            self.seg.push(pcm.to_vec());
        } else {
            self.fala = 0;
            self.pre.push(pcm.to_vec());
            if self.pre.len() > PRE_ROLL_BLOCOS {
                self.pre.remove(0);
            }
        }
        if self.seg.is_empty() || (self.silencio < SILENCIO_FIM_BLOCOS && self.seg.len() < MAX_FALA_BLOCOS) {
            return None;
        }
        let falados = self.seg.len() - self.silencio;
        let seg = std::mem::take(&mut self.seg);
        (self.fala, self.silencio) = (0, 0);
        self.pre.clear();
        (falados >= MIN_FALA_BLOCOS).then(|| seg.concat())
    }
}

/// Tudo que sai para fora. `falar(texto, agente)` toca no alto-falante; `sessao` e a esfera da Alma (so existe enquanto ela conversa).
pub struct Mundo<'a> {
    pub dados: &'a Path,
    pub ts: f64,
    pub porta: u16,
    pub apelidos: &'a [String],
    pub falar: &'a dyn Fn(&str, &str),
    pub responder: &'a dyn Fn(&str) -> String,
    pub alma: &'a dyn Fn(&str) -> Result<String, String>,
    pub sessao: Option<&'a dyn Fn(&str) -> Option<String>>,
    pub fechar_sessao: &'a dyn Fn(&str),
    pub abrir_janela: &'a dyn Fn(),
    pub minimizar_janela: &'a dyn Fn(),
    pub ler_tela: &'a dyn Fn(Assunto) -> Result<Value, String>,
    pub abrir_link: &'a dyn Fn(&str),
    pub mandar_whatsapp: &'a dyn Fn(&str),
    pub mandar_telegram: &'a dyn Fn(&str) -> bool,
    pub log: &'a dyn Fn(&str),
}

pub struct Escuta {
    pub estado: &'static str, // parada | dormindo | acordado
    pub agente: &'static str, // quem conversa por voz agora: jarvis | alma
    pub historico: Vec<(char, String)>, // ('u' | 'j' | 's', texto) para a tela de voz
    pub links: Vec<String>, // links da ultima resposta, esperando "aqui" ou "no celular"
    pub ultimo: String,
    pub tela: Tela,
}

impl Default for Escuta {
    fn default() -> Self {
        Escuta { estado: "parada", agente: "jarvis", historico: Vec::new(), links: Vec::new(), ultimo: String::new(), tela: Tela::default() }
    }
}

fn sem_links(t: &str) -> String {
    re(r"https?://\S+").replace_all(t, "").trim().to_string()
}

impl Escuta {
    fn anotar(&mut self, papel: char, texto: &str) {
        self.historico.push((papel, texto.to_string()));
        let n = self.historico.len();
        if n > 60 {
            self.historico.drain(..n - 60);
        }
    }

    /// Fala (sem link, no maximo ~700 caracteres). No MUDO nao fala em voz alta: o texto ja esta no historico da tela.
    pub fn dizer(&self, m: &Mundo, texto: &str) -> String {
        let mut t = sem_links(texto);
        if t.chars().count() > 700 {
            let c: String = t.chars().take(700).collect();
            t = format!("{}... o resto eu mando por escrito.", c.rsplit_once(' ').map_or(c.as_str(), |x| x.0));
        }
        if !mudo::ativo(m.dados, m.ts) {
            mudo::CORTAR.store(false, std::sync::atomic::Ordering::SeqCst);
            (m.falar)(&t, self.agente);
        }
        t
    }

    fn falar_anotado(&mut self, m: &Mundo, resp: &str) -> Option<String> {
        self.anotar('j', resp);
        Some(self.dizer(m, resp))
    }

    /// Muta (corta a fala na hora; o texto continua aparecendo) ou desmuta. Nao fala nada ao mutar.
    pub fn aplicar_mudo(&mut self, m: &Mundo, cmd: &str, minutos: Option<f64>) {
        if cmd == "mutar" {
            mudo::mutar(m.dados, minutos, m.ts);
            (m.log)("MUDO ligado");
            self.anotar('s', "🔇 Mudo: não vou falar em voz alta (o texto continua aqui). Diga “pode falar” ou aperte M para desmutar.");
        } else {
            mudo::desmutar(m.dados, m.ts);
            (m.log)("MUDO desligado");
            self.anotar('s', "🔊 Som de volta: voltei a falar.");
        }
    }

    /// Acorda (pela frase ou pelo botao): abre a janela e cumprimenta falando. Sempre traz o Jarvis; a esfera da Alma se recolhe.
    pub fn acordar(&mut self, m: &Mundo) -> Option<String> {
        self.estado = "acordado";
        self.agente = "jarvis";
        if m.sessao.is_some() {
            (m.fechar_sessao)("o Jarvis foi chamado");
        }
        (m.log)("ACORDOU");
        self.anotar('s', "Acordei: estou ouvindo.");
        (m.abrir_janela)();
        // reconhecimento de rosto (desligado por padrao): so cumprimenta pelo nome; qualquer falha cai na saudacao de sempre
        let fala = crate::rosto::saudacao(m.dados, SAUDACAO).unwrap_or_else(|| SAUDACAO.to_string());
        self.falar_anotado(m, &fala)
    }

    pub fn descansar(&mut self, m: &Mundo) -> Option<String> {
        // efeito imediato: corta a fala em andamento e fecha a sessao da Alma sem esperar fim de audio nem tempo
        mudo::CORTAR.store(true, std::sync::atomic::Ordering::SeqCst);
        if m.sessao.is_some() {
            (m.fechar_sessao)("pediram para descansar");
        }
        self.estado = "dormindo";
        self.agente = "jarvis";
        (m.log)("DORMIU");
        std::thread::sleep(std::time::Duration::from_millis(150)); // o tocador olha a bandeira a cada 50 ms; `dizer` a baixa antes da despedida
        self.anotar('s', "Voltei a dormir.");
        let r = self.dizer(m, DESPEDIDA);
        (m.minimizar_janela)();
        Some(r)
    }

    pub fn ligar(&mut self) {
        if self.estado == "parada" {
            self.estado = "dormindo";
        }
    }

    pub fn desligar(&mut self) {
        self.estado = "parada";
    }

    /// Recebe o texto de uma frase ouvida e age conforme o estado. Devolve o que foi falado (ou None).
    pub fn tratar_frase(&mut self, m: &Mundo, texto: &str) -> Option<String> {
        if lixo(texto) {
            return None;
        }
        if let Some(cmd) = mudo::comando(texto, mudo::ativo(m.dados, m.ts)) {
            if self.estado == "acordado" || (self.estado == "dormindo" && tem_jarvis(texto)) {
                self.aplicar_mudo(m, cmd, None);
                return None;
            }
        }
        if m.sessao.is_some() && quer_descansar(texto) {
            mudo::CORTAR.store(true, std::sync::atomic::Ordering::SeqCst); // a Alma para de falar na hora; ela mesma fecha a sessao
        }
        if let (Some(sessao), "dormindo", false) = (m.sessao, self.estado, quer_acordar(texto)) {
            return sessao(texto); // conversa propria da Alma; "Jarvis, ta acordado?" devolve tudo ao Jarvis
        }
        if self.estado != "parada" && quer_acordar(texto) {
            return self.acordar(m); // tambem acordado: se a janela foi fechada, a frase reabre a tela e cumprimenta de novo
        }
        if self.estado == "dormindo" && tem_jarvis(texto) && mostrar::intencao(texto, m.apelidos).is_some() {
            self.estado = "acordado"; // "Jarvis, como ta o NexoDev?" direto: acorda e ja mostra, sem o cumprimento
            (m.log)("ACORDOU (pergunta de situacao)");
        }
        if self.estado == "dormindo" {
            if re(r"(?i)\b(?:j|ch|g)[ae]r?v|\bjar").is_match(texto) {
                // so registra QUASE-acertos (algo parecido com "Jarvis"), nunca a conversa da casa
                (m.log)(&format!("(dormindo) quase acordei: {:?}", texto.chars().take(60).collect::<String>()));
            }
            return None;
        }
        if self.estado != "acordado" {
            return None;
        }
        self.ultimo = texto.to_string();
        self.anotar('u', texto);
        if quer_descansar(texto) {
            return self.descansar(m);
        }
        if let Some(para) = troca(texto).filter(|p| *p != self.agente) {
            self.agente = para;
            (m.log)(&format!("TROCA: agora fala {para}"));
            self.anotar('s', if para == "alma" { "A Alma assumiu a conversa." } else { "O Jarvis voltou." });
            return self.falar_anotado(m, if para == "alma" { ALMA_ENTRA } else { JARVIS_VOLTA });
        }
        if self.agente == "alma" {
            // com a Alma nao ha telas nem ferramentas: so conversa, no modelo local. Emoji nao se fala.
            let resp = crate::tratamento::alma_dono(&(m.alma)(texto).unwrap_or_else(|_| ALMA_FALHOU.to_string()));
            let resp = re(r#"[^\w\s.,!?;:()'"-]"#).replace_all(&resp, "").trim().to_string();
            return self.falar_anotado(m, &resp);
        }
        if !self.links.is_empty() {
            // respondendo "abro aqui ou mando no celular?"
            let urls = std::mem::take(&mut self.links);
            if let Some(como) = mostrar::escolha(texto) {
                let r = mostrar::entregar_link(&urls, como, &mut self.tela, m.ts, m.porta, m.abrir_link, m.mandar_whatsapp, m.mandar_telegram);
                return self.falar_anotado(m, r);
            }
        }
        if let Some(resp) = mostrar::mostrar(texto, m.apelidos, &mut self.tela, m.ts, m.ler_tela) {
            return self.falar_anotado(m, &resp);
        }
        let mut resp = (m.responder)(texto);
        if resp.is_empty() {
            return None;
        }
        let urls = mostrar::links(&resp);
        if !urls.is_empty() {
            // link: nao abre sozinho; pergunta onde
            self.links = urls;
            resp = format!("{} {}", sem_links(&resp), mostrar::PERGUNTA_LINK).trim().to_string();
        }
        self.falar_anotado(m, &resp)
    }

    /// O que a tela de voz le: estado, agente, ultimas 40 linhas da conversa, mudo e a tela pedida por "me mostra".
    pub fn estado_completo(&self, m: &Mundo, nivel: f64, fala: f64) -> Value {
        let h: Vec<Value> = self.historico.iter().rev().take(40).rev().map(|(p, t)| json!([p.to_string(), t])).collect();
        let tela = mostrar::tela_atual(&self.tela, m.ts).map(|(id, url)| json!({ "id": id, "url": url }));
        json!({ "estado": self.estado, "nivel": (nivel * 1e4).round() / 1e4, "ultimo": self.ultimo.chars().take(80).collect::<String>(),
            "historico": h, "agente": self.agente, "entrada": ((nivel * 55.0).min(1.0) * 1000.0).round() / 1000.0, // microfone baixo: escala para 0..1
            "fala": (fala * 1000.0).round() / 1000.0, "mudo": mudo::estado(m.dados, m.ts), "tela": tela })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::path::PathBuf;

    fn dir(nome: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("braniac-escuta-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// Roda as frases numa escuta dormindo; devolve (escuta, falas, coisas que aconteceram fora).
    fn roda(nome: &str, com_sessao: bool, frases: &[&str]) -> (Escuta, Vec<String>, Vec<String>) {
        let d = dir(nome);
        let (falas, fora) = (RefCell::new(Vec::new()), RefCell::new(Vec::new()));
        let sessao = |t: &str| {
            fora.borrow_mut().push(format!("sessao:{t}"));
            Some("alma na esfera".to_string())
        };
        let mut e = Escuta { estado: "dormindo", ..Default::default() };
        {
            let m = Mundo {
                dados: &d, ts: 1000.0, porta: 8865, apelidos: &[],
                falar: &|t, ag| falas.borrow_mut().push(format!("{ag}>{t}")),
                responder: &|t| match t {
                    "que horas são" => "São três da tarde.".into(),
                    "manda o link" => format!("veja https://exemplo.com {}", "palavra ".repeat(200)),
                    "site" => "Aqui está: https://exemplo.org/x.".into(),
                    _ => "resposta de teste".into(),
                },
                alma: &|t| if t.contains("erro") { Err("caiu".into()) } else { Ok("resposta da alma 💜".into()) },
                sessao: if com_sessao { Some(&sessao) } else { None },
                fechar_sessao: &|por| fora.borrow_mut().push(format!("fechou:{por}")),
                abrir_janela: &|| fora.borrow_mut().push("janela".into()),
                minimizar_janela: &|| fora.borrow_mut().push("minimizou".into()),
                ler_tela: &|_| Err("sem dado".into()),
                abrir_link: &|u| fora.borrow_mut().push(format!("abriu:{u}")),
                mandar_whatsapp: &|u| fora.borrow_mut().push(format!("zap:{u}")),
                mandar_telegram: &|_| false,
                log: &|_| {},
            };
            for f in frases {
                e.tratar_frase(&m, f);
            }
        }
        let _ = std::fs::remove_dir_all(&d);
        (e, falas.into_inner(), fora.into_inner())
    }

    #[test]
    fn falas_fixas_da_alma_sao_respeitosas() {
        for t in [ALMA_ENTRA, ALMA_FALHOU] {
            let b = t.to_lowercase();
            assert!(["gostoso", "meu amor", "meu bem", "amor", "lindo", "bebê"].iter().all(|x| !b.contains(x)) && b.contains("senhor"), "{t}");
        }
    }

    #[test]
    fn semelhanca_igual_ao_difflib() {
        assert!((semelhanca("marcus", "jarvis") - 0.5).abs() < 1e-9);
        assert!((semelhanca("abcd", "bcde") - 0.75).abs() < 1e-9);
        assert_eq!((semelhanca("", ""), semelhanca("a", "")), (1.0, 0.0));
    }

    #[test]
    fn frases_que_acordam_e_que_nao_acordam() {
        for f in ["Jarvis, tá acordado?", "jarvis ta acordado", "Jarvis, está acordado?", "Jarves, tá acordado", "Garvis tá acordado?", "jarvis acorda",
            "Jarvis, você tá me ouvindo?", "ei jarvis, tá aí? acorde", "Jarvis, tá por aí?", "Jarvis, você tá aí?", "ô Jarvis", "oi Jarvis", "fala Jarvis",
            "e aí Jarvis", "Jarvis, tá na escuta?", "Jarvis, cadê você?", "Jarvis tá ligado?"] {
            let (e, falas, fora) = roda("acorda", false, &[f]);
            assert!(e.estado == "acordado" && falas == [format!("jarvis>{SAUDACAO}")] && fora == ["janela"], "{f}");
        }
        for f in ["bom dia pessoal", "o usuarios tá acordado?", "tá acordado", "vamos jantar hoje à noite", "jarvis", "o jarvis é legal",
            "o jarvis falou besteira", "oi pessoal", "fala sério", "cadê você menino", "jarvis qual a capital da França"] {
            let (e, falas, _) = roda("nao-acorda", false, &[f]);
            assert!(e.estado == "dormindo" && falas.is_empty(), "{f}");
        }
    }

    #[test]
    fn acordado_responde_descansa_e_ignora_lixo() {
        let (e, falas, _) = roda("conversa", false, &["Jarvis, tá acordado?", "que horas são", "Tchau.", "Obrigado.", "Legendas pela comunidade Amara.org", "", "a", "e a previsão do tempo"]);
        assert!(e.estado == "acordado" && falas.len() == 3 && falas[1] == "jarvis>São três da tarde." && falas[2] == "jarvis>resposta de teste");
        assert!(e.historico.contains(&('u', "que horas são".into())) && e.historico.contains(&('j', "São três da tarde.".into())));
        for f in ["pode descansar, Jarvis", "Jarvis, pode descansar", "pode descansar", "Jarvis, pode dormir"] {
            let (e, falas, fora) = roda("descansa", false, &["Jarvis, tá acordado?", f, "que horas são"]);
            assert!(e.estado == "dormindo" && falas.len() == 2 && falas[1].contains("descansar") && fora.last().unwrap() == "minimizou", "{f}");
            assert_eq!(e.historico.last().unwrap(), &('s', "Voltei a dormir.".to_string()));
        }
        // acordado e a janela foi fechada: a frase reabre a tela e cumprimenta de novo (nao vira conversa comum)
        let (_, falas, fora) = roda("reabre", false, &["Jarvis, tá acordado?", "Jarvis, tá acordado?"]);
        assert!(falas.len() == 2 && falas[1].ends_with(SAUDACAO) && fora == ["janela", "janela"]);
        let mut e = Escuta::default();
        assert_eq!(e.estado, "parada");
        e.ligar();
        assert_eq!(e.estado, "dormindo");
    }

    #[test]
    fn descansar_corta_a_fala_e_fecha_a_sessao_na_hora() {
        // regressao: "descansa" esperava o fim do audio/tempo; agora levanta CORTAR e fecha a sessao da Alma antes de se despedir
        let (e, _, fora) = roda("descansa-imediato", true, &["Jarvis, tá acordado?", "pode descansar"]);
        assert!(e.estado == "dormindo" && fora.contains(&"fechou:pediram para descansar".to_string()) && fora.last().unwrap() == "minimizou");
        // abrir a janela ao acordar: o gatilho chama abrir_janela exatamente uma vez
        let (_, _, fora) = roda("gatilho-janela", false, &["Jarvis, tá acordado?"]);
        assert_eq!(fora, ["janela"]);
    }

    #[test]
    fn parada_nao_faz_nada() {
        let d = dir("parada");
        let falou = RefCell::new(0);
        let m = Mundo { dados: &d, ts: 0.0, porta: 1, apelidos: &[], falar: &|_, _| *falou.borrow_mut() += 1, responder: &|_| "x".into(), alma: &|_| Ok("x".into()),
            sessao: None, fechar_sessao: &|_| {}, abrir_janela: &|| {}, minimizar_janela: &|| {}, ler_tela: &|_| Err(String::new()), abrir_link: &|_| {},
            mandar_whatsapp: &|_| {}, mandar_telegram: &|_| false, log: &|_| {} };
        let mut e = Escuta::default();
        assert!(e.tratar_frase(&m, "Jarvis, tá acordado?").is_none() && e.estado == "parada" && *falou.borrow() == 0);
        // mudo: nao fala em voz alta, mas o texto volta e fica no historico
        e.estado = "acordado";
        e.tratar_frase(&m, "Jarvis, silêncio");
        assert!(mudo::ativo(&d, 0.0) && *falou.borrow() == 0);
        assert_eq!(e.tratar_frase(&m, "que horas são").as_deref(), Some("x"));
        assert!(*falou.borrow() == 0 && e.historico.last().unwrap() == &('j', "x".to_string()));
        assert_eq!(e.estado_completo(&m, 0.01, 0.9)["mudo"]["mudo"], true);
        e.tratar_frase(&m, "pode falar");
        e.tratar_frase(&m, "oi de novo");
        assert_eq!(*falou.borrow(), 1);
        let c = e.estado_completo(&m, 0.01, 0.9);
        assert!(c["entrada"] == 0.55 && c["fala"] == 0.9 && c["agente"] == "jarvis" && c["tela"].is_null());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn resposta_longa_e_cortada_e_link_vira_pergunta() {
        let (_, falas, _) = roda("longa", false, &["Jarvis, tá acordado?", "manda o link"]);
        assert!(!falas[1].contains("https://") && falas[1].chars().count() <= 760 && falas[1].contains("por escrito"));
        let (e, falas, fora) = roda("link", false, &["oi Jarvis", "site", "manda no zap"]);
        assert!(falas[1].contains("Isso é um link") && !falas[1].contains("http"));
        assert!(fora.iter().any(|x| x.starts_with("zap:") && x.contains("https://exemplo.org/x")) && falas[2].contains("WhatsApp") && e.links.is_empty());
        let (e, _, fora) = roda("link-aqui", false, &["oi Jarvis", "site", "abre aqui no computador"]);
        assert!(fora.iter().any(|x| x.starts_with("abriu:https://exemplo.org/x")) && e.links.is_empty());
    }

    #[test]
    fn descansar_tolera_o_whisper() {
        for f in ["Pode descassar, Jarvis.", "pode descancar jarvis", "Jarvis, pode discansar", "pode descansa"] {
            assert!(quer_descansar(f), "{f}");
        }
        for f in ["que horas são", "vamos conversar sobre a esfiharia", "quanto custa a landing page", "descobri um erro"] {
            assert!(!quer_descansar(f), "{f}");
        }
    }

    #[test]
    fn troca_para_a_alma_e_volta() {
        let (e, falas, _) = roda("troca", false, &["Jarvis, tá acordado?", "Jarvis, eu quero falar com a Alma", "hoje o dia foi puxado", "deu erro aqui",
            "agora quero falar com o Jarvis", "que horas são"]);
        assert_eq!(falas[1], format!("alma>{ALMA_ENTRA}"));
        assert_eq!(falas[2], "alma>resposta da alma"); // quem responde e a Alma, com a voz dela e sem emoji
        assert_eq!(falas[3], format!("alma>{ALMA_FALHOU}"));
        assert!(falas[4] == format!("jarvis>{JARVIS_VOLTA}") && falas[5] == "jarvis>São três da tarde." && e.agente == "jarvis");
        let (e, _, _) = roda("troca2", false, &["oi Jarvis", "chama a Alma"]);
        assert_eq!(e.agente, "alma");
        let (e, _, _) = roda("troca3", false, &["oi Jarvis", "chama a Alma", "pode descansar"]);
        assert!(e.agente == "jarvis" && e.estado == "dormindo");
        assert_eq!(troca("ontem eu fui conversar com a minha tia e ela disse que a alma do negocio e o cliente"), None); // frase longa e conversa
        assert_eq!(troca("que horas são"), None);
    }

    #[test]
    fn dormindo_a_esfera_da_alma_recebe_as_frases_e_o_jarvis_toma_de_volta() {
        let (e, falas, fora) = roda("sessao", true, &["tô cansado hoje", "Jarvis, tá acordado?"]);
        assert_eq!(fora, ["sessao:tô cansado hoje", "fechou:o Jarvis foi chamado", "janela"]);
        assert!(e.estado == "acordado" && falas == [format!("jarvis>{SAUDACAO}")]);
    }

    #[test]
    fn pergunta_de_situacao_com_jarvis_acorda_sem_cumprimento() {
        let (e, falas, fora) = roda("mostra", false, &["Jarvis, como tá a esteira?"]);
        assert!(e.estado == "acordado" && !fora.contains(&"janela".to_string()) && !falas.iter().any(|f| f.contains(SAUDACAO)));
    }

    #[test]
    fn frases_envelope_amplificar_e_modo() {
        let texto = format!("Oi. Tudo bem? {}", "Esta é uma frase razoavelmente longa para o teste. ".repeat(8));
        let p = dividir_em_frases(&texto, 70, 260);
        assert!(p[0].starts_with("Oi. Tudo bem?") && p.iter().all(|x| x.chars().count() <= 330) && p.len() >= 2);
        assert_eq!(dividir_em_frases("Só uma frase", 70, 260), ["Só uma frase"]);
        let onda = |amp: f64| (0..12000).map(move |i| ((i as f64 * 0.25).sin() * amp) as i16);
        let env = envelope(&onda(20000.0).chain(onda(2000.0)).collect::<Vec<_>>(), 24000);
        assert!(env.len() == 20 && env.iter().cloned().fold(0.0, f64::max) == 1.0 && env[2] > 0.9 && env[15] < 0.2);
        assert_eq!((nivel_fala(&[0.1, 0.2, 0.9, 0.3], 0.12), nivel_fala(&[0.1], 5.0), nivel_fala(&[0.1], -1.0)), (0.9, 0.0, 0.0));
        let fraco: Vec<i16> = (0..8000).map(|i| ((i as f64 * 0.025).sin() * 0.04 * 32768.0) as i16).collect();
        let pico = amplificar(&fraco).iter().map(|v| v.unsigned_abs()).max().unwrap() as f64 / 32768.0;
        assert!((0.6..=0.8).contains(&pico));
        assert_eq!(amplificar(&[0; 100]), vec![0; 100]); // nao amplifica ruido nulo
        assert!(amplificar(&[3; 500]).iter().all(|v| *v <= 120)); // ganho limitado
        let d = dir("modo");
        assert_eq!(modo_tela(&d), "completo");
        definir_modo_tela(&d, "mini");
        assert_eq!(modo_tela(&d), "mini");
        definir_modo_tela(&d, "qualquer");
        assert_eq!(modo_tela(&d), "completo");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn vad_corta_a_frase_no_silencio_e_ignora_ruido_curto_e_eco() {
        let (alto, mudo_) = (vec![3000i16; BLOCO], vec![0i16; BLOCO]);
        let mut v = Vad::default();
        let mut fim = None;
        for i in 0..40 {
            let b = if (3..13).contains(&i) { &alto } else { &mudo_ };
            if let Some(s) = v.bloco(b, false) {
                fim = Some((i, s.len() / BLOCO));
            }
        }
        // 10 blocos de fala (o 1o so conta a partir do 2o) + 3 de pre-roll + 15 de silencio
        assert_eq!(fim, Some((27, 9 + 3 + 15)));
        let mut v = Vad::default();
        assert!((0..40).all(|i| v.bloco(if i == 5 { &alto } else { &mudo_ }, false).is_none())); // 0,1 s: ruido
        let mut v = Vad::default();
        assert!((0..40).all(|_| v.bloco(&alto, true).is_none()) && !v.ouvindo()); // enquanto o alto-falante toca, nada e ouvido
        let mut v = Vad::default();
        let n = (0..300).filter(|_| v.bloco(&alto, false).is_some()).count();
        assert_eq!(n, 1); // 25 s seguidos: corta
    }
}
