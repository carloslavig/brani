//! O agente de verdade por tras de `dono.rs`: "faz um jogo e me manda" (pelo WhatsApp do dono ou pelo painel) e a resposta tardia
//! quando a IA do Jarvis falha. Liga o `agente_dev` (por padrao `claude -p`, a assinatura do dono, com `--permission-mode acceptEdits`,
//! SEM pular permissoes) a uma thread. As travas ja moram em `dono.rs` (PROIBIDA, teto diario, pasta propria `dados/entregas/<data>-<pedido>/`);
//! aqui so se roda o agente e se entrega: o arquivo e o aviso viram linhas na caixa de saida nos canais `pedido-arquivo` e `pedido-aviso`,
//! que o despachante so entrega com a chave do WhatsApp (e confere a conversa no topo antes de enviar). Com chave desligada nada sai.
//! Painel: o aviso vai para a tela de avisos e o resultado abre no PC (programa nunca e executado: abre a pasta).
use crate::agente_dev::{self, Chamada, Falha, Opcoes, Saida, Sistema};
use crate::contexto::Ctx;
use crate::dono::{self, Lido, Portas};
use crate::tempo::Agora;
use crate::{avisos, saida};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const TEMPO_TAREFA_S: u64 = 1200;
const TEMPO_RESPOSTA_S: u64 = 300;

/// Roda o programa de `c.args[0]` com o resto dos argumentos, o pedido na entrada padrao e um prazo.
pub fn executar_processo(c: &Chamada) -> Result<Saida, Falha> {
    let exe = c.args.first().ok_or_else(|| Falha::Outra("sem programa".into()))?;
    let mut cmd = Command::new(exe);
    // se o Braniac subiu de dentro de uma sessao do Claude Code, o filho nao pode achar que esta aninhado nela
    cmd.args(&c.args[1..]).env_remove("CLAUDECODE").env_remove("CLAUDE_CODE_ENTRYPOINT").current_dir(c.pasta)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut filho = cmd.spawn().map_err(|e| Falha::Outra(e.to_string()))?;
    if let Some(mut e) = filho.stdin.take() {
        let _ = e.write_all(c.entrada.unwrap_or("").as_bytes());
    }
    let ler = |mut r: Box<dyn Read + Send>| std::thread::spawn(move || { let mut t = String::new(); let _ = r.read_to_string(&mut t); t });
    let out = ler(Box::new(filho.stdout.take().ok_or_else(|| Falha::Outra("sem saida".into()))?));
    let err = ler(Box::new(filho.stderr.take().ok_or_else(|| Falha::Outra("sem saida de erro".into()))?));
    let limite = Instant::now() + Duration::from_secs(c.timeout_s);
    loop {
        match filho.try_wait() {
            Ok(Some(s)) => return Ok(Saida { codigo: s.code().unwrap_or(1), stdout: out.join().unwrap_or_default(), stderr: err.join().unwrap_or_default() }),
            Ok(None) if Instant::now() < limite => std::thread::sleep(Duration::from_millis(250)),
            _ => {
                let _ = filho.kill();
                let _ = filho.wait();
                return Err(Falha::Tempo(out.join().unwrap_or_default()));
            }
        }
    }
}

/// O agente escolhido (padrao `claude`) rodando de verdade. `pasta` = onde ele pode trabalhar (so a entrega); None = so leitura.
fn rodar_agente(dados: &Path, prompt: &str, pasta: Option<&Path>) -> (i32, String) {
    let env = |k: &str| std::env::var(k).ok();
    let which = |n: &str| crate::claude_cli::localizar().filter(|_| n.starts_with("claude")).map(|p| p.to_string_lossy().to_string());
    let (guia, raiz) = (dados.join("AGENTE.md"), pasta.unwrap_or(dados).to_path_buf());
    let s = Sistema { env: &env, which: &which, executar: &executar_processo, guia: &guia, raiz: &raiz };
    let workdir = pasta.map(|p| p.to_string_lossy().to_string());
    let o = Opcoes { workdir: workdir.as_deref(), timeout_s: if pasta.is_some() { TEMPO_TAREFA_S } else { TEMPO_RESPOSTA_S }, editar: pasta.is_some(), ..Opcoes::default() };
    agente_dev::rodar(&s, dados, prompt, &o)
}

/// O pedido do dono, de volta do prompt montado por `dono.rs` (entre « e »).
fn pedido_do_prompt(prompt: &str) -> String {
    prompt.split_once('«').and_then(|(_, r)| r.split_once("».").or_else(|| r.split_once('»'))).map_or_else(String::new, |(p, _)| p.to_string())
}

fn arquivos(pasta: &Path, v: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(pasta).into_iter().flatten().flatten() {
        let c = e.path();
        if c.is_dir() { arquivos(&c, v) } else { v.push(c) }
    }
}

/// Compacta a pasta inteira num .zip AO LADO dela (como no Python) e devolve o caminho.
pub fn zipar(pasta: &Path) -> Result<PathBuf, String> {
    let destino = pasta.with_extension("zip");
    let mut todos = vec![];
    arquivos(pasta, &mut todos);
    todos.sort();
    let mut z = zip::ZipWriter::new(std::fs::File::create(&destino).map_err(|e| e.to_string())?);
    let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for a in todos {
        let rel = a.strip_prefix(pasta).map_err(|e| e.to_string())?.to_string_lossy().replace('\\', "/");
        z.start_file(rel, o).map_err(|e| e.to_string())?;
        z.write_all(&std::fs::read(&a).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    }
    z.finish().map_err(|e| e.to_string())?;
    Ok(destino)
}

fn nada_ler(_: &str, _: Option<&str>) -> Option<Lido> { None }
fn nada_responder(_: &str, _: &str) -> Result<(String, bool), String> { Err("sem IA aqui".into()) }
fn nada_ouvir(_: &str, _: usize) -> Result<String, String> { Err("sem audio aqui".into()) }
fn nada_baixar(_: &str, _: &str) -> Result<PathBuf, String> { Err("sem download aqui".into()) }
fn nunca(_: &str) -> bool { false }
fn nada(_: &str) {}
fn sem_agente(_: &str, _: &str, _: Option<&Path>) {}

/// As portas de `dono.rs` para quem so precisa do agente, do teto e de uma forma de avisar. O resto nao se usa fora do laco do chat.
pub fn portas<'a>(pode: &'a dyn Fn() -> bool, usar: &'a dyn Fn(), agente: &'a dyn Fn(&str, &str, Option<&Path>), enviar: &'a dyn Fn(&str, &str) -> String,
                  falar: &'a dyn Fn(&str, &str) -> bool) -> Portas<'a> {
    Portas { ler: &nada_ler, responder: &nada_responder, revisar: None, enviar, falar, ouvir: &nada_ouvir, foi_nosso: &nunca, pode_agente: pode, usar_agente: usar,
        agente, baixar: &nada_baixar, observar: &nada }
}

/// Fim de um acionamento pelo WhatsApp: "tarefa" entrega o arquivo (ou avisa a falha), "resposta" manda a resposta tardia. Tudo na caixa de saida.
pub fn concluir_whatsapp(dados: &Path, agora: &Agora, tipo: &str, prompt: &str, pasta: Option<&Path>, saida_agente: &str) {
    let ctx = Ctx::ler(dados);
    let Some(nome) = ctx.chat_do_dono() else { return };
    let enviar = |n: &str, t: &str| saida::registrar(dados, "pedido", n, t);
    let falar = |n: &str, t: &str| saida::registrar(dados, "pedido-aviso", n, t).starts_with('✅');
    let p = portas(&|| false, &|| (), &sem_agente, &enviar, &falar);
    match (tipo, pasta) {
        ("tarefa", Some(pasta)) => {
            let arq = |n: &str, a: &Path, l: &str| saida::registrar(dados, "pedido-arquivo", n, &format!("{}\n{l}", a.display()));
            dono::entregar(dados, agora, &ctx, &p, pasta, &pedido_do_prompt(prompt), saida_agente, &arq, &zipar);
        }
        ("resposta", _) => {
            dono::avisar(&ctx, &p, nome, &dono::resposta_tardia(saida_agente));
        }
        _ => {} // "documentos": o download nao esta ligado
    }
}

/// Acionamento do chat do dono: roda o agente numa thread e, no fim, entrega pela caixa de saida. So e chamado depois do teto e da PROIBIDA.
pub fn disparar_whatsapp(dados: &Path, tipo: &str, prompt: &str, pasta: Option<&Path>) {
    let (dados, tipo, prompt, pasta) = (dados.to_path_buf(), tipo.to_string(), prompt.to_string(), pasta.map(Path::to_path_buf));
    std::thread::spawn(move || {
        let editar = tipo == "tarefa" && pasta.is_some();
        let (_, texto) = rodar_agente(&dados, &prompt, pasta.as_deref().filter(|_| editar));
        concluir_whatsapp(&dados, &crate::tempo::agora(), &tipo, &prompt, pasta.as_deref(), &texto);
    });
}

/// Fim de um pedido feito no painel ou por voz: aviso na tela de avisos e devolve o que abrir no PC.
pub fn concluir_painel(dados: &Path, agora: &Agora, prompt: &str, pasta: &Path, saida_agente: &str) -> Option<PathBuf> {
    let (aviso, abre) = dono::fim_tarefa_local(pasta, &pedido_do_prompt(prompt), saida_agente);
    avisos::registrar(dados, agora, "dono", &aviso);
    abre
}

/// Acionamento do painel: roda numa thread, avisa na tela e abre o resultado.
pub fn disparar_painel(dados: &Path, prompt: &str, pasta: &Path) {
    let (dados, prompt, pasta) = (dados.to_path_buf(), prompt.to_string(), pasta.to_path_buf());
    std::thread::spawn(move || {
        let (_, texto) = rodar_agente(&dados, &prompt, Some(&pasta));
        if let Some(abre) = concluir_painel(&dados, &crate::tempo::agora(), &prompt, &pasta, &texto) {
            let _ = crate::abrir_no_pc(&abre.to_string_lossy());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;
    use serde_json::{json, Value};

    fn agora() -> Agora {
        Agora { data: Data::civil(2026, 11, 21).unwrap(), hora: 10, min: 0, seg: 0 }
    }

    fn pasta(n: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-dagente-{n}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("pessoal.json"), json!({ "nome": "Dono", "contatos": { "dono": "Eu(D)" } }).to_string()).unwrap();
        d
    }

    fn linhas(d: &Path) -> Vec<Value> {
        std::fs::read_to_string(d.join("saida.jsonl")).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
    }

    #[test]
    fn pedido_volta_do_prompt() {
        assert_eq!(pedido_do_prompt("O dono pediu: «faz um jogo». Faça isso."), "faz um jogo");
        assert_eq!(pedido_do_prompt("sem aspas"), "");
    }

    #[test]
    fn um_arquivo_vai_sozinho_e_varios_viram_zip_e_o_aviso_vem_depois() {
        let d = pasta("entrega");
        let e1 = d.join("entregas").join("a-jogo");
        std::fs::create_dir_all(&e1).unwrap();
        std::fs::write(e1.join("index.html"), "<html>").unwrap();
        concluir_whatsapp(&d, &agora(), "tarefa", "O dono pediu: «faz um jogo». Faça isso.", Some(&e1), "fiz");
        let l = linhas(&d);
        assert_eq!((l[0]["canal"].as_str(), l[0]["destino"].as_str()), (Some("pedido-arquivo"), Some("Eu(D)")));
        assert!(l[0]["texto"].as_str().unwrap().contains("index.html") && l[0]["texto"].as_str().unwrap().ends_with("Pronto: faz um jogo"));
        assert!(l[1]["canal"].as_str().unwrap().starts_with("pedido") && l[1]["canal"] != "pedido-arquivo");
        assert!(l[1]["texto"].as_str().unwrap().contains("Te mandei o arquivo"));
        // varios arquivos: zip ao lado da pasta
        let e2 = d.join("entregas").join("b-site");
        std::fs::create_dir_all(e2.join("css")).unwrap();
        std::fs::write(e2.join("index.html"), "<html>").unwrap();
        std::fs::write(e2.join("css").join("a.css"), "x{}").unwrap();
        concluir_whatsapp(&d, &agora(), "tarefa", "O dono pediu: «faz um site». Faça isso.", Some(&e2), "fiz");
        let z = linhas(&d)[2]["texto"].as_str().unwrap().lines().next().unwrap().to_string();
        assert!(z.ends_with("b-site.zip") && Path::new(&z).is_file(), "{z}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn pasta_vazia_avisa_a_falha_e_nao_manda_arquivo() {
        let d = pasta("vazia");
        let e = d.join("entregas").join("c");
        std::fs::create_dir_all(&e).unwrap();
        concluir_whatsapp(&d, &agora(), "tarefa", "O dono pediu: «faz um jogo». Faça isso.", Some(&e), "deu erro");
        let l = linhas(&d);
        assert_eq!(l.len(), 1);
        assert!(l[0]["canal"].as_str().unwrap().starts_with("pedido") && l[0]["texto"].as_str().unwrap().contains("não consegui terminar"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn resposta_tardia_vai_ao_chat_do_dono_e_painel_avisa_e_abre() {
        let d = pasta("resposta");
        concluir_whatsapp(&d, &agora(), "resposta", "x", None, "A resposta é curta.");
        let l = linhas(&d);
        assert!(l[0]["canal"].as_str().unwrap().starts_with("pedido") && l[0]["texto"] == "A resposta é curta.");
        let e = d.join("entregas").join("p");
        std::fs::create_dir_all(&e).unwrap();
        std::fs::write(e.join("jogo.html"), "<html>").unwrap();
        let abre = concluir_painel(&d, &agora(), "O dono pediu: «faz um jogo». Faça isso.", &e, "ok").unwrap();
        assert!(abre.ends_with("jogo.html"));
        assert!(std::fs::read_to_string(d.join("avisos.jsonl")).unwrap().contains("Abri aqui no PC"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn processo_roda_com_entrada_e_prazo() {
        let d = pasta("proc");
        let (prog, args): (&str, Vec<String>) = if cfg!(windows) { ("cmd", vec!["cmd".into(), "/c".into(), "echo oi".into()]) } else { ("sh", vec!["sh".into(), "-c".into(), "echo oi".into()]) };
        let _ = prog;
        let r = executar_processo(&Chamada { args: &args, entrada: None, pasta: d.to_str().unwrap(), timeout_s: 20 });
        assert!(matches!(r, Ok(ref s) if s.codigo == 0 && s.stdout.contains("oi")));
        let _ = std::fs::remove_dir_all(&d);
    }
}
