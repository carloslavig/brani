//! Claquete, a linha de cortes de video: pedidos em dados/empresa_cortes/ (equipe.json, pedidos.json, uso.json, fila_publicacao.json, fontes_vistas.json).
//! Fluxo: novo -> transcrito -> cortado -> montado (9:16) -> legendado (legenda + credito da fonte) -> revisado (Lupa) -> aguardando_ok.
//! Travas: sem autor nao abre (o corte sempre cita a fonte); pedido com palavra de segredo e recusado; nada entra na fila de publicacao sem
//! `aprovar` (pedido do dono), e `aprovar` exige `direitos_ok` e `gameplay_ok` no equipe.json e o locutor confirmado; tetos de passos e de pedidos
//! por dia; so das 7h as 21h. Este modulo NAO publica: `publicar_vencidos` so chama a closure `subir` que quem integra passar, para item ja
//! aprovado e agendado, um por vez, e so e chamado pelo `tick` com `tiktok_auto` ligado no equipe.json.
//! Ficou de fora (closures em `Estudio`): IA, yt-dlp, Whisper, ffmpeg/ffprobe e o envio pelo navegador.
//! Diferenca do Python: o nome de quem aparece nas fontes vem de `busca.nome` no equipe.json (la estava fixo no codigo); sem ele a busca nao acha nada.
#![allow(dead_code)]
use crate::empresa_dev::{corta, gravar, ler, limite_de, lista, rx, segredo, somar_uso, uso};
use crate::tempo::{Agora, Data};
use regex::Regex;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const PASSOS: [&str; 6] = ["novo", "transcrito", "cortado", "montado", "legendado", "revisado"];
const EVITAR_PADRAO: [&str; 22] = ["resposta", "reação", "reacao", "react", "destrói", "destroi", "humilha", "cortes", "melhores momentos", "perdeu a paciência", "perdeu a paciencia",
    "perde a paciência", "briga", "bate-boca", "bate boca", "discute", "detona", "desmonta", "expõe", "expoe", " vs ", "versus"];

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}

fn pasta(dados: &Path) -> PathBuf {
    dados.join("empresa_cortes")
}

fn caminho(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

pub fn config(dados: &Path) -> Value {
    ler(&pasta(dados).join("equipe.json")).filter(Value::is_object).unwrap_or_else(|| json!({ "empresa": "Claquete", "agentes": [], "limites": {} }))
}

/// A equipe do equipe.json mais os tres do estudio 3D, que moram no mesmo andar.
pub fn agentes(dados: &Path) -> Vec<Value> {
    let mut base = config(dados)["agentes"].as_array().cloned().unwrap_or_default();
    for (nome, papel, instrucao) in [("Pena", "Roteiro do estúdio 3D", "Você é a Pena: escreve a narração curta e as cenas do vídeo explicativo. Só conceitos conhecidos; nunca inventa número, data ou fonte."),
        ("Eco", "Voz e sincronia", "Você é o Eco: gera a narração com a voz do Braniac e sincroniza a legenda com o Whisper local."),
        ("Prisma", "Cenas 3D e render", "Você é o Prisma: monta as cenas 3D (barras, globo, pilha) nos tempos da fala e renderiza o vídeo vertical 1080x1920.")] {
        if !base.iter().any(|a| a["nome"] == nome) {
            base.push(json!({ "nome": nome, "papel": papel, "sala": "Estúdio", "instrucao": instrucao }));
        }
    }
    base
}

pub fn nomes(dados: &Path) -> Vec<String> {
    agentes(dados).iter().map(|a| s(a, "nome").to_string()).collect()
}

pub fn limite(dados: &Path, chave: &str, padrao: i64) -> i64 {
    limite_de(&config(dados), chave, padrao)
}

pub fn pedidos(dados: &Path) -> Vec<Value> {
    lista(&pasta(dados).join("pedidos.json"))
}

fn anotar(p: &mut Value, texto: &str, ts: f64) {
    if !p["historico"].is_array() {
        p["historico"] = json!([]);
    }
    if let Some(h) = p["historico"].as_array_mut() {
        h.push(json!({ "ts": ts, "texto": corta(texto, 300) }));
    }
}

fn gravar_pedidos(dados: &Path, ps: &[Value]) -> Result<(), String> {
    gravar(&pasta(dados).join("pedidos.json"), &json!(ps))
}

/// ffmpeg da montagem 9:16 (1080x1920): fala em cima, gameplay embaixo, audio da fala. So monta os argumentos.
pub fn args_montar(fala: &str, gameplay: &str, ini: f64, dur: f64, destino: &str) -> Vec<String> {
    let f = "scale=1080:960:force_original_aspect_ratio=increase,crop=1080:960";
    let (ini, dur, filtro) = (ini.to_string(), dur.to_string(), format!("[0:v]{f}[a];[1:v]{f}[b];[a][b]vstack=inputs=2[v]"));
    ["ffmpeg", "-y", "-hide_banner", "-loglevel", "error", "-ss", &ini, "-t", &dur, "-i", fala, "-stream_loop", "-1", "-t", &dur, "-i", gameplay, "-filter_complex", &filtro,
        "-map", "[v]", "-map", "0:a", "-c:v", "libx264", "-preset", "veryfast", "-crf", "23", "-c:a", "aac", "-shortest", destino].map(String::from).to_vec()
}

/// ffmpeg que queima a legenda no video (nomes de arquivo relativos a pasta do pedido). So monta os argumentos.
pub fn args_queimar(base: &str, srt: &str, destino: &str) -> Vec<String> {
    let filtro = format!("subtitles={srt}:force_style='FontSize=9,Alignment=2,MarginV=150,Outline=2,Bold=1'");
    ["ffmpeg", "-y", "-hide_banner", "-loglevel", "error", "-i", base, "-vf", &filtro, "-c:a", "copy", destino].map(String::from).to_vec()
}

/// O que a Claquete faz fora deste modulo. Quem chama decide como.
pub struct Estudio<'a> {
    /// IA gratis: (sistema, prompt) -> texto.
    pub ia: &'a dyn Fn(&str, &str) -> Result<String, String>,
    /// Baixa a fonte quando ela e URL: (url, destino) -> caminho local.
    pub baixar: &'a dyn Fn(&str, &Path) -> Result<String, String>,
    /// Transcricao local com tempos: video -> [{ini, fim, texto}].
    pub segmentos: &'a dyn Fn(&str) -> Result<Vec<Value>, String>,
    /// Monta o 9:16: (fala, gameplay, inicio, duracao, destino).
    pub montar: &'a dyn Fn(&str, &str, f64, f64, &Path) -> Result<(), String>,
    /// Queima a legenda: (base, srt, destino).
    pub queimar: &'a dyn Fn(&Path, &Path, &Path) -> Result<(), String>,
    /// Sonda do video pronto: {duracao, largura, altura, audio, volume_db}.
    pub sonda: &'a dyn Fn(&str) -> Result<Value, String>,
    /// Busca de videos longos: consulta -> linhas "id|titulo|duracao em s|canal".
    pub buscar: &'a dyn Fn(&str) -> Vec<String>,
}

/// Novo pedido. `fonte` = arquivo local ou URL do video longo; autor e titulo sao citados na legenda; `gameplay` = video de fundo local.
pub fn abrir(dados: &Path, fonte: &str, autor: &str, titulo_fonte: &str, gameplay: &str, prova: bool, agora: &Agora, ts: f64) -> Result<(Value, String), String> {
    if segredo(&format!("{fonte} {autor} {titulo_fonte}")) {
        return Err("Pedido recusado: tem palavra de segredo/dado pessoal.".into());
    }
    if autor.trim().is_empty() {
        return Err("Sem autor não abro: o corte sempre cita a fonte.".into());
    }
    let cfg = config(dados);
    let gameplay = if gameplay.is_empty() { s(&cfg, "gameplay_padrao") } else { gameplay };
    if gameplay.is_empty() || !Path::new(gameplay).exists() {
        return Err("Falta o vídeo de gameplay (arquivo local). Origem e permissão ainda estão em aberto com o dono.".into());
    }
    if !(fonte.starts_with("http://") || fonte.starts_with("https://") || Path::new(fonte).exists()) {
        return Err("Fonte não encontrada (passe um arquivo local ou URL).".into());
    }
    let (dir, hoje) = (pasta(dados), agora.data.iso());
    if uso(&dir, &hoje, "pedidos") >= limite(dados, "pedidos_dia", 3) {
        return Err("Teto de pedidos de hoje atingido.".into());
    }
    let mut ps = pedidos(dados);
    let mut p = json!({ "id": format!("V{:03}", ps.len() + 1), "fonte": fonte, "autor": corta(autor.trim(), 80), "titulo_fonte": corta(titulo_fonte, 120), "gameplay": gameplay, "prova": prova,
        "estado": "novo", "historico": [], "criado": ts });
    anotar(&mut p, "pedido aberto", ts);
    ps.push(p.clone());
    gravar_pedidos(dados, &ps)?;
    somar_uso(&dir, &hoje, "pedidos")?;
    let msg = format!("{} aberto (fonte: {}).", s(&p, "id"), autor.trim());
    Ok((p, msg))
}

fn tempo_srt(x: f64) -> String {
    let x = x.max(0.0);
    let (h, r) = ((x / 3600.0).floor(), x % 3600.0);
    let (m, seg) = ((r / 60.0).floor(), r % 60.0);
    format!("{:02}:{:02}:{:02},{:03}", h as i64, m as i64, seg as i64, ((seg % 1.0) * 1000.0).round() as i64)
}

/// Legenda do trecho, com os tempos contados do inicio do corte, e o credito da fonte nos 3 ultimos segundos.
pub fn srt(segs: &[Value], ini: f64, fim: f64, credito: &str) -> String {
    let t = |v: &Value, k: &str| v[k].as_f64().unwrap_or(0.0);
    let mut linhas: Vec<String> = segs.iter().filter(|x| !(t(x, "fim") <= ini || t(x, "ini") >= fim)).enumerate()
        .map(|(i, x)| format!("{}\n{} --> {}\n{}\n", i + 1, tempo_srt(t(x, "ini").max(ini) - ini), tempo_srt(t(x, "fim").min(fim) - ini), s(x, "texto"))).collect();
    let dur = fim - ini;
    linhas.push(format!("{}\n{} --> {}\n{credito}\n", linhas.len() + 1, tempo_srt((dur - 3.0).max(0.0)), tempo_srt(dur)));
    linhas.join("\n")
}

/// Mina: a IA aponta o trecho; sem resposta valida, a janela mais densa em palavras dentro dos limites de duracao.
pub fn escolher(segs: &[Value], lo: f64, hi: f64, ia: &dyn Fn(&str, &str) -> Result<String, String>) -> Option<(f64, f64)> {
    static R: OnceLock<Regex> = OnceLock::new();
    let t = |v: &Value, k: &str| v[k].as_f64().unwrap_or(0.0);
    let texto = corta(&segs.iter().enumerate().map(|(i, x)| format!("{i}|{}-{}|{}", x["ini"], x["fim"], s(x, "texto"))).collect::<Vec<_>>().join("\n"), 6000);
    let resposta = ia("Você escolhe o trecho mais marcante e autossuficiente de uma fala para um corte vertical curto. Responda só JSON: {\"ini\": segundos, \"fim\": segundos}.",
        &format!("Duração entre {lo} e {hi} segundos. Segmentos (índice|início-fim|texto):\n{texto}")).unwrap_or_default();
    let apontado = rx(&R, r"(?s)\{.*\}").find(&resposta).and_then(|m| serde_json::from_str::<Value>(m.as_str()).ok()).and_then(|d| Some((d["ini"].as_f64()?, d["fim"].as_f64()?)));
    if let Some((ini, fim)) = apontado.filter(|(ini, fim)| (lo..=hi).contains(&(fim - ini)) && segs.iter().any(|x| t(x, "ini") <= *ini && *ini < t(x, "fim") + 1.0)) {
        return Some((ini, fim));
    }
    let mut melhor: Option<(usize, f64, f64)> = None;
    for (i, a) in segs.iter().enumerate() {
        let (mut fim, mut pal) = (t(a, "ini"), 0);
        for b in &segs[i..] {
            if t(b, "fim") - t(a, "ini") > hi {
                break;
            }
            fim = t(b, "fim");
            pal += s(b, "texto").split_whitespace().count();
        }
        if fim - t(a, "ini") >= lo && melhor.map_or(true, |m| pal > m.0) {
            melhor = Some((pal, t(a, "ini"), fim));
        }
    }
    melhor.map(|m| (m.1, m.2))
}

/// Fontes novas: videos longos em que a pessoa de `busca.nome` aparece, no canal dela ou em canal aprovado (`busca.canais_ok`), ainda nao usados.
/// Fora: respostas e reacoes de terceiros (edicao hostil), videos curtos demais ou longos demais. Devolve (url, titulo, canal, duracao).
pub fn buscar_fontes(dados: &Path, buscar: &dyn Fn(&str) -> Vec<String>, qtd: usize) -> Vec<(String, String, String, f64)> {
    let busca = &config(dados)["busca"];
    let nome = s(busca, "nome").to_lowercase();
    if nome.is_empty() {
        return vec![];
    }
    let textos = |v: &Value| -> Vec<String> { v.as_array().into_iter().flatten().filter_map(|x| x.as_str().map(str::to_lowercase)).collect() };
    let mut usados: Vec<String> = pedidos(dados).iter().map(|p| s(p, "fonte").to_string()).collect();
    usados.extend(lista(&pasta(dados).join("fontes_vistas.json")).iter().filter_map(|x| x.as_str().map(String::from)));
    let evitar = if busca["evitar"].is_array() { textos(&busca["evitar"]) } else { EVITAR_PADRAO.map(String::from).to_vec() };
    let canais_ok = textos(&busca["canais_ok"]);
    let consultas = if busca["consultas"].is_array() { textos(&busca["consultas"]) } else { ["entrevista", "podcast", "live", "debate"].map(|c| format!("{nome} {c}")).to_vec() };
    let (min_s, max_s) = (busca["min_s"].as_f64().unwrap_or(600.0), busca["max_s"].as_f64().unwrap_or(14400.0));
    let mut achados: Vec<(String, String, String, f64)> = vec![];
    for q in consultas {
        for linha in buscar(&q) {
            let mut c = linha.split('|');
            let (vid, titulo, dur, canal) = (c.next().unwrap_or(""), c.next().unwrap_or(""), c.next().unwrap_or(""), c.next().unwrap_or(""));
            let Ok(dur) = dur.trim().parse::<f64>() else { continue };
            let url = format!("https://www.youtube.com/watch?v={vid}");
            let (tl, cl) = (titulo.to_lowercase(), canal.to_lowercase());
            let ruim = evitar.iter().any(|w| tl.contains(w));
            let canal_ok = cl.contains(&nome) || canais_ok.iter().any(|c| cl.contains(c));
            if !ruim && canal_ok && !usados.contains(&url) && format!("{tl} {cl}").contains(&nome) && (min_s..=max_s).contains(&dur) && !achados.iter().any(|a| a.0 == url) {
                achados.push((url, titulo.to_string(), canal.to_string(), dur));
            }
        }
    }
    achados.truncate(qtd);
    achados
}

/// Abre o proximo pedido com a fonte nova mais curta (a transcricao local termina antes). A fonte fica marcada como vista.
pub fn abrir_da_busca(dados: &Path, f: &Estudio, agora: &Agora, ts: f64) -> Result<(Value, String), String> {
    let mut fontes = buscar_fontes(dados, f.buscar, 5);
    fontes.sort_by(|a, b| a.3.partial_cmp(&b.3).unwrap_or(std::cmp::Ordering::Equal));
    let (url, titulo, canal, _) = fontes.into_iter().next().ok_or("Nenhuma fonte nova na busca.")?;
    let arq = pasta(dados).join("fontes_vistas.json");
    let mut vistas = lista(&arq);
    vistas.push(json!(url));
    gravar(&arq, &json!(vistas))?;
    let autor = s(&config(dados)["busca"], "nome").to_string();
    abrir(dados, &url, &autor, &format!("{titulo} ({canal})"), "", false, agora, ts)
}

fn passo(dados: &Path, p: &mut Value, f: &Estudio) -> Result<String, String> {
    let id = s(p, "id").to_string();
    let dir = pasta(dados).join("saida").join(&id);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let t = |v: &Value, k: &str| v[k].as_f64().unwrap_or(0.0);
    match s(p, "estado") {
        "novo" => {
            let fonte = s(p, "fonte").to_string();
            let fala = if fonte.starts_with("http") { (f.baixar)(&fonte, &dir.join("fonte.mp4"))? } else { fonte };
            let segs = (f.segmentos)(&fala)?;
            if segs.is_empty() {
                return Err("sem fala reconhecida na fonte".into());
            }
            p["fala"] = json!(fala);
            p["segmentos"] = json!(segs);
            p["estado"] = json!("transcrito");
        }
        "transcrito" => {
            let segs = p["segmentos"].as_array().cloned().unwrap_or_default();
            let (ini, fim) = escolher(&segs, limite(dados, "duracao_min_s", 20) as f64, limite(dados, "duracao_max_s", 45) as f64, f.ia).ok_or("nenhum trecho dentro dos limites de duração")?;
            let texto: Vec<&str> = segs.iter().filter(|x| ini <= t(x, "ini") && t(x, "ini") < fim).map(|x| s(x, "texto")).collect();
            p["trecho"] = json!({ "ini": ini, "fim": fim, "texto": corta(&texto.join(" "), 400) });
            p["estado"] = json!("cortado");
        }
        "cortado" => {
            let (ini, fim) = (t(&p["trecho"], "ini"), t(&p["trecho"], "fim"));
            (f.montar)(s(p, "fala"), s(p, "gameplay"), ini, ((fim - ini) * 100.0).round() / 100.0, &dir.join("base.mp4"))?;
            p["arquivo"] = json!(caminho(&dir.join("base.mp4")));
            p["estado"] = json!("montado");
        }
        "montado" => {
            let cred = format!("Fonte: {}{}", s(p, "autor"), if s(p, "titulo_fonte").is_empty() { String::new() } else { format!(" - {}", s(p, "titulo_fonte")) });
            let legenda = srt(p["segmentos"].as_array().map_or(&[][..], Vec::as_slice), t(&p["trecho"], "ini"), t(&p["trecho"], "fim"), &cred);
            std::fs::write(dir.join("leg.srt"), legenda).map_err(|e| e.to_string())?;
            (f.queimar)(&dir.join("base.mp4"), &dir.join("leg.srt"), &dir.join("final.mp4"))?;
            p["arquivo"] = json!(caminho(&dir.join("final.mp4")));
            p["estado"] = json!("legendado");
        }
        "legendado" => {
            let sd = (f.sonda)(s(p, "arquivo"))?;
            let (dur, lo, hi) = (t(&sd, "duracao"), limite(dados, "duracao_min_s", 20) as f64, limite(dados, "duracao_max_s", 45) as f64);
            let mut erros = vec![];
            if !(lo - 1.0..=hi + 1.0).contains(&dur) {
                erros.push(format!("duração {dur:.0}s fora do limite"));
            }
            if (sd["largura"].as_i64(), sd["altura"].as_i64()) != (Some(1080), Some(1920)) {
                erros.push(format!("formato {}x{} (esperado 1080x1920)", sd["largura"], sd["altura"]));
            }
            if sd["audio"] != true || sd["volume_db"].as_f64().is_some_and(|v| v < -50.0) {
                erros.push("sem áudio audível".into());
            }
            if !erros.is_empty() {
                p["estado"] = json!("reprovado");
                return Ok(format!("{id} reprovado pela Lupa: {}", erros.join("; ")));
            }
            p["estado"] = json!("revisado");
        }
        "revisado" => {
            p["estado"] = json!("aguardando_ok");
            return Ok(format!("{id} pronto em {}, aguardando o OK do dono (confira se a voz é de {}).", s(p, "arquivo"), s(p, "autor")));
        }
        _ => {}
    }
    Ok(format!("{id} → {}", s(p, "estado")))
}

/// Avanca ate `n` pedidos um estado cada, dentro do teto diario de passos. Erro = pedido "falhou".
pub fn trabalhar(dados: &Path, f: &Estudio, agora: &Agora, ts: f64, n: usize, ignorar_teto: bool) -> Vec<String> {
    let (dir, hoje) = (pasta(dados), agora.data.iso());
    let mut ps = pedidos(dados);
    let mut feitos = vec![];
    for i in 0..ps.len() {
        if feitos.len() >= n {
            break;
        }
        if !PASSOS.contains(&s(&ps[i], "estado")) {
            continue;
        }
        if !ignorar_teto && uso(&dir, &hoje, "passos") >= limite(dados, "passos_dia", 12) {
            feitos.push("Teto de passos de hoje atingido.".to_string());
            break;
        }
        let txt = passo(dados, &mut ps[i], f).unwrap_or_else(|e| {
            ps[i]["estado"] = json!("falhou");
            format!("{} erro: {}", s(&ps[i], "id"), corta(&e, 160))
        });
        anotar(&mut ps[i], &txt, ts);
        feitos.push(txt);
        if somar_uso(&dir, &hoje, "passos").is_err() || gravar_pedidos(dados, &ps).is_err() {
            break; // sem gravar, o proximo ciclo repetiria o passo fora do teto
        }
    }
    feitos
}

fn hora_cheia(agora: &Agora) -> String {
    format!("{} {}", agora.data.iso(), agora.hm())
}

/// OK do dono: agenda o corte no primeiro horario livre e deixa uma copia na pasta do Kwai. So agenda; quem sobe e outra etapa.
/// Recusa enquanto faltarem os direitos dos trechos, a permissao da gameplay ou a confirmacao de quem fala.
pub fn aprovar(dados: &Path, id: &str, pedido_do_dono: bool, agora: &Agora, ts: f64) -> String {
    if !pedido_do_dono {
        return "Só o dono aprova um corte.".into();
    }
    let cfg = config(dados);
    if !(cfg["direitos_ok"] == true && cfg["gameplay_ok"] == true) {
        return "Ainda não: faltam o OK dos direitos dos trechos e a origem/permissão da gameplay (equipe.json: direitos_ok, gameplay_ok).".into();
    }
    let mut ps = pedidos(dados);
    let Some(p) = ps.iter_mut().find(|x| s(x, "id") == id.trim().to_uppercase() && x["estado"] == "aguardando_ok") else { return "Pedido não encontrado ou não está aguardando OK.".into() };
    let pid = s(p, "id").to_string();
    if p["locutor_ok"] != true {
        return format!("{pid}: confirme antes que a fala é mesmo de {} (a transcrição não identifica quem fala; debates têm outras pessoas). Use confirmar_locutor.", s(p, "autor"));
    }
    let arq_fila = pasta(dados).join("fila_publicacao.json");
    let mut fila = lista(&arq_fila);
    let mut horarios: Vec<String> = cfg["tiktok_horarios"].as_array().into_iter().flatten().filter_map(|h| h.as_str().map(String::from)).collect();
    if horarios.is_empty() {
        horarios = vec!["09:00".into(), "20:00".into()];
    }
    let livre = |c: &String| !fila.iter().any(|x| x["plataforma"] == "tiktok" && x["quando"] == c.as_str()) && *c > hora_cheia(agora);
    // um ano de horarios ocupados nao existe; o limite so impede laco sem fim com arquivo estragado
    let Some(quando) = (0..366).flat_map(|d| horarios.iter().map(move |h| format!("{} {h}", Data(agora.data.0 + d).iso()))).find(livre) else { return format!("{pid}: não achei horário livre para agendar.") };
    let kwai = pasta(dados).join("saida").join("kwai");
    if let Err(e) = std::fs::create_dir_all(&kwai).and_then(|_| std::fs::copy(s(p, "arquivo"), kwai.join(format!("{pid}.mp4")))) {
        return format!("{pid}: não consegui copiar o vídeo ({e}); nada foi agendado.");
    }
    fila.push(json!({ "id": pid, "plataforma": "tiktok", "quando": quando, "arquivo": p["arquivo"], "estado": "agendado" }));
    let ritmo = !cfg["youtube_ritmo"].is_null() && cfg["youtube_ritmo"] != false && cfg["youtube_ritmo"] != "";
    if ritmo {
        fila.push(json!({ "id": pid, "plataforma": "youtube", "quando": null, "arquivo": p["arquivo"], "estado": format!("ritmo {}", cfg["youtube_ritmo"].as_str().map_or_else(|| cfg["youtube_ritmo"].to_string(), String::from)) }));
    }
    p["estado"] = json!("aprovado");
    anotar(p, &format!("aprovado pelo dono; TikTok {quando}"), ts);
    if let Err(e) = gravar(&arq_fila, &json!(fila)).and_then(|_| gravar_pedidos(dados, &ps)) {
        return e;
    }
    format!("{pid} aprovado: TikTok em {quando}; cópia na pasta do Kwai (você sobe). YouTube: {}.", if ritmo { "agendado pelo ritmo" } else { "espera o ritmo do conselho" })
}

/// O dono assistiu ao corte e confirma que a voz e do autor citado.
pub fn confirmar_locutor(dados: &Path, id: &str, pedido_do_dono: bool, ts: f64) -> String {
    if !pedido_do_dono {
        return "Só o dono confirma quem fala no corte.".into();
    }
    let mut ps = pedidos(dados);
    let Some(p) = ps.iter_mut().find(|x| s(x, "id") == id.trim().to_uppercase()) else { return "Pedido não encontrado.".into() };
    p["locutor_ok"] = json!(true);
    anotar(p, "locutor confirmado pelo dono", ts);
    let pid = s(p, "id").to_string();
    gravar_pedidos(dados, &ps).err().unwrap_or_else(|| format!("{pid}: locutor confirmado."))
}

pub fn rejeitar(dados: &Path, id: &str, ts: f64) -> String {
    let mut ps = pedidos(dados);
    let Some(p) = ps.iter_mut().find(|x| s(x, "id") == id.trim().to_uppercase() && ["aguardando_ok", "reprovado", "falhou"].contains(&s(x, "estado"))) else { return "Pedido não encontrado.".into() };
    p["estado"] = json!("rejeitado");
    anotar(p, "rejeitado", ts);
    let pid = s(p, "id").to_string();
    gravar_pedidos(dados, &ps).err().unwrap_or_else(|| format!("{pid} rejeitado."))
}

/// Entrega a closure `subir` (arquivo, legenda) -> "" se subiu ou o motivo do erro, UM item agendado cujo horario chegou.
/// So entra na fila o que o dono aprovou. O item vira "enviando" antes da chamada, para nao repetir se travar no meio.
pub fn publicar_vencidos(dados: &Path, agora: &Agora, subir: &dyn Fn(&str, &str) -> String, enviar: &dyn Fn(&str)) -> Option<String> {
    let arq = pasta(dados).join("fila_publicacao.json");
    let mut fila = lista(&arq);
    let i = fila.iter().position(|x| x["plataforma"] == "tiktok" && x["estado"] == "agendado" && s(x, "quando") <= hora_cheia(agora).as_str())?;
    let (id, arquivo) = (s(&fila[i], "id").to_string(), s(&fila[i], "arquivo").to_string());
    let p = pedidos(dados).into_iter().find(|x| x["id"] == id.as_str()).unwrap_or_default();
    let legenda = format!("Fonte: {}{} #cortes #naooficial", s(&p, "autor"), if s(&p, "titulo_fonte").is_empty() { String::new() } else { format!(" - {}", s(&p, "titulo_fonte")) });
    fila[i]["estado"] = json!("enviando");
    gravar(&arq, &json!(fila)).ok()?;
    let erro = corta(&subir(&arquivo, &corta(&legenda, 300)), 120);
    let mut fila = lista(&arq);
    for x in fila.iter_mut().filter(|x| x["id"] == id.as_str() && x["plataforma"] == "tiktok" && x["estado"] == "enviando") {
        x["estado"] = json!(if erro.is_empty() { "publicado".to_string() } else { format!("erro: {erro}") });
    }
    let _ = gravar(&arq, &json!(fila));
    let msg = format!("Claquete: {id} {}", if erro.is_empty() { "publicado no TikTok".to_string() } else { format!("NÃO subiu no TikTok ({erro})") });
    enviar(&msg);
    Some(msg)
}

/// Laco da rotina: das 7h as 21h. Sem pedido em andamento e com `busca_automatica`, abre um da busca; depois avanca um passo.
/// `subir` so e usado com `tiktok_auto` ligado no equipe.json.
pub fn tick(dados: &Path, f: &Estudio, agora: &Agora, ts: f64, subir: &dyn Fn(&str, &str) -> String, enviar: &dyn Fn(&str)) -> Vec<String> {
    if !(7..21).contains(&agora.hora) {
        return vec![];
    }
    let cfg = config(dados);
    if cfg["tiktok_auto"] == true {
        publicar_vencidos(dados, agora, subir, enviar);
    }
    if !pedidos(dados).iter().any(|p| PASSOS.contains(&s(p, "estado"))) && cfg["busca_automatica"] == true && uso(&pasta(dados), &agora.data.iso(), "pedidos") < limite(dados, "pedidos_dia", 3) {
        if let Ok((p, _)) = abrir_da_busca(dados, f, agora, ts) {
            enviar(&format!("Claquete: nova fonte na fila - {}", corta(s(&p, "titulo_fonte"), 100)));
        }
    }
    let feitos = trabalhar(dados, f, agora, ts, 1, false);
    for x in feitos.iter().filter(|x| x.contains("aguardando o OK") || x.contains("erro") || x.contains("reprovado")) {
        enviar(&format!("Claquete: {x}"));
    }
    feitos
}

pub fn situacao(dados: &Path) -> Value {
    let (ps, c) = (pedidos(dados), config(dados));
    json!({ "empresa": c["empresa"], "agentes": agentes(dados).len(), "pedidos": ps.len(), "aguardando_ok": ps.iter().filter(|p| p["estado"] == "aguardando_ok").count(),
        "em_andamento": ps.iter().filter(|p| PASSOS.contains(&s(p, "estado"))).count(), "liberado": c["direitos_ok"] == true && c["gameplay_ok"] == true })
}

pub fn quadro(dados: &Path) -> String {
    let (ps, sit) = (pedidos(dados), situacao(dados));
    let mut linhas = vec![format!("Claquete: {} pedido(s), {} em andamento, {} aguardando OK{}.", sit["pedidos"], sit["em_andamento"], sit["aguardando_ok"],
        if sit["liberado"] == true { "" } else { " (publicação travada: faltam direitos/gameplay)" })];
    linhas.extend(ps[ps.len().saturating_sub(10)..].iter().map(|p| format!("{} [{}] {}", s(p, "id"), s(p, "estado"), s(p, "autor"))));
    linhas.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    fn dir(nome: &str) -> (PathBuf, String, String) {
        let d = std::env::temp_dir().join(format!("braniac-empresa_cortes-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        for n in ["fala.mp4", "game.mp4"] {
            std::fs::write(d.join(n), "video").unwrap();
        }
        equipe(&d, json!({}));
        (d.clone(), caminho(&d.join("fala.mp4")), caminho(&d.join("game.mp4")))
    }

    fn equipe(d: &Path, extra: Value) {
        let mut c = json!({ "empresa": "Claquete", "agentes": [{ "nome": "Mina" }, { "nome": "Lupa" }], "limites": {}, "busca": { "nome": "Ana Teste" } });
        for (k, v) in extra.as_object().unwrap() {
            c[k] = v.clone();
        }
        gravar(&d.join("empresa_cortes/equipe.json"), &c).unwrap();
    }

    fn quando(dia: i64, hora: u32, min: u32) -> Agora {
        Agora { data: Data::civil(2026, 11, dia).unwrap(), hora, min, seg: 0 }
    }

    fn segs() -> Vec<Value> {
        (0..12).map(|i| json!({ "ini": i as f64 * 5.0, "fim": i as f64 * 5.0 + 5.0, "texto": format!("frase número {i} da fala") })).collect()
    }

    struct Falso {
        largura: Cell<i64>,
        linhas: RefCell<Vec<&'static str>>,
        cred: RefCell<String>,
    }

    impl Falso {
        fn novo() -> Falso {
            Falso { largura: Cell::new(1080), linhas: RefCell::default(), cred: RefCell::default() }
        }
        fn roda<R>(&self, f: impl FnOnce(&Estudio) -> R) -> R {
            let grava = |p: &Path| std::fs::write(p, "mp4").map_err(|e| e.to_string());
            f(&Estudio { ia: &|_, _| Ok("nada".into()), baixar: &|_, destino| grava(destino).map(|_| caminho(destino)), segmentos: &|_| Ok(segs()),
                montar: &|_, _, _, _, destino| grava(destino),
                queimar: &|_, srt, destino| { *self.cred.borrow_mut() = std::fs::read_to_string(srt).unwrap(); grava(destino) },
                sonda: &|_| Ok(json!({ "duracao": 30.0, "largura": self.largura.get(), "altura": 1920, "audio": true, "volume_db": -20.0 })),
                buscar: &|_| self.linhas.borrow().iter().map(|x| x.to_string()).collect() })
        }
    }

    #[test]
    fn abrir_recusa_sem_autor_gameplay_e_segredo() {
        let (d, fala, game) = dir("abrir");
        let a = quando(4, 10, 0);
        assert!(abrir(&d, &fala, "", "", &game, false, &a, 1.0).unwrap_err().contains("Sem autor"));
        assert!(abrir(&d, &fala, "Ana", "", "", false, &a, 1.0).unwrap_err().contains("gameplay"));
        assert!(abrir(&d, &fala, "Ana", "", "nao-existe.mp4", false, &a, 1.0).unwrap_err().contains("gameplay"));
        assert!(abrir(&d, &fala, "Ana", "a senha do cofre", &game, false, &a, 1.0).unwrap_err().contains("recusado"));
        assert!(abrir(&d, "nao-existe.mp4", "Ana", "", &game, false, &a, 1.0).unwrap_err().contains("Fonte não encontrada"));
        assert!(pedidos(&d).is_empty());
        for i in 1..=3 {
            assert_eq!(abrir(&d, &fala, "Ana", "Live", &game, false, &a, 1.0).unwrap().0["id"], format!("V{i:03}"));
        }
        assert!(abrir(&d, &fala, "Ana", "", &game, false, &a, 1.0).unwrap_err().contains("Teto de pedidos")); // 3 por dia
        assert!(abrir(&d, "https://exemplo.test/v", "Ana", "", &game, false, &quando(5, 10, 0), 1.0).is_ok());
    }

    #[test]
    fn fluxo_completo_cita_fonte_para_no_ok_e_so_agenda_com_tudo_liberado() {
        let (d, fala, game) = dir("fluxo");
        let (f, a) = (Falso::novo(), quando(4, 10, 0));
        abrir(&d, &fala, "Ana Teste", "Live de terça", &game, false, &a, 1.0).unwrap();
        let feitos = f.roda(|x| (0..7).flat_map(|_| trabalhar(&d, x, &a, 1.0, 1, false)).collect::<Vec<_>>());
        assert_eq!(feitos.len(), 6); // o setimo ciclo nao tem o que fazer: parou esperando o OK
        assert!(feitos[5].contains("aguardando o OK do dono") && pedidos(&d)[0]["estado"] == "aguardando_ok");
        assert!(f.cred.borrow().contains("Fonte: Ana Teste - Live de terça"));
        let trecho = &pedidos(&d)[0]["trecho"];
        assert!((20.0..=45.0).contains(&(trecho["fim"].as_f64().unwrap() - trecho["ini"].as_f64().unwrap())));
        // aprovar: travado ate liberar direitos, gameplay e locutor; e so o dono
        assert!(aprovar(&d, "V001", true, &a, 1.0).starts_with("Ainda não"));
        equipe(&d, json!({ "direitos_ok": true }));
        assert!(aprovar(&d, "V001", true, &a, 1.0).starts_with("Ainda não"));
        equipe(&d, json!({ "direitos_ok": true, "gameplay_ok": true }));
        assert!(aprovar(&d, "V001", false, &a, 1.0).contains("Só o dono"));
        assert!(aprovar(&d, "V001", true, &a, 1.0).contains("confirme antes"));
        assert!(confirmar_locutor(&d, "V001", false, 1.0).contains("Só o dono") && !d.join("empresa_cortes/fila_publicacao.json").exists());
        confirmar_locutor(&d, "v001", true, 1.0);
        assert!(aprovar(&d, "V001", true, &a, 1.0).contains("TikTok em 2026-11-04 20:00")); // 09:00 de hoje ja passou
        assert!(pedidos(&d)[0]["estado"] == "aprovado" && d.join("empresa_cortes/saida/kwai/V001.mp4").exists());
        assert!(aprovar(&d, "V001", true, &a, 1.0).starts_with("Pedido não"));
        assert_eq!(lista(&d.join("empresa_cortes/fila_publicacao.json")).len(), 1); // sem youtube_ritmo, so o TikTok
        assert!(quadro(&d).starts_with("Claquete: 1 pedido(s), 0 em andamento, 0 aguardando OK.\nV001 [aprovado] Ana Teste"));
    }

    #[test]
    fn lupa_reprova_formato_errado_e_rejeitar() {
        let (d, fala, game) = dir("lupa");
        let (f, a) = (Falso::novo(), quando(4, 10, 0));
        f.largura.set(1920);
        abrir(&d, &fala, "Ana", "", &game, false, &a, 1.0).unwrap();
        let feitos = f.roda(|x| trabalhar(&d, x, &a, 1.0, 1, false).into_iter().chain((0..5).flat_map(|_| trabalhar(&d, x, &a, 1.0, 1, false))).collect::<Vec<_>>());
        assert!(feitos[4].contains("reprovado pela Lupa: formato 1920x1920") && pedidos(&d)[0]["estado"] == "reprovado");
        assert_eq!(feitos.len(), 5);
        assert!(quadro(&d).contains("publicação travada"));
        assert!(rejeitar(&d, "V001", 1.0).contains("rejeitado") && rejeitar(&d, "V001", 1.0).contains("não encontrado"));
    }

    #[test]
    fn escolha_sem_ia_usa_janela_densa_e_srt_tem_credito() {
        let nada = |_: &str, _: &str| -> Result<String, String> { Ok("nada".into()) };
        let (ini, fim) = escolher(&segs(), 20.0, 45.0, &nada).unwrap();
        assert_eq!((ini, fim), (0.0, 45.0));
        let boa = |_: &str, _: &str| -> Result<String, String> { Ok("Sugiro {\"ini\": 10, \"fim\": 35.5}".into()) };
        assert_eq!(escolher(&segs(), 20.0, 45.0, &boa), Some((10.0, 35.5)));
        let fora = |_: &str, _: &str| -> Result<String, String> { Ok("{\"ini\": 10, \"fim\": 300}".into()) }; // longo demais: ignora a IA
        assert_eq!(escolher(&segs(), 20.0, 45.0, &fora), Some((0.0, 45.0)));
        assert_eq!(escolher(&segs()[..2], 20.0, 45.0, &nada), None);
        let leg = srt(&segs(), 12.0, 22.5, "Fonte: Ana");
        assert_eq!(leg, "1\n00:00:00,000 --> 00:00:03,000\nfrase número 2 da fala\n\n2\n00:00:03,000 --> 00:00:08,000\nfrase número 3 da fala\n\n3\n00:00:08,000 --> 00:00:10,500\nfrase número 4 da fala\n\n4\n00:00:07,500 --> 00:00:10,500\nFonte: Ana\n");
        assert_eq!(tempo_srt(3725.25), "01:02:05,250");
        assert_eq!(args_montar("f.mp4", "g.mp4", 12.0, 30.5, "base.mp4")[5..9], ["-ss", "12", "-t", "30.5"]);
        assert!(args_queimar("base.mp4", "leg.srt", "final.mp4")[8].starts_with("subtitles=leg.srt:force_style="));
    }

    #[test]
    fn teto_de_passos_e_erro_vira_falhou() {
        let (d, fala, game) = dir("teto");
        let (f, a) = (Falso::novo(), quando(4, 10, 0));
        equipe(&d, json!({ "limites": { "passos_dia": 1 } }));
        abrir(&d, &fala, "A", "", &game, false, &a, 1.0).unwrap();
        assert_eq!(f.roda(|x| trabalhar(&d, x, &a, 1.0, 1, false)).len(), 1);
        assert!(f.roda(|x| trabalhar(&d, x, &a, 1.0, 1, false))[0].contains("Teto"));
        assert!(f.roda(|x| tick(&d, x, &quando(4, 22, 0), 1.0, &|_, _| String::new(), &|_| ())).is_empty()); // fora do horario
        let sem_fala = |_: &str| -> Result<Vec<Value>, String> { Ok(vec![]) };
        abrir(&d, &fala, "B", "", &game, false, &a, 1.0).unwrap();
        let r = f.roda(|x| trabalhar(&d, &Estudio { segmentos: &sem_fala, ..*x }, &quando(5, 10, 0), 1.0, 5, true));
        assert!(r[0].contains("V001 →") && r[1] == "V002 erro: sem fala reconhecida na fonte" && pedidos(&d)[1]["estado"] == "falhou");
        assert_eq!(nomes(&d), ["Mina", "Lupa", "Pena", "Eco", "Prisma"]);
    }

    #[test]
    fn busca_so_aceita_canal_da_pessoa_ou_aprovado_e_barra_hostis_curtos_e_usados() {
        let (d, _, game) = dir("busca");
        let f = Falso::novo();
        *f.linhas.borrow_mut() = vec!["a1|Ana Teste no Podcast X|3000|Ana Teste", "a2|RESPOSTA ao Ana Teste|3000|Ana Teste", "a3|Ana Teste trecho|60|Ana Teste", "a4|Outro assunto qualquer|3000|Canal",
            "a5|Ana Teste live longa|90000|Ana Teste", "a6|ANA TESTE PERDEU A PACIÊNCIA|2400|Canal Hostil", "a7|Ana Teste no podcast Y|2000|Podcast Y", "a8|Ana Teste debate vs fulano|3000|Ana Teste", "a9|sem duracao|NA|Ana Teste"];
        let urls = |d: &Path| f.roda(|x| buscar_fontes(d, x.buscar, 3)).into_iter().map(|a| a.0[a.0.len() - 2..].to_string()).collect::<Vec<_>>();
        assert_eq!(urls(&d), ["a1"]);
        equipe(&d, json!({ "busca": { "nome": "Ana Teste", "canais_ok": ["Podcast Y"] }, "gameplay_padrao": game, "busca_automatica": true }));
        assert_eq!(urls(&d), ["a1", "a7"]);
        let avisos = RefCell::new(vec![]);
        let feitos = f.roda(|x| tick(&d, x, &quando(4, 10, 0), 1.0, &|_, _| String::new(), &|m| avisos.borrow_mut().push(m.to_string())));
        let p = pedidos(&d)[0].clone();
        assert_eq!((p["fonte"].as_str(), p["autor"].as_str(), feitos.len()), (Some("https://www.youtube.com/watch?v=a7"), Some("Ana Teste"), 1)); // a mais curta primeiro
        assert_eq!(avisos.borrow()[0], "Claquete: nova fonte na fila - Ana Teste no podcast Y (Podcast Y)");
        assert_eq!(urls(&d), ["a1"]);
        equipe(&d, json!({ "busca": {} }));
        assert!(urls(&d).is_empty()); // sem nome configurado nao busca nada
    }

    #[test]
    fn publicar_vencidos_so_no_horario_uma_vez_e_erro_fica_registrado() {
        let (d, _, _) = dir("publicar");
        let arq = d.join("empresa_cortes/fila_publicacao.json");
        let fila = json!([{ "id": "V001", "plataforma": "tiktok", "quando": "2026-11-04 09:00", "arquivo": "x.mp4", "estado": "agendado" }]);
        gravar(&arq, &fila).unwrap();
        let subidos = RefCell::new(vec![]);
        let ok = |_: &str, leg: &str| { subidos.borrow_mut().push(leg.to_string()); String::new() };
        assert!(publicar_vencidos(&d, &quando(4, 8, 59), &ok, &|_| ()).is_none() && subidos.borrow().is_empty());
        assert!(publicar_vencidos(&d, &quando(4, 9, 0), &ok, &|_| ()).unwrap().contains("publicado"));
        assert!(publicar_vencidos(&d, &quando(4, 9, 5), &ok, &|_| ()).is_none() && subidos.borrow().len() == 1);
        assert_eq!(lista(&arq)[0]["estado"], "publicado");
        gravar(&arq, &fila).unwrap();
        assert!(publicar_vencidos(&d, &quando(4, 9, 1), &|_, _| "sessão caiu".into(), &|_| ()).unwrap().contains("NÃO subiu"));
        assert_eq!(lista(&arq)[0]["estado"], "erro: sessão caiu");
        // sem tiktok_auto no equipe.json o tick nem tenta subir
        gravar(&arq, &fila).unwrap();
        let f = Falso::novo();
        f.roda(|x| tick(&d, x, &quando(4, 10, 0), 1.0, &ok, &|_| ()));
        assert!(lista(&arq)[0]["estado"] == "agendado" && subidos.borrow().len() == 1);
    }
}
