//! Delega uma tarefa no PC ao agente de programacao escolhido (ver agente_dev.rs) e registra TODA delegacao (inicio e fim) em
//! dados/delegacoes.log, para o resultado nao sumir quando nao ha canal aberto para avisar.
//! Regras: o pedido vai com a ordem de seguir o guia da pasta e as travas (nada de pagamento, senha ou dado bancario; mensagem so se a
//! tarefa pedir); o texto da tarefa entra como dado do dono; o resultado e cortado antes de ir a quem pediu.
//! Ficou de fora: a thread e o aviso ao dono quando ninguem esta ouvindo (closures; quem chama decide), achar o executavel pelo monitor,
//! e o teto diario de acionamentos (no Python ele tambem nao mora neste modulo: quem chama confere).
//! Diferenca do Python: o prompt nao cita nome de pessoa nem de imovel; essas travas estao no guia da pasta, que o agente e mandado seguir.
#![allow(dead_code)]
use crate::empresa_dev::{cauda, corta};
use crate::tempo::Agora;
use std::io::Write;
use std::path::Path;

#[derive(Debug, PartialEq)]
pub struct Cfg {
    pub cli: String,
    pub workdir: String,
    pub mode: String,
    pub skip: bool,
}

/// Secao [claude] do config.ini do monitor (CLI_PATH, WORKDIR, PERMISSION_MODE, DANGEROUS_SKIP_PERMISSIONS). Pular permissoes so com "true" escrito.
pub fn config(ini: &str, raiz: &str) -> Cfg {
    let mut c = Cfg { cli: "auto".into(), workdir: raiz.into(), mode: "acceptEdits".into(), skip: false };
    let mut dentro = false;
    for l in ini.trim_start_matches('\u{feff}').lines().map(str::trim) {
        if l.starts_with('[') {
            dentro = l.eq_ignore_ascii_case("[claude]");
        } else if let (true, Some((k, v))) = (dentro, l.split_once(['=', ':'])) {
            let v = v.split(" #").next().unwrap_or("").trim().to_string();
            match k.trim().to_uppercase().as_str() {
                "CLI_PATH" => c.cli = v,
                "WORKDIR" => c.workdir = v,
                "PERMISSION_MODE" => c.mode = v,
                "DANGEROUS_SKIP_PERMISSIONS" => c.skip = v.eq_ignore_ascii_case("true"),
                _ => {}
            }
        }
    }
    c
}

pub fn montar_prompt(tarefa: &str) -> String {
    format!("O dono pediu esta tarefa pelo Jarvis: {} | Siga o CLAUDE.md desta pasta e todas as travas de seguranca (nada de pagamento, senha ou dados bancarios; \
        nao invente informacao). Para enviar mensagens no WhatsApp so faca o que a tarefa pedir explicitamente. Ao terminar, responda com um resumo curto em portugues do Brasil do que foi feito.", tarefa.trim())
}

fn log(dados: &Path, agora: &Agora, linha: &str) {
    let _ = std::fs::create_dir_all(dados);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dados.join("delegacoes.log")) {
        let _ = writeln!(f, "{} {}:{:02} | {}", agora.data.iso(), agora.hm(), agora.seg, linha.replace('\n', " "));
    }
}

/// O que dizer a quem pediu, conforme o codigo de saida do agente (0 ok, 124 tempo, 127 sem agente).
pub fn resultado(codigo: i32, saida: &str, quem: &str) -> String {
    let s = saida.trim();
    match codigo {
        0 if !s.is_empty() => cauda(s, 3500),
        127 => s.to_string(),
        124 => format!("{quem} passou do tempo limite e foi encerrado. Confira o que ficou pela metade."),
        _ => format!("{quem} terminou com erro (código {codigo}):\n{}", cauda(s, 800)),
    }
}

/// Roda a tarefa (bloqueante) e entrega o resumo ou o erro a `ao_terminar`. `rodar(prompt)` = o agente; `quem` = como chama-lo na mensagem.
pub fn executar(dados: &Path, tarefa: &str, agora: &Agora, quem: &str, rodar: &dyn Fn(&str) -> Result<(i32, String), String>, ao_terminar: &dyn Fn(&str)) -> String {
    log(dados, agora, &format!("INICIO | {}", corta(tarefa.trim(), 300)));
    let r = match rodar(&montar_prompt(tarefa)) {
        Ok((cod, saida)) => resultado(cod, &saida, quem),
        Err(e) => format!("Não consegui acionar o agente de programação: {}", corta(&e, 200)),
    };
    log(dados, agora, &format!("FIM | {}", corta(&r, 500)));
    ao_terminar(&r);
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;

    #[test]
    fn config_do_ini_e_pular_permissoes_so_com_true() {
        let padrao = config("", "/proj");
        assert_eq!(padrao, Cfg { cli: "auto".into(), workdir: "/proj".into(), mode: "acceptEdits".into(), skip: false });
        let c = config("[geral]\nWORKDIR = /errado\n[claude]\ncli_path = C:/x/claude.exe  # caminho\nPERMISSION_MODE: plan\nDANGEROUS_SKIP_PERMISSIONS = True\n[outra]\nPERMISSION_MODE = bypass\n", "/proj");
        assert_eq!(c, Cfg { cli: "C:/x/claude.exe".into(), workdir: "/proj".into(), mode: "plan".into(), skip: true });
        assert!(!config("[claude]\nDANGEROUS_SKIP_PERMISSIONS = sim\n", "/p").skip);
    }

    #[test]
    fn registra_inicio_e_fim_e_traduz_o_codigo() {
        let d = std::env::temp_dir().join(format!("braniac-claude_agent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let agora = Agora { data: Data::civil(2026, 11, 18).unwrap(), hora: 8, min: 1, seg: 2 };
        let visto = std::cell::RefCell::new(Vec::new());
        let fim = |r: &str| visto.borrow_mut().push(r.to_string());
        let r = executar(&d, "  arruma a planilha\ndo mês ", &agora, "Claude", &|p| { assert!(p.contains("arruma a planilha") && p.contains("travas de seguranca")); Ok((0, " feito \n".into())) }, &fim);
        assert!(r == "feito" && visto.borrow()[0] == "feito");
        assert!(executar(&d, "x", &agora, "Claude", &|_| Ok((124, "meio".into())), &fim).contains("tempo limite"));
        assert_eq!(executar(&d, "x", &agora, "Claude", &|_| Ok((127, "Sem agente de programação.".into())), &fim), "Sem agente de programação.");
        assert!(executar(&d, "x", &agora, "Codex", &|_| Ok((3, "deu ruim".into())), &fim).starts_with("Codex terminou com erro (código 3):\ndeu ruim"));
        assert!(executar(&d, "x", &agora, "Claude", &|_| Ok((0, "  ".into())), &fim).contains("código 0")); // ok sem resposta nao e sucesso
        assert!(executar(&d, "x", &agora, "Claude", &|_| Err("sumiu".into()), &fim).contains("Não consegui acionar"));
        assert_eq!(resultado(0, &"a".repeat(5000), "Claude").len(), 3500);
        let log = std::fs::read_to_string(d.join("delegacoes.log")).unwrap();
        assert!(log.lines().count() == 12 && log.starts_with("2026-11-18 08:01:02 | INICIO | arruma a planilha do mês") && log.lines().nth(1).unwrap().ends_with("FIM | feito"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
