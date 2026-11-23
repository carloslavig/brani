//! Banco unificado: fatos e eventos em duas camadas. Porte do nucleo de braniac/banco.py
//! (ainda sem sincronia com outras fontes e sem os bancos de assunto).
//! Regra central: IA "online" so enxerga nivel 'publico'; "local" e "claude" enxergam tudo.
//! Rascunho do Otto (NexoDev, T060, veio cortado); fechado e ajustado pelo Claude.
use rusqlite::{params, Connection};
use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const PARADAS: &str = " a o as os um uma uns umas de do da dos das em no na nos nas por para pra pro com sem e ou que se eu voce vc tu ele ela \
nao sim ja mais mas so tambem eh sao foi ser esta estao tem ter como qual quais quem onde quando isso isto esse essa \
aquele aquela meu minha seu sua nosso me te lhe ao aos jarvis ai fala fale diz diga conta ";

pub struct Banco {
    con: Mutex<Connection>,
}

fn agora() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

fn corta(s: &str, n: usize) -> String {
    s.chars().take(n).collect() // por caractere: corte por byte quebra no meio de acento
}

fn sem_acento(c: char) -> char {
    match c {
        'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
        'é' | 'ê' | 'è' | 'ë' => 'e',
        'í' | 'ì' | 'î' | 'ï' => 'i',
        'ó' | 'ô' | 'õ' | 'ò' | 'ö' => 'o',
        'ú' | 'ù' | 'û' | 'ü' => 'u',
        'ç' => 'c',
        outro => outro,
    }
}

/// Palavras uteis da busca: minusculas, sem acento, 3+ letras, fora as palavras vazias.
pub fn palavras(texto: &str) -> Vec<String> {
    let limpo: String = texto.to_lowercase().chars().map(sem_acento).map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect();
    limpo.split_whitespace().filter(|w| w.chars().count() >= 3 && !PARADAS.contains(&format!(" {w} "))).map(String::from).collect()
}

/// Quantos radicais (5 letras) da busca aparecem no texto: 'video' casa com 'VIDEOS'.
pub fn pontua(texto: &str, pal: &[String]) -> usize {
    let no_texto: HashSet<String> = palavras(texto).iter().map(|w| corta(w, 5)).collect();
    pal.iter().map(|w| corta(w, 5)).collect::<HashSet<_>>().intersection(&no_texto).count()
}

pub fn camada_completa(tipo: &str) -> bool {
    !(tipo.is_empty() || tipo == "online")
}

impl Banco {
    pub fn abrir(caminho: &Path) -> Result<Banco, String> {
        Self::preparar(Connection::open(caminho).map_err(|e| e.to_string())?)
    }

    #[cfg(test)]
    pub fn em_memoria() -> Result<Banco, String> {
        Self::preparar(Connection::open_in_memory().map_err(|e| e.to_string())?)
    }

    fn preparar(con: Connection) -> Result<Banco, String> {
        con.execute_batch(
            "CREATE TABLE IF NOT EXISTS fatos(
                id INTEGER PRIMARY KEY, escopo TEXT, chave TEXT, texto TEXT, nivel TEXT DEFAULT 'local', fonte TEXT, ts REAL,
                validade REAL, UNIQUE(escopo, chave));
             CREATE TABLE IF NOT EXISTS eventos(
                id INTEGER PRIMARY KEY, origem TEXT UNIQUE, ts REAL, canal TEXT, contato TEXT, direcao TEXT, texto TEXT,
                nivel TEXT DEFAULT 'local');
             CREATE INDEX IF NOT EXISTS ev_ts ON eventos(ts);
             CREATE INDEX IF NOT EXISTS ev_contato ON eventos(contato);",
        ).map_err(|e| e.to_string())?;
        Ok(Banco { con: Mutex::new(con) })
    }

    /// Cria/atualiza um fato. Qualquer nivel que nao seja "publico" vira "local" (na duvida, nao sai do PC).
    pub fn gravar_fato(&self, escopo: &str, chave: &str, texto: &str, nivel: &str, fonte: &str, validade: Option<f64>) -> bool {
        let texto = texto.trim();
        let Ok(con) = self.con.lock() else { return false };
        if texto.is_empty() {
            return false;
        }
        let nivel = if nivel == "publico" { "publico" } else { "local" };
        // so atualiza quando mudou: regravar o mesmo fato nao "rejuvenesce" o ts
        con.execute(
            "INSERT INTO fatos(escopo,chave,texto,nivel,fonte,ts,validade) VALUES(?,?,?,?,?,?,?)
             ON CONFLICT(escopo,chave) DO UPDATE SET texto=excluded.texto, nivel=excluded.nivel, fonte=excluded.fonte,
                ts=excluded.ts, validade=excluded.validade
             WHERE fatos.texto != excluded.texto OR fatos.nivel != excluded.nivel",
            params![escopo, corta(chave, 120), texto, nivel, fonte, agora(), validade],
        ).is_ok()
    }

    /// Origem repetida nao duplica (devolve false).
    #[allow(clippy::too_many_arguments, dead_code)] // usado quando os canais (WhatsApp, Telegram) forem portados
    pub fn registrar_evento(&self, canal: &str, contato: &str, direcao: &str, texto: &str, nivel: &str, ts: Option<f64>, origem: Option<&str>) -> bool {
        let texto = texto.trim();
        let Ok(con) = self.con.lock() else { return false };
        if texto.is_empty() {
            return false;
        }
        let nivel = if nivel == "publico" { "publico" } else { "local" };
        con.execute(
            "INSERT OR IGNORE INTO eventos(origem,ts,canal,contato,direcao,texto,nivel) VALUES(?,?,?,?,?,?,?)",
            params![origem, ts.unwrap_or_else(agora), canal, contato, direcao, corta(texto, 2000), nivel],
        ).is_ok_and(|n| n > 0)
    }

    /// (texto para o prompt, se entrou algo da camada local). `tipo` = tipo do provedor que vai receber.
    pub fn consultar(&self, busca: &str, tipo: &str, horas: Option<f64>, n: usize) -> (String, bool) {
        let nada = ("Nada registrado no banco sobre isso (nao invente: diga que nao tem registro).".to_string(), false);
        let Ok(con) = self.con.lock() else { return nada };
        let completo = camada_completa(tipo);
        let pal = palavras(busca);
        let (mut linhas, mut usou_local) = (Vec::new(), false);

        let filtro = if completo { "" } else { " AND nivel='publico'" };
        let sql = format!("SELECT escopo, texto, nivel FROM fatos WHERE (validade IS NULL OR validade>?){filtro}");
        let mut fatos: Vec<(usize, String, String, String)> = con.prepare(&sql).and_then(|mut s| {
            s.query_map(params![agora()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))
                .map(|linhas| linhas.filter_map(Result::ok).map(|(e, t, nv)| (pontua(&t, &pal) * 2 + pontua(&e, &pal), e, t, nv)).collect())
        }).unwrap_or_default();
        fatos.sort_by(|a, b| b.0.cmp(&a.0));
        for (p, e, t, nv) in fatos.into_iter().take(n) {
            if p > 0 || pal.is_empty() {
                linhas.push(format!("[{e}] {}", corta(&t, 420)));
                usou_local |= nv == "local";
            }
        }

        if completo {
            let desde = agora() - horas.unwrap_or(if pal.is_empty() { 24.0 } else { 24.0 * 14.0 }) * 3600.0;
            // (pontos, ts, linha pronta), do mais novo para o mais antigo
            let evs: Vec<(usize, f64, String)> = con.prepare("SELECT ts, contato, direcao, texto, canal FROM eventos WHERE ts>? ORDER BY ts DESC LIMIT 600").and_then(|mut s| {
                s.query_map(params![desde], |r| Ok((r.get::<_, f64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?)))
                    .map(|linhas| linhas.filter_map(Result::ok).map(|(ts, c, d, t, canal)| {
                        let quem = if d == "E" { c.clone() } else { format!("Jarvis→{c}") };
                        let junto = t.split_whitespace().collect::<Vec<_>>().join(" ");
                        (pontua(&format!("{c} {t}"), &pal), ts, format!("[{canal}] {quem}: {}", corta(&junto, 220)))
                    }).collect())
            }).unwrap_or_default();
            let mut escolhidos: Vec<(usize, f64, String)> = if pal.is_empty() {
                evs.into_iter().take(n).collect()
            } else {
                let mut com_ponto: Vec<_> = evs.into_iter().filter(|e| e.0 > 0).collect();
                com_ponto.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.total_cmp(&a.1)));
                com_ponto.truncate(n);
                com_ponto
            };
            escolhidos.sort_by(|a, b| a.1.total_cmp(&b.1)); // em ordem de tempo
            if !escolhidos.is_empty() {
                let textos: Vec<String> = escolhidos.into_iter().map(|e| e.2).collect();
                linhas.push(format!("MENSAGENS/EVENTOS REGISTRADOS:\n{}", textos.join("\n")));
                usou_local = true;
            }
        }
        if linhas.is_empty() { nada } else { (linhas.join("\n"), usou_local) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn banco() -> Banco {
        let b = Banco::em_memoria().unwrap();
        assert!(b.gravar_fato("negocio", "preco", "A página custa 150", "publico", "t", None));
        assert!(b.gravar_fato("pessoa", "maria", "Maria gosta de VIDEOS curtos", "local", "t", None));
        assert!(b.registrar_evento("whatsapp", "Maria", "E", "manda o video hoje", "local", None, Some("m1")));
        b
    }

    #[test]
    fn online_so_ve_o_publico() {
        let b = banco();
        let (t, local) = b.consultar("", "online", None, 8);
        assert!(t.contains("150") && !t.contains("Maria") && !t.contains("MENSAGENS") && !local);
        let (t, _) = b.consultar("video", "online", None, 8);
        assert!(t.starts_with("Nada registrado"));
        assert!(b.consultar("video", "", None, 8).0.starts_with("Nada registrado"));
    }

    #[test]
    fn local_ve_tudo_e_radical_casa() {
        let (t, local) = banco().consultar("video", "local", None, 8);
        assert!(t.contains("VIDEOS curtos") && t.contains("[whatsapp] Maria: manda o video hoje") && local);
        assert!(!t.contains("150"));
    }

    #[test]
    fn vencido_some_origem_repetida_nao_duplica_e_nivel_estranho_vira_local() {
        let b = banco();
        assert!(b.gravar_fato("hoje", "x", "reuniao so hoje", "publico", "t", Some(agora() - 10.0)));
        assert!(!b.consultar("reuniao", "local", None, 8).0.contains("reuniao"));
        assert!(!b.registrar_evento("whatsapp", "Maria", "E", "de novo", "local", None, Some("m1")));
        assert!(!b.registrar_evento("whatsapp", "Maria", "E", "  ", "local", None, None));
        assert!(b.gravar_fato("pessoa", "z", "segredo do zeca", "qualquer", "t", None));
        assert!(!b.consultar("zeca", "online", None, 8).0.contains("zeca"));
    }

    #[test]
    fn acento_nao_derruba_o_corte() {
        let b = Banco::em_memoria().unwrap();
        assert!(b.gravar_fato("e", &"é".repeat(300), &"ação ".repeat(200), "publico", "t", None));
        assert!(b.registrar_evento("c", "x", "S", &"ç".repeat(5000), "local", None, None));
        assert!(b.consultar("acao", "local", None, 8).0.contains("[e]"));
        assert_eq!(palavras("Não quero os VÍDEOS da ação"), vec!["quero", "videos", "acao"]);
    }
}
