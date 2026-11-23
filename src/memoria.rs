//! Memoria local (SQLite em dados/memoria.db, mesmo schema do braniac/memoria.py): mensagens por contato, respostas ja
//! conhecidas, decisoes, fatos por pessoa e correcoes. Regra de ouro: consultar o banco ANTES de perguntar a uma IA.
//! Dado de conversa fica SO aqui: nada deste modulo fala com a rede.
//! Ficou de fora: o guia do Claudinho dentro de `contexto_para_decidir` entra como parametro (`regras`), e a hora local
//! de `resumo_periodo` entra como closure (o modulo nao le relogio nem fuso).
#![allow(dead_code)]
use crate::texto_pt::sem_acento;
use rusqlite::{params, Connection};
use std::collections::BTreeSet;
use std::path::Path;

const PARADAS: &str = " a o as os um uma uns umas de do da dos das em no na nos nas por para pra pro com sem e ou que se eu voce vc tu ele ela \
nao sim ja mais mas so tambem eh sao foi ser esta estao tem ter como qual quais quem onde quando isso isto esse essa \
aquele aquela meu minha seu sua nosso me te lhe ao aos ";

fn corta(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Minusculas, sem acento, so letras e digitos, um espaco entre palavras.
// shortcut: so tira acento do portugues (texto_pt); letra acentuada de outra lingua vira espaco. Trocar se chegar contato estrangeiro.
pub fn norm(x: &str) -> String {
    let s: String = sem_acento(x).chars().map(|c| if c.is_ascii_lowercase() || c.is_ascii_digit() { c } else { ' ' }).collect();
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn palavras(x: &str) -> BTreeSet<String> {
    norm(x).split(' ').filter(|p| p.len() > 1 && !PARADAS.contains(&format!(" {p} "))).map(String::from).collect()
}

fn db(dados: &Path) -> Result<Connection, String> {
    std::fs::create_dir_all(dados).map_err(|e| e.to_string())?;
    let c = Connection::open(dados.join("memoria.db")).map_err(|e| e.to_string())?;
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS mensagens(id INTEGER PRIMARY KEY, contato TEXT, direcao TEXT, canal TEXT, texto TEXT, audio INTEGER DEFAULT 0, ts REAL);
         CREATE INDEX IF NOT EXISTS ix_msg ON mensagens(contato, ts);
         CREATE TABLE IF NOT EXISTS conhecimento(id INTEGER PRIMARY KEY, chave TEXT UNIQUE, pergunta TEXT, resposta TEXT, fonte TEXT,
             contato TEXT, criado REAL, usos INTEGER DEFAULT 0, ultimo_uso REAL);
         CREATE TABLE IF NOT EXISTS decisoes(id INTEGER PRIMARY KEY, contato TEXT, entrada TEXT, entrada_norm TEXT, cod TEXT, texto TEXT, ts REAL, usos INTEGER DEFAULT 0);
         CREATE INDEX IF NOT EXISTS ix_dec ON decisoes(contato, entrada_norm);
         CREATE TABLE IF NOT EXISTS fatos_contato(id INTEGER PRIMARY KEY, contato TEXT, fato TEXT, fonte TEXT, ts REAL, UNIQUE(contato, fato));
         CREATE TABLE IF NOT EXISTS correcoes(id INTEGER PRIMARY KEY, errado TEXT, certo TEXT, ts REAL, UNIQUE(errado, certo));",
    ).map_err(|e| e.to_string())?;
    Ok(c)
}

/// Linhas de uma consulta; banco fora do ar ou linha ilegivel = lista vazia (ler nunca derruba quem chama).
fn linhas<T>(dados: &Path, sql: &str, args: &[&dyn rusqlite::ToSql], f: impl Fn(&rusqlite::Row) -> rusqlite::Result<T>) -> Vec<T> {
    let Ok(c) = db(dados) else { return Vec::new() };
    let Ok(mut st) = c.prepare(sql) else { return Vec::new() };
    let r = st.query_map(args, |r| f(r)).map(|it| it.flatten().collect()).unwrap_or_default();
    r
}

fn exec(dados: &Path, sql: &str, args: &[&dyn rusqlite::ToSql]) -> Result<usize, String> {
    db(dados)?.execute(sql, args).map_err(|e| e.to_string())
}

// ---------- mensagens ----------

/// direcao: "E" = a pessoa falou; "C" = nos falamos. A mesma mensagem nas ultimas 6 h nao entra de novo
/// (a mesma conversa e lida varias vezes). Ok(false) = vazia ou repetida.
pub fn registrar_msg(dados: &Path, contato: &str, direcao: &str, texto: &str, canal: &str, audio: bool, ts: f64) -> Result<bool, String> {
    let texto = texto.trim();
    if texto.is_empty() {
        return Ok(false);
    }
    let ja = linhas(dados, "SELECT 1 FROM mensagens WHERE contato=? AND direcao=? AND texto=? AND ts>? LIMIT 1", params![contato, direcao, texto, ts - 6.0 * 3600.0], |r| r.get::<_, i64>(0));
    if !ja.is_empty() {
        return Ok(false);
    }
    exec(dados, "INSERT INTO mensagens(contato,direcao,canal,texto,audio,ts) VALUES(?,?,?,?,?,?)", params![contato, direcao, canal, texto, audio as i64, ts])?;
    Ok(true)
}

/// Ultimas n mensagens da pessoa, da mais antiga para a mais nova: (direcao, texto).
pub fn historico(dados: &Path, contato: &str, n: usize) -> Vec<(String, String)> {
    let mut v = linhas(dados, "SELECT direcao, texto FROM mensagens WHERE contato=? ORDER BY ts DESC, id DESC LIMIT ?", params![contato, n as i64], |r| Ok((r.get(0)?, r.get(1)?)));
    v.reverse();
    v
}

/// O que a PESSOA mandou depois de `ts`, em ordem: (ts, texto).
pub fn mensagens_desde(dados: &Path, contato: &str, ts: f64) -> Vec<(f64, String)> {
    linhas(dados, "SELECT ts, texto FROM mensagens WHERE contato=? AND direcao='E' AND ts>? ORDER BY ts, id", params![contato, ts], |r| Ok((r.get(0)?, r.get(1)?)))
}

/// O que NOS dissemos a esta pessoa nas ultimas horas (para nao repetir).
pub fn saidas_recentes(dados: &Path, contato: &str, horas: f64, ts: f64) -> Vec<String> {
    linhas(dados, "SELECT texto FROM mensagens WHERE contato=? AND direcao='C' AND ts>? ORDER BY ts", params![contato, ts - horas * 3600.0], |r| r.get(0))
}

/// True se ja mandamos alguma mensagem a esta pessoa (senao e contato NOVO).
pub fn ja_falamos(dados: &Path, contato: &str) -> bool {
    !linhas(dados, "SELECT 1 FROM mensagens WHERE contato=? AND direcao='C' LIMIT 1", params![contato], |r| r.get::<_, i64>(0)).is_empty()
}

// ---------- conhecimento (cache de perguntas ja respondidas) ----------

fn chave(pergunta: &str) -> String {
    palavras(pergunta).into_iter().collect::<Vec<_>>().join(" ")
}

/// Grava a resposta para nunca mais perguntar a mesma coisa. Pergunta so de palavras vazias ou resposta vazia: nao grava.
pub fn guardar_resposta(dados: &Path, pergunta: &str, resposta: &str, fonte: &str, contato: Option<&str>, ts: f64) -> Result<(), String> {
    let k = chave(pergunta);
    if k.is_empty() || resposta.trim().is_empty() {
        return Ok(());
    }
    exec(dados, "INSERT INTO conhecimento(chave,pergunta,resposta,fonte,contato,criado,usos,ultimo_uso) VALUES(?,?,?,?,?,?,0,?) \
                 ON CONFLICT(chave) DO UPDATE SET resposta=excluded.resposta, fonte=excluded.fonte, pergunta=excluded.pergunta",
        params![k, corta(pergunta, 300), resposta.trim(), fonte, contato, ts, ts]).map(|_| ())
}

/// Resposta ja conhecida para esta pergunta (igual ou muito parecida: Jaccard das palavras >= limiar; padrao do Python 0.8). Conta o uso.
pub fn buscar_resposta(dados: &Path, pergunta: &str, limiar: f64, ts: f64) -> Option<String> {
    let alvo = palavras(pergunta);
    if alvo.is_empty() {
        return None;
    }
    let mut melhor: Option<(i64, String)> = None;
    let mut melhor_s = 0.0;
    for (id, ch, resp) in linhas(dados, "SELECT id, chave, resposta FROM conhecimento ORDER BY id", params![], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))) {
        let outro: BTreeSet<String> = ch.split_whitespace().map(String::from).collect();
        if outro.is_empty() {
            continue;
        }
        let s = alvo.intersection(&outro).count() as f64 / alvo.union(&outro).count() as f64;
        if s > melhor_s {
            melhor = Some((id, resp));
            melhor_s = s;
        }
    }
    let (id, resp) = melhor.filter(|_| melhor_s >= limiar)?;
    let _ = exec(dados, "UPDATE conhecimento SET usos=usos+1, ultimo_uso=? WHERE id=?", params![ts, id]);
    Some(resp)
}

/// Apaga do conhecimento o que mencionar `termo` (algo aprendido errado). Devolve quantos apagou.
pub fn esquecer(dados: &Path, termo: &str) -> usize {
    let alvo = norm(termo);
    if alvo.is_empty() {
        return 0;
    }
    let rows = linhas(dados, "SELECT id, chave, pergunta, resposta FROM conhecimento", params![], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?)));
    rows.into_iter().filter(|(_, ch, p, r)| norm(p).contains(&alvo) || norm(r).contains(&alvo) || ch.contains(&alvo))
        .filter(|(id, ..)| exec(dados, "DELETE FROM conhecimento WHERE id=?", params![id]).is_ok()).count()
}

// ---------- decisoes (para nao perguntar de novo a mesma situacao) ----------

pub fn registrar_decisao(dados: &Path, contato: &str, entrada: &str, cod: &str, texto: &str, ts: f64) -> Result<(), String> {
    exec(dados, "INSERT INTO decisoes(contato,entrada,entrada_norm,cod,texto,ts) VALUES(?,?,?,?,?,?)",
        params![contato, corta(entrada, 300), norm(entrada), cod, corta(texto, 500), ts]).map(|_| ())
}

/// Mesma pessoa mandando EXATAMENTE a mesma mensagem ja decidida com um dos `cods` (padrao do Python: so "I", ignorar) nos ultimos `dias`.
/// So serve para decisoes seguras (ignorar/avisar): quem chama NUNCA passa "E" (uma resposta nao se reaproveita para situacao nova).
pub fn decisao_repetida(dados: &Path, contato: &str, entrada: &str, cods: &[&str], dias: f64, ts: f64) -> Option<String> {
    let n = norm(entrada);
    if n.is_empty() {
        return None;
    }
    let (id, cod) = linhas(dados, "SELECT id, cod FROM decisoes WHERE contato=? AND entrada_norm=? AND ts>? ORDER BY ts DESC, id DESC", params![contato, n, ts - dias * 86400.0],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))).into_iter().find(|(_, c)| cods.contains(&c.as_str()))?;
    let _ = exec(dados, "UPDATE decisoes SET usos=usos+1 WHERE id=?", params![id]);
    Some(cod)
}

/// Remove decisoes de uma pessoa (ex.: decisao errada que nao deve virar regra). Devolve quantas apagou.
pub fn apagar_decisoes(dados: &Path, contato: &str, cod: Option<&str>) -> usize {
    match cod {
        Some(c) => exec(dados, "DELETE FROM decisoes WHERE contato=? AND cod=?", params![contato, c]),
        None => exec(dados, "DELETE FROM decisoes WHERE contato=?", params![contato]),
    }.unwrap_or(0)
}

// ---------- fatos sobre cada pessoa ----------

pub fn aprender_fato(dados: &Path, contato: &str, fato: &str, fonte: &str, ts: f64) -> Result<(), String> {
    let fato = fato.trim();
    if fato.is_empty() {
        return Ok(());
    }
    exec(dados, "INSERT OR IGNORE INTO fatos_contato(contato,fato,fonte,ts) VALUES(?,?,?,?)", params![contato, corta(fato, 300), fonte, ts]).map(|_| ())
}

pub fn fatos(dados: &Path, contato: &str) -> Vec<String> {
    linhas(dados, "SELECT fato FROM fatos_contato WHERE contato=? ORDER BY ts, id", params![contato], |r| r.get(0))
}

// ---------- ficha ----------

/// Resumo compacto da pessoa: fatos aprendidos, ultimas decisoes e ultimas mensagens.
pub fn ficha(dados: &Path, contato: &str, n_msgs: usize) -> String {
    let mut partes = Vec::new();
    let f = fatos(dados, contato);
    if !f.is_empty() {
        partes.push(format!("FATOS: {}", f[f.len().saturating_sub(6)..].join(" | ")));
    }
    let mut dec = linhas(dados, "SELECT cod, entrada, texto FROM decisoes WHERE contato=? ORDER BY ts DESC, id DESC LIMIT 3", params![contato],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)));
    dec.reverse();
    if !dec.is_empty() {
        let d: Vec<String> = dec.iter().map(|(c, e, t)| format!("{c} ('{}'->'{}')", corta(e, 40), corta(t, 50))).collect();
        partes.push(format!("DECISOES RECENTES: {}", d.join(" | ")));
    }
    let h = historico(dados, contato, n_msgs);
    if !h.is_empty() {
        let m: Vec<String> = h.iter().map(|(d, t)| format!("{}{}", if d == "E" { "ELE: " } else { "NOS: " }, corta(t, 90))).collect();
        partes.push(format!("ULTIMAS MENSAGENS: {}", m.join(" ¦ ")));
    }
    partes.join("\n")
}

// ---------- correcoes ----------

/// O dono corrige uma vez e a correcao volta como exemplo sempre que o assunto reaparecer. False = faltou um dos lados.
pub fn corrigir(dados: &Path, errado: &str, certo: &str, ts: f64) -> bool {
    let (e, c) = (corta(errado.trim(), 300), corta(certo.trim(), 300));
    !e.is_empty() && !c.is_empty() && exec(dados, "INSERT OR REPLACE INTO correcoes(errado,certo,ts) VALUES(?,?,?)", params![e, c, ts]).is_ok()
}

/// Correcoes cujo "errado" divide palavras com o texto (ao menos 2, ou todas se forem poucas), mais recentes primeiro.
pub fn correcoes_parecidas(dados: &Path, texto: &str, n: usize) -> Vec<String> {
    let pal = palavras(texto);
    linhas(dados, "SELECT errado, certo FROM correcoes ORDER BY ts DESC, id DESC LIMIT 200", params![], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .into_iter().filter(|(e, _)| {
            let pe = palavras(e);
            !pe.is_empty() && pe.intersection(&pal).count() >= pe.len().min(2)
        }).take(n).map(|(e, c)| format!("{e} -> {c}")).collect()
}

/// Bloco para decidir sem perguntar: ficha da pessoa + resposta ja conhecida + regras do gerente (`regras`, vem de quem chama) + correcoes.
pub fn contexto_para_decidir(dados: &Path, contato: &str, entrada: &str, n: usize, regras: &str, ts: f64) -> String {
    let mut blocos = vec![ficha(dados, contato, n)];
    if let Some(c) = buscar_resposta(dados, entrada, 0.5, ts) {
        blocos.push(format!("JA RESPONDIDO ANTES (consistencia): {}", corta(&c, 300)));
    }
    if !regras.trim().is_empty() {
        blocos.push(format!("REGRAS DO CLAUDINHO (gerente; obedeca): {}", regras.replace("- ", "").lines().collect::<Vec<_>>().join(" | ")));
    }
    let cor = correcoes_parecidas(dados, entrada, 3);
    if !cor.is_empty() {
        blocos.push(format!("CORRECOES DO DONO (obedeca): {}", cor.join(" | ")));
    }
    blocos.retain(|b| !b.is_empty());
    blocos.join("\n")
}

/// (mensagens, conhecimento, decisoes, fatos).
pub fn resumo(dados: &Path) -> (i64, i64, i64, i64) {
    let c = |t: &str| linhas(dados, &format!("SELECT COUNT(*) FROM {t}"), params![], |r| r.get::<_, i64>(0)).first().copied().unwrap_or(0);
    (c("mensagens"), c("conhecimento"), c("decisoes"), c("fatos_contato"))
}

/// Resumo REAL (do banco local, sem IA) do que rolou nas ultimas horas: por conversa, quantas mensagens e as ultimas falas.
/// `hora_de(ts)` devolve "HH:MM" local (quem chama conhece o fuso).
pub fn resumo_periodo(dados: &Path, horas: f64, max_contatos: usize, max_linhas: usize, ts: f64, hora_de: &dyn Fn(f64) -> String) -> String {
    let rows = linhas(dados, "SELECT contato, direcao, texto, audio, ts FROM mensagens WHERE ts>? ORDER BY ts, id", params![ts - horas * 3600.0],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?, r.get::<_, f64>(4)?)));
    if rows.is_empty() {
        return format!("Não há nada registrado no WhatsApp nas últimas {horas} h.");
    }
    // por conversa, na ordem da ultima mensagem (mais recente primeiro)
    let mut por: Vec<(String, Vec<&(String, String, String, i64, f64)>)> = Vec::new();
    for r in &rows {
        match por.iter_mut().find(|(c, _)| *c == r.0) {
            Some((_, v)) => v.push(r),
            None => por.push((r.0.clone(), vec![r])),
        }
    }
    por.sort_by(|a, b| b.1[b.1.len() - 1].4.total_cmp(&a.1[a.1.len() - 1].4));
    let mut out = vec![format!("WhatsApp nas últimas {horas} h: {} mensagens em {} conversas.", rows.len(), por.len())];
    for (c, ms) in por.iter().take(max_contatos) {
        let eles = ms.iter().filter(|m| m.1 == "E").count();
        out.push(format!("\n• {c}: {eles} recebida(s), {} enviada(s)", ms.len() - eles));
        for m in &ms[ms.len().saturating_sub(max_linhas)..] {
            let t = corta(&m.2.split_whitespace().collect::<Vec<_>>().join(" "), 140);
            out.push(format!("   {} {}: {t}{}", hora_de(m.4), if m.1 == "E" { "ela/ele" } else { "nós" }, if m.3 != 0 { " 🎙" } else { "" }));
        }
    }
    if por.len() > max_contatos {
        out.push(format!("\n(+ {} conversas menos recentes)", por.len() - max_contatos));
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pasta(nome: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("braniac-memoria-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }
    const T: f64 = 1_800_000_000.0;
    fn par(a: &str, b: &str) -> (String, String) { (a.to_string(), b.to_string()) }

    #[test]
    fn mensagens_ficam_guardadas_sem_duplicar() {
        let p = pasta("msg");
        assert!(registrar_msg(&p, "Ana", "E", "tem vaga pra novembro?", "whatsapp", false, T).unwrap());
        assert!(!registrar_msg(&p, "Ana", "E", "tem vaga pra novembro?", "whatsapp", false, T + 60.0).unwrap()); // lida de novo: nao duplica
        assert!(!registrar_msg(&p, "Ana", "E", "   ", "whatsapp", false, T).unwrap());
        assert!(!ja_falamos(&p, "Ana"));
        registrar_msg(&p, "Ana", "C", "Tem sim, qual a data?", "whatsapp", false, T + 61.0).unwrap();
        assert_eq!(historico(&p, "Ana", 12), vec![par("E", "tem vaga pra novembro?"), par("C", "Tem sim, qual a data?")]);
        assert!(historico(&p, "Bia", 12).is_empty() && ja_falamos(&p, "Ana") && !ja_falamos(&p, "Bia"));
        // passadas 6 h a mesma frase e mensagem nova
        assert!(registrar_msg(&p, "Ana", "E", "tem vaga pra novembro?", "whatsapp", true, T + 7.0 * 3600.0).unwrap());
        assert_eq!(mensagens_desde(&p, "Ana", T + 10.0), vec![(T + 7.0 * 3600.0, "tem vaga pra novembro?".to_string())]);
        assert_eq!(saidas_recentes(&p, "Ana", 24.0, T + 3600.0), vec!["Tem sim, qual a data?"]);
        assert_eq!(historico(&p, "Ana", 1), vec![par("E", "tem vaga pra novembro?")]);
        let r = resumo_periodo(&p, 24.0, 12, 3, T + 8.0 * 3600.0, &|_| "10:00".into());
        assert!(r.contains("3 mensagens em 1 conversas") && r.contains("• Ana: 2 recebida(s), 1 enviada(s)") && r.contains("10:00 nós: Tem sim") && r.contains("🎙"));
        assert!(resumo_periodo(&p, 1.0, 12, 3, T + 90.0 * 3600.0, &|_| String::new()).starts_with("Não há nada"));
        assert_eq!(resumo(&p).0, 3);
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn resposta_conhecida_e_reaproveitada_com_outras_palavras_e_esquecer_apaga() {
        let p = pasta("resp");
        guardar_resposta(&p, "Qual é o preço da diária do Studio?", "Varia por data; confirmo e aviso.", "claude", None, T).unwrap();
        assert_eq!(buscar_resposta(&p, "qual o preco da diaria do studio", 0.8, T).as_deref(), Some("Varia por data; confirmo e aviso."));
        assert_eq!(buscar_resposta(&p, "quem é o Zeca?", 0.8, T), None);
        assert_eq!(buscar_resposta(&p, "o que e isso", 0.8, T), None); // so palavras vazias
        guardar_resposta(&p, "quem é o Zeca?", "Zeca é primo", "claude", None, T).unwrap();
        guardar_resposta(&p, "quem é o Zeca?", "", "claude", None, T).unwrap(); // resposta vazia nao apaga a boa
        assert_eq!(buscar_resposta(&p, "Quem é o Zeca?", 0.8, T).as_deref(), Some("Zeca é primo"));
        assert_eq!(esquecer(&p, "zeca"), 1);
        assert_eq!((buscar_resposta(&p, "quem é o Zeca?", 0.8, T), esquecer(&p, "zzz"), esquecer(&p, " ")), (None, 0, 0));
        assert_eq!(resumo(&p).1, 1);
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn decisao_repetida_so_reaproveita_ignorar_e_avisar_nunca_resposta() {
        let p = pasta("dec");
        registrar_decisao(&p, "Ana", "Bom dia, gente!", "I", "", T).unwrap();
        registrar_decisao(&p, "Ana", "tem vaga?", "E", "Tem sim!", T).unwrap();
        assert_eq!(decisao_repetida(&p, "Ana", "bom dia gente", &["I"], 30.0, T).as_deref(), Some("I")); // mesma mensagem ja ignorada
        assert_eq!(decisao_repetida(&p, "Ana", "tem vaga?", &["I"], 30.0, T), None); // uma RESPOSTA nao e reaproveitada
        assert_eq!(decisao_repetida(&p, "Bia", "bom dia gente", &["I"], 30.0, T), None); // so para a mesma pessoa
        assert_eq!(decisao_repetida(&p, "Ana", "bom dia gente", &["I"], 30.0, T + 31.0 * 86400.0), None); // venceu
        assert_eq!(decisao_repetida(&p, "Ana", "!!!", &["I"], 30.0, T), None);
        registrar_decisao(&p, "Ana", "preciso falar urgente", "A", "avisar", T).unwrap();
        assert_eq!(decisao_repetida(&p, "Ana", "preciso falar urgente", &["A"], 1.0, T).as_deref(), Some("A"));
        assert_eq!((apagar_decisoes(&p, "Ana", Some("I")), apagar_decisoes(&p, "Ana", None), resumo(&p).2), (1, 2, 0));
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn ficha_junta_fatos_decisoes_mensagens_e_o_contexto_leva_correcao() {
        let p = pasta("ficha");
        assert_eq!(ficha(&p, "Ana", 6), "");
        aprender_fato(&p, "Ana", "prefere áudio", "claude", T).unwrap();
        aprender_fato(&p, "Ana", "prefere áudio", "claude", T + 1.0).unwrap(); // repetido nao duplica
        registrar_decisao(&p, "Ana", "tem vaga?", "E", "Tem sim!", T).unwrap();
        registrar_msg(&p, "Ana", "E", "tem vaga?", "whatsapp", false, T).unwrap();
        let f = ficha(&p, "Ana", 6);
        assert!(f.contains("FATOS: prefere áudio\n") && f.contains("DECISOES RECENTES: E ('tem vaga?'->'Tem sim!')") && f.contains("ELE: tem vaga?"), "{f}");
        assert_eq!(fatos(&p, "Ana").len(), 1);
        assert!(corrigir(&p, "horario da esfiha", "abre as 18h, nao 17h", T) && !corrigir(&p, " ", "x", T));
        assert_eq!(correcoes_parecidas(&p, "qual o horario da esfiha hoje?", 3), vec!["horario da esfiha -> abre as 18h, nao 17h"]);
        assert!(correcoes_parecidas(&p, "sobre viagem", 3).is_empty());
        guardar_resposta(&p, "qual o horario da esfiha", "Abre 18h.", "dono", None, T).unwrap();
        let c = contexto_para_decidir(&p, "Ana", "qual o horario da esfiha hoje?", 6, "- nao se reapresente\n- seja curto", T);
        assert!(c.contains("JA RESPONDIDO ANTES (consistencia): Abre 18h.") && c.contains("REGRAS DO CLAUDINHO (gerente; obedeca): nao se reapresente | seja curto")
            && c.contains("CORRECOES DO DONO (obedeca): horario da esfiha -> abre") && c.starts_with("FATOS:"), "{c}");
        assert!(!contexto_para_decidir(&p, "Bia", "oi", 6, "", T).contains("REGRAS"));
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn norma_e_palavras() {
        assert_eq!(norm("  Bom dia,   GENTE! Ação nº 5 "), "bom dia gente acao n 5");
        assert_eq!(palavras("Qual é o preço da diária?").into_iter().collect::<Vec<_>>(), vec!["diaria", "preco"]);
    }
}
