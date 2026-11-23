//! Monitor das conversas do dono (braniac/monitor_dono.py): o Jarvis acompanha a lista de conversas e, por padrao, FICA QUIETO.
//! So age quando o DONO escreve (ou fala) num chat um comando dirigido a ele: "Jarvis, se apresente", "Jarvis, pare de responder",
//! "Jarvis, pode voltar a responder", "Jarvis, fale isso: ...". Aqui ficam as regras; mandar audio, silenciar o chat e marcar
//! "apresentado" entram como closure. O nome do dono na apresentacao vem de configuracao.
//! De fora: ouvir o audio do dono (transcricao) e o teto MAX_POR_HORA, que o Python define mas nao usa neste modulo.
#![allow(dead_code)]
use regex::Regex;
use serde_json::{json, Value};
use std::path::Path;

pub const MAX_AUDIO_S: i64 = 60;
pub const JANELA_JARVIS_S: f64 = 600.0;
pub const MAX_POR_HORA: usize = 6;
pub const MAX_FALA: usize = 600;

const PEDIDO: &str = r"(?i)\bjarvis\b[\s,:;!.-]*(?:(?:por favor|pf|pfv|agora|aqui|pode|ent[aã]o)[\s,]+)*(?:se\s+apresent\w*|apresent\w*[- ]?se|se\s+apresenta)\b";
const J: &str = r"(?i)\bjarvis\b[\s,:;!.-]*(?:(?:por favor|pf|pfv|agora|ent[aã]o|olha|ei|aqui)[\s,]+)*";
const SILENCIAR: &str = r"(?:(?:pare|para|parar)\s+de\s+(?:falar|responder|mandar)|n[aã]o\s+quero\s+que\s+(?:voc[eê]|vc|tu)\s+(?:responda|fale|diga|mande)|n[aã]o\s+(?:responda|fale|responde|fala)\b|(?:fique|fica)\s+(?:quieto|calado|em\s+sil[eê]ncio))";
const LIBERAR: &str = r"(?:(?:pode\s+)?(?:volt\w+)\s+a\s+(?:responder|falar)|pode\s+(?:responder|falar)\s+(?:de\s+novo|novamente|nessa\s+conversa|com\s+ele|com\s+ela))";
const FALAR: &str = r"(?s)(?:fale|fala|diga|diz|avise|avisa|passe|passa|repasse|repassa|mande|manda)\s+(?:isso(?:\s+(?:daqui|aqui))?|essa\s+informa\w+|a\s+informa\w+|que|pra\s+(?:ela|ele|pessoa)|para\s+(?:ela|ele|a\s+pessoa))\b[\s:,.-]*(?:(?:pra|para)\s+(?:ela|ele|a\s+pessoa)\b[\s:,.-]*)?(?P<c>.*)";

fn re(p: &str) -> Regex {
    Regex::new(p).expect("regex fixa")
}

pub fn apresentacao(dono: &str) -> String {
    format!("Oi, aqui é o Jarvis. Eu sou o assistente de inteligência artificial do {dono}, um sistema autônomo que ele criou para ajudar no atendimento e nas tarefas do dia a dia. Qualquer coisa que precisarem, é só falar comigo por aqui.")
}

/// True se a previa da conversa (ultima mensagem DO DONO) pede para o Jarvis se apresentar.
pub fn pedido_de_apresentacao(resumo: &str) -> bool {
    re(PEDIDO).is_match(resumo)
}

/// Manda a apresentacao em audio para `nome`. Devolve o resultado do envio (comeca com ✅ se enviou); so entao marca "apresentado".
pub fn apresentar(nome: &str, dono: &str, enviar_voz: &dyn Fn(&str, &str) -> String, marcar_apresentado: &dyn Fn(&str)) -> String {
    let r = enviar_voz(nome, &apresentacao(dono));
    if r.starts_with('✅') {
        marcar_apresentado(nome);
    }
    r
}

fn norm(n: &str) -> String {
    n.to_lowercase().chars().filter(|c| c.is_alphanumeric() || *c == '_').collect()
}

fn envios(dados: &Path) -> Vec<(String, f64)> {
    let v = std::fs::read_to_string(dados.join("audios_jarvis.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
    v.and_then(|v| v.as_array().cloned()).unwrap_or_default().iter().filter_map(|x| Some((x[0].as_str()?.to_string(), x[1].as_f64()?))).collect()
}

/// Guarda quando o Jarvis mandou audio nesse chat (o audio dele tambem aparece como "nosso" na lista). So a ultima hora, ate 41 envios.
pub fn registrar_envio(dados: &Path, nome: &str, agora: f64) {
    let mut v: Vec<(String, f64)> = envios(dados).into_iter().filter(|x| agora - x.1 < 3600.0).collect();
    v.drain(..v.len().saturating_sub(40));
    v.push((norm(nome), agora));
    let _ = std::fs::create_dir_all(dados);
    let _ = std::fs::write(dados.join("audios_jarvis.json"), json!(v).to_string());
}

/// True se a ultima mensagem do chat e um audio NOSSO curto (ate 60 s) que o Jarvis nao enviou agora ha pouco (= o dono falou no celular).
/// `resumo` = previa da lista, que termina na duracao ("0:12").
pub fn audio_curto_do_dono(dados: &Path, nome: &str, nossa: bool, resumo: &str, agora: f64) -> bool {
    if !nossa {
        return false;
    }
    let Some(m) = re(r"(?:^|\|)\s*(\d{1,2}):(\d{2})\s*$").captures(resumo) else { return false };
    let seg = m[1].parse::<i64>().unwrap_or(99) * 60 + m[2].parse::<i64>().unwrap_or(99);
    seg <= MAX_AUDIO_S && !envios(dados).iter().any(|x| x.0 == norm(nome) && agora - x.1 < JANELA_JARVIS_S)
}

/// (acao, conteudo) do comando do dono: "silenciar" | "liberar" | "apresentar" | "falar" (conteudo = o que dizer); (None, "") se nao for comando.
pub fn interpretar(texto: &str) -> (Option<&'static str>, String) {
    if re(&format!("{J}{SILENCIAR}")).is_match(texto) {
        return (Some("silenciar"), String::new());
    }
    if re(&format!("{J}{LIBERAR}")).is_match(texto) {
        return (Some("liberar"), String::new());
    }
    if re(PEDIDO).is_match(texto) {
        return (Some("apresentar"), String::new());
    }
    let c = re(&format!("{J}{FALAR}")).captures(texto).map(|m| m["c"].split_whitespace().collect::<Vec<_>>().join(" ")).unwrap_or_default();
    if c.is_empty() { (None, String::new()) } else { (Some("falar"), c.chars().take(MAX_FALA).collect()) }
}

/// Executa o comando. Silenciar: o Jarvis NAO responde mais esse chat (so fala se o dono mandar "fale/passe isso"). Liberar desfaz.
/// `apresentar_fn(nome)` e `falar_fn(nome, texto)` mandam o audio e devolvem o resultado do envio.
pub fn executar(nome: &str, acao: &str, conteudo: &str, silenciar: &dyn Fn(&str, bool), apresentar_fn: &dyn Fn(&str) -> String, falar_fn: &dyn Fn(&str, &str) -> String) -> String {
    match acao {
        "silenciar" => {
            silenciar(nome, true);
            "silenciado (sem aviso na conversa)".into()
        }
        "liberar" => {
            silenciar(nome, false);
            "liberado".into()
        }
        "apresentar" => apresentar_fn(nome),
        "falar" => falar_fn(nome, conteudo),
        _ => "nada".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn so_comando_dirigido_ao_jarvis_vira_acao() {
        for (frase, acao, conteudo) in [
            ("Jarvis, se apresente", Some("apresentar"), ""),
            ("jarvis por favor, apresente-se", Some("apresentar"), ""),
            ("Jarvis, pare de responder", Some("silenciar"), ""),
            ("Jarvis, não quero que você responda aqui", Some("silenciar"), ""),
            ("Jarvis fica quieto", Some("silenciar"), ""),
            ("Jarvis, pode voltar a responder", Some("liberar"), ""),
            ("Jarvis, pode falar com ela", Some("liberar"), ""),
            ("Jarvis, fale isso:  a reunião\n mudou   para as 15h", Some("falar"), "a reunião mudou para as 15h"),
            ("Jarvis, avisa que eu chego em dez minutos", Some("falar"), "eu chego em dez minutos"),
            ("Jarvis, passa essa informação pra ela: o portão fica aberto", Some("falar"), "o portão fica aberto"),
            ("Jarvis, fale isso", None, ""),               // sem conteudo nao e comando
            ("se apresente pra turma", None, ""),          // nao chamou o Jarvis
            ("o jarvis é legal, pare de falar nisso", None, ""),
            ("bom dia, tudo bem?", None, ""),
        ] {
            assert_eq!(interpretar(frase), (acao, conteudo.to_string()), "{frase}");
        }
        assert!(pedido_de_apresentacao("Jarvis, se apresenta aí") && !pedido_de_apresentacao("0:12"));
        assert_eq!(interpretar(&format!("Jarvis, diga que {}", "a".repeat(900))).1.chars().count(), MAX_FALA);
    }

    #[test]
    fn executa_pelas_closures_e_so_marca_apresentado_se_o_audio_foi() {
        let (sil, marcados, falas) = (RefCell::new(vec![]), RefCell::new(vec![]), RefCell::new(vec![]));
        let silenciar = |n: &str, s: bool| sil.borrow_mut().push((n.to_string(), s));
        let marcar = |n: &str| marcados.borrow_mut().push(n.to_string());
        let foi = |n: &str, t: &str| -> String {
            falas.borrow_mut().push((n.to_string(), t.to_string()));
            "✅ áudio enviado".into()
        };
        let x = |acao: &str| executar("Bia", acao, "oi", &silenciar, &|n| apresentar(n, "Ana", &foi, &marcar), &foi);
        assert_eq!((x("silenciar"), x("liberar"), x("outra")), ("silenciado (sem aviso na conversa)".to_string(), "liberado".to_string(), "nada".to_string()));
        assert_eq!(*sil.borrow(), [("Bia".to_string(), true), ("Bia".to_string(), false)]);
        assert!(falas.borrow().is_empty());
        assert!(x("apresentar").starts_with('✅') && x("falar").starts_with('✅'));
        assert!(falas.borrow()[0].1.contains("assistente de inteligência artificial do Ana") && falas.borrow()[1] == ("Bia".to_string(), "oi".to_string()));
        assert_eq!(*marcados.borrow(), ["Bia"]);
        assert_eq!(apresentar("Caio", "Ana", &|_, _| "❌ sem voz".into(), &|_| panic!("nao marca se nao enviou")), "❌ sem voz");
    }

    #[test]
    fn audio_curto_nosso_e_do_dono_so_se_o_jarvis_nao_mandou_agora_ha_pouco() {
        let d = std::env::temp_dir().join(format!("braniac-monitor_dono-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let t0 = 1_800_000_000.0;
        assert!(audio_curto_do_dono(&d, "Bia", true, "0:12", t0) && audio_curto_do_dono(&d, "Bia", true, "✓ | 1:00 ", t0));
        for (nossa, resumo) in [(false, "0:12"), (true, "1:01"), (true, "12:00"), (true, "te ligo 0:12 depois"), (true, "oi")] {
            assert!(!audio_curto_do_dono(&d, "Bia", nossa, resumo, t0), "{resumo}");
        }
        registrar_envio(&d, "Bia 🤍", t0);
        assert!(!audio_curto_do_dono(&d, "bia", true, "0:12", t0 + 599.0)); // o audio na lista e o do proprio Jarvis
        assert!(audio_curto_do_dono(&d, "Caio", true, "0:12", t0 + 599.0) && audio_curto_do_dono(&d, "Bia", true, "0:12", t0 + 601.0));
        for i in 0..50 {
            registrar_envio(&d, "Caio", t0 + 700.0 + i as f64);
        }
        assert_eq!(envios(&d).len(), 41);
        registrar_envio(&d, "Caio", t0 + 9000.0);
        assert_eq!(envios(&d).len(), 1); // so a ultima hora
    }
}
