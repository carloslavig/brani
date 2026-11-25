//! "Me mostra" por voz (braniac/mostrar.py): pergunta de situacao ("como ta o NexoDev?") -> qual tela abrir DENTRO do Braniac e a
//! frase a falar; e os links de fora: em vez de abrir sozinho, pergunta se abre aqui ou manda no celular.
//! Os dados de cada resumo (esteira, estacoes, agentes do predio) entram por closure; abrir navegador e mandar mensagem tambem.
//! De fora / diferente: o apelido da esteira (nome de pessoa) vem de configuracao em `apelidos`, nao fica no codigo; a porta local e parametro.
#![allow(dead_code)]
use crate::texto_pt::sem_acento;
use regex::Regex;
use serde_json::Value;

pub const VALIDADE_S: f64 = 20.0; // pagina que so ler depois disso (aba fechada) nao troca de tela atrasada
pub const PERGUNTA_LINK: &str =
    "Isso é um link. Abro aqui no navegador ou te mando no celular, no WhatsApp ou no Telegram? Se não souber, mando no WhatsApp, que pesa menos no PC.";

/// A ultima tela pedida; as paginas leem e trocam de tela uma vez por id.
#[derive(Default, Debug)]
pub struct Tela {
    pub id: u64,
    pub url: Option<String>,
    pub t: f64,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Assunto {
    Estacoes,
    Lote,
    /// (nome falado, numero do andar na API do predio)
    Andar(&'static str, i64),
    Predio,
}

fn re(p: &str) -> Regex {
    Regex::new(p).expect("regex fixa")
}

fn norm(x: &str) -> String {
    sem_acento(x).chars().map(|c| if c.is_ascii_lowercase() || c.is_ascii_digit() { c } else { ' ' }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

fn tem(n: &str, padrao: &str) -> bool {
    re(padrao).is_match(n)
}

// o Whisper escreve "estreia" para "esteira". Ordem importa: o mais especifico primeiro.
const TELAS: [(&str, &str, Assunto); 8] = [
    (r"\b(time|andar|equipe|pessoal|estacoes?) da (esteira|estreia)\b", "/sauraki?andar=esteira", Assunto::Estacoes),
    (r"\b(esteira|lote)\b", "/parceiro", Assunto::Lote),
    (r"\b(nexo ?dev|nexus ?dev|nexo de?v|time de programacao)\b", "/sauraki?andar=nexodev", Assunto::Andar("NexoDev", 2)),
    (r"\b(fornalha|marketing)\b", "/sauraki?andar=marketing", Assunto::Andar("Fornalha", 3)),
    (r"\b(claquete|cortes)\b", "/sauraki?andar=cortes", Assunto::Andar("Claquete", 5)),
    (r"\b(pechincha|compras)\b", "/sauraki?andar=compras", Assunto::Andar("Pechincha", 6)),
    (r"\b(quantix|investimentos?)\b", "/sauraki?andar=investimentos", Assunto::Andar("Quantix", 4)),
    (r"\b(escritorio|predio)\b", "/sauraki", Assunto::Predio),
];

/// (url, assunto) se for pergunta de situacao de um assunto da tabela. `apelidos` = outros nomes da esteira (configuracao).
pub fn intencao(texto: &str, apelidos: &[String]) -> Option<(&'static str, Assunto)> {
    let n = format!(" {} ", norm(texto));
    if !tem(&n, r"\b(como (ta|esta|anda|vai|que ta|que esta)|situacao|status|me mostra|mostra|abre|abrir|ve pra mim|da uma olhada)\b") {
        return None;
    }
    for (padrao, url, assunto) in TELAS {
        let apelido = assunto == Assunto::Lote && apelidos.iter().map(|a| norm(a)).any(|a| !a.is_empty() && n.contains(&format!(" {a} ")));
        if apelido || tem(&n, padrao) {
            return Some((url, assunto));
        }
    }
    None
}

pub fn abrir_tela(tela: &mut Tela, url: &str, agora: f64) {
    *tela = Tela { id: tela.id + 1, url: Some(url.to_string()), t: agora };
}

/// A ultima tela pedida, enquanto vale.
pub fn tela_atual(tela: &Tela, agora: f64) -> Option<(u64, String)> {
    tela.url.clone().filter(|_| agora - tela.t < VALIDADE_S).map(|u| (tela.id, u))
}

fn lista(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or(&[])
}

/// Frase falada de um assunto, so com dado real; sem dado diz que nao tem.
/// `dado`: Lote = objeto da esteira {meta, prontos, atual{cidade}}; Estacoes = lista [{agente, estado}]; Andar = lista de agentes [{andar, estado}].
pub fn resumo(assunto: Assunto, dado: &Value) -> String {
    match assunto {
        Assunto::Predio => "Abri o prédio do Braniac pra você.".into(),
        Assunto::Lote => {
            if !dado.as_object().is_some_and(|o| !o.is_empty()) {
                return "Não achei os dados da esteira de vídeos agora.".into();
            }
            let meta = dado["meta"].as_i64().filter(|m| *m != 0).unwrap_or(50);
            let fim = dado["atual"]["cidade"].as_str().filter(|c| !c.is_empty()).map(|c| format!(", agora fazendo {c}.")).unwrap_or_else(|| ".".into());
            format!("Esteira de vídeos: {} de {meta} vídeos prontos no lote{fim}", lista(&dado["prontos"]).len())
        }
        Assunto::Estacoes => {
            let est = lista(dado);
            if est.is_empty() {
                return "Não achei os dados do andar da esteira agora.".into();
            }
            let ativos: Vec<&str> = est.iter().filter(|s| s["estado"] == "executando").map(|s| s["agente"].as_str().unwrap_or("?")).collect();
            if ativos.is_empty() {
                return format!("Andar da esteira: as {} estações estão paradas agora.", est.len());
            }
            format!("Andar da esteira: {} de {} estações trabalhando ({}).", ativos.len(), est.len(), ativos.iter().take(4).cloned().collect::<Vec<_>>().join(", "))
        }
        Assunto::Andar(nome, andar) => {
            let ags: Vec<&Value> = lista(dado).iter().filter(|a| a["andar"].as_i64() == Some(andar)).collect();
            if ags.is_empty() {
                return format!("Não achei os dados do {nome} agora.");
            }
            let ativos = ags.iter().filter(|a| ["executando", "analisando", "planejando", "consultando"].contains(&a["estado"].as_str().unwrap_or(""))).count();
            if ativos == 0 {
                return format!("{nome}: o time está ocioso agora, {} agentes parados.", ags.len());
            }
            format!("{nome}: {ativos} de {} agentes trabalhando agora.", ags.len())
        }
    }
}

/// Abre a tela e devolve a frase a falar, ou None se nao for pergunta de situacao. `ler(assunto)` traz o dado do resumo.
pub fn mostrar(texto: &str, apelidos: &[String], tela: &mut Tela, agora: f64, ler: &dyn Fn(Assunto) -> Result<Value, String>) -> Option<String> {
    let (url, assunto) = intencao(texto, apelidos)?;
    abrir_tela(tela, url, agora);
    if assunto == Assunto::Predio {
        return Some(resumo(assunto, &Value::Null));
    }
    Some(ler(assunto).map(|d| resumo(assunto, &d)).unwrap_or_else(|_| "Abri a tela, mas não consegui ler os números agora.".into()))
}

// ---------- links de fora ----------

pub fn links(texto: &str) -> Vec<String> {
    re(r"https?://[^\s)>\]]+").find_iter(texto).map(|m| m.as_str().trim_end_matches(['.', ',', ';', ':', '!', '?']).to_string()).collect()
}

/// Link do proprio Braniac (127.0.0.1 ou localhost, sem porta ou na `porta` dele).
pub fn eh_local(url: &str, porta: u16) -> bool {
    re(&format!(r"^https?://(127\.0\.0\.1|localhost)(:{porta})?(/|$)")).is_match(url)
}

/// "aqui" | "whatsapp" | "telegram" | "nao" | None (nao respondeu a pergunta do link).
pub fn escolha(texto: &str) -> Option<&'static str> {
    let n = norm(texto);
    if tem(&n, r"\btelegram\b") {
        return Some("telegram");
    }
    if tem(&n, r"\b(nao|deixa|esquece|nada|cancela)\b") && !tem(&n, r"\b(manda|envia|abre)\b") {
        return Some("nao");
    }
    if tem(&n, r"\b(whats ?app|zap|zapzap|celular|telefone|manda|mande|envia|pode mandar)\b") {
        return Some("whatsapp");
    }
    tem(&n, r"\b(aqui|navegador|computador|pc|abre|abrir|abra)\b").then_some("aqui")
}

/// Executa a escolha do dono. Mandar e abrir sao de quem chama (WhatsApp vai pela caixa de saida do Braniac).
#[allow(clippy::too_many_arguments)]
pub fn entregar_link(urls: &[String], como: &str, tela: &mut Tela, agora: f64, porta: u16, abrir: &dyn Fn(&str), mandar_whatsapp: &dyn Fn(&str), mandar_telegram: &dyn Fn(&str) -> bool) -> &'static str {
    let texto = format!("Link que você pediu:\n{}", urls.join("\n"));
    match como {
        "aqui" => {
            for u in urls {
                if eh_local(u, porta) {
                    let caminho = re(r"^https?://[^/]+").replace(u, "").to_string();
                    abrir_tela(tela, if caminho.is_empty() { "/" } else { &caminho }, agora);
                } else {
                    abrir(u);
                }
            }
            "Pronto, abri aqui."
        }
        "telegram" if mandar_telegram(&texto) => "Mandei no seu Telegram.",
        "telegram" => "Não consegui mandar no Telegram. Quer que eu mande no WhatsApp?",
        "whatsapp" => {
            mandar_whatsapp(&texto);
            "Beleza, mandei no seu WhatsApp."
        }
        _ => "Tudo bem, deixei pra lá.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::RefCell;

    #[test]
    fn pergunta_de_situacao_escolhe_a_tela_e_o_mais_especifico_ganha() {
        let ap = vec!["Bento".to_string()];
        assert_eq!(intencao("Como tá o NexoDev?", &ap), Some(("/sauraki?andar=nexodev", Assunto::Andar("NexoDev", 2))));
        assert_eq!(intencao("como está o time da esteira?", &ap).map(|x| x.0), Some("/sauraki?andar=esteira"));
        assert_eq!(intencao("como anda a estreia?", &ap), None); // "estreia" sozinha nao e a esteira
        assert_eq!(intencao("como tá a esteira?", &ap).map(|x| x.1), Some(Assunto::Lote));
        assert_eq!(intencao("me mostra os vídeos do Bento", &ap).map(|x| x.0), Some("/parceiro"));
        assert_eq!(intencao("me mostra os vídeos do Bento", &[]), None);
        assert_eq!(intencao("abre o prédio", &ap), Some(("/sauraki", Assunto::Predio)));
        assert_eq!(intencao("status dos investimentos", &ap).map(|x| x.1), Some(Assunto::Andar("Quantix", 4)));
        assert_eq!(intencao("a fornalha entregou o post", &ap), None); // sem pergunta de situacao
        assert_eq!(intencao("como ta o tempo?", &ap), None);
    }

    #[test]
    fn resumos_so_com_dado_real() {
        assert_eq!(resumo(Assunto::Lote, &json!({})), "Não achei os dados da esteira de vídeos agora.");
        assert_eq!(resumo(Assunto::Lote, &json!({"prontos": [1, 2], "atual": {"cidade": "Vila Azul"}})), "Esteira de vídeos: 2 de 50 vídeos prontos no lote, agora fazendo Vila Azul.");
        assert_eq!(resumo(Assunto::Lote, &json!({"meta": 5, "prontos": []})), "Esteira de vídeos: 0 de 5 vídeos prontos no lote.");
        let est = json!([{"agente": "Voz", "estado": "executando"}, {"agente": "Render", "estado": "ocioso"}]);
        assert_eq!(resumo(Assunto::Estacoes, &est), "Andar da esteira: 1 de 2 estações trabalhando (Voz).");
        assert_eq!(resumo(Assunto::Estacoes, &json!([{"agente": "Voz", "estado": "ocioso"}])), "Andar da esteira: as 1 estações estão paradas agora.");
        assert_eq!(resumo(Assunto::Estacoes, &json!(null)), "Não achei os dados do andar da esteira agora.");
        let ags = json!([{"andar": 2, "estado": "planejando"}, {"andar": 2, "estado": "ocioso"}, {"andar": 3, "estado": "ocioso"}]);
        assert_eq!(resumo(Assunto::Andar("NexoDev", 2), &ags), "NexoDev: 1 de 2 agentes trabalhando agora.");
        assert_eq!(resumo(Assunto::Andar("Fornalha", 3), &ags), "Fornalha: o time está ocioso agora, 1 agentes parados.");
        assert_eq!(resumo(Assunto::Andar("Quantix", 4), &ags), "Não achei os dados do Quantix agora.");
    }

    #[test]
    fn mostrar_abre_a_tela_que_vale_por_20_segundos() {
        let mut tela = Tela::default();
        assert_eq!(tela_atual(&tela, 5.0), None);
        assert_eq!(mostrar("bom dia", &[], &mut tela, 100.0, &|_| panic!("nao le")), None);
        assert_eq!(tela.id, 0);
        let fala = mostrar("como tá a fornalha?", &[], &mut tela, 100.0, &|_| Ok(json!([{"andar": 3, "estado": "executando"}])));
        assert_eq!(fala.as_deref(), Some("Fornalha: 1 de 1 agentes trabalhando agora."));
        assert_eq!(tela_atual(&tela, 110.0), Some((1, "/sauraki?andar=marketing".to_string())));
        assert_eq!(tela_atual(&tela, 121.0), None);
        assert_eq!(mostrar("abre o escritório", &[], &mut tela, 200.0, &|_| panic!("predio nao le")).as_deref(), Some("Abri o prédio do Braniac pra você."));
        assert_eq!(mostrar("como ta o lote", &[], &mut tela, 300.0, &|_| Err("caiu".into())).as_deref(), Some("Abri a tela, mas não consegui ler os números agora."));
        assert_eq!(tela.id, 3);
    }

    #[test]
    fn link_nao_abre_sozinho_e_a_escolha_decide() {
        assert_eq!(links("olha https://exemplo.test/a?b=1, e (http://x.test/y)."), ["https://exemplo.test/a?b=1", "http://x.test/y"]);
        assert!(links("sem link").is_empty());
        assert!(eh_local("http://127.0.0.1:8865/b2", 8865) && eh_local("http://localhost", 8865) && !eh_local("http://127.0.0.1:9999/", 8865) && !eh_local("https://localhost.exemplo.test/", 8865));
        for (frase, e) in [("manda no Telegram", Some("telegram")), ("não, deixa", Some("nao")), ("não, manda no zap", Some("whatsapp")), ("no celular", Some("whatsapp")),
                           ("abre aqui", Some("aqui")), ("no PC mesmo", Some("aqui")), ("que horas são", None)] {
            assert_eq!(escolha(frase), e, "{frase}");
        }
        let (abertos, zap, tg) = (RefCell::new(vec![]), RefCell::new(vec![]), RefCell::new(vec![]));
        let abrir = |u: &str| abertos.borrow_mut().push(u.to_string());
        let wpp = |t: &str| zap.borrow_mut().push(t.to_string());
        let urls = vec!["https://exemplo.test/a".to_string(), "http://127.0.0.1:8865/sauraki".to_string()];
        let mut tela = Tela::default();
        assert_eq!(entregar_link(&urls, "nao", &mut tela, 1.0, 8865, &abrir, &wpp, &|_| true), "Tudo bem, deixei pra lá.");
        assert!(abertos.borrow().is_empty() && zap.borrow().is_empty() && tela.id == 0);
        assert_eq!(entregar_link(&urls, "aqui", &mut tela, 1.0, 8865, &abrir, &wpp, &|_| true), "Pronto, abri aqui.");
        assert_eq!((abertos.borrow().clone(), tela.url.clone()), (vec!["https://exemplo.test/a".to_string()], Some("/sauraki".to_string())));
        assert_eq!(entregar_link(&urls, "whatsapp", &mut tela, 1.0, 8865, &abrir, &wpp, &|_| true), "Beleza, mandei no seu WhatsApp.");
        assert!(zap.borrow()[0].starts_with("Link que você pediu:\nhttps://exemplo.test/a\n"));
        let telegram = |t: &str| { tg.borrow_mut().push(t.to_string()); false };
        assert_eq!(entregar_link(&urls, "telegram", &mut tela, 1.0, 8865, &abrir, &wpp, &telegram), "Não consegui mandar no Telegram. Quer que eu mande no WhatsApp?");
        assert_eq!((tg.borrow().len(), zap.borrow().len(), abertos.borrow().len()), (1, 1, 1));
    }
}
