//! Limpeza de disco assistida (porte de braniac/limpeza.py): SO ANALISA. Mostra onde o espaco esta indo e quais arquivos
//! grandes sao duplicados EXATOS. Este modulo nao apaga, nao move e nao esvazia nada: quem libera espaco e o dono.
//! Nunca entra em pastas do sistema, de programas, de projetos, perfis de navegador nem nas pastas movidas para outro disco;
//! juncao/atalho de pasta nao e seguido (contaria o tamanho em dobro).
//! Diferenca do Python: sem crate de SHA-1, os grupos sao formados por FNV-1a (inicio+fim, depois o arquivo inteiro) e so
//! valem depois de comparados byte a byte. Relatorio em dados/limpeza/relatorio.json.
#![allow(dead_code)]
use crate::tempo::Agora;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub const MIN_DUP: u64 = 20 * 1024 * 1024;
const MEGA: usize = 1 << 20;
const IGNORAR: [&str; 21] = ["$recycle.bin", "system volume information", "windows", "program files", "program files (x86)", "programdata", "appdata", ".git",
    "node_modules", "projetos", "chromeagente", "braniacloja", "recovery", "msocache", "$windows.~bt", "windowsapps", "onedrive",
    // modelo de IA e disco de VM que moram em outro disco (juncao no caminho antigo) nunca sao "duplicado"
    "ollama-models", "cache-ia", "braniac-dados", "wsl"];

pub fn pular(nome: &str) -> bool {
    IGNORAR.contains(&nome.to_lowercase().as_str())
}

/// (pasta, bytes) das maiores pastas ate `profundidade` niveis abaixo da raiz: onde o espaco esta indo.
pub fn tamanhos_pastas(raiz: &Path, profundidade: usize, topo: usize) -> Vec<(String, u64)> {
    fn andar(p: &Path, nivel: usize, profundidade: usize, tot: &mut Vec<(String, u64)>) -> u64 {
        let Ok(it) = std::fs::read_dir(p) else { return 0 };
        let mut total = 0;
        for e in it.flatten() {
            let Ok(tipo) = e.file_type() else { continue };
            if tipo.is_symlink() {
                continue; // juncao = pasta que mora em outro disco: contar aqui dobraria o tamanho
            }
            total += if tipo.is_dir() { andar(&e.path(), nivel + 1, profundidade, tot) } else { e.metadata().map_or(0, |m| m.len()) };
        }
        if nivel <= profundidade {
            tot.push((p.to_string_lossy().into_owned(), total));
        }
        total
    }
    let mut tot = Vec::new();
    andar(raiz, 0, profundidade, &mut tot);
    tot.sort_by(|a, b| b.1.cmp(&a.1));
    tot.truncate(topo);
    tot
}

fn fnv(h: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(h, |h, b| (h ^ *b as u64).wrapping_mul(0x100000001b3))
}

/// Parcial = primeiro e ultimo MiB (barato, separa quase tudo); completo = o arquivo inteiro.
fn resumo_do_arquivo(caminho: &Path, parcial: bool) -> Option<u64> {
    let mut f = std::fs::File::open(caminho).ok()?;
    let (mut h, mut buf) = (0xcbf29ce484222325u64, vec![0u8; MEGA]);
    if parcial {
        let tam = f.metadata().ok()?.len();
        let lidos = ler_cheio(&mut f, &mut buf).ok()?;
        h = fnv(h, &buf[..lidos]);
        f.seek(SeekFrom::Start(tam.saturating_sub(MEGA as u64))).ok()?;
        let lidos = ler_cheio(&mut f, &mut buf).ok()?;
        return Some(fnv(h, &buf[..lidos]));
    }
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            return Some(h);
        }
        h = fnv(h, &buf[..n]);
    }
}

fn iguais(a: &Path, b: &Path) -> bool {
    let (Ok(mut fa), Ok(mut fb)) = (std::fs::File::open(a), std::fs::File::open(b)) else { return false };
    let (mut ba, mut bb) = (vec![0u8; MEGA], vec![0u8; MEGA]);
    loop {
        let (Ok(na), Ok(nb)) = (ler_cheio(&mut fa, &mut ba), ler_cheio(&mut fb, &mut bb)) else { return false };
        if na != nb || ba[..na] != bb[..nb] {
            return false;
        }
        if na == 0 {
            return true;
        }
    }
}

fn ler_cheio(f: &mut std::fs::File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match f.read(&mut buf[n..])? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n)
}

fn agrupar(caminhos: Vec<PathBuf>, parcial: bool) -> Vec<Vec<PathBuf>> {
    let mut por: BTreeMap<u64, Vec<PathBuf>> = BTreeMap::new();
    for p in caminhos {
        if let Some(h) = resumo_do_arquivo(&p, parcial) {
            por.entry(h).or_default().push(p);
        }
    }
    por.into_values().filter(|l| l.len() > 1).collect()
}

/// Grupos de arquivos IDENTICOS de pelo menos `minimo` bytes: {"tamanho", "arquivos", "desperdicio"}, do maior desperdicio ao menor.
/// `limite_s` limita so a caminhada pelas pastas (a comparacao do que ja foi achado vai ate o fim).
pub fn duplicados(raizes: &[PathBuf], minimo: u64, limite_s: u64) -> Vec<Value> {
    let fim = std::time::Instant::now() + std::time::Duration::from_secs(limite_s);
    let mut por_tam: BTreeMap<u64, Vec<PathBuf>> = BTreeMap::new();
    let mut pilha: Vec<PathBuf> = raizes.iter().rev().cloned().collect();
    while let Some(dir) = pilha.pop() {
        if std::time::Instant::now() > fim {
            break;
        }
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let Ok(tipo) = e.file_type() else { continue };
            if tipo.is_symlink() {
                continue;
            }
            if tipo.is_dir() {
                if !pular(&e.file_name().to_string_lossy()) {
                    pilha.push(e.path());
                }
            } else if let Some(tam) = e.metadata().ok().map(|m| m.len()).filter(|t| *t >= minimo) {
                por_tam.entry(tam).or_default().push(e.path());
            }
        }
    }
    let mut grupos = Vec::new();
    for (tam, caminhos) in por_tam.into_iter().filter(|(_, c)| c.len() > 1) {
        for lista in agrupar(caminhos, true).into_iter().flat_map(|l| agrupar(l, false)) {
            // o resumo de 64 bits so aponta candidatos: quem nao for igual byte a byte ao primeiro fica de fora
            let mut arqs: Vec<String> = lista.iter().filter(|p| *p == &lista[0] || iguais(&lista[0], p)).map(|p| p.to_string_lossy().into_owned()).collect();
            if arqs.len() > 1 {
                arqs.sort();
                grupos.push(json!({ "tamanho": tam, "desperdicio": tam * (arqs.len() as u64 - 1), "arquivos": arqs }));
            }
        }
    }
    grupos.sort_by_key(|g| std::cmp::Reverse(g["desperdicio"].as_u64().unwrap_or(0)));
    grupos
}

/// Varre as raizes que existem e grava dados/limpeza/relatorio.json. So leitura do disco varrido.
pub fn varrer(dados: &Path, raizes: &[PathBuf], agora: &Agora) -> Value {
    let existem: Vec<PathBuf> = raizes.iter().filter(|r| r.exists()).cloned().collect();
    let pastas: serde_json::Map<String, Value> = existem.iter().map(|r| (r.to_string_lossy().into_owned(), json!(tamanhos_pastas(r, 2, 25)))).collect();
    let dup = duplicados(&existem, MIN_DUP, 3600);
    let total: u64 = dup.iter().map(|g| g["desperdicio"].as_u64().unwrap_or(0)).sum();
    let rel = json!({ "quando": format!("{} {}", agora.data.iso(), agora.hm()), "raizes": raizes.iter().map(|r| r.to_string_lossy().into_owned()).collect::<Vec<_>>(),
                      "pastas": pastas, "duplicados": dup, "desperdicio_total_gb": (total as f64 / 1e8).round() / 10.0 });
    let pasta = dados.join("limpeza");
    let _ = std::fs::create_dir_all(&pasta);
    let _ = std::fs::write(pasta.join("relatorio.json"), serde_json::to_string_pretty(&rel).unwrap_or_default());
    rel
}

/// Ultimo relatorio gravado, se houver.
pub fn ultimo(dados: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(dados.join("limpeza").join("relatorio.json")).ok()?).ok()
}

pub fn resumo(rel: &Value, max_grupos: usize) -> String {
    let vazio = Vec::new();
    let dup = rel["duplicados"].as_array().unwrap_or(&vazio);
    let mut linhas = vec![format!("Varredura de {}: {} grupos de duplicados exatos, ~{} GB repetidos.", rel["quando"].as_str().unwrap_or("?"), dup.len(), rel["desperdicio_total_gb"])];
    for g in dup.iter().take(max_grupos) {
        let arqs: Vec<&str> = g["arquivos"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        linhas.push(format!("- {:.2} GB x{}: {}", g["tamanho"].as_f64().unwrap_or(0.0) / 1e9, arqs.len(), arqs.join(" | ")));
    }
    linhas.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;

    fn pasta(nome: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("braniac-limpeza-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn pastas_protegidas_ficam_fora_e_so_duplicado_exato_conta() {
        for nome in ["ollama-models", "cache-ia", "braniac-dados", "wsl", "AppData", "Projetos", ".git", "Windows"] {
            assert!(pular(nome), "{nome}");
        }
        assert!(!pular("Downloads"));
        let p = pasta("dup");
        for (sub, nome, conteudo) in [("ollama-models", "blob", vec![b'x'; 64]), ("copia", "blob", vec![b'x'; 64]), ("copia", "blob2", vec![b'x'; 64]),
            ("copia", "outro", vec![b'y'; 64]), ("copia", "quase", [vec![b'x'; 63], vec![b'z']].concat()), ("copia", "pequeno", vec![b'x'; 8]), ("copia/sub", "pequeno2", vec![b'x'; 8])] {
            std::fs::create_dir_all(p.join(sub)).unwrap();
            std::fs::write(p.join(sub).join(nome), conteudo).unwrap();
        }
        let grupos = duplicados(&[p.clone()], 10, 60);
        let esperado: Vec<String> = ["blob", "blob2"].iter().map(|n| p.join("copia").join(n).to_string_lossy().into_owned()).collect();
        assert_eq!(grupos, vec![json!({ "tamanho": 64, "arquivos": esperado, "desperdicio": 64 })]);
        assert_eq!(duplicados(&[p.clone()], 1, 60).len(), 2); // com minimo 1 os dois pequenos tambem formam grupo
        assert!(iguais(&p.join("copia/blob"), &p.join("copia/blob2")) && !iguais(&p.join("copia/blob"), &p.join("copia/quase")));
        // nada foi apagado nem movido pela analise
        assert!(p.join("copia/blob2").exists() && p.join("ollama-models/blob").exists());
        let agora = Agora { data: Data::civil(2026, 11, 17).unwrap(), hora: 8, min: 30, seg: 0 };
        let rel = varrer(&p, &[p.join("copia"), p.join("nao-existe")], &agora);
        assert!(rel["quando"] == "2026-11-17 08:30" && rel["pastas"].as_object().unwrap().len() == 1 && rel["duplicados"].as_array().unwrap().is_empty());
        assert_eq!(ultimo(&p), Some(rel));
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn tamanho_por_pasta_ate_a_profundidade_e_resumo() {
        let p = pasta("tam");
        std::fs::create_dir_all(p.join("a/b/c/d")).unwrap();
        std::fs::write(p.join("a/um.bin"), vec![0u8; 1000]).unwrap();
        std::fs::write(p.join("a/b/c/d/fundo.bin"), vec![0u8; 500]).unwrap();
        std::fs::write(p.join("solto.bin"), vec![0u8; 10]).unwrap();
        let t = tamanhos_pastas(&p, 2, 25);
        let tam = |x: &Path| t.iter().find(|(c, _)| *c == x.to_string_lossy()).map(|(_, n)| *n);
        assert_eq!((tam(&p), tam(&p.join("a")), tam(&p.join("a").join("b")), tam(&p.join("a").join("b").join("c"))), (Some(1510), Some(1500), Some(500), None));
        assert!(t[0].1 == 1510 && tamanhos_pastas(&p, 2, 1).len() == 1);
        let rel = json!({ "quando": "2026-11-17 08:30", "desperdicio_total_gb": 4.2, "duplicados": [{ "tamanho": 2_100_000_000u64, "arquivos": ["E:\\a.mp4", "E:\\b.mp4", "E:\\c.mp4"] }] });
        assert_eq!(resumo(&rel, 15), "Varredura de 2026-11-17 08:30: 1 grupos de duplicados exatos, ~4.2 GB repetidos.\n- 2.10 GB x3: E:\\a.mp4 | E:\\b.mp4 | E:\\c.mp4");
        let _ = std::fs::remove_dir_all(&p);
    }

    #[cfg(windows)]
    #[test]
    fn juncao_nao_conta_em_dobro_nem_e_seguida() {
        let p = pasta("juncao");
        std::fs::create_dir_all(p.join("real")).unwrap();
        std::fs::write(p.join("real/a.bin"), vec![b'x'; 1000]).unwrap();
        let ok = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(p.join("atalho")).arg(p.join("real")).output().map(|o| o.status.success()).unwrap_or(false);
        assert!(ok && p.join("atalho/a.bin").exists(), "nao criei a juncao de teste");
        assert_eq!(tamanhos_pastas(&p, 2, 25).iter().find(|(c, _)| *c == p.to_string_lossy()).map(|(_, n)| *n), Some(1000));
        assert!(duplicados(&[p.clone()], 1, 60).is_empty());
        let _ = std::fs::remove_dir(p.join("atalho")); // tira so a juncao, nunca o conteudo dela
        let _ = std::fs::remove_dir_all(&p);
    }
}
