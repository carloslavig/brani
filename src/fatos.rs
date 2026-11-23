//! Fatos por assunto: cada assunto tem o seu banco (dados/assuntos/<id>.db, mesmo formato do Braniac em Python).
//! Todo fato tem origem, data e VALIDADE ("so hoje" vence as 5h). O curador vence o que passou do prazo, refaz a ficha
//! e aponta fatos que se contradizem. Nada e apagado: so marcado como vencido.
use crate::contradiz::contradiz;
use crate::tempo::{Agora, Data};
use crate::texto_pt::sem_acento;
use crate::{agentes, banco};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Mutex;

static TRAVA: Mutex<()> = Mutex::new(());

pub fn agora_ts() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

fn db(dados: &Path, id: &str) -> Result<Connection, String> {
    // o id vira nome de arquivo: so letras minusculas, digitos e traco
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
        return Err(format!("id de assunto inválido: {id:?}"));
    }
    let pasta = dados.join("assuntos");
    std::fs::create_dir_all(&pasta).map_err(|e| e.to_string())?;
    let c = Connection::open(pasta.join(format!("{id}.db"))).map_err(|e| e.to_string())?;
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS fatos(id INTEGER PRIMARY KEY, chave TEXT UNIQUE, texto TEXT, nivel TEXT DEFAULT 'local', fonte TEXT, ts REAL,
            validade REAL, ativo INTEGER DEFAULT 1, motivo TEXT);
         CREATE TABLE IF NOT EXISTS ficha(k TEXT PRIMARY KEY, texto TEXT, ts REAL);
         CREATE TABLE IF NOT EXISTS log(id INTEGER PRIMARY KEY, ts REAL, evento TEXT);",
    ).map_err(|e| e.to_string())?;
    Ok(c)
}

/// Prazo de fato temporario: "so hoje" vale ate as 5h do dia seguinte ("Hoje (11/10/2026)" conta daquela data);
/// "esta semana" 7 dias; "temporariamente" 3 dias. None = sem prazo.
pub fn validade_do_texto(texto: &str, agora: &Agora, ts: f64) -> Option<f64> {
    let limpo: String = sem_acento(texto).chars().map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' }).collect();
    let w: Vec<&str> = limpo.split_whitespace().collect();
    let t = format!(" {} ", w.join(" "));
    let tem = |frases: &[&str]| frases.iter().any(|f| t.contains(&format!(" {f} ")));
    let num = |i: usize, max: usize| w.get(i).filter(|x| x.len() <= max && x.chars().all(|c| c.is_ascii_digit())).and_then(|x| x.parse::<i64>().ok());
    // "hoje 11 10 [2026]": a data dita junto do "hoje"
    let datado = w.iter().position(|x| *x == "hoje").and_then(|i| Some((num(i + 1, 2)?, num(i + 2, 2)?, num(i + 3, 4).filter(|a| *a > 999))));
    if datado.is_some() || tem(&["so hoje", "apenas hoje", "somente hoje", "so por hoje", "apenas por hoje", "somente por hoje", "so vale hoje"]) {
        let dita = datado.and_then(|(d, m, a)| Data::civil(a.unwrap_or(agora.data.partes().0), m, d));
        // com data dita a conta parte do meio-dia daquele dia; sem ela, de agora (antes das 5h ainda e "hoje" ate as 5h)
        let dias = match dita {
            Some(d) => d.0 - agora.data.0 + 1,
            None => i64::from(agora.hora >= 5),
        };
        return Some(ts - agora.segundos() as f64 + (dias * 86400 + 5 * 3600) as f64);
    }
    if tem(&["nesta semana", "esta semana", "essa semana"]) {
        return Some(ts + 7.0 * 86400.0);
    }
    tem(&["temporariamente"]).then_some(ts + 3.0 * 86400.0)
}

/// Grava/atualiza um fato no banco DO ASSUNTO. Se a chave ja tinha outro texto, o antigo vira historico no log.
pub fn registrar(dados: &Path, assunto: &str, chave: &str, texto: &str, nivel: &str, fonte: &str, validade: Option<f64>, ts: f64) -> Result<(), String> {
    let (chave, texto) = (chave.chars().take(120).collect::<String>(), texto.trim());
    let nivel = if nivel == "publico" { "publico" } else { "local" }; // na duvida, nao sai do PC
    let _t = TRAVA.lock().map_err(|e| e.to_string())?;
    let c = db(dados, assunto)?;
    let ant: Option<String> = c.query_row("SELECT texto FROM fatos WHERE chave=?", [&chave], |r| r.get(0)).ok();
    if let Some(a) = ant.filter(|a| a != texto) {
        let _ = c.execute("INSERT INTO log(ts,evento) VALUES(?,?)", params![ts, format!("substituido [{chave}]: {}", a.chars().take(200).collect::<String>())]);
    }
    c.execute(
        "INSERT INTO fatos(chave,texto,nivel,fonte,ts,validade,ativo) VALUES(?,?,?,?,?,?,1) ON CONFLICT(chave) DO UPDATE SET
            texto=excluded.texto,nivel=excluded.nivel,fonte=excluded.fonte,ts=excluded.ts,validade=excluded.validade,ativo=1,motivo=NULL",
        params![chave, texto, nivel, fonte, ts, validade],
    ).map(|_| ()).map_err(|e| e.to_string())
}

/// Chamado a cada fato gravado no banco principal: copia para o banco do assunto certo. Devolve o id do assunto.
pub fn espelhar(dados: &Path, escopo: &str, chave: &str, texto: &str, nivel: &str, fonte: &str, validade: Option<f64>) -> Option<String> {
    let d = agentes::detectar(dados, &format!("{escopo} {chave} {texto}"));
    let id = d.first()?.0["id"].as_str()?.to_string();
    let chave: String = format!("{escopo}:{chave}").chars().take(120).collect();
    registrar(dados, &id, &chave, texto, nivel, fonte, validade, agora_ts()).ok()?;
    // o fato mudou de assunto: sai dos outros (nao fica valido em dois lugares)
    for outro in agentes::listar(dados).iter().filter_map(|a| a["id"].as_str()).filter(|o| *o != id && dados.join("assuntos").join(format!("{o}.db")).exists()) {
        let Ok(_t) = TRAVA.lock() else { continue };
        if let Ok(c) = db(dados, outro) {
            let _ = c.execute("UPDATE fatos SET ativo=0, motivo=? WHERE chave=? AND ativo=1", params![format!("movido para {id}"), chave]);
        }
    }
    Some(id)
}

/// Fatos validos de um assunto (texto, ts), do mais novo ao mais antigo. `so_publico` = o que pode ir a IA online.
pub fn validos(dados: &Path, id: &str, so_publico: bool, ts: f64) -> Vec<(String, f64)> {
    let Ok(_t) = TRAVA.lock() else { return vec![] };
    let Ok(c) = db(dados, id) else { return vec![] };
    let filtro = if so_publico { " AND nivel='publico'" } else { "" };
    let Ok(mut s) = c.prepare(&format!("SELECT texto,ts FROM fatos WHERE ativo=1 AND (validade IS NULL OR validade>?){filtro} ORDER BY ts DESC")) else { return vec![] };
    let r = s.query_map([ts], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)));
    r.map(|it| it.flatten().collect()).unwrap_or_default()
}

/// Todos os fatos de um assunto para a tela, validos primeiro e do mais novo ao mais antigo. Vencido continua aparecendo (nada e apagado).
pub fn todos(dados: &Path, id: &str, ts: f64) -> Vec<Value> {
    if !dados.join("assuntos").join(format!("{id}.db")).exists() {
        return vec![]; // so olhar a tela nao cria banco vazio
    }
    let Ok(_t) = TRAVA.lock() else { return vec![] };
    let Ok(c) = db(dados, id) else { return vec![] };
    let Ok(mut s) = c.prepare("SELECT texto,nivel,fonte,ts,validade,ativo=1 AND (validade IS NULL OR validade>?1) FROM fatos ORDER BY 6 DESC, ts DESC LIMIT 200") else { return vec![] };
    let r = s.query_map([ts], |r| {
        Ok(json!({ "texto": r.get::<_, String>(0)?, "nivel": r.get::<_, Option<String>>(1)?, "fonte": r.get::<_, Option<String>>(2)?,
                   "ts": r.get::<_, Option<f64>>(3)?, "validade": r.get::<_, Option<f64>>(4)?, "valido": r.get::<_, bool>(5)? }))
    });
    r.map(|it| it.flatten().collect()).unwrap_or_default()
}

/// Os fatos validos que mais tem a ver com a fala, para o prompt (ate 5).
pub fn bloco(dados: &Path, id: &str, texto: &str, so_publico: bool) -> String {
    let pal = banco::palavras(texto);
    let mut f = validos(dados, id, so_publico, agora_ts());
    f.sort_by(|a, b| banco::pontua(&b.0, &pal).cmp(&banco::pontua(&a.0, &pal))); // estavel: empate fica no mais novo
    let l: Vec<String> = f.iter().take(5).map(|(t, _)| format!("- {}", t.chars().take(260).collect::<String>())).collect();
    if l.is_empty() { "- (nada registrado)".into() } else { l.join("\n") }
}

/// Curador: vence os fatos com prazo, refaz a ficha de cada assunto e acha contradicoes entre fatos validos.
/// Devolve (vencidos, [(agente, fato, fato)]).
pub fn curar(dados: &Path, ts: f64) -> (usize, Vec<(String, String, String)>) {
    let (mut vencidos, mut contra) = (0, vec![]);
    let curto = |t: &str| t.chars().take(90).collect::<String>();
    for a in agentes::listar(dados) {
        let (Some(id), agente, nome) = (a["id"].as_str(), a["agente"].as_str().unwrap_or(""), a["nome"].as_str().unwrap_or("")) else { continue };
        if !dados.join("assuntos").join(format!("{id}.db")).exists() {
            continue; // assunto sem nenhum fato: nao cria banco vazio
        }
        {
            let Ok(_t) = TRAVA.lock() else { continue };
            let Ok(c) = db(dados, id) else { continue };
            let chaves: Vec<String> = c.prepare("SELECT chave FROM fatos WHERE ativo=1 AND validade IS NOT NULL AND validade<?").ok()
                .map(|mut s| s.query_map([ts], |r| r.get(0)).map(|it| it.flatten().collect()).unwrap_or_default()).unwrap_or_default();
            for ch in &chaves {
                let _ = c.execute("UPDATE fatos SET ativo=0, motivo='venceu' WHERE chave=?", [ch]);
                let _ = c.execute("INSERT INTO log(ts,evento) VALUES(?,?)", params![ts, format!("venceu [{ch}]")]);
            }
            vencidos += chaves.len();
        }
        let vivos = validos(dados, id, false, ts);
        for (i, x) in vivos.iter().enumerate() {
            for y in &vivos[i + 1..] {
                if contradiz(&x.0, &y.0) {
                    contra.push((agente.to_string(), curto(&x.0), curto(&y.0)));
                }
            }
        }
        let linhas: Vec<String> = vivos.iter().take(12).map(|(t, _)| format!("- {}", t.chars().take(240).collect::<String>())).collect();
        let ficha: String = format!("ASSUNTO {nome} (agente {agente}). Fatos válidos, do mais novo ao mais antigo:\n{}", linhas.join("\n")).chars().take(2400).collect();
        let Ok(_t) = TRAVA.lock() else { continue };
        if let Ok(c) = db(dados, id) {
            let _ = c.execute("INSERT INTO ficha(k,texto,ts) VALUES('atual',?,?) ON CONFLICT(k) DO UPDATE SET texto=excluded.texto, ts=excluded.ts", params![ficha, ts]);
        }
    }
    (vencidos, contra)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agora(hora: u32) -> Agora {
        Agora { data: Data::civil(2026, 11, 17).unwrap(), hora, min: 0, seg: 0 }
    }

    #[test]
    fn validade_so_hoje_semana_e_data_dita() {
        let ts = 1_000_000.0;
        // 10h: vence as 5h de amanha = faltam 19 h
        assert_eq!(validade_do_texto("Brócolis em falta SÓ HOJE", &agora(10), ts), Some(ts + 19.0 * 3600.0));
        // 3h da madrugada: ainda e "hoje" ate as 5h
        assert_eq!(validade_do_texto("so por hoje sem entrega", &agora(3), ts), Some(ts + 2.0 * 3600.0));
        // data dita de 2 dias atras: ja nasce vencido
        assert!(validade_do_texto("Hoje (15/11/2026) não tem rúcula", &agora(10), ts).unwrap() < ts);
        assert_eq!(validade_do_texto("promoção esta semana", &agora(10), ts), Some(ts + 7.0 * 86400.0));
        assert_eq!(validade_do_texto("fechado temporariamente", &agora(10), ts), Some(ts + 3.0 * 86400.0));
        assert_eq!(validade_do_texto("hoje eu almocei cedo; a loja abre as 18h", &agora(10), ts), None);
    }

    #[test]
    fn espelha_no_assunto_vence_e_aponta_contradicao() {
        let d = std::env::temp_dir().join(format!("braniac-fatos-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("agentes.json"), r#"{"agentes": [{"id": "loja", "nome": "Loja", "agente": "Balcão", "descricao": "x", "instrucao": "Regra.", "palavras": ["esfiha", "loja"]},
            {"id": "frota", "nome": "Frota", "agente": "Roda", "descricao": "x", "instrucao": "Regra.", "palavras": ["carro"]}]}"#).unwrap();
        let ts = agora_ts();
        assert_eq!(espelhar(&d, "aprendido", "a", "A esfiha de brócolis da loja central acabou hoje", "local", "t", None).as_deref(), Some("loja"));
        assert_eq!(espelhar(&d, "aprendido", "b", "A esfiha de brócolis da loja central voltou hoje", "publico", "t", None).as_deref(), Some("loja"));
        assert_eq!(espelhar(&d, "aprendido", "c", "esfiha doce só hoje", "local", "t", Some(ts - 10.0)).as_deref(), Some("loja"));
        assert_eq!(espelhar(&d, "aprendido", "z", "o céu é azul", "local", "t", None), None);
        assert!(registrar(&d, "../fora", "k", "x", "local", "t", None, ts).is_err());
        assert_eq!(validos(&d, "loja", false, ts).len(), 2); // o vencido nao aparece
        assert_eq!(validos(&d, "loja", true, ts).len(), 1); // online so ve o publico
        assert!(bloco(&d, "loja", "brócolis", true).contains("voltou") && !bloco(&d, "loja", "brócolis", true).contains("acabou"));
        let (venc, contra) = curar(&d, ts);
        assert_eq!((venc, contra.len(), contra[0].0.as_str()), (1, 1, "Balcão"));
        assert_eq!(curar(&d, ts).0, 0); // vencido so conta uma vez
        let t = todos(&d, "loja", ts); // a tela mostra os 3: validos primeiro, o vencido por ultimo
        assert_eq!((t.len(), t[0]["valido"].as_bool(), t[2]["valido"].as_bool(), t[2]["texto"].as_str()), (3, Some(true), Some(false), Some("esfiha doce só hoje")));
        assert!(todos(&d, "nunca-usado", ts).is_empty() && !d.join("assuntos").join("nunca-usado.db").exists());
        // regravar a mesma chave reativa o fato e guarda o texto antigo no log
        espelhar(&d, "aprendido", "c", "esfiha doce voltou de vez", "local", "t", None);
        assert_eq!(validos(&d, "loja", false, ts).len(), 3);
        let c = db(&d, "loja").unwrap();
        let logs: i64 = c.query_row("SELECT COUNT(*) FROM log WHERE evento LIKE 'substituido%'", [], |r| r.get(0)).unwrap();
        let ficha: String = c.query_row("SELECT texto FROM ficha WHERE k='atual'", [], |r| r.get(0)).unwrap();
        assert!(logs == 1 && ficha.starts_with("ASSUNTO Loja (agente Balcão)"));
        drop(c);
        // a mesma chave regravada com outro assunto: sai da loja e fica so na frota
        assert_eq!(espelhar(&d, "aprendido", "c", "o carro voltou da oficina", "local", "t", None).as_deref(), Some("frota"));
        assert_eq!((validos(&d, "loja", false, ts).len(), validos(&d, "frota", false, ts).len()), (2, 1));
        let _ = std::fs::remove_dir_all(&d);
    }
}
