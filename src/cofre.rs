//! Cofre: a meta de saldo ate o 5o dia util do mes (feriados nacionais fixos + Sexta-feira Santa). Mesmas contas do braniac/financeiro.py.
//! O Rust NAO le banco nem faz login: o saldo e informado pela pessoa (POST /api/cofre) e fica so em dados/cofre.json.
//! Sem saldo informado, o Cofre diz que nao tem o saldo: nunca chuta. Nada daqui vai a IA online.
use crate::tempo::Data;
use serde_json::{json, Value};
use std::path::Path;

const JANELA_DIAS: i64 = 14;
const FERIADOS: [(i64, i64); 9] = [(1, 1), (4, 21), (5, 1), (9, 7), (10, 12), (11, 2), (11, 15), (11, 20), (12, 25)];
const SEXTA_SANTA: [(i64, i64, i64); 2] = [(2026, 4, 3), (2027, 3, 26)]; // shortcut: so 2026 e 2027, acrescentar a data de cada ano novo

pub fn dia_util(d: Data) -> bool {
    let (y, m, dia) = d.partes();
    d.semana() < 5 && !FERIADOS.contains(&(m, dia)) && !SEXTA_SANTA.contains(&(y, m, dia))
}

pub fn quinto_dia_util(ano: i64, mes: i64) -> Data {
    let mut d = Data::civil(ano, mes, 1).expect("dia 1 existe");
    let mut n = 0;
    loop {
        n += dia_util(d) as i32;
        if n == 5 {
            return d;
        }
        d = Data(d.0 + 1);
    }
}

/// 5o dia util do mes corrente; passou -> o do proximo mes.
pub fn prazo_atual(hoje: Data) -> Data {
    let (y, m, _) = hoje.partes();
    let p = quinto_dia_util(y, m);
    if hoje <= p { p } else { quinto_dia_util(y + (m == 12) as i64, m % 12 + 1) }
}

/// Dias uteis de amanha ate o prazo (inclusive).
pub fn dias_uteis_ate(hoje: Data, prazo: Data) -> i64 {
    (hoje.0 + 1..=prazo.0).filter(|&d| dia_util(Data(d))).count() as i64
}

/// (entra por dia, sai por dia, dias de historico) nos ultimos 14 dias.
pub fn ritmo(movs: &[(Data, f64)], hoje: Data) -> (f64, f64, i64) {
    let mv: Vec<&(Data, f64)> = movs.iter().filter(|m| m.0 .0 > hoje.0 - JANELA_DIAS).collect();
    let Some(primeiro) = mv.iter().map(|m| m.0 .0).min() else { return (0.0, 0.0, 0) };
    let dias = (hoje.0 - primeiro + 1).max(1);
    let soma = |positivo: bool| mv.iter().map(|m| m.1).filter(|v| (*v > 0.0) == positivo).sum::<f64>().abs() / dias as f64;
    (soma(true), soma(false), dias)
}

/// Situacao da meta. nivel: batida | ok | atencao | critico. Pura.
pub fn avaliar(saldo: f64, meta: f64, movs: &[(Data, f64)], hoje: Data) -> Value {
    let prazo = prazo_atual(hoje);
    let du = dias_uteis_ate(hoje, prazo);
    let (entra, sai, hist) = ritmo(movs, hoje);
    let previsto = saldo + (entra - sai) * (prazo.0 - hoje.0) as f64;
    let falta = (meta - saldo).max(0.0);
    let nivel = if saldo >= meta {
        "batida"
    } else if previsto >= meta && du > 2 {
        "ok"
    } else if du <= 3 || previsto < meta * 0.5 {
        "critico"
    } else if previsto < meta {
        "atencao"
    } else {
        "ok"
    };
    json!({ "saldo": saldo, "meta": meta, "falta": falta, "prazo": prazo.iso(), "dias_uteis": du, "entra_dia": entra, "sai_dia": sai,
            "historico_dias": hist, "previsto": previsto, "nivel": nivel, "precisa_por_dia_util": if du > 0 { falta / du as f64 } else { falta } })
}

pub fn brl(v: f64) -> String {
    let c = (v.abs() * 100.0).round() as i64;
    let inteiro = (c / 100).to_string();
    let mut s = String::new();
    for (i, ch) in inteiro.chars().enumerate() {
        if i > 0 && (inteiro.len() - i) % 3 == 0 {
            s.push('.');
        }
        s.push(ch);
    }
    format!("{}R$ {s},{:02}", if v < 0.0 { "-" } else { "" }, c % 100)
}

pub fn texto(av: &Value) -> String {
    let f = |k: &str| av[k].as_f64().unwrap_or(0.0);
    let prazo = Data::de_iso(av["prazo"].as_str().unwrap_or("")).map(|d| d.dia_mes()).unwrap_or_default();
    if av["nivel"] == "batida" {
        return format!("✅ Meta batida: saldo {} e a meta de {} vale até {prazo}.", brl(f("saldo")), brl(f("meta")));
    }
    let mut base = format!("saldo {}, faltam {} para {} até o 5º dia útil ({prazo}, {} dia(s) útil(eis)). Precisa entrar uns {} por dia útil.",
        brl(f("saldo")), brl(f("falta")), brl(f("meta")), av["dias_uteis"], brl(f("precisa_por_dia_util")));
    if av["historico_dias"].as_i64().unwrap_or(0) >= 5 {
        base += &format!(" Pelo ritmo dos últimos {} dias (entra {}/dia, sai {}/dia) a previsão é fechar em {}.", av["historico_dias"], brl(f("entra_dia")), brl(f("sai_dia")), brl(f("previsto")));
    } else {
        base += " Ainda tenho pouco histórico para prever bem.";
    }
    format!("{}{base}", match av["nivel"].as_str() { Some("critico") => "🚨 META EM RISCO: ", Some("atencao") => "⚠️ Atenção na meta: ", _ => "📈 Meta no caminho: " })
}

fn ler(dados: &Path) -> Value {
    std::fs::read_to_string(dados.join("cofre.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(|| json!({}))
}

/// Guarda o saldo informado (e a meta, se vier). A diferenca para o saldo anterior vira um movimento do dia: e dai que sai o ritmo.
pub fn informar(dados: &Path, hoje: Data, saldo: Option<f64>, meta: Option<f64>) -> Result<Value, String> {
    let ok = |v: f64| v.is_finite() && (0.0..1e9).contains(&v);
    if saldo.is_none() && meta.is_none() {
        return Err("mande {\"saldo\": 123.45} e/ou {\"meta\": 1600}".into());
    }
    if !saldo.map_or(true, ok) || !meta.map_or(true, |m| ok(m) && m > 0.0) {
        return Err("valor fora do esperado".into());
    }
    let mut e = ler(dados);
    if let Some(m) = meta {
        e["meta"] = json!(m);
    }
    if let Some(s) = saldo {
        if let Some(antes) = e["saldo"].as_f64().filter(|a| (a - s).abs() >= 0.005) {
            let mut movs = e["movs"].as_array().cloned().unwrap_or_default();
            movs.push(json!({ "data": hoje.iso(), "valor": s - antes }));
            let corte = movs.len().saturating_sub(400);
            e["movs"] = json!(movs[corte..]);
        }
        e["saldo"] = json!(s);
        e["atualizado"] = json!(hoje.iso());
    }
    std::fs::write(dados.join("cofre.json"), e.to_string()).map_err(|x| format!("nao gravei o cofre: {x}"))?;
    Ok(situacao(dados, hoje))
}

/// O que o Cofre sabe hoje, com a fala pronta. Sem meta ou sem saldo informado, diz o que falta em vez de chutar.
pub fn situacao(dados: &Path, hoje: Data) -> Value {
    let e = ler(dados);
    let prazo = prazo_atual(hoje);
    let (Some(meta), Some(saldo)) = (e["meta"].as_f64(), e["saldo"].as_f64()) else {
        let falta = match (e["meta"].as_f64(), e["saldo"].as_f64()) { (None, None) => "a meta e o saldo", (None, _) => "a meta", _ => "o saldo" };
        return json!({ "tem": false, "prazo": prazo.iso(), "fala": format!("Não tenho {falta} registrados aqui. O próximo 5º dia útil é {} ({} dia(s) útil(eis)).", prazo.dia_mes(), dias_uteis_ate(hoje, prazo)) });
    };
    let movs: Vec<(Data, f64)> = e["movs"].as_array().into_iter().flatten()
        .filter_map(|m| Some((Data::de_iso(m["data"].as_str()?)?, m["valor"].as_f64()?))).collect();
    let mut av = avaliar(saldo, meta, &movs, hoje);
    let visto = e["atualizado"].as_str().and_then(Data::de_iso).map(|d| d.dia_mes()).unwrap_or_default();
    av["fala"] = json!(format!("{} (Saldo informado em {visto}; eu não leio o banco.)", texto(&av)));
    av["tem"] = json!(true);
    av
}

#[cfg(test)]
mod tests {
    use super::*;
    fn d(y: i64, m: i64, dia: i64) -> Data { Data::civil(y, m, dia).unwrap() }

    #[test]
    fn quinto_dia_util_pula_fim_de_semana_e_feriado() {
        assert_eq!(quinto_dia_util(2026, 11), d(2026, 11, 9));     // 02/11 e feriado: 3,4,5,6 e 9
        assert_eq!(quinto_dia_util(2026, 12), d(2026, 12, 7));
        assert_eq!(quinto_dia_util(2026, 4), d(2026, 4, 8));       // 03/04 Sexta-feira Santa
        assert_eq!(prazo_atual(d(2026, 11, 18)), d(2026, 12, 7));  // o de novembro ja passou
        assert_eq!(prazo_atual(d(2026, 12, 20)), quinto_dia_util(2027, 1));
        assert_eq!(dias_uteis_ate(d(2026, 12, 1), d(2026, 12, 7)), 4);
    }

    #[test]
    fn avalia_o_nivel_e_fala_em_reais() {
        let hoje = d(2026, 11, 18);
        assert_eq!(avaliar(1700.0, 1600.0, &[], hoje)["nivel"], "batida");
        assert_eq!(avaliar(100.0, 1600.0, &[], hoje)["nivel"], "critico"); // sem historico a previsao e o proprio saldo
        let movs: Vec<(Data, f64)> = (0..10).map(|i| (Data(hoje.0 - i), 150.0)).collect();
        let av = avaliar(100.0, 1600.0, &movs, hoje);
        assert_eq!(av["nivel"], "ok");
        assert!(texto(&av).starts_with("📈") && texto(&av).contains("R$ 1.500,00"));
        assert_eq!(brl(1234567.891), "R$ 1.234.567,89");
        assert_eq!(ritmo(&[(hoje, 100.0), (hoje, -40.0), (Data(hoje.0 - 30), 999.0)], hoje), (100.0, 40.0, 1));
    }

    #[test]
    fn sem_saldo_nao_chuta_e_a_diferenca_vira_movimento() {
        let p = std::env::temp_dir().join(format!("braniac-cofre-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        let hoje = d(2026, 11, 18);
        let s = situacao(&p, hoje);
        assert!(s["tem"] == false && s["fala"].as_str().unwrap().contains("Não tenho a meta e o saldo"));
        assert!(informar(&p, hoje, None, None).is_err() && informar(&p, hoje, Some(-5.0), None).is_err() && informar(&p, hoje, Some(f64::NAN), None).is_err());
        informar(&p, hoje, Some(500.0), Some(1600.0)).unwrap();
        let s = informar(&p, Data(hoje.0 + 1), Some(650.0), None).unwrap();
        assert!(s["tem"] == true && s["falta"] == 950.0 && s["fala"].as_str().unwrap().contains("não leio o banco"));
        assert_eq!(ler(&p)["movs"][0]["valor"], 150.0);
        let _ = std::fs::remove_dir_all(&p);
    }
}
