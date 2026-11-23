//! Airbnb (anfitriao): LEITURA do que ja foi lido da pagina (porte das funcoes puras de braniac/airbnb.py): linha da caixa de
//! entrada -> quem e o hospede, qual o anuncio, quem falou por ultimo; bolhas da conversa -> quem escreveu cada uma;
//! respostas rapidas; e as conferencias de antes/depois de enviar ou decidir reserva.
//! Ficou de fora: a sessao no navegador (classe AB: abrir aba, clicar, digitar) e os scripts de pagina. Nunca faz login.
//! `parse_noites`, `parse_hospedes`, `estado_reserva`, `prazo_vencido` e `eh_quarto` ja estao em `airbnb_vigia.rs`.
//! Diferenca do Python: o anuncio em que so se atende hospede e reconhecido pelo TITULO (fixo aqui) mais o que vier em
//! dados/pessoal.json "airbnb_protegido" (regex; no Python o apelido do dono do apartamento ficava no codigo).
#![allow(dead_code)]
use crate::contexto::re;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

pub const MAX_TEXTO: usize = 900;

re!(titulo_protegido, r"(?i)apartamento\s+completo\s*-\s*perto\s+da\s+praia\s+e\s+de\s+tudo");
re!(linha_re, r"(?s)^(?P<head>.*?)\.\s*A [uú]ltima mensagem (?P<quando>.+?) enviada [eé]:\s*(?P<prev>.*)$");
re!(ponto_espaco, r"\.\s+");
re!(suporte_nome, r"(?i)atendimento ao cliente airbnb");
re!(suporte_re, r"(?i)atendimento ao cliente airbnb|airbnb\s+suporte");
re!(quem_fala, r"^([^:]{1,40}):\s*(.*)$");
re!(cab, r"(?s)^(?P<nome>[^·]{1,60}?)\s*·\s*(?P<papel>Anfitri[ãa]\(o\)|Coanfitri[ãa]\(o\)|Respons[áa]vel pela reserva|H[óo]spede[^\d]*?|Convidad[oa]|Co-?anfitri[ãa]\(o\))\s*(?P<resto>.*)$");
re!(hora_re, r"(?s)^(?P<h>\d{1,2}:\d{2})\s*(?P<resto>.*)$");
re!(lida, r"(?i)\s*(?:Lida por [^\n]{1,40}|Entregue|Enviada)\s*$");
re!(respondendo, r"^Respondendo a (?:você|[^ ]+)\s+");
re!(sistema, r"(?i)^(Consulta de reserva|Pedido (?:de reserva|recusado|aceito|expirado|cancelado)|Reserva (?:confirmada|cancelada|alterada|solicitada|pendente)|Confirmado\b|Cancelad[oa]\b|Solicita[cç][ãa]o|Convite|Pr[eé]-aprova|Voc[eê] pr[eé]-aprovou|Altera[cç][ãa]o|Aviso)");
re!(anfitriao, r"(?i)^(Co)?anfitri");

fn norm(t: &str) -> String {
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Regex extra do anuncio protegido, da configuracao (o titulo fixo vale sempre).
pub fn protegido_extra(dados: &Path) -> Option<Regex> {
    let v: Value = std::fs::read_to_string(dados.join("pessoal.json")).ok().and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok())?;
    v["airbnb_protegido"].as_str().filter(|s| !s.trim().is_empty()).and_then(|s| Regex::new(&format!("(?i){s}")).ok())
}

/// Anuncio em que so se responde hospede: nunca reserva, nunca alteracao.
pub fn anuncio_protegido(anuncio: &str, extra: Option<&Regex>) -> bool {
    titulo_protegido().is_match(anuncio) || extra.is_some_and(|r| r.is_match(anuncio))
}

/// Texto de uma linha da caixa de entrada -> objeto (ou None se nao der para entender). As chaves sao as de `airbnb_vigia::Linha`
/// mais "nao_lida". Ex.: 'Lidas. Consulta. 8 – 9 de out.. Quarto X · casa com piscina. Fabio. A última mensagem 22:48 enviada é: Você: Bem vindo'
pub fn parse_linha(thread_id: &str, texto: &str, extra: Option<&Regex>) -> Option<Value> {
    let t = norm(texto);
    let m = linha_re().captures(&t)?;
    let partes: Vec<&str> = ponto_espaco().split(&m["head"]).map(str::trim).filter(|p| !p.is_empty()).collect();
    if partes.len() < 3 {
        return None;
    }
    let n = partes.len();
    let (status, meta, mut anuncio, mut hospede) = (partes[0], &partes[1..n - 2], partes[n - 2], partes[n - 1]);
    if suporte_nome().is_match(partes[n - 1]) {
        // conversa com o SUPORTE do Airbnb: o nome vem antes do rotulo
        (anuncio, hospede) = (partes[n - 1], partes[n - 2]);
    }
    let prev = m["prev"].trim().trim_end_matches('.');
    let primeiro = |x: &str| x.split(' ').next().unwrap_or("").to_lowercase();
    let (de, previa) = if let Some(r) = prev.strip_prefix("Você:") {
        ("voce", r.trim())
    } else {
        match quem_fala().captures(prev) {
            Some(c) => (if primeiro(&norm(&c[1])) == primeiro(hospede) { "hospede" } else { "outro" }, c.get(2).map_or("", |x| x.as_str())),
            None => ("outro", prev),
        }
    };
    Some(json!({ "id": thread_id, "nao_lida": !status.to_lowercase().starts_with("lida"), "meta": meta.join(". "), "anuncio": anuncio, "hospede": hospede,
        "quando": m["quando"].trim(), "ultima_de": de, "previa": previa, "suporte_airbnb": suporte_re().is_match(anuncio), "protegido": anuncio_protegido(anuncio, extra) }))
}

/// A conversa espera resposta NOSSA? (a ultima mensagem e do hospede e nao e o suporte do Airbnb)
pub fn pendente_de_resposta(linha: &Value) -> bool {
    linha["ultima_de"] == "hospede" && linha["suporte_airbnb"] != true
}

pub fn limpar_texto(t: &str) -> String {
    let t = norm(t);
    let t = respondendo().replace(&t, "");
    lida().replace(&t, "").trim().to_string()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Mensagem {
    /// "hospede" | "anfitriao" | "voce" | "sistema"
    pub de: String,
    pub nome: String,
    pub hora: String,
    pub texto: String,
}

/// itens = [{"txt": str, "escuro": bool, "y": int}] -> mensagens em ordem cronologica (com "y" em todos, vale a ordem VISUAL).
/// Bolha ESCURA = enviada pela conta logada ("voce"); clara = do outro lado. O cabecalho "Nome · Papel HH:MM" vem na 1a mensagem
/// de cada sequencia e diz se o outro lado e o hospede ou um anfitriao; as bolhas claras seguintes herdam o ultimo cabecalho.
pub fn parse_mensagens(itens: &[Value]) -> Vec<Mensagem> {
    let mut itens: Vec<&Value> = itens.iter().collect();
    if !itens.is_empty() && itens.iter().all(|i| i["y"].is_number()) {
        itens.sort_by(|a, b| a["y"].as_f64().partial_cmp(&b["y"].as_f64()).unwrap_or(std::cmp::Ordering::Equal));
    }
    let (mut out, mut papel, mut nome) = (Vec::new(), String::new(), String::new());
    for it in itens {
        let txt = norm(it["txt"].as_str().unwrap_or(""));
        if txt.is_empty() {
            continue;
        }
        let com_cab = cab().captures(&txt);
        let corpo = match &com_cab {
            Some(c) => {
                (nome, papel) = (c["nome"].trim().to_string(), c["papel"].trim().to_string());
                c["resto"].to_string()
            }
            None => txt.clone(),
        };
        let (hora, resto) = match hora_re().captures(&corpo) {
            Some(h) => (h["h"].to_string(), h["resto"].to_string()),
            None => (String::new(), corpo),
        };
        let de = if sistema().is_match(&if com_cab.is_some() { norm(&resto) } else { txt.clone() }) {
            "sistema"
        } else if it["escuro"] == true {
            "voce"
        } else if anfitriao().is_match(&papel) {
            "anfitriao"
        } else {
            "hospede"
        };
        let texto = limpar_texto(if de == "sistema" { &txt } else { &resto });
        if !texto.is_empty() {
            let quem = match de {
                "voce" => "Você".to_string(),
                "sistema" => "Airbnb".to_string(),
                _ => nome.clone(),
            };
            out.push(Mensagem { de: de.into(), nome: quem, hora, texto });
        }
    }
    out
}

/// Texto do painel "Respostas rapidas" (titulo e corpo em linhas alternadas) -> {titulo em minusculas: corpo}.
pub fn parse_respostas_rapidas(texto: &str) -> BTreeMap<String, String> {
    let mut linhas: Vec<&str> = texto.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    if linhas.first().is_some_and(|l| l.to_lowercase().starts_with("respostas r")) {
        linhas.remove(0);
    }
    let mut out = BTreeMap::new();
    for par in linhas.chunks(2) {
        if par.len() < 2 || par[0].to_lowercase().starts_with("obtenha mais respostas") {
            break;
        }
        out.insert(par[0].to_lowercase(), par[1].to_string());
    }
    out
}

/// So os digitos do id da conversa; id sem digito nao abre nada.
pub fn id_conversa(thread_id: &str) -> Result<String, String> {
    let id: String = thread_id.chars().filter(char::is_ascii_digit).collect();
    if id.is_empty() { Err("id de conversa invalido".into()) } else { Ok(id) }
}

/// O texto que pode ir para a caixa de mensagem (espacos arrumados), ou por que nao: vazio, ou longo demais.
pub fn validar_envio(texto: &str) -> Result<String, String> {
    let t = norm(texto);
    if t.is_empty() {
        Err("texto vazio".into())
    } else if t.chars().count() > MAX_TEXTO {
        Err(format!("texto longo demais (>{MAX_TEXTO})"))
    } else {
        Ok(t)
    }
}

/// Antes de clicar em Enviar: o que esta na caixa e mesmo o texto? Se nao, a caixa e limpa e NADA e enviado.
pub fn texto_entrou(digitado: &str, texto: &str) -> Result<(), String> {
    let ini = |x: &str| x.chars().take(30).collect::<String>();
    if ini(&norm(digitado)) == ini(texto) { Ok(()) } else { Err("o texto nao entrou na caixa (nada foi enviado)".into()) }
}

/// Depois de clicar: a mensagem apareceu na conversa?
pub fn envio_confirmado(antes: usize, depois: usize) -> Result<(), String> {
    if depois > antes { Ok(()) } else { Err("a mensagem nao apareceu na conversa (confira no Airbnb)".into()) }
}

/// Rotulos da decisao de reserva: (botao, botao de confirmar, marca que aparece na conversa). So existem pre-aprovar e aceitar:
/// recusar, valor e reembolso nunca sao feitos pelo sistema.
pub fn rotulos_decisao(acao: &str) -> Option<(&'static str, &'static str, &'static str)> {
    match acao {
        "preaprovar" => Some((r"^Pré-aprovar$", r"^Sim, pré-aprovar", r"(?i)Convite enviado|pr[eé]-aprova")),
        "aceitar" => Some((r"^Aceitar( pedido)?$", r"^(Sim, aceitar|Aceitar|Confirmar)", r"(?i)Pedido aceito|Confirmado")),
        _ => None,
    }
}

/// Quantas vezes a marca da decisao aparece nos cartoes de sistema (conta antes e depois de confirmar).
pub fn marcas_da_decisao(acao: &str, msgs: &[Mensagem]) -> usize {
    let Some(r) = rotulos_decisao(acao).and_then(|(_, _, m)| Regex::new(m).ok()) else { return 0 };
    msgs.iter().filter(|m| m.de == "sistema").map(|m| r.find_iter(&m.texto).count()).sum()
}

/// A janela de confirmacao e a esperada? E depois: o resultado apareceu na conversa?
pub fn dialogo_esperado(texto_do_dialogo: &str) -> Result<(), String> {
    re!(d, r"(?i)pré-aprovar|aceitar|aceit");
    if d().is_match(texto_do_dialogo) { Ok(()) } else { Err("a janela de confirmacao nao apareceu como esperado (nada foi confirmado)".into()) }
}

pub fn decisao_confirmada(antes: usize, depois: usize) -> Result<(), String> {
    if depois > antes { Ok(()) } else { Err("cliquei em confirmar mas nao vi o resultado na conversa; confira no Airbnb".into()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::airbnb_vigia::Linha;

    #[test]
    fn linha_enviada_por_nos() {
        let l = parse_linha("2692213014", "Lidas. 8 – 9 de out.. Quarto Azul · casa com piscina, 500m da praia. Graça. A última mensagem 22:50 enviada é: Você: Convite enviado.", None).unwrap();
        assert!(l["hospede"] == "Graça" && l["anuncio"].as_str().unwrap().starts_with("Quarto Azul") && l["ultima_de"] == "voce" && l["nao_lida"] == false);
        assert!(l["previa"] == "Convite enviado" && l["quando"] == "22:50" && !pendente_de_resposta(&l));
        assert_eq!(Linha::de_json(&l).hospede, "Graça"); // o formato e o que o vigia le
    }

    #[test]
    fn linha_do_hospede_e_nao_lida() {
        let l = parse_linha("2682241976", "Não lidas. Check-in hoje. Quarto Verde · casa com piscina, 500m da praia. João Vitor. A última mensagem 15:18 enviada é: João Vitor: A caminho, chegando em breve.", None).unwrap();
        assert!(l["nao_lida"] == true && l["ultima_de"] == "hospede" && l["meta"] == "Check-in hoje" && pendente_de_resposta(&l));
        assert_eq!(l["previa"], "A caminho, chegando em breve");
    }

    #[test]
    fn datas_com_ponto_nao_quebram_o_anuncio_nem_o_hospede() {
        let l = parse_linha("1", "Lidas. 31 de dez. de 2026 – 3 de jan. de 2027. Casa do Sul · 3 quartos e piscina. Joana. A última mensagem 21:39 enviada é: Joana: Ok. Obrigado.", None).unwrap();
        assert!(l["hospede"] == "Joana" && l["anuncio"].as_str().unwrap().starts_with("Casa do Sul") && l["meta"].as_str().unwrap().contains("2027") && l["ultima_de"] == "hospede");
        assert!(parse_linha("9", "texto que nao e linha da caixa", None).is_none() && parse_linha("9", "Lidas. Joana. A última mensagem 10:00 enviada é: oi", None).is_none());
    }

    #[test]
    fn conversa_do_suporte_nao_pede_resposta_e_terceiro_nao_e_hospede() {
        let l = parse_linha("2", "Lidas. Encerrado. Marcio. Atendimento ao Cliente Airbnb. A última mensagem 16:59 enviada é: Como nos saímos?.", None).unwrap();
        assert!(l["suporte_airbnb"] == true && l["hospede"] == "Marcio" && !pendente_de_resposta(&l));
        let l = parse_linha("4", "Lidas. Hospedado. Quarto Azul · casa. Paulo. A última mensagem 10:00 enviada é: Nice: Pode vir.", None).unwrap();
        assert!(l["ultima_de"] == "outro" && !pendente_de_resposta(&l));
    }

    #[test]
    fn anuncio_protegido_e_marcado() {
        let l = parse_linha("3", "Lidas. Apartamento completo - Perto da praia e de tudo. Paulo. A última mensagem 10:00 enviada é: Paulo: Oi.", None).unwrap();
        assert!(l["protegido"] == true && pendente_de_resposta(&l));
        let extra = Regex::new("(?i)studio norte").unwrap();
        assert!(anuncio_protegido("Studio Norte · centro", Some(&extra)) && !anuncio_protegido("Studio Norte · centro", None));
        let d = std::env::temp_dir().join(format!("braniac-airbnb-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        assert!(protegido_extra(&d).is_none());
        std::fs::write(d.join("pessoal.json"), r#"{"airbnb_protegido": "studio norte|ap do tio"}"#).unwrap();
        assert!(protegido_extra(&d).unwrap().is_match("AP do Tio"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn mensagens_quem_escreveu_pela_cor_e_cabecalho_em_ordem_visual() {
        let itens = vec![
            // fora de ordem de proposito: o "y" manda
            json!({"txt": "Joana · Responsável pela reserva\n21:39\nOk. Obrigado", "escuro": false, "y": 900}),
            json!({"txt": "15:18\nBoa tarde, estadia mínima pra esse período é 5 dias\nLida por Joana", "escuro": true, "y": 300}),
            json!({"txt": "Joana · Responsável pela reserva\n13:12\nBoa tarde! Gostaria de saber mais", "escuro": false, "y": 100}),
            json!({"txt": "Consulta de reserva\nR$ 3.138,17 · 16 hóspedes\nVocê tem até 7 de out. para responder\nResponder", "escuro": false, "y": 200}),
            json!({"txt": "Nice · Anfitriã(o)\n16:17\nSó se você quiser esticar", "escuro": false, "y": 500}),
            json!({"txt": "Domingo tem vaga", "escuro": false, "y": 560}),
        ];
        let ms = parse_mensagens(&itens);
        assert_eq!(ms.iter().map(|m| m.de.as_str()).collect::<Vec<_>>(), ["hospede", "sistema", "voce", "anfitriao", "anfitriao", "hospede"]);
        assert!(ms[0].texto.starts_with("Boa tarde! Gostaria") && ms[0].hora == "13:12");
        assert_eq!(ms[2].texto, "Boa tarde, estadia mínima pra esse período é 5 dias"); // "Lida por" removido
        assert!(ms[1].nome == "Airbnb" && ms[3].nome == "Nice" && ms[4].texto == "Domingo tem vaga" && ms[2].nome == "Você");
        // sem "y", vale a ordem dada
        assert_eq!(parse_mensagens(&[json!({"txt": "b", "escuro": true}), json!({"txt": "", "escuro": true}), json!({"txt": "a"})]).iter().map(|m| m.texto.as_str()).collect::<Vec<_>>(), ["b", "a"]);
    }

    #[test]
    fn cartao_confirmado_com_cabecalho_do_hospede_e_sistema() {
        let ms = parse_mensagens(&[json!({"txt": "Adilson · Responsável pela reserva\nConfirmado Quarto Verde · 11 – 12 de out. de 2026 · R$ 175,83", "escuro": false, "y": 1})]);
        assert_eq!(ms[0].de, "sistema");
        assert_eq!(marcas_da_decisao("aceitar", &ms), 1);
        assert_eq!(marcas_da_decisao("preaprovar", &ms), 0);
    }

    #[test]
    fn envio_recusa_vazio_longo_e_texto_que_nao_entrou() {
        assert_eq!(validar_envio("   "), Err("texto vazio".into()));
        assert!(validar_envio(&"x".repeat(901)).is_err());
        assert_eq!(validar_envio("Oi,   tudo bem?"), Ok("Oi, tudo bem?".into()));
        assert!(texto_entrou("Oi, tudo bem?", "Oi, tudo bem?").is_ok());
        assert!(texto_entrou("", "Olá").unwrap_err().contains("nada foi enviado"));
        assert!(envio_confirmado(5, 6).is_ok() && envio_confirmado(5, 5).is_err());
        assert_eq!(id_conversa("thread/123-4"), Ok("1234".into()));
        assert!(id_conversa("abc").is_err());
    }

    #[test]
    fn decisao_so_preaprova_ou_aceita_e_confere_o_resultado() {
        assert!(rotulos_decisao("preaprovar").is_some() && rotulos_decisao("aceitar").is_some());
        assert!(rotulos_decisao("recusar").is_none() && rotulos_decisao("").is_none());
        assert!(dialogo_esperado("Sim, pré-aprovar").is_ok() && dialogo_esperado("Cookies").unwrap_err().contains("nada foi confirmado"));
        assert!(decisao_confirmada(0, 1).is_ok() && decisao_confirmada(1, 1).is_err());
        let r = parse_respostas_rapidas("Respostas rápidas\nfirstcontact\nOlá! O quarto é privativo.\nCheck-in\nA partir das 14h.\nObtenha mais respostas\nx");
        assert_eq!(r.len(), 2);
        assert_eq!(r["firstcontact"], "Olá! O quarto é privativo.");
        assert_eq!(r["check-in"], "A partir das 14h.");
    }
}
