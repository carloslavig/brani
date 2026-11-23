//! link/fonte_siconfi.py esta TRAVADO no Python: o RREO/Tesouro nao vale como fonte de numero de video. Numero de video vem
//! SO do tribunal de contas que fiscaliza o municipio. Aqui fica so a trava: `preparar` sempre recusa e explica; quem chama
//! registra a pendencia. A leitura do RREO (baixar, montar, achado) NAO foi portada de proposito: codigo que nao existe nao
//! vira fonte por engano.
#![allow(dead_code)]

pub const MOTIVO: &str = "o RREO/Tesouro não vale como fonte de vídeo: use o leitor do tribunal de contas do estado";

/// Nunca serve de fonte de numero de video.
pub fn serve_de_fonte() -> bool {
    false
}

/// Sempre Err(MOTIVO), para qualquer cidade. Nao le, nao baixa e nao grava nada.
pub fn preparar(_numero: &str, _uf: &str, _nome: &str, _pasta: &std::path::Path) -> Result<serde_json::Value, String> {
    Err(MOTIVO.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recusa_sempre_e_nao_grava_nada() {
        let p = std::env::temp_dir().join(format!("braniac-link_fonte_siconfi-{}", std::process::id()));
        for (n, uf, nome) in [("1001", "SP", "Capital Um"), ("901", "MT", "Cidade Teste"), ("", "", "")] {
            assert_eq!(preparar(n, uf, nome, &p).unwrap_err(), MOTIVO);
        }
        assert!(!serve_de_fonte() && MOTIVO.contains("tribunal de contas") && !p.exists());
    }
}
