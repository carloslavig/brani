//! Processador da fila do LINK (link/render_estudio.py): pedido de um par (audio da voz + roteiro de cenas) -> video pelo NOSSO estudio.
//! Recebe SO dados (meta.json + audio): nunca roda codigo vindo de fora. Aqui ficam a validacao do pedido (texto com palavra de segredo
//! e recusado, audio de 3 a 420 s, ate 40 cenas so com as pecas do estudio), o preparo das cenas (mesa continua, tempos por cena,
//! mapas e maquetes marcados pela legenda) e a legenda com as palavras do autor nos tempos da transcricao.
//! Limite atual: o `estudio3d::validar_cenas` deste projeto ainda recusa a peca "maquete"; as regras de mesa/maquete ja estao aqui
//! (mesa_continua, marcar_maquetes) e passam a valer quando o estudio aceitar a peca.
//! Um render por vez: quem garante e a fila (`link_fila::processar_um`) e o `estudio3d::renderizar(ocupado, ...)`.
//! Ficou de fora (closure/argumentos para quem chama executar): ffprobe, ffmpeg, a transcricao, a narracao com a voz do Braniac,
//! baixar malhas e satelite do mapa e o render. Trocas de texto da legenda (ex.: o arroba do canal) vem de configuracao.
#![allow(dead_code)]
use crate::estudio3d;
use crate::link_preparar_parceiro::{opcodes, Op};
use crate::texto_pt::sem_acento;
use regex::Regex;
use serde_json::{json, Map, Value};
use std::sync::OnceLock;

pub const AUDIO_EXT: [&str; 6] = [".mp3", ".wav", ".m4a", ".ogg", ".aac", ".flac"];
pub const MAX_AUDIO_S: f64 = 420.0;
pub const MAX_CENAS: usize = 40;
const SEGREDO: &str = r"(?i)senha|token|cpf|cart[aã]o|pix|chave de acesso|c[oó]digo de verifica";

fn rx(c: &'static OnceLock<Regex>, p: &str) -> &'static Regex {
    c.get_or_init(|| Regex::new(p).expect("regex fixa"))
}

fn segredo(t: &str) -> bool {
    static R: OnceLock<Regex> = OnceLock::new();
    rx(&R, SEGREDO).is_match(t)
}

fn r2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

fn norm(t: &str) -> String {
    sem_acento(t).chars().filter(char::is_ascii).collect()
}

fn corta(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn um_espaco(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn verdade(v: &Value) -> bool {
    !(v.is_null() || *v == false || *v == 0 || *v == "" || v.as_array().is_some_and(Vec::is_empty) || v.as_object().is_some_and(Map::is_empty))
}

pub fn args_duracao(arq: &str) -> Vec<String> {
    ["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "default=nw=1:nk=1", arq].iter().map(|s| s.to_string()).collect()
}

pub fn ler_duracao(saida: &str) -> Result<f64, String> {
    saida.trim().parse().map_err(|_| "audio ilegivel (ffprobe nao leu a duracao)".to_string())
}

pub fn args_wav(audio: &str, wav: &str) -> Vec<String> {
    ["ffmpeg", "-y", "-i", audio, "-vn", "-acodec", "pcm_s16le", "-ar", "44100", wav].iter().map(|s| s.to_string()).collect()
}

/// O arquivo de audio do pedido: "audio.<ext de audio>" (o primeiro em ordem de nome).
pub fn audio_do_pedido(nomes: &[&str]) -> Option<String> {
    let mut n: Vec<&str> = nomes.to_vec();
    n.sort();
    n.into_iter().find(|x| x.rfind('.').is_some_and(|i| &x[..i] == "audio" && AUDIO_EXT.contains(&x[i..].to_lowercase().as_str()))).map(str::to_string)
}

#[derive(Debug, PartialEq)]
pub enum Motor {
    /// Pedido em zip no formato do pacote: `link_render_parceiro` (mesmas travas de entrada).
    PacoteZip,
    /// Numero que talvez ja tenha codigo no pacote: tentar `link_render_parceiro::ficha_de_meta`; sem codigo la, cai no estudio.
    PacoteMeta,
    Estudio,
}

pub fn motor(tem_zip: bool, sem_codigo: bool, pacote_instalado: bool) -> Motor {
    if tem_zip {
        Motor::PacoteZip
    } else if !sem_codigo && pacote_instalado {
        Motor::PacoteMeta
    } else {
        Motor::Estudio
    }
}

#[derive(Debug, PartialEq)]
pub struct Pedido {
    pub titulo: String,
    pub narracao: String,
    pub fontes: String,
    /// Sem audio no pedido: o texto que o Braniac narra com a voz dele (ate 6000 caracteres).
    pub narrar: Option<String>,
    pub estilo: &'static str,
    pub cabecalho: Option<Value>,
    pub cenas: Vec<Value>,
    /// Todas as cenas vieram com "inicio": os tempos do autor valem; senao sao ajustados pela legenda.
    pub com_tempo: bool,
}

/// Confere o pedido ANTES de qualquer trabalho. Texto de terceiro com palavra de segredo/dado pessoal: recusado.
pub fn validar(meta: &Value, tem_audio: bool) -> Result<Pedido, String> {
    if !meta.is_object() {
        return Err("meta invalida".into());
    }
    let s = |k: &str| meta[k].as_str().unwrap_or("").to_string();
    let narrar = if tem_audio {
        None
    } else {
        let narr = um_espaco(&s("narracao"));
        if narr.split(' ').filter(|x| !x.is_empty()).count() < 25 || segredo(&narr) {
            return Err("falta o arquivo audio.mp3 (ou .wav/.m4a/.ogg) e o meta.json nao traz 'narracao' (25 palavras ou mais)".into());
        }
        Some(corta(&narr, 6000))
    };
    let titulo = corta(&um_espaco(&s("titulo")), 80);
    let titulo = if titulo.is_empty() { "Video".to_string() } else { titulo };
    let (narracao, fontes) = (corta(&s("narracao"), 20000), corta(&s("fontes"), 4000));
    if segredo(&format!("{titulo} {narracao} {fontes}")) {
        return Err("texto com palavra de segredo/dado pessoal: pedido recusado".into());
    }
    let mut cenas = estudio3d::validar_cenas(&meta["cenas"], MAX_CENAS)?;
    let com_tempo = cenas.iter().all(|c| !c["inicio"].is_null());
    let estilo = ["padrao", "quente", "mesa"].into_iter().find(|e| meta["estilo"] == *e).unwrap_or("padrao");
    if estilo == "mesa" {
        // mapa em tela cheia e achados como pilhas de moedas na mesa
        for c in cenas.iter_mut() {
            match c["peca"].as_str() {
                Some("mapa") => c["dados"]["tela_cheia"] = json!(true),
                Some("barras") => c["dados"]["mesa"] = json!(true),
                _ => {}
            }
            tempos_da_cena(c);
        }
        mesa_continua(&mut cenas);
    }
    let cab = &meta["cabecalho"];
    let cabecalho = (cab.is_object() && verdade(&cab["titulo"])).then(|| {
        let mut o = json!({ "titulo": corta(cab["titulo"].as_str().unwrap_or(""), 40) });
        for (k, lim) in [("arroba", 30), ("fonte", 120), ("escala", 120)] {
            if verdade(&cab[k]) {
                o[k] = json!(corta(&cab[k].as_str().map(str::to_string).unwrap_or_else(|| cab[k].to_string()), lim));
            }
        }
        o
    });
    Ok(Pedido { titulo, narracao, fontes, narrar, estilo, cabecalho, cenas, com_tempo })
}

pub fn validar_duracao(dur: f64) -> Result<(), String> {
    if (3.0..=MAX_AUDIO_S).contains(&dur) { Ok(()) } else { Err(format!("audio com {dur:.0}s: aceito de 3 a {MAX_AUDIO_S:.0}s")) }
}

/// O roteiro.json inicial (a legenda entra depois da transcricao).
pub fn roteiro_inicial(p: &Pedido) -> Value {
    let cenas: Vec<Value> = p
        .cenas
        .iter()
        .map(|c| {
            let mut c = c.clone();
            for (k, padrao) in [("inicio", 0), ("fim", 3)] {
                if c[k].is_null() {
                    c[k] = json!(padrao);
                }
            }
            c
        })
        .collect();
    let mut ro = json!({ "titulo": p.titulo, "estilo": p.estilo, "legenda": [], "cenas": cenas });
    if let Some(c) = &p.cabecalho {
        ro["cabecalho"] = c.clone();
    }
    ro
}

pub fn fontes_md(fontes: &str) -> String {
    format!("Pedido externo pelo BRANIAC LINK. Voz e roteiro sao do autor do pedido; conferir antes de publicar.\n{fontes}\n")
}

/// Os "t" dos enfeites (plaquinhas, faixa, etiqueta, destaques, barras) vem em segundos do VIDEO: passa para segundos da CENA.
pub fn tempos_da_cena(c: &mut Value) {
    let (ini, fim) = (c["inicio"].as_f64().unwrap_or(0.0), c["fim"].as_f64().unwrap_or(0.0));
    let acerta = |x: &mut Value| {
        if let Some(t) = x["t"].as_f64().filter(|t| x.is_object() && ini - 0.5 <= *t && *t <= fim + 0.5) {
            x["t"] = json!(r2((t - ini).max(0.0)));
        }
    };
    let Some(d) = c["dados"].as_object_mut() else { return };
    for k in ["plaquinhas", "destaques", "barras"] {
        d.get_mut(k).and_then(Value::as_array_mut).into_iter().flatten().for_each(&acerta);
    }
    for k in ["faixa", "etiqueta", "saldo", "arrecadou", "gastou"] {
        d.get_mut(k).into_iter().for_each(&acerta);
    }
}

/// UMA mesa do comeco ao fim: cada casinha sobe na 1a vez que o setor e falado e fica ate o fim, sempre no mesmo lugar (ordem da
/// fala); o ano comeca com as pilhas em volta do cofre. Monta, para cada cena de mesa, o que ja esta de pe (ja), as pilhas ja pousadas
/// no ano (ja_ano), as dos anos anteriores (anos_ant) e os tempos globais (t0, volta).
pub fn mesa_continua(cenas: &mut [Value]) {
    let maquete = |c: &Value| c["peca"] == "maquete";
    let (mut ordem, mut tipos): (Vec<String>, Map<String, Value>) = (Vec::new(), Map::new());
    for c in cenas.iter().filter(|c| maquete(c) && !verdade(&c["dados"]["visao_geral"])) {
        for s in c["dados"]["setores"].as_array().into_iter().flatten() {
            if let Some(nome) = s["nome"].as_str().filter(|n| !tipos.contains_key(*n)) {
                ordem.push(nome.to_string());
                tipos.insert(nome.to_string(), json!(s["tipo"].as_str().unwrap_or("casa")));
            }
        }
    }
    let todos: Vec<Value> = ordem.iter().take(16).map(|n| json!({ "nome": n, "tipo": tipos[n] })).collect();
    let (mut falados, mut ja_ano, mut ano, mut volta, mut gastou, mut base) = (Vec::<String>::new(), Map::new(), Value::Null, -1i64, json!(0), Map::new());
    let (mut hist, mut hist_ano): (Map<String, Value>, Vec<(String, Value, Value)>) = (Map::new(), Vec::new()); // hist: pilhas dos anos ANTERIORES por setor
    for c in cenas.iter_mut() {
        let (eh_maquete, t0) = (maquete(c), if c["inicio"].is_null() { json!(0) } else { c["inicio"].clone() });
        let barras = c["peca"] == "barras";
        let Some(d) = c["dados"].as_object_mut() else { continue };
        let pega = |d: &Map<String, Value>, k: &str| d.get(k).cloned().unwrap_or(Value::Null);
        if !(eh_maquete || (barras && verdade(&pega(d, "mesa")))) {
            continue;
        }
        let geral = verdade(&pega(d, "visao_geral"));
        if eh_maquete && !geral && pega(d, "ano") != ano {
            for (nome, cor, v) in hist_ano.drain(..) {
                if let Some(l) = hist.entry(nome).or_insert_with(|| json!([])).as_array_mut() {
                    l.push(json!({ "cor": cor, "valor": v }));
                }
            }
            ano = pega(d, "ano");
            volta += 1;
            ja_ano = Map::new();
            let g = pega(d, "gastou")["valor"].clone();
            gastou = if verdade(&g) { g } else { json!(0) };
            base = ["cor", "ano", "pilha_mi"].iter().filter_map(|k| d.get(*k).map(|v| (k.to_string(), v.clone()))).collect();
            d.insert("inicio_ano".into(), json!(true));
        }
        if barras || geral {
            for (k, v) in &base {
                d.entry(k.clone()).or_insert_with(|| v.clone());
            }
        }
        for (k, v) in [("todos", json!(todos)), ("ja", json!(falados)), ("ja_ordem", json!(falados)), ("ja_ano", json!(ja_ano)), ("anos_ant", json!(hist)), ("t0", t0), ("volta", json!(volta.max(0))), ("gastou_ano", if eh_maquete { gastou.clone() } else { json!(0) })] {
            d.insert(k.into(), v);
        }
        if eh_maquete && !geral {
            for s in d.get("setores").and_then(Value::as_array).into_iter().flatten() {
                let Some(nome) = s["nome"].as_str() else { continue };
                if !falados.iter().any(|f| f == nome) {
                    falados.push(nome.to_string());
                }
                if s["valor"].is_number() {
                    ja_ano.insert(nome.to_string(), s["valor"].clone());
                    hist_ano.retain(|h| h.0 != nome);
                    hist_ano.push((nome.to_string(), base.get("cor").cloned().unwrap_or(json!("dourado")), s["valor"].clone()));
                }
            }
        }
    }
}

fn falas_da_cena(ro: &Value, c: &Value) -> Vec<(f64, String)> {
    let (ini, fim) = (c["inicio"].as_f64().unwrap_or(0.0), c["fim"].as_f64().unwrap_or(0.0));
    ro["legenda"].as_array().into_iter().flatten().filter_map(|f| f["inicio"].as_f64().map(|t| (t, norm(f["texto"].as_str().unwrap_or(""))))).filter(|(t, _)| ini - 0.1 <= *t && *t < fim).collect()
}

/// Cenas de mapa: acha na legenda QUANDO a fala chega no estado e na cidade (ultima citacao dentro da cena) para o zoom acompanhar a
/// narracao. `geo(uf minusculo, slug do municipio)` devolve a malha ja baixada ({nome_uf, alvo: {nome}}).
pub fn marcar_mapas(ro: &mut Value, geo: &dyn Fn(&str, &str) -> Option<Value>) {
    let leg = json!({ "legenda": ro["legenda"] });
    for c in ro["cenas"].as_array_mut().into_iter().flatten().filter(|c| c["peca"] == "mapa") {
        let (uf, mun) = (c["dados"]["uf"].as_str().unwrap_or("").to_lowercase(), estudio3d::slug(c["dados"]["municipio"].as_str().unwrap_or("")));
        let Some(g) = geo(&uf, &mun) else { continue };
        let (falas, ini) = (falas_da_cena(&leg, c), c["inicio"].as_f64().unwrap_or(0.0));
        let achar = |termos: &[String], depois: f64| falas.iter().filter(|(t, txt)| *t > depois && termos.iter().any(|x| !x.is_empty() && txt.contains(x.as_str()))).map(|(t, _)| *t).next_back();
        let Some(est) = achar(&[norm(g["nome_uf"].as_str().unwrap_or("")), "estado".into()], -1.0) else { continue };
        let mut marcas = json!({ "estado": r2(est - ini) });
        if let Some(cid) = achar(&[norm(g["alvo"]["nome"].as_str().unwrap_or("")), "municipio".into(), "cidade".into()], est) {
            marcas["cidade"] = json!(r2(cid - ini));
        }
        c["dados"]["marcas"] = marcas;
    }
}

/// Cenas de maquete: cada setor entra quando a legenda o cita pela primeira vez (dentro da cena); sem citacao, mantem o tempo dado.
pub fn marcar_maquetes(ro: &mut Value) {
    let leg = json!({ "legenda": ro["legenda"] });
    for c in ro["cenas"].as_array_mut().into_iter().flatten().filter(|c| c["peca"] == "maquete") {
        let (falas, ini, fim) = (falas_da_cena(&leg, c), c["inicio"].as_f64().unwrap_or(0.0), c["fim"].as_f64().unwrap_or(0.0));
        let Some(setores) = c["dados"]["setores"].as_array_mut() else { continue };
        let ts: Vec<f64> = setores.iter().filter_map(|s| s["t"].as_f64()).collect();
        let (maior, menor) = (ts.iter().copied().fold(f64::MIN, f64::max), ts.iter().copied().fold(f64::MAX, f64::min));
        if !ts.is_empty() && maior > (fim - ini) + 0.5 && menor >= ini - 0.5 {
            // "t" veio em segundos do VIDEO (nao da cena): converte
            for s in setores.iter_mut() {
                if let Some(t) = s["t"].as_f64() {
                    s["t"] = json!(r2((t - ini - 0.3).max(0.2)));
                }
            }
        }
        let mut usado = ini - 1.0;
        for s in setores.iter_mut().filter(|s| s["t"].is_null()) {
            let nome = norm(s["nome"].as_str().unwrap_or(""));
            let chave = nome.split(' ').next().unwrap_or("");
            if let Some(t) = falas.iter().find(|(t, txt)| *t > usado && txt.contains(chave)).map(|f| f.0) {
                s["t"] = json!(r2((t - ini - 0.3).max(0.3)));
                usado = t;
            }
        }
    }
}

fn chave(w: &str) -> String {
    norm(w).chars().filter(|c| c.is_ascii_digit() || c.is_ascii_lowercase() || *c == '@').collect()
}

/// Troca as PALAVRAS da transcricao pelas do roteiro do autor (mantendo os tempos da transcricao). Trecho que nao casa recebe o tempo
/// do trecho que ele substitui, repartido por igual. A legenda NUNCA parte um valor ("12 bilhoes e 200 milhoes" fica junto).
pub fn legenda_do_texto(ro: &mut Value, texto: &str, max_palavras: usize) {
    let wh: Vec<(f64, f64, String)> = ro["legenda"].as_array().into_iter().flatten().flat_map(|f| f["palavras"].as_array().into_iter().flatten()).map(|p| (p["inicio"].as_f64().unwrap_or(0.0), p["fim"].as_f64().unwrap_or(0.0), p["texto"].as_str().unwrap_or("").to_string())).collect();
    let sc: Vec<&str> = texto.split_whitespace().collect();
    if wh.is_empty() || sc.len() < 5 {
        return;
    }
    let (ka, kb): (Vec<String>, Vec<String>) = (wh.iter().map(|p| chave(&p.2)).collect(), sc.iter().map(|w| chave(w)).collect());
    let mut novas: Vec<(f64, f64, &str)> = Vec::new();
    for (op, a1, a2, b1, b2) in opcodes(&ka, &kb) {
        if op == Op::Igual {
            novas.extend((0..b2 - b1).map(|k| (wh[a1 + k].0, wh[a1 + k].1, sc[b1 + k])));
        } else if b2 > b1 {
            // troca/insere: reparte o tempo do trecho da transcricao (ou o vao entre vizinhos)
            let n = (b2 - b1) as f64;
            let ini = if a2 > a1 { wh[a1].0 } else { novas.last().map_or(wh[0].0, |u| u.1) };
            let fim = if a2 > a1 { wh[a2 - 1].1 } else if a1 < wh.len() { wh[a1].0 } else { ini + 0.3 * n };
            let passo = ((fim - ini) / n).max(0.05);
            novas.extend((0..b2 - b1).map(|k| (r2(ini + k as f64 * passo), r2(ini + (k + 1) as f64 * passo), sc[b1 + k])));
        }
    }
    let eh_num = |w: &str| w.starts_with(|c: char| c.is_ascii_digit());
    let unidade = |w: &str| ["mil", "milhao", "milhoes", "bilhao", "bilhoes", "mi", "bi"].contains(&chave(w).as_str());
    let acaba = |w: &str, fins: &str| w.ends_with(|c| fins.contains(c));
    let (mut falas, mut bloco): (Vec<Vec<(f64, f64, &str)>>, Vec<(f64, f64, &str)>) = (Vec::new(), Vec::new());
    for (i, p) in novas.iter().enumerate() {
        bloco.push(*p);
        let (prox, depois) = (novas.get(i + 1).map_or("", |x| x.2), novas.get(i + 2).map_or("", |x| x.2));
        let no_meio = !acaba(p.2, ",.?!") && (unidade(prox) || (eh_num(prox) && (eh_num(p.2) || chave(p.2) == "e")) || (chave(prox) == "e" && eh_num(depois)));
        let cheio = bloco.len() >= max_palavras && !(no_meio && bloco.len() < max_palavras + 4);
        if cheio || acaba(p.2, ".?!") || (bloco.len() >= 3 && p.2.ends_with(',')) {
            falas.push(std::mem::take(&mut bloco));
        }
    }
    if !bloco.is_empty() {
        falas.push(bloco);
    }
    let mut leg: Vec<Value> = falas
        .iter()
        .filter_map(|b| Some((b.first()?, b.last()?, b)))
        .map(|(p, u, b)| json!({ "inicio": p.0, "fim": r2(u.1 + 0.15), "texto": b.iter().map(|x| x.2).collect::<Vec<_>>().join(" "), "palavras": b.iter().map(|x| json!({ "inicio": x.0, "fim": x.1, "texto": x.2 })).collect::<Vec<_>>() }))
        .collect();
    for i in 0..leg.len().saturating_sub(1) {
        // sem sobreposicao
        let prox = leg[i + 1]["inicio"].as_f64().unwrap_or(f64::MAX);
        leg[i]["fim"] = json!(r2(leg[i]["fim"].as_f64().unwrap_or(0.0).min(prox)));
    }
    ro["legenda"] = json!(leg);
}

/// Legenda so da transcricao (sem roteiro): "3 ,7" -> "3,7" (numero e uma palavra so), "Educação,2" -> "Educação, 2", e as trocas
/// configuradas (`trocas`: expressao regular do que a transcricao escreve -> como deve aparecer).
pub fn limpar_texto(t: &str, trocas: &[(Regex, String)]) -> String {
    static N: OnceLock<Regex> = OnceLock::new();
    let t = rx(&N, r"(\d)\s+,\s*(\d)").replace_all(t, "${1},${2}");
    let c: Vec<char> = t.chars().collect();
    let mut out = String::new();
    for (i, x) in c.iter().enumerate() {
        out.push(*x);
        // virgula colada entre texto (nao digito) e a palavra seguinte ganha espaco
        if *x == ',' && i > 0 && !c[i - 1].is_numeric() && !c[i - 1].is_whitespace() && c.get(i + 1).is_some_and(|p| !p.is_whitespace()) {
            out.push(' ');
        }
    }
    trocas.iter().fold(out, |t, (r, para)| r.replace_all(&t, regex::NoExpand(para)).into_owned())
}

pub fn limpar_legenda(ro: &mut Value, trocas: &[(Regex, String)]) {
    for f in ro["legenda"].as_array_mut().into_iter().flatten() {
        f["texto"] = json!(limpar_texto(f["texto"].as_str().unwrap_or(""), trocas));
    }
}

/// Fecha o roteiro depois da transcricao: tempos das cenas (se o autor nao deu), legenda com o texto do autor (padrao) ou limpa, e as
/// marcas de mapa e maquete. `falado`: o roteiro-falado.txt do pedido, usado quando o meta nao traz a narracao.
pub fn fechar_roteiro(ro: &mut Value, p: &Pedido, meta: &Value, falado: Option<&str>, trocas: &[(Regex, String)], geo: &dyn Fn(&str, &str) -> Option<Value>) {
    if !p.com_tempo {
        let falas: Vec<(f64, f64)> = ro["legenda"].as_array().into_iter().flatten().map(|f| (f["inicio"].as_f64().unwrap_or(0.0), f["fim"].as_f64().unwrap_or(0.0))).collect();
        if let Some(cenas) = ro["cenas"].as_array_mut() {
            estudio3d::ajustar_cenas(cenas, &falas);
        }
    }
    let leg = &meta["legenda"];
    let texto = if p.narracao.trim().is_empty() { corta(falado.unwrap_or(""), 20000) } else { p.narracao.clone() };
    if !texto.trim().is_empty() && leg["usar_texto_da_narracao"] != false {
        legenda_do_texto(ro, &texto, leg["max_palavras"].as_u64().filter(|m| (2..=8).contains(m)).unwrap_or(5) as usize);
    } else {
        limpar_legenda(ro, trocas);
    }
    marcar_mapas(ro, geo);
    marcar_maquetes(ro);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta() -> Value {
        json!({"titulo": "  Contas   da cidade ", "narracao": "", "cenas": [{"peca": "globo"}, {"peca": "pilha"}]})
    }

    #[test]
    fn pedido_so_de_dados_e_com_travas() {
        let p = validar(&meta(), true).unwrap();
        assert_eq!((p.titulo.as_str(), p.estilo, p.narrar.clone(), p.cenas.len(), p.com_tempo), ("Contas da cidade", "padrao", None, 2, false));
        assert!(validar(&json!([1]), true).unwrap_err().contains("meta invalida"));
        // sem audio: so com narracao de 25+ palavras e sem palavra de segredo
        assert!(validar(&meta(), false).unwrap_err().contains("falta o arquivo audio"));
        let mut m = meta();
        m["narracao"] = json!("palavra ".repeat(30));
        assert_eq!(validar(&m, false).unwrap().narrar.map(|n| n.split(' ').count()), Some(30));
        m["narracao"] = json!(format!("{} a senha do banco", "palavra ".repeat(30)));
        assert!(validar(&m, false).is_err() && validar(&m, true).unwrap_err().contains("segredo"));
        let mut m = meta();
        m["fontes"] = json!("meu CPF e tal");
        assert!(validar(&m, true).unwrap_err().contains("segredo"));
        let mut m = meta();
        m["cenas"] = json!([{"peca": "executar", "comando": "calc"}, {"peca": "globo"}]);
        assert_eq!(validar(&m, true).unwrap().cenas[0]["peca"], "barras"); // peca que o estudio nao tem nunca vira comando: cai na peca padrao, so com dados limpos
        assert!(validar(&m, true).unwrap().cenas[0]["dados"].get("comando").is_none());
        m["cenas"] = json!([{"peca": "globo"}]);
        assert!(validar(&m, true).is_err()); // poucas cenas
        m["cenas"] = json!(vec![meta()["cenas"][0].clone(); MAX_CENAS + 5]);
        assert_eq!(validar(&m, true).unwrap().cenas.len(), MAX_CENAS);
        assert!(validar_duracao(2.0).is_err() && validar_duracao(421.0).is_err() && validar_duracao(60.0).is_ok());
        assert_eq!(ler_duracao("83.5\n"), Ok(83.5));
        assert!(ler_duracao("N/A").is_err());
        // cabecalho e estilo
        let mut m = meta();
        m["estilo"] = json!("neon");
        m["cabecalho"] = json!({"titulo": "T".repeat(50), "arroba": "@canal", "fonte": ""});
        let p = validar(&m, true).unwrap();
        assert_eq!((p.estilo, p.cabecalho.as_ref().map(|c| c["titulo"].as_str().unwrap().len()), p.cabecalho.as_ref().map(|c| c.as_object().unwrap().len())), ("padrao", Some(40), Some(2)));
        let ro = roteiro_inicial(&p);
        assert_eq!((ro["cenas"][0]["inicio"].as_i64(), ro["cenas"][0]["fim"].as_i64(), ro["legenda"].as_array().map(Vec::len)), (Some(0), Some(3), Some(0)));
        assert!(ro["cabecalho"]["arroba"] == "@canal");
        assert_eq!(audio_do_pedido(&["meta.json", "audio.WAV", "audio.mp3", "audio.exe", "musica.mp3"]).as_deref(), Some("audio.WAV"));
        assert_eq!(audio_do_pedido(&["musica.mp3"]), None);
        assert_eq!((motor(true, true, false), motor(false, false, true), motor(false, true, true), motor(false, false, false)), (Motor::PacoteZip, Motor::PacoteMeta, Motor::Estudio, Motor::Estudio));
        assert_eq!(args_wav("a.mp3", "b.wav")[0], "ffmpeg");
    }

    #[test]
    fn mesa_continua_acumula_casinhas_e_pilhas_por_ano() {
        let mut cenas = vec![
            json!({"peca": "maquete", "inicio": 10, "fim": 20, "dados": {"ano": "2024", "cor": "azul", "gastou": {"valor": 12, "t": 11.0}, "setores": [{"nome": "Educação", "valor": 5}, {"nome": "Saúde", "valor": 3, "tipo": "hospital"}]}}),
            json!({"peca": "barras", "inicio": 20, "fim": 25, "dados": {"mesa": true, "barras": [{"t": 21.5}, {"t": 99.0}]}}),
            json!({"peca": "numero", "inicio": 25, "fim": 26, "dados": {}}),
            json!({"peca": "maquete", "inicio": 26, "fim": 40, "dados": {"ano": "2025", "setores": [{"nome": "Saúde", "valor": 4}, {"nome": "Obras"}]}}),
        ];
        cenas.iter_mut().for_each(tempos_da_cena);
        mesa_continua(&mut cenas);
        let d = |i: usize| cenas[i]["dados"].clone();
        assert_eq!(d(0)["gastou"]["t"], 1.0); // segundos do video -> da cena
        assert_eq!((d(1)["barras"][0]["t"].as_f64(), d(1)["barras"][1]["t"].as_f64()), (Some(1.5), Some(99.0))); // fora da cena: fica como veio
        assert_eq!(d(0)["todos"], json!([{"nome": "Educação", "tipo": "casa"}, {"nome": "Saúde", "tipo": "hospital"}, {"nome": "Obras", "tipo": "casa"}]));
        assert_eq!((d(0)["inicio_ano"].as_bool(), d(0)["ja"].clone(), d(0)["volta"].as_i64(), d(0)["gastou_ano"].as_i64(), d(0)["t0"].as_i64()), (Some(true), json!([]), Some(0), Some(12), Some(10)));
        assert_eq!((d(1)["ja"].clone(), d(1)["ja_ano"].clone(), d(1)["cor"].as_str(), d(1)["ano"].as_str(), d(1)["gastou_ano"].as_i64()), (json!(["Educação", "Saúde"]), json!({"Educação": 5, "Saúde": 3}), Some("azul"), Some("2024"), Some(0)));
        assert!(d(2).get("todos").is_none()); // cena que nao e de mesa fica intocada
        assert_eq!((d(3)["volta"].as_i64(), d(3)["ja_ano"].clone(), d(3)["gastou_ano"].as_i64()), (Some(1), json!({}), Some(0)));
        assert_eq!(d(3)["anos_ant"], json!({"Educação": [{"cor": "azul", "valor": 5}], "Saúde": [{"cor": "azul", "valor": 3}]}));
    }

    #[test]
    fn legenda_usa_o_texto_do_autor_nos_tempos_da_transcricao_e_nao_parte_valor() {
        let w = |i: usize, t: &str| json!({"inicio": i as f64, "fim": i as f64 + 0.8, "texto": t});
        let ouvido = "A cidade gastou 12 bilhoes e 200 milhoes com arroba canal hoje";
        let palavras: Vec<Value> = ouvido.split(' ').enumerate().map(|(i, t)| w(i, t)).collect();
        let mut ro = json!({"legenda": [{"inicio": 0, "fim": 12, "texto": ouvido, "palavras": palavras}], "cenas": []});
        legenda_do_texto(&mut ro, "A cidade gastou 12 bilhões e 200 milhões com @canal hoje.", 3);
        let textos: Vec<&str> = ro["legenda"].as_array().unwrap().iter().map(|f| f["texto"].as_str().unwrap()).collect();
        assert_eq!(textos, ["A cidade gastou", "12 bilhões e 200 milhões", "com @canal hoje."]);
        let l = &ro["legenda"];
        assert_eq!((l[1]["inicio"].as_f64(), l[1]["fim"].as_f64()), (Some(3.0), Some(7.95)));
        assert_eq!(l[0]["fim"].as_f64(), Some(2.95));
        assert_eq!(l[2]["palavras"][1], json!({"inicio": 9.0, "fim": 10.8, "texto": "@canal"})); // 2 palavras ouvidas viram 1 escrita, no mesmo tempo
        // texto curto demais ou sem palavras com tempo: nada muda
        let antes = ro.clone();
        legenda_do_texto(&mut ro, "so tres palavras", 5);
        assert_eq!(ro, antes);
    }

    #[test]
    fn limpa_legenda_e_marca_mapa_e_maquete_pela_fala() {
        let troca = vec![(Regex::new(r"(?i)\barroba\s+canal\b").unwrap(), "@canal".to_string())];
        assert_eq!(limpar_texto("Gastou 3 ,7 milhões. Educação,2 e Saúde, 1,5. Siga Arroba Canal", &troca), "Gastou 3,7 milhões. Educação, 2 e Saúde, 1,5. Siga @canal");
        let mut ro = json!({
            "legenda": [{"inicio": 1.0, "fim": 2.0, "texto": "No estado da Bahia,"}, {"inicio": 3.0, "fim": 4.0, "texto": "fica a cidade de Ilhéus."}, {"inicio": 6.0, "fim": 7.0, "texto": "Educação levou 5"}, {"inicio": 8.0, "fim": 9.0, "texto": "e saúde levou 3"}],
            "cenas": [
                {"peca": "mapa", "inicio": 0.5, "fim": 5.0, "dados": {"uf": "BA", "municipio": "Ilhéus"}},
                {"peca": "maquete", "inicio": 5.0, "fim": 10.0, "dados": {"setores": [{"nome": "Educação básica"}, {"nome": "Saúde"}, {"nome": "Obras"}]}},
                {"peca": "maquete", "inicio": 20.0, "fim": 30.0, "dados": {"setores": [{"nome": "A", "t": 22.0}, {"nome": "B", "t": 25.0}]}},
            ]
        });
        let geo = |uf: &str, mun: &str| (uf == "ba" && mun == "ilheus").then(|| json!({"nome_uf": "Bahia", "alvo": {"nome": "Ilhéus"}}));
        let p = validar(&meta(), true).unwrap();
        let p = Pedido { com_tempo: true, ..p };
        fechar_roteiro(&mut ro, &p, &json!({}), None, &troca, &geo);
        assert_eq!(ro["cenas"][0]["dados"]["marcas"], json!({"estado": 0.5, "cidade": 2.5}));
        let s = &ro["cenas"][1]["dados"]["setores"];
        assert_eq!((s[0]["t"].as_f64(), s[1]["t"].as_f64(), s[2].get("t")), (Some(0.7), Some(2.7), None));
        assert_eq!(ro["cenas"][2]["dados"]["setores"], json!([{"nome": "A", "t": 1.7}, {"nome": "B", "t": 4.7}])); // t em segundos do video vira da cena
        assert_eq!(ro["legenda"][0]["texto"], "No estado da Bahia,"); // sem narracao no pedido: legenda da transcricao, so limpa
        assert!(fontes_md("x").starts_with("Pedido externo") && args_duracao("a.mp3").len() == 8);
    }
}
