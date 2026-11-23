//! Monza: corridas de carro pelo chat. Pergunta onde a pessoa esta e para onde vai e da o VALOR ESTIMADO pela distancia.
//! Porte de braniac/monza.py. Nunca pede dado de pagamento nem fecha a corrida: quando o preco sai, avisa o responsavel.
//! Ficou de fora: a consulta ao mapa (Nominatim + OSRM). A distancia entra como closure `km_de(origem, destino)`;
//! este modulo nao fala com a rede. O nome do dono saiu das frases ("o responsável").
#![allow(dead_code)]
use regex::Regex;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::OnceLock;

pub const PRECO_KM: f64 = 2.30;
const VALIDADE_S: f64 = 3.0 * 3600.0; // conversa parada ha mais de 3 h recomeca do zero

fn pedido() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)\b(corrida|uber|taxi|t[áa]xi|motorista|me\s+busca|vem\s+me\s+buscar|pode\s+me\s+levar|quanto\s+(?:custa|fica|[ée])\s+(?:uma?\s+)?(?:corrida|ir|at[ée]))\b").expect("regex fixa"))
}

fn de_para() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)\bde\s+(.+?)\s+(?:pra|para|at[ée]|ao|a)\s+(.+)$").expect("regex fixa"))
}

fn ler(dados: &Path) -> Value {
    std::fs::read_to_string(dados.join("monza.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}))
}

fn gravar(dados: &Path, d: &Value) -> Result<(), String> {
    std::fs::create_dir_all(dados).and_then(|_| std::fs::write(dados.join("monza.json"), d.to_string())).map_err(|e| format!("não gravei a conversa da corrida: {e}"))
}

pub fn preco(km: f64) -> f64 {
    (km * PRECO_KM * 100.0).round() / 100.0
}

fn brl(v: f64) -> String {
    format!("R$ {v:.2}").replace('.', ",")
}

/// Mensagem nova de corrida, ou conversa de corrida ainda em andamento com esse contato.
pub fn eh_pedido(dados: &Path, texto: &str, nome: &str, ts: f64) -> bool {
    pedido().is_match(texto) || ler(dados)[nome]["ts"].as_f64().map_or(false, |t| t > ts - VALIDADE_S)
}

/// Proxima fala do Monza. Estado por contato em dados/monza.json: origem -> destino -> valor.
/// `km_de` devolve a distancia de carro em km (None = sem rota); `avisar` leva o resumo ao responsavel.
pub fn responder(dados: &Path, nome: &str, texto: &str, ts: f64, km_de: &dyn Fn(&str, &str) -> Option<f64>, avisar: &dyn Fn(&str)) -> Result<String, String> {
    let mut est = ler(dados);
    let c = &est[nome];
    let vivo = c["ts"].as_f64().map_or(false, |t| t >= ts - VALIDADE_S);
    let campo = |k: &str| c[k].as_str().filter(|s| vivo && !s.is_empty()).map(String::from);
    let (mut origem, mut destino) = (campo("origem"), campo("destino"));
    let texto = texto.trim();
    if origem.is_none() {
        if let Some(m) = de_para().captures(texto) {
            (origem, destino) = (Some(m[1].trim().to_string()), Some(m[2].trim().to_string()));
        } else if !(texto.is_empty() || (pedido().is_match(texto) && texto.chars().count() < 60)) {
            origem = Some(texto.to_string()); // frase curta que so pede a corrida nao e endereco
        }
    } else if destino.is_none() && !texto.is_empty() {
        destino = Some(texto.to_string());
    }
    let (Some(o), Some(d)) = (origem.clone(), destino) else {
        est[nome] = json!({ "origem": origem, "destino": Value::Null, "ts": ts });
        gravar(dados, &est)?;
        return Ok(if origem.is_none() { "Oi! Aqui é o Monza, assistente de corridas. Me diz onde você está agora (rua, bairro ou ponto de referência)?" } else { "Beleza! E pra onde você quer ir?" }.into());
    };
    if let Some(m) = est.as_object_mut() {
        m.remove(nome); // conversa fechada
    }
    gravar(dados, &est)?;
    let Some(km) = km_de(&o, &d).filter(|k| k.is_finite() && *k >= 0.0) else {
        return Ok("Não consegui achar esse trajeto no mapa. Pode me mandar o endereço com rua e bairro (ou um ponto de referência)? Vou refazer o cálculo.".into());
    };
    let v = brl(preco(km));
    avisar(&format!("Monza: {nome} pediu corrida de «{o}» até «{d}», {km:.1} km, valor médio {v}."));
    Ok(format!("Saindo de {o} até {d} são cerca de {km:.1} km, então o valor médio da corrida fica em {v} ({} por km). É uma estimativa: já avisei o responsável e ele confirma com você a corrida.", brl(PRECO_KM)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn pasta(n: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("braniac-monza-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    #[test]
    fn conversa_completa_e_preco() {
        assert_eq!((preco(10.0), brl(11.5)), (23.0, "R$ 11,50".to_string()));
        let p = pasta("conversa");
        let avisos = RefCell::new(Vec::new());
        let av = |t: &str| avisos.borrow_mut().push(t.to_string());
        let km = |_: &str, _: &str| Some(10.0);
        assert!(eh_pedido(&p, "quero uma corrida", "", 1000.0) && !eh_pedido(&p, "Rua A", "Ana", 1000.0));
        assert!(responder(&p, "Ana", "quero uma corrida", 1000.0, &km, &av).unwrap().contains("onde você está"));
        assert!(eh_pedido(&p, "Rua A", "Ana", 1010.0)); // conversa em andamento
        assert!(responder(&p, "Ana", "Rua A", 1010.0, &km, &av).unwrap().contains("pra onde"));
        assert!(avisos.borrow().is_empty());
        let r = responder(&p, "Ana", "Rodoviária", 1020.0, &km, &av).unwrap();
        assert!(r.contains("R$ 23,00") && r.contains("10.0 km") && r.contains("Saindo de Rua A até Rodoviária") && r.contains("R$ 2,30 por km"), "{r}");
        assert!(avisos.borrow().len() == 1 && avisos.borrow()[0].contains("«Rua A» até «Rodoviária»"));
        assert!(!eh_pedido(&p, "oi", "Ana", 1030.0)); // conversa fechada
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn de_para_direto_sem_rota_e_conversa_velha_recomeca() {
        let p = pasta("depara");
        let nada = |_: &str| {};
        assert!(responder(&p, "Bia", "corrida de Rua Um para Rua Dois", 0.0, &|a, b| (a == "Rua Um" && b == "Rua Dois").then_some(5.0), &nada).unwrap().contains("R$ 11,50"));
        let avisou = RefCell::new(false);
        assert!(responder(&p, "Bia", "de Rua Um para Lugar Nenhum", 0.0, &|_, _| None, &|_| *avisou.borrow_mut() = true).unwrap().contains("Não consegui achar"));
        assert!(!*avisou.borrow()); // sem preco nao ha aviso
        responder(&p, "Caio", "Rua C", 0.0, &|_, _| Some(1.0), &nada).unwrap();
        // mais de 3 h depois a origem antiga nao vale: recomeca perguntando onde esta
        assert!(responder(&p, "Caio", "taxi", 4.0 * 3600.0, &|_, _| Some(1.0), &nada).unwrap().contains("onde você está"));
        let _ = std::fs::remove_dir_all(&p);
    }
}
