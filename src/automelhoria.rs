//! Automelhoria: o sistema muda o proprio codigo COM TRAVAS. Fluxo: ponto de volta limpo -> testes antes -> plano -> implementador ->
//! o Braniac CONFERE sozinho, sem confiar no implementador -> aprovado: commit "automelhoria: ..."; reprovado: tudo desfeito.
//! Travas (o mais importante): arquivo protegido intocado; nenhuma linha com identificador de trava criada, mudada ou apagada; o CORPO das
//! funcoes/valores de trava identico antes e depois; suite inteira passando; nenhum teste a menos; uma por vez; no maximo 4 por dia;
//! nao comeca se os testes ja falhavam. Desfazer a ultima so se nada veio depois dela e se os testes continuam passando.
//! Ficou de fora (entra como closure em `Ambiente`): git, rodar os testes, o conselho que faz o plano, o implementador (agente ou IAs
//! gratis), atualizar o grafo e reiniciar o servidor.
//! Diferencas do Python: o corpo das travas e achado por recuo (bloco que comeca na coluna 0), nao pela arvore sintatica, e vale tambem
//! para arquivo .rs; na duvida a comparacao e mais rigorosa (comentario colado ao bloco conta). Alem dos protegidos do Python entram
//! `src/automelhoria.rs` e `src/dev_livre.rs`, os equivalentes deste porte.
#![allow(dead_code)]
use crate::empresa_dev::{cauda, corta, gravar, ler, rx};
use regex::Regex;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub const LIMITE_DIA: usize = 4;
pub const PROTEGIDOS: [&str; 7] = ["tests/conftest.py", "automelhoria.py", "tests/test_automelhoria.py", "tests/test_seguranca_atender.py", "tests/test_revisao_1410.py",
    "src/automelhoria.rs", "src/dev_livre.rs"];
/// Nenhuma linha que contenha um destes pode ser criada, mudada ou apagada por automelhoria.
pub const TRAVAS: [&str; 11] = ["def _gate(", "def _gate_envio(", "FAMILIA_LIVRE", "def tem_dado_real", "SENSIVEL", "def trava_destino", "def destino_confere", "nao_responder", "def _bloquear", "PROTEGIDOS", "TRAVAS"];
/// Funcoes e valores cujo corpo inteiro nao pode mudar.
pub const TRAVAS_DEFS: [&str; 12] = ["_gate", "_gate_envio", "tem_dado_real", "trava_destino", "destino_confere", "_bloquear", "_eh_familia", "FAMILIA_LIVRE", "SENSIVEL", "PROTEGIDOS", "TRAVAS", "TRAVAS_DEFS"];

/// Tudo que sai do processo. `git(args)` devolve (codigo, saida) e roda na raiz do repositorio do codigo.
pub struct Ambiente<'a> {
    pub raiz: &'a Path,
    pub git: &'a dyn Fn(&[&str]) -> (i32, String),
    /// {"ok", "passaram", "falharam", "falhas": [...]}
    pub testes: &'a dyn Fn() -> Value,
    pub plano: &'a dyn Fn(&str) -> String,
    /// prompt -> (codigo, o que o implementador disse)
    pub implementar: &'a dyn Fn(&str) -> (i32, String),
    pub atualizar_grafo: &'a dyn Fn(),
    pub reiniciar: &'a dyn Fn(),
    pub avisar: &'a dyn Fn(&str),
}

fn head(a: &Ambiente) -> String {
    (a.git)(&["rev-parse", "HEAD"]).1.trim().to_string()
}

/// Rastreados que mudaram desde `base` + novos nao ignorados.
fn arquivos_mudados(a: &Ambiente, base: &str) -> Vec<String> {
    let txt = format!("{}\n{}", (a.git)(&["diff", "--name-only", base]).1, (a.git)(&["ls-files", "--others", "--exclude-standard"]).1);
    let s: BTreeSet<String> = txt.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("warning:") && !l.starts_with("fatal:")).map(|l| l.replace('\\', "/")).collect();
    s.into_iter().collect()
}

fn linhas_alteradas(a: &Ambiente, base: &str) -> Vec<String> {
    (a.git)(&["add", "-N", "."]); // arquivo novo entra no diff
    (a.git)(&["diff", "-U0", base]).1.lines().filter(|l| (l.starts_with('+') || l.starts_with('-')) && !l.starts_with("+++") && !l.starts_with("---")).map(|l| l[1..].to_string()).collect()
}

/// {nome: codigo} dos blocos de topo (comecam na coluna 0) cujo nome esta em TRAVAS_DEFS: o bloco INTEIRO, nao so a primeira linha.
pub fn defs(fonte: &str) -> BTreeMap<String, String> {
    static NOME: OnceLock<Regex> = OnceLock::new();
    let nome = rx(&NOME, r"^(?:pub(?:\([a-z]+\))?\s+)?(?:(?:async\s+)?(?:def|fn)|class|const|static(?:\s+mut)?)\s+(\w+)|^(\w+)\s*(?::[^=]+)?=[^=]");
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    let mut atual: Option<String> = None;
    for l in fonte.lines() {
        let topo = l.chars().next().is_some_and(|c| !c.is_whitespace() && !matches!(c, ')' | ']' | '}' | '#'));
        if topo {
            atual = nome.captures(l).and_then(|c| c.get(1).or(c.get(2))).map(|m| m.as_str().to_string()).filter(|n| TRAVAS_DEFS.contains(&n.as_str()));
            if let Some(n) = &atual {
                out.insert(n.clone(), String::new()); // redefinicao: vale a ultima, como na execucao
            }
        }
        if let (Some(n), false) = (&atual, l.trim().is_empty()) {
            let corpo = out.entry(n.clone()).or_default();
            corpo.push_str(l.trim_end());
            corpo.push('\n');
        }
    }
    out
}

/// Nomes de travas cujo codigo mudou (bloco inteiro antes x depois) em cada arquivo de codigo alterado (testes ficam de fora).
pub fn travas_alteradas(a: &Ambiente, base: &str, mudados: &[String]) -> Vec<String> {
    let mut alt = BTreeSet::new();
    for arq in mudados.iter().filter(|x| (x.ends_with(".py") || x.ends_with(".rs")) && !x.starts_with("tests/")) {
        let (cod, antes) = (a.git)(&["show", &format!("{base}:{arq}")]);
        let antes = defs(if cod == 0 { &antes } else { "" });
        let depois = defs(&std::fs::read_to_string(a.raiz.join(arq)).unwrap_or_default());
        alt.extend(antes.keys().chain(depois.keys()).filter(|k| antes.get(*k) != depois.get(*k)).cloned());
    }
    alt.into_iter().collect()
}

fn desfazer_ate(a: &Ambiente, base: &str) {
    (a.git)(&["reset", "--hard", base]);
    (a.git)(&["clean", "-fd"]); // so arquivo novo NAO ignorado: a pasta de dados nunca e tocada
}

fn arq(dados: &Path) -> PathBuf {
    dados.join("automelhoria.json")
}

fn registrar(dados: &Path, item: Value) {
    let mut d = ler(&arq(dados)).filter(Value::is_object).unwrap_or_else(|| json!({}));
    let mut h = d["historico"].as_array().cloned().unwrap_or_default();
    h.push(item);
    d["historico"] = json!(h[h.len().saturating_sub(50)..]);
    let _ = gravar(&arq(dados), &d);
}

/// Quantas automelhorias (aplicadas ou nao) ja foram tentadas no dia `hoje` (AAAA-MM-DD).
pub fn feitas_hoje(dados: &Path, hoje: &str) -> usize {
    ler(&arq(dados)).map_or(0, |d| d["historico"].as_array().map_or(0, |h| h.iter().filter(|x| x["dia"] == hoje).count()))
}

/// Nome + primeira linha da documentacao de cada modulo (contexto tecnico para o plano, sem dados pessoais).
pub fn mapa_modulos(pasta: &Path, maximo: usize) -> String {
    let mut arqs: Vec<PathBuf> = std::fs::read_dir(pasta).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "py" || x == "rs")).collect();
    arqs.sort();
    arqs.iter().filter_map(|p| {
        let txt = std::fs::read_to_string(p).ok()?;
        let doc = txt.lines().find_map(|l| l.strip_prefix("//!").or_else(|| l.split_once("\"\"\"").map(|x| x.1))).unwrap_or("");
        Some(format!("{}: {}", p.file_stem()?.to_string_lossy(), corta(doc.trim().trim_end_matches("\"\"\""), maximo)))
    }).collect::<Vec<_>>().join("\n")
}

fn falhas(r: &Value) -> String {
    r["falhas"].as_array().into_iter().flatten().filter_map(Value::as_str).take(3).collect::<Vec<_>>().join(", ")
}

/// (ok, motivo). Nao confia no implementador: confere tudo de novo. `antes` = quantos testes passavam.
pub fn verificar(a: &Ambiente, base: &str, antes: i64) -> (bool, String) {
    let mudados = arquivos_mudados(a, base);
    if mudados.is_empty() {
        return (false, "nenhum arquivo foi alterado".into());
    }
    let proibidos: Vec<&str> = mudados.iter().map(String::as_str).filter(|m| PROTEGIDOS.contains(m)).collect();
    if !proibidos.is_empty() {
        return (false, format!("mexeu em arquivo protegido: {}", proibidos.join(", ")));
    }
    let mut travas: BTreeSet<String> = travas_alteradas(a, base, &mudados).into_iter().collect();
    for l in linhas_alteradas(a, base) {
        travas.extend(TRAVAS.iter().filter(|t| l.contains(**t)).map(|t| t.to_string()));
    }
    if !travas.is_empty() {
        return (false, format!("mexeu em trava de segurança ({})", travas.into_iter().collect::<Vec<_>>().join(", ")));
    }
    let r = (a.testes)();
    let passaram = r["passaram"].as_i64().unwrap_or(0);
    if r["ok"].as_bool() != Some(true) {
        return (false, format!("testes falharam ({} falha(s): {})", r["falharam"], falhas(&r)));
    }
    if passaram < antes {
        return (false, format!("sumiram testes ({passaram} agora, {antes} antes)"));
    }
    (true, format!("{} arquivo(s) alterado(s), {passaram} testes passando", mudados.len()))
}

pub fn prompt_implementador(pedido: &str, plano: &str) -> String {
    format!("Você é o IMPLEMENTADOR da AUTOMELHORIA do Braniac, acionado pelo próprio Jarvis (sem o dono na tela). Pedido do dono: {} | Plano do conselho de IAs: {} | \
        Regras: 1) mude só o necessário dentro da pasta do código (a menor mudança que resolve); 2) leia só os arquivos envolvidos; \
        3) NÃO altere os arquivos protegidos ({}) nem nenhuma trava de segurança ({}); 4) escreva ou ajuste testes para o que mudou; \
        5) não faça commit, não reinicie nada, não envie mensagens a ninguém e não toque na pasta de dados; \
        6) se o pedido exigir quebrar uma trava ou for perigoso, não mude nada e explique. \
        Ao terminar, responda em até 6 linhas, em português, o que mudou e em quais arquivos.", pedido.trim(), corta(plano.trim(), 3000), PROTEGIDOS.join(", "), TRAVAS.join(", "))
}

static EM_CURSO: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

struct Vez(PathBuf);

impl Vez {
    fn pegar(dados: &Path) -> Option<Vez> {
        let mut r = EM_CURSO.lock().ok()?;
        if r.iter().any(|p| p == dados) {
            return None;
        }
        r.push(dados.to_path_buf());
        Some(Vez(dados.to_path_buf()))
    }
}

impl Drop for Vez {
    fn drop(&mut self) {
        if let Ok(mut r) = EM_CURSO.lock() {
            r.retain(|p| *p != self.0);
        }
    }
}

fn fim(dados: &Path, hoje: &str, pedido: &str, ok: bool, texto: &str, commit: &str) -> String {
    registrar(dados, json!({ "dia": hoje, "pedido": corta(pedido, 200), "ok": ok, "commit": commit, "resultado": corta(texto, 500) }));
    format!("{}{texto}", if ok { "✅ Automelhoria aplicada: " } else { "❌ Automelhoria não aplicada: " })
}

/// A automelhoria inteira (bloqueante). `plano` pronto pula o conselho. `hoje` = AAAA-MM-DD. Devolve o texto final para o dono.
pub fn melhorar(dados: &Path, a: &Ambiente, pedido: &str, plano: Option<&str>, hoje: &str) -> String {
    let pedido = pedido.trim();
    if pedido.chars().count() < 8 {
        return "Qual melhoria você quer? Me diga o que mudar no Braniac/Jarvis.".into();
    }
    if feitas_hoje(dados, hoje) >= LIMITE_DIA {
        return format!("Já fiz {LIMITE_DIA} automelhorias hoje (limite de segurança). Amanhã eu continuo, ou peça ao agente de programação direto.");
    }
    let Some(_vez) = Vez::pegar(dados) else { return "Já estou no meio de uma automelhoria. Te aviso quando terminar.".into() };
    // ponto de volta limpo: o que ja estava mudado vira um commit proprio
    if !arquivos_mudados(a, "HEAD").is_empty() {
        (a.git)(&["add", "-A"]);
        (a.git)(&["commit", "-q", "-m", "estado antes da automelhoria"]);
    }
    let base = head(a);
    if base.is_empty() {
        return "Não comecei: não achei o repositório do código (sem ponto de volta, não mexo).".into();
    }
    let antes = (a.testes)();
    if antes["ok"].as_bool() != Some(true) {
        return format!("Não comecei: os testes já estavam falhando antes ({}). Peça para arrumar primeiro.", falhas(&antes));
    }
    (a.avisar)("🔧 Automelhoria: chamei o conselho de IAs para planejar.");
    let plano = plano.map(String::from).unwrap_or_else(|| (a.plano)(pedido));
    (a.avisar)("🔧 Plano pronto. O implementador está trabalhando; eu confiro tudo antes de aplicar.");
    let (cod, saida) = (a.implementar)(&prompt_implementador(pedido, &plano));
    let saida = saida.split("[stderr]").next().unwrap_or("").trim().to_string();
    if cod != 0 {
        desfazer_ate(a, &base);
        return fim(dados, hoje, pedido, false, &format!("o implementador falhou (código {cod}); nada foi mudado"), "");
    }
    let (ok, motivo) = verificar(a, &base, antes["passaram"].as_i64().unwrap_or(0));
    if !ok {
        desfazer_ate(a, &base);
        return fim(dados, hoje, pedido, false, &format!("{motivo}; desfiz tudo, o Braniac ficou como estava"), "");
    }
    (a.git)(&["add", "-A"]);
    (a.git)(&["commit", "-q", "-m", &format!("automelhoria: {}\n\nPlano do conselho:\n{}\n\nImplementador:\n{}", corta(pedido, 70), corta(&plano, 1500), cauda(&saida, 1500))]);
    (a.atualizar_grafo)();
    (a.reiniciar)();
    fim(dados, hoje, pedido, true, &format!("{motivo}. O que mudou: {}\nVou reiniciar em instantes. Para voltar atrás: 'Jarvis, desfaz a última automelhoria'.", cauda(&saida, 600)), &head(a))
}

/// git revert do ultimo commit "automelhoria:", so se nada mais veio depois dele (para nao desfazer trabalho de outra pessoa).
pub fn desfazer_ultima(dados: &Path, a: &Ambiente) -> String {
    let Some(_vez) = Vez::pegar(dados) else { return "Já estou no meio de uma automelhoria. Te aviso quando terminar.".into() };
    let log = (a.git)(&["log", "--format=%H %s", "-n", "15"]).1;
    let linhas: Vec<(&str, &str)> = log.lines().filter_map(|l| l.split_once(' ')).collect();
    let Some((hash, assunto)) = linhas.iter().find(|(_, s)| s.starts_with("automelhoria:")).copied() else { return "Não achei nenhuma automelhoria para desfazer.".into() };
    if linhas[0].0 != hash {
        return "Depois da última automelhoria já houve outras mudanças; peça ao agente de programação para desfazer com cuidado.".into();
    }
    let base = head(a);
    let (cod, out) = (a.git)(&["revert", "--no-edit", hash]);
    if cod != 0 {
        (a.git)(&["revert", "--abort"]);
        return format!("Não consegui desfazer: {}", cauda(&out, 200));
    }
    if (a.testes)()["ok"].as_bool() != Some(true) {
        desfazer_ate(a, &base);
        return "Desfazer quebraria os testes; deixei como estava. Peça ao agente de programação.".into();
    }
    (a.atualizar_grafo)();
    (a.reiniciar)();
    format!("↩️ Desfeita a automelhoria «{}». Vou reiniciar em instantes.", assunto.trim_start_matches("automelhoria:").trim())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;

    /// Repositorio de mentira: arquivos na pasta de teste; "git" so sabe o que este porte usa.
    struct Repo {
        raiz: PathBuf,
        base: RefCell<HashMap<String, String>>,
        log: RefCell<Vec<(String, String)>>,
        cmds: RefCell<Vec<String>>,
    }

    impl Repo {
        fn novo(nome: &str) -> Repo {
            let raiz = std::env::temp_dir().join(format!("braniac-automelhoria-{nome}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&raiz);
            let r = Repo { raiz, base: RefCell::default(), log: RefCell::new(vec![("h0".into(), "base".into())]), cmds: RefCell::default() };
            for (arq, txt) in [("app.py", "def soma(a, b):\n    return a + b\n"), ("jarvis.py", "def _gate(origem, o_que):\n    return None\n"), ("tests/conftest.py", "# protegido\n")] {
                r.escrever(arq, txt);
                r.base.borrow_mut().insert(arq.into(), txt.into());
            }
            r
        }

        fn escrever(&self, arq: &str, txt: &str) {
            let p = self.raiz.join(arq);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, txt).unwrap();
        }

        fn ler(&self, arq: &str) -> Option<String> {
            std::fs::read_to_string(self.raiz.join(arq)).ok()
        }

        fn atuais(&self) -> HashMap<String, String> {
            let mut m = HashMap::new();
            for dir in ["", "tests", "src"] {
                for e in std::fs::read_dir(self.raiz.join(dir)).into_iter().flatten().flatten().filter(|e| e.path().is_file()) {
                    let nome = format!("{}{}{}", dir, if dir.is_empty() { "" } else { "/" }, e.file_name().to_string_lossy());
                    m.insert(nome.clone(), self.ler(&nome).unwrap_or_default());
                }
            }
            m
        }

        fn git(&self, args: &[&str]) -> (i32, String) {
            self.cmds.borrow_mut().push(args.join(" "));
            let (base, atuais) = (self.base.borrow().clone(), self.atuais());
            match args {
                ["rev-parse", "HEAD"] => (0, self.log.borrow()[0].0.clone()),
                ["diff", "--name-only", _] => (0, base.iter().filter(|(k, v)| atuais.get(*k) != Some(v)).map(|(k, _)| k.clone()).collect::<Vec<_>>().join("\n")),
                ["ls-files", ..] => (0, atuais.keys().filter(|k| !base.contains_key(*k)).cloned().collect::<Vec<_>>().join("\n")),
                ["diff", "-U0", _] => {
                    let mut d = String::new();
                    for (k, novo) in &atuais {
                        let velho = base.get(k).cloned().unwrap_or_default();
                        if velho != *novo {
                            d += &format!("--- a/{k}\n+++ b/{k}\n");
                            d += &velho.lines().filter(|l| !novo.lines().any(|n| n == *l)).map(|l| format!("-{l}\n")).collect::<String>();
                            d += &novo.lines().filter(|l| !velho.lines().any(|n| n == *l)).map(|l| format!("+{l}\n")).collect::<String>();
                        }
                    }
                    (0, d)
                }
                ["show", alvo] => alvo.split_once(':').and_then(|(_, a)| base.get(a).cloned()).map_or((128, "fatal: nao existe".into()), |t| (0, t)),
                ["reset", "--hard", _] => {
                    for (k, v) in &base {
                        self.escrever(k, v);
                    }
                    (0, String::new())
                }
                ["clean", "-fd"] => {
                    for k in atuais.keys().filter(|k| !base.contains_key(*k)) {
                        let _ = std::fs::remove_file(self.raiz.join(k));
                    }
                    (0, String::new())
                }
                ["commit", "-q", "-m", msg] => {
                    *self.base.borrow_mut() = atuais;
                    let n = self.log.borrow().len();
                    self.log.borrow_mut().insert(0, (format!("h{n}"), msg.lines().next().unwrap_or("").to_string()));
                    (0, String::new())
                }
                ["log", ..] => (0, self.log.borrow().iter().map(|(h, s)| format!("{h} {s}")).collect::<Vec<_>>().join("\n")),
                ["revert", "--no-edit", _] => {
                    self.escrever("app.py", "def soma(a, b):\n    return a + b\n");
                    *self.base.borrow_mut() = self.atuais();
                    (0, String::new())
                }
                _ => (0, String::new()),
            }
        }

        /// Roda melhorar com um implementador que faz `mexer`; o 1o resultado dos testes e bom, os seguintes sao `depois`.
        fn rodar(&self, mexer: &dyn Fn(), depois: Value) -> (String, usize, usize) {
            let (reinicios, avisos, n) = (Cell::new(0), Cell::new(0), Cell::new(0));
            let a = Ambiente {
                raiz: &self.raiz, git: &|x| self.git(x),
                testes: &|| { n.set(n.get() + 1); if n.get() == 1 { json!({ "ok": true, "passaram": 10, "falharam": 0, "falhas": [] }) } else { depois.clone() } },
                plano: &|_| panic!("plano ja veio pronto"), implementar: &|p| { assert!(p.contains("deixar a soma mais clara") && p.contains("PROTEGIDOS")); mexer(); (0, "mudei app.py\n[stderr]\nruido".into()) },
                atualizar_grafo: &|| {}, reiniciar: &|| reinicios.set(reinicios.get() + 1), avisar: &|_| avisos.set(avisos.get() + 1),
            };
            (melhorar(&self.raiz.join("dados"), &a, "deixar a soma mais clara", Some("mudar app.py"), "2026-11-18"), reinicios.get(), avisos.get())
        }
    }

    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.raiz);
        }
    }

    fn bons(n: i64) -> Value {
        json!({ "ok": true, "passaram": n, "falharam": 0, "falhas": [] })
    }

    #[test]
    fn aplica_quando_tudo_passa() {
        let r = Repo::novo("aplica");
        let (txt, reinicios, avisos) = r.rodar(&|| r.escrever("app.py", "def soma(a, b):\n    \"\"\"Soma.\"\"\"\n    return a + b\n"), bons(10));
        assert!(txt.starts_with("✅ Automelhoria aplicada") && reinicios == 1 && avisos == 2 && !txt.contains("ruido"), "{txt}");
        assert_eq!(r.log.borrow()[0].1, "automelhoria: deixar a soma mais clara");
        assert_eq!(feitas_hoje(&r.raiz.join("dados"), "2026-11-18"), 1);
    }

    #[test]
    fn desfaz_se_os_testes_falham_ou_somem_e_apaga_arquivo_novo() {
        let r = Repo::novo("falha");
        let ruim = json!({ "ok": false, "passaram": 9, "falharam": 1, "falhas": ["x"] });
        let (txt, reinicios, _) = r.rodar(&|| r.escrever("app.py", "quebrado("), ruim.clone());
        assert!(txt.starts_with("❌") && txt.contains("testes falharam") && reinicios == 0);
        assert!(r.ler("app.py").unwrap().starts_with("def soma"));
        assert!(r.rodar(&|| r.escrever("app.py", "x = 1\n"), bons(7)).0.contains("sumiram testes"));
        let (txt, _, _) = r.rodar(&|| r.escrever("novo.py", "x("), ruim);
        assert!(txt.starts_with("❌") && r.ler("novo.py").is_none()); // arquivo novo tambem e desfeito
        assert!(r.rodar(&|| {}, bons(10)).0.contains("nenhum arquivo foi alterado"));
        assert_eq!(r.log.borrow().len(), 1); // nada virou commit
    }

    #[test]
    fn recusa_trava_e_arquivo_protegido() {
        let r = Repo::novo("trava");
        let (txt, reinicios, _) = r.rodar(&|| r.escrever("jarvis.py", "def _gate(origem, o_que):\n    return 'liberado'\n"), bons(10));
        assert!(txt.contains("trava de segurança") && txt.contains("_gate") && reinicios == 0 && r.ler("jarvis.py").unwrap().contains("return None"), "{txt}");
        let (txt, _, _) = r.rodar(&|| r.escrever("tests/conftest.py", "# liberado\n"), bons(10));
        assert!(txt.contains("arquivo protegido: tests/conftest.py") && r.ler("tests/conftest.py").unwrap() == "# protegido\n");
        // arquivo protegido NOVO e linha nova com identificador de trava tambem sao barrados
        assert!(r.rodar(&|| r.escrever("src/automelhoria.rs", "// outro\n"), bons(10)).0.contains("arquivo protegido: src/automelhoria.rs"));
        let r = Repo::novo("trava2"); // repositorio novo: o teto de 4 por dia tambem conta as recusadas
        assert!(r.rodar(&|| r.escrever("extra.py", "FAMILIA_LIVRE = True\n"), bons(10)).0.contains("FAMILIA_LIVRE"));
        assert!(r.rodar(&|| r.escrever("app.py", "def soma(a, b):\n    return a + b\n\nnao_responder = []\n"), bons(10)).0.contains("nao_responder"));
        assert_eq!(r.log.borrow().len(), 1);
    }

    #[test]
    fn defs_pega_o_bloco_inteiro_em_python_e_rust() {
        let py = "import x\nSENSIVEL = (\n    'a',\n    'b',\n)\n\ndef _gate(o):\n    if o:\n        return None\n\n    return 1\n\ndef outra():\n    pass\nx == 1\n";
        let d = defs(py);
        assert_eq!(d.keys().collect::<Vec<_>>(), ["SENSIVEL", "_gate"]);
        assert!(d["_gate"].ends_with("    return 1\n") && d["SENSIVEL"].contains("'b'"));
        assert_ne!(defs(&py.replace("return 1", "return 2"))["_gate"], d["_gate"]);
        assert_eq!(defs(&py.replace("pass", "return 9")), d); // funcao comum pode mudar
        let rs = "pub fn tem_dado_real(t: &str) -> bool {\n    t.len() > 3\n}\n\npub(crate) const PROTEGIDOS: [&str; 1] = [\"a\"];\nfn livre() {}\n";
        assert_eq!(defs(rs).keys().collect::<Vec<_>>(), ["PROTEGIDOS", "tem_dado_real"]);
        assert!(defs(rs)["tem_dado_real"].contains("t.len() > 3"));
    }

    #[test]
    fn limites_uma_por_vez_teto_do_dia_e_testes_ja_falhando() {
        let r = Repo::novo("limites");
        let dados = r.raiz.join("dados");
        let chamou = Cell::new(false);
        let a = Ambiente { raiz: &r.raiz, git: &|x| r.git(x), testes: &|| json!({ "ok": false, "falhas": ["t1"] }), plano: &|_| String::new(), implementar: &|_| { chamou.set(true); (0, String::new()) },
                           atualizar_grafo: &|| {}, reiniciar: &|| {}, avisar: &|_| {} };
        assert!(melhorar(&dados, &a, "curto", None, "2026-11-18").starts_with("Qual melhoria"));
        assert!(melhorar(&dados, &a, "melhorar qualquer coisa", None, "2026-11-18").contains("já estavam falhando antes (t1)") && !chamou.get());
        let vez = Vez::pegar(&dados).unwrap();
        assert!(melhorar(&dados, &a, "melhorar qualquer coisa", None, "2026-11-18").contains("no meio de uma automelhoria"));
        assert!(desfazer_ultima(&dados, &a).contains("no meio"));
        drop(vez);
        for _ in 0..LIMITE_DIA {
            registrar(&dados, json!({ "dia": "2026-11-18", "pedido": "x", "ok": true }));
        }
        assert!(melhorar(&dados, &a, "melhorar qualquer coisa", None, "2026-11-18").contains("limite de segurança"));
        assert!(!melhorar(&dados, &a, "melhorar qualquer coisa", None, "2026-11-19").contains("limite de segurança"));
        let falhou = Ambiente { implementar: &|_| (1, "erro".into()), testes: &|| bons(10), ..a };
        assert!(melhorar(&dados, &falhou, "melhorar qualquer coisa", Some("p"), "2026-11-19").contains("o implementador falhou (código 1)"));
    }

    #[test]
    fn desfazer_ultima_so_se_for_o_ultimo_commit_e_os_testes_passarem() {
        let r = Repo::novo("desfazer");
        let dados = r.raiz.join("dados");
        let (reinicios, ok) = (Cell::new(0), Cell::new(true));
        let a = Ambiente { raiz: &r.raiz, git: &|x| r.git(x), testes: &|| json!({ "ok": ok.get(), "passaram": 10 }), plano: &|_| String::new(), implementar: &|_| (0, String::new()),
                           atualizar_grafo: &|| {}, reiniciar: &|| reinicios.set(reinicios.get() + 1), avisar: &|_| {} };
        assert!(desfazer_ultima(&dados, &a).starts_with("Não achei"));
        r.rodar(&|| r.escrever("app.py", "def soma(a, b):\n    return b + a\n"), bons(10));
        ok.set(false);
        assert!(desfazer_ultima(&dados, &a).contains("quebraria os testes") && reinicios.get() == 0);
        r.escrever("app.py", "def soma(a, b):\n    return b + a\n");
        *r.base.borrow_mut() = r.atuais();
        ok.set(true);
        let txt = desfazer_ultima(&dados, &a);
        assert!(txt.starts_with("↩️") && txt.contains("«deixar a soma mais clara»") && reinicios.get() == 1 && r.ler("app.py").unwrap().contains("return a + b"));
        r.log.borrow_mut().insert(0, ("h9".into(), "trabalho de outra pessoa".into()));
        assert!(desfazer_ultima(&dados, &a).contains("já houve outras mudanças"));
        assert!(mapa_modulos(&r.raiz, 20).contains("app: ") && prompt_implementador("p", "q").contains("tests/conftest.py"));
    }
}
