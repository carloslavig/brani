//! Orquestra um corte do inicio ao fim (legenda -> titulo -> render 9:16 -> fila) e a publicacao dos APROVADOS.
//! Travas: com "aprovacao_manual" (padrao) o corte entra como "renderizado" e so sobe depois de `aprovar`; `publicar_aprovados` so olha
//! a lista "aprovado"; teto diario do YouTube por perfil; grade de horarios so para perfil publico (perfil privado nunca vira publico por
//! agendamento); falha no YouTube tira o corte da fila automatica ("erro_youtube") para nao tentar nem avisar a cada ciclo.
//! Ficou de fora (closures em `Estudio` e `Saidas`): Whisper, ffmpeg, yt-dlp, IA local, envio ao YouTube/TikTok e o aviso ao dono.
//! Diferenca do Python: o aviso de corte novo nao esta preso ao Telegram (quem chama passa as closures) e a grade de horarios e arredondada
//! ao minuto (o relogio entra por parametro).
#![allow(dead_code)]
use crate::cortes_config::{do_perfil, pasta, sub};
use crate::cortes_legenda::Palavra;
use crate::cortes_titulos::{sem_repetir, Pedido};
use crate::cortes_youtube::Subir;
use crate::empresa_dev::corta;
use crate::tempo::{Agora, Data};
use crate::{cortes_deteccao, cortes_fila as fila, cortes_legenda, cortes_render, cortes_tiktok, cortes_titulos, cortes_youtube};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn log(dados: &Path, agora: &Agora, txt: &str) {
    let _ = std::fs::create_dir_all(pasta(dados));
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(pasta(dados).join("cortes.log")) {
        let _ = writeln!(f, "{} {:02}:{:02}:{:02} {txt}", agora.data.dia_mes(), agora.hora, agora.min, agora.seg);
    }
}

pub fn slug(t: &str) -> String {
    let mut s = String::new();
    for c in t.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            s.push(c);
        } else if !s.ends_with('_') {
            s.push('_');
        }
    }
    let s = corta(s.trim_matches('_'), 40);
    if s.is_empty() { "corte".into() } else { s }
}

/// Meia-noite local de hoje, em carimbo, arredondada ao minuto.
pub fn meia_noite(agora: &Agora, ts: f64) -> f64 {
    ((ts - agora.segundos() as f64) / 60.0).round() * 60.0
}

/// "dd/mm HH:MM" local de um carimbo.
pub fn dia_hora(alvo: f64, agora: &Agora, ts: f64) -> String {
    let l = (alvo - meia_noite(agora, ts)).floor() as i64;
    let r = l.rem_euclid(86400);
    format!("{} {:02}:{:02}", Data(agora.data.0 + l.div_euclid(86400)).dia_mes(), r / 3600, r / 60 % 60)
}

/// Descricao do video: modelo do perfil (canal gringo: ingles, com credito ao streamer e link do clipe original).
pub fn descricao(cfg: &Value, canal: &str, nome: &str, titulo: &str, hashtags: &[String], fonte: &str) -> String {
    let tags = hashtags.join(" ");
    let Some(modelo) = cfg["descricao_modelo"].as_str().filter(|m| !m.is_empty()) else { return format!("{titulo}\n\nCorte da live de twitch.tv/{canal}\n\n{tags}") };
    let link = if fonte.starts_with("http") { format!("\u{1F3AC} Original clip: {fonte}\n") } else { String::new() };
    // os campos entram numa passada so: texto de terceiros (titulo do clipe) com "{nome}" dentro nao e trocado de novo
    let mut out = String::new();
    let mut resto = modelo;
    while let Some(i) = resto.find('{') {
        out += &resto[..i];
        let campos = [("{titulo}", titulo), ("{nome}", nome), ("{canal}", canal), ("{link_original}", link.as_str()), ("{hashtags}", tags.as_str())];
        match campos.iter().find(|(k, _)| resto[i..].starts_with(k)) {
            Some((k, v)) => {
                out += v;
                resto = &resto[i + k.len()..];
            }
            None => {
                out.push('{');
                resto = &resto[i + 1..];
            }
        }
    }
    out + resto
}

pub struct Estudio<'a> {
    /// (video, inicio, fim, idioma) -> (palavras com tempo, texto corrido). Tira o audio e transcreve (Whisper local).
    pub transcrever: &'a dyn Fn(&str, f64, f64, &str) -> Result<(Vec<Palavra>, String), String>,
    /// (argumentos do ffmpeg, pasta onde rodar). Ok so se o mp4 saiu inteiro.
    pub ffmpeg: &'a dyn Fn(&[String], &Path) -> Result<(), String>,
    /// Prompt -> JSON da IA LOCAL.
    pub ia_titulo: &'a dyn Fn(&str) -> Result<String, String>,
    /// (url, destino sem extensao, so audio, trecho) -> arquivo baixado (yt-dlp).
    pub baixar: &'a dyn Fn(&str, &Path, bool, Option<(f64, f64)>) -> Result<PathBuf, String>,
    /// Segundos do arquivo (ffprobe); 0 = nao deu para medir.
    pub duracao: &'a dyn Fn(&Path) -> f64,
    /// Volume por segundo do arquivo (ffmpeg + `cortes_deteccao::energia_de_pcm`).
    pub energia: &'a dyn Fn(&Path) -> Result<Vec<f64>, String>,
    /// Modo jogo: nao acorda a IA.
    pub pausado: bool,
}

#[derive(Default)]
pub struct Trecho<'a> {
    pub video: &'a str,
    pub inicio: f64,
    pub fim: f64,
    pub canal: &'a str,
    pub origem: &'a str,
    pub fonte: &'a str,
    pub score: f64,
    pub motivo: &'a str,
    pub jogo: &'a str,
    pub titulo_base: &'a str,
}

/// Corta o trecho, legenda, renderiza 9:16, gera titulo e coloca na fila. Devolve o id.
pub fn processar_corte(dados: &Path, e: &Estudio, t: &Trecho, cfg: &Value, agora: &Agora, ts: f64) -> Result<i64, String> {
    let carimbo = agora.carimbo().replace('-', "_");
    let tmp = sub(dados, "tmp").join(format!("{carimbo}_{}", t.inicio as i64));
    std::fs::create_dir_all(&tmp).map_err(|x| x.to_string())?;
    let idioma = cfg["idioma"].as_str().unwrap_or("pt");
    let modo = cfg["enquadramento"].as_str().unwrap_or("desfocado");
    let (mut texto, mut ass) = (String::new(), None);
    if cfg["legenda"].as_bool().unwrap_or(true) {
        match (e.transcrever)(t.video, t.inicio, t.fim, idioma) {
            Ok((palavras, txt)) => {
                texto = txt;
                if !palavras.is_empty() && std::fs::write(tmp.join("legenda.ass"), cortes_legenda::gerar_ass(&palavras, if modo == "centro" { 380 } else { 470 }, 80)).is_ok() {
                    ass = Some("legenda.ass");
                }
            }
            Err(x) => log(dados, agora, &format!("legenda falhou ({x}); segue sem legenda")),
        }
    }
    let nome = cfg["nome"].as_str().filter(|n| !n.is_empty()).unwrap_or(t.canal);
    let tit = cortes_titulos::gerar(&Pedido { canal: t.canal, transcricao: &texto, jogo: t.jogo, titulo_base: t.titulo_base, idioma, nome }, cfg["titulos_ollama"].as_bool().unwrap_or(true), e.pausado, e.ia_titulo);
    let fixas = cfg["hashtags_fixas"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect::<Vec<String>>()).unwrap_or_default();
    let tags: Vec<String> = sem_repetir(fixas.into_iter().chain(tit.hashtags).collect()).into_iter().take(8).collect();
    let perfil = cfg["perfil"].as_str().unwrap_or("");
    let saida = sub(dados, &if perfil.is_empty() { "saida".to_string() } else { format!("saida/{perfil}") }).join(format!("{}_{carimbo}_{}.mp4", t.canal, slug(&tit.titulo)));
    let onde = if ass.is_some() { tmp.clone() } else { saida.parent().map(Path::to_path_buf).unwrap_or_default() };
    cortes_render::renderizar(t.video, t.inicio, t.fim, &saida.to_string_lossy(), ass, modo, &|a| (e.ffmpeg)(a, &onde))?;
    let id = fila::adicionar(dados, &json!({"canal": t.canal, "origem": t.origem, "fonte": t.fonte, "inicio": t.inicio, "fim": t.fim, "score": t.score, "motivo": t.motivo,
        "arquivo": saida.to_string_lossy(), "transcricao": corta(&texto, 4000), "titulo": tit.titulo, "descricao": descricao(cfg, t.canal, nome, &tit.titulo, &tags, t.fonte),
        "hashtags": tags.join(" "), "status": if cfg["aprovacao_manual"].as_bool().unwrap_or(true) { "renderizado" } else { "aprovado" }, "perfil": perfil}), ts)?;
    log(dados, agora, &format!("corte #{id} pronto: {} ({:.0}s, {})", saida.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), t.fim - t.inicio, t.motivo));
    Ok(id)
}

/// Video local (gravacao da live): detecta os picos e gera os cortes. `chat_jsonl` = texto do chat gravado, se houver.
pub fn processar_arquivo(dados: &Path, e: &Estudio, video: &str, chat_jsonl: Option<&str>, canal: &str, origem: &str, jogo: &str, cfg: &Value, agora: &Agora, ts: f64) -> Result<Vec<i64>, String> {
    let janelas = cortes_deteccao::detectar(&(e.energia)(Path::new(video))?, chat_jsonl, cfg);
    log(dados, agora, &format!("{video}: {} momento(s) acima do limiar (chat: {})", janelas.len(), if chat_jsonl.is_some() { "sim" } else { "nao" }));
    let mut ids = vec![];
    for j in &janelas {
        let (ini, fim) = (j["inicio"].as_f64().unwrap_or(0.0), j["fim"].as_f64().unwrap_or(0.0));
        match processar_corte(dados, e, &Trecho { video, inicio: ini, fim, canal, origem, fonte: video, score: j["score"].as_f64().unwrap_or(0.0), motivo: j["motivo"].as_str().unwrap_or(""), jogo, ..Default::default() }, cfg, agora, ts) {
            Ok(id) => ids.push(id),
            Err(x) => log(dados, agora, &format!("falha no corte {ini:.0}s: {x}")),
        }
    }
    Ok(ids)
}

/// VOD da Twitch: baixa SO o audio para achar os picos e depois so os trechos escolhidos (economiza disco e banda).
pub fn processar_vod(dados: &Path, e: &Estudio, url: &str, canal: &str, chave: &str, cfg: &Value, agora: &Agora, ts: f64) -> Result<Vec<i64>, String> {
    let chave = if chave.is_empty() { url } else { chave };
    let tmp = sub(dados, "tmp").join(format!("vod_{}", slug(chave)));
    let audio = (e.baixar)(url, &tmp.join("audio"), true, None)?;
    let janelas = cortes_deteccao::detectar(&(e.energia)(&audio)?, None, cfg);
    log(dados, agora, &format!("{} momento(s) no VOD {url}", janelas.len()));
    let mut ids = vec![];
    for (n, j) in janelas.iter().enumerate() {
        let (ini, fim) = (j["inicio"].as_f64().unwrap_or(0.0), j["fim"].as_f64().unwrap_or(0.0));
        let feito = (e.baixar)(url, &tmp.join(format!("trecho{n}")), false, Some((ini, fim))).and_then(|trecho| {
            let dur = Some((e.duracao)(&trecho)).filter(|d| *d > 0.0).unwrap_or(fim - ini);
            processar_corte(dados, e, &Trecho { video: &trecho.to_string_lossy(), fim: dur, canal, origem: "vod", fonte: &format!("{url}@{ini:.0}"), score: j["score"].as_f64().unwrap_or(0.0), motivo: j["motivo"].as_str().unwrap_or(""), ..Default::default() }, cfg, agora, ts)
        });
        match feito {
            Ok(id) => ids.push(id),
            Err(x) => log(dados, agora, &format!("falha no trecho {ini:.0}s do VOD: {x}")),
        }
    }
    fila::marcar_visto(dados, &format!("vod:{chave}"), ts)?;
    Ok(ids)
}

/// Clipe ja escolhido pela comunidade (os mais vistos): so vira vertical com legenda.
pub fn processar_clipe(dados: &Path, e: &Estudio, clipe: &Value, canal: &str, cfg: &Value, agora: &Agora, ts: f64) -> Result<i64, String> {
    let id_clipe = clipe["id"].as_str().map(String::from).unwrap_or_else(|| clipe["id"].to_string());
    let url = clipe["url"].as_str().ok_or("clipe sem link")?;
    let arq = (e.baixar)(url, &sub(dados, "tmp").join(format!("clipe_{}", slug(&id_clipe))), false, None)?;
    let dur = Some((e.duracao)(&arq)).filter(|d| *d > 0.0).unwrap_or(30.0).min(cfg["duracao_max_s"].as_f64().unwrap_or(58.0));
    let views = clipe["views"].as_f64().unwrap_or(0.0);
    let titulo = clipe["titulo"].as_str().unwrap_or("");
    let id = processar_corte(dados, e, &Trecho { video: &arq.to_string_lossy(), fim: dur, canal, origem: "clipe", fonte: url, score: views, motivo: &format!("clipe da Twitch com {views:.0} views: {}", corta(titulo, 60)), titulo_base: titulo, ..Default::default() }, cfg, agora, ts)?;
    fila::marcar_visto(dados, &format!("clipe:{id_clipe}"), ts)?;
    Ok(id)
}

/// Aviso de cortes novos ao dono: um texto e, por corte, o video com legenda (arquivo de 49 MB ou mais vai so o caminho).
pub fn avisar_novos(dados: &Path, cfg: &Value, ids: &[i64], texto: &dyn Fn(&str), video: &dyn Fn(&str, &str)) {
    if ids.is_empty() || !cfg["avisar_telegram"].as_bool().unwrap_or(true) {
        return;
    }
    texto(&format!("🎬 {} corte(s) novo(s) pronto(s). Assista e diga ao Jarvis \"aprovar corte N\" ou \"rejeitar corte N\" (ou \"aprovar todos os cortes\").", ids.len()));
    for id in ids {
        let Some(c) = fila::pegar(dados, *id) else { continue };
        let arq = c["arquivo"].as_str().unwrap_or("");
        let leg = format!("#{id} · {}\n{}", c["titulo"].as_str().unwrap_or(""), c["motivo"].as_str().unwrap_or(""));
        match std::fs::metadata(arq) {
            Ok(m) if m.len() < 49 * 1024 * 1024 => video(arq, &corta(&leg, 1000)),
            _ => texto(&format!("{leg}\n(arquivo grande: {arq})")),
        }
    }
}

fn trocar(dados: &Path, ids: &[i64], de: &[&str], para: &str, ts: f64) -> Vec<i64> {
    ids.iter().copied().filter(|i| fila::pegar(dados, *i).is_some_and(|c| de.contains(&c["status"].as_str().unwrap_or(""))) && fila::atualizar(dados, *i, &json!({"status": para}), ts).is_ok()).collect()
}

/// So quem chama a pedido do dono. "erro_youtube" volta para a fila com um novo aprovar.
pub fn aprovar(dados: &Path, ids: &[i64], ts: f64) -> Vec<i64> {
    trocar(dados, ids, &["renderizado", "rejeitado", "erro_youtube"], "aprovado", ts)
}

pub fn rejeitar(dados: &Path, ids: &[i64], ts: f64) -> Vec<i64> {
    trocar(dados, ids, &["renderizado", "aprovado"], "rejeitado", ts)
}

/// Proximo horario livre da grade (youtube_horarios, hora local) dentro de youtube_agendar_dias. Horario de HOJE que ja passou e ficou
/// vazio conta tambem (sobe na hora, para a meta do dia nao furar). None = agenda cheia.
pub fn proximo_horario(cfg: &Value, ocupados: &[f64], agora: &Agora, ts: f64) -> Option<f64> {
    let mut hs: Vec<(i64, i64)> = cfg["youtube_horarios"].as_array()?.iter().filter_map(|h| {
        let (a, b) = h.as_str()?.split_once(':')?;
        Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
    }).collect();
    hs.sort();
    let zero = meia_noite(agora, ts);
    for d in 0..=cfg["youtube_agendar_dias"].as_i64().unwrap_or(2) {
        for (hh, mm) in &hs {
            let t = zero + (d * 86400 + hh * 3600 + mm * 60) as f64;
            if !ocupados.iter().any(|o| (o - t).abs() < 1.0) && (d == 0 || t > ts) {
                return Some(t);
            }
        }
    }
    None
}

pub struct Saidas<'a> {
    pub youtube: Subir<'a>,
    /// Arquivo ja na pasta do TikTok -> publish_id do rascunho (so com "tiktok_rascunho_api").
    pub rascunho_tiktok: &'a dyn Fn(&Path) -> Result<String, String>,
}

/// Aprovados -> pasta do TikTok (sempre) + YouTube (se configurado e dentro do limite). Devolve o relatorio.
pub fn publicar_aprovados(dados: &Path, cfg_geral: &Value, agora: &Agora, ts: f64, s: &Saidas) -> Vec<String> {
    let (mut rel, mut cheios, mut sem_login): (Vec<String>, Vec<String>, Vec<String>) = (vec![], vec![], vec![]);
    let par = |p: &str| if p.is_empty() { String::new() } else { format!(" ({p})") };
    for c in fila::listar(dados, Some("aprovado"), 50).iter().rev() {
        let Some(id) = c["id"].as_i64() else { continue };
        let perfil = c["perfil"].as_str().unwrap_or("");
        let cfg = do_perfil(cfg_geral, perfil);
        let conta = cfg["youtube_conta"].as_str().unwrap_or("");
        if c["tiktok"].as_str().unwrap_or("").is_empty() {
            let feito = cortes_tiktok::preparar(dados, c, perfil).and_then(|destino| {
                let estado = if cfg["tiktok_rascunho_api"].as_bool().unwrap_or(false) { format!("rascunho:{}", (s.rascunho_tiktok)(&destino)?) } else { "pasta".to_string() };
                fila::atualizar(dados, id, &json!({"tiktok": estado}), ts)?;
                Ok(if estado == "pasta" { format!("pronto em {}", destino.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()) } else { "rascunho enviado ao app".into() })
            });
            match feito {
                Ok(m) => rel.push(format!("#{id} TikTok: {m}")),
                Err(x) => {
                    let _ = fila::atualizar(dados, id, &json!({"erro": corta(&format!("tiktok: {x}"), 300)}), ts);
                    rel.push(format!("#{id} TikTok falhou: {}", corta(&x, 100)));
                }
            }
        }
        if !cortes_youtube::configurado(dados, conta) {
            if !sem_login.iter().any(|x| x == conta) {
                sem_login.push(conta.to_string());
            }
            continue;
        }
        let privacidade = cfg["youtube_privacidade"].as_str().unwrap_or("private");
        let cheio = cheios.iter().any(|x| x == perfil);
        let grade = cfg["youtube_horarios"].as_array().is_some_and(|a| !a.is_empty()) && privacidade == "public";
        let mut quando = None;
        if grade {
            quando = if cheio { None } else { proximo_horario(&cfg, &fila::horarios_ocupados(dados, perfil), agora, ts) };
            if quando.is_none() {
                if !cheio {
                    rel.push(format!("agenda do YouTube{} cheia pelos proximos {} dia(s); o resto espera na fila", par(perfil), cfg["youtube_agendar_dias"].as_i64().unwrap_or(2)));
                    cheios.push(perfil.to_string());
                }
                continue;
            }
        } else if cheio || fila::publicados_hoje(dados, perfil, meia_noite(agora, ts)) >= cfg["youtube_max_por_dia"].as_i64().unwrap_or(5) {
            if !cheio {
                rel.push(format!("limite diario do YouTube{} atingido; o resto sobe amanha", par(perfil)));
                cheios.push(perfil.to_string());
            }
            continue;
        }
        let futuro = quando.filter(|q| *q > ts + 600.0); // horario de hoje que ja passou: publica na hora
        let categoria = cfg["youtube_categoria"].as_str().map(String::from).unwrap_or_else(|| cfg["youtube_categoria"].as_i64().unwrap_or(20).to_string());
        match cortes_youtube::enviar(c, privacidade, &categoria, conta, futuro, s.youtube) {
            Ok(vid) => {
                let _ = fila::atualizar(dados, id, &json!({"status": "publicado", "youtube_id": vid, "erro": null, "agendado": quando}), ts);
                let modo = futuro.map(|f| format!("agendado p/ {}", dia_hora(f, agora, ts))).unwrap_or_else(|| privacidade.to_string());
                rel.push(format!("#{id} YouTube ({modo}): https://youtube.com/shorts/{vid}"));
            }
            Err(x) => {
                let _ = fila::atualizar(dados, id, &json!({"status": "erro_youtube", "erro": corta(&format!("youtube: {x}"), 300)}), ts);
                rel.push(format!("#{id} YouTube falhou: {} (diga 'aprovar corte {id}' para tentar de novo)", corta(&x, 120)));
            }
        }
    }
    if !rel.is_empty() {
        sem_login.sort();
        for c in sem_login {
            let (a, b) = if c.is_empty() { (String::new(), String::new()) } else { (format!(" {c}"), format!(" --perfil {c}")) };
            rel.push(format!("YouTube{a} ainda nao configurado (python -m braniac.cortes youtube-login{b}); os aprovados ficam esperando"));
        }
    }
    rel
}

#[cfg(test)]
pub(crate) fn agora_teste() -> (Agora, f64) {
    // 10/10/2026 15:20 local; o carimbo e qualquer numero redondo de minuto
    (Agora { data: Data::civil(2026, 10, 10).unwrap(), hora: 15, min: 20, seg: 0 }, 1_000_000.0 * 60.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cortes_config::padrao;
    use crate::cortes_fila::tmp;
    use std::cell::RefCell;

    fn video(d: &Path) -> String {
        std::fs::create_dir_all(d).unwrap();
        let v = d.join("v.mp4");
        std::fs::write(&v, vec![b'0'; 100]).unwrap();
        v.to_string_lossy().into_owned()
    }

    fn sem_rede<'a>() -> Saidas<'a> {
        Saidas { youtube: &|_, _, _| panic!("YouTube nao pode ser chamado"), rascunho_tiktok: &|_| panic!("rascunho nao pode ser chamado") }
    }

    #[test]
    fn aprovar_e_rejeitar_respeitam_o_estado() {
        let d = tmp("pipe-aprovar");
        let a = fila::adicionar(&d, &json!({"titulo": "A"}), 1.0).unwrap();
        let b = fila::adicionar(&d, &json!({"titulo": "B", "status": "publicado"}), 1.0).unwrap();
        assert_eq!(aprovar(&d, &[a, b, 99], 2.0), vec![a]);
        assert_eq!(aprovar(&d, &[a], 2.0), Vec::<i64>::new());
        assert_eq!(rejeitar(&d, &[a, b], 3.0), vec![a]);
        assert_eq!(fila::pegar(&d, b).unwrap()["status"], "publicado");
        fila::atualizar(&d, a, &json!({"status": "erro_youtube"}), 4.0).unwrap();
        assert_eq!((rejeitar(&d, &[a], 5.0).len(), aprovar(&d, &[a], 5.0)), (0, vec![a]));
    }

    #[test]
    fn publicar_sem_youtube_prepara_tiktok_e_nao_repete_aviso() {
        let d = tmp("pipe-sem-yt");
        let (agora, ts) = agora_teste();
        let v = video(&d);
        let a = fila::adicionar(&d, &json!({"canal": "c", "arquivo": v, "titulo": "T", "hashtags": "#shorts #x", "status": "aprovado"}), ts).unwrap();
        let g = fila::adicionar(&d, &json!({"canal": "f", "arquivo": v, "titulo": "T", "hashtags": "#shorts", "status": "aprovado", "perfil": "gringo"}), ts).unwrap();
        let espera = fila::adicionar(&d, &json!({"canal": "c", "arquivo": v, "titulo": "nao aprovado"}), ts).unwrap();
        let rel = publicar_aprovados(&d, &padrao(), &agora, ts, &sem_rede());
        assert!(rel.iter().any(|r| r.contains("TikTok: pronto em tiktok")) && d.join(format!("cortes/tiktok/corte_{a:04}.mp4")).exists());
        assert!(d.join(format!("cortes/tiktok/gringo/corte_{g:04}.mp4")).exists() && rel.iter().any(|r| r.contains("--perfil gringo")));
        assert!(!d.join(format!("cortes/tiktok/corte_{espera:04}.mp4")).exists()); // sem aprovar nao sai nem para a pasta
        assert_eq!(fila::publicados_hoje(&d, "gringo", 0.0), 0);
        assert_eq!(publicar_aprovados(&d, &padrao(), &agora, ts, &sem_rede()), Vec::<String>::new()); // segundo ciclo: nada novo
    }

    #[test]
    fn youtube_respeita_teto_diario_e_erro_sai_da_fila_automatica() {
        let d = tmp("pipe-teto");
        let (agora, ts) = agora_teste();
        let v = video(&d);
        std::fs::write(sub(&d, "").join("youtube_token.json"), "{}").unwrap();
        let mut cfg = padrao();
        cfg["youtube_max_por_dia"] = json!(2);
        let ids: Vec<i64> = (0..4).map(|i| fila::adicionar(&d, &json!({"arquivo": v, "titulo": format!("T{i}"), "status": "aprovado", "tiktok": "pasta"}), ts).unwrap()).collect();
        let subidos = RefCell::new(vec![]);
        let yt = |_: &str, corpo: &Value, _: &str| {
            let t = corpo["snippet"]["title"].as_str().unwrap_or("").to_string();
            subidos.borrow_mut().push(format!("{t}:{}", corpo["status"]["privacyStatus"].as_str().unwrap_or("")));
            if t == "T0" { Err("quota".to_string()) } else { Ok(format!("vid{t}")) }
        };
        let s = Saidas { youtube: &yt, rascunho_tiktok: &|_| Ok("p".into()) };
        let rel = publicar_aprovados(&d, &cfg, &agora, ts, &s);
        // o mais antigo primeiro; T0 falha, T1 e T2 sobem privados, T3 bate no teto
        assert_eq!(*subidos.borrow(), vec!["T0:private", "T1:private", "T2:private"]);
        assert!(rel[0].contains("YouTube falhou: quota") && rel[0].contains(&format!("aprovar corte {}", ids[0])));
        assert!(rel[1].contains("YouTube (private): https://youtube.com/shorts/vidT1") && rel[3].contains("limite diario do YouTube atingido"));
        assert_eq!((fila::pegar(&d, ids[0]).unwrap()["status"].as_str().map(String::from), fila::estoque(&d, "")), (Some("erro_youtube".into()), 1));
        subidos.borrow_mut().clear();
        assert_eq!(publicar_aprovados(&d, &cfg, &agora, ts, &s).len(), 1); // so o aviso do teto; o que deu erro nao tenta de novo
        assert!(subidos.borrow().is_empty());
    }

    #[test]
    fn grade_de_horarios() {
        let (agora, ts) = agora_teste();
        let cfg = json!({"youtube_horarios": ["19:00", "12:00"], "youtube_agendar_dias": 1});
        let (mut oc, mut vistos) = (vec![], vec![]);
        while let Some(t) = proximo_horario(&cfg, &oc, &agora, ts) {
            vistos.push(dia_hora(t, &agora, ts));
            oc.push(t + 0.4); // o carimbo guardado pode vir com fracao
        }
        // horario de hoje que ja passou ainda conta (sobe na hora); amanha so os futuros; depois disso a agenda fecha
        assert_eq!(vistos, vec!["10/10 12:00", "10/10 19:00", "11/10 12:00", "11/10 19:00"]);
        assert_eq!(proximo_horario(&json!({"youtube_horarios": []}), &[], &agora, ts), None);
        assert_eq!(proximo_horario(&json!({"youtube_horarios": ["meio-dia"]}), &[], &agora, ts), None);
    }

    #[test]
    fn grade_so_agenda_perfil_publico_e_agendado_sobe_privado() {
        let d = tmp("pipe-grade");
        let (agora, ts) = agora_teste();
        let v = video(&d);
        std::fs::write(sub(&d, "").join("youtube_token.json"), "{}").unwrap();
        let mut cfg = padrao();
        cfg["youtube_horarios"] = json!(["12:00", "19:00"]);
        cfg["youtube_agendar_dias"] = json!(0);
        cfg["youtube_privacidade"] = json!("public");
        for i in 0..3 {
            fila::adicionar(&d, &json!({"arquivo": v, "titulo": format!("T{i}"), "status": "aprovado", "tiktok": "pasta"}), ts).unwrap();
        }
        let corpos = RefCell::new(vec![]);
        let yt = |_: &str, corpo: &Value, _: &str| {
            corpos.borrow_mut().push(corpo["status"].clone());
            Ok("v".to_string())
        };
        let rel = publicar_aprovados(&d, &cfg, &agora, ts, &Saidas { youtube: &yt, rascunho_tiktok: &|_| Ok("p".into()) });
        // 12:00 de hoje ja passou: sobe na hora (publico); 19:00: privado com publishAt; o terceiro espera
        assert_eq!(corpos.borrow().len(), 2);
        assert_eq!((corpos.borrow()[0]["privacyStatus"].as_str().map(String::from), corpos.borrow()[0].get("publishAt").cloned()), (Some("public".into()), None));
        assert_eq!(corpos.borrow()[1]["privacyStatus"], "private");
        assert!(corpos.borrow()[1]["publishAt"].is_string() && rel[1].contains("agendado p/ 10/10 19:00") && rel[2].contains("agenda do YouTube cheia"));
        assert_eq!(fila::horarios_ocupados(&d, "").len(), 2);
    }

    #[test]
    fn descricao_slug_e_tempo() {
        let cfg = crate::cortes_config::para_canal(&json!({"login": "fulano", "nome": "Fulano", "perfil": "gringo"}), &padrao());
        let dsc = descricao(&cfg, "fulano", "Fulano", "T {nome} {x}", &["#shorts".into()], "https://clips.twitch.tv/abc");
        assert!(dsc.contains("twitch.tv/fulano") && dsc.contains("Original clip: https://clips.twitch.tv/abc") && dsc.contains("credit") && dsc.starts_with("T {nome} {x}\n\n"));
        assert_eq!(descricao(&json!({}), "c", "C", "T", &["#a".into(), "#b".into()], "arquivo.ts"), "T\n\nCorte da live de twitch.tv/c\n\n#a #b");
        assert!(!descricao(&cfg, "f", "F", "T", &[], "v.ts").contains("Original clip"));
        assert_eq!((slug("Olá, Mundo!! 2"), slug("???"), slug(&"a".repeat(60)).len()), ("ol_mundo_2".to_string(), "corte".to_string(), 40));
        let (agora, ts) = agora_teste();
        assert_eq!((dia_hora(ts, &agora, ts), dia_hora(ts + 86400.0 * 22.0, &agora, ts)), ("10/10 15:20".to_string(), "01/11 15:20".to_string()));
    }

    #[test]
    fn corte_entra_na_fila_esperando_aprovacao_e_avisa() {
        let d = tmp("pipe-corte");
        let (agora, ts) = agora_teste();
        let rodou = RefCell::new(vec![]);
        let ffmpeg = |a: &[String], onde: &Path| {
            rodou.borrow_mut().push((a.contains(&"h264_nvenc".to_string()), a.iter().any(|x| x.ends_with("ass=legenda.ass[v]")), onde.join("legenda.ass").exists()));
            std::fs::write(a.last().unwrap(), "mp4").map_err(|e| e.to_string())
        };
        let e = Estudio {
            transcrever: &|_, _, _, idioma| {
                assert_eq!(idioma, "pt");
                Ok((vec![(0.0, 0.4, "que".to_string()), (0.4, 0.9, "jogada".to_string())], "que jogada foi essa meu amigo".to_string()))
            },
            ffmpeg: &ffmpeg,
            ia_titulo: &|_| Err("sem IA".into()),
            baixar: &|_, dest, _, _| {
                let p = dest.with_extension("mp4");
                std::fs::create_dir_all(dest.parent().unwrap()).map_err(|e| e.to_string())?;
                std::fs::write(&p, "x").map_err(|e| e.to_string())?;
                Ok(p)
            },
            duracao: &|_| 0.0,
            energia: &|_| Ok(crate::cortes_deteccao::audio_teste(600, &[300])),
            pausado: false,
        };
        let cfg = crate::cortes_config::para_canal(&json!({"login": "ana"}), &padrao());
        let id = processar_corte(&d, &e, &Trecho { video: "live.ts", inicio: 100.0, fim: 130.0, canal: "ana", origem: "live", fonte: "live.ts", motivo: "audio +5.0σ", ..Default::default() }, &cfg, &agora, ts).unwrap();
        let c = fila::pegar(&d, id).unwrap();
        assert_eq!((c["status"].as_str(), c["titulo"].as_str(), c["hashtags"].as_str()), (Some("renderizado"), Some("Que jogada foi essa meu amigo"), Some("#shorts #twitch #ana #cortes")));
        assert!(c["arquivo"].as_str().unwrap().ends_with("ana_20261010_152000_que_jogada_foi_essa_meu_amigo.mp4"));
        assert_eq!(*rodou.borrow(), vec![(true, true, true)]);
        // live inteira: um pico -> um corte; clipe: marca visto; modo automatico (sem aprovacao manual) ja entra aprovado
        let ids = processar_arquivo(&d, &e, "live.ts", None, "ana", "live", "", &cfg, &agora, ts).unwrap();
        assert_eq!(ids.len(), 1);
        let mut auto = cfg.clone();
        auto["aprovacao_manual"] = json!(false);
        auto["legenda"] = json!(false);
        let idc = processar_clipe(&d, &e, &json!({"id": "Clipe-9", "url": "https://clips.twitch.tv/x", "views": 1234, "titulo": "virada absurda"}), "ana", &auto, &agora, ts).unwrap();
        let cc = fila::pegar(&d, idc).unwrap();
        assert_eq!((cc["status"].as_str(), cc["fim"].as_f64(), cc["titulo"].as_str(), cc["motivo"].as_str()), (Some("aprovado"), Some(30.0), Some("virada absurda | ana"), Some("clipe da Twitch com 1234 views: virada absurda")));
        assert!(fila::ja_visto(&d, "clipe:Clipe-9"));
        assert_eq!(processar_vod(&d, &e, "https://www.twitch.tv/videos/1", "ana", "1", &auto, &agora, ts).unwrap().len(), 1);
        assert!(fila::ja_visto(&d, "vod:1"));
        let avisos = RefCell::new(vec![]);
        avisar_novos(&d, &cfg, &[id, 999], &|t| avisos.borrow_mut().push(format!("T:{}", corta(t, 12))), &|a, leg| avisos.borrow_mut().push(format!("V:{}:{}", a.ends_with(".mp4"), leg.starts_with(&format!("#{id} · Que jogada")))));
        assert_eq!(*avisos.borrow(), vec!["T:🎬 2 corte(s)".to_string(), "V:true:true".to_string()]);
        avisos.borrow_mut().clear();
        avisar_novos(&d, &json!({"avisar_telegram": false}), &[id], &|_| panic!(), &|_, _| panic!());
        // ffmpeg quebrado: nada entra na fila
        let ruim = Estudio { ffmpeg: &|_, _| Err("sem ffmpeg".into()), ..e };
        let antes = fila::listar(&d, None, 100).len();
        assert!(processar_corte(&d, &ruim, &Trecho { video: "v", fim: 30.0, canal: "ana", ..Default::default() }, &cfg, &agora, ts).is_err());
        assert_eq!(fila::listar(&d, None, 100).len(), antes);
    }
}
