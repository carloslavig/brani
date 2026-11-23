//! Troca do Braniac em Python pelo B.R.A.N.I, parte de REDE: copia das identidades (para `dados/`, nunca lidas de la em producao),
//! roteiro da troca (`TROCA.md`) e estado das chaves `loja`, `link` e `portal`.
//! `loja` e `link` ligam, mas quem faz a rede (WebRTC + ntfy + AES-GCM; Ed25519) e a PONTE temporaria em Python (ponte.rs): nao ha crate
//! de nenhum dos dois no B.R.A.N.I. `portal` tem servico de verdade (portal_servico).
//! Nada aqui imprime conteudo de arquivo: o retorno so traz nomes e contagens.
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const ROTEIRO: &str = include_str!("../TROCA.md");

/// (caminho relativo ao `data` do Python, e-obrigatorio-para)
const ARQUIVOS: [(&str, &str); 5] = [("portal.json", "portal"), ("par.json", "loja"), ("rede.json", "loja"), ("link/pares.json", "link"), ("link/ice.json", "link")];
const PASTAS: [(&str, &str); 1] = [("link/identidade", "link")];

fn origem_padrao() -> Option<PathBuf> {
    std::env::var_os("BRANIAC_ENV").map(|e| Path::new(&e).with_file_name("data"))
}

fn copiar_um(o: &Path, d: &Path, forcar: bool) -> &'static str {
    if !o.is_file() {
        return "ausente na origem";
    }
    if d.exists() && !forcar {
        return "ja existia (nao troquei)";
    }
    if let Some(p) = d.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    if std::fs::copy(o, d).is_ok() { "copiado" } else { "falhou" }
}

/// Copia as identidades do Python para `dados/`. Sem `forcar`, o que ja existe fica. So devolve nomes e resultados.
pub fn copiar(dados: &Path, origem: &Path, forcar: bool) -> Value {
    let mut r = vec![];
    for (rel, para) in ARQUIVOS {
        r.push(json!({ "arquivo": rel, "para": para, "resultado": copiar_um(&origem.join(rel), &dados.join(rel), forcar) }));
    }
    for (rel, para) in PASTAS {
        let mut n = 0;
        for e in std::fs::read_dir(origem.join(rel)).into_iter().flatten().flatten() {
            let nome = e.file_name();
            let res = copiar_um(&e.path(), &dados.join(rel).join(&nome), forcar);
            r.push(json!({ "arquivo": format!("{rel}/{}", nome.to_string_lossy()), "para": para, "resultado": res }));
            n += 1;
        }
        if n == 0 {
            r.push(json!({ "arquivo": rel, "para": para, "resultado": "ausente na origem" }));
        }
    }
    json!({ "copias": r, "obs": "so nomes: nenhum conteudo e mostrado; os arquivos ficam em dados/ (fora do Git)" })
}

pub fn estado(dados: &Path) -> Value {
    json!({
        "portal": crate::portal_servico::estado(dados),
        "loja": { "chave": crate::wpp_laco::chave(dados, "loja"), "servico": "ponte", "obs": "Rede (WebRTC, ntfy, AES-GCM) feita pela PONTE temporaria em Python; o B.R.A.N.I pensa e responde. So 1 ponta por vez: com o servidor antigo no ar a ponte se recusa a subir. Veja GET /api/ponte." },
        "link": { "chave": crate::wpp_laco::chave(dados, "link"), "servico": "ponte", "obs": "Rede (Ed25519, conexao direta/ntfy, fila 8771) feita pela PONTE temporaria em Python; pedido de render/producao e recusado com \"ainda nao disponivel\". Veja GET /api/ponte." },
    })
}

pub fn get(dados: &Path, url: &str) -> Option<crate::Resp> {
    match url.split('?').next() {
        Some("/api/troca/roteiro") => Some(crate::resposta(ROTEIRO.to_string(), "text/markdown", 200)),
        Some("/api/troca/rede") => Some(crate::dado(estado(dados))),
        _ => None,
    }
}

/// POST /api/troca/copiar {"origem": "<pasta data do Python>" (opcional), "forcar": bool}
pub fn post(dados: &Path, caminho: &str, v: &Value) -> Option<crate::Resp> {
    (caminho == "/api/troca/copiar").then(|| match v["origem"].as_str().map(PathBuf::from).or_else(origem_padrao) {
        Some(o) => crate::dado(copiar(dados, &o, v["forcar"] == true)),
        None => crate::erro("mande {\"origem\": \"<pasta data do Braniac antigo>\"} (ou defina BRANIAC_ENV)", 400),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copia_sem_sobrescrever_e_sem_mostrar_conteudo() {
        let t = std::env::temp_dir().join(format!("braniac-troca-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let (o, d) = (t.join("o"), t.join("d"));
        std::fs::create_dir_all(o.join("link/identidade")).unwrap();
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(o.join("portal.json"), "SEGREDO-NOVO").unwrap();
        std::fs::write(o.join("link/identidade/chave.x"), "SEGREDO2").unwrap();
        std::fs::write(d.join("portal.json"), "antigo").unwrap();
        let r = copiar(&d, &o, false);
        assert!(!r.to_string().contains("SEGREDO"));
        assert_eq!(std::fs::read_to_string(d.join("portal.json")).unwrap(), "antigo");
        assert_eq!(std::fs::read_to_string(d.join("link/identidade/chave.x")).unwrap(), "SEGREDO2");
        copiar(&d, &o, true);
        assert_eq!(std::fs::read_to_string(d.join("portal.json")).unwrap(), "SEGREDO-NOVO");
    }
}
