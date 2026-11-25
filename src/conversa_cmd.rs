//! Conversa-comando dentro das sessoes (/api/b2/chat): sem tela propria.
//! Pedido de esteira ("coloca os proximos 5 videos na esteira e manda pro site") e entendido aqui, sem IA, e roda a ferramenta
//! `esteira_parceiro` ja existente (mesmas travas); o resto vai para o Jarvis (/api/chat). Nenhuma rota nova.
use crate::roteador::Roteador;
use crate::{executor, fatos, jarvis_travas, power, tempo};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, PartialEq)]
pub enum Pedido {
    /// acao da ferramenta esteira_parceiro ("status", "parar", "continuar", "mais"), quantos (so "mais") e se disse "site"
    Esteira { acao: &'static str, quantos: i64, site: bool },
    Livre,
}

fn sem_acento(s: &str) -> String {
    s.to_lowercase().chars().map(|c| match c { 'á'|'à'|'â'|'ã' => 'a', 'é'|'ê' => 'e', 'í' => 'i', 'ó'|'ô'|'õ' => 'o', 'ú' => 'u', 'ç' => 'c', c => c }).collect()
}

/// Primeiro numero do pedido, em digitos ou por extenso (ate dez).
fn quantos(t: &str) -> Option<i64> {
    const EXT: [&str; 10] = ["um", "dois", "tres", "quatro", "cinco", "seis", "sete", "oito", "nove", "dez"];
    t.split(|c: char| !c.is_alphanumeric()).find_map(|w| w.parse::<i64>().ok().or_else(|| EXT.iter().position(|e| *e == w).map(|i| i as i64 + 1)))
}

pub fn interpretar(texto: &str) -> Pedido {
    let t = sem_acento(texto);
    let tem = |ps: &[&str]| ps.iter().any(|p| t.contains(p));
    if !tem(&["esteira", "video", "cidade"]) {
        return Pedido::Livre;
    }
    let site = tem(&["site", "parceiro"]);
    if tem(&["parar a esteira", "para a esteira", "pare a esteira", "pausar a esteira"]) {
        return Pedido::Esteira { acao: "parar", quantos: 0, site };
    }
    if tem(&["continuar a esteira", "continua a esteira", "retomar a esteira", "religa a esteira"]) {
        return Pedido::Esteira { acao: "continuar", quantos: 0, site };
    }
    // "mais" so com numero explicito: sem N a conversa nunca aumenta a meta sozinha
    if tem(&["coloca", "coloque", "bota", "ponha", "poe", "mais ", "proximos", "proximas", "proximo", "faz ", "faca"]) && tem(&["esteira", "video"]) {
        if let Some(n) = quantos(&t).filter(|n| *n > 0) {
            return Pedido::Esteira { acao: "mais", quantos: n.min(200), site };
        }
    }
    if tem(&["esteira"]) && tem(&["status", "como esta", "como anda", "o que falta", "falta", "situacao"]) {
        return Pedido::Esteira { acao: "status", quantos: 0, site };
    }
    Pedido::Livre
}

/// Roda o pedido de esteira com `rodar(acao, quantos)` (a ferramenta esteira_parceiro). None = nao e pedido de esteira.
/// O Link com o Aliado e tocado pelo proprio maestro da esteira (nunca pelo site); aqui so se avisa.
pub fn tentar_com(texto: &str, rodar: &dyn Fn(&str, i64) -> String) -> Option<String> {
    let texto = texto.split("

[Projeto").next().unwrap_or(texto); // tira o contexto colado pela sessao
    let Pedido::Esteira { acao, quantos, site } = interpretar(texto) else { return None };
    let antes = rodar("status", 0);
    let aviso = "(Conferência com o Aliado: pelo Link da própria esteira, não por aqui; sem dado de prefeitura inventado.)";
    if acao == "status" {
        return Some(format!("{antes}
{aviso}"));
    }
    let destino = if site { " O envio ao site do Parceiro segue o fluxo da esteira (de 5 em 5, um render por vez, auditoria do guia mantida)." } else { "" };
    Some(format!("Antes: {antes}
{}{destino}
{aviso}", rodar(acao, quantos)))
}

/// Chamado pelo chat das sessoes (/api/b2/chat): pedido de esteira roda aqui com as mesmas travas do executor; o resto segue ao Jarvis.
pub fn tentar(dados: &Path, rot: &Arc<Roteador>, conv: &str, texto: &str) -> Option<Value> {
    let (agora, ts) = (tempo::agora(), fatos::agora_ts());
    let amb = crate::ambiente(dados, rot, &agora, ts, false);
    let (ctx, familia) = (crate::contexto::Ctx::ler(dados), jarvis_travas::familia_livre(dados));
    let ped = jarvis_travas::Pedido { origem: "pc", conv: jarvis_travas::DONO_CONV, original: "conversa", armada: power::armada(dados, ts), ctx: &ctx, familia: &familia };
    let rodar = |a: &str, q: i64| executor::executar(&amb, &ped, "esteira_parceiro", &json!({ "acao": a, "quantos": q })).map(|(r, _)| r).unwrap_or_else(|| "esteira_parceiro indisponível".into());
    let resposta = tentar_com(texto, &rodar)?;
    let limpo = texto.split("

[Projeto").next().unwrap_or(texto);
    let _ = rot.store.add_msg(conv, "user", limpo, None);
    let _ = rot.store.add_msg(conv, "assistant", &resposta, Some("esteira_parceiro"));
    Some(json!({ "resposta": resposta, "provedor": "esteira_parceiro" }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pedido_de_esteira_extrai_n_e_destino() {
        let p = interpretar("coloca na esteira os próximos 5 vídeos das 5 cidades que faltam e manda para o site do Parceiro");
        assert_eq!(p, Pedido::Esteira { acao: "mais", quantos: 5, site: true });
        assert_eq!(interpretar("Coloque três videos na esteira"), Pedido::Esteira { acao: "mais", quantos: 3, site: false });
        assert_eq!(interpretar("como está a esteira?"), Pedido::Esteira { acao: "status", quantos: 0, site: false });
        assert_eq!(interpretar("para a esteira"), Pedido::Esteira { acao: "parar", quantos: 0, site: false });
        // sem numero nunca aumenta a meta; fora do assunto vai para o Jarvis
        assert_eq!(interpretar("coloca videos na esteira"), Pedido::Livre);
        assert_eq!(interpretar("qual a previsão do tempo?"), Pedido::Livre);
        assert_eq!(interpretar("coloca 999 videos na esteira"), Pedido::Esteira { acao: "mais", quantos: 200, site: false });
    }

    #[test]
    fn pedido_na_sessao_dispara_a_mesma_acao_e_menu_sem_conversar() {
        let chamadas = std::cell::RefCell::new(vec![]);
        let rodar = |a: &str, q: i64| { chamadas.borrow_mut().push((a.to_string(), q)); format!("ok {a}") };
        let r = tentar_com("coloca os próximos 5 vídeos na esteira e manda pro site

[Projeto: X. Instrucoes do projeto: video]", &rodar).unwrap();
        assert!(r.contains("ok mais") && r.contains("site do Parceiro"));
        assert_eq!(*chamadas.borrow(), vec![("status".to_string(), 0), ("mais".to_string(), 5)]);
        assert!(tentar_com("oi, tudo bem?", &rodar).is_none());
        let painel = include_str!("painel_b1.html");
        assert!(!painel.contains("conversa-cmd") && !painel.contains("convCmd") && !painel.contains(r#"["conversar""#));
    }
}
