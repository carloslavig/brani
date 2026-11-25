//! Os tres vigias da esteira do Parceiro (scripts-braniac/vigia_painel.py, vigia_mp4.py e vigia_esteira.py), agora dentro do B.R.A.N.I.
//! Cada um e um laco proprio numa thread propria (o relogio de 60 s so os garante no ar, uma vez) e le/grava os MESMOS arquivos e formatos
//! do Python, entao o historico continua de onde estava:
//! - painel: a cada 20 s junta orquestrador.json + log + lote.json + checklist em `lote.json["esteira"]` (o que o /api/parceiro devolve);
//! - mp4: a cada 5 s espelha (hardlink) e guarda no E: todo .mp4 final que o Lupa marcou "pronto"; se um sumir, volta do espelho;
//! - esteira: a cada 60 s acusa LOOP, TENTATIVAS e SEM AVANCO no orquestrador.log e chama o agente (com permissao de editar) para achar a
//!   causa. Teto: 1 chamada por ocorrencia a cada 3 h e 3 por dia. Esteira parada ou meta batida: nao vigia. Nunca publica, envia nem paga.
//! Diferencas do Python: a limpeza dos .mp4 aprovados (`limpar_aprovados`, a cada 10 min no painel) nao foi portada porque no Python esta
//! DESLIGADA por ordem do dono (APAGAR_DESLIGADO); `BRANIAC_VIGIAS=0` desliga os tres (para a troca do Python pelo Rust, sem vigia em dobro).
use crate::esteira_orq::{ascii, exec_real, Cmd, Config, Exec};
use crate::tempo::{Agora, Data};
use regex::Regex;
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const META: usize = 50;
const SEM_AVANCO_MIN: i64 = 75;
const LOOP_MIN: usize = 6;
const TETO_DIA: i64 = 3;
const ESPERA_H: f64 = 3.0;
const CHAMADA_S: u64 = 1800;



// ---------------------------------------------------------------- relogio e utilitarios

/// Agora em duas escalas: `epoch` (segundos reais) e `local` (relogio de parede contado desde 1970, como o mktime/localtime do Python).
#[derive(Clone, Copy)]
pub struct Relogio {
    pub epoch: f64,
    pub local: i64,
}

impl Relogio {
    pub fn agora() -> Relogio {
        let a: Agora = crate::tempo::agora();
        Relogio { epoch: SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0), local: a.data.0 * 86400 + a.segundos() }
    }
    fn ano(&self) -> i64 {
        Data(self.local.div_euclid(86400)).partes().0
    }
    fn hoje(&self) -> String {
        Data(self.local.div_euclid(86400)).iso()
    }
    /// Relogio de parede -> segundos reais (o fuso e a diferenca entre as duas escalas agora).
    fn para_epoch(&self, local: i64) -> f64 {
        local as f64 - (self.local as f64 - self.epoch).round()
    }
}

/// "dd/mm HH:MM:SS" (so os 14 primeiros caracteres da linha) -> relogio de parede deste ano; 0 se nao for hora.
fn hora_do_log(txt: &str, ano: i64) -> i64 {
    let t: String = txt.chars().take(14).collect();
    let Some((dm, hms)) = t.trim().split_once(' ') else { return 0 };
    let Some((d, m)) = dm.split_once('/') else { return 0 };
    let n = |s: &str| s.trim().parse::<i64>().ok();
    let hms: Vec<Option<i64>> = hms.split(':').map(n).collect();
    let (Some(d), Some(m), [Some(h), Some(mi), Some(s)]) = (n(d), n(m), hms.as_slice()) else { return 0 };
    Data::civil(ano, m, d).map_or(0, |dt| dt.0 * 86400 + h * 3600 + mi * 60 + s)
}

fn ler_json(p: &Path) -> Option<Value> {
    serde_json::from_str(std::fs::read_to_string(p).ok()?.trim_start_matches('\u{feff}')).ok()
}

fn anexar(p: &Path, texto: &str) {
    use std::io::Write;
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
        let _ = f.write_all(texto.as_bytes());
    }
}

fn texto<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}

fn lista(v: &Value) -> &[Value] {
    v.as_array().map_or(&[], |a| a.as_slice())
}

/// Primeiro item de uma lista como texto (numero da cidade em [n, nome, ...]).
fn primeiro(v: &Value) -> Option<String> {
    match v.as_array()?.first()? {
        Value::String(x) => Some(x.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn arredonda(x: f64, casas: i32) -> f64 {
    let f = 10f64.powi(casas);
    (x * f).round() / f
}

fn cortar_fim(s: &str, n: usize) -> String {
    let c: Vec<char> = s.chars().collect();
    c[c.len().saturating_sub(n)..].iter().collect()
}

/// Gravacao com arquivo ao lado e troca; no Windows o rename falha se alguem esta lendo, entao tenta de novo.
fn gravar_json(arq: &Path, v: &Value, espera: Duration) -> Result<(), String> {
    let e = |e: std::io::Error| format!("{}: {e}", arq.display());
    std::fs::create_dir_all(arq.parent().unwrap_or(Path::new("."))).map_err(e)?;
    let tmp = arq.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&tmp, serde_json::to_string_pretty(v).map_err(|e| e.to_string())?).map_err(e)?;
    for i in 0..20u32 {
        if std::fs::rename(&tmp, arq).is_ok() {
            return Ok(());
        }
        std::thread::sleep(espera * (i + 1));
    }
    let _ = std::fs::remove_file(&tmp);
    Err(format!("arquivo ocupado: {}", arq.display()))
}

fn contar_pdfs(p: &Path) -> usize {
    std::fs::read_dir(p).into_iter().flatten().flatten().map(|e| {
        let c = e.path();
        if c.is_dir() { contar_pdfs(&c) } else { c.extension().is_some_and(|x| x.eq_ignore_ascii_case("pdf")) as usize }
    }).sum()
}

// ---------------------------------------------------------------- o conjunto dos tres vigias

pub struct Vigias {
    pub cfg: Config,
    exec: Box<Exec>,
    /// A pasta que tem `parceiro/` dentro (o `dados` do link_lote_parceiro): a pai da pasta da esteira.
    dados_lote: PathBuf,
}

impl Vigias {
    pub fn novo(cfg: Config, exec: Box<Exec>) -> Vigias {
        let dados_lote = cfg.pasta.parent().map(Path::to_path_buf).unwrap_or_else(|| cfg.pasta.clone());
        Vigias { cfg, exec, dados_lote }
    }

    fn dorme(&self, s: u64) {
        std::thread::sleep(self.cfg.segundo * s as u32);
    }

    fn arq(&self, nome: &str) -> PathBuf {
        self.cfg.pasta.join(nome)
    }

    fn comando(&self) -> (i64, bool, bool) {
        let c = ler_json(&self.arq("comando_esteira.json")).unwrap_or(Value::Null);
        (c["meta"].as_i64().unwrap_or(50), c["parado"].as_bool().unwrap_or(false), c["so_fila"].as_bool().unwrap_or(false))
    }

    // ============================================================ painel (vigia_painel.py)

    fn fila_md(&self) -> String {
        let arq = std::env::var_os("BRANIAC_FILA").map(PathBuf::from).unwrap_or_else(|| self.cfg.pacote.join("videos").join("_fila-parceiro.md")); // a mesma fonte do municipios.py
        std::fs::read_to_string(arq).unwrap_or_default()
    }

    /// As capitais do painel (numero, cidade, etapa, detalhe, desde), na ordem do capitais.json.
    fn capitais(&self, d: &Value) -> Vec<Value> {
        let Some(Value::Object(reg)) = ler_json(&self.arq("capitais.json")) else { return vec![] };
        reg.iter().map(|(n, r)| {
            let c = &d["cidades"][n.as_str()];
            json!({ "numero": n, "cidade": r["cidade"], "etapa": if texto(c, "etapa").is_empty() { "fila" } else { texto(c, "etapa") },
                    "detalhe": texto(c, "detalhe"), "desde": c["desde"] })
        }).collect()
    }

    /// Todas as cidades que a esteira ja tocou, na ordem, com quantos PDFs do TCM estao baixados e a etapa.
    fn sequencia(&self, linhas: &[String], o: &Value, d: &Value) -> Vec<Value> {
        let re = Regex::new(r"(?:COMEÇANDO|PREPARANDO|VOZ\+RENDER) (\d+) (.+)$").expect("regex fixa");
        let mut ordem: Vec<(String, String)> = vec![];
        let mut add = |n: String, nome: String| {
            if !ordem.iter().any(|(x, _)| *x == n) {
                ordem.push((n, nome));
            }
        };
        for l in linhas {
            if let Some(m) = re.captures(l) {
                add(m[1].to_string(), m[2].trim().to_string());
            }
        }
        for x in lista(&o["preparadas"]) {
            if let (Some(n), Some(nome)) = (primeiro(x), x.get(1).and_then(Value::as_str)) {
                add(n, nome.to_string());
            }
        }
        for (n, c) in d["cidades"].as_object().into_iter().flatten() {
            if !texto(c, "cidade").is_empty() {
                add(n.clone(), texto(c, "cidade").to_string());
            }
        }
        ordem.sort_by_key(|(n, _)| if n.chars().all(|c| c.is_ascii_digit()) { n.parse::<i64>().unwrap_or(9999) } else { 9999 });
        let pend: BTreeSet<&str> = lista(&d["pendencias"]).iter().map(|p| texto(p, "numero")).collect();
        // enviados pela esteira + as pastas NNN-CIDADE que a entrega ao site monta
        let mut no_site: BTreeSet<String> = lista(&o["enviados"]).iter().filter_map(Value::as_str).map(|x| x.split(' ').next().unwrap_or("").to_string()).collect();
        for e in std::fs::read_dir(&self.cfg.site).into_iter().flatten().flatten() {
            let nome = e.file_name().to_string_lossy().into_owned();
            if e.path().is_dir() && nome.chars().take(3).count() == 3 && nome.chars().take(3).all(|c| c.is_ascii_digit()) {
                no_site.insert(nome.split('-').next().unwrap_or("").to_string());
            }
        }
        let prep: BTreeSet<String> = lista(&o["preparadas"]).iter().filter_map(primeiro).collect();
        let voz_ok: BTreeSet<String> = lista(&o["voz_ok"]).iter().filter_map(primeiro).collect();
        let prontos: BTreeSet<&str> = lista(&o["prontos"]).iter().filter_map(Value::as_str).collect();
        ordem.into_iter().map(|(n, nome)| {
            let pasta = self.cfg.pacote.join(format!("RECEITAS-DESPESAS-{}", ascii(&nome).to_uppercase().replace(' ', "-")));
            let mut etapa = texto(&d["cidades"][n.as_str()], "etapa").to_string();
            if no_site.contains(&n) {
                etapa = "no_site".into(); // mostrar quais ja foram para o site do Parceiro
            } else if prontos.contains(n.as_str()) {
                etapa = "pronto".into();
            } else if etapa == "render" {
                // renderizando agora: fica como esta (tem que aparecer piscando)
            } else if voz_ok.contains(&n) {
                etapa = "esperando_render".into();
            } else if pend.contains(n.as_str()) {
                etapa = "guia".into();
            } else if prep.contains(&n) && etapa != "voz" {
                etapa = "esperando_voz".into();
            }
            json!({ "numero": n, "cidade": nome, "pdfs": contar_pdfs(&pasta), "etapa": etapa })
        }).collect()
    }

    /// O resumo "esteira" que vai para dentro do lote.json.
    pub fn resumo(&self, rel: Relogio) -> Value {
        let mut o = ler_json(&self.arq("orquestrador.json")).filter(Value::is_object).unwrap_or_else(|| json!({}));
        for k in ["prontos", "feitas", "enviados", "lote_site", "preparadas", "voz_ok"] {
            if !o[k].is_array() {
                o[k] = json!([]);
            }
        }
        let linhas: Vec<String> = std::fs::read(self.arq("orquestrador.log")).map(|b| String::from_utf8_lossy(&b).lines().map(String::from).collect()).unwrap_or_default();
        let d = crate::link_lote_parceiro::ler(&self.dados_lote);
        let cid = &d["cidades"];
        let re_atual = Regex::new(r"(?:COMEÇANDO|PREPARANDO|VOZ\+RENDER) (\d+) (.+)$").expect("regex fixa");
        let feitas: BTreeSet<&str> = lista(&o["feitas"]).iter().filter_map(Value::as_str).collect();
        let mut atual = Value::Null;
        if let Some(m) = linhas.iter().rev().find_map(|l| re_atual.captures(l)) {
            let n = &m[1];
            if !feitas.contains(n) {
                let c = &cid[n];
                atual = json!({ "numero": n, "cidade": &m[2], "etapa": c["etapa"], "detalhe": c["detalhe"], "desde": c["desde"] });
            }
        }
        let inicio = linhas.iter().find(|l| l.contains("COMEÇANDO"));
        let mut t0 = inicio.map(|l| hora_do_log(l, rel.ano())).filter(|t| *t > 0).map(|t| rel.para_epoch(t));
        let caps = if self.comando().2 { self.capitais(&d) } else { vec![] };
        let mut oc = o.clone(); // o que os contadores contam: no modo capitais, so os numeros das capitais
        if !caps.is_empty() {
            let so: BTreeSet<&str> = caps.iter().map(|c| texto(c, "numero")).collect();
            oc["prontos"] = json!(lista(&o["prontos"]).iter().filter(|n| so.contains(n.as_str().unwrap_or(""))).collect::<Vec<_>>());
            oc["feitas"] = json!(lista(&o["feitas"]).iter().filter(|n| so.contains(n.as_str().unwrap_or(""))).collect::<Vec<_>>());
            oc["lote_site"] = json!(lista(&o["lote_site"]).iter().filter(|x| primeiro(x).is_some_and(|n| so.contains(n.as_str()))).collect::<Vec<_>>());
            oc["enviados"] = json!(lista(&o["enviados"]).iter().filter(|x| so.contains(x.as_str().unwrap_or("").split(' ').next().unwrap_or(""))).collect::<Vec<_>>());
            let re_n = Regex::new(r" (\d+) ").expect("regex fixa");
            let t0c = linhas.iter().find(|l| re_n.captures(l).is_some_and(|m| so.contains(&m[1])));
            t0 = t0c.map(|l| hora_do_log(l, rel.ano())).filter(|t| *t > 0).map(|t| rel.para_epoch(t));
            // "Fazendo agora" = a capital mais adiantada que ainda nao ficou pronta; a preparacao do SICONFI e rapida e nao conta
            const ORDEM: [&str; 5] = ["render", "voz", "roteiro", "levantamento", "baixando_tcm"];
            let pr: BTreeSet<&str> = lista(&oc["prontos"]).iter().filter_map(Value::as_str).collect();
            let vivas = caps.iter().filter(|c| !pr.contains(texto(c, "numero")) && ORDEM.contains(&texto(c, "etapa")));
            atual = vivas.min_by_key(|c| (ORDEM.iter().position(|e| *e == texto(c, "etapa")), texto(c, "numero").parse::<i64>().unwrap_or(0)))
                .map_or(Value::Null, |a| json!({ "numero": a["numero"], "cidade": a["cidade"], "etapa": a["etapa"], "detalhe": a["detalhe"], "desde": a["desde"] }));
        }
        let prontos_oc: Vec<String> = lista(&oc["prontos"]).iter().filter_map(|x| x.as_str().map(String::from)).collect();
        let prontos = prontos_oc.len();
        // fracao do video em producao: peso por etapa; no render, sobe com as partes ja renderizadas (videos/NNN/render.log)
        let peso = |e: &str| match e { "reservada" => 0.0, "baixando_tcm" => 0.05, "levantamento" => 0.15, "roteiro" => 0.30, "voz" => 0.45, "render" => 0.75, "pronto" | "no_site" => 1.0, _ => 0.0 };
        let etapa_atual = texto(&atual, "etapa");
        let mut frac = peso(etapa_atual);
        if etapa_atual == "voz" {
            let desde = atual["desde"].as_f64().unwrap_or(rel.epoch);
            frac = 0.45 + 0.25 * ((rel.epoch - desde) / 1500.0).min(1.0); // voz + conferencia levam ~25 min
        }
        if etapa_atual == "render" {
            if let Ok(rl) = std::fs::read(self.cfg.pacote.join("videos").join(texto(&atual, "numero")).join("render.log")) {
                let rl = String::from_utf8_lossy(&rl);
                let et: Vec<i64> = Regex::new(r"=+ (\d)/5 ").expect("regex fixa").captures_iter(&rl).filter_map(|c| c[1].parse().ok()).collect();
                let pt: Vec<(i64, i64, i64)> = Regex::new(r"Parte (\d+) de (\d+): (\d+)%").expect("regex fixa").captures_iter(&rl)
                    .filter_map(|c| Some((c[1].parse().ok()?, c[2].parse().ok()?, c[3].parse().ok()?))).collect();
                let e5 = et.last().copied().unwrap_or(1);
                let dentro = match pt.last() {
                    Some(&(p, t, pc)) if e5 == 2 && t > 0 => Some(((p - 1) as f64 + pc as f64 / 100.0) / t as f64),
                    Some(_) if e5 == 2 => None,
                    _ => Some(if e5 > 2 { 1.0 } else { 0.0 }),
                };
                if let Some(dentro) = dentro {
                    frac = 0.70 + 0.30 * (((e5 - 1) as f64 + dentro) / 5.0).min(1.0);
                }
            }
        }
        let horas = t0.map_or(0.0, |t| (rel.epoch - t) / 3600.0);
        let ritmo = (horas > 0.5 && prontos > 0).then(|| prontos as f64 / horas);
        let fila = self.fila_md();
        let m = crate::link_municipios::resumo(&self.dados_lote, &fila, rel.epoch);
        let nome = |n: &str| texto(&cid[n], "cidade").to_string();
        // os 50 quadrados = as cidades DESTA leva em sequencia, cada uma com a sua etapa; o resto completa com a fila
        let seq = self.sequencia(&linhas, &o, &d);
        let mut leva: BTreeSet<String> = ["feitas", "prontos"].iter().flat_map(|k| lista(&o[*k]).iter().filter_map(|x| x.as_str().map(String::from))).collect();
        leva.extend(["preparadas", "voz_ok"].iter().flat_map(|k| lista(&o[*k]).iter().filter_map(primeiro)));
        if let Some(n) = primeiro(&o["atual"]) {
            leva.insert(n);
        }
        let mut vagas: Vec<Value> = seq.iter().filter(|c| leva.contains(texto(c, "numero"))).take(META).map(|c| { let mut c = c.clone(); c["estado"] = c["etapa"].clone(); c }).collect();
        let falta: Vec<String> = lista(&m["falta"]).iter().filter_map(|x| x.as_str().map(String::from)).collect();
        let usados: BTreeSet<String> = vagas.iter().map(|v| texto(v, "numero").to_string()).collect();
        for item in &falta {
            if vagas.len() >= META {
                break;
            }
            let (num, nm) = item.split_once(' ').unwrap_or((item, ""));
            if !usados.contains(num) {
                vagas.push(json!({ "numero": num, "cidade": nm, "estado": "fila" }));
            }
        }
        let mut meta_l = META;
        if !caps.is_empty() {
            // esteira das capitais: os quadrados e a porcentagem sao as capitais, nao a leva de 50 da Bahia
            let por: HashMap<&str, &Value> = seq.iter().map(|c| (texto(c, "numero"), c)).collect();
            let pronto_o: BTreeSet<&str> = lista(&o["prontos"]).iter().filter_map(Value::as_str).collect();
            vagas = caps.iter().map(|c| {
                let n = texto(c, "numero");
                let e = por.get(n).map(|x| texto(x, "etapa")).filter(|e| !e.is_empty()).unwrap_or(if pronto_o.contains(n) { "pronto" } else { "fila" });
                json!({ "numero": n, "cidade": c["cidade"], "estado": e })
            }).collect();
            meta_l = vagas.len();
        }
        let peso_v = |e: &str| match e { "baixando_tcm" => 0.05, "levantamento" | "guia" => 0.15, "roteiro" => 0.30, "esperando_voz" => 0.45, "voz" => 0.55, "esperando_render" => 0.70, "render" => 0.75, "pronto" | "no_site" => 1.0, _ => 0.0 };
        let mut pct = 100.0 * vagas.iter().map(|v| peso_v(texto(v, "estado"))).sum::<f64>() / meta_l as f64;
        // o painel mostra so o LOTE ATUAL (o ultimo "mais N"); os quadrados sao as cidades desse lote
        let lote = crate::link_lote_parceiro::painel_lote(&self.dados_lote, &seq, if caps.is_empty() { &falta } else { &[] }, true).unwrap_or_else(|e| {
            eprintln!("esteira_vigias: lote {}", e.chars().take(200).collect::<String>());
            None
        });
        if let (Some(l), true) = (&lote, caps.is_empty()) {
            vagas = lista(&l["cidades"]).iter().map(|c| json!({ "numero": c["numero"], "cidade": c["cidade"], "estado": c["etapa"] })).collect();
            meta_l = vagas.len();
            pct = l["pct"].as_f64().unwrap_or(0.0);
        }
        let prontos_s: Vec<&str> = prontos_oc.iter().map(String::as_str).collect();
        let pdfs = crate::link_lote_parceiro::checklist_pdfs(&self.dados_lote, &seq, &prontos_s, &linhas.join("\n"));
        let pendencias: Vec<Value> = lista(&oc["feitas"]).iter().filter_map(Value::as_str).filter(|n| !prontos_oc.iter().any(|p| p == n)).map(|n| {
            let motivo = lista(&d["pendencias"]).iter().find(|p| texto(p, "numero") == n).map_or("", |p| texto(p, "motivo"));
            json!({ "numero": n, "cidade": nome(n), "motivo": motivo })
        }).collect();
        let faltam = match (&lote, caps.is_empty()) {
            (Some(l), true) => l["faltam"].as_f64().unwrap_or(0.0),
            _ => (meta_l as f64 - prontos as f64).max(0.0),
        };
        let n_de = |k: &str| m[k].as_i64().unwrap_or(0);
        json!({
            "meta": meta_l, "progresso_pct": arredonda(pct, 2), "fracao_atual": arredonda(frac, 3),
            "prontos": prontos_oc.iter().map(|n| json!({ "numero": n, "cidade": nome(n) })).collect::<Vec<_>>(),
            "pendencias_esteira": pendencias,
            "aguardando_site": lista(&oc["lote_site"]).iter().map(|x| json!({ "numero": primeiro(x), "cidade": x.get(1) })).collect::<Vec<_>>(),
            "enviados_site": oc["enviados"], "atual": atual, "inicio": t0, "ritmo_por_hora": ritmo,
            "previsao_horas": ritmo.map(|r| faltam / r),
            "cidades_bahia": { "total": m["total"], "finalizadas": n_de("n_finalizado") + n_de("n_postado"), "entregues": n_de("n_entregue"),
                               "producao": n_de("n_producao"), "faltam": n_de("n_falta") },
            "cidades_seq": seq, "slots": vagas, "vagas": vagas, "lote": lote, "pdfs_site": pdfs,
            "log": linhas[linhas.len().saturating_sub(12)..], "atualizado": rel.epoch,
        })
    }

    /// Uma volta do painel: escreve so a chave "esteira", relendo o lote.json na hora (a esteira tambem grava nele).
    pub fn volta_painel(&self, rel: Relogio) -> Result<(), String> {
        let r = self.resumo(rel);
        for _ in 0..3 {
            let mut d = crate::link_lote_parceiro::ler(&self.dados_lote);
            d["esteira"] = r.clone();
            gravar_json(&self.dados_lote.join("parceiro").join("lote.json"), &d, self.cfg.segundo / 4)?;
            if crate::link_lote_parceiro::ler(&self.dados_lote)["esteira"]["atualizado"] == r["atualizado"] {
                break;
            }
        }
        Ok(())
    }

    // ============================================================ mp4 (vigia_mp4.py)

    fn ed(&self) -> PathBuf {
        self.cfg.estudio.join("edicoes")
    }

    fn guarda(&self) -> PathBuf {
        self.cfg.pacote.join("VIDEOS-PRONTOS")
    }

    /// Numero da cidade pelo nome da pasta ("NNN. Nome" ou "CONCLUÍDO NNN. Nome").
    fn numero_da_pasta(p: &Path) -> Option<String> {
        let nome = p.parent()?.file_name()?.to_string_lossy().into_owned();
        let re = Regex::new(r"^(?:CONCLUÍDO )?(\d+)\.").expect("regex fixa");
        re.captures(&nome).map(|m| m[1].to_string())
    }

    /// So copia depois que o Lupa marcar a cidade como "pronto" (depois do render, do volume e das copias de fontes).
    fn braniac_diz_pronto(&self, p: &Path) -> bool {
        let Some(n) = Self::numero_da_pasta(p) else { return false };
        let Some(lote) = ler_json(&self.arq("lote.json")) else { return false };
        texto(&lote["cidades"][n.as_str()], "etapa") == "pronto" && !self.cfg.pacote.join(".render-trava").exists()
    }

    /// Video finalizado (no site, apagado do PC) nao ganha espelho, backup nem volta do espelho.
    fn finalizado(&self, p: &Path) -> bool {
        let Some(n) = Self::numero_da_pasta(p) else { return false };
        match ler_json(&self.arq("finalizados.json")) {
            Some(Value::Object(o)) => o.contains_key(&n),
            Some(Value::Array(a)) => a.iter().any(|x| x.as_str() == Some(n.as_str())),
            _ => false,
        }
    }

    /// So o video final da pasta (NOME.mp4), nao .lim.mp4 nem copias de trabalho: {caminho: tamanho}.
    fn finais(&self) -> HashMap<PathBuf, u64> {
        let mut out = HashMap::new();
        for pasta in std::fs::read_dir(self.ed()).into_iter().flatten().flatten().filter(|e| e.path().is_dir()) {
            for f in std::fs::read_dir(pasta.path()).into_iter().flatten().flatten() {
                let (c, nome) = (f.path(), f.file_name().to_string_lossy().to_lowercase());
                if c.is_file() && nome.ends_with(".mp4") && !nome.ends_with(".lim.mp4") {
                    out.insert(c, std::fs::metadata(&f.path()).map(|m| m.len()).unwrap_or(0));
                }
            }
        }
        out
    }

    fn sem_prefixo(p: &Path) -> String {
        let n = p.parent().and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        n.strip_prefix("CONCLUÍDO ").map(String::from).unwrap_or(n)
    }

    fn espelho(&self, p: &Path) -> PathBuf {
        self.guarda().join(Self::sem_prefixo(p)).join(p.file_name().unwrap_or_default())
    }

    fn processos(&self) -> String {
        let ps = "Get-CimInstance Win32_Process | Where-Object { $_.Name -match 'python|node|bash|powershell|cmd' } | ForEach-Object { $_.ProcessId.ToString() + ' ' + $_.CommandLine }";
        (self.exec)(&Cmd { prog: "powershell".into(), args: vec!["-NoProfile".into(), "-Command".into(), ps.into()], cwd: self.cfg.raiz.clone(), limite: Duration::from_secs(60), arquivo: None }).texto
    }

    /// Copia o video final e os arquivos da pasta (capa, legenda, fontes, roteiro) para o E:. Nunca sobrescreve: versao nova ganha outro nome.
    /// A trava de escrita do backup esta desligada (TRAVA_DESLIGADA no Python, 15/11): so copia.
    fn backup(&self, p: &Path, rel: &Agora) -> Result<(), String> {
        let (pasta, e) = (p.parent().ok_or("sem pasta")?, |e: std::io::Error| e.to_string());
        let dest = self.cfg.backup.join(Self::sem_prefixo(p));
        std::fs::create_dir_all(&dest).map_err(e)?;
        let reg = self.guarda().join("backup-copiados.json"); // sem isto copiava o mesmo video a cada minuto
        let mut feitos = ler_json(&reg).filter(Value::is_object).unwrap_or_else(|| json!({}));
        let dest_nome = dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut arquivos = vec![p.to_path_buf()];
        let mut extras: Vec<PathBuf> = std::fs::read_dir(pasta).map_err(e)?.flatten().map(|x| x.path())
            .filter(|f| f.is_file() && f.extension().is_some_and(|x| ["jpg", "txt", "md"].contains(&x.to_string_lossy().to_lowercase().as_str()))).collect();
        extras.sort();
        arquivos.extend(extras);
        for f in arquivos {
            let md = std::fs::metadata(&f).map_err(e)?;
            let nome = f.file_name().unwrap_or_default().to_string_lossy().into_owned();
            let (stem, sufixo) = (f.file_stem().unwrap_or_default().to_string_lossy().into_owned(), f.extension().map(|x| format!(".{}", x.to_string_lossy())).unwrap_or_default());
            let mtime = md.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
            let (chave, assin) = (format!("{dest_nome}/{nome}"), format!("{}:{mtime}", md.len()));
            if feitos[&chave] == json!(assin) {
                continue;
            }
            // ja existe copia com o mesmo tamanho (qualquer versao)? entao e o mesmo arquivo: so registra
            let igual = std::fs::read_dir(&dest).map_err(e)?.flatten().any(|x| {
                let n = x.file_name().to_string_lossy().into_owned();
                n.starts_with(&stem) && n.ends_with(&sufixo) && x.metadata().is_ok_and(|m| m.len() == md.len())
            });
            if !igual {
                let mut alvo = dest.join(&nome);
                if alvo.exists() {
                    alvo = dest.join(format!("{stem} (versão {} {:02}h{:02}){sufixo}", rel.data.dia_mes().replace('/', "-"), rel.hora, rel.min));
                }
                std::fs::copy(&f, &alvo).map_err(e)?;
            }
            feitos[&chave] = json!(assin);
        }
        std::fs::write(&reg, serde_json::to_string_pretty(&feitos).map_err(|x| x.to_string())?).map_err(e)
    }

    /// Uma volta (5 s). `est` guarda o que a volta anterior viu; devolve nada, erro vira linha em sumicos.log.
    pub fn volta_mp4(&self, est: &mut EstadoMp4) {
        let log = self.guarda().join("sumicos.log");
        let a = crate::tempo::agora();
        let carimbo = format!("{} {:02}:{:02}:{:02}", a.data.dia_mes(), a.hora, a.min, a.seg);
        let erro = |x: &dyn std::fmt::Debug| anexar(&log, &format!("{:02}:{:02}:{:02} erro {x:?}\n", a.hora, a.min, a.seg));
        let agora: HashMap<PathBuf, u64> = self.finais().into_iter().filter(|(p, _)| !self.finalizado(p)).collect();
        for (p, &s) in &agora {
            if est.tam.get(p) == Some(&s) && s > 0 && self.braniac_diz_pronto(p) {
                let e = self.espelho(p);
                if !e.exists() || std::fs::metadata(&e).map(|m| m.len()).unwrap_or(0) != s {
                    let _ = std::fs::create_dir_all(e.parent().unwrap_or(Path::new(".")));
                    if e.exists() {
                        // video refeito: a versao anterior fica guardada ao lado
                        let ext = e.extension().map(|x| format!(".{}", x.to_string_lossy())).unwrap_or_default();
                        let ant = e.with_file_name(format!("{} (anterior {} {:02}h{:02}){ext}", e.file_stem().unwrap_or_default().to_string_lossy(), a.data.dia_mes().replace('/', "-"), a.hora, a.min));
                        if let Err(x) = std::fs::rename(&e, ant).and_then(|_| std::fs::hard_link(p, &e)) {
                            erro(&x);
                        }
                    } else if let Err(x) = std::fs::hard_link(p, &e) {
                        erro(&x);
                    }
                }
                if let Err(x) = self.backup(p, &a) {
                    anexar(&log, &format!("{carimbo} backup E: falhou {}: {x}\n", p.file_name().unwrap_or_default().to_string_lossy()));
                }
            }
            est.tam.insert(p.clone(), s);
        }
        let sumiram: Vec<PathBuf> = est.antes.keys().filter(|p| !agora.contains_key(*p)).cloned().collect();
        for p in sumiram {
            if self.finalizado(&p) {
                continue;
            }
            let e = self.espelho(&p);
            anexar(&log, &format!("\n=== {carimbo} SUMIU {}\n{}\n", p.display(), self.processos()));
            let apagar = self.cfg.pacote.join("APAGAR-FUTURAMENTE");
            let nome = p.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
            let mut movido = apagar.exists() && tem_arquivo(&apagar, &nome);
            // pasta renomeada depois do site: e o mesmo video
            movido = movido || self.ed().join(format!("CONCLUÍDO {}", p.parent().and_then(Path::file_name).unwrap_or_default().to_string_lossy())).exists();
            if e.exists() && !p.exists() && !movido {
                let _ = std::fs::create_dir_all(p.parent().unwrap_or(Path::new(".")));
                if std::fs::hard_link(&e, &p).is_ok() {
                    anexar(&log, &format!("restaurado do espelho: {}\n", p.display()));
                }
            }
        }
        est.antes = self.finais();
    }

    // ============================================================ esteira (vigia_esteira.py)

    fn linhas_log(&self, n: usize) -> Vec<String> {
        let l: Vec<String> = std::fs::read(self.arq("orquestrador.log")).map(|b| String::from_utf8_lossy(&b).lines().map(String::from).collect()).unwrap_or_default();
        l[l.len().saturating_sub(n)..].to_vec()
    }

    fn estado_vigia(&self) -> Value {
        let mut e = ler_json(&self.arq("vigia_esteira.json")).filter(Value::is_object).unwrap_or_else(|| json!({}));
        for k in ["vistos", "chamadas"] {
            if !e[k].is_object() {
                e[k] = json!({});
            }
        }
        e
    }

    /// Uma volta (60 s): acusa o que achar e, dentro dos tetos, chama o agente. Devolve as ocorrencias novas.
    pub fn volta_esteira(&self, rel: Relogio, agente: &dyn Fn(&str) -> (i32, String)) -> Vec<(String, String)> {
        let (meta, parado, _) = self.comando();
        let Some(orq) = ler_json(&self.arq("orquestrador.json")) else { return vec![] };
        if parado || lista(&orq["prontos"]).len() as i64 >= meta {
            return vec![];
        }
        let linhas = self.linhas_log(400);
        let achados = achar(&linhas, &orq, rel.local, rel.ano());
        let mut est = self.estado_vigia();
        let novos: Vec<(String, String)> = achados.into_iter().filter(|(a, _)| rel.epoch - est["vistos"][a.as_str()].as_f64().unwrap_or(0.0) > ESPERA_H * 3600.0).collect();
        if novos.is_empty() {
            return novos;
        }
        let alertas = self.arq("alertas_vigia.jsonl");
        let hoje = rel.hoje();
        anexar(&alertas, &(json!({ "t": rel.epoch, "achados": novos.iter().map(|(_, t)| t).collect::<Vec<_>>() }).to_string() + "\n"));
        for (a, _) in &novos {
            est["vistos"][a.as_str()] = json!(rel.epoch);
        }
        let feitas_hoje = est["chamadas"][hoje.as_str()].as_i64().unwrap_or(0);
        if feitas_hoje < TETO_DIA {
            est["chamadas"] = json!({ hoje: feitas_hoje + 1 }); // grava antes de chamar: se o agente travar, a conta ja foi
            let _ = std::fs::write(self.arq("vigia_esteira.json"), est.to_string());
            let (cod, txt) = agente(&prompt(&novos, &linhas, &orq));
            anexar(&alertas, &(json!({ "t": SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64()), "claude": cod, "resposta": cortar_fim(&txt, 900) }).to_string() + "\n"));
        } else {
            let _ = std::fs::write(self.arq("vigia_esteira.json"), est.to_string());
        }
        novos
    }
}

/// O que a volta do mp4 lembra da anterior.
#[derive(Default)]
pub struct EstadoMp4 {
    pub antes: HashMap<PathBuf, u64>,
    pub tam: HashMap<PathBuf, u64>,
}

/// Tem arquivo com esse nome (minusculo) em algum lugar da pasta?
fn tem_arquivo(pasta: &Path, nome: &str) -> bool {
    std::fs::read_dir(pasta).into_iter().flatten().flatten().any(|e| {
        let c = e.path();
        if c.is_dir() { tem_arquivo(&c, nome) } else { e.file_name().to_string_lossy().to_lowercase() == nome }
    })
}

/// Ocorrencias [(assinatura, texto)] a partir do log e do estado. Pura: `agora` no relogio de parede, `ano` o do relogio.
pub fn achar(linhas: &[String], orq: &Value, agora: i64, ano: i64) -> Vec<(String, String)> {
    let re_erro = Regex::new(r"^(\d+/\d+ \d+:\d+:\d+) (\d{3,4}) (voz reprovada \d.*|erro inesperado.*|download falhou.*|FALHOU.*)$").expect("regex fixa");
    let re_voz = Regex::new(r"voz reprovada \d").expect("regex fixa");
    let re_prog = Regex::new(r"VOZ PRONTA|RENDER|PRONTO|PREPARANDO|SITE |FASE:").expect("regex fixa");
    let mut out = vec![];
    let mut erros: Vec<((String, String), usize)> = vec![];
    for l in linhas {
        let Some(m) = re_erro.captures(l) else { continue };
        if agora - hora_do_log(&m[1], ano) < 3600 {
            let t = re_voz.replace_all(&m[3], "voz reprovada");
            let chave = (m[2].to_string(), t.chars().take(90).collect::<String>());
            match erros.iter_mut().find(|(k, _)| *k == chave) {
                Some((_, n)) => *n += 1,
                None => erros.push((chave, 1)),
            }
        }
    }
    for ((cid, t), n) in erros {
        if n >= LOOP_MIN {
            out.push((format!("loop-{cid}"), format!("cidade {cid}: o mesmo erro {n}x na última hora: {t}")));
        }
    }
    for (cid, n) in orq["voz_tentativas"].as_object().into_iter().flatten() {
        if n.as_i64().unwrap_or(0) >= 3 && cid.parse::<i64>().is_ok_and(|c| c >= 900) {
            out.push((format!("tent-{cid}"), format!("cidade {cid}: {n} rodadas de voz reprovadas")));
        }
    }
    let ult = linhas.iter().filter(|l| re_prog.is_match(l)).map(|l| hora_do_log(l, ano)).max().unwrap_or(0);
    if ult > 0 && (agora - ult) / 60 > SEM_AVANCO_MIN {
        let (d, s) = (Data(ult.div_euclid(86400)), ult.rem_euclid(86400));
        out.push(("sem-avanco".into(), format!("a esteira está sem avançar há {} min (último avanço {} {:02}:{:02})", (agora - ult) / 60, d.dia_mes(), s / 3600, s / 60 % 60)));
    }
    out
}

/// "['a', 'b']" como o Python escreve uma lista de textos.
fn lista_py(v: &[String]) -> String {
    format!("[{}]", v.iter().map(|x| format!("'{x}'")).collect::<Vec<_>>().join(", "))
}

/// O pedido ao agente. A esteira agora e conduzida pelo B.R.A.N.I em Rust; o orquestrador Python nao pode ser religado.
fn prompt(achados: &[(String, String)], linhas: &[String], orq: &Value) -> String {
    let nums = |k: &str| lista_py(&lista(&orq[k]).iter().filter_map(primeiro).collect::<Vec<_>>());
    let fase = if orq["fase"].is_null() { "None".to_string() } else { orq["fase"].as_str().map_or_else(|| orq["fase"].to_string(), String::from) };
    format!("VIGIA DA ESTEIRA DO PARCEIRO. A esteira agora é conduzida pelo B.R.A.N.I em Rust (E:\\braniac-rs\\src\\esteira_orq.rs, que grava o mesmo log e o mesmo estado: \
C:\\Users\\usuario\\Projetos\\Agente\\braniac\\data\\parceiro\\orquestrador.log e orquestrador.json). O orquestrador Python (orquestrador_50.py) NÃO deve ser religado. \
A esteira travou ou está em loop. Ocorrências:\n- {}\n\nFase: {fase}. Com voz pronta esperando render: {}. Na fila da voz: {}. Tentativas de voz: {}.\nÚltimas linhas do log:\n{}\n\n\
Faça: 1) ache a CAUSA RAIZ (reproduza o erro sem gastar cota de voz: rode o passo que quebra com traceback; ex.: preparar_parceiro.py ou parceiro_velocidade.py sem --voz-nova); \
2) corrija a causa NO CÓDIGO e rode os testes que tocar (cargo test em E:\\braniac-rs para o que for do B.R.A.N.I, usando outro CARGO_TARGET_DIR e nunca cargo build --release em target\\release, onde o servidor está rodando; \
braniac/tests para os scripts em Python); 3) se a cidade continuar impossível, tire-a da vez (resolver/pendência) para a fila andar. \
Regras: número só do tribunal de contas e só pelos programas (levantar.py, gera_cidade.py, gera_legenda.py): você NUNCA digita, arredonda nem edita à mão \
número em saida/, roteiro, legenda, folhas ou fontes.md (Dono, 17/11/2026); a auditoria sem IA (braniac/link/auditar.py, auditar.porta) barra o que não tiver prova, e \
ela é trava: não mexa nela; nada de publicar, enviar ao site, pagar ou mexer em conta; não reinicie a esteira sem necessidade; não toque nas travas. \
Termine com 3 linhas: causa, o que corrigiu, o que ficou pendente.",
        achados.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>().join("\n- "), nums("voz_ok"), nums("preparadas"),
        if orq["voz_tentativas"].is_null() { "None".to_string() } else { orq["voz_tentativas"].to_string() },
        linhas[linhas.len().saturating_sub(25)..].join("\n"))
}

/// Disco cheio derrubou renders (16/11, ENOSPC): o Remotion deixa um bundle de GBs no Temp a cada render. Com menos de 15 GB livres,
/// apaga os `remotion-*` com mais de `idade_s` segundos. `remove_dir_all` do Rust nao segue junção/link (os bundles tem ligacoes para as
/// pastas reais). Nunca robocopy /MIR. Devolve quantos apagou.
pub fn limpar_temp(temp: &Path, livre_gb: f64, idade_s: u64) -> usize {
    if livre_gb >= 15.0 {
        return 0;
    }
    let mut n = 0;
    for e in std::fs::read_dir(temp).into_iter().flatten().flatten() {
        let velho = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|d| d > Duration::from_secs(idade_s));
        if e.file_name().to_string_lossy().starts_with("remotion-") && velho {
            let _ = std::fs::remove_dir_all(e.path());
            n += 1;
        }
    }
    n
}

/// Espaco livre em C: em GB (Windows); fora dele, infinito (nao limpa nada).
#[cfg(windows)]
fn livre_c_gb() -> f64 {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetDiskFreeSpaceExW(dir: *const u16, livre: *mut u64, total: *mut u64, livre_total: *mut u64) -> i32;
    }
    let dir: Vec<u16> = "C:\\".encode_utf16().chain(std::iter::once(0)).collect();
    let (mut livre, mut total, mut lt) = (0u64, 0u64, 0u64);
    // SAFETY: ponteiros para variaveis locais vivas; a chamada so escreve nelas.
    if unsafe { GetDiskFreeSpaceExW(dir.as_ptr(), &mut livre, &mut total, &mut lt) } == 0 {
        return f64::INFINITY;
    }
    livre as f64 / 1e9
}

#[cfg(not(windows))]
fn livre_c_gb() -> f64 {
    f64::INFINITY
}

// ---------------------------------------------------------------- ligacao com o servidor

/// O agente de verdade: o escolhido em agente_dev (padrao `claude -p`), com permissao de editar e 30 min de prazo.
fn agente_real(dados: &Path, raiz: &Path, prompt: &str) -> (i32, String) {
    use crate::agente_dev::{self, Opcoes, Sistema};
    let env = |k: &str| std::env::var(k).ok();
    let which = |n: &str| crate::claude_cli::localizar().filter(|_| n.starts_with("claude")).map(|p| p.to_string_lossy().to_string());
    let guia = dados.join("AGENTE.md");
    let s = Sistema { env: &env, which: &which, executar: &crate::dono_agente::executar_processo, guia: &guia, raiz };
    agente_dev::rodar(&s, dados, prompt, &Opcoes { timeout_s: CHAMADA_S, editar: true, ..Opcoes::default() })
}

static NO_AR: OnceLock<()> = OnceLock::new();

/// Sobe os tres vigias, uma vez, cada um na sua thread. Erro (ou panico) numa volta vira registro e o laco segue.
pub fn garantir(dados: &Path) {
    if std::env::var("BRANIAC_VIGIAS").is_ok_and(|v| v == "0") {
        return;
    }
    NO_AR.get_or_init(|| {
        let v = Arc::new(Vigias::novo(Config::real(dados), Box::new(exec_real)));
        let dados = dados.to_path_buf();
        let lacos: [(&str, u64); 3] = [("painel", 20), ("mp4", 5), ("esteira", 60)];
        for (nome, espera) in lacos {
            let (v, dados) = (v.clone(), dados.clone());
            let _ = std::thread::Builder::new().name(format!("esteira-vigia-{nome}")).spawn(move || {
                let mut mp4 = EstadoMp4::default();
                let _ = std::fs::create_dir_all(&v.cfg.backup); // como o vigia_mp4: a pasta do backup existe desde o comeco
                let _ = std::fs::create_dir_all(v.guarda());
                loop {
                    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match nome {
                        "painel" => {
                            if let Err(e) = v.volta_painel(Relogio::agora()) {
                                eprintln!("esteira_vigias: painel {}", e.chars().take(200).collect::<String>());
                            }
                        }
                        "mp4" => v.volta_mp4(&mut mp4),
                        _ => {
                            limpar_temp(&std::env::temp_dir(), livre_c_gb(), 90 * 60);
                            v.volta_esteira(Relogio::agora(), &|p| agente_real(&dados, &v.cfg.raiz, p));
                        }
                    }));
                    if r.is_err() {
                        eprintln!("esteira_vigias: erro inesperado no vigia {nome}; segue");
                    }
                    v.dorme(espera);
                }
            });
        }
    });
}

/// Relogio de 60 s: so garante os tres vigias no ar (nao bloqueia: cada um tem a sua thread).
pub fn tick(dados: &Path) {
    garantir(dados);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::esteira_orq::Saida;
    use std::sync::Mutex;

    fn tmp(nome: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("braniac-esteira_vigias-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn cfg(base: &Path) -> Config {
        Config {
            pasta: base.join("parceiro"),
            pacote: base.join("pacote"),
            estudio: base.join("pacote").join("estudio-3d"),
            site: base.join("site"),
            envia: base.join("envia_site.py"),
            raiz: base.to_path_buf(),
            ajudante: base.join("passo_esteira.py"),
            backup: base.join("backup"),
            ligada: base.join("esteira_ligada"),
            python: "python".into(),
            segundo: Duration::from_millis(1),
        }
    }

    fn vigias(base: &Path) -> Vigias {
        std::fs::create_dir_all(base.join("parceiro")).unwrap();
        Vigias::novo(cfg(base), Box::new(|_| Saida { codigo: 0, texto: "123 python fake".into() }))
    }

    /// Relogio de parede fixo: 18/11/2026 12:00:00 (e o mesmo em segundos reais, fuso zero).
    fn rel() -> Relogio {
        let local = Data::civil(2026, 11, 18).unwrap().0 * 86400 + 12 * 3600;
        Relogio { epoch: local as f64, local }
    }

    fn linha(min_atras: i64, resto: &str) -> String {
        let t = rel().local - min_atras * 60;
        let (d, s) = (Data(t.div_euclid(86400)), t.rem_euclid(86400));
        format!("{} {:02}:{:02}:{:02} {resto}", d.dia_mes(), s / 3600, s / 60 % 60, s % 60)
    }

    fn escrever(p: &Path, t: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, t).unwrap();
    }

    // ---- esteira

    #[test]
    fn acha_loop_so_com_seis_na_ultima_hora() {
        let erro = |m| linha(m, "203 voz reprovada 4 (WER alto)");
        let seis: Vec<String> = (1..=6).map(|i| erro(i * 5)).chain([linha(1, "PRONTO 200")]).collect();
        let a = achar(&seis, &json!({}), rel().local, rel().ano());
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].0, "loop-203");
        assert!(a[0].1.contains("6x") && a[0].1.contains("voz reprovada (WER alto)"), "{}", a[0].1);
        // cinco nao e loop; erro de mais de uma hora atras nao conta
        let cinco: Vec<String> = (1..=5).map(|i| erro(i * 5)).chain([linha(1, "PRONTO 200")]).collect();
        assert!(achar(&cinco, &json!({}), rel().local, rel().ano()).is_empty());
        let velhos: Vec<String> = (1..=6).map(|i| erro(70 + i)).chain([linha(1, "PRONTO 200")]).collect();
        assert!(achar(&velhos, &json!({}), rel().local, rel().ano()).is_empty());
    }

    #[test]
    fn acha_tentativas_so_de_cidade_900_ou_mais_e_sem_avanco() {
        let orq = json!({ "voz_tentativas": { "203": 5, "901": 3, "902": 2 } });
        let a = achar(&[linha(1, "PRONTO 1")], &orq, rel().local, rel().ano());
        assert_eq!(a.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["tent-901"]);
        let parada = achar(&[linha(80, "RENDER 5")], &json!({}), rel().local, rel().ano());
        assert_eq!(parada[0].0, "sem-avanco");
        assert!(parada[0].1.contains("80 min"), "{}", parada[0].1);
        assert!(achar(&[linha(70, "RENDER 5")], &json!({}), rel().local, rel().ano()).is_empty());
        assert!(achar(&[], &json!({}), rel().local, rel().ano()).is_empty()); // sem nenhuma linha de progresso nao acusa
    }

    fn esteira_com(base: &Path, orq: Value, linhas: &[String]) -> Vigias {
        let v = vigias(base);
        escrever(&v.arq("orquestrador.json"), &orq.to_string());
        escrever(&v.arq("orquestrador.log"), &(linhas.join("\n") + "\n"));
        v
    }

    fn agente_falso() -> (Arc<Mutex<Vec<String>>>, impl Fn(&str) -> (i32, String) + Send + Sync) {
        let chamadas = Arc::new(Mutex::new(vec![]));
        let c = chamadas.clone();
        (chamadas, move |p: &str| {
            c.lock().unwrap().push(p.to_string());
            (0, "causa: x".to_string())
        })
    }

    #[test]
    fn chama_o_agente_uma_vez_por_ocorrencia_a_cada_3h() {
        let base = tmp("teto3h");
        let linhas = vec![linha(80, "RENDER 5")];
        let v = esteira_com(&base, json!({ "prontos": [], "fase": "voz" }), &linhas);
        let (chamadas, agente) = agente_falso();
        assert_eq!(v.volta_esteira(rel(), &agente).len(), 1);
        assert_eq!(chamadas.lock().unwrap().len(), 1);
        let p = chamadas.lock().unwrap()[0].clone();
        assert!(p.contains("esteira_orq.rs") && p.contains("NÃO deve ser religado") && !p.contains("orquestrador_50.py, log em"), "{p}");
        // dois minutos depois: a mesma ocorrencia nao chama de novo; 3 h e 1 min depois chama
        let mais = Relogio { epoch: rel().epoch + 120.0, local: rel().local + 120 };
        assert!(v.volta_esteira(mais, &agente).is_empty());
        let depois = Relogio { epoch: rel().epoch + 3.0 * 3600.0 + 60.0, local: rel().local + 3 * 3600 + 60 };
        assert_eq!(v.volta_esteira(depois, &agente).len(), 1);
        assert_eq!(chamadas.lock().unwrap().len(), 2);
        // o registro dos alertas tem o achado e a resposta do agente
        let jl = std::fs::read_to_string(v.arq("alertas_vigia.jsonl")).unwrap();
        assert!(jl.contains("sem avançar") && jl.contains("\"claude\":0") && jl.contains("causa: x"), "{jl}");
    }

    #[test]
    fn teto_de_tres_chamadas_por_dia_mas_o_alerta_fica_registrado() {
        let base = tmp("tetodia");
        let v = esteira_com(&base, json!({ "prontos": [] }), &[linha(80, "RENDER 5")]);
        escrever(&v.arq("vigia_esteira.json"), &json!({ "vistos": {}, "chamadas": { rel().hoje(): 3 } }).to_string());
        let (chamadas, agente) = agente_falso();
        assert_eq!(v.volta_esteira(rel(), &agente).len(), 1);
        assert!(chamadas.lock().unwrap().is_empty());
        assert!(std::fs::read_to_string(v.arq("alertas_vigia.jsonl")).unwrap().contains("sem avançar"));
        let est = v.estado_vigia();
        assert!(est["vistos"]["sem-avanco"].is_number() && est["chamadas"][rel().hoje().as_str()] == 3);
    }

    #[test]
    fn esteira_parada_meta_batida_ou_sem_estado_nao_vigia() {
        let (chamadas, agente) = agente_falso();
        let linhas = [linha(80, "RENDER 5")];
        let v = esteira_com(&tmp("parada"), json!({ "prontos": [] }), &linhas);
        escrever(&v.arq("comando_esteira.json"), r#"{"meta":50,"parado":true}"#);
        assert!(v.volta_esteira(rel(), &agente).is_empty());
        let v = esteira_com(&tmp("meta"), json!({ "prontos": ["1", "2"] }), &linhas);
        escrever(&v.arq("comando_esteira.json"), r#"{"meta":2}"#);
        assert!(v.volta_esteira(rel(), &agente).is_empty());
        let v = vigias(&tmp("semestado")); // sem orquestrador.json
        assert!(v.volta_esteira(rel(), &agente).is_empty());
        assert!(chamadas.lock().unwrap().is_empty());
    }

    #[test]
    fn limpa_temp_so_com_disco_apertado_e_so_os_velhos() {
        let t = tmp("temp");
        std::fs::create_dir_all(t.join("remotion-velho")).unwrap();
        std::fs::create_dir_all(t.join("outro-velho")).unwrap();
        std::thread::sleep(Duration::from_millis(1500));
        std::fs::create_dir_all(t.join("remotion-novo")).unwrap();
        assert_eq!(limpar_temp(&t, 50.0, 1), 0); // disco folgado: nada
        assert_eq!(limpar_temp(&t, 5.0, 1), 1);
        assert!(!t.join("remotion-velho").exists() && t.join("remotion-novo").exists() && t.join("outro-velho").exists());
    }

    // ---- mp4

    fn mp4_com(base: &Path, pasta: &str, etapa: &str) -> (Vigias, PathBuf) {
        let v = vigias(base);
        let p = v.ed().join(pasta).join("video.mp4");
        escrever(&p, "conteudo do video");
        escrever(&p.with_file_name("capa.jpg"), "capa");
        escrever(&v.arq("lote.json"), &json!({ "cidades": { "203": { "etapa": etapa } } }).to_string());
        (v, p)
    }

    #[test]
    fn mp4_so_e_guardado_depois_que_o_lupa_marca_pronto_e_estavel() {
        let base = tmp("mp4pronto");
        let (v, p) = mp4_com(&base, "203. Cidade", "render");
        let mut est = EstadoMp4::default();
        for _ in 0..3 {
            v.volta_mp4(&mut est); // ainda em render: nada de espelho nem backup
        }
        assert!(!v.espelho(&p).exists() && !base.join("backup").join("203. Cidade").exists());
        escrever(&v.arq("lote.json"), &json!({ "cidades": { "203": { "etapa": "pronto" } } }).to_string());
        v.volta_mp4(&mut est); // o tamanho ja era conhecido e estavel: espelho e backup
        assert_eq!(std::fs::read_to_string(v.espelho(&p)).unwrap(), "conteudo do video");
        let dest = base.join("backup").join("203. Cidade");
        assert!(dest.join("video.mp4").exists() && dest.join("capa.jpg").exists());
        assert!(v.guarda().join("backup-copiados.json").exists());
        // trava do render ligada: nao guarda video novo
        // estado novo (video recem-visto): a 1a volta so memoriza o tamanho, a 2a guarda
        let (v3, p3) = mp4_com(&tmp("mp4novo"), "203. Nova", "pronto");
        let mut e3 = EstadoMp4::default();
        v3.volta_mp4(&mut e3);
        assert!(!v3.espelho(&p3).exists());
        v3.volta_mp4(&mut e3);
        assert!(v3.espelho(&p3).exists());
        let (v2, _) = mp4_com(&tmp("mp4trava"), "203. Outra", "pronto");
        escrever(&v2.cfg.pacote.join(".render-trava"), "");
        let mut e2 = EstadoMp4::default();
        v2.volta_mp4(&mut e2);
        v2.volta_mp4(&mut e2);
        assert!(!v2.guarda().join("203. Outra").exists());
    }

    #[test]
    fn mp4_que_some_volta_do_espelho_e_o_finalizado_nao() {
        let base = tmp("mp4sumiu");
        let (v, p) = mp4_com(&base, "203. Cidade", "pronto");
        let mut est = EstadoMp4::default();
        v.volta_mp4(&mut est);
        v.volta_mp4(&mut est);
        assert!(v.espelho(&p).exists());
        std::fs::remove_file(&p).unwrap();
        v.volta_mp4(&mut est); // viu sumir: registra e restaura
        assert!(p.exists());
        let log = std::fs::read_to_string(v.guarda().join("sumicos.log")).unwrap();
        assert!(log.contains("SUMIU") && log.contains("123 python fake") && log.contains("restaurado do espelho"), "{log}");
        // finalizado (no site, apagado do PC) nao volta
        escrever(&v.arq("finalizados.json"), r#"{"203": {}}"#);
        std::fs::remove_file(&p).unwrap();
        v.volta_mp4(&mut est);
        v.volta_mp4(&mut est);
        assert!(!p.exists());
    }

    #[test]
    fn mp4_movido_para_apagar_futuramente_nao_volta() {
        let base = tmp("mp4movido");
        let (v, p) = mp4_com(&base, "203. Cidade", "pronto");
        let mut est = EstadoMp4::default();
        v.volta_mp4(&mut est);
        v.volta_mp4(&mut est);
        std::fs::remove_file(&p).unwrap();
        escrever(&v.cfg.pacote.join("APAGAR-FUTURAMENTE").join("x").join("video.mp4"), "movido");
        v.volta_mp4(&mut est);
        assert!(!p.exists());
    }

    #[test]
    fn mp4_ignora_lim_e_refeito_guarda_a_versao_anterior() {
        let base = tmp("mp4lim");
        let (v, p) = mp4_com(&base, "203. Cidade", "pronto");
        escrever(&p.with_file_name("video.lim.mp4"), "trabalho");
        assert_eq!(v.finais().len(), 1);
        let mut est = EstadoMp4::default();
        v.volta_mp4(&mut est);
        v.volta_mp4(&mut est);
        std::fs::remove_file(&p).unwrap(); // video refeito = arquivo novo (escrever por cima mudaria o espelho junto: e o mesmo arquivo)
        escrever(&p, "versao nova do video, maior");
        v.volta_mp4(&mut est);
        v.volta_mp4(&mut est);
        assert_eq!(std::fs::read_to_string(v.espelho(&p)).unwrap(), "versao nova do video, maior");
        let antes = std::fs::read_dir(v.guarda().join("203. Cidade")).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().contains("(anterior")).count();
        assert_eq!(antes, 1);
        let copias = std::fs::read_dir(base.join("backup").join("203. Cidade")).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("video")).count();
        assert_eq!(copias, 2); // a versao antiga e a nova, nunca sobrescrita
    }

    // ---- painel

    #[test]
    fn painel_junta_estado_log_e_lote_no_lote_json() {
        let base = tmp("painel");
        let v = vigias(&base);
        escrever(&v.arq("orquestrador.json"), &json!({ "prontos": ["201"], "feitas": ["201", "202"], "enviados": [], "lote_site": [["201", "Alfa"]],
            "preparadas": [["203", "Gama"]], "voz_ok": [], "atual": ["203", "Gama"] }).to_string());
        let log = [linha(300, "COMEÇANDO 201 Alfa"), linha(200, "PRONTO 201"), linha(10, "PREPARANDO 203 Gama")];
        escrever(&v.arq("orquestrador.log"), &(log.join("\n") + "\n"));
        escrever(&v.arq("lote.json"), &json!({ "cidades": { "201": { "cidade": "Alfa", "etapa": "pronto" }, "202": { "cidade": "Beta", "etapa": "pronto" },
            "203": { "cidade": "Gama", "etapa": "roteiro", "detalhe": "x", "desde": rel().epoch - 60.0 } },
            "pendencias": [{ "numero": "202", "motivo": "sem dado do tribunal" }], "outra_chave": 1 }).to_string());
        v.volta_painel(rel()).unwrap();
        let d: Value = ler_json(&v.arq("lote.json")).unwrap();
        let e = &d["esteira"];
        assert_eq!(d["outra_chave"], 1); // o resto do lote.json fica
        assert_eq!(e["meta"], 50);
        assert_eq!(e["prontos"][0]["cidade"], "Alfa");
        assert_eq!(e["pendencias_esteira"][0], json!({ "numero": "202", "cidade": "Beta", "motivo": "sem dado do tribunal" }));
        assert_eq!(e["aguardando_site"][0], json!({ "numero": "201", "cidade": "Alfa" }));
        assert_eq!(e["atual"]["numero"], "203");
        assert_eq!(e["atual"]["etapa"], "roteiro");
        assert_eq!(e["fracao_atual"], 0.3);
        assert_eq!(e["log"].as_array().unwrap().len(), 3);
        assert_eq!(e["atualizado"], rel().epoch);
        // 1 pronto em 5 h: 0,2 por hora
        assert!((e["ritmo_por_hora"].as_f64().unwrap() - 0.2).abs() < 1e-9, "{}", e["ritmo_por_hora"]);
        let seq: Vec<(&str, &str)> = e["cidades_seq"].as_array().unwrap().iter().map(|c| (c["numero"].as_str().unwrap(), c["etapa"].as_str().unwrap())).collect();
        assert_eq!(seq, [("201", "pronto"), ("202", "guia"), ("203", "esperando_voz")]);
        assert!(e["pdfs_site"]["cidades"].is_array());
    }

    #[test]
    fn painel_no_modo_capitais_conta_so_as_capitais_e_le_o_render_log() {
        let base = tmp("capitais");
        let v = vigias(&base);
        escrever(&v.arq("comando_esteira.json"), r#"{"meta":3,"so_fila":true}"#);
        escrever(&v.arq("capitais.json"), &json!({ "1001": { "cidade": "Cap Um", "uf": "XX" }, "1002": { "cidade": "Cap Dois", "uf": "XX" } }).to_string());
        escrever(&v.arq("orquestrador.json"), &json!({ "prontos": ["201", "1001"], "feitas": ["201", "1001"] }).to_string());
        escrever(&v.arq("orquestrador.log"), &(linha(30, "1002 VOZ+RENDER 1002 Cap Dois") + "\n"));
        escrever(&v.arq("lote.json"), &json!({ "cidades": { "1002": { "cidade": "Cap Dois", "etapa": "render", "desde": 1.0 } } }).to_string());
        escrever(&v.cfg.pacote.join("videos").join("1002").join("render.log"), "== 2/5 render ==\nParte 2 de 4: 50%\n");
        let e = v.resumo(rel());
        assert_eq!(e["meta"], 2);
        assert_eq!(e["prontos"].as_array().unwrap().len(), 1); // a 201 (Bahia) fica de fora
        assert_eq!(e["atual"]["numero"], "1002");
        // render: etapa 2/5, parte 2 de 4 a 50% -> dentro = 0,375; frac = 0,7 + 0,3 * (1,375 / 5) = 0,7825
        assert!((e["fracao_atual"].as_f64().unwrap() - 0.7825).abs() < 0.002, "{}", e["fracao_atual"]);
        let vagas: Vec<(&str, &str)> = e["vagas"].as_array().unwrap().iter().map(|c| (c["numero"].as_str().unwrap(), c["estado"].as_str().unwrap())).collect();
        assert_eq!(vagas, [("1001", "pronto"), ("1002", "render")]);
        // 1 de 2 (1,0) + render (0,75) = 87,5 %
        assert_eq!(e["progresso_pct"], 87.5);
    }

    #[test]
    fn hora_do_log_le_o_carimbo_do_orquestrador() {
        let t = hora_do_log("18/11 09:05:07 PRONTO", 2026);
        assert_eq!(t, Data::civil(2026, 11, 18).unwrap().0 * 86400 + 9 * 3600 + 5 * 60 + 7);
        assert_eq!(hora_do_log("sem hora aqui", 2026), 0);
        assert_eq!(hora_do_log("31/02 10:00:00", 2026), 0);
    }
}
