//! Cérebro: ponte do B.R.A.N.I com a memória unificada das IAs (E:\Cerebro, ou CEREBRO_DIR).
//! GET /cerebro (tela), GET /api/cerebro/indice (INDEX.md + fila de pedidos + handoffs); POST /api/cerebro/pedido {de,para,assunto,contexto}.
//! Só lê/grava a camada pública (INDEX.md, pedidos\, handoffs\); `privado\` nunca é tocado. Mesmo formato de pedido do MCP (E:\Cerebro\mcp).
use crate::projetos_eventos as pe;
use crate::{dado, erro, Resp};
use serde_json::{json, Value};
use std::path::PathBuf;

const IAS: [&str; 7] = ["claude", "codex", "antigravity", "brani", "nexodev", "deepseek", "usuarios"];

fn raiz() -> PathBuf {
    PathBuf::from(std::env::var("CEREBRO_DIR").unwrap_or_else(|_| r"E:\Cerebro".into()))
}

fn ler(p: PathBuf) -> Value {
    std::fs::read_to_string(p).ok().and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok()).unwrap_or(Value::Null)
}

/// (carimbo "AAAAMMDD-HHMMSS", "AAAA-MM-DD HH:MM") no horário de Brasília
fn agora() -> (String, String) {
    let s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0) - 3 * 3600;
    let (dias, resto) = (s.div_euclid(86400), s.rem_euclid(86400));
    let z = dias + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (format!("{y:04}{m:02}{d:02}-{:02}{:02}{:02}", resto / 3600, resto % 3600 / 60, resto % 60), format!("{y:04}-{m:02}-{d:02} {:02}:{:02}", resto / 3600, resto % 3600 / 60))
}

fn pedidos(r: &std::path::Path) -> Vec<Value> {
    let mut v: Vec<Value> = std::fs::read_dir(r.join("pedidos")).map(|rd| rd.flatten().map(|e| ler(e.path())).filter(|x| !x.is_null()).collect()).unwrap_or_default();
    v.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    v
}

pub fn indice(r: &std::path::Path) -> Value {
    let ped = pedidos(r);
    let conta = |e: &str| ped.iter().filter(|p| p["estado"] == e).count();
    let handoffs: Vec<Value> = std::fs::read_dir(r.join("handoffs")).map(|rd| rd.flatten().filter(|e| e.path().extension().map_or(false, |x| x == "md")).map(|e| {
        let t = std::fs::read_to_string(e.path()).unwrap_or_default();
        json!({ "ia": e.path().file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(), "resumo": t.chars().take(600).collect::<String>() })
    }).collect()).unwrap_or_default();
    json!({
        "indice": std::fs::read_to_string(r.join("INDEX.md")).unwrap_or_default(),
        "contagem": { "novo": conta("novo"), "andando": conta("andando"), "feito": conta("feito"), "devolvido": conta("devolvido") },
        "pedidos": ped,
        "handoffs": handoffs,
        "atualizado": std::fs::metadata(r.join("INDEX.md")).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()),
        "projetos": "use GET /api/projetos (o hub não guarda cópia própria no Brani)"
    })
}

pub fn abrir(r: &std::path::Path, v: &Value) -> Result<Value, String> {
    let t = |k: &str| v[k].as_str().unwrap_or("").trim().to_string();
    let (de, para, ass, ctx) = (t("de"), t("para"), t("assunto"), t("contexto"));
    if !IAS.contains(&de.as_str()) || !IAS.contains(&para.as_str()) {
        return Err(format!("de/para devem ser um de: {}", IAS.join(", ")));
    }
    if ass.is_empty() || ass.chars().count() > 200 || ctx.chars().count() > 8000 {
        return Err("assunto de 1 a 200 caracteres, contexto até 8000".into());
    }
    if pe::sensivel(&ass) || pe::sensivel(&ctx) {
        return Err("recusado: o texto parece ter segredo ou dado pessoal".into());
    }
    let (carimbo, hora) = agora();
    let id = format!("P-{carimbo}-{}", pedidos(r).len() + 1);
    let p = json!({ "id": id, "de": de, "para": para, "assunto": ass, "estado": "novo", "contexto": ctx, "arquivos": v["arquivos"].as_array().cloned().unwrap_or_default(), "resultado": "", "criado": hora, "atualizado": hora });
    std::fs::create_dir_all(r.join("pedidos")).map_err(|e| e.to_string())?;
    std::fs::write(r.join("pedidos").join(format!("{id}.json")), serde_json::to_string_pretty(&p).unwrap_or_default()).map_err(|e| e.to_string())?;
    Ok(p)
}

pub fn get(url: &str) -> Option<Resp> {
    Some(match url.split('?').next().unwrap_or("") {
        "/cerebro" => crate::pagina(include_str!("cerebro.html")),
        "/api/cerebro/indice" => dado(indice(&raiz())),
        _ => return None,
    })
}

/// O cabeçalho X-Braniac é exigido antes, no `main.rs` (todo POST).
pub fn post(caminho: &str, v: &Value) -> Option<Resp> {
    Some(match caminho {
        "/api/cerebro/pedido" => abrir(&raiz(), v).map(dado).unwrap_or_else(|e| erro(e, 400)),
        _ => return None,
    })
}

#[cfg(test)]
mod testes {
    use super::*;

    fn pasta(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("brani_cerebro_{nome}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("pedidos")).unwrap();
        std::fs::create_dir_all(d.join("privado")).unwrap();
        std::fs::write(d.join("privado").join("x.md"), "so-privado").unwrap();
        std::fs::write(d.join("INDEX.md"), "# indice\n- item").unwrap();
        d
    }

    #[test]
    fn pedido_vira_arquivo_e_aparece_no_indice() {
        let d = pasta("ok");
        let p = abrir(&d, &json!({ "de": "brani", "para": "codex", "assunto": "revisar", "contexto": "ctx" })).unwrap();
        assert_eq!(p["estado"], "novo");
        let i = indice(&d);
        assert_eq!(i["contagem"]["novo"], 1);
        assert_eq!(i["pedidos"][0]["para"], "codex");
        assert!(i["indice"].as_str().unwrap().contains("# indice"));
    }

    #[test]
    fn recusa_ia_desconhecida_e_segredo_e_nao_expoe_privado() {
        let d = pasta("recusa");
        assert!(abrir(&d, &json!({ "de": "estranho", "para": "codex", "assunto": "a" })).is_err());
        assert!(abrir(&d, &json!({ "de": "brani", "para": "codex", "assunto": "a", "contexto": "senha = abc12345xyz" })).is_err());
        assert!(!indice(&d).to_string().contains("so-privado"));
        assert_eq!(std::fs::read_dir(d.join("pedidos")).unwrap().count(), 0);
    }
}
