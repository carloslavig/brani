//! Reconciliacao das abas-agente da VM. Causa das dezenas de abas repetidas (24/11/2026): o Chrome da VM RESTAURA as abas da sessao
//! anterior com ids novos; o mapa id->agente (`vm_abas.json`) nao as conhecia, entao `abas_fixas` achava que faltavam e abria outras;
//! a cada reinicio dobrava. Aqui o tipo da aba vem da URL (host), nao so do mapa:
//! - aba ja aberta de um tipo gerenciado e REUSADA (adotada no mapa), nunca se abre outra;
//! - sobras do mesmo tipo sao fechadas (fica a mais saudavel); tipo desconhecido e aba de agente nao gerenciado (Pesquisa...) nunca sao tocados;
//! - aba em erro (chrome-error, "Aw, Snap", nao responde) e recarregada UMA vez (cooldown) ou recriada;
//! - com o CDP fora do ar nada e aberto: tenta ligar a VM (no maximo 1x a cada 10 min) e espera.
//! So abre a pagina: NUNCA faz login nem digita. Registro em `dados/abas_reconcilia.log` (so tipos e contagens, nunca texto de pagina).
use crate::abas_fixas;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Mutex;

static EM_CURSO: Mutex<()> = Mutex::new(());
const MAX_FECHAR: usize = 20;
const MAX_ABRIR: usize = 6;
const COOLDOWN_S: f64 = 900.0;

#[derive(Clone, Debug)]
pub struct Alvo {
    pub id: String,
    pub url: String,
    pub titulo: String,
    pub agente: String,
    pub erro: bool,
}

#[derive(Debug, Default, PartialEq)]
pub struct Plano {
    /// (id, agente): aba que ja existe e passa a ser a do agente
    pub adotar: Vec<(String, String)>,
    pub fechar: Vec<String>,
    /// (id, agente, url): aba escolhida mas em erro
    pub recarregar: Vec<(String, String, String)>,
    /// (agente, url): nao ha aba nenhuma do tipo
    pub faltam: Vec<(String, String)>,
}

fn host(url: &str) -> String {
    let r = url.split_once("://").map_or(url, |x| x.1);
    r.split(['/', '?', '#']).next().unwrap_or("").to_lowercase()
}

/// A aba esta no site daquele tipo? (host igual ou subdominio do host configurado, sem o "www.")
fn do_tipo(url_aba: &str, url_tipo: &str) -> bool {
    let (a, t) = (host(url_aba), host(url_tipo));
    let t = t.trim_start_matches("www.");
    !t.is_empty() && (a == t || a.ends_with(&format!(".{t}")))
}

pub fn pagina_de_erro(url: &str, titulo: &str) -> bool {
    let t = titulo.to_lowercase();
    url.starts_with("chrome-error://") || t.starts_with("aw, snap") || t.contains("página não encontrada") || t.contains("não foi possível acessar esse site")
}

/// Regra pura: o que fazer com as abas vivas para cada tipo gerenciado em `lista` (agente, url).
pub fn planejar(alvos: &[Alvo], lista: &[(String, String)]) -> Plano {
    let mut plano = Plano::default();
    for (tipo, url_tipo) in lista {
        // candidatas: pelo site; ou (em erro/em branco) as que o mapa ja diz serem do tipo. Aba de agente nao gerenciado nunca entra.
        let gerenciado = |ag: &str| ag.is_empty() || lista.iter().any(|(t, _)| t.eq_ignore_ascii_case(ag));
        let mut cand: Vec<(usize, &Alvo)> = alvos.iter().enumerate().filter(|(_, a)| {
            gerenciado(&a.agente)
                && (do_tipo(&a.url, url_tipo)
                    || (a.agente.eq_ignore_ascii_case(tipo) && (a.erro || a.url.starts_with("about:blank") || a.url.starts_with("chrome-error://"))))
        }).collect();
        if cand.is_empty() {
            plano.faltam.push((tipo.clone(), url_tipo.clone()));
            continue;
        }
        // a melhor: sem erro, ja ligada ao tipo, com titulo (carregou), e a primeira da lista (a mais recentemente usada)
        cand.sort_by_key(|(i, a)| (a.erro, !a.agente.eq_ignore_ascii_case(tipo), a.titulo.trim().is_empty(), *i));
        let (_, boa) = cand[0];
        if !boa.agente.eq_ignore_ascii_case(tipo) {
            plano.adotar.push((boa.id.clone(), tipo.clone()));
        }
        if boa.erro {
            plano.recarregar.push((boa.id.clone(), tipo.clone(), url_tipo.clone()));
        }
        plano.fechar.extend(cand.iter().skip(1).map(|(_, a)| a.id.clone()));
    }
    plano
}

fn alvos_de(abas: &Value, viva: &dyn Fn(&str) -> bool, lista: &[(String, String)]) -> Vec<Alvo> {
    abas.as_array().into_iter().flatten().filter_map(|a| {
        let (id, url) = (a["id"].as_str()?.to_string(), a["url"].as_str().unwrap_or("").to_string());
        let (titulo, agente) = (a["titulo"].as_str().unwrap_or("").to_string(), a["agente"].as_str().unwrap_or("").to_string());
        // so sonda (conecta) quem pode ser escolhido: site gerenciado; as demais abas nem sao tocadas
        let gerenciada = lista.iter().any(|(_, u)| do_tipo(&url, u)) || lista.iter().any(|(t, _)| t.eq_ignore_ascii_case(&agente));
        let erro = pagina_de_erro(&url, &titulo) || (gerenciada && !url.starts_with("about:blank") && !viva(&id));
        Some(Alvo { id, url, titulo, agente, erro })
    }).collect()
}

fn log(dados: &Path, linha: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dados.join("abas_reconcilia.log")) {
        let _ = writeln!(f, "{} {linha}", crate::tempo::agora().data.iso() + " " + &crate::tempo::agora().hm());
    }
}

fn estado(dados: &Path) -> Value {
    std::fs::read_to_string(dados.join("abas_reconcilia.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}))
}

/// UMA reconciliacao. Err = CDP fora do ar (nada foi aberto). Ok = o que fez.
pub fn passo(dados: &Path) -> Result<Value, String> {
    let Ok(_g) = EM_CURSO.try_lock() else { return Ok(json!({ "pulou": "ja ha uma reconciliacao em curso" })) };
    let lista = abas_fixas::lista(dados);
    let abas = crate::vm::abas(dados).map_err(|e| format!("VM/CDP fora do ar, nada aberto ({e})"))?;
    if crate::wpp_midia::gravando() {
        return Ok(json!({ "pulou": "gravando audio: abrir/fechar aba corta o audio" }));
    }
    let base = crate::vm::CDP.trim_start_matches("http://");
    let alvos = alvos_de(&abas, &|id| crate::cdp::viva(base, id), &lista);
    let plano = planejar(&alvos, &lista);
    let (mut adotadas, mut fechadas, mut recarregadas, mut abertas) = (vec![], vec![], vec![], vec![]);
    for (id, ag) in &plano.adotar {
        if crate::vm::registrar_agente(dados, id, ag).is_ok() {
            adotadas.push(ag.clone());
        }
    }
    // abas que fecham: tambem sai do mapa (vm::fechar_aba)
    for id in plano.fechar.iter().take(MAX_FECHAR) {
        if crate::vm::fechar_aba(dados, id).is_ok() {
            fechadas.push(id.chars().take(6).collect::<String>());
        }
    }
    let mut est = estado(dados);
    let agora = crate::fatos::agora_ts();
    for (id, ag, url) in &plano.recarregar {
        if agora - est["recuperou"][ag].as_f64().unwrap_or(0.0) < COOLDOWN_S {
            continue; // ja tentou ha pouco: uma vez so, sem laco
        }
        est["recuperou"][ag] = json!(agora);
        let ok = crate::cdp::Aba::conectar(base, id).and_then(|mut a| a.navegar(url)).is_ok();
        if !ok {
            // aba caida (crash): abre outra e so entao fecha a velha
            if crate::vm::abrir_aba(dados, ag, url).is_ok() {
                let _ = crate::vm::fechar_aba(dados, id);
                recarregadas.push(format!("{ag}(recriada)"));
            }
        } else {
            recarregadas.push(format!("{ag}(recarregada)"));
        }
    }
    let _ = std::fs::write(dados.join("abas_reconcilia.json"), est.to_string());
    for (ag, url) in plano.faltam.iter().take(MAX_ABRIR) {
        let mut ok = false;
        for _ in 0..3 {
            if crate::vm::abrir_aba(dados, ag, url).is_ok() {
                ok = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_secs(3));
        }
        if ok {
            abertas.push(ag.clone());
            std::thread::sleep(std::time::Duration::from_millis(1500));
        }
    }
    let r = json!({ "abas": alvos.len(), "adotadas": adotadas, "fechadas": fechadas, "recarregadas": recarregadas, "abertas": abertas });
    if !(adotadas.is_empty() && fechadas.is_empty() && recarregadas.is_empty() && abertas.is_empty()) {
        log(dados, &r.to_string());
    }
    Ok(r)
}

/// Liga a VM se ela existe e esta desligada (no maximo 1x a cada 10 min). Nao espera.
fn religar_se_preciso(ultima: &mut f64) {
    let agora = crate::fatos::agora_ts();
    if agora - *ultima < 600.0 {
        return;
    }
    if crate::vm::estado().is_ok_and(|e| e["existe"] == true && e["ligada"] == false) {
        *ultima = agora;
        let _ = crate::vm::ligar();
    }
}

/// Laco do servidor: na partida (espera o CDP com limite) e depois a cada 2 min confere e repoe/limpa as abas.
pub fn iniciar() {
    std::thread::spawn(|| {
        let (mut ultima_ligada, mut falhas) = (0.0f64, 0u32);
        std::thread::sleep(std::time::Duration::from_secs(20)); // deixa o resto do servidor subir
        loop {
            let dados = crate::pasta_dados();
            match passo(&dados) {
                Ok(_) => falhas = 0,
                Err(_) => {
                    falhas += 1;
                    religar_se_preciso(&mut ultima_ligada);
                }
            }
            // CDP fora do ar: tenta de novo logo (15 s) por ate 20 vezes, depois volta ao ritmo normal
            std::thread::sleep(std::time::Duration::from_secs(if falhas > 0 && falhas < 20 { 15 } else { 120 }));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alvo(id: &str, url: &str, titulo: &str, agente: &str) -> Alvo {
        Alvo { id: id.into(), url: url.into(), titulo: titulo.into(), agente: agente.into(), erro: pagina_de_erro(url, titulo) }
    }
    fn lista() -> Vec<(String, String)> {
        abas_fixas::lista(Path::new("pasta-que-nao-existe"))
    }
    /// Aplica o plano numa lista fake (como o servidor faria) e devolve o que sobrou.
    fn aplicar(abas: &mut Vec<Alvo>, p: &Plano, proximo: &mut u32) {
        for (id, ag) in &p.adotar {
            abas.iter_mut().find(|a| a.id == *id).unwrap().agente = ag.clone();
        }
        abas.retain(|a| !p.fechar.contains(&a.id));
        for (ag, url) in &p.faltam {
            *proximo += 1;
            abas.push(Alvo { id: format!("novo{proximo}"), url: url.clone(), titulo: "ok".into(), agente: ag.clone(), erro: false });
        }
    }
    fn conta(abas: &[Alvo], host: &str) -> usize {
        abas.iter().filter(|a| a.url.contains(host)).count()
    }

    #[test]
    fn dedup_mantem_uma_por_tipo_e_nao_toca_o_desconhecido() {
        let mut abas = vec![
            alvo("w1", "https://web.whatsapp.com/", "(1) WhatsApp Business", ""),
            alvo("w2", "https://web.whatsapp.com/", "WhatsApp", "whatsapp"),
            alvo("w3", "chrome-error://chromewebdata/", "web.whatsapp.com", "whatsapp"),
            alvo("a1", "https://www.airbnb.com.br/hosting/messages/1", "Mensagens", ""),
            alvo("a2", "https://www.airbnb.com.br/hosting/inbox", "Airbnb", "airbnb"),
            alvo("c1", "https://chatgpt.com/", "ChatGPT", ""),
            alvo("c2", "https://chatgpt.com/c/abc", "ChatGPT", ""),
            alvo("p1", "https://html.duckduckgo.com/html/?q=x", "x at DuckDuckGo", "Pesquisa"),
            alvo("t1", "https://www.tiktok.com/foryou", "TikTok", ""),
            alvo("ch", "chrome://newtab/", "Nova guia", ""),
        ];
        let p = planejar(&abas, &lista());
        // whatsapp: fica a ja mapeada e sadia (w2); w1 e w3 saem. airbnb: a mapeada (a2). chatgpt: a 1a da lista, adotada.
        assert!(p.fechar.contains(&"w1".into()) && p.fechar.contains(&"w3".into()) && !p.fechar.contains(&"w2".into()));
        assert!(p.fechar.contains(&"a1".into()) && !p.fechar.contains(&"a2".into()));
        assert_eq!(p.adotar, vec![("c1".to_string(), "chatgpt".to_string())]);
        assert!(p.fechar.contains(&"c2".into()));
        for intocada in ["p1", "t1", "ch"] {
            assert!(!p.fechar.contains(&intocada.into()), "{intocada} nao pode ser fechada");
        }
        // gemini, grok e deepseek nao existem: faltam; nada mais
        let f: Vec<&str> = p.faltam.iter().map(|x| x.0.as_str()).collect();
        assert_eq!(f, ["gemini", "grok", "deepseek"]);
        assert!(p.recarregar.is_empty());
        let mut n = 0;
        aplicar(&mut abas, &p, &mut n);
        assert_eq!(conta(&abas, "whatsapp"), 1);
        assert_eq!(conta(&abas, "airbnb"), 1);
    }

    #[test]
    fn reiniciar_tres_vezes_nao_cria_aba() {
        // o Chrome restaura as abas antigas (sem dono no mapa) a cada reinicio: a reconciliacao nunca pode aumentar o total
        let mut abas: Vec<Alvo> = vec![];
        let mut n = 0;
        let mut total = vec![];
        for _ in 0..3 {
            let p = planejar(&abas, &lista());
            aplicar(&mut abas, &p, &mut n);
            // reinicio da VM: ids novos, mapa perdido (agente vazio), titulo carregado
            for (i, a) in abas.iter_mut().enumerate() {
                a.id = format!("r{n}-{i}");
                a.agente.clear();
            }
            total.push(abas.len());
        }
        assert_eq!(total, vec![6, 6, 6]);
    }

    #[test]
    fn duplicadas_antigas_viram_uma_so_e_reutiliza_sem_abrir() {
        let mut abas: Vec<Alvo> = (0..5).flat_map(|i| lista().into_iter().map(move |(ag, u)| alvo(&format!("{ag}{i}"), &u, "pagina", &ag))).collect();
        assert_eq!(abas.len(), 30);
        let p = planejar(&abas, &lista());
        assert!(p.faltam.is_empty(), "tem aba de todos os tipos: nao abre nenhuma");
        assert_eq!(p.fechar.len(), 24);
        let mut n = 0;
        aplicar(&mut abas, &p, &mut n);
        assert_eq!(abas.len(), 6);
    }

    #[test]
    fn aba_em_erro_nao_e_a_escolhida_e_so_recarrega_se_for_a_unica() {
        let abas = vec![alvo("e", "https://web.whatsapp.com/", "Aw, Snap!", "whatsapp"), alvo("ok", "https://web.whatsapp.com/", "WhatsApp", "")];
        let p = planejar(&abas, &lista());
        assert_eq!(p.adotar, vec![("ok".to_string(), "whatsapp".to_string())]);
        assert!(p.fechar.contains(&"e".into()) && p.recarregar.is_empty());
        let unica = vec![alvo("e", "chrome-error://chromewebdata/", "web.whatsapp.com", "whatsapp")];
        let p = planejar(&unica, &lista());
        assert_eq!(p.recarregar, vec![("e".to_string(), "whatsapp".to_string(), "https://web.whatsapp.com/".to_string())]);
        assert!(p.fechar.is_empty());
    }

    #[test]
    fn aba_de_agente_nao_gerenciado_nunca_e_tocada_mesmo_no_site_de_um_tipo() {
        let abas = vec![alvo("p", "https://chatgpt.com/", "ChatGPT", "Pesquisa"), alvo("c", "https://chatgpt.com/", "ChatGPT", "")];
        let p = planejar(&abas, &lista());
        assert!(!p.fechar.contains(&"p".into()) && p.adotar == vec![("c".to_string(), "chatgpt".to_string())]);
    }

    #[test]
    fn cdp_fora_do_ar_nao_abre_nada() {
        // sem VM/CDP em 127.0.0.1:9333 o passo devolve erro antes de planejar (nenhuma aba e aberta)
        let d = std::env::temp_dir().join(format!("braniac-reconcilia-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        if crate::vm::abas(&d).is_err() {
            assert!(passo(&d).is_err());
        }
        let _ = std::fs::remove_dir_all(&d);
    }
}
