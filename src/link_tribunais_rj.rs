//! Molde do Rio de Janeiro, capital (link/tribunais/rj.py): Radar Carioca, o painel Power BI de execucao orcamentaria do TCM-RJ.
//! Despesas_Mensal (por PT = orgao/unidade/funcao/subfuncao/programa/acao e mes: Dotacao Inicial, Atualizada, Empenhadas e
//! Liquidadas) e Receitas (por origem e mes, ja liquidas do Fundeb). 2026 vale ate o ultimo mes com liquidado de TODOS os orgaos
//! que liquidam todo mes e com receita. Consulta cortada (30 mil linhas) ou recusada, PT fora do padrao, tipo de despesa
//! desconhecido ou valor que nao e numero = Err (pendencia, nunca video). So a capital: o resto do RJ e do TCE-RJ.
//! Entra como closure: `consulta(comando)` manda o comando (montado aqui, o mesmo formato do molde de GO) ao painel e devolve a
//! resposta crua; a chave publica, o modelo e a pausa entre consultas sao de quem chama. Ficou de fora: os brutos em disco.
#![allow(dead_code)]
use crate::link_tribunais_base::{self as base, Dados};
use crate::link_tribunais_go::{comando, decodifica, Filtros};
use crate::link_tribunais_util::{self as u, num0, Acoes};
use crate::texto_pt::sem_acento;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const IBGE: i64 = 3304557;
pub const PT: &str = "PT  (ESFERA /ÓRGÃO_UO /UNIDADE_ORÇ / FÇ /SUBFÇ/ PRG /ACAO) + Descricao ACAO";
const ORGAO: &str = "DESCRIÇÃO ÓRGÃO SUPERIOR (Orçamento)";
const TIPOS: [&str; 4] = ["Dotação Inicial", "Dotação Atualizada", "Despesas Empenhadas", "Despesas Liquidadas"];
pub type Consulta<'a> = &'a dyn Fn(&Value) -> Result<String, String>;

pub fn tribunal() -> Value {
    json!({ "sigla": "TCM-RJ", "fala": "Tribunal de Contas do Município", "uf": "RJ", "estado": "Rio de Janeiro", "doc_despesa": "RADAR CARIOCA DESPESAS", "doc_receita": "RADAR CARIOCA RECEITAS",
        "tribunal_estado": "TCM-RJ (Tribunal de Contas do Município do Rio de Janeiro)", "site_tribunal": "tcmrio.tc.br",
        "fonte_md": "Tudo do Radar Carioca, o painel de execução orçamentária do TCM-RJ (Tribunal de Contas do Município do Rio de Janeiro), aberto pelo Portal do Controle Social > Execução Orçamentária (www.tcmrio.tc.br/t/painelpbi_radarcarioca): Receitas (valor arrecadado por rubrica e mês; transferências já líquidas da retenção do Fundeb) e Despesas (Dotação Inicial, Dotação Atualizada, Empenhadas e Liquidadas por ação e mês). \"Gastou\" = LIQUIDADO, município inteiro (Prefeitura, Câmara, TCM, autarquias, fundações, empresas dependentes e fundos), intraorçamentárias incluídas nos dois lados. \"Previa\" = Dotação Inicial (lei do orçamento).",
        "setores_md": "Setores = soma do liquidado das ações (projeto/atividade) do Radar Carioca do TCM-RJ, pela função; festas e eventos separados da cultura; segurança pública fora.",
        "pendencias_md": [], "orgaos": "Prefeitura, Câmara, autarquias, fundações, empresas dependentes e fundos",
        "legenda_docs": ["• Receita: Radar Carioca do TCM-RJ, tabela de Receitas (valor arrecadado) de 2024, 2025 e 2026", "• Despesa, setores e festas: Radar Carioca do TCM-RJ, Despesas por ação (Dotação Inicial e Liquidadas)", "• www.tcmrio.tc.br/t/painelpbi_radarcarioca"] })
}

fn aspas(vs: &[&str]) -> Vec<String> {
    vs.iter().map(|v| format!("'{v}'")).collect()
}

/// 2026: ultimo mes com liquidado de TODOS os orgaos que liquidam com regularidade (pelo menos metade dos meses) e com receita.
/// `om` = (orgao, mes, liquidado); `rec` = (mes "jan".."dez", valor).
pub fn corte26(om: &[Value], rec: &[Value]) -> Result<usize, String> {
    let mut por: BTreeMap<String, BTreeSet<i64>> = BTreeMap::new();
    for x in om {
        if num0(&x["Valor Despesas"])? != 0.0 {
            let m = &x["Mês"];
            por.entry(u::pystr(&x[ORGAO])).or_default().insert(m.as_i64().or_else(|| m.as_str().and_then(|s| s.trim().parse().ok())).ok_or_else(|| format!("mês que não é número: {m}"))?);
        }
    }
    let maior = |s: &BTreeSet<i64>| s.iter().next_back().copied().unwrap_or(0);
    let ultimo = por.values().map(maior).max().ok_or("TCM-RJ: nenhum órgão com liquidado em 2026 no Radar Carioca")?;
    let mes = por.values().filter(|s| s.len() as f64 >= ultimo as f64 / 2.0).map(maior).min().ok_or("TCM-RJ: nenhum órgão liquida com regularidade em 2026")?;
    let mut mrec = None;
    for x in rec {
        if num0(&x["Valor"])? != 0.0 {
            let m = u::txt(x, "Mês")?;
            mrec = mrec.max(Some(base::ABR.iter().position(|a| *a == m).ok_or_else(|| format!("mês da receita fora do padrão: {m:?}"))? + 1));
        }
    }
    Ok((mes as usize).min(mrec.ok_or("TCM-RJ: nenhuma receita de 2026 no Radar Carioca")?))
}

/// Le os 3 anos pelo painel. Devolve os dados no formato da base e o mes de corte de 2026.
pub fn ler(consulta: Consulta) -> Result<(Dados, String), String> {
    let pede = |tabela: &str, colunas: &[&str], somas: &[&str], filtros: &Filtros| -> Result<Vec<Value>, String> {
        let nomes: Vec<&str> = colunas.iter().chain(somas).copied().collect();
        decodifica(&consulta(&comando(tabela, colunas, somas, filtros))?, tabela, &nomes).map_err(|e| format!("Radar Carioca do TCM-RJ: {e}"))
    };
    let om = pede("Despesas_Mensal", &[ORGAO, "Mês"], &["Valor Despesas"], &[("Ano Referência", aspas(&["2026"])), ("Tipo de Despesas", aspas(&["Despesas Liquidadas"]))])?;
    let mes26 = corte26(&om, &pede("Receitas", &["Mês"], &["Valor"], &[("Ano Referência", aspas(&["2026"]))])?)?;
    let org: BTreeMap<String, String> = pede("Origem Receita", &["Cod", "Descrição Origem"], &[], &[])?.iter().map(|x| (u::pystr(&x["Cod"]), u::pystr(&x["Descrição Origem"]))).collect();
    let mut dados = Dados::new();
    for a in base::ANOS {
        let (mut fd, mut fr) = (vec![("Ano Referência", aspas(&[a]))], vec![("Ano Referência", aspas(&[a]))]);
        if a == "2026" {
            fd.push(("Mês", (1..=mes26).map(|m| format!("'{m}'")).collect()));
            fr.push(("Mês", aspas(&base::ABR[..mes26])));
        }
        fd.push(("Tipo de Despesas", aspas(&TIPOS)));
        let mut acoes = Acoes::default();
        for r in pede("Despesas_Mensal", &[ORGAO, "FUNÇÃO", "SUBFUNÇÃO", PT, "Tipo de Despesas"], &["Valor Despesas"], &fd)? {
            let pt = r[PT].as_str().unwrap_or("");
            let (p, nome) = pt.split_once(" - ").unwrap_or((pt, ""));
            if p.len() != 24 || !p.is_ascii() {
                return Err(format!("PT fora do padrão no Radar Carioca: {}", r[PT]));
            }
            // esfera+orgao.unidade.funcao.subfuncao.programa.acao (o script de setores le a subfuncao na 4a parte)
            let cod = [&p[..5], &p[5..11], &p[11..13], &p[13..16], &p[16..20], &p[20..]].join(".");
            let mut f = u::txt(&r, "FUNÇÃO")?.split(" - ").next().unwrap_or("");
            // o TCM-RJ tambem e funcao 01 Legislativa: no video a "Camara" e so a Camara; o tribunal vai a "Outros"
            if f == "01" && r[ORGAO].as_str().unwrap_or("").to_uppercase().contains("TRIBUNAL DE CONTAS") {
                f = "0";
            }
            let (tipo, v) = (u::pystr(&r["Tipo de Despesas"]), num0(&r["Valor Despesas"])?);
            let x = acoes.entra(&cod);
            (x.f, x.sub, x.acao, x.cod) = (f.into(), u::txt(&r, "SUBFUNÇÃO")?.split(" - ").next().unwrap_or("").into(), nome.to_uppercase(), cod);
            match tipo.as_str() {
                "Dotação Inicial" => x.orcado += v,
                "Dotação Atualizada" => x.atualizado += v,
                "Despesas Empenhadas" => x.empenhado += v,
                "Despesas Liquidadas" => x.liquidado += v,
                _ => return Err(format!("tipo de despesa desconhecido no Radar Carioca: {tipo}")),
            }
        }
        let mut orig = vec![];
        for r in pede("Receitas", &["ORIGEM"], &["Valor"], &fr)? {
            let c = u::txt(&r, "ORIGEM")?;
            if c.is_empty() {
                return Err("receita sem código de origem no Radar Carioca".into());
            }
            let nome = org.get(c).cloned().unwrap_or_else(|| format!("Origem {c}")) + if c.starts_with(['7', '8']) { " (intraorçamentária)" } else { "" };
            u::soma(&mut orig, &nome, num0(&r["Valor"])?);
        }
        acoes.lista.retain(|x| x.orcado != 0.0 || x.atualizado != 0.0 || x.empenhado != 0.0 || x.liquidado != 0.0);
        dados.insert(a.into(), u::ano(acoes.lista, orig, format!("Radar Carioca TCM-RJ · Despesas {a}"), format!("Radar Carioca TCM-RJ · Receitas {a}")));
    }
    Ok((dados, format!("{mes26:02}")))
}

/// O que a esteira chama. So a capital.
pub fn preparar(consulta: Consulta, setores: &dyn Fn(&Path) -> Result<(), String>, numero: &str, nome: &str, pasta: &Path) -> Result<Value, String> {
    if sem_acento(nome).to_uppercase() != "RIO DE JANEIRO" {
        return Err("o molde do TCM-RJ é só da capital; as outras cidades do RJ são do TCE-RJ".into());
    }
    let (dados, mes26) = ler(consulta)?;
    base::montar(pasta, numero, nome, "RJ", IBGE, &tribunal(), &dados, &mes26, &base::Opcoes::default(), setores)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dsr(n: usize, linhas: &[Value]) -> String {
        let s: Vec<Value> = (0..n).map(|i| json!({ "N": format!("G{i}") })).collect();
        let dm: Vec<Value> = linhas.iter().enumerate().map(|(i, l)| if i == 0 { json!({ "S": s, "C": l }) } else { json!({ "C": l }) }).collect();
        json!({ "results": [{ "result": { "data": { "dsr": { "DS": [{ "PH": [{ "DM0": dm }] }] } } } }] }).to_string()
    }

    fn radar(cmd: &Value) -> Result<String, String> {
        let q = &cmd["SemanticQueryDataShapeCommand"]["Query"];
        let (tabela, ncol) = (q["From"][0]["Entity"].as_str().unwrap(), q["Select"].as_array().unwrap().len());
        let pt = |cod: &str, nome: &str| format!("{cod} - {nome}");
        Ok(match (tabela, ncol) {
            // secretaria liquida ate agosto; Camara ate julho; fundo eventual so em fevereiro
            ("Despesas_Mensal", 3) => {
                assert_eq!(q["Where"][1]["Condition"]["In"]["Values"][0][0]["Literal"]["Value"], "'Despesas Liquidadas'");
                dsr(3, &(1..=8).flat_map(|m| [json!(["SECRETARIA", m, 10.5]), json!(["CAMARA", m.to_string(), if m <= 7 { 1 } else { 0 }])]).chain([json!(["FUNDO", 2, 3])]).collect::<Vec<_>>())
            }
            ("Receitas", 2) if q["Select"][0]["Column"]["Property"] == "Mês" => dsr(2, &[json!(["jan", 1]), json!(["set", 5.5]), json!(["out", 0])]),
            ("Origem Receita", 2) => dsr(2, &[json!(["11", "Impostos, Taxas e Contribuições de Melhoria"]), json!(["71", "Impostos"])]),
            ("Despesas_Mensal", 6) => {
                let w = q["Where"].as_array().unwrap();
                if w[0]["Condition"]["In"]["Values"][0][0]["Literal"]["Value"] == "'2026'" {
                    assert_eq!(w[1]["Condition"]["In"]["Values"], json!([[{ "Literal": { "Value": "'1'" } }], [{ "Literal": { "Value": "'2'" } }], [{ "Literal": { "Value": "'3'" } }], [{ "Literal": { "Value": "'4'" } }], [{ "Literal": { "Value": "'5'" } }], [{ "Literal": { "Value": "'6'" } }], [{ "Literal": { "Value": "'7'" } }]]));
                }
                dsr(6, &[
                    json!(["SECRETARIA DE CULTURA", "13 - Cultura", "392 - Difusão Cultural", pt("101001300113392030011234", "Projeto Carnaval"), "Dotação Inicial", 1000]),
                    json!(["SECRETARIA DE CULTURA", "13 - Cultura", "392 - Difusão Cultural", pt("101001300113392030011234", "Projeto Carnaval"), "Dotação Atualizada", 1200]),
                    json!(["SECRETARIA DE CULTURA", "13 - Cultura", "392 - Difusão Cultural", pt("101001300113392030011234", "Projeto Carnaval"), "Despesas Empenhadas", 900]),
                    json!(["SECRETARIA DE CULTURA", "13 - Cultura", "392 - Difusão Cultural", pt("101001300113392030011234", "Projeto Carnaval"), "Despesas Liquidadas", 800.5]),
                    json!(["Tribunal de Contas do Município", "01 - Legislativa", "032 - Controle Externo", pt("102002000101032000102000", "Fiscalização - externa"), "Despesas Liquidadas", 50]),
                    json!(["CÂMARA MUNICIPAL", "01 - Legislativa", "031 - Ação Legislativa", pt("101003000101031000102100", "Processo Legislativo"), "Despesas Liquidadas", 70]),
                    json!(["SECRETARIA", "04 - Administração", "122 - Adm", pt("101004000104122000102999", "Zerada"), "Dotação Inicial", null]),
                ])
            }
            ("Receitas", 2) => {
                if q["Where"][0]["Condition"]["In"]["Values"][0][0]["Literal"]["Value"] == "'2026'" {
                    assert_eq!(q["Where"][1]["Condition"]["In"]["Values"].as_array().unwrap().len(), 7);
                    assert_eq!(q["Where"][1]["Condition"]["In"]["Values"][6][0]["Literal"]["Value"], "'jul'");
                }
                dsr(2, &[json!(["11", 5000]), json!(["71", 100]), json!(["24", 30.5])])
            }
            x => return Err(format!("consulta inesperada {x:?}")),
        })
    }

    #[test]
    fn le_por_acao_e_corta_no_mes_de_todos() {
        let (dados, m) = ler(&radar).unwrap();
        assert_eq!(m, "07"); // a Camara liquidou ate julho; o fundo eventual nao conta; a receita vai ate setembro
        let a = &dados["2026"];
        assert_eq!(a.acoes.len(), 3); // a zerada sai
        let x = &a.acoes[0];
        assert_eq!((x.acao.as_str(), x.cod.as_str(), x.f.as_str(), x.sub.as_str()), ("PROJETO CARNAVAL", "10100.130011.33.920.3001.1234", "13", "392"));
        assert_eq!((x.orcado, x.atualizado, x.empenhado, x.liquidado), (1000.0, 1200.0, 900.0, 800.5));
        assert_eq!((a.acoes[1].f.as_str(), a.acoes[1].acao.as_str(), a.acoes[2].f.as_str()), ("0", "FISCALIZAÇÃO - EXTERNA", "01")); // o tribunal sai da funcao 01
        assert_eq!(a.receita, [("Impostos, Taxas e Contribuições de Melhoria".to_string(), 5000.0), ("Impostos (intraorçamentária)".to_string(), 100.0), ("Origem 24".to_string(), 30.5)]);
        assert_eq!((a.liquidado, a.doc_receita.as_str()), (920.5, "Radar Carioca TCM-RJ · Receitas 2026"));
    }

    #[test]
    fn recusa_o_que_falta() {
        let troca = |de: &'static str, para: &'static str| move |cmd: &Value| radar(cmd).map(|t| t.replace(de, para));
        assert!(ler(&troca("101001300113392030011234", "1010013001133920300112")).unwrap_err().contains("PT fora do padrão"));
        assert!(ler(&troca("Despesas Empenhadas", "Despesas Pagas")).unwrap_err().contains("tipo de despesa desconhecido"));
        assert!(ler(&troca("800.5", "\"oitocentos\"")).unwrap_err().contains("não é número"));
        assert!(ler(&troca("\"PH\"", "\"RT\":[[1]],\"PH\"")).unwrap_err().contains("Radar Carioca do TCM-RJ: consulta Despesas_Mensal passou de 30 mil linhas"));
        assert!(ler(&troca("\"set\"", "\"setembro\"")).unwrap_err().contains("mês da receita fora do padrão"));
        assert!(corte26(&[], &[]).unwrap_err().contains("nenhum órgão com liquidado"));
        let p = std::env::temp_dir().join(format!("braniac-link_tribunais_rj-{}", std::process::id()));
        assert!(preparar(&radar, &|_| Ok(()), "1", "Niterói", &p).unwrap_err().contains("só da capital"));
    }
}
