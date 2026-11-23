//! Molde do Amazonas (link/tribunais/am.py), Manaus: API de Dados Abertos do e-Contas do TCE-AM (Prestacao de Contas Mensal de
//! cada unidade gestora) e o Relatorio de Entrega da PCM (mes de corte de 2026 = ultimo mes que TODAS as UGs entregaram).
//! O tribunal nao publica liquidado por acao que feche: cada UG vira UMA linha com o liquidado oficial do balancete e a funcao
//! predominante dela pela dotacao inicial; as acoes de festas ficam em linhas proprias pelo PAGO e saem da linha da UG, para a
//! soma fechar com o liquidado oficial. UG sem dotacao nem pagamento para achar a funcao, ano sem receita ou despesa, relatorio
//! da PCM em outro formato, campo que falta ou soma que nao fecha = Err (pendencia, nunca video).
//! Entram como closure: `pega(rel)` = JSON de uma rota da API ("unidades", "dotacoes/{ug}/{ano}", "pagamentos/{ug}/{ano}/{mes}",
//! "movimentacoesContabeis/{ug}/{ano}/{mes}", "receitasArrecadadas/{ug}/{ano}"), uma por vez; `pcm()` = o CSV exportado pela tela
//! do Relatorio de Entrega (exercicio 2026, ente Manaus). Ficou de fora: o cache em disco e o passo a passo da tela da PCM (JSF).
#![allow(dead_code)]
use crate::link_tribunais_base::{self as base, Acao, Ano, Dados};
use crate::link_tribunais_util::{self as u, ini, pystr};
use crate::texto_pt::sem_acento;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

pub const IBGE: i64 = 1302603;
/// Manaus no e-Contas = idMunicipio 1.
pub const ID_MUN: i64 = 1;
pub const FESTAS_EXTRA: &str = "R[EÉ]VEILLON|BOI MANAUS";
pub type Pega<'a> = &'a dyn Fn(&str) -> Result<String, String>;

pub fn tribunal() -> Value {
    json!({ "sigla": "TCE-AM", "fala": "Tribunal de Contas do Estado", "uf": "AM", "estado": "Amazonas", "doc_despesa": "e-CONTAS DESPESAS", "doc_receita": "e-CONTAS RECEITAS",
        "tribunal_estado": "TCE-AM (Tribunal de Contas do Estado do Amazonas)", "site_tribunal": "tceam.tc.br",
        "fonte_md": "Tudo do TCE-AM (Tribunal de Contas do Estado do Amazonas): menu Cidadão > Consultas > Consulta às receitas e despesas públicas (Espaço do Controle Social do e-Contas), pela API de Dados Abertos do tribunal (econtasapi.tce.am.gov.br), com os dados da Prestação de Contas Mensal de todas as unidades gestoras do município (Prefeitura, Câmara, autarquias, fundações e fundos). Receita = receitas arrecadadas (já líquidas das deduções do Fundeb). \"Gastou\" = LIQUIDADO do balancete de cada unidade gestora. Festas = valor PAGO das ações de festas (o TCE-AM não publica liquidado por ação). \"Previa\" = dotação do Orçamento Inicial.",
        "setores_md": "Setores = liquidado de cada unidade gestora (balancete do e-Contas do TCE-AM) pela função principal dela (a de maior dotação inicial); festas = valor pago das ações de festas, descontado da unidade delas; segurança pública fora.",
        "pendencias_md": ["Festas = valor pago (o TCE-AM não publica liquidado por ação); setores = liquidado de cada unidade pela função principal dela (uma unidade com mais de uma função conta toda na principal).", "O TCE-AM não publica o crédito atualizado por ação (alterações orçamentárias vêm sem valor)."],
        "orgaos": "Prefeitura, Câmara, autarquias, fundações e fundos municipais (todas as unidades gestoras de Manaus no e-Contas)",
        "legenda_docs": ["• Receita: TCE-AM, e-Contas, receitas arrecadadas de 2024, 2025 e 2026 (líquidas do Fundeb)", "• Despesa e festas: TCE-AM, e-Contas, dotações (orçamento inicial), pagamentos por ação e balancete mensal", "• tceam.tc.br > Cidadão > Consultas > Consulta às receitas e despesas públicas"] })
}

/// Mes de corte de 2026 pelo Relatorio de Entrega da PCM (CSV: UG nas 2 primeiras colunas, depois JAN..DEZ): ultimo mes em que
/// TODAS as UGs estao "ENTREGUE" ou "FORA DO PRAZO". O balancete da API as vezes ja tem o mes seguinte em poucas UGs.
pub fn corte_pcm(csv: &str) -> Result<String, String> {
    let linhas = u::linhas_csv(csv, ',');
    let meses: Vec<String> = base::ABR.iter().map(|m| m.to_uppercase()).collect();
    if linhas.len() < 2 || linhas[0].get(2..14) != Some(&meses[..]) {
        return Err("TCE-AM: Relatório de Entrega da PCM de Manaus vazio ou em outro formato".into());
    }
    let mut corte = 0;
    'mes: while corte < 12 {
        for l in &linhas[1..] {
            if !["ENTREGUE", "FORA DO PRAZO"].contains(&u::cel(l, 2 + corte)?.trim().to_uppercase().as_str()) {
                break 'mes;
            }
        }
        corte += 1;
    }
    if corte == 0 {
        return Err("TCE-AM: nem janeiro/2026 foi entregue por todas as UGs de Manaus (Relatório de Entrega da PCM)".into());
    }
    Ok(format!("{corte:02}"))
}

fn inteiro(v: &Value, k: &str) -> Result<i64, String> {
    let x = &v[k];
    x.as_i64().or_else(|| x.as_str().and_then(|s| s.trim().parse().ok())).ok_or_else(|| format!("TCE-AM: campo {k} que não é número: {x}"))
}

/// Saldo (credito - debito) das contas que comecam por um dos prefixos.
fn saldo(movs: &[Value], prefixos: &[&str]) -> Result<f64, String> {
    let mut s = 0.0;
    for m in movs {
        if prefixos.iter().any(|p| u::txt(m, "codContaContabil").is_ok_and(|c| c.starts_with(p))) {
            s += u::num(m, "vlCredito")? - u::num(m, "vlDebito")?;
        } else {
            u::txt(m, "codContaContabil")?;
        }
    }
    Ok(s)
}

/// As acoes de UMA unidade gestora, na ordem em que apareceram.
#[derive(Default)]
struct DaUg {
    lista: Vec<Acao>,
    onde: BTreeMap<String, usize>,
    /// Pagamento sem codigo de programa/acao, so com o nome: acha o codigo na dotacao da mesma UO.
    nomes: BTreeMap<String, (i64, i64)>,
}

impl DaUg {
    fn acao(&mut self, ug: &str, x: &Value) -> Result<&mut Acao, String> {
        let (uo, f, sub) = (pystr(&x["codUnidadeOrcamentaria"]), inteiro(x, "codFuncaoGoverno")?, inteiro(x, "codSubfuncaoGoverno")?);
        if x["codUnidadeOrcamentaria"].is_null() {
            return Err("TCE-AM: lançamento sem unidade orçamentária".into());
        }
        let nm = x["desAcaoGoverno"].as_str().unwrap_or("").trim().to_uppercase();
        let b = format!("{uo}|{f}|{sub}");
        let pa = if x["codProgramaGoverno"].is_null() || x["codAcaoGoverno"].is_null() {
            self.nomes.get(&format!("{b}|{nm}")).copied().unwrap_or((0, 0))
        } else {
            *self.nomes.entry(format!("{b}|{nm}")).or_insert((inteiro(x, "codProgramaGoverno")?, inteiro(x, "codAcaoGoverno")?))
        };
        let k = format!("{b}|{}|{}|{}", pa.0, pa.1, if pa == (0, 0) { nm.as_str() } else { "" });
        let n = self.lista.len();
        let i = *self.onde.entry(k).or_insert(n);
        if i == n {
            self.lista.push(Acao::default());
        }
        let y = &mut self.lista[i];
        (y.f, y.sub, y.acao) = (f.to_string(), format!("{sub:03}"), x["desAcaoGoverno"].as_str().unwrap_or("").to_string());
        y.cod = format!("{ug}.{uo}.{f:02}.{sub:03}.{:04}.{:04}", pa.0, pa.1);
        Ok(y)
    }
}

/// O primeiro maior (como o max() do Python sobre um dict na ordem de entrada).
fn maior(por: &[(String, f64)]) -> Option<&str> {
    por.iter().fold(None, |m: Option<&(String, f64)>, x| if m.is_none_or(|m| x.1 > m.1) { Some(x) } else { m }).map(|x| x.0.as_str())
}

/// Le os 3 anos. Devolve os dados, o mes de corte de 2026 e os avisos (UGs com funcao principal abaixo de 60% da dotacao).
pub fn ler(pega: Pega, pcm: &dyn Fn() -> Result<String, String>) -> Result<(Dados, String, Vec<String>), String> {
    let lista = |rel: &str| -> Result<Vec<Value>, String> { u::json(&pega(rel)?, rel).and_then(|v| u::lista(&v, rel).cloned()) };
    let mut ugs: Vec<(String, String, String)> = vec![]; // id, sigla, nome
    for x in lista("unidades")? {
        if x["idMunicipio"].as_i64() == Some(ID_MUN) && x["esfera"].as_i64() == Some(2) {
            if x["id_unidade_gestora"].is_null() {
                return Err("TCE-AM: unidade gestora sem id".into());
            }
            let nome = u::txt(&x, "nome")?;
            let sg = x["sigla"].as_str().unwrap_or("").trim();
            ugs.push((pystr(&x["id_unidade_gestora"]), if sg.is_empty() { ini(nome, 20).to_string() } else { sg.to_string() }, nome.trim().to_uppercase()));
        }
    }
    if ugs.is_empty() {
        return Err("TCE-AM: nenhuma unidade gestora municipal de Manaus na lista de unidades".into());
    }
    let mes26 = corte_pcm(&pcm()?)?;
    let m26 = u::int(&mes26, "mês de corte")?;
    let extra = Regex::new(&format!("(?i){FESTAS_EXTRA}")).expect("regex fixa");
    let (mut dados, mut ambiguas) = (Dados::new(), vec![]);
    for a in base::ANOS {
        let ano_n = u::int(a, "ano")?;
        let (mut acoes, mut liq_tot, mut emp_tot) = (vec![], 0.0, 0.0);
        for (ug, sig, nome_ug) in &ugs {
            let dot = lista(&format!("dotacoes/{ug}/{a}"))?;
            if dot.is_empty() {
                continue;
            }
            let mut minhas = DaUg::default();
            for x in &dot {
                let v = u::num(x, "vlDotacao")?;
                let y = minhas.acao(ug, x)?;
                if x["codOrigemDotacaoOrcamentaria"].as_i64() == Some(1) {
                    y.orcado += v;
                }
                y.atualizado += v;
            }
            let mut movs = vec![];
            for m in 1..=if a == "2026" { m26 } else { 12 } {
                for p in lista(&format!("pagamentos/{ug}/{a}/{m}"))? {
                    // restos a pagar de anos anteriores ficam fora (ja contaram no ano deles)
                    if inteiro(&p, "exercicio")? == ano_n {
                        minhas.acao(ug, &p)?.liquidado += u::num(&p, "vlPagamentoEmpenho")?;
                    }
                }
                movs.extend(lista(&format!("movimentacoesContabeis/{ug}/{a}/{m}"))?);
            }
            let liq = saldo(&movs, &["6221303", "6221304", "6221307"])?;
            emp_tot += saldo(&movs, &["62213"])?;
            liq_tot += liq;
            let (fest, resto): (Vec<Acao>, Vec<Acao>) = minhas.lista.into_iter().partition(|y| base::eh_festa(&y.acao, Some(&extra)) && (y.orcado != 0.0 || y.liquidado != 0.0));
            let mut por_f: Vec<(String, f64)> = vec![];
            resto.iter().for_each(|y| u::soma(&mut por_f, &y.f, y.orcado));
            if !por_f.iter().any(|x| x.1 != 0.0) {
                // UG sem dotacao inicial fora das festas: vale o que ela pagou
                resto.iter().for_each(|y| u::soma(&mut por_f, &y.f, y.liquidado));
            }
            let f = maior(&por_f).ok_or_else(|| format!("TCE-AM: {a} UG {sig} sem dotação nem pagamento para achar a função"))?.to_string();
            let (tot, do_f): (f64, f64) = (por_f.iter().map(|x| x.1).sum(), por_f.iter().find(|x| x.0 == f).map_or(0.0, |x| x.1));
            if tot != 0.0 && do_f / tot < 0.6 {
                ambiguas.push(format!("{a} {sig}: função {f} = {:.0}% da dotação", 100.0 * do_f / tot));
            }
            let mut por_s: Vec<(String, f64)> = vec![];
            resto.iter().filter(|y| y.f == f).for_each(|y| u::soma(&mut por_s, &y.sub, if y.orcado != 0.0 { y.orcado } else { y.liquidado }));
            let s = maior(&por_s).ok_or_else(|| format!("TCE-AM: {a} UG {sig} sem subfunção na função principal"))?.to_string();
            let soma = |l: &[Acao], c: fn(&Acao) -> f64| l.iter().map(c).sum::<f64>();
            acoes.push(Acao { cod: format!("{ug}.0.{:02}.{s}.0000.0000", u::int(&f, "função")?), f, sub: s, acao: format!("{nome_ug} (liquidado da unidade gestora)"), orcado: soma(&resto, |y| y.orcado), atualizado: soma(&resto, |y| y.atualizado), empenhado: 0.0, liquidado: liq - soma(&fest, |y| y.liquidado) });
            acoes.extend(fest);
        }
        let mut rec = vec![];
        for (ug, _, _) in &ugs {
            for r in lista(&format!("receitasArrecadadas/{ug}/{a}"))? {
                if a == "2026" && inteiro(&r, "codCompetencia")? % 100 > m26 {
                    continue;
                }
                let c = pystr(&r["codItemReceita"]);
                if r["codItemReceita"].is_null() || c.is_empty() {
                    return Err("TCE-AM: receita sem código do item".into());
                }
                u::soma(&mut rec, if c.starts_with(['7', '8']) { u::INTRA } else { u::origem(ini(&c, 2)).unwrap_or("Outras receitas") }, u::num(&r, "vlLancado")?);
            }
        }
        if rec.is_empty() || acoes.is_empty() {
            return Err(format!("TCE-AM: {a} sem receita ou sem despesa publicada para Manaus"));
        }
        if (acoes.iter().map(|x| x.liquidado).sum::<f64>() - liq_tot).abs() >= 1.0 {
            return Err(format!("TCE-AM: {a}: soma das linhas diferente do liquidado dos balancetes"));
        }
        let ano = u::ano(acoes, rec, format!("TCE-AM e-Contas · dotações, pagamentos e balancetes {a}"), format!("TCE-AM e-Contas · receitas arrecadadas {a}"));
        dados.insert(a.into(), Ano { liquidado: liq_tot, empenhado: emp_tot, ..ano });
    }
    Ok((dados, mes26, ambiguas))
}

/// O que a esteira chama. O molde foi feito e conferido para Manaus.
pub fn preparar(pega: Pega, pcm: &dyn Fn() -> Result<String, String>, setores: &dyn Fn(&Path) -> Result<(), String>, numero: &str, nome: &str, pasta: &Path) -> Result<Value, String> {
    if sem_acento(nome).to_uppercase() != "MANAUS" {
        return Err("o molde do TCE-AM foi feito e conferido para Manaus (idMunicipio 1 no e-Contas); outra cidade precisa do id dela".into());
    }
    let (dados, mes26, _) = ler(pega, pcm)?;
    base::montar(pasta, numero, nome, "AM", IBGE, &tribunal(), &dados, &mes26, &base::Opcoes { festas_extra: Some(FESTAS_EXTRA), ..Default::default() }, setores)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm() -> Result<String, String> {
        Ok("\u{feff}UG,Nome,JAN,FEV,MAR,ABR,MAI,JUN,JUL,AGO,SET,OUT,NOV,DEZ\n10,SEMEF,Entregue,ENTREGUE,Fora do prazo,PENDENTE,,,,,,,,\n20,\"CAMARA, MANAUS\",ENTREGUE,ENTREGUE,ENTREGUE,ENTREGUE,,,,,,,,\n".into())
    }

    fn api(rel: &str) -> Result<String, String> {
        let p: Vec<&str> = rel.split('/').collect();
        Ok(match (p[0], p.get(1).copied().unwrap_or(""), p.get(3).copied().unwrap_or("")) {
            ("unidades", ..) => json!([
                { "id_unidade_gestora": 10, "idMunicipio": 1, "esfera": 2, "sigla": " MANAUSCULT ", "nome": "Fundação de Cultura " },
                { "id_unidade_gestora": 20, "idMunicipio": 1, "esfera": 2, "sigla": null, "nome": "Câmara Municipal de Manaus e seus anexos" },
                { "id_unidade_gestora": 30, "idMunicipio": 1, "esfera": 2, "sigla": "FUNDO", "nome": "Fundo sem dotação" },
                { "id_unidade_gestora": 40, "idMunicipio": 1, "esfera": 1, "sigla": "SEFAZ", "nome": "Estado" },
                { "id_unidade_gestora": 50, "idMunicipio": 2, "esfera": 2, "sigla": "X", "nome": "Outro município" }
            ]),
            ("dotacoes", "10", _) => json!([
                { "codUnidadeOrcamentaria": "1001", "codFuncaoGoverno": 13, "codSubfuncaoGoverno": 392, "codProgramaGoverno": 7, "codAcaoGoverno": 2050, "desAcaoGoverno": " Boi Manaus ", "codOrigemDotacaoOrcamentaria": 1, "vlDotacao": 1000.0 },
                { "codUnidadeOrcamentaria": "1001", "codFuncaoGoverno": 13, "codSubfuncaoGoverno": 392, "codProgramaGoverno": 7, "codAcaoGoverno": 2050, "desAcaoGoverno": " Boi Manaus ", "codOrigemDotacaoOrcamentaria": 2, "vlDotacao": 200.0 },
                { "codUnidadeOrcamentaria": "1001", "codFuncaoGoverno": "13", "codSubfuncaoGoverno": 391, "codProgramaGoverno": 7, "codAcaoGoverno": 2051, "desAcaoGoverno": "Patrimônio", "codOrigemDotacaoOrcamentaria": 1, "vlDotacao": 500.0 },
                { "codUnidadeOrcamentaria": "1001", "codFuncaoGoverno": 4, "codSubfuncaoGoverno": 122, "codProgramaGoverno": 1, "codAcaoGoverno": 2001, "desAcaoGoverno": "Administração", "codOrigemDotacaoOrcamentaria": 1, "vlDotacao": 400.0 }
            ]),
            ("dotacoes", "20", _) => json!([{ "codUnidadeOrcamentaria": "2001", "codFuncaoGoverno": 1, "codSubfuncaoGoverno": 31, "codProgramaGoverno": 2, "codAcaoGoverno": 2100, "desAcaoGoverno": "Processo legislativo", "codOrigemDotacaoOrcamentaria": 1, "vlDotacao": 900.0 }]),
            ("dotacoes", ..) => json!([]),
            ("pagamentos", "10", "1") => json!([
                { "exercicio": 2026, "codUnidadeOrcamentaria": "1001", "codFuncaoGoverno": 13, "codSubfuncaoGoverno": 392, "codProgramaGoverno": null, "codAcaoGoverno": null, "desAcaoGoverno": "BOI MANAUS", "vlPagamentoEmpenho": 300.5 },
                { "exercicio": 2023, "codUnidadeOrcamentaria": "1001", "codFuncaoGoverno": 13, "codSubfuncaoGoverno": 392, "codProgramaGoverno": 7, "codAcaoGoverno": 2050, "desAcaoGoverno": "BOI MANAUS", "vlPagamentoEmpenho": 9999.0 },
                { "exercicio": p[2].parse::<i64>().unwrap(), "codUnidadeOrcamentaria": "1001", "codFuncaoGoverno": 4, "codSubfuncaoGoverno": 122, "codProgramaGoverno": 1, "codAcaoGoverno": 2001, "desAcaoGoverno": "Administração", "vlPagamentoEmpenho": 50.0 }
            ]),
            ("pagamentos", ..) => json!([]),
            ("movimentacoesContabeis", ug, _) => json!([
                { "codContaContabil": "622130300", "vlCredito": if ug == "10" { 100.0 } else { 10.0 }, "vlDebito": 0.0 },
                { "codContaContabil": "622130400", "vlCredito": 30.0, "vlDebito": 5.0 },
                { "codContaContabil": "622130100", "vlCredito": 7.0, "vlDebito": 0.0 },
                { "codContaContabil": "621200000", "vlCredito": 9999.0, "vlDebito": 0.0 }
            ]),
            ("receitasArrecadadas", "10", _) => json!([{ "codCompetencia": 202601, "codItemReceita": 11125000, "vlLancado": 800.0 }, { "codCompetencia": 202603, "codItemReceita": "72180000", "vlLancado": 40.0 }, { "codCompetencia": 202604, "codItemReceita": 11125000, "vlLancado": 77.0 }]),
            ("receitasArrecadadas", "30", _) => json!([{ "codCompetencia": 202602, "codItemReceita": 99990000, "vlLancado": -5.5 }]),
            ("receitasArrecadadas", ..) => json!([]),
            _ => return Err(format!("rota inesperada {rel}")),
        }
        .to_string())
    }

    #[test]
    fn corte_pelo_relatorio_de_entrega() {
        assert_eq!(corte_pcm(&pcm().unwrap()).unwrap(), "03"); // abril esta pendente na SEMEF
        assert!(corte_pcm("UG,Nome,JAN\n1,x,ENTREGUE").unwrap_err().contains("outro formato"));
        assert!(corte_pcm("").unwrap_err().contains("vazio"));
        assert!(corte_pcm(&pcm().unwrap().replace("10,SEMEF,Entregue", "10,SEMEF,Em análise")).unwrap_err().contains("nem janeiro/2026"));
        assert!(corte_pcm(&pcm().unwrap().replace("ENTREGUE,ENTREGUE,ENTREGUE,ENTREGUE,,,,,,,,", "ENTREGUE,ENTREGUE")).unwrap_err().contains("colunas")); // linha cortada
    }

    #[test]
    fn uma_linha_por_unidade_com_o_liquidado_do_balancete_e_festas_pelo_pago() {
        let (dados, m, avisos) = ler(&api, &pcm).unwrap();
        assert_eq!(m, "03");
        let a = &dados["2026"];
        assert_eq!(a.acoes.len(), 3); // Fundacao (linha da UG + festa) e Camara; o fundo sem dotacao nao entra
        let x = &a.acoes[0];
        // balancete: 3 meses x (100 + 25) = 375 liquidado; festa paga 300.5 sai da linha da UG; funcao principal 13 (500 de 900 fora das festas)
        assert_eq!((x.acao.as_str(), x.cod.as_str(), x.f.as_str(), x.sub.as_str()), ("FUNDAÇÃO DE CULTURA (liquidado da unidade gestora)", "10.0.13.391.0000.0000", "13", "391"));
        assert_eq!((x.orcado, x.atualizado, x.empenhado, x.liquidado), (900.0, 900.0, 0.0, 375.0 - 300.5));
        let f = &a.acoes[1];
        assert_eq!((f.acao.as_str(), f.cod.as_str(), f.orcado, f.atualizado, f.liquidado), ("BOI MANAUS", "10.1001.13.392.0007.2050", 1000.0, 1200.0, 300.5)); // pagamento sem codigo achou a acao pelo nome
        assert_eq!((a.acoes[2].f.as_str(), a.acoes[2].cod.as_str(), a.acoes[2].liquidado), ("1", "20.0.01.031.0000.0000", 3.0 * 35.0));
        assert_eq!((a.liquidado, a.empenhado), (480.0, 3.0 * (132.0 + 42.0)));
        assert_eq!(a.receita, [("Impostos, taxas e contribuições de melhoria".to_string(), 800.0), (u::INTRA.to_string(), 40.0), ("Outras receitas".to_string(), -5.5)]); // abril fica fora
        assert_eq!(avisos.iter().filter(|x| x.starts_with("2026")).collect::<Vec<_>>(), ["2026 MANAUSCULT: função 13 = 56% da dotação"]);
        assert_eq!(dados["2025"].acoes[0].liquidado, 12.0 * 125.0); // restos a pagar de 2023 e pagamento de outro exercicio ficam fora
    }

    #[test]
    fn recusa_o_que_falta() {
        let troca = |de: &'static str, para: &'static str| move |rel: &str| api(rel).map(|t| t.replace(de, para));
        assert!(ler(&troca("\"idMunicipio\":1", "\"idMunicipio\":3"), &pcm).unwrap_err().contains("nenhuma unidade gestora municipal de Manaus"));
        assert!(ler(&troca("\"vlDebito\":5.0", "\"vlDebito\":null"), &pcm).unwrap_err().contains("vlDebito"));
        assert!(ler(&troca("\"vlLancado\":800.0", "\"valor\":800.0"), &pcm).unwrap_err().contains("vlLancado"));
        assert!(ler(&troca("\"codFuncaoGoverno\":4", "\"codFuncaoGoverno\":\"adm\""), &pcm).unwrap_err().contains("codFuncaoGoverno"));
        // unidade so com a acao de festas: nao da para achar a funcao dela
        let so_festa = |rel: &str| if rel.starts_with("dotacoes/20/") { Ok(json!([{ "codUnidadeOrcamentaria": "2001", "codFuncaoGoverno": 13, "codSubfuncaoGoverno": 392, "codProgramaGoverno": 2, "codAcaoGoverno": 1, "desAcaoGoverno": "Carnaval", "codOrigemDotacaoOrcamentaria": 1, "vlDotacao": 9.0 }]).to_string()) } else { api(rel) };
        assert!(ler(&so_festa, &pcm).unwrap_err().contains("2024 UG Câmara Municipal de  sem dotação nem pagamento para achar a função"));
        let sem_receita = |rel: &str| if rel.starts_with("receitasArrecadadas") { Ok("[]".to_string()) } else { api(rel) };
        assert!(ler(&sem_receita, &pcm).unwrap_err().contains("2024 sem receita ou sem despesa"));
        assert!(ler(&api, &|| Err("a tela da PCM mudou".into())).is_err() && ler(&|_| Ok("{}".into()), &pcm).unwrap_err().contains("não devolveu uma lista"));
        let p = std::env::temp_dir().join(format!("braniac-link_tribunais_am-{}", std::process::id()));
        assert!(preparar(&api, &pcm, &|_| Ok(()), "1", "Parintins", &p).unwrap_err().contains("conferido para Manaus"));
    }
}
