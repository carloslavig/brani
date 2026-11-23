//! Painel de pendencias e erros (porte de braniac/pendencias.py): o que espera o OK do dono e o que esta quebrado, num lugar so.
//! So LE arquivos que ja existem em `dados` (sem IA, sem rede). Regra que vem do Python: nenhuma rotina manda pendencia sozinha;
//! `fala` so e usada quando o dono PERGUNTA. Por isso aqui nao existe (nem deve existir) funcao de cobrar/avisar.
//! Diferenca do Python: a lista do que so o dono resolve fica em dados/pendencias_dono.md e a chave do painel e "so_dono".
#![allow(dead_code)]
use serde_json::{json, Value};
use std::path::Path;

fn json_de(dados: &Path, rel: &str) -> Value {
    std::fs::read_to_string(dados.join(rel)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null)
}

fn corta(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn texto(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}

/// Propostas de melhoria (dados/propostas/*.md) ainda "AGUARDANDO APROVA...", da mais nova para a mais velha pelo nome.
fn propostas(dados: &Path) -> Vec<Value> {
    let mut arqs: Vec<_> = std::fs::read_dir(dados.join("propostas")).into_iter().flatten().flatten().map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md")).collect();
    arqs.sort();
    arqs.reverse();
    let mut out = Vec::new();
    for f in arqs {
        let Ok(bytes) = std::fs::read(&f) else { continue };
        let txt = String::from_utf8_lossy(&bytes);
        if !corta(&txt, 600).to_uppercase().contains("AGUARDANDO APROVA") {
            continue;
        }
        let nome = |p: Option<&std::ffi::OsStr>| p.map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
        let tema = txt.lines().find_map(|l| l.strip_prefix("Tema:")).map(|t| t.trim().to_string()).unwrap_or_else(|| nome(f.file_stem()));
        let ts = f.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0.0, |d| d.as_secs_f64());
        out.push(json!({ "agente": "Alma/Forja", "tipo": "ok", "titulo": format!("Proposta de melhoria: {}", corta(&tema, 90)), "detalhe": nome(f.file_name()), "ts": ts }));
    }
    out
}

/// Consultas do Airbnb paradas esperando o OK de alguem da casa.
fn reservas(dados: &Path) -> Vec<Value> {
    let est = json_de(dados, "airbnb_estado.json");
    est["consultas"].as_object().into_iter().flatten().map(|(_, c)| c).filter(|c| c["estado"] == "consulta").map(|c| {
        let ou = |a: &str, b: &str, padrao: &str| [texto(&c[a]), texto(&c[b]), padrao].into_iter().find(|x| !x.is_empty()).unwrap_or("").to_string();
        json!({ "agente": "Jorge", "tipo": "ok", "titulo": format!("Aguardando OK: {}", corta(&ou("hospede", "nome", "reserva/hóspede"), 60)),
                "detalhe": corta(&ou("pergunta", "resumo", ""), 160), "ts": c["ts"].as_f64().unwrap_or(0.0) })
    }).collect()
}

fn erros(dados: &Path) -> Vec<Value> {
    let mut out = Vec::new();
    let m = json_de(dados, "manutencao.json");
    if m["testes_ok"] == false {
        let falhas: Vec<String> = m["falhas"].as_array().filter(|a| !a.is_empty()).map(|a| a.iter().map(|f| texto(f).to_string()).collect()).unwrap_or_else(|| vec!["teste falhando".into()]);
        for f in falhas.into_iter().take(5) {
            out.push(json!({ "agente": "Forja", "tipo": "erro", "titulo": "Teste falhando na madrugada", "detalhe": f, "ts": 0 }));
        }
    }
    let vaq = &json_de(dados, "vaquinha.json")["ultimo_erro"];
    if !vaq.is_null() && *vaq != false && *vaq != "" {
        let d = vaq.as_str().map(String::from).unwrap_or_else(|| vaq.to_string());
        out.push(json!({ "agente": "Vaquinha", "tipo": "erro", "titulo": "Último deploy do site falhou", "detalhe": corta(&d, 160), "ts": 0 }));
    }
    for (modulo, arq, ruim) in [("NexoDev", "empresa_dev/tickets.json", "bloqueado"), ("Fornalha", "empresa_mkt/pedidos.json", "falhou")] {
        for t in json_de(dados, arq).as_array().into_iter().flatten().filter(|t| t["estado"] == ruim) {
            let h = t["historico"].as_array().and_then(|h| h.last()).map_or("", |u| texto(&u["txt"]));
            let id = t["id"].as_str().map(String::from).unwrap_or_else(|| t["id"].to_string());
            let titulo = if texto(&t["titulo"]).is_empty() { texto(&t["tipo"]) } else { texto(&t["titulo"]) };
            out.push(json!({ "agente": modulo, "tipo": "erro", "titulo": format!("{id} {ruim}: {}", corta(titulo, 70)), "detalhe": corta(h, 160), "ts": t["criado"].as_f64().unwrap_or(0.0) }));
        }
    }
    out
}

/// Linhas `- [Área] texto` de dados/pendencias_dono.md: o que so o dono pode fazer (login, conta, OK).
/// Depois de um titulo "## ... (Claude)" as linhas sao do que anda sem depender do dono (tipo "claude").
pub fn so_dono(dados: &Path) -> Vec<Value> {
    let Ok(txt) = std::fs::read_to_string(dados.join("pendencias_dono.md")) else { return Vec::new() };
    let (mut out, mut tipo) = (Vec::new(), "dono");
    for l in txt.lines().map(str::trim) {
        if l.starts_with("## ") {
            tipo = if l.to_lowercase().contains("claude") { "claude" } else { "dono" };
        } else if let Some(resto) = l.strip_prefix("- [") {
            let (area, t) = resto.split_once(']').unwrap_or((resto, ""));
            out.push(json!({ "agente": area.trim(), "tipo": tipo, "titulo": t.trim(), "detalhe": "", "ts": 0 }));
        }
    }
    out
}

/// O painel inteiro. `total` conta o que espera o dono e os erros (o que esta "comigo" nao entra).
pub fn painel(dados: &Path, ts: f64) -> Value {
    let (mut ok, erros, lista) = (propostas(dados), erros(dados), so_dono(dados));
    ok.extend(reservas(dados));
    let (dono, claude): (Vec<Value>, Vec<Value>) = lista.into_iter().partition(|x| x["tipo"] == "dono");
    json!({ "ts": ts, "total": ok.len() + erros.len() + dono.len(), "aguardando": ok, "erros": erros, "so_dono": dono, "comigo": claude })
}

/// Resposta curta (vai em audio) para quando o dono PERGUNTA das pendencias. Pura: recebe o painel pronto.
pub fn fala(p: &Value, maximo: usize) -> String {
    let vazio = Vec::new();
    let (c, ok) = (p["so_dono"].as_array().unwrap_or(&vazio), p["aguardando"].as_array().unwrap_or(&vazio));
    if c.is_empty() && ok.is_empty() {
        return "Nenhuma pendência esperando você agora.".into();
    }
    let mut partes = Vec::new();
    if !c.is_empty() {
        let itens: Vec<String> = c.iter().take(maximo)
            .map(|x| format!("{}: {}", texto(&x["agente"]), texto(&x["titulo"]).split('(').next().unwrap_or("").trim().trim_end_matches('.'))).collect();
        partes.push(format!("Tem {} coisas paradas que só você pode resolver. As primeiras são: {}.", c.len(), itens.join("; ")));
    }
    if !ok.is_empty() {
        partes.push(format!("E {} esperando o seu OK.", ok.len()));
    }
    partes.push("O mapa completo está no computador, na tela do mapa do Braniac.".into());
    partes.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pasta(nome: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("braniac-pendencias-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn painel_junta_ok_e_erros() {
        let p = pasta("painel");
        for d in ["propostas", "empresa_dev", "empresa_mkt"] {
            std::fs::create_dir_all(p.join(d)).unwrap();
        }
        std::fs::write(p.join("propostas/2026-11-01-a.md"), "# Proposta\nEstado: AGUARDANDO APROVAÇÃO\nTema: voz mais calma\n").unwrap();
        std::fs::write(p.join("propostas/2026-11-02-b.md"), "# Proposta\nEstado: aprovada\nTema: outra\n").unwrap();
        std::fs::write(p.join("propostas/nota.txt"), "AGUARDANDO APROVAÇÃO").unwrap();
        std::fs::write(p.join("airbnb_estado.json"), r#"{"consultas": {"1": {"estado": "consulta", "hospede": "Ana", "pergunta": "tem vaga?", "ts": 5},
            "2": {"estado": "respondida", "hospede": "Bia"}, "3": {"estado": "consulta"}}}"#).unwrap();
        std::fs::write(p.join("manutencao.json"), r#"{"testes_ok": false, "falhas": ["test_a", "test_b"]}"#).unwrap();
        std::fs::write(p.join("vaquinha.json"), r#"{"ultimo_erro": "push recusado"}"#).unwrap();
        std::fs::write(p.join("empresa_dev/tickets.json"), r#"[{"id": "T001", "estado": "bloqueado", "titulo": "tela", "historico": [{"txt": "a"}, {"txt": "build quebrou"}], "criado": 9},
            {"id": "T002", "estado": "pronto"}]"#).unwrap();
        std::fs::write(p.join("empresa_mkt/pedidos.json"), r#"[{"id": 7, "estado": "falhou", "tipo": "video"}]"#).unwrap();
        std::fs::write(p.join("pendencias_dono.md"), "# t\n- [Gmail] Conectar o Gmail\ntexto solto\n## Comigo (Claude)\n- [Site] publicando\n").unwrap();
        let x = painel(&p, 1.0);
        let titulos = |k: &str| x[k].as_array().unwrap().iter().map(|i| i["titulo"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        assert_eq!(titulos("aguardando"), ["Proposta de melhoria: voz mais calma", "Aguardando OK: Ana", "Aguardando OK: reserva/hóspede"]);
        assert_eq!(x["aguardando"][0]["detalhe"], "2026-11-01-a.md");
        assert_eq!(titulos("erros"), ["Teste falhando na madrugada", "Teste falhando na madrugada", "Último deploy do site falhou", "T001 bloqueado: tela", "7 falhou: video"]);
        assert!(x["erros"][3]["detalhe"] == "build quebrou" && x["erros"][3]["agente"] == "NexoDev");
        assert!(x["so_dono"].as_array().unwrap().len() == 1 && x["comigo"][0]["agente"] == "Site");
        assert_eq!(x["total"], 3 + 5 + 1);
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn so_dono_le_lista_e_pasta_vazia_nao_quebra() {
        let p = pasta("lista");
        assert_eq!(painel(&p, 0.0)["total"], 0);
        std::fs::write(p.join("manutencao.json"), r#"{"testes_ok": false}"#).unwrap();
        assert_eq!(painel(&p, 0.0)["erros"][0]["detalhe"], "teste falhando");
        std::fs::write(p.join("pendencias_dono.md"), "# t\n- [Gmail] Conectar o Gmail\ntexto solto\n").unwrap();
        assert_eq!(so_dono(&p), vec![json!({"agente": "Gmail", "tipo": "dono", "titulo": "Conectar o Gmail", "detalhe": "", "ts": 0})]);
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn pergunta_devolve_a_lista_e_sem_pendencia_diz_que_nao_tem() {
        assert!(fala(&json!({"so_dono": [], "aguardando": [], "total": 0}), 4).contains("Nenhuma pendência"));
        let p = json!({"so_dono": [{"agente": "Gmail", "titulo": "Conectar (uma vez)."}, {"agente": "Site", "titulo": "Aprovar."}], "aguardando": [{}]});
        assert_eq!(fala(&p, 1), "Tem 2 coisas paradas que só você pode resolver. As primeiras são: Gmail: Conectar. E 1 esperando o seu OK. O mapa completo está no computador, na tela do mapa do Braniac.");
        assert!(fala(&json!({"aguardando": [{}]}), 4).starts_with("E 1 esperando o seu OK."));
    }

    /// Regra do dono: pendencia so quando ele pergunta. Este modulo so le e monta a fala; quem liga nao pode por `fala` em rotina.
    #[test]
    fn modulo_nao_tem_funcao_que_avisa_sozinha() {
        let fonte = include_str!("pendencias.rs");
        let codigo = fonte.split("#[cfg(test)]").next().unwrap();
        assert!(!codigo.contains("fn cobrar") && !codigo.contains("fn tick") && !codigo.contains("enviar"));
    }
}
