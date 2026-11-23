//! Parte comum dos moldes dos tribunais (link/tribunais/base.py): recebe o que o molde do estado LEU NO TRIBUNAL (acoes com
//! orcado/empenhado/liquidado e receita por origem) e grava os arquivos que o gerador entende (saida/cidade.json, acoes-*.json,
//! receita-*.json, festas-cods.json). Regras: gastou = liquidado; festas e eventos separados da cultura; achado = acao de festas
//! (previa = orcado inicial x liquidado); segundo par = beneficios eventuais ou, sem essa acao, o previsto atualizado das festas.
//! Nenhum numero vem de fora do tribunal. Soma que nao fecha, ano que falta ou achado que nao existe = Err (vira pendencia,
//! nunca video); nada e estimado.
//! Entra como closure: `setores(saida)`, o script do pacote que monta o setores.json.
//! Ficou de fora: `AbaEscondida` (aba do navegador fora da tela) e o `cli`.
#![allow(dead_code)]
use crate::link_auditar::br;
use crate::texto_pt::sem_acento;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

pub const ANOS: [&str; 3] = ["2024", "2025", "2026"];
pub const NM: [&str; 12] = ["janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro", "dezembro"];
pub const ABR: [&str; 12] = ["jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez"];

/// Uma acao (projeto/atividade) como o tribunal publica.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Acao {
    pub f: String,
    pub sub: String,
    pub cod: String,
    pub acao: String,
    pub orcado: f64,
    pub atualizado: f64,
    pub empenhado: f64,
    pub liquidado: f64,
}

/// Um ano lido no tribunal. Os totais vem do tribunal e TEM de fechar com a soma das linhas.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ano {
    pub acoes: Vec<Acao>,
    pub receita: Vec<(String, f64)>,
    pub arrecadado: f64,
    pub liquidado: f64,
    pub empenhado: f64,
    /// Documento/sistema de onde saiu (vai as fontes).
    pub doc_despesa: String,
    pub doc_receita: String,
}

pub type Dados = BTreeMap<String, Ano>;

#[derive(Default)]
pub struct Opcoes<'a> {
    /// Regex a mais para festas, do molde do estado.
    pub festas_extra: Option<&'a str>,
    /// O tribunal NAO publica o orcado por acao: o achado fala so o gasto das festas; nada e estimado.
    pub sem_previsto: bool,
    /// (regex, nome real): acao de ajuda a populacao com outro nome, usada so se nao houver beneficios eventuais.
    pub social: Option<(&'a str, &'a str)>,
    /// "o que o tribunal recebeu de 2026": a prefeitura nao entregou 2026 -> video de pauta; 2026 nao e lido.
    pub ausente26: Option<&'a str>,
}

/// Arredonda a 2 casas como o round() do Python (pelo valor exato do numero).
pub fn r2(x: f64) -> f64 {
    format!("{x:.2}").parse().unwrap_or(0.0)
}

/// A mesma regra de festas do script de setores (senao a fala de "festas e eventos" no ano e no achado da dois valores).
pub fn eh_festa(acao: &str, extra: Option<&Regex>) -> bool {
    static RE: OnceLock<(Regex, Regex)> = OnceLock::new();
    let (geral, comemora) = RE.get_or_init(|| {
        (
            Regex::new(r"(?i)EVENTOS DE CAR[ÁA]TER POPULAR|EVENTOS TRADICIONAIS|EVENTOS CULTURAIS E TRADICIONAIS|\bFESTAS?\b|FESTEJOS?|S[ÃA]O JO[ÃA]O|CARNAVAL|\bSHOWS?\b").expect("regex fixa"),
            Regex::new(r"(?i)COMEMORA[ÇC][ÃA]O (DE |E |DAS? )?FESTIVIDADES").expect("regex fixa"),
        )
    });
    // "PARA COMEMORACAO DE FESTIVIDADES" nao e festa (no Python era um lookbehind)
    let para = |i: usize| acao[..i].to_uppercase().ends_with("PARA ");
    geral.is_match(acao) || comemora.find_iter(acao).any(|m| !para(m.start())) || extra.is_some_and(|e| e.is_match(acao))
}

pub fn eh_beneficio(acao: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)BENEF[IÍ]CIOS? EVENTUA").expect("regex fixa")).is_match(acao)
}

fn decimal(x: f64, casas: usize) -> String {
    format!("{x:.casas$}").replace('.', ",")
}

/// Valor curto para a capa: "R$ 1,50 bi", "R$ 2,00 mi", "R$ 3,5 mil".
pub fn capa(v: f64) -> String {
    if v >= 1e9 {
        format!("R$ {} bi", decimal(v / 1e9, 2))
    } else if v >= 1e6 {
        format!("R$ {} mi", decimal(v / 1e6, 2))
    } else {
        format!("R$ {} mil", decimal(v / 1e3, 1))
    }
}

/// So as letras, sem acento, minusculas ("São João d'Aliança" -> "saojoaodalianca").
pub fn slug(nome: &str) -> String {
    sem_acento(nome).chars().filter(char::is_ascii_alphabetic).collect()
}

pub type Linha = BTreeMap<String, String>;

/// CSV com cabecalho -> uma linha por registro (aspas, aspas dobradas e quebra de linha dentro de aspas).
pub fn csv(texto: &str, sep: char) -> Vec<Linha> {
    let (mut regs, mut reg, mut campo, mut aspas): (Vec<Vec<String>>, Vec<String>, String, bool) = (vec![], vec![], String::new(), false);
    let mut it = texto.trim_start_matches('\u{feff}').chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '"' if aspas && it.peek() == Some(&'"') => {
                campo.push('"');
                it.next();
            }
            '"' => aspas = !aspas,
            c if c == sep && !aspas => reg.push(std::mem::take(&mut campo)),
            '\r' if !aspas => {}
            '\n' if !aspas => {
                reg.push(std::mem::take(&mut campo));
                regs.push(std::mem::take(&mut reg));
            }
            c => campo.push(c),
        }
    }
    if !campo.is_empty() || !reg.is_empty() {
        reg.push(campo);
        regs.push(reg);
    }
    let mut regs = regs.into_iter().filter(|r| r.len() > 1 || r.iter().any(|c| !c.is_empty()));
    let Some(cab) = regs.next() else { return vec![] };
    regs.map(|r| cab.iter().cloned().zip(r).collect()).collect()
}

/// A coluna da linha; coluna que nao existe no arquivo e erro (nunca vira zero calado).
pub fn col<'a>(r: &'a Linha, k: &str) -> Result<&'a str, String> {
    r.get(k).map(String::as_str).ok_or_else(|| format!("arquivo do tribunal sem a coluna {k}"))
}

/// Valor da coluna: vazio = 0; texto que nao e numero e erro.
pub fn valor(r: &Linha, k: &str) -> Result<f64, String> {
    let t = col(r, k)?.trim();
    if t.is_empty() { Ok(0.0) } else { t.parse().map_err(|_| format!("coluna {k} com valor que não é número: {t:?}")) }
}

/// Primeira letra maiuscula, o resto minusculo.
pub fn maiuscula(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|p| p.to_uppercase().chain(c.flat_map(char::to_lowercase)).collect()).unwrap_or_default()
}

/// O valor redondo que deixa `valor_mi / p` mais perto de `alvo` (o primeiro, se empatar).
pub fn redondo(valor_mi: f64, alvo: f64, opcoes: &[f64]) -> f64 {
    opcoes.iter().copied().fold(None, |m: Option<f64>, p| if m.is_none_or(|m| (valor_mi / p - alvo).abs() < (valor_mi / m - alvo).abs()) { Some(p) } else { m }).unwrap_or(1.0)
}

fn n_json(v: f64) -> Value {
    if v.fract() == 0.0 && v.abs() < 1e15 { json!(v as i64) } else { json!(v) }
}

fn grava(arq: &Path, v: &Value) -> Result<(), String> {
    std::fs::write(arq, serde_json::to_string_pretty(v).map_err(|e| e.to_string())?).map_err(|e| format!("{}: {e}", arq.display()))
}

fn apaga(saida: &Path, casa: &dyn Fn(&str) -> bool) {
    for e in std::fs::read_dir(saida).into_iter().flatten().flatten() {
        if casa(&e.file_name().to_string_lossy()) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

fn funcao(a: &Acao) -> Result<i64, String> {
    a.f.trim().parse::<f64>().map(|f| f as i64).map_err(|_| format!("ação {} com função que não é número: {:?}", a.cod, a.f))
}

fn primeiro_max<'a>(chave: &dyn Fn(&str) -> f64) -> &'a str {
    ANOS.iter().copied().fold(ANOS[0], |m, a| if chave(a) > chave(m) { a } else { m })
}

type Par = BTreeMap<&'static str, (f64, f64)>; // ano -> (previa, gastou)

fn par_json(f: &Par) -> Value {
    Value::Object(f.iter().map(|(a, (p, g))| (a.to_string(), json!({ "prev": p, "gasto": g }))).collect())
}

fn mapa_json(b: &BTreeMap<&'static str, f64>) -> Value {
    Value::Object(b.iter().map(|(a, v)| (a.to_string(), json!(v))).collect())
}

/// Monta saida/cidade.json e os arquivos do gerador a partir do que o molde leu. `tribunal` = ficha do tribunal (sigla, fala...).
#[allow(clippy::too_many_arguments)]
pub fn montar(pasta: &Path, numero: &str, nome: &str, uf: &str, ibge: i64, tribunal: &Value, dados: &Dados, mes26: &str, op: &Opcoes, setores: &dyn Fn(&Path) -> Result<(), String>) -> Result<Value, String> {
    let m26: usize = mes26.parse().ok().filter(|m| (1..=12).contains(m)).ok_or_else(|| format!("mês de corte de 2026 inválido: {mes26:?}"))?;
    let sigla_t = tribunal["sigla"].as_str().ok_or("ficha do tribunal sem sigla")?;
    let saida = pasta.join("saida");
    std::fs::create_dir_all(&saida).map_err(|e| e.to_string())?;
    // sobra de um mes anterior dava dois blocos de 2026 e travava a voz
    apaga(&saida, &|n| n.starts_with("acoes-") && n.ends_with(".json"));
    let extra = op.festas_extra.map(|e| Regex::new(&format!("(?i){e}")).map_err(|e| format!("regex de festas do molde: {e}"))).transpose()?;
    let social = op.social.map(|(rx, nm)| Regex::new(&format!("(?i){rx}")).map(|r| (r, nm)).map_err(|e| format!("regex da ação social do molde: {e}"))).transpose()?;
    let ausente = op.ausente26.is_some();
    let sg = slug(nome);
    let mut anos: BTreeMap<&str, (f64, f64, f64)> = BTreeMap::new(); // arrec, gastou, emp
    let (mut f, mut bn, mut ba, mut bs): (Par, BTreeMap<&str, f64>, BTreeMap<&str, f64>, BTreeMap<&str, f64>) = Default::default();
    let mut cods: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    let mut totais: Vec<Value> = vec![];
    let doc = |a: &str| dados.get(a).map(|d| (d.doc_despesa.clone(), d.doc_receita.clone())).unwrap_or_default();
    for a in ANOS {
        if a == "2026" && ausente {
            apaga(&saida, &|n| n.contains("-2026") && n.ends_with(".json")); // 2026 de uma rodada antiga nao pode virar setor
            anos.insert(a, (0.0, 0.0, 0.0));
            f.insert(a, (0.0, 0.0));
            (ba.insert(a, 0.0), bn.insert(a, 0.0), bs.insert(a, 0.0), cods.insert(a, vec![]));
            continue;
        }
        let d = dados.get(a).ok_or_else(|| format!("{a}: o molde não leu esse ano no tribunal"))?;
        let mes = if a == "2026" { mes26 } else { "12" };
        let ate = format!("jan a {}/{a}", ABR[if a == "2026" { m26 - 1 } else { 11 }]);
        let soma: f64 = d.acoes.iter().map(|x| x.liquidado).sum();
        if (soma - d.liquidado).abs() >= 1.0 {
            return Err(format!("{a}: soma das ações {soma:.2} != total liquidado {:.2}", d.liquidado));
        }
        let sr: f64 = d.receita.iter().map(|(_, v)| v).sum();
        if (sr - d.arrecadado).abs() >= 1.0 {
            return Err(format!("{a}: origens da receita {sr:.2} != total {:.2}", d.arrecadado));
        }
        anos.insert(a, (r2(d.arrecadado), r2(d.liquidado), r2(d.empenhado)));
        let js = d.acoes.iter().map(|x| Ok(json!({ "f": funcao(x)?.to_string(), "cod": x.cod, "acao": x.acao, "liquidado": r2(x.liquidado), "pag": 0 }))).collect::<Result<Vec<_>, String>>()?;
        grava(&saida.join(format!("acoes-{a}-{mes}.json")), &json!(js))?;
        grava(&saida.join(format!("receita-{a}.json")), &json!(d.receita.iter().filter(|(_, v)| *v != 0.0).map(|(n, v)| json!([n, r2(*v)])).collect::<Vec<_>>()))?;
        let fs: Vec<&Acao> = d.acoes.iter().filter(|x| eh_festa(&x.acao, extra.as_ref()) && (x.orcado != 0.0 || x.liquidado != 0.0)).collect();
        cods.insert(a, fs.iter().map(|x| x.cod.clone()).collect());
        f.insert(a, (r2(fs.iter().map(|x| x.orcado).sum()), r2(fs.iter().map(|x| x.liquidado).sum())));
        ba.insert(a, r2(fs.iter().map(|x| x.atualizado).sum()));
        bn.insert(a, r2(d.acoes.iter().filter(|x| eh_beneficio(&x.acao)).map(|x| x.liquidado).sum()));
        if let Some((rx, _)) = &social {
            bs.insert(a, r2(d.acoes.iter().filter(|x| rx.is_match(&x.acao)).map(|x| x.liquidado).sum()));
        }
        let rot = if a == "2026" { format!("2026 até {mes}") } else { a.to_string() };
        let (arrec, gastou, emp) = anos[a];
        totais.push(json!({ "dito": format!("Arrecadou ({rot})"), "valor": arrec, "arquivo": d.doc_receita, "onde": format!("soma do arrecadado · {ate} (deduções já descontadas)") }));
        totais.push(json!({ "dito": format!("Gastou ({rot})"), "valor": gastou, "arquivo": d.doc_despesa, "onde": format!("soma do liquidado de {} ações · {ate}", d.acoes.len()) }));
        if a != "2026" {
            let dif = r2(d.arrecadado - d.liquidado);
            totais.push(json!({ "dito": format!("{} ({a})", if dif >= 0.0 { "Sobrou" } else { "Gastou a mais" }), "valor": dif.abs(), "arquivo": "conta própria", "onde": format!("conta própria: {} − {}", br(d.arrecadado), br(d.liquidado)) }));
        } else {
            totais.push(json!({ "dito": format!("Já empenhou ({rot})"), "valor": emp, "arquivo": d.doc_despesa, "onde": format!("soma do empenhado líquido · {ate}") }));
        }
        for x in fs {
            if !op.sem_previsto {
                totais.push(json!({ "dito": format!("Festas previa ({a})"), "valor": r2(x.orcado), "arquivo": d.doc_despesa, "onde": format!("ação {} ({}) · orçado (dotação inicial)", x.acao, x.cod) }));
            }
            let onde = if op.sem_previsto { format!("ação {} ({}) · liquidado", x.acao, x.cod) } else { "mesma ação · liquidado".to_string() };
            totais.push(json!({ "dito": format!("Festas gastou ({a})"), "valor": r2(x.liquidado), "arquivo": d.doc_despesa, "onde": onde }));
        }
    }
    // sem festas: o achado e a acao que gastou MAIS QUE O DOBRO do previsto na lei, presente nos 3 anos, previsto >= 500 mil e gasto >= 1 mi
    let mut dobrou: Option<Acao> = None;
    let mut be: BTreeMap<&str, f64> = BTreeMap::new();
    if !ausente && !ANOS.iter().any(|a| f[a].0 != 0.0 || f[a].1 != 0.0) {
        let generica = Regex::new(r"(?i)ADMINISTRA[ÇC][ÃA]O GERAL|DEMAIS SUBFUN|SEM NOME").expect("regex fixa");
        let candidatos = |por: &BTreeMap<&str, BTreeMap<String, &Acao>>| -> Result<Vec<(f64, String)>, String> {
            let mut out = vec![];
            for (c, x) in &por["2025"] {
                let func = funcao(x)?;
                if x.orcado >= 5e5 && x.liquidado >= 1e6 && x.liquidado >= 2.0 * x.orcado && func != 6 && func != 0 && x.sub.trim_start_matches('0') != "122" && !generica.is_match(&x.acao) && ANOS.iter().all(|a| por[a].contains_key(c)) {
                    out.push((x.liquidado / x.orcado, c.clone()));
                }
            }
            Ok(out)
        };
        let mut por: BTreeMap<&str, BTreeMap<String, &Acao>> = ANOS.iter().map(|a| (*a, dados[*a].acoes.iter().map(|x| (x.cod.clone(), x)).collect())).collect();
        let mut cand = candidatos(&por)?;
        if cand.is_empty() {
            // o codigo mudou entre os anos: a mesma acao pelo NOME; nome repetido no ano nao casa
            let chave = |s: &str| sem_acento(s).to_uppercase().chars().filter(char::is_ascii_alphanumeric).collect::<String>();
            por = ANOS
                .iter()
                .map(|a| {
                    let mut nomes: BTreeMap<String, Vec<&Acao>> = BTreeMap::new();
                    dados[*a].acoes.iter().for_each(|x| nomes.entry(chave(&x.acao)).or_default().push(x));
                    (*a, nomes.into_iter().filter(|(_, xs)| xs.len() == 1).map(|(n, xs)| (n, xs[0])).collect())
                })
                .collect();
            cand = candidatos(&por)?;
        }
        if op.sem_previsto || cand.is_empty() {
            return Err("sem festas e sem ação que passou do dobro do previsto: achado à mão (pauta do guia)".into());
        }
        let c = cand.into_iter().fold(None, |m: Option<(f64, String)>, x| if m.as_ref().is_none_or(|m| (x.0, &x.1) > (m.0, &m.1)) { Some(x) } else { m }).map(|x| x.1).unwrap_or_default();
        dobrou = Some(por["2025"][&c].clone());
        for a in ANOS {
            let x = por[a][&c];
            f.insert(a, (r2(x.orcado), r2(x.liquidado)));
            ba.insert(a, r2(x.atualizado));
            be.insert(a, r2(x.empenhado));
            cods.insert(a, vec![]);
            totais.push(json!({ "dito": format!("Achado previa ({a})"), "valor": f[a].0, "arquivo": doc(a).0, "onde": format!("{} ({}) · orçado", x.acao, x.cod) }));
            totais.push(json!({ "dito": format!("Achado gastou ({a})"), "valor": f[a].1, "arquivo": doc(a).0, "onde": "mesma linha · liquidado" }));
        }
    }
    let usa_benef = ANOS.iter().all(|a| bn[a] >= 1000.0);
    // um ano zerado viraria "0 reais" na fala: nunca dizer zero
    let usa_social = !usa_benef && social.is_some() && ANOS.iter().all(|a| bs.get(a).copied().unwrap_or(0.0) >= 1000.0);
    if op.sem_previsto && !usa_benef && !usa_social && !ausente {
        return Err("sem orçado por ação e sem benefícios eventuais no tribunal: o achado precisa ser escolhido à mão (pauta do guia)".into());
    }
    grava(&saida.join("festas-cods.json"), &json!(cods))?;
    let maior = anos.values().map(|v| v.0).fold(0.0, f64::max);
    let pilha = redondo(maior / 1e6, 45.0, &[1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0]);
    let topo = f.values().map(|v| v.0.max(v.1)).fold(0.0, f64::max);
    let gav = redondo(if topo == 0.0 { 1.0 } else { topo } / 1e6, 15.0, &[0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0]);
    let est = primeiro_max(&|a| if f[a].0 != 0.0 { f[a].1 - f[a].0 } else { -1e18 });
    let capa_previa = |a: &str| json!(["Festas:", format!("previa {}", capa(f[a].0)), "gastou", format!("{}.", capa(f[a].1))]);
    let ufm = uf.to_lowercase();
    let mut cid = json!({
        "numero": numero, "nome": nome, "uf": uf, "codigo": format!("orcamento{sg}{ufm}"), "comp": format!("Orcamento{}{}", maiuscula(&sg), maiuscula(uf)),
        "kit_pasta": format!("orcamento-{sg}-{ufm}"), "arquivo_mapa": format!("{sg}.mp4"), "ibge": ibge, "sigla": format!("{}{numero}", sg.chars().take(3).collect::<String>().to_uppercase()),
        "mes26": ABR[m26 - 1], "mes26_nome": NM[m26 - 1], "pilha_mi": n_json(pilha), "pilha_gaveta_mi": n_json(gav), "fonte": "tribunal",
        "anos": anos.iter().map(|(a, (ar, g, e))| (a.to_string(), json!({ "arrec": ar, "gastou": g, "emp": e }))).collect::<serde_json::Map<_, _>>(),
        "festas": par_json(&f), "festas_fonte": {}, "benef": mapa_json(if usa_benef { &bn } else { &ba }), "tribunal": tribunal, "setor_min_frac": 0.005,
        "despesa_fonte": ANOS.iter().map(|a| (a.to_string(), json!({ "arquivo": doc(a).0, "leitor": format!("tribunais/{ufm}.py"), "conferido": "soma das ações = total" }))).collect::<serde_json::Map<_, _>>(),
        "capa": if op.sem_previsto { json!(["Festas:", format!("{}.", capa(f["2026"].1)), "Ajuda às famílias:", format!("{}.", capa(bn["2026"]))]) } else { capa_previa(est) },
        "sem_previsto": op.sem_previsto, "totais": totais, "ausentes": [], "meses": { "2024": "12", "2025": "12", "2026": mes26 }, "guia": [], "sem_balanco": [], "revisao_humana": [],
    });
    let arq_cid = saida.join("cidade.json");
    if let Some(aus) = op.ausente26 {
        // o achado e a propria ausencia (o gerador faz a fala da pauta; a gaveta de festas nao aparece)
        cid["ausentes"] = json!(["2026"]);
        cid["ausencia"] = json!({ "2026": { "dez": aus } });
        cid["capa"] = json!(["2026:", "contas", "não entregues", format!("ao {sigla_t}.")]);
        grava(&arq_cid, &cid)?;
        setores(&saida)?;
        return Ok(cid);
    }
    let estouro = primeiro_max(&|a| f[a].1 - f[a].0); // o ano em que as festas mais passaram do previsto
    let passou = f[estouro].1 > f[estouro].0;
    if !usa_benef && !passou {
        return Err("as festas não passaram do previsto em nenhum ano e não há benefícios eventuais nos 3 anos: achado à mão (pauta do guia)".into());
    }
    let yy = &estouro[2..];
    let achado_festas = |pergunta: String, chip: String| json!({ "fala": "festas e eventos", "curto": "as festas", "tela": "FESTAS E EVENTOS", "sub": "FESTAS", "verbo": "levaram", "pergunta": pergunta, "chip": chip });
    let pergunta_lei = format!("Mas se a lei do orçamento de {estouro} previa {{P{yy}}} para festas e eventos, por que as festas levaram {{G{yy}}}?");
    let chip_lei = format!("...E GASTOU {{G{yy}ROT}} EM {estouro}?");
    let gat_mudancas = json!({ "pub": "Com as mudancas no orcamento", "b24": "passou para {B24}", "b25": "{B25} em 2025", "pubg": "e {B26} em 2026", "porque": "Mas se a lei" });
    if !usa_benef {
        // sem beneficios eventuais no tribunal: o 2o par e o previsto atualizado das mesmas festas (nada inventado)
        cid["par2"] = json!({ "fala": "Com as mudanças no orçamento ao longo do ano, o previsto das festas passou para {B24} em 2024, {B25} em 2025 e {B26} em 2026.",
            "gat": gat_mudancas, "tela": "PREVISTO DEPOIS DAS MUDANÇAS NO ORÇAMENTO", "curta": "PREVISTO ATUALIZADO", "aj": "ATUALIZADO", "porque_chip": format!("PREVIA NA LEI EM {estouro}: {{P{yy}ROT}}") });
        cid["achado"] = achado_festas(pergunta_lei.clone(), chip_lei.clone());
        cid["capa"] = capa_previa(estouro);
    }
    if let Some(d) = &dobrou {
        // achado do guia (mais que o dobro do previsto): textos com o nome real da acao
        let nome_a = Regex::new(r"\s*·\s*").expect("regex fixa").replace_all(d.acao.trim(), ", ").into_owned(); // "·" quebra a fala
        let toda_maiuscula = nome_a.chars().any(char::is_uppercase) && !nome_a.chars().any(char::is_lowercase);
        let fala_a = if toda_maiuscula { nome_a.to_lowercase() } else { let mut c = nome_a.chars(); c.next().map(|p| p.to_lowercase().chain(c).collect()).unwrap_or_default() };
        let alto = nome_a.to_uppercase();
        cid["achado"] = json!({ "fala": fala_a, "curto": "essa ação", "tela": alto.chars().take(60).collect::<String>(), "sub": alto.chars().take(22).collect::<String>(), "verbo": "levou", "prefixo": "A ação de", "guia": "dobrou",
            "pergunta": "Mas se a lei do orçamento de 2025 previa {P25} para essa ação, por que essa ação levou {G25}?", "chip": "...E GASTOU {G25ROT} EM 2025?" });
        cid["capa"] = json!([format!("{}:", nome_a.chars().take(20).collect::<String>()), format!("previa {}", capa(f["2025"].0)), "gastou", format!("{}.", capa(f["2025"].1))]);
        if !usa_benef {
            let usa_atual = ANOS.iter().all(|a| ba[a] >= 1000.0) && ANOS.iter().any(|a| ba[a] != f[a].0);
            cid["benef"] = mapa_json(if usa_atual { &ba } else { &be });
            if usa_atual {
                let p = &mut cid["par2"];
                p["fala"] = json!("Com as mudanças no orçamento ao longo do ano, o previsto dessa ação passou para {B24} em 2024, {B25} em 2025 e {B26} em 2026.");
                p["legenda"] = json!("📝 Com as mudanças no orçamento, o previsto dessa ação passou para {B24} em 2024, {B25} em 2025 e {B26} em 2026.");
                p["porque_chip"] = json!("PREVIA NA LEI EM 2025: {P25ROT}");
            } else {
                cid["par2"] = json!({ "fala": "O empenhado dessa ação, o dinheiro já reservado com fornecedor, foi {B24} em 2024, {B25} em 2025 e {B26} em 2026.",
                    "gat": { "pub": "O empenhado dessa acao", "b24": "foi {B24} em 2024", "b25": "{B25} em 2025", "pubg": "e {B26} em 2026", "porque": "Mas se a lei" },
                    "tela": "EMPENHADO DA AÇÃO (RESERVADO COM FORNECEDOR)", "curta": "EMPENHADO", "aj": "EMPENHOU", "porque_chip": "PREVIA NA LEI EM 2025: {P25ROT}",
                    "legenda": "📝 O empenhado dessa ação (reservado com fornecedor) foi {B24} em 2024, {B25} em 2025 e {B26} em 2026." });
            }
        }
    }
    if usa_social {
        // 2o par = a acao social com o nome real dela (nunca chamada de "beneficios eventuais")
        let nm = social.as_ref().map(|s| s.1).unwrap_or("");
        let (mesn, abr, alto) = (NM[m26 - 1], ABR[m26 - 1], nm.to_uppercase());
        cid["benef"] = mapa_json(&bs);
        cid["par2"] = json!({ "fala": format!("A ação de {nm}, da assistência social, ficou em {{B24}} em 2024, {{B25}} em 2025 e {{B26}} até {mesn} de 2026."),
            "gat": { "pub": format!("A acao de {}", nm.split_whitespace().next().unwrap_or("")), "b24": "ficou em {B24} em 2024", "b25": "{B25} em 2025", "pubg": "e {B26} ate", "porque": "Mas se a acao" },
            "tela": format!("AÇÃO DE {alto} (ASSISTÊNCIA SOCIAL)"), "curta": alto, "aj": "SOCIAL", "porque_chip": format!("{alto} ATÉ {}/2026: {{B26ROT}}", abr.to_uppercase()),
            "legenda": format!("🤝 Ação de {nm} (assistência social): {{B24}} em 2024, {{B25}} em 2025 e {{B26}} até {abr}/2026.") });
        cid["capa"] = json!(["Festas:", format!("{}.", capa(f["2026"].1)), format!("{}:", maiuscula(nm)), format!("{}.", capa(bs["2026"]))]);
        cid["achado"] = achado_festas(format!("Mas se a ação de {nm} ficou em {{B26}} até {mesn}, por que as festas já levaram {{G26}}?"), "...E AS FESTAS: {G26ROT}?".into());
    }
    // "se a ajuda ficou em X, por que as festas levaram Y" so faz sentido se as festas levaram MAIS; senao vai para o ano em que
    // as festas mais passaram do previsto
    let b26 = cid["benef"]["2026"].as_f64().unwrap_or(0.0);
    if dobrou.is_none() && f["2026"].1 <= b26 {
        if op.sem_previsto || !passou {
            return Err("as festas gastaram menos que a ajuda às famílias e não passaram do previsto: achado à mão (pauta do guia)".into());
        }
        if cid["achado"].is_null() {
            cid["achado"] = achado_festas(String::new(), String::new());
        }
        cid["achado"]["pergunta"] = json!(pergunta_lei);
        cid["achado"]["chip"] = json!(chip_lei);
        if cid["par2"].is_null() {
            // 2o par padrao (beneficios eventuais) escrito aqui so para o gatilho da pergunta nova casar
            cid["par2"] = json!({ "fala": format!("Os benefícios eventuais, a ajuda às famílias em necessidade, ficaram em {{B24}} em 2024, {{B25}} em 2025 e {{B26}} até {} de 2026.", NM[m26 - 1]),
                "gat": { "pub": "Os beneficios eventuais", "b24": "ficaram em {B24}", "b25": "{B25} em 2025", "pubg": "e {B26} ate" },
                "tela": "BENEFÍCIOS EVENTUAIS: AJUDA ÀS FAMÍLIAS", "curta": "AJUDA ÀS FAMÍLIAS", "aj": "AJUDA",
                "legenda": format!("🤝 Benefícios eventuais (ajuda às famílias em necessidade): {{B24}} em 2024, {{B25}} em 2025 e {{B26}} até {}/2026.", ABR[m26 - 1]) });
        }
        cid["par2"]["porque_chip"] = json!(format!("PREVIA NA LEI EM {estouro}: {{P{yy}ROT}}"));
        cid["par2"]["gat"]["porque"] = json!("Mas se a lei");
        cid["capa"] = capa_previa(estouro);
    }
    grava(&arq_cid, &cid)?;
    setores(&saida)?;
    if dobrou.is_none() {
        // a fala de "festas e eventos" no ano e no achado tem de ser o mesmo numero
        let st: Value = std::fs::read_to_string(saida.join("setores.json")).map_err(|e| format!("setores.json: {e}")).and_then(|t| serde_json::from_str(&t).map_err(|e| format!("setores.json: {e}")))?;
        for a in ANOS {
            let liq = st.as_object().and_then(|o| o.iter().find(|(k, _)| k.starts_with(a))).and_then(|(_, v)| v["Festas e eventos"]["liquidado"].as_f64());
            if !liq.is_some_and(|l| (l - f[a].1).abs() < 1.0) {
                return Err(format!("{a}: setor festas != achado"));
            }
        }
    }
    Ok(cid)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pasta(nome: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("braniac-link_tribunais_base-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn t() -> Value {
        json!({ "sigla": "TCE-XX", "fala": "Tribunal de Contas do Estado", "uf": "XX", "estado": "Teste" })
    }

    fn acao(cod: &str, nome: &str, orcado: f64, liq: f64, f: &str, sub: &str) -> Acao {
        Acao { f: f.into(), sub: sub.into(), cod: format!("0.0.{f}.{sub}.0.{cod}"), acao: nome.into(), orcado, atualizado: orcado, empenhado: liq, liquidado: liq }
    }

    fn ano(acoes: Vec<Acao>) -> Ano {
        let (liq, emp) = (acoes.iter().map(|x| x.liquidado).sum(), acoes.iter().map(|x| x.empenhado).sum());
        Ano { acoes, receita: vec![("Impostos, taxas e contribuições de melhoria".into(), 9e7)], arrecadado: 9e7, liquidado: liq, empenhado: emp, doc_despesa: "painel".into(), doc_receita: "painel".into() }
    }

    fn escola(c: &str) -> Acao {
        acao(c, "MANUTENÇÃO DAS ESCOLAS", 4e7, 4e7, "12", "361")
    }

    fn sem_setores(_: &Path) -> Result<(), String> {
        Ok(())
    }

    /// Faz o papel do script de setores: festas = soma do liquidado das acoes de festa de cada arquivo acoes-*.json.
    fn setores_falso(saida: &Path) -> Result<(), String> {
        let mut st = serde_json::Map::new();
        for e in std::fs::read_dir(saida).unwrap().flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if let Some(k) = n.strip_prefix("acoes-").and_then(|k| k.strip_suffix(".json")) {
                let v: Value = serde_json::from_str(&std::fs::read_to_string(e.path()).unwrap()).unwrap();
                let s: f64 = v.as_array().unwrap().iter().filter(|x| eh_festa(x["acao"].as_str().unwrap(), None)).map(|x| x["liquidado"].as_f64().unwrap()).sum();
                st.insert(k.into(), json!({ "Festas e eventos": { "liquidado": s } }));
            }
        }
        std::fs::write(saida.join("setores.json"), Value::Object(st).to_string()).map_err(|e| e.to_string())
    }

    fn com_festas(prev: f64, gasto: [f64; 3], benef: f64) -> Dados {
        ANOS.iter().zip(gasto).map(|(a, g)| (a.to_string(), ano(vec![escola("1"), acao("2", "FESTEJOS POPULARES", prev, g, "13", "392"), acao("3", "BENEFÍCIOS EVENTUAIS", benef, benef, "8", "244")]))).collect()
    }

    #[test]
    fn regras_de_festa_e_formatos() {
        for sim in ["Carnaval 2026", "APOIO A FESTAS", "festa do padroeiro", "SÃO JOAO", "Comemoração das Festividades", "SHOW DA VIRADA", "EVENTOS TRADICIONAIS"] {
            assert!(eh_festa(sim, None), "{sim}");
        }
        for nao in ["MANIFESTAR APOIO", "PARA COMEMORAÇÃO DE FESTIVIDADES", "para comemoracao festividades", "SHOWROOM", "MANUTENÇÃO DAS ESCOLAS"] {
            assert!(!eh_festa(nao, None), "{nao}");
        }
        let extra = Regex::new("(?i)^APOIO E REALIZACAO DE EVENTOS$").unwrap();
        assert!(eh_festa("apoio e realizacao de eventos", Some(&extra)) && !eh_festa("APOIO E REALIZACAO DE EVENTOS ESPORTIVOS", Some(&extra)));
        assert!(eh_beneficio("Benefício Eventual") && eh_beneficio("BENEFICIOS EVENTUAIS") && !eh_beneficio("BENEFÍCIOS PREVIDENCIÁRIOS"));
        assert_eq!((capa(1.5e9), capa(2e6), capa(3500.0)), ("R$ 1,50 bi".into(), "R$ 2,00 mi".into(), "R$ 3,5 mil".into()));
        assert_eq!((r2(1.115), r2(2.675), r2(0.125)), (1.11, 2.67, 0.12)); // como o round() do Python
        assert_eq!((slug("São João d'Aliança"), maiuscula("ce")), ("saojoaodalianca".into(), "Ce".into()));
        assert_eq!(redondo(90.0, 45.0, &[1.0, 2.0, 5.0]), 2.0);
        let l = csv("\u{feff}A;B;C\r\n1;\"x; \"\"y\"\"\nz\";\r\n\r\n2;w;3.5", ';');
        assert_eq!(l.len(), 2);
        assert_eq!((l[0]["A"].as_str(), l[0]["B"].as_str(), l[0]["C"].as_str()), ("1", "x; \"y\"\nz", ""));
        assert_eq!((valor(&l[1], "C"), valor(&l[0], "C")), (Ok(3.5), Ok(0.0)));
        assert!(valor(&l[1], "B").is_err() && col(&l[0], "D").unwrap_err().contains("sem a coluna D") && csv("", ',').is_empty());
    }

    #[test]
    fn ano_2026_nao_entregue_vira_pauta() {
        let p = pasta("pauta");
        std::fs::create_dir_all(p.join("saida")).unwrap();
        std::fs::write(p.join("saida/acoes-2026-05.json"), "[]").unwrap(); // sobra de rodada antiga
        std::fs::write(p.join("saida/receita-2026.json"), "[]").unwrap();
        let e = acao("1", "MANUTENÇÃO DAS ESCOLAS", 0.0, 5e7, "12", "361");
        let dados: Dados = [("2024".to_string(), ano(vec![e.clone()])), ("2025".to_string(), ano(vec![e]))].into(); // 2026 nem existe: nao pode ser lido
        let op = Opcoes { sem_previsto: true, ausente26: Some("nada da Prefeitura"), ..Default::default() };
        let c = montar(&p, "9999", "Cidade Teste", "XX", 1, &t(), &dados, "01", &op, &sem_setores).unwrap();
        assert!(c["ausentes"] == json!(["2026"]) && c["ausencia"]["2026"]["dez"] == "nada da Prefeitura");
        assert_eq!(c["anos"]["2026"], json!({ "arrec": 0.0, "gastou": 0.0, "emp": 0.0 }));
        assert_eq!(c["capa"][3], "ao TCE-XX.");
        assert!(!p.join("saida/acoes-2026-05.json").exists() && !p.join("saida/receita-2026.json").exists() && p.join("saida/acoes-2025-12.json").exists());
        assert!(c["codigo"] == "orcamentocidadetestexx" && c["comp"] == "OrcamentoCidadetesteXx" && c["sigla"] == "CID9999");
        // sem o aviso de ausencia, ano que o molde nao leu e erro (nunca se completa com outra fonte)
        assert!(montar(&p, "9999", "Cidade Teste", "XX", 1, &t(), &dados, "01", &Opcoes::default(), &sem_setores).unwrap_err().contains("2026: o molde não leu"));
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn achado_dobrou_casa_pelo_nome() {
        let p = pasta("dobrou");
        let dados: Dados = [
            ("2024".to_string(), ano(vec![escola("20.12.361"), acao("51.13.392", "OPERACIONALIZACAODOFMIC", 0.0, 6.9e6, "13", "392")])), // nome sem espaco no sistema antigo
            ("2025".to_string(), ano(vec![escola("2020"), acao("4051", "OPERACIONALIZAÇÃO DO FMIC", 4.36e6, 9.28e6, "13", "392")])),
            ("2026".to_string(), ano(vec![escola("2002"), acao("4066", "OPERACIONALIZAÇÃO DO FMIC", 10.1e6, 1.8e5, "13", "392")])),
        ]
        .into();
        let c = montar(&p, "9998", "Cidade Teste", "XX", 1, &t(), &dados, "09", &Opcoes::default(), &sem_setores).unwrap();
        assert_eq!(c["achado"]["guia"], "dobrou");
        assert_eq!(c["achado"]["fala"], "operacionalização do fmic");
        assert_eq!(c["festas"]["2025"], json!({ "prev": 4.36e6, "gasto": 9.28e6 }));
        assert!(c["festas"]["2024"]["gasto"] == 6.9e6 && c["festas"]["2024"]["prev"] == 0.0); // 2024 sem orcado no tribunal
        assert_eq!(c["par2"]["curta"], "EMPENHADO"); // o previsto atualizado e igual ao da lei: fala o empenhado
        assert_eq!(c["benef"]["2025"], 9.28e6);
        let gravado: Value = serde_json::from_str(&std::fs::read_to_string(p.join("saida/cidade.json")).unwrap()).unwrap();
        assert_eq!(gravado, c);
        // tribunal sem orcado por acao nao pode ter achado "dobrou"
        let op = Opcoes { sem_previsto: true, ..Default::default() };
        assert!(montar(&p, "9998", "Cidade Teste", "XX", 1, &t(), &dados, "09", &op, &sem_setores).unwrap_err().contains("achado à mão"));
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn festas_com_beneficios_e_as_travas() {
        let p = pasta("festas");
        let dados = com_festas(1e6, [2e6, 3e6, 2.5e6], 5e5);
        let c = montar(&p, "9001", "Cidade Teste", "XX", 1234567, &t(), &dados, "08", &Opcoes::default(), &setores_falso).unwrap();
        assert_eq!(c["festas"]["2025"], json!({ "prev": 1e6, "gasto": 3e6 }));
        assert!(c["benef"]["2026"] == 5e5 && c["par2"].is_null() && c["achado"].is_null()); // caso padrao: o gerador faz os textos
        assert_eq!(c["capa"], json!(["Festas:", "previa R$ 1,00 mi", "gastou", "R$ 3,00 mi."])); // o ano que mais passou do previsto
        assert!(c["mes26"] == "ago" && c["mes26_nome"] == "agosto" && c["meses"]["2026"] == "08" && c["pilha_mi"] == 2 && c["pilha_gaveta_mi"] == 0.2);
        let sobrou = c["totais"].as_array().unwrap().iter().find(|x| x["dito"] == "Sobrou (2024)").unwrap().clone();
        assert!(sobrou["valor"] == 4.75e7 && sobrou["onde"] == "conta própria: 90.000.000,00 − 42.500.000,00");
        assert!(c["totais"].as_array().unwrap().iter().any(|x| x["dito"] == "Festas previa (2026)" && x["valor"] == 1e6));
        let cods: Value = serde_json::from_str(&std::fs::read_to_string(p.join("saida/festas-cods.json")).unwrap()).unwrap();
        assert_eq!(cods["2025"], json!(["0.0.13.392.0.2"]));
        let acs: Value = serde_json::from_str(&std::fs::read_to_string(p.join("saida/acoes-2026-08.json")).unwrap()).unwrap();
        assert!(acs[2]["f"] == "8" && acs[0]["pag"] == 0);

        // soma das acoes diferente do total do tribunal: para
        let mut ruim = dados.clone();
        ruim.get_mut("2025").unwrap().liquidado += 10.0;
        assert!(montar(&p, "9001", "Cidade Teste", "XX", 1, &t(), &ruim, "08", &Opcoes::default(), &setores_falso).unwrap_err().contains("2025: soma das ações"));
        let mut ruim = dados.clone();
        ruim.get_mut("2024").unwrap().arrecadado -= 5.0;
        assert!(montar(&p, "9001", "Cidade Teste", "XX", 1, &t(), &ruim, "08", &Opcoes::default(), &setores_falso).unwrap_err().contains("2024: origens da receita"));
        // setor de festas diferente do achado: para
        let outro = |s: &Path| std::fs::write(s.join("setores.json"), r#"{"2024-12":{"Festas e eventos":{"liquidado":1}},"2025-12":{"Festas e eventos":{"liquidado":1}},"2026-08":{"Festas e eventos":{"liquidado":1}}}"#).map_err(|e| e.to_string());
        assert_eq!(montar(&p, "9001", "Cidade Teste", "XX", 1, &t(), &dados, "08", &Opcoes::default(), &outro).unwrap_err(), "2024: setor festas != achado");
        assert!(montar(&p, "9001", "Cidade Teste", "XX", 1, &t(), &dados, "08", &Opcoes::default(), &|_| Err("script de setores falhou".into())).is_err());
        assert!(montar(&p, "9001", "Cidade Teste", "XX", 1, &t(), &dados, "13", &Opcoes::default(), &setores_falso).unwrap_err().contains("mês de corte"));
        // festas nao passaram do previsto e levaram menos que a ajuda: achado a mao
        let calmo = com_festas(1e6, [5e5, 5e5, 4e5], 5e5);
        assert!(montar(&p, "9001", "Cidade Teste", "XX", 1, &t(), &calmo, "08", &Opcoes::default(), &setores_falso).unwrap_err().contains("gastaram menos que a ajuda"));
        // sem beneficios e sem estouro
        let sem = com_festas(1e6, [5e5, 5e5, 4e5], 0.0);
        assert!(montar(&p, "9001", "Cidade Teste", "XX", 1, &t(), &sem, "08", &Opcoes::default(), &setores_falso).unwrap_err().contains("não passaram do previsto em nenhum ano"));
        // tribunal sem orcado por acao e sem beneficios: nada e estimado
        let op = Opcoes { sem_previsto: true, ..Default::default() };
        assert!(montar(&p, "9001", "Cidade Teste", "XX", 1, &t(), &sem, "08", &op, &setores_falso).unwrap_err().contains("sem orçado por ação"));
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn segundo_par_sem_beneficios_e_pergunta_do_ano_que_estourou() {
        let p = pasta("par2");
        // sem beneficios eventuais: o 2o par e o previsto atualizado das festas
        let c = montar(&p, "9002", "Cidade Teste", "XX", 1, &t(), &com_festas(1e6, [2e6, 3e6, 2.5e6], 0.0), "08", &Opcoes::default(), &setores_falso).unwrap();
        assert!(c["par2"]["curta"] == "PREVISTO ATUALIZADO" && c["par2"]["porque_chip"] == "PREVIA NA LEI EM 2025: {P25ROT}");
        assert_eq!(c["achado"]["pergunta"], "Mas se a lei do orçamento de 2025 previa {P25} para festas e eventos, por que as festas levaram {G25}?");
        assert_eq!(c["benef"]["2026"], 1e6);
        // beneficios maiores que as festas de 2026: a pergunta vai para o ano em que as festas mais passaram do previsto
        let c = montar(&p, "9002", "Cidade Teste", "XX", 1, &t(), &com_festas(1e6, [4e6, 2e6, 1.5e6], 3e6), "08", &Opcoes::default(), &setores_falso).unwrap();
        assert!(c["achado"]["chip"] == "...E GASTOU {G24ROT} EM 2024?" && c["par2"]["aj"] == "AJUDA" && c["par2"]["gat"]["porque"] == "Mas se a lei");
        assert!(c["par2"]["fala"].as_str().unwrap().contains("até agosto de 2026"));
        // sem orcado por acao (tribunal nao publica): capa com festas x ajuda
        let op = Opcoes { sem_previsto: true, ..Default::default() };
        let c = montar(&p, "9002", "Cidade Teste", "XX", 1, &t(), &com_festas(0.0, [2e6, 3e6, 2.5e6], 5e5), "08", &op, &setores_falso).unwrap();
        assert_eq!(c["capa"], json!(["Festas:", "R$ 2,50 mi.", "Ajuda às famílias:", "R$ 500,0 mil."]));
        assert!(c["sem_previsto"] == true && !c["totais"].as_array().unwrap().iter().any(|x| x["dito"].as_str().unwrap().starts_with("Festas previa")));
        // acao social com o nome real, quando nao ha beneficios eventuais
        let mut d = com_festas(1e6, [2e6, 3e6, 2.5e6], 0.0);
        for a in d.values_mut() {
            a.acoes.push(acao("9", "PROGRAMA RENDA E CIDADANIA", 2e5, 2e5, "8", "244"));
            a.liquidado += 2e5;
            a.empenhado += 2e5;
        }
        let op = Opcoes { social: Some(("RENDA E CIDADANIA", "renda e cidadania")), ..Default::default() };
        let c = montar(&p, "9002", "Cidade Teste", "XX", 1, &t(), &d, "08", &op, &setores_falso).unwrap();
        assert!(c["par2"]["curta"] == "RENDA E CIDADANIA" && c["benef"]["2026"] == 2e5 && c["capa"][2] == "Renda e cidadania:");
        assert!(c["achado"]["pergunta"].as_str().unwrap().contains("renda e cidadania ficou em {B26} até agosto"));
        let _ = std::fs::remove_dir_all(&p);
    }
}
