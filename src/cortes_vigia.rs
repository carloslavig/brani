//! Vigia dos cortes: as REGRAS de um ciclo (checar live, gravar, mandar a gravacao/clipes/VOD para a fila de trabalho, faxina).
//! Travas: so canal com "autorizado": true e vigiado; PAUSA_ANDAR (pausa duradoura: o andar Claquete assumiu os cortes) impede ligar;
//! PARAR pede a parada; trabalho pesado espera o modo jogo acabar (`pode_trabalhar`); a mesma chave nunca entra duas vezes na fila.
//! Ficou de fora (closures em `Mundo`): Twitch, streamlink, o processo em segundo plano e o laco com sleep. `ciclo` devolve os trabalhos a
//! fazer (um por vez, por quem chama, com `cortes_pipeline`) e diz se ha aprovado esperando publicacao; quem publica e avisa e quem chama.
#![allow(dead_code)]
use crate::cortes_config::{para_canal, pasta, sub};
use crate::tempo::Agora;
use crate::{cortes_captura, cortes_fila as fila, cortes_pipeline::log};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn pid(dados: &Path) -> PathBuf {
    pasta(dados).join("vigia.pid")
}

fn parar(dados: &Path) -> PathBuf {
    pasta(dados).join("PARAR")
}

pub fn pausa_do_andar(dados: &Path) -> bool {
    pasta(dados).join("PAUSA_ANDAR").exists()
}

pub fn deve_parar(dados: &Path) -> bool {
    parar(dados).exists()
}

/// A "batida" do vigia: ele regrava o arquivo a cada ciclo. (Nunca testar o processo pelo numero: no Windows isso mata o processo.)
pub fn bater(dados: &Path) {
    let _ = std::fs::create_dir_all(pasta(dados));
    let _ = std::fs::write(pid(dados), std::process::id().to_string());
}

pub fn rodando(dados: &Path, cfg: &Value, ts: f64) -> bool {
    let batida = std::fs::metadata(pid(dados)).ok().and_then(|m| m.modified().ok()).and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs_f64());
    batida.is_some_and(|b| ts - b < cfg["intervalo_min"].as_f64().unwrap_or(2.0) * 60.0 + 300.0)
}

/// `subir` e quem sobe o vigia como processo separado.
pub fn ligar(dados: &Path, cfg: &Value, ts: f64, subir: &dyn Fn() -> Result<(), String>) -> String {
    if pausa_do_andar(dados) {
        return "rotina antiga de cortes da Twitch pausada (03/11/2026); apague data/cortes/PAUSA_ANDAR para religar".into();
    }
    if rodando(dados, cfg, ts) {
        return "o vigia dos cortes ja esta ligado".into();
    }
    let _ = std::fs::remove_file(parar(dados));
    match subir() {
        Ok(()) => "vigia dos cortes ligado".into(),
        Err(e) => format!("nao consegui ligar o vigia dos cortes: {e}"),
    }
}

pub fn desligar(dados: &Path) -> String {
    let _ = std::fs::create_dir_all(pasta(dados));
    let _ = std::fs::write(parar(dados), "");
    "pedido de parada enviado ao vigia dos cortes (para no fim do ciclo atual)".into()
}

/// Fim do laco: tira a batida.
pub fn encerrar(dados: &Path) {
    let _ = std::fs::remove_file(pid(dados));
}

pub fn canais_autorizados(cfg: &Value) -> Vec<Value> {
    cfg["canais"].as_array().map(|a| a.iter().filter(|c| c["login"].as_str().is_some_and(|l| !l.is_empty()) && c["autorizado"].as_bool().unwrap_or(false)).cloned().collect()).unwrap_or_default()
}

/// O dono jogando: GPU e CPU sao dele.
pub fn pode_trabalhar(cfg: &Value, modo_jogo: bool) -> bool {
    !(cfg["pausar_no_modo_jogo"].as_bool().unwrap_or(true) && modo_jogo)
}

/// Segundos entre buscas de clipes. Com meta diaria (youtube_horarios) e pouco corte em estoque, busca de hora em hora.
pub fn espera_clipes(ccfg: &Value, estoque: i64) -> f64 {
    let espera = ccfg["clipes_cada_h"].as_f64().unwrap_or(6.0) * 3600.0;
    let horarios = ccfg["youtube_horarios"].as_array().map(Vec::len).unwrap_or(0) as i64;
    if horarios > 0 && estoque < 2 * horarios { espera.min(3600.0) } else { espera }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Trabalho {
    Live { video: PathBuf, chat: PathBuf, login: String, jogo: String, cfg: Value },
    Clipe { clipe: Value, login: String, cfg: Value },
    Vod { url: String, login: String, id: String, cfg: Value },
}

fn texto(v: &Value) -> String {
    v.as_str().map(String::from).unwrap_or_else(|| v.to_string())
}

impl Trabalho {
    pub fn chave(&self) -> String {
        match self {
            Trabalho::Live { video, .. } => format!("live:{}", video.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
            Trabalho::Clipe { clipe, .. } => format!("clipe:{}", texto(&clipe["id"])),
            Trabalho::Vod { id, .. } => format!("vod:{id}"),
        }
    }
}

#[derive(Default)]
pub struct Estado {
    /// login -> (video, chat) da gravacao em andamento
    pub gravando: BTreeMap<String, (PathBuf, PathBuf)>,
    pub jogo: BTreeMap<String, String>,
    pub na_fila: BTreeSet<String>,
    pub ultimo_clipes: BTreeMap<String, f64>,
    pub ultima_limpeza: f64,
}

impl Estado {
    /// Quem fez o trabalho (com erro ou nao) avisa aqui, para a chave poder entrar de novo.
    pub fn concluir(&mut self, chave: &str) {
        self.na_fila.remove(chave);
    }
}

pub struct Mundo<'a> {
    /// Some({jogo, ...}) se o canal esta ao vivo.
    pub ao_vivo: &'a dyn Fn(&str) -> Result<Option<Value>, String>,
    /// O streamlink daquele login ainda esta gravando?
    pub gravacao_ativa: &'a dyn Fn(&str) -> bool,
    /// (login, video, chat): comeca a gravar a live e o chat.
    pub iniciar_gravacao: &'a dyn Fn(&str, &Path, &Path) -> Result<(), String>,
    pub parar_gravacao: &'a dyn Fn(&str),
    /// (login, janela em horas, maximo, minimo de views) -> clipes mais vistos.
    pub clipes: &'a dyn Fn(&str, f64, usize, f64) -> Result<Vec<Value>, String>,
    pub ultimo_vod: &'a dyn Fn(&str) -> Result<Option<Value>, String>,
}

#[derive(Debug, Default, PartialEq)]
pub struct Ciclo {
    pub trabalhos: Vec<Trabalho>,
    /// Ha corte aprovado esperando: chamar `cortes_pipeline::publicar_aprovados`.
    pub publicar: bool,
}

pub fn ciclo(dados: &Path, est: &mut Estado, cfg: &Value, agora: &Agora, ts: f64, m: &Mundo) -> Ciclo {
    let mut novos: Vec<Trabalho> = vec![];
    for c in canais_autorizados(cfg) {
        let login = c["login"].as_str().unwrap_or("").to_lowercase();
        let ccfg = para_canal(&c, cfg); // perfil (ex.: gringo em ingles) por cima da config geral
        if est.gravando.contains_key(&login) && !(m.gravacao_ativa)(&login) {
            // a live acabou (ou caiu): processa a gravacao
            (m.parar_gravacao)(&login);
            if let Some((video, chat)) = est.gravando.remove(&login) {
                if std::fs::metadata(&video).is_ok_and(|x| x.len() > 5_000_000) {
                    log(dados, agora, &format!("live de {login} terminou; gravacao na fila de cortes"));
                    novos.push(Trabalho::Live { video, chat, login: login.clone(), jogo: est.jogo.get(&login).cloned().unwrap_or_default(), cfg: ccfg.clone() });
                }
            }
        }
        if c["live"].as_bool().unwrap_or(true) && !est.gravando.contains_key(&login) {
            match (m.ao_vivo)(&login) {
                Ok(Some(info)) => {
                    let (video, chat) = cortes_captura::nomes_gravacao(&sub(dados, "gravacoes"), &login, agora);
                    match (m.iniciar_gravacao)(&login, &video, &chat) {
                        Ok(()) => {
                            let jogo = info["jogo"].as_str().unwrap_or("").to_string();
                            log(dados, agora, &format!("{login} AO VIVO ({}): gravando em {}", if jogo.is_empty() { "?" } else { &jogo }, video.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
                            est.jogo.insert(login.clone(), jogo);
                            est.gravando.insert(login.clone(), (video, chat));
                        }
                        Err(e) => log(dados, agora, &format!("gravacao de {login} nao comecou: {e}")),
                    }
                }
                Ok(None) => {}
                Err(e) => log(dados, agora, &format!("checagem de {login} falhou: {e}")),
            }
        }
        let espera = espera_clipes(&ccfg, fila::estoque(dados, ccfg["perfil"].as_str().unwrap_or("")));
        if c["clipes"].as_bool().unwrap_or(false) && ts - est.ultimo_clipes.get(&login).copied().unwrap_or(0.0) > espera {
            est.ultimo_clipes.insert(login.clone(), ts);
            let maximo = ccfg["clipes_max"].as_u64().unwrap_or(5) as usize;
            match (m.clipes)(&login, ccfg["clipes_janela_h"].as_f64().unwrap_or(48.0), (maximo * 5).max(15), ccfg["clipes_min_views"].as_f64().unwrap_or(0.0)) {
                // os mais vistos que AINDA NAO viraram corte (pegar so o top N, ja vistos, nao trazia nada novo)
                Ok(cs) => novos.extend(cs.into_iter().filter(|cl| !fila::ja_visto(dados, &format!("clipe:{}", texto(&cl["id"])))).take(maximo).map(|clipe| Trabalho::Clipe { clipe, login: login.clone(), cfg: ccfg.clone() })),
                Err(e) => log(dados, agora, &format!("busca de clipes de {login} falhou: {e}")),
            }
        }
        // VOD so depois da live (antes disso esta incompleto)
        if c["vods"].as_bool().unwrap_or(false) && !est.gravando.contains_key(&login) {
            match (m.ultimo_vod)(&login) {
                Ok(Some(v)) if !fila::ja_visto(dados, &format!("vod:{}", texto(&v["id"]))) => novos.push(Trabalho::Vod { url: texto(&v["url"]), login: login.clone(), id: texto(&v["id"]), cfg: ccfg.clone() }),
                Ok(_) => {}
                Err(e) => log(dados, agora, &format!("busca de VOD de {login} falhou: {e}")),
            }
        }
    }
    novos.retain(|t| est.na_fila.insert(t.chave()));
    if ts - est.ultima_limpeza > 3600.0 {
        est.ultima_limpeza = ts;
        cortes_captura::limpar_gravacoes(dados, cfg["manter_gravacoes_dias"].as_f64().unwrap_or(3.0), ts);
    }
    Ciclo { trabalhos: novos, publicar: !fila::listar(dados, Some("aprovado"), 1).is_empty() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cortes_config::padrao;
    use crate::cortes_fila::tmp;
    use crate::cortes_pipeline::agora_teste;
    use serde_json::json;
    use std::cell::RefCell;

    fn mundo_proibido<'a>() -> Mundo<'a> {
        Mundo { ao_vivo: &|_| panic!("canal sem autorizacao nao e vigiado"), gravacao_ativa: &|_| panic!(), iniciar_gravacao: &|_, _, _| panic!(), parar_gravacao: &|_| panic!(), clipes: &|_, _, _, _| panic!(), ultimo_vod: &|_| panic!() }
    }

    #[test]
    fn canal_sem_autorizacao_nao_e_vigiado() {
        let d = tmp("vigia-sem");
        let (agora, ts) = agora_teste();
        let mut cfg = padrao(); // o exemplo vem com autorizado: false
        assert_eq!(ciclo(&d, &mut Estado::default(), &cfg, &agora, ts, &mundo_proibido()), Ciclo::default());
        cfg["canais"] = json!([{"login": "", "autorizado": true}, {"login": "ana"}, {"login": "bia", "autorizado": "sim"}]);
        assert_eq!(ciclo(&d, &mut Estado::default(), &cfg, &agora, ts, &mundo_proibido()), Ciclo::default());
    }

    #[test]
    fn live_grava_e_ao_acabar_vira_trabalho_clipes_sem_repetir_e_vod() {
        let d = tmp("vigia-live");
        let (agora, ts) = agora_teste();
        let mut cfg = padrao();
        cfg["canais"] = json!([{"login": "Ana", "autorizado": true, "live": true, "clipes": true, "vods": true, "perfil": "gringo"}]);
        cfg["perfis"]["gringo"]["clipes_max"] = json!(2);
        let (no_ar, ativa, pedidos) = (RefCell::new(true), RefCell::new(true), RefCell::new(vec![]));
        let m = Mundo {
            ao_vivo: &|l| Ok(if *no_ar.borrow() { Some(json!({"jogo": "Jogo X", "login": l})) } else { None }),
            gravacao_ativa: &|_| *ativa.borrow(),
            iniciar_gravacao: &|_, video, _| std::fs::write(video, vec![0u8; 5_000_001]).map_err(|e| e.to_string()),
            parar_gravacao: &|l| pedidos.borrow_mut().push(format!("parar {l}")),
            clipes: &|l, janela, max, min| {
                pedidos.borrow_mut().push(format!("clipes {l} {janela} {max} {min}"));
                Ok((1..=4).map(|i| json!({"id": format!("c{i}"), "url": "u", "views": 9000})).collect())
            },
            ultimo_vod: &|_| Ok(Some(json!({"id": 77, "url": "https://v/77"}))),
        };
        fila::marcar_visto(&d, "clipe:c1", ts).unwrap();
        let mut est = Estado::default();
        let c1 = ciclo(&d, &mut est, &cfg, &agora, ts, &m);
        // ao vivo: grava; clipes c2 e c3 (c1 ja visto, teto 2); VOD so depois da live
        assert_eq!(c1.trabalhos.iter().map(Trabalho::chave).collect::<Vec<_>>(), vec!["clipe:c2", "clipe:c3"]);
        assert!(est.gravando.contains_key("ana") && est.jogo["ana"] == "Jogo X" && !c1.publicar);
        assert_eq!(*pedidos.borrow(), vec!["clipes ana 168 15 2000"]);
        let Trabalho::Clipe { cfg: ccfg, .. } = &c1.trabalhos[0] else { panic!() };
        assert_eq!((ccfg["idioma"].as_str(), ccfg["perfil"].as_str()), (Some("en"), Some("gringo")));
        // 10 min depois: a live acabou; clipes ainda nao (espera de 6 h); c2/c3 seguem na fila de trabalho
        *ativa.borrow_mut() = false;
        *no_ar.borrow_mut() = false;
        fila::adicionar(&d, &json!({"titulo": "x", "status": "aprovado"}), ts).unwrap();
        let c2 = ciclo(&d, &mut est, &cfg, &agora, ts + 600.0, &m);
        assert_eq!(c2.trabalhos.iter().map(Trabalho::chave).collect::<Vec<_>>(), vec!["live:ana_20261010_1520.ts", "vod:77"]);
        assert!(c2.publicar && est.gravando.is_empty() && pedidos.borrow().contains(&"parar ana".to_string()));
        let Trabalho::Live { jogo, chat, .. } = &c2.trabalhos[0] else { panic!() };
        assert!(jogo == "Jogo X" && chat.ends_with("ana_20261010_1520.chat.jsonl"));
        // mesma chave nao entra duas vezes; depois de concluir, entra
        assert!(ciclo(&d, &mut est, &cfg, &agora, ts + 700.0, &m).trabalhos.is_empty());
        est.concluir("vod:77");
        assert_eq!(ciclo(&d, &mut est, &cfg, &agora, ts + 800.0, &m).trabalhos.len(), 1);
        // 7 h depois busca clipes de novo: c2 e c3 ainda na fila de trabalho, nada novo entra em dobro
        assert!(ciclo(&d, &mut est, &cfg, &agora, ts + 7.0 * 3600.0, &m).trabalhos.is_empty());
        assert_eq!(pedidos.borrow().iter().filter(|p| p.starts_with("clipes")).count(), 2);
    }

    #[test]
    fn gravacao_pequena_e_falha_da_twitch_nao_viram_trabalho() {
        let d = tmp("vigia-falha");
        let (agora, ts) = agora_teste();
        let mut cfg = padrao();
        cfg["canais"] = json!([{"login": "ana", "autorizado": true}]);
        let ativa = RefCell::new(true);
        let m = Mundo { ao_vivo: &|_| if *ativa.borrow() { Ok(Some(json!({}))) } else { Err("HTTP 500".into()) }, gravacao_ativa: &|_| *ativa.borrow(), iniciar_gravacao: &|_, v, _| std::fs::write(v, "curta").map_err(|e| e.to_string()),
                        parar_gravacao: &|_| {}, clipes: &|_, _, _, _| panic!("clipes desligado"), ultimo_vod: &|_| panic!("vods desligado") };
        let mut est = Estado::default();
        assert!(ciclo(&d, &mut est, &cfg, &agora, ts, &m).trabalhos.is_empty() && est.gravando.len() == 1);
        *ativa.borrow_mut() = false;
        assert!(ciclo(&d, &mut est, &cfg, &agora, ts + 60.0, &m).trabalhos.is_empty() && est.gravando.is_empty());
        assert!(std::fs::read_to_string(d.join("cortes/cortes.log")).unwrap().contains("checagem de ana falhou: HTTP 500"));
    }

    #[test]
    fn ligar_desligar_pausa_e_esperas() {
        let d = tmp("vigia-ligar");
        let cfg = padrao();
        let agora = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64();
        let subiu = RefCell::new(0);
        let subir = || {
            *subiu.borrow_mut() += 1;
            Ok(())
        };
        assert!(!rodando(&d, &cfg, agora));
        desligar(&d);
        assert!(deve_parar(&d));
        assert_eq!(ligar(&d, &cfg, agora, &subir), "vigia dos cortes ligado");
        assert!(!deve_parar(&d));
        bater(&d);
        assert!(rodando(&d, &cfg, agora) && !rodando(&d, &cfg, agora + 2.0 * 60.0 + 301.0));
        assert_eq!(ligar(&d, &cfg, agora, &subir), "o vigia dos cortes ja esta ligado");
        encerrar(&d);
        std::fs::write(d.join("cortes/PAUSA_ANDAR"), "").unwrap();
        assert!(ligar(&d, &cfg, agora, &subir).contains("pausada"));
        assert_eq!(*subiu.borrow(), 1);
        assert!(!pode_trabalhar(&cfg, true) && pode_trabalhar(&cfg, false) && pode_trabalhar(&json!({"pausar_no_modo_jogo": false}), true));
        let meta = json!({"clipes_cada_h": 6, "youtube_horarios": ["12:00", "19:00"]});
        assert_eq!((espera_clipes(&meta, 3), espera_clipes(&meta, 4), espera_clipes(&json!({"clipes_cada_h": 6}), 0)), (3600.0, 21600.0, 21600.0));
    }
}
