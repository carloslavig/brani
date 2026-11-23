//! Organiza a pasta Downloads (braniac/organizador.py). So MOVE arquivo solto para subpasta por tipo: nunca apaga, nunca abre/executa,
//! nunca sobrescreve, nunca toca em pasta existente. Copia identica vai para "Duplicados". Cada rodada grava o que moveu em
//! dados/organizador.json (desfazer = mover de volta).
//! Diferenca do Python: copia identica e achada comparando os bytes (sem crate de SHA-256); o resultado e o mesmo.
#![allow(dead_code)]
use crate::tempo::Agora;
use serde_json::json;
use std::io::Read;
use std::path::{Path, PathBuf};

const PARADOS: u64 = 600; // so mexe em arquivo parado ha 10 min (download em andamento fica quieto)
const IGNORAR: [&str; 1] = ["desktop.ini"];
const PARCIAIS: [&str; 4] = [".crdownload", ".part", ".tmp", ".download"];
const CATEGORIAS: [(&str, &[&str]); 7] = [
    ("Instaladores", &[".exe", ".msi", ".msix", ".appx", ".winmd"]),
    ("Imagens", &[".jpg", ".jpeg", ".png", ".gif", ".webp", ".bmp", ".heic"]),
    ("Videos", &[".mp4", ".mov", ".mkv", ".avi", ".webm"]),
    ("Audios", &[".mp3", ".wav", ".ogg", ".m4a", ".opus"]),
    ("Documentos", &[".pdf", ".docx", ".doc", ".xlsx", ".xls", ".pptx", ".txt", ".csv", ".odt"]),
    ("Paginas e codigo", &[".html", ".htm", ".css", ".js", ".py", ".json", ".xml", ".ini"]),
    ("Compactados", &[".zip", ".rar", ".7z", ".tar", ".gz"]),
];

fn ext(nome: &str) -> String {
    Path::new(nome).extension().map(|e| format!(".{}", e.to_string_lossy().to_lowercase())).unwrap_or_default()
}

/// Subpasta de um arquivo pelo nome.
pub fn destino(nome: &str) -> &'static str {
    let e = ext(nome);
    if nome.to_lowercase().starts_with("client_secret") && e == ".json" {
        return "Credenciais"; // segredo de API: fica separado e sinalizado na checagem de seguranca
    }
    CATEGORIAS.iter().find(|c| c.1.contains(&e.as_str())).map(|c| c.0).unwrap_or("Outros")
}

fn iguais(a: &Path, b: &Path) -> std::io::Result<bool> {
    let (mut fa, mut fb) = (std::io::BufReader::new(std::fs::File::open(a)?), std::io::BufReader::new(std::fs::File::open(b)?));
    let (mut ba, mut bb) = (vec![0u8; 1 << 16], vec![0u8; 1 << 16]);
    loop {
        let n = fa.read(&mut ba)?;
        if n == 0 {
            return Ok(fb.read(&mut bb)? == 0);
        }
        if fb.read_exact(&mut bb[..n]).is_err() || ba[..n] != bb[..n] {
            return Ok(false);
        }
    }
}

/// Devolve [(origem, destino)] movidos (ou que seriam, se `simular`). `epoch` = agora em segundos desde 1970.
pub fn organizar(pasta: &Path, dados: &Path, agora: &Agora, epoch: u64, simular: bool) -> Vec<(String, String)> {
    let Ok(rd) = pasta.read_dir() else { return vec![] };
    let mut arqs: Vec<(u64, String, u64)> = vec![]; // (mtime, nome, tamanho)
    for e in rd.flatten() {
        let Ok(m) = e.metadata() else { continue };
        let nome = e.file_name().to_string_lossy().to_string();
        let mtime = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(epoch);
        if m.is_file() && !IGNORAR.contains(&nome.to_lowercase().as_str()) && !PARCIAIS.contains(&ext(&nome).as_str()) && epoch.saturating_sub(mtime) >= PARADOS {
            arqs.push((mtime, nome, m.len()));
        }
    }
    // o original (mais antigo, nome mais curto) fica; a copia vai para Duplicados
    arqs.sort_by_key(|a| (a.0, a.1.chars().count()));
    let mut vistos: Vec<(u64, PathBuf)> = vec![]; // (tamanho, onde o arquivo esta agora)
    let mut movidos = vec![];
    for (_, nome, tam) in arqs {
        let p = pasta.join(&nome);
        let repetido = vistos.iter().any(|v| v.0 == tam && iguais(&v.1, &p).unwrap_or(false));
        let sub = if repetido { "Duplicados" } else { destino(&nome) };
        let (tronco, e) = (Path::new(&nome).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(), Path::new(&nome).extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default());
        let mut alvo = pasta.join(sub).join(&nome);
        let mut n = 1;
        while alvo.exists() {
            // nunca sobrescreve
            alvo = pasta.join(sub).join(format!("{tronco} ({n}){e}"));
            n += 1;
        }
        if !simular && (std::fs::create_dir_all(pasta.join(sub)).is_err() || std::fs::rename(&p, &alvo).is_err()) {
            continue;
        }
        if !repetido {
            vistos.push((tam, if simular { p.clone() } else { alvo.clone() }));
        }
        movidos.push((p.display().to_string(), alvo.display().to_string()));
    }
    if !movidos.is_empty() && !simular {
        let _ = std::fs::create_dir_all(dados);
        let log = json!({ "quando": format!("{} {}", agora.data.iso(), agora.hm()), "movidos": movidos });
        let _ = std::fs::write(dados.join("organizador.json"), serde_json::to_string_pretty(&log).unwrap_or_default());
    }
    movidos
}

/// Organiza e anota no relatorio da madrugada (so se mexeu em algo).
pub fn rodar(pasta: &Path, dados: &Path, agora: &Agora, epoch: u64, simular: bool, registrar: &dyn Fn(&str)) -> Vec<(String, String)> {
    let movidos = organizar(pasta, dados, agora, epoch, simular);
    if !movidos.is_empty() {
        let dup = movidos.iter().filter(|m| m.1.contains("Duplicados")).count();
        registrar(&format!("🗂️ Downloads organizado: {} arquivo(s) para subpastas por tipo ({dup} duplicado(s) separado(s), nada apagado).", movidos.len()));
    }
    movidos
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;
    use std::time::{Duration, SystemTime};

    fn pasta(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-organizador-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn agora() -> (Agora, u64) {
        (Agora { data: Data::civil(2026, 11, 18).unwrap(), hora: 3, min: 5, seg: 0 }, SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs())
    }

    fn velho(p: &Path, txt: &str) {
        std::fs::write(p, txt).unwrap();
        std::fs::File::options().write(true).open(p).unwrap().set_modified(SystemTime::now() - Duration::from_secs(3600)).unwrap();
    }

    #[test]
    fn organiza_sem_apagar_e_separa_duplicado() {
        let t = pasta("a");
        let (a, epoch) = agora();
        for (nome, txt) in [("a.pdf", "x"), ("a (1).pdf", "x"), ("setup.exe", "y"), ("desktop.ini", "i"), ("filme.mp4.crdownload", "p")] {
            velho(&t.join(nome), txt);
        }
        std::fs::write(t.join("novo.jpg"), "z").unwrap(); // baixado agora: fica quieto
        std::fs::create_dir_all(t.join("pasta")).unwrap();
        std::fs::write(t.join("pasta").join("dentro.pdf"), "x").unwrap();
        assert_eq!(organizar(&t, &t.join("dados"), &a, epoch, true).len(), 3);
        assert!(t.join("a.pdf").exists() && !t.join("dados").exists()); // simular nao mexe em nada
        let avisos = std::cell::RefCell::new(vec![]);
        let movidos = rodar(&t, &t.join("dados"), &a, epoch, false, &|s| avisos.borrow_mut().push(s.to_string()));
        assert_eq!(movidos.len(), 3);
        assert!(t.join("Documentos").join("a.pdf").exists() && t.join("Duplicados").join("a (1).pdf").exists());
        assert!(t.join("Instaladores").join("setup.exe").exists() && t.join("novo.jpg").exists() && t.join("desktop.ini").exists());
        assert!(t.join("filme.mp4.crdownload").exists() && t.join("pasta").join("dentro.pdf").exists());
        assert!(avisos.borrow()[0].contains("3 arquivo(s)") && avisos.borrow()[0].contains("1 duplicado(s)"));
        let log: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(t.join("dados").join("organizador.json")).unwrap()).unwrap();
        assert_eq!((log["quando"].as_str(), log["movidos"].as_array().map(|m| m.len())), (Some("2026-11-18 03:05"), Some(3)));
        // nada foi apagado: 6 arquivos antes, 6 depois (mais o de dentro da pasta)
        let conta = |d: &Path| d.read_dir().unwrap().flatten().filter(|e| e.path().is_file()).count();
        assert_eq!(conta(&t) + conta(&t.join("Documentos")) + conta(&t.join("Duplicados")) + conta(&t.join("Instaladores")), 6);
        assert!(rodar(&t, &t.join("dados"), &a, epoch, false, &|_| panic!("nao mexeu, nao registra")).is_empty());
    }

    #[test]
    fn nunca_sobrescreve_e_escolhe_a_subpasta_pelo_tipo() {
        let t = pasta("b");
        let (a, epoch) = agora();
        std::fs::create_dir_all(t.join("Documentos")).unwrap();
        std::fs::write(t.join("Documentos").join("nota.txt"), "antiga").unwrap();
        velho(&t.join("nota.txt"), "nova");
        let m = organizar(&t, &t.join("dados"), &a, epoch, false);
        assert!(m[0].1.ends_with("nota (1).txt"));
        assert_eq!(std::fs::read_to_string(t.join("Documentos").join("nota.txt")).unwrap(), "antiga");
        assert_eq!(std::fs::read_to_string(t.join("Documentos").join("nota (1).txt")).unwrap(), "nova");
        for (nome, sub) in [("Client_Secret_1.json", "Credenciais"), ("dados.json", "Paginas e codigo"), ("FOTO.JPG", "Imagens"), ("x.tar.gz", "Compactados"), ("semext", "Outros"), ("a.xyz", "Outros")] {
            assert_eq!(destino(nome), sub);
        }
    }
}
