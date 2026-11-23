//! Molde de Rondonia (link/tribunais/ro.py), Porto Velho. PENDENTE NO TRIBUNAL: nenhuma base publica do TCE-RO entrega hoje
//! 2024-2026 por acao sem login/captcha (To no Controle em manutencao; SIGAP publico so ate 2020; Portal do Cidadao com login;
//! PCe com captcha; Radar so por funcao e vazio). O molde confere as bases e PARA com a lista: nada e estimado nem vem de outra
//! fonte. Quando o To no Controle voltar, falta escrever a leitura com a estrutura nova dele.
//! Entram como closure: `abre(url)` = titulo + comeco do texto + endereco da pagina (aba do navegador, uma por vez);
//! `projetos(ano)` = JSON da consulta de projetos/atividades do SIGAP (entidade 37).
#![allow(dead_code)]
use crate::link_tribunais_base as base;
use crate::link_tribunais_util as u;
use crate::texto_pt::sem_acento;
use serde_json::{json, Value};

pub const IBGE: i64 = 1100205;
const TNC: &str = "Tô no Controle (tonocontrole.tcero.tc.br, sistema do guia)";
pub type Abre<'a> = &'a dyn Fn(&str) -> Result<String, String>;

pub fn tribunal() -> Value {
    json!({ "sigla": "TCE-RO", "fala": "Tribunal de Contas do Estado", "uf": "RO", "estado": "Rondônia", "doc_despesa": "TÔ NO CONTROLE DESPESAS", "doc_receita": "TÔ NO CONTROLE RECEITAS",
        "tribunal_estado": "TCE-RO (Tribunal de Contas do Estado de Rondônia)", "site_tribunal": "tonocontrole.tcero.tc.br",
        "fonte_md": "Tudo do TCE-RO (Tribunal de Contas do Estado de Rondônia), portal Tô no Controle.",
        "setores_md": "Setores = soma do liquidado das ações pela função; festas e eventos separados da cultura; segurança pública fora.",
        "pendencias_md": [], "orgaos": "Município inteiro (Prefeitura, Câmara, autarquias, fundações e fundos)",
        "legenda_docs": ["• Receita e despesa: TCE-RO, Tô no Controle", "• tonocontrole.tcero.tc.br"] })
}

/// Uma consulta por vez em cada base publica do TCE-RO; devolve [(base, situacao)] do que falta.
pub fn verificar(abre: Abre, projetos: Abre) -> Result<Vec<(String, String)>, String> {
    let mut falta: Vec<(String, String)> = vec![];
    let t = abre("https://tonocontrole.tcero.tc.br/")?;
    if t.to_lowercase().contains("manuten") {
        falta.push((TNC.into(), "página em manutenção".into()));
    } else if t.contains("chrome-error") || t.contains("ERR_") {
        // o dominio sumiu do DNS e a pagina de erro do navegador passava por "voltou"
        falta.push((TNC.into(), "fora do ar (o endereço não existe no DNS em 16/11/2026)".into()));
    }
    for ano in base::ANOS {
        let r = projetos(ano)?;
        let pa = u::json(if r.trim().is_empty() { "[]" } else { &r }, "SIGAP")?;
        if pa.is_null() || pa.as_array().is_some_and(Vec::is_empty) || pa.as_object().is_some_and(|o| o.is_empty()) {
            falta.push((format!("Consulta pública SIGAP (www.tce.ro.gov.br/sigap), {ano}"), "sem dados (a consulta pública para em 2020)".into()));
        }
    }
    if abre("https://portalcidadao.tcero.tc.br/")?.to_lowercase().contains("login") {
        falta.push(("Portal do Cidadão (portalcidadao.tcero.tc.br)".into(), "exige login: parar".into()));
    }
    falta.push(("Consulta de processos PCe (prestação de contas anual)".into(), "a busca exige captcha: parar".into()));
    falta.push(("Radar Cidadão (radar.tcero.tc.br, API radarcidadao-api)".into(), "só por função (sem ação) e devolveu 'Nenhuma despesa encontrada' em 2023-2025 (15/11/2026)".into()));
    Ok(falta)
}

/// O que a esteira chama: sempre para (pendencia), com o motivo.
pub fn preparar(abre: Abre, projetos: Abre, nome: &str) -> Result<Value, String> {
    if sem_acento(nome).to_uppercase() != "PORTO VELHO" {
        return Err("o molde do TCE-RO foi feito para Porto Velho (UG 37 no SIGAP)".into());
    }
    let falta = verificar(abre, projetos)?;
    if falta.iter().any(|(b, _)| b.contains("Tô no Controle")) {
        return Err(format!("TCE-RO sem base pública com despesa por ação de 2024-2026 hoje:\n  {}", falta.iter().map(|(b, s)| format!("{b}: {s}")).collect::<Vec<_>>().join("\n  ")));
    }
    Err("Tô no Controle voltou: falta escrever ro.ler() com a estrutura nova dele (nada foi estimado)".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confere_as_bases_e_sempre_para() {
        let manut = |url: &str| Ok(if url.contains("tonocontrole") { "TCE-RO | Página em Manutenção | https://tonocontrole.tcero.tc.br/" } else { "Entrar | Faça seu Login com gov.br | x" }.to_string());
        let vazio = |ano: &str| Ok(if ano == "2025" { String::new() } else { "[]".to_string() });
        let f = verificar(&manut, &vazio).unwrap();
        assert_eq!(f.len(), 7); // To no Controle + 3 anos do SIGAP + login + captcha + radar
        assert!(f[0].1 == "página em manutenção" && f[2].0.ends_with("2025") && f[4].1 == "exige login: parar");
        let e = preparar(&manut, &vazio, "Porto Velho").unwrap_err();
        assert!(e.starts_with("TCE-RO sem base pública") && e.contains("\n  Consulta de processos PCe (prestação de contas anual): a busca exige captcha: parar"));
        // pagina de erro do navegador nao passa por "voltou"
        let dns = |_: &str| Ok("tonocontrole.tcero.tc.br | ERR_NAME_NOT_RESOLVED | chrome-error://chromewebdata/".to_string());
        assert!(verificar(&dns, &vazio).unwrap()[0].1.starts_with("fora do ar"));
        // mesmo com o portal de volta e dado no SIGAP, o molde para: a leitura nova ainda nao existe
        let voltou = |_: &str| Ok("Tô no Controle | Painel".to_string());
        let com = |_: &str| Ok(r#"[{"id":1}]"#.to_string());
        assert_eq!(verificar(&voltou, &com).unwrap().len(), 2);
        assert!(preparar(&voltou, &com, "Porto Velho").unwrap_err().contains("falta escrever ro.ler()"));
        assert!(preparar(&voltou, &com, "Ji-Paraná").unwrap_err().contains("feito para Porto Velho"));
        assert!(verificar(&|_| Err("aba não abriu".into()), &com).is_err() && verificar(&voltou, &|_| Ok("<html>".into())).is_err());
        assert_eq!(tribunal()["sigla"], "TCE-RO");
    }
}
