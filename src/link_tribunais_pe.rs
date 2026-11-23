//! Molde de Pernambuco (link/tribunais/pe.py), Recife. 2024 e 2025: Prestacao de Contas de Governo no e-TCEPE (Item 4 Balanco
//! Orcamentario = receita por origem e totais; Item 18 = despesa realizada (EMPENHADA) por acao e funcao; Item 47 = LOA, dotacao
//! inicial por acao). 2026: API de Dados Abertos do Tome Conta (ReceitasMunicipais e DespesasMunicipais, um registro por empenho),
//! ate o ultimo mes que todos os orgaos regulares entregaram. O TCE-PE nao publica a dotacao por acao na API: 2026 fica com
//! orcado 0 (dito nas pendencias), nada e estimado.
//! Balanco que nao fecha, Item 18 que nao soma o empenhado do Balanco, funcao sem valor na linha ou sem codigo na Portaria 42,
//! resposta da API com status ruim ou CORTADA no limite, documento que falta = Err (pendencia). LOA de 2026 achada tambem para:
//! falta ligar as acoes da API (sem codigo) as da LOA.
//! Entram como closure: `get(metodo, parametros)` devolve o texto de uma consulta da API (uma por vez; pausa e novas tentativas
//! de quem chama) e `pdf(ano, item)` devolve as palavras de cada pagina do PDF do e-TCEPE (posicao x0/x1/top, como o pdfplumber)
//! e o texto da pagina; Ok(None) = a prestacao ou o item nao esta no e-TCEPE.
//! Ficou de fora: o navegador do e-TCEPE (JS da consulta publica), a leitura do PDF em si e o cache de 20 dias.
#![allow(dead_code)]
use crate::link_tribunais_base::{self as base, Acao, Ano, Dados};
use crate::link_tribunais_util::{self as u, ini, pystr, Acoes};
use crate::texto_pt::sem_acento;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::OnceLock;

pub const IBGE_RECIFE: i64 = 2611606;
pub const FESTAS_EXTRA: &str = r"FESTIVIDADES CULTURAIS E TRADICIONAIS";
pub const SOCIAL: (&str, &str) = (r"PROTE[ÇC][ÃA]O SOCIAL B[ÁA]SICA", "proteção social básica");
pub type Get<'a> = &'a dyn Fn(&str, &[(&str, String)]) -> Result<String, String>;
pub type Pdf<'a> = &'a dyn Fn(&str, &str) -> Result<Option<Vec<Pagina>>, String>;

/// Uma palavra do PDF com a posicao na pagina (o extract_words do pdfplumber).
#[derive(Clone, Debug, Default)]
pub struct Palavra {
    pub text: String,
    pub x0: f64,
    pub x1: f64,
    pub top: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Pagina {
    pub palavras: Vec<Palavra>,
    pub texto: String,
}

pub fn tribunal() -> Value {
    json!({ "sigla": "TCE-PE", "fala": "Tribunal de Contas do Estado", "uf": "PE", "estado": "Pernambuco", "doc_despesa": "TOME CONTA DESPESAS", "doc_receita": "TOME CONTA RECEITAS",
        "tribunal_estado": "TCE-PE (Tribunal de Contas do Estado de Pernambuco)", "site_tribunal": "tomeconta.tcepe.tc.br",
        "fonte_md": "Tudo do TCE-PE (Tribunal de Contas do Estado de Pernambuco). 2024 e 2025: Prestação de Contas de Governo no e-TCEPE (Balanço Orçamentário, despesa por projeto/atividade do Item 18 e LOA do Item 47). 2026: Tome Conta, API de Dados Abertos (sistemas.tce.pe.gov.br/DadosAbertos), ReceitasMunicipais e DespesasMunicipais (liquidado por ação). \"Previa\" = dotação inicial da LOA. Em 2024/2025 o gasto por ação é a despesa realizada (empenhada) do Item 18.",
        "setores_md": "Setores = soma da despesa por ação (Item 18 em 2024/2025, liquidado da API em 2026), pela função; festas e eventos separados da cultura; segurança pública fora.",
        "pendencias_md": ["Sem LOA 2026 no e-TCEPE (orçado 2026 = 0).", "A Câmara não tem despesa de 2026 no TCE-PE (só receita).", "2024/2025: gasto por ação = empenhado (Item 18); o liquidado total do Balanço é um pouco menor."],
        "orgaos": "Prefeitura, Câmara, autarquias e fundos (consolidado)",
        "legenda_docs": ["• 2024 e 2025: e-TCEPE, Prestação de Contas de Governo do Recife (Balanço Orçamentário, Item 18 e LOA)", "• 2026: Tome Conta do TCE-PE, Dados Abertos (receitas e despesas municipais)", "• etce.tcepe.tc.br e tomeconta.tcepe.tc.br"] })
}

/// Portaria MOG 42/1999: o TCE-PE manda a funcao so pelo nome.
pub fn funcao(nome: &str) -> Option<i64> {
    const F: [&str; 28] = ["legislativa", "judiciaria", "essencial a justica", "administracao", "defesa nacional", "seguranca publica", "relacoes exteriores", "assistencia social", "previdencia social", "saude", "trabalho", "educacao", "cultura", "direitos da cidadania", "urbanismo", "habitacao", "saneamento", "gestao ambiental", "ciencia e tecnologia", "agricultura", "organizacao agraria", "industria", "comercio e servicos", "comunicacoes", "energia", "transporte", "desporto e lazer", "encargos especiais"];
    match n(nome).as_str() {
        "encargos especias" => Some(28), // grafia errada que vem assim do proprio TCE-PE
        "reserva de contingencia" => Some(99),
        x => F.iter().position(|f| *f == x).map(|i| i as i64 + 1),
    }
}

fn sub_divida(nome: &str) -> &'static str {
    match n(nome).as_str() {
        "refinanciamento da divida interna" => "841",
        "refinanciamento da divida externa" => "842",
        "servico da divida interna" => "843",
        "servico da divida externa" => "844",
        _ => "0",
    }
}

/// Sem acento, minusculas, um espaco so entre as palavras.
fn n(s: &str) -> String {
    sem_acento(s).split_whitespace().collect::<Vec<_>>().join(" ")
}

fn inteiro(v: &Value, o_que: &str) -> Result<i64, String> {
    v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())).ok_or_else(|| format!("TCE-PE: {o_que} que não é número: {v}"))
}

/// O conteudo de uma resposta da API. Status diferente de OK ou resultado cortado no limite da API = Err.
pub fn resposta(texto: &str, o_que: &str) -> Result<Vec<Value>, String> {
    let v = u::json(texto, o_que)?;
    let r = &v["resposta"];
    if r["status"] != "OK" {
        return Err(format!("TCE-PE {o_que}: status {}", r["status"]));
    }
    let limite = if r["limiteResultado"].is_null() { i64::MAX } else { inteiro(&r["limiteResultado"], "limite")? };
    if !r["tamanhoResultado"].is_null() && inteiro(&r["tamanhoResultado"], "tamanho")? >= limite {
        return Err(format!("TCE-PE {o_que}: resultado cortado no limite da API ({limite})"));
    }
    u::lista(&r["conteudo"], o_que).cloned()
}

/// O que foi lido da API: receita por ano e despesa por (ano, mes).
#[derive(Default)]
#[derive(Debug)]
pub struct Baixado {
    pub receita: BTreeMap<String, Vec<Value>>,
    pub despesa: BTreeMap<(String, i64), Vec<Value>>,
}

pub fn baixar(get: Get, ibge: i64) -> Result<Baixado, String> {
    let pede = |met: &str, q: &[(&str, String)]| resposta(&get(met, q)?, &format!("{met} {q:?}"));
    let muns = pede("Municipios", &[])?;
    let cod = muns.iter().find(|m| m["CODIGOIBGE"] == ibge.to_string()).map(|m| u::txt(m, "CODIGO").map(String::from)).ok_or_else(|| format!("IBGE {ibge} não está na lista de municípios do TCE-PE"))??;
    let mut b = Baixado::default();
    for a in base::ANOS {
        let ugs: Vec<Value> = pede("UnidadesJurisdicionadasMunicipais", &[("EXERCICIO", a.into())])?.into_iter().filter(|x| x["CODIGOMUNICIPIOUNIDADEGESTORA"] == cod.as_str()).collect();
        if ugs.is_empty() {
            return Err(format!("TCE-PE sem unidades gestoras de {cod} em {a}"));
        }
        let rec = b.receita.entry(a.into()).or_default();
        for x in &ugs {
            if x["ID_UNIDADEGESTORA"].is_null() {
                return Err(format!("TCE-PE {a}: unidade gestora sem ID_UNIDADEGESTORA"));
            }
            rec.extend(pede("ReceitasMunicipais", &[("ID_UNIDADE_GESTORA", pystr(&x["ID_UNIDADEGESTORA"])), ("ANOREFERENCIA", a.into())])?);
        }
        for m in 1..=12 {
            b.despesa.insert((a.into(), m), pede("DespesasMunicipais", &[("CODIGO_MUNICIPIO", cod.clone()), ("ANOREFERENCIA", a.into()), ("MESREFERENCIA", m.to_string())])?);
        }
    }
    Ok(b)
}

fn mes<'a>(b: &'a Baixado, a: &str, m: i64) -> Result<&'a Vec<Value>, String> {
    b.despesa.get(&(a.to_string(), m)).ok_or_else(|| format!("TCE-PE: despesa de {m:02}/{a} não foi lida no tribunal"))
}

/// Ultimo mes de 2026 que todos os orgaos regulares entregaram (regulares = os que empenharam em TODOS os meses de 2025): o menor
/// "ultimo mes com empenho" deles em 2026. Orgao regular sem nenhum empenho de 2026 nao segura o corte: volta na 2a lista (pendencia).
pub fn corte26(b: &Baixado) -> Result<(i64, Vec<String>), String> {
    let meses = |a: &str| -> Result<Vec<BTreeSet<String>>, String> { (1..=12).map(|m| Ok(mes(b, a, m)?.iter().map(|x| pystr(&x["ID_UNIDADE_GESTORA"])).collect())).collect() };
    let (m25, m26) = (meses("2025")?, meses("2026")?);
    let regulares: BTreeSet<&String> = m25[0].iter().filter(|ug| m25.iter().all(|s| s.contains(*ug))).collect();
    if regulares.is_empty() {
        return Err("TCE-PE: nenhum órgão com remessa em todos os meses de 2025, não dá para achar o corte de 2026".into());
    }
    let ult: Vec<(&String, i64)> = regulares.iter().map(|ug| (*ug, (1..=12).rev().find(|m| m26[*m as usize - 1].contains(*ug)).unwrap_or(0))).collect();
    let sem: Vec<String> = ult.iter().filter(|x| x.1 == 0).map(|x| x.0.clone()).collect();
    let corte = ult.iter().map(|x| x.1).filter(|m| *m > 0).min().ok_or("TCE-PE: nenhum órgão tem empenho em 2026")?;
    Ok((corte, sem))
}

/// Le os 3 anos da API. Devolve os dados, o mes de corte de 2026 e os orgaos regulares sem despesa de 2026.
pub fn ler(b: &Baixado) -> Result<(Dados, String, Vec<String>), String> {
    let (mes26, sem) = corte26(b)?;
    let mut dados = Dados::new();
    for a in base::ANOS {
        let ate = if a == "2026" { mes26 } else { 12 };
        let mut orig = vec![];
        for r in b.receita.get(a).ok_or_else(|| format!("TCE-PE: receita de {a} não foi lida no tribunal"))? {
            if inteiro(&r["MESREFERENCIA"], "mês da receita")? > ate {
                continue;
            }
            let nome = if pystr(&r["DEDUCAORECEITA"]) != "0" { "Deduções da receita (Fundeb e outras)".to_string() } else { base::maiuscula(r["ORIGEMRECEITA"].as_str().filter(|s| !s.is_empty()).unwrap_or("Outras")) };
            u::soma(&mut orig, &nome, u::num0(&r["VALORLANCAMENTO"])?);
        }
        let (mut acoes, mut chaves): (Acoes, Vec<[String; 5]>) = Default::default();
        for m in 1..=ate {
            for r in mes(b, a, m)? {
                let nome_f = r["FUNCAO"].as_str().unwrap_or("");
                let f = funcao(nome_f).ok_or_else(|| format!("TCE-PE {a}: função sem código na Portaria 42: {nome_f:?}"))?;
                let k = [pystr(&r["ID_UNIDADE_GESTORA"]), f.to_string(), pystr(&r["SUBFUNCAO"]), pystr(&r["PROGRAMA"]), pystr(&r["ACAO"])];
                let (i, nova) = acoes.idx(&k.join("\u{1}"));
                if nova {
                    // orcado/atualizado: a API nao tem; em 2026 nao ha LOA 2026 no e-TCEPE (vale 0, dito nas pendencias)
                    acoes.lista[i] = Acao { f: f.to_string(), sub: r["SUBFUNCAO"].as_str().unwrap_or("").into(), acao: r["ACAO"].as_str().unwrap_or("").to_uppercase(), ..Default::default() };
                    chaves.push(k);
                }
                acoes.lista[i].empenhado += u::num0(&r["VALOREMPENHADO"])?;
                acoes.lista[i].liquidado += u::num0(&r["VALORLIQUIDADO"])?;
            }
        }
        // numero da acao = posicao na ordem das chaves (a API nao da codigo); o script de setores le a subfuncao na 4a parte
        let mut ordem: Vec<usize> = (0..chaves.len()).collect();
        ordem.sort_by(|x, y| chaves[*x].cmp(&chaves[*y]));
        for (i, pos) in ordem.into_iter().enumerate() {
            let k = &chaves[pos];
            acoes.lista[pos].cod = format!("{}.{:02}.{i}.{}", k[0], k[1].parse::<i64>().unwrap_or(0), sub_divida(if k[2] == "None" { "" } else { &k[2] }));
        }
        dados.insert(a.into(), u::ano(acoes.lista, orig, format!("Tome Conta TCE-PE · DespesasMunicipais {a}"), format!("Tome Conta TCE-PE · ReceitasMunicipais {a}")));
    }
    Ok((dados, format!("{mes26:02}"), sem))
}

fn eh_valor(s: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^-?[\d.]+,\d\d$").expect("regex fixa")).is_match(s)
}

fn digitos(s: &str, n: usize) -> bool {
    s.len() == n && s.bytes().all(|b| b.is_ascii_digit())
}

fn v(s: &str) -> Result<f64, String> {
    if s.bytes().any(|b| b.is_ascii_digit()) { u::n_br(s) } else { Err(format!("valor que não é número: {s:?}")) }
}

/// Palavras da pagina agrupadas por linha (topo), sem a coluna da assinatura digital (x > 800).
pub fn linhas(pg: &Pagina) -> Vec<Vec<&Palavra>> {
    let mut ws: Vec<&Palavra> = pg.palavras.iter().filter(|w| w.x0 <= 800.0).collect();
    ws.sort_by(|a, b| a.top.total_cmp(&b.top).then(a.x0.total_cmp(&b.x0)));
    let mut rows: Vec<Vec<&Palavra>> = vec![];
    for w in ws {
        match rows.last_mut() {
            Some(r) if (r[0].top - w.top).abs() < 3.0 => r.push(w),
            _ => rows.push(vec![w]),
        }
    }
    rows.iter_mut().for_each(|r| r.sort_by(|a, b| a.x0.total_cmp(&b.x0)));
    rows
}

/// Balanco Orcamentario (Anexo XII): (receita realizada por origem, total da receita, empenhado, liquidado). Origens tem de fechar o total.
pub fn balanco(pags: &[Pagina], nome: &str) -> Result<(Vec<(String, f64)>, f64, f64, f64), String> {
    let (mut orig, mut rec, mut emp, mut liq, mut na_receita) = (vec![], None, None, None, false);
    for pg in pags {
        for r in linhas(pg) {
            let txt = r.iter().filter(|w| !eh_valor(&w.text)).map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
            let n: Vec<f64> = r.iter().filter(|w| eh_valor(&w.text)).map(|w| v(&w.text)).collect::<Result<_, _>>()?;
            if txt.contains("RECEITAS ORÇAMENTÁRIAS") {
                na_receita = true;
            } else if txt.contains("DESPESAS ORÇAMENTÁRIAS") || txt.contains("Operações de Crédito / Refinanciamento") {
                na_receita = false;
            }
            let maiusculo = txt.chars().any(char::is_uppercase) && !txt.chars().any(char::is_lowercase);
            if txt.starts_with("SUBTOTAL DAS RECEITAS") && n.len() >= 3 && rec.is_none() {
                rec = Some(n[2]);
            } else if txt.starts_with("SUBTOTAL DAS DESPESAS") && n.len() >= 4 && emp.is_none() {
                (emp, liq) = (Some(n[2]), Some(n[3]));
            } else if na_receita && n.len() == 4 && maiusculo && !txt.starts_with("SUBTOTAL") && !txt.starts_with("RECEITAS") {
                u::soma(&mut orig, &base::maiuscula(&txt), n[2]);
            }
        }
    }
    let soma: f64 = orig.iter().map(|o| o.1).sum();
    match (rec, emp, liq) {
        (Some(rec), Some(emp), Some(liq)) if (soma - rec).abs() <= 1.0 => {
            orig.retain(|o| o.1 != 0.0);
            Ok((orig, rec, emp, liq))
        }
        _ => Err(format!("{nome}: balanço não fechou (receita {rec:?}, origens {soma}, empenhado {emp:?}, liquidado {liq:?})")),
    }
}

/// Uma linha de funcao do Item 18, com a acao a que pertence e a pagina (para o documento que acompanha o video).
#[derive(Clone, Debug, PartialEq)]
pub struct Linha18 {
    pub pagina: usize,
    pub acao: String,
    pub funcao: String,
    pub valor: String,
}

/// Despesa realizada por projeto/atividade e funcao (Item 18), linha a linha, e o nome de cada acao. O leiaute muda de um ano
/// para outro: acao = codigo de 4 digitos na margem mais a esquerda; funcao = 2 digitos; cada linha de funcao e da acao mais
/// proxima acima dela (acao que continua na pagina seguinte tambem).
pub fn item18_linhas(pags: &[Pagina], nome: &str) -> Result<(Vec<Linha18>, BTreeMap<String, String>), String> {
    let cod4 = |ws: &[Palavra], i: usize| digitos(&ws[i].text, 4) && ws.get(i + 1).is_some_and(|x| x.text == "-") && ws[i].x0 < 300.0;
    let margem = pags.iter().flat_map(|pg| (0..pg.palavras.len()).filter(|i| cod4(&pg.palavras, *i)).map(|i| pg.palavras[i].x0)).fold(f64::INFINITY, f64::min) + 5.0;
    if margem.is_infinite() {
        return Err(format!("{nome}: nenhuma ação no Item 18"));
    }
    let (mut acoes, mut funs, mut nomes): (Vec<(usize, f64, &str)>, Vec<(usize, f64, &str, &str)>, BTreeMap<String, String>) = Default::default();
    for (pn, pg) in pags.iter().enumerate() {
        let ws = &pg.palavras;
        for (i, w) in ws.iter().enumerate() {
            let na_linha = |z: &&Palavra| (z.top - w.top).abs() < 3.0;
            if cod4(ws, i) && w.x0 < margem {
                acoes.push((pn, w.top, w.text.as_str()));
                let linha: Vec<&Palavra> = ws.iter().filter(|z| na_linha(z) && z.x0 > w.x0 + 15.0).collect();
                let fim = linha.iter().filter(|z| eh_valor(&z.text) || digitos(&z.text, 2)).map(|z| z.x0).fold(440.0, f64::min);
                let nm = linha.iter().filter(|z| z.x0 < fim).map(|z| z.text.as_str()).collect::<Vec<_>>().join(" ");
                nomes.entry(w.text.clone()).or_insert_with(|| nm.trim_start_matches(['-', ' ']).to_string());
            }
            if digitos(&w.text, 2) && ws.get(i + 1).is_some_and(|x| x.text == "-") && w.x0 < 300.0 {
                let maior = ws.iter().filter(|z| eh_valor(&z.text) && z.x0 > 400.0 && na_linha(z)).fold(None, |m: Option<&Palavra>, z| if m.is_none_or(|m| z.x0 > m.x0) { Some(z) } else { m });
                funs.push((pn, w.top, w.text.as_str(), maior.ok_or_else(|| format!("{nome} p.{}: função {} sem valor na linha", pn + 1, w.text))?.text.as_str()));
            }
        }
    }
    acoes.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let mut out = vec![];
    for (pn, top, f, valor) in funs {
        let dona = acoes.iter().rev().find(|a| a.0 < pn || (a.0 == pn && a.1 <= top + 6.0)).ok_or_else(|| format!("{nome} p.{}: função {f} sem ação acima dela", pn + 1))?;
        out.push(Linha18 { pagina: pn + 1, acao: dona.2.to_string(), funcao: f.to_string(), valor: valor.to_string() });
    }
    Ok((out, nomes))
}

/// Item 18 somado por (acao, funcao). O total do demonstrativo e o EMPENHADO do Balanco.
pub fn item18(pags: &[Pagina], nome: &str) -> Result<(BTreeMap<(String, String), f64>, BTreeMap<String, String>), String> {
    let (linhas, nomes) = item18_linhas(pags, nome)?;
    let mut soma = BTreeMap::new();
    for l in linhas {
        *soma.entry((l.acao, l.funcao)).or_insert(0.0) += v(&l.valor)?;
    }
    Ok((soma, nomes))
}

/// Dotacao inicial (LOA) por acao, com o texto do valor e a pagina: demonstrativos da despesa por projetos, atividades e
/// operacoes especiais (todas as fontes). Devolve tambem o nome de cada acao.
pub fn loa_paginas(pags: &[Pagina], nome: &str) -> Result<(BTreeMap<String, (String, usize)>, BTreeMap<String, String>), String> {
    static RE: OnceLock<(Regex, Regex)> = OnceLock::new();
    let (cod, num) = RE.get_or_init(|| (Regex::new(r"^\d\.\d{3}$").expect("regex fixa"), Regex::new(r"^[\d.]+(,\d\d)?$").expect("regex fixa")));
    let (mut dot, mut nomes, mut dentro) = (BTreeMap::new(), BTreeMap::new(), false);
    for (pn, pg) in pags.iter().enumerate() {
        let topo = ini(&pg.texto, 600);
        dentro |= topo.contains("DEMONSTRATIVO DA DESPESA POR PROJETOS");
        if !dentro {
            continue;
        }
        let rows = linhas(pg);
        let cods: Vec<(f64, &str)> = rows.iter().filter(|r| r[0].x0 < 80.0 && cod.is_match(&r[0].text)).map(|r| (r[0].top, r[0].text.as_str())).collect();
        let todas = || rows.iter().flatten();
        let tot_x = todas().filter(|w| w.text == "TOTAL" && w.x0 > 400.0).map(|w| w.x0).fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.max(x)))).unwrap_or(500.0);
        for (i, (top, c)) in cods.iter().enumerate() {
            let fim = cods.get(i + 1).map_or(top + 40.0, |p| p.0 - 2.0);
            let na_faixa = |w: &&&Palavra| top - 2.0 <= w.top && w.top < fim;
            let k = c.replace('.', "");
            if let Some(w) = todas().find(|w| num.is_match(&w.text) && w.x1 > tot_x && na_faixa(w)) {
                if !dot.contains_key(&k) {
                    dot.insert(k.clone(), (w.text.clone(), pn + 1));
                    nomes.insert(k, todas().filter(|w| 85.0 < w.x0 && w.x0 < 370.0 && na_faixa(w)).map(|w| w.text.as_str()).collect::<Vec<_>>().join(" "));
                }
            }
        }
        if topo.contains("MODALIDADE DE APLICAÇÃO") || topo.contains("POR ELEMENTO DE DESPESA") {
            break;
        }
    }
    if dot.is_empty() { Err(format!("{nome}: não achei a dotação por ação na LOA")) } else { Ok((dot, nomes)) }
}

pub fn loa(pags: &[Pagina], nome: &str) -> Result<(BTreeMap<String, f64>, BTreeMap<String, String>), String> {
    let (dot, nomes) = loa_paginas(pags, nome)?;
    Ok((dot.into_iter().map(|(k, (t, _))| Ok((k, v(&t)?))).collect::<Result<_, String>>()?, nomes))
}

/// 2024/2025 pelo metodo do guia: documentos da prestacao de contas de governo no e-TCEPE. Devolve o ano e as pendencias dele.
pub fn anual(pdf: Pdf, ano: &str) -> Result<(Ano, Value), String> {
    let doc = |item: &str| pdf(ano, item)?.ok_or_else(|| format!("e-TCEPE {ano}: prestação de contas de governo sem o item {item}"));
    let (orig, rec, emp, liq) = balanco(&doc("04")?, &format!("e-TCEPE {ano} item 04"))?;
    let (soma, nomes) = item18(&doc("18")?, &format!("e-TCEPE {ano} item 18"))?;
    let (dot, nloa) = loa(&doc("47")?, &format!("e-TCEPE {ano} item 47"))?;
    let total: f64 = soma.values().sum();
    if (total - emp).abs() > 1.0 {
        return Err(format!("{ano}: soma do Item 18 {total:.2} != empenhado do Balanço {emp:.2}"));
    }
    let (mut acoes, mut usou) = (vec![], BTreeSet::new());
    for ((c, f), valor) in &soma {
        // a dotacao da LOA e por acao; se a acao tiver varias funcoes, vai inteira na primeira (a soma por acao continua certa)
        let o = if usou.insert(c) { dot.get(c).copied().unwrap_or(0.0) } else { 0.0 };
        let nome = nloa.get(c).filter(|x| !x.is_empty()).or(nomes.get(c).filter(|x| !x.is_empty())).unwrap_or(c).to_uppercase();
        acoes.push(Acao { f: u::int(f, "função")?.to_string(), sub: String::new(), cod: format!("{c}.{f}.0.0"), acao: nome, orcado: o, atualizado: 0.0, empenhado: *valor, liquidado: *valor });
    }
    let sem_execucao: Vec<&String> = dot.keys().filter(|c| !soma.keys().any(|k| &k.0 == *c)).collect();
    let pend = json!({ "despesa_por_acao": "empenhado (Item 18)", "liquidado_balanco": liq, "empenhado_balanco": emp, "dotacao_sem_execucao": sem_execucao });
    let mut receita = orig;
    receita.sort_by(|x, y| y.1.total_cmp(&x.1));
    let ano_lido = Ano { acoes, receita, arrecadado: rec, liquidado: emp, empenhado: emp, doc_despesa: format!("e-TCEPE · Prestação de Contas de Governo {ano} · Item 18 e LOA (Item 47)"), doc_receita: format!("e-TCEPE · Prestação de Contas de Governo {ano} · Balanço Orçamentário (Item 4)") };
    Ok((ano_lido, pend))
}

/// O que a esteira chama. `ibge` None = Recife (so ela tem padrao).
pub fn preparar(get: Get, pdf: Pdf, setores: &dyn Fn(&Path) -> Result<(), String>, numero: &str, nome: &str, pasta: &Path, ibge: Option<i64>) -> Result<Value, String> {
    let ibge = match ibge {
        Some(i) => i,
        None if sem_acento(nome).to_uppercase() == "RECIFE" => IBGE_RECIFE,
        None => return Err("passe o código IBGE do município (só Recife tem padrão)".into()),
    };
    let (mut dados, mes26, sem_2026) = ler(&baixar(get, ibge)?)?; // API de Dados Abertos: vale para 2026 (folha ja esta na Remessa)
    let mut pend = serde_json::Map::new();
    for a in ["2024", "2025"] {
        let (ano, p) = anual(pdf, a)?;
        dados.insert(a.into(), ano);
        pend.insert(a.into(), p);
    }
    // LOA 2026 so entra na prestacao de 2026 (entregue em 2027)
    let achou26 = matches!(pdf("2026", "47"), Ok(Some(p)) if loa(&p, "e-TCEPE 2026 item 47").is_ok());
    pend.insert("2026".into(), json!({ "loa_2026": if achou26 { "achada" } else { "não está no e-TCEPE: orçado 2026 = 0" }, "orgaos_sem_despesa_2026": sem_2026 }));
    if achou26 {
        return Err("LOA 2026 achada no e-TCEPE: falta ligar as ações da API (sem código) às da LOA".into());
    }
    let d = pasta.join("tce-pe");
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    std::fs::write(d.join("pendencias.json"), serde_json::to_string_pretty(&pend).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let op = base::Opcoes { festas_extra: Some(FESTAS_EXTRA), social: Some(SOCIAL), ..Default::default() };
    base::montar(pasta, numero, nome, "PE", ibge, &tribunal(), &dados, &mes26, &op, setores)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// Uma linha do PDF: (x0, palavras separadas por espaco) a partir de cada posicao.
    pub fn linha(top: f64, pedacos: &[(f64, &str)]) -> Vec<Palavra> {
        pedacos.iter().flat_map(|(x, t)| t.split(' ').enumerate().map(move |(i, p)| (x + 30.0 * i as f64, p))).map(|(x0, p)| Palavra { text: p.into(), x0, x1: x0 + 6.0 * p.chars().count() as f64, top }).collect()
    }

    pub fn pagina(texto: &str, linhas: Vec<Vec<Palavra>>) -> Pagina {
        Pagina { palavras: linhas.into_iter().flatten().collect(), texto: texto.into() }
    }

    fn pdfs(ano: &str, item: &str) -> Result<Option<Vec<Pagina>>, String> {
        fn nums(a: &'static str, b: &'static str, c: &'static str, d: &'static str) -> Vec<(f64, &'static str)> {
            vec![(420.0, a), (520.0, b), (620.0, c), (720.0, d)]
        }
        fn com(top: f64, x: f64, t: &'static str, n: Vec<(f64, &'static str)>) -> Vec<Palavra> {
            linha(top, &[vec![(x, t)], n].concat())
        }
        Ok(Some(match (ano, item) {
            ("2026", _) => return Ok(None),
            (_, "04") => vec![pagina("", vec![
                linha(10.0, &[(50.0, "RECEITAS ORÇAMENTÁRIAS")]),
                com(20.0, 50.0, "IMPOSTOS", nums("1.000.000,00", "1.000.000,00", "900.000,00", "-100.000,00")),
                com(30.0, 50.0, "TRANSFERÊNCIAS CORRENTES", nums("0,00", "0,00", "100.000,00", "100.000,00")),
                com(40.0, 50.0, "Receita Tributária", nums("1,00", "1,00", "5,00", "1,00")), // so as linhas em maiusculas sao origem
                linha(50.0, &[(50.0, "SUBTOTAL DAS RECEITAS"), (420.0, "1.000.000,00"), (520.0, "1.000.000,00"), (620.0, "1.000.000,00")]),
                linha(60.0, &[(50.0, "DESPESAS ORÇAMENTÁRIAS")]),
                com(70.0, 50.0, "SUBTOTAL DAS DESPESAS", nums("900.000,00", "900.000,00", "800.000,00", "700.000,00")),
                linha(80.0, &[(850.0, "assinado 9,99")]),
            ])],
            (_, "18") => vec![
                pagina("", vec![
                    linha(10.0, &[(40.0, "2100 - PROMOÇÃO DE EVENTOS"), (420.0, "300.000,00")]),
                    linha(20.0, &[(60.0, "13 - Cultura"), (420.0, "250.000,00"), (520.0, "300.000,00")]),
                    linha(30.0, &[(60.0, "1234 - Programa recuado")]),
                    linha(40.0, &[(40.0, "2200 - PROTEÇÃO SOCIAL")]),
                    linha(50.0, &[(60.0, "08 - Assistência"), (520.0, "400.000,00")]),
                ]),
                pagina("", vec![linha(5.0, &[(60.0, "10 - Saúde"), (520.0, "100.000,00")])]), // a acao continua na pagina seguinte
            ],
            (_, "47") => vec![
                pagina("capa da lei", vec![linha(10.0, &[(50.0, "2.100"), (520.0, "999,00")])]),
                pagina("DEMONSTRATIVO DA DESPESA POR PROJETOS, ATIVIDADES", vec![
                    linha(5.0, &[(90.0, "ESPECIFICAÇÃO"), (450.0, "TESOURO"), (520.0, "TOTAL")]),
                    linha(10.0, &[(50.0, "2.100"), (90.0, "PROMOÇÃO DE EVENTOS E FESTIVIDADES CULTURAIS E TRADICIONAIS"), (455.0, "90.000,00"), (530.0, "100.000,00")]),
                    linha(30.0, &[(50.0, "2.200"), (90.0, "PROTEÇÃO SOCIAL BÁSICA"), (530.0, "50.000")]),
                    linha(50.0, &[(50.0, "2.900"), (90.0, "SEM EXECUÇÃO"), (530.0, "7.000,00")]),
                ]),
                pagina("DEMONSTRATIVO POR ELEMENTO DE DESPESA", vec![linha(10.0, &[(50.0, "3.000"), (90.0, "OUTRO QUADRO"), (530.0, "1,00")])]),
                pagina("depois do quadro", vec![linha(10.0, &[(50.0, "4.000"), (530.0, "1,00")])]),
            ],
            x => return Err(format!("item inesperado {x:?}")),
        }))
    }

    fn api(met: &str, q: &[(&str, String)]) -> Result<String, String> {
        let p = |k: &str| q.iter().find(|x| x.0 == k).map(|x| x.1.clone()).unwrap_or_default();
        let ok = |c: Value| Ok(json!({ "resposta": { "status": "OK", "tamanhoResultado": c.as_array().unwrap().len(), "limiteResultado": 100000, "conteudo": c } }).to_string());
        match met {
            "Municipios" => ok(json!([{ "CODIGOIBGE": "2611606", "CODIGO": "P120" }, { "CODIGOIBGE": "1", "CODIGO": "X" }])),
            "UnidadesJurisdicionadasMunicipais" => ok(json!([{ "CODIGOMUNICIPIOUNIDADEGESTORA": "P120", "ID_UNIDADEGESTORA": 1 }, { "CODIGOMUNICIPIOUNIDADEGESTORA": "X", "ID_UNIDADEGESTORA": 9 }])),
            "ReceitasMunicipais" => {
                assert_eq!(p("ID_UNIDADE_GESTORA"), "1");
                ok(Value::Array((1..=12).flat_map(|m| [json!({ "MESREFERENCIA": m, "DEDUCAORECEITA": 0, "ORIGEMRECEITA": "IMPOSTOS, TAXAS", "VALORLANCAMENTO": "100000.5" }), json!({ "MESREFERENCIA": m.to_string(), "DEDUCAORECEITA": 9, "ORIGEMRECEITA": "TRANSFERÊNCIAS", "VALORLANCAMENTO": -10000 })]).collect()))
            }
            "DespesasMunicipais" => {
                let (a, m) = (p("ANOREFERENCIA"), p("MESREFERENCIA").parse::<i64>().unwrap());
                let e = |ug: i64, f: &str, sub: Value, acao: &str, liq: Value| json!({ "ID_UNIDADE_GESTORA": ug, "FUNCAO": f, "SUBFUNCAO": sub, "PROGRAMA": "P1", "ACAO": acao, "VALOREMPENHADO": 50000, "VALORLIQUIDADO": liq });
                let mut l = vec![];
                if a != "2026" || m <= 8 {
                    l.push(e(1, "Cultura", json!("Difusão Cultural"), "Promoção de eventos e festividades culturais e tradicionais", json!("40000")));
                    l.push(e(1, "Assistência  Social", json!("Assistência Comunitária"), "Proteção social básica", json!(10000)));
                    l.push(e(1, "Encargos Especias", json!("Serviço da Dívida Interna"), "Amortização", json!(null)));
                }
                if a != "2026" {
                    l.push(e(494, "Legislativa", json!(null), "Processo legislativo", json!(1000)));
                }
                if a == "2026" && m == 3 {
                    l.push(e(7, "Saúde", json!("Atenção Básica"), "Fundo eventual", json!(5)));
                }
                ok(json!(l))
            }
            x => Err(format!("método inesperado {x}")),
        }
    }

    /// Faz o papel do script de setores: festas = soma do liquidado das acoes de festa de cada arquivo acoes-*.json.
    pub fn setores_falso(festas_extra: &'static str) -> impl Fn(&Path) -> Result<(), String> {
        move |saida| setores(saida, festas_extra)
    }

    fn setores(saida: &Path, festas_extra: &str) -> Result<(), String> {
        let (extra, mut st) = (Regex::new(&format!("(?i){festas_extra}")).unwrap(), serde_json::Map::new());
        for e in std::fs::read_dir(saida).unwrap().flatten() {
            let nome = e.file_name().to_string_lossy().into_owned();
            if let Some(k) = nome.strip_prefix("acoes-").and_then(|k| k.strip_suffix(".json")) {
                let acs: Value = serde_json::from_str(&std::fs::read_to_string(e.path()).unwrap()).unwrap();
                let s: f64 = acs.as_array().unwrap().iter().filter(|x| base::eh_festa(x["acao"].as_str().unwrap(), Some(&extra))).map(|x| x["liquidado"].as_f64().unwrap()).sum();
                st.insert(k.into(), json!({ "Festas e eventos": { "liquidado": s } }));
            }
        }
        std::fs::write(saida.join("setores.json"), Value::Object(st).to_string()).map_err(|e| e.to_string())
    }

    #[test]
    fn api_de_2026_com_corte_e_orgao_sem_remessa() {
        let (dados, m, sem) = ler(&baixar(&api, IBGE_RECIFE).unwrap()).unwrap();
        assert_eq!((m.as_str(), sem), ("08", vec!["494".to_string()])); // a Camara nao segura o corte; o fundo eventual nao e regular
        let a = &dados["2026"];
        assert_eq!(a.acoes.iter().map(|x| (x.cod.as_str(), x.f.as_str(), x.liquidado)).collect::<Vec<_>>(), [("1.13.0.0", "13", 320000.0), ("1.08.2.0", "8", 80000.0), ("1.28.1.843", "28", 0.0), ("7.10.3.0", "10", 5.0)]);
        assert_eq!((a.acoes[0].acao.as_str(), a.acoes[0].orcado, a.acoes[0].empenhado), ("PROMOÇÃO DE EVENTOS E FESTIVIDADES CULTURAIS E TRADICIONAIS", 0.0, 400000.0));
        assert_eq!(a.receita, [("Impostos, taxas".to_string(), 800004.0), ("Deduções da receita (Fundeb e outras)".to_string(), -80000.0)]);
        assert_eq!(dados["2025"].acoes[3].sub, ""); // subfuncao que o tribunal nao mandou
        assert_eq!((funcao("Ciência e  Tecnologia"), funcao("Encargos Especias"), funcao("Lazer")), (Some(19), Some(28), None));
    }

    #[test]
    fn api_recusa_status_corte_e_funcao_desconhecida() {
        let troca = |de: &'static str, para: &'static str| move |met: &str, q: &[(&str, String)]| api(met, q).map(|t| t.replace(de, para));
        let le = |g: Get| baixar(g, IBGE_RECIFE).and_then(|b| ler(&b)).map(|_| ());
        assert!(le(&troca("\"status\":\"OK\"", "\"status\":\"ERRO\"")).unwrap_err().contains("status \"ERRO\""));
        assert!(le(&troca("\"limiteResultado\":100000", "\"limiteResultado\":2")).unwrap_err().contains("cortado no limite da API (2)"));
        assert!(le(&troca("\"Cultura\"", "\"Lazer\"")).unwrap_err().contains("função sem código na Portaria 42: \"Lazer\""));
        assert!(le(&troca("\"40000\"", "\"quarenta\"")).unwrap_err().contains("não é número"));
        assert!(baixar(&api, 123).unwrap_err().contains("IBGE 123 não está na lista"));
        assert!(le(&|_, _| Err("servidor caiu".into())).is_err());
    }

    #[test]
    fn documentos_do_etcepe() {
        let bal = pdfs("2024", "04").unwrap().unwrap();
        let (orig, rec, emp, liq) = balanco(&bal, "b").unwrap();
        assert_eq!((orig, rec, emp, liq), (vec![("Impostos".to_string(), 900000.0), ("Transferências correntes".to_string(), 100000.0)], 1e6, 8e5, 7e5));
        let (linhas18, nomes) = item18_linhas(&pdfs("2024", "18").unwrap().unwrap(), "i").unwrap();
        assert_eq!(linhas18.iter().map(|l| (l.pagina, l.acao.as_str(), l.funcao.as_str(), l.valor.as_str())).collect::<Vec<_>>(), [(1, "2100", "13", "300.000,00"), (1, "2200", "08", "400.000,00"), (2, "2200", "10", "100.000,00")]);
        assert_eq!(nomes["2100"], "PROMOÇÃO DE EVENTOS");
        let (dot, nloa) = loa(&pdfs("2024", "47").unwrap().unwrap(), "l").unwrap();
        assert_eq!(dot, [("2100".to_string(), 100000.0), ("2200".to_string(), 50000.0), ("2900".to_string(), 7000.0), ("3000".to_string(), 1.0)].into()); // para no quadro por elemento
        assert_eq!(nloa["2200"], "PROTEÇÃO SOCIAL BÁSICA");
        let (ano, pend) = anual(&pdfs, "2025").unwrap();
        assert_eq!(ano.acoes.iter().map(|x| (x.cod.as_str(), x.f.as_str(), x.orcado, x.liquidado)).collect::<Vec<_>>(), [("2100.13.0.0", "13", 100000.0, 300000.0), ("2200.08.0.0", "8", 50000.0, 400000.0), ("2200.10.0.0", "10", 0.0, 100000.0)]);
        assert_eq!((ano.liquidado, ano.arrecadado, &pend["dotacao_sem_execucao"], &pend["liquidado_balanco"]), (8e5, 1e6, &json!(["2900", "3000"]), &json!(7e5)));

        // balanco que nao fecha, Item 18 que nao soma o empenhado, funcao sem valor, LOA sem o quadro, documento que falta
        fn erro(item: &str, f: &dyn Fn(&mut Vec<Pagina>)) -> String {
            let mexido = |a: &str, i: &str| {
                pdfs(a, i).map(|p| {
                    p.map(|mut p| {
                        if i == item {
                            f(&mut p);
                        }
                        p
                    })
                })
            };
            anual(&mexido, "2024").unwrap_err()
        }
        assert!(erro("04", &|p: &mut Vec<Pagina>| p[0].palavras.iter_mut().filter(|w| w.text == "900.000,00" && w.top == 20.0).for_each(|w| w.text = "800.000,00".into())).contains("balanço não fechou"));
        assert!(erro("04", &|p: &mut Vec<Pagina>| p[0].palavras.retain(|w| w.top != 70.0)).contains("balanço não fechou"));
        assert!(erro("18", &|p: &mut Vec<Pagina>| { p.pop(); }).contains("soma do Item 18 700000.00 != empenhado do Balanço 800000.00"));
        assert!(erro("18", &|p: &mut Vec<Pagina>| p[0].palavras.retain(|w| !(w.top == 50.0 && w.x0 > 400.0))).contains("função 08 sem valor na linha"));
        assert!(erro("47", &|p: &mut Vec<Pagina>| p.truncate(1)).contains("não achei a dotação por ação na LOA"));
        assert!(anual(&|_, i| if i == "18" { Ok(None) } else { pdfs("2024", i) }, "2024").unwrap_err().contains("sem o item 18"));
    }

    #[test]
    fn prepara_recife_e_guarda_as_pendencias() {
        let p = std::env::temp_dir().join(format!("braniac-link_tribunais_pe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        let c = preparar(&api, &pdfs, &setores_falso(FESTAS_EXTRA), "993", "Recife", &p, None).unwrap();
        assert_eq!((c["meses"]["2026"].as_str(), &c["festas"]["2025"], &c["festas"]["2026"]), (Some("08"), &json!({ "prev": 100000.0, "gasto": 300000.0 }), &json!({ "prev": 0.0, "gasto": 320000.0 })));
        assert_eq!((c["par2"]["curta"].as_str(), &c["benef"]["2026"], &c["anos"]["2024"]["gastou"]), (Some("PROTEÇÃO SOCIAL BÁSICA"), &json!(80000.0), &json!(800000.0)));
        let pend: Value = serde_json::from_str(&std::fs::read_to_string(p.join("tce-pe/pendencias.json")).unwrap()).unwrap();
        assert_eq!((&pend["2026"]["orgaos_sem_despesa_2026"], pend["2026"]["loa_2026"].as_str().unwrap().starts_with("não está"), &pend["2024"]["empenhado_balanco"]), (&json!(["494"]), true, &json!(8e5)));
        // LOA de 2026 achada: para (as acoes da API nao tem codigo para ligar)
        let com26 = |a: &str, i: &str| pdfs(if a == "2026" { "2025" } else { a }, i);
        assert!(preparar(&api, &com26, &setores_falso(FESTAS_EXTRA), "993", "Recife", &p, None).unwrap_err().contains("LOA 2026 achada"));
        assert!(preparar(&api, &pdfs, &setores_falso(FESTAS_EXTRA), "1", "Olinda", &p, None).unwrap_err().contains("só Recife tem padrão"));
        let _ = std::fs::remove_dir_all(&p);
    }
}
