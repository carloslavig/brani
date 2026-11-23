//! Configuracao dos cortes de lives: dados/cortes/cortes.json (criado com o padrao na 1a leitura). Mesmo formato do Python.
//! Trava que nasce aqui: canal so e vigiado com "autorizado": true (o exemplo vem com false) e nada e publicado sem aprovacao
//! ("aprovacao_manual": true, YouTube "private"). Ficou de fora: `env` (chaves do .env); chave e token entram por closure em quem chama a rede.
#![allow(dead_code)]
use crate::empresa_dev::gravar;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

pub fn pasta(dados: &Path) -> PathBuf {
    dados.join("cortes")
}

pub fn arq(dados: &Path) -> PathBuf {
    pasta(dados).join("cortes.json")
}

pub fn padrao() -> Value {
    // em pedacos: um json! so com tudo estoura o limite de recursao da macro
    let vigia = json!({
        "canais": [{"login": "exemplo_canal", "autorizado": false, "politica": "", "live": true, "vods": false, "clipes": true}],
        "intervalo_min": 2, "clipes_cada_h": 6, "clipes_janela_h": 48, "clipes_max": 5, "clipes_min_views": 20,
        "clipes_ignorar_titulos": ["replay", "rerun", "reprise"],
        "manter_gravacoes_dias": 3, "pausar_no_modo_jogo": true, "avisar_telegram": true
    });
    let corte = json!({
        "cortes_por_live": 6, "duracao_min_s": 25, "duracao_max_s": 58, "antes_do_pico_s": 22, "depois_do_pico_s": 10, "atraso_chat_s": 6,
        "peso_audio": 1.0, "peso_chat": 1.4, "score_minimo": 2.0,
        "idioma": "pt", "whisper_modelo": "", "enquadramento": "desfocado", "legenda": true,
        "titulos_ollama": true, "ollama_modelo": "qwen2.5:3b"
    });
    let publicacao = json!({
        "aprovacao_manual": true, "youtube_privacidade": "private", "youtube_categoria": "20", "youtube_max_por_dia": 5,
        "youtube_horarios": [], "youtube_agendar_dias": 2, "tiktok_rascunho_api": false
    });
    let gringo = json!({
        "idioma": "en", "youtube_conta": "gringo", "youtube_privacidade": "private", "aprovacao_manual": true,
        "clipes_janela_h": 168, "clipes_cada_h": 6, "clipes_max": 5, "clipes_min_views": 2000, "titulos_ollama": true,
        "hashtags_fixas": ["#shorts", "#twitch", "#twitchclips", "#streamer"],
        "descricao_modelo": "{titulo}\n\n🎮 Streamer: {nome} — watch the full streams live at https://twitch.tv/{canal}\n{link_original}\nAll credit goes to {nome}. We share short highlights so more people discover the stream. If you are the creator and want this removed, comment or contact us and it will be taken down.\n\n🔔 Subscribe for the best Twitch moments every day.\n\n{hashtags}"
    });
    junta(&[&vigia, &corte, &publicacao, &json!({"perfis": {"gringo": gringo}})])
}

/// Junta objetos, o da direita por cima (so o primeiro nivel, como o `{**a, **b}` do Python).
pub fn junta(partes: &[&Value]) -> Value {
    let mut m = Map::new();
    for p in partes {
        if let Some(o) = p.as_object() {
            m.extend(o.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
    }
    Value::Object(m)
}

pub fn carregar(dados: &Path) -> Value {
    let a = arq(dados);
    if !a.exists() {
        let _ = gravar(&a, &padrao());
        return padrao();
    }
    // arquivo ilegivel = so o padrao (o BOM do Bloco de Notas nao pode derrubar a leitura)
    let cfg = std::fs::read_to_string(&a).ok().and_then(|t| serde_json::from_str::<Value>(t.trim_start_matches('\u{feff}')).ok()).unwrap_or(Value::Null);
    junta(&[&padrao(), &cfg])
}

/// Config efetiva de um canal: gerais + perfil + ajustes do proprio canal. Inclui "perfil", "canal" e "nome".
pub fn para_canal(canal: &Value, cfg: &Value) -> Value {
    let perfil = canal["perfil"].as_str().unwrap_or("");
    let mut e = junta(&[cfg, &cfg["perfis"][perfil], &canal["ajustes"], &json!({"perfil": perfil})]);
    if canal.is_object() {
        let login = canal["login"].as_str().unwrap_or("");
        e["canal"] = json!(login);
        e["nome"] = json!(canal["nome"].as_str().filter(|n| !n.is_empty()).unwrap_or(login));
        if let Some(i) = canal["idioma"].as_str().filter(|i| !i.is_empty()) {
            e["idioma"] = json!(i);
        }
    }
    e
}

pub fn do_perfil(cfg: &Value, perfil: &str) -> Value {
    junta(&[cfg, &cfg["perfis"][perfil], &json!({"perfil": perfil})])
}

pub fn canal_por_login(cfg: &Value, login: &str) -> Value {
    let achado = cfg["canais"].as_array().and_then(|cs| cs.iter().find(|c| c["login"].as_str().unwrap_or("").to_lowercase() == login.to_lowercase()));
    achado.cloned().unwrap_or_else(|| json!({"login": login}))
}

/// Subpasta de dados/cortes, criada se faltar.
pub fn sub(dados: &Path, nome: &str) -> PathBuf {
    let p = pasta(dados).join(nome);
    let _ = std::fs::create_dir_all(&p);
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(n: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-cortes_config-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn padrao_nasce_travado_e_arquivo_do_dono_vai_por_cima() {
        let d = tmp("a");
        let c = carregar(&d);
        assert!(arq(&d).exists());
        assert_eq!((c["aprovacao_manual"].as_bool(), c["youtube_privacidade"].as_str(), c["canais"][0]["autorizado"].as_bool()), (Some(true), Some("private"), Some(false)));
        std::fs::write(arq(&d), "\u{feff}{\"clipes_max\": 9}").unwrap();
        let c = carregar(&d);
        assert_eq!((c["clipes_max"].as_i64(), c["duracao_max_s"].as_i64()), (Some(9), Some(58)));
        std::fs::write(arq(&d), "{quebrado").unwrap();
        assert_eq!(carregar(&d)["clipes_max"].as_i64(), Some(5));
    }

    #[test]
    fn perfil_gringo_ingles_e_canal_sem_perfil_continua_pt() {
        let cfg = padrao();
        let c = para_canal(&json!({"login": "fulano", "nome": "Fulano", "perfil": "gringo"}), &cfg);
        assert_eq!((c["idioma"].as_str(), c["clipes_janela_h"].as_i64(), c["perfil"].as_str(), c["nome"].as_str()), (Some("en"), Some(168), Some("gringo"), Some("Fulano")));
        let x = para_canal(&json!({"login": "x"}), &cfg);
        assert_eq!((x["idioma"].as_str(), x["nome"].as_str(), x["perfil"].as_str()), (Some("pt"), Some("x"), Some("")));
        let aj = para_canal(&json!({"login": "y", "idioma": "es", "ajustes": {"clipes_max": 2}}), &cfg);
        assert_eq!((aj["idioma"].as_str(), aj["clipes_max"].as_i64()), (Some("es"), Some(2)));
        assert_eq!(do_perfil(&cfg, "gringo")["youtube_conta"].as_str(), Some("gringo"));
        assert_eq!(do_perfil(&cfg, "")["youtube_conta"].as_str(), None);
        let mut cfg2 = cfg.clone();
        cfg2["canais"] = json!([{"login": "Ana", "autorizado": true}]);
        assert_eq!(canal_por_login(&cfg2, "ana")["autorizado"].as_bool(), Some(true));
        assert_eq!(canal_por_login(&cfg2, "bia"), json!({"login": "bia"}));
    }
}
