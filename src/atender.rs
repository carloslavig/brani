//! Atendimento de mensagens: SO as partes puras de braniac/atender.py. As travas da resposta a terceiros (validar_resposta,
//! sensivel, tem_dado_real), o que e importante avisar, pedido sobre o que nos entregamos, apresentacao, grupo, idade da
//! mensagem, quem esta esperando resposta e as flags do protocolo do canal.
//! Ficou de fora: o laco atender() e coletar() (leem e enviam mensagem), _enviar_resposta, decidir, _resposta_familia (IA),
//! tarefa_entrega (caminhos e chamadas do PC do dono) e _trato_familia (nomes de pessoas). O historico da conversa
//! (direcao "C" = nos, "E" = a pessoa; texto) entra por parametro: quem chama le do banco.
#![allow(dead_code)]
use crate::contexto::{self, re, Ctx};
use crate::tempo::{Agora, Data};
use regex::Regex;

pub const PRIMEIRO_CONTATO: &str = "Como posso te ajudar?";
/// Resposta a terceiros em audio (~45 s, ja com a apresentacao); maior que isso vai por escrito.
pub const VOZ_ATENDER_MAX: usize = 650;
pub const ANOTADO_ENTREGA: &str = "Pode deixar. O que for de ajuste ou de texto sobre o material que te mandei eu preparo direito e te envio por escrito.";

// links, chaves Pix, telefones, contas
re!(perigoso, r"(?i)https?://|www\.|\.com\b|\.br\b|\d{6,}");
// sites OFICIAIS (governo/BC/Serasa) podem ser citados
re!(oficial, r"(?i)(?:https?://)?(?:www\.)?(?:registrato\.bcb\.gov\.br|valoresareceber\.bcb\.gov\.br|consumidor\.gov\.br|bcb\.gov\.br|gov\.br|serasa\.com\.br)/?");
// nunca usar o dono como desculpa; sempre explicar quem e o Jarvis
re!(ocupado, r"(?i)ocupad[oa]|n[aã]o\s+est[aá]\s+dispon[ií]vel|indispon[ií]vel|(?:est[aá]|t[aá])\s+off|sem\s+tempo|n[aã]o\s+pode\s+atender");
// Conversa com dado sensivel NUNCA vai para a camada gratuita online (router.SENSIVEL do Python).
re!(sensivel_re, concat!(
    r"(?i)\bpix\b|cart[aã]o|senha|\bcvv\b|\bcpf\b|\bcnpj\b|ag[eê]ncia|conta\s+(banc|corrente)|",
    r"c[oó]digo\s+de\s+(verifica|seguran)|token|\b\d{4}[ .-]?\d{4}[ .-]?\d{4}[ .-]?\d{4}\b|",
    r"\b\d{3}\.?\d{3}\.?\d{3}-?\d{2}\b|passaporte|\brg\b"));
// DADO REAL (nao o assunto): numero de cartao, CPF, senha/codigo dito com valor, agencia/conta com numero.
re!(dado_real, concat!(
    r"(?i)\b\d{4}[ .-]?\d{4}[ .-]?\d{4}[ .-]?\d{4}\b|\b\d{3}\.?\d{3}\.?\d{3}-?\d{2}\b|",
    r"\b(?:senha|cvv|token|c[oó]digo\s+de\s+(?:verifica\w+|seguran\w+))\b\s*(?:[ée]|:|=)\s*\S{3,}|ag[eê]ncia\s*\d|conta\s*(?:corrente\s*)?\d"));
re!(pede_dado, concat!(
    r"(?i)\b(?:me\s+)?(?:mande|manda|envie|envia|passe|passa|informe|informa|digite|digita)\b[^.!?\n]{0,30}",
    r"\b(?:cart[aã]o|senha|cvv|cpf|c[oó]digo|token|dados\s+banc\w+)\b"));
re!(saudacao, r"(?i)\b(?:oi+|ol[aá]|opa|e\s*a[ií]|eai|bom\s+dia|boa\s+tarde|boa\s+noite|tudo\s+(?:bem|bom|certo|joia)|td\s+bem|blz|beleza|salve|fala|hey|hello)\b");
// "tira uma duvida" nao e pedido
re!(verbo_entrega, concat!(
    r"(?i)\b(?:escrev\w+|escrit[oa]|mud[ae]\w*|alter\w+|ajust\w+|troc\w+|troqu\w+|corrig\w+|corrij\w+|refa[zç]\w*|edit[ae]\w*|coloc\w+|coloqu\w+|",
    r"acrescent\w+|adicion\w+|tir[ae]\s+(?:as?|os?|ess[ae]|aquel[ae])\b|com\s+base\s+n[oa])"));
re!(coisa_entrega, r"(?i)\b(?:v[ií]deos?|legendas?|descri[cç][aã]o|textos?|artes?|imagem|imagens|fotos?|m[uú]sica|narra[cç][aã]o|voz|logo|vers[aã]o|reels?|post|material|arquivo|capa)\b");
re!(autor_re, r"\]\s*(.*?):\s*$");
re!(carimbo, r"\[(\d{1,2}):(\d{2}),\s*(\d{1,2})/(\d{1,2})/(\d{4})\]");
re!(hora_re, r"^(\d{1,2}):(\d{2})$");
re!(cabecalho_1a1, r"visto|online|digitando|[úu]ltim");

/// Uma conversa com mensagens esperando decisao (o "it" do Python).
#[derive(Default, Clone, Debug)]
pub struct Item {
    pub nome: String,
    pub msgs: Vec<String>,
    /// Conversa recente: "C: ..." = nosso lado, "E: ..." = o lado deles.
    pub ctx: Vec<String>,
    /// Autores distintos das mensagens novas (mais de um = grupo).
    pub autores: Vec<String>,
    pub midia: bool,
    pub grupo: bool,
    /// Do outro lado e uma IA em modo GL1.
    pub gl_ativo: bool,
}

/// Mensagem da tela: d = "ELE" ou "EU", t = texto, c = cabecalho "[17:13, 01/10/2026] Nome: ".
#[derive(Default, Clone, Debug, PartialEq)]
pub struct Msg {
    pub d: String,
    pub t: String,
    pub c: String,
}

/// Linha da lista de conversas: resumo = "Nome | 17:47 | texto"; nossa = a ultima mensagem e nossa.
#[derive(Default, Clone, Debug)]
pub struct Linha {
    pub nome: String,
    pub resumo: String,
    pub nossa: bool,
}

/// O assunto e sensivel (pix, cartao, senha, CPF...): a conversa so pode ir a IA local.
pub fn sensivel(texto: &str) -> bool {
    sensivel_re().is_match(texto)
}

/// O texto TRAZ o dado (cartao/CPF/senha com valor) ou PEDE esses dados. Falar de pagamento/cartao/Pix e permitido.
pub fn tem_dado_real(texto: &str) -> bool {
    dado_real().is_match(texto) || pede_dado().is_match(texto)
}

/// Trava da resposta a terceiros: barra o que poderia causar dano se a decisao estivesse errada ou manipulada.
/// `links_proprios` = os unicos enderecos do dono que podem ser citados (pessoal.json "links_proprios"), exatos.
pub fn validar_resposta(texto: &str, links_proprios: &[String]) -> Result<(), &'static str> {
    if texto.trim().is_empty() {
        return Err("vazia");
    }
    if texto.chars().count() > 500 {
        return Err("longa demais");
    }
    let mut limpo = oficial().replace_all(texto, "").into_owned();
    let proprios: Vec<String> = links_proprios.iter().map(|l| l.trim().trim_start_matches("https://").trim_start_matches("http://")).filter(|l| !l.is_empty()).map(regex::escape).collect();
    if !proprios.is_empty() {
        if let Ok(r) = Regex::new(&format!("(?i)(?:https?://)?(?:{})", proprios.join("|"))) {
            limpo = r.replace_all(&limpo, "").into_owned();
        }
    }
    if perigoso().is_match(&limpo) {
        return Err("contem link/numero");
    }
    if ocupado().is_match(texto) {
        return Err("diz que o dono esta ocupado/indisponivel (nao pode; explique quem e o Jarvis)");
    }
    if tem_dado_real(texto) {
        return Err("contem dado sensivel (cartao/CPF/senha) ou pede esses dados");
    }
    Ok(())
}

/// Primeira mensagem de contato novo que o dono precisa ver: urgencia, pedido para falar/ligar com ele, proposta, contrato,
/// problema, audio (🎙). Duvida de rotina (preco, vaga, horario) o Jarvis responde sozinho: nao vira aviso.
pub fn importante(entrada: &str, dono: &str) -> bool {
    let t = entrada.trim();
    if t.is_empty() {
        return false;
    }
    let d = contexto::dono_re(dono);
    let urgente = Regex::new(&format!(
        r"(?i)urgente|urg[eê]ncia|preciso\s+falar|me\s+liga|ligar\s+(?:pra|para)|liga\s+(?:pra|para)|falar\s+com\s+(?:[oa]\s+)?{d}|chama\s+[oa]\s+{d}|proposta|entrevista|reuni[aã]o|contrato|cobran[cç]a|problema|reclama|socorro|emerg[eê]ncia"));
    urgente.is_ok_and(|r| r.is_match(t)) || t.contains('🎙')
}

pub fn eh_saudacao(texto: &str) -> bool {
    saudacao().is_match(texto)
}

/// Nome do ultimo arquivo que NOS mandamos a essa pessoa ("" se nao ha). `historico` = ultimas ~40 mensagens, da mais antiga a mais nova.
pub fn entrega_a(historico: &[(String, String)]) -> String {
    historico.iter().rev().find(|(d, t)| d == "C" && t.starts_with("[arquivo ")).map(|(_, t)| t["[arquivo ".len()..].split(']').next().unwrap_or("").to_string()).unwrap_or_default()
}

/// A mensagem de agora pede para escrever ou mudar algo, e a coisa (video, legenda, foto...) esta nela ou nas ultimas falas
/// da pessoa. `historico` = as ultimas ~6 mensagens da conversa.
pub fn pedido_sobre_entrega(entrada: &str, historico: &[(String, String)]) -> bool {
    let falas: Vec<&str> = historico.iter().filter(|(d, _)| d == "E").map(|(_, t)| t.as_str()).collect();
    verbo_entrega().is_match(entrada) && coisa_entrega().is_match(&format!("{entrada} {}", falas.join(" ")))
}

pub fn nao_responder_ou_grupo(ctx: &Ctx, nome: &str, flags: &str) -> bool {
    ctx.nao_responder(nome) || flags.contains('G')
}

/// 1o contato: se apresenta como Jarvis (quem tem abertura propria em "intros" recebe a dela). Quem ja conhece o Jarvis nao
/// ouve a apresentacao de novo (nem a que a IA escreveu no proprio texto), salvo se perguntar quem ele e.
pub fn com_apresentacao(ctx: &Ctx, chat: &str, texto: &str, pergunta: &str, historico: &[(String, String)]) -> String {
    let q = contexto::quem_sou(&ctx.dono);
    if ctx.apresentado(chat) || historico.iter().any(|(d, t)| d == "C" && q.is_match(t)) {
        return ctx.sem_reapresentacao(texto, pergunta);
    }
    if texto.to_lowercase().contains("jarvis") {
        return texto.to_string();
    }
    ctx.abertura_padrao(chat) + texto
}

/// Quem escreveu, pelo cabecalho "[17:13, 01/10/2026] Nome: ".
pub fn autor(cabecalho: &str) -> String {
    autor_re().captures(cabecalho).and_then(|c| c.get(1)).map(|m| m.as_str().trim().to_string()).unwrap_or_default()
}

/// Mensagens DELES no fim da conversa, depois da ultima nossa (as "sem resposta").
pub fn finais_deles(msgs: &[Msg]) -> Vec<Msg> {
    let (mut fim, mut pulando) = (vec![], true);
    for m in msgs.iter().rev() {
        if pulando && m.d != "ELE" && contexto::saudacao_automatica(&m.t) {
            continue; // saudacao automatica (vem como "nossa"): nao conta como resposta
        }
        pulando = false;
        if m.d != "ELE" {
            break;
        }
        fim.push(m.clone());
    }
    fim.reverse();
    fim
}

fn segundos(a: &Agora) -> i64 {
    a.data.0 * 86400 + a.segundos()
}

/// Idade (em horas) de uma mensagem pelo cabecalho "[17:13, 01/10/2026] Nome: ". None se nao der para ler.
pub fn idade_horas(cabecalho: &str, agora: &Agora) -> Option<f64> {
    let c = carimbo().captures(cabecalho)?;
    let n = |i: usize| c.get(i).and_then(|m| m.as_str().parse::<i64>().ok());
    let (h, mi, d, mo, a) = (n(1)?, n(2)?, n(3)?, n(4)?, n(5)?);
    if h > 23 || mi > 59 {
        return None;
    }
    let quando = Data::civil(a, mo, d)?.0 * 86400 + h * 3600 + mi * 60;
    Some((segundos(agora) - quando) as f64 / 3600.0)
}

/// Idade (horas) da ultima mensagem pela LISTA ("Nome | 17:47 | texto"): so entende horario de hoje (HH:MM).
/// "Ontem", dia da semana ou data => None (velho demais para responder de repente).
pub fn idade_linha(resumo: &str, agora: &Agora) -> Option<f64> {
    for parte in resumo.split(" | ").skip(1).take(2) {
        let Some(c) = hora_re().captures(parte.trim()) else { continue };
        let (h, mi) = (c[1].parse::<i64>().ok()?, c[2].parse::<i64>().ok()?);
        if h > 23 || mi > 59 {
            return None;
        }
        let horas = (agora.segundos() - h * 3600 - mi * 60) as f64 / 3600.0;
        return Some(if horas >= 0.0 { horas } else { horas + 24.0 }); // horario "no futuro" = ontem
    }
    None
}

/// Conversas cuja ULTIMA mensagem e DELES (lida ou nao) e recente: as que estao esperando resposta.
pub fn esperando_resposta(ctx: &Ctx, linhas: &[Linha], agora: &Agora, max_idade_h: f64) -> Vec<String> {
    let dono = ctx.chat_do_dono();
    linhas.iter()
        .filter(|c| !(c.nossa && !contexto::saudacao_automatica(&c.resumo)) && Some(c.nome.as_str()) != dono && !ctx.nao_responder(&c.nome))
        .filter(|c| idade_linha(&c.resumo, agora).is_some_and(|h| h <= max_idade_h))
        .map(|c| c.nome.clone()).collect()
}

/// Cabecalho do chat aberto: grupo mostra os participantes ("A, B, Você") ou "dados do grupo"; 1:1 mostra "visto por último"/"online".
pub fn eh_grupo(cabecalho: &str) -> bool {
    let h = cabecalho.to_lowercase();
    if h.contains("dados do grupo") || h.contains("group info") {
        return true;
    }
    h.matches(',').count() >= 2 && !cabecalho_1a1().is_match(&h)
}

/// Flags do protocolo do canal: G grupo, S sensivel, A audio/midia, N 1o contato, M IA em GL1; "-" = nada.
pub fn flags_de(it: &Item, apresentado: bool) -> String {
    let mut autores: Vec<&String> = it.autores.iter().collect();
    autores.sort();
    autores.dedup();
    let mut f = String::new();
    if autores.len() > 1 || it.grupo {
        f.push('G');
    }
    if sensivel(&it.msgs.join(" ")) {
        f.push('S');
    }
    if it.midia {
        f.push('A');
    }
    if !apresentado {
        f.push('N');
    }
    if it.gl_ativo {
        f.push('M');
    }
    if f.is_empty() { "-".into() } else { f }
}

fn corte(t: &str, n: usize) -> String {
    t.chars().take(n).collect()
}

/// Completa o contexto da conversa com o que o banco local lembra (mais antigo que as ultimas mensagens da tela).
/// `historico` = ultimas ~14 mensagens do banco, da mais antiga a mais nova.
pub fn enriquecer_ctx(it: &mut Item, historico: &[(String, String)]) {
    let visto = it.ctx.join(" ");
    let ate = historico.len().saturating_sub(2);
    let antigas: Vec<String> = historico[..ate].iter().filter(|(_, t)| !visto.contains(&corte(t, 40)))
        .map(|(d, t)| format!("{}{}", if d == "E" { "E: " } else { "C: " }, corte(t, 100))).collect();
    let mut novo: Vec<String> = antigas[antigas.len().saturating_sub(6)..].to_vec();
    novo.append(&mut it.ctx);
    it.ctx = novo.split_off(novo.len().saturating_sub(10));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agora(h: u32, m: u32) -> Agora {
        Agora { data: Data::civil(2026, 10, 1).unwrap(), hora: h, min: m, seg: 0 }
    }

    fn ctx() -> Ctx {
        let mut c = Ctx { dono: "Davi".into(), nao_responder: vec!["cobrabot".into()], ..Default::default() };
        c.contatos.insert("davi".into(), "Meu Zap".into());
        c
    }

    fn h(v: &[(&str, &str)]) -> Vec<(String, String)> {
        v.iter().map(|(d, t)| (d.to_string(), t.to_string())).collect()
    }

    #[test]
    fn trava_da_resposta_barra_link_numero_ocupado_e_dado() {
        let proprios = vec!["https://codigo.exemplo.org/davi/PROJETO".to_string(), "fotos-do-davi.exemplo.app".to_string()];
        let v = |t: &str| validar_resposta(t, &proprios);
        assert_eq!(v("Claro, pode vir amanhã às 10h."), Ok(()));
        assert_eq!((v(""), v("   "), v(&"a".repeat(501))), (Err("vazia"), Err("vazia"), Err("longa demais")));
        assert_eq!(v(&"á".repeat(500)), Ok(())); // o limite e em letras, nao em bytes
        // sites oficiais podem ser citados; link qualquer e numero longo nao
        assert_eq!(v("Entra no registrato.bcb.gov.br com o gov.br dele"), Ok(()));
        for ruim in ["Acessa https://golpe.com/x", "Manda pro 11999998888", "veja www.qualquer.net", "site loja.com", "pagina.br agora", "chave 123456"] {
            assert_eq!(v(ruim), Err("contem link/numero"), "{ruim}");
        }
        // so os links do dono que estao na lista, exatos
        assert_eq!(v("Aqui: https://codigo.exemplo.org/davi/PROJETO"), Ok(()));
        assert_eq!(v("Veja fotos-do-davi.exemplo.app"), Ok(()));
        assert!(v("Aqui: https://codigo.exemplo.org/davi/outro-repo").is_err());
        assert!(validar_resposta("Aqui: https://codigo.exemplo.org/davi/PROJETO", &[]).is_err());
        // nunca usa o dono como desculpa
        for frase in ["Ele está ocupado agora", "O Davi não está disponível", "ele tá off", "ele está indisponível", "Davi está sem tempo", "Ele não pode atender agora"] {
            assert!(v(&format!("{frase}, mas anoto seu recado.")).is_err_and(|m| m.contains("ocupado")), "{frase}");
        }
        // dado real ou pedido de dado: barrado; falar DO assunto pode
        for ruim in ["sua senha é abc123", "CPF 123.456.789-09", "me manda o número do cartão", "passa a senha por favor", "agência 1 e conta 2", "token: xyz9"] {
            assert!(v(ruim).is_err(), "{ruim}");
        }
        assert_eq!(v("me passa a senha do wifi?").err(), Some("contem dado sensivel (cartao/CPF/senha) ou pede esses dados"));
        for bom in ["Pode pagar no Pix ou no cartão, como preferir.", "Nunca peço senha a ninguém."] {
            assert_eq!(v(bom), Ok(()), "{bom}");
        }
    }

    #[test]
    fn sensivel_e_dado_real_sao_coisas_diferentes() {
        for t in ["minha senha", "faz um pix", "meu CPF", "cartão de crédito", "conta corrente", "código de verificação", "1234 5678 9012 3456", "RG e passaporte", "token"] {
            assert!(sensivel(t), "{t}");
        }
        assert!(!sensivel("bom dia, tem vaga pra sexta?") && !sensivel("vou pagar amanhã"));
        assert!(tem_dado_real("1234-5678-9012-3456") && tem_dado_real("12345678909") && tem_dado_real("a senha = abcd") && tem_dado_real("envia o cvv"));
        assert!(!tem_dado_real("aceita cartão?") && !tem_dado_real("esqueci minha senha") && !tem_dado_real(""));
    }

    #[test]
    fn importante_so_o_que_o_dono_precisa_ver() {
        assert!(importante("é urgente, me liga", "Davi") && importante("tenho uma proposta de trabalho", "Davi") && importante("🎙 oi, preciso falar com voce", "Davi"));
        assert!(importante("quero falar com o Davi", "Davi") && importante("chama o davi aí", "Davi"));
        assert!(!importante("oi", "Davi") && !importante("bom dia, tem vaga pra sexta?", "Davi") && !importante("quanto custa um ensaio?", "Davi") && !importante("  ", "Davi"));
        assert!(importante("socorro", "") && !importante("quero falar com o", ""));
    }

    #[test]
    fn pedido_sobre_o_que_nos_entregamos() {
        let hist = h(&[("E", "oi"), ("C", "[arquivo Predio-Novo.mp4] Vídeo de chamada"), ("E", "É com base no vídeo")]);
        assert_eq!((entrega_a(&hist), entrega_a(&h(&[("E", "[arquivo x.pdf] meu")])), entrega_a(&[])), ("Predio-Novo.mp4".to_string(), String::new(), String::new()));
        assert!(pedido_sobre_entrega("Escreva uma descrição pra eu utilizar como legenda no reels", &[]));
        assert!(pedido_sobre_entrega("Faça escrito", &hist)); // a coisa esta na fala anterior DELE
        assert!(!pedido_sobre_entrega("Faça escrito", &h(&[("C", "segue o vídeo")]))); // fala nossa nao conta
        assert!(pedido_sobre_entrega("Troca a foto do final do vídeo", &[]) && pedido_sobre_entrega("tira essa música", &[]));
        assert!(!pedido_sobre_entrega("Tira uma dúvida: quanto custa um site?", &hist) && !pedido_sobre_entrega("Qual o horário de vocês amanhã?", &hist));
    }

    #[test]
    fn apresentacao_so_para_quem_ainda_nao_conhece() {
        let mut c = ctx();
        let ja = h(&[("C", "Oi! Aqui é o Jarvis, assistente do Davi. Como posso ajudar?")]);
        assert_eq!(com_apresentacao(&c, "+55 66 9999-0000", "Sou o Jarvis, assistente de IA do Davi. Obrigado!", "", &ja), "Obrigado!");
        let novo = com_apresentacao(&c, "Contato Novo", "Posso ajudar.", "", &[]);
        assert!(novo.starts_with("Oi, Contato! Eu sou o Jarvis") && novo.ends_with("Posso ajudar.") && !ocupado().is_match(&novo));
        assert_eq!(com_apresentacao(&c, "Contato Novo", "Aqui é o Jarvis, tudo bem?", "", &[]), "Aqui é o Jarvis, tudo bem?"); // a IA ja se apresentou
        c.apresentados.push("contato novo".into());
        assert_eq!(com_apresentacao(&c, "Contato Novo", "Posso ajudar.", "", &[]), "Posso ajudar.");
        assert!(nao_responder_ou_grupo(&c, "CobraBot", "-") && nao_responder_ou_grupo(&c, "Rui", "GN") && !nao_responder_ou_grupo(&c, "Rui", "N"));
    }

    #[test]
    fn idade_pela_lista_e_pelo_cabecalho() {
        let a = agora(18, 0);
        let perto = |x: Option<f64>, y: f64| x.is_some_and(|v| (v - y).abs() < 0.05);
        assert!(perto(idade_linha("Rui | 17:47 | oi", &a), 13.0 / 60.0));
        assert_eq!((idade_linha("Rui | Ontem | oi", &a), idade_linha("Rui | terça-feira | oi", &a), idade_linha("Rui | 25:99 | oi", &a)), (None, None, None));
        assert!(perto(idade_linha("+55 86 9000-0003 | +55 86 9000-0003 | 16:04 | oi", &a), 1.93));
        assert!(perto(idade_linha("X | 23:30 | oi", &a), 18.5)); // horario "no futuro" = ontem
        let a = agora(19, 0);
        assert!(perto(idade_horas("[17:00, 01/10/2026] Maria: ", &a), 2.0));
        assert!(idade_horas("[10:00, 28/09/2026] Maria: ", &a).is_some_and(|v| v > 70.0));
        assert_eq!((idade_horas("sem data", &a), idade_horas("[10:00, 31/02/2026] Maria: ", &a)), (None, None));
    }

    #[test]
    fn esperando_resposta_pega_so_o_que_esta_esperando() {
        let l = |nome: &str, hora: &str, texto: &str, nossa: bool| Linha { nome: nome.into(), resumo: format!("{nome} | {hora} | {texto}"), nossa };
        let linhas = [l("Rui", "18:05", "oi", false), l("Mamãe", "18:09", "oi", true), l("Meu Zap", "18:00", "oi", false), l("CobraBot", "08:06", "oi", false), l("Ana", "Ontem", "oi", false),
                      l("Novo Contato", "18:08", "A Empresa agradece seu contato.", true), l("Eva", "03:00", "oi", false)];
        assert_eq!(esperando_resposta(&ctx(), &linhas, &agora(18, 10), 12.0), ["Rui", "Novo Contato"]);
    }

    #[test]
    fn grupo_autor_finais_e_flags() {
        assert!(eh_grupo("Ana, Bia, Você") && eh_grupo("Vagas dados do grupo"));
        assert!(!eh_grupo("Maria visto por último hoje às 10:00") && !eh_grupo("online") && !eh_grupo("Ana, Bia, visto por último"));
        assert_eq!((autor("[17:13, 01/10/2026] Ana Souza: "), autor("sem cabecalho")), ("Ana Souza".to_string(), String::new()));
        let m = |d: &str, t: &str| Msg { d: d.into(), t: t.into(), c: String::new() };
        let msgs = [m("ELE", "antiga"), m("EU", "resposta"), m("ELE", "oi"), m("ELE", "tem vaga?"), m("EU", "A Empresa agradece seu contato.")];
        assert_eq!(finais_deles(&msgs), [m("ELE", "oi"), m("ELE", "tem vaga?")]);
        assert!(finais_deles(&msgs[..2]).is_empty());
        let item = |msgs: &[&str], autores: &[&str], midia: bool| Item { nome: "N".into(), msgs: msgs.iter().map(|s| s.to_string()).collect(), autores: autores.iter().map(|s| s.to_string()).collect(), midia, ..Default::default() };
        assert_eq!(flags_de(&item(&["oi"], &["N"], false), false), "N");
        assert_eq!(flags_de(&item(&["oi"], &["N", "N"], false), true), "-");
        assert!(flags_de(&item(&["a", "b"], &["x", "y"], false), true).contains('G'));
        assert!(flags_de(&item(&["minha senha"], &["N"], false), true).contains('S'));
        assert_eq!(flags_de(&Item { grupo: true, gl_ativo: true, ..item(&["[AUDIO]"], &["N"], true) }, false), "GANM");
    }

    #[test]
    fn contexto_ganha_o_que_o_banco_lembra_sem_repetir_a_tela() {
        let mut it = Item { ctx: vec!["E: tem vaga sexta?".into(), "C: tem sim".into()], ..Default::default() };
        let hist = h(&[("E", "sou a Ana do curso"), ("C", "Oi, Ana!"), ("E", "tem vaga sexta?"), ("E", "penultima"), ("C", "ultima")]);
        enriquecer_ctx(&mut it, &hist);
        assert_eq!(it.ctx, ["E: sou a Ana do curso", "C: Oi, Ana!", "E: tem vaga sexta?", "C: tem sim"]);
    }
}
