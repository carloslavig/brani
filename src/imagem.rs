//! Imagens (braniac/imagem.py): LER (descrever/transcrever o que ha na foto) e CRIAR. Aqui ficam as regras: a leitura tenta o modelo LOCAL
//! primeiro (privado, sem cota) e so depois a nuvem; a instrucao proibe repetir numero de cartao, conta, CPF ou chave; imagem vazia ou grande
//! demais e recusada; modelo que falha descansa (15 min se foi cota, 2 min se foi outro erro); ao criar, no maximo 4 imagens de referencia e o
//! gerador gratis de reserva so entra quando NAO ha referencia (ele so cria, nao edita).
//! De fora (closures): o modelo de visao local, a API de visao/geracao e o gerador de reserva. Encolher foto grande (Pillow) ficou de fora.
#![allow(dead_code)]
use std::collections::HashMap;

pub const MODELOS_VISAO: [&str; 3] = ["gemini-flash-latest", "gemini-flash-lite-latest", "gemini-2.5-flash-lite"];
pub const MODELOS_GERAR: [&str; 3] = ["gemini-2.5-flash-image", "gemini-3.1-flash-image", "gemini-3.1-flash-lite-image"];
/// O 1o instalado vale: o primeiro le portugues e texto; o segundo e o basico (so entende pedido curto em ingles).
pub const MODELOS_LOCAIS: [&str; 2] = ["qwen2.5vl:3b", "moondream"];
pub const LIMITE_BYTES: usize = 6_000_000;
pub const MAX_REFERENCIAS: usize = 4;

pub const PROMPT_LER: &str = "Voce ajuda o atendimento de WhatsApp de um pequeno negocio no Brasil. Descreva esta imagem em portugues, objetivo e curto. \
Se houver TEXTO (print, cardapio, tabela de precos, taxa de entrega, endereco, recado), transcreva fielmente o texto e os valores. \
Se for foto de comida, produto, lugar ou pessoa, diga o que aparece em 1 ou 2 frases. \
Se parecer comprovante de pagamento, cartao, documento ou dado bancario, diga SO que e isso (tipo e valor total, se for comprovante) \
e NAO escreva numeros de cartao, conta, CPF ou chave. Nao invente nada que nao esteja visivel.";
pub const PROMPT_LOCAL: &str = "Describe this image in detail. Transcribe any text you see.";

pub fn mime(dados: &[u8]) -> &'static str {
    if dados.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if dados.starts_with(b"RIFF") && dados.get(8..12) == Some(b"WEBP") {
        "image/webp"
    } else {
        "image/jpeg"
    }
}

/// (modelo local a usar, instrucao que ele entende, prefixo da resposta). None = nenhum modelo de visao instalado.
pub fn modelo_local(instalados: &[String], instrucao: &str) -> Option<(&'static str, String, &'static str)> {
    let m = MODELOS_LOCAIS.into_iter().find(|m| instalados.iter().any(|i| i == m || *i == format!("{m}:latest")))?;
    if m.starts_with("qwen") { Some((m, instrucao.to_string(), "")) } else { Some((m, PROMPT_LOCAL.to_string(), "(leitura local, aproximada, em inglês) ")) }
}

/// Modelos em descanso: depois de falhar, um modelo so volta a ser tentado quando o prazo passa.
#[derive(Default)]
pub struct Descanso(HashMap<String, f64>);

impl Descanso {
    pub fn descansando(&self, modelo: &str, ts: f64) -> bool {
        self.0.get(modelo).is_some_and(|ate| *ate > ts)
    }

    /// HTTP 429 (cota) = 15 min; outro erro = 2 min.
    pub fn falhou(&mut self, modelo: &str, status: u16, ts: f64) {
        self.0.insert(modelo.to_string(), ts + if status == 429 { 900.0 } else { 120.0 });
    }
}

/// Tenta cada modelo da lista, pulando os que descansam. `chamar(modelo)` devolve Ok(resultado) ou Err(status HTTP; 0 = rede, 200 = veio vazio).
pub fn por_modelos<T>(modelos: &[&str], descanso: &mut Descanso, ts: f64, chamar: &dyn Fn(&str) -> Result<T, u16>) -> Result<(T, String), String> {
    let mut erros = Vec::new();
    for m in modelos {
        if descanso.descansando(m, ts) {
            erros.push(format!("{m}: em descanso"));
            continue;
        }
        match chamar(m) {
            Ok(r) => return Ok((r, format!("gemini:{m}"))),
            Err(0) => erros.push(format!("{m}: rede")),
            Err(200) => erros.push(format!("{m}: resposta vazia")),
            Err(s) => {
                descanso.falhou(m, s, ts);
                erros.push(format!("{m}: HTTP {s}"));
            }
        }
    }
    Err(erros.join("; "))
}

/// Bytes da imagem -> texto. `local` e `nuvem` recebem (imagem, instrucao). Local primeiro quando `local_primeiro` (o padrao do dono).
pub fn descrever(dados: &[u8], instrucao: Option<&str>, local_primeiro: bool, local: &dyn Fn(&[u8], &str) -> Result<String, String>, nuvem: &dyn Fn(&[u8], &str) -> Result<String, String>) -> Result<String, String> {
    if dados.is_empty() {
        return Err("imagem vazia".into());
    }
    if dados.len() > LIMITE_BYTES {
        return Err("imagem grande demais".into());
    }
    let instrucao = instrucao.unwrap_or(PROMPT_LER);
    let ordem = if local_primeiro { [local, nuvem] } else { [nuvem, local] };
    let mut erros = Vec::new();
    for f in ordem {
        match f(dados, instrucao) {
            Ok(t) if !t.trim().is_empty() => return Ok(t.trim().to_string()),
            Ok(_) => erros.push("resposta vazia".to_string()),
            Err(e) => erros.push(e),
        }
    }
    Err(erros.join(" | "))
}

/// Prompt (e ate 4 imagens de referencia) -> bytes. `gerador(prompt, referencias)`; `reserva(prompt)` e o gerador gratis, que so entra sem referencia.
pub fn gerar(prompt: &str, referencias: &[Vec<u8>], gerador: &dyn Fn(&str, &[Vec<u8>]) -> Result<Vec<u8>, String>, reserva: Option<&dyn Fn(&str) -> Result<Vec<u8>, String>>) -> Result<Vec<u8>, String> {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err("sem descricao da imagem".into());
    }
    let refs = &referencias[..referencias.len().min(MAX_REFERENCIAS)];
    let mut erros = match gerador(prompt, refs) {
        Ok(i) if !i.is_empty() => return Ok(i),
        Ok(_) => vec!["sem imagem na resposta (recusou?)".to_string()],
        Err(e) => vec![e],
    };
    if let (Some(r), true) = (reserva, refs.is_empty()) {
        match r(&prompt.chars().take(900).collect::<String>()) {
            Ok(i) => return Ok(i),
            Err(e) => erros.push(e),
        }
    }
    Err(erros.join("; "))
}

/// A resposta do gerador de reserva so vale se for imagem de verdade.
pub fn reserva_valida(status: u16, tipo: &str, bytes: usize) -> bool {
    status == 200 && tipo.starts_with("image/") && bytes >= 2000
}

/// Nome do arquivo salvo em dados/imagens.
pub fn nome_arquivo(carimbo: &str, dados: &[u8]) -> String {
    format!("{}.{}", carimbo.replace('-', "_"), if mime(dados) == "image/png" { "png" } else { "jpg" })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn ler_local_primeiro_recusa_vazia_e_grande_e_nao_repete_numeros() {
        let ordem = RefCell::new(Vec::new());
        let local = |_: &[u8], i: &str| {
            ordem.borrow_mut().push(format!("local:{}", i.len()));
            Err("ollama sem modelo de visao".to_string())
        };
        let nuvem = |_: &[u8], _: &str| {
            ordem.borrow_mut().push("nuvem".into());
            Ok(" um cardapio ".to_string())
        };
        assert_eq!(descrever(b"img", None, true, &local, &nuvem).unwrap(), "um cardapio");
        assert_eq!(*ordem.borrow(), [format!("local:{}", PROMPT_LER.len()), "nuvem".to_string()]);
        assert_eq!(descrever(b"img", Some("x"), true, &local, &|_, _| Err("sem chave".into())).unwrap_err(), "ollama sem modelo de visao | sem chave");
        assert_eq!(descrever(b"", None, true, &|_, _| panic!("vazia"), &nuvem).unwrap_err(), "imagem vazia");
        assert_eq!(descrever(&vec![0; LIMITE_BYTES + 1], None, true, &|_, _| panic!("grande"), &nuvem).unwrap_err(), "imagem grande demais");
        assert!(PROMPT_LER.contains("NAO escreva numeros de cartao, conta, CPF ou chave") && PROMPT_LER.contains("Nao invente"));
        assert_eq!((mime(b"\x89PNG\r\n\x1a\nxx"), mime(b"RIFFxxxxWEBPxx"), mime(b"\xff\xd8"), mime(b"")), ("image/png", "image/webp", "image/jpeg", "image/jpeg"));
        let inst = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(modelo_local(&inst(&["moondream:latest", "llama3"]), "leia").unwrap(), ("moondream", PROMPT_LOCAL.to_string(), "(leitura local, aproximada, em inglês) "));
        assert_eq!(modelo_local(&inst(&["moondream", "qwen2.5vl:3b"]), "leia").unwrap(), ("qwen2.5vl:3b", "leia".to_string(), ""));
        assert!(modelo_local(&inst(&["llama3"]), "leia").is_none());
    }

    #[test]
    fn modelo_sem_cota_descansa_e_o_seguinte_responde() {
        let mut d = Descanso::default();
        let chamar = |m: &str| match m {
            "a" => Err(429),
            "b" => Err(500),
            "c" => Err(0),
            "d" => Err(200),
            _ => Ok(m.to_uppercase()),
        };
        assert_eq!(por_modelos(&["a", "b", "c", "d", "e"], &mut d, 1000.0, &chamar).unwrap(), ("E".to_string(), "gemini:e".to_string()));
        assert!(d.descansando("a", 1000.0 + 899.0) && !d.descansando("a", 1901.0) && d.descansando("b", 1100.0) && !d.descansando("b", 1121.0) && !d.descansando("c", 1000.0));
        let e = por_modelos::<String>(&["a", "b", "c", "d"], &mut d, 1010.0, &|m| if m == "a" || m == "b" { panic!("em descanso nao chama") } else { chamar(m) }).unwrap_err();
        assert_eq!(e, "a: em descanso; b: em descanso; c: rede; d: resposta vazia");
    }

    #[test]
    fn criar_limita_referencias_e_a_reserva_gratis_so_entra_sem_referencia() {
        let vistas = RefCell::new(0);
        let recusa = |_: &str, r: &[Vec<u8>]| {
            *vistas.borrow_mut() = r.len();
            Err("HTTP 429".to_string())
        };
        let reserva = |p: &str| Ok(p.as_bytes().to_vec());
        assert_eq!(gerar(" um gato ", &[], &recusa, Some(&reserva)).unwrap(), b"um gato");
        let seis = vec![vec![1u8]; 6];
        assert_eq!(gerar("um gato", &seis, &recusa, Some(&|_| panic!("com referencia a reserva nao entra"))).unwrap_err(), "HTTP 429");
        assert_eq!(*vistas.borrow(), 4);
        assert_eq!(gerar("  ", &[], &|_, _| panic!("sem descricao"), None).unwrap_err(), "sem descricao da imagem");
        assert_eq!(gerar("x", &[], &|_, _| Ok(vec![]), Some(&|_| Err("pollinations: HTTP 500".into()))).unwrap_err(), "sem imagem na resposta (recusou?); pollinations: HTTP 500");
        assert_eq!(gerar("x", &[], &|_, _| Ok(vec![9]), None).unwrap(), [9]);
        assert!(reserva_valida(200, "image/jpeg", 5000) && !reserva_valida(200, "text/html", 5000) && !reserva_valida(200, "image/jpeg", 100) && !reserva_valida(502, "image/jpeg", 5000));
        assert_eq!((nome_arquivo("20261116-090507", b"\x89PNG\r\n\x1a\n"), nome_arquivo("20261116-090507", b"x")), ("20261116_090507.png".to_string(), "20261116_090507.jpg".to_string()));
    }
}
