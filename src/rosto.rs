//! Reconhecimento de rosto 100% LOCAL (sem IA online, sem token): YuNet acha o rosto, SFace gera 128 numeros, cosseno compara
//! com as pessoas cadastradas. Camera e fotos entram por ffmpeg (sem DLL). Camera so liga quando o Jarvis e chamado, por ~2 s.
//! Pecas puras (cosseno, NMS, alinhamento) vieram do NexoDev: rosto_nucleo.rs e rosto_detector.rs. Dados em `dados/rosto/`.
//! Desligado por padrao: so age com `dados/rosto/config.json` {"ligado": true}. Serve para CUMPRIMENTAR, nunca para liberar acao grave.
use crate::oculto;
use crate::rosto_detector::{decodificar, nms, Rosto};
use crate::rosto_nucleo::{alinhar, cosseno, identificar, normalizar_l2, Pessoa, Veredito};
use serde_json::{json, Value};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use tract_onnx::prelude::*;

const LARG: usize = 640;
const ALT: usize = 480;
type Plano = TypedRunnableModel<TypedModel>;
struct Modelos {
    det: Plano,
    rec: Plano,
}
static MODELOS: OnceLock<Result<Modelos, String>> = OnceLock::new();
static OCUPADO: Mutex<()> = Mutex::new(());

fn pasta(dados: &Path) -> PathBuf {
    dados.join("rosto")
}

fn carregar(dados: &Path) -> &'static Result<Modelos, String> {
    MODELOS.get_or_init(|| {
        let d = dados.join("modelos_rosto");
        let abrir = |nome: &str, forma: [usize; 4]| -> Result<Plano, String> {
            tract_onnx::onnx()
                .model_for_path(d.join(nome))
                .and_then(|m| m.with_input_fact(0, f32::fact(forma).into()))
                .and_then(|m| m.into_optimized())
                .and_then(|m| m.into_runnable())
                .map_err(|e| format!("modelo {nome}: {e}"))
        };
        Ok(Modelos { det: abrir("yunet.onnx", [1, 3, 640, 640])?, rec: abrir("sface.onnx", [1, 3, 112, 112])? })
    })
}

/// Parametros editaveis em `dados/rosto/config.json`.
pub struct Config {
    pub ligado: bool,
    pub camera: String,
    pub limiar: f32,
    pub margem: f32,
    pub deteccao: f32,
}
pub fn config(dados: &Path) -> Config {
    let v: Value = std::fs::read_to_string(pasta(dados).join("config.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(json!({}));
    Config {
        ligado: v["ligado"].as_bool().unwrap_or(false),
        camera: v["camera"].as_str().unwrap_or("HD User Facing").to_string(),
        limiar: v["limiar"].as_f64().unwrap_or(0.45) as f32,
        margem: v["margem"].as_f64().unwrap_or(0.06) as f32,
        deteccao: v["deteccao"].as_f64().unwrap_or(0.6) as f32,
    }
}

/// Ate N quadros RGB 640x480 (letterbox, mantem a proporcao). `entrada` = ["-f","dshow","-i","video=..."] ou ["-i","foto.jpg"].
fn ffmpeg_quadros(entrada: &[&str], quadros: usize, extra: &[&str]) -> Result<Vec<Vec<u8>>, String> {
    let filtro = format!("scale={LARG}:{ALT}:force_original_aspect_ratio=decrease,pad={LARG}:{ALT}:(ow-iw)/2:(oh-ih)/2");
    let mut c = oculto::cmd("ffmpeg");
    c.args(["-hide_banner", "-loglevel", "error"]).args(extra).args(entrada).args(["-vf", &filtro, "-f", "rawvideo", "-pix_fmt", "rgb24", "-"]);
    c.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).stdin(std::process::Stdio::null());
    let mut filho = c.spawn().map_err(|e| format!("ffmpeg nao abriu: {e}"))?;
    let mut o = filho.stdout.take().ok_or("sem saida")?;
    let tam = LARG * ALT * 3;
    let mut buf = vec![0u8; tam];
    let mut todos: Vec<Vec<u8>> = Vec::new();
    while o.read_exact(&mut buf).is_ok() {
        todos.push(buf.clone());
        if todos.len() >= quadros + 40 {
            break;
        }
    }
    let _ = filho.kill();
    let _ = filho.wait();
    if todos.is_empty() {
        return Err("a camera nao entregou imagem (ocupada, desligada ou nome errado)".into());
    }
    let ini = todos.len().saturating_sub(quadros);
    Ok(todos.split_off(ini))
}

/// Camera: sobe e estabiliza a exposicao (descarta os primeiros), devolve os 3 ultimos quadros.
fn capturar(camera: &str) -> Result<Vec<Vec<u8>>, String> {
    let nome = format!("video={camera}");
    ffmpeg_quadros(&["-f", "dshow", "-video_size", "640x480", "-framerate", "30", "-i", &nome], 3, &["-t", "3"])
}

fn ler_foto(caminho: &str) -> Result<Vec<u8>, String> {
    ffmpeg_quadros(&["-i", caminho], 1, &["-frames:v", "1"]).map(|mut v| v.remove(0))
}

fn detectar(m: &Modelos, rgb: &[u8], limiar: f32) -> Result<Vec<Rosto>, String> {
    // YuNet quer BGR 0..255, NCHW, 640x640; a imagem fica no canto e o resto zerado (coordenadas nao mudam)
    let mut dados = vec![0f32; 3 * 640 * 640];
    for y in 0..ALT {
        for x in 0..LARG {
            let i = (y * LARG + x) * 3;
            let (r, g, b) = (rgb[i] as f32, rgb[i + 1] as f32, rgb[i + 2] as f32);
            dados[y * 640 + x] = b;
            dados[640 * 640 + y * 640 + x] = g;
            dados[2 * 640 * 640 + y * 640 + x] = r;
        }
    }
    let t: Tensor = tract_ndarray::Array4::from_shape_vec((1, 3, 640, 640), dados).map_err(|e| e.to_string())?.into();
    let r = m.det.run(tvec!(t.into())).map_err(|e| format!("detector: {e}"))?;
    let pega = |i: usize| -> Result<Vec<f32>, String> { Ok(r[i].to_array_view::<f32>().map_err(|e| e.to_string())?.iter().copied().collect()) };
    let g = |ini: usize| -> Result<[Vec<f32>; 3], String> { Ok([pega(ini)?, pega(ini + 1)?, pega(ini + 2)?]) };
    let rostos = decodificar(&g(0)?, &g(3)?, &g(6)?, &g(9)?, 640, limiar);
    Ok(nms(rostos, 0.3))
}

fn maior(rostos: Vec<Rosto>) -> Option<Rosto> {
    rostos.into_iter().max_by(|a, b| (a.l * a.a).partial_cmp(&(b.l * b.a)).unwrap_or(std::cmp::Ordering::Equal))
}

fn vetor(m: &Modelos, rgb: &[u8], r: &Rosto) -> Result<Vec<f32>, String> {
    let hwc = alinhar(rgb, LARG, ALT, r.pontos); // 112*112*3, BGR intercalado
    let mut chw = vec![0f32; 3 * 112 * 112];
    for p in 0..112 * 112 {
        for c in 0..3 {
            chw[c * 112 * 112 + p] = hwc[p * 3 + c];
        }
    }
    let t: Tensor = tract_ndarray::Array4::from_shape_vec((1, 3, 112, 112), chw).map_err(|e| e.to_string())?.into();
    let s = m.rec.run(tvec!(t.into())).map_err(|e| format!("reconhecedor: {e}"))?;
    let mut v: Vec<f32> = s[0].to_array_view::<f32>().map_err(|e| e.to_string())?.iter().copied().collect();
    normalizar_l2(&mut v);
    Ok(v)
}

pub fn pessoas(dados: &Path) -> Vec<Pessoa> {
    let v: Value = std::fs::read_to_string(pasta(dados).join("pessoas.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(json!({}));
    v.as_object()
        .map(|o| {
            o.iter()
                .map(|(nome, vs)| Pessoa {
                    nome: nome.clone(),
                    vetores: vs.as_array().map(|a| a.iter().map(|x| x.as_array().map(|y| y.iter().filter_map(|n| n.as_f64().map(|f| f as f32)).collect()).unwrap_or_default()).collect()).unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn gravar(dados: &Path, gente: &[Pessoa]) -> Result<(), String> {
    let mut o = serde_json::Map::new();
    for p in gente {
        o.insert(p.nome.clone(), json!(p.vetores));
    }
    std::fs::create_dir_all(pasta(dados)).map_err(|e| e.to_string())?;
    let tmp = pasta(dados).join("pessoas.json.tmp");
    std::fs::write(&tmp, serde_json::to_string(&Value::Object(o)).unwrap()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, pasta(dados).join("pessoas.json")).map_err(|e| e.to_string())
}

/// Cadastra (ou reforca) uma pessoa a partir de fotos. So guarda os 128 numeros, nunca a foto. Foto sem exatamente um rosto nitido e recusada.
pub fn cadastrar(dados: &Path, nome: &str, fotos: &[String]) -> Result<Value, String> {
    let nome = nome.trim().to_lowercase();
    if nome.is_empty() || fotos.is_empty() {
        return Err("mande {\"nome\": \"usuarios\", \"fotos\": [\"caminho.jpg\", ...]}".into());
    }
    let _g = OCUPADO.lock().map_err(|e| e.to_string())?;
    let m = carregar(dados).as_ref().map_err(|e| e.clone())?;
    let cfg = config(dados);
    let mut gente = pessoas(dados);
    let mut aceitas = 0;
    let mut recusadas = Vec::new();
    let mut novos = Vec::new();
    for f in fotos {
        let res = ler_foto(f).and_then(|rgb| {
            let rs = detectar(m, &rgb, cfg.deteccao)?;
            match rs.len() {
                0 => Err("nenhum rosto".to_string()),
                1 => vetor(m, &rgb, &rs[0]),
                _ => maior(rs).map(|r| vetor(m, &rgb, &r)).unwrap_or(Err("sem rosto".into())),
            }
        });
        match res {
            Ok(v) => {
                novos.push(v);
                aceitas += 1;
            }
            Err(e) => recusadas.push(json!({ "foto": f, "motivo": e })),
        }
    }
    if aceitas > 0 {
        match gente.iter_mut().find(|p| p.nome == nome) {
            Some(p) => p.vetores.extend(novos),
            None => gente.push(Pessoa { nome: nome.clone(), vetores: novos }),
        }
        gravar(dados, &gente)?;
    }
    Ok(json!({ "nome": nome, "aceitas": aceitas, "recusadas": recusadas, "total_da_pessoa": gente.iter().find(|p| p.nome == nome).map(|p| p.vetores.len()).unwrap_or(0) }))
}

/// Cadastra a pessoa que esta na frente da camera agora (3 rodadas de ~3 s, 2 s entre elas; peca para virar a cabeca um pouco).
/// Quadro sem rosto ou com mais de um rosto nao entra: so a cara maior, e so se houver um unico rosto.
pub fn cadastrar_camera(dados: &Path, nome: &str) -> Result<Value, String> {
    let nome = nome.trim().to_lowercase();
    if nome.is_empty() {
        return Err("mande {\"nome\": \"usuarios\", \"camera\": true}".into());
    }
    let _g = OCUPADO.lock().map_err(|e| e.to_string())?;
    let m = carregar(dados).as_ref().map_err(|e| e.clone())?;
    let cfg = config(dados);
    let mut novos = Vec::new();
    for rodada in 0..3 {
        if rodada > 0 {
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
        for q in capturar(&cfg.camera)? {
            let rs = detectar(m, &q, cfg.deteccao)?;
            if rs.len() == 1 {
                novos.push(vetor(m, &q, &rs[0])?);
            }
        }
    }
    if novos.len() < 3 {
        return Err(format!("so vi um rosto nitido em {} quadros de 9; fique de frente, com luz, sozinho na imagem", novos.len()));
    }
    let mut gente = pessoas(dados);
    let n = novos.len();
    match gente.iter_mut().find(|p| p.nome == nome) {
        Some(p) => p.vetores.extend(novos),
        None => gente.push(Pessoa { nome: nome.clone(), vetores: novos }),
    }
    gravar(dados, &gente)?;
    Ok(json!({ "nome": nome, "quadros_aceitos": n }))
}

/// Resultado de olhar para a camera.
#[derive(Debug, Clone, PartialEq)]
pub enum Visto {
    Conhecido(String, f32),
    Desconhecido,
    Ninguem,
}

/// Consenso de varios quadros: o mesmo nome em pelo menos 2 deles; rostos sem consenso = desconhecido; sem rosto = ninguem.
pub fn consenso(vereditos: &[Option<Veredito>]) -> Visto {
    let com_rosto: Vec<&Veredito> = vereditos.iter().flatten().collect();
    if com_rosto.is_empty() {
        return Visto::Ninguem;
    }
    let mut contagem: Vec<(String, usize, f32)> = Vec::new();
    for v in &com_rosto {
        if let Veredito::Conhecido { nome, pontuacao } = v {
            match contagem.iter_mut().find(|c| &c.0 == nome) {
                Some(c) => {
                    c.1 += 1;
                    c.2 = c.2.max(*pontuacao);
                }
                None => contagem.push((nome.clone(), 1, *pontuacao)),
            }
        }
    }
    match contagem.into_iter().filter(|c| c.1 >= 2).max_by_key(|c| c.1) {
        Some((n, _, p)) => Visto::Conhecido(n, p),
        None => Visto::Desconhecido,
    }
}

/// Liga a camera ~3 s, olha e decide. Nunca guarda imagem.
pub fn quem(dados: &Path) -> Result<Visto, String> {
    let _g = OCUPADO.lock().map_err(|e| e.to_string())?;
    let cfg = config(dados);
    let m = carregar(dados).as_ref().map_err(|e| e.clone())?;
    let gente = pessoas(dados);
    let quadros = capturar(&cfg.camera)?;
    let mut vs = Vec::new();
    for q in &quadros {
        let r = detectar(m, q, cfg.deteccao)?;
        vs.push(match maior(r) {
            Some(r) => Some(identificar(&vetor(m, q, &r)?, &gente, cfg.limiar, cfg.margem)),
            None => None,
        });
    }
    Ok(consenso(&vs))
}

/// Como chamar cada pessoa (o resto usa o proprio nome).
pub fn tratamento(nome: &str) -> String {
    match nome {
        "usuarios" => "Senhor".into(),
        "parceira" | "parceira" => "Parceira".into(),
        "mae" => "Mãe".into(),
        "pai" => "Pai".into(),
        outro => {
            let mut c = outro.chars();
            c.next().map(|p| p.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        }
    }
}

pub fn fala(v: &Visto, padrao: &str) -> String {
    match v {
        Visto::Conhecido(n, _) if n == "usuarios" => format!("Bom te ver, {}. Estou acordado. Em que posso te ajudar?", tratamento(n)),
        Visto::Conhecido(n, _) => format!("Olá, {}! Aqui é o Jarvis. Em que posso te ajudar?", tratamento(n)),
        Visto::Desconhecido => "Não reconheci quem está aí. Quem é, por favor?".into(),
        Visto::Ninguem => padrao.into(),
    }
}

/// Gancho do `escuta::acordar`: com a funcao desligada (padrao) ou qualquer falha, devolve None e vale a saudacao de sempre.
pub fn saudacao(dados: &Path, padrao: &str) -> Option<String> {
    if !config(dados).ligado {
        return None;
    }
    match quem(dados) {
        Ok(v) => Some(fala(&v, padrao)),
        Err(e) => {
            let _ = std::fs::OpenOptions::new().create(true).append(true).open(pasta(dados).join("rosto.log")).and_then(|mut f| std::io::Write::write_all(&mut f, format!("falhou: {e}\n").as_bytes()));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn c(n: &str, p: f32) -> Option<Veredito> {
        Some(Veredito::Conhecido { nome: n.into(), pontuacao: p })
    }
    #[test]
    fn consenso_exige_o_mesmo_nome_em_dois_quadros() {
        assert_eq!(consenso(&[c("usuarios", 0.7), c("usuarios", 0.6), None]), Visto::Conhecido("usuarios".into(), 0.7));
        assert_eq!(consenso(&[c("usuarios", 0.7), c("parceira", 0.6), None]), Visto::Desconhecido);
        assert_eq!(consenso(&[c("usuarios", 0.7), Some(Veredito::Desconhecido), Some(Veredito::Desconhecido)]), Visto::Desconhecido);
        assert_eq!(consenso(&[None, None]), Visto::Ninguem);
    }
    #[test]
    fn fala_trata_cada_um_pelo_nome_certo() {
        assert!(fala(&Visto::Conhecido("usuarios".into(), 0.8), "x").contains("Senhor"));
        assert!(fala(&Visto::Conhecido("parceira".into(), 0.8), "x").contains("Parceira"));
        assert_eq!(fala(&Visto::Ninguem, "padrao"), "padrao");
        assert!(fala(&Visto::Desconhecido, "x").contains("Quem é"));
    }
    #[test]
    fn desligado_por_padrao_nao_abre_camera() {
        let d = std::env::temp_dir().join("rosto_teste_desligado");
        let _ = std::fs::create_dir_all(&d);
        assert_eq!(saudacao(&d, "oi"), None);
    }
    #[test]
    fn detector_roda_nos_modelos_reais_e_imagem_vazia_nao_tem_rosto() {
        let dados = Path::new("E:/braniac-rs/dados");
        if !dados.join("modelos_rosto/sface.onnx").exists() {
            return;
        }
        let m = carregar(dados).as_ref().expect("modelos");
        let preto = vec![0u8; LARG * ALT * 3];
        assert!(detectar(m, &preto, 0.6).unwrap().is_empty());
    }
}
