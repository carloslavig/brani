//! Molde da Paraiba (link/tribunais/pb.py): SAGRES do TCE-PB. Dados Abertos (receitas e despesas por empenho, CSV) e SAGRES
//! Online (despesas autorizadas = orcado por dotacao; remessas mensais; liquidacoes com data). 2026 vale ate o ultimo mes em que
//! TODAS as unidades com movimento tem a remessa ENVIADO, e o que foi liquidado depois do corte sai do liquidado (o arquivo traz
//! o acumulado ate hoje). Rubrica sem origem, liquidado negativo depois do corte, coluna que falta ou valor que nao e numero = Err.
//! Entram como closure: `texto(tipo, ano)` = "receitas" | "despesas" (CSV ja tirado do zip, UTF-8) e "dotacao" (JSON das despesas
//! autorizadas); `pega(caminho)` = JSON de uma consulta ao SAGRES Online (aba do navegador, uma por vez, com pausa: de quem chama).
//! Ficou de fora: baixar os zips, a aba escondida (Cloudflare), achar o codigo do municipio nas listas do site e o cache em disco.
#![allow(dead_code)]
use crate::link_auditar::br;
use crate::link_tribunais_base::{self as base, col, csv, Dados};
use crate::link_tribunais_util::{self as u, ini, n_br, Acoes};
use crate::tempo::Data;
use crate::texto_pt::sem_acento;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Nomes locais conferidos nos empenhos de 2024 a 2026 (o resto a regra padrao pega).
pub const FESTAS_EXTRA: &str = "NATAL DE SENTIM";
/// Prefixos de UG da Camara e do Fundo da Camara.
pub const CAMARA: [&str; 2] = ["101", "614"];
pub type Texto<'a> = &'a dyn Fn(&str, &str) -> Result<String, String>;
pub type Pega<'a> = &'a dyn Fn(&str) -> Result<String, String>;
/// "ug|uo|empenho" -> liquidado depois do corte.
pub type Depois = BTreeMap<String, f64>;

pub fn tribunal() -> Value {
    json!({ "sigla": "TCE-PB", "fala": "Tribunal de Contas do Estado", "uf": "PB", "estado": "Paraíba", "doc_despesa": "SAGRES DESPESAS", "doc_receita": "SAGRES RECEITAS",
        "tribunal_estado": "TCE-PB (Tribunal de Contas do Estado da Paraíba)", "site_tribunal": "sagresonline.tce.pb.gov.br",
        "fonte_md": "Tudo do SAGRES do TCE-PB (Tribunal de Contas do Estado da Paraíba): Dados Abertos (dados-abertos.tce.pb.gov.br, arquivos receitas-<ano> e despesas-<ano> do município; deduções do Fundeb já descontadas; empenhado e liquidado por empenho) e SAGRES Online (sagresonline.tce.pb.gov.br: despesas autorizadas, com Orçado e Autorizado por dotação; remessas mensais; liquidações com data). \"Gastou\" = LIQUIDADO, todas as unidades do município. \"Previa\" = Orçado (dotação inicial da lei do orçamento).",
        "setores_md": "Setores = soma do liquidado das ações (projeto/atividade) dos empenhos do SAGRES, pela função; festas e eventos separados da cultura; segurança pública fora.",
        "pendencias_md": ["O SAGRES Online ainda não tem a dotação de 2026 (despesas autorizadas de 2026 vazias): orçado e atualizado de 2026 = 0.", "O Fundo da Câmara (FEJP) não tem dotação no SAGRES Online: orçado dele = 0 (liquidado entra normalmente)."],
        "orgaos": "Município inteiro (Prefeitura, Câmara, autarquias, fundações e fundos que mandam o SAGRES)",
        "legenda_docs": ["• Receita: SAGRES do TCE-PB, Dados Abertos, receitas de 2024, 2025 e 2026", "• Despesa, setores e festas: SAGRES do TCE-PB, empenhos (liquidado) e despesas autorizadas (orçado), por ação", "• dados-abertos.tce.pb.gov.br e sagresonline.tce.pb.gov.br"] })
}

/// Numero de empenho sem zeros a esquerda ("00012" -> "12"); o que nao e so digito fica como veio.
fn num(s: &str) -> String {
    let t = s.trim();
    if t.is_empty() || !t.chars().all(|c| c.is_ascii_digit()) {
        return t.into();
    }
    let z = t.trim_start_matches('0');
    if z.is_empty() { "0".into() } else { z.into() }
}

/// A acao da chave UO.funcao.subfuncao.programa.acao (a funcao fica como numero em `f` ate o fim da leitura).
fn entra(acoes: &mut Acoes, uo: &str, f: &str, sb: &str, pr: &str, ac: &str) -> Result<usize, String> {
    let (uo, f, sb, pr, ac) = (uo.trim(), u::int(f, "função")?, u::int(sb, "subfunção")?, pr.trim(), ac.trim());
    let (i, nova) = acoes.idx(&format!("{uo}|{f}|{sb}|{pr}|{ac}"));
    if nova {
        let y = &mut acoes.lista[i];
        (y.f, y.sub, y.cod) = (f.to_string(), format!("{sb:03}"), format!("{uo}.{f:02}.{sb:03}.{pr}.{ac}"));
    }
    Ok(i)
}

fn dia(y: i64, m: i64, d: i64) -> Result<Data, String> {
    Data::civil(y, m, d).ok_or_else(|| format!("data inválida: {y}-{m}-{d}"))
}

/// Primeiro dia do mes seguinte.
fn mes_seguinte(d: Data) -> Result<Data, String> {
    let (y, m, _) = d.partes();
    dia(y + i64::from(m == 12), m % 12 + 1, 1)
}

/// Unidades gestoras com movimento em 2026 (receitas ou despesas).
pub fn ativas(texto: Texto) -> Result<BTreeSet<String>, String> {
    let mut out = BTreeSet::new();
    for t in ["receitas", "despesas"] {
        for x in csv(&texto(t, "2026")?, ';') {
            out.insert(col(&x, "codigo_unidade_gestora")?.to_string());
        }
    }
    Ok(out)
}

/// Ultimo mes de 2026 em que TODAS as unidades com movimento em 2026 tem a remessa mensal com situacao ENVIADO.
pub fn mes26(pega: Pega, ugs: &BTreeSet<String>, hoje: Data) -> Result<String, String> {
    let (y, mh, _) = hoje.partes();
    'mes: for m in (1..=if y == 2026 { mh } else { 12 }).rev() {
        let fim = Data(mes_seguinte(dia(2026, m, 1)?)?.0 - 1).iso();
        for ug in ugs {
            let r = u::json(&pega(&format!("/captura/api/v1/envios?dataMinima=2026-{m:02}-01&dataMaxima={fim}&tipoEnvio=mensal&withPrazoAtraso=true&codUnidadeGestora={ug}"))?, "envios")?;
            if !u::lista(&r, "envios")?.iter().any(|e| e["mes"].as_i64() == Some(m) && e["situacao"] == "ENVIADO") {
                continue 'mes;
            }
        }
        return Ok(format!("{m:02}"));
    }
    Err("TCE-PB: nenhum mês de 2026 com a remessa mensal de todas as unidades enviada".into())
}

/// Liquidacoes de empenhos de 2026 feitas DEPOIS do corte, mes a mes ate hoje.
pub fn liq_depois(pega: Pega, ugs: &BTreeSet<String>, mes26: &str, hoje: Data) -> Result<Depois, String> {
    let mut ini_ = mes_seguinte(dia(2026, u::int(mes26, "mês de corte")?, 1)?)?;
    let mut depois = Depois::new();
    while ini_ <= hoje {
        let fim = Data(mes_seguinte(ini_)?.0 - 1).min(hoje);
        for ug in ugs {
            let r = u::json(&pega(&format!("/api/v2/municipal/execucao-orcamentaria/liquidacoes?startDate={}&endDate={}&searchText=&unidadesGestoras[]={ug}", ini_.iso(), fim.iso()))?, "liquidacoes")?;
            for x in u::lista(&r, "liquidacoes")? {
                let (p, ci) = (&x["dadosPrincipais"], &x["classificacaoInstitucional"]);
                if ini(u::txt(p, "dataEmpenho")?, 4) == "2026" {
                    for k in ["unidadeGestoraCod", "unidadeOrcamentariaCod"] {
                        if ci[k].is_null() {
                            return Err(format!("liquidação do SAGRES Online sem o campo {k}"));
                        }
                    }
                    if p["numEmpenho"].is_null() {
                        return Err("liquidação do SAGRES Online sem o campo numEmpenho".into());
                    }
                    let k = format!("{}|{}|{}", u::pystr(&ci["unidadeGestoraCod"]), u::pystr(&ci["unidadeOrcamentariaCod"]), num(&u::pystr(&p["numEmpenho"])));
                    *depois.entry(k).or_default() += u::num(&x["dadosNotaFiscal"], "valorLiquidado")?;
                }
            }
        }
        ini_ = Data(fim.0 + 1);
    }
    Ok(depois)
}

/// Le os 3 anos. `depois` = liquidacoes de 2026 depois do corte (saem do liquidado).
pub fn ler(texto: Texto, depois: &Depois, mes26: &str) -> Result<Dados, String> {
    let m26 = u::int(mes26, "mês de corte")?;
    let mut dados = Dados::new();
    for a in base::ANOS {
        let corte = if a == "2026" { m26 } else { 12 };
        let mut orig = vec![];
        for r in csv(&texto("receitas", a)?, ';') {
            if u::int(ini(col(&r, "mes_ano")?, 2), "mes_ano")? > corte {
                continue;
            }
            let (c, tipo) = (col(&r, "codigo_receita")?.trim(), col(&r, "tipo_atualizacao_receita")?);
            let nome = if tipo == "Dedução do Fundeb" {
                Some("Deduções da receita (Fundeb)")
            } else if tipo.contains("Dedu") {
                Some("Outras deduções da receita")
            } else {
                u::origem(ini(c, 2)).or(c.starts_with(['7', '8']).then_some(u::INTRA))
            };
            u::soma(&mut orig, nome.ok_or_else(|| format!("TCE-PB: rubrica de receita sem origem conhecida: {c} ({tipo})"))?, n_br(col(&r, "valor")?)?);
        }
        let (mut acoes, mut camara): (Acoes, BTreeSet<usize>) = Default::default(); // camara = acoes com empenho da Camara
        let dot = u::json(&texto("dotacao", a)?, "despesas autorizadas")?;
        for x in u::lista(&dot, "despesas autorizadas")? {
            let p: Vec<&str> = u::txt(x, "dotacao")?.split('.').collect();
            if p.len() < 5 {
                return Err(format!("TCE-PB: dotação fora do formato UO.função.subfunção.programa.ação: {}", x["dotacao"]));
            }
            let i = entra(&mut acoes, p[0], p[1], p[2], p[3], p[4])?;
            let y = &mut acoes.lista[i];
            if y.acao.is_empty() {
                y.acao = u::txt(x, "acao")?.trim().to_uppercase();
            }
            y.orcado += u::num(x, "valorOrcado")?;
            y.atualizado += u::num(x, "valorAutorizado")?;
        }
        let mut tirado = 0.0;
        for r in csv(&texto("despesas", a)?, ';') {
            if u::int(ini(col(&r, "mes")?, 2), "mes")? > corte {
                continue;
            }
            let (ug, uo) = (col(&r, "codigo_unidade_gestora")?, col(&r, "codigo_unidade_orcamentaria")?);
            let i = entra(&mut acoes, uo, col(&r, "codigo_funcao")?, col(&r, "codigo_subfuncao")?, col(&r, "codigo_programa")?, col(&r, "codigo_acao")?)?;
            let y = &mut acoes.lista[i];
            y.acao = col(&r, "acao")?.trim().to_uppercase();
            let mut liq = n_br(col(&r, "valor_liquidado")?)?;
            if a == "2026" {
                let t = depois.get(&format!("{ug}|{}|{}", uo.trim(), num(col(&r, "numero_empenho")?))).copied().unwrap_or(0.0);
                liq -= t;
                tirado += t;
            }
            y.empenhado += n_br(col(&r, "valor_empenhado")?)?;
            y.liquidado += liq;
            // funcao 01 (Legislativa) so da Camara e do Fundo da Camara; outro orgao na 01 vai para "Outros"
            if CAMARA.contains(&ini(ug, 3)) {
                camara.insert(i);
            }
        }
        let mut l = vec![];
        for (i, mut y) in acoes.lista.into_iter().enumerate() {
            if y.orcado == 0.0 && y.atualizado == 0.0 && y.empenhado == 0.0 && y.liquidado == 0.0 {
                continue;
            }
            if y.liquidado <= -1.0 {
                return Err(format!("{a}: liquidado negativo depois do corte em {}", y.cod));
            }
            if y.f == "1" && !camara.contains(&i) && y.liquidado != 0.0 {
                y.f = "0".into();
            }
            l.push(y);
        }
        let ate = if a == "2026" { format!(" (até {mes26}/2026; liquidações depois do corte descontadas: {})", br(tirado)) } else { String::new() };
        dados.insert(a.into(), u::ano(l, orig, format!("SAGRES TCE-PB · despesas-{a}.zip + despesas autorizadas {a}{ate}"), format!("SAGRES TCE-PB · receitas-{a}.zip")));
    }
    Ok(dados)
}

/// O que a esteira chama. So a capital tem codigo do IBGE neste molde.
pub fn preparar(texto: Texto, pega: Pega, setores: &dyn Fn(&Path) -> Result<(), String>, numero: &str, nome: &str, pasta: &Path, hoje: Data) -> Result<Value, String> {
    if sem_acento(nome).to_uppercase() != "JOAO PESSOA" {
        return Err("TCE-PB: código IBGE só da capital neste molde; acrescente o do município".into());
    }
    let ugs = ativas(texto)?;
    let m = mes26(pega, &ugs, hoje)?;
    let dados = ler(texto, &liq_depois(pega, &ugs, &m, hoje)?, &m)?;
    base::montar(pasta, numero, nome, "PB", 2507507, &tribunal(), &dados, &m, &base::Opcoes { festas_extra: Some(FESTAS_EXTRA), ..Default::default() }, setores)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAB_D: &str = "codigo_unidade_gestora;mes;codigo_unidade_orcamentaria;codigo_funcao;codigo_subfuncao;codigo_programa;codigo_acao;acao;numero_empenho;valor_empenhado;valor_liquidado";
    const CAB_R: &str = "codigo_unidade_gestora;mes_ano;codigo_receita;tipo_atualizacao_receita;valor";

    fn despesas() -> String {
        format!(
            "{CAB_D}\n201095;01/2026; 02010 ;13;392;1001;2010;São João da cidade;0000012;1.000,00;800,00\n201095;02/2026;02010;13;392;1001;2010;SÃO JOÃO DA CIDADE;13;500,00;500,00\n201095;02/2026;02010;1;31;0001;2001;Apoio;14;10,00;10,00\n101095;02/2026;01010;01;031;0001;2001;Atividade legislativa;1;70,00;70,00\n201095;09/2026;02010;13;392;1001;2010;X;99;9,00;9,00"
        )
    }

    fn receitas() -> String {
        format!("{CAB_R}\n201095;01/2026;1112;Lançamento;2.000,00\n201095;02/2026;1751;Dedução do Fundeb;-300,00\n201095;02/2026;1321;Dedução de receita;-5,00\n201095;02/2026;7218;Lançamento;40,00\n201095;09/2026;1112;Lançamento;999,00")
    }

    fn fonte(tipo: &str, _: &str) -> Result<String, String> {
        Ok(match tipo {
            "receitas" => receitas(),
            "despesas" => despesas(),
            _ => r#"[{"dotacao":"02010.13.392.1001.2010.339039","acao":" São João ","valorOrcado":300.0,"valorAutorizado":350},{"dotacao":"02010.13.392.1001.2010.449052","acao":"x","valorOrcado":100,"valorAutorizado":100},{"dotacao":"03000.04.122.0002.2999.3390","acao":"Sem execução","valorOrcado":7,"valorAutorizado":7},{"dotacao":"03000.04.122.0002.2998.3390","acao":"Zerada","valorOrcado":0,"valorAutorizado":0}]"#.into(),
        })
    }

    fn online(c: &str) -> Result<String, String> {
        if c.starts_with("/captura/") {
            // a Camara (101095) so enviou ate agosto; a Prefeitura ate setembro
            let m: i64 = c.split("dataMinima=2026-").nth(1).unwrap()[..2].parse().unwrap();
            let ok = if c.ends_with("=101095") { m <= 8 } else { m <= 9 };
            return Ok(format!(r#"[{{"mes":{m},"situacao":"{}"}}]"#, if ok { "ENVIADO" } else { "PENDENTE" }));
        }
        assert!(c.starts_with("/api/v2/municipal/execucao-orcamentaria/liquidacoes?startDate=2026-"));
        Ok(if c.contains("startDate=2026-09-01&endDate=2026-09-30") && c.ends_with("=201095") {
            r#"[{"dadosPrincipais":{"dataEmpenho":"2026-01-10","numEmpenho":"12"},"classificacaoInstitucional":{"unidadeGestoraCod":201095,"unidadeOrcamentariaCod":"02010"},"dadosNotaFiscal":{"valorLiquidado":100.5}},
                {"dadosPrincipais":{"dataEmpenho":"2025-12-30","numEmpenho":"12"},"classificacaoInstitucional":{"unidadeGestoraCod":201095,"unidadeOrcamentariaCod":"02010"},"dadosNotaFiscal":{"valorLiquidado":999}}]"#
        } else if c.contains("startDate=2026-10-01&endDate=2026-10-15") && c.ends_with("=201095") {
            r#"[{"dadosPrincipais":{"dataEmpenho":"2026-01-10","numEmpenho":"0012"},"classificacaoInstitucional":{"unidadeGestoraCod":201095,"unidadeOrcamentariaCod":"02010"},"dadosNotaFiscal":{"valorLiquidado":50}}]"#
        } else {
            "[]"
        }
        .into())
    }

    fn hoje() -> Data {
        Data::civil(2026, 10, 15).unwrap()
    }

    #[test]
    fn corte_pelas_remessas_e_liquidado_depois_do_corte_sai() {
        let ugs = ativas(&fonte).unwrap();
        assert_eq!(ugs.iter().collect::<Vec<_>>(), ["101095", "201095"]);
        assert_eq!(mes26(&online, &ugs, hoje()).unwrap(), "08");
        let depois = liq_depois(&online, &ugs, "08", hoje()).unwrap();
        assert_eq!(depois, Depois::from([("201095|02010|12".to_string(), 150.5)])); // empenho de 2025 nao entra
        let dados = ler(&fonte, &depois, "08").unwrap();
        let a = &dados["2026"];
        assert_eq!(a.acoes.len(), 4); // a zerada sai; setembro fica fora
        let x = &a.acoes[0];
        assert_eq!((x.acao.as_str(), x.cod.as_str(), x.f.as_str(), x.sub.as_str()), ("SÃO JOÃO DA CIDADE", "02010.13.392.1001.2010", "13", "392"));
        assert_eq!((x.orcado, x.atualizado, x.empenhado, x.liquidado), (400.0, 450.0, 1500.0, 1300.0 - 150.5));
        assert_eq!((a.acoes[1].acao.as_str(), a.acoes[1].orcado, a.acoes[1].liquidado), ("SEM EXECUÇÃO", 7.0, 0.0));
        assert_eq!((a.acoes[2].f.as_str(), a.acoes[2].cod.as_str(), a.acoes[3].f.as_str()), ("0", "02010.01.031.0001.2001", "1")); // funcao 01 so da Camara
        assert_eq!(a.receita, [("Impostos, taxas e contribuições de melhoria".to_string(), 2000.0), (u::INTRA.to_string(), 40.0), ("Outras deduções da receita".to_string(), -5.0), ("Deduções da receita (Fundeb)".to_string(), -300.0)]);
        assert!(a.doc_despesa.ends_with("(até 08/2026; liquidações depois do corte descontadas: 150,50)"));
        assert_eq!(dados["2025"].acoes[0].liquidado, 1309.0); // ano fechado: sem desconto, com o mes 09
        assert_eq!((num("0000012"), num(" 000 "), num("12-A")), ("12".to_string(), "0".to_string(), "12-A".to_string()));
    }

    #[test]
    fn recusa_o_que_falta() {
        let ugs = ativas(&fonte).unwrap();
        assert!(mes26(&|_| Ok(r#"[{"mes":1,"situacao":"PENDENTE"}]"#.into()), &ugs, hoje()).unwrap_err().contains("nenhum mês de 2026"));
        assert!(mes26(&|_| Ok("<html>Um momento…</html>".into()), &ugs, hoje()).unwrap_err().contains("não é JSON"));
        let d = Depois::new();
        let ruim = |de: &'static str, para: &'static str| move |tipo: &str, ano: &str| fonte(tipo, ano).map(|t| t.replace(de, para));
        assert!(ler(&ruim(";1112;", ";3112;"), &d, "08").unwrap_err().contains("sem origem conhecida: 3112 (Lançamento)"));
        assert!(ler(&ruim("800,00", "oitocentos"), &d, "08").unwrap_err().contains("não é número"));
        assert!(ler(&ruim("valor_liquidado", "pago"), &d, "08").unwrap_err().contains("sem a coluna valor_liquidado"));
        assert!(ler(&ruim("02010.13.392.1001.2010.449052", "02010.13"), &d, "08").unwrap_err().contains("fora do formato"));
        assert!(ler(&ruim("\"valorOrcado\":100,", ""), &d, "08").unwrap_err().contains("valorOrcado"));
        // desconto maior que o liquidado do arquivo: algo nao fecha
        let demais = Depois::from([("201095|02010|14".to_string(), 500.0)]);
        assert!(ler(&fonte, &demais, "08").unwrap_err().contains("2026: liquidado negativo depois do corte em 02010.01.031.0001.2001"));
        let p = std::env::temp_dir().join(format!("braniac-link_tribunais_pb-{}", std::process::id()));
        assert!(preparar(&fonte, &online, &|_| Ok(()), "1", "Campina Grande", &p, hoje()).unwrap_err().contains("só da capital"));
    }
}
