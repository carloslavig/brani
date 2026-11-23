//! NexoDev, a empresa de desenvolvimento: tickets em dados/empresa_dev/ (equipe.json, tickets.json, uso.json, mesmo formato do Python).
//! Fluxo: novo -> desenvolvimento -> revisao -> qa -> pronto -> (OK do dono) entregue. Travas: ticket com palavra de segredo e recusado;
//! arquivo so com caminho relativo, extensao da lista e nunca build.rs; teto de passos e de revisoes do senior por dia; 6 tentativas e para.
//! Ficou de fora (entra como closure em `Ferramentas`): IA, git (branch, commit, merge), cargo/go build, o senior em segundo plano e o barramento.
//! Diferenca do Python: os tickets de um lote andam um depois do outro (la, em threads); as mensagens dizem "dono" no lugar do nome.
#![allow(dead_code)]
use crate::tempo::Agora;
use crate::texto_pt::sem_acento;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const MAX_TICKETS_DIA: i64 = 6;
pub const MAX_CLAUDE_DIA: i64 = 3;
pub const MAX_TENTATIVAS: i64 = 3; // ciclos dev->revisao antes de ir ao senior
pub const MAX_TENTATIVAS_TOTAL: i64 = 6; // depois disso o ticket para e pede o dono
pub const ESTADOS: [&str; 7] = ["novo", "desenvolvimento", "revisao", "qa", "pronto", "bloqueado", "entregue"];
const ATIVOS: [&str; 4] = ["novo", "desenvolvimento", "revisao", "qa"];
const EXT_OK: [&str; 27] = ["rs", "c", "h", "cpp", "hpp", "cc", "go", "mod", "lock", "py", "js", "ts", "tsx", "jsx", "html", "css", "json", "md", "txt", "sql", "yml", "yaml", "toml", "sh", "java", "xml", "mod"];
const POLITICA: &str = "PÁGINAS WEB E TELAS: HTML, CSS e JS puros, leves, em um arquivo ou poucos, SEM Cargo.toml e sem framework. LINGUAGEM do resto: prioridade total para RUST (leve, rápido, sem runtime pesado). Use C, C++ ou Go só quando fizer mais sentido que Rust. Python SÓ para automação que realmente não tem jeito de fazer em Rust. Projeto novo = Cargo.toml + src/main.rs (ou lib). Sem dependências à toa.";

pub(crate) fn rx(cel: &'static OnceLock<Regex>, p: &str) -> &'static Regex {
    cel.get_or_init(|| Regex::new(p).expect("regex fixa"))
}

/// Palavra de segredo ou dado pessoal: o texto vai a IAs online gratuitas, entao o pedido e recusado.
pub fn segredo(texto: &str) -> bool {
    static R: OnceLock<Regex> = OnceLock::new();
    rx(&R, r"(?i)\bsenha\b|\bcvv\b|\bcpf\b|api[_ -]?key|secret|\btoken\b|cart[aã]o|\bpix\b").is_match(texto)
}

pub(crate) fn norm(x: &str) -> String {
    sem_acento(x).chars().filter(char::is_ascii).collect()
}

pub(crate) fn corta(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

pub(crate) fn cauda(s: &str, n: usize) -> String {
    let c: Vec<char> = s.chars().collect();
    c[c.len().saturating_sub(n)..].iter().collect()
}

pub(crate) fn ler(p: &Path) -> Option<Value> {
    std::fs::read_to_string(p).ok().and_then(|t| serde_json::from_str(&t).ok())
}

pub(crate) fn gravar(p: &Path, v: &Value) -> Result<(), String> {
    let txt = serde_json::to_string_pretty(v).map_err(|e| e.to_string())?;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let tmp = p.with_extension("tmp");
    // Windows: leitor segurando o arquivo impede a troca; ai grava direto
    std::fs::write(&tmp, &txt).and_then(|_| std::fs::rename(&tmp, p)).or_else(|_| std::fs::write(p, &txt)).map_err(|e| format!("não gravei {}: {e}", p.display()))
}

pub(crate) fn lista(p: &Path) -> Vec<Value> {
    ler(p).and_then(|v| v.as_array().cloned()).unwrap_or_default()
}

/// Teto vindo de `limites` do equipe.json; valor ilegivel vale o padrao; nunca negativo.
pub(crate) fn limite_de(cfg: &Value, chave: &str, padrao: i64) -> i64 {
    let v = &cfg["limites"][chave];
    v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)).or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())).map_or(padrao, |n| n.max(0))
}

/// Contagem do dia em uso.json ({"AAAA-MM-DD": {chave: n}}).
pub(crate) fn uso(pasta: &Path, hoje: &str, chave: &str) -> i64 {
    ler(&pasta.join("uso.json")).and_then(|d| d[hoje][chave].as_i64()).unwrap_or(0)
}

/// Soma 1 e guarda so o dia de hoje.
pub(crate) fn somar_uso(pasta: &Path, hoje: &str, chave: &str) -> Result<(), String> {
    let mut dia = ler(&pasta.join("uso.json")).map(|d| d[hoje].clone()).filter(Value::is_object).unwrap_or_else(|| json!({}));
    dia[chave] = json!(dia[chave].as_i64().unwrap_or(0) + 1);
    gravar(&pasta.join("uso.json"), &json!({ hoje: dia }))
}

/// Trava entre processos: so um ciclo por vez. `false` = outro ciclo comecou ha menos de `validade` segundos.
pub(crate) fn travar(pasta: &Path, ts: f64, validade: f64) -> bool {
    let arq = pasta.join(".trabalhando");
    if std::fs::read_to_string(&arq).ok().and_then(|t| t.trim().parse::<f64>().ok()).is_some_and(|t| ts - t < validade) {
        return false;
    }
    let _ = std::fs::create_dir_all(pasta).and_then(|_| std::fs::write(&arq, ts.to_string()));
    true
}

pub(crate) fn destravar(pasta: &Path) {
    let _ = std::fs::remove_file(pasta.join(".trabalhando"));
}

pub(crate) fn historico(t: &mut Value, quem: &str, txt: &str, ts: f64, max: usize) {
    if !t["historico"].is_array() {
        t["historico"] = json!([]);
    }
    if let Some(h) = t["historico"].as_array_mut() {
        h.push(json!({ "ts": ts, "quem": quem, "txt": txt }));
        let sobra = h.len().saturating_sub(max);
        h.drain(..sobra);
    }
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}

fn pasta(dados: &Path) -> PathBuf {
    dados.join("empresa_dev")
}

fn padrao() -> Value {
    let a = |nome: &str, papel: &str, area: &str, nivel: &str, modelo: &str, instrucao: &str| json!({ "nome": nome, "papel": papel, "area": area, "nivel": nivel, "modelo": modelo, "instrucao": instrucao });
    json!({ "empresa": "Software House Braniac", "agentes": [
        a("Lia", "Product Owner / Gerente", "gestao", "gestao", "gratis", "Você é a gerente de produto. Quebra pedidos em tickets pequenos, define critério de aceite e escolhe quem faz."),
        a("Nina", "Dev Front End", "front", "pleno", "gratis", "Você é dev front end (HTML, CSS, JS, React). Entrega código completo, acessível e responsivo."),
        a("Beto", "Dev Back End", "back", "pleno", "gratis", "Você é dev back end (Python, Node, bancos). Código simples, validado nas bordas, com tratamento de erro."),
        a("Ravi", "Dev de APIs", "api", "pleno", "gratis", "Você desenha e implementa APIs REST: contrato primeiro, erros claros, versionamento, exemplos de chamada."),
        a("Juca", "Dev Júnior", "junior", "junior", "gratis", "Você é dev júnior: faz tarefas pequenas e bem definidas, pergunta quando há dúvida e nunca inventa requisito."),
        a("Dora", "QA / Testes", "qa", "pleno", "gratis", "Você é QA: escreve casos de teste, procura falhas e só aprova com evidência."),
        a("Sênior", "Arquiteto Sênior (Claude)", "senior", "senior", "claude", "Você é o arquiteto sênior: revisa o que os outros entregaram, decide arquitetura e desempata."),
    ] })
}

/// equipe.json manda; sem ele vale a equipe padrao.
pub fn config(dados: &Path) -> Value {
    ler(&pasta(dados).join("equipe.json")).filter(Value::is_object).unwrap_or_else(padrao)
}

/// Tetos diarios: "passos_dia" e "claude_dia" em `limites` do equipe.json; senao as constantes.
pub fn limite(dados: &Path, chave: &str) -> i64 {
    limite_de(&config(dados), chave, if chave == "claude_dia" { MAX_CLAUDE_DIA } else { MAX_TICKETS_DIA })
}

pub fn agentes(dados: &Path) -> Vec<Value> {
    config(dados)["agentes"].as_array().cloned().unwrap_or_default()
}

pub fn nomes(dados: &Path) -> Vec<String> {
    agentes(dados).iter().map(|a| s(a, "nome").to_string()).collect()
}

/// Sala do agente no andar da empresa, deduzida da area.
pub fn sala_de(a: &Value) -> &'static str {
    match s(a, "area") {
        "gestao" => "Produto",
        "junior" => "Júniors",
        "qa" => "Qualidade",
        "senior" => "Arquitetura",
        _ => "Desenvolvimento",
    }
}

pub fn tickets(dados: &Path) -> Vec<Value> {
    lista(&pasta(dados).join("tickets.json"))
}

fn gravar_tickets(dados: &Path, ts: &[Value]) -> Result<(), String> {
    gravar(&pasta(dados).join("tickets.json"), &json!(ts))
}

/// Quem pega o ticket: dev da area; area desconhecida vai ao junior; sem junior, ao primeiro pleno.
pub fn escolher(ag: &[Value], area: &str) -> Option<Value> {
    ag.iter().find(|a| s(a, "area") == area && s(a, "modelo") != "claude" && s(a, "area") != "gestao")
        .or_else(|| ag.iter().find(|a| s(a, "nivel") == "junior")).or_else(|| ag.iter().find(|a| s(a, "nivel") == "pleno")).cloned()
}

fn inferir_area(texto: &str) -> &'static str {
    let t = norm(texto);
    for (area, pal) in [("api", "api endpoint rest webhook rota swagger openapi"), ("front", "tela pagina site css html botao layout react interface visual responsivo"),
        ("back", "banco dados sql servidor python node regra fila cron script"), ("qa", "teste testes bug falha"), ("devops", "deploy dependencia docker seguranca ci")] {
        if pal.split(' ').any(|p| t.contains(p)) {
            return area;
        }
    }
    "junior"
}

/// Nome de pasta do projeto: so [a-z0-9-], ate 40 letras. Tambem protege o caminho quando o tickets.json foi editado a mao.
pub fn slug(projeto: &str) -> String {
    static R: OnceLock<Regex> = OnceLock::new();
    let p = corta(rx(&R, "[^a-z0-9]+").replace_all(&norm(projeto), "-").trim_matches('-'), 40);
    if p.is_empty() { "geral".into() } else { p }
}

fn pasta_projeto(dados: &Path, t: &Value) -> PathBuf {
    pasta(dados).join("projetos").join(slug(s(t, "projeto")))
}

/// Novo ticket. Devolve (ticket, mensagem); Err com o motivo quando recusa.
pub fn abrir(dados: &Path, projeto: &str, titulo: &str, descricao: &str, area: &str, critico: bool, ts: f64) -> Result<(Value, String), String> {
    if segredo(&format!("{titulo} {descricao}")) {
        return Err("Ticket recusado: tem palavra de segredo/dado pessoal (senha, token, CPF, cartão, Pix). Tire e abra de novo.".into());
    }
    if titulo.trim().chars().count() < 5 {
        return Err("Ticket sem título.".into());
    }
    let proj = slug(projeto);
    let mut todos = tickets(dados);
    let area = if area.is_empty() { inferir_area(&format!("{titulo} {descricao}")) } else { area };
    let resp = escolher(&agentes(dados), area).map_or(Value::Null, |a| a["nome"].clone());
    let t = json!({ "id": format!("T{:03}", todos.len() + 1), "projeto": proj, "titulo": corta(titulo.trim(), 140), "descricao": corta(descricao.trim(), 2000), "area": area,
        "critico": critico, "estado": "novo", "responsavel": resp, "tentativas": 0, "historico": [], "criado": ts });
    todos.push(t.clone());
    gravar_tickets(dados, &todos)?;
    let msg = format!("{} aberto em {proj}: {} → {}", s(&t, "id"), s(&t, "titulo"), t["responsavel"].as_str().unwrap_or("ninguém"));
    Ok((t, msg))
}

fn ext_ok(nome: &str) -> bool {
    nome.rsplit_once('.').is_some_and(|(_, e)| EXT_OK.contains(&e.to_lowercase().as_str()))
}

fn limpa_rotulo(corpo: &str) -> String {
    static R: OnceLock<Regex> = OnceLock::new();
    let rot = rx(&R, r"(?i)^(?:java|xml|rust|toml|c|cpp|c\+\+|go|python|py|json|html|css|js|javascript|ts|typescript|text|txt|bash|sh|yaml|yml|sql|markdown|md|copiar( c[oó]digo)?|copy( code)?|editar|edit|baixar|download|executar|run)$");
    let l: Vec<&str> = corpo.split('\n').collect();
    let pula = l.iter().take_while(|x| x.trim().is_empty() || rot.is_match(x.trim())).count();
    l[pula..].join("\n")
}

/// {caminho: conteudo} dos arquivos que a IA entregou: blocos `=== ARQUIVO: x ===` ... `=== FIM ===` e cercas de codigo com o nome na linha de antes
/// (ou no primeiro comentario). Caminho so relativo, sem "..", sem comecar com ".", extensao da lista e nunca build.rs (roda codigo no build).
pub fn extrair_arquivos(texto: &str) -> BTreeMap<String, String> {
    static M: OnceLock<Regex> = OnceLock::new();
    static B: OnceLock<Regex> = OnceLock::new();
    static N: OnceLock<Regex> = OnceLock::new();
    static C: OnceLock<Regex> = OnceLock::new();
    let texto = texto.replace("\r\n", "\n");
    let nome_rx = rx(&N, r"([\w.-]+(?:/[\w.-]+)*\.[A-Za-z]{1,5})");
    let seguro = |n: &str| !n.contains("..") && !n.starts_with('.') && n.rsplit('/').next() != Some("build.rs") && ext_ok(n);
    let mut out = BTreeMap::new();
    for c in rx(&M, r"(?s)=== *ARQUIVO: *([^ \n=]+) *===[ \t]*\n(.*?)\n?=== *FIM *===").captures_iter(&texto) {
        let nome = c[1].trim().trim_start_matches('/').to_string();
        if seguro(&nome) {
            out.entry(nome).or_insert_with(|| limpa_rotulo(&c[2]));
        }
    }
    for c in rx(&B, r"(?s)(?:^|\n)([^\n`]{0,120})\n?```[\w+-]*[ \t]*\n(.*?)```").captures_iter(&texto) {
        let primeira = c[2].split('\n').next().unwrap_or("");
        let comentario = rx(&C, r"^\s*(#|//|<!--|/\*)").is_match(primeira);
        let Some(m) = nome_rx.find(&c[1]).or_else(|| if comentario { nome_rx.find(primeira) } else { None }) else { continue };
        let nome = m.as_str().trim_start_matches('/').to_string();
        if seguro(&nome) {
            out.entry(nome).or_insert_with(|| c[2].to_string());
        }
    }
    out
}

/// O que a empresa faz fora deste modulo. Quem chama decide como (IA, git, cargo, Claude, barramento).
pub struct Ferramentas<'a> {
    /// IA que escreve a entrega do dev: (sistema, prompt) -> texto.
    pub gerar: &'a dyn Fn(&str, &str) -> Result<String, String>,
    /// IA gratis da revisao e do QA: (sistema, prompt) -> texto.
    pub ia: &'a dyn Fn(&str, &str) -> Result<String, String>,
    /// Texto com os arquivos que ja estao na main do projeto ("" se nao ha repositorio).
    pub contexto: &'a dyn Fn(&Value) -> String,
    /// Grava a entrega na branch do ticket: (ticket, nome do .md, conteudo, arquivos ja filtrados por `extrair_arquivos`).
    pub commit: &'a dyn Fn(&Value, &str, &str, &BTreeMap<String, String>) -> bool,
    /// Compila a branch do ticket: (Some(true) passou, Some(false) falhou, None nada compilavel; log).
    pub build: &'a dyn Fn(&Value) -> (Option<bool>, String),
    /// Aciona o senior em segundo plano com a tarefa; o parecer volta por `senior_resposta`.
    pub senior: &'a dyn Fn(&Value, &str) -> Result<(), String>,
    /// Recado no barramento: (de, para, tipo, texto).
    pub recado: &'a dyn Fn(&str, &str, &str, &str),
}

fn desenvolver(dados: &Path, t: &mut Value, f: &Ferramentas, ts: f64) -> Result<(), String> {
    let ag = agentes(dados);
    let a = ag.iter().find(|a| a["nome"] == t["responsavel"]).cloned().or_else(|| escolher(&ag, s(t, "area"))).ok_or("ninguém na equipe para pegar o ticket")?;
    let ultimo = t["historico"].as_array().and_then(|h| h.iter().rev().find(|x| x["quem"] == "revisao")).map(|h| s(h, "txt").to_string()).unwrap_or_default();
    let reprovou = if ultimo.is_empty() { String::new() } else { format!("\nA revisão reprovou da última vez: {ultimo}\nCorrija.\n") };
    let prompt = format!("PROJETO: {}\nTICKET {}: {}\n{}\n{}{reprovou}\nEntregue: 1) resumo de 2 linhas do que fez; 2) o código/arquivos completos em blocos; 3) como testar. Não invente requisito; dúvida = liste no fim.",
        s(t, "projeto"), s(t, "id"), s(t, "titulo"), s(t, "descricao"), (f.contexto)(t));
    let sistema = format!("{} {POLITICA} Responda em português do Brasil. Entregue CADA arquivo EXATAMENTE neste formato, sem cercas de markdown: uma linha '=== ARQUIVO: src/main.rs ===', depois o código DENTRO de um bloco de código markdown (três crases + linguagem), depois uma linha '=== FIM ==='. Nunca escreva código fora de bloco de código. Você só propõe código em texto; não executa nada.", s(&a, "instrucao"));
    let texto = (f.gerar)(&sistema, &prompt)?;
    let quem = s(&a, "nome");
    let arq = format!("{}-{}-{}.md", s(t, "id"), norm(quem), t["tentativas"].as_i64().unwrap_or(0));
    let conteudo = format!("# {} {}\nAgente: {quem}\n\n{texto}\n", s(t, "id"), s(t, "titulo"));
    let d = pasta_projeto(dados, t);
    std::fs::create_dir_all(&d).and_then(|_| std::fs::write(d.join(&arq), &conteudo)).map_err(|e| format!("não gravei a entrega: {e}"))?;
    (f.commit)(t, &arq, &conteudo, &extrair_arquivos(&conteudo));
    t["tentativas"] = json!(t["tentativas"].as_i64().unwrap_or(0) + 1);
    t["estado"] = json!("revisao");
    historico(t, quem, &format!("entregou {arq}"), ts, 12);
    Ok(())
}

fn aprovado(txt: &str) -> bool {
    norm(txt).starts_with("aprovado")
}

fn pedido_de_parecer(t: &Value, entrega: &str) -> String {
    format!("TICKET {}: {}\n{}\n\nENTREGA:\n{}", s(t, "id"), s(t, "titulo"), s(t, "descricao"), corta(entrega, 15000))
}

/// Revisao por colega pleno. A resposta precisa comecar com APROVADO; qualquer outra coisa reprova.
fn revisar(dados: &Path, t: &mut Value, entrega: &str, f: &Ferramentas, ts: f64) -> Result<bool, String> {
    let ag = agentes(dados);
    let outro = |a: &&Value| a["nome"] != t["responsavel"];
    let lider = ag.iter().filter(outro).find(|a| s(a, "nivel") == "pleno" && s(a, "area") == s(t, "area")).or_else(|| ag.iter().filter(outro).find(|a| ["pleno", "gestao"].contains(&s(a, "nivel"))));
    let sistema = format!("{} Revise com rigor. Responda começando EXATAMENTE com APROVADO ou REPROVADO, depois até 4 linhas de motivo.", lider.map_or("Você é revisor de código.", |r| s(r, "instrucao")));
    let txt = (f.ia)(&sistema, &pedido_de_parecer(t, entrega))?;
    let ok = aprovado(&txt);
    historico(t, "revisao", &format!("{}: {}", if ok { "APROVADO" } else { "REPROVADO" }, corta(txt.trim(), 300)), ts, 12);
    Ok(ok)
}

fn qa(dados: &Path, t: &mut Value, entrega: &str, f: &Ferramentas, ts: f64) -> Result<bool, String> {
    let (ok_b, log) = (f.build)(t);
    if ok_b == Some(false) {
        // nao compila ou teste falha: reprova sem gastar IA
        historico(t, "qa", &format!("REPROVADO (build): {}", cauda(&log, 300)), ts, 12);
        historico(t, "revisao", &format!("REPROVADO: o build falhou. {}", cauda(&log, 400)), ts, 12);
        return Ok(false);
    }
    let Some(a) = agentes(dados).into_iter().find(|x| s(x, "area") == "qa") else { return Ok(true) };
    let txt = (f.ia)(&format!("{} Responda começando EXATAMENTE com APROVADO ou REPROVADO, depois casos de teste curtos (até 6 linhas).", s(&a, "instrucao")), &pedido_de_parecer(t, entrega))?;
    let ok = aprovado(&txt);
    historico(t, "qa", &format!("{}: {}", if ok { "APROVADO" } else { "REPROVADO" }, corta(txt.trim(), 300)), ts, 12);
    Ok(ok)
}

/// O senior so entra em ticket critico ou depois de 3 tentativas.
pub fn senior_precisa(t: &Value) -> bool {
    t["critico"] == true || t["tentativas"].as_i64().unwrap_or(0) >= MAX_TENTATIVAS
}

/// Pede a revisao ao senior, dentro do teto diario. `false` = teto estourado ou nao deu para acionar.
pub fn senior_revisa(dados: &Path, t: &Value, f: &Ferramentas, agora: &Agora) -> bool {
    let (p, hoje) = (pasta(dados), agora.data.iso());
    if uso(&p, &hoje, "claude") >= limite(dados, "claude_dia") || somar_uso(&p, &hoje, "claude").is_err() {
        return false;
    }
    let proj = slug(s(t, "projeto"));
    let tarefa = format!("REVISAO SENIOR do ticket {} do projeto {proj} ({}). Leia os arquivos em dados/empresa_dev/projetos/{proj}/ (entregas dos devs), aponte falhas de arquitetura, segurança e testes e diga se pode ir para a entrega. NÃO altere nada fora dessa pasta, não faça push nem deploy. Responda em até 8 linhas começando com APROVADO ou REPROVADO.", s(t, "id"), s(t, "titulo"));
    (f.senior)(t, &tarefa).is_ok()
}

/// Parecer do senior chegou: aprovado vai ao QA; reprovado volta aos devs com o ciclo zerado.
pub fn senior_resposta(dados: &Path, id: &str, texto: &str, ts: f64) -> Result<(), String> {
    let mut todos = tickets(dados);
    for t in todos.iter_mut().filter(|t| t["id"] == id) {
        let ok = aprovado(texto);
        historico(t, "senior", &format!("{}: {}", if ok { "APROVADO" } else { "REPROVADO" }, corta(texto.trim(), 300)), ts, 12);
        t["estado"] = json!(if ok { "qa" } else { "desenvolvimento" });
        if !ok {
            t["tentativas"] = json!(0);
        }
    }
    gravar_tickets(dados, &todos)
}

fn ultima_entrega(dados: &Path, t: &Value) -> String {
    let prefixo = format!("{}-", s(t, "id"));
    let mut arqs: Vec<_> = std::fs::read_dir(pasta_projeto(dados, t)).into_iter().flatten().flatten()
        .filter(|e| e.file_name().to_str().is_some_and(|n| n.starts_with(&prefixo) && n.ends_with(".md")))
        .map(|e| (e.metadata().and_then(|m| m.modified()).ok(), e.path())).collect();
    arqs.sort();
    arqs.last().and_then(|(_, p)| std::fs::read_to_string(p).ok()).unwrap_or_default()
}

/// Da UM passo no ticket (uma chamada de IA por passo). Devolve o que aconteceu.
pub fn avancar(dados: &Path, t: &mut Value, f: &Ferramentas, agora: &Agora, ts: f64) -> Result<String, String> {
    let (id, est, tent) = (s(t, "id").to_string(), s(t, "estado").to_string(), t["tentativas"].as_i64().unwrap_or(0));
    if ATIVOS.contains(&est.as_str()) && tent >= MAX_TENTATIVAS_TOTAL {
        t["estado"] = json!("bloqueado");
        historico(t, "sistema", &format!("parou após {tent} tentativas: precisa do dono"), ts, 12);
        if let Some(g) = agentes(dados).iter().find(|a| s(a, "nivel") == "gestao") {
            (f.recado)(s(g, "nome"), "Jarvis", "alerta", &format!("{id} ({}) travou após {tent} tentativas e precisa de você.", corta(s(t, "titulo"), 60)));
        }
        return Ok(format!("{id} travou: precisa do dono"));
    }
    match est.as_str() {
        "novo" | "desenvolvimento" => {
            t["estado"] = json!("desenvolvimento");
            desenvolver(dados, t, f, ts)?;
            Ok(format!("{id} {} entregou (tentativa {})", t["responsavel"].as_str().unwrap_or("?"), t["tentativas"]))
        }
        "revisao" => {
            let entrega = ultima_entrega(dados, t);
            if (senior_precisa(t) || config(dados)["revisao_senior"] == true) && senior_revisa(dados, t, f, agora) {
                t["estado"] = json!("bloqueado"); // espera o parecer (chega em senior_resposta)
                historico(t, "senior", "pedido de revisão enviado ao Claude", ts, 12);
                return Ok(format!("{id} foi para o sênior (Claude)"));
            }
            let ok = revisar(dados, t, &entrega, f, ts)?;
            t["estado"] = json!(if ok { "qa" } else { "desenvolvimento" });
            Ok(format!("{id} {}", if ok { "aprovado na revisão" } else { "reprovado na revisão, volta ao dev" }))
        }
        "qa" => {
            let entrega = ultima_entrega(dados, t);
            let ok = qa(dados, t, &entrega, f, ts)?;
            t["estado"] = json!(if ok { "pronto" } else { "desenvolvimento" });
            Ok(format!("{id} {}", if ok { "passou no QA: PRONTO para o OK do dono" } else { "reprovou no QA, volta ao dev" }))
        }
        _ => Ok(format!("{id} {est}")),
    }
}

/// Um ciclo: a cada rodada, ate `paralelo` tickets de PROJETOS DIFERENTES (no mesmo projeto, um por vez), dentro do teto diario de passos.
/// `ignorar_teto` so a pedido do dono. Devolve a lista do que foi feito.
pub fn trabalhar(dados: &Path, f: &Ferramentas, agora: &Agora, ts: f64, maximo: Option<i64>, paralelo: usize, ignorar_teto: bool) -> Vec<String> {
    let (p, hoje) = (pasta(dados), agora.data.iso());
    if !travar(&p, ts, 900.0) {
        return vec![];
    }
    let teto = limite(dados, "passos_dia");
    let falta = if ignorar_teto { maximo.unwrap_or(50) } else { teto - uso(&p, &hoje, "passos") };
    let mut restante = falta.min(maximo.unwrap_or(teto));
    let mut feitos = vec![];
    while restante > 0 {
        let mut lote: Vec<Value> = vec![];
        for x in tickets(dados) {
            if ATIVOS.contains(&s(&x, "estado")) && x["parado"].is_null() && !lote.iter().any(|l| l["projeto"] == x["projeto"]) && (lote.len() as i64) < (paralelo as i64).min(restante) {
                lote.push(x);
            }
        }
        if lote.is_empty() {
            break;
        }
        for t in lote.iter_mut() {
            let r = avancar(dados, t, f, agora, ts).unwrap_or_else(|e| {
                historico(t, "sistema", &format!("erro: {}", corta(&e, 380)), ts, 12);
                t["estado"] = json!("bloqueado");
                format!("{} erro: {}", s(t, "id"), corta(&e, 80))
            });
            feitos.push(r);
            let _ = somar_uso(&p, &hoje, "passos");
            restante -= 1;
        }
        // rele o arquivo: o parecer do senior pode ter mexido em outro ticket no meio do ciclo
        let mut atuais = tickets(dados);
        for novo in &lote {
            if let Some(v) = atuais.iter_mut().find(|v| v["id"] == novo["id"]) {
                *v = novo.clone();
            }
        }
        if gravar_tickets(dados, &atuais).is_err() {
            break; // sem gravar, repetir o passo gastaria IA a toa
        }
    }
    destravar(&p);
    feitos
}

/// Estaciona o ticket em "desenvolvimento" com o motivo no historico (ex.: commit e build nao ligados). A entrega em texto continua
/// na pasta do projeto; `trabalhar` pula ticket com `parado` ate alguem tirar o campo.
pub fn parar(dados: &Path, id: &str, motivo: &str, ts: f64) -> Result<(), String> {
    let mut todos = tickets(dados);
    for t in todos.iter_mut().filter(|t| t["id"] == id) {
        t["estado"] = json!("desenvolvimento");
        t["parado"] = json!(motivo);
        historico(t, "sistema", &format!("parado: {motivo}"), ts, 12);
    }
    gravar_tickets(dados, &todos)
}

/// OK de entrega: ticket "pronto" vira "entregue" e a branch e mesclada (closure). So vale quando o pedido veio do dono; deploy e publicacao continuam manuais.
pub fn aprovar(dados: &Path, id: &str, pedido_do_dono: bool, ts: f64, mesclar: &dyn Fn(&Value) -> bool) -> String {
    if !pedido_do_dono {
        return "Só o dono aprova a entrega de um ticket.".into();
    }
    let mut todos = tickets(dados);
    let Some(t) = todos.iter_mut().find(|x| s(x, "id").eq_ignore_ascii_case(id.trim())) else { return "Ticket não encontrado.".into() };
    if t["estado"] != "pronto" {
        return format!("{} está em {}, ainda não está pronto.", s(t, "id"), s(t, "estado"));
    }
    t["estado"] = json!("entregue");
    historico(t, "dono", "OK de entrega", ts, 12);
    let t = t.clone();
    if let Err(e) = gravar_tickets(dados, &todos) {
        return e;
    }
    let merge = mesclar(&t);
    if merge {
        // copias de leitura: o merge traz os arquivos de volta (so apaga se o merge deu certo, para nao perder a entrega)
        let prefixo = format!("{}-", s(&t, "id"));
        for e in std::fs::read_dir(pasta_projeto(dados, &t)).into_iter().flatten().flatten() {
            if e.file_name().to_str().is_some_and(|n| n.starts_with(&prefixo) && n.ends_with(".md")) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    format!("{} entregue{} Os arquivos estão em dados/empresa_dev/projetos/{}/.", s(&t, "id"), if merge { " e mesclado na main." } else { "." }, slug(s(&t, "projeto")))
}

pub fn quadro(dados: &Path, agora: &Agora) -> String {
    let (todos, empresa) = (tickets(dados), s(&config(dados), "empresa").to_string());
    if todos.is_empty() {
        return format!("{empresa}: nenhum ticket ainda.");
    }
    let linhas: Vec<String> = todos[todos.len().saturating_sub(15)..].iter()
        .map(|t| format!("• {} [{}] {}: {} → {}", s(t, "id"), s(t, "estado"), s(t, "projeto"), s(t, "titulo"), t["responsavel"].as_str().unwrap_or("ninguém"))).collect();
    format!("{empresa} ({}/{} passos hoje):\n{}", uso(&pasta(dados), &agora.data.iso(), "passos"), limite(dados, "passos_dia"), linhas.join("\n"))
}

pub fn situacao(dados: &Path, agora: &Agora) -> Value {
    let (todos, hoje) = (tickets(dados), agora.data.iso());
    let por: serde_json::Map<String, Value> = ESTADOS.iter().map(|e| (e.to_string(), json!(todos.iter().filter(|t| t["estado"] == *e).count()))).collect();
    json!({ "empresa": config(dados)["empresa"], "agentes": agentes(dados).len(), "tickets": por, "claude_hoje": uso(&pasta(dados), &hoje, "claude"), "passos_hoje": uso(&pasta(dados), &hoje, "passos") })
}

/// Daily curta: resume o quadro e posta no barramento em nome de quem e da gestao.
pub fn daily(dados: &Path, agora: &Agora, f: &Ferramentas) -> String {
    let sit = situacao(dados, agora);
    let partes: Vec<String> = ESTADOS.iter().filter(|e| sit["tickets"][**e].as_i64().unwrap_or(0) > 0).map(|e| format!("{e} {}", sit["tickets"][*e])).collect();
    let txt = format!("Daily {}: {}", s(&sit, "empresa"), partes.join(", "));
    if let Some(g) = agentes(dados).iter().find(|a| s(a, "nivel") == "gestao") {
        (f.recado)(s(g, "nome"), "Jarvis", "status", &corta(&txt, 200));
    }
    txt
}

/// Laco da rotina (~10 min): daily 1x por dia entre 9h e 18h em dia util, e um passo por ciclo.
pub fn tick(dados: &Path, f: &Ferramentas, agora: &Agora, ts: f64, enviar: &dyn Fn(&str)) -> Vec<String> {
    if !(9..18).contains(&agora.hora) || agora.data.semana() >= 5 {
        return vec![];
    }
    let (arq, hoje) = (pasta(dados).join("daily.json"), agora.data.iso());
    if ler(&arq).map_or(true, |d| d["dia"] != hoje.as_str()) && gravar(&arq, &json!({ "dia": hoje })).is_ok() {
        enviar(&daily(dados, agora, f));
    }
    trabalhar(dados, f, agora, ts, Some(1), 3, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;
    use std::cell::RefCell;

    fn dir(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-empresa_dev-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let a = |n: &str, area: &str, nivel: &str, modelo: &str| json!({ "nome": n, "papel": area, "area": area, "nivel": nivel, "modelo": modelo, "instrucao": format!("Você é {n}.") });
        gravar(&d.join("empresa_dev/equipe.json"), &json!({ "empresa": "Oficina Teste", "agentes": [a("Ana", "gestao", "gestao", "gratis"), a("Bia", "front", "pleno", "gratis"),
            a("Caio", "back", "pleno", "gratis"), a("Duda", "api", "pleno", "gratis"), a("Edu", "junior", "junior", "gratis"), a("Eva", "qa", "pleno", "gratis"), a("Sol", "senior", "senior", "claude")] })).unwrap();
        d
    }

    fn quando(dia: i64, hora: u32) -> Agora {
        Agora { data: Data::civil(2026, 11, dia).unwrap(), hora, min: 0, seg: 0 }
    }

    struct Falsa {
        fila: RefCell<Vec<&'static str>>,
        senior: RefCell<Vec<String>>,
        recados: RefCell<Vec<String>>,
        commits: RefCell<Vec<Vec<String>>>,
        build: Option<bool>,
    }

    impl Falsa {
        fn nova(respostas: &[&'static str]) -> Falsa {
            Falsa { fila: RefCell::new(respostas.to_vec()), senior: RefCell::default(), recados: RefCell::default(), commits: RefCell::default(), build: None }
        }
        fn roda<R>(&self, f: impl FnOnce(&Ferramentas) -> R) -> R {
            let ia = |_: &str, _: &str| -> Result<String, String> {
                let mut q = self.fila.borrow_mut();
                Ok(if q.is_empty() { "APROVADO ok".to_string() } else { q.remove(0).to_string() })
            };
            f(&Ferramentas { gerar: &ia, ia: &ia, contexto: &|_| String::new(),
                commit: &|_, _, _, arqs| { self.commits.borrow_mut().push(arqs.keys().cloned().collect()); true },
                build: &|_| (self.build, "error[E0001]: quebrou".into()),
                senior: &|t, _| { self.senior.borrow_mut().push(t["id"].as_str().unwrap().to_string()); Ok(()) },
                recado: &|de, para, tipo, _| self.recados.borrow_mut().push(format!("{de}>{para}:{tipo}")) })
        }
    }

    #[test]
    fn ticket_recusa_segredo_e_roteia_por_area() {
        let d = dir("abrir");
        for ruim in ["guardar a senha do cliente", "ler o token da conta", "campo de CPF no cadastro", "pagar com cartão", "chave pix na tela", "usar a api_key nova"] {
            assert!(abrir(&d, "loja", ruim, "", "", false, 1.0).unwrap_err().contains("recusado"), "{ruim}");
        }
        assert!(abrir(&d, "loja", "abc", "", "", false, 1.0).unwrap_err().contains("sem título"));
        assert!(tickets(&d).is_empty());
        let (t, _) = abrir(&d, "Loja Líbano", "Criar endpoint de pedidos", "rota REST para listar pedidos", "", false, 1.0).unwrap();
        assert_eq!((t["area"].as_str(), t["responsavel"].as_str(), t["projeto"].as_str(), t["id"].as_str()), (Some("api"), Some("Duda"), Some("loja-libano"), Some("T001")));
        let (t2, msg) = abrir(&d, "loja", "Ajustar botão do site", "css do botão", "", false, 1.0).unwrap();
        assert_eq!((t2["area"].as_str(), t2["responsavel"].as_str()), (Some("front"), Some("Bia")));
        assert!(msg.starts_with("T002 aberto em loja"));
        assert_eq!(abrir(&d, "../../fora", "Organizar a papelada", "", "", false, 1.0).unwrap().0["responsavel"], "Edu"); // area desconhecida: junior
        assert_eq!(slug("../../Fora de Casa"), "fora-de-casa");
    }

    #[test]
    fn fluxo_completo_ate_pronto_e_reprovacao_volta() {
        let d = dir("fluxo");
        let f = Falsa::nova(&["=== ARQUIVO: web/index.html ===\n<p>v1</p>\n=== FIM ===", "REPROVADO falta tratar erro", "codigo v2", "APROVADO bom", "Aprovado, testes ok"]);
        abrir(&d, "app", "Criar tela de login", "html css botao", "", false, 1.0).unwrap();
        let feitos = f.roda(|x| trabalhar(&d, x, &quando(2, 10), 100.0, None, 3, false));
        let t = &tickets(&d)[0];
        assert_eq!((t["estado"].as_str(), t["tentativas"].as_i64(), feitos.len()), (Some("pronto"), Some(2), 5));
        assert_eq!(std::fs::read_dir(d.join("empresa_dev/projetos/app")).unwrap().count(), 2);
        assert_eq!(f.commits.borrow()[0], vec!["web/index.html".to_string()]);
        assert_eq!(uso(&d.join("empresa_dev"), "2026-11-02", "passos"), 5);
        assert!(!d.join("empresa_dev/.trabalhando").exists());
        // aprovar: so o dono, so o que esta pronto
        assert!(aprovar(&d, "t001", false, 1.0, &|_| true).contains("Só o dono"));
        assert_eq!(tickets(&d)[0]["estado"], "pronto");
        assert!(aprovar(&d, "T009", true, 1.0, &|_| true).contains("não encontrado"));
        assert!(aprovar(&d, "t001", true, 1.0, &|_| true).contains("entregue e mesclado"));
        assert_eq!(std::fs::read_dir(d.join("empresa_dev/projetos/app")).unwrap().count(), 0);
        assert!(aprovar(&d, "T001", true, 1.0, &|_| true).contains("ainda não está pronto"));
        assert!(quadro(&d, &quando(2, 10)).contains("• T001 [entregue] app"));
    }

    #[test]
    fn teto_diario_e_senior_so_quando_critico() {
        let d = dir("teto");
        let f = Falsa::nova(&[]);
        abrir(&d, "app", "Banco de dados do estoque", "sql", "", true, 1.0).unwrap();
        abrir(&d, "site", "Outro ajuste simples de css", "css", "", false, 1.0).unwrap();
        let agora = quando(2, 10);
        f.roda(|x| trabalhar(&d, x, &agora, 1.0, Some(4), 3, false)); // projetos diferentes andam juntos: 2 entregas, 2 revisoes
        let t = tickets(&d);
        assert_eq!((f.senior.borrow().clone(), t[0]["estado"].as_str(), t[1]["estado"].as_str()), (vec!["T001".to_string()], Some("bloqueado"), Some("qa")));
        senior_resposta(&d, "T001", "REPROVADO: falta índice", 2.0).unwrap();
        assert_eq!((tickets(&d)[0]["estado"].as_str(), tickets(&d)[0]["tentativas"].as_i64()), (Some("desenvolvimento"), Some(0)));
        // teto do senior: depois de 3 no dia, a revisao cai no colega
        for _ in 0..2 {
            somar_uso(&d.join("empresa_dev"), "2026-11-02", "claude").unwrap();
        }
        assert!(!f.roda(|x| senior_revisa(&d, &t[0], x, &agora)));
        // teto de passos: 6 por dia, 4 ja gastos
        let feitos = f.roda(|x| trabalhar(&d, x, &agora, 1.0, None, 3, false));
        assert_eq!((feitos.len(), uso(&d.join("empresa_dev"), "2026-11-02", "passos")), (2, 6));
        assert!(f.roda(|x| trabalhar(&d, x, &agora, 1.0, None, 3, false)).is_empty());
        assert_eq!(f.roda(|x| trabalhar(&d, x, &agora, 1.0, Some(1), 3, true)).len(), 1); // so a pedido do dono
        // ciclo ja em andamento (trava de 15 min) nao comeca outro
        std::fs::write(d.join("empresa_dev/.trabalhando"), "1000").unwrap();
        assert!(f.roda(|x| trabalhar(&d, x, &quando(3, 10), 1500.0, None, 3, false)).is_empty());
        assert!(!f.roda(|x| trabalhar(&d, x, &quando(3, 10), 2000.0, Some(1), 3, false)).is_empty());
        assert!(f.roda(|x| senior_revisa(&d, &t[0], x, &quando(3, 10)))); // dia novo zera
    }

    #[test]
    fn build_quebrado_reprova_sem_ia_e_seis_tentativas_param() {
        let d = dir("build");
        let mut f = Falsa::nova(&["codigo", "APROVADO"]);
        f.build = Some(false);
        abrir(&d, "app", "Criar tela de login", "html", "", false, 1.0).unwrap();
        let feitos = f.roda(|x| trabalhar(&d, x, &quando(2, 10), 1.0, Some(3), 3, false));
        assert!(feitos[2].contains("reprovou no QA"));
        let t = tickets(&d)[0].clone();
        assert!(f.fila.borrow().is_empty() && t["estado"] == "desenvolvimento");
        assert!(t["historico"].as_array().unwrap().iter().any(|h| h["quem"] == "revisao" && h["txt"].as_str().unwrap().contains("o build falhou")));
        let mut todos = tickets(&d);
        todos[0]["tentativas"] = json!(6);
        gravar_tickets(&d, &todos).unwrap();
        let feitos = f.roda(|x| trabalhar(&d, x, &quando(2, 10), 1.0, Some(1), 3, false));
        assert!(feitos[0].contains("travou") && tickets(&d)[0]["estado"] == "bloqueado");
        assert_eq!(f.recados.borrow().clone(), vec!["Ana>Jarvis:alerta".to_string()]);
        // erro da IA bloqueia o ticket em vez de repetir
        abrir(&d, "outro", "Criar tela de cadastro", "html", "", false, 1.0).unwrap();
        let erro = |_: &str, _: &str| -> Result<String, String> { Err("sem provedor".into()) };
        let r = f.roda(|x| trabalhar(&d, &Ferramentas { gerar: &erro, ..*x }, &quando(2, 10), 1.0, Some(1), 3, false));
        assert!(r[0].contains("erro: sem provedor") && tickets(&d)[1]["estado"] == "bloqueado");
    }

    #[test]
    fn tick_so_em_dia_util_e_horario() {
        let d = dir("tick");
        let f = Falsa::nova(&[]);
        let avisos = RefCell::new(vec![]);
        let enviar = |m: &str| avisos.borrow_mut().push(m.to_string());
        abrir(&d, "app", "Criar tela de login", "html", "", false, 1.0).unwrap();
        assert!(f.roda(|x| tick(&d, x, &quando(1, 10), 1.0, &enviar)).is_empty()); // domingo
        assert!(f.roda(|x| tick(&d, x, &quando(2, 22), 1.0, &enviar)).is_empty()); // noite
        assert!(f.roda(|x| tick(&d, x, &quando(2, 8), 1.0, &enviar)).is_empty());
        assert_eq!(f.roda(|x| tick(&d, x, &quando(2, 10), 1.0, &enviar)).len(), 1);
        assert_eq!(f.roda(|x| tick(&d, x, &quando(2, 11), 1.0, &enviar)).len(), 1);
        assert_eq!(avisos.borrow().clone(), vec!["Daily Oficina Teste: novo 1".to_string()]); // daily uma vez por dia
        assert_eq!(f.recados.borrow().clone(), vec!["Ana>Jarvis:status".to_string()]);
    }

    #[test]
    fn extrai_arquivos_e_barra_caminho_perigoso() {
        let r = extrair_arquivos("Resumo.\n**app.py**\n```python\nprint('oi')\n```\n```html\n<!-- web/index.html -->\n<p>oi</p>\n```\n`../../fora.py`\n```python\nx=1\n```\n`run.exe`\n```\nlixo\n```\n");
        assert_eq!(r.keys().collect::<Vec<_>>(), vec!["app.py", "web/index.html"]);
        assert_eq!(r["app.py"].trim(), "print('oi')");
        let r = extrair_arquivos("resumo\r\n=== ARQUIVO: src/main.rs ===\r\nrust\nCopiar código\nfn main() {}\n=== FIM ===\n=== ARQUIVO: ../x.rs ===\nruim\n=== FIM ===\n=== ARQUIVO: /etc/x.sh ===\nok\n=== FIM ===");
        assert_eq!(r, BTreeMap::from([("src/main.rs".to_string(), "fn main() {}".to_string()), ("etc/x.sh".to_string(), "ok".to_string())]));
        for ruim in ["build.rs", "sub/build.rs", ".env", ".git/config.txt", "jogo.exe", "roda.bat", "a/../../b.rs", "semextensao"] {
            assert!(extrair_arquivos(&format!("=== ARQUIVO: {ruim} ===\nfn main(){{}}\n=== FIM ===")).is_empty(), "{ruim}");
        }
    }

    #[test]
    fn limites_e_equipe_padrao() {
        let d = std::env::temp_dir().join(format!("braniac-empresa_dev-{}-padrao", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        assert_eq!((nomes(&d).len(), limite(&d, "passos_dia"), limite(&d, "claude_dia")), (7, 6, 3));
        gravar(&d.join("empresa_dev/equipe.json"), &json!({ "empresa": "X", "agentes": [], "limites": { "passos_dia": "15", "claude_dia": -4 } })).unwrap();
        assert_eq!((limite(&d, "passos_dia"), limite(&d, "claude_dia")), (15, 0));
        assert!(abrir(&d, "p", "Titulo valido", "", "", false, 1.0).unwrap().0["responsavel"].is_null());
        assert_eq!(sala_de(&json!({ "area": "qa" })), "Qualidade");
    }
}
