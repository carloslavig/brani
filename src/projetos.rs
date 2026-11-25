//! Tela "Projetos": registro dos projetos do Dono e do Parceiro em dados/projetos.json.
//! GET /projetos (tela), GET /api/projetos (json + contagem de pendencias), GET /api/projetos/sugestoes (vigia),
//! POST /api/projetos/pendencia {id, i, feita}, /evento {projeto, tarefa, estado, nota?, origem?}, /sugestao {chave, acao}, /ajuda {id, texto}.
//! So le e grava dados/projetos*.json(l); nao envia nada para fora (a ajuda usa so o roteador de IAs, com contexto do projeto).
use crate::projetos_eventos as pe;
use crate::{dado, erro, Resp};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static TRAVA: Mutex<()> = Mutex::new(());

fn arquivo(dados: &Path) -> PathBuf {
    dados.join("projetos.json")
}

pub fn carregar(arq: &Path) -> Result<Value, String> {
    let t = std::fs::read_to_string(arq).map_err(|e| format!("projetos.json: {e}"))?;
    let mut v: Value = serde_json::from_str(&t).map_err(|e| format!("projetos.json invalido: {e}"))?;
    if !v["projetos"].is_array() {
        return Err("projetos.json sem a lista 'projetos'".into());
    }
    for p in v["projetos"].as_array_mut().unwrap() {
        let abertas = p["pendencias"].as_array().map_or(0, |l| l.iter().filter(|x| x["feita"] != true).count());
        p["abertas"] = json!(abertas);
    }
    Ok(v)
}

fn guardar(arq: &Path, v: &Value) -> Result<(), String> {
    let tmp = arq.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(v).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, arq).map_err(|e| e.to_string())
}

/// Marca (ou desmarca) a pendencia `i` do projeto `id` e grava o arquivo.
pub fn marcar(arq: &Path, id: &str, i: usize, feita: bool) -> Result<Value, String> {
    let _g = TRAVA.lock().unwrap_or_else(|e| e.into_inner());
    let t = std::fs::read_to_string(arq).map_err(|e| e.to_string())?;
    let mut v: Value = serde_json::from_str(&t).map_err(|e| e.to_string())?;
    let p = v["projetos"].as_array_mut().and_then(|l| l.iter_mut().find(|p| p["id"] == id)).ok_or("projeto nao existe")?;
    let pend = p["pendencias"].as_array_mut().and_then(|l| l.get_mut(i)).ok_or("pendencia nao existe")?;
    pend["feita"] = json!(feita);
    guardar(arq, &v)?;
    Ok(json!({ "ok": true }))
}

fn hoje() -> String {
    crate::tempo::agora().data.iso()
}

fn historico(dados: &Path, linha: Value) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dados.join("projetos-eventos.jsonl")) {
        let _ = writeln!(f, "{linha}");
    }
}

/// Acha o projeto por id exato ou por nome aproximado (so aceita casamento claro).
fn achar(v: &Value, nome: &str) -> Option<usize> {
    let l = v["projetos"].as_array()?;
    if let Some(i) = l.iter().position(|p| p["id"] == nome) {
        return Some(i);
    }
    l.iter()
        .enumerate()
        .map(|(i, p)| (i, pe::semelhanca(nome, p["nome"].as_str().unwrap_or("")).max(pe::semelhanca(nome, p["id"].as_str().unwrap_or("")))))
        .filter(|x| x.1 >= 0.5)
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|x| x.0)
}

/// Evento EXPLICITO (so ele marca feito): o texto recebido e dado, nunca instrucao.
pub fn evento(dados: &Path, v: &Value) -> Result<Value, String> {
    let (proj, tarefa, estado) = (v["projeto"].as_str().unwrap_or("").trim(), v["tarefa"].as_str().unwrap_or(""), v["estado"].as_str().unwrap_or(""));
    let nota = v["nota"].as_str().filter(|n| !n.trim().is_empty());
    if proj.is_empty() || proj.chars().count() > 80 || nota.map_or(false, |n| n.chars().count() > 200 || pe::sensivel(n)) {
        return Err("projeto ou nota invalidos".into());
    }
    let _g = TRAVA.lock().unwrap_or_else(|e| e.into_inner());
    let arq = arquivo(dados);
    let mut j: Value = serde_json::from_str(&std::fs::read_to_string(&arq).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let i = achar(&j, proj).ok_or("projeto nao encontrado")?;
    let p = &mut j["projetos"][i];
    let r = pe::aplicar(p, tarefa, estado, nota, &hoje())?;
    let id = p["id"].clone();
    guardar(&arq, &j)?;
    let origem: String = v["origem"].as_str().unwrap_or("?").chars().take(40).collect();
    historico(dados, json!({ "data": hoje(), "projeto": id, "tarefa": tarefa, "estado": estado, "nota": nota, "origem": origem, "resultado": r }));
    Ok(json!({ "ok": true, "projeto": id, "resultado": r }))
}

fn arq_sug(dados: &Path) -> PathBuf {
    dados.join("projetos-sugestoes.json")
}

fn chave(id: &str, texto: &str) -> String {
    format!("{id}|{}", pe::normalizar(texto))
}

fn agora_s() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn ler_json(arq: &Path) -> Value {
    std::fs::read_to_string(arq).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null)
}

/// Sinais baratos e locais por projeto: commits recentes, TROCA-PENDENTE.md e tickets do NexoDev prontos/entregues.
fn sinais_do(p: &Value, tickets: &[Value]) -> Vec<(String, &'static str, f64)> {
    let (id, pasta) = (p["id"].as_str().unwrap_or(""), Path::new(p["pasta"].as_str().unwrap_or("")));
    let mut s: Vec<(String, &'static str, f64)> = vec![];
    if pasta.join(".git").exists() {
        if let Ok(o) = crate::oculto::cmd("git").arg("-C").arg(pasta).args(["log", "--since=2.days", "--pretty=%s", "-n", "30"]).output() {
            s.extend(String::from_utf8_lossy(&o.stdout).lines().map(|l| (l.to_string(), "commit recente", 0.4)));
        }
    }
    if let Ok(t) = std::fs::read_to_string(pasta.join("TROCA-PENDENTE.md")) {
        s.extend(t.lines().filter(|l| { let n = pe::normalizar(l); n.contains("feito") || n.contains("concluid") || n.contains("compilado") }).map(|l| (l.to_string(), "TROCA-PENDENTE.md", 0.5)));
    }
    let nome = pe::normalizar(p["nome"].as_str().unwrap_or(""));
    for t in tickets {
        let (est, pj) = (t["estado"].as_str().unwrap_or(""), pe::normalizar(t["projeto"].as_str().unwrap_or("")));
        if (est == "entregue" || est == "pronto") && !pj.is_empty() && (pe::normalizar(id).contains(&pj) || nome.contains(&pj)) {
            s.push((t["titulo"].as_str().unwrap_or("").to_string(), "ticket NexoDev", 0.5));
        }
    }
    s
}

/// Vigia: PROPOE "provavelmente feito"; nunca grava em projetos.json (so evento explicito marca).
pub fn vigiar(dados: &Path) -> Value {
    let v = carregar(&arquivo(dados)).unwrap_or(json!({ "projetos": [] }));
    let ign: Vec<Value> = ler_json(&arq_sug(dados))["ignoradas"].as_array().cloned().unwrap_or_default();
    let tickets: Vec<Value> = ler_json(&dados.join("empresa_dev").join("tickets.json")).as_array().cloned().unwrap_or_default();
    let mut itens: Vec<Value> = vec![];
    for p in v["projetos"].as_array().unwrap_or(&vec![]) {
        let id = p["id"].as_str().unwrap_or("");
        let abertas: Vec<&str> = p["pendencias"].as_array().map(|l| l.iter().filter(|x| x["feita"] != true).filter_map(|x| x["texto"].as_str()).collect()).unwrap_or_default();
        if abertas.is_empty() {
            continue;
        }
        let sinais = sinais_do(p, &tickets);
        for texto in abertas {
            let k = chave(id, texto);
            if ign.contains(&json!(k)) {
                continue;
            }
            let melhor = sinais.iter().map(|s| (pe::semelhanca(&s.0, texto), s)).filter(|(m, s)| *m >= s.2).max_by(|a, b| a.0.total_cmp(&b.0));
            if let Some((_, (txt, fonte, _))) = melhor {
                itens.push(json!({ "chave": k, "id": id, "nome": p["nome"], "tarefa": texto, "motivo": format!("{fonte}: {}", txt.chars().take(100).collect::<String>()) }));
            }
        }
    }
    let r = json!({ "geradas": agora_s(), "itens": itens, "ignoradas": ign });
    let _ = guardar(&arq_sug(dados), &r);
    r
}

/// Reaproveita o ultimo resultado por 3 minutos ("a cada poucos minutos", sem thread nova).
fn sugestoes(dados: &Path) -> Value {
    let ant = ler_json(&arq_sug(dados));
    if agora_s().saturating_sub(ant["geradas"].as_u64().unwrap_or(0)) > 180 { vigiar(dados) } else { ant }
}

fn decidir_sugestao(dados: &Path, k: &str, confirmar: bool) -> Result<Value, String> {
    let mut s = sugestoes(dados);
    let it = s["itens"].as_array().and_then(|l| l.iter().find(|x| x["chave"] == k)).cloned().ok_or("sugestao nao existe mais")?;
    let r = if confirmar { evento(dados, &json!({ "projeto": it["id"], "tarefa": it["tarefa"], "estado": "feito", "nota": "confirmado pelo Dono (sugestao do vigia)", "origem": "vigia-confirmado" }))? } else { json!({ "ok": true }) };
    s["itens"] = json!(s["itens"].as_array().unwrap().iter().filter(|x| x["chave"] != k).collect::<Vec<_>>());
    if !confirmar {
        if let Some(l) = s["ignoradas"].as_array_mut() {
            l.push(json!(k));
        } else {
            s["ignoradas"] = json!([k]);
        }
    }
    guardar(&arq_sug(dados), &s)?;
    Ok(r)
}

/// "Como resolver": 2 a 4 caminhos curtos, so com contexto do projeto (nada pessoal); cache por pendencia.
pub fn ajuda(dados: &Path, ia: &dyn Fn(&str, &str) -> Result<String, String>, v: &Value) -> Result<Value, String> {
    let (id, texto) = (v["id"].as_str().unwrap_or(""), v["texto"].as_str().unwrap_or("").trim());
    if texto.is_empty() || texto.chars().count() > 400 || pe::sensivel(texto) {
        return Err("texto invalido".into());
    }
    let arq = dados.join("projetos-ajuda.json");
    let mut cache = ler_json(&arq);
    if !cache.is_object() {
        cache = json!({});
    }
    let k = chave(id, texto);
    if v["regen"] != true {
        if let Some(c) = cache.get(&k) {
            return Ok(json!({ "opcoes": c, "cache": true }));
        }
    }
    let proj = carregar(&arquivo(dados))?["projetos"].as_array().and_then(|l| l.iter().find(|p| p["id"] == id).cloned()).ok_or("projeto nao existe")?;
    let ctx = format!("Projeto: {}. Grupo: {}. Estado: {}. Proximos passos: {}. Bloqueios: {}.", proj["nome"], proj["grupo"], proj["estado"], proj["proximos"], proj["bloqueios"]);
    if pe::sensivel(&ctx) {
        return Err("contexto do projeto tem dado sensivel".into());
    }
    let pedido = format!("{ctx}\nTarefa a resolver: {texto}\nDe 2 a 4 jeitos curtos de resolver. Responda SO um JSON: [{{\"titulo\":\"...\",\"como\":\"1 frase\",\"esforco\":\"rapido|medio|longo\",\"quem\":\"Claude|NexoDev|Dono\"}}]");
    let r = ia("Voce ajuda a decidir o proximo passo de projetos de software. Responda em portugues, so o JSON pedido.", &pedido)?;
    let (a, b) = (r.find('[').ok_or("resposta sem lista")?, r.rfind(']').ok_or("resposta sem lista")?);
    let l: Vec<Value> = serde_json::from_str(r.get(a..=b).ok_or("resposta ilegivel")?).map_err(|e| format!("resposta ilegivel: {e}"))?;
    let corta = |x: &Value, n: usize| x.as_str().unwrap_or("").chars().take(n).collect::<String>();
    let ops: Vec<Value> = l.iter().take(4).map(|o| json!({ "titulo": corta(&o["titulo"], 80), "como": corta(&o["como"], 220), "esforco": corta(&o["esforco"], 10), "quem": corta(&o["quem"], 10) })).filter(|o| o["titulo"] != "").collect();
    if ops.len() < 2 {
        return Err("a IA nao devolveu opcoes".into());
    }
    cache[&k] = json!(ops);
    guardar(&arq, &cache)?;
    Ok(json!({ "opcoes": ops, "cache": false }))
}

pub fn get(dados: &Path, url: &str) -> Option<Resp> {
    Some(match url.split('?').next().unwrap_or("") {
        "/projetos" => crate::pagina(include_str!("projetos.html")),
        "/api/projetos" => carregar(&arquivo(dados)).map(dado).unwrap_or_else(|e| erro(e, 500)),
        "/api/projetos/sugestoes" => dado(sugestoes(dados)),
        _ => return None,
    })
}

/// O cabecalho X-Braniac e exigido antes, no `main.rs` (todo POST).
pub fn post(dados: &Path, ia: &dyn Fn(&str, &str) -> Result<String, String>, caminho: &str, v: &Value) -> Option<Resp> {
    Some(match caminho {
        "/api/projetos/pendencia" => match v["i"].as_u64() {
            Some(i) => marcar(&arquivo(dados), v["id"].as_str().unwrap_or(""), i as usize, v["feita"].as_bool().unwrap_or(true)).map(dado).unwrap_or_else(|e| erro(e, 400)),
            None => erro("i invalido", 400),
        },
        "/api/projetos/evento" => evento(dados, v).map(dado).unwrap_or_else(|e| erro(e, 400)),
        "/api/projetos/sugestao" => decidir_sugestao(dados, v["chave"].as_str().unwrap_or(""), v["acao"] == "confirmar").map(dado).unwrap_or_else(|e| erro(e, 400)),
        "/api/projetos/ajuda" => ajuda(dados, ia, v).map(dado).unwrap_or_else(|e| erro(e, 502)),
        _ => return None,
    })
}

#[cfg(test)]
mod testes {
    use super::*;

    fn pasta(nome: &str, json: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("brani_pj_{nome}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("projetos.json"), json).unwrap();
        d
    }
    const UM: &str = r#"{"projetos":[{"id":"x","nome":"Projeto X","grupo":"G","estado":"andando","pendencias":[{"texto":"Conferir o painel da esteira","feita":false},{"texto":"Ligar o relogio","feita":false}],"proximos":["Abrir a tela nova"]}]}"#;

    #[test]
    fn carrega_o_projetos_json_de_verdade() {
        if !Path::new(env!("CARGO_MANIFEST_DIR")).join("dados").join("projetos.json").exists() { return; } // dados reais nao vao ao Git
        let v = carregar(&Path::new(env!("CARGO_MANIFEST_DIR")).join("dados").join("projetos.json")).unwrap();
        let l = v["projetos"].as_array().unwrap();
        assert!(l.len() >= 20);
        assert!(l.iter().all(|p| p["id"].is_string() && p["nome"].is_string() && p["grupo"].is_string() && p["abertas"].is_number()));
    }

    #[test]
    fn marcar_pendencia_grava_e_reduz_a_contagem() {
        let d = std::env::temp_dir().join("brani_teste_projetos.json");
        std::fs::write(&d, r#"{"projetos":[{"id":"x","nome":"X","grupo":"G","pendencias":[{"texto":"a","feita":false}]}]}"#).unwrap();
        assert_eq!(carregar(&d).unwrap()["projetos"][0]["abertas"], 1);
        marcar(&d, "x", 0, true).unwrap();
        assert_eq!(carregar(&d).unwrap()["projetos"][0]["abertas"], 0);
        assert!(marcar(&d, "x", 5, true).is_err() && marcar(&d, "nao", 0, true).is_err());
        let _ = std::fs::remove_file(&d);
    }

    #[test]
    fn evento_feito_casa_a_pendencia_e_grava_historico() {
        let d = pasta("feito", UM);
        let r = evento(&d, &json!({ "projeto": "projeto x", "tarefa": "conferir painel esteira", "estado": "feito", "origem": "teste" })).unwrap();
        assert_eq!(r["resultado"], "feita");
        assert_eq!(carregar(&arquivo(&d)).unwrap()["projetos"][0]["pendencias"][0]["feita"], true);
        assert!(std::fs::read_to_string(d.join("projetos-eventos.jsonl")).unwrap().contains("conferir painel esteira"));
    }

    #[test]
    fn evento_novo_cria_pendencia_e_ultimo_feito_promove_o_proximo() {
        let d = pasta("novo", UM);
        evento(&d, &json!({ "projeto": "x", "tarefa": "Escrever o relatorio final", "estado": "novo" })).unwrap();
        assert_eq!(carregar(&arquivo(&d)).unwrap()["projetos"][0]["pendencias"].as_array().unwrap().len(), 3);
        for t in ["Conferir o painel da esteira", "Ligar o relogio", "Escrever o relatorio final"] {
            evento(&d, &json!({ "projeto": "x", "tarefa": t, "estado": "feito" })).unwrap();
        }
        let p = &carregar(&arquivo(&d)).unwrap()["projetos"][0];
        assert_eq!(p["abertas"], 1); // "Abrir a tela nova" virou pendencia
        assert!(p["proximos"].as_array().unwrap().is_empty());
    }

    #[test]
    fn evento_recusa_segredo_projeto_inexistente_e_estado_invalido() {
        let d = pasta("recusa", UM);
        assert!(evento(&d, &json!({ "projeto": "x", "tarefa": "usar a senha: abc123", "estado": "novo" })).is_err());
        assert!(evento(&d, &json!({ "projeto": "x", "tarefa": "ok", "estado": "feito", "nota": "CPF 123.456.789-09" })).is_err());
        assert!(evento(&d, &json!({ "projeto": "zzzzqqq", "tarefa": "ok", "estado": "feito" })).is_err());
        assert!(evento(&d, &json!({ "projeto": "x", "tarefa": "ok", "estado": "inventado" })).is_err());
        assert_eq!(carregar(&arquivo(&d)).unwrap()["projetos"][0]["abertas"], 2);
    }

    #[test]
    fn vigia_so_sugere_e_nunca_marca_feito() {
        let d = pasta("vigia", UM);
        let g = d.join("repo");
        std::fs::create_dir_all(&g).unwrap();
        std::fs::write(g.join("TROCA-PENDENTE.md"), "Conferir o painel da esteira: feito e compilado\n").unwrap();
        let mut j = ler_json(&arquivo(&d));
        j["projetos"][0]["pasta"] = json!(g.to_string_lossy());
        guardar(&arquivo(&d), &j).unwrap();
        let s = vigiar(&d);
        assert_eq!(s["itens"].as_array().unwrap().len(), 1);
        assert_eq!(carregar(&arquivo(&d)).unwrap()["projetos"][0]["abertas"], 2); // inferencia nao marca
        decidir_sugestao(&d, s["itens"][0]["chave"].as_str().unwrap(), false).unwrap();
        assert!(vigiar(&d)["itens"].as_array().unwrap().is_empty()); // ignorada nao volta
    }

    #[test]
    fn post_so_chega_aqui_depois_do_cabecalho_x_braniac() {
        let m = include_str!("main.rs");
        assert!(m.find("falta o cabecalho X-Braniac").unwrap() < m.find("projetos::post(").unwrap());
    }

    #[test]
    fn ajuda_usa_cache_e_recusa_texto_sensivel() {
        let d = pasta("ajuda", UM);
        let ia = |_: &str, _: &str| Ok(r#"[{"titulo":"A","como":"x","esforco":"rapido","quem":"Claude"},{"titulo":"B","como":"y","esforco":"longo","quem":"Dono"}]"#.to_string());
        let r = ajuda(&d, &ia, &json!({ "id": "x", "texto": "Ligar o relogio" })).unwrap();
        assert_eq!(r["opcoes"].as_array().unwrap().len(), 2);
        let sem_ia = |_: &str, _: &str| Err("fora".to_string());
        assert_eq!(ajuda(&d, &sem_ia, &json!({ "id": "x", "texto": "Ligar o relogio" })).unwrap()["cache"], true);
        assert!(ajuda(&d, &ia, &json!({ "id": "x", "texto": "usar a senha: 1234" })).is_err());
    }
}
