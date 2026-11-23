//! Lista oficial de municipios do IBGE, por estado, na ordem de trabalho do dono (braniac/ibge_cidades.py).
//! Fonte unica: API de localidades do IBGE (aberta, sem chave, sem IA). Nada aqui vem de memoria: estado sem arquivo = None.
//! A rede entra como closure `get(caminho) -> JSON` (caminho relativo a `API`). Arquivos em `pasta` (dados/ibge): UF.json, UF.md,
//! estados.json, capitais.json, estados.md. Diferenca do Python: o JSON sai com recuo de 2 espacos (la era 1).
#![allow(dead_code)]
use crate::tempo::Data;
use crate::texto_pt::sem_acento;
use serde_json::{json, Value};
use std::path::Path;

pub const API: &str = "https://servicodados.ibge.gov.br/api/v1/localidades";

/// Nordeste (comecando pela Bahia), Norte, Centro-Oeste, Sudeste, Sul.
pub const ORDEM: [(&str, &[&str]); 5] = [
    ("Nordeste", &["BA", "SE", "AL", "PE", "PB", "RN", "CE", "PI", "MA"]),
    ("Norte", &["TO", "PA", "AP", "RR", "AM", "AC", "RO"]),
    ("Centro-Oeste", &["MT", "MS", "GO", "DF"]),
    ("Sudeste", &["SP", "RJ", "MG", "ES"]),
    ("Sul", &["PR", "SC", "RS"]),
];

/// Codigo IBGE da capital de cada estado; o nome sai da API e o `baixar` falha se o codigo nao existir la.
pub const CAPITAIS: [(&str, i64); 27] = [
    ("BA", 2927408), ("SE", 2800308), ("AL", 2704302), ("PE", 2611606), ("PB", 2507507), ("RN", 2408102), ("CE", 2304400), ("PI", 2211001), ("MA", 2111300),
    ("TO", 1721000), ("PA", 1501402), ("AP", 1600303), ("RR", 1400100), ("AM", 1302603), ("AC", 1200401), ("RO", 1100205), ("MT", 5103403), ("MS", 5002704),
    ("GO", 5208707), ("DF", 5300108), ("SP", 3550308), ("RJ", 3304557), ("MG", 3106200), ("ES", 3205309), ("PR", 4106902), ("SC", 4205407), ("RS", 4314902),
];

fn capital(uf: &str) -> Option<i64> {
    CAPITAIS.iter().find(|c| c.0 == uf).map(|c| c.1)
}

fn chave(nome: &str) -> String {
    sem_acento(nome).chars().filter(char::is_ascii).collect()
}

fn grava(arq: &Path, texto: String) -> Result<(), String> {
    std::fs::write(arq, texto).map_err(|e| format!("{}: {e}", arq.display()))
}

fn bonito(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

fn linha_cidade(c: &Value) -> String {
    format!("{:03}. {} (IBGE {}){}", c["n"].as_i64().unwrap_or(0), c["nome"].as_str().unwrap_or(""), c["ibge"], if c["capital"] == true { " [CAPITAL]" } else { "" })
}

/// Baixa os 27 estados e grava em `pasta`. Qualquer conta que nao feche com o IBGE (estados, capital, total) e erro: nada e completado de memoria.
pub fn baixar(pasta: &Path, hoje: Data, get: &dyn Fn(&str) -> Result<Value, String>) -> Result<Value, String> {
    let lista = |v: Value| v.as_array().cloned().ok_or("resposta do IBGE fora do formato".to_string());
    let nomes_uf: Vec<(String, String)> = lista(get("estados")?)?.iter().filter_map(|e| Some((e["sigla"].as_str()?.to_string(), e["nome"].as_str()?.to_string()))).collect();
    let mut ufs: Vec<&str> = ORDEM.iter().flat_map(|r| r.1.iter().copied()).collect();
    let mut do_ibge: Vec<&str> = nomes_uf.iter().map(|n| n.0.as_str()).collect();
    ufs.sort();
    do_ibge.sort();
    if ufs != do_ibge || ufs.iter().any(|u| capital(u).is_none()) {
        return Err("ORDEM/CAPITAIS nao batem com os estados do IBGE".into());
    }
    std::fs::create_dir_all(pasta).map_err(|e| e.to_string())?;
    let hoje = hoje.iso();
    let (mut estados, mut capitais, mut total) = (vec![], vec![], 0);
    for (regiao, lista_ufs) in ORDEM {
        for uf in lista_ufs {
            let cap_id = capital(uf).unwrap_or(0);
            let nome_uf = nomes_uf.iter().find(|n| n.0 == *uf).map(|n| n.1.as_str()).unwrap_or("");
            // capital e a numero 1; o resto em ordem alfabetica
            let mut bruto = lista(get(&format!("estados/{uf}/municipios"))?)?;
            bruto.sort_by_key(|m| (m["id"].as_i64() != Some(cap_id), chave(m["nome"].as_str().unwrap_or(""))));
            let cidades: Vec<Value> = bruto.iter().enumerate().map(|(i, m)| json!({ "n": i + 1, "ibge": m["id"], "nome": m["nome"], "capital": m["id"].as_i64() == Some(cap_id) })).collect();
            let caps: Vec<&Value> = cidades.iter().filter(|c| c["capital"] == true).collect();
            let [cap] = caps[..] else { return Err(format!("capital de {uf} nao encontrada na lista do IBGE")) };
            let ordem = estados.len() + 1;
            let fonte = format!("{API}/estados/{uf}/municipios");
            let dados = json!({ "ordem": ordem, "uf": uf, "estado": nome_uf, "regiao": regiao, "capital": cap["nome"], "total": cidades.len(), "fonte": fonte, "baixado_em": hoje, "cidades": cidades });
            grava(&pasta.join(format!("{uf}.json")), bonito(&dados))?;
            let mut linhas = vec![
                format!("# {ordem:02}. {nome_uf} ({uf}), {regiao}: {} municipios", cidades.len()),
                format!("Fonte: IBGE, {fonte} (baixado em {hoje}). Capital: {}.", cap["nome"].as_str().unwrap_or("")),
                String::new(),
            ];
            linhas.extend(cidades.iter().map(linha_cidade));
            grava(&pasta.join(format!("{uf}.md")), linhas.join("\n") + "\n")?;
            estados.push(json!({ "ordem": ordem, "uf": uf, "estado": nome_uf, "regiao": regiao, "capital": cap["nome"], "total": cidades.len() }));
            capitais.push(json!({ "ordem": ordem, "uf": uf, "estado": nome_uf, "capital": cap["nome"], "ibge": cap["ibge"], "n_no_estado": cap["n"] }));
            total += cidades.len();
        }
    }
    let brasil = lista(get("municipios")?)?.len();
    if total != brasil {
        return Err(format!("soma dos estados ({total}) diferente do total do IBGE ({brasil})"));
    }
    let mut linhas = vec![format!("# Estados na ordem de trabalho ({total} municipios, IBGE, {hoje})"), String::new()];
    linhas.extend(estados.iter().map(|e| {
        let t = |k: &str| e[k].as_str().unwrap_or("").to_string();
        format!("{:02}. {} {} ({}): {} municipios, capital {}", e["ordem"].as_i64().unwrap_or(0), t("uf"), t("estado"), t("regiao"), e["total"], t("capital"))
    }));
    let indice = json!({ "fonte": API, "baixado_em": hoje, "total_municipios": total, "estados": estados });
    grava(&pasta.join("estados.json"), bonito(&indice))?;
    grava(&pasta.join("capitais.json"), bonito(&json!(capitais)))?;
    grava(&pasta.join("estados.md"), linhas.join("\n") + "\n")?;
    Ok(indice)
}

/// Lista gravada de um estado; None se ainda nao foi baixada (nunca completa de memoria).
pub fn estado(pasta: &Path, uf: &str) -> Option<Value> {
    let uf = uf.trim().to_uppercase();
    if uf.len() != 2 || !uf.bytes().all(|b| b.is_ascii_uppercase()) {
        return None;
    }
    serde_json::from_str(&std::fs::read_to_string(pasta.join(format!("{uf}.json"))).ok()?).ok()
}

pub fn procurar(pasta: &Path, uf: &str, termo: &str) -> Vec<Value> {
    let t = chave(termo);
    estado(pasta, uf).and_then(|d| d["cidades"].as_array().cloned()).unwrap_or_default().into_iter().filter(|c| chave(c["nome"].as_str().unwrap_or("")).contains(&t)).collect()
}

/// As linhas "001. Nome (IBGE n) [CAPITAL]" de um estado (ou so as que casam com o termo). None = sem lista baixada.
pub fn mostrar(pasta: &Path, uf: &str, termo: Option<&str>) -> Option<Vec<String>> {
    let d = estado(pasta, uf)?;
    let cidades = match termo {
        Some(t) => procurar(pasta, uf, t),
        None => d["cidades"].as_array().cloned().unwrap_or_default(),
    };
    Some(cidades.iter().map(linha_cidade).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::path::PathBuf;

    fn tmp(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-ibge_cidades-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn ordem_e_capitais_cobrem_os_27() {
        let ufs: Vec<&str> = ORDEM.iter().flat_map(|r| r.1.iter().copied()).collect();
        assert_eq!((ufs.len(), ufs.iter().collect::<HashSet<_>>().len()), (27, 27));
        assert!(ufs.iter().all(|u| capital(u).is_some()));
        assert_eq!((&ufs[..2], ORDEM.map(|r| r.0)), (&["BA", "SE"][..], ["Nordeste", "Norte", "Centro-Oeste", "Sudeste", "Sul"]));
        // codigo IBGE da capital comeca pelo codigo do estado: pega troca de capital entre estados
        assert_eq!(CAPITAIS.iter().map(|c| c.1 / 100000).collect::<HashSet<_>>().len(), 27);
    }

    #[test]
    fn estado_sem_arquivo_nao_inventa() {
        let t = tmp("vazio");
        assert!(estado(&t, "SE").is_none() && procurar(&t, "SE", "aracaju").is_empty() && mostrar(&t, "SE", None).is_none());
        assert!(estado(&t, "../x").is_none() && estado(&t, "").is_none());
    }

    /// IBGE falso: cada estado tem a capital e mais duas cidades inventadas.
    fn ibge(caminho: &str, falta_capital_de: &str, sobra: usize) -> Result<Value, String> {
        let cidades = |uf: &str| -> Vec<Value> {
            let c = capital(uf).unwrap();
            let mut v = vec![json!({"id": c + 2, "nome": "Zulmira"}), json!({"id": c + 1, "nome": "Água Boa"})];
            if uf != falta_capital_de {
                v.push(json!({"id": c, "nome": format!("Capital de {uf}")}));
            }
            v
        };
        Ok(match caminho {
            "estados" => json!(CAPITAIS.iter().map(|c| json!({"sigla": c.0, "nome": format!("Estado {}", c.0)})).collect::<Vec<_>>()),
            "municipios" => json!(CAPITAIS.iter().flat_map(|c| cidades(c.0)).chain((0..sobra).map(|_| json!({}))).collect::<Vec<_>>()),
            c => json!(cidades(c.split('/').nth(1).ok_or("caminho")?)),
        })
    }

    #[test]
    fn baixa_com_a_capital_em_primeiro_e_o_resto_em_ordem_alfabetica() {
        let t = tmp("baixa");
        let i = baixar(&t, Data::civil(2026, 11, 18).unwrap(), &|c| ibge(c, "", 0)).unwrap();
        assert_eq!((i["total_municipios"].as_i64(), i["estados"].as_array().map(Vec::len), i["estados"][0]["uf"].as_str(), i["estados"][26]["uf"].as_str()), (Some(81), Some(27), Some("BA"), Some("RS")));
        assert_eq!(mostrar(&t, "se", None).unwrap(), ["001. Capital de SE (IBGE 2800308) [CAPITAL]", "002. Água Boa (IBGE 2800309)", "003. Zulmira (IBGE 2800310)"]);
        assert_eq!(mostrar(&t, "SE", Some("AGUA")).unwrap(), ["002. Água Boa (IBGE 2800309)"]);
        let md = std::fs::read_to_string(t.join("SE.md")).unwrap();
        assert!(md.starts_with("# 02. Estado SE (SE), Nordeste: 3 municipios\nFonte: IBGE, https://servicodados.ibge.gov.br/api/v1/localidades/estados/SE/municipios (baixado em 2026-11-18). Capital: Capital de SE.\n\n001."));
        assert!(std::fs::read_to_string(t.join("estados.md")).unwrap().contains("\n20. DF Estado DF (Centro-Oeste): 3 municipios, capital Capital de DF\n"));
        let caps: Value = serde_json::from_str(&std::fs::read_to_string(t.join("capitais.json")).unwrap()).unwrap();
        assert_eq!(caps[1], json!({"ordem": 2, "uf": "SE", "estado": "Estado SE", "capital": "Capital de SE", "ibge": 2800308, "n_no_estado": 1}));
    }

    #[test]
    fn conta_que_nao_fecha_com_o_ibge_e_erro() {
        let hoje = Data::civil(2026, 11, 18).unwrap();
        assert_eq!(baixar(&tmp("e1"), hoje, &|c| ibge(c, "PE", 0)).unwrap_err(), "capital de PE nao encontrada na lista do IBGE");
        assert_eq!(baixar(&tmp("e2"), hoje, &|c| ibge(c, "", 2)).unwrap_err(), "soma dos estados (81) diferente do total do IBGE (83)");
        let t = tmp("e3");
        assert_eq!(baixar(&t, hoje, &|c| if c == "estados" { Ok(json!([{"sigla": "BA", "nome": "Bahia"}])) } else { ibge(c, "", 0) }).unwrap_err(), "ORDEM/CAPITAIS nao batem com os estados do IBGE");
        assert_eq!(baixar(&t, hoje, &|_| Err("IBGE fora do ar".into())).unwrap_err(), "IBGE fora do ar");
        assert!(!t.exists());
    }
}
