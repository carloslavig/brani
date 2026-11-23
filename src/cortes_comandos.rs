//! Comandos dos cortes em texto: a entrada unica da ferramenta "cortes" do Jarvis (status, listar, aprovar, rejeitar, processar, publicar,
//! ligar, desligar). Ficou de fora (closures em `Acoes`): rodar o processamento em segundo plano, publicar e subir o vigia.
//! O alvo de "processar" e DADO: so e classificado (arquivo, clipe ou VOD), nunca executado.
#![allow(dead_code)]
use crate::cortes_config::arq;
use crate::empresa_dev::{corta, rx};
use crate::tempo::Agora;
use crate::{cortes_fila as fila, cortes_pipeline as pipeline, cortes_vigia as vigia, cortes_youtube as youtube};
use regex::Regex;
use serde_json::Value;
use std::path::Path;
use std::sync::OnceLock;

#[derive(Debug, PartialEq)]
pub enum Alvo {
    Arquivo(String),
    Clipe(String),
    Vod(String),
}

pub struct Acoes<'a> {
    pub tem_api_twitch: bool,
    /// (alvo, canal): comeca o processamento em segundo plano.
    pub processar: &'a dyn Fn(&Alvo, &str),
    /// `cortes_pipeline::publicar_aprovados` ja com as saidas de quem integra.
    pub publicar: &'a dyn Fn() -> Vec<String>,
    pub subir_vigia: &'a dyn Fn() -> Result<(), String>,
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}

pub fn status(dados: &Path, cfg: &Value, agora: &Agora, ts: f64, tem_api_twitch: bool) -> String {
    let canais: Vec<String> = vigia::canais_autorizados(cfg).iter().map(|c| format!("{}{}", s(c, "login"), if s(c, "perfil").is_empty() { String::new() } else { format!(" ({})", s(c, "perfil")) })).collect();
    let yt = |conta: &str| if youtube::configurado(dados, conta) { "configurado" } else { "falta login" };
    let perfis: String = cfg["perfis"].as_object().map(|o| o.iter().map(|(p, v)| format!(" · YouTube {p}: {}", yt(v["youtube_conta"].as_str().unwrap_or(p)))).collect()).unwrap_or_default();
    let r: Vec<String> = fila::resumo(dados).iter().map(|(k, v)| format!("{k} {v}")).collect();
    let mut linhas = vec![
        format!("Vigia dos cortes: {}", if vigia::rodando(dados, cfg, ts) { "LIGADO" } else { "desligado" }),
        format!("Canais autorizados: {}", if canais.is_empty() { format!("nenhum (editar {})", arq(dados).display()) } else { canais.join(", ") }),
        format!("Twitch API: {} · YouTube: {}{perfis}", if tem_api_twitch { "sim" } else { "nao (usando streamlink/yt-dlp)" }, yt("")),
        format!("Fila: {}", if r.is_empty() { "vazia".to_string() } else { r.join(", ") }),
    ];
    let horarios: Vec<&str> = cfg["youtube_horarios"].as_array().map(|a| a.iter().filter_map(|h| h.as_str()).collect()).unwrap_or_default();
    if !horarios.is_empty() {
        let ag: Vec<String> = fila::agenda(dados, "", pipeline::meia_noite(agora, ts)).iter().map(|c| format!("{} #{}", pipeline::dia_hora(c["agendado"].as_f64().unwrap_or(0.0), agora, ts), c["id"])).collect();
        linhas.push(format!("Meta: {} video(s)/dia as {} · estoque {} · agenda: {}", horarios.len(), horarios.join(", "), fila::estoque(dados, ""), if ag.is_empty() { "vazia".to_string() } else { ag.join("; ") }));
    }
    let pend: Vec<String> = fila::listar(dados, Some("renderizado"), 10).iter().map(|c| format!("#{} {}", c["id"], corta(s(c, "titulo"), 50))).collect();
    if !pend.is_empty() {
        linhas.push(format!("Esperando aprovacao: {}", pend.join("; ")));
    }
    linhas.join("\n")
}

pub fn listar(dados: &Path, status: &str) -> String {
    let cs = fila::listar(dados, Some(status), 15);
    if cs.is_empty() {
        return format!("nenhum corte{}", if status.is_empty() { String::new() } else { format!(" com status {status}") });
    }
    cs.iter().map(|c| format!("#{} [{}] {} · {:.0}s · {} ({}){}", c["id"], s(c, "status"), s(c, "canal"), c["fim"].as_f64().unwrap_or(0.0) - c["inicio"].as_f64().unwrap_or(0.0), corta(s(c, "titulo"), 60), s(c, "motivo"),
        if s(c, "youtube_id").is_empty() { String::new() } else { format!(" · yt {}", s(c, "youtube_id")) })).collect::<Vec<_>>().join("\n")
}

/// "todos" = os que esperam aprovacao; senao, os numeros que aparecem no texto.
pub fn ids(dados: &Path, alvo: &str) -> Vec<i64> {
    static N: OnceLock<Regex> = OnceLock::new();
    if ["todos", "tudo", "all"].contains(&alvo.trim().to_lowercase().as_str()) {
        return fila::listar(dados, Some("renderizado"), 100).iter().filter_map(|c| c["id"].as_i64()).collect();
    }
    rx(&N, r"\d+").find_iter(alvo).filter_map(|m| m.as_str().parse().ok()).collect()
}

fn numeros(v: &[i64]) -> String {
    v.iter().map(|i| format!("#{i}")).collect::<Vec<_>>().join(", ")
}

pub fn aprovar(dados: &Path, alvo: &str, ts: f64) -> String {
    let ok = pipeline::aprovar(dados, &ids(dados, alvo), ts);
    if ok.is_empty() {
        return "nenhum corte aprovado (confira os numeros com 'listar')".into();
    }
    format!("aprovado(s): {}. Sobem no proximo ciclo do vigia (ou rode 'publicar').", numeros(&ok))
}

pub fn rejeitar(dados: &Path, alvo: &str, ts: f64) -> String {
    let ok = pipeline::rejeitar(dados, &ids(dados, alvo), ts);
    if ok.is_empty() { "nenhum corte rejeitado".into() } else { format!("rejeitado(s): {}", numeros(&ok)) }
}

/// (o que processar, canal). `existe`: o alvo e um arquivo que existe no disco.
pub fn alvo_de(alvo: &str, canal: &str, existe: bool) -> Result<(Alvo, String), String> {
    static T: OnceLock<Regex> = OnceLock::new();
    let alvo = alvo.trim();
    let do_link = rx(&T, r"twitch\.tv/(\w+)").captures(alvo).map(|c| c[1].to_string()).filter(|c| c != "videos");
    let canal = if canal.is_empty() { do_link.unwrap_or_else(|| "canal".into()) } else { canal.to_string() };
    let a = if existe {
        Alvo::Arquivo(alvo.into())
    } else if alvo.contains("clip") {
        Alvo::Clipe(alvo.into())
    } else if alvo.starts_with("http") {
        Alvo::Vod(alvo.into())
    } else {
        return Err(format!("nao achei o arquivo nem e um link: {alvo}"));
    };
    Ok((a, canal))
}

/// Entrada unica para o Jarvis.
pub fn executar(dados: &Path, cfg: &Value, acao: &str, alvo: &str, agora: &Agora, ts: f64, a: &Acoes) -> String {
    let acao = if acao.trim().is_empty() { "status".to_string() } else { acao.trim().to_lowercase() };
    match acao.as_str() {
        "status" => status(dados, cfg, agora, ts, a.tem_api_twitch),
        "listar" => listar(dados, alvo),
        "aprovar" => aprovar(dados, alvo, ts),
        "rejeitar" => rejeitar(dados, alvo, ts),
        "processar" if alvo.trim().is_empty() => "diga o arquivo ou link".into(),
        "processar" => match alvo_de(alvo, "", Path::new(alvo.trim()).exists()) {
            Ok((x, canal)) => {
                (a.processar)(&x, &canal);
                "processando em segundo plano; os cortes chegam no Telegram quando ficarem prontos".into()
            }
            Err(e) => e,
        },
        "publicar" => {
            let rel = (a.publicar)();
            if rel.is_empty() { "nada aprovado esperando publicacao".into() } else { rel.join("\n") }
        }
        "ligar" => vigia::ligar(dados, cfg, ts, a.subir_vigia),
        "desligar" => vigia::desligar(dados),
        outra => format!("acao desconhecida: {outra}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cortes_config::padrao;
    use crate::cortes_fila::tmp;
    use crate::cortes_pipeline::agora_teste;
    use serde_json::json;
    use std::cell::RefCell;

    #[test]
    fn fila_aprovar_rejeitar_e_comandos() {
        let d = tmp("comandos");
        let a = fila::adicionar(&d, &json!({"canal": "c", "origem": "live", "fonte": "x", "inicio": 0, "fim": 30, "arquivo": "a.mp4", "titulo": "A", "motivo": "m"}), 1.0).unwrap();
        let b = fila::adicionar(&d, &json!({"canal": "c", "origem": "live", "fonte": "x", "inicio": 60, "fim": 90, "arquivo": "b.mp4", "titulo": "B", "motivo": "m"}), 1.0).unwrap();
        assert!(listar(&d, "").starts_with(&format!("#{b} [renderizado] c · 30s · B (m)\n#{a} ")));
        assert!(aprovar(&d, &a.to_string(), 2.0).contains(&format!("#{a}")));
        assert_eq!(fila::pegar(&d, a).unwrap()["status"], "aprovado");
        assert!(rejeitar(&d, &format!("corte {b}"), 2.0).contains(&format!("#{b}")));
        assert!(aprovar(&d, "todos", 3.0).starts_with("nenhum")); // nada mais esperando
        assert_eq!(fila::resumo(&d).into_iter().collect::<Vec<_>>(), vec![("aprovado".to_string(), 1), ("rejeitado".to_string(), 1)]);
        assert_eq!((ids(&d, "cortes 3, 12 e 7"), ids(&d, "nenhum")), (vec![3, 12, 7], vec![]));
        assert_eq!(listar(&d, "publicado"), "nenhum corte com status publicado");
        assert_eq!(rejeitar(&d, "99", 4.0), "nenhum corte rejeitado");
    }

    #[test]
    fn alvo_e_classificado_sem_executar() {
        assert_eq!(alvo_de(" https://www.twitch.tv/ana/clip/Abc ", "", false), Ok((Alvo::Clipe("https://www.twitch.tv/ana/clip/Abc".into()), "ana".into())));
        assert_eq!(alvo_de("https://www.twitch.tv/videos/123", "", false), Ok((Alvo::Vod("https://www.twitch.tv/videos/123".into()), "canal".into())));
        assert_eq!(alvo_de("live.ts", "bia", true), Ok((Alvo::Arquivo("live.ts".into()), "bia".into())));
        assert!(alvo_de("apaga tudo; rm -rf", "", false).is_err());
    }

    #[test]
    fn executar_e_status() {
        let d = tmp("comandos-exec");
        let (agora, ts) = agora_teste();
        let mut cfg = padrao();
        cfg["canais"] = json!([{"login": "ana", "autorizado": true, "perfil": "gringo"}, {"login": "bia", "autorizado": false}]);
        cfg["youtube_horarios"] = json!(["12:00"]);
        let feitos = RefCell::new(vec![]);
        let a = Acoes { tem_api_twitch: false, processar: &|x, canal| feitos.borrow_mut().push(format!("{x:?} {canal}")), publicar: &Vec::new, subir_vigia: &|| Err("sem python".into()) };
        let id = fila::adicionar(&d, &json!({"titulo": "Um corte", "status": "publicado"}), ts).unwrap();
        fila::atualizar(&d, id, &json!({"agendado": pipeline::meia_noite(&agora, ts) + 19.0 * 3600.0}), ts).unwrap();
        fila::adicionar(&d, &json!({"titulo": "Esperando"}), ts).unwrap();
        let st = executar(&d, &cfg, "", "", &agora, ts, &a);
        assert!(st.contains("Vigia dos cortes: desligado") && st.contains("Canais autorizados: ana (gringo)\n") && st.contains("Twitch API: nao (usando streamlink/yt-dlp) · YouTube: falta login · YouTube gringo: falta login"));
        assert!(st.contains("Fila: publicado 1, renderizado 1") && st.contains("Meta: 1 video(s)/dia as 12:00 · estoque 0 · agenda: 10/10 19:00 #1") && st.ends_with("Esperando aprovacao: #2 Esperando"));
        assert_eq!(executar(&d, &cfg, "Processar", "", &agora, ts, &a), "diga o arquivo ou link");
        assert!(executar(&d, &cfg, "processar", "https://clips.twitch.tv/Xyz", &agora, ts, &a).starts_with("processando em segundo plano"));
        assert!(executar(&d, &cfg, "processar", "nada disso", &agora, ts, &a).starts_with("nao achei"));
        assert_eq!(*feitos.borrow(), vec![r#"Clipe("https://clips.twitch.tv/Xyz") Xyz"#]);
        assert_eq!(executar(&d, &cfg, "publicar", "", &agora, ts, &a), "nada aprovado esperando publicacao");
        assert!(executar(&d, &cfg, "ligar", "", &agora, ts, &a).contains("nao consegui ligar"));
        assert!(executar(&d, &cfg, "desligar", "", &agora, ts, &a).starts_with("pedido de parada") && vigia::deve_parar(&d));
        assert_eq!(executar(&d, &cfg, "apagar", "", &agora, ts, &a), "acao desconhecida: apagar");
        assert!(executar(&d, &json!({}), "status", "", &agora, ts, &a).contains("Canais autorizados: nenhum (editar "));
    }
}
