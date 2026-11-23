//! TAXA DO SE PIQUE: confirma a "ajuda de custo" do app de taximetro. O app informa {aparelho, nome, valor_centavos}; o Cofre (so leitura
//! do extrato) procura um Pix RECEBIDO com esse nome e esse valor e marca a solicitacao como confirmada. Nunca paga, transfere nem mexe na conta.
//! Travas: pedido validado campo a campo (nome so com letras, valor de R$ 10 a R$ 5.000); ate 3 pendentes por aparelho e 20 novas em 10 min;
//! cada Pix serve uma unica vez; so Pix do dia anterior ao pedido em diante; o extrato e lido no maximo a cada 3 min e so com pendente;
//! sem Pix em 72 h vira "nao_encontrada". Dados em dados/taxa_sepique.json (nomes de quem pagou ficam so no PC).
//! Ficou de fora: o servidor HTTP das 3 rotas, o tunel e a publicacao do endereco; a leitura do extrato (`ler`), o aviso ao dono (`avisar`)
//! e o sorteio do id (`id_novo`, que tem de ser imprevisivel) entram por parametro.
#![allow(dead_code)]
use crate::empresa_dev::{gravar, ler as ler_json, rx};
use crate::tempo::{Agora, Data};
use crate::texto_pt::sem_acento;
use regex::Regex;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const MIN_CENTAVOS: i64 = 1000;
pub const MAX_CENTAVOS: i64 = 500000;
pub const EXPIRA_S: f64 = 72.0 * 3600.0;
pub const GUARDA_S: f64 = 30.0 * 86400.0;
pub const INTERVALO_LEITURA_S: f64 = 180.0;
pub const MAX_PENDENTES_APARELHO: usize = 3;
pub const MAX_NOVAS_10MIN: usize = 20;
const PARTICULAS: [&str; 6] = ["de", "da", "do", "das", "dos", "e"];

fn arq(dados: &Path) -> PathBuf {
    dados.join("taxa_sepique.json")
}

fn estado(dados: &Path) -> Value {
    ler_json(&arq(dados)).filter(Value::is_object).unwrap_or_else(|| json!({}))
}

fn palavras(s: &str) -> Vec<String> {
    sem_acento(s).split(|c: char| !c.is_ascii_lowercase()).filter(|t| !t.is_empty()).map(String::from).collect()
}

fn partes_do_nome(nome: &str) -> Vec<String> {
    palavras(nome).into_iter().filter(|t| t.len() >= 2 && !PARTICULAS.contains(&t.as_str())).collect()
}

/// Todas as partes do nome informado (sem "de/da/do...") aparecem na descricao do Pix; exige 2 ou mais partes.
pub fn nome_confere(nome: &str, desc: &str) -> bool {
    let (partes, achadas) = (partes_do_nome(nome), palavras(desc));
    partes.len() >= 2 && partes.iter().all(|p| achadas.contains(p))
}

pub fn chave_mov(m: &Value) -> String {
    let t = |k: &str| m[k].as_str().unwrap_or("");
    format!("{}|{}|{}|{}", t("data"), t("hora"), t("desc"), m["valor"])
}

/// Casa solicitacoes pendentes com Pix RECEBIDOS: mesmo valor (centavos), nome confere, data >= dia anterior ao pedido, cada Pix serve
/// uma vez. `dia_min(criado)` da a data "AAAA-MM-DD" do dia anterior ao pedido. Muda `claims`/`consumidos` e devolve os ids confirmados.
pub fn conferir(claims: &mut Value, movs: &[Value], consumidos: &mut Vec<String>, ts: f64, dia_min: &dyn Fn(f64) -> String) -> Vec<String> {
    let entradas: Vec<&Value> = movs.iter().filter(|m| m["valor"].as_f64().unwrap_or(0.0) > 0.0 && sem_acento(m["desc"].as_str().unwrap_or("")).starts_with("pix")).collect();
    let Some(mapa) = claims.as_object_mut() else { return vec![] };
    let mut pendentes: Vec<&mut Value> = mapa.values_mut().filter(|c| c["estado"] == "pendente").collect();
    pendentes.sort_by(|a, b| a["criado"].as_f64().unwrap_or(0.0).total_cmp(&b["criado"].as_f64().unwrap_or(0.0)));
    let mut ok = vec![];
    for c in pendentes {
        let minimo = dia_min(c["criado"].as_f64().unwrap_or(0.0));
        let achado = entradas.iter().map(|m| (chave_mov(m), m)).find(|(k, m)| {
            !consumidos.contains(k) && m["data"].as_str().unwrap_or("") >= minimo.as_str() && (m["valor"].as_f64().unwrap_or(0.0) * 100.0).round() as i64 == c["centavos"].as_i64().unwrap_or(-1) && nome_confere(c["nome"].as_str().unwrap_or(""), m["desc"].as_str().unwrap_or(""))
        });
        if let Some((k, _)) = achado {
            c["estado"] = json!("confirmada");
            c["confirmada_em"] = json!(ts);
            c["movimento"] = json!(k);
            consumidos.push(k);
            ok.extend(c["id"].as_str().map(String::from));
        }
    }
    ok
}

/// (aparelho, nome, centavos) ou a mensagem curta do erro.
pub fn validar(corpo: &Value) -> Result<(String, String, i64), String> {
    static AP: OnceLock<Regex> = OnceLock::new();
    static NOME: OnceLock<Regex> = OnceLock::new();
    if !corpo.is_object() {
        return Err("pedido inválido".into());
    }
    let aparelho = corpo["aparelho"].as_str().filter(|a| rx(&AP, r"^[0-9a-fA-F-]{8,64}$").is_match(a)).ok_or("aparelho inválido")?;
    let nome = corpo["nome"].as_str().map(str::trim).filter(|n| (5..=80).contains(&n.chars().count()) && rx(&NOME, r"^[A-Za-zÀ-ÿ'.\- ]+$").is_match(n) && partes_do_nome(n).len() >= 2).ok_or("nome inválido")?;
    // so inteiro de verdade: 1234.0, "1234" e true nao valem
    let cent = corpo["valor_centavos"].as_i64().filter(|c| (MIN_CENTAVOS..=MAX_CENTAVOS).contains(c)).ok_or("valor inválido (mínimo R$ 10,00)")?;
    Ok((aparelho.to_lowercase(), nome.split_whitespace().collect::<Vec<_>>().join(" "), cent))
}

fn limpar(est: &mut Value, ts: f64) {
    let mut claims = est["claims"].as_object().cloned().unwrap_or_default();
    claims.retain(|_, c| ts - c["criado"].as_f64().unwrap_or(0.0) < GUARDA_S);
    for c in claims.values_mut() {
        if c["estado"] == "pendente" && ts - c["criado"].as_f64().unwrap_or(0.0) > EXPIRA_S {
            c["estado"] = json!("nao_encontrada");
        }
    }
    est["claims"] = Value::Object(claims);
}

/// Registra o aviso de pagamento. Devolve (resposta, status HTTP). Repetir o mesmo aviso devolve o mesmo id.
pub fn reivindicar(dados: &Path, corpo: &Value, ts: f64, id_novo: &str) -> (Value, u16) {
    let (aparelho, nome, cent) = match validar(corpo) {
        Ok(x) => x,
        Err(e) => return (json!({"erro": e}), 400),
    };
    let mut est = estado(dados);
    limpar(&mut est, ts);
    let claims = est["claims"].as_object().cloned().unwrap_or_default();
    let pendente = |c: &&Value| c["estado"] == "pendente" && c["aparelho"] == aparelho.as_str();
    if let Some(c) = claims.values().filter(pendente).find(|c| c["nome"] == nome.as_str() && c["centavos"] == cent) {
        return (json!({"id": c["id"], "estado": "pendente"}), 200);
    }
    if claims.values().filter(pendente).count() >= MAX_PENDENTES_APARELHO || claims.values().filter(|c| ts - c["criado"].as_f64().unwrap_or(0.0) < 600.0).count() >= MAX_NOVAS_10MIN {
        return (json!({"erro": "muitas tentativas"}), 429);
    }
    if id_novo.is_empty() || claims.contains_key(id_novo) {
        return (json!({"erro": "tente de novo"}), 500);
    }
    est["claims"][id_novo] = json!({"id": id_novo, "aparelho": aparelho, "nome": nome, "centavos": cent, "criado": ts, "estado": "pendente"});
    match gravar(&arq(dados), &est) {
        Ok(()) => (json!({"id": id_novo, "estado": "pendente"}), 200),
        Err(_) => (json!({"erro": "tente de novo"}), 500),
    }
}

pub fn status(dados: &Path, id: &str) -> (Value, u16) {
    match estado(dados)["claims"].get(id).filter(|c| c.is_object()) {
        Some(c) => (json!({"estado": c["estado"], "valor_centavos": c["centavos"]}), 200),
        None => (json!({"erro": "não encontrado"}), 404),
    }
}

fn brl(cent: i64) -> String {
    crate::cofre::brl(cent as f64 / 100.0)
}

/// Se ha solicitacao pendente, le o extrato (no maximo a cada 3 min), confere e avisa o dono. Devolve quantas foram confirmadas.
/// Falha na leitura (sessao do banco caiu) nao confirma nada: tenta de novo no proximo ciclo.
pub fn ciclo(dados: &Path, agora: &Agora, ts: f64, ler: &dyn Fn() -> Result<Vec<Value>, String>, avisar: &dyn Fn(&str)) -> usize {
    let mut est = estado(dados);
    limpar(&mut est, ts);
    if !est["claims"].as_object().is_some_and(|c| c.values().any(|c| c["estado"] == "pendente")) {
        let _ = gravar(&arq(dados), &est);
        return 0;
    }
    if ts - est["ultima_leitura"].as_f64().unwrap_or(0.0) < INTERVALO_LEITURA_S {
        return 0;
    }
    est["ultima_leitura"] = json!(ts);
    if gravar(&arq(dados), &est).is_err() {
        return 0;
    }
    let Ok(movs) = ler() else { return 0 };
    let mut consumidos: Vec<String> = est["consumidos"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
    // data local de um carimbo, pela diferenca para agora (o Brasil nao tem horario de verao)
    let dia_min = |criado: f64| Data(agora.data.0 + ((criado - 86400.0 - (ts - agora.segundos() as f64)) / 86400.0).floor() as i64).iso();
    let feitos = conferir(&mut est["claims"], &movs, &mut consumidos, ts, &dia_min);
    let sobra = consumidos.len().saturating_sub(800);
    est["consumidos"] = json!(consumidos[sobra..]);
    if gravar(&arq(dados), &est).is_err() {
        return 0; // sem gravar nao avisa: senao o mesmo Pix seria confirmado de novo
    }
    for id in &feitos {
        let c = &est["claims"][id];
        avisar(&format!("💸 Taxa do Se Pique confirmada: {} de {}.", brl(c["centavos"].as_i64().unwrap_or(0)), c["nome"].as_str().unwrap_or("")));
    }
    feitos.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const DEV: &str = "3f020bc7-6a96-4d0b-800b-e86596be97d9";
    const TS: f64 = 1_000_000.0 * 60.0;

    fn tmp(n: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-taxa_sepique-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn agora() -> Agora {
        Agora { data: Data::civil(2026, 10, 10).unwrap(), hora: 15, min: 20, seg: 0 }
    }

    fn pedido(extra: Value) -> Value {
        let mut p = json!({"aparelho": DEV, "nome": "Maria da Silva", "valor_centavos": 1234});
        for (k, v) in extra.as_object().into_iter().flatten() {
            p[k] = v.clone();
        }
        p
    }

    fn pix(desc: &str, valor: f64, data: &str) -> Value {
        json!({"data": data, "desc": desc, "valor": valor, "hora": "10:00"})
    }

    #[test]
    fn nome_confere_ignora_acento_caixa_e_particulas() {
        assert!(nome_confere("Maria da Silva", "Pix MARIA SILVA") && nome_confere("José  Antônio", "Pix Jose Antonio Souza"));
        assert!(!nome_confere("Maria Silva", "Pix Maria Souza") && !nome_confere("Maria", "Pix Maria Silva")); // uma parte so nao basta
    }

    #[test]
    fn validar_recusa_pedido_ruim() {
        assert_eq!(validar(&pedido(json!({"nome": "  maria   da silva ", "aparelho": "ABCDEF12"}))), Ok(("abcdef12".into(), "maria da silva".into(), 1234)));
        for ruim in [json!({"valor_centavos": 999}), json!({"valor_centavos": true}), json!({"valor_centavos": "1234"}), json!({"valor_centavos": 1234.0}), json!({"nome": "Maria"}), json!({"nome": "Maria <script>"}),
                     json!({"aparelho": "x y"}), json!({"valor_centavos": 1_000_000_000}), json!({"nome": "M1ria da Silva"}), json!({"aparelho": null})] {
            assert!(validar(&pedido(ruim.clone())).is_err(), "{ruim}");
        }
        assert_eq!(validar(&json!("texto")), Err("pedido inválido".into()));
    }

    #[test]
    fn reivindicar_e_idempotente_tem_limites_e_status() {
        let d = tmp("reiv");
        let (r, code) = reivindicar(&d, &pedido(json!({})), TS, "id-a");
        assert_eq!((code, r), (200, json!({"id": "id-a", "estado": "pendente"})));
        assert_eq!(reivindicar(&d, &pedido(json!({})), TS + 5.0, "id-b").0["id"], "id-a");
        assert_eq!(status(&d, "id-a"), (json!({"estado": "pendente", "valor_centavos": 1234}), 200));
        assert_eq!(status(&d, "nao-existe").1, 404);
        assert_eq!(reivindicar(&d, &pedido(json!({"valor_centavos": 50})), TS, "id-c").1, 400);
        assert_eq!(reivindicar(&d, &pedido(json!({"valor_centavos": 1100})), TS, "id-a").1, 500); // id repetido nunca sobrescreve
        for (v, id) in [(1100, "id-d"), (1200, "id-e")] {
            assert_eq!(reivindicar(&d, &pedido(json!({"valor_centavos": v})), TS, id).1, 200);
        }
        assert_eq!(reivindicar(&d, &pedido(json!({"valor_centavos": 1300})), TS, "id-f"), (json!({"erro": "muitas tentativas"}), 429));
        // teto global: 20 novas em 10 minutos, de qualquer aparelho
        let g = tmp("teto");
        for i in 0..20 {
            assert_eq!(reivindicar(&g, &pedido(json!({"aparelho": format!("{i:08x}")})), TS, &format!("g{i}")).1, 200);
        }
        assert_eq!(reivindicar(&g, &pedido(json!({"aparelho": "ffffffff"})), TS + 599.0, "g20").1, 429);
        assert_eq!(reivindicar(&g, &pedido(json!({"aparelho": "ffffffff"})), TS + 601.0, "g21").1, 200);
    }

    #[test]
    fn ciclo_confirma_pix_com_mesmo_nome_e_valor_uma_unica_vez() {
        let d = tmp("ciclo");
        let avisos = RefCell::new(vec![]);
        let avisar = |t: &str| avisos.borrow_mut().push(t.to_string());
        reivindicar(&d, &pedido(json!({})), TS, "id-a");
        let movs = vec![pix("Pix Joao Pereira", 12.34, "2026-10-10"), pix("Pix Maria da Silva Souza", 12.34, "2026-10-09"), pix("Vendas", 12.34, "2026-10-10")];
        assert_eq!(ciclo(&d, &agora(), TS + 400.0, &|| Ok(movs.clone()), &avisar), 1);
        assert_eq!(status(&d, "id-a").0["estado"], "confirmada");
        assert_eq!(*avisos.borrow(), vec!["💸 Taxa do Se Pique confirmada: R$ 12,34 de Maria da Silva."]);
        // o mesmo Pix nao serve a uma segunda solicitacao
        reivindicar(&d, &pedido(json!({"aparelho": "bbbbbbbb"})), TS + 500.0, "id-b");
        assert_eq!(ciclo(&d, &agora(), TS + 900.0, &|| Ok(movs.clone()), &avisar), 0);
        assert_eq!((status(&d, "id-b").0["estado"].clone(), avisos.borrow().len()), (json!("pendente"), 1));
    }

    #[test]
    fn valor_diferente_saida_data_antiga_ou_falha_na_leitura_nao_confirmam() {
        let d = tmp("nao");
        reivindicar(&d, &pedido(json!({})), TS, "id-a");
        let movs = vec![pix("Pix Maria da Silva", 12.35, "2026-10-10"), pix("Pix Maria da Silva", -12.34, "2026-10-10"), pix("Pix Maria da Silva", 12.34, "2026-10-08"), pix("Transferencia Maria da Silva", 12.34, "2026-10-10")];
        let nunca = |_: &str| panic!("nada foi confirmado");
        assert_eq!(ciclo(&d, &agora(), TS + 400.0, &|| Ok(movs.clone()), &nunca), 0);
        assert_eq!(ciclo(&d, &agora(), TS + 800.0, &|| Err("sessao caiu".into()), &nunca), 0);
        assert_eq!(status(&d, "id-a").0["estado"], "pendente");
    }

    #[test]
    fn sem_pendente_nao_le_o_banco_leitura_tem_intervalo_e_expira_em_72h() {
        let d = tmp("intervalo");
        let lidas = RefCell::new(0);
        let ler = || {
            *lidas.borrow_mut() += 1;
            Ok(vec![])
        };
        assert_eq!((ciclo(&d, &agora(), TS, &ler, &|_| {}), *lidas.borrow()), (0, 0));
        reivindicar(&d, &pedido(json!({})), TS, "id-a");
        ciclo(&d, &agora(), TS + 10.0, &ler, &|_| {});
        ciclo(&d, &agora(), TS + 20.0, &ler, &|_| {}); // menos de 3 min: nao le de novo
        assert_eq!(*lidas.borrow(), 1);
        ciclo(&d, &agora(), TS + EXPIRA_S + 60.0, &ler, &|_| {});
        assert_eq!((status(&d, "id-a").0["estado"].clone(), *lidas.borrow()), (json!("nao_encontrada"), 1));
        ciclo(&d, &agora(), TS + GUARDA_S + 1.0, &ler, &|_| {});
        assert_eq!(status(&d, "id-a").1, 404); // solicitacao antiga some do arquivo
    }
}
