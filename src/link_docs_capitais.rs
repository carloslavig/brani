//! Documentos do tribunal de contas ao lado dos numeros das capitais (link/docs_capitais.py). Para cada capital monta a pasta
//! <site>/docs/NNN-<slug>-docs com: os arquivos do tribunal usados, em tabela de texto (linha 1 = cabecalho, uma linha por
//! registro), para o fontes.md citar "linha N, coluna X"; um PDF-extrato com, para cada numero do video, as linhas do arquivo do
//! tribunal que o formam; o fontes.md com arquivo + pagina/linha/coluna em cada numero; e o meta.json tipo documentos.
//! Regras: sem nome de eleito e sem CPF em nada que vai ao site (`sem_pessoa`, `limpar`), arquivo de ate ~22 MB (o que passa e
//! dividido em partes com o cabecalho repetido, sem perder linha); codigo de acao que nao esta no arquivo do tribunal fica
//! escrito "não achado" (e contado no relatorio), nunca e trocado por linha parecida.
//! Daqui nada sai para fora: desenhar o PDF e closure (`desenha`; as paginas de cada secao sao calculadas aqui, em `paginas`),
//! e o envio ao site (`enviar`/`situacao` do Python, que rodam o script de envio) ficou de fora: e de quem chama.
//! Diferencas do Python: as colunas de `tabela_json` saem em ordem alfabetica (o JSON lido aqui nao guarda a ordem das chaves);
//! entidade de HTML so as numericas e as basicas (`link_tribunais_util::desescapa`).
#![allow(dead_code)]
use crate::link_auditar::br;
use crate::link_tribunais_util::{desescapa, pystr};
use crate::texto_pt::sem_acento;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// O site recusa acima de 24 MB; folga para o cabecalho repetido nas partes.
pub const TETO_ARQ: usize = 22_000_000;
const BAK: &str = ".bak-20261116-docs";
const NAO_ACHADO: &str = "código não achado no arquivo (ver extrato)";

/// Um arquivo da entrega.
#[derive(Clone, Debug, PartialEq)]
pub struct Arq {
    pub ano: String,
    /// despesa | receita | pdf | outro
    pub tipo: String,
    pub path: PathBuf,
    /// De onde veio (texto + endereco do tribunal).
    pub origem: String,
    /// false = grande demais para o site (fica em _grandes; o extrato cita as linhas dele).
    pub enviado: bool,
}

impl Arq {
    pub fn novo(ano: &str, tipo: &str, path: PathBuf, origem: String) -> Arq {
        Arq { ano: ano.into(), tipo: tipo.into(), path, origem, enviado: true }
    }
    fn nome(&self) -> String {
        self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }
}

/// O adaptador de uma capital: os arquivos de cada ano e as colunas; o resto e generico.
pub struct Adaptador<'a> {
    pub pasta: PathBuf,
    pub tribunal: String,
    pub arquivos: Vec<Arq>,
    /// "liquidado", "previa", "empenhado", "arrecadado" -> nome da coluna no .txt.
    pub col: BTreeMap<&'static str, String>,
    /// (ano, tipo) -> quais linhas entram na soma.
    pub filtro: BTreeMap<(String, String), String>,
    /// codigo -> campos a procurar na linha (vazio = os pedacos do codigo).
    pub partes: &'a dyn Fn(&str) -> Vec<String>,
    /// Como achar codigos de acao num texto (None = `codigos`).
    pub codigos: Option<&'a dyn Fn(&str) -> Vec<String>>,
    /// Quantas faixas de linhas citar antes do "(+N trechos)".
    pub faixa_mx: usize,
}

pub fn slug(s: &str) -> String {
    let t: String = sem_acento(s).chars().filter(char::is_ascii).map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    t.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-")
}

/// Uma celula da tabela de texto: sem tab nem quebra de linha.
pub fn cel(v: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[\t\r\n]+").expect("regex fixa")).replace_all(v, " ").trim().to_string()
}

pub fn cel_json(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Bool(b) => if *b { "True" } else { "False" }.into(),
        v => cel(&pystr(v)),
    }
}

// ---------- conversao para tabela de texto ----------

/// Tabela HTML (bytes em latin-1, como as planilhas ".xls" dos tribunais) -> linhas de celulas, com o cabecalho.
pub fn tabela_html(b: &[u8]) -> Vec<Vec<String>> {
    static RE: OnceLock<(Regex, Regex, Regex)> = OnceLock::new();
    let (tr, td, tag) = RE.get_or_init(|| (Regex::new(r"(?is)<tr[^>]*>(.*?)</tr>").expect("regex fixa"), Regex::new(r"(?is)<t[dh][^>]*>(.*?)</t[dh]>").expect("regex fixa"), Regex::new(r"<[^>]+>").expect("regex fixa")));
    let t: String = b.iter().map(|x| *x as char).collect();
    tr.captures_iter(&t).map(|r| td.captures_iter(&r[1]).map(|c| cel(&desescapa(&tag.replace_all(&c[1], "")))).collect()).collect()
}

/// Lista de registros (ou a maior lista de registros dentro do JSON) -> [cabecalho] + linhas.
pub fn tabela_json(obj: &Value) -> Vec<Vec<String>> {
    fn maior(o: &Value) -> Option<&Vec<Value>> {
        let mut best = o.as_array().filter(|l| !l.is_empty() && l.iter().all(Value::is_object));
        let filhos: Vec<&Value> = match o {
            Value::Object(m) => m.values().collect(),
            Value::Array(l) => l.iter().collect(),
            _ => vec![],
        };
        for f in filhos {
            if let Some(m) = maior(f).filter(|m| best.is_none_or(|b| m.len() > b.len())) {
                best = Some(m);
            }
        }
        best
    }
    let vazio = vec![];
    let l = maior(obj).unwrap_or(&vazio);
    let mut cab: Vec<&String> = vec![];
    for k in l.iter().filter_map(Value::as_object).flat_map(|x| x.keys()) {
        if !cab.contains(&k) {
            cab.push(k);
        }
    }
    std::iter::once(cab.iter().map(|k| k.to_string()).collect()).chain(l.iter().map(|x| cab.iter().map(|k| cel_json(&x[k.as_str()])).collect())).collect()
}

/// Grava a tabela em TSV (linha 1 = cabecalho). Devolve o caminho.
pub fn gravar(pasta: &Path, nome: &str, linhas: &[Vec<String>]) -> Result<PathBuf, String> {
    let p = pasta.join(nome);
    let t: String = linhas.iter().map(|r| r.iter().map(|c| cel(c)).collect::<Vec<_>>().join("\t") + "\n").collect();
    std::fs::write(&p, t).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok(p)
}

/// Como `gravar`, mas com aspas de CSV quando o campo tem aspas (sem isso quem le junta linhas).
pub fn gravar_com_aspas(pasta: &Path, nome: &str, linhas: &[Vec<String>]) -> Result<PathBuf, String> {
    let campo = |c: &String| {
        let c = cel(c);
        if c.contains('"') { format!("\"{}\"", c.replace('"', "\"\"")) } else { c }
    };
    let p = pasta.join(nome);
    let t: String = linhas.iter().map(|r| r.iter().map(campo).collect::<Vec<_>>().join("\t") + "\n").collect();
    std::fs::write(&p, t).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok(p)
}

pub fn copiar(src: &Path, pasta: &Path, nome: &str) -> Result<PathBuf, String> {
    let p = pasta.join(nome);
    std::fs::copy(src, &p).map_err(|e| format!("{}: {e}", src.display()))?;
    Ok(p)
}

// ---------- sem nome de eleito, sem CPF ----------

fn de_palavra(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn letra(c: char) -> bool {
    c.is_ascii_alphabetic() || ('À'..='ÿ').contains(&c) || c == 'Ÿ'
}

/// Se o texto em `i` comeca com `p` (sem olhar maiuscula/minuscula), a posicao depois dele.
fn come(c: &[char], i: usize, p: &str) -> Option<usize> {
    let n = p.chars().count();
    (c.len() >= i + n && c[i..i + n].iter().zip(p.chars()).all(|(a, b)| a.to_ascii_uppercase() == b)).then_some(i + n)
}

/// Uma palavra de nome a partir de `p`: letra e depois letras, apostrofo ou ponto.
fn pal(c: &[char], p: usize) -> Option<usize> {
    c.get(p).filter(|x| letra(**x)).map(|_| p + 1 + c[p + 1..].iter().take_while(|x| letra(**x) || **x == '\'' || **x == '.').count())
}

/// O nome que vem depois do cargo, a partir de `p` (ate 6 palavras, com "de/da/dos/e" no meio; nao engole o "R$" do valor).
fn nome_proprio(c: &[char], p: usize) -> Option<usize> {
    const NAO_NOME: [&str; 28] = ["E", "OU", "DE", "DA", "DAS", "DO", "DOS", "EM", "NA", "NAS", "NO", "NOS", "PARA", "COM", "POR", "MUNICIPAL", "MUNICIPAIS", "VICE", "ELEITO", "ELEITA", "ESTADUAL", "FEDERAL", "IMPOSITIVA", "IMPOSITIVAS", "IMPOSITIVO", "IMPOSITIVOS", "EMENDA", "EMENDAS"];
    if NAO_NOME.iter().any(|w| come(c, p, w).is_some_and(|e| !c.get(e).is_some_and(|x| de_palavra(*x)))) {
        return None;
    }
    let mut f = pal(c, p)?;
    for _ in 0..5 {
        let q = f + 1;
        if c.get(f) != Some(&' ') || (come(c, q, "R").is_some() && c.get(q + 1) == Some(&'$')) {
            break;
        }
        let d = come(c, q, "D").filter(|x| c.get(*x).is_some_and(|v| "AEO".contains(v.to_ascii_uppercase()))).map(|x| x + 1);
        let conectores = [d.and_then(|x| come(c, x, "S ")), d.and_then(|x| come(c, x, " ")), come(c, q, "E ")];
        match conectores.iter().flatten().find_map(|x| pal(c, *x)).or_else(|| pal(c, q)) {
            Some(x) => f = x,
            None => break,
        }
    }
    Some(f)
}

/// Cargo de eleito em `i` seguido de nome: (onde o nome comeca, onde acaba). A mesma ordem de tentativas da regex do Python.
fn nome_de_eleito(c: &[char], i: usize) -> Option<(usize, usize)> {
    let mut fins = vec![];
    let com = |base: &str, sufixos: &[&str], sozinho: bool, fins: &mut Vec<usize>| {
        if let Some(e) = come(c, i, base) {
            fins.extend(sufixos.iter().filter_map(|s| come(c, e, s)));
            if sozinho {
                fins.push(e);
            }
        }
    };
    com("VEREADOR", &["A", "ES"], true, &mut fins);
    com("PREFEIT", &["O", "A"], false, &mut fins);
    for d in ["DEPUTADO", "DEPUTADA"] {
        com(d, &[" FEDERAL", " ESTADUAL", " DISTRITAL"], true, &mut fins);
    }
    com("SENADOR", &["A"], true, &mut fins);
    com("DEP.", &[], true, &mut fins);
    com("VER.", &[], true, &mut fins);
    let ch = |k: usize| c.get(k).copied();
    let simb = |k: usize| matches!(ch(k), Some(':') | Some('-'));
    for e in fins {
        let mut seps = vec![];
        if ch(e) == Some(' ') && simb(e + 1) {
            seps.extend((ch(e + 2) == Some(' ')).then_some(e + 3));
            seps.push(e + 2);
        }
        if simb(e) {
            seps.extend((ch(e + 1) == Some(' ')).then_some(e + 2));
            seps.push(e + 1);
        }
        seps.extend((ch(e) == Some(' ')).then_some(e + 1));
        if let Some(x) = seps.into_iter().find_map(|p| nome_proprio(c, p).map(|f| (p, f))) {
            return Some(x);
        }
    }
    None
}

/// Troca o nome depois de vereador/prefeito/deputado/senador por [nome omitido] e CPF por [omitido]. Nao muda o numero de
/// linhas: o nome nunca atravessa a coluna (tab, ; ou |) nem a linha.
pub fn sem_pessoa(t: &str) -> String {
    static CPF: OnceLock<Regex> = OnceLock::new();
    let c: Vec<char> = t.chars().collect();
    let (mut sem_nome, mut i) = (String::with_capacity(t.len()), 0);
    while i < c.len() {
        match (i == 0 || !de_palavra(c[i - 1])).then(|| nome_de_eleito(&c, i)).flatten() {
            Some((ini, fim)) => {
                sem_nome.extend(&c[i..ini]);
                sem_nome.push_str("[nome omitido]");
                i = fim;
            }
            None => {
                sem_nome.push(c[i]);
                i += 1;
            }
        }
    }
    // CPF com pontos e traco (pode vir mascarado com *); CNPJ nao casa. Nao pode estar colado em outro numero.
    let cpf = CPF.get_or_init(|| Regex::new(r"[\d*]{3}\.[\d*]{3}\.[\d*]{3}-[\d*]{2}").expect("regex fixa"));
    let (mut out, mut pos, mut de) = (String::with_capacity(sem_nome.len()), 0, 0);
    while let Some(m) = cpf.find_at(&sem_nome, de) {
        let antes = sem_nome[..m.start()].chars().next_back();
        let depois = sem_nome[m.end()..].chars().next();
        if antes.is_some_and(|x| x.is_numeric() || x == '.') || depois.is_some_and(char::is_numeric) {
            de = m.start() + sem_nome[m.start()..].chars().next().map_or(1, char::len_utf8);
            continue;
        }
        out.push_str(&sem_nome[pos..m.start()]);
        out.push_str("[omitido]");
        (pos, de) = (m.end(), m.end());
    }
    out + &sem_nome[pos..]
}

fn separador(linha: &str) -> char {
    ['\t', ';', ','].into_iter().fold('\t', |m, s| if linha.matches(s).count() > linha.matches(m).count() { s } else { m })
}

/// CPF so com digitos (11) nas colunas cujo cabecalho fala em CPF. Linha com numero de campos diferente do cabecalho fica como esta.
pub fn sem_cpf_coluna(linhas: Vec<String>) -> Vec<String> {
    let Some(cab) = linhas.first() else { return linhas };
    let sep = separador(cab);
    let (n, alvo): (usize, Vec<usize>) = (cab.split(sep).count(), cab.split(sep).enumerate().filter(|(_, c)| c.to_lowercase().contains("cpf")).map(|x| x.0).collect());
    if alvo.is_empty() {
        return linhas;
    }
    let onze = |c: &str| {
        let d = c.trim();
        let d = d.strip_prefix('"').unwrap_or(d);
        let d = d.strip_suffix('"').unwrap_or(d);
        d.len() == 11 && d.bytes().all(|b| b.is_ascii_digit())
    };
    linhas
        .into_iter()
        .enumerate()
        .map(|(i, l)| {
            let cs: Vec<&str> = l.split(sep).collect();
            if i == 0 || cs.len() != n || !alvo.iter().any(|a| onze(cs[*a])) {
                return l;
            }
            cs.iter().enumerate().map(|(k, c)| if alvo.contains(&k) && onze(c) { "[omitido]" } else { c }).collect::<Vec<_>>().join(&sep.to_string())
        })
        .collect()
}

/// (texto, se e UTF-8) de um arquivo do tribunal: UTF-8 ou, se nao for, latin-1.
fn decodifica(raw: Vec<u8>) -> (String, bool) {
    match String::from_utf8(raw) {
        Ok(t) => (t, true),
        Err(e) => (e.into_bytes().iter().map(|b| *b as char).collect(), false),
    }
}

fn codifica(t: &str, utf8: bool) -> Vec<u8> {
    if utf8 { t.as_bytes().to_vec() } else { t.chars().map(|c| if (c as u32) < 256 { c as u8 } else { b'?' }).collect() }
}

/// Passa cada .txt da entrega pelo `sem_pessoa`; o que mudou vira NOME-SN.txt e o que passa do teto e dividido em -PARTE1,
/// -PARTE2... (cabecalho repetido em cada parte). Devolve a lista nova de arquivos.
pub fn limpar(arqs: Vec<Arq>, teto: usize) -> Result<Vec<Arq>, String> {
    let grava = |p: &Path, b: Vec<u8>| std::fs::write(p, b).map_err(|e| format!("{}: {e}", p.display()));
    let mut novos = vec![];
    for a in arqs {
        let p = a.path.clone();
        let radical = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        if p.extension().is_none_or(|e| e != "txt") || !p.exists() || radical.contains("-SN") {
            novos.push(a);
            continue;
        }
        let raw = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        let tamanho = raw.len();
        let (t, utf8) = decodifica(raw);
        let linhas = sem_cpf_coluna(sem_pessoa(&t).split('\n').map(String::from).collect());
        let t2 = linhas.join("\n");
        let grande = a.enviado && tamanho > teto;
        if t2 == t && !grande {
            novos.push(a);
            continue;
        }
        if !a.enviado {
            // fica em _grandes (nao vai ao site), mas o extrato cita linhas dele
            grava(&p, codifica(&t2, utf8))?;
            novos.push(a);
            continue;
        }
        std::fs::remove_file(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        if !grande {
            let q = p.with_file_name(format!("{radical}-SN.txt"));
            grava(&q, codifica(&t2, utf8))?;
            novos.push(Arq { path: q, ..a });
            continue;
        }
        let (cab, mut parte, mut tam, mut k) = (&linhas[0], vec![], 0, 1);
        let mut fecha = |parte: &mut Vec<&str>, k: &mut usize| -> Result<(), String> {
            let q = p.with_file_name(format!("{radical}-SN-PARTE{k}.txt"));
            let corpo = std::iter::once(cab.as_str()).chain(parte.iter().copied()).chain([""]).collect::<Vec<_>>().join("\n");
            grava(&q, codifica(&corpo, utf8))?;
            novos.push(Arq { path: q, origem: format!("{} (parte {k})", a.origem), ..a.clone() });
            parte.clear();
            *k += 1;
            Ok(())
        };
        for l in &linhas[1..] {
            if l.is_empty() && tam == 0 {
                continue;
            }
            let n = codifica(l, utf8).len() + 1;
            if tam + n > teto && !parte.is_empty() {
                fecha(&mut parte, &mut k)?;
                tam = 0;
            }
            parte.push(l.as_str());
            tam += n;
        }
        if parte.iter().any(|l| !l.is_empty()) {
            fecha(&mut parte, &mut k)?;
        }
    }
    Ok(novos)
}

// ---------- achar as linhas de um codigo de acao ----------

fn norm(x: &str) -> String {
    let x = x.trim();
    if !x.is_empty() && x.chars().all(|c| c.is_ascii_digit()) {
        let t = x.trim_start_matches('0');
        if t.is_empty() { "0".into() } else { t.into() }
    } else {
        x.to_lowercase()
    }
}

/// O leitor de CSV do Python: aspas so abrem no comeco do campo, "" dentro de aspas e uma aspa, quebra de linha dentro de
/// aspas fica no campo; linha vazia e um registro vazio (o numero do registro e o que o fontes.md cita).
pub fn csv_py(t: &str, sep: char) -> Vec<Vec<String>> {
    let (mut regs, mut reg, mut campo): (Vec<Vec<String>>, Vec<String>, String) = (vec![], vec![], String::new());
    let (mut aspas, mut comeco, mut algo) = (false, true, false); // dentro de aspas; comeco do campo; a linha tem algo
    let mut it = t.chars().peekable();
    while let Some(c) = it.next() {
        if aspas {
            match c {
                '"' if it.peek() == Some(&'"') => {
                    campo.push('"');
                    it.next();
                }
                '"' => aspas = false,
                c => campo.push(c),
            }
            continue;
        }
        match c {
            '"' if comeco => (aspas, comeco, algo) = (true, false, true),
            c if c == sep => {
                reg.push(std::mem::take(&mut campo));
                (comeco, algo) = (true, true);
            }
            '\r' if it.peek() == Some(&'\n') => {}
            '\n' | '\r' => {
                if algo {
                    reg.push(std::mem::take(&mut campo));
                }
                regs.push(std::mem::take(&mut reg));
                (comeco, algo) = (true, false);
            }
            c => {
                campo.push(c);
                (comeco, algo) = (false, true);
            }
        }
    }
    if algo {
        reg.push(campo);
        regs.push(reg);
    }
    regs
}

/// Os registros de um .txt TSV/CSV; o registro `i` e a "linha i + 1" (1 = cabecalho).
pub fn ler_linhas(txt: &Path, sep: Option<char>) -> Result<Vec<Vec<String>>, String> {
    let (t, _) = decodifica(std::fs::read(txt).map_err(|e| format!("{}: {e}", txt.display()))?);
    let sep = sep.unwrap_or_else(|| separador(t.split('\n').next().unwrap_or("")));
    Ok(csv_py(&t, sep))
}

/// Para achar acoes: campo normalizado -> linhas que o tem; e os campos normalizados de cada linha.
pub struct Indice {
    campos: Vec<HashSet<String>>,
    por: HashMap<String, Vec<usize>>,
}

pub fn indice(txt: &Path, sep: Option<char>) -> Result<Indice, String> {
    let mut idx = Indice { campos: vec![], por: HashMap::new() };
    for (i, r) in ler_linhas(txt, sep)?.into_iter().enumerate() {
        let s: HashSet<String> = r.iter().filter(|c| !c.is_empty()).map(|c| norm(c)).collect();
        s.iter().for_each(|c| idx.por.entry(c.clone()).or_default().push(i));
        idx.campos.push(s);
    }
    Ok(idx)
}

/// Linhas cujo conjunto de campos contem TODAS as partes do codigo (ex.: 26.23.695.7000.4035). `partes` vazio = os pedacos do codigo.
pub fn achar(idx: &Indice, cod: &str, partes: &[String]) -> Vec<usize> {
    let ps: Vec<String> = if partes.is_empty() { cod.split(['.', '-', '/', ' ']).filter(|p| !p.is_empty()).map(norm).collect() } else { partes.iter().filter(|p| !p.is_empty()).map(|p| norm(p)).collect() };
    let Some(ultima) = ps.last() else { return vec![] };
    idx.por.get(ultima).into_iter().flatten().filter(|i| ps.iter().all(|p| idx.campos[**i].contains(p))).map(|i| i + 1).collect()
}

/// [3,4,5,9] -> "3-5, 9" (no maximo `mx` pedacos, depois "(+N trechos)").
pub fn faixa(ns: &[usize], mx: usize) -> String {
    let ns: Vec<usize> = ns.iter().copied().collect::<BTreeSet<_>>().into_iter().collect();
    let Some(&primeiro) = ns.first() else { return "não achei".into() };
    let (mut g, mut ini, mut ant) = (vec![], primeiro, primeiro);
    for n in ns[1..].iter().map(|n| Some(*n)).chain([None]) {
        if n == Some(ant + 1) {
            ant += 1;
            continue;
        }
        g.push(if ini == ant { ini.to_string() } else { format!("{ini}-{ant}") });
        if let Some(n) = n {
            (ini, ant) = (n, n);
        }
    }
    format!("{}{}", g.iter().take(mx).cloned().collect::<Vec<_>>().join(", "), if g.len() > mx { format!(" (+{} trechos)", g.len() - mx) } else { String::new() })
}

// ---------- PDF ----------

/// O texto como cabe no PDF (latin-1; o que nao cabe vira "?").
pub fn lat(s: &str) -> String {
    let s = s.replace(['·', '−', '–', '—'], "-").replace('→', "->").replace('…', "...");
    s.chars().map(|c| if (c as u32) < 256 { c } else { '?' }).collect()
}

/// As secoes prontas para o PDF: titulo e linhas ja sem nome de eleito/CPF e em latin-1.
pub fn secoes_limpas(secoes: &[(String, Vec<String>)]) -> Vec<(String, Vec<String>)> {
    secoes.iter().map(|(t, ls)| (lat(t), ls.iter().map(|l| lat(&sem_pessoa(l))).collect())).collect()
}

/// Pagina em que cada secao comeca no PDF-extrato (Courier 7 em A4 deitado, `larg` caracteres por linha): a mesma conta de
/// quem desenha. Na ordem das secoes.
pub fn paginas(secoes: &[(String, Vec<String>)], larg: usize) -> Vec<(String, usize)> {
    let h = 210.0 * (72.0 / 25.4);
    let (mut pag, mut y, mut out) = (1, h - 40.0, vec![]);
    for (st, linhas) in secoes {
        if y < 80.0 {
            (pag, y) = (pag + 1, h - 40.0);
        }
        out.push((st.clone(), pag));
        y -= 12.0;
        for ln in linhas {
            for _ in 0..lat(&sem_pessoa(ln)).chars().count().max(1).div_ceil(larg) {
                if y < 30.0 {
                    (pag, y) = (pag + 1, h - 40.0);
                }
                y -= 8.4;
            }
        }
        y -= 8.0;
    }
    out
}

// ---------- fontes.md ----------

/// A pasta do video no site (NNN-...).
pub fn pasta_video(site: &Path, numero: &str) -> Result<PathBuf, String> {
    let mut ds: Vec<PathBuf> = std::fs::read_dir(site).map_err(|e| format!("{}: {e}", site.display()))?.flatten().map(|e| e.path()).filter(|p| p.is_dir() && p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(&format!("{numero}-")))).collect();
    ds.sort();
    ds.into_iter().next().ok_or_else(|| format!("vídeo {numero} sem pasta no site"))
}

fn fontes_em(d: &Path) -> Result<PathBuf, String> {
    let mut fs: Vec<PathBuf> = std::fs::read_dir(d).map_err(|e| format!("{}: {e}", d.display()))?.flatten().map(|e| e.path()).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().ends_with("fontes.md"))).collect();
    fs.sort();
    fs.into_iter().next().ok_or_else(|| format!("{} sem o fontes.md", d.display()))
}

fn copia_original(f: &Path) -> PathBuf {
    PathBuf::from(format!("{}{BAK}", f.display()))
}

/// O fontes.md do video e o texto ORIGINAL dele (rodar de novo parte sempre do original guardado).
pub fn ler_fontes(site: &Path, numero: &str) -> Result<(PathBuf, String), String> {
    let f = fontes_em(&pasta_video(site, numero)?)?;
    let bak = copia_original(&f);
    let t = std::fs::read_to_string(if bak.exists() { &bak } else { &f }).map_err(|e| format!("{}: {e}", f.display()))?;
    Ok((f, t))
}

/// (indice da linha, celulas) da tabela principal (Valor dito | Valor exato | Arquivo | Pagina...).
pub fn linhas_principais(md: &str) -> Vec<(usize, Vec<String>)> {
    let (mut out, mut dentro) = (vec![], false);
    for (i, l) in md.split('\n').enumerate() {
        if l.starts_with("| Valor dito | Valor exato | Arquivo") {
            dentro = true;
        } else if dentro {
            if !l.starts_with('|') {
                break;
            }
            if !l.starts_with("|---") {
                out.push((i, l.trim().trim_matches('|').split('|').map(|c| c.trim().to_string()).collect()));
            }
        }
    }
    out
}

pub fn ano_da(celula: &str) -> Option<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\((\d{4})").expect("regex fixa")).captures(celula).map(|m| m[1].to_string())
}

/// Codigos de acao (so numeros com ponto, 3 pedacos ou mais) num texto; valor em dinheiro nao e codigo.
pub fn codigos(txt: &str) -> Vec<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b\d+(?:\.\d+){2,}\b").expect("regex fixa")).find_iter(txt).map(|m| m.as_str().to_string()).collect()
}

pub fn meta(pasta: &Path, numero: &str, cidade: &str, uf: &str) -> Result<Value, String> {
    let m = json!({ "numero": numero, "cidade": cidade, "projeto": format!("{numero}. orçamento de {}", cidade.to_lowercase()), "tipo": "documentos", "uf": uf });
    std::fs::write(pasta.join("meta.json"), serde_json::to_string_pretty(&m).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(m)
}

// ---------- montagem generica de uma capital ----------

fn brl(v: f64) -> String {
    format!("R$ {}", br(v))
}

/// _grandes = usado no extrato, mas nao vai ao site (tamanho).
pub fn achar_arq(pasta: &Path, nome: &str) -> PathBuf {
    let p = pasta.join(nome);
    if p.exists() { p } else { pasta.join("_grandes").join(nome) }
}

/// O ultimo acoes-<ano>-MM.json do levantamento.
pub fn acoes_do_ano(saida: &Path, ano: &str) -> Option<PathBuf> {
    let mut fs: Vec<PathBuf> = std::fs::read_dir(saida).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(&format!("acoes-{ano}-")) && n.to_string_lossy().ends_with(".json"))).collect();
    fs.sort();
    fs.pop()
}

/// "2026-MM" do ultimo acoes-2026-MM.json do levantamento (sem 2026: "2025-12").
pub fn mes_extrato(pasta_dados: &Path) -> String {
    acoes_do_ano(&pasta_dados.join("saida"), "2026").and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned())).and_then(|s| s.get(s.len().saturating_sub(2)..).map(|m| format!("2026-{m}"))).unwrap_or_else(|| "2025-12".into())
}

/// Pasta nova (vazia) da entrega de documentos.
pub fn nova_pasta(site: &Path, numero: &str, cidade: &str) -> Result<PathBuf, String> {
    let p = site.join("docs").join(format!("{numero}-{}-docs", slug(cidade)));
    if p.exists() {
        std::fs::remove_dir_all(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    std::fs::create_dir_all(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok(p)
}

fn json_de(arq: &Path) -> Result<Value, String> {
    let t = std::fs::read_to_string(arq).map_err(|e| format!("{}: {e}", arq.display()))?;
    serde_json::from_str(&t).map_err(|e| format!("{}: {e}", arq.display()))
}

/// Quem acha as linhas de um codigo nos .txt de despesa de um ano (com o indice de cada arquivo guardado).
pub struct Referencias<'a> {
    pub arquivos: &'a [Arq],
    pub partes: &'a dyn Fn(&str) -> Vec<String>,
    pub faixa_mx: usize,
    idx: std::cell::RefCell<BTreeMap<PathBuf, std::rc::Rc<Indice>>>,
}

impl<'a> Referencias<'a> {
    pub fn novo(arquivos: &'a [Arq], partes: &'a dyn Fn(&str) -> Vec<String>, faixa_mx: usize) -> Self {
        Referencias { arquivos, partes, faixa_mx, idx: Default::default() }
    }

    /// (nome do arquivo, linhas) de cada .txt de despesa do ano que tem o codigo.
    pub fn linhas_de(&self, ano: &str, cod: &str) -> Result<Vec<(String, Vec<usize>)>, String> {
        let mut out = vec![];
        for a in self.arquivos.iter().filter(|a| a.ano == ano && a.tipo == "despesa" && a.path.extension().is_some_and(|e| e == "txt")) {
            let ja = self.idx.borrow().get(&a.path).cloned();
            let idx = match ja {
                Some(i) => i,
                None => {
                    let i = std::rc::Rc::new(indice(&a.path, None)?);
                    self.idx.borrow_mut().insert(a.path.clone(), i.clone());
                    i
                }
            };
            let ns = achar(&idx, cod, &(self.partes)(cod));
            if !ns.is_empty() {
                out.push((a.nome(), ns));
            }
        }
        Ok(out)
    }

    /// "ARQUIVO linhas 3-5, 9; OUTRO linhas 2" ou o aviso de que o codigo nao esta no arquivo.
    pub fn ref_(&self, ano: &str, cod: &str) -> Result<String, String> {
        let r = self.linhas_de(ano, cod)?;
        Ok(if r.is_empty() { NAO_ACHADO.into() } else { r.iter().map(|(n, ns)| format!("{n} linhas {}", faixa(ns, self.faixa_mx))).collect::<Vec<_>>().join("; ") })
    }
}

/// Monta a entrega de uma capital. `site` = pasta do site local (com a pasta NNN-... do video e a subpasta docs);
/// `desenha(destino, titulo, secoes)` escreve o PDF-extrato. Devolve a pasta e o relatorio.
pub fn montar(site: &Path, numero: &str, cidade: &str, uf: &str, ad: &mut Adaptador, pasta_dados: &Path, desenha: &dyn Fn(&Path, &str, &[(String, Vec<String>)]) -> Result<(), String>) -> Result<(PathBuf, Value), String> {
    let (pasta, sai) = (ad.pasta.clone(), pasta_dados.join("saida"));
    let (fmd, md) = ler_fontes(site, numero)?;
    let nome_pdf = format!("{}_{uf}-EXTRATO-TRIBUNAL-SN.pdf", mes_extrato(pasta_dados)); // -SN: sem nome de eleito e sem CPF
    ad.arquivos = limpar(std::mem::take(&mut ad.arquivos), TETO_ARQ)?;
    let arqs = &ad.arquivos;
    let nomes_de = |ano: &str, tipo: &str| arqs.iter().filter(|a| a.ano == ano && a.tipo == tipo).map(Arq::nome).collect::<Vec<_>>().join(", ");
    let refs = Referencias::novo(arqs, ad.partes, ad.faixa_mx);
    let acha_codigos = |t: &str| ad.codigos.map_or_else(|| codigos(t), |f| f(t));
    let col = |k: &str| ad.col.get(k).cloned().ok_or_else(|| format!("adaptador sem a coluna {k}"));
    let previa = ad.col.get("previa").cloned().unwrap_or_else(|| "-".into());
    let filtro = |ano: &str, tipo: &str, padrao: &str| ad.filtro.get(&(ano.to_string(), tipo.to_string())).cloned().unwrap_or_else(|| padrao.into());
    // extrato
    let mut sobre = vec![
        format!("{cidade} ({uf}) - vídeo {numero}. Números tirados SÓ dos arquivos do tribunal de contas do estado, listados abaixo."),
        "Os .txt desta entrega são os arquivos do tribunal em tabela de texto (linha 1 = cabeçalho; CSV vai como veio; HTML/XLS/JSON/XML".into(),
        "viram uma linha por registro). As 'linhas' citadas aqui e no fontes.md são as linhas desses .txt.".into(),
        String::new(),
    ];
    sobre.extend(arqs.iter().map(|a| format!("- {}{}: {}", if a.enviado { "" } else { "(não enviado, grande demais) " }, a.nome(), a.origem)));
    let mut secs = vec![("Sobre este extrato".to_string(), sobre)];
    let mut rows = linhas_principais(&md);
    if let Some((i, _)) = rows.iter().find(|(_, c)| c.len() < 4) {
        return Err(format!("{}: linha {} da tabela principal com menos de 4 colunas", fmd.display(), i + 1));
    }
    let ano_de = |c: &[String]| ano_da(&c[0]).unwrap_or_default();
    let anos: BTreeSet<String> = rows.iter().filter_map(|(_, c)| ano_da(&c[0])).collect();
    // 1) referencia (arquivo, linhas, coluna) de cada numero
    let (mut tipo_de, mut ult): (BTreeMap<usize, &str>, Option<String>) = Default::default();
    for (i, c) in rows.iter_mut() {
        let (ano, dito) = (ano_de(c), c[0].to_lowercase());
        if dito.starts_with("sobrou") || c[3].contains("conta própria") {
            continue;
        }
        let cod = acha_codigos(&c[3]).into_iter().next().or_else(|| ult.clone().filter(|_| c[3].contains("mesma ação")));
        if let Some(cod) = cod {
            c[3] = format!("{} · coluna {} · {}", c[3], if dito.contains("previa") { previa.clone() } else { col("liquidado")? }, refs.ref_(&ano, &cod)?);
            ult = Some(cod);
            tipo_de.insert(*i, "acao");
        } else if dito.starts_with("arrecadou") {
            c[3] = format!("soma da coluna {} · {}", col("arrecadado")?, filtro(&ano, "receita", "todas as linhas"));
            tipo_de.insert(*i, "receita");
        } else if dito.starts_with("gastou") || dito.contains("empenhou") {
            c[3] = format!("soma da coluna {} · {}", col(if dito.contains("empenhou") { "empenhado" } else { "liquidado" })?, filtro(&ano, "despesa", "todas as linhas"));
            tipo_de.insert(*i, "despesa");
        }
    }
    // 2) extrato
    for ano in &anos {
        let nums: Vec<&Vec<String>> = rows.iter().map(|x| &x.1).filter(|c| &ano_de(c) == ano).collect();
        secs.push((format!("{ano}: números do vídeo (arquivo do tribunal, linhas e coluna)"), nums.iter().map(|c| format!("{:<26} {:>24}   {}", c[0], c[1], c[3])).collect()));
        let (mut lit, mut cods): (Vec<String>, Vec<String>) = (vec![], vec![]); // linhas literais do tribunal para as acoes de festa
        for c in &nums {
            for x in acha_codigos(&c[3]) {
                if !cods.contains(&x) {
                    cods.push(x);
                }
            }
        }
        for cod in &cods {
            for (nome, ns) in refs.linhas_de(ano, cod)? {
                let t = ler_linhas(&achar_arq(&pasta, &nome), None)?;
                let linha = |n: usize| t.get(n - 1).map(|r| r.join(" | ")).ok_or_else(|| format!("{nome}: linha {n} sumiu do arquivo"));
                lit.push(format!("ação {cod} - arquivo {nome} - {} linha(s); cabeçalho na linha 1:", ns.len()));
                lit.push(format!("   1: {}", linha(1)?));
                for n in ns.iter().take(400) {
                    lit.push(format!("{n:>6}: {}", linha(*n)?));
                }
                lit.push(String::new());
            }
        }
        if !lit.is_empty() {
            secs.push((format!("{ano}: linhas do tribunal das ações de festa (previa = {previa}; gastou = {})", col("liquidado")?), lit));
        }
        if let Some(ac) = acoes_do_ano(&sai, ano) {
            let v = json_de(&ac)?;
            let mut l: Vec<&Value> = v.as_array().ok_or_else(|| format!("{}: não é uma lista de ações", ac.display()))?.iter().collect();
            let liq = |x: &Value| x["liquidado"].as_f64().unwrap_or(0.0);
            l.sort_by(|a, b| liq(b).total_cmp(&liq(a)));
            let mut linhas = vec![format!("filtro: {}", filtro(ano, "despesa", "-")), String::new()];
            for x in &l {
                let cod = x["cod"].as_str().ok_or_else(|| format!("{}: ação sem código", ac.display()))?;
                linhas.push(format!("{cod:<28} {:<60} {:>22}   {}", x["acao"].as_str().unwrap_or("").chars().take(60).collect::<String>(), brl(liq(x)), refs.ref_(ano, cod)?));
            }
            secs.push((format!("{ano}: despesa por ação = soma da coluna {} = {}", col("liquidado")?, brl(l.iter().map(|x| liq(x)).sum())), linhas));
        }
        let rc = sai.join(format!("receita-{ano}.json"));
        if rc.exists() {
            let v = json_de(&rc)?;
            let r: Vec<(&str, f64)> = v.as_array().into_iter().flatten().map(|x| Ok((x[0].as_str().ok_or("origem sem nome")?, x[1].as_f64().ok_or("origem sem valor")?))).collect::<Result<_, &str>>().map_err(|e| format!("{}: {e}", rc.display()))?;
            let mut linhas = vec![format!("filtro: {}", filtro(ano, "receita", "-")), String::new()];
            linhas.extend(r.iter().map(|(k, v)| format!("{k:<60} {:>24}", brl(*v))));
            linhas.extend([String::new(), format!("arquivo(s): {}", nomes_de(ano, "receita"))]);
            secs.push((format!("{ano}: receita por origem = soma da coluna {} = {}", col("arrecadado")?, brl(r.iter().map(|x| x.1).sum())), linhas));
        }
    }
    let titulo = format!("{numero}. orçamento de {} - extrato dos arquivos do tribunal ({})", cidade.to_lowercase(), ad.tribunal);
    let pags = paginas(&secs, 186);
    desenha(&pasta.join(&nome_pdf), &lat(&titulo), &secoes_limpas(&secs))?;
    // 3) fontes.md: arquivo + pagina do extrato
    let mut l: Vec<String> = md.split('\n').map(String::from).collect();
    for (i, c) in rows.iter_mut() {
        let Some(t) = tipo_de.get(i) else { continue };
        let ano = ano_de(c);
        let arq = Some(nomes_de(&ano, if *t == "receita" { "receita" } else { "despesa" })).filter(|a| !a.is_empty()).unwrap_or_else(|| "-".into());
        let chave = format!("{ano}: {}", match *t { "acao" => "números", x => x });
        let pg = pags.iter().find(|(k, _)| k.starts_with(&chave)).map_or("None".to_string(), |x| x.1.to_string());
        c[2] = format!("{arq} (extrato: {nome_pdf} p. {pg})");
        l[*i] = format!("| {} |", c.join(" | "));
    }
    let (setor, sem_pagina) = (Regex::new(r"^## Setores (\d{4})").expect("regex fixa"), Regex::new(r"\(([\d.]+), p\. 0\)").expect("regex fixa"));
    let (mut ano_set, mut out): (Option<String>, Vec<String>) = (None, vec![]);
    for linha in l {
        if let Some(m) = setor.captures(&linha) {
            ano_set = Some(m[1].to_string());
        }
        let mut linha = linha;
        if let Some(a) = ano_set.as_ref().filter(|_| linha.contains("p. 0)")) {
            let mut erro = None;
            linha = sem_pagina.replace_all(&linha, |m: &regex::Captures| format!("({}, {})", &m[1], refs.ref_(a, &m[1]).unwrap_or_else(|e| erro.insert(e).clone()))).into_owned();
            if let Some(e) = erro {
                return Err(e);
            }
        }
        out.push(linha.replace("página do PCMGE065", &format!("linhas no arquivo do tribunal ({})", ad.tribunal)));
    }
    let md2 = sem_pessoa(&out.join("\n"));
    let nome_md = fmd.file_name().ok_or("fontes.md sem nome")?;
    let grava = |p: PathBuf| std::fs::write(&p, &md2).map_err(|e| format!("{}: {e}", p.display()));
    grava(pasta.join(nome_md))?;
    let bak = copia_original(&fmd);
    if !bak.exists() {
        std::fs::copy(&fmd, &bak).map_err(|e| format!("{}: {e}", fmd.display()))?;
    }
    grava(fmd.clone())?;
    meta(&pasta, numero, cidade, uf)?;
    let mb = arqs.iter().filter_map(|a| std::fs::metadata(&a.path).ok()).map(|m| m.len() as f64).sum::<f64>() / 1e6;
    let rel = json!({ "pdf": nome_pdf, "paginas": pags.iter().map(|(k, v)| (k.clone(), json!(v))).collect::<serde_json::Map<_, _>>(), "codigos_nao_achados": md2.matches("não achado").count(),
        "arquivos": arqs.iter().map(Arq::nome).collect::<Vec<_>>(), "mb": (mb * 10.0).round() / 10.0 });
    Ok((pasta, rel))
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn pasta(nome: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("braniac-link_docs_capitais-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    pub fn grava(arq: &Path, t: &str) {
        std::fs::create_dir_all(arq.parent().unwrap()).unwrap();
        std::fs::write(arq, t).unwrap();
    }

    #[test]
    fn nome_de_eleito_sai_e_o_resto_fica() {
        let l = "3390390000000000\t15000000202406 - EMENDA PARLAMENTAR DO VEREADOR JUCELINO RODRIGUES\t250.000,00";
        assert_eq!(sem_pessoa(l), "3390390000000000\t15000000202406 - EMENDA PARLAMENTAR DO VEREADOR [nome omitido]\t250.000,00");
        assert_eq!(sem_pessoa("EMENDA DA VEREADORA PROFESSORA IOLANDA CASTRO"), "EMENDA DA VEREADORA [nome omitido]");
        assert_eq!(sem_pessoa("Rua Deputado Fulano de Tal, 10"), "Rua Deputado [nome omitido], 10");
        assert_eq!(sem_pessoa("Clube - Vereador Gabrielzinho R$ 100.000,00 (PRE.1)"), "Clube - Vereador [nome omitido] R$ 100.000,00 (PRE.1)");
        // ate 6 palavras, com de/dos/e no meio; nunca atravessa a coluna
        assert_eq!(sem_pessoa("emenda dep. José D'Ávila dos Santos e Silva Irmao. Neto Filho;10|Ver. - Ana"), "emenda dep. [nome omitido] Filho;10|Ver. - [nome omitido]");
        assert_eq!(sem_pessoa("DEPUTADA ESTADUAL - Maria das Dores\nSENADORA: Bia E Cia"), "DEPUTADA ESTADUAL - [nome omitido]\nSENADORA: [nome omitido]");
        for fica in ["GABINETE DO PREFEITO\t10", "PREFEITURA MUNICIPAL DE PALMAS", "GABINETE DO PREFEITO E VICE", "SUBSIDIO DOS VEREADORES\t1", "DEPUTADO FEDERAL", "SUPERVEREADOR Fulano", "PREFEITO: de 2024", "VEREADOR 123"] {
            assert_eq!(sem_pessoa(fica), fica);
        }
    }

    #[test]
    fn cpf_sai_cnpj_fica() {
        assert_eq!(sem_pessoa("credor 123.456.789-09 ok"), "credor [omitido] ok");
        assert_eq!(sem_pessoa("***.456.789-**"), "[omitido]");
        assert_eq!(sem_pessoa("24.851.511/0001-85 e 1123.456.789-091 e 9.123.456.789-09"), "24.851.511/0001-85 e 1123.456.789-091 e 9.123.456.789-09");
        let l = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(sem_cpf_coluna(l(&["nome;cpf_cnpj;valor", "a;12345678909;1", "b;24851511000185;2", "c;12345678909", "d;\"12345678909\";3"])), l(&["nome;cpf_cnpj;valor", "a;[omitido];1", "b;24851511000185;2", "c;12345678909", "d;[omitido];3"]));
        assert_eq!(sem_cpf_coluna(l(&["nome\tdoc", "a\t12345678909"])), l(&["nome\tdoc", "a\t12345678909"]));
    }

    #[test]
    fn limpar_renomeia_divide_e_guarda_as_linhas() {
        let p = pasta("limpar");
        let (igual, nome, grande, fora) = (p.join("2024-12_XX-RECEITA.txt"), p.join("2024-12_XX-DESPESA.txt"), p.join("2025-12_XX-PAGAMENTOS.txt"), p.join("2025-12_XX-FORA.txt"));
        grava(&igual, "a\tb\n1\t2\n");
        std::fs::write(&nome, b"cod\tfonte\n1\tEMENDA DO VEREADOR FULANO SILVA\n2\tDOTA\xc7\xc3O\n").unwrap(); // latin-1 continua latin-1
        grava(&grande, &format!("cod\tv\n{}", (0..100).map(|i| format!("{i}\t{}\n", "x".repeat(40))).collect::<String>()));
        grava(&fora, "cod\tcpf\n1\t12345678909\n");
        let a = |path: &Path, ano: &str| Arq::novo(ano, "despesa", path.to_path_buf(), "TCE".into());
        let arqs = limpar(vec![a(&igual, "2024"), a(&nome, "2024"), a(&grande, "2025"), Arq { enviado: false, ..a(&fora, "2025") }, a(&p.join("x.pdf"), "2025")], 2000).unwrap();
        let nomes: Vec<String> = arqs.iter().map(Arq::nome).collect();
        assert!(nomes[..2] == ["2024-12_XX-RECEITA.txt", "2024-12_XX-DESPESA-SN.txt"] && !nome.exists() && !grande.exists());
        assert_eq!(std::fs::read(&arqs[1].path).unwrap(), b"cod\tfonte\n1\tEMENDA DO VEREADOR [nome omitido]\n2\tDOTA\xc7\xc3O\n"); // mesmas linhas do original
        let partes = &arqs[2..arqs.len() - 2];
        assert!(partes.len() > 1 && nomes[2] == "2025-12_XX-PAGAMENTOS-SN-PARTE1.txt" && partes[1].origem == "TCE (parte 2)");
        let mut corpo = vec![];
        for a in partes {
            let t = std::fs::read_to_string(&a.path).unwrap();
            let l: Vec<&str> = t.split('\n').collect();
            assert!(l[0] == "cod\tv" && t.len() <= 2100);
            corpo.extend(l[1..].iter().filter(|x| !x.is_empty()).map(|x| x.split('\t').next().unwrap().to_string()));
        }
        assert_eq!(corpo, (0..100).map(|i| i.to_string()).collect::<Vec<_>>());
        // o que nao vai ao site e limpo no lugar; o que nao e .txt fica como esta
        assert!(nomes[nomes.len() - 2..] == ["2025-12_XX-FORA.txt", "x.pdf"] && std::fs::read_to_string(&fora).unwrap() == "cod\tcpf\n1\t[omitido]\n");
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn tabelas_faixas_e_codigos() {
        assert_eq!((faixa(&[3, 4, 5, 9], 6), faixa(&[], 6), faixa(&[9, 1, 2, 2, 5, 7], 2)), ("3-5, 9".into(), "não achei".into(), "1-2, 5 (+2 trechos)".into()));
        assert_eq!(codigos("ação X (26.23.695.7000.4035) · R$ 1.234,00"), ["26.23.695.7000.4035"]);
        assert_eq!((slug("São João d'Aliança"), cel(" a\tb\r\nc "), ano_da("Gastou (2026 até 08)"), ano_da("x")), ("sao-joao-d-alianca".into(), "a b c".into(), Some("2026".into()), None));
        assert_eq!(tabela_html(b"<TABLE><tr><th>Unidade</th><th>Valor</th></tr>\n<tr class=x><td><b>Sa\xfade &amp; Cia</b>\n</td><td>1,00</td></tr></table>"), [vec!["Unidade", "Valor"], vec!["Saúde & Cia", "1,00"]]);
        let j = json!({ "meta": [{ "x": 1 }], "dados": { "linhas": [{ "b": 1.0, "a": "x\ty" }, { "a": null, "c": true }] } });
        assert_eq!(tabela_json(&j), [vec!["a", "b", "c"], vec!["x y", "1.0", ""], vec!["", "", "True"]]);
        assert_eq!(tabela_json(&json!(3)), [Vec::<String>::new()]);
        assert_eq!(csv_py("a;\"b;\"\"c\"\"\nd\";e \"f\"\r\n\r\ng;;\n", ';'), [vec!["a", "b;\"c\"\nd", "e \"f\""], vec![], vec!["g", "", ""]]);
        assert_eq!(lat("a · b → c… ação ✓"), "a - b -> c... ação ?");
        let p = pasta("achar");
        let t = gravar(&p, "x.txt", &[vec!["codigo_acao_video".into(), "remessa".into(), "liq".into()], vec!["14.23.695.7000.4035".into(), "8".into(), "1,00".into()], vec!["26.23.695.7000.4035".into(), "14".into(), "2,00".into()], vec!["0026".into(), "23".into(), "695 7000 04035".into()]]).unwrap();
        let idx = indice(&t, None).unwrap();
        assert_eq!((achar(&idx, "26.23.695.7000.4035", &["26.23.695.7000.4035".into()]), achar(&idx, "14.23.695.7000.4035", &["14.23.695.7000.4035".into()])), (vec![3], vec![2]));
        assert_eq!((achar(&idx, "26.23", &[]), achar(&idx, "26.99", &[]), achar(&idx, "", &[])), (vec![4], vec![], vec![])); // pelos pedacos: todos tem de estar na linha
        let a = gravar_com_aspas(&p, "a.txt", &[vec!["h".into(), "v".into()], vec!["diz \"oi\"".into(), "1".into()], vec!["\"x".into(), "2".into()]]).unwrap();
        assert_eq!(ler_linhas(&a, None).unwrap(), [vec!["h", "v"], vec!["diz \"oi\"", "1"], vec!["\"x", "2"]]);
        // a secao so comeca em pagina nova quando sobra pouco espaco; linha comprida ocupa mais de uma
        let linhas = |n: usize| (0..n).map(|i| format!("linha {i}")).collect::<Vec<_>>();
        let pg = paginas(&[("a".into(), linhas(55)), ("b".into(), linhas(1)), ("c".into(), vec!["x".repeat(400)]), ("d".into(), linhas(70))], 186);
        assert_eq!(pg, [("a".to_string(), 1), ("b".to_string(), 2), ("c".to_string(), 2), ("d".to_string(), 2)]);
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn monta_a_entrega_de_uma_capital() {
        let raiz = pasta("montar");
        let (site, dados) = (raiz.join("site"), raiz.join("RECEITAS-DESPESAS-TESTE-XX"));
        let fontes = site.join("999-teste/999. orçamento de teste - fontes.md");
        grava(&fontes, "# Fontes\n\n| Valor dito | Valor exato | Arquivo | Página / linha |\n|---|---|---|---|\n| Arrecadou (2025) | R$ 10,00 | painel | soma do arrecadado |\n| Gastou (2025) | R$ 8,00 | painel | soma do liquidado |\n| Sobrou (2025) | R$ 2,00 | conta própria | conta própria: 10 − 8 |\n| Festas previa (2025) | R$ 1,00 | painel | ação FESTA DO VEREADOR FULANO (1.13.392) · orçado |\n| Festas gastou (2025) | R$ 5,00 | painel | mesma ação · liquidado |\n| Já empenhou (2026 até 08) | R$ 9,00 | painel | soma do empenhado |\n\n## Setores 2025\n- Cultura: R$ 5,00 (1.13.392, p. 0); Saúde (7.10.301, p. 0), página do PCMGE065\n");
        grava(&dados.join("saida/acoes-2025-12.json"), &json!([{ "cod": "2.12.361", "acao": "ESCOLAS", "liquidado": 3.0 }, { "cod": "1.13.392", "acao": "FESTA", "liquidado": 5.0 }]).to_string());
        grava(&dados.join("saida/acoes-2026-08.json"), "[]");
        grava(&dados.join("saida/receita-2025.json"), &json!([["Impostos", 10.0]]).to_string());
        let p = nova_pasta(&site, "999", "Teste").unwrap();
        let desp = gravar(&p, "2025-12_XX-DESPESA.txt", &[vec!["codigo_acao_video".into(), "nome".into(), "liq".into()], vec!["1.13.392".into(), "FESTA DO PREFEITO BELTRANO".into(), "2,00".into()], vec!["2.12.361".into(), "ESCOLAS".into(), "3,00".into()], vec!["1.13.392".into(), "FESTA".into(), "3,00".into()]]).unwrap();
        let rec = gravar(&p, "2025-12_XX-RECEITA.txt", &[vec!["origem".into(), "valor".into()], vec!["Impostos".into(), "10,00".into()]]).unwrap();
        let inteiro = |c: &str| vec![c.to_string()];
        let mut ad = Adaptador { pasta: p.clone(), tribunal: "TCE-XX".into(), arquivos: vec![Arq::novo("2025", "despesa", desp, "TCE-XX, arquivo de despesa".into()), Arq::novo("2025", "receita", rec, "TCE-XX, arquivo de receita".into())],
            col: [("liquidado", "'liq'".to_string()), ("previa", "'orcado'".to_string()), ("empenhado", "'emp'".to_string()), ("arrecadado", "'valor'".to_string())].into(), filtro: [(("2025".to_string(), "despesa".to_string()), "todas as linhas do arquivo".to_string())].into(), partes: &inteiro, codigos: None, faixa_mx: 6 };
        let desenhado = std::cell::RefCell::new(vec![]);
        let desenha = |destino: &Path, titulo: &str, secs: &[(String, Vec<String>)]| {
            desenhado.borrow_mut().push((destino.to_path_buf(), titulo.to_string(), secs.to_vec()));
            Ok(())
        };
        let (pasta_e, rel) = montar(&site, "999", "Teste", "XX", &mut ad, &dados, &desenha).unwrap();
        // o arquivo com nome de eleito virou -SN, com as mesmas linhas
        assert_eq!(rel["arquivos"], json!(["2025-12_XX-DESPESA-SN.txt", "2025-12_XX-RECEITA.txt"]));
        assert_eq!((rel["pdf"].as_str(), &rel["codigos_nao_achados"], &rel["paginas"]["2025: números do vídeo (arquivo do tribunal, linhas e coluna)"]), (Some("2026-08_XX-EXTRATO-TRIBUNAL-SN.pdf"), &json!(1), &json!(1)));
        let md = std::fs::read_to_string(pasta_e.join("999. orçamento de teste - fontes.md")).unwrap();
        assert_eq!(md, std::fs::read_to_string(&fontes).unwrap());
        for esperado in [
            "| Arrecadou (2025) | R$ 10,00 | 2025-12_XX-RECEITA.txt (extrato: 2026-08_XX-EXTRATO-TRIBUNAL-SN.pdf p. 1) | soma da coluna 'valor' · todas as linhas |",
            "| Gastou (2025) | R$ 8,00 | 2025-12_XX-DESPESA-SN.txt (extrato: 2026-08_XX-EXTRATO-TRIBUNAL-SN.pdf p. 1) | soma da coluna 'liq' · todas as linhas do arquivo |",
            "| Sobrou (2025) | R$ 2,00 | conta própria | conta própria: 10 − 8 |",
            "ação FESTA DO VEREADOR [nome omitido] (1.13.392) · orçado · coluna 'orcado' · 2025-12_XX-DESPESA-SN.txt linhas 2, 4 |",
            "| mesma ação · liquidado · coluna 'liq' · 2025-12_XX-DESPESA-SN.txt linhas 2, 4 |",
            "| Já empenhou (2026 até 08) | R$ 9,00 | - (extrato: 2026-08_XX-EXTRATO-TRIBUNAL-SN.pdf p. 1) | soma da coluna 'emp' · todas as linhas |",
            "- Cultura: R$ 5,00 (1.13.392, 2025-12_XX-DESPESA-SN.txt linhas 2, 4); Saúde (7.10.301, código não achado no arquivo (ver extrato)), linhas no arquivo do tribunal (TCE-XX)",
        ] {
            assert!(md.contains(esperado), "{esperado}\n{md}");
        }
        let d = desenhado.borrow();
        let (destino, titulo, secs) = &d[0];
        assert!(destino.ends_with("docs/999-teste-docs/2026-08_XX-EXTRATO-TRIBUNAL-SN.pdf") && titulo == "999. orçamento de teste - extrato dos arquivos do tribunal (TCE-XX)");
        assert_eq!(secs.iter().map(|s| s.0.as_str()).collect::<Vec<_>>(), ["Sobre este extrato", "2025: números do vídeo (arquivo do tribunal, linhas e coluna)", "2025: linhas do tribunal das ações de festa (previa = 'orcado'; gastou = 'liq')", "2025: despesa por ação = soma da coluna 'liq' = R$ 8,00", "2025: receita por origem = soma da coluna 'valor' = R$ 10,00", "2026: números do vídeo (arquivo do tribunal, linhas e coluna)", "2026: despesa por ação = soma da coluna 'liq' = R$ 0,00"]);
        assert_eq!(secs[2].1, ["ação 1.13.392 - arquivo 2025-12_XX-DESPESA-SN.txt - 2 linha(s); cabeçalho na linha 1:", "   1: codigo_acao_video | nome | liq", "     2: 1.13.392 | FESTA DO PREFEITO [nome omitido] | 2,00", "     4: 1.13.392 | FESTA | 3,00", ""]);
        assert!(secs[3].1[2].starts_with(&format!("{:<28} FESTA ", "1.13.392")) && secs[3].1[2].ends_with("R$ 5,00   2025-12_XX-DESPESA-SN.txt linhas 2, 4") && secs[0].1[4] == "- 2025-12_XX-DESPESA-SN.txt: TCE-XX, arquivo de despesa");
        let meta: Value = serde_json::from_str(&std::fs::read_to_string(pasta_e.join("meta.json")).unwrap()).unwrap();
        assert_eq!(meta, json!({ "numero": "999", "cidade": "Teste", "projeto": "999. orçamento de teste", "tipo": "documentos", "uf": "XX" }));
        drop(d);
        // rodar de novo parte do fontes.md ORIGINAL (guardado ao lado), nao do que ja foi reescrito
        let p = nova_pasta(&site, "999", "Teste").unwrap();
        ad.pasta = p.clone();
        ad.arquivos = vec![];
        let (_, rel) = montar(&site, "999", "Teste", "XX", &mut ad, &dados, &desenha).unwrap();
        assert_eq!(rel["codigos_nao_achados"], json!(4)); // sem os arquivos, cada codigo fica dito como nao achado
        assert!(std::fs::read_to_string(&fontes).unwrap().contains("| soma da coluna 'valor' · todas as linhas |") && std::fs::read_to_string(copia_original(&fontes)).unwrap().contains("| painel | soma do arrecadado |"));
        assert!(montar(&site, "123", "Teste", "XX", &mut ad, &dados, &desenha).unwrap_err().contains("vídeo 123 sem pasta no site"));
        let _ = std::fs::remove_dir_all(&raiz);
    }
}
