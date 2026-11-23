//! Molde de Mato Grosso do Sul (link/tribunais/ms.py): paineis Qlik do TCE-MS. 2024 vem do SICOM (empenhos do exercicio por
//! projeto/atividade no "Detalhe Empenho"; nome da acao e receita ACUMULADA por rubrica, vale o ultimo mes); 2025 e 2026 do
//! e-Sfinge (execucao com data, dotacao inicial e atualizada por acao, nomes no PPA, receita por natureza). 2026 vale ate o
//! menor entre o ultimo mes de receita das unidades que mandam todo mes e o bimestre que todas as unidades com liquidacao ja
//! entregaram. Falta no tribunal: dotacao por acao de 2024 (orcado 0) e nome de acoes criadas fora do PPA.
//! Codigo sem correspondencia (funcao, subfuncao, rubrica), linha fora do formato ou valor que nao e numero = Err (pendencia).
//! Entra como closure: `bruto(nome)` devolve o JSON das linhas de um cubo ja consultado (dimensoes como texto, medidas como
//! numero): "rec_esf", "exec2025", "exec2026", "dot2025", "dot2026", "ppa2022", "ppa2022b", "ppa2026", "des2024", "nomes2024",
//! "funcoes" ({"f": [...], "s": [...]}) e "rec2024". O websocket do Qlik, a paginacao e a pausa sao de quem chama.
//! Ficou de fora: as definicoes das consultas (dimensoes e medidas de cada cubo, em `baixar` do Python), o cache de 2 dias e a
//! lista do IBGE (o codigo da cidade entra como parametro).
#![allow(dead_code)]
use crate::link_tribunais_base::{self as base, Acao, Dados};
use crate::link_tribunais_util::{self as u, ini, Acoes};
use crate::texto_pt::sem_acento;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const IBGE_CAMPO_GRANDE: i64 = 5002704;
pub type Bruto<'a> = &'a dyn Fn(&str) -> Result<String, String>;

pub fn tribunal() -> Value {
    json!({ "sigla": "TCE-MS", "fala": "Tribunal de Contas do Estado", "uf": "MS", "estado": "Mato Grosso do Sul", "doc_despesa": "E-SFINGE / SICOM DESPESA", "doc_receita": "E-SFINGE / SICOM RECEITA",
        "tribunal_estado": "TCE-MS (Tribunal de Contas do Estado de Mato Grosso do Sul)", "site_tribunal": "painel.tce.ms.gov.br",
        "fonte_md": "Tudo dos painéis do TCE-MS (Tribunal de Contas do Estado de Mato Grosso do Sul) em painel.tce.ms.gov.br: 2025 e 2026 do e-Sfinge (execução por empenho, liquidação e dotação por projeto/atividade; receita arrecadada por natureza, deduções do Fundeb descontadas); 2024 do SICOM (retrato acumulado por empenho e por rubrica no último mês). \"Gastou\" = LIQUIDADO menos estornos, todos os órgãos do município. \"Previa\" = dotação inicial da LOA.",
        "setores_md": "Setores = soma do liquidado por projeto/atividade (e-Sfinge e SICOM do TCE-MS), pela função; festas e eventos separados da cultura; segurança pública fora.",
        "pendencias_md": ["O TCE-MS não publica a dotação por ação de 2024 (SICOM): o orçado de 2024 fica 0.", "Ações de 2025 criadas fora do PPA não têm nome nos painéis do TCE-MS."],
        "orgaos": "Prefeitura, Câmara, autarquias, fundações e fundos (todas as unidades gestoras do município)",
        "legenda_docs": ["• Receita: TCE-MS, e-Sfinge (2025 e 2026) e SICOM (2024), arrecadado por natureza", "• Despesa, setores e festas: TCE-MS, e-Sfinge e SICOM, liquidado por projeto/atividade", "• painel.tce.ms.gov.br"] })
}

fn sem(t: &str) -> String {
    sem_acento(t).to_uppercase().trim().to_string()
}

fn erro(c: &str) -> String {
    format!("TCE-MS: código sem correspondência no tribunal: {c}")
}

/// As linhas de um cubo, cada uma com exatamente `n` colunas.
fn linhas(v: &Value, nome: &str, n: usize) -> Result<Vec<Vec<Value>>, String> {
    u::lista(v, nome)?.iter().map(|l| l.as_array().filter(|l| l.len() == n).cloned().ok_or_else(|| format!("TCE-MS: linha do cubo {nome} fora do formato"))).collect()
}

fn cubo(bruto: Bruto, nome: &str, n: usize) -> Result<Vec<Vec<Value>>, String> {
    linhas(&u::json(&bruto(nome)?, nome)?, nome, n)
}

/// Dimensao (texto) da linha.
fn t(l: &[Value], i: usize) -> Result<&str, String> {
    l[i].as_str().ok_or_else(|| format!("TCE-MS: dimensão que não é texto: {}", l[i]))
}

/// Medida (numero) da linha.
fn n(l: &[Value], i: usize) -> Result<f64, String> {
    l[i].as_f64().ok_or_else(|| format!("TCE-MS: medida que não é número: {}", l[i]))
}

/// Mes de um "AAAAMM".
fn mes_de(am: &str) -> Result<i64, String> {
    u::int(am.get(4..).unwrap_or(""), "anomes")
}

fn antes_do_traco(s: &str) -> &str {
    s.split('-').next().unwrap_or("").trim()
}

/// Ultimo mes de 2026 que TODAS as unidades entregaram: na receita, o menor "ultimo mes" das unidades que mandam receita todo
/// mes; na despesa, o bimestre (competencia do e-Sfinge) que todas as unidades com liquidacao em 2026 ja entregaram.
pub fn mes26(bruto: Bruto) -> Result<String, String> {
    let mut rec: BTreeMap<String, BTreeSet<i64>> = BTreeMap::new();
    for l in cubo(bruto, "rec_esf", 6)? {
        if t(&l, 0)? == "2026" {
            rec.entry(t(&l, 2)?.into()).or_default().insert(mes_de(t(&l, 1)?)?);
        }
    }
    let m_rec = rec.values().filter(|ms| ms.len() >= 6).filter_map(|ms| ms.last().copied()).min().ok_or("TCE-MS: nenhuma unidade com receita em 6 meses de 2026")?;
    let (mut bim, mut liq): (BTreeMap<String, i64>, BTreeSet<String>) = Default::default();
    for l in cubo(bruto, "exec2026", 9)? {
        let (ug, comp) = (t(&l, 0)?, t(&l, 7)?);
        if ini(comp, 4) == "2026" {
            // qualquer movimento do bimestre = remessa entregue
            let b = bim.entry(ug.into()).or_default();
            *b = (*b).max(mes_de(comp)?);
        }
        if t(&l, 5)? == "Liquidacao" && n(&l, 8)? != 0.0 {
            liq.insert(ug.into()); // unidade sem liquidacao no ano nao muda numero nenhum
        }
    }
    let m_des = 2 * liq.iter().map(|ug| bim.get(ug).copied().unwrap_or(0)).min().ok_or("TCE-MS: nenhuma unidade com liquidação em 2026")?;
    Ok(format!("{:02}", m_rec.min(m_des)))
}

/// Acao nova. `f`/`sub` = "13 - Cultura" ou so o codigo; Camara = funcao 01 so da Camara.
fn acao(f: &str, sub: &str, cod: &str, nome: &str, camara: bool) -> Acao {
    let (mut f, sub) = (antes_do_traco(f), antes_do_traco(sub));
    if !f.is_empty() && f.chars().all(|c| c.is_ascii_digit()) && f.trim_start_matches('0') == "1" && !camara {
        f = "0";
    }
    let (f, s0) = (if f.is_empty() { "0" } else { f }, if sub.is_empty() { "0" } else { sub });
    // cod: o script de setores le a subfuncao na 4a posicao
    Acao { f: f.into(), sub: sub.into(), cod: format!("0.0.{f}.{s0}.0.{cod}"), acao: nome.into(), ..Default::default() }
}

/// 2025 e 2026: execucao do e-Sfinge ate o mes de corte, dotacao por numero da acao e nomes do PPA vigente.
fn esfinge(bruto: Bruto, a: &str, corte: &str) -> Result<Vec<Acao>, String> {
    let mut nomes: BTreeMap<String, String> = BTreeMap::new();
    for arq in if a == "2025" { &["ppa2022b", "ppa2022"][..] } else { &["ppa2026"][..] } {
        // o segundo arquivo manda
        for l in cubo(bruto, arq, 5)? {
            let desc = t(&l, 1)?;
            nomes.insert(t(&l, 0)?.into(), desc.split_once(" - ").map_or(desc, |x| x.1).trim().into());
        }
    }
    let nome = |cod: &str| nomes.get(cod).filter(|n| !n.is_empty()).cloned().unwrap_or_else(|| format!("AÇÃO {cod} (sem nome no e-Sfinge)"));
    let (mut acoes, mut do_cod): (Acoes, BTreeMap<String, Vec<usize>>) = Default::default();
    let limite = format!("{a}-{corte}");
    for l in cubo(bruto, &format!("exec{a}"), 9)? {
        if t(&l, 6)? > limite.as_str() {
            continue;
        }
        let (f, sub, cod, tipo, v) = (t(&l, 2)?, t(&l, 3)?, t(&l, 4)?, t(&l, 5)?, n(&l, 8)?);
        let cam = sem(t(&l, 1)?).contains("CAMARA MUNICIPAL");
        let (i, nova) = acoes.idx(&[cod, f, sub, if cam { "C" } else { "" }].join("\u{1}"));
        if nova {
            acoes.lista[i] = acao(f, sub, &format!("{cod}.{}.{}{}", f.split('-').next().unwrap_or(""), sub.split('-').next().unwrap_or(""), if cam { ".CAM" } else { "" }), &nome(cod), cam);
            do_cod.entry(cod.into()).or_default().push(i);
        }
        match tipo {
            "Liquidacao" => acoes.lista[i].liquidado += v,
            "Empenho" => acoes.lista[i].empenhado += v,
            _ => {}
        }
    }
    // dotacao por numero da acao: vai para a combinacao funcao/subfuncao da acao com mais liquidado
    for l in cubo(bruto, &format!("dot{a}"), 3)? {
        let cod = t(&l, 0)?;
        let alvo = do_cod.get(cod).and_then(|is| is.iter().copied().fold(None, |m: Option<usize>, i| if m.is_none_or(|m| acoes.lista[i].liquidado > acoes.lista[m].liquidado) { Some(i) } else { m }));
        let i = match alvo {
            Some(i) => i,
            None => {
                let (i, nova) = acoes.idx(&[cod, "-", "-", ""].join("\u{1}"));
                if nova {
                    acoes.lista[i] = acao("0", "0", &format!("{cod}.0.0"), &nome(cod), false);
                    do_cod.entry(cod.into()).or_default().push(i);
                }
                i
            }
        };
        acoes.lista[i].orcado += n(&l, 1)?;
        acoes.lista[i].atualizado += n(&l, 2)?;
    }
    Ok(acoes.lista)
}

/// 2024: valores do "Detalhe Empenho"; nome da acao e codigos de funcao/subfuncao do SICOM pelo numero do projeto/atividade
/// (o mesmo numero com dois nomes desempata pela unidade gestora).
fn sicom(bruto: Bruto) -> Result<Vec<Acao>, String> {
    let ug = |x: &str| sem(x).replace(" DE CAMPO GRANDE", "").trim_end_matches('S').to_string();
    type Nomes = BTreeMap<String, BTreeSet<(String, String)>>;
    let (mut nomes, mut por_nr, mut cf, mut cs): (Nomes, Nomes, BTreeMap<String, String>, BTreeMap<String, String>) = Default::default();
    for l in cubo(bruto, "nomes2024", 8)? {
        let (org, fd, sd, nr, nome) = (ug(t(&l, 0)?), t(&l, 1)?, t(&l, 2)?, t(&l, 5)?, t(&l, 6)?);
        nomes.entry([fd, sd, nr].join("\u{1}")).or_default().insert((org.clone(), nome.into()));
        por_nr.entry(nr.into()).or_default().insert((org, nome.into())); // reserva: mesmo numero na mesma unidade com outra funcao/subfuncao
        cf.insert(fd.into(), t(&l, 3)?.into());
        cs.insert(sd.into(), t(&l, 4)?.into());
    }
    // tabela completa de funcoes e subfuncoes
    let tab = u::json(&bruto("funcoes")?, "funcoes")?;
    for (k, dic) in [("f", &mut cf), ("s", &mut cs)] {
        for l in linhas(&tab[k], "funcoes", 3)? {
            dic.entry(t(&l, 0)?.into()).or_insert(t(&l, 1)?.into());
        }
    }
    let mut acoes = Acoes::default();
    for l in cubo(bruto, "des2024", 6)? {
        let (org, fd, sd, nr) = (t(&l, 0)?, t(&l, 1)?, t(&l, 2)?, t(&l, 3)?);
        let vazio = BTreeSet::new();
        let cand = nomes.get(&[fd, sd, nr].join("\u{1}")).unwrap_or(&vazio);
        fn da_ug<'a>(c: &'a BTreeSet<(String, String)>, ug: &str) -> BTreeSet<&'a str> {
            c.iter().filter(|x| x.0 == ug).map(|x| x.1.as_str()).collect()
        }
        let mut nm = da_ug(cand, &ug(org));
        if nm.is_empty() {
            nm = cand.iter().map(|x| x.1.as_str()).collect();
        }
        if nm.is_empty() {
            nm = da_ug(por_nr.get(nr).unwrap_or(&vazio), &ug(org));
        }
        let nome = if nm.is_empty() { format!("AÇÃO {nr} (sem nome no SICOM)") } else { nm.into_iter().collect::<Vec<_>>().join(" / ") };
        let f = cf.get(fd).filter(|x| !x.is_empty()).ok_or_else(|| erro(&format!("função {fd}")))?;
        let sub = cs.get(sd).filter(|x| !x.is_empty()).ok_or_else(|| erro(&format!("subfunção {sd}")))?;
        let cam = sem(org).contains("CAMARA MUNICIPAL");
        let (i, nova) = acoes.idx(&[nr, nome.as_str(), f.as_str(), sub.as_str(), if cam { "C" } else { "" }].join("\u{1}"));
        if nova {
            acoes.lista[i] = acao(f, sub, &format!("{nr}.{f}.{sub}{}", if cam { ".CAM" } else { "" }), &nome, cam);
        }
        acoes.lista[i].liquidado += n(&l, 4)?;
        acoes.lista[i].empenhado += n(&l, 5)?;
    }
    Ok(acoes.lista)
}

/// Receita por origem (so as diferentes de zero, da maior para a menor).
fn receita(bruto: Bruto, a: &str, corte: &str) -> Result<Vec<(String, f64)>, String> {
    let mut orig = vec![];
    if a == "2024" {
        // receita ACUMULADA: de cada (unidade, rubrica, fonte) vale o ultimo mes
        let meses: Vec<String> = base::NM.iter().map(|m| m.to_uppercase()).collect();
        let (mut ultimo, mut onde): (Vec<(usize, String, String, f64)>, BTreeMap<String, usize>) = Default::default();
        for l in cubo(bruto, "rec2024", 6)? {
            let Some(m) = meses.iter().position(|x| x == t(&l, 4).unwrap_or("")) else { continue };
            let (k, novo) = ([t(&l, 0)?, t(&l, 1)?, t(&l, 3)?].join("\u{1}"), (m, t(&l, 1)?.to_string(), t(&l, 2)?.to_string(), n(&l, 5)?));
            match onde.get(&k) {
                Some(i) if m > ultimo[*i].0 => ultimo[*i] = novo,
                Some(_) => {}
                None => {
                    onde.insert(k, ultimo.len());
                    ultimo.push(novo);
                }
            }
        }
        for (_, rub, desc, v) in ultimo {
            if rub.is_empty() {
                return Err(erro("rubrica vazia"));
            }
            if rub.starts_with('9') {
                u::soma(&mut orig, if sem(&desc).contains("FUNDEB") || rub.starts_with("917") { "Deduções da receita (Fundeb)" } else { "Outras deduções da receita" }, -v);
            } else {
                u::soma(&mut orig, u::origem(ini(&rub, 2)).or(rub.starts_with(['7', '8']).then_some(u::INTRA)).ok_or_else(|| erro(&rub))?, v);
            }
        }
    } else {
        let c26 = u::int(corte, "mês de corte")?;
        for l in cubo(bruto, "rec_esf", 6)? {
            if t(&l, 0)? != a || mes_de(t(&l, 1)?)? > c26 {
                continue;
            }
            let (c, ded, v) = (t(&l, 3)?.replace('.', ""), t(&l, 4)?, n(&l, 5)?);
            if !["0", "", "-"].contains(&ded) {
                u::soma(&mut orig, if ded == "95" { "Deduções da receita (Fundeb)" } else { "Outras deduções da receita" }, v); // ja vem negativo
            } else {
                // natureza vazia cai nas intraorcamentarias, como no Python
                u::soma(&mut orig, u::origem(ini(&c, 2)).or((c.is_empty() || c.starts_with(['7', '8'])).then_some(u::INTRA)).ok_or_else(|| erro(&c))?, v);
            }
        }
    }
    orig.retain(|o| o.1 != 0.0);
    Ok(orig)
}

/// Le os 3 anos. Devolve os dados no formato da base e o mes de corte de 2026.
pub fn ler(bruto: Bruto) -> Result<(Dados, String), String> {
    let m26 = mes26(bruto)?;
    let mut dados = Dados::new();
    for a in base::ANOS {
        let corte = if a == "2026" { m26.as_str() } else { "12" };
        let acoes = if a == "2024" { sicom(bruto)? } else { esfinge(bruto, a, corte)? };
        let sis = if a == "2024" { "SICOM" } else { "e-Sfinge" };
        dados.insert(a.into(), u::ano(acoes, receita(bruto, a, corte)?, format!("TCE-MS painel · {sis} despesa até {corte}/{a}"), format!("TCE-MS painel · {sis} receita até {corte}/{a}")));
    }
    Ok((dados, m26))
}

/// O que a esteira chama. `ibge` = codigo da cidade (lista do IBGE de MS, de quem chama).
pub fn preparar(bruto: Bruto, setores: &dyn Fn(&Path) -> Result<(), String>, numero: &str, nome: &str, pasta: &Path, ibge: i64) -> Result<Value, String> {
    let (dados, mes26) = ler(bruto)?;
    base::montar(pasta, numero, nome, "MS", ibge, &tribunal(), &dados, &mes26, &base::Opcoes::default(), setores)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paineis(nome: &str) -> Result<String, String> {
        // execucao do e-Sfinge: ug, nome da ug, funcao, subfuncao, acao, tipo, anoMes, competencia (AAAA + bimestre), saldo
        let exec = |a: &str| {
            json!([
                ["1", "Prefeitura Municipal", "13 - Cultura", "392 - Difusão", "2050", "Empenho", format!("{a}-02"), format!("{a}1"), 1000.0],
                ["1", "Prefeitura Municipal", "13 - Cultura", "392 - Difusão", "2050", "Liquidacao", format!("{a}-03"), format!("{a}2"), 800.5],
                ["1", "Prefeitura Municipal", "13 - Cultura", "392 - Difusão", "2050", "Pagamento", format!("{a}-03"), format!("{a}2"), 700.0],
                ["1", "Prefeitura Municipal", "13 - Cultura", "392 - Difusão", "2050", "Liquidacao", format!("{a}-09"), format!("{a}5"), 50.0],
                ["1", "Prefeitura Municipal", "13 - Cultura", "391 - Patrimônio", "2050", "Liquidacao", format!("{a}-01"), format!("{a}1"), 10.0],
                ["1", "Prefeitura Municipal", "01 - Legislativa", "031 - Ação", "2001", "Liquidacao", format!("{a}-01"), format!("{a}1"), 5.0],
                ["2", "Câmara Municipal de Campo Grande", "01 - Legislativa", "031 - Ação", "2001", "Liquidacao", format!("{a}-04"), format!("{a}4"), 300.0],
                ["3", "Fundo sem liquidação", "08 - Assistência", "244 - Comunitária", "2060", "Empenho", format!("{a}-01"), format!("{a}1"), 9.0]
            ])
        };
        Ok(match nome {
            "rec_esf" => {
                let mut l = vec![];
                for (ano, ate) in [("2025", 12), ("2026", 9)] {
                    for m in 1..=ate {
                        l.push(json!([ano, format!("{ano}{m:02}"), "1", "1.1.1.2.50", "0", 1000.0]));
                        l.push(json!([ano, format!("{ano}{m:02}"), "1", "1.7.5.1", "95", -100.5]));
                        l.push(json!([ano, format!("{ano}{m:02}"), "1", "1.3.2.1", "98", -1.0]));
                    }
                }
                l.push(json!(["2026", "202602", "9", "7.2.1.8", "-", 40.0])); // unidade que so mandou receita em um mes
                json!(l)
            }
            "exec2025" => exec("2025"),
            "exec2026" => exec("2026"),
            "dot2025" | "dot2026" => json!([["2050", 2000.0, 2500.0], ["2001", 400.0, 400.0], ["7777", 60.0, 60.0]]),
            "ppa2022b" => json!([["2050", "2050 - NOME ANTIGO", "13", "392", 1.0], ["2001", " PROCESSO LEGISLATIVO ", "01", "031", 1.0]]),
            "ppa2022" | "ppa2026" => json!([["2050", "2050 - FESTIVAL DE INVERNO - EDIÇÃO", "13", "392", 1.0]]),
            "nomes2024" => json!([
                ["Fundos de Campo Grande", "Cultura", "Difusão", "13", "392", "2050", "FESTIVAL", 3.0],
                ["Secretaria de Obras", "Cultura", "Difusão", "13", "392", "2050", "PRAÇAS", 1.0],
                ["Câmara Municipal", "Legislativa", "Ação Legislativa", "01", "031", "2001", "PROCESSO LEGISLATIVO", 1.0]
            ]),
            "funcoes" => json!({ "f": [["Cultura", "99", 1.0], ["Saúde", "10", 1.0]], "s": [["Atenção Básica", "301", 1.0]] }),
            "des2024" => json!([
                ["Fundo de Campo Grande", "Cultura", "Difusão", "2050", 500.0, 600.0],
                ["Secretaria de Esporte", "Cultura", "Difusão", "2050", 20.0, 20.0],
                ["Câmara Municipal", "Legislativa", "Ação Legislativa", "2001", 70.0, 70.0],
                ["Secretaria de Saúde", "Saúde", "Atenção Básica", "3000", 30.0, 30.0]
            ]),
            "rec2024" => json!([
                ["Prefeitura", "1112", "IPTU", "F1", "NOVEMBRO", 900.0],
                ["Prefeitura", "1112", "IPTU", "F1", "DEZEMBRO", 1000.0],
                ["Prefeitura", "1112", "IPTU", "F1", "MARÇO", 200.0],
                ["Prefeitura", "1112", "IPTU", "F2", "JUNHO", 5.0],
                ["Prefeitura", "9517", "Dedução para o FUNDEB", "F1", "DEZEMBRO", 100.0],
                ["Prefeitura", "9111", "Restituições", "F1", "DEZEMBRO", 3.0],
                ["Prefeitura", "9171", "Dedução FPM", "F1", "DEZEMBRO", 2.0],
                ["Prefeitura", "7218", "Patronal", "F1", "Total", 999.0],
                ["Prefeitura", "1321", "Zerada", "F1", "DEZEMBRO", 0.0]
            ]),
            x => return Err(format!("cubo inesperado {x}")),
        }
        .to_string())
    }

    #[test]
    fn corte_e_leitura_dos_tres_anos() {
        // receita ate setembro (a unidade de um mes so nao conta); despesa: a Camara entregou ate o 4o bimestre -> agosto
        let (dados, m) = ler(&paineis).unwrap();
        assert_eq!(m, "08");
        let a = &dados["2026"];
        assert_eq!(a.acoes.len(), 6);
        let x = &a.acoes[0];
        assert_eq!((x.acao.as_str(), x.cod.as_str(), x.f.as_str(), x.sub.as_str()), ("FESTIVAL DE INVERNO - EDIÇÃO", "0.0.13.392.0.2050.13 .392 ", "13", "392"));
        assert_eq!((x.orcado, x.atualizado, x.empenhado, x.liquidado), (2000.0, 2500.0, 1000.0, 800.5)); // setembro fora; a dotacao vai para a combinacao com mais liquidado
        assert_eq!((a.acoes[1].orcado, a.acoes[2].f.as_str(), a.acoes[3].f.as_str(), a.acoes[3].orcado), (0.0, "0", "01", 400.0)); // funcao 01 so da Camara
        assert!(a.acoes[3].cod.ends_with(".CAM") && a.acoes[2].acao == "AÇÃO 2001 (sem nome no e-Sfinge)");
        assert_eq!((a.acoes[5].cod.as_str(), a.acoes[5].orcado, a.acoes[5].liquidado), ("0.0.0.0.0.7777.0.0", 60.0, 0.0)); // dotacao de acao sem execucao
        assert_eq!(a.receita, [("Impostos, taxas e contribuições de melhoria".to_string(), 8000.0), (u::INTRA.to_string(), 40.0), ("Outras deduções da receita".to_string(), -8.0), ("Deduções da receita (Fundeb)".to_string(), -804.0)]);
        assert_eq!(a.doc_despesa, "TCE-MS painel · e-Sfinge despesa até 08/2026");
        // 2025: o PPA 2022 (segundo arquivo) manda no nome; a acao que so esta no primeiro continua com nome
        let b = &dados["2025"];
        assert_eq!((b.acoes[0].acao.as_str(), b.acoes[0].liquidado, b.acoes[2].acao.as_str()), ("FESTIVAL DE INVERNO - EDIÇÃO", 850.5, "PROCESSO LEGISLATIVO"));
        // 2024 (SICOM): nome pela unidade; outra unidade junta os nomes; funcao/subfuncao pela tabela; receita = ultimo mes
        let c = &dados["2024"];
        assert_eq!(c.acoes.iter().map(|x| (x.acao.as_str(), x.cod.as_str(), x.f.as_str(), x.liquidado)).collect::<Vec<_>>(), [("FESTIVAL", "0.0.13.392.0.2050.13.392", "13", 500.0), ("FESTIVAL / PRAÇAS", "0.0.13.392.0.2050.13.392", "13", 20.0), ("PROCESSO LEGISLATIVO", "0.0.01.031.0.2001.01.031.CAM", "01", 70.0), ("AÇÃO 3000 (sem nome no SICOM)", "0.0.10.301.0.3000.10.301", "10", 30.0)]);
        assert_eq!(c.receita, [("Impostos, taxas e contribuições de melhoria".to_string(), 1005.0), ("Outras deduções da receita".to_string(), -3.0), ("Deduções da receita (Fundeb)".to_string(), -102.0)]);
        assert_eq!((c.arrecadado, c.acoes[0].orcado, c.doc_receita.as_str()), (900.0, 0.0, "TCE-MS painel · SICOM receita até 12/2024"));
    }

    #[test]
    fn recusa_o_que_falta() {
        let troca = |de: &'static str, para: &'static str| move |nome: &str| paineis(nome).map(|x| x.replace(de, para));
        assert!(ler(&troca("\"Saúde\",\"10\"", "\"Saude\",\"10\"")).unwrap_err().contains("código sem correspondência no tribunal: função Saúde"));
        assert!(ler(&troca("\"1321\",\"Zerada\"", "\"3321\",\"Zerada\"")).unwrap_err().contains("sem correspondência no tribunal: 3321"));
        assert!(ler(&troca("\"1.3.2.1\",\"98\"", "\"4.3.2.1\",\"0\"")).unwrap_err().contains("sem correspondência no tribunal: 4321"));
        assert!(ler(&troca("800.5", "\"-\"")).unwrap_err().contains("medida que não é número"));
        assert!(ler(&troca("[\"2001\",400.0,400.0]", "[\"2001\",400.0]")).unwrap_err().contains("cubo dot2025 fora do formato"));
        assert!(ler(&troca("\"Liquidacao\"", "\"Pagamento\"")).unwrap_err().contains("nenhuma unidade com liquidação em 2026"));
        assert!(ler(&troca("\"1\",\"1.", "\"1\",1.")).is_err() && ler(&|_| Err("socket caiu".into())).is_err());
    }
}
