//! Catalogo de vozes (braniac/vozes_catalogo.py): cada MODELO de voz com as vozes de dentro dele e a regra de quem pode escolher o que:
//! motor que passa de 30 s fica desligado (nem como reserva), voz tem de existir e estar instalada, agente de um genero so nao aceita o outro.
//! De fora (entram como dado ou closure): a lista de vozes do Edge e do Kokoro, saber se o modelo do Piper esta no disco, gerar a amostra
//! e baixar o modelo (aqui so os enderecos). O texto da amostra nao leva nome de pessoa nem compromisso.
#![allow(dead_code)]
use crate::vozes_agentes;
use serde_json::{json, Value};
use std::path::Path;

pub const LIMITE_S: f64 = 30.0;
pub const TEXTO: &str = "Oi. Esta é a minha voz. Qualquer coisa, é só me chamar.";
const GEMINI_F: [&str; 14] = ["Achernar", "Aoede", "Autonoe", "Callirrhoe", "Despina", "Erinome", "Gacrux", "Kore", "Laomedeia", "Leda", "Pulcherrima", "Sulafat", "Vindemiatrix", "Zephyr"];
const GEMINI_M: [&str; 16] = ["Puck", "Charon", "Fenrir", "Orus", "Enceladus", "Iapetus", "Umbriel", "Algieba", "Algenib", "Rasalgethi", "Alnilam", "Schedar", "Achird", "Zubenelgenubi", "Sadachbia", "Sadaltager"];
// vozes do Piper em portugues do Brasil; todas masculinas
const PIPER: [(&str, &str); 4] = [("faber/medium", "Faber"), ("cadu/medium", "Cadu"), ("jeff/medium", "Jeff"), ("edresson/low", "Edresson (qualidade baixa)")];
const COMBINACOES: [(&str, &str, &str); 9] = [("Gemini, Kore", "gemini", "Kore"), ("Gemini, Aoede", "gemini", "Aoede"), ("Gemini, Leda", "gemini", "Leda"),
    ("Gemini, Zephyr", "gemini", "Zephyr"), ("Gemini, Sulafat", "gemini", "Sulafat"), ("Gemini, Callirrhoe", "gemini", "Callirrhoe"),
    ("Edge, Francisca", "edge", "pt-BR-FranciscaNeural"), ("Edge, Thalita", "edge", "pt-BR-ThalitaMultilingualNeural"), ("Kokoro, pf_dora", "kokoro", "pf_dora")];

/// O que so quem liga sabe: vozes do Edge (nome curto, "Female"/"Male", idioma), vozes do Kokoro e se um modelo do Piper esta instalado.
pub struct Fontes<'a> {
    pub edge: &'a [(String, String, String)],
    pub kokoro: &'a [String],
    pub piper_instalada: &'a dyn Fn(&str) -> bool,
}

/// Sem internet: so as de portugues.
pub fn edge_minimo() -> Vec<(String, String, String)> {
    [("pt-BR-AntonioNeural", "Male", "pt-BR"), ("pt-BR-FranciscaNeural", "Female", "pt-BR"), ("pt-BR-ThalitaMultilingualNeural", "Female", "pt-BR"),
        ("pt-PT-DuarteNeural", "Male", "pt-PT"), ("pt-PT-RaquelNeural", "Female", "pt-PT")].iter().map(|(n, g, l)| (n.to_string(), g.to_string(), l.to_string())).collect()
}

fn v(id: &str, nome: &str, genero: &str, idioma: &str, instalada: bool) -> Value {
    json!({ "id": id, "nome": nome, "genero": genero, "idioma": idioma, "instalada": instalada })
}

fn lingua(c: char) -> String {
    match c {
        'a' => "inglês EUA", 'b' => "inglês UK", 'e' => "espanhol", 'f' => "francês", 'h' => "hindi", 'i' => "italiano", 'j' => "japonês", 'p' => "português", 'z' => "chinês",
        _ => return c.to_string(),
    }.to_string()
}

fn motores(f: &Fontes) -> Vec<Value> {
    let mut kok: Vec<&String> = f.kokoro.iter().filter(|n| n.chars().count() >= 2).collect();
    kok.sort_by_key(|n| (!n.starts_with('p'), n.to_string())); // portugues primeiro
    let kok: Vec<Value> = kok.iter().map(|n| {
        let mut c = n.chars();
        let (l, g) = (c.next().unwrap_or(' '), c.next().unwrap_or(' '));
        v(n, n, if g == 'f' { "F" } else { "M" }, &lingua(l), true)
    }).collect();
    let mut ed: Vec<Value> = f.edge.iter().map(|(curto, genero, idioma)| {
        let nome = curto.splitn(3, '-').nth(2).unwrap_or(curto).replace("Neural", "");
        v(curto, &nome, if genero == "Female" { "F" } else { "M" }, idioma, true)
    }).collect();
    let s = |x: &Value, k: &str| x[k].as_str().unwrap_or("").to_string();
    ed.sort_by_key(|x| (!s(x, "idioma").starts_with("pt"), s(x, "idioma"), s(x, "nome")));
    let ge: Vec<Value> = GEMINI_F.iter().map(|n| v(n, n, "F", "multilíngue", true)).chain(GEMINI_M.iter().map(|n| v(n, n, "M", "multilíngue", true))).collect();
    let pi: Vec<Value> = PIPER.iter().map(|(i, n)| v(i, n, "M", "português BR", (f.piper_instalada)(i))).collect();
    vec![
        json!({ "id": "kokoro", "nome": "Kokoro", "onde": "Local (CPU), sem limite de uso", "lento": false, "vozes": kok }),
        json!({ "id": "piper", "nome": "Piper", "onde": "Local (CPU), sem limite de uso", "lento": false, "vozes": pi }),
        json!({ "id": "edge", "nome": "Edge TTS (Microsoft)", "onde": "Online, sem chave nem cota (depende da Microsoft)", "lento": false, "vozes": ed }),
        json!({ "id": "gemini", "nome": "Gemini TTS (Google)", "onde": "Online, com cota por chave", "lento": false, "vozes": ge }),
        json!({ "id": "rafael", "nome": "Rafael (Pocket TTS)", "onde": "Local, mas leva ~60 s: passa dos 30 s", "lento": true, "vozes": [v("rafael", "Rafael", "M", "português", true)] }),
        json!({ "id": "xtts", "nome": "XTTS-v2 (placa de vídeo)", "onde": "Local, mas leva ~110 s: passa dos 30 s", "lento": true, "vozes": [v("referencia", "Clone de voz (referência)", "M", "português", true)] }),
    ]
}

pub fn catalogo(dados: &Path, f: &Fontes) -> Value {
    let presets = vozes_agentes::presets();
    let combos: Vec<Value> = COMBINACOES.iter().map(|(n, m, vz)| json!({ "id": format!("{m}:{vz}"), "nome": format!("{n}, sedutora"), "motor": m, "voz": vz, "ajustes": presets["sedutora"]["ajustes"] })).collect();
    let agentes: Vec<Value> = vozes_agentes::agentes(dados).into_iter().map(|mut a| {
        a["escolha"] = vozes_agentes::escolha(dados, a["id"].as_str().unwrap_or(""));
        a
    }).collect();
    json!({ "presets": presets, "combinacoes": combos, "motores": motores(f), "escolha": vozes_agentes::escolha(dados, "jarvis"), "limite_s": LIMITE_S, "agentes": agentes })
}

/// (wav, segundos que levou). Passou de LIMITE_S = erro: essa voz nao serve.
/// shortcut: mede depois que o motor devolve (nao interrompe o motor no segundo 30); trocar por processo com prazo se algum motor travar.
pub fn amostra(gerar: &dyn Fn() -> Result<Vec<u8>, String>) -> Result<(Vec<u8>, f64), String> {
    let t0 = std::time::Instant::now();
    let wav = gerar()?;
    let s = t0.elapsed().as_secs_f64();
    if s > LIMITE_S {
        return Err(format!("passou de {LIMITE_S} s: essa voz não serve"));
    }
    Ok((wav, (s * 10.0).round() / 10.0))
}

/// posicao 1 = primaria, 2 = secundaria, 3 = terciaria. So grava voz que existe, esta instalada, responde em 30 s e e do genero do agente.
pub fn escolher(dados: &Path, f: &Fontes, motor: &str, voz_id: &str, agente: &str, posicao: i64) -> Result<Value, String> {
    let ms = motores(f);
    let m = ms.iter().find(|m| m["id"] == motor && m["lento"] == false).ok_or("esse motor passa de 30 s e está desligado")?;
    let voz = m["vozes"].as_array().into_iter().flatten().find(|x| x["id"] == voz_id && x["instalada"] == true).ok_or("voz inexistente ou não instalada")?;
    let genero = voz["genero"].as_str().unwrap_or("");
    if let Some(so) = vozes_agentes::so_genero(dados, agente).filter(|so| so != genero) {
        return Err(if so == "F" { "esse agente só usa voz feminina" } else { "esse agente só usa voz masculina" }.into());
    }
    vozes_agentes::escolher(dados, agente, motor, voz_id, genero, posicao)
}

/// Enderecos (modelo e configuracao) do Piper no acervo publico. Baixar e com quem liga, e so a pedido do dono.
pub fn piper_urls(voz_id: &str) -> Result<[String; 2], String> {
    if !PIPER.iter().any(|(i, _)| *i == voz_id) {
        return Err("voz do Piper desconhecida".into());
    }
    let (nome, qual) = voz_id.split_once('/').ok_or("voz do Piper desconhecida")?;
    let base = format!("https://huggingface.co/rhasspy/piper-voices/resolve/main/pt/pt_BR/{nome}/{qual}/pt_BR-{nome}-{qual}.onnx");
    Ok([base.clone(), base + ".json"])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vozes_agentes as va;

    #[test]
    fn so_escolhe_voz_que_existe_rapida_instalada_e_do_genero_do_agente() {
        let d = std::env::temp_dir().join(format!("braniac-vozes_catalogo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let edge: Vec<(String, String, String)> = [("pt-BR-FranciscaNeural", "Female", "pt-BR"), ("pt-BR-AntonioNeural", "Male", "pt-BR"), ("en-US-AvaNeural", "Female", "en-US")]
            .iter().map(|(a, b, c)| (a.to_string(), b.to_string(), c.to_string())).collect();
        let kokoro = vec!["af_sky".to_string(), "pf_dora".to_string(), "pm_alex".to_string()];
        let f = Fontes { edge: &edge, kokoro: &kokoro, piper_instalada: &|id| id == "faber/medium" };
        escolher(&d, &f, "gemini", "Kore", "alma", 1).unwrap();
        escolher(&d, &f, "edge", "pt-BR-FranciscaNeural", "alma", 2).unwrap();
        escolher(&d, &f, "kokoro", "pf_dora", "alma", 3).unwrap();
        assert_eq!(va::reservas(&d, "alma").iter().map(|r| r["motor"].as_str().unwrap()).collect::<Vec<_>>(), ["edge", "kokoro"]);
        assert_eq!(escolher(&d, &f, "edge", "pt-BR-AntonioNeural", "alma", 2).unwrap_err(), "esse agente só usa voz feminina"); // tambem na reserva
        assert!(escolher(&d, &f, "xtts", "referencia", "jarvis", 2).unwrap_err().contains("30 s")); // passa de 30 s: nem como reserva
        assert!(escolher(&d, &f, "rafael", "rafael", "jarvis", 1).is_err() && escolher(&d, &f, "nenhum", "x", "jarvis", 1).is_err());
        assert_eq!(escolher(&d, &f, "piper", "cadu/medium", "jarvis", 1).unwrap_err(), "voz inexistente ou não instalada");
        assert_eq!(escolher(&d, &f, "piper", "faber/medium", "jarvis", 1).unwrap()["genero"], "M");
        assert!(escolher(&d, &f, "gemini", "Inexistente", "jarvis", 1).is_err() && escolher(&d, &f, "gemini", "Kore", "fantasma", 1).is_err());
        let c = catalogo(&d, &f);
        let nomes = |i: usize| c["motores"][i]["vozes"].as_array().unwrap().iter().map(|x| x["id"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        assert_eq!(nomes(0), ["pf_dora", "pm_alex", "af_sky"]); // portugues primeiro
        assert_eq!(nomes(2), ["pt-BR-AntonioNeural", "pt-BR-FranciscaNeural", "en-US-AvaNeural"]);
        assert!(c["motores"][2]["vozes"][1]["nome"] == "Francisca" && c["motores"][0]["vozes"][2]["idioma"] == "inglês EUA" && c["limite_s"] == 30.0);
        assert!(c["agentes"][1]["escolha"]["motor"] == "gemini" && c["combinacoes"].as_array().unwrap().len() == 9 && c["escolha"]["motor"] == "piper");
        assert!(c["combinacoes"].as_array().unwrap().iter().all(|x| GEMINI_F.contains(&x["voz"].as_str().unwrap()) || x["motor"] != "gemini")); // combinacoes da Alma: so voz feminina
        assert_eq!(amostra(&|| Ok(vec![1, 2])).unwrap().0, [1, 2]);
        assert!(amostra(&|| Err("caiu".into())).is_err());
        assert!(piper_urls("faber/medium").unwrap()[1].ends_with("pt_BR-faber-medium.onnx.json") && piper_urls("../x").is_err() && piper_urls("outra/medium").is_err());
        assert_eq!(edge_minimo().len(), 5);
        let _ = std::fs::remove_dir_all(&d);
    }
}
