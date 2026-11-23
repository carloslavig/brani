//! Auto-atualizacao SEGURA: de madrugada (a partir das 3h, antes das 6h, uma vez por dia) o Braniac se confere sozinho: suite de testes
//! (falha NOVA desde ontem = alerta), dependencias desatualizadas e modelos locais configurados mas nao instalados.
//! NAO INSTALA NEM ATUALIZA NADA: so confere e relata (dados/manutencao.json, manutencao.log e o relatorio da manha). Nunca registra chave
//! nem token: so nomes de pacote, versoes e contagem de testes.
//! Ficou de fora (quem chama roda e entrega a saida): os processos (testes, pip), a pergunta ao Ollama, o organizador, a seguranca do PC
//! e a thread. Diferenca do Python: alem da saida do pytest, a leitura de testes entende a do `cargo test`.
#![allow(dead_code)]
use crate::barramento;
use crate::empresa_dev::{corta, gravar, ler, rx};
use crate::tempo::Agora;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;
use std::sync::OnceLock;

pub const HORA: u32 = 3;
pub const PACOTES: [&str; 10] = ["faster-whisper", "edge-tts", "piper-tts", "sounddevice", "requests", "av", "pedalboard", "numpy", "ctranslate2", "onnxruntime"];

pub fn ultimo(dados: &Path) -> Value {
    ler(&dados.join("manutencao.json")).filter(Value::is_object).unwrap_or_else(|| json!({}))
}

fn log(dados: &Path, agora: &Agora, txt: &str) {
    let _ = std::fs::create_dir_all(dados);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dados.join("manutencao.log")) {
        let _ = writeln!(f, "{} {}:{:02} | {txt}", agora.data.iso(), agora.hm(), agora.seg);
    }
}

/// {"ok", "passaram", "falharam", "falhas": [nomes]} a partir de (codigo, saida) dos testes. ok = nenhuma falha; nao conseguir rodar tambem e falha.
pub fn testes(rodar: &dyn Fn() -> Result<(i32, String), String>) -> Value {
    static FALHA: OnceLock<Regex> = OnceLock::new();
    let (cod, saida) = match rodar() {
        Ok(x) => x,
        Err(e) => return json!({ "ok": false, "passaram": 0, "falharam": 0, "falhas": [], "erro": corta(&e, 120) }),
    };
    let saida = saida.replace('\r', "");
    let num = |rot: &str| Regex::new(&format!(r"(\d+) {rot}")).ok().and_then(|r| r.captures(&saida)?.get(1)?.as_str().parse::<i64>().ok()).unwrap_or(0);
    let falhas: BTreeSet<&str> = rx(&FALHA, r"(?m)^(?:FAILED|ERROR) (\S+?)(?:\[| - |$)|^test (\S+) \.\.\. FAILED").captures_iter(&saida)
        .filter_map(|c| c.get(1).or(c.get(2))).map(|m| m.as_str()).collect();
    let mut res = json!({ "ok": cod == 0, "passaram": num("passed"), "falharam": num("failed") + num("error"), "falhas": falhas });
    if ![0, 1, 101].contains(&cod) {
        res["erro"] = json!(format!("os testes saíram com código {cod}"));
    }
    res
}

/// [(pacote, versao atual, versao nova)] so dos pacotes de `pacotes`, a partir do JSON do `pip list --outdated`. Saida ilegivel = None (nao e falha do Braniac).
pub fn dependencias(saida: &str, pacotes: &[&str]) -> Option<Vec<(String, String, String)>> {
    let n = |x: &str| x.to_lowercase().replace('_', "-");
    let lista: Value = match (saida.find('['), saida.rfind(']')) {
        (Some(i), Some(f)) if i < f => serde_json::from_str(&saida[i..=f]).ok()?,
        _ => return None,
    };
    let t = |d: &Value, k: &str| d[k].as_str().unwrap_or("").to_string();
    Some(lista.as_array()?.iter().filter(|d| pacotes.iter().any(|p| n(p) == n(d["name"].as_str().unwrap_or("")))).map(|d| (t(d, "name"), t(d, "version"), t(d, "latest_version"))).collect())
}

/// Modelos locais configurados (provedores) que NAO estao instalados. `tags` = nomes instalados; None = o servidor local nao respondeu.
pub fn modelos(cfg: &Value, tags: Option<&[String]>) -> Option<Vec<String>> {
    let tags = tags?;
    let mut queridos: BTreeSet<&str> = cfg["cadeia"].as_array().into_iter().flatten().filter(|p| p["tipo"] == "ollama").filter_map(|p| p["modelo"].as_str()).collect();
    queridos.extend(cfg["modelo_local"].as_str());
    let inst: BTreeSet<String> = tags.iter().cloned().chain(tags.iter().filter(|t| !t.contains(':')).map(|t| format!("{t}:latest"))).collect();
    Some(queridos.into_iter().filter(|q| !inst.contains(*q) && !inst.contains(&format!("{q}:latest"))).map(String::from).collect())
}

/// Uma linha com quantos recados cada agente mandou no barramento nas ultimas `horas` (pedidos nao contam). So nomes e contagem.
pub fn predio_feitos(dados: &Path, ts: f64, horas: f64) -> Option<String> {
    let mut conta: BTreeMap<String, i64> = BTreeMap::new();
    for m in barramento::historico(dados, 500) {
        if m["ts"].as_f64().unwrap_or(0.0) >= ts - horas * 3600.0 && m["tipo"] != "pedido" {
            *conta.entry(m["de"].as_str().unwrap_or("").to_string()).or_insert(0) += 1;
        }
    }
    let mut v: Vec<(String, i64)> = conta.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    (!v.is_empty()).then(|| format!("🏢 Prédio nas últimas 24 h (mensagens de cada agente): {}.", v.iter().map(|(k, n)| format!("{k} {n}")).collect::<Vec<_>>().join(", ")))
}

/// O que ja foi lido do sistema nesta rodada.
pub struct Leituras {
    pub testes: Value,
    pub dependencias: Option<Vec<(String, String, String)>>,
    pub modelos: Option<Vec<String>>,
}

/// Uma rodada: monta o resumo, grava e relata. Nada e instalado nem alterado.
pub fn rodar_manutencao(dados: &Path, agora: &Agora, l: &Leituras, registrar: &dyn Fn(&str)) -> Value {
    let anterior = ultimo(dados);
    let tem_anterior = anterior.as_object().is_some_and(|o| !o.is_empty());
    let t = &l.testes;
    let nomes = |v: &Value| v["falhas"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect::<Vec<_>>();
    let (falhas, de_ontem) = (nomes(t), nomes(&anterior));
    let novas: Vec<String> = falhas.iter().filter(|f| !tem_anterior || !de_ontem.contains(f)).cloned().collect();
    let mut partes = vec![format!("🔧 Manutenção da madrugada: testes {} ok / {} com falha{}{}.", t["passaram"], t["falharam"],
        if !novas.is_empty() && tem_anterior { format!(" — ⚠️ {} falha(s) NOVA(S) desde ontem: {}", novas.len(), novas.iter().take(5).map(|n| n.rsplit("::").next().unwrap_or(n)).collect::<Vec<_>>().join(", ")) } else { String::new() },
        t["erro"].as_str().map(|e| format!(" ({e})")).unwrap_or_default())];
    partes.push(match &l.dependencias {
        Some(d) if !d.is_empty() => format!("📦 Atualizações disponíveis (NÃO apliquei; peça para testar e aplicar): {}.", d.iter().take(8).map(|(n, a, b)| format!("{n} {a}→{b}")).collect::<Vec<_>>().join(", ")),
        Some(_) => "📦 Dependências em dia.".into(),
        None => "📦 Não consegui verificar atualizações (sem internet?).".into(),
    });
    match &l.modelos {
        Some(m) if !m.is_empty() => partes.push(format!("🧠 Modelos locais configurados mas não instalados: {} (o Braniac usa outro da cadeia).", m.join(", "))),
        Some(_) => {}
        None => partes.push("🧠 O servidor de modelos locais não estava rodando: não conferi os modelos.".into()),
    }
    let resumo = partes.join(" ");
    let res = json!({ "quando": format!("{} {}", agora.data.dia_mes(), agora.hm()), "dia": agora.data.iso(), "testes_ok": t["ok"].as_bool().unwrap_or(false),
                      "passaram": t["passaram"], "falharam": t["falharam"], "falhas": falhas, "novas_falhas": novas, "dependencias": l.dependencias, "modelos_faltando": l.modelos, "resumo": resumo });
    let _ = gravar(&dados.join("manutencao.json"), &res);
    log(dados, agora, &resumo);
    registrar(&resumo);
    res
}

/// Hora de rodar? Uma vez por dia, das 3h ate antes do bom dia das 6h, e nunca duas juntas (`rodando`).
pub fn deve_rodar(dados: &Path, agora: &Agora, rodando: bool) -> bool {
    (HORA..6).contains(&agora.hora) && ultimo(dados)["dia"] != agora.data.iso().as_str() && !rodando
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;

    fn agora(dia: i64, hora: u32) -> Agora {
        Agora { data: Data::civil(2026, 11, dia).unwrap(), hora, min: 10, seg: 5 }
    }

    #[test]
    fn le_a_saida_dos_testes() {
        let py = "FAILED braniac/tests/test_a.py::test_x - AssertionError\r\nERROR braniac/tests/test_b.py::test_y[caso1]\nFAILED braniac/tests/test_a.py::test_x - de novo\n=== 2 failed, 310 passed, 1 error in 9s ===";
        let t = testes(&|| Ok((1, py.into())));
        assert!(t["ok"] == false && t["passaram"] == 310 && t["falharam"] == 3 && t["erro"].is_null());
        assert_eq!(t["falhas"], json!(["braniac/tests/test_a.py::test_x", "braniac/tests/test_b.py::test_y"]));
        let rs = "test a::tests::um ... ok\ntest b::tests::dois ... FAILED\n\ntest result: FAILED. 410 passed; 1 failed; 0 ignored";
        let t = testes(&|| Ok((101, rs.into())));
        assert!(t["passaram"] == 410 && t["falharam"] == 1 && t["falhas"] == json!(["b::tests::dois"]) && t["erro"].is_null());
        assert!(testes(&|| Ok((0, "5 passed".into())))["ok"] == true);
        assert!(testes(&|| Ok((4, "uso errado".into())))["erro"].as_str().unwrap().contains("código 4"));
        let t = testes(&|| Err("não achei o executável".into()));
        assert!(t["ok"] == false && t["erro"].is_string()); // erro ao rodar conta como falha
    }

    #[test]
    fn dependencias_e_modelos() {
        let pip = "aviso qualquer\n[{\"name\": \"Edge_TTS\", \"version\": \"6.1\", \"latest_version\": \"7.0\"}, {\"name\": \"outro\", \"version\": \"1\", \"latest_version\": \"2\"}]\n";
        assert_eq!(dependencias(pip, &PACOTES), Some(vec![("Edge_TTS".into(), "6.1".into(), "7.0".into())]));
        assert!(dependencias("[]", &PACOTES) == Some(vec![]) && dependencias("sem rede", &PACOTES).is_none());
        let cfg = json!({ "modelo_local": "qwen2.5:7b", "cadeia": [{ "tipo": "ollama", "modelo": "llama3" }, { "tipo": "ollama", "modelo": "falta:1b" }, { "tipo": "groq", "modelo": "x" }] });
        let tags = vec!["qwen2.5:7b".to_string(), "llama3:latest".to_string()];
        assert_eq!(modelos(&cfg, Some(&tags)), Some(vec!["falta:1b".to_string()]));
        assert_eq!(modelos(&cfg, None), None);
    }

    #[test]
    fn rodada_so_relata_marca_falha_nova_e_roda_uma_vez_por_dia() {
        let d = std::env::temp_dir().join(format!("braniac-manutencao-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let notas = std::cell::RefCell::new(Vec::new());
        let reg = |t: &str| notas.borrow_mut().push(t.to_string());
        assert!(deve_rodar(&d, &agora(17, 3), false) && !deve_rodar(&d, &agora(17, 2), false) && !deve_rodar(&d, &agora(17, 6), false) && !deve_rodar(&d, &agora(17, 4), true));
        let l1 = Leituras { testes: json!({ "ok": false, "passaram": 300, "falharam": 1, "falhas": ["t/a.py::velha"] }), dependencias: Some(vec![]), modelos: Some(vec![]) };
        let r = rodar_manutencao(&d, &agora(17, 3), &l1, &reg);
        assert!(r["testes_ok"] == false && r["dia"] == "2026-11-17" && !notas.borrow()[0].contains("NOVA") && notas.borrow()[0].contains("Dependências em dia"));
        assert!(!deve_rodar(&d, &agora(17, 4), false) && deve_rodar(&d, &agora(18, 4), false)); // uma vez por dia
        let l2 = Leituras { testes: json!({ "ok": false, "passaram": 299, "falharam": 2, "falhas": ["t/a.py::velha", "t/b.py::nova"], "erro": "x" }),
                            dependencias: None, modelos: None };
        let r = rodar_manutencao(&d, &agora(18, 3), &l2, &reg);
        let n = notas.borrow()[1].clone();
        assert!(r["novas_falhas"] == json!(["t/b.py::nova"]) && n.contains("1 falha(s) NOVA(S) desde ontem: nova") && n.contains("(x)") && n.contains("sem internet") && n.contains("não conferi"));
        let l3 = Leituras { testes: json!({ "ok": true, "passaram": 301, "falharam": 0, "falhas": [] }), dependencias: Some(vec![("numpy".into(), "1".into(), "2".into())]), modelos: Some(vec!["m:1".into()]) };
        let r = rodar_manutencao(&d, &agora(19, 3), &l3, &reg);
        assert!(r["testes_ok"] == true && r["resumo"].as_str().unwrap().contains("numpy 1→2") && r["resumo"].as_str().unwrap().contains("NÃO apliquei") && ultimo(&d)["dia"] == "2026-11-19");
        assert!(std::fs::read_to_string(d.join("manutencao.log")).unwrap().lines().count() == 3);
        assert_eq!(predio_feitos(&d, 0.0, 24.0), None);
        barramento::enviar(&d, "Forja", "Jarvis", "alerta", "x", 0, None).unwrap();
        barramento::enviar(&d, "Forja", "Ada", "pedido", "y", 0, None).unwrap();
        let agora_ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64();
        assert_eq!(predio_feitos(&d, agora_ts, 24.0).unwrap(), "🏢 Prédio nas últimas 24 h (mensagens de cada agente): Forja 1.");
        let _ = std::fs::remove_dir_all(&d);
    }
}
