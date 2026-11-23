//! Busca a pedido + proatividade (braniac/busca_pedido.py): quando a familia pede para PROCURAR preco/lugar/telefone, o Jarvis avisa que
//! vai procurar, junta os lugares do Maps e os precos escritos nas paginas, e age conforme a proatividade (0-10):
//! 0-4 so informa; 5-7 PERGUNTA se manda mensagem aos lugares; 8-10 ja manda (so a celular). As respostas dos lugares voltam a quem pediu.
//! TRAVAS: nenhum preco inventado (so os escritos na pagina); no maximo 3 lugares por pedido; so numero de celular; o mesmo lugar nao e
//! contatado 2x em 24 h; a mensagem diz que e um assistente de IA e que NADA sera agendado; o Jarvis nao responde ao lugar nem marca nada.
//! IA, Maps, pesquisa na web e TODO envio entram como closure (`Mundo`). Nomes (dono, parentes, cidade padrao) vem de configuracao (`Cfg`).
//! De fora: a thread de segundo plano (`trabalho` e sincrono; quem chama poe em thread depois de mandar o `recado_imediato`).
#![allow(dead_code)]
use crate::busca_maps::whatsapp_de;
use crate::texto_pt::sem_acento;
use regex::Regex;
use serde_json::{json, Value};
use std::path::Path;

const VALIDADE_S: f64 = 24.0 * 3600.0;
pub const MAX_CONTATOS: usize = 3;
const PEDIDOS: &str = "busca_pedidos.json"; // oferta pendente por solicitante
const CONTATOS: &str = "busca_contatos.json"; // lugares contatados, pela chave dos ultimos 8 digitos

pub struct Cfg<'a> {
    /// nome do dono, para "vou avisar o Fulano"
    pub dono: &'a str,
    /// como o Jarvis se apresenta aos lugares por padrao: "do Fulano"
    pub de_dono: &'a str,
    /// (nome do contato sem acento e minusculo, "da Fulana"): parentes tratados por "tia"
    pub rotulos: &'a [(&'a str, &'a str)],
    pub cidade_padrao: &'a str,
}

/// Tudo que sai para fora. `enviar_numero(fone, texto)` devolve "✅..." quando a mensagem ao lugar foi.
pub struct Mundo<'a> {
    pub ia: &'a dyn Fn(&str, &str) -> Result<String, String>,
    pub lugares: &'a dyn Fn(&str, usize) -> Result<Vec<Value>, String>,
    pub pesquisar: &'a dyn Fn(&str, usize) -> Result<Vec<Value>, String>,
    pub enviar: &'a dyn Fn(&str),
    pub avisar: &'a dyn Fn(&str),
    pub enviar_numero: &'a dyn Fn(&str, &str) -> String,
}

fn re(p: &str) -> Regex {
    Regex::new(p).expect("regex fixa")
}

fn ini(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn junto(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn ler(dados: &Path, arq: &str) -> Value {
    std::fs::read_to_string(dados.join(arq)).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}))
}

fn gravar(dados: &Path, arq: &str, d: &Value) {
    let _ = std::fs::create_dir_all(dados);
    let _ = std::fs::write(dados.join(arq), d.to_string());
}

fn rotulo<'a>(cfg: &Cfg<'a>, nome: &str) -> Option<&'a str> {
    let n = sem_acento(nome);
    cfg.rotulos.iter().find(|r| r.0 == n).map(|r| r.1)
}

pub fn eh_pedido(texto: &str) -> bool {
    let n = texto.chars().count();
    25 < n
        && n < 1500
        && re(r"(?i)\b(?:procur\w*|pesquis\w*|busc\w*|acha\w*|descobr\w*|verifi\w*|ver\s+(?:pra|para)\s+mim|d[aá]\s+(?:pra|para)\s+(?:ver|achar))\b").is_match(texto)
        && re(r"(?i)\b(?:quanto|pre[cç]o\w*|valor\w*|custa\w*|onde|cl[ií]nica\w*|lugar\w*|endere[cç]o\w*|telefone\w*|loja\w*|farm[aá]cia\w*|laborat[oó]rio\w*|hospital\w*|consult[oó]rio\w*|exame\w*)\b").is_match(texto)
}

/// Erros comuns da transcricao de audio em termos de saude ("nossa magnetica").
fn corrigir(t: &str) -> String {
    let t = re(r"(?i)\b(?:nossa|a?\s*ressona\w*|resson\w*)\s+magn[eé]tica\b").replace_all(t, "ressonância magnética");
    re(r"(?i)\bressonancia\b").replace_all(&t, "ressonância").to_string()
}

/// Cidade citada no pedido ("em Serrana"); None se nao houver.
pub fn cidade_de(texto: &str) -> Option<String> {
    re(r"\b(?:em|de|na|no)\s+([A-ZÀ-Ý][\wà-ÿ]{3,}(?:\s+[A-ZÀ-Ý][\wà-ÿ]+)?)").captures(texto).map(|m| m[1].to_string())
}

/// Frase de busca limpa a partir do audio transcrito. A IA corrige a transcricao; sem IA (ou resposta fora do tamanho), um recorte simples.
pub fn consulta_de(texto: &str, cidade_padrao: &str, ia: &dyn Fn(&str, &str) -> Result<String, String>) -> String {
    let t = corrigir(&junto(texto));
    let sistema = format!(
        "Voce transforma o pedido falado (transcrito por audio, pode ter palavras erradas) numa consulta curta para o Google: servico/produto + cidade + 'preco' se pediu valor. \
         Devolva SO a consulta, sem aspas, ate 10 palavras, em portugues. Nao invente cidade nem servico que nao estejam no texto. Cidade citada: use ela. Sem cidade, use {cidade_padrao}."
    );
    if let Ok(r) = ia(&sistema, &ini(&t, 900)) {
        let r = r.replace(['"', '\n'], " ").trim_matches([' ', '.']).to_string();
        if (3..=120).contains(&r.chars().count()) {
            return r;
        }
    }
    let m = re(r"(?i)(ressonância magnética[^.?!]*|(?:exame|consulta|cl[ií]nica|farm[aá]cia|loja)[^.?!]*)").find(&t).map(|m| m.as_str().to_string());
    format!("{} preço", ini(&m.unwrap_or(t), 100)).trim().to_string()
}

/// So os precos ESCRITOS no texto ("R$ 450,00"), sem repetir.
pub fn precos(texto: &str, n: usize) -> Vec<String> {
    let mut vistos: Vec<String> = vec![];
    for m in re(r"R\$\s*(\d{1,3}(?:\.\d{3})*,\d{2}|\d+,\d{2})").captures_iter(texto) {
        let v = format!("R$ {}", &m[1]);
        if !vistos.contains(&v) {
            vistos.push(v);
        }
    }
    vistos.truncate(n);
    vistos
}

/// "ressonância magnética ombro direito preço Serrana" -> "ressonância magnética ombro direito".
pub fn servico_de(consulta: &str, cidade: Option<&str>) -> String {
    let mut s = re(r"(?i)\b(?:pre[cç]o|valor|quanto custa)\b").replace_all(consulta, "").to_string();
    if let Some(c) = cidade.filter(|c| !c.is_empty()) {
        s = re(&format!("(?i){}", regex::escape(c))).replace_all(&s, "").to_string();
    }
    junto(&s).trim_matches([' ', ',', '.', '-']).to_string()
}

pub fn mensagem_ao_lugar(servico: &str, solicitante: &str, cfg: &Cfg) -> String {
    format!(
        "Olá, tudo bem? Aqui é o Jarvis, assistente de IA {}. Estou pesquisando: qual o valor particular de {servico} e quais as datas disponíveis para realizar? Por enquanto é só uma consulta, nada será agendado agora. Muito obrigado!",
        rotulo(cfg, solicitante).unwrap_or(cfg.de_dono)
    )
}

fn chave(fone: &str) -> String {
    let d: Vec<char> = fone.chars().filter(char::is_ascii_digit).collect();
    d[d.len().saturating_sub(8)..].iter().collect()
}

fn zap_de(l: &Value) -> Option<String> {
    whatsapp_de(l["telefone"].as_str().unwrap_or(""))
}

/// Manda a pergunta (valor + datas) aos lugares com celular. Devolve os nomes contatados.
pub fn contatar(dados: &Path, solicitante: &str, lugares: &[Value], servico: &str, agora: f64, cfg: &Cfg, enviar_numero: &dyn Fn(&str, &str) -> String, avisar: &dyn Fn(&str)) -> Vec<String> {
    let mut contatos = ler(dados, CONTATOS);
    let mut feitos = vec![];
    for l in lugares {
        let Some(zap) = zap_de(l) else { continue };
        let k = chave(&zap);
        let recente = contatos.get(&k).is_some_and(|c| agora - c["ts"].as_f64().unwrap_or(0.0) < VALIDADE_S);
        if recente || feitos.len() >= MAX_CONTATOS {
            continue;
        }
        let nome = l["nome"].as_str().unwrap_or("").to_string();
        let r = enviar_numero(&zap, &mensagem_ao_lugar(servico, solicitante, cfg));
        if r.starts_with('✅') {
            contatos[&k] = json!({ "nome": nome, "fone": zap, "solicitante": solicitante, "servico": servico, "ts": agora });
            feitos.push(nome);
        } else {
            avisar(&format!("⚠️ Não consegui mandar mensagem para {nome} ({zap}): {}", ini(&r, 100)));
        }
    }
    gravar(dados, CONTATOS, &contatos);
    feitos
}

/// Mensagem recebida de um lugar que contatamos? Devolve (solicitante, texto para ele) ou None. Nao responde ao lugar: so repassa.
pub fn resposta_de_lugar(dados: &Path, nome_chat: &str, texto: &str, agora: f64, cfg: &Cfg) -> Option<(String, String)> {
    let contatos = ler(dados, CONTATOS);
    let (d, chat) = (chave(nome_chat), sem_acento(nome_chat));
    let achado = contatos.as_object()?.iter().filter(|(_, c)| agora - c["ts"].as_f64().unwrap_or(0.0) <= 7.0 * VALIDADE_S).filter(|(k, c)| {
        let nome_c = sem_acento(c["nome"].as_str().unwrap_or("").split(':').next().unwrap_or(""));
        (d.len() >= 8 && d == **k) || (nome_c.chars().count() >= 6 && chat.contains(&nome_c))
    });
    let c = achado.last()?.1;
    let solicitante = c["solicitante"].as_str().unwrap_or("").to_string();
    let abre = if rotulo(cfg, &solicitante).is_some() { "Tia, a" } else { "A" };
    let msg = format!("{abre} {} respondeu: «{}». Quer que eu responda alguma coisa? Não marco nada sem a sua confirmação.", c["nome"].as_str().unwrap_or(""), ini(&junto(texto), 500));
    Some((solicitante, msg))
}

/// Maps primeiro (nome, nota, endereco, telefone); depois precos das paginas (so os escritos nelas). Texto para o WhatsApp.
/// `fontes` = [{url, titulo, texto}]; `lugares` = saida de `busca_maps::interpretar`.
#[allow(clippy::too_many_arguments)]
pub fn montar_resposta(consulta: &str, fontes: &[Value], apelido: &str, cidade: Option<&str>, lugares: &[Value], final_: &str, cfg: &Cfg) -> String {
    let apelido = if apelido.is_empty() { String::new() } else { format!(", {apelido}") }; // sem apelido: frase sem vocativo
    if fontes.is_empty() && lugares.is_empty() {
        return format!("Não consegui achar nada sobre \"{consulta}\" agora{apelido}. Vou avisar o {} pra ele ver isso contigo.", cfg.dono);
    }
    let txt = |v: &Value, k: &str| v[k].as_str().unwrap_or("").to_string();
    let mut linhas = vec![];
    if !lugares.is_empty() {
        linhas.push(format!("Achei no Google Maps {} lugar(es) pra \"{consulta}\"{apelido}:", lugares.len()));
        for l in lugares.iter().take(4) {
            let det: Vec<String> = [if txt(l, "nota").is_empty() { String::new() } else { format!("nota {}", txt(l, "nota")) }, txt(l, "endereco")].into_iter().filter(|x| !x.is_empty()).collect();
            let mut s = format!("• {}", ini(&txt(l, "nome"), 70));
            if !det.is_empty() {
                s += &format!("\n  {}", det.join(" · "));
            }
            s += &format!("\n  📞 {}", if txt(l, "telefone").is_empty() { "sem telefone no Maps".to_string() } else { txt(l, "telefone") });
            if !txt(l, "url").is_empty() {
                s += &format!("\n  {}", txt(l, "url").split("/data=").next().unwrap_or(""));
            }
            linhas.push(s);
        }
    }
    let cidade = cidade.filter(|c| !c.is_empty());
    let cit = |f: &Value| cidade.is_some_and(|c| sem_acento(&txt(f, "texto")).contains(&sem_acento(c)));
    let mut ord: Vec<&Value> = fontes.iter().collect();
    ord.sort_by_key(|f| !cit(f));
    ord.truncate(3);
    if !ord.is_empty() {
        linhas.push(if lugares.is_empty() { format!("Achei {} página(s) pra \"{consulta}\"{apelido}:", ord.len()) } else { "Preços que aparecem em sites:".to_string() });
        for f in ord {
            let pr = precos(&txt(f, "texto"), 3);
            let local = match cidade {
                None => String::new(),
                Some(c) if cit(f) => format!(" (cita {c})"),
                Some(c) => format!(" (não vi {c} na página, pode ser de outra cidade)"),
            };
            let titulo = if txt(f, "titulo").is_empty() { txt(f, "url") } else { txt(f, "titulo") };
            linhas.push(format!("• {}{local}\n  {}{}", ini(&titulo, 60), txt(f, "url"), if pr.is_empty() { "\n  (sem preço na página)".to_string() } else { format!("\n  preços na página: {}", pr.join(", ")) }));
        }
    }
    linhas.push("Os valores são os dos sites e podem mudar; confirma por telefone antes de ir.".into());
    if !final_.is_empty() {
        linhas.push(final_.to_string());
    }
    linhas.join("\n")
}

/// Texto de fecho conforme a proatividade; no nivel 8+ ja contata, no 5-7 guarda a oferta e pergunta.
#[allow(clippy::too_many_arguments)]
pub fn final_por_nivel(dados: &Path, nivel: i64, solicitante: &str, servico: &str, lugares: &[Value], agora: f64, cfg: &Cfg, enviar_numero: &dyn Fn(&str, &str) -> String, avisar: &dyn Fn(&str)) -> String {
    if nivel <= 4 || lugares.is_empty() {
        return String::new();
    }
    let zaps: Vec<Value> = lugares.iter().filter(|l| zap_de(l).is_some()).take(MAX_CONTATOS).cloned().collect();
    if zaps.is_empty() {
        return format!("Os telefones que achei são fixos (não dá pra mandar WhatsApp); se quiser, o {} pode ligar pra perguntar valor e datas.", cfg.dono);
    }
    if nivel >= 8 {
        let feitos = contatar(dados, solicitante, &zaps, servico, agora, cfg, enviar_numero, avisar);
        if feitos.is_empty() {
            return String::new();
        }
        return format!("Já mandei mensagem pra {} perguntando o valor e as datas disponíveis. Não marquei nada; te aviso quando responderem.", feitos.join(", "));
    }
    let nomes: Vec<&str> = zaps.iter().map(|l| l["nome"].as_str().unwrap_or("")).collect();
    let pergunta = format!("Quer que eu mande mensagem pra {} perguntando o valor e as datas disponíveis? Só pergunto, não marco nada sem você confirmar.", nomes.join(", "));
    let mut d = ler(dados, PEDIDOS);
    d[sem_acento(solicitante)] = json!({ "servico": servico, "lugares": zaps, "ts": agora });
    gravar(dados, PEDIDOS, &d);
    pergunta
}

/// Resposta curta (sim/nao) a uma oferta pendente. Devolve o texto para quem pediu, ou None se nao ha oferta / nao e resposta.
/// A oferta vale 24 h e e consumida: nunca manda duas vezes.
pub fn confirmacao(dados: &Path, solicitante: &str, texto: &str, agora: f64, cfg: &Cfg, enviar_numero: &dyn Fn(&str, &str) -> String, avisar: &dyn Fn(&str)) -> Option<String> {
    let t = texto.trim();
    if t.chars().count() > 80 {
        return None;
    }
    let sim = re(r"(?i)^\W*(?:sim|pode(?:\s+sim)?|quero|claro|manda|fa[cç]a|ok\s+pode|pode\s+mandar|por\s+favor|isso)\b").is_match(t);
    let nao = re(r"(?i)^\W*(?:n[aã]o|deixa|deixe|depois)\b").is_match(t);
    let mut d = ler(dados, PEDIDOS);
    let k = sem_acento(solicitante);
    let o = d.get(&k).filter(|o| agora - o["ts"].as_f64().unwrap_or(0.0) <= VALIDADE_S && (sim || nao))?.clone();
    d.as_object_mut()?.remove(&k);
    gravar(dados, PEDIDOS, &d);
    if nao && !sim {
        return Some("Tudo bem, não mando nada. Se mudar de ideia é só falar. 💛".into());
    }
    let servico = o["servico"].as_str().unwrap_or("");
    let feitos = contatar(dados, solicitante, o["lugares"].as_array().map(Vec::as_slice).unwrap_or(&[]), servico, agora, cfg, enviar_numero, avisar);
    if feitos.is_empty() {
        return Some(format!("Não consegui mandar as mensagens agora. Vou avisar o {} pra ver isso.", cfg.dono));
    }
    avisar(&format!("📨 {solicitante} autorizou: mandei mensagem para {} (valor e datas de {servico}). Nada agendado.", feitos.join(", ")));
    Some(format!("Mandei mensagem pra {} perguntando o valor e as datas. Assim que responderem eu te conto. Não marquei nada.", feitos.join(", ")))
}

/// O recado imediato a quem pediu; a pesquisa (`trabalho`) roda depois, em segundo plano.
pub fn recado_imediato(nome: &str, cfg: &Cfg) -> &'static str {
    if rotulo(cfg, nome).is_some() {
        "Já tô procurando isso pra você, minha tia, e daqui a pouquinho te mando os links e os telefones. 🔎"
    } else {
        "Já tô procurando isso pra você e daqui a pouquinho te mando os links e os telefones. 🔎"
    }
}

/// A pesquisa inteira: consulta -> Maps -> paginas -> fecho pela proatividade -> `enviar` a quem pediu -> aviso ao dono.
/// Maps ou web fora do ar viram aviso ao dono e a resposta sai com o que deu.
pub fn trabalho(dados: &Path, nome: &str, texto: &str, nivel: i64, agora: f64, cfg: &Cfg, m: &Mundo) {
    let texto = corrigir(texto);
    let consulta = consulta_de(&texto, cfg.cidade_padrao, m.ia);
    let cidade = cidade_de(&texto).or_else(|| cidade_de(&consulta));
    let servico = servico_de(&consulta, cidade.as_deref());
    let lugares = (m.lugares)(format!("{servico} {}", cidade.as_deref().unwrap_or("")).trim(), 5).unwrap_or_else(|e| {
        (m.avisar)(&format!("⚠️ Google Maps falhou para {nome} (\"{consulta}\"): {}", ini(&e, 100)));
        vec![]
    });
    let fontes = (m.pesquisar)(&consulta, 3).unwrap_or_else(|e| {
        (m.avisar)(&format!("⚠️ Busca na web falhou para {nome} (\"{consulta}\"): {}", ini(&e, 100)));
        vec![]
    });
    let final_ = final_por_nivel(dados, nivel, nome, &servico, &lugares, agora, cfg, m.enviar_numero, m.avisar);
    // so quem esta nos rotulos e "tia": o vocativo nao vai para as outras pessoas
    let apelido = if rotulo(cfg, nome).is_some() { "tia" } else { "" };
    (m.enviar)(&montar_resposta(&consulta, &fontes, apelido, cidade.as_deref(), &lugares, &final_, cfg));
    (m.avisar)(&format!("🔎 {nome} pediu uma busca (\"{consulta}\"); respondi com {} lugar(es) e {} página(s).", lugares.len(), fontes.len()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::path::PathBuf;

    const AUDIO: &str = "Você não tem como procurar ver para mim quanto é o resto da nossa magnética do Ombro Direito em Serrana. Para ver se tem umas clínicas que façam mais de valores para mim.";
    const CFG: Cfg = Cfg { dono: "Ana", de_dono: "da Ana", rotulos: &[("mainha", "da Bia")], cidade_padrao: "Vila Azul" };
    const T0: f64 = 1_800_000_000.0;

    fn dados(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-busca_pedido-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn zap() -> Value {
        json!({"nome": "Clínica Zap", "url": "", "nota": "4,5", "endereco": "Rua A, 1", "telefone": "(11) 99876-5432", "site": false})
    }

    fn fixo() -> Value {
        json!({"nome": "Clínica Fixo", "url": "https://maps.exemplo.test/place/F/data=!4m7", "nota": null, "endereco": null, "telefone": "(11) 3043-0000", "site": true})
    }

    /// Roda o trabalho inteiro com o mundo falso. Devolve (resposta a quem pediu, mensagens aos lugares, avisos ao dono).
    fn rodar(d: &Path, nome: &str, nivel: i64, lugares: Vec<Value>) -> (String, Vec<(String, String)>, Vec<String>) {
        let (env, zaps, avisos) = (RefCell::new(vec![]), RefCell::new(vec![]), RefCell::new(vec![]));
        let m = Mundo {
            ia: &|_, _| Ok("ressonância magnética ombro direito preço Serrana".into()),
            lugares: &|q, n| {
                assert_eq!((q, n), ("ressonância magnética ombro direito Serrana", 5));
                Ok(lugares.clone())
            },
            pesquisar: &|_, _| Ok(vec![]),
            enviar: &|t| env.borrow_mut().push(t.to_string()),
            avisar: &|t| avisos.borrow_mut().push(t.to_string()),
            enviar_numero: &|f, t| {
                zaps.borrow_mut().push((f.to_string(), t.to_string()));
                "✅ ok".into()
            },
        };
        trabalho(d, nome, AUDIO, nivel, T0, &CFG, &m);
        assert_eq!(env.borrow().len(), 1);
        let resposta = env.borrow()[0].clone();
        (resposta, zaps.into_inner(), avisos.into_inner())
    }

    #[test]
    fn reconhece_pedido_de_busca_e_ignora_conversa() {
        assert!(eh_pedido(AUDIO));
        assert!(eh_pedido("Jarvis, verifique o valor da passagem aérea de Serrana a Vila Azul"));
        assert!(!eh_pedido("Hoje dormi bem, mas o braço dói muito e o pescoço também, tia cansada"));
        assert!(!eh_pedido("ok") && !eh_pedido("procura o preço") && !eh_pedido(&format!("procura o preço {}", "x".repeat(1500))));
        assert_eq!(cidade_de(AUDIO).as_deref(), Some("Serrana"));
        assert_eq!(cidade_de("quanto custa isso aqui"), None);
    }

    #[test]
    fn consulta_vem_da_ia_ou_de_um_recorte_e_o_servico_sai_limpo() {
        assert_eq!(consulta_de(AUDIO, "Vila Azul", &|s, t| {
            assert!(s.ends_with("Sem cidade, use Vila Azul.") && t.contains("ressonância magnética do Ombro"));
            Ok(" \"exame de ombro preço Serrana\".\n".into())
        }), "exame de ombro preço Serrana");
        let recorte = consulta_de(AUDIO, "Vila Azul", &|_, _| Err("sem IA".into()));
        assert_eq!(recorte, "ressonância magnética do Ombro Direito em Serrana preço");
        assert_eq!(consulta_de("quero uma farmácia aberta agora. obrigada", "Vila Azul", &|_, _| Ok("x".repeat(200))), "farmácia aberta agora preço");
        assert_eq!(servico_de("ressonância magnética ombro direito preço Serrana", Some("serrana")), "ressonância magnética ombro direito");
        assert_eq!(servico_de("Quanto custa, valor do exame.", None), "do exame");
    }

    #[test]
    fn resposta_traz_telefone_e_so_preco_que_esta_na_pagina() {
        let fontes = [json!({"url": "https://clinica.exemplo.test/r", "titulo": "Clínica X", "texto": "Ressonância ombro R$ 450,00 ou R$ 1.200,00 ou R$ 450,00"}),
                      json!({"url": "https://outra.exemplo.test/", "texto": "atendemos em Serrana, consulte valores"})];
        let r = montar_resposta("ressonância Serrana", &fontes, "tia", Some("Serrana"), &[zap(), fixo()], "", &CFG);
        assert!(r.starts_with("Achei no Google Maps 2 lugar(es) pra \"ressonância Serrana\", tia:\n• Clínica Zap\n  nota 4,5 · Rua A, 1\n  📞 (11) 99876-5432\n• Clínica Fixo\n  📞 (11) 3043-0000\n  https://maps.exemplo.test/place/F\nPreços que aparecem em sites:\n• https://outra.exemplo.test/ (cita Serrana)"), "{r}");
        assert!(r.contains("(sem preço na página)") && r.contains("preços na página: R$ 450,00, R$ 1.200,00") && r.matches("R$").count() == 2);
        assert!(r.contains("• Clínica X (não vi Serrana na página, pode ser de outra cidade)") && r.ends_with("confirma por telefone antes de ir."));
        assert_eq!(montar_resposta("x", &[], "", None, &[], "", &CFG), "Não consegui achar nada sobre \"x\" agora. Vou avisar o Ana pra ele ver isso contigo.");
        assert!(montar_resposta("x", &fontes[..1], "", None, &[], "fecho", &CFG).starts_with("Achei 1 página(s) pra \"x\":\n• Clínica X\n"));
        assert_eq!(precos("de R$ 10,00 a R$10,00, R$ 1.5 ou 20,00", 3), ["R$ 10,00"]);
    }

    #[test]
    fn so_parente_e_tia() {
        let d = dados("tia");
        let (mae, _, avisos) = rodar(&d, "Mainha", 3, vec![fixo()]);
        assert!(mae.contains(", tia:") && recado_imediato("mainha", &CFG).contains("minha tia"));
        assert!(avisos[0].contains("Mainha pediu uma busca") && avisos[0].contains("1 lugar(es) e 0 página(s)"));
        let (outra, _, _) = rodar(&d, "Carla Souza 🤍", 3, vec![fixo()]);
        assert!(!format!("{outra}{}", recado_imediato("Carla Souza 🤍", &CFG)).to_lowercase().contains("tia") && outra.contains("Achei no Google Maps"));
    }

    #[test]
    fn proatividade_baixa_so_informa() {
        let (texto, zaps, _) = rodar(&dados("baixa"), "mainha", 3, vec![zap(), fixo()]);
        assert!(!texto.contains("Quer que eu mande") && zaps.is_empty());
    }

    #[test]
    fn proatividade_neutra_pergunta_e_so_manda_depois_do_sim() {
        let d = dados("neutra");
        let (texto, zaps, _) = rodar(&d, "mainha", 5, vec![zap(), fixo()]);
        assert!(texto.contains("Quer que eu mande mensagem pra Clínica Zap perguntando") && zaps.is_empty()); // o fixo nao entra na oferta
        let (enviados, avisos) = (RefCell::new(vec![]), RefCell::new(vec![]));
        let enviar = |f: &str, t: &str| -> String {
            enviados.borrow_mut().push((f.to_string(), t.to_string()));
            "✅ ok".into()
        };
        let avisar = |t: &str| avisos.borrow_mut().push(t.to_string());
        for nao_e_resposta in ["tia, hoje chove muito aqui na cidade né", "Hoje foi bom", &"sim ".repeat(30)] {
            assert_eq!(confirmacao(&d, "mainha", nao_e_resposta, T0 + 60.0, &CFG, &enviar, &avisar), None);
        }
        assert_eq!(confirmacao(&d, "outra pessoa", "sim", T0 + 60.0, &CFG, &enviar, &avisar), None);
        let resp = confirmacao(&d, "mainha", "Sim, pode mandar", T0 + 60.0, &CFG, &enviar, &avisar).unwrap();
        assert!(resp.contains("Clínica Zap") && resp.contains("Não marquei nada"));
        let e = enviados.borrow();
        assert!(e.len() == 1 && e[0].0 == "+55 11 99876-5432" && e[0].1.contains("nada será agendado") && e[0].1.contains("assistente de IA da Bia"));
        assert!(avisos.borrow()[0].contains("mainha autorizou") && avisos.borrow()[0].contains("Nada agendado"));
        assert_eq!(confirmacao(&d, "mainha", "sim", T0 + 61.0, &CFG, &|_, _| panic!("oferta consumida: nao manda duas vezes"), &avisar), None);
    }

    #[test]
    fn resposta_nao_cancela_e_oferta_velha_nao_vale() {
        let d = dados("nao");
        let nunca = |_: &str, _: &str| -> String { panic!("nao manda") };
        rodar(&d, "mainha", 6, vec![zap()]);
        assert!(confirmacao(&d, "mainha", "Não, deixa", T0 + 5.0, &CFG, &nunca, &|_| {}).unwrap().contains("não mando nada"));
        rodar(&d, "mainha", 6, vec![zap()]);
        assert_eq!(confirmacao(&d, "mainha", "sim", T0 + VALIDADE_S + 1.0, &CFG, &nunca, &|_| {}), None);
    }

    #[test]
    fn proatividade_alta_ja_contata_sem_repetir_e_no_maximo_tres() {
        let d = dados("alta");
        let cel = |i: u32| json!({"nome": format!("Lugar {i}"), "telefone": format!("(11) 9000{i}-000{i}")});
        let (texto, zaps, _) = rodar(&d, "Carla", 9, vec![zap(), fixo(), cel(1), cel(2), cel(3)]);
        assert!(texto.contains("Já mandei mensagem pra Clínica Zap, Lugar 1, Lugar 2 perguntando") && zaps.len() == MAX_CONTATOS);
        assert!(zaps[0].1.contains("assistente de IA da Ana") && zaps.iter().all(|z| z.0.starts_with("+55 11 9")));
        // mesmo lugar nas ultimas 24 h: nao repete; depois de 24 h pode
        let ok = |_: &str, _: &str| -> String { "✅".into() };
        assert!(contatar(&d, "Carla", &[zap()], "ressonância", T0 + 100.0, &CFG, &ok, &|_| {}).is_empty());
        assert_eq!(contatar(&d, "Carla", &[zap(), fixo()], "ressonância", T0 + VALIDADE_S + 1.0, &CFG, &ok, &|_| {}), ["Clínica Zap"]);
        // so fixo: explica e nao manda nada
        let (texto, zaps, _) = rodar(&dados("fixo"), "Carla", 9, vec![fixo()]);
        assert!(texto.contains("Os telefones que achei são fixos") && zaps.is_empty());
    }

    #[test]
    fn envio_que_falha_avisa_o_dono_e_nao_conta_como_contatado() {
        let d = dados("falha");
        let avisos = RefCell::new(vec![]);
        let feitos = contatar(&d, "Carla", &[zap()], "exame", T0, &CFG, &|_, _| "não enviado".into(), &|t| avisos.borrow_mut().push(t.to_string()));
        assert!(feitos.is_empty() && avisos.borrow()[0].starts_with("⚠️ Não consegui mandar mensagem para Clínica Zap (+55 11 99876-5432): não enviado"));
        assert_eq!(contatar(&d, "Carla", &[zap()], "exame", T0 + 1.0, &CFG, &|_, _| "✅".into(), &|_| {}).len(), 1);
    }

    #[test]
    fn resposta_da_clinica_volta_para_quem_pediu_e_maps_fora_do_ar_avisa() {
        let d = dados("volta");
        rodar(&d, "mainha", 9, vec![zap()]);
        let (quem, msg) = resposta_de_lugar(&d, "+55 11 99876-5432", "Custa R$ 600,00.\n Temos vaga dia 12.", T0 + 3600.0, &CFG).unwrap();
        assert_eq!((quem.as_str(), msg.as_str()), ("mainha", "Tia, a Clínica Zap respondeu: «Custa R$ 600,00. Temos vaga dia 12.». Quer que eu responda alguma coisa? Não marco nada sem a sua confirmação."));
        assert!(resposta_de_lugar(&d, "Clinica Zap Atendimento", "oi", T0 + 3600.0, &CFG).is_some()); // pelo nome do chat
        assert_eq!(resposta_de_lugar(&d, "+55 11 90000-1111", "oi", T0 + 3600.0, &CFG), None);
        assert_eq!(resposta_de_lugar(&d, "+55 11 99876-5432", "oi", T0 + 8.0 * VALIDADE_S, &CFG), None); // contato de mais de 7 dias
        let (env, avisos) = (RefCell::new(vec![]), RefCell::new(vec![]));
        let m = Mundo { ia: &|_, _| Err("x".into()), lugares: &|_, _| Err("aba travou".into()), pesquisar: &|_, _| Err("sem rede".into()), enviar: &|t| env.borrow_mut().push(t.to_string()),
                        avisar: &|t| avisos.borrow_mut().push(t.to_string()), enviar_numero: &|_, _| panic!("nao manda") };
        trabalho(&dados("fora"), "Carla", AUDIO, 9, T0, &CFG, &m);
        assert!(env.borrow()[0].starts_with("Não consegui achar nada sobre") && avisos.borrow().len() == 3);
        assert!(avisos.borrow()[0].contains("Google Maps falhou para Carla") && avisos.borrow()[1].contains("Busca na web falhou"));
    }
}
