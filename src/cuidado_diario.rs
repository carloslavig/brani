//! Cuidado diario da familia (porte de rotina.cuidado_diario do Braniac em Python): bom dia + "como voce esta?" 1x por dia por pessoa,
//! a partir de uma hora e ate as 21h. Nomes, contatos, horarios e textos NAO estao no codigo: vem de `dados/cuidado_diario.json`
//! (`rotinas`: chave, apelido, hora, so_audio, textos por dia da semana, `apos` {data, textos} e `sufixo`) e do contato em `pessoal.json`.
//! Audio primeiro (voz do Jarvis; sem emojis), texto de reserva se o audio falhar. `so_audio` (o pai): voz do agente "zelo", NUNCA texto;
//! se falhar tenta de novo daqui a 10 min. Estado em `dados/cuidado_estado.json`; no dia da troca, o que o Braniac antigo ja mandou
//! hoje (`rotina.json` ao lado do .env dele, so leitura) conta como feito. Nao passa pelo funil. Quem chama so chama com a chave
//! `whatsapp` (todos) ligada: terceiro nunca recebe com a chave restrita ou desligada.
use crate::contexto::Ctx;
use crate::tempo::Agora;
use serde_json::{json, Value};
use std::path::Path;

const ESPERA_FALHA_S: f64 = 600.0;

pub struct Canais<'a> {
    pub texto: &'a dyn Fn(&str, &str) -> String,
    /// (agente da voz, nome, texto)
    pub audio: &'a dyn Fn(&str, &str, &str) -> String,
}

fn json_de(p: &Path) -> Value {
    std::fs::read_to_string(p).ok().and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}))
}

fn do_antigo(chave: &str) -> Option<String> {
    let env = std::env::var_os("BRANIAC_ENV")?;
    let v = json_de(&Path::new(&env).with_file_name("data").join("rotina.json"));
    v[chave].as_str().map(String::from)
}

/// O texto de hoje de uma rotina (indice = dia da semana, 0 = segunda, como no Python), com a troca pos-data e o sufixo.
fn texto_de(r: &Value, agora: &Agora) -> Option<String> {
    let hoje = agora.data.iso();
    let lista = match r["apos"]["data"].as_str() {
        Some(d) if hoje.as_str() > d => &r["apos"]["textos"],
        _ => &r["textos"],
    };
    let l = lista.as_array().filter(|l| !l.is_empty())?;
    let t = l[agora.data.semana() % l.len()].as_str()?.to_string();
    Some(format!("{t}{}", r["sufixo"].as_str().unwrap_or("")))
}

/// Pendente agora? (barato: o despachante usa para decidir se pega a trava do WhatsApp)
pub fn ha_pendente(dados: &Path, agora: &Agora, ts: f64) -> bool {
    let (cfg, est, ctx) = (json_de(&dados.join("cuidado_diario.json")), json_de(&dados.join("cuidado_estado.json")), Ctx::ler(dados));
    let x = pendentes(&cfg, &est, &ctx, agora, ts).next().is_some();
    x
}

fn pendentes<'a>(cfg: &'a Value, est: &'a Value, ctx: &'a Ctx, agora: &'a Agora, ts: f64) -> impl Iterator<Item = &'a Value> + 'a {
    let (hoje, ate) = (agora.data.iso(), cfg["ate_hora"].as_u64().unwrap_or(21) as u32);
    cfg["rotinas"].as_array().into_iter().flatten().filter(move |r| {
        let chave = r["chave"].as_str().unwrap_or("");
        let hora = r["hora"].as_u64().unwrap_or(99) as u32;
        !chave.is_empty() && hora <= agora.hora && agora.hora < ate && est[chave].as_str() != Some(hoje.as_str()) && ts >= est["prox"][chave].as_f64().unwrap_or(0.0)
            && r["apelido"].as_str().and_then(|a| ctx.contato(a)).is_some() && do_antigo(chave).as_deref() != Some(hoje.as_str())
    })
}

/// UM passo. Devolve os apelidos que receberam agora.
pub fn passo(dados: &Path, agora: &Agora, ts: f64, ctx: &Ctx, c: &Canais) -> Vec<String> {
    let cfg = json_de(&dados.join("cuidado_diario.json"));
    let mut est = json_de(&dados.join("cuidado_estado.json"));
    let hoje = agora.data.iso();
    let mut feitos = vec![];
    let lista: Vec<Value> = pendentes(&cfg, &est, ctx, agora, ts).cloned().collect();
    for r in lista {
        let (chave, apelido) = (r["chave"].as_str().unwrap_or(""), r["apelido"].as_str().unwrap_or(""));
        let (Some(nome), Some(texto)) = (ctx.contato(apelido), texto_de(&r, agora)) else { continue };
        let falado = ["☀️", "🌞", "💛", "🌷", "\u{fe0f}"].iter().fold(texto.clone(), |t, e| t.replace(e, "")).trim().to_string();
        let so_audio = r["so_audio"].as_bool().unwrap_or(false);
        let audio_ok = (c.audio)(if so_audio { "zelo" } else { "jarvis" }, nome, &falado).starts_with('✅');
        let ok = audio_ok || (!so_audio && (c.texto)(nome, &texto).starts_with('✅')); // so_audio: nunca cai para texto
        if ok {
            est[chave] = json!(hoje);
            let _ = crate::memoria::registrar_msg(dados, nome, "C", &texto, "whatsapp", audio_ok, ts);
            feitos.push(apelido.to_string());
        } else {
            est["prox"][chave] = json!(ts + ESPERA_FALHA_S);
        }
    }
    if !feitos.is_empty() || est["prox"].is_object() {
        let _ = std::fs::write(dados.join("cuidado_estado.json"), est.to_string());
    }
    feitos
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;
    use std::cell::RefCell;

    fn agora(dia: i64, hora: u32) -> Agora {
        Agora { data: Data::civil(2026, 11, dia).unwrap(), hora, min: 0, seg: 0 }
    }

    fn pasta(n: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-cuid-{n}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("pessoal.json"), json!({ "nome": "Dono", "contatos": { "dono": "Eu(D)", "parceira": "Parceira", "mae": "Mae", "pai": "Pai" } }).to_string()).unwrap();
        std::fs::write(d.join("cuidado_diario.json"), json!({ "ate_hora": 21, "rotinas": [
            { "chave": "c_jana", "apelido": "parceira", "hora": 7, "textos": ["bom dia A ☀️"] },
            { "chave": "c_mae", "apelido": "mae", "hora": 8, "textos": ["bom dia B. E o braço?"], "apos": { "data": "2026-11-15", "textos": ["bom dia B."] }, "sufixo": " Remédio?" },
            { "chave": "c_pai", "apelido": "pai", "hora": 9, "so_audio": true, "textos": ["bom dia C"] } ] }).to_string()).unwrap();
        d
    }

    #[derive(Default)]
    struct Reg {
        v: RefCell<Vec<String>>,
        audio_falha: RefCell<bool>,
        texto_falha: RefCell<bool>,
    }

    fn rodar(d: &Path, r: &Reg, a: &Agora, ts: f64) -> Vec<String> {
        let texto = |n: &str, t: &str| { r.v.borrow_mut().push(format!("texto|{n}|{t}")); if *r.texto_falha.borrow() { "falhou".into() } else { "✅".to_string() } };
        let audio = |g: &str, n: &str, t: &str| { r.v.borrow_mut().push(format!("audio|{g}|{n}|{t}")); if *r.audio_falha.borrow() { "falhou".into() } else { "✅".to_string() } };
        passo(d, a, ts, &Ctx::ler(d), &Canais { texto: &texto, audio: &audio })
    }

    #[test]
    fn cada_um_recebe_uma_vez_no_horario_certo() {
        let d = pasta("hora");
        let r = Reg::default();
        assert!(rodar(&d, &r, &agora(21, 6), 1.0).is_empty()); // cedo demais
        assert_eq!(rodar(&d, &r, &agora(21, 7), 2.0), vec!["parceira"]);
        assert_eq!(rodar(&d, &r, &agora(21, 9), 3.0), vec!["mae", "pai"]);
        assert!(rodar(&d, &r, &agora(21, 10), 4.0).is_empty(), "1x por dia");
        assert!(rodar(&d, &r, &agora(21, 22), 5.0).is_empty());
        assert_eq!(rodar(&d, &r, &agora(22, 8), 6.0), vec!["parceira", "mae"]); // dia seguinte
        let v = r.v.borrow().clone();
        assert_eq!(v[0], "audio|jarvis|Parceira|bom dia A"); // sem emoji na voz
        assert_eq!(v[1], "audio|jarvis|Mae|bom dia B. Remédio?"); // depois da data: sem a pergunta antiga; com o sufixo
        assert_eq!(v[2], "audio|zelo|Pai|bom dia C");
        assert!(!v.iter().any(|x| x.starts_with("texto")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn pai_nunca_recebe_texto_e_o_audio_que_falha_e_tentado_de_novo() {
        let d = pasta("pai");
        let r = Reg::default();
        *r.audio_falha.borrow_mut() = true;
        assert_eq!(rodar(&d, &r, &agora(21, 10), 100.0), vec!["parceira", "mae"]);
        let v = r.v.borrow().clone();
        assert!(v.iter().filter(|x| x.contains("|Pai|")).all(|x| x.starts_with("audio|zelo")), "{v:?}");
        // jana e mae cairam no texto de reserva; o pai nao
        assert_eq!(v.iter().filter(|x| x.starts_with("texto")).count(), 2);
        r.v.borrow_mut().clear();
        assert!(rodar(&d, &r, &agora(21, 10), 200.0).is_empty() && r.v.borrow().is_empty(), "espera antes de tentar de novo");
        *r.audio_falha.borrow_mut() = false;
        assert_eq!(rodar(&d, &r, &agora(21, 10), 800.0), vec!["pai"]);
        assert_eq!(*r.v.borrow(), vec!["audio|zelo|Pai|bom dia C"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn audio_falhou_cai_no_texto_com_emoji_para_quem_pode() {
        let d = pasta("reserva");
        let r = Reg::default();
        *r.audio_falha.borrow_mut() = true;
        assert_eq!(rodar(&d, &r, &agora(21, 8), 1.0), vec!["parceira", "mae"]);
        assert!(r.v.borrow().contains(&"texto|Parceira|bom dia A ☀️".to_string()));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn sem_contato_ou_sem_arquivo_nao_faz_nada() {
        let d = pasta("sem");
        std::fs::write(d.join("pessoal.json"), json!({ "nome": "Dono", "contatos": { "dono": "Eu(D)" } }).to_string()).unwrap();
        let r = Reg::default();
        assert!(rodar(&d, &r, &agora(21, 12), 1.0).is_empty() && r.v.borrow().is_empty());
        let _ = std::fs::remove_file(d.join("cuidado_diario.json"));
        assert!(rodar(&d, &r, &agora(21, 12), 1.0).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
}
