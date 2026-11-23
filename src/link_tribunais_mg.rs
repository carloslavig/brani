//! Molde de Minas Gerais (link/tribunais/mg.py): arquivos DESPESA e RECEITA do SICOM, dos Dados Abertos do TCE-MG.
//! Aqui so o parse do CSV ja baixado: 2026 vale ate o ultimo mes em que TODOS os orgaos entregaram despesa e ha receita (sem
//! buraco antes dele); ano fechado sem os 12 meses, arquivo vazio ou de outro municipio = Err (pendencia, nunca video).
//! Entra como closure: `texto(tipo, ano)` devolve o CSV (tipo = "despesa" | "receita"); baixar pelo navegador e abrir o zip
//! sao de quem chama. Ficou de fora: o passo a passo da tela (`baixar`).
#![allow(dead_code)]
use crate::link_tribunais_base::{self as base, col, csv, maiuscula, valor, Acao, Ano, Dados, Linha};
use crate::texto_pt::sem_acento;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Festas de MG com outro nome.
pub const FESTAS_EXTRA: &str = "ARRAIAL|R[EÉ]VEILLON";
// shortcut: so a capital tem codigo aqui; outra cidade de MG -> acrescentar o IBGE dela
pub const IBGE: [(&str, i64); 1] = [("belo horizonte", 3106200)];
pub type Texto<'a> = &'a dyn Fn(&str, &str) -> Result<String, String>;

pub fn tribunal() -> Value {
    json!({ "sigla": "TCE-MG", "fala": "Tribunal de Contas do Estado", "uf": "MG", "estado": "Minas Gerais", "doc_despesa": "SICOM DESPESA", "doc_receita": "SICOM RECEITA",
        "tribunal_estado": "TCE-MG (Tribunal de Contas do Estado de Minas Gerais)", "site_tribunal": "dadosabertos.tce.mg.gov.br",
        "fonte_md": "Tudo do SICOM do TCE-MG (Tribunal de Contas do Estado de Minas Gerais), a mesma base do Fiscalizando com o TCE e do Panorama Municipal, baixada nos Dados Abertos do tribunal (dadosabertos.tce.mg.gov.br > Jurisdicionado): arquivo RECEITA (receita.csv, coluna realizado, deduções do Fundeb já descontadas) e arquivo DESPESA (despesa.csv, colunas previsto, acréscimo, dedução, empenhado e liquidado). \"Gastou\" = LIQUIDADO, todos os órgãos do município. \"Previa\" = previsto (dotação inicial da lei do orçamento).",
        "setores_md": "Setores = soma do liquidado das ações e subações do arquivo DESPESA do SICOM (TCE-MG), pela função; festas e eventos separados da cultura; segurança pública fora.",
        "pendencias_md": [], "orgaos": "Câmara, Prefeitura, autarquias, fundações e fundos do município",
        "legenda_docs": ["• Receita: SICOM do TCE-MG, Dados Abertos, arquivo RECEITA (coluna realizado) de 2024, 2025 e 2026", "• Despesa, setores e festas: SICOM do TCE-MG, arquivo DESPESA (colunas previsto e liquidado), por ação e subação", "• dadosabertos.tce.mg.gov.br"] })
}

/// "10 - Saude" -> "10".
fn cod(s: &str) -> &str {
    s.split(" - ").next().unwrap_or("").trim()
}

/// "10 - Saude" -> "Saude".
fn txt(s: &str) -> &str {
    s.split_once(" - ").map_or(s, |x| x.1).trim()
}

fn mes(r: &Linha) -> Result<u32, String> {
    col(r, "num_mesexercicio")?.trim().parse().map_err(|_| "num_mesexercicio que não é número".to_string())
}

/// Le os 3 anos. Devolve os dados no formato da base e o mes de corte de 2026 ("07").
pub fn ler(texto: Texto, ibge: i64) -> Result<(Dados, String), String> {
    let (mut desp, mut rec): (BTreeMap<&str, Vec<Linha>>, BTreeMap<&str, Vec<Linha>>) = Default::default();
    for a in base::ANOS {
        let d = csv(&texto("despesa", a)?, ';');
        let mut outro = false;
        for x in &d {
            outro |= col(x, "cod_municipio")? != ibge.to_string();
        }
        if d.is_empty() || outro {
            return Err(format!("despesa {a}: arquivo vazio ou de outro município (esperado IBGE {ibge})"));
        }
        desp.insert(a, d);
        rec.insert(a, csv(&texto("receita", a)?, ';'));
    }
    // 2026: ultimo mes em que TODOS os orgaos do ano entregaram despesa e ha receita (sem buraco antes dele)
    let mut org: BTreeMap<u32, BTreeSet<&str>> = BTreeMap::new();
    for x in &desp["2026"] {
        org.entry(mes(x)?).or_default().insert(col(x, "cod_orgao")?);
    }
    let todos: BTreeSet<&str> = org.values().flatten().copied().collect();
    let mr: BTreeSet<u32> = rec["2026"].iter().map(mes).collect::<Result<_, _>>()?;
    let mut m = 0;
    while org.get(&(m + 1)) == Some(&todos) && mr.contains(&(m + 1)) {
        m += 1;
    }
    if m == 0 {
        return Err("2026: nenhum mês com todos os órgãos no SICOM do TCE-MG".into());
    }
    let bairro = Regex::new(r"\bSAO JOAO BATISTA\b").expect("regex fixa");
    let mut dados = Dados::new();
    for a in base::ANOS {
        let lim = if a == "2026" { m } else { 12 };
        let meses: BTreeSet<u32> = desp[a].iter().map(mes).collect::<Result<_, _>>()?;
        if a != "2026" && meses != (1..=12).collect() {
            return Err(format!("despesa {a}: SICOM sem os 12 meses ({meses:?})"));
        }
        let (mut acoes, mut onde): (Vec<Acao>, BTreeMap<[String; 7], usize>) = Default::default();
        for r in &desp[a] {
            if mes(r)? > lim {
                continue;
            }
            let sub = Some(col(r, "dsc_subacao")?).filter(|s| !["", "-", "-9"].contains(&cod(s)));
            let k = [col(r, "cod_orgao")?, col(r, "cod_unidade")?, cod(col(r, "dsc_funcao")?), cod(col(r, "dsc_subfuncao")?), cod(col(r, "dsc_programa")?), cod(col(r, "dsc_acao")?), sub.map_or("", cod)].map(String::from);
            let nome = format!("{}{}", txt(col(r, "dsc_acao")?), sub.map(|s| format!(" — {}", txt(s))).unwrap_or_default());
            // bairro (emenda de saude), nao e festa junina
            let nome = bairro.replace_all(&nome, "S. JOAO BATISTA").into_owned();
            let i = *onde.entry(k.clone()).or_insert_with(|| {
                acoes.push(Acao { f: k[2].clone(), sub: k[3].clone(), cod: k.join("."), ..Default::default() });
                acoes.len() - 1
            });
            let x = &mut acoes[i];
            x.acao = nome;
            let previsto = valor(r, "vlr_previsto")?;
            x.orcado += previsto;
            x.atualizado += previsto + valor(r, "vlr_acrescimo")? - valor(r, "vlr_deducao")?;
            x.empenhado += valor(r, "vlr_empenhado")?;
            x.liquidado += valor(r, "vlr_liquidado")?;
        }
        let mut orig: Vec<(String, f64)> = vec![];
        for r in &rec[a] {
            if mes(r)? > lim {
                continue;
            }
            let ded = !["", "-"].contains(&col(r, "dsc_tipodeducao")?.trim());
            let nome = if ded { "Deduções da receita (Fundeb e outras)".to_string() } else { maiuscula(txt(col(r, "dsc_origem")?)) };
            let v = valor(r, "vlr_realizadoateperiodo")?;
            match orig.iter_mut().find(|(n, _)| *n == nome) {
                Some(o) => o.1 += v,
                None => orig.push((nome, v)),
            }
        }
        orig.sort_by(|x, y| y.1.total_cmp(&x.1));
        acoes.retain(|x| x.orcado != 0.0 || x.atualizado != 0.0 || x.empenhado != 0.0 || x.liquidado != 0.0);
        dados.insert(
            a.into(),
            Ano {
                arrecadado: orig.iter().map(|o| o.1).sum(),
                liquidado: acoes.iter().map(|x| x.liquidado).sum(),
                empenhado: acoes.iter().map(|x| x.empenhado).sum(),
                doc_despesa: format!("SICOM TCE-MG · Dados Abertos · {a}.{ibge}.despesa.despesa.csv"),
                doc_receita: format!("SICOM TCE-MG · Dados Abertos · {a}.{ibge}.receita.receita.csv"),
                acoes,
                receita: orig,
            },
        );
    }
    Ok((dados, format!("{m:02}")))
}

/// O que a esteira chama para uma cidade de MG. Cidade sem codigo do IBGE aqui e recusada.
pub fn preparar(texto: Texto, setores: &dyn Fn(&Path) -> Result<(), String>, numero: &str, nome: &str, pasta: &Path) -> Result<Value, String> {
    let ibge = IBGE.iter().find(|(n, _)| *n == sem_acento(nome)).map(|x| x.1).ok_or_else(|| format!("molde do TCE-MG: falta o código IBGE de {nome}"))?;
    let (dados, mes26) = ler(texto, ibge)?;
    base::montar(pasta, numero, nome, "MG", ibge, &tribunal(), &dados, &mes26, &base::Opcoes { festas_extra: Some(FESTAS_EXTRA), ..Default::default() }, setores)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAB_D: &str = "cod_municipio;num_mesexercicio;cod_orgao;cod_unidade;dsc_funcao;dsc_subfuncao;dsc_programa;dsc_acao;dsc_subacao;vlr_previsto;vlr_acrescimo;vlr_deducao;vlr_empenhado;vlr_liquidado";
    const CAB_R: &str = "num_mesexercicio;dsc_origem;dsc_tipodeducao;vlr_realizadoateperiodo";

    /// Despesa: dois orgaos por mes ate `ate`; o orgao 02 so ate `ate_02`.
    fn despesa(ate: u32, ate_02: u32) -> String {
        let mut t = CAB_D.to_string();
        for m in 1..=ate {
            t += &format!("\n3106200;{m};01;001;13 - Cultura;392 - Difusao;0001 - Prog;2001 - ARRAIAL DA CIDADE;-9 - ;{};10;5;30;20", if m == 1 { "1200" } else { "0" });
            t += &format!("\n3106200;{m};01;001;10 - Saude;301 - Atencao;0002 - Prog;2002 - EMENDA;7 - UPA SAO JOAO BATISTA;0;0;0;8;7");
            t += "\n3106200;1;01;001;04 - Adm;122 - Adm;0003 - Prog;2003 - ZERADA;-;0;0;0;0;0";
            if m <= ate_02 {
                t += &format!("\n3106200;{m};02;001;12 - Educacao;361 - Ensino;0004 - Prog;2004 - ESCOLAS;- ;0;0;0;100;100");
            }
        }
        t
    }

    fn receita(ate: u32) -> String {
        let mut t = CAB_R.to_string();
        for m in 1..=ate {
            t += &format!("\n{m};1 - IMPOSTOS, TAXAS;-;300\n{m};7 - TRANSFERENCIAS CORRENTES;-;900\n{m};7 - TRANSFERENCIAS CORRENTES;Fundeb;-100");
        }
        t
    }

    fn fonte(d26: String, r26: String) -> impl Fn(&str, &str) -> Result<String, String> {
        move |tipo: &str, ano: &str| Ok(match (tipo, ano) {
            ("despesa", "2026") => d26.clone(),
            ("receita", "2026") => r26.clone(),
            ("despesa", _) => despesa(12, 12),
            _ => receita(12),
        })
    }

    #[test]
    fn le_ate_o_mes_que_todos_os_orgaos_entregaram() {
        // 2026: orgao 02 so entregou ate junho; a receita vai ate agosto -> corte em junho
        let (dados, m) = ler(&fonte(despesa(8, 6), receita(8)), 3106200).unwrap();
        assert_eq!(m, "06");
        let a = &dados["2026"];
        assert_eq!(a.acoes.len(), 3); // a zerada sai; as fontes/meses da mesma acao somam
        let x = &a.acoes[0];
        assert_eq!((x.acao.as_str(), x.cod.as_str(), x.f.as_str(), x.sub.as_str()), ("ARRAIAL DA CIDADE", "01.001.13.392.0001.2001.", "13", "392"));
        assert_eq!((x.orcado, x.atualizado, x.empenhado, x.liquidado), (1200.0, 1230.0, 180.0, 120.0));
        assert_eq!(a.acoes[1].acao, "EMENDA — UPA S. JOAO BATISTA"); // bairro nao vira festa junina
        assert!(!base::eh_festa(&a.acoes[1].acao, None) && a.acoes[1].cod.ends_with(".2002.7"));
        assert_eq!((a.liquidado, a.empenhado, a.arrecadado), (120.0 + 42.0 + 600.0, 180.0 + 48.0 + 600.0, 6.0 * 1100.0));
        assert_eq!(a.receita, [("Transferencias correntes".to_string(), 5400.0), ("Impostos, taxas".to_string(), 1800.0), ("Deduções da receita (Fundeb e outras)".to_string(), -600.0)]);
        assert_eq!(dados["2024"].liquidado, 12.0 * 127.0);
        assert_eq!(a.doc_despesa, "SICOM TCE-MG · Dados Abertos · 2026.3106200.despesa.despesa.csv");
        // a receita so vai ate maio: o corte desce para maio
        assert_eq!(ler(&fonte(despesa(8, 8), receita(5)), 3106200).unwrap().1, "05");
    }

    #[test]
    fn recusa_levantamento_pela_metade() {
        assert!(ler(&fonte(despesa(8, 8), receita(8)), 3100000).unwrap_err().contains("de outro município"));
        assert!(ler(&fonte(CAB_D.into(), receita(8)), 3106200).unwrap_err().contains("arquivo vazio"));
        // janeiro de 2026 sem um dos orgaos: nenhum mes vale
        let buraco = despesa(8, 8).lines().filter(|l| !l.starts_with("3106200;1;02;")).collect::<Vec<_>>().join("\n");
        assert!(ler(&fonte(buraco, receita(8)), 3106200).unwrap_err().contains("nenhum mês com todos os órgãos"));
        assert!(ler(&fonte(despesa(8, 8), CAB_R.into()), 3106200).unwrap_err().contains("nenhum mês"));
        // ano fechado sem os 12 meses
        let curto = |tipo: &str, ano: &str| Ok(if tipo == "despesa" { despesa(if ano == "2025" { 11 } else { 12 }, 12) } else { receita(12) });
        assert!(ler(&curto, 3106200).unwrap_err().contains("despesa 2025: SICOM sem os 12 meses"));
        assert!(ler(&|_, _| Err("download falhou".into()), 3106200).is_err());
        // coluna que falta e valor que nao e numero nunca viram zero
        assert!(ler(&fonte(despesa(8, 8).replace("vlr_liquidado", "outra"), receita(8)), 3106200).unwrap_err().contains("sem a coluna vlr_liquidado"));
        assert!(ler(&fonte(despesa(8, 8).replace(";100;100", ";100;cem"), receita(8)), 3106200).unwrap_err().contains("não é número"));
        let p = std::env::temp_dir().join(format!("braniac-link_tribunais_mg-{}", std::process::id()));
        assert!(preparar(&fonte(despesa(8, 8), receita(8)), &|_| Ok(()), "1", "Uberaba", &p).unwrap_err().contains("falta o código IBGE"));
        assert!(!p.exists());
    }
}
