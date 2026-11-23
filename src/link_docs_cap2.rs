//! Adaptadores por capital dos documentos do tribunal, 2a parte (continua `link_docs_cap`, que ja passava de 600 linhas):
//! `to`, `rs`, `sc`, `ac`, `am`, `ms`, `pb`, `pe`, `pi` e `pr` da pasta link/docs_cap/ do Python. Cada um transforma o que o
//! leitor guardou do TRIBUNAL em tabela de texto (documento copiado ou recortado como veio), poe o codigo da acao do video numa
//! coluna propria, montado com as colunas da propria linha, e chama o motor (`link_docs_capitais::montar`), que tira nome de
//! eleito e CPF. Coluna que falta, linha fora do formato, arquivo que o leitor nao guardou = Err; nada e completado.
//! Entra por parametro: os arquivos de trabalho do leitor (`Fonte`: quem chama diz como ler e listar; `da_pasta` le do disco),
//! o texto e as palavras dos PDF (`pi`, `pe`), a consulta ao painel (`ac`) e tudo que no Python estava escrito a mao para a
//! entrega de 16/11: mes de corte de 2026 (`to`, `rs`, `sc`), remessas Consolidadas e CNPJ (`to::Corte`), anos com remessa
//! (`ac`), processo do e-TCEPE (`pe`), dia da consulta das liquidacoes (`pb`). Quem chama passa o que o molde do tribunal descobriu.
//! `conferir` (am, pe, pr) devolve o relatorio do Python quando toda acao bate com o arquivo; acao que nao bate = Err com o relatorio.
//! Diferencas do Python: `rs` conta os orgaos com arquivo em vez de dizer "8 orgaos" e "a Camara nao tem receita"; `pb` diz as
//! datas pelo mes de corte (la "30/09/2026" e "outubro e novembro" eram fixos); `ac` nao guarda a consulta em disco; colunas de
//! JSON em ordem alfabetica (como no motor); `am` e `pr` param se faltar unidade gestora ou arquivo de liquidacao (la saia vazio).
//! Ficou de fora: o WebSocket do Qlik do `ac` (closure), ler PDF (closures) e o envio ao site.
#![allow(dead_code)]
use crate::link_docs_cap::{cod_texto, colunas, com_coluna, es, filtros, inteiro, js, lista, mes_de, posicoes, sem_zero, tabela, Onde, ANOS};
use crate::link_docs_capitais::{self as d, Adaptador, Arq};
use crate::link_tribunais_base::{ABR, NM};
use crate::link_tribunais_util::{self as u, pystr};
use crate::texto_pt::sem_acento;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Os arquivos de trabalho que o leitor guardou do tribunal (no Python, a pasta tce-xx do pacote). Nome relativo, com "/".
pub struct Fonte<'a> {
    /// O conteudo de um arquivo; None = o leitor nao guardou.
    pub le: &'a dyn Fn(&str) -> Result<Option<Vec<u8>>, String>,
    /// Todos os nomes (para o que o Python achava por glob).
    pub nomes: &'a dyn Fn() -> Result<Vec<String>, String>,
}

fn utf8(b: Vec<u8>, nome: &str) -> Result<String, String> {
    String::from_utf8(b).map(|t| t.trim_start_matches('\u{feff}').to_string()).map_err(|_| format!("{nome}: não está em UTF-8"))
}

impl Fonte<'_> {
    pub fn bytes(&self, nome: &str) -> Result<Vec<u8>, String> {
        (self.le)(nome)?.ok_or_else(|| format!("{nome}: o leitor não guardou este arquivo do tribunal"))
    }
    pub fn texto(&self, nome: &str) -> Result<String, String> {
        utf8(self.bytes(nome)?, nome)
    }
    pub fn json(&self, nome: &str) -> Result<Value, String> {
        serde_json::from_str(&self.texto(nome)?).map_err(|e| format!("{nome}: {e}"))
    }
    /// Lista de registros de um arquivo que pode nao existir (consulta que o tribunal respondeu vazia nao e guardada).
    fn lista_ou_vazia(&self, nome: &str) -> Result<Vec<Value>, String> {
        match (self.le)(nome)? {
            None => Ok(vec![]),
            Some(b) => serde_json::from_str::<Value>(&utf8(b, nome)?).map_err(|e| format!("{nome}: {e}")).and_then(|v| lista(&v, nome).cloned()),
        }
    }
    /// Copia o arquivo do tribunal como veio.
    fn copia(&self, nome: &str, pasta: &Path, destino: &str) -> Result<PathBuf, String> {
        escreve(pasta.join(destino), &self.bytes(nome)?)
    }
}

/// A `Fonte` de uma pasta do disco: (le, nomes).
pub fn da_pasta(dir: &Path) -> (impl Fn(&str) -> Result<Option<Vec<u8>>, String> + '_, impl Fn() -> Result<Vec<String>, String> + '_) {
    fn anda(base: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), String> {
        for e in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?.flatten() {
            let p = e.path();
            if p.is_dir() {
                anda(base, &p, out)?;
            } else if let Ok(r) = p.strip_prefix(base) {
                out.push(r.to_string_lossy().replace('\\', "/"));
            }
        }
        Ok(())
    }
    let le = move |nome: &str| {
        let p = dir.join(nome);
        if p.is_file() { std::fs::read(&p).map(Some).map_err(|e| format!("{}: {e}", p.display())) } else { Ok(None) }
    };
    let nomes = move || {
        let mut out = vec![];
        anda(dir, dir, &mut out)?;
        out.sort();
        Ok(out)
    };
    (le, nomes)
}

fn escreve(p: PathBuf, b: &[u8]) -> Result<PathBuf, String> {
    std::fs::write(&p, b).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok(p)
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

fn cels(r: &[Value]) -> Vec<String> {
    r.iter().map(d::cel_json).collect()
}

fn nome_de(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// O mes de corte "MM" como numero; fora de 01..12 e erro.
fn mes_valido(m: &str) -> Result<u32, String> {
    m.parse().ok().filter(|n| m.len() == 2 && (1..=12).contains(n)).ok_or_else(|| format!("mês de corte inválido {m:?}"))
}

/// O ultimo dia do mes, "dd/mm/aaaa".
fn ultimo_dia(ano: &str, mes: u32) -> Result<String, String> {
    let a: u32 = ano.parse().map_err(|_| format!("ano inválido {ano:?}"))?;
    let dia = match mes {
        2 if a % 4 == 0 && (a % 100 != 0 || a % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    Ok(format!("{dia:02}/{mes:02}/{ano}"))
}

/// O `int(v)` do Python para numero ou texto de numero.
fn int_py(v: &Value, o_que: &str) -> Result<i64, String> {
    v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)).or_else(|| v.as_str().and_then(|x| x.trim().parse().ok())).ok_or_else(|| format!("{o_que} que não é número: {v}"))
}

fn real(t: &str, o_que: &str) -> Result<f64, String> {
    t.trim().parse().map_err(|_| format!("{o_que}: valor que não é número: {t:?}"))
}

/// As linhas (listas) de uma consulta guardada; `n` > 0 = cada uma com exatamente `n` colunas.
fn linhas_de<'a>(v: &'a Value, o_que: &str, n: usize) -> Result<Vec<&'a Vec<Value>>, String> {
    lista(v, o_que)?.iter().map(|x| x.as_array().filter(|x| n == 0 || x.len() == n).ok_or_else(|| format!("{o_que}: linha fora do formato do tribunal"))).collect()
}

/// O campo de um registro; campo que nao veio e erro (nulo vira celula vazia).
fn campo(x: &Value, k: &str, o_que: &str) -> Result<String, String> {
    x.get(k).map(d::cel_json).ok_or_else(|| format!("{o_que}: registro sem {k}"))
}

/// As acoes do ultimo acoes-<ano>-MM.json do levantamento.
fn acoes(dados: &Path, ano: &str) -> Result<Vec<Value>, String> {
    let arq = d::acoes_do_ano(&dados.join("saida"), ano).ok_or_else(|| format!("{}: levantamento sem acoes-{ano}-MM.json", dados.display()))?;
    lista(&js(&arq)?, &nome_de(&arq)).cloned()
}

/// O mes de corte de 2026 do levantamento (do nome do ultimo acoes-2026-MM.json).
fn corte26(dados: &Path) -> Result<String, String> {
    let m = d::mes_extrato(dados);
    m.strip_prefix("2026-").filter(|m| mes_valido(m).is_ok()).map(String::from).ok_or_else(|| format!("{}: levantamento sem acoes-2026-MM.json", dados.display()))
}

/// O primeiro arquivo da pasta (ou de _grandes, com `fundo`) com o nome no padrao (so "*" como curinga), como o glob do Python.
fn acha(pasta: &Path, padrao: &str, fundo: bool) -> Result<PathBuf, String> {
    let re = Regex::new(&format!("^{}$", regex::escape(padrao).replace(r"\*", ".*"))).map_err(|e| e.to_string())?;
    let mut fs = vec![];
    for dir in [Some(pasta.to_path_buf()), fundo.then(|| pasta.join("_grandes"))].into_iter().flatten() {
        fs.extend(std::fs::read_dir(&dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_file() && re.is_match(&nome_de(p))));
    }
    fs.sort();
    fs.into_iter().next().ok_or_else(|| format!("{}: sem arquivo {padrao}", pasta.display()))
}

fn r2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// Soma de uma coluna (ja convertida por `val`) nas linhas `ns` (1 = cabecalho) da tabela.
fn soma(t: &[Vec<String>], ns: &[usize], c: usize, val: &dyn Fn(&str) -> Result<f64, String>) -> Result<f64, String> {
    ns.iter().map(|n| t.get(n - 1).and_then(|r| r.get(c)).ok_or_else(|| format!("linha {n} sem a coluna {}", c + 1)).and_then(|x| val(x))).sum()
}

fn liquidado(x: &Value) -> Result<(&str, f64), String> {
    Ok((x["cod"].as_str().ok_or("ação sem código no levantamento")?, x["liquidado"].as_f64().ok_or("ação sem liquidado no levantamento")?))
}

/// O relatorio do `conferir`: acao que nao bateu com o arquivo do tribunal = Err (com o relatorio inteiro).
fn conferido(uf: &str, ruins: usize, out: serde_json::Map<String, Value>) -> Result<Value, String> {
    if ruins == 0 { Ok(Value::Object(out)) } else { Err(format!("{uf}: {ruins} ação(ões) não bateram com os arquivos do tribunal: {}", Value::Object(out))) }
}

/// 975 Palmas (TCE-TO): planilhas do SICAP (HTML com extensao .xls, `<planilha>-<ano>-<remessa>-<CNPJ>.xls`) -> um .txt por
/// ano e tipo, com a unidade gestora na 1a coluna.
pub mod to {
    use super::*;

    const PLAN: &str = "https://www.tceto.tc.br/sicap/contabilx/relatorios/planilhas";

    /// O que o molde do tribunal descobriu (no Python estava escrito a mao): quem chama passa.
    pub struct Corte<'a> {
        /// Ultima remessa de 2026 que todas as unidades entregaram ("09").
        pub mes26: &'a str,
        /// CNPJ da unidade principal (a que manda a remessa Consolidada).
        pub principal: &'a str,
        /// ano -> numero da remessa Consolidada da Prefeitura que o leitor usou; 2026 sem entrada = remessa `mes26` de cada unidade.
        pub consolidada: &'a [(&'a str, i64)],
    }

    /// O SICAP marca texto com ' no fim e mistura UTF-8 lido como latin-1.
    fn fix(c: &str) -> String {
        let c = c.trim_end_matches('\'');
        let b: Option<Vec<u8>> = c.chars().map(|x| u8::try_from(x as u32).ok()).collect();
        b.and_then(|b| String::from_utf8(b).ok()).unwrap_or_else(|| c.to_string())
    }

    pub fn montar(o: &Onde, f: &Fonte, c: &Corte) -> Result<(PathBuf, Value), String> {
        let dados = o.pacote.join("RECEITAS-DESPESAS-PALMAS-TO");
        mes_valido(c.mes26)?;
        let da = |a: &str| c.consolidada.iter().find(|x| x.0 == a).map(|x| x.1);
        if let Some(a) = ANOS.iter().find(|a| **a != "2026" && da(a).is_none()) {
            return Err(format!("TCE-TO: {a} sem o número da remessa Consolidada da Prefeitura"));
        }
        let p = d::nova_pasta(o.site, "975", "Palmas")?;
        let todos = (f.nomes)()?;
        let mut arqs = vec![];
        for ano in ANOS {
            for (tipo, nome, rot) in [("despesa", "balanceteDespesa", "BALANCETE-DESPESA"), ("receita", "balanceteReceita", "BALANCETE-RECEITA")] {
                let pre = format!("{nome}-{ano}-");
                let mut fs: Vec<&String> = todos.iter().filter(|n| n.starts_with(&pre) && n.ends_with(".xls")).collect();
                fs.sort();
                let (mut linhas, mut ugs): (Vec<Vec<String>>, BTreeSet<&str>) = Default::default();
                for n in fs {
                    let t = d::tabela_html(&f.bytes(n)?);
                    let Some((h, corpo)) = t.split_first() else { return Err(format!("{n}: planilha do tribunal sem tabela")) };
                    if linhas.is_empty() {
                        let antes: &[&str] = if tipo == "despesa" { &["codigo_acao_video", "arquivo do TCE-TO (unidade gestora)"] } else { &["arquivo do TCE-TO (unidade gestora)"] };
                        linhas.push([s(antes), h.iter().map(|x| fix(x)).collect()].concat());
                    }
                    for r in corpo {
                        let r: Vec<String> = r.iter().map(|x| fix(x)).collect();
                        let mut l = vec![];
                        if tipo == "despesa" {
                            // codigo igual ao do leitor (orgao.funcao.subfuncao.programa.acao), para achar a acao so por ele
                            let partes = [3, 5, 6, 7, 9].iter().map(|i| u::cel(&r, *i).map_err(|e| format!("{n}: {e}"))).collect::<Result<Vec<_>, _>>()?;
                            l.push(partes.join("."));
                        }
                        l.push(n.to_string());
                        l.extend(r);
                        linhas.push(l);
                    }
                    ugs.insert(n.trim_end_matches(".xls").split('-').nth(3).ok_or_else(|| format!("{n}: nome de planilha sem a unidade gestora"))?);
                }
                if linhas.is_empty() {
                    return Err(format!("TCE-TO: o leitor não guardou nenhuma planilha {nome} de {ano}"));
                }
                let out = d::gravar(&p, &format!("{ano}-{}_TO-{rot}.txt", mes_de(ano, c.mes26)), &linhas)?;
                arqs.push(Arq::novo(ano, tipo, out, format!("TCE-TO SICAP, planilha {nome} {ano} de {} unidade(s) gestora(s) ({PLAN}/{nome}.php?unidade=<CNPJ>&exercicio={ano}&bimestre=<remessa>)", ugs.len())));
            }
        }
        let filtro = |a: &str, t: &str| match (da(a), sem_zero(c.mes26), t == "despesa") {
            (Some(n), _, true) => format!("remessa {n} Consolidada da Prefeitura (CNPJ {}), remessas 1 a {n}", c.principal),
            (Some(n), _, false) => format!("contas analíticas (A) da remessa {n} Consolidada, menos deduções"),
            (None, n, true) => format!("remessa {n} de cada unidade gestora (remessas 1 a {n})"),
            (None, n, false) => format!("contas analíticas (A) da remessa {n} de cada unidade gestora, menos deduções"),
        };
        let mut ad = Adaptador { pasta: p, tribunal: "TCE-TO".into(), arquivos: arqs, partes: &inteiro, codigos: None, faixa_mx: 6,
            col: colunas("'Valor Liquidado' (somado nas remessas 1..N)", "'Dotação Inicial' (linha da remessa N)", "'Valor Empenhado' (somado nas remessas 1..N)", "'Receita Realizada' acumulada (contas analíticas 'A', linha da remessa N; 105 = dedução Fundeb)"),
            filtro: filtros(&filtro) };
        d::montar(o.site, "975", "Palmas", "TO", &mut ad, &dados, o.desenha)
    }
}

/// 991 Porto Alegre (TCE-RS): balancetes do SIAPC (um CSV por orgao, Dados Abertos: `<tipo>_<ano>_<orgao>.csv`) -> um .txt
/// por ano e tipo, com o arquivo do orgao na 1a coluna.
pub mod rs {
    use super::*;
    use crate::link_tribunais_rs::ORGAOS;

    const URL: &str = "https://dados.tce.rs.gov.br/dados/municipal";
    /// O motor casa campo a campo sem olhar a posicao: o numero do projeto tambem aparece noutra coluna e puxava linhas de
    /// outra acao; a chave montada com estes campos (a mesma do leitor) casa a acao exata.
    const CAMPOS: [&str; 7] = ["CD_ORGAO", "CD_ORGAO_ORCAMENTARIO", "CD_UNIDADE_ORCAMENTARIA", "CD_FUNCAO", "CD_SUBFUNCAO", "CD_PROGRAMA", "CD_PROJETO"];

    /// `mes26` = mes de corte de 2026 que o molde do tribunal achou (no Python estava fixo em "09").
    pub fn montar(o: &Onde, f: &Fonte, mes26: &str) -> Result<(PathBuf, Value), String> {
        let dados = o.pacote.join("RECEITAS-DESPESAS-PORTO-ALEGRE-RS");
        mes_valido(mes26)?;
        let p = d::nova_pasta(o.site, "991", "Porto Alegre")?;
        let chave = format!("código da ação (montado: {})", CAMPOS.join("."));
        let (mut arqs, mut filtro) = (vec![], BTreeMap::new());
        for ano in ANOS {
            for (tipo, rot) in [("despesa", "BALANCETE-DESPESA"), ("receita", "BALANCETE-RECEITA")] {
                let (mut linhas, mut com, mut sem): (Vec<Vec<String>>, Vec<String>, Vec<&str>) = Default::default();
                for (org, nome) in ORGAOS {
                    let arq = format!("{tipo}_{ano}_{org}.csv");
                    // orgao sem balancete no tribunal (a Camara nao tem o de receita): fica dito no filtro
                    let Some(b) = (f.le)(&arq)? else {
                        sem.push(nome);
                        continue;
                    };
                    let t = d::csv_py(&utf8(b, &arq)?, ',');
                    let Some((h, corpo)) = t.split_first() else { return Err(format!("{arq}: arquivo do tribunal vazio")) };
                    if linhas.is_empty() {
                        linhas.push([vec!["arquivo do TCE-RS (órgão)".to_string(), chave.clone()], h.clone()].concat());
                    }
                    let ix = if tipo == "despesa" { posicoes(h, &CAMPOS, &arq)? } else { vec![] };
                    for r in corpo {
                        let cod = ix.iter().map(|i| u::cel(r, *i).map_err(|e| format!("{arq}: {e}"))).collect::<Result<Vec<_>, _>>()?.join(".");
                        linhas.push([vec![format!("{ano}/{org}.csv ({nome})"), cod], r.clone()].concat());
                    }
                    com.push(format!("{org} {nome}"));
                }
                if linhas.is_empty() {
                    return Err(format!("TCE-RS: o leitor não guardou nenhum balancete de {tipo} de {ano}"));
                }
                let out = d::gravar(&p, &format!("{ano}-{}_RS-{rot}.txt", mes_de(ano, mes26)), &linhas)?;
                arqs.push(Arq::novo(ano, tipo, out, format!("TCE-RS Dados Abertos, balancete-{tipo} {ano} de {} órgão(s) ({URL}/balancete-{tipo}/{ano}/<CD_ORGAO>.csv)", com.len())));
                let fora = if sem.is_empty() { String::new() } else { format!(" (sem balancete de {tipo} no tribunal: {})", sem.join(", ")) };
                filtro.insert((ano.to_string(), tipo.to_string()), if tipo == "despesa" { format!("todas as linhas dos {} órgãos ({}){fora}", com.len(), com.join(", ")) } else { format!("linhas NIVEL = A dos {} órgãos com receita{fora}", com.len()) });
            }
        }
        let mut ad = Adaptador { pasta: p, tribunal: "TCE-RS".into(), arquivos: arqs, partes: &inteiro, codigos: None, faixa_mx: 6, filtro,
            col: colunas("'VL_LIQUIDADO' (acumulado até o mês da coluna MES)", "'VL_DOTACAO_INICIAL'", "'VL_EMPENHADO' (acumulado até o mês da coluna MES)", "'VL_ARRECADADO' (só linhas NIVEL A; restituições e dedução Fundeb já vêm negativas, CAR_PECULIAR 102/105)") };
        d::montar(o.site, "991", "Porto Alegre", "RS", &mut ad, &dados, o.desenha)
    }
}

/// 981 Florianopolis (TCE-SC): consultas aos paineis do Farol (e-Sfinge) que o leitor guardou em JSON (despesa-MM.json,
/// receita-MM.json, remessas.json) -> um .txt por ano e tipo. O painel Qlik nao tem arquivo para baixar: o JSON e a resposta
/// do proprio painel do tribunal.
pub mod sc {
    use super::*;
    use crate::link_tribunais_sc::{sigla, ENTE, MUN};

    const P: &str = "https://paineistransparencia.tce.sc.gov.br/extensions/";

    fn num(v: &Value) -> f64 {
        v.as_f64().unwrap_or(0.0)
    }

    fn codigo(v: &Value, casas: usize) -> String {
        if v.is_number() { format!("{:0casas$}", num(v) as i64) } else { pystr(v) }
    }

    /// O codigo de cada linha do painel como o leitor monta o das acoes: SIGLA.0.funcao.subfuncao.0.numero (-N quando o
    /// numero se repete na unidade, na ordem do leitor). Linha zerada o leitor nao usa: fica sem codigo.
    pub fn codigos(linhas: &[&Vec<Value>]) -> Result<Vec<String>, String> {
        let (mut cods, mut vistos, mut chave): (Vec<String>, BTreeMap<String, usize>, BTreeMap<[String; 5], String>) = Default::default();
        for l in linhas {
            if num(&l[5]) == 0.0 && num(&l[6]) == 0.0 {
                cods.push(String::new());
                continue;
            }
            let k = [pystr(&l[0]), codigo(&l[1], 2), codigo(&l[2], 3), pystr(&l[3]), pystr(&l[4])];
            if !chave.contains_key(&k) {
                let c = format!("{}.0.{}.{}.0.{}", sigla(&k[0])?, k[1], k[2], k[3]);
                let n = vistos.entry(c.clone()).or_default();
                *n += 1;
                chave.insert(k.clone(), if *n > 1 { format!("{c}-{n}") } else { c });
            }
            cods.push(chave[&k].clone());
        }
        Ok(cods)
    }

    /// `mes26` = mes de corte de 2026 que o molde achou no painel de remessas (no Python estava fixo em "09").
    pub fn montar(o: &Onde, f: &Fonte, mes26: &str) -> Result<(PathBuf, Value), String> {
        let dados = o.pacote.join("RECEITAS-DESPESAS-FLORIANOPOLIS-SC");
        let m26 = mes_valido(mes26)?;
        let p = d::nova_pasta(o.site, "981", "Florianópolis")?;
        let (desp, rec) = (f.json(&format!("despesa-{mes26}.json"))?, f.json(&format!("receita-{mes26}.json"))?);
        let mut arqs = vec![];
        for ano in ANOS {
            let m = if ano == "2026" { m26 } else { 12 };
            let ac = linhas_de(&desp[ano]["acoes"], &format!("despesa {ano}, ações"), 7)?;
            let mut l = vec![s(&["codigo_acao_video", "consulta ao painel", "nomeUG", "codigoFuncao", "codigoSubFuncao", "Nro. ProjetoAtividade", "Projeto Atividade", "Liquidado (Valor Transação Liquidação + Valor Estorno Liquidação, até o fim do período)", "Empenhado (Valor Empenho + Valor Estorno, até o fim do período)"])];
            l.extend(codigos(&ac)?.into_iter().zip(&ac).map(|(c, r)| [vec![c, "ações".into()], cels(r)].concat()));
            for t in linhas_de(&desp[ano]["total"], &format!("despesa {ano}, total"), 0)? {
                let (ente, resto) = t.split_first().ok_or_else(|| format!("despesa {ano}: total do município vazio"))?;
                l.push([vec![String::new(), "total do município (Ente)".into(), d::cel_json(ente)], vec![String::new(); 4], cels(resto)].concat());
            }
            let out = d::gravar(&p, &format!("{ano}-{m:02}_SC-ESFINGE-DESPESAS-MUNICIPAIS.txt"), &l)?;
            arqs.push(Arq::novo(ano, "despesa", out, format!("TCE-SC, Farol, painel Despesas Municipais (e-Sfinge) {P}appDespesasMunicipaisExternoNovo/index.html, [Mun/UF] = '{MUN}', Ano = {ano}, por unidade gestora, função, subfunção e projeto/atividade, liquidações e empenhos com data até {} (consulta Qlik feita pelo leitor, uma por vez)", ultimo_dia(ano, m)?)));
            let mut l = vec![s(&["consulta ao painel", "codigoreceita", "valorarrecadado (até o fim do período)"])];
            for (rotulo, nome) in [("rubricas", "rubricas"), ("total do município (nome_ente)", "total")] {
                l.extend(linhas_de(&rec[ano][nome], &format!("receita {ano}, {nome}"), 0)?.into_iter().map(|r| [vec![rotulo.to_string()], cels(r)].concat()));
            }
            let out = d::gravar(&p, &format!("{ano}-{m:02}_SC-ESFINGE-RECEITAS-MUNICIPAIS.txt"), &l)?;
            arqs.push(Arq::novo(ano, "receita", out, format!("TCE-SC, Farol, painel Receitas Municipais (e-Sfinge) {P}appReceitasMunicipaisExterno/index.html, nome_ente = '{ENTE}', competência {ano}, anomes <= {ano}{m:02}")));
        }
        let rem = f.json("remessas.json")?;
        let mut l = vec![s(&["nomeunidade", "anomes", "descricao", "statusenvio", "contagem"])];
        l.extend(linhas_de(&rem, "remessas.json", 0)?.into_iter().map(|r| cels(r)));
        let out = d::gravar(&p, &format!("2026-{mes26}_SC-ESFINGE-REMESSAS-2026.txt"), &l)?;
        arqs.push(Arq::novo("2026", "outro", out, format!("TCE-SC, painel Remessas Situação {P}appRemessasSituacao/index.html: remessa mensal de cada unidade em 2026 (corte = {}, último mês de 'Registros Contábeis' entregue por todas)", NM[m26 as usize - 1])));
        let partes = es::partes_festas(&dados)?;
        let mut ad = Adaptador { pasta: p, tribunal: "TCE-SC".into(), arquivos: arqs, partes: &partes, codigos: None, faixa_mx: 6,
            col: colunas("'Liquidado (Valor Transação Liquidação + Valor Estorno Liquidação, até o fim do período)'", "- (o TCE-SC não publica a dotação por ação)", "'Empenhado (Valor Empenho + Valor Estorno, até o fim do período)'", "'valorarrecadado (até o fim do período)' (já líquido das deduções)"),
            filtro: filtros(&|_, t| if t == "despesa" { "linhas da consulta 'ações' (todas as unidades gestoras de Florianópolis); confere com a linha 'total do município'".to_string() } else { "linhas da consulta 'rubricas'; confere com a linha 'total do município'".to_string() }) };
        let (pasta, rel) = d::montar(o.site, "981", "Florianópolis", "SC", &mut ad, &dados, o.desenha)?;
        let rel = es::setores(o.site, &pasta, "981", &ad, rel)?;
        Ok((pasta, rel))
    }
}

/// 977 Rio Branco (TCE-AC): o leitor leu os paineis Qlik do TCE-AC e nao guardou nada do tribunal. Aqui a mesma consulta, com
/// entidade e remessa (mes) a mais nas linhas, vira o arquivo: o "exportar dados" do Qlik responde "Access denied" ao anonimo,
/// entao o documento e a resposta da consulta, como no Radar Carioca do RJ.
pub mod ac {
    use super::*;
    use crate::link_tribunais_ac::Cubo;

    const H: &str = "qlik.tceac.tc.br";
    const APP_DESPESA: &str = "7e2eb635-c55d-41d4-9d1e-3d33c6730a1c";
    const APP_RECEITA: &str = "c0fc28aa-9c84-4117-a080-81101f98bc0b";
    const ENTE: &str = "RIO BRANCO";
    pub const DESP: [&str; 6] = ["entidade", "NUMERO_REMESSA", "numero_funcao", "numero_subfuncao", "CODIGO_ACAO", "TITULO_ACAO"];
    pub const REC: [&str; 3] = ["ENTIDADE.NOME", "MES", "CODIGO-NUMERO_NATUREZA_RECEITA"];

    /// As medidas da consulta, com os mesmos filtros do leitor.
    pub fn medidas(tipo: &str, a: &str) -> Vec<String> {
        if tipo == "despesa" {
            let f = format!("{{<NOME_ENTE={{'{ENTE}'}},ANO_REMESSA={{{a}}},ano_empenho={{{a}}}>}}");
            vec![format!("Sum({f}valor_liquidado)"), format!("Sum({f}VALOR_EMPENHO)")]
        } else {
            let f = format!("{{<ENTE.NOME={{'{ENTE}'}},ANO={{{a}}}>}}");
            vec![format!("Sum({f}RECEITA_REALIZADA)"), format!("Sum({f}DEDUCAO)")]
        }
    }

    /// O codigo da acao do video, como o leitor monta (0.0.funcao.subfuncao.0.acao).
    fn cod(r: &[Value]) -> Result<String, String> {
        let a = r[4].as_str().ok_or("painel de despesa do TCE-AC: linha sem o código da ação")?;
        let acao = if !a.is_empty() && a.bytes().all(|b| b.is_ascii_digit()) { a.parse::<u64>().map_or(a.to_string(), |n| n.to_string()) } else { a.to_string() };
        Ok(format!("0.0.{:02}.{:03}.0.{acao}", int_py(&r[2], "função")?, int_py(&r[3], "subfunção")?))
    }

    /// `cubo(tipo, dimensoes, medidas)` = o JSON com todas as linhas da consulta ao motor do Qlik (sessao anonima da pagina
    /// publica, uma consulta por vez com pausa: e de quem chama). `anos` = os anos com remessa da Prefeitura, que o molde achou
    /// (no Python, 2024 e 2025 fixos); so ano inteiro, como no Python.
    pub fn montar(o: &Onde, cubo: Cubo, anos: &[&str]) -> Result<(PathBuf, Value), String> {
        let dados = o.pacote.join("RECEITAS-DESPESAS-RIO-BRANCO-AC");
        if anos.is_empty() || anos.contains(&"2026") {
            return Err("TCE-AC: os documentos só têm montagem para ano inteiro (2026 parcial não está no Python)".into());
        }
        let p = d::nova_pasta(o.site, "977", "Rio Branco")?;
        let mut arqs = vec![];
        for ano in anos {
            for (tipo, rot, app, dims) in [("despesa", "REMESSAS-DESPESA", APP_DESPESA, &DESP[..]), ("receita", "REMESSAS-RECEITA", APP_RECEITA, &REC[..])] {
                let meas = medidas(tipo, ano);
                let o_que = format!("painel de {tipo} do TCE-AC {ano}");
                let v = u::json(&cubo(tipo, dims, &meas)?, &o_que)?;
                let todas = linhas_de(&v, &o_que, dims.len() + meas.len())?;
                if todas.is_empty() {
                    return Err(format!("{o_que}: consulta sem linhas"));
                }
                // tira linha toda zerada; Sum({<filtro>}campo) -> campo (o filtro vai no origem)
                let l: Vec<&&Vec<Value>> = todas.iter().filter(|r| r[dims.len()..].iter().any(|x| x.as_f64().is_some_and(|x| x != 0.0))).collect();
                let cab = dims.iter().map(|x| x.to_string()).chain(meas.iter().map(|k| k.rsplit('}').next().unwrap_or("").trim_end_matches(')').to_string())).collect();
                let mut linhas: Vec<Vec<String>> = std::iter::once(cab).chain(l.iter().map(|r| cels(r))).collect();
                if tipo == "despesa" {
                    linhas = com_coluna("codigo_acao_video", l.iter().map(|r| cod(r)).collect::<Result<_, _>>()?, linhas);
                }
                let out = d::gravar(&p, &format!("{ano}-12_AC-{rot}.txt"), &linhas)?;
                arqs.push(Arq::novo(ano, tipo, out, format!("TCE-AC, painel Qlik '{}' de tceac.tc.br/site/inicio/acre-transparente (https://{H}/single/?appid={app}), consulta ao motor do Qlik: {} x {}; linhas todas zeradas tiradas{}",
                    if tipo == "despesa" { "Despesa" } else { "Receita" }, dims.join(", "), meas.join(", "), if tipo == "despesa" { "; 1a coluna acrescentada = código da ação do vídeo, tirado das colunas da própria linha" } else { "" })));
            }
        }
        let filtro = anos.iter().flat_map(|a| [((a.to_string(), "despesa".to_string()), format!("todas as linhas (todas as entidades de Rio Branco, remessas 1 a 12, empenhos de {a})")), ((a.to_string(), "receita".to_string()), "todas as linhas (todas as entidades, meses 1 a 12): realizada + dedução".to_string())]).collect();
        let col = [("liquidado", "'valor_liquidado' (soma no Qlik, empenhos do próprio ano)"), ("empenhado", "'VALOR_EMPENHO' (soma no Qlik)"), ("arrecadado", "'RECEITA_REALIZADA' mais 'DEDUCAO' (dedução já vem negativa)")].into_iter().map(|(k, v)| (k, v.to_string())).collect();
        let mut ad = Adaptador { pasta: p, tribunal: "TCE-AC".into(), arquivos: arqs, partes: &inteiro, codigos: None, faixa_mx: 6, col, filtro };
        d::montar(o.site, "977", "Rio Branco", "AC", &mut ad, &dados, o.desenha)
    }
}

/// 995 Manaus (TCE-AM, e-Contas, API de Dados Abertos): ~3.800 JSON (uma consulta por UG e ano/mes) -> um .txt por ano e tipo,
/// com a 1a coluna dizendo a consulta de origem e so as colunas usadas. Como o leitor monta os numeros: festas = PAGO da acao
/// (pagamentos, exercicio = ano) e previa = vlDotacao de origem 1 (dotacoes); gastou/setores = LIQUIDADO do balancete de cada UG
/// (contas 6221303 + 6221304 + 6221307, credito - debito); arrecadado = vlLancado das receitasArrecadadas.
pub mod am {
    use super::*;
    use crate::link_tribunais_am::ID_MUN;

    const API: &str = "https://econtasapi.tce.am.gov.br/transparencia/dados-abertos";
    const LIQ: [&str; 3] = ["6221303", "6221304", "6221307"];
    const PAG: [&str; 12] = ["idPagamentoEmpenho", "exercicio", "numNotaEmpenho", "dtPagamentoEmpenho", "vlPagamentoEmpenho", "idUnidadeGestora", "codUnidadeOrcamentaria", "codFuncaoGoverno", "codSubfuncaoGoverno", "codProgramaGoverno", "codAcaoGoverno", "desAcaoGoverno"];
    const DOT: [&str; 12] = ["idDotacaoOrcamentaria", "codOrigemDotacaoOrcamentaria", "vlDotacao", "idUnidadeGestora", "codUnidadeOrcamentaria", "codFuncaoGoverno", "codSubfuncaoGoverno", "codProgramaGoverno", "codAcaoGoverno", "desAcaoGoverno", "codElementoDespesa", "codFonteRecurso"];
    const MOV: [&str; 7] = ["idUnidadeGestora", "codContaContabil", "desContaContabil", "vlCredito", "vlDebito", "codCompetencia", "exercicio"];
    const REC: [&str; 8] = ["idReceitaArrecadada", "idUnidadeGestora", "codItemReceita", "desItemReceita", "desTipoLancamentoReceita", "vlLancado", "codCompetencia", "exercicio"];
    const FLAG: &str = "regra do leitor (conta)";
    /// Os 3 anos de pagamentos passam de 90 MB com o resto: o deste ano fica so citado (pasta _grandes, nao vai ao site).
    const GRANDE: &str = "2024";

    /// Linha da unidade gestora (liquidado do balancete).
    fn eh_ug(c: &str) -> bool {
        c.ends_with(".0000.0000") && c.split('.').nth(1) == Some("0")
    }

    fn regra(conta: &str) -> &'static str {
        if LIQ.iter().any(|p| conta.starts_with(p)) {
            "LIQUIDADO e EMPENHADO"
        } else if conta.starts_with("62213") {
            "EMPENHADO"
        } else {
            ""
        }
    }

    /// O mesmo codigo do leitor: pagamento sem programa/acao usa o da dotacao da mesma UO com o mesmo nome.
    fn cod(ug: &str, x: &Value, nomes: &mut BTreeMap<String, (i64, i64)>) -> Result<String, String> {
        if x["codUnidadeOrcamentaria"].is_null() {
            return Err("TCE-AM: lançamento sem unidade orçamentária".into());
        }
        let (uo, f, sub) = (pystr(&x["codUnidadeOrcamentaria"]), int_py(&x["codFuncaoGoverno"], "TCE-AM: função")?, int_py(&x["codSubfuncaoGoverno"], "TCE-AM: subfunção")?);
        let nm = format!("{ug}|{uo}|{f}|{sub}|{}", x["desAcaoGoverno"].as_str().unwrap_or("").trim().to_uppercase());
        let pa = if x["codProgramaGoverno"].is_null() || x["codAcaoGoverno"].is_null() {
            nomes.get(&nm).copied().unwrap_or((0, 0))
        } else {
            let pa = (int_py(&x["codProgramaGoverno"], "TCE-AM: programa")?, int_py(&x["codAcaoGoverno"], "TCE-AM: ação")?);
            nomes.entry(nm).or_insert(pa);
            pa
        };
        Ok(format!("{ug}.{uo}.{f:02}.{sub:03}.{:04}.{:04}", pa.0, pa.1))
    }

    fn vals(x: &Value, ks: &[&str]) -> Vec<String> {
        ks.iter().map(|k| d::cel_json(&x[*k])).collect()
    }

    pub fn montar(o: &Onde, f: &Fonte) -> Result<(PathBuf, Value), String> {
        let dados = o.pacote.join("RECEITAS-DESPESAS-MANAUS-AM");
        let corte26 = mes_valido(&corte26(&dados)?)?;
        let un = f.json("unidades.json")?;
        let ugs: Vec<String> = lista(&un, "unidades.json")?.iter().filter(|x| x["idMunicipio"].as_i64() == Some(ID_MUN) && x["esfera"].as_i64() == Some(2)).map(|x| if x["id_unidade_gestora"].is_null() { Err("TCE-AM: unidade gestora sem id".to_string()) } else { Ok(pystr(&x["id_unidade_gestora"])) }).collect::<Result<_, _>>()?;
        if ugs.is_empty() {
            return Err("TCE-AM: nenhuma unidade gestora municipal de Manaus em unidades.json".into());
        }
        let p = d::nova_pasta(o.site, "995", "Manaus")?;
        std::fs::create_dir(p.join("_grandes")).map_err(|e| e.to_string())?;
        let mut arqs = vec![];
        for a in ANOS {
            let corte = if a == "2026" { corte26 } else { 12 };
            let (mm, ano_n) = (format!("{corte:02}"), u::int(a, "ano")? as f64);
            let mut cod_ug = BTreeMap::new();
            for x in acoes(&dados, a)? {
                let c = x["cod"].as_str().ok_or_else(|| format!("acoes-{a}: ação sem código"))?;
                if eh_ug(c) {
                    cod_ug.insert(u::int(c.split('.').next().unwrap_or(""), "unidade gestora")?.to_string(), c.to_string());
                }
            }
            let (mut pag, mut dot, mut mov, mut rec, mut nomes): (Vec<Vec<String>>, Vec<Vec<String>>, Vec<Vec<String>>, Vec<Vec<String>>, BTreeMap<String, (i64, i64)>) = Default::default();
            for ug in &ugs {
                let ds = f.lista_ou_vazia(&format!("dotacoes_{ug}_{a}.json"))?;
                let ativo = !ds.is_empty(); // o leitor so conta UG com dotacao no ano
                for x in &ds {
                    dot.push([vec![cod(ug, x, &mut nomes)?, format!("dotacoes/{ug}/{a}")], vals(x, &DOT)].concat());
                }
                for x in f.lista_ou_vazia(&format!("receitasArrecadadas_{ug}_{a}.json"))? {
                    if a != "2026" || int_py(&x["codCompetencia"], "TCE-AM: competência")? % 100 <= corte as i64 {
                        rec.push([vec![format!("receitasArrecadadas/{ug}/{a}")], vals(&x, &REC)].concat());
                    }
                }
                for m in 1..=corte {
                    // restos a pagar de anos anteriores ficam fora, como no leitor
                    for x in f.lista_ou_vazia(&format!("pagamentos_{ug}_{a}_{m}.json"))?.iter().filter(|x| x["exercicio"].as_f64() == Some(ano_n)) {
                        pag.push([vec![if ativo { cod(ug, x, &mut nomes)? } else { String::new() }, format!("pagamentos/{ug}/{a}/{m}")], vals(x, &PAG)].concat());
                    }
                    for x in f.lista_ou_vazia(&format!("movimentacoesContabeis_{ug}_{a}_{m}.json"))? {
                        let rg = regra(u::txt(&x, "codContaContabil")?);
                        let c = if ativo && rg.starts_with("LIQ") { cod_ug.get(ug).cloned().ok_or_else(|| format!("TCE-AM {a}: unidade gestora {ug} com dotação e sem linha no levantamento"))? } else { String::new() };
                        mov.push([vec![c, format!("movimentacoesContabeis/{ug}/{a}/{m}")], vals(&x, &MOV), vec![rg.to_string()]].concat());
                    }
                }
            }
            let q = format!("consulta da API ({API}/...)");
            let grande = a == GRANDE;
            let grava = |pasta: &Path, nome: String, antes: &[&str], ks: &[&str], depois: &[&str], linhas: Vec<Vec<String>>| d::gravar_com_aspas(pasta, &nome, &[vec![s(&[antes, ks, depois].concat())], linhas].concat());
            let cv = ["codigo_acao_video", q.as_str()];
            let fp = grava(&if grande { p.join("_grandes") } else { p.clone() }, format!("{a}-{mm}_AM-ECONTAS-PAGAMENTOS.txt"), &cv, &PAG, &[], pag)?;
            let fd = grava(&p, format!("{a}-12_AM-ECONTAS-DOTACOES.txt"), &cv, &DOT, &[], dot)?;
            let fm = grava(&p, format!("{a}-{mm}_AM-ECONTAS-BALANCETES.txt"), &cv, &MOV, &[FLAG], mov)?;
            let fr = grava(&p, format!("{a}-{mm}_AM-ECONTAS-RECEITAS-ARRECADADAS.txt"), &cv[1..], &REC, &[], rec)?;
            arqs.extend([
                Arq::novo(a, "despesa", fm, format!("TCE-AM e-Contas, {API}/movimentacoesContabeis/<ug>/{a}/<mês 1..{corte}> ({} unidades gestoras de Manaus): balancete mensal, todas as linhas, só as colunas usadas; a última coluna marca as contas que o leitor soma (liquidado = 6221303+6221304+6221307, empenhado = 62213, crédito − débito)", ugs.len())),
                Arq::novo(a, "despesa", fd, format!("TCE-AM e-Contas, {API}/dotacoes/<ug>/{a}: todas as linhas, só as colunas usadas")),
                Arq { enviado: !grande, ..Arq::novo(a, "despesa", fp, format!("TCE-AM e-Contas, {API}/pagamentos/<ug>/{a}/<mês 1..{corte}>: só os pagamentos de empenhos do próprio ano (como no leitor) e só as colunas usadas{}", if grande { "; grande demais para o site junto com o resto (baixe as consultas completas nesse endereço, uma por unidade gestora e mês)" } else { "" })) },
                Arq::novo(a, "receita", fr, format!("TCE-AM e-Contas, {API}/receitasArrecadadas/<ug>/{a}{}: todas as linhas, só as colunas usadas", if a == "2026" { format!(", competências até {corte}/2026") } else { String::new() })),
            ]);
        }
        // 1a coluna codigo_acao_video = cod do leitor; no BALANCETES a linha da UG (u.0.ff.sss.0000.0000) vai nas contas de liquidado dela
        let mut ad = Adaptador { pasta: p, tribunal: "TCE-AM".into(), arquivos: arqs, partes: &inteiro, codigos: None, faixa_mx: 6,
            col: colunas("festas: 'vlPagamentoEmpenho' (PAGAMENTOS; o TCE-AM não publica liquidado por ação); unidades gestoras: 'vlCredito' − 'vlDebito' das linhas marcadas LIQUIDADO no BALANCETES (a linha da Manauscult desconta o pago das festas)", "'vlDotacao' das linhas com codOrigemDotacaoOrcamentaria 1 (Orçamento Inicial) no DOTACOES", "'vlCredito' − 'vlDebito' das linhas marcadas EMPENHADO no BALANCETES", "'vlLancado' (já líquido das deduções do Fundeb)"),
            filtro: filtros(&|a, t| if t == "receita" { "todas as linhas".to_string() } else { format!("BALANCETES: linhas com codigo_acao_video preenchido (contas de liquidado das unidades gestoras com dotação no ano){}", if a == "2026" { format!(", meses 1 a {corte26}") } else { String::new() }) }) };
        d::montar(o.site, "995", "Manaus", "AM", &mut ad, &dados, o.desenha)
    }

    /// Soma a coluna de liquidado das linhas achadas pelo codigo_acao_video de CADA acao de acoes-*.json e compara.
    pub fn conferir(pacote: &Path, pasta: &Path) -> Result<Value, String> {
        let dados = pacote.join("RECEITAS-DESPESAS-MANAUS-AM");
        let (mut out, mut ruins) = (serde_json::Map::new(), 0);
        for a in ANOS {
            let abre = |padrao: &str, fundo: bool, cols: &[&str]| -> Result<(Vec<Vec<String>>, d::Indice, Vec<usize>), String> {
                let arq = acha(pasta, &format!("{a}-*_AM-ECONTAS-{padrao}.txt"), fundo)?;
                let t = d::ler_linhas(&arq, None)?;
                let c = posicoes(t.first().ok_or_else(|| format!("{}: vazio", arq.display()))?, cols, &nome_de(&arq))?;
                Ok((t, d::indice(&arq, None)?, c))
            };
            let ((tm, im, cm), (tp, ip, cp), (td, id, cd)) = (abre("BALANCETES", false, &["vlCredito", "vlDebito"])?, abre("PAGAMENTOS", true, &["vlPagamentoEmpenho"])?, abre("DOTACOES", false, &["vlDotacao", "codOrigemDotacaoOrcamentaria"])?);
            let v = |x: &str| real(x, "TCE-AM");
            let (ac, mut ruim, mut festas) = (acoes(&dados, a)?, vec![], vec![]);
            for x in &ac {
                let (c, liq) = liquidado(x)?;
                let um = [c.to_string()];
                let so = if eh_ug(c) {
                    let ns = d::achar(&im, c, &um);
                    soma(&tm, &ns, cm[0], &v)? - soma(&tm, &ns, cm[1], &v)?
                } else {
                    let so = soma(&tp, &d::achar(&ip, c, &um), cp[0], &v)?;
                    let iniciais: Vec<usize> = d::achar(&id, c, &um).into_iter().filter(|n| td[n - 1].get(cd[1]).is_some_and(|x| x == "1")).collect();
                    festas.push(json!([c, x["acao"], r2(so), liq, r2(soma(&td, &iniciais, cd[0], &v)?), x["orcado"]]));
                    so
                };
                if (so - liq).abs() > 0.01 {
                    ruim.push(json!([c, u::ini(x["acao"].as_str().unwrap_or(""), 50), r2(so), liq, r2(so - liq)]));
                }
            }
            let com_cod: Vec<usize> = (2..=tm.len()).filter(|n| tm[n - 1].first().is_some_and(|c| !c.is_empty())).collect();
            let tot = soma(&tm, &com_cod, cm[0], &v)? - soma(&tm, &com_cod, cm[1], &v)?;
            ruins += ruim.len();
            out.insert(a.into(), json!({ "acoes": ac.len(), "nao_bateram": ruim, "festas (cod, ação, pago achado, liquidado json, orçado achado, orçado json)": festas, "liquidado_balancete": r2(tot), "total_acoes": r2(ac.iter().filter_map(|x| x["liquidado"].as_f64()).sum()) }));
        }
        conferido("TCE-AM", ruins, out)
    }
}

/// 987 Campo Grande (TCE-MS): consultas aos paineis Qlik do tribunal (gravadas em <nome>.json pelo leitor) -> .txt por ano e
/// tipo. O codigo da acao do video ganha uma coluna propria nos .txt de despesa e dotacao, calculada com as mesmas regras do
/// leitor; '.CAM' (Camara) fica fora do padrao do motor, por isso o `cod_texto`.
pub mod ms {
    use super::*;
    use crate::link_tribunais_ms::mes26;

    const WS: &str = "wss://painel.tce.ms.gov.br/app/";
    const SICOM: &str = "6a9f40ab-1554-4971-868d-80915f73afce";
    const ESF: &str = "5d5bcac7-96d8-45c1-b0f4-01534afb3512";
    const ESF_PPA: &str = "6d157349-49c8-4e38-b57c-398bd19c629d";
    const ESF_REC: &str = "1a0a1967-925d-4982-b115-4664d2247425";
    const EMP: &str = "6d1acf0e-eaff-482a-89be-7fbfe7e9557f";
    const CHAVE: &str = "código da ação do vídeo (montado pelas regras do leitor: 0.0.função.subfunção.0.nº ação.função.subfunção[.CAM])";
    const PPA: [&str; 5] = ["Acao_codigoAcao", "Acao_descricao", "Acao_funcao", "Acao_subfuncao", "Count(DISTINCT %Acao)"];

    /// arquivo -> (app do Qlik, cabecalho = dimensoes + medidas exatamente como o leitor pediu).
    fn cab(n: &str) -> Result<(&'static str, &'static [&'static str]), String> {
        Ok(match n {
            "exec2025" | "exec2026" => (ESF, &["identificadorUnidadeGestora", "nomeUnidade", "funcaoFormatada", "subFuncaoFormatada", "numeroProjetoAtividade", "tipoMovimento", "anoMes", "CompetenciaMovimento", "saldo (Sum valor menos estorno)"]),
            "dot2025" | "dot2026" => (ESF, &["Acao_codigoAcao", "valorOrcado (dotação inicial da LOA)", "Ade_dotacaoAtualizada (dotação atualizada)"]),
            "ppa2022" | "ppa2026" => (ESF, &PPA),
            "ppa2022b" => (ESF_PPA, &PPA),
            "des2024" => (EMP, &["ug", "funcao", "subfuncao", "nro_projeto_atividade", "liquidado (valor_liquidado - valor_anula_liquidacao)", "empenhado (valor_empenho - valor_anula_empenho)"]),
            "nomes2024" => (SICOM, &["Órgão", "Descrição da função", "subfuncao_desc", "cod_funcao", "cod_subfuncao", "nr_projeto_atividade", "desc_natureza_acao", "Count([Nº empenho])"]),
            "funcoes" => (SICOM, &["tabela (f = função, s = subfunção)", "descrição", "código", "Count"]),
            "rec2024" => (SICOM, &["nome_ug", "Rúbrica", "Descrição rúbrica", "cod_fonte_completo", "Receitas - Mês", "Receita Acumulada"]),
            "rec_esf" => (ESF_REC, &["ano", "anomes", "unidade_gestora", "natureza_receita", "cod_tipo_deducao_receita", "valor_arrecadado"]),
            _ => return Err(format!("TCE-MS: consulta desconhecida {n}")),
        })
    }

    fn app(id: &str) -> &'static str {
        match id {
            SICOM => "Acompanhamento de Receita e Despesas - Municípios",
            ESF => "e-Sfinge - Custos",
            ESF_PPA => "Esfinge - Custos_PRODUÇÃO",
            ESF_REC => "Acompanhamentos das Receitas Municipais - E-Sfinge",
            _ => "Detalhe Empenho",
        }
    }

    /// As linhas de uma consulta gravada, cada uma com as colunas do cabecalho ("funcoes" = as duas tabelas, com a letra na frente).
    fn ld(f: &Fonte, n: &str) -> Result<Vec<Vec<Value>>, String> {
        let (v, cols) = (f.json(&format!("{n}.json"))?, cab(n)?.1.len());
        if n != "funcoes" {
            return Ok(linhas_de(&v, n, cols)?.into_iter().cloned().collect());
        }
        let mut out = vec![];
        for k in ["f", "s"] {
            out.extend(linhas_de(&v[k], n, cols - 1)?.into_iter().map(|r| [vec![json!(k)], r.clone()].concat()));
        }
        Ok(out)
    }

    fn camara(nome: &Value) -> bool {
        sem_acento(&pystr(nome)).to_uppercase().contains("CAMARA MUNICIPAL")
    }

    /// O `cod` da acao como o leitor monta. `f`/`sub` = "13 - Cultura" ou so o codigo; funcao 01 fora da Camara vira 0.
    fn cod(f: &str, sub: &str, resto: &str, cam: bool) -> String {
        let antes = |x: &str| x.split('-').next().unwrap_or("").trim().to_string();
        let (mut f, sub) = (antes(f), antes(sub));
        if !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()) && f.trim_start_matches('0') == "1" && !cam {
            f = "0".into();
        }
        format!("0.0.{}.{}.0.{resto}{}", if f.is_empty() { "0" } else { &f }, if sub.is_empty() { "0" } else { &sub }, if cam { ".CAM" } else { "" })
    }

    type Chave<'a> = &'a dyn Fn(&[Value]) -> Result<String, String>;

    /// 2024: funcao/subfuncao pelo nome (nomes do SICOM, depois a tabela completa), Camara a parte.
    fn chave_2024(f: &Fonte) -> Result<impl Fn(&[Value]) -> Result<String, String>, String> {
        let (mut cf, mut cs): (BTreeMap<String, String>, BTreeMap<String, String>) = Default::default();
        for l in ld(f, "nomes2024")? {
            cf.insert(pystr(&l[1]), pystr(&l[3]));
            cs.insert(pystr(&l[2]), pystr(&l[4]));
        }
        for l in ld(f, "funcoes")? {
            (if l[0] == "f" { &mut cf } else { &mut cs }).entry(pystr(&l[1])).or_insert(pystr(&l[2]));
        }
        Ok(move |r: &[Value]| {
            let (fd, sd, nr) = (pystr(&r[1]), pystr(&r[2]), pystr(&r[3]));
            let (fu, sub) = (cf.get(&fd).ok_or_else(|| format!("TCE-MS: função sem código no tribunal: {fd}"))?, cs.get(&sd).ok_or_else(|| format!("TCE-MS: subfunção sem código no tribunal: {sd}"))?);
            Ok(cod(fu, sub, &format!("{nr}.{fu}.{sub}"), camara(&r[0])))
        })
    }

    fn chave_exec(r: &[Value]) -> Result<String, String> {
        let (fu, sub) = (pystr(&r[2]), pystr(&r[3]));
        Ok(cod(&fu, &sub, &format!("{}.{}.{}", pystr(&r[4]), fu.split('-').next().unwrap_or(""), sub.split('-').next().unwrap_or("")), camara(&r[1])))
    }

    /// A dotacao vai para a funcao/subfuncao da acao com mais liquidado ate o corte (mesma regra e mesma ordem do leitor).
    fn chave_dot(f: &Fonte, a: &str, corte: &str) -> Result<impl Fn(&[Value]) -> Result<String, String>, String> {
        let (limite, mut acoes, mut onde): (String, Vec<(String, f64)>, BTreeMap<[String; 4], usize>) = (format!("{a}-{corte}"), vec![], BTreeMap::new());
        for l in ld(f, &format!("exec{a}"))? {
            if pystr(&l[6]) > limite {
                continue;
            }
            let n = acoes.len();
            let i = *onde.entry([pystr(&l[4]), pystr(&l[2]), pystr(&l[3]), camara(&l[1]).to_string()]).or_insert(n);
            if i == n {
                acoes.push((chave_exec(&l)?, 0.0));
            }
            if l[5] == "Liquidacao" {
                acoes[i].1 += l[8].as_f64().ok_or_else(|| format!("TCE-MS: medida que não é número: {}", l[8]))?;
            }
        }
        Ok(move |r: &[Value]| {
            let nr = pystr(&r[0]);
            let alvo = acoes.iter().filter(|x| x.0.split('.').nth(5) == Some(nr.as_str())).fold(None, |m: Option<&(String, f64)>, x| if m.is_none_or(|m| x.1 > m.1) { Some(x) } else { m });
            Ok(alvo.map_or_else(|| format!("0.0.0.0.0.{nr}.0.0"), |x| x.0.clone()))
        })
    }

    fn tab(f: &Fonte, nomes: &[&str], chave: Option<Chave>, filtro: Option<&dyn Fn(&[Value]) -> bool>) -> Result<Vec<Vec<String>>, String> {
        let primeiro = nomes.first().ok_or("TCE-MS: tabela sem consulta")?;
        let mut out = vec![[s(&["arquivo do TCE-MS (consulta gravada)"]), s(if chave.is_some() { &[CHAVE] } else { &[] }), s(cab(primeiro)?.1)].concat()];
        for n in nomes {
            for r in ld(f, n)?.iter().filter(|r| filtro.is_none_or(|t| t(r))) {
                out.push([vec![format!("{n}.json")], chave.map(|c| c(r)).transpose()?.into_iter().collect(), cels(r)].concat());
            }
        }
        Ok(out)
    }

    pub fn montar(o: &Onde, f: &Fonte) -> Result<(PathBuf, Value), String> {
        let dados = o.pacote.join("RECEITAS-DESPESAS-CAMPO-GRANDE-MS");
        let m26 = mes26(&|n: &str| f.texto(&format!("{n}.json")))?;
        let p = d::nova_pasta(o.site, "987", "Campo Grande")?;
        let ente = f.json("ente.json")?.get(0).filter(|x| !x.is_null()).map(pystr).ok_or("ente.json sem o código do ente")?;
        let mut arqs = vec![];
        let mut add = |ano: &str, tipo: &str, rot: &str, linhas: Vec<Vec<String>>, nomes: &[&str], extra: &str| -> Result<(), String> {
            let out = d::gravar(&p, &format!("{ano}-{}_MS-{rot}.txt", mes_de(ano, &m26)), &linhas)?;
            let org = nomes.iter().map(|n| Ok(format!("tce-ms/{n}.json = consulta ao app Qlik '{}' do TCE-MS ({WS}{}, acesso anônimo)", app(cab(n)?.0), cab(n)?.0))).collect::<Result<Vec<_>, String>>()?;
            arqs.push(Arq::novo(ano, tipo, out, org.join("; ") + extra));
            Ok(())
        };
        add("2024", "despesa", "SICOM-DETALHE-EMPENHO", tab(f, &["des2024"], Some(&chave_2024(f)?), None)?, &["des2024"], "; filtro do tribunal: Município CAMPO GRANDE, Exercício empenho 2024")?;
        add("2024", "outro", "SICOM-NOMES-ACOES", tab(f, &["nomes2024"], None, None)?, &["nomes2024"], "; nome da ação e códigos de função/subfunção (Despesas - Ano 2024)")?;
        add("2024", "outro", "SICOM-FUNCOES", tab(f, &["funcoes"], None, None)?, &["funcoes"], "; tabela de funções e subfunções do SICOM")?;
        add("2024", "receita", "SICOM-RECEITA-ACUMULADA", tab(f, &["rec2024"], None, None)?, &["rec2024"], "; filtro do tribunal: Município CAMPO GRANDE, Receitas - Ano 2024")?;
        for a in ["2025", "2026"] {
            let (exec, dot) = (format!("exec{a}"), format!("dot{a}"));
            add(a, "despesa", "ESFINGE-EXECUCAO", tab(f, &[&exec], Some(&chave_exec), None)?, &[&exec], &format!("; filtro do tribunal: ente {ente} (Campo Grande), Ano e anoMovimento {a}"))?;
            add(a, "despesa", "ESFINGE-DOTACAO", tab(f, &[&dot], Some(&chave_dot(f, a, mes_de(a, &m26))?), None)?, &[&dot], &format!("; filtro do tribunal: ente Campo Grande, ano {a}"))?;
            let ppa: &[&str] = if a == "2025" { &["ppa2022", "ppa2022b"] } else { &["ppa2026"] };
            add(a, "outro", &format!("ESFINGE-PPA-{}", &ppa[0][3..]), tab(f, ppa, None, None)?, ppa, "; nome das ações (PPA vigente no ano)")?;
            add(a, "receita", "ESFINGE-RECEITA", tab(f, &["rec_esf"], None, Some(&|r| r[0] == a))?, &["rec_esf"], &format!("; filtro do tribunal: ente Campo Grande; aqui só as linhas de ano = {a}"))?;
        }
        let filtro = |a: &str, t: &str| match (a, t) {
            ("2024", "despesa") => "todas as linhas de SICOM-DETALHE-EMPENHO (todas as unidades gestoras de Campo Grande)".to_string(),
            (_, "despesa") => format!("linhas de ESFINGE-EXECUCAO com tipoMovimento = Liquidacao e anoMes até {a}-{}", mes_de(a, &m26)),
            ("2024", _) => "SICOM-RECEITA-ACUMULADA: para cada unidade/rubrica/fonte só a linha do último mês; rubricas que começam com 9 (deduções) entram negativas".to_string(),
            ("2025", _) => "todas as linhas de ESFINGE-RECEITA (ano 2025)".to_string(),
            _ => format!("linhas de ESFINGE-RECEITA com anomes até 2026{m26}"),
        };
        let mut ad = Adaptador { pasta: p.clone(), tribunal: "TCE-MS".into(), arquivos: arqs, partes: &inteiro, codigos: None, faixa_mx: 6, filtro: filtros(&filtro),
            col: colunas("'saldo (Sum valor menos estorno)' só nas linhas tipoMovimento = Liquidacao (ESFINGE-EXECUCAO); 2024: 'liquidado (valor_liquidado - valor_anula_liquidacao)' (SICOM-DETALHE-EMPENHO)", "'valorOrcado (dotação inicial da LOA)' (ESFINGE-DOTACAO; 2024: o tribunal não publica dotação por ação = 0)", "'saldo (Sum valor menos estorno)' só nas linhas tipoMovimento = Empenho (ESFINGE-EXECUCAO)", "2025/2026: 'valor_arrecadado' (ESFINGE-RECEITA, deduções do Fundeb já negativas); 2024: 'Receita Acumulada' do último mês de cada unidade/rubrica/fonte, rubricas 9... subtraídas") };
        cod_texto::montar(o, "987", "Campo Grande", "MS", &mut ad, &dados)
    }
}

/// 985 Joao Pessoa (TCE-PB): SAGRES Dados Abertos (zips com um CSV) + SAGRES Online (JSON da API publica: despesas autorizadas
/// = orcado por dotacao; liquidacoes feitas depois do corte de 2026) -> .txt por ano e tipo. Os CSV de despesa vao com os mesmos
/// campos e uma 1a coluna codigo_acao_video (casar campo a campo dava falso positivo, ex. empenho n. 829 x acao 0829).
pub mod pb {
    use super::*;

    const DL: &str = "https://download.tce.pb.gov.br/dados-abertos/dados-por-municipio/<código do município>";
    const SO: &str = "https://sagresonline.tce.pb.gov.br";
    /// Os CSV de despesa somam ~140 MB: o deste ano (o maior) fica em _grandes (nao vai ao site).
    const GRANDE: &str = "2025";
    type Empenhos = BTreeMap<[String; 3], String>;

    /// O cod do leitor: UO.funcao.subfuncao.programa.acao.
    fn k(p: &[&str], o_que: &str) -> Result<String, String> {
        let [uo, f, sb, pr, ac] = p else { return Err(format!("{o_que}: classificação com menos de 5 partes")) };
        Ok(format!("{}.{:02}.{:03}.{}.{}", uo.trim(), u::int(f, "função")?, u::int(sb, "subfunção")?, pr.trim(), ac.trim()))
    }

    fn num(x: &str) -> String {
        let t = x.trim();
        if t.is_empty() || !t.bytes().all(|b| b.is_ascii_digit()) {
            return t.into();
        }
        let z = t.trim_start_matches('0');
        if z.is_empty() { "0".into() } else { z.into() }
    }

    /// O unico CSV de dentro do zip do tribunal.
    pub fn csv_do_zip(b: &[u8], nome: &str) -> Result<Vec<u8>, String> {
        let erro = |e: &dyn std::fmt::Display| format!("{nome}: {e}");
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(b)).map_err(|e| erro(&e))?;
        let ns: Vec<String> = z.file_names().map(String::from).collect();
        if ns.len() != 1 || !ns[0].ends_with(".csv") {
            return Err(format!("{nome}: esperava 1 CSV, veio {ns:?}"));
        }
        let mut raw = vec![];
        std::io::Read::read_to_end(&mut z.by_index(0).map_err(|e| erro(&e))?, &mut raw).map_err(|e| erro(&e))?;
        Ok(raw)
    }

    /// O CSV de despesa com os mesmos campos + 1a coluna codigo_acao_video, preenchida so nas linhas que o leitor soma
    /// (mes <= corte); `empenhos` recebe (UG, UO, n. do empenho) -> codigo, para as liquidacoes depois do corte de 2026.
    pub fn despesa(raw: Vec<u8>, nome: &str, corte: u32, mut empenhos: Option<&mut Empenhos>) -> Result<String, String> {
        let regs = d::csv_py(&utf8(raw, nome)?, ';');
        let Some((h, corpo)) = regs.split_first() else { return Err(format!("{nome}: CSV do tribunal vazio")) };
        let i = posicoes(h, &["mes", "codigo_unidade_gestora", "codigo_unidade_orcamentaria", "numero_empenho"], nome)?;
        let ck = posicoes(h, &["codigo_unidade_orcamentaria", "codigo_funcao", "codigo_subfuncao", "codigo_programa", "codigo_acao"], nome)?;
        let linha = |c: &str, r: &[String]| std::iter::once(c).chain(r.iter().map(String::as_str)).map(|x| if x.contains([';', '"', '\n', '\r']) { format!("\"{}\"", x.replace('"', "\"\"")) } else { x.to_string() }).collect::<Vec<_>>().join(";") + "\n";
        let mut out = linha("codigo_acao_video", h);
        for r in corpo {
            let cel = |j: usize| u::cel(r, j).map_err(|e| format!("{nome}: {e}"));
            let mut c = String::new();
            if u::ini(cel(i[0])?, 2).parse::<u32>().map_err(|_| format!("{nome}: mês que não é número: {:?}", r[i[0]]))? <= corte {
                c = k(&ck.iter().map(|j| cel(*j)).collect::<Result<Vec<_>, _>>()?, nome)?;
                if let Some(e) = empenhos.as_deref_mut() {
                    e.insert([cel(i[1])?.to_string(), cel(i[2])?.trim().to_string(), num(cel(i[3])?)], c.clone());
                }
            }
            out += &linha(&c, r);
        }
        Ok(out)
    }

    /// `consulta_ate` = o dia ("dd/mm/aaaa") em que o leitor consultou as liquidacoes depois do corte (no Python, fixo).
    pub fn montar(o: &Onde, f: &Fonte, consulta_ate: &str) -> Result<(PathBuf, Value), String> {
        let dados = o.pacote.join("RECEITAS-DESPESAS-JOAO-PESSOA-PB");
        let m26 = f.texto("mes26.txt")?.trim().to_string();
        let n26 = mes_valido(&m26)?;
        let (fim, depois) = (ultimo_dia("2026", n26)?, if n26 == 12 { "01/01/2027".to_string() } else { format!("01/{:02}/2026", n26 + 1) });
        let p = d::nova_pasta(o.site, "985", "João Pessoa")?;
        std::fs::create_dir(p.join("_grandes")).map_err(|e| e.to_string())?;
        let (mut arqs, mut emp) = (vec![], Empenhos::new());
        for a in ANOS {
            let (m, grande) = (mes_de(a, &m26), a == GRANDE);
            let meses = format!("mês 01 a {m}{}", if a == "2026" { " (o arquivo pode trazer meses depois do corte; esses ficam fora, sem codigo_acao_video)" } else { "" });
            let zipn = format!("despesas-{a}.zip");
            let t = despesa(csv_do_zip(&f.bytes(&zipn)?, &zipn)?, &zipn, u::int(m, "mês")? as u32, (a == "2026").then_some(&mut emp))?;
            let out = escreve(if grande { p.join("_grandes") } else { p.clone() }.join(format!("{a}-{m}_PB-SAGRES-DESPESAS.txt")), t.as_bytes())?;
            arqs.push(Arq { enviado: !grande, ..Arq::novo(a, "despesa", out, format!("TCE-PB, SAGRES Dados Abertos, despesas/despesas-{a}.zip ({DL}/despesas/despesas-{a}.zip), um registro por empenho; {meses}; 1a coluna codigo_acao_video = UO.função.subfunção.programa.ação (acrescentada){}", if grande { " - arquivo completo grande demais para o site: baixar no endereço acima" } else { "" })) });
            let arq = format!("dotacao{a}.json");
            let dj = f.json(&arq)?;
            let l = lista(&dj, &arq)?;
            if !l.is_empty() {
                let cods = l.iter().map(|x| k(&x["dotacao"].as_str().ok_or_else(|| format!("{arq}: registro sem dotacao"))?.split('.').take(5).collect::<Vec<_>>(), &arq)).collect::<Result<_, _>>()?;
                let out = d::gravar(&p, &format!("{a}-12_PB-SAGRES-DESPESAS-AUTORIZADAS.txt"), &com_coluna("codigo_acao_video", cods, tabela(l, &arq)?))?;
                arqs.push(Arq::novo(a, "despesa", out, format!("TCE-PB, SAGRES Online, {SO}/api/v2/municipal/orcamento/despesas-autorizadas?ano={a} (todas as unidades gestoras de João Pessoa); valorOrcado = dotação inicial; codigo_acao_video = 5 primeiras partes do campo dotacao")));
            }
            let zipn = format!("receitas-{a}.zip");
            let out = escreve(p.join(format!("{a}-{m}_PB-SAGRES-RECEITAS.txt")), &csv_do_zip(&f.bytes(&zipn)?, &zipn)?)?; // como veio
            arqs.push(Arq::novo(a, "receita", out, format!("TCE-PB, SAGRES Dados Abertos, receitas/receitas-{a}.zip ({DL}/receitas/receitas-{a}.zip); {meses} (coluna mes_ano)")));
        }
        let dep = f.json("liq_depois.json")?;
        let mut l = vec![vec![format!("codigo_acao_video (do empenho no CSV de 2026, mês 01 a {m26})"), "codigo_unidade_gestora".into(), "codigo_unidade_orcamentaria".into(), "numero_empenho".into(), format!("valorLiquidado depois de {fim} (somado por empenho)")]];
        for (ch, v) in dep.as_object().ok_or("liq_depois.json fora do formato")? {
            let ps: Vec<String> = ch.split('|').map(String::from).collect();
            let c = <[String; 3]>::try_from(ps.clone()).ok().and_then(|x| emp.get(&x).cloned()).unwrap_or_default();
            l.push([vec![c], ps, vec![d::cel_json(v)]].concat());
        }
        let out = d::gravar(&p, &format!("2026-{m26}_PB-SAGRES-LIQUIDACOES-DEPOIS-DO-CORTE.txt"), &l)?;
        arqs.push(Arq::novo("2026", "despesa", out, format!("TCE-PB, SAGRES Online, {SO}/api/v2/municipal/execucao-orcamentaria/liquidacoes (de {depois} até {consulta_ate}, cada unidade gestora): liquidações de empenhos de 2026 feitas depois do corte, descontadas do valor_liquidado (acumulado) do CSV de 2026")));
        let filtro = |a: &str, t: &str| match (a, t) {
            ("2026", "despesa") => format!("linhas do CSV de despesas 2026 com mes 01 a {m26} (as que têm codigo_acao_video), menos, de cada empenho dessas linhas, o liquidado depois de {fim} (arquivo LIQUIDACOES-DEPOIS-DO-CORTE)"),
            (_, "despesa") => format!("todas as linhas do CSV de despesas {a} (todas as unidades gestoras), meses 01 a 12"),
            _ => format!("todas as linhas do CSV de receitas {a} com mes_ano 01 a {}", mes_de(a, &m26)),
        };
        // festas do SAGRES = dezenas de empenhos espalhados: citar todas as faixas
        let mut ad = Adaptador { pasta: p, tribunal: "TCE-PB".into(), arquivos: arqs, partes: &inteiro, codigos: None, faixa_mx: 200, filtro: filtros(&filtro),
            col: colunas(&format!("'valor_liquidado' (CSV de despesas; em 2026 menos a coluna 'valorLiquidado depois de {fim}' do mesmo empenho)"), "'valorOrcado' (despesas autorizadas, somado nas naturezas da ação; 2026: o SAGRES Online ainda não tem a dotação)", "'valor_empenhado' (CSV de despesas)", "'valor' (CSV de receitas; deduções do Fundeb vêm negativas)") };
        d::montar(o.site, "985", "João Pessoa", "PB", &mut ad, &dados, o.desenha)
    }
}

/// 993 Recife (TCE-PE). 2024/2025: PDFs da Prestacao de Contas de Governo no e-TCEPE (etce<ano>/item04.pdf Balanco, item18.pdf
/// despesa por acao, item47.pdf LOA) vao como vieram, mais uma transcricao em .txt das linhas do Item 18 (com a pagina do PDF)
/// para o fontes.md citar linha a linha. 2026: Tome Conta, API de Dados Abertos (despesa2026-MM.json, receita2026.json).
pub mod pe {
    use super::*;
    use crate::link_tribunais_pe::{funcao, item18_linhas, loa_paginas, Pagina};

    const API: &str = "https://sistemas.tce.pe.gov.br/DadosAbertos";
    const ETCE: &str = "https://etce.tcepe.tc.br/epp/ConsultaPublica/listView.seam";
    /// Recife na API do TCE-PE.
    const MUN: &str = "P120";

    fn sub_divida(nome: &str) -> &'static str {
        match sem_acento(nome).split_whitespace().collect::<Vec<_>>().join(" ").as_str() {
            "refinanciamento da divida interna" => "841",
            "refinanciamento da divida externa" => "842",
            "servico da divida interna" => "843",
            "servico da divida externa" => "844",
            _ => "0",
        }
    }

    fn chave(r: &Value) -> Result<[String; 5], String> {
        let nome = r["FUNCAO"].as_str().unwrap_or("");
        let f = funcao(nome).ok_or_else(|| format!("TCE-PE 2026: função sem código na Portaria 42: {nome:?}"))?;
        Ok([pystr(&r["ID_UNIDADE_GESTORA"]), f.to_string(), pystr(&r["SUBFUNCAO"]), pystr(&r["PROGRAMA"]), pystr(&r["ACAO"])])
    }

    /// `paginas(arquivo)` = as palavras de cada pagina do PDF (ler PDF e de quem chama); `processos` = (ano, numero do
    /// processo da prestacao de contas no e-TCEPE), que no Python estava escrito a mao.
    pub fn montar(o: &Onde, f: &Fonte, paginas: &dyn Fn(&str) -> Result<Vec<Pagina>, String>, processos: &[(&str, &str)]) -> Result<(PathBuf, Value), String> {
        let dados = o.pacote.join("RECEITAS-DESPESAS-RECIFE-PE");
        let mes = corte26(&dados)?;
        let ate = mes_valido(&mes)?;
        let p = d::nova_pasta(o.site, "993", "Recife")?;
        let mut arqs = vec![];
        for ano in ["2024", "2025"] {
            let pdf = |item: &str, rot: &str| f.copia(&format!("etce{ano}/{item}.pdf"), &p, &format!("{ano}-12_PE-ETCEPE-{rot}.pdf"));
            let (pdf18, pdf47, pdf04) = (pdf("item18", "ITEM18-DESPESA-POR-ACAO")?, pdf("item47", "ITEM47-LOA")?, pdf("item04", "ITEM04-BALANCO-ORCAMENTARIO")?);
            let (loa, _) = loa_paginas(&paginas(&format!("etce{ano}/item47.pdf"))?, &format!("Item 47 de {ano}"))?;
            let (rows, nomes) = item18_linhas(&paginas(&format!("etce{ano}/item18.pdf"))?, &format!("Item 18 de {ano}"))?;
            // a dotacao da LOA vai na 1a funcao (em ordem) de cada acao, como no leitor
            let mut prim: BTreeMap<&str, &str> = BTreeMap::new();
            for r in &rows {
                let e = prim.entry(&r.acao).or_insert(&r.funcao);
                *e = (*e).min(&r.funcao);
            }
            let mut linhas = vec![s(&["codigo_acao_video", "página do PDF Item 18", "AÇÃO", "NOME DA AÇÃO", "FUNÇÃO", "DESPESA REALIZADA (Item 18)", "DOTAÇÃO INICIAL (LOA, Item 47)", "página do PDF Item 47"])];
            let mut usou = BTreeSet::new();
            for r in &rows {
                let (mut dot, mut pg) = (String::new(), String::new());
                if prim[r.acao.as_str()] == r.funcao && !usou.contains(&r.acao) {
                    if let Some((t, n)) = loa.get(&r.acao) {
                        (dot, pg) = (t.clone(), format!("p. {n}"));
                        usou.insert(&r.acao);
                    }
                }
                // "p. N": pagina so numerica casava com o codigo da funcao
                linhas.push(vec![format!("{}.{}.0.0", r.acao, r.funcao), format!("p. {}", r.pagina), r.acao.clone(), nomes.get(&r.acao).cloned().unwrap_or_default(), r.funcao.clone(), r.valor.clone(), dot, pg]);
            }
            let txt = d::gravar_com_aspas(&p, &format!("{ano}-12_PE-ETCEPE-ITEM18-TRANSCRICAO.txt"), &linhas)?;
            let base = format!("e-TCEPE, Prestação de Contas de Governo {ano} da Prefeitura do Recife{} ({ETCE})", processos.iter().find(|x| x.0 == ano).map(|x| format!(", processo {}", x.1)).unwrap_or_default());
            arqs.extend([
                Arq::novo(ano, "despesa", txt, format!("transcrição linha a linha do Item 18 ({}, coluna 'página do PDF Item 18') com a dotação inicial da LOA ({}, coluna 'página do PDF Item 47'; vai na 1a função da ação); 1a coluna codigo_acao_video = ação.função.0.0, o código do vídeo", nome_de(&pdf18), nome_de(&pdf47))),
                Arq::novo(ano, "despesa", pdf18, format!("{base}: Item 18, despesa realizada por projeto/atividade e função")),
                Arq::novo(ano, "despesa", pdf47, format!("{base}: Item 47, LOA {ano} (dotação inicial por ação)")),
                Arq::novo(ano, "receita", pdf04, format!("{base}: Item 4, Balanço Orçamentário consolidado")),
            ]);
        }
        // 2026: API de Dados Abertos (Tome Conta). A numeracao das acoes e a do leitor: posicao na ordem das chaves.
        let meses = (1..=ate).map(|m| format!("despesa2026-{m:02}.json")).map(|n| f.json(&n).map(|v| (n, v))).collect::<Result<Vec<_>, _>>()?;
        let mut chaves = BTreeSet::new();
        for (n, v) in &meses {
            for r in lista(v, n)? {
                chaves.insert(chave(r)?);
            }
        }
        let cods: BTreeMap<&[String; 5], String> = chaves.iter().enumerate().map(|(i, k)| (k, format!("{}.{:02}.{i}.{}", k[0], k[1].parse::<i64>().unwrap_or(0), sub_divida(if k[2] == "None" { "" } else { &k[2] })))).collect();
        let (mut linhas, mut cab): (Vec<Vec<String>>, Vec<String>) = Default::default();
        for (n, v) in &meses {
            let l = lista(v, n)?;
            let t = tabela(l, n)?;
            if cab.is_empty() {
                cab = t[0].clone();
            }
            if t[0] != cab {
                return Err(format!("{n}: o cabeçalho do tribunal mudou de um mês para o outro"));
            }
            for (r, x) in l.iter().zip(&t[1..]) {
                linhas.push([vec![cods[&chave(r)?].clone()], x.clone()].concat());
            }
        }
        linhas.insert(0, [s(&["codigo_acao_video"]), cab].concat());
        let des = d::gravar_com_aspas(&p, &format!("2026-{mes}_PE-TOMECONTA-DESPESAS-MUNICIPAIS.txt"), &linhas)?;
        let r26 = f.json("receita2026.json")?;
        let ultimo = lista(&r26, "receita2026.json")?.iter().map(|x| int_py(&x["MESREFERENCIA"], "receita2026.json: mês")).collect::<Result<Vec<_>, _>>()?.into_iter().max().ok_or("receita2026.json sem registros")?;
        let rec = d::gravar_com_aspas(&p, &format!("2026-{ultimo:02}_PE-TOMECONTA-RECEITAS-MUNICIPAIS.txt"), &d::tabela_json(&r26))?;
        let uv = f.json("ugs2026.json")?;
        let ugs: BTreeSet<String> = lista(&uv, "ugs2026.json")?.iter().filter(|x| x["CODIGOMUNICIPIOUNIDADEGESTORA"] == MUN).map(|x| pystr(&x["ID_UNIDADEGESTORA"])).collect();
        arqs.extend([
            Arq::novo("2026", "despesa", des, format!("TCE-PE Tome Conta, Dados Abertos: {API}/DespesasMunicipais!json?CODIGO_MUNICIPIO={MUN}&ANOREFERENCIA=2026&MESREFERENCIA=<1..{ate}> (um registro por empenho; 1a coluna codigo_acao_video = código da ação no vídeo, montado como no leitor)")),
            Arq::novo("2026", "receita", rec, format!("TCE-PE Tome Conta, Dados Abertos: {API}/ReceitasMunicipais!json?ID_UNIDADE_GESTORA=<ug>&ANOREFERENCIA=2026 ({} órgãos do Recife)", ugs.len())),
        ]);
        let filtro = |a: &str, t: &str| match (a, t) {
            ("2026", "despesa") => format!("todas as linhas, MESREFERENCIA 1 a {ate} (a Câmara, UG 494, não tem empenho de 2026 no TCE-PE)"),
            ("2026", _) => format!("todas as linhas com MESREFERENCIA 1 a {ate}, deduções incluídas (negativas)"),
            (_, "despesa") => "todas as linhas da transcrição do Item 18 (soma = empenhado do Balanço, Item 4)".to_string(),
            _ => "Balanço Orçamentário consolidado: SUBTOTAL DAS RECEITAS, coluna Receitas Realizadas".to_string(),
        };
        // 1a coluna codigo_acao_video = cod do leitor (casar por campos soltos da falso positivo)
        let mut ad = Adaptador { pasta: p, tribunal: "TCE-PE".into(), arquivos: arqs, partes: &inteiro, codigos: None, faixa_mx: 6, filtro: filtros(&filtro),
            col: colunas("'DESPESA REALIZADA (Item 18)' em 2024/2025 (= empenhado do Balanço; o Item 18 não separa o liquidado) e 'VALORLIQUIDADO' em 2026 (somado nos meses)", "'DOTAÇÃO INICIAL (LOA, Item 47)' (2026: sem LOA no tribunal, vale 0)", "'VALOREMPENHADO' (somado nos meses)", "2024/2025: 'Receitas Realizadas' do Balanço Orçamentário, linha SUBTOTAL DAS RECEITAS; 2026: 'VALORLANCAMENTO' (deduções vêm negativas, DEDUCAORECEITA=9)") };
        let (pasta, rel) = d::montar(o.site, "993", "Recife", "PE", &mut ad, &dados, o.desenha)?;
        // o fontes.md dos setores de 2024/2025 dizia "Tome Conta"; esses numeros vem do Item 18 do e-TCEPE
        let fmd = d::ler_fontes(o.site, "993")?.0;
        for arq in [pasta.join(fmd.file_name().ok_or("fontes.md sem nome")?), fmd] {
            let t = std::fs::read_to_string(&arq).map_err(|e| format!("{}: {e}", arq.display()))?;
            escreve(arq, ["2024", "2025"].iter().fold(t, |t, a| t.replace(&format!("Tome Conta Despesas {a}-12:"), &format!("e-TCEPE Item 18 {a}:"))).as_bytes())?;
        }
        Ok((pasta, rel))
    }

    /// Soma as linhas achadas de CADA acao de acoes-*.json e compara com o liquidado (e mostra o total do ano).
    pub fn conferir(pacote: &Path, pasta: &Path) -> Result<Value, String> {
        let dados = pacote.join("RECEITAS-DESPESAS-RECIFE-PE");
        let (mut out, mut ruins) = (serde_json::Map::new(), 0);
        for ano in ANOS {
            let txt = acha(pasta, &format!("{ano}-*_PE-*{}*.txt", if ano == "2026" { "DESPESAS" } else { "ITEM18-TRANSCRICAO" }), false)?;
            let (t, idx) = (d::ler_linhas(&txt, None)?, d::indice(&txt, None)?);
            let cl = posicoes(t.first().ok_or("arquivo vazio")?, &[if ano == "2026" { "VALORLIQUIDADO" } else { "DESPESA REALIZADA (Item 18)" }], &nome_de(&txt))?[0];
            let val = |x: &str| if ano == "2026" { real(x, "TCE-PE") } else { x.replace('.', "").replace(',', ".").parse().map_err(|_| format!("TCE-PE: valor que não é número: {x:?}")) };
            let (ac, mut ruim) = (acoes(&dados, ano)?, vec![]);
            for x in &ac {
                let (c, liq) = liquidado(x)?;
                let so = soma(&t, &d::achar(&idx, c, &[c.to_string()]), cl, &val)?;
                if (so - liq).abs() > 0.01 {
                    ruim.push(json!([c, so, liq]));
                }
            }
            let tot = soma(&t, &(2..=t.len()).collect::<Vec<_>>(), cl, &val)?;
            ruins += ruim.len();
            out.insert(ano.into(), json!({ "acoes": ac.len(), "nao_bateram": ruim.iter().take(5).collect::<Vec<_>>(), "n_ruins": ruim.len(), "total_arquivo": r2(tot), "total_acoes": r2(ac.iter().filter_map(|x| x["liquidado"].as_f64()).sum()) }));
        }
        conferido("TCE-PE", ruins, out)
    }
}

/// 986 Teresina (TCE-PI): os PDFs da Documentacao Web usados pelo leitor (LOA, Anexo 6 do Balanco Geral, Relacao das notas de
/// empenhos) vao como vieram e viram tabela de texto pelas mesmas funcoes do leitor, com o PDF e a pagina na 1a coluna; a API
/// (despesa por funcao) e o painel de Receitas vao em tabela, uma linha por registro. O codigo da acao do video (o mesmo do
/// leitor) fica numa coluna propria, para o motor casar so a acao exata.
pub mod pi {
    use super::*;
    use crate::link_tribunais_pi::{anexo6, funcao, loa};

    const API: &str = "https://sistemas.tce.pi.gov.br/api/portaldacidadania";
    const PBI: &str = "https://wabi-brazil-south-api.analysis.windows.net/public/reports";
    const CHAVE_REC: &str = "436859aa-32bc-41cc-a664-314faa87bad9";
    const CHAVE: &str = "código da ação do vídeo (o mesmo do leitor; funções: 0.0.FF.000.0000.0000)";

    struct Doc {
        id: String,
        arq: String,
        envio: String,
    }

    /// Os documentos de um ano cujo nome comeca com `inicio`, em ordem de envio.
    fn docs(lst: &[Value], ano: &str, inicio: &str) -> Result<Vec<Doc>, String> {
        let mut ds = vec![];
        for x in lst {
            if pystr(&x["exercicio"]) == ano && u::txt(x, "documento")?.starts_with(inicio) {
                if x["idDocWebDownload"].is_null() {
                    return Err(format!("TCE-PI: documento {inicio} de {ano} sem id de download"));
                }
                ds.push((u::txt(x, "dataEnvio")?, pystr(&x["idDocWebDownload"])));
            }
        }
        ds.sort_by(|a, b| a.0.cmp(b.0));
        Ok(ds.into_iter().map(|(envio, id)| Doc { arq: format!("{id}.pdf"), id, envio: u::ini(envio, 10).to_string() }).collect())
    }

    type Especiais = BTreeMap<(i64, String), String>;

    /// (funcao, acao) -> codigo do video, e funcao -> codigo da linha da funcao, tirados do acoes-*.json do proprio video.
    fn mapa(dados: &Path, ano: &str, mes: &str) -> Result<(Especiais, BTreeMap<i64, String>), String> {
        let arq = dados.join("saida").join(format!("acoes-{ano}-{mes}.json"));
        let (mut esp, mut fun) = (BTreeMap::new(), BTreeMap::new());
        for x in lista(&js(&arq)?, &nome_de(&arq))? {
            let c = x["cod"].as_str().ok_or_else(|| format!("{}: ação sem código", arq.display()))?;
            let p: Vec<&str> = c.split('.').collect();
            let [_, _, f, sub, _, ac, ..] = p[..] else { return Err(format!("{}: código de ação fora do padrão do leitor: {c}", arq.display())) };
            let f = u::int(f, "função")?;
            if sub == "000" {
                fun.insert(f, c.to_string());
            } else {
                esp.insert((f, ac.to_string()), c.to_string());
            }
        }
        Ok((esp, fun))
    }

    /// "1,3" = as paginas (1 = a primeira) em que `tem` vale; nenhuma = "?".
    fn onde(pags: &[&str], tem: &dyn Fn(&str) -> bool) -> String {
        let v: Vec<String> = pags.iter().enumerate().filter(|(_, t)| tem(t)).map(|(i, _)| (i + 1).to_string()).collect();
        if v.is_empty() { "?".into() } else { v.join(",") }
    }

    /// `texto(arquivo.pdf)` = o texto do PDF como o leitor le (pdftotext -raw, paginas separadas por \f): e de quem chama.
    pub fn montar(o: &Onde, f: &Fonte, texto: &dyn Fn(&str) -> Result<String, String>) -> Result<(PathBuf, Value), String> {
        let dados = o.pacote.join("RECEITAS-DESPESAS-TERESINA-PI");
        let lv = f.json("documentos_133_pagina_1_qtdePorPagina_5000.json")?;
        let lst = lista(&lv, "documentos")?;
        let mut m22 = BTreeMap::new();
        for doc in docs(lst, "2026", "Relação das notas de empenhos emitidas")? {
            let n = format!("m22_{}.json", doc.id);
            let v = f.json(&n)?;
            let l: Vec<Vec<Value>> = linhas_de(&v[1], &n, 5)?.into_iter().cloned().collect();
            m22.insert(v[0].as_str().ok_or_else(|| format!("{n}: sem o mês"))?.to_string(), (doc, l)); // reenvio do mesmo mes: vale o ultimo (como no leitor)
        }
        let n26 = (1..=12).take_while(|m| m22.contains_key(&format!("{m:02}"))).count();
        if n26 == 0 {
            return Err("TCE-PI: nenhuma Relação de empenhos de 2026 (janeiro) guardada pelo leitor".into());
        }
        let mes26 = format!("{n26:02}");
        let p = d::nova_pasta(o.site, "986", "Teresina")?;
        let arqs = std::cell::RefCell::new(vec![]);
        let pdf = |ano: &str, mes: &str, rot: &str, doc: &Doc, nome_doc: &str| -> Result<String, String> {
            let out = f.copia(&doc.arq, &p, &format!("{ano}-{mes}_PI-{rot}.pdf"))?;
            let nome = nome_de(&out);
            arqs.borrow_mut().push(Arq::novo(ano, "pdf", out, format!("TCE-PI Documentação Web (sistemas.tce.pi.gov.br/documentacao-web/api/prestacao/download/{}), documento '{nome_doc}' enviado em {}", doc.id, doc.envio)));
            Ok(nome)
        };
        let txt = |ano: &str, mes: &str, tipo: &str, rot: &str, linhas: Vec<Vec<String>>, origem: String| -> Result<(), String> {
            arqs.borrow_mut().push(Arq::novo(ano, tipo, d::gravar(&p, &format!("{ano}-{mes}_PI-{rot}.txt"), &linhas)?, origem));
            Ok(())
        };
        let ultimo = |ano: &str, inicio: &str| docs(lst, ano, inicio)?.pop().ok_or_else(|| format!("TCE-PI: documento '{inicio}' de {ano} não está na lista do tribunal"));
        for ano in ANOS {
            let mes = mes_de(ano, &mes26);
            let (esp, fun) = mapa(&dados, ano, mes)?;
            // API: despesa do municipio inteiro por funcao
            let arq = format!("despesas_133_{ano}_lista_porFuncao.json");
            let api = f.json(&arq)?;
            let mut l = vec![s(&["arquivo do TCE-PI", CHAVE, "funcao", "código da função (Portaria 42/1999)", "empenhada", "liquidada", "paga"])];
            for x in lista(&api, &arq)? {
                let nf = u::txt(x, "funcao")?;
                let cf = funcao(nf)?;
                l.push(vec![format!("despesas/133/{ano}/lista/porFuncao"), fun.get(&cf).cloned().unwrap_or_default(), nf.to_string(), format!("{cf:02}"), campo(x, "empenhada", &arq)?, campo(x, "liquidada", &arq)?, campo(x, "paga", &arq)?]);
            }
            txt(ano, mes, "despesa", "API-DESPESA-POR-FUNCAO", l, format!("TCE-PI API do Portal da Cidadania ({API}/despesas/133/{ano}/lista/porFuncao): empenhado, liquidado e pago do município inteiro por função"))?;
            // LOA, Anexo VI (dotacao inicial por acao)
            let ld = ultimo(ano, "Lei Orçamentária Anual")?;
            let nome = pdf(ano, "12", "LOA", &ld, "Lei Orçamentária Anual - LOA")?;
            let t = texto(&ld.arq)?;
            let pags: Vec<&str> = t.split('\u{c}').collect();
            let mut acs: Vec<_> = loa(&t, &format!("LOA {ano} ({})", ld.arq))?.into_iter().collect();
            acs.sort_by(|a, b| a.1 .0.cmp(&b.1 .0));
            let mut l = vec![s(&["PDF do TCE-PI (página)", CHAVE, "código (13 dígitos)", "função", "projeto/atividade", "especificação", "dotação inicial"])];
            for (k, (c, nm, v)) in acs {
                let dot = if v.fract() == 0.0 { format!("{}", v as i64) } else { v.to_string() };
                l.push(vec![format!("{nome} p. {}", onde(&pags, &|t| t.contains(&c))), esp.get(&k).cloned().unwrap_or_default(), c.clone(), k.0.to_string(), k.1, nm, dot]);
            }
            txt(ano, "12", "despesa", "LOA-ANEXO-VI", l, format!("{nome} (Anexo VI da LOA {ano}) em tabela: uma linha por projeto/atividade, lida como no leitor (tribunais/pi.py, _loa)"))?;
            if ano != "2026" {
                let an = ultimo(ano, "Programa de trabalho (Anexo 6")?;
                let nome = pdf(ano, "12", "BALANCO-ANEXO-6", &an, "Programa de trabalho (Anexo 6) do Balanço Geral")?;
                let t = texto(&an.arq)?;
                let pags: Vec<&str> = t.split('\u{c}').collect();
                let mut acs: Vec<_> = anexo6(&t)?.into_iter().collect();
                acs.sort_by(|a, b| a.1 .1.cmp(&b.1 .1));
                let mut l = vec![s(&["PDF do TCE-PI (página)", CHAVE, "órgão", "código", "função", "projeto/atividade", "especificação", "empenhado (soma por função e ação)"])];
                for (k, (org, c, nm, v)) in acs {
                    let re = Regex::new(&format!(r"(?m)^{:02}\d{{7}}\.{} ", k.0, regex::escape(&k.1))).map_err(|e| e.to_string())?;
                    l.push(vec![format!("{nome} p. {}", onde(&pags, &|t| re.is_match(t))), esp.get(&k).cloned().unwrap_or_default(), org, c, k.0.to_string(), k.1, nm, pystr(&json!(v))]);
                }
                txt(ano, "12", "despesa", "BALANCO-ANEXO-6", l, format!("{nome} (Anexo 6 do Balanço Geral {ano}, consolidado) em tabela: despesa realizada (empenhada) por função e projeto/atividade, lida como no leitor (tribunais/pi.py, _anexo6)"))?;
            } else {
                let mut l = vec![s(&["PDF do TCE-PI (mês)", "nº de ordem no PDF", CHAVE, "órgão", "função", "projeto/atividade", "empenhado", "anulado"])];
                for (m, (doc, rows)) in m22.iter().filter(|x| *x.0 <= mes26) {
                    let nome = pdf(ano, m, "EMPENHOS-MODELO-XXII", doc, "Relação das notas de empenhos emitidas (Modelo XXII)")?;
                    for (i, r) in rows.iter().enumerate() {
                        let k = (int_py(&r[1], "TCE-PI: função")?, pystr(&r[2]));
                        l.push([vec![nome.clone(), (i + 1).to_string(), esp.get(&k).cloned().unwrap_or_default()], cels(r)].concat());
                    }
                }
                txt(ano, &mes26, "despesa", "EMPENHOS-MODELO-XXII", l, format!("Relação das notas de empenhos emitidas (Modelo XXII) de jan a {}/2026 em tabela: um empenho por linha, lido como no leitor (tribunais/pi.py, _modelo22, gravado em tce-pi/m22_<id>.json)", ABR[n26 - 1]))?;
            }
            let arq = format!("receita_painel_{ano}.json");
            let rec = f.json(&arq)?;
            let mut l = vec![s(&["arquivo do TCE-PI", "Unidade Gestora", "Origem", "Receita Bruta", "Deduções", "Receita Líquida"])];
            for x in lista(&rec, &arq)? {
                l.push(std::iter::once(Ok(arq.clone())).chain(["Unidade Gestora", "Origem", "Receita Bruta", "Deduções", "Receita Líquida"].iter().map(|k| campo(x, k, &arq))).collect::<Result<_, _>>()?);
            }
            txt(ano, mes, "receita", "PAINEL-RECEITAS", l, format!("TCE-PI painel Power BI 'Receitas Municipios - Portal da Cidadania' (aberto pelo Portal; {PBI}/{CHAVE_REC}), tabela SagresContabilReceita, Município Teresina, Ano {ano}"))?;
        }
        let rv = f.json("receitas_133.json")?;
        let mut l = vec![s(&["exercicio", "arrecadada", "prevista"])];
        for x in lista(&rv, "receitas_133.json")? {
            l.push(["exercicio", "arrecadada", "prevista"].iter().map(|k| campo(x, k, "receitas_133.json")).collect::<Result<_, _>>()?);
        }
        txt("2026", &mes26, "outro", "API-RECEITAS-POR-ANO", l, format!("TCE-PI API do Portal da Cidadania ({API}/receitas/133): arrecadado por ano (conferência do painel de Receitas)"))?;
        let liq = format!("'liquidada' (API-DESPESA-POR-FUNCAO; na função que tem ação de festas/benefícios, menos o empenhado dessas ações, que têm linha própria); ações de festas e benefícios: 'empenhado (soma por função e ação)' (BALANCO-ANEXO-6, 2024/2025) ou 'empenhado' menos 'anulado' (EMPENHOS-MODELO-XXII, meses 1 a {n26} de 2026), porque o TCE-PI não publica liquidado por ação");
        let mut ad = Adaptador { pasta: p.clone(), tribunal: "TCE-PI".into(), arquivos: arqs.take(), partes: &inteiro, codigos: None, faixa_mx: 6,
            col: colunas(&liq, "'dotação inicial' (LOA-ANEXO-VI)", "'empenhada' (API-DESPESA-POR-FUNCAO)", "'Receita Líquida' (PAINEL-RECEITAS), só a Unidade Gestora 'P. M. DE TERESINA' (consolidado)"),
            filtro: filtros(&|_, t| if t == "despesa" { "todas as linhas de API-DESPESA-POR-FUNCAO (município inteiro: Prefeitura, Câmara, IPMT, fundações e fundos)".to_string() } else { "linhas da Unidade Gestora 'P. M. DE TERESINA' (consolidado; as linhas da Câmara e do IPMT já estão nele, não somar)".to_string() }) };
        d::montar(o.site, "986", "Teresina", "PI", &mut ad, &dados, o.desenha)
    }
}

/// 994 Curitiba (TCE-PR, PIT, Download de Dados do SIM-AM). Os XML do PIT sao enormes (Empenho.xml ~140 MB por ano): os .txt
/// levam SO as colunas usadas, uma linha por registro, e o origem diz isso. Liquidado por acao: a EmpenhoLiquidacao nao tem a
/// classificacao; cada liquidacao vai com as colunas de classificacao do empenho dela (mesmo idEmpenho), a juncao do leitor.
pub mod pr {
    use super::*;
    use crate::link_tribunais_pr::{xml, Attrs, IBGE6};

    const URL: &str = "https://pit.tce.pr.gov.br/Arquivos";
    const COD: [&str; 7] = ["idPessoa", "cdOrgao", "cdUnidade", "cdFuncao", "cdSubFuncao", "cdPrograma", "cdProjetoAtividade"];
    const CLASS: [&str; 8] = ["idPessoa", "cdOrgao", "cdUnidade", "cdFuncao", "cdSubFuncao", "cdPrograma", "cdProjetoAtividade", "dsProjetoAtividade"];
    const EMP: [&str; 4] = ["idEmpenho", "nrEmpenho", "dtEmpenho", "vlEmpenho"];
    const LIQ: [&str; 4] = ["idLiquidacao", "idEmpenho", "dtLiquidacao", "vlLiquidacaoLiquido"];
    const REC: [&str; 9] = ["idpessoa", "nrAno", "nrMes", "idSumarioItem", "dsItem", "vlPrevisaoInicialDoExercicio", "vlPrevisaoAtualizadaDoExercicio", "vlRealizadoDoMes", "vlRealizadoAteOMes"];

    fn at<'a>(r: &'a Attrs, k: &str) -> Result<&'a str, String> {
        r.get(k).map(String::as_str).ok_or_else(|| format!("XML do TCE-PR sem o atributo {k}"))
    }

    fn limpos(r: &Attrs, ks: &[&str]) -> Result<Vec<String>, String> {
        ks.iter().map(|k| at(r, k).map(|v| v.trim().to_string())).collect()
    }

    /// Mes de uma data "AAAA-MM-DD...".
    fn mes(r: &Attrs, k: &str) -> Result<u32, String> {
        at(r, k)?.get(5..7).and_then(|m| m.parse().ok()).ok_or_else(|| format!("data fora do padrão em {k}: {:?}", r[k]))
    }

    pub fn montar(o: &Onde, f: &Fonte) -> Result<(PathBuf, Value), String> {
        let dados = o.pacote.join("RECEITAS-DESPESAS-CURITIBA-PR");
        let corte26 = mes_valido(&corte26(&dados)?)?;
        let p = d::nova_pasta(o.site, "994", "Curitiba")?;
        let todos = (f.nomes)()?;
        let (consorcio, de_liq) = (Regex::new("(?i)CONS[ÓO]RCIO").expect("regex fixa"), Regex::new(r"_EmpenhoLiquidacao(_\d+)?\.xml$").expect("regex fixa"));
        let mut arqs = vec![];
        for a in ANOS {
            let corte = if a == "2026" { corte26 } else { 12 };
            let (mm, zipd) = (format!("{corte:02}"), format!("{URL}/{a}/{a}_{IBGE6}_Despesa.zip"));
            let (mut emp, mut cls): (Vec<Vec<String>>, BTreeMap<String, Vec<String>>) = Default::default();
            xml(&f.texto(&format!("{a}_Despesa/{a}_{IBGE6}_Empenho.xml"))?, "Empenho", &mut |r| {
                // consorcio intermunicipal sediado na cidade nao e o municipio
                if consorcio.is_match(at(r, "nmEntidade")?) || mes(r, "dtEmpenho")? > corte {
                    return Ok(());
                }
                // o mesmo cod do leitor, sem tirar espaco
                let c = [vec![COD.iter().map(|k| at(r, k)).collect::<Result<Vec<_>, _>>()?.join(".")], limpos(r, &CLASS)?].concat();
                emp.push([limpos(r, &EMP)?, c.clone()].concat());
                cls.insert(at(r, "idEmpenho")?.to_string(), c);
                Ok(())
            })?;
            let pre = format!("{a}_Despesa/");
            let (mut liq, mut nomes) = (vec![], vec![]);
            for arq in todos.iter().filter(|n| n.strip_prefix(&pre).is_some_and(|r| !r.contains('/')) && de_liq.is_match(n)) {
                let nome = arq[pre.len()..].to_string();
                xml(&f.texto(arq)?, "EmpenhoLiquidacao", &mut |r| {
                    // liquidacao de restos a pagar nao casa com empenho do ano e fica fora
                    if let Some(c) = cls.get(at(r, "idEmpenho")?) {
                        if at(r, "dtLiquidacao")?.get(..4) == Some(a) && mes(r, "dtLiquidacao")? <= corte {
                            liq.push([vec![nome.clone()], limpos(r, &LIQ)?, c.clone()].concat());
                        }
                    }
                    Ok(())
                })?;
                nomes.push(nome);
            }
            if nomes.is_empty() {
                return Err(format!("TCE-PR: nenhum arquivo de liquidação de {a} guardado pelo leitor"));
            }
            let do_empenho: Vec<String> = CLASS.iter().map(|k| format!("{k} (do empenho)")).collect();
            let fl = d::gravar_com_aspas(&p, &format!("{a}-{mm}_PR-PIT-LIQUIDACOES.txt"), &[vec![[s(&["arquivo do PIT"]), s(&LIQ), s(&["codigo_acao_video (do empenho)"]), do_empenho].concat()], liq].concat())?;
            let fe = d::gravar_com_aspas(&p, &format!("{a}-{mm}_PR-PIT-EMPENHOS.txt"), &[vec![[s(&EMP), s(&["codigo_acao_video"]), s(&CLASS)].concat()], emp].concat())?;
            let rpre = format!("{a}_Receita/");
            let rx = todos.iter().find(|n| n.starts_with(&rpre) && n.ends_with("ReceitasConsolidado.xml")).ok_or_else(|| format!("TCE-PR: ReceitasConsolidado.xml de {a} não foi guardado pelo leitor"))?;
            let mut rec = vec![];
            xml(&f.texto(rx)?, "ReceitasConsolidado", &mut |r| {
                rec.push(limpos(r, &REC)?);
                Ok(())
            })?;
            let ultimo = rec.iter().map(|x| u::int(&x[2], "nrMes")).collect::<Result<Vec<_>, _>>()?.into_iter().max().ok_or_else(|| format!("{rx}: sem registros"))?;
            let fr = d::gravar_com_aspas(&p, &format!("{a}-{ultimo:02}_PR-PIT-RECEITAS-CONSOLIDADO.txt"), &[vec![s(&REC)], rec].concat())?;
            let ate = if a == "2026" { format!(" com data até {mm}/2026 (corte do vídeo)") } else { String::new() };
            arqs.extend([
                Arq::novo(a, "despesa", fl, format!("TCE-PR PIT, {zipd} -> {}: só as liquidações de {a}{ate} de empenhos do próprio ano (restos a pagar e consórcios intermunicipais fora, como no leitor), só as colunas {}; as colunas '(do empenho)' vêm do Empenho.xml pelo mesmo idEmpenho", nomes.join(", "), LIQ.join(", "))),
                Arq::novo(a, "despesa", fe, format!("TCE-PR PIT, {zipd} -> {a}_{IBGE6}_Empenho.xml: empenhos de {a}{ate} sem os consórcios intermunicipais, só as colunas usadas ({}); o XML completo tem entidade, credor, fonte e natureza; codigo_acao_video = {}, o código do vídeo", [EMP.join(", "), CLASS.join(", ")].join(", "), COD.join("."))),
                Arq::novo(a, "receita", fr, format!("TCE-PR PIT, {URL}/{a}/{a}_{IBGE6}_Receita.zip -> ReceitasConsolidado.xml, todas as linhas, só as colunas {}", REC.join(", "))),
            ]);
        }
        let filtro = |a: &str, t: &str| match (a, t) {
            ("2026", "despesa") => format!("todas as linhas (já só até {corte26:02}/2026)"),
            (_, "despesa") => "todas as linhas (arquivo LIQUIDACOES para o liquidado, EMPENHOS para o empenhado)".to_string(),
            _ => format!("nrMes {}, idSumarioItem 5010", if a == "2026" { corte26 } else { 12 }),
        };
        // coluna codigo_acao_video = cod do leitor (casar por campos soltos da falso positivo)
        let mut ad = Adaptador { pasta: p, tribunal: "TCE-PR".into(), arquivos: arqs, partes: &inteiro, codigos: None, faixa_mx: 6, filtro: filtros(&filtro),
            col: colunas("'vlLiquidacaoLiquido' do arquivo LIQUIDACOES", "- (o TCE-PR não publica dotação por ação)", "'vlEmpenho' do arquivo EMPENHOS (bruto: o TCE-PR não publica anulação)", "'vlRealizadoAteOMes' da linha idSumarioItem 5010 (RECEITAS EXCETO INTRA-ORÇAMENTÁRIAS) no último mês") };
        d::montar(o.site, "994", "Curitiba", "PR", &mut ad, &dados, o.desenha)
    }

    /// Soma a coluna liquidado das linhas achadas de CADA acao de acoes-*.json; totais do arquivo x totais.json (gastou, ja
    /// empenhou, arrecadou), lado a lado para quem confere.
    pub fn conferir(pacote: &Path, pasta: &Path) -> Result<Value, String> {
        let dados = pacote.join("RECEITAS-DESPESAS-CURITIBA-PR");
        let tv = js(&dados.join("saida").join("totais.json"))?;
        let tot = lista(&tv, "totais.json")?;
        let dito = |comeco: String| tot.iter().find(|x| x["dito"].as_str().is_some_and(|k| k.starts_with(&comeco))).map(|x| x["valor"].clone());
        let v = |x: &str| real(x, "TCE-PR");
        let (mut out, mut ruins) = (serde_json::Map::new(), 0);
        for a in ANOS {
            let txt = acha(pasta, &format!("{a}-*_PR-PIT-LIQUIDACOES.txt"), false)?;
            let (t, idx) = (d::ler_linhas(&txt, None)?, d::indice(&txt, None)?);
            let i = posicoes(t.first().ok_or("arquivo vazio")?, &["vlLiquidacaoLiquido"], &nome_de(&txt))?[0];
            let (ac, mut ruim) = (acoes(&dados, a)?, vec![]);
            for x in &ac {
                let (c, liq) = liquidado(x)?;
                let so = soma(&t, &d::achar(&idx, c, &[c.to_string()]), i, &v)?;
                if (so - liq).abs() > 0.01 {
                    ruim.push(json!([c, so, liq]));
                }
            }
            let te = d::ler_linhas(&acha(pasta, &format!("{a}-*_PR-PIT-EMPENHOS.txt"), false)?, None)?;
            let ie = posicoes(te.first().ok_or("arquivo vazio")?, &["vlEmpenho"], "EMPENHOS")?[0];
            let tr = d::ler_linhas(&acha(pasta, &format!("{a}-*_PR-PIT-RECEITAS-CONSOLIDADO.txt"), false)?, None)?;
            let m = if a == "2026" { u::int(u::ini(&nome_de(&txt), 7).get(5..).unwrap_or(""), "mês do arquivo")? } else { 12 };
            let mut arrecadado = vec![];
            for x in tr.iter().skip(1).filter(|x| x.get(3).is_some_and(|c| c == "5010")) {
                if u::int(u::cel(x, 2)?, "nrMes")? == m {
                    arrecadado.push(v(u::cel(x, 8)?)?);
                }
            }
            let nao = |o_que: &str| format!("totais.json sem '{o_que} ({a}'");
            ruins += ruim.len();
            out.insert(a.into(), json!({ "acoes": ac.len(), "nao_bateram": ruim.len(), "ex": ruim.iter().take(3).collect::<Vec<_>>(),
                "liquidado_arquivo": r2(soma(&t, &(2..=t.len()).collect::<Vec<_>>(), i, &v)?), "gastou_video": dito(format!("Gastou ({a}")).ok_or_else(|| nao("Gastou"))?,
                "empenhado_arquivo": r2(soma(&te, &(2..=te.len()).collect::<Vec<_>>(), ie, &v)?), "empenhou_video": dito(format!("Já empenhou ({a}")),
                "arrecadado_arquivo": arrecadado, "arrecadou_video": dito(format!("Arrecadou ({a}")).ok_or_else(|| nao("Arrecadou"))? }));
        }
        conferido("TCE-PR", ruins, out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link_docs_cap::tests::{cenario, linhas, nada};
    use crate::link_docs_capitais::tests::grava;

    fn txts(v: &[&str]) -> Vec<String> {
        s(v)
    }

    #[test]
    fn palmas_e_porto_alegre_com_o_corte_por_parametro() {
        let cod = "10.13.392.20.2050";
        let (site, pacote, fontes) = cenario("to2", "975", "RECEITAS-DESPESAS-PALMAS-TO", cod, cod);
        let src = site.parent().unwrap().join("src");
        let html = |ug: &str| format!("<table><tr><th>a</th><th>b</th><th>c</th><th>Órgão</th><th>e</th><th>Função</th><th>Sub</th><th>Prog</th><th>i</th><th>Ação</th><th>Nome</th></tr>\n<tr><td>{ug}'</td><td></td><td></td><td>10</td><td></td><td>13</td><td>392</td><td>20</td><td></td><td>2050</td><td>Promoção'</td></tr></table>");
        for a in ANOS {
            grava(&src.join(format!("balanceteDespesa-{a}-08-111.xls")), &html("x"));
            grava(&src.join(format!("balanceteDespesa-{a}-08-222.xls")), &html("y"));
            grava(&src.join(format!("balanceteReceita-{a}-08-111.xls")), "<table><tr><td>Conta</td><td>Valor</td></tr><tr><td>A</td><td>10,00</td></tr></table>");
        }
        let (le, nomes) = da_pasta(&src);
        let (o, f) = (Onde { site: &site, pacote: &pacote, desenha: &nada }, Fonte { le: &le, nomes: &nomes });
        let corte = to::Corte { mes26: "08", principal: "111", consolidada: &[("2024", 8), ("2025", 14)] };
        let (p, rel) = to::montar(&o, &f, &corte).unwrap();
        assert!(p.ends_with("docs/975-palmas-docs") && rel["arquivos"].as_array().unwrap().len() == 6 && rel["codigos_nao_achados"] == 0);
        // o texto do SICAP (UTF-8 lido como latin-1, ' no fim) volta ao normal; uma linha por unidade gestora, com o arquivo dela
        let t = linhas(&p.join("2026-08_TO-BALANCETE-DESPESA.txt"));
        assert_eq!((t[0][..2].to_vec(), t[0][5].as_str(), t.len()), (txts(&["codigo_acao_video", "arquivo do TCE-TO (unidade gestora)"]), "Órgão", 3));
        assert_eq!((t[1][..3].to_vec(), t[1][12].as_str(), t[2][1].as_str()), (txts(&[cod, "balanceteDespesa-2026-08-111.xls", "x"]), "Promoção", "balanceteDespesa-2026-08-222.xls"));
        let md = std::fs::read_to_string(&fontes).unwrap();
        assert!(md.contains("| soma da coluna 'Valor Liquidado' (somado nas remessas 1..N) · remessa 14 Consolidada da Prefeitura (CNPJ 111), remessas 1 a 14 |"), "{md}");
        assert!(md.contains("2025-12_TO-BALANCETE-DESPESA.txt linhas 2-3 |"));
        // ano sem a remessa Consolidada informada e linha cortada: para
        assert!(to::montar(&o, &f, &to::Corte { consolidada: &[("2024", 8)], ..corte }).unwrap_err().contains("2025 sem o número da remessa"));
        grava(&src.join("balanceteDespesa-2024-08-111.xls"), "<table><tr><td>a</td></tr><tr><td>1</td></tr></table>");
        assert!(to::montar(&o, &f, &corte).unwrap_err().contains("balanceteDespesa-2024-08-111.xls: linha da planilha do tribunal com 1 colunas"));
        let _ = std::fs::remove_dir_all(site.parent().unwrap());

        let cod = "54900.7.1.13.392.20.1753";
        let (site, pacote, fontes) = cenario("rs2", "991", "RECEITAS-DESPESAS-PORTO-ALEGRE-RS", cod, cod);
        let src = site.parent().unwrap().join("src");
        for a in ANOS {
            grava(&src.join(format!("despesa_{a}_54900.csv")), "\u{feff}CD_ORGAO,CD_ORGAO_ORCAMENTARIO,CD_UNIDADE_ORCAMENTARIA,CD_FUNCAO,CD_SUBFUNCAO,CD_PROGRAMA,CD_PROJETO,NOME,VL_LIQUIDADO\n54900,7,1,13,392,20,1753,\"FESTA, A\",5.0\n54900,7,1,13,392,20,1754,1753,1.0\n");
            grava(&src.join(format!("receita_{a}_54900.csv")), "NIVEL,VL_ARRECADADO\nA,10\n");
        }
        let (le, nomes) = da_pasta(&src);
        let (o, f) = (Onde { site: &site, pacote: &pacote, desenha: &nada }, Fonte { le: &le, nomes: &nomes });
        let (p, _) = rs::montar(&o, &f, "07").unwrap();
        let t = linhas(&p.join("2026-07_RS-BALANCETE-DESPESA.txt"));
        assert_eq!((t[0][1].as_str(), t[1][..3].to_vec(), t[1][9].as_str()), ("código da ação (montado: CD_ORGAO.CD_ORGAO_ORCAMENTARIO.CD_UNIDADE_ORCAMENTARIA.CD_FUNCAO.CD_SUBFUNCAO.CD_PROGRAMA.CD_PROJETO)", txts(&["2026/54900.csv (Prefeitura)", cod, "54900"]), "FESTA, A"));
        assert_eq!(linhas(&p.join("2025-12_RS-BALANCETE-RECEITA.txt"))[1], ["2025/54900.csv (Prefeitura)", "", "A", "10"]);
        // so a linha da acao exata (a de baixo tem o 1753 noutra coluna); o filtro diz os orgaos que tem arquivo
        let md = std::fs::read_to_string(&fontes).unwrap();
        assert!(md.contains("2025-12_RS-BALANCETE-DESPESA.txt linhas 2 |") && md.contains("· todas as linhas dos 1 órgãos (54900 Prefeitura) (sem balancete de despesa no tribunal: Câmara, DMAE,"), "{md}");
        assert!(rs::montar(&o, &f, "13").unwrap_err().contains("mês de corte inválido"));
        grava(&src.join("despesa_2024_54900.csv"), "X\n1\n");
        assert!(rs::montar(&o, &f, "07").unwrap_err().contains("despesa_2024_54900.csv: arquivo do tribunal sem a coluna CD_ORGAO"));
        let _ = std::fs::remove_dir_all(site.parent().unwrap());
    }

    #[test]
    fn florianopolis_numera_como_o_leitor_e_rio_branco_pela_consulta() {
        let ug = "Fundação Cultural de Florianópolis Franklin Cascaes";
        let cod = format!("{}.0.13.392.0.2050", crate::link_tribunais_sc::sigla(ug).unwrap());
        let (site, pacote, fontes) = cenario("sc2", "981", "RECEITAS-DESPESAS-FLORIANOPOLIS-SC", "0.13.392.0.2050", &cod);
        let src = site.parent().unwrap().join("src");
        grava(&pacote.join("RECEITAS-DESPESAS-FLORIANOPOLIS-SC/saida/festas-cods.json"), &json!({ "2025": [cod] }).to_string());
        let (mut dj, mut rj) = (json!({}), json!({}));
        for a in ANOS {
            dj[a] = json!({ "acoes": [[ug, 13, 392, "2050", "FESTA", 5.0, 6.0], [ug, 13, 392, "2050", "OUTRA", 1.0, 1.0], [ug, "13", "392", "2051", "ZERADA", 0, "-"], [ug, 13, 392, "2050", "FESTA", 2.0, 0.0]], "total": [["FLORIANÓPOLIS", 8.0, 7.0]] });
            rj[a] = json!({ "rubricas": [["1112", 10.0]], "total": [["FLORIANÓPOLIS", 10.0]] });
        }
        grava(&src.join("despesa-08.json"), &dj.to_string());
        grava(&src.join("receita-08.json"), &rj.to_string());
        grava(&src.join("remessas.json"), &json!([["Prefeitura", "202608", "Registros Contábeis", "Enviado", 1]]).to_string());
        let (le, nomes) = da_pasta(&src);
        let (o, f) = (Onde { site: &site, pacote: &pacote, desenha: &nada }, Fonte { le: &le, nomes: &nomes });
        let (p, rel) = sc::montar(&o, &f, "08").unwrap();
        let t = linhas(&p.join("2026-08_SC-ESFINGE-DESPESAS-MUNICIPAIS.txt"));
        // mesmo numero com outro nome ganha -2; linha zerada fica sem codigo; a mesma acao repete o codigo
        assert_eq!(t[1..].iter().map(|r| r[0].clone()).collect::<Vec<_>>(), [cod.clone(), format!("{cod}-2"), String::new(), cod.clone(), String::new()]);
        assert_eq!((t[1][1..5].to_vec(), t[5].clone()), (txts(&["ações", ug, "13", "392"]), txts(&["", "total do município (Ente)", "FLORIANÓPOLIS", "", "", "", "", "8.0", "7.0"])));
        assert_eq!(linhas(&p.join("2025-12_SC-ESFINGE-RECEITAS-MUNICIPAIS.txt"))[1..], [txts(&["rubricas", "1112", "10.0"]), txts(&["total do município (nome_ente)", "FLORIANÓPOLIS", "10.0"])]);
        let md = std::fs::read_to_string(&fontes).unwrap();
        assert!(md.contains("2025-12_SC-ESFINGE-DESPESAS-MUNICIPAIS.txt linhas 2, 5 |") && md.contains(&format!("({cod}, 2025-12_SC-ESFINGE-DESPESAS-MUNICIPAIS.txt linhas 2, 5)")), "{md}");
        assert!(rel["codigos_nao_achados"] == 0 && p.join("2026-08_SC-ESFINGE-REMESSAS-2026.txt").exists());
        assert_eq!((ultimo_dia("2024", 2).unwrap(), ultimo_dia("2026", 9).unwrap()), ("29/02/2024".to_string(), "30/09/2026".to_string()));
        dj["2025"]["acoes"] = json!([[ug, 13, 392, "2050", "FESTA", 5.0]]);
        grava(&src.join("despesa-08.json"), &dj.to_string());
        assert!(sc::montar(&o, &f, "08").unwrap_err().contains("despesa 2025, ações: linha fora do formato"));
        assert!(sc::montar(&o, &f, "09").unwrap_err().contains("despesa-09.json: o leitor não guardou"));
        let _ = std::fs::remove_dir_all(site.parent().unwrap());

        let cod = "0.0.13.392.0.2050";
        let (site, pacote, fontes) = cenario("ac2", "977", "RECEITAS-DESPESAS-RIO-BRANCO-AC", cod, cod);
        let o = Onde { site: &site, pacote: &pacote, desenha: &nada };
        let pedidas = std::cell::RefCell::new(vec![]);
        let cubo = |tipo: &str, dims: &[&str], meas: &[String]| -> Result<String, String> {
            pedidas.borrow_mut().push(meas[0].clone());
            assert_eq!(dims.len(), if tipo == "despesa" { 6 } else { 3 });
            Ok(if tipo == "despesa" { json!([["PREFEITURA", "1", "13", "392", "02050", "FESTA", 5.0, 6.0], ["PREFEITURA", "2", "13", "392", "A1", "X", 0, 0.0], ["CAMARA", "1", "1", "31", "A1", "Y", 1.5, null]]) } else { json!([["PREFEITURA", "1", "1112", 10.0, -1.0]]) }.to_string())
        };
        let (p, rel) = ac::montar(&o, &cubo, &["2024", "2025"]).unwrap();
        assert_eq!(pedidas.borrow()[0], "Sum({<NOME_ENTE={'RIO BRANCO'},ANO_REMESSA={2024},ano_empenho={2024}>}valor_liquidado)");
        // a linha toda zerada sai; o filtro some do nome da coluna
        assert_eq!(linhas(&p.join("2025-12_AC-REMESSAS-DESPESA.txt")), [txts(&["codigo_acao_video", "entidade", "NUMERO_REMESSA", "numero_funcao", "numero_subfuncao", "CODIGO_ACAO", "TITULO_ACAO", "valor_liquidado", "VALOR_EMPENHO"]), txts(&[cod, "PREFEITURA", "1", "13", "392", "02050", "FESTA", "5.0", "6.0"]), txts(&["0.0.01.031.0.A1", "CAMARA", "1", "1", "31", "A1", "Y", "1.5", ""])]);
        assert_eq!(linhas(&p.join("2024-12_AC-REMESSAS-RECEITA.txt"))[0], ["ENTIDADE.NOME", "MES", "CODIGO-NUMERO_NATUREZA_RECEITA", "RECEITA_REALIZADA", "DEDUCAO"]);
        assert!(rel["arquivos"].as_array().unwrap().len() == 4 && std::fs::read_to_string(&fontes).unwrap().contains("· orçado · coluna - · 2025-12_AC-REMESSAS-DESPESA.txt linhas 2 |"));
        assert!(ac::montar(&o, &cubo, &["2025", "2026"]).unwrap_err().contains("só têm montagem para ano inteiro"));
        assert!(ac::montar(&o, &|_, _, _| Ok(json!([["P", "1", "treze", "392", "1", "F", 5.0, 6.0]]).to_string()), &["2025"]).unwrap_err().contains("função que não é número"));
        assert!(ac::montar(&o, &|_, _, _| Ok("[]".into()), &["2025"]).unwrap_err().contains("consulta sem linhas"));
        let _ = std::fs::remove_dir_all(site.parent().unwrap());
    }

    #[test]
    fn manaus_monta_e_confere() {
        let (festa, ug) = ("7.70.13.392.0020.2050", "7.0.13.392.0000.0000");
        let (site, pacote, fontes) = cenario("am2", "995", "RECEITAS-DESPESAS-MANAUS-AM", festa, ug);
        let (src, saida) = (site.parent().unwrap().join("src"), pacote.join("RECEITAS-DESPESAS-MANAUS-AM/saida"));
        let levantamento = |liq: f64| json!([{ "cod": ug, "acao": "CULTURA (liquidado da unidade gestora)", "liquidado": liq }, { "cod": festa, "acao": "FESTA", "liquidado": 5.0, "orcado": 9.0 }]).to_string();
        grava(&src.join("unidades.json"), &json!([{ "id_unidade_gestora": 7, "idMunicipio": 1, "esfera": 2 }, { "id_unidade_gestora": 8, "idMunicipio": 2, "esfera": 2 }]).to_string());
        for (a, m) in [("2024", "12"), ("2025", "12"), ("2026", "08")] {
            let n: i64 = a.parse().unwrap();
            grava(&saida.join(format!("acoes-{a}-{m}.json")), &levantamento(4.0));
            let class = |prog: Value| json!({ "codUnidadeOrcamentaria": 70, "codFuncaoGoverno": 13, "codSubfuncaoGoverno": 392, "codProgramaGoverno": prog, "codAcaoGoverno": prog.as_i64().map(|_| 2050), "desAcaoGoverno": "Festa " });
            let com = |mut x: Value, mais: Value| {
                mais.as_object().unwrap().iter().for_each(|(k, v)| x[k] = v.clone());
                x
            };
            grava(&src.join(format!("dotacoes_7_{a}.json")), &json!([com(class(json!(20)), json!({ "vlDotacao": 9.0, "codOrigemDotacaoOrcamentaria": 1 })), com(class(json!(20)), json!({ "vlDotacao": 3.0, "codOrigemDotacaoOrcamentaria": 2 }))]).to_string());
            // pagamento sem programa/acao: o codigo vem da dotacao com o mesmo nome; resto a pagar de outro ano fica fora
            grava(&src.join(format!("pagamentos_7_{a}_1.json")), &json!([com(class(Value::Null), json!({ "exercicio": n, "vlPagamentoEmpenho": 5.0, "desAcaoGoverno": "FESTA" })), com(class(json!(20)), json!({ "exercicio": n - 1, "vlPagamentoEmpenho": 100.0 }))]).to_string());
            grava(&src.join(format!("movimentacoesContabeis_7_{a}_1.json")), &json!([{ "codContaContabil": "622130300", "vlCredito": 10.0, "vlDebito": 6.0 }, { "codContaContabil": "622130100", "vlCredito": 50.0, "vlDebito": 0.0 }, { "codContaContabil": "111", "vlCredito": 1.0, "vlDebito": 0.0 }]).to_string());
            grava(&src.join(format!("receitasArrecadadas_7_{a}.json")), &json!([{ "codCompetencia": n * 100 + 3, "vlLancado": 10.0 }, { "codCompetencia": n * 100 + 9, "vlLancado": 1.0 }]).to_string());
        }
        let (le, nomes) = da_pasta(&src);
        let (o, f) = (Onde { site: &site, pacote: &pacote, desenha: &nada }, Fonte { le: &le, nomes: &nomes });
        let (p, rel) = am::montar(&o, &f).unwrap();
        assert!(rel["arquivos"].as_array().unwrap().len() == 12 && p.join("_grandes/2024-12_AM-ECONTAS-PAGAMENTOS.txt").exists() && !p.join("2024-12_AM-ECONTAS-PAGAMENTOS.txt").exists());
        let t = linhas(&p.join("2025-12_AM-ECONTAS-PAGAMENTOS.txt"));
        assert_eq!((t.len(), t[1][..4].to_vec()), (2, txts(&[festa, "pagamentos/7/2025/1", "", "2025"])));
        let t = linhas(&p.join("2026-08_AM-ECONTAS-BALANCETES.txt"));
        assert_eq!(t[1..].iter().map(|r| (r[0].as_str(), r[9].as_str())).collect::<Vec<_>>(), [(ug, "LIQUIDADO e EMPENHADO"), ("", "EMPENHADO"), ("", "")]);
        assert_eq!((linhas(&p.join("2026-08_AM-ECONTAS-RECEITAS-ARRECADADAS.txt")).len(), linhas(&p.join("2025-12_AM-ECONTAS-RECEITAS-ARRECADADAS.txt")).len(), linhas(&p.join("2025-12_AM-ECONTAS-DOTACOES.txt"))[2][0].as_str()), (2, 3, festa));
        assert!(std::fs::read_to_string(&fontes).unwrap().contains("2025-12_AM-ECONTAS-DOTACOES.txt linhas 2-3; 2025-12_AM-ECONTAS-PAGAMENTOS.txt linhas 2 |"));
        let c = am::conferir(&pacote, &p).unwrap();
        assert_eq!((&c["2025"]["nao_bateram"], &c["2025"]["liquidado_balancete"], &c["2024"]["festas (cod, ação, pago achado, liquidado json, orçado achado, orçado json)"]), (&json!([]), &json!(4.0), &json!([[festa, "FESTA", 5.0, 5.0, 9.0, 9.0]])));
        // o levantamento diz outro numero: para, com o relatorio
        grava(&saida.join("acoes-2025-12.json"), &levantamento(6.0));
        let e = am::conferir(&pacote, &p).unwrap_err();
        assert!(e.starts_with("TCE-AM: 1 ação(ões) não bateram") && e.contains("-2.0"), "{e}");
        std::fs::remove_file(src.join("unidades.json")).unwrap();
        assert!(am::montar(&o, &f).unwrap_err().contains("unidades.json: o leitor não guardou"));
        let _ = std::fs::remove_dir_all(site.parent().unwrap());
    }

    #[test]
    fn campo_grande_codigo_do_leitor_e_dotacao_na_acao_com_mais_liquidado() {
        let cam = "0.0.01.031.0.2001.01.031.CAM";
        let (site, pacote, fontes) = cenario("ms2", "987", "RECEITAS-DESPESAS-CAMPO-GRANDE-MS", cam, cam);
        let src = site.parent().unwrap().join("src");
        let g = |n: &str, v: Value| grava(&src.join(format!("{n}.json")), &v.to_string());
        let mut rec: Vec<Value> = (1..=8).map(|m| json!(["2026", format!("2026{m:02}"), "PREF", "1112", "0", 1.0])).collect();
        rec.push(json!(["2025", "202503", "PREF", "1112", "0", 10.0]));
        g("rec_esf", json!(rec));
        for a in ["2025", "2026"] {
            let l = |ug: &str, nome: &str, fu: &str, sub: &str, cod: &str, tipo: &str, mes: &str, v: f64| json!([ug, nome, fu, sub, cod, tipo, format!("{a}-{mes}"), format!("{a}04"), v]);
            g(&format!("exec{a}"), json!([l("1", "PREFEITURA MUNICIPAL", "13-Cultura", "392-Difusão", "2050", "Liquidacao", "03", 5.0), l("1", "PREFEITURA MUNICIPAL", "04-Adm", "122-Geral", "2050", "Liquidacao", "03", 7.0), l("2", "CÂMARA MUNICIPAL DE CG", "01-Legislativa", "031-Ação", "2001", "Empenho", "02", 1.0), l("1", "PREFEITURA MUNICIPAL", "13-Cultura", "392-Difusão", "2050", "Liquidacao", "12", 100.0)]));
            g(&format!("dot{a}"), json!([["2050", 9.0, 9.5], ["9999", 1.0, 1.0]]));
        }
        for n in ["ppa2022", "ppa2022b", "ppa2026"] {
            g(n, json!([["2050", "2050 - FESTA", "13", "392", 1]]));
        }
        g("des2024", json!([["PREFEITURA", "Cultura", "Difusão Cultural", "2050", 5.0, 6.0], ["CAMARA MUNICIPAL", "Legislativa", "Ação Legislativa", "2001", 1.0, 1.0]]));
        g("nomes2024", json!([["PREFEITURA", "Cultura", "Difusão Cultural", "13", "392", "2050", "FESTA", 3]]));
        g("funcoes", json!({ "f": [["Legislativa", "01", 1], ["Cultura", "99", 1]], "s": [["Ação Legislativa", "031", 1]] }));
        g("rec2024", json!([["PREF", "1112", "IPTU", "1500", "DEZEMBRO", 10.0]]));
        g("ente", json!(["123"]));
        let (le, nomes) = da_pasta(&src);
        let (o, f) = (Onde { site: &site, pacote: &pacote, desenha: &nada }, Fonte { le: &le, nomes: &nomes });
        let (p, rel) = ms::montar(&o, &f).unwrap();
        // 2025 (ate dezembro) a cultura tem mais liquidado; 2026 (ate agosto) a administracao; acao so com dotacao fica 0.0
        let dot = |a: &str| linhas(&p.join(format!("{a}_MS-ESFINGE-DOTACAO.txt")))[1..].iter().map(|r| r[..3].join("|")).collect::<Vec<_>>();
        assert_eq!((dot("2025-12"), dot("2026-08")), (txts(&["dot2025.json|0.0.13.392.0.2050.13.392|2050", "dot2025.json|0.0.0.0.0.9999.0.0|9999"]), txts(&["dot2026.json|0.0.04.122.0.2050.04.122|2050", "dot2026.json|0.0.0.0.0.9999.0.0|9999"])));
        let t = linhas(&p.join("2024-12_MS-SICOM-DETALHE-EMPENHO.txt"));
        assert_eq!((t[1][1].as_str(), t[2][1].as_str(), t[0].len()), ("0.0.13.392.0.2050.13.392", cam, 8));
        assert_eq!(linhas(&p.join("2024-12_MS-SICOM-FUNCOES.txt"))[1..], [txts(&["funcoes.json", "f", "Legislativa", "01", "1"]), txts(&["funcoes.json", "f", "Cultura", "99", "1"]), txts(&["funcoes.json", "s", "Ação Legislativa", "031", "1"])]);
        assert_eq!((linhas(&p.join("2025-12_MS-ESFINGE-PPA-2022.txt")).len(), linhas(&p.join("2026-08_MS-ESFINGE-PPA-2026.txt")).len(), linhas(&p.join("2025-12_MS-ESFINGE-RECEITA.txt")).len()), (3, 2, 2));
        // o codigo com .CAM e achado pelo cod_texto no quadro principal e no setor
        let md = std::fs::read_to_string(&fontes).unwrap();
        assert!(md.contains("2025-12_MS-ESFINGE-EXECUCAO.txt linhas 4 |") && md.contains(&format!("({cam}, 2025-12_MS-ESFINGE-EXECUCAO.txt linhas 4)")), "{md}");
        assert_eq!((&rel["codigos_nao_achados"], &rel["setor_sem_linha"], rel["arquivos"].as_array().unwrap().len()), (&json!(0), &json!(0), 12));
        g("des2024", json!([["PREFEITURA", "Lazer Novo", "Difusão Cultural", "2050", 5.0, 6.0]]));
        assert!(ms::montar(&o, &f).unwrap_err().contains("função sem código no tribunal: Lazer Novo"));
        g("des2024", json!([["PREFEITURA", "Cultura"]]));
        assert!(ms::montar(&o, &f).unwrap_err().contains("des2024: linha fora do formato"));
        let _ = std::fs::remove_dir_all(site.parent().unwrap());
    }

    fn zipa(arquivos: &[(&str, &str)]) -> Vec<u8> {
        let mut z = zip::ZipWriter::new(std::io::Cursor::new(vec![]));
        for (nome, t) in arquivos {
            z.start_file(*nome, zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut z, t.as_bytes()).unwrap();
        }
        z.finish().unwrap().into_inner()
    }

    #[test]
    fn joao_pessoa_csv_do_zip_com_o_codigo_so_ate_o_corte() {
        let cod = "10.13.392.5001.0829";
        let (site, pacote, fontes) = cenario("pb2", "985", "RECEITAS-DESPESAS-JOAO-PESSOA-PB", cod, cod);
        let src = site.parent().unwrap().join("src");
        let csv = |a: &str| format!("\u{feff}mes;codigo_unidade_gestora;codigo_unidade_orcamentaria;numero_empenho;codigo_funcao;codigo_subfuncao;codigo_programa;codigo_acao;historico;valor_liquidado\n03/{a};201;10 ;0829;13;392;5001;0829;\"diz \"\"oi\"\"; fim\";5.0\n10/{a};201;10;900;13;392;5001;0829;x;7.0\n");
        grava(&src.join("mes26.txt"), "09\n");
        for a in ANOS {
            std::fs::write(src.join(format!("despesas-{a}.zip")), zipa(&[("despesas.csv", &csv(a))])).unwrap();
            std::fs::write(src.join(format!("receitas-{a}.zip")), zipa(&[("receitas.csv", "mes_ano;valor\r\n01;10\r\n")])).unwrap();
            grava(&src.join(format!("dotacao{a}.json")), &if a == "2026" { json!([]) } else { json!([{ "dotacao": "10.13.392.5001.0829.3390", "valorOrcado": 9.0 }]) }.to_string());
        }
        grava(&src.join("liq_depois.json"), &json!({ "201|10|829": 2.0, "201|10|77": 1.0 }).to_string());
        let (le, nomes) = da_pasta(&src);
        let (o, f) = (Onde { site: &site, pacote: &pacote, desenha: &nada }, Fonte { le: &le, nomes: &nomes });
        let (p, rel) = pb::montar(&o, &f, "15/11/2026").unwrap();
        // 2026: outubro fica sem codigo; o campo com ; e aspas continua um campo so
        let t = std::fs::read_to_string(p.join("2026-09_PB-SAGRES-DESPESAS.txt")).unwrap();
        assert_eq!(t.lines().skip(1).collect::<Vec<_>>(), [format!("{cod};03/2026;201;10 ;0829;13;392;5001;0829;\"diz \"\"oi\"\"; fim\";5.0"), ";10/2026;201;10;900;13;392;5001;0829;x;7.0".to_string()]);
        assert!(p.join("_grandes/2025-12_PB-SAGRES-DESPESAS.txt").exists() && !p.join("2026-12_PB-SAGRES-DESPESAS-AUTORIZADAS.txt").exists());
        assert_eq!((linhas(&p.join("2025-12_PB-SAGRES-DESPESAS-AUTORIZADAS.txt"))[1][0].as_str(), std::fs::read_to_string(p.join("2026-09_PB-SAGRES-RECEITAS.txt")).unwrap().as_str()), (cod, "mes_ano;valor\r\n01;10\r\n"));
        // liquidado depois do corte: o codigo vem do empenho do CSV de 2026 (0829 = 829); empenho que nao esta la fica sem codigo
        assert_eq!(linhas(&p.join("2026-09_PB-SAGRES-LIQUIDACOES-DEPOIS-DO-CORTE.txt")), [txts(&["codigo_acao_video (do empenho no CSV de 2026, mês 01 a 09)", "codigo_unidade_gestora", "codigo_unidade_orcamentaria", "numero_empenho", "valorLiquidado depois de 30/09/2026 (somado por empenho)"]), txts(&["", "201", "10", "77", "1.0"]), txts(&[cod, "201", "10", "829", "2.0"])]);
        let md = std::fs::read_to_string(&fontes).unwrap();
        assert!(md.contains("2025-12_PB-SAGRES-DESPESAS.txt linhas 2-3; 2025-12_PB-SAGRES-DESPESAS-AUTORIZADAS.txt linhas 2 |") && rel["arquivos"].as_array().unwrap().len() == 9, "{md}");
        std::fs::write(src.join("receitas-2024.zip"), zipa(&[("a.csv", "x"), ("b.csv", "y")])).unwrap();
        assert!(pb::montar(&o, &f, "15/11/2026").unwrap_err().contains("receitas-2024.zip: esperava 1 CSV"));
        assert!(pb::despesa(b"mes;x\n01;2\n".to_vec(), "d.zip", 12, None).unwrap_err().contains("d.zip: arquivo do tribunal sem a coluna codigo_unidade_gestora"));
        let _ = std::fs::remove_dir_all(site.parent().unwrap());
    }

    #[test]
    fn recife_transcreve_o_item_18_e_curitiba_junta_empenho_e_liquidacao() {
        use crate::link_tribunais_pe::tests::{linha, pagina};
        let cod = "2100.13.0.0";
        let (site, pacote, fontes) = cenario("pe2", "993", "RECEITAS-DESPESAS-RECIFE-PE", cod, cod);
        let (src, saida) = (site.parent().unwrap().join("src"), pacote.join("RECEITAS-DESPESAS-RECIFE-PE/saida"));
        std::fs::write(&fontes, std::fs::read_to_string(&fontes).unwrap() + "- Tome Conta Despesas 2025-12: setores\n").unwrap();
        for a in ["2024", "2025"] {
            grava(&saida.join(format!("acoes-{a}-12.json")), &json!([{ "cod": cod, "acao": "FESTA", "liquidado": 300000.0 }, { "cod": "2200.08.0.0", "acao": "SOCIAL", "liquidado": 400000.0 }, { "cod": "2200.10.0.0", "acao": "SOCIAL", "liquidado": 100000.0 }]).to_string());
            for item in ["item04", "item18", "item47"] {
                grava(&src.join(format!("etce{a}/{item}.pdf")), &format!("%PDF {item}"));
            }
        }
        grava(&saida.join("acoes-2026-08.json"), &json!([{ "cod": "5.13.0.0", "acao": "FESTA", "liquidado": 12.0 }, { "cod": "5.28.1.843", "acao": "DÍVIDA", "liquidado": 2.0 }]).to_string());
        let reg = |fu: &str, sub: &str, acao: &str, v: f64| json!({ "ID_UNIDADE_GESTORA": 5, "FUNCAO": fu, "SUBFUNCAO": sub, "PROGRAMA": "P", "ACAO": acao, "VALORLIQUIDADO": v, "VALOREMPENHADO": 2.0, "HISTORICO": "diz \"oi\"" });
        for m in 1..=8 {
            let mut l = vec![reg("Cultura", "Difusão Cultural", "Festa", 1.5)];
            l.extend((m == 1).then(|| reg("Encargos Especiais", "Serviço da Dívida Interna", "Dívida", 2.0)));
            grava(&src.join(format!("despesa2026-{m:02}.json")), &json!(l).to_string());
        }
        grava(&src.join("receita2026.json"), &json!([{ "MESREFERENCIA": "7", "VALORLANCAMENTO": 10.0 }]).to_string());
        grava(&src.join("ugs2026.json"), &json!([{ "ID_UNIDADEGESTORA": 5, "CODIGOMUNICIPIOUNIDADEGESTORA": "P120" }, { "ID_UNIDADEGESTORA": 6, "CODIGOMUNICIPIOUNIDADEGESTORA": "P001" }]).to_string());
        let paginas = |arq: &str| -> Result<Vec<crate::link_tribunais_pe::Pagina>, String> {
            Ok(if arq.ends_with("item18.pdf") {
                vec![pagina("", vec![linha(10.0, &[(40.0, "2100 - PROMOÇÃO DE EVENTOS"), (420.0, "300.000,00")]), linha(20.0, &[(60.0, "13 - Cultura"), (420.0, "250.000,00"), (520.0, "300.000,00")]), linha(40.0, &[(40.0, "2200 - PROTEÇÃO SOCIAL")]), linha(50.0, &[(60.0, "10 - Saúde"), (520.0, "100.000,00")])]), pagina("", vec![linha(5.0, &[(60.0, "08 - Assistência"), (520.0, "400.000,00")])])]
            } else {
                vec![pagina("capa", vec![]), pagina("DEMONSTRATIVO DA DESPESA POR PROJETOS, ATIVIDADES", vec![linha(5.0, &[(90.0, "ESPECIFICAÇÃO"), (520.0, "TOTAL")]), linha(10.0, &[(50.0, "2.100"), (90.0, "PROMOÇÃO DE EVENTOS"), (530.0, "100.000,00")]), linha(30.0, &[(50.0, "2.200"), (90.0, "PROTEÇÃO SOCIAL"), (530.0, "50.000")])])]
            })
        };
        let sobre = std::cell::RefCell::new(String::new());
        let desenha = |_: &Path, _: &str, secs: &[(String, Vec<String>)]| {
            *sobre.borrow_mut() = secs[0].1.join("\n");
            Ok(())
        };
        let (le, nomes) = da_pasta(&src);
        let (o, f) = (Onde { site: &site, pacote: &pacote, desenha: &desenha }, Fonte { le: &le, nomes: &nomes });
        let (p, rel) = pe::montar(&o, &f, &paginas, &[("2025", "123-4")]).unwrap();
        // a dotacao da LOA vai so na 1a funcao (08) da acao, mesmo vindo depois no PDF
        assert_eq!(linhas(&p.join("2025-12_PE-ETCEPE-ITEM18-TRANSCRICAO.txt"))[1..], [txts(&[cod, "p. 1", "2100", "PROMOÇÃO DE EVENTOS", "13", "300.000,00", "100.000,00", "p. 2"]), txts(&["2200.10.0.0", "p. 1", "2200", "PROTEÇÃO SOCIAL", "10", "100.000,00", "", ""]), txts(&["2200.08.0.0", "p. 2", "2200", "PROTEÇÃO SOCIAL", "08", "400.000,00", "50.000", "p. 2"])]);
        assert_eq!(std::fs::read_to_string(p.join("2024-12_PE-ETCEPE-ITEM47-LOA.pdf")).unwrap(), "%PDF item47");
        // 2026: o numero da acao e a posicao na ordem das chaves, como no leitor; o historico com aspas continua um campo
        let t = linhas(&p.join("2026-08_PE-TOMECONTA-DESPESAS-MUNICIPAIS.txt"));
        assert_eq!((t.len(), t[0][..3].to_vec(), t[1][0].as_str(), t[2][0].as_str(), t[1][3].as_str()), (10, txts(&["codigo_acao_video", "ACAO", "FUNCAO"]), "5.13.0.0", "5.28.1.843", "diz \"oi\""));
        assert!(p.join("2026-07_PE-TOMECONTA-RECEITAS-MUNICIPAIS.txt").exists() && rel["arquivos"].as_array().unwrap().len() == 10);
        let (md, s0) = (std::fs::read_to_string(&fontes).unwrap(), sobre.borrow().clone());
        assert!(md.contains("2025-12_PE-ETCEPE-ITEM18-TRANSCRICAO.txt linhas 2 |") && md.contains("- e-TCEPE Item 18 2025: setores") && md == std::fs::read_to_string(p.join("993. orçamento - fontes.md")).unwrap(), "{md}");
        assert!(s0.contains("Prefeitura do Recife, processo 123-4 (") && s0.contains("Governo 2024 da Prefeitura do Recife (") && s0.contains("(1 órgãos do Recife)"), "{s0}");
        let c = pe::conferir(&pacote, &p).unwrap();
        assert_eq!((&c["2025"]["total_arquivo"], &c["2026"]["total_arquivo"], &c["2026"]["n_ruins"]), (&json!(800000.0), &json!(14.0), &json!(0)));
        grava(&saida.join("acoes-2026-08.json"), &json!([{ "cod": "5.13.0.0", "liquidado": 99.0 }]).to_string());
        assert!(pe::conferir(&pacote, &p).unwrap_err().starts_with("TCE-PE: 1 ação(ões) não bateram"));
        // mes sem registro (cabecalho diferente) e funcao que a Portaria 42 nao tem: para
        grava(&src.join("despesa2026-03.json"), "[]");
        assert!(pe::montar(&o, &f, &paginas, &[]).unwrap_err().contains("despesa2026-03.json: o cabeçalho do tribunal mudou"));
        grava(&src.join("despesa2026-03.json"), &json!([reg("Lazer Novo", "x", "y", 1.0)]).to_string());
        assert!(pe::montar(&o, &f, &paginas, &[]).unwrap_err().contains("função sem código na Portaria 42"));
        let _ = std::fs::remove_dir_all(site.parent().unwrap());

        let cod = "9.1.2.13.392.20.2050";
        let (site, pacote, fontes) = cenario("pr2", "994", "RECEITAS-DESPESAS-CURITIBA-PR", cod, cod);
        let (src, saida) = (site.parent().unwrap().join("src"), pacote.join("RECEITAS-DESPESAS-CURITIBA-PR/saida"));
        let mut totais = vec![json!({ "dito": "Já empenhou (2026 até 08)", "valor": "R$ 8" })];
        for (a, m) in [("2024", "12"), ("2025", "12"), ("2026", "08")] {
            let seguinte = a.parse::<i64>().unwrap() + 1;
            grava(&saida.join(format!("acoes-{a}-{m}.json")), &json!([{ "cod": cod, "acao": "FESTA", "liquidado": 5.0 }, { "cod": "9.1.2.13.392.20.2051", "acao": "OUTRA", "liquidado": 0.0 }]).to_string());
            totais.extend([json!({ "dito": format!("Gastou ({a})"), "valor": "R$ 5" }), json!({ "dito": format!("Arrecadou ({a})"), "valor": "R$ 10" })]);
            let emp = |id: u8, mes: &str, ent: &str, acao: &str| format!("<Empenho idEmpenho=\"{id}\" nrEmpenho=\"1{id}\" dtEmpenho=\"{a}-{mes}-01\" vlEmpenho=\"8.0\" nmEntidade=\"{ent}\" idPessoa=\"9\" cdOrgao=\"1\" cdUnidade=\"2\" cdFuncao=\"13\" cdSubFuncao=\"392\" cdPrograma=\"20\" cdProjetoAtividade=\"{acao}\" dsProjetoAtividade=\" Festa &quot;x&quot; \"/>\n");
            grava(&src.join(format!("{a}_Despesa/{a}_410690_Empenho.xml")), &format!("<?xml version=\"1.0\"?><Empenhos>\n{}{}{}</Empenhos>", emp(1, "03", "MUNICIPIO", "2050"), emp(2, "03", "Consórcio Intermunicipal", "2050"), emp(3, "11", "MUNICIPIO", "2051")));
            let liq = |id: u8, e: u8, data: String| format!("<EmpenhoLiquidacao idLiquidacao=\"{id}\" idEmpenho=\"{e}\" dtLiquidacao=\"{data}\" vlLiquidacaoLiquido=\"5.0\"/>");
            grava(&src.join(format!("{a}_Despesa/{a}_410690_EmpenhoLiquidacao_1.xml")), &format!("<L>{}{}{}</L>", liq(7, 1, format!("{a}-04-01")), liq(8, 2, format!("{a}-04-01")), liq(9, 1, format!("{seguinte}-01-05"))));
            grava(&src.join(format!("{a}_Despesa/{a}_410690_EmpenhoLiquidacaoItem.xml")), "outro arquivo, nao e lido");
            grava(&src.join(format!("{a}_Receita/{a}_410690_ReceitasConsolidado.xml")), &format!("<R><ReceitasConsolidado idpessoa=\"9\" nrAno=\"{a}\" nrMes=\"{}\" idSumarioItem=\"5010\" dsItem=\"RECEITAS\" vlPrevisaoInicialDoExercicio=\"1\" vlPrevisaoAtualizadaDoExercicio=\"1\" vlRealizadoDoMes=\"1\" vlRealizadoAteOMes=\"10.0\"/></R>", m.trim_start_matches('0')));
        }
        grava(&saida.join("totais.json"), &json!(totais).to_string());
        let (le, nomes) = da_pasta(&src);
        let (o, f) = (Onde { site: &site, pacote: &pacote, desenha: &nada }, Fonte { le: &le, nomes: &nomes });
        let (p, rel) = pr::montar(&o, &f).unwrap();
        // so a liquidacao do ano de empenho do ano que nao e de consorcio; a classificacao vem do empenho
        assert_eq!(linhas(&p.join("2025-12_PR-PIT-LIQUIDACOES.txt"))[1..], [txts(&["2025_410690_EmpenhoLiquidacao_1.xml", "7", "1", "2025-04-01", "5.0", cod, "9", "1", "2", "13", "392", "20", "2050", "Festa \"x\""])]);
        assert_eq!((linhas(&p.join("2025-12_PR-PIT-EMPENHOS.txt")).len(), linhas(&p.join("2026-08_PR-PIT-EMPENHOS.txt")).len(), linhas(&p.join("2026-08_PR-PIT-RECEITAS-CONSOLIDADO.txt"))[1][2].as_str()), (3, 2, "8"));
        assert!(rel["arquivos"].as_array().unwrap().len() == 9 && std::fs::read_to_string(&fontes).unwrap().contains("2025-12_PR-PIT-LIQUIDACOES.txt linhas 2; 2025-12_PR-PIT-EMPENHOS.txt linhas 2 |"));
        let c = pr::conferir(&pacote, &p).unwrap();
        assert_eq!((&c["2025"]["arrecadado_arquivo"], &c["2025"]["empenhado_arquivo"], &c["2025"]["gastou_video"], &c["2025"]["empenhou_video"], &c["2026"]["empenhou_video"], &c["2026"]["nao_bateram"]), (&json!([10.0]), &json!(16.0), &json!("R$ 5"), &Value::Null, &json!("R$ 8"), &json!(0)));
        grava(&src.join("2024_Despesa/2024_410690_Empenho.xml"), "<Empenhos><Empenho idEmpenho=\"1\"/>");
        assert!(pr::montar(&o, &f).unwrap_err().contains("cortado"));
        let _ = std::fs::remove_dir_all(site.parent().unwrap());
    }

    #[test]
    fn teresina_pdf_com_pagina_e_empenhos_ate_o_ultimo_mes_seguido() {
        let cod = "13.0.13.392.0001.2100";
        let (site, pacote, fontes) = cenario("pi2", "986", "RECEITAS-DESPESAS-TERESINA-PI", cod, cod);
        let (src, saida) = (site.parent().unwrap().join("src"), pacote.join("RECEITAS-DESPESAS-TERESINA-PI/saida"));
        std::fs::remove_file(saida.join("acoes-2026-08.json")).unwrap();
        let mut docs = vec![];
        for (i, (a, m)) in [("2024", "12"), ("2025", "12"), ("2026", "02")].into_iter().enumerate() {
            let n: i64 = a.parse().unwrap();
            grava(&saida.join(format!("acoes-{a}-{m}.json")), &json!([{ "cod": cod, "acao": "FESTA", "liquidado": 5.0 }, { "cod": "0.0.13.000.0000.0000", "acao": "CULTURA (liquidado da função)", "liquidado": 3.0 }]).to_string());
            docs.push(json!({ "exercicio": n, "documento": "Lei Orçamentária Anual - LOA", "idDocWebDownload": 100 + i, "dataEnvio": format!("{a}-01-10T08:00:00") }));
            docs.push(json!({ "exercicio": n, "documento": "Programa de trabalho (Anexo 6) do Balanço", "idDocWebDownload": 200 + i, "dataEnvio": format!("{a}-12-10T08:00:00") }));
            grava(&src.join(format!("despesas_133_{a}_lista_porFuncao.json")), &json!([{ "funcao": "Cultura", "empenhada": 9.0, "liquidada": 8.0, "paga": 7.0 }]).to_string());
            grava(&src.join(format!("receita_painel_{a}.json")), &json!([{ "Unidade Gestora": "P. M. DE TERESINA", "Origem": "Impostos", "Receita Bruta": 11.0, "Deduções": 1.0, "Receita Líquida": 10.0 }]).to_string());
        }
        // fevereiro foi reenviado (vale o ultimo envio); marco nao veio: o corte e fevereiro
        for (id, mes, envio, v) in [(301, "01", "2026-02-10", 500.0), (302, "02", "2026-03-10", 1.0), (303, "02", "2026-03-20", 7.0)] {
            docs.push(json!({ "exercicio": 2026, "documento": "Relação das notas de empenhos emitidas", "idDocWebDownload": id, "dataEnvio": format!("{envio}T08:00:00") }));
            grava(&src.join(format!("m22_{id}.json")), &json!([mes, [[13, 13, "2100", v, 0.0], [8, 8, "2200", 10.0, 1.0]]]).to_string());
        }
        for d in &docs {
            grava(&src.join(format!("{}.pdf", d["idDocWebDownload"])), &format!("%PDF {}", d["idDocWebDownload"]));
        }
        grava(&src.join("documentos_133_pagina_1_qtdePorPagina_5000.json"), &json!(docs).to_string());
        grava(&src.join("receitas_133.json"), &json!([{ "exercicio": 2025, "arrecadada": 10.0, "prevista": 12.0 }]).to_string());
        let texto = |arq: &str| -> Result<String, String> {
            if arq.starts_with('2') {
                return Ok("Anexo 6\nÓrgão: 13 Fundação Cultural\nRealização das Festividades\nJuninas\n133920001.2100 1.000,00 2.000.000,00\nTotais 5,00\n\u{c}Órgão: 08 Assistência\nConcessão de Benefícios Eventuais\n082440005.2200 300.000,00\n".into());
            }
            let mut t = String::from("Anexo VI\u{c}Órgão: 13 Fundação Cultural\n1339200012100 Realização das Festividades Juninas 1.500.000 - 1.500.000\n0824400052200 Concessão de Benefícios Eventuais\n800.000 - 800.000\nTotal 9.999.999\n");
            (0..100).for_each(|i| t += &format!("04{:07}{:04} Manutenção da unidade {i} 1.000 1.000\n", 1220000 + i, 3000 + i));
            Ok(t)
        };
        let (le, nomes) = da_pasta(&src);
        let (o, f) = (Onde { site: &site, pacote: &pacote, desenha: &nada }, Fonte { le: &le, nomes: &nomes });
        let (p, rel) = pi::montar(&o, &f, &texto).unwrap();
        assert_eq!(linhas(&p.join("2025-12_PI-API-DESPESA-POR-FUNCAO.txt"))[1], ["despesas/133/2025/lista/porFuncao", "0.0.13.000.0000.0000", "Cultura", "13", "9.0", "8.0", "7.0"]);
        // LOA e Anexo 6 em tabela, em ordem de codigo, com o PDF e a pagina; a dotacao como numero inteiro
        let t = linhas(&p.join("2025-12_PI-LOA-ANEXO-VI.txt"));
        assert_eq!((t.len(), t.last().unwrap().clone(), t[1][1].as_str()), (103, txts(&["2025-12_PI-LOA.pdf p. 2", cod, "1339200012100", "13", "2100", "Realização das Festividades Juninas", "1500000"]), ""));
        assert_eq!(linhas(&p.join("2024-12_PI-BALANCO-ANEXO-6.txt"))[1..], [txts(&["2024-12_PI-BALANCO-ANEXO-6.pdf p. 2", "", "08", "082440005.2200", "8", "2200", "Concessão de Benefícios Eventuais", "300000.0"]), txts(&["2024-12_PI-BALANCO-ANEXO-6.pdf p. 1", cod, "13", "133920001.2100", "13", "2100", "Realização das Festividades Juninas", "2000000.0"])]);
        let t = linhas(&p.join("2026-02_PI-EMPENHOS-MODELO-XXII.txt"));
        assert_eq!((t.len(), t[1].clone(), t[3][..3].to_vec(), t[3][6].as_str()), (5, txts(&["2026-01_PI-EMPENHOS-MODELO-XXII.pdf", "1", cod, "13", "13", "2100", "500.0", "0.0"]), txts(&["2026-02_PI-EMPENHOS-MODELO-XXII.pdf", "1", cod]), "7.0"));
        assert_eq!((std::fs::read_to_string(p.join("2026-02_PI-EMPENHOS-MODELO-XXII.pdf")).unwrap().as_str(), std::fs::read_to_string(p.join("2026-12_PI-LOA.pdf")).unwrap().as_str()), ("%PDF 303", "%PDF 102"));
        assert!(!p.join("2026-12_PI-BALANCO-ANEXO-6.pdf").exists() && linhas(&p.join("2026-02_PI-API-RECEITAS-POR-ANO.txt"))[1] == ["2025", "10.0", "12.0"] && rel["codigos_nao_achados"] == 0);
        let md = std::fs::read_to_string(&fontes).unwrap();
        assert!(md.contains("2025-12_PI-LOA-ANEXO-VI.txt linhas 103; 2025-12_PI-BALANCO-ANEXO-6.txt linhas 3 |"), "{md}");
        // funcao sem codigo e arquivo de trabalho que falta: para
        grava(&src.join("despesas_133_2024_lista_porFuncao.json"), &json!([{ "funcao": "Lazer Novo", "empenhada": 1.0, "liquidada": 1.0, "paga": 1.0 }]).to_string());
        assert!(pi::montar(&o, &f, &texto).unwrap_err().contains("função sem código na Portaria 42"));
        std::fs::remove_file(src.join("m22_301.json")).unwrap();
        assert!(pi::montar(&o, &f, &texto).unwrap_err().contains("m22_301.json: o leitor não guardou"));
        let _ = std::fs::remove_dir_all(site.parent().unwrap());
    }
}
