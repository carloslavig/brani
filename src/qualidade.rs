//! Mede quantas respostas do Jarvis o dono corrigiu (porte de braniac/qualidade.py). Local, sem IA: conta correcoes explicitas
//! ("nao foi isso que perguntei", "nada a ver", "errou") na mensagem seguinte a uma resposta. So LE o banco de eventos.
//! Diferenca do Python: o nome do dono (comeco do contato no banco) entra como parametro.
#![allow(dead_code)]
use regex::Regex;
use std::path::Path;
use std::sync::OnceLock;

pub fn correcao(t: &str) -> bool {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)nada a ver|n[ãa]o,? (eu|era|é|foi|entendeu)|errou|seu cu|de novo|n[ãa]o entendeu|n[ãa]o foi isso|pra que desgra|viajou|n[ãa]o perguntei|n[ãa]o pedi").expect("regex fixa"))
        .is_match(t)
}

fn corta(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// (trocas, [(fala que corrige, texto anterior)]) a partir das linhas (direcao, texto) em ordem de tempo.
/// Mesma regra do Python: "C" abre uma troca; um "E" curto (< 200) com correcao logo depois conta um erro, uma vez por troca.
pub fn contar(linhas: &[(String, String)]) -> (usize, Vec<(String, String)>) {
    let (mut ant, mut trocas, mut erros): (Option<String>, usize, Vec<(String, String)>) = (None, 0, Vec::new());
    for (d, t) in linhas {
        // o contexto anexado a mensagem nao e fala da pessoa
        let t = t.split("\n\n[Conversa recente").next().unwrap_or("").split("\n\n[Projeto").next().unwrap_or("");
        if d == "C" {
            ant = Some(t.to_string());
            trocas += 1;
        } else if d == "E" && ant.as_deref().is_some_and(|a| !a.is_empty()) && correcao(t) && t.chars().count() < 200 {
            erros.push((corta(t, 90), corta(&ant.take().unwrap_or_default(), 110)));
        }
    }
    (trocas, erros)
}

/// (trocas, correcoes, exemplos) dos ultimos `dias` no banco `db` (tabela eventos). Banco ausente ou sem a tabela = zeros.
pub fn medir(db: &Path, dono: &str, dias: f64, ts: f64) -> (usize, usize, Vec<(String, String)>) {
    let ler = || -> rusqlite::Result<Vec<(String, String)>> {
        let c = rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let mut s = c.prepare("SELECT direcao, texto FROM eventos WHERE ts>? AND contato LIKE ? AND canal IN ('whatsapp','conversa-jarvis') ORDER BY ts")?;
        let r = s.query_map(rusqlite::params![ts - dias * 86400.0, format!("{dono}%")], |r| Ok((r.get(0)?, r.get(1)?)))?.collect();
        r
    };
    let (trocas, erros) = contar(&ler().unwrap_or_default());
    (trocas, erros.len(), erros)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regex_pega_correcoes_e_ignora_conversa_normal() {
        for t in ["Não foi isso que eu perguntei", "nada a ver", "NÃO, era o outro", "errou", "você viajou", "não pedi isso"] {
            assert!(correcao(t), "{t}");
        }
        assert!(!correcao("ok, obrigado, pode mandar") && !correcao("não sei, pode ser"));
    }

    #[test]
    fn conta_uma_correcao_por_troca_e_mede_do_banco() {
        let l = |d: &str, t: &str| (d.to_string(), t.to_string());
        let longa = format!("errou {}", "x".repeat(200));
        let linhas = vec![l("E", "errou"), // sem resposta antes: nao conta
            l("C", "resposta um\n\n[Conversa recente: nada a ver]"), l("E", "nada a ver"), l("E", "errou de novo"), // so a primeira conta
            l("C", "resposta dois"), l("E", "valeu"), l("E", &longa),
            l("C", "resposta tres"), l("E", "ok\n\n[Projeto X: errou]")];
        let (trocas, erros) = contar(&linhas);
        assert_eq!((trocas, erros), (3, vec![l("nada a ver", "resposta um")]));

        let p = std::env::temp_dir().join(format!("braniac-qualidade-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        let db = p.join("jarvis.db");
        assert_eq!(medir(&db, "Dono", 1.0, 1000.0), (0, 0, vec![])); // sem banco nao quebra
        {
            let c = rusqlite::Connection::open(&db).unwrap();
            c.execute_batch("CREATE TABLE eventos(ts REAL, canal TEXT, contato TEXT, direcao TEXT, texto TEXT);
                INSERT INTO eventos VALUES (10, 'whatsapp', 'Dono (eu)', 'C', 'velha'), (11, 'whatsapp', 'Dono (eu)', 'E', 'errou'),
                    (200000, 'whatsapp', 'Dono (eu)', 'C', 'resposta'), (200001, 'whatsapp', 'Dono (eu)', 'E', 'não foi isso'),
                    (200002, 'whatsapp', 'Bia', 'C', 'oi'), (200003, 'whatsapp', 'Bia', 'E', 'errou'),
                    (200004, 'telegram', 'Dono', 'C', 'x'), (200005, 'telegram', 'Dono', 'E', 'errou');").unwrap();
        }
        assert_eq!(medir(&db, "Dono", 1.0, 200010.0), (1, 1, vec![l("não foi isso", "resposta")]));
        assert_eq!(medir(&db, "Dono", 14.0, 200010.0).1, 2);
        let _ = std::fs::remove_dir_all(&p);
    }
}
