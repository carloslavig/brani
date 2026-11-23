//! O que o agente NUNCA faz sozinho numa pagina, porque e sempre a pessoa: pagar, comprar, transferir,
//! apagar conta, entrar em conta e digitar segredo. Na duvida, recusa.
//! Escrito pelo Otto (NexoDev, T064), revisado e ajustado pelo Claude.

/// Normaliza texto: minúsculo, remove acentos/cedilha e reduz espaços.
pub fn normal(s: &str) -> String {
    // substituição simples de caracteres acentuados comuns
    fn replace_accent(c: char) -> char {
        match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' | 'Á' | 'À' | 'Â' | 'Ã' | 'Ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' | 'Í' | 'Ì' | 'Î' | 'Ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' | 'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' | 'Ú' | 'Ù' | 'Û' | 'Ü' => 'u',
            'ç' | 'Ç' => 'c',
            _ => c,
        }
    }

    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for c in s.chars() {
        let lc = replace_accent(c.to_ascii_lowercase());
        if lc.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(lc);
            prev_space = false;
        }
    }
    out.trim().to_string()
}

/// Verifica se a URL aponta para um site de banco ou pagamento.
pub fn site_proibido(url: &str) -> Option<&'static str> {
    // extrai host entre "://" e próximo delimitador
    fn host_part(u: &str) -> Option<&str> {
        let after_scheme = u.split("://").nth(1)?;
        let host_end = after_scheme
            .find(&['/', '?', '#'][..])
            .unwrap_or(after_scheme.len());
        let host = &after_scheme[..host_end];
        // remove credenciais "user@" se houver
        let host = host.rsplit_once('@').map(|(_, h)| h).unwrap_or(host);
        // remove porta ":port" se houver
        let host = host.split(':').next().unwrap_or(host);
        Some(host)
    }

    let host = host_part(url)?;
    let norm = normal(host);
    const BANCO: &[&str] = &[
        "banco", "bank", "nubank", "itau", "bradesco", "santander", "caixa.gov",
        "bb.com.br", "inter.co", "c6bank", "infinitepay", "mercadopago",
        "pagseguro", "pagbank", "picpay", "paypal", "stripe", "wise.com",
        "binance", "gov.br",
    ];
    for p in BANCO {
        if norm.contains(p) {
            return Some("site de banco ou pagamento: ai so a pessoa mexe");
        }
    }
    None
}

/// Verifica se o rótulo de botão/link é proibido.
pub fn clique_proibido(rotulo: &str) -> Option<&'static str> {
    let norm = normal(rotulo);
    let palavras: Vec<&str> = norm
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();

    // palavras que só são bloqueadas como termo inteiro
    const PALAVRAS_INTEIRAS: &[(&[&str], &str)] = &[
        (&["pix", "pagar", "pague", "pago", "doar", "doe", "buy", "pay", "donate", "donation"], "pagamento ou compra: isso e sempre a pessoa"),
    ];

    // termos genéricos (substrings)
    const TERMO_SUB: &[(&[&str], &str)] = &[
        (
            &[
                "pagamento", "comprar", "compre agora", "finalizar pedido",
                "finalizar compra", "fechar pedido", "confirmar pedido", "assinar",
                "assine", "transferir", "transferencia", "doacao", "checkout",
                "place order", "purchase", "subscribe",
            ],
            "pagamento ou compra: isso e sempre a pessoa",
        ),
        (
            &[
                "excluir conta", "apagar conta", "deletar conta", "encerrar conta",
                "delete account", "excluir tudo", "apagar tudo",
            ],
            "apagar conta ou dados: isso e sempre a pessoa",
        ),
    ];

    // checagem de palavras inteiras
    for (list, msg) in PALAVRAS_INTEIRAS {
        for &pal in *list {
            if palavras.iter().any(|&w| w == pal) {
                return Some(msg);
            }
        }
    }

    // checagem de termos que podem aparecer como substring
    for (list, msg) in TERMO_SUB {
        for &term in *list {
            if norm.contains(term) {
                return Some(msg);
            }
        }
    }

    // correspondências exatas (rótulo inteiro)
    const EXATO: &[(&[&str], &str)] = &[(
        &[
            "entrar", "login", "log in", "sign in", "fazer login",
            "acessar conta", "criar conta", "cadastrar", "cadastre-se",
            "sign up", "registrar", "create account", "register",
        ],
        "entrar ou criar conta: isso e sempre a pessoa",
    )];
    for (list, msg) in EXATO {
        for &term in *list {
            if norm == term {
                return Some(msg);
            }
        }
    }

    None
}

/// Verifica se um campo de formulário deve ser protegido.
pub fn campo_proibido(tipo: &str, rotulo: &str) -> Option<&'static str> {
    let tipo_norm = normal(tipo);
    if tipo_norm == "password" {
        return Some("campo de segredo: isso e sempre a pessoa");
    }
    let rot_norm = normal(rotulo);
    const PALAVRAS: &[&str] = &[
        "senha", "password", "cvv", "cvc", "codigo de seguranca",
        "codigo de verificacao", "numero do cartao", "card number",
        "cpf", "token",
    ];
    for &p in PALAVRAS {
        if rot_norm.contains(p) {
            return Some("campo de segredo: isso e sempre a pessoa");
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal() {
        assert_eq!(normal("  AÇÃO   São "), "acao sao");
    }

    #[test]
    fn test_site_proibido() {
        assert!(site_proibido("https://app.nubank.com.br/x").is_some());
        assert!(site_proibido("https://user@www.itau.com.br:443/a").is_some());
        assert!(site_proibido("https://exemplo.com/banco").is_none());
        assert!(site_proibido("https://duckduckgo.com/?q=banco").is_none());
    }

    #[test]
    fn test_clique_proibido() {
        assert!(clique_proibido("Pagar agora").is_some());
        assert!(clique_proibido("Apagar").is_none()); // palavra completa "apagar" não está na lista
        assert!(clique_proibido("Pixel art").is_none());
        assert!(clique_proibido("Fazer PIX").is_some());
        assert!(clique_proibido("Entrar").is_some());
        assert!(clique_proibido("Entrar em contato").is_none());
        assert!(clique_proibido("Excluir conta").is_some());
        assert!(clique_proibido("Pesquisar").is_none());
        assert!(clique_proibido("Doar agora").is_some() && clique_proibido("Buy").is_some() && clique_proibido("Pague aqui").is_some());
        assert!(clique_proibido("Apagar conta").is_some() && clique_proibido("Compreender melhor").is_none());
        assert!(clique_proibido("Donate").is_some() && clique_proibido("Create account").is_some() && clique_proibido("Log in").is_some());
        assert!(clique_proibido("Adoar").is_none() && clique_proibido("Buyer guide").is_none());
    }

    #[test]
    fn test_campo_proibido() {
        assert!(campo_proibido("password", "").is_some());
        assert!(campo_proibido("text", "Digite sua senha").is_some());
        assert!(campo_proibido("text", "Buscar").is_none());
    }
}
