//! Molde de Goias (link/tribunais/go.py): Portal do Cidadao do TCM-GO, relatorios Power BI com dados do SICOM (Receitas,
//! Despesas por empenho e Orcamento/LOA). O tribunal nao publica o liquidado por acao: a "acao" aqui e a menor unidade publicada
//! na execucao (orgao + unidade orcamentaria + funcao + subfuncao); o orcado e a soma das acoes da LOA nessa chave.
//! Consulta que passou de 30 mil linhas (resposta cortada), consulta recusada, funcao/subfuncao sem codigo no molde ou valor
//! que nao e numero = Err (pendencia, nunca video).
//! Entra como closure: `consulta(pag, comando)` manda o comando (SemanticQueryDataShapeCommand, montado aqui) ao relatorio da
//! pagina `pag` ("despesas" | "receitas" | "orcamento") e devolve a resposta crua. Token, modelo, pausa entre consultas: de quem chama.
//! Ficou de fora: abrir a pagina e tirar o token (classe Painel) e os arquivos brutos de conferencia em disco.
#![allow(dead_code)]
use crate::link_tribunais_base::{self as base, Dados};
use crate::link_tribunais_util::{self as u, ini, num0, Acoes};
use crate::texto_pt::sem_acento;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const IBGE: i64 = 5208707;
pub type Consulta<'a> = &'a dyn Fn(&str, &Value) -> Result<String, String>;
pub type Filtros<'a> = [(&'a str, Vec<String>)];

pub fn tribunal() -> Value {
    json!({ "sigla": "TCM-GO", "fala": "Tribunal de Contas dos Municípios", "uf": "GO", "estado": "Goiás", "doc_despesa": "PORTAL DO CIDADÃO DESPESAS", "doc_receita": "PORTAL DO CIDADÃO RECEITAS",
        "tribunal_estado": "TCM-GO (Tribunal de Contas dos Municípios do Estado de Goiás)", "site_tribunal": "portalcidadao.tcmgo.tc.br",
        "fonte_md": "Tudo do Portal do Cidadão do TCM-GO (Tribunal de Contas dos Municípios do Estado de Goiás), portalcidadao.tcmgo.tc.br, dados do SICOM: Contabilidade > Receitas (total da receita por rubrica, já líquido das deduções do Fundeb), Contabilidade > Despesas (empenhos com órgão, unidade, função e subfunção; \"Gastou\" = LIQUIDADO, todos os órgãos do município) e Planejamento > Orçamento (\"Previa\" = valor da lei do orçamento).",
        "setores_md": "Setores = soma do liquidado dos empenhos (Portal do Cidadão do TCM-GO) por órgão, unidade, função e subfunção; festas e eventos separados da cultura; segurança pública fora.",
        "pendencias_md": ["O TCM-GO não publica o liquidado por ação (projeto/atividade): a execução do Portal do Cidadão vem por empenho, só com órgão, unidade, função e subfunção. Nem a dotação atualizada.", "A lei do orçamento de Cidade (Portal do Cidadão > Orçamento, 2024 a 2026) não tem ação de festas e eventos."],
        "orgaos": "Prefeitura, Câmara, autarquias, fundos e previdência (todos os órgãos do município no SICOM)",
        "legenda_docs": ["• Receita: Portal do Cidadão do TCM-GO, Contabilidade > Receitas, de 2024, 2025 e 2026", "• Despesa e setores: Portal do Cidadão do TCM-GO, Contabilidade > Despesas (liquidado) e Planejamento > Orçamento", "• portalcidadao.tcmgo.tc.br"] })
}

/// Nome da funcao no portal -> codigo da classificacao funcional (Portaria MOG 42/1999). Nome novo = para o molde (nada de chute).
pub fn funcao(nome: &str) -> Option<&'static str> {
    const T: [(&str, &str); 22] = [("Legislativa", "01"), ("Essencial à Justiça", "03"), ("Administração", "04"), ("Segurança Pública", "06"), ("Assistência social", "08"), ("Previdência Social", "09"), ("Saúde", "10"), ("Trabalho", "11"), ("Educação", "12"), ("Cultura", "13"), ("Direitos da Cidadania", "14"), ("Urbanismo", "15"), ("Habitação", "16"), ("Saneamento", "17"), ("Gestão Ambiental", "18"), ("Ciência e Tecnologia", "19"), ("Indústria", "22"), ("Comércio e Serviços", "23"), ("Transporte", "26"), ("Desporto e Lazer", "27"), ("Encargos Especiais", "28"), ("Reserva de Contingencia", "99")];
    T.iter().find(|x| x.0 == nome).map(|x| x.1)
}

/// Nome da subfuncao como o portal escreve (com os erros de digitacao dele) -> codigo.
pub fn subfuncao(nome: &str) -> Option<&'static str> {
    const T: [(&str, &str); 50] = [
        ("Ação Legislativa", "031"), ("Controle Externo", "032"), ("Representação Judicial e Extrajudicial", "092"), ("Administração Geral", "122"), ("Administração Financeira", "123"), ("Normatização e Fiscalização", "125"),
        ("Tecnologia e Informatização", "126"), ("Ordenamento Territorial", "127"), ("Formação de Recursos Humanos", "128"), ("Administração de Concessões", "130"), ("Comunicação Social", "131"), ("Policiamento", "181"),
        ("Defesa Civil", "182"), ("Assistência ao Idoso", "241"), ("Assistência à Criança e ao Adolecente", "243"), ("Assistência Comunitária", "244"), ("Previdência do Regime Estatutário", "272"), ("Atenção Básica", "301"),
        ("Assistênica Hospitalar e Ambulatorial", "302"), ("Suporte Profilático e Terapêutico", "303"), ("Vigilânica Epidemiológica", "305"), ("Alimentação e Nutrição", "306"), ("Proteção e Beneficios ao Trabalhador", "331"),
        ("Fomento ao Trabalho", "333"), ("Empregabilidade", "334"), ("Ensino Fundamental", "361"), ("Educação Infantil", "365"), ("Educação Jovens e Adultos", "366"), ("Educação Especial", "367"), ("Difusão Cultural", "392"),
        ("Direitos Individuais, Coletivos e Difusos", "422"), ("Infra-Estrutura Urbana", "451"), ("Serviços Urbanos", "452"), ("Transportes Coletivos Urbanos", "453"), ("Habitação Rural", "481"), ("Habitação Urbana", "482"),
        ("Saneamento Básico Urbano", "512"), ("Preservação e Conservação Ambiental", "541"), ("Desenvolvimento Tecnológico e Engenharia", "572"), ("Difusão do Conhecimento Científico e Tecnológico", "573"),
        ("Promoção Industrial", "661"), ("Promoção Comercial", "691"), ("Turismo", "695"), ("Tranporte Rodoviário", "782"), ("Desporto Comunitário", "812"), ("Lazer", "813"), ("Serviço da Dívida Interna", "843"),
        ("Serviço da Dívida Externa", "844"), ("Outros Encargos Especiais", "846"), ("Reserva de Contingencia", "999"),
    ];
    T.iter().find(|x| x.0 == nome).map(|x| x.1)
}

/// Origem da receita: 2 primeiros digitos; 7 e 8 = intraorcamentarias; 9 = deducoes (Fundeb).
fn origem(c: &str) -> String {
    u::origem(ini(c, 2)).or(match ini(c, 1) {
        "7" | "8" => Some(u::INTRA),
        "9" => Some("Deduções da receita (Fundeb)"),
        _ => None,
    })
    .map_or_else(|| format!("Natureza {}", ini(c, 2)), String::from)
}

/// O comando Power BI: agrupa `colunas`, soma `somas`, filtros (coluna, literais). Janela de 30 mil linhas.
pub fn comando(tabela: &str, colunas: &[&str], somas: &[&str], filtros: &Filtros) -> Value {
    let col = |p: &str| json!({ "Column": { "Expression": { "SourceRef": { "Source": "t" } }, "Property": p } });
    let mut sel: Vec<Value> = colunas.iter().enumerate().map(|(i, c)| json!({ "Column": col(c)["Column"], "Name": format!("c{i}") })).collect();
    sel.extend(somas.iter().enumerate().map(|(i, s)| json!({ "Aggregation": { "Expression": col(s), "Function": 0 }, "Name": format!("s{i}") })));
    let onde: Vec<Value> = filtros.iter().map(|(c, vs)| json!({ "Condition": { "In": { "Expressions": [col(c)], "Values": vs.iter().map(|v| json!([{ "Literal": { "Value": v } }])).collect::<Vec<_>>() } } })).collect();
    let proj: Vec<usize> = (0..sel.len()).collect();
    json!({ "SemanticQueryDataShapeCommand": { "Query": { "Version": 2, "From": [{ "Name": "t", "Entity": tabela, "Type": 0 }], "Select": sel, "Where": onde },
        "Binding": { "Primary": { "Groupings": [{ "Projections": proj }] }, "DataReduction": { "DataVolume": 4, "Primary": { "Window": { "Count": 30000 } } }, "Version": 1 } } })
}

/// Um bloco de linhas do formato DSR do Power BI (`S` = esquema, `C` = valores, `R` = repete a linha de cima, `Ø` = nulo, `DN` =
/// indice no dicionario da coluna). `nulo_primeiro`: qual marca vale quando as duas vem juntas (cada molde do Python faz de um jeito).
pub fn dsr_bloco(rows: &[Value], dicts: &Value, nulo_primeiro: bool, o_que: &str) -> Result<Vec<Vec<Value>>, String> {
    let curta = || format!("consulta {o_que}: linha da resposta com menos valores que colunas");
    let (mut sch, mut linhas): (Option<&Vec<Value>>, Vec<Vec<Value>>) = (None, vec![]);
    for row in rows {
        sch = row["S"].as_array().or(sch);
        let (mut it, rep, nulo) = (row["C"].as_array().map_or(&[][..], Vec::as_slice).iter(), row["R"].as_u64().unwrap_or(0), row["Ø"].as_u64().unwrap_or(0));
        let mut vals = vec![];
        for (i, s) in sch.ok_or_else(|| format!("consulta {o_que}: resposta sem o esquema das colunas"))?.iter().enumerate() {
            let (r, z) = (rep >> i & 1 == 1, nulo >> i & 1 == 1);
            let val = if z && (nulo_primeiro || !r) {
                Value::Null
            } else if r {
                linhas.last().and_then(|p| p.get(i)).cloned().ok_or_else(curta)?
            } else {
                let x = it.next().ok_or_else(curta)?;
                match (s["DN"].as_str(), x.as_u64()) {
                    (Some(dn), Some(n)) => dicts[dn].get(n as usize).cloned().ok_or_else(|| format!("consulta {o_que}: valor fora do dicionário {dn}"))?,
                    _ => x.clone(),
                }
            };
            vals.push(val);
        }
        linhas.push(vals);
    }
    Ok(linhas)
}

/// Decodifica a resposta de uma consulta em linhas {nome: valor}. Resposta recusada ou cortada (passou da janela de 30 mil
/// linhas) e erro: dividir a consulta, nunca somar pela metade.
pub fn decodifica(resposta: &str, tabela: &str, nomes: &[&str]) -> Result<Vec<Value>, String> {
    let v = u::json(resposta, tabela)?;
    let r = &v["results"][0]["result"]["data"];
    if r["dsr"].is_null() {
        return Err(format!("o painel do tribunal recusou a consulta {tabela}: {}", ini(&r.to_string(), 300)));
    }
    let ds = &r["dsr"]["DS"][0];
    if !ds.is_object() {
        return Err(format!("consulta {tabela}: resposta sem o bloco de dados"));
    }
    if !(ds["RT"].is_null() || ds["RT"] == json!(false) || ds["RT"] == json!([])) {
        return Err(format!("consulta {tabela} passou de 30 mil linhas; dividir a consulta"));
    }
    let linhas = dsr_bloco(ds["PH"][0]["DM0"].as_array().map_or(&[][..], Vec::as_slice), &ds["ValueDicts"], false, tabela)?;
    Ok(linhas.into_iter().map(|vals| Value::Object(nomes.iter().map(|n| n.to_string()).zip(vals).collect())).collect())
}

fn inteiro(v: &Value, o_que: &str) -> Result<i64, String> {
    v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())).ok_or_else(|| format!("{o_que} que não é número: {v}"))
}

/// 2026: ultimo mes com liquidado de TODOS os orgaos que liquidam todo mes (fundo que liquida de vez em quando nao conta; fundo
/// que liquida quase todo mes e parou = nao entregou) e com receita lancada. `om` = (orgao, mes, liquidado); `rec` = (mes, receita).
pub fn corte26(om: &[Value], rec: &[Value]) -> Result<i64, String> {
    let mut por: BTreeMap<String, BTreeSet<i64>> = BTreeMap::new();
    for x in om {
        if num0(&x["total_liquidacao"])? != 0.0 {
            por.entry(u::pystr(&x["orgao"])).or_default().insert(inteiro(&x["mes"], "mês")?);
        }
    }
    let maior = |s: &BTreeSet<i64>| s.iter().next_back().copied().unwrap_or(0);
    let ultimo = por.values().map(maior).max().ok_or("TCM-GO: nenhum órgão com liquidado em 2026")?;
    let mes = por.values().filter(|s| s.len() as i64 >= ultimo - 1).map(maior).min().ok_or("TCM-GO: nenhum órgão liquidou todo mês em 2026")?;
    let mut mrec = None;
    for x in rec {
        if num0(&x["total_receita"])? != 0.0 {
            mrec = mrec.max(Some(inteiro(&x["mes"], "mês")?));
        }
    }
    Ok(mes.min(mrec.ok_or("TCM-GO: nenhuma receita lançada em 2026")?))
}

/// cod = orgao.unidade.funcao.subfuncao (o script de setores le a subfuncao na 4a parte).
fn chave(x: &Value) -> Result<String, String> {
    let limpa = |v: &Value| Regex::new(r"[.\s]+").expect("regex fixa").replace_all(v.as_str().unwrap_or(""), " ").trim().to_string();
    let (f, s) = (x["funcao"].as_str().unwrap_or(""), x["subfuncao"].as_str().unwrap_or(""));
    match (funcao(f), subfuncao(s)) {
        (Some(cf), Some(cs)) => Ok([&limpa(&x["orgao"]), &limpa(&x["unidade_orcamentaria"]), cf, cs].join(".")),
        _ => Err(format!("função/subfunção nova no TCM-GO, pôr o código no molde: {f} / {s}")),
    }
}

/// Le os 3 anos pelo portal. Devolve os dados no formato da base e o mes de corte de 2026.
pub fn ler(consulta: Consulta, nome: &str) -> Result<(Dados, String), String> {
    let pede = |pag: &str, tabela: &str, colunas: &[&str], somas: &[&str], filtros: &Filtros| -> Result<Vec<Value>, String> {
        let nomes: Vec<&str> = colunas.iter().chain(somas).copied().collect();
        decodifica(&consulta(pag, &comando(tabela, colunas, somas, filtros))?, tabela, &nomes).map_err(|e| format!("Portal do Cidadão do TCM-GO: {e}"))
    };
    let mun = ("municipio", vec![format!("'{}'", nome.replace('\'', "''"))]);
    let ano = |a: &str| ("ano", vec![format!("{a}L")]);
    let om = pede("despesas", "chat_despesa", &["orgao", "mes"], &["total_liquidacao"], &[mun.clone(), ano("2026")])?;
    let rm = pede("receitas", "chat receita", &["mes"], &["total_receita"], &[mun.clone(), ano("2026")])?;
    let mes26 = corte26(&om, &rm)?;
    let mut dados = Dados::new();
    for a in base::ANOS {
        let mut flt = vec![mun.clone(), ano(a)];
        if a == "2026" {
            flt.push(("mes", (1..=mes26).map(|m| format!("{m}L")).collect()));
        }
        let dd = pede("despesas", "chat_despesa", &["tipo_orgao", "orgao", "unidade_orcamentaria", "funcao", "subfuncao"], &["total_empenho", "total_liquidacao"], &flt)?;
        let rr = pede("receitas", "chat receita", &["categoria", "cod_natureza_receita"], &["total_receita"], &flt)?;
        let ll = pede("orcamento", "chat orcamento", &["orgao", "unidade_orcamentaria", "funcao", "subfuncao", "programa", "acao"], &["valor_fonte"], &[mun.clone(), ano(a)])?;
        let mut acoes = Acoes::default();
        let mut nova = |x: &Value| -> Result<usize, String> {
            let k = chave(x)?;
            let (i, nova) = acoes.idx(&k);
            if nova {
                let p: Vec<&str> = k.split('.').collect();
                let y = &mut acoes.lista[i];
                (y.f, y.sub) = (p[2].into(), p[3].into());
                y.acao = format!("{} · {}", u::txt(x, "unidade_orcamentaria")?.trim(), u::txt(x, "subfuncao")?).to_uppercase();
                y.cod = k;
            }
            Ok(i)
        };
        let mut soma: Vec<(usize, f64, f64, f64, bool)> = vec![]; // acao, orcado, empenhado, liquidado, "01 fora da Camara"
        for x in &dd {
            // Camara no video = so a Camara na funcao 01
            soma.push((nova(x)?, 0.0, num0(&x["total_empenho"])?, num0(&x["total_liquidacao"])?, x["funcao"] == "Legislativa" && x["tipo_orgao"] != "Legislativo"));
        }
        for x in &ll {
            soma.push((nova(x)?, num0(&x["valor_fonte"])?, 0.0, 0.0, false));
        }
        for (i, orc, emp, liq, fora) in soma {
            let y = &mut acoes.lista[i];
            (y.orcado, y.empenhado, y.liquidado) = (y.orcado + orc, y.empenhado + emp, y.liquidado + liq);
            if fora {
                y.f = "0".into();
            }
        }
        let mut orig = vec![];
        for x in &rr {
            let c = if x["cod_natureza_receita"].is_null() { String::new() } else { u::pystr(&x["cod_natureza_receita"]) };
            u::soma(&mut orig, &origem(&c), num0(&x["total_receita"])?);
        }
        dados.insert(a.into(), u::ano(acoes.lista, orig, format!("Portal do Cidadão TCM-GO · Despesas e Orçamento {a}"), format!("Portal do Cidadão TCM-GO · Receitas {a}")));
    }
    Ok((dados, format!("{mes26:02}")))
}

/// O que a esteira chama. Molde testado so com Goiania.
pub fn preparar(consulta: Consulta, setores: &dyn Fn(&Path) -> Result<(), String>, numero: &str, nome: &str, pasta: &Path) -> Result<Value, String> {
    if sem_acento(nome).to_uppercase() != "GOIANIA" {
        return Err("molde do GO testado só com Cidade (IBGE 5208707); outra cidade precisa do código IBGE dela".into());
    }
    let (dados, mes26) = ler(consulta, nome)?;
    base::montar(pasta, numero, nome, "GO", IBGE, &tribunal(), &dados, &mes26, &base::Opcoes::default(), setores)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Resposta DSR simples: cada linha com todos os valores.
    fn dsr(n: usize, linhas: &[Value]) -> String {
        let s: Vec<Value> = (0..n).map(|i| json!({ "N": format!("G{i}") })).collect();
        let dm: Vec<Value> = linhas.iter().enumerate().map(|(i, l)| if i == 0 { json!({ "S": s, "C": l }) } else { json!({ "C": l }) }).collect();
        json!({ "results": [{ "result": { "data": { "dsr": { "DS": [{ "PH": [{ "DM0": dm }] }] } } } }] }).to_string()
    }

    fn portal(pag: &str, cmd: &Value) -> Result<String, String> {
        let q = &cmd["SemanticQueryDataShapeCommand"]["Query"];
        let (tabela, ncol) = (q["From"][0]["Entity"].as_str().unwrap(), q["Select"].as_array().unwrap().len());
        let ano = q["Where"][1]["Condition"]["In"]["Values"][0][0]["Literal"]["Value"].as_str().unwrap();
        assert_eq!(q["Where"][0]["Condition"]["In"]["Values"][0][0]["Literal"]["Value"], "'Cidade'");
        Ok(match (pag, tabela, ncol) {
            // Prefeitura liquida todo mes ate agosto; Camara ate julho; fundo pequeno so em marco
            ("despesas", "chat_despesa", 3) => dsr(3, &(1..=8).flat_map(|m| [json!(["PREFEITURA", m, 10.5]), json!(["CAMARA", m, if m <= 7 { 1.0 } else { 0.0 }])]).chain([json!(["FUNDO DO IDOSO", 3, 2])]).collect::<Vec<_>>()),
            ("receitas", "chat receita", 2) => dsr(2, &[json!([9, 5.0]), json!([10, null])]),
            ("despesas", "chat_despesa", 7) => {
                if ano == "2026L" {
                    assert_eq!(q["Where"][2]["Condition"]["In"]["Values"].as_array().unwrap().len(), 7); // meses 1 a 7
                }
                dsr(7, &[json!(["Executivo", "02. SEC. DE CULTURA", "0201 Gabinete", "Cultura", "Difusão Cultural", 300, 250.5]), json!(["Executivo", "02. SEC. DE CULTURA", "0201 Gabinete", "Legislativa", "Ação Legislativa", 5, 5]), json!(["Legislativo", "01 CAMARA", "0101 Camara", "Legislativa", "Ação Legislativa", 80, 70])])
            }
            ("receitas", "chat receita", 3) => dsr(3, &[json!(["Receitas Correntes", 11125000, 900.0]), json!(["Receitas Correntes", "1751", "100.5"]), json!(["Deduções", 9172, -60]), json!(["Intra", 7218, 30]), json!(["?", null, 1])]),
            ("orcamento", "chat orcamento", 7) => dsr(7, &[json!(["02. SEC. DE CULTURA", "0201 Gabinete", "Cultura", "Difusão Cultural", "P1", "A1", 100]), json!(["02. SEC. DE CULTURA", "0201 Gabinete", "Cultura", "Difusão Cultural", "P1", "A2", 50]), json!(["03 SAUDE", "0301 Fundo", "Saúde", "Atenção Básica", "P2", "A3", 400])]),
            x => return Err(format!("consulta inesperada {x:?}")),
        })
    }

    #[test]
    fn decodifica_repeticao_nulo_e_dicionario() {
        let r = json!({ "results": [{ "result": { "data": { "dsr": { "DS": [{ "ValueDicts": { "D0": ["SAUDE", "CULTURA"] }, "PH": [{ "DM0": [
            { "S": [{ "N": "G0", "DN": "D0" }, { "N": "G1" }, { "N": "M0" }], "C": [0, 1, 10.5] },
            { "C": [2, 7], "R": 1 },            // 1a coluna repete a de cima
            { "C": [1], "R": 2, "Ø": 4 },      // 2a repete, 3a nula
        ] }] }] } } } }] })
        .to_string();
        let l = decodifica(&r, "t", &["orgao", "mes", "v"]).unwrap();
        assert_eq!(l, [json!({ "orgao": "SAUDE", "mes": 1, "v": 10.5 }), json!({ "orgao": "SAUDE", "mes": 2, "v": 7 }), json!({ "orgao": "CULTURA", "mes": 2, "v": null })]);
        // resposta cortada, recusada, curta
        assert!(decodifica(&r.replace("\"ValueDicts\"", "\"RT\":[[\"x\"]],\"ValueDicts\""), "chat_despesa", &[]).unwrap_err().contains("passou de 30 mil linhas"));
        assert!(decodifica(r#"{"results":[{"result":{"data":{"descriptor":{},"error":"x"}}}]}"#, "t", &[]).unwrap_err().contains("recusou a consulta t"));
        assert!(decodifica(&r.replace("[0,1,10.5]", "[0,1]"), "t", &[]).unwrap_err().contains("menos valores"));
        assert!(decodifica(&r.replace("[0,1,10.5]", "[5,1,1]"), "t", &[]).unwrap_err().contains("fora do dicionário"));
        assert!(decodifica("Unauthorized", "t", &[]).is_err());
        let c = comando("chat_despesa", &["orgao"], &["total_liquidacao"], &[("ano", vec!["2026L".into()])]);
        let q = &c["SemanticQueryDataShapeCommand"];
        assert!(q["Query"]["Select"][1]["Aggregation"]["Function"] == 0 && q["Query"]["Select"][0]["Name"] == "c0" && q["Binding"]["Primary"]["Groupings"][0]["Projections"] == json!([0, 1]));
        assert_eq!(q["Binding"]["DataReduction"]["Primary"]["Window"]["Count"], 30000);
    }

    #[test]
    fn le_por_orgao_unidade_funcao_e_subfuncao() {
        let (dados, m) = ler(&portal, "Cidade").unwrap();
        assert_eq!(m, "07"); // a Camara parou em julho; o fundo que liquida de vez em quando nao conta; a receita vai ate setembro
        let a = &dados["2026"];
        assert_eq!(a.acoes.len(), 4);
        let x = &a.acoes[0];
        assert_eq!((x.acao.as_str(), x.cod.as_str(), x.f.as_str(), x.sub.as_str()), ("0201 GABINETE · DIFUSÃO CULTURAL", "02 SEC DE CULTURA.0201 Gabinete.13.392", "13", "392"));
        assert_eq!((x.orcado, x.atualizado, x.empenhado, x.liquidado), (150.0, 0.0, 300.0, 250.5));
        assert_eq!((a.acoes[1].f.as_str(), a.acoes[2].f.as_str(), a.acoes[3].orcado, a.acoes[3].liquidado), ("0", "01", 400.0, 0.0)); // 01 fora da Camara vai para "Outros"
        assert_eq!(a.receita, [("Impostos, taxas e contribuições de melhoria".to_string(), 900.0), ("Transferências correntes".to_string(), 100.5), (u::INTRA.to_string(), 30.0), ("Natureza ".to_string(), 1.0), ("Deduções da receita (Fundeb)".to_string(), -60.0)]);
        assert_eq!((a.liquidado, a.empenhado, a.doc_despesa.as_str()), (325.5, 385.0, "Portal do Cidadão TCM-GO · Despesas e Orçamento 2026"));
    }

    #[test]
    fn recusa_o_que_falta() {
        let troca = |de: &'static str, para: &'static str| move |pag: &str, cmd: &Value| portal(pag, cmd).map(|t| t.replace(de, para));
        assert!(ler(&troca("Difusão Cultural", "Economia Criativa"), "Cidade").unwrap_err().contains("função/subfunção nova no TCM-GO, pôr o código no molde: Cultura / Economia Criativa"));
        assert!(ler(&troca("250.5", "\"muito\""), "Cidade").unwrap_err().contains("não é número"));
        assert!(ler(&troca("\"PH\"", "\"RT\":[[1]],\"PH\""), "Cidade").unwrap_err().contains("passou de 30 mil linhas"));
        assert!(corte26(&[], &[]).unwrap_err().contains("nenhum órgão com liquidado"));
        assert!(corte26(&[json!({ "orgao": "A", "mes": 1, "total_liquidacao": 1 })], &[]).unwrap_err().contains("nenhuma receita"));
        assert!(ler(&|_, _| Err("token vencido".into()), "Cidade").is_err());
        let p = std::env::temp_dir().join(format!("braniac-link_tribunais_go-{}", std::process::id()));
        assert!(preparar(&portal, &|_| Ok(()), "1", "Anápolis", &p).unwrap_err().contains("só com Cidade"));
    }
}
