//! Molde do Acre (link/tribunais/ac.py), Rio Branco: paineis Qlik do TCE-AC alimentados pelas remessas mensais (despesa por
//! empenho com funcao, subfuncao e acao; receita realizada e deducoes por natureza). O painel nao tem dotacao: `sem_previsto`.
//! 2026 so vale ate o mes que TODOS os orgaos que mandaram 2025 inteiro ja entregaram, sem buraco; se a Prefeitura nao mandou
//! nenhuma remessa de 2026, o video e de pauta (2026 nao e lido). 2026 incompleto com a Prefeitura em dia, natureza sem origem,
//! valor que nao e numero ou acoes que nao fecham com o painel de remessas = Err (pendencia, nada estimado).
//! Entra como closure: `cubo(app, dims, medidas)` devolve o JSON com TODAS as linhas do hipercubo (app = "despesa" | "receita");
//! o socket do Qlik, a paginacao dele e a pausa entre consultas sao de quem chama.
//! Ficou de fora: o JS do motor do Qlik e a lista do IBGE (o codigo da cidade entra como parametro).
#![allow(dead_code)]
use crate::link_tribunais_base::{self as base, Ano, Dados};
use crate::link_tribunais_util::{self as u, ini, pystr, Acoes};
use crate::texto_pt::sem_acento;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const IBGE_RIO_BRANCO: i64 = 1200401;
pub type Cubo<'a> = &'a dyn Fn(&str, &[&str], &[String]) -> Result<String, String>;

pub fn tribunal() -> Value {
    json!({ "sigla": "TCE-AC", "fala": "Tribunal de Contas do Estado", "uf": "AC", "estado": "Acre", "tribunal_estado": "TCE-AC (Tribunal de Contas do Estado do Acre)", "site_tribunal": "tceac.tc.br",
        "doc_despesa": "REMESSAS TCE-AC DESPESA", "doc_receita": "REMESSAS TCE-AC RECEITA",
        "fonte_md": "Tudo dos painéis Qlik do TCE-AC (Tribunal de Contas do Estado do Acre), links \"Despesa\" e \"Receita\" em tceac.tc.br/site/inicio/acre-transparente, alimentados pelas remessas mensais que prefeitura, Câmara, autarquias e fundos mandam ao tribunal: despesa por empenho do próprio exercício (função, subfunção e ação; liquidado e empenhado menos estornos) e receita realizada por natureza, deduções do Fundeb descontadas. \"Gastou\" = LIQUIDADO. O painel não tem dotação (orçado) por ação. 2026: a Prefeitura não mandou nenhuma remessa ao TCE-AC.",
        "setores_md": "Setores = soma do liquidado por ação (remessas ao TCE-AC), pela função; festas e eventos separados da cultura; segurança pública fora.",
        "pendencias_md": ["O TCE-AC não publica a dotação (orçado) por ação: vídeo sem \"previa\".", "Portal do Cidadão do TCE-AC (Relatório de Entrega de Remessas) fora do ar em 15 e 16/11/2026; a falta de 2026 da Prefeitura foi conferida no painel de remessas do próprio tribunal."],
        "orgaos": "Prefeitura, Câmara, autarquias, empresas e fundos (todas as entidades de Rio Branco que mandam remessa ao TCE-AC)",
        "legenda_docs": ["• Receita: TCE-AC, remessas de receita, realizado menos deduções, 2024 e 2025", "• Despesa e setores: TCE-AC, remessas de despesa, liquidado por ação, 2024 e 2025", "• 2026: nenhuma remessa da Prefeitura no TCE-AC", "• tceac.tc.br > Acre Transparente > Despesa / Receita"] })
}

/// Remessas por orgao do ente e o que da para dizer de 2026.
#[derive(Debug, Default, PartialEq)]
pub struct Situacao {
    /// Ultimo mes de 2026 que TODOS os orgaos com 2025 completo entregaram (sem buraco antes); None = nao ha.
    pub mes26: Option<i64>,
    /// Orgaos com 2025 completo e nenhuma remessa de 2026.
    pub faltam: Vec<String>,
    /// ano -> (liquidado, empenhado) do painel de remessas.
    pub totais: BTreeMap<i64, (f64, f64)>,
    /// orgao -> ano -> remessas entregues.
    pub por: BTreeMap<String, BTreeMap<i64, BTreeSet<i64>>>,
}

fn linhas(cubo: Cubo, app: &str, dims: &[&str], meas: &[String], n: usize) -> Result<Vec<Vec<Value>>, String> {
    let v = u::json(&cubo(app, dims, meas)?, "painel do TCE-AC")?;
    u::lista(&v, "painel do TCE-AC")?.iter().map(|l| l.as_array().filter(|l| l.len() == n).cloned().ok_or_else(|| format!("painel de {app} do TCE-AC: linha fora do formato"))).collect()
}

fn inteiro(v: &Value, o_que: &str) -> Result<i64, String> {
    v.as_i64().or_else(|| v.as_f64().filter(|f| f.fract() == 0.0).map(|f| f as i64)).or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())).ok_or_else(|| format!("TCE-AC: {o_que} que não é número: {v}"))
}

/// Valor de uma medida: nulo = 0; texto (o "-" do painel) no lugar do numero e erro.
fn valor(v: &Value) -> Result<f64, String> {
    if v.is_null() { Ok(0.0) } else { v.as_f64().ok_or_else(|| format!("TCE-AC: valor que não é número no painel: {v}")) }
}

fn cheio(v: &Value) -> bool {
    !(v.is_null() || v.as_f64() == Some(0.0) || v.as_str() == Some(""))
}

pub fn situacao(cubo: Cubo, ente: &str) -> Result<Situacao, String> {
    let f = format!("{{<NOME_ENTE={{'{ente}'}}>}}");
    let mut s = Situacao::default();
    for l in linhas(cubo, "despesa", &["entidade", "ANO_REMESSA", "NUMERO_REMESSA"], &[format!("Count({f}id_despesa)")], 4)? {
        if cheio(&l[3]) && pystr(&l[1]) != "-" {
            s.por.entry(pystr(&l[0])).or_default().entry(inteiro(&l[1], "ano da remessa")?).or_default().insert(inteiro(&l[2], "remessa")?);
        }
    }
    for l in linhas(cubo, "despesa", &["ANO_REMESSA"], &[format!("Sum({f}valor_liquidado)"), format!("Sum({f}VALOR_EMPENHO)")], 3)? {
        if pystr(&l[0]) != "-" {
            s.totais.insert(inteiro(&l[0], "ano da remessa")?, (valor(&l[1])?, valor(&l[2])?));
        }
    }
    let vazio = BTreeSet::new();
    let de26 = |o: &String| s.por[o].get(&2026).unwrap_or(&vazio);
    // orgaos que mandaram o ano de 2025 inteiro
    let obrig: Vec<&String> = s.por.iter().filter(|(_, d)| d.get(&2025).is_some_and(|r| r.len() == 12)).map(|(o, _)| o).collect();
    s.mes26 = if obrig.is_empty() { None } else { (1..=12).rev().find(|m| obrig.iter().all(|o| (1..=*m).all(|i| de26(o).contains(&i)))) };
    s.faltam = obrig.iter().filter(|o| de26(o).is_empty()).map(|o| o.to_string()).collect();
    Ok(s)
}

/// Nomes dos municipios (NOME_ENTE) no painel de despesa.
pub fn entes(cubo: Cubo) -> Result<Vec<String>, String> {
    Ok(linhas(cubo, "despesa", &["NOME_ENTE"], &["Count(id_despesa)".into()], 2)?.iter().filter(|l| cheio(&l[0]) && l[0] != "-").map(|l| pystr(&l[0])).collect())
}

/// Despesa por acao (empenhos do proprio exercicio) e receita por natureza dos `anos`; 2026 so ate `mes26`.
pub fn ler(cubo: Cubo, anos: &[&str], ente: &str, mes26: Option<i64>) -> Result<Dados, String> {
    let mut dados = Dados::new();
    for a in anos {
        let ate = |campo: &str| mes26.filter(|m| *a == "2026" && *m != 0).map(|m| format!(",{campo}={{\"<={m}\"}}")).unwrap_or_default();
        let f = format!("{{<NOME_ENTE={{'{ente}'}},ANO_REMESSA={{{a}}},ano_empenho={{{a}}}{}>}}", ate("NUMERO_REMESSA"));
        let g = format!("{{<ENTE.NOME={{'{ente}'}},ANO={{{a}}}{}>}}", ate("MES"));
        let mut ac = Acoes::default();
        for l in linhas(cubo, "despesa", &["numero_funcao", "numero_subfuncao", "CODIGO_ACAO", "TITULO_ACAO"], &[format!("Sum({f}valor_liquidado)"), format!("Sum({f}VALOR_EMPENHO)")], 6)? {
            if !cheio(&l[4]) && !cheio(&l[5]) {
                continue;
            }
            let (fu, sub) = (inteiro(&l[0], "função")?, inteiro(&l[1], "subfunção")?);
            let k = format!("0.0.{fu:02}.{sub:03}.0.{}", pystr(&l[2])); // o script de setores le a subfuncao na 4a posicao
            let (i, nova) = ac.idx(&k);
            let x = &mut ac.lista[i];
            if nova {
                (x.f, x.sub, x.cod, x.acao) = (fu.to_string(), format!("{sub:03}"), k, pystr(&l[3]).trim().to_string());
            }
            x.liquidado += valor(&l[4])?;
            x.empenhado += valor(&l[5])?;
        }
        let mut orig = vec![];
        for l in linhas(cubo, "receita", &["CODIGO-NUMERO_NATUREZA_RECEITA"], &[format!("Sum({g}RECEITA_REALIZADA)"), format!("Sum({g}DEDUCAO)")], 3)? {
            let c = pystr(&l[0]);
            if cheio(&l[1]) {
                let nome = u::origem(ini(&c, 2)).or(c.starts_with(['7', '8']).then_some(u::INTRA)).ok_or_else(|| format!("TCE-AC: natureza de receita sem origem conhecida: {c}"))?;
                u::soma(&mut orig, nome, valor(&l[1])?);
            }
            if cheio(&l[2]) {
                // ja vem negativa; Fundeb sai das transferencias (17)
                u::soma(&mut orig, if ini(&c, 2) == "17" { "Deduções da receita (Fundeb)" } else { "Outras deduções da receita" }, valor(&l[2])?);
            }
        }
        dados.insert(a.to_string(), Ano { arrecadado: orig.iter().map(|o| o.1).sum(), liquidado: ac.lista.iter().map(|x| x.liquidado).sum(), empenhado: ac.lista.iter().map(|x| x.empenhado).sum(), acoes: ac.lista, receita: orig,
            doc_despesa: format!("TCE-AC painel Qlik · remessas de despesa {a}"), doc_receita: format!("TCE-AC painel Qlik · remessas de receita {a}") });
    }
    Ok(dados)
}

/// O que a esteira chama. `ibge` = codigo da cidade (lista do IBGE do Acre, de quem chama).
pub fn preparar(cubo: Cubo, setores: &dyn Fn(&Path) -> Result<(), String>, numero: &str, nome: &str, pasta: &Path, ibge: i64) -> Result<Value, String> {
    let chave = |s: &str| sem_acento(s).to_uppercase();
    let ente = entes(cubo)?.into_iter().find(|e| chave(e) == chave(nome)).ok_or_else(|| format!("TCE-AC: '{nome}' não aparece no painel de despesa do tribunal (NOME_ENTE)"))?;
    let s = situacao(cubo, &ente)?;
    let op = base::Opcoes { sem_previsto: true, ..Default::default() };
    if let Some(m) = s.mes26 {
        // todos os orgaos entregaram 2026 ate m: video normal, sem "previa" (o painel nao tem dotacao)
        return base::montar(pasta, numero, nome, "AC", ibge, &tribunal(), &ler(cubo, &base::ANOS, &ente, Some(m))?, &format!("{m:02}"), &op, setores);
    }
    if !s.faltam.iter().any(|o| o.to_uppercase().contains("PREFEITURA")) {
        return Err(format!("TCE-AC: 2026 incompleto, mas a Prefeitura mandou remessa; faltam {} (pendência, nada estimado)", s.faltam.join(", ")));
    }
    // guia do AC: "Prefeitura ou camara em atraso no Relatorio de Entrega de Remessas: por que nao prestou contas?" -> video de pauta
    let meses: BTreeSet<i64> = s.por.values().filter_map(|d| d.get(&2026)).flatten().copied().collect();
    let nm = |m: &i64| usize::try_from(*m - 1).ok().and_then(|i| base::NM.get(i)).copied().ok_or_else(|| format!("TCE-AC: remessa de 2026 com mês inválido: {m}"));
    let ausente = match (meses.first(), meses.last()) {
        (Some(p), Some(ul)) => {
            let rot = if meses.len() <= 2 { meses.iter().map(&nm).collect::<Result<Vec<_>, _>>()?.join(" e ") } else { format!("{} a {}", nm(p)?, nm(ul)?) };
            let camara = s.faltam.iter().any(|o| o.to_uppercase().contains("CÂMARA") || o.to_uppercase().contains("CAMARA"));
            format!("só remessas de {rot} de alguns fundos e autarquias, e nada da Prefeitura{}", if camara { " nem da Câmara" } else { "" })
        }
        _ => "nenhuma remessa".to_string(),
    };
    let dados = ler(cubo, &["2024", "2025"], &ente, None)?;
    for a in [2024, 2025] {
        // o painel de remessas e a leitura por acao tem de dar o mesmo liquidado
        let (lido, rem) = (dados[&a.to_string()].liquidado, s.totais.get(&a).ok_or_else(|| format!("TCE-AC: {a} sem total no painel de remessas"))?.0);
        if (lido - rem).abs() >= 1.0 {
            return Err(format!("{a}: ações {lido:.2} != remessas {rem:.2}"));
        }
    }
    base::montar(pasta, numero, nome, "AC", ibge, &tribunal(), &dados, &meses.last().map_or("01".to_string(), |m| format!("{m:02}")), &base::Opcoes { ausente26: Some(&ausente), ..op }, setores)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Painel de teste. `pref26`/`cam26` = ate que mes a Prefeitura e a Camara mandaram 2026 (0 = nada).
    fn painel(pref26: i64, cam26: i64) -> impl Fn(&str, &[&str], &[String]) -> Result<String, String> {
        move |app, dims, meas| {
            let ano = meas[0].split("ANO_REMESSA={").nth(1).or(meas[0].split("ANO={").nth(1)).map(|s| &s[..4]);
            Ok(match (app, dims[0]) {
                ("despesa", "NOME_ENTE") => json!([["RIO BRANCO", 9], ["-", 1], ["XAPURI", 2]]),
                ("despesa", "entidade") => {
                    assert_eq!(meas[0], "Count({<NOME_ENTE={'RIO BRANCO'}>}id_despesa)");
                    let mut l = vec![json!(["FUNDO NOVO", 2026, 1, 3]), json!(["SAERB", "-", 1, 3])];
                    for (org, ate26) in [("PREFEITURA DE RIO BRANCO", pref26), ("CÂMARA MUNICIPAL", cam26), ("SAERB", 2)] {
                        l.extend((1..=12).map(|m| json!([org, 2025, m, 5])));
                        l.extend((1..=ate26).map(|m| json!([org, "2026", m, 5])));
                    }
                    json!(l)
                }
                ("despesa", "ANO_REMESSA") => json!([[2024, 2060.5, 2000], [2025, 2060.5, 2000], ["-", 5, 5]]),
                ("despesa", "numero_funcao") => {
                    if ano == Some("2026") {
                        assert!(meas[0].contains(",NUMERO_REMESSA={\"<=2\"}>}valor_liquidado"), "{}", meas[0]);
                    }
                    json!([[13, 392, "2050", " CARNAVAL DA FAMILIA ", 1000.5, 1200], ["8", "244", 2060, "BENEFICIOS EVENTUAIS", 1000, 1000], [13, 392, "2050", "CARNAVAL", 60, 60], [4, 122, 2001, "ZERADA", 0, null]])
                }
                ("receita", _) => json!([["1112", 5000, null], [1751, 900.5, -100.5], ["7218", 40, 0], [1321, 0, -3]]),
                x => return Err(format!("consulta inesperada {x:?}")),
            }
            .to_string())
        }
    }

    fn setores_falso(saida: &Path) -> Result<(), String> {
        let mut st = serde_json::Map::new();
        for e in std::fs::read_dir(saida).unwrap().flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if let Some(k) = n.strip_prefix("acoes-").and_then(|k| k.strip_suffix(".json")) {
                let v: Value = serde_json::from_str(&std::fs::read_to_string(e.path()).unwrap()).unwrap();
                let s: f64 = v.as_array().unwrap().iter().filter(|x| base::eh_festa(x["acao"].as_str().unwrap(), None)).map(|x| x["liquidado"].as_f64().unwrap()).sum();
                st.insert(k.into(), json!({ "Festas e eventos": { "liquidado": s } }));
            }
        }
        std::fs::write(saida.join("setores.json"), Value::Object(st).to_string()).map_err(|e| e.to_string())
    }

    fn pasta(nome: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("braniac-link_tribunais_ac-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    #[test]
    fn situacao_das_remessas_e_leitura() {
        let s = situacao(&painel(3, 2), "RIO BRANCO").unwrap();
        assert_eq!((s.mes26, s.faltam.len(), s.totais[&2025], s.por.len()), (Some(2), 0, (2060.5, 2000.0), 4)); // o fundo novo (sem 2025 inteiro) nao segura o corte
        let s = situacao(&painel(0, 0), "RIO BRANCO").unwrap();
        assert_eq!((s.mes26, s.faltam), (None, vec!["CÂMARA MUNICIPAL".to_string(), "PREFEITURA DE RIO BRANCO".to_string()]));
        assert_eq!(entes(&painel(0, 0)).unwrap(), ["RIO BRANCO", "XAPURI"]);
        let d = ler(&painel(3, 2), &base::ANOS, "RIO BRANCO", Some(2)).unwrap();
        let a = &d["2026"];
        assert_eq!(a.acoes.len(), 2); // a zerada sai; o mesmo codigo soma
        let x = &a.acoes[0];
        assert_eq!((x.acao.as_str(), x.cod.as_str(), x.f.as_str(), x.sub.as_str(), x.liquidado, x.empenhado), ("CARNAVAL DA FAMILIA", "0.0.13.392.0.2050", "13", "392", 1060.5, 1260.0));
        assert_eq!(a.receita, [("Impostos, taxas e contribuições de melhoria".to_string(), 5000.0), ("Transferências correntes".to_string(), 900.5), ("Deduções da receita (Fundeb)".to_string(), -100.5), (u::INTRA.to_string(), 40.0), ("Outras deduções da receita".to_string(), -3.0)]);
        assert_eq!((a.arrecadado, a.liquidado, a.doc_receita.as_str()), (5837.0, 2060.5, "TCE-AC painel Qlik · remessas de receita 2026"));
    }

    #[test]
    fn video_normal_pauta_ou_pendencia() {
        // todos entregaram ate fevereiro: video normal, sem "previa"
        let p = pasta("normal");
        let c = preparar(&painel(3, 2), &setores_falso, "9", "Rio Branco", &p, IBGE_RIO_BRANCO).unwrap();
        assert!(c["sem_previsto"] == true && c["meses"]["2026"] == "02" && c["ausentes"] == json!([]) && c["anos"]["2026"]["gastou"] == 2060.5);
        // a Prefeitura e a Camara nao mandaram 2026: video de pauta, 2026 nao e lido
        let c = preparar(&painel(0, 0), &|_| Ok(()), "9", "Rio Branco", &p, IBGE_RIO_BRANCO).unwrap();
        assert_eq!(c["ausencia"]["2026"]["dez"], "só remessas de janeiro e fevereiro de alguns fundos e autarquias, e nada da Prefeitura nem da Câmara");
        assert!(c["ausentes"] == json!(["2026"]) && c["meses"]["2026"] == "02" && c["anos"]["2026"]["gastou"] == 0.0);
        // so a Camara falta: pendencia, nada estimado
        assert!(preparar(&painel(3, 0), &|_| Ok(()), "9", "Rio Branco", &p, IBGE_RIO_BRANCO).unwrap_err().contains("a Prefeitura mandou remessa; faltam CÂMARA MUNICIPAL"));
        // pauta, mas as acoes nao fecham com o painel de remessas
        let outro = |app: &str, dims: &[&str], meas: &[String]| painel(0, 0)(app, dims, meas).map(|t| if dims[0] == "ANO_REMESSA" { t.replace("[2025,2060.5", "[2025,1400.5") } else { t });
        assert_eq!(preparar(&outro, &|_| Ok(()), "9", "Rio Branco", &p, IBGE_RIO_BRANCO).unwrap_err(), "2025: ações 2060.50 != remessas 1400.50");
        assert!(preparar(&painel(0, 0), &|_| Ok(()), "9", "Cruzeiro do Sul", &p, 1).unwrap_err().contains("não aparece no painel"));
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn recusa_o_que_falta() {
        let troca = |de: &'static str, para: &'static str| move |app: &str, dims: &[&str], meas: &[String]| painel(3, 2)(app, dims, meas).map(|t| t.replace(de, para));
        assert!(ler(&troca("\"1112\"", "\"3112\""), &["2024"], "RIO BRANCO", None).unwrap_err().contains("sem origem conhecida: 3112"));
        assert!(ler(&troca("1000.5", "\"-\""), &["2024"], "RIO BRANCO", None).unwrap_err().contains("valor que não é número"));
        assert!(ler(&troca(",\"BENEFICIOS EVENTUAIS\"", ""), &["2024"], "RIO BRANCO", None).unwrap_err().contains("fora do formato"));
        assert!(ler(&troca("\"8\"", "\"oito\""), &["2024"], "RIO BRANCO", None).unwrap_err().contains("função que não é número"));
        assert!(situacao(&|_, _, _| Ok("null".into()), "X").is_err() && situacao(&|_, _, _| Err("socket caiu".into()), "X").is_err());
    }
}
