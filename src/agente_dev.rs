//! Agente de programacao PLUGAVEL: quem mexe no codigo, faz tarefa no PC e responde duvida dificil. O Braniac funciona sem nenhum.
//!   claude -> `claude -p`   codex -> `codex exec`   gemini -> `gemini -p`   custom -> qualquer CLI (`comando`)   nenhum -> sem agente
//! Escolha (a primeira que existir vale): BRANIAC_AGENTE, depois dados/agente_dev.json {"agente", "comando"}, senao `claude`.
//! BRANIAC_SEM_CLAUDE=1 = `nenhum`. So leitura por padrao: editar e pular permissoes so quando quem chama pede.
//! Ficou de fora (entra em `Sistema`): ler variavel de ambiente, achar o executavel e rodar o processo.
//! Diferenca do Python: o `comando` em texto e separado por espacos respeitando aspas (la, shlex).
#![allow(dead_code)]
use crate::empresa_dev::{corta, ler};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const AGENTES: [&str; 5] = ["claude", "codex", "gemini", "custom", "nenhum"];

pub struct Saida {
    pub codigo: i32,
    pub stdout: String,
    pub stderr: String,
}

pub enum Falha {
    /// Passou do tempo; traz o que saiu ate ali.
    Tempo(String),
    Outra(String),
}

pub struct Chamada<'a> {
    pub args: &'a [String],
    /// Pedido pela entrada padrao (None = entrada fechada).
    pub entrada: Option<&'a str>,
    pub pasta: &'a str,
    pub timeout_s: u64,
}

pub struct Sistema<'a> {
    pub env: &'a dyn Fn(&str) -> Option<String>,
    /// Nome do programa -> caminho, se instalado.
    pub which: &'a dyn Fn(&str) -> Option<String>,
    pub executar: &'a dyn Fn(&Chamada) -> Result<Saida, Falha>,
    /// Guia generico do projeto, indicado aos agentes que nao leem o CLAUDE.md sozinhos.
    pub guia: &'a Path,
    /// Pasta de trabalho padrao.
    pub raiz: &'a Path,
}

pub struct Opcoes<'a> {
    pub workdir: Option<&'a str>,
    pub timeout_s: u64,
    pub editar: bool,
    pub navegador: bool,
    pub permissao: &'a str,
    pub pular_permissoes: bool,
}

impl Default for Opcoes<'_> {
    fn default() -> Self {
        Opcoes { workdir: None, timeout_s: 600, editar: false, navegador: false, permissao: "acceptEdits", pular_permissoes: false }
    }
}

fn config(dados: &Path) -> Value {
    ler(&dados.join("agente_dev.json")).filter(Value::is_object).unwrap_or_else(|| json!({}))
}

fn env(s: &Sistema, k: &str) -> Option<String> {
    (s.env)(k).map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// Nome do agente em uso; valor desconhecido cai no padrao.
pub fn escolhido(s: &Sistema, dados: &Path) -> &'static str {
    if (s.env)("BRANIAC_SEM_CLAUDE").as_deref() == Some("1") {
        return "nenhum";
    }
    let nome = env(s, "BRANIAC_AGENTE").or_else(|| config(dados)["agente"].as_str().map(String::from)).unwrap_or_default().trim().to_lowercase();
    AGENTES.iter().copied().find(|a| *a == nome).unwrap_or("claude")
}

/// Como chamar o agente nas mensagens ao usuario.
pub fn rotulo(nome: &str) -> &'static str {
    match nome {
        "claude" => "Claude",
        "codex" => "Codex",
        "gemini" => "Gemini CLI",
        "custom" => "o agente configurado",
        _ => "o agente de programação",
    }
}

fn separar(c: &str) -> Vec<String> {
    let (mut out, mut atual, mut aspas, mut tem) = (Vec::new(), String::new(), None, false);
    for ch in c.chars() {
        match (aspas, ch) {
            (Some(a), x) if x == a => aspas = None,
            (None, '"' | '\'') => { aspas = Some(ch); tem = true }
            (None, x) if x.is_whitespace() => {
                if tem || !atual.is_empty() {
                    out.push(std::mem::take(&mut atual));
                }
                tem = false;
            }
            _ => atual.push(ch),
        }
    }
    if tem || !atual.is_empty() {
        out.push(atual);
    }
    out
}

fn comando_custom(s: &Sistema, dados: &Path) -> Vec<String> {
    match &config(dados)["comando"] {
        Value::Array(a) => a.iter().map(|x| x.as_str().map_or_else(|| x.to_string(), String::from)).collect(),
        Value::String(c) if !c.trim().is_empty() => separar(c),
        _ => separar(&env(s, "BRANIAC_AGENTE_CMD").unwrap_or_default()),
    }
}

/// Caminho do executavel, ou None se o agente nao esta instalado.
pub fn localizar(s: &Sistema, dados: &Path, nome: &str) -> Option<String> {
    match nome {
        "nenhum" => None,
        "custom" => {
            let cmd = comando_custom(s, dados);
            let c = cmd.first()?;
            (s.which)(c).or_else(|| Path::new(c).exists().then(|| c.clone()))
        }
        n => [n.to_string(), format!("{n}.cmd")].iter().find_map(|e| (s.which)(e)),
    }
}

/// (argumentos, o pedido vai pela entrada padrao?). Cada agente tem o seu jeito de ficar so leitura x editar.
fn montar(s: &Sistema, dados: &Path, nome: &str, exe: &str, workdir: &str, o: &Opcoes, saida: &str) -> (Vec<String>, bool) {
    let v = |a: &[&str]| a.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    match nome {
        "claude" => {
            let mut args = vec![exe.to_string()];
            if o.navegador {
                args.push("--chrome".into());
            }
            args.push("-p".into());
            if o.editar && o.pular_permissoes {
                args.push("--dangerously-skip-permissions".into());
            } else if o.editar && !o.permissao.is_empty() {
                args.extend(v(&["--permission-mode", o.permissao]));
            }
            (args, true)
        }
        // `-` = ler o pedido da entrada padrao; a resposta final vai para o arquivo
        "codex" => (v(&[exe, "exec", "--skip-git-repo-check", "-C", workdir, "-s", if o.editar { "workspace-write" } else { "read-only" }, "-o", saida, "-"]), true),
        "gemini" => (v(&[exe, "--approval-mode", if o.editar { "auto_edit" } else { "default" }, "-p", "Execute o pedido recebido pela entrada padrao."]), true),
        _ => {
            let cmd = comando_custom(s, dados);
            let por_stdin = !cmd.iter().any(|a| a.contains("{prompt}"));
            (cmd, por_stdin)
        }
    }
}

static SEQ: AtomicU64 = AtomicU64::new(0);

/// Roda o agente escolhido: (codigo, texto). 0 = ok; 124 = estourou o tempo; 127 = sem agente ou sem instalacao.
pub fn rodar(s: &Sistema, dados: &Path, prompt: &str, o: &Opcoes) -> (i32, String) {
    let nome = escolhido(s, dados);
    let Some(exe) = localizar(s, dados, nome) else {
        let motivo = if nome == "nenhum" { "nenhum agente de programação configurado (BRANIAC_AGENTE=nenhum)".to_string() } else { format!("não encontrei o {} instalado", rotulo(nome)) };
        return (127, format!("Sem agente de programação: {motivo}. O Braniac segue funcionando com as IAs grátis/locais."));
    };
    let raiz = s.raiz.to_string_lossy().to_string();
    let workdir = o.workdir.unwrap_or(&raiz);
    let prompt = if nome == "claude" { prompt.to_string() } else {
        format!("Antes de agir, leia o guia do projeto em {} e, se existir, o CLAUDE.md ou AGENTS.md da pasta de trabalho; siga as travas de seguranca deles.\n\n{prompt}", s.guia.display())
    };
    let tmp: PathBuf = std::env::temp_dir().join(format!("braniac-agente-{}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::create_dir_all(&tmp);
    let saida = tmp.join("resposta.txt");
    let (mut args, por_stdin) = montar(s, dados, nome, &exe, workdir, o, &saida.to_string_lossy());
    if !por_stdin {
        // CLI .cmd no Windows corta o argumento na quebra de linha
        let plano = prompt.split('\n').collect::<Vec<_>>().join(" ");
        args = args.iter().map(|a| a.replace("{prompt}", &plano)).collect();
    }
    let r = (s.executar)(&Chamada { args: &args, entrada: por_stdin.then_some(prompt.as_str()), pasta: workdir, timeout_s: o.timeout_s });
    let fim = if nome == "codex" { std::fs::read_to_string(&saida).ok().map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) } else { None };
    let _ = std::fs::remove_dir_all(&tmp);
    match r {
        Err(Falha::Tempo(parcial)) => (124, format!("{parcial}\n[TIMEOUT] passou de {}s e foi encerrado.", o.timeout_s)),
        Err(Falha::Outra(e)) => (1, format!("Não consegui acionar {}: {}", rotulo(nome), corta(&e, 200))),
        Ok(r) => {
            let mut texto = fim.unwrap_or(r.stdout);
            if r.codigo != 0 && !r.stderr.is_empty() {
                texto += &format!("\n[stderr]\n{}", r.stderr);
            }
            (r.codigo, texto)
        }
    }
}

/// Duvida ou decisao em texto (sem editar nada). None se nao ha agente ou ele falhou.
pub fn perguntar(s: &Sistema, dados: &Path, prompt: &str, timeout_s: u64) -> Option<String> {
    let (cod, texto) = rodar(s, dados, prompt, &Opcoes { timeout_s, ..Opcoes::default() });
    (cod == 0 && !texto.trim().is_empty()).then(|| texto.trim().to_string())
}

/// Retrato para o painel.
pub fn status(s: &Sistema, dados: &Path) -> Value {
    let nome = escolhido(s, dados);
    let caminho = localizar(s, dados, nome);
    json!({ "agente": nome, "instalado": caminho.is_some(), "caminho": caminho, "opcoes": AGENTES, "sem_agente_o_braniac_funciona": true })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    struct Falso {
        env: HashMap<&'static str, &'static str>,
        chamadas: RefCell<Vec<(Vec<String>, Option<String>, String)>>,
        dados: PathBuf,
    }

    impl Falso {
        fn novo(nome: &str, env: &[(&'static str, &'static str)]) -> Falso {
            let dados = std::env::temp_dir().join(format!("braniac-agente_dev-{nome}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dados);
            std::fs::create_dir_all(&dados).unwrap();
            Falso { env: env.iter().copied().collect(), chamadas: RefCell::default(), dados }
        }

        fn com<T>(&self, resp: &dyn Fn(&Chamada) -> Result<Saida, Falha>, f: impl Fn(&Sistema) -> T) -> T {
            let exec = |c: &Chamada| {
                self.chamadas.borrow_mut().push((c.args.to_vec(), c.entrada.map(String::from), c.pasta.to_string()));
                resp(c)
            };
            f(&Sistema { env: &|k| self.env.get(k).map(|v| v.to_string()), which: &|n| (!n.ends_with(".cmd") && n != "ausente").then(|| format!("/bin/{n}")), executar: &exec,
                         guia: Path::new("/proj/AGENTS.md"), raiz: Path::new("/proj") })
        }

        fn rodar(&self, prompt: &str, o: &Opcoes) -> (i32, String) {
            self.com(&|c| {
                if let Some(i) = c.args.iter().position(|a| a == "-o") { // o codex grava a resposta final no arquivo pedido
                    std::fs::write(&c.args[i + 1], "feito pelo codex").unwrap();
                }
                Ok(Saida { codigo: 0, stdout: "saida".into(), stderr: String::new() })
            }, |s| rodar(s, &self.dados, prompt, o))
        }
    }

    impl Drop for Falso {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dados);
        }
    }

    #[test]
    fn padrao_e_claude_so_leitura_e_editar_so_quando_pedem() {
        let f = Falso::novo("claude", &[]);
        assert_eq!(f.rodar("oi", &Opcoes::default()), (0, "saida".to_string()));
        assert_eq!(f.chamadas.borrow()[0], (vec!["/bin/claude".to_string(), "-p".to_string()], Some("oi".to_string()), "/proj".to_string()));
        f.rodar("faz", &Opcoes { editar: true, navegador: true, ..Opcoes::default() });
        assert_eq!(f.chamadas.borrow()[1].0, ["/bin/claude", "--chrome", "-p", "--permission-mode", "acceptEdits"]);
        f.rodar("faz", &Opcoes { pular_permissoes: true, ..Opcoes::default() }); // sem editar, pular permissoes nao vale
        assert_eq!(f.chamadas.borrow()[2].0, ["/bin/claude", "-p"]);
        f.rodar("faz", &Opcoes { editar: true, pular_permissoes: true, ..Opcoes::default() });
        assert_eq!(f.chamadas.borrow()[3].0, ["/bin/claude", "-p", "--dangerously-skip-permissions"]);
    }

    #[test]
    fn codex_gemini_e_a_ordem_da_escolha() {
        let f = Falso::novo("codex", &[("BRANIAC_AGENTE", "codex")]);
        std::fs::write(f.dados.join("agente_dev.json"), r#"{"agente": "gemini"}"#).unwrap();
        assert_eq!(f.rodar("faz", &Opcoes { workdir: Some("C:/x"), editar: true, ..Opcoes::default() }), (0, "feito pelo codex".to_string())); // a variavel vence o arquivo
        let (args, entrada, pasta) = f.chamadas.borrow()[0].clone();
        assert!(args[..2] == ["/bin/codex", "exec"] && args.contains(&"workspace-write".to_string()) && args.last().unwrap() == "-" && pasta == "C:/x");
        let entrada = entrada.unwrap();
        assert!(entrada.contains("AGENTS.md") && entrada.ends_with("faz")); // agente que nao le o CLAUDE.md recebe o guia
        f.rodar("responda", &Opcoes::default());
        assert!(f.chamadas.borrow()[1].0.contains(&"read-only".to_string()));
        let g = Falso::novo("gemini", &[]);
        std::fs::write(g.dados.join("agente_dev.json"), r#"{"agente": "Gemini"}"#).unwrap();
        g.rodar("x", &Opcoes { editar: true, ..Opcoes::default() });
        assert!(g.chamadas.borrow()[0].0[..3] == ["/bin/gemini", "--approval-mode", "auto_edit"]);
        std::fs::write(g.dados.join("agente_dev.json"), r#"{"agente": "inventado"}"#).unwrap();
        assert_eq!(g.com(&|_| Err(Falha::Outra(String::new())), |s| escolhido(s, &g.dados)), "claude");
    }

    #[test]
    fn nenhum_ausente_custom_e_falhas() {
        let n = Falso::novo("nenhum", &[("BRANIAC_SEM_CLAUDE", "1"), ("BRANIAC_AGENTE", "codex")]);
        let (cod, txt) = n.rodar("x", &Opcoes::default());
        assert!(cod == 127 && txt.contains("segue funcionando") && n.chamadas.borrow().is_empty());
        assert!(n.com(&|_| Err(Falha::Outra(String::new())), |s| perguntar(s, &n.dados, "x", 10).is_none() && status(s, &n.dados)["instalado"] == false && status(s, &n.dados)["agente"] == "nenhum"));
        let c = Falso::novo("custom", &[("BRANIAC_AGENTE", "custom")]);
        std::fs::write(c.dados.join("agente_dev.json"), r#"{"comando": "meu-cli --modo \"so leitura\" {prompt}"}"#).unwrap();
        c.rodar("linha 1\nlinha 2", &Opcoes::default());
        let (args, entrada, _) = c.chamadas.borrow()[0].clone();
        assert!(args[..3] == ["meu-cli", "--modo", "so leitura"] && args[3].ends_with("linha 1 linha 2") && entrada.is_none());
        std::fs::write(c.dados.join("agente_dev.json"), r#"{"comando": ["meu-cli", "-"]}"#).unwrap();
        c.rodar("pela entrada", &Opcoes::default());
        assert!(c.chamadas.borrow()[1].1.as_deref().unwrap().ends_with("pela entrada"));
        std::fs::write(c.dados.join("agente_dev.json"), r#"{"comando": ["ausente"]}"#).unwrap();
        assert_eq!(c.rodar("x", &Opcoes::default()).0, 127);
        let f = Falso::novo("falhas", &[]);
        let t = f.com(&|_| Err(Falha::Tempo("meio".into())), |s| rodar(s, &f.dados, "x", &Opcoes { timeout_s: 5, ..Opcoes::default() }));
        assert!(t.0 == 124 && t.1.contains("meio") && t.1.contains("passou de 5s"));
        assert!(f.com(&|_| Err(Falha::Outra("acesso negado".into())), |s| rodar(s, &f.dados, "x", &Opcoes::default())) == (1, "Não consegui acionar Claude: acesso negado".to_string()));
        let e = f.com(&|_| Ok(Saida { codigo: 2, stdout: "parcial".into(), stderr: "limit reached".into() }), |s| rodar(s, &f.dados, "x", &Opcoes::default()));
        assert_eq!(e, (2, "parcial\n[stderr]\nlimit reached".to_string()));
        assert_eq!(f.com(&|_| Ok(Saida { codigo: 0, stdout: "  resposta  ".into(), stderr: String::new() }), |s| perguntar(s, &f.dados, "x", 10)).as_deref(), Some("resposta"));
        assert_eq!(separar("a  'b c' \"\" d"), ["a", "b c", "", "d"]);
    }
}
