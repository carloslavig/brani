//! Agent Skills no Braniac (braniac/skills.py, formato aberto agentskills.io): cada skill e uma pasta `<nome>/` com um SKILL.md
//! (frontmatter com `name` e `description` + instrucoes) e, opcional, arquivos de apoio. Divulgacao progressiva:
//! 1) `catalogo`: so "nome: descricao"; 2) `usar(nome)`: instrucoes completas; 3) `usar(nome, arquivo)`: um arquivo de apoio.
//! Travas: skill so vale com nome no padrao e igual ao da pasta; arquivo de apoio so de DENTRO da propria skill; script de skill NAO e executado aqui.
#![allow(dead_code)]
use regex::Regex;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn ini(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// (campos, corpo). YAML simples: `chave: valor` numa linha (aspas opcionais); linha recuada e ignorada.
pub fn frontmatter(texto: &str) -> (Vec<(String, String)>, String) {
    let Some(m) = Regex::new(r"(?s)^---\s*\n(.*?)\n---\s*\n?(.*)$").expect("regex fixa").captures(texto) else { return (vec![], texto.to_string()) };
    let mut campos: Vec<(String, String)> = vec![];
    for linha in m[1].lines() {
        let Some((k, v)) = linha.split_once(':') else { continue };
        if !linha.starts_with([' ', '\t']) {
            campos.retain(|c| c.0 != k.trim());
            campos.push((k.trim().to_string(), v.trim().trim_matches('"').trim_matches('\'').to_string()));
        }
    }
    (campos, m[2].to_string())
}

fn campo<'a>(campos: &'a [(String, String)], k: &str) -> &'a str {
    campos.iter().find(|c| c.0 == k).map(|c| c.1.as_str()).unwrap_or("")
}

fn nome_valido(n: &str) -> bool {
    !n.is_empty() && n.split('-').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
}

/// [(nome, descricao, pasta)] das skills validas (nome no padrao e igual ao da pasta, descricao presente).
pub fn listar(base: &Path) -> Vec<(String, String, PathBuf)> {
    let Ok(rd) = base.read_dir() else { return vec![] };
    let mut pastas: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    pastas.sort();
    let mut out = vec![];
    for pasta in pastas {
        let Ok(txt) = std::fs::read_to_string(pasta.join("SKILL.md")) else { continue };
        let (campos, _) = frontmatter(&txt);
        let (nome, desc) = (campo(&campos, "name"), campo(&campos, "description"));
        if nome_valido(nome) && pasta.file_name().is_some_and(|f| f.to_string_lossy() == nome) && !desc.is_empty() {
            out.push((nome.to_string(), ini(desc, 1024), pasta));
        }
    }
    out
}

/// Linhas "nome: descricao" para a descricao da ferramenta (fase de descoberta).
pub fn catalogo(base: &Path, maximo: usize) -> String {
    listar(base).iter().map(|(n, d, _)| format!("{n}: {}", ini(d, maximo))).collect::<Vec<_>>().join("; ")
}

/// Campo `allowed-tools` (nomes separados por espaco): ferramentas que entram no agente quando a skill e ativada.
pub fn ferramentas_da_skill(nome: &str, base: &Path) -> BTreeSet<String> {
    let Some((_, _, pasta)) = listar(base).into_iter().find(|s| s.0 == nome) else { return BTreeSet::new() };
    let txt = std::fs::read_to_string(pasta.join("SKILL.md")).unwrap_or_default();
    campo(&frontmatter(&txt).0, "allowed-tools").split_whitespace().map(str::to_string).collect()
}

fn apoio(pasta: &Path, atual: &Path, out: &mut Vec<String>) {
    let Ok(rd) = atual.read_dir() else { return };
    for p in rd.flatten().map(|e| e.path()) {
        if p.is_dir() {
            apoio(pasta, &p, out);
        } else if p.file_name().is_some_and(|f| f != "SKILL.md") {
            if let Ok(rel) = p.strip_prefix(pasta) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
}

/// Fase de ativacao: instrucoes completas da skill (ou um arquivo de apoio dela). O texto devolvido e procedimento a seguir com as
/// ferramentas normais; nada e executado aqui.
pub fn usar(nome: &str, arquivo: &str, base: &Path) -> String {
    let skills = listar(base);
    let Some((_, _, pasta)) = skills.iter().find(|s| s.0 == nome) else {
        let nomes: Vec<&str> = skills.iter().map(|s| s.0.as_str()).collect();
        return format!("Skill '{nome}' não existe. Disponíveis: {}.", if nomes.is_empty() { "nenhuma".to_string() } else { nomes.join(", ") });
    };
    if !arquivo.is_empty() {
        // so arquivos de dentro da propria skill: o caminho real (sem .. nem atalho) tem que ficar debaixo da pasta dela
        let dentro = pasta.canonicalize().ok().zip(pasta.join(arquivo).canonicalize().ok()).filter(|(raiz, alvo)| alvo != raiz && alvo.starts_with(raiz) && alvo.is_file());
        return match dentro.and_then(|(_, alvo)| std::fs::read_to_string(alvo).ok()) {
            Some(t) => ini(&t, 8000),
            None => format!("Arquivo '{arquivo}' não existe na skill {nome}."),
        };
    }
    let (_, corpo) = frontmatter(&std::fs::read_to_string(pasta.join("SKILL.md")).unwrap_or_default());
    let mut extras = vec![];
    apoio(pasta, pasta, &mut extras);
    extras.sort();
    let fim = if extras.is_empty() { String::new() } else { format!("\n\n(Arquivos de apoio: {} — leia com usar_skill(nome, arquivo).)", extras.join(", ")) };
    ini(corpo.trim(), 8000) + &fim
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-skills-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let skill = |pasta: &str, md: &str| {
            std::fs::create_dir_all(d.join("skills").join(pasta).join("references")).unwrap();
            std::fs::write(d.join("skills").join(pasta).join("SKILL.md"), md).unwrap();
        };
        skill("fazer-cafe", "---\nname: fazer-cafe\ndescription: \"Como passar cafe: agua, po e filtro\"\nallowed-tools: pc  pesquisar\nmetadata:\n  name: recuado\n---\n\n# Passo a passo\nFerva a agua.\n");
        skill("outra", "---\nname: 'outra'\ndescription: Segunda skill\n---\nCorpo.");
        skill("nome-errado", "---\nname: diferente\ndescription: x\n---\nx");
        skill("Maiuscula", "---\nname: Maiuscula\ndescription: x\n---\nx");
        skill("sem-descricao", "---\nname: sem-descricao\n---\nx");
        skill("sem-frontmatter", "so texto");
        std::fs::write(d.join("skills").join("fazer-cafe").join("references").join("graos.md"), "Arabica e robusta.").unwrap();
        std::fs::write(d.join("skills").join("fazer-cafe").join("rodar.ps1"), "Remove-Item x").unwrap();
        std::fs::write(d.join("segredo.txt"), "fora das skills").unwrap();
        d
    }

    #[test]
    fn lista_so_skill_valida_e_monta_o_catalogo() {
        let b = base("lista");
        let l = listar(&b.join("skills"));
        assert_eq!(l.iter().map(|s| s.0.as_str()).collect::<Vec<_>>(), ["fazer-cafe", "outra"]);
        assert_eq!(l[0].1, "Como passar cafe: agua, po e filtro");
        assert_eq!(catalogo(&b.join("skills"), 12), "fazer-cafe: Como passar ; outra: Segunda skil");
        assert_eq!(catalogo(&b.join("nao-existe"), 160), "");
        assert_eq!(ferramentas_da_skill("fazer-cafe", &b.join("skills")).into_iter().collect::<Vec<_>>(), ["pc", "pesquisar"]);
        assert!(ferramentas_da_skill("outra", &b.join("skills")).is_empty() && ferramentas_da_skill("nome-errado", &b.join("skills")).is_empty());
        for (n, ok) in [("a", true), ("a-b2", true), ("", false), ("-a", false), ("a-", false), ("a--b", false), ("A", false), ("a_b", false), ("a b", false)] {
            assert_eq!(nome_valido(n), ok, "{n}");
        }
    }

    #[test]
    fn usar_devolve_instrucoes_e_so_le_arquivo_de_dentro_da_skill() {
        let b = base("usar");
        let s = b.join("skills");
        assert_eq!(usar("fazer-cafe", "", &s), "# Passo a passo\nFerva a agua.\n\n(Arquivos de apoio: references/graos.md, rodar.ps1 — leia com usar_skill(nome, arquivo).)");
        assert_eq!(usar("outra", "", &s), "Corpo.");
        assert_eq!(usar("fazer-cafe", "references/graos.md", &s), "Arabica e robusta.");
        // script de skill so e LIDO como texto, nunca executado
        assert_eq!(usar("fazer-cafe", "rodar.ps1", &s), "Remove-Item x");
        for fora in ["../../segredo.txt", "..\\..\\segredo.txt", "../outra/SKILL.md", "references", ".", "nao-existe.md", &b.join("segredo.txt").display().to_string()] {
            assert_eq!(usar("fazer-cafe", fora, &s), format!("Arquivo '{fora}' não existe na skill fazer-cafe."), "{fora}");
        }
        assert_eq!(usar("nome-errado", "", &s), "Skill 'nome-errado' não existe. Disponíveis: fazer-cafe, outra.");
        assert_eq!(usar("x", "", &b.join("vazio")), "Skill 'x' não existe. Disponíveis: nenhuma.");
    }

    #[test]
    fn frontmatter_simples() {
        let (c, corpo) = frontmatter("---\r\nname: a\r\ndescription: 'b: c'\r\n---\r\ncorpo\r\n");
        assert_eq!((campo(&c, "name"), campo(&c, "description"), corpo.trim()), ("a", "b: c", "corpo"));
        assert_eq!(frontmatter("sem cabecalho"), (vec![], "sem cabecalho".to_string()));
    }
}
