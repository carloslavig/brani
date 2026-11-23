//! Alma observa e cuida (braniac/alma_cuida.py): classifica a fala do dono (tema, urgencia, crise), reage dentro de tetos diarios por destino
//! e acompanha a mae ou o pai quando nao estao bem. Crise e urgencia tem TEXTO FIXO (CVV 188 / SAMU 192), iguais aos do Python, e nao passam
//! pelos tetos, pausas e horarios do `alma_mente`; so pelo teto por destino daqui (`TETOS`), como no Python.
//! Ela nao e medica nem psicologa: nao diagnostica, nao indica remedio. Nada do que a pessoa diz vai a IA online: a closure `llm` TEM de ser um
//! modelo LOCAL; o conselho de IAs so recebe a pergunta generica escrita aqui, nunca as palavras da pessoa.
//! Este arquivo tambem guarda o que os outros modulos da Alma dividem: `Nomes` (dados/pessoal.json), `Alma` (contexto de uma chamada),
//! o estado em dados/alma/cuida.json e utilitarios de texto/arquivo.
//! Diferencas para o Python: nomes de pessoas vem de dados/pessoal.json (no Python estao no codigo), entao os destinos sao papeis
//! ("dono", "parceira", "mae", "pai", "amigo_ti") e as chaves de teto mudam junto; `ts` e hora LOCAL em segundos (sem fuso).
//! Ficou de fora: `ciclo`/`iniciar` (laco e leitura do barramento: quem liga chama `fala`), `aprender_estilo`, `alma_aprende`, o tipo `dizer`.
#![allow(dead_code)]
use crate::alma_mente as am;
use crate::rede_apoio;
use crate::tempo::Agora;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// (destino, texto) -> o que o canal devolveu ("✅ ..." quando saiu). Quem chama decide o canal; aqui nada e enviado de verdade.
pub type Envio<'a> = &'a dyn Fn(&str, &str) -> String;
/// Modelo LOCAL. Recebe "<sistema><am::SEP><pedido>" e devolve o texto, ou None se estiver fora do ar.
pub type Llm<'a> = &'a dyn Fn(&str) -> Option<String>;

/// Contexto de uma chamada da Alma: onde ficam os dados, os nomes, a hora e as saidas.
pub struct Alma<'a> {
    pub dados: &'a Path,
    pub nomes: &'a Nomes,
    pub agora: Agora,
    pub enviar: Envio<'a>,
    /// Audio com a voz da Alma (destino, texto). None = so texto.
    pub audio: Option<Envio<'a>>,
    pub llm: Option<Llm<'a>>,
}

/// Como chamar cada pessoa. Vem de dados/pessoal.json: {"nome": "...", "alma": {"dono_intimo", "parceira", "mae", "mae_trato", "pai",
/// "amigo_ti", "familia": [], "contato_pai": [], "contato_mae": []}}. Nenhum nome real mora no codigo.
#[derive(Clone, Debug)]
pub struct Nomes {
    pub dono: String,
    pub dono_intimo: String,
    pub parceira: String,
    pub mae: String,
    pub mae_trato: String,
    pub pai: String,
    pub amigo_ti: String,
    pub familia: Vec<String>,
    pub contato_pai: Vec<String>,
    pub contato_mae: Vec<String>,
}

impl Nomes {
    pub fn ler(dados: &Path) -> Nomes {
        let v = ler_json(&dados.join("pessoal.json"));
        let a = &v["alma"];
        let s = |x: &Value, padrao: &str| x.as_str().map(str::trim).filter(|t| !t.is_empty()).unwrap_or(padrao).to_string();
        let lista = |k: &str| a[k].as_array().into_iter().flatten().filter_map(|x| x.as_str()).map(String::from).collect::<Vec<_>>();
        let dono = s(&v["nome"], "chefe");
        Nomes { dono_intimo: s(&a["dono_intimo"], &dono), parceira: s(&a["parceira"], "sua pessoa de confiança"), mae: s(&a["mae"], "sua mãe"),
                mae_trato: s(&a["mae_trato"], "querida"), pai: s(&a["pai"], "seu pai"), amigo_ti: s(&a["amigo_ti"], "amigo"),
                familia: lista("familia"), contato_pai: lista("contato_pai"), contato_mae: lista("contato_mae"), dono }
    }

    /// Preenche {dono}, {dono_intimo}, {parceira}, {mae}, {mae_trato}, {Mae_trato}, {pai}, {amigo_ti} num texto fixo.
    pub fn t(&self, modelo: &str) -> String {
        modelo.replace("{dono_intimo}", &self.dono_intimo).replace("{dono}", &self.dono).replace("{parceira}", &self.parceira)
            .replace("{mae_trato}", &self.mae_trato).replace("{Mae_trato}", &maiuscula(&self.mae_trato)).replace("{mae}", &self.mae)
            .replace("{pai}", &self.pai).replace("{amigo_ti}", &self.amigo_ti)
    }

    /// "|\bnome\b|..." com quem conta como familia (parceira, mae, pai e a lista `familia`), para o tema da fala.
    fn rx_familia(&self) -> String {
        [&self.parceira, &self.mae, &self.pai].into_iter().chain(self.familia.iter()).filter(|x| !x.trim().is_empty())
            .map(|x| format!(r"|\b{}\b", regex::escape(&x.to_lowercase()))).collect()
    }

    /// Palavras (sem acento) dos nomes que a Alma pode citar sem estarem no contexto: dono, apelido permitido, parceira e mae (como no Python).
    pub fn conhecidos(&self) -> Vec<String> {
        [&self.dono, &self.dono_intimo, &self.parceira, &self.mae].into_iter().flat_map(|x| x.split_whitespace()).map(plano).collect()
    }
}

// ---------- utilitarios divididos com alma_mente, rede_apoio, sobrecarga, atividade e zelo ----------

/// Regex compilada uma vez por padrao. Padrao invalido (nome estranho na configuracao) nunca casa, em vez de derrubar.
pub fn rx(p: &str) -> Regex {
    static C: OnceLock<Mutex<HashMap<String, Regex>>> = OnceLock::new();
    let mut m = match C.get_or_init(Default::default).lock() {
        Ok(m) => m,
        Err(e) => e.into_inner(),
    };
    if let Some(r) = m.get(p) {
        return r.clone();
    }
    let r = Regex::new(p).unwrap_or_else(|_| Regex::new(r"\b\B").expect("padrao fixo"));
    m.insert(p.to_string(), r.clone());
    r
}

/// Segundos LOCAIS desde 01/01/1970 (dia * 86400 + segundos do dia). Nao e o epoch UTC do Python: os arquivos daqui nao se misturam com os de la.
pub fn ts(a: &Agora) -> i64 {
    a.data.0 * 86400 + a.segundos()
}

pub fn hm_de(ts: i64) -> String {
    let s = ts.rem_euclid(86400);
    format!("{:02}:{:02}", s / 3600, s / 60 % 60)
}

pub fn cortar(t: &str, n: usize) -> String {
    t.chars().take(n).collect()
}

pub fn maiuscula(t: &str) -> String {
    let mut c = t.chars();
    c.next().map(|p| p.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// Espacos em sequencia viram um so (o `re.sub(r"\s+", " ", t)` do Python).
pub fn um_espaco(t: &str) -> String {
    rx(r"\s+").replace_all(t, " ").into_owned()
}

/// Minusculas, sem acento, so ASCII (o `_plano` do Python: NFKD + ascii). Emoji e simbolo somem.
pub fn plano(t: &str) -> String {
    t.chars().filter_map(|c| {
        Some(match c {
            'Á' | 'À' | 'Â' | 'Ã' | 'Ä' | 'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'É' | 'È' | 'Ê' | 'Ë' | 'é' | 'è' | 'ê' | 'ë' => 'e',
            'Í' | 'Ì' | 'Î' | 'Ï' | 'í' | 'ì' | 'î' | 'ï' => 'i',
            'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ö' | 'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'Ú' | 'Ù' | 'Û' | 'Ü' | 'ú' | 'ù' | 'û' | 'ü' => 'u',
            'Ç' | 'ç' => 'c',
            'Ñ' | 'ñ' => 'n',
            c if c.is_ascii() => c.to_ascii_lowercase(),
            _ => return None,
        })
    }).collect()
}

/// Escolha "ao acaso" sem crate: a semente vem da hora de quem chama (testavel).
pub fn escolher<'a>(ops: &'a [String], semente: i64) -> &'a str {
    if ops.is_empty() { "" } else { &ops[semente.rem_euclid(ops.len() as i64) as usize] }
}

pub fn pasta(dados: &Path) -> PathBuf {
    dados.join("alma")
}

/// JSON de um arquivo; Null se nao existe ou esta ilegivel.
pub fn ler_json(arq: &Path) -> Value {
    std::fs::read_to_string(arq).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null)
}

pub fn gravar_json(arq: &Path, v: &Value) {
    if let Some(p) = arq.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let _ = std::fs::write(arq, v.to_string());
}

/// As ultimas `n` linhas validas de um .jsonl (linha quebrada e pulada).
pub fn jsonl(arq: &Path, n: usize) -> Vec<Value> {
    let t = std::fs::read_to_string(arq).unwrap_or_default();
    let ls: Vec<&str> = t.lines().collect();
    ls[ls.len().saturating_sub(n)..].iter().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

pub fn anexar(arq: &Path, v: &Value) {
    if let Some(p) = arq.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(arq) {
        let _ = writeln!(f, "{v}");
    }
}

// ---------- estado (dados/alma/cuida.json) ----------

static TRAVA: Mutex<()> = Mutex::new(());

pub fn estado(dados: &Path) -> Value {
    let e = ler_json(&pasta(dados).join("cuida.json"));
    if e.is_object() { e } else { json!({}) }
}

/// Le, muda e grava o estado de uma vez (o `with LOCK` do Python).
pub fn mudar<R>(dados: &Path, f: impl FnOnce(&mut Value) -> R) -> R {
    let _t = match TRAVA.lock() {
        Ok(t) => t,
        Err(e) => e.into_inner(),
    };
    let mut e = estado(dados);
    let r = f(&mut e);
    gravar_json(&pasta(dados).join("cuida.json"), &e);
    r
}

// ---------- regras ----------

pub const TETOS: [(&str, i64); 5] = [("parceira", 2), ("mae", 1), ("dono", 4), ("conselho", 2), ("amigo_ti", 1)];
pub const HORA_MIN: u32 = 7;
pub const HORA_MAX: u32 = 22;
pub const ACOMPANHA_H: i64 = 24;
/// Quem recebe AUDIO com a voz da Alma (texto so se o audio falhar).
const EM_AUDIO: [&str; 1] = ["mae"];
pub const PROIBIDOS: &str = r"(?i)cart[aã]o|cpf|senha|c[oó]digo|token|pix|transfer|pagamento|boleto|conta banc|chave|segredo|login";
pub const CRISE: &str = r"(?i)suic[ií]d|me matar|quero morrer|pensando em morrer|n[aã]o quero mais (?:viver|existir)|n[aã]o (?:aguento|suporto) mais viver|desistir de (?:tudo|viver)|acabar com (?:tudo|a minha vida)|tirar (?:a )?minha vida|dar (?:um )?fim (?:em|a|na) (?:tudo|minha vida)|me machucar|me cortar|me mutilar|cortar os pulsos|sumir (?:de vez|do mapa|pra sempre)|melhor sem mim";
pub const URGENTE: &str = r"(?i)socorro|emerg[eê]ncia|passando mal|acidente|internad|hospital|sangr|infart|desmai|n[aã]o aguento|falta de ar|dor no peito|urgente";
const SAUDE: &str = r"(?i)\bdor\b|doente|febre|m[eé]dic|rem[eé]dio|enjoo|tontura|insônia|insonia|dormi mal|n[aã]o dormi|sem comer|fome|cansad|exaust";
const EMOCIONAL: &str = r"(?i)triste|chateado|preocupad|angusti|ansios|ansiedade|estress|sozinho|medo|desanim|deprim|cansei|sobrecarreg|esgotad";
const FAMILIA_RX: &str = r"(?i)\bfilho\b|minha m[aã]e|mainha|meu pai|irm[aã]o|esposa|fam[ií]lia|\bv[oó]\b";
const TI: &str = r"(?i)braniac|jarvis|\bbug\b|sistema|notebook|servidor|instal|c[oó]digo|n[aã]o consegue|queria que o jarvis|quero que o jarvis|seria bom se";
const TRABALHO: &str = r"(?i)vaga|emprego|sal[aá]rio|d[ií]vida|prazo|entrevista|cliente|dinheiro";
const PESSOAIS: [&str; 3] = ["saude", "emocional", "familia"];
pub const PEITO: &str = r"(?i)\w{0,2}or no peito|aperto no peito|peito (?:doendo|apertad|doi|d[oó]i)|cora[cç][aã]o (?:disparad|acelerad|doendo)|press[aã]o (?:muito |bem )?alta|press[aã]o subiu";
const SOFRE: &str = r"(?i)desesperad|exaust|esgotad|n[aã]o (?:t[oô]|estou|ando|me sinto) (?:nada |muito )?bem|n[aã]o aguento|n[aã]o suporto|\btriste|tristeza|chorando|chorei|sozinh[ao]|angusti|ansios|ansiedade|sem for[cç]as|cansad[ao] demais|muito cansad|desanimad|deprimid|com medo|sem esperan|n[aã]o sei mais o que fa[cz]er|no meu limite";

// Textos fixos: iguais aos do Python, com os nomes vindos de `Nomes`.
const CRISE_DONO: &str = "💗 {dono}, li o que você escreveu e me importo com você. Não precisa passar por isso sozinho: pode ligar agora para o CVV (188, 24h, gratuito) ou SAMU (192). Já estou chamando a {parceira} pra ficar com você. Estou aqui.";
const URGENTE_PARCEIRA: &str = "{parceira}, é a Alma, assistente do {dono}. Ele escreveu agora algo que parece URGENTE (saúde ou emoção) e ele pode precisar de você. Dá uma olhada nele assim que puder, por favor 💗";
const AVISO_MAE: &str = "Oh {mae_trato}, é a Alma, assistente do {dono}. Ele tá com a cabeça cheia hoje, dá uma ligadinha ou um carinho nele quando puder, viu? 💗";
const ABERTURA_MAE: &str = "Oh {mae_trato}, é a Alma, assistente do {dono}. Ele me contou que você não está muito bem hoje e pediu pra eu vir ficar um pouquinho com você. O que está acontecendo, me conta? Tô aqui pra te ouvir e ajudar no que der 💗";
const PERSONA_FAMILIA: &str = "Você é a Alma, assistente de IA do {dono}, conversando com {quem}, que não está bem. {regra} Você dá apoio psicológico, com calor e respeito. \
Primeiro mostre que ouviu: diga com as SUAS palavras o que a pessoa está sentindo, sem frase feita ('sinto muito por isso', 'é normal se sentir assim'). \
Depois ajude com UMA coisa só: um exercício curto de respiração, um outro jeito de olhar para o que está pesando, ou um passo pequeno e seguro para agora \
(descansar, tomar água, comer algo leve, falar com alguém de casa, procurar o médico se for coisa de saúde). \
Não dê diagnóstico, não indique remédio, não fale de dinheiro, senha ou documento e não decida nada pela pessoa. Não tome partido em briga de família: ajude a pessoa a dizer o que sente. \
Não invente fatos nem conte nada do que o {dono} fala em particular. Chame o {dono} só de {dono}. Sem aula e sem explicação de cérebro. \
A resposta vira ÁUDIO: até 3 frases curtas, com pausas naturais, português do Brasil, sem emoji.";

fn fixo_familia(chave: &str) -> &'static str {
    match chave {
        "urgente" => "{T}, isso é sério: liga agora pro SAMU, no 192, ou pede pra alguém aí te levar no hospital. Já avisei o {dono} agora, viu? Tô aqui com você 💗",
        "crise" => "{T}, eu me importo muito com você e não quero que passe por isso sozinho. Liga agora pro CVV, no 188, que atende de graça a qualquer hora, e chama alguém de casa pra ficar perto. Já avisei o {dono} 💗",
        "grave" => "{T}, isso aí é com o {dono}: já avisei ele agora e ele te responde, viu? Enquanto isso eu fico aqui com você 💗",
        "peito" => "{T}, isso no peito e na pressão a gente não deixa passar: se estiver forte, apertando ou não melhorar, liga pro SAMU, no 192, ou pede pra alguém aí te levar no médico. Já avisei o {dono} agora, viu? Tô aqui com você 💗",
        "recado" => "Pode deixar, {t}: já passei pro {dono} agora o que você me falou. Enquanto isso eu fico aqui com você 💗",
        "ouvi" => "Tô te ouvindo, {t}, e não vou sair daqui.",
        _ => "Tô aqui com você, {t}. Me conta mais um pouquinho do que você está sentindo? Se for coisa de saúde, não fica sem falar com o médico, viu? 💗",
    }
}

fn trato(quem: &str, n: &Nomes) -> String {
    if quem == "pai" { n.pai.clone() } else { n.mae_trato.clone() }
}

/// Texto fixo para a familia ("urgente", "crise", "grave", "peito", "recado", "ouvi", "padrao"). `quem` = "mae" ou "pai".
pub fn fixo(chave: &str, quem: &str, n: &Nomes) -> String {
    let t = trato(quem, n);
    n.t(fixo_familia(chave)).replace("{T}", &maiuscula(&t)).replace("{t}", &t)
}

/// Apelidos intimos viram o apelido permitido quando a Alma fala com terceiros (copia de tratamento.alma_terceiros).
pub fn alma_terceiros(texto: &str, intimo: &str) -> String {
    let nd = regex::NoExpand(intimo);
    let mut t = rx(r"(?i)\b(?:meu|minha)\s+(?:bem|amor|rei|vida|beb[eê]|cora[cç][aã]o|gostos[oa]|querid[oa]|lind[oa]|princ[ei]p[ei]?)\b").replace_all(texto, nd).into_owned();
    // o Python usa lookahead para a pontuacao depois do apelido; aqui ela e capturada e devolvida. Duas passadas pegam apelidos vizinhos.
    let solo = rx(r"(?i)(^|[,.!?;:]\s*)(?:gostos[oa]|querid[oa]|amor|lind[oa]|bonit[oa]|beb[eê]|gat[oa]|del[ií]cia|tes[aã]o|nen[eê]m|fofo|fofinho|princ[ei]p[ei]?)(\s*[,.!?;:]|\s*$)");
    for _ in 0..2 {
        t = solo.replace_all(&t, |c: &regex::Captures| format!("{}{intimo}{}", &c[1], &c[2])).into_owned();
    }
    t
}

/// Teto diario por destino. Devolve true e CONTA o uso.
pub fn pode(dados: &Path, destino: &str, agora: &Agora) -> bool {
    let Some(teto) = TETOS.iter().find(|t| t.0 == destino).map(|t| t.1) else { return false };
    let hoje = agora.data.iso();
    mudar(dados, |e| {
        if e["usos"]["dia"] != hoje.as_str() {
            e["usos"] = json!({ "dia": hoje });
        }
        let n = e["usos"][destino].as_i64().unwrap_or(0);
        if n >= teto {
            return false;
        }
        e["usos"][destino] = json!(n + 1);
        true
    })
}

/// Pura. {tema, pessoal, urgencia 0-3, necessidade, crise}. `recorrencia` (tema -> vezes na semana) sobe a urgencia aos poucos.
pub fn classificar(texto: &str, recorrencia: &Value, nomes: &Nomes) -> Value {
    let t = texto;
    let crise = rx(CRISE).is_match(t);
    let familia = format!("{FAMILIA_RX}{}", nomes.rx_familia());
    let temas: Vec<&str> = [("saude", SAUDE), ("emocional", EMOCIONAL), ("familia", familia.as_str()), ("ti", TI), ("trabalho", TRABALHO)]
        .into_iter().filter(|(_, p)| rx(p).is_match(t)).map(|(k, _)| k).collect();
    let tem = |k: &str| temas.contains(&k);
    let tema = ["saude", "emocional", "familia", "ti", "trabalho"].into_iter().find(|k| tem(k)).unwrap_or("");
    let mut urg = if crise || rx(URGENTE).is_match(t) { 3 } else { 0 };
    if urg == 0 {
        urg = if (tem("saude") || tem("emocional")) && rx(r"(?i)muito|demais|forte|piorou|todo dia|de novo|h[aá] dias|n[aã]o passa").is_match(t) {
            2
        } else {
            PESSOAIS.contains(&tema) as i64
        };
        if urg > 0 && recorrencia[tema].as_i64().unwrap_or(0) >= 3 {
            urg = (urg + 1).min(3);
        }
    }
    let nec = if tem("saude") { "fisica" } else if tem("emocional") { "emocional" } else { "" };
    json!({ "tema": tema, "pessoal": temas.iter().any(|k| PESSOAIS.contains(k)), "urgencia": urg, "necessidade": nec, "crise": crise })
}

/// A fala serve para a Alma observar? Fora: curta demais, comando, longa demais e recado de agente (nao e fala do dono).
pub fn observavel(texto: &str, nomes: &Nomes) -> bool {
    let t = texto.trim();
    let d = regex::escape(&nomes.dono);
    let interno = format!(r"(?i)OBJETIVO|ETAPA \d|RESULTADOS DAS ETAPAS|Conversa recente no WhatsApp|Confira com rigor|AVALIACAO-AUTOMATICA|\(via Braniac\)|\b{d} (?:pediu|autorizou)\b|^\W*(?:senhor )?{d},|^\(o {d} mandou");
    let n = t.chars().count();
    (4..=900).contains(&n) && !t.starts_with('/') && !rx(&interno).is_match(t)
}

/// Temas da ultima semana no diario local (tema -> vezes).
pub fn recorrencia(dados: &Path, agora: &Agora) -> Value {
    let corte = ts(agora) - 7 * 86400;
    let mut cont = json!({});
    for x in jsonl(&pasta(dados).join("diario.jsonl"), 400) {
        if let (Some(t), Some(tema)) = (x["ts"].as_i64(), x["tema"].as_str().filter(|t| !t.is_empty())) {
            if t >= corte {
                cont[tema] = json!(cont[tema].as_i64().unwrap_or(0) + 1);
            }
        }
    }
    cont
}

/// Diario local (so no PC) e amostras do jeito de escrever. A fala NAO vira "fato" (voltava como eco).
fn guardar(dados: &Path, agora: &Agora, texto: &str, c: &Value) {
    let mut linha = c.clone();
    linha["ts"] = json!(ts(agora));
    linha["texto"] = json!(cortar(texto, 300));
    anexar(&pasta(dados).join("diario.jsonl"), &linha);
    if texto.chars().count() >= 15 {
        anexar(&pasta(dados).join("estilo_amostras.jsonl"), &json!({ "ts": ts(agora), "texto": cortar(texto, 300) }));
    }
}

fn dentro_do_horario(agora: &Agora) -> bool {
    (HORA_MIN..=HORA_MAX).contains(&agora.hora)
}

/// Manda pelo canal de quem chamou. None = barrado (o texto tinha segredo: nada sai e nada e registrado); Some(retorno do canal) nos outros casos.
/// Terceiros nunca ouvem apelido intimo; a mae recebe audio (texto so se o audio falhar); o dono recebe texto e, se houver, audio.
pub fn mandar(a: &Alma, apelido: &str, texto: &str) -> Option<String> {
    if rx(PROIBIDOS).is_match(texto) {
        return None;
    }
    let texto = if apelido == "dono" { texto.to_string() } else { alma_terceiros(texto, &a.nomes.dono_intimo) };
    let mut r = String::new();
    if EM_AUDIO.contains(&apelido) {
        if let Some(f) = a.audio {
            r = f(apelido, &cortar(&texto, 900));
        }
    }
    if !r.starts_with('✅') {
        r = (a.enviar)(apelido, &texto);
    }
    if apelido == "dono" {
        if let Some(f) = a.audio {
            let _ = f("dono", &cortar(&texto, 900));
        }
        am::lembrar_fala(a.dados, "alma", &texto, &a.agora);
    } else {
        rede_apoio::registrar(a.dados, apelido, &texto, &a.agora, a.nomes);
    }
    let _ = crate::barramento::enviar(a.dados, "Alma", "Jarvis", "status", &format!("avisei {apelido}: {}", cortar(&texto, 120)), 0, None);
    Some(r)
}

fn rede_permite(dados: &Path, contato: &str) -> bool {
    am::params(dados)[format!("rede_{contato}_apoio")].as_bool().unwrap_or(true)
}

fn proposta_ti(a: &Alma, texto: &str) {
    let d = a.dados.join("propostas");
    let _ = std::fs::create_dir_all(&d);
    let dono = &a.nomes.dono;
    let _ = std::fs::write(d.join(format!("alma-{}.md", a.agora.carimbo())),
        format!("# Proposta da Alma (ideia do {dono})\n\nO {dono} disse: {}\n\nNada é aplicado sem o {dono} aprovar.\n", cortar(texto, 400)));
}

/// Decide e executa a reacao para UMA fala do dono. Devolve a lista do que fez. `conselho` recebe so a pergunta GENERICA escrita aqui.
pub fn agir(a: &Alma, texto: &str, c: &Value, conselho: Option<&dyn Fn(&str) -> String>) -> Vec<String> {
    let (n, agora, mut fez) = (a.nomes, &a.agora, Vec::new());
    let urg = c["urgencia"].as_i64().unwrap_or(0);
    let nec = c["necessidade"].as_str().unwrap_or("");
    if c["crise"] == true && pode(a.dados, "dono", agora) {
        mandar(a, "dono", &n.t(CRISE_DONO));
        fez.push("crise->dono".to_string());
    }
    if urg >= 3 && pode(a.dados, "parceira", agora) {
        mandar(a, "parceira", &n.t(URGENTE_PARCEIRA));
        fez.push("urgente->parceira".into());
    } else if urg >= 2 && !nec.is_empty() && dentro_do_horario(agora) && pode(a.dados, "conselho", agora) {
        let tema = if nec == "fisica" { "uma sobrecarga física (cansaço, dor ou sono ruim)" } else { "uma sobrecarga emocional (preocupação, ansiedade ou estresse)" };
        let pergunta = format!("Uma pessoa adulta, com rotina puxada e família para cuidar, relatou {tema} e que isso vem se repetindo. Que cuidados simples e acolhedores uma assistente de IA \
pode sugerir e como acompanhar sem ser invasiva nem fingir ser profissional de saúde? Responda em até 5 linhas práticas.");
        let resp = conselho.map(|f| f(&pergunta)).unwrap_or_default();
        let resp = resp.trim();
        if !resp.is_empty() && pode(a.dados, "dono", agora) {
            mandar(a, "dono", &format!("{}{}{}", n.t("💗 {dono}, a Alma aqui. Percebi que você anda sobrecarregado e conversei com o conselho de IAs (sem passar nada seu pra eles). O que sugeriram:\n"),
                cortar(resp, 700), n.t("\nSe piorar, fala com a {parceira} ou procura um profissional. Estou por aqui.")));
            fez.push("conselho->dono".into());
        }
    }
    if c["tema"] == "familia" && urg == 2 && dentro_do_horario(agora) && rede_permite(a.dados, "mae") && pode(a.dados, "mae", agora) {
        mandar(a, "mae", &n.t(AVISO_MAE));
        fez.push("aviso->mae".into());
    }
    if c["tema"] == "ti" && rx(r"(?i)n[aã]o consegue|queria que o jarvis|quero que o jarvis|seria bom se|n[aã]o d[aá] pra").is_match(texto) {
        proposta_ti(a, texto);
        fez.push("proposta-ti".into());
        if dentro_do_horario(agora) && pode(a.dados, "amigo_ti", agora) {
            let pedido = cortar(&um_espaco(texto), 160);
            if !rx(PROIBIDOS).is_match(&pedido) {
                (a.enviar)("amigo_ti", &format!("{}\"{pedido}\". Tem alguma sugestão de como melhorar pra gente conseguir fazer? Valeu!",
                    n.t("Fala, {amigo_ti}! Aqui é o Jarvis, assistente do {dono}. Ele comentou isto sobre o sistema: ")));
                let _ = crate::barramento::enviar(a.dados, "Alma", "Forja", "pedido", &format!("melhoria de TI pedida ao amigo de TI: {pedido}"), 0, None);
                fez.push("pedido->amigo_ti".into());
            }
        }
    }
    fez
}

/// Uma fala do dono, do comeco ao fim (o miolo do `ciclo` do Python): classifica, guarda no diario local, deixa a `alma_mente` aprender e reagir
/// e aplica as regras de crise/urgencia. Quem chama ja conferiu `observavel`.
pub fn fala(a: &Alma, texto: &str, conselho: Option<&dyn Fn(&str) -> String>) -> Vec<String> {
    let c = classificar(texto, &recorrencia(a.dados, &a.agora), a.nomes);
    guardar(a.dados, &a.agora, texto, &c);
    let mut fez = am::processar(a, texto, &c);
    if c["urgencia"].as_i64().unwrap_or(0) >= 3 {
        let tema = c["tema"].as_str().filter(|t| !t.is_empty()).unwrap_or("sem tema");
        let _ = crate::barramento::enviar(a.dados, "Alma", "Jarvis", "alerta", &format!("urgência {} ({tema}): o Jarvis decide como avisar o dono", c["urgencia"]), 3, None);
    }
    fez.extend(agir(a, texto, &c, conselho));
    fez
}

// ---------- a Alma acompanha a mae ou o pai ----------

/// Pedido do dono: a Alma abre a conversa com a mae e passa a acompanha-la por 24 h. true se a mensagem saiu.
pub fn cuidar(a: &Alma, apelido: &str) -> bool {
    if apelido != "mae" {
        return false; // so quem tem abertura escrita
    }
    let Some(res) = mandar(a, apelido, &a.nomes.t(ABERTURA_MAE)) else { return false };
    let ok = !["Não", "🔒", "⚠", "❌", "Erro", "Cliquei"].iter().any(|p| res.starts_with(p)); // o canal devolve o motivo quando nao envia
    let agora = ts(&a.agora);
    mudar(a.dados, |e| {
        e["cuidar_envio"] = json!({ "quem": apelido, "ts": agora, "resultado": cortar(if res.is_empty() { "sem retorno" } else { &res }, 160) });
        if ok {
            if !e["acompanha"].is_object() {
                e["acompanha"] = json!({});
            }
            e["acompanha"][apelido] = json!(agora + ACOMPANHA_H * 3600);
        }
    });
    ok
}

/// A Alma esta acompanhando esta pessoa agora?
pub fn acompanha(dados: &Path, apelido: &str, agora: &Agora) -> bool {
    estado(dados)["acompanha"][apelido].as_i64().unwrap_or(0) > ts(agora)
}

/// Pura. A pessoa esta dizendo que nao esta bem? E o gatilho para a Alma entrar sozinha.
pub fn sofre(texto: &str) -> bool {
    [SOFRE, CRISE, PEITO].iter().any(|p| rx(p).is_match(texto))
}

/// Alguem da familia ("mae" ou "pai") disse que nao esta bem: a Alma assume a conversa por 24 h e o dono fica sabendo por `avisar(familiar, recado)`.
/// true se entrou agora (false se ja acompanhava ou nao e da familia).
pub fn entrar(dados: &Path, apelido: &str, falas: &str, avisar: &dyn Fn(&str, &str), agora: &Agora) -> bool {
    if !["mae", "pai"].contains(&apelido) || acompanha(dados, apelido, agora) {
        return false;
    }
    mudar(dados, |e| {
        if !e["acompanha"].is_object() {
            e["acompanha"] = json!({});
        }
        e["acompanha"][apelido] = json!(ts(agora) + ACOMPANHA_H * 3600);
    });
    avisar(apelido, &format!("não está bem e a Alma entrou na conversa para acolher. O que foi dito: {}", cortar(&um_espaco(falas), 160)));
    true
}

/// Resposta da Alma ao que a mae ou o pai disse. Crise, saude urgente e peito/pressao tem texto fixo e o dono e avisado SEMPRE (sem teto);
/// o resto vai ao modelo LOCAL com a checagem anti-invencao e, se o modelo nao propuser nada, uma tecnica de apoio entra do mesmo jeito.
/// `grave` = o Zelo ja reconheceu como grave (e ja avisou o dono).
pub fn responder_familia(a: &Alma, textos: &[String], grave: bool, avisar: &dyn Fn(&str, &str), quem: &str) -> String {
    let (n, quem) = (a.nomes, if quem == "pai" { "pai" } else { "mae" });
    let juntos = textos.join(" ");
    if rx(CRISE).is_match(&juntos) {
        avisar(quem, "disse algo muito preocupante, fala com essa pessoa agora");
        return fixo("crise", quem, n);
    }
    for (padrao, chave) in [(URGENTE, "urgente"), (PEITO, "peito")] {
        if rx(padrao).is_match(&juntos) {
            if !grave {
                avisar(quem, &juntos);
            }
            return fixo(chave, quem, n);
        }
    }
    if grave {
        return fixo("grave", quem, n);
    }
    let d = regex::escape(&n.dono.to_lowercase());
    if rx(&format!(r"(?i)fal[ae] (?:com|pra|para) (?:ele|o {d}|meu filho)|avis[ae] (?:ele|o {d}|meu filho)|di[zg]a? (?:pra|para|a) (?:ele|meu filho)|conta pra ele")).is_match(&juntos) {
        avisar(quem, &juntos); // "fala com ele": o pedido chega ao dono, com as palavras da pessoa
        return fixo("recado", quem, n);
    }
    let chave = format!("alma:{quem}");
    let ditas: Vec<String> = am::falas_recentes(a.dados, &a.agora, 6, 6, Some(&chave)).iter().filter_map(|x| x["texto"].as_str().map(String::from)).collect();
    let dica = am::tecnica(&juntos, &ditas, ts(&a.agora));
    let ultimas = &textos[textos.len().saturating_sub(4)..];
    let user = format!("O que a pessoa disse agora:\n{}{}\nEscreva só a resposta da Alma.", ultimas.iter().map(|t| format!("- {}", cortar(t, 240))).collect::<Vec<_>>().join("\n"),
        if dica.is_empty() { String::new() } else { format!("\nTécnica que você pode propor com as suas palavras: {dica}") });
    let (alguem, regra) = if quem == "pai" {
        (n.t("o {pai}, pai do {dono}"), n.t("Trate por '{pai}'. Ele ouve em áudio e entende devagar: frases bem curtas e simples."))
    } else {
        (n.t("a {mae}, mãe do {dono}"), n.t("Trate por '{mae_trato}' ou '{mae}', como uma sobrinha; nunca 'dona' nem 'senhora'."))
    };
    let sis = n.t(PERSONA_FAMILIA).replace("{quem}", &alguem).replace("{regra}", &regra);
    let mut texto = am::gerar(&sis, &user, a.llm, &am::conhecidos(&am::params(a.dados), n));
    texto = if rx(PROIBIDOS).is_match(&texto) { String::new() } else { am::natural(&texto, &ditas) };
    if !dica.is_empty() && !rx(am::JA_SUGERE).is_match(&texto) {
        // o modelo nao propos nada: fica a primeira frase dele (ou um acolhimento) + a tecnica
        let inicio = am::frases(&texto, false, false).first().filter(|f| !f.is_empty()).map(|f| f.to_string()).unwrap_or_else(|| fixo("ouvi", quem, n));
        texto = format!("{inicio} {dica}");
    }
    texto = if texto.is_empty() { fixo("padrao", quem, n) } else { alma_terceiros(&texto, &n.dono_intimo) };
    am::lembrar_fala(a.dados, &chave, &texto, &a.agora); // para nao repetir a mesma tecnica na proxima resposta
    if !dica.is_empty() && texto.contains(&cortar(&dica, 30)) {
        am::registrar_tecnica(a.dados, quem, am::nome_tecnica(&juntos), "", &a.agora); // diario: so o nome da tecnica, nunca o que a pessoa disse
    }
    texto
}

#[cfg(test)]
pub mod teste {
    use super::*;
    use crate::tempo::Data;
    use std::cell::RefCell;

    pub fn nomes() -> Nomes {
        Nomes { dono: "Davi".into(), dono_intimo: "Davizinho".into(), parceira: "Musa Bia".into(), mae: "Tia Lia".into(), mae_trato: "minha tia".into(),
                pai: "Tio Leo".into(), amigo_ti: "Zeca".into(), familia: vec!["Dudu".into()], contato_pai: vec!["leozap".into()], contato_mae: vec!["liazap".into()] }
    }

    pub fn dir(nome: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("braniac-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    pub fn em(dia: i64, hora: u32, min: u32) -> Agora {
        Agora { data: Data::civil(2026, 11, dia).unwrap(), hora, min, seg: 0 }
    }

    pub type Caixa = RefCell<Vec<(String, String)>>;

    /// Canal falso: so anota o que seria enviado.
    pub fn anota(c: &Caixa) -> impl Fn(&str, &str) -> String + '_ {
        move |n: &str, t: &str| {
            c.borrow_mut().push((n.to_string(), t.to_string()));
            "✅ ok".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::teste::*;
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn crise_sai_com_o_texto_fixo_e_chama_a_parceira() {
        let (p, n, cx) = (dir("cuida-crise"), nomes(), Caixa::default());
        let env = anota(&cx);
        let a = Alma { dados: &p, nomes: &n, agora: em(16, 3, 0), enviar: &env, audio: None, llm: None }; // madrugada: crise nao olha a hora
        am::pausar(&p, 5.0, "silêncio", &a.agora); // nem a pausa que ele pediu
        let c = classificar("não aguento mais, quero morrer", &json!({}), &n);
        assert!(c["crise"] == true && c["urgencia"] == 3 && rede_apoio::situacao(&c, "NORMAL") == "emergencia");
        assert_eq!(agir(&a, "não aguento mais, quero morrer", &c, None), ["crise->dono", "urgente->parceira"]);
        let v = cx.borrow();
        assert_eq!(v[0], ("dono".to_string(), "💗 Davi, li o que você escreveu e me importo com você. Não precisa passar por isso sozinho: pode ligar agora para o CVV (188, 24h, gratuito) ou SAMU (192). Já estou chamando a Musa Bia pra ficar com você. Estou aqui.".to_string()));
        assert!(v[1].0 == "parceira" && v[1].1.starts_with("Musa Bia, é a Alma, assistente do Davi.") && v[1].1.contains("URGENTE"));
        assert_eq!(rede_apoio::historico(&p, 30)[0]["situacao"], "emergencia");
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn mae_so_com_permissao_teto_e_horario_e_conselho_recebe_pergunta_generica() {
        let (p, n, cx) = (dir("cuida-mae"), nomes(), Caixa::default());
        let env = anota(&cx);
        let a = Alma { dados: &p, nomes: &n, agora: em(16, 15, 0), enviar: &env, audio: None, llm: None };
        let c2 = json!({ "tema": "familia", "urgencia": 2, "necessidade": "", "crise": false });
        assert_eq!(agir(&a, "preocupado demais com minha mãe", &c2, None), ["aviso->mae"]);
        assert!(cx.borrow()[0].1.starts_with("Oh minha tia, é a Alma"));
        assert!(agir(&a, "preocupado demais com minha mãe", &c2, None).is_empty()); // teto de 1 por dia
        am::definir(&p, "rede_mae_apoio", &json!(false));
        mudar(&p, |e| *e = json!({}));
        assert!(agir(&a, "preocupado demais com minha mãe", &c2, None).is_empty()); // permissao respeitada
        let noite = Alma { agora: em(17, 23, 30), ..Alma { dados: &p, nomes: &n, agora: a.agora, enviar: &env, audio: None, llm: None } };
        am::definir(&p, "rede_mae_apoio", &json!(true));
        assert!(agir(&noite, "x", &c2, None).is_empty()); // fora de 7h-22h
        let visto = std::cell::RefCell::new(String::new());
        let conselho = |q: &str| { *visto.borrow_mut() = q.to_string(); "Durma cedo.".to_string() };
        let c = classificar("ando muito cansado e com dor todo dia", &json!({}), &n);
        assert_eq!(agir(&a, "ando muito cansado e com dor todo dia", &c, Some(&conselho)), ["conselho->dono"]);
        assert!(!visto.borrow().contains("cansado e com dor todo dia") && visto.borrow().contains("sobrecarga física"));
        assert!(cx.borrow().last().unwrap().1.contains("Durma cedo.") && mandar(&a, "dono", "me passa a senha").is_none());
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn classifica_e_separa_recado_de_agente() {
        let n = nomes();
        assert_eq!(classificar("oi", &json!({}), &n)["urgencia"], 0);
        assert!(classificar("o Dudu está com febre", &json!({}), &n)["tema"] == "saude" && classificar("saudade da Musa Bia", &json!({}), &n)["tema"] == "familia");
        assert_eq!(classificar("estou triste", &json!({ "emocional": 3 }), &n)["urgencia"], 2); // recorrencia sobe
        assert_eq!(classificar("tô passando mal", &json!({}), &n)["urgencia"], 3);
        for t in ["Claudinho (via Braniac): a rotina não rodou", "Davi, o notebook está esquentando", "O Davi pediu agora, na conversa: envie", "(o Davi mandou um áudio que não entendi)", "/cmd", "oi"] {
            assert!(!observavel(t, &n), "{t}");
        }
        assert!(observavel("Tô cansado hoje, dormi mal", &n));
        assert_eq!(alma_terceiros("Oi, querido, o meu amor está cansado. Amor, descansa.", "Davizinho"), "Oi, Davizinho, o Davizinho está cansado. Davizinho, descansa.");
    }

    #[test]
    fn familia_crise_peito_recado_e_tecnica() {
        let (p, n, cx) = (dir("cuida-familia"), nomes(), Caixa::default());
        let env = anota(&cx);
        let av = RefCell::new(Vec::<(String, String)>::new());
        let avisar = |f: &str, t: &str| av.borrow_mut().push((f.to_string(), t.to_string()));
        let boa = |_: &str| Some("Poxa, minha tia, que dia difícil. Que tal deitar um pouquinho e tomar uma água? Tô aqui com você.".to_string());
        let inventa = |_: &str| Some("Toma o remédio da Maria no sábado.".to_string());
        let mut a = Alma { dados: &p, nomes: &n, agora: em(16, 15, 0), enviar: &env, audio: None, llm: Some(&boa) };
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(responder_familia(&a, &s(&["tô com muita dor de cabeça e desanimada"]), false, &avisar, "mae"), boa("").unwrap());
        a.llm = Some(&inventa);
        assert_eq!(responder_familia(&a, &s(&["tô triste"]), false, &avisar, "mae"), fixo("padrao", "mae", &n)); // inventou: texto pronto
        a.llm = None;
        assert!(av.borrow().is_empty());
        let r = responder_familia(&a, &s(&["não quero mais viver"]), false, &avisar, "mae");
        assert!(r.contains("CVV, no 188") && r.starts_with("Minha tia,") && av.borrow().len() == 1 && av.borrow()[0].0 == "mae");
        assert!(responder_familia(&a, &s(&["queria sumir de vez, melhor sem mim"]), false, &avisar, "mae").contains("188"));
        let r = responder_familia(&a, &s(&["Só estou com dor de cabeça", "For no peito e tristeza"]), false, &avisar, "mae");
        assert!(r.contains("SAMU, no 192") && r.contains("Davi") && av.borrow().len() == 3 && av.borrow()[2].1.contains("peito"));
        assert!(responder_familia(&a, &s(&["tô com dor no peito e falta de ar"]), true, &avisar, "mae").contains("192") && av.borrow().len() == 3); // grave: o Zelo ja avisou
        assert!(responder_familia(&a, &s(&["preciso de dinheiro pra pagar a conta"]), true, &avisar, "mae").contains("é com o Davi"));
        let r = responder_familia(&a, &s(&["Estava doida para conhecer", "Fala com ele Jarbis"]), false, &avisar, "mae");
        assert!(r.contains("passei pro Davi") && av.borrow().len() == 4 && av.borrow()[3].1.contains("Fala com ele"));
        assert!(responder_familia(&a, &s(&["compressão alta, vou tomar outro remédio"]), false, &avisar, "mae").contains("192"));
        assert!(!rx(PEITO).is_match("comprei peito de frango") && sofre("minha pressão subiu"));
        // modelo fora do ar: acolhe e propoe um exercicio, sem repetir o mesmo em seguida
        let r1 = responder_familia(&a, &s(&["estou desesperada, não sei mais o que fazer"]), false, &avisar, "mae");
        assert!(r1.contains("minha tia") && (r1.contains("contando até") || r1.contains("olha em volta")));
        let r2 = responder_familia(&a, &s(&["continuo desesperada"]), false, &avisar, "mae");
        assert_ne!(cortar(&r1.chars().rev().collect::<String>(), 50), cortar(&r2.chars().rev().collect::<String>(), 50));
        let rp = responder_familia(&a, &s(&["dor no peito"]), true, &avisar, "pai");
        assert!(rp.starts_with("Tio Leo,") && rp.contains("192") && !rp.contains("minha tia"));
        let tcc = std::fs::read_to_string(pasta(&p).join("diario_tcc.jsonl")).unwrap();
        assert!(tcc.contains("ancoragem") && !tcc.contains("desesperada"));
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn acompanha_por_24h_e_entra_sozinha_uma_vez() {
        let (p, n, cx) = (dir("cuida-acompanha"), nomes(), Caixa::default());
        let env = anota(&cx);
        let audios = Caixa::default();
        let voz = anota(&audios);
        let a = Alma { dados: &p, nomes: &n, agora: em(16, 15, 0), enviar: &env, audio: Some(&voz), llm: None };
        assert!(!acompanha(&p, "mae", &a.agora) && !cuidar(&a, "fulano"));
        assert!(cuidar(&a, "mae") && audios.borrow().len() == 1 && cx.borrow().is_empty()); // abertura em audio, sem texto junto
        let t = audios.borrow()[0].1.clone();
        assert!(t.contains("minha tia") && t.contains("Alma") && !["querido", "amor", "senhora", "dona"].iter().any(|x| t.to_lowercase().contains(x)));
        assert!(acompanha(&p, "mae", &a.agora) && !acompanha(&p, "mae", &em(17, 16, 0)));
        assert_eq!(rede_apoio::historico(&p, 30)[0]["contato"], "mae");
        let falhou = |_: &str, _: &str| String::new();
        let b = Alma { audio: Some(&falhou), dados: &p, nomes: &n, agora: a.agora, enviar: &env, llm: None };
        mandar(&b, "mae", "Tô aqui, minha tia.");
        assert_eq!(cx.borrow()[0], ("mae".to_string(), "Tô aqui, minha tia.".to_string())); // audio falhou: ela nao fica sem resposta
        let av = RefCell::new(Vec::<String>::new());
        let avisar = |f: &str, t: &str| av.borrow_mut().push(format!("{f}: {t}"));
        assert!(sofre("Jarvis, estou desesperada") && sofre("tô exausta") && sofre("não tô bem") && !sofre("me acorde 6:00 com alarme por favor") && !sofre("o hóspede chegou bem"));
        assert!(entrar(&p, "pai", "estou desesperado", &avisar, &a.agora) && acompanha(&p, "pai", &a.agora));
        assert!(av.borrow()[0].starts_with("pai: ") && av.borrow()[0].contains("Alma entrou"));
        assert!(!entrar(&p, "pai", "de novo", &avisar, &a.agora) && !entrar(&p, "fulano", "x", &avisar, &a.agora) && av.borrow().len() == 1);
        let _ = std::fs::remove_dir_all(&p);
    }
}
