//! Fornalha, a empresa de marketing: pedidos em dados/empresa_mkt/ (equipe.json, pedidos.json, uso.json, pauta.json, mesmo formato do Python).
//! Fluxo: novo -> prompt -> gerado -> acabado -> legendado -> revisado -> entregue (pasta do Drive do cliente). NADA e publicado: nao existe
//! funcao de publicar aqui. Travas: pedido com palavra de segredo e recusado; cliente tem de estar cadastrado; legenda sem preco e sem promessa
//! que nao veio do briefing; nota minima da Cris (75); duas reprovacoes ou dois erros = "falhou"; teto de passos por dia; so das 7h as 21h.
//! Ficou de fora (closures em `Oficina`): IA, descricao da foto, gerar imagem/video (Gemini, Meta AI), acabamento (Pillow/ffmpeg),
//! medir imagem/sondar video e o envio ao Drive pelo navegador.
#![allow(dead_code)]
use crate::empresa_dev::{corta, destravar, gravar, historico, ler, limite_de, lista, norm, rx, segredo, somar_uso, travar, uso};
use crate::fornalha_estudio as oficina;
use crate::tempo::Agora;
use regex::Regex;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const TIPOS: [&str; 4] = ["imagem", "carrossel", "video", "stories"];
/// Pauta da semana, 0 = segunda.
const CALENDARIO: [&[&str]; 7] = [&["imagem"], &["video"], &["carrossel"], &["video"], &["imagem", "imagem"], &["imagem"], &["imagem"]];
const PASSOS: [(&str, &str); 6] = [("novo", "prompt"), ("prompt", "gerado"), ("gerado", "acabado"), ("acabado", "legendado"), ("legendado", "revisado"), ("revisado", "entregue")];

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}

fn pasta(dados: &Path) -> PathBuf {
    dados.join("empresa_mkt")
}

pub fn config(dados: &Path) -> Value {
    ler(&pasta(dados).join("equipe.json")).filter(Value::is_object).unwrap_or_else(|| json!({ "empresa": "Fornalha", "agentes": [], "clientes": {}, "limites": {} }))
}

pub fn nomes(dados: &Path) -> Vec<String> {
    config(dados)["agentes"].as_array().into_iter().flatten().map(|a| s(a, "nome").to_string()).collect()
}

pub fn limite(dados: &Path, chave: &str, padrao: i64) -> i64 {
    limite_de(&config(dados), chave, padrao)
}

pub fn pedidos(dados: &Path) -> Vec<Value> {
    lista(&pasta(dados).join("pedidos.json"))
}

fn proximo(est: &str) -> Option<&'static str> {
    PASSOS.iter().find(|(de, _)| *de == est).map(|(_, para)| *para)
}

fn chave_cliente(cfg: &Value, cliente: &str) -> String {
    let c = norm(cliente);
    cfg["clientes"].as_object().into_iter().flatten().map(|(k, _)| k).find(|k| !c.is_empty() && (c.contains(k.as_str()) || k.contains(&c))).cloned().unwrap_or(c)
}

/// Novo pedido. `foto` = caminho de uma foto REAL do cliente. Err com o motivo quando recusa.
pub fn abrir(dados: &Path, cliente: &str, tipo: &str, briefing: &str, foto: &str, ts: f64) -> Result<(Value, String), String> {
    let tipo = if tipo.is_empty() { "imagem".to_string() } else { tipo.to_lowercase().replace('í', "i").replace('é', "e") };
    let tipo = if tipo.starts_with("carro") { "carrossel".to_string() } else { tipo };
    if !TIPOS.contains(&tipo.as_str()) {
        return Err(format!("Tipo inválido ({}).", TIPOS.join("/")));
    }
    if segredo(briefing) {
        return Err("Pedido recusado: tem palavra de segredo/dado pessoal (senha, token, CPF, cartão, Pix).".into());
    }
    let cfg = config(dados);
    let ck = chave_cliente(&cfg, cliente);
    if cfg["clientes"].get(&ck).is_none() {
        return Err(format!("Cliente «{cliente}» ainda não está cadastrado na Fornalha."));
    }
    let mut ps = pedidos(dados);
    let p = json!({ "id": format!("P{:03}", ps.len() + 1), "cliente": ck, "tipo": tipo, "briefing": corta(briefing.trim(), 800), "foto": foto, "estado": "novo",
        "tentativas": 0, "arquivos": [], "legenda": "", "drive": [], "historico": [], "criado": ts });
    ps.push(p.clone());
    gravar(&pasta(dados).join("pedidos.json"), &json!(ps))?;
    let msg = format!("{} aberto: {tipo} para {ck}", s(&p, "id"));
    Ok((p, msg))
}

/// Foto real do cliente menos usada (rodizio), das pastas configuradas. So lista os .jpg; nao abre nenhum.
fn foto_base(dados: &Path, cfg: &Value, ck: &str) -> String {
    let c = &cfg["clientes"][ck];
    let mut fotos: Vec<String> = vec![];
    for d in std::iter::once(&c["fotos"]).chain(c["fotos_extra"].as_array().into_iter().flatten()).filter_map(Value::as_str).filter(|d| !d.is_empty()) {
        let mut desta: Vec<String> = std::fs::read_dir(d).into_iter().flatten().flatten().map(|e| e.path())
            .filter(|f| f.extension().and_then(|x| x.to_str()).is_some_and(|x| ["jpg", "jpeg"].contains(&x.to_lowercase().as_str()))).map(|f| f.to_string_lossy().into_owned()).collect();
        desta.sort();
        fotos.extend(desta);
    }
    let usadas = pedidos(dados);
    fotos.into_iter().min_by_key(|f| usadas.iter().filter(|x| x["foto"] == f.as_str()).count()).unwrap_or_default()
}

/// O que a Fornalha faz fora deste modulo. Quem chama decide como.
pub struct Oficina<'a> {
    /// IA gratis: (sistema, prompt) -> texto.
    pub ia: &'a dyn Fn(&str, &str) -> Result<String, String>,
    /// Descricao curta da foto real ("" se nao deu).
    pub descrever: &'a dyn Fn(&str) -> String,
    /// Gera a imagem ou o video: (prompt, destino, foto de referencia) -> caminho do arquivo gerado.
    pub gerar: &'a dyn Fn(&str, &Path, Option<&str>) -> Result<String, String>,
    /// Acabamento local: (arquivo, tipo do pedido) -> resumo do que fez.
    pub acabar: &'a dyn Fn(&str, &str) -> Result<String, String>,
    /// Mede a imagem para o placar.
    pub medir: &'a dyn Fn(&Path) -> Result<oficina::MedidaImagem, String>,
    /// Sonda do video (None = ffprobe nao leu).
    pub sondar: &'a dyn Fn(&Path) -> Option<oficina::Sonda>,
    /// Sobe os arquivos para a pasta do Drive: (arquivos, id da pasta). So entrega; nao publica.
    pub subir: &'a dyn Fn(&[String], &str) -> Result<(), String>,
}

/// Ravi: 1x por dia abre os pedidos da pauta (calendario da semana), no maximo `pedidos_dia` por cliente. Devolve os ids abertos.
pub fn planejar(dados: &Path, f: &Oficina, agora: &Agora, ts: f64) -> Vec<String> {
    let (arq, hoje) = (pasta(dados).join("pauta.json"), agora.data.iso());
    if ler(&arq).is_some_and(|d| d["dia"] == hoje.as_str()) || gravar(&arq, &json!({ "dia": hoje })).is_err() {
        return vec![];
    }
    let cfg = config(dados);
    let mut abertos = vec![];
    for (ck, c) in cfg["clientes"].as_object().into_iter().flatten() {
        for tipo in CALENDARIO[agora.data.semana()].iter().take(limite(dados, "pedidos_dia", 3) as usize) {
            let tema = (f.ia)("Você é o Ravi, planejador de pauta de marketing. Responda só uma linha, em português do Brasil.",
                &format!("Dê a ideia do post de hoje ({tipo}) para: {}. Uma linha, sem preço nem horário.", s(c, "marca"))).unwrap_or_default();
            let tema = if tema.trim().is_empty() { "post do dia".to_string() } else { corta(tema.trim(), 300) };
            if let Ok((p, _)) = abrir(dados, ck, tipo, &tema, &foto_base(dados, &cfg, ck), ts) {
                abertos.push(s(&p, "id").to_string());
            }
        }
    }
    abertos
}

fn fazer_prompt(dados: &Path, p: &mut Value, f: &Oficina, ts: f64) -> Result<(), String> {
    let cfg = config(dados);
    let (tipo, cliente) = (s(p, "tipo").to_string(), s(p, "cliente").to_string());
    let quem = if tipo == "video" { "Cena" } else { "Vega" };
    let formato = match tipo.as_str() {
        "carrossel" => "imagem de carrossel vertical 4:5 (uma de 3)",
        "video" => "vídeo curto vertical 9:16 de 6 a 8 segundos",
        "stories" => "imagem de stories vertical 9:16",
        _ => "imagem de feed vertical 4:5",
    };
    let foto = s(p, "foto");
    let desc = if !foto.is_empty() && Path::new(foto).exists() { (f.descrever)(foto) } else { String::new() };
    let mut extra = String::new();
    let est = oficina::estilo(&pasta(dados), &cliente);
    if !est.is_empty() {
        extra += &format!("\nLivro de estilo do cliente (siga):\n{est}");
    }
    let ganhos: Vec<String> = oficina::melhores(&pasta(dados), &cliente, &tipo, 2).iter().map(|g| corta(g, 500)).collect();
    if !ganhos.is_empty() {
        extra += &format!("\nPrompts que já deram certo para este cliente (mesmo nível de detalhe, ideia nova):\n- {}", ganhos.join("\n- "));
    }
    if tipo == "video" {
        extra += &format!("\n{}", oficina::ROTEIRO_VIDEO);
    }
    let txt = (f.ia)(&format!("Você é {quem}, diretor(a) de arte de marketing de comida. Escreva UM prompt completo e detalhado em português para o Gemini gerar {formato}. Descreva composição, luz, cores, enquadramento e movimento (se vídeo). Preserve fielmente o produto da foto de referência. Sem texto escrito dentro da arte, sem preços, sem marcas de terceiros. Responda só o prompt."),
        &format!("Marca: {}\nPedido: {}\nFoto real de referência: {}{extra}", s(&cfg["clientes"][&cliente], "marca"), s(p, "briefing"), if desc.is_empty() { "anexada" } else { &desc }))?;
    if txt.chars().count() < 30 {
        return Err("não consegui montar o prompt".into());
    }
    p["prompt"] = json!(corta(txt.trim(), 2200));
    historico(p, quem, "prompt pronto", ts, 12);
    Ok(())
}

fn gerar_arquivos(dados: &Path, p: &mut Value, f: &Oficina) -> Result<(), String> {
    let (id, tipo) = (s(p, "id").to_string(), s(p, "tipo").to_string());
    let dir = pasta(dados).join("saida").join(s(p, "cliente").replace(' ', "-")).join(&id);
    let n = if tipo == "carrossel" { 3 } else { 1 };
    let foto = Some(s(p, "foto")).filter(|x| !x.is_empty() && Path::new(x).exists());
    let mut arqs = vec![];
    for i in 1..=n {
        let mut prompt = s(p, "prompt").to_string();
        if !s(p, "ajuste").is_empty() {
            prompt += &format!("\n\nCorrija o defeito da versão anterior: {}.", s(p, "ajuste"));
        }
        if n > 1 {
            prompt += &format!("\n\nEsta é a imagem {i} de {n} de um carrossel, no mesmo estilo.");
        }
        let destino = if tipo == "video" { dir.join(format!("{id}.mp4")) } else { dir.join(format!("{id}_{i}.png")) };
        arqs.push((f.gerar)(&prompt, &destino, foto)?);
    }
    p["arquivos"] = json!(arqs);
    Ok(())
}

fn arquivos(p: &Value) -> Vec<String> {
    p["arquivos"].as_array().into_iter().flatten().filter_map(|a| a.as_str().map(String::from)).collect()
}

fn ext(a: &str) -> String {
    Path::new(a).extension().and_then(|x| x.to_str()).unwrap_or("").to_lowercase()
}

/// Verso: a IA escreve; o codigo tira preco que nao veio do briefing e promessa que ele nao autorizou.
fn legenda(dados: &Path, p: &mut Value, f: &Oficina, ts: f64) -> Result<(), String> {
    static TEM: OnceLock<Regex> = OnceLock::new();
    static PRECO: OnceLock<Regex> = OnceLock::new();
    let briefing = s(p, "briefing").to_string();
    let mut txt = (f.ia)("Você é o Verso, redator de marketing. Escreva a legenda de Instagram em português do Brasil: 2 a 4 linhas, 1 chamada para ação e 5 hashtags. NUNCA cite preço, horário ou sabor que não esteja no pedido.",
        &format!("Marca: {}\nPedido: {briefing}", s(&config(dados)["clientes"][s(p, "cliente")], "marca")))?;
    if !rx(&TEM, r"R\$\s*\d").is_match(&briefing) {
        txt = rx(&PRECO, r"R\$\s*[\d.,]+").replace_all(&txt, "").into_owned();
    }
    let (txt, tirados) = oficina::tirar_promessas(txt.trim(), &briefing);
    p["legenda"] = json!(corta(&txt, 900));
    let nota = if tirados.is_empty() { String::new() } else { format!(" (tirei promessas sem autorização: {})", corta(&tirados.join(", "), 80)) };
    historico(p, "Verso", &format!("legenda pronta{nota}"), ts, 12);
    Ok(())
}

/// Cris: cada arquivo existe, tem tamanho e passa no placar 0-100. Devolve (aprovado, motivo).
fn revisar(dados: &Path, p: &mut Value, f: &Oficina) -> (bool, String) {
    let minima = limite(dados, "nota_minima", 75);
    let mut notas = vec![];
    for a in arquivos(p) {
        let arq = Path::new(&a);
        let nome = arq.file_name().and_then(|n| n.to_str()).unwrap_or(&a).to_string();
        let bytes = std::fs::metadata(arq).map(|m| m.len()).unwrap_or(0);
        if bytes < 10_000 {
            return (false, format!("{nome} ausente ou vazio"));
        }
        let (nota, avisos) = match ext(&a).as_str() {
            "png" | "jpg" | "jpeg" => match (f.medir)(arq) {
                Ok(m) => oficina::placar_imagem(&m, s(p, "tipo")),
                Err(e) => (0, vec![format!("não consegui abrir a imagem ({})", corta(&e, 60))]),
            },
            "mp4" => oficina::placar_video((f.sondar)(arq).as_ref(), bytes),
            _ => continue,
        };
        notas.push(nota);
        if nota < minima {
            return (false, format!("{nome} nota {nota}: {}", avisos.join("; ")));
        }
    }
    let nota = notas.into_iter().min().unwrap_or(100);
    p["nota"] = json!(nota);
    (true, format!("nota {nota}"))
}

/// Pasta: nomes data_cliente_formato_n e a legenda em .txt ao lado.
fn nomear(p: &mut Value, agora: &Agora) -> Result<(), String> {
    let (dia, cliente, tipo) = (agora.data.iso().replace('-', ""), s(p, "cliente").replace(' ', "-"), s(p, "tipo").to_string());
    let mut novos = vec![];
    for (i, a) in arquivos(p).iter().enumerate() {
        let novo = Path::new(a).with_file_name(format!("{dia}_{cliente}_{tipo}_{}.{}", i + 1, ext(a)));
        std::fs::rename(a, &novo).map_err(|e| format!("não renomeei {a}: {e}"))?;
        novos.push(novo.to_string_lossy().into_owned());
    }
    if let (Some(primeiro), false) = (novos.first(), s(p, "legenda").is_empty()) {
        let base = Path::new(primeiro);
        let leg = base.with_file_name(format!("{}_legenda.txt", base.file_stem().and_then(|x| x.to_str()).unwrap_or("post")));
        std::fs::write(&leg, s(p, "legenda")).map_err(|e| format!("não gravei a legenda: {e}"))?;
        novos.push(leg.to_string_lossy().into_owned());
    }
    p["arquivos"] = json!(novos);
    Ok(())
}

fn entregar(dados: &Path, p: &mut Value, f: &Oficina, agora: &Agora, ts: f64) -> Result<(), String> {
    let drive = &config(dados)["clientes"][s(p, "cliente")]["drive"];
    let pasta_id = Some(s(drive, s(p, "tipo"))).filter(|x| !x.is_empty()).unwrap_or(s(drive, "imagem")).to_string();
    nomear(p, agora)?;
    if pasta_id.is_empty() {
        return Err("cliente sem pasta no Drive".into());
    }
    let arqs = arquivos(p);
    (f.subir)(&arqs, &pasta_id)?;
    p["drive"] = json!(arqs.iter().map(|a| Path::new(a).file_name().and_then(|n| n.to_str()).unwrap_or(a).to_string()).collect::<Vec<_>>());
    historico(p, "Pasta", "enviado ao Drive", ts, 12);
    oficina::guardar_vencedor(&pasta(dados), p, p["nota"].as_i64().unwrap_or(0), ts)
}

/// Um passo do pedido. Reprovacao da Cris volta ao prompt com o defeito anotado; na segunda, o pedido falha.
pub fn avancar(dados: &Path, p: &mut Value, f: &Oficina, agora: &Agora, ts: f64) -> Result<String, String> {
    let (id, est) = (s(p, "id").to_string(), s(p, "estado").to_string());
    match est.as_str() {
        "novo" => fazer_prompt(dados, p, f, ts)?,
        "prompt" => {
            gerar_arquivos(dados, p, f)?;
            let quem = if s(p, "tipo") == "video" { "Cena" } else { "Vega" };
            historico(p, quem, &format!("{} arquivo(s) gerado(s)", arquivos(p).len()), ts, 12);
        }
        "gerado" => {
            let mut feito = vec![];
            for a in arquivos(p).iter().filter(|a| ["png", "jpg", "jpeg", "mp4"].contains(&ext(a).as_str())) {
                feito.push((f.acabar)(a, s(p, "tipo"))?);
            }
            historico(p, "Matiz", &corta(&format!("acabamento local: {}", if feito.is_empty() { "nada a fazer".to_string() } else { feito.join("; ") }), 300), ts, 12);
        }
        "acabado" => legenda(dados, p, f, ts)?,
        "legendado" => {
            let (ok, motivo) = revisar(dados, p, f);
            historico(p, "Cris", &corta(&motivo, 300), ts, 12);
            if !ok {
                let tent = p["tentativas"].as_i64().unwrap_or(0) + 1;
                p["tentativas"] = json!(tent);
                p["ajuste"] = json!(corta(&motivo, 160));
                oficina::registrar_desperdicio(&pasta(dados), p, &motivo, ts)?;
                p["estado"] = json!(if tent >= 2 { "falhou" } else { "prompt" });
                return Ok(format!("{id} reprovado pela Cris: {motivo}"));
            }
        }
        "revisado" => entregar(dados, p, f, agora, ts)?,
        _ => return Ok(format!("{id} {est}")),
    }
    let novo = proximo(&est).unwrap_or("falhou");
    p["estado"] = json!(novo);
    Ok(format!("{id} → {novo}"))
}

/// Avanca ate `maximo` passos (um pedido por vez), dentro do teto diario. `ignorar_teto` so a pedido do dono.
pub fn trabalhar(dados: &Path, f: &Oficina, agora: &Agora, ts: f64, maximo: usize, ignorar_teto: bool) -> Vec<String> {
    let (dir, hoje) = (pasta(dados), agora.data.iso());
    if !travar(&dir, ts, 1500.0) {
        return vec![];
    }
    let mut feitos = vec![];
    for _ in 0..maximo {
        if !ignorar_teto && uso(&dir, &hoje, "passos") >= limite(dados, "passos_dia", 8) {
            break;
        }
        let Some(mut p) = pedidos(dados).into_iter().find(|x| proximo(s(x, "estado")).is_some()) else { break };
        feitos.push(avancar(dados, &mut p, f, agora, ts).unwrap_or_else(|e| {
            let tent = p["tentativas"].as_i64().unwrap_or(0) + 1;
            p["tentativas"] = json!(tent);
            historico(&mut p, "sistema", &format!("erro: {}", corta(&e, 150)), ts, 12);
            if tent >= 2 {
                p["estado"] = json!("falhou");
            }
            format!("{} erro: {}", s(&p, "id"), corta(&e, 100))
        }));
        let mut ps = pedidos(dados);
        if let Some(x) = ps.iter_mut().find(|x| x["id"] == p["id"]) {
            *x = p;
        }
        if somar_uso(&dir, &hoje, "passos").is_err() || gravar(&dir.join("pedidos.json"), &json!(ps)).is_err() {
            break; // sem gravar, o proximo passo repetiria o mesmo pedido sem contar no teto
        }
    }
    destravar(&dir);
    feitos
}

pub fn quadro(dados: &Path, agora: &Agora, ts: f64) -> String {
    let (ps, empresa) = (pedidos(dados), s(&config(dados), "empresa").to_string());
    if ps.is_empty() {
        return format!("{empresa}: nenhum pedido ainda.");
    }
    let linhas: Vec<String> = ps[ps.len().saturating_sub(12)..].iter().map(|p| format!("• {} [{}] {} {}: {}", s(p, "id"), s(p, "estado"), s(p, "tipo"), s(p, "cliente"), corta(s(p, "briefing"), 60))).collect();
    format!("{empresa} ({}/{} passos hoje, {} reprovação(ões) na semana):\n{}", uso(&pasta(dados), &agora.data.iso(), "passos"), limite(dados, "passos_dia", 8), oficina::desperdicio(&pasta(dados), 7.0, ts), linhas.join("\n"))
}

pub fn situacao(dados: &Path, agora: &Agora, ts: f64) -> Value {
    let ps = pedidos(dados);
    let por: serde_json::Map<String, Value> = PASSOS.iter().map(|(e, _)| *e).chain(["entregue", "falhou"]).map(|e| (e.to_string(), json!(ps.iter().filter(|p| p["estado"] == e).count()))).collect();
    json!({ "empresa": config(dados)["empresa"], "agentes": nomes(dados).len(), "pedidos": por, "passos_hoje": uso(&pasta(dados), &agora.data.iso(), "passos"), "reprovados_semana": oficina::desperdicio(&pasta(dados), 7.0, ts) })
}

/// Laco da rotina (~10 min): das 7h as 21h abre a pauta do dia e avanca um passo; avisa de entrega, erro e reprovacao.
pub fn tick(dados: &Path, f: &Oficina, agora: &Agora, ts: f64, enviar: &dyn Fn(&str)) -> Vec<String> {
    if !(7..21).contains(&agora.hora) {
        return vec![];
    }
    let novos = planejar(dados, f, agora, ts);
    if !novos.is_empty() {
        enviar(&format!("Fornalha: pauta de hoje aberta ({}).", novos.join(", ")));
    }
    let feitos = trabalhar(dados, f, agora, ts, 1, false);
    for x in feitos.iter().filter(|x| x.contains("→ entregue") || x.contains("erro") || x.contains("reprovado")) {
        enviar(&format!("Fornalha: {x}"));
    }
    feitos
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;
    use std::cell::{Cell, RefCell};

    fn dir(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-empresa_mkt-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        gravar(&d.join("empresa_mkt/equipe.json"), &json!({ "empresa": "Fornalha Teste", "agentes": [{ "nome": "Ravi" }, { "nome": "Cris" }], "limites": { "passos_dia": 8 },
            "clientes": { "casa da ana": { "marca": "Casa da Ana, esfihas", "fotos": d.join("fotos").to_string_lossy(), "drive": { "imagem": "PASTA-IMG", "video": "PASTA-VID" } } } })).unwrap();
        d
    }

    fn quando(dia: i64, hora: u32) -> Agora {
        Agora { data: Data::civil(2026, 11, dia).unwrap(), hora, min: 0, seg: 0 }
    }

    struct Falsa {
        legenda: &'static str,
        medida: Cell<oficina::MedidaImagem>,
        subidos: RefCell<Vec<(Vec<String>, String)>>,
        prompts: RefCell<Vec<String>>,
        drive_fora: Cell<bool>,
    }

    impl Falsa {
        fn nova(legenda: &'static str) -> Falsa {
            Falsa { legenda, medida: Cell::new(oficina::MedidaImagem { w: 1080, h: 1350, media: 120.0, desvio: 45.0, borda: 300.0 }), subidos: RefCell::default(), prompts: RefCell::default(), drive_fora: Cell::new(false) }
        }
        fn roda<R>(&self, f: impl FnOnce(&Oficina) -> R) -> R {
            f(&Oficina {
                ia: &|sis, _| Ok(if sis.contains("Verso") { self.legenda.to_string() } else if sis.contains("Ravi") { "esfiha do dia".to_string() } else { "Foto de esfiha dourada sobre mesa de madeira, luz lateral quente.".to_string() }),
                descrever: &|_| "esfiha".into(),
                gerar: &|prompt, destino, _| {
                    self.prompts.borrow_mut().push(prompt.to_string());
                    std::fs::create_dir_all(destino.parent().unwrap()).unwrap();
                    std::fs::write(destino, vec![7u8; 12_000]).unwrap();
                    Ok(destino.to_string_lossy().into_owned())
                },
                acabar: &|_, _| Ok("1080x1350".into()),
                medir: &|_| Ok(self.medida.get()),
                sondar: &|_| Some(oficina::Sonda { w: 1080, h: 1920, dur: 7.0, audio: true }),
                subir: &|arqs, pasta| {
                    if self.drive_fora.get() {
                        return Err("Drive deslogado".into());
                    }
                    self.subidos.borrow_mut().push((arqs.to_vec(), pasta.to_string()));
                    Ok(())
                },
            })
        }
    }

    #[test]
    fn pedido_recusa_segredo_tipo_e_cliente_desconhecido() {
        let d = dir("abrir");
        assert!(abrir(&d, "Casa da Ana", "imagem", "post com a senha do wifi", "", 1.0).unwrap_err().contains("recusado"));
        assert!(abrir(&d, "Casa da Ana", "imagem", "aceitamos pix", "", 1.0).unwrap_err().contains("recusado"));
        assert!(abrir(&d, "Padaria do Zé", "imagem", "pão", "", 1.0).unwrap_err().contains("não está cadastrado"));
        assert!(abrir(&d, "Casa da Ana", "outdoor", "esfiha", "", 1.0).unwrap_err().contains("Tipo inválido"));
        assert!(pedidos(&d).is_empty());
        let (p, msg) = abrir(&d, "ana", "Carrosséis", "esfiha gratinada", "", 1.0).unwrap();
        assert_eq!((p["cliente"].as_str(), p["tipo"].as_str(), msg.as_str()), (Some("casa da ana"), Some("carrossel"), "P001 aberto: carrossel para casa da ana"));
        assert_eq!(abrir(&d, "Casa da Ana", "Vídeo", "esfiha", "", 1.0).unwrap().0["tipo"], "video");
    }

    #[test]
    fn fluxo_completo_entrega_no_drive_sem_preco_nem_promessa_inventados() {
        let d = dir("fluxo");
        let f = Falsa::nova("Esfiha gratinada por R$ 9,90! A mais saudável da cidade. Peça já. #esfiha");
        abrir(&d, "Casa da Ana", "imagem", "esfiha gratinada", "", 1.0).unwrap();
        let agora = quando(2, 10);
        let feitos = f.roda(|x| trabalhar(&d, x, &agora, 1.0, 10, false));
        assert_eq!(feitos, ["P001 → prompt", "P001 → gerado", "P001 → acabado", "P001 → legendado", "P001 → revisado", "P001 → entregue"]);
        let p = pedidos(&d)[0].clone();
        let leg = p["legenda"].as_str().unwrap();
        assert!(!leg.contains("R$") && !leg.contains("9,90") && !leg.contains("saudável") && leg.contains("Peça já"), "{leg}");
        assert_eq!((p["estado"].as_str(), p["nota"].as_i64()), (Some("entregue"), Some(100)));
        let subidos = f.subidos.borrow();
        assert_eq!((subidos.len(), subidos[0].1.as_str()), (1, "PASTA-IMG"));
        assert_eq!(p["drive"], json!(["20261102_casa-da-ana_imagem_1.png", "20261102_casa-da-ana_imagem_1_legenda.txt"]));
        assert!(Path::new(&subidos[0].0[1]).exists());
        assert_eq!(oficina::melhores(&d.join("empresa_mkt"), "casa da ana", "imagem", 2).len(), 1);
        assert_eq!(uso(&d.join("empresa_mkt"), "2026-11-02", "passos"), 6);
        // preco que veio do briefing fica
        abrir(&d, "Casa da Ana", "imagem", "esfiha gratinada por R$ 9,90", "", 1.0).unwrap();
        let feitos = f.roda(|x| trabalhar(&d, x, &agora, 1.0, 10, false));
        assert_eq!(feitos.len(), 2); // teto de 8 passos por dia
        f.roda(|x| trabalhar(&d, x, &quando(3, 10), 1.0, 2, false));
        assert!(pedidos(&d)[1]["legenda"].as_str().unwrap().contains("R$ 9,90"));
        assert!(quadro(&d, &quando(3, 10), 1.0).contains("(2/8 passos hoje, 0 reprovação(ões) na semana)"));
    }

    #[test]
    fn cris_reprova_abaixo_da_nota_refaz_uma_vez_e_depois_falha() {
        let d = dir("cris");
        let f = Falsa::nova("Venha provar.");
        f.medida.set(oficina::MedidaImagem { w: 600, h: 600, media: 40.0, desvio: 20.0, borda: 30.0 });
        abrir(&d, "Casa da Ana", "carrossel", "esfiha", "", 1.0).unwrap();
        let feitos = f.roda(|x| trabalhar(&d, x, &quando(2, 10), 1.0, 5, true));
        assert!(feitos[4].starts_with("P001 reprovado pela Cris: P001_1.png nota 48"), "{feitos:?}");
        let p = pedidos(&d)[0].clone();
        assert_eq!((p["estado"].as_str(), p["tentativas"].as_i64(), p["arquivos"].as_array().unwrap().len()), (Some("prompt"), Some(1), 3));
        let feitos = f.roda(|x| trabalhar(&d, x, &quando(2, 10), 1.0, 4, true));
        assert!(f.prompts.borrow()[3].contains("Corrija o defeito da versão anterior") && f.prompts.borrow()[4].contains("imagem 2 de 3"));
        assert!(feitos[3].contains("reprovado") && pedidos(&d)[0]["estado"] == "falhou");
        assert_eq!(oficina::desperdicio(&d.join("empresa_mkt"), 7.0, 2.0), 2);
        assert!(f.subidos.borrow().is_empty());
        assert!(f.roda(|x| trabalhar(&d, x, &quando(2, 10), 1.0, 4, true)).is_empty());
    }

    #[test]
    fn erro_duas_vezes_falha_e_trava_segura_o_ciclo() {
        let d = dir("erro");
        let f = Falsa::nova("Venha provar.");
        f.drive_fora.set(true);
        abrir(&d, "Casa da Ana", "video", "esfiha", "", 1.0).unwrap();
        let feitos = f.roda(|x| trabalhar(&d, x, &quando(2, 10), 1.0, 7, true));
        assert_eq!((feitos[5].as_str(), feitos[6].as_str(), pedidos(&d)[0]["estado"].as_str()), ("P001 erro: Drive deslogado", "P001 erro: Drive deslogado", Some("falhou")));
        assert!(f.subidos.borrow().is_empty() && pedidos(&d)[0]["drive"] == json!([]));
        std::fs::write(d.join("empresa_mkt/.trabalhando"), "100").unwrap();
        abrir(&d, "Casa da Ana", "imagem", "esfiha", "", 1.0).unwrap();
        assert!(f.roda(|x| trabalhar(&d, x, &quando(2, 10), 200.0, 1, true)).is_empty());
        assert_eq!(f.roda(|x| trabalhar(&d, x, &quando(2, 10), 2000.0, 1, true)).len(), 1);
    }

    #[test]
    fn planejar_uma_vez_por_dia_e_tick_so_de_dia() {
        let d = dir("pauta");
        let f = Falsa::nova("Venha provar.");
        std::fs::create_dir_all(d.join("fotos")).unwrap();
        for n in ["a.jpg", "b.JPG", "c.txt"] {
            std::fs::write(d.join("fotos").join(n), "x").unwrap();
        }
        let avisos = RefCell::new(vec![]);
        let enviar = |m: &str| avisos.borrow_mut().push(m.to_string());
        assert!(f.roda(|x| tick(&d, x, &quando(6, 6), 1.0, &enviar)).is_empty() && f.roda(|x| tick(&d, x, &quando(6, 21), 1.0, &enviar)).is_empty());
        assert!(pedidos(&d).is_empty());
        assert_eq!(f.roda(|x| tick(&d, x, &quando(6, 8), 1.0, &enviar)), ["P001 → prompt"]); // sexta: duas imagens
        let ps = pedidos(&d);
        assert_eq!((ps.len(), ps[0]["briefing"].as_str(), ps[0]["tipo"].as_str()), (2, Some("esfiha do dia"), Some("imagem")));
        assert!(ps[0]["foto"].as_str().unwrap().ends_with("a.jpg") && ps[1]["foto"].as_str().unwrap().ends_with("b.JPG")); // rodizio de fotos reais
        assert_eq!(avisos.borrow().clone(), vec!["Fornalha: pauta de hoje aberta (P001, P002).".to_string()]);
        assert!(f.roda(|x| planejar(&d, x, &quando(6, 9), 1.0)).is_empty());
        assert_eq!(f.roda(|x| planejar(&d, x, &quando(3, 9), 1.0)), ["P003"]); // terca: video
    }
}
