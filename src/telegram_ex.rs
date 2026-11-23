//! Conversa cuidada pelo Jarvis com UMA pessoa no Telegram Web, sobre UM assunto (porte das regras de braniac/telegram_ex.py).
//! Responde so com os fatos anotados em dados/telegram_ex_fatos.md; sem fato, assunto de dinheiro/senha/documento ou resposta
//! duvidosa vira "vou confirmar" + aviso ao dono; agradecimento nao pede confirmacao; recado de terceiro so e repassado se for
//! do assunto. O texto dela so pode passar por IA LOCAL (closure `gerar`).
//! Diferenca do Python: nome dela, do dono, de quem traz as novidades e o assunto nao ficam no codigo: vem em `Quem`
//! (quem integra le da configuracao). Ficou de fora: abrir a aba, ler as bolhas e enviar (closures).
#![allow(dead_code)]
use crate::contexto::re;
use crate::tempo::Data;
use regex::Regex;
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;

re!(agradece, r"(?i)obrigad|obg|valeu|me avisa|me avise|combinado|ok\b|beleza|tendo mais");
re!(seguranca, r"(?i)\b(?:senha|cpf|cart[aã]o|pix|conta\s+banc|c[oó]digo)\b");

pub struct Quem {
    /// primeiro nome da pessoa (saudacao e conferencia do titulo da conversa)
    pub ela: String,
    pub dono: String,
    /// quem costuma trazer as novidades do assunto
    pub informante: String,
    /// nome do assunto, como aparece nas frases ("o processo", "a obra")
    pub assunto: String,
    /// o que conta como "e do assunto" num recado de terceiro
    pub do_assunto: Regex,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Msg {
    pub mid: i64,
    /// enviada por nos
    pub out: bool,
    pub t: String,
}

fn corta(t: &str, n: usize) -> String {
    t.chars().take(n).collect()
}

pub fn fatos(dados: &Path) -> String {
    std::fs::read_to_string(dados.join("telegram_ex_fatos.md")).map(|t| t.trim().to_string()).unwrap_or_default()
}

pub fn anotar_fato(dados: &Path, texto: &str, hoje: Data) -> Result<(), String> {
    let (a, m, d) = hoje.partes();
    std::fs::create_dir_all(dados).and_then(|_| std::fs::OpenOptions::new().create(true).append(true).open(dados.join("telegram_ex_fatos.md")))
        .and_then(|mut f| writeln!(f, "- {d:02}/{m:02}/{a}: {}", texto.trim())).map_err(|e| e.to_string())
}

/// So se le e responde quando o titulo da conversa aberta e mesmo o dela.
pub fn titulo_confere(titulo: &str, q: &Quem) -> bool {
    !q.ela.trim().is_empty() && titulo.contains(q.ela.trim())
}

/// (texto, precisa_do_dono). So com os fatos anotados; sem fato ou assunto delicado => "vou confirmar".
/// `gerar` = IA LOCAL (o que ela escreveu nunca vai a servico online).
pub fn resposta(pergunta: &str, fatos: &str, q: &Quem, gerar: &dyn Fn(&str) -> Option<String>) -> (String, bool) {
    let confirma = (format!("Oi, {}! Aqui é o Jarvis, assistente do {}. Vou confirmar isso com ele e te retorno por aqui.", q.ela, q.dono), true);
    let delicado = seguranca().is_match(pergunta);
    // agradecimento ou pedido de aviso nao e pergunta: nunca "vou confirmar"
    if !delicado && !pergunta.contains('?') && agradece().is_match(pergunta) {
        return (format!("Combinado, {}! Qualquer novidade sobre {} eu te aviso por aqui. Obrigado!", q.ela, q.assunto), false);
    }
    if delicado || fatos.trim().is_empty() {
        return confirma;
    }
    let p = format!("Você é o Jarvis, assistente de IA do {dono}, conversando por Telegram com {ela}. Responda em português do Brasil, curto, gentil e direto (1 a 3 frases). \
Use SOMENTE os fatos abaixo; se a resposta não estiver neles, responda exatamente: CONFIRMAR. Não invente datas, valores ou prazos. \
Não fale de dinheiro nem de assuntos pessoais do {dono}.\nFATOS:\n{fatos}\n\nMENSAGEM DELA:\n{pergunta}", dono = q.dono, ela = q.ela);
    let r = gerar(&p).unwrap_or_default().split_whitespace().collect::<Vec<_>>().join(" ");
    if r.is_empty() || r.to_uppercase().contains("CONFIRMAR") || r.chars().count() > 600 { confirma } else { (r, false) }
}

fn estado(dados: &Path) -> Value {
    std::fs::read_to_string(dados.join("telegram_ex.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}))
}

fn salvar_visto(dados: &Path, visto: i64) {
    let mut e = estado(dados);
    e["visto"] = json!(visto);
    let _ = std::fs::create_dir_all(dados).and_then(|_| std::fs::write(dados.join("telegram_ex.json"), e.to_string()));
}

pub struct Portas<'a> {
    /// mensagens da conversa DELA (quem le confere o titulo com `titulo_confere`; vazio se nao for ela)
    pub msgs: &'a dyn Fn() -> Vec<Msg>,
    pub enviar: &'a dyn Fn(&str) -> Result<(), String>,
    /// IA LOCAL
    pub gerar: &'a dyn Fn(&str) -> Option<String>,
    pub avisar_dono: &'a dyn Fn(&str),
    /// registro local da conversa
    pub log: &'a dyn Fn(&str),
}

/// Le a conversa e responde o que ela escreveu desde a ultima vez. Devolve quantas respostas enviou (0 ou 1).
/// Na 1a vez o que ja existe conta como visto: nunca responde o passado.
pub fn tick(dados: &Path, q: &Quem, p: &Portas) -> usize {
    let ms: Vec<Msg> = (p.msgs)().into_iter().filter(|m| m.mid != 0).collect();
    let Some(topo) = ms.iter().map(|m| m.mid).max() else { return 0 };
    let Some(visto) = estado(dados)["visto"].as_i64() else {
        salvar_visto(dados, topo);
        return 0;
    };
    let novas: Vec<&Msg> = ms.iter().filter(|m| !m.out && m.mid > visto && !m.t.is_empty()).collect();
    salvar_visto(dados, visto.max(topo));
    if novas.is_empty() {
        return 0;
    }
    let pergunta = corta(&novas.iter().map(|m| m.t.as_str()).collect::<Vec<_>>().join(" "), 800);
    let (texto, dono) = resposta(&pergunta, &fatos(dados), q, p.gerar);
    if let Err(e) = (p.enviar)(&texto) {
        (p.log)(&format!("envio falhou: {e}"));
        return 0;
    }
    // rele: o id real da nossa resposta (somar um numero fixo engolia mensagens dela)
    let novo = (p.msgs)().iter().map(|m| m.mid).chain([topo, visto]).max().unwrap_or(topo);
    salvar_visto(dados, novo);
    (p.log)(&format!("ela: {} | eu: {}", corta(&pergunta, 150), corta(&texto, 150)));
    if dono {
        (p.avisar_dono)(&format!("A {} escreveu no Telegram: «{}». Respondi que ia confirmar com você.", q.ela, corta(&pergunta, 200)));
    }
    1
}

/// Alguem escreveu ao dono: SO o que for do assunto (e sem dado sensivel) vira fato anotado e recado para ela. True se avisou.
pub fn atualizacao(dados: &Path, q: &Quem, texto: &str, hoje: Data, p: &Portas) -> bool {
    if !q.do_assunto.is_match(texto) || seguranca().is_match(texto) {
        return false;
    }
    let _ = anotar_fato(dados, &format!("{} avisou: «{}»", q.informante, corta(texto.trim(), 300)), hoje);
    let msg = format!("Oi, {}! Aqui é o Jarvis, assistente do {}. Teve atualização sobre {}: {} avisou «{}». Se houver mais novidades, eu aviso.",
        q.ela, q.dono, q.assunto, q.informante, corta(texto.trim(), 250));
    match (p.enviar)(&msg) {
        Ok(()) => {
            (p.log)(&format!("atualizacao enviada: {}", corta(texto, 150)));
            true
        }
        Err(e) => {
            (p.log)(&format!("atualizacao falhou: {e}"));
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::path::PathBuf;

    fn quem() -> Quem {
        Quem { ela: "Lia".into(), dono: "Davi".into(), informante: "o contador".into(), assunto: "o inventário".into(), do_assunto: Regex::new(r"(?i)invent[aá]rio|cart[oó]rio|document").unwrap() }
    }

    fn pasta(n: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-telegram_ex-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn resposta_so_com_fatos() {
        let (d, q) = (pasta("fatos"), quem());
        assert!(resposta("algum retorno?", &fatos(&d), &q, &|_| Some("Invento.".into())).1); // sem fatos: confirma com o dono
        anotar_fato(&d, "entrega no cartório hoje", Data::civil(2026, 11, 5).unwrap()).unwrap();
        assert_eq!(fatos(&d), "- 05/11/2026: entrega no cartório hoje");
        let f = fatos(&d);
        let visto = RefCell::new(String::new());
        let r = resposta("algum retorno?", &f, &q, &|p| { *visto.borrow_mut() = p.to_string(); Some("Ele entrega  no cartório hoje.".into()) });
        assert_eq!(r, ("Ele entrega no cartório hoje.".to_string(), false));
        assert!(visto.borrow().contains("FATOS:\n- 05/11/2026") && visto.borrow().contains("MENSAGEM DELA:\nalgum retorno?"));
        assert!(resposta("quanto falta pagar?", &f, &q, &|_| Some("CONFIRMAR".into())).1);
        assert!(resposta("e aí?", &f, &q, &|_| None).1 && resposta("e aí?", &f, &q, &|_| Some("x".repeat(601))).1);
        // dado sensivel nunca sai: nem pergunta a IA
        let r = resposta("qual o cpf dele?", &f, &q, &|_| panic!("nao gera"));
        assert!(r.1 && r.0.starts_with("Oi, Lia! Aqui é o Jarvis, assistente do Davi."));
        // agradecimento nao pede confirmacao (mesmo sem fatos), mas com pergunta ou dado sensivel pede
        assert_eq!(resposta("Tendo mais informações me avisa, obrigada!", "", &q, &|_| panic!("nao gera")).1, false);
        assert!(resposta("obrigada, e o pix?", &f, &q, &|_| Some("x".into())).1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn tick_nao_responde_o_passado_e_avisa_o_dono_quando_confirma() {
        let (d, q) = (pasta("tick"), quem());
        let conversa = RefCell::new(vec![Msg { mid: 10, out: false, t: "antiga dela".into() }, Msg { mid: 11, out: true, t: "nossa".into() }]);
        let (enviados, avisos) = (RefCell::new(Vec::<String>::new()), RefCell::new(Vec::<String>::new()));
        let msgs = || conversa.borrow().clone();
        let enviar = |t: &str| { enviados.borrow_mut().push(t.to_string()); let n = conversa.borrow().iter().map(|m| m.mid).max().unwrap_or(0) + 1; conversa.borrow_mut().push(Msg { mid: n, out: true, t: t.into() }); Ok(()) };
        let (gerar, avisar, log) = (|_: &str| Some("O contador entrega amanhã.".to_string()), |t: &str| avisos.borrow_mut().push(t.to_string()), |_: &str| ());
        let p = Portas { msgs: &msgs, enviar: &enviar, gerar: &gerar, avisar_dono: &avisar, log: &log };
        assert_eq!(tick(&d, &q, &p), 0); // 1a vez: tudo conta como visto
        assert_eq!(tick(&d, &q, &p), 0);
        conversa.borrow_mut().push(Msg { mid: 12, out: false, t: "tem novidade?".into() });
        assert_eq!(tick(&d, &q, &p), 1); // sem fatos: confirma e avisa o dono
        assert!(enviados.borrow()[0].contains("Vou confirmar") && avisos.borrow()[0].contains("«tem novidade?»"));
        assert_eq!(tick(&d, &q, &p), 0); // a nossa resposta nao vira pergunta, e a dela nao e respondida 2x
        anotar_fato(&d, "o contador entrega amanhã", Data::civil(2026, 11, 5).unwrap()).unwrap();
        conversa.borrow_mut().push(Msg { mid: 20, out: false, t: "e agora?".into() });
        conversa.borrow_mut().push(Msg { mid: 21, out: false, t: "alguma data?".into() });
        assert_eq!(tick(&d, &q, &p), 1);
        assert_eq!((enviados.borrow()[1].as_str(), avisos.borrow().len()), ("O contador entrega amanhã.", 1)); // respondeu com fato: nao incomoda o dono
        assert_eq!(estado(&d)["visto"], 22);
        // conversa que nao e a dela (leitor devolve vazio): nada acontece
        let vazio = Vec::new;
        assert_eq!(tick(&d, &q, &Portas { msgs: &vazio, ..p }), 0);
        assert!(titulo_confere("Lia Prado", &q) && !titulo_confere("Outro Contato", &q));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn atualizacao_so_repassa_o_que_e_do_assunto() {
        let (d, q) = (pasta("atual"), quem());
        let enviados = RefCell::new(Vec::<String>::new());
        let msgs = Vec::new;
        let enviar = |t: &str| { enviados.borrow_mut().push(t.to_string()); Ok(()) };
        let (gerar, avisar, log) = (|_: &str| None, |_: &str| (), |_: &str| ());
        let p = Portas { msgs: &msgs, enviar: &enviar, gerar: &gerar, avisar_dono: &avisar, log: &log };
        let hoje = Data::civil(2026, 11, 5).unwrap();
        assert!(!atualizacao(&d, &q, "bom dia, tudo bem?", hoje, &p) && enviados.borrow().is_empty());
        assert!(!atualizacao(&d, &q, "o cartório pediu o cpf e a senha do portal", hoje, &p) && fatos(&d).is_empty()); // dado sensivel nao e repassado nem anotado
        assert!(atualizacao(&d, &q, "Entrego no cartório hoje", hoje, &p));
        assert!(enviados.borrow()[0].contains("cartório") && enviados.borrow()[0].starts_with("Oi, Lia!") && fatos(&d).contains("o contador avisou: «Entrego no cartório hoje»"));
        let falha = |_: &str| Err("aba fechada".to_string());
        assert!(!atualizacao(&d, &q, "documento pronto", hoje, &Portas { enviar: &falha, ..p }));
        let _ = std::fs::remove_dir_all(&d);
    }
}
