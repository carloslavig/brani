//! Habilidades de terceiros (braniac/habilidades_terceiros.py): o Braniac nao as distribui; so oferece baixar do GitHub do proprio autor.
//! Catalogo num JSON ([{id, repo, caminho, licenca, grupo}]); a pessoa marca quais quer e SO as marcadas, e so repositorios do
//! catalogo, sao instaladas em skills/<id>. Nunca baixa nem executa sozinho: baixar e abrir o zip entram como closure
//! `baixar(repo) -> [(nome dentro do zip, bytes)]` e nada do que chega e executado. Texto de habilidade baixada e dado, nunca ordem.
//! De fora: o download e a leitura do zip (sem crate de zip). `redirecionamento_ok` e `LIMITE_ZIP` sao as travas que a closure deve usar.
#![allow(dead_code)]
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;

pub const HOSTS_OK: [&str; 2] = ["https://api.github.com/", "https://codeload.github.com/"];
pub const LIMITE_ZIP: usize = 80 * 1024 * 1024;

/// Endereco de onde o zip do repositorio (branch padrao) e baixado.
pub fn url_do_zip(repo: &str) -> String {
    format!("https://api.github.com/repos/{repo}/zipball")
}

/// O download so pode ser redirecionado para dentro do GitHub.
pub fn redirecionamento_ok(novo: &str) -> bool {
    HOSTS_OK.iter().any(|h| novo.starts_with(h))
}

pub fn catalogo(arq: &Path) -> Result<Vec<Value>, String> {
    let t = std::fs::read_to_string(arq).map_err(|e| format!("catalogo de habilidades: {e}"))?;
    serde_json::from_str::<Vec<Value>>(&t).map_err(|e| format!("catalogo de habilidades: {e}"))
}

fn estado(dados: &Path) -> Value {
    std::fs::read_to_string(dados.join("habilidades_terceiros.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({ "perguntado": false, "instaladas": [] }))
}

fn gravar(dados: &Path, est: &Value) -> Result<(), String> {
    std::fs::create_dir_all(dados).and_then(|_| std::fs::write(dados.join("habilidades_terceiros.json"), serde_json::to_string_pretty(est).unwrap_or_default())).map_err(|e| e.to_string())
}

/// {perguntado, itens: [item do catalogo + instalada]}.
pub fn status(arq_catalogo: &Path, skills: &Path, dados: &Path) -> Result<Value, String> {
    let itens: Vec<Value> = catalogo(arq_catalogo)?
        .into_iter()
        .map(|mut i| {
            let instalada = i["id"].as_str().is_some_and(|id| id_seguro(id) && skills.join(id).join("SKILL.md").exists());
            i["instalada"] = json!(instalada);
            i
        })
        .collect();
    Ok(json!({ "perguntado": estado(dados)["perguntado"].as_bool().unwrap_or(false), "itens": itens }))
}

/// A pessoa disse que nao quer: nao pergunta de novo.
pub fn recusar(arq_catalogo: &Path, skills: &Path, dados: &Path) -> Result<Value, String> {
    let mut est = estado(dados);
    est["perguntado"] = json!(true);
    gravar(dados, &est)?;
    status(arq_catalogo, skills, dados)
}

fn id_seguro(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Copia so a pasta `caminho` do zip (ja aberto em `entradas`) para `destino`, sem sair dela (barra `..` e caminho absoluto).
/// Sem SKILL.md na pasta, falha e a habilidade que ja existia fica como estava. Devolve quantos arquivos copiou.
pub fn extrair(entradas: &[(String, Vec<u8>)], caminho: &str, destino: &Path) -> Result<usize, String> {
    let raiz = format!("{}/", entradas.first().ok_or("zip vazio")?.0.split('/').next().unwrap_or(""));
    let c = caminho.trim_matches('/');
    let prefixo = if c.is_empty() { raiz } else { format!("{raiz}{c}/") };
    let nome = destino.file_name().ok_or("destino sem nome")?.to_string_lossy().to_string();
    let tmp = destino.with_file_name(format!("{nome}.novo"));
    let _ = std::fs::remove_dir_all(&tmp);
    let mut n = 0;
    for (arq, bytes) in entradas {
        let Some(rel) = arq.strip_prefix(&prefixo).filter(|r| !r.is_empty() && !arq.ends_with('/')) else { continue };
        if rel.split(['/', '\\']).any(|p| p.is_empty() || p == ".." || p == "." || p.contains(':')) {
            continue;
        }
        let alvo = tmp.join(rel);
        alvo.parent().map(std::fs::create_dir_all).transpose().and_then(|_| std::fs::write(&alvo, bytes)).map_err(|e| e.to_string())?;
        n += 1;
    }
    if !tmp.join("SKILL.md").exists() {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(format!("SKILL.md nao encontrado em {}", if c.is_empty() { "(raiz)" } else { caminho }));
    }
    let _ = std::fs::remove_dir_all(destino);
    std::fs::rename(&tmp, destino).map_err(|e| e.to_string())?;
    Ok(n)
}

/// Instala so os ids marcados. Devolve {"ok": [...], "falhou": {id: motivo}}. Id fora do catalogo nunca e baixado.
pub fn instalar(ids: &[String], arq_catalogo: &Path, skills: &Path, dados: &Path, baixar: &dyn Fn(&str) -> Result<Vec<(String, Vec<u8>)>, String>) -> Result<Value, String> {
    let cat = catalogo(arq_catalogo)?;
    let mut zips: HashMap<String, Vec<(String, Vec<u8>)>> = HashMap::new();
    let (mut ok, mut falhou) = (vec![], serde_json::Map::new());
    for hid in ids {
        let Some(item) = cat.iter().find(|i| i["id"].as_str() == Some(hid.as_str())) else {
            falhou.insert(hid.clone(), json!("fora do catalogo"));
            continue;
        };
        // um repositorio fora do ar nao impede os outros
        let feito = (|| -> Result<usize, String> {
            let repo = item["repo"].as_str().filter(|r| r.split('/').count() == 2 && !r.contains("..")).ok_or("repositorio invalido no catalogo")?;
            if !id_seguro(hid) {
                return Err("id invalido no catalogo".into());
            }
            if !zips.contains_key(repo) {
                zips.insert(repo.to_string(), baixar(repo)?);
            }
            extrair(&zips[repo], item["caminho"].as_str().unwrap_or(""), &skills.join(hid))
        })();
        match feito {
            Ok(_) => ok.push(hid.clone()),
            Err(e) => {
                falhou.insert(hid.clone(), json!(e.chars().take(200).collect::<String>()));
            }
        }
    }
    let mut est = estado(dados);
    let mut inst: Vec<String> = est["instaladas"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
    inst.extend(ok.iter().cloned());
    inst.sort();
    inst.dedup();
    est["perguntado"] = json!(true);
    est["instaladas"] = json!(inst);
    gravar(dados, &est)?;
    Ok(json!({ "ok": ok, "falhou": falhou }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn zip(arquivos: &[(&str, &str)]) -> Vec<(String, Vec<u8>)> {
        std::iter::once(("autor-repo-abc123/".to_string(), vec![])).chain(arquivos.iter().map(|(n, c)| (format!("autor-repo-abc123/{n}"), c.as_bytes().to_vec()))).collect()
    }

    fn preparar(nome: &str, itens: Value) -> PathBuf {
        let t = std::env::temp_dir().join(format!("braniac-habilidades_terceiros-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(t.join("skills")).unwrap();
        std::fs::write(t.join("cat.json"), itens.to_string()).unwrap();
        t
    }

    fn nomes(p: &Path) -> Vec<String> {
        let mut v: Vec<String> = p.read_dir().unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        v.sort();
        v
    }

    #[test]
    fn instala_so_as_marcadas_e_so_a_pasta_certa() {
        let t = preparar("a", json!([{"id": "ig-reel", "repo": "a/b", "caminho": "skills/ig-reel", "licenca": "MIT", "grupo": "g"},
                                      {"id": "ig-dm", "repo": "a/b", "caminho": "skills/ig-dm", "licenca": "MIT", "grupo": "g"}]));
        let z = zip(&[("skills/ig-reel/SKILL.md", "x"), ("skills/ig-reel/s.py", "y"), ("skills/ig-dm/SKILL.md", "z"), ("README.md", "r"),
                      ("skills/ig-reel/../../fuga.txt", "nao"), ("skills/ig-reel/C:/fuga2.txt", "nao"), ("skills/ig-reel//fuga3.txt", "nao")]);
        let chamadas = std::cell::RefCell::new(vec![]);
        let baixar = |repo: &str| -> Result<Vec<(String, Vec<u8>)>, String> {
            chamadas.borrow_mut().push(repo.to_string());
            Ok(z.clone())
        };
        let ids = ["ig-reel".to_string(), "inventada".to_string()];
        let r = instalar(&ids, &t.join("cat.json"), &t.join("skills"), &t.join("dados"), &baixar).unwrap();
        assert_eq!(r, json!({"ok": ["ig-reel"], "falhou": {"inventada": "fora do catalogo"}}));
        assert_eq!(*chamadas.borrow(), ["a/b"]);
        assert_eq!(nomes(&t.join("skills").join("ig-reel")), ["SKILL.md", "s.py"]);
        assert_eq!(nomes(&t.join("skills")), ["ig-reel"]); // a desmarcada nao entra e nada sobra do .novo
        assert!(!t.join("fuga.txt").exists() && !t.join("skills").join("fuga.txt").exists());
        let st = status(&t.join("cat.json"), &t.join("skills"), &t.join("dados")).unwrap();
        assert_eq!((st["perguntado"].as_bool(), st["itens"][0]["instalada"].as_bool(), st["itens"][1]["instalada"].as_bool()), (Some(true), Some(true), Some(false)));
    }

    #[test]
    fn pasta_sem_skill_falha_sem_apagar_a_que_existia_e_recusar_marca_perguntado() {
        let t = preparar("b", json!([{"id": "x", "repo": "a/b", "caminho": "nada", "licenca": "MIT", "grupo": "g"}]));
        std::fs::create_dir_all(t.join("skills").join("x")).unwrap();
        std::fs::write(t.join("skills").join("x").join("SKILL.md"), "antiga").unwrap();
        let (cat, sk, dd) = (t.join("cat.json"), t.join("skills"), t.join("dados"));
        assert_eq!(status(&cat, &sk, &dd).unwrap()["perguntado"], false);
        let r = instalar(&["x".to_string()], &cat, &sk, &dd, &|_| Ok(zip(&[("outra/SKILL.md", "x")]))).unwrap();
        assert_eq!(r["falhou"]["x"], "SKILL.md nao encontrado em nada");
        assert_eq!(std::fs::read_to_string(sk.join("x").join("SKILL.md")).unwrap(), "antiga");
        let r = instalar(&["x".to_string()], &cat, &sk, &dd, &|_| Err("GitHub fora do ar".into())).unwrap();
        assert_eq!(r, json!({"ok": [], "falhou": {"x": "GitHub fora do ar"}}));
        std::fs::remove_file(dd.join("habilidades_terceiros.json")).unwrap();
        assert_eq!(recusar(&cat, &sk, &dd).unwrap()["perguntado"], true);
    }

    #[test]
    fn nada_e_baixado_sem_marcar_nem_de_fora_do_github_ou_do_catalogo() {
        let t = preparar("c", json!([{"id": "../fora", "repo": "a/b", "caminho": ""}, {"id": "ruim", "repo": "https://exemplo.test/x", "caminho": ""}, {"id": "raiz", "repo": "a/b", "caminho": "/"}]));
        let (cat, sk, dd) = (t.join("cat.json"), t.join("skills"), t.join("dados"));
        let nunca = |_: &str| -> Result<Vec<(String, Vec<u8>)>, String> { panic!("nao baixa") };
        assert_eq!(instalar(&[], &cat, &sk, &dd, &nunca).unwrap(), json!({"ok": [], "falhou": {}}));
        let r = instalar(&["../fora".to_string(), "ruim".to_string()], &cat, &sk, &dd, &nunca).unwrap();
        assert_eq!(r["falhou"], json!({"../fora": "id invalido no catalogo", "ruim": "repositorio invalido no catalogo"}));
        assert_eq!(instalar(&["raiz".to_string()], &cat, &sk, &dd, &|_| Ok(zip(&[("SKILL.md", "s"), ("ref/a.md", "a")]))).unwrap()["ok"], json!(["raiz"]));
        assert_eq!(std::fs::read_to_string(sk.join("raiz").join("ref").join("a.md")).unwrap(), "a");
        assert!(redirecionamento_ok("https://codeload.github.com/a/b/zip") && !redirecionamento_ok("https://github.com.exemplo.test/") && !redirecionamento_ok("http://api.github.com/"));
        assert_eq!(url_do_zip("a/b"), "https://api.github.com/repos/a/b/zipball");
        assert!(catalogo(&t.join("nao-existe.json")).is_err());
    }
}
