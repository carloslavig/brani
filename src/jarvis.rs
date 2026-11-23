//! O cerebro do Jarvis (porte de braniac/jarvis.py): a ORDEM de `processar` (atalhos sem IA antes da IA), as pendencias
//! (duvida que a IA sinalizou, confirmacao; a pendencia 'duvida_contato' do Python nunca e criada la e ficou de fora), a escolha das ferramentas por assunto e o fecho do agente.
//! As travas estao em `jarvis_travas`, os comandos diretos em `jarvis_comandos`, a lista de ferramentas em `jarvis_ferramentas`.
//! Tudo que sai para fora (WhatsApp, IA, agente de programacao, navegador, Spotify, e-mail, agenda, conselho...) entra por
//! closure em `Pecas`; aqui ficam so a ordem, as condicoes e as travas de origem.
//! Diferencas do Python: origem local vale "web" ou "pc"; "/mudo" usa so `mudo.rs` (sem a escuta por voz); as frases de musica
//! e do legado sao reconhecidas por closure; o modo profundo barrado devolve o aviso da trava (no Python voltava o texto para a IA);
//! o aprendizado de habilidade depois de 3 ferramentas nao esta aqui. O executor das ferramentas e de quem integra
//! (`jarvis_travas::trava_ferramenta` diz o que barrar).
#![allow(dead_code)]
use crate::contexto::{self, norm, Ctx};
use crate::jarvis_comandos::{self as cmd, Direto, Email, Saidas};
use crate::jarvis_ferramentas as jf;
use crate::jarvis_travas::{self as tr, gate, gate_envio, DONO_CONV};
use crate::{confirmacao, dono, memoria, mudo, personalidade, power};
use regex::Regex;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;

/// PC, WhatsApp e Telegram sao UM Jarvis so; conversas de projeto ("b2:N") tem historico proprio.
pub const CONV_UNICA: &str = "jarvis";
const PANICO: [&str; 6] = ["/panico", "/pânico", "panico", "pânico", "botao de panico", "botão de pânico"];
const OPINIAO: &str = "\n\n[PERGUNTA DE OPINIAO: nao e fato. Opine em 1a pessoa, de forma pessoal e curta, com base nos fatos do relatorio. Texto corrido, SEM rotulos ('Fato:', 'Minha impressao:') e SEM dois-pontos de pausa. Introduza o que ha registrado com 'o que eu tenho registrado aqui e' ou 'o que sei sobre ela e que'; e a opiniao com 'tenho a impressao de que' ou 'a minha impressao a respeito dela e que'. Havendo fatos, NAO diga 'nao tenho registrado'.]";

// ---------- ferramentas ----------

/// O que o Python monta na hora e cola na descricao das ferramentas.
#[derive(Default)]
pub struct Catalogos {
    pub artcraft: String,
    pub artcraft_acoes: Vec<String>,
    pub skills: String,
    pub juridico: String,
    pub juridico_acoes: Vec<String>,
}

/// As 59 ferramentas do Jarvis (nome, descricao, parametros), com os catalogos no lugar.
pub fn ferramentas(c: &Catalogos) -> Vec<Value> {
    let skills = if c.skills.is_empty() { "nenhuma" } else { &c.skills };
    jf::crua().into_iter().map(|mut f| {
        let d = f["function"]["description"].as_str().unwrap_or("").replace("{{ARTCRAFT_CATALOGO}}", &c.artcraft).replace("{{SKILLS_CATALOGO}}", skills).replace("{{JURIDICO_CATALOGO}}", &c.juridico);
        f["function"]["description"] = json!(d);
        match f["function"]["name"].as_str() {
            Some("artcraft") => f["function"]["parameters"]["properties"]["acao"]["enum"] = json!(c.artcraft_acoes),
            Some("juridico") => f["function"]["parameters"]["properties"]["acao"]["enum"] = json!(c.juridico_acoes),
            _ => {}
        }
        f
    }).collect()
}

fn nome_de(f: &Value) -> &str {
    f["function"]["name"].as_str().unwrap_or("")
}

fn gatilhos() -> &'static Vec<Regex> {
    static R: std::sync::OnceLock<Vec<Regex>> = std::sync::OnceLock::new();
    R.get_or_init(|| jf::GRUPOS.iter().map(|(p, _)| Regex::new(&format!("(?i){p}")).expect("regex fixa")).collect())
}

/// Subconjunto das ferramentas para ESTA mensagem: o nucleo sempre + os grupos cujo assunto aparece na mensagem ou nas 2 ultimas
/// falas ("sim, pode marcar" continua o assunto anterior). O contexto anexado nao escolhe ferramenta. `extras` = gatilhos da
/// configuracao (nomes de pessoas e de clientes nao ficam no codigo).
pub fn ferramentas_para(todas: &[Value], texto: &str, falas: &[String], extras: &[(Regex, Vec<String>)]) -> Vec<Value> {
    let texto = texto.split(cmd::CONTEXTO_ANEXADO).next().unwrap_or("");
    let mut alvo = texto.to_string();
    for f in &falas[falas.len().saturating_sub(2)..] {
        alvo.push(' ');
        alvo.push_str(f);
    }
    let mut nomes: BTreeSet<&str> = jf::NUCLEO.iter().copied().collect();
    for (r, (_, grupo)) in gatilhos().iter().zip(jf::GRUPOS.iter()) {
        if r.is_match(&alvo) {
            nomes.extend(grupo.iter().copied());
        }
    }
    for (r, grupo) in extras {
        if r.is_match(&alvo) {
            nomes.extend(grupo.iter().map(String::as_str));
        }
    }
    todas.iter().filter(|f| nomes.contains(nome_de(f))).cloned().collect()
}

/// Ferramentas que o Jarvis em Python tem e o `ferramentas.rs` ainda nao.
pub fn faltam_no_rust() -> Vec<String> {
    let tem = crate::ferramentas::lista();
    let tem: BTreeSet<&str> = tem.as_array().into_iter().flatten().map(nome_de).collect();
    jf::crua().iter().map(nome_de).filter(|n| !tem.contains(n)).map(str::to_string).collect()
}

#[derive(Debug, Clone, PartialEq)]
pub enum Falha {
    /// o Braniac esta desligado
    Desligado(String),
    /// nenhum provedor respondeu
    TodosFalharam(String),
}

/// O que o dono ve depois que o agente rodou. O resultado REAL das ferramentas (`saida_direta`) ganha da fala da IA, que nao viu
/// o conteudo e poderia contradizer; se a IA caiu depois de agir, mostra o que foi feito em vez de dizer "nao fiz nada".
pub fn fecho_do_agente(resultado: Result<String, Falha>, saida_direta: &[String], feitos: &[(String, String)], chamadas: &[String]) -> Result<(String, bool), Falha> {
    if !saida_direta.is_empty() && !matches!(resultado, Err(Falha::Desligado(_))) {
        return Ok((saida_direta.join("\n\n"), false));
    }
    match resultado {
        Err(Falha::TodosFalharam(_)) if !feitos.is_empty() => {
            let l: Vec<&str> = feitos.iter().map(|(_, r)| r.as_str()).filter(|r| !r.starts_with('(')).collect();
            Ok((if l.is_empty() { "Feito.".into() } else { l.join("\n") }, false))
        }
        Err(e) => Err(e),
        Ok(fim) if !chamadas.iter().any(|c| c == "whatsapp_enviar") && cmd::envio_inventado(&fim) =>
            Ok(("Não consegui preparar esse envio direito. Diga assim: \"manda pra <nome> dizendo <texto>\" que eu mando na hora.".into(), false)),
        Ok(fim) => Ok((if fim.is_empty() { "Feito.".into() } else { fim }, true)),
    }
}

// ---------- pendencias ----------

#[derive(Debug, Clone, PartialEq)]
pub enum Tipo {
    DuvidaFato { pergunta: String, original: String },
    Whatsapp { nome: String, texto: String },
    WhatsappAudio { nome: String, texto: String },
    Claude { tarefa: String },
}

/// Algo aguardando a resposta do dono; vence em `ate` (segundos desde 1970).
#[derive(Debug, Clone, PartialEq)]
pub struct Pendente {
    pub ate: f64,
    pub tipo: Tipo,
}

/// Escada de duvidas: o agente de programacao antes do dono.
#[derive(Debug, Clone, PartialEq)]
pub enum Consulta {
    Resp(String),
    /// o proprio agente julgou que so o dono decide: a pergunta que ele redigiu
    Perguntar(String),
    Nada,
}

/// O que os agentes de assunto devolveram.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Delegado {
    pub texto: String,
    /// dado pessoal/financeiro: a resposta sai montada aqui, sem IA online
    pub local_only: bool,
    /// ninguem tinha nada sobre o assunto
    pub vazio: bool,
    /// um agente so, dos que respondem direto (sai tambem em audio)
    pub direto: bool,
}

type F0<'a, R> = Box<dyn Fn() -> R + 'a>;
type F1<'a, R> = Box<dyn Fn(&str) -> R + 'a>;
type F2<'a, R> = Box<dyn Fn(&str, &str) -> R + 'a>;
type F3<'a, R> = Box<dyn Fn(&str, &str, &str) -> R + 'a>;

/// As pecas que `processar` usa. `Pecas::neutras` devolve tudo "nao e comigo"; quem integra troca cada closure pela de verdade.
pub struct Pecas<'a> {
    pub dados: &'a Path,
    pub ctx: Ctx,
    /// apelidos que recebem recado sem autonomia armada (`jarvis_travas::familia_livre`)
    pub familia: Vec<String>,
    pub ts: f64,
    pub teto_claude: i64,
    /// ha um canal para avisar depois (tarefas em segundo plano)
    pub pode_notificar: bool,
    /// ha provedor com ferramentas (o agente)
    pub tem_agente: bool,
    /// ficha fixa "quem sou eu", anexada quando perguntam do proprio Jarvis
    pub ficha: String,
    pub saidas: Saidas<'a>,
    /// (texto, origem, conversa): tudo que o dono diz vai para a Alma
    pub observar: F3<'a, ()>,
    pub tarefa_local: F1<'a, String>,
    pub panico: F1<'a, String>,
    pub assinatura: F1<'a, Option<String>>,
    pub historico: Box<dyn Fn(&str, usize) -> Vec<Value> + 'a>,
    pub tipos_de_erro: F2<'a, Vec<String>>,
    /// (texto, origem)
    pub legado: F2<'a, Option<String>>,
    pub status: F0<'a, String>,
    pub ao_desligar: F0<'a, ()>,
    pub voz: Box<dyn Fn(Option<&str>) -> String + 'a>,
    /// "ligar" | "desligar" | "estado"
    pub escuta: F1<'a, String>,
    pub auto: F1<'a, String>,
    pub nao_lidas: F0<'a, String>,
    pub ler_conversa: F1<'a, String>,
    pub eh_musica: F1<'a, bool>,
    pub musica: F1<'a, String>,
    pub email: Box<dyn Fn(&Email) -> String + 'a>,
    pub resumo_wpp: Box<dyn Fn(f64) -> String + 'a>,
    pub ultimas_do_contato: F1<'a, Vec<String>>,
    /// (texto, trava para criar arquivo): Some = era pedido juridico claro
    pub juridico: Box<dyn Fn(&str, Option<&str>) -> Option<String> + 'a>,
    pub financeiro: F1<'a, Option<String>>,
    pub conselho_pediu: F1<'a, bool>,
    pub conselho: F1<'a, String>,
    pub sensivel: F1<'a, bool>,
    /// ha como perguntar a uma IA parceira pelo navegador; sem isso, "pergunta pro Grok" e "o que voce sabe de mim" seguem para a IA daqui
    pub tem_ia_web: bool,
    /// (ia, pergunta) ja conferida
    pub ia_web: F2<'a, String>,
    pub uso_claude: F0<'a, i64>,
    pub add_uso_claude: F0<'a, ()>,
    pub claude_segundo_plano: F1<'a, ()>,
    pub consultar_claude: F1<'a, Consulta>,
    /// (chave, resposta, fonte)
    pub registrar_fato: F3<'a, ()>,
    pub atender: F0<'a, String>,
    pub contato_conhecido: F1<'a, Option<String>>,
    pub ver_imagem: F1<'a, Result<String, String>>,
    pub agenda_listar: Box<dyn Fn(&str, i64) -> String + 'a>,
    pub airbnb_reconhecer: F1<'a, Option<String>>,
    pub quem_e: F1<'a, Option<String>>,
    pub projeto_consulta: F1<'a, Option<String>>,
    pub sem_registro: F1<'a, Option<String>>,
    pub delegar: F1<'a, Option<Delegado>>,
    pub eh_opiniao: F1<'a, bool>,
    pub levar_ao_conselho: F1<'a, String>,
    /// (texto de agora, texto com o contexto, conversa)
    pub nota_de_contexto: F3<'a, String>,
    pub profundo: F1<'a, String>,
    /// (texto com o contexto, conversa, extra)
    pub agente: F3<'a, Result<(String, bool), Falha>>,
    /// (conversa, texto + extra)
    pub responder: F2<'a, Result<String, Falha>>,
}

impl<'a> Pecas<'a> {
    pub fn neutras(dados: &'a Path, ts: f64) -> Pecas<'a> {
        Pecas {
            dados, ctx: Ctx::ler(dados), familia: tr::familia_livre(dados), ts, teto_claude: 20, pode_notificar: false, tem_agente: false, tem_ia_web: false, ficha: String::new(),
            saidas: Saidas { listar_nomes: &|| Ok(vec![]), buscar_contatos: &|_| vec![], perguntar_claude: &|_| None, enviar_texto: &|_, _| "Envio não ligado.".into(),
                enviar_audio: &|_, _| "Envio não ligado.".into(), resolver_arquivo: &|_| Err("envio de arquivo não ligado".into()), enviar_arquivo: &|_, _, _| "Envio não ligado.".into() },
            observar: Box::new(|_, _, _| ()), tarefa_local: Box::new(|_| String::new()), panico: Box::new(|_| String::new()), assinatura: Box::new(|_| None),
            historico: Box::new(|_, _| vec![]), tipos_de_erro: Box::new(|_, _| vec![]), legado: Box::new(|_, _| None), status: Box::new(String::new),
            ao_desligar: Box::new(|| ()), voz: Box::new(|_| String::new()), escuta: Box::new(|_| String::new()), auto: Box::new(|_| String::new()),
            nao_lidas: Box::new(String::new), ler_conversa: Box::new(|_| String::new()), eh_musica: Box::new(|_| false), musica: Box::new(|_| String::new()),
            email: Box::new(|_| String::new()), resumo_wpp: Box::new(|_| String::new()), ultimas_do_contato: Box::new(|_| vec![]), juridico: Box::new(|_, _| None),
            financeiro: Box::new(|_| None), conselho_pediu: Box::new(|_| false), conselho: Box::new(|_| String::new()), sensivel: Box::new(|_| false),
            ia_web: Box::new(|_, _| String::new()), uso_claude: Box::new(|| 0), add_uso_claude: Box::new(|| ()), claude_segundo_plano: Box::new(|_| ()),
            consultar_claude: Box::new(|_| Consulta::Nada), registrar_fato: Box::new(|_, _, _| ()), atender: Box::new(String::new), contato_conhecido: Box::new(|_| None),
            ver_imagem: Box::new(|_| Err("não ligado".into())), agenda_listar: Box::new(|_, _| String::new()), airbnb_reconhecer: Box::new(|_| None), quem_e: Box::new(|_| None),
            projeto_consulta: Box::new(|_| None), sem_registro: Box::new(|_| None), delegar: Box::new(|_| None), eh_opiniao: Box::new(|_| false),
            levar_ao_conselho: Box::new(|_| String::new()), nota_de_contexto: Box::new(|_, _, _| String::new()), profundo: Box::new(|_| String::new()),
            agente: Box::new(|_, _, _| Err(Falha::TodosFalharam("sem agente".into()))), responder: Box::new(|_, _| Err(Falha::TodosFalharam("sem IA".into()))),
        }
    }
}

/// Ficha fixa do Python ("quem e voce / como voce funciona"): fatos fixos para a IA nao inventar. Quem integra pode trocar por outra.
pub fn ficha_jarvis(dono: &str) -> String {
    let d = if dono.is_empty() { "dono" } else { dono };
    format!("\n\n[SOBRE VOCE E O BRANIAC (fatos fixos; use SO isto, sem inventar numeros nem tecnologias): Voce e o Jarvis, assistente de IA pessoal do {d}, que roda no PC dele. \
O Braniac e o sistema que te contem: um servidor local, com janela nativa e telas em HTML/JavaScript. \
Ele escolhe sozinho a IA que responde: primeiro as gratis online, depois o modelo local e, so por ultimo, o agente de programacao. Tem um banco local unificado (memoria e fatos, com validade), agentes por assunto, \
um conselho de IAs, atende WhatsApp e Telegram, fala por voz e muda o proprio codigo com travas e testes. \
Foi feito para ECONOMIZAR TOKENS: o agente pago so entra quando as gratis e o modelo local falham ou quando o pedido e programar/mexer no PC. \
Isto NAO e segredo: explique com naturalidade a quem perguntar. Responda em 4 a 6 frases, em 1a pessoa, sem listar agentes nem relatorio.]")
}

/// Texto fixo para desabafo/crise (o modelo local pequeno errava o nome e o telefone de ajuda).
pub fn acolher(dono: &str) -> String {
    let d = if dono.is_empty() { String::new() } else { format!("{dono}, ") };
    format!("{d}eu li tudo e tô aqui com você. Respira fundo comigo: inspira contando até 4, segura 4 e solta devagar até 6. Faz isso umas cinco vezes. \
Você não tá sozinho nisso. Se o aperto for muito forte ou vier pensamento de se machucar, liga pro CVV no 188, é de graça e 24 horas. \
Sobre o cartão: abre o app do banco e bloqueia ele por lá agora, assim você fica tranquilo até achar. Quer que eu fique aqui te ouvindo?")
}

fn local(origem: &str) -> bool {
    matches!(origem, "web" | "pc")
}

fn corta(t: &str, n: usize) -> String {
    t.chars().take(n).collect()
}

fn pode_claude(p: &Pecas) -> bool {
    (p.uso_claude)() < p.teto_claude
}

/// Pergunta ao agente contando no teto diario; sem teto, ninguem e perguntado.
fn perguntar_claude(p: &Pecas, pergunta: &str) -> Consulta {
    if !pode_claude(p) {
        return Consulta::Nada;
    }
    (p.add_uso_claude)();
    (p.consultar_claude)(pergunta)
}

fn acionar_claude(p: &Pecas, tarefa: &str) -> String {
    if !pode_claude(p) {
        return format!("O teto diário de {} acionamentos do Claude já foi atingido. Tente amanhã ou ajuste claude_max_por_dia.", p.teto_claude);
    }
    (p.add_uso_claude)();
    (p.claude_segundo_plano)(tarefa);
    "Acionei o Claude. Ele pode levar alguns minutos; te aviso aqui quando terminar. 🤖".into()
}

fn consultar_ia(p: &Pecas, ia: &str, pergunta: &str) -> String {
    tr::ia_web_barrada(ia, pergunta, (p.sensivel)(pergunta), false).unwrap_or_else(|| corta(&(p.ia_web)(ia, pergunta.trim()), 4000))
}

fn reprocessar(p: &Pecas, conv: &str, original: &str, pergunta: &str, resposta: &str) -> (String, bool) {
    match (p.responder)(conv, &format!("{original}\n\n[Contexto confirmado: {pergunta} -> {resposta}]")) {
        Err(Falha::Desligado(e)) | Err(Falha::TodosFalharam(e)) => (e, false),
        Ok(r) => match cmd::duvida(&r) {
            Some(q) => (q, false),
            None => (r, true),
        },
    }
}

/// A IA sinalizou [DUVIDA]: o agente primeiro; se ele nao souber, pergunta ao dono (e fica pendente por 30 min).
fn tratar_duvida_fato(p: &Pecas, pend: &mut Option<Pendente>, pergunta: &str, original: &str, conv: &str) -> (String, bool) {
    let texto = match perguntar_claude(p, pergunta) {
        Consulta::Resp(r) => {
            (p.registrar_fato)(pergunta, &r, "claude");
            return reprocessar(p, conv, original, pergunta, &r);
        }
        Consulta::Perguntar(q) => q,
        Consulta::Nada => pergunta.to_string(),
    };
    *pend = Some(Pendente { ate: p.ts + 1800.0, tipo: Tipo::DuvidaFato { pergunta: pergunta.into(), original: original.into() } });
    (format!("Preciso de uma ajuda sua: {texto}"), false)
}

fn ler_conversa(p: &Pecas, s: &Saidas, nome: &str) -> (String, bool) {
    match cmd::resolver_contato(p.dados, &p.ctx, s, nome) {
        Ok(exato) => ((p.ler_conversa)(&exato), false),
        Err(m) => (m, false),
    }
}

/// Devolve (resposta, e_da_ia). e_da_ia = true so para conversa (sai tambem em audio).
/// `origem`: "telegram"/"whatsapp" (de fora, restrito) ou "web"/"pc" (local). `pend` = o que esta aguardando o dono.
pub fn processar(p: &Pecas, pend: &mut Option<Pendente>, texto: &str, conv: &str, origem: &str, extra: &str) -> (String, bool) {
    let t_llm = texto.trim();
    // os atalhos por palavra-chave olham so o que o dono DISSE agora; o contexto anexado serve so para a IA
    let t = cmd::sem_contexto(t_llm);
    let barra = t.starts_with('/');
    let minusc = t.to_lowercase();
    let baixo = minusc.trim_matches([' ', '.', '!']);
    let armada = power::armada(p.dados, p.ts);
    let g = |o_que: &str| gate(origem, o_que, armada);
    let b2 = conv.starts_with("b2:");
    let mut extra = extra.to_string();
    let perguntar = |q: &str| match perguntar_claude(p, q) {
        Consulta::Resp(r) => Some(r),
        _ => None,
    };
    let s = Saidas { perguntar_claude: &perguntar, ..p.saidas };

    // so conversa de GENTE vai a Alma (pedido de outro agente pela API nao e fala do dono)
    if matches!(conv, "web" | "voz" | "wpp-dono" | "telegram") || b2 {
        (p.observar)(t, origem, conv);
    }
    // pedido de construcao dito no painel ou por voz e construido de verdade
    if origem != "telegram" && !barra && (matches!(conv, "web" | "voz") || b2) && dono::eh_tarefa(t) {
        return ((p.tarefa_local)(t), false);
    }
    if !barra {
        if g("mudar minha personalidade").is_none() {
            if let Some(r) = personalidade::comando(p.dados, t) {
                return (r, false);
            }
        }
        extra += &personalidade::lembrete(&personalidade::niveis(p.dados), &p.ctx.dono);
    }
    let conv = if b2 { conv } else { CONV_UNICA };
    if PANICO.contains(&baixo) {
        return ((p.panico)(origem), false); // so DESARMA/DESLIGA: vale de qualquer canal
    }
    if !barra {
        if let Some(r) = (p.assinatura)(t) {
            return (r, true);
        }
    }
    if origem != "telegram" && !barra && cmd::mal(t) {
        return (acolher(&p.ctx.dono), true);
    }
    if cmd::numero_solto(t) {
        return (format!("Anotei o número {}. Quer que eu faça algo com ele (salvar como contato, mandar mensagem, ligar, pesquisar)? Se for só pra guardar, me diz o nome.", t.trim()), false);
    }
    if cmd::eh_erro(t) {
        return (cmd::registrar_erro(p.dados, &(p.historico)(conv, 8), t, p.ts, &*p.tipos_de_erro), false);
    }

    if pend.as_ref().is_some_and(|x| p.ts > x.ate) {
        *pend = None;
    }
    if let Some(pe) = pend.clone() {
        if confirmacao::classificar(baixo) == Some(false) {
            *pend = None;
            return ("Cancelado, nada foi feito.".into(), false);
        }
        match pe.tipo {
            Tipo::DuvidaFato { pergunta, original } if !barra => {
                *pend = None;
                (p.registrar_fato)(&pergunta, t.trim(), "dono");
                return reprocessar(p, conv, &original, &pergunta, t.trim());
            }
            Tipo::Whatsapp { .. } | Tipo::WhatsappAudio { .. } | Tipo::Claude { .. } if confirmacao::classificar(baixo) == Some(true) => {
                *pend = None;
                if let Some(x) = g("enviar mensagens e acionar o Claude") {
                    return (x, false);
                }
                return (match pe.tipo {
                    Tipo::WhatsappAudio { nome, texto } => (s.enviar_audio)(&nome, &texto),
                    Tipo::Whatsapp { nome, texto } => (s.enviar_texto)(&nome, &texto),
                    Tipo::Claude { tarefa } => {
                        (p.add_uso_claude)();
                        (p.claude_segundo_plano)(&tarefa);
                        "Acionei o Claude. Ele pode levar alguns minutos; te aviso aqui quando terminar. 🤖".into()
                    }
                    _ => String::new(),
                }, false);
            }
            _ => {}
        }
    }

    if let Some(r) = (p.legado)(t, origem) {
        return (r, false);
    }
    match t {
        "/status" => return ((p.status)(), false),
        "/ligar" => {
            power::ligar(p.dados);
            return ("Braniac ligado.".into(), false);
        }
        "/desligar" => {
            power::desligar(p.dados, "manual", "");
            (p.ao_desligar)();
            return ("Braniac desligado.".into(), false);
        }
        _ => {}
    }
    if let Some(h) = cmd::humor_cmd(t) {
        return (match h {
            None => format!("Meu humor está em {}%. Para mudar: /humor 50", p.ctx.humor()),
            Some(n) => match contexto::definir_humor(p.dados, n.min(100)) {
                Ok(novo) => format!("Humor ajustado para {novo}%. {}", if novo <= 10 { "Agora é só trabalho, sem piadas." } else { "Já aparece no jeito de falar e no tom da voz." }),
                Err(e) => format!("Não consegui guardar o humor ({e})."),
            },
        }, false);
    }
    if let Some(c) = re_esquecer().captures(t) {
        let alvo = c["t"].trim();
        let n = memoria::esquecer(p.dados, alvo);
        return (if n > 0 { format!("Apaguei {n} item(ns) da memória sobre '{alvo}'. Da próxima vez eu pergunto de novo.") } else { format!("Não achei nada na memória sobre '{alvo}'.") }, false);
    }
    if let Some(c) = re_corrigir().captures(t) {
        memoria::corrigir(p.dados, &c["e"], &c["c"], p.ts);
        return ("Anotado. Quando esse assunto voltar, eu uso a sua correção.".into(), false);
    }
    if minusc.trim() == "/memoria" {
        let (m, c, d, f) = memoria::resumo(p.dados);
        return (format!("Memória local: {m} mensagens guardadas, {c} respostas aprendidas, {d} decisões e {f} fatos sobre pessoas. Para apagar algo errado: /esquecer <assunto>"), false);
    }
    let m_mudo = re_mudo().captures(t);
    let arg_mudo = m_mudo.as_ref().and_then(|c| c.name("a")).map(|a| a.as_str().to_lowercase());
    let cmd_mudo = match (&m_mudo, arg_mudo.as_deref()) {
        (Some(_), Some("off" | "sair" | "desligar")) => Some("desmutar"),
        (Some(_), _) => Some("mutar"),
        (None, _) => mudo::comando(t, mudo::ativo(p.dados, p.ts)),
    };
    if let Some(c) = cmd_mudo {
        let minutos = arg_mudo.as_deref().filter(|a| a.chars().all(|x| x.is_ascii_digit())).and_then(|a| a.parse::<f64>().ok());
        if c == "desmutar" {
            mudo::desmutar(p.dados, p.ts);
            return ("🔊 Som de volta: voltei a falar em voz alta.".into(), false);
        }
        mudo::mutar(p.dados, minutos, p.ts);
        let quanto = minutos.filter(|m| *m != 0.0).map_or("até você desmutar".to_string(), |m| format!("por {m} min"));
        return (format!("🔇 Mudo {quanto}: não falo em voz alta (o texto continua). Para voltar: /mudo off ou «pode falar»."), false);
    }
    if let Some(c) = re_voz().captures(t) {
        return ((p.voz)(c.name("n").map(|n| n.as_str())), false);
    }
    // microfone e atendimento automatico: DESLIGAR de qualquer canal; LIGAR so no PC
    if let Some(arg) = minusc.strip_prefix("/escuta").map(str::trim) {
        return (match arg {
            "off" | "desligar" | "parar" => {
                (p.escuta)("desligar");
                "Escuta por voz DESLIGADA: o microfone do notebook não está mais sendo usado.".into()
            }
            "on" | "ligar" if !local(origem) => "🔒 A escuta por voz só liga pelo PC. Desligar eu consigo por aqui: /escuta off".into(),
            "on" | "ligar" => {
                (p.escuta)("ligar");
                "Escuta por voz LIGADA. Fale: «Jarvis, tá acordado?»".into()
            }
            _ => (p.escuta)("estado"),
        }, false);
    }
    if let Some(arg) = minusc.strip_prefix("/auto").map(str::trim) {
        return (match arg {
            "off" | "desligar" | "parar" => {
                (p.auto)("desligar");
                "Atendimento automático DESLIGADO. Eu só respondo pessoas quando você mandar.".into()
            }
            "on" | "ligar" if !local(origem) => "🔒 O atendimento automático só liga pelo botão da interface do PC. Desligar eu consigo por aqui: /auto off".into(),
            "on" | "ligar" => {
                (p.auto)("ligar");
                "Atendimento automático LIGADO. A partir de agora respondo sozinho só as mensagens novas.".into()
            }
            _ => (p.auto)("estado"),
        }, false);
    }
    if power::pausado(p.dados) {
        // desligado: so os comandos acima funcionam
        let e = power::estado(p.dados);
        let jogo = if e["motivo"] == "jogo" { format!(" (jogo: {})", e["jogo"].as_str().unwrap_or("")) } else { String::new() };
        return (format!("Braniac desligado{jogo}. Mande /ligar para religar."), false);
    }
    if t == "/whatsapp" {
        return ((p.nao_lidas)(), false);
    }
    if let Some(nome) = t.strip_prefix("/wpp ") {
        return ler_conversa(p, &s, nome.trim());
    }
    if (p.eh_musica)(t) {
        return (g("controlar o Spotify do PC").unwrap_or_else(|| (p.musica)(t)), false);
    }
    let envio = cmd::eh_envio(t);
    if !barra && !envio {
        match cmd::email_pedido(t) {
            Some(Email::Mudar) => return (cmd::EMAIL_SO_LEITURA.into(), false),
            Some(e) => return (g("ler o seu e-mail").unwrap_or_else(|| (p.email)(&e)), false),
            None => {}
        }
        // resumo e "o que fulano disse" saem do banco LOCAL (o que aconteceu de verdade), nunca de uma IA
        if cmd::pede_resumo_wpp(t) {
            return ((p.resumo_wpp)(cmd::horas_do_pedido(t)), false);
        }
        if let Some(nome) = cmd::sobre_contato(t) {
            let nome = p.ctx.contato(&nome).map(str::to_string).unwrap_or(nome);
            let n = norm(&nome);
            if !["voce", "vc", "eu", "ele", "ela", "isso", "jarvis"].contains(&n.as_str()) && (p.ctx.dono.is_empty() || n != norm(&p.ctx.dono)) {
                let msgs = (p.ultimas_do_contato)(&nome);
                if !msgs.is_empty() {
                    return (format!("Últimas mensagens de {nome} (do banco do Jarvis):\n{}", msgs.join("\n")), false);
                }
            }
        }
    }
    // pedido juridico claro vai a fonte oficial; o arquivo da carta passa pela MESMA trava de mexer em arquivos
    if let Some(r) = (p.juridico)(t, g("mexer em arquivos do PC com o ArtCraft").as_deref()) {
        return (r, false);
    }
    if !barra && t.chars().count() < 300 {
        if let Some(r) = (p.financeiro)(t) {
            return (r, false);
        }
    }
    if !barra && (p.conselho_pediu)(t) {
        return (g("convocar o conselho de IAs").unwrap_or_else(|| (p.conselho)(t)), false);
    }
    if let Some((ia, q)) = cmd::ia_pedido(t).filter(|_| p.tem_ia_web) {
        if let Some(x) = g("consultar outras IAs") {
            return (x, false);
        }
        if q.chars().count() < 12 {
            return (format!("O que você quer que eu pergunte ao {ia}? Me manda a pergunta completa."), false);
        }
        return (format!("🤖 {ia} respondeu:\n{}", consultar_ia(p, &ia, &q)), false);
    }
    if p.tem_ia_web && cmd::pergunta_pessoal(t) {
        if let Some(x) = g("consultar o ChatGPT") {
            return (x, false);
        }
        let resp = consultar_ia(p, "chatgpt", &format!("Responda com base no que você sabe sobre mim: {t}"));
        return (if resp.starts_with("nao consegui falar") || resp.starts_with("pergunta barrada") { resp } else { format!("🤖 Perguntei ao ChatGPT (ele te conhece):\n{resp}") }, false);
    }
    if let Some(quem) = cmd::sem_audio(t) {
        let _ = contexto::definir_preferencia(p.dados, p.ctx.contato(&quem).unwrap_or(&quem), "texto");
        return (format!("Combinado: para {quem} eu só mando texto, nunca áudio. Isso fica salvo."), false);
    }
    let ge = |o_que: &str, nome: &str| gate_envio(origem, o_que, nome, armada, &p.ctx, &p.familia);
    match cmd::direto(t) {
        Some(Direto::Arquivo { nome, caminho, legenda }) =>
            return (ge("enviar arquivos", &nome).unwrap_or_else(|| cmd::enviar_arquivo(p.dados, &p.ctx, p.ts, &s, &nome, &caminho, &legenda)), false),
        Some(Direto::Audio { nome, texto }) => return (ge("enviar áudios", &nome).unwrap_or_else(|| cmd::enviar_audio(p.dados, &p.ctx, p.ts, &s, &nome, &texto)), false),
        Some(Direto::Texto { nome, texto }) => return (ge("enviar mensagens", &nome).unwrap_or_else(|| cmd::enviar_texto(p.dados, &p.ctx, &s, &nome, &texto)), false),
        None => {}
    }
    if let Some(tarefa) = cmd::claude_tarefa(t) {
        // igual ao Python: aqui `conv` ja virou a conversa unica, entao o chat do dono NAO fica livre da trava neste atalho
        return ((if conv == DONO_CONV { None } else { g("acionar o Claude") }).unwrap_or_else(|| acionar_claude(p, &tarefa)), false);
    }
    if cmd::quer_atender(t) {
        return (g("atender o WhatsApp").unwrap_or_else(|| (p.atender)()), false);
    }
    if let Some(quem) = cmd::imagem_de(t).and_then(|n| p.ctx.contato(&n).map(str::to_string).or_else(|| (p.contato_conhecido)(&n))) {
        if let Some(x) = g("ver imagens do WhatsApp") {
            return (x, false);
        }
        return (match (p.ver_imagem)(&quem) {
            Ok(desc) => format!("🖼 {quem}: {desc}"),
            Err(e) => format!("Não consegui ver a imagem de {quem}: {e}"),
        }, false);
    }
    if let Some((dia, dias)) = cmd::agenda_ver(t) {
        return ((p.agenda_listar)(&dia, dias), false);
    }
    if cmd::ja_respondi(t) {
        if let Some(r) = (p.airbnb_reconhecer)(t) {
            return (r, false);
        }
    }
    let si = cmd::sobre_si(t);
    if !si {
        // 'quem e X?': so o que o banco registra
        if let Some(r) = (p.quem_e)(t) {
            return (r, false);
        }
    }
    let (mut desconhecido, mut consultado) = (false, false);
    if p.pode_notificar && !barra && g("consultar outras IAs").is_none() {
        if let Some(x) = (p.projeto_consulta)(t) {
            desconhecido = x.contains("NAO ha nada");
            consultado = true;
            extra = format!("{extra}\n\n{x}");
        }
    }
    if !consultado && !si {
        // sem nada no banco e sem consulta feita: diz que nao tem, nao chuta
        if let Some(r) = (p.sem_registro)(t) {
            return (r, false);
        }
    }
    if si {
        extra += &p.ficha;
    }
    // so o Jarvis fala com o dono: ele vai ao agente do assunto, pergunta e repassa
    let del = if desconhecido || si { None } else { (p.delegar)(t) };
    if del.as_ref().is_some_and(|d| d.vazio) && p.pode_notificar && g("convocar o conselho de IAs").is_none() {
        return ((p.levar_ao_conselho)(t), false);
    }
    if let Some(d) = del {
        if (p.eh_opiniao)(t) {
            extra += OPINIAO;
        }
        if d.local_only || !p.tem_agente {
            return (d.texto, d.direto);
        }
        extra = format!("{extra}\n\n[RELATORIO DOS AGENTES - responda ao dono com estes dados, sem inventar nada alem deles]\n{}", d.texto);
    }
    if !extra.contains("ESTÁ CONFIRMANDO SUA ÚLTIMA MENSAGEM") {
        extra += &(p.nota_de_contexto)(t, t_llm, conv);
    }
    if p.tem_agente {
        if tr::pede_profundo(t) {
            return (g("trabalhar a fundo").unwrap_or_else(|| (p.profundo)(t)), false);
        }
        match (p.agente)(t_llm, conv, &extra) {
            Ok(r) => return r,
            Err(Falha::Desligado(e)) => return (format!("{e}. Mande /ligar para religar."), false),
            Err(Falha::TodosFalharam(_)) => {} // nenhum provedor com ferramentas agora: reserva por palavras-chave abaixo
        }
    }
    if let Some(conversa) = cmd::zap_ver(t) {
        return match conversa {
            Some(nome) => ler_conversa(p, &s, &nome),
            None => ((p.nao_lidas)(), false),
        };
    }
    match (p.responder)(conv, &format!("{t_llm}{extra}")) {
        Err(Falha::Desligado(e)) => (format!("{e}. Mande /ligar para religar."), false),
        Err(Falha::TodosFalharam(e)) => (format!("Nenhuma IA respondeu agora: {e}"), false),
        Ok(resp) => match cmd::duvida(&resp) {
            Some(q) => tratar_duvida_fato(p, pend, &q, t, conv),
            None => (resp, true),
        },
    }
}

contexto::re!(re_esquecer, r"(?i)^/esquecer\s+(?P<t>.+)$");
contexto::re!(re_corrigir, r"(?is)^/corrigir\s+(?P<e>.+?)\s*(?:=>|->)\s*(?P<c>.+)$");
contexto::re!(re_mudo, r"(?i)^/mudo(?:\s+(?P<a>\w+))?\s*$");
contexto::re!(re_voz, r"(?i)^/voz(?:\s+(?P<n>\w+))?\s*$");

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::path::PathBuf;

    #[derive(Default)]
    struct Visto {
        ia: RefCell<Vec<String>>,
        respostas: RefCell<Vec<String>>,
        enviados: RefCell<Vec<(String, String, String)>>,
        fatos: RefCell<Vec<String>>,
        claude: RefCell<Vec<String>>,
        outros: RefCell<Vec<String>>,
        uso: Cell<i64>,
    }

    fn pasta(n: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-jarvis-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("pessoal.json"), r#"{"nome": "Davi", "contatos": {"deusa bia": "Beatriz Lima 🤍", "bia": "Beatriz Lima 🤍", "davi": "Eu(SP)"}, "familia_livre": ["bia", "deusa bia"]}"#).unwrap();
        d
    }

    fn fixo<'a, T: ?Sized>(b: Box<T>) -> &'a T {
        Box::leak(b)
    }

    /// Pecas de teste: a IA responde o que estiver programado (ou "ia"), os envios so anotam.
    fn pecas<'a>(d: &'a Path, v: &'a Visto) -> Pecas<'a> {
        let mut p = Pecas::neutras(d, 1000.0);
        p.tem_ia_web = true;
        p.saidas = Saidas {
            listar_nomes: &|| Ok(["Beatriz Lima 🤍", "mainha", "Rita Vendas", "Eu(SP)"].iter().map(|s| s.to_string()).collect()),
            buscar_contatos: &|_| vec![],
            perguntar_claude: &|_| None,
            enviar_texto: fixo::<dyn Fn(&str, &str) -> String + 'a>(Box::new(move |n, t| { v.enviados.borrow_mut().push(("texto".into(), n.into(), t.into())); format!("✅ Mensagem enviada para {n}.") })),
            enviar_audio: fixo::<dyn Fn(&str, &str) -> String + 'a>(Box::new(move |n, t| { v.enviados.borrow_mut().push(("audio".into(), n.into(), t.into())); format!("✅ Áudio enviado para {n}.") })),
            resolver_arquivo: &|c| Ok(PathBuf::from(c)),
            enviar_arquivo: fixo::<dyn Fn(&str, &Path, &str) -> String + 'a>(Box::new(move |n, a, _| { v.enviados.borrow_mut().push(("arquivo".into(), n.into(), a.display().to_string())); "✅ ok".into() })),
        };
        p.responder = Box::new(move |_, t| {
            v.ia.borrow_mut().push(t.to_string());
            let mut r = v.respostas.borrow_mut();
            Ok(if r.is_empty() { "ia".to_string() } else { r.remove(0) })
        });
        p.uso_claude = Box::new(move || v.uso.get());
        p.add_uso_claude = Box::new(move || v.uso.set(v.uso.get() + 1));
        p.claude_segundo_plano = Box::new(move |t| v.claude.borrow_mut().push(t.to_string()));
        p.registrar_fato = Box::new(move |k, r, f| v.fatos.borrow_mut().push(format!("{k}|{r}|{f}")));
        p.nao_lidas = Box::new(|| "LISTA".into());
        p.ler_conversa = Box::new(|n| format!("CONV:{n}"));
        p.auto = Box::new(move |a| { v.outros.borrow_mut().push(format!("auto {a}")); "estado do atendimento".into() });
        p.escuta = Box::new(move |a| { v.outros.borrow_mut().push(format!("escuta {a}")); "Escuta por voz: parada".into() });
        p
    }

    fn falar(p: &Pecas, t: &str, origem: &str) -> (String, bool) {
        processar(p, &mut None, t, if origem == "whatsapp" { "wpp-dono" } else { origem }, origem, "")
    }

    #[test]
    fn lista_de_ferramentas_completa_e_escolhida_por_assunto() {
        let todas = ferramentas(&Catalogos { artcraft: "foto_girar".into(), artcraft_acoes: vec!["foto_girar".into()], ..Catalogos::default() });
        let nomes: BTreeSet<&str> = todas.iter().map(nome_de).collect();
        assert_eq!((todas.len(), nomes.len()), (59, 59));
        assert!(jf::NUCLEO.iter().all(|n| nomes.contains(n)));
        assert!(jf::GRUPOS.iter().all(|(_, g)| g.iter().all(|n| nomes.contains(n))));
        let art = todas.iter().find(|f| nome_de(f) == "artcraft").unwrap();
        assert!(art["function"]["description"].as_str().unwrap().contains("Acoes: foto_girar.") && art["function"]["parameters"]["properties"]["acao"]["enum"] == json!(["foto_girar"]));
        assert!(todas.iter().find(|f| nome_de(f) == "usar_skill").unwrap()["function"]["description"].as_str().unwrap().contains("Disponiveis: nenhuma."));
        assert!(todas.iter().all(|f| !f.to_string().contains("{{")));
        let para = |t: &str, h: &[&str]| -> BTreeSet<String> {
            let h: Vec<String> = h.iter().map(|s| s.to_string()).collect();
            ferramentas_para(&todas, t, &h, &[]).iter().map(|f| nome_de(f).to_string()).collect()
        };
        let tem = |s: &BTreeSet<String>, n: &str| s.contains(n);
        assert!(jf::NUCLEO.iter().all(|n| tem(&para("bom dia", &[]), n)));
        assert!(para("bom dia", &[]).len() < todas.len() / 3);
        assert!(tem(&para("me lembra amanhã às 9h de ligar pro banco", &[]), "agenda") && !tem(&para("me lembra amanhã às 9h", &[]), "airbnb_reserva"));
        assert!(tem(&para("aceita a reserva do hóspede João", &[]), "airbnb_reserva"));
        assert!(tem(&para("toca um rock aí", &[]), "spotify") && tem(&para("chegou alguma vaga no email?", &[]), "email"));
        assert!(tem(&para("sim, pode marcar", &["Quer que eu anote na agenda sexta às 15h?"]), "agenda")); // continua o assunto
        assert!(!tem(&para("sim, pode ser", &["Quer que eu anote na agenda?", "a", "b"]), "agenda")); // so as 2 ultimas falas
        assert!(tem(&para("faz uma lista dos agentes e manda pra Bia", &[]), "listar_agentes"));
        assert!(tem(&para("veja com o conselho o que melhorar no seu código", &[]), "melhorar_braniac"));
        assert!(tem(&para("tenho uma consulta no dentista", &[]), "agenda") && tem(&para("qual o ipca acumulado?", &[]), "juridico"));
        // o contexto anexado nao escolhe ferramenta
        assert!(!tem(&para("oi\n\n[Conversa recente no WhatsApp: toca uma música no spotify]", &[]), "spotify"));
        // gatilho que vem da configuracao (nome de cliente nao fica no codigo)
        let extras = vec![(Regex::new("(?i)cliente zeta").unwrap(), vec!["site_magno".to_string()])];
        assert!(ferramentas_para(&todas, "como está o site do cliente zeta?", &[], &extras).iter().any(|f| nome_de(f) == "site_magno"));
        let faltam = faltam_no_rust();
        assert!(faltam.contains(&"whatsapp_enviar".to_string()) && !faltam.contains(&"agenda".to_string()) && !faltam.contains(&"consultar_banco".to_string()));
    }

    #[test]
    fn fecho_do_agente_mostra_o_resultado_real() {
        let s = |x: &str| x.to_string();
        let direta = vec![s("CONTEUDO PRIVADO")];
        assert_eq!(fecho_do_agente(Ok(s("Pronto, é isso.")), &direta, &[], &[s("whatsapp_nao_lidas")]), Ok((s("CONTEUDO PRIVADO"), false)));
        assert_eq!(fecho_do_agente(Err(Falha::TodosFalharam(s("x"))), &direta, &[], &[]), Ok((s("CONTEUDO PRIVADO"), false)));
        // a IA caiu depois de agir: mostra o que foi feito
        let feitos = vec![(s("agenda"), s("Anotado para sexta 15h")), (s("email"), s("(mostrado ao dono)"))];
        assert_eq!(fecho_do_agente(Err(Falha::TodosFalharam(s("x"))), &[], &feitos, &[]), Ok((s("Anotado para sexta 15h"), false)));
        assert_eq!(fecho_do_agente(Err(Falha::TodosFalharam(s("x"))), &[], &feitos[1..], &[]), Ok((s("Feito."), false)));
        assert!(fecho_do_agente(Err(Falha::TodosFalharam(s("x"))), &[], &[], &[]).is_err());
        assert!(fecho_do_agente(Err(Falha::Desligado(s("x"))), &direta, &[], &[]).is_err());
        // disse que "preparou o envio" sem chamar a ferramenta
        assert!(fecho_do_agente(Ok(s("Preparei a mensagem. /sim?")), &[], &[], &[]).unwrap().0.starts_with("Não consegui preparar"));
        assert_eq!(fecho_do_agente(Ok(s("Enviei, confirmo o envio.")), &[], &[], &[s("whatsapp_enviar")]), Ok((s("Enviei, confirmo o envio."), true)));
        assert_eq!(fecho_do_agente(Ok(String::new()), &[], &[], &[]), Ok((s("Feito."), true)));
    }

    #[test]
    fn conversa_vai_para_a_ia_e_o_contexto_anexado_nao_dispara_atalho() {
        let (d, v) = (pasta("ia"), Visto::default());
        let p = pecas(&d, &v);
        assert_eq!(falar(&p, "sim", "web"), ("ia".to_string(), true)); // "sim" sem pendencia e conversa normal
        assert_eq!(falar(&p, "oi, tudo bem?", "web"), ("ia".to_string(), true));
        let r = falar(&p, "o que acha?\n\n[Conversa recente no WhatsApp: manda uma mensagem pra Bia dizendo que cheguei]", "whatsapp");
        assert_eq!(r, ("ia".to_string(), true));
        assert!(v.enviados.borrow().is_empty() && v.ia.borrow().last().unwrap().contains("[Conversa recente no WhatsApp")); // a IA ve o contexto
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn envio_direto_respeita_a_origem_e_a_familia() {
        let (d, v) = (pasta("envio"), Visto::default());
        let p = pecas(&d, &v);
        let r = falar(&p, "manda uma mensagem pra Deusa Bia dizendo que cheguei", "web").0;
        assert!(r.starts_with("✅") && v.enviados.borrow()[0] == ("texto".into(), "Beatriz Lima 🤍".into(), "cheguei".into()));
        falar(&p, "/enviar mainha | oi mãe", "web");
        assert_eq!(v.enviados.borrow()[1].1, "mainha");
        // de fora, sem autonomia: Telegram nunca; WhatsApp so para a familia
        assert!(falar(&p, "manda uma mensagem pra Deusa Bia dizendo oi", "telegram").0.starts_with("🔒"));
        assert!(falar(&p, "avisa a Rita Vendas que cheguei", "whatsapp").0.starts_with("🔒"));
        assert!(falar(&p, "/arquivo Rita Vendas|guia.pdf|segue", "whatsapp").0.starts_with("🔒"));
        assert_eq!(v.enviados.borrow().len(), 2);
        assert!(falar(&p, "avisa a bia que cheguei", "whatsapp").0.starts_with("✅"));
        assert!(falar(&p, "manda um áudio pra bia dizendo bom dia", "whatsapp").0.starts_with("✅ Áudio"));
        assert_eq!(v.enviados.borrow().len(), 4);
        // com a autonomia armada no PC, de fora tambem sai
        power::armar(&d, 10, 1000.0);
        assert!(falar(&p, "avisa a Rita Vendas que cheguei", "telegram").0.starts_with("✅"));
        assert!(v.ia.borrow().is_empty()); // nada disso passou pela IA
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn claude_e_acionado_na_hora_dentro_do_teto_e_da_trava() {
        let (d, v) = (pasta("claude"), Visto::default());
        let p = pecas(&d, &v);
        let r = falar(&p, "pede pro Claude abrir o painel e ver as reservas de hoje", "web").0;
        assert!(r.contains("Acionei o Claude") && !r.contains("Confirma?"));
        assert_eq!((v.claude.borrow().clone(), v.uso.get()), (vec!["abrir o painel e ver as reservas de hoje".to_string()], 1));
        assert!(falar(&p, "/claude faz algo", "telegram").0.starts_with("🔒"));
        assert!(falar(&p, "pede pro Claude ver o disco", "whatsapp").0.starts_with("🔒")); // igual ao Python: neste atalho o chat do dono nao fica livre
        v.uso.set(20);
        assert!(falar(&p, "/claude faz algo", "web").0.contains("teto"));
        assert_eq!(v.claude.borrow().len(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn desligado_so_aceita_comandos_de_energia_e_ligar_coisas_so_no_pc() {
        let (d, v) = (pasta("energia"), Visto::default());
        let p = pecas(&d, &v);
        assert!(falar(&p, "/auto on", "telegram").0.starts_with("🔒") && falar(&p, "/escuta ligar", "whatsapp").0.starts_with("🔒"));
        assert!(v.outros.borrow().is_empty());
        assert!(falar(&p, "/auto off", "telegram").0.contains("DESLIGADO") && falar(&p, "/auto on", "web").0.contains("LIGADO"));
        assert!(falar(&p, "/escuta off", "telegram").0.contains("DESLIGADA") && falar(&p, "/escuta", "web").0.contains("parada"));
        assert_eq!(*v.outros.borrow(), ["auto desligar", "auto ligar", "escuta desligar", "escuta estado"]);
        assert_eq!(falar(&p, "/desligar", "web").0, "Braniac desligado.");
        assert!(falar(&p, "olha meu whatsapp", "web").0.to_lowercase().contains("desligado"));
        assert!(falar(&p, "manda uma mensagem pra bia dizendo oi", "web").0.contains("desligado") && v.enviados.borrow().is_empty());
        assert!(falar(&p, "/auto off", "web").0.contains("DESLIGADO")); // desligar coisas continua valendo
        assert_eq!(falar(&p, "/ligar", "web").0, "Braniac ligado.");
        assert_eq!(falar(&p, "olha meu whatsapp", "web"), ("LISTA".to_string(), false));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn atalhos_sem_ia_erro_numero_mudo_e_leitura() {
        let (d, v) = (pasta("atalhos"), Visto::default());
        let mut p = pecas(&d, &v);
        p.historico = Box::new(|_, _| vec![json!({"role": "user", "content": "toca a playlist"}), json!({"role": "assistant", "content": "Mandei."})]);
        p.panico = Box::new(|o| format!("PANICO {o}"));
        assert!(falar(&p, "errou", "web").0.starts_with("Anotado") && d.join("erros_jarvis.jsonl").exists());
        assert!(falar(&p, "+55 (11) 99999-0000", "web").0.starts_with("Anotei o número +55 (11) 99999-0000"));
        assert_eq!(falar(&p, "Pânico!", "telegram").0, "PANICO telegram");
        let (r, da_ia) = falar(&p, "tô mal hoje, não aguento mais", "web");
        assert!(r.starts_with("Davi, eu li tudo") && r.contains("188") && da_ia);
        assert!(falar(&p, "/mudo 10", "web").0.contains("por 10 min") && mudo::ativo(&d, 1000.0) && !mudo::ativo(&d, 1000.0 + 601.0));
        assert!(falar(&p, "/mudo off", "web").0.starts_with("🔊") && !mudo::ativo(&d, 1000.0));
        assert!(falar(&p, "silêncio", "web").0.contains("até você desmutar"));
        assert!(falar(&p, "/humor 5", "web").0.contains("só trabalho") && falar(&p, "/memoria", "web").0.starts_with("Memória local: 0 mensagens"));
        assert!(falar(&p, "/esquecer preço da página", "web").0.starts_with("Não achei nada"));
        assert_eq!(falar(&p, "responde o e-mail do banco", "web").0, cmd::EMAIL_SO_LEITURA);
        assert!(falar(&p, "tenho e-mail novo?", "telegram").0.starts_with("🔒"));
        // reserva sem IA quando nao ha agente
        assert_eq!(falar(&p, "tem mensagem nova no zap?", "web"), ("LISTA".to_string(), false));
        assert_eq!(falar(&p, "lê a conversa da Beatriz no whatsapp", "web").0, "CONV:Beatriz Lima 🤍");
        assert_eq!(falar(&p, "/wpp mainha", "web").0, "CONV:mainha");
        assert!(falar(&p, "/wpp fulano de tal", "web").0.contains("Procurei 'fulano de tal'"));
        assert!(v.ia.borrow().is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn duvida_da_ia_vai_ao_agente_depois_ao_dono_e_fica_guardada() {
        let (d, v) = (pasta("duvida"), Visto::default());
        let p = pecas(&d, &v);
        let mut pend = None;
        *v.respostas.borrow_mut() = vec!["[DUVIDA] Quem é o Nando?".into(), "Certo, Nando é seu irmão.".into()];
        let r = processar(&p, &mut pend, "fala com o Nando", "web", "web", "").0;
        assert_eq!(r, "Preciso de uma ajuda sua: Quem é o Nando?");
        assert!(matches!(&pend, Some(Pendente { tipo: Tipo::DuvidaFato { .. }, .. })) && v.uso.get() == 1); // perguntou ao agente antes (conta no teto)
        assert_eq!(processar(&p, &mut pend, "é meu irmão mais novo", "web", "web", ""), ("Certo, Nando é seu irmão.".to_string(), true));
        assert!(pend.is_none() && v.fatos.borrow()[0] == "Quem é o Nando?|é meu irmão mais novo|dono");
        assert!(v.ia.borrow().last().unwrap().contains("[Contexto confirmado: Quem é o Nando? -> é meu irmão mais novo]"));
        // o agente sabe: nem incomoda o dono
        let mut p = pecas(&d, &v);
        p.consultar_claude = Box::new(|_| Consulta::Resp("Davi é o dono.".into()));
        *v.respostas.borrow_mut() = vec!["[DÚVIDA] Quem é o Davi no WhatsApp?".into(), "Entendido.".into()];
        assert_eq!(processar(&p, &mut pend, "quem sou eu no zap?", "web", "web", ""), ("Entendido.".to_string(), true));
        assert!(pend.is_none() && v.fatos.borrow()[1].ends_with("|claude"));
        // "não" cancela; pendencia vencida some sozinha
        pend = Some(Pendente { ate: 2000.0, tipo: Tipo::DuvidaFato { pergunta: "x".into(), original: "y".into() } });
        assert_eq!(processar(&p, &mut pend, "não, esquece", "web", "web", "").0, "Cancelado, nada foi feito.");
        pend = Some(Pendente { ate: 500.0, tipo: Tipo::DuvidaFato { pergunta: "x".into(), original: "y".into() } });
        assert_eq!(processar(&p, &mut pend, "bom dia", "web", "web", "").0, "ia");
        assert!(pend.is_none());
        // confirmacao pendente de envio continua atras da trava de origem
        pend = Some(Pendente { ate: 2000.0, tipo: Tipo::Whatsapp { nome: "Rita Vendas".into(), texto: "oi".into() } });
        assert!(processar(&p, &mut pend, "sim", "jarvis", "telegram", "").0.starts_with("🔒") && pend.is_none() && v.enviados.borrow().is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn agente_delegacao_e_travas_dos_atalhos_que_gastam_fora() {
        let (d, v) = (pasta("agente"), Visto::default());
        let extras = RefCell::new(vec![]);
        let mut p = pecas(&d, &v);
        p.tem_agente = true;
        p.ficha = "[FICHA]".into();
        p.agente = Box::new(|t, _, extra| {
            extras.borrow_mut().push(extra.to_string());
            match t {
                "cai" => Err(Falha::TodosFalharam("x".into())),
                "off" => Err(Falha::Desligado("Braniac desligado".into())),
                _ => Ok(("do agente".into(), true)),
            }
        });
        p.delegar = Box::new(|t| t.contains("saldo").then(|| Delegado { texto: "Saldo anotado pelo agente.".into(), local_only: true, ..Delegado::default() })
            .or_else(|| t.contains("loja").then(|| Delegado { texto: "RELATO DA LOJA".into(), ..Delegado::default() })));
        p.eh_musica = Box::new(|t| t.starts_with("toca "));
        p.musica = Box::new(|_| "Tocando.".into());
        p.conselho_pediu = Box::new(|t| t.contains("conselho"));
        p.conselho = Box::new(|_| "RELATORIO".into());
        p.profundo = Box::new(|_| "Trabalhando a fundo.".into());
        assert_eq!(falar(&p, "bom dia", "web"), ("do agente".to_string(), true));
        assert_eq!(falar(&p, "off", "web").0, "Braniac desligado. Mande /ligar para religar.");
        assert_eq!(falar(&p, "cai", "web"), ("ia".to_string(), true)); // sem agente agora: reserva
        // dado local: resposta montada sem IA
        assert_eq!(falar(&p, "como está o saldo?", "web"), ("Saldo anotado pelo agente.".to_string(), false));
        falar(&p, "como vai a loja?", "web");
        assert!(extras.borrow().last().unwrap().contains("[RELATORIO DOS AGENTES") && extras.borrow().last().unwrap().contains("RELATO DA LOJA"));
        // pergunta sobre o proprio Jarvis: ficha fixa, sem agente de assunto no lugar
        falar(&p, "como você funciona, e a loja?", "web");
        assert!(extras.borrow().last().unwrap().contains("[FICHA]") && !extras.borrow().last().unwrap().contains("RELATO"));
        // atalhos que agem fora: de fora do PC so com autonomia
        assert_eq!(falar(&p, "toca um rock", "web").0, "Tocando.");
        assert!(falar(&p, "toca um rock", "telegram").0.starts_with("🔒"));
        assert!(falar(&p, "chama o conselho sobre preço", "whatsapp").0.starts_with("🔒") && falar(&p, "chama o conselho sobre preço", "web").0 == "RELATORIO");
        assert!(falar(&p, "pergunta pro Grok quando foi a copa de 94", "telegram").0.starts_with("🔒"));
        assert!(falar(&p, "pergunta pro Grok oi", "web").0.starts_with("O que você quer que eu pergunte ao grok?"));
        assert!(falar(&p, "pesquisa a fundo o mercado", "telegram").0.starts_with("🔒"));
        assert_eq!(falar(&p, "pesquisa a fundo o mercado", "web").0, "Trabalhando a fundo.");
        assert!(falar(&p, "olha o whatsapp e responde as pessoas", "telegram").0.starts_with("🔒"));
        // pergunta a IA parceira com dado sensivel nao sai
        p.sensivel = Box::new(|t| t.contains("senha"));
        assert!(falar(&p, "pergunta pro Grok qual a senha padrão do roteador da operadora", "web").0.contains("pergunta barrada"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
