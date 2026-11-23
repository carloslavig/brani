//! Video de prefeitura de Mato Grosso (link/radar_mt.py): os numeros vem do Radar do TCE-MT (painel de consulta), nunca de
//! outra fonte. Aqui: as consultas que o painel recebe, o mes de corte de 2026 (o ultimo que TODOS os orgaos entregaram), a
//! extracao (retoma o que ja veio) e a montagem de saida/cidade.json, acoes-*.json e receita-*.json.
//! Regras: "gastou" = LIQUIDADO; consolidado (prefeitura + Camara + autarquias + previdencia); o Radar nao tem a dotacao por
//! acao, entao `sem_previsto` e o achado sai sem "previa". Ano que a Prefeitura nao entregou ao tribunal: 2026 vira pauta;
//! ano fechado e Err (pendencia). Nunca se completa com outra fonte. Soma que nao fecha = Err.
//! Entram como closure: a consulta ao painel (`painel(site, dimensoes, medidas, n) -> {"linhas": [...], "total": n}`; aba,
//! pausa e firewall sao de quem chama) e o script de setores. Ficou de fora: `ibge_de` (rede; ha `ibge_cidades`) e o `cli`.
#![allow(dead_code)]
use crate::link_auditar::br;
use crate::link_tribunais_base::{eh_festa, r2, redondo, slug, ABR, ANOS, NM};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const SITE_DESPESA: &str = "radardespesa.tce.mt.gov.br";
pub const SITE_RECEITA: &str = "radarreceita.tce.mt.gov.br";
/// (site, dimensoes, medidas, maximo de linhas) -> {"linhas": [[...]], "total": linhas que existem}.
pub type Painel<'a> = &'a dyn Fn(&str, &[String], &[String], usize) -> Result<Value, String>;
type Consulta = (String, Vec<String>, Vec<String>, usize);

pub fn tribunal() -> Value {
    json!({ "sigla": "TCE-MT", "fala": "Tribunal de Contas do Estado", "uf": "MT", "estado": "Mato Grosso", "doc_despesa": "RADAR DESPESA", "doc_receita": "RADAR RECEITA",
        "fonte_md": "Tudo do Radar de Controle Público do TCE-MT (radardespesa.tce.mt.gov.br e radarreceita.tce.mt.gov.br), dados que a prefeitura, a Câmara, as autarquias e o fundo de previdência mandam ao Tribunal. \"Gastou\" = LIQUIDADO. Receita = arrecadada (já líquida da dedução do Fundeb)." })
}

fn meses(ate: usize) -> Vec<String> {
    (1..=ate).map(|m| format!("{m:02}")).collect()
}

// o set analysis {1<...>} ignora o filtro que a pagina abre (exercicio atual)
fn filtro(mun: &str, ano: &str, ms: &[String]) -> String {
    format!("{{1<Município={{'{mun}'}}, [Orçamento Fiscal e da Seguridade Social – OFSS]={{'Sim'}}, Exercício={{{ano}}}, [Mês Referência]={{{}}}>}}", ms.iter().map(|m| format!("'{m}'")).collect::<Vec<_>>().join(","))
}

fn textos(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

pub fn consultas_despesa(mun: &str, meses_ano: &BTreeMap<&str, Vec<String>>) -> Vec<Consulta> {
    let mut q = vec![];
    for (a, ms) in meses_ano {
        let f = filtro(mun, a, ms);
        let (liq, emp) = (format!("Sum({f}[Valor Liquidado])"), format!("Sum({f}[Valor Empenhado])"));
        q.push((format!("acoes-{a}"), textos(&["Código Jurisdicionado", "Código Função", "Código Sub Função", "Número Projeto Atividade", "Projeto Atividade Descrição"]), vec![liq.clone(), emp.clone()], 1400));
        q.push((format!("total-{a}"), textos(&["Município"]), vec![liq.clone(), emp.clone(), format!("Sum({f}[Valor Pago])")], 1000));
        q.push((format!("orgaos-{a}"), textos(&["Jurisdicionado"]), vec![liq, emp], 50));
    }
    q
}

pub fn consultas_receita(mun: &str, meses_ano: &BTreeMap<&str, Vec<String>>) -> Vec<Consulta> {
    let mut q = vec![];
    for (a, ms) in meses_ano {
        let arr = vec![format!("Sum({}[Valor Receita Arrecadada])", filtro(mun, a, ms))];
        q.push((format!("grupos-{a}"), textos(&["Grupo de Receita"]), arr.clone(), 100));
        q.push((format!("origens-{a}"), textos(&["Origem"]), arr.clone(), 100));
        q.push((format!("total-{a}"), textos(&["Município"]), arr, 1000));
    }
    q
}

fn num(v: &Value) -> f64 {
    v.as_f64().or_else(|| v.as_str().and_then(|t| t.trim().parse().ok())).unwrap_or(0.0)
}

fn linhas(v: &Value) -> &[Value] {
    v.as_array().map_or(&[], |a| a.as_slice())
}

/// Ultimo mes de 2026 que todos os orgaos entregaram (um orgao so ate julho puxa o corte para julho).
pub fn ultimo_mes_comum(linhas_meses: &[Value], ano: &str) -> Option<String> {
    let mut por: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for l in linhas_meses {
        let ex = l[0].as_str().map(String::from).unwrap_or_else(|| l[0].to_string());
        if ex.split('.').next() == Some(ano) && num(&l[3]) != 0.0 {
            let mes = match &l[2] {
                Value::Number(n) => format!("{:02}", n.as_f64().unwrap_or(0.0) as i64),
                m => match m.as_str().unwrap_or("").parse::<f64>() {
                    Ok(n) => format!("{:02}", n as i64),
                    Err(_) => m.as_str().unwrap_or("").to_string(),
                },
            };
            por.entry(l[1].as_str().map(String::from).unwrap_or_else(|| l[1].to_string())).or_default().insert(mes);
        }
    }
    let mut it = por.values();
    let primeiro = it.next()?.clone();
    it.fold(primeiro, |c, s| c.intersection(s).cloned().collect()).into_iter().max()
}

fn rodar(painel: Painel, site: &str, consultas: &[Consulta]) -> Result<Map<String, Value>, String> {
    let mut out = Map::new();
    for (nome, dims, meas, n) in consultas {
        let r = painel(site, dims, meas, *n)?;
        let ls = r["linhas"].as_array().ok_or_else(|| format!("{nome}: o painel respondeu sem linhas"))?;
        // veio cortado: usar seria somar pela metade
        if r["total"].as_u64().is_none_or(|t| t as usize > ls.len()) {
            return Err(format!("{nome}: {} linhas, só vieram {} (dividir a consulta)", r["total"], ls.len()));
        }
        out.insert(nome.clone(), json!(ls));
    }
    Ok(out)
}

fn jler(arq: &Path) -> Result<Value, String> {
    std::fs::read_to_string(arq).map_err(|e| format!("{}: {e}", arq.display())).and_then(|t| serde_json::from_str(&t).map_err(|e| format!("{}: {e}", arq.display())))
}

fn jgravar(arq: &Path, v: &Value) -> Result<(), String> {
    std::fs::write(arq, serde_json::to_string_pretty(v).map_err(|e| e.to_string())?).map_err(|e| format!("{}: {e}", arq.display()))
}

/// Bruto do Radar em <pasta>/radar/{despesa,receita}.json. Retoma: o que ja veio nao e consultado de novo. Devolve o mes de corte.
pub fn extrair(painel: Painel, municipio: &str, pasta: &Path) -> Result<String, String> {
    let radar = pasta.join("radar");
    std::fs::create_dir_all(&radar).map_err(|e| e.to_string())?;
    let (arq_d, arq_r) = (radar.join("despesa.json"), radar.join("receita.json"));
    let meses_de = |m26: &str| -> Result<BTreeMap<&str, Vec<String>>, String> {
        let m: usize = m26.parse().ok().filter(|m| (1..=12).contains(m)).ok_or_else(|| format!("mês de corte inválido: {m26:?}"))?;
        Ok(BTreeMap::from([("2024", meses(12)), ("2025", meses(12)), ("2026", meses(m))]))
    };
    if !arq_d.exists() {
        let medida = vec![format!("Sum({{1<Município={{'{municipio}'}}, Exercício={{2024,2025,2026}}>}}[Valor Liquidado])")];
        let mm = rodar(painel, SITE_DESPESA, &[("meses".into(), textos(&["Exercício", "Jurisdicionado", "Mês Referência"]), medida, 600)])?.remove("meses").unwrap_or_default();
        let m26 = ultimo_mes_comum(linhas(&mm), "2026").ok_or("nenhum mês de 2026 que todos os órgãos entregaram ao tribunal")?;
        let mut desp = rodar(painel, SITE_DESPESA, &consultas_despesa(municipio, &meses_de(&m26)?))?;
        desp.insert("meses_por_orgao".into(), mm);
        desp.insert("mes26".into(), json!(m26));
        jgravar(&arq_d, &Value::Object(desp))?;
    }
    let m26 = jler(&arq_d)?["mes26"].as_str().ok_or("despesa.json sem o mês de corte")?.to_string();
    if !arq_r.exists() {
        let mut rec = rodar(painel, SITE_RECEITA, &consultas_receita(municipio, &meses_de(&m26)?))?;
        rec.insert("mes26".into(), json!(m26));
        jgravar(&arq_r, &Value::Object(rec))?;
    }
    Ok(m26)
}

fn inteiro(v: &Value) -> i64 {
    num(v) as i64
}

fn capa_mi(v: f64) -> String {
    let t = if v >= 1e6 { format!("R$ {:.2} mi.", v / 1e6) } else { format!("R$ {:.1} mil.", v / 1e3) };
    t.replacen('.', ",", 1)
}

/// A linha com valor (a dimensao lista todos os municipios; o filtro zera os outros).
fn com_valor<'a>(ls: &'a Value, nome: &str) -> Result<&'a Value, String> {
    linhas(ls).iter().fold(None, |m: Option<&Value>, l| if m.is_none_or(|m| num(&l[1]) > num(&m[1])) { Some(l) } else { m }).ok_or_else(|| format!("{nome}: o Radar não trouxe o total"))
}

/// Monta os arquivos que o gerador entende a partir do bruto do Radar. `pilha_mi`: valor fixo da pilha (None = calcula).
pub fn montar(pasta: &Path, numero: &str, nome: &str, ibge: i64, pilha_mi: Option<f64>, setores: &dyn Fn(&Path) -> Result<(), String>) -> Result<Value, String> {
    let (d, r) = (jler(&pasta.join("radar").join("despesa.json"))?, jler(&pasta.join("radar").join("receita.json"))?);
    let m26 = d["mes26"].as_str().ok_or("despesa.json sem o mês de corte")?;
    let i26: usize = m26.parse().ok().filter(|m| (1..=12).contains(m)).ok_or_else(|| format!("mês de corte inválido: {m26:?}"))?;
    let saida = pasta.join("saida");
    std::fs::create_dir_all(&saida).map_err(|e| e.to_string())?;
    let sg = slug(nome);
    let benef = Regex::new(r"(?i)BENEF[IÍ]CIOS EVENTUAIS").expect("regex fixa");
    let (mut anos, mut festas, mut ffonte, mut bn) = (Map::new(), Map::new(), Map::new(), Map::new());
    let (mut totais, mut aus): (Vec<Value>, Vec<&str>) = (vec![], vec![]);
    let (mut maior, mut festa_max) = (0.0f64, 0.0f64);
    for a in ANOS {
        // o que nao foi entregue ao tribunal vira pauta, nunca se completa com outra fonte
        let orgaos = &d[format!("orgaos-{a}")];
        if !orgaos.is_null() && !linhas(orgaos).iter().any(|o| o[0].as_str().unwrap_or("").to_uppercase().contains("PREFEITURA")) {
            if a != "2026" {
                return Err(format!("a Prefeitura de {nome} não enviou ao TCE-MT os dados de {a} (Radar Despesa só tem outros órgãos): pela regra do guia isso é pauta (prestação de contas que falta), não número de vídeo"));
            }
            // ano corrente ausente: video de pauta; zeros so para nao quebrar contas (nada disso e dito)
            aus.push(a);
            anos.insert(a.into(), json!({ "arrec": 0, "gastou": 0, "emp": 0 }));
            festas.insert(a.into(), json!({ "prev": 0, "gasto": 0 }));
            ffonte.insert(a.into(), json!([]));
            bn.insert(a.into(), json!(0));
            for e in std::fs::read_dir(&saida).into_iter().flatten().flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                if n.contains(&format!("-{a}")) && n.ends_with(".json") {
                    let _ = std::fs::remove_file(e.path()); // sobra de rodada antiga nao pode virar setor
                }
            }
            continue;
        }
        let mes = if a == "2026" { m26 } else { "12" };
        let ate = format!("jan a {}/{a}", ABR[if a == "2026" { i26 - 1 } else { 11 }]);
        let tot = com_valor(&d[format!("total-{a}")], &format!("despesa de {a}"))?;
        let (liq, emp) = (num(&tot[1]), num(&tot[2]));
        let arrec = num(&com_valor(&r[format!("total-{a}")], &format!("receita de {a}"))?[1]);
        let acoes = linhas(&d[format!("acoes-{a}")]);
        let soma: f64 = acoes.iter().map(|x| num(&x[5])).sum();
        if (soma - liq).abs() >= 0.05 {
            return Err(format!("{a}: soma das ações {soma:.2} != total {liq:.2}"));
        }
        anos.insert(a.into(), json!({ "arrec": r2(arrec), "gastou": r2(liq), "emp": r2(emp) }));
        maior = maior.max(r2(arrec));
        // cod no formato que o script de setores le: a subfuncao na 4a posicao
        let js: Vec<Value> = acoes
            .iter()
            .filter(|x| num(&x[5]) != 0.0 || num(&x[6]) != 0.0)
            .map(|x| {
                let proj = x[3].as_str().map(String::from).unwrap_or_else(|| x[3].to_string());
                json!({ "f": inteiro(&x[1]).to_string(), "cod": format!("{}.0.{}.{}.0.{proj}", inteiro(&x[0]), inteiro(&x[1]), inteiro(&x[2])), "acao": x[4], "liquidado": r2(num(&x[5])), "pag": 0 })
            })
            .collect();
        jgravar(&saida.join(format!("acoes-{a}-{mes}.json")), &json!(js))?;
        let nome_de = |x: &Value| x["acao"].as_str().unwrap_or("").to_string();
        let fs: Vec<&Value> = js.iter().filter(|x| eh_festa(&nome_de(x), None)).collect();
        let gasto = r2(fs.iter().map(|x| num(&x["liquidado"])).sum());
        festa_max = festa_max.max(gasto);
        festas.insert(a.into(), json!({ "prev": 0, "gasto": gasto }));
        ffonte.insert(a.into(), json!(fs.iter().map(|x| json!({ "acao": x["cod"], "nome": x["acao"], "pag": 0, "arquivo": SITE_DESPESA, "como": "nome da ação" })).collect::<Vec<_>>()));
        bn.insert(a.into(), json!(r2(js.iter().filter(|x| benef.is_match(&nome_de(x))).map(|x| num(&x["liquidado"])).sum())));
        let g: Vec<(String, f64)> = linhas(&r[format!("origens-{a}")]).iter().filter(|o| num(&o[1]) != 0.0).map(|o| (o[0].as_str().map(String::from).unwrap_or_else(|| o[0].to_string()), r2(num(&o[1])))).collect();
        if (g.iter().map(|o| o.1).sum::<f64>() - arrec).abs() >= 0.05 {
            return Err(format!("{a}: origens da receita não fecham com o total"));
        }
        jgravar(&saida.join(format!("receita-{a}.json")), &json!(g.iter().map(|(n, v)| json!([n, v])).collect::<Vec<_>>()))?;
        let rot = if a == "2026" { format!("2026 até {mes}") } else { a.to_string() };
        let alto = nome.to_uppercase();
        totais.push(json!({ "dito": format!("Arrecadou ({rot})"), "valor": r2(arrec), "arquivo": SITE_RECEITA, "onde": format!("Município {alto} · OFSS · {ate} · Valor Receita Arrecadada") }));
        totais.push(json!({ "dito": format!("Gastou ({rot})"), "valor": r2(liq), "arquivo": SITE_DESPESA, "onde": format!("Município {alto} · OFSS · {ate} · Valor Liquidado (soma de {} ações)", acoes.len()) }));
        if a != "2026" {
            let dif = r2(arrec - liq);
            totais.push(json!({ "dito": format!("{} ({a})", if dif >= 0.0 { "Sobrou" } else { "Gastou a mais" }), "valor": dif.abs(), "arquivo": "conta própria", "onde": format!("conta própria: {} − {}", br(arrec), br(liq)) }));
        } else {
            totais.push(json!({ "dito": format!("Já empenhou ({rot})"), "valor": r2(emp), "arquivo": SITE_DESPESA, "onde": format!("{ate} · Valor Empenhado") }));
        }
        let linha = |dito: &str, x: &Value| json!({ "dito": format!("{dito} ({a})"), "valor": x["liquidado"], "arquivo": SITE_DESPESA, "onde": format!("{ate} · ação {} ({}) · Valor Liquidado", nome_de(x), x["cod"].as_str().unwrap_or("")) });
        totais.extend(fs.iter().map(|x| linha("Festas gastou", x)));
        totais.extend(js.iter().filter(|x| benef.is_match(&nome_de(x)) && num(&x["liquidado"]) != 0.0).map(|x| linha("Benefícios eventuais", x)));
    }
    let pilha = pilha_mi.unwrap_or_else(|| redondo(maior / 1e6, 45.0, &[1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0])); // ~45 pilhas no maior ano
    let trib = tribunal();
    let capa = if aus.is_empty() {
        json!(["Festas:", format!("R$ {:.2} mi", num(&festas["2026"]["gasto"]) / 1e6).replace('.', ",") + ".", "Ajuda às famílias:", capa_mi(num(&bn["2026"]))])
    } else {
        json!(["2026:", "contas", "não entregues", "ao TCE-MT."])
    };
    let mut cid = json!({
        "numero": numero, "nome": nome, "uf": "MT", "codigo": format!("orcamento{sg}mt"), "comp": format!("Orcamento{}{}Mt", sg.chars().take(1).collect::<String>().to_uppercase(), sg.chars().skip(1).collect::<String>()),
        "kit_pasta": format!("orcamento-{sg}-mt"), "arquivo_mapa": format!("{sg}.mp4"), "ibge": ibge, "sigla": format!("{}{numero}", sg.chars().take(3).collect::<String>().to_uppercase()),
        "mes26": ABR[i26 - 1], "mes26_nome": NM[i26 - 1], "pilha_mi": if pilha.fract() == 0.0 { json!(pilha as i64) } else { json!(pilha) }, "pilha_gaveta_mi": if festa_max < 3e6 { 0.1 } else { 0.2 },
        "anos": anos, "festas": festas, "festas_fonte": ffonte, "benef": bn, "tribunal": trib, "sem_previsto": true,
        "despesa_fonte": ANOS.iter().map(|a| (a.to_string(), json!({ "arquivo": SITE_DESPESA, "leitor": "radar_mt.py", "conferido": "soma das ações = total do Radar" }))).collect::<Map<_, _>>(),
        "capa": capa, "totais": totais, "ausentes": aus, "meses": { "2024": "12", "2025": "12", "2026": m26 }, "guia": [], "sem_balanco": [], "revisao_humana": [],
    });
    if !aus.is_empty() {
        cid["ausencia"] = json!({ "2026": { "dez": "nenhuma despesa da Prefeitura no Radar do TCE-MT" } });
    }
    jgravar(&saida.join("cidade.json"), &cid)?;
    setores(&saida)?;
    Ok(cid)
}

/// O que a esteira chama para uma cidade de MT (serie 900): Radar -> arquivos do gerador. `ibge` vem de quem chama.
pub fn preparar(painel: Painel, setores: &dyn Fn(&Path) -> Result<(), String>, numero: &str, nome: &str, ibge: i64, pasta: &Path) -> Result<Value, String> {
    let mun: String = crate::texto_pt::sem_acento(nome).to_uppercase();
    extrair(painel, &mun, pasta)?;
    montar(pasta, numero, nome, ibge, None, setores)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn pasta(nome: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("braniac-link_radar_mt-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(p.join("radar")).unwrap();
        p
    }

    fn ok(_: &Path) -> Result<(), String> {
        Ok(())
    }

    #[test]
    fn ultimo_mes_comum_espera_todos_os_orgaos() {
        let mut l = vec![json!([2026, "PREFEITURA", "08", 10.0]), json!([2026, "PREFEITURA", "07", 5.0]), json!([2026, "CAMARA", "09", 1.0]), json!([2026, "CAMARA", "07", 1.0]),
            json!([2026, "CAMARA", "08", 1.0]), json!([2025, "AUTARQUIA", "12", 1.0]), json!([2026, "CAMARA", "10", 0])];
        assert_eq!(ultimo_mes_comum(&l, "2026").as_deref(), Some("08"));
        l.push(json!([2026.0, "AUTARQUIA", 7, 2.0])); // so ate julho: o corte desce para julho
        assert_eq!(ultimo_mes_comum(&l, "2026").as_deref(), Some("07"));
        l.push(json!(["2026", "FUNDO", "03", 2.0]));
        assert_eq!(ultimo_mes_comum(&l, "2026"), None); // nenhum mes comum: nao ha corte
        assert_eq!(ultimo_mes_comum(&[], "2026"), None);
    }

    fn radar(p: &Path) -> (Value, Value) {
        let (mut d, mut r) = (json!({ "mes26": "07" }), json!({ "mes26": "07" }));
        for (k, a) in ANOS.iter().enumerate() {
            let k = (k + 1) as f64;
            d[format!("acoes-{a}")] = json!([[1, 12, 361, 10, "ENSINO FUNDAMENTAL", 100.0 * k, 100.0 * k], [1, 23, 695, 11, "DESENVOLVIMENTO E APOIO A EVENTOS TRADICIONAIS", 20.0 * k, 20.0 * k],
                [1, 8, 244, 12, "BENEFÍCIOS EVENTUAIS", 5.0 * k, 5.0 * k], [1, 4, 122, 13, "SEM MOVIMENTO", 0, 0]]);
            d[format!("total-{a}")] = json!([["OUTRA", 0, 0, 0], ["CIDADE", 125.0 * k, 130.0 * k, 120.0 * k]]);
            r[format!("origens-{a}")] = json!([["IMPOSTOS", 50.0 * k], ["TRANSFERÊNCIAS", 100.0 * k], ["ZERADA", 0]]);
            r[format!("total-{a}")] = json!([["CIDADE", 150.0 * k]]);
        }
        jgravar(&p.join("radar/despesa.json"), &d).unwrap();
        jgravar(&p.join("radar/receita.json"), &r).unwrap();
        (d, r)
    }

    #[test]
    fn montar_gera_cidade_json() {
        let p = pasta("montar");
        radar(&p);
        let c = montar(&p, "901", "Cidade Teste", 5100000, None, &ok).unwrap();
        assert_eq!(c["anos"]["2025"], json!({ "arrec": 300.0, "gastou": 250.0, "emp": 260.0 }));
        assert!(c["festas"]["2026"] == json!({ "prev": 0, "gasto": 60.0 }) && c["benef"]["2024"] == 5.0);
        assert!(c["sem_previsto"] == true && c["tribunal"]["sigla"] == "TCE-MT" && c["mes26_nome"] == "julho" && c["ausentes"] == json!([]) && c["ausencia"].is_null());
        assert!(c["codigo"] == "orcamentocidadetestemt" && c["comp"] == "OrcamentoCidadetesteMt" && c["pilha_mi"] == 1 && c["pilha_gaveta_mi"] == 0.1);
        assert_eq!(c["capa"], json!(["Festas:", "R$ 0,00 mi.", "Ajuda às famílias:", "R$ 0,0 mil."]));
        let le = |n: &str| jler(&p.join("saida").join(n)).unwrap();
        assert_eq!(le("receita-2024.json"), json!([["IMPOSTOS", 50.0], ["TRANSFERÊNCIAS", 100.0]]));
        let acs = le("acoes-2026-07.json");
        assert!(acs.as_array().unwrap().len() == 3 && acs[0]["cod"] == "1.0.12.361.0.10" && acs[0]["f"] == "12"); // subfuncao na 4a posicao; acao zerada fora
        assert_eq!(le("cidade.json"), c);
        let dito = |d: &str| c["totais"].as_array().unwrap().iter().find(|t| t["dito"] == d).cloned().unwrap();
        assert!(dito("Sobrou (2024)")["valor"] == 25.0 && dito("Sobrou (2024)")["onde"].as_str().unwrap().contains("150,00 − 125,00"));
        assert!(dito("Festas gastou (2025)")["valor"] == 40.0 && dito("Benefícios eventuais (2026)")["valor"] == 15.0 && dito("Já empenhou (2026 até 07)")["valor"] == 390.0);
        assert!(montar(&p, "901", "Cidade Teste", 5100000, Some(5.0), &|_| Err("setores falhou".into())).is_err());
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn montar_recusa_o_que_nao_fecha_e_ano_nao_entregue() {
        let p = pasta("recusa");
        let (d, r) = radar(&p);
        let mut ruim = d.clone();
        ruim["acoes-2024"].as_array_mut().unwrap().remove(2);
        jgravar(&p.join("radar/despesa.json"), &ruim).unwrap();
        assert!(montar(&p, "901", "Cidade Teste", 1, None, &ok).unwrap_err().contains("2024: soma das ações")); // soma != total tem de parar
        jgravar(&p.join("radar/despesa.json"), &d).unwrap();
        let mut rr = r.clone();
        rr["origens-2025"][0][1] = json!(1.0);
        jgravar(&p.join("radar/receita.json"), &rr).unwrap();
        assert!(montar(&p, "901", "Cidade Teste", 1, None, &ok).unwrap_err().contains("2025: origens da receita"));
        jgravar(&p.join("radar/receita.json"), &r).unwrap();
        // ano fechado sem a Prefeitura no tribunal: pauta, nao numero de video
        let mut sem = d.clone();
        sem["orgaos-2025"] = json!([["CAMARA MUNICIPAL", 1.0, 1.0]]);
        jgravar(&p.join("radar/despesa.json"), &sem).unwrap();
        assert!(montar(&p, "901", "Cidade Teste", 1, None, &ok).unwrap_err().contains("não enviou ao TCE-MT os dados de 2025"));
        // 2026 sem a Prefeitura: video de pauta, 2026 zerado e sem arquivo de 2026 de rodada antiga
        let mut sem = d.clone();
        sem["orgaos-2024"] = json!([["Prefeitura Municipal", 1.0, 1.0]]);
        sem["orgaos-2026"] = json!([["CAMARA MUNICIPAL", 1.0, 1.0]]);
        jgravar(&p.join("radar/despesa.json"), &sem).unwrap();
        std::fs::create_dir_all(p.join("saida")).unwrap();
        std::fs::write(p.join("saida/acoes-2026-07.json"), "[]").unwrap();
        let c = montar(&p, "901", "Cidade Teste", 1, None, &ok).unwrap();
        assert!(c["ausentes"] == json!(["2026"]) && c["anos"]["2026"]["gastou"] == 0 && c["capa"][3] == "ao TCE-MT." && c["ausencia"]["2026"]["dez"].is_string());
        assert!(!p.join("saida/acoes-2026-07.json").exists() && p.join("saida/acoes-2025-12.json").exists());
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn extrair_corta_no_mes_comum_retoma_e_recusa_consulta_cortada() {
        let p = pasta("extrair");
        let pedidos = RefCell::new(vec![]);
        let painel = |site: &str, dims: &[String], meas: &[String], n: usize| -> Result<Value, String> {
            pedidos.borrow_mut().push(format!("{site}|{}|{}|{n}", dims.join(","), meas.join(";")));
            if dims[0] == "Exercício" {
                return Ok(json!({ "linhas": [[2026, "PREFEITURA", "08", 1.0], [2026, "CAMARA", "08", 1.0], [2026, "CAMARA", "09", 1.0]], "total": 3 }));
            }
            Ok(json!({ "linhas": [["X", 1.0, 1.0, 1.0]], "total": 1 }))
        };
        assert_eq!(extrair(&painel, "CIDADE TESTE", &p).unwrap(), "08");
        let n = pedidos.borrow().len();
        assert_eq!(n, 1 + 9 + 9);
        let ped = pedidos.borrow().join("\n");
        assert!(ped.contains("Município={'CIDADE TESTE'}") && ped.contains("Exercício={2026}, [Mês Referência]={'01','02','03','04','05','06','07','08'}>}[Valor Liquidado])"));
        assert!(ped.contains("radarreceita.tce.mt.gov.br|Origem|") && ped.contains("OFSS]={'Sim'}"));
        assert_eq!(extrair(&painel, "CIDADE TESTE", &p).unwrap(), "08");
        assert_eq!(pedidos.borrow().len(), n); // o que ja veio nao e consultado de novo
        let d = jler(&p.join("radar/despesa.json")).unwrap();
        assert!(d["mes26"] == "08" && d["acoes-2026"].is_array() && d["meses_por_orgao"].as_array().unwrap().len() == 3);
        // consulta que veio cortada nao e gravada
        let q = pasta("cortada");
        let cortado = |_: &str, dims: &[String], _: &[String], _: usize| -> Result<Value, String> {
            Ok(if dims[0] == "Exercício" { json!({ "linhas": [[2026, "PREFEITURA", "08", 1.0]], "total": 1 }) } else { json!({ "linhas": [["X", 1.0, 1.0]], "total": 2000 }) })
        };
        assert!(extrair(&cortado, "CIDADE TESTE", &q).unwrap_err().contains("dividir a consulta"));
        assert!(!q.join("radar/despesa.json").exists());
        assert!(extrair(&|_, _, _, _| Ok(json!({ "linhas": [], "total": 0 })), "CIDADE TESTE", &q).unwrap_err().contains("nenhum mês de 2026"));
        let _ = (std::fs::remove_dir_all(&p), std::fs::remove_dir_all(&q));
    }
}
