//! Reuniao da manha (porte de braniac/reuniao_matinal.py): antes do bom dia os agentes passam ao Jarvis o que tem a reportar;
//! quem nao tem nada fica calado. Cada relato e local (nenhuma IA), curto e falavel. Os agentes so RELATAM: nenhum age.
//! Aqui ficam a reuniao em si (`reunir`, que registra pedido e resposta no barramento, sem o conteudo), a fala e os relatos que
//! sao so texto: Cofre, Correio, Jorge (Airbnb) e Tino (qualidade). Cada relato entra em `reunir` como closure.
//! Ficou de fora: relatos do Compasso (agenda), Claudinho e Nando (dependem de modulos nao portados) e a leitura/limpeza da
//! caixa do Correio (aqui so o texto a partir dos itens; quem chama zera a lista depois de entregar).
#![allow(dead_code)]
use crate::tempo::Data;
use serde_json::Value;
use std::path::Path;

fn brl(v: &Value) -> String {
    crate::cofre::brl(v.as_f64().unwrap_or(0.0)).replace("R$ ", "")
}

/// Relato do Cofre a partir da avaliacao (as chaves de cofre::avaliar). `lido` = "HH:MM" da leitura do saldo, ou "".
pub fn relato_cofre(av: &Value, lido: &str) -> String {
    if !av.is_object() {
        return String::new();
    }
    let prazo = Data::de_iso(av["prazo"].as_str().unwrap_or("")).map(|d| d.dia_mes()).unwrap_or_default();
    if av["nivel"] == "batida" {
        return format!("A meta está batida: o saldo é {}, e a meta de {} vale até {prazo}.", brl(&av["saldo"]), brl(&av["meta"]));
    }
    let lido = if lido.is_empty() { String::new() } else { format!(" (lido às {lido})") };
    format!("O saldo{lido} é {} e faltam {} para bater a meta de {} até {prazo}, o quinto dia útil, com {} dia(s) útil(eis). Para manter o ritmo, hoje precisam entrar uns {}.",
        brl(&av["saldo"]), brl(&av["falta"]), brl(&av["meta"]), av["dias_uteis"], brl(&av["precisa_por_dia_util"]))
}

/// Relato do Correio: `itens` = [{"tipo": "pessoa"|"trabalho", "quem", "assunto"}] guardados desde a ultima reuniao. Nada e enviado.
pub fn relato_correio(itens: &[Value]) -> String {
    let lista = |tipo: &str| itens.iter().filter(|i| i["tipo"] == tipo).take(3)
        .map(|i| format!("{}, {}", i["quem"].as_str().unwrap_or("?"), i["assunto"].as_str().unwrap_or("?"))).collect::<Vec<_>>().join("; ");
    if itens.is_empty() {
        return String::new();
    }
    let (vagas, pessoas, mut f) = (lista("trabalho"), lista("pessoa"), Vec::new());
    if !vagas.is_empty() {
        f.push(format!("Chegou possível oferta de trabalho: {vagas}."));
    }
    if !pessoas.is_empty() {
        f.push(format!("E-mail de pessoa: {pessoas}."));
    }
    f.join(" ") + " Quer que eu te ajude a redigir uma resposta? Eu não envio nada sozinho."
}

/// Relato do Jorge: consultas do Airbnb (dados/airbnb_estado.json) paradas esperando o OK de alguem da casa.
pub fn relato_jorge(dados: &Path) -> String {
    let est: Value = std::fs::read_to_string(dados.join("airbnb_estado.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    let pend: Vec<&str> = est["consultas"].as_object().into_iter().flatten().filter(|(_, c)| c["estado"] == "consulta").map(|(_, c)| c["hospede"].as_str().unwrap_or("?")).collect();
    if pend.is_empty() {
        return String::new();
    }
    format!("No Airbnb, {} reserva(s) ou hóspede(s) esperando o OK de vocês: {}.", pend.len(), pend.iter().take(4).copied().collect::<Vec<_>>().join(", "))
}

/// Relato do Tino a partir de qualidade::medir(.., 1 dia, ..): so fala se houve correcao.
pub fn relato_tino(trocas: usize, correcoes: usize) -> String {
    if correcoes == 0 {
        return String::new();
    }
    format!("Nas últimas vinte e quatro horas você corrigiu o Jarvis {correcoes} vez(es) em {trocas} conversas.")
}

/// [(agente, texto)] so de quem tem algo a dizer, na ordem recebida (no Python: Cofre, Correio, Nando, Jorge, Compasso, Tino, Claudinho).
/// Registra no barramento o pedido do Jarvis e se cada um tinha algo; o texto do relato nao vai para o barramento.
pub fn reunir(dados: &Path, relatos: &[(&str, &dyn Fn() -> String)]) -> Vec<(String, String)> {
    let mut saida = Vec::new();
    for (nome, relato) in relatos {
        let texto = relato().trim().to_string();
        let _ = crate::barramento::enviar(dados, "Jarvis", nome, "pedido", "reunião da manhã: o que você tem a reportar?", 0, None);
        let _ = crate::barramento::enviar(dados, nome, "Jarvis", "resposta", if texto.is_empty() { "nada a reportar" } else { "tenho algo a reportar" }, 0, None);
        if !texto.is_empty() {
            saida.push((nome.to_string(), texto));
        }
    }
    saida
}

/// Bloco falado da reuniao, ou "" se ninguem tem nada.
pub fn para_fala(r: &[(String, String)]) -> String {
    if r.is_empty() {
        return String::new();
    }
    format!("Passei pelos agentes antes de falar com você. {}", r.iter().map(|(n, t)| format!("{n} reporta: {t}")).collect::<Vec<_>>().join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn relatos_falam_so_quando_ha_o_que_dizer() {
        assert_eq!(relato_cofre(&Value::Null, ""), "");
        let av = json!({"nivel": "atencao", "saldo": 1234.5, "meta": 2000.0, "falta": 765.5, "prazo": "2026-12-07", "dias_uteis": 4, "precisa_por_dia_util": 191.375});
        assert_eq!(relato_cofre(&av, "05:40"), "O saldo (lido às 05:40) é 1.234,50 e faltam 765,50 para bater a meta de 2.000,00 até 07/12, o quinto dia útil, com 4 dia(s) útil(eis). Para manter o ritmo, hoje precisam entrar uns 191,38.");
        assert_eq!(relato_cofre(&json!({"nivel": "batida", "saldo": 2100, "meta": 2000, "prazo": "2026-12-07"}), ""), "A meta está batida: o saldo é 2.100,00, e a meta de 2.000,00 vale até 07/12.");
        assert_eq!(relato_correio(&[]), "");
        let itens = [json!({"tipo": "pessoa", "quem": "Ana", "assunto": "orçamento"}), json!({"tipo": "trabalho", "quem": "Empresa X", "assunto": "vaga"})];
        assert_eq!(relato_correio(&itens), "Chegou possível oferta de trabalho: Empresa X, vaga. E-mail de pessoa: Ana, orçamento. Quer que eu te ajude a redigir uma resposta? Eu não envio nada sozinho.");
        assert_eq!((relato_tino(12, 0), relato_tino(12, 2)), (String::new(), "Nas últimas vinte e quatro horas você corrigiu o Jarvis 2 vez(es) em 12 conversas.".to_string()));
    }

    #[test]
    fn reuniao_junta_quem_tem_algo_e_registra_no_barramento() {
        let p = std::env::temp_dir().join(format!("braniac-reuniao-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        assert_eq!(relato_jorge(&p), "");
        std::fs::write(p.join("airbnb_estado.json"), r#"{"consultas": {"a": {"estado": "consulta", "hospede": "Ana"}, "b": {"estado": "respondida", "hospede": "Bia"}, "c": {"estado": "consulta"}}}"#).unwrap();
        assert_eq!(relato_jorge(&p), "No Airbnb, 2 reserva(s) ou hóspede(s) esperando o OK de vocês: Ana, ?.");
        // o barramento so aceita agente conhecido (dados/agentes.json); desconhecido fica sem registro, mas relata do mesmo jeito
        std::fs::write(p.join("agentes.json"), r#"{"agentes": [{"id": "cofre", "agente": "Cofre"}, {"id": "correio", "agente": "Correio"}, {"id": "airbnb", "agente": "Jorge"}]}"#).unwrap();
        let (cofre, calado, jorge) = (|| "  saldo em dia. ".to_string(), String::new, || relato_jorge(&p));
        let r = reunir(&p, &[("Cofre", &cofre), ("Correio", &calado), ("Jorge", &jorge)]);
        assert_eq!(r.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["Cofre", "Jorge"]);
        assert!(para_fala(&r).starts_with("Passei pelos agentes antes de falar com você. Cofre reporta: saldo em dia. Jorge reporta: No Airbnb"));
        assert_eq!(para_fala(&[]), "");
        let h = crate::barramento::historico(&p, 50);
        assert_eq!(h.len(), 6); // um pedido e uma resposta por agente, mesmo de quem ficou calado
        assert!(!serde_json::to_string(&h).unwrap().contains("saldo em dia")); // o conteudo do relato nao vai ao barramento
        let _ = std::fs::remove_dir_all(&p);
    }
}
