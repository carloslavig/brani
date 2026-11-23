//! Dois fatos curtos da memoria se contradizem? (Otto, NexoDev T069)
use crate::texto_pt::sem_acento;
use std::collections::HashSet;

/// Normaliza texto: minúsculas, sem acento, só letras/dígitos ASCII,
/// palavras separadas por um único espaço e com espaço nas bordas.
fn normaliza(texto: &str) -> String {
    let mut limpo = String::new();
    for c in sem_acento(texto).chars() {
        if c.is_ascii_alphanumeric() {
            limpo.push(c.to_ascii_lowercase());
        } else {
            limpo.push(' ');
        }
    }
    let unido = limpo.split_whitespace().collect::<Vec<_>>().join(" ");
    format!(" {} ", unido)
}

/// Detecta se a string contém alguma frase de negação.
fn tem_negacao(s: &str) -> bool {
    const NEG: [&str; 10] = [
        "nao ha", "nao tem", "nao temos", "nao esta", "nao estao",
        "nao disponivel", "indisponivel", "em falta", "fora do ar", "acabou",
    ];
    NEG.iter().any(|p| s.contains(&format!(" {} ", p)))
}

/// Detecta se a string contém alguma frase de afirmação.
fn tem_afirmacao(s: &str) -> bool {
    const AFF: [&str; 7] = [
        "esta disponivel", "estao disponivel", "disponiveis",
        "voltou", "voltaram", "temos sim", "tem sim",
    ];
    AFF.iter().any(|p| s.contains(&format!(" {} ", p)))
}

/// Diz se dois fatos curtos se contradizem.
pub fn contradiz(a: &str, b: &str) -> bool {
    if a.chars().count() > 320 || b.chars().count() > 320 {
        return false;
    }

    let na = normaliza(a);
    let nb = normaliza(b);

    // palavras >4 caracteres, sem duplicação
    let set_a: HashSet<&str> = na.split_whitespace().filter(|w| w.len() > 4).collect();
    let set_b: HashSet<&str> = nb.split_whitespace().filter(|w| w.len() > 4).collect();

    if set_a.intersection(&set_b).count() < 3 {
        return false;
    }

    let a_neg = tem_negacao(&na);
    let b_neg = tem_negacao(&nb);
    let a_aff = tem_afirmacao(&na);
    let b_aff = tem_afirmacao(&nb);

    (a_neg && b_aff) || (a_aff && b_neg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contradiz_true() {
        assert_eq!(
            contradiz(
                "A esfiha de brócolis da loja central acabou hoje",
                "A esfiha de brócolis da loja central voltou hoje"
            ),
            true
        );
    }

    #[test]
    fn test_contradiz_true_inverso() {
        assert_eq!(
            contradiz(
                "A esfiha de brócolis da loja central voltou hoje",
                "A esfiha de brócolis da loja central acabou hoje"
            ),
            true
        );
    }

    #[test]
    fn test_contradiz_false_ambos_neg() {
        assert_eq!(
            contradiz(
                "O servidor principal está fora do ar",
                "O servidor principal está fora do ar desde ontem"
            ),
            false
        );
    }

    #[test]
    fn test_contradiz_false_poucas_comuns() {
        assert_eq!(contradiz("acabou o gás", "voltou a luz"), false);
    }

    #[test]
    fn test_contradiz_false_long_text() {
        let longo = format!("{} acabou hoje", "a".repeat(400));
        assert_eq!(
            contradiz(&longo, "A esfiha de brócolis da loja central voltou hoje"),
            false
        );
    }
}
