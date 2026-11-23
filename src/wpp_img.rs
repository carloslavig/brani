//! Imagens no WhatsApp: LER a foto/print mais recente que a pessoa mandou (porte das regras de braniac/wpp_img.py).
//! Vale so imagem RECEBIDA (nunca as nossas) e com pelo menos 80 px de largura (figurinha/icone nao conta); a conversa aberta
//! tem que ser a pedida; nada e baixado para disco nem enviado. Uma operacao na pagina por vez.
//! Ficou de fora: ler os bytes da miniatura na pagina e descrever a imagem (closures).
#![allow(dead_code)]
use crate::contexto::Ctx;
use crate::wpp_voz::{abrir_chat, um_por_vez, Pagina};

/// a miniatura carrega depois de a conversa abrir: tenta 8 vezes, 1 s entre elas
pub const TENTATIVAS: usize = 8;
pub const LARGURA_MIN: u32 = 80;

/// Entre as imagens da conversa (nossa?, largura), a mais recente RECEBIDA e grande o bastante.
pub fn escolher_imagem(imgs: &[(bool, u32)]) -> Option<usize> {
    imgs.iter().rposition(|(nossa, largura)| !nossa && *largura >= LARGURA_MIN)
}

/// Abre a conversa (conferindo o destino) e devolve os bytes da imagem recebida mais recente. `ler_imagem` le a miniatura escolhida.
pub fn baixar_ultima_imagem(ctx: &Ctx, pg: &Pagina, nome: &str, ler_imagem: &dyn Fn() -> Option<Vec<u8>>, esperar: &dyn Fn(f64)) -> Result<Vec<u8>, String> {
    um_por_vez(|| {
        abrir_chat(ctx, pg, nome, None)?;
        for _ in 0..TENTATIVAS {
            esperar(1.0);
            if let Some(b) = ler_imagem() {
                return Ok(b);
            }
        }
        Err("Não achei imagem recebida nessa conversa.".into())
    })
}

/// Baixa e descreve. O erro de quem descreve volta curto (120 letras).
pub fn ver_ultima_imagem(ctx: &Ctx, pg: &Pagina, nome: &str, ler_imagem: &dyn Fn() -> Option<Vec<u8>>, esperar: &dyn Fn(f64), descrever: &dyn Fn(&[u8]) -> Result<String, String>) -> Result<String, String> {
    let dados = baixar_ultima_imagem(ctx, pg, nome, ler_imagem, esperar)?;
    descrever(&dados).map_err(|e| e.chars().take(120).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn so_imagem_recebida_e_grande_conta() {
        assert_eq!(escolher_imagem(&[(false, 300), (true, 400), (false, 40)]), Some(0)); // a nossa e a figurinha ficam de fora
        assert_eq!(escolher_imagem(&[(false, 300), (false, 80)]), Some(1));
        assert_eq!(escolher_imagem(&[(true, 400), (false, 79)]), None);
    }

    #[test]
    fn espera_a_miniatura_e_confere_o_destino() {
        let c = Ctx::default();
        let listar = || vec!["Ana Souza".to_string()];
        let (buscar, abrir, cancelar) = (|_: &str| (), |_: &str| true, || ());
        let titulo = || "Ana Souza".to_string();
        let pg = Pagina { listar: &listar, buscar: &buscar, abrir: &abrir, titulo: &titulo, cancelar_gravacao: &cancelar };
        let n = Cell::new(0);
        let na_terceira = || { n.set(n.get() + 1); (n.get() == 3).then(|| vec![1u8, 2]) };
        let esperas = Cell::new(0.0);
        let esperar = |s: f64| esperas.set(esperas.get() + s);
        let ok = |b: &[u8]| Ok(format!("print com {} bytes", b.len()));
        assert_eq!(ver_ultima_imagem(&c, &pg, "ana", &na_terceira, &esperar, &ok).unwrap(), "print com 2 bytes");
        assert_eq!(esperas.get(), 3.0);
        let nunca = || None;
        assert!(baixar_ultima_imagem(&c, &pg, "ana", &nunca, &esperar).unwrap_err().contains("Não achei imagem"));
        assert_eq!(esperas.get(), 3.0 + TENTATIVAS as f64);
        // conversa errada aberta: nem tenta ler
        let outro = || "Caio".to_string();
        let leu = Cell::new(false);
        let ler = || { leu.set(true); Some(vec![1u8]) };
        assert!(baixar_ultima_imagem(&c, &Pagina { titulo: &outro, ..pg }, "ana", &ler, &esperar).unwrap_err().starts_with("🔒") && !leu.get());
        let ruim = |_: &[u8]| Err("x".repeat(500));
        assert_eq!(ver_ultima_imagem(&c, &pg, "ana", &ler, &esperar, &ruim).unwrap_err().len(), 120);
    }
}
