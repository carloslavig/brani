//! Fila de PRODUCAO do Link (link/producao.py): pedidos "producao_completa" do par, um por vez, na ordem. Estado em disco
//! (dados/link/producao/fila.json, formato do Python), sobrevive a reinicio. Cada mudanca de etapa vai para o banco do processo
//! e para o checklist das cidades (cidade ja pronta ou reservada pelo outro lado -> o pedido fica "esperando": nunca dois fazem
//! a mesma cidade). Pacote recebido e so DADO: o zip e conferido antes de abrir e nada dele e executado.
//! Ficou de fora: abrir/extrair o zip (nao ha crate de zip: a lista de entradas entra por parametro em `conferir_entradas`), o
//! texto da tarefa para a IA que produz (`tarefa`, com nomes de pessoas), a linha de comando e a chamada do Senior (o parecer
//! dele entra pronto em `entregar`; ver link_processo::parecer_senior).
#![allow(dead_code)]
use crate::{link_municipios as municipios, link_processo as processo};
use regex::Regex;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const ETAPAS: [&str; 10] = ["recebido", "pesquisa", "fontes_enviado", "fontes_aprovado", "roteiro", "voz", "video", "entregue", "erro", "esperando"];
pub const FINAIS: [&str; 2] = ["entregue", "erro"];
/// Esperando o par (aprovar fontes, destravar) nao segura a fila.
pub const NAO_BLOQUEIA: [&str; 4] = ["entregue", "erro", "esperando", "fontes_enviado"];
pub const PROIBIDAS: [&str; 14] = [".exe", ".bat", ".cmd", ".ps1", ".sh", ".py", ".js", ".msi", ".dll", ".vbs", ".scr", ".com", ".jar", ".lnk"];
pub const MAX_ARQS: usize = 2000;
pub const MAX_DESCOMP: u64 = 1024 * 1024 * 1024;
static TRAVA: Mutex<()> = Mutex::new(());

fn nome_base(n: &str) -> &str {
    n.trim_end_matches('/').rsplit('/').next().unwrap_or("")
}

fn sufixo(nome: &str) -> String {
    let nome = nome.trim_end_matches('.');
    nome.rfind('.').filter(|i| *i > 0).map(|i| nome[i..].to_lowercase()).unwrap_or_default()
}

/// Confere as entradas de um zip SEM extrair: (nome, tamanho descompactado, e pasta?). Devolve os nomes ou o motivo da recusa
/// (caminho absoluto, "..", executavel, muitos arquivos, grande demais).
pub fn conferir_entradas(entradas: &[(String, u64, bool)]) -> Result<Vec<String>, String> {
    if entradas.len() > MAX_ARQS {
        return Err(format!("zip recusado: mais de {MAX_ARQS} arquivos"));
    }
    if entradas.iter().map(|e| e.1).fold(0u64, u64::saturating_add) > MAX_DESCOMP {
        return Err("zip recusado: mais de 1 GB descompactado".into());
    }
    let mut nomes = vec![];
    for (nome, _, pasta) in entradas {
        let n = nome.replace('\\', "/");
        let b = n.as_bytes();
        if n.starts_with('/') || (b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':') || n.split('/').any(|p| p == "..") {
            return Err(format!("zip recusado: caminho invalido ({})", n.chars().take(60).collect::<String>()));
        }
        if !pasta && PROIBIDAS.contains(&sufixo(nome_base(&n)).as_str()) {
            return Err(format!("zip recusado: executavel dentro ({})", nome_base(&n).chars().take(60).collect::<String>()));
        }
        nomes.push(n);
    }
    Ok(nomes)
}

pub fn tem_instrucoes(nomes: &[String]) -> bool {
    let re = Regex::new(r"(?i)^(LEIA-PRIMEIRO|INSTRUCOES)[^/]*\.md$").expect("regex fixa");
    nomes.iter().any(|n| re.is_match(nome_base(n)))
}

fn titulo(s: &str) -> String {
    let mut inicio = true;
    s.chars().map(|c| {
        let r = if inicio { c.to_uppercase().next().unwrap_or(c) } else { c.to_lowercase().next().unwrap_or(c) };
        inicio = !c.is_alphabetic();
        r
    }).collect()
}

/// "143-ITATIM - instrucoes.zip" -> ("143", "Itatim"). Sem padrao: ("", "").
pub fn do_nome(nome: &str) -> (String, String) {
    let base = nome_base(&nome.replace('\\', "/")).to_string();
    let stem = base.rfind('.').filter(|i| *i > 0).map_or(base.as_str(), |i| &base[..i]);
    let re = Regex::new(r"^\s*(\d{1,4})\s*[-_ ·]+\s*([^\-_.·]+?)\s*(?:[-_.·]|$)").expect("regex fixa");
    re.captures(stem).map(|m| (m[1].to_string(), titulo(m[2].trim()))).unwrap_or_default()
}

// ---------- fila persistente ----------
fn pasta(dados: &Path) -> PathBuf {
    dados.join("link").join("producao")
}

fn ler(dados: &Path) -> Vec<Value> {
    let d: Value = std::fs::read_to_string(pasta(dados).join("fila.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    d["pedidos"].as_array().cloned().unwrap_or_default()
}

fn gravar(dados: &Path, pedidos: &[Value]) -> Result<(), String> {
    let p = pasta(dados);
    std::fs::create_dir_all(&p).map_err(|e| e.to_string())?;
    let tmp = p.join("fila.json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&json!({ "pedidos": pedidos })).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, p.join("fila.json")).map_err(|e| e.to_string())
}

/// Os pedidos na ordem da fila.
pub fn pedidos(dados: &Path) -> Vec<Value> {
    let mut v = ler(dados);
    v.sort_by(|a, b| {
        let k = |x: &Value| (x["ordem"].as_i64().unwrap_or(9999), x["criado"].as_f64().unwrap_or(0.0));
        k(a).partial_cmp(&k(b)).unwrap_or(std::cmp::Ordering::Equal)
    });
    v
}

pub fn get(dados: &Path, pid: &str) -> Option<Value> {
    ler(dados).into_iter().find(|x| x["id"] == pid)
}

fn texto(v: &Value, max: usize) -> String {
    let s = match v {
        Value::Null | Value::Bool(false) => String::new(),
        Value::Number(n) if n.as_f64() == Some(0.0) => String::new(),
        Value::String(s) => s.clone(),
        outro => outro.to_string(),
    };
    s.chars().take(max).collect()
}

/// Entra no fim da fila (ou na "ordem" pedida). Devolve a posicao entre os que ainda nao terminaram (1 = o proximo).
/// `fila_md`: o texto do arquivo da fila das cidades, para o checklist (None = checklist fora do ar, nao trava a producao).
pub fn adicionar(dados: &Path, fila_md: Option<&str>, pid: &str, par: &str, meta: &Value, material: &str, agora: f64) -> Result<usize, String> {
    {
        let _t = TRAVA.lock().map_err(|e| e.to_string())?;
        let mut d = ler(dados);
        if d.iter().any(|x| x["id"] == pid) {
            return Err("pedido ja existe".into());
        }
        let ordem = meta["ordem"].as_i64().unwrap_or_else(|| d.iter().map(|x| x["ordem"].as_i64().unwrap_or(0)).max().unwrap_or(0).max(0) + 1);
        d.push(json!({ "id": pid, "par": par, "numero": texto(&meta["numero"], usize::MAX), "cidade": texto(&meta["cidade"], 60), "uf": texto(&meta["uf"], 2), "ibge": texto(&meta["ibge"], 7),
            "instrucoes": texto(&meta["instrucoes"], 20000), "material": material, "ordem": ordem, "etapa": "recebido", "detalhe": "", "avisado": "recebido", "criado": agora }));
        gravar(dados, &d)?;
    }
    if let Some(x) = get(dados, pid) {
        espelho(dados, &x, "chegou da fila do par", agora);
        checklist(dados, fila_md, &x, agora);
    }
    pedidos(dados).iter().filter(|x| !FINAIS.contains(&x["etapa"].as_str().unwrap_or(""))).position(|x| x["id"] == pid).map(|i| i + 1).ok_or_else(|| "pedido nao encontrado".to_string())
}

/// Muda a etapa de um pedido. Etapa fora da lista e recusada. `extra`: outros campos a gravar junto (objeto ou null).
pub fn mudar(dados: &Path, fila_md: Option<&str>, pid: &str, etapa: &str, detalhe: &str, extra: &Value, agora: f64) -> Result<Value, String> {
    if !ETAPAS.contains(&etapa) {
        return Err(format!("etapa desconhecida: {etapa}"));
    }
    let x = {
        let _t = TRAVA.lock().map_err(|e| e.to_string())?;
        let mut d = ler(dados);
        let x = d.iter_mut().find(|x| x["id"] == pid).ok_or("pedido nao encontrado")?;
        if let (Some(o), Some(e)) = (x.as_object_mut(), extra.as_object()) {
            o.extend(e.clone());
        }
        x["etapa"] = json!(etapa);
        x["detalhe"] = json!(detalhe.chars().take(300).collect::<String>());
        x["atualizado"] = json!(agora);
        let x = x.clone();
        gravar(dados, &d)?;
        x
    };
    espelho(dados, &x, detalhe, agora);
    checklist(dados, fila_md, &x, agora);
    Ok(x)
}

/// Espelho no banco do processo (auditoria, tela /producao). Banco fora nao trava a fila.
fn espelho(dados: &Path, x: &Value, resumo: &str, agora: f64) {
    let s = |k: &str| x[k].as_str().unwrap_or("");
    let _ = processo::espelhar(dados, x, agora);
    let _ = processo::registrar(dados, agora, [s("id"), s("cidade"), s("etapa"), "fila", "etapa", "", "", resumo]);
}

/// Checklist das cidades: o pedido reserva a cidade, cada etapa renova, entregue marca "entregue" (rascunho, ainda falta a
/// aprovacao de uma pessoa), erro libera. Cidade ja pronta ou reservada pelo outro lado -> o pedido fica "esperando".
fn checklist(dados: &Path, fila_md: Option<&str>, x: &Value, agora: f64) {
    let s = |k: &str| x[k].as_str().unwrap_or("");
    let alvo = if s("cidade").is_empty() { s("numero") } else { s("cidade") };
    let Some(fila) = fila_md.filter(|_| !alvo.is_empty() && s("etapa") != "esperando") else { return };
    let r = match s("etapa") {
        "entregue" => municipios::passo(dados, fila, alvo, "braniac", "concluir", s("id"), agora),
        "erro" => municipios::passo(dados, fila, alvo, "braniac", "liberar", s("id"), agora),
        _ => municipios::reservar(dados, fila, alvo, "braniac", s("id"), agora),
    };
    let motivo = r["motivo"].as_str().unwrap_or("");
    if r["ok"] != true && !FINAIS.contains(&s("etapa")) && motivo != municipios::FORA_DA_FILA && !motivo.starts_with("banco das reservas falhou") {
        let _ = mudar(dados, fila_md, s("id"), "esperando", &format!("checklist: {} {motivo}", s("cidade")), &Value::Null, agora);
    }
}

pub fn marcar_avisado(dados: &Path, pid: &str, etapa: &str) -> Result<(), String> {
    let _t = TRAVA.lock().map_err(|e| e.to_string())?;
    let mut d = ler(dados);
    for x in d.iter_mut().filter(|x| x["id"] == pid) {
        x["avisado"] = json!(etapa);
    }
    gravar(dados, &d)
}

/// O pedido a trabalhar agora: o primeiro (na ordem) parado em "recebido" ou "fontes_aprovado", se nenhum outro estiver no meio de uma fase.
pub fn proximo(dados: &Path) -> Option<Value> {
    pedidos(dados).into_iter().find(|x| !NAO_BLOQUEIA.contains(&x["etapa"].as_str().unwrap_or(""))).filter(|x| x["etapa"] == "recebido" || x["etapa"] == "fontes_aprovado")
}

/// Texto de status que o outro lado le sozinho (nao mudar o comeco).
pub fn linha(x: &Value) -> String {
    let s = |k: &str| x[k].as_str().unwrap_or("");
    match s("etapa") {
        "entregue" => format!("FILA pedido {}: CONCLUIDO", s("id")),
        "esperando" => format!("FILA pedido {}: ESPERANDO · {}", s("id"), if s("detalhe").is_empty() { "sem motivo" } else { s("detalhe") }),
        "erro" => format!("FILA pedido {}: ERRO · {}", s("id"), s("detalhe")).trim_end_matches([' ', '·']).to_string(),
        e => format!("FILA pedido {}: {} · {}", s("id"), e.to_uppercase(), s("cidade")),
    }
}

/// Etapa 13: o Senior confere antes da entrega; `parecer` e o que ele devolveu ({aprovado, problemas}).
/// Ok(linha de status) = entregue, ou ESPERANDO na 3a reprovacao (a fila segue). Err = nao entregou: Senior sem resposta (nao
/// conta como reprovacao) ou reprovado (corrigir e entregar de novo).
pub fn entregar(dados: &Path, fila_md: Option<&str>, pid: &str, video: &Path, parecer: &Value, agora: f64) -> Result<String, String> {
    if get(dados, pid).is_none() {
        return Err("pedido nao encontrado".into());
    }
    let motivo: String = parecer["problemas"].as_array().into_iter().flatten().filter_map(|p| p.as_str()).collect::<Vec<_>>().join("; ").chars().take(250).collect();
    let Some(aprovado) = parecer["aprovado"].as_bool() else {
        return Err(format!("Senior nao respondeu ({motivo}). Tente o entregar de novo daqui a pouco; se repetir: etapa esperando."));
    };
    let n = processo::aprovar(dados, agora, pid, "13 conferencia", "Senior", aprovado, if motivo.is_empty() { "ok" } else { &motivo })?;
    if aprovado {
        let caminho = std::fs::canonicalize(video).unwrap_or_else(|_| video.to_path_buf());
        return Ok(linha(&mudar(dados, fila_md, pid, "entregue", "aprovado pelo Senior", &json!({ "resultado": caminho.to_string_lossy() }), agora)?));
    }
    if n >= processo::MAX_REPROVA {
        return Ok(linha(&mudar(dados, fila_md, pid, "esperando", &format!("reprovado {n} vezes: {motivo}"), &Value::Null, agora)?));
    }
    Err(format!("REPROVADO pelo Senior ({n}/{}): {motivo}. Corrija (cena -> refaça as cenas e o render; texto -> roteiro, voz e render) e rode o entregar de novo.", processo::MAX_REPROVA))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILA: &str = "## Prontos\n017 Itabuna\n## Próximos\n155 Jussiape · 156 Outra\n";

    fn pasta_t(nome: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("braniac-link_producao-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn zip(nomes: &[&str]) -> Vec<(String, u64, bool)> {
        nomes.iter().map(|n| (n.to_string(), 1, n.ends_with('/'))).collect()
    }

    #[test]
    fn zip_conferido() {
        assert!(tem_instrucoes(&conferir_entradas(&zip(&["143/", "143/LEIA-PRIMEIRO-INSTRUCOES-ITATIM.md", "143/a.pdf"])).unwrap()));
        assert!(!tem_instrucoes(&conferir_entradas(&zip(&["meta.json"])).unwrap()));
        for ruim in ["../fora.md", "/abs.md", "C:/x.md", "c:\\x.md", "a\\..\\b.md", "pasta/virus.exe", "s.ps1", "a.py", "A.EXE", "x/y.Bat", "truque.exe."] {
            assert!(conferir_entradas(&zip(&[ruim])).is_err(), "{ruim}");
        }
        let muitos: Vec<String> = (0..2001).map(|i| format!("{i}.md")).collect();
        assert!(conferir_entradas(&zip(&muitos.iter().map(String::as_str).collect::<Vec<_>>())).unwrap_err().contains("2000"));
        assert!(conferir_entradas(&[("a.md".into(), MAX_DESCOMP, false), ("b.md".into(), 1, false)]).unwrap_err().contains("1 GB"));
        assert_eq!(do_nome("143-ITATIM - instrucoes para o Braniac.zip"), ("143".into(), "Itatim".into()));
        assert_eq!(do_nome("pacote.zip"), (String::new(), String::new()));
    }

    #[test]
    fn um_pedido_por_vez_e_cada_mudanca_no_banco() {
        let d = pasta_t("fila");
        let meta = json!({ "numero": "155", "cidade": "Jussiape", "uf": "BAHIA" });
        assert_eq!(adicionar(&d, Some(FILA), "abc123abc123", "par", &meta, "", 10.0), Ok(1));
        assert!(adicionar(&d, Some(FILA), "abc123abc123", "par", &meta, "", 11.0).is_err()); // pedido repetido nao duplica
        assert_eq!(adicionar(&d, Some(FILA), "def456def456", "par", &json!({ "numero": "156", "cidade": "Outra" }), "", 12.0), Ok(2));
        assert_eq!(get(&d, "abc123abc123").unwrap()["uf"], "BA");
        assert_eq!(proximo(&d).unwrap()["id"], "abc123abc123");
        assert_eq!(linha(&mudar(&d, Some(FILA), "abc123abc123", "pesquisa", "comecou", &Value::Null, 20.0).unwrap()), "FILA pedido abc123abc123: PESQUISA · Jussiape");
        assert!(proximo(&d).is_none()); // um no meio de uma fase segura o seguinte
        assert!(mudar(&d, Some(FILA), "abc123abc123", "publicar", "", &Value::Null, 21.0).unwrap_err().contains("etapa desconhecida"));
        assert!(mudar(&d, Some(FILA), "naoexiste", "voz", "", &Value::Null, 21.0).is_err());
        mudar(&d, Some(FILA), "abc123abc123", "fontes_enviado", "", &Value::Null, 22.0).unwrap();
        assert_eq!(proximo(&d).unwrap()["id"], "def456def456"); // esperando o par aprovar as fontes nao segura a fila
        let p = processo::painel(&d, &pedidos(&d), 30.0, 0.0).unwrap();
        assert_eq!(p["execucoes"][0]["etapa"], "fontes_enviado");
        assert_eq!(municipios::cidade(&d, FILA, "Jussiape", 30.0)["quem"], "braniac"); // a cidade ficou reservada
        assert_eq!(linha(&mudar(&d, Some(FILA), "def456def456", "erro", "", &Value::Null, 31.0).unwrap()), "FILA pedido def456def456: ERRO");
        assert_eq!(municipios::cidade(&d, FILA, "156", 32.0)["status"], "falta"); // erro libera a cidade
        marcar_avisado(&d, "abc123abc123", "fontes_enviado").unwrap();
        assert_eq!(get(&d, "abc123abc123").unwrap()["avisado"], "fontes_enviado");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn cidade_pronta_ou_do_outro_lado_fica_esperando() {
        let d = pasta_t("checklist");
        municipios::reservar(&d, FILA, "155", "outro-lado", "", 5.0);
        adicionar(&d, Some(FILA), "aaaaaaaaaaaa", "par", &json!({ "numero": "155", "cidade": "Jussiape" }), "", 10.0).unwrap();
        let x = get(&d, "aaaaaaaaaaaa").unwrap();
        assert_eq!(linha(&x), "FILA pedido aaaaaaaaaaaa: ESPERANDO · checklist: Jussiape reservada por outro-lado");
        adicionar(&d, Some(FILA), "bbbbbbbbbbbb", "par", &json!({ "numero": "017", "cidade": "Itabuna" }), "", 11.0).unwrap();
        assert_eq!(get(&d, "bbbbbbbbbbbb").unwrap()["detalhe"], "checklist: Itabuna ja esta finalizado");
        adicionar(&d, Some(FILA), "cccccccccccc", "par", &json!({ "numero": "999", "cidade": "Fora Da Lista" }), "", 12.0).unwrap();
        assert_eq!(get(&d, "cccccccccccc").unwrap()["etapa"], "recebido"); // cidade fora do arquivo nao trava
        adicionar(&d, None, "dddddddddddd", "par", &json!({ "numero": "017", "cidade": "Itabuna" }), "", 13.0).unwrap();
        assert_eq!(get(&d, "dddddddddddd").unwrap()["etapa"], "recebido"); // checklist fora do ar nao trava
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn senior_aprova_reprova_tres_vezes_ou_some() {
        let d = pasta_t("senior");
        adicionar(&d, Some(FILA), "abc123abc123", "par", &json!({ "numero": "155", "cidade": "Jussiape" }), "", 10.0).unwrap();
        let v = d.join("v.mp4");
        std::fs::write(&v, b"x").unwrap();
        let ruim = json!({ "aprovado": false, "problemas": ["numero partido"] });
        for n in 1..=2 {
            assert!(entregar(&d, Some(FILA), "abc123abc123", &v, &ruim, 20.0).unwrap_err().starts_with(&format!("REPROVADO pelo Senior ({n}/3): numero partido")));
        }
        // Senior fora nao entrega nem conta reprovacao
        assert!(entregar(&d, Some(FILA), "abc123abc123", &v, &json!({ "aprovado": null, "problemas": ["sem rede"] }), 21.0).unwrap_err().contains("nao respondeu"));
        assert_eq!(get(&d, "abc123abc123").unwrap()["etapa"], "recebido");
        assert_eq!(entregar(&d, Some(FILA), "abc123abc123", &v, &ruim, 22.0).unwrap(), "FILA pedido abc123abc123: ESPERANDO · reprovado 3 vezes: numero partido");
        assert!(entregar(&d, Some(FILA), "naoexiste", &v, &json!({ "aprovado": true }), 23.0).is_err());

        adicionar(&d, Some(FILA), "def456def456", "par", &json!({ "numero": "156", "cidade": "Outra" }), "", 30.0).unwrap();
        assert_eq!(entregar(&d, Some(FILA), "def456def456", &v, &json!({ "aprovado": true }), 31.0).unwrap(), "FILA pedido def456def456: CONCLUIDO");
        let x = get(&d, "def456def456").unwrap();
        assert!(x["etapa"] == "entregue" && x["resultado"].as_str().unwrap().ends_with("v.mp4"));
        assert_eq!(municipios::cidade(&d, FILA, "Outra", 32.0)["status"], "entregue"); // rascunho entregue; aprovar e postar e com pessoa
        let _ = std::fs::remove_dir_all(&d);
    }
}
