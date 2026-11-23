//! Molde de Sao Paulo, capital (link/tribunais/sp.py): Dados Abertos do IRIS do TCM-SP. Base de Receita (uma linha por rubrica
//! e mes: Arrecadado, deducoes do Fundeb negativas) e base de Dotacao (uma linha por dotacao: Orcado, Atualizado, Empenhado
//! liquido e Liquidado acumulados no ano). O mes de corte de 2026 e o ultimo mes da base de Receita. Linha com coluna faltando,
//! valor que nao e numero ou arquivo sem linhas = Err (pendencia, nunca video). So a capital: o resto de SP e do TCE-SP.
//! Entra como closure: `texto(tipo, ano)` devolve o CSV ja lido como cp850 (tipo = "receita": o CSV de dentro do zip; "dotacao").
//! Ficou de fora: baixar e abrir o zip. A mais que o Python: arquivo so com o cabecalho e erro (la virava um ano zerado).
#![allow(dead_code)]
use crate::link_tribunais_base::{self as base, maiuscula, Dados};
use crate::link_tribunais_util::{self as u, cel, Acoes};
use crate::texto_pt::sem_acento;
use serde_json::{json, Value};
use std::path::Path;

/// Festas de SP com outro nome (conferidas na base de Dotacao de 2024 a 2026).
pub const FESTAS_EXTRA: &str = "VIRADA CULTURAL|R[EÉ]VEILLON";
pub const IBGE: i64 = 3550308;
pub type Texto<'a> = &'a dyn Fn(&str, &str) -> Result<String, String>;

pub fn tribunal() -> Value {
    json!({ "sigla": "TCM-SP", "fala": "Tribunal de Contas do Município", "uf": "SP", "estado": "São Paulo", "doc_despesa": "IRIS DOTAÇÃO", "doc_receita": "IRIS RECEITA",
        "tribunal_estado": "TCM-SP (Tribunal de Contas do Município de São Paulo)", "site_tribunal": "iris.tcm.sp.gov.br",
        "fonte_md": "Tudo do IRIS do TCM-SP (Tribunal de Contas do Município de São Paulo), Dados Abertos em iris.tcm.sp.gov.br/dadosAbertos.html: base de Receita (csvReceita<ano>.zip, coluna Arrecadado, deduções do Fundeb já descontadas) e base de Dotação (Dotacao<ano>.csv, colunas Orçado, Atualizado, EmpenhadoLiquido e Liquidado). \"Gastou\" = LIQUIDADO, todas as empresas da Prefeitura (consolidado). \"Previa\" = Orçado (dotação inicial da lei do orçamento).",
        "setores_md": "Setores = soma do liquidado das ações (projeto/atividade) da base de Dotação do IRIS, pela função; festas e eventos separados da cultura; segurança pública fora.",
        "pendencias_md": [], "orgaos": "Prefeitura (administração direta, autarquias e fundos)",
        "legenda_docs": ["• Receita: IRIS do TCM-SP, Dados Abertos, base de Receita (coluna Arrecadado) de 2024, 2025 e 2026", "• Despesa, setores e festas: IRIS do TCM-SP, base de Dotação (colunas Orçado e Liquidado), por projeto/atividade", "• iris.tcm.sp.gov.br/dadosAbertos.html"] })
}

/// Numero com ponto decimal; vazio = 0; o que nao e numero e erro.
fn f(s: &str) -> Result<f64, String> {
    if s.is_empty() { Ok(0.0) } else { s.trim().parse().map_err(|_| format!("valor que não é número: {s:?}")) }
}

/// As linhas do arquivo, sem o cabecalho.
fn linhas(texto: Texto, tipo: &str, a: &str) -> Result<Vec<Vec<String>>, String> {
    let mut l = u::linhas_csv(&texto(tipo, a)?, ',');
    if l.len() < 2 {
        return Err(format!("IRIS TCM-SP: base de {tipo} de {a} sem linhas"));
    }
    l.remove(0);
    Ok(l)
}

/// Le os 3 anos. Devolve os dados no formato da base e o mes de corte de 2026.
pub fn ler(texto: Texto) -> Result<(Dados, String), String> {
    let (mut dados, mut mes26) = (Dados::new(), String::new());
    for a in base::ANOS {
        let (mut orig, mut ultimo) = (vec![], 0);
        for r in linhas(texto, "receita", a)? {
            ultimo = ultimo.max(u::int(cel(&r, 1)?, "mês da receita")?);
            let nome = if cel(&r, 15)?.starts_with("09") { "Deduções da receita (Fundeb)".to_string() } else { maiuscula(cel(&r, 4)?.split_once(" - ").map_or(cel(&r, 4)?, |x| x.1)) };
            u::soma(&mut orig, &nome, f(cel(&r, 18)?)?);
        }
        if a == "2026" {
            mes26 = format!("{ultimo:02}");
        }
        let mut acoes = Acoes::default();
        for r in linhas(texto, "dotacao", a)? {
            let k = [cel(&r, 3)?, cel(&r, 5)?, cel(&r, 7)?, cel(&r, 9)?, cel(&r, 11)?, cel(&r, 13)?].join(".");
            let x = acoes.entra(&k);
            // o TCM-SP tambem e funcao 01 Legislativa: no video a "Camara" e so a Camara; o tribunal vai a "Outros"
            x.f = if cel(&r, 7)? == "01" && cel(&r, 4)?.to_uppercase().contains("TRIBUNAL DE CONTAS") { "0".into() } else { cel(&r, 7)?.into() };
            (x.sub, x.acao, x.cod) = (cel(&r, 9)?.into(), cel(&r, 14)?.into(), k);
            x.orcado += f(cel(&r, 29)?)?;
            x.atualizado += f(cel(&r, 30)?)?;
            x.empenhado += f(cel(&r, 33)?)?;
            x.liquidado += f(cel(&r, 34)?)?;
        }
        dados.insert(a.into(), u::ano(acoes.lista, orig, format!("IRIS TCM-SP · Dotacao{a}.csv"), format!("IRIS TCM-SP · csvReceita{a}.zip")));
    }
    Ok((dados, mes26))
}

/// O que a esteira chama. So a capital.
pub fn preparar(texto: Texto, setores: &dyn Fn(&Path) -> Result<(), String>, numero: &str, nome: &str, pasta: &Path) -> Result<Value, String> {
    if sem_acento(nome).to_uppercase() != "SAO PAULO" {
        return Err("o molde do TCM-SP é só da capital; as outras cidades de SP são do TCE-SP".into());
    }
    let (dados, mes26) = ler(texto)?;
    base::montar(pasta, numero, nome, "SP", IBGE, &tribunal(), &dados, &mes26, &base::Opcoes { festas_extra: Some(FESTAS_EXTRA), ..Default::default() }, setores)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linha(n: usize, campos: &[(usize, &str)]) -> String {
        let mut c = vec![String::new(); n];
        for (i, v) in campos {
            c[*i] = format!("\"{v}\"");
        }
        c.join(",")
    }

    fn dot(org: &str, nome_org: &str, f: &str, pa: &str, acao: &str, orc: &str, liq: &str) -> String {
        linha(35, &[(3, org), (4, nome_org), (5, "10"), (7, f), (9, "392"), (11, "3001"), (13, pa), (14, acao), (29, orc), (30, "1500.5"), (33, "900"), (34, liq)])
    }

    fn fonte(tipo: &str, ano: &str) -> Result<String, String> {
        Ok(if tipo == "receita" {
            let ate = if ano == "2026" { 9 } else { 12 };
            let mut t = "cab".to_string();
            for m in 1..=ate {
                t += &format!("\n{}\n{}", linha(19, &[(1, &m.to_string()), (4, "1 - IMPOSTOS, TAXAS E CONTRIBUICOES"), (15, "01"), (18, "1000.25")]), linha(19, &[(1, &m.to_string()), (4, "7 - TRANSFERENCIAS"), (15, "09"), (18, "-200")]));
            }
            t
        } else {
            format!("cab\n{}\n{}\n{}\n{}", dot("25", "Secretaria de Cultura", "13", "6001", "VIRADA CULTURAL, EDIÇÃO", "1000", "700.5"), dot("25", "Secretaria de Cultura", "13", "6001", "VIRADA CULTURAL, EDIÇÃO", "", "99.5"), dot("10", "Tribunal de Contas do Município", "01", "2100", "FISCALIZAÇÃO", "5", "5"), dot("09", "Câmara Municipal", "01", "2200", "PROCESSO LEGISLATIVO", "7", "7"))
        })
    }

    #[test]
    fn le_receita_e_dotacao_do_iris() {
        let (dados, m) = ler(&fonte).unwrap();
        assert_eq!(m, "09");
        let a = &dados["2026"];
        let x = &a.acoes[0];
        assert_eq!((x.acao.as_str(), x.cod.as_str(), x.f.as_str(), x.sub.as_str()), ("VIRADA CULTURAL, EDIÇÃO", "25.10.13.392.3001.6001", "13", "392"));
        assert_eq!((x.orcado, x.atualizado, x.empenhado, x.liquidado), (1000.0, 3001.0, 1800.0, 800.0));
        assert_eq!((a.acoes[1].f.as_str(), a.acoes[2].f.as_str(), a.acoes.len()), ("0", "01", 3)); // o tribunal sai da funcao 01
        assert_eq!(a.receita, [("Impostos, taxas e contribuicoes".to_string(), 9002.25), ("Deduções da receita (Fundeb)".to_string(), -1800.0)]);
        assert_eq!((a.liquidado, a.doc_despesa.as_str(), dados["2025"].arrecadado), (812.0, "IRIS TCM-SP · Dotacao2026.csv", 12.0 * 800.25));
    }

    #[test]
    fn recusa_o_que_falta() {
        let ruim = |de: &'static str, para: &'static str| move |tipo: &str, ano: &str| fonte(tipo, ano).map(|t| t.replace(de, para));
        assert!(ler(&ruim("700.5", "700,5")).unwrap_err().contains("não é número"));
        assert!(ler(&ruim(",\"99.5\"", "")).unwrap_err().contains("colunas"));
        assert!(ler(&ruim("\"1\",", "\"jan\",")).unwrap_err().contains("mês da receita"));
        let vazio = |tipo: &str, ano: &str| if tipo == "dotacao" && ano == "2025" { Ok("cab\n".to_string()) } else { fonte(tipo, ano) };
        assert!(ler(&vazio).unwrap_err().contains("base de dotacao de 2025 sem linhas"));
        assert!(ler(&|_, _| Err("404".into())).is_err());
        let p = std::env::temp_dir().join(format!("braniac-link_tribunais_sp-{}", std::process::id()));
        assert!(preparar(&fonte, &|_| Ok(()), "1", "Campinas", &p).unwrap_err().contains("só da capital"));
    }
}
