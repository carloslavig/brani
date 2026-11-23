//! Organiza as conversas da aba Sessoes por assunto (braniac/organizar_sessoes.py). So MOVE conversas que ainda estao em "Geral"
//! para o projeto do assunto (cria o projeto se faltar). Nao apaga, nao arquiva, nao renomeia.
//! Trabalha sobre as tabelas do b2: projetos(id, nome, instrucoes, criado) e conversas(id, titulo, arquivada, projeto).
//! Diferenca do Python: as regras (projeto, palavras) vem de configuracao, porque as de la tem nome de pessoa e de cliente;
//! `REGRAS_GERAIS` traz so as palavras sem nome proprio.
#![allow(dead_code)]
use regex::Regex;
use rusqlite::Connection;

/// (projeto, palavras). A primeira regra que casar vence; projeto novo e criado com este nome.
pub const REGRAS_GERAIS: [(&str, &str); 3] = [
    ("🎬 Vídeos", r"v[ií]deo|esteira|reels?|cortes?\b"),
    ("🌐 Sites e clientes", r"\bsites?\b|landing|estoque|loja|vaquinha"),
    ("🧠 Braniac e ferramentas", r"braniac|jarvis|pr[eé]dio|agentes?\b|habilidade|skill|programar|conselho|claude|\bpc\b|notebook"),
];

/// Projeto do assunto do titulo, pela primeira regra que casar (sem diferenciar maiuscula). Regra com regex invalida e pulada.
pub fn projeto_de<'a>(titulo: &str, regras: &'a [(&'a str, &'a str)]) -> Option<&'a str> {
    regras.iter().find(|r| Regex::new(&format!("(?i){}", r.1)).is_ok_and(|x| x.is_match(titulo))).map(|r| r.0)
}

/// Devolve [(id, titulo, projeto novo)] do que moveu (ou moveria, se `aplicar` = false).
pub fn organizar(con: &Connection, regras: &[(&str, &str)], aplicar: bool) -> Result<Vec<(i64, String, String)>, String> {
    let e = |x: rusqlite::Error| x.to_string();
    let geral: i64 = match con.query_row("SELECT id FROM projetos WHERE nome='Geral'", [], |r| r.get(0)) {
        Ok(g) => g,
        Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(vec![]),
        Err(x) => return Err(e(x)),
    };
    let conversas: Vec<(i64, String)> = {
        let mut st = con.prepare("SELECT id, titulo FROM conversas WHERE arquivada=0 AND projeto=?1").map_err(e)?;
        let linhas = st.query_map([geral], |r| Ok((r.get(0)?, r.get::<_, Option<String>>(1)?.unwrap_or_default()))).map_err(e)?;
        linhas.collect::<Result<_, _>>().map_err(e)?
    };
    let mut feitas = vec![];
    for (cid, titulo) in conversas {
        let Some(nome) = projeto_de(&titulo, regras) else { continue };
        if aplicar {
            let existente = con.query_row("SELECT id FROM projetos WHERE nome=?1", [nome], |r| r.get::<_, i64>(0));
            let pid = match existente {
                Ok(p) => p,
                Err(rusqlite::Error::QueryReturnedNoRows) => {
                    con.execute("INSERT INTO projetos(nome, instrucoes, criado) VALUES(?1, '', strftime('%s','now'))", [nome]).map_err(e)?;
                    con.last_insert_rowid()
                }
                Err(x) => return Err(e(x)),
            };
            con.execute("UPDATE conversas SET projeto=?1 WHERE id=?2", [pid, cid]).map_err(e)?;
        }
        feitas.push((cid, titulo, nome.to_string()));
    }
    Ok(feitas)
}

pub fn resumo(con: &Connection, regras: &[(&str, &str)], aplicar: bool) -> Result<String, String> {
    let f = organizar(con, regras, aplicar)?;
    if f.is_empty() {
        return Ok("Nada a organizar: as conversas de Geral nao casam com nenhum assunto.".into());
    }
    let itens: Vec<String> = f.iter().map(|(_, t, p)| format!("'{}' -> {p}", t.chars().take(40).collect::<String>())).collect();
    Ok(format!("{}{} conversa(s): {}", if aplicar { "Movi " } else { "Moveria " }, f.len(), itens.join("; ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn banco() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE projetos(id INTEGER PRIMARY KEY, nome TEXT, instrucoes TEXT, criado INTEGER);
             CREATE TABLE conversas(id INTEGER PRIMARY KEY, titulo TEXT, arquivada INTEGER DEFAULT 0, projeto INTEGER);
             INSERT INTO projetos(id, nome, instrucoes, criado) VALUES (1, 'Geral', '', 0), (2, 'Outro', '', 0);
             INSERT INTO conversas(id, titulo, arquivada, projeto) VALUES (1, 'a ESTEIRA ta funcionando?', 0, 1), (2, 'Quem é você', 0, 1),
                (3, 'vídeo antigo', 1, 1), (4, 'site novo', 0, 2), (5, NULL, 0, 1), (6, 'landing do cliente', 0, 1), (7, 'corte de vídeo', 0, 1);",
        )
        .unwrap();
        c
    }

    fn projetos(c: &Connection) -> Vec<(i64, String)> {
        let mut st = c.prepare("SELECT c.id, p.nome FROM conversas c JOIN projetos p ON p.id = c.projeto ORDER BY c.id").unwrap();
        let v = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().flatten().collect();
        v
    }

    #[test]
    fn move_de_geral_para_o_assunto_sem_tocar_o_resto() {
        let c = banco();
        let antes = projetos(&c);
        assert_eq!(resumo(&c, &REGRAS_GERAIS, false).unwrap(), "Moveria 3 conversa(s): 'a ESTEIRA ta funcionando?' -> 🎬 Vídeos; 'landing do cliente' -> 🌐 Sites e clientes; 'corte de vídeo' -> 🎬 Vídeos");
        assert_eq!(projetos(&c), antes); // simular nao mexe
        assert_eq!(organizar(&c, &REGRAS_GERAIS, true).unwrap().len(), 3);
        let depois: Vec<String> = projetos(&c).into_iter().map(|p| p.1).collect();
        // arquivada (3), a de outro projeto (4) e as sem assunto (2, 5) ficam onde estavam
        assert_eq!(depois, ["🎬 Vídeos", "Geral", "Geral", "Outro", "Geral", "🌐 Sites e clientes", "🎬 Vídeos"]);
        assert_eq!(c.query_row("SELECT count(*) FROM projetos", [], |r| r.get::<_, i64>(0)).unwrap(), 4); // um projeto por assunto, sem duplicar
        assert_eq!(c.query_row("SELECT count(*) FROM conversas", [], |r| r.get::<_, i64>(0)).unwrap(), 7); // nada apagado
        assert!(organizar(&c, &REGRAS_GERAIS, true).unwrap().is_empty());
        assert_eq!(resumo(&c, &REGRAS_GERAIS, true).unwrap(), "Nada a organizar: as conversas de Geral nao casam com nenhum assunto.");
    }

    #[test]
    fn regras_vem_de_configuracao_e_sem_geral_nao_faz_nada() {
        let regras = [("Quebrada", "(("), ("📦 Clientes", "cliente|você")];
        assert_eq!(projeto_de("Quem é VOCÊ", &regras), Some("📦 Clientes"));
        assert_eq!(projeto_de("bom dia", &regras), None);
        let c = banco();
        c.execute("UPDATE projetos SET nome='Inicio' WHERE id=1", []).unwrap();
        assert!(organizar(&c, &regras, true).unwrap().is_empty());
        assert!(organizar(&Connection::open_in_memory().unwrap(), &regras, true).is_err()); // banco sem as tabelas
    }
}
