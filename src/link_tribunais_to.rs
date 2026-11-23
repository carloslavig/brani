//! Molde do Tocantins (link/tribunais/to.py), Palmas: planilhas do SICAP no Portal do Cidadao do TCE-TO (Balancete Despesa e
//! Balancete Receita por unidade gestora e remessa). 2024 e 2025 = remessa Consolidada da Prefeitura (municipio inteiro);
//! 2026 = soma das unidades na ultima remessa mensal que TODAS as ativas entregaram. Ano fechado sem a Consolidada, linha de
//! planilha cortada, valor que nao e numero ou conta de receita sem origem conhecida = Err (pendencia, nunca video).
//! Entra como closure: `busca(rota, unidade, ano, remessa)` devolve o texto (rota = "getUnidades" | "getRemessas": JSON do portal;
//! "balanceteDespesa" | "balanceteReceita": a planilha HTML ja lida como latin-1). Uma consulta por vez, com pausa, e de quem chama.
//! Ficou de fora: o cache em disco das planilhas e o remessas.json de conferencia.
#![allow(dead_code)]
use crate::link_tribunais_base::{self as base, Dados};
use crate::link_tribunais_util::{self as u, cel, ini, n_br, Acoes};
use crate::texto_pt::sem_acento;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

pub const IBGE: i64 = 1721000;
/// PREFEITURA MUNICIPAL (unidade principal, manda a Consolidada).
pub const PRINCIPAL: &str = "24851511000185";
/// Na Consolidada a Camara e o orgao 10 (funcao 01 = so ela).
pub const ORGAO_CAMARA: &str = "10";
pub const FESTAS_EXTRA: &str = "R[EÉ]VEILLON|EVENTOS DE FLUXOS TUR[IÍ]STICOS|PROMO[ÇC][ÃA]O DAS FEIRAS CULTURAIS|REALIZA[ÇC][ÃA]O (E APOIO A |DE )EVENTOS CULTURAIS";
pub type Busca<'a> = &'a dyn Fn(&str, &str, &str, i64) -> Result<String, String>;
/// ano -> [(cnpj, nome, ultima remessa enviada)].
pub type Remessas = BTreeMap<String, Vec<(String, String, i64)>>;

fn consolidada(ano: &str) -> Option<i64> {
    match ano {
        "2024" => Some(8),
        "2025" => Some(14),
        _ => None,
    }
}

pub fn tribunal() -> Value {
    json!({ "sigla": "TCE-TO", "fala": "Tribunal de Contas do Estado", "uf": "TO", "estado": "Tocantins", "doc_despesa": "SICAP BALANCETE DE DESPESA", "doc_receita": "SICAP BALANCETE DE RECEITA",
        "tribunal_estado": "TCE-TO (Tribunal de Contas do Estado do Tocantins)", "site_tribunal": "portaldocidadao.tceto.tc.br",
        "fonte_md": "Tudo do TCE-TO (Tribunal de Contas do Estado do Tocantins), Portal do Cidadão > Informações dos Municípios > Palmas > Dados Abertos (planilhas do SICAP em www.tceto.tc.br/sicap/contabilx/relatorios/planilhas): Balancete Despesa (Dotação Inicial, Dotação Atualizada, Empenhado e Liquidado por ação) e Balancete Receita (realizada acumulada, deduções para o Fundeb descontadas). 2024 e 2025: remessa Consolidada da Prefeitura (município inteiro). 2026: soma das unidades gestoras na última remessa que todas entregaram. \"Gastou\" = LIQUIDADO. \"Previa\" = Dotação Inicial.",
        "setores_md": "Setores = soma do liquidado das ações do Balancete Despesa do SICAP (TCE-TO), pela função; festas e eventos separados da cultura; segurança pública fora.",
        "pendencias_md": [], "orgaos": "Município inteiro (Prefeitura, Câmara, autarquias, fundações e fundos)",
        "legenda_docs": ["• Receita: TCE-TO, Portal do Cidadão, Balancete Receita do SICAP de 2024, 2025 e 2026 (líquido do Fundeb)", "• Despesa, setores e festas: TCE-TO, Balancete Despesa do SICAP (Dotação Inicial e Liquidado), por ação", "• portaldocidadao.tceto.tc.br"] })
}

/// Texto da celula: parte dos nomes vem em UTF-8 lido como latin-1 ("ManutenÃ§Ã£o"); se desfaz, desfaz.
fn celula(s: &str) -> String {
    let s = u::sem_tags(s);
    let s = s.trim_end_matches('\'');
    let bytes: Option<Vec<u8>> = s.chars().map(|c| u8::try_from(c as u32).ok()).collect();
    bytes.and_then(|b| String::from_utf8(b).ok()).unwrap_or_else(|| s.to_string())
}

/// Todas as unidades gestoras do municipio e a ultima remessa que cada uma enviou em cada ano.
pub fn remessas(busca: Busca) -> Result<Remessas, String> {
    let ugs = u::json(&busca("getUnidades", "", "", 0)?, "getUnidades")?;
    let mut out = Remessas::new();
    for a in base::ANOS {
        let mut l = vec![];
        for ug in u::lista(&ugs, "getUnidades")? {
            let (id, mut ult) = (u::txt(ug, "id")?, None);
            let r = u::json(&busca("getRemessas", id, a, 0)?, "getRemessas")?;
            for x in u::lista(&r, "getRemessas")? {
                if u::txt(x, "envio")? == "1" {
                    ult = ult.max(Some(u::num(x, "numero")? as i64));
                }
            }
            if let Some(r) = ult {
                l.push((id.to_string(), u::txt(ug, "nome")?.to_string(), r));
            }
        }
        out.insert(a.into(), l);
    }
    Ok(out)
}

/// Ultimo mes de 2026 que todas as unidades ATIVAS entregaram. Ativa = mandou remessa a partir de junho; as que pararam antes
/// (extintas na reforma de 2026) entram com a ultima remessa delas.
pub fn corte26(lista: &[(String, String, i64)]) -> Result<i64, String> {
    lista.iter().map(|x| x.2).filter(|r| (6..=12).contains(r)).min().ok_or_else(|| "TCE-TO: nenhuma unidade gestora com remessa de 2026 a partir de junho".to_string())
}

/// Le os 3 anos. Devolve os dados no formato da base e o mes de corte de 2026.
pub fn ler(busca: Busca) -> Result<(Dados, String), String> {
    let rem = remessas(busca)?;
    let vazio = vec![];
    let mes = corte26(rem.get("2026").unwrap_or(&vazio))?;
    let mut dados = Dados::new();
    for a in base::ANOS {
        let ra = rem.get(a).unwrap_or(&vazio);
        let fontes: Vec<(&str, i64)> = match consolidada(a) {
            Some(c) => {
                if !ra.iter().any(|x| x.0 == PRINCIPAL && x.2 >= c) {
                    return Err(format!("TCE-TO: {a} sem remessa Consolidada da Prefeitura"));
                }
                vec![(PRINCIPAL, c)]
            }
            None => ra.iter().filter(|x| x.2 >= 1).map(|x| (x.0.as_str(), x.2.min(mes))).collect(),
        };
        let (mut acoes, mut orig) = (Acoes::default(), vec![]);
        for (ug, r) in &fontes {
            // a planilha da remessa N traz as remessas 1..N: empenhado/liquidado sao DO MES (soma-se 1..N); dotacao so da linha da remessa N
            for x in u::tabela(&busca("balanceteDespesa", ug, a, *r)?, &celula) {
                let rx = cel(&x, 2)?;
                if !rx.is_empty() && u::int(rx, "remessa")? > *r {
                    continue;
                }
                // sem a unidade orcamentaria: em 2026 ela vem vazia em parte das linhas
                let k = [cel(&x, 3)?, cel(&x, 5)?, cel(&x, 6)?, cel(&x, 7)?, cel(&x, 9)?].join(".");
                let y = acoes.entra(&k);
                y.f = if cel(&x, 5)? == "01" && cel(&x, 3)? != ORGAO_CAMARA { "0".into() } else { cel(&x, 5)?.into() };
                (y.sub, y.acao, y.cod) = (cel(&x, 6)?.into(), cel(&x, 10)?.into(), k);
                y.empenhado += n_br(cel(&x, 34)?)?;
                y.liquidado += n_br(cel(&x, 35)?)?;
                if rx == r.to_string() {
                    y.orcado += n_br(cel(&x, 13)?)?;
                    y.atualizado += n_br(cel(&x, 33)?)?;
                }
            }
            for x in u::tabela(&busca("balanceteReceita", ug, a, *r)?, &celula) {
                // so contas analiticas (as sinteticas repetem a soma) e so a remessa N (realizada ja acumulada)
                if cel(&x, 14)? != "A" || cel(&x, 2)? != r.to_string() {
                    continue;
                }
                let (op, c, v) = (ini(cel(&x, 7)?, 3), cel(&x, 5)?, n_br(cel(&x, 12)?)?);
                match op {
                    "105" => u::soma(&mut orig, "Deduções da receita (Fundeb)", -v),
                    "101" => {
                        let nome = if c.starts_with(['7', '8']) { Some(u::INTRA) } else { u::origem(ini(c, 2)) };
                        u::soma(&mut orig, nome.ok_or_else(|| format!("TCE-TO: conta de receita sem origem conhecida: {c} {}", x.get(13).map_or("", String::as_str)))?, v);
                    }
                    _ => u::soma(&mut orig, "Outras deduções da receita", -v),
                }
            }
        }
        if acoes.lista.iter().any(|x| x.f == "01" && !x.cod.starts_with(&format!("{ORGAO_CAMARA}."))) {
            return Err(format!("TCE-TO: {a} com função 01 fora da Câmara"));
        }
        let rot = consolidada(a).map_or_else(|| format!("remessa {mes} ({} unidades gestoras)", fontes.len()), |c| format!("remessa {c} Consolidada"));
        dados.insert(a.into(), u::ano(acoes.lista, orig, format!("SICAP TCE-TO · Balancete Despesa {a} · {rot}"), format!("SICAP TCE-TO · Balancete Receita {a} · {rot}")));
    }
    Ok((dados, format!("{mes:02}")))
}

/// O que a esteira chama. O molde foi feito para Palmas (unidade principal e IBGE fixos).
pub fn preparar(busca: Busca, setores: &dyn Fn(&Path) -> Result<(), String>, numero: &str, nome: &str, pasta: &Path) -> Result<Value, String> {
    if sem_acento(nome).to_uppercase() != "PALMAS" {
        return Err("o molde do TCE-TO foi feito para Palmas (unidade principal 24851511000185); outra cidade: trocar PRINCIPAL e IBGE".into());
    }
    let (dados, mes26) = ler(busca)?;
    base::montar(pasta, numero, nome, "TO", IBGE, &tribunal(), &dados, &mes26, &base::Opcoes { festas_extra: Some(FESTAS_EXTRA), ..Default::default() }, setores)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Linha do Balancete Despesa com 36 colunas.
    fn d(rem: &str, org: &str, f: &str, acao: &str, nome: &str, ini_: &str, atu: &str, emp: &str, liq: &str) -> String {
        let mut c = vec![String::new(); 36];
        for (i, v) in [(2, rem), (3, org), (5, f), (6, "392"), (7, "0001"), (9, acao), (10, nome), (13, ini_), (33, atu), (34, emp), (35, liq)] {
            c[i] = v.into();
        }
        format!("<tr>{}</tr>", c.iter().map(|x| format!("<td class='x'>{x}</td>")).collect::<String>())
    }

    fn r(rem: &str, conta: &str, op: &str, v: &str, tipo: &str) -> String {
        let mut c = vec![String::new(); 15];
        for (i, x) in [(2, rem), (5, conta), (7, op), (12, v), (13, "nome"), (14, tipo)] {
            c[i] = x.into();
        }
        format!("<tr>{}</tr>", c.iter().map(|x| format!("<td>{x}</td>")).collect::<String>())
    }

    fn portal(desp26: String) -> impl Fn(&str, &str, &str, i64) -> Result<String, String> {
        move |rota, ug, ano, rem| {
            Ok(match (rota, ano) {
                ("getUnidades", _) => r#"[{"id":"24851511000185","nome":"PREFEITURA"},{"id":"111","nome":"CAMARA"},{"id":"222","nome":"AGENCIA EXTINTA"},{"id":"333","nome":"SEM REMESSA"}]"#.into(),
                ("getRemessas", "2026") => match ug {
                    "24851511000185" => r#"[{"numero":"8","envio":"1"},{"numero":"9","envio":"0"}]"#,
                    "111" => r#"[{"numero":"7","envio":"1"}]"#,
                    "222" => r#"[{"numero":"3","envio":"1"}]"#,
                    _ => "[]",
                }
                .into(),
                ("getRemessas", a) => if ug == PRINCIPAL { format!(r#"[{{"numero":"{}","envio":"1"}}]"#, if a == "2024" { 8 } else { 14 }) } else { "[]".into() },
                ("balanceteDespesa", "2026") if ug == PRINCIPAL => {
                    assert_eq!(rem, 7); // a Prefeitura mandou a 8, mas o corte e a 7 (a Camara)
                    desp26.clone()
                }
                ("balanceteDespesa", "2026") => format!("<table><tr><td>cab</td></tr>{}</table>", d(&rem.to_string(), "10", "01", "2001", "LEGISLATIVO", "50,00", "50,00", "5,00", "4,00")),
                ("balanceteDespesa", _) => format!("<table><tr><td>cab</td></tr>{}{}</table>", d(&rem.to_string(), "10", "01", "2001", "LEGISLATIVO", "10,00", "10,00", "9,00", "8,00"), d(&rem.to_string(), "20", "01", "2002", "APOIO", "1,00", "1,00", "1,00", "1,00")),
                ("balanceteReceita", _) => format!("<table><tr><td>cab</td></tr>{}{}{}{}{}</table>", r(&rem.to_string(), "1112", "101 - Receita", "1.000,00", "A"), r(&rem.to_string(), "1", "101", "9.999,00", "S"), r(&rem.to_string(), "1751", "105 - Fundeb", "200,00", "A"), r(&rem.to_string(), "7211", "101", "50,00", "A"), r(&(rem - 1).to_string(), "1112", "101", "77,00", "A")),
                _ => return Err(format!("rota {rota}")),
            })
        }
    }

    fn desp26() -> String {
        format!(
            "<table><tr><td>cab</td></tr>{}{}{}{}</table>",
            d("6", "30", "13", "4035", "RealizaÃ§Ã£o do RÃ©veillon'", "", "", "1.000,50", "900,25"),
            d("7", "30", "13", "4035", "Realização &amp; apoio", "2.000,00", "2.500,00", "10,00", "5,00"),
            d("8", "30", "13", "4035", "DEPOIS DO CORTE", "9,00", "9,00", "9,00", "9,00"),
            d("7", "30", "01", "2003", "GABINETE", "", "", "3,00", "3,00"),
        )
    }

    #[test]
    fn le_a_consolidada_e_soma_as_unidades_em_2026() {
        let (dados, m) = ler(&portal(desp26())).unwrap();
        assert_eq!(m, "07");
        let a = &dados["2026"];
        assert_eq!(a.acoes.len(), 3); // Prefeitura (2 acoes) + Camara; a extinta entra com a remessa 3 dela (mesma acao da Camara)
        let x = &a.acoes[0];
        assert_eq!((x.acao.as_str(), x.cod.as_str(), x.f.as_str()), ("Realização & apoio", "30.13.392.0001.4035", "13"));
        assert_eq!((x.orcado, x.atualizado, x.empenhado, x.liquidado), (2000.0, 2500.0, 1010.5, 905.25)); // dotacao so da remessa 7
        assert_eq!(a.acoes[1].f, "0"); // funcao 01 fora do orgao 10 vai para "Outros"
        assert_eq!((a.acoes[2].liquidado, a.acoes[2].orcado), (8.0, 100.0)); // Camara (rem 7) + extinta (rem 3)
        assert_eq!(a.receita, [("Impostos, taxas e contribuições de melhoria".to_string(), 3000.0), (u::INTRA.to_string(), 150.0), ("Deduções da receita (Fundeb)".to_string(), -600.0)]);
        assert_eq!(a.doc_despesa, "SICAP TCE-TO · Balancete Despesa 2026 · remessa 7 (3 unidades gestoras)");
        assert_eq!((dados["2025"].liquidado, dados["2025"].acoes[1].f.as_str(), dados["2024"].doc_receita.as_str()), (9.0, "0", "SICAP TCE-TO · Balancete Receita 2024 · remessa 8 Consolidada"));
        assert_eq!(celula("<b>ManutenÃ§Ã£o</b> "), "Manutenção");
        assert_eq!(celula("Ação já certa"), "Ação já certa");
    }

    #[test]
    fn recusa_o_que_falta() {
        let ok = portal(desp26());
        // 2025 sem a Consolidada (so ate a remessa 12)
        let sem = |rota: &str, ug: &str, ano: &str, rem: i64| if rota == "getRemessas" && ano == "2025" { Ok(r#"[{"numero":"12","envio":"1"}]"#.to_string()) } else { ok(rota, ug, ano, rem) };
        assert!(ler(&sem).unwrap_err().contains("2025 sem remessa Consolidada"));
        // ninguem entregou a partir de junho
        let cedo = |rota: &str, ug: &str, ano: &str, rem: i64| if rota == "getRemessas" && ano == "2026" { Ok(r#"[{"numero":"5","envio":"1"}]"#.to_string()) } else { ok(rota, ug, ano, rem) };
        assert!(ler(&cedo).unwrap_err().contains("a partir de junho"));
        // linha cortada, valor que nao e numero, conta sem origem
        assert!(ler(&portal(desp26().replace("<td class='x'>900,25</td>", ""))).unwrap_err().contains("colunas"));
        assert!(ler(&portal(desp26().replace("900,25", "novecentos"))).unwrap_err().contains("não é número"));
        let rec = |rota: &str, ug: &str, ano: &str, rem: i64| if rota == "balanceteReceita" { Ok(format!("<tr></tr>{}", r(&rem.to_string(), "9911", "101", "1,00", "A"))) } else { ok(rota, ug, ano, rem) };
        assert!(ler(&rec).unwrap_err().contains("sem origem conhecida: 9911"));
        assert!(ler(&|_, _, _, _| Err("fora do ar".into())).is_err());
        let p = std::env::temp_dir().join(format!("braniac-link_tribunais_to-{}", std::process::id()));
        assert!(preparar(&ok, &|_| Ok(()), "1", "Araguaína", &p).unwrap_err().contains("feito para Palmas"));
        assert!(!p.exists());
    }
}
