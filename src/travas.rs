//! Travas da resposta final: resposta crua e numero inventado. Porte de braniac/router.py.
//! Escrito pelo Otto (NexoDev, T061), revisado pelo Claude.
use std::collections::HashSet;

pub fn numeros(texto: &str) -> Vec<String> {
    let mut res = Vec::new();
    let chars: Vec<(usize, char)> = texto.char_indices().collect();
    let mut i = 0;
    while i < chars.len() {
        let (_, c) = chars[i];
        if c.is_ascii_digit() {
            let start = chars[i].0;
            let mut j = i + 1;
            while j < chars.len() {
                let (_, ch) = chars[j];
                if ch.is_ascii_digit() || ch == '.' || ch == ',' {
                    j += 1;
                } else {
                    break;
                }
            }
            let end = chars[j - 1].0 + chars[j - 1].1.len_utf8();
            if texto[end - 1..end].chars().next().map_or(false, |ch| ch.is_ascii_digit()) {
                res.push(texto[start..end].to_string());
            }
            i = j;
        } else {
            i += 1;
        }
    }
    res
}

fn inteiro(s: &str) -> String {
    s.replace('.', "")
        .split(',')
        .next()
        .unwrap_or("")
        .to_string()
}

fn curtas_ok() -> HashSet<&'static str> {
    let mut set = HashSet::new();
    for w in ["sim", "nao", "não", "ok", "oi", "ola", "olá"] {
        set.insert(w);
    }
    set
}

pub fn resposta_crua(t: &str) -> bool {
    let t = t.trim();
    if t.contains("RESULTADO DE FERRAMENTA") || t.starts_with("ERRO na ferramenta") {
        return true;
    }
    let limpo: String = t
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric()) // com acento: "olá" e "não" sao respostas validas
        .collect();
    limpo.chars().count() < 4
        && !curtas_ok().contains(limpo.as_str())
        && (limpo.is_empty() || !limpo.chars().all(|c| c.is_ascii_digit())) // vazio nao e "so digitos"
}

/// A pessoa pediu uma CONTA (tem numero e operacao na pergunta): o resultado e um numero novo e legitimo.
pub fn eh_conta(texto: &str) -> bool {
    let t = format!(" {} ", texto.to_lowercase());
    let tem = |lista: &[&str]| lista.iter().any(|p| t.contains(p));
    // "mais" e "menos" aparecem em qualquer frase: so valem como operacao entre DOIS numeros
    t.chars().any(|c| c.is_ascii_digit())
        && (t.chars().any(|c| matches!(c, '+' | '*' | '×' | '÷' | '%' | '^' | '='))
            || tem(&[" por cento", " soma", " some ", " calcul", " multiplic", " convert", " porcentagem", " metade ", " dobro ", " média ", " media "])
            || (numeros(texto).len() >= 2 && tem(&[" vezes ", " mais ", " menos ", " dividido ", " quanto é ", " quanto e ", " quanto dá ", " quanto da "])))
}

pub fn numeros_inventados(resposta: &str, base: &str, agora: &[String]) -> Vec<String> {
    let mut ok: HashSet<String> = numeros(base).iter().map(|n| inteiro(n)).collect();
    for a in agora {
        ok.insert(a.clone());
    }
    let so_digitos: String = base
        .chars()
        .filter(|c| !c.is_ascii_whitespace() && *c != '.' && *c != ',')
        .collect();

    let mut fora = Vec::new();
    for n in numeros(resposta) {
        let i = inteiro(&n);
        if i.len() >= 2
            && !ok.contains(&i)
            && !ok.contains(&i.trim_start_matches('0').to_string())
            && !so_digitos.contains(&n.chars().filter(|c| *c != '.' && *c != ',').collect::<String>())
            && !fora.contains(&n)
        {
            fora.push(n);
        }
    }
    fora
}

pub fn sem_frases_com(resposta: &str, numeros: &[String]) -> String {
    let mut frases = Vec::new();
    let mut start = 0usize;
    let bytes = resposta.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let ch = bytes[i] as char;
        if ".!?".contains(ch) {
            let next = i + ch.len_utf8();
            if next < bytes.len() && (bytes[next] as char).is_whitespace() {
                let end = next;
                let frase = resposta[start..end].trim().to_string();
                frases.push(frase);
                let mut j = end;
                while j < bytes.len() && (bytes[j] as char).is_whitespace() {
                    j += 1;
                }
                start = j;
                i = j;
                continue;
            }
        }
        i += 1;
    }
    if start < bytes.len() {
        frases.push(resposta[start..].trim().to_string());
    }

    let filtrado: Vec<String> = frases
        .into_iter()
        .filter(|f| !numeros.iter().any(|n| f.contains(n)))
        .collect();

    if filtrado.is_empty() {
        "Não tenho esse número confirmado agora. Quer que eu confira?".to_string()
    } else {
        filtrado.join(" ").trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conta_e_pergunta_com_numero_e_operacao() {
        for sim in ["quanto é 12 vezes 12?", "Calcula 15% de 200", "2+2", "qual a metade de 90?"] {
            assert!(eh_conta(sim), "{sim}");
        }
        for nao in ["quanto custa a página?", "que horas é a reunião do dia 15?", "quanto falta pra meta", "me fala mais sobre o item 3"] {
            assert!(!eh_conta(nao), "{nao}");
        }
    }

    #[test]
    fn test_numeros() {
        assert_eq!(
            numeros("custa 1.600,00 e 5"),
            vec!["1.600,00".to_string(), "5".to_string()]
        );
    }

    #[test]
    fn numero_colado_no_texto_base_nao_e_inventado() {
        assert!(numeros_inventados("o codigo e 4567", "pedido AB-4.567 chegou", &[]).is_empty());
        assert_eq!(numeros_inventados("sao 99 itens", "pedido 4567", &[]), vec!["99"]);
    }

    #[test]
    fn test_resposta_crua() {
        assert!(resposta_crua(""));
        assert!(!resposta_crua("olá") && !resposta_crua("Não."));
        assert!(resposta_crua("ia"));
        assert!(!resposta_crua("ok"));
        assert!(!resposta_crua("42"));
        assert!(resposta_crua("RESULTADO DE FERRAMENTA: x"));
        assert!(resposta_crua("ERRO na ferramenta algo"));
    }

    #[test]
    fn test_numeros_inventados() {
        let base = "meta de 1.600,00";
        let resp = "sao 1600 reais e 35 dias";
        let out = numeros_inventados(resp, base, &[]);
        assert_eq!(out, vec!["35".to_string()]);
    }

    #[test]
    fn test_sem_frases_com() {
        let resp = "Hoje tem 10 unidades. Amanhã não tem.";
        let nums = vec!["10".to_string()];
        let out = sem_frases_com(resp, &nums);
        assert_eq!(out, "Amanhã não tem.");
        let vazio = sem_frases_com("Só 5.", &vec!["5".to_string()]);
        assert_eq!(
            vazio,
            "Não tenho esse número confirmado agora. Quer que eu confira?"
        );
    }
}
