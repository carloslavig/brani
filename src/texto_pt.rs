//! Texto em portugues sem acento e titulo de agenda sem assunto. Escrito pelo Otto (NexoDev, T067), revisado pelo Claude.

/// Minusculas, sem acento do portugues, sem espaco nas pontas.
pub fn sem_acento(texto: &str) -> String {
    let mut out = String::new();
    for ch in texto.trim().chars() {
        let mapped = match ch {
            'Á' | 'À' | 'Â' | 'Ã' | 'Ä' | 'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'É' | 'È' | 'Ê' | 'Ë' | 'é' | 'è' | 'ê' | 'ë' => 'e',
            'Í' | 'Ì' | 'Î' | 'Ï' | 'í' | 'ì' | 'î' | 'ï' => 'i',
            'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ö' | 'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'Ú' | 'Ù' | 'Û' | 'Ü' | 'ú' | 'ù' | 'û' | 'ü' => 'u',
            'Ç' | 'ç' => 'c',
            _ => ch.to_ascii_lowercase(),
        };
        out.push(mapped);
    }
    out
}

/// True para titulo sem assunto ("Lembrar", "Lembrar Dono", "tarefa"...): o item seria lixo.
pub fn titulo_vazio(titulo: &str) -> bool {
    let txt = sem_acento(titulo);
    let cleaned: String = txt.chars().map(|c| if c.is_ascii_lowercase() { c } else { ' ' }).collect();
    let stop_words = [
        "me", "lembre", "lembra", "lembrar", "lembrete", "dono", "de", "do", "da", "um", "uma", "o", "a",
        "para", "pra", "tarefa", "compromisso", "evento", "amanha", "hoje",
    ];
    cleaned.split_whitespace().filter(|w| !stop_words.contains(w)).map(str::len).sum::<usize>() < 3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sem_acento() {
        assert_eq!(sem_acento("  Amanhã às 9h, TERÇA "), "amanha as 9h, terca");
    }

    #[test]
    fn test_titulo_vazio() {
        for c in ["", "Lembrar", "Lembrar Dono", "lembrete", "Me lembra amanhã", "tarefa 1"] {
            assert!(titulo_vazio(c), "{c}");
        }
        for c in ["Dentista", "Lembrar de ligar pro Jr", "Reunião", "pão"] {
            assert!(!titulo_vazio(c), "{c}");
        }
    }
}
