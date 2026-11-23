//! Criar imagem pelo conselho de IAs (braniac/criacao.py): pedido -> mini-reuniao do prompt (uma IA escreve, outra melhora) -> motores de
//! geracao na ordem, pulando o que falhar ou estourar o limite -> arquivo em dados/criacoes + registro. O que e RECUSADO: pedido com dado
//! sensivel (nao vai para IA da internet) e um segundo pedido enquanto o primeiro roda. Imagem de exemplo so entra se for imagem e < 12 MB.
//! De fora (closures): as IAs do conselho, os motores (API, site logado, local), a leitura LOCAL das imagens de exemplo, o envio do arquivo
//! e a regra de "sensivel" (a do roteador). Video saiu daqui: quem faz e o estudio3d.
#![allow(dead_code)]
use crate::tempo::Agora;
use regex::Regex;
use serde_json::json;
use std::path::{Path, PathBuf};

const EXT_IMG: [&str; 4] = ["jpg", "jpeg", "png", "webp"];
const MAX_REF_BYTES: u64 = 12_000_000;

fn re(p: &str) -> Regex {
    Regex::new(p).expect("regex fixa")
}

fn curto(t: &str, n: usize) -> String {
    t.chars().take(n).collect()
}

/// O site respondeu que acabou a cota de imagens do dia.
pub fn limite(texto: &str) -> bool {
    re(r"(?i)limite|limit reached|reached (?:your|the) (?:daily )?limit|too many|daily limit|upgrade|volte (?:mais )?tarde|try again (?:later|tomorrow)|attempt limit|excedeu").is_match(texto)
}

/// Tira cerca de codigo, o rotulo "Prompt:" e aspas; no maximo 1800 caracteres.
pub fn limpar_prompt(t: &str) -> String {
    let t = re(r"(?s)```.*?\n|```").replace_all(t, "");
    let t = re(r"(?i)^(?:prompt|improved prompt|final prompt)\s*[:\-]\s*").replace(t.trim(), "").trim().trim_matches(['"', '“', '”']).to_string();
    curto(&t, 1800)
}

/// Das imagens de exemplo (caminho, tamanho), as que valem: so imagem, menor que 12 MB, no maximo `maximo` (3).
pub fn referencias_validas(candidatas: &[(PathBuf, u64)], maximo: usize) -> Vec<PathBuf> {
    candidatas.iter().filter(|(p, n)| *n < MAX_REF_BYTES && p.extension().and_then(|e| e.to_str()).is_some_and(|e| EXT_IMG.contains(&e.to_lowercase().as_str())))
        .map(|(p, _)| p.clone()).take(maximo).collect()
}

/// Estilo de cada imagem de exemplo, lido LOCALMENTE (`descrever` = modelo de visao do PC). So o texto segue para as IAs da internet.
pub fn descrever_referencias(refs: &[(String, Vec<u8>)], descrever: &dyn Fn(&[u8], &str) -> Result<String, String>) -> Vec<String> {
    const INSTRUCAO: &str = "Descreva em portugues o ESTILO visual desta imagem de exemplo (cores, composicao, tipo de arte, elementos principais), em 3 frases.";
    refs.iter().filter_map(|(nome, dados)| descrever(dados, INSTRUCAO).ok().map(|d| format!("{nome}: {d}"))).collect()
}

/// 1o prompt: o ChatGPT; se falhar, o Grok. (prompt, quem escreveu).
pub fn prompt_inicial(pedido: &str, contexto: &str, perguntar: &dyn Fn(&str, &str) -> Result<String, String>) -> Result<(String, &'static str), String> {
    let base = format!("Voce e especialista em prompts para geradores de imagem/video. Escreva UM prompt detalhado, em INGLES, para este pedido do cliente: \"{pedido}\". {}\
        Descreva sujeito, cena, estilo, iluminacao, cores, composicao e formato. Nao peca texto escrito dentro da imagem. Responda SOMENTE com o prompt, sem explicacao.",
        if contexto.is_empty() { String::new() } else { format!("Imagens de exemplo que ele quer como referencia de estilo: {contexto}. ") });
    let mut erros = Vec::new();
    for ia in ["chatgpt", "grok"] {
        match perguntar(ia, &base).map(|r| limpar_prompt(&r)) {
            Ok(p) if p.chars().count() > 30 => return Ok((p, ia)),
            Ok(_) => {}
            Err(e) => erros.push(format!("{ia}: {}", curto(&e, 60))),
        }
    }
    Err(format!("nenhuma IA do conselho escreveu o prompt ({})", erros.join("; ")))
}

/// Outra IA melhora o prompt sem mudar a ideia do pedido. Falhou ou devolveu pouco: fica o prompt que ja havia.
pub fn melhorar_prompt(prompt: &str, pedido: &str, contexto: &str, melhorar: &dyn Fn(&str) -> Result<String, String>) -> (String, &'static str) {
    let ped = format!("Melhore este prompt de geracao de imagem para ficar o mais fiel e bonito possivel: deixe mais especifico (sujeito, estilo, luz, lente, cores, composicao), \
        sem mudar a ideia do pedido original do cliente (\"{pedido}\"). {}Responda SOMENTE com o prompt final em ingles.\n\nPROMPT ATUAL:\n{prompt}",
        if contexto.is_empty() { String::new() } else { format!("Referencias de estilo: {contexto}. ") });
    match melhorar(&ped).map(|r| limpar_prompt(&r)) {
        Ok(novo) if novo.chars().count() > 30 => (novo, "gemini"),
        _ => (prompt.to_string(), "(sem melhoria)"),
    }
}

/// Um motor de geracao: (nome, prompt -> bytes da imagem).
pub type Motor<'a> = (&'a str, &'a dyn Fn(&str) -> Result<Vec<u8>, String>);

pub struct Pedido<'a> {
    pub dados: &'a Path,
    pub agora: &'a Agora,
    /// A regra de dado sensivel do roteador (cartao, CPF, senha, dado real...).
    pub sensivel: &'a dyn Fn(&str) -> bool,
    /// (ia, pergunta) -> texto, para o 1o prompt.
    pub perguntar: &'a dyn Fn(&str, &str) -> Result<String, String>,
    pub melhorar: &'a dyn Fn(&str) -> Result<String, String>,
    /// Na ordem de preferencia; o seguinte so entra se o anterior falhar (cota, deslogado, recusou).
    pub motores: &'a [Motor<'a>],
    pub avisar: &'a dyn Fn(&str),
}

#[derive(Debug)]
pub struct Criacao {
    pub caminho: PathBuf,
    pub motor: String,
    pub prompt: String,
    pub reuniao: Vec<(String, String)>,
    pub falhas: Vec<String>,
}

/// Roda a reuniao e gera. `contexto` = descricoes das imagens de exemplo (ja lidas no PC). Err traz o motivo (e o prompt pronto, se houve).
pub fn criar(p: &Pedido, pedido: &str, contexto: &[String]) -> Result<Criacao, String> {
    if (p.sensivel)(pedido) {
        return Err("o pedido tem dado sensivel (cartao, CPF, senha...): nao mando para IA da internet".into());
    }
    let ctx = contexto.join(" | ");
    if !contexto.is_empty() {
        (p.avisar)(&format!("🖼 Li {} imagem(ns) de exemplo (modelo local, no seu PC).", contexto.len()));
    }
    let (p1, quem) = prompt_inicial(pedido, &ctx, p.perguntar)?;
    (p.avisar)(&format!("🗣 {quem} escreveu o 1º prompt. Agora o Gemini melhora."));
    let (p2, quem2) = melhorar_prompt(&p1, pedido, &ctx, p.melhorar);
    let reuniao = vec![(quem.to_string(), p1), (quem2.to_string(), p2.clone())];
    let mut falhas = Vec::new();
    for (nome, gerar) in p.motores {
        let img = match gerar(&p2) {
            Ok(i) if !i.is_empty() => i,
            r => {
                let e = r.err().unwrap_or_else(|| "imagem vazia".into());
                falhas.push(format!("{nome}: {}", curto(&e, 70)));
                (p.avisar)(&format!("↪ {nome} não deu ({}); tentando o próximo.", curto(&e, 50)));
                continue;
            }
        };
        let pasta = p.dados.join("criacoes");
        std::fs::create_dir_all(&pasta).map_err(|e| e.to_string())?;
        let base = p.agora.carimbo().replace('-', "_");
        let caminho = pasta.join(format!("{base}.{}", if img.starts_with(b"\x89PNG") { "png" } else { "jpg" }));
        std::fs::write(&caminho, &img).map_err(|e| e.to_string())?;
        let registro = json!({ "caminho": caminho.to_string_lossy(), "motor": nome, "prompt": p2, "reuniao": reuniao, "falhas": falhas, "pedido": pedido });
        let _ = std::fs::write(pasta.join(format!("{base}.json")), serde_json::to_string_pretty(&registro).unwrap_or_default());
        return Ok(Criacao { caminho, motor: nome.to_string(), prompt: p2, reuniao, falhas });
    }
    Err(format!("nenhum motor gerou a imagem ({}). Prompt pronto: {p2}", falhas.join("; ")))
}

/// Legenda do arquivo entregue ao dono.
pub fn legenda(c: &Criacao) -> String {
    format!("Imagem pronta (feita por {}). Prompt: {}", c.motor, curto(&c.prompt, 300))
}

/// Uma criacao por vez.
#[derive(Default)]
pub struct Estado {
    pub rodando: bool,
    pub pedido: String,
}

impl Estado {
    /// Ok(fala de "comecei") e marca como rodando; Err(fala) se ja ha outra criacao em curso. Quem liga roda `criar` e chama `terminar`.
    pub fn comecar(&mut self, pedido: &str) -> Result<&'static str, String> {
        if self.rodando {
            return Err(format!("Já estou criando outra coisa: {}. Aguarde terminar.", curto(&self.pedido, 60)));
        }
        (self.rodando, self.pedido) = (true, pedido.to_string());
        Ok("Comecei: o conselho (ChatGPT/Grok → Gemini) vai montar o melhor prompt e eu gero a imagem. Pode demorar; te aviso e mando no WhatsApp.")
    }

    /// Fim da criacao: libera a vez e devolve o aviso ao dono.
    pub fn terminar(&mut self, r: &Result<Criacao, String>) -> String {
        *self = Estado::default();
        match r {
            Ok(c) => format!("✅ Imagem criada por {} e enviada no seu WhatsApp.", c.motor),
            Err(e) => format!("⚠️ Não consegui criar: {}", curto(e, 300)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;
    use std::cell::RefCell;

    const BOM: &str = "A cinematic photo of a small red boat on a calm lake at sunrise, soft light";

    #[test]
    fn reuniao_gera_pula_motor_sem_cota_e_registra() {
        let d = std::env::temp_dir().join(format!("braniac-criacao-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let agora = Agora { data: Data::civil(2026, 11, 16).unwrap(), hora: 9, min: 5, seg: 7 };
        let (avisos, perguntas) = (RefCell::new(Vec::new()), RefCell::new(Vec::new()));
        let perguntar = |ia: &str, p: &str| {
            perguntas.borrow_mut().push(format!("{ia}|{p}"));
            if ia == "chatgpt" { Err("deslogado".to_string()) } else { Ok(format!("Prompt: \"{BOM}\"")) }
        };
        let melhorar = |_: &str| Ok(format!("```\n{BOM}, 35mm lens, golden hour\n```"));
        let (sem_cota, png, nunca) = (|_: &str| Err("limite de imagens do dia".to_string()), |_: &str| Ok(b"\x89PNG....".to_vec()), |_: &str| panic!("o motor seguinte nao entra"));
        let motores: [Motor; 3] = [("gemini", &sem_cota), ("chatgpt", &png), ("grok", &nunca)];
        let avisar = |t: &str| avisos.borrow_mut().push(t.to_string());
        let p = Pedido { dados: &d, agora: &agora, sensivel: &|t| t.contains("cpf"), perguntar: &perguntar, melhorar: &melhorar, motores: &motores, avisar: &avisar };
        let c = criar(&p, "um barco vermelho no lago", &["a.png: tons quentes".to_string()]).unwrap();
        assert!(c.motor == "chatgpt" && c.prompt.ends_with("golden hour") && c.falhas == ["gemini: limite de imagens do dia"]);
        assert!(c.reuniao[0] == ("grok".to_string(), BOM.to_string()) && c.reuniao[1].0 == "gemini"); // o ChatGPT falhou: o Grok escreveu
        assert!(c.caminho.ends_with("criacoes/20261116_090507.png") && std::fs::read_to_string(d.join("criacoes").join("20261116_090507.json")).unwrap().contains("barco vermelho"));
        assert!(perguntas.borrow()[1].contains("tons quentes") && avisos.borrow().iter().any(|a| a.contains("gemini não deu")) && legenda(&c).starts_with("Imagem pronta (feita por chatgpt)"));
        // pedido sensivel e barrado antes de qualquer IA
        let barra = Pedido { perguntar: &|_, _| panic!("nao pode ir a IA"), ..p };
        assert!(criar(&barra, "desenha meu cpf", &[]).unwrap_err().contains("sensivel"));
        // nenhum motor: o erro leva o prompt pronto; sem melhoria fica o 1o prompt
        let so_falha: [Motor; 1] = [("gemini", &sem_cota)];
        let ruim = Pedido { motores: &so_falha, melhorar: &|_| Ok("curto".into()), sensivel: &|_| false, perguntar: &perguntar, ..p };
        let e = criar(&ruim, "um barco", &[]).unwrap_err();
        assert!(e.contains("nenhum motor gerou") && e.ends_with(&format!("Prompt pronto: {BOM}")));
        let mudo = Pedido { perguntar: &|_, _| Ok("x".into()), sensivel: &|_| false, ..p };
        assert!(criar(&mudo, "um barco", &[]).unwrap_err().contains("nenhuma IA do conselho"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn uma_criacao_por_vez_limpeza_do_prompt_e_referencias() {
        let mut e = Estado::default();
        assert!(e.comecar("um gato astronauta").unwrap().starts_with("Comecei"));
        assert!(e.comecar("outro").unwrap_err().contains("um gato astronauta") && e.rodando);
        assert!(e.terminar(&Err("caiu".into())).starts_with("⚠️ Não consegui criar: caiu") && !e.rodando && e.comecar("outro").is_ok());
        assert_eq!(limpar_prompt("```text\nFinal prompt: \"a cat\"\n```"), "a cat");
        assert_eq!(limpar_prompt(&"x".repeat(3000)).len(), 1800);
        assert!(limite("You've reached your daily limit") && limite("Você excedeu o uso") && !limite("Here is your image"));
        let c = |n: &str, t: u64| (PathBuf::from(n), t);
        let v = referencias_validas(&[c("a.PNG", 10), c("b.exe", 10), c("c.jpg", 13_000_000), c("d.webp", 5), c("e.jpeg", 5), c("f.jpg", 5)], 3);
        assert_eq!(v, [PathBuf::from("a.PNG"), PathBuf::from("d.webp"), PathBuf::from("e.jpeg")]);
        let d = descrever_referencias(&[("a.png".into(), vec![1]), ("b.png".into(), vec![2])], &|b, _| if b[0] == 1 { Ok("aquarela".into()) } else { Err("x".into()) });
        assert_eq!(d, ["a.png: aquarela"]);
    }
}
