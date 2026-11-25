//! Voz do dono (braniac/voz_do_usuarios.py): estatisticas LOCAIS do jeito como o dono escreve (tamanho das frases, giria, perguntas,
//! pontuacao, emoji) viram um guia curto anexado ao prompt do Jarvis. No maximo 1x por semana (e com 30+ mensagens) o conselho de IAs
//! transforma uma descricao ABSTRATA (so numeros e tracos) num guia melhor. Privacidade: nenhuma frase do dono sai do PC; a pergunta
//! ao conselho e montada so a partir das estatisticas. O conselho entra por closure. O nome do dono vem de configuracao (parametro).
#![allow(dead_code)]
use regex::Regex;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const MIN_AMOSTRAS: i64 = 30;
pub const CADA_S: f64 = 7.0 * 86400.0;
const GIRIAS: [&str; 20] = ["né", "tá", "pra", "vc", "bora", "cara", "beleza", "blz", "valeu", "vlw", "opa", "então", "mano", "poha", "uai", "kkk", "rapidão", "rapido", "show", "massa"];
const FORMAL: [&str; 8] = ["por favor", "obrigado", "obrigada", "bom dia", "boa tarde", "boa noite", "agradeço", "gostaria"];

fn re(p: &str) -> Regex {
    Regex::new(p).expect("regex fixa")
}

fn pasta(dados: &Path) -> PathBuf {
    dados.join("alma")
}

/// O que o dono disse (dados/alma/estilo_amostras.jsonl), sem o que e texto de maquina anexado.
pub fn amostras(dados: &Path) -> Vec<String> {
    let lixo = re("AVALIACAO-AUTOMATICA|Conversa recente|RESULTADO DE FERRAMENTA");
    std::fs::read_to_string(pasta(dados).join("estilo_amostras.jsonl")).unwrap_or_default().lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|v| v["texto"].as_str().map(|t| t.trim().to_string()))
        .filter(|t| !t.is_empty() && !lixo.is_match(t)).collect()
}

fn r2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// Pura. Tracos do jeito de falar: so numeros e listas de palavras comuns. Value::Null sem textos.
pub fn estatisticas(textos: &[String]) -> Value {
    if textos.is_empty() {
        return Value::Null;
    }
    let n = textos.len() as f64;
    let palavra = re(r"[\wÀ-ÿ']+");
    let (mut total, mut cont): (usize, Vec<(String, usize)>) = (0, Vec::new());
    for t in textos {
        for w in palavra.find_iter(&t.to_lowercase()) {
            total += 1;
            if GIRIAS.contains(&w.as_str()) {
                match cont.iter_mut().find(|(g, _)| g == w.as_str()) {
                    Some(c) => c.1 += 1,
                    None => cont.push((w.as_str().to_string(), 1)),
                }
            }
        }
    }
    cont.sort_by(|a, b| b.1.cmp(&a.1)); // estavel: empate fica na ordem em que apareceu
    let baixo = textos.join(" ").to_lowercase();
    let fracao = |f: &dyn Fn(&str) -> bool| r2(textos.iter().filter(|t| f(t)).count() as f64 / n);
    let (emoji, fim) = (re(r"[\x{1F300}-\x{1FAFF}☀-➿]"), re(r"[.!?…]$"));
    let imperativo = re(r"(?i)^(?:jarvis\W*)?(?:me\s+)?(?:manda|mande|faz|faça|cria|crie|abre|vê|ve|coloca|poe|bota|olha|pesquisa|mostra)\b");
    json!({ "amostras": textos.len(), "palavras_por_mensagem": (total as f64 / n * 10.0).round() / 10.0,
        "pergunta": fracao(&|t| t.contains('?')), "exclamacao": fracao(&|t| t.contains('!')), "emoji": fracao(&|t| emoji.is_match(t)),
        "comeca_minuscula": fracao(&|t| t.chars().next().is_some_and(char::is_lowercase)), "termina_sem_ponto": fracao(&|t| !fim.is_match(t)),
        "girias": cont.iter().take(6).map(|c| c.0.clone()).collect::<Vec<_>>(), "formal": FORMAL.iter().filter(|f| baixo.contains(**f)).take(3).collect::<Vec<_>>(),
        "imperativo": fracao(&|t| imperativo.is_match(t)) })
}

fn f(e: &Value, k: &str) -> f64 {
    e[k].as_f64().unwrap_or(0.0)
}

fn lista(e: &Value, k: &str, n: usize) -> String {
    e[k].as_array().into_iter().flatten().filter_map(Value::as_str).take(n).collect::<Vec<_>>().join(", ")
}

/// Pura. Texto neutro (sem frases do dono) com os tracos, para o conselho transformar em guia.
pub fn descricao_abstrata(e: &Value) -> String {
    if e.is_null() {
        return String::new();
    }
    let p = |k: &str| format!("{}%", (f(e, k) * 100.0).round());
    let (girias, formal) = (lista(e, "girias", 6), lista(e, "formal", 3));
    format!("Pessoa que escreve mensagens de {:.1} palavras em média ({} mensagens analisadas). {} das mensagens são perguntas, {} têm exclamação, {} têm emoji; \
        {} começam em minúscula e {} terminam sem ponto final. {} são pedidos diretos no imperativo. {}{}",
        f(e, "palavras_por_mensagem"), e["amostras"], p("pergunta"), p("exclamacao"), p("emoji"), p("comeca_minuscula"), p("termina_sem_ponto"), p("imperativo"),
        if girias.is_empty() { "Quase não usa gírias. ".to_string() } else { format!("Gírias/expressões frequentes: {girias}. ") },
        if formal.is_empty() { "Raramente usa fórmulas de cortesia.".to_string() } else { format!("Fórmulas de cortesia usadas: {formal}.") })
}

/// Pura. Regras de voz derivadas das estatisticas, sem IA (o minimo que ja ajuda). Precisa de 8 mensagens.
pub fn guia_local(e: &Value, dono: &str) -> String {
    if e.is_null() || e["amostras"].as_i64().unwrap_or(0) < 8 {
        return String::new();
    }
    let girias = lista(e, "girias", 4);
    let mut r = vec![if f(e, "palavras_por_mensagem") <= 14.0 { "frases curtas e diretas" } else { "frases de tamanho médio, explicando o necessário" }.to_string(),
        if !girias.is_empty() || f(e, "comeca_minuscula") > 0.4 { "tom informal e natural" } else { "tom educado e claro" }.to_string()];
    if !girias.is_empty() {
        r.push(format!("pode usar expressões como {girias}"));
    }
    if f(e, "termina_sem_ponto") > 0.6 {
        r.push("sem ponto final nas frases curtas".into());
    }
    if f(e, "emoji") < 0.1 {
        r.push("quase sem emoji".into());
    }
    if f(e, "imperativo") > 0.3 {
        r.push("vai direto ao ponto: faz o que foi pedido e confirma em uma frase".into());
    }
    format!("Ao escrever ou responder em nome do {dono}: {}.", r.join("; "))
}

/// Trecho de prompt (ate 900 caracteres) que o roteador anexa ao sistema do Jarvis: o guia refinado, ou o local enquanto nao ha refinado.
pub fn guia(dados: &Path, dono: &str) -> String {
    let refinado = std::fs::read_to_string(pasta(dados).join("guia_de_voz.md")).unwrap_or_default().trim().to_string();
    if !refinado.is_empty() {
        return refinado.chars().take(700).collect::<String>().trim().to_string();
    }
    guia_local(&estatisticas(&amostras(dados)), dono).chars().take(900).collect()
}

/// A pergunta que vai ao conselho: feita SO da descricao estatistica.
pub fn pergunta(e: &Value) -> String {
    format!("Você é especialista em redação e persona de assistentes. Abaixo está uma descrição ESTATÍSTICA do jeito como uma pessoa escreve (sem nenhum texto dela). \
        Escreva um GUIA DE VOZ para um assistente de IA responder e redigir mensagens do jeito dela: no máximo 8 regras curtas e práticas (tom, tamanho, abertura, \
        fechamento, expressões, pontuação), em português do Brasil, sem inventar fatos sobre a pessoa. Responda só com as regras em lista.\n\nDESCRIÇÃO: {}", descricao_abstrata(e))
}

/// `consultar(ia, pergunta)` -> texto do conselho. No maximo 1x por semana e so com amostras suficientes. true se atualizou o guia.
pub fn refinar(dados: &Path, dono: &str, consultar: &dyn Fn(&str, &str) -> Result<String, String>, ts: f64, forcar: bool) -> bool {
    let e = estatisticas(&amostras(dados));
    let arq = pasta(dados).join("voz_estado.json");
    let mut est: Value = std::fs::read_to_string(&arq).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}));
    if !forcar && (e["amostras"].as_i64().unwrap_or(0) < MIN_AMOSTRAS || ts - est["ts"].as_f64().unwrap_or(0.0) < CADA_S) {
        return false;
    }
    let Ok(r) = consultar("chatgpt", &pergunta(&e)) else { return false };
    let (r, baixo) = (r.trim(), r.to_lowercase());
    if r.chars().count() < 80 || baixo.trim_start().starts_with("nao consegui") || baixo.contains("não consegui falar") {
        return false;
    }
    let _ = std::fs::create_dir_all(pasta(dados));
    let texto = format!("Guia de voz do {dono} (use ao responder a ele e ao redigir mensagens em nome dele):\n{}", crate::tratamento::alma_dono(&r.chars().take(1500).collect::<String>()));
    if std::fs::write(pasta(dados).join("guia_de_voz.md"), texto).is_err() {
        return false;
    }
    est["ts"] = json!(ts);
    est["amostras"] = e["amostras"].clone();
    let _ = std::fs::write(arq, est.to_string());
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn falas() -> Vec<String> {
        ["bora ver isso rapidão", "me manda o link do site", "tá, pode fazer", "vê o saldo pra mim", "cria a lista nova", "olha a agenda de hoje", "valeu cara",
            "coloca a meta de 900", "e aí jarvis, como tá o sistema?", "beleza, manda ver", "pra quando fica pronto?", "abre o painel pra mim"].repeat(3).iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn estatisticas_e_descricao_nao_vazam_as_frases() {
        let e = estatisticas(&falas());
        assert!(e["amostras"] == 36 && f(&e, "palavras_por_mensagem") < 8.0 && f(&e, "comeca_minuscula") > 0.9 && f(&e, "pergunta") == 0.17 && f(&e, "emoji") == 0.0);
        assert!(lista(&e, "girias", 6).contains("bora") && e["girias"][0] == "pra" && f(&e, "imperativo") > 0.3);
        let d = descricao_abstrata(&e);
        assert!(!d.contains("link do site") && !d.to_lowercase().contains("painel") && d.contains("bora") && d.contains("17% das mensagens são perguntas")); // so tracos
        assert!(estatisticas(&[]).is_null() && descricao_abstrata(&Value::Null).is_empty() && guia_local(&Value::Null, "Davi").is_empty());
        assert!(guia_local(&estatisticas(&falas()[..5]), "Davi").is_empty()); // menos de 8 mensagens: ainda nao diz nada
        let formal = estatisticas(&["Bom dia! Gostaria de saber o prazo, por favor. 🙂".to_string()]);
        assert!(formal["formal"] == json!(["por favor", "bom dia", "gostaria"]) && f(&formal, "emoji") == 1.0 && f(&formal, "termina_sem_ponto") == 1.0);
    }

    #[test]
    fn guia_local_e_refinamento_semanal_pelo_conselho() {
        let d = std::env::temp_dir().join(format!("braniac-voz_do_usuarios-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("alma")).unwrap();
        assert_eq!(guia(&d, "Davi"), "");
        let mut linhas: Vec<String> = falas().iter().map(|t| json!({ "texto": t }).to_string()).collect();
        linhas.push(json!({ "texto": "[Conversa recente] segredo anexado pela maquina" }).to_string());
        linhas.push("linha torta".into());
        std::fs::write(d.join("alma").join("estilo_amostras.jsonl"), linhas.join("\n")).unwrap();
        assert_eq!(amostras(&d).len(), 36);
        let g = guia(&d, "Davi");
        assert!(g.starts_with("Ao escrever ou responder em nome do Davi:") && g.contains("frases curtas") && g.contains("informal") && g.chars().count() <= 900);
        let pedidos = RefCell::new(Vec::new());
        let conselho = |ia: &str, p: &str| {
            pedidos.borrow_mut().push((ia.to_string(), p.to_string()));
            Ok("1. Frases curtas.\n2. Tom informal.\n3. Vai direto ao ponto e confirma em uma frase.\n4. Sem ponto final nas frases curtas.".to_string())
        };
        assert!(refinar(&d, "Davi", &conselho, 1_000_000.0, false));
        let (ia, p) = pedidos.borrow()[0].clone();
        assert!(ia == "chatgpt" && !p.contains("link do site") && !p.to_lowercase().contains("painel") && !p.contains("Davi")); // nenhuma frase nem nome vai ao conselho
        assert!(guia(&d, "Davi").contains("Vai direto ao ponto") && !guia(&d, "Davi").contains("Ao escrever"));
        assert!(!refinar(&d, "Davi", &|_, _| Ok("x".repeat(200)), 1_000_000.0 + 3600.0, false)); // 1x por semana
        assert!(!refinar(&d, "Davi", &|_, _| Ok("curto".into()), 1_000_000.0 + 8.0 * 86400.0, false)); // resposta ruim nao troca o guia
        assert!(!refinar(&d, "Davi", &|_, _| Err("fora".into()), 1_000_000.0 + 8.0 * 86400.0, false));
        assert!(guia(&d, "Davi").contains("Vai direto ao ponto"));
        assert!(refinar(&d, "Davi", &|_, _| Ok("y".repeat(200)), 1_000_000.0 + 8.0 * 86400.0, false));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn poucas_amostras_nao_vao_ao_conselho() {
        let d = std::env::temp_dir().join(format!("braniac-voz_do_usuarios-b-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("alma")).unwrap();
        std::fs::write(d.join("alma").join("estilo_amostras.jsonl"), json!({ "texto": "oi" }).to_string()).unwrap();
        assert!(!refinar(&d, "Davi", &|_, _| panic!("nao devia consultar"), 9e9, false));
        let _ = std::fs::remove_dir_all(&d);
    }
}
