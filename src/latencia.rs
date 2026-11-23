//! Latencia por provedor de IA (porte de braniac/latencia.py): cada chamada vira uma linha {t, provedor, ms, ok} em
//! dados/latencia.jsonl. NUNCA grava o texto da pergunta. Dai saem a mediana por provedor, as falhas seguidas (o roteador usa
//! para tirar da fila quem so falha) e o relatorio P50/P95. Linha torta ou sem provedor e ignorada. Nada ficou de fora.
#![allow(dead_code)]
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::path::Path;

pub const JANELA: usize = 50;

/// Medir latencia nunca derruba a resposta: erro de disco e engolido.
pub fn registrar(dados: &Path, provedor: &str, ms: i64, ok: bool, ts: f64) {
    let _ = std::fs::create_dir_all(dados);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dados.join("latencia.jsonl")) {
        let _ = writeln!(f, "{}", json!({ "t": ts as i64, "provedor": provedor, "ms": ms, "ok": ok }));
    }
}

// shortcut: le o arquivo inteiro; se ficar grande (MBs), passar a ler so o fim ou rotacionar
fn linhas(dados: &Path) -> Vec<(String, i64, bool, i64)> {
    std::fs::read_to_string(dados.join("latencia.jsonl")).unwrap_or_default().lines().filter_map(|l| {
        let d: Value = serde_json::from_str(l).ok()?;
        Some((d["provedor"].as_str()?.to_string(), d["ms"].as_i64().unwrap_or(0), d["ok"] == true, d["t"].as_i64().unwrap_or(0)))
    }).collect()
}

/// {provedor: ms das ultimas `janela` chamadas que deram certo}.
pub fn carregar(dados: &Path, janela: usize) -> HashMap<String, VecDeque<i64>> {
    let mut out: HashMap<String, VecDeque<i64>> = HashMap::new();
    for (p, ms, _, _) in linhas(dados).into_iter().filter(|l| l.2) {
        let fila = out.entry(p).or_default();
        fila.push_back(ms);
        if fila.len() > janela {
            fila.pop_front();
        }
    }
    out
}

/// {provedor: (quantas falhas seguidas ate agora, hora da ultima)}: so quem terminou o registro falhando.
pub fn falhas_seguidas(dados: &Path) -> HashMap<String, (u32, i64)> {
    let mut out: HashMap<String, (u32, i64)> = HashMap::new();
    for (p, _, ok, t) in linhas(dados) {
        if ok {
            out.remove(&p);
        } else {
            let n = out.get(&p).map_or(0, |x| x.0) + 1;
            out.insert(p, (n, t));
        }
    }
    out
}

/// Mediana em segundos das amostras do provedor, ou None sem amostra.
pub fn mediana_s(amostras: &HashMap<String, VecDeque<i64>>, nome: &str) -> Option<f64> {
    let mut v: Vec<i64> = amostras.get(nome)?.iter().copied().collect();
    if v.is_empty() {
        return None;
    }
    v.sort_unstable();
    let m = v.len() / 2;
    Some(if v.len() % 2 == 1 { v[m] as f64 } else { (v[m - 1] + v[m]) as f64 / 2.0 } / 1000.0)
}

/// Percentil pelo indice arredondado (meio vai para o par, como o round() do Python).
fn pct(v: &[i64], p: f64) -> i64 {
    let mut v = v.to_vec();
    v.sort_unstable();
    let x = p / 100.0 * (v.len() - 1) as f64;
    let (chao, resto) = (x.floor(), x - x.floor());
    let i = if resto > 0.5 || (resto == 0.5 && chao as usize % 2 == 1) { chao as usize + 1 } else { chao as usize };
    v[i.min(v.len() - 1)]
}

/// Tabela P50/P95 por provedor, do mais rapido ao mais lento; quem nunca deu certo vai para o fim.
pub fn relatorio(dados: &Path) -> String {
    let mut por: Vec<(String, Vec<i64>, usize)> = Vec::new();
    for (p, ms, ok, _) in linhas(dados) {
        let i = por.iter().position(|r| r.0 == p).unwrap_or_else(|| {
            por.push((p, Vec::new(), 0));
            por.len() - 1
        });
        if ok { por[i].1.push(ms) } else { por[i].2 += 1 }
    }
    if por.is_empty() {
        return "Sem chamadas registradas ainda.".into();
    }
    por.sort_by_key(|r| if r.1.is_empty() { i64::MAX } else { pct(&r.1, 50.0) });
    let mut out = vec![format!("{:<24}{:>9}{:>9}{:>6}{:>8}", "provedor", "P50 ms", "P95 ms", "ok", "falhas")];
    for (nome, ms, falhas) in &por {
        let (p50, p95) = if ms.is_empty() { ("-".to_string(), "-".to_string()) } else { (pct(ms, 50.0).to_string(), pct(ms, 95.0).to_string()) };
        out.push(format!("{nome:<24}{p50:>9}{p95:>9}{:>6}{falhas:>8}", ms.len()));
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pasta(nome: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("braniac-latencia-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn registra_sem_texto_e_conta_falhas_seguidas() {
        let p = pasta("falhas");
        assert!(carregar(&p, JANELA).is_empty() && falhas_seguidas(&p).is_empty());
        assert_eq!(relatorio(&p), "Sem chamadas registradas ainda.");
        for (prov, ms, ok, t) in [("a", 100, true, 1), ("b", 0, false, 2), ("b", 0, false, 3), ("a", 300, true, 4), ("c", 0, false, 5), ("c", 900, true, 6), ("a", 0, false, 7)] {
            registrar(&p, prov, ms, ok, t as f64 + 0.9);
        }
        let bruto = std::fs::read_to_string(p.join("latencia.jsonl")).unwrap();
        assert_eq!(bruto.lines().next().unwrap(), r#"{"ms":100,"ok":true,"provedor":"a","t":1}"#); // so estes 4 campos
        std::fs::write(p.join("latencia.jsonl"), format!("{bruto}lixo\n{{\"ok\": false}}\n")).unwrap(); // linha torta nao derruba
        let f = falhas_seguidas(&p);
        assert!(f.len() == 2 && f["b"] == (2, 3) && f["a"] == (1, 7) && !f.contains_key("c")); // c voltou a responder: zera
        let am = carregar(&p, JANELA);
        assert_eq!(mediana_s(&am, "a"), Some(0.2));
        assert_eq!(mediana_s(&am, "c"), Some(0.9));
        assert_eq!(mediana_s(&am, "b"), None);
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn janela_percentil_e_relatorio() {
        let p = pasta("relatorio");
        for i in 1..=60 {
            registrar(&p, "rapido", i, true, 0.0);
        }
        registrar(&p, "lento", 5000, true, 0.0);
        registrar(&p, "lento", 0, false, 0.0);
        registrar(&p, "morto", 0, false, 0.0);
        let am = carregar(&p, JANELA);
        assert!(am["rapido"].len() == 50 && am["rapido"][0] == 11); // so as 50 ultimas
        assert_eq!((pct(&[1, 2, 3, 4], 50.0), pct(&[1, 2, 3, 4, 5, 6], 50.0), pct(&[1, 2, 3, 4], 95.0), pct(&[7], 95.0)), (3, 3, 4, 7)); // 1,5 -> 2; 2,5 -> 2
        let r = relatorio(&p);
        let l: Vec<&str> = r.lines().collect();
        assert_eq!(l[0], "provedor                   P50 ms   P95 ms    ok  falhas");
        assert_eq!(l[1], "rapido                         31       57    60       0");
        assert_eq!(l[2], "lento                        5000     5000     1       1");
        assert_eq!(l[3], "morto                           -        -     0       1");
        let _ = std::fs::remove_dir_all(&p);
    }
}
