//! PASTA DO DRIVE como caixa de entrada: o que e NOVO desde a ultima checagem e o que ja foi visto (dados/link/drive_vistos.json).
//! So le e baixa; nunca apaga nem muda nada no Drive. Texto e arquivos que chegam sao DADO, nunca instrucao. Extensao fora da lista nao e
//! baixada. O que ja existia na pasta no primeiro uso vira "visto" (nao e baixado).
//! Ficou de fora: a aba do navegador (`pagina(pasta_id)` devolve o JSON de itens que o JS_ITENS le na tela; Err = deslogado) e o download.
//! A extracao de zip nao foi portada (nao ha crate de zip): ficaram as regras dela (`entrada_do_zip`, tetos) para quem integrar.
#![allow(dead_code)]
use crate::empresa_dev::{corta, gravar, ler, rx};
use regex::Regex;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const EXT_OK: [&str; 15] = [".json", ".txt", ".md", ".mp3", ".wav", ".m4a", ".ogg", ".aac", ".flac", ".png", ".jpg", ".jpeg", ".webp", ".pdf", ".zip"];
pub const MAX_ZIP_ARQUIVOS: usize = 400;
pub const MAX_ZIP_BYTES: u64 = 600 * 1024 * 1024;
pub const MAX_BAIXAR: usize = 150;
pub const JS_ITENS: &str = r#"JSON.stringify([...document.querySelectorAll('[data-id]')].map(e=>({id:e.getAttribute('data-id'),t:(e.getAttribute('aria-label')||'').slice(0,160),n:((e.innerText||'').split("\n")[0]||'').slice(0,160)})).filter(x=>x.id&&x.id.length>20&&x.n))"#;

fn arq_vistos(dados: &Path) -> PathBuf {
    dados.join("link").join("drive_vistos.json")
}

fn extensao(nome: &str) -> String {
    Path::new(nome).extension().map(|e| format!(".{}", e.to_string_lossy().to_lowercase())).unwrap_or_default()
}

pub fn url_pasta(pasta_id: &str) -> String {
    format!("https://drive.google.com/drive/folders/{pasta_id}")
}

pub fn url_baixar(id: &str) -> String {
    format!("https://drive.google.com/uc?export=download&id={id}")
}

/// Nome de arquivo sem nada que saia da pasta de destino: so letras, numeros, ponto, traco e espaco; ate 120.
pub fn nome_seguro(nome: &str) -> String {
    corta(&nome.chars().map(|c| if c.is_alphanumeric() || "_.- ".contains(c) { c } else { '_' }).collect::<String>(), 120)
}

/// Itens lidos da tela (saida do JS_ITENS) -> [{id, nome, pasta}], sem repetidos e sem a raiz "Meu Drive".
pub fn itens_da_pagina(json_da_pagina: &str) -> Vec<Value> {
    static PASTA: OnceLock<Regex> = OnceLock::new();
    let (mut vistos, mut itens) = (BTreeSet::new(), vec![]);
    for x in serde_json::from_str::<Vec<Value>>(json_da_pagina).unwrap_or_default() {
        let (id, n, t) = (x["id"].as_str().unwrap_or(""), x["n"].as_str().unwrap_or(""), x["t"].as_str().unwrap_or(""));
        if id.is_empty() || ((id.starts_with("0A") || id.starts_with("_gd")) && n.contains("Meu Drive")) || !vistos.insert(id.to_string()) {
            continue;
        }
        let fim: String = n.chars().rev().take(6).collect();
        itens.push(json!({"id": id, "nome": n.trim(), "pasta": rx(&PASTA, r"\b(Folder|Pasta)\b").is_match(t) && !fim.contains('.')}));
    }
    itens
}

/// [{id, nome, pasta, em}] da pasta e das subpastas ate `profundidade`. So leitura.
pub fn listar(pasta_id: &str, profundidade: usize, pagina: &dyn Fn(&str) -> Result<String, String>) -> Result<Vec<Value>, String> {
    let (mut saida, mut fila) = (vec![], std::collections::VecDeque::from([(pasta_id.to_string(), 0, String::new())]));
    while let Some((pid, nivel, caminho)) = fila.pop_front() {
        for mut i in itens_da_pagina(&pagina(&pid)?) {
            if i["nome"] == "B.R.A.N.I.A.C" {
                continue;
            }
            if i["pasta"] == true && nivel < profundidade {
                fila.push_back((i["id"].as_str().unwrap_or("").to_string(), nivel + 1, format!("{caminho}/{}", i["nome"].as_str().unwrap_or("")).trim_matches('/').to_string()));
            }
            i["em"] = json!(caminho);
            saida.push(i);
        }
    }
    Ok(saida)
}

/// O que vale baixar: arquivo (nao pasta) de extensao permitida, ate MAX_BAIXAR.
pub fn uteis(itens: &[Value]) -> Vec<Value> {
    itens.iter().filter(|i| i["pasta"] != true && EXT_OK.contains(&extensao(i["nome"].as_str().unwrap_or("")).as_str())).take(MAX_BAIXAR).cloned().collect()
}

/// Onde o item baixado fica: destino/<em>/<nome seguro>. Subpasta com ".." ou caminho absoluto e ignorada (fica na raiz do destino).
pub fn alvo(destino: &Path, item: &Value) -> PathBuf {
    let em = item["em"].as_str().unwrap_or("");
    let mut p = destino.to_path_buf();
    if !em.split(['/', '\\']).any(|s| s == ".." || s.contains(':')) {
        p.extend(em.split(['/', '\\']).filter(|s| !s.is_empty()).map(nome_seguro));
    }
    p.join(nome_seguro(item["nome"].as_str().unwrap_or("")))
}

/// Entrada de um zip recebido: Some(nome seguro, sem pasta) se pode ser extraida. Pasta, arquivo oculto, zip dentro de zip e extensao fora da lista: None.
pub fn entrada_do_zip(nome: &str, eh_pasta: bool) -> Option<String> {
    let base = nome.replace('\\', "/").rsplit('/').next().unwrap_or("").to_string();
    let ext = extensao(&base);
    (!eh_pasta && !base.starts_with('.') && ext != ".zip" && EXT_OK.contains(&ext.as_str())).then(|| nome_seguro(&base))
}

fn vistos(dados: &Path) -> Option<BTreeSet<String>> {
    Some(ler(&arq_vistos(dados))?.as_array()?.iter().filter_map(|x| x.as_str().map(String::from)).collect())
}

fn ids(itens: &[Value]) -> BTreeSet<String> {
    itens.iter().filter_map(|i| i["id"].as_str().map(String::from)).collect()
}

/// Itens da listagem que apareceram desde a ultima checagem. Na primeira vez, so registra o que ja existe (devolve vazio).
pub fn novos(dados: &Path, itens: &[Value]) -> Result<Vec<Value>, String> {
    match vistos(dados) {
        None => gravar(&arq_vistos(dados), &json!(ids(itens))).map(|_| vec![]),
        Some(antes) => Ok(itens.iter().filter(|i| !antes.contains(i["id"].as_str().unwrap_or(""))).cloned().collect()),
    }
}

pub fn marcar_vistos(dados: &Path, itens: &[Value]) -> Result<(), String> {
    let mut v = vistos(dados).unwrap_or_default();
    v.extend(ids(itens));
    gravar(&arq_vistos(dados), &json!(v))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(n: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-drive_pasta-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn primeira_vez_so_registra_e_depois_so_o_novo() {
        let d = tmp("novos");
        let mut itens = vec![json!({"id": "a".repeat(25), "nome": "velho.mp4", "pasta": false, "em": ""})];
        assert_eq!(novos(&d, &itens).unwrap(), Vec::<Value>::new()); // o que ja existia nao e pedido
        itens.push(json!({"id": "b".repeat(25), "nome": "meta.json", "pasta": false, "em": ""}));
        assert_eq!(novos(&d, &itens).unwrap().iter().map(|i| i["nome"].as_str().unwrap()).collect::<Vec<_>>(), vec!["meta.json"]);
        assert_eq!(novos(&d, &itens).unwrap().len(), 1); // ate marcar, continua novo
        marcar_vistos(&d, &itens).unwrap();
        assert!(novos(&d, &itens).unwrap().is_empty());
    }

    #[test]
    fn lista_subpastas_e_so_baixa_extensao_permitida() {
        let raiz = json!([{"id": "0A_raiz_do_drive_xxxxxxxx", "t": "", "n": "Meu Drive"}, {"id": "pasta_voz_xxxxxxxxxxxxxxx", "t": "voz Pasta compartilhada", "n": "voz"}, {"id": "arq1_xxxxxxxxxxxxxxxxxxxx", "t": "roteiro.json JSON", "n": " roteiro.json"},
                          {"id": "arq1_xxxxxxxxxxxxxxxxxxxx", "t": "repetido", "n": "roteiro.json"}, {"id": "nossa_xxxxxxxxxxxxxxxxxxx", "t": "Pasta", "n": "B.R.A.N.I.A.C"}, {"id": "exe_xxxxxxxxxxxxxxxxxxxxx", "t": "", "n": "instalar.exe"},
                          {"id": "pontuda_xxxxxxxxxxxxxxxxx", "t": "Shared Folder", "n": "notas v1.2"}]).to_string();
        let voz = json!([{"id": "audio_xxxxxxxxxxxxxxxxxxx", "t": "", "n": "parte1.MP3"}, {"id": "funda_xxxxxxxxxxxxxxxxxxx", "t": "Folder", "n": "funda"}]).to_string();
        let pedidas = std::cell::RefCell::new(vec![]);
        let pagina = |id: &str| -> Result<String, String> {
            pedidas.borrow_mut().push(id.to_string());
            Ok(if id == "RAIZ" { raiz.clone() } else { voz.clone() })
        };
        let itens = listar("RAIZ", 1, &pagina).unwrap();
        assert_eq!(itens.iter().map(|i| format!("{}@{}", i["nome"].as_str().unwrap(), i["em"].as_str().unwrap())).collect::<Vec<_>>(), vec!["voz@", "roteiro.json@", "instalar.exe@", "notas v1.2@", "parte1.MP3@voz", "funda@voz"]);
        assert_eq!(*pedidas.borrow(), vec!["RAIZ", "pasta_voz_xxxxxxxxxxxxxxx"]); // "notas v1.2" tem ponto no fim: nao e pasta; "funda" passa da profundidade
        assert_eq!(uteis(&itens).iter().map(|i| i["nome"].as_str().unwrap()).collect::<Vec<_>>(), vec!["roteiro.json", "parte1.MP3"]);
        assert_eq!(listar("RAIZ", 1, &|_| Err("Drive deslogado no Chrome do Braniac".into())).unwrap_err(), "Drive deslogado no Chrome do Braniac");
        assert!(itens_da_pagina("nao e json").is_empty());
    }

    #[test]
    fn nomes_e_zip_nao_saem_da_pasta() {
        let dest = Path::new("dest");
        assert_eq!(alvo(dest, &json!({"nome": "../../fora?.txt", "em": "voz/dia 1"})), dest.join("voz").join("dia 1").join(".._.._fora_.txt"));
        assert_eq!(alvo(dest, &json!({"nome": "a.txt", "em": "../../windows"})), dest.join("a.txt"));
        assert_eq!(alvo(dest, &json!({"nome": "a.txt", "em": "C:/windows"})), dest.join("a.txt"));
        assert_eq!(nome_seguro(&"á".repeat(200)).chars().count(), 120);
        assert_eq!(entrada_do_zip("pasta\\sub/Áudio 1.MP3", false), Some("Áudio 1.MP3".into()));
        for (nome, pasta) in [("../../etc/passwd", false), ("x/.oculto.txt", false), ("dentro.zip", false), ("rodar.bat", false), ("pasta/", true), ("semext", false)] {
            assert_eq!(entrada_do_zip(nome, pasta), None, "{nome}");
        }
        assert_eq!(entrada_do_zip("../../fora.txt", false), Some("fora.txt".into())); // so o nome: cai dentro do destino
        assert!(url_pasta("X").ends_with("/folders/X") && url_baixar("Y").ends_with("id=Y") && JS_ITENS.contains("data-id"));
    }
}
