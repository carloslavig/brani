//! Videos ja CONCLUIDOS (link/concluidos.py): video que aparece no site de publicacao vira concluido e a esteira nunca mais faz
//! aquela cidade (preparar, voz, render, envio), a nao ser que uma PESSOA peca para refazer: `pedir_refazer` libera UMA vez so
//! (`usar_pedido` apaga quando a esteira pega). Daqui nada fala com site nenhum: o texto do sitemap publico chega pela closure
//! `baixar`, que quem chama decide; marcar "aprovado" no checklist tambem e closure.
//! Diferenca do Python: a hora da ultima conferencia fica em dados/magno/concluidos_site.ultima (la era memoria do processo),
//! para o intervalo de 3 h valer tambem depois de reiniciar.
#![allow(dead_code)]
use crate::tempo::Agora;
use regex::Regex;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const INTERVALO: f64 = 3.0 * 3600.0;
static TRAVA: Mutex<()> = Mutex::new(());

fn arq(dados: &Path) -> PathBuf {
    dados.join("magno").join("concluidos_site.json")
}

fn pedidos(dados: &Path) -> PathBuf {
    dados.join("magno").join("refazer_pedidos.json") // {numero: {quem, quando, nota}}: pedido humano de refazer
}

fn ler(arq: &Path) -> Map<String, Value> {
    std::fs::read_to_string(arq).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn gravar(arq: &Path, d: &Map<String, Value>) -> Result<(), String> {
    if let Some(p) = arq.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    std::fs::write(arq, serde_json::to_string_pretty(d).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

/// "14" -> "014"; o que nao e so digito fica como veio.
pub fn num(n: &str) -> String {
    if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) { format!("{:0>3}", n.trim_start_matches('0')) } else { n.to_string() }
}

fn quando(a: &Agora) -> String {
    let (y, m, d) = a.data.partes();
    format!("{d:02}/{m:02}/{y:04} {}", a.hm())
}

pub fn lista(dados: &Path) -> Map<String, Value> {
    ler(&arq(dados))
}

pub fn concluido(dados: &Path, numero: &str) -> bool {
    lista(dados).contains_key(&num(numero))
}

pub fn pedido_humano(dados: &Path, numero: &str) -> bool {
    ler(&pedidos(dados)).contains_key(&num(numero))
}

/// true = a esteira nao pode mexer nesta cidade (ja esta no site e nenhuma pessoa pediu para refazer).
pub fn bloqueia(dados: &Path, numero: &str) -> bool {
    concluido(dados, numero) && !pedido_humano(dados, numero)
}

/// Pedido de uma PESSOA para refazer um video ja publicado. Vale uma vez. `quem` vazio e recusado (o Python nao conferia).
pub fn pedir_refazer(dados: &Path, numero: &str, quem: &str, nota: &str, agora: &Agora) -> Result<String, String> {
    if quem.trim().is_empty() {
        return Err("pedido de refazer precisa dizer quem pediu".into());
    }
    let n = num(numero);
    let _t = TRAVA.lock().map_err(|e| e.to_string())?;
    let mut p = ler(&pedidos(dados));
    p.insert(n.clone(), json!({ "quem": quem, "quando": quando(agora), "nota": nota.chars().take(200).collect::<String>() }));
    gravar(&pedidos(dados), &p)?;
    Ok(n)
}

/// A esteira pegou o pedido: ele deixa de valer. Devolve se havia pedido.
pub fn usar_pedido(dados: &Path, numero: &str) -> Result<bool, String> {
    let _t = TRAVA.lock().map_err(|e| e.to_string())?;
    let mut p = ler(&pedidos(dados));
    if p.remove(&num(numero)).is_none() {
        return Ok(false);
    }
    gravar(&pedidos(dados), &p).map(|_| true)
}

/// (numero, link) de todos os videos publicados: paginas /uf/cidade/NNN-.../ do sitemap, na ordem em que aparecem, sem repetir.
pub fn publicados(sitemap: &str) -> Vec<(String, String)> {
    let re = Regex::new(r#"(https?://[^<\s"]+?/[a-z]{2}/[a-z0-9-]+/(\d{3,4})-[a-z0-9-]+/)"#).expect("regex fixa");
    let mut out: Vec<(String, String)> = vec![];
    for m in re.captures_iter(sitemap) {
        if !out.iter().any(|(n, _)| n == &m[2]) {
            out.push((m[2].to_string(), m[1].to_string()));
        }
    }
    out
}

/// Marca como concluido todo video novo do site e devolve os numeros novos. No maximo a cada 3 h (ou `forcar`): fora disso
/// `baixar` nem e chamada. `agora_s`: relogio em segundos (so para o intervalo). `aprovar(numero)`: o checklist passa a dizer aprovado.
pub fn sincronizar(dados: &Path, baixar: &dyn Fn() -> Result<String, String>, agora: &Agora, agora_s: f64, forcar: bool, aprovar: &dyn Fn(&str)) -> Result<Vec<String>, String> {
    let marca = dados.join("magno").join("concluidos_site.ultima");
    let ultima = std::fs::read_to_string(&marca).ok().and_then(|t| t.trim().parse::<f64>().ok()).unwrap_or(0.0);
    if !forcar && agora_s - ultima < INTERVALO {
        return Ok(vec![]);
    }
    std::fs::create_dir_all(dados.join("magno")).map_err(|e| e.to_string())?;
    std::fs::write(&marca, agora_s.to_string()).map_err(|e| e.to_string())?;
    let publ = publicados(&baixar()?);
    let mut novos = vec![];
    {
        let _t = TRAVA.lock().map_err(|e| e.to_string())?;
        let mut d = lista(dados);
        for (n, url) in publ {
            if !d.contains_key(&n) {
                d.insert(n.clone(), json!({ "numero": n, "site": url, "concluido": true, "marcado": quando(agora) }));
                novos.push(n);
            }
        }
        if !novos.is_empty() {
            gravar(&arq(dados), &d)?;
        }
    }
    for n in &novos {
        aprovar(n);
    }
    Ok(novos)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;
    use std::cell::RefCell;

    const SITEMAP: &str = "<urlset><url><loc>https://site.exemplo/ba/</loc></url>\n<url><loc>https://site.exemplo/ba/nilo-pecanha/170-nilo-pecanha-orcamento/</loc></url>\n<url><loc>https://site.exemplo/ba/ilheus/014-ilheus-orcamento-2025/</loc></url></urlset>";

    fn pasta(nome: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("braniac-link_concluidos-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn agora() -> Agora {
        Agora { data: Data::civil(2026, 11, 18).unwrap(), hora: 9, min: 5, seg: 0 }
    }

    #[test]
    fn site_vira_concluido_e_bloqueia() {
        let d = pasta("site");
        let (baixou, aprovados) = (RefCell::new(0), RefCell::new(vec![]));
        let baixar = || { *baixou.borrow_mut() += 1; Ok(SITEMAP.to_string()) };
        let aprovar = |n: &str| aprovados.borrow_mut().push(n.to_string());
        let mut novos = sincronizar(&d, &baixar, &agora(), 100_000.0, false, &aprovar).unwrap();
        novos.sort();
        assert_eq!(novos, ["014", "170"]);
        assert!(bloqueia(&d, "170") && bloqueia(&d, "14") && !bloqueia(&d, "171"));
        assert_eq!(lista(&d)["170"]["marcado"], "18/11/2026 09:05");
        assert!(sincronizar(&d, &baixar, &agora(), 100_001.0, false, &aprovar).unwrap().is_empty()); // no maximo a cada 3 h
        assert_eq!((*baixou.borrow(), aprovados.borrow().len()), (1, 2)); // dentro do intervalo nem baixa
        assert!(sincronizar(&d, &baixar, &agora(), 100_002.0, true, &aprovar).unwrap().is_empty()); // forcado: baixa, mas nada e novo
        assert!(sincronizar(&d, &|| Err("fora do ar".into()), &agora(), 200_000.0, false, &aprovar).is_err());
        assert!(bloqueia(&d, "170")); // falha ao baixar nao desmarca ninguem
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn so_pessoa_libera_e_vale_uma_vez() {
        let d = pasta("pessoa");
        sincronizar(&d, &|| Ok(SITEMAP.to_string()), &agora(), 100_000.0, false, &|_| ()).unwrap();
        assert!(pedir_refazer(&d, "170", "  ", "", &agora()).is_err() && bloqueia(&d, "170")); // sem dizer quem, nao libera
        assert_eq!(pedir_refazer(&d, "170", "Ana (whatsapp)", &"n".repeat(500), &agora()).unwrap(), "170");
        assert!(!bloqueia(&d, "170") && pedido_humano(&d, "170") && bloqueia(&d, "014")); // so a cidade pedida
        assert_eq!(ler(&pedidos(&d))["170"]["nota"].as_str().unwrap().len(), 200);
        assert_eq!(usar_pedido(&d, "170"), Ok(true));
        assert!(bloqueia(&d, "170"));
        assert_eq!(usar_pedido(&d, "170"), Ok(false)); // vale uma vez
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn numero_e_sitemap() {
        assert_eq!((num("14"), num("0014"), num("1001"), num("x1"), num("000")), ("014".into(), "014".into(), "1001".into(), "x1".into(), "000".into()));
        let p = publicados("<loc>https://s.exemplo/ba/a/170-a/</loc><loc>https://s.exemplo/ba/a/170-a-v2/</loc><loc>https://s.exemplo/sobre/</loc>");
        assert_eq!(p, [("170".to_string(), "https://s.exemplo/ba/a/170-a/".to_string())]);
    }
}
