//! B.R.A.N.I.A.C 1: projetos e conversas SEPARADOS (dados/b2.db, mesmo formato do Python; cada conversa e a `b2:<id>` do historico),
//! instrucoes do projeto que acompanham cada mensagem, habilidades criadas pela interface, aparencia e chaves de IA.
//! Nada e apagado: conversa arquivada so some da lista. A listagem de chaves NUNCA devolve o valor.
//! Ficou de fora: abrir a aba de uma conta no navegador (`abrir_conta`), procurar nas conversas antigas (o Store nao expoe o banco),
//! transcrever audio, o sinal para a janela do programa e `os.environ` (quem chama ativa a chave gravada). IA e leitura de imagem entram
//! como closure. Diferenca do Python: o teto de habilidades aprendidas por dia fica em arquivo (dados/b2/uso.json), nao na memoria.
#![allow(dead_code)]
use crate::empresa_dev::{corta, gravar, ler, rx, somar_uso, uso};
use crate::store::Store;
use crate::tempo::Agora;
use crate::{base64, segredos};
use regex::Regex;
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

pub const MAX_APRENDIDAS_DIA: i64 = 3;
pub const CORES: [(&str, &str); 8] = [("ciano", "#2fe3ff"), ("azul", "#3b82f6"), ("verde", "#22c55e"), ("roxo", "#a855f7"), ("rosa", "#ec4899"), ("laranja", "#f97316"), ("vermelho", "#ef4444"), ("dourado", "#eab308")];
pub const CONTAS: [(&str, &str); 7] = [("chatgpt", "https://chatgpt.com/"), ("gemini", "https://gemini.google.com/app"), ("grok", "https://grok.com/"), ("deepseek", "https://chat.deepseek.com/"),
    ("kimi", "https://www.kimi.ai/"), ("claude", "https://claude.ai/"), ("google", "https://myaccount.google.com/")];
/// variavel no .env, nome, onde criar a chave gratis
pub const CHAVES: [(&str, &str, &str); 10] = [
    ("GEMINI_API_KEY", "Gemini (Google)", "https://aistudio.google.com/apikey"),
    ("GEMINI_API_KEY_2", "Gemini (2ª chave)", "https://aistudio.google.com/apikey"),
    ("GROQ_API_KEY", "Groq", "https://console.groq.com/keys"),
    ("OPENROUTER_API_KEY", "OpenRouter", "https://openrouter.ai/keys"),
    ("CEREBRAS_API_KEY", "Cerebras", "https://cloud.cerebras.ai/"),
    ("MISTRAL_API_KEY", "Mistral", "https://console.mistral.ai/api-keys"),
    ("NVIDIA_API_KEY", "NVIDIA NIM", "https://build.nvidia.com/"),
    ("HUGGINGFACE_API_KEY", "HuggingFace", "https://huggingface.co/settings/tokens"),
    ("SAMBANOVA_API_KEY", "SambaNova", "https://cloud.sambanova.ai/apis"),
    ("CLOUDFLARE_API_KEY", "Cloudflare Workers AI", "https://dash.cloudflare.com/profile/api-tokens"),
];

fn e(x: impl ToString) -> String {
    x.to_string()
}

fn db(dados: &Path, ts: f64) -> Result<Connection, String> {
    std::fs::create_dir_all(dados).map_err(e)?;
    let c = Connection::open(dados.join("b2.db")).map_err(e)?;
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS projetos(id INTEGER PRIMARY KEY, nome TEXT NOT NULL, instrucoes TEXT DEFAULT '', criado REAL);
         CREATE TABLE IF NOT EXISTS conversas(id INTEGER PRIMARY KEY, projeto INTEGER, titulo TEXT, criado REAL, atualizado REAL, arquivada INTEGER DEFAULT 0);",
    ).map_err(e)?;
    if c.query_row("SELECT COUNT(*) FROM projetos", [], |r| r.get::<_, i64>(0)).map_err(e)? == 0 {
        c.execute("INSERT INTO projetos(nome, instrucoes, criado) VALUES('Geral', '', ?)", [ts]).map_err(e)?;
    }
    // coluna nova em banco antigo; NULL = conversa antiga (todos os agentes)
    if c.prepare("SELECT modulos FROM conversas LIMIT 1").is_err() {
        c.execute("ALTER TABLE conversas ADD COLUMN modulos TEXT", []).map_err(e)?;
    }
    Ok(c)
}

fn mods_json(txt: Option<String>) -> Value {
    txt.and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null)
}

/// Projetos, conversas nao arquivadas (mais recentes primeiro, ate 300) e os agentes que uma conversa pode liberar.
pub fn estado(dados: &Path, disponiveis: &[String], ts: f64) -> Result<Value, String> {
    let c = db(dados, ts)?;
    let mut st = c.prepare("SELECT id, nome, instrucoes FROM projetos ORDER BY id").map_err(e)?;
    let p: Vec<Value> = st.query_map([], |r| Ok(json!({ "id": r.get::<_, i64>(0)?, "nome": r.get::<_, String>(1)?, "instrucoes": r.get::<_, Option<String>>(2)?.unwrap_or_default() })))
        .map_err(e)?.flatten().collect();
    let mut st = c.prepare("SELECT id, projeto, titulo, atualizado, modulos FROM conversas WHERE arquivada=0 ORDER BY atualizado DESC LIMIT 300").map_err(e)?;
    let v: Vec<Value> = st.query_map([], |r| {
        Ok(json!({ "id": r.get::<_, i64>(0)?, "projeto": r.get::<_, Option<i64>>(1)?, "titulo": r.get::<_, Option<String>>(2)?, "atualizado": r.get::<_, Option<f64>>(3)?, "modulos": mods_json(r.get(4)?) }))
    }).map_err(e)?.flatten().collect();
    let mut disp = disponiveis.to_vec();
    disp.sort();
    Ok(json!({ "projetos": p, "conversas": v, "modulos_disponiveis": disp }))
}

/// Agentes liberados nesta conversa; None = conversa antiga ou inexistente (sem restricao).
pub fn modulos_da_conversa(dados: &Path, conv: i64, ts: f64) -> Option<Vec<String>> {
    let c = db(dados, ts).ok()?;
    let txt: Option<String> = c.query_row("SELECT modulos FROM conversas WHERE id=?", [conv], |r| r.get(0)).ok()?;
    serde_json::from_str(&txt?).ok()
}

fn texto(v: &Value, n: usize) -> Result<String, String> {
    let t = v.as_str().unwrap_or("").trim();
    if t.is_empty() || t.chars().count() > n {
        return Err(format!("texto vazio ou maior que {n} caracteres"));
    }
    Ok(t.to_string())
}

fn inteiro(v: &Value) -> Result<i64, String> {
    v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())).ok_or_else(|| "id invalido".to_string())
}

fn mods_validos(v: &Value, disponiveis: &[String]) -> String {
    let m: Vec<&str> = v.as_array().into_iter().flatten().filter_map(Value::as_str).filter(|m| disponiveis.iter().any(|d| d == m)).collect();
    json!(m).to_string()
}

fn junta(mut a: Value, b: Value) -> Value {
    if let (Some(a), Some(b)) = (a.as_object_mut(), b.as_object()) {
        a.extend(b.clone());
    }
    a
}

/// Uma acao da interface. Devolve o estado novo com `novo` (id criado ou mexido). `hoje` = "dd/mm/aaaa" (vai no SKILL.md).
pub fn acao(dados: &Path, skills: &Path, d: &Value, disponiveis: &[String], ts: f64, hoje: &str) -> Result<Value, String> {
    let c = db(dados, ts)?;
    let instr = corta(d["instrucoes"].as_str().unwrap_or(""), 4000);
    let novo = match d["acao"].as_str().unwrap_or("") {
        "criar_projeto" => {
            c.execute("INSERT INTO projetos(nome, instrucoes, criado) VALUES(?,?,?)", params![texto(&d["nome"], 80)?, instr, ts]).map_err(e)?;
            c.last_insert_rowid()
        }
        "editar_projeto" => {
            let id = inteiro(&d["id"])?;
            c.execute("UPDATE projetos SET nome=?, instrucoes=? WHERE id=?", params![texto(&d["nome"], 80)?, instr, id]).map_err(e)?;
            id
        }
        "criar_conversa" => {
            let titulo = corta(d["titulo"].as_str().filter(|t| !t.is_empty()).unwrap_or("Nova conversa"), 80);
            c.execute("INSERT INTO conversas(projeto, titulo, criado, atualizado, modulos) VALUES(?,?,?,?,?)",
                      params![inteiro(&d["projeto"]).unwrap_or(1), titulo, ts, ts, mods_validos(&d["modulos"], disponiveis)]).map_err(e)?;
            c.last_insert_rowid()
        }
        "modulos" => {
            let id = inteiro(&d["id"])?;
            c.execute("UPDATE conversas SET modulos=? WHERE id=?", params![mods_validos(&d["modulos"], disponiveis), id]).map_err(e)?;
            id
        }
        "renomear" => {
            let id = inteiro(&d["id"])?;
            c.execute("UPDATE conversas SET titulo=? WHERE id=?", params![texto(&d["titulo"], 80)?, id]).map_err(e)?;
            id
        }
        "mover" => {
            let id = inteiro(&d["id"])?;
            c.execute("UPDATE conversas SET projeto=? WHERE id=?", params![inteiro(&d["projeto"])?, id]).map_err(e)?;
            id
        }
        // nada e apagado: so some da lista
        "arquivar" => {
            let id = inteiro(&d["id"])?;
            c.execute("UPDATE conversas SET arquivada=1 WHERE id=?", [id]).map_err(e)?;
            id
        }
        "aparencia" => {
            let a = salvar_aparencia(dados, d["nome"].as_str().unwrap_or(""), d["cor"].as_str().unwrap_or(""))?;
            return Ok(junta(json!({ "aparencia": a }), estado(dados, disponiveis, ts)?));
        }
        "criar_skill" => {
            let sk = criar_skill(skills, d["nome"].as_str().unwrap_or(""), d["descricao"].as_str().unwrap_or(""), d["passos"].as_str().unwrap_or(""), d["ferramentas"].as_str().unwrap_or(""), hoje)?;
            return Ok(junta(json!({ "skill": sk }), estado(dados, disponiveis, ts)?));
        }
        _ => return Err("acao desconhecida".into()),
    };
    drop(c);
    Ok(junta(json!({ "novo": novo }), estado(dados, disponiveis, ts)?))
}

/// Texto extra que vai junto com a mensagem: o projeto e as instrucoes dele. Tambem atualiza a data da conversa.
pub fn contexto_da_conversa(dados: &Path, conv: i64, ts: f64) -> Result<String, String> {
    let c = db(dados, ts)?;
    let r: Option<(String, Option<String>)> = c.query_row("SELECT p.nome, p.instrucoes FROM conversas v JOIN projetos p ON p.id=v.projeto WHERE v.id=?", [conv], |r| Ok((r.get(0)?, r.get(1)?))).ok();
    c.execute("UPDATE conversas SET atualizado=? WHERE id=?", params![ts, conv]).map_err(e)?;
    let (nome, instr) = r.ok_or("conversa nao existe")?;
    let instr = instr.unwrap_or_default();
    let instr = instr.trim();
    Ok(format!("\n\n[Projeto: {nome}.{}]", if instr.is_empty() { String::new() } else { format!(" Instrucoes do projeto: {instr}") }))
}

/// A primeira mensagem vira o titulo da conversa nova.
pub fn titulo_inicial(dados: &Path, conv: i64, msg: &str, ts: f64) -> Result<(), String> {
    let t = corta(&msg.split_whitespace().collect::<Vec<_>>().join(" "), 60);
    db(dados, ts)?.execute("UPDATE conversas SET titulo=? WHERE id=? AND titulo='Nova conversa'", params![t, conv]).map(|_| ()).map_err(e)
}

/// O contexto do projeto nao aparece na tela.
pub fn sem_contexto(t: &str) -> String {
    static R: OnceLock<Regex> = OnceLock::new();
    rx(&R, r"\n\n\[Projeto: [^\]]*\]$").replace(t, "").into_owned()
}

pub fn mensagens(store: &Store, conv: i64, n: usize) -> Vec<Value> {
    store.historico(&format!("b2:{conv}"), n).into_iter()
        .map(|m| json!({ "role": m["role"], "content": sem_contexto(m["content"].as_str().unwrap_or("")), "provedor": m["provider"] })).collect()
}

/// Habilidade nova (formato Agent Skills). Nao sobrescreve uma que ja existe. Devolve o nome (slug).
pub fn criar_skill(skills: &Path, nome: &str, descricao: &str, passos: &str, ferramentas: &str, hoje: &str) -> Result<String, String> {
    static NAO: OnceLock<Regex> = OnceLock::new();
    static FERR: OnceLock<Regex> = OnceLock::new();
    let slug = corta(rx(&NAO, "[^a-z0-9]+").replace_all(&nome.to_lowercase(), "-").trim_matches('-'), 40);
    if slug.is_empty() || descricao.trim().is_empty() || passos.trim().is_empty() {
        return Err("habilidade precisa de nome, descricao (quando usar) e passos".into());
    }
    let pasta = skills.join(&slug);
    if pasta.exists() {
        return Err(format!("ja existe uma habilidade chamada {slug}"));
    }
    // so nomes de ferramenta: qualquer outra coisa (caminho, opcao de comando) e descartada
    let ferr = rx(&FERR, "[a-z_]+").find_iter(ferramentas).map(|m| m.as_str()).collect::<Vec<_>>().join(" ");
    let desc = corta(&descricao.split_whitespace().collect::<Vec<_>>().join(" "), 300);
    std::fs::create_dir_all(&pasta).map_err(e)?;
    let md = format!("---\nname: {slug}\ndescription: {desc}\n{}metadata:\n  origem: criada pelo B.R.A.N.I.A.C 1 em {hoje}\n---\n\n# {}\n\n{}\n",
                     if ferr.is_empty() { String::new() } else { format!("allowed-tools: {ferr}\n") }, nome.trim(), passos.trim());
    std::fs::write(pasta.join("SKILL.md"), md).map_err(e)?;
    Ok(slug)
}

pub fn relatorio(dados: &Path) -> String {
    std::fs::read_to_string(dados.join("relatorio_noturno.md")).ok().map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).unwrap_or_else(|| "Nada registrado ainda.".into())
}

/// Situacao das contas a partir da lista de abas do navegador do Braniac (None = navegador fechado): aba aberta? pede login?
pub fn contas(abas: Option<&Value>) -> Value {
    static LOGIN: OnceLock<Regex> = OnceLock::new();
    let Some(abas) = abas.and_then(Value::as_array) else { return json!({ "navegador": false, "contas": [] }) };
    let out: Vec<Value> = CONTAS.iter().map(|(nome, url)| {
        let dom = url.split('/').nth(2).unwrap_or("");
        let aba = abas.iter().find(|a| a["type"] == "page" && a["url"].as_str().is_some_and(|u| u.contains(dom))).and_then(|a| a["url"].as_str());
        let login = aba.is_some_and(|u| rx(&LOGIN, r"(?i)sign_?in|login|signin|auth|accounts\.google").is_match(u));
        json!({ "nome": nome, "aberta": aba.is_some(), "precisa_login": login, "url": aba.unwrap_or(url) })
    }).collect();
    json!({ "navegador": true, "contas": out })
}

/// IA costuma por quebra de linha crua dentro do texto do JSON: escapa o que estiver dentro de aspas.
fn json_frouxo(t: &str) -> Option<Value> {
    let (i, f) = (t.find('{')?, t.rfind('}')?);
    let (mut out, mut dentro, mut esc) = (String::new(), false, false);
    for ch in t.get(i..=f)?.chars() {
        match ch {
            '"' if !esc => dentro = !dentro,
            '\n' if dentro => { out.push_str("\\n"); continue }
            '\r' if dentro => continue,
            '\t' if dentro => { out.push_str("\\t"); continue }
            _ => {}
        }
        esc = ch == '\\' && !esc;
        out.push(ch);
    }
    serde_json::from_str(&out).ok()
}

/// Depois de uma tarefa com 3+ ferramentas diferentes, a IA (closure: sistema, pedido) decide se vale guardar como habilidade.
/// Teto de 3 por dia. `registrar` anota no relatorio da manha. `feitos` = [(ferramenta, resultado)].
pub fn aprender(dados: &Path, skills: &Path, pedido: &str, feitos: &[(String, String)], agora: &Agora, ia: &dyn Fn(&str, &str) -> Result<String, String>, registrar: &dyn Fn(&str)) -> Option<String> {
    let usadas: BTreeSet<&str> = feitos.iter().map(|(n, _)| n.as_str()).filter(|n| !matches!(*n, "usar_skill" | "criar_habilidade")).collect();
    if usadas.len() < 3 || feitos.iter().any(|(n, _)| n == "criar_habilidade") {
        return None;
    }
    let (pasta, hoje) = (dados.join("b2"), agora.data.iso());
    if uso(&pasta, &hoje, "aprendidas") >= MAX_APRENDIDAS_DIA {
        return None;
    }
    let passos = feitos.iter().take(12).map(|(n, r)| format!("- {n}: {}", corta(r, 160))).collect::<Vec<_>>().join("\n");
    let usadas = usadas.into_iter().collect::<Vec<_>>().join(" ");
    let sistema = "Voce decide se uma tarefa resolvida por um assistente vale virar HABILIDADE reutilizavel (passo a passo). Responda SO um JSON: \
        {\"vale\": true|false, \"nome\": \"kebab-case\", \"descricao\": \"quando usar (1 frase)\", \"passos\": \"1. ...\\n2. ...\", \"ferramentas\": \"nomes separados por espaco\"}. \
        vale=false se for algo unico, pessoal demais ou trivial.";
    let resp = ia(sistema, &format!("Pedido: {}\nFerramentas usadas e resultados:\n{passos}\nFerramentas: {usadas}", corta(pedido, 500))).ok()?;
    let d = json_frouxo(&resp)?;
    if d["vale"].as_bool() != Some(true) {
        return None;
    }
    let desc = d["descricao"].as_str()?;
    let (_, m, dia) = agora.data.partes();
    let data = format!("{dia:02}/{m:02}/{}", agora.data.partes().0);
    let nome = criar_skill(skills, d["nome"].as_str()?, desc, d["passos"].as_str()?, d["ferramentas"].as_str().filter(|f| !f.is_empty()).unwrap_or(&usadas), &data).ok()?;
    let _ = somar_uso(&pasta, &hoje, "aprendidas");
    registrar(&format!("O Jarvis aprendeu uma habilidade nova sozinho: '{nome}' ({}). Veja na aba Habilidades do B.R.A.N.I.A.C 1.", corta(desc, 120)));
    Some(nome)
}

fn cores() -> Value {
    Value::Object(CORES.iter().map(|(k, v)| (k.to_string(), json!(v))).collect())
}

/// {"nome", "cor", "configurado", "cores"}; padrao Jarvis/ciano enquanto ninguem escolheu.
pub fn aparencia(dados: &Path) -> Value {
    match ler(&dados.join("aparencia.json")) {
        Some(d) if d["nome"].is_string() && d["cor"].is_string() => json!({ "nome": d["nome"], "cor": d["cor"], "configurado": true, "cores": cores() }),
        _ => json!({ "nome": "Jarvis", "cor": CORES[0].1, "configurado": false, "cores": cores() }),
    }
}

pub fn salvar_aparencia(dados: &Path, nome: &str, cor: &str) -> Result<Value, String> {
    static NOME: OnceLock<Regex> = OnceLock::new();
    static COR: OnceLock<Regex> = OnceLock::new();
    let nome = corta(&nome.split_whitespace().collect::<Vec<_>>().join(" "), 24);
    if !rx(&NOME, r"^[\w .'-]{2,24}$").is_match(&nome) {
        return Err("nome precisa ter de 2 a 24 letras".into());
    }
    let cor = CORES.iter().find(|(k, _)| *k == cor).map_or(cor, |(_, v)| v).to_lowercase();
    if !rx(&COR, "^#[0-9a-f]{6}$").is_match(&cor) {
        return Err("cor invalida".into());
    }
    gravar(&dados.join("aparencia.json"), &json!({ "nome": nome, "cor": cor }))?;
    Ok(aparencia(dados))
}

/// Quais chaves estao configuradas. NUNCA devolve o valor. `env` = como ler a variavel (quem chama decide).
pub fn chaves(env: &dyn Fn(&str) -> Option<String>) -> Vec<Value> {
    CHAVES.iter().map(|(v, n, u)| json!({ "env": v, "nome": n, "onde": u, "ativa": env(v).is_some_and(|x| !x.trim().is_empty()) })).collect()
}

/// Grava a chave no .env (criptografada pelo Windows quando possivel). So variavel conhecida e valor com cara de chave.
/// Devolve o valor limpo para quem chama ativar; ele nunca vai a log nem a resposta da interface.
pub fn salvar_chave(arquivo: &Path, env: &str, valor: &str) -> Result<String, String> {
    static OK: OnceLock<Regex> = OnceLock::new();
    if !CHAVES.iter().any(|(v, _, _)| *v == env) {
        return Err("variavel desconhecida".into());
    }
    let valor = valor.trim().trim_matches('"').trim_matches('\'');
    if !rx(&OK, r"^[A-Za-z0-9_\-\.:=+/]{8,400}$").is_match(valor) {
        return Err("chave invalida: use so a chave, sem espacos nem aspas (8 a 400 caracteres)".into());
    }
    let atual = std::fs::read_to_string(arquivo).unwrap_or_default();
    let mut linhas: Vec<String> = atual.trim_start_matches('\u{feff}').lines().map(String::from).collect();
    let guardado = segredos::proteger(valor).ok().filter(|c| segredos::revelar(c).ok().as_deref() == Some(valor)).unwrap_or_else(|| valor.to_string());
    let nova = format!("{env}={guardado}");
    match linhas.iter_mut().find(|l| l.split('=').next().is_some_and(|k| k.trim() == env)) {
        Some(l) => *l = nova,
        None => linhas.push(nova),
    }
    let tmp = arquivo.with_extension("env.tmp");
    std::fs::write(&tmp, linhas.join("\n") + "\n").and_then(|_| std::fs::rename(&tmp, arquivo)).map_err(|x| format!("não gravei a chave: {x}"))?;
    Ok(valor.to_string())
}

static PARTES: Mutex<BTreeMap<String, HashMap<i64, String>>> = Mutex::new(BTreeMap::new());

/// Envio em pedacos ({id, i, n, parte em base64}; o tunel do celular aceita ~64 KB por mensagem). Devolve os bytes quando chegou tudo.
pub fn juntar_partes(d: &Value) -> Result<Option<Vec<u8>>, String> {
    let cid = corta(d["id"].as_str().unwrap_or(""), 40);
    let (i, n) = (d["i"].as_i64().unwrap_or(0), d["n"].as_i64().unwrap_or(1));
    if cid.is_empty() || !(1..=400).contains(&n) {
        return Err("envio invalido".into());
    }
    let mut p = PARTES.lock().map_err(e)?;
    if p.len() > 20 {
        p.clear();
    }
    let partes = p.entry(cid.clone()).or_default();
    partes.insert(i, d["parte"].as_str().unwrap_or("").to_string());
    if (partes.len() as i64) < n {
        return Ok(None);
    }
    let partes = p.remove(&cid).unwrap_or_default();
    let txt: String = (0..n).map(|k| partes.get(&k).map_or("", String::as_str)).collect();
    base64::decodificar(&txt).map(Some)
}

/// Guarda a imagem em dados/anexos e pede a leitura a `descrever` (closure; IA local). Devolve {caminho, descricao}.
pub fn salvar_anexo(dados: &Path, bytes: &[u8], agora: &Agora, descrever: &dyn Fn(&[u8]) -> Result<String, String>) -> Result<Value, String> {
    if bytes.len() > 12_000_000 {
        return Err("imagem grande demais (maximo 12 MB)".into());
    }
    let ext = if bytes.starts_with(b"\x89PNG") { "png" } else { "jpg" };
    let pasta = dados.join("anexos");
    std::fs::create_dir_all(&pasta).map_err(e)?;
    let caminho = pasta.join(format!("{}.{ext}", agora.carimbo().replace('-', "_")));
    std::fs::write(&caminho, bytes).map_err(e)?;
    let desc = descrever(bytes).unwrap_or_else(|x| format!("(nao consegui ler a imagem agora: {})", corta(&x, 80)));
    Ok(json!({ "caminho": caminho.to_string_lossy(), "descricao": desc }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;
    use std::path::PathBuf;

    fn pasta(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-b2-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn agora() -> Agora {
        Agora { data: Data::civil(2026, 11, 18).unwrap(), hora: 9, min: 5, seg: 7 }
    }

    fn disp() -> Vec<String> {
        vec!["Cofre".to_string(), "Ada".to_string()]
    }

    #[test]
    fn projeto_conversa_contexto_e_nada_e_apagado() {
        let d = pasta("projeto");
        let a = |v: Value, ts: f64| acao(&d, &d.join("skills"), &v, &disp(), ts, "18/11/2026");
        let e1 = a(json!({ "acao": "criar_projeto", "nome": "Sites", "instrucoes": "Responda curto." }), 10.0).unwrap();
        let pid = e1["novo"].as_i64().unwrap();
        assert_eq!(e1["projetos"].as_array().unwrap().len(), 2); // o Geral nasce sozinho
        assert_eq!(e1["modulos_disponiveis"], json!(["Ada", "Cofre"]));
        let c = a(json!({ "acao": "criar_conversa", "projeto": pid, "modulos": ["Cofre", "Inexistente"] }), 11.0).unwrap()["novo"].as_i64().unwrap();
        assert_eq!(modulos_da_conversa(&d, c, 11.0), Some(vec!["Cofre".to_string()])); // agente desconhecido nao e liberado
        assert_eq!(modulos_da_conversa(&d, 999, 11.0), None);
        assert!(contexto_da_conversa(&d, c, 12.0).unwrap().contains("[Projeto: Sites. Instrucoes do projeto: Responda curto.]"));
        assert!(contexto_da_conversa(&d, 999, 12.0).is_err());
        titulo_inicial(&d, c, "Arrumar   o site\nda loja", 13.0).unwrap();
        let titulos = |ts: f64| estado(&d, &disp(), ts).unwrap()["conversas"].as_array().unwrap().iter().map(|v| v["titulo"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        assert_eq!(titulos(13.0), ["Arrumar o site da loja"]);
        titulo_inicial(&d, c, "outro titulo", 13.0).unwrap(); // so a primeira mensagem da o titulo
        a(json!({ "acao": "renomear", "id": c, "titulo": "Site novo" }), 14.0).unwrap();
        assert_eq!(titulos(14.0), ["Site novo"]);
        a(json!({ "acao": "mover", "id": c.to_string(), "projeto": 1 }), 15.0).unwrap();
        assert!(contexto_da_conversa(&d, c, 15.0).unwrap().ends_with("[Projeto: Geral.]"));
        a(json!({ "acao": "editar_projeto", "id": 1, "nome": "Tudo", "instrucoes": "" }), 16.0).unwrap();
        a(json!({ "acao": "modulos", "id": c, "modulos": [] }), 16.0).unwrap();
        assert_eq!(modulos_da_conversa(&d, c, 16.0), Some(vec![]));
        a(json!({ "acao": "arquivar", "id": c }), 17.0).unwrap();
        assert!(titulos(17.0).is_empty()); // arquivar so esconde
        let n: i64 = db(&d, 17.0).unwrap().query_row("SELECT COUNT(*) FROM conversas", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        for ruim in [json!({ "acao": "criar_projeto", "nome": "" }), json!({ "acao": "criar_projeto", "nome": "x".repeat(81) }), json!({ "acao": "renomear", "id": "abc", "titulo": "x" }),
                     json!({ "acao": "apagar_tudo" }), json!({ "acao": "abrir_conta", "nome": "chatgpt" })] {
            assert!(a(ruim, 18.0).is_err());
        }
        assert_eq!(sem_contexto("oi\n\n[Projeto: Geral.]"), "oi");
        assert_eq!(sem_contexto("[Projeto: x] no meio"), "[Projeto: x] no meio");
        let st = Store::em_memoria().unwrap();
        st.add_msg("b2:7", "user", "oi\n\n[Projeto: Sites.]", None).unwrap();
        assert_eq!(mensagens(&st, 7, 10)[0]["content"], "oi");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn criar_skill_nao_sobrescreve_valida_e_limpa_ferramentas() {
        let d = pasta("skill");
        let nome = criar_skill(&d, "Responder Hóspede!", "quando hospede perguntar do check-in", "1. ler\n2. responder", "airbnb_ler rm -rf /", "18/11/2026").unwrap();
        let md = std::fs::read_to_string(d.join(&nome).join("SKILL.md")).unwrap();
        assert!(nome == "responder-h-spede" && md.contains("allowed-tools: airbnb_ler rm rf\n") && md.contains("name: responder-h-spede"));
        assert!(criar_skill(&d, "Responder Hóspede!", "x", "y", "", "").is_err());
        assert!(criar_skill(&d, "sem passos", "x", "", "", "").is_err());
        assert!(criar_skill(&d, "../..", "x", "y", "", "").is_err()); // nome sem letra nao vira pasta
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn aprender_so_em_tarefa_complexa_e_com_teto_diario() {
        let d = pasta("aprender");
        let sk = d.join("skills");
        let f = |n: &[&str]| n.iter().map(|x| (x.to_string(), "ok".to_string())).collect::<Vec<_>>();
        let chamou = std::cell::Cell::new(0);
        let js = "claro:\n{\"vale\": true, \"nome\": \"agendar-dentista\", \"descricao\": \"quando pedir para marcar dentista\", \"passos\": \"1. agenda\n2. pc\", \"ferramentas\": \"agenda pc\"}";
        let ia = |_: &str, _: &str| { chamou.set(chamou.get() + 1); Ok(js.to_string()) };
        let notas = std::cell::RefCell::new(Vec::new());
        let reg = |t: &str| notas.borrow_mut().push(t.to_string());
        assert!(aprender(&d, &sk, "marca dentista", &f(&["agenda", "pc"]), &agora(), &ia, &reg).is_none() && chamou.get() == 0); // 2 ferramentas: nem pergunta
        assert!(aprender(&d, &sk, "x", &f(&["agenda", "pc", "criar_habilidade", "web"]), &agora(), &ia, &reg).is_none() && chamou.get() == 0);
        assert_eq!(aprender(&d, &sk, "marca dentista", &f(&["agenda", "pc", "pesquisar_web"]), &agora(), &ia, &reg).as_deref(), Some("agendar-dentista"));
        assert!(sk.join("agendar-dentista/SKILL.md").exists() && notas.borrow()[0].contains("aprendeu"));
        assert!(std::fs::read_to_string(sk.join("agendar-dentista/SKILL.md")).unwrap().contains("em 18/11/2026"));
        assert!(aprender(&d, &sk, "x", &f(&["a", "b", "c"]), &agora(), &|_, _| Ok("{\"vale\": false}".into()), &reg).is_none());
        assert!(aprender(&d, &sk, "x", &f(&["a", "b", "c"]), &agora(), &|_, _| Err("fora do ar".into()), &reg).is_none());
        for i in 0..MAX_APRENDIDAS_DIA {
            let _ = aprender(&d, &sk, "x", &f(&["a", "b", "c"]), &agora(), &|_, _| Ok(js.replace("agendar-dentista", &format!("outra-{i}"))), &reg);
        }
        let antes = chamou.get();
        assert!(aprender(&d, &sk, "x", &f(&["a", "b", "c"]), &agora(), &ia, &reg).is_none() && chamou.get() == antes); // teto: nem pergunta
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn chave_so_conhecida_valida_e_nunca_listada() {
        let d = pasta("chave");
        let env = d.join(".env");
        std::fs::write(&env, "GROQ_API_KEY=abc\nCEREBRAS_API_KEY=\n").unwrap();
        assert_eq!(salvar_chave(&env, "CEREBRAS_API_KEY", " \"csk-1234567890abcdef\" ").unwrap(), "csk-1234567890abcdef");
        let guardado = |k: &str| {
            let t = std::fs::read_to_string(&env).unwrap();
            let v = t.lines().find_map(|l| l.strip_prefix(&format!("{k}="))).unwrap().to_string();
            if v.starts_with(segredos::PREFIXO) { segredos::revelar(&v).unwrap() } else { v }
        };
        assert!(guardado("CEREBRAS_API_KEY") == "csk-1234567890abcdef" && guardado("GROQ_API_KEY") == "abc");
        salvar_chave(&env, "NVIDIA_API_KEY", "nvapi-abcdef123456").unwrap(); // variavel nova: acrescenta no fim
        assert!(std::fs::read_to_string(&env).unwrap().lines().last().unwrap().starts_with("NVIDIA_API_KEY="));
        let lista = chaves(&|k| (k == "CEREBRAS_API_KEY").then(|| "csk-1234567890abcdef".to_string()));
        assert!(!json!(lista).to_string().contains("csk-1234")); // a listagem nunca traz o valor
        assert!(lista.iter().any(|c| c["env"] == "CEREBRAS_API_KEY" && c["ativa"] == true) && lista.iter().any(|c| c["ativa"] == false));
        for ruim in ["curto", "com espaco no meio xx", &"x".repeat(500)] {
            assert!(salvar_chave(&env, "MISTRAL_API_KEY", ruim).is_err());
        }
        assert!(salvar_chave(&env, "PATH", "valorvalido123").is_err()); // so as variaveis conhecidas
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn aparencia_contas_relatorio_e_anexo_em_pedacos() {
        let d = pasta("resto");
        assert!(aparencia(&d)["configurado"] == false && aparencia(&d)["nome"] == "Jarvis");
        assert_eq!(salvar_aparencia(&d, "  Sexta   Feira ", "roxo").unwrap()["cor"], "#a855f7");
        assert!(aparencia(&d)["nome"] == "Sexta Feira" && aparencia(&d)["configurado"] == true);
        assert!(salvar_aparencia(&d, "x", "roxo").is_err() && salvar_aparencia(&d, "<script>", "roxo").is_err() && salvar_aparencia(&d, "Nome", "javascript:1").is_err());
        assert_eq!(relatorio(&d), "Nada registrado ainda.");
        assert_eq!(contas(None)["navegador"], false);
        let abas = json!([{ "type": "page", "url": "https://chatgpt.com/auth/login" }, { "type": "page", "url": "https://grok.com/c/1" }, { "type": "worker", "url": "https://claude.ai/x" }]);
        let c = contas(Some(&abas));
        let de = |n: &str| c["contas"].as_array().unwrap().iter().find(|x| x["nome"] == n).unwrap().clone();
        assert!(de("chatgpt")["precisa_login"] == true && de("grok")["aberta"] == true && de("grok")["precisa_login"] == false && de("claude")["aberta"] == false);
        let bytes: Vec<u8> = b"\x89PNG".iter().copied().chain((0..=255u8).cycle().take(4000)).collect();
        let t = base64::codificar(&bytes);
        let partes: Vec<&str> = t.as_bytes().chunks(1000).map(|c| std::str::from_utf8(c).unwrap()).collect();
        let n = partes.len();
        for (i, p) in partes.iter().enumerate().rev().take(n - 1) { // fora de ordem: indexa por i
            assert_eq!(juntar_partes(&json!({ "id": "x", "i": i, "n": n, "parte": p })).unwrap(), None);
        }
        assert_eq!(juntar_partes(&json!({ "id": "x", "i": 0, "n": n, "parte": partes[0] })).unwrap(), Some(bytes.clone()));
        assert!(juntar_partes(&json!({ "id": "", "i": 0, "n": 1, "parte": "" })).is_err() && juntar_partes(&json!({ "id": "y", "i": 0, "n": 401, "parte": "" })).is_err());
        let r = salvar_anexo(&d, &bytes, &agora(), &|_| Ok("um logo vermelho".into())).unwrap();
        assert!(r["descricao"] == "um logo vermelho" && r["caminho"].as_str().unwrap().ends_with("20261118_090507.png"));
        assert_eq!(std::fs::read(r["caminho"].as_str().unwrap()).unwrap(), bytes);
        assert!(salvar_anexo(&d, &bytes, &agora(), &|_| Err("x".into())).unwrap()["descricao"].as_str().unwrap().contains("nao consegui"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
