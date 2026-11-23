//! Molde do Parana (link/tribunais/pr.py), Curitiba: Download de Dados do PIT do TCE-PR (SIM-AM), XML dentro de zip.
//! Receita: ReceitasConsolidado.xml (item do Balanco e mes, Realizado ate o mes; as origens tem de fechar com o total 5010 do
//! proprio tribunal). Despesa: Empenho.xml (um empenho por linha) e EmpenhoLiquidacao*.xml (liquidacoes com data). O tribunal nao
//! publica a dotacao por acao (`sem_previsto`) nem anulacao de empenho (empenhado = bruto). 2026 vale ate o ultimo mes que TODAS
//! as entidades entregaram ao SIM-AM e que ja esta na receita. Consorcio intermunicipal sediado na cidade fica fora.
//! XML cortado (sem o fecho da raiz), atributo que falta, valor que nao e numero, origens que nao fecham = Err (pendencia).
//! Entra como closure: `textos(tipo, ano)` devolve o conteudo ja extraido do zip (tipo = "receita": ReceitasConsolidado.xml;
//! "empenho": Empenho.xml; "liquidacao": todos os EmpenhoLiquidacao*.xml). Ficou de fora: baixar e abrir os zips.
//! A mais que o Python: ano sem nenhum arquivo de liquidacao e erro (la o liquidado ficava zero calado).
#![allow(dead_code)]
use crate::link_tribunais_base::{self as base, maiuscula, Ano, Dados};
use crate::link_tribunais_util::{self as u, desescapa, Acoes};
use crate::texto_pt::sem_acento;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const IBGE: i64 = 4106902;
pub const IBGE6: &str = "410690";
/// Origens da receita = os grandes itens do Balanco (sem intra-orcamentarias); 5010 e o total.
pub const ORIGENS: [&str; 13] = ["5030", "5080", "5130", "5220", "5270", "5330", "5360", "5430", "5510", "5540", "5570", "5580", "5660"];
/// Curitiba: "Promocao e apoio a grandes eventos" (Instituto Municipal de Turismo).
pub const FESTAS_EXTRA: &str = "GRANDES EVENTOS";
pub type Textos<'a> = &'a dyn Fn(&str, &str) -> Result<Vec<String>, String>;
pub type Attrs = BTreeMap<String, String>;

pub fn tribunal() -> Value {
    json!({ "sigla": "TCE-PR", "fala": "Tribunal de Contas do Estado", "uf": "PR", "estado": "Paraná", "doc_despesa": "PIT EMPENHOS", "doc_receita": "PIT RECEITAS",
        "tribunal_estado": "TCE-PR (Tribunal de Contas do Estado do Paraná)", "site_tribunal": "pit.tce.pr.gov.br",
        "fonte_md": "Tudo do Portal Informação para Todos (PIT) do TCE-PR, Download de Dados (pit.tce.pr.gov.br/Dados/DadosConsulta/Consulta), dados do SIM-AM: Receita (ReceitasConsolidado, Realizado até o mês, todas as entidades do município, já líquida das deduções do Fundeb) e Despesa (Empenho e EmpenhoLiquidacao, por projeto/atividade). \"Gastou\" = LIQUIDADO. O TCE-PR não publica o orçado por ação nem as anulações de empenho: o empenhado é o BRUTO (vlEmpenho).",
        "setores_md": "Setores = soma do liquidado dos empenhos por projeto/atividade (PIT do TCE-PR), pela função; festas e eventos separados da cultura; segurança pública fora.",
        "pendencias_md": ["O TCE-PR não publica a dotação (orçado inicial/atualizado) por ação: PIT e Download de Dados só têm empenho e liquidação."],
        "orgaos": "Prefeitura, Câmara, autarquias, fundações e fundos (consórcios intermunicipais fora)",
        "legenda_docs": ["• Receita: PIT do TCE-PR, Download de Dados, ReceitasConsolidado (Realizado) de 2024, 2025 e 2026", "• Despesa, setores e festas: PIT do TCE-PR, Download de Dados, empenhos e liquidações por projeto/atividade", "• pit.tce.pr.gov.br"] })
}

/// Leitor minimo de XML: chama `f` com os atributos de cada elemento `<tag .../>` (ou `<tag ...>`). So o que os arquivos do PIT
/// usam: atributos entre aspas e entidades basicas. Arquivo que nao termina no fecho da raiz (cortado) = Err.
pub fn xml(texto: &str, tag: &str, f: &mut dyn FnMut(&Attrs) -> Result<(), String>) -> Result<(), String> {
    let raiz = Regex::new(r"<([A-Za-z_][\w.:-]*)").expect("regex fixa").captures(texto).map(|c| c[1].to_string()).ok_or("XML do tribunal sem elemento raiz")?;
    let fim = texto.trim_end();
    let raiz_sozinha = Regex::new(&format!(r"^(?s)\s*(<\?.*?\?>\s*)?<{}(\s[^>]*)?/>$", regex::escape(&raiz))).expect("regex da raiz").is_match(fim);
    if !raiz_sozinha && !fim.ends_with(&format!("</{raiz}>")) {
        return Err(format!("XML do tribunal cortado: não termina em </{raiz}>"));
    }
    let el = Regex::new(&format!(r#"<{}((?:\s+[\w.:-]+\s*=\s*(?:"[^"]*"|'[^']*'))*)\s*/?>"#, regex::escape(tag))).expect("regex da tag");
    let at = Regex::new(r#"([\w.:-]+)\s*=\s*(?:"([^"]*)"|'([^']*)')"#).expect("regex fixa");
    for e in el.captures_iter(texto) {
        f(&at.captures_iter(&e[1]).map(|a| (a[1].to_string(), desescapa(a.get(2).or(a.get(3)).map_or("", |m| m.as_str())))).collect())?;
    }
    Ok(())
}

fn at<'a>(r: &'a Attrs, k: &str) -> Result<&'a str, String> {
    r.get(k).map(String::as_str).ok_or_else(|| format!("XML do TCE-PR sem o atributo {k}"))
}

fn num(r: &Attrs, k: &str) -> Result<f64, String> {
    at(r, k)?.trim().parse().map_err(|_| format!("atributo {k} com valor que não é número: {:?}", r[k]))
}

/// Mes de uma data "AAAA-MM-DD...".
fn mes_de(r: &Attrs, k: &str) -> Result<i64, String> {
    at(r, k)?.get(5..7).ok_or_else(|| format!("data fora do padrão em {k}: {:?}", r[k])).and_then(|m| u::int(m, k))
}

fn consorcio(r: &Attrs) -> Result<bool, String> {
    // consorcio intermunicipal sediado na cidade nao e o municipio
    Ok(Regex::new("(?i)CONS[ÓO]RCIO").expect("regex fixa").is_match(at(r, "nmEntidade")?))
}

fn primeiro(textos: Textos, tipo: &str, a: &str) -> Result<String, String> {
    textos(tipo, a)?.into_iter().next().ok_or_else(|| format!("TCE-PR: arquivo de {tipo} de {a} não veio no zip"))
}

/// Origens da receita ate o mes (so as diferentes de zero, da maior para a menor) e o total do tribunal.
pub fn receita(texto: &str, a: &str, mes: i64) -> Result<(Vec<(String, f64)>, f64), String> {
    let (mut orig, mut total): (Vec<(String, f64)>, Option<f64>) = (vec![], None);
    xml(texto, "ReceitasConsolidado", &mut |r| {
        if u::int(at(r, "nrMes")?, "nrMes")? == mes {
            let id = at(r, "idSumarioItem")?;
            if ORIGENS.contains(&id) {
                orig.push((maiuscula(at(r, "dsItem")?.trim()), num(r, "vlRealizadoAteOMes")?));
            } else if id == "5010" && total.is_none() {
                total = Some(num(r, "vlRealizadoAteOMes")?);
            }
        }
        Ok(())
    })?;
    let total = total.ok_or_else(|| format!("{a}: receita do TCE-PR sem o total (item 5010) no mês {mes}"))?;
    if (orig.iter().map(|o| o.1).sum::<f64>() - total).abs() >= 1.0 {
        return Err(format!("{a}: origens da receita não fecham com o total do TCE-PR"));
    }
    orig.retain(|o| o.1 != 0.0);
    orig.sort_by(|x, y| y.1.total_cmp(&x.1));
    Ok((orig, total))
}

/// 2026: ultimo mes que TODAS as entidades do municipio entregaram ao SIM-AM e que ja esta na receita consolidada.
pub fn mes26(empenho26: &str, receita26: &str) -> Result<i64, String> {
    let (mut envio, mut rec): (BTreeMap<String, i64>, BTreeSet<i64>) = Default::default();
    xml(empenho26, "Empenho", &mut |r| {
        if !consorcio(r)? {
            let ult = at(r, "ultimoEnvioSIMAMNesteExercicio")?;
            envio.insert(at(r, "idPessoa")?.into(), u::int(ult.split('/').nth(1).ok_or_else(|| format!("último envio ao SIM-AM fora do padrão: {ult:?}"))?, "mês do último envio")?);
        }
        Ok(())
    })?;
    xml(receita26, "ReceitasConsolidado", &mut |r| {
        if num(r, "vlRealizadoAteOMes")? != 0.0 {
            rec.insert(u::int(at(r, "nrMes")?, "nrMes")?);
        }
        Ok(())
    })?;
    match (envio.values().min(), rec.last()) {
        (Some(e), Some(r)) => Ok(*e.min(r)),
        _ => Err("TCE-PR: 2026 sem empenho de entidade do município ou sem receita realizada".into()),
    }
}

/// Le os 3 anos. Devolve os dados no formato da base e o mes de corte de 2026.
pub fn ler(textos: Textos) -> Result<(Dados, String), String> {
    let m26 = mes26(&primeiro(textos, "empenho", "2026")?, &primeiro(textos, "receita", "2026")?)?;
    let mut dados = Dados::new();
    for a in base::ANOS {
        let corte = if a == "2026" { m26 } else { 12 };
        let (mut acoes, mut de): (Acoes, BTreeMap<String, usize>) = Default::default();
        xml(&primeiro(textos, "empenho", a)?, "Empenho", &mut |r| {
            if consorcio(r)? || mes_de(r, "dtEmpenho")? > corte {
                return Ok(());
            }
            let k = [at(r, "idPessoa")?, at(r, "cdOrgao")?, at(r, "cdUnidade")?, at(r, "cdFuncao")?, at(r, "cdSubFuncao")?, at(r, "cdPrograma")?, at(r, "cdProjetoAtividade")?].join(".");
            let (i, nova) = acoes.idx(&k);
            let x = &mut acoes.lista[i];
            if nova {
                (x.f, x.sub, x.cod, x.acao) = (at(r, "cdFuncao")?.into(), at(r, "cdSubFuncao")?.into(), k, at(r, "dsProjetoAtividade")?.trim().into());
            }
            x.empenhado += num(r, "vlEmpenho")?;
            de.insert(at(r, "idEmpenho")?.into(), i);
            Ok(())
        })?;
        // liquidacoes do ano; as de restos a pagar nao casam com empenho do ano e ficam fora
        let liqs = textos("liquidacao", a)?;
        if liqs.is_empty() {
            return Err(format!("TCE-PR: nenhum arquivo de liquidação de {a} no zip"));
        }
        for t in &liqs {
            xml(t, "EmpenhoLiquidacao", &mut |r| {
                if let Some(i) = de.get(at(r, "idEmpenho")?) {
                    if at(r, "dtLiquidacao")?.get(..4) == Some(a) && mes_de(r, "dtLiquidacao")? <= corte {
                        acoes.lista[*i].liquidado += num(r, "vlLiquidacaoLiquido")?;
                    }
                }
                Ok(())
            })?;
        }
        let (orig, arrecadado) = receita(&primeiro(textos, "receita", a)?, a, corte)?;
        dados.insert(a.into(), Ano { arrecadado, liquidado: acoes.lista.iter().map(|x| x.liquidado).sum(), empenhado: acoes.lista.iter().map(|x| x.empenhado).sum(), acoes: acoes.lista, receita: orig,
            doc_despesa: format!("PIT TCE-PR · {a}_{IBGE6}_Despesa.zip"), doc_receita: format!("PIT TCE-PR · {a}_{IBGE6}_Receita.zip") });
    }
    Ok((dados, format!("{m26:02}")))
}

/// O que a esteira chama. Molde testado so com Curitiba.
pub fn preparar(textos: Textos, setores: &dyn Fn(&Path) -> Result<(), String>, numero: &str, nome: &str, pasta: &Path) -> Result<Value, String> {
    if sem_acento(nome).to_uppercase() != "CURITIBA" {
        return Err("molde do PR testado só com Curitiba (IBGE 4106902); outra cidade precisa do código IBGE dela".into());
    }
    let (dados, mes26) = ler(textos)?;
    let op = base::Opcoes { festas_extra: Some(FESTAS_EXTRA), sem_previsto: true, social: Some(("RENDA E CIDADANIA", "renda e cidadania")), ..Default::default() };
    base::montar(pasta, numero, nome, "PR", IBGE, &tribunal(), &dados, &mes26, &op, setores)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn emp(id: u32, ent: &str, pessoa: u32, f: &str, pa: &str, nome: &str, data: &str, v: &str, envio: &str) -> String {
        format!("  <Empenho idEmpenho=\"{id}\" nmEntidade=\"{ent}\" idPessoa=\"{pessoa}\" cdOrgao=\"10\" cdUnidade=\"01\" cdFuncao=\"{f}\" cdSubFuncao=\"392\" cdPrograma=\"0005\" cdProjetoAtividade=\"{pa}\" dsProjetoAtividade=\"{nome}\" dtEmpenho=\"{data}\" vlEmpenho=\"{v}\" ultimoEnvioSIMAMNesteExercicio=\"{envio}\" />\n")
    }

    fn empenhos(a: &str) -> String {
        let (e1, e2) = if a == "2026" { ("2026/9", "2026/8") } else { ("x/12", "x/12") };
        format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<SIM-AM>\n{}{}{}{}{}</SIM-AM>\n",
            emp(1, "MUNICÍPIO DE CURITIBA", 100, "13", "2050", " PROMOÇÃO &amp; APOIO A GRANDES EVENTOS ", &format!("{a}-02-10T00:00:00"), "1000.50", e1),
            emp(2, "MUNICÍPIO DE CURITIBA", 100, "13", "2050", "outro nome", &format!("{a}-03-01"), "500", e1),
            emp(3, "CÂMARA MUNICIPAL DE CURITIBA", 200, "01", "2001", "PROCESSO LEGISLATIVO", &format!("{a}-01-05"), "300", e2),
            emp(4, "Consórcio Intermunicipal de Saúde", 300, "10", "2900", "CONSORCIO", &format!("{a}-01-05"), "9999", "2026/1"),
            emp(5, "MUNICÍPIO DE CURITIBA", 100, "12", "2100", "ESCOLAS", &format!("{a}-11-20"), "70", e1),
        )
    }

    fn liq(id: u32, data: &str, v: &str) -> String {
        format!("<EmpenhoLiquidacao idEmpenho='{id}' dtLiquidacao=\"{data}\" vlLiquidacaoLiquido=\"{v}\"/>")
    }

    fn rec(mes: u32, id: &str, nome: &str, v: &str) -> String {
        format!("<ReceitasConsolidado nrMes=\"{mes}\" idSumarioItem=\"{id}\" dsItem=\"{nome}\" vlRealizadoAteOMes=\"{v}\"/>")
    }

    fn fonte(tipo: &str, a: &str) -> Result<Vec<String>, String> {
        Ok(match tipo {
            "empenho" => vec![empenhos(a)],
            "liquidacao" => vec![
                format!("<L>{}{}{}</L>", liq(1, &format!("{a}-03-15"), "800.25"), liq(1, &format!("{a}-10-01"), "100"), liq(3, &format!("{a}-02-01"), "300")),
                format!("<L>\n{}{}{}{}\n</L>", liq(2, &format!("{a}-04-01"), "50"), liq(2, "2099-01-01", "7777"), liq(999, &format!("{a}-01-01"), "8888"), liq(4, &format!("{a}-01-09"), "9999")),
            ],
            _ => {
                let ate = if a == "2026" { 9 } else { 12 };
                let l: String = (1..=ate).map(|m| format!("{}{}{}{}{}", rec(m, "5010", "RECEITAS (EXCETO INTRA)", &(1500 * m).to_string()), rec(m, "5030", " IMPOSTOS, TAXAS ", &(1000 * m).to_string()), rec(m, "5220", "TRANSFERÊNCIAS CORRENTES", &(500 * m).to_string()), rec(m, "5080", "CONTRIBUIÇÕES", "0"), rec(m, "5031", "IMPOSTOS (detalhe)", "999"))).collect();
                vec![format!("<R>{l}</R>")]
            }
        })
    }

    #[test]
    fn leitor_minimo_de_xml() {
        let mut v = vec![];
        xml("<?xml version='1.0'?>\n<A x=\"1\">\n<B n=\"a &amp; b\" m='2'/><Bc n=\"nao\"/><B\n n=\"&#231;\" ></B><B/></A>\n", "B", &mut |r| {
            v.push(r.clone());
            Ok(())
        })
        .unwrap();
        assert_eq!(v, [Attrs::from([("m".into(), "2".into()), ("n".into(), "a & b".into())]), Attrs::from([("n".into(), "ç".into())]), Attrs::new()]);
        let nada = &mut |_: &Attrs| Ok(());
        assert!(xml("<A><B n=\"1\"/><B n=\"2", "B", nada).unwrap_err().contains("cortado: não termina em </A>")); // download pela metade
        assert!(xml("", "B", nada).unwrap_err().contains("sem elemento raiz"));
        assert!(xml("<?xml version=\"1.0\"?><A/>", "B", nada).is_ok());
        assert!(xml("<A><B n=\"1\"/></A>", "B", &mut |_| Err("para".into())).is_err());
    }

    #[test]
    fn le_empenhos_liquidacoes_e_receita() {
        let (dados, m) = ler(&fonte).unwrap();
        assert_eq!(m, "08"); // a Camara so entregou ate agosto; a receita vai ate setembro; o consorcio nao conta
        let a = &dados["2026"];
        assert_eq!(a.acoes.len(), 2); // consorcio fora; empenho de novembro fora do corte
        let x = &a.acoes[0];
        assert_eq!((x.acao.as_str(), x.cod.as_str(), x.f.as_str(), x.sub.as_str()), ("PROMOÇÃO & APOIO A GRANDES EVENTOS", "100.10.01.13.392.0005.2050", "13", "392"));
        assert_eq!((x.orcado, x.empenhado, x.liquidado), (0.0, 1500.5, 850.25)); // liquidacao de outubro, de outro ano e de empenho que nao e do ano ficam fora
        assert_eq!((a.acoes[1].liquidado, a.liquidado, a.empenhado), (300.0, 1150.25, 1800.5));
        assert_eq!(a.receita, [("Impostos, taxas".to_string(), 8000.0), ("Transferências correntes".to_string(), 4000.0)]);
        assert_eq!((a.arrecadado, a.doc_despesa.as_str()), (12000.0, "PIT TCE-PR · 2026_410690_Despesa.zip"));
        let b = &dados["2025"];
        assert_eq!((b.acoes.len(), b.acoes[0].liquidado, b.arrecadado), (3, 950.25, 18000.0));
    }

    #[test]
    fn recusa_o_que_falta() {
        let troca = |de: &'static str, para: &'static str| move |tipo: &str, a: &str| fonte(tipo, a).map(|v| v.into_iter().map(|t| t.replace(de, para)).collect());
        assert!(ler(&troca("</SIM-AM>", "")).unwrap_err().contains("XML do tribunal cortado"));
        assert!(ler(&troca("vlEmpenho=\"500\"", "vlEmpenho=\"quinhentos\"")).unwrap_err().contains("não é número"));
        assert!(ler(&troca(" cdFuncao=\"12\"", "")).unwrap_err().contains("sem o atributo cdFuncao"));
        assert!(ler(&troca("dtLiquidacao=\"2099-01-01\"", "dataLiq=\"x\"")).unwrap_err().contains("sem o atributo dtLiquidacao"));
        assert!(ler(&troca("idSumarioItem=\"5220\"", "idSumarioItem=\"5221\"")).unwrap_err().contains("origens da receita não fecham"));
        assert!(ler(&troca("idSumarioItem=\"5010\"", "idSumarioItem=\"5011\"")).unwrap_err().contains("sem o total (item 5010)"));
        assert!(ler(&troca("2026/8", "agosto")).unwrap_err().contains("último envio ao SIM-AM fora do padrão"));
        let sem_liq = |tipo: &str, a: &str| if tipo == "liquidacao" { Ok(vec![]) } else { fonte(tipo, a) };
        assert!(ler(&sem_liq).unwrap_err().contains("nenhum arquivo de liquidação de 2024"));
        let sem_rec = |tipo: &str, a: &str| if tipo == "receita" { Ok(vec![]) } else { fonte(tipo, a) };
        assert!(ler(&sem_rec).unwrap_err().contains("arquivo de receita de 2026 não veio no zip"));
        let p = std::env::temp_dir().join(format!("braniac-link_tribunais_pr-{}", std::process::id()));
        assert!(preparar(&fonte, &|_| Ok(()), "1", "Londrina", &p).unwrap_err().contains("só com Curitiba"));
    }
}
