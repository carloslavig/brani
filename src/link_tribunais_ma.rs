//! Molde do Maranhao (link/tribunais/ma.py), Sao Luis: SINC Fiscal do TCE-MA pela API de dados do tribunal (dotacao inicial,
//! alteracoes orcamentarias, empenhos, liquidacoes, receita orcamentaria e nome das acoes). Restos a pagar (empenho de outro ano)
//! ficam fora; anulacao/estorno (tipo X, alteracao 12) entram negativos. Mes de corte de 2026 = ultimo bimestre com envio COMPLETO
//! no Painel de Envio do SINC. A API nao ordena as paginas (a mesma linha volta em duas paginas e outra some): cada fatia tem de
//! vir INTEIRA de uma vez; fatia grande e dividida pelos filtros e as partes tem de somar o todo; no ultimo caso as paginas sao
//! repetidas ate a uniao das linhas fechar o total. Pagina incompleta, total que nao fecha, liquidacao sem o empenho, natureza de
//! receita sem origem, campo que falta = Err (pendencia), nunca dado aproximado.
//! Entram como closure: `q(ano, metodo, size, page, filtros)` devolve o JSON de uma consulta (uma por vez, pausa e novas
//! tentativas sao de quem chama; Err = o tribunal nao respondeu) e `envio(bimestre)` devolve o JSON do Painel de Envio.
//! Ficou de fora: o cache em arquivo das fatias (retomar de onde parou) e a lista do IBGE (so Sao Luis, como no Python).
//! Diferenca do Python: status "incompleta" no Painel de Envio NAO conta como completa (la a palavra "completa" casava dentro dela).
#![allow(dead_code)]
use crate::link_tribunais_base::{self as base, Dados};
use crate::link_tribunais_util::{self as u, ini, pystr, Acoes};
use crate::texto_pt::sem_acento;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const IBGE_SAO_LUIS: i64 = 2111300;
pub const FESTAS_EXTRA: &str = r"PROMOVER E DESENVOLVER O AMBIENTE CULTURAL DE S[AÃ]O LU[IÍ]S|ANIVERS[AÁ]RIO DE S[AÃ]O LU[IÍ]S|R[EÉ]VEILLON";
pub const SOCIAL: (&str, &str) = (r"BENEF[IÍ]CIOS SOCIOASSISTENCIAIS", "benefícios socioassistenciais");
pub const LIMITE_LENTO: usize = 35;
const DIMS26: [&str; 4] = ["tipoEmpenho", "classe", "credor*", "fonteRecurso*"]; // "*" = filtro pelo comeco: divide digito a digito
pub type Filtros = BTreeMap<String, String>;
pub type Consulta<'a> = &'a dyn Fn(&str, &str, usize, usize, &Filtros) -> Result<String, String>;
pub type Valores = BTreeMap<&'static str, BTreeSet<String>>;
/// (ano, metodo) -> linhas ja com os nomes de campo de 2024/2025.
pub type Arquivos = BTreeMap<(String, String), Vec<Value>>;

pub fn tribunal(pendencias: &[String]) -> Value {
    json!({ "sigla": "TCE-MA", "fala": "Tribunal de Contas do Estado", "uf": "MA", "estado": "Maranhão", "doc_despesa": "SINC FISCAL EMPENHOS", "doc_receita": "SINC FISCAL RECEITA",
        "tribunal_estado": "TCE-MA (Tribunal de Contas do Estado do Maranhão)", "site_tribunal": "tcema.tc.br",
        "fonte_md": "Tudo do SINC Fiscal do TCE-MA (Tribunal de Contas do Estado do Maranhão), pela API de dados do tribunal (tce-api.apps.tcema.tc.br, link na página inicial de www.tcema.tc.br; os painéis do TCE+ e do Dashboard Municipal mostram os mesmos dados só por função e programa): receita orçamentária (arrecadado menos estornos e deduções do Fundeb), dotação inicial da LOA, alterações orçamentárias, empenhos e liquidações por ação. \"Gastou\" = LIQUIDADO (menos estornos, sem restos a pagar), município inteiro. \"Previa\" = dotação inicial da lei do orçamento.",
        "setores_md": "Setores = soma do liquidado das ações (projeto/atividade) do SINC Fiscal do TCE-MA, pela função; festas e eventos separados da cultura; segurança pública fora.",
        "pendencias_md": pendencias, "orgaos": "Município inteiro (Prefeitura, Câmara, fundos e autarquias: remessa única do SINC Fiscal)",
        "legenda_docs": ["• Receita: SINC Fiscal do TCE-MA, receita orçamentária de 2024, 2025 e 2026", "• Despesa, setores e festas: SINC Fiscal do TCE-MA, dotação, empenhos e liquidações por projeto/atividade", "• tce-api.apps.tcema.tc.br (API de dados do TCE-MA) e Painel de Envio do SINC"] })
}

fn limite(met: &str) -> usize {
    if met == "empenho" { 1500 } else { 80 }
}

fn dims(met: &str) -> &'static [&'static str] {
    match met {
        "empenho" => &["cnpjUnidadeGestora", "codigoUnidadeOrcamentaria", "tipoEmpenho", "codigoAcao", "codigoNaturezaDespesa", "fonteRecurso"],
        "dotacao" => &["cnpjUnidadeGestora", "codigoUnidadeOrcamentaria", "codigoFuncao", "codigoAcao", "fonteRecurso"],
        _ => &["cnpjUnidadeGestora", "competencia", "tipoLancamentoReceitaOrcamentaria", "fonteRecurso"],
    }
}

fn prefixo(dim: &str) -> usize {
    if dim == "credor" { 14 } else { 4 }
}

/// 2026 veio com outro leiaute na API (codUo, codAcao...): traduz para os nomes de 2024/2025.
pub fn norm(linhas: &[Value]) -> Result<Vec<Value>, String> {
    let novo = |k: &str| match k {
        "codUo" => Some("codigoUnidadeOrcamentaria"),
        "codFuncao" => Some("codigoFuncao"),
        "codSubfuncao" => Some("codigoSubfuncao"),
        "codPrograma" => Some("codigoPrograma"),
        "codAcao" => Some("codigoAcao"),
        "codNaturezaDespesa" => Some("codigoNaturezaDespesa"),
        "tipoLancamentoRecOrc" => Some("tipoLancamentoReceitaOrcamentaria"),
        "codReceitaOrcamentaria" => Some("codigoReceitaOrcamentaria"),
        "entidadeidUo" | "entidadeIdUo" => Some("cnpjUnidadeGestora"),
        _ => None,
    };
    linhas
        .iter()
        .map(|x| {
            let o = x.as_object().ok_or("TCE-MA: linha da API que não é um registro")?;
            Ok(Value::Object(o.iter().filter(|(k, _)| *k != "arquivo").map(|(k, v)| match novo(k) {
                Some("cnpjUnidadeGestora") => ("cnpjUnidadeGestora".to_string(), json!(pystr(v))),
                n => (n.unwrap_or(k).to_string(), v.clone()),
            }).collect::<Map<_, _>>()))
        })
        .collect()
}

/// Uma consulta: a resposta tem de trazer `content`.
fn pagina(q: Consulta, ano: &str, met: &str, size: usize, page: usize, f: &Filtros) -> Result<Value, String> {
    let v = u::json(&q(ano, met, size, page, f)?, &format!("TCE-MA {ano} {met}"))?;
    if v["content"].is_array() { Ok(v) } else { Err(format!("TCE-MA não respondeu: {ano} {met} {f:?}")) }
}

fn inteiro(v: &Value, k: &str, o_que: &str) -> Result<usize, String> {
    v[k].as_u64().map(|n| n as usize).ok_or_else(|| format!("TCE-MA {o_que}: resposta sem {k}"))
}

fn conteudo(v: &Value) -> Vec<Value> {
    v["content"].as_array().cloned().unwrap_or_default()
}

/// Ultimo recurso (fatia que nenhum filtro divide): paginas de 25, repetidas ate a uniao das linhas fechar o total.
pub fn varrer(q: Consulta, ano: &str, met: &str, total: usize, f: &Filtros) -> Result<Vec<Value>, String> {
    let mut vistas: BTreeMap<String, Value> = BTreeMap::new();
    for _ in 0..8 {
        for pg in 0..total.div_ceil(25) {
            for x in conteudo(&pagina(q, ano, met, 25, pg, f)?) {
                vistas.insert(x.to_string(), x); // o Map do serde_json ja sai com as chaves em ordem
            }
        }
        if vistas.len() == total {
            return Ok(vistas.into_values().collect());
        }
    }
    Err(format!("TCE-MA {ano} {met} {f:?}: as páginas não fecharam {total} linhas (ficaram {})", vistas.len()))
}

/// Linhas de uma fatia; se passar do limite (ou o tribunal nao responder), divide pela proxima dimensao e confere que as partes
/// somam o todo.
pub fn fatia(q: Consulta, ano: &str, met: &str, valores: &Valores, dims: &[&str], f: &Filtros, lim: usize) -> Result<Vec<Value>, String> {
    let o_que = format!("{ano} {met} {f:?}");
    let total = inteiro(&pagina(q, ano, met, 1, 0, f)?, "totalElements", &o_que)?;
    if total == 0 {
        return Ok(vec![]);
    }
    if total <= lim {
        match pagina(q, ano, met, total + 10, 0, f) {
            Ok(r) => {
                let veio = inteiro(&r, "numberOfElements", &o_que)?;
                if veio != total || conteudo(&r).len() != total {
                    return Err(format!("TCE-MA {o_que}: veio {veio} de {total}"));
                }
                return Ok(conteudo(&r));
            }
            Err(_) if dims.is_empty() => return varrer(q, ano, met, total, f),
            Err(_) => {}
        }
    } else if dims.is_empty() {
        return varrer(q, ano, met, total, f);
    }
    let dim = dims[0].trim_end_matches('*');
    let com = |v: String| {
        let mut n = f.clone();
        n.insert(dim.to_string(), v);
        n
    };
    let mut linhas = vec![];
    if dims[0].ends_with('*') {
        // filtro pelo comeco: um digito a mais por nivel; codigo completo passa para a proxima dimensao
        let pre = f.get(dim).cloned().unwrap_or_default();
        if pre.chars().count() >= prefixo(dim) {
            return fatia(q, ano, met, valores, &dims[1..], f, lim);
        }
        for c in '0'..='9' {
            linhas.extend(fatia(q, ano, met, valores, dims, &com(format!("{pre}{c}")), lim)?);
        }
    } else {
        for v in valores.get(dim).into_iter().flatten() {
            linhas.extend(fatia(q, ano, met, valores, &dims[1..], &com(v.clone()), lim)?);
        }
    }
    if linhas.len() != total {
        // a lista de valores dessa dimensao nao cobre tudo: divide pela proxima; na ultima, varre (o varrer confere o total)
        return if dims.len() > 1 { fatia(q, ano, met, valores, &dims[1..], f, lim) } else { varrer(q, ano, met, total, f) };
    }
    Ok(linhas)
}

fn campo(x: &Value, k: &str) -> Result<String, String> {
    if x[k].is_null() { Err(format!("TCE-MA: registro sem o campo {k}")) } else { Ok(pystr(&x[k])) }
}

/// Baixa os 3 anos: os metodos rapidos numa consulta so (pagina incompleta = Err) e dotacao, receita e empenho em fatias.
pub fn baixar(q: Consulta) -> Result<Arquivos, String> {
    let mut arqs = Arquivos::new();
    for a in base::ANOS {
        let rapidos: &[&str] = if a == "2026" { &["acao", "atualizacaoorcamentaria", "liquidacao", "dotacao", "receitaorcamentaria"] } else { &["acao", "atualizacaoorcamentaria", "liquidacao", "unidadeorcamentaria"] };
        for met in rapidos {
            let r = pagina(q, a, met, 100_000, 0, &Filtros::new())?;
            let o_que = format!("{a} {met}");
            if inteiro(&r, "numberOfElements", &o_que)? != inteiro(&r, "totalElements", &o_que)? || conteudo(&r).len() != inteiro(&r, "totalElements", &o_que)? {
                return Err(format!("TCE-MA {a} {met}: página incompleta"));
            }
            arqs.insert((a.into(), met.to_string()), norm(&conteudo(&r))?);
        }
        let vazio = vec![];
        let de = |arqs: &Arquivos, mets: &[&str], k: &str| -> Result<BTreeSet<String>, String> { mets.iter().flat_map(|m| arqs.get(&(a.to_string(), m.to_string())).unwrap_or(&vazio)).map(|x| campo(x, k)).collect() };
        let fixo = |vs: &[&str]| vs.iter().map(|v| v.to_string()).collect::<BTreeSet<_>>();
        let mut valores = Valores::new();
        valores.insert("cnpjUnidadeGestora", de(&arqs, &["atualizacaoorcamentaria", "liquidacao"], "cnpjUnidadeGestora")?);
        valores.insert("codigoUnidadeOrcamentaria", de(&arqs, &["atualizacaoorcamentaria", "liquidacao", "unidadeorcamentaria"], "codigoUnidadeOrcamentaria")?);
        valores.insert("tipoEmpenho", fixo(&["O", "E", "G", "X", "A"]));
        valores.insert("tipoLancamentoReceitaOrcamentaria", fixo(&["O", "X"]));
        valores.insert("classe", fixo(&["0", "1"]));
        valores.insert("competencia", (1..=12).map(|m| format!("{m:02}{a}")).collect());
        valores.insert("codigoFuncao", (1..=28).map(|i| format!("{i:02}")).chain(["99".to_string()]).collect());
        valores.insert("codigoAcao", de(&arqs, &["acao", "atualizacaoorcamentaria"], "codigoAcao")?);
        valores.insert("fonteRecurso", de(&arqs, &["atualizacaoorcamentaria"], "fonteRecurso")?);
        valores.insert("codigoNaturezaDespesa", de(&arqs, &["liquidacao"], "codNaturezaDespesaCompleto")?.iter().map(|c| ini(c, 6).to_string()).collect());
        for met in ["dotacao", "receitaorcamentaria", "empenho"] {
            if !arqs.contains_key(&(a.to_string(), met.to_string())) {
                let linhas = fatia(q, a, met, &valores, if a == "2026" { &DIMS26 } else { dims(met) }, &Filtros::new(), if a == "2024" { limite(met) } else { LIMITE_LENTO })?;
                arqs.insert((a.into(), met.into()), norm(&linhas)?);
            }
            if met == "dotacao" {
                // a lista de unidades gestoras cresce com as que so aparecem na dotacao
                let mais = de(&arqs, &["dotacao"], "cnpjUnidadeGestora")?;
                valores.entry("cnpjUnidadeGestora").or_default().extend(mais);
            }
        }
    }
    Ok(arqs)
}

/// Ultimo mes de 2026 coberto por remessas COMPLETAS do SINC Fiscal (bimestrais) no Painel de Envio do TCE-MA.
pub fn mes26(nome: &str, envio: &dyn Fn(u8) -> Result<String, String>) -> Result<String, String> {
    let mut ok = 0;
    for b in 1..=6u8 {
        let v = u::json(&envio(b)?, "Painel de Envio do SINC")?;
        let rows = v["data"]["rows"].as_array().ok_or("Painel de Envio do SINC: resposta sem as linhas")?;
        let st: Vec<&Value> = rows.iter().filter(|r| r[0].as_str().is_some_and(|t| t.contains(&format!("-{nome}-")))).map(|r| &r[6]).collect();
        let [st] = st[..] else { return Err(format!("Painel de Envio do SINC: {nome} não achado (ou repetido) no {b}º bimestre")) };
        let st = st.as_str().ok_or_else(|| format!("Painel de Envio do SINC: {b}º bimestre sem o status da remessa"))?.to_lowercase();
        if !st.contains("completa") || st.contains("incompleta") {
            break;
        }
        ok = b;
    }
    if ok == 0 { Err("Painel de Envio do SINC: nenhum bimestre de 2026 completo".into()) } else { Ok(format!("{:02}", 2 * ok)) }
}

fn valor(x: &Value) -> Result<f64, String> {
    x["valor"].as_f64().ok_or_else(|| format!("TCE-MA: registro sem valor numérico: {}", ini(&x.to_string(), 120)))
}

fn chave(x: &Value) -> Result<[String; 5], String> {
    Ok([u::txt(x, "codigoUnidadeOrcamentaria")?.into(), u::txt(x, "codigoFuncao")?.into(), u::txt(x, "codigoSubfuncao")?.into(), u::txt(x, "codigoPrograma")?.into(), u::txt(x, "codigoAcao")?.into()])
}

/// A acao dessa chave, guardando a ordem em que as chaves apareceram.
fn entra<'a>(acoes: &'a mut Acoes, ordem: &mut Vec<String>, k: &str) -> &'a mut base::Acao {
    let (i, nova) = acoes.idx(k);
    if nova {
        ordem.push(k.to_string());
    }
    &mut acoes.lista[i]
}

/// Le os 3 anos. Devolve os dados no formato da base e as acoes sem nome no SINC ({ano: codigos}).
pub fn ler(arqs: &Arquivos, m26: &str) -> Result<(Dados, BTreeMap<String, Vec<String>>), String> {
    let j = |a: &str, m: &str| arqs.get(&(a.to_string(), m.to_string())).ok_or_else(|| format!("TCE-MA {a}: {m} não foi lido no tribunal"));
    let mut nomes: BTreeMap<&str, BTreeMap<(String, String), String>> = BTreeMap::new();
    for a in base::ANOS {
        let n = nomes.entry(a).or_default();
        for x in j(a, "acao")? {
            n.insert((u::txt(x, "codigoPrograma")?.into(), u::txt(x, "codigoAcao")?.into()), u::txt(x, "tituloAcao")?.trim().into());
        }
    }
    let (mut dados, mut sem_nome) = (Dados::new(), BTreeMap::new());
    for a in base::ANOS {
        let corte = if a == "2026" { u::int(m26, "mês de corte")? } else { 12 };
        let no_corte = |data: &str| -> Result<bool, String> { Ok(ini(data, 4) == a && u::int(data.get(5..7).ok_or_else(|| format!("TCE-MA {a}: data fora do formato: {data:?}"))?, "mês")? <= corte) };
        let sinal = |x: &Value, k: &str| -> Result<f64, String> { Ok(if u::txt(x, k)? == "X" { -valor(x)? } else { valor(x)? }) };
        let (mut acoes, mut ordem) = (Acoes::default(), vec![]);
        for x in j(a, "dotacao")? {
            let ac = entra(&mut acoes, &mut ordem, &chave(x)?.join("\u{1}"));
            ac.orcado += valor(x)?;
            ac.atualizado += valor(x)?;
        }
        for x in j(a, "atualizacaoorcamentaria")? {
            if no_corte(u::txt(x, "dataAto")?)? {
                // tipoAlteracao 12 = anulacao de dotacao
                entra(&mut acoes, &mut ordem, &chave(x)?.join("\u{1}")).atualizado += if x["tipoAlteracao"].as_i64() == Some(12) { -valor(x)? } else { valor(x)? };
            }
        }
        let mut de: BTreeMap<[String; 3], String> = BTreeMap::new();
        for x in j(a, "empenho")? {
            let nr = u::txt(x, "numeroEmpenho")?;
            if !nr.ends_with(a) {
                continue; // empenho de ano anterior (restos a pagar, lancado em 01/01): fora
            }
            let k = chave(x)?.join("\u{1}");
            de.insert([campo(x, "cnpjUnidadeGestora")?, campo(x, "codigoUnidadeOrcamentaria")?, nr.into()], k.clone()); // o numero se repete entre unidades
            if no_corte(u::txt(x, "dataEmpenho")?)? {
                entra(&mut acoes, &mut ordem, &k).empenhado += sinal(x, "tipoEmpenho")?;
            }
        }
        for x in j(a, "liquidacao")? {
            let nr = u::txt(x, "numeroEmpenhoOriginal")?;
            if !nr.ends_with(a) || !no_corte(u::txt(x, "dataLiquidacao")?)? {
                continue; // empenho de outro ano = restos a pagar
            }
            let k = de.get(&[campo(x, "cnpjUnidadeGestora")?, campo(x, "codigoUnidadeOrcamentaria")?, nr.into()]).ok_or_else(|| format!("TCE-MA {a}: liquidação {} sem o empenho {nr} no SINC", pystr(&x["numeroLiquidacao"])))?;
            entra(&mut acoes, &mut ordem, k).liquidado += sinal(x, "tipoLiquidacao")?;
        }
        for (i, k) in ordem.iter().enumerate() {
            let p: Vec<&str> = k.split('\u{1}').collect();
            let (prog, cod) = ((p[3].to_string(), p[4].to_string()), p[4]);
            // acao criada por credito adicional nao esta na lista da LOA do ano: usa o nome do mesmo programa+acao em outro ano do SINC
            let nm = nomes[a].get(&prog).filter(|n| !n.is_empty()).or_else(|| base::ANOS.iter().find_map(|b| nomes[b].get(&prog))).cloned();
            let nm = nm.unwrap_or_else(|| {
                sem_nome.entry(a.to_string()).or_insert_with(Vec::new).push(cod.to_string());
                format!("Ação {cod} (sem nome no SINC do TCE-MA)")
            });
            let ac = &mut acoes.lista[i];
            (ac.f, ac.sub, ac.cod, ac.acao) = (p[1].into(), p[2].into(), p.join("."), nm);
        }
        let mut orig = vec![];
        for x in j(a, "receitaorcamentaria")? {
            let (c, comp) = (u::txt(x, "codigoReceitaOrcamentaria")?, u::txt(x, "competencia")?);
            if u::int(ini(comp, 2), "competência")? > corte || comp.get(2..) != Some(a) {
                continue;
            }
            let o = u::origem(ini(c, 2)).or(c.starts_with(['7', '8']).then_some(u::INTRA)).ok_or_else(|| format!("TCE-MA {a}: natureza de receita sem origem conhecida: {c}"))?;
            u::soma(&mut orig, o, sinal(x, "tipoLancamentoReceitaOrcamentaria")?);
        }
        let ate = format!("{corte:02}/{a}");
        dados.insert(a.into(), u::ano(acoes.lista, orig, format!("SINC Fiscal TCE-MA · dotação, empenhos e liquidações até {ate}"), format!("SINC Fiscal TCE-MA · receita orçamentária até {ate}")));
    }
    Ok((dados, sem_nome))
}

/// O que a esteira chama. So Sao Luis (o molde foi conferido so com ela).
pub fn preparar(q: Consulta, envio: &dyn Fn(u8) -> Result<String, String>, setores: &dyn Fn(&Path) -> Result<(), String>, numero: &str, nome: &str, pasta: &Path) -> Result<Value, String> {
    if sem_acento(nome).to_uppercase() != "SAO LUIS" {
        return Err("molde do MA testado só com São Luís (IBGE 2111300); outra cidade precisa do código IBGE dela".into());
    }
    let m26 = mes26("São Luís", envio)?;
    let (dados, sem_nome) = ler(&baixar(q)?, &m26)?;
    let pend: Vec<String> = if sem_nome.is_empty() {
        vec![]
    } else {
        let por_ano = |(a, c): (&String, &Vec<String>)| format!("{a}: {}", c.iter().collect::<BTreeSet<_>>().into_iter().cloned().collect::<Vec<_>>().join(", "));
        vec![format!("Ações sem nome no SINC do TCE-MA (criadas por crédito adicional, fora da lista da LOA): {}", sem_nome.iter().map(por_ano).collect::<Vec<_>>().join("; "))]
    };
    let op = base::Opcoes { festas_extra: Some(FESTAS_EXTRA), social: Some(SOCIAL), ..Default::default() };
    base::montar(pasta, numero, nome, "MA", IBGE_SAO_LUIS, &tribunal(&pend), &dados, &m26, &op, setores)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// As tabelas do SINC de um ano, com os nomes de campo de 2024/2025.
    fn tabela(a: &str, met: &str) -> Vec<Value> {
        let k = |acao: &str| json!({ "cnpjUnidadeGestora": "06", "codigoUnidadeOrcamentaria": "1301", "codigoFuncao": "13", "codigoSubfuncao": "392", "codigoPrograma": "0020", "codigoAcao": acao, "fonteRecurso": "1500" });
        let com = |b: Value, mais: Value| {
            let mut o = b.as_object().unwrap().clone();
            o.extend(mais.as_object().unwrap().clone());
            Value::Object(o)
        };
        match met {
            "acao" => vec![json!({ "codigoPrograma": "0020", "codigoAcao": "2100", "tituloAcao": " SÃO JOÃO DE SÃO LUÍS " })],
            "unidadeorcamentaria" => vec![json!({ "codigoUnidadeOrcamentaria": "1301" })],
            "dotacao" => vec![com(k("2100"), json!({ "valor": 1000.0 })), com(k("2100"), json!({ "valor": 500.0 }))],
            "atualizacaoorcamentaria" => vec![
                com(k("2100"), json!({ "valor": 200.0, "tipoAlteracao": 13, "dataAto": format!("{a}-03-10") })),
                com(k("2100"), json!({ "valor": 50.0, "tipoAlteracao": 12, "dataAto": format!("{a}-04-10") })),
                com(k("9999"), json!({ "valor": 70.0, "tipoAlteracao": 3, "dataAto": format!("{a}-11-10") })),
            ],
            "empenho" => vec![
                com(k("2100"), json!({ "numeroEmpenho": format!("0001/{a}"), "dataEmpenho": format!("{a}-02-01"), "tipoEmpenho": "O", "valor": 900.0, "codigoNaturezaDespesa": "339039" })),
                com(k("2100"), json!({ "numeroEmpenho": format!("0001/{a}"), "dataEmpenho": format!("{a}-03-01"), "tipoEmpenho": "X", "valor": 100.0, "codigoNaturezaDespesa": "339039" })),
                com(k("2100"), json!({ "numeroEmpenho": "0007/2023", "dataEmpenho": format!("{a}-01-01"), "tipoEmpenho": "O", "valor": 5000.0, "codigoNaturezaDespesa": "339039" })),
                com(k("9999"), json!({ "numeroEmpenho": format!("0002/{a}"), "dataEmpenho": format!("{a}-11-05"), "tipoEmpenho": "O", "valor": 60.0, "codigoNaturezaDespesa": "339039" })),
            ],
            "liquidacao" => vec![
                com(k("2100"), json!({ "numeroLiquidacao": "L1", "numeroEmpenhoOriginal": format!("0001/{a}"), "dataLiquidacao": format!("{a}-04-01"), "tipoLiquidacao": "O", "valor": 700.0, "codNaturezaDespesaCompleto": "33903900" })),
                com(k("2100"), json!({ "numeroLiquidacao": "L2", "numeroEmpenhoOriginal": format!("0001/{a}"), "dataLiquidacao": format!("{a}-05-01"), "tipoLiquidacao": "X", "valor": 100.0, "codNaturezaDespesaCompleto": "33903900" })),
                com(k("2100"), json!({ "numeroLiquidacao": "L3", "numeroEmpenhoOriginal": "0007/2023", "dataLiquidacao": format!("{a}-02-01"), "tipoLiquidacao": "O", "valor": 5000.0, "codNaturezaDespesaCompleto": "33903900" })),
                com(k("9999"), json!({ "numeroLiquidacao": "L4", "numeroEmpenhoOriginal": format!("0002/{a}"), "dataLiquidacao": format!("{a}-11-20"), "tipoLiquidacao": "O", "valor": 60.0, "codNaturezaDespesaCompleto": "33903900" })),
            ],
            "receitaorcamentaria" => vec![
                json!({ "cnpjUnidadeGestora": "06", "competencia": format!("01{a}"), "tipoLancamentoReceitaOrcamentaria": "O", "fonteRecurso": "1500", "codigoReceitaOrcamentaria": "11125001", "valor": 3000.0 }),
                json!({ "cnpjUnidadeGestora": "06", "competencia": format!("02{a}"), "tipoLancamentoReceitaOrcamentaria": "X", "fonteRecurso": "1500", "codigoReceitaOrcamentaria": "17115001", "valor": 40.0 }),
                json!({ "cnpjUnidadeGestora": "06", "competencia": format!("12{a}"), "tipoLancamentoReceitaOrcamentaria": "O", "fonteRecurso": "1500", "codigoReceitaOrcamentaria": "72100000", "valor": 9.0 }),
            ],
            x => panic!("método inesperado {x}"),
        }
    }

    /// A API: filtra pelo valor exato (ou pelo comeco, em credor/fonte de 2026) e pagina.
    fn api(troca: &'static dyn Fn(&str, &str, Vec<Value>) -> Vec<Value>) -> impl Fn(&str, &str, usize, usize, &Filtros) -> Result<String, String> {
        move |a, met, size, page, f| {
            let t: Vec<Value> = troca(a, met, tabela(a, met)).into_iter().filter(|x| f.iter().all(|(k, v)| pystr(&x[k]).starts_with(v.as_str()))).collect();
            let pg: Vec<Value> = t.iter().skip(page * size).take(size).cloned().collect();
            Ok(json!({ "content": pg, "numberOfElements": pg.len(), "totalElements": t.len() }).to_string())
        }
    }

    fn igual(_: &str, _: &str, t: Vec<Value>) -> Vec<Value> {
        t
    }

    fn painel(completos: u8) -> impl Fn(u8) -> Result<String, String> {
        move |b| Ok(json!({ "data": { "rows": [["2-Outra-MA", 0, 0, 0, 0, 0, "Remessa completa"], ["1-São Luís-MA", 0, 0, 0, 0, 0, if b <= completos { "Remessa completa" } else { "Remessa incompleta" }]] } }).to_string())
    }

    #[test]
    fn le_os_tres_anos_sem_restos_a_pagar() {
        assert_eq!(mes26("São Luís", &painel(2)).unwrap(), "04");
        assert!(mes26("São Luís", &painel(0)).unwrap_err().contains("nenhum bimestre"));
        assert!(mes26("Imperatriz", &painel(2)).unwrap_err().contains("não achado"));
        let arqs = baixar(&api(&igual)).unwrap();
        let (dados, sem_nome) = ler(&arqs, "04").unwrap();
        let a = &dados["2025"];
        let x = &a.acoes[0];
        assert_eq!((x.cod.as_str(), x.acao.as_str(), x.f.as_str(), x.sub.as_str()), ("1301.13.392.0020.2100", "SÃO JOÃO DE SÃO LUÍS", "13", "392"));
        assert_eq!((x.orcado, x.atualizado, x.empenhado, x.liquidado), (1500.0, 1650.0, 800.0, 600.0)); // anulacao e estorno negativos; restos a pagar fora
        assert_eq!((a.acoes[1].acao.as_str(), a.acoes[1].atualizado, a.acoes[1].liquidado), ("Ação 9999 (sem nome no SINC do TCE-MA)", 70.0, 60.0));
        assert_eq!(a.receita, [("Impostos, taxas e contribuições de melhoria".to_string(), 3000.0), (u::INTRA.to_string(), 9.0), ("Transferências correntes".to_string(), -40.0)]);
        assert_eq!((a.liquidado, a.arrecadado, a.doc_despesa.as_str()), (660.0, 2969.0, "SINC Fiscal TCE-MA · dotação, empenhos e liquidações até 12/2025"));
        // 2026 so ate abril: a acao que so se mexeu em novembro nao entra e a receita de dezembro sai
        let b = &dados["2026"];
        assert_eq!((b.acoes.len(), b.acoes[0].liquidado, b.arrecadado), (1, 700.0, 2960.0)); // o estorno de maio fica fora
        assert_eq!(sem_nome["2024"], ["9999"]);
        assert!(preparar(&api(&igual), &painel(2), &|_| Ok(()), "1", "Imperatriz", Path::new("x")).unwrap_err().contains("só com São Luís"));
    }

    #[test]
    fn leiaute_de_2026_vira_o_de_2025() {
        let n = norm(&[json!({ "codUo": "1301", "entidadeIdUo": 6, "codAcao": "2100", "arquivo": "x.json", "valor": 1.5 })]).unwrap();
        assert_eq!(n, [json!({ "codigoUnidadeOrcamentaria": "1301", "cnpjUnidadeGestora": "6", "codigoAcao": "2100", "valor": 1.5 })]);
        assert!(norm(&[json!(3)]).is_err());
    }

    #[test]
    fn fatia_divide_confere_e_recusa() {
        let linhas: Vec<Value> = (0..60).map(|i| json!({ "ug": format!("{}", i % 3), "fonte": format!("15{:02}", i % 4), "n": i })).collect();
        let contagem = RefCell::new(0);
        let serve = |esconde: bool, repete: bool| {
            let (linhas, contagem) = (linhas.clone(), &contagem);
            move |_: &str, _: &str, size: usize, page: usize, f: &Filtros| -> Result<String, String> {
                *contagem.borrow_mut() += 1;
                let t: Vec<&Value> = linhas.iter().filter(|x| f.iter().all(|(k, v)| pystr(&x[k]).starts_with(v.as_str()))).collect();
                let mut pg: Vec<Value> = t.iter().skip(page * size).take(size).map(|x| (*x).clone()).collect();
                if esconde && size > 1 && pg.len() > 1 {
                    pg.pop(); // o tribunal devolve menos linhas do que diz
                }
                if repete && size == 25 && page > 0 {
                    pg = t.iter().take(size).map(|x| (*x).clone()).collect(); // a mesma pagina de novo: outra linha some
                }
                Ok(json!({ "content": pg, "numberOfElements": pg.len(), "totalElements": t.len() }).to_string())
            }
        };
        let valores: Valores = [("ug", ["0", "1", "2"].iter().map(|s| s.to_string()).collect())].into();
        let todas = |v: Vec<Value>| v.iter().map(|x| x["n"].as_i64().unwrap()).collect::<BTreeSet<_>>().len();
        // cabe no limite: uma consulta so (mais a do total)
        assert_eq!(todas(fatia(&serve(false, false), "2024", "empenho", &valores, &["ug"], &Filtros::new(), 80).unwrap()), 60);
        assert_eq!(*contagem.borrow(), 2);
        // passa do limite: divide pela unidade (20 cada) e as partes somam o todo
        assert_eq!(todas(fatia(&serve(false, false), "2024", "empenho", &valores, &["ug"], &Filtros::new(), 35).unwrap()), 60);
        // a lista de unidades nao cobre tudo: vai para a proxima dimensao (filtro pelo comeco, digito a digito)
        let poucas: Valores = [("ug", ["0".to_string()].into())].into();
        assert_eq!(todas(fatia(&serve(false, false), "2026", "empenho", &poucas, &["ug", "fonte*"], &Filtros::new(), 35).unwrap()), 60);
        // nenhum filtro divide: paginas de 25 ate fechar o total
        assert_eq!(todas(fatia(&serve(false, false), "2024", "empenho", &valores, &[], &Filtros::new(), 35).unwrap()), 60);
        // pagina cortada e pagina repetida: Err, nunca a soma pela metade
        assert!(fatia(&serve(true, false), "2024", "empenho", &valores, &["ug"], &Filtros::new(), 80).unwrap_err().contains("veio 59 de 60"));
        assert!(fatia(&serve(false, true), "2024", "empenho", &valores, &[], &Filtros::new(), 35).unwrap_err().contains("não fecharam 60 linhas"));
        assert!(fatia(&|_, _, _, _, _| Err("caiu".into()), "2024", "empenho", &valores, &[], &Filtros::new(), 35).is_err());
    }

    #[test]
    fn recusa_o_que_falta() {
        fn sem_empenho(_: &str, met: &str, t: Vec<Value>) -> Vec<Value> {
            if met == "empenho" { t.into_iter().filter(|x| x["codigoAcao"] != "9999").collect() } else { t }
        }
        fn natureza(_: &str, met: &str, mut t: Vec<Value>) -> Vec<Value> {
            if met == "receitaorcamentaria" {
                t[0]["codigoReceitaOrcamentaria"] = json!("31125001");
            }
            t
        }
        fn texto(_: &str, met: &str, mut t: Vec<Value>) -> Vec<Value> {
            if met == "dotacao" {
                t[0]["valor"] = json!("mil");
            }
            t
        }
        fn sem_campo(_: &str, met: &str, mut t: Vec<Value>) -> Vec<Value> {
            if met == "liquidacao" {
                t[0].as_object_mut().unwrap().remove("dataLiquidacao");
            }
            t
        }
        let le = |troca: &'static dyn Fn(&str, &str, Vec<Value>) -> Vec<Value>| baixar(&api(troca)).and_then(|a| ler(&a, "12")).map(|_| ());
        assert!(le(&sem_empenho).unwrap_err().contains("liquidação L4 sem o empenho"));
        assert!(le(&natureza).unwrap_err().contains("sem origem conhecida: 31125001"));
        assert!(le(&texto).unwrap_err().contains("sem valor numérico"));
        assert!(le(&sem_campo).unwrap_err().contains("sem o campo dataLiquidacao"));
        // metodo rapido com pagina incompleta
        let curta = |a: &str, met: &str, size: usize, page: usize, f: &Filtros| api(&igual)(a, met, size, page, f).map(|t| if met == "liquidacao" { t.replace("\"totalElements\":4", "\"totalElements\":5") } else { t });
        assert!(baixar(&curta).unwrap_err().contains("liquidacao: página incompleta"));
        let mut arqs = baixar(&api(&igual)).unwrap();
        arqs.remove(&("2025".to_string(), "empenho".to_string()));
        assert!(ler(&arqs, "12").unwrap_err().contains("2025: empenho não foi lido"));
    }
}
