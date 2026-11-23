//! Molde do Rio Grande do Norte (link/tribunais/rn.py): Painel Fiscal do Observatorio do TCE-RN (Power BI publico, dados do SIAI
//! Fiscal). Despesa por funcao e subfuncao (o tribunal nao publica a acao), receita por rubrica e deducoes, tudo acumulado no
//! arquivo do bimestre da PREFEITURA (o consolidado do municipio). Ano fechado sem o 6o bimestre, bimestre sem despesa ou
//! receita, ano que o painel nao publica, consulta recusada ou com 30 mil linhas (cortada), rubrica sem origem = Err.
//! Entra como closure: `consulta(corpo)` manda o corpo (montado aqui) a API publica do painel e devolve a resposta crua; a chave
//! publica, as tentativas e a pausa sao de quem chama. Ficou de fora: o cache de 7 dias em disco.
#![allow(dead_code)]
use crate::link_tribunais_base::{self as base, Acao, Dados};
use crate::link_tribunais_go::dsr_bloco;
use crate::link_tribunais_util::{self as u, ini, num0, pystr};
use crate::texto_pt::sem_acento;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

/// Modelo do Painel Fiscal do Observatorio do TCE-RN.
pub const MODELO: i64 = 9015261;
pub type Consulta<'a> = &'a dyn Fn(&Value) -> Result<String, String>;
/// O que foi lido de um ano: linhas de despesa e receita, total das deducoes e o bimestre.
#[derive(Debug)]
pub struct Bruto {
    pub despesa: Vec<Vec<Value>>,
    pub receita: Vec<Vec<Value>>,
    pub deducoes: f64,
    pub bim: i64,
}

pub fn tribunal() -> Value {
    json!({ "sigla": "TCE-RN", "fala": "Tribunal de Contas do Estado", "uf": "RN", "estado": "Rio Grande do Norte", "doc_despesa": "SIAI FISCAL DESPESA", "doc_receita": "SIAI FISCAL RECEITA",
        "tribunal_estado": "TCE-RN (Tribunal de Contas do Estado do Rio Grande do Norte)", "site_tribunal": "tce.rn.gov.br/TransparenciaJurisdicionados/Observatorio",
        "fonte_md": "Tudo do SIAI Fiscal do TCE-RN (Tribunal de Contas do Estado do Rio Grande do Norte), pelo Observatório > Painéis de Transparência > Painel Fiscal (Transparência Jurisdicionados): receita realizada menos as deduções (Fundeb e restituições) e despesa por função e subfunção (dotação inicial, atualizada, empenhado e liquidado acumulados até o bimestre). \"Gastou\" = LIQUIDADO do arquivo consolidado do município. \"Previa\" = dotação inicial da lei do orçamento. O tribunal não publica a despesa por projeto/atividade: a linha mais fina é a subfunção.",
        "setores_md": "Setores = soma do liquidado por função e subfunção do SIAI Fiscal (Painel Fiscal do TCE-RN); festas e eventos separados da cultura; segurança pública fora.",
        "pendencias_md": ["O TCE-RN não publica a despesa por ação (projeto/atividade): a linha é a subfunção.", "API de dados abertos do TCE-RN (apidadosabertos.tce.rn.gov.br) fora do ar em 15/11/2026."],
        "orgaos": "Município inteiro (arquivo consolidado da Prefeitura no SIAI Fiscal: Câmara, secretarias, autarquias e fundos)",
        "legenda_docs": ["• Receita: SIAI Fiscal do TCE-RN, receita realizada menos deduções, de 2024, 2025 e 2026", "• Despesa, setores e festas: SIAI Fiscal do TCE-RN, dotação inicial e liquidado por função e subfunção", "• tce.rn.gov.br > Observatório > Painel Fiscal (Transparência Jurisdicionados)"] })
}

/// Corpo da consulta. `frm` (apelido, tabela); `sel` (tipo 'c' coluna | 's' soma, apelido, coluna); `onde` (apelido, coluna, valores:
/// texto vira literal com aspas, numero vira inteiro longo).
pub fn corpo(frm: &[(&str, &str)], sel: &[(char, &str, &str)], onde: &[(&str, &str, Vec<Value>)]) -> Value {
    let col = |a: &str, p: &str| json!({ "Expression": { "SourceRef": { "Source": a } }, "Property": p });
    let s: Vec<Value> = sel.iter().map(|(k, a, p)| if *k == 'c' { json!({ "Column": col(a, p), "Name": format!("{a}.{p}") }) } else { json!({ "Aggregation": { "Expression": { "Column": col(a, p) }, "Function": 0 }, "Name": format!("{a}.{p}") }) }).collect();
    let lit = |v: &Value| json!([{ "Literal": { "Value": v.as_str().map_or_else(|| format!("{v}L"), |t| format!("'{t}'")) } }]);
    let w: Vec<Value> = onde.iter().map(|(a, p, vs)| json!({ "Condition": { "In": { "Expressions": [{ "Column": col(a, p) }], "Values": vs.iter().map(lit).collect::<Vec<_>>() } } })).collect();
    let proj: Vec<usize> = (0..s.len()).collect();
    json!({ "version": "1.0.0", "modelId": MODELO, "cancelQueries": [], "queries": [{ "Query": { "Commands": [{ "SemanticQueryDataShapeCommand": {
        "Query": { "Version": 2, "From": frm.iter().map(|(a, e)| json!({ "Name": a, "Entity": e, "Type": 0 })).collect::<Vec<_>>(), "Select": s, "Where": w },
        "Binding": { "Primary": { "Groupings": [{ "Projections": proj }] }, "DataReduction": { "DataVolume": 4, "Primary": { "Window": { "Count": 30000 } } }, "Version": 1 } } }] } }] })
}

/// Linhas da resposta (formato DSR). Recusada, sem o bloco de dados ou com 30 mil linhas (cortada) = Err.
pub fn linhas(resposta: &str) -> Result<Vec<Vec<Value>>, String> {
    let v = u::json(resposta, "Painel Fiscal do TCE-RN")?;
    let ds = &v["results"][0]["result"]["data"]["dsr"]["DS"][0];
    let ph = ds["PH"].as_array().ok_or_else(|| format!("Painel Fiscal do TCE-RN recusou a consulta: {}", ini(&v.to_string(), 300)))?;
    if !ds["odata.error"].is_null() || !ds["Error"].is_null() {
        return Err(format!("Painel Fiscal do TCE-RN recusou a consulta: {}", ini(&ds.to_string(), 300)));
    }
    let mut out = vec![];
    for p in ph {
        for (chave, rows) in p.as_object().into_iter().flatten() {
            if chave.starts_with("DM") {
                out.extend(dsr_bloco(rows.as_array().map_or(&[][..], Vec::as_slice), &ds["ValueDicts"], true, "do Painel Fiscal do TCE-RN")?);
            }
        }
    }
    if out.len() >= 30000 || !(ds["RT"].is_null() || ds["RT"] == json!(false) || ds["RT"] == json!([])) {
        return Err("Painel Fiscal do TCE-RN: consulta passou de 30000 linhas (resultado cortado)".into());
    }
    Ok(out)
}

fn celula(x: &[Value], i: usize) -> Result<&Value, String> {
    x.get(i).ok_or_else(|| "Painel Fiscal do TCE-RN: linha com menos colunas que o pedido".to_string())
}

/// Acha a Prefeitura e os arquivos dela e le, de cada ano, o ultimo bimestre publicado (ano fechado: tem de ser o 6o).
/// Devolve o bruto por ano e o mes de corte de 2026.
pub fn baixar(consulta: Consulta, nome: &str) -> Result<(BTreeMap<String, Bruto>, String), String> {
    let q = |frm: &[(&str, &str)], sel: &[(char, &str, &str)], onde: &[(&str, &str, Vec<Value>)]| linhas(&consulta(&corpo(frm, sel, onde))?);
    let pref = format!("PREFEITURA MUNICIPAL DE {}", sem_acento(nome).to_uppercase());
    let orgs = q(&[("o", "Dim_Orgao")], &[('c', "o", "IdOrgao"), ('c', "o", "NomeOrgao")], &[("o", "EsferaOrgao", vec![json!("M")])])?;
    let mut org = vec![];
    for o in &orgs {
        if sem_acento(&pystr(celula(o, 1)?)).to_uppercase() == pref {
            org.push(celula(o, 0)?.clone());
        }
    }
    if org.len() != 1 {
        return Err(format!("TCE-RN: '{pref}' não achada (ou repetida) entre os jurisdicionados do Painel Fiscal"));
    }
    let arqs = q(&[("a", "Dim_ArquivoLRF")], &[('c', "a", "Ano"), ('c', "a", "Bimestre"), ('c', "a", "IdArquivoLRF")], &[("a", "IdOrgao", org), ("a", "Ano", base::ANOS.iter().map(|a| json!(a.parse::<i64>().unwrap_or(0))).collect())])?;
    let mut por: BTreeMap<(String, i64), Value> = BTreeMap::new();
    for x in &arqs {
        let b = celula(x, 1)?;
        por.insert((pystr(celula(x, 0)?), b.as_i64().or_else(|| b.as_str().and_then(|s| s.trim().parse().ok())).ok_or_else(|| format!("TCE-RN: bimestre que não é número: {b}"))?), celula(x, 2)?.clone());
    }
    let (mut out, mut mes26) = (BTreeMap::new(), None);
    for a in base::ANOS {
        for b in (1..=6).rev() {
            let Some(i) = por.get(&(a.to_string(), b)) else { continue };
            let arq = |ap: &'static str| [(ap, "IdArquivoLRF", vec![i.clone()])];
            let des = q(&[("d", "Fato_Despesa"), ("s", "Dim_SubfuncaoDespesa"), ("f", "Dim_FuncaoDespesa")], &[('c', "f", "CodigoFuncaoDespesa"), ('c', "s", "CodigoSubfuncaoDespesa"), ('c', "s", "DescricaoSubfuncaoDespesa"), ('s', "d", "ValorDotacaoInicial"), ('s', "d", "ValorDotacaoAtualizada"), ('s', "d", "ValorEmpenhadoExercicio"), ('s', "d", "ValorLiquidadoExercicio")], &arq("d"))?;
            let rec = q(&[("r", "Fato_Receita"), ("c", "Dim_ReceitaClassificacao")], &[('c', "c", "CodigoReceitaClassificacao"), ('s', "r", "ValorRealizadoExercicio")], &arq("r"))?;
            if a != "2026" && b != 6 {
                return Err(format!("TCE-RN: {a} sem o 6º bimestre da Prefeitura no SIAI Fiscal"));
            }
            if des.is_empty() || rec.is_empty() {
                if a == "2026" {
                    continue; // bimestre ainda sem dado publicado: tenta o anterior
                }
                return Err(format!("TCE-RN: arquivo do {b}º bimestre de {a} sem {} no Painel Fiscal", if des.is_empty() { "despesa" } else { "receita" }));
            }
            let mut deducoes = 0.0;
            for x in q(&[("r", "Fato_Receita_Deducoes")], &[('c', "r", "IdArquivoLRF"), ('s', "r", "ValorRealizadoExercicio")], &arq("r"))? {
                deducoes += num0(celula(&x, 1)?)?;
            }
            out.insert(a.to_string(), Bruto { despesa: des, receita: rec, deducoes, bim: b });
            if a == "2026" {
                mes26 = Some(format!("{:02}", 2 * b));
            }
            break;
        }
        if !out.contains_key(a) {
            let bims: Vec<i64> = por.keys().filter(|k| k.0 == a).map(|k| k.1).collect();
            return Err(format!("TCE-RN: o Painel Fiscal (SIAI Fiscal) ainda não publica receita e despesa de {a} de {nome} (a Prefeitura entregou os bimestres {bims:?} no SIAI Fiscal, mas o painel não tem os valores). Como a Prefeitura entregou, não é pauta de 'não prestou contas': esperar o TCE-RN publicar ou pedir os demonstrativos de {a} pela ouvidoria/e-SIC do TCE-RN"));
        }
    }
    Ok((out, mes26.ok_or("TCE-RN: 2026 sem bimestre lido")?))
}

/// O bruto de cada ano no formato da base: "acao" = subfuncao dentro da funcao.
pub fn ler(brutos: &BTreeMap<String, Bruto>) -> Result<Dados, String> {
    let mut dados = Dados::new();
    for (a, x) in brutos {
        let mut orig = vec![];
        for r in &x.receita {
            let c = pystr(celula(r, 0)?).trim().to_string();
            let nome = u::origem(ini(&c, 2)).or(c.starts_with(['7', '8']).then_some(u::INTRA)).ok_or_else(|| format!("TCE-RN: rubrica de receita sem origem conhecida: {c}"))?;
            u::soma(&mut orig, nome, num0(celula(r, 1)?)?);
        }
        orig.push(("Deduções da receita (Fundeb e restituições)".into(), -x.deducoes));
        let mut acoes = vec![];
        for r in &x.despesa {
            let (f, sub) = (pystr(celula(r, 0)?), pystr(celula(r, 1)?));
            acoes.push(Acao { cod: format!("{f}.{sub}"), f, sub, acao: celula(r, 2)?.as_str().unwrap_or("").trim().into(), orcado: num0(celula(r, 3)?)?, atualizado: num0(celula(r, 4)?)?, empenhado: num0(celula(r, 5)?)?, liquidado: num0(celula(r, 6)?)? });
        }
        let rot = format!("{}º bimestre/{a}", x.bim);
        dados.insert(a.clone(), u::ano(acoes, orig, format!("SIAI Fiscal TCE-RN · despesa por função e subfunção · {rot}"), format!("SIAI Fiscal TCE-RN · receita realizada menos deduções · {rot}")));
    }
    Ok(dados)
}

/// O que a esteira chama. So a capital tem codigo do IBGE no molde.
pub fn preparar(consulta: Consulta, setores: &dyn Fn(&Path) -> Result<(), String>, numero: &str, nome: &str, pasta: &Path) -> Result<Value, String> {
    let (brutos, mes26) = baixar(consulta, nome)?;
    let dados = ler(&brutos)?;
    if sem_acento(nome).to_uppercase() != "NATAL" {
        return Err(format!("TCE-RN: código IBGE de '{nome}' não cadastrado no molde"));
    }
    base::montar(pasta, numero, nome, "RN", 2408102, &tribunal(), &dados, &mes26, &base::Opcoes::default(), setores)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dsr(n: usize, linhas: &[Value]) -> String {
        let s: Vec<Value> = (0..n).map(|i| json!({ "N": format!("G{i}") })).collect();
        let dm: Vec<Value> = linhas.iter().enumerate().map(|(i, l)| if i == 0 { json!({ "S": s, "C": l }) } else { json!({ "C": l }) }).collect();
        json!({ "results": [{ "result": { "data": { "dsr": { "DS": [{ "PH": [{ "DM0": dm }] }] } } } }] }).to_string()
    }

    /// Painel de teste: 2024 e 2025 com o 6o bimestre; 2026 com o 4o entregue mas ainda sem valores e o 3o publicado.
    fn painel(c: &Value) -> Result<String, String> {
        let q = &c["queries"][0]["Query"]["Commands"][0]["SemanticQueryDataShapeCommand"]["Query"];
        assert_eq!(c["modelId"], MODELO);
        let arq = q["Where"][0]["Condition"]["In"]["Values"][0][0]["Literal"]["Value"].as_str().unwrap_or("").to_string();
        Ok(match q["From"][0]["Entity"].as_str().unwrap() {
            "Dim_Orgao" => {
                assert_eq!(arq, "'M'");
                dsr(2, &[json!([77, "Prefeitura Municipal de Natal"]), json!([78, "CÂMARA MUNICIPAL DE NATAL"]), json!([79, "PREFEITURA MUNICIPAL DE NATALÂNDIA"])])
            }
            "Dim_ArquivoLRF" => {
                assert!(arq == "77L" && q["Where"][1]["Condition"]["In"]["Values"][2][0]["Literal"]["Value"] == "2026L");
                dsr(3, &[json!([2024, 6, 246]), json!([2025, 6, 256]), json!([2025, 5, 255]), json!([2026, 3, 263]), json!([2026, "4", 264])])
            }
            "Fato_Despesa" if arq == "264L" => dsr(7, &[]),
            "Fato_Despesa" => dsr(7, &[json!(["13", "392", " Difusão Cultural ", 1000, 1200.5, 900, 800.25]), json!(["01", "031", "Ação Legislativa", 50, 50, 40, null])]),
            "Fato_Receita" => dsr(2, &[json!(["1112", 5000]), json!([" 1751 ", "300.5"]), json!(["7218", 20])]),
            "Fato_Receita_Deducoes" => dsr(2, &[json!([arq.clone(), 100]), json!([arq, 50.5])]),
            x => return Err(format!("consulta inesperada {x}")),
        })
    }

    #[test]
    fn le_o_ultimo_bimestre_publicado() {
        let (brutos, m) = baixar(&painel, "Natal").unwrap();
        assert_eq!((m.as_str(), brutos["2026"].bim, brutos["2025"].bim, brutos["2026"].deducoes), ("06", 3, 6, 150.5)); // o 4o bimestre ainda nao tem valores
        let dados = ler(&brutos).unwrap();
        let a = &dados["2026"];
        let x = &a.acoes[0];
        assert_eq!((x.acao.as_str(), x.cod.as_str(), x.f.as_str(), x.sub.as_str()), ("Difusão Cultural", "13.392", "13", "392"));
        assert_eq!((x.orcado, x.atualizado, x.empenhado, x.liquidado, a.acoes[1].liquidado), (1000.0, 1200.5, 900.0, 800.25, 0.0));
        assert_eq!(a.receita, [("Impostos, taxas e contribuições de melhoria".to_string(), 5000.0), ("Transferências correntes".to_string(), 300.5), (u::INTRA.to_string(), 20.0), ("Deduções da receita (Fundeb e restituições)".to_string(), -150.5)]);
        assert_eq!((a.arrecadado, a.doc_despesa.as_str()), (5170.0, "SIAI Fiscal TCE-RN · despesa por função e subfunção · 3º bimestre/2026"));
        let c = corpo(&[("d", "Fato_Despesa")], &[('c', "f", "Cod"), ('s', "d", "Valor")], &[("d", "Id", vec![json!(7), json!("x")])]);
        let q = &c["queries"][0]["Query"]["Commands"][0]["SemanticQueryDataShapeCommand"];
        assert!(q["Query"]["Select"][1]["Aggregation"]["Function"] == 0 && q["Query"]["Select"][0]["Name"] == "f.Cod" && q["Query"]["Where"][0]["Condition"]["In"]["Values"][1][0]["Literal"]["Value"] == "'x'");
    }

    #[test]
    fn decodifica_nulo_antes_da_repeticao_e_recusa_resposta_ruim() {
        let r = json!({ "results": [{ "result": { "data": { "dsr": { "DS": [{ "ValueDicts": { "D0": ["a", "b"] }, "PH": [{ "DM0": [{ "S": [{ "DN": "D0" }, {}], "C": [1, 5] }, { "C": [9], "R": 1, "Ø": 1 }], "X": 1 }, { "DM1": [{ "S": [{}], "C": [7] }] }] }] } } } }] }).to_string();
        assert_eq!(linhas(&r).unwrap(), [vec![json!("b"), json!(5)], vec![json!(null), json!(9)], vec![json!(7)]]);
        assert!(linhas(&r.replace("\"ValueDicts\"", "\"Error\":{\"c\":1},\"ValueDicts\"")).unwrap_err().contains("recusou a consulta"));
        assert!(linhas(r#"{"results":[{"result":{"data":{"error":"x"}}}]}"#).unwrap_err().contains("recusou a consulta"));
        assert!(linhas(&dsr(1, &vec![json!([1]); 30000])).unwrap_err().contains("resultado cortado"));
        assert!(linhas("Bad Gateway").is_err());
    }

    #[test]
    fn recusa_o_que_falta() {
        let troca = |de: &'static str, para: &'static str| move |c: &Value| painel(c).map(|t| t.replace(de, para));
        assert!(baixar(&painel, "Natalândia do Sul").unwrap_err().contains("'PREFEITURA MUNICIPAL DE NATALANDIA DO SUL' não achada"));
        assert!(baixar(&troca("[2025,6,256]", "[2023,6,256]"), "Natal").unwrap_err().contains("2025 sem o 6º bimestre"));
        assert!(baixar(&troca("[2024,6,246]", "[2023,6,246]"), "Natal").unwrap_err().contains("ainda não publica receita e despesa de 2024"));
        assert!(baixar(&troca("[2026,3,263]", "[2023,3,263]"), "Natal").unwrap_err().contains("de 2026 de Natal (a Prefeitura entregou os bimestres [4]"));
        let (b, _) = baixar(&troca("\"1112\"", "\"3112\""), "Natal").unwrap();
        assert!(ler(&b).unwrap_err().contains("sem origem conhecida: 3112"));
        let (b, _) = baixar(&troca("800.25", "\"x\""), "Natal").unwrap();
        assert!(ler(&b).unwrap_err().contains("não é número"));
        // ano fechado com o 6o bimestre entregue mas sem receita no painel
        let sem_rec = |c: &Value| if c.to_string().contains("Fato_Receita\"") && c.to_string().contains("246L") { Ok(dsr(2, &[])) } else { painel(c) };
        assert!(baixar(&sem_rec, "Natal").unwrap_err().contains("arquivo do 6º bimestre de 2024 sem receita"));
        let p = std::env::temp_dir().join(format!("braniac-link_tribunais_rn-{}", std::process::id()));
        assert!(preparar(&painel, &|_| Ok(()), "1", "Mossoró", &p).unwrap_err().contains("não achada"));
    }
}
