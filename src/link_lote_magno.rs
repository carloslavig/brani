//! Lupa, o agente da esteira de videos de orcamento de prefeitura (link/lote_magno.py): andamento de cada cidade e PENDENCIAS em
//! dados/magno/lote.json, lote de 5 para aprovacao, meta e parada da esteira (comando_esteira.json), lote atual do painel
//! (lotes_esteira.json), fila de prioridade (pendencia resolvida, serie 900 de outro estado, capitais) e o checklist dos PDFs.
//! So registra e avisa: quem produz o video e outro processo. Recado ao par fica SEGURADO (avisos_segurados.txt) a nao ser que o
//! dono tenha pedido; aviso ao dono sai como bloco `[resumo]` no outbox (vai no resumo do funil, nunca na hora).
//! Entram como closure: checklist compartilhado e barramento (`Fora`), situacao do checklist (`status`), religar/matar processos,
//! e a chamada ao agente com permissao de editar (`rodar` no `tick`).
//! Ficou de fora: `limpar_aprovados` (no Python esta DESLIGADA por ordem do dono: nenhum video sai do PC), a consulta aos
//! processos do Windows e o `taskkill` (closures), e as chamadas a concluidos/finalizar dentro do `tick` (quem chama roda antes).
//! Diferenca do Python: o nome "Lote de dd/mm HH:MM" e gravado no lote ao abrir (aqui nao ha fuso para converter o carimbo depois).
#![allow(dead_code)]
use crate::tempo::Agora;
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub const ETAPAS: [&str; 10] = ["reservada", "baixando_tcm", "levantamento", "roteiro", "voz", "sincronia", "render", "conferencia", "pronto", "pendencia"];
/// Nada de recado sozinho ao par; so o que o dono mandar (`pedido`).
pub const AVISO_AUTOMATICO: bool = false;
pub const TAM_LOTE: usize = 5;
pub const PROMPT_PROXIMO: &str = "Produza o PRÓXIMO lote de 5 vídeos de orçamento de prefeituras, do começo ao fim, seguindo EXATAMENTE o guia LOTE-5-VIDEOS.md do pacote de produção. Pendência de prefeitura vira registro no Lupa e você passa para a próxima cidade. Não publique nada; no fim registre o lote com os 5 números para a aprovação.";
pub const FEITO: [&str; 2] = ["pronto", "no_site"];
pub const ANDANDO: [&str; 7] = ["baixando_tcm", "levantamento", "roteiro", "esperando_voz", "voz", "esperando_render", "render"];
pub const SEM_DOCS: [(&str, &str); 3] = [("150", "capital modelo"), ("901", "painel do tribunal, sem PDF"), ("1001", "o par ja tem")];
const ESTEIRA_PUBLICA: [&str; 16] = ["meta", "prontos", "pendencias_esteira", "aguardando_site", "enviados_site", "atual", "ritmo_por_hora", "previsao_horas", "cidades_bahia", "atualizado", "progresso_pct", "fracao_atual", "cidades_seq", "vagas", "lote", "pdfs_site"];
static TRAVA: Mutex<()> = Mutex::new(());

/// O que sai do modulo sem ser arquivo: checklist compartilhado (cidade, nota, feito) e barramento (de, para, tipo, texto).
pub struct Fora<'a> {
    pub checklist: &'a dyn Fn(&str, &str, bool),
    pub barramento: &'a dyn Fn(&str, &str, &str, &str),
}

fn peso(etapa: &str) -> f64 {
    match etapa {
        "baixando_tcm" => 0.05,
        "levantamento" => 0.15,
        "roteiro" => 0.30,
        "esperando_voz" => 0.45,
        "voz" => 0.55,
        "esperando_render" => 0.70,
        "render" => 0.75,
        "pronto" | "no_site" => 1.0,
        _ => 0.0,
    }
}

fn m(dados: &Path, nome: &str) -> PathBuf {
    dados.join("magno").join(nome)
}

fn jler(arq: &Path) -> Option<Value> {
    serde_json::from_str(std::fs::read_to_string(arq).ok()?.trim_start_matches('\u{feff}')).ok()
}

fn jgravar(arq: &Path, v: &Value) -> Result<(), String> {
    let e = |e: std::io::Error| format!("{}: {e}", arq.display());
    std::fs::create_dir_all(arq.parent().unwrap_or(Path::new("."))).map_err(e)?;
    let tmp = arq.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&tmp, serde_json::to_string_pretty(v).map_err(|e| e.to_string())?).map_err(e)?;
    // no Windows o rename falha enquanto outro processo le o arquivo: tenta de novo antes de desistir
    for i in 0..20u64 {
        if std::fs::rename(&tmp, arq).is_ok() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(250 * (i + 1)));
    }
    let _ = std::fs::remove_file(&tmp);
    Err(format!("arquivo ocupado: {}", arq.display()))
}

fn corta(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn txt<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}

fn lista(v: &Value) -> &[Value] {
    v.as_array().map_or(&[], |a| a.as_slice())
}

fn num(n: &str) -> i64 {
    if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) { n.parse().unwrap_or(99999) } else { 99999 }
}

fn quando(a: &Agora) -> String {
    format!("{} {}", a.data.dia_mes(), a.hm())
}

pub fn ler(dados: &Path) -> Value {
    let mut d = jler(&m(dados, "lote.json")).filter(Value::is_object).unwrap_or_else(|| json!({}));
    for (k, vazio) in [("cidades", json!({})), ("pendencias", json!([])), ("historico", json!([]))] {
        if d[k].is_null() {
            d[k] = vazio;
        }
    }
    d
}

fn gravar(dados: &Path, d: &Value) -> Result<(), String> {
    jgravar(&m(dados, "lote.json"), d)
}

fn so(x: &Value, ks: &[&str]) -> Value {
    Value::Object(x.as_object().map(|o| ks.iter().filter_map(|k| o.get(*k).map(|v| (k.to_string(), v.clone()))).collect()).unwrap_or_default())
}

fn log_privado(linha: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    // linha de log com caminho, link, contato ou chave nao sai
    RE.get_or_init(|| Regex::new(r"(?i)[a-z]:[\\/]|/users/|https?://|@|\+55|senha|\bkey\b|token|sk-").expect("regex fixa")).is_match(linha)
}

/// O que o par pode ver pelo Link: so cidades, etapas, pendencias e contadores. Lista branca de chaves: campo novo nao vaza sozinho.
/// `falta` = fila ("231 Nome", ...) para os quadrados; `situacao(uf)` = molde do tribunal daquele estado (None = em construcao).
pub fn publico(dados: &Path, falta: &[String], situacao: &dyn Fn(&str) -> Option<Value>) -> Value {
    let d = ler(dados);
    let e = &d["esteira"];
    let hist = lista(&d["historico"]);
    let log = lista(&e["log"]);
    let texto = |l: &Value| l.as_str().map(String::from).unwrap_or_else(|| l.to_string());
    let mut esteira = so(e, &ESTEIRA_PUBLICA);
    esteira["log"] = json!(log[log.len().saturating_sub(40)..].iter().map(texto).filter(|l| !log_privado(l)).collect::<Vec<_>>());
    esteira["slots"] = json!(slots(e, falta));
    json!({
        "cidades": d["cidades"].as_object().map(|o| o.iter().map(|(n, c)| (n.clone(), so(c, &["numero", "cidade", "etapa", "detalhe", "desde"]))).collect::<Map<_, _>>()).unwrap_or_default(),
        "pendencias": lista(&d["pendencias"]).iter().map(|p| so(p, &["numero", "cidade", "motivo", "t"])).collect::<Vec<_>>(),
        "historico": hist[hist.len().saturating_sub(30)..].iter().map(|h| so(h, &["t", "numero", "cidade", "etapa", "detalhe"])).collect::<Vec<_>>(),
        "esteira": esteira,
        "capitais": capitais_publico(dados, &d, situacao),
    })
}

fn capitais_publico(dados: &Path, d: &Value, situacao: &dyn Fn(&str) -> Option<Value>) -> Vec<Value> {
    let Some(Value::Object(reg)) = jler(&m(dados, "capitais.json")) else { return vec![] };
    let conf = jler(&m(dados, "conferencia.json")).unwrap_or(Value::Null);
    let mut out: Vec<(i64, Value)> = vec![];
    for (n, r) in &reg {
        let (c, k) = (&d["cidades"][n], &conf[n]);
        let mol = situacao(txt(r, "uf")).unwrap_or_else(|| json!({ "molde": "em construção", "fonte_guia": "" }));
        let probs: Vec<String> = lista(&k["problemas"]).iter().take(5).map(|p| corta(&p.as_str().map(String::from).unwrap_or_else(|| p.to_string()), 160)).collect();
        out.push((
            num(n),
            json!({ "molde": mol["molde"], "fonte_guia": mol["fonte_guia"], "numero": n, "cidade": r["cidade"], "uf": r["uf"], "populacao": r["populacao"],
                "etapa": c["etapa"].as_str().filter(|s| !s.is_empty()).unwrap_or("fila"), "detalhe": txt(c, "detalhe"), "desde": c["desde"],
                "conferido": k["ok"], "conferido_em": k["quando"], "problemas": probs }),
        ));
    }
    out.sort_by_key(|(n, _)| *n);
    out.into_iter().map(|(_, v)| v).collect()
}

/// Os quadrados da leva, um por cidade: prontos, a atual e as proximas da fila.
/// shortcut: as proximas sao previsao; pendencia resolvida pode furar a fila, e o quadrado se corrige na leitura seguinte.
pub fn slots(e: &Value, falta: &[String]) -> Vec<Value> {
    let meta = e["meta"].as_i64().filter(|v| *v != 0).unwrap_or(50).max(0) as usize;
    if !lista(&e["slots"]).is_empty() {
        return lista(&e["slots"]).iter().take(meta).map(|v| json!({ "numero": v["numero"], "cidade": v["cidade"], "estado": v["estado"] })).collect();
    }
    let mut out: Vec<Value> = lista(&e["prontos"]).iter().take(meta).map(|p| json!({ "numero": p["numero"], "cidade": p["cidade"], "estado": "pronto" })).collect();
    let mut usados: BTreeSet<String> = out.iter().chain(lista(&e["pendencias_esteira"])).map(|s| txt(s, "numero").to_string()).collect();
    let a = &e["atual"];
    if !txt(a, "numero").is_empty() && !usados.contains(txt(a, "numero")) && out.len() < meta {
        out.push(json!({ "numero": a["numero"], "cidade": a["cidade"], "estado": "agora" }));
        usados.insert(txt(a, "numero").into());
    }
    for item in falta {
        if out.len() >= meta {
            break;
        }
        let (n, nome) = item.split_once(' ').unwrap_or((item, ""));
        if !usados.contains(n) {
            out.push(json!({ "numero": n, "cidade": nome, "estado": "fila" }));
        }
    }
    out
}

fn anexar(arq: &Path, texto: &str) -> Result<(), String> {
    let e = |e: std::io::Error| e.to_string();
    std::fs::create_dir_all(arq.parent().unwrap_or(Path::new("."))).map_err(e)?;
    std::fs::OpenOptions::new().create(true).append(true).open(arq).and_then(|mut f| f.write_all(texto.as_bytes())).map_err(e)
}

/// Recado ao par pelo Link (outbox_par.txt, que o vigia entrega). Aviso automatico (pedido = false) so fica registrado em
/// avisos_segurados.txt. Devolve true se foi para o outbox.
pub fn avisar_par(dados: &Path, texto: &str, pedido: bool, agora: &Agora) -> Result<bool, String> {
    let link = dados.join("link");
    if !pedido && !AVISO_AUTOMATICO {
        anexar(&link.join("avisos_segurados.txt"), &format!("{} {}\n", quando(agora), corta(&texto.trim().replace('\n', " "), 400)))?;
        return Ok(false);
    }
    let cx = link.join("outbox_par.txt");
    let antes = std::fs::read_to_string(&cx).unwrap_or_default().trim_start_matches('\u{feff}').trim().to_string();
    std::fs::create_dir_all(&link).map_err(|e| e.to_string())?;
    std::fs::write(&cx, format!("{}{}", if antes.is_empty() { String::new() } else { antes + "\n" }, texto.trim())).map_err(|e| e.to_string())?;
    Ok(true)
}

/// Aviso ao dono: bloco `[resumo]` no outbox (append: regravar perdia blocos de outros). Vai no resumo do funil, nao na hora.
pub fn avisar_dono(dados: &Path, texto: &str) -> Result<(), String> {
    anexar(&dados.join("outbox_carlos.txt"), &format!("\n---\n[resumo] {texto}"))
}

/// Registra a etapa da cidade. Etapa fora da lista e erro. `agora_s` = relogio em segundos.
#[allow(clippy::too_many_arguments)]
pub fn etapa(dados: &Path, numero: &str, cidade: &str, nome: &str, detalhe: &str, avisar: bool, agora_s: f64, agora: &Agora, f: &Fora) -> Result<Value, String> {
    if !ETAPAS.contains(&nome) {
        return Err(format!("etapa desconhecida: {nome}"));
    }
    let c = {
        let _t = TRAVA.lock().map_err(|e| e.to_string())?;
        let mut d = ler(dados);
        if d["cidades"][numero].is_null() {
            d["cidades"][numero] = json!({ "numero": numero, "cidade": cidade, "inicio": agora_s });
        }
        let c = &mut d["cidades"][numero];
        c["etapa"] = json!(nome);
        c["detalhe"] = json!(corta(detalhe, 300));
        c["desde"] = json!(agora_s);
        let c = c.clone();
        let mut h = lista(&d["historico"]).to_vec();
        h.push(json!({ "t": agora_s, "numero": numero, "cidade": cidade, "etapa": nome, "detalhe": corta(detalhe, 200) }));
        d["historico"] = json!(h[h.len().saturating_sub(300)..]);
        gravar(dados, &d)?;
        c
    };
    let com = |sep: &str, n: usize, fim: &str| if detalhe.is_empty() { String::new() } else { format!("{sep}{}{fim}", corta(detalhe, n)) };
    (f.barramento)("Lupa", "Compasso", "status", &format!("{numero} {cidade}: {nome}{}", com(" · ", usize::MAX, "")));
    (f.checklist)(cidade, &corta(&format!("Braniac: {nome}{}", com(" (", 120, ")")), 200), nome == "pronto");
    if avisar && ["levantamento", "render", "pronto", "pendencia"].contains(&nome) {
        avisar_par(dados, &format!("[Braniac · Lupa] {numero} {cidade}: {nome}{}", com(" · ", 200, "")), false, agora)?;
    }
    Ok(c)
}

/// Prefeitura/documento que nao deu: registra, avisa, e a fila segue para a proxima cidade.
pub fn pendencia(dados: &Path, numero: &str, cidade: &str, motivo: &str, avisar: bool, agora_s: f64, agora: &Agora, f: &Fora) -> Result<(), String> {
    {
        let _t = TRAVA.lock().map_err(|e| e.to_string())?;
        let mut d = ler(dados);
        let mut ps: Vec<Value> = lista(&d["pendencias"]).iter().filter(|p| txt(p, "numero") != numero).cloned().collect();
        ps.push(json!({ "numero": numero, "cidade": cidade, "motivo": corta(motivo, 400), "t": agora_s }));
        d["pendencias"] = json!(ps);
        gravar(dados, &d)?;
    }
    etapa(dados, numero, cidade, "pendencia", motivo, avisar, agora_s, agora, f)?;
    (f.barramento)("Lupa", "Jarvis", "alerta", &corta(&format!("Vídeo de prefeitura {numero} {cidade} parado: {motivo}"), 140));
    Ok(())
}

/// Os videos prontos vao para a aprovacao. So entra lote em que TODOS estao "pronto".
pub fn abrir_lote(dados: &Path, numeros: &[&str], agora_s: f64, agora: &Agora) -> Result<Value, String> {
    let (lote, nomes) = {
        let _t = TRAVA.lock().map_err(|e| e.to_string())?;
        let mut d = ler(dados);
        let faltam: Vec<&str> = numeros.iter().filter(|n| d["cidades"][**n]["etapa"] != "pronto").copied().collect();
        if !faltam.is_empty() {
            return Err(format!("não estão prontos: {}", faltam.join(", ")));
        }
        d["lote"] = json!({ "numeros": numeros, "enviado": agora_s, "estado": "aguardando_aprovacao" });
        gravar(dados, &d)?;
        (d["lote"].clone(), numeros.iter().map(|n| format!("{n} {}", txt(&d["cidades"][*n], "cidade"))).collect::<Vec<_>>().join(", "))
    };
    avisar_par(dados, &format!("[Braniac · Lupa] LOTE PARA APROVAÇÃO: {nomes}. Vídeo, capa, legenda e fontes na pasta de edições do pacote. Para aprovar, responda por vídeo: VIDEO APROVADO NNN (ou VIDEO REPROVADO NNN: motivo). Com os 5 aprovados, o Braniac produz os próximos 5."), false, agora)?;
    Ok(lote)
}

/// (meta, parado, so_fila). so_fila: so a fila de prioridade (capitais), sem puxar cidade da fila comum.
pub fn comando(dados: &Path) -> (i64, bool, bool) {
    let c = jler(&m(dados, "comando_esteira.json")).unwrap_or(Value::Null);
    (c["meta"].as_i64().unwrap_or(50), c["parado"].as_bool().unwrap_or(false), c["so_fila"].as_bool().unwrap_or(false))
}

pub fn gravar_comando(dados: &Path, meta: i64, parado: bool, so_fila: bool) -> Result<(), String> {
    jgravar(&m(dados, "comando_esteira.json"), &json!({ "meta": meta, "parado": parado, "so_fila": so_fila }))
}

fn orquestrador(dados: &Path) -> Value {
    jler(&m(dados, "orquestrador.json")).unwrap_or(Value::Null)
}

/// Religa o que caiu. `processos` = (script, argumentos); `rodando` = linhas de comando dos processos vivos (None = nao deu
/// para ver: na duvida nao religa nada, para nao ter esteira em dobro). Nao religa a esteira parada pelo dono nem a que bateu a meta.
pub fn garantir_esteira(dados: &Path, processos: &[(PathBuf, Vec<String>)], rodando: Option<&str>, iniciar: &dyn Fn(&Path, &[String], &Path)) -> Vec<String> {
    let Some(rodando) = rodando else { return vec![] };
    let (meta, parado, _) = comando(dados);
    let meta_batida = lista(&orquestrador(dados)["prontos"]).len() as i64 >= meta;
    let mut religados = vec![];
    for (script, args) in processos {
        let arquivo = script.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if arquivo.is_empty() || rodando.contains(&arquivo) || !script.exists() {
            continue;
        }
        let mut args = args.clone();
        if arquivo.contains("orquestrador") {
            if meta_batida || parado {
                continue;
            }
            args = vec!["--meta".into(), meta.to_string()];
        }
        let nome = script.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        iniciar(script, &args, &m(dados, &format!("{nome}.auto.log")));
        religados.push(nome);
    }
    religados
}

/// acao: "status" | "parar" | "continuar" | "mais" (quantos videos a mais). Devolve o texto para o dono.
/// `matar` derruba o orquestrador e devolve quantos processos caiu; `religar` sobe de novo.
pub fn esteira(dados: &Path, acao: &str, quantos: i64, matar: &dyn Fn() -> usize, religar: &dyn Fn(), agora_s: f64, agora: &Agora) -> Result<String, String> {
    let (meta, parado, so_fila) = comando(dados);
    let o = orquestrador(dados);
    let prontos = lista(&o["prontos"]).len() as i64;
    Ok(match acao {
        "parar" => {
            gravar_comando(dados, meta, true, so_fila)?;
            let n = matar();
            format!("⏸ Esteira parada com {prontos}/{meta} vídeos prontos.{} Para seguir: 'continuar a esteira'.", if n > 0 { " O vídeo que estava no meio volta para a fila." } else { "" })
        }
        "continuar" => {
            gravar_comando(dados, meta, false, so_fila)?;
            if prontos >= meta {
                return Ok(format!("A esteira já bateu a meta ({prontos}/{meta}). Para fazer mais, peça 'mais 50 vídeos'."));
            }
            religar();
            format!("▶ Esteira seguindo: {prontos}/{meta} prontos, retoma de onde parou.")
        }
        "mais" => {
            let quantos = if quantos == 0 { 50 } else { quantos }.clamp(1, 200);
            let nova = meta.max(prontos) + quantos;
            gravar_comando(dados, nova, false, so_fila)?;
            // fica de fora do lote novo tudo que a esteira ja tocou e nao esta andando agora (prontas, pendencias, capitais paradas)
            let mut excluir: BTreeSet<String> = lista(&o["prontos"]).iter().filter_map(|p| p.as_str().map(String::from)).collect();
            excluir.extend(lista(&ler(dados)["esteira"]["cidades_seq"]).iter().filter(|x| !ANDANDO.contains(&txt(x, "etapa"))).map(|x| txt(x, "numero").to_string()));
            novo_lote(dados, quantos, Some(meta), nova, &excluir, &[], agora_s, agora)?;
            religar();
            format!("▶ Mais {quantos}: a meta agora é {nova} vídeos ({prontos} já prontos). A esteira segue na ordem, de 5 em 5 para o site.")
        }
        _ => {
            let situacao = if parado {
                "PARADA".to_string()
            } else if prontos >= meta {
                "meta batida".into()
            } else {
                format!("rodando ({})", if o["fase"]["modo"] == "voz" { "vozes" } else { "render" })
            };
            let n = |k: &str| lista(&o[k]).len();
            format!("Esteira {situacao}: {prontos}/{meta} prontos, {} no site, {} com voz esperando render, {} esperando voz, {} pendências.", n("enviados"), n("voz_ok"), n("preparadas"), n("pendencias"))
        }
    })
}

/// {"atual": lote ou null, "anteriores": [...]}.
pub fn lotes(dados: &Path) -> Value {
    let l = jler(&m(dados, "lotes_esteira.json")).unwrap_or(Value::Null);
    json!({ "atual": l["atual"], "anteriores": if l["anteriores"].is_array() { l["anteriores"].clone() } else { json!([]) } })
}

/// Abre o lote pedido agora; o atual vai para "anteriores". `excluir` = cidades que nao entram neste lote.
/// `cidades`: lista fixa (para registrar um lote que ja estava rodando); vazia, o painel preenche conforme a esteira pega.
#[allow(clippy::too_many_arguments)]
pub fn novo_lote(dados: &Path, quantos: i64, meta_anterior: Option<i64>, meta: i64, excluir: &BTreeSet<String>, cidades: &[&str], agora_s: f64, agora: &Agora) -> Result<Value, String> {
    let mut l = lotes(dados);
    if !l["atual"].is_null() {
        let velho = l["atual"].take();
        if let Some(a) = l["anteriores"].as_array_mut() {
            a.push(velho);
        }
    }
    let excluir: Vec<&String> = excluir.iter().filter(|n| !cidades.contains(&n.as_str())).collect();
    l["atual"] = json!({ "inicio": agora_s, "nome": format!("Lote de {}", quando(agora)), "quantos": quantos, "meta_anterior": meta_anterior, "meta": meta, "excluir": excluir, "cidades": cidades });
    jgravar(&m(dados, "lotes_esteira.json"), &l)?;
    Ok(l["atual"].clone())
}

fn nome_lote(x: &Value) -> String {
    if let Some(n) = x["nome"].as_str().filter(|s| !s.is_empty()) {
        return n.to_string();
    }
    // lote aberto pelo Python (sem nome gravado): "Lote de dd/mm HH:MM" pelo inicio, no fuso desta maquina, como o Python fazia
    match x["inicio"].as_f64() {
        Some(i) => format!("Lote de {}", quando_epoch(i, &crate::tempo::agora(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0))),
        None => "Lote".to_string(),
    }
}

/// Carimbo -> "dd/mm HH:MM" local. O fuso sai da diferenca entre a hora local de agora e o carimbo de agora (arredondada a 15 min).
fn quando_epoch(t: f64, agora: &Agora, agora_s: f64) -> String {
    let local_agora = agora.data.0 * 86400 + agora.segundos();
    let fuso = ((local_agora as f64 - agora_s) / 900.0).round() as i64 * 900;
    let l = t as i64 + fuso;
    let a = Agora { data: crate::tempo::Data(l.div_euclid(86400)), hora: (l.rem_euclid(86400) / 3600) as u32, min: (l.rem_euclid(3600) / 60) as u32, seg: 0 };
    quando(&a)
}

/// O lote atual para o painel: as cidades dele (as que a esteira ja pegou + as proximas da fila ate completar), X de N e a %.
/// `seq` = [{numero, cidade, etapa}] do vigia; `falta` = fila ("231 Nome", ...) para prever as proximas (previsao nao e gravada).
pub fn painel_lote(dados: &Path, seq: &[Value], falta: &[String], grava: bool) -> Result<Option<Value>, String> {
    let mut l = lotes(dados);
    if l["atual"].is_null() {
        return Ok(None);
    }
    let por: BTreeMap<&str, &Value> = seq.iter().map(|c| (txt(c, "numero"), c)).collect();
    let etapa = |n: &str| por.get(n).map(|c| txt(c, "etapa")).filter(|e| !e.is_empty()).unwrap_or("fila").to_string();
    let strs = |v: &Value| lista(v).iter().filter_map(|x| x.as_str().map(String::from)).collect::<Vec<_>>();
    let mut cids = strs(&l["atual"]["cidades"]);
    let quantos = l["atual"]["quantos"].as_i64().unwrap_or(0);
    // cidade em pendencia ("guia") nao conta: a esteira pega outra no lugar
    let ativos = |cids: &[String]| cids.iter().filter(|n| etapa(n) != "guia").count() as i64;
    let mut usados: BTreeSet<String> = strs(&l["atual"]["excluir"]).into_iter().chain(cids.clone()).collect();
    usados.extend(lista(&l["anteriores"]).iter().flat_map(|x| strs(&x["cidades"])));
    let mut ordem: Vec<&Value> = seq.iter().collect();
    ordem.sort_by_key(|c| num(txt(c, "numero")));
    let mut novas = false;
    for c in ordem {
        if ativos(&cids) >= quantos {
            break;
        }
        let n = txt(c, "numero");
        if !usados.contains(n) && txt(c, "etapa") != "guia" {
            cids.push(n.into());
            usados.insert(n.into());
            novas = true;
        }
    }
    if novas && grava {
        l["atual"]["cidades"] = json!(cids);
        jgravar(&m(dados, "lotes_esteira.json"), &l)?;
    }
    let nomes: Vec<(&str, &str)> = falta.iter().map(|i| i.split_once(' ').unwrap_or((i, ""))).collect();
    let nome_de = |n: &str| por.get(n).map(|c| txt(c, "cidade")).filter(|s| !s.is_empty()).or_else(|| nomes.iter().find(|(x, _)| *x == n).map(|(_, nm)| *nm)).unwrap_or("").to_string();
    let mut cidades: Vec<Value> = cids.iter().map(|n| json!({ "numero": n, "cidade": nome_de(n), "etapa": etapa(n) })).collect();
    let mut vagas = quantos - ativos(&cids);
    for (n, nm) in &nomes {
        if vagas <= 0 {
            break;
        }
        if !usados.contains(*n) {
            cidades.push(json!({ "numero": n, "cidade": nm, "etapa": "fila" }));
            vagas -= 1;
        }
    }
    let prontos = cidades.iter().filter(|c| FEITO.contains(&txt(c, "etapa"))).count() as i64;
    let soma: f64 = cidades.iter().map(|c| peso(txt(c, "etapa"))).sum();
    let pct = if quantos > 0 { 100.0 * soma.min(quantos as f64) / quantos as f64 } else { 0.0 };
    let ant: Vec<Value> = lista(&l["anteriores"])
        .iter()
        .map(|x| {
            let cs = strs(&x["cidades"]);
            let p = if cs.is_empty() { x["prontos"].as_i64().unwrap_or(0) } else { cs.iter().filter(|n| FEITO.contains(&etapa(n).as_str())).count() as i64 };
            json!({ "nome": nome_lote(x), "quantos": x["quantos"], "prontos": p })
        })
        .collect();
    let a = &l["atual"];
    Ok(Some(json!({ "nome": nome_lote(a), "inicio": a["inicio"], "quantos": quantos, "meta_anterior": a["meta_anterior"], "meta": a["meta"], "prontos": prontos,
        "faltam": (quantos - prontos).max(0), "pendencias": cidades.iter().filter(|c| c["etapa"] == "guia").count(),
        "pct": (pct * 100.0).round() / 100.0, "cidades": cidades, "anteriores": ant })))
}

/// Para cada cidade que a esteira produziu, se os PDFs do tribunal ja subiram: status ok | falta | sem_doc. So fontes locais.
pub fn checklist_pdfs(dados: &Path, seq: &[Value], prontos: &[&str], log: &str) -> Value {
    let j = |nome: &str| jler(&m(dados, nome)).unwrap_or(Value::Null);
    let (fin, conc, caps) = (j("finalizados.json"), j("concluidos_site.json"), j("capitais.json"));
    let env: BTreeSet<String> = lista(&j("pdfs_enviados.json")).iter().filter_map(|x| x.as_str().map(String::from)).collect();
    let re = Regex::new(r"SITE-PDFS (\d+) .*\bok\b").expect("regex fixa");
    let no_log: BTreeSet<String> = re.captures_iter(log).map(|c| c[1].to_string()).collect();
    let obj = |v: &Value| v.as_object().cloned().unwrap_or_default();
    let (mut nomes, mut quem): (BTreeMap<String, String>, BTreeSet<String>) = Default::default();
    for (n, c) in obj(&conc) {
        if (139..900).contains(&num(&n)) {
            // 139 em diante = feitos pela esteira
            nomes.insert(n.clone(), txt(&c, "cidade").into());
            quem.insert(n);
        }
    }
    for c in seq {
        nomes.entry(txt(c, "numero").into()).or_insert_with(|| txt(c, "cidade").into());
        if FEITO.contains(&txt(c, "etapa")) {
            quem.insert(txt(c, "numero").into());
        }
    }
    quem.extend(obj(&fin).keys().cloned().chain(env.iter().cloned()).chain(prontos.iter().map(|p| p.to_string())));
    for (n, c) in obj(&fin) {
        nomes.entry(n).or_insert_with(|| txt(&c, "cidade").into());
    }
    for (n, c) in obj(&caps) {
        let nm = if txt(&c, "cidade").is_empty() { nomes.get(&n).cloned().unwrap_or_default() } else { txt(&c, "cidade").to_string() };
        nomes.insert(n, nm);
    }
    quem.remove("999"); // 999 = numero antigo da 1001 no site
    let mut ordem: Vec<&String> = quem.iter().collect();
    ordem.sort_by_key(|n| num(n));
    let mut conta = BTreeMap::from([("ok", 0), ("falta", 0), ("sem_doc", 0)]);
    let out: Vec<Value> = ordem
        .into_iter()
        .map(|n| {
            let c = &conc[n.as_str()];
            let subiu = !fin[n.as_str()].is_null() || env.contains(n) || no_log.contains(n) || ![Value::Null, json!(false), json!(0), json!("")].contains(&c["pdfs_enviados_ao_site"]) ||c["pdfs_no_site"].as_f64().unwrap_or(0.0) > 0.0;
            let st = if SEM_DOCS.iter().any(|(k, _)| k == n) { "sem_doc" } else if subiu { "ok" } else { "falta" };
            *conta.entry(st).or_default() += 1;
            let grupo = if num(n) >= 1000 { "Capitais" } else if num(n) >= 900 { "Mato Grosso" } else { "Bahia" };
            json!({ "numero": n, "cidade": nomes.get(n).cloned().filter(|c| !c.is_empty()).unwrap_or_else(|| if n == "150" { "Salvador".into() } else { String::new() }), "grupo": grupo, "status": st })
        })
        .collect();
    json!({ "ok": conta["ok"], "falta": conta["falta"], "sem_doc": conta["sem_doc"], "cidades": out })
}

/// Rotina: lote todo aprovado (checklist "finalizado"/"postado") -> dispara `rodar(PROMPT_PROXIMO)` para os proximos 5.
/// Um disparo por lote. `rodar` e o agente com permissao de editar: quem chama decide (e poe em segundo plano).
pub fn tick(dados: &Path, agora_s: f64, agora: &Agora, status: &dyn Fn(&str) -> Option<String>, rodar: &dyn Fn(&str)) -> Result<Option<&'static str>, String> {
    let numeros = {
        let _t = TRAVA.lock().map_err(|e| e.to_string())?;
        let mut d = ler(dados);
        if d["lote"]["estado"] != "aguardando_aprovacao" {
            return Ok(None);
        }
        let numeros: Vec<String> = lista(&d["lote"]["numeros"]).iter().filter_map(|n| n.as_str().map(String::from)).collect();
        if numeros.is_empty() || !numeros.iter().all(|n| matches!(status(n).as_deref(), Some("finalizado" | "postado"))) {
            return Ok(None);
        }
        d["lote"]["estado"] = json!("produzindo_proximo");
        d["lote"]["aprovado"] = json!(agora_s);
        gravar(dados, &d)?;
        numeros
    };
    avisar_dono(dados, &format!("Claudinho (via Braniac): os 5 vídeos foram aprovados ({}). Comecei os próximos 5.", numeros.join(", ")))?;
    avisar_par(dados, "[Braniac · Lupa] Os 5 vídeos foram aprovados. Começando os próximos 5; as cidades aparecem reservadas no checklist.", false, agora)?;
    rodar(PROMPT_PROXIMO);
    Ok(Some("proximo_lote"))
}

fn prioridade(dados: &Path) -> Vec<Value> {
    jler(&m(dados, "prioridade.json")).and_then(|v| v.as_array().cloned()).unwrap_or_default()
}

fn por_na_fila(dados: &Path, itens: &[(String, String)]) -> Result<Vec<Value>, String> {
    let mut fila = prioridade(dados);
    for (n, nome) in itens {
        if !fila.iter().any(|f| f[0] == n.as_str()) {
            fila.push(json!([n, nome]));
        }
    }
    jgravar(&m(dados, "prioridade.json"), &json!(fila))?;
    Ok(fila)
}

/// Pendencia resolvida: a cidade sai das pendencias e entra como a PROXIMA da esteira, logo depois da que esta sendo produzida.
pub fn resolver(dados: &Path, numero: &str, nota: &str, agora_s: f64, agora: &Agora, f: &Fora) -> Result<Vec<Value>, String> {
    let (cidade, fila) = {
        let _t = TRAVA.lock().map_err(|e| e.to_string())?;
        let mut d = ler(dados);
        let cidade = txt(&d["cidades"][numero], "cidade").to_string();
        d["pendencias"] = json!(lista(&d["pendencias"]).iter().filter(|p| txt(p, "numero") != numero).collect::<Vec<_>>());
        gravar(dados, &d)?;
        let fila = por_na_fila(dados, &[(numero.into(), cidade.clone())])?;
        (cidade, fila)
    };
    let nota = if nota.is_empty() { "pendência resolvida" } else { nota };
    etapa(dados, numero, &cidade, "reservada", &format!("{nota}: próxima da esteira"), false, agora_s, agora, f)?;
    Ok(fila)
}

/// Cidade de outro estado (serie 900): numero proprio (o mesmo se ja tiver), entra como PROXIMA da fila e a meta sobe 1.
/// Os numeros vem do painel do tribunal daquele estado (link_radar_mt); o video pronto NAO vai ao site.
pub fn mt(dados: &Path, nome: &str, religar: &dyn Fn(), agora_s: f64, agora: &Agora) -> Result<String, String> {
    let n = {
        let _t = TRAVA.lock().map_err(|e| e.to_string())?;
        let mut nums = jler(&m(dados, "mt.json")).filter(Value::is_object).unwrap_or_else(|| json!({ "Cidade": "901" }));
        let maior = nums.as_object().into_iter().flatten().filter_map(|(_, v)| v.as_str()?.parse::<i64>().ok()).fold(900, i64::max);
        let n = nums[nome].as_str().map(String::from).unwrap_or_else(|| (maior + 1).to_string());
        nums[nome] = json!(n);
        jgravar(&m(dados, "mt.json"), &nums)?;
        por_na_fila(dados, &[(n.clone(), nome.into())])?;
        n
    };
    Ok(format!("{nome} (MT) entrou na esteira como {n}. {}", esteira(dados, "mais", 1, &|| 0, religar, agora_s, agora)?))
}

/// true = video desta serie nao vai ao site (outro estado, serie 900, e capitais, 1001+): sai so para o dono.
pub fn fora_do_site(numero: &str) -> bool {
    num(numero) >= 900
}

/// Esteira das capitais: `lista_caps` = [[UF, nome, IBGE, populacao], ...] da maior para a menor; numeros 1001.., so elas.
pub fn capitais(dados: &Path, lista_caps: &Value, religar: &dyn Fn(), agora_s: f64, agora: &Agora) -> Result<String, String> {
    let caps = lista(lista_caps);
    if caps.is_empty() {
        return Err("lista de capitais vazia".into());
    }
    let reg: Vec<(String, Value)> = caps.iter().enumerate().map(|(i, c)| ((1001 + i).to_string(), json!({ "uf": c[0], "cidade": c[1], "ibge": c[2], "populacao": c[3] }))).collect();
    {
        let _t = TRAVA.lock().map_err(|e| e.to_string())?;
        jgravar(&m(dados, "capitais.json"), &Value::Object(reg.iter().cloned().collect()))?;
        let d = ler(dados);
        // pronta nao volta a fila
        let novas: Vec<(String, String)> = reg.iter().filter(|(n, _)| d["cidades"][n.as_str()]["etapa"] != "pronto").map(|(n, r)| (n.clone(), txt(r, "cidade").to_string())).collect();
        por_na_fila(dados, &novas)?;
    }
    let (meta, parado, _) = comando(dados);
    gravar_comando(dados, meta, parado, true)?;
    let texto = esteira(dados, "mais", reg.len() as i64, &|| 0, religar, agora_s, agora)?;
    Ok(format!("Esteira das capitais: {} cidades, de {} a {}. {texto}", reg.len(), caps[0][1].as_str().unwrap_or(""), caps[caps.len() - 1][1].as_str().unwrap_or("")))
}

/// Registro da capital (uf, cidade, ibge) pelo numero da esteira.
pub fn capital(dados: &Path, numero: &str) -> Option<Value> {
    jler(&m(dados, "capitais.json")).map(|mut v| v[numero].take()).filter(|v| !v.is_null())
}

/// Tira e devolve a primeira cidade da fila de prioridade ([numero, cidade]).
pub fn proxima_prioridade(dados: &Path) -> Result<Option<Value>, String> {
    let _t = TRAVA.lock().map_err(|e| e.to_string())?;
    let mut fila = prioridade(dados);
    if fila.is_empty() {
        return Ok(None);
    }
    let prim = fila.remove(0);
    jgravar(&m(dados, "prioridade.json"), &json!(fila))?;
    Ok(Some(prim))
}

/// {"por_etapa": {etapa: ["153 Cidade", ...]}, "pendencias": [...]}.
pub fn resumo(dados: &Path) -> Value {
    let d = ler(dados);
    let mut por: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for c in d["cidades"].as_object().into_iter().flat_map(|o| o.values()) {
        por.entry(txt(c, "etapa").into()).or_default().push(format!("{} {}", txt(c, "numero"), txt(c, "cidade")));
    }
    json!({ "por_etapa": por, "pendencias": d["pendencias"] })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;
    use std::cell::RefCell;

    fn pasta(nome: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("braniac-link_lote_magno-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(p.join("magno")).unwrap();
        p
    }

    fn ag() -> Agora {
        Agora { data: Data::civil(2026, 11, 18).unwrap(), hora: 9, min: 5, seg: 0 }
    }

    fn nada() -> Fora<'static> {
        Fora { checklist: &|_, _, _| (), barramento: &|_, _, _, _| () }
    }

    fn seq(etapas: &[(&str, &str)]) -> Vec<Value> {
        etapas.iter().map(|(n, e)| json!({ "numero": n, "cidade": format!("C{n}"), "etapa": e })).collect()
    }

    fn le(p: PathBuf) -> String {
        std::fs::read_to_string(p).unwrap_or_default()
    }

    #[test]
    fn aviso_automatico_segurado_so_passa_pedido() {
        let d = pasta("aviso");
        assert_eq!(avisar_par(&d, "lote novo\ncom quebra", false, &ag()), Ok(false));
        assert!(!d.join("link/outbox_par.txt").exists());
        assert_eq!(le(d.join("link/avisos_segurados.txt")), "18/11 09:05 lote novo com quebra\n");
        assert_eq!(avisar_par(&d, "relatório que o dono pediu", true, &ag()), Ok(true));
        avisar_par(&d, "segundo", true, &ag()).unwrap();
        assert_eq!(le(d.join("link/outbox_par.txt")), "relatório que o dono pediu\nsegundo");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn etapa_pendencia_e_recados() {
        let d = pasta("etapa");
        let (check, barra) = (RefCell::new(vec![]), RefCell::new(vec![]));
        let f = Fora { checklist: &|c, n, feito| check.borrow_mut().push(format!("{c}|{n}|{feito}")), barramento: &|de, para, tipo, t| barra.borrow_mut().push(format!("{de}>{para} {tipo}: {t}")) };
        etapa(&d, "153", "Barra do Proprietaria", "levantamento", "números fecharam", true, 10.0, &ag(), &f).unwrap();
        pendencia(&d, "152", "Aurelino Leal", "documento de dez/2024 não entregue", true, 20.0, &ag(), &f).unwrap();
        pendencia(&d, "152", "Aurelino Leal", "documento de dez/2024 não entregue", true, 21.0, &ag(), &f).unwrap(); // de novo: nao duplica
        let l = ler(&d);
        assert_eq!((txt(&l["cidades"]["153"], "etapa"), txt(&l["cidades"]["152"], "etapa")), ("levantamento", "pendencia"));
        assert_eq!(l["cidades"]["153"]["inicio"], 10.0);
        assert_eq!(lista(&l["pendencias"]).len(), 1);
        assert_eq!(resumo(&d)["por_etapa"]["pendencia"], json!(["152 Aurelino Leal"]));
        // automatico: nada vai ao par, so fica registrado
        assert!(!d.join("link/outbox_par.txt").exists());
        assert!(le(d.join("link/avisos_segurados.txt")).contains("152 Aurelino Leal: pendencia"));
        assert_eq!(check.borrow()[0], "Barra do Proprietaria|Braniac: levantamento (números fecharam)|false");
        assert!(barra.borrow().iter().any(|b| b.starts_with("Lupa>Jarvis alerta: Vídeo de prefeitura 152 Aurelino Leal parado")));
        assert!(barra.borrow().iter().any(|b| b == "Lupa>Compasso status: 153 Barra do Proprietaria: levantamento · números fecharam"));
        assert!(etapa(&d, "1", "X", "inventada", "", true, 1.0, &ag(), &f).unwrap_err().contains("etapa desconhecida"));
        etapa(&d, "153", "X", "pronto", "", false, 30.0, &ag(), &f).unwrap();
        assert_eq!(check.borrow().last().unwrap(), "X|Braniac: pronto|true");
        assert_eq!(txt(&ler(&d)["cidades"]["153"], "cidade"), "Barra do Proprietaria"); // o nome registrado fica
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn lote_aprovado_dispara_proximo_uma_vez() {
        let d = pasta("lote");
        let nums = ["153", "154", "158", "159", "169"];
        etapa(&d, "159", "Cidade 159", "voz", "", false, 1.0, &ag(), &nada()).unwrap();
        assert!(abrir_lote(&d, &["159"], 2.0, &ag()).unwrap_err().contains("não estão prontos: 159")); // lote so com video pronto
        for n in nums {
            etapa(&d, n, &format!("Cidade {n}"), "pronto", "", false, 1.0, &ag(), &nada()).unwrap();
        }
        assert_eq!(abrir_lote(&d, &nums, 2.0, &ag()).unwrap()["estado"], "aguardando_aprovacao");
        assert!(le(d.join("link/avisos_segurados.txt")).contains("LOTE PARA APROVAÇÃO: 153 Cidade 153"));
        let st = RefCell::new("entregue");
        let status = |_: &str| Some(st.borrow().to_string());
        let disparos = RefCell::new(vec![]);
        let rodar = |p: &str| disparos.borrow_mut().push(p.to_string());
        assert_eq!(tick(&d, 10.0, &ag(), &status, &rodar), Ok(None)); // ainda esperando a aprovacao
        assert_eq!(tick(&d, 10.0, &ag(), &|n| (n != "169").then(|| "finalizado".to_string()), &rodar), Ok(None)); // falta um
        *st.borrow_mut() = "finalizado";
        assert_eq!(tick(&d, 11.0, &ag(), &status, &rodar), Ok(Some("proximo_lote")));
        assert_eq!(tick(&d, 12.0, &ag(), &status, &rodar), Ok(None)); // um disparo so por lote
        assert_eq!(disparos.borrow().as_slice(), [PROMPT_PROXIMO]);
        assert!(le(d.join("outbox_carlos.txt")).starts_with("\n---\n[resumo] Claudinho (via Braniac): os 5 vídeos foram aprovados (153, 154")); // pelo funil
        assert!(!d.join("link/outbox_par.txt").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn pendencia_resolvida_entra_como_proxima() {
        let d = pasta("resolver");
        pendencia(&d, "141", "Irecê", "balanço não fecha", false, 1.0, &ag(), &nada()).unwrap();
        assert_eq!(proxima_prioridade(&d), Ok(None));
        resolver(&d, "141", "", 2.0, &ag(), &nada()).unwrap();
        resolver(&d, "141", "", 3.0, &ag(), &nada()).unwrap(); // duas vezes: entra uma so
        let l = ler(&d);
        assert!(lista(&l["pendencias"]).is_empty() && l["cidades"]["141"]["etapa"] == "reservada");
        assert_eq!(l["cidades"]["141"]["detalhe"], "pendência resolvida: próxima da esteira");
        assert_eq!(proxima_prioridade(&d), Ok(Some(json!(["141", "Irecê"]))));
        assert_eq!(proxima_prioridade(&d), Ok(None));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn garantir_esteira_religa_so_o_que_caiu_e_respeita_parada_e_meta() {
        let d = pasta("garantir");
        let (f1, f2) = (d.join("orquestrador_50.py"), d.join("vigia_painel.py"));
        std::fs::write(&f1, "").unwrap();
        std::fs::write(&f2, "").unwrap();
        let procs = vec![(f1, vec!["--meta".to_string(), "50".to_string()]), (f2, vec![]), (d.join("nao_existe.py"), vec![])];
        let cmds = RefCell::new(vec![]);
        let iniciar = |s: &Path, a: &[String], _: &Path| cmds.borrow_mut().push((s.to_path_buf(), a.to_vec()));
        assert_eq!(garantir_esteira(&d, &procs, Some("python vigia_painel.py"), &iniciar), ["orquestrador_50"]);
        assert!(garantir_esteira(&d, &procs, Some("orquestrador_50.py vigia_painel.py"), &iniciar).is_empty());
        assert!(garantir_esteira(&d, &procs, None, &iniciar).is_empty()); // nao deu para ver os processos: nao religa nada
        assert_eq!(cmds.borrow().len(), 1);
        jgravar(&m(&d, "orquestrador.json"), &json!({ "prontos": vec!["1"; 50] })).unwrap();
        assert_eq!(garantir_esteira(&d, &procs, Some("python"), &iniciar), ["vigia_painel"]); // meta 50 batida
        gravar_comando(&d, 100, true, false).unwrap();
        assert_eq!(garantir_esteira(&d, &procs, Some("python"), &iniciar), ["vigia_painel"]); // parada pelo dono
        gravar_comando(&d, 100, false, false).unwrap();
        assert_eq!(garantir_esteira(&d, &procs, Some("python"), &iniciar), ["orquestrador_50", "vigia_painel"]);
        assert_eq!(cmds.borrow()[3].1, ["--meta", "100"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn comando_esteira_parar_continuar_mais() {
        let d = pasta("comando");
        jgravar(&m(&d, "orquestrador.json"), &json!({ "prontos": (0..50).map(|i| i.to_string()).collect::<Vec<_>>(), "enviados": [] })).unwrap();
        let (mortos, religados) = (RefCell::new(0), RefCell::new(0));
        let matar = || { *mortos.borrow_mut() += 1; 1 };
        let religar = || *religados.borrow_mut() += 1;
        assert!(esteira(&d, "status", 0, &matar, &religar, 1.0, &ag()).unwrap().contains("meta batida"));
        assert!(esteira(&d, "parar", 0, &matar, &religar, 1.0, &ag()).unwrap().contains("Esteira parada com 50/50"));
        assert!(*mortos.borrow() == 1 && comando(&d).1);
        assert!(esteira(&d, "status", 0, &matar, &religar, 1.0, &ag()).unwrap().contains("PARADA"));
        assert!(esteira(&d, "continuar", 0, &matar, &religar, 1.0, &ag()).unwrap().contains("já bateu a meta"));
        assert!(*religados.borrow() == 0 && !comando(&d).1);
        assert!(esteira(&d, "mais", 50, &matar, &religar, 1.0, &ag()).unwrap().contains("meta agora é 100"));
        assert!(*religados.borrow() == 1 && comando(&d) == (100, false, false));
        assert!(esteira(&d, "mais", 9999, &matar, &religar, 1.0, &ag()).unwrap().contains("Mais 200")); // teto por pedido
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn mais_abre_lote_e_painel_conta_so_ele() {
        let d = pasta("painel");
        gravar_comando(&d, 4, false, false).unwrap();
        jgravar(&m(&d, "orquestrador.json"), &json!({ "prontos": ["1", "2", "3", "4"] })).unwrap();
        // o lote anterior tinha 4 prontos e uma pendencia (5): nenhuma entra no lote novo
        let mut velho = seq(&[("1", "no_site"), ("2", "no_site"), ("3", "no_site"), ("4", "no_site")]);
        velho.push(json!({ "numero": "5", "cidade": "x", "etapa": "guia" }));
        gravar(&d, &json!({ "cidades": {}, "pendencias": [], "historico": [], "esteira": { "cidades_seq": velho } })).unwrap();
        assert!(esteira(&d, "mais", 3, &|| 0, &|| (), 1.0, &ag()).unwrap().contains("meta agora é 7"));
        let a = lotes(&d)["atual"].clone();
        assert_eq!((a["quantos"].as_i64(), a["meta_anterior"].as_i64(), a["meta"].as_i64(), a["cidades"].clone()), (Some(3), Some(4), Some(7), json!([])));
        assert_eq!(a["nome"], "Lote de 18/11 09:05");
        // lote aberto pelo Python, sem nome: 1795271440 = 21/11/2026 11:30:40 em Brasilia (UTC-3)
        let agl = Agora { data: crate::tempo::Data::civil(2026, 11, 22).unwrap(), hora: 10, min: 0, seg: 0 };
        let ag_s = 1795352400.0; // 22/11/2026 13:00 UTC = 10:00 local
        assert_eq!(quando_epoch(1795271440.0, &agl, ag_s), "21/11 11:30");
        // a esteira pegou 6 e 7; 6 ficou pronta; 8 esta na voz
        let mut s = seq(&[("1", "no_site"), ("2", "no_site"), ("3", "no_site"), ("4", "no_site"), ("5", "guia"), ("6", "pronto"), ("7", "render"), ("8", "voz")]);
        let falta = ["9 Nove".to_string(), "10 Dez".to_string()];
        let p = painel_lote(&d, &s, &falta, true).unwrap().unwrap();
        let ns = |p: &Value| lista(&p["cidades"]).iter().map(|c| format!("{}:{}", txt(c, "numero"), txt(c, "etapa"))).collect::<Vec<_>>();
        assert_eq!(ns(&p), ["6:pronto", "7:render", "8:voz"]);
        assert_eq!((p["prontos"].as_i64(), p["quantos"].as_i64(), p["faltam"].as_i64()), (Some(1), Some(3), Some(2)));
        s[6]["etapa"] = json!("guia"); // 7 caiu em pendencia: nao conta, a esteira pega outra
        let p = painel_lote(&d, &s, &falta, true).unwrap().unwrap();
        assert_eq!(ns(&p), ["6:pronto", "7:guia", "8:voz", "9:fila"]);
        assert!(p["pendencias"] == 1 && p["cidades"][3]["cidade"] == "Nove");
        assert_eq!(lotes(&d)["atual"]["cidades"], json!(["6", "7", "8"])); // o que esta na fila e so previsao, nao fica gravado
        let pct = p["pct"].as_f64().unwrap();
        assert!(pct > 0.0 && pct < 100.0);
        // lote seguinte: o atual vai para "anteriores" e as cidades dele nao entram no novo
        esteira(&d, "mais", 2, &|| 0, &|| (), 2.0, &ag()).unwrap();
        s.push(json!({ "numero": "9", "cidade": "C9", "etapa": "roteiro" }));
        let p = painel_lote(&d, &s, &[], true).unwrap().unwrap();
        assert_eq!(ns(&p), ["9:roteiro"]);
        assert!(p["anteriores"][0]["prontos"] == 1 && p["anteriores"][0]["quantos"] == 3);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn lote_registrado_com_cidades_fixas() {
        let d = pasta("fixas");
        assert_eq!(painel_lote(&d, &[], &[], true), Ok(None));
        let cid: Vec<String> = (207..232).map(|n| n.to_string()).collect();
        let cid: Vec<&str> = cid.iter().map(String::as_str).collect();
        let excluir: BTreeSet<String> = ["150", "206", "207"].iter().map(|s| s.to_string()).collect();
        let a = novo_lote(&d, 25, Some(72), 97, &excluir, &cid, 1.0, &ag()).unwrap();
        assert_eq!(a["excluir"], json!(["150", "206"])); // cidade do lote nao fica na lista de fora
        let s = seq(&[("206", "no_site"), ("207", "no_site"), ("208", "pronto"), ("225", "render"), ("150", "pronto")]);
        let p = painel_lote(&d, &s, &["231 Campo Formoso".to_string()], true).unwrap().unwrap();
        assert_eq!((lista(&p["cidades"]).len(), p["prontos"].as_i64(), p["faltam"].as_i64()), (25, Some(2), Some(23)));
        assert_eq!(p["cidades"][24], json!({ "numero": "231", "cidade": "Campo Formoso", "etapa": "fila" }));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn checklist_dos_pdfs() {
        let d = pasta("pdfs");
        std::fs::write(m(&d, "pdfs_enviados.json"), r#"["141"]"#).unwrap();
        jgravar(&m(&d, "finalizados.json"), &json!({ "1002": { "cidade": "Capital Dois" }, "901": { "cidade": "Cidade MT" } })).unwrap();
        jgravar(&m(&d, "concluidos_site.json"), &json!({ "014": { "cidade": "Antiga" }, "139": { "cidade": "Feira", "pdfs_no_site": 17 }, "150": { "concluido": true }, "999": { "concluido": true } })).unwrap();
        let s = seq(&[("141", "no_site"), ("202", "no_site"), ("223", "pronto"), ("230", "voz")]);
        let r = checklist_pdfs(&d, &s, &["1001"], "17/11 02:45:56 SITE-PDFS 202 Araci ok");
        let st: Vec<String> = lista(&r["cidades"]).iter().map(|c| format!("{}:{}", txt(c, "numero"), txt(c, "status"))).collect();
        assert_eq!(st, ["139:ok", "141:ok", "150:sem_doc", "202:ok", "223:falta", "901:sem_doc", "1001:sem_doc", "1002:ok"]);
        assert_eq!((r["ok"].as_i64(), r["falta"].as_i64(), r["sem_doc"].as_i64()), (Some(4), Some(1), Some(3)));
        assert!(r["cidades"][7]["grupo"] == "Capitais" && r["cidades"][5]["grupo"] == "Mato Grosso" && r["cidades"][0]["grupo"] == "Bahia");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn cidade_de_outro_estado_entra_na_esteira_e_nao_vai_ao_site() {
        let d = pasta("mt");
        let r = mt(&d, "Nova Xavantina", &|| (), 1.0, &ag()).unwrap();
        assert!(r.contains("902"), "{r}");
        assert!(mt(&d, "Cidade", &|| (), 1.0, &ag()).unwrap().starts_with("Cidade (MT) entrou na esteira como 901"));
        assert!(mt(&d, "Nova Xavantina", &|| (), 1.0, &ag()).unwrap().contains("como 902")); // o mesmo numero se ja tiver
        assert_eq!(proxima_prioridade(&d), Ok(Some(json!(["902", "Nova Xavantina"]))));
        assert_eq!(comando(&d), (53, false, false));
        assert!(fora_do_site("901") && fora_do_site("1003") && !fora_do_site("231"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn capitais_entram_na_ordem_e_so_elas() {
        let d = pasta("capitais");
        etapa(&d, "1003", "Capital Tres", "pronto", "", false, 1.0, &ag(), &nada()).unwrap();
        let caps = json!([["SP", "Capital Um", 3550308, 11451999], ["RJ", "Capital Dois", 3304557, 6211223], ["XX", "Capital Tres", 1, 2]]);
        let r = capitais(&d, &caps, &|| (), 1.0, &ag()).unwrap();
        assert!(r.contains("de Capital Um a Capital Tres"));
        assert!(capitais(&d, &json!([]), &|| (), 1.0, &ag()).is_err());
        assert!(comando(&d).2 && capital(&d, "1002").unwrap()["uf"] == "RJ" && capital(&d, "1099").is_none());
        let p = publico(&d, &[], &|uf| (uf == "RJ").then(|| json!({ "molde": "conferido", "fonte_guia": "TCM-RJ" })));
        let et: Vec<String> = lista(&p["capitais"]).iter().map(|c| format!("{}:{}:{}", txt(c, "numero"), txt(c, "etapa"), txt(c, "molde"))).collect();
        assert_eq!(et, ["1001:fila:em construção", "1002:fila:conferido", "1003:pronto:em construção"]);
        assert_eq!(proxima_prioridade(&d), Ok(Some(json!(["1001", "Capital Um"]))));
        assert_eq!(proxima_prioridade(&d), Ok(Some(json!(["1002", "Capital Dois"]))));
        assert_eq!(proxima_prioridade(&d), Ok(None)); // a pronta nao volta a fila
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn publico_so_lista_branca() {
        let d = pasta("publico");
        gravar(&d, &json!({ "cidades": { "153": { "numero": "153", "cidade": "X", "etapa": "voz", "segredo": "nao sai" } }, "pendencias": [{ "numero": "1", "cidade": "Y", "motivo": "m", "t": 1, "interno": 2 }],
            "historico": [], "esteira": { "meta": 3, "interno": "nao sai", "prontos": [{ "numero": "150", "cidade": "P" }], "atual": { "numero": "151", "cidade": "A" },
                "log": ["render ok", "salvo em C:\\pasta\\x.mp4", "veja https://exemplo.test/x", "token abc", "fale com ana@exemplo.test"] } })).unwrap();
        let p = publico(&d, &["150 P".into(), "152 B".into(), "153 C".into()], &|_| None);
        assert!(p["cidades"]["153"]["segredo"].is_null() && p["cidades"]["153"]["etapa"] == "voz");
        assert!(p["pendencias"][0]["interno"].is_null() && p["esteira"]["interno"].is_null() && p["esteira"]["meta"] == 3);
        assert_eq!(p["esteira"]["log"], json!(["render ok"]));
        let s: Vec<String> = lista(&p["esteira"]["slots"]).iter().map(|c| format!("{}:{}", txt(c, "numero"), txt(c, "estado"))).collect();
        assert_eq!(s, ["150:pronto", "151:agora", "152:fila"]); // ate a meta, sem repetir
        let _ = std::fs::remove_dir_all(&d);
    }
}
