//! Moldes por estado (link/tribunais/__init__.py): o numero do video vem SO do tribunal de contas que fiscaliza o municipio, do
//! jeito que o guia daquele estado manda. Nunca do Tesouro/RREO nem do portal da prefeitura. Estado sem molde = pendencia.
//! Aqui: o que o guia diz (`guia`), a situacao do molde para o painel (`situacao`), os moldes conferidos (dados/magno/moldes.json)
//! e os moldes PENDENTES (al, ap, pa, rr, se): o tribunal nao publica o que o video precisa, entao `pendente` devolve o motivo e
//! `preparar_pendente` sempre recusa. Nada e estimado.
//! Ficou de fora: a conferencia ao vivo que os moldes pendentes do Python fazem (rede/navegador) para ver se o tribunal abriu
//! alguma base; o motivo aqui e o da conferencia registrada no Python.
#![allow(dead_code)]
use crate::texto_pt::sem_acento;
use serde_json::{json, Map, Value};
use std::path::Path;

/// Moldes com leitor portado em Rust (parse do que ja foi baixado).
pub const PORTADOS: [&str; 4] = ["CE", "MG", "MT", "RS"];
/// Moldes que existem no Python (o leitor de cada um ainda precisa ser portado, menos os de PORTADOS).
pub const NO_PYTHON: [&str; 25] = ["AC", "AL", "AM", "AP", "CE", "ES", "GO", "MA", "MG", "MS", "MT", "PA", "PB", "PE", "PI", "PR", "RJ", "RN", "RO", "RR", "RS", "SC", "SE", "SP", "TO"];

/// (UF, capital para a qual o molde foi feito ou "", motivo da pendencia).
const PENDENTES: [(&str, &str, &str); 5] = [
    ("AL", "MACEIO", "TCE-AL sem base pública com despesa por ação e receita por origem (pendência, nada estimado): a Consulta de Receitas e Despesas é o RREO do SICONFI republicado, só totais (fonte proibida); o SIAP (balancetes e remessas) exige login; as peças da Prestação de Contas de Governo só abrem com autenticação e não há 2026"),
    ("AP", "MACAPA", "TCE-AP sem base pública com a despesa do município inteiro por ação (pendência, nada estimado): a Consulta Despesas de Municípios só tem o balancete da administração direta, sem saúde, educação, Câmara e previdência; o Portal do Jurisdicionado exige login; Contas de Governo e Busca de Processo só têm andamento e parecer"),
    ("PA", "", "O TCM-PA não publica a receita arrecadada nem a despesa por ação (orçado e liquidado) dos municípios: o Raio-X dos Municípios só tem recortes, o Painel da Transparência Municipal só leva ao portal da prefeitura e a Consulta Pública do SPE exige captcha. Pedir os balancetes ao TCM-PA pela ouvidoria"),
    ("RR", "BOA VISTA", "TCE-RR sem base pública com 2026 e com a despesa por ação (orçado, atualizado, empenhado e liquidado) (pendência, nada estimado): o Roraicontas só tem prestações ANUAIS, com um valor por ação; os painéis de Educação e Saúde republicam fonte federal (proibida); os demais sistemas exigem login"),
    ("SE", "ARACAJU", "TCE-SE sem base pública com despesa por ação, orçado por ação e receita de 2026 (pendência, nada estimado): o Painel de Receitas e Despesas não tem projeto/atividade nem dotação; a API de Dados Abertos só aceita as unidades do próprio tribunal; o SAGRES exige login; a consulta processual só mostra a tramitação"),
];

fn chave(nome: &str) -> String {
    sem_acento(nome).to_uppercase()
}

/// Motivo da pendencia do molde daquele estado, ou None se o estado nao esta marcado como pendente.
pub fn pendente(uf: &str) -> Option<&'static str> {
    PENDENTES.iter().find(|(u, _, _)| u.eq_ignore_ascii_case(uf)).map(|(_, _, m)| *m)
}

/// O "preparar" de um molde pendente: NUNCA devolve dado, so o motivo (vira pendencia na esteira). None = o estado nao e pendente.
pub fn preparar_pendente(uf: &str, nome: &str) -> Option<String> {
    let (u, capital, motivo) = PENDENTES.iter().find(|(u, _, _)| u.eq_ignore_ascii_case(uf))?;
    if !capital.is_empty() && chave(nome) != *capital {
        return Some(format!("molde de {u} feito só para a capital"));
    }
    Some(format!("{u} ({nome}): {motivo}"))
}

/// Do guia do estado (JSON ja lido): o tribunal que fiscaliza (o da capital onde ha um so para ela) e os sistemas de consulta.
pub fn guia(uf: &str, d: &Value, capital: bool) -> Result<Value, String> {
    let cap = if capital { d["outros_tribunais"].as_array().and_then(|o| o.iter().find(|o| o["escopo"].as_str().unwrap_or("").to_lowercase().contains("capital"))) } else { None };
    let base = cap.unwrap_or(d);
    let t = cap.unwrap_or(&d["tribunal"]);
    if !t["sigla"].is_string() || !t["nome"].is_string() {
        return Err(format!("guia de {uf} sem tribunal"));
    }
    let lista = |v: &Value| v.as_array().cloned().unwrap_or_default();
    let sistemas: Vec<Value> = lista(&base["sistemas"]).into_iter().filter(|s| s["tipo"] == "consulta").map(|s| s["nome"].clone()).collect();
    Ok(json!({ "uf": uf, "sigla": t["sigla"], "nome": t["nome"], "site": base["site"], "sistemas": sistemas, "passos": lista(&base["passo_a_passo_acesso"]), "lacunas": lista(&d["lacunas"]) }))
}

/// Moldes conferidos: {UF: data}.
pub fn conferidos(dados: &Path) -> Map<String, Value> {
    std::fs::read_to_string(dados.join("magno").join("moldes.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn marcar_conferido(dados: &Path, uf: &str, quando: &str) -> Result<(), String> {
    let mut d = conferidos(dados);
    d.insert(uf.into(), json!(quando));
    std::fs::create_dir_all(dados.join("magno")).map_err(|e| e.to_string())?;
    std::fs::write(dados.join("magno").join("moldes.json"), Value::Object(d).to_string()).map_err(|e| e.to_string())
}

/// Para o painel: molde conferido, em teste (existe mas ninguem conferiu), pendente ou em construcao, e de onde (pelo guia) vem os numeros.
/// Diferenca do Python: molde pendente aparece como "pendente" (la aparecia como "em teste", porque o modulo existe).
pub fn situacao(dados: &Path, uf: &str, guia_do_estado: &Value, capital: bool) -> Result<Value, String> {
    let g = guia(uf, guia_do_estado, capital)?;
    let sis: Vec<&str> = g["sistemas"].as_array().into_iter().flatten().filter_map(Value::as_str).take(2).collect();
    let fonte = format!("{}{}", g["sigla"].as_str().unwrap_or(""), if sis.is_empty() { String::new() } else { format!(" · {}", sis.join(", ")) });
    let molde = if conferidos(dados).get(uf).is_some_and(|v| !v.is_null()) {
        "conferido"
    } else if pendente(uf).is_some() {
        "pendente"
    } else if NO_PYTHON.contains(&uf) {
        "em teste"
    } else {
        "em construção"
    };
    Ok(json!({ "molde": molde, "fonte_guia": fonte }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pendente_devolve_motivo_e_nunca_dado() {
        for uf in ["AL", "AP", "PA", "RR", "SE"] {
            assert!(pendente(uf).unwrap().len() > 80, "{uf}");
        }
        assert!(pendente("CE").is_none() && preparar_pendente("CE", "Fortaleza").is_none());
        assert!(preparar_pendente("AL", "Maceió").unwrap().contains("nada estimado"));
        assert!(preparar_pendente("AL", "Arapiraca").unwrap().contains("só para a capital"));
        assert!(preparar_pendente("pa", "Santarém").unwrap().starts_with("PA (Santarém): O TCM-PA não publica"));
        assert!(preparar_pendente("RR", "boa vista").unwrap().contains("ANUAIS"));
    }

    #[test]
    fn guia_e_situacao() {
        let g = json!({ "tribunal": { "sigla": "TCE-XX", "nome": "Tribunal do Estado" }, "site": "tce.exemplo", "lacunas": ["falta x"],
            "sistemas": [{ "nome": "Portal", "tipo": "consulta" }, { "nome": "Envio", "tipo": "remessa" }, { "nome": "Painel", "tipo": "consulta" }, { "nome": "Outro", "tipo": "consulta" }],
            "outros_tribunais": [{ "sigla": "TCM-XX", "nome": "Tribunal do Município", "escopo": "só a Capital", "site": "tcm.exemplo", "sistemas": [{ "nome": "Iris", "tipo": "consulta" }] }] });
        let cap = guia("XX", &g, true).unwrap();
        assert!(cap["sigla"] == "TCM-XX" && cap["site"] == "tcm.exemplo" && cap["sistemas"] == json!(["Iris"]) && cap["lacunas"] == json!(["falta x"]));
        let est = guia("XX", &g, false).unwrap();
        assert!(est["sigla"] == "TCE-XX" && est["sistemas"] == json!(["Portal", "Painel", "Outro"]));
        assert!(guia("XX", &json!({}), true).is_err());
        let d = std::env::temp_dir().join(format!("braniac-link_tribunais-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        assert_eq!(situacao(&d, "XX", &g, false).unwrap(), json!({ "molde": "em construção", "fonte_guia": "TCE-XX · Portal, Painel" }));
        assert_eq!(situacao(&d, "MG", &g, true).unwrap()["molde"], "em teste");
        assert_eq!(situacao(&d, "SE", &g, true).unwrap()["molde"], "pendente");
        marcar_conferido(&d, "MG", "16/11/2026").unwrap();
        marcar_conferido(&d, "CE", "17/11/2026").unwrap();
        assert_eq!(situacao(&d, "MG", &g, true).unwrap()["molde"], "conferido");
        assert_eq!(conferidos(&d).len(), 2);
        let _ = std::fs::remove_dir_all(&d);
    }
}
