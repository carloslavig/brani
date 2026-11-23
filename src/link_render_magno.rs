//! Pedido no formato do PACOTE de videos de prefeitura (link/render_magno.py): chega UM zip por cidade; o Braniac confere, extrai por
//! cima do pacote instalado e roda o orquestrador do pacote. O zip traz CODIGO de cena, entao as travas sao o que importa:
//! so entram arquivos de cena e dados (lista EXT) dentro das pastas RAIZES; script, programa e config NAO sao extraidos; caminho que
//! tenta sair da pasta derruba o pedido inteiro; arquivo de texto com cara de chave de API derruba; os campos da ficha viram argumento
//! de comando, por isso tem formato rigido. `sem_codigo` (BRANIAC_LINK_SEM_CODIGO=1) desliga este caminho ("so dados").
//! Aqui nada e executado: o render sai como LISTA DE ARGUMENTOS e quem chama roda (um por vez, a fila garante).
//! Ficou de fora: abrir o zip e gravar os arquivos (closure `ler`), consertar nome de zip do Mac (cp437/acento decomposto: quem le o
//! zip entrega os nomes ja em UTF-8 composto), ffmpeg/ffprobe, baixar o satelite do mapa e copiar a capa.
#![allow(dead_code)]
use regex::Regex;
use serde_json::{json, Value};
use std::sync::OnceLock;

pub const NUCLEOS: u32 = 6; // notebook: 6 abas; erro de placa/memoria -> 4; nunca passa de 8
pub const OPCOES: [&str; 2] = ["--gl=angle", "--quadros-por-parte=600"];
pub const RAIZES: [&str; 5] = ["estudio-3d/src/", "estudio-3d/projetos/", "kit-mapas/src/", "kit-mapas/public/satelite/", "videos/"];
pub const EXT: [&str; 16] = [".ts", ".tsx", ".json", ".jsonl", ".md", ".txt", ".css", ".svg", ".woff2", ".wav", ".mp3", ".mp4", ".jpg", ".jpeg", ".png", ".webp"];
pub const EXT_TEXTO: [&str; 6] = [".ts", ".tsx", ".json", ".jsonl", ".md", ".txt"];
pub const MAX_BYTES: u64 = 3 * 1024 * 1024 * 1024;
pub const MAX_ARQS: usize = 20000;
const NOME: &str = r"^[0-9A-Za-zÀ-ÿ][0-9A-Za-zÀ-ÿ .\-]{0,59}$";
const COMPOSICAO: &str = r"^[A-Za-z][A-Za-z0-9]{0,79}$";
/// Campos de texto da ficha e o formato de cada um (viram argumento de comando).
pub const FORMATO: [(&str, &str); 5] = [("numero", r"^\d{1,4}$"), ("nome", NOME), ("projeto", r"^[0-9A-Za-zÀ-ÿ][0-9A-Za-zÀ-ÿ .\-]{0,99}$"), ("composicao_estudio", COMPOSICAO), ("arquivo_mapa", r"^[a-z0-9][a-z0-9_\-]{0,59}\.mp4$")];

fn re(p: &str) -> Regex {
    Regex::new(p).expect("regex fixa")
}

fn ext(nome: &str) -> String {
    let base = nome.rsplit('/').next().unwrap_or("");
    base.rfind('.').filter(|i| *i > 0).map(|i| base[i..].to_lowercase()).unwrap_or_default()
}

pub struct Entrada {
    pub nome: String,
    pub tam: u64,
    pub dir: bool,
}

/// Indices das entradas do zip que podem ser extraidas (pasta e tipo permitidos) e quantas foram ignoradas.
/// Caminho que tenta sair da pasta derruba o pedido inteiro.
pub fn entradas(lista: &[Entrada]) -> Result<(Vec<usize>, usize), String> {
    let (mut boas, mut ignoradas, mut total) = (Vec::new(), 0, 0u64);
    for (i, e) in lista.iter().enumerate().filter(|(_, e)| !e.dir) {
        let nome = e.nome.replace('\\', "/");
        if nome.starts_with('/') || nome.split('/').any(|p| p == "..") || nome.contains(':') {
            return Err(format!("zip recusado: caminho invalido ({})", nome.chars().take(60).collect::<String>()));
        }
        if !RAIZES.iter().any(|r| nome.starts_with(r)) || !EXT.contains(&ext(&nome).as_str()) {
            ignoradas += 1;
            continue;
        }
        total = total.saturating_add(e.tam);
        boas.push(i);
    }
    if total > MAX_BYTES || boas.len() > MAX_ARQS {
        return Err("zip recusado: grande demais".into());
    }
    Ok((boas, ignoradas))
}

/// Algo parecido com chave de API (Google, Groq, NVIDIA, OpenAI).
pub fn tem_chave(dados: &[u8]) -> bool {
    static R: OnceLock<regex::bytes::Regex> = OnceLock::new();
    R.get_or_init(|| regex::bytes::Regex::new(r"AIza[0-9A-Za-z_-]{20}|gsk_[0-9A-Za-z]{20}|nvapi-[0-9A-Za-z_-]{20}|sk-[A-Za-z0-9]{20}").expect("regex fixa")).is_match(dados)
}

/// Antes de gravar cada arquivo: texto com cara de chave de API derruba o pedido.
pub fn pode_gravar(nome: &str, dados: &[u8]) -> Result<(), String> {
    if EXT_TEXTO.contains(&ext(nome).as_str()) && tem_chave(dados) {
        return Err(format!("zip recusado: achei algo parecido com chave de API em {}", nome.chars().take(60).collect::<String>()));
    }
    Ok(())
}

/// Confere a ficha videos/NNN/video.json contra os nomes extraiveis do zip. `ler(nome)` devolve o conteudo de uma entrada.
pub fn ficha(nomes: &[&str], ler: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<Value, String> {
    let rf = re(r"^videos/\d{1,4}/video\.json$");
    let fichas: Vec<&&str> = nomes.iter().filter(|n| rf.is_match(n)).collect();
    if fichas.len() != 1 {
        return Err(format!("o zip precisa de UMA ficha videos/NNN/video.json (achei {})", fichas.len()));
    }
    let bruto = ler(fichas[0]).ok_or("video.json nao e JSON valido")?;
    let f: Value = serde_json::from_str(String::from_utf8_lossy(&bruto).trim_start_matches('\u{feff}')).map_err(|_| "video.json nao e JSON valido".to_string())?;
    for (campo, formato) in FORMATO {
        if !f[campo].as_str().is_some_and(|v| re(formato).is_match(v)) {
            return Err(format!("video.json: campo '{campo}' ausente ou fora do formato"));
        }
    }
    let s = |k: &str| f[k].as_str().unwrap_or("");
    let n = s("numero");
    if **fichas[0] != format!("videos/{n}/video.json") {
        return Err("video.json: 'numero' diferente da pasta videos/NNN".into());
    }
    if !f["composicao_kit"].is_null() && !f["composicao_kit"].as_str().is_some_and(|c| re(COMPOSICAO).is_match(c)) {
        return Err("video.json: 'composicao_kit' fora do formato (ou use null)".into());
    }
    if !f["folhas"].is_null() && f["folhas"] != format!("videos/{n}/folhas.json").as_str() {
        return Err(format!("video.json: 'folhas' deve ser videos/{n}/folhas.json (ou null)"));
    }
    if !(f["quadro_capa"].is_i64() || f["quadro_capa"].is_u64()) || !f["duracao_esperada_s"].is_number() {
        return Err("video.json: 'quadro_capa' (inteiro) e 'duracao_esperada_s' (numero) sao obrigatorios".into());
    }
    let mut precisa = vec![format!("estudio-3d/projetos/{}/audio/narracao-norm.wav", s("projeto")), "estudio-3d/src/Root.tsx".to_string()];
    precisa.extend(f["folhas"].as_str().map(str::to_string));
    if !f["composicao_kit"].is_null() {
        precisa.push("kit-mapas/src/Root.tsx".into());
    }
    let falta: Vec<String> = precisa.into_iter().filter(|x| !nomes.contains(&x.as_str())).collect();
    if !falta.is_empty() {
        return Err(format!("faltou no zip: {}", falta.join("; ")));
    }
    if f["composicao_kit"].is_null() && !nomes.contains(&format!("estudio-3d/projetos/{}/mapas/{}", s("projeto"), s("arquivo_mapa")).as_str()) {
        return Err("sem composicao_kit o zip precisa trazer o mapa pronto em projetos/<projeto>/mapas/".into());
    }
    let comp = s("composicao_estudio");
    if !ler("estudio-3d/src/Root.tsx").is_some_and(|r| r.windows(comp.len()).any(|w| w == comp.as_bytes())) {
        return Err(format!("a composicao {comp} nao esta registrada no estudio-3d/src/Root.tsx do zip"));
    }
    Ok(f)
}

/// O pedido em zip inteiro, ANTES de extrair qualquer coisa: liga/desliga, pacote instalado, um zip so, entradas, ficha e chaves.
/// Devolve (ficha, indices das entradas a extrair, quantas ignoradas).
pub fn conferir_zip(sem_codigo: bool, pacote_instalado: bool, zips: usize, zip_integro: bool, lista: &[Entrada], ler: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<(Value, Vec<usize>, usize), String> {
    if sem_codigo {
        return Err("pedido em zip desligado neste Braniac (so dados)".into());
    }
    if !pacote_instalado {
        return Err("o pacote de producao nao esta instalado neste PC".into());
    }
    if zips != 1 {
        return Err("mande UM zip por pedido (video-NNN.zip)".into());
    }
    if !zip_integro {
        return Err("zip corrompido: envie de novo".into());
    }
    let (boas, ignoradas) = entradas(lista)?;
    let nomes: Vec<String> = boas.iter().map(|i| lista[*i].nome.replace('\\', "/")).collect();
    let f = ficha(&nomes.iter().map(String::as_str).collect::<Vec<_>>(), ler)?;
    for n in &nomes {
        if EXT_TEXTO.contains(&ext(n).as_str()) {
            pode_gravar(n, &ler(n).unwrap_or_default())?;
        }
    }
    Ok((f, boas, ignoradas))
}

/// Quantas abas tentar, em ordem: o configurado (teto 8) e, se falhar, 4.
pub fn tentativas(nucleos: u32) -> Vec<u32> {
    let (a, b) = (nucleos.clamp(1, 8), nucleos.clamp(1, 4));
    if a == b { vec![a] } else { vec![a, b] }
}

/// O comando do render como lista de argumentos (nada e executado aqui). `numero` ja passou pelo formato da ficha.
pub fn args_render(numero: &str, nucleos: u32, de: Option<&str>) -> Result<Vec<String>, String> {
    if !re(FORMATO[0].1).is_match(numero) || de.is_some_and(|d| !re(r"^[a-z]{1,20}$").is_match(d)) {
        return Err("numero ou etapa fora do formato".into());
    }
    let mut a = vec!["node".to_string(), "renderizar.mjs".into(), format!("videos/{numero}/video.json"), format!("--nucleos={}", nucleos.clamp(1, 8))];
    a.extend(OPCOES.iter().map(|s| s.to_string()));
    a.extend(de.map(|d| format!("--de={d}")));
    Ok(a)
}

/// Roda o orquestrador (`rodar(args)` devolve o codigo de saida). Falhou com 6 abas: tenta de novo com 4 (as partes prontas sao
/// aproveitadas). Codigo 2 = rodou ate o fim, mas a conferencia reprovou: outra tentativa nao muda isso.
pub fn renderizar(numero: &str, nucleos: u32, de: Option<&str>, rodar: &dyn Fn(&[String]) -> Result<i32, String>, fim_do_log: &dyn Fn() -> String) -> Result<(), String> {
    let mut codigo = 1;
    for nuc in tentativas(nucleos) {
        codigo = rodar(&args_render(numero, nuc, de)?)?;
        if codigo == 0 || codigo == 2 {
            break;
        }
    }
    if codigo == 0 {
        return Ok(());
    }
    let log = fim_do_log();
    let linhas: Vec<&str> = log.lines().collect();
    let fim = linhas[linhas.len().saturating_sub(6)..].iter().map(|x| x.trim()).filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" | ");
    let rabo: String = fim.chars().rev().take(240).collect::<Vec<_>>().into_iter().rev().collect();
    Err(format!("{}{rabo}", if codigo == 2 { "conferencia final reprovou: " } else { "render falhou: " }))
}

/// Pasta do video no kit de mapas a partir do id da composicao (Root.tsx do kit).
pub fn pasta_kit(root_tsx: &str, comp_kit: &str) -> Result<String, String> {
    let nao = || format!("composicao {comp_kit} nao esta no kit-mapas/src/Root.tsx");
    let m = re(&format!(r#"id="{}"\s+component=\{{(\w+)\."#, regex::escape(comp_kit))).captures(root_tsx).ok_or_else(nao)?;
    re(&format!(r#"import \* as {} from "\./videos/([\w\-]+)""#, regex::escape(&m[1]))).captures(root_tsx).map(|i| i[1].to_string()).ok_or_else(nao)
}

/// Pasta dos tiles do satelite declarada no roteiro do kit (None = nao usa).
pub fn pasta_tiles(roteiro_ts: &str) -> Option<String> {
    re(r#"PASTA_TILES\s*=\s*"([^"]+)""#).captures(roteiro_ts).map(|m| m[1].to_string())
}

/// O .tsx de um video ja instalado e o do numero pedido? Devolve {projeto, composicao_estudio, arquivo_mapa}.
pub fn codigo_do_tsx(txt: &str, numero: &str) -> Option<Value> {
    let m = re(&format!(r"(?m)^const P = '({}\. [^']+)';", regex::escape(numero))).captures(txt)?;
    let comp = re(r"(?m)^export const (\w+): React\.FC").captures(txt)?;
    let mapa = re(r"mapas/([a-z0-9_\-]+\.mp4)").captures(txt).map(|x| x[1].to_string());
    Some(json!({ "projeto": &m[1], "composicao_estudio": &comp[1], "arquivo_mapa": mapa }))
}

/// A composicao tambem existe no kit de mapas?
pub fn composicao_kit(base: &Value, root_kit: &str) -> Value {
    match base["composicao_estudio"].as_str() {
        Some(c) if root_kit.contains(&format!("id=\"{c}\"")) => json!(c),
        _ => Value::Null,
    }
}

/// "R$ 1.234,50" -> 1234.5 (numero copiado do pedido, sem mexer).
pub fn num(v: &Value) -> Result<f64, String> {
    match v {
        Value::String(s) => s.replace("R$", "").trim().replace('.', "").replace(',', ".").parse().map_err(|_| format!("numero ilegivel: {s}")),
        outro => outro.as_f64().ok_or_else(|| format!("numero ilegivel: {outro}")),
    }
}

fn linhas(v: &Value) -> Result<Vec<Value>, String> {
    v.as_array().into_iter().flatten().map(|l| Ok(json!([l[0], num(&l[1])?]))).collect()
}

/// Folhas no formato do gerador a partir das cenas de maquete do meta.json do pedido (numeros copiados sem mexer; soma que nao fecha
/// e barrada em `folhas_ruins`).
pub fn folhas_de_meta(meta: &Value) -> Result<Vec<Value>, String> {
    let mes = |ano: &str| match ano {
        "2026" => "set/2026".to_string(),
        a => format!("dez/{a}"),
    };
    let mut por_ano: Vec<(String, Vec<&Value>)> = Vec::new();
    for c in meta["cenas"].as_array().into_iter().flatten().filter(|c| c["peca"] == "maquete") {
        let ano = c["dados"]["ano"].as_str().ok_or("cena de maquete sem o ano")?.to_string();
        match por_ano.iter_mut().find(|(a, _)| *a == ano) {
            Some((_, v)) => v.push(c),
            None => por_ano.push((ano, vec![c])),
        }
    }
    let t = |v: &Value, o: &str| v.as_f64().ok_or_else(|| format!("{o} sem o tempo"));
    let mut folhas = Vec::new();
    for (ano_txt, cenas) in &por_ano {
        let (ano, ano_tit, parcial): (String, String, bool) = (ano_txt.chars().take(4).collect(), ano_txt.replace("ATÉ SET", "até set"), ano_txt.contains("ATÉ"));
        let fim_mes = mes(&ano);
        let setores: Vec<(&Value, usize)> = cenas.iter().enumerate().flat_map(|(i, c)| c["dados"]["setores"].as_array().into_iter().flatten().filter(|s| s["folha"].is_object()).map(move |s| (s, i))).collect();
        for c in cenas {
            let fr = &c["dados"]["folha_receita"];
            if !fr.is_object() {
                continue;
            }
            let mut f = json!({ "tipo": "receita", "titulo": format!("Receita arrecadada · {ano_tit}"), "total": num(&fr["total"])?, "linhas": linhas(&fr["linhas"])?,
                "rodape": if parcial { format!("Receita consolidada {fim_mes} (PCMGE066) · TCM-BA") } else { format!("Receita {fim_mes} consolidada (PCMGE066) · total = Balanço {ano} · TCM-BA") },
                "de": t(&fr["t"], "folha de receita")?, "ate": match setores.first() { Some((s, _)) => t(&s["t"], "setor")?, None => t(&c["fim"], "cena")? } });
            if !fr["destaque"].is_null() {
                f["destaque"] = fr["destaque"].clone();
            }
            folhas.push(f);
        }
        for (i, (s, ic)) in setores.iter().enumerate() {
            let fo = &s["folha"];
            let ate = match setores.get(i + 1) {
                Some((prox, ip)) if ip == ic => t(&prox["t"], "setor")?,
                _ => t(&cenas[*ic]["fim"], "cena")?,
            };
            let mut f = json!({ "tipo": "despesa", "titulo": format!("{} · {ano_tit} · onde foi", s["nome"].as_str().unwrap_or("")), "total": num(&fo["total"])?, "linhas": linhas(&fo["linhas"])?,
                "rodape": if parcial { format!("Despesa consolidada {fim_mes} (PCMGE065) · liquidado · TCM-BA") } else { format!("Despesa consolidada {fim_mes} (PCMGE065) = Balanço {ano} · liquidado · TCM-BA") },
                "de": t(&s["t"], "setor")?, "ate": ate, "min": 1.5 });
            if !fo["destaque"].is_null() {
                f["destaque"] = fo["destaque"].clone();
            }
            folhas.push(f);
        }
    }
    Ok(folhas)
}

/// Titulos das folhas cuja soma das linhas difere do total: nao se mexe em numero aqui, o pedido volta para quem mandou.
pub fn folhas_ruins(folhas: &[Value]) -> Vec<String> {
    folhas.iter().filter(|f| (f["linhas"].as_array().into_iter().flatten().filter_map(|l| l[1].as_f64()).sum::<f64>() - f["total"].as_f64().unwrap_or(f64::NAN)).abs() > 0.05 || f["total"].as_f64().is_none()).map(|f| f["titulo"].as_str().unwrap_or("").to_string()).collect()
}

/// Numero do video no comeco do titulo do pedido ("150 Cidade ...").
pub fn numero_do_titulo(meta: &Value) -> Option<String> {
    re(r"^\s*(\d{1,4})\b").captures(meta["titulo"].as_str().unwrap_or("")).map(|m| m[1].to_string())
}

/// Pedido meta.json + audio de um video cujo codigo JA esta no pacote: monta a ficha (base = `codigo_do_tsx` + `composicao_kit`).
/// `dur_audio`: duracao da narracao normalizada. Folha com soma errada = erro (avisar quem mandou).
pub fn ficha_de_meta(meta: &Value, base: &Value, n: &str, dur_audio: f64) -> Result<(Value, Vec<Value>), String> {
    let projeto = base["projeto"].as_str().unwrap_or("");
    let nome = re(r"^\d+\.\s*orçamento de\s*").replace(projeto, "").to_uppercase();
    if !re(NOME).is_match(&nome) {
        return Err(format!("nome do video fora do formato: {}", nome.chars().take(40).collect::<String>()));
    }
    let folhas = folhas_de_meta(meta)?;
    let ruins = folhas_ruins(&folhas);
    if !ruins.is_empty() {
        return Err(format!("folha com soma diferente do total (avise quem mandou, nao se mexe em numero aqui): {}", ruins.join("; ").chars().take(200).collect::<String>()));
    }
    let dur = ((dur_audio + 0.9) * 100.0).round() / 100.0;
    let capa = meta["capa"]["t"].as_f64().map_or(((dur - 3.0) * 30.0) as i64, |t| (t * 30.0).round() as i64);
    let ficha = json!({ "_explica": "Montada pelo Braniac (render_magno.processar_meta) a partir do meta.json + audio do pedido.", "numero": n, "nome": nome, "projeto": projeto,
        "composicao_estudio": base["composicao_estudio"], "composicao_kit": base["composicao_kit"], "arquivo_mapa": base["arquivo_mapa"],
        "folhas": if folhas.is_empty() { Value::Null } else { json!(format!("videos/{n}/folhas.json")) }, "quadro_capa": capa, "duracao_esperada_s": dur });
    Ok((ficha, folhas))
}

pub fn folhas_json(folhas: &[Value]) -> Value {
    json!({ "entrada": "", "saida": "", "pasta_tmp": "", "folhas": folhas })
}

/// Linha do videos/tempos.jsonl (quanto cada video levou).
pub fn linha_tempos(f: &Value, minutos: f64, nucleos: u32, quando: &str) -> String {
    json!({ "numero": f["numero"], "nome": f["nome"], "minutos": (minutos * 10.0).round() / 10.0, "nucleos": nucleos, "quando": quando }).to_string()
}

/// Onde o render deixa o video e a capa, relativo ao pacote.
pub fn saidas(f: &Value) -> (String, String) {
    let (p, n) = (f["projeto"].as_str().unwrap_or(""), f["nome"].as_str().unwrap_or(""));
    (format!("estudio-3d/edicoes/{p}/{n}.mp4"), format!("estudio-3d/edicoes/{p}/{n} - capa.jpg"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    const PROJ: &str = "151. orçamento de cidade";

    fn zip(ficha: &Value, extras: &[(&str, &str)], sem: &[&str]) -> BTreeMap<String, Vec<u8>> {
        let mut a: BTreeMap<String, Vec<u8>> = [
            ("videos/151/video.json".to_string(), ficha.to_string()),
            ("videos/151/folhas.json".into(), "{}".into()),
            ("estudio-3d/src/Root.tsx".into(), "<Composition id=\"OrcamentoCidade\" />".into()),
            (format!("estudio-3d/projetos/{PROJ}/audio/narracao-norm.wav"), "RIFF".into()),
            (format!("estudio-3d/projetos/{PROJ}/mapas/cidade.mp4"), "x".into()),
        ]
        .into_iter()
        .map(|(k, v)| (k, v.into_bytes()))
        .collect();
        a.extend(extras.iter().map(|(k, v)| (k.to_string(), v.as_bytes().to_vec())));
        sem.iter().for_each(|s| drop(a.remove(*s)));
        a
    }

    fn boa() -> Value {
        json!({"numero": "151", "nome": "CIDADE", "projeto": PROJ, "composicao_estudio": "OrcamentoCidade", "composicao_kit": null, "arquivo_mapa": "cidade.mp4", "folhas": "videos/151/folhas.json", "quadro_capa": 5000, "duracao_esperada_s": 170.5})
    }

    fn confere(z: &BTreeMap<String, Vec<u8>>) -> Result<(Value, Vec<String>, usize), String> {
        let lista: Vec<Entrada> = z.iter().map(|(n, d)| Entrada { nome: n.clone(), tam: d.len() as u64, dir: false }).collect();
        conferir_zip(false, true, 1, true, &lista, &|n| z.get(n).cloned()).map(|(f, b, i)| (f, b.into_iter().map(|i| lista[i].nome.clone()).collect(), i))
    }

    #[test]
    fn pacote_bom_extrai_so_cena_e_dados() {
        let z = zip(&boa(), &[("estudio-3d/scripts/render.mjs", "mal"), ("estudio-3d/projetos/_arte/gera.py", "mal"), ("renderizar.mjs", "mal"), ("estudio-3d/src/videos/cidade/Cena.tsx", "export const a = 1"), ("kit-mapas/public/satelite/a.exe", "MZ")], &[]);
        let (f, boas, ignoradas) = confere(&z).unwrap();
        assert_eq!((f["numero"].as_str(), ignoradas), (Some("151"), 4)); // script, programa e o orquestrador nao entram
        assert!(boas.contains(&"estudio-3d/src/videos/cidade/Cena.tsx".to_string()) && !boas.iter().any(|b| b.ends_with(".mjs") || b.ends_with(".py") || b.ends_with(".exe")));
        assert_eq!(saidas(&f), (format!("estudio-3d/edicoes/{PROJ}/CIDADE.mp4"), format!("estudio-3d/edicoes/{PROJ}/CIDADE - capa.jpg")));
    }

    #[test]
    fn ficha_fora_do_formato_e_recusada_antes_de_extrair() {
        for (campo, valor) in [("composicao_estudio", json!("X & calc")), ("nome", json!("A\" & del")), ("projeto", json!("../fora")), ("arquivo_mapa", json!("a b.mp4")), ("folhas", json!("videos/139/folhas.json")), ("numero", json!("15x")), ("numero", json!("152")), ("quadro_capa", json!(1.5)), ("composicao_kit", json!("a;b")), ("duracao_esperada_s", json!("170"))] {
            let mut f = boa();
            f[campo] = valor;
            assert!(confere(&zip(&f, &[], &[])).is_err(), "{campo}");
        }
        let mut z = zip(&boa(), &[], &[]);
        z.insert("videos/151/video.json".into(), b"{nao e json".to_vec());
        assert!(confere(&z).unwrap_err().contains("JSON"));
        z.insert("videos/152/video.json".into(), boa().to_string().into_bytes());
        assert!(confere(&z).unwrap_err().contains("UMA ficha"));
        // composicao que nao esta registrada no Root.tsx do zip
        let mut f = boa();
        f["composicao_estudio"] = json!("OutraCoisa");
        assert!(confere(&zip(&f, &[], &[])).unwrap_err().contains("nao esta registrada"));
        // com composicao_kit precisa do Root do kit; sem ela, do mapa pronto
        let mut f = boa();
        f["composicao_kit"] = json!("OrcamentoCidade");
        assert!(confere(&zip(&f, &[], &[])).unwrap_err().contains("kit-mapas/src/Root.tsx"));
        assert!(confere(&zip(&f, &[("kit-mapas/src/Root.tsx", "x")], &[])).is_ok());
        assert!(confere(&zip(&boa(), &[], &[&format!("estudio-3d/projetos/{PROJ}/mapas/cidade.mp4")])).unwrap_err().contains("mapa pronto"));
    }

    #[test]
    fn caminho_que_sai_da_pasta_chave_falta_de_audio_e_desligado() {
        for nome in ["../fora.tsx", "/etc/x.ts", "C:/x.ts", "videos/../../x.json", "videos\\..\\x.json"] {
            assert!(confere(&zip(&boa(), &[(nome, "x")], &[])).unwrap_err().contains("caminho invalido"), "{nome}");
        }
        let chave = format!("const k = 'gsk_{}'", "a".repeat(30));
        assert!(confere(&zip(&boa(), &[("estudio-3d/src/a.ts", &chave)], &[])).unwrap_err().contains("chave de API"));
        assert!(pode_gravar("videos/1/foto.png", chave.as_bytes()).is_ok()); // binario nao e varrido
        assert!(confere(&zip(&boa(), &[], &[&format!("estudio-3d/projetos/{PROJ}/audio/narracao-norm.wav")])).unwrap_err().contains("faltou no zip"));
        let z = zip(&boa(), &[], &[]);
        let lista: Vec<Entrada> = z.iter().map(|(n, d)| Entrada { nome: n.clone(), tam: d.len() as u64, dir: false }).collect();
        let ler = |n: &str| z.get(n).cloned();
        assert!(conferir_zip(true, true, 1, true, &lista, &ler).unwrap_err().contains("desligado"));
        assert!(conferir_zip(false, false, 1, true, &lista, &ler).unwrap_err().contains("nao esta instalado"));
        assert!(conferir_zip(false, true, 2, true, &lista, &ler).unwrap_err().contains("UM zip"));
        assert!(conferir_zip(false, true, 1, false, &lista, &ler).unwrap_err().contains("corrompido"));
        let grande = [Entrada { nome: "videos/1/a.mp4".into(), tam: MAX_BYTES + 1, dir: false }, Entrada { nome: "videos/1/".into(), tam: 0, dir: true }];
        assert!(entradas(&grande).unwrap_err().contains("grande demais"));
    }

    #[test]
    fn render_e_lista_de_argumentos_e_cai_para_4_abas() {
        assert_eq!(args_render("151", 6, None).unwrap(), ["node", "renderizar.mjs", "videos/151/video.json", "--nucleos=6", "--gl=angle", "--quadros-por-parte=600"]);
        assert_eq!(args_render("151", 99, Some("folhas")).unwrap()[3..], ["--nucleos=8", "--gl=angle", "--quadros-por-parte=600", "--de=folhas"]);
        assert!(args_render("151; calc", 6, None).is_err() && args_render("151", 6, Some("x & y")).is_err());
        assert_eq!((tentativas(6), tentativas(12), tentativas(4), tentativas(2)), (vec![6, 4], vec![8, 4], vec![4], vec![2]));
        let vistos = RefCell::new(Vec::new());
        let roda = |codigos: &'static [i32]| {
            vistos.borrow_mut().clear();
            let v = &vistos;
            move |a: &[String]| -> Result<i32, String> {
                v.borrow_mut().push(a[3].clone());
                Ok(codigos[v.borrow().len() - 1])
            }
        };
        let log = || "linha 1\n\n  erro de placa  \nfim\n".to_string();
        assert!(renderizar("151", 6, None, &roda(&[0]), &log).is_ok());
        assert_eq!(*vistos.borrow(), ["--nucleos=6"]);
        assert!(renderizar("151", 6, None, &roda(&[1, 0]), &log).is_ok());
        assert_eq!(*vistos.borrow(), ["--nucleos=6", "--nucleos=4"]);
        assert_eq!(renderizar("151", 6, None, &roda(&[1, 1]), &log).unwrap_err(), "render falhou: linha 1 | erro de placa | fim");
        assert!(renderizar("151", 6, None, &roda(&[2, 0]), &log).unwrap_err().starts_with("conferencia final reprovou"));
        assert_eq!(vistos.borrow().len(), 1); // reprovado na conferencia: nao tenta de novo
    }

    #[test]
    fn folhas_do_meta_copiam_os_numeros_e_soma_errada_barra() {
        let meta = json!({"titulo": "150 Cidade", "capa": {"t": 100.0}, "cenas": [
            {"peca": "mapa", "dados": {}},
            {"peca": "maquete", "fim": 60.0, "dados": {"ano": "2024", "folha_receita": {"total": "R$ 3.000,50", "linhas": [["Impostos", "1.000,50"], ["Repasses", 2000]], "t": 20.0, "destaque": "Repasses"},
                "setores": [{"nome": "Educação", "t": 30.0, "folha": {"total": 10, "linhas": [["Escolas", 6], ["Merenda", 4]]}}, {"nome": "Sem folha", "t": 35.0}, {"nome": "Saúde", "t": 40.0, "folha": {"total": 5, "linhas": [["Postos", 5]]}}]}},
            {"peca": "maquete", "fim": 90.0, "dados": {"ano": "2026 ATÉ SET", "setores": [{"nome": "Obras", "t": 70.0, "folha": {"total": 2, "linhas": [["Ruas", 2]]}}]}},
        ]});
        let f = folhas_de_meta(&meta).unwrap();
        assert_eq!(f.iter().map(|x| x["titulo"].as_str().unwrap()).collect::<Vec<_>>(), ["Receita arrecadada · 2024", "Educação · 2024 · onde foi", "Saúde · 2024 · onde foi", "Obras · 2026 até set · onde foi"]);
        assert_eq!((f[0]["total"].as_f64(), f[0]["linhas"][0][1].as_f64(), f[0]["de"].as_f64(), f[0]["ate"].as_f64(), f[0]["destaque"].as_str()), (Some(3000.5), Some(1000.5), Some(20.0), Some(30.0), Some("Repasses")));
        assert_eq!((f[1]["ate"].as_f64(), f[2]["ate"].as_f64(), f[3]["ate"].as_f64()), (Some(40.0), Some(60.0), Some(90.0)));
        assert!(f[1]["rodape"].as_str().unwrap().contains("dez/2024") && f[3]["rodape"].as_str().unwrap().starts_with("Despesa consolidada set/2026 (PCMGE065) · liquidado"));
        assert!(folhas_ruins(&f).is_empty());
        let base = json!({"projeto": "150. orçamento de cidade nova", "composicao_estudio": "OrcamentoCidadeNova", "arquivo_mapa": "cidade.mp4", "composicao_kit": null});
        let (ficha, fol) = ficha_de_meta(&meta, &base, "150", 120.0).unwrap();
        assert_eq!((ficha["nome"].as_str(), ficha["quadro_capa"].as_i64(), ficha["duracao_esperada_s"].as_f64(), ficha["folhas"].as_str(), fol.len()), (Some("CIDADE NOVA"), Some(3000), Some(120.9), Some("videos/150/folhas.json"), 4));
        assert_eq!(folhas_json(&fol)["folhas"].as_array().map(Vec::len), Some(4));
        let mut errado = meta.clone();
        errado["cenas"][1]["dados"]["setores"][0]["folha"]["total"] = json!(11);
        assert!(ficha_de_meta(&errado, &base, "150", 120.0).unwrap_err().contains("Educação · 2024"));
        errado["cenas"][1]["dados"]["setores"][0]["folha"]["total"] = json!("dez");
        assert!(folhas_de_meta(&errado).is_err());
        assert!(ficha_de_meta(&meta, &json!({"projeto": "150. orçamento de a & b"}), "150", 1.0).unwrap_err().contains("fora do formato"));
        assert_eq!((numero_do_titulo(&meta).as_deref(), numero_do_titulo(&json!({"titulo": "Cidade 150"}))), (Some("150"), None));
        assert_eq!(ficha_de_meta(&json!({"cenas": []}), &base, "150", 63.1).unwrap().0["quadro_capa"], 1830); // sem capa pedida: 3 s antes do fim
    }

    #[test]
    fn acha_o_codigo_ja_instalado_e_a_pasta_do_kit() {
        let tsx = "import x;\nconst P = '150. orçamento de cidade';\nexport const OrcamentoCidade: React.FC = () => null;\nstaticFile(`mapas/cidade_v2.mp4`)";
        let base = codigo_do_tsx(tsx, "150").unwrap();
        assert_eq!(base, json!({"projeto": "150. orçamento de cidade", "composicao_estudio": "OrcamentoCidade", "arquivo_mapa": "cidade_v2.mp4"}));
        assert_eq!(codigo_do_tsx(tsx, "15"), None); // 15 nao e 150
        let root = "import * as Cid from \"./videos/cidade-150\";\n<Composition id=\"OrcamentoCidade\" component={Cid.Video} />";
        assert_eq!((composicao_kit(&base, root), composicao_kit(&base, "nada")), (json!("OrcamentoCidade"), Value::Null));
        assert_eq!(pasta_kit(root, "OrcamentoCidade").as_deref(), Ok("cidade-150"));
        assert!(pasta_kit(root, "Outra").is_err());
        assert_eq!((pasta_tiles("export const PASTA_TILES = \"satelite/cidade\";").as_deref(), pasta_tiles("x")), (Some("satelite/cidade"), None));
        assert!(linha_tempos(&boa(), 12.34, 6, "01/01/2026 10:00").contains("\"minutos\":12.3"));
    }
}
