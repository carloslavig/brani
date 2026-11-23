//! Molde do Piaui (link/tribunais/pi.py), Teresina: Portal da Cidadania do TCE-PI (dados do SAGRES). Despesa EMPENHADA e
//! LIQUIDADA do municipio inteiro por funcao (API do Portal); receita liquida por origem do consolidado "P. M. DE <CIDADE>"
//! (painel de Receitas, tem de bater com o arrecadado da API); da Documentacao Web: LOA Anexo VI (dotacao inicial por acao),
//! Anexo 6 do Balanco Geral (empenhado por acao, 2024 e 2025) e Relacao das notas de empenhos, Modelo XXII (2026, mes a mes).
//! O TCE-PI nao publica liquidado nem credito atualizado por acao: gastou e setores = liquidado por funcao; as acoes de festas e
//! de beneficios eventuais entram pelo EMPENHADO e saem da linha da funcao delas, para a soma fechar com o liquidado oficial.
//! Corte de 2026 = ultimo mes da sequencia jan, fev... com a relacao de empenhos entregue.
//! Acao repetida na LOA, LOA sem o Anexo VI, linha de empenho fora do padrao, funcao sem codigo, painel cortado ou recusado,
//! receita que nao bate com a API, documento que falta = Err (pendencia). Nada do RREO/SICONFI.
//! Entram como closure (`Fora`): `api(caminho)` = JSON da API do Portal; `painel(ano)` = resposta crua da consulta ao painel de
//! Receitas (comando em `consulta_receita`); `texto(id)` = texto do PDF (pdftotext -raw); `palavras(id)` = palavras com posicao
//! de cada pagina do PDF (Modelo XXII). Uma consulta por vez e a pausa sao de quem chama.
//! Ficou de fora: o cache em arquivo e o download dos PDFs.
//! Diferenca do Python: origem de receita sem nome no painel e Err (la virava uma origem "None").
#![allow(dead_code)]
use crate::link_tribunais_base::{self as base, Acao, Ano, Dados};
use crate::link_tribunais_go as pbi;
use crate::link_tribunais_pe::{Pagina, Palavra};
use crate::link_tribunais_util::{self as u, pystr};
use crate::texto_pt::sem_acento;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

pub const IBGE: i64 = 2211001;
pub const FESTAS_EXTRA: &str = r"FESTIVIDADES"; // Teresina: "Realização das Festividades Carnavalescas/Juninas/Natalinas"
const COLUNAS: [&str; 5] = ["Unidade Gestora", "Origem", "Receita Bruta", "Deduções", "Receita Líquida"];
type Chave = (i64, String); // (funcao, acao)

pub struct Fora<'a> {
    pub api: &'a dyn Fn(&str) -> Result<String, String>,
    pub painel: &'a dyn Fn(&str) -> Result<String, String>,
    pub texto: &'a dyn Fn(&str) -> Result<String, String>,
    pub palavras: &'a dyn Fn(&str) -> Result<Vec<Pagina>, String>,
}

pub fn tribunal() -> Value {
    json!({ "sigla": "TCE-PI", "fala": "Tribunal de Contas do Estado", "uf": "PI", "estado": "Piauí", "doc_despesa": "PORTAL DA CIDADANIA DESPESAS", "doc_receita": "PORTAL DA CIDADANIA RECEITAS",
        "tribunal_estado": "TCE-PI (Tribunal de Contas do Estado do Piauí)", "site_tribunal": "tcepi.tc.br",
        "fonte_md": "Tudo do TCE-PI (Tribunal de Contas do Estado do Piauí), Portal da Cidadania (dados do SAGRES): despesa empenhada e liquidada do município inteiro por função (API do Portal), receita líquida das deduções do Fundeb por origem (painel de Receitas do Portal, consolidado da Prefeitura) e, da Documentação Web (prestação de contas enviada ao tribunal), a LOA (dotação inicial por ação), o Anexo 6 do Balanço Geral (despesa empenhada por ação, 2024 e 2025) e a Relação das notas de empenhos emitidas (2026). \"Gastou\" = LIQUIDADO do município (Prefeitura, Câmara, IPMT, fundações e fundos). \"Previa\" = dotação inicial da LOA.",
        "setores_md": "Setores = liquidado do município por função (Portal da Cidadania do TCE-PI); festas e benefícios eventuais pelo empenhado da ação, descontados da função delas; segurança pública fora.",
        "pendencias_md": ["Festas e benefícios eventuais = valor EMPENHADO da ação (o TCE-PI não publica liquidado por ação): Anexo 6 do Balanço Geral em 2024/2025 e Relação das notas de empenhos (Modelo XXII) em 2026.", "O TCE-PI não publica o crédito atualizado por ação; setores saem da função (sem subfunção)."],
        "orgaos": "Prefeitura (consolidado: administração direta, Câmara, IPMT, fundações e fundos)",
        "legenda_docs": ["• Receita: TCE-PI, Portal da Cidadania, painel de Receitas (receita líquida) de 2024, 2025 e 2026", "• Despesa e setores: TCE-PI, Portal da Cidadania, despesa liquidada por função", "• Festas: TCE-PI, Documentação Web, LOA (previsto) e Anexo 6 / relação de empenhos (empenhado)", "• tcepi.tc.br > Portal da Cidadania"] })
}

/// Codigo da funcao (Portaria 42) pelo comeco do nome.
pub fn funcao(nome: &str) -> Result<i64, String> {
    const F: [(&str, i64); 29] = [("legisl", 1), ("judic", 2), ("essencial", 3), ("admin", 4), ("defesa", 5), ("seguranca", 6), ("relacoes", 7), ("assist", 8), ("previd", 9), ("saude", 10), ("trabalho", 11), ("educa", 12), ("cultura", 13), ("direitos", 14), ("urban", 15), ("habita", 16), ("sanea", 17), ("gestao amb", 18), ("ciencia", 19), ("agric", 20), ("organizacao", 21), ("industr", 22), ("comerc", 23), ("comunica", 24), ("energia", 25), ("transp", 26), ("desporto", 27), ("encargos", 28), ("reserva", 99)];
    let s = sem_acento(nome);
    F.iter().find(|(k, _)| s.starts_with(k)).map(|x| x.1).ok_or_else(|| format!("TCE-PI: função sem código na Portaria 42: {nome:?}"))
}

/// O comando da consulta ao painel de Receitas (tabela SagresContabilReceita), para quem faz a chamada.
pub fn consulta_receita(nome: &str, ano: &str) -> Value {
    pbi::comando("SagresContabilReceita", &COLUNAS[..2], &COLUNAS[2..], &[("Município", vec![format!("'{nome}'")]), ("Ano", vec![format!("{ano}L")])])
}

/// Receita liquida por origem do consolidado da Prefeitura (UG "P. M. DE <CIDADE>"). Camara e instituto de previdencia aparecem
/// tambem como UG propria: NAO somar (ja estao no consolidado).
pub fn receita(resposta: &str, nome: &str, ano: &str) -> Result<Vec<(String, f64)>, String> {
    let ug = sem_acento(&format!("P. M. DE {nome}"));
    let mut orig = vec![];
    for x in pbi::decodifica(resposta, "painel de Receitas do TCE-PI", &COLUNAS)? {
        if sem_acento(x["Unidade Gestora"].as_str().unwrap_or("")) == ug {
            u::soma(&mut orig, x["Origem"].as_str().ok_or_else(|| format!("painel de Receitas do TCE-PI {ano}: linha sem a origem"))?, u::num0(&x["Receita Líquida"])?);
        }
    }
    if orig.is_empty() { Err(format!("painel de Receitas do TCE-PI sem a UG consolidada P. M. DE {} em {ano}", nome.to_uppercase())) } else { Ok(orig) }
}

/// As linhas do texto como o splitlines() do Python (quebra de pagina tambem separa linha).
fn linhas_py(t: &str) -> Vec<String> {
    t.replace("\r\n", "\n").split(['\n', '\r', '\u{b}', '\u{c}', '\u{1c}', '\u{1d}', '\u{1e}', '\u{85}', '\u{2028}', '\u{2029}']).map(String::from).collect()
}

/// Numeros inteiros soltos de uma linha da LOA ("-", "1.500.000", "30"): nao pega pedaco de codigo, de palavra nem de decimal.
/// (No Python era uma regex com olhar para tras e para a frente; aqui a mesma regra em codigo.)
pub fn inteiros(s: &str) -> Vec<String> {
    let c: Vec<char> = s.chars().collect();
    let w = |x: char| x.is_alphanumeric() || x == '_';
    let d = |i: usize| c.get(i).is_some_and(char::is_ascii_digit);
    let livre = |e: usize| !c.get(e).is_some_and(|x| w(*x) || *x == ',') && !(c.get(e) == Some(&'.') && d(e + 1));
    let (mut out, mut i) = (vec![], 0);
    while i < c.len() {
        let fim = if i > 0 && (w(c[i - 1]) || c[i - 1] == '.' || c[i - 1] == ',') {
            None
        } else if c[i] == '-' {
            livre(i + 1).then_some(i + 1)
        } else {
            let nd = (0..3).take_while(|k| d(i + k)).count();
            let mut cands = vec![];
            for n in (1..=nd).rev() {
                let grupos = (0..).take_while(|g| c.get(i + n + 4 * g) == Some(&'.') && (1..=3).all(|k| d(i + n + 4 * g + k))).count();
                cands.extend((1..=grupos).rev().map(|g| i + n + 4 * g)); // com os grupos de milhar primeiro
            }
            cands.extend((1..=nd).rev().map(|n| i + n));
            cands.into_iter().find(|e| livre(*e))
        };
        match fim {
            Some(e) => {
                out.push(c[i..e].iter().collect());
                i = e;
            }
            None => i += 1,
        }
    }
    out
}

/// LOA, Anexo VI (Programa de Trabalho por orgao, consolidado): {(funcao, acao): (codigo de 13 digitos, nome, dotacao inicial)}.
pub fn loa(texto: &str, nome: &str) -> Result<BTreeMap<Chave, (String, String, f64)>, String> {
    let (cab, para, rabo) = (Regex::new(r"^(\d{13}) (.*)").expect("regex fixa"), Regex::new(r"^(\d{2,13} |Órgão|Total|TOTAL)").expect("regex fixa"), Regex::new(r"\s+(-|[\d.]+)(\s+(-|[\d.]+))*\s*$").expect("regex fixa"));
    let (linhas, mut out) = (linhas_py(texto), BTreeMap::new());
    for (i, l) in linhas.iter().enumerate() {
        let Some(m) = cab.captures(l) else { continue };
        let mut pedaco = vec![&m[2]];
        pedaco.extend(linhas.iter().skip(i + 1).take(5).take_while(|l2| !para.is_match(l2)).map(String::as_str));
        let (c, nums) = (&m[1], inteiros(&pedaco.join(" ")));
        let dot = match nums.last().map(String::as_str) {
            None | Some("-") => 0.0,
            Some(n) => u::int(&n.replace('.', ""), "dotação da LOA")? as f64,
        };
        let k = (u::int(&c[..2], "função")?, c[9..].to_string());
        if out.insert(k, (c.to_string(), rabo.replace(&m[2], "").trim().to_string(), dot)).is_some() {
            return Err(format!("{nome}: ação {c} repetida na LOA"));
        }
    }
    if out.len() < 100 { Err(format!("{nome}: não achei o Anexo VI (dotação por ação) na LOA")) } else { Ok(out) }
}

fn v(s: &str) -> Result<f64, String> {
    if s.trim() == "-" { Ok(0.0) } else { u::n_br(s) }
}

/// Balanco Geral, Anexo 6 consolidado: {(funcao, acao): (orgao, codigo, nome, empenhado)} (soma por funcao+acao).
pub fn anexo6(texto: &str) -> Result<BTreeMap<Chave, (String, String, String, f64)>, String> {
    let (orgao, linha) = (Regex::new(r"^Órgão: (\d+)").expect("regex fixa"), Regex::new(r"^(\d{9})\.(\d{4})((?: [\d.]+,\d\d)+)$").expect("regex fixa"));
    const CABECALHO: [&str; 9] = ["Totais", "Código", "Operações", "Especiais", "Programa de Trabalho", "Prefeitura", "Exercício", "Anexo", "ORÇAMENTO"];
    let (mut out, mut nome, mut org): (BTreeMap<Chave, (String, String, String, f64)>, Vec<String>, String) = (BTreeMap::new(), vec![], "0".into());
    for l in linhas_py(texto) {
        if let Some(m) = orgao.captures(&l) {
            (org, nome) = (m[1].to_string(), vec![]);
        } else if let Some(m) = linha.captures(&l) {
            let valor = v(m[3].split_whitespace().last().unwrap_or(""))?;
            let o = out.entry((u::int(&m[1][..2], "função")?, m[2].to_string())).or_insert_with(|| (org.clone(), format!("{}.{}", &m[1], &m[2]), nome.join(" "), 0.0));
            o.3 += valor;
            nome.clear();
        } else if l.chars().next().is_some_and(|c| c.is_ascii_digit()) || CABECALHO.iter().any(|c| l.starts_with(c)) {
            nome.clear();
        } else {
            nome.push(l.trim().to_string());
        }
    }
    Ok(out)
}

/// Um empenho da relacao: (orgao, funcao, acao, empenhado, anulado).
pub type Empenho = (i64, i64, String, f64, f64);

/// Relacao das notas de empenhos emitidas (Modelo XXII): mes ("01".."12"), os empenhos e os numeros dos empenhos sem
/// funcao/acao impressa (ficam fora das acoes). Colunas pela borda direita das palavras (o texto do PDF embaralha as colunas).
pub fn modelo22(pags: &[Pagina], nome: &str) -> Result<(String, Vec<Empenho>, Vec<String>), String> {
    let (rx_mes, rx_data, rx_fp, rx_elem) = (Regex::new(r"^[a-z]{3}/\d\d$").expect("regex fixa"), Regex::new(r"^\d\d/\d\d/\d{4}$").expect("regex fixa"), Regex::new(r"^(\d+)(\d)-(\d{3})$").expect("regex fixa"), Regex::new(r"^\d{6}").expect("regex fixa"));
    let digitos = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let (mut mes, mut linhas, mut sem_acao, mut cols): (Option<&str>, Vec<Empenho>, Vec<String>, Option<(f64, f64)>) = (None, vec![], vec![], None);
    for pg in pags {
        let ws = &pg.palavras;
        mes = mes.or_else(|| ws.iter().find(|w| rx_mes.is_match(&w.text)).map(|w| w.text.as_str()));
        let cab = |t: &str| ws.iter().rev().find(|w| w.text == t);
        if let (Some(e), Some(a), Some(p)) = (cab("Empenhado"), cab("Anulado"), cab("Pago")) {
            // valores alinhados a direita: a coluna vai ate o meio do vao entre o titulo dela e o da proxima
            cols = Some(((e.x1 + a.x0) / 2.0, (a.x1 + p.x0) / 2.0));
        }
        let Some((c1, c2)) = cols else { continue }; // pagina sem cabecalho repete as colunas da anterior
        let mut por: BTreeMap<i64, Vec<&Palavra>> = BTreeMap::new();
        ws.iter().for_each(|w| por.entry((w.top / 3.0).round_ties_even() as i64).or_default().push(w));
        for mut l in por.into_values() {
            l.sort_by(|a, b| a.x0.total_cmp(&b.x0));
            if l.len() < 3 || !(digitos(&l[0].text) && l[0].x0 < 70.0 && rx_data.is_match(&l[1].text)) {
                continue;
            }
            // depois da data: orgao, unidade e funcao+projeto/atividade (as vezes grudados, ex. "82-047" = funcao 8, acao 2047)
            let tk: Vec<&str> = l.iter().skip(2).take(4).map(|w| w.text.as_str()).collect();
            let (mut fp, mut i) = (String::new(), 0);
            for (j, s) in tk.iter().enumerate().skip(2) {
                fp.push_str(s);
                i = j;
                if s.contains('-') {
                    break;
                }
            }
            let (m, orgao_ok) = (rx_fp.captures(&fp), digitos(tk[0]));
            let Some(m) = m.filter(|_| orgao_ok) else {
                if orgao_ok && fp.contains(',') {
                    sem_acao.push(l[0].text.clone()); // empenho sem funcao/acao impressa: nao e de acao nenhuma
                    continue;
                }
                return Err(format!("{nome}: linha de empenho fora do padrão {:?}", l.iter().take(10).map(|w| w.text.as_str()).collect::<Vec<_>>()));
            };
            let fim = l.iter().take(i + 3).map(|w| w.x1).fold(f64::MIN, f64::max); // tk comeca no 3o item da linha
            let col = |a: f64, b: f64| l.iter().filter(|w| a < w.x1 && w.x1 <= b && w.x0 > fim).map(|w| w.text.as_str()).collect::<String>();
            let emp = col(0.0, c1);
            let emp = if emp.contains(',') { rx_elem.replace(&emp, "").into_owned() } else { String::new() }; // tira o elemento de despesa (6 digitos) da frente
            linhas.push((u::int(tk[0], "órgão")?, u::int(&m[1], "função")?, format!("{}{}", &m[2], &m[3]), v(&emp)?, v(&col(c1, c2))?));
        }
    }
    let (Some(mes), false) = (mes, linhas.is_empty()) else { return Err(format!("{nome}: Modelo XXII sem mês ou sem empenhos")) };
    let n = base::ABR.iter().position(|a| mes.starts_with(a)).ok_or_else(|| format!("{nome}: mês desconhecido {mes:?}"))?;
    Ok((format!("{:02}", n + 1), linhas, sem_acao))
}

/// Documentos de um ano cujo nome comeca com `inicio`, em ordem de envio: (id, data do envio).
fn docs(lista: &[Value], ano: &str, inicio: &str) -> Result<Vec<String>, String> {
    let mut ds = vec![];
    for x in lista {
        if pystr(&x["exercicio"]) == ano && u::txt(x, "documento")?.starts_with(inicio) {
            if x["idDocWebDownload"].is_null() {
                return Err(format!("TCE-PI: documento {inicio} de {ano} sem id de download"));
            }
            ds.push((u::txt(x, "dataEnvio")?, pystr(&x["idDocWebDownload"])));
        }
    }
    ds.sort_by(|a, b| a.0.cmp(b.0));
    Ok(ds.into_iter().map(|x| x.1).collect())
}

/// Le os 3 anos. Devolve os dados, o mes de corte de 2026 e os avisos (empenhos sem funcao/acao impressa).
pub fn ler(f: &Fora, nome: &str) -> Result<(Dados, String, Vec<String>), String> {
    let api = |rel: &str| u::json(&(f.api)(rel)?, rel);
    let mesma = |a: &str| sem_acento(a) == sem_acento(nome);
    let muns = api(&format!("prefeituras/{nome}"))?;
    let achados: Vec<&Value> = u::lista(&muns, "prefeituras")?.iter().filter(|x| x["nome"].as_str().is_some_and(mesma)).collect();
    let [mun] = achados[..] else { return Err(format!("TCE-PI: {nome} (IBGE {IBGE}) não achado no Portal da Cidadania")) };
    if pystr(&mun["codIBGE"]) != IBGE.to_string() || mun["id"].is_null() {
        return Err(format!("TCE-PI: {nome} (IBGE {IBGE}) não achado no Portal da Cidadania"));
    }
    let mid = pystr(&mun["id"]);
    let lista_v = api(&format!("documentos/{mid}?pagina=1&qtdePorPagina=5000"))?;
    let lista = u::lista(&lista_v, "documentos")?;
    let rec_v = api(&format!("receitas/{mid}"))?;
    let arrec: BTreeMap<String, f64> = u::lista(&rec_v, "receitas")?.iter().map(|x| Ok((pystr(&x["exercicio"]), u::num(x, "arrecadada")?))).collect::<Result<_, String>>()?;
    let (mut loas, mut exe, mut anexos, mut mes26, mut avisos): (BTreeMap<&str, BTreeMap<Chave, (String, String, f64)>>, BTreeMap<&str, BTreeMap<Chave, f64>>, BTreeMap<&str, BTreeMap<Chave, (String, String, String, f64)>>, String, Vec<String>) = Default::default();
    for a in base::ANOS {
        let ld = docs(lista, a, "Lei Orçamentária Anual")?;
        let id = ld.last().ok_or_else(|| format!("TCE-PI: LOA {a} não está na Documentação Web"))?;
        loas.insert(a, loa(&(f.texto)(id)?, &format!("LOA {a} ({id}.pdf)"))?);
        if a != "2026" {
            let an = docs(lista, a, "Programa de trabalho (Anexo 6")?;
            let id = an.last().ok_or_else(|| format!("TCE-PI: Anexo 6 do Balanço Geral {a} não está na Documentação Web"))?;
            let x = anexo6(&(f.texto)(id)?)?;
            exe.insert(a, x.iter().map(|(k, v)| (k.clone(), v.3)).collect());
            anexos.insert(a, x);
        } else {
            let mut por_mes: BTreeMap<String, Vec<Empenho>> = BTreeMap::new();
            for id in docs(lista, a, "Relação das notas de empenhos emitidas")? {
                let (m, linhas, sem_acao) = modelo22(&(f.palavras)(&id)?, &format!("{id}.pdf"))?;
                if !sem_acao.is_empty() {
                    avisos.push(format!("TCE-PI {id}.pdf ({m}/2026): empenhos sem função/ação impressa, fora das ações: {}", sem_acao.join(", ")));
                }
                por_mes.insert(m, linhas); // reenvio do mesmo mes: vale o ultimo enviado (lista em ordem de envio)
            }
            // corte de 2026: ultimo mes da sequencia jan, fev... com a relacao de empenhos entregue
            let n = (1..=12).take_while(|m| por_mes.contains_key(&format!("{m:02}"))).count();
            if n == 0 {
                return Err("TCE-PI: nenhuma Relação de empenhos de 2026 (janeiro) na Documentação Web".into());
            }
            mes26 = format!("{n:02}");
            let mut soma: BTreeMap<Chave, f64> = BTreeMap::new();
            for (_, linhas) in por_mes.iter().filter(|(m, _)| **m <= mes26) {
                for (_, fun, ac, emp, anu) in linhas {
                    *soma.entry((*fun, ac.clone())).or_default() += emp - anu;
                }
            }
            exe.insert(a, soma);
        }
    }
    // acoes de festas e de beneficios eventuais: pelo nome na LOA ou no Anexo 6 de qualquer ano (a relacao de empenhos nao tem nome)
    let sp = |c: &str| format!("{}.{}", c.get(2..5).unwrap_or(""), c.get(5..9).unwrap_or(""));
    let mut nomes: BTreeMap<Chave, (Option<String>, String, String)> = BTreeMap::new();
    for a in base::ANOS {
        for (k, (c, nm, _)) in &loas[a] {
            nomes.entry(k.clone()).or_insert_with(|| (None, sp(c), nm.clone()));
        }
        for (k, (org, c, nm, _)) in anexos.get(a).into_iter().flatten() {
            nomes.insert(k.clone(), (Some(org.clone()), sp(c), nm.clone()));
        }
    }
    let extra = Regex::new(&format!("(?i){FESTAS_EXTRA}")).expect("regex fixa");
    nomes.retain(|_, x| base::eh_festa(&x.2, Some(&extra)) || base::eh_beneficio(&x.2));
    let mut dados = Dados::new();
    for a in base::ANOS {
        let pf_v = api(&format!("despesas/{mid}/{a}/lista/porFuncao"))?;
        let pf = u::lista(&pf_v, "despesa por função")?;
        let (mut acoes, mut tira): (Vec<Acao>, BTreeMap<i64, f64>) = Default::default();
        for (k, (org, sp, nm)) in &nomes {
            let (o, e) = (loas[a].get(k).map_or(0.0, |x| x.2), exe[a].get(k).copied().unwrap_or(0.0));
            if o == 0.0 && e == 0.0 {
                continue;
            }
            let nm_ano = loas[a].get(k).map(|x| &x.1).filter(|n| !n.is_empty()).unwrap_or(nm);
            // liquidado por acao nao existe: empenhado
            acoes.push(Acao { f: k.0.to_string(), sub: u::ini(sp, 3).into(), cod: format!("{}.0.{:02}.{sp}.{}", org.as_deref().filter(|o| !o.is_empty()).unwrap_or("0"), k.0, k.1), acao: nm_ano.to_uppercase(), orcado: o, atualizado: 0.0, empenhado: e, liquidado: e });
            *tira.entry(k.0).or_default() += e;
        }
        let (mut liq_total, mut emp_total) = (0.0, 0.0);
        for x in pf {
            let (nome_f, liquidada, empenhada) = (u::txt(x, "funcao")?, u::num(x, "liquidada")?, u::num(x, "empenhada")?);
            let cf = funcao(nome_f)?;
            let t = tira.get(&cf).copied().unwrap_or(0.0);
            if liquidada - t < -1.0 {
                return Err(format!("TCE-PI {a}: empenhado das ações especiais passa do liquidado da função {nome_f}"));
            }
            acoes.push(Acao { f: cf.to_string(), sub: "000".into(), cod: format!("0.0.{cf:02}.000.0000.0000"), acao: format!("{} (liquidado da função)", nome_f.to_uppercase()), orcado: 0.0, atualizado: 0.0, empenhado: empenhada - t, liquidado: liquidada - t });
            liq_total += liquidada;
            emp_total += empenhada;
        }
        let mut rec = receita(&(f.painel)(a)?, nome, a)?;
        let (soma, oficial) = (rec.iter().map(|o| o.1).sum::<f64>(), *arrec.get(a).ok_or_else(|| format!("TCE-PI {a}: a API não tem o arrecadado do ano"))?);
        if (soma - oficial).abs() > 1.0 {
            return Err(format!("TCE-PI {a}: receita do painel {soma:.2} != arrecadado da API {oficial:.2}"));
        }
        rec.sort_by(|x, y| y.1.total_cmp(&x.1));
        let fonte = if a != "2026" { "Anexo 6 do Balanço Geral".to_string() } else { format!("Relação das notas de empenhos jan-{}", base::ABR[u::int(&mes26, "mês")? as usize - 1]) };
        dados.insert(a.into(), Ano { acoes, receita: rec, arrecadado: soma, liquidado: liq_total, empenhado: emp_total, doc_despesa: format!("TCE-PI Portal da Cidadania · despesa por função {a}; LOA e {fonte} (Documentação Web)"), doc_receita: format!("TCE-PI Portal da Cidadania · painel de Receitas {a} (receita líquida)") });
    }
    Ok((dados, mes26, avisos))
}

/// O que a esteira chama. So Teresina (o molde foi feito e conferido para ela).
pub fn preparar(f: &Fora, setores: &dyn Fn(&Path) -> Result<(), String>, numero: &str, nome: &str, pasta: &Path) -> Result<Value, String> {
    if sem_acento(nome).to_uppercase() != "TERESINA" {
        return Err("o molde do TCE-PI foi feito e conferido para Teresina (IBGE 2211001); outra cidade precisa do IBGE dela".into());
    }
    let (dados, mes26, _) = ler(f, nome)?;
    base::montar(pasta, numero, nome, "PI", IBGE, &tribunal(), &dados, &mes26, &base::Opcoes { festas_extra: Some(FESTAS_EXTRA), ..Default::default() }, setores)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link_tribunais_pe::tests::{linha, pagina, setores_falso};

    fn texto_loa(festas: &str) -> String {
        let mut t = String::from("Anexo VI\u{c}Órgão: 13 Fundação Cultural\n");
        t += &format!("1339200012100 Realização das Festividades Juninas {festas} - {festas}\r\n");
        t += "0824400052200 Concessão de Benefícios Eventuais\n800.000 - 800.000\nTotal 9.999.999\n";
        (0..100).for_each(|i| t += &format!("04{:07}{:04} Manutenção da unidade {i} 1.000 1.000\n", 1220000 + i, 3000 + i));
        t
    }

    fn texto_anexo6() -> String {
        "Anexo 6\nÓrgão: 13 Fundação Cultural\nRealização das Festividades\nJuninas\n133920001.2100 1.000,00 2.000.000,00\nTotais 5,00\nÓrgão: 08 Assistência\nConcessão de Benefícios Eventuais\n082440005.2200 300.000,00\n082440005.2200 100.000,00\n041220000.3000 7,00\n".into()
    }

    fn relacao(mes: &str, festas: &str) -> Vec<Pagina> {
        let emp = |top: f64, nr: &'static str, fp: &'static str, valor: &str, anulado: &'static str| linha(top, &[(30.0, nr), (80.0, "05/01/2026"), (150.0, "13"), (180.0, "01"), (210.0, fp), (440.0, "339039"), (480.0, valor), (600.0, anulado), (700.0, "0,00")]);
        vec![
            pagina("", vec![linha(10.0, &[(30.0, "Relação"), (300.0, mes)]), linha(20.0, &[(500.0, "Empenhado"), (600.0, "Anulado"), (700.0, "Pago")]), emp(30.0, "101", "132-100", festas, "500.000,00"), emp(60.0, "102", "82-200", "250.000,00", "-")]),
            // pagina sem cabecalho repete as colunas; funcao e acao em duas palavras; empenho sem funcao/acao impressa
            pagina("", vec![linha(30.0, &[(30.0, "103"), (80.0, "06/01/2026"), (150.0, "4"), (180.0, "01"), (210.0, "4"), (235.0, "3-000"), (440.0, "339030"), (480.0, "9,00"), (600.0, "0,00")]), linha(60.0, &[(30.0, "104"), (80.0, "06/01/2026"), (150.0, "13"), (180.0, "01"), (480.0, "1.000,00")])]),
        ]
    }

    fn dsr(linhas: &[Value]) -> String {
        let s: Vec<Value> = (0..5).map(|i| json!({ "N": format!("G{i}") })).collect();
        let dm: Vec<Value> = linhas.iter().enumerate().map(|(i, l)| if i == 0 { json!({ "S": s, "C": l }) } else { json!({ "C": l }) }).collect();
        json!({ "results": [{ "result": { "data": { "dsr": { "DS": [{ "PH": [{ "DM0": dm }] }] } } } }] }).to_string()
    }

    fn api(rel: &str) -> Result<String, String> {
        let doc = |a: i64, nome: &str, data: &str, id: i64| json!({ "exercicio": a, "documento": nome, "dataEnvio": data, "idDocWebDownload": id });
        Ok(match rel {
            "prefeituras/Teresina" => json!([{ "nome": "TERESINA", "codIBGE": 2211001, "id": 133 }, { "nome": "Teresina de Goiás", "codIBGE": 1, "id": 9 }]),
            "documentos/133?pagina=1&qtdePorPagina=5000" => json!([
                doc(2024, "Lei Orçamentária Anual - LOA", "2024-01-10", 1), doc(2025, "Lei Orçamentária Anual - LOA", "2025-01-10", 2), doc(2026, "Lei Orçamentária Anual - LOA", "2026-01-10", 3),
                doc(2026, "Lei Orçamentária Anual - LOA", "2026-01-05", 30), // o envio mais antigo nao vale
                doc(2024, "Programa de trabalho (Anexo 6), Balanço Geral", "2025-03-01", 4), doc(2025, "Programa de trabalho (Anexo 6), Balanço Geral", "2026-03-01", 5),
                doc(2026, "Relação das notas de empenhos emitidas", "2026-02-10", 6), doc(2026, "Relação das notas de empenhos emitidas", "2026-03-10", 7),
                doc(2026, "Relação das notas de empenhos emitidas", "2026-02-20", 8), doc(2026, "Relação das notas de empenhos emitidas", "2026-05-10", 9),
                doc(2026, "Balancete", "2026-02-10", 99)
            ]),
            "receitas/133" => json!([{ "exercicio": 2024, "arrecadada": 9e7 }, { "exercicio": 2025, "arrecadada": 9e7 }, { "exercicio": 2026, "arrecadada": 9e7 }]),
            r if r.ends_with("lista/porFuncao") => json!([{ "funcao": "Cultura", "liquidada": 5e6, "empenhada": 6e6 }, { "funcao": "Assistência Social", "liquidada": 2e6, "empenhada": 2e6 }, { "funcao": "Educação", "liquidada": 4e7, "empenhada": 4.1e7 }]),
            x => return Err(format!("consulta inesperada {x}")),
        }
        .to_string())
    }

    fn painel(_: &str) -> Result<String, String> {
        Ok(dsr(&[json!(["P. M. DE TERESINA", "Impostos", 8e7, 0, 8e7]), json!(["P. M. de Teresina", "Transferências Correntes", 2e7, -1e7, 1e7]), json!(["CAMARA MUNICIPAL DE TERESINA", "Outras", 5, 0, 5])]))
    }

    fn texto(id: &str) -> Result<String, String> {
        Ok(match id {
            "1" | "2" => texto_loa("1.500.000"),
            "3" => texto_loa("1.600.000"),
            "4" | "5" => texto_anexo6(),
            x => return Err(format!("texto inesperado {x}")),
        })
    }

    fn palavras(id: &str) -> Result<Vec<Pagina>, String> {
        Ok(match id {
            "6" => relacao("jan/26", "1,00"),            // reenviado depois (8)
            "8" => relacao("jan/26", "2.500.000,00"),
            "7" => relacao("fev/26", "1.000.000,00"),
            "9" => relacao("abr/26", "9.000.000,00"), // marco nao veio: abril fica fora
            x => return Err(format!("pdf inesperado {x}")),
        })
    }

    const F: Fora = Fora { api: &api, painel: &painel, texto: &texto, palavras: &palavras };

    #[test]
    fn pecas_dos_documentos() {
        assert_eq!(inteiros("Juninas 1.500.000 - 30 1339200012100 12,5 1.2 a1 7. x-y -- 1.234.5678"), ["1.500.000", "-", "30", "7", "-", "-"]);
        let l = loa(&texto_loa("1.500.000"), "loa").unwrap();
        assert_eq!(l[&(13, "2100".to_string())], ("1339200012100".to_string(), "Realização das Festividades Juninas".to_string(), 1.5e6));
        assert_eq!(l[&(8, "2200".to_string())], ("0824400052200".to_string(), "Concessão de Benefícios Eventuais".to_string(), 8e5)); // dotacao na linha de baixo
        assert_eq!((l.len(), l[&(4, "3007".to_string())].2), (102, 1000.0));
        assert!(loa(&format!("{}1339200012100 Outra 5\n", texto_loa("1")), "loa").unwrap_err().contains("ação 1339200012100 repetida na LOA"));
        assert!(loa("1339200012100 Só uma 5\n", "loa").unwrap_err().contains("não achei o Anexo VI"));
        let a = anexo6(&texto_anexo6()).unwrap();
        assert_eq!(a[&(13, "2100".to_string())], ("13".to_string(), "133920001.2100".to_string(), "Realização das Festividades Juninas".to_string(), 2e6));
        assert_eq!((a[&(8, "2200".to_string())].3, a[&(4, "3000".to_string())].2.as_str()), (4e5, ""));
        let (m, e, sem) = modelo22(&relacao("fev/26", "2.500.000,00"), "m").unwrap();
        assert_eq!((m.as_str(), sem), ("02", vec!["104".to_string()]));
        assert_eq!(e, [(13, 13, "2100".to_string(), 2.5e6, 5e5), (13, 8, "2200".to_string(), 2.5e5, 0.0), (4, 4, "3000".to_string(), 9.0, 0.0)]);
        let mut ruim = relacao("jan/26", "1,00");
        ruim[0].palavras.iter_mut().filter(|w| w.text == "82-200").for_each(|w| w.text = "8200".into());
        assert!(modelo22(&ruim, "m").unwrap_err().contains("linha de empenho fora do padrão"));
        assert!(modelo22(&relacao("xyz/26", "1,00"), "m").unwrap_err().contains("mês desconhecido"));
        assert!(modelo22(&[pagina("", vec![linha(10.0, &[(30.0, "jan/26")])])], "m").unwrap_err().contains("sem mês ou sem empenhos"));
        assert_eq!((funcao("Gestão Ambiental"), funcao("EDUCAÇÃO"), funcao("Lazer").is_err()), (Ok(18), Ok(12), true));
        assert_eq!(consulta_receita("Teresina", "2026")["SemanticQueryDataShapeCommand"]["Query"]["Where"][1]["Condition"]["In"]["Values"][0][0]["Literal"]["Value"], "2026L");
    }

    #[test]
    fn le_teresina_e_monta() {
        let (dados, m, avisos) = ler(&F, "Teresina").unwrap();
        assert_eq!((m.as_str(), avisos.len()), ("02", 4)); // jan (o reenvio vale) e fev; abril sem marco fica fora
        let a = &dados["2026"];
        assert_eq!(a.acoes.iter().map(|x| (x.cod.as_str(), x.acao.as_str(), x.orcado, x.liquidado)).collect::<Vec<_>>(), [
            ("08.0.08.244.0005.2200", "CONCESSÃO DE BENEFÍCIOS EVENTUAIS", 8e5, 5e5),
            ("13.0.13.392.0001.2100", "REALIZAÇÃO DAS FESTIVIDADES JUNINAS", 1.6e6, 2.5e6),
            ("0.0.13.000.0000.0000", "CULTURA (liquidado da função)", 0.0, 2.5e6),
            ("0.0.08.000.0000.0000", "ASSISTÊNCIA SOCIAL (liquidado da função)", 0.0, 1.5e6),
            ("0.0.12.000.0000.0000", "EDUCAÇÃO (liquidado da função)", 0.0, 4e7),
        ]);
        assert_eq!((a.liquidado, a.empenhado, a.arrecadado, a.acoes[2].empenhado), (4.7e7, 4.9e7, 9e7, 3.5e6));
        assert_eq!(a.receita, [("Impostos".to_string(), 8e7), ("Transferências Correntes".to_string(), 1e7)]); // a UG da Camara nao soma
        assert!(a.doc_despesa.ends_with("LOA e Relação das notas de empenhos jan-fev (Documentação Web)"));
        assert_eq!((dados["2024"].acoes[1].liquidado, dados["2024"].acoes[1].orcado, dados["2024"].acoes[0].cod.as_str()), (2e6, 1.5e6, "08.0.08.244.0005.2200"));
        let p = std::env::temp_dir().join(format!("braniac-link_tribunais_pi-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        let c = preparar(&F, &setores_falso(FESTAS_EXTRA), "994", "Teresina", &p).unwrap();
        assert_eq!((&c["festas"]["2026"], &c["benef"]["2025"], c["meses"]["2026"].as_str()), (&json!({ "prev": 1.6e6, "gasto": 2.5e6 }), &json!(4e5), Some("02")));
        assert!(preparar(&F, &setores_falso(FESTAS_EXTRA), "1", "Parnaíba", &p).unwrap_err().contains("conferido para Teresina"));
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn recusa_o_que_falta() {
        let erro = |f: Fora| ler(&f, "Teresina").unwrap_err();
        let troca = |de: &'static str, para: &'static str| move |rel: &str| api(rel).map(|t| t.replace(de, para));
        assert!(erro(Fora { api: &troca("\"arrecadada\":90000000.0", "\"arrecadada\":90000002.0"), ..F }).contains("receita do painel 90000000.00 != arrecadado da API 90000002.00"));
        assert!(erro(Fora { api: &troca("\"liquidada\":5000000.0", "\"liquidada\":1000.0"), ..F }).contains("passa do liquidado da função Cultura"));
        assert!(erro(Fora { api: &troca("\"Educação\"", "\"Lazer\""), ..F }).contains("função sem código na Portaria 42"));
        assert!(erro(Fora { api: &troca("Programa de trabalho (Anexo 6), Balanço Geral\",\"exercicio\":2025", "Outro\",\"exercicio\":2025"), ..F }).contains("Anexo 6 do Balanço Geral 2025 não está"));
        assert!(erro(Fora { api: &troca("Lei Orçamentária Anual - LOA\",\"exercicio\":2026", "Lei X\",\"exercicio\":2026"), ..F }).contains("LOA 2026 não está na Documentação Web"));
        assert!(erro(Fora { api: &troca("Relação das notas", "Relatório das notas"), ..F }).contains("nenhuma Relação de empenhos de 2026"));
        assert!(erro(Fora { api: &troca("\"codIBGE\":2211001", "\"codIBGE\":2211002"), ..F }).contains("não achado no Portal da Cidadania"));
        assert!(erro(Fora { painel: &|_| Ok(dsr(&[json!(["CAMARA", "Outras", 5, 0, 5])])), ..F }).contains("sem a UG consolidada P. M. DE TERESINA"));
        assert!(erro(Fora { painel: &|_| painel("").map(|t| t.replace("\"PH\"", "\"RT\":[[1]],\"PH\"")), ..F }).contains("passou de 30 mil linhas"));
        assert!(erro(Fora { painel: &|_| Ok(json!({ "results": [{ "result": { "data": { "erro": "x" } } }] }).to_string()), ..F }).contains("recusou a consulta"));
        assert!(erro(Fora { texto: &|_| Err("pdftotext falhou".into()), ..F }).contains("pdftotext falhou"));
    }
}
