//! Producao completa de video de prefeitura (link/preparar_magno.py): as regras e os parses que deixam o video pronto para o render
//! quando a narracao chega. Sem IA e sem rede: palavras com tempo -> blocos por ano -> folhas; conferencia da voz contra o roteiro
//! (numero pulado ou trocado reprova, buraco na transcricao reprova); texto para a voz; divisao em blocos; pausas encurtadas.
//! Ficou de fora (closure ou argumentos devolvidos para quem chama executar): ffmpeg/ffprobe, Whisper, os pedidos de TTS (Google e
//! Gemini, com a chave so no ambiente), ler/gravar os arquivos do pacote e a leitura do registro do Windows.
//! Diferencas: os prefixos tirados do nome da acao e as trocas do texto para a voz vem de configuracao (no Python havia nomes
//! de pessoas no codigo); os setores de um ano sao lidos em ordem alfabetica do JSON (a ordem final e a da FALA, como no Python).
#![allow(dead_code)]
use crate::escuta::semelhanca;
use crate::link_auditar::valores;
use crate::texto_pt::sem_acento;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::OnceLock;

pub const MODELO_VOZ: &str = "gemini-2.5-flash-preview-tts"; // um modelo so do comeco ao fim: alternar muda o timbre
pub const ESTILO_VOZ: &str = "Narre em português do Brasil, voz de locutor de notícias, firme e clara, sem emoção exagerada, com uma pausa curta entre os parágrafos. Leia cada número exatamente como está escrito e nunca acrescente a palavra reais: ";

fn rx(c: &'static OnceLock<Regex>, p: &str) -> &'static Regex {
    c.get_or_init(|| Regex::new(p).expect("regex fixa"))
}

// ---------- difflib.SequenceMatcher(None, a, b, autojunk=False).get_opcodes() ----------

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    Igual,
    Troca,
    Apaga,
    Insere,
}

fn maior_bloco<T: Eq + Hash>(a: &[T], b2j: &HashMap<&T, Vec<usize>>, alo: usize, ahi: usize, blo: usize, bhi: usize) -> (usize, usize, usize) {
    let (mut bi, mut bj, mut bk) = (alo, blo, 0);
    let mut j2len: HashMap<usize, usize> = HashMap::new();
    for i in alo..ahi {
        let mut novo = HashMap::new();
        for &j in b2j.get(&a[i]).into_iter().flatten() {
            if j < blo {
                continue;
            }
            if j >= bhi {
                break;
            }
            let k = j.checked_sub(1).and_then(|x| j2len.get(&x)).copied().unwrap_or(0) + 1;
            novo.insert(j, k);
            if k > bk {
                (bi, bj, bk) = (i + 1 - k, j + 1 - k, k);
            }
        }
        j2len = novo;
    }
    (bi, bj, bk)
}

/// As mesmas operacoes do Python (igual / troca / apaga / insere) para ir de `a` a `b`.
pub fn opcodes<T: Eq + Hash>(a: &[T], b: &[T]) -> Vec<(Op, usize, usize, usize, usize)> {
    let mut b2j: HashMap<&T, Vec<usize>> = HashMap::new();
    for (j, x) in b.iter().enumerate() {
        b2j.entry(x).or_default().push(j);
    }
    let (mut fila, mut blocos) = (vec![(0, a.len(), 0, b.len())], Vec::new());
    while let Some((alo, ahi, blo, bhi)) = fila.pop() {
        let (i, j, k) = maior_bloco(a, &b2j, alo, ahi, blo, bhi);
        if k > 0 {
            blocos.push((i, j, k));
            if alo < i && blo < j {
                fila.push((alo, i, blo, j));
            }
            if i + k < ahi && j + k < bhi {
                fila.push((i + k, ahi, j + k, bhi));
            }
        }
    }
    blocos.sort();
    blocos.push((a.len(), b.len(), 0));
    let (mut out, mut i, mut j) = (Vec::new(), 0, 0);
    for (ai, bj, k) in blocos {
        let op = match (i < ai, j < bj) {
            (true, true) => Some(Op::Troca),
            (true, false) => Some(Op::Apaga),
            (false, true) => Some(Op::Insere),
            _ => None,
        };
        if let Some(op) = op {
            out.push((op, i, ai, j, bj));
        }
        (i, j) = (ai + k, bj + k);
        if k > 0 {
            match out.last_mut() {
                Some((Op::Igual, _, a2, _, b2)) if *a2 == ai && *b2 == bj => (*a2, *b2) = (i, j), // blocos colados viram um so
                _ => out.push((Op::Igual, ai, i, bj, j)),
            }
        }
    }
    out
}

// ---------- palavras com tempo ----------

#[derive(Debug, Clone, PartialEq)]
pub struct Pal {
    pub w: String,
    pub s: f64,
    pub e: f64,
}

/// Minusculas, sem acento, so letras, digitos, "_" e "@".
pub fn norm(w: &str) -> String {
    sem_acento(w).chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '@').collect()
}

fn r2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

pub fn pal_de_json(v: &Value) -> Vec<Pal> {
    v.as_array().into_iter().flatten().map(|p| Pal { w: p["w"].as_str().unwrap_or("").to_string(), s: p["s"].as_f64().unwrap_or(0.0), e: p["e"].as_f64().unwrap_or(0.0) }).collect()
}

/// Le o palavras.ts do estudio ("export const PALAVRAS ... = [ ... ];").
pub fn palavras_do_ts(t: &str) -> Result<Vec<Pal>, String> {
    let (a, b) = (t.find("= [").ok_or("palavras.ts sem lista")? + 2, t.rfind(']').ok_or("palavras.ts sem lista")? + 1);
    serde_json::from_str::<Value>(t.get(a..b).ok_or("palavras.ts sem lista")?).map(|v| pal_de_json(&v)).map_err(|_| "palavras.ts ilegivel".to_string())
}

/// Tempo estimado (sem voz), so para revisar o visual: nunca vai para o render final.
pub fn sintetico(roteiro_falado: &str) -> Vec<Pal> {
    let (mut out, mut t) = (Vec::new(), 0.0);
    for par in roteiro_falado.replace("\r\n", "\n").split("\n\n").filter(|p| !p.trim().is_empty()) {
        for w in par.split_whitespace() {
            let d = 0.25 + 0.035 * w.chars().count() as f64;
            out.push(Pal { w: w.to_string(), s: r2(t), e: r2(t + d) });
            t += d + 0.05;
        }
        t += 0.45;
    }
    out
}

pub fn palavras_ts(pal: &[Pal]) -> String {
    let lista: Vec<Value> = pal.iter().map(|p| json!({ "w": p.w, "s": p.s, "e": p.e })).collect();
    format!("// SINTETICO (sem voz): so para revisar quadros. O preparar_magno troca pelo alinhamento da narracao real.\nexport const PALAVRAS: {{w: string; s: number; e: number}}[] = {};\n", json!(lista))
}

fn alvo(frase: &str) -> Vec<String> {
    frase.split_whitespace().map(norm).collect()
}

/// Quando a frase e falada pela n-esima vez. Se nao casar exato, aceita a janela bem parecida (>= 0,8) com as 6 primeiras palavras
/// (a transcricao escreve diferente nomes longos); senao, erro.
pub fn quando(pal: &[Pal], frase: &str, n: usize) -> Result<f64, String> {
    let (alvo, ws): (Vec<String>, Vec<String>) = (alvo(frase), pal.iter().map(|p| norm(&p.w)).collect());
    let nao = || format!("nao achei '{frase}' (vez {n}) nas palavras");
    if alvo.is_empty() || n == 0 {
        return Err(nao());
    }
    if let Some(i) = (0..(ws.len() + 1).saturating_sub(alvo.len())).filter(|i| ws[*i..*i + alvo.len()] == alvo[..]).nth(n - 1) {
        return Ok(pal[i].s);
    }
    let curto = &alvo[..alvo.len().min(6)];
    let (junto, mut boas, mut ultima) = (curto.join(" "), Vec::new(), None::<usize>);
    for i in 0..(ws.len() + 1).saturating_sub(curto.len()) {
        if semelhanca(&ws[i..i + curto.len()].join(" "), &junto) >= 0.8 {
            if ultima.is_none_or(|u| i - u > curto.len()) {
                boas.push(i); // uma por ocorrencia
            }
            ultima = Some(i);
        }
    }
    boas.get(n - 1).map(|i| pal[*i].s).ok_or_else(nao)
}

/// Primeira vez que a frase comeca entre t0 e t1. Busca presa ao bloco do ano ("a mais do que arrecadou" tambem conta como "Arrecadou").
pub fn depois(pal: &[Pal], frase: &str, t0: f64, t1: f64) -> Option<f64> {
    let (alvo, ws): (Vec<String>, Vec<String>) = (alvo(frase), pal.iter().map(|p| norm(&p.w)).collect());
    (0..(ws.len() + 1).saturating_sub(alvo.len())).find(|i| t0 <= pal[*i].s && pal[*i].s < t1 && ws[*i..*i + alvo.len()] == alvo[..]).map(|i| pal[i].s)
}

/// (inicio, fim) de cada ano falado: o ano seguido de "Arrecadou" nas 4 palavras seguintes, depois de `desde` e do ano anterior;
/// termina no inicio do proximo, e o ultimo na frase que fecha os setores.
pub fn blocos(pal: &[Pal], anos: &[String], fim_frase: &str, desde: f64) -> Result<Vec<(f64, f64)>, String> {
    let ws: Vec<String> = pal.iter().map(|p| norm(&p.w)).collect();
    let (mut ini, mut j) = (Vec::new(), 0);
    for a in anos {
        let i = (j..ws.len())
            .find(|i| ws[*i] == *a && pal[*i].s >= desde && ws[(*i + 1).min(ws.len())..(*i + 5).min(ws.len())].iter().any(|w| w == "arrecadou"))
            .ok_or_else(|| format!("nao achei o bloco do ano {a} ('{a}' seguido de 'Arrecadou') nas palavras"))?;
        ini.push(pal[i].s);
        j = i + 1;
    }
    let Some(ultimo) = ini.last().copied() else { return Ok(Vec::new()) };
    let fim = depois(pal, fim_frase, ultimo, f64::MAX).filter(|t| *t != 0.0).unwrap_or_else(|| pal.last().map_or(0.0, |p| p.e));
    Ok(ini.iter().copied().zip(ini.iter().skip(1).copied().chain([fim])).collect())
}

/// Tempos do mapa = os da fala (A ate o ano que abre, B ate "Gastou" + 0,8 s). Devolve o roteiro.ts do kit com os tempos trocados.
pub fn kit(roteiro_ts: &str, mapa: &Value, pal: &[Pal]) -> Result<String, String> {
    static R: [OnceLock<Regex>; 4] = [OnceLock::new(), OnceLock::new(), OnceLock::new(), OnceLock::new()];
    let m = |k: &str| quando(pal, mapa[k].as_str().unwrap_or(""), 1);
    let ano = quando(pal, "2024", 2)?; // o "2024." que abre o ano (o 1o e o da frase de abertura)
    let a = [("inicio", 0.2), ("r24", m("r24")?), ("r25", m("r25")?), ("r26", m("r26")?), ("abri", quando(pal, "Eu abri", 1)?), ("tribunal", quando(pal, "Tribunal", 1)?), ("ano", ano)];
    let gas = quando(pal, "Gastou", 1)?;
    let b = [("ano", ano), ("arrecadou", quando(pal, "Arrecadou", 2)?), ("n12", m("n_b")?), ("gastou", gas)];
    let lista = |x: &[(&str, f64)]| x.iter().map(|(k, v)| format!("{k}: s({v:.2})")).collect::<Vec<_>>().join(", ");
    let t = rx(&R[0], r"export const INI_B = s\([\d.]+\);").replace_all(roteiro_ts, regex::NoExpand(&format!("export const INI_B = s({:.2});", ano - 0.12)));
    let t = rx(&R[1], r"export const DURACAO = s\([\d.]+\);").replace_all(&t, regex::NoExpand(&format!("export const DURACAO = s({:.2});", gas + 0.8)));
    let t = rx(&R[2], r"export const A = \{[^}]*\};").replace_all(&t, regex::NoExpand(&format!("export const A = {{{}}};", lista(&a))));
    Ok(rx(&R[3], r"export const B = \{[^}]*\};").replace_all(&t, regex::NoExpand(&format!("export const B = {{{}}};", lista(&b)))).into_owned())
}

fn capitaliza(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|p| p.to_uppercase().chain(c.flat_map(char::to_lowercase)).collect()).unwrap_or_default()
}

/// Uma folha por assunto falado, na ordem da fala: receita do ano (por origem) e depois um setor por vez (as 6 maiores acoes +
/// "Outras N acoes"). Cada folha entra na palavra do assunto e sai quando o proximo e falado; a ultima do ano sai quando o ano
/// seguinte comeca. `setores`: {ano: {setor: {liquidado, acoes: [[nome, valor]]}}}; `receita(ano)`: [[origem, valor, ...]].
/// Os numeros sao copiados do levantamento, nunca calculados aqui (so a soma das linhas, que o gerador das folhas confere).
pub fn folhas(cfg: &Value, setores: &Value, receita: &dyn Fn(&str) -> Option<Value>, pal: &[Pal]) -> Result<Vec<Value>, String> {
    let vazio = serde_json::Map::new();
    let set = setores.as_object().unwrap_or(&vazio);
    let falas = &cfg["falas_setor"]; // setor -> palavra falada
    let cfg_s = |k: &str, padrao: &str| cfg[k].as_str().unwrap_or(padrao).to_string();
    let anos: Vec<String> = set.keys().map(|k| k.chars().take(4).collect()).collect();
    let bl = blocos(pal, &anos, &cfg_s("fim_setores", "A ação de"), quando(pal, "Eu abri", 1)?)?;
    let trib = cfg_s("tribunal", "TCM-BA");
    let prefixos: Vec<&str> = cfg["tirar_prefixos"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_else(|| vec!["por:"]);
    let limpa = |nome: &str| {
        let sem = prefixos.iter().find_map(|p| nome.strip_prefix(p).filter(|r| r.starts_with(char::is_whitespace))).unwrap_or(nome);
        capitaliza(sem.trim()).chars().take(58).collect::<String>()
    };
    let soma = |v: &[f64]| r2(v.iter().sum());
    let mut fol = Vec::new();
    for ((ano, tab), (b0, b1)) in set.iter().zip(bl) {
        let a = &ano.chars().take(4).collect::<String>();
        let ate_set = a == "2026";
        let mes = if ate_set { format!("{}/{a}", cfg_s("mes26", "set")) } else { format!("{}/{a}", cfg["mes_ano"][a].as_str().unwrap_or("dez")) }; // ultimo mes entregue
        let rec: Vec<Value> = receita(a).and_then(|r| r.as_array().cloned()).ok_or_else(|| format!("{a}: sem a receita do ano"))?.into_iter().filter(|l| l[1].as_f64().is_some_and(|v| v != 0.0)).map(|l| json!([l[0], l[1]])).collect();
        let de_rec = depois(pal, "Arrecadou", b0, b1).ok_or_else(|| format!("{a}: 'Arrecadou' fora do bloco do ano ({b0:.1}-{b1:.1} s); confira a transcrição"))?;
        let mut itens = Vec::new();
        let mut faltou = Vec::new();
        for (s, d) in tab.as_object().unwrap_or(&vazio) {
            let Some(fala) = falas[s].as_str().filter(|_| d["liquidado"].as_f64().is_some_and(|v| v > 0.0)) else { continue };
            match depois(pal, fala, de_rec, b1) {
                Some(t) => itens.push((s, d, t)),
                None => faltou.push(s.as_str()),
            }
        }
        if !faltou.is_empty() {
            return Err(format!("{a}: setor sem a palavra na fala: {} (confira falas_setor e a transcrição)", faltou.join(", ")));
        }
        itens.sort_by(|x, y| x.2.total_cmp(&y.2));
        let doc_r = cfg["doc_receita"][a].as_str().filter(|x| !x.is_empty()).unwrap_or(if ate_set { "PCMGE016" } else { "Balanço · PCAGO015" });
        fol.push(json!({ "tipo": "receita", "titulo": format!("{doc_r} · Receita arrecadada · {mes} · {trib}"), "total": soma(&rec.iter().filter_map(|l| l[1].as_f64()).collect::<Vec<_>>()),
            "linhas": rec, "rodape": cfg["rodape_receita"][a].as_str().unwrap_or(""), "de": de_rec, "ate": itens.first().map_or(b1, |x| x.2) }));
        let doc_d = cfg["doc_despesa"][a].as_str().filter(|x| !x.is_empty()).unwrap_or(if ate_set { "PCMGE015" } else { "PCMGE065" });
        for (k, (s, d, t)) in itens.iter().enumerate() {
            let acs = d["acoes"].as_array().cloned().unwrap_or_default();
            let mut linhas: Vec<Value> = acs.iter().take(6).map(|x| json!([limpa(x[0].as_str().unwrap_or("")), x[1]])).collect();
            if acs.len() > 6 {
                linhas.push(json!([format!("Outras {} ações", acs.len() - 6), soma(&acs[6..].iter().filter_map(|x| x[1].as_f64()).collect::<Vec<_>>())]));
            }
            let doc = if s.starts_with("Câmara") && trib == "TCM-BA" { "PCMGE015 da Câmara" } else { doc_d }; // na BA a Camara presta contas no proprio demonstrativo
            fol.push(json!({ "tipo": "despesa", "titulo": format!("{doc} · Despesa por ação · {s} · {mes} · {trib}"), "total": d["liquidado"], "linhas": linhas,
                "rodape": cfg["rodape_despesa"][a].as_str().filter(|x| !x.is_empty()).unwrap_or("liquidado"), "de": t, "ate": itens.get(k + 1).map_or(b1, |x| x.2), "min": 1.5 }));
        }
    }
    Ok(fol)
}

// ---------- conferencia da voz ----------

fn sig(v: i128) -> String {
    let s = v.to_string();
    let t = s.trim_end_matches('0');
    if t.is_empty() { "0".into() } else { t.to_string() }
}

/// Cada valor do roteiro tem de aparecer na transcricao, na mesma ordem (a voz nao pode pular nem trocar numero). Devolve os problemas.
/// Tolera erro de ESCALA da transcricao e ultimo digito cortado (compara os digitos significativos); troca de digito reprova.
pub fn conferir_numeros(roteiro_falado: &str, transcricao: &str) -> Vec<String> {
    let (a, b) = (valores(roteiro_falado), valores(transcricao));
    let (sa, sb): (Vec<String>, Vec<String>) = (a.iter().map(|x| sig(*x)).collect(), b.iter().map(|x| sig(*x)).collect());
    let ok = |r: &String, v: &String| r == v || (v.len() >= 2 && r.starts_with(v.as_str()));
    opcodes(&sa, &sb)
        .into_iter()
        .filter(|(op, i1, i2, j1, j2)| *op == Op::Apaga || (*op == Op::Troca && !(i2 - i1 == j2 - j1 && sa[*i1..*i2].iter().zip(&sb[*j1..*j2]).all(|(r, v)| ok(r, v)))))
        .map(|(_, i1, i2, j1, j2)| format!("roteiro {:?} x voz {:?}", &a[i1..i2], &b[j1..j2]))
        .collect()
}

/// Para antes da sincronia se a transcricao tem buraco (fala sem palavra por mais de `max_buraco` s) ou se a voz falou numero de 2+
/// digitos que nao esta no roteiro.
pub fn conferir_transcricao(pal: &[Pal], roteiro_falado: &str, max_buraco: f64) -> Vec<String> {
    static N: OnceLock<Regex> = OnceLock::new();
    static W: OnceLock<Regex> = OnceLock::new();
    let mut erros: Vec<String> = pal.windows(2).filter(|p| p[1].s - p[0].e > max_buraco).map(|p| format!("buraco de {:.1} s sem palavra depois de {:.1} s", p[1].s - p[0].e, p[0].e)).collect();
    let no_rot: Vec<&str> = rx(&N, r"\d+").find_iter(roteiro_falado).map(|m| m.as_str()).collect();
    let pontua: &[char] = &['.', ',', ';', ':', '!', '?'];
    let mut estranhos: Vec<&str> = pal.iter().filter(|p| rx(&W, r"^\d{2,}[.,;:!?]?$").is_match(&p.w)).map(|p| p.w.trim_matches(pontua)).filter(|w| !no_rot.contains(w)).collect();
    estranhos.sort();
    estranhos.dedup();
    if !estranhos.is_empty() {
        erros.push(format!("a voz falou número que não está no roteiro: {}", estranhos.join(", ")));
    }
    erros
}

// ---------- texto para a voz e divisao em blocos ----------

fn depois_e(resto: &str, palavra: &str) -> bool {
    resto.strip_prefix(palavra).is_some_and(|r| !r.starts_with(|c: char| c.is_alphanumeric() || c == '_'))
}

/// O roteiro abrevia "3 milhões e 500" (= 3,5 mi) e a voz completava sozinha com "reais" ou trocava o numero: para a VOZ o "mil" vai
/// escrito, e "48 mil e 300" ganha "reais". Roteiro e legenda continuam como estao. `trocas`: (como esta no roteiro, como se fala).
pub fn texto_para_voz(p: &str, trocas: &[(&str, &str)]) -> String {
    static A: OnceLock<Regex> = OnceLock::new();
    static B: OnceLock<Regex> = OnceLock::new();
    let completa = |t: &str, r: &Regex, fim: &str, barra: &[&str]| {
        let (mut out, mut ult) = (String::new(), 0);
        for m in r.find_iter(t) {
            out.push_str(&t[ult..m.end()]);
            if !barra.iter().any(|b| depois_e(&t[m.end()..], b)) {
                out.push_str(fim);
            }
            ult = m.end();
        }
        out + &t[ult..]
    };
    let p = trocas.iter().fold(p.to_string(), |t, (de, para)| t.replace(de, para));
    let p = completa(&p, rx(&A, r"\b\d+ milh(?:ão|ões) e \d+"), " mil", &[" mil"]);
    completa(&p, rx(&B, r"\b\d+ mil e \d+"), " reais", &[" mil", " reais"])
}

/// Divide os paragrafos em n blocos seguidos com quantidade parecida de palavras (nunca parte um paragrafo no meio).
pub fn grupos(pars: &[String], n: usize) -> Vec<Vec<String>> {
    let palavras = |p: &String| p.split_whitespace().count();
    let tot: usize = pars.iter().map(palavras).sum();
    let (mut out, mut acc): (Vec<Vec<String>>, usize) = (vec![Vec::new()], 0);
    for p in pars {
        if out.last().is_some_and(|u| !u.is_empty()) && out.len() < n && (acc * n) as f64 >= (tot * out.len()) as f64 {
            out.push(Vec::new());
        }
        if let Some(u) = out.last_mut() {
            u.push(p.clone());
        }
        acc += palavras(p);
    }
    out
}

/// Paragrafos como vao para a voz: o que cita "crime" vai junto do anterior (sozinho a voz recusa) e lista longa (mais de 6 frases)
/// vai de 4 em 4 frases (num pedido so a voz troca numero no meio). Como no Python, a juncao e em cascata: o paragrafo junto tambem
/// cita "crime", entao tudo que vem ANTES dele acaba num paragrafo so (que depois e cortado de 4 em 4 frases).
pub fn curtos(roteiro_falado: &str) -> Vec<String> {
    let mut pars: Vec<String> = roteiro_falado.replace("\r\n", "\n").split("\n\n").map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect();
    for i in (1..pars.len()).rev() {
        if pars[i].contains("crime") {
            let junto = format!("{}\n\n{}", pars[i - 1], pars[i]);
            pars.splice(i - 1..=i, [junto]);
        }
    }
    let mut out = Vec::new();
    for p in pars {
        // frases: corta no espaco que vem depois de ponto
        let (mut fr, mut atual, mut ponto) = (Vec::new(), String::new(), false);
        for c in p.chars() {
            if c.is_whitespace() && ponto {
                if !atual.is_empty() {
                    fr.push(std::mem::take(&mut atual));
                }
                continue; // espacos seguidos depois do ponto somem (ponto continua valendo)
            }
            ponto = c == '.';
            atual.push(c);
        }
        if !atual.is_empty() {
            fr.push(atual);
        }
        if fr.len() > 6 {
            out.extend(fr.chunks(4).map(|c| c.join(" ")));
        } else {
            out.push(p);
        }
    }
    out
}

/// Tudo num pedido so; cortou antes do fim -> 2 blocos, depois 3... `tts(texto, k)` grava o bloco k (1, 2...); `falou(k, texto)` diz
/// se a voz chegou ao fim; `limpar()` apaga os blocos da tentativa anterior. Um paragrafo por pedido tenta 3 vezes o mesmo.
pub fn gravar_blocos(pars: &[String], inicio: usize, trocas: &[(&str, &str)], limpar: &dyn Fn(), tts: &dyn Fn(&str, usize) -> Result<(), String>, falou: &dyn Fn(usize, &str) -> bool) -> Result<Vec<Vec<String>>, String> {
    for n in inicio.clamp(1, pars.len().max(1))..=pars.len() {
        limpar();
        let bl = grupos(pars, n);
        let mut inteiro = true;
        for (k, b) in bl.iter().enumerate() {
            let texto = b.iter().map(|p| texto_para_voz(p, trocas)).collect::<Vec<_>>().join("\n\n");
            let mut ok = false;
            for _ in 0..if n == pars.len() { 3 } else { 1 } {
                tts(&texto, k + 1)?;
                ok = falou(k + 1, &texto);
                if ok {
                    break;
                }
            }
            if !ok {
                inteiro = false;
                break;
            }
        }
        if inteiro {
            return Ok(bl);
        }
    }
    Err("voz: cortou mesmo com um paragrafo por pedido".into())
}

/// Rapida demais = a voz cortou (crua, ela fala de 1,1 a 2,5 palavras por segundo).
pub fn rapida_demais(texto: &str, dur_s: f64) -> bool {
    dur_s <= 0.0 || texto.split_whitespace().count() as f64 / dur_s > 2.8
}

/// As ultimas 4 palavras (so letras) do bloco e quantas a escuta do fim precisa achar para valer "falou tudo".
pub fn fim_esperado(texto: &str) -> (Vec<String>, usize) {
    let p: Vec<String> = texto.split_whitespace().map(norm).filter(|w| !w.is_empty() && w.chars().all(|c| c.is_ascii_alphabetic())).collect();
    let fim = p[p.len().saturating_sub(4)..].to_vec();
    let n = fim.len().min(2);
    (fim, n)
}

/// Chave do pedido `tent` (0, 1...): reveza as chaves so quando ha varias; o MODELO nunca muda. (indice, total de tentativas)
pub fn rodizio(tent: usize, chaves: usize) -> (usize, usize) {
    let c = chaves.max(1);
    (tent % c, 6 + 2 * c)
}

/// Fator do atempo para chegar na cadencia pedida (palavras por segundo), entre 1,0 e 1,35.
pub fn fator_atempo(dur_s: f64, roteiro_falado: &str, wps: f64) -> f64 {
    (dur_s / (roteiro_falado.split_whitespace().count() as f64 / wps)).clamp(1.0, 1.35)
}

/// Silencio (janela de 10 ms com RMS < limiar) maior que `maximo` s vira `maximo` s, metade de cada ponta. PCM de 16 bits.
pub fn encurtar_pausas(pcm: &[i16], taxa: u32, canais: u16, maximo: f64, limiar: f64) -> Vec<i16> {
    let q = ((taxa / 100).max(1) * canais.max(1) as u32) as usize;
    let jan: Vec<&[i16]> = pcm.chunks(q).collect();
    let mudo: Vec<bool> = jan.iter().map(|j| (j.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / j.len() as f64).sqrt() < limiar).collect();
    let (cab, mut fica, mut i) = ((maximo * 100.0).round() as usize, Vec::new(), 0);
    while i < jan.len() {
        let k = (i..jan.len()).find(|k| !mudo[*k]).unwrap_or(jan.len());
        if k == i {
            fica.push(i);
            i += 1;
            continue;
        }
        if k - i <= cab {
            fica.extend(i..k);
        } else {
            fica.extend(i..i + cab / 2);
            fica.extend(k - (cab - cab / 2)..k);
        }
        i = k;
    }
    fica.into_iter().flat_map(|j| jan[j].iter().copied()).collect()
}

/// Lista do concat do ffmpeg: os blocos com o silencio entre eles (nao depois do ultimo).
pub fn lista_concat(blocos: &[String], silencio: &str) -> String {
    blocos.iter().enumerate().map(|(i, b)| format!("file '{b}'\n{}", if i + 1 < blocos.len() { format!("file '{silencio}'\n") } else { String::new() })).collect::<String>().replace('\\', "/")
}

// ---------- comandos (quem chama executa) ----------

pub fn args_normalizar(audio: &str, saida: &str) -> Vec<String> {
    ["ffmpeg", "-v", "error", "-y", "-i", audio, "-af", "loudnorm=I=-15:TP=-1.5:LRA=11", "-ar", "48000", saida].iter().map(|s| s.to_string()).collect()
}

/// Transcricao local com o modelo grande (o turbo pulou trechos de numeros).
pub fn args_transcrever(projeto: &str) -> Vec<String> {
    vec!["tools/transcrever.py".into(), format!("projetos/{projeto}"), "--motor".into(), "local".into(), "--modelo".into(), "large-v3".into()]
}

pub fn args_alinhar(projeto: &str, codigo: &str) -> Vec<String> {
    vec!["tools/alinhar_roteiro.py".into(), format!("projetos/{projeto}"), codigo.into()]
}

/// A voz so segue para a sincronia sem problema (ou com `aceitar_voz` no preparo).
pub fn voz_aprovada(cfg: &Value, erros: &[String]) -> Result<(), String> {
    if erros.is_empty() || cfg["aceitar_voz"] == true { Ok(()) } else { Err(format!("narração com problema (refaça a parte da voz ou a transcrição): {}", erros.join("; "))) }
}

/// Troca o AUDIO_SEG do .tsx pela duracao real (so a primeira ocorrencia).
pub fn com_audio_seg(tsx: &str, dur: f64) -> String {
    static R: OnceLock<Regex> = OnceLock::new();
    rx(&R, r"const AUDIO_SEG = [^;]+;").replace(tsx, regex::NoExpand(&format!("const AUDIO_SEG = {dur:.2};"))).into_owned()
}

/// A ficha videos/NNN/video.json da producao completa.
pub fn ficha(numero: &str, cfg: &Value, pal: &[Pal], dur: f64, sintetico: bool) -> Result<Value, String> {
    let capa = quando(pal, cfg["capa_palavra"].as_str().unwrap_or(""), 1)?;
    Ok(json!({ "_explica": format!("Producao completa do Braniac (preparar_magno).{}", if sintetico { " SINTETICO: nao renderizar." } else { "" }),
        "numero": numero, "nome": cfg["nome"], "projeto": cfg["projeto"], "composicao_estudio": cfg["comp"], "composicao_kit": cfg["comp"], "arquivo_mapa": cfg["arquivo_mapa"],
        "folhas": format!("videos/{numero}/folhas.json"), "quadro_capa": ((capa + 1.2) * 30.0).round() as i64, "duracao_esperada_s": r2(dur + 0.9) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pal(texto: &str) -> Vec<Pal> {
        texto.split_whitespace().enumerate().map(|(i, w)| Pal { w: w.into(), s: r2(i as f64 * 0.5), e: r2(i as f64 * 0.5 + 0.4) }).collect()
    }

    const FALA: &str = "Cidade arrecadou 10 milhões em 2024, 11 em 2025 e 9 até setembro de 2026. Eu abri as contas da Câmara. 2024. Arrecadou 10. Gastou 12. Gastou 2 a mais do que arrecadou. Educação, 5. Saúde, 3. Câmara, 1. 2025. Arrecadou 11. Gastou 10. Saúde, 4. Educação, 6. 2026, até setembro. Arrecadou 9. Educação, 4. Câmara, 1. A ação de festas gastou 2.";

    fn dados() -> (Value, Value) {
        let s = |v: f64| json!({"liquidado": v, "acoes": [["por: AÇÃO A", v]]});
        (
            json!({"falas_setor": {"Educação": "Educação", "Saúde": "Saúde", "Câmara": "Câmara"}}),
            json!({"2024-12": {"Educação": s(5.0), "Saúde": s(3.0), "Câmara": s(1.0)}, "2025-12": {"Educação": s(6.0), "Saúde": s(4.0)}, "2026-09": {"Educação": s(4.0), "Câmara": s(1.0), "Parado": s(0.0)}}),
        )
    }

    fn rec(_: &str) -> Option<Value> {
        Some(json!([["Transferências", 1.0, "x"], ["Zerada", 0.0]]))
    }

    #[test]
    fn opcodes_iguais_aos_do_python() {
        let a: Vec<&str> = "e por que a acao de operacoes especiais cumprimento de sentencas judiciais".split(' ').collect();
        let b: Vec<&str> = "por que a acao X de operacoes cumprimento novo".split(' ').collect();
        assert_eq!(opcodes(&a, &b), vec![(Op::Apaga, 0, 1, 0, 0), (Op::Igual, 1, 5, 0, 4), (Op::Insere, 5, 5, 4, 5), (Op::Igual, 5, 7, 5, 7), (Op::Apaga, 7, 8, 7, 7), (Op::Igual, 8, 9, 7, 8), (Op::Troca, 9, 12, 8, 9)]);
        assert_eq!(opcodes::<u8>(&[], &[]), vec![]);
        assert_eq!(opcodes(&[1, 2], &[1, 2]), vec![(Op::Igual, 0, 2, 0, 2)]);
        assert_eq!(opcodes(&[1], &[2]), vec![(Op::Troca, 0, 1, 0, 1)]);
    }

    #[test]
    fn folhas_presas_ao_ano_e_uma_por_vez() {
        let (cfg, setores) = dados();
        let p = pal(FALA);
        let fol = folhas(&cfg, &setores, &rec, &p).unwrap();
        let nomes: Vec<String> = fol.iter().map(|f| if f["tipo"] == "despesa" { f["titulo"].as_str().unwrap().split(" · ").nth(2).unwrap().to_string() } else { "receita".into() }).collect();
        assert_eq!(nomes, ["receita", "Educação", "Saúde", "Câmara", "receita", "Saúde", "Educação", "receita", "Educação", "Câmara"]);
        let ano25 = depois(&p, "2025. Arrecadou", 0.0, f64::MAX).unwrap();
        let t = |i: usize, k: &str| fol[i][k].as_f64().unwrap();
        assert_eq!(t(4, "de"), ano25 + 0.5); // a receita de 2025 entra no "Arrecadou" de 2025, nunca no "do que arrecadou" de 2024
        assert!((0..fol.len() - 1).all(|i| t(i, "ate") <= t(i + 1, "de"))); // uma folha por vez
        assert!(t(1, "ate") == t(2, "de") && t(0, "ate") == t(1, "de"));
        assert_eq!(t(3, "ate"), ano25); // a ultima do ano sai quando o ano seguinte comeca
        assert_eq!(t(fol.len() - 1, "ate"), depois(&p, "A ação de", 0.0, f64::MAX).unwrap());
        assert_eq!(fol[1]["titulo"], "PCMGE065 · Despesa por ação · Educação · dez/2024 · TCM-BA");
        assert!(fol[8]["titulo"].as_str().unwrap().starts_with("PCMGE015 · Despesa por ação · Educação · set/2026"));
        assert_eq!(fol[3]["titulo"], "PCMGE015 da Câmara · Despesa por ação · Câmara · dez/2024 · TCM-BA");
        assert_eq!(fol[0]["linhas"], json!([["Transferências", 1.0]])); // linha zerada nao entra
        assert_eq!((fol[0]["total"].as_f64(), fol[1]["total"].as_f64(), fol[1]["linhas"][0][0].as_str()), (Some(1.0), Some(5.0), Some("Ação a")));
        // abertura de capital cita ano + "arrecadou": os blocos nao embaralham
        let fol = folhas(&cfg, &setores, &rec, &pal(&format!("Em 2025 Cidade arrecadou 11 milhões. {FALA}"))).unwrap();
        assert_eq!(fol.iter().filter(|f| f["tipo"] == "receita").count(), 3);
        assert!((0..fol.len() - 1).all(|i| fol[i]["ate"].as_f64() <= fol[i + 1]["de"].as_f64()));
        // setor sem a palavra na fala e outro tribunal
        let mut cfg2 = cfg.clone();
        cfg2["falas_setor"]["Saúde"] = json!("Hospitais");
        assert!(folhas(&cfg2, &setores, &rec, &p).unwrap_err().contains("setor sem a palavra na fala: Saúde"));
        let mut cfg3 = cfg.clone();
        cfg3["tribunal"] = json!("TCE-MT");
        assert!(folhas(&cfg3, &setores, &rec, &p).unwrap()[3]["titulo"].as_str().unwrap().starts_with("PCMGE065 · Despesa por ação · Câmara"));
        assert!(folhas(&cfg, &setores, &rec, &pal("Eu abri. 2025. Arrecadou 1.")).unwrap_err().contains("bloco do ano 2024"));
        assert!(folhas(&cfg, &setores, &|_| None, &p).is_err());
    }

    #[test]
    fn quando_tolera_a_grafia_da_transcricao() {
        let p: Vec<Pal> = "e por que a ação de operações especiais cumprimento de sentenças judiciais".split(' ').enumerate().map(|(i, w)| Pal { w: w.into(), s: i as f64, e: 0.0 }).collect();
        assert_eq!(quando(&p, "por que a ação de operações especiais - cumprimento de setenças judiciais", 1), Ok(1.0));
        assert!(quando(&p, "nada parecido com isso aqui mesmo", 1).is_err());
        assert_eq!(quando(&p, "de", 2), Ok(9.0));
        assert!(quando(&p, "de", 3).is_err() && quando(&p, "", 1).is_err());
    }

    #[test]
    fn conferencia_dos_numeros_tolera_escala_e_corte_mas_nao_troca() {
        let rot = "Educação, 45 milhões e 190. Obras, 5 milhões e 320. Dívidas, 3 milhões e 830.";
        assert_eq!(conferir_numeros(rot, "Educação R $ 45 ,19 mil. Obras 5,3 milhões. Dívidas 3 milhões e 830 mil."), Vec::<String>::new());
        assert!(!conferir_numeros(rot, "Educação 45 milhões e 190 mil. Obras 5,3 milhões. Dívidas 3,33 milhões.").is_empty()); // 3,83 virou 3,33
        assert!(!conferir_numeros(rot, "Educação 45 milhões e 190 mil. Dívidas 3 milhões e 830 mil.").is_empty()); // pulou um valor
        let p = {
            let mut p = pal("Cidade arrecadou 602 milhões e 160 mil.");
            p.last_mut().unwrap().s += 10.0;
            p
        };
        let erros = conferir_transcricao(&p, "Cidade arrecadou 62 milhões e 160 mil.", 3.0);
        assert!(erros.iter().any(|e| e.contains("buraco")) && erros.iter().any(|e| e.contains("602")));
        assert!(conferir_transcricao(&pal("Cidade arrecadou 62 milhões e 160 mil."), "Cidade arrecadou 62 milhões e 160 mil.", 3.0).is_empty());
        assert!(voz_aprovada(&json!({}), &erros).is_err() && voz_aprovada(&json!({"aceitar_voz": true}), &erros).is_ok() && voz_aprovada(&json!({}), &[]).is_ok());
    }

    #[test]
    fn texto_para_voz_escreve_o_mil() {
        let p = "Obras, 3 milhões e 500. Câmara, 1 milhão e 980. Arrecadou 62 milhões e 160 mil em 2024. Cultura, 71 mil. Câmara, 2 milhões. @canal";
        assert_eq!(texto_para_voz(p, &[("@canal", "arroba canal")]), "Obras, 3 milhões e 500 mil. Câmara, 1 milhão e 980 mil. Arrecadou 62 milhões e 160 mil em 2024. Cultura, 71 mil. Câmara, 2 milhões. arroba canal");
        assert_eq!(texto_para_voz("Festas, 48 mil e 300. Outro, 5 mil e 20 reais. E 7 mil e 100 mil.", &[]), "Festas, 48 mil e 300 reais. Outro, 5 mil e 20 reais. E 7 mil e 100 mil.");
    }

    #[test]
    fn voz_divide_so_quando_corta() {
        let pars: Vec<String> = ["um dois tres", "quatro cinco seis", "sete oito nove", "dez onze doze"].iter().map(|s| s.to_string()).collect();
        assert_eq!(grupos(&pars, 1), vec![pars.clone()]);
        assert_eq!(grupos(&pars, 2), vec![pars[..2].to_vec(), pars[2..].to_vec()]);
        assert_eq!(grupos(&pars, 3).iter().map(Vec::len).collect::<Vec<_>>(), [2, 1, 1]);
        let pedidos = std::cell::RefCell::new(Vec::new());
        let tts = |t: &str, k: usize| -> Result<(), String> {
            pedidos.borrow_mut().push((k, t.to_string()));
            Ok(())
        };
        assert_eq!(gravar_blocos(&pars, 1, &[], &|| (), &tts, &|_, _| true).unwrap(), vec![pars.clone()]); // tudo coube: um pedido so
        assert!(pedidos.borrow().len() == 1 && pedidos.borrow()[0].1.contains("dez onze doze"));
        pedidos.borrow_mut().clear();
        let ate = |n: usize| move |_: usize, t: &str| t.split_whitespace().count() <= n;
        assert_eq!(gravar_blocos(&pars, 1, &[], &|| (), &tts, &ate(6)).unwrap(), vec![pars[..2].to_vec(), pars[2..].to_vec()]); // cortou: divide em 2 e para ali
        assert_eq!(pedidos.borrow().iter().map(|p| p.0).collect::<Vec<_>>(), [1, 1, 2]);
        assert_eq!(gravar_blocos(&pars, 1, &[], &|| (), &tts, &ate(3)).unwrap().len(), 4);
        pedidos.borrow_mut().clear();
        assert!(gravar_blocos(&pars, 4, &[], &|| (), &tts, &|_, _| false).unwrap_err().contains("um paragrafo por pedido"));
        assert_eq!(pedidos.borrow().len(), 3); // um paragrafo por pedido: tenta 3 vezes o mesmo e para
        assert!(gravar_blocos(&pars, 1, &[], &|| (), &|_, _| Err("cota".into()), &|_, _| true).is_err());
        // paragrafos para a voz
        let c = curtos("Abertura.\n\nA. B. C. D. E. F. G.\n\nFecho.\n\nGastar não é crime. Tchau.");
        assert_eq!(c, ["Abertura. A. B. C.", "D. E. F. G.", "Fecho. Gastar não é crime. Tchau."]); // igual ao Python: a juncao do "crime" puxa tudo que vem antes
        assert_eq!(curtos("Um. Dois.\n\nA. B. C. D. E. F. G."), ["Um. Dois.", "A. B. C. D.", "E. F. G."]);
        assert_eq!(fim_esperado("Gastou 12 milhões, e 3 foram com festas."), (vec!["e".into(), "foram".into(), "com".into(), "festas".into()], 2));
        assert!(rapida_demais("a b c d e f", 2.0) && !rapida_demais("a b c d", 2.0) && rapida_demais("a", 0.0));
        assert_eq!((rodizio(0, 1), rodizio(4, 3)), ((0, 8), (1, 12)));
        assert_eq!((fator_atempo(10.0, "a b c d e f g h i", 2.25), fator_atempo(1.0, "a b c d e f g h i", 2.25), fator_atempo(4.4, "a b c d e f g h i", 2.25)), (1.35, 1.0, 1.1));
    }

    #[test]
    fn pausa_longa_encurta_e_curta_fica() {
        let tom = |s: f64| (0..(24000.0 * s) as usize).map(|i| if i % 20 < 10 { 8000i16 } else { -8000 }).collect::<Vec<_>>();
        let pcm = [tom(1.0), vec![0; 24000], tom(0.5), vec![0; 2400], tom(0.5)].concat(); // 1 s de pausa e 0,1 s de pausa
        let curto = encurtar_pausas(&pcm, 24000, 1, 0.28, 300.0);
        assert!((curto.len() as f64 / 24000.0 - (2.0 + 0.28 + 0.1)).abs() < 0.011);
        assert_eq!(encurtar_pausas(&[], 24000, 1, 0.28, 300.0), Vec::<i16>::new());
        assert_eq!(lista_concat(&["C:\\x\\b01.wav".into(), "b02.wav".into()], "sil.wav"), "file 'C:/x/b01.wav'\nfile 'sil.wav'\nfile 'b02.wav'\n");
    }

    #[test]
    fn sintetico_kit_e_ficha() {
        let s = sintetico("Eu abri as contas.\n\nFim.");
        assert_eq!((s.len(), s[0].s, s[0].e), (5, 0.0, 0.32));
        assert!(s[4].s - s[3].e > 0.45); // pausa entre paragrafos
        assert_eq!(palavras_do_ts(&palavras_ts(&s)).unwrap(), s);
        assert!(palavras_do_ts("export const X = 3;").is_err());
        let fala = "Cidade arrecadou 10 em 2024 11 em 2025 e 9 em 2026. Eu abri o Tribunal. 2024. Arrecadou 10. Gastou 12.";
        let p = pal(fala);
        let ts = "export const INI_B = s(1.00);\nexport const DURACAO = s(9.9);\nexport const A = { inicio: s(0) };\nexport const B = {};\n";
        let novo = kit(ts, &json!({"r24": "10", "r25": "11", "r26": "9", "n_b": "10."}), &p).unwrap();
        let ano = quando(&p, "2024", 2).unwrap();
        assert!(novo.contains(&format!("export const INI_B = s({:.2});", ano - 0.12)) && novo.contains(&format!("ano: s({ano:.2})")) && novo.contains("export const A = {inicio: s(0.20), r24: s(1.00)"));
        assert!(novo.contains(&format!("export const DURACAO = s({:.2});", quando(&p, "Gastou", 1).unwrap() + 0.8)));
        assert!(kit(ts, &json!({"r24": "nao existe"}), &p).is_err());
        assert_eq!(com_audio_seg("const AUDIO_SEG = 10;\nconst AUDIO_SEG = 10;", 83.456), "const AUDIO_SEG = 83.46;\nconst AUDIO_SEG = 10;");
        let f = ficha("143", &json!({"nome": "CIDADE", "projeto": "143. cidade", "comp": "Orc", "arquivo_mapa": "m.mp4", "capa_palavra": "Tribunal."}), &p, 60.0, true).unwrap();
        assert_eq!((f["quadro_capa"].as_i64(), f["duracao_esperada_s"].as_f64(), f["folhas"].as_str()), (Some(((quando(&p, "Tribunal", 1).unwrap() + 1.2) * 30.0).round() as i64), Some(60.9), Some("videos/143/folhas.json")));
        assert!(f["_explica"].as_str().unwrap().contains("SINTETICO"));
        assert_eq!(args_transcrever("143. x")[1], "projetos/143. x");
    }
}
