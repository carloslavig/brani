//! Estudio 3D (braniac/estudio3d.py): pedido -> roteiro de IA gratis -> cenas JSON validadas -> video curto vertical. Aqui as regras:
//! duracao de 15 a 120 s (padrao 35; "1 minuto"/"45 segundos" no pedido valem), 3 videos por dia, um por vez, tema com segredo recusado,
//! tema de politica/saude/financas so com fonte (vai para a skill com o Claude), cenas so com as pecas que o estudio tem e dados limpos,
//! foto so de acervo oficial com credito (sem imagem a cena vira globo; a mesma imagem nunca repete), e o render: motor proprio primeiro,
//! o outro se ele recusar; nunca dois renders juntos.
//! De fora (closures): a IA do roteiro, a busca e o download das fotos, narrar/transcrever, os dois motores de render, o aviso ao dono.
//! NAO portado: a peca "maquete", o fecho "fim_selo" e os enfeites extras do padrao de prefeitura (cena com eles e recusada, nao adivinhada).
#![allow(dead_code)]
use crate::tempo::Agora;
use crate::texto_pt::sem_acento;
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::path::Path;

pub const PECAS: [&str; 8] = ["barras", "globo", "pilha", "mapa", "numero", "linha", "moedas", "foto"];
pub const MAX_DIA: i64 = 3;
pub const MAX_FOTO: usize = 12 * 1024 * 1024;
const SEGREDO: &str = r"(?i)senha|token|cpf|cart[aã]o|pix|chave de acesso|c[oó]digo de verifica";
const SENSIVEL: &str = r"(?i)elei[cç][aã]o|pol[ií]tic|partido|vacina|doen[cç]a|rem[eé]dio|sa[uú]de|invest|a[cç][aã]o da bolsa|criptomoeda|bitcoin|imposto";
// tema de espaco: "imagens oficiais" = acervo da NASA primeiro (a IA quase nunca marca o acervo sozinha)
const ESPACO: &str = r"constela|estrela|planeta|galax|nebulos|astro|universo|espaco|cosmo|\blua\b|\bsol\b|sistema solar|cometa|asteroide|\bnasa\b|telescop|buraco negro|foguete|orion|via lactea";
const COR: &str = r"^(?:dourado|verde|rosa|azul|prata|vermelho|#[0-9a-fA-F]{6})$";
const COR_BARRA: &str = r"^(?:(?:dourado|verde|rosa)(?: claro)?|azul|prata|vermelho|#[0-9a-fA-F]{6})$";

const PROMPT: &str = "Voce escreve roteiros de video curto narrado (vertical) com cenas. Siga A RISCA o que o pedido determinar (assunto, tom, o que mostrar, ordem, tipo de imagem). \
Responda SO um JSON, sem texto fora dele: {\"titulo\": \"...\", \"narracao\": \"frases curtas, PAL palavras, gancho na 1a frase\", \
\"cenas\": [{\"titulo\": \"ate 4 palavras\", \"peca\": \"foto|barras|globo|pilha\", \"dados\": {}}]}. NCENAS cenas na ordem da narracao. \
Pecas: foto = imagem REAL de acervo oficial (dados.busca = 2 a 4 palavras EM INGLES dizendo o que a imagem mostra; \
dados.acervo = \"nasa\" para espaco, astronomia e Terra vista do espaco, senao \"commons\"), barras = comparar/crescer (dados.valores com 3 a 6 numeros so ilustrativos), \
globo = mundo/rede/dados, pilha = acumular (dados.itens de 1 a 12). Use foto quando o pedido falar em imagens ou fotos reais/oficiais, ou o assunto for lugar, objeto, astro ou animal; \
cada foto com uma busca diferente. NAO invente estatistica, data, nome ou fonte; so fatos gerais e bem conhecidos. Portugues do Brasil.";
const REVISAO: &str = "Voce e revisor de fatos de um video educativo. Releia a narracao e corrija SO o que estiver errado ou duvidoso (troque pelo fato certo e bem conhecido, ou corte a afirmacao). \
Mantenha o tom, a ordem das ideias e o tamanho. Responda SO com a narracao corrigida, sem comentario.";

fn re(p: &str) -> Regex {
    Regex::new(p).expect("regex fixa")
}

fn curto(v: &Value, n: usize) -> String {
    match v {
        Value::String(s) => s.chars().take(n).collect(),
        Value::Null => String::new(),
        outro => outro.to_string().chars().take(n).collect(),
    }
}

fn um_espaco(t: &str) -> String {
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Segundos do video: o que veio da ferramenta; senao o que o pedido diz ("1 minuto", "45 segundos"); senao 35. Sempre de 15 a 120.
pub fn duracao(tema: &str, pedida: Option<f64>) -> i64 {
    let s = pedida.filter(|p| p.is_finite()).unwrap_or_else(|| {
        re(r"(\d{1,3})\s*(min|seg|s\b)").captures(&sem_acento(tema)).map_or(35.0, |m| m[1].parse::<f64>().unwrap_or(35.0) * if &m[2] == "min" { 60.0 } else { 1.0 })
    });
    s.clamp(15.0, 120.0) as i64
}

/// Quantas cenas pedir para a duracao (uma a cada ~6 s, de 4 a 11).
pub fn n_cenas(dur: i64) -> i64 {
    ((dur as f64 / 6.0).round() as i64).clamp(4, 11)
}

pub fn slug(titulo: &str) -> String {
    let s: String = re(r"[^a-z0-9]+").replace_all(&sem_acento(titulo), "-").trim_matches('-').chars().take(40).collect();
    if s.is_empty() { "video".into() } else { s }
}

/// Numero do proximo projeto, dado o nome das pastas que ja existem ("012-titulo"). Aceita 3 ou 4 digitos (depois do 999 nao volta a cair em cima).
pub fn proximo_numero(pastas: &[String]) -> String {
    let n = pastas.iter().filter_map(|p| re(r"^(\d{3,4})-").captures(p).and_then(|m| m[1].parse::<u32>().ok())).max().map_or(1, |m| m + 1);
    format!("{n:03}")
}

/// Rascunho da IA -> (titulo, narracao, cenas validas). Narracao curta demais ou com palavra de segredo e recusada.
pub fn validar(dados: &Value, maximo: usize) -> Result<(String, String, Vec<Value>), String> {
    let narr = um_espaco(dados["narracao"].as_str().unwrap_or(""));
    if narr.split(' ').count() < 25 {
        return Err("narracao curta demais".into());
    }
    if re(SEGREDO).is_match(&narr) {
        return Err("narracao com palavra de segredo".into());
    }
    let titulo = dados["titulo"].as_str().filter(|t| !t.is_empty()).unwrap_or("Video explicativo").chars().take(80).collect();
    Ok((titulo, narr, validar_cenas(&dados["cenas"], maximo)?))
}

fn numeros(v: &Value, n: usize) -> Vec<f64> {
    v.as_array().into_iter().flatten().filter_map(Value::as_f64).take(n).collect()
}

fn textos(d: &Value, chaves: &[&str], n: usize, para: &mut Map<String, Value>) {
    for k in chaves {
        if !d[*k].is_null() && d[*k] != "" && d[*k] != false && d[*k] != 0 {
            para.insert(k.to_string(), json!(curto(&d[*k], n)));
        }
    }
}

fn casas(d: &Value, para: &mut Map<String, Value>) {
    if let Some(c) = d["casas"].as_i64().filter(|c| (0..=3).contains(c)) {
        para.insert("casas".into(), json!(c));
    }
}

fn dados_da_peca(peca: &str, c: &Value) -> Result<Value, String> {
    let d = &c["dados"];
    let mut m = Map::new();
    match peca {
        "barras" => {
            let v = numeros(&d["valores"], 6);
            m.insert("valores".into(), if v.len() >= 2 { json!(v) } else { json!([1, 2, 3, 4]) });
            if let Some(r) = d["rotulos"].as_array() {
                m.insert("rotulos".into(), json!(r.iter().take(6).map(|x| curto(x, 60)).collect::<Vec<_>>()));
            }
            let cores: Vec<&str> = d["cores"].as_array().into_iter().flatten().filter_map(Value::as_str).filter(|x| re(COR_BARRA).is_match(x)).take(6).collect();
            if !cores.is_empty() {
                m.insert("cores".into(), json!(cores));
            }
            textos(d, &["titulo"], 60, &mut m);
        }
        "pilha" => {
            m.insert("itens".into(), json!(d["itens"].as_f64().map_or(6, |x| x as i64)));
        }
        "mapa" => {
            let (uf, mun) = (curto(&d["uf"], 10).trim().to_uppercase(), curto(&d["municipio"], 200).trim().chars().take(60).collect::<String>());
            if !re(r"^[A-Z]{2}$").is_match(&uf) || mun.is_empty() {
                return Err("cena mapa precisa de dados.uf (2 letras) e dados.municipio".into());
            }
            let pinos: Vec<Value> = d["pinos"].as_array().into_iter().flatten().take(8).filter_map(|p| {
                let (lat, lon) = (p["lat"].as_f64()?, p["lon"].as_f64()?);
                ((-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon)).then(|| json!({ "nome": curto(&p["nome"], 30), "lat": lat, "lon": lon }))
            }).collect();
            m.insert("uf".into(), json!(uf));
            m.insert("municipio".into(), json!(mun));
            if d["zoom_final"] == "estado" {
                m.insert("zoom_final".into(), json!("estado"));
            }
            if !pinos.is_empty() {
                m.insert("pinos".into(), json!(pinos));
            }
        }
        "numero" => {
            let v = &d["valor"];
            if !(v.is_number() || v.as_str().is_some_and(|s| !s.is_empty() && s.chars().count() <= 30)) {
                return Err("cena numero precisa de dados.valor (numero ou texto curto)".into());
            }
            m.insert("valor".into(), v.clone());
            textos(d, &["prefixo", "sufixo", "rotulo", "detalhe"], 60, &mut m);
            casas(d, &mut m);
        }
        "moedas" => {
            let ps: Vec<Value> = d["pilhas"].as_array().into_iter().flatten().take(5).filter(|p| p.is_object()).map(|p| {
                let mut q = json!({ "ano": curto(&p["ano"], 8), "rotulo": curto(&p["rotulo"], 24) });
                if let Some(v) = p["valor"].as_f64().filter(|v| *v > 0.0) {
                    q["valor"] = json!(v);
                }
                if let Some(i) = p["itens"].as_i64().filter(|i| (1..=40).contains(i)) {
                    q["itens"] = json!(i);
                }
                if let Some(cor) = p["cor"].as_str().filter(|x| re(COR).is_match(x)) {
                    q["cor"] = json!(cor);
                }
                q
            }).collect();
            if ps.is_empty() {
                return Err("cena moedas precisa de dados.pilhas (ate 5, com ano e valor/itens)".into());
            }
            m.insert("pilhas".into(), json!(ps));
        }
        "linha" => {
            let v = numeros(&d["valores"], 48);
            if v.len() < 2 {
                return Err("cena linha precisa de dados.valores com 2 ou mais numeros".into());
            }
            m.insert("valores".into(), json!(v));
            if let Some(r) = d["rotulos"].as_array() {
                m.insert("rotulos".into(), json!(r.iter().take(48).map(|x| curto(x, 12)).collect::<Vec<_>>()));
            }
            textos(d, &["rotulo", "prefixo", "sufixo"], 60, &mut m);
            casas(d, &mut m);
            if let Some(f) = d["final"].as_f64() {
                m.insert("final".into(), json!(f));
            }
        }
        "foto" => {
            let arq = d["arquivo"].as_str().unwrap_or("");
            if re(r"^fotos/[\w-]+/\d+\.(?:jpg|png)$").is_match(arq) {
                // ja baixada: so caminho dentro da pasta de fotos do estudio
                m.insert("arquivo".into(), json!(arq));
                m.insert("credito".into(), json!(curto(&d["credito"], 80)));
            } else {
                // ainda so o pedido de busca; `buscar_fotos` troca pelo arquivo
                let bruto = d["busca"].as_str().filter(|b| !b.is_empty()).or(c["titulo"].as_str()).unwrap_or("");
                let busca: String = re(r"[^\w.,'-]+").replace_all(bruto, " ").trim().chars().take(80).collect();
                if busca.chars().count() < 3 {
                    return Err("cena foto precisa de dados.busca (o que a imagem mostra)".into());
                }
                m.insert("busca".into(), json!(busca));
                m.insert("acervo".into(), json!(if d["acervo"] == "nasa" { "nasa" } else { "commons" }));
            }
        }
        _ => {}
    }
    Ok(Value::Object(m))
}

/// Lista de cenas (de IA ou de um par) -> cenas limpas, so com as pecas que o estudio tem (peca desconhecida vira barras). inicio/fim, se
/// vierem em segundos validos, sao mantidos. Menos de 2 cenas = erro.
pub fn validar_cenas(lista: &Value, maximo: usize) -> Result<Vec<Value>, String> {
    let mut cenas = Vec::new();
    for c in lista.as_array().into_iter().flatten().take(maximo).filter(|c| c.is_object()) {
        let pedida = c["peca"].as_str().unwrap_or("");
        if ["maquete", "fim_selo"].contains(&pedida) {
            return Err(format!("peca {pedida} ainda nao e feita por este motor"));
        }
        let peca = PECAS.into_iter().find(|p| *p == pedida).unwrap_or("barras");
        let mut cena = json!({ "titulo": curto(&c["titulo"], 40), "peca": peca, "dados": dados_da_peca(peca, c)? });
        if let (Some(i), Some(f)) = (c["inicio"].as_f64(), c["fim"].as_f64()) {
            if (0.0..3600.0).contains(&i) && (0.0..3600.0).contains(&f) && f > i {
                cena["inicio"] = json!(i);
                cena["fim"] = json!(f);
            }
        }
        cenas.push(cena);
    }
    if cenas.len() < 2 { Err("poucas cenas".into()) } else { Ok(cenas) }
}

/// Reparte as cenas igualmente na duracao da fala, encostando cada corte no inicio da legenda mais proxima. falas = [(inicio, fim)].
pub fn ajustar_cenas(cenas: &mut [Value], falas: &[(f64, f64)]) {
    let fim = falas.iter().map(|f| f.1).fold(3.0, f64::max);
    let n = cenas.len();
    let mut cortes = vec![0.0];
    for i in 1..n {
        let alvo = fim * i as f64 / n as f64;
        cortes.push(falas.iter().map(|f| f.0).min_by(|a, b| (a - alvo).abs().total_cmp(&(b - alvo).abs())).unwrap_or(alvo));
    }
    cortes.push(fim + 0.5);
    let r2 = |x: f64| (x * 100.0).round() / 100.0;
    for (i, c) in cenas.iter_mut().enumerate() {
        c["inicio"] = json!(r2(cortes[i]));
        c["fim"] = json!(r2(cortes[i + 1].max(cortes[i] + 1.0)));
    }
}

/// So JPG/PNG de verdade, de 20 KB a 12 MB. (Barrar foto quase toda preta precisa decodificar a imagem: fica com quem baixa.)
pub fn foto_valida(dados: &[u8]) -> bool {
    dados.len() > 20_000 && dados.len() <= MAX_FOTO && (dados.starts_with(b"\xff\xd8\xff") || dados.starts_with(b"\x89PN"))
}

/// Cada cena foto com dados.busca ganha um arquivo real em fotos/<nome>/ (a mesma imagem nunca repete). Sem imagem, a cena vira globo.
/// `candidatas(acervo, termo)` -> [(url, credito)]; `baixar(url)` -> bytes; `gravar(arquivo relativo, bytes)`. Busca geral nao cai no acervo
/// de espaco (viria foto de foguete). Devolve as linhas de credito, para o fontes.md.
pub fn buscar_fotos(cenas: &mut [Value], nome: &str, candidatas: &dyn Fn(&str, &str) -> Vec<(String, String)>, baixar: &dyn Fn(&str) -> Option<Vec<u8>>, gravar: &dyn Fn(&str, &[u8]) -> bool) -> Vec<String> {
    let (mut creditos, mut usadas) = (Vec::new(), HashSet::new());
    for (i, c) in cenas.iter_mut().enumerate().map(|(i, c)| (i + 1, c)) {
        if c["peca"] != "foto" || c["dados"]["arquivo"].as_str().is_some_and(|a| !a.is_empty()) {
            continue;
        }
        let termo = c["dados"]["busca"].as_str().unwrap_or("").to_string();
        let acervos: &[&str] = if c["dados"]["acervo"] == "nasa" { &["nasa", "commons"] } else { &["commons"] };
        let achou = acervos.iter().flat_map(|a| candidatas(a, &termo)).filter(|(url, _)| !usadas.contains(url)).find_map(|(url, credito)| {
            let dados = baixar(&url).filter(|d| foto_valida(d))?;
            let arq = format!("fotos/{nome}/{i}.{}", if dados.starts_with(b"\xff\xd8") { "jpg" } else { "png" });
            gravar(&arq, &dados).then_some((url, credito, arq))
        });
        match achou {
            Some((url, credito, arq)) => {
                creditos.push(format!("- cena {i} ({termo}): {credito} | {url}"));
                c["dados"] = json!({ "arquivo": arq, "credito": credito });
                usadas.insert(url);
            }
            None => {
                c["peca"] = json!("globo");
                c["dados"] = json!({});
            }
        }
    }
    creditos
}

/// fontes.md do projeto: sempre diz que e rascunho de IA; com fotos, traz credito e origem de cada uma.
pub fn fontes_md(creditos: &[String]) -> String {
    let fotos = if creditos.is_empty() { String::new() } else { format!("\nImagens (credito e origem):\n{}\n", creditos.join("\n")) };
    format!("Rascunho gerado pelo Jarvis (IA gratis), conceitos gerais; sem fonte primaria. Conferir antes de publicar.\n{fotos}")
}

/// 2a passada de IA so para os fatos. Resposta estranha (JSON, tamanho muito diferente, palavra de segredo) = fica o original.
pub fn revisar(narr: &str, ia: &dyn Fn(&str, &str) -> Result<String, String>) -> String {
    let Ok(r) = ia(REVISAO, narr) else { return narr.to_string() };
    let novo = um_espaco(&r);
    let razao = novo.split(' ').count() as f64 / narr.split_whitespace().count().max(1) as f64;
    if !novo.contains('{') && (0.7..=1.3).contains(&razao) && !re(SEGREDO).is_match(&novo) { novo } else { narr.to_string() }
}

/// Pede o roteiro a IA (`ia(sistema, pedido)`), valida e revisa os fatos. A IA gratis as vezes devolve o JSON quebrado: uma 2a chance.
pub fn roteiro(tema: &str, dur: i64, ia: &dyn Fn(&str, &str) -> Result<String, String>) -> Result<(String, String, Vec<Value>), String> {
    let n = n_cenas(dur);
    let sis = PROMPT.replace("PAL", &format!("{} a {}", (dur as f64 * 2.1) as i64, (dur as f64 * 2.6) as i64)).replace("NCENAS", &format!("{n} a {}", n + 1));
    let mut erro = String::new();
    for _ in 0..2 {
        let tentativa = ia(&sis, &format!("Pedido: {tema}")).and_then(|texto| {
            let (a, b) = (texto.find('{').ok_or("resposta sem JSON")?, texto.rfind('}').ok_or("resposta sem JSON")?);
            let dados: Value = serde_json::from_str(texto.get(a..=b).ok_or("resposta sem JSON")?).map_err(|e| format!("JSON quebrado: {e}"))?;
            validar(&dados, n as usize + 1)
        });
        match tentativa {
            Ok((titulo, narr, mut cenas)) => {
                if re(ESPACO).is_match(&sem_acento(tema)) {
                    for c in cenas.iter_mut().filter(|c| c["peca"] == "foto" && c["dados"]["busca"].is_string()) {
                        c["dados"]["acervo"] = json!("nasa");
                    }
                }
                return Ok((titulo, revisar(&narr, ia), cenas));
            }
            Err(e) => erro = e,
        }
    }
    Err(erro)
}

/// Um video por vez.
#[derive(Default)]
pub struct Estudio {
    pub rodando: bool,
}

/// Porteiro de `criar`: Ok((duracao, fala de "comecando")) e o video passa a contar no teto do dia; Err(fala) explica a recusa.
/// Quem liga roda roteiro/producao em segundo plano e, no fim, chama `terminar`.
pub fn comecar(dados: &Path, agora: &Agora, estudio: &mut Estudio, tema: &str, pedida: Option<f64>, estudio_instalado: bool) -> Result<(i64, String), String> {
    let tema = tema.trim();
    let dur = duracao(tema, pedida);
    if tema.chars().count() < 8 {
        return Err("Qual o tema do vídeo? Me passa uma frase.".into());
    }
    if re(SEGREDO).is_match(tema) {
        return Err("Tema recusado: tem palavra de segredo/dado pessoal.".into());
    }
    if re(SENSIVEL).is_match(tema) {
        return Err("Esse tema (política, saúde ou finanças) precisa de fonte primária. Peça pela skill estudio-3d com o Claude, que pesquisa e cita as fontes.".into());
    }
    if !estudio_instalado {
        return Err("Não achei o Estúdio 3D neste PC.".into());
    }
    let (arq, hoje) = (dados.join("estudio3d_uso.json"), agora.data.iso());
    let uso: Value = std::fs::read_to_string(&arq).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let n = if uso["dia"] == hoje.as_str() { uso["n"].as_i64().unwrap_or(0) } else { 0 };
    if n >= MAX_DIA {
        return Err(format!("Já fiz {MAX_DIA} vídeos hoje (teto do dia)."));
    }
    if estudio.rodando {
        return Err("Já estou fazendo um vídeo; um por vez.".into());
    }
    let _ = std::fs::create_dir_all(dados);
    std::fs::write(&arq, json!({ "dia": hoje, "n": n + 1 }).to_string()).map_err(|e| e.to_string())?;
    estudio.rodando = true;
    let t: String = tema.chars().take(60).collect();
    Ok((dur, format!("Começando o vídeo sobre «{t}» (uns {dur} s, com narração e legenda). Leva alguns minutos; quando ficar pronto eu mando o arquivo no seu WhatsApp.")))
}

/// Fim do video: libera a vez e devolve os blocos para a caixa de saida do dono (o arquivo e um aviso em audio; ou o motivo da falha).
/// O video e RASCUNHO: so entrega o arquivo ao dono, nunca publica. `trato` = como chamar o dono (configuracao).
pub fn terminar(estudio: &mut Estudio, tema: &str, r: &Result<(String, String), String>, trato: &str) -> Vec<String> {
    estudio.rodando = false;
    match r {
        Ok((titulo, caminho)) => vec![format!("[arquivo] {caminho}\nVídeo «{titulo}» (rascunho: confira antes de publicar)."), format!("[audio] {trato}, o vídeo {titulo} ficou pronto. Mandei o arquivo aqui.")],
        Err(e) => vec![format!("Não consegui fazer o vídeo sobre «{}»: {}", tema.chars().take(50).collect::<String>(), e.chars().take(150).collect::<String>())],
    }
}

/// Render: o motor proprio primeiro (None = desligado/nao instalado; Err = recusou o roteiro ou falhou); o outro motor faz como sempre.
/// `ocupado` = ja ha um render rodando no PC (a placa nao aguenta dois): quem liga espera e tenta de novo. Devolve (arquivo, motor).
pub fn renderizar(ocupado: bool, motor_rs: &dyn Fn() -> Option<Result<String, String>>, remotion: &dyn Fn() -> Result<String, String>) -> Result<(String, &'static str), String> {
    if ocupado {
        return Err("um render por vez: ja ha outro rodando".into());
    }
    if let Some(Ok(saida)) = motor_rs() {
        return Ok((saida, "motor-rs"));
    }
    remotion().map(|s| (s, "remotion")).map_err(|e| format!("render falhou: {}", e.chars().rev().take(200).collect::<Vec<_>>().into_iter().rev().collect::<String>()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;
    use std::cell::RefCell;

    const NARR: &str = "Olhe para o ceu numa noite limpa e voce vai achar tres estrelas em linha. Elas formam o cinturao de uma figura famosa, vista por povos do mundo inteiro ha milhares de anos.";

    #[test]
    fn duracao_slug_e_numero() {
        assert_eq!((duracao("faz um vídeo de 1 minuto sobre o mar", None), duracao("vídeo de 45 segundos", None), duracao("vídeo curto", None)), (60, 45, 35));
        assert_eq!((duracao("vídeo de 5 minutos", None), duracao("vídeo de 3 s", None), duracao("vídeo de 1 minuto", Some(20.0)), duracao("x", Some(900.0)), duracao("x", Some(f64::NAN))), (120, 15, 20, 120, 35));
        assert_eq!((n_cenas(15), n_cenas(35), n_cenas(120)), (4, 6, 11));
        assert_eq!((slug("Órion: a Constelação!"), slug("!!!"), slug(&"a".repeat(60)).len()), ("orion-a-constelacao".to_string(), "video".to_string(), 40));
        let p = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!((proximo_numero(&[]), proximo_numero(&p(&["001-a", "012-b", "notas"])), proximo_numero(&p(&["999-a"])), proximo_numero(&p(&["999-a", "1000-b"]))),
            ("001".to_string(), "013".to_string(), "1000".to_string(), "1001".to_string()));
    }

    #[test]
    fn cenas_so_com_pecas_do_estudio_e_dados_limpos() {
        let lista = json!([
            { "titulo": "Um titulo comprido demais para caber na tela do video inteiro", "peca": "barras", "dados": { "valores": [1, "x", 3, 4, 5, 6, 7, 8], "cores": ["verde claro", "roxo", "#00ff00"], "codigo": "rm -rf" } },
            { "titulo": "Inventada", "peca": "holograma", "dados": { "valores": [9] }, "inicio": 2, "fim": 5 },
            "lixo",
            { "titulo": "Pilha", "peca": "pilha", "dados": {}, "inicio": 9, "fim": 5 },
            { "titulo": "Orion nebula", "peca": "foto", "dados": { "acervo": "nasa" } },
            { "titulo": "F2", "peca": "foto", "dados": { "busca": "sea <script> turtle", "acervo": "outro" } },
            { "titulo": "F3", "peca": "foto", "dados": { "arquivo": "fotos/001-x/3.jpg", "credito": "NASA" } },
            { "titulo": "Total", "peca": "numero", "dados": { "valor": 42, "sufixo": "%", "casas": 9 } },
            { "titulo": "Linha", "peca": "linha", "dados": { "valores": [1, 2, 3], "casas": 2, "final": 3.5 } },
            { "titulo": "Mapa", "peca": "mapa", "dados": { "uf": "mt", "municipio": "Vila Nova", "pinos": [{ "nome": "a", "lat": -15, "lon": -52 }, { "lat": 999, "lon": 0 }] } },
            { "titulo": "Moedas", "peca": "moedas", "dados": { "pilhas": [{ "ano": 2026, "valor": 3, "itens": 99, "cor": "roxo" }] } },
        ]);
        let c = validar_cenas(&lista, 20).unwrap();
        assert_eq!(c.len(), 10);
        assert!(c[0]["titulo"].as_str().unwrap().chars().count() == 40 && c[0]["dados"] == json!({ "valores": [1.0, 3.0, 4.0, 5.0, 6.0, 7.0], "cores": ["verde claro", "#00ff00"] }));
        assert!(c[1]["peca"] == "barras" && c[1]["dados"]["valores"] == json!([1, 2, 3, 4]) && c[1]["inicio"] == 2.0 && c[1]["fim"] == 5.0); // peca que nao existe vira barras
        assert!(c[2]["dados"] == json!({ "itens": 6 }) && c[2].get("inicio").is_none()); // fim antes do inicio: descartado
        assert_eq!(c[3]["dados"], json!({ "busca": "Orion nebula", "acervo": "nasa" })); // sem busca, vale o titulo
        assert_eq!(c[4]["dados"], json!({ "busca": "sea script turtle", "acervo": "commons" }));
        assert_eq!(c[5]["dados"], json!({ "arquivo": "fotos/001-x/3.jpg", "credito": "NASA" }));
        assert_eq!(c[6]["dados"], json!({ "valor": 42, "sufixo": "%" }));
        assert_eq!(c[7]["dados"], json!({ "valores": [1.0, 2.0, 3.0], "casas": 2, "final": 3.5 }));
        assert_eq!(c[8]["dados"], json!({ "uf": "MT", "municipio": "Vila Nova", "pinos": [{ "nome": "a", "lat": -15.0, "lon": -52.0 }] }));
        assert_eq!(c[9]["dados"], json!({ "pilhas": [{ "ano": "2026", "rotulo": "", "valor": 3.0 }] }));
        for (ruim, trecho) in [
            (json!([{ "peca": "foto", "dados": { "arquivo": "../../segredo.png" } }, { "peca": "globo" }]), "dados.busca"), // caminho fora da pasta de fotos nao passa
            (json!([{ "peca": "mapa", "dados": { "uf": "Mato Grosso", "municipio": "x" } }, { "peca": "globo" }]), "dados.uf"),
            (json!([{ "peca": "numero", "dados": {} }, { "peca": "globo" }]), "dados.valor"),
            (json!([{ "peca": "linha", "dados": { "valores": [1] } }, { "peca": "globo" }]), "2 ou mais"),
            (json!([{ "peca": "moedas", "dados": {} }, { "peca": "globo" }]), "dados.pilhas"),
            (json!([{ "peca": "maquete", "dados": {} }, { "peca": "globo" }]), "ainda nao"),
            (json!([{ "peca": "globo" }]), "poucas cenas"), (Value::Null, "poucas cenas"),
        ] {
            assert!(validar_cenas(&ruim, 6).unwrap_err().contains(trecho), "{trecho}");
        }
        assert_eq!(validar_cenas(&json!([{ "peca": "globo" }, { "peca": "globo" }, { "peca": "globo" }]), 2).unwrap().len(), 2); // teto de cenas
        let cenas2 = json!([{ "peca": "globo" }, { "peca": "pilha" }]);
        assert_eq!(validar(&json!({ "narracao": "curta", "cenas": cenas2 }), 6).unwrap_err(), "narracao curta demais");
        assert_eq!(validar(&json!({ "narracao": format!("{NARR} A senha do cofre e outra."), "cenas": cenas2 }), 6).unwrap_err(), "narracao com palavra de segredo");
        let (t, n, c) = validar(&json!({ "narracao": format!("  {NARR}\n\n"), "cenas": cenas2 }), 6).unwrap();
        assert!(t == "Video explicativo" && n == NARR && c.len() == 2);
    }

    #[test]
    fn cortes_das_cenas_e_fotos_com_credito_sem_repetir() {
        let mut cenas = validar_cenas(&json!([{ "peca": "globo" }, { "peca": "globo" }, { "peca": "globo" }]), 6).unwrap();
        ajustar_cenas(&mut cenas, &[(0.0, 2.8), (3.1, 5.9), (6.2, 9.0)]);
        let t: Vec<(f64, f64)> = cenas.iter().map(|c| (c["inicio"].as_f64().unwrap(), c["fim"].as_f64().unwrap())).collect();
        assert_eq!(t, [(0.0, 3.1), (3.1, 6.2), (6.2, 9.5)]); // cada corte encosta no inicio da legenda mais proxima
        ajustar_cenas(&mut cenas, &[]);
        assert_eq!(cenas[2]["fim"], 3.5);
        let mut cenas = validar_cenas(&json!([
            { "titulo": "A", "peca": "foto", "dados": { "busca": "orion nebula", "acervo": "nasa" } },
            { "titulo": "B", "peca": "foto", "dados": { "busca": "orion belt", "acervo": "nasa" } },
            { "titulo": "C", "peca": "foto", "dados": { "busca": "sea turtle" } },
            { "titulo": "D", "peca": "foto", "dados": { "busca": "nothing here" } },
            { "titulo": "E", "peca": "pilha", "dados": {} },
        ]), 6).unwrap();
        let (buscas, gravados) = (RefCell::new(Vec::new()), RefCell::new(Vec::new()));
        let jpg = [b"\xff\xd8\xff".as_slice(), &[1u8; 30_000]].concat();
        let candidatas = |acervo: &str, termo: &str| {
            buscas.borrow_mut().push(format!("{acervo}:{termo}"));
            match (acervo, termo) {
                ("nasa", _) => vec![("https://a/1.jpg".to_string(), "NASA/JPL".to_string())],
                ("commons", "orion belt") => vec![("https://a/pequena.jpg".to_string(), "x".to_string()), ("https://c/2.png".to_string(), "Ana · CC BY 4.0 · Wikimedia Commons".to_string())],
                ("commons", "sea turtle") => vec![("https://c/3.jpg".to_string(), "Bia · Wikimedia Commons".to_string())],
                _ => vec![],
            }
        };
        let baixar = |url: &str| match url {
            "https://a/pequena.jpg" => Some(vec![0xff, 0xd8, 0xff]), // pequena demais
            "https://c/2.png" => Some([b"\x89PNG".as_slice(), &[1u8; 30_000]].concat()),
            _ => Some(jpg.clone()),
        };
        let gravar = |arq: &str, _: &[u8]| {
            gravados.borrow_mut().push(arq.to_string());
            true
        };
        let creditos = buscar_fotos(&mut cenas, "007-orion", &candidatas, &baixar, &gravar);
        assert_eq!(*gravados.borrow(), ["fotos/007-orion/1.jpg", "fotos/007-orion/2.png", "fotos/007-orion/3.jpg"]);
        assert_eq!(cenas[0]["dados"], json!({ "arquivo": "fotos/007-orion/1.jpg", "credito": "NASA/JPL" }));
        assert_eq!(creditos[1], "- cena 2 (orion belt): Ana · CC BY 4.0 · Wikimedia Commons | https://c/2.png"); // a 1a da NASA ja foi usada: nao repete
        assert!(cenas[3]["peca"] == "globo" && cenas[3]["dados"] == json!({}) && cenas[4]["peca"] == "pilha"); // sem imagem, vira globo
        assert!(!buscas.borrow().contains(&"nasa:sea turtle".to_string())); // busca geral nao cai no acervo de espaco
        assert!(fontes_md(&creditos).contains("Conferir antes de publicar") && fontes_md(&creditos).contains("Imagens (credito e origem):\n- cena 1") && !fontes_md(&[]).contains("Imagens"));
        assert!(validar_cenas(&Value::Array(cenas), 6).is_ok()); // as cenas com arquivo continuam validas
        assert!(!foto_valida(b"\xff\xd8\xff") && !foto_valida(&[b'<'; 30_000]) && foto_valida(&jpg));
    }

    #[test]
    fn roteiro_tem_segunda_chance_marca_o_acervo_e_revisa_os_fatos() {
        let chamadas = RefCell::new(Vec::new());
        let json_bom = json!({ "titulo": "Orion", "narracao": NARR, "cenas": [{ "titulo": "Nebulosa", "peca": "foto", "dados": { "busca": "orion nebula" } }, { "titulo": "Mundo", "peca": "globo" }] }).to_string();
        let ia = |sis: &str, pedido: &str| {
            chamadas.borrow_mut().push(sis.to_string());
            let n = chamadas.borrow().len();
            if sis.starts_with("Voce e revisor") { Ok(format!("{pedido} ")) } else if n == 1 { Ok("claro! aqui vai: {quebrado".to_string()) } else { Ok(format!("Segue:\n```json\n{json_bom}\n```")) }
        };
        let (titulo, narr, cenas) = roteiro("a constelação de Órion", 35, &ia).unwrap();
        assert!(titulo == "Orion" && narr == NARR && cenas[0]["dados"]["acervo"] == "nasa" && chamadas.borrow().len() == 3);
        assert!(chamadas.borrow()[0].contains("73 a 91 palavras") && chamadas.borrow()[0].contains("6 a 7 cenas") && chamadas.borrow()[0].contains("NAO invente estatistica"));
        assert!(roteiro("tartarugas marinhas", 35, &|_, _| Ok("sem json".into())).is_err() && roteiro("tartarugas", 35, &|_, _| Err("fora do ar".into())).unwrap_err() == "fora do ar");
        let (_, _, cenas) = roteiro("tartarugas marinhas", 35, &|s, p| if s.starts_with("Voce e revisor") { Ok(p.into()) } else { Ok(json_bom.clone()) }).unwrap();
        assert_eq!(cenas[0]["dados"]["acervo"], "commons");
        // revisao estranha nao entra
        assert_eq!(revisar(NARR, &|_, _| Ok("{\"narracao\": 1}".into())), NARR);
        assert_eq!(revisar(NARR, &|_, _| Ok("Curta.".into())), NARR);
        assert_eq!(revisar(NARR, &|_, _| Ok(format!("{NARR} senha"))), NARR);
        assert_eq!(revisar(NARR, &|_, _| Err("x".into())), NARR);
        assert_eq!(revisar(NARR, &|_, n| Ok(n.replace("tres", "três"))), NARR.replace("tres", "três"));
    }

    #[test]
    fn porteiro_recusa_segredo_tema_sensivel_teto_e_dois_ao_mesmo_tempo() {
        let d = std::env::temp_dir().join(format!("braniac-estudio3d-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let a = Agora { data: Data::civil(2026, 11, 17).unwrap(), hora: 10, min: 0, seg: 0 };
        let mut e = Estudio::default();
        assert!(comecar(&d, &a, &mut e, "oi", None, true).unwrap_err().starts_with("Qual o tema"));
        assert!(comecar(&d, &a, &mut e, "como descobrir a senha do wifi", None, true).unwrap_err().contains("segredo"));
        for t in ["quem ganha a eleição deste ano", "a vacina funciona mesmo?", "vale investir em bitcoin agora"] {
            assert!(comecar(&d, &a, &mut e, t, None, true).unwrap_err().contains("fonte primária"), "{t}");
        }
        assert!(comecar(&d, &a, &mut e, "a constelação de Órion", None, false).unwrap_err().contains("Não achei"));
        assert!(!e.rodando && !d.join("estudio3d_uso.json").exists()); // recusa nao gasta o teto
        let (dur, fala) = comecar(&d, &a, &mut e, "a constelação de Órion em 1 minuto", None, true).unwrap();
        assert!(dur == 60 && fala.contains("uns 60 s") && e.rodando);
        assert_eq!(comecar(&d, &a, &mut e, "as tartarugas marinhas", None, true).unwrap_err(), "Já estou fazendo um vídeo; um por vez.");
        let avisos = terminar(&mut e, "Órion", &Ok(("Órion".into(), "C:/v/video.mp4".into())), "Senhor Davi");
        assert!(avisos[0].starts_with("[arquivo] C:/v/video.mp4\n") && avisos[0].contains("rascunho: confira antes de publicar") && avisos[1].starts_with("[audio] Senhor Davi, o vídeo Órion") && !e.rodando);
        for _ in 0..2 {
            comecar(&d, &a, &mut e, "as tartarugas marinhas", None, true).unwrap();
            assert!(terminar(&mut e, "as tartarugas marinhas", &Err("render falhou".into()), "Senhor Davi")[0].contains("Não consegui fazer o vídeo sobre «as tartarugas marinhas»: render falhou"));
        }
        assert_eq!(comecar(&d, &a, &mut e, "as tartarugas marinhas", None, true).unwrap_err(), "Já fiz 3 vídeos hoje (teto do dia)."); // falha tambem conta
        let amanha = Agora { data: Data(a.data.0 + 1), ..a };
        assert!(comecar(&d, &amanha, &mut e, "as tartarugas marinhas", None, true).is_ok());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn render_um_por_vez_e_o_segundo_motor_so_se_o_primeiro_nao_fizer() {
        assert!(renderizar(true, &|| panic!("ocupado nao renderiza"), &|| panic!("ocupado nao renderiza")).unwrap_err().contains("um render por vez"));
        assert_eq!(renderizar(false, &|| Some(Ok("v.mp4".into())), &|| panic!("o motor proprio ja fez")).unwrap(), ("v.mp4".to_string(), "motor-rs"));
        assert_eq!(renderizar(false, &|| Some(Err("peca sem suporte".into())), &|| Ok("v.mp4".into())).unwrap().1, "remotion");
        assert_eq!(renderizar(false, &|| None, &|| Ok("v.mp4".into())).unwrap().1, "remotion");
        assert_eq!(renderizar(false, &|| None, &|| Err("faltou memoria".into())).unwrap_err(), "render falhou: faltou memoria");
    }
}
