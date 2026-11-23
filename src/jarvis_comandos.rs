//! COMANDOS DIRETOS do Jarvis, reconhecidos sem IA (porte das partes puras de braniac/jarvis.py): "/enviar nome|texto",
//! "/arquivo nome|caminho|legenda", "manda um audio para X dizendo...", "avisa X que...", "pede pro Claude...", "errou" (anota em
//! erros_jarvis.jsonl), numero solto, "so texto para X", e-mail (so leitura), consulta a IA parceira e os fluxos de envio
//! (acha o contato sem perguntar -> corrige o tratamento -> barra dado sensivel -> envia pela closure).
//! Nada aqui envia sozinho: listar contatos, mandar texto/audio/arquivo e perguntar ao agente entram por `Saidas`.
//! Ficou de fora: Spotify (frases de musica), legado, /voz e /escuta (entram como closures em `jarvis::Pecas`).
#![allow(dead_code)]
use crate::contexto::{self, re, Ctx};
use crate::jarvis_travas::{melhor_candidato, resolver_nome, tem_dado_real};
use crate::{memoria, mudo, tratamento};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const CONTEXTO_ANEXADO: &str = "\n\n[Conversa recente no WhatsApp:";

// nome de contato e curto e sem virgula/dois-pontos/interrogacao: uma pergunta comprida ja virou "nome" e a mensagem saiu para um desconhecido
macro_rules! nome {
    () => {
        r"(?P<nome>[^\n,;:!?]{1,40}?)"
    };
}
re!(re_envio, concat!(
    r"(?is)^(?:jarvis[,. ]+)?(?:manda|mande|envia|envie)\s+(?:uma\s+)?(?:mensagem|msg|recado)?\s*(?:pra|pro|para|a|à)\s+(?:a\s+|o\s+)?", nome!(),
    r"\s+(?:no\s+whatsapp\s+|pelo\s+whatsapp\s+)?(?:dizendo|falando|avisando|perguntando)\s*(?:que\s+)?(?P<texto>.+)$"));
re!(re_audio, concat!(
    r"(?is)^(?:jarvis[,. ]+)?(?:manda|mande|envia|envie)\s+(?:um\s+)?[aá]udio\s+(?:pra|pro|para)\s+(?:a\s+|o\s+)?", nome!(),
    r"\s+(?:dizendo|falando|explicando|avisando|contando)\s*(?:que\s+)?(?P<texto>.+)$"));
re!(re_aviso, concat!(
    r"(?is)^(?:jarvis[,. ]+)?(?:avisa|avise|fala|fale|diz|diga|pergunta|pergunte)\s+(?:pra|pro|para|com|a|à)?\s*(?:a\s+|o\s+)?", nome!(),
    r"\s+(?:que|se|:)\s+(?P<texto>.+)$"));
re!(re_cmd_enviar, r"(?s)^/enviar\s+(?P<nome>[^|]+)\|(?P<texto>.+)$");
re!(re_cmd_arquivo, r"(?s)^/arquivo\s+(?P<nome>[^|]+)\|(?P<caminho>[^|]+)(?:\|(?P<legenda>.*))?$");
re!(re_claude, r"(?is)^(?:jarvis[,. ]+)?(?:(?:pede|pe[cç]a|chama|aciona|manda)\s+(?:pro|para\s+o|ao|pra)\s+claude\s*(?:para|pra|que)?\s*|/claude\s+)(?P<tarefa>.+)$");
re!(re_ia, concat!(
    r"(?is)^(?:jarvis[,. ]+)?(?:entr\w*\s+em\s+contato\s+com|pergunt\w*|consult\w*|fal\w*|ped\w*|pe[cç]\w*|v[aá]\s+(?:no|na)|manda\w*|mande|chec\w*|verific\w*|confer\w*|confir\w*)\b",
    r".*?\b(?P<ia>chat\s?gpt|gpt|gemini|grok)\b[\s,:;]*(?:e\s+)?(?:(?:pergunt\w*|diz\w*|dig\w*|pe[cç]\w*|fal\w*)\s+)?(?:(?:que|se|:)\s+)?(?P<q>.*)$"));
re!(re_nome_ia, r"(?i)\b(?:chat\s?gpt|gpt|gemini|grok)\b");
re!(re_e_me_diz, r"(?is)\s*\be\s+(me\s+)?(diz|diga|passa|passe|fala|fale|conta|mostra)\b.*$");
re!(re_erro, r"(?i)\b(?:errou|seu\s+cu)\b");
re!(re_numero, r"^\+?[\d\s().-]{7,}$");
re!(re_atender, r"(?is)\b(respond\w*|atend\w*|cuid\w*)\b.*\b(whats\s?app|zap|wpp|pessoas|mensagens|conversas)\b|\b(whats\s?app|zap|wpp)\b.*\b(respond\w*|atend\w*)\b");
re!(re_consegue, r"(?i)\b(consegue|pode|poderia|sabe|d[aá]\s+pra)\b");
re!(re_sem_audio, concat!(
    r"(?i)(?:\bpar(?:a|e|ar)\s+(?:de\s+)?(?:mandar|enviar)\s+(?:os?\s+)?[áa]udios?\s+(?:pra|para|pro|a|à|ao|na|no|com)\s+(?:a\s+|o\s+)?(?P<n1>[^.,;!?\n]+))|",
    r"(?:(?:^|\n|,|\.)\s*(?:a\s+|o\s+)?(?P<n2>[^.,;!?\n]{2,40}?)\s+n[aã]o\s+(?:entende|entendem|escuta|ouve|consegue\s+(?:ouvir|entender))\s+(?:o\s+|os\s+)?[áa]udios?)|",
    r"(?:\bs[óo]\s+texto\s+(?:pra|para|pro|com)\s+(?:a\s+|o\s+)?(?P<n3>[^.,;!?\n]+))"));
re!(re_humor, r"(?i)^(?:jarvis[,. ]+)?(?:/humor|(?:aumenta|diminui|muda|coloca|p[õo]e|define|ajusta|seta|deixa)\s+(?:o\s+)?humor\b)(?:[^\d\n]*?(?P<n>\d{1,3})\s*%?)?[^\n]*$");
re!(re_resumo_wpp, concat!(
    r"(?is)\b(?:resum\w+|o\s+que\s+(?:rolou|aconteceu|houve|teve|chegou)|novidades?|como\s+(?:foi|est[aá])|me\s+(?:atualiza|conta|fala))\b.*\b(?:whats(?:app)?|wpp|zap)\b",
    r"|\b(?:whats(?:app)?|wpp|zap)\b.*\b(?:resum\w+|o\s+que\s+(?:rolou|aconteceu|houve|teve)|novidades?)\b"));
re!(re_horas, r"(?:[uú]ltim[ao]s?\s+)?(\d{1,3})\s*h(?:oras?)?\b");
re!(re_sobre_contato, concat!(
    r"(?i)\bo\s+que\s+(?:a\s+|o\s+)?(?P<nome>[^?.!,]{2,40}?)\s+(?:me\s+|te\s+)?(?:perguntou|disse|mandou|falou|pediu|escreveu|respondeu|quer(?:ia)?)\b",
    r"|\b[uú]ltimas?\s+mensagens?\s+(?:d[aeo]s?|do|da)\s+(?:a\s+|o\s+)?(?P<nome2>[^?.!,]{2,40})"));
re!(re_sobre_contato_fim, r"(?i)\s+(?:por\s+[uú]ltimo|ontem|hoje|agora|mesmo)$");
// e-mail: SO LEITURA
re!(re_email_mudar, r"(?i)\b(?:manda\w*|envia\w*|escrev\w*|respond\w*|encaminh\w*|apag\w*|exclu\w*|delet\w*|arquiv\w*)\b[^\n]{0,40}\be-?mails?\b");
re!(re_email_ler, r"(?i)\b(?:l[êe]|ler|abr\w*|mostr\w*)\b[^\n]{0,30}\be-?mails?\b[^\d\n]{0,15}(?P<n>\d{1,2})\b");
re!(re_email_busca, concat!(
    r"(?i)\b(?:procur\w*|busc\w*|ach\w*|encontr\w*|pesquis\w*)\b[^\n]{0,20}\be-?mails?\b\s*(?:d[aeo]s?|sobre|com|do\s+assunto)?\s*(?P<q>[^?.!\n]+)",
    r"|\b(?:tem|chegou|recebi|veio)\b[^\n]{0,15}\be-?mails?\s+(?:d[aeo]s?|sobre)\s+(?P<q2>[^?.!\n]+)"));
re!(re_email_vagas, r"(?i)\bvagas?\b[^\n]{0,40}\b(?:e-?mails?|gmail)\b|\b(?:e-?mails?|gmail)\b[^\n]{0,40}\bvagas?\b");
re!(re_email_novos, r"(?i)\be-?mails?\b|\bgmail\b|\bcaixa\s+de\s+entrada\b");
re!(re_email_novos2, r"(?i)\b(?:novo|nova|novos|novas|n[ãa]o\s*lid\w*|chegou|chegaram|tenho|tem|olh\w*|v[êe]|confer\w*|resum\w*|caixa|inbox|notícia|noticia)\b");
re!(re_pessoal, concat!(
    r"(?i)\b(sobre\s+mim|de\s+mim|me\s+conhece|meu\s+perfil|minha\s+vida|minha\s+hist[oó]ria|meus?\s+projetos?|",
    r"meus?\s+(?:gostos|interesses|objetivos|sonhos|planos|pontos\s+fortes|defeitos)|meu\s+jeito|minha\s+personalidade|me\s+descreve|me\s+descreva)\b"));
re!(re_quem_sou, r"(?i)\bquem\s+(?:eu\s+)?sou\b");
re!(re_no_zap, r"(?i)^\s+(?:eu\s+)?no\s+(?:zap|whats|wpp)");
re!(re_imagem_de, concat!(
    r"(?i)\b(?:olha|veja|v[eê]|confere|confira|abre|l[eê]|descreve|ver)\b[^?.!\n]{0,40}?\b(?:foto|imagem|print|captura|screenshot)s?\b[^?.!\n]{0,25}?",
    r"\b(?:que|do|da|de)\s+(?:o\s+|a\s+)?(?P<nome>\w+(?:\s+\w+){0,2}?)(?:\s+(?:mandou|enviou|te\s+mandou|me\s+mandou))?\s*[?.!]?$|",
    r"\b(?:olha|veja|v[eê]|confere|confira|ver)\b\s+(?:o\s+)?que\s+(?:o\s+|a\s+)?(?P<nome2>\w+(?:\s+\w+){0,2}?)\s+(?:mandou|enviou)\b[^?.!\n]{0,20}\b(?:foto|imagem|print|captura)s?\b"));
re!(re_mandou_fim, r"(?i)\s+(?:mandou|enviou)$");
re!(re_agenda, concat!(
    r"(?i)\b(?:o\s+que\s+(?:eu\s+)?tenho|quais?\s+(?:s[aã]o\s+)?(?:meus?|minhas?)|mostra|v[eê]|lista|abre|como\s+t[aá])\b[^?]{0,30}?",
    r"\b(?:agenda|compromissos?|tarefas|rotina)\b(?:[^?]*?\b(?P<dia>hoje|amanh[aã]|depois de amanh[aã]|segunda|ter[cç]a|quarta|quinta|sexta|s[aá]bado|domingo|\d{1,2}/\d{1,2}))?"));
re!(re_agenda_verbo, r"(?i)^(?:jarvis\W*)?(?:mostra|ve|v[eê]|lista|abre)\b");
re!(re_semana, r"(?i)\bsemana\b");
re!(re_ja_respondi, r"(?i)\bj[aá]\b.{0,25}\brespond|\brespondi\b|\bj[aá] foi respondid");
re!(re_sobre_si, concat!(
    r"(?i)quem\s+[eé]\s+(?:voc[eê]|tu)\b|(?:do|o)\s+que\s+(?:voc[eê]|o\s+braniac)\s+[eé]\b|o\s+que\s+[eé]\s+o\s+braniac\b|como\s+(?:voc[eê]|o\s+braniac)\s+foi\s+(?:feit|cri|program)|quem\s+(?:te|o)\s+(?:fez|criou)|",
    r"(?:resumo|explica\w*|fala\w*|conta\w*).{0,40}(?:de|sobre)\s+(?:voc[eê]|si\s+mesmo|o\s+braniac)\b",
    r"|como\s+(?:voc[eê]|o\s+braniac|o\s+jarvis)\s+(?:funciona|trabalha|escolhe|decide|economiza|gasta|roteia)|economiz\w*\s+(?:de\s+)?tokens"));
re!(re_zap, r"(?i)whats\s?app|\bzap\b|\bwpp\b");
re!(re_ver, r"(?i)\b(olh\w*|v[eê]|ve[rj]\w*|l[eê]\w*|leia|checa\w*|confer\w*|novas?|n[aã]o\s+lidas?|mensagens?|chegou|recebi)\b");
// o que vem depois do nome era um lookahead no Python: aqui e consumido (so o nome interessa)
re!(re_conversa_de, r"(?i)(?:conversa|mensagens?|papo)\s+(?:d[aeo]s?|com)\s+(?:a\s+|o\s+)?(?P<nome>[^?.!,]+?)(?:\s+(?:n[ao]|pelo|d[oa])\s+(?:whats\s?app|zap|wpp)|\s+pra\s+mim|[?.!,]|$)");
re!(re_envio_inventado, r"(?i)/sim|confirm\w*\s+(?:o\s+)?envio|preparei\s+a\s+mensagem");
re!(re_duvida, r"(?is)^\s*\[D[ÚU]VIDA\]\s*(?P<pergunta>.+)$");
re!(re_mal, r"(?i)crise de ansiedade|ansiedade|n[aã]o estou (?:me sentindo )?bem|n[aã]o t[oô] bem|depress|desesper|n[aã]o aguento mais|sem sa[ií]da|estou mal|t[oô] mal");

fn corta(t: &str, n: usize) -> String {
    t.chars().take(n).collect()
}

/// So o que o dono disse AGORA (sem o "[Conversa recente no WhatsApp: ...]" que o chat dele anexa para a IA).
pub fn sem_contexto(t: &str) -> &str {
    // o contexto do WhatsApp e as instrucoes do projeto (b2) nao sao fala da pessoa: atalho por palavra-chave nao pode le-los
    match [t.find(CONTEXTO_ANEXADO), t.find("\n\n[Projeto:")].into_iter().flatten().min() {
        Some(i) if i > 0 => t[..i].trim(),
        _ => t,
    }
}

fn nome_texto(c: regex::Captures) -> (String, String) {
    (c["nome"].trim().to_string(), c["texto"].trim_matches([' ', ':', ',', '-']).to_string())
}

pub fn intencao_envio(t: &str) -> Option<(String, String)> {
    re_envio().captures(t).map(nome_texto)
}

pub fn intencao_audio(t: &str) -> Option<(String, String)> {
    re_audio().captures(t).map(nome_texto)
}

pub fn intencao_aviso(t: &str) -> Option<(String, String)> {
    re_aviso().captures(t).map(nome_texto)
}

/// A frase e uma ORDEM de envio (texto ou audio) a alguem: os atalhos de leitura nao podem engolir isso.
pub fn eh_envio(t: &str) -> bool {
    re_envio().is_match(t) || re_aviso().is_match(t) || re_audio().is_match(t)
}

#[derive(Debug, PartialEq)]
pub enum Direto {
    Arquivo { nome: String, caminho: String, legenda: String },
    Audio { nome: String, texto: String },
    Texto { nome: String, texto: String },
}

/// Ordem de envio reconhecida sem IA, na ordem do Python: /arquivo, audio, /enviar, "manda ... dizendo", "avisa ... que".
pub fn direto(t: &str) -> Option<Direto> {
    if let Some(c) = re_cmd_arquivo().captures(t) {
        return Some(Direto::Arquivo { nome: c["nome"].trim().into(), caminho: c["caminho"].trim().into(), legenda: c.name("legenda").map_or("", |m| m.as_str()).trim().into() });
    }
    if let Some((nome, texto)) = intencao_audio(t) {
        return Some(Direto::Audio { nome, texto });
    }
    re_cmd_enviar().captures(t).map(nome_texto).or_else(|| intencao_envio(t)).or_else(|| intencao_aviso(t)).map(|(nome, texto)| Direto::Texto { nome, texto })
}

/// "pede pro Claude ..." / "/claude ...": a tarefa.
pub fn claude_tarefa(t: &str) -> Option<String> {
    re_claude().captures(t).map(|c| c["tarefa"].trim().to_string())
}

/// "errou" (ou a outra expressao de erro) em frase curta: a ultima resposta do Jarvis estava errada.
pub fn eh_erro(t: &str) -> bool {
    re_erro().is_match(t) && t.chars().count() <= 80 && !t.starts_with('/')
}

/// Numero solto, mandado so para guardar: o Jarvis pergunta o que fazer em vez de inventar.
pub fn numero_solto(t: &str) -> bool {
    re_numero().is_match(t.trim()) && t.chars().filter(|c| c.is_numeric()).count() >= 7
}

pub fn horas_do_pedido(t: &str) -> f64 {
    let t = t.to_lowercase();
    if let Some(n) = re_horas().captures(&t).and_then(|c| c[1].parse::<f64>().ok()) {
        return n;
    }
    if t.contains("última hora") || t.contains("ultima hora") { 1.0 } else if t.contains("semana") { 168.0 } else if t.contains("ontem") { 48.0 } else { 24.0 }
}

pub fn pede_resumo_wpp(t: &str) -> bool {
    re_resumo_wpp().is_match(t)
}

/// 'o que a Ana me perguntou?' / 'ultimas mensagens da Ana': o nome dito.
pub fn sobre_contato(t: &str) -> Option<String> {
    let c = re_sobre_contato().captures(t)?;
    let nome = c.name("nome").or_else(|| c.name("nome2")).map_or("", |m| m.as_str()).trim();
    Some(re_sobre_contato_fim().replace(nome, "").into_owned())
}

/// Pedido imperativo ('olha o whatsapp e responde as pessoas'); pergunta ('voce consegue responder?') nao conta.
pub fn quer_atender(t: &str) -> bool {
    re_atender().is_match(t) && !t.contains('?') && !re_consegue().is_match(t)
}

/// "para de mandar audio pra X" / "X nao entende audio" / "so texto pra X": quem passa a receber so texto.
pub fn sem_audio(t: &str) -> Option<String> {
    let c = re_sem_audio().captures(t)?;
    if re_audio().is_match(t) {
        return None;
    }
    let quem = c.name("n1").or_else(|| c.name("n2")).or_else(|| c.name("n3")).map_or("", |m| m.as_str()).trim_matches([' ', '.', ',', ';', '!', '?']);
    let quem = quem.strip_prefix("jarvis").or_else(|| quem.strip_prefix("Jarvis")).map_or(quem, |r| r.trim_start().trim_start_matches([',', ':']).trim());
    (!quem.is_empty() && quem.chars().count() <= 40).then(|| quem.to_string())
}

/// '/humor 50', 'aumenta o humor pra 50%': Some(None) = so consulta; Some(Some(n)) = ajusta.
pub fn humor_cmd(t: &str) -> Option<Option<i64>> {
    if t.chars().count() > 90 {
        return None;
    }
    re_humor().captures(t).map(|c| c.name("n").and_then(|m| m.as_str().parse().ok()))
}

#[derive(Debug, PartialEq)]
pub enum Email {
    /// pediu para enviar/responder/apagar: recusa fixa
    Mudar,
    Vagas,
    Ler(String),
    Buscar(String),
    Novos,
}
pub const EMAIL_SO_LEITURA: &str = "Por segurança eu só LEIO e-mails: não envio, não respondo, não apago nem arquivo nada.";

/// Assunto de e-mail? None = nao e. `Mudar` responde com `EMAIL_SO_LEITURA`; os demais passam pela trava de origem antes de ler.
pub fn email_pedido(t: &str) -> Option<Email> {
    if re_email_mudar().is_match(t) {
        return Some(Email::Mudar);
    }
    let vagas = re_email_vagas().is_match(t);
    let ler = re_email_ler().captures(t).map(|c| c["n"].to_string());
    let busca = re_email_busca().captures(t).map(|c| c.name("q").or_else(|| c.name("q2")).map_or("", |m| m.as_str()).trim().to_string());
    let novos = re_email_novos().is_match(t) && re_email_novos2().is_match(t);
    if !(vagas || ler.is_some() || busca.is_some() || novos) {
        return None;
    }
    let consulta = busca.filter(|q| !q.is_empty());
    Some(if vagas && ler.is_none() && consulta.is_none() {
        Email::Vagas
    } else if let Some(n) = ler {
        Email::Ler(n)
    } else if let Some(q) = consulta {
        Email::Buscar(q)
    } else {
        Email::Novos
    })
}

/// 'pergunta pro Grok ...': (ia, pergunta). O nome da IA dentro de um texto a ENVIAR a alguem nao conta.
pub fn ia_pedido(t: &str) -> Option<(String, String)> {
    let c = re_ia().captures(t)?;
    if re_audio().is_match(t) || re_envio().is_match(t) || intencao_aviso(t).is_some_and(|(nome, _)| !re_nome_ia().is_match(&nome)) {
        return None;
    }
    let ia = c["ia"].to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ");
    let ia = if ia == "gemini" || ia == "grok" { ia } else { "chatgpt".to_string() };
    let q = re_e_me_diz().replace(c["q"].trim(), "").trim_matches([' ', '.', ',', ';', ':']).to_string();
    Some((ia, q))
}

/// Pergunta PESSOAL do dono ('o que voce sabe sobre mim?'); 'quem sou eu no zap' nao conta.
pub fn pergunta_pessoal(t: &str) -> bool {
    let quem_sou = re_quem_sou().find_iter(t).any(|m| !re_no_zap().is_match(&t[m.end()..]));
    (re_pessoal().is_match(t) || quem_sou) && !t.starts_with('/') && t.chars().count() >= 12 && !eh_envio(t) && !re_claude().is_match(t)
}

/// 'olha a foto/print que o Fulano mandou': o nome dito (quem chama confere se e um contato conhecido).
pub fn imagem_de(t: &str) -> Option<String> {
    let c = re_imagem_de().captures(t.trim())?;
    let nome = c.name("nome").or_else(|| c.name("nome2")).map_or("", |m| m.as_str()).trim();
    let nome = re_mandou_fim().replace(nome, "").into_owned();
    (!nome.is_empty()).then_some(nome)
}

/// 'o que tenho na agenda amanha?' / 'mostra a agenda': (dia, quantos dias).
pub fn agenda_ver(t: &str) -> Option<(String, i64)> {
    let c = re_agenda().captures(t)?;
    (t.contains('?') || re_agenda_verbo().is_match(t)).then(|| (c.name("dia").map_or("", |m| m.as_str()).to_string(), if re_semana().is_match(t) { 7 } else { 1 }))
}

pub fn ja_respondi(t: &str) -> bool {
    re_ja_respondi().is_match(t) && t.chars().count() < 220
}

pub fn sobre_si(t: &str) -> bool {
    re_sobre_si().is_match(t)
}

/// Reserva sem IA: 'olha meu whatsapp' -> Some(None); 'le a conversa da Ana no whatsapp' -> Some(Some("Ana")).
pub fn zap_ver(t: &str) -> Option<Option<String>> {
    (re_zap().is_match(t) && re_ver().is_match(t)).then(|| re_conversa_de().captures(t).map(|c| c["nome"].trim().to_string()))
}

/// A IA disse que "preparou o envio" sem ter chamado a ferramenta.
pub fn envio_inventado(resposta: &str) -> bool {
    re_envio_inventado().is_match(resposta)
}

/// A IA sinalizou "[DUVIDA] pergunta": a pergunta.
pub fn duvida(resposta: &str) -> Option<String> {
    re_duvida().captures(resposta).map(|c| c["pergunta"].trim().to_string())
}

/// Desabafo/crise: nunca cai na cadeia de ferramentas.
pub fn mal(t: &str) -> bool {
    re_mal().is_match(t)
}

/// Guarda (pedido, resposta errada, reclamacao) em dados/erros_jarvis.jsonl, a regressao do avaliador. `hist` = ultimas falas
/// ({"role","content"}, mais antiga primeiro); `tipos(pedido, resposta)` classifica o erro.
pub fn registrar_erro(dados: &Path, hist: &[Value], t: &str, ts: f64, tipos: &dyn Fn(&str, &str) -> Vec<String>) -> String {
    let fala = |m: &Value| m["content"].as_str().unwrap_or("").to_string();
    let Some(i) = hist.iter().rposition(|m| m["role"] == "assistant") else {
        return "Anotei que algo estava errado, mas não achei a resposta anterior para guardar. Me diz o que você tinha pedido?".into();
    };
    let resp = fala(&hist[i]);
    if resp.is_empty() {
        return "Anotei que algo estava errado, mas não achei a resposta anterior para guardar. Me diz o que você tinha pedido?".into();
    }
    let ped = hist[..i].iter().rev().find(|m| m["role"] == "user").map(fala).unwrap_or_default();
    let ped = ped.split(CONTEXTO_ANEXADO).next().unwrap_or("").trim().to_string();
    let linha = json!({ "ts": ts, "pedido": corta(&ped, 600), "resposta_errada": corta(&resp, 600), "nota": corta(t, 200), "tipos": tipos(&ped, &resp) });
    let gravou = std::fs::create_dir_all(dados).and_then(|_| std::fs::OpenOptions::new().create(true).append(true).open(dados.join("erros_jarvis.jsonl")))
        .and_then(|mut f| writeln!(f, "{linha}"));
    if gravou.is_err() {
        return "Tentei anotar esse erro mas não consegui gravar o arquivo. Me avisa se isso se repetir.".into();
    }
    "Anotado: guardei a sua pergunta e a minha resposta errada no teste automático, para eu não repetir. Se quiser, me diz o que eu devia ter feito.".into()
}

/// Tudo que sai para fora nos envios. Quem chama decide o que cada closure faz (nos testes, so anota).
pub struct Saidas<'a> {
    /// nomes das conversas visiveis, ou o texto do erro
    pub listar_nomes: &'a dyn Fn() -> Result<Vec<String>, String>,
    /// busca na agenda inteira do aplicativo
    pub buscar_contatos: &'a dyn Fn(&str) -> Vec<String>,
    /// pergunta ao agente de programacao (ja dentro do teto diario); Some so quando ele soube responder
    pub perguntar_claude: &'a dyn Fn(&str) -> Option<String>,
    /// (nome exato, texto) -> resultado; comeca com ✅ quando saiu. A closure confere o destino (`trava_destino`) antes de enviar.
    pub enviar_texto: &'a dyn Fn(&str, &str) -> String,
    /// (nome exato, texto a falar) -> resultado
    pub enviar_audio: &'a dyn Fn(&str, &str) -> String,
    /// atalho, caminho ou parte do nome -> arquivo liberado, ou o motivo da recusa
    pub resolver_arquivo: &'a dyn Fn(&str) -> Result<PathBuf, String>,
    /// (chave de busca do contato, arquivo, legenda) -> resultado
    pub enviar_arquivo: &'a dyn Fn(&str, &Path, &str) -> String,
}

/// Nome exato do contato, sem NUNCA listar contatos nem perguntar "quem e?": conversas visiveis -> agenda inteira -> agente.
/// Err = a frase que explica (erro do aplicativo ou "nao achei").
pub fn resolver_contato(dados: &Path, ctx: &Ctx, s: &Saidas, nome: &str) -> Result<String, String> {
    let visiveis = (s.listar_nomes)()?;
    let cands = resolver_nome(ctx, nome, &visiveis);
    if let Some(c) = melhor_candidato(ctx, nome, &cands) {
        return Ok(if cands.len() == 1 { cands[0].clone() } else { c });
    }
    let achados = (s.buscar_contatos)(nome);
    if let Some(escolhido) = melhor_candidato(ctx, nome, &achados) {
        if achados.len() == 1 {
            let _ = contexto::registrar_contato(dados, nome, &escolhido); // aprende SO quando nao havia duvida
        }
        return Ok(escolhido);
    }
    let dono = if ctx.dono.is_empty() { "dono".to_string() } else { ctx.dono.clone() };
    let pergunta = format!("Qual destes contatos do WhatsApp do {dono} é '{nome}'? Contatos: {visiveis:?}. Responda SOMENTE com o nome exato de um contato da lista, ou NAO SEI.");
    if let Some(resp) = (s.perguntar_claude)(&pergunta) {
        let achado = resolver_nome(ctx, resp.trim_matches([' ', '.', '"', '\'']), &visiveis);
        if achado.len() == 1 {
            let _ = contexto::registrar_contato(dados, nome, &achado[0]);
            return Ok(achado[0].clone());
        }
    }
    Err(format!("Procurei '{nome}' na agenda inteira do WhatsApp e não achei. Se me mandar o número com DDD eu salvo e tento por ele."))
}

fn com_o_dito(r: String, texto: &str) -> String {
    if r.starts_with('✅') { format!("{r} Disse: «{}»", corta(texto, 160)) } else { r }
}

/// Manda o texto NA HORA (sem pedir confirmacao). So o DADO sensivel nao sai; falar de pagamento/cartao pode.
pub fn enviar_texto(dados: &Path, ctx: &Ctx, s: &Saidas, nome: &str, texto: &str) -> String {
    let exato = match resolver_contato(dados, ctx, s, nome) {
        Ok(e) => e,
        Err(m) => return m,
    };
    let texto = tratamento::corrigir(texto, &tratamento::regras(dados));
    if tem_dado_real(&texto) {
        return "Não mandei: a mensagem tem um dado sensível (número de cartão, CPF, senha ou conta), e esse tipo de dado eu não envio.".into();
    }
    com_o_dito((s.enviar_texto)(&exato, &texto), &texto)
}

fn igual(a: &str, b: &str) -> bool {
    let n = |x: &str| x.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ");
    n(a) == n(b)
}

/// Manda um AUDIO na hora. Quem so recebe texto (preferencia salva) ou o dono com o mudo ligado recebe por escrito.
pub fn enviar_audio(dados: &Path, ctx: &Ctx, ts: f64, s: &Saidas, nome: &str, texto: &str) -> String {
    let exato = match resolver_contato(dados, ctx, s, nome) {
        Ok(e) => e,
        Err(m) => return m,
    };
    let texto = tratamento::corrigir(texto, &tratamento::regras(dados));
    if tem_dado_real(&texto) {
        return "Não mandei: o áudio teria um dado sensível (número de cartão, CPF, senha ou conta), e esse tipo de dado eu não envio.".into();
    }
    let eh_dono = ctx.chat_do_dono().is_some_and(|c| igual(c, &exato));
    let mudo_dono = eh_dono && mudo::bloqueia_audio_dono(dados, ts);
    if (!eh_dono && ctx.preferencia(&exato) == "texto") || mudo_dono {
        let r = com_o_dito((s.enviar_texto)(&exato, &texto), &texto);
        return if mudo_dono { format!("(MUDO ligado: sem áudio no WhatsApp para o dono, então mandei por escrito) {r}") } else { format!("({exato} só recebe texto, então mandei por escrito) {r}") };
    }
    com_o_dito((s.enviar_audio)(&exato, &texto), &texto)
}

/// Manda um arquivo na hora. O caminho passa por `resolver_arquivo` (so pastas liberadas); a entrega fica no historico do contato.
pub fn enviar_arquivo(dados: &Path, ctx: &Ctx, ts: f64, s: &Saidas, nome: &str, caminho: &str, legenda: &str) -> String {
    let exato = match resolver_contato(dados, ctx, s, nome) {
        Ok(e) => e,
        Err(m) => return m,
    };
    if tem_dado_real(legenda) {
        return "Não mandei: a legenda tem um dado sensível (número de cartão, CPF, senha ou conta).".into();
    }
    let arq = match (s.resolver_arquivo)(caminho) {
        Ok(a) => a,
        Err(e) => return format!("Não mandei: {e}"),
    };
    let r = (s.enviar_arquivo)(&exato, &arq, legenda);
    if r.starts_with('✅') {
        // o atendimento precisa saber o que ja foi entregue a essa pessoa
        let arquivo = arq.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let _ = memoria::registrar_msg(dados, &exato, "C", format!("[arquivo {arquivo}] {legenda}").trim(), "whatsapp", false, ts);
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contexto::norm;
    use std::cell::RefCell;

    fn pasta(n: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-jarvis_comandos-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn ctx() -> Ctx {
        let mut c = Ctx { dono: "Davi".into(), ..Ctx::default() };
        for (a, n) in [("deusa bia", "Beatriz Lima 🤍"), ("davi", "Eu(SP)")] {
            c.contatos.insert(norm(a), n.to_string());
        }
        c
    }

    fn nomes() -> Result<Vec<String>, String> {
        Ok(["Beatriz Lima 🤍", "mainha", "Banco X", "Eduarda", "Eu(SP)"].iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn frase_longa_nao_e_nome_de_contato() {
        let frase = "Pergunta de astronomia, responda só com fatos, em português do Brasil: liste 6 curiosidades. Diga também se Órion é constelação ou galáxia.";
        assert!(!eh_envio(frase) && direto(frase).is_none());
        assert!(intencao_aviso(&format!("fala {}que sim", "palavra ".repeat(9))).is_none()); // "nome" com mais de 40 letras
        assert_eq!(intencao_aviso("avisa a Deusa Bia que cheguei").unwrap(), ("Deusa Bia".to_string(), "cheguei".to_string()));
        assert_eq!(intencao_envio("manda uma mensagem pra Deusa Bia dizendo que cheguei").unwrap().0, "Deusa Bia");
        assert_eq!(intencao_audio("manda um áudio para Contato Fz dizendo oi").unwrap(), ("Contato Fz".to_string(), "oi".to_string()));
        assert_eq!(intencao_aviso("Jarvis, fala pra minha mãe que o hóspede esqueceu a carteira na suíte").unwrap().0, "minha mãe");
    }

    #[test]
    fn comandos_diretos_na_ordem_certa() {
        assert_eq!(direto("/enviar mainha | oi mãe").unwrap(), Direto::Texto { nome: "mainha".into(), texto: "oi mãe".into() });
        assert_eq!(direto("/arquivo Ana|C:/x/video.mp4|segue o vídeo").unwrap(), Direto::Arquivo { nome: "Ana".into(), caminho: "C:/x/video.mp4".into(), legenda: "segue o vídeo".into() });
        assert_eq!(direto("/arquivo Ana | guia.pdf").unwrap(), Direto::Arquivo { nome: "Ana".into(), caminho: "guia.pdf".into(), legenda: String::new() });
        // 'manda um audio pra X dizendo' e audio, nao mensagem
        assert_eq!(direto("manda um áudio pra Ana dizendo: bom dia").unwrap(), Direto::Audio { nome: "Ana".into(), texto: "bom dia".into() });
        assert_eq!(direto("avisa a Ana que cheguei").unwrap(), Direto::Texto { nome: "Ana".into(), texto: "cheguei".into() });
        assert!(direto("oi, tudo bem?").is_none() && direto("/enviar sem barra").is_none());
        assert_eq!(claude_tarefa("pede pro Claude abrir o painel e ver as reservas de hoje").unwrap(), "abrir o painel e ver as reservas de hoje");
        assert_eq!(claude_tarefa("/claude faz algo").unwrap(), "faz algo");
        assert!(claude_tarefa("o claude é bom?").is_none());
    }

    #[test]
    fn erro_numero_solto_e_atalhos_pequenos() {
        assert!(eh_erro("errou") && eh_erro("SEU CU") && !eh_erro("o resto do assunto") && !eh_erro("/errou"));
        let longa = "o Jarvis errou ontem na agenda mas hoje ele acertou tudo direitinho e eu gostei bastante do resultado final";
        assert!(longa.len() > 80 && !eh_erro(longa)); // frase longa nao dispara
        assert!(numero_solto("11999990000") && numero_solto("+55 (11) 99999-0000"));
        assert!(!numero_solto("me lembra às 15") && !numero_solto("123") && !numero_solto("liga pro 11999990000"));
        assert_eq!(horas_do_pedido("o que rolou nas últimas 6 horas no zap"), 6.0);
        assert_eq!(horas_do_pedido("resumo da semana"), 168.0);
        assert_eq!(horas_do_pedido("resumo do whatsapp"), 24.0);
        assert!(pede_resumo_wpp("o que rolou no whatsapp hoje?") && !pede_resumo_wpp("bom dia"));
        assert_eq!(sobre_contato("o que a Ana me perguntou ontem?").unwrap(), "Ana");
        assert!(quer_atender("olha o whatsapp e responde as pessoas") && !quer_atender("você consegue responder as pessoas no whatsapp?"));
        assert_eq!(sem_audio("para de mandar áudio pra Ana").unwrap(), "Ana");
        assert_eq!(sem_audio("a Ana não entende áudio").unwrap(), "Ana");
        assert!(sem_audio("manda um áudio pra Ana dizendo que só texto pra Bia").is_none());
        assert_eq!(humor_cmd("/humor 50"), Some(Some(50)));
        assert_eq!(humor_cmd("/humor"), Some(None));
        assert_eq!(humor_cmd("aumenta o humor pra 80%"), Some(Some(80)));
        assert_eq!(humor_cmd("que humor é esse"), None);
        assert_eq!(duvida("[DÚVIDA] Quem é o Nando?").unwrap(), "Quem é o Nando?");
        assert!(envio_inventado("Preparei a mensagem, confirma o envio?") && !envio_inventado("Enviei."));
        assert_eq!(sem_contexto("sim\n\n[Conversa recente no WhatsApp: Jarvis: troco?]"), "sim");
    }

    #[test]
    fn email_e_so_leitura_e_ia_parceira_nao_e_contato() {
        assert_eq!(email_pedido("responde o e-mail do banco"), Some(Email::Mudar));
        assert_eq!(email_pedido("tenho e-mail novo?"), Some(Email::Novos));
        assert_eq!(email_pedido("lê o e-mail 2"), Some(Email::Ler("2".into())));
        assert_eq!(email_pedido("procura e-mail da escola"), Some(Email::Buscar("escola".into())));
        assert_eq!(email_pedido("chegou alguma vaga no gmail?"), Some(Email::Vagas));
        assert_eq!(email_pedido("bom dia"), None);
        assert_eq!(ia_pedido("pergunta pro Grok quando foi a copa de 94").unwrap(), ("grok".to_string(), "quando foi a copa de 94".to_string()));
        assert_eq!(ia_pedido("consulta o chat gpt: qual a capital da Bolívia").unwrap().0, "chatgpt");
        // o nome da IA esta no TEXTO a enviar: e envio, nao pergunta a ela
        assert!(ia_pedido("manda um áudio pra Ana dizendo que o ChatGPT caiu").is_none());
        assert!(ia_pedido("fala pra Ana que o gemini voltou").is_none());
        assert!(pergunta_pessoal("o que você sabe sobre mim?") && pergunta_pessoal("me diz quem sou eu de verdade"));
        assert!(!pergunta_pessoal("quem sou eu no zap?") && !pergunta_pessoal("sobre mim"));
        assert_eq!(imagem_de("olha a foto que o Nando mandou").unwrap(), "Nando");
        assert!(imagem_de("bom dia").is_none());
        assert_eq!(agenda_ver("o que eu tenho na agenda amanhã?").unwrap(), ("amanhã".to_string(), 1));
        assert_eq!(agenda_ver("mostra a agenda da semana").unwrap().1, 7);
        assert!(agenda_ver("coloca na agenda o dentista").is_none());
        assert_eq!(zap_ver("tem mensagem nova no zap?"), Some(None));
        assert_eq!(zap_ver("lê a conversa da Beatriz no whatsapp"), Some(Some("Beatriz".to_string())));
        assert_eq!(zap_ver("oi, tudo bem?"), None);
        assert!(sobre_si("como você funciona por dentro?") && !sobre_si("como funciona o motor do carro?"));
        assert!(mal("tô mal hoje") && ja_respondi("já respondi o hóspede"));
    }

    #[test]
    fn erro_anotado_com_o_pedido_e_a_resposta() {
        let d = pasta("erro");
        let hist = vec![json!({"role": "user", "content": "toca a playlist da festa\n\n[Conversa recente no WhatsApp: x]"}), json!({"role": "assistant", "content": "Mandei."}), json!({"role": "user", "content": "errou"})];
        let r = registrar_erro(&d, &hist, "errou", 10.0, &|_, _| vec!["acao".into()]);
        assert!(r.starts_with("Anotado"));
        let l: Value = serde_json::from_str(std::fs::read_to_string(d.join("erros_jarvis.jsonl")).unwrap().trim()).unwrap();
        assert_eq!((l["pedido"].as_str(), l["resposta_errada"].as_str(), l["nota"].as_str(), l["tipos"][0].as_str()), (Some("toca a playlist da festa"), Some("Mandei."), Some("errou"), Some("acao")));
        assert!(registrar_erro(&d, &[json!({"role": "user", "content": "oi"})], "errou", 1.0, &|_, _| vec![]).contains("não achei a resposta anterior"));
        let _ = std::fs::remove_dir_all(&d);
    }

    struct Fake {
        enviados: RefCell<Vec<(String, String, String)>>,
        busca: Vec<String>,
        claude: Option<String>,
    }

    fn rodar<T>(f: &Fake, corpo: impl FnOnce(&Saidas) -> T) -> T {
        let s = Saidas {
            listar_nomes: &nomes,
            buscar_contatos: &|_| f.busca.clone(),
            perguntar_claude: &|_| f.claude.clone(),
            enviar_texto: &|n, t| { f.enviados.borrow_mut().push(("texto".into(), n.into(), t.into())); format!("✅ Mensagem enviada para {n}.") },
            enviar_audio: &|n, t| { f.enviados.borrow_mut().push(("audio".into(), n.into(), t.into())); format!("✅ Áudio enviado para {n}.") },
            resolver_arquivo: &|c| if c.contains(".env") { Err("Esse arquivo guarda segredos".into()) } else { Ok(PathBuf::from("C:/entregas").join(c)) },
            enviar_arquivo: &|n, a, l| { f.enviados.borrow_mut().push(("arquivo".into(), n.into(), format!("{}|{l}", a.display()))); "✅ ok".into() },
        };
        corpo(&s)
    }

    fn fake() -> Fake {
        Fake { enviados: RefCell::new(vec![]), busca: vec![], claude: None }
    }

    #[test]
    fn envio_sai_na_hora_para_o_contato_certo() {
        let (d, c, f) = (pasta("envio"), ctx(), fake());
        let r = rodar(&f, |s| enviar_texto(&d, &c, s, "Deusa Bia", "cheguei"));
        assert!(r.starts_with("✅") && r.contains("Disse: «cheguei»"));
        assert_eq!(f.enviados.borrow()[0], ("texto".into(), "Beatriz Lima 🤍".into(), "cheguei".into()));
        rodar(&f, |s| enviar_texto(&d, &c, s, "mainha", "oi mãe"));
        assert_eq!(f.enviados.borrow()[1].1, "mainha");
        // dado sensivel nao sai; falar do assunto pode
        assert!(rodar(&f, |s| enviar_texto(&d, &c, s, "mainha", "a senha é abc123")).starts_with("Não mandei"));
        assert_eq!(f.enviados.borrow().len(), 2);
        assert!(rodar(&f, |s| enviar_texto(&d, &c, s, "mainha", "pode pagar no cartão")).starts_with("✅"));
        // erro do aplicativo volta como esta e nada e enviado
        let s = Saidas { listar_nomes: &|| Err("WhatsApp fechado".into()), buscar_contatos: &|_| vec![], perguntar_claude: &|_| None, enviar_texto: &|_, _| panic!("nao envia"),
            enviar_audio: &|_, _| panic!(), resolver_arquivo: &|_| panic!(), enviar_arquivo: &|_, _, _| panic!() };
        assert_eq!(enviar_texto(&d, &c, &s, "mainha", "oi"), "WhatsApp fechado");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn contato_desconhecido_nao_lista_nem_pergunta_e_aprende_quando_acha() {
        let (d, c) = (pasta("contato"), ctx());
        let f = fake();
        let r = rodar(&f, |s| enviar_texto(&d, &c, s, "Nando", "bom dia"));
        assert!(r.contains("Procurei 'Nando'") && !r.contains("1)") && f.enviados.borrow().is_empty());
        // achado pela busca da agenda (um so): envia e aprende
        let f = Fake { busca: vec!["Manhinha 🏍".into()], ..fake() };
        assert!(rodar(&f, |s| enviar_texto(&d, &c, s, "manhinha", "oi, como você está?")).starts_with("✅"));
        assert_eq!(f.enviados.borrow()[0].1, "Manhinha 🏍");
        assert_eq!(Ctx::ler(&d).contato("manhinha"), Some("Manhinha 🏍"));
        // varios parecidos na busca: escolhe o melhor, mas NAO vira apelido
        let f = Fake { busca: vec!["Rita Obras".into(), "Rita Vendas".into()], ..fake() };
        rodar(&f, |s| enviar_texto(&d, &c, s, "rita vendas", "oi"));
        assert_eq!(f.enviados.borrow()[0].1, "Rita Vendas");
        assert_eq!(Ctx::ler(&d).contato("rita vendas"), None);
        // o agente resolve o apelido: envia e aprende
        let f = Fake { claude: Some("Eduarda.".into()), ..fake() };
        assert!(rodar(&f, |s| enviar_texto(&d, &c, s, "minha namorada", "oi")).contains("Eduarda"));
        assert_eq!(Ctx::ler(&d).contato("minha namorada"), Some("Eduarda"));
        // resposta do agente que nao e um contato da lista nao vale
        let f = Fake { claude: Some("NAO SEI".into()), ..fake() };
        assert!(rodar(&f, |s| enviar_texto(&d, &c, s, "meu primo", "oi")).contains("Procurei"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn audio_respeita_so_texto_e_mudo_e_arquivo_fica_no_historico() {
        let (d, c, f) = (pasta("audio"), ctx(), fake());
        assert!(rodar(&f, |s| enviar_audio(&d, &c, 100.0, s, "Eduarda", "oi")).starts_with("✅ Áudio"));
        contexto::definir_preferencia(&d, "Eduarda", "texto").unwrap();
        let r = rodar(&f, |s| enviar_audio(&d, &Ctx::ler(&d), 100.0, s, "Eduarda", "oi de novo"));
        assert!(r.starts_with("(Eduarda só recebe texto") && f.enviados.borrow()[1].0 == "texto");
        // o dono: pedido explicito de audio sai em audio; so o MUDO (com o interruptor do WhatsApp) manda por escrito
        assert!(rodar(&f, |s| enviar_audio(&d, &c, 100.0, s, "Davi", "resumo")).starts_with("✅ Áudio"));
        mudo::mutar(&d, None, 100.0);
        mudo::definir_whatsapp_dono(&d, true, 100.0);
        assert!(rodar(&f, |s| enviar_audio(&d, &c, 101.0, s, "Davi", "resumo")).starts_with("(MUDO ligado"));
        assert!(rodar(&f, |s| enviar_audio(&d, &c, 101.0, s, "mainha", "o cpf é 123.456.789-01")).starts_with("Não mandei"));
        // arquivo
        let r = rodar(&f, |s| enviar_arquivo(&d, &c, 102.0, s, "Deusa Bia", "video.mp4", "segue"));
        assert_eq!(r, "✅ ok");
        assert_eq!(f.enviados.borrow().last().unwrap().1, "Beatriz Lima 🤍"); // nome exato: quem envia confere o titulo inteiro
        assert!(memoria::historico(&d, "Beatriz Lima 🤍", 3).iter().any(|(_, t)| t == "[arquivo video.mp4] segue"));
        assert!(rodar(&f, |s| enviar_arquivo(&d, &c, 103.0, s, "mainha", "C:/x/.env", "")).starts_with("Não mandei: Esse arquivo guarda segredos"));
        assert!(rodar(&f, |s| enviar_arquivo(&d, &c, 103.0, s, "mainha", "a.pdf", "senha: abc123")).starts_with("Não mandei: a legenda"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
