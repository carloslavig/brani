//! Molde do Espirito Santo (link/tribunais/es.py): dados abertos do CidadES do TCE-ES (Prestacao de Contas Mensal), CSV de
//! despesas e receitas dos municipios e a lista de remessas homologadas. 2026 vale ate o ultimo mes que TODAS as unidades
//! gestoras do municipio homologaram; receita ou despesa vazia, receita sem origem conhecida, coluna que falta ou valor que
//! nao e numero = Err (pendencia, nunca video).
//! Entra como closure: `texto(tipo, ano)` devolve o CSV ja aberto do zip e lido como cp1252 (tipo = "despesas" | "receitas";
//! "pcm" com ano "" = obrigacoes da PCM). Ficou de fora: baixar, o cache de 20 dias e guardar so as linhas do municipio em disco.
#![allow(dead_code)]
use crate::link_tribunais_base::{self as base, col, csv, Dados, Linha};
use crate::link_tribunais_util::{self as u, ini, n_br, Acoes};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Vitoria poe festas e eventos na acao da Companhia de Desenvolvimento, Turismo e Inovacao, sem "festa" no nome.
pub const FESTAS_EXTRA: &str = "EVENTOS TUR[IÍ]STICOS E CULTURAIS|PROMO[CÇ][AÃ]O DE EVENTOS TUR[IÍ]STICOS";
pub const IBGE_VITORIA: i64 = 3205309;
pub type Texto<'a> = &'a dyn Fn(&str, &str) -> Result<String, String>;

pub fn tribunal() -> Value {
    json!({ "sigla": "TCE-ES", "fala": "Tribunal de Contas do Estado", "uf": "ES", "estado": "Espírito Santo", "doc_despesa": "CIDADES PCM DESPESAS", "doc_receita": "CIDADES PCM RECEITAS",
        "tribunal_estado": "TCE-ES (Tribunal de Contas do Estado do Espírito Santo)", "site_tribunal": "paineldecontrole.tcees.tc.br",
        "fonte_md": "Tudo do CidadES do TCE-ES (Tribunal de Contas do Estado do Espírito Santo), a base do Painel de Controle, pelos dados abertos que o TCE-ES publica (dados.es.gov.br, organização TCE-ES: despesas-municipios e receitas-municipios, remessas mensais homologadas da Prestação de Contas Mensal). Receita = arrecadado já líquido da dedução do Fundeb, por origem. \"Gastou\" = LIQUIDADO acumulado no ano, todas as unidades gestoras do município. \"Previa\" = previsão inicial da despesa (dotação inicial da lei).",
        "setores_md": "Setores = soma do liquidado das ações (projeto/atividade) do CidadES, pela função; festas e eventos separados da cultura; segurança pública fora.",
        "pendencias_md": [], "orgaos": "Município inteiro (Câmara, secretarias, fundos, institutos e empresa dependente que mandam a PCM ao CidadES)",
        "legenda_docs": ["• Receita: CidadES do TCE-ES, receitas dos municípios de 2024, 2025 e 2026 (arrecadado líquido do Fundeb)", "• Despesa, setores e festas: CidadES do TCE-ES, despesas dos municípios (previsão inicial e liquidado), por ação", "• paineldecontrole.tcees.tc.br · dados abertos do TCE-ES em dados.es.gov.br"] })
}

/// URL do recurso pelo nome no catalogo (os ids mudam quando o tribunal republica). `catalogo` = resposta do package_show.
/// Conjunto que nao e do TCE-ES ou recurso que nao e exatamente um = Err.
pub fn recurso(catalogo: &str, pacote: &str, nome: &str) -> Result<String, String> {
    let r = &u::json(catalogo, pacote)?["result"];
    let org = u::txt(&r["organization"], "name")?;
    if !org.contains("tcees") {
        return Err(format!("{pacote}: conjunto não é do TCE-ES ({org})"));
    }
    let mut urls = vec![];
    for x in u::lista(&r["resources"], pacote)? {
        if u::txt(x, "name")?.trim().to_lowercase() == nome.to_lowercase() {
            urls.push(u::txt(x, "url")?.to_string());
        }
    }
    if urls.len() != 1 {
        return Err(format!("TCE-ES: recurso '{nome}' não achado em {pacote}"));
    }
    Ok(urls.remove(0))
}

/// Ultimo mes de 2026 que TODAS as unidades gestoras do municipio (as que mandaram alguma remessa em 2026) homologaram na PCM.
pub fn mes26(pcm: &str, mun: &str) -> Result<String, String> {
    let mut h: BTreeMap<String, BTreeSet<i64>> = BTreeMap::new();
    for x in csv(pcm, ';') {
        if col(&x, "Município")? == mun && col(&x, "Ano")? == "2026" && !col(&x, "DataHomologacao")?.is_empty() {
            h.entry(col(&x, "UnidadeGestora")?.to_string()).or_default().insert(u::int(col(&x, "Mes")?, "Mes")?);
        }
    }
    if h.is_empty() {
        return Err(format!("TCE-ES: nenhuma remessa de 2026 homologada para {mun}"));
    }
    let mut m = 0;
    while h.values().all(|s| s.contains(&(m + 1))) {
        m += 1;
    }
    if m == 0 {
        return Err("TCE-ES: nenhum mês de 2026 homologado por todas as unidades gestoras".into());
    }
    Ok(format!("{m:02}"))
}

/// As linhas do municipio ate o mes de corte.
fn do_municipio(texto: &str, mun: &str, ate: i64) -> Result<Vec<Linha>, String> {
    let mut out = vec![];
    for x in csv(texto, ';') {
        if col(&x, "EsferaAdministrativa")? == mun && u::int(col(&x, "Mes")?, "Mes")? <= ate {
            out.push(x);
        }
    }
    Ok(out)
}

/// Le os 3 anos do municipio `mun` (o nome como o CidadES escreve).
pub fn ler(texto: Texto, mun: &str, mes26: &str) -> Result<Dados, String> {
    let m26 = u::int(mes26, "mês de corte")?;
    if !(1..=12).contains(&m26) {
        return Err(format!("mês de corte de 2026 inválido: {mes26:?}"));
    }
    let mut dados = Dados::new();
    for a in base::ANOS {
        let ate = if a == "2026" { m26 } else { 12 };
        let rec = do_municipio(&texto("receitas", a)?, mun, ate)?;
        let des = do_municipio(&texto("despesas", a)?, mun, ate)?;
        if rec.is_empty() || des.is_empty() {
            return Err(format!("TCE-ES: {} de {a} vazia no CidadES", if rec.is_empty() { "receita" } else { "despesa" }));
        }
        let mut orig = vec![];
        for r in &rec {
            let c = col(r, "ClassificacaoReceita")?.trim();
            let nome = u::origem(ini(c, 2)).or(c.starts_with(['7', '8']).then_some(u::INTRA)).ok_or_else(|| format!("TCE-ES: receita sem origem conhecida: {c}"))?;
            u::soma(&mut orig, nome, n_br(col(r, "Arrecadada")?)?); // ja liquida do Fundeb (a deducao vem em ArrecadadaFUNDEB)
        }
        let mut acoes = Acoes::default();
        for r in &des {
            let ug = col(r, "CodigoUnidadeGestora")?;
            let k = [ug, col(r, "CodigoOrgao")?, col(r, "CodigoUnidadeOrcamentaria")?, col(r, "CodigoFuncao")?, col(r, "CodigoSubFuncao")?, col(r, "CodigoPrograma")?, col(r, "CodigoAcao")?].join(".");
            let x = acoes.entra(&k);
            // Camara: so a funcao 01 (legislativa) da propria Camara (4a letra do codigo da unidade = L); o resto vai para "Outros"
            let f = col(r, "CodigoFuncao")?;
            x.f = if f == "01" && ug.chars().nth(3) != Some('L') { "0".into() } else { f.into() };
            (x.sub, x.acao, x.cod) = (col(r, "CodigoSubFuncao")?.into(), col(r, "NomeAcao")?.trim().into(), k);
            x.orcado += n_br(col(r, "PrevisaoInicial")?)?;
            x.atualizado += n_br(col(r, "PrevisaoAtualizada")?)?;
            x.empenhado += n_br(col(r, "Empenhada")?)?;
            x.liquidado += n_br(col(r, "Liquidada")?)?;
        }
        let per = format!("jan-{}/{a}", base::ABR[ate as usize - 1]);
        dados.insert(a.into(), u::ano(acoes.lista, orig, format!("CidadES TCE-ES · despesas dos municípios {per}"), format!("CidadES TCE-ES · receitas dos municípios {per}")));
    }
    Ok(dados)
}

/// O que a esteira chama para uma cidade do ES (`ibge` da cidade; o da capital e IBGE_VITORIA).
pub fn preparar(texto: Texto, setores: &dyn Fn(&Path) -> Result<(), String>, numero: &str, nome: &str, pasta: &Path, ibge: i64) -> Result<Value, String> {
    let m = mes26(&texto("pcm", "")?, nome)?;
    let dados = ler(texto, nome, &m)?;
    base::montar(pasta, numero, nome, "ES", ibge, &tribunal(), &dados, &m, &base::Opcoes { festas_extra: Some(FESTAS_EXTRA), ..Default::default() }, setores)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAB_D: &str = "EsferaAdministrativa;Mes;CodigoUnidadeGestora;CodigoOrgao;CodigoUnidadeOrcamentaria;CodigoFuncao;CodigoSubFuncao;CodigoPrograma;CodigoAcao;NomeAcao;PrevisaoInicial;PrevisaoAtualizada;Empenhada;Liquidada";
    const CAB_R: &str = "EsferaAdministrativa;Mes;ClassificacaoReceita;Arrecadada";

    fn despesas(ate: u32) -> String {
        let mut t = CAB_D.to_string();
        for m in 1..=ate {
            t += &format!("\nVitória;{m};077E0700001;10;01;13;392;0001;2001; EVENTOS TURISTICOS E CULTURAIS ;{};1.100,00;30,50;20,25", if m == 1 { "1.000,00" } else { "" });
            t += &format!("\nVitória;{m};077L0200001;01;01;01;031;0002;2002;PROCESSO LEGISLATIVO;0;0;10,00;10,00");
            t += &format!("\nVitória;{m};077E0700001;10;01;01;031;0002;2003;APOIO;0;0;1,00;1,00");
            t += &format!("\nSerra;{m};069E0700001;10;01;12;361;0003;2004;ESCOLAS;0;0;999,00;999,00");
        }
        t
    }

    fn receitas(ate: u32) -> String {
        let mut t = CAB_R.to_string();
        for m in 1..=ate {
            t += &format!("\nVitória;{m};1112;1.000,00\nVitória;{m};1751;500,00\nVitória;{m};7215;40,00\nSerra;{m};1112;9,00");
        }
        t
    }

    fn pcm() -> String {
        "Município;Ano;Mes;UnidadeGestora;DataHomologacao\nVitória;2026;1;Prefeitura;01/02/2026\nVitória;2026;2;Prefeitura;01/03/2026\nVitória;2026;3;Prefeitura;01/04/2026\nVitória;2026;1;Câmara;01/02/2026\nVitória;2026;2;Câmara;02/03/2026\nVitória;2026;3;Câmara;\nVitória;2025;9;Câmara;x\nSerra;2026;5;Prefeitura;x".into()
    }

    fn fonte(tipo: &str, ano: &str) -> Result<String, String> {
        Ok(match tipo {
            "pcm" => pcm(),
            "despesas" => despesas(if ano == "2026" { 5 } else { 12 }),
            _ => receitas(if ano == "2026" { 5 } else { 12 }),
        })
    }

    #[test]
    fn corte_pela_homologacao_e_leitura_por_acao() {
        assert_eq!(mes26(&pcm(), "Vitória").unwrap(), "02"); // a Camara nao homologou marco
        let dados = ler(&fonte, "Vitória", "02").unwrap();
        let a = &dados["2026"];
        assert_eq!(a.acoes.len(), 3); // a Serra fica de fora
        let x = &a.acoes[0];
        assert_eq!((x.acao.as_str(), x.cod.as_str(), x.f.as_str(), x.sub.as_str()), ("EVENTOS TURISTICOS E CULTURAIS", "077E0700001.10.01.13.392.0001.2001", "13", "392"));
        assert_eq!((x.orcado, x.atualizado, x.empenhado, x.liquidado), (1000.0, 2200.0, 61.0, 40.5));
        assert!(base::eh_festa(&x.acao, Some(&regex::Regex::new(FESTAS_EXTRA).unwrap())));
        assert_eq!((a.acoes[1].f.as_str(), a.acoes[2].f.as_str()), ("01", "0")); // funcao 01 so da Camara
        assert_eq!(a.receita, [("Impostos, taxas e contribuições de melhoria".to_string(), 2000.0), ("Transferências correntes".to_string(), 1000.0), (u::INTRA.to_string(), 80.0)]);
        assert_eq!((a.arrecadado, a.liquidado, a.doc_despesa.as_str()), (3080.0, 62.5, "CidadES TCE-ES · despesas dos municípios jan-fev/2026"));
        assert_eq!((dados["2024"].liquidado, dados["2025"].doc_receita.as_str()), (12.0 * 31.25, "CidadES TCE-ES · receitas dos municípios jan-dez/2025"));
    }

    #[test]
    fn recusa_o_que_falta() {
        assert!(mes26(&pcm(), "Cariacica").unwrap_err().contains("nenhuma remessa de 2026 homologada"));
        assert!(mes26(&pcm().replace("Vitória;2026;1;Câmara;01/02/2026", "Vitória;2026;1;Câmara;"), "Vitória").unwrap_err().contains("nenhum mês de 2026 homologado por todas"));
        assert!(mes26(&pcm().replace("DataHomologacao", "Outra"), "Vitória").unwrap_err().contains("sem a coluna DataHomologacao"));
        assert!(ler(&fonte, "Cariacica", "02").unwrap_err().contains("receita de 2024 vazia"));
        assert!(ler(&fonte, "Vitória", "13").unwrap_err().contains("inválido"));
        let ruim = |de: &'static str, para: &'static str| move |tipo: &str, ano: &str| fonte(tipo, ano).map(|t| t.replace(de, para));
        assert!(ler(&ruim(";1112;", ";9112;"), "Vitória", "02").unwrap_err().contains("receita sem origem conhecida: 9112"));
        assert!(ler(&ruim("20,25", "vinte"), "Vitória", "02").unwrap_err().contains("não é número"));
        assert!(ler(&ruim("Liquidada", "Paga"), "Vitória", "02").unwrap_err().contains("sem a coluna Liquidada"));
        assert!(ler(&ruim("Vitória;3;", "Vitória;mar;"), "Vitória", "02").unwrap_err().contains("Mes que não é número"));
        // catalogo: so vale conjunto do TCE-ES e recurso unico
        let cat = r#"{"result":{"organization":{"name":"tcees"},"resources":[{"name":" Municipios-Despesas-2025.zip ","url":"https://x/d.zip"},{"name":"outro","url":"u"}]}}"#;
        assert_eq!(recurso(cat, "despesas-municipios", "municipios-despesas-2025.zip").unwrap(), "https://x/d.zip");
        assert!(recurso(cat, "p", "municipios-despesas-2024.zip").unwrap_err().contains("não achado"));
        assert!(recurso(&cat.replace("tcees", "sefaz"), "p", "outro").unwrap_err().contains("não é do TCE-ES"));
    }
}
