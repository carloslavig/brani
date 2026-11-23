//! O Jarvis PROGRAMA sozinho com as IAs gratis (sem agente de programacao): o implementador reserva da automelhoria.
//! Quem confere tudo depois e a automelhoria (testes, travas, protegidos, desfazer); aqui so se limita ONDE a IA pode mexer: dentro da
//! pasta do codigo, nunca na pasta de dados, em `.git`, em `.env` nem (para escrever) em arquivo protegido. Teto de 40 passos.
//! Ficou de fora (closures): a IA com ferramentas e rodar os testes.
//! Diferencas do Python: a busca olha .py e .rs; caminho com `..`, absoluto ou com letra de unidade e recusado direto (la, pelo resolve).
#![allow(dead_code)]
use crate::automelhoria::PROTEGIDOS;
use crate::empresa_dev::{cauda, corta};
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};

pub const PASSOS: usize = 40;
const FECHADAS: [&str; 3] = ["data", "dados", ".git"];

/// Caminho dentro da pasta do codigo, ou erro. Leitura: sem pasta de dados, `.git` e `.env`. Escrita: tambem sem arquivo protegido.
pub fn caminho(raiz: &Path, rel: &str, escrita: bool) -> Result<PathBuf, String> {
    let rel = rel.replace('\\', "/");
    let rel = rel.strip_prefix("braniac/").unwrap_or(&rel).trim_start_matches('/');
    let p = Path::new(rel);
    if rel.is_empty() || !p.components().all(|c| matches!(c, Component::Normal(_))) {
        return Err("caminho fora da pasta do código".into());
    }
    let nome = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    if FECHADAS.contains(&rel.split('/').next().unwrap_or("").to_lowercase().as_str()) || nome.starts_with(".env") {
        return Err("pasta/arquivo proibido".into());
    }
    if escrita && (PROTEGIDOS.contains(&rel) || nome.ends_with(".env")) {
        return Err(format!("{rel} e protegido"));
    }
    let cheio = raiz.join(p);
    // atalho (link) que aponta para fora nao passa: confere a pasta de verdade
    let real = |x: &Path| x.ancestors().find_map(|a| a.canonicalize().ok());
    match (real(&cheio), raiz.canonicalize().ok()) {
        (Some(c), Some(r)) if c.starts_with(&r) => Ok(cheio),
        _ => Err("caminho fora da pasta do código".into()),
    }
}

fn arquivos(pasta: &Path, out: &mut Vec<PathBuf>) {
    let mut itens: Vec<PathBuf> = std::fs::read_dir(pasta).into_iter().flatten().flatten().map(|e| e.path()).collect();
    itens.sort();
    for p in itens {
        let nome = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        if p.is_dir() {
            if !FECHADAS.contains(&nome.as_str()) && nome != "target" {
                arquivos(&p, out);
            }
        } else if nome.ends_with(".py") || nome.ends_with(".rs") {
            out.push(p);
        }
    }
}

fn txt<'a>(a: &'a Value, k: &str) -> Result<&'a str, String> {
    a[k].as_str().ok_or_else(|| format!("falta o argumento {k}"))
}

/// Uma ferramenta da IA. Err = pedido recusado (caminho proibido, argumento faltando, arquivo ilegivel).
pub fn executor(raiz: &Path, nome: &str, a: &Value, rodar_testes: &dyn Fn(Option<&Path>) -> String) -> Result<String, String> {
    let e = |x: std::io::Error| x.to_string();
    match nome {
        "ler_arquivo" => {
            let t = std::fs::read_to_string(caminho(raiz, txt(a, "caminho")?, false)?).map_err(e)?;
            let (i, f) = (a["inicio"].as_u64().unwrap_or(1).max(1) as usize, a["fim"].as_u64().unwrap_or(200) as usize);
            let l: Vec<String> = t.lines().enumerate().skip(i - 1).take((f + 1).saturating_sub(i)).map(|(n, l)| format!("{}: {l}", n + 1)).collect();
            Ok(if l.is_empty() { "(vazio)".into() } else { l.join("\n") })
        }
        "buscar_codigo" => {
            let alvo = txt(a, "texto")?.to_lowercase();
            let (mut arqs, mut achou) = (Vec::new(), Vec::new());
            arquivos(raiz, &mut arqs);
            for p in arqs {
                let rel = p.strip_prefix(raiz).unwrap_or(&p).to_string_lossy().replace('\\', "/");
                for (n, l) in String::from_utf8_lossy(&std::fs::read(&p).unwrap_or_default()).lines().enumerate() {
                    if l.to_lowercase().contains(&alvo) && achou.len() < 40 {
                        achou.push(format!("{rel}:{}: {}", n + 1, corta(l.trim(), 140)));
                    }
                }
            }
            Ok(if achou.is_empty() { "nada encontrado".into() } else { achou.join("\n") })
        }
        "substituir_trecho" => {
            let p = caminho(raiz, txt(a, "caminho")?, true)?;
            let (t, antigo) = (std::fs::read_to_string(&p).map_err(e)?, txt(a, "antigo")?);
            let n = if antigo.is_empty() { 0 } else { t.matches(antigo).count() };
            if n != 1 {
                return Ok(format!("ERRO: o trecho antigo aparece {n} vez(es); preciso de exatamente 1 (inclua mais linhas ao redor)"));
            }
            std::fs::write(&p, t.replace(antigo, txt(a, "novo")?)).map_err(e)?;
            Ok("ok".into())
        }
        "criar_arquivo" => {
            let p = caminho(raiz, txt(a, "caminho")?, true)?;
            if let Some(d) = p.parent() {
                std::fs::create_dir_all(d).map_err(e)?;
            }
            std::fs::write(&p, txt(a, "conteudo")?).map_err(e)?;
            Ok("ok".into())
        }
        "rodar_testes" => {
            let alvo = a["arquivo"].as_str().filter(|x| !x.is_empty()).map(|x| caminho(raiz, x, false)).transpose()?;
            Ok(cauda(&rodar_testes(alvo.as_deref()), 1800))
        }
        _ => Ok(format!("ferramenta desconhecida: {nome}")),
    }
}

pub fn ferramentas() -> Value {
    let s = json!({ "type": "string" });
    let f = |nome: &str, desc: &str, props: Value, req: &[&str]| json!({ "type": "function", "function": { "name": nome, "description": desc, "parameters": { "type": "object", "properties": props, "required": req } } });
    json!([
        f("ler_arquivo", "Le um arquivo do codigo com numero de linha (padrao linhas 1-200).", json!({ "caminho": s, "inicio": { "type": "integer" }, "fim": { "type": "integer" } }), &["caminho"]),
        f("buscar_codigo", "Procura um texto nos arquivos de codigo (arquivo:linha).", json!({ "texto": s }), &["texto"]),
        f("substituir_trecho", "Troca um trecho EXATO (aparece 1 vez) por outro num arquivo. Prefira isto a reescrever o arquivo.", json!({ "caminho": s, "antigo": s, "novo": s }), &["caminho", "antigo", "novo"]),
        f("criar_arquivo", "Cria um arquivo novo (ex.: um teste).", json!({ "caminho": s, "conteudo": s }), &["caminho", "conteudo"]),
        f("rodar_testes", "Roda os testes (um arquivo ou todos se omitir).", json!({ "arquivo": s }), &[]),
    ])
}

/// Mesma assinatura do implementador da automelhoria: (codigo, texto). `agente(prompt, ferramentas, max_passos)` = a IA gratis com ferramentas.
pub fn rodar(prompt: &str, agente: &dyn Fn(&str, &Value, usize) -> Result<String, String>) -> (i32, String) {
    let p = format!("{prompt}\nVoce trabalha com FERRAMENTAS (ler_arquivo, buscar_codigo, substituir_trecho, criar_arquivo, rodar_testes): leia o codigo antes de mudar, \
        mude pouco, escreva um teste e rode-o. Quando terminar, responda em ate 6 linhas o que mudou.");
    match agente(&p, &ferramentas(), PASSOS) {
        Ok(t) if !t.is_empty() && !corta(&t, 20).contains("Não consegui") => (0, t),
        Ok(t) => (1, if t.is_empty() { "sem resposta".into() } else { t }),
        Err(e) => (1, format!("erro: {}", corta(&e, 200))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raiz(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-dev_livre-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("data")).unwrap();
        std::fs::create_dir_all(d.join("tests")).unwrap();
        std::fs::write(d.join("a.py"), "x = 1\ny = 1\nx = 1\n").unwrap();
        std::fs::write(d.join("data/segredo.py"), "y = 1 # dado pessoal\n").unwrap();
        d
    }

    fn sem_testes(_: Option<&Path>) -> String {
        "5 passed".into()
    }

    #[test]
    fn so_mexe_dentro_da_pasta_do_codigo() {
        let r = raiz("limites");
        for ruim in ["../fora.py", "data/jarvis.db", "dados/x.json", ".env", ".env.local", "braniac/data/x", ".git/config", "/etc/passwd", "C:/Windows/x", "a/../../fora", "", "DATA/x"] {
            assert!(executor(&r, "ler_arquivo", &json!({ "caminho": ruim }), &sem_testes).is_err(), "{ruim}");
        }
        for protegido in ["automelhoria.py", "tests/conftest.py", "src/automelhoria.rs", "src/dev_livre.rs", "config/prod.env", "tests\\conftest.py"] {
            assert!(executor(&r, "criar_arquivo", &json!({ "caminho": protegido, "conteudo": "x" }), &sem_testes).is_err(), "{protegido}");
            assert!(executor(&r, "substituir_trecho", &json!({ "caminho": protegido, "antigo": "a", "novo": "b" }), &sem_testes).is_err());
        }
        assert!(executor(&r, "rodar_testes", &json!({ "arquivo": "../x" }), &sem_testes).is_err());
        assert!(executor(&r, "ler_arquivo", &json!({}), &sem_testes).is_err());
        assert!(!r.join("automelhoria.py").exists());
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn le_busca_substitui_trecho_unico_e_cria() {
        let r = raiz("uso");
        let x = |n: &str, a: Value| executor(&r, n, &a, &sem_testes).unwrap();
        assert_eq!(x("ler_arquivo", json!({ "caminho": "braniac/a.py", "inicio": 2, "fim": 3 })), "2: y = 1\n3: x = 1");
        assert_eq!(x("ler_arquivo", json!({ "caminho": "a.py", "inicio": 9 })), "(vazio)");
        assert!(x("substituir_trecho", json!({ "caminho": "a.py", "antigo": "x = 1", "novo": "x = 2" })).contains("2 vez"));
        assert!(x("substituir_trecho", json!({ "caminho": "a.py", "antigo": "", "novo": "x" })).contains("0 vez"));
        assert_eq!(x("substituir_trecho", json!({ "caminho": "a.py", "antigo": "y = 1", "novo": "y = 2" })), "ok");
        assert!(std::fs::read_to_string(r.join("a.py")).unwrap().contains("y = 2"));
        assert_eq!(x("buscar_codigo", json!({ "texto": "Y = 2" })), "a.py:2: y = 2");
        assert_eq!(x("buscar_codigo", json!({ "texto": "y = 1" })), "nada encontrado"); // a pasta de dados nunca entra na busca
        assert_eq!(x("criar_arquivo", json!({ "caminho": "tests/test_novo.py", "conteudo": "def test_x():\n    assert 1\n" })), "ok");
        assert!(r.join("tests/test_novo.py").exists());
        assert_eq!(x("rodar_testes", json!({ "arquivo": "tests/test_novo.py" })), "5 passed");
        assert!(x("apagar_tudo", json!({})).contains("ferramenta desconhecida"));
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn rodar_devolve_codigo_como_o_implementador() {
        let visto = std::cell::Cell::new(0);
        let ok = rodar("pedido", &|p, f, passos| { visto.set(f.as_array().unwrap().len() * 100 + passos); assert!(p.starts_with("pedido\n")); Ok("mudei a.py".into()) });
        assert!(ok == (0, "mudei a.py".to_string()) && visto.get() == 540);
        assert_eq!(rodar("p", &|_, _, _| Ok("Não consegui fechar uma resposta".into())).0, 1);
        assert_eq!(rodar("p", &|_, _, _| Ok(String::new())), (1, "sem resposta".to_string()));
        assert_eq!(rodar("p", &|_, _, _| Err("sem cota".into())), (1, "erro: sem cota".to_string()));
    }
}
