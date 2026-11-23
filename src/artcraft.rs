//! ArtCraft dentro do Jarvis (braniac/artcraft.py): so as TRAVAS de arquivo. A entrada tem de existir e nao pode estar em local protegido
//! (chaves, perfil do navegador, .env); a saida fica SEMPRE na pasta de trabalho, com nome simples e extensao conhecida, entao o original
//! nunca e alterado. De fora: rodar os aplicativos, o catalogo de acoes e o desenho por descricao (nao portados).
#![allow(dead_code)]
use regex::Regex;
use std::path::{Path, PathBuf};

const EXT_SAIDA: [&str; 22] = ["png", "jpg", "jpeg", "webp", "tif", "tiff", "pdf", "svg", "docx", "odt", "txt", "xlsx", "csv", "tsv", "json", "html", "pptx", "wav", "flac", "mp3", "ogg", "dxf"];
const BLOQUEADO: [&str; 6] = ["\\.ssh", "\\appdata\\roaming\\microsoft", ".env", "id_rsa", "login data", "\\cookies"];

fn protegido(caminho: &str) -> bool {
    let c = caminho.to_lowercase().replace('/', "\\");
    BLOQUEADO.iter().any(|b| c.contains(b))
}

/// Caminho real do arquivo de entrada (atalhos e ".." resolvidos), ou o motivo da recusa.
/// shortcut: "~" nao e expandido; quem liga passa o caminho inteiro.
pub fn entrada(caminho: &str) -> Result<PathBuf, String> {
    let bruto = caminho.trim().trim_matches('"');
    if protegido(bruto) {
        return Err("Esse local é protegido; não abro por segurança.".into());
    }
    let real = std::fs::canonicalize(bruto).ok().filter(|c| c.is_file()).ok_or(format!("Não achei o arquivo: {bruto}"))?;
    let texto = real.to_string_lossy();
    let texto = texto.strip_prefix(r"\\?\").unwrap_or(&texto);
    if protegido(texto) {
        return Err("Esse local é protegido; não abro por segurança.".into()); // o atalho apontava para la
    }
    Ok(PathBuf::from(texto))
}

/// Caminho de saida: sempre DENTRO de `trabalho`, so o nome do arquivo (sem pastas), caracteres simples; extensao desconhecida vira `ext`.
pub fn saida(trabalho: &Path, nome: &str, ext: &str) -> PathBuf {
    let so_nome = nome.trim().trim_matches('"').rsplit(['/', '\\']).next().unwrap_or("");
    let mut n = Regex::new(r"[^\w\-. ]").expect("regex fixa").replace_all(so_nome, "_").to_string();
    if n.is_empty() || n.chars().all(|c| c == '.') {
        n = "resultado".into();
    }
    let p = Path::new(&n);
    if !p.extension().and_then(|e| e.to_str()).is_some_and(|e| EXT_SAIDA.contains(&e.to_lowercase().as_str()) || e.eq_ignore_ascii_case("dwg")) {
        n = format!("{}.{}", p.file_stem().and_then(|s| s.to_str()).unwrap_or("resultado"), ext.trim_start_matches('.'));
    }
    trabalho.join(n)
}

/// Varios itens num campo so, separados por ";" ou quebra de linha.
pub fn lista(v: &str) -> Vec<String> {
    v.split([';', '\n']).map(str::trim).filter(|x| !x.is_empty()).map(String::from).collect()
}

/// Fala de entrega: lembra que o original ficou como estava.
pub fn pronto(alvo: &Path) -> String {
    format!("Pronto. Resultado em {} (o original não foi alterado).", alvo.display())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saida_fica_na_pasta_de_trabalho_e_local_protegido_nao_abre() {
        let t = Path::new("C:/trabalho");
        assert_eq!(saida(t, r"..\..\Windows\system32\foto final.PNG", ".png"), t.join("foto final.PNG"));
        assert_eq!(saida(t, "/etc/passwd", ".pdf"), t.join("passwd.pdf"));
        assert_eq!(saida(t, "relatório<1>.exe", ".pdf"), t.join("relatório_1_.pdf")); // extensao que nao e de saida vira a pedida
        assert_eq!((saida(t, "", "png"), saida(t, "..", ".png"), saida(t, "\"a.dwg\"", ".dxf")), (t.join("resultado.png"), t.join("resultado.png"), t.join("a.dwg")));
        assert!(saida(t, "a/b/c.docx", ".docx").parent() == Some(t));
        let d = std::env::temp_dir().join(format!("braniac-artcraft-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("sub")).unwrap();
        std::fs::write(d.join("foto.png"), b"x").unwrap();
        std::fs::write(d.join("producao.env"), b"x").unwrap();
        let p = |n: &str| d.join(n).to_string_lossy().to_string();
        assert!(entrada(&format!(" \"{}\" ", p("foto.png"))).unwrap().ends_with("foto.png"));
        assert!(entrada(&p("sub/../foto.png")).unwrap().ends_with("foto.png") && !entrada(&p("sub/../foto.png")).unwrap().to_string_lossy().contains(".."));
        assert!(entrada(&p("nao-existe.png")).unwrap_err().starts_with("Não achei o arquivo") && entrada(&p("sub")).unwrap_err().starts_with("Não achei"));
        for protegido in [p("producao.env"), r"C:\Users\x\.ssh\id_rsa".to_string(), "C:/Users/x/AppData/Roaming/Microsoft/a.txt".to_string(), r"C:\perfil\Login Data".to_string(), r"C:\perfil\Cookies".to_string()] {
            assert_eq!(entrada(&protegido).unwrap_err(), "Esse local é protegido; não abro por segurança.", "{protegido}");
        }
        assert_eq!(lista("a.pdf; b.pdf\n\n c.pdf ;"), ["a.pdf", "b.pdf", "c.pdf"]);
        assert!(pronto(Path::new("x.png")).contains("o original não foi alterado"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
