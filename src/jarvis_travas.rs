//! TRAVAS do Jarvis (porte de braniac/jarvis.py `_gate`, `_gate_envio`, `_eh_familia`, `DONO_CONV`/`DONO_LIVRE`, as travas por
//! ferramenta do `_Executor`; de braniac/whatsapp.py `resolver_nome`, `melhor_candidato`, `_chave_busca`, `destino_confere`,
//! `trava_destino`; de braniac/router.py `tem_dado_real`; de braniac/site_parceiro.py `mandou`).
//! Nada aqui envia nem le pagina: quem chama passa a origem, se a autonomia esta armada e o titulo da conversa aberta.
//! Diferencas do Python: os apelidos da familia que recebem recado sem autonomia armada nao ficam no codigo: so os genericos
//! ("mae", "mainha", "pai") sao fixos, o resto vem de dados/pessoal.json "familia_livre"; os avisos dizem "o dono" em vez do nome.
#![allow(dead_code)]
use crate::contexto::{norm, re, Ctx};
use serde_json::Value;
use std::path::Path;

/// Conversa do chat VERIFICADO do dono. So nela `DONO_LIVRE` roda sem a autonomia armada.
pub const DONO_CONV: &str = "wpp-dono";
pub const DONO_LIVRE: [&str; 2] = ["pc", "delegar_ao_claude"];
const FAMILIA_GENERICA: [&str; 3] = ["mae", "mainha", "pai"];
/// O que a IA recebe quando a trava barra: o aviso de verdade vai direto ao dono, nao e a IA que decide contornar.
pub const BLOQUEADO: &str = "(bloqueado por seguranca; o dono ja foi avisado; nao tente de outro jeito)";

/// De fora do PC (Telegram/WhatsApp) so se age com a autonomia armada na interface LOCAL. None = liberado.
pub fn gate(origem: &str, o_que: &str, armada: bool) -> Option<String> {
    (matches!(origem, "telegram" | "whatsapp") && !armada).then(|| {
        format!("🔒 Por segurança, por aqui (Telegram/WhatsApp) eu só converso e leio. Para {o_que} é preciso armar a 'Autonomia remota' na interface do PC: esse botão só funciona no computador.")
    })
}

/// Apelidos da familia que recebem recado do dono mesmo sem autonomia (ja normalizados).
pub fn familia_livre(dados: &Path) -> Vec<String> {
    let v: Value = std::fs::read_to_string(dados.join("pessoal.json")).ok().and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok()).unwrap_or_default();
    let mut l: Vec<String> = FAMILIA_GENERICA.iter().map(|s| s.to_string()).collect();
    l.extend(v["familia_livre"].as_array().into_iter().flatten().filter_map(Value::as_str).map(norm).filter(|s| !s.is_empty()));
    l
}

pub fn eh_familia(ctx: &Ctx, familia: &[String], nome: &str) -> bool {
    let alvo = norm(ctx.contato(nome).unwrap_or(nome));
    if alvo.is_empty() {
        return false;
    }
    let dentro = |x: &str| familia.iter().any(|a| norm(a) == x || ctx.contato(a).is_some_and(|c| !norm(c).is_empty() && norm(c) == x));
    dentro(&alvo) || dentro(&norm(nome))
}

/// Recado para a familia sai sem autonomia SO quando o pedido veio do WhatsApp; Telegram e qualquer outro contato seguem no `gate`.
pub fn gate_envio(origem: &str, o_que: &str, nome: &str, armada: bool, ctx: &Ctx, familia: &[String]) -> Option<String> {
    if origem == "whatsapp" && eh_familia(ctx, familia, nome) { None } else { gate(origem, o_que, armada) }
}

fn palavras(t: &str) -> Vec<String> {
    re!(w, r"\w+");
    w().find_iter(t).map(|m| m.as_str().to_string()).collect()
}

/// Todas as palavras ditas sao INICIO de alguma palavra do nome ('eu' nao casa com 'deusa').
fn casa_por_inicio(pedido_norm: &str, nome: &str) -> bool {
    let (q, toks) = (palavras(pedido_norm), palavras(&norm(nome)));
    !q.is_empty() && q.iter().all(|p| toks.iter().any(|t| t.starts_with(p.as_str())))
}

/// Apelido -> nome do contato; busca sem acento/maiuscula por inicio de palavra. Devolve os candidatos.
pub fn resolver_nome(ctx: &Ctx, nome: &str, visiveis: &[String]) -> Vec<String> {
    let alvo = norm(ctx.contato(nome).unwrap_or(nome));
    let exatos: Vec<String> = visiveis.iter().filter(|n| norm(n) == alvo).cloned().collect();
    if !exatos.is_empty() || alvo.is_empty() {
        return exatos;
    }
    visiveis.iter().filter(|n| casa_por_inicio(&alvo, n)).cloned().collect()
}

/// Letras em comum pelo maior bloco, recursivo (a conta do difflib.SequenceMatcher).
fn casados(a: &[char], b: &[char]) -> usize {
    let (mut ia, mut ib, mut n) = (0, 0, 0);
    for i in 0..a.len() {
        for j in 0..b.len() {
            let k = a[i..].iter().zip(&b[j..]).take_while(|(x, y)| x == y).count();
            if k > n {
                (ia, ib, n) = (i, j, k);
            }
        }
    }
    if n == 0 { 0 } else { n + casados(&a[..ia], &b[..ib]) + casados(&a[ia + n..], &b[ib + n..]) }
}

pub fn semelhanca(a: &str, b: &str) -> f64 {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a.is_empty() && b.is_empty() { 1.0 } else { 2.0 * casados(&a, &b) as f64 / (a.len() + b.len()) as f64 }
}

/// Entre varios parecidos, o que mais combina com TODAS as palavras ditas (tolera erro de transcricao). Empate: nome exato, senao o primeiro.
pub fn melhor_candidato(ctx: &Ctx, nome: &str, cands: &[String]) -> Option<String> {
    let base = norm(ctx.contato(nome).unwrap_or(nome));
    let q: Vec<&str> = base.split_whitespace().filter(|p| p.chars().count() > 1).collect();
    let pontos = |c: &str| {
        let n = norm(c);
        let total: f64 = q.iter().map(|p| n.split_whitespace().map(|t| semelhanca(p, t)).fold(0.0, f64::max)).filter(|r| *r >= 0.75).sum();
        total + if n == norm(nome) { 0.5 } else { 0.0 }
    };
    let mut melhor: Option<(&String, f64)> = None;
    for c in cands {
        let p = pontos(c);
        if melhor.map_or(true, |(_, m)| p > m) {
            melhor = Some((c, p));
        }
    }
    melhor.map(|(c, _)| c.clone())
}

/// Parte do nome que a busca do aplicativo entende (sem emoji/pontuacao): "Bia 🏍" -> "Bia".
pub fn chave_busca(nome: &str) -> String {
    let corte = nome.find(|c: char| !(c.is_alphanumeric() || c == '_' || c.is_whitespace() || c == '.' || c == '-')).unwrap_or(nome.len());
    let k = nome[..corte].trim();
    if k.is_empty() { nome.trim().to_string() } else { k.to_string() }
}

/// TRAVA FINAL: a conversa ABERTA tem que ser a PEDIDA (apelido conhecido ou as palavras do nome). Titulo vazio = nao conferiu = nao envia.
pub fn destino_confere(ctx: &Ctx, pedido: &str, aberto: &str) -> bool {
    if aberto.trim().is_empty() {
        return false;
    }
    !resolver_nome(ctx, pedido, &[aberto.to_string()]).is_empty() || casa_por_inicio(&norm(pedido), aberto)
}

/// None se pode enviar; senao o texto do bloqueio (nada e enviado). `aberto` = nome lido no topo da conversa aberta agora.
pub fn trava_destino(ctx: &Ctx, pedido: &str, aberto: &str) -> Option<String> {
    (!destino_confere(ctx, pedido, aberto)).then(|| {
        format!("🔒 Não enviei: abri a conversa '{}', mas o pedido era para '{pedido}'. Nada foi enviado; diga o nome do contato como aparece no WhatsApp.",
            if aberto.trim().is_empty() { "?" } else { aberto.trim() })
    })
}

re!(dado_real, concat!(
    r"(?i)\b\d{4}[ .-]?\d{4}[ .-]?\d{4}[ .-]?\d{4}\b|\b\d{3}\.?\d{3}\.?\d{3}-?\d{2}\b|",
    r"\b(?:senha|cvv|token|c[oó]digo\s+de\s+(?:verifica\w+|seguran\w+))\b\s*(?:[ée]|:|=)\s*\S{3,}|ag[eê]ncia\s*\d|conta\s*(?:corrente\s*)?\d"));
re!(pede_dado, concat!(
    r"(?i)\b(?:me\s+)?(?:mande|manda|envie|envia|passe|passa|informe|informa|digite|digita)\b[^.!?\n]{0,30}",
    r"\b(?:cart[aã]o|senha|cvv|cpf|c[oó]digo|token|dados\s+banc\w+)\b"));

/// O DADO (numero de cartao, CPF, senha com valor, conta) ou um pedido dele. So barra o envio do dado, nunca a conversa sobre o assunto.
pub fn tem_dado_real(texto: &str) -> bool {
    dado_real().is_match(texto) || pede_dado().is_match(texto)
}

pub const IAS_WEB: [&str; 6] = ["chatgpt", "gemini", "grok", "deepseek", "kimi", "meta"];

/// Pergunta a uma IA parceira: None = pode perguntar; senao o texto que volta no lugar. `confiavel` so quando o pedido sai do agente do proprio dono.
pub fn ia_web_barrada(ia: &str, pergunta: &str, sensivel: bool, confiavel: bool) -> Option<String> {
    let ia = ia.trim().to_lowercase();
    if !IAS_WEB.contains(&ia.as_str()) {
        return Some(format!("IA desconhecida: {ia}. Use chatgpt, gemini, grok, deepseek ou kimi."));
    }
    if pergunta.trim().is_empty() {
        return Some("pergunta vazia".into());
    }
    ((sensivel && !confiavel) || pergunta.trim().chars().count() > 3000)
        .then(|| "pergunta barrada: tem dado sensivel ou e longa demais; reescreva sem dados pessoais/financeiros".into())
}

re!(profundo_pedido, r"(?i)\b(?:a\s+fundo|com\s+calma|sem\s+pressa|melhor\s+(?:resposta|a[cç][aã]o|resultado)\s+poss[ií]vel|o\s+melhor\s+poss[ií]vel|modo\s+profundo)\b");
re!(so_responder, r"(?i)\bs[oó]\s+(?:responda|responde|me\s+responda)\b|\bn[aã]o\s+(?:execute|fa[cç]a)\s+(?:nenhuma\s+)?a[cç][aã]o");

/// 'a fundo / com calma / o melhor possivel': o pedido quer o modo profundo.
pub fn pede_profundo(t: &str) -> bool {
    profundo_pedido().is_match(t)
}

pub const SITE_MEXE: [&str; 3] = ["trocar_texto", "desfazer", "republicar"];

/// O dono mandou ESTA acao no site na mensagem de AGORA? ("sim" sozinho e o contexto anexado nao valem como ordem.)
pub fn site_mandou(acao: &str, texto: &str) -> bool {
    re!(corte, r"\s*\[Conversa recente");
    re!(trocar, r"(?i)troc|mud[ae]|corri[gj]|alter|substitu|consert|arrum|ajust|atualiz");
    re!(desfazer, r"(?i)desfa[zç]|desfez|revert|volt");
    re!(republicar, r"(?i)public");
    let agora = corte().split(texto).next().unwrap_or("");
    match acao {
        "trocar_texto" => trocar().is_match(agora),
        "desfazer" => desfazer().is_match(agora),
        "republicar" => republicar().is_match(agora),
        _ => false,
    }
}

/// Quem pediu e por onde: tudo que as travas de ferramenta precisam saber.
pub struct Pedido<'a> {
    pub origem: &'a str,
    pub conv: &'a str,
    /// o que o dono escreveu (com o contexto anexado, se houver)
    pub original: &'a str,
    pub armada: bool,
    pub ctx: &'a Ctx,
    pub familia: &'a [String],
}

#[derive(Debug, PartialEq)]
pub enum Trava {
    Livre,
    /// `direto` = aviso que vai ao dono sem passar pela IA; `para_ia` = o que a IA recebe como resultado da ferramenta.
    Bloqueada { direto: Option<String>, para_ia: String },
}

const SENSIVEIS: [(&str, &str); 14] = [
    ("whatsapp_enviar", "enviar mensagens"), ("whatsapp_enviar_audio", "enviar áudios"), ("whatsapp_enviar_arquivo", "enviar arquivos"),
    ("pesquisar_web", "pesquisar na internet"), ("melhorar_alma", "melhorar a A.L.M.A."), ("desfazer_melhoria_alma", "desfazer melhoria da A.L.M.A."),
    ("ler_aba", "ler abas do navegador"), ("delegar_ao_claude", "acionar o Claude"), ("whatsapp_atender", "atender o WhatsApp"),
    ("consultar_ia_web", "consultar outras IAs"), ("pc", "mexer no PC"), ("usar_notebook", "mexer no notebook"),
    ("trabalho_profundo", "trabalhar a fundo"), ("criar_habilidade", "criar habilidades"),
];
/// Ferramentas em que o texto da trava volta para a IA repassar (no Python o `_gate` e devolvido como resultado).
const GATE_SIMPLES: [(&str, &str); 6] = [
    ("desenhar_vetor", "desenhar com o VectorCraft"), ("rede_braniac", "usar a rede de Braniacs"), ("artcraft", "mexer em arquivos do PC com o ArtCraft"),
    ("email", "ler o seu e-mail"), ("gerar_imagem", "criar imagens e vídeos"), ("criar_video", "criar imagens e vídeos"),
];
/// (ferramenta, modulo que precisa estar ligado, nome que aparece no aviso).
const MODULOS: [(&str, &str, &str); 9] = [
    ("airbnb_pendentes", "airbnb", "Hospedagem (Airbnb)"), ("airbnb_ler", "airbnb", "Hospedagem (Airbnb)"), ("airbnb_responder", "airbnb", "Hospedagem (Airbnb)"),
    ("airbnb_reserva", "airbnb", "Hospedagem (Airbnb)"), ("nexodev", "programacao", "Programação (NexoDev)"), ("investimentos", "investimentos", "Investimentos (Quantix)"),
    ("compras", "compras", "Compras (Pechincha)"), ("cortes_video", "cortes_video", "Cortes de vídeo (Claquete)"), ("marketing", "marketing", "Marketing (Fornalha)"),
];

fn barrada(g: String) -> Trava {
    Trava::Bloqueada { direto: Some(g), para_ia: BLOQUEADO.into() }
}

/// Todas as travas que o executor de ferramentas aplica ANTES de agir, na ordem do Python. `modulo_ativo("airbnb")`, `sensivel(texto)`.
pub fn trava_ferramenta(p: &Pedido, nome: &str, args: &Value, modulo_ativo: &dyn Fn(&str) -> bool, sensivel: &dyn Fn(&str) -> bool) -> Trava {
    let arg = |k: &str| args[k].as_str().unwrap_or("");
    let g = |o_que: &str| gate(p.origem, o_que, p.armada);
    let para_ia = |t: String| Trava::Bloqueada { direto: None, para_ia: t };
    if let Some((_, o_que)) = SENSIVEIS.iter().find(|(n, _)| *n == nome) {
        let livre = (p.origem == "whatsapp" && matches!(nome, "whatsapp_enviar" | "whatsapp_enviar_audio") && eh_familia(p.ctx, p.familia, arg("nome")))
            || (p.conv == DONO_CONV && DONO_LIVRE.contains(&nome));
        if let Some(t) = (!livre).then(|| g(o_que)).flatten() {
            return barrada(t);
        }
    }
    if let Some((_, m, rotulo)) = MODULOS.iter().find(|(n, _, _)| *n == nome) {
        if !modulo_ativo(m) {
            return para_ia(format!("O módulo {rotulo} está desligado. Para ligar: painel > Módulos."));
        }
    }
    let acao = arg("acao");
    let com_aviso = match (nome, acao) {
        ("airbnb_responder" | "airbnb_reserva", _) => Some("responder hóspedes e decidir reservas no Airbnb"),
        ("nexodev", "aprovar") => Some("aprovar a entrega de um ticket da NexoDev"),
        ("investimentos", "aprovar" | "depositar" | "fechar") => Some("mexer na carteira simulada da Quantix"),
        ("cortes_video", "aprovar" | "rejeitar" | "confirmar_locutor") => Some("aprovar ou rejeitar um corte de vídeo"),
        ("vm_windows", "restaurar") => Some("restaurar a VM Windows (apaga o que foi feito nela desde o snapshot)"),
        ("compasso_rotinas", "ativar" | "pausar") => Some("ligar ou pausar rotinas do Compasso"),
        _ => None,
    };
    if let Some(t) = com_aviso.and_then(g) {
        return barrada(t);
    }
    // o proprio codigo: so o Telegram e barrado aqui (igual ao Python)
    if matches!(nome, "melhorar_braniac" | "desfazer_automelhoria") && p.origem == "telegram" && !p.armada {
        return barrada(g("mexer no meu proprio codigo").unwrap_or_default());
    }
    if let Some(t) = GATE_SIMPLES.iter().find(|(n, _)| *n == nome).and_then(|(_, o)| g(o)) {
        return para_ia(t);
    }
    match nome {
        "trabalho_profundo" if !pede_profundo(p.original) && (p.conv == "claude" || so_responder().is_match(p.original)) =>
            para_ia("(nao abra o modo profundo: e so uma pergunta. Responda direto, curto, com o que voce sabe)".into()),
        "gerar_imagem_meta" if arg("prompt").trim().is_empty() || sensivel(arg("prompt")) || arg("prompt").trim().chars().count() > 1500 =>
            para_ia("prompt vazio, longo demais ou com dado sensivel".into()),
        "consultar_ia_web" => ia_web_barrada(arg("ia"), arg("pergunta"), sensivel(arg("pergunta")), false).map_or(Trava::Livre, para_ia),
        // video ja publicado so volta a esteira a pedido de uma PESSOA: chat verificado do dono ou o PC
        "esteira_parceiro" if acao == "refazer" && p.conv != DONO_CONV && g("mandar refazer um vídeo que já está no site").is_some() =>
            para_ia("(refazer vídeo já publicado só a pedido do dono, pelo chat dele ou pelo PC)".into()),
        "site_parceiro" if SITE_MEXE.contains(&acao) => {
            if p.conv != DONO_CONV && g("mexer no site").is_some() {
                para_ia("(mexer no site só a pedido do dono, pelo chat dele ou pelo PC)".into())
            } else if !site_mandou(acao, p.original) {
                para_ia("(não mexi no site: só faço isso quando o dono manda nesta mensagem, com as palavras dele; pergunte a ele o que quer que mude)".into())
            } else {
                Trava::Livre
            }
        }
        _ => Trava::Livre,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx() -> Ctx {
        let mut c = Ctx { dono: "Davi".into(), ..Ctx::default() };
        for (a, n) in [("minha mae", "mainha"), ("tia nice", "mainha"), ("deusa bia", "Beatriz Lima 🤍"), ("bia", "Beatriz Lima 🤍"), ("davi", "Eu(SP)"), ("mae", "mainha")] {
            c.contatos.insert(norm(a), n.to_string());
        }
        c
    }

    fn familia() -> Vec<String> {
        ["mae", "mainha", "pai", "tia nice", "bia", "deusa bia", "beatriz", "nando"].iter().map(|s| s.to_string()).collect()
    }

    fn pedido<'a>(origem: &'a str, conv: &'a str, original: &'a str, armada: bool, c: &'a Ctx, f: &'a [String]) -> Pedido<'a> {
        Pedido { origem, conv, original, armada, ctx: c, familia: f }
    }

    #[test]
    fn gate_so_trava_de_fora_sem_autonomia() {
        assert!(gate("whatsapp", "usar a rede de Braniacs", false).unwrap().starts_with("🔒"));
        assert!(gate("telegram", "x", false).is_some());
        assert!(gate("telegram", "x", true).is_none() && gate("web", "x", false).is_none() && gate("pc", "x", false).is_none());
    }

    #[test]
    fn familia_passa_sem_autonomia_e_os_outros_nao() {
        let (c, f) = (ctx(), familia());
        for nome in ["minha mãe", "Tia Nice", "Deusa Bia", "bia", "Nando", "pai"] {
            assert!(gate_envio("whatsapp", "enviar mensagens", nome, false, &c, &f).is_none(), "{nome}");
        }
        for nome in ["Caio Vendas", "Padaria Central", "Rita", ""] {
            assert!(gate_envio("whatsapp", "enviar mensagens", nome, false, &c, &f).is_some(), "{nome}");
        }
        // pelo Telegram nem a familia passa; com a autonomia armada todo mundo passa
        assert!(gate_envio("telegram", "enviar mensagens", "bia", false, &c, &f).is_some());
        assert!(gate_envio("telegram", "enviar mensagens", "Rita", true, &c, &f).is_none());
    }

    #[test]
    fn familia_vem_da_configuracao() {
        let d = std::env::temp_dir().join(format!("braniac-jarvis_travas-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        assert_eq!(familia_livre(&d), vec!["mae", "mainha", "pai"]);
        std::fs::write(d.join("pessoal.json"), r#"{"familia_livre": ["Tia Nice", "Bia", 3]}"#).unwrap();
        assert_eq!(familia_livre(&d), vec!["mae", "mainha", "pai", "tia nice", "bia"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn apelidos_e_busca_sem_acento() {
        let c = ctx();
        let nomes: Vec<String> = ["Beatriz Lima 🤍", "mainha", "Banco X", "Eduarda"].iter().map(|s| s.to_string()).collect();
        assert_eq!(resolver_nome(&c, "Deusa Bia", &nomes), ["Beatriz Lima 🤍"]);
        assert_eq!(resolver_nome(&c, "beatriz", &nomes), ["Beatriz Lima 🤍"]);
        assert_eq!(resolver_nome(&c, "Tia Nice", &nomes), ["mainha"]);
        assert!(resolver_nome(&c, "zzz", &nomes).is_empty() && resolver_nome(&c, "", &nomes).is_empty());
    }

    #[test]
    fn destino_tem_que_ser_o_pedido() {
        let c = ctx();
        assert!(destino_confere(&c, "bia", "Beatriz Lima"));
        assert!(destino_confere(&c, "lima", "Deusa Beatriz Lima"));
        assert!(destino_confere(&c, "Eu(SP)", "Eu(SP)"));
        assert!(!destino_confere(&c, "Eu", "Deusa Beatriz Lima")); // 'eu' nao e inicio de 'deusa'
        assert!(!destino_confere(&c, "Caio Vendas", "Caio Obras"));
        assert!(!destino_confere(&c, "mae", "")); // nao leu o topo: nao envia
        assert!(trava_destino(&c, "bia", "Beatriz Lima 🤍").is_none());
        let t = trava_destino(&c, "Caio Vendas", "Caio Obras").unwrap();
        assert!(t.starts_with("🔒") && t.contains("Caio Obras") && t.contains("Nada foi enviado"));
        assert!(trava_destino(&c, "mae", " ").unwrap().contains("'?'"));
    }

    #[test]
    fn melhor_candidato_tolera_erro_e_prefere_todas_as_palavras() {
        let c = ctx();
        let cands: Vec<String> = ["Caio Obras", "Caio Pereira Fresco Da Silva"].iter().map(|s| s.to_string()).collect();
        assert_eq!(melhor_candidato(&c, "Caio Altista Fresco", &cands).unwrap(), "Caio Pereira Fresco Da Silva");
        assert_eq!(melhor_candidato(&c, "Caio", &cands).unwrap(), "Caio Obras"); // empate: o primeiro
        assert_eq!(melhor_candidato(&c, "caio obras", &cands).unwrap(), "Caio Obras");
        assert!(melhor_candidato(&c, "x", &[]).is_none());
        assert!((semelhanca("altista", "autista") - 6.0 / 7.0).abs() < 1e-9);
        assert_eq!(chave_busca("Manhinha 🏍"), "Manhinha");
        assert_eq!(chave_busca("🏍"), "🏍");
    }

    #[test]
    fn dado_real_barra_o_dado_nao_o_assunto() {
        assert!(tem_dado_real("o cartao e 1234 5678 9012 3456") && tem_dado_real("cpf 123.456.789-01") && tem_dado_real("a senha é abc123"));
        assert!(tem_dado_real("me manda o numero do cartão") && tem_dado_real("agência 1234"));
        assert!(!tem_dado_real("pode pagar no cartão ou no Pix") && !tem_dado_real("esqueci a senha do portão"));
    }

    #[test]
    fn ferramenta_de_envio_pra_familia_nao_e_bloqueada_e_o_resto_e() {
        let (c, f) = (ctx(), familia());
        let (sim, nao) = (|_: &str| true, |_: &str| false);
        let p = pedido("whatsapp", "jarvis", "avisa a bia", false, &c, &f);
        assert_eq!(trava_ferramenta(&p, "whatsapp_enviar", &json!({"nome": "Deusa Bia", "texto": "oi"}), &sim, &nao), Trava::Livre);
        assert_eq!(trava_ferramenta(&p, "whatsapp_enviar_audio", &json!({"nome": "bia", "texto": "oi"}), &sim, &nao), Trava::Livre);
        // arquivo nao entra na excecao da familia
        assert!(matches!(trava_ferramenta(&p, "whatsapp_enviar_arquivo", &json!({"nome": "bia"}), &sim, &nao), Trava::Bloqueada { direto: Some(_), .. }));
        match trava_ferramenta(&p, "whatsapp_enviar", &json!({"nome": "Caio Vendas", "texto": "oi"}), &sim, &nao) {
            Trava::Bloqueada { direto: Some(d), para_ia } => assert!(d.starts_with("🔒") && para_ia == BLOQUEADO),
            outro => panic!("{outro:?}"),
        }
        let tg = pedido("telegram", "jarvis", "avisa a bia", false, &c, &f);
        assert!(matches!(trava_ferramenta(&tg, "whatsapp_enviar", &json!({"nome": "bia"}), &sim, &nao), Trava::Bloqueada { .. }));
    }

    #[test]
    fn pc_e_claude_livres_so_no_chat_verificado_do_dono() {
        let (c, f) = (ctx(), familia());
        let (sim, nao) = (|_: &str| true, |_: &str| false);
        let dono = pedido("whatsapp", DONO_CONV, "procura o arquivo", false, &c, &f);
        assert_eq!(trava_ferramenta(&dono, "pc", &json!({"acao": "procurar", "alvo": "x"}), &sim, &nao), Trava::Livre);
        assert_eq!(trava_ferramenta(&dono, "delegar_ao_claude", &json!({"tarefa": "x"}), &sim, &nao), Trava::Livre);
        // o chat do dono nao libera o resto
        assert!(matches!(trava_ferramenta(&dono, "usar_notebook", &json!({"tarefa": "x"}), &sim, &nao), Trava::Bloqueada { .. }));
        assert!(matches!(trava_ferramenta(&dono, "whatsapp_enviar", &json!({"nome": "Rita"}), &sim, &nao), Trava::Bloqueada { .. }));
        let tele = pedido("telegram", "telegram", "procura o arquivo", false, &c, &f);
        assert!(matches!(trava_ferramenta(&tele, "pc", &json!({"acao": "procurar"}), &sim, &nao), Trava::Bloqueada { direto: Some(_), .. }));
        let outro = pedido("whatsapp", "jarvis", "procura o arquivo", false, &c, &f);
        assert!(matches!(trava_ferramenta(&outro, "pc", &json!({"acao": "procurar"}), &sim, &nao), Trava::Bloqueada { .. }));
        let web = pedido("web", "jarvis", "procura", false, &c, &f);
        assert_eq!(trava_ferramenta(&web, "pc", &json!({}), &sim, &nao), Trava::Livre);
    }

    #[test]
    fn travas_por_acao_modulo_e_site() {
        let (c, f) = (ctx(), familia());
        let (sim, nao) = (|_: &str| true, |_: &str| false);
        let z = pedido("whatsapp", "jarvis", "x", false, &c, &f);
        for (n, a) in [("nexodev", "aprovar"), ("investimentos", "depositar"), ("investimentos", "fechar"), ("cortes_video", "rejeitar"), ("vm_windows", "restaurar"),
                       ("compasso_rotinas", "pausar"), ("airbnb_reserva", "aceitar"), ("airbnb_responder", "")] {
            assert!(matches!(trava_ferramenta(&z, n, &json!({ "acao": a }), &sim, &nao), Trava::Bloqueada { direto: Some(_), .. }), "{n} {a}");
        }
        for (n, a) in [("nexodev", "quadro"), ("investimentos", "propor"), ("vm_windows", "estado"), ("compasso_rotinas", "listar"), ("airbnb_pendentes", ""), ("esteira_parceiro", "parar")] {
            assert_eq!(trava_ferramenta(&z, n, &json!({ "acao": a }), &sim, &nao), Trava::Livre, "{n} {a}");
        }
        assert!(matches!(trava_ferramenta(&z, "artcraft", &json!({}), &sim, &nao), Trava::Bloqueada { direto: None, .. }));
        // modulo desligado nem chega na acao
        match trava_ferramenta(&z, "airbnb_pendentes", &json!({}), &nao, &nao) {
            Trava::Bloqueada { direto: None, para_ia } => assert!(para_ia.contains("Hospedagem") && para_ia.contains("desligado")),
            o => panic!("{o:?}"),
        }
        // automelhoria: so o Telegram sem autonomia e barrado
        let tg = pedido("telegram", "jarvis", "x", false, &c, &f);
        assert!(matches!(trava_ferramenta(&tg, "melhorar_braniac", &json!({}), &sim, &nao), Trava::Bloqueada { direto: Some(_), .. }));
        assert_eq!(trava_ferramenta(&z, "melhorar_braniac", &json!({}), &sim, &nao), Trava::Livre);
        // refazer video publicado e mexer no site: chat do dono ou PC, e a ordem tem que estar na mensagem de agora
        assert!(matches!(trava_ferramenta(&z, "esteira_parceiro", &json!({"acao": "refazer", "numero": "170"}), &sim, &nao), Trava::Bloqueada { direto: None, .. }));
        let dono = pedido("whatsapp", DONO_CONV, "troca no site o texto antigo por texto novo", false, &c, &f);
        assert_eq!(trava_ferramenta(&dono, "esteira_parceiro", &json!({"acao": "refazer", "numero": "170"}), &sim, &nao), Trava::Livre);
        assert_eq!(trava_ferramenta(&dono, "site_parceiro", &json!({"acao": "trocar_texto"}), &sim, &nao), Trava::Livre);
        assert_eq!(trava_ferramenta(&dono, "site_parceiro", &json!({"acao": "pedidos"}), &sim, &nao), Trava::Livre);
        assert!(matches!(trava_ferramenta(&z, "site_parceiro", &json!({"acao": "republicar"}), &sim, &nao), Trava::Bloqueada { .. }));
        let so_sim = pedido("whatsapp", DONO_CONV, "sim\n\n[Conversa recente no WhatsApp: Jarvis: quer que eu troque o texto do site?]", false, &c, &f);
        match trava_ferramenta(&so_sim, "site_parceiro", &json!({"acao": "trocar_texto"}), &sim, &nao) {
            Trava::Bloqueada { para_ia, .. } => assert!(para_ia.contains("não mexi no site")),
            o => panic!("{o:?}"),
        }
        assert!(site_mandou("republicar", "publica de novo") && !site_mandou("desfazer", "sim") && !site_mandou("outra", "troca"));
    }

    #[test]
    fn ia_parceira_profundo_e_imagem_nao_recebem_dado_sensivel() {
        let (c, f) = (ctx(), familia());
        let sim = |_: &str| true;
        let sens = |t: &str| t.contains("senha");
        let web = pedido("web", "jarvis", "qual a capital da França? só responda", false, &c, &f);
        assert!(matches!(trava_ferramenta(&web, "trabalho_profundo", &json!({"objetivo": "x"}), &sim, &sens), Trava::Bloqueada { direto: None, .. }));
        let fundo = pedido("web", "claude", "pesquisa a fundo o mercado", false, &c, &f);
        assert_eq!(trava_ferramenta(&fundo, "trabalho_profundo", &json!({"objetivo": "x"}), &sim, &sens), Trava::Livre);
        assert!(matches!(trava_ferramenta(&web, "gerar_imagem_meta", &json!({"prompt": "minha senha num quadro"}), &sim, &sens), Trava::Bloqueada { .. }));
        assert!(matches!(trava_ferramenta(&web, "gerar_imagem_meta", &json!({"prompt": ""}), &sim, &sens), Trava::Bloqueada { .. }));
        assert_eq!(trava_ferramenta(&web, "gerar_imagem_meta", &json!({"prompt": "um farol ao por do sol"}), &sim, &sens), Trava::Livre);
        assert!(matches!(trava_ferramenta(&web, "consultar_ia_web", &json!({"ia": "grok", "pergunta": "qual a senha do wifi"}), &sim, &sens), Trava::Bloqueada { .. }));
        assert_eq!(trava_ferramenta(&web, "consultar_ia_web", &json!({"ia": "Grok", "pergunta": "quando foi a copa de 94?"}), &sim, &sens), Trava::Livre);
        assert!(ia_web_barrada("bard", "oi", false, false).unwrap().starts_with("IA desconhecida"));
        assert_eq!(ia_web_barrada("gemini", " ", false, false).unwrap(), "pergunta vazia");
        assert!(ia_web_barrada("gemini", &"x".repeat(3001), false, true).is_some());
        assert!(ia_web_barrada("gemini", "cpf do cliente", true, true).is_none()); // so o agente do proprio dono passa `confiavel`
    }
}
