//! A fala e uma pergunta de consulta, e nao um pedido de acao? (Otto, NexoDev)
use crate::texto_pt::sem_acento;

/// Verifica se o texto é uma pergunta de consulta (não ação).
pub fn eh_consulta(texto: &str) -> bool {
    // 1. limpeza básica
    let t = texto.trim();
    if t.is_empty() || t.chars().count() >= 400 || t.starts_with('/') {
        return false;
    }

    // 2. normaliza (minusculas, sem acento, sem espaços nas pontas)
    let n = sem_acento(t);
    // separa por não alfanumérico ASCII
    let mut palavras: Vec<&str> = n
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect();

    // 3. ignora "jarvis" inicial
    if let Some(&"jarvis") = palavras.first() {
        palavras.remove(0);
    }

    // listas de palavras-chave
    const ACAO: &[&str] = &[
        "manda", "mand", "envia", "envie", "avisa", "avise", "abre", "abra", "cria", "crie",
        "liga", "ligue", "desliga", "desligue", "publica", "publique", "edita", "edite",
        "apaga", "apague", "gera", "gere", "toca", "toque", "pausa", "pause",
        "coloca", "ponha", "poe", "marca", "marque", "anota", "anote",
        "lembra", "lembre", "responde", "responda",
    ];
    const WHATS: &[&str] = &["whats", "whatsapp", "zap", "wpp", "telegram"];
    const PERGUNTA: &[&str] = &[
        "quanto", "quantos", "quanta", "quantas", "qual", "quais", "quando",
        "onde", "quem", "situacao", "status", "relatorio", "resumo",
        "andamento", "prazo", "prazos", "falta",
    ];
    const FRASE: &[&str] = &[
        "como está", "como anda", "como ta", "como vai", "como foi",
        "o que", "me fala", "me conta", "me diz", "me diga",
        "me mostra", "me passa", "tem algum", "ja chegou",
        "ja caiu", "ja entrou",
    ];

    // 3. verifica ação ou palavra de chat
    if let Some(&primeira) = palavras.first() {
        if ACAO.contains(&primeira) {
            return false;
        }
    }
    if palavras.iter().any(|p| WHATS.contains(p)) {
        return false;
    }

    // 4. checa presença de '?' ou palavras de pergunta
    if t.contains('?') {
        return true;
    }
    if palavras.iter().any(|p| PERGUNTA.contains(p)) {
        return true;
    }

    // 5. checa frases cercadas por espaços
    let lower = t.to_lowercase();
    let esp = format!(" {} ", lower);
    for &f in FRASE {
        let frase = format!(" {} ", f);
        if esp.contains(&frase) {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eh_consulta() {
        // positivos
        assert_eq!(eh_consulta("Como está a agenda"), true);
        assert_eq!(eh_consulta("quanto falta pra meta"), true);
        assert_eq!(eh_consulta("me conta as novidades de hoje"), true);
        assert_eq!(eh_consulta("Jarvis, qual o prazo"), true);
        assert_eq!(eh_consulta("tem reunião hoje?"), true);
        // negativos
        assert_eq!(eh_consulta(""), false);
        assert_eq!(eh_consulta("   "), false);
        assert_eq!(eh_consulta("/ajuda?"), false);
        assert_eq!(eh_consulta("Anota reunião amanhã às 10h?"), false);
        assert_eq!(
            eh_consulta("Jarvis, manda um recado: quando ele chega?"),
            false
        );
        assert_eq!(eh_consulta("tem mensagem nova no whatsapp?"), false);
        assert_eq!(eh_consulta("bom dia"), false);
        let long = format!("{}?", "a".repeat(400));
        assert_eq!(eh_consulta(&long), false);
    }
}
