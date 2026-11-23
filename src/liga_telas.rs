//! Ligacao (liga_telas): as telas do Braniac em Python (escritorio isometrico, voz, Alma, vozes, Cliente, sala, pendencias, mapa,
//! producao, falhas, e-mail, remoto e a tela classica), copiadas sem redesenho (src/*_b1.html, so com o shim do X-Braniac), e as rotas
//! /api/... que elas chamam, no MESMO formato do Python mas com os dados do B.R.A.N.I. Esta ligacao e consultada ANTES das outras: aqui
//! mora o que o Python faz diferente do primeiro desenho do B.R.A.N.I.
//! Regra: nada daqui envia para fora, abre login ou liga acesso remoto. O que o B.R.A.N.I nao tem (microfone nativo, captura de aba logada,
//! Gmail, acesso remoto) responde no formato certo, vazio, com aviso honesto; nunca dado inventado.
use crate::roteador::Roteador;
use crate::{alma_aprende, alma_mente, andares, clareza, contexto, elo_loja, fatos, link_integracao, link_lote_magno, link_processo, link_producao, liga_casa, mudo, pagina, power, rede_apoio, saida, sobrecarga, tempo, Resp};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::{Arc, Mutex};

const AVISO: &str = "ainda desligado no B.R.A.N.I";

fn html(nome: &str) -> Option<&'static str> {
    Some(match nome {
        "/sauraki" => include_str!("sauraki3d_b1.html"),
        "/sauraki?plano=1" => include_str!("sauraki_b1.html"),
        "/hud" | "/hud.html" => include_str!("hud_b1.html"),
        "/hud-classico" => include_str!("hud_classico_b1.html"),
        "/voz" | "/voz.html" => include_str!("voz_b1.html"),
        "/alma" => include_str!("alma_b1.html"),
        "/vozes" | "/vozes.html" => include_str!("vozes_b1.html"),
        "/magno" => include_str!("magno_b1.html"),
        "/sala" | "/sala.html" => include_str!("sala_b1.html"),
        "/pendencias" => include_str!("pendencias_b1.html"),
        "/mapa" => include_str!("mapa_b1.html"),
        "/producao" => include_str!("producao_b1.html"),
        "/falhas" => include_str!("falhas_b1.html"),
        "/email" | "/email.html" => include_str!("email_b1.html"),
        "/remoto" | "/remoto/" => include_str!("remoto_b1.html"),
        "/classico" | "/index.html" => include_str!("index_classico_b1.html"),
        _ => return None,
    })
}

fn png(bytes: &'static [u8]) -> Resp {
    tiny_http::Response::from_data(bytes.to_vec()).with_header(tiny_http::Header::from_bytes("Content-Type", "image/png").unwrap())
}

/// Mudo no formato do Python (a chave do dono la se chama whatsapp_carlos).
pub fn mudo_py(dados: &Path, ts: f64) -> Value {
    let mut m = mudo::estado(dados, ts);
    m["whatsapp_carlos"] = m["whatsapp_dono"].clone();
    m
}

// ---------- temperatura e carga (cache em segundo plano: a leitura demora) ----------

static TERMICO: Mutex<Option<(f64, Value)>> = Mutex::new(None);

fn sem_janela(c: &mut std::process::Command) {
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(c, 0x0800_0000); // CREATE_NO_WINDOW
    let _ = c;
}

/// GPU pelo nvidia-smi; CPU (carga) e memoria pelo WMI. Sem sensor = null, nunca numero inventado.
fn medir() -> Value {
    let mut g = std::process::Command::new("nvidia-smi");
    g.args(["--query-gpu=temperature.gpu", "--format=csv,noheader,nounits"]);
    sem_janela(&mut g);
    let gpu = g.output().ok().and_then(|o| crate::termico::parse_gpu(&String::from_utf8_lossy(&o.stdout)));
    let mut p = std::process::Command::new("powershell.exe");
    p.args(["-NoProfile", "-NonInteractive", "-Command",
        "$c=(Get-CimInstance Win32_Processor|Measure-Object LoadPercentage -Average).Average;$o=Get-CimInstance Win32_OperatingSystem;\
         \"$c $([math]::Round(100*($o.TotalVisibleMemorySize-$o.FreePhysicalMemory)/$o.TotalVisibleMemorySize,1))\""]);
    sem_janela(&mut p);
    let saida = p.output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    let mut n = saida.split_whitespace().map(|x| x.replace(',', ".").parse::<f64>().ok());
    let (cpu, ram) = (n.next().flatten(), n.next().flatten());
    let l = crate::termico::Leitura { gpu_c: gpu, cpu_c: None, cpu_pct: cpu.unwrap_or(0.0), ram_pct: ram.unwrap_or(0.0), top: vec![] };
    json!({ "pronto": gpu.is_some() || cpu.is_some(), "gpu_c": gpu, "cpu_c": null, "cpu_pct": cpu, "ram_pct": ram, "quente": crate::termico::quente(&l) })
}

/// Ultima leitura; pede uma nova (em segundo plano) quando tem mais de 10 s. A primeira chamada devolve pronto:false.
pub fn termico() -> Value {
    let agora = fatos::agora_ts();
    let g = TERMICO.lock().unwrap_or_else(|e| e.into_inner());
    let (velha, atual) = match &*g {
        Some((t, v)) => (agora - t > 10.0, v.clone()),
        None => (true, json!({ "pronto": false, "gpu_c": null, "cpu_c": null, "cpu_pct": null, "ram_pct": null, "quente": null })),
    };
    drop(g);
    if velha {
        // marca antes de medir: pedidos seguidos nao abrem varios processos
        *TERMICO.lock().unwrap_or_else(|e| e.into_inner()) = Some((agora, atual.clone()));
        std::thread::spawn(|| {
            let v = medir();
            *TERMICO.lock().unwrap_or_else(|e| e.into_inner()) = Some((fatos::agora_ts(), v));
        });
    }
    atual
}

/// Recursos para o retrato do escritorio (so os quatro campos que ele mostra).
pub fn recursos() -> Value {
    let t = termico();
    json!({ "gpu_c": t["gpu_c"], "cpu_pct": t["cpu_pct"], "ram_pct": t["ram_pct"], "quente": t["quente"] })
}

// ---------- estado de voz no formato do Python ----------

fn escuta_py(dados: &Path, ts: f64) -> Value {
    let Some(par) = liga_casa::voz_livre() else { return json!({ "estado": "dormindo", "pensando": true }) };
    let mut e = liga_casa::estado_voz(dados, &par, ts);
    e["mudo"] = mudo_py(dados, ts);
    let nivel = crate::aparelho::estado()["nivel"].as_f64().unwrap_or(0.0); // 0 com o microfone fechado
    e["nivel"] = json!(nivel);
    e["entrada"] = json!((nivel * 55.0).min(1.0));
    e["microfone"] = crate::aparelho::estado();
    e["microfone"]["chave"] = json!(crate::aparelho::chave(dados));
    e["fala"] = json!(0.0);
    e["atividade"] = json!("idle");
    e
}

/// Botoes da tela de voz. Sem microfone nativo: acordar e descansar passam pela mesma maquina de estados, por frase.
fn escuta_acao(dados: &Path, rot: &Arc<Roteador>, v: &Value, ts: f64) -> Value {
    let estado = liga_casa::voz_livre().map(|p| p.0.estado);
    match v["acao"].as_str().unwrap_or("") {
        "ligar" => {
            crate::aparelho::MIC_LIGADO.store(true, std::sync::atomic::Ordering::SeqCst);
            liga_casa::VOZ.lock().unwrap_or_else(|e| e.into_inner()).0.ligar()
        }
        "desligar" => {
            crate::aparelho::MIC_LIGADO.store(false, std::sync::atomic::Ordering::SeqCst);
            liga_casa::VOZ.lock().unwrap_or_else(|e| e.into_inner()).0.desligar()
        }
        "acordar" if estado == Some("dormindo") => drop(liga_casa::post(dados, rot, "/api/escuta/frase", &json!({ "texto": "Jarvis, acorda" }))),
        "descansar" if estado == Some("acordado") => drop(liga_casa::post(dados, rot, "/api/escuta/frase", &json!({ "texto": "pode descansar" }))),
        "frase" if !v["texto"].as_str().unwrap_or("").trim().is_empty() => {
            let t: String = v["texto"].as_str().unwrap_or("").chars().take(300).collect();
            drop(liga_casa::post(dados, rot, "/api/escuta/frase", &json!({ "texto": t })))
        }
        _ => {}
    }
    escuta_py(dados, ts)
}

// ---------- Alma ----------

/// O B.R.A.N.I nomeia por papel; a tela do Python por pessoa.
const NOMES_ALMA: [(&str, &str); 2] = [("rede_parceira_sobrecarga", "rede_jana_sobrecarga"), ("rede_mae_apoio", "rede_marcia_apoio")];

fn params_alma(dados: &Path) -> Value {
    let p = alma_mente::params(dados);
    let mut o = serde_json::Map::new();
    for (k, _) in alma_mente::EDITAVEIS.iter() {
        let nome = NOMES_ALMA.iter().find(|n| n.0 == *k).map_or(*k, |n| n.1);
        o.insert(nome.to_string(), p[*k].clone());
    }
    Value::Object(o)
}

fn alma_painel(dados: &Path, agora: &tempo::Agora, ts: f64) -> Value {
    let rede: Vec<Value> = rede_apoio::historico(dados, 15).into_iter().map(|mut r| {
        r["contato"] = match r["contato"].as_str() { Some("mae") => json!("mae"), Some("parceira") => json!("jana"), _ => r["contato"].clone() };
        r
    }).collect();
    let mudo = mudo::ativo(dados, ts);
    json!({ "diario": alma_mente::diario(dados, 14, agora), "params": params_alma(dados), "sobrecarga": sobrecarga::avaliar(dados, agora, false),
            "historico": sobrecarga::historico(dados, 36), "rede": rede, "quieta": alma_mente::quieta(dados, agora),
            "voz": { "ativa": false, "nivel": 0, "estado": "idle", "legenda": "", "historico": [], "entrada": 0.0, "fala": 0.0, "mudo": mudo, "microfone": crate::aparelho::estado()["aberto"] },
            "impedida": "a esfera de voz não existe no B.R.A.N.I (falta microfone nativo)" })
}

fn alma_acao(dados: &Path, agora: &tempo::Agora, v: &Value) -> Resp {
    match v["acao"].as_str().unwrap_or("") {
        "config" => {
            let chave = v["chave"].as_str().unwrap_or("");
            let chave = NOMES_ALMA.iter().find(|n| n.1 == chave).map_or(chave, |n| n.0);
            let r = alma_mente::definir(dados, chave, &v["valor"]);
            if r["erro"].is_null() { crate::dado(json!({ "ok": true, "params": params_alma(dados) })) } else { crate::dado(r) }
        }
        "rotular" => crate::dado(json!({ "ok": alma_aprende::rotular(dados, agora, v["id"].as_i64().unwrap_or(0), v["tema"].as_str().unwrap_or("")) })),
        "apagar" => crate::dado(json!({ "ok": alma_aprende::apagar(dados, agora, v["escopo"].as_str().unwrap_or("")) })),
        "consentimento" => {
            let m: Vec<(&str, bool)> = ["temas", "sinais", "exemplos", "pausado"].iter().filter_map(|k| Some((*k, v[*k].as_bool()?))).collect();
            crate::dado(alma_aprende::definir_consentimento(dados, &m))
        }
        _ => crate::erro("acao desconhecida", 400),
    }
}

// ---------- status no formato do Python ----------

fn status_py(dados: &Path, rot: &Roteador, ts: f64) -> Value {
    let mut s = crate::status(dados, rot, ts);
    let p = power::estado(dados);
    s["mudo"] = mudo_py(dados, ts);
    s["motivo"] = p["motivo"].clone();
    s["jogo"] = p["jogo"].clone();
    s["modo_jogo_auto"] = p["modo_jogo_auto"].clone();
    s["spotify"] = json!({ "client_id": false, "logado": false });
    s["vm"] = crate::vm::estado().unwrap_or_else(|e| json!({ "erro": e }));
    s["whatsapp"] = crate::whatsapp::estado(dados).map(|w| json!({ "estado": w["estado"], "aviso": w["aviso"], "nao_lidas": w["nao_lidas"] }))
        .unwrap_or_else(|e| json!({ "estado": "fechado", "aviso": e }));
    s
}

fn contas() -> Value {
    let vm = crate::vm::estado().ok().and_then(|e| e["ligada"].as_bool()).unwrap_or(false);
    let c: Vec<Value> = [("chatgpt", "https://chatgpt.com/"), ("gemini", "https://gemini.google.com/app?pli=1"), ("grok", "https://grok.com/"), ("deepseek", "https://chat.deepseek.com/"),
        ("kimi", "https://www.kimi.ai/"), ("claude", "https://claude.ai/"), ("google", "https://myaccount.google.com/")]
        .iter().map(|(n, u)| json!({ "nome": n, "aberta": false, "precisa_login": false, "url": u })).collect();
    json!({ "navegador": vm, "contas": c, "aviso": "o login nas IAs é sempre seu, na tela da VM; o B.R.A.N.I não sabe o que está aberto" })
}

fn remoto() -> Value {
    json!({ "ligado": false, "conectado": false, "ultimo": "", "erro": format!("acesso remoto {AVISO}: só o dono liga"), "tentativas": [], "codigo": "" })
}

fn n_de(url: &str, ch: &str, padrao: f64) -> f64 {
    crate::parametro(url, ch).and_then(|x| x.parse().ok()).unwrap_or(padrao)
}

pub fn get(dados: &Path, rot: &Arc<Roteador>, url: &str) -> Option<Resp> {
    let (agora, ts) = (tempo::agora(), fatos::agora_ts());
    let caminho = url.split('?').next().unwrap_or("");
    let chave = if caminho == "/sauraki" && url.contains("plano=1") { "/sauraki?plano=1" } else { caminho };
    if let Some(h) = html(chave) {
        return Some(pagina(h));
    }
    Some(match caminho {
        "/magno-icone-192.png" => png(include_bytes!("magno-icone-192.png")),
        "/magno-icone-512.png" => png(include_bytes!("magno-icone-512.png")),
        // login do Google e sempre do dono
        "/email/login" => crate::erro(format!("Gmail {AVISO}: o login é sempre seu"), 400),
        "/api/predio" => crate::dado(andares::estado(dados, &[])),
        "/api/pendencias" => {
            let mut p = crate::pendencias::painel(dados, ts);
            // a tela do Python chama de so_carlos o que o B.R.A.N.I chama de so_dono
            if p["so_carlos"].is_null() {
                p["so_carlos"] = p["so_dono"].clone();
            }
            crate::dado(p)
        }
        "/api/par/link" => {
            let n = n_de(url, "n", 300.0) as usize;
            let em_no_ar = std::fs::read_to_string(dados.join("link").join("fila_url.txt")).ok().map(|u| u.trim().to_string()).filter(|u| !u.is_empty());
            let tunel = link_integracao::tunel(dados, None, em_no_ar.as_deref(), &|| {}).unwrap_or_else(|_| json!({ "ligado": false, "url": null }));
            let mut r = link_integracao::resumo(dados, ts, "Xandao");
            r["tunel"] = tunel;
            r["mensagens"] = json!(link_integracao::conversa(dados, n, n_de(url, "depois", 0.0)));
            crate::dado(r)
        }
        "/api/par/conversa" => {
            let mut r = elo_loja::resumo(dados, ts);
            r["mensagens"] = json!(elo_loja::historico(dados, n_de(url, "n", 300.0) as usize, n_de(url, "depois", 0.0)));
            crate::dado(r)
        }
        // captura de aba logada do Chrome do Braniac: o B.R.A.N.I nao tem esse navegador (a VM e outra coisa)
        "/api/tela" => crate::erro(format!("captura da aba {AVISO}"), 409),
        "/api/magno" => {
            let mut d = link_lote_magno::ler(dados);
            if d["esteira"].is_object() {
                let s = link_lote_magno::slots(&d["esteira"], &[]);
                d["esteira"]["slots"] = json!(s);
            }
            crate::dado(d)
        }
        "/api/producao" => {
            let inicio = ts - (agora.hora * 3600 + agora.min * 60 + agora.seg) as f64;
            link_processo::painel(dados, &link_producao::pedidos(dados), ts, inicio).map(crate::dado).unwrap_or_else(|e| crate::erro(e, 500))
        }
        "/api/falhas" => crate::dado(clareza::resumo(dados, &contexto::Ctx::ler(dados), ts, 7.0, &|_| false)),
        "/api/escuta-estado" => crate::dado(escuta_py(dados, ts)),
        "/api/mudo" => crate::dado(mudo_py(dados, ts)),
        "/api/aparelho/voz" => crate::dado(json!({ "chave": crate::aparelho::chave(dados), "microfone": crate::aparelho::estado() })),
        "/api/termico" => crate::dado(termico()),
        "/api/status" => crate::dado(status_py(dados, rot, ts)),
        "/api/b2/contas" => crate::dado(contas()),
        "/api/alma/painel" => crate::dado(alma_painel(dados, &agora, ts)),
        "/api/alma/sabe" => {
            let mut s = alma_aprende::sabe(dados, &agora);
            s["pendentes"] = json!(alma_aprende::pendentes_rotulo(dados, 10));
            crate::dado(s)
        }
        "/api/alma/voz-estado" => crate::dado(json!({ "ativa": false, "nivel": 0, "estado": "idle", "legenda": "", "historico": [], "entrada": 0.0, "fala": 0.0, "mudo": mudo::ativo(dados, ts), "microfone": crate::aparelho::estado()["aberto"] })),
        "/api/remoto" => crate::dado(remoto()),
        // fila de avisos do Python: {"avisos": [...]} e a tela esvazia ao ler; aqui nada e esvaziado, os avisos ficam em /api/avisos
        _ => return None,
    })
}

pub fn post(dados: &Path, rot: &Arc<Roteador>, caminho: &str, v: &Value) -> Option<Resp> {
    let (agora, ts) = (tempo::agora(), fatos::agora_ts());
    Some(match caminho {
        "/api/predio/construir" => andares::construir(dados, v["andar"].as_str().unwrap_or(""), &[]).map(crate::dado).unwrap_or_else(|e| crate::erro(e, 400)),
        // nada sai para o Xandao: o texto fica so na caixa de saida do B.R.A.N.I
        "/api/par/link/falar" => {
            let t = v["texto"].as_str().unwrap_or("").trim();
            if t.is_empty() {
                return Some(crate::dado(json!({ "ok": false, "detalhe": "texto vazio" })));
            }
            // chave "link" ligada: sai pela ponte (marcado como do dono); desligada: so a caixa de saida
            if let Some(r) = crate::ponte::falar_ao_par(dados, t) {
                return Some(crate::dado(r));
            }
            saida::registrar(dados, "link-xandao", "Parceiro", t);
            crate::dado(json!({ "ok": false, "detalhe": format!("envio ao Parceiro {AVISO}; o texto ficou só na caixa de saída do B.R.A.N.I (/saida)") }))
        }
        "/api/par/link/tunel" => crate::dado(json!({ "ligado": false, "url": null, "aviso": format!("túnel {AVISO}: só o dono liga") })),
        "/api/tela-acao" => crate::erro(format!("ação na aba {AVISO}"), 409),
        "/api/escuta-acao" => crate::dado(escuta_acao(dados, rot, v, ts)),
        // fala pedida de forma explicita (amostra de voz): toca mesmo com a chave "voz" desligada, uma vez, no agente pedido
        "/api/voz/tocar" => {
            let t: String = v["texto"].as_str().unwrap_or("").trim().chars().take(300).collect();
            if t.is_empty() {
                return Some(crate::erro("mande {\"texto\": \"...\", \"agente\": \"jarvis|alma\"}", 400));
            }
            let (rot, d, ag) = (rot.clone(), dados.to_path_buf(), if v["agente"] == "alma" { "alma" } else { "jarvis" });
            std::thread::spawn(move || drop(crate::aparelho::falar(&d, &rot, &t, ag)));
            crate::dado(json!({ "ok": true }))
        }
        "/api/orbe" => {
            match v["acao"].as_str() {
                Some("mini") => crate::escuta::definir_modo_tela(dados, "mini"),
                Some("abrir") => {}
                _ => crate::escuta::definir_modo_tela(dados, "completo"),
            }
            crate::dado(json!({ "modo": crate::escuta::modo_tela(dados), "aviso": "sem janela nativa no B.R.A.N.I: só o modo foi guardado" }))
        }
        "/api/modo-jogo" => {
            power::set_modo_jogo_auto(dados, !matches!(v["auto"], Value::Null | Value::Bool(false)));
            crate::dado(status_py(dados, rot, ts))
        }
        "/api/alma/acao" => alma_acao(dados, &agora, v),
        "/api/alma/voz-acao" => crate::dado(json!({ "ok": false, "aviso": format!("esfera de voz da Alma {AVISO}") })),
        "/api/producao/pausa" => crate::dado(json!({ "pausado": link_processo::pausado(dados), "aviso": format!("o B.R.A.N.I não renderiza; pausa {AVISO}") })),
        "/api/remoto" | "/api/remoto/ligar" | "/api/remoto/desligar" | "/api/remoto/codigo" => crate::dado(remoto()),
        "/api/email" => {
            if !v["client_id"].is_null() || !v["client_secret"].is_null() {
                return Some(crate::erro(format!("Gmail {AVISO}: o B.R.A.N.I não guarda credenciais do Google"), 400));
            }
            crate::dado(json!({ "cliente": false, "logado": false, "conta": null }))
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paginas_e_formatos_do_python() {
        for p in ["/sauraki", "/sauraki?plano=1", "/hud", "/hud-classico", "/voz", "/alma", "/vozes", "/magno", "/sala", "/pendencias", "/mapa", "/producao", "/falhas", "/email", "/remoto", "/classico", "/index.html"] {
            assert!(html(p).is_some_and(|h| h.contains("X-Braniac")), "{p}");
        }
        assert_eq!(params_alma(Path::new("/nao/existe"))["rede_jana_sobrecarga"], false);
        let d = std::env::temp_dir().join(format!("braniac-telas-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        assert!(mudo_py(&d, 1.0).get("whatsapp_carlos").is_some());
        assert_eq!(alma_acao(&d, &tempo::agora(), &json!({ "acao": "voa" })).status_code().0, 400);
        assert_eq!(remoto()["ligado"], false);
        let _ = std::fs::remove_dir_all(&d);
    }
}
