//! Voz propria da Alma (braniac/alma_voz.py): a sessao de conversa da esfera, quando ela pode abrir (nunca com o Jarvis em conversa, no mudo,
//! com o Braniac pausado ou com outro programa usando o microfone), a presenca (em que nivel aparecer para a sobrecarga de agora), o bom dia
//! por voz (5h-12h, 1x por dia), o "trabalhando ate tarde", e a ajuda: so com o "sim" da pessoa, no maximo 1 ticket por dia.
//! De fora (closures em `Voz`): alto-falante, janela da esfera, leitura do registro do microfone, retrato do escritorio, abrir ticket.
//! Diferente do Python: sem threads. A espera de 45 s pela resposta e um prazo (`espera_ate`) que quem liga confere com `expirar`.
#![allow(dead_code)]
use crate::alma_cuida::{self as ac, rx, ts, Alma};
use crate::alma_mente as am;
use crate::atividade::{self, Leitura};
use crate::tempo::{Agora, Data};
use crate::{barramento, mudo, power, rede_apoio, sobrecarga};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;

pub const ESPERA_S: f64 = 45.0; // quanto ela espera a resposta antes de se recolher
const SIM: &str = r"(?i)^(?:sim|pode|quero|claro|por favor|manda|vai|isso|pode sim|quero sim|pode ser|faz isso|bora|aham|uhum|ok)\b";
const NAO: &str = r"(?i)^(?:n[aã]o|nao precisa|deixa|deixa quieto|depois|agora n[aã]o)\b";
const FIM: &str = r"(?i)\b(?:tchau|era s[oó] isso|pode ir|pode fechar|at[eé] mais|at[eé] logo|obrigad[oa],? alma|valeu,? alma|[eé] s[oó] isso|vai (?:dormir|descansar)|pode (?:dormir|descansar)|boa noite)\b";

/// Pura. entradas = (programa, inicio, fim) do registro do Windows; fim == 0 = esta usando o microfone AGORA. O proprio Braniac nao conta.
pub fn em_uso(entradas: &[(String, u64, u64)], proprio: &str) -> bool {
    entradas.iter().any(|(nome, ini, fim)| *ini != 0 && *fim == 0 && !nome.to_lowercase().contains(proprio))
}

fn sem_emoji(t: &str) -> String {
    rx(r#"[^\w\s.,!?;:()'"-]"#).replace_all(t, "").trim().to_string()
}

fn maiuscula(ap: &str) -> String {
    ac::maiuscula(ap)
}

/// O mundo de uma chamada: a Alma (dados, nomes, hora, modelo local) e o que sai para fora.
pub struct Voz<'a> {
    pub alma: &'a Alma<'a>,
    /// Relogio do mudo (segundos desde 1970).
    pub relogio: f64,
    /// Fala com a voz DELA, pela mesma trava de alto-falante do Jarvis (nunca duas vozes juntas).
    pub dizer: &'a dyn Fn(&str),
    /// O Jarvis esta acordado ou falando.
    pub jarvis_em_conversa: bool,
    /// Outro programa esta com o microfone aberto (reuniao ou chamada).
    pub em_chamada: bool,
    pub abrir_orbe: &'a dyn Fn(),
    /// Retrato do escritorio: {"agentes": [{"exibir", "estado"}]} (so estados, sem conteudo).
    pub retrato: &'a dyn Fn() -> Result<Value, String>,
    /// (projeto, titulo, descricao) -> abriu? None = time de programacao desligado.
    pub abrir_ticket: Option<&'a dyn Fn(&str, &str, &str) -> bool>,
    pub semente: i64,
}

impl Voz<'_> {
    fn apelido(&self) -> String {
        let ops = am::apelidos(&am::params(self.alma.dados), self.alma.nomes);
        ac::escolher(&ops, self.semente).to_string()
    }
}

#[derive(Default)]
pub struct Sessao {
    pub ativa: bool,
    pub nivel: i64, // 0 repouso; 1 canexemplo; 2 conversa; 3 atencao
    pub estado: &'static str, // idle | pensando | ouvindo | interrompida
    pub historico: Vec<(char, String)>, // ('a' = ela, 'u' = ele)
    pub legenda: String,
    pub oferta: bool,
    pub espontanea: bool,
    pub respondeu: bool,
    pub espera_ate: f64,
}

impl Sessao {
    /// "" = pode abrir e falar. Senao, o motivo.
    pub fn impedida(&self, v: &Voz) -> &'static str {
        if self.ativa {
            "já existe uma conversa da Alma aberta"
        } else if mudo::ativo(v.alma.dados, v.relogio) {
            "mudo ligado"
        } else if power::pausado(v.alma.dados) {
            "Braniac pausado"
        } else if v.jarvis_em_conversa {
            "o Jarvis está em conversa"
        } else if v.em_chamada {
            "microfone em uso por outro programa (reunião ou chamada)"
        } else {
            ""
        }
    }

    /// Abre a esfera no `nivel`, fala `texto` e fica ouvindo. Devolve "" se abriu, ou o motivo de nao abrir.
    pub fn abrir(&mut self, v: &Voz, texto: &str, nivel: i64, oferta: bool, espontanea: bool) -> &'static str {
        let motivo = self.impedida(v);
        if !motivo.is_empty() {
            return motivo;
        }
        *self = Sessao { ativa: true, nivel: nivel.clamp(1, 3), estado: "pensando", oferta, espontanea, ..Default::default() };
        (v.abrir_orbe)();
        self.turno(v, texto, false);
        ""
    }

    fn turno(&mut self, v: &Voz, texto: &str, fechar: bool) {
        let texto = &crate::tratamento::alma_dono(texto);
        self.historico.push(('a', texto.to_string()));
        let n = self.historico.len();
        if n > 30 {
            self.historico.drain(..n - 30);
        }
        self.legenda = texto.to_string(); // a legenda aparece mesmo sem voz
        am::lembrar_fala(v.alma.dados, "alma", texto, &v.alma.agora);
        (v.dizer)(&sem_emoji(texto));
        self.estado = if mudo::CORTAR.load(Ordering::SeqCst) { "interrompida" } else { "idle" };
        if fechar || !self.ativa {
            return self.fechar(v, "");
        }
        if self.estado != "interrompida" {
            self.estado = "ouvindo";
        }
        self.espera_ate = v.relogio + ESPERA_S;
    }

    /// Passou o prazo sem resposta: ela se recolhe. true se fechou.
    pub fn expirar(&mut self, v: &Voz) -> bool {
        let fecha = self.ativa && v.relogio >= self.espera_ate;
        if fecha {
            self.fechar(v, "sem resposta");
        }
        fecha
    }

    /// Uma frase da pessoa (microfone, pela escuta, ou digitada na esfera). Devolve o que a Alma respondeu.
    pub fn ouvir(&mut self, v: &Voz, texto: &str) -> Option<String> {
        if !self.ativa {
            return None;
        }
        let d = v.alma.dados;
        self.respondeu = true;
        self.historico.push(('u', texto.to_string()));
        self.estado = "pensando";
        am::contar_iniciativa(d, false);
        let (ap, p) = (v.apelido(), am::params(d));
        if mudo::comando(texto, mudo::ativo(d, v.relogio)) == Some("mutar") {
            mudo::mutar(d, None, v.relogio);
            self.fechar(v, "mudo");
            self.legenda = "Mudo ligado: não falo em voz alta.".into();
            return None;
        }
        let limpo = texto.trim();
        let (resp, fechar) = if rx(am::OCUPADO).is_match(texto) {
            am::pausar(d, p["pausa_ocupado_h"].as_f64().unwrap_or(3.0), "ele disse que está ocupado", &v.alma.agora);
            (format!("Tá bom, {ap}. Fico quietinha por aqui; quando quiser é só me chamar."), true)
        } else if rx(FIM).is_match(texto) {
            (format!("Tô por aqui, {ap}."), true)
        } else if self.oferta && rx(SIM).is_match(limpo) {
            self.oferta = false;
            (ajudar(v), false)
        } else if self.oferta && rx(NAO).is_match(limpo) {
            self.oferta = false;
            (format!("Tudo bem, {ap}, não mexo em nada. Só não esquece de respirar um pouco."), false)
        } else {
            (am::responder(v.alma, texto), false)
        };
        self.turno(v, &resp, fechar);
        Some(resp)
    }

    /// Botao da esfera: corta a fala dela na hora (o mesmo corte do mudo, sem deixar o mudo ligado).
    pub fn interromper(&mut self) {
        mudo::CORTAR.store(true, Ordering::SeqCst);
        self.estado = "interrompida";
    }

    pub fn fechar(&mut self, v: &Voz, _motivo: &str) {
        if !self.ativa {
            return;
        }
        if self.espontanea && !self.respondeu {
            am::contar_iniciativa(v.alma.dados, true); // sem resposta: conta para ela parar de insistir, nunca para alarme
        }
        (self.ativa, self.nivel, self.estado, self.oferta) = (false, 0, "idle", false);
        self.legenda.clear();
    }

    pub fn definir_nivel(&mut self, n: i64) {
        self.nivel = if self.ativa { n.clamp(0, 3) } else { 0 };
    }

    /// O que a esfera desenha. `falando` = o alto-falante esta tocando; `entrada`/`fala` = niveis de 0 a 1.
    pub fn estado_completo(&self, v: &Voz, falando: bool, entrada: f64, fala: f64, microfone: bool) -> Value {
        let est = if !self.ativa { "idle" } else if falando { "falando" } else { self.estado };
        let h: Vec<Value> = self.historico.iter().rev().take(8).rev().map(|(q, t)| json!([q.to_string(), t])).collect();
        json!({ "ativa": self.ativa, "nivel": self.nivel, "estado": est, "legenda": self.legenda, "historico": h,
            "entrada": if est == "ouvindo" { entrada } else { 0.0 }, "fala": fala, "mudo": mudo::ativo(v.alma.dados, v.relogio), "microfone": microfone })
    }
}

/// A pessoa aceitou ajuda: ela olha o escritorio e diz o que esta travado. Se algum agente esta com ERRO e ha time de programacao, abre UM
/// ticket por dia (o time so propoe; nada e aplicado sem o dono). Nao inventa capacidade: sem nada travado, diz isso.
pub fn ajudar(v: &Voz) -> String {
    let retrato = (v.retrato)().unwrap_or_default();
    let Some(ags) = retrato["agentes"].as_array() else {
        return "Tentei olhar o escritório agora e não consegui. Não vou chutar: te digo assim que der pra ver.".into();
    };
    let nomes = |estado: &str| ags.iter().filter(|a| a["estado"] == estado).filter_map(|a| a["exibir"].as_str()).collect::<Vec<_>>();
    let (erro, espera) = (nomes("erro"), nomes("aguardando_autorizacao"));
    let rodando = ags.iter().filter(|a| ["executando", "analisando", "planejando", "consultando"].contains(&a["estado"].as_str().unwrap_or(""))).count();
    let tres = |l: &[&str]| l.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
    let mut partes = Vec::new();
    if !erro.is_empty() {
        partes.push(format!("{} {} com erro", tres(&erro), if erro.len() == 1 { "está" } else { "estão" }));
    }
    if !espera.is_empty() {
        partes.push(format!("{} {} uma autorização sua", tres(&espera), if espera.len() == 1 { "espera" } else { "esperam" }));
    }
    if partes.is_empty() {
        return format!("Olhei o escritório: não tem nada travado nem esperando você agora. {rodando} agentes estão trabalhando sozinhos, então isso não precisa ficar na sua cabeça.");
    }
    let mut fala = format!("Olhei o escritório: {}.", partes.join(" e "));
    if let (Some(primeiro), Some(abrir)) = (erro.first(), v.abrir_ticket) {
        let hoje = v.alma.agora.data.iso();
        let pode = ac::mudar(v.alma.dados, |e| {
            let pode = e["tickets_alma"]["dia"] != hoje.as_str();
            if pode {
                e["tickets_alma"] = json!({ "dia": hoje });
            }
            pode
        });
        if pode && abrir("braniac", &format!("Verificar erro do agente {primeiro}"),
            "Pedido da Alma com o OK do dono por voz: investigar por que o agente aparece com erro no escritório e propor a correção.") {
            fala += &format!(" Abri um ticket no NexoDev pro time olhar o erro de {primeiro}; eles só propõem, quem aprova é você.");
        }
    }
    if !espera.is_empty() {
        fala += " A autorização eu não dou por você: quando parar um minuto, é só olhar.";
    }
    fala
}

fn pn(p: &Value, k: &str) -> i64 {
    p[k].as_f64().unwrap_or(0.0) as i64
}

fn ligado(p: &Value, k: &str) -> bool {
    p[k] == true
}

/// Pura. Nivel (0 = nao aparece) para a sobrecarga de agora. Nao basta o indice: a pessoa precisa estar no PC AGORA, dentro do horario, sem
/// pedido de silencio, com teto diario e intervalo respeitados, e o estado precisa durar (15 min; 30 min para o nivel de atencao).
pub fn decidir(av: &Value, p: &Value, agora: &Agora, quieta: &str, usos: &Value) -> i64 {
    let t = ts(agora);
    if !(ligado(p, "voz_iniciativa") && ligado(p, "iniciativa_ligada")) || !quieta.is_empty() || !(pn(p, "voz_hora_min")..=pn(p, "voz_hora_max")).contains(&(agora.hora as i64)) {
        return 0;
    }
    let hoje = if usos["dia"] == agora.data.iso().as_str() { pn(usos, "n") } else { 0 };
    if hoje >= pn(p, "voz_dia") || t - pn(usos, "ultima") < pn(p, "voz_intervalo_min") * 60 {
        return 0;
    }
    if ![atividade::HUMANA, atividade::SUPERVISIONADA].contains(&av["resumo"]["agora"].as_str().unwrap_or("")) {
        return 0;
    }
    let dura = |nome: &str| av["desde"][nome].as_f64().map_or(-1, |d| (t - d as i64) / 60);
    let n = if dura("HIGH") >= 30 { 3 } else if dura("ELEVATED") >= 15 { 2 } else if dura("ATTENTION") >= 15 { 1 } else { 0 };
    n.min(pn(p, "voz_nivel_max"))
}

/// 23h e 2h da manha sao a mesma noite.
fn noite(agora: &Agora) -> String {
    Data(agora.data.0 - (agora.hora < 6) as i64).iso()
}

/// Pura. true = hora de chamar para descansar: passou de `tarde_hora` (ate as 5h) e a PESSOA esta mexendo no PC (teclado/mouse em pelo menos
/// 20 dos ultimos 30 min). PC ligado com agentes trabalhando nao conta. Teto por noite e intervalo; silencio pedido e respeitado.
pub fn tarde(r: &Value, p: &Value, agora: &Agora, quieta: &str, usos: &Value) -> bool {
    if !(ligado(p, "tarde_ligado") && ligado(p, "iniciativa_ligada")) || !quieta.is_empty() || !(agora.hora as i64 >= pn(p, "tarde_hora") || agora.hora < 5) {
        return false;
    }
    if pn(r, "presente_30min") < 20 {
        return false;
    }
    if (if usos["noite"] == noite(agora).as_str() { pn(usos, "n") } else { 0 }) >= pn(p, "tarde_vezes") {
        return false;
    }
    ts(agora) - pn(usos, "ultima") >= pn(p, "tarde_intervalo_min") * 60
}

/// Pura. Texto fixo; o unico numero e a hora de agora.
pub fn fala_tarde(agora: &Agora, ap: &str, vez: i64) -> String {
    let (ap, hm) = (maiuscula(ap), agora.hm());
    if vez > 0 {
        format!("{ap}, ainda por aqui? Já são {hm}. Fecha por hoje, vai. Seu corpo precisa dormir.")
    } else {
        format!("{ap}, já são {hm} e você ainda está no computador. Que tal parar por hoje e descansar? Amanhã você continua com a cabeça mais leve.")
    }
}

fn dura(minutos: i64) -> String {
    let (h, m) = (minutos / 60, minutos % 60);
    if h == 0 {
        return format!("{m} minutos");
    }
    format!("{h} hora{}{}", if h != 1 { "s" } else { "" }, if m != 0 { format!(" e {m}") } else { String::new() })
}

/// Pura. Bom dia da Alma quando a pessoa chega no PC. Os unicos numeros sao as horas que o monitor viu e a recomendacao (7 a 9 h para adulto).
pub fn fala_manha(ap: &str, dormiu: Option<&Value>, minimo_h: i64) -> String {
    let ap = maiuscula(ap);
    let Some(d) = dormiu else { return format!("Bom dia, {ap}! Como você dormiu essa noite?") };
    let min = pn(d, "min");
    if min < minimo_h * 60 {
        return format!("Bom dia, {ap}! Você parou no computador às {} e voltou às {}: dá no máximo {} de sono. \
            O recomendado para adulto é de 7 a 9 horas por noite. Como você tá, não tá cansado? Minha sugestão: encaixa uma pausa depois do almoço e deita mais cedo hoje.",
            d["parou"].as_str().unwrap_or(""), d["voltou"].as_str().unwrap_or(""), dura(min));
    }
    format!("Bom dia, {ap}! Pelo que vi, você ficou umas {} longe do computador essa noite. Acordou descansado?", dura(min))
}

/// Pura. (texto, oferta) da primeira fala. Texto fixo e so com numero que veio do monitor: aqui o modelo nao escreve nada.
pub fn abordagem(nivel: i64, r: &Value, ap: &str) -> (String, bool) {
    let horas = ((pn(r, "min_humano") + pn(r, "min_supervisionado")) as f64 / 60.0).round() as i64;
    let ap = maiuscula(ap);
    match nivel {
        3.. => (format!("{ap}, para um minuexemplo comigo. Já são umas {horas} horas de trabalho hoje, quase sem pausa. Quer que eu veja o que dá pra tirar das suas costas?"), true),
        2 => (format!("{ap}, posso falar uma coisinha? Percebi que você está há bastante tempo trabalhando e resolvendo várias coisas ao mesmo tempo. Quer que eu veja o que posso assumir para aliviar um pouco?"), true),
        _ => (format!("{ap}, posso falar uma coisinha? Você está há bastante tempo trabalhando. Como estão as coisas por aí?"), false),
    }
}

/// 1x por minuto: amostra a atividade, avalia a sobrecarga e decide (ver `tick_com`). Sem amostra (monitor desligado, Alma pausada): nada.
pub fn tick(v: &Voz, sessao: &mut Sessao, l: &Leitura) -> Vec<String> {
    let (d, agora) = (v.alma.dados, &v.alma.agora);
    if atividade::amostrar(d, agora, l).is_none() {
        return Vec::new();
    }
    let av = sobrecarga::avaliar(d, agora, true);
    tick_com(v, sessao, &av)
}

/// Com a sobrecarga `av` ja avaliada: bom dia por voz, chamada para descansar, esfera no nivel certo e o aviso discreto da rede de apoio.
pub fn tick_com(v: &Voz, sessao: &mut Sessao, av: &Value) -> Vec<String> {
    let (d, agora) = (v.alma.dados, &v.alma.agora);
    let (p, e, hoje, t) = (am::params(d), ac::estado(d), agora.data.iso(), ts(agora));
    let (quieta, ap) = (am::quieta(d, agora), v.apelido());
    let mut fez = Vec::new();
    if ligado(&p, "manha_ligado") && ligado(&p, "iniciativa_ligada") && (5..12).contains(&agora.hora) && e["manha_dia"] != hoje.as_str()
        && pn(&av["resumo"], "presente_30min") >= 2 && quieta.is_empty() && sessao.impedida(v).is_empty() {
        // chegou no PC: bom dia por voz aqui, nao no aplicativo de mensagem
        let dormiu = atividade::sono(&atividade::linhas(d, agora, 18), agora);
        if sessao.abrir(v, &fala_manha(&ap, dormiu.as_ref(), pn(&p, "sono_minimo_h")), 1, false, true).is_empty() {
            ac::mudar(d, |e| {
                e["manha_dia"] = json!(hoje);
                if let Some(Value::Object(s)) = &dormiu {
                    let mut s = s.clone();
                    s.insert("dia".into(), json!(hoje));
                    e["sono"] = Value::Object(s);
                }
            });
            return vec!["voz:manha".into()];
        }
    }
    let ut = &e["tarde_usos"];
    if tarde(&av["resumo"], &p, agora, &quieta, ut) && sessao.impedida(v).is_empty() {
        let n = noite(agora);
        let vez = if ut["noite"] == n.as_str() { pn(ut, "n") } else { 0 };
        if sessao.abrir(v, &fala_tarde(agora, &ap, vez), 1, false, true).is_empty() {
            ac::mudar(d, |e| e["tarde_usos"] = json!({ "noite": n, "n": vez + 1, "ultima": t }));
            return vec!["voz:tarde".into()];
        }
    }
    let usos = &e["voz_usos"];
    let nivel = decidir(av, &p, agora, &quieta, usos);
    if nivel > 0 && sessao.impedida(v).is_empty() {
        let (texto, oferta) = abordagem(nivel, &av["resumo"], &ap);
        if sessao.abrir(v, &texto, nivel, oferta, true).is_empty() {
            let n = if usos["dia"] == hoje.as_str() { pn(usos, "n") } else { 0 } + 1;
            ac::mudar(d, |e| e["voz_usos"] = json!({ "dia": hoje, "n": n, "ultima": t }));
            let _ = barramento::enviar(d, "Alma", "Jarvis", "status",
                &format!("abri a esfera de voz (nível {nivel}): sobrecarga {}, índice {}", av["estado"].as_str().unwrap_or(""), av["indice"]), 0, None);
            fez.push(format!("voz:nivel{nivel}"));
        }
    }
    if rede_apoio::avisar_sobrecarga(v.alma, av) {
        fez.push("sobrecarga->parceira".into());
    }
    fez
}

/// Botoes da esfera e do painel (so o PC alcanca). O botao do microfone (ligar/desligar a escuta) fica com quem liga.
pub fn acao(v: &Voz, sessao: &mut Sessao, d: &Value) {
    match d["acao"].as_str().unwrap_or("") {
        // clicou na esfera: conversa pedida pela pessoa (nao conta como iniciativa)
        "abrir" if !sessao.ativa => {
            let n = d["nivel"].as_i64().filter(|n| *n != 0).unwrap_or(2);
            sessao.abrir(v, &format!("Oi, {}. Tô aqui.", v.apelido()), n, false, false);
        }
        "nivel" => sessao.definir_nivel(d["nivel"].as_i64().filter(|n| *n != 0).unwrap_or(1)),
        "interromper" if sessao.ativa => sessao.interromper(),
        "dispensar" if sessao.ativa => {
            sessao.interromper();
            sessao.fechar(v, "dispensada");
        }
        // modo silencioso temporario (minutos); 0 = pode voltar a falar
        "silencio" => {
            let min = d["minutos"].as_f64().unwrap_or(0.0);
            am::pausar(v.alma.dados, min / 60.0, "silêncio pedido na esfera", &v.alma.agora);
            if min != 0.0 && sessao.ativa {
                sessao.interromper();
                sessao.fechar(v, "silêncio");
            }
        }
        "texto" if sessao.ativa => {
            if let Some(t) = d["texto"].as_str().filter(|t| !t.is_empty()) {
                sessao.ouvir(v, &t.chars().take(300).collect::<String>());
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alma_cuida::teste::*;
    use std::cell::RefCell;
    use std::path::Path;

    struct Fora {
        ditas: RefCell<Vec<String>>,
        orbe: RefCell<u32>,
        tickets: RefCell<Vec<String>>,
        retrato: Value,
    }

    fn fora(retrato: Value) -> Fora {
        Fora { ditas: RefCell::new(Vec::new()), orbe: RefCell::new(0), tickets: RefCell::new(Vec::new()), retrato }
    }

    macro_rules! voz {
        ($v:ident, $d:expr, $agora:expr, $f:expr, $jarvis:expr, $chamada:expr) => {
            let nomes = nomes();
            let caixa: Caixa = RefCell::new(Vec::new());
            let envia = anota(&caixa);
            let llm = |_: &str| Some("Tô te ouvindo, pode falar comigo.".to_string());
            let alma = Alma { dados: $d, nomes: &nomes, agora: $agora, enviar: &envia, audio: None, llm: Some(&llm) };
            let (dizer, orbe) = (|t: &str| $f.ditas.borrow_mut().push(t.to_string()), || *$f.orbe.borrow_mut() += 1);
            let retrato = || if $f.retrato.is_null() { Err("fora do ar".to_string()) } else { Ok($f.retrato.clone()) };
            let ticket = |p: &str, t: &str, _: &str| {
                $f.tickets.borrow_mut().push(format!("{p}|{t}"));
                true
            };
            let $v = Voz { alma: &alma, relogio: 5000.0, dizer: &dizer, jarvis_em_conversa: $jarvis, em_chamada: $chamada, abrir_orbe: &orbe,
                retrato: &retrato, abrir_ticket: Some(&ticket), semente: 0 };
        };
    }

    fn limpa(d: &Path) {
        mudo::CORTAR.store(false, Ordering::SeqCst);
        let _ = std::fs::remove_dir_all(d);
    }

    // um teste so para tudo que mexe na bandeira CORTAR (ela e global)
    #[test]
    fn sessao_propria_fala_escuta_e_nao_disputa_o_microfone() {
        let d = dir("alma-voz-sessao");
        let f = fora(json!({ "agentes": [] }));
        voz!(v, &d, em(16, 15, 0), f, false, false);
        let mut s = Sessao::default();
        assert_eq!(s.abrir(&v, "Davi, posso falar uma coisinha? 💜", 1, false, true), "");
        assert!(*f.orbe.borrow() == 1 && *f.ditas.borrow() == ["Davi, posso falar uma coisinha?"]); // abre a PROPRIA janela; emoji nao se fala
        assert!(s.estado == "ouvindo" && s.nivel == 1 && s.espera_ate == 5045.0);
        assert!(s.abrir(&v, "de novo", 1, false, true).contains("já existe")); // nunca duas sessoes
        let r = s.ouvir(&v, "tô bem, só cansado").unwrap();
        let e = s.estado_completo(&v, false, 0.3, 0.0, true);
        assert!(e["legenda"] == r.as_str() && e["historico"].as_array().unwrap().iter().map(|h| h[0].as_str().unwrap()).collect::<String>() == "aua");
        assert!(e["entrada"] == 0.3 && s.estado_completo(&v, true, 0.3, 0.4, true)["estado"] == "falando");
        for n in [2, 3, 1, 9] {
            s.definir_nivel(n);
            assert_eq!(s.nivel, n.min(3));
        }
        s.interromper(); // ele corta a fala
        assert!(mudo::CORTAR.load(Ordering::SeqCst) && s.estado_completo(&v, false, 0.0, 0.0, true)["estado"] == "interrompida");
        mudo::CORTAR.store(false, Ordering::SeqCst);
        s.fechar(&v, "");
        assert!(!s.ativa && s.nivel == 0 && s.ouvir(&v, "oi").is_none());
        assert_eq!(ac::estado(&d)["sem_resposta"].as_i64().unwrap_or(0), 0); // ele respondeu: nao conta como ignorada
        // sem resposta em 45 s: fecha e conta 1 (para ela parar de insistir)
        s.abrir(&v, "Oi?", 1, false, true);
        assert!(!s.expirar(&v));
        s.espera_ate = 4000.0;
        assert!(s.expirar(&v) && !s.ativa && ac::estado(&d)["sem_resposta"] == 1);
        // conversa pedida por ele nao conta como iniciativa
        s.abrir(&v, "Oi", 2, false, false);
        s.fechar(&v, "");
        assert_eq!(ac::estado(&d)["sem_resposta"], 1);
        // ocupado: fecha e ela fica quieta
        s.abrir(&v, "Posso falar?", 1, false, true);
        let r = s.ouvir(&v, "agora não, tô ocupado").unwrap();
        assert!(r.contains("quietinha") && !s.ativa && am::quieta(&d, &v.alma.agora) == "ele disse que está ocupado");
        // "tchau" fecha; "silencio" liga o mudo e fecha sem falar
        s.abrir(&v, "Oi", 1, false, false);
        assert!(s.ouvir(&v, "era só isso, tchau").unwrap().starts_with("Tô por aqui") && !s.ativa);
        s.abrir(&v, "Oi", 1, false, false);
        let antes = f.ditas.borrow().len();
        assert!(s.ouvir(&v, "silêncio").is_none() && !s.ativa && mudo::ativo(&d, 5000.0) && f.ditas.borrow().len() == antes);
        assert_eq!(s.abrir(&v, "oi", 1, false, true), "mudo ligado");
        mudo::desmutar(&d, 5000.0);
        // Jarvis em conversa, reuniao e Braniac pausado: ela nao abre
        {
            voz!(v2, &d, em(16, 15, 0), f, true, false);
            assert!(Sessao::default().abrir(&v2, "oi", 1, false, true).contains("Jarvis"));
        }
        {
            voz!(v3, &d, em(16, 15, 0), f, false, true);
            assert!(Sessao::default().abrir(&v3, "oi", 1, false, true).contains("reunião"));
        }
        power::desligar(&d, "teste", "");
        assert_eq!(Sessao::default().impedida(&v), "Braniac pausado");
        assert_eq!(f.ditas.borrow().len(), antes);
        let e = |n: &str, i, f| (n.to_string(), i, f);
        assert!(em_uso(&[e("C:#Zoom#Zoom.exe", 5, 0)], "braniac") && !em_uso(&[e("C:#app#Braniac.exe", 5, 0), e("Teams", 5, 9), e("X", 0, 0)], "braniac"));
        limpa(&d);
    }

    #[test]
    fn ajuda_so_diz_o_que_existe_e_delega_com_o_sim_um_ticket_por_dia() {
        let d = dir("alma-voz-ajuda");
        let f = fora(json!({ "agentes": [{ "exibir": "Forja", "estado": "erro" }, { "exibir": "Cofre", "estado": "aguardando_autorizacao" }, { "exibir": "Ada", "estado": "executando" }] }));
        voz!(v, &d, em(16, 15, 0), f, false, false);
        let mut s = Sessao::default();
        s.abrir(&v, "Quer que eu veja o que posso assumir?", 2, true, true);
        assert!(s.ouvir(&v, "não precisa").unwrap().contains("não mexo em nada") && f.tickets.borrow().is_empty()); // sem o sim, nada e delegado
        assert!(!s.oferta);
        s.fechar(&v, "");
        s.abrir(&v, "Quer que eu veja?", 2, true, true);
        let fala = s.ouvir(&v, "pode sim").unwrap();
        assert!(fala.contains("Forja está com erro") && fala.contains("Cofre espera uma autorização sua") && fala.contains("A autorização eu não dou por você"));
        assert!(fala.contains("Abri um ticket") && *f.tickets.borrow() == ["braniac|Verificar erro do agente Forja"]);
        assert!(!ajudar(&v).contains("Abri um ticket") && f.tickets.borrow().len() == 1); // no maximo 1 ticket da Alma por dia
        {
            voz!(amanha, &d, em(17, 9, 0), f, false, false);
            assert!(ajudar(&amanha).contains("Abri um ticket"));
        }
        let nada = fora(json!({ "agentes": [{ "exibir": "Ada", "estado": "executando" }] }));
        {
            voz!(v2, &d, em(16, 15, 0), nada, false, false);
            let r = ajudar(&v2);
            assert!(r.contains("não tem nada travado") && r.contains("1 agentes") && nada.tickets.borrow().is_empty());
        }
        let caido = fora(Value::Null);
        {
            voz!(v3, &d, em(16, 15, 0), caido, false, false);
            assert!(ajudar(&v3).starts_with("Tentei olhar"));
        }
        limpa(&d);
    }

    fn av(estado: &str, minutos: i64, cat: &str, agora: &Agora) -> Value {
        let ordem = ["ATTENTION", "ELEVATED", "HIGH"];
        let i = ordem.iter().position(|x| *x == estado).unwrap();
        let desde: serde_json::Map<String, Value> = ordem[..=i].iter().map(|n| (n.to_string(), json!(ts(agora) - minutos * 60))).collect();
        json!({ "estado": estado, "indice": 80, "desde": desde, "resumo": { "agora": cat, "min_humano": 420, "presente_30min": 30 } })
    }

    #[test]
    fn presenca_exige_duracao_horario_teto_e_a_pessoa_no_pc() {
        let (p, a, n) = (am::padrao(), em(16, 15, 0), Value::Null);
        let h = atividade::HUMANA;
        assert_eq!(decidir(&av("ATTENTION", 20, h, &a), &p, &a, "", &n), 1);
        assert_eq!(decidir(&av("ELEVATED", 20, h, &a), &p, &a, "", &n), 2);
        assert_eq!(decidir(&av("HIGH", 40, h, &a), &p, &a, "", &n), 3);
        assert_eq!(decidir(&av("HIGH", 20, h, &a), &p, &a, "", &n), 2); // alta, mas ainda nao persistente: nao toma a tela
        assert_eq!(decidir(&av("HIGH", 5, h, &a), &p, &a, "", &n), 0); // pico de 5 min nao e motivo
        assert_eq!(decidir(&av("HIGH", 40, atividade::AUTONOMA, &a), &p, &a, "", &n), 0); // a pessoa nem esta no PC
        assert_eq!(decidir(&av("HIGH", 40, h, &a), &p, &a, "silencio", &n), 0);
        let tarde_ = em(16, 23, 0);
        assert_eq!(decidir(&av("HIGH", 40, h, &tarde_), &p, &tarde_, "", &n), 0); // fora do horario
        assert_eq!(decidir(&av("HIGH", 40, h, &a), &p, &a, "", &json!({ "dia": "2026-11-16", "n": 3, "ultima": 0 })), 0); // teto do dia
        assert_eq!(decidir(&av("HIGH", 40, h, &a), &p, &a, "", &json!({ "dia": "2026-11-16", "n": 1, "ultima": ts(&a) - 600 })), 0); // intervalo
        let mut q = p.clone();
        q["voz_iniciativa"] = json!(false);
        assert_eq!(decidir(&av("HIGH", 40, h, &a), &q, &a, "", &n), 0); // desligavel
        q = p.clone();
        q["voz_nivel_max"] = json!(1);
        assert_eq!(decidir(&av("HIGH", 40, h, &a), &q, &a, "", &n), 1);
        let (t, oferta) = abordagem(2, &json!({ "min_humano": 420 }), "querido");
        assert!(t.starts_with("Querido, posso falar uma coisinha?") && oferta && am::inventado(&t, "", &[]).is_empty());
        assert!(abordagem(3, &json!({ "min_humano": 420 }), "senhor").0.contains("7 horas") && !abordagem(1, &n, "senhor").1);
    }

    #[test]
    fn tarde_so_com_a_pessoa_mexendo_no_pc() {
        let (p, noite_, n) = (am::padrao(), em(16, 23, 40), Value::Null);
        let r = json!({ "presente_30min": 28 });
        assert!(tarde(&r, &p, &noite_, "", &n));
        assert!(!tarde(&json!({ "presente_30min": 0 }), &p, &noite_, "", &n)); // PC ligado rodando coisas sozinho nao conta
        assert!(!tarde(&r, &p, &em(16, 21, 40), "", &n)); // ainda nao e tarde
        assert!(tarde(&r, &p, &em(17, 2, 0), "", &n)); // madrugada conta
        assert!(!tarde(&r, &p, &em(17, 5, 30), "", &n));
        assert!(!tarde(&r, &p, &noite_, "silencio", &n));
        let mut q = p.clone();
        q["tarde_ligado"] = json!(false);
        assert!(!tarde(&r, &q, &noite_, "", &n));
        assert!(!tarde(&r, &p, &em(17, 1, 0), "", &json!({ "noite": "2026-11-16", "n": 2, "ultima": 0 }))); // teto da noite
        let f = fala_tarde(&noite_, "querido", 0);
        assert!(f.contains("23:40") && am::inventado(&f.replace("23:40", ""), "", &[]).is_empty() && fala_tarde(&noite_, "senhor", 1).starts_with("Senhor, ainda por aqui?"));
        assert_eq!((dura(45), dura(60), dura(125), dura(480)), ("45 minutos".into(), "1 hora".into(), "2 horas e 5".into(), "8 horas".into()));
        assert!(fala_manha("querido", None, 7).ends_with("Como você dormiu essa noite?"));
        let curto = fala_manha("querido", Some(&json!({ "min": 300, "parou": "02:10", "voltou": "07:10" })), 7);
        assert!(curto.contains("02:10") && curto.contains("no máximo 5 horas") && curto.contains("7 a 9 horas"));
        assert!(fala_manha("senhor", Some(&json!({ "min": 480, "parou": "23:00", "voltou": "07:00" })), 7).contains("umas 8 horas longe"));
    }

    #[test]
    fn tick_abre_uma_vez_registra_e_respeita_o_intervalo() {
        let d = dir("alma-voz-tick");
        let f = fora(json!({ "agentes": [] }));
        let a = em(16, 15, 0);
        {
            voz!(v, &d, a, f, false, false);
            let mut s = Sessao::default();
            assert_eq!(tick_com(&v, &mut s, &av("ELEVATED", 20, atividade::HUMANA, &a)), ["voz:nivel2"]);
            assert!(s.ativa && s.nivel == 2 && s.oferta && ac::estado(&d)["voz_usos"]["n"] == 1);
            s.fechar(&v, "");
            assert!(tick_com(&v, &mut s, &av("ELEVATED", 20, atividade::HUMANA, &a)).is_empty()); // intervalo: nao repete
        }
        // trabalhando ate tarde: chama para descansar, uma vez por intervalo
        let noite_ = em(16, 23, 40);
        let normal = json!({ "estado": "NORMAL", "indice": 0, "desde": {}, "resumo": { "presente_30min": 30, "agora": atividade::HUMANA } });
        {
            voz!(v, &d, noite_, f, false, false);
            ac::mudar(&d, |e| e["sem_resposta"] = json!(0));
            let mut s = Sessao::default();
            assert_eq!(tick_com(&v, &mut s, &normal), ["voz:tarde"]);
            assert!(f.ditas.borrow().last().unwrap().contains("descansar") && s.nivel == 1);
            s.ouvir(&v, "já vou");
            s.fechar(&v, "");
            assert!(tick_com(&v, &mut s, &normal).is_empty());
        }
        // bom dia por voz: 5h-12h, 1x por dia, so com a pessoa no PC
        {
            voz!(v, &d, em(17, 8, 0), f, false, false);
            let mut s = Sessao::default();
            assert_eq!(tick_com(&v, &mut s, &normal), ["voz:manha"]);
            assert!(f.ditas.borrow().last().unwrap().starts_with("Bom dia") && ac::estado(&d)["manha_dia"] == "2026-11-17");
            s.ouvir(&v, "dormi bem");
            s.fechar(&v, "");
            assert!(tick_com(&v, &mut s, &normal).is_empty()); // 1x por dia
        }
        {
            voz!(v, &d, em(18, 8, 0), f, false, false);
            let longe = json!({ "estado": "NORMAL", "desde": {}, "resumo": { "presente_30min": 0, "agora": atividade::PARADA } });
            assert!(tick_com(&v, &mut Sessao::default(), &longe).is_empty()); // PC ligado sem ele: nao da bom dia para ninguem
            // botoes da esfera
            let mut s = Sessao::default();
            acao(&v, &mut s, &json!({ "acao": "abrir" }));
            assert!(s.ativa && s.nivel == 2 && !s.espontanea);
            acao(&v, &mut s, &json!({ "acao": "nivel", "nivel": 3 }));
            assert_eq!(s.nivel, 3);
            acao(&v, &mut s, &json!({ "acao": "texto", "texto": "oi alma" }));
            assert_eq!(s.historico.len(), 3);
            acao(&v, &mut s, &json!({ "acao": "silencio", "minutos": 30 }));
            assert!(!s.ativa && am::quieta(&d, &v.alma.agora) == "silêncio pedido na esfera");
            acao(&v, &mut s, &json!({ "acao": "silencio", "minutos": 0 }));
            assert_eq!(am::quieta(&d, &v.alma.agora), "");
        }
        limpa(&d);
    }
}
