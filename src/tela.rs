//! Jarvis usando o notebook "como uma pessoa" (olha a tela, clica, digita): aqui ficam SO as travas e a leitura do plano que a IA devolve.
//! Travas: nao digita senha/cartao/CPF/codigo; tarefa que fala de senha, cartao de credito ou CAPTCHA nem comeca; janela de WhatsApp, banco,
//! login ou pagamento NAO vai para IA online (para ANTES da foto e avisa); atalhos perigosos barrados; a acao "perguntar" (enviar, pagar,
//! apagar, aceitar termos, instalar) devolve a pergunta ao dono em vez de agir; pedido de parada interrompe na hora; teto de passos.
//! Ficou de fora (closures em `Mundo`): foto da tela, a IA com visao, mouse/teclado, o sinal de parada (flag, arquivo data/tela_parar ou mouse
//! no canto) e o registro em data/tela.log. O texto que a IA le na tela e DADO: so vale o JSON de UMA acao, e ele passa por `validar`.
#![allow(dead_code)]
use crate::empresa_dev::{corta, rx};
use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

pub const MAX_PASSOS: usize = 40;
pub const TECLAS_BARRADAS: [&str; 6] = ["alt+f4", "win+r", "ctrl+alt+delete", "ctrl+shift+esc", "win+x", "ctrl+shift+delete"];
const VK: [(&str, u32); 20] = [("enter", 0x0D), ("tab", 0x09), ("esc", 0x1B), ("escape", 0x1B), ("backspace", 0x08), ("delete", 0x2E), ("space", 0x20), ("up", 0x26), ("down", 0x28), ("left", 0x25), ("right", 0x27),
    ("home", 0x24), ("end", 0x23), ("pageup", 0x21), ("pagedown", 0x22), ("ctrl", 0x11), ("shift", 0x10), ("alt", 0x12), ("win", 0x5B), ("f1", 0x70)];

/// Titulo de janela que pode ter dado pessoal ou financeiro: essa tela nao e fotografada para IA online.
pub fn janela_sensivel(titulo: &str) -> bool {
    static R: OnceLock<Regex> = OnceLock::new();
    rx(&R, r"(?i)whatsapp|banco|bradesco|ita[uú]|nubank|caixa econ|santander|inter\b|senha|password|login|entrar|gov\.br|paypal|cart[aã]o|pix|carteira|bitcoin|binance").is_match(titulo)
}

/// Texto que o Jarvis nunca digita.
pub fn texto_proibido(t: &str) -> bool {
    static R: OnceLock<Regex> = OnceLock::new();
    rx(&R, r"(?i)senha|password|cvv|cpf|token|cart[aã]o").is_match(t)
}

/// Tarefa que nem comeca: e com o dono.
pub fn objetivo_proibido(objetivo: &str) -> bool {
    static R: OnceLock<Regex> = OnceLock::new();
    rx(&R, r"(?i)senha|password|cvv|cart[aã]o de cr[eé]dito|captcha").is_match(objetivo)
}

/// Codigos de tecla (Windows) do atalho, na ordem de apertar. Atalho barrado ou tecla desconhecida = erro.
pub fn codigos_da_tecla(combo: &str) -> Result<Vec<u32>, String> {
    let partes: Vec<String> = combo.split('+').map(|p| p.trim().to_lowercase()).filter(|p| !p.is_empty()).collect();
    if partes.is_empty() || TECLAS_BARRADAS.contains(&partes.join("+").as_str()) {
        return Err(if partes.is_empty() { format!("tecla desconhecida: {combo}") } else { format!("atalho barrado: {combo}") });
    }
    partes.iter().map(|p| {
        let f = p.strip_prefix('f').and_then(|n| n.parse::<u32>().ok()).filter(|n| (1..=12).contains(n)).map(|n| 0x6F + n);
        let letra = Some(p.chars().collect::<Vec<_>>()).filter(|c| c.len() == 1).and_then(|c| c[0].to_uppercase().next()).filter(|c| c.is_ascii()).map(|c| c as u32);
        VK.iter().find(|(n, _)| n == p).map(|x| x.1).or(f).or(letra).ok_or_else(|| format!("tecla desconhecida: {combo}"))
    }).collect()
}

#[derive(Debug, Clone, PartialEq)]
pub enum Acao {
    /// Coordenadas de 0 a 1000 (0,0 = canto superior esquerdo).
    Clicar { x: f64, y: f64, duplo: bool, direito: bool },
    Digitar(String),
    /// Codigos de tecla ja conferidos.
    Tecla(Vec<u32>),
    Rolar { x: f64, y: f64, quanto: f64 },
    Esperar,
}

/// Coordenada 0-1000 da IA -> pixel da tela.
pub fn pixel(n: f64, tamanho: u32) -> i32 {
    (n.clamp(0.0, 1000.0) / 1000.0 * tamanho as f64) as i32
}

pub fn prompt(tarefa: &str, historico: &[String]) -> String {
    let feitos: Vec<&str> = historico.iter().rev().take(8).rev().map(String::as_str).collect();
    format!("Voce controla o notebook do dono para cumprir a TAREFA. Olhe a tela e escolha UMA proxima acao.\nTAREFA: {tarefa}\nJA FEITO: {}\nResponda SO um JSON, sem texto fora dele, num destes formatos (coordenadas x,y de 0 a 1000: 0,0 = canto superior esquerdo, 1000,1000 = inferior direito):\n{{\"acao\":\"clicar\",\"x\":500,\"y\":500,\"porque\":\"...\"}}  |  {{\"acao\":\"duplo_clique\",\"x\":..,\"y\":..}}  |  {{\"acao\":\"clicar_direito\",\"x\":..,\"y\":..}}\n{{\"acao\":\"digitar\",\"texto\":\"...\"}}  (digita no campo com foco; clique nele antes)  |  {{\"acao\":\"tecla\",\"combo\":\"enter\"}}  (ex.: ctrl+l, tab, esc)\n{{\"acao\":\"rolar\",\"x\":500,\"y\":500,\"quanto\":-3}}  (negativo = sobe)  |  {{\"acao\":\"esperar\"}}\n{{\"acao\":\"perguntar\",\"pergunta\":\"...\"}}  quando precisar de decisao do dono OU for enviar, pagar, apagar, aceitar termos, instalar, ou pedir senha/cartao/codigo/CAPTCHA\n{{\"acao\":\"fim\",\"resultado\":\"o que foi feito/encontrado, curto\"}}  quando a tarefa estiver cumprida (ou impossivel: diga por que)\nRegras: nunca digite senha, cartao, CPF ou codigo; nunca invente dados; se a tela nao mudou, tente outra coisa; ao terminar de ler algo, devolva em \"fim\".",
        if feitos.is_empty() { "nada ainda".to_string() } else { feitos.join("; ") })
}

/// O JSON da resposta da IA (do primeiro "{" ao ultimo "}").
pub fn plano(resposta: &str) -> Result<Value, String> {
    let (Some(i), Some(f)) = (resposta.find('{'), resposta.rfind('}')) else { return Err(format!("resposta sem JSON: {}", corta(resposta, 80))) };
    let bruto = resposta.get(i..=f).ok_or("resposta sem JSON")?;
    // quebra de linha crua dentro de texto (o Python aceitava com strict=False)
    serde_json::from_str(bruto).or_else(|_| serde_json::from_str(&bruto.replace(['\n', '\r', '\t'], " "))).map_err(|e| format!("JSON ilegivel: {e}"))
}

/// Confere a acao pedida pela IA antes de qualquer toque no mouse ou no teclado. "fim" e "perguntar" nao passam por aqui.
pub fn validar(a: &Value) -> Result<Acao, String> {
    let n = |k: &str, padrao: Option<f64>| a[k].as_f64().or(padrao).ok_or_else(|| format!("acao sem {k}"));
    let k = a["acao"].as_str().unwrap_or("");
    Ok(match k {
        "clicar" | "duplo_clique" | "clicar_direito" => Acao::Clicar { x: n("x", None)?, y: n("y", None)?, duplo: k == "duplo_clique", direito: k == "clicar_direito" },
        "digitar" => {
            let t = a["texto"].as_str().map(String::from).unwrap_or_else(|| if a["texto"].is_null() { String::new() } else { a["texto"].to_string() });
            if texto_proibido(&t) {
                return Err("nao digito senha/cartao/CPF/codigo".into());
            }
            Acao::Digitar(t)
        }
        "tecla" => Acao::Tecla(codigos_da_tecla(a["combo"].as_str().unwrap_or(""))?),
        "rolar" => Acao::Rolar { x: n("x", Some(500.0))?, y: n("y", Some(500.0))?, quanto: n("quanto", Some(-3.0))? },
        "esperar" => Acao::Esperar,
        outra => return Err(format!("acao desconhecida: {outra}")),
    })
}

pub struct Mundo<'a> {
    /// O dono mandou parar (flag, arquivo de parada ou mouse no canto superior esquerdo).
    pub parado: &'a dyn Fn() -> bool,
    pub janela_ativa: &'a dyn Fn() -> String,
    /// Foto da tela (JPEG). So e chamada depois de a janela passar pela trava.
    pub foto: &'a dyn Fn() -> Vec<u8>,
    /// (prompt, foto) -> resposta da IA com visao.
    pub decidir: &'a dyn Fn(&str, &[u8]) -> Result<String, String>,
    /// Faz a acao ja validada (mouse, teclado, espera).
    pub executar: &'a dyn Fn(&Acao) -> Result<(), String>,
    pub avisar: &'a dyn Fn(&str),
    pub esperar: &'a dyn Fn(f64),
    pub log: &'a dyn Fn(&str),
}

/// Cumpre `objetivo` mexendo na tela. Devolve o texto final (resultado, pergunta ao dono, ou motivo de ter parado).
pub fn tarefa(objetivo: &str, max_passos: usize, m: &Mundo) -> String {
    let objetivo = objetivo.trim();
    if objetivo.is_empty() {
        return "Diga o que fazer no notebook.".into();
    }
    if objetivo_proibido(objetivo) {
        return "Nao mexo com senha, cartao ou CAPTCHA: isso e com voce.".into();
    }
    let mut feitos: Vec<String> = vec![];
    let ultimos = |f: &[String], n: usize| f.iter().rev().take(n).rev().cloned().collect::<Vec<_>>().join("; ");
    (m.log)(&format!("INICIO: {objetivo}"));
    for passo in 1..=max_passos {
        if (m.parado)() {
            (m.log)("PARADO pelo dono");
            return format!("Parei, como pediu. {}", if feitos.is_empty() { String::new() } else { format!("Ate aqui: {}", ultimos(&feitos, 4)) });
        }
        let titulo = (m.janela_ativa)();
        if janela_sensivel(&titulo) {
            (m.log)(&format!("parou: janela sensivel '{titulo}'"));
            return format!("Parei: a janela '{titulo}' pode ter dado pessoal ou financeiro e eu nao mando essa tela para IA online. Continue voce ou me diga para seguir por outro caminho.");
        }
        let a = match (m.decidir)(&prompt(objetivo, &feitos), &(m.foto)()).and_then(|r| plano(&r)) {
            Ok(a) => a,
            Err(e) => {
                (m.log)(&format!("passo {passo}: erro ao decidir: {}", corta(&e, 100)));
                if passo > 2 && feitos.is_empty() {
                    return format!("Nao consegui enxergar a tela agora ({}). Tente de novo em instantes.", corta(&e, 80));
                }
                feitos.push("(nao entendi a tela, tentando de novo)".into());
                (m.esperar)(2.0);
                continue;
            }
        };
        (m.log)(&format!("passo {passo} [{}]: {}", corta(&titulo, 40), corta(&a.to_string(), 200)));
        let k = a["acao"].as_str().unwrap_or("");
        let campo = |c: &str| a[c].as_str().filter(|x| !x.is_empty());
        match k {
            "fim" => return campo("resultado").unwrap_or("Pronto.").to_string(),
            "perguntar" => return format!("Preciso de voce: {} (me diga e eu sigo)", campo("pergunta").unwrap_or("posso continuar?")),
            _ => {}
        }
        if let Err(e) = validar(&a).and_then(|acao| (m.executar)(&acao)) {
            feitos.push(format!("falhou {k}: {}", corta(&e, 60)));
            (m.log)(&format!("passo {passo}: falhou: {e}"));
            if e.contains("barrado") || e.contains("nao digito") {
                return format!("Parei: {e}.");
            }
            continue;
        }
        feitos.push(corta(format!("{k} {}", campo("porque").or(campo("texto")).or(campo("combo")).unwrap_or("")).trim(), 80));
        if passo % 5 == 0 {
            (m.avisar)(&format!("…ainda no notebook (passo {passo}): {}", feitos.last().map(String::as_str).unwrap_or("")));
        }
        (m.esperar)(1.2);
    }
    format!("Cheguei no limite de passos sem terminar. Ate aqui: {}", ultimos(&feitos, 5))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::RefCell;

    struct Falso {
        respostas: RefCell<Vec<Result<String, String>>>,
        titulo: &'static str,
        feito: RefCell<Vec<String>>,
        parar_no: usize,
    }

    impl Falso {
        fn novo(titulo: &'static str, respostas: Vec<Result<String, String>>) -> Self {
            Falso { respostas: RefCell::new(respostas), titulo, feito: RefCell::new(vec![]), parar_no: usize::MAX }
        }

        fn rodar(&self, objetivo: &str, max: usize) -> String {
            let nota = |x: String| self.feito.borrow_mut().push(x);
            let voltas = RefCell::new(0);
            tarefa(objetivo, max, &Mundo {
                parado: &|| {
                    *voltas.borrow_mut() += 1;
                    *voltas.borrow() >= self.parar_no
                },
                janela_ativa: &|| self.titulo.to_string(),
                foto: &|| {
                    nota("foto".into());
                    vec![1]
                },
                decidir: &|p, _| {
                    assert!(p.contains("TAREFA: ") && p.contains("JA FEITO: "));
                    let mut r = self.respostas.borrow_mut();
                    if r.len() > 1 { r.remove(0) } else { r[0].clone() }
                },
                executar: &|a| {
                    nota(format!("{a:?}"));
                    Ok(())
                },
                avisar: &|t| nota(format!("aviso {t}")),
                esperar: &|_| {},
                log: &|_| {},
            })
        }
    }

    fn ok(v: Value) -> Result<String, String> {
        Ok(format!("Claro! {v} pronto."))
    }

    #[test]
    fn segue_o_plano_ate_o_fim_e_pergunta_antes_de_agir_no_que_e_do_dono() {
        let f = Falso::novo("Bloco de Notas", vec![ok(json!({"acao": "clicar", "x": 500, "y": 250, "porque": "abrir o menu"})), ok(json!({"acao": "digitar", "texto": "lista de compras"})), ok(json!({"acao": "tecla", "combo": "Ctrl+S"})),
                                                    ok(json!({"acao": "fim", "resultado": "Salvei a lista."}))]);
        assert_eq!(f.rodar("escreve a lista de compras", 40), "Salvei a lista.");
        assert_eq!(*f.feito.borrow(), vec!["foto", "Clicar { x: 500.0, y: 250.0, duplo: false, direito: false }", "foto", "Digitar(\"lista de compras\")", "foto", "Tecla([17, 83])", "foto"]);
        let p = Falso::novo("Loja", vec![ok(json!({"acao": "perguntar", "pergunta": "Posso finalizar a compra?"}))]);
        assert_eq!(p.rodar("ve o preco do teclado", 40), "Preciso de voce: Posso finalizar a compra? (me diga e eu sigo)");
        assert_eq!(*p.feito.borrow(), vec!["foto"]); // nada foi clicado
    }

    #[test]
    fn janela_sensivel_nao_e_fotografada_e_tarefa_de_senha_nem_comeca() {
        for titulo in ["WhatsApp", "Internet Banking - Banco X", "Entrar na conta", "Carteira de Bitcoin", "Pagamento por Pix"] {
            let f = Falso::novo(titulo, vec![Err("nao pode chegar na IA".into())]);
            assert!(f.rodar("le a tela", 40).starts_with(&format!("Parei: a janela '{titulo}'")), "{titulo}");
            assert!(f.feito.borrow().is_empty(), "{titulo}"); // nem a foto foi tirada
        }
        assert!(!janela_sensivel("Bloco de Notas") && !janela_sensivel("Interior do Brasil - mapa"));
        let f = Falso::novo("Navegador", vec![ok(json!({"acao": "fim"}))]);
        for objetivo in ["digita minha senha do site", "resolve o CAPTCHA", "preenche o cartão de crédito"] {
            assert_eq!(f.rodar(objetivo, 40), "Nao mexo com senha, cartao ou CAPTCHA: isso e com voce.");
        }
        assert_eq!(f.rodar("  ", 40), "Diga o que fazer no notebook.");
        assert!(f.feito.borrow().is_empty());
        assert_eq!(f.rodar("abre o navegador", 40), "Pronto.");
    }

    #[test]
    fn nao_digita_segredo_nem_aperta_atalho_barrado() {
        let d = Falso::novo("Site", vec![ok(json!({"acao": "digitar", "texto": "minha senha é abc"}))]);
        assert_eq!(d.rodar("preenche o cadastro", 40), "Parei: nao digito senha/cartao/CPF/codigo.");
        let t = Falso::novo("Site", vec![ok(json!({"acao": "tecla", "combo": "Alt + F4"}))]);
        assert_eq!(t.rodar("fecha a aba", 40), "Parei: atalho barrado: Alt + F4.");
        assert!(d.feito.borrow().iter().chain(t.feito.borrow().iter()).all(|x| x == "foto"));
        for barrado in TECLAS_BARRADAS {
            assert!(codigos_da_tecla(barrado).unwrap_err().contains("barrado"));
        }
        assert_eq!((codigos_da_tecla("ctrl+l"), codigos_da_tecla("F12"), codigos_da_tecla(" enter ")), (Ok(vec![0x11, 0x4C]), Ok(vec![0x7B]), Ok(vec![0x0D])));
        assert!(codigos_da_tecla("ctrl+banana").is_err() && codigos_da_tecla("").is_err() && codigos_da_tecla("f13").is_err());
        for (a, erro) in [(json!({"acao": "formatar_disco"}), "acao desconhecida"), (json!({"acao": "clicar", "x": 5}), "acao sem y"), (json!({"acao": "digitar", "texto": "CPF 000"}), "nao digito")] {
            assert!(validar(&a).unwrap_err().contains(erro));
        }
        assert_eq!(validar(&json!({"acao": "rolar"})), Ok(Acao::Rolar { x: 500.0, y: 500.0, quanto: -3.0 }));
        assert_eq!(validar(&json!({"acao": "clicar_direito", "x": 1, "y": 2})), Ok(Acao::Clicar { x: 1.0, y: 2.0, duplo: false, direito: true }));
        assert_eq!((pixel(500.0, 1920), pixel(5000.0, 1080), pixel(-3.0, 1080)), (960, 1080, 0));
    }

    #[test]
    fn para_quando_o_dono_manda_tem_teto_de_passos_e_ia_cega_desiste() {
        let mut f = Falso::novo("Editor", vec![ok(json!({"acao": "esperar"}))]);
        f.parar_no = 3;
        assert_eq!(f.rodar("espera carregar", 40), "Parei, como pediu. Ate aqui: esperar; esperar");
        let teto = Falso::novo("Editor", vec![ok(json!({"acao": "rolar", "porque": "descer"}))]);
        assert!(teto.rodar("le tudo", 6).starts_with("Cheguei no limite de passos sem terminar. Ate aqui: rolar descer; rolar descer"));
        assert_eq!(teto.feito.borrow().iter().filter(|x| x.starts_with("Rolar")).count(), 6);
        assert_eq!(teto.feito.borrow().iter().filter(|x| x.starts_with("aviso")).count(), 1); // a cada 5 passos
        let cega = Falso::novo("Editor", vec![Err("visao fora do ar".into())]);
        // igual ao Python: como cada falha entra no historico, a frase "Nao consegui enxergar a tela" nunca sai; termina pelo teto de passos
        assert_eq!(cega.rodar("le a tela", 3), format!("Cheguei no limite de passos sem terminar. Ate aqui: {}", ["(nao entendi a tela, tentando de novo)"; 3].join("; ")));
        assert!(cega.feito.borrow().iter().all(|x| x == "foto"));
        let torta = Falso::novo("Editor", vec![Ok("nao sei".into()), Ok("{\"acao\": \"fim\", \"resultado\": \"linha 1\nlinha 2\"}".into())]);
        assert_eq!(torta.rodar("le a tela", 40), "linha 1 linha 2");
        assert!(plano("sem chaves").is_err() && plano("} ao contrario {").is_err());
        assert!(prompt("x", &[]).contains("JA FEITO: nada ainda") && prompt("x", &["a".into(), "b".into()]).contains("JA FEITO: a; b\n") && !prompt("x", &[]).to_lowercase().contains("dono"));
    }
}
