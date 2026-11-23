//! IAs GRATIS EXTRAS: catalogo de modelos gratis com os limites de cada um (por minuto e por dia). Com a chave do provedor no ambiente,
//! os melhores modelos dele entram na cadeia do roteador, antes do modelo local e do Claude. So entra modelo que o provedor oferece AGORA
//! (o catalogo envelhece); sem conseguir conferir, nao entra nada (nao arrisca id velho).
//! Ficou de fora: a rede (`buscar(provedor, dados)` devolve a resposta do GET /models; a chave fica com quem chama e NUNCA e gravada: o cache
//! em dados/gratis_modelos.json guarda so os ids) e a leitura do ambiente (`env` e closure). O catalogo vem do arquivo que quem chama aponta.
#![allow(dead_code)]
use crate::empresa_dev::{gravar, ler, rx};
use regex::Regex;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::OnceLock;

pub const TTL_S: f64 = 12.0 * 3600.0;
pub const POR_PROVEDOR: usize = 3; // quantos modelos de cada provedor entram (os melhores pelo ranking)
pub const CONTEXTO_MIN: i64 = 30000; // o Braniac manda dossie + historico: modelo com contexto pequeno nao serve

/// Modelos novos que o catalogo antigo nao conhece.
fn familia(id: &str) -> bool {
    static R: OnceLock<Regex> = OnceLock::new();
    rx(&R, r"(?i)gpt-oss|deepseek|kimi|qwen3|nemotron|llama-3\.3|llama-4|gemma-3|mistral-large|magistral|glm-4|minimax").is_match(id)
}

/// Nao sao modelos de conversa.
fn ruim(id: &str) -> bool {
    static R: OnceLock<Regex> = OnceLock::new();
    rx(&R, r"(?i)embed|whisper|guard|vision|image|tts|rerank|safety|moderation|orpheus|-vl\b|prompt").is_match(id)
}

/// Campo "provedores" do catalogo_gratis.json; arquivo ausente ou ilegivel = vazio.
pub fn catalogo(arq: &Path) -> Value {
    ler(arq).map(|c| c["provedores"].clone()).filter(Value::is_object).unwrap_or_else(|| json!({}))
}

/// Os n melhores (ranking do catalogo: numero menor primeiro). `vivos` = ids que o provedor oferece agora; se faltar, completa com
/// modelos recentes das familias fortes (sem limites conhecidos).
pub fn melhores(modelos: &Value, n: usize, vivos: Option<&BTreeSet<String>>, excluir: &BTreeSet<String>) -> Vec<(String, Value)> {
    let vazio = serde_json::Map::new();
    let modelos = modelos.as_object().unwrap_or(&vazio);
    let mut ok: Vec<(i64, i64, &String, &Value)> = modelos.iter().filter(|(mid, v)| v["contexto"].as_i64().unwrap_or(0) >= CONTEXTO_MIN && vivos.is_none_or(|vv| vv.contains(*mid)) && !excluir.contains(*mid) && !ruim(mid))
        .map(|(mid, v)| (v["inteligencia"].as_i64().unwrap_or(i64::MAX), v["velocidade"].as_i64().unwrap_or(i64::MAX), mid, v)).collect();
    ok.sort_by(|a, b| (a.0, a.1, a.2).cmp(&(b.0, b.1, b.2)));
    let mut escolhidos: Vec<(String, Value)> = ok.into_iter().take(n).map(|x| (x.2.clone(), x.3.clone())).collect();
    for mid in vivos.into_iter().flatten() {
        if escolhidos.len() >= n {
            break;
        }
        if !escolhidos.iter().any(|(m, _)| m == mid) && !modelos.contains_key(mid) && !excluir.contains(mid) && familia(mid) && !ruim(mid) {
            escolhidos.push((mid.clone(), json!({"inteligencia": 50, "velocidade": 9, "rpm": null, "rpd": null, "contexto": null})));
        }
    }
    escolhidos
}

/// Ids da resposta do GET /models (no OpenRouter so os ":free").
pub fn ids_da_resposta(plat: &str, resposta: &Value) -> BTreeSet<String> {
    resposta["data"].as_array().map(|a| a.iter().filter_map(|i| i["id"].as_str().map(String::from).or_else(|| i["id"].as_i64().map(|n| n.to_string()))).filter(|id| !id.is_empty() && (plat != "openrouter" || id.ends_with(":free"))).collect()).unwrap_or_default()
}

/// Ids que o provedor oferece agora, com cache de 12 h em dados/gratis_modelos.json. None se nao der para saber.
pub fn listar_vivos(dados: &Path, plat: &str, d: &Value, ts: f64, buscar: &dyn Fn(&str, &Value) -> Result<Value, String>) -> Option<BTreeSet<String>> {
    let arq = dados.join("gratis_modelos.json");
    let mut cache = ler(&arq).filter(Value::is_object).unwrap_or_else(|| json!({}));
    let guardado = |c: &Value| -> Option<BTreeSet<String>> { Some(c[plat]["ids"].as_array()?.iter().filter_map(|x| x.as_str().map(String::from)).collect()) };
    if cache[plat]["ts"].as_f64().is_some_and(|t| ts - t < TTL_S) {
        return guardado(&cache);
    }
    let ids = buscar(plat, d).map(|r| ids_da_resposta(plat, &r)).unwrap_or_default();
    if ids.is_empty() {
        return guardado(&cache); // falhou agora: vale o ultimo que deu certo, mesmo velho
    }
    cache[plat] = json!({"ts": ts, "ids": ids});
    let _ = gravar(&arq, &cache);
    Some(ids)
}

/// Acrescenta a cfg["cadeia"] os melhores modelos gratis dos provedores com chave no ambiente. Devolve os nomes adicionados.
/// `listar(provedor, dados)` = ids vivos (None = nao deu para conferir: aquele provedor fica de fora).
pub fn estender(cfg: &mut Value, cat: &Value, env: &dyn Fn(&str) -> String, n: usize, listar: &dyn Fn(&str, &Value) -> Option<BTreeSet<String>>) -> Vec<String> {
    let Some(cadeia) = cfg["cadeia"].as_array().cloned() else { return vec![] };
    let mut novos = vec![];
    for (plat, d) in cat.as_object().into_iter().flatten() {
        let (Some(var), Some(base)) = (d["env"].as_str(), d["base"].as_str()) else { continue };
        if env(var).trim().is_empty() {
            continue;
        }
        let url = format!("{}/chat/completions", base.trim_end_matches('/'));
        let Some(vivos) = listar(plat, d) else { continue };
        let ja: BTreeSet<String> = cadeia.iter().filter(|p| p["url"] == url.as_str()).filter_map(|p| p["modelo"].as_str().map(String::from)).collect();
        for (mid, v) in melhores(&d["modelos"], n, Some(&vivos), &ja) {
            let mut p = json!({"nome": format!("gratis:{plat}:{mid}"), "tipo": "online", "api": "openai", "url": url, "modelo": mid, "chave_env": var,
                               "classe": if v["velocidade"].as_i64().unwrap_or(9) <= 3 { "rapido" } else { "padrao" }});
            // sem limite conhecido = sem a chave (o roteador usa um padrao)
            for (k, campo) in [("limite_min", "rpm"), ("limite_dia", "rpd")] {
                if v[campo].as_i64().is_some_and(|x| x != 0) {
                    p[k] = v[campo].clone();
                }
            }
            novos.push(p);
        }
    }
    let nomes = novos.iter().filter_map(|p| p["nome"].as_str().map(String::from)).collect();
    let pos = cadeia.iter().position(|p| p["tipo"] != "online").unwrap_or(cadeia.len()); // antes do modelo local e do Claude
    let mut nova = cadeia;
    nova.splice(pos..pos, novos);
    cfg["cadeia"] = Value::Array(nova);
    nomes
}

/// Uma linha por provedor: ativo (com os modelos que entrariam) ou onde criar a chave. Nunca mostra a chave.
pub fn diagnostico(cat: &Value, env: &dyn Fn(&str) -> String, listar: &dyn Fn(&str, &Value) -> Option<BTreeSet<String>>) -> String {
    cat.as_object().into_iter().flatten().map(|(plat, d)| {
        let var = d["env"].as_str().unwrap_or("");
        let tem = !env(var).trim().is_empty();
        let vivos = if tem { listar(plat, d) } else { None };
        let modelos: Vec<String> = melhores(&d["modelos"], POR_PROVEDOR, vivos.as_ref(), &BTreeSet::new()).into_iter().map(|x| x.0).collect();
        format!("{} {:<12} {var:<22} {}", if tem { "ATIVO  " } else { "sem chave" }, d["nome"].as_str().unwrap_or(""), if tem { modelos.join(", ") } else { format!("crie a chave em {}", d["criar_chave"].as_str().unwrap_or("")) })
    }).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn cat() -> Value {
        json!({"alfa": {"nome": "Alfa", "base": "https://alfa.example/v1/", "env": "ALFA_KEY", "criar_chave": "https://alfa.example/keys", "modelos": {
                   "grande": {"inteligencia": 2, "velocidade": 5, "rpm": 30, "rpd": 1000, "contexto": 131072}, "rapido": {"inteligencia": 5, "velocidade": 1, "rpm": 60, "rpd": null, "contexto": 32000},
                   "pequeno": {"inteligencia": 1, "velocidade": 1, "contexto": 8000}, "sumiu": {"inteligencia": 1, "velocidade": 1, "contexto": 100000}, "llama-guard": {"inteligencia": 1, "velocidade": 1, "contexto": 100000}}},
               "beta": {"nome": "Beta", "base": "https://beta.example/v1", "env": "BETA_KEY", "criar_chave": "https://beta.example/keys", "modelos": {"b1": {"inteligencia": 3, "velocidade": 2, "contexto": 64000}}}})
    }

    fn conj(v: &[&str]) -> BTreeSet<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn so_entra_o_que_existe_agora_antes_do_local_e_sem_repetir() {
        let mut cfg = json!({"cadeia": [{"nome": "g", "tipo": "online", "url": "https://alfa.example/v1/chat/completions", "modelo": "rapido"}, {"nome": "local", "tipo": "local"}, {"nome": "claude", "tipo": "claude"}]});
        let env = |k: &str| if k == "ALFA_KEY" { " chave ".to_string() } else { String::new() };
        let listar = |plat: &str, _: &Value| {
            assert_eq!(plat, "alfa"); // provedor sem chave nem e consultado
            Some(conj(&["grande", "rapido", "llama-guard", "qwen3-novo", "embed-qwen3", "desconhecido"]))
        };
        let nomes = estender(&mut cfg, &cat(), &env, 3, &listar);
        // "rapido" ja estava na cadeia; "sumiu" nao existe mais; "pequeno" tem contexto curto; guard/embed nao conversam; a familia nova completa
        assert_eq!(nomes, vec!["gratis:alfa:grande", "gratis:alfa:qwen3-novo"]);
        let c = cfg["cadeia"].as_array().unwrap();
        assert_eq!(c.iter().map(|p| p["nome"].as_str().unwrap()).collect::<Vec<_>>(), vec!["g", "gratis:alfa:grande", "gratis:alfa:qwen3-novo", "local", "claude"]);
        assert_eq!((c[1]["limite_min"].as_i64(), c[1]["limite_dia"].as_i64(), c[1]["classe"].as_str(), c[1]["chave_env"].as_str(), c[1]["url"].as_str()), (Some(30), Some(1000), Some("padrao"), Some("ALFA_KEY"), Some("https://alfa.example/v1/chat/completions")));
        assert!(c[2].get("limite_min").is_none() && !cfg.to_string().contains(" chave "));
        // nao deu para conferir os modelos vivos: nao entra nada
        let mut cfg2 = json!({"cadeia": [{"nome": "local", "tipo": "local"}]});
        assert!(estender(&mut cfg2, &cat(), &|_| "x".into(), 3, &|_, _| None).is_empty());
        assert_eq!(cfg2["cadeia"].as_array().unwrap().len(), 1);
        assert!(estender(&mut json!({}), &cat(), &|_| "x".into(), 3, &|_, _| None).is_empty());
    }

    #[test]
    fn ranking_e_cache_de_12_horas() {
        let c = cat();
        let m = melhores(&c["alfa"]["modelos"], 2, None, &BTreeSet::new());
        assert_eq!(m.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), vec!["sumiu", "grande"]); // sem lista viva vale o catalogo
        let d = std::env::temp_dir().join(format!("braniac-gratis-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let chamadas = RefCell::new(0);
        let buscar = |plat: &str, _: &Value| -> Result<Value, String> {
            *chamadas.borrow_mut() += 1;
            if *chamadas.borrow() == 2 { Err("fora do ar".into()) } else { Ok(json!({"data": [{"id": format!("{plat}/m1:free")}, {"id": "pago"}, {"sem": "id"}]})) }
        };
        assert_eq!(listar_vivos(&d, "openrouter", &json!({}), 1000.0, &buscar), Some(conj(&["openrouter/m1:free"])));
        assert_eq!(listar_vivos(&d, "openrouter", &json!({}), 1000.0 + TTL_S - 1.0, &buscar), Some(conj(&["openrouter/m1:free"])));
        assert_eq!(*chamadas.borrow(), 1);
        assert_eq!(listar_vivos(&d, "openrouter", &json!({}), 1000.0 + TTL_S + 1.0, &buscar), Some(conj(&["openrouter/m1:free"]))); // falhou: vale o guardado
        assert_eq!(listar_vivos(&d, "alfa", &json!({}), 5000.0, &|_, _| Err("x".into())), None);
        assert_eq!(listar_vivos(&d, "alfa", &json!({}), 5000.0, &buscar), Some(conj(&["alfa/m1:free", "pago"])));
        assert!(catalogo(&d.join("nao-existe.json")).as_object().unwrap().is_empty());
    }

    #[test]
    fn diagnostico_nao_mostra_chave() {
        let t = diagnostico(&cat(), &|k| if k == "BETA_KEY" { "segredo-123".into() } else { String::new() }, &|_, _| Some(conj(&["b1"])));
        let linhas: Vec<&str> = t.lines().collect();
        assert!(linhas[0].starts_with("sem chave Alfa") && linhas[0].ends_with("crie a chave em https://alfa.example/keys"));
        assert!(linhas[1].starts_with("ATIVO   Beta") && linhas[1].ends_with(" b1") && !t.contains("segredo-123"));
    }
}
