//! Importador do Braniac antigo (Python) para o B.R.A.N.I: `POST /api/importar-antigo` (corpo opcional `{"origem": "<pasta data do Python>"}`;
//! padrao = pasta `data` ao lado do `BRANIAC_ENV`). SO LEITURA no Python: os bancos dele sao anexados com `mode=ro`, o resto so e lido.
//! IDEMPOTENTE: linha de banco entra se nao existir igual (todas as colunas menos o id); arquivo JSON so entra se o daqui nao existe ou
//! esta vazio; arquivo de linhas (jsonl) so acrescenta as linhas que faltam. Rodar duas vezes nao duplica nada.
//! Traz: apresentados, memoria de conversas por contato (memoria.db), banco unificado (jarvis.db), bancos de assunto, historico do
//! Jarvis (braniac.db) e da sala, agenda e Compasso, estado da Alma, guia do Claudinho e erros, funil, candidaturas.
//! NAO traz (de proposito): vigia_estado/dono_estado/airbnb_estado/airbnb_escalada (fariam o B.R.A.N.I responder de novo ao antigo),
//! rotina.json (suprimiria o bom dia de hoje), pessoal.json (formatos diferentes; mesclar e do dono), agentes.db (recados velhos do
//! barramento), braincenter.db (WAL; ele refaz), b2.db. Devolve so CONTAGENS: nada de conteudo, nome ou mensagem.
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

fn origem_padrao() -> Option<PathBuf> {
    std::env::var_os("BRANIAC_ENV").map(|e| Path::new(&e).with_file_name("data"))
}

fn aspas(n: &str) -> String {
    format!("\"{}\"", n.replace('"', ""))
}

fn colunas(c: &Connection, esquema: &str, tabela: &str) -> Vec<String> {
    let Ok(mut st) = c.prepare(&format!("PRAGMA {esquema}.table_info({})", aspas(tabela))) else { return vec![] };
    st.query_map([], |r| r.get::<_, String>(1)).map(|i| i.flatten().collect()).unwrap_or_default()
}

/// Abre `destino` (cria se faltar) e anexa `fonte` como `py`, somente leitura. None = a fonte nao existe.
fn abrir(destino: &Path, fonte: &Path) -> Result<Option<Connection>, String> {
    if !fonte.is_file() {
        return Ok(None);
    }
    if let Some(p) = destino.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let c = Connection::open_with_flags(destino, OpenFlags::default() | OpenFlags::SQLITE_OPEN_URI).map_err(|e| e.to_string())?;
    let _ = c.busy_timeout(std::time::Duration::from_secs(15));
    let uri = format!("file:///{}?mode=ro", fonte.to_string_lossy().replace('\\', "/").trim_start_matches('/').replace(' ', "%20"));
    c.execute("ATTACH DATABASE ?1 AS py", [uri]).map_err(|e| format!("nao abri o banco do antigo: {e}"))?;
    Ok(Some(c))
}

/// Traz as linhas de `py.tabela` que ainda nao existem (igual em todas as colunas, sem o id). Cria a tabela com o DDL do antigo se faltar.
/// Devolve (linhas na fonte, linhas novas).
fn tabela(c: &Connection, nome: &str) -> (usize, usize) {
    let existe: bool = c.query_row("SELECT count(*) FROM py.sqlite_master WHERE type='table' AND name=?1", [nome], |r| r.get::<_, i64>(0)).unwrap_or(0) > 0;
    if !existe {
        return (0, 0);
    }
    if colunas(c, "main", nome).is_empty() {
        if let Ok(ddl) = c.query_row("SELECT sql FROM py.sqlite_master WHERE type='table' AND name=?1", [nome], |r| r.get::<_, String>(0)) {
            let _ = c.execute_batch(&ddl);
        }
    }
    let origem = colunas(c, "py", nome);
    let cols: Vec<String> = colunas(c, "main", nome).into_iter().filter(|x| x != "id" && origem.contains(x)).collect();
    let total: usize = c.query_row(&format!("SELECT count(*) FROM py.{}", aspas(nome)), [], |r| r.get::<_, i64>(0)).unwrap_or(0) as usize;
    if cols.is_empty() {
        return (total, 0);
    }
    let lista = cols.iter().map(|x| aspas(x)).collect::<Vec<_>>().join(",");
    let igual = cols.iter().map(|x| format!("m.{0} IS p.{0}", aspas(x))).collect::<Vec<_>>().join(" AND ");
    let sql = format!("INSERT OR IGNORE INTO main.{t}({lista}) SELECT {lista} FROM py.{t} p WHERE NOT EXISTS (SELECT 1 FROM main.{t} m WHERE {igual})", t = aspas(nome));
    let novos = c.execute(&sql, []).unwrap_or(0);
    (total, novos)
}

fn banco(dados: &Path, origem: &Path, arq: &str, tabelas: &[&str], rel: &mut Value, chave: &str) {
    match abrir(&dados.join(arq), &origem.join(arq)) {
        Ok(Some(c)) => {
            let mut r = json!({});
            for t in tabelas {
                let (a, n) = tabela(&c, t);
                r[*t] = json!({ "no_antigo": a, "novos": n });
            }
            rel[chave] = r;
        }
        Ok(None) => rel[chave] = json!("não existe no antigo"),
        Err(e) => rel[chave] = json!(format!("erro: {e}")),
    }
}

fn copiar_se_falta(src: &Path, dst: &Path) -> &'static str {
    if !src.is_file() {
        return "não existe no antigo";
    }
    if std::fs::read_to_string(dst).is_ok_and(|t| !t.trim().is_empty()) {
        return "já existe aqui (mantido)";
    }
    if let Some(p) = dst.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    if std::fs::copy(src, dst).is_ok() { "copiado" } else { "erro ao copiar" }
}

/// Acrescenta as linhas que faltam (sem repetir). Devolve (linhas no antigo, novas).
fn mesclar_linhas(src: &Path, dst: &Path) -> (usize, usize) {
    let Ok(t) = std::fs::read_to_string(src) else { return (0, 0) };
    let ja: HashSet<String> = std::fs::read_to_string(dst).unwrap_or_default().lines().map(|l| l.trim().to_string()).collect();
    let novas: Vec<&str> = t.lines().filter(|l| !l.trim().is_empty() && !ja.contains(l.trim())).collect();
    if !novas.is_empty() {
        use std::io::Write;
        let _ = std::fs::create_dir_all(dst.parent().unwrap_or(Path::new(".")));
        let falta_quebra = std::fs::read_to_string(dst).is_ok_and(|x| !x.is_empty() && !x.ends_with('\n'));
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dst) {
            if falta_quebra {
                let _ = writeln!(f);
            }
            for l in &novas {
                let _ = writeln!(f, "{l}");
            }
        }
    }
    (t.lines().filter(|l| !l.trim().is_empty()).count(), novas.len())
}

/// Lista JSON de objetos (ou {"<campo>": [...]}) mesclada pelo campo `id`.
fn mesclar_lista(src: &Path, dst: &Path, campo_lista: Option<&str>, id: &str) -> (usize, usize) {
    let ler = |p: &Path| std::fs::read_to_string(p).ok().and_then(|t| serde_json::from_str::<Value>(t.trim_start_matches('\u{feff}')).ok());
    let pega = |v: &Value| -> Vec<Value> { match campo_lista { Some(c) => v[c].as_array().cloned().unwrap_or_default(), None => v.as_array().cloned().unwrap_or_default() } };
    let Some(antigo) = ler(src).map(|v| pega(&v)) else { return (0, 0) };
    let mut atual = ler(dst).map(|v| pega(&v)).unwrap_or_default();
    let ids: HashSet<String> = atual.iter().filter_map(|x| x[id].as_str().map(String::from)).collect();
    let mut novos = 0;
    for x in &antigo {
        if x[id].as_str().is_some_and(|i| !ids.contains(i)) {
            atual.push(x.clone());
            novos += 1;
        }
    }
    if novos > 0 {
        let v = match campo_lista { Some(c) => json!({ c: atual }), None => json!(atual) };
        let _ = std::fs::create_dir_all(dst.parent().unwrap_or(Path::new(".")));
        let _ = std::fs::write(dst, serde_json::to_string_pretty(&v).unwrap_or_default());
    }
    (antigo.len(), novos)
}

pub fn importar(dados: &Path, origem: &Path) -> Value {
    if !origem.join("memoria.db").is_file() {
        return json!({ "ok": false, "erro": "a pasta de origem não parece o `data` do Braniac antigo (falta memoria.db)" });
    }
    let mut rel = json!({ "ok": true });
    // quem ja conhece o Jarvis: sem isto ele se reapresenta
    let antes = crate::contexto::Ctx::ler(dados);
    let novos = crate::wpp_laco::importar_apresentados(dados, &origem.join("contexto.json"));
    rel["apresentados"] = json!({ "novos": novos, "total_depois": crate::contexto::Ctx::ler(dados).apresentados.len(), "antes": antes.apresentados.len() });
    banco(dados, origem, "memoria.db", &["mensagens", "conhecimento", "decisoes", "fatos_contato", "correcoes"], &mut rel, "memoria");
    banco(dados, origem, "jarvis.db", &["fatos", "eventos"], &mut rel, "banco_unificado");
    banco(dados, origem, "braniac.db", &["msgs"], &mut rel, "historico_do_jarvis");
    banco(dados, origem, "sala.db", &["msgs"], &mut rel, "sala");
    // bancos de assunto: mesmos nomes dos dois lados
    let mut ass = json!({});
    if let Ok(rd) = std::fs::read_dir(origem.join("assuntos")) {
        for e in rd.flatten().filter(|e| e.path().extension().is_some_and(|x| x == "db")) {
            let nome = e.file_name().to_string_lossy().to_string();
            if let Ok(Some(c)) = abrir(&dados.join("assuntos").join(&nome), &e.path()) {
                let mut r = json!({});
                for t in ["fatos", "ficha", "log"] {
                    let (a, n) = tabela(&c, t);
                    r[t] = json!([a, n]);
                }
                ass[nome.trim_end_matches(".db")] = r;
            }
        }
    }
    rel["assuntos"] = ass;
    banco(dados, origem, "alma/perfil.db", &["sinais", "rotulos", "estilo", "temperamento", "exemplos", "correcoes"], &mut rel, "alma_perfil");
    // agenda e Compasso
    rel["agenda"] = json!(mesclar_lista(&origem.join("agenda.json"), &dados.join("agenda.json"), None, "id"));
    rel["compasso_rotinas"] = json!(mesclar_lista(&origem.join("compasso/rotinas.json"), &dados.join("compasso/rotinas.json"), Some("rotinas"), "id"));
    rel["compasso_pedidos"] = json!(mesclar_linhas(&origem.join("compasso/pedidos.jsonl"), &dados.join("compasso/pedidos.jsonl")));
    // Alma
    let mut alma = json!({});
    for f in ["diario.jsonl", "falas.jsonl", "estilo_amostras.jsonl", "humor_diario.jsonl", "sobrecarga.jsonl", "atividade.jsonl"] {
        alma[f] = json!(mesclar_linhas(&origem.join("alma").join(f), &dados.join("alma").join(f)));
    }
    for f in ["cuida.json", "humor_estado.json", "parametros.json", "loops.json", "voz_estado.json", "guia_de_voz.md"] {
        alma[f] = json!(copiar_se_falta(&origem.join("alma").join(f), &dados.join("alma").join(f)));
    }
    rel["alma"] = alma;
    // Claudinho, erros, funil, outros
    let mut c = json!({});
    for f in ["guia.md", "estado.json"] {
        c[f] = json!(copiar_se_falta(&origem.join("claudinho").join(f), &dados.join("claudinho").join(f)));
    }
    c["achados.jsonl"] = json!(mesclar_linhas(&origem.join("claudinho/achados.jsonl"), &dados.join("claudinho/achados.jsonl")));
    for f in ["erros_jarvis.jsonl", "clareza.jsonl"] {
        c[f] = json!(mesclar_linhas(&origem.join(f), &dados.join(f)));
    }
    rel["claudinho_e_erros"] = c;
    rel["funil"] = json!(copiar_se_falta(&origem.join("funil_usuarios.json"), &dados.join("funil_dono.json")));
    rel["candidaturas"] = json!(mesclar_lista(&origem.join("candidaturas.json"), &dados.join("candidaturas.json"), None, "chave"));
    rel["silenciados"] = json!(copiar_se_falta(&origem.join("silenciados.json"), &dados.join("silenciados.json")));
    rel["vozes_agentes"] = json!(copiar_se_falta(&origem.join("vozes_agentes.json"), &dados.join("vozes_agentes.json")));
    rel["nao_traz"] = json!(["vigia_estado", "dono_estado", "airbnb_estado", "airbnb_escalada", "rotina.json", "pessoal.json", "agentes.db", "braincenter.db", "b2.db", "gibberlink.json", "guarda.json"]);
    rel
}

pub fn post(dados: &Path, caminho: &str, v: &Value) -> Option<crate::Resp> {
    (caminho == "/api/importar-antigo").then(|| {
        let origem = v["origem"].as_str().map(PathBuf::from).or_else(origem_padrao);
        match origem {
            Some(o) => crate::dado(importar(dados, &o)),
            None => crate::erro("mande {\"origem\": \"<pasta data do Braniac antigo>\"} (ou defina BRANIAC_ENV)", 400),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pasta(n: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-imp-{n}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn importar_duas_vezes_nao_duplica_e_nao_mexe_no_antigo() {
        let (o, d) = (pasta("o"), pasta("d"));
        {
            let c = Connection::open(o.join("memoria.db")).unwrap();
            c.execute_batch("CREATE TABLE mensagens(id INTEGER PRIMARY KEY, contato TEXT, direcao TEXT, canal TEXT, texto TEXT, audio INTEGER DEFAULT 0, ts REAL);
                INSERT INTO mensagens(contato,direcao,canal,texto,ts) VALUES ('Ana','in','w','oi',1.0),('Ana','out','w','ola',2.0);").unwrap();
        }
        std::fs::write(o.join("contexto.json"), json!({ "apresentados": ["Ana", "Bia"] }).to_string()).unwrap();
        std::fs::write(o.join("agenda.json"), json!([{ "id": "a1", "titulo": "x" }]).to_string()).unwrap();
        std::fs::write(o.join("erros_jarvis.jsonl"), "{\"a\":1}\n{\"a\":2}\n").unwrap();
        let antes = std::fs::read(o.join("memoria.db")).unwrap();
        let r1 = importar(&d, &o);
        assert_eq!(r1["memoria"]["mensagens"]["novos"], 2);
        assert_eq!(r1["apresentados"]["novos"], 2);
        let r2 = importar(&d, &o);
        assert_eq!(r2["memoria"]["mensagens"]["novos"], 0);
        assert_eq!(r2["apresentados"]["novos"], 0);
        assert_eq!(r2["agenda"], json!([1, 0]));
        assert_eq!(r2["claudinho_e_erros"]["erros_jarvis.jsonl"], json!([2, 0]));
        assert_eq!(Connection::open(d.join("memoria.db")).unwrap().query_row("SELECT count(*) FROM mensagens", [], |r| r.get::<_, i64>(0)).unwrap(), 2);
        assert_eq!(std::fs::read(o.join("memoria.db")).unwrap(), antes, "o banco do antigo nao pode mudar");
        assert_eq!(importar(&d, &pasta("vazia"))["ok"], false);
        let _ = std::fs::remove_dir_all(&o);
        let _ = std::fs::remove_dir_all(&d);
    }
}
