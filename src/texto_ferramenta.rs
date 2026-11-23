//! Ferramenta que a IA escreveu como texto em vez de usar o recurso de ferramentas. Porte de braniac/router.py.
//! Escrito pelo Otto (NexoDev, T062), revisado e ajustado pelo Claude.
use serde_json::{json, Value};

/// Tenta extrair a chamada de ferramenta a partir de um texto livre.
/// Retorna `(nome, argumentos)` onde `argumentos` é um `serde_json::Value` objeto.
pub fn chamada_em_texto(texto: &str, nomes_validos: Option<&[&str]>) -> Option<(String, Value)> {
    let t = texto.trim();
    // o texto INTEIRO tem de ser o JSON (com ou sem cerca): JSON no meio de uma frase e conversa, nao chamada
    let json_str = match t.strip_prefix("```") {
        Some(resto) => resto.strip_suffix("```")?.trim().trim_start_matches("json").trim(),
        None => t,
    };
    if !json_str.starts_with(['{', '[']) {
        return None;
    }
    let mut data: Value = serde_json::from_str(json_str).ok()?;
    // varias chamadas escritas como texto: fica com o primeiro objeto (o laco chama de novo se faltar)
    if let Value::Array(arr) = data {
        data = arr.into_iter().find(Value::is_object)?;
    }
    let obj = data.as_object()?;
    // como o "a or b" do Python: valor vazio (null, "", {}) conta como ausente
    let cheio = |v: &&Value| !(v.is_null() || v.as_str() == Some("") || v.as_object().is_some_and(|o| o.is_empty()));
    let primeiro = |chaves: &[&str]| chaves.iter().filter_map(|k| obj.get(*k)).find(cheio);
    let nome = match primeiro(&["tool", "name", "function", "tool_name"])? {
        Value::String(s) => s.clone(),
        Value::Object(m) => m.get("name")?.as_str()?.to_string(),
        _ => return None,
    };
    let args = match primeiro(&["arguments", "parameters", "args"]) {
        Some(v) => Value::Object(v.as_object()?.clone()),
        None => json!({}),
    };
    if nomes_validos.is_some_and(|lista| !lista.contains(&nome.as_str())) {
        return None;
    }
    Some((nome, args))
}

/// Heurística rápida para detectar se um texto parece ser a resposta JSON de uma ferramenta.
pub fn resposta_e_json_de_ferramenta(texto: &str) -> bool {
    let t = texto.trim();
    let first = t.chars().next();
    if first != Some('{') && first != Some('[') {
        return false;
    }
    let preview: String = t.chars().take(300).collect();
    if !preview.contains("\"arguments\"") {
        return false;
    }
    preview.contains("\"name\"") || preview.contains("\"tool\"") || preview.contains("\"function\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn simples_objeto() {
        let txt = r#"{"tool":"x","arguments":{"a":1}}"#;
        let res = chamada_em_texto(txt, None).unwrap();
        assert_eq!(res.0, "x");
        assert_eq!(res.1, json!({"a":1}));
    }

    #[test]
    fn dentro_fence_json() {
        let txt = "```json\n{\"name\":\"y\",\"arguments\":{}}\n```";
        let res = chamada_em_texto(txt, None).unwrap();
        assert_eq!(res.0, "y");
    }

    #[test]
    fn lista_pega_primeiro_objeto() {
        let txt = r#"[{"tool":"a","arguments":{}},{"tool":"b","arguments":{}}]"#;
        let res = chamada_em_texto(txt, None).unwrap();
        assert_eq!(res.0, "a");
    }

    #[test]
    fn function_com_nome_embutido() {
        let txt = r#"{"function":{"name":"z"},"arguments":{}}"#;
        let res = chamada_em_texto(txt, None).unwrap();
        assert_eq!(res.0, "z");
    }

    #[test]
    fn nome_fora_da_lista() {
        let txt = r#"{"tool":"ok","arguments":{}}"#;
        let res = chamada_em_texto(txt, Some(&["nao"]));
        assert!(res.is_none());
    }

    #[test]
    fn texto_com_ruido_falha() {
        let txt = "algum texto {\"name\":\"x\"}";
        assert!(chamada_em_texto(txt, None).is_none());
    }

    #[test]
    fn arguments_nao_objeto_falha() {
        let txt = r#"{"tool":"x","arguments":"texto"}"#;
        assert!(chamada_em_texto(txt, None).is_none());
    }

    #[test]
    fn cerca_com_sobra_e_valor_vazio() {
        assert!(chamada_em_texto("```json\n{\"tool\":\"x\"}\n``` e mais texto", None).is_none());
        let r = chamada_em_texto(r#"{"tool":"","name":"y","arguments":{},"args":{"a":1}}"#, None).unwrap();
        assert_eq!(r, ("y".to_string(), json!({"a":1})));
        assert_eq!(chamada_em_texto(r#"{"tool":"x"}"#, None).unwrap().1, json!({}));
    }

    #[test]
    fn resposta_true() {
        let txt = r#"{"name":"x","arguments":{}}"#;
        assert!(resposta_e_json_de_ferramenta(txt));
    }

    #[test]
    fn resposta_false() {
        let txt = r#"ola {"name":"x"}"#;
        assert!(!resposta_e_json_de_ferramenta(txt));
    }
}
