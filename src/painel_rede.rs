//! Painel da esteira na rede local (celular, tablet), SO LEITURA: uma porta propria onde so existem a tela, os dados em lista branca
//! (o resumo publico da esteira), o manifesto e o icone. Qualquer outra rota = 404: nada do resto do Braniac sai para a rede.
//! Endereco FIXO vence o tunel provisorio, e NENHUM link sai sozinho: o endereco so e dito quando o dono pede.
//! Ficou de fora: o servidor HTTP em si, desenhar o icone, subir e matar o tunel (processos). Aqui ficam as regras que eles seguem.
//! Diferenca do Python: o texto do endereco nao cita nome de usuario.
#![allow(dead_code)]
use crate::empresa_dev::rx;
use regex::Regex;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::OnceLock;

pub const PORTA: u16 = 8772;

#[derive(Debug, PartialEq)]
pub enum Rota {
    Tela,
    Dados,
    Manifesto,
    Icone(u32),
    /// Tudo que nao esta na lista branca.
    NaoExiste,
}

/// So GET nas rotas da lista. A consulta (`?...`) e ignorada.
pub fn rota(metodo: &str, caminho: &str) -> Rota {
    if metodo != "GET" {
        return Rota::NaoExiste;
    }
    match caminho.split('?').next().unwrap_or("") {
        "/" | "/parceiro" => Rota::Tela,
        "/api/parceiro" => Rota::Dados,
        "/manifest.webmanifest" => Rota::Manifesto,
        "/icone-192.png" => Rota::Icone(192),
        "/icone-512.png" => Rota::Icone(512),
        _ => Rota::NaoExiste,
    }
}

pub fn manifesto() -> Value {
    json!({ "name": "Esteira de vídeos", "short_name": "Esteira", "start_url": "/", "scope": "/", "display": "standalone", "background_color": "#16151A", "theme_color": "#16151A", "lang": "pt-BR",
            "icons": [{ "src": "/icone-192.png", "sizes": "192x192", "type": "image/png" }, { "src": "/icone-512.png", "sizes": "512x512", "type": "image/png", "purpose": "any maskable" }] })
}

fn texto(arq: &Path) -> Option<String> {
    std::fs::read_to_string(arq).ok().map(|t| t.trim_start_matches('\u{feff}').trim().to_string()).filter(|t| !t.is_empty())
}

/// Endereco fixo do painel (arquivo escrito por quem configurou o acesso de fora), se existir.
pub fn endereco_fixo(arq_fixo: &Path) -> Option<String> {
    texto(arq_fixo)
}

/// Link provisorio do tunel rapido, guardado em dados/parceiro/painel_url.txt.
pub fn url_publica(dados: &Path) -> Option<String> {
    texto(&dados.join("parceiro").join("painel_url.txt"))
}

/// Resposta quando o DONO pede o link do painel. E o unico jeito de o endereco sair.
pub fn endereco(arq_fixo: &Path, dados: &Path) -> String {
    if let Some(f) = endereco_fixo(arq_fixo) {
        let base = f.trim_end_matches('/');
        return format!("Braniac de qualquer lugar: {base}/  · painel da esteira dentro dele: {base}/parceiro  · só o painel: {base}:8443/");
    }
    match url_publica(dados) {
        Some(u) => format!("Painel da esteira (link provisório, o fixo ainda não foi ativado): {u}"),
        None => "O painel da esteira ainda não tem endereço fora de casa.".into(),
    }
}

/// Com endereco fixo o tunel provisorio nao sobe (nem volta depois de cair).
pub fn deve_subir_tunel(arq_fixo: &Path) -> bool {
    endereco_fixo(arq_fixo).is_none()
}

/// Linha da saida do tunel: se trouxer um endereco novo, SO guarda (nunca avisa ninguem). Devolve o endereco guardado.
pub fn anotar_tunel(dados: &Path, linha: &str) -> Option<String> {
    static R: OnceLock<Regex> = OnceLock::new();
    let url = rx(&R, r"https://[a-z0-9-]+\.trycloudflare\.com").find(linha)?.as_str().to_string();
    if url_publica(dados).as_deref() == Some(&url) {
        return None;
    }
    let arq = dados.join("parceiro").join("painel_url.txt");
    std::fs::create_dir_all(arq.parent()?).ok()?;
    std::fs::write(arq, &url).ok()?;
    Some(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn so_a_lista_branca_responde() {
        assert!(rota("GET", "/") == Rota::Tela && rota("GET", "/parceiro?x=1") == Rota::Tela && rota("GET", "/api/parceiro") == Rota::Dados);
        assert!(rota("GET", "/manifest.webmanifest") == Rota::Manifesto && rota("GET", "/icone-512.png") == Rota::Icone(512));
        for fora in ["/api/status", "/api/chat", "/b2", "/api/parceiro/../status", "/icone-999.png", "//", "/parceiro/", "/api/sauraki"] {
            assert_eq!(rota("GET", fora), Rota::NaoExiste, "{fora}");
        }
        for m in ["POST", "PUT", "DELETE"] {
            assert_eq!(rota(m, "/api/parceiro"), Rota::NaoExiste); // so leitura
        }
        assert_eq!(manifesto()["icons"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn endereco_fixo_vence_o_provisorio_e_o_tunel_so_guarda() {
        let d = std::env::temp_dir().join(format!("braniac-painel_rede-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let fixo = d.join("endereco.txt");
        assert!(endereco(&fixo, &d).contains("ainda não tem endereço") && deve_subir_tunel(&fixo));
        assert_eq!(anotar_tunel(&d, "INF |  https://abc-def-1.trycloudflare.com  |").as_deref(), Some("https://abc-def-1.trycloudflare.com"));
        assert_eq!(anotar_tunel(&d, "de novo https://abc-def-1.trycloudflare.com"), None); // o mesmo endereco nao e regravado
        assert_eq!(anotar_tunel(&d, "INF conexão registrada https://exemplo.com"), None);
        assert!(endereco(&fixo, &d).contains("provisório") && endereco(&fixo, &d).contains("abc-def-1"));
        std::fs::write(&fixo, "\u{feff}https://meu-pc.exemplo.ts.net/\n").unwrap();
        let e = endereco(&fixo, &d);
        assert!(e.contains("https://meu-pc.exemplo.ts.net/parceiro") && e.contains(":8443/") && !e.contains("trycloudflare") && !deve_subir_tunel(&fixo));
        let _ = std::fs::remove_dir_all(&d);
    }
}
