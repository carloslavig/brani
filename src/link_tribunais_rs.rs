//! Molde do Rio Grande do Sul (link/tribunais/rs.py): balancetes de despesa e de receita do Portal de Dados Abertos do TCE-RS,
//! um CSV por orgao do municipio e por ano. Aqui so o parse do que ja foi baixado. 2026 vale no ultimo mes que TODOS os
//! orgaos entregaram (o balancete e acumulado ate o mes): orgao fora do corte, rubricas que nao fecham ou arquivo que falta
//! = Err (pendencia, nunca video).
//! Entra como closure: `texto(tipo, ano, orgao)` devolve o CSV, ou None se o tribunal nao tem aquele arquivo.
//! Diferenca do Python (mais rigoroso): balancete de DESPESA que falta para um orgao e erro (la o orgao era pulado calado);
//! so a Camara pode ficar sem balancete de receita (ela nao tem receita orcamentaria).
#![allow(dead_code)]
use crate::link_tribunais_base::{self as base, col, csv, valor, Acao, Ano, Dados, Linha};
use crate::texto_pt::sem_acento;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const IBGE: i64 = 4314902;
/// Orgaos da capital no tribunal (codigo, nome).
pub const ORGAOS: [(&str, &str); 8] = [("54900", "Prefeitura"), ("54901", "Câmara"), ("54907", "DMAE"), ("54908", "DMLU"), ("54909", "DEMHAB"), ("54910", "FASC"), ("88152", "PREVIMPA"), ("87056", "EPTC")];
pub const CAMARA: &str = "54901";
/// A acao de eventos da secretaria de turismo e eventos.
pub const FESTAS_EXTRA: &str = "LICENCIAMENTO DE EVENTOS";
/// A capital nao tem "beneficios eventuais" com esse nome: a acao equivalente (regex, nome real).
pub const SOCIAL: (&str, &str) = ("BENEF[IÍ]CIOS? ASSISTENCIA", "benefícios assistenciais");
pub type Texto<'a> = &'a dyn Fn(&str, &str, &str) -> Result<Option<String>, String>;

pub fn tribunal() -> Value {
    json!({ "sigla": "TCE-RS", "fala": "Tribunal de Contas do Estado", "uf": "RS", "estado": "Rio Grande do Sul", "doc_despesa": "TCE-RS BALANCETE DE DESPESA", "doc_receita": "TCE-RS BALANCETE DE RECEITA",
        "tribunal_estado": "TCE-RS (Tribunal de Contas do Estado do Rio Grande do Sul)", "site_tribunal": "dados.tce.rs.gov.br",
        "fonte_md": "Tudo do Portal de Dados Abertos do TCE-RS (dados.tce.rs.gov.br), balancetes do SIAPC enviados por cada órgão do município (Prefeitura, Câmara, DMAE, DMLU, DEMHAB, FASC, PREVIMPA e EPTC): Balancete de Receita (arrecadado por rubrica, já líquido das restituições e da dedução para formação do Fundeb) e Balancete de Despesa (dotação inicial, créditos, empenhado e liquidado por projeto/atividade). \"Gastou\" = LIQUIDADO. \"Previa\" = dotação inicial. Intraorçamentárias (contribuições e aportes ao PREVIMPA) contadas na receita e na despesa.",
        "setores_md": "Setores = soma do liquidado por projeto/atividade (balancetes de despesa do TCE-RS), pela função; festas e eventos separados da cultura; segurança pública fora.",
        "pendencias_md": [], "orgaos": "Prefeitura, Câmara, autarquias (DMAE, DMLU, DEMHAB, PREVIMPA), FASC e EPTC (Procempa, Carris e consórcios fora)",
        "legenda_docs": ["• Receita: TCE-RS, Dados Abertos, Balancete de Receita de 2024, 2025 e 2026 (líquido do Fundeb)", "• Despesa, setores e festas: TCE-RS, Dados Abertos, Balancete de Despesa por projeto/atividade", "• dados.tce.rs.gov.br"] })
}

fn origem(conta: &str) -> &'static str {
    match conta.get(..2).unwrap_or("") {
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
        _ => "Outras receitas",
    }
}

fn mes(r: &Linha) -> Result<u32, String> {
    col(r, "MES")?.trim().parse().map_err(|_| "MES que não é número".to_string())
}

/// Le os 3 anos de todos os orgaos. Devolve os dados no formato da base e o mes de corte de 2026 ("08").
pub fn ler(texto: Texto) -> Result<(Dados, String), String> {
    let despesa = |a: &str, o: &str| -> Result<Vec<Linha>, String> {
        let t = texto("despesa", a, o)?.ok_or_else(|| format!("{o} {a}: falta o balancete de despesa do órgão no TCE-RS"))?;
        Ok(csv(&t, ','))
    };
    // 2026: ultimo mes que TODOS os orgaos entregaram
    let mut meses = BTreeSet::new();
    for (o, _) in ORGAOS {
        if let Some(r) = despesa("2026", o)?.first() {
            meses.insert(mes(r)?);
        }
    }
    let mes26 = *meses.first().ok_or("2026: nenhum órgão com balancete de despesa no TCE-RS")?;
    let mut dados = Dados::new();
    for a in base::ANOS {
        let (mut acoes, mut onde): (Vec<Acao>, BTreeMap<[String; 7], usize>) = Default::default();
        let mut orig: Vec<(String, f64)> = vec![];
        for (o, _) in ORGAOS {
            let linhas = despesa(a, o)?;
            for r in &linhas {
                if col(r, "ANO")? != a || (a == "2026" && mes(r)? != mes26) {
                    return Err(format!("{o} {a}: balancete fora do corte; baixe de novo quando todos os órgãos estiverem no mesmo mês"));
                }
                let k = [o, col(r, "CD_ORGAO_ORCAMENTARIO")?, col(r, "CD_UNIDADE_ORCAMENTARIA")?, col(r, "CD_FUNCAO")?, col(r, "CD_SUBFUNCAO")?, col(r, "CD_PROGRAMA")?, col(r, "CD_PROJETO")?].map(String::from);
                let fun = format!("{:0>2}", k[3]);
                let i = match onde.get(&k) {
                    Some(i) => *i,
                    None => {
                        // funcao Legislativa fora da Camara nao entra como Camara
                        acoes.push(Acao { f: if fun == "01" && o != CAMARA { "0".into() } else { fun }, sub: k[4].clone(), cod: k.join("."), acao: col(r, "NM_PROJETO")?.trim().into(), ..Default::default() });
                        onde.insert(k, acoes.len() - 1);
                        acoes.len() - 1
                    }
                };
                let ini = valor(r, "VL_DOTACAO_INICIAL")?;
                let mut atual = ini;
                for (k, sinal) in [("VL_ATUALIZACAO_MONETARIA", 1.0), ("VL_CREDITOS_SUPLEMENTARES", 1.0), ("VL_CREDITOS_ESPECIAIS", 1.0), ("VL_CREDITOS_EXTRAORDINARIOS", 1.0), ("VL_REDUCAO_DOTACAO", -1.0), ("VL_SUPLEMENTACAO_RECURSO", 1.0), ("VL_REDUCAO_RECURSO", -1.0), ("VL_TRANSFERENCIA", 1.0), ("VL_TRANSPOSICAO", 1.0), ("VL_REMANEJAMENTO", 1.0)] {
                    atual += sinal * valor(r, k)?;
                }
                let x = &mut acoes[i];
                x.orcado += ini;
                x.atualizado += atual;
                x.empenhado += valor(r, "VL_EMPENHADO")?;
                x.liquidado += valor(r, "VL_LIQUIDADO")?;
            }
            let rec = match texto("receita", a, o)? {
                Some(t) => csv(&t, ','),
                None if o == CAMARA => vec![], // a Camara nao tem receita orcamentaria
                None => return Err(format!("{o} {a}: falta o balancete de receita do órgão no TCE-RS")),
            };
            let (mut soma, mut topo) = (0.0, 0.0);
            for r in &rec {
                let v = valor(r, "VL_ARRECADADO")?;
                match col(r, "NIVEL")? {
                    "S" if col(r, "NR_NIVEL")? == "1" => topo += v,
                    "A" => {
                        soma += v;
                        let c = col(r, "CD_CONTA_SG")?;
                        let nome = if col(r, "CAR_PECULIAR")? == "105" { "Deduções da receita (Fundeb)" } else if c.starts_with(['7', '8']) { "Receitas intraorçamentárias" } else { origem(c) };
                        match orig.iter_mut().find(|(n, _)| n == nome) {
                            Some(x) => x.1 += v,
                            None => orig.push((nome.into(), v)),
                        }
                    }
                    _ => {}
                }
            }
            if (soma - topo).abs() >= 1.0 {
                return Err(format!("{o} {a}: rubricas analíticas não fecham com as categorias"));
            }
        }
        let arrecadado = orig.iter().map(|o| o.1).sum();
        orig.retain(|o| o.1.abs() >= 0.01);
        orig.sort_by(|x, y| y.1.total_cmp(&x.1));
        dados.insert(
            a.into(),
            Ano {
                arrecadado,
                liquidado: acoes.iter().map(|x| x.liquidado).sum(),
                empenhado: acoes.iter().map(|x| x.empenhado).sum(),
                doc_despesa: format!("TCE-RS Dados Abertos · balancete-despesa/{a} (8 órgãos)"),
                doc_receita: format!("TCE-RS Dados Abertos · balancete-receita/{a} (7 órgãos)"),
                acoes,
                receita: orig,
            },
        );
    }
    Ok((dados, format!("{mes26:02}")))
}

/// O que a esteira chama. O molde so tem os codigos de orgao da capital: outra cidade e recusada.
pub fn preparar(texto: Texto, setores: &dyn Fn(&Path) -> Result<(), String>, numero: &str, nome: &str, pasta: &Path) -> Result<Value, String> {
    if sem_acento(nome) != "porto alegre" {
        return Err("molde do RS testado só com a capital; outra cidade precisa dos códigos de órgão dela".into());
    }
    let (dados, mes26) = ler(texto)?;
    base::montar(pasta, numero, nome, "RS", IBGE, &tribunal(), &dados, &mes26, &base::Opcoes { festas_extra: Some(FESTAS_EXTRA), social: Some(SOCIAL), ..Default::default() }, setores)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAB_D: &str = "ANO,MES,CD_ORGAO_ORCAMENTARIO,CD_UNIDADE_ORCAMENTARIA,CD_FUNCAO,CD_SUBFUNCAO,CD_PROGRAMA,CD_PROJETO,NM_PROJETO,VL_DOTACAO_INICIAL,VL_ATUALIZACAO_MONETARIA,VL_CREDITOS_SUPLEMENTARES,VL_CREDITOS_ESPECIAIS,VL_CREDITOS_EXTRAORDINARIOS,VL_REDUCAO_DOTACAO,VL_SUPLEMENTACAO_RECURSO,VL_REDUCAO_RECURSO,VL_TRANSFERENCIA,VL_TRANSPOSICAO,VL_REMANEJAMENTO,VL_EMPENHADO,VL_LIQUIDADO";
    const CAB_R: &str = "NIVEL,NR_NIVEL,CD_CONTA_SG,CAR_PECULIAR,VL_ARRECADADO";

    fn despesa(a: &str, mes: u32) -> String {
        format!("{CAB_D}\n{a},{mes},10,1,1,31,5,2001,\" Atividade legislativa \",100,0,50,0,0,20,0,0,0,0,0,90,80\n{a},{mes},10,1,1,31,5,2001,Atividade legislativa,10,,,,,,,,,,,5,5\n{a},{mes},20,2,13,392,7,2002,\"Captação, fomento e licenciamento de eventos\",40,0,0,0,0,0,0,0,0,0,0,30,20")
    }

    fn receita(extra: f64) -> String {
        format!("{CAB_R}\nS,1,1000000,000,{}\nS,2,1100000,000,300\nA,5,1112000,000,300\nA,5,1711000,000,700\nA,5,1751000,105,-100\nA,5,7211000,000,100\nA,5,3311000,000,0.001", 1000.001 + extra)
    }

    fn fonte(mes_de: impl Fn(&str) -> u32) -> impl Fn(&str, &str, &str) -> Result<Option<String>, String> {
        move |tipo: &str, a: &str, o: &str| Ok(match tipo {
            "despesa" => Some(despesa(a, if a == "2026" { mes_de(o) } else { 12 })),
            _ if o == CAMARA => None,
            _ => Some(receita(0.0)),
        })
    }

    #[test]
    fn le_todos_os_orgaos_no_mesmo_corte() {
        let (dados, m) = ler(&fonte(|_| 8)).unwrap();
        assert_eq!(m, "08");
        let a = &dados["2026"];
        assert_eq!(a.acoes.len(), 16); // 2 acoes por orgao, 8 orgaos
        let (pref, cam) = (&a.acoes[0], &a.acoes[2]);
        assert_eq!((pref.f.as_str(), cam.f.as_str(), a.acoes[1].f.as_str()), ("0", "01", "13")); // Legislativa so conta como Camara na Camara
        assert_eq!((cam.acao.as_str(), cam.cod.as_str(), cam.sub.as_str()), ("Atividade legislativa", "54901.10.1.1.31.5.2001", "31"));
        assert_eq!((cam.orcado, cam.atualizado, cam.empenhado, cam.liquidado), (110.0, 140.0, 95.0, 85.0));
        assert!(base::eh_festa(&a.acoes[1].acao, Some(&regex::Regex::new(&format!("(?i){FESTAS_EXTRA}")).unwrap())));
        assert_eq!((a.liquidado, a.empenhado), (8.0 * 105.0, 8.0 * 125.0));
        assert!((a.arrecadado - 7.0 * 1000.001).abs() < 1e-6); // 7 orgaos com receita (a Camara nao tem)
        let nomes: Vec<&str> = a.receita.iter().map(|x| x.0.as_str()).collect();
        assert_eq!(nomes, ["Transferências correntes", "Impostos, taxas e contribuições de melhoria", "Receitas intraorçamentárias", "Deduções da receita (Fundeb)"]); // origem abaixo de 1 centavo nao entra
        assert_eq!(a.receita[3].1, -700.0);
    }

    #[test]
    fn recusa_levantamento_pela_metade() {
        // um orgao ainda em julho: o corte e julho e os outros (em agosto) estao fora dele
        assert!(ler(&fonte(|o| if o == "88152" { 7 } else { 8 })).unwrap_err().contains("balancete fora do corte"));
        // arquivo de outro ano
        let trocado = |tipo: &str, a: &str, o: &str| fonte(|_| 8)(tipo, if tipo == "despesa" && a == "2025" && o == "54907" { "2024" } else { a }, o);
        assert!(ler(&trocado).unwrap_err().contains("54907 2025: balancete fora do corte"));
        // orgao sem balancete de despesa, ou orgao (que nao a Camara) sem receita
        let sem = |tipo: &str, a: &str, o: &str| if tipo == "despesa" && a == "2024" && o == "54910" { Ok(None) } else { fonte(|_| 8)(tipo, a, o) };
        assert!(ler(&sem).unwrap_err().contains("54910 2024: falta o balancete de despesa"));
        let sem = |tipo: &str, a: &str, o: &str| if tipo == "receita" && o == "54900" { Ok(None) } else { fonte(|_| 8)(tipo, a, o) };
        assert!(ler(&sem).unwrap_err().contains("54900 2024: falta o balancete de receita"));
        // rubricas que nao fecham com o total da categoria
        let aberto = |tipo: &str, a: &str, o: &str| if tipo == "receita" && o == "54907" { Ok(Some(receita(50.0))) } else { fonte(|_| 8)(tipo, a, o) };
        assert!(ler(&aberto).unwrap_err().contains("54907 2024: rubricas analíticas não fecham"));
        assert!(ler(&|_, _, _| Ok(Some(CAB_D.to_string()))).unwrap_err().contains("nenhum órgão com balancete"));
        assert!(ler(&|_, _, _| Err("download falhou".into())).is_err());
        let p = std::env::temp_dir().join(format!("braniac-link_tribunais_rs-{}", std::process::id()));
        assert!(preparar(&fonte(|_| 8), &|_| Ok(()), "1", "Pelotas", &p).unwrap_err().contains("só com a capital"));
        assert!(!p.exists());
    }
}
