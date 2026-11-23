//! Molde de Santa Catarina (link/tribunais/sc.py), Florianopolis: paineis Qlik do Farol TCE/SC (dados do e-Sfinge): Despesas
//! Municipais (liquidado e empenhado por projeto/atividade, com estornos, ate a data de corte), Receitas Municipais (arrecadado
//! por rubrica) e Remessas (ultimo mes de 2026 que TODAS as unidades entregaram os Registros Contabeis). O tribunal nao publica
//! a dotacao por acao: `sem_previsto`. Pagina que nao fecha com o total de linhas do painel, rubricas que nao fecham com o total
//! da receita, linha fora do formato ou sigla repetida para duas unidades = Err (pendencia, nunca video).
//! Entra como closure: `pagina(painel, dims, medidas, n, topo)` devolve o JSON `{"linhas": [[...]], "total": N}` de UMA pagina
//! do cubo (painel = "remessas" | "despesas" | "receitas"). A aba do navegador e a pausa entre consultas sao de quem chama.
//! Ficou de fora: abrir o painel (JS do Qlik) e o cache em disco. A mais que o Python: a conferencia linhas lidas = total do cubo.
#![allow(dead_code)]
use crate::link_tribunais_base::{self as base, Dados};
use crate::link_tribunais_util::{self as u, ini, pystr, Acoes};
use crate::tempo::Data;
use crate::texto_pt::sem_acento;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const IBGE: i64 = 4205407;
/// Campo [Mun/UF] do painel de despesas.
pub const MUN: &str = "FLORIANOPOLIS , SC , BR";
/// Campo nome_ente/nomeente dos paineis de receita e remessas.
pub const ENTE: &str = "FLORIANÓPOLIS";
/// Florianopolis poe os eventos na acao "APOIO A EVENTOS" (o resto a regra da base pega).
pub const FESTAS_EXTRA: &str = r"\bAPOIO A EVENTOS\b";
pub type Pagina<'a> = &'a dyn Fn(&str, &[&str], &[String], usize, usize) -> Result<String, String>;

pub fn tribunal() -> Value {
    json!({ "sigla": "TCE-SC", "fala": "Tribunal de Contas do Estado", "uf": "SC", "estado": "Santa Catarina", "doc_despesa": "TCE-SC DESPESAS MUNICIPAIS (e-Sfinge)", "doc_receita": "TCE-SC RECEITAS MUNICIPAIS (e-Sfinge)",
        "tribunal_estado": "TCE-SC (Tribunal de Contas do Estado de Santa Catarina)", "site_tribunal": "servicos.tcesc.tc.br/farol_externo",
        "fonte_md": "Tudo do TCE-SC (Tribunal de Contas do Estado de Santa Catarina), dados que cada unidade do município manda ao e-Sfinge, publicados nos painéis do Farol TCE/SC (paineistransparencia.tce.sc.gov.br): Receitas Municipais (arrecadado por natureza da receita, já líquido das deduções do Fundeb, outras deduções e restituições) e Despesas Municipais (empenho a empenho: liquidações e estornos com data). \"Gastou\" = LIQUIDADO menos estornos, Prefeitura, Câmara, autarquias, fundações e fundos. O tribunal não publica a dotação (orçado) por ação: o vídeo fala só o gasto.",
        "setores_md": "Setores = soma do liquidado por projeto/atividade (painel Despesas Municipais do TCE-SC, e-Sfinge), pela função; festas e eventos separados da cultura; segurança pública fora.",
        "pendencias_md": ["O TCE-SC não publica nos painéis do Farol a dotação (orçado inicial e atualizado) por ação: sem \"previa\" no achado."],
        "orgaos": "Prefeitura, Câmara, autarquias (COMCAP, IPREF, IPUF, IGEOF), fundações e fundos municipais",
        "legenda_docs": ["• Receita: TCE-SC, Farol, painel Receitas Municipais (e-Sfinge) de 2024, 2025 e 2026 (líquido do Fundeb)", "• Despesa, setores e festas: TCE-SC, Farol, painel Despesas Municipais (e-Sfinge), liquidado por projeto/atividade", "• servicos.tcesc.tc.br/farol_externo"] })
}

/// Datas do Qlik sao numero de dias desde 30/12/1899 (como no Excel).
pub fn serial(d: Data) -> i64 {
    d.0 - Data::civil(1899, 12, 30).map_or(0, |x| x.0)
}

/// Ultimo dia do mes, como numero do Qlik.
pub fn fim(ano: i64, mes: i64) -> Result<i64, String> {
    Data::civil(ano + i64::from(mes == 12), mes % 12 + 1, 1).map(|d| serial(d) - 1).ok_or_else(|| format!("mês inválido: {mes}/{ano}"))
}

/// Todas as linhas de um cubo, pagina a pagina. Linhas lidas diferentes do total que o painel informa = Err.
pub fn cubo(pagina: Pagina, painel: &str, dims: &[&str], meas: &[String]) -> Result<Vec<Value>, String> {
    let (n, mut top, mut out) = (10000 / (dims.len() + meas.len()), 0, vec![]);
    loop {
        let r = u::json(&pagina(painel, dims, meas, n, top)?, painel)?;
        out.extend(u::lista(&r["linhas"], painel)?.iter().cloned());
        let total = r["total"].as_u64().ok_or_else(|| format!("{painel}: o painel não informou o total de linhas"))? as usize;
        top += n;
        if top >= total {
            return if out.len() == total { Ok(out) } else { Err(format!("{painel}: o painel informou {total} linhas e vieram {} (página cortada ou repetida)", out.len())) };
        }
    }
}

fn mes_do(v: &Value) -> Result<i64, String> {
    v.as_i64().or_else(|| v.as_f64().filter(|f| f.fract() == 0.0).map(|f| f as i64)).or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())).map(|am| am % 100).ok_or_else(|| format!("anomes que não é número: {v}"))
}

/// Ultimo mes de 2026 em que todas as unidades do municipio entregaram os Registros Contabeis ao e-Sfinge.
/// Linhas = [unidade, anomes, descricao, status, contagem].
pub fn corte26(linhas: &[Value]) -> Result<String, String> {
    let mut entregue: BTreeMap<String, BTreeSet<i64>> = BTreeMap::new();
    for x in linhas {
        let x = x.as_array().filter(|x| x.len() == 5).ok_or("painel de remessas do TCE-SC: linha fora do formato")?;
        let conta = match &x[4] {
            Value::Number(n) => n.as_f64() != Some(0.0),
            Value::String(s) => !s.is_empty(),
            _ => false,
        };
        // "Cumpriu o Prazo" / "Descumpriu o Prazo" = entregue; "Em Periodo de Envio" = ainda nao
        if conta && x[2] == "Registros Contábeis" && pystr(&x[3]).contains("Prazo") {
            entregue.entry(pystr(&x[0])).or_default().insert(mes_do(&x[1])?);
        }
    }
    let mut m = 0;
    while !entregue.is_empty() && entregue.values().all(|s| s.contains(&(m + 1))) {
        m += 1;
    }
    if m == 0 {
        return Err("nenhum mês de 2026 entregue por todas as unidades (painel de remessas do TCE-SC)".into());
    }
    Ok(format!("{m:02}"))
}

/// As medidas (liquidado, empenhado) do painel de despesas ate o fim do mes de corte, ja com os estornos.
pub fn medidas_despesa(ano: &str, mes: i64) -> Result<Vec<String>, String> {
    let fim = fim(u::int(ano, "ano")?, mes)?;
    let f = format!("[Mun/UF]={{'{MUN}'}}, Ano={{{ano}}}");
    let ate = |data: &str, valor: &str| format!("Sum({{1<{f}, [{data}]={{\"=Floor([{data}])<={fim}\"}}>}} [{valor}])");
    Ok(vec![
        format!("RangeSum({}, {})", ate("Data Liquidação", "Valor Transação Liquidação"), ate("Data Estorno Liquidação", "Valor Estorno Liquidação")),
        format!("RangeSum({}, {})", ate("Data Empenho", "Valor Empenho"), ate("Data Estorno Empenho", "Valor Estorno")),
    ])
}

pub fn medida_receita(ano: &str, mes: i64) -> Vec<String> {
    vec![format!("Sum({{1<nome_ente={{'{ENTE}'}}, ano_competencia={{{ano}}}, anomes={{\"<={ano}{mes:02}\"}}>}} valorarrecadado)")]
}

fn origem(cod: &Value) -> &'static str {
    let c: String = pystr(cod).chars().filter(char::is_ascii_digit).collect();
    // intraorcamentarias entram dos dois lados; codigo sem digito cai aqui tambem, como no Python (o total da receita confere a soma)
    if c.is_empty() || c.starts_with(['7', '8']) { u::INTRA } else { u::origem(ini(&c, 2)).unwrap_or("Outras receitas") }
}

/// Sigla da unidade gestora para o codigo da acao (o script de setores le a subfuncao na 4a parte do codigo).
pub fn sigla(ug: &str) -> Result<String, String> {
    let re = Regex::new(r"[A-Za-zÀ-ú]{4,}").expect("regex fixa");
    let w: Vec<&str> = re.find_iter(ug).map(|m| m.as_str()).filter(|x| !["florianópolis", "municipal", "público", "públicos"].contains(&x.to_lowercase().as_str())).collect();
    let (ult, antes) = w.split_last().ok_or_else(|| format!("unidade gestora sem nome para a sigla: {ug:?}"))?;
    // ultima palavra com 3 letras: Fundo do Idoso x de Inovacao
    Ok((antes.iter().filter_map(|x| x.chars().next()).collect::<String>() + ini(ult, 3)).to_uppercase())
}

/// Numero do JSON ou 0 (o painel devolve texto, "-", quando a soma nao existe).
fn n(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.0)
}

fn codigo(v: &Value, casas: usize) -> String {
    if v.is_number() { format!("{:0casas$}", n(v) as i64) } else { pystr(v) }
}

/// Le os 3 anos ate o mes de corte de 2026.
pub fn ler(pagina: Pagina, mes26: &str) -> Result<Dados, String> {
    let m26 = u::int(mes26, "mês de corte")?;
    let mut dados = Dados::new();
    for a in base::ANOS {
        let mes = if a == "2026" { m26 } else { 12 };
        let md = medidas_despesa(a, mes)?;
        let (mut acoes, mut siglas): (Acoes, BTreeMap<String, String>) = Default::default();
        for l in cubo(pagina, "despesas", &["nomeUG", "codigoFuncao", "codigoSubFuncao", "Nro. ProjetoAtividade", "Projeto Atividade"], &md)? {
            let l = l.as_array().filter(|l| l.len() == 7).ok_or_else(|| format!("{a}: linha do painel de despesas fora do formato"))?;
            let (liq, emp) = (n(&l[5]), n(&l[6]));
            if liq == 0.0 && emp == 0.0 {
                continue;
            }
            let (ug, f, sub, nro, nome) = (pystr(&l[0]), codigo(&l[1], 2), codigo(&l[2], 3), pystr(&l[3]), pystr(&l[4]));
            let sg = sigla(&ug)?;
            if *siglas.entry(sg.clone()).or_insert_with(|| ug.clone()) != ug {
                return Err(format!("sigla repetida para duas unidades: {sg}"));
            }
            let (i, nova) = acoes.idx(&[ug.as_str(), &f, &sub, &nro, &nome].join("\u{1}"));
            let x = &mut acoes.lista[i];
            if nova {
                // Camara = funcao 01 so da Camara; outra unidade na funcao 01 vai a "Outros"
                x.f = if f == "01" && !ug.to_uppercase().contains("CÂMARA") { "0".into() } else { f.clone() };
                (x.cod, x.sub, x.acao) = (format!("{sg}.0.{f}.{sub}.0.{nro}"), sub, nome.trim().to_string());
            }
            x.liquidado += liq;
            x.empenhado += emp;
        }
        // o numero do projeto/atividade se repete entre acoes diferentes (2024: "6" = eventos e auxilio funeral)
        let mut vistos: BTreeMap<String, usize> = BTreeMap::new();
        for x in &mut acoes.lista {
            let v = vistos.entry(x.cod.clone()).or_default();
            *v += 1;
            if *v > 1 {
                x.cod += &format!("-{v}");
            }
        }
        let tot = cubo(pagina, "despesas", &["Ente"], &md)?;
        let mr = medida_receita(a, mes);
        let mut orig = vec![];
        for l in cubo(pagina, "receitas", &["codigoreceita"], &mr)? {
            let l = l.as_array().filter(|l| l.len() == 2).ok_or_else(|| format!("{a}: linha do painel de receitas fora do formato"))?;
            u::soma(&mut orig, origem(&l[0]), n(&l[1]));
        }
        let tr: f64 = cubo(pagina, "receitas", &["nome_ente"], &mr)?.iter().map(|t| n(&t[1])).sum();
        let mut ano = u::ano(acoes.lista, orig, format!("TCE-SC Farol · Despesas Municipais (e-Sfinge) {a}"), format!("TCE-SC Farol · Receitas Municipais (e-Sfinge) {a}"));
        if (ano.arrecadado - tr).abs() >= 1.0 {
            return Err(format!("{a}: rubricas da receita não fecham com o total"));
        }
        // os totais sao os do painel (a base confere a soma das acoes contra eles)
        (ano.arrecadado, ano.liquidado, ano.empenhado) = (tr, tot.iter().map(|t| n(&t[1])).sum(), tot.iter().map(|t| n(&t[2])).sum());
        dados.insert(a.into(), ano);
    }
    Ok(dados)
}

/// O que a esteira chama. Este molde esta montado para a capital.
pub fn preparar(pagina: Pagina, setores: &dyn Fn(&Path) -> Result<(), String>, numero: &str, nome: &str, pasta: &Path) -> Result<Value, String> {
    if sem_acento(nome).to_uppercase() != "FLORIANOPOLIS" {
        return Err("este molde do TCE-SC está montado para a capital (Florianópolis)".into());
    }
    let rem = cubo(pagina, "remessas", &["nomeunidade", "anomes", "descricao", "statusenvio"], &[format!("Count({{1<nomeente={{'{ENTE}'}}, Ano={{2026}}>}} chaveuganomes)")])?;
    let mes26 = corte26(&rem)?;
    let dados = ler(pagina, &mes26)?;
    base::montar(pasta, numero, nome, "SC", IBGE, &tribunal(), &dados, &mes26, &base::Opcoes { festas_extra: Some(FESTAS_EXTRA), sem_previsto: true, ..Default::default() }, setores)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn despesas() -> Vec<Value> {
        vec![
            json!(["Prefeitura Municipal de Florianópolis", 4, 122, 6, " APOIO A EVENTOS ", 100.5, 120]),
            json!(["Prefeitura Municipal de Florianópolis", 4, 122, 6, "AUXILIO FUNERAL", 10, 10]),
            json!(["Prefeitura Municipal de Florianópolis", 1, 31, 7, "APOIO AO LEGISLATIVO", 1, 1]),
            json!(["Câmara Municipal de Florianópolis", 1, 31, 2001, "PROCESSO LEGISLATIVO", 50, 60]),
            json!(["Fundo Municipal do Idoso", "08", "241", "2.050", "ZERADA", "-", "-"]),
            json!(["Fundo Municipal de Inovação", 19, 572, 9, "INOVACAO", "-", 5]),
        ]
    }

    fn painel(painel: &str, dims: &[&str], meas: &[String], n: usize, top: usize) -> Result<String, String> {
        let linhas = match (painel, dims[0]) {
            ("despesas", "nomeUG") => {
                assert_eq!(n, 10000 / 7);
                assert!(meas[0].starts_with("RangeSum(Sum({1<[Mun/UF]={'FLORIANOPOLIS , SC , BR'}, Ano={20") && meas[1].contains("[Valor Estorno])"));
                despesas()
            }
            ("despesas", "Ente") => vec![json!(["FLORIANOPOLIS", 161.5, 196])],
            ("receitas", "codigoreceita") => vec![json!(["1.1.1.2.50", 1000]), json!([17180000, 400.5]), json!(["7.2.1", 30]), json!(["9.9", -5]), json!(["-", 2])],
            ("receitas", "nome_ente") => vec![json!(["FLORIANÓPOLIS", 1427.5])],
            ("remessas", _) => vec![
                json!(["Prefeitura", 202601, "Registros Contábeis", "Cumpriu o Prazo", 1]),
                json!(["Prefeitura", 202602, "Registros Contábeis", "Descumpriu o Prazo", 1]),
                json!(["Prefeitura", 202603, "Registros Contábeis", "Cumpriu o Prazo", 1]),
                json!(["Câmara", "202601", "Registros Contábeis", "Cumpriu o Prazo", 1]),
                json!(["Câmara", 202602, "Registros Contábeis", "Cumpriu o Prazo", 1]),
                json!(["Câmara", 202603, "Registros Contábeis", "Em Período de Envio", 1]),
                json!(["Câmara", 202603, "Atos de Pessoal", "Cumpriu o Prazo", 1]),
                json!(["Fundo", 202603, "Registros Contábeis", "Cumpriu o Prazo", 0]),
            ],
            x => return Err(format!("consulta inesperada {x:?}")),
        };
        assert_eq!(top, 0);
        Ok(json!({ "linhas": linhas, "total": linhas.len() }).to_string())
    }

    #[test]
    fn datas_do_qlik_e_sigla() {
        assert_eq!((serial(Data::civil(1900, 1, 1).unwrap()), fim(2026, 2).unwrap(), fim(2025, 12).unwrap()), (2, 46081, 46022)); // 28/02/2026 e 31/12/2025
        assert_eq!(sigla("Fundo Municipal do Idoso").unwrap(), "FIDO");
        assert_eq!(sigla("Fundo Municipal de Inovação").unwrap(), "FINO");
        assert_eq!(sigla("Prefeitura Municipal de Florianópolis").unwrap(), "PRE");
        assert!(sigla("FMS").is_err());
        assert_eq!(medida_receita("2026", 7), ["Sum({1<nome_ente={'FLORIANÓPOLIS'}, ano_competencia={2026}, anomes={\"<=202607\"}>} valorarrecadado)"]);
        assert_eq!(medidas_despesa("2026", 2).unwrap()[0], "RangeSum(Sum({1<[Mun/UF]={'FLORIANOPOLIS , SC , BR'}, Ano={2026}, [Data Liquidação]={\"=Floor([Data Liquidação])<=46081\"}>} [Valor Transação Liquidação]), Sum({1<[Mun/UF]={'FLORIANOPOLIS , SC , BR'}, Ano={2026}, [Data Estorno Liquidação]={\"=Floor([Data Estorno Liquidação])<=46081\"}>} [Valor Estorno Liquidação]))");
    }

    #[test]
    fn corte_pelas_remessas_e_leitura_por_projeto_atividade() {
        let rem = cubo(&painel, "remessas", &["nomeunidade", "anomes", "descricao", "statusenvio"], &["Count(x)".into()]).unwrap();
        assert_eq!(corte26(&rem).unwrap(), "02"); // a Camara ainda esta no periodo de envio de marco
        let dados = ler(&painel, "02").unwrap();
        let a = &dados["2026"];
        assert_eq!(a.acoes.len(), 5); // a zerada sai
        let x = &a.acoes[0];
        assert_eq!((x.acao.as_str(), x.cod.as_str(), x.f.as_str(), x.sub.as_str(), x.liquidado, x.empenhado, x.orcado), ("APOIO A EVENTOS", "PRE.0.04.122.0.6", "04", "122", 100.5, 120.0, 0.0));
        assert_eq!(a.acoes[1].cod, "PRE.0.04.122.0.6-2"); // mesmo numero de projeto, outra acao
        assert_eq!((a.acoes[2].f.as_str(), a.acoes[3].f.as_str(), a.acoes[3].cod.as_str(), a.acoes[4].cod.as_str()), ("0", "01", "CÂM.0.01.031.0.2001", "FINO.0.19.572.0.9"));
        assert_eq!(a.receita, [("Impostos, taxas e contribuições de melhoria".to_string(), 1000.0), ("Transferências correntes".to_string(), 400.5), (u::INTRA.to_string(), 32.0), ("Outras receitas".to_string(), -5.0)]);
        assert_eq!((a.arrecadado, a.liquidado, a.empenhado), (1427.5, 161.5, 196.0));
    }

    #[test]
    fn recusa_o_que_falta() {
        // cubo maior que uma pagina: junta as paginas e confere com o total do painel
        let duas = |_: &str, _: &[&str], _: &[String], n: usize, top: usize| Ok(json!({ "linhas": vec![json!(["x", 1]); if top == 0 { n } else { 3 }], "total": n + 3 }).to_string());
        assert_eq!(cubo(&duas, "p", &["d"], &["m".into()]).unwrap().len(), 5003);
        let repete = |_: &str, _: &[&str], _: &[String], n: usize, _: usize| Ok(json!({ "linhas": vec![json!(["x", 1]); n], "total": n + 3 }).to_string());
        assert!(cubo(&repete, "p", &["d"], &["m".into()]).unwrap_err().contains("página cortada ou repetida"));
        let corta = |p: &str, d: &[&str], m: &[String], n: usize, top: usize| painel(p, d, m, n, top).map(|t| if d[0] == "nomeUG" { t.replace("\"total\":6", "\"total\":7") } else { t });
        assert!(ler(&corta, "02").unwrap_err().contains("informou 7 linhas e vieram 6"));
        let troca = |de: &'static str, para: &'static str| move |p: &str, d: &[&str], m: &[String], n: usize, top: usize| painel(p, d, m, n, top).map(|t| t.replace(de, para));
        assert!(ler(&troca("1427.5", "1500"), "02").unwrap_err().contains("2024: rubricas da receita não fecham"));
        assert!(ler(&troca("Fundo Municipal de Inovação", "Prefeitura de Florianópolis Municipal"), "02").unwrap_err().contains("sigla repetida para duas unidades: PRE"));
        assert!(ler(&troca("\"INOVACAO\",", ""), "02").unwrap_err().contains("fora do formato"));
        assert!(corte26(&[json!(["Câmara", 202601, "Registros Contábeis", "Em Período de Envio", 1])]).unwrap_err().contains("nenhum mês de 2026"));
        assert!(corte26(&[json!(["Câmara", "jan", "Registros Contábeis", "Cumpriu o Prazo", 1])]).unwrap_err().contains("anomes"));
        assert!(corte26(&[json!(["Câmara", 202601])]).unwrap_err().contains("fora do formato"));
        assert!(ler(&|_, _, _, _, _| Ok("<html>".into()), "02").is_err());
        let p = std::env::temp_dir().join(format!("braniac-link_tribunais_sc-{}", std::process::id()));
        assert!(preparar(&painel, &|_| Ok(()), "1", "Joinville", &p).unwrap_err().contains("montado para a capital"));
    }
}
