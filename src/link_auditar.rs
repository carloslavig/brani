//! Auditoria de origem dos videos de prefeitura, SEM IA (link/auditar.py): so texto de documento e conta, entao nao tem como inventar.
//!   1. DOCUMENTO: cada valor levantado (acao: previsto, empenhado, liquidado; receita por origem) aparece, igual no centavo, num
//!      documento do tribunal daquele ano;
//!   2. CONTA: cada numero que o video fala (arrecadou, gastou, empenhou, setores, festas, beneficios) e um desses valores ou a soma deles;
//!   3. VIDEO: todo numero do roteiro, da legenda e das folhas e um numero do passo 2 (arredondado como o gerador arredonda).
//! Problema = numero sem prova. `porta` e a trava da esteira ANTES da voz: lista vazia = pode seguir; qualquer outra coisa barra.
//! `triagem`/`consertar` dizem O QUE esta errado comparando tres leituras (o video, o levantamento do PC e o tribunal lido agora).
//! Numero de video vem SO do tribunal de contas: nada aqui aceita outra fonte.
//! O que entra por parametro (este arquivo nao roda programa, nao le PDF, nao fala com rede nem com IA): o texto extraido dos PDFs
//! (`documentos`), reler o tribunal, refazer a legenda, mandar o video de volta para a esteira, conferir capital e avisar o dono.
//! Diferencas do Python: o "t" do historico e o relogio LOCAL em segundos (la era UTC) e ganhou o campo "quando" pronto para o
//! relatorio; a ordem dos problemas de setor segue a ordem alfabetica das chaves do JSON.
#![allow(dead_code)]
use crate::tempo::{Agora, Data};
use regex::{Captures, Regex};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const INTERVALO: f64 = 3.0 * 3600.0;
/// {ano: valores (em centavos) que estao em algum documento do tribunal daquele ano}.
pub type Docs = BTreeMap<String, HashSet<i64>>;

pub struct Locais {
    /// Pasta do pacote de producao: RECEITAS-DESPESAS-*/saida, videos/NNN, estudio-3d/...
    pub pacote: PathBuf,
    /// Copia de seguranca dos projetos (roteiro e legenda de video antigo).
    pub backup: PathBuf,
}

/// Tudo o que sai para fora da conta. Quem chama decide cada uma.
pub struct Fora<'a> {
    /// Valores dos documentos do tribunal da pasta da cidade (texto dos PDFs lido de novo; o .txt ao lado nao vale como prova).
    pub docs: &'a dyn Fn(&Path) -> Docs,
    /// Le o tribunal de novo numa pasta limpa (grava saida/cidade.json la) e devolve {dito: valor}.
    pub reler: &'a dyn Fn(&Value, &Path) -> Result<BTreeMap<String, f64>, String>,
    /// Refaz a legenda da pasta, por codigo.
    pub gera_legenda: &'a dyn Fn(&Path) -> Result<(), String>,
    /// (numero, motivo): registra o pedido de refazer em nome do dono e poe o video como o proximo da esteira.
    pub refazer: &'a dyn Fn(&str, &str) -> Result<(), String>,
    /// Capital (sem PDF no PC): Some(problemas da releitura do tribunal); None = nao e capital.
    pub capital: &'a dyn Fn(&str) -> Option<Vec<String>>,
    /// Aviso ao dono (vai pelo resumo; quem chama decide o canal).
    pub avisar: &'a dyn Fn(&str),
}

/// Tudo comparado em centavos (numero quebrado nao serve para igualdade).
pub fn c(v: f64) -> i64 {
    (v * 100.0).round_ties_even() as i64
}

/// 1234567.891 -> "1.234.567,89".
pub fn br(v: f64) -> String {
    let t = format!("{:.2}", v.abs());
    let (inteiro, cent) = t.split_once('.').unwrap_or((&t, "00"));
    let mut s = String::new();
    for (i, ch) in inteiro.chars().enumerate() {
        if i > 0 && (inteiro.len() - i) % 3 == 0 {
            s.push('.');
        }
        s.push(ch);
    }
    format!("{}{s},{cent}", if v < 0.0 && t != "0.00" { "-" } else { "" })
}

fn ts(a: &Agora) -> f64 {
    (a.data.0 * 86400 + a.segundos()) as f64
}

fn quando(a: &Agora) -> String {
    format!("{} {}", a.data.dia_mes(), a.hm())
}

fn json_de(arq: &Path) -> Result<Value, String> {
    let t = std::fs::read_to_string(arq).map_err(|e| format!("{}: {e}", arq.display()))?;
    serde_json::from_str(&t).map_err(|e| format!("{}: {e}", arq.display()))
}

fn txt(v: &Value) -> String {
    v.as_str().map(String::from).unwrap_or_else(|| v.to_string())
}

fn exige(v: &Value, nome: &str) -> Result<f64, String> {
    v.as_f64().ok_or_else(|| format!("levantamento incompleto: falta {nome}"))
}

fn ou_zero(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.0)
}

/// Valores (em centavos) escritos no texto de um documento: tudo no formato 1.234,56.
pub fn numeros_do_texto(texto: &str) -> HashSet<i64> {
    static NUM_BR: OnceLock<Regex> = OnceLock::new();
    let re = NUM_BR.get_or_init(|| Regex::new(r"\d{1,3}(?:\.\d{3})*,\d{2}").expect("regex fixa"));
    re.find_iter(texto).filter_map(|m| m.as_str().chars().filter(char::is_ascii_digit).collect::<String>().parse().ok()).collect()
}

/// Junta os documentos por ano. `textos`: (nome do PDF "AAAA-MM_...", texto extraido dele).
pub fn documentos(textos: &[(String, String)]) -> Docs {
    let mut out = Docs::new();
    for (nome, texto) in textos {
        out.entry(nome.chars().take(4).collect()).or_default().extend(numeros_do_texto(texto));
    }
    out
}

fn milhar(m2: &str) -> Option<i128> {
    format!("{m2:0<3}").parse().ok()
}

/// Valores de um texto, nas formas do roteiro e da transcricao: "27 milhões e 240 [mil]", "27.240.000", "66 mil e 400", "66.400", "30".
/// Numero que nao da para ler vira -1: nunca tem prova, entao aparece como problema.
pub fn valores(s: &str) -> Vec<i128> {
    static RX: OnceLock<Vec<Regex>> = OnceLock::new();
    let rx = RX.get_or_init(|| {
        let m = "milh(?:ão|ões|ao|oes)";
        let b = "bilh(?:ão|ões|ao|oes)";
        [
            r"\[\d\d:\d\d\.\d+\]|<!--.*?-->|R\s?\$".to_string(),
            r"(\d)\s+([.,]\d)".to_string(),
            format!(r"(?i)\b(\d+),(\d{{1,3}})\s+{b}\b"),
            format!(r"(?i)\b(\d+)\s+{b}(?:,?\s+e\s+(\d+)\s+{m})?"),
            r"\b(\d+),(\d{3}) mil\b".to_string(),
            format!(r"(?i)\b(\d+),(\d{{1,3}})\s+{m}\b"),
            format!(r"(?i)({m}),\s+(\d+ mil)\b"),
            r"\b\d{1,3}(?:,\d{3})+\b".to_string(),
            r"\b(\d+),(\d{1,2}) mil\b".to_string(),
            format!(r"(?i)(\d[\d.]*)(?:,\d\d)?\s+{m}(?:\s+e\s+(\d+)(?:\s+mil)?)?|(\d[\d.]*)\s+mil(?:\s+e\s+(\d+))?|(\d{{1,3}}(?:\.\d{{3}})+)(?:,\d\d)?|(\d+)"),
        ]
        .iter()
        .map(|p| Regex::new(p).expect("regex fixa"))
        .collect()
    });
    let n = |t: &str| t.replace('.', "").parse::<i128>().ok();
    let igual = |m: &Captures| m[0].to_string(); // nao deu para ler: fica como esta e o numero cai no -1 la embaixo
    let s = rx[0].replace_all(s, " ");
    let s = rx[1].replace_all(&s, "${1}${2}");
    // "117 bilhões e 10 milhões" / "117,01 bilhões" viram milhões
    let s = rx[2].replace_all(&s, |m: &Captures| match (n(&m[1]), milhar(&m[2])) {
        (Some(a), Some(b)) => format!("{} milhões", a * 1000 + b),
        _ => igual(m),
    });
    let s = rx[3].replace_all(&s, |m: &Captures| match (n(&m[1]), m.get(2).map_or(Some(0), |x| n(x.as_str()))) {
        (Some(a), Some(b)) => format!("{} milhões", a * 1000 + b),
        _ => igual(m),
    });
    let s = rx[4].replace_all(&s, "${1} milhões e ${2} mil"); // transcricao: "1,120 mil" = 1 milhao e 120 mil
    let s = rx[5].replace_all(&s, |m: &Captures| match milhar(&m[2]) {
        Some(b) => format!("{} milhões e {b} mil", &m[1]), // "2,7 milhões" = 2 milhoes e 700 mil
        None => igual(m),
    });
    let s = rx[6].replace_all(&s, "${1} e ${2}");
    let s = rx[7].replace_all(&s, |m: &Captures| m[0].replace(',', "")); // transcricao: "92,240,000"
    let s = rx[8].replace_all(&s, |m: &Captures| format!("{} mil e {:0<3}", &m[1], &m[2]));
    let mut out = vec![];
    for m in rx[9].captures_iter(&s) {
        let g = |i: usize| m.get(i).map(|x| x.as_str());
        let v = if let Some(a) = g(1) {
            n(a).zip(g(2).map_or(Some(0), n)).map(|(a, b)| a * 1_000_000 + b * 1000)
        } else if let Some(a) = g(3) {
            n(a).zip(g(4).map_or(Some(0), n)).map(|(a, b)| a * 1000 + b)
        } else {
            g(5).or(g(6)).and_then(n)
        };
        out.push(v.unwrap_or(-1));
    }
    out
}

fn arred(x: f64) -> i64 {
    (x + 0.5).floor() as i64
}

/// O que a voz fala para um valor (o mesmo arredondamento do gerador do roteiro).
pub fn dito(v: f64) -> String {
    if v >= 1e9 {
        let t = arred(v / 1e7);
        let (b, mm) = (t / 100, t % 100);
        return format!("{b} {}{}", if b == 1 { "bilhão" } else { "bilhões" }, if mm > 0 { format!(" e {} milhões", mm * 10) } else { String::new() });
    }
    if v >= 1e6 {
        let t = arred(v / 1e4);
        let (m, k) = (t / 100, t % 100);
        return format!("{m} {}{}", if m == 1 { "milhão" } else { "milhões" }, if k > 0 { format!(" e {}", k * 10) } else { String::new() });
    }
    if v >= 1e5 {
        return format!("{} mil", arred(v / 1e3));
    }
    if v < 1000.0 {
        return format!("{} reais", arred(v));
    }
    let t = arred(v / 100.0);
    format!("{} mil{}", t / 10, if t % 10 > 0 { format!(" e {}", t % 10 * 100) } else { String::new() })
}

/// Roteiro, legenda ou fontes do video: no projeto, na pasta de edicao ou na copia de seguranca. "" = nao esta mais no PC.
fn arquivo(l: &Locais, n: &str, sufixo: &str, onde: Option<&str>) -> Result<String, String> {
    let Ok(cfg) = json_de(&l.pacote.join("videos").join(n).join("preparo.json")) else { return Ok(String::new()) };
    let (Some(projeto), Some(nome)) = (cfg["projeto"].as_str(), cfg["nome"].as_str()) else { return Err("preparo.json sem projeto ou nome".into()) };
    let mut cands = vec![];
    if let Some(o) = onde {
        cands.push(l.pacote.join("estudio-3d").join("projetos").join(projeto).join("audio").join(o));
    }
    cands.push(l.pacote.join("estudio-3d").join("edicoes").join(projeto).join(format!("{nome} - {sufixo}")));
    cands.push(l.backup.join(projeto).join(format!("{nome} - {sufixo}")));
    match cands.iter().find(|p| p.exists()) {
        Some(p) => std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display())),
        None => Ok(String::new()),
    }
}

struct Provas<'a> {
    docs: &'a Docs,
    falavel: BTreeMap<i64, f64>, // o que o video pode dizer
    prob: Vec<String>,
}

impl Provas<'_> {
    /// O valor esta no documento do ano ou e igual a uma conta refeita aqui com valores do documento.
    fn prova(&mut self, ano: &str, nome: &str, v: f64, somas: &[f64]) {
        if v == 0.0 {
            return;
        }
        if self.docs.get(ano).is_some_and(|d| d.contains(&c(v))) || somas.iter().any(|s| (v - s).abs() < 0.015) {
            self.falavel.insert(c(v), v);
        } else if !self.docs.is_empty() {
            self.prob.push(format!("{ano} {nome}: R$ {} não está no documento do tribunal nem fecha com a soma das linhas", br(v)));
        } else {
            self.falavel.insert(c(v), v); // sem documento aqui nao da para reprovar; a nota avisa
        }
    }
}

/// (problemas, notas) de uma cidade. Sem problema = todo numero do video tem prova no documento do tribunal.
/// Err = levantamento pela metade (nao deu para auditar): quem chama trata como NAO aprovado.
pub fn conferir(l: &Locais, pasta: &Path, docs: &Docs) -> Result<(Vec<String>, Vec<String>), String> {
    let saida = pasta.join("saida");
    let (cid, setores) = (json_de(&saida.join("cidade.json"))?, json_de(&saida.join("setores.json"))?);
    let mut notas = vec![];
    if docs.is_empty() {
        notas.push("sem PDF do tribunal no PC: a origem não foi conferida aqui (capital e MT: conferir_capitais refaz pelo tribunal)".to_string());
    }
    let meses = cid["meses"].as_object().filter(|m| !m.is_empty()).ok_or("levantamento incompleto: falta meses")?;
    let ultimo = meses.keys().max().cloned().unwrap_or_default();
    let mut folhas: HashSet<i64> = HashSet::new(); // valores provados no documento
    let mut p = Provas { docs, falavel: BTreeMap::new(), prob: vec![] };

    for (ano, mm) in meses {
        let mm = txt(mm);
        let acoes = json_de(&saida.join(format!("acoes-{ano}-{mm}.json")))?;
        let rec = json_de(&saida.join(format!("receita-{ano}.json")))?;
        let acoes = acoes.as_array().ok_or("levantamento incompleto: acoes")?;
        let rec = rec.as_array().ok_or("levantamento incompleto: receita")?;
        let mut linhas: Vec<(String, f64)> = vec![];
        for a in acoes {
            for k in ["previsto", "empenhado", "liquidado"] {
                if ou_zero(&a[k]) != 0.0 {
                    linhas.push((format!("ação {} ({k})", txt(&a["cod"])), ou_zero(&a[k])));
                }
            }
        }
        linhas.extend(rec.iter().filter(|r| ou_zero(&r[1]) != 0.0).map(|r| (format!("receita {}", txt(&r[0])), ou_zero(&r[1]))));
        let no_doc = |v: f64| docs.get(ano).is_some_and(|d| d.contains(&c(v)));
        let fora: Vec<&(String, f64)> = linhas.iter().filter(|(_, v)| !docs.is_empty() && !no_doc(*v)).collect();
        let de_fora: HashSet<i64> = fora.iter().map(|(_, v)| c(*v)).collect();
        folhas.extend(linhas.iter().map(|(_, v)| c(*v)).filter(|x| !de_fora.contains(x)));
        if !fora.is_empty() {
            let ex: Vec<String> = fora.iter().take(3).map(|(o, v)| format!("{o} R$ {}", br(*v))).collect();
            p.prob.push(format!("{ano}: {} de {} valores levantados não estão no documento do tribunal, ex.: {}", fora.len(), linhas.len(), ex.join("; ")));
        }
        let soma = |k: &str, so: Option<&HashSet<String>>| acoes.iter().filter(|a| so.is_none_or(|s| s.contains(&txt(&a["cod"])))).map(|a| ou_zero(&a[k])).sum::<f64>();
        let a = &cid["anos"][ano];
        let liq: HashSet<(String, i64)> = acoes.iter().map(|x| (txt(&x["cod"]), c(ou_zero(&x["liquidado"])))).collect();
        let (arrec, gastou) = (exige(&a["arrec"], "arrec")?, exige(&a["gastou"], "gastou")?);
        let mut total_rec = 0.0;
        for r in rec {
            total_rec += exige(&r[1], "valor da receita")?;
        }
        p.prova(ano, "arrecadou", arrec, &[total_rec]);
        p.prova(ano, "gastou", gastou, &[soma("liquidado", None)]);
        if *ano == ultimo {
            p.prova(ano, "empenhou", exige(&a["emp"], "emp")?, &[soma("empenhado", None)]);
        }
        p.falavel.insert(c((arrec - gastou).abs()), (arrec - gastou).abs()); // sobrou / gastou a mais: conta de dois provados
        let cods: HashSet<String> = cid["festas_fonte"][ano].as_array().into_iter().flatten().map(|f| txt(&f["acao"])).collect();
        let festas = cid["festas"][ano].as_object().ok_or("levantamento incompleto: falta festas do ano")?;
        let de = |k: &str| festas.get(k).map_or(0.0, ou_zero);
        p.prova(ano, "achado (previa)", de("prev"), &[soma("previsto", Some(&cods))]);
        p.prova(ano, "achado (gastou)", de("gasto"), &[soma("liquidado", Some(&cods))]);
        let benef = cid["benef"].as_object().ok_or("levantamento incompleto: falta benef")?.get(ano).map_or(0.0, ou_zero);
        p.prova(ano, "benefícios eventuais", benef, &acoes.iter().map(|x| ou_zero(&x["liquidado"])).collect::<Vec<_>>());
        for (setor, d) in setores[format!("{ano}-{mm}")].as_object().into_iter().flatten() {
            let do_setor: Vec<&Value> = d["acoes"].as_array().into_iter().flatten().collect();
            let soltas: Vec<&&Value> = do_setor.iter().filter(|x| !liq.contains(&(txt(&x[2]), c(ou_zero(&x[1]))))).collect();
            if let Some(x) = soltas.first() {
                p.prob.push(format!("{ano} setor {setor}: ação que não está no levantamento: {} R$ {}", txt(&x[0]), br(ou_zero(&x[1]))));
            }
            let somas: Vec<f64> = if !do_setor.is_empty() && soltas.is_empty() { vec![do_setor.iter().map(|x| ou_zero(&x[1])).sum()] } else { vec![] };
            p.prova(ano, &format!("setor {setor}"), exige(&d["liquidado"], "liquidado do setor")?, &somas);
        }
    }

    let Provas { falavel, mut prob, .. } = p;
    let n = txt(&cid["numero"]);
    let rot = arquivo(l, &n, "roteiro.txt", Some("roteiro-falado.txt"))?;
    if !rot.is_empty() {
        let mut ok: HashSet<i128> = falavel.values().flat_map(|v| valores(&dito(*v))).collect();
        ok.extend([2024, 2025, 2026]);
        let dos_meses = meses.values().map(txt).collect::<Vec<_>>().join(" ");
        ok.extend(Regex::new(r"\d+").expect("regex fixa").find_iter(&dos_meses).filter_map(|m| m.as_str().parse::<i128>().ok()));
        let soltos: BTreeSet<i128> = valores(&rot).into_iter().filter(|v| !ok.contains(v)).collect();
        if !soltos.is_empty() {
            prob.push(format!("roteiro fala número sem prova: [{}]", soltos.iter().take(6).map(|v| v.to_string()).collect::<Vec<_>>().join(", ")));
        }
    } else {
        notas.push("roteiro não está mais no PC".to_string());
    }
    let leg = arquivo(l, &n, "legenda.txt", None)?;
    let leg = leg.split("\u{2696}\u{fe0f}").next().unwrap_or(""); // o aviso legal cita artigos de lei, nao valores
    if !leg.is_empty() {
        let reais = Regex::new(r"R\$\s?(\d+)(?:,(\d+))?(?:\s?(bi|mi|mil)\b)?").expect("regex fixa");
        let mut soltos = vec![];
        for m in reais.captures_iter(leg) {
            let u = match m.get(3).map(|x| x.as_str()) { Some("bi") => 1e9, Some("mi") => 1e6, Some("mil") => 1e3, _ => 1.0 };
            let dec = m.get(2).map_or("", |x| x.as_str());
            let casas = 10f64.powi(dec.len() as i32);
            let lido = m[1].parse::<f64>().ok().zip(if dec.is_empty() { Some(0.0) } else { dec.parse::<f64>().ok() });
            // valor que nao da para ler nao tem prova
            let Some((int, frac)) = lido else { soltos.push(m[0].to_string()); continue };
            let (x, tol) = ((int + frac / casas) * u, u / casas); // 1 unidade da ultima casa escrita
            if x != 0.0 && !falavel.values().any(|v| (v - x).abs() <= tol) {
                soltos.push(m[0].to_string());
            }
        }
        if !soltos.is_empty() {
            prob.push(format!("legenda com valor sem prova: [{}]", soltos.iter().take(6).map(|s| format!("'{s}'")).collect::<Vec<_>>().join(", ")));
        }
    } else {
        notas.push("legenda não achada".to_string());
    }
    match json_de(&l.pacote.join("videos").join(&n).join("folhas.json")) {
        Ok(fj) => {
            for f in fj["folhas"].as_array().ok_or("folhas.json sem folhas")? {
                let provado = |v: &Value| v.as_f64().is_some_and(|x| folhas.contains(&c(x)) || falavel.contains_key(&c(x)));
                let ruins: Vec<&Value> = f["linhas"].as_array().ok_or("folha sem linhas")?.iter().filter(|x| !x[0].as_str().unwrap_or("").starts_with("Outras") && !provado(&x[1])).collect();
                let total_ok = f["total"].as_f64().is_some_and(|t| falavel.contains_key(&c(t)));
                if !total_ok || !ruins.is_empty() {
                    let titulo: String = f["titulo"].as_str().unwrap_or("").chars().take(50).collect();
                    let o_que = match ruins.first() { Some(r) => format!("linha sem prova {r}"), None => format!("total R$ {} sem prova", br(ou_zero(&f["total"]))) };
                    prob.push(format!("folha '{titulo}': {o_que}"));
                }
            }
        }
        Err(_) => notas.push("folhas da tela não estão mais no PC".to_string()),
    }
    Ok((prob, notas))
}

/// Trava da esteira ANTES da voz: devolve os problemas (lista vazia = pode seguir). Levantamento que nao da para auditar tambem barra.
/// Capital nao tem PDF no PC: a prova dela e o tribunal, lido de novo, dar o mesmo numero (pega download pela metade ou repetido).
pub fn porta(l: &Locais, pasta: &Path, docs: &Docs, capital: &dyn Fn(&str) -> Option<Vec<String>>) -> Vec<String> {
    let lido = conferir(l, pasta, docs).and_then(|(prob, _)| Ok((prob, json_de(&pasta.join("saida").join("cidade.json"))?)));
    let (prob, cid) = match lido {
        Ok(x) => x,
        Err(e) => return vec![format!("não deu para auditar: {e}")],
    };
    if prob.is_empty() && cid["fonte"] == "tribunal" {
        if let Some(da_capital) = capital(&txt(&cid["numero"])) {
            return da_capital.into_iter().filter(|p| !p.starts_with("voz:")).collect();
        }
    }
    prob
}

/// Titulo de cada categoria da triagem, na ordem do relatorio.
pub const NOMES: [(&str, &str); 5] = [
    ("legenda", "Legenda com valor antigo"),
    ("levantamento", "Levantamento do PC estava errado (o vídeo está certo)"),
    ("tribunal_mudou", "O tribunal mudou o número depois da consulta"),
    ("refazer", "Vídeo não bate com o tribunal: refazer"),
    ("pessoa", "Precisa de uma pessoa olhar"),
];

/// {dito: valor} do fontes.md que foi com o video: o que o video afirma, no centavo.
pub fn numeros_do_video(l: &Locais, n: &str) -> Result<BTreeMap<String, f64>, String> {
    let re = Regex::new(r"(?m)^\|\s*(.+?)\s*\|\s*R\$ ([\d.]+,\d\d)\s*\|").expect("regex fixa");
    let texto = arquivo(l, n, "fontes.md", Some("fontes.md"))?;
    Ok(re.captures_iter(&texto).filter_map(|m| Some((m[1].to_string(), m[2].replace('.', "").replace(',', ".").parse().ok()?))).collect())
}

fn totais(cid: &Value) -> BTreeMap<String, f64> {
    cid["totais"].as_array().into_iter().flatten().filter_map(|t| Some((t["dito"].as_str()?.to_string(), t["valor"].as_f64()?))).collect()
}

fn corta(s: String, n: usize) -> String {
    s.chars().take(n).collect()
}

/// O QUE esta errado, sem IA. Devolve (categoria de NOMES, explicacao, pasta com a leitura nova ou None).
/// `tmp`: onde criar a pasta limpa da releitura.
pub fn triagem(l: &Locais, pasta: &Path, prob: &[String], reler: &dyn Fn(&Value, &Path) -> Result<BTreeMap<String, f64>, String>, tmp: &Path) -> Result<(&'static str, String, Option<PathBuf>), String> {
    let cid = json_de(&pasta.join("saida").join("cidade.json"))?;
    if prob.iter().all(|p| p.starts_with("legenda")) {
        return Ok(("legenda", "só a legenda tem valor que não é o do vídeo; roteiro e tela batem".into(), None));
    }
    let n = txt(&cid["numero"]);
    let v = numeros_do_video(l, &n)?;
    if cid["uf"].as_str().unwrap_or("").is_empty() || v.is_empty() {
        // a prova e o PDF, ou o video nao tem fontes.md guardado: a esteira refaz do zero
        let dois = prob.iter().take(2).cloned().collect::<Vec<_>>().join("; ");
        return Ok(("refazer", format!("o vídeo tem número sem prova no documento do tribunal: {}", corta(dois, 300)), None));
    }
    let d = totais(&cid);
    let nova = tmp.join(format!("auditoria-{n}-{}", crate::link_identidade::id_novo(8)));
    std::fs::create_dir_all(&nova).map_err(|e| e.to_string())?;
    let f = match reler(&cid, &nova) {
        Ok(f) => f,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&nova);
            return Ok(("pessoa", format!("não consegui reler o tribunal agora ({})", corta(e, 120)), None));
        }
    };
    type M = BTreeMap<String, f64>;
    let dif = |a: &M, b: &M| -> Vec<String> { a.iter().filter(|(k, x)| b.get(*k).is_some_and(|y| (*x - y).abs() > 0.01)).map(|(k, _)| k.clone()).collect() };
    let lado = |ks: &[String], a: &M, na: &str, b: &M, nb: &str| ks.iter().take(4).map(|k| format!("{k}: {na} R$ {}, {nb} R$ {}", br(a[k]), br(b[k]))).collect::<Vec<_>>().join("; ");
    let mud = dif(&v, &f);
    if mud.is_empty() {
        return Ok(("levantamento", format!("o vídeo bate com o tribunal lido agora; errado estava o levantamento do PC ({})", lado(&dif(&v, &d), &v, "vídeo", &d, "PC")), Some(nova)));
    }
    let _ = std::fs::remove_dir_all(&nova);
    if !dif(&d, &f).is_empty() {
        return Ok(("pessoa", format!("três leituras diferentes (vídeo, PC e tribunal agora): {}", lado(&mud, &v, "vídeo", &f, "tribunal agora")), None));
    }
    let aberto = format!("({}", cid["meses"].as_object().and_then(|m| m.keys().max().cloned()).ok_or("levantamento incompleto: falta meses")?);
    if mud.iter().all(|k| k.contains(&aberto)) {
        return Ok(("tribunal_mudou", format!("ano ainda aberto, o tribunal atualizou depois da consulta: {}", lado(&mud, &v, "vídeo", &f, "tribunal agora")), None));
    }
    Ok(("refazer", format!("o vídeo não bate com o tribunal (lido duas vezes, igual): {}", lado(&mud, &v, "vídeo", &f, "tribunal")), None))
}

fn copiar_pasta(de: &Path, para: &Path) -> Result<(), String> {
    std::fs::create_dir_all(para).map_err(|e| e.to_string())?;
    for e in std::fs::read_dir(de).map_err(|e| e.to_string())?.flatten() {
        let alvo = para.join(e.file_name());
        if e.path().is_dir() {
            copiar_pasta(&e.path(), &alvo)?;
        } else {
            std::fs::copy(e.path(), &alvo).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Age conforme a triagem. Devolve (categoria, texto do que achou e do que fez). `pode_refazer` false = ja voltou para a esteira
/// ha pouco: nao entra em laco.
pub fn consertar(l: &Locais, pasta: &Path, prob: &[String], f: &Fora, pode_refazer: bool, tmp: &Path) -> Result<(&'static str, String), String> {
    let (cat, porque, nova) = triagem(l, pasta, prob, f.reler, tmp)?;
    let feito = match (cat, nova) {
        ("legenda", _) => match (f.gera_legenda)(pasta) {
            Ok(()) => "legenda refeita por código; falta reenviar ao site".to_string(),
            Err(e) => format!("o gerador da legenda falhou: {}", e.chars().rev().take(160).collect::<Vec<_>>().into_iter().rev().collect::<String>()),
        },
        ("levantamento", Some(nova)) => {
            let r = copiar_pasta(&nova, pasta);
            let _ = std::fs::remove_dir_all(&nova);
            r?;
            "levantamento do PC trocado pela leitura conferida do tribunal".to_string()
        }
        ("refazer", _) if pode_refazer => {
            let n = txt(&json_de(&pasta.join("saida").join("cidade.json"))?["numero"]);
            (f.refazer)(&n, &porque)?;
            "o vídeo voltou para a esteira como o próximo".to_string()
        }
        ("refazer", _) => "já voltou para a esteira nos últimos 7 dias e continua sem bater: agora é com uma pessoa".to_string(),
        _ => "nada automático: fica no relatório para os donos decidirem".to_string(),
    };
    Ok((cat, format!("{porque}. Feito: {feito}.")))
}

fn hist(dados: &Path) -> PathBuf {
    dados.join("magno").join("auditoria_historico.jsonl")
}

fn historico(dados: &Path) -> Vec<Value> {
    std::fs::read_to_string(hist(dados)).unwrap_or_default().lines().filter(|x| !x.trim().is_empty()).filter_map(|x| serde_json::from_str(x).ok()).collect()
}

fn quando_de(h: &Value) -> String {
    if let Some(q) = h["quando"].as_str() {
        return q.to_string();
    }
    // shortcut: linha antiga (do Python) so tem "t" em UTC; sai na hora de UTC, sem fuso
    let t = h["t"].as_f64().unwrap_or(0.0) as i64;
    format!("{} {:02}:{:02}", Data(t.div_euclid(86400)).dia_mes(), t.rem_euclid(86400) / 3600, t.rem_euclid(3600) / 60)
}

/// Relatorio para o dono, montado por codigo a partir da auditoria (nenhuma IA escreve aqui). Grava dados/magno/relatorio_auditoria.md.
pub fn relatorio(dados: &Path, conf: &Map<String, Value>, agora: &Agora, sem_auditar: &[String]) -> Result<PathBuf, String> {
    let ruim = |v: &Value| v["ok"] != true && v["agora_ok"] != true;
    let n_ruins = conf.values().filter(|v| ruim(v)).count();
    let mut t: Vec<String> = vec![
        format!("# Auditoria dos vídeos de prefeitura ({})", quando(agora)), String::new(),
        "Feita só por código: cada número é comparado com o documento do tribunal. Nenhuma IA participa.".into(), String::new(),
        format!("- Vídeos conferidos no PC: {}", conf.len()), format!("- Certos: {}", conf.len() - n_ruins), format!("- Com problema agora: {n_ruins}"), String::new(),
    ];
    let tipos = NOMES.iter().map(|(k, v)| (Some(*k), *v)).chain([(None, "Problema ainda sem triagem")]);
    for (cat, titulo) in tipos {
        let do_tipo: Vec<(&String, &Value)> = conf.iter().filter(|(_, v)| ruim(v) && v["categoria"].as_str() == cat).collect();
        if do_tipo.is_empty() {
            continue;
        }
        t.extend([format!("## {titulo}"), String::new()]);
        for (n, v) in do_tipo {
            t.extend([format!("### {n} {}", txt(&v["cidade"])), String::new()]);
            if let Some(feito) = v["feito"].as_str().filter(|x| !x.is_empty()) {
                t.extend([feito.to_string(), String::new()]);
            }
            t.extend(v["problemas"].as_array().into_iter().flatten().take(12).map(|p| format!("- {}", txt(p))));
            t.push(String::new());
        }
    }
    let feitos: Vec<Value> = historico(dados).into_iter().filter(|h| matches!(h["categoria"].as_str(), Some("legenda" | "levantamento" | "refazer"))).collect();
    if !feitos.is_empty() {
        t.extend(["## O que a auditoria já consertou ou mandou refazer".to_string(), String::new()]);
        t.extend(feitos[feitos.len().saturating_sub(60)..].iter().map(|h| format!("- {} · {} {}: {}", quando_de(h), txt(&h["numero"]), txt(&h["cidade"]), txt(&h["texto"]))));
        t.push(String::new());
    }
    // videos ja publicados cujos dados sairam do PC: reauditados baixando os PDFs de novo
    match json_de(&dados.join("magno").join("reauditoria.json")).ok().and_then(|v| v.as_object().cloned()) {
        Some(re) => {
            let mal: Vec<(&String, &Value)> = re.iter().filter(|(_, v)| v["ok"] == false).collect();
            t.extend(["## Vídeos já publicados (PDFs baixados de novo)".to_string(), String::new(), format!("- Reauditados: {}", re.len()), format!("- Com divergência: {}", mal.len()), String::new()]);
            for (n, v) in mal {
                t.extend([format!("### {n} {}", v["cidade"].as_str().unwrap_or("")), String::new()]);
                t.extend(v["problemas"].as_array().into_iter().flatten().take(12).map(|p| format!("- {}", txt(p))));
                t.push(String::new());
            }
        }
        None => t.extend(["## Vídeos já publicados (139 a 231)".to_string(), String::new(), "A reauditoria deles ainda não terminou (os PDFs estão sendo baixados de novo do e-TCM).".to_string(), String::new()]),
    }
    if !sem_auditar.is_empty() {
        t.extend(["## Não deu para auditar (levantamento ainda pela metade)".to_string(), String::new()]);
        t.extend(sem_auditar.iter().map(|x| format!("- {x}")));
        t.push(String::new());
    }
    let certos: Vec<String> = conf.iter().filter(|(_, v)| !ruim(v)).map(|(n, v)| format!("{n} {}", txt(&v["cidade"]))).collect();
    t.extend(["## Certos".to_string(), String::new(), if certos.is_empty() { "nenhum".to_string() } else { certos.join(", ") }, String::new()]);
    let arq = dados.join("magno").join("relatorio_auditoria.md");
    std::fs::create_dir_all(dados.join("magno")).map_err(|e| e.to_string())?;
    std::fs::write(&arq, t.join("\n")).map_err(|e| e.to_string())?;
    Ok(arq)
}

fn mexido(arq: &Path) -> f64 {
    std::fs::metadata(arq).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0.0, |d| d.as_secs_f64())
}

pub struct Rodada<'a> {
    /// Numeros a auditar; vazio = todos.
    pub alvos: &'a [&'a str],
    pub avisar: bool,
    /// Pula a cidade cujo levantamento e folhas nao mudaram desde a ultima auditoria (rodada da rotina).
    pub so_novas: bool,
    /// Faz a triagem do que deu problema e conserta ou manda refazer.
    pub arrumar: bool,
}

/// Audita as cidades do pacote e grava dados/magno/auditoria.json e o relatorio. Devolve o conferido {numero: {...}}.
pub fn rodar(dados: &Path, l: &Locais, r: &Rodada, agora: &Agora, f: &Fora) -> Result<Map<String, Value>, String> {
    let arq = dados.join("magno").join("auditoria.json");
    let mut conf: Map<String, Value> = json_de(&arq).ok().and_then(|v| v.as_object().cloned()).unwrap_or_default();
    let (mut novos, mut sem) = (vec![], vec![]);
    let mut pastas: Vec<PathBuf> = std::fs::read_dir(&l.pacote).into_iter().flatten().flatten().map(|e| e.path())
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("RECEITAS-DESPESAS-")) && p.join("saida").join("cidade.json").exists()).collect();
    pastas.sort();
    for pasta in pastas {
        let cj = pasta.join("saida").join("cidade.json");
        let apelido = pasta.file_name().map(|n| n.to_string_lossy().replace("RECEITAS-DESPESAS-", "")).unwrap_or_default();
        let Ok(cid) = json_de(&cj) else { sem.push(apelido); continue };
        let (n, nome) = (txt(&cid["numero"]), txt(&cid["nome"]));
        let sinal = mexido(&cj).max(mexido(&l.pacote.join("videos").join(&n).join("folhas.json")));
        if (!r.alvos.is_empty() && !r.alvos.contains(&n.as_str())) || (r.so_novas && conf.get(&n).and_then(|v| v["sinal"].as_f64()).is_some_and(|s| (s - sinal).abs() < 1e-3)) {
            continue;
        }
        // levantamento pela metade (cidade em preparo): fica para a proxima rodada
        let Ok((prob, notas)) = conferir(l, &pasta, &(f.docs)(&pasta)) else { sem.push(apelido); continue };
        let velho = conf.get(&n).cloned().unwrap_or(Value::Null);
        let antes: HashSet<String> = velho["problemas"].as_array().into_iter().flatten().map(txt).collect();
        let mut novo = json!({ "cidade": nome, "ok": prob.is_empty(), "quando": ts(agora), "sinal": sinal, "problemas": prob, "notas": notas });
        if !prob.is_empty() && velho["categoria"].as_str().is_some_and(|x| !x.is_empty()) && velho["problemas"] == json!(prob) {
            for k in ["categoria", "feito", "agora_ok"] {
                if !velho[k].is_null() {
                    novo[k] = velho[k].clone(); // ja triado e nada mudou: nao rele o tribunal de novo
                }
            }
        } else if !prob.is_empty() && r.arrumar {
            let ja = historico(dados).iter().any(|h| txt(&h["numero"]) == n && h["categoria"] == "refazer" && ts(agora) - h["t"].as_f64().unwrap_or(0.0) < 7.0 * 86400.0);
            let (cat, texto, depois) = match consertar(l, &pasta, &prob, f, !ja, &dados.join("magno").join("tmp")) {
                Ok((cat, texto)) if cat == "legenda" || cat == "levantamento" => match conferir(l, &pasta, &(f.docs)(&pasta)) {
                    Ok((depois, _)) => (cat, texto, depois),
                    Err(e) => (cat, texto, vec![e]),
                },
                Ok((cat, texto)) => (cat, texto, prob.clone()),
                Err(e) => ("pessoa", format!("o conserto automático falhou: {}", corta(e, 160)), prob.clone()),
            };
            novo["categoria"] = json!(cat);
            novo["feito"] = json!(texto);
            novo["agora_ok"] = json!(depois.is_empty());
            let linha = json!({ "t": ts(agora), "quando": quando(agora), "numero": n, "cidade": nome, "categoria": cat, "texto": texto, "agora_ok": depois.is_empty() });
            let mut tudo = std::fs::read_to_string(hist(dados)).unwrap_or_default();
            tudo.push_str(&format!("{linha}\n"));
            std::fs::create_dir_all(dados.join("magno")).and_then(|_| std::fs::write(hist(dados), tudo)).map_err(|e| e.to_string())?;
        }
        if prob.iter().any(|p| !antes.contains(p)) {
            let o_que = novo["feito"].as_str().filter(|x| !x.is_empty()).map(String::from).unwrap_or_else(|| prob.iter().take(2).cloned().collect::<Vec<_>>().join("; "));
            novos.push(format!("{n} {nome}: {}", corta(o_que, 300)));
        }
        conf.insert(n, novo);
    }
    std::fs::create_dir_all(dados.join("magno")).map_err(|e| e.to_string())?;
    std::fs::write(&arq, serde_json::to_string_pretty(&conf).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    relatorio(dados, &conf, agora, &sem)?;
    if !novos.is_empty() && r.avisar {
        (f.avisar)(&format!("Auditoria dos vídeos achou número sem prova no documento do tribunal (sem IA, só conta):\n{}", corta(novos.join("\n"), 1200)));
    }
    Ok(conf)
}

/// No relogio da rotina: uma rodada a cada 3 h, so nas cidades que mudaram, ja consertando. None = ainda nao e hora.
pub fn tick(dados: &Path, l: &Locais, agora: &Agora, f: &Fora) -> Option<Result<Map<String, Value>, String>> {
    let marca = dados.join("magno").join("auditoria.ultima");
    if std::fs::read_to_string(&marca).ok().and_then(|t| t.trim().parse::<f64>().ok()).is_some_and(|u| ts(agora) - u < INTERVALO) {
        return None;
    }
    if let Err(e) = std::fs::create_dir_all(dados.join("magno")).and_then(|_| std::fs::write(&marca, ts(agora).to_string())) {
        return Some(Err(e.to_string()));
    }
    Some(rodar(dados, l, &Rodada { alvos: &[], avisar: true, so_novas: true, arrumar: true }, agora, f))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const COD: &str = "2.2120.13.392.10.2.2117";
    const ROTEIRO: &str = "2026, até setembro. Arrecadou 10 milhões e 460. Gastou 8 milhões e 740. Festas, 718 mil. A ajuda às famílias ficou em 22 mil e 200. Segue o perfil.";
    const LEGENDA: &str = "2026: arrecadou R$ 10,46 mi e gastou R$ 8,74 mi. Festas: R$ 718 mil. \u{2696}\u{fe0f} AVISO: art. 5º, R$ 999 mil";

    fn acoes() -> Value {
        json!([{ "cod": "1.1", "previsto": 9000000.0, "empenhado": 8500000.0, "liquidado": 8000000.0 },
               { "cod": COD, "previsto": 980000.0, "empenhado": 800000.0, "liquidado": 718313.42 },
               { "cod": "6.6", "previsto": 30000.0, "empenhado": 25000.0, "liquidado": 22196.93 }])
    }

    fn receita() -> Value {
        json!([["Impostos", 1463552.21, 1], ["Transferências Correntes", 9000000.0, 5]])
    }

    /// O "PDF do tribunal" tem os valores originais; o que nao estiver aqui nao tem prova.
    fn docs() -> Docs {
        let mut texto = String::new();
        for a in acoes().as_array().unwrap() {
            for k in ["previsto", "empenhado", "liquidado"] {
                texto += &format!("linha {} x\n", br(a[k].as_f64().unwrap()));
            }
        }
        for r in receita().as_array().unwrap() {
            texto += &format!("{} ", br(r[1].as_f64().unwrap()));
        }
        documentos(&[("2026-09_PCMGE.pdf".into(), texto), ("2025-12_OUTRO.pdf".into(), "1,00".into())])
    }

    fn grava(arq: &Path, texto: &str) {
        std::fs::create_dir_all(arq.parent().unwrap()).unwrap();
        std::fs::write(arq, texto).unwrap();
    }

    fn cidade(nome: &str, acoes_: Value, roteiro: &str, legenda: &str) -> (PathBuf, Locais, PathBuf) {
        let tmp = std::env::temp_dir().join(format!("braniac-link_auditar-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let pasta = tmp.join("pacote").join("RECEITAS-DESPESAS-X");
        let gastou: f64 = acoes().as_array().unwrap().iter().map(|a| a["liquidado"].as_f64().unwrap()).sum();
        let cid = json!({ "numero": "777", "nome": "X", "meses": { "2026": "09" }, "anos": { "2026": { "arrec": 10463552.21, "gastou": gastou, "emp": 9325000.0 } },
            "festas": { "2026": { "prev": 980000.0, "gasto": 718313.42 } }, "festas_fonte": { "2026": [{ "acao": COD }] }, "benef": { "2026": 22196.93 } });
        grava(&pasta.join("saida/cidade.json"), &cid.to_string());
        grava(&pasta.join("saida/acoes-2026-09.json"), &acoes_.to_string());
        grava(&pasta.join("saida/receita-2026.json"), &receita().to_string());
        let setores = json!({ "2026-09": { "Educação": { "liquidado": 8000000.0, "acoes": [["Ensino", 8000000.0, "1.1", 3]] },
            "Festas e eventos": { "liquidado": 718313.42, "acoes": [["Festas", 718313.42, COD, 24]] } } });
        grava(&pasta.join("saida/setores.json"), &setores.to_string());
        grava(&tmp.join("pacote/videos/777/preparo.json"), &json!({ "projeto": "777. x", "nome": "X" }).to_string());
        grava(&tmp.join("pacote/estudio-3d/projetos/777. x/audio/roteiro-falado.txt"), roteiro);
        grava(&tmp.join("pacote/estudio-3d/edicoes/777. x/X - legenda.txt"), legenda);
        std::fs::create_dir_all(tmp.join("dados/magno")).unwrap();
        let l = Locais { pacote: tmp.join("pacote"), backup: tmp.join("sem-backup") };
        (tmp, l, pasta)
    }

    fn nada<'a>(d: &'a dyn Fn(&Path) -> Docs) -> Fora<'a> {
        Fora { docs: d, reler: &|_, _| panic!("nao era para reler"), gera_legenda: &|_| panic!("nao era para gerar legenda"), refazer: &|_, _| panic!("nao era para refazer"),
               capital: &|_| None, avisar: &|_| panic!("nao era para avisar") }
    }

    fn agora() -> Agora {
        Agora { data: Data::civil(2026, 11, 18).unwrap(), hora: 10, min: 30, seg: 0 }
    }

    #[test]
    fn contas_de_apoio() {
        assert_eq!((c(718313.42), c(0.005), c(22196.93)), (71831342, 0, 2219693));
        assert_eq!((br(1234567.891), br(0.0), br(999.5), br(-1500.0)), ("1.234.567,89".into(), "0,00".into(), "999,50".into(), "-1.500,00".into()));
        assert_eq!(numeros_do_texto("Total 1.463.552,21 e 22,00; data 12/2026; 1.234 e 5,5"), HashSet::from([146355221, 2200]));
        assert_eq!((dito(10463552.21), dito(718313.42), dito(22196.93), dito(950.4)), ("10 milhões e 460".into(), "718 mil".into(), "22 mil e 200".into(), "950 reais".into()));
        assert_eq!((dito(13789180284.92), dito(1e9), dito(1000000.0), dito(45000.0)), ("13 bilhões e 790 milhões".into(), "1 bilhão".into(), "1 milhão".into(), "45 mil".into()));
        assert_eq!(valores("27 milhões e 240, 27.240.000, 66 mil e 400 e R$ 66.400,00; sobrou 30"), [27240000, 27240000, 66400, 66400, 30]);
        assert_eq!(valores("117 bilhões e 10 milhões; 117,01 bilhões; 2,7 milhões; 92 milhões, 240 mil"), [117010000000, 117010000000, 2700000, 92240000]);
        assert_eq!(valores("R$ 92,240,000 [00:12.5] 1,120 mil <!-- 999 --> 45,19 mil"), [92240000, 1120000, 45190]);
        assert_eq!(valores(&"9".repeat(60)), [-1]); // numero que nao da para ler nunca tem prova
        for v in [10463552.21, 8740510.35, 718313.42, 22196.93, 13789180284.92, 950.0] {
            assert_eq!(valores(&dito(v)).len(), 1, "{}", dito(v));
        }
    }

    #[test]
    fn tudo_com_prova_passa() {
        let (tmp, l, pasta) = cidade("ok", acoes(), ROTEIRO, LEGENDA);
        assert_eq!(conferir(&l, &pasta, &docs()).unwrap().0, Vec::<String>::new());
        let d = |_: &Path| docs();
        let conf = rodar(&tmp.join("dados"), &l, &Rodada { alvos: &[], avisar: false, so_novas: false, arrumar: false }, &agora(), &nada(&d)).unwrap();
        assert_eq!(conf["777"]["ok"], true);
        assert!(tmp.join("dados/magno/auditoria.json").exists() && tmp.join("dados/magno/relatorio_auditoria.md").exists());
        // so_novas: nada mudou, nem le os documentos de novo
        let nunca = |_: &Path| -> Docs { panic!("auditou de novo sem mudanca") };
        rodar(&tmp.join("dados"), &l, &Rodada { alvos: &[], avisar: false, so_novas: true, arrumar: true }, &agora(), &nada(&nunca)).unwrap();
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn valor_levantado_fora_do_documento() {
        let mut mexida = acoes();
        mexida[0]["liquidado"] = json!(8100000.0); // alguem (pessoa ou IA) mudou o numero depois de ler o documento
        let (tmp, l, pasta) = cidade("fora", mexida, ROTEIRO, LEGENDA);
        let prob = conferir(&l, &pasta, &docs()).unwrap().0;
        assert!(prob.iter().any(|p| p.contains("2026: 1 de 11 valores levantados não estão no documento do tribunal, ex.: ação 1.1 (liquidado) R$ 8.100.000,00")), "{prob:?}");
        assert!(prob.iter().any(|p| p.contains("setor Educação: ação que não está no levantamento: Ensino R$ 8.000.000,00")), "{prob:?}");
        assert!(!porta(&l, &pasta, &docs(), &|_| None).is_empty());
        // documento de OUTRO ano nao prova nada deste
        let mut de_outro_ano = Docs::new();
        de_outro_ano.insert("2025".into(), docs().remove("2026").unwrap());
        let (_, l2, pasta2) = cidade("fora", acoes(), ROTEIRO, LEGENDA);
        assert!(conferir(&l2, &pasta2, &de_outro_ano).unwrap().0.iter().any(|p| p.contains("11 de 11 valores levantados")));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn total_que_nao_fecha_com_a_soma_e_setor_sem_prova() {
        let (tmp, l, pasta) = cidade("total", acoes(), ROTEIRO, LEGENDA);
        let mut cid = json_de(&pasta.join("saida/cidade.json")).unwrap();
        cid["anos"]["2026"]["gastou"] = json!(8840510.35); // total falado diferente da soma do liquidado
        cid["festas"]["2026"]["gasto"] = json!(818313.42);
        grava(&pasta.join("saida/cidade.json"), &cid.to_string());
        let prob = conferir(&l, &pasta, &docs()).unwrap().0;
        assert!(prob.iter().any(|p| p.starts_with("2026 gastou: R$ 8.840.510,35 não está no documento")), "{prob:?}");
        assert!(prob.iter().any(|p| p.starts_with("2026 achado (gastou): R$ 818.313,42")), "{prob:?}");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn roteiro_e_legenda_com_numero_inventado() {
        let (tmp, l, pasta) = cidade("inventado", acoes(), "2026. Arrecadou 10 milhões e 460. Festas, 918 mil.", "Festas: R$ 918 mil. Educação: R$ 8 mi.");
        let prob = conferir(&l, &pasta, &docs()).unwrap().0;
        assert!(prob.contains(&"roteiro fala número sem prova: [918000]".to_string()), "{prob:?}");
        assert!(prob.contains(&"legenda com valor sem prova: ['R$ 918 mil']".to_string()), "{prob:?}");
        // a legenda arredonda so ate a ultima casa escrita: 10,46 mi vale, 10,56 mi nao
        let (_, l, pasta) = cidade("inventado", acoes(), ROTEIRO, "Arrecadou R$ 10,56 mi; festas R$ 718.313 e R$ 0");
        let prob = conferir(&l, &pasta, &docs()).unwrap().0;
        assert_eq!(prob, ["legenda com valor sem prova: ['R$ 10,56 mi', 'R$ 718']"]);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn folha_da_tela_com_linha_ou_total_sem_prova() {
        let (tmp, l, pasta) = cidade("folha", acoes(), ROTEIRO, LEGENDA);
        let folha = |linha: f64, total: f64| json!({ "folhas": [{ "titulo": "Festas", "total": total, "linhas": [["Festas", linha], ["Outras ações", 5.0]] }] }).to_string();
        let arq = tmp.join("pacote/videos/777/folhas.json");
        grava(&arq, &folha(718313.42, 718313.42));
        assert!(conferir(&l, &pasta, &docs()).unwrap().0.is_empty());
        grava(&arq, &folha(718313.43, 718313.42));
        assert!(conferir(&l, &pasta, &docs()).unwrap().0[0].starts_with("folha 'Festas': linha sem prova"));
        grava(&arq, &folha(718313.42, 918313.42));
        assert_eq!(conferir(&l, &pasta, &docs()).unwrap().0, ["folha 'Festas': total R$ 918.313,42 sem prova"]);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn porta_da_esteira() {
        let (tmp, l, pasta) = cidade("porta", acoes(), ROTEIRO, LEGENDA);
        assert!(porta(&l, &pasta, &docs(), &|_| panic!("nao e do tribunal direto")).is_empty());
        let (_, l, pasta) = cidade("porta", acoes(), "Festas, 918 mil.", LEGENDA);
        assert!(!porta(&l, &pasta, &docs(), &|_| None).is_empty()); // numero inventado: a esteira para antes da voz
        // levantamento pela metade nao passa
        let (_, l, pasta) = cidade("porta", acoes(), ROTEIRO, LEGENDA);
        std::fs::remove_file(pasta.join("saida/receita-2026.json")).unwrap();
        assert!(porta(&l, &pasta, &docs(), &|_| None)[0].starts_with("não deu para auditar"));
        // capital: sem PDF no PC, a prova e a releitura do tribunal dar o mesmo numero
        let (_, l, pasta) = cidade("porta", acoes(), ROTEIRO, LEGENDA);
        let mut cid = json_de(&pasta.join("saida/cidade.json")).unwrap();
        cid["fonte"] = json!("tribunal");
        grava(&pasta.join("saida/cidade.json"), &cid.to_string());
        let sem_pdf = Docs::new();
        assert_eq!(conferir(&l, &pasta, &sem_pdf).unwrap().1[0], "sem PDF do tribunal no PC: a origem não foi conferida aqui (capital e MT: conferir_capitais refaz pelo tribunal)");
        assert!(porta(&l, &pasta, &sem_pdf, &|n| { assert_eq!(n, "777"); Some(vec!["voz: sem arquivo".into()]) }).is_empty());
        assert_eq!(porta(&l, &pasta, &sem_pdf, &|_| Some(vec!["voz: x".into(), "Gastou: no vídeo 1, na fonte agora 2".into()])), ["Gastou: no vídeo 1, na fonte agora 2"]);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // --- triagem e conserto: o que esta errado e o video, o levantamento do PC, a legenda ou o tribunal que mudou? ---
    fn totais_base() -> BTreeMap<String, f64> {
        BTreeMap::from([("Gastou (2025)".to_string(), 13789180284.92), ("Gastou (2026 até 09)".to_string(), 11510222826.96), ("Já empenhou (2026 até 09)".to_string(), 11801530706.17)])
    }

    fn com(k: &str, v: f64) -> BTreeMap<String, f64> {
        let mut t = totais_base();
        t.insert(k.to_string(), v);
        t
    }

    fn lista(t: &BTreeMap<String, f64>) -> Value {
        json!(t.iter().map(|(k, v)| json!({ "dito": k, "valor": v })).collect::<Vec<_>>())
    }

    fn capital(nome: &str, video: &BTreeMap<String, f64>, pc: &BTreeMap<String, f64>) -> (PathBuf, Locais, PathBuf) {
        let (tmp, l, pasta) = cidade(nome, acoes(), ROTEIRO, LEGENDA);
        let mut cid = json_de(&pasta.join("saida/cidade.json")).unwrap();
        cid["uf"] = json!("CE");
        cid["meses"] = json!({ "2025": "12", "2026": "09" });
        cid["totais"] = lista(pc);
        grava(&pasta.join("saida/cidade.json"), &cid.to_string());
        let linhas: String = video.iter().map(|(k, v)| format!("| {k} | R$ {} | SIM | soma |\n", br(*v))).collect();
        grava(&tmp.join("pacote/estudio-3d/projetos/777. x/audio/fontes.md"), &format!("| Valor dito | Valor exato | Arquivo | Onde |\n|---|---|---|---|\n{linhas}"));
        (tmp, l, pasta)
    }

    /// O "tribunal lido agora": grava a leitura na pasta nova, como o molde faz.
    fn tribunal(valores: BTreeMap<String, f64>) -> impl Fn(&Value, &Path) -> Result<BTreeMap<String, f64>, String> {
        move |cid, destino| {
            let mut novo = cid.clone();
            novo["totais"] = lista(&valores);
            grava(&destino.join("saida/cidade.json"), &novo.to_string());
            Ok(valores.clone())
        }
    }

    const ROT: &str = "roteiro fala número sem prova: [1]";

    #[test]
    fn triagem_so_legenda() {
        let (tmp, l, pasta) = capital("so-legenda", &totais_base(), &totais_base());
        let r = triagem(&l, &pasta, &["legenda com valor sem prova: ['R$ 1,36 bi']".to_string()], &|_, _| panic!("nao rele"), &tmp).unwrap();
        assert_eq!(r.0, "legenda");
        let gerou = RefCell::new(0);
        let d = |_: &Path| docs();
        let f = Fora { gera_legenda: &|_| { *gerou.borrow_mut() += 1; Ok(()) }, ..nada(&d) };
        let (cat, texto) = consertar(&l, &pasta, &["legenda com valor sem prova: ['R$ 1,36 bi']".to_string()], &f, true, &tmp).unwrap();
        assert!(cat == "legenda" && *gerou.borrow() == 1 && texto.contains("legenda refeita por código"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn levantamento_do_pc_errado_e_trocado() {
        // o video estava certo; o download refeito veio com linha repetida. Conserta o PC, nao refaz o video.
        let (tmp, l, pasta) = capital("levantamento", &totais_base(), &com("Gastou (2025)", 14364575195.39));
        let d = |_: &Path| docs();
        let relido = tribunal(totais_base());
        let f = Fora { reler: &relido, ..nada(&d) }; // refazer entra em panico se for chamado
        let (cat, texto) = consertar(&l, &pasta, &[ROT.to_string()], &f, true, &tmp.join("tmp")).unwrap();
        assert!(cat == "levantamento" && texto.contains("Gastou (2025): vídeo R$ 13.789.180.284,92, PC R$ 14.364.575.195,39"), "{texto}");
        assert_eq!(totais(&json_de(&pasta.join("saida/cidade.json")).unwrap()), totais_base());
        assert_eq!(std::fs::read_dir(tmp.join("tmp")).unwrap().count(), 0); // a leitura temporaria some
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn tribunal_mudou_no_ano_aberto_nao_refaz() {
        let novo = com("Já empenhou (2026 até 09)", 11785285087.29);
        let (tmp, l, pasta) = capital("mudou", &totais_base(), &novo);
        assert_eq!(triagem(&l, &pasta, &[ROT.to_string()], &tribunal(novo.clone()), &tmp.join("tmp")).unwrap().0, "tribunal_mudou");
        let d = |_: &Path| docs();
        let relido = tribunal(novo);
        let (cat, texto) = consertar(&l, &pasta, &[ROT.to_string()], &Fora { reler: &relido, ..nada(&d) }, true, &tmp.join("tmp")).unwrap();
        assert!(cat == "tribunal_mudou" && texto.contains("nada automático"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn video_que_nao_bate_volta_para_a_esteira_uma_vez() {
        let novo = com("Gastou (2025)", 13000000000.0); // ano fechado diferente no tribunal (PC e leitura nova iguais)
        let (tmp, l, pasta) = capital("refazer", &totais_base(), &novo);
        let chamadas = RefCell::new(vec![]);
        let d = |_: &Path| docs();
        let relido = tribunal(novo);
        let f = Fora { reler: &relido, refazer: &|n, _| { chamadas.borrow_mut().push(n.to_string()); Ok(()) }, ..nada(&d) };
        assert_eq!(consertar(&l, &pasta, &[ROT.to_string()], &f, true, &tmp.join("tmp")).unwrap().0, "refazer");
        assert_eq!(*chamadas.borrow(), ["777"]);
        chamadas.borrow_mut().clear(); // ja voltou para a esteira ha pouco e continua errado: nao entra em laco
        let (cat, texto) = consertar(&l, &pasta, &[ROT.to_string()], &f, false, &tmp.join("tmp")).unwrap();
        assert!(cat == "refazer" && chamadas.borrow().is_empty() && texto.contains("agora é com uma pessoa"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn tres_leituras_diferentes_ou_tribunal_fora_vai_para_pessoa() {
        let (tmp, l, pasta) = capital("pessoa", &totais_base(), &com("Gastou (2025)", 1.0));
        assert_eq!(triagem(&l, &pasta, &[ROT.to_string()], &tribunal(com("Gastou (2025)", 2.0)), &tmp.join("tmp")).unwrap().0, "pessoa");
        let r = triagem(&l, &pasta, &[ROT.to_string()], &|_, _| Err("tribunal fora do ar".into()), &tmp.join("tmp")).unwrap();
        assert!(r.0 == "pessoa" && r.1.contains("não consegui reler"));
        // sem uf (a prova e o PDF) ou sem fontes.md guardado: refaz do zero, sem reler
        let (_, l, pasta) = cidade("pessoa", acoes(), ROTEIRO, LEGENDA);
        assert_eq!(triagem(&l, &pasta, &[ROT.to_string()], &|_, _| panic!("nao rele"), &tmp.join("tmp")).unwrap().0, "refazer");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn rodada_que_arruma_registra_avisa_e_nao_refaz_em_laco() {
        let (tmp, l, _) = capital("rodada", &totais_base(), &com("Gastou (2025)", 13000000000.0));
        grava(&tmp.join("pacote/estudio-3d/projetos/777. x/audio/roteiro-falado.txt"), "Festas, 918 mil.");
        let cj = tmp.join("pacote/RECEITAS-DESPESAS-X/saida/cidade.json");
        let mut cid = json_de(&cj).unwrap();
        cid["meses"] = json!({ "2026": "09" }); // o levantamento de teste so tem 2026
        grava(&cj, &cid.to_string());
        let dados = tmp.join("dados");
        let (refeitos, avisos) = (RefCell::new(0), RefCell::new(vec![]));
        let d = |_: &Path| docs();
        let relido = tribunal(com("Gastou (2025)", 13000000000.0));
        let f = Fora { docs: &d, reler: &relido, gera_legenda: &|_| panic!("legenda"), refazer: &|_, _| { *refeitos.borrow_mut() += 1; Ok(()) }, capital: &|_| None,
                       avisar: &|t| avisos.borrow_mut().push(t.to_string()) };
        let tudo = Rodada { alvos: &[], avisar: true, so_novas: false, arrumar: true };
        let conf = rodar(&dados, &l, &tudo, &agora(), &f).unwrap();
        assert!(conf["777"]["ok"] == false && conf["777"]["categoria"] == "refazer" && conf["777"]["agora_ok"] == false);
        assert_eq!((*refeitos.borrow(), avisos.borrow().len()), (1, 1));
        assert!(avisos.borrow()[0].contains("777 X: ") && avisos.borrow()[0].contains("sem IA"));
        let h = historico(&dados);
        assert!(h.len() == 1 && h[0]["categoria"] == "refazer" && h[0]["quando"] == "18/11 10:30");
        // mesma coisa de novo: ja triado, nao rele, nao refaz, nao avisa
        rodar(&dados, &l, &tudo, &agora(), &f).unwrap();
        assert_eq!((*refeitos.borrow(), avisos.borrow().len(), historico(&dados).len()), (1, 1, 1));
        // o problema mudou dentro dos 7 dias: triagem de novo, mas sem mandar refazer outra vez
        grava(&tmp.join("pacote/estudio-3d/projetos/777. x/audio/roteiro-falado.txt"), "Festas, 919 mil.");
        let conf = rodar(&dados, &l, &tudo, &Agora { data: Data(agora().data.0 + 2), ..agora() }, &f).unwrap();
        assert!(*refeitos.borrow() == 1 && conf["777"]["feito"].as_str().unwrap().contains("agora é com uma pessoa"));
        let t = std::fs::read_to_string(dados.join("magno/relatorio_auditoria.md")).unwrap();
        assert!(t.contains("## Vídeo não bate com o tribunal: refazer") && t.contains("### 777 X") && t.contains("- 18/11 10:30 · 777 X: "), "{t}");
        // tick: uma rodada a cada 3 h
        let _ = std::fs::remove_file(dados.join("magno/auditoria.ultima"));
        assert!(tick(&dados, &l, &agora(), &f).is_some() && tick(&dados, &l, &Agora { hora: 12, ..agora() }, &f).is_none());
        assert!(tick(&dados, &l, &Agora { hora: 13, min: 31, ..agora() }, &f).is_some());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn relatorio_separa_por_categoria() {
        let (tmp, _, _) = cidade("relatorio", acoes(), ROTEIRO, LEGENDA);
        let conf = json!({ "777": { "cidade": "X", "ok": false, "problemas": ["legenda com valor sem prova"], "categoria": "pessoa", "feito": "olhar" },
                           "778": { "cidade": "Y", "ok": true, "problemas": [] }, "779": { "cidade": "Z", "ok": false, "problemas": ["p"] },
                           "780": { "cidade": "W", "ok": false, "agora_ok": true, "problemas": ["p"], "categoria": "legenda" } });
        let arq = relatorio(&tmp.join("dados"), conf.as_object().unwrap(), &agora(), &["QUASE".to_string()]).unwrap();
        let t = std::fs::read_to_string(arq).unwrap();
        for trecho in ["Precisa de uma pessoa olhar", "### 777 X", "778 Y, 780 W", "Com problema agora: 2", "## Problema ainda sem triagem", "- QUASE", "Nenhuma IA participa"] {
            assert!(t.contains(trecho), "faltou {trecho:?} em:\n{t}");
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
