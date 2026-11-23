//! Quantix, a empresa de investimentos: SO SIMULACAO (papel). Arquivos em dados/empresa_inv/ (equipe.json, carteira.json, propostas.json), como no Python.
//! TRAVA PRINCIPAL: nao existe caminho de ordem real neste modulo. Nenhuma funcao recebe corretora, conta ou closure de ordem: deposito e virtual,
//! `aprovar` so muda numeros em carteira.json e o unico estado de execucao e "executada_papel". Nada aqui paga, transfere ou pede dado bancario.
//! So compra (stop abaixo da entrada), sem alavancagem. Cada proposta passa por Sigma (tamanho) -> Vigil (risco) -> Guarda (disciplina) e so
//! vira posicao em papel com o OK do dono, reavaliando os limites na hora.
//! Ficou de fora: a consulta de preco na internet (closure `preco` no `tick`). Perfil sem limites completos no equipe.json e vetado (no Python dava erro).
#![allow(dead_code)]
use crate::empresa_dev::{corta, gravar, ler, lista, rx};
use crate::tempo::Agora;
use regex::Regex;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const PERFIS: [&str; 3] = ["daytrade", "swing", "holding"];
pub const AVISO: &str = "Simulação, não é consultoria financeira, sem garantia de retorno.";
pub const MODO: &str = "simulacao";

fn pasta(dados: &Path) -> PathBuf {
    dados.join("empresa_inv")
}

fn n(v: &Value, k: &str) -> f64 {
    v[k].as_f64().unwrap_or(0.0)
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}

fn centavos(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

pub fn ticker_ok(t: &str) -> bool {
    static R: OnceLock<Regex> = OnceLock::new();
    rx(&R, r"^[A-Z0-9.\-]{1,10}$").is_match(t)
}

pub fn config(dados: &Path) -> Value {
    ler(&pasta(dados).join("equipe.json")).filter(Value::is_object).unwrap_or_else(|| json!({ "empresa": "Quantix", "agentes": [], "perfis": {} }))
}

pub fn nomes(dados: &Path) -> Vec<String> {
    config(dados)["agentes"].as_array().into_iter().flatten().map(|a| s(a, "nome").to_string()).collect()
}

/// Carteira de papel, com os tres perfis sempre presentes.
pub fn carteira(dados: &Path) -> Value {
    let mut c = ler(&pasta(dados).join("carteira.json")).filter(Value::is_object).unwrap_or_else(|| json!({}));
    for p in PERFIS {
        if !c[p].is_object() {
            c[p] = json!({ "capital": 0.0, "caixa": 0.0, "posicoes": [], "fechadas": [] });
        }
    }
    c
}

fn gravar_carteira(dados: &Path, c: &Value) -> Result<(), String> {
    gravar(&pasta(dados).join("carteira.json"), c)
}

fn propostas(dados: &Path) -> Vec<Value> {
    lista(&pasta(dados).join("propostas.json"))
}

/// Deposito VIRTUAL no perfil. Dinheiro real nunca passa por aqui.
pub fn depositar(dados: &Path, perfil: &str, valor: f64) -> String {
    if !PERFIS.contains(&perfil) || !(valor > 0.0) || !valor.is_finite() {
        return "Perfil inválido ou valor <= 0.".into();
    }
    let mut c = carteira(dados);
    c[perfil]["capital"] = json!(n(&c[perfil], "capital") + valor);
    c[perfil]["caixa"] = json!(n(&c[perfil], "caixa") + valor);
    match gravar_carteira(dados, &c) {
        Ok(()) => format!("Depósito virtual de {valor:.2} no perfil {perfil}. {AVISO}"),
        Err(e) => e,
    }
}

fn fechadas(cp: &Value) -> impl Iterator<Item = &Value> {
    cp["fechadas"].as_array().into_iter().flatten()
}

/// Sigma dimensiona; Vigil e Guarda vetam. Devolve (quantidade, motivos de veto). Pura: a carteira e os limites chegam por parametro.
pub fn avaliar(cfg: &Value, cart: &Value, perfil: &str, ticker: &str, entrada: f64, stop: f64, alvo: f64, agora: &Agora, ts: f64) -> (i64, Vec<String>) {
    let pc = &cfg["perfis"][perfil];
    if !PERFIS.contains(&perfil) || !pc.is_object() {
        return (0, vec!["perfil desconhecido".into()]);
    }
    if ["risco_trade", "exposicao", "breaker"].iter().any(|k| !pc[*k].is_number()) {
        return (0, vec!["Guarda: perfil sem limites completos (risco_trade, exposicao, breaker)".into()]);
    }
    let cp = &cart[perfil];
    if !ticker_ok(ticker) || !(entrada > 0.0 && stop > 0.0 && stop < entrada) || !entrada.is_finite() {
        return (0, vec!["Guarda: ticker inválido, ou não é compra com stop abaixo da entrada".into()]);
    }
    let capital = n(cp, "capital");
    if capital <= 0.0 {
        return (0, vec!["Vigil: perfil sem capital (faça um depósito virtual)".into()]);
    }
    let risco_un = entrada - stop;
    let qtd = (capital * n(pc, "risco_trade") / 100.0 / risco_un).min(capital * n(pc, "exposicao") / 100.0 / entrada).max(0.0) as i64;
    let mut vetos = vec![];
    if qtd < 1 {
        vetos.push("Sigma: capital pequeno demais para 1 unidade dentro do limite".to_string());
    }
    if qtd as f64 * entrada > n(cp, "caixa") {
        vetos.push("Vigil: caixa insuficiente".into());
    }
    let (rr, rr_min) = (if alvo != 0.0 { (alvo - entrada) / risco_un } else { 0.0 }, n(pc, "rr_min"));
    if rr_min != 0.0 && rr < rr_min {
        vetos.push(format!("Guarda: retorno/risco {rr:.2} abaixo de {rr_min}"));
    }
    let hoje = agora.data.iso();
    let pnl_dia: f64 = fechadas(cp).filter(|f| s(f, "fechada_em").starts_with(&hoje)).map(|f| n(f, "pnl")).sum();
    if n(pc, "perda_dia") != 0.0 && -pnl_dia >= capital * n(pc, "perda_dia") / 100.0 {
        vetos.push("Vigil: perda máxima do dia atingida".into());
    }
    let janela = s(pc, "breaker_janela");
    let dias = match janela { "semana" => 7.0, "trimestre" => 90.0, _ => 30.0 };
    let perdas: f64 = fechadas(cp).filter(|f| n(f, "ts_fechada") >= ts - dias * 86400.0).map(|f| n(f, "pnl")).sum();
    let dd = (-perdas).max(0.0) / capital * 100.0;
    if dd >= n(pc, "breaker") {
        vetos.push(format!("Vigil: circuit breaker (perda de {dd:.1}% na janela de {janela})"));
    }
    (qtd, vetos)
}

/// Abre uma proposta; so vira posicao (em papel) com `aprovar`.
pub fn propor(dados: &Path, perfil: &str, ticker: &str, entrada: f64, stop: f64, alvo: f64, tese: &str, agora: &Agora, ts: f64) -> String {
    let ticker = ticker.trim().to_uppercase();
    let (qtd, vetos) = avaliar(&config(dados), &carteira(dados), perfil, &ticker, entrada, stop, alvo, agora, ts);
    let mut ps = propostas(dados);
    let (id, risco_max) = (format!("Q{:03}", ps.len() + 1), if qtd > 0 { centavos(qtd as f64 * (entrada - stop)) } else { 0.0 });
    ps.push(json!({ "id": id, "perfil": perfil, "ticker": ticker, "entrada": entrada, "stop": stop, "alvo": alvo, "qtd": qtd, "risco_max": risco_max, "tese": corta(tese, 300),
        "estado": if vetos.is_empty() { "aguardando_ok" } else { "vetada" }, "vetos": vetos, "criada": ts }));
    if let Err(e) = gravar(&pasta(dados).join("propostas.json"), &json!(ps)) {
        return e;
    }
    if !vetos.is_empty() {
        return format!("{id} vetada: {}", vetos.join("; "));
    }
    format!("{id}: {perfil} compra {qtd} {ticker} a {entrada}, stop {stop}, alvo {alvo}, risco máx {risco_max}. Aguardando o OK do dono. {AVISO}")
}

/// OK do dono: executa em PAPEL, reavaliando os limites na hora. Ordem real nao existe aqui.
pub fn aprovar(dados: &Path, id: &str, pedido_do_dono: bool, agora: &Agora, ts: f64) -> String {
    if !pedido_do_dono {
        return "Só o dono dá o OK de uma proposta.".into();
    }
    let mut ps = propostas(dados);
    let Some(p) = ps.iter_mut().find(|x| s(x, "id") == id.trim().to_uppercase() && x["estado"] == "aguardando_ok") else { return "Proposta não encontrada ou não está aguardando OK.".into() };
    let mut cart = carteira(dados);
    let (pid, perfil, ticker, entrada) = (s(p, "id").to_string(), s(p, "perfil").to_string(), s(p, "ticker").to_string(), n(p, "entrada"));
    let (qtd, vetos) = avaliar(&config(dados), &cart, &perfil, &ticker, entrada, n(p, "stop"), n(p, "alvo"), agora, ts);
    let arq = pasta(dados).join("propostas.json");
    if !vetos.is_empty() {
        p["estado"] = json!("vetada");
        p["vetos"] = json!(vetos);
        return gravar(&arq, &json!(ps)).err().unwrap_or_else(|| format!("{pid} vetada na reavaliação: {}", vetos.join("; ")));
    }
    let cp = &mut cart[&perfil];
    cp["caixa"] = json!(n(cp, "caixa") - qtd as f64 * entrada);
    let pos = json!({ "id": pid, "ticker": ticker, "qtd": qtd, "entrada": entrada, "stop": p["stop"], "alvo": p["alvo"], "aberta_em": format!("{}T{:02}:{:02}:{:02}", agora.data.iso(), agora.hora, agora.min, agora.seg) });
    match cp["posicoes"].as_array_mut() {
        Some(v) => v.push(pos),
        None => cp["posicoes"] = json!([pos]),
    }
    p["estado"] = json!("executada_papel");
    if let Err(e) = gravar_carteira(dados, &cart).and_then(|_| gravar(&arq, &json!(ps))) {
        return e;
    }
    format!("{pid} executada em PAPEL: {qtd} {ticker} a {entrada}. {AVISO}")
}

/// Encerra a posicao de papel (stop, alvo ou decisao) e registra no diario.
pub fn fechar(dados: &Path, id: &str, preco_saida: f64, agora: &Agora, ts: f64) -> String {
    let mut cart = carteira(dados);
    let id = id.trim().to_uppercase();
    for perfil in PERFIS {
        let cp = &mut cart[perfil];
        let Some(i) = cp["posicoes"].as_array().and_then(|v| v.iter().position(|x| s(x, "id") == id)) else { continue };
        let mut pos = cp["posicoes"].as_array_mut().map(|v| v.remove(i)).unwrap_or_default();
        let (qtd, ticker) = (n(&pos, "qtd"), s(&pos, "ticker").to_string());
        let pnl = centavos((preco_saida - n(&pos, "entrada")) * qtd);
        cp["caixa"] = json!(n(cp, "caixa") + qtd * preco_saida);
        cp["capital"] = json!(n(cp, "capital") + pnl);
        pos["saida"] = json!(preco_saida);
        pos["pnl"] = json!(pnl);
        pos["fechada_em"] = json!(format!("{}T{:02}:{:02}:{:02}", agora.data.iso(), agora.hora, agora.min, agora.seg));
        pos["ts_fechada"] = json!(ts);
        match cp["fechadas"].as_array_mut() {
            Some(v) => v.push(pos),
            None => cp["fechadas"] = json!([pos]),
        }
        return gravar_carteira(dados, &cart).err().unwrap_or_else(|| format!("Cronos: {id} {ticker} fechada a {preco_saida}, resultado {pnl:+.2}."));
    }
    "Posição não encontrada.".into()
}

/// Vigil: fecha em papel as posicoes que tocaram stop ou alvo. Sem posicao aberta nao consulta preco nenhum.
pub fn tick(dados: &Path, agora: &Agora, ts: f64, preco: &dyn Fn(&str) -> Option<f64>, enviar: &dyn Fn(&str)) -> Vec<String> {
    let cart = carteira(dados);
    let mut saidas = vec![];
    for pos in PERFIS.iter().flat_map(|p| cart[*p]["posicoes"].as_array().into_iter().flatten()) {
        let Some(px) = Some(s(pos, "ticker")).filter(|t| ticker_ok(t)).and_then(preco) else { continue };
        if px <= n(pos, "stop") || (n(pos, "alvo") != 0.0 && px >= n(pos, "alvo")) {
            saidas.push(fechar(dados, s(pos, "id"), px, agora, ts));
        }
    }
    if !saidas.is_empty() {
        enviar(&saidas.join("\n"));
    }
    saidas
}

pub fn situacao(dados: &Path) -> Value {
    let c = carteira(dados);
    let perfis: serde_json::Map<String, Value> = PERFIS.iter().map(|p| (p.to_string(), json!({ "capital": centavos(n(&c[*p], "capital")),
        "posicoes": c[*p]["posicoes"].as_array().map_or(0, Vec::len), "fechadas": c[*p]["fechadas"].as_array().map_or(0, Vec::len) }))).collect();
    json!({ "empresa": config(dados)["empresa"], "agentes": nomes(dados).len(), "modo": MODO, "perfis": perfis,
        "propostas_aguardando": propostas(dados).iter().filter(|x| x["estado"] == "aguardando_ok").count() })
}

pub fn quadro(dados: &Path) -> String {
    let sit = situacao(dados);
    let perfis: Vec<String> = PERFIS.iter().map(|p| format!("{p}: capital {}, {} aberta(s)", sit["perfis"][*p]["capital"], sit["perfis"][*p]["posicoes"])).collect();
    format!("Quantix ({MODO}): {}; {} proposta(s) aguardando OK. {AVISO}", perfis.join("; "), sit["propostas_aguardando"])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;
    use std::cell::RefCell;

    fn dir(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-empresa_inv-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        gravar(&d.join("empresa_inv/equipe.json"), &json!({ "empresa": "Quantix", "agentes": [{ "nome": "Sigma" }, { "nome": "Vigil" }], "perfis": {
            "daytrade": { "risco_trade": 0.5, "exposicao": 20, "rr_min": 2, "perda_dia": 1.5, "breaker": 3, "breaker_janela": "semana" },
            "swing": { "risco_trade": 0.5, "exposicao": 15, "rr_min": 2, "breaker": 8, "breaker_janela": "mes" } } })).unwrap();
        d
    }

    fn agora() -> Agora {
        Agora { data: Data::civil(2026, 11, 3).unwrap(), hora: 10, min: 5, seg: 0 }
    }

    #[test]
    fn fluxo_em_papel_e_vetos() {
        let (d, a) = (dir("fluxo"), agora());
        assert!(propor(&d, "swing", "AAPL", 100.0, 95.0, 112.0, "", &a, 1.0).contains("perfil sem capital"));
        assert!(depositar(&d, "swing", -5.0).contains("inválido") && depositar(&d, "poupanca", 5.0).contains("inválido") && depositar(&d, "swing", f64::NAN).contains("inválido"));
        assert!(depositar(&d, "swing", 10000.0).contains("Depósito virtual de 10000.00"));
        assert!(propor(&d, "swing", "AAPL", 100.0, 105.0, 112.0, "", &a, 1.0).contains("stop abaixo da entrada")); // venda a descoberto nao existe
        assert!(propor(&d, "swing", "AAPL", 100.0, 95.0, 102.0, "", &a, 1.0).contains("retorno/risco 0.40 abaixo de 2"));
        assert!(propor(&d, "swing", "aapl; rm", 100.0, 95.0, 112.0, "", &a, 1.0).contains("ticker inválido"));
        assert!(propor(&d, "holding", "AAPL", 100.0, 95.0, 112.0, "", &a, 1.0).contains("perfil desconhecido")); // sem limites no equipe.json
        let r = propor(&d, "swing", " aapl ", 100.0, 95.0, 112.0, "rompimento", &a, 1.0);
        assert!(r.starts_with("Q006: swing compra 10 AAPL a 100, stop 95, alvo 112, risco máx 50") && r.contains("Aguardando o OK") && r.ends_with(AVISO), "{r}");
        assert!(aprovar(&d, "Q006", false, &a, 1.0).contains("Só o dono"));
        assert!(carteira(&d)["swing"]["posicoes"].as_array().unwrap().is_empty());
        assert!(aprovar(&d, "Q001", true, &a, 1.0).contains("não está aguardando OK")); // vetada nao executa
        assert!(aprovar(&d, "q006", true, &a, 1.0).contains("executada em PAPEL: 10 AAPL"));
        assert!(aprovar(&d, "Q006", true, &a, 1.0).contains("não está aguardando OK")); // nao executa duas vezes
        let c = carteira(&d);
        assert_eq!((c["swing"]["posicoes"][0]["qtd"].as_i64(), c["swing"]["caixa"].as_f64(), c["swing"]["capital"].as_f64()), (Some(10), Some(9000.0), Some(10000.0)));
        assert!(quadro(&d).starts_with("Quantix (simulacao): ") && quadro(&d).contains("swing: capital 10000.0, 1 aberta(s)"));
        assert_eq!(fechar(&d, "Q006", 110.0, &a, 2.0), "Cronos: Q006 AAPL fechada a 110, resultado +100.00.");
        let c = carteira(&d);
        assert_eq!((c["swing"]["capital"].as_f64(), c["swing"]["caixa"].as_f64(), c["swing"]["fechadas"][0]["fechada_em"].as_str()), (Some(10100.0), Some(10100.0), Some("2026-11-03T10:05:00")));
        assert_eq!(fechar(&d, "Q006", 110.0, &a, 2.0), "Posição não encontrada.");
    }

    #[test]
    fn so_simulacao_nunca_dinheiro_real() {
        let (d, a) = (dir("papel"), agora());
        depositar(&d, "swing", 10000.0);
        propor(&d, "swing", "AAPL", 100.0, 95.0, 112.0, "", &a, 1.0);
        propor(&d, "swing", "AAPL", 100.0, 99.0, 100.5, "", &a, 1.0);
        aprovar(&d, "Q001", true, &a, 1.0);
        // os unicos estados que existem: vetada, aguardando_ok e executada em PAPEL; a situacao sempre diz "simulacao"
        let estados: Vec<String> = propostas(&d).iter().map(|p| p["estado"].as_str().unwrap().to_string()).collect();
        assert_eq!(estados, ["executada_papel", "vetada"]);
        assert_eq!(situacao(&d)["modo"], "simulacao");
        // o modulo so escreve os proprios arquivos de papel
        let mut arqs: Vec<String> = std::fs::read_dir(d.join("empresa_inv")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        arqs.sort();
        assert_eq!(arqs, ["carteira.json", "equipe.json", "propostas.json"]);
        // no codigo-fonte nao ha rede, processo externo nem palavra de ordem real (a consulta de preco e closure de quem chama)
        let fonte = include_str!("empresa_inv.rs").split("#[cfg(test)]").next().unwrap();
        for proibido in ["ureq", "tungstenite", "Command", "corretora:", "ordem_real", "transferir", "pagar("] {
            assert!(!fonte.contains(proibido), "{proibido}");
        }
        for texto in [depositar(&d, "swing", 1.0), propor(&d, "swing", "AAPL", 100.0, 95.0, 112.0, "", &a, 1.0), quadro(&d)] {
            assert!(texto.contains("Simulação"), "{texto}");
        }
    }

    #[test]
    fn circuit_breaker_perda_do_dia_e_reavaliacao_no_ok() {
        let (d, a) = (dir("breaker"), agora());
        depositar(&d, "daytrade", 10000.0);
        assert!(propor(&d, "daytrade", "SPY", 100.0, 99.0, 103.0, "", &a, 1000.0).contains("Aguardando"));
        let mut c = carteira(&d);
        c["daytrade"]["fechadas"] = json!([{ "pnl": -400, "fechada_em": "2026-11-03T09:00:00", "ts_fechada": 900.0 }]);
        gravar_carteira(&d, &c).unwrap();
        let r = propor(&d, "daytrade", "SPY", 100.0, 99.0, 103.0, "", &a, 1000.0);
        assert!(r.contains("circuit breaker (perda de 4.0% na janela de semana)") && r.contains("perda máxima do dia atingida"), "{r}");
        let r = aprovar(&d, "Q001", true, &a, 1000.0); // estava ok quando foi proposta; o limite e conferido de novo no OK
        assert!(r.contains("vetada na reavaliação") && carteira(&d)["daytrade"]["posicoes"].as_array().unwrap().is_empty(), "{r}");
        assert!(propor(&d, "daytrade", "SPY", 100.0, 99.0, 103.0, "", &a, 1000.0 + 8.0 * 86400.0).contains("perda máxima do dia")); // saiu da janela, mas o dia ainda conta
        let amanha = Agora { data: Data::civil(2026, 11, 12).unwrap(), ..a };
        assert!(propor(&d, "daytrade", "SPY", 100.0, 99.0, 103.0, "", &amanha, 1000.0 + 8.0 * 86400.0).contains("Aguardando"));
        // capital pequeno e caixa
        let (d2, _) = (dir("pequeno"), ());
        depositar(&d2, "swing", 100.0);
        assert!(propor(&d2, "swing", "AAPL", 100.0, 95.0, 112.0, "", &a, 1.0).contains("Sigma: capital pequeno demais"));
    }

    #[test]
    fn tick_fecha_no_stop_ou_alvo_e_sem_posicao_nao_consulta_preco() {
        let (d, a) = (dir("tick"), agora());
        let consultas = RefCell::new(vec![]);
        let avisos = RefCell::new(vec![]);
        let preco = |t: &str| -> Option<f64> {
            consultas.borrow_mut().push(t.to_string());
            if t == "AAPL" { Some(94.0) } else { None }
        };
        let enviar = |m: &str| avisos.borrow_mut().push(m.to_string());
        assert!(tick(&d, &a, 1.0, &preco, &enviar).is_empty() && consultas.borrow().is_empty());
        depositar(&d, "swing", 20000.0);
        propor(&d, "swing", "AAPL", 100.0, 95.0, 112.0, "", &a, 1.0);
        propor(&d, "swing", "MSFT", 100.0, 95.0, 112.0, "", &a, 1.0);
        aprovar(&d, "Q001", true, &a, 1.0);
        aprovar(&d, "Q002", true, &a, 1.0);
        let r = tick(&d, &a, 2.0, &preco, &enviar);
        assert_eq!(r, ["Cronos: Q001 AAPL fechada a 94, resultado -120.00."]); // sem preco (MSFT) a posicao fica
        assert_eq!((avisos.borrow().len(), carteira(&d)["swing"]["posicoes"].as_array().unwrap().len(), carteira(&d)["swing"]["capital"].as_f64()), (1, 1, Some(19880.0)));
    }
}
