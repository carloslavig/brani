//! Molde do Ceara (link/tribunais/ce.py): balancetes do SIM do TCE-CE (API de dados abertos, ate 1000 linhas por pagina).
//! A PAGINACAO DA API E INSTAVEL (a mesma pagina pedida duas vezes veio com so 892 das 1000 linhas em comum; um balancete
//! saiu com linhas repetidas e outras faltando, e o video ficaria com "gastou" errado). Por isso `get` NAO pagina: se a
//! consulta nao cabe numa pagina, corta pelos campos do orcamento (orgao, unidade, funcao, subfuncao) ate cada fatia caber
//! inteira; e `inteiro` PROVA que o download nao repetiu nem perdeu linha. Nao fechou = Err: a cidade vira pendencia.
//! A rede entra como closure `pagina(metodo, parametros) -> linhas` (uma consulta so; pausa e tentativas sao de quem chama).
//! Diferenca do Python: a validade de 20 dias do que ja foi baixado usa a hora passada por parametro.
#![allow(dead_code)]
use crate::link_tribunais_base::{self as base, Acao, Ano, Dados};
use crate::texto_pt::sem_acento;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

pub const PAGINA: usize = 1000;
/// Do corte maior para o menor.
pub const CORTES: [&str; 4] = ["codigo_orgao", "codigo_unidade_orcamentaria", "codigo_funcao", "codigo_subfuncao"];
pub const VALIDADE_S: f64 = 20.0 * 86400.0; // o SIM recebe balancete todo mes
/// A acao generica onde a capital poe festas e eventos, sem a palavra "festa".
pub const FESTAS_EXTRA: &str = "^APOIO E REALIZACAO DE EVENTOS$";
const DESPESA: &str = "balancetes_despesas_orcamentarias";
const RECEITA: &str = "balancetes_receitas_orcamentarias";

pub type Params = BTreeMap<String, String>;
pub type Pagina<'a> = &'a dyn Fn(&str, &Params) -> Result<Vec<Value>, String>;

pub fn tribunal() -> Value {
    json!({ "sigla": "TCE-CE", "fala": "Tribunal de Contas do Estado", "uf": "CE", "estado": "Ceará", "doc_despesa": "SIM BALANCETE DE DESPESA", "doc_receita": "SIM BALANCETE DE RECEITA",
        "tribunal_estado": "TCE-CE (Tribunal de Contas do Estado do Ceará)", "site_tribunal": "api-dados-abertos.tce.ce.gov.br/sim",
        "fonte_md": "Tudo do SIM do TCE-CE (Tribunal de Contas do Estado do Ceará), a base do Portal de Transparência dos Municípios, pela API de Dados Abertos (api-dados-abertos.tce.ce.gov.br/sim): balancetes de receita orçamentária (arrecadado menos anulações até o mês; deduções do Fundeb já descontadas) e balancetes de despesa orçamentária (fixado, suplementado, anulado, empenhado e liquidado acumulados). \"Gastou\" = LIQUIDADO menos estornos, todos os órgãos do município. \"Previa\" = valor fixado no orçamento (dotação inicial da lei).",
        "setores_md": "Setores = soma do liquidado das ações (projeto/atividade) dos balancetes de despesa do SIM, pela função; festas e eventos separados da cultura; segurança pública fora.",
        "pendencias_md": [], "orgaos": "Município inteiro (Câmara, secretarias, fundos e entidades que mandam o SIM)",
        "legenda_docs": ["• Receita: SIM do TCE-CE, balancete de receita orçamentária de 2024, 2025 e 2026", "• Despesa, setores e festas: SIM do TCE-CE, balancete de despesa orçamentária (fixado e liquidado), por projeto/atividade", "• api-dados-abertos.tce.ce.gov.br/sim (Portal de Transparência dos Municípios)"] })
}

fn origem(rubrica: &str) -> Option<&'static str> {
    Some(match rubrica.get(..2)? {
        "11" => "Impostos, taxas e contribuições de melhoria",
        "12" => "Contribuições",
        "13" => "Receita patrimonial",
        "14" => "Receita agropecuária",
        "15" => "Receita industrial",
        "16" => "Receita de serviços",
        "17" => "Transferências correntes",
        "19" => "Outras receitas correntes",
        "21" => "Operações de crédito",
        "22" => "Alienação de bens",
        "23" => "Amortização de empréstimos",
        "24" => "Transferências de capital",
        "29" => "Outras receitas de capital",
        "95" => "Deduções da receita (Fundeb)",
        r if r.starts_with(['7', '8']) => "Receitas intraorçamentárias",
        r if r.starts_with('9') => "Outras deduções da receita",
        _ => return None,
    })
}

fn s<'a>(x: &'a Value, k: &str) -> &'a str {
    x[k].as_str().unwrap_or("")
}

fn f(x: &Value, k: &str) -> f64 {
    x[k].as_f64().or_else(|| x[k].as_str().and_then(|t| t.trim().parse().ok())).unwrap_or(0.0)
}

/// A linha inteira como texto (chaves em ordem): duas linhas iguais dao o mesmo texto.
fn txt(x: &Value) -> String {
    x.to_string()
}

fn params(pares: &[(&str, &str)]) -> Params {
    pares.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn com(p: &Params, k: &str, v: &str) -> Params {
    let mut q = p.clone();
    q.insert(k.into(), v.into());
    q
}

/// Tudo de um metodo SEM paginar. `orc` = linhas do orcamento (de onde saem os valores de cada corte).
pub fn get(pagina: Pagina, met: &str, orc: &[Value], p: &Params) -> Result<Vec<Value>, String> {
    let r = pagina(met, p)?;
    if r.len() < PAGINA {
        return Ok(r);
    }
    let campo = CORTES.iter().find(|c| !p.contains_key(**c));
    let valores: BTreeSet<&str> = match campo {
        Some(campo) => orc.iter().filter(|x| CORTES.iter().all(|c| p.get(*c).is_none_or(|v| s(x, c).trim() == v))).map(|x| s(x, campo).trim()).collect(),
        None => BTreeSet::new(),
    };
    let Some(campo) = campo.filter(|_| !valores.is_empty()) else {
        return Err(format!("TCE-CE: {met} não cabe numa página e não há como cortar mais ({p:?}); a paginação do SIM não é confiável"));
    };
    let mut out = vec![];
    for v in valores {
        out.extend(get(pagina, met, orc, &com(p, campo, v))?);
    }
    Ok(out)
}

/// Prova de que o download em fatias nao repetiu nem perdeu linha: uma passada paginada (instavel, mas so devolve linha de
/// verdade) tem de estar toda dentro dele. Nao fechou = Err, e nada daquele download e usado.
pub fn inteiro(pagina: Pagina, met: &str, linhas: &[Value], p: &Params) -> Result<(), String> {
    let tem: HashSet<String> = linhas.iter().map(txt).collect();
    if tem.len() != linhas.len() {
        return Err(format!("TCE-CE: {met} veio com {} linhas repetidas", linhas.len() - tem.len()));
    }
    if linhas.len() < PAGINA {
        return Ok(());
    }
    for i in (0..linhas.len() + PAGINA).step_by(PAGINA) {
        let pg = pagina(met, &com(p, "$start_index", &i.to_string()))?;
        if let Some(fora) = pg.iter().find(|x| !tem.contains(&txt(x))) {
            return Err(format!("TCE-CE: {met} tem linha que o download por órgão não trouxe (órgão {}); nada é usado até fechar", s(fora, "codigo_orgao")));
        }
        if pg.len() < PAGINA {
            break;
        }
    }
    Ok(())
}

fn repetidas(l: &[Value]) -> bool {
    l.iter().map(txt).collect::<HashSet<_>>().len() != l.len()
}

fn idade_ok(arq: &Path, agora_s: f64) -> bool {
    let mtime = arq.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs_f64());
    mtime.is_some_and(|t| agora_s - t <= VALIDADE_S)
}

/// Le do disco se o arquivo e novo e sem linha repetida; senao baixa em fatias, PROVA que esta inteiro e so entao grava.
pub fn cache(pagina: Pagina, d: &Path, nome: &str, met: &str, orc: &[Value], p: &Params, agora_s: f64) -> Result<Vec<Value>, String> {
    let arq = d.join(format!("{nome}.json"));
    if idade_ok(&arq, agora_s) {
        let l: Option<Vec<Value>> = std::fs::read_to_string(&arq).ok().and_then(|t| serde_json::from_str(&t).ok());
        if let Some(l) = l.filter(|l| !repetidas(l)) {
            return Ok(l); // arquivo da paginacao antiga (linhas repetidas) nao serve: baixa de novo
        }
    }
    let l = get(pagina, met, orc, p)?;
    inteiro(pagina, met, &l, p)?;
    std::fs::write(&arq, json!(l).to_string()).map_err(|e| e.to_string())?;
    Ok(l)
}

/// Ultimo mes de 2026 em que TODOS os orgaos do orcamento entregaram o balancete de despesa (e ha balancete de receita).
pub fn mes26(pagina: Pagina, cod: &str, orc26: &[Value]) -> Result<String, String> {
    let orgaos: BTreeSet<&str> = orc26.iter().filter(|x| f(x, "valor_total_fixado_projeto_atividade") != 0.0).map(|x| s(x, "codigo_orgao")).collect();
    for m in (1..=12).rev() {
        let rf = format!("2026{m:02}");
        let p = params(&[("codigo_municipio", cod), ("exercicio_orcamento", "202600"), ("data_referencia_doc", &rf)]);
        if pagina(RECEITA, &com(&p, "$count", "1"))?.is_empty() {
            continue;
        }
        let des = get(pagina, DESPESA, orc26, &p)?;
        let entregaram: BTreeSet<&str> = des.iter().map(|x| s(x, "codigo_orgao")).collect();
        if orgaos.is_subset(&entregaram) {
            return Ok(format!("{m:02}"));
        }
    }
    Err("TCE-CE: nenhum mês de 2026 com balancete de despesa de todos os órgãos do município no SIM".into())
}

fn referencia(a: &str, mes26: &str) -> String {
    format!("{a}{}", if a == "2026" { mes26 } else { "12" })
}

/// Baixa (ou reaproveita) orcamento, despesa e receita dos 3 anos em <pasta>/tce-ce. Devolve a pasta e o mes de corte de 2026.
pub fn baixar(pagina: Pagina, pasta: &Path, cod: &str, agora_s: f64) -> Result<(PathBuf, String), String> {
    let d = pasta.join("tce-ce");
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    let mut orc: BTreeMap<&str, Vec<Value>> = BTreeMap::new();
    for a in base::ANOS {
        let p = params(&[("codigo_municipio", cod), ("exercicio_orcamento", &format!("{a}00"))]);
        orc.insert(a, cache(pagina, &d, &format!("orcamento{a}"), "orcamento_despesas_projetos_atividades", &[], &p, agora_s)?);
    }
    let marca = d.join("mes26.txt");
    if !idade_ok(&marca, agora_s) {
        std::fs::write(&marca, mes26(pagina, cod, &orc["2026"])?).map_err(|e| e.to_string())?;
    }
    let m26 = std::fs::read_to_string(&marca).map_err(|e| e.to_string())?.trim().to_string();
    for a in base::ANOS {
        let rf = referencia(a, &m26);
        let p = params(&[("codigo_municipio", cod), ("exercicio_orcamento", &format!("{a}00")), ("data_referencia_doc", &rf)]);
        for (nome, met) in [("despesa", DESPESA), ("receita", RECEITA)] {
            cache(pagina, &d, &format!("{nome}{rf}"), met, &orc[a], &p, agora_s)?;
        }
    }
    Ok((d, m26))
}

fn arquivo(d: &Path, nome: &str) -> Result<Vec<Value>, String> {
    let t = std::fs::read_to_string(d.join(format!("{nome}.json"))).map_err(|e| format!("TCE-CE: {nome}.json: {e}"))?;
    serde_json::from_str(&t).map_err(|e| format!("TCE-CE: {nome}.json: {e}"))
}

fn chave(x: &Value) -> [String; 8] {
    ["codigo_orgao", "codigo_unidade_orcamentaria", "codigo_funcao", "codigo_subfuncao", "codigo_programa", "codigo_projeto_atividade", "numero_projeto_atividade", "numero_subprojeto_atividade"]
        .map(|k| if k == "codigo_unidade_orcamentaria" { s(x, k).trim().to_string() } else { s(x, k).to_string() })
}

/// Le o que `baixar` deixou em disco e devolve os 3 anos no formato da base (acoes somadas por projeto/atividade, receita por origem).
pub fn ler(d: &Path, mes26: &str) -> Result<Dados, String> {
    let mut dados = Dados::new();
    for a in base::ANOS {
        let rf = referencia(a, mes26);
        let nomes: BTreeMap<[String; 8], String> = arquivo(d, &format!("orcamento{a}"))?.iter().map(|x| (chave(x), s(x, "nome_projeto_atividade").trim().to_string())).collect();
        let (rec, des) = (arquivo(d, &format!("receita{rf}"))?, arquivo(d, &format!("despesa{rf}"))?);
        if rec.is_empty() || des.is_empty() {
            return Err(format!("TCE-CE: balancete de {} de {rf} vazio no SIM", if rec.is_empty() { "receita" } else { "despesa" }));
        }
        if repetidas(&des) || repetidas(&rec) {
            return Err(format!("TCE-CE: balancete de {rf} com linhas repetidas (download antigo, paginado); baixe de novo"));
        }
        let mut orig: Vec<(String, f64)> = vec![];
        for r in &rec {
            let c = s(r, "codigo_rubrica").trim();
            let nome = origem(c).ok_or_else(|| format!("TCE-CE: rubrica de receita sem origem conhecida: {c}"))?;
            let v = f(r, "valor_arrecadacao_ate_mes") - f(r, "valor_anulacoes_ate_mes");
            match orig.iter_mut().find(|(n, _)| n == nome) {
                Some(o) => o.1 += v,
                None => orig.push((nome.into(), v)),
            }
        }
        let (mut acoes, mut onde): (Vec<Acao>, BTreeMap<[String; 8], usize>) = Default::default();
        for r in &des {
            let k = chave(r);
            let nome = nomes.get(&k).ok_or_else(|| format!("TCE-CE: ação {k:?} do balancete {rf} sem nome no orçamento do SIM"))?;
            let i = *onde.entry(k.clone()).or_insert_with(|| {
                acoes.push(Acao { f: s(r, "codigo_funcao").into(), sub: s(r, "codigo_subfuncao").into(), cod: k.join("."), acao: nome.clone(), ..Default::default() });
                acoes.len() - 1
            });
            let x = &mut acoes[i];
            let fixado = f(r, "valor_fixado_orcamento_bal_despesa");
            x.orcado += fixado;
            x.atualizado += fixado + f(r, "valor_suplementado_ate_mes") - f(r, "valor_anulacoes_dotacao_ate_mes");
            x.empenhado += f(r, "valor_empenhado_ate_mes") - f(r, "valor_anulacoes_empenhos_ate_mes");
            x.liquidado += f(r, "valor_liquidado_ate_mes") - f(r, "valor_estornos_liquidacao_ate_mes");
        }
        orig.sort_by(|x, y| y.1.total_cmp(&x.1));
        let ano = Ano {
            arrecadado: orig.iter().map(|o| o.1).sum(),
            liquidado: acoes.iter().map(|x| x.liquidado).sum(),
            empenhado: acoes.iter().map(|x| x.empenhado).sum(),
            doc_despesa: format!("SIM TCE-CE · balancete de despesa {}/{a}", &rf[4..]),
            doc_receita: format!("SIM TCE-CE · balancete de receita {}/{a}", &rf[4..]),
            acoes,
            receita: orig,
        };
        dados.insert(a.into(), ano);
    }
    Ok(dados)
}

/// O que a esteira chama para uma cidade do Ceara: acha o municipio no SIM, baixa, prova, le e monta.
pub fn preparar(pagina: Pagina, setores: &dyn Fn(&Path) -> Result<(), String>, numero: &str, nome: &str, pasta: &Path, agora_s: f64) -> Result<Value, String> {
    let lista = get(pagina, "municipios", &[], &Params::new())?;
    let mun: Vec<&Value> = lista.iter().filter(|m| sem_acento(s(m, "nome_municipio")) == sem_acento(nome)).collect();
    let [m] = mun.as_slice() else { return Err(format!("TCE-CE: município '{nome}' não achado (ou repetido) na lista do SIM")) };
    let ibge = m["codigo_municipio_ibge"].as_i64().or_else(|| s(m, "codigo_municipio_ibge").trim().parse().ok()).ok_or("TCE-CE: município sem código do IBGE no SIM")?;
    let (d, m26) = baixar(pagina, pasta, s(m, "codigo_municipio"), agora_s)?;
    let dados = ler(&d, &m26)?;
    base::montar(pasta, numero, nome, "CE", ibge, &tribunal(), &dados, &m26, &base::Opcoes { festas_extra: Some(FESTAS_EXTRA), ..Default::default() }, setores)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn linhas_base() -> Vec<Value> {
        let (mut linhas, mut n) = (vec![], 0);
        for (org, uni, fun, qtd) in [("01", "101 ", "04", 300), ("02", "201 ", "10", 900), ("02", "202 ", "10", 700), ("02", "202 ", "12", 600)] {
            for _ in 0..qtd {
                n += 1;
                linhas.push(json!({ "codigo_orgao": org, "codigo_unidade_orcamentaria": uni, "codigo_funcao": fun, "codigo_subfuncao": "122", "id": n }));
            }
        }
        linhas
    }

    /// A API de mentira. Instavel: cada pedido vem numa ordem diferente (rodada a partir de um ponto que muda), entao as
    /// paginas de pedidos seguidos se sobrepoem e deixam linha de fora, como no tribunal.
    fn api(linhas: &[Value], instavel: bool) -> impl Fn(&str, &Params) -> Result<Vec<Value>, String> + '_ {
        let pedidos = Cell::new(0usize);
        move |_met: &str, p: &Params| {
            let mut r: Vec<Value> = linhas.iter().filter(|x| CORTES.iter().all(|c| p.get(*c).is_none_or(|v| s(x, c).trim() == v))).cloned().collect();
            if instavel && !r.is_empty() {
                pedidos.set(pedidos.get() + 1);
                let giro = (pedidos.get() * 337) % r.len();
                r.rotate_left(giro);
            }
            let i: usize = p.get("$start_index").map_or(0, |v| v.parse().unwrap());
            Ok(r.into_iter().skip(i).take(PAGINA).collect())
        }
    }

    fn ids(r: &[Value]) -> Vec<i64> {
        let mut v: Vec<i64> = r.iter().map(|x| x["id"].as_i64().unwrap()).collect();
        v.sort();
        v
    }

    #[test]
    fn baixa_tudo_em_fatias_sem_repetir_nem_perder() {
        let linhas = linhas_base();
        let p = params(&[("codigo_municipio", "057")]);
        // paginar na API instavel perde e repete linha (e por isso que o leitor nao pagina)
        let inst = api(&linhas, true);
        let mut paginado = vec![];
        for i in (0..3000).step_by(PAGINA) {
            paginado.extend(inst(DESPESA, &com(&p, "$start_index", &i.to_string())).unwrap());
        }
        assert!(ids(&paginado) != (1..=2500).collect::<Vec<_>>());
        let r = get(&inst, DESPESA, &linhas, &p).unwrap();
        assert_eq!(ids(&r), (1..=2500).collect::<Vec<_>>());
        inteiro(&api(&linhas, false), DESPESA, &r, &p).unwrap(); // fecha: nada faltou
        inteiro(&inst, DESPESA, &r, &p).unwrap(); // mesmo com a passada instavel: toda linha que ela mostra esta no download
    }

    #[test]
    fn download_repetido_ou_faltando_e_erro() {
        let linhas = linhas_base();
        let est = api(&linhas, false);
        let mut rep = linhas.clone();
        rep.extend_from_slice(&linhas[..3]);
        assert!(inteiro(&est, "m", &rep, &Params::new()).unwrap_err().contains("3 linhas repetidas"));
        // a passada paginada mostra linha que ficou de fora
        assert!(inteiro(&est, "m", &linhas[..linhas.len() - 50], &Params::new()).unwrap_err().contains("não trouxe"));
        assert!(inteiro(&est, "m", &linhas[50..], &Params::new()).unwrap_err().contains("órgão 01"));
        assert!(inteiro(&|_, _| Err("fora do ar".into()), "m", &linhas, &Params::new()).is_err()); // sem a prova, nada e usado
        inteiro(&|_, _| Err("nem chama".into()), "m", &linhas[..10], &Params::new()).unwrap(); // coube numa pagina: so confere repeticao
    }

    #[test]
    fn fatia_que_nao_cabe_nem_no_menor_corte_e_erro() {
        let linhas: Vec<Value> = (0..1500).map(|i| json!({ "codigo_orgao": "01", "codigo_unidade_orcamentaria": "101", "codigo_funcao": "04", "codigo_subfuncao": "122", "id": i })).collect();
        assert!(get(&api(&linhas, true), "m", &linhas, &Params::new()).unwrap_err().contains("não é confiável"));
        assert!(get(&api(&linhas, true), "m", &[], &Params::new()).unwrap_err().contains("não é confiável")); // sem orcamento para cortar
    }

    fn orc(org: &str, proj: &str, nome: &str) -> Value {
        json!({ "codigo_orgao": org, "codigo_unidade_orcamentaria": "01 ", "codigo_funcao": "13", "codigo_subfuncao": "392", "codigo_programa": "0001", "codigo_projeto_atividade": "2",
            "numero_projeto_atividade": proj, "numero_subprojeto_atividade": "0000", "nome_projeto_atividade": format!(" {nome} "), "valor_total_fixado_projeto_atividade": "100.0" })
    }

    fn des(org: &str, proj: &str, fonte: &str, fixado: f64, liq: f64) -> Value {
        let mut x = orc(org, proj, "");
        x["fonte"] = json!(fonte);
        x["valor_fixado_orcamento_bal_despesa"] = json!(fixado);
        x["valor_suplementado_ate_mes"] = json!(50.0);
        x["valor_anulacoes_dotacao_ate_mes"] = json!("10");
        x["valor_empenhado_ate_mes"] = json!(liq + 30.0);
        x["valor_anulacoes_empenhos_ate_mes"] = json!(5.0);
        x["valor_liquidado_ate_mes"] = json!(liq + 2.0);
        x["valor_estornos_liquidacao_ate_mes"] = json!(2.0);
        x
    }

    fn rec(rubrica: &str, v: f64, anul: f64) -> Value {
        json!({ "codigo_rubrica": rubrica, "valor_arrecadacao_ate_mes": v, "valor_anulacoes_ate_mes": anul })
    }

    fn grava(d: &Path, nome: &str, v: &Value) {
        std::fs::write(d.join(format!("{nome}.json")), v.to_string()).unwrap();
    }

    #[test]
    fn ler_soma_por_acao_e_recusa_o_que_nao_fecha() {
        let d = std::env::temp_dir().join(format!("braniac-link_tribunais_ce-ler-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        for (a, rf) in [("2024", "202412"), ("2025", "202512"), ("2026", "202607")] {
            grava(&d, &format!("orcamento{a}"), &json!([orc("01", "001", "FESTEJOS"), orc("02", "002", "ESCOLAS")]));
            grava(&d, &format!("despesa{rf}"), &json!([des("01", "001", "500", 100.0, 80.0), des("01", "001", "501", 20.0, 10.0), des("02", "002", "500", 300.0, 250.0)]));
            grava(&d, &format!("receita{rf}"), &json!([rec("1112", 200.0, 0.0), rec("1711 ", 500.0, 20.0), rec("1113", 50.0, 0.0), rec("9511", -40.0, 0.0), rec("7211", 5.0, 0.0)]));
        }
        let dados = ler(&d, "07").unwrap();
        let a = &dados["2026"];
        assert_eq!(a.acoes.len(), 2); // duas fontes da mesma acao viram uma linha
        let x = &a.acoes[0];
        assert_eq!((x.acao.as_str(), x.cod.as_str(), x.f.as_str(), x.sub.as_str()), ("FESTEJOS", "01.01.13.392.0001.2.001.0000", "13", "392"));
        assert_eq!((x.orcado, x.atualizado, x.empenhado, x.liquidado), (120.0, 200.0, 140.0, 90.0)); // liquidado menos estornos
        assert_eq!((a.liquidado, a.empenhado, a.arrecadado), (340.0, 415.0, 695.0));
        assert_eq!(a.receita[0], ("Transferências correntes".to_string(), 480.0)); // da maior para a menor, anulacoes descontadas
        assert_eq!(a.receita[1], ("Impostos, taxas e contribuições de melhoria".to_string(), 250.0));
        assert!(a.receita.contains(&("Deduções da receita (Fundeb)".to_string(), -40.0)) && a.receita.contains(&("Receitas intraorçamentárias".to_string(), 5.0)));
        assert_eq!((a.doc_despesa.as_str(), dados["2024"].doc_receita.as_str()), ("SIM TCE-CE · balancete de despesa 07/2026", "SIM TCE-CE · balancete de receita 12/2024"));

        grava(&d, "receita202512", &json!([rec("3311", 1.0, 0.0)]));
        assert!(ler(&d, "07").unwrap_err().contains("rubrica de receita sem origem conhecida: 3311"));
        grava(&d, "receita202512", &json!([]));
        assert!(ler(&d, "07").unwrap_err().contains("balancete de receita de 202512 vazio"));
        grava(&d, "receita202512", &json!([rec("1112", 1.0, 0.0), rec("1112", 1.0, 0.0)]));
        assert!(ler(&d, "07").unwrap_err().contains("linhas repetidas"));
        grava(&d, "receita202512", &json!([rec("1112", 1.0, 0.0)]));
        grava(&d, "despesa202412", &json!([des("03", "009", "500", 1.0, 1.0)]));
        assert!(ler(&d, "07").unwrap_err().contains("sem nome no orçamento"));
        std::fs::remove_file(d.join("despesa202412.json")).unwrap();
        assert!(ler(&d, "07").is_err()); // ano sem arquivo: nunca se completa
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn mes_de_corte_espera_todos_os_orgaos_e_cache_prova_antes_de_gravar() {
        let orc26 = vec![orc("01", "001", "A"), orc("02", "002", "B"), json!({ "codigo_orgao": "03", "valor_total_fixado_projeto_atividade": 0 })];
        // setembro: so o orgao 01 entregou; agosto: os dois (o 03 nao tem orcamento, nao conta); outubro em diante: sem receita
        let pagina = |met: &str, p: &Params| -> Result<Vec<Value>, String> {
            let mes = &p["data_referencia_doc"][4..];
            Ok(match (met, mes) {
                (RECEITA, "08" | "09") => vec![rec("1112", 1.0, 0.0)],
                (DESPESA, "09") => vec![des("01", "001", "500", 1.0, 1.0)],
                (DESPESA, "08") => vec![des("01", "001", "500", 1.0, 1.0), des("02", "002", "500", 1.0, 1.0)],
                _ => vec![],
            })
        };
        assert_eq!(mes26(&pagina, "057", &orc26).unwrap(), "08");
        assert!(mes26(&|_, _| Ok(vec![]), "057", &orc26).unwrap_err().contains("nenhum mês de 2026"));

        let d = std::env::temp_dir().join(format!("braniac-link_tribunais_ce-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let agora = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64();
        let linhas = linhas_base();
        // a API perde 50 linhas no download por fatias (some o orgao 01 quando se corta por orgao): a prova pega e NADA e gravado
        let furada = |met: &str, p: &Params| api(&linhas, false)(met, p).map(|r| if p.get("codigo_orgao").is_some_and(|o| o == "01") { r[..250].to_vec() } else { r });
        assert!(cache(&furada, &d, "despesa", DESPESA, &linhas, &Params::new(), agora).unwrap_err().contains("não trouxe"));
        assert!(!d.join("despesa.json").exists());
        let chamadas = Cell::new(0);
        let boa = |met: &str, p: &Params| { chamadas.set(chamadas.get() + 1); api(&linhas, false)(met, p) };
        assert_eq!(cache(&boa, &d, "despesa", DESPESA, &linhas, &Params::new(), agora).unwrap().len(), 2500);
        let antes = chamadas.get();
        assert_eq!(cache(&boa, &d, "despesa", DESPESA, &linhas, &Params::new(), agora).unwrap().len(), 2500);
        assert_eq!(chamadas.get(), antes); // arquivo novo e inteiro: nao consulta de novo
        cache(&boa, &d, "despesa", DESPESA, &linhas, &Params::new(), agora + VALIDADE_S + 60.0).unwrap();
        assert!(chamadas.get() > antes); // passou de 20 dias: baixa de novo
        let _ = std::fs::remove_dir_all(&d);
    }
}
