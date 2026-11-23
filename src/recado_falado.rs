//! Recado falado (porte de braniac/recado_falado.py): em vez de "📩 Recado de X: «...»", o Jarvis conta o recado ao dono em audio,
//! em tom de conversa. O texto da pessoa e conteudo de mensagem: o resumo so pode ser feito por IA LOCAL, que entra como closure
//! (`resumir`); sem ela (ou resposta curta demais) sai um roteiro fixo com as palavras da pessoa.
//! Ficou de fora: a chamada ao modelo local (aqui so o prompt, em `prompt_resumo`). Nomes e rotulos ("sua mae") vem de quem chama.
#![allow(dead_code)]
use regex::Regex;
use std::sync::OnceLock;

pub const MAX_FALA: usize = 900; // ~1 min de audio

fn re(cel: &'static OnceLock<Regex>, padrao: &str) -> &'static Regex {
    cel.get_or_init(|| Regex::new(padrao).expect("regex fixa"))
}

/// Como o dono chama a pessoa: `rotulos` = [(nome no chat, "sua mãe")], montado de dados/pessoal.json. Os outros vao pelo nome.
pub fn quem(nome: &str, rotulos: &[(String, String)]) -> String {
    rotulos.iter().find(|(n, _)| n == nome).map_or(nome, |(_, r)| r.as_str()).to_string()
}

/// Link vira "um link", emoji e seta saem, espacos se juntam (texto para ser falado).
pub fn limpar(t: &str) -> String {
    static LINK: OnceLock<Regex> = OnceLock::new();
    static EMOJI: OnceLock<Regex> = OnceLock::new();
    let t = re(&LINK, r"https?://\S+").replace_all(t, "um link");
    let t = re(&EMOJI, r"[\x{1F000}-\x{1FFFF}\x{2600}-\x{27BF}\x{2190}-\x{21FF}]").replace_all(&t, "");
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn limpas(v: &[String]) -> Vec<String> {
    v.iter().map(|x| limpar(x)).filter(|x| !x.is_empty()).collect()
}

/// Pedido para a IA LOCAL resumir em 2 a 4 frases. Nunca mandar este texto a IA online.
pub fn prompt_resumo(dono: &str, rotulo: &str, falas: &[String], respostas: &[String]) -> String {
    let msgs = falas.iter().map(|f| format!("- {f}")).collect::<Vec<_>>().join("\n");
    let resp = if respostas.is_empty() { String::new() } else { format!("\nO Jarvis respondeu a ela(e): {}", respostas.join(" / ")) };
    format!("Você é o Jarvis, assistente do {dono}, contando a ele por áudio o que {rotulo} disse. Escreva SÓ a fala, em português do Brasil, \
simples, direta e carinhosa, em 2 a 4 frases curtas, sem listas, sem emojis, sem aspas. Conte o que a pessoa disse (como ela está, \
se pediu algo, se precisa de algo) e, se o Jarvis já respondeu, diga o que respondeu. Não invente nada além do que está abaixo. \
Não comece com saudação.\nO que {rotulo} disse:\n{msgs}{resp}")
}

/// Fala completa para o dono. `falas` = o que a pessoa escreveu/disse (literal); `resumir(rotulo, falas, respostas)` = IA local ou "".
pub fn roteiro(dono: &str, rotulo: &str, falas: &[String], respostas: &[String], resumir: &dyn Fn(&str, &[String], &[String]) -> String) -> String {
    let (falas, respostas) = (limpas(falas), limpas(respostas));
    let mut resumo = limpar(&resumir(rotulo, &falas, &respostas));
    if resumo.chars().count() < 15 {
        // sem IA local: conta as palavras dela, curtas
        let junto: String = falas.join(" ").chars().take(500).collect();
        let abre = if rotulo.starts_with("seu ") {
            "Ele disse assim: "
        } else if rotulo.starts_with("sua ") || rotulo.starts_with("a ") {
            "Ela disse assim: "
        } else {
            "A mensagem foi: "
        };
        resumo = format!("{abre}{junto}");
    }
    format!("Oi, {dono}! Tudo bom? Olha, {rotulo} entrou em contato. {resumo} Qualquer coisa, é só abrir o WhatsApp e olhar a conversa.").chars().take(MAX_FALA).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: &[&str]) -> Vec<String> {
        x.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn roteiro_conversa_e_resume() {
        let rotulos = vec![("mainha".to_string(), "sua mãe".to_string()), ("Zeca".to_string(), "seu pai".to_string())];
        let t = roteiro("Dono", &quem("mainha", &rotulos), &v(&["ainda tô com muita dor no braço"]), &v(&["Poxa, melhoras!"]),
            &|r, f, rs| {
                assert!(r == "sua mãe" && f.len() == 1 && rs.len() == 1);
                "Ela disse que ainda sente muita dor no braço.".into()
            });
        assert!(t.starts_with("Oi, Dono! Tudo bom? Olha, sua mãe entrou em contato.") && t.contains("dor no braço") && t.contains("WhatsApp"));
        // sem IA local: roteiro fixo, sem quebrar
        let t2 = roteiro("Dono", &quem("Zeca", &rotulos), &v(&["oi filho"]), &[], &|_, _, _| String::new());
        assert!(t2.contains("seu pai") && t2.contains("Ele disse assim: oi filho"));
        assert!(roteiro("Dono", "sua mãe", &v(&["oi"]), &[], &|_, _, _| "curto".into()).contains("Ela disse assim: oi"));
        assert!(roteiro("Dono", &quem("Bia", &rotulos), &v(&["oi"]), &[], &|_, _, _| String::new()).contains("Olha, Bia entrou em contato. A mensagem foi: oi"));
    }

    #[test]
    fn limpa_link_e_emoji_e_corta_a_fala() {
        assert_eq!(limpar("olha  https://x.y/z 🙏 → aqui\n ☀"), "olha um link aqui");
        let longa = roteiro("Dono", "Bia", &v(&[&"palavra ".repeat(200)]), &[], &|_, _, _| "resumo ".repeat(300));
        assert_eq!(longa.chars().count(), MAX_FALA);
        let p = prompt_resumo("Dono", "sua mãe", &v(&["a", "b"]), &v(&["r"]));
        assert!(p.contains("assistente do Dono") && p.ends_with("O que sua mãe disse:\n- a\n- b\nO Jarvis respondeu a ela(e): r"));
    }
}
