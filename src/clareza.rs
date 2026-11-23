//! CLAREZA: entender a PERGUNTA e nao responder outra coisa (porte de braniac/clareza.py). Tudo regra; a unica chamada de IA
//! (reescrever pergunta curta cheia de "isso/ele/ela") entra como closure.
//!  1. precisa_reescrever/reescrever/nota_de_contexto: pergunta que depende do contexto vira pergunta completa, ou manda perguntar.
//!  2. sem_registro: "voce sabe o que e o projeto X?" sem nada no banco -> "nao tenho isso", em vez de chutar.
//!  3. fora_do_assunto/so_promessa/conferir: resposta que foge da pergunta ou so promete olhar depois e refeita UMA vez.
//!  4. polir: ultimo filtro ANTES de mandar a terceiros (agradecimento, repeticao, vazamento de nomes internos, textao).
//!  5. classificar/resumo: tipos de falha e quantas vezes cada trava agiu (dados/clareza.jsonl).
//! O banco ("tem fato sobre X?"), a IA e o registro de evento entram por parametro.
#![allow(dead_code)]
use crate::contexto::{self, frases, re, Ctx};
use crate::revisao;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const STOP: &str = "para como quando onde porque pois isso essa esse esta este aquilo aquela aquele voce voces tambem ainda muito mais menos sobre entre depois antes \
                    quais qual quem tinha tenho temos pode posso quer quero vamos fazer feito disse falou algo alguma algum nada tudo todas todos mesmo mesma seria \
                    ainda agora hoje ontem dele dela deles delas nele nela comigo conosco aquilo estou estamos ficou fica ficar";
const REESCREVE: &str = "Reescreva a ULTIMA mensagem do usuario como uma pergunta COMPLETA e autossuficiente, trocando 'isso/ele/ela/la' pelo que o contexto indica. \
                         Responda SO com a pergunta reescrita, em uma linha, sem explicar. Se a mensagem NAO e uma pergunta (e pedido ou afirmacao), responda exatamente: IGUAL. \
                         Se o contexto NAO deixa claro a que a mensagem se refere, responda exatamente: AMBIGUO";

re!(vaza, r"(?i)\b(?:braniac|claude|ollama|groq|gemini|chatgpt|openai|conselho\s+de\s+ias?|modelo\s+de\s+linguagem)\b");
re!(anafora, concat!(
    r"(?i)\b(?:isso|isto|disso|nisso|aquilo|ele|ela|eles|elas|esse|essa|esses|essas|aquele|aquela|desse|dessa|nesse|nessa|l[aá]|assim|",
    r"o\s+mesmo|a\s+mesma|de\s+novo|por\s+onde|como\s+assim|qual\s+deles|qual\s+delas)\b|^\s*(?:e\s|u[eé]\b|mas\s|ent[aã]o\s)"));
re!(entidade, r"(?i:\b(?:projeto|app|aplicativo|sistema|site|cliente|empresa|loja|contato)\s+(?:chamad[oa]\s+|d[oa]s?\s+)?)([A-ZÀ-Ú][\wÀ-ú\-]{2,30})");
re!(pede_saber, r"(?i)\?|\bsabe\b|\bconhece\b|\blembra\b|o\s+que\s+[eé]|quem\s+[eé]|\bfalou\s+(?:de|sobre)\b");
re!(pergunta_re, r"(?i)\?|^\s*(?:u[eé]\b|qual|quais|quem|quando|onde|como|por\s+que|o\s+que|e\s+(?:o|a|os|as)\s|cad[eê])");
re!(bloco_whatsapp, r"(?s)\[Conversa recente no WhatsApp:(.*?)\]\s*$");
re!(promessa, concat!(
    r"(?i)\b(?:vou|irei|deixa eu|deixe-me)\s+(?:dar uma\s+)?(?:olhad\w*|checad\w*|conferid\w*|olhar|checar|verificar|conferir|ver)\b",
    r"[^.!?]{0,60}?\b(?:j[aá]|logo|depois|em seguida)\s+(?:te\s+|lhe\s+)?(?:digo|falo|aviso|retorno|conto|respondo)"));
re!(palavra, r"[a-zà-ú]{4,}");
re!(quer_detalhe, r"(?i)detalh|explic|passo|como\s+funciona");
re!(sim_virgula, r"(?i)^\s*sim,");

fn tamanho(t: &str) -> usize {
    t.chars().count()
}

fn fim(t: &str, n: usize) -> &str {
    t.char_indices().rev().nth(n.saturating_sub(1)).map_or(t, |(i, _)| &t[i..])
}

fn corte(t: &str, n: usize) -> String {
    t.chars().take(n).collect()
}

/// Uma linha em dados/clareza.jsonl: qual trava agiu.
pub fn evento(dados: &Path, ts: f64, tipo: &str, detalhe: &str) {
    use std::io::Write;
    let _ = std::fs::create_dir_all(dados);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dados.join("clareza.jsonl")) {
        let _ = writeln!(f, "{}", json!({ "ts": ts, "tipo": tipo, "d": corte(detalhe, 120) }));
    }
}

// ---------- 1. entender a pergunta ----------

/// PERGUNTA curta que depende de contexto ("isso", "ele", "por onde"...). Pedido/afirmacao e pergunta completa nao gastam IA.
pub fn precisa_reescrever(t: &str) -> bool {
    let t = t.trim();
    (2..=90).contains(&tamanho(t)) && !t.starts_with('/') && pergunta_re().is_match(t) && anafora().is_match(t)
}

/// Ultimas trocas como texto: o bloco de conversa anexado ao pedido ou o historico (papel "user"/outro, conteudo).
pub fn contexto_recente(t_llm: &str, historico: &[(String, String)], dono: &str) -> String {
    if let Some(b) = bloco_whatsapp().captures(t_llm).and_then(|c| c.get(1)).map(|m| m.as_str().trim()).filter(|b| !b.is_empty()) {
        return fim(b, 700).to_string();
    }
    let dono = if dono.trim().is_empty() { "Usuário" } else { dono.trim() };
    let linhas: Vec<String> = historico[historico.len().saturating_sub(5)..].iter().map(|(papel, c)| format!("{}: {}", if papel == "user" { dono } else { "Jarvis" }, corte(c, 180))).collect();
    fim(&linhas.join("\n"), 700).to_string()
}

/// ia(sistema, texto) -> texto. Devolve a pergunta completa, "AMBIGUO" ou None (falhou ou nao e pergunta: segue sem reescrita).
pub fn reescrever(pergunta: &str, ctx: &str, ia: &dyn Fn(&str, &str) -> Option<String>) -> Option<String> {
    let r = ia(REESCREVE, &format!("CONTEXTO:\n{ctx}\n\nULTIMA MENSAGEM: {pergunta}"))?;
    let r = r.trim().trim_matches('"');
    if r.is_empty() || r.contains('\n') || tamanho(r) > 260 || r.to_uppercase().starts_with("IGUAL") {
        return None;
    }
    Some(if r.to_uppercase().starts_with("AMBIGUO") { "AMBIGUO".into() } else { r.to_string() })
}

/// Texto para anexar ao pedido ("" se nao precisa ou nao deu): a pergunta ja resolvida ou o aviso de que e ambigua.
/// `ctx` = contexto_recente(...); `ev(tipo, detalhe)` registra a trava.
pub fn nota_de_contexto(t: &str, ctx: &str, ia: &dyn Fn(&str, &str) -> Option<String>, ev: &dyn Fn(&str, &str)) -> String {
    if !precisa_reescrever(t) || tamanho(ctx) < 20 {
        return String::new();
    }
    match reescrever(t, ctx, ia).as_deref() {
        None => String::new(),
        Some("AMBIGUO") => {
            ev("ambiguo", t);
            "\n\n[ATENCAO: a mensagem depende do contexto e o contexto NAO esclarece a que ela se refere. NAO chute: pergunte numa frase curta a que a pessoa se refere.]".into()
        }
        Some(r) => {
            ev("reescreveu", &format!("{t} => {r}"));
            format!("\n\n[A PERGUNTA, COM O CONTEXTO RESOLVIDO: {r} (responda a esta; nao fuja dela)]")
        }
    }
}

// ---------- 2. sem dado, sem chute ----------

/// "voce sabe o que e o projeto X?" sem nada no banco -> resposta honesta. None se nao e essa pergunta ou ha registro.
/// So vale para "projeto/app/sistema/cliente/loja + Nome": conhecimento geral nao entra.
pub fn sem_registro(t: &str, tem_fato: &dyn Fn(&str) -> bool, ev: &dyn Fn(&str, &str)) -> Option<String> {
    let t = t.trim();
    let nome = entidade().captures(t)?.get(1)?.as_str();
    if !pede_saber().is_match(t) || tamanho(t) > 140 || tem_fato(nome) {
        return None;
    }
    ev("sem_registro", nome);
    Some(format!("Não tenho nada registrado sobre {nome}, e prefiro não chutar. Me conta o que é, ou quer que eu pesquise?"))
}

// ---------- 3. resposta que foge do assunto ----------

fn radicais(txt: &str) -> BTreeSet<String> {
    let t = txt.to_lowercase();
    palavra().find_iter(&t).map(|m| m.as_str()).filter(|w| !STOP.split_whitespace().any(|s| s == *w)).map(|w| corte(w, 5)).collect()
}

/// Pergunta com assunto claro (4+ palavras de conteudo) e resposta que nao toca em nenhuma. Pergunta de volta ("...?") nunca e "fora".
pub fn fora_do_assunto(pergunta: &str, resposta: &str) -> bool {
    let (p, r) = (radicais(pergunta), resposta.trim());
    p.len() >= 4 && tamanho(r) >= 40 && !r.ends_with('?') && p.is_disjoint(&radicais(r))
}

/// "Vou dar uma olhada e ja te digo" sem olhar: resposta curta que so promete = refazer.
pub fn so_promessa(texto: &str) -> bool {
    let t = texto.trim();
    tamanho(t) < 260 && promessa().is_match(t)
}

/// Resposta fora do assunto ou que so promete olhar depois: refaz UMA vez (refazer(dica) -> texto); se a nova tambem nao
/// servir, fica com a nova mesmo assim. Sem nova resposta, fica a original.
pub fn conferir(resposta: &str, pergunta: &str, refazer: &dyn Fn(&str) -> Option<String>, ev: &dyn Fn(&str, &str)) -> String {
    let prometeu = so_promessa(resposta);
    if !prometeu && !fora_do_assunto(pergunta, resposta) {
        return resposta.to_string();
    }
    ev(if prometeu { "so_promessa" } else { "fora_do_assunto" }, &corte(pergunta, 80));
    let dica = if prometeu {
        "\n\n[ATENCAO: sua resposta anterior so PROMETEU verificar depois. Use AGORA a ferramenta certa e responda com o dado; \
         se nao houver ferramenta para isso, diga o que sabe e o que nao sabe. Nada de 'vou ver e ja te digo'.]".to_string()
    } else {
        format!("\n\n[ATENCAO: sua resposta anterior NAO respondeu a pergunta. Responda DIRETAMENTE a: \"{pergunta}\". Se faltar dado, diga que nao tem; nao invente nem mude de assunto.]")
    };
    refazer(&dica).filter(|n| !n.is_empty()).unwrap_or_else(|| resposta.to_string())
}

// ---------- 4. filtro ANTES de mandar a terceiros ----------

/// Texto final para um TERCEIRO, ou None (nao mandar). O dono e a gente de casa passam direto.
/// `saidas` = o que ja dissemos a essa pessoa (ultimas mensagens nossas).
pub fn polir(ctx: &Ctx, nome: &str, texto: &str, pergunta: &str, saidas: &[String], ev: &dyn Fn(&str, &str)) -> Option<String> {
    if texto.trim().is_empty() {
        return None;
    }
    if ctx.de_casa(nome) {
        return Some(texto.to_string());
    }
    if contexto::agradecimento(pergunta).is_some() {
        ev("polir_agradecimento", nome);
        return None;
    }
    if tamanho(texto) > 30 && revisao::repete(texto, saidas) { // resposta curta ("Tem sim!") pode se repetir
        ev("polir_repeticao", nome);
        return None;
    }
    let mut fr = frases(texto.trim());
    if fr.iter().any(|f| vaza().is_match(f)) {
        fr.retain(|f| !vaza().is_match(f));
        ev("polir_vazamento", nome);
        if fr.is_empty() {
            return None;
        }
    }
    let mut saida = fr.join(" ");
    if !pergunta.is_empty() && tamanho(pergunta.trim()) <= 40 && tamanho(&saida) > 320 && !quer_detalhe().is_match(pergunta) {
        // a apresentacao do 1o contato fica inteira; so o corpo e cortado, e so ENTRE frases
        let abertura = ctx.abertura_padrao(nome);
        let (cabeca, corpo) = match saida.strip_prefix(&abertura) {
            Some(c) if !abertura.is_empty() => (abertura.as_str(), c),
            _ => ("", saida.as_str()),
        };
        let partes = frases(corpo.trim());
        if partes.len() > 2 {
            saida = format!("{cabeca}{}", partes[..2].join(" "));
            ev("polir_tamanho", nome);
        }
    }
    Some(saida)
}

// ---------- 5. classificar e contar falhas ----------

pub fn classificar(ctx: &Ctx, pedido: &str, resposta: &str, tem_fato: &dyn Fn(&str) -> bool) -> Vec<&'static str> {
    let mut tipos = vec![];
    if fora_do_assunto(pedido, resposta) {
        tipos.push("fora_do_assunto");
    }
    if vaza().is_match(resposta) {
        tipos.push("vazou_interno");
    }
    if tamanho(pedido.trim()) <= 40 && tamanho(resposta) > 320 {
        tipos.push("exagerou");
    }
    if contexto::quem_sou(&ctx.dono).is_match(resposta) {
        tipos.push("reapresentou");
    }
    if sim_virgula().is_match(resposta) && sem_registro(pedido, tem_fato, &|_, _| ()).is_some() {
        tipos.push("inventou");
    }
    if tipos.is_empty() { vec!["outro"] } else { tipos }
}

fn jsonl(arq: &Path) -> Vec<Value> {
    std::fs::read_to_string(arq).map(|t| t.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()).unwrap_or_default()
}

/// {dias, marcados: {tipo: n}, claudinho: {tipo: n}, travas: {tipo: n}} dos ultimos `dias` (erros marcados pelo dono,
/// achados da auditoria e travas deste modulo). `ts` = agora, em segundos.
pub fn resumo(dados: &Path, ctx: &Ctx, ts: f64, dias: f64, tem_fato: &dyn Fn(&str) -> bool) -> Value {
    let limite = ts - dias * 86400.0;
    let recente = |e: &Value| e["ts"].as_f64().unwrap_or(0.0) >= limite;
    let (mut marcados, mut claud, mut travas): (BTreeMap<String, u64>, BTreeMap<String, u64>, BTreeMap<String, u64>) = Default::default();
    for e in jsonl(&dados.join("erros_jarvis.jsonl")).iter().filter(|e| recente(e)) {
        let tipos: Vec<String> = match e["tipos"].as_array().filter(|a| !a.is_empty()) {
            Some(a) => a.iter().filter_map(|t| t.as_str()).map(str::to_string).collect(),
            None => classificar(ctx, e["pedido"].as_str().unwrap_or(""), e["resposta_errada"].as_str().unwrap_or(""), tem_fato).iter().map(|t| t.to_string()).collect(),
        };
        for t in tipos {
            *marcados.entry(t).or_default() += 1;
        }
    }
    for (arq, mapa) in [(dados.join("claudinho").join("achados.jsonl"), &mut claud), (dados.join("clareza.jsonl"), &mut travas)] {
        for e in jsonl(&arq).iter().filter(|e| recente(e)) {
            if let Some(t) = e["tipo"].as_str() {
                *mapa.entry(t.to_string()).or_default() += 1;
            }
        }
    }
    json!({ "dias": dias, "marcados": marcados, "claudinho": claud, "travas": travas })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const CURTA: &str = "Ué, a Bia falou isso por onde?";
    const PROJETO: &str = "Você sabe o que é o projeto Tucumã?";

    fn nada(_: &str, _: &str) {}

    fn ctx() -> Ctx {
        let mut c = Ctx { dono: "Davi".into(), casa: vec!["mae".into()], ..Default::default() };
        c.contatos.insert("davi".into(), "Meu Zap".into());
        c.contatos.insert("mae".into(), "Mamãe ❤️".into());
        c
    }

    #[test]
    fn reescreve_so_mensagem_curta_que_depende_de_contexto() {
        assert!(precisa_reescrever(CURTA) && precisa_reescrever("e o preço?"));
        assert!(!precisa_reescrever("A Bia disse que o estoque acabou, anota isso")); // afirmacao/pedido
        assert!(!precisa_reescrever("qual o saldo da conta?") && !precisa_reescrever("/auto off") && !precisa_reescrever("?"));
        let c = "Davi: a Bia disse que o estoque acabou\nJarvis: entendi";
        assert!(reescrever(CURTA, c, &|_, _| Some("\"Onde a Bia disse que o estoque acabou?\"".into())).is_some_and(|r| r.starts_with("Onde a Bia")));
        assert_eq!(reescrever(CURTA, c, &|_, _| Some("ambiguo.".into())).as_deref(), Some("AMBIGUO"));
        for lixo in [Some("linha 1\nlinha 2".to_string()), None, Some("IGUAL".into()), Some("  ".into()), Some("x".repeat(261))] {
            assert_eq!(reescrever(CURTA, c, &|_, _| lixo.clone()), None);
        }
    }

    #[test]
    fn nota_de_contexto_usa_o_historico_e_nao_gasta_ia_a_toa() {
        let hist = vec![("user".to_string(), "a Bia disse que o estoque acabou".to_string()), ("assistant".to_string(), "entendi, vou anotar isso".to_string())];
        let c = contexto_recente(CURTA, &hist, "Davi");
        assert_eq!(c, "Davi: a Bia disse que o estoque acabou\nJarvis: entendi, vou anotar isso");
        assert_eq!(contexto_recente("oi\n\n[Conversa recente no WhatsApp:\nEla: oi\nEu: ola]", &hist, "Davi"), "Ela: oi\nEu: ola");
        let eventos = RefCell::new(vec![]);
        let ev = |t: &str, _: &str| eventos.borrow_mut().push(t.to_string());
        assert!(nota_de_contexto(CURTA, &c, &|_, _| Some("Onde a Bia disse que o estoque acabou?".into()), &ev).contains("COM O CONTEXTO RESOLVIDO: Onde a Bia"));
        assert!(nota_de_contexto(CURTA, &c, &|_, _| Some("AMBIGUO".into()), &ev).contains("NAO chute"));
        let sem_ia = |_: &str, _: &str| -> Option<String> { panic!("pergunta completa nao gasta IA") };
        assert_eq!(nota_de_contexto("qual o saldo da conta?", &c, &sem_ia, &ev), "");
        assert_eq!(nota_de_contexto(CURTA, "curto", &sem_ia, &ev), "");
        assert_eq!(*eventos.borrow(), ["reescreveu", "ambiguo"]);
    }

    #[test]
    fn sem_registro_nao_chuta_e_so_vale_para_projeto_ou_cliente() {
        let r = sem_registro(PROJETO, &|_| false, &nada);
        assert!(r.is_some_and(|r| r.contains("Tucumã") && r.contains("Não tenho")));
        assert_eq!(sem_registro(PROJETO, &|n| n == "Tucumã", &nada), None);
        assert_eq!(sem_registro("o que é Pix copia e cola?", &|_| false, &nada), None); // conhecimento geral nao entra
        assert_eq!(sem_registro("abre o projeto Tucumã", &|_| false, &nada), None); // nao pediu para saber
    }

    #[test]
    fn resposta_fora_do_assunto_ou_so_promessa_e_refeita_uma_vez() {
        let p = "Qual o preço da landing page para a esfiharia do Tito?";
        let fora = "Bom dia! Hoje o tempo está ótimo em Vila Nova, aproveite o dia lá fora.";
        assert!(fora_do_assunto(p, fora));
        assert!(!fora_do_assunto(p, "A landing page custa R$ 150 na promoção."));
        assert!(!fora_do_assunto(p, "Qual esfiharia você quer dizer, a do Tito?")); // pergunta de volta nunca e "fora"
        assert!(!fora_do_assunto("e o preço?", fora)); // pergunta sem assunto claro nao julga
        let dicas = RefCell::new(vec![]);
        let refazer = |d: &str| {
            dicas.borrow_mut().push(d.to_string());
            Some("A landing page custa R$ 150.".to_string())
        };
        assert_eq!(conferir(fora, p, &refazer, &nada), "A landing page custa R$ 150.");
        assert!(dicas.borrow().len() == 1 && dicas.borrow()[0].contains(p));
        assert_eq!(conferir("A landing page custa R$ 150.", p, &|_| panic!("resposta boa nao refaz"), &nada), "A landing page custa R$ 150.");
        assert_eq!(conferir(fora, p, &|_| None, &nada), fora); // sem nova resposta fica a original
        // so promessa
        assert!(so_promessa("Vou dar uma olhada na esteira e já te digo.") && so_promessa("Deixa eu verificar aqui, logo te aviso"));
        assert!(!so_promessa("Olhei a esteira: 3 vídeos prontos.") && !so_promessa("Vou ver o filme hoje."));
        assert_eq!(conferir("Vou conferir e já te falo.", "oi", &refazer, &nada), "A landing page custa R$ 150.");
        assert!(dicas.borrow()[1].contains("so PROMETEU"));
    }

    #[test]
    fn polir_filtra_antes_de_mandar_a_terceiro() {
        let c = ctx();
        let eventos = RefCell::new(vec![]);
        let ev = |t: &str, _: &str| eventos.borrow_mut().push(t.to_string());
        assert_eq!(polir(&c, "Fulano", "Pronto! Usei o Claude pra conferir. O valor é R$ 150.", "quanto custa?", &[], &ev).as_deref(), Some("Pronto! O valor é R$ 150."));
        assert_eq!(polir(&c, "Fulano", "Sou o Braniac, o sistema do Davi.", "oi", &[], &ev), None); // so tinha vazamento
        assert_eq!(polir(&c, "Fulano", "De nada, fico à disposição!", "valeu, obrigado", &[], &ev), None); // agradecimento: sem resposta
        let ja = vec!["O valor é R$ 150 para a primeira página.".to_string()];
        assert_eq!(polir(&c, "Fulano", "O valor é R$ 150 para a primeira página.", "quanto custa?", &ja, &ev), None);
        assert_eq!(polir(&c, "Fulano", "Tem sim!", "tem vaga?", &["Tem sim!".to_string()], &ev).as_deref(), Some("Tem sim!")); // curta pode repetir
        assert_eq!(polir(&c, "Fulano", "  ", "oi", &[], &ev), None);
        let longo = (1..12).map(|i| format!("Frase número {i} bem comprida para encher espaço.")).collect::<Vec<_>>().join(" ");
        assert_eq!(polir(&c, "Fulano", &longo, "e o prazo?", &[], &ev).as_deref(), Some("Frase número 1 bem comprida para encher espaço. Frase número 2 bem comprida para encher espaço."));
        assert_eq!(polir(&c, "Fulano", &longo, "me explica em detalhes como funciona", &[], &ev).as_deref(), Some(longo.as_str()));
        // a apresentacao do 1o contato fica inteira; so o corpo e cortado
        let com_abertura = format!("{}{longo}", c.abertura_padrao("Fulano"));
        assert_eq!(polir(&c, "Fulano", &com_abertura, "e o prazo?", &[], &ev), Some(format!("{}Frase número 1 bem comprida para encher espaço. Frase número 2 bem comprida para encher espaço.", c.abertura_padrao("Fulano"))));
        // o dono e a gente de casa passam direto
        for casa in ["Meu Zap", "Davi Souza", "Mamãe ❤️"] {
            assert_eq!(polir(&c, casa, "Usei o Claude e o Braniac.", "oi", &[], &ev).as_deref(), Some("Usei o Claude e o Braniac."), "{casa}");
        }
        assert_eq!(*eventos.borrow(), ["polir_vazamento", "polir_vazamento", "polir_agradecimento", "polir_repeticao", "polir_tamanho", "polir_tamanho"]);
    }

    #[test]
    fn classifica_os_erros() {
        let c = ctx();
        assert!(classificar(&c, PROJETO, "Sim, senhor. O projeto Tucumã é um assistente virtual para a loja.", &|_| false).contains(&"inventou"));
        assert!(!classificar(&c, PROJETO, "Sim, senhor. O projeto Tucumã é um assistente virtual para a loja.", &|_| true).contains(&"inventou"));
        assert_eq!(classificar(&c, "oi", "Sou o Jarvis, assistente de IA do Davi, e posso ajudar.", &|_| true), ["reapresentou"]);
        assert_eq!(classificar(&c, "qual o prazo da página?", "ok", &|_| true), ["outro"]);
        assert_eq!(classificar(&c, "oi", &format!("Usei o Gemini. {}", "x".repeat(320)), &|_| true), ["vazou_interno", "exagerou"]);
    }

    #[test]
    fn resumo_conta_marcados_auditoria_e_travas() {
        let d = std::env::temp_dir().join(format!("braniac-clareza-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("claudinho")).unwrap();
        let agora = 2_000_000_000.0;
        std::fs::write(d.join("erros_jarvis.jsonl"), format!("{}\n{}\n{}\nlinha quebrada\n",
            json!({"ts": agora, "pedido": "oi", "resposta_errada": "x", "tipos": ["exagerou"]}),
            json!({"ts": agora - 30.0 * 86400.0, "pedido": "a", "resposta_errada": "b", "tipos": ["velho"]}),
            json!({"ts": agora, "pedido": "oi", "resposta_errada": "Usei o Groq."}))).unwrap();
        std::fs::write(d.join("claudinho").join("achados.jsonl"), format!("{}\n", json!({"ts": agora, "tipo": "repetiu"}))).unwrap();
        evento(&d, agora, "ambiguo", &"x".repeat(500));
        let r = resumo(&d, &ctx(), agora, 7.0, &|_| true);
        assert_eq!((r["marcados"].clone(), r["claudinho"].clone(), r["travas"].clone()), (json!({"exagerou": 1, "vazou_interno": 1}), json!({"repetiu": 1}), json!({"ambiguo": 1})));
        assert_eq!(jsonl(&d.join("clareza.jsonl"))[0]["d"].as_str().map(str::len), Some(120));
        let _ = std::fs::remove_dir_all(&d);
    }
}
