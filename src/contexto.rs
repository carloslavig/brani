//! Quem e quem e regras de conversa (porte das partes PURAS de braniac/contexto.py): apelido -> contato, quem nao se responde,
//! audio x texto, agradecimento/elogio/encerramento, reapresentacao, palavra negada repetida, numero sem fonte, humor.
//! Nada de nome no codigo: o que e da pessoa vem de dados/pessoal.json ("nome", "contatos", "nao_responder", "prioritarios",
//! "intros", "sempre_texto_numeros", "de_casa") e o que se aprende fica em dados/contexto.json (mesmo formato do Python) e dados/silenciados.json.
//! Ficou de fora: dossie, decidir_gratis/decidir_claude, perguntar_claude/local, consultar (precisam de IA), registrar_fato e voz_nome.
#![allow(dead_code)]
use crate::tempo::Agora;
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::Path;

/// Regex fixa compilada uma vez so.
macro_rules! re {
    ($nome:ident, $pat:expr) => {
        fn $nome() -> &'static regex::Regex {
            static R: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
            R.get_or_init(|| regex::Regex::new($pat).expect("regex fixa"))
        }
    };
}
pub(crate) use re;

/// Letra minuscula sem acento (o NFD + tirar marca do Python, para o alfabeto latino).
pub fn sem_marca(c: char) -> char {
    match c {
        'á' | 'à' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'í' | 'ì' | 'î' | 'ï' => 'i',
        'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
        'ú' | 'ù' | 'û' | 'ü' => 'u',
        'ç' => 'c',
        'ñ' => 'n',
        'ý' | 'ÿ' => 'y',
        _ => c,
    }
}

/// Minusculas, sem acento, sem espaco nas pontas (a pontuacao fica). Marca solta e seletor de emoji somem, como no Python.
pub fn norm(x: &str) -> String {
    let s: String = x.chars().flat_map(char::to_lowercase).map(sem_marca).filter(|c| !matches!(*c, '\u{300}'..='\u{36f}' | '\u{fe00}'..='\u{fe0f}')).collect();
    s.trim().to_string()
}

fn ler_json(p: &Path) -> Value {
    std::fs::read_to_string(p).ok().and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok()).unwrap_or(Value::Null)
}

fn lista(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str()).map(norm).filter(|x| !x.is_empty()).collect()).unwrap_or_default()
}

fn pares(v: &Value) -> Vec<(String, String)> {
    v.as_object().map(|o| o.iter().filter_map(|(k, x)| Some((norm(k), x.as_str()?.to_string()))).collect()).unwrap_or_default()
}

/// Tudo que as regras precisam saber da pessoa, lido de `dados` uma vez por atendimento.
#[derive(Default, Clone, Debug)]
pub struct Ctx {
    /// Nome do dono (pessoal.json "nome"). Vazio = nao configurado.
    pub dono: String,
    pub contatos: BTreeMap<String, String>,
    pub nao_responder: Vec<String>,
    pub prioritarios: Vec<String>,
    pub intros: Vec<(String, String)>,
    pub sempre_texto: Vec<String>,
    pub apresentados: Vec<String>,
    pub preferencias: Vec<(String, String)>,
    pub humor: Option<i64>,
    pub silenciados: Vec<String>,
    /// Gente de casa (pessoal.json "de_casa": apelidos ou nomes de chat): o filtro para terceiros nao vale para eles.
    pub casa: Vec<String>,
}

pub const HUMOR_PADRAO: i64 = 35;
const POR_NADA: [&str; 4] = ["Por nada! 😊", "Disponha!", "Tamo junto! 🙌", "De nada, qualquer coisa é só chamar."];
const AGRADECE: [&str; 4] = ["Valeu demais, fico feliz que gostou! 😊", "Que bom que gostou, obrigado pelo carinho!", "Opa, muito obrigado! Fico feliz em ajudar.", "Obrigado pelo elogio, isso me motiva! 😄"];

re!(texto_re, concat!(
    r"(?i)por\s+escrito|em\s+texto|s[óo]\s+texto|(?:manda|mande|envia|envie|responde|responda|escreve|escreva)\s+(?:isso\s+)?(?:em|por)\s+(?:texto|escrito)|",
    r"mensagem\s+de\s+texto|escreve\s+pra\s+mim|sem\s+[áa]udio|n[aã]o\s+(?:consigo|posso|d[aá])\s+(?:ouvir|escutar)|",
    r"n[aã]o\s+(?:manda|mande|envie)\s+[áa]udio|prefiro\s+(?:texto|escrito)|pode\s+escrever|",
    r"par(?:a|e|ar)\s+(?:de\s+)?(?:mandar|enviar|me\s+mandar)\s+(?:os?\s+)?[áa]udios?|par(?:a|e)\s+com\s+(?:o|os)\s+[áa]udios?|chega\s+de\s+[áa]udios?|",
    r"n[aã]o\s+(?:entend\w+|escut\w+|ou[cç]\w+|consigo\s+entender)\s+(?:o\s+|os\s+)?[áa]udios?|(?:s[óo]|apenas)\s+escrev\w+|",
    r"manda\s+(?:por\s+)?(?:escrito|texto|mensagem)|texto\s+(?:por\s+favor|pfv|pf)|digit[ae]\s+(?:pra|para)\s+mim|",
    r"(?:fa[çc]a|faz|quero)\s+(?:isso\s+)?escrito"));
re!(voz_re, r"(?i)volta\w*\s+(?:a\s+|pro\s+|para\s+o\s+)?(?:mandar\s+)?[áa]udio|prefiro\s+[áa]udio|pode\s+(?:mandar|responder)\s+(?:em\s+|por\s+)?[áa]udio");
// confirmacoes, links, numeros: tem que ser LIDO
re!(precisa_texto, r"(?i)/sim|/nao|confirma\?|https?://|www\.|«|\d{6,}");
re!(encerra, concat!(
    r"(?i)^(?:(?:ok|okay|okk|certo|certinho|beleza|blz|show|perfeito|entendi|entendido|combinado|fechado|valeu|vlw|obrigad[oa]|obg|tmj|tudo bem|tudo certo|",
    r"t[aá] bom|t[aá] certo|t[aá] [oó]timo|[oó]timo|legal|massa|top|boa|de nada|por nada|imagina|disponha|sim|uhum|isso|",
    r"j[aá] (?:vi|anotei|entendi)|anotado|recebido|bom dia|boa tarde|boa noite)[\s,.!👍🙏😊🙂😄❤️✅]*)+$"));
re!(elogio_re, r"(?i)\b(?:muito\s+bom|[eé]\s+bom|bom\s+demais|show|top|demais|parab[eé]ns|incr[ií]vel|excelente|[oó]timo|massa|gostei|arrasou|mandou\s+bem|fant[aá]stico|perfeito|nota\s*10|sensacional|genial)\b");
re!(pede, r"(?i)\b(?:quero|preciso|pode|poderia|consegue|me\s+(?:manda|passa|ajuda)|como|quando|onde|quanto|qual|por\s*que)\b");
re!(agradecimento_re, r"(?i)^\W*(?:muito\s+)?(?:valeu|vlw|obrigad[oa]s?|obg|brigad[oa]|tmj)\b[\w\s,.!👍🙏❤️😊🙂😄]*$");
re!(negado, r"(?i)\bn[aã]o\s+(?:[eé]\s+)?(?:\w+\s+){0,2}?(\w{4,})\s+n[aã]o\b");
re!(saudacoes_automaticas, r"(?i)agradece\s+seu\s+contato|agradecemos\s+(?:o\s+seu\s+|seu\s+)?contato|mensagem\s+autom[aá]tica");
re!(num_fato, r"(?i)R\$\s*\d[\d.,]*|\b\d{1,2}\s*h(?:\d{2})?\b|\b\d{1,2}:\d{2}\b|\b\d+\s*(?:reais|min(?:utos)?|dias|km)\b");
re!(nao_nome, r"[^A-Za-zÀ-ÿ\s'\-]");

/// O nome do dono dentro de uma regex; sem nome configurado, um trecho que nunca casa.
pub fn dono_re(dono: &str) -> String {
    if dono.trim().is_empty() { r"\x00".into() } else { regex::escape(dono.trim()) }
}

/// Frases em que o Jarvis diz quem e / o que faz. Quem ja foi apresentado nao precisa ouvir isso de novo.
pub fn quem_sou(dono: &str) -> Regex {
    Regex::new(&format!(
        r"(?i)\b(?:jarvis|braniac)\b.*\b(?:assistente|sou|aqui\s+[ée]|uso|usando|cuido|conecta|roteia|consulta)\b|\b(?:sou|aqui\s+[ée]|eu\s+sou)\s+(?:o\s+)?jarvis\b|\bassistente\s+(?:de\s+)?(?:ia|intelig[eê]ncia\s+artificial|virtual)\s+d[oa]\s+{}\b",
        dono_re(dono))).expect("regex fixa")
}
re!(pergunta_quem, r"(?i)quem\s+(?:[ée]|[eé]s|foi)\s+(?:voc[eê]|vc|o\s+jarvis|esse)|o\s+que\s+(?:voc[eê]|vc|o\s+jarvis)\s+(?:faz|[ée])|que\s+(?:voc[eê]|vc)\s+[ée]|como\s+(?:voc[eê]|vc)\s+funciona|pra\s+que\s+serve|para\s+que\s+serve|\bjarvis\b.*\?");

fn tamanho(t: &str) -> usize {
    t.chars().count()
}

fn sorteio(t: &str, n: usize) -> usize {
    t.chars().map(|c| c as usize).sum::<usize>() % n
}

impl Ctx {
    pub fn ler(dados: &Path) -> Ctx {
        let (p, c) = (ler_json(&dados.join("pessoal.json")), ler_json(&dados.join("contexto.json")));
        let mut contatos: BTreeMap<String, String> = pares(&p["contatos"]).into_iter().collect();
        contatos.extend(pares(&c["contatos"])); // o aprendido ganha da semente
        let mut nao_responder = lista(&p["nao_responder"]);
        nao_responder.extend(lista(&c["nao_responder"]));
        Ctx {
            dono: p["nome"].as_str().unwrap_or("").trim().to_string(),
            contatos,
            nao_responder,
            prioritarios: lista(&p["prioritarios"]),
            intros: pares(&p["intros"]),
            sempre_texto: lista(&p["sempre_texto_numeros"]),
            apresentados: lista(&c["apresentados"]),
            preferencias: pares(&c["preferencias"]),
            humor: c["humor"].as_i64(),
            silenciados: lista(&ler_json(&dados.join("silenciados.json"))),
            casa: lista(&p["de_casa"]),
        }
    }

    /// "do Fulano", ou um texto neutro quando o nome nao foi configurado.
    fn do_dono(&self) -> String {
        if self.dono.is_empty() { "do dono deste número".into() } else { format!("do {}", self.dono) }
    }

    /// Nome do contato no aplicativo de mensagem para um apelido ("minha mae" -> como esta salvo).
    pub fn contato(&self, apelido: &str) -> Option<&str> {
        self.contatos.get(&norm(apelido)).map(String::as_str)
    }

    /// O chat pelo qual o proprio dono fala com o Jarvis (apelido = o nome dele).
    pub fn chat_do_dono(&self) -> Option<&str> {
        if self.dono.is_empty() { None } else { self.contato(&self.dono) }
    }

    /// O proprio dono ou alguem de casa: tom e acesso deles sao outros.
    pub fn de_casa(&self, nome: &str) -> bool {
        let n = norm(nome);
        let dono = norm(&self.dono);
        (!dono.is_empty() && n.starts_with(&dono)) || self.chat_do_dono().is_some_and(|c| norm(c) == n)
            || self.casa.iter().any(|a| *a == n || self.contatos.get(a).is_some_and(|c| norm(c) == n))
    }

    pub fn intro_para(&self, chat: &str) -> Option<&str> {
        let n = norm(chat);
        self.intros.iter().find(|(k, _)| !k.is_empty() && n.contains(k.as_str())).map(|(_, v)| v.as_str())
    }

    /// Apresentacao no 1o contato, SEMPRE explicando quem e o Jarvis. Nunca diz que o dono esta ocupado.
    pub fn abertura_padrao(&self, chat: &str) -> String {
        if let Some(e) = self.intro_para(chat) {
            return e.to_string();
        }
        let oi = primeiro_nome(chat).map_or("Oi! ".to_string(), |n| format!("Oi, {n}! "));
        format!("{oi}Eu sou o Jarvis, o assistente de inteligência artificial {}. Eu cuido do atendimento dele no WhatsApp: respondo as mensagens, anoto os recados e passo tudo pra ele. ", self.do_dono())
    }

    /// Conversas que o Jarvis SEMPRE responde (nunca entram em "nao responder").
    pub fn prioritario(&self, chat: &str) -> bool {
        let n = norm(chat);
        self.prioritarios.iter().any(|x| n.contains(x.as_str()))
    }

    pub fn silenciado(&self, chat: &str) -> bool {
        self.silenciados.contains(&norm(chat))
    }

    /// Trava: conta automatica/empresa/grupo da lista, ou conversa que o dono mandou calar.
    pub fn nao_responder(&self, chat: &str) -> bool {
        if self.silenciado(chat) {
            return true; // ordem direta do dono vale ate para o prioritario
        }
        if self.prioritario(chat) {
            return false;
        }
        let n = norm(chat);
        self.nao_responder.iter().any(|x| n.contains(x.as_str()))
    }

    /// "voz" (padrao) ou "texto". Numero de bot sempre texto; a preferencia salva vale tambem para nome parecido.
    pub fn preferencia(&self, chat: &str) -> &str {
        let n = norm(chat);
        if self.sempre_texto.iter().any(|x| n.contains(x.as_str())) {
            return "texto";
        }
        if let Some((_, v)) = self.preferencias.iter().find(|(k, _)| *k == n) {
            return v;
        }
        self.preferencias.iter().find(|(k, _)| tamanho(k) >= 4 && tamanho(&n) >= 4 && (n.contains(k.as_str()) || k.contains(n.as_str()))).map_or("voz", |(_, v)| v.as_str())
    }

    /// Responder em AUDIO? Sim por padrao; nao se a pessoa pediu texto, se a resposta tem que ser lida (confirmacao, link, numero) ou e longa.
    pub fn deve_falar(&self, chat: &str, resposta: &str) -> bool {
        let n = norm(chat);
        let eh_dono = !self.dono.is_empty() && (n == norm(&self.dono) || self.chat_do_dono().is_some_and(|c| norm(c) == n));
        let maximo = if eh_dono { 1800 } else { 600 }; // ao dono, relatorio longo tambem vai em audio
        self.preferencia(chat) != "texto" && !precisa_texto().is_match(resposta) && tamanho(resposta) <= maximo
    }

    pub fn apresentado(&self, chat: &str) -> bool {
        self.apresentados.contains(&norm(chat))
    }

    pub fn humor(&self) -> i64 {
        self.humor.map_or(HUMOR_PADRAO, |v| v.clamp(0, 100))
    }

    /// Tira as frases de auto-apresentacao de uma resposta a quem JA conhece o Jarvis. Se a pessoa perguntou quem ele e, mantem.
    pub fn sem_reapresentacao(&self, texto: &str, pergunta: &str) -> String {
        if texto.is_empty() || pergunta_quem().is_match(pergunta) {
            return texto.to_string();
        }
        let (q, todas) = (quem_sou(&self.dono), frases(texto.trim()));
        let ficam: Vec<&str> = todas.iter().copied().filter(|f| !q.is_match(f)).collect();
        if ficam.len() == todas.len() {
            return texto.to_string();
        }
        let r = ficam.join(" ");
        if r.trim().is_empty() { "Claro! Posso ajudar em mais alguma coisa?".into() } else { r.trim().to_string() }
    }
}

/// Corta nas pausas de fim de frase (espaco depois de . ! ?), como o split com lookbehind do Python.
pub(crate) fn frases(t: &str) -> Vec<&str> {
    let cs: Vec<(usize, char)> = t.char_indices().collect();
    let (mut v, mut ini, mut i) = (vec![], 0, 0);
    while i < cs.len() {
        if cs[i].1.is_whitespace() && i > 0 && matches!(cs[i - 1].1, '.' | '!' | '?') {
            let mut j = i;
            while j < cs.len() && cs[j].1.is_whitespace() {
                j += 1;
            }
            v.push(&t[ini..cs[i].0]);
            ini = cs.get(j).map_or(t.len(), |c| c.0);
            i = j;
        } else {
            i += 1;
        }
    }
    v.push(&t[ini..]);
    v
}

/// Primeiro nome como esta salvo ("Ana Souza 🌝" -> "Ana"). None para numero/sem nome.
pub fn primeiro_nome(chat: &str) -> Option<String> {
    let t = chat.trim();
    if t.starts_with(['+', '(']) || t.chars().next().is_some_and(char::is_numeric) {
        return None;
    }
    nao_nome().replace_all(chat, " ").split_whitespace().next().map(str::to_string)
}

pub fn quer_voz(msg: &str) -> bool {
    voz_re().is_match(msg)
}

/// A pessoa pediu resposta por escrito (e nao pediu audio na mesma fala).
pub fn quer_texto(msg: &str) -> bool {
    texto_re().is_match(msg) && !quer_voz(msg)
}

/// Mensagem so de agradecimento/confirmacao (sem pergunta nem pedido): nao precisa de resposta, senao vira laco infinito.
pub fn encerramento(texto: &str) -> bool {
    let t = texto.trim();
    !t.is_empty() && !t.contains('?') && tamanho(t) <= 60 && encerra().is_match(t)
}

/// Mensagem so de elogio: devolve um agradecimento curto e variado, em vez de a IA repetir a apresentacao.
pub fn elogio(texto: &str) -> Option<&'static str> {
    let t = texto.trim();
    (!t.is_empty() && !t.contains('?') && tamanho(t) <= 100 && elogio_re().is_match(t) && !pede().is_match(t)).then(|| AGRADECE[sorteio(t, AGRADECE.len())])
}

/// So "valeu"/"obrigado" (sem pergunta nem pedido): devolve um "por nada" curto ou None.
pub fn agradecimento(texto: &str) -> Option<&'static str> {
    let t = texto.trim();
    (!t.is_empty() && !t.contains('?') && tamanho(t) <= 40 && agradecimento_re().is_match(t) && !pede().is_match(t)).then(|| POR_NADA[sorteio(t, POR_NADA.len())])
}

/// A pessoa corrigiu ("nao com o filho nao, com o marido") e a resposta repetiu a palavra negada: devolve a palavra, ou None.
pub fn repete_negado(entrada: &str, resposta: &str) -> Option<String> {
    let p = negado().captures(entrada)?.get(1)?.as_str().to_lowercase();
    Regex::new(&format!(r"(?i)\b{}\b", regex::escape(&p))).ok()?.is_match(resposta).then_some(p)
}

/// Saudacao automatica do aplicativo de mensagem: aparece como nossa, mas nao e resposta.
pub fn saudacao_automatica(texto: &str) -> bool {
    saudacoes_automaticas().is_match(texto)
}

/// Resposta segura quando falta fonte: promete confirmar com o dono.
pub fn confirmar(dono: &str) -> String {
    if dono.trim().is_empty() { "Deixa eu confirmar esse ponto e já te respondo, tá?".into() } else { format!("Deixa eu confirmar esse ponto com o {} e já te respondo, tá?", dono.trim()) }
}

/// Trava para IA gratis: valor em reais, horario ou prazo que NAO esta na base vira "vou confirmar" (cod A = avisa o dono).
/// `res` = (id, cod, texto) do protocolo do canal.
pub fn sem_numero_inventado(res: (String, String, String), base: &str, dono: &str) -> (String, String, String) {
    if res.1 != "E" {
        return res;
    }
    let junto = |s: &str| s.to_lowercase().split_whitespace().collect::<String>();
    let base = junto(base);
    if num_fato().find_iter(&res.2).any(|n| !base.contains(&junto(n.as_str()))) {
        return (res.0, "A".into(), confirmar(dono));
    }
    res
}

/// Instrucao de personalidade para o nivel de humor (0-100). Vale para qualquer canal.
pub fn diretriz_humor(n: i64) -> String {
    let regra = "NUNCA faça piada em assunto sério (dinheiro, saúde, luto, reclamação, segurança, pedido urgente) nem quando a pessoa estiver chateada, \
                 e nunca à custa de ninguém; a autoironia leve sobre ser uma IA é bem-vinda. A piada é curta e natural, nunca forçada, e a resposta continua útil e clara.";
    match n {
        i64::MIN..=10 => format!("Humor {n}%: sério e objetivo, sem piadas."),
        11..=25 => format!("Humor {n}%: bem discreto; só de vez em quando uma observação leve. {regra}"),
        26..=45 => format!("Humor {n}%: bom humor discreto e natural, mais ou menos uma tirada leve (ironia amigável, trocadilho, comentário espirituoso) a cada três respostas; nas outras, só simpatia. {regra}"),
        46..=70 => format!("Humor {n}%: bem-humorado, com graça em boa parte das respostas, sem exagero. {regra}"),
        _ => format!("Humor {n}%: muito bem-humorado e brincalhão, mas sempre claro e respeitoso. {regra}"),
    }
}

/// Data e hora ATUAIS em texto, com o periodo do dia para saudacoes.
pub fn agora_texto(a: &Agora) -> String {
    const DIAS: [&str; 7] = ["segunda-feira", "terça-feira", "quarta-feira", "quinta-feira", "sexta-feira", "sábado", "domingo"];
    let periodo = match a.hora { 0..=4 => "madrugada", 5..=11 => "manhã", 12..=17 => "tarde", _ => "noite" };
    let (y, m, d) = a.data.partes();
    format!("{}, {d:02}/{m:02}/{y:04}, {} ({periodo}). Use ESTA hora para saudações (bom dia/boa tarde/boa noite) e para dizer se a loja está aberta.", DIAS[a.data.semana()], a.hm())
}

// ---------- o que se aprende: dados/contexto.json (so a chave mexida muda) ----------

fn mudar(dados: &Path, f: impl FnOnce(&mut Map<String, Value>)) -> Result<(), String> {
    let arq = dados.join("contexto.json");
    let mut o = match ler_json(&arq) {
        Value::Object(o) => o,
        _ => Map::new(),
    };
    f(&mut o);
    std::fs::create_dir_all(dados).map_err(|e| e.to_string())?;
    std::fs::write(arq, serde_json::to_string_pretty(&Value::Object(o)).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

fn objeto<'a>(o: &'a mut Map<String, Value>, k: &str) -> &'a mut Map<String, Value> {
    let v = o.entry(k).or_insert_with(|| json!({}));
    if !v.is_object() {
        *v = json!({});
    }
    v.as_object_mut().expect("acabou de virar objeto")
}

/// Frase solta nunca vira apelido: no maximo 3 palavras.
pub fn registrar_contato(dados: &Path, apelido: &str, nome_no_chat: &str) -> Result<(), String> {
    let a = norm(apelido);
    if a.is_empty() || a.split_whitespace().count() > 3 {
        return Ok(());
    }
    mudar(dados, |o| {
        objeto(o, "contatos").insert(a, json!(nome_no_chat));
    })
}

pub fn definir_preferencia(dados: &Path, chat: &str, valor: &str) -> Result<(), String> {
    mudar(dados, |o| {
        objeto(o, "preferencias").insert(norm(chat), json!(valor));
    })
}

/// Le o que a pessoa disse: pediu por escrito -> texto (fica ate pedir audio de novo); pediu audio -> voz. Devolve a preferencia que vale.
pub fn atualizar_preferencia(dados: &Path, chat: &str, msg: &str) -> String {
    let novo = if quer_voz(msg) { Some("voz") } else if quer_texto(msg) { Some("texto") } else { None };
    if let Some(v) = novo {
        let _ = definir_preferencia(dados, chat, v);
    }
    Ctx::ler(dados).preferencia(chat).to_string()
}

pub fn marcar_apresentado(dados: &Path, chat: &str) -> Result<(), String> {
    let n = norm(chat);
    mudar(dados, |o| {
        let v = o.entry("apresentados").or_insert_with(|| json!([]));
        if !v.is_array() {
            *v = json!([]);
        }
        if let Some(a) = v.as_array_mut().filter(|a| !a.iter().any(|x| x.as_str() == Some(&n))) {
            a.push(json!(n));
        }
    })
}

pub fn definir_humor(dados: &Path, nivel: i64) -> Result<i64, String> {
    let n = nivel.clamp(0, 100);
    mudar(dados, |o| {
        o.insert("humor".into(), json!(n));
    }).map(|_| n)
}

/// Ordem do dono ("pare de falar nessa conversa"): o Jarvis nao responde mais esse chat ate ele liberar.
pub fn silenciar(dados: &Path, chat: &str, sim: bool) -> Result<(), String> {
    let mut s: std::collections::BTreeSet<String> = lista(&ler_json(&dados.join("silenciados.json"))).into_iter().collect();
    if sim { s.insert(norm(chat)); } else { s.remove(&norm(chat)); }
    std::fs::create_dir_all(dados).map_err(|e| e.to_string())?;
    std::fs::write(dados.join("silenciados.json"), json!(s).to_string()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;

    fn ctx() -> Ctx {
        Ctx { dono: "Davi".into(), nao_responder: vec!["cobrabot".into(), "operadora".into()], prioritarios: vec!["loja da bia".into(), "bia".into()],
              intros: vec![("mamae".into(), "Oi, minha tia! ".into())], sempre_texto: vec!["9000-0002".into()], ..Default::default() }
    }

    #[test]
    fn agradecimento_so_quando_e_so_agradecimento() {
        assert!(agradecimento("valeu").is_some() && agradecimento("Muito obrigado! 🙏").is_some() && agradecimento("vlw mesmo").is_some());
        assert_eq!(agradecimento("valeu, quanto custa?"), None);
        assert_eq!(agradecimento("obrigado, mas preciso do orçamento"), None);
        assert_eq!(agradecimento("fechado"), None); // pode ser aceite de venda: nunca e so agradecimento
        assert_eq!(agradecimento(""), None);
    }

    #[test]
    fn encerramento_e_elogio() {
        assert!(encerramento("Ok, obrigada! 👍") && encerramento("Entendido") && encerramento("beleza, valeu"));
        assert!(!encerramento("Ok, e o pedido do João?") && !encerramento("Preciso de ajuda com o cardápio") && !encerramento(""));
        assert!(elogio("Você é muito bom!").is_some() && elogio("Show").is_some());
        assert!(elogio("muito bom, como funciona?").is_none() && elogio("Quero um orçamento, muito bom o site").is_none());
    }

    #[test]
    fn sem_reapresentacao_tira_so_a_apresentacao() {
        let (c, t) = (ctx(), "Sou o Jarvis, assistente de IA do Davi. A landing page custa R$ 150.");
        assert_eq!(c.sem_reapresentacao(t, ""), "A landing page custa R$ 150.");
        assert_eq!(c.sem_reapresentacao(t, "quem é você?"), t); // perguntou quem e: explica
        assert_eq!(c.sem_reapresentacao("Oi! Eu sou o Jarvis, assistente de IA do Davi.", ""), "Oi!");
        assert_eq!(c.sem_reapresentacao("Aqui é o Jarvis, assistente do Davi.", ""), "Claro! Posso ajudar em mais alguma coisa?");
        assert_eq!(c.sem_reapresentacao("Pode ser amanhã às 10h.", ""), "Pode ser amanhã às 10h.");
        // sem dono configurado a frase generica continua saindo e nada quebra
        assert_eq!(Ctx::default().sem_reapresentacao("Sou o Jarvis. Tudo certo.", ""), "Tudo certo.");
    }

    #[test]
    fn palavra_negada_que_a_resposta_repete() {
        assert_eq!(repete_negado("Não comeu filho não Jarvis, comeu marido.", "Entendo essa situação com o seu filho.").as_deref(), Some("filho"));
        assert_eq!(repete_negado("Não é com o filho não, é com o marido", "Desculpa! Com o seu marido, entendi."), None);
        assert_eq!(repete_negado("bom dia, tudo bem?", "Tudo bem, minha tia!"), None);
    }

    #[test]
    fn numero_que_nao_esta_na_base_vira_confirmar() {
        let r = |t: &str| ("ab12".to_string(), "E".to_string(), t.to_string());
        let base = "A página custa R$ 150 e a loja abre às 18h. Prazo: 5 dias.";
        assert_eq!(sem_numero_inventado(r("Custa R$150 e abre 18h, em 5 dias."), base, "Davi"), r("Custa R$150 e abre 18h, em 5 dias."));
        for inventado in ["Custa R$ 200.", "Abre às 19h.", "Fica pronto em 3 dias.", "Chego 10:30.", "São 40 reais."] {
            assert_eq!(sem_numero_inventado(r(inventado), base, "Davi"), ("ab12".into(), "A".into(), confirmar("Davi")), "{inventado}");
        }
        // so a resposta que seria ENVIADA e conferida
        let nota = ("ab12".to_string(), "A".to_string(), "pediu R$ 999".to_string());
        assert_eq!(sem_numero_inventado(nota.clone(), base, "Davi"), nota);
    }

    #[test]
    fn nao_responder_prioritario_e_silenciado() {
        let mut c = ctx();
        assert!(c.nao_responder("CobraBot Pagamentos") && c.nao_responder("Operadora") && !c.nao_responder("Rui"));
        assert!(c.prioritario("Loja da Bía") && c.prioritario("Bia") && !c.prioritario("Rui"));
        c.nao_responder.push("loja".into());
        assert!(!c.nao_responder("Loja da Bia")); // prioritario nunca entra na lista
        c.silenciados.push("loja da bia".into());
        assert!(c.nao_responder("Loja da Bia")); // ordem direta do dono vale ate para o prioritario
    }

    #[test]
    fn pedidos_de_texto_e_de_voz() {
        for msg in ["para de mandar áudio por favor", "pare de me mandar áudio", "chega de áudio", "não entendo áudio", "só escreve pra mim", "manda por escrito",
                    "para com os áudios", "Faça escrito", "faz isso escrito pra mim"] {
            assert!(quer_texto(msg), "{msg}");
        }
        assert!(!quer_texto("está escrito no contrato") && !quer_texto("oi, tudo bem?"));
        assert!(quer_voz("pode mandar áudio") && !quer_texto("prefiro áudio, sem áudio não entendo"));
    }

    #[test]
    fn primeiro_nome_e_abertura() {
        assert_eq!(primeiro_nome("Rui Lopes 🌝").as_deref(), Some("Rui"));
        assert_eq!(primeiro_nome("Zélia ✨ Eva").as_deref(), Some("Zélia"));
        assert_eq!((primeiro_nome("+55 86 9000-0003"), primeiro_nome("(11) 99999"), primeiro_nome("🌝")), (None, None, None));
        let c = ctx();
        for chat in ["Maria", "+55 66 9000-0001", "Rui Lopes 🌝"] {
            let a = c.abertura_padrao(chat);
            assert!(a.contains("Jarvis") && a.contains("anoto os recados"), "{chat}");
        }
        assert!(c.abertura_padrao("Rui Lopes").starts_with("Oi, Rui! Eu sou o Jarvis, o assistente de inteligência artificial do Davi."));
        assert!(c.abertura_padrao("+55 66 9000-0001").starts_with("Oi! Eu sou"));
        assert_eq!(c.abertura_padrao("Mamãe ❤️"), "Oi, minha tia! ");
    }

    #[test]
    fn estado_em_arquivo_preferencia_apresentado_humor_contato() {
        let d = std::env::temp_dir().join(format!("braniac-contexto-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("pessoal.json"), r#"{"nome": "Davi", "contatos": {"Davi": "Meu Zap", "Tia Lúcia": "Lucia Zap"},
            "nao_responder": ["CobraBot"], "sempre_texto_numeros": ["9000-0002"]}"#).unwrap();
        let c = Ctx::ler(&d);
        assert_eq!((c.contato("tia lucia"), c.chat_do_dono(), c.contato("ninguem")), (Some("Lucia Zap"), Some("Meu Zap"), None));
        assert!(c.preferencia("Maria") == "voz" && c.deve_falar("Maria", "Tudo certo, pode vir amanhã.") && c.humor() == 35);
        // pediu por escrito: fica salvo ate pedir audio de novo, e vale para nome parecido
        assert_eq!(atualizar_preferencia(&d, "Maria", "manda por escrito"), "texto");
        let c = Ctx::ler(&d);
        assert!(c.preferencia("Maria") == "texto" && !c.deve_falar("Maria", "Tudo certo.") && c.deve_falar("Joao", "Tudo certo."));
        assert!(c.preferencia("Maria Silva Souza") == "texto" && c.preferencia("Joana") == "voz");
        assert_eq!(atualizar_preferencia(&d, "Maria", "pode mandar áudio"), "voz");
        // numero de bot: sempre texto
        assert!(c.preferencia("+55 66 9000-0002") == "texto" && !c.deve_falar("+55 66 9000-0002", "oi tudo bem"));
        // o que tem de ser lido nao vira audio; ao dono o limite e maior
        assert!(!c.deve_falar("Joao", "Confirma? /sim") && !c.deve_falar("Joao", "veja https://x.y") && !c.deve_falar("Joao", "código 123456"));
        let longo = "a ".repeat(500);
        assert!(!c.deve_falar("Joao", &longo) && c.deve_falar("Meu Zap", &longo) && c.deve_falar("Davi", &longo));
        // apresentado, humor, contato aprendido, silenciar
        assert!(!c.apresentado("Rui 🌝"));
        marcar_apresentado(&d, "Rui 🌝").unwrap();
        marcar_apresentado(&d, "Rui 🌝").unwrap();
        assert_eq!(definir_humor(&d, 250), Ok(100));
        registrar_contato(&d, "Zé", "Jose Zap").unwrap();
        registrar_contato(&d, "uma frase solta que nao e apelido", "X").unwrap();
        silenciar(&d, "Beltrano", true).unwrap();
        let c = Ctx::ler(&d);
        assert!(c.apresentado("rui 🌝") && c.apresentados.len() == 1 && c.humor() == 100);
        assert_eq!((c.contato("ze"), c.contatos.len()), (Some("Jose Zap"), 3));
        assert!(c.silenciado("Beltrano") && c.nao_responder("Beltrano") && c.nao_responder("cobrabot"));
        silenciar(&d, "Beltrano", false).unwrap();
        assert!(!Ctx::ler(&d).nao_responder("Beltrano"));
        // arquivo estragado nao derruba: volta ao padrao
        std::fs::write(d.join("contexto.json"), "{quebrado").unwrap();
        assert_eq!(Ctx::ler(&d).humor(), 35);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn humor_saudacao_automatica_e_hora() {
        assert!(diretriz_humor(35).contains("35%") && diretriz_humor(35).contains("a cada três respostas"));
        assert!(diretriz_humor(5).contains("sem piadas") && diretriz_humor(90).contains("brincalhão"));
        assert!(saudacao_automatica("A Empresa agradece seu contato. Como posso ajudar?"));
        assert!(!saudacao_automatica("Oi, Ana! Eu sou o Jarvis. Como posso te ajudar?"));
        let a = Agora { data: Data::civil(2026, 10, 13).unwrap(), hora: 9, min: 5, seg: 0 };
        assert!(agora_texto(&a).contains("terça-feira, 13/10/2026, 09:05 (manhã)"));
    }
}
