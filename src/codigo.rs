//! Arquivos que os agentes programaram, para a tela de codigo. Nunca le fora da raiz.
//! Escrito pelo Leo (NexoDev, T055), revisado e ajustado pelo Claude.
use std::fs::{self, File, Metadata};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

/// Tamanho máximo permitido para um arquivo (512 KB).
const MAX_FILE_SIZE: u64 = 512 * 1024;
/// Máximo de itens retornados na listagem.
const MAX_ITEMS: usize = 2000;

/// Verifica se o diretório deve ser ignorado.
fn ignored_dir(entry: &Path) -> bool {
    if let Some(name) = entry.file_name().and_then(|n| n.to_str()) {
        matches!(name, "target" | ".git" | "node_modules")
    } else {
        false
    }
}

/// Converte `SystemTime` em segundos desde UNIX_EPOCH (ou 0 em caso de erro).
fn sys_time_to_secs(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Lista recursivamente arquivos de texto a partir de `raiz`.
/// Retorna um `serde_json::Value` contendo um array de objetos:
/// `{ "caminho": "...", "bytes": ..., "modificado": ... }`
pub fn listar(raiz: &Path) -> Value {
    let mut items: Vec<(PathBuf, Metadata)> = Vec::new();
    let mut stack: Vec<PathBuf> = vec![raiz.to_path_buf()];

    while let Some(dir) = stack.pop() {
        // Se o diretório for um dos ignorados, pula.
        if ignored_dir(&dir) {
            continue;
        }

        let read_dir = match fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(_) => continue,
        };

        for entry in read_dir.filter_map(Result::ok) {
            let path = entry.path();
            // Ignora diretórios especiais
            if path.is_dir() {
                if ignored_dir(&path) {
                    continue;
                }
                stack.push(path);
                continue;
            }

            // Só arquivos regulares
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            if !meta.is_file() {
                continue;
            }

            // Tamanho máximo
            if meta.len() > MAX_FILE_SIZE {
                continue;
            }

            items.push((path, meta));

            if items.len() >= MAX_ITEMS {
                break;
            }
        }

        if items.len() >= MAX_ITEMS {
            break;
        }
    }

    // Ordena pelo timestamp de modificação (mais recente primeiro)
    items.sort_by_key(|(_, meta)| {
        meta.modified()
            .map(|t| std::cmp::Reverse(sys_time_to_secs(t)))
            .unwrap_or(std::cmp::Reverse(0))
    });

    let json_array: Vec<Value> = items
        .into_iter()
        .map(|(path, meta)| {
            let rel_path = path
                .strip_prefix(raiz)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/"); // garante barra '/' mesmo no Windows

            json!({
                "caminho": rel_path,
                "bytes": meta.len(),
                "modificado": meta.modified()
                    .map(sys_time_to_secs)
                    .unwrap_or(0)
            })
        })
        .collect();

    json!(json_array)
}

/// Lê um arquivo dentro da árvore `raiz` a partir do caminho relativo `rel`.
/// Retorna `{ "caminho": "...", "texto": "...", "modificado": ... }` ou erro em `String`.
pub fn ler(raiz: &Path, rel: &str) -> Result<Value, String> {
    // 1. Verificações básicas de caminho
    let rel_path = Path::new(rel);
    if rel_path.is_absolute() {
        return Err("Caminho absoluto não permitido".into());
    }
    if rel_path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err("Uso de '..' não permitido".into());
    }

    // 2. Caminho completo e canonicalização
    let full_path = raiz.join(rel_path);
    let full_canonical = match full_path.canonicalize() {
        Ok(p) => p,
        Err(e) => return Err(format!("Falha ao canonicalizar caminho: {}", e)),
    };
    let raiz_canonical = match raiz.canonicalize() {
        Ok(p) => p,
        Err(e) => return Err(format!("Falha ao canonicalizar raiz: {}", e)),
    };

    if !full_canonical.starts_with(&raiz_canonical) {
        return Err("Caminho escapa da raiz".into());
    }

    // 3. Leitura do arquivo
    let mut file = File::open(&full_canonical)
        .map_err(|e| format!("Erro ao abrir arquivo: {}", e))?;

    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)
        .map_err(|e| format!("Erro ao ler arquivo: {}", e))?;

    // 4. Verifica UTF‑8
    let texto = String::from_utf8(buffer)
        .map_err(|_| "Arquivo não está em UTF-8".to_string())?;

    // 5. Metadados
    let meta = full_canonical
        .metadata()
        .map_err(|e| format!("Erro ao obter metadata: {}", e))?;
    let modificado = meta
        .modified()
        .map(sys_time_to_secs)
        .unwrap_or(0);

    Ok(json!({
        "caminho": rel.replace('\\', "/"),
        "texto": texto,
        "modificado": modificado
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use std::io::Write;
    use std::path::PathBuf;
    use std::time::Duration;

    fn temp_dir() -> PathBuf {
        let mut base = env::temp_dir();
        // cria um nome único usando timestamp
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0))
            .as_millis();
        base.push(format!("braniac_test_{}_{:?}", ts, std::thread::current().id()));
        fs::create_dir_all(&base).unwrap();
        base
    }

    #[test]
    fn test_listar_e_limitar() {
        let dir = temp_dir();

        // cria alguns arquivos
        for i in 0..5 {
            let sub = dir.join(format!("sub{}", i));
            fs::create_dir_all(&sub).unwrap();
            let file_path = sub.join("arquivo.txt");
            let mut f = File::create(&file_path).unwrap();
            writeln!(f, "conteúdo {}", i).unwrap();
            // modifica a data para testar ordenação
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = fs::metadata(&file_path).unwrap().permissions();
                perms.set_mode(0o644);
                fs::set_permissions(&file_path, perms).unwrap();
            }
        }

        let json = listar(&dir);
        assert!(json.is_array());
        let arr = json.as_array().unwrap();
        // não deve exceder MAX_ITEMS
        assert!(arr.len() <= MAX_ITEMS);
        // deve conter exatamente 5 itens
        assert_eq!(arr.len(), 5);
        // verifica campos
        for item in arr {
            assert!(item.get("caminho").is_some());
            assert!(item.get("bytes").is_some());
            assert!(item.get("modificado").is_some());
        }

        // limpeza
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_ler_correto() {
        let dir = temp_dir();
        let file_path = dir.join("exemplo.txt");
        let mut f = File::create(&file_path).unwrap();
        write!(f, "Olá Mundo").unwrap();

        let res = ler(&dir, "exemplo.txt").expect("deveria ler sem erro");
        assert_eq!(res["caminho"], "exemplo.txt");
        assert_eq!(res["texto"], "Olá Mundo");
        assert!(res.get("modificado").is_some());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_ler_rejeita_caminho_absoluto() {
        let dir = temp_dir();
        let abs = dir.canonicalize().unwrap();
        let result = ler(&dir, abs.to_str().unwrap());
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Caminho absoluto não permitido");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_ler_rejeita_parent_dir() {
        let dir = temp_dir();
        let result = ler(&dir, "../fora.txt");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Uso de '..' não permitido");
        let _ = fs::remove_dir_all(dir);
    }
}
