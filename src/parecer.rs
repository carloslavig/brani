//! Le o parecer do revisor de respostas (Otto, NexoDev T074)
use serde_json::Value;

/// Extrai e valida o trecho inventado de um parecer em JSON.
///
/// Retorna `Some(trecho)` se:
///   - há um bloco JSON entre o primeiro `{` e o último `}`
///   - o JSON contém `"inventou": true`
///   - o campo `"trecho"` tem ≥ 8 caracteres após limpar aspas e espaços
///   - o trecho (ignoring case) está presente em `resposta`.
/// Caso contrário, devolve `None`.
pub fn ler_parecer(saida: &str, resposta: &str) -> Option<String> {
    // 1. localizar o bloco JSON
    let inicio = saida.find('{')?;
    let fim = saida.rfind('}')?;
    if fim < inicio {
        return None;
    }
    let json_str = &saida[inicio..=fim];

    // 2. parsear
    let v: Value = serde_json::from_str(json_str).ok()?;

    // 3. campo "inventou" = true
    if v.get("inventou")?.as_bool()? != true {
        return None;
    }

    // 4. extrair e limpar "trecho"
    let raw = v.get("trecho")?.as_str()?;
    let trecho = raw
        .trim()
        .trim_matches(|c| c == '"' || c == '«' || c == '»')
        .to_string();
    if trecho.chars().count() < 8 {
        return None;
    }

    // 5. checar presença (case‑insensitive)
    if !resposta
        .to_lowercase()
        .contains(&trecho.to_lowercase())
    {
        return None;
    }

    // 6. sucesso
    Some(trecho)
}

#[cfg(test)]
mod tests {
    use super::ler_parecer;

    #[test]
    fn casos() {
        let resposta = "A reunião é às 15h com o Paulo. Bom trabalho!";
        assert_eq!(
            ler_parecer(r#"{"inventou": true, "trecho": "às 15h com o Paulo"}"#, resposta),
            Some("às 15h com o Paulo".to_string())
        );
        assert_eq!(
            ler_parecer(
                "Claro! ```json\n{\"inventou\": true, \"trecho\": \"«A reunião é às 15h»\"}\n``` pronto",
                resposta
            ),
            Some("A reunião é às 15h".to_string())
        );
        assert_eq!(
            ler_parecer(r#"{"inventou": true, "trecho": "AS 15H COM O PAULO"}"#, resposta),
            None
        );
        assert_eq!(
            ler_parecer(r#"{"inventou": true, "trecho": "ÀS 15H COM O PAULO"}"#, resposta),
            Some("ÀS 15H COM O PAULO".to_string())
        );
        assert_eq!(ler_parecer(r#"{"inventou": false}"#, resposta), None);
        assert_eq!(
            ler_parecer(
                r#"{"inventou": "true", "trecho": "às 15h com o Paulo"}"#,
                resposta
            ),
            None
        );
        assert_eq!(
            ler_parecer(r#"{"inventou": true, "trecho": "15h"}"#, resposta),
            None
        );
        assert_eq!(
            ler_parecer(r#"{"inventou": true, "trecho": "o céu é verde hoje"}"#, resposta),
            None
        );
        assert_eq!(ler_parecer("nao sei", resposta), None);
        assert_eq!(ler_parecer("{quebrado", resposta), None);
    }
}
