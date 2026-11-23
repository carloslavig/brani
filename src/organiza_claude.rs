//! Organiza a barra lateral do app do Claude SEM IA (organiza_claude.py): zero token.
//! So grava quando o app esta FECHADO (aberto, ele guarda tudo na memoria e grava por cima; na duvida, nao grava).
//! - Titulo sem status ganha "🟡 Parada · " (app fechado = nada rodando; "▶️" vira parada). Titulo que ja tem status fica intocado.
//! - Conversa sem secao vai para a secao do assunto, por palavra-chave no titulo/pasta (cria a secao se faltar).
//! Faz copia do config antes de gravar. Ficou de fora: perguntar ao sistema se o app esta aberto (quem chama passa a saida do
//! comando) e o laco de 5 min. Diferencas: as palavras-chave com nomes de pessoas/clientes vem de configuracao (`secoes`), aqui so
//! ha termos gerais; o config regravado sai com as chaves em ordem alfabetica (mesmo conteudo).
#![allow(dead_code)]
use crate::tempo::Agora;
use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const STATUS: [&str; 4] = ["✅", "🔵", "🟡", "🔁"];
pub const PARADA: &str = "🟡 Parada · ";
pub const PADRAO: &str = "🧠 Braniac e ferramentas";

pub type Secoes = Vec<(String, Vec<String>)>;

/// Secoes com termos gerais. Primeira cujo termo aparece no titulo/pasta vale; o resto vai para PADRAO.
pub fn secoes_padrao() -> Secoes {
    [
        ("🎬 Vídeos de prefeitura", vec!["vídeo", "video", "esteira", "prefeitura", "tcm", "município", "municipio", "capitais", "cidade"]),
        ("🧊 Estúdio 3D", vec!["estúdio 3d", "estudio 3d", "estudio3d", "motor rust", "render 3d"]),
        ("🌐 Sites e clientes", vec!["site", "landing", "vercel", "portfólio", "portfolio", "currículo", "curriculo"]),
    ]
    .into_iter()
    .map(|(n, t)| (n.to_string(), t.into_iter().map(str::to_string).collect()))
    .collect()
}

pub fn secao_de(texto: &str, secoes: &Secoes) -> String {
    let t = texto.to_lowercase();
    secoes.iter().find(|(_, termos)| termos.iter().any(|x| t.contains(&x.to_lowercase()))).map_or(PADRAO.to_string(), |(n, _)| n.clone())
}

/// Titulo com o status de parada, ou None se ja tem status (ou esta vazio).
pub fn titulo_novo(titulo: &str) -> Option<String> {
    let t = titulo.trim();
    if let Some(r) = t.strip_prefix("▶️").or_else(|| t.strip_prefix('▶')) {
        return Some(format!("{PARADA}{}", r.trim()));
    }
    (!t.is_empty() && !STATUS.iter().any(|s| t.starts_with(s))).then(|| format!("{PARADA}{t}"))
}

/// O app desktop roda? `caminhos` = saida de "(Get-Process claude).Path" (None = nao deu para saber: conta como aberto).
/// O CLI do Claude Code nao conta.
pub fn app_aberto(caminhos: Option<&str>) -> bool {
    caminhos.is_none_or(|s| s.contains("WindowsApps") || s.contains("AnthropicClaude"))
}

/// Pastas de dados do app (uma por perfil) que tem config + conversas.
pub fn perfis(local: &Path) -> Vec<PathBuf> {
    [local.join("Claude-Data"), local.to_path_buf()]
        .iter()
        .flat_map(|b| fs::read_dir(b).into_iter().flatten().flatten())
        .map(|e| e.path())
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase().starts_with("claude")) && p.join("claude_desktop_config.json").is_file() && p.join("claude-code-sessions").is_dir())
        .collect()
}

fn gravar(p: &Path, obj: &Value) -> Result<(), String> {
    let tmp = p.with_extension("tmp");
    fs::write(&tmp, serde_json::to_string_pretty(obj).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    fs::rename(&tmp, p).map_err(|e| e.to_string())
}

fn sessoes(perfil: &Path) -> Vec<PathBuf> {
    let filhos = |p: &Path| fs::read_dir(p).into_iter().flatten().flatten().map(|e| e.path()).collect::<Vec<_>>();
    let mut v: Vec<PathBuf> = filhos(&perfil.join("claude-code-sessions"))
        .iter()
        .flat_map(|a| filhos(a))
        .flat_map(|b| filhos(&b))
        .filter(|f| f.file_name().is_some_and(|n| n.to_string_lossy().starts_with("local_") && n.to_string_lossy().ends_with(".json")))
        .collect();
    v.sort();
    v
}

/// Aplica as regras num perfil. `copias`: pasta da copia do config; `novo_id()`: id da secao nova. Devolve o que mudou.
pub fn organizar_perfil(perfil: &Path, copias: &Path, secoes: &Secoes, agora: &Agora, novo_id: &dyn Fn() -> String) -> Result<Vec<String>, String> {
    let cfg_p = perfil.join("claude_desktop_config.json");
    let mut cfg: Value = serde_json::from_str(&fs::read_to_string(&cfg_p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    if !cfg.is_object() {
        return Err("config do app ilegivel".into());
    }
    let (mut feito, mut mudou_cfg) = (Vec::new(), false);
    for arq in sessoes(perfil) {
        let Some(mut s) = fs::read_to_string(&arq).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).filter(Value::is_object) else { continue };
        let (Some(sid), false) = (s["sessionId"].as_str().map(str::to_string), s["isArchived"] == true) else { continue };
        if let Some(novo) = titulo_novo(s["title"].as_str().unwrap_or("")) {
            s["title"] = json!(novo);
            gravar(&arq, &s)?;
            feito.push(format!("título: {novo}"));
        }
        let pasta = |p: Option<&Path>| p.and_then(Path::file_name).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let escopo = format!("{}/{}", pasta(arq.parent().and_then(Path::parent)), pasta(arq.parent()));
        let esc = &mut cfg["preferences"]["epitaxyPrefs"]["dframe-group-scopes"][&escopo];
        for (k, vazio) in [("groups", json!([])), ("assignments", json!({})), ("order", json!({}))] {
            if esc[k].is_null() {
                esc[k] = vazio;
            }
        }
        let chave = format!("code:{sid}");
        if !esc["assignments"][&chave].is_null() {
            continue;
        }
        let nome = secao_de(&format!("{} {}", s["title"].as_str().unwrap_or(""), s["cwd"].as_str().unwrap_or("")), secoes);
        let gid = match esc["groups"].as_array().into_iter().flatten().find(|g| g["name"] == nome.as_str()).and_then(|g| g["id"].as_str()) {
            Some(id) => id.to_string(),
            None => {
                let id = format!("cg-{}", novo_id());
                if let Some(g) = esc["groups"].as_array_mut() {
                    g.push(json!({ "id": id, "name": nome, "mode": "code" }));
                }
                id
            }
        };
        esc["assignments"][&chave] = json!(gid);
        if !esc["order"][&gid].is_array() {
            esc["order"][&gid] = json!([]);
        }
        if let Some(o) = esc["order"][&gid].as_array_mut() {
            o.push(json!(chave));
        }
        mudou_cfg = true;
        feito.push(format!("seção {nome}: {}", s["title"].as_str().unwrap_or("")));
    }
    if mudou_cfg {
        fs::create_dir_all(copias).map_err(|e| e.to_string())?;
        let nome = perfil.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        fs::copy(&cfg_p, copias.join(format!("{nome}-{}.json", agora.carimbo()))).map_err(|e| e.to_string())?; // copia ANTES de gravar
        gravar(&cfg_p, &cfg)?;
    }
    Ok(feito)
}

/// Um ciclo: com o app aberto nao le nem grava nada. `copias` tambem recebe o log.txt.
pub fn ciclo(aberto: bool, local: &Path, copias: &Path, secoes: &Secoes, agora: &Agora, novo_id: &dyn Fn() -> String) -> Vec<String> {
    if aberto {
        return Vec::new();
    }
    let mut feito = Vec::new();
    for p in perfis(local) {
        match organizar_perfil(&p, copias, secoes, agora, novo_id) {
            Ok(f) => feito.extend(f),
            Err(e) => feito.push(format!("erro em {}: {e}", p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())),
        }
    }
    if !feito.is_empty() && fs::create_dir_all(copias).is_ok() {
        if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(copias.join("log.txt")) {
            let _ = writeln!(f, "{} {} {}", agora.data.dia_mes(), agora.hm(), feito.join(" | "));
        }
    }
    feito
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;

    fn agora() -> Agora {
        Agora { data: Data::civil(2026, 11, 18).unwrap(), hora: 10, min: 0, seg: 0 }
    }

    fn perfil(nome: &str) -> (PathBuf, PathBuf, PathBuf) {
        let raiz = std::env::temp_dir().join(format!("braniac-organiza_claude-{}-{nome}", std::process::id()));
        let _ = fs::remove_dir_all(&raiz);
        let p = raiz.join("Claude-X");
        let pasta = p.join("claude-code-sessions").join("conta").join("org");
        fs::create_dir_all(&pasta).unwrap();
        let cfg = json!({"outra": 1, "preferences": {"epitaxyPrefs": {"dframe-group-scopes": {"conta/org": {"groups": [{"id": "cg-1", "name": "🎬 Vídeos de prefeitura", "mode": "code"}], "assignments": {"code:local_b": "cg-1"}, "order": {"cg-1": ["code:local_b"]}}}}}});
        fs::write(p.join("claude_desktop_config.json"), cfg.to_string()).unwrap();
        for (sid, titulo, arquivada) in [("local_a", "Vídeo de Cidade A", false), ("local_b", "✅ Concluída · Esteira", false), ("local_c", "▶️ Jarvis novo", false), ("local_d", "Velha", true)] {
            fs::write(pasta.join(format!("{sid}.json")), json!({"sessionId": sid, "title": titulo, "cwd": "C:\\x", "isArchived": arquivada}).to_string()).unwrap();
        }
        (raiz, p, pasta)
    }

    #[test]
    fn organiza_titulo_e_secao_com_copia_antes() {
        let (raiz, p, pasta) = perfil("titulo");
        let bak = raiz.join("bak");
        let feito = organizar_perfil(&p, &bak, &secoes_padrao(), &agora(), &|| "novo".into()).unwrap();
        let t = |sid: &str| serde_json::from_str::<Value>(&fs::read_to_string(pasta.join(format!("{sid}.json"))).unwrap()).unwrap()["title"].as_str().unwrap().to_string();
        assert_eq!(t("local_a"), "🟡 Parada · Vídeo de Cidade A");
        assert_eq!(t("local_b"), "✅ Concluída · Esteira"); // ja tinha status: intocado
        assert_eq!(t("local_c"), "🟡 Parada · Jarvis novo"); // app fechado: nada roda
        assert_eq!(t("local_d"), "Velha"); // arquivada fica como esta
        let cfg: Value = serde_json::from_str(&fs::read_to_string(p.join("claude_desktop_config.json")).unwrap()).unwrap();
        let esc = &cfg["preferences"]["epitaxyPrefs"]["dframe-group-scopes"]["conta/org"];
        assert_eq!(esc["assignments"]["code:local_a"], "cg-1"); // foi para a secao que ja existia
        assert_eq!(esc["assignments"]["code:local_c"], "cg-novo"); // secao criada
        assert!(esc["groups"].as_array().unwrap().iter().any(|g| g["id"] == "cg-novo" && g["name"] == PADRAO));
        assert_eq!(esc["order"]["cg-1"], json!(["code:local_b", "code:local_a"]));
        assert!(esc["assignments"]["code:local_d"].is_null() && cfg["outra"] == 1);
        let copia = fs::read_to_string(bak.join("Claude-X-20261118-100000.json")).unwrap();
        assert!(!copia.contains("cg-novo")); // a copia e a de ANTES
        assert_eq!(feito.len(), 4);
        assert!(organizar_perfil(&p, &bak, &secoes_padrao(), &agora(), &|| "x".into()).unwrap().is_empty()); // segunda vez nao muda nada
        let _ = fs::remove_dir_all(raiz);
    }

    #[test]
    fn nao_le_nem_grava_com_o_app_aberto() {
        let (raiz, _p, pasta) = perfil("aberto");
        assert!(app_aberto(None) && app_aberto(Some("C:\\Program Files\\WindowsApps\\Claude\\claude.exe")) && !app_aberto(Some("C:\\Users\\x\\.local\\bin\\claude.exe")) && !app_aberto(Some("")));
        assert!(ciclo(true, &raiz, &raiz.join("bak"), &secoes_padrao(), &agora(), &|| "n".into()).is_empty());
        assert!(fs::read_to_string(pasta.join("local_a.json")).unwrap().contains("\"Vídeo de Cidade A\"") && !raiz.join("bak").exists());
        // fechado: acha o perfil (direto na pasta local) e registra no log
        assert_eq!(perfis(&raiz).len(), 1);
        assert_eq!(ciclo(false, &raiz, &raiz.join("bak"), &secoes_padrao(), &agora(), &|| "n".into()).len(), 4);
        assert!(fs::read_to_string(raiz.join("bak/log.txt")).unwrap().starts_with("18/11 10:00 título: "));
        let _ = fs::remove_dir_all(raiz);
    }

    #[test]
    fn regras_de_titulo_e_secao() {
        assert_eq!(titulo_novo("  Site novo "), Some("🟡 Parada · Site novo".into()));
        assert_eq!((titulo_novo(""), titulo_novo("🔵 Falta você · x"), titulo_novo("🔁 Rotina")), (None, None, None));
        let mut s = secoes_padrao();
        assert_eq!(secao_de("Landing page da pousada C:\\x", &s), "🌐 Sites e clientes");
        assert_eq!(secao_de("Render 3D do globo", &s), "🧊 Estúdio 3D");
        assert_eq!(secao_de("Conserto do roteador", &s), PADRAO);
        s.insert(0, ("Cliente Ana".into(), vec!["Ana".into()]));
        assert_eq!(secao_de("pedido da ana", &s), "Cliente Ana"); // nomes vem da configuracao
    }
}
