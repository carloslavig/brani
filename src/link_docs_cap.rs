//! Adaptadores por capital dos documentos do tribunal (pasta link/docs_cap/ do Python), num arquivo so, um modulo por estado:
//! cada um transforma o que o leitor do estado guardou do TRIBUNAL (JSON das consultas, CSV dos dados abertos) em tabela de
//! texto, poe na 1a coluna o codigo da acao do video montado com as colunas da PROPRIA linha (do mesmo jeito que o leitor monta)
//! e chama o motor generico (`link_docs_capitais::montar`). Coluna que falta, registro quebrado em duas linhas ou tabela que nao
//! acompanha o JSON = Err; nada e completado.
//! Portados: `cod_texto` (codigo de acao que nao e so numero com ponto), `es` (com `partes_festas` e `setores`, usados por
//! outros estados), `ce`, `mg`, `rj`, `mt` e `go`.
//! Os outros dez (`to`, `rs`, `sc`, `ac`, `am`, `ms`, `pb`, `pe`, `pi`, `pr`) estao em `link_docs_cap2`, que usa os auxiliares daqui.
//! Entra por parametro (`Onde`): a pasta do site local, a pasta do pacote de producao e quem desenha o PDF; em MG, quem abre o zip.
#![allow(dead_code)]
use crate::link_docs_capitais::{self as d, Adaptador, Arq};
use crate::link_tribunais_util::pystr;
use regex::Regex;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub struct Onde<'a> {
    pub site: &'a Path,
    pub pacote: &'a Path,
    pub desenha: &'a dyn Fn(&Path, &str, &[(String, Vec<String>)]) -> Result<(), String>,
}

pub(crate) const ANOS: [&str; 3] = ["2024", "2025", "2026"];

pub(crate) fn js(arq: &Path) -> Result<Value, String> {
    let t = std::fs::read_to_string(arq).map_err(|e| format!("{}: {e}", arq.display()))?;
    serde_json::from_str(t.trim_start_matches('\u{feff}')).map_err(|e| format!("{}: {e}", arq.display()))
}

pub(crate) fn lista<'a>(v: &'a Value, o_que: &str) -> Result<&'a Vec<Value>, String> {
    v.as_array().ok_or_else(|| format!("{o_que}: não é uma lista de registros"))
}

/// O mes de corte de 2026 que o leitor guardou (mes26.txt), sempre "MM".
fn mes26_txt(src: &Path) -> Result<String, String> {
    let arq = src.join("mes26.txt");
    let m = std::fs::read_to_string(&arq).map_err(|e| format!("{}: {e}", arq.display()))?.trim().to_string();
    if m.len() == 2 && m.parse::<u8>().is_ok_and(|n| (1..=12).contains(&n)) { Ok(m) } else { Err(format!("{}: mês de corte inválido {m:?}", arq.display())) }
}

pub(crate) fn mes_de<'a>(ano: &str, mes26: &'a str) -> &'a str {
    if ano == "2026" { mes26 } else { "12" }
}

pub(crate) fn sem_zero(mes: &str) -> String {
    mes.trim_start_matches('0').to_string()
}

pub(crate) fn colunas(liquidado: &str, previa: &str, empenhado: &str, arrecadado: &str) -> BTreeMap<&'static str, String> {
    [("liquidado", liquidado), ("previa", previa), ("empenhado", empenhado), ("arrecadado", arrecadado)].into_iter().map(|(k, v)| (k, v.to_string())).collect()
}

pub(crate) fn filtros(f: &dyn Fn(&str, &str) -> String) -> BTreeMap<(String, String), String> {
    ANOS.iter().flat_map(|a| ["despesa", "receita"].map(|t| ((a.to_string(), t.to_string()), f(a, t)))).collect()
}

pub(crate) fn inteiro(c: &str) -> Vec<String> {
    vec![c.to_string()]
}

/// A posicao de cada coluna pedida no cabecalho; coluna que falta e erro.
pub(crate) fn posicoes(cab: &[String], campos: &[&str], o_que: &str) -> Result<Vec<usize>, String> {
    campos.iter().map(|c| cab.iter().position(|x| x == c).ok_or_else(|| format!("{o_que}: arquivo do tribunal sem a coluna {c}"))).collect()
}

/// A tabela de uma lista de registros, com a garantia de uma linha por registro (na mesma ordem).
pub(crate) fn tabela(l: &[Value], o_que: &str) -> Result<Vec<Vec<String>>, String> {
    let t = d::tabela_json(&Value::Array(l.to_vec()));
    if t.len() != l.len() + 1 { Err(format!("{o_que}: a tabela não acompanha os registros ({} linhas para {})", t.len() - 1, l.len())) } else { Ok(t) }
}

/// Poe uma 1a coluna (titulo + um valor por linha) na tabela.
pub(crate) fn com_coluna(titulo: &str, valores: Vec<String>, t: Vec<Vec<String>>) -> Vec<Vec<String>> {
    std::iter::once(titulo.to_string()).chain(valores).zip(t).map(|(c, r)| std::iter::once(c).chain(r).collect()).collect()
}

/// Codigo de acao que nao e so numeros com ponto (GO: orgao.unidade.funcao.subfuncao em texto; MS: ...".CAM").
pub mod cod_texto {
    use super::*;
    use std::cell::RefCell;

    /// Os codigos conhecidos (acoes-*.json do levantamento), do mais comprido para o mais curto.
    pub fn cods(dados: &Path) -> Result<Vec<String>, String> {
        let saida = dados.join("saida");
        let mut cs = std::collections::BTreeSet::new();
        for e in std::fs::read_dir(&saida).map_err(|e| format!("{}: {e}", saida.display()))?.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.starts_with("acoes-") && n.ends_with(".json") {
                for x in lista(&js(&e.path())?, &n)? {
                    cs.insert(x["cod"].as_str().ok_or_else(|| format!("{n}: ação sem código"))?.to_string());
                }
            }
        }
        let mut cs: Vec<String> = cs.into_iter().collect();
        cs.sort_by(|a, b| b.chars().count().cmp(&a.chars().count()));
        Ok(cs)
    }

    /// O codigo esta no texto como um todo (nao colado em letra, numero ou ponto)?
    pub fn acha(c: &str, txt: &str) -> bool {
        let borda = |x: Option<char>| !x.is_some_and(|x| x.is_alphanumeric() || x == '_' || x == '.');
        !c.is_empty() && txt.match_indices(c).any(|(i, _)| borda(txt[..i].chars().next_back()) && borda(txt[i + c.len()..].chars().next()))
    }

    /// Como o motor, mas reconhecendo os codigos conhecidos e "mesma linha"; depois os setores com codigo fora do padrao do
    /// motor ganham as linhas do arquivo do tribunal (nas duas copias do fontes.md).
    pub fn montar(o: &Onde, numero: &str, cidade: &str, uf: &str, ad: &mut Adaptador, dados: &Path) -> Result<(PathBuf, Value), String> {
        let cs = cods(dados)?;
        let ult: RefCell<Option<String>> = RefCell::new(None);
        let codigos = |txt: &str| -> Vec<String> {
            if let Some(c) = cs.iter().find(|c| acha(c, txt)) {
                *ult.borrow_mut() = Some(c.clone());
                return vec![c.clone()];
            }
            let r = d::codigos(txt);
            if r.is_empty() && txt.contains("mesma linha") { ult.borrow().iter().cloned().collect() } else { r }
        };
        let (pasta, mut rel) = {
            let mut com = Adaptador { pasta: ad.pasta.clone(), tribunal: ad.tribunal.clone(), arquivos: std::mem::take(&mut ad.arquivos), col: ad.col.clone(), filtro: ad.filtro.clone(), partes: ad.partes, codigos: Some(&codigos), faixa_mx: ad.faixa_mx };
            let r = d::montar(o.site, numero, cidade, uf, &mut com, dados, o.desenha);
            ad.arquivos = com.arquivos;
            r?
        };
        let refs = d::Referencias::novo(&ad.arquivos, ad.partes, ad.faixa_mx);
        let fmd = d::ler_fontes(o.site, numero)?.0;
        let copia = pasta.join(fmd.file_name().ok_or("fontes.md sem nome")?);
        let md = std::fs::read_to_string(&copia).map_err(|e| format!("{}: {e}", copia.display()))?;
        let setor = Regex::new(r"^## Setores (\d{4})").expect("regex fixa");
        let (mut out, mut ano): (Vec<String>, Option<String>) = (vec![], None);
        for l in md.split('\n') {
            if let Some(m) = setor.captures(l) {
                ano = Some(m[1].to_string());
            }
            let mut l = l.to_string();
            if let Some(a) = ano.as_ref().filter(|_| l.contains("p. 0)")) {
                for c in &cs {
                    let de = format!("({c}, p. 0)");
                    if l.contains(&de) {
                        l = l.replace(&de, &format!("({c}, {})", refs.ref_(a, c)?));
                    }
                }
            }
            out.push(l.replace("mesmas páginas do PCMGE065", &format!("mesmas linhas no arquivo do tribunal ({})", ad.tribunal)));
        }
        let md = out.join("\n");
        for p in [&copia, &fmd] {
            std::fs::write(p, &md).map_err(|e| format!("{}: {e}", p.display()))?;
        }
        rel["codigos_nao_achados"] = md.matches("não achado").count().into();
        rel["setor_sem_linha"] = md.matches("p. 0)").count().into();
        Ok((pasta, rel))
    }
}

/// 976 Vitoria (TCE-ES): dados abertos do CidadES (PCM) que o leitor guardou em JSON (so as linhas do municipio).
pub mod es {
    use super::*;

    const COD: [&str; 7] = ["CodigoUnidadeGestora", "CodigoOrgao", "CodigoUnidadeOrcamentaria", "CodigoFuncao", "CodigoSubFuncao", "CodigoPrograma", "CodigoAcao"];
    const CKAN: &str = "https://dados.es.gov.br/dataset (organização tcees)";

    /// O motor procura o codigo inteiro na coluna codigo_acao_video. No quadro principal o codigo perde o prefixo com letra:
    /// volta pela lista de festas do leitor (festas-cods.json).
    pub fn partes_festas(dados: &Path) -> Result<impl Fn(&str) -> Vec<String>, String> {
        let fc = js(&dados.join("saida").join("festas-cods.json"))?;
        let mut m = BTreeMap::new();
        for c in fc.as_object().ok_or("festas-cods.json fora do formato")?.values().flat_map(|cs| cs.as_array().into_iter().flatten()).filter_map(Value::as_str) {
            if let Some((_, resto)) = c.split_once('.') {
                m.insert(resto.to_string(), c.to_string());
            }
        }
        Ok(move |c: &str| vec![m.get(c).cloned().unwrap_or_else(|| c.to_string())])
    }

    /// O motor so troca "(cod, p. 0)" de codigo so com digitos; aqui o codigo comeca com letra (UG do CidadES, sigla do e-Sfinge).
    pub fn setores(site: &Path, pasta: &Path, numero: &str, ad: &Adaptador, mut rel: Value) -> Result<Value, String> {
        let refs = d::Referencias::novo(&ad.arquivos, ad.partes, ad.faixa_mx);
        let fmd = d::ler_fontes(site, numero)?.0;
        let copia = pasta.join(fmd.file_name().ok_or("fontes.md sem nome")?);
        let md = std::fs::read_to_string(&copia).map_err(|e| format!("{}: {e}", copia.display()))?;
        let (setor, sem_pagina) = (Regex::new(r"^## Setores (\d{4})").expect("regex fixa"), Regex::new(r"\(([^(),\s]+), p\. 0\)").expect("regex fixa"));
        let (mut out, mut ano): (Vec<String>, Option<String>) = (vec![], None);
        for l in md.split('\n') {
            if let Some(m) = setor.captures(l) {
                ano = Some(m[1].to_string());
            }
            let mut l = l.to_string();
            if let Some(a) = ano.as_ref().filter(|_| l.contains("p. 0)")) {
                for m in sem_pagina.captures_iter(&l.clone()) {
                    l = l.replace(&m[0], &format!("({}, {})", &m[1], refs.ref_(a, &m[1])?));
                }
            }
            out.push(l);
        }
        let md = out.join("\n");
        for p in [&copia, &fmd] {
            std::fs::write(p, &md).map_err(|e| format!("{}: {e}", p.display()))?;
        }
        rel["codigos_nao_achados"] = (md.matches("não achado").count() + md.matches("p. 0)").count()).into();
        Ok(rel)
    }

    pub fn montar(o: &Onde) -> Result<(PathBuf, Value), String> {
        let dados = o.pacote.join("RECEITAS-DESPESAS-VITORIA-ES");
        let (p, src) = (d::nova_pasta(o.site, "976", "Vitória")?, dados.join("tce-es"));
        let m26 = mes26_txt(&src)?;
        let mut arqs = vec![];
        for ano in ANOS {
            let mes = mes_de(ano, &m26);
            for (tipo, nome, rot) in [("despesa", "despesas", "CIDADES-PCM-DESPESAS"), ("receita", "receitas", "CIDADES-PCM-RECEITAS")] {
                let arq = format!("{nome}{ano}.json");
                let todos = js(&src.join(&arq))?;
                let mut l = vec![];
                for x in lista(&todos, &arq)? {
                    let m = pystr(&x["Mes"]).trim().parse::<u8>().map_err(|_| format!("{arq}: registro sem o mês"))?;
                    if m <= mes.parse::<u8>().unwrap_or(0) {
                        l.push(x.clone());
                    }
                }
                let mut t = tabela(&l, &arq)?;
                if tipo == "despesa" {
                    // 1a coluna = codigo da acao como o leitor monta: o motor procura so por ela
                    let cods = l.iter().map(|x| COD.iter().map(|k| x[*k].as_str().map(String::from).ok_or_else(|| format!("{arq}: registro sem {k}"))).collect::<Result<Vec<_>, _>>().map(|v| v.join("."))).collect::<Result<Vec<_>, String>>()?;
                    t = com_coluna("codigo_acao_video", cods, t);
                }
                let out = d::gravar(&p, &format!("{ano}-{mes}_ES-{rot}.txt"), &t)?;
                arqs.push(Arq::novo(ano, tipo, out, format!("TCE-ES CidadES (Prestação de Contas Mensal), dados abertos {nome}-municipios, arquivo municipios-{nome}-{ano}.zip ({CKAN}), só as linhas de EsferaAdministrativa = Vitória, meses 1 a {}", sem_zero(mes))));
            }
        }
        let partes = partes_festas(&dados)?;
        let mut ad = Adaptador { pasta: p, tribunal: "TCE-ES".into(), arquivos: arqs, partes: &partes, codigos: None, faixa_mx: 6,
            col: colunas("'Liquidada' (movimento do mês, somado nos meses 1..N)", "'PrevisaoInicial' (somada nas linhas da ação)", "'Empenhada' (somada nos meses 1..N)", "'Arrecadada' (já líquida do Fundeb, somada nos meses 1..N)"),
            filtro: filtros(&|a, _| format!("todas as linhas (todas as unidades gestoras de Vitória), meses 1 a {}", sem_zero(mes_de(a, &m26)))) };
        let (pasta, rel) = d::montar(o.site, "976", "Vitória", "ES", &mut ad, &dados, o.desenha)?;
        let rel = setores(o.site, &pasta, "976", &ad, rel)?;
        Ok((pasta, rel))
    }
}

/// 997 Fortaleza (TCE-CE): SIM pela API de Dados Abertos (JSON) -> balancete de despesa, de receita e orcamento de cada ano.
pub mod ce {
    use super::*;

    const API: &str = "https://api-dados-abertos.tce.ce.gov.br/sim/";
    const MUN: &str = "codigo_municipio=057"; // Fortaleza no SIM
    const CAMPOS: [&str; 8] = ["codigo_orgao", "codigo_unidade_orcamentaria", "codigo_funcao", "codigo_subfuncao", "codigo_programa", "codigo_projeto_atividade", "numero_projeto_atividade", "numero_subprojeto_atividade"];

    pub fn montar(o: &Onde) -> Result<(PathBuf, Value), String> {
        let dados = o.pacote.join("RECEITAS-DESPESAS-FORTALEZA-CE");
        let (p, src) = (d::nova_pasta(o.site, "997", "Fortaleza")?, dados.join("tce-ce"));
        let mes26 = mes26_txt(&src)?;
        let mut arqs = vec![];
        for ano in ANOS {
            let mes = mes_de(ano, &mes26);
            let rf = format!("{ano}{mes}");
            for (tipo, arq, met, rot) in [("despesa", format!("despesa{rf}"), "balancetes_despesas_orcamentarias", "SIM-BALANCETE-DESPESA"), ("receita", format!("receita{rf}"), "balancetes_receitas_orcamentarias", "SIM-BALANCETE-RECEITA"), ("outro", format!("orcamento{ano}"), "orcamento_despesas_projetos_atividades", "SIM-ORCAMENTO-NOMES-ACOES")] {
                let mut t = d::tabela_json(&js(&src.join(format!("{arq}.json")))?);
                if tipo == "despesa" {
                    // os pedacos soltos ("1", "2"...) casavam com outros campos e puxavam linha de acao vizinha: o codigo inteiro numa coluna
                    let i = posicoes(&t[0], &CAMPOS, &arq)?;
                    let cods = t[1..].iter().map(|r| i.iter().map(|k| r[*k].trim()).collect::<Vec<_>>().join(".")).collect();
                    t = com_coluna("codigo_acao_video", cods, t);
                }
                let f = d::gravar(&p, &format!("{ano}-{mes}_CE-{rot}.txt"), &t)?;
                let filtro = if tipo == "outro" { String::new() } else { format!("&data_referencia_doc={rf}") };
                arqs.push(Arq::novo(ano, tipo, f, format!("TCE-CE SIM, API de Dados Abertos: {API}{met}?{MUN}&exercicio_orcamento={ano}00{filtro} (todos os órgãos, um por consulta: a paginação da API não é estável){}{}",
                    if tipo == "outro" { " - só o nome de cada projeto/atividade" } else { "" }, if tipo == "despesa" { "; 1a coluna acrescentada = código da ação do vídeo, tirado das colunas da própria linha" } else { "" })));
            }
        }
        let mut ad = Adaptador { pasta: p, tribunal: "TCE-CE".into(), arquivos: arqs, partes: &inteiro, codigos: None, faixa_mx: 6,
            col: colunas("'valor_liquidado_ate_mes' menos 'valor_estornos_liquidacao_ate_mes'", "'valor_fixado_orcamento_bal_despesa' (dotação inicial)", "'valor_empenhado_ate_mes' menos 'valor_anulacoes_empenhos_ate_mes'", "'valor_arrecadacao_ate_mes' menos 'valor_anulacoes_ate_mes' (rubricas 95... = deduções do Fundeb, negativas)"),
            filtro: filtros(&|a, _| format!("todas as linhas (todos os órgãos), balancete acumulado até {}/{a}", mes_de(a, &mes26))) };
        d::montar(o.site, "997", "Fortaleza", "CE", &mut ad, &dados, o.desenha)
    }
}

/// 996 Belo Horizonte (TCE-MG): SICOM dos Dados Abertos (zip com CSV ";") -> despesa.csv e receita.csv de cada ano.
pub mod mg {
    use super::*;

    const IBGE: i64 = 3106200;
    const SITE: &str = "https://dadosabertos.tce.mg.gov.br/#/public/selecionar-item-assunto/jurisdicionado";

    fn cod(s: &str) -> &str {
        s.split(" - ").next().unwrap_or("").trim()
    }

    /// A mesma chave do leitor (subacao "-", "-9" ou vazia = sem subacao).
    pub fn chave(cab: &[String], r: &[String], o_que: &str) -> Result<String, String> {
        let v = |k: &str| cab.iter().position(|c| c == k).and_then(|i| r.get(i)).map(String::as_str).ok_or_else(|| format!("{o_que}: arquivo do tribunal sem a coluna {k}"));
        let sub = Some(cod(v("dsc_subacao")?)).filter(|s| !["", "-", "-9"].contains(s)).unwrap_or("");
        Ok([v("cod_orgao")?, v("cod_unidade")?, cod(v("dsc_funcao")?), cod(v("dsc_subfuncao")?), cod(v("dsc_programa")?), cod(v("dsc_acao")?), sub].join(".").trim_end_matches('.').to_string())
    }

    /// `zip(arquivo, membro)` devolve os bytes de um arquivo de dentro do zip do tribunal.
    pub fn montar(o: &Onde, zip: &dyn Fn(&Path, &str) -> Result<Vec<u8>, String>) -> Result<(PathBuf, Value), String> {
        let dados = o.pacote.join("RECEITAS-DESPESAS-BELO-HORIZONTE-MG");
        let p = d::nova_pasta(o.site, "996", "Belo Horizonte")?;
        let mes26 = d::acoes_do_ano(&dados.join("saida"), "2026").and_then(|a| a.file_stem().map(|s| s.to_string_lossy().into_owned())).and_then(|s| s.rsplit('-').next().map(String::from)).ok_or("BH: levantamento sem acoes-2026-MM.json")?;
        let le = |ano: &str, tipo: &str| -> Result<String, String> {
            let membro = format!("{ano}.{IBGE}.{tipo}.{tipo}.csv");
            String::from_utf8(zip(&dados.join("tce-mg").join(format!("{tipo}{ano}.zip")), &membro)?).map(|t| t.trim_start_matches('\u{feff}').to_string()).map_err(|_| format!("{membro}: não está em UTF-8"))
        };
        let grava = |nome: String, t: String| -> Result<PathBuf, String> {
            let f = p.join(nome);
            std::fs::write(&f, t).map_err(|e| format!("{}: {e}", f.display()))?;
            Ok(f)
        };
        let mut arqs = vec![];
        for ano in ANOS {
            let mes = mes_de(ano, &mes26);
            let t = le(ano, "despesa")?;
            let (l, regs): (Vec<&str>, Vec<Vec<String>>) = (t.lines().collect(), d::csv_py(&t, ';'));
            // sem campo quebrado em 2 linhas: linha N do .txt = registro N do CSV
            if l.len() != regs.len() || l.is_empty() {
                return Err(format!("despesa {ano}: {} linhas para {} registros (campo quebrado em duas linhas?)", l.len(), regs.len()));
            }
            let mut out = format!("codigo_acao_video;{}\n", l[0]);
            for (linha, r) in l.iter().zip(&regs).skip(1) {
                out += &format!("{};{linha}\n", chave(&regs[0], r, &format!("despesa {ano}"))?);
            }
            let f = grava(format!("{ano}-{mes}_MG-SICOM-DESPESA.txt"), out)?;
            arqs.push(Arq::novo(ano, "despesa", f, format!("TCE-MG SICOM, Dados Abertos > Jurisdicionado > Despesas, {ano}, Belo Horizonte: {ano}.{IBGE}.despesa.despesa.csv de despesa{ano}.zip ({SITE}); 1a coluna acrescentada = código da ação do vídeo, tirado das colunas da própria linha")));
            let f = grava(format!("{ano}-{mes}_MG-SICOM-RECEITA.txt"), le(ano, "receita")?)?;
            arqs.push(Arq::novo(ano, "receita", f, format!("TCE-MG SICOM, Dados Abertos > Jurisdicionado > Receitas, {ano}, Belo Horizonte: {ano}.{IBGE}.receita.receita.csv de receita{ano}.zip ({SITE}), como veio")));
        }
        let ate = |a: &str| format!("num_mesexercicio 1 a {}", sem_zero(mes_de(a, &mes26)));
        let sem_ponto = |c: &str| vec![c.trim_end_matches('.').to_string()];
        let mut ad = Adaptador { pasta: p.clone(), tribunal: "TCE-MG".into(), arquivos: arqs, partes: &sem_ponto, codigos: None, faixa_mx: 6,
            col: colunas("'vlr_liquidado' (valor do mês, somado nos meses 1..N)", "'vlr_previsto' (dotação inicial, vem no mês 1)", "'vlr_empenhado' (valor do mês, somado nos meses 1..N)", "'vlr_realizadoateperiodo' (valor do mês, somado nos meses 1..N; deduções do Fundeb vêm negativas, com dsc_tipodeducao)"),
            filtro: filtros(&|a, t| if t == "despesa" { format!("todas as linhas (todos os órgãos do município), {}", ate(a)) } else { format!("todas as linhas (todos os órgãos), {}, deduções incluídas (negativas)", ate(a)) }) };
        d::montar(o.site, "996", "Belo Horizonte", "MG", &mut ad, &dados, o.desenha)
    }
}

/// 998 Rio de Janeiro (TCM-RJ): Radar Carioca (painel publico do tribunal) -> Despesas e Receitas de cada ano.
pub mod rj {
    use super::*;

    const PAINEL: &str = "www.tcmrio.tc.br/t/painelpbi_radarcarioca";
    pub const PT: &str = "PT  (ESFERA /ÓRGÃO_UO /UNIDADE_ORÇ / FÇ /SUBFÇ/ PRG /ACAO) + Descricao ACAO";

    pub fn montar(o: &Onde) -> Result<(PathBuf, Value), String> {
        let dados = o.pacote.join("RECEITAS-DESPESAS-RIO-DE-JANEIRO-RJ");
        let (p, src) = (d::nova_pasta(o.site, "998", "Rio de Janeiro")?, dados.join("tcm-rj"));
        let mes26 = mes26_txt(&src)?;
        let meses = |a: &str| if a == "2026" { format!("meses 1 a {}", sem_zero(&mes26)) } else { "ano inteiro".to_string() };
        let mut arqs = vec![];
        for ano in ANOS {
            let mes = mes_de(ano, &mes26);
            let arq = format!("despesas-{ano}.json");
            let v = js(&src.join(&arq))?;
            let l = lista(&v, &arq)?;
            let chaves: Vec<&String> = l.first().and_then(Value::as_object).ok_or_else(|| format!("{arq}: sem registros"))?.keys().collect();
            if !chaves.iter().any(|k| *k == PT) {
                return Err(format!("{arq}: consulta do tribunal sem a coluna do PT"));
            }
            // o PT vem como "<24 digitos> - descricao": vai separado, para o codigo da acao do video achar a linha
            let mut linhas = vec![chaves.iter().flat_map(|k| if *k == PT { vec!["PT (24 dígitos)".to_string(), "Descrição ação".to_string()] } else { vec![k.to_string()] }).collect::<Vec<_>>()];
            for r in l {
                let mut lin = vec![];
                for k in &chaves {
                    let c = if r[k.as_str()].is_null() { return Err(format!("{arq}: registro sem a coluna {k}")) } else { pystr(&r[k.as_str()]) };
                    match (*k == PT, c.split_once(" - ")) {
                        (true, Some((pt, desc))) => lin.extend([pt.to_string(), desc.to_string()]),
                        (true, None) => lin.extend([c, String::new()]),
                        _ => lin.push(c),
                    }
                }
                linhas.push(lin);
            }
            let f = d::gravar(&p, &format!("{ano}-{mes}_RJ-RADAR-CARIOCA-DESPESAS.txt"), &linhas)?;
            arqs.push(Arq::novo(ano, "despesa", f, format!("TCM-RJ Radar Carioca ({PAINEL}), tabela Despesas_Mensal, Ano Referência {ano}, {}, por órgão, função, subfunção, PT e Tipo de Despesas (soma de Valor Despesas)", meses(ano))));
            let f = d::gravar(&p, &format!("{ano}-{mes}_RJ-RADAR-CARIOCA-RECEITAS.txt"), &d::tabela_json(&js(&src.join(format!("receitas-{ano}.json")))?))?;
            arqs.push(Arq::novo(ano, "receita", f, format!("TCM-RJ Radar Carioca ({PAINEL}), tabela Receitas, Ano Referência {ano}, {}, por ORIGEM (soma de Valor)", meses(ano))));
        }
        let org = js(&src.join("origens.json"))?;
        let mut linhas = vec![vec!["Cod".to_string(), "Descrição Origem".to_string()]];
        linhas.extend(org.as_object().ok_or("origens.json fora do formato")?.iter().map(|(k, v)| vec![k.clone(), pystr(v)]));
        let f = d::gravar(&p, &format!("2026-{mes26}_RJ-RADAR-CARIOCA-ORIGENS.txt"), &linhas)?;
        arqs.push(Arq::novo("2026", "outro", f, format!("TCM-RJ Radar Carioca ({PAINEL}), tabela Origem Receita (nome de cada ORIGEM)")));
        let sem_pontos = |c: &str| vec![c.replace('.', "")];
        let mut ad = Adaptador { pasta: p, tribunal: "TCM-RJ".into(), arquivos: arqs, partes: &sem_pontos, codigos: None, faixa_mx: 6,
            col: colunas("'Valor Despesas' nas linhas com Tipo de Despesas = 'Despesas Liquidadas'", "'Valor Despesas' nas linhas com Tipo de Despesas = 'Dotação Inicial'", "'Valor Despesas' nas linhas com Tipo de Despesas = 'Despesas Empenhadas'", "'Valor' (transferências já líquidas da retenção do Fundeb)"),
            filtro: filtros(&|a, t| if t == "despesa" { format!("todos os órgãos ({}), só as linhas do Tipo de Despesas dito na coluna", meses(a)) } else { "todas as linhas (todas as origens, intraorçamentárias 7x incluídas)".to_string() }) };
        d::montar(o.site, "998", "Rio de Janeiro", "RJ", &mut ad, &dados, o.desenha)
    }
}

/// 983 Cuiaba (TCE-MT): consultas ao Radar de Controle Publico que o leitor guardou em radar/*.json. O Radar nao tem arquivo
/// para baixar: o JSON e a resposta do proprio painel do tribunal. 2026 sem remessa da Prefeitura vai junto como prova.
pub mod mt {
    use super::*;

    fn inteiro_de(v: &Value, o_que: &str) -> Result<i64, String> {
        v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())).map(|f| f as i64).ok_or_else(|| format!("{o_que}: código que não é número: {v}"))
    }

    /// As linhas de uma consulta guardada, cada uma com pelo menos `n` colunas.
    fn consulta<'a>(j: &'a Value, nome: &str, n: usize) -> Result<Vec<&'a Vec<Value>>, String> {
        lista(&j[nome], nome)?.iter().map(|x| x.as_array().filter(|x| x.len() >= n).ok_or_else(|| format!("radar {nome}: linha com menos de {n} colunas"))).collect()
    }

    fn celulas(x: &[Value]) -> Vec<String> {
        x.iter().map(d::cel_json).collect()
    }

    pub fn montar(o: &Onde) -> Result<(PathBuf, Value), String> {
        let dados = o.pacote.join("RECEITAS-DESPESAS-CUIABA-MT");
        let p = d::nova_pasta(o.site, "983", "Cuiabá")?;
        let (dj, rj) = (js(&dados.join("radar").join("despesa.json"))?, js(&dados.join("radar").join("receita.json"))?);
        let m26 = dj["mes26"].as_str().ok_or("radar/despesa.json sem o mês de corte de 2026")?;
        let filtro = |a: &str| format!("Município = 'CUIABA', [Orçamento Fiscal e da Seguridade Social – OFSS] = 'Sim', Exercício = {a}, [Mês Referência] = 01 a {}", mes_de(a, m26));
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let vazios = |n: usize| vec![String::new(); n];
        let mut arqs = vec![];
        for a in ANOS {
            let m = mes_de(a, m26);
            let mut l = vec![s(&["codigo_acao_video", "consulta ao Radar", "Código Jurisdicionado / Município / Jurisdicionado", "Código Função", "Código Sub Função", "Número Projeto Atividade", "Projeto Atividade Descrição", "Valor Liquidado", "Valor Empenhado", "Valor Pago"])];
            for x in consulta(&dj, &format!("acoes-{a}"), 7)? {
                // o codigo da acao como o leitor monta (jurisdicionado.0.funcao.subfuncao.0.numero; os "0" sao enchimento)
                let cod = format!("{}.0.{}.{}.0.{}", inteiro_de(&x[0], "jurisdicionado")?, inteiro_de(&x[1], "função")?, inteiro_de(&x[2], "subfunção")?, pystr(&x[3]));
                l.push([vec![cod, "ações".into()], celulas(&x[..7]), vazios(1)].concat());
            }
            for x in consulta(&dj, &format!("total-{a}"), 4)? {
                l.push([vec![String::new(), "total do município".into()], celulas(&x[..1]), vazios(4), celulas(&x[1..4])].concat());
            }
            for x in consulta(&dj, &format!("orgaos-{a}"), 3)? {
                l.push([vec![String::new(), "por órgão (jurisdicionado)".into()], celulas(&x[..1]), vazios(4), celulas(&x[1..3]), vazios(1)].concat());
            }
            let out = d::gravar(&p, &format!("{a}-{m}_MT-RADAR-DESPESA.txt"), &l)?;
            arqs.push(Arq::novo(a, "despesa", out, format!("TCE-MT, Radar de Controle Público - Despesa (https://radardespesa.tce.mt.gov.br/), {} (consulta Qlik feita pelo leitor radar_mt)", filtro(a))));
            let mut l = vec![s(&["consulta ao Radar", "Grupo de Receita / Origem / Município", "Valor Receita Arrecadada"])];
            for (rotulo, nome) in [("origens", format!("origens-{a}")), ("grupos", format!("grupos-{a}")), ("total do município", format!("total-{a}"))] {
                l.extend(consulta(&rj, &nome, 1)?.into_iter().map(|x| [vec![rotulo.to_string()], celulas(x)].concat()));
            }
            let out = d::gravar(&p, &format!("{a}-{m}_MT-RADAR-RECEITA.txt"), &l)?;
            arqs.push(Arq::novo(a, "receita", out, format!("TCE-MT, Radar de Controle Público - Receita (https://radarreceita.tce.mt.gov.br/), {} (consulta Qlik feita pelo leitor radar_mt)", filtro(a))));
        }
        let mut l = vec![s(&["Exercício", "Jurisdicionado", "Mês Referência", "Valor Liquidado"])];
        l.extend(consulta(&dj, "meses_por_orgao", 1)?.into_iter().map(|x| celulas(x)));
        let out = d::gravar(&p, &format!("2026-{m26}_MT-RADAR-MESES-POR-ORGAO.txt"), &l)?;
        arqs.push(Arq::novo("2026", "outro", out, "TCE-MT, Radar Despesa: liquidado por exercício, órgão e mês de referência (2024-2026); mostra que em 2026 a Prefeitura não mandou nada (só outros órgãos aparecem)".into()));
        let mut ad = Adaptador { pasta: p, tribunal: "TCE-MT".into(), arquivos: arqs, partes: &inteiro, codigos: None, faixa_mx: 6,
            col: colunas("'Valor Liquidado' (linhas 'ações')", "- (o Radar não traz a dotação por ação)", "'Valor Empenhado' (linhas 'ações')", "'Valor Receita Arrecadada' (linhas 'origens')"),
            filtro: filtros(&|a, t| if t == "despesa" { format!("linhas da consulta 'ações' ({}); confere com a linha 'total do município'", filtro(a)) } else { "linhas da consulta 'origens'; confere com a linha 'total do município'".to_string() }) };
        d::montar(o.site, "983", "Cuiabá", "MT", &mut ad, &dados, o.desenha)
    }
}

/// 992 Goiania (TCM-GO): consultas do Portal do Cidadao gravadas em tcm-go/*.json pelo leitor. A "acao" do video em GO e
/// orgao.unidade.funcao.subfuncao (o tribunal nao publica execucao por projeto/atividade): codigo em texto, com coluna propria.
pub mod go {
    use super::*;
    use crate::link_tribunais_go::{funcao, subfuncao};

    const PORTAL: &str = "https://portalcidadao.tcmgo.tc.br/new-portal-cidadao/pages/visualizador.php";
    const CHAVE: &str = "código da ação (montado: orgao.unidade_orcamentaria.função.subfunção, códigos da Portaria 42/1999)";

    /// A mesma chave do leitor: orgao.unidade.funcao.subfuncao.
    pub fn chave(x: &Value) -> Result<String, String> {
        let limpa = |v: &Value| Regex::new(r"[.\s]+").expect("regex fixa").replace_all(v.as_str().unwrap_or(""), " ").trim().to_string();
        let (f, s) = (x["funcao"].as_str().unwrap_or(""), x["subfuncao"].as_str().unwrap_or(""));
        match (funcao(f), subfuncao(s)) {
            (Some(cf), Some(cs)) => Ok([&limpa(&x["orgao"]), &limpa(&x["unidade_orcamentaria"]), cf, cs].join(".")),
            _ => Err(format!("função/subfunção nova no TCM-GO, pôr o código no molde: {f} / {s}")),
        }
    }

    /// O motor compara sem maiusculas: a LOA as vezes grafa a unidade com outra caixa e as duas acoes viravam uma. Codigo com
    /// gemeo de outra caixa ganha " (grafia N)" na coluna-chave e nas partes procuradas.
    pub fn marca(cods: &[String]) -> impl Fn(&str) -> String {
        let mut g: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut ordenados = cods.to_vec();
        ordenados.sort();
        ordenados.into_iter().for_each(|c| g.entry(c.to_lowercase()).or_default().push(c));
        move |c: &str| match g.get(&c.to_lowercase()).filter(|v| v.len() >= 2).and_then(|v| v.iter().position(|x| x == c)) {
            Some(i) => format!("{c} (grafia {})", i + 1),
            None => c.to_string(),
        }
    }

    pub fn montar(o: &Onde) -> Result<(PathBuf, Value), String> {
        let dados = o.pacote.join("RECEITAS-DESPESAS-GOIANIA-GO");
        let (p, src) = (d::nova_pasta(o.site, "992", "Cidade")?, dados.join("tcm-go"));
        let mes26 = mes26_txt(&src)?;
        let marca = marca(&cod_texto::cods(&dados)?);
        let mut arqs = vec![];
        for ano in ANOS {
            let mes = mes_de(ano, &mes26);
            let meses = if ano == "2026" { format!("jan a {mes}/{ano}") } else { ano.to_string() };
            for (nome, tipo, rot, tab) in [("despesas", "despesa", "DESPESAS", "chat_despesa"), ("orcamento", "despesa", "ORCAMENTO-LOA", "chat orcamento"), ("receitas", "receita", "RECEITAS", "chat receita")] {
                let arq = format!("{nome}-{ano}.json");
                let v = js(&src.join(&arq))?;
                let l = lista(&v, &arq)?;
                let mut t = tabela(l, &arq)?;
                if tipo == "despesa" {
                    t = com_coluna(CHAVE, l.iter().map(|x| chave(x).map(|c| marca(&c))).collect::<Result<_, _>>()?, t);
                }
                let out = d::gravar(&p, &format!("{ano}-{mes}_GO-{rot}.txt"), &t)?;
                arqs.push(Arq::novo(ano, tipo, out, format!("TCM-GO Portal do Cidadão ({PORTAL}?pag={nome}&municipio=Cidade), relatório Power BI do tribunal, tabela '{tab}', município Cidade, ano {ano}{} ({meses}); consulta agrupada gravada como veio, uma linha por registro",
                    if ano == "2026" && nome != "orcamento" { format!(", meses 1 a {}", sem_zero(mes)) } else { String::new() })));
            }
        }
        let partes = |c: &str| vec![marca(c)];
        let mut ad = Adaptador { pasta: p, tribunal: "TCM-GO".into(), arquivos: arqs, partes: &partes, codigos: None, faixa_mx: 6,
            col: colunas("'total_liquidacao' (arquivo GO-DESPESAS)", "'valor_fonte' (arquivo GO-ORCAMENTO-LOA, lei do orçamento)", "'total_empenho' (arquivo GO-DESPESAS)", "'total_receita' (arrecadado menos anulado; deduções do Fundeb vêm negativas, natureza 9...)"),
            filtro: filtros(&|a, t| if t == "receita" { "todas as linhas do arquivo GO-RECEITAS".to_string() } else { format!("todas as linhas do arquivo GO-DESPESAS (todos os órgãos do município{})", if a == "2026" { format!(", meses 1 a {})", sem_zero(&mes26)) } else { ")".to_string() }) }) };
        cod_texto::montar(o, "992", "Cidade", "GO", &mut ad, &dados)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::link_docs_capitais::tests::{grava, pasta};
    use serde_json::json;

    pub(crate) fn nada(_: &Path, _: &str, _: &[(String, Vec<String>)]) -> Result<(), String> {
        Ok(())
    }

    /// Site com a pasta do video e o fontes.md; devolve (site, pacote, fontes.md).
    pub(crate) fn cenario(nome: &str, numero: &str, pasta_dados: &str, acao: &str, setor: &str) -> (PathBuf, PathBuf, PathBuf) {
        let raiz = pasta(nome);
        let fontes = raiz.join(format!("site/{numero}-cidade/{numero}. orçamento - fontes.md"));
        grava(&fontes, &format!("| Valor dito | Valor exato | Arquivo | Página / linha |\n|---|---|---|---|\n| Gastou (2025) | R$ 8,00 | painel | soma do liquidado |\n| Festas previa (2025) | R$ 1,00 | painel | ação FESTA ({acao}) · orçado |\n| Festas gastou (2025) | R$ 5,00 | painel | mesma linha · liquidado |\n\n## Setores 2025\n- Cultura: R$ 5,00 ({setor}, p. 0), mesmas páginas do PCMGE065\n"));
        let saida = raiz.join(format!("pacote/{pasta_dados}/saida"));
        grava(&saida.join("acoes-2025-12.json"), &json!([{ "cod": setor, "acao": "FESTA", "liquidado": 5.0 }]).to_string());
        grava(&saida.join("acoes-2026-08.json"), &json!([{ "cod": setor, "acao": "FESTA", "liquidado": 1.0 }]).to_string());
        (raiz.join("site"), raiz.join("pacote"), fontes)
    }

    pub(crate) fn linhas(arq: &Path) -> Vec<Vec<String>> {
        d::ler_linhas(arq, None).unwrap()
    }

    #[test]
    fn ceara_poe_o_codigo_inteiro_na_primeira_coluna() {
        let (site, pacote, fontes) = cenario("ce", "997", "RECEITAS-DESPESAS-FORTALEZA-CE", "1.1.13.392.20.2.50.1", "1.1.13.392.20.2.50.1");
        let src = pacote.join("RECEITAS-DESPESAS-FORTALEZA-CE/tce-ce");
        grava(&src.join("mes26.txt"), "08\n");
        let desp = |org: &str| json!({ "codigo_orgao": org, "codigo_unidade_orcamentaria": "1 ", "codigo_funcao": "13", "codigo_subfuncao": "392", "codigo_programa": "20", "codigo_projeto_atividade": "2", "numero_projeto_atividade": "50", "numero_subprojeto_atividade": "1", "valor_liquidado_ate_mes": 5.0 });
        for (a, m) in [("2024", "12"), ("2025", "12"), ("2026", "08")] {
            grava(&src.join(format!("despesa{a}{m}.json")), &json!({ "data": [desp("1"), desp("2")] }).to_string());
            grava(&src.join(format!("receita{a}{m}.json")), &json!([{ "rubrica": "1112", "valor_arrecadacao_ate_mes": 10.0 }]).to_string());
            grava(&src.join(format!("orcamento{a}.json")), &json!([{ "nome": "FESTA" }]).to_string());
        }
        let o = Onde { site: &site, pacote: &pacote, desenha: &nada };
        let (p, rel) = ce::montar(&o).unwrap();
        assert!(p.ends_with("docs/997-fortaleza-docs") && rel["codigos_nao_achados"] == 0 && rel["arquivos"].as_array().unwrap().len() == 9);
        let t = linhas(&p.join("2026-08_CE-SIM-BALANCETE-DESPESA.txt"));
        assert_eq!((t[0][0].as_str(), t[1][0].as_str(), t[2][0].as_str(), t.len()), ("codigo_acao_video", "1.1.13.392.20.2.50.1", "2.1.13.392.20.2.50.1", 3));
        assert!(p.join("2025-12_CE-SIM-ORCAMENTO-NOMES-ACOES.txt").exists());
        // so a linha da acao exata (a do orgao 2 tem os mesmos pedacos soltos)
        assert!(std::fs::read_to_string(&fontes).unwrap().contains("(1.1.13.392.20.2.50.1) · orçado · coluna 'valor_fixado_orcamento_bal_despesa' (dotação inicial) · 2025-12_CE-SIM-BALANCETE-DESPESA.txt linhas 2 |"));
        // coluna que o tribunal deixou de mandar e mes de corte torto: para
        grava(&src.join("despesa202412.json"), &json!([{ "codigo_orgao": "1" }]).to_string());
        assert!(ce::montar(&o).unwrap_err().contains("despesa202412: arquivo do tribunal sem a coluna codigo_unidade_orcamentaria"));
        grava(&src.join("mes26.txt"), "13");
        assert!(ce::montar(&o).unwrap_err().contains("mês de corte inválido"));
        let _ = std::fs::remove_dir_all(site.parent().unwrap());
    }

    #[test]
    fn minas_confere_linha_por_registro_e_rio_separa_o_pt() {
        let (site, pacote, _) = cenario("mg", "996", "RECEITAS-DESPESAS-BELO-HORIZONTE-MG", "1.2.13.392.20.2050", "1.2.13.392.20.2050");
        let csv = |quebrado: bool| move |arq: &Path, membro: &str| -> Result<Vec<u8>, String> {
            assert!(arq.to_string_lossy().ends_with(".zip") && membro.contains(".3106200."));
            Ok(if membro.contains("despesa") {
                format!("\u{feff}cod_orgao;cod_unidade;dsc_funcao;dsc_subfuncao;dsc_programa;dsc_acao;dsc_subacao;vlr_liquidado\n1;2;13 - Cultura;392 - Difusão;20 - Programa;2050 - Festa;-9 - Sem;5.0\n1;2;13 - Cultura;392 - Difusão;20 - Programa;2050 - Festa;{};3.0\n", if quebrado { "\"7 - Sub\nação\"" } else { "7 - Subação" })
            } else {
                "rubrica;valor\r\n1112;10.0\r\n".to_string()
            }
            .into_bytes())
        };
        let o = Onde { site: &site, pacote: &pacote, desenha: &nada };
        let (p, rel) = mg::montar(&o, &csv(false)).unwrap();
        let t = std::fs::read_to_string(p.join("2026-08_MG-SICOM-DESPESA.txt")).unwrap();
        assert_eq!(t.lines().map(|l| l.split(';').next().unwrap()).collect::<Vec<_>>(), ["codigo_acao_video", "1.2.13.392.20.2050", "1.2.13.392.20.2050.7"]);
        assert!(rel["codigos_nao_achados"] == 0 && std::fs::read_to_string(p.join("2024-12_MG-SICOM-RECEITA.txt")).unwrap() == "rubrica;valor\r\n1112;10.0\r\n");
        assert!(mg::montar(&o, &csv(true)).unwrap_err().contains("campo quebrado em duas linhas"));
        assert!(mg::montar(&o, &|_, _| Ok(b"cod_orgao;x\n1;2\n".to_vec())).unwrap_err().contains("sem a coluna dsc_subacao"));
        let _ = std::fs::remove_dir_all(site.parent().unwrap());

        let (site, pacote, fontes) = cenario("rj", "998", "RECEITAS-DESPESAS-RIO-DE-JANEIRO-RJ", "1010.0130.0113.3920.3001.1234", "1010.0130.0113.3920.3001.1234");
        let src = pacote.join("RECEITAS-DESPESAS-RIO-DE-JANEIRO-RJ/tcm-rj");
        grava(&src.join("mes26.txt"), "07");
        for a in ANOS {
            grava(&src.join(format!("despesas-{a}.json")), &json!([{ "Órgão": "CULTURA", rj::PT: "101001300113392030011234 - Projeto - Carnaval", "Tipo de Despesas": "Despesas Liquidadas", "Valor Despesas": 5.0 }]).to_string());
            grava(&src.join(format!("receitas-{a}.json")), &json!([{ "ORIGEM": "11", "Valor": 10.0 }]).to_string());
        }
        grava(&src.join("origens.json"), &json!({ "11": "Impostos" }).to_string());
        let o = Onde { site: &site, pacote: &pacote, desenha: &nada };
        let (p, _) = rj::montar(&o).unwrap();
        let t = linhas(&p.join("2026-07_RJ-RADAR-CARIOCA-DESPESAS.txt"));
        assert_eq!(t, [vec!["PT (24 dígitos)", "Descrição ação", "Tipo de Despesas", "Valor Despesas", "Órgão"], vec!["101001300113392030011234", "Projeto - Carnaval", "Despesas Liquidadas", "5.0", "CULTURA"]]);
        assert!(linhas(&p.join("2026-07_RJ-RADAR-CARIOCA-ORIGENS.txt"))[1] == ["11", "Impostos"] && std::fs::read_to_string(&fontes).unwrap().contains("2025-12_RJ-RADAR-CARIOCA-DESPESAS.txt linhas 2 |"));
        grava(&src.join("despesas-2025.json"), &json!([{ "Órgão": "CULTURA" }]).to_string());
        assert!(rj::montar(&o).unwrap_err().contains("sem a coluna do PT"));
        let _ = std::fs::remove_dir_all(site.parent().unwrap());
    }

    #[test]
    fn goias_codigo_em_texto_e_grafia_gemea() {
        let cod = "SECRETARIA DE CULTURA.Fundo Municipal de Cultura.13.392";
        let (site, pacote, fontes) = cenario("go", "992", "RECEITAS-DESPESAS-GOIANIA-GO", cod, cod);
        let dados = pacote.join("RECEITAS-DESPESAS-GOIANIA-GO");
        grava(&dados.join("saida/acoes-2024-12.json"), &json!([{ "cod": "SECRETARIA DE CULTURA.FUNDO MUNICIPAL DE CULTURA.13.392" }, { "cod": "SAUDE.FMS.10.301" }]).to_string());
        grava(&dados.join("tcm-go/mes26.txt"), "08");
        let (f, s) = ("Cultura", "Difusão Cultural");
        let reg = |unidade: &str, v: f64| json!({ "orgao": "SECRETARIA DE CULTURA", "unidade_orcamentaria": unidade, "funcao": f, "subfuncao": s, "total_liquidacao": v });
        for a in ANOS {
            grava(&dados.join(format!("tcm-go/despesas-{a}.json")), &json!([reg("Fundo Municipal de Cultura", 5.0), reg("FUNDO MUNICIPAL DE CULTURA", 2.0)]).to_string());
            grava(&dados.join(format!("tcm-go/orcamento-{a}.json")), &json!([reg("Fundo Municipal de Cultura", 1.0)]).to_string());
            grava(&dados.join(format!("tcm-go/receitas-{a}.json")), &json!([{ "natureza": "11", "total_receita": 10.0 }]).to_string());
        }
        let o = Onde { site: &site, pacote: &pacote, desenha: &nada };
        let (p, rel) = go::montar(&o).unwrap();
        let t = linhas(&p.join("2025-12_GO-DESPESAS.txt"));
        assert_eq!((t[1][0].as_str(), t[2][0].as_str()), ("SECRETARIA DE CULTURA.Fundo Municipal de Cultura.13.392 (grafia 2)", "SECRETARIA DE CULTURA.FUNDO MUNICIPAL DE CULTURA.13.392 (grafia 1)"));
        // o codigo em texto e achado no quadro principal, em "mesma linha" e nos setores; a grafia gemea nao entra junto
        let md = std::fs::read_to_string(&fontes).unwrap();
        assert!(md.contains("· orçado · coluna 'valor_fonte' (arquivo GO-ORCAMENTO-LOA, lei do orçamento) · 2025-12_GO-DESPESAS.txt linhas 2; 2025-12_GO-ORCAMENTO-LOA.txt linhas 2 |"), "{md}");
        assert!(md.contains("| mesma linha · liquidado · coluna 'total_liquidacao' (arquivo GO-DESPESAS) · 2025-12_GO-DESPESAS.txt linhas 2; 2025-12_GO-ORCAMENTO-LOA.txt linhas 2 |"));
        assert!(md.contains(&format!("({cod}, 2025-12_GO-DESPESAS.txt linhas 2; 2025-12_GO-ORCAMENTO-LOA.txt linhas 2), mesmas linhas no arquivo do tribunal (TCM-GO)")));
        assert_eq!((&rel["codigos_nao_achados"], &rel["setor_sem_linha"]), (&json!(0), &json!(0)));
        assert_eq!(md, std::fs::read_to_string(p.join("992. orçamento - fontes.md")).unwrap());
        assert!(cod_texto::acha("A.B.13", "ação (A.B.13) x") && !cod_texto::acha("A.B.13", "A.B.13.392") && !cod_texto::acha("B.13", "A.B.13") && !cod_texto::acha("", "x"));
        // funcao que o molde nao conhece: para
        grava(&dados.join("tcm-go/despesas-2024.json"), &json!([{ "orgao": "X", "unidade_orcamentaria": "Y", "funcao": "Lazer Novo", "subfuncao": s }]).to_string());
        assert!(go::montar(&o).unwrap_err().contains("função/subfunção nova no TCM-GO"));
        let _ = std::fs::remove_dir_all(site.parent().unwrap());
    }

    #[test]
    fn vitoria_codigo_com_letra_e_cuiaba_do_radar() {
        let cod = "077E0300001.07.01.13.392.0020.2050";
        let (site, pacote, fontes) = cenario("es", "976", "RECEITAS-DESPESAS-VITORIA-ES", "07.01.13.392.0020.2050", cod);
        let dados = pacote.join("RECEITAS-DESPESAS-VITORIA-ES");
        grava(&dados.join("saida/festas-cods.json"), &json!({ "2025": [cod], "2026": [] }).to_string());
        grava(&dados.join("tce-es/mes26.txt"), "08");
        let reg = |mes: i64, acao: &str| json!({ "Mes": mes, "CodigoUnidadeGestora": "077E0300001", "CodigoOrgao": "07", "CodigoUnidadeOrcamentaria": "01", "CodigoFuncao": "13", "CodigoSubFuncao": "392", "CodigoPrograma": "0020", "CodigoAcao": acao, "Liquidada": 1.0 });
        for a in ANOS {
            grava(&dados.join(format!("tce-es/despesas{a}.json")), &json!([reg(1, "2050"), reg(9, "2050"), reg(2, "2051")]).to_string());
            grava(&dados.join(format!("tce-es/receitas{a}.json")), &json!([{ "Mes": "3", "Arrecadada": 10.0 }]).to_string());
        }
        let o = Onde { site: &site, pacote: &pacote, desenha: &nada };
        let (p, rel) = es::montar(&o).unwrap();
        assert_eq!((linhas(&p.join("2026-08_ES-CIDADES-PCM-DESPESAS.txt")).len(), linhas(&p.join("2025-12_ES-CIDADES-PCM-DESPESAS.txt")).len()), (3, 4)); // setembro de 2026 fica fora
        let md = std::fs::read_to_string(&fontes).unwrap();
        // no quadro principal o codigo perdeu o prefixo com letra: volta pela lista de festas; no setor o codigo com letra ganha as linhas
        assert!(md.contains("(07.01.13.392.0020.2050) · orçado · coluna 'PrevisaoInicial' (somada nas linhas da ação) · 2025-12_ES-CIDADES-PCM-DESPESAS.txt linhas 2-3 |"), "{md}");
        assert!(md.contains(&format!("({cod}, 2025-12_ES-CIDADES-PCM-DESPESAS.txt linhas 2-3)")) && rel["codigos_nao_achados"] == 0);
        grava(&dados.join("tce-es/despesas2025.json"), &json!([{ "Mes": 1, "CodigoOrgao": "07" }]).to_string());
        assert!(es::montar(&o).unwrap_err().contains("despesas2025.json: registro sem CodigoUnidadeGestora"));
        let _ = std::fs::remove_dir_all(site.parent().unwrap());

        let (site, pacote, fontes) = cenario("mt", "983", "RECEITAS-DESPESAS-CUIABA-MT", "1118.0.13.392.0.2050", "1118.0.13.392.0.2050");
        let radar = pacote.join("RECEITAS-DESPESAS-CUIABA-MT/radar");
        let (mut dj, mut rj) = (json!({ "mes26": "08", "meses_por_orgao": [["2026", "CAMARA", "01", 1.0]] }), json!({}));
        for a in ANOS {
            dj[format!("acoes-{a}")] = json!([[1118.0, "13", 392, "2050", "FESTA", 5.0, 6.0], ["1118", 13, 392, "2051", "OUTRA", 1.0, 1.0]]);
            dj[format!("total-{a}")] = json!([["CUIABA", 6.0, 7.0, 5.5]]);
            dj[format!("orgaos-{a}")] = json!([["PREFEITURA", 6.0, 7.0]]);
            (rj[format!("origens-{a}")], rj[format!("grupos-{a}")], rj[format!("total-{a}")]) = (json!([["Impostos", 10.0]]), json!([["Correntes", 10.0]]), json!([["CUIABA", 10.0]]));
        }
        grava(&radar.join("despesa.json"), &dj.to_string());
        grava(&radar.join("receita.json"), &rj.to_string());
        let o = Onde { site: &site, pacote: &pacote, desenha: &nada };
        let (p, rel) = mt::montar(&o).unwrap();
        let t = linhas(&p.join("2025-12_MT-RADAR-DESPESA.txt"));
        assert_eq!(t[1], ["1118.0.13.392.0.2050", "ações", "1118.0", "13", "392", "2050", "FESTA", "5.0", "6.0", ""]);
        assert_eq!((t[3].clone(), t[4].clone()), (vec!["", "total do município", "CUIABA", "", "", "", "", "6.0", "7.0", "5.5"].into_iter().map(String::from).collect::<Vec<_>>(), vec!["", "por órgão (jurisdicionado)", "PREFEITURA", "", "", "", "", "6.0", "7.0", ""].into_iter().map(String::from).collect::<Vec<_>>()));
        assert!(rel["arquivos"].as_array().unwrap().len() == 7 && std::fs::read_to_string(&fontes).unwrap().contains("coluna - (o Radar não traz a dotação por ação) · 2025-12_MT-RADAR-DESPESA.txt linhas 2 |"));
        dj["acoes-2025"] = json!([["1118", "treze", 392, "2050", "FESTA", 5.0, 6.0]]);
        grava(&radar.join("despesa.json"), &dj.to_string());
        assert!(mt::montar(&o).unwrap_err().contains("função: código que não é número"));
        let _ = std::fs::remove_dir_all(site.parent().unwrap());
    }
}
