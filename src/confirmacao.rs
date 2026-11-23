//! Entende confirmacao e negacao em linguagem natural ("pode sim", "manda ver" = sim; "nem a pau", "ta maluco?" = nao),
//! em vez de exigir "/sim" e "/nao". Porte completo de braniac/confirmacao.py.
//! Cuidado: so decide quando a frase e CURTA e e so isso. Frase longa ou pedido novo nunca vale como confirmacao do anterior.
#![allow(dead_code)]
use crate::contexto::sem_marca;

const MAX_PALAVRAS: usize = 8;
const MARCADORES_DE_PEDIDO: [&str; 12] = ["pra", "para", "pro", "dizendo", "falando", "avisando", "audio", "mensagem", "msg", "recado", "explicando", "contando"];
const NEGATIVAS: &[&str] = &[
    "nem fudendo", "nem a pau", "nem pensar", "nem ferrando", "nem morto", "nem por decreto", "nem ligando",
    "nao e pra", "nao quero", "nao faca", "nao faz", "nao mande", "nao manda", "nao envie", "nao envia", "nao precisa", "nao pode",
    "ta maluco", "esta maluco", "ta doido", "esta doido", "ta louco", "esta louco", "de jeito nenhum", "em hipotese nenhuma",
    "sem chance", "melhor nao", "deixa quieto", "deixa pra la", "deixa isso", "deixa assim", "esquece", "esqueca", "cancela", "cancelar", "cancele",
    "desiste", "desista", "negativo", "jamais", "nunca", "para tudo", "pare tudo", "nao", "nem",
];
/// So valem no INICIO da frase ("para!", "pare com isso").
const PRIMEIRA_PALAVRA_NEGATIVA: [&str; 5] = ["para", "pare", "chega", "ei", "calma"];
const POSITIVAS: &[&str] = &[
    "pode sim", "pode mandar", "pode enviar", "pode fazer", "pode ir", "pode ser", "pode", "podem ver", "manda ai", "manda ver", "manda bala",
    "manda brasa", "manda", "mande", "faca", "faz", "fazer", "faca isso", "envia", "envie", "executa", "execute", "isso mesmo", "isso",
    "claro", "com certeza", "certeza", "certo", "beleza", "blz", "fechado", "fechou", "bora", "vai la", "vai", "vamos", "positivo",
    "afirmativo", "confirmo", "confirmado", "confirma", "sim senhor", "sim", "ok", "okay", "uhum", "aham", "ta bom", "tudo bem", "combinado",
    "perfeito", "autorizo", "autorizado", "por favor", "senhor",
];

/// Palavras da frase: minusculas, sem acento, so letras e digitos.
fn palavras(texto: &str) -> Vec<String> {
    let t: String = texto.chars().flat_map(char::to_lowercase).map(sem_marca).map(|c| if c.is_ascii_lowercase() || c.is_ascii_digit() { c } else { ' ' }).collect();
    t.split_whitespace().map(str::to_string).collect()
}

/// Posicao (em palavras) da primeira ocorrencia de qualquer frase, inteira, ou None.
fn primeira_posicao(frases: &[&str], palavras: &[String]) -> Option<usize> {
    frases.iter().filter_map(|f| {
        let alvo: Vec<&str> = f.split(' ').collect();
        palavras.windows(alvo.len()).position(|j| j.iter().zip(&alvo).all(|(a, b)| a == b))
    }).min()
}

/// Some(true) = sim, Some(false) = nao, None = nao e uma resposta de confirmacao.
pub fn classificar(texto: &str) -> Option<bool> {
    let p = palavras(texto);
    if p.is_empty() || p.len() > MAX_PALAVRAS {
        return None;
    }
    let neg = primeira_posicao(NEGATIVAS, &p).or_else(|| PRIMEIRA_PALAVRA_NEGATIVA.contains(&p[0].as_str()).then_some(0));
    let pos = primeira_posicao(POSITIVAS, &p);
    if neg.is_some_and(|n| pos.map_or(true, |s| n <= s)) {
        return Some(false);
    }
    pos?;
    // "manda um audio pra Ana dizendo oi" e um pedido novo, nao um "sim"
    if !["sim", "pode", "ok", "claro", "isso"].contains(&p[0].as_str()) && p.iter().any(|x| MARCADORES_DE_PEDIDO.contains(&x.as_str())) {
        return None;
    }
    Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frases_que_confirmam() {
        for f in ["sim", "Sim.", "pode sim", "Pode mandar", "manda aí", "manda ver", "mande", "faça", "faz", "pode fazer", "isso mesmo", "claro",
                  "com certeza", "beleza", "fechou", "bora", "vai lá", "positivo", "confirmo", "uhum", "tá bom", "pode ser", "Sim, podem ver.", "manda bala", "/sim", "ok"] {
            assert_eq!(classificar(f), Some(true), "{f}");
        }
    }

    #[test]
    fn frases_que_negam() {
        for f in ["não", "nao", "nem fudendo", "nem a pau", "tá maluco?", "ta doido", "não é pra mandar", "nao quero", "cancela", "esquece isso", "deixa quieto",
                  "de jeito nenhum", "negativo", "jamais", "melhor não", "para", "pare tudo", "/nao", "não, cancela isso", "nem pensar", "NÃO!", "calma aí"] {
            assert_eq!(classificar(f), Some(false), "{f}");
        }
    }

    #[test]
    fn frases_que_nao_sao_confirmacao() {
        for f in ["me explica melhor", "manda um áudio pra Bia dizendo oi", "manda uma mensagem pra mamãe avisando que cheguei",
                  "qual é a capital da França", "o que você acha disso e do plano que a gente fez ontem à noite com a Ana",
                  "nao sei se entendi o que voce quer dizer com isso", "", "   ", "?!", "simplesmente não quero isso agora porque mudei de ideia e prefiro esperar",
                  "simples assim", "podemos conversar"] {
            assert_eq!(classificar(f), None, "{f}");
        }
    }

    #[test]
    fn a_primeira_palavra_decide_quando_ha_sim_e_nao() {
        assert_eq!(classificar("não, pode mandar"), Some(false));
        assert_eq!(classificar("pode mandar, não"), Some(true));
        // quem comeca com "sim/pode/ok" confirma mesmo citando o destino
        assert_eq!(classificar("pode mandar pra ela"), Some(true));
        // "para" so nega no inicio da frase
        assert_eq!(classificar("manda para ela"), None);
    }
}
