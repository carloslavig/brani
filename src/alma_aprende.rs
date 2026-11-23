//! A Alma aprende (braniac/alma_aprende.py), 100% local e sem IA: tema por Naive Bayes (confianca < 0,55 = "indefinido", nunca forca tema),
//! etiqueta de contexto (trabalho/casa/neutro), sinais indiretos (SO numeros de cada mensagem: tamanho, pontuacao, emoji, giria, intervalo),
//! humor por z-score contra a propria linha de base com histerese (unico efeito: pedir ao Jarvis um tom mais calmo; nunca pergunta de humor),
//! perfil por janela e exemplos aprovados. Privacidade: consentimento por parte, pausa, apagar tudo, retencao de 90 dias, e texto com
//! segredo ou de crise nunca entra (nem vira numero). Banco: dados/alma/perfil.db (mesmas tabelas do Python).
//! Diferente do Python: o relogio e `Agora` (segundos LOCAIS, como o resto da Alma em Rust) e os nomes da familia vem de configuracao.
#![allow(dead_code)]
use crate::alma_cuida::{rx, ts};
use crate::tempo::Agora;
use rusqlite::{params, Connection};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub const CONF_MIN: f64 = 0.55;
const RETENCAO_S: f64 = 90.0 * 86400.0;
const SINAIS: [&str; 6] = ["comp", "excl", "ret", "emoji", "giria", "intervalo"];
const PISO: [f64; 6] = [3.0, 0.5, 0.5, 0.5, 0.5, 120.0]; // base sem variacao nao pode dividir por ~zero
const PARTES: [&str; 4] = ["temas", "sinais", "exemplos", "pausado"];
const TEMAS: [&str; 6] = ["saude", "emocional", "familia", "ti", "trabalho", "outros"];
const EMOJI: &str = r"[\x{1F300}-\x{1FAFF}☀-➿]";
const GIRIAS: &str = r"(?i)\b(né|tá|pra|vc|bora|cara|beleza|blz|valeu|vlw|opa|mano|poha|uai|kkk+|show|massa)\b";
const SEGREDO: &str = r"(?i)cart[aã]o|cpf|senha|c[oó]digo|token|pix|transfer|pagamento|boleto|conta banc|chave|segredo|login|\bcvv\b";
const CRISE: &str = r"(?i)suicid|me matar|quero morrer|n[aã]o quero mais viver|desistir de tudo|acabar com tudo|me machucar";

fn pasta(dados: &Path) -> PathBuf {
    dados.join("alma")
}

fn db(dados: &Path) -> Result<Connection, String> {
    std::fs::create_dir_all(pasta(dados)).map_err(|e| e.to_string())?;
    let c = Connection::open(pasta(dados).join("perfil.db")).map_err(|e| e.to_string())?;
    c.execute_batch("CREATE TABLE IF NOT EXISTS sinais(ts REAL, hora INT, dow INT, comp REAL, excl REAL, ret REAL, emoji REAL, giria REAL, intervalo REAL);
        CREATE TABLE IF NOT EXISTS rotulos(id INTEGER PRIMARY KEY, ts REAL, texto TEXT, tema_sugerido TEXT, tema_dono TEXT);
        CREATE TABLE IF NOT EXISTS estilo(janela TEXT, ts REAL, json TEXT);
        CREATE TABLE IF NOT EXISTS temperamento(ts REAL, json TEXT);
        CREATE TABLE IF NOT EXISTS exemplos(id INTEGER PRIMARY KEY, ts REAL, situacao TEXT, resposta TEXT, contexto TEXT, aprovado INT DEFAULT 1);
        CREATE TABLE IF NOT EXISTS correcoes(ts REAL, o_que TEXT, antes TEXT, depois TEXT);").map_err(|e| e.to_string())?;
    Ok(c)
}

fn agora_s(a: &Agora) -> f64 {
    ts(a) as f64
}

// ---------- consentimento ----------

/// {"temas", "sinais", "exemplos": true, "pausado": false} + o que o dono mudou.
pub fn consentimento(dados: &Path) -> Value {
    let mut c = json!({ "temas": true, "sinais": true, "exemplos": true, "pausado": false });
    let salvo: Value = std::fs::read_to_string(pasta(dados).join("consentimento.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    for k in PARTES {
        if let Some(b) = salvo[k].as_bool() {
            c[k] = json!(b);
        }
    }
    c
}

pub fn definir_consentimento(dados: &Path, mudancas: &[(&str, bool)]) -> Value {
    let mut c = consentimento(dados);
    for (k, v) in mudancas.iter().filter(|(k, _)| PARTES.contains(k)) {
        c[*k] = json!(v);
    }
    let _ = std::fs::create_dir_all(pasta(dados));
    let _ = std::fs::write(pasta(dados).join("consentimento.json"), c.to_string());
    c
}

fn ativo(dados: &Path, parte: &str) -> bool {
    let c = consentimento(dados);
    c["pausado"] != true && c[parte] != false
}

fn sensivel(t: &str) -> bool {
    rx(SEGREDO).is_match(t) || rx(CRISE).is_match(t)
}

fn tokens(t: &str) -> Vec<String> {
    rx(r"[a-zà-ÿ]{3,}").find_iter(&t.to_lowercase()).map(|m| m.as_str().to_string()).collect()
}

// ---------- 1) tema + contexto ----------

/// trabalho | casa | neutro, por tema e palavras. `familia` = nomes e apelidos da casa (configuracao).
pub fn contexto(texto: &str, tema: &str, familia: &[String]) -> &'static str {
    let baixo = texto.to_lowercase();
    if ["trabalho", "ti"].contains(&tema) || rx(r"(?i)cliente|vaga|entrevista|prazo|reuni[aã]o|projeto|deploy|servidor").is_match(texto) {
        "trabalho"
    } else if ["familia", "saude", "emocional"].contains(&tema) || rx(r"(?i)casa|jantar|almo[cç]o|filho|m[aã]e|pai\b").is_match(texto)
        || familia.iter().any(|n| !n.trim().is_empty() && baixo.contains(&n.trim().to_lowercase())) {
        "casa"
    } else {
        "neutro"
    }
}

/// Amostras de treino: o diario (tema ja dado pelas regras) + os rotulos que o dono deu.
pub fn treino(dados: &Path) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = std::fs::read_to_string(pasta(dados).join("diario.jsonl")).unwrap_or_default().lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|x| x["tema"].as_str().filter(|t| !t.is_empty()).map(|t| (t.to_string(), tokens(x["texto"].as_str().unwrap_or("")))))
        .collect();
    if let Ok(c) = db(dados) {
        if let Ok(mut s) = c.prepare("SELECT tema_dono, texto FROM rotulos WHERE tema_dono IS NOT NULL AND tema_dono != ''") {
            if let Ok(linhas) = s.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))) {
                out.extend(linhas.flatten().map(|(tema, texto)| (tema, tokens(&texto))));
            }
        }
    }
    out
}

/// Pura. (tema, confianca). Naive Bayes multinomial com suavizacao de Laplace. Sem dados suficientes: ("indefinido", 0.0).
pub fn tema_nb(texto: &str, treino: &[(String, Vec<String>)]) -> (String, f64) {
    let toks = tokens(texto);
    let mut classes: Vec<(&str, f64)> = Vec::new(); // na ordem em que aparecem (desempate igual ao do Python)
    for (t, _) in treino {
        match classes.iter_mut().find(|c| c.0 == t) {
            Some(c) => c.1 += 1.0,
            None => classes.push((t, 1.0)),
        }
    }
    if treino.len() < 20 || classes.len() < 2 || toks.is_empty() {
        return ("indefinido".into(), 0.0);
    }
    let vocab: HashSet<&String> = treino.iter().flat_map(|(_, ws)| ws).collect();
    let logs: Vec<f64> = classes.iter().map(|(k, n)| {
        let mut cont: HashMap<&String, f64> = HashMap::new();
        let mut soma = 0.0;
        for w in treino.iter().filter(|(t, _)| t == k).flat_map(|(_, ws)| ws) {
            *cont.entry(w).or_default() += 1.0;
            soma += 1.0;
        }
        let tot = soma + vocab.len() as f64 + 1.0;
        (n / treino.len() as f64).ln() + toks.iter().map(|w| ((cont.get(w).copied().unwrap_or(0.0) + 1.0) / tot).ln()).sum::<f64>()
    }).collect();
    let m = logs.iter().cloned().fold(f64::MIN, f64::max);
    let exp: Vec<f64> = logs.iter().map(|v| (v - m).exp()).collect();
    let (mut melhor, s) = (0, exp.iter().sum::<f64>());
    for (i, e) in exp.iter().enumerate() {
        if *e > exp[melhor] {
            melhor = i;
        }
    }
    let conf = exp[melhor] / s;
    (if conf >= CONF_MIN { classes[melhor].0.to_string() } else { "indefinido".into() }, (conf * 100.0).round() / 100.0)
}

/// Chamado pelo ciclo da Alma: mantem o tema das regras; se vazio, tenta o NB. Acrescenta confianca e contexto em `c`.
pub fn completar_tema(dados: &Path, texto: &str, c: &mut Value, familia: &[String]) {
    if c["tema"].as_str().is_some_and(|t| !t.is_empty()) {
        c["confianca"] = json!(1.0);
    } else if ativo(dados, "temas") {
        let (tema, conf) = tema_nb(texto, &treino(dados));
        let indef = tema == "indefinido";
        c["tema"] = json!(if indef { "" } else { &tema });
        c["confianca"] = json!(conf);
        if indef {
            c["tema_indef"] = json!(true);
        }
    }
    c["contexto"] = json!(contexto(texto, c["tema"].as_str().unwrap_or(""), familia));
}

/// Frase sem tema entra na fila para o dono rotular. Crise, segredo, frase curta e repetida ficam de fora.
pub fn enfileirar_rotulo(dados: &Path, agora: &Agora, texto: &str, tema_sugerido: &str) -> bool {
    if !ativo(dados, "temas") || sensivel(texto) || texto.chars().count() < 12 {
        return false;
    }
    let t: String = texto.chars().take(200).collect();
    let Ok(c) = db(dados) else { return false };
    if c.query_row("SELECT 1 FROM rotulos WHERE texto=?", params![t], |_| Ok(())).is_ok() {
        return false;
    }
    c.execute("INSERT INTO rotulos(ts,texto,tema_sugerido) VALUES(?,?,?)", params![agora_s(agora), t, tema_sugerido]).is_ok()
}

pub fn pendentes_rotulo(dados: &Path, n: i64) -> Vec<Value> {
    let Ok(c) = db(dados) else { return vec![] };
    let Ok(mut s) = c.prepare("SELECT id,texto,tema_sugerido FROM rotulos WHERE tema_dono IS NULL ORDER BY ts DESC LIMIT ?") else { return vec![] };
    let r = s.query_map(params![n], |r| Ok(json!({ "id": r.get::<_, i64>(0)?, "texto": r.get::<_, String>(1)?, "sugerido": r.get::<_, Option<String>>(2)? })));
    r.map(|it| it.flatten().collect()).unwrap_or_default()
}

/// O dono da o tema de uma frase. Tema fora da lista e recusado.
pub fn rotular(dados: &Path, agora: &Agora, id: i64, tema: &str) -> bool {
    if !TEMAS.contains(&tema) {
        return false;
    }
    let Ok(c) = db(dados) else { return false };
    let ok = c.execute("UPDATE rotulos SET tema_dono=? WHERE id=?", params![tema, id]).is_ok();
    let _ = c.execute("INSERT INTO correcoes VALUES(?,?,?,?)", params![agora_s(agora), "rotulo", "", tema]);
    ok
}

// ---------- 2) sinais e humor ----------

/// Pura. Os numeros de UMA mensagem, na ordem de SINAIS. O texto nao e guardado.
pub fn metricas(texto: &str, t: f64, anterior: Option<f64>) -> [f64; 6] {
    [texto.split_whitespace().count().max(1) as f64, texto.matches('!').count() as f64, rx(r"\.\.\.|…").find_iter(texto).count() as f64,
        rx(EMOJI).find_iter(texto).count() as f64, rx(GIRIAS).find_iter(texto).count() as f64, anterior.filter(|a| *a != 0.0).map_or(0.0, |a| (t - a).min(21600.0))]
}

/// Guarda SO numeros. Texto de crise ou com segredo nem gera metrica.
pub fn registrar_sinais(dados: &Path, agora: &Agora, texto: &str) -> bool {
    if !ativo(dados, "sinais") || sensivel(texto) {
        return false;
    }
    let Ok(c) = db(dados) else { return false };
    let ant: Option<f64> = c.query_row("SELECT MAX(ts) FROM sinais", [], |r| r.get(0)).unwrap_or(None);
    let (t, m) = (agora_s(agora), metricas(texto, agora_s(agora), ant));
    c.execute("INSERT INTO sinais VALUES(?,?,?,?,?,?,?,?,?)", params![t, agora.hora, agora.data.semana() as i64, m[0], m[1], m[2], m[3], m[4], m[5]]).is_ok()
}

fn faixa(h: i64) -> i64 {
    match h {
        ..=5 => 0,
        6..=11 => 1,
        12..=17 => 2,
        _ => 3,
    }
}

/// (hora, [sinais]) de cada mensagem no intervalo.
fn linhas(c: &Connection, de: f64, ate: f64) -> Vec<(i64, [f64; 6])> {
    let Ok(mut s) = c.prepare("SELECT hora,comp,excl,ret,emoji,giria,intervalo FROM sinais WHERE ts>=? AND ts<? ORDER BY ts") else { return vec![] };
    let r = s.query_map(params![de, ate], |r| Ok((r.get::<_, i64>(0)?, [r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?])));
    r.map(|it| it.flatten().collect()).unwrap_or_default()
}

fn media(l: &[(i64, [f64; 6])], i: usize) -> f64 {
    l.iter().map(|x| x.1[i]).sum::<f64>() / l.len() as f64
}

/// Quantos sinais saem do normal (|z| > 1,5) e o z medio de energia e de tensao. base = (media, desvio) de cada sinal.
fn janela_fora(l: &[(i64, [f64; 6])], base: &[(f64, f64); 6]) -> (usize, f64, f64) {
    let mut z = [0.0; 6];
    for i in 0..6 {
        z[i] = (media(l, i) - base[i].0) / base[i].1.max(PISO[i]);
    }
    (z.iter().filter(|v| v.abs() > 1.5).count(), (z[0] + z[1] + z[3] - z[5]) / 4.0, (z[1] + z[2]) / 2.0)
}

fn r2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// {estado, alerta, energia, tensao, confianca}. Exige linha de base (30+ mensagens na mesma faixa do dia, 14+ dias de historia) e 2 janelas
/// de 3 h seguidas com 3+ sinais fora. Histerese: so sai do alerta quando cai para menos de 2 sinais fora. Nao e diagnostico.
pub fn estado_humor(dados: &Path, agora: &Agora) -> Value {
    let vazio = |estado: &str| json!({ "estado": estado, "alerta": false, "energia": 0.0, "tensao": 0.0, "confianca": 0.0 });
    if !ativo(dados, "sinais") {
        return vazio("pausado");
    }
    let (t, Ok(c)) = (agora_s(agora), db(dados)) else { return vazio("sem linha de base") };
    let ini: Option<f64> = c.query_row("SELECT MIN(ts) FROM sinais", [], |r| r.get(0)).unwrap_or(None);
    if ini.is_none_or(|i| t - i < 14.0 * 86400.0) {
        return vazio("sem linha de base");
    }
    let (w1, w2) = (linhas(&c, t - 3.0 * 3600.0, t + 1.0), linhas(&c, t - 6.0 * 3600.0, t - 3.0 * 3600.0));
    let f = faixa(agora.hora as i64);
    let hist: Vec<(i64, [f64; 6])> = linhas(&c, t - 30.0 * 86400.0, t - 6.0 * 3600.0).into_iter().filter(|l| faixa(l.0) == f).collect();
    if hist.len() < 30 || w1.len() < 2 {
        return vazio("sem linha de base");
    }
    let mut base = [(0.0, 0.0); 6];
    for (i, b) in base.iter_mut().enumerate() {
        let mu = media(&hist, i);
        *b = (mu, (hist.iter().map(|x| (x.1[i] - mu).powi(2)).sum::<f64>() / hist.len() as f64).sqrt());
    }
    let (f1, e1, t1) = janela_fora(&w1, &base);
    let f2 = if w2.len() >= 2 { janela_fora(&w2, &base).0 } else { 0 };
    let arq = pasta(dados).join("humor_estado.json");
    let antes = std::fs::read_to_string(&arq).ok().and_then(|x| serde_json::from_str::<Value>(&x).ok()).is_some_and(|v| v["alerta"] == true);
    let alerta = (f1 >= 3 && f2 >= 3) || (antes && f1 >= 2);
    let _ = std::fs::write(arq, json!({ "alerta": alerta, "ts": t }).to_string());
    json!({ "estado": if alerta { "diferente do normal" } else { "normal" }, "alerta": alerta, "energia": r2(e1), "tensao": r2(t1), "confianca": r2((hist.len() as f64 / 120.0).min(1.0)) })
}

/// Trecho que o roteador acrescenta ao prompt do Jarvis. Vazio quando esta tudo normal (o caso comum).
pub fn dica_tom(dados: &Path, agora: &Agora, dono: &str) -> String {
    if estado_humor(dados, agora)["alerta"] != true {
        return String::new();
    }
    format!("\n\nTOM AGORA (sinal discreto da Alma): {dono} parece diferente do habitual. Seja um pouco mais breve, calmo e acolhedor. \
        NÃO pergunte como ele está, NÃO comente que percebeu algo, NÃO fale em humor ou emoção a menos que ele puxe o assunto.")
}

// ---------- 3) perfil ----------

/// Apaga sinais com mais de 90 dias e refaz o estilo por janela (7/30/90 dias) e o temperamento por faixa do dia.
pub fn atualizar_perfil(dados: &Path, agora: &Agora) {
    let (t, Ok(c)) = (agora_s(agora), db(dados)) else { return };
    let _ = c.execute("DELETE FROM sinais WHERE ts<?", params![t - RETENCAO_S]);
    for (nome, dias) in [("7d", 7.0), ("30d", 30.0), ("90d", 90.0)] {
        let ls = linhas(&c, t - dias * 86400.0, t + 1.0);
        if ls.len() < 5 {
            continue;
        }
        let mut m: Map<String, Value> = SINAIS.iter().enumerate().map(|(i, s)| (s.to_string(), json!(r2(media(&ls, i))))).collect();
        m.insert("n".into(), json!(ls.len()));
        let mut horas: Vec<(i64, usize)> = Vec::new();
        for l in &ls {
            match horas.iter_mut().find(|h| h.0 == l.0) {
                Some(h) => h.1 += 1,
                None => horas.push((l.0, 1)),
            }
        }
        horas.sort_by(|a, b| b.1.cmp(&a.1));
        m.insert("horas_ativas".into(), json!(horas.iter().take(4).map(|h| h.0).collect::<Vec<_>>()));
        let _ = c.execute("DELETE FROM estilo WHERE janela=?", params![nome]);
        let _ = c.execute("INSERT INTO estilo VALUES(?,?,?)", params![nome, t, Value::Object(m).to_string()]);
    }
    let ls = linhas(&c, t - 90.0 * 86400.0, t + 1.0);
    if ls.len() >= 30 {
        let mut curva = Map::new();
        for f in 0..4 {
            let v: Vec<(i64, [f64; 6])> = ls.iter().filter(|l| faixa(l.0) == f).cloned().collect();
            if !v.is_empty() {
                curva.insert(f.to_string(), json!({ "n": v.len(), "comp": (media(&v, 0) * 10.0).round() / 10.0, "excl": r2(media(&v, 1)), "intervalo": media(&v, 5).round() }));
            }
        }
        let _ = c.execute("DELETE FROM temperamento", []);
        let _ = c.execute("INSERT INTO temperamento VALUES(?,?)", params![t, Value::Object(curva).to_string()]);
    }
}

/// "Como ele responderia a X": so com aprovacao do dono. Segredo e crise nunca entram.
pub fn registrar_exemplo(dados: &Path, agora: &Agora, situacao: &str, resposta: &str, contexto: &str) -> bool {
    if !ativo(dados, "exemplos") || sensivel(situacao) || sensivel(resposta) {
        return false;
    }
    let corta = |t: &str, n: usize| t.chars().take(n).collect::<String>();
    db(dados).is_ok_and(|c| c.execute("INSERT INTO exemplos(ts,situacao,resposta,contexto) VALUES(?,?,?,?)", params![agora_s(agora), corta(situacao, 400), corta(resposta, 600), contexto]).is_ok())
}

/// Os k exemplos aprovados mais parecidos com a situacao (palavras em comum).
pub fn parecidos(dados: &Path, situacao: &str, k: usize) -> Vec<Value> {
    let a: HashSet<String> = tokens(situacao).into_iter().collect();
    let Ok(c) = db(dados) else { return vec![] };
    let Ok(mut s) = c.prepare("SELECT situacao,resposta,contexto FROM exemplos WHERE aprovado=1") else { return vec![] };
    let Ok(linhas) = s.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?))) else { return vec![] };
    let mut pont: Vec<(f64, String, String, Option<String>)> = linhas.flatten().map(|(sit, r, ctx)| {
        let b: HashSet<String> = tokens(&sit).into_iter().collect();
        (a.intersection(&b).count() as f64 / a.union(&b).count().max(1) as f64, sit, r, ctx)
    }).filter(|p| p.0 > 0.0).collect();
    pont.sort_by(|x, y| y.0.total_cmp(&x.0).then_with(|| y.1.cmp(&x.1)));
    pont.into_iter().take(k).map(|(p, sit, r, ctx)| json!({ "situacao": sit, "resposta": r, "contexto": ctx, "similaridade": r2(p) })).collect()
}

// ---------- painel: ver, apagar, pausar ----------

fn diario(dados: &Path) -> Vec<Value> {
    std::fs::read_to_string(pasta(dados).join("diario.jsonl")).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

/// Tudo que a Alma sabe, em numeros (para o painel).
pub fn sabe(dados: &Path, agora: &Agora) -> Value {
    let Ok(c) = db(dados) else { return json!({}) };
    let n = |q: &str| c.query_row(q, [], |r| r.get::<_, i64>(0)).unwrap_or(0);
    let mut estilo = Map::new();
    if let Ok(mut s) = c.prepare("SELECT janela,json FROM estilo") {
        if let Ok(l) = s.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))) {
            estilo.extend(l.flatten().map(|(j, x)| (j, serde_json::from_str(&x).unwrap_or_default())));
        }
    }
    let temp: Value = c.query_row("SELECT json FROM temperamento", [], |r| r.get::<_, String>(0)).ok().and_then(|x| serde_json::from_str(&x).ok()).unwrap_or_else(|| json!({}));
    let d = diario(dados);
    let tem_tema = |x: &Value| x["tema"].as_str().is_some_and(|t| !t.is_empty());
    let mut por_tema = Map::new();
    for x in &d {
        let k = if tem_tema(x) { x["tema"].as_str().unwrap_or("") } else { "sem tema" };
        por_tema.insert(k.to_string(), json!(por_tema.get(k).and_then(Value::as_i64).unwrap_or(0) + 1));
    }
    let sem = d.iter().filter(|x| !tem_tema(x)).count();
    json!({ "consentimento": consentimento(dados), "sinais": n("SELECT COUNT(*) FROM sinais"), "exemplos": n("SELECT COUNT(*) FROM exemplos"),
        "rotulos_pendentes": n("SELECT COUNT(*) FROM rotulos WHERE tema_dono IS NULL"), "estilo": estilo, "temperamento": temp, "humor": estado_humor(dados, agora),
        "diario": d.len(), "sem_tema_pct": if d.is_empty() { 0 } else { (100.0 * sem as f64 / d.len() as f64).round() as i64 }, "por_tema": por_tema })
}

/// escopo: sinais | exemplos | rotulos | perfil | diario | tema:<nome> | tudo. O pedido de apagar fica registrado (sem o conteudo).
pub fn apagar(dados: &Path, agora: &Agora, escopo: &str) -> bool {
    let Ok(c) = db(dados) else { return false };
    let em = |l: &[&str]| l.contains(&escopo);
    for (tabelas, vale) in [(&["sinais"][..], em(&["sinais", "tudo", "perfil"])), (&["exemplos"], em(&["exemplos", "tudo"])), (&["rotulos"], em(&["rotulos", "tudo"])), (&["estilo", "temperamento"], em(&["perfil", "tudo"]))] {
        for t in tabelas.iter().filter(|_| vale) {
            let _ = c.execute(&format!("DELETE FROM {t}"), []);
        }
    }
    let _ = c.execute("INSERT INTO correcoes VALUES(?,?,?,?)", params![agora_s(agora), "apagar", escopo, ""]);
    let arq = pasta(dados).join("diario.jsonl");
    if em(&["diario", "tudo"]) {
        let _ = std::fs::write(&arq, "");
        // conversa recente e, no "tudo", o historico de atividade
        let extras: &[&str] = if escopo == "tudo" { &["falas.jsonl", "diario_tcc.jsonl", "atividade.jsonl", "sobrecarga.jsonl"] } else { &["falas.jsonl", "diario_tcc.jsonl"] };
        for e in extras {
            let _ = std::fs::remove_file(pasta(dados).join(e));
        }
    } else if let Some(tema) = escopo.strip_prefix("tema:") {
        let fica: String = diario(dados).iter().filter(|x| x["tema"] != tema).map(|x| format!("{x}\n")).collect();
        let _ = std::fs::write(&arq, fica);
    }
    true
}

/// Texto curto para o Jarvis dar em audio: o que a Alma aprendeu na semana (so numeros, nada do que o dono escreveu).
pub fn resumo_semanal(dados: &Path, agora: &Agora) -> String {
    let s = sabe(dados, agora);
    let mut partes = vec![format!("{} registros no diário, {}% ainda sem tema", s["diario"], s["sem_tema_pct"]), format!("{} frases esperando você dar um tema", s["rotulos_pendentes"])];
    let e = &s["estilo"]["7d"];
    if e.is_object() {
        partes.push(format!("nos últimos 7 dias, {} mensagens, de {:.0} palavras em média", e["n"], e["comp"].as_f64().unwrap_or(0.0)));
    }
    format!("Resumo da Alma: {}. Você pode ver, corrigir ou apagar tudo em /alma.", partes.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alma_cuida::teste::{dir, em};

    #[test]
    fn nb_sem_dados_nao_forca_tema_e_com_dados_acerta() {
        assert_eq!(tema_nb("preciso pagar o aluguel", &[]).0, "indefinido");
        let w = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let mut t: Vec<(String, Vec<String>)> = vec![("trabalho".to_string(), w(&["cliente", "pagou", "projeto"])); 12];
        t.extend(vec![("familia".to_string(), w(&["filho", "escola", "jantar"])); 12]);
        let (tema, conf) = tema_nb("o cliente do projeto", &t);
        assert!(tema == "trabalho" && conf > 0.9);
        assert_eq!(tema_nb("zzz yyy", &t), ("indefinido".to_string(), 0.5)); // nenhuma palavra conhecida: confianca baixa
        assert_eq!(tema_nb("o cliente", &t[..12]).0, "indefinido"); // uma classe so nao e treino
        let fam = vec!["Dudu".to_string()];
        assert_eq!((contexto("fechei com o cliente", "", &fam), contexto("jantar com meu filho", "", &fam), contexto("ok", "", &fam), contexto("o dudu ligou", "", &fam), contexto("ok", "saude", &[])),
            ("trabalho", "casa", "neutro", "casa", "casa"));
        // completar: tema das regras fica; sem tema, o NB tenta e marca indefinido
        let d = dir("alma_aprende-nb");
        let mut c = json!({ "tema": "ti" });
        completar_tema(&d, "o servidor caiu", &mut c, &[]);
        assert!(c["confianca"] == 1.0 && c["contexto"] == "trabalho");
        let mut c = json!({});
        completar_tema(&d, "que dia bonito", &mut c, &[]);
        assert!(c["tema"] == "" && c["tema_indef"] == true && c["contexto"] == "neutro");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn sinais_guardam_so_numeros_ignoram_segredo_e_crise_e_a_pausa_desliga() {
        let (d, a) = (dir("alma_aprende-sinais"), em(16, 15, 0));
        assert!(registrar_sinais(&d, &a, "oi tudo bem!"));
        assert!(!registrar_sinais(&d, &a, "minha senha é 123") && !registrar_sinais(&d, &a, "quero morrer"));
        assert_eq!(sabe(&d, &a)["sinais"], 1);
        assert_eq!(metricas("bora ver isso... tá bom!! 🙂", 1000.0, Some(400.0)), [6.0, 2.0, 1.0, 1.0, 2.0, 600.0]);
        assert_eq!(metricas("", 99999.0, Some(1.0))[5], 21600.0); // intervalo tem teto de 6 h
        // sem linha de base nao alerta; pausado nao guarda nada
        assert!(estado_humor(&d, &a)["alerta"] == false && dica_tom(&d, &a, "Davi").is_empty());
        definir_consentimento(&d, &[("pausado", true), ("inventado", true)]);
        assert!(!registrar_sinais(&d, &a, "oi de novo") && estado_humor(&d, &a)["estado"] == "pausado" && consentimento(&d).get("inventado").is_none());
        assert!(!enfileirar_rotulo(&d, &a, "fiquei pensando na reforma do quintal", "") && !registrar_exemplo(&d, &a, "situacao qualquer", "resposta", "neutro"));
        definir_consentimento(&d, &[("pausado", false), ("sinais", false)]);
        assert!(!registrar_sinais(&d, &a, "oi") && enfileirar_rotulo(&d, &a, "fiquei pensando na reforma do quintal", "")); // consentimento por parte
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn humor_alerta_com_tres_sinais_em_duas_janelas_e_tem_histerese() {
        let (d, a) = (dir("alma_aprende-humor"), em(28, 15, 0));
        let (t, c) = (agora_s(&a), db(&d).unwrap());
        let poe = |ts: f64, v: [f64; 6]| c.execute("INSERT INTO sinais VALUES(?,?,?,?,?,?,?,?,?)", params![ts, 15, 0, v[0], v[1], v[2], v[3], v[4], v[5]]).unwrap();
        for i in 0..60 {
            // linha de base: 20 dias, mensagens curtas e calmas na mesma faixa do dia
            poe(t - (7 + i % 20) as f64 * 86400.0 + (i % 3) as f64 * 60.0, [5.0 + (i % 3) as f64, 0.0, 0.0, 0.0, 0.0, 300.0 + (i % 5) as f64]);
        }
        assert_eq!(estado_humor(&d, &a)["estado"], "sem linha de base"); // ainda sem mensagens agora
        for ts in [t - 100.0, t - 200.0, t - 3.0 * 3600.0 - 100.0, t - 3.0 * 3600.0 - 200.0] {
            poe(ts, [40.0, 6.0, 4.0, 5.0, 4.0, 5.0]); // duas janelas fora do normal
        }
        let h = estado_humor(&d, &a);
        assert!(h["alerta"] == true && h["estado"] == "diferente do normal" && h["confianca"] == 0.5);
        let dica = dica_tom(&d, &a, "Davi");
        assert!(dica.contains("Davi parece diferente") && dica.contains("NÃO pergunte como ele está"));
        // uma janela so fora do normal nao basta para ENTRAR em alerta
        let d2 = dir("alma_aprende-humor2");
        let c2 = db(&d2).unwrap();
        let poe2 = |ts: f64, v: [f64; 6]| c2.execute("INSERT INTO sinais VALUES(?,?,?,?,?,?,?,?,?)", params![ts, 15, 0, v[0], v[1], v[2], v[3], v[4], v[5]]).unwrap();
        for i in 0..60 {
            poe2(t - (7 + i % 20) as f64 * 86400.0, [5.0 + (i % 3) as f64, 0.0, 0.0, 0.0, 0.0, 300.0]);
        }
        poe2(t - 100.0, [40.0, 6.0, 4.0, 5.0, 4.0, 5.0]);
        poe2(t - 200.0, [40.0, 6.0, 4.0, 5.0, 4.0, 5.0]);
        assert_eq!(estado_humor(&d2, &a)["alerta"], false);
        std::fs::write(pasta(&d2).join("humor_estado.json"), r#"{"alerta": true}"#).unwrap();
        assert_eq!(estado_humor(&d2, &a)["alerta"], true); // ja estava em alerta: continua enquanto houver 2+ sinais fora
        // perfil: estilo por janela, e retencao de 90 dias
        poe(t - 100.0 * 86400.0, [9.0; 6]);
        atualizar_perfil(&d, &a);
        let s = sabe(&d, &a);
        assert!(s["sinais"] == 64 && s["estilo"]["30d"]["n"] == 64 && s["estilo"]["7d"]["horas_ativas"] == json!([15]) && s["temperamento"]["2"]["n"] == 64);
        assert!(resumo_semanal(&d, &a).contains("nos últimos 7 dias") && resumo_semanal(&d, &a).ends_with("apagar tudo em /alma."));
        drop((c, c2));
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::remove_dir_all(&d2);
    }

    #[test]
    fn exemplos_rotulos_e_apagar() {
        let (d, a) = (dir("alma_aprende-exemplos"), em(16, 15, 0));
        assert!(registrar_exemplo(&d, &a, "o cliente pediu desconto", "vou ver o que dá pra fazer", "trabalho"));
        assert!(!registrar_exemplo(&d, &a, "manda o pix", "ok", "neutro") && !registrar_exemplo(&d, &a, "situacao", "quero morrer", "neutro"));
        let p = parecidos(&d, "cliente pedindo desconto", 3);
        assert!(p.len() == 1 && p[0]["resposta"].as_str().unwrap().starts_with("vou ver") && p[0]["similaridade"] == 0.5 && parecidos(&d, "xyz", 3).is_empty());
        assert!(enfileirar_rotulo(&d, &a, "fiquei pensando na reforma do quintal", "") && !enfileirar_rotulo(&d, &a, "fiquei pensando na reforma do quintal", "")); // repetida
        assert!(!enfileirar_rotulo(&d, &a, "oi", "") && !enfileirar_rotulo(&d, &a, "qual a senha do roteador mesmo", ""));
        let id = pendentes_rotulo(&d, 10)[0]["id"].as_i64().unwrap();
        assert!(!rotular(&d, &a, id, "xyz") && rotular(&d, &a, id, "familia") && pendentes_rotulo(&d, 10).is_empty());
        assert_eq!(treino(&d), [("familia".to_string(), tokens("fiquei pensando na reforma do quintal"))]);
        std::fs::write(pasta(&d).join("diario.jsonl"), "{\"tema\":\"ti\",\"texto\":\"servidor\"}\n{\"tema\":\"\",\"texto\":\"x\"}\n{\"tema\":\"saude\",\"texto\":\"y\"}\n").unwrap();
        std::fs::write(pasta(&d).join("falas.jsonl"), "x").unwrap();
        std::fs::write(pasta(&d).join("atividade.jsonl"), "x").unwrap();
        let s = sabe(&d, &a);
        assert!(s["diario"] == 3 && s["sem_tema_pct"] == 33 && s["por_tema"]["sem tema"] == 1 && s["exemplos"] == 1);
        apagar(&d, &a, "tema:ti");
        assert!(sabe(&d, &a)["diario"] == 2 && sabe(&d, &a)["exemplos"] == 1);
        apagar(&d, &a, "diario");
        assert!(sabe(&d, &a)["diario"] == 0 && !pasta(&d).join("falas.jsonl").exists() && pasta(&d).join("atividade.jsonl").exists());
        apagar(&d, &a, "tudo");
        assert!(sabe(&d, &a)["exemplos"] == 0 && treino(&d).is_empty() && !pasta(&d).join("atividade.jsonl").exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
