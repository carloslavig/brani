//! Conferencia periodica da esteira das capitais (link/conferir_capitais.py). Sem IA, so conta e texto. Para cada capital ja levantada:
//!   1. FONTE: o molde do estado rele o TRIBUNAL numa pasta limpa e cada numero do cidade.json e dos setores tem de bater no
//!      centavo (so compara se o mes de corte de 2026 e o mesmo; o tribunal publicar mes novo nao e divergencia);
//!   2. GUIA: o tribunal citado e o do guia do estado (capital com tribunal proprio = o do municipio);
//!   3. ROTEIRO: todo numero falado e um numero levantado (nada inventado) e nenhuma palavra proibida;
//!   4. TELA e LEGENDA: nada de outra fonte (e-TCM, PCMGE, Tesouro, RREO, SICONFI, Radar fora de MT) e o tribunal certo na legenda;
//!   5. VOZ: se ja tem transcricao, cada numero dito pela voz confere com o roteiro.
//! Fonte Tesouro/RREO e recusada de cara: numero de video so vem do tribunal de contas.
//! Divergencia NAO e sinonimo de video errado: aqui so se diz "no video X, na fonte agora Y". O que esta errado (video,
//! levantamento do PC ou o tribunal que mudou) sai das tres leituras de `link_auditar::triagem`. Fonte fora do ar ou leitura
//! que nao fecha (Err do molde) fica em `erro_fonte` e NAO marca o video como errado: tenta na proxima rodada.
//! Resultado em dados/magno/conferencia.json; problema NOVO vira aviso ao dono (closure; quem chama decide o canal).
//! Entram como closure (`Fora`): reler o tribunal, a sigla do guia do estado, a conferencia da voz e o aviso.
//! Ficou de fora: o bloco do SICONFI (no Python ja esta desligado) e a linha de comando. Diferenca: a lista de numeros/linhas
//! dentro da mensagem sai no formato do Rust (aspas duplas).
#![allow(dead_code)]
use crate::link_auditar::{br, dito, valores};
use crate::link_lote_magno;
use crate::texto_pt::sem_acento;
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub const INTERVALO: f64 = 3.0 * 3600.0;
pub const PROIBIDAS: [&str; 6] = ["suspeit", "irregular", "desvio", "corrup", "%", "segurança"];
pub const SEM_LEVANTAMENTO: &str = "ainda sem levantamento";
const ANOS: [&str; 3] = ["2024", "2025", "2026"];

pub struct Fora<'a> {
    /// (numero, cidade.json atual) -> (cidade.json, setores.json) relidos AGORA do tribunal, numa pasta limpa.
    pub reler: &'a dyn Fn(&str, &Value) -> Result<(Value, Value), String>,
    /// A sigla do tribunal que o guia do estado manda para a capital.
    pub sigla_do_guia: &'a dyn Fn(&str) -> Result<String, String>,
    /// Problemas dos numeros da voz contra o roteiro (pasta do projeto do video).
    pub voz: &'a dyn Fn(&Path) -> Vec<String>,
    pub avisar: &'a dyn Fn(&str),
}

fn json_de(arq: &Path) -> Result<Value, String> {
    let t = std::fs::read_to_string(arq).map_err(|e| format!("{}: {e}", arq.display()))?;
    serde_json::from_str(&t).map_err(|e| format!("{}: {e}", arq.display()))
}

fn txt<'a>(v: &'a Value, k: &str, onde: &str) -> Result<&'a str, String> {
    v[k].as_str().ok_or_else(|| format!("{onde} sem {k}"))
}

fn num(v: &Value, onde: &str) -> Result<f64, String> {
    v.as_f64().ok_or_else(|| format!("levantamento sem o número de {onde}"))
}

/// 1234567.891 -> "1,234,567.89" (como o Python escrevia a mensagem).
fn us(v: f64) -> String {
    br(v).chars().map(|c| match c {
        '.' => ',',
        ',' => '.',
        c => c,
    }).collect()
}

/// A pasta do levantamento da capital e o registro dela (cidade, uf).
pub fn pasta_de(dados: &Path, pacote: &Path, numero: &str) -> Option<(PathBuf, Value)> {
    let cap = link_lote_magno::capital(dados, numero)?;
    let cidade: String = sem_acento(cap["cidade"].as_str()?).chars().filter(char::is_ascii).collect();
    Some((pacote.join(format!("RECEITAS-DESPESAS-{}-{}", cidade.to_uppercase().replace(' ', "-"), cap["uf"].as_str()?)), cap))
}

fn comparar(a: &Value, b: &Value, onde: &str, prob: &mut Vec<String>) -> Result<(), String> {
    let (a, b) = (num(a, onde)?, num(b, &format!("{onde} (na fonte)"))?);
    if (a - b).abs() > 0.01 {
        prob.push(format!("{onde}: no vídeo {}, na fonte agora {}", us(a), us(b)));
    }
    Ok(())
}

fn objeto<'a>(v: &'a Value, onde: &str) -> Result<&'a Map<String, Value>, String> {
    v.as_object().ok_or_else(|| format!("levantamento sem {onde}"))
}

/// A lista de problemas de uma capital (vazia = tudo certo). Err = nao deu para conferir (fonte fora do ar, levantamento pela
/// metade): NAO e problema do video.
pub fn conferir(dados: &Path, pacote: &Path, numero: &str, baixar: bool, f: &Fora) -> Result<Vec<String>, String> {
    let Some((pasta, _)) = pasta_de(dados, pacote, numero) else { return Ok(vec![SEM_LEVANTAMENTO.into()]) };
    let saida = pasta.join("saida");
    if !saida.join("cidade.json").exists() {
        return Ok(vec![SEM_LEVANTAMENTO.into()]);
    }
    let (c, set) = (json_de(&saida.join("cidade.json"))?, json_de(&saida.join("setores.json"))?);
    let mut prob = vec![];
    // 1. fonte de novo: o molde do estado rele o TRIBUNAL e as contas tem de bater no centavo
    if c["fonte"] == "siconfi" {
        return Ok(vec!["fonte errada: Tesouro/RREO (só vale o tribunal de contas)".into()]);
    }
    let uf = txt(&c, "uf", "cidade.json")?;
    if baixar && c["fonte"] == "tribunal" {
        let (n, s2) = (f.reler)(numero, &c)?;
        // o tribunal publicou mes novo: compara so o que e do mesmo periodo
        if !c["meses"]["2026"].is_null() && n["meses"]["2026"] == c["meses"]["2026"] {
            for a in ANOS {
                for k in ["arrec", "gastou", "emp"] {
                    comparar(&c["anos"][a][k], &n["anos"][a][k], &format!("{a} {k}"), &mut prob)?;
                }
                for k in ["prev", "gasto"] {
                    comparar(&c["festas"][a][k], &n["festas"][a][k], &format!("{a} festas {k}"), &mut prob)?;
                }
            }
            for (ano, tab) in objeto(&set, "setores")? {
                for (s, d) in objeto(tab, "setores")? {
                    comparar(&d["liquidado"], &s2[ano][s]["liquidado"], &format!("{ano} {s}"), &mut prob)?;
                }
            }
        }
    }
    // 2. guia: o tribunal citado e o do guia
    let (certo, citado) = ((f.sigla_do_guia)(uf)?, c["tribunal"]["sigla"].as_str());
    if citado != Some(certo.as_str()) {
        prob.push(format!("tribunal fora do guia: {} (o guia de {uf} manda o {certo})", citado.unwrap_or("nenhum")));
    }
    // 3. roteiro: so numeros levantados e nenhuma palavra proibida
    let cfg_arq = pacote.join("videos").join(numero).join("preparo.json");
    if !cfg_arq.exists() {
        return Ok(prob);
    }
    let cfg = json_de(&cfg_arq)?;
    let (projeto, nome) = (txt(&cfg, "projeto", "preparo.json")?, txt(&cfg, "nome", "preparo.json")?);
    let estudio = pacote.join("estudio-3d");
    let pj = estudio.join("projetos").join(projeto);
    let rot = std::fs::read_to_string(pj.join("audio").join("roteiro-falado.txt")).unwrap_or_default();
    if !rot.is_empty() {
        let mut vals = vec![];
        for (a, ano) in objeto(&c["anos"], "anos")? {
            for v in objeto(ano, "anos")?.values() {
                vals.push(num(v, &format!("anos {a}"))?);
            }
            vals.push((num(&ano["arrec"], "arrec")? - num(&ano["gastou"], "gastou")?).abs());
        }
        for fe in objeto(&c["festas"], "festas")?.values() {
            vals.extend([num(&fe["prev"], "festas prev")?, num(&fe["gasto"], "festas gasto")?]);
        }
        for b in objeto(&c["benef"], "benef")?.values() {
            vals.push(num(b, "benef")?);
        }
        for tab in objeto(&set, "setores")?.values() {
            for d in objeto(tab, "setores")?.values() {
                vals.push(num(&d["liquidado"], "setor")?);
            }
        }
        let mut ok: BTreeSet<i128> = vals.iter().filter(|v| **v != 0.0).flat_map(|v| valores(&dito(*v))).chain([2024, 2025, 2026]).collect();
        let meses = objeto(&c["meses"], "meses")?.values().filter_map(Value::as_str).collect::<Vec<_>>().join(" ");
        ok.extend(Regex::new(r"\d+").expect("regex fixa").find_iter(&meses).filter_map(|m| m.as_str().parse::<i128>().ok()));
        let soltos: BTreeSet<i128> = valores(&rot.replace("@exemplo", "")).into_iter().filter(|v| !ok.contains(v)).collect();
        if !soltos.is_empty() {
            prob.push(format!("roteiro com número que não veio da fonte: {:?}", soltos.iter().take(5).collect::<Vec<_>>()));
        }
        let baixo = rot.to_lowercase();
        prob.extend(PROIBIDAS.iter().filter(|p| baixo.contains(*p)).map(|p| format!("roteiro com palavra proibida: {p}")));
        if baixo.matches("crime").count() != 1 {
            prob.push("roteiro: 'crime' só pode aparecer uma vez (em 'Gastar não é crime')".into());
        }
    }
    // 4. tela e legenda
    let outra_fonte = Regex::new(r"TCM-BA|PCMGE|PCAGO|E-TCM|e-TCM|Bahia|Tesouro|TESOURO|RREO|SICONFI").expect("regex fixa");
    let tsx = estudio.join("src").join("videos").join(txt(&cfg, "codigo", "preparo.json")?).join(format!("{}.tsx", txt(&cfg, "comp", "preparo.json")?));
    if let Ok(t) = std::fs::read_to_string(&tsx) {
        let ruins: Vec<String> = t.lines().filter(|l| !l.trim_start().starts_with("//")).filter(|l| outra_fonte.is_match(l) || (uf != "MT" && l.to_uppercase().contains("TCE-MT"))).map(|l| l.trim().chars().take(80).collect()).collect();
        if !ruins.is_empty() {
            prob.push(format!("tela com texto de outra fonte: {:?}", &ruins[..ruins.len().min(2)]));
        }
    }
    if let Ok(t) = std::fs::read_to_string(estudio.join("edicoes").join(projeto).join(format!("{nome} - legenda.txt"))) {
        if outra_fonte.is_match(&t) || (uf != "MT" && (t.contains("TCE-MT") || t.contains("tce.mt.gov.br"))) {
            prob.push("legenda com texto de outra fonte".into());
        }
        let sig = c["tribunal"]["tribunal_estado"].as_str().unwrap_or("").split(' ').next().unwrap_or("");
        if !sig.is_empty() && !t.contains(sig) {
            prob.push(format!("legenda sem o tribunal do guia ({sig})"));
        }
    }
    // 5. voz
    if pj.join("transcricao").join("transcript.md").exists() {
        prob.extend((f.voz)(&pj).into_iter().map(|x| format!("voz: {x}")));
    }
    Ok(prob)
}

fn ler_mapa(arq: &Path) -> Map<String, Value> {
    std::fs::read_to_string(arq).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// Confere as capitais pedidas (ou todas as da esteira) e grava dados/magno/conferencia.json. So problema NOVO vira aviso.
pub fn rodar(dados: &Path, pacote: &Path, numeros: Option<&[String]>, agora_s: f64, avisar: bool, f: &Fora) -> Result<Map<String, Value>, String> {
    let magno = dados.join("magno");
    let (reg, arq) = (ler_mapa(&magno.join("capitais.json")), magno.join("conferencia.json"));
    let mut conf = ler_mapa(&arq);
    let todos: Vec<String> = numeros.map_or_else(|| reg.keys().cloned().collect(), <[String]>::to_vec);
    let mut novos = vec![];
    for n in &todos {
        let p = match conferir(dados, pacote, n, true, f) {
            // fonte fora do ar: tenta na proxima rodada, nao marca como erro do video
            Err(e) => {
                let reg_n = conf.entry(n.clone()).or_insert_with(|| json!({}));
                if !reg_n.is_object() {
                    *reg_n = json!({});
                }
                reg_n["erro_fonte"] = json!(e.chars().take(160).collect::<String>());
                continue;
            }
            Ok(p) if p == [SEM_LEVANTAMENTO] => continue,
            Ok(p) => p,
        };
        let antes: BTreeSet<&str> = conf.get(n).and_then(|x| x["problemas"].as_array()).into_iter().flatten().filter_map(Value::as_str).collect();
        if p.iter().any(|x| !antes.contains(x.as_str())) {
            novos.push(format!("{n} {}: {}", reg.get(n).and_then(|r| r["cidade"].as_str()).unwrap_or(""), p.iter().take(3).cloned().collect::<Vec<_>>().join("; ")));
        }
        conf.insert(n.clone(), json!({ "ok": p.is_empty(), "quando": agora_s, "problemas": p }));
    }
    std::fs::create_dir_all(&magno).map_err(|e| e.to_string())?;
    std::fs::write(&arq, serde_json::to_string_pretty(&conf).map_err(|e| e.to_string())?).map_err(|e| format!("{}: {e}", arq.display()))?;
    if !novos.is_empty() && avisar {
        (f.avisar)(&format!("Conferência das capitais achou problema (o vídeo não é mandado como certo até alguém olhar):\n{}", novos.join("\n").chars().take(1200).collect::<String>()));
    }
    Ok(conf)
}

/// No relogio da rotina: uma rodada a cada 3 h. None = ainda nao e hora.
pub fn tick(dados: &Path, pacote: &Path, agora_s: f64, f: &Fora) -> Option<Result<Map<String, Value>, String>> {
    let marca = dados.join("magno").join("conferencia.ultima");
    if std::fs::read_to_string(&marca).ok().and_then(|t| t.trim().parse::<f64>().ok()).is_some_and(|u| agora_s - u < INTERVALO) {
        return None;
    }
    if let Err(e) = std::fs::create_dir_all(dados.join("magno")).and_then(|_| std::fs::write(&marca, agora_s.to_string())) {
        return Some(Err(e.to_string()));
    }
    Some(rodar(dados, pacote, None, agora_s, true, f))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct Caso {
        raiz: PathBuf,
        dados: PathBuf,
        pacote: PathBuf,
    }

    fn grava(arq: &Path, t: &str) {
        std::fs::create_dir_all(arq.parent().unwrap()).unwrap();
        std::fs::write(arq, t).unwrap();
    }

    fn cidade(fonte: &str) -> Value {
        let por_ano = |v: Value| Value::Object(ANOS.iter().map(|a| (a.to_string(), v.clone())).collect());
        json!({ "nome": "São Teste", "uf": "SP", "fonte": fonte, "anos": por_ano(json!({ "arrec": 2e9, "gastou": 1.5e9, "emp": 1.6e9 })), "festas": por_ano(json!({ "prev": 1e6, "gasto": 3e6 })),
            "benef": por_ano(json!(2e6)), "meses": { "2024": "12", "2025": "12", "2026": "08" }, "tribunal": { "sigla": "TCM-SP", "tribunal_estado": "TCM-SP (Tribunal de Contas do Município)" } })
    }

    fn setores() -> Value {
        json!({ "2024-12": { "Saúde": { "liquidado": 5e8, "acoes": [] } } })
    }

    const CERTO: &str = "São Teste arrecadou 2 bilhões. Gastou 1 bilhão e 500 milhões até agosto, mês 08. Saúde, 500 milhões. Gastar não é crime. @exemplo";

    fn caso(nome: &str, fonte: &str) -> Caso {
        let raiz = std::env::temp_dir().join(format!("braniac-link_conferir_capitais-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&raiz);
        let c = Caso { dados: raiz.join("dados"), pacote: raiz.join("pacote"), raiz };
        grava(&c.dados.join("magno/capitais.json"), &json!({ "1099": { "cidade": "São Teste", "uf": "SP" }, "1100": { "cidade": "Sem Pasta", "uf": "RJ" } }).to_string());
        let saida = c.pacote.join("RECEITAS-DESPESAS-SAO-TESTE-SP/saida");
        grava(&saida.join("cidade.json"), &cidade(fonte).to_string());
        grava(&saida.join("setores.json"), &setores().to_string());
        grava(&c.pacote.join("videos/1099/preparo.json"), &json!({ "projeto": "1099. orçamento de teste", "codigo": "x", "comp": "X", "nome": "TESTE" }).to_string());
        grava(&c.pacote.join("estudio-3d/projetos/1099. orçamento de teste/audio/roteiro-falado.txt"), CERTO);
        c
    }

    fn guia(uf: &str) -> Result<String, String> {
        if uf == "SP" { Ok("TCM-SP".into()) } else { Err(format!("sem guia de {uf}")) }
    }

    fn voz(_: &Path) -> Vec<String> {
        vec!["roteiro [3] x voz [4]".into()]
    }

    fn fora<'a>(reler: &'a dyn Fn(&str, &Value) -> Result<(Value, Value), String>, avisar: &'a dyn Fn(&str)) -> Fora<'a> {
        Fora { reler, sigla_do_guia: &guia, voz: &voz, avisar }
    }

    fn nao_rele(_: &str, _: &Value) -> Result<(Value, Value), String> {
        panic!("não era para reler o tribunal")
    }

    #[test]
    fn conferencia_pega_numero_que_nao_veio_da_fonte() {
        let c = caso("roteiro", "radar");
        let calado = |_: &str| {};
        let f = fora(&nao_rele, &calado);
        let confere = || conferir(&c.dados, &c.pacote, "1099", false, &f).unwrap();
        assert_eq!(confere(), Vec::<String>::new());
        let roteiro = c.pacote.join("estudio-3d/projetos/1099. orçamento de teste/audio/roteiro-falado.txt");
        grava(&roteiro, &format!("{CERTO} A área gastou 777 milhões."));
        assert_eq!(confere(), ["roteiro com número que não veio da fonte: [777000000]"]);
        grava(&roteiro, &CERTO.replace("Gastar não é crime.", "Há suspeita de desvio de 30%."));
        assert_eq!(confere(), ["roteiro com número que não veio da fonte: [30]", "roteiro com palavra proibida: suspeit", "roteiro com palavra proibida: desvio", "roteiro com palavra proibida: %", "roteiro: 'crime' só pode aparecer uma vez (em 'Gastar não é crime')"]);
        grava(&roteiro, CERTO);
        // tela e legenda com outra fonte; legenda sem o tribunal do guia; voz
        grava(&c.pacote.join("estudio-3d/src/videos/x/X.tsx"), "// FONTE: PCMGE065 (comentário não conta)\nconst a = 'FONTE: TCM-SP';\n  const b = 'Tesouro Nacional';\nconst d = 'tce-mt';");
        grava(&c.pacote.join("estudio-3d/edicoes/1099. orçamento de teste/TESTE - legenda.txt"), "Fonte: RREO do município");
        grava(&c.pacote.join("estudio-3d/projetos/1099. orçamento de teste/transcricao/transcript.md"), "x");
        assert_eq!(confere(), ["tela com texto de outra fonte: [\"const b = 'Tesouro Nacional';\", \"const d = 'tce-mt';\"]", "legenda com texto de outra fonte", "legenda sem o tribunal do guia (TCM-SP)", "voz: roteiro [3] x voz [4]"]);
        // tribunal fora do guia; capital sem pasta; Tesouro nunca vale
        grava(&c.pacote.join("RECEITAS-DESPESAS-SAO-TESTE-SP/saida/cidade.json"), &cidade("radar").to_string().replace("\"sigla\":\"TCM-SP\"", "\"sigla\":\"TCE-SP\""));
        assert_eq!(confere()[0], "tribunal fora do guia: TCE-SP (o guia de SP manda o TCM-SP)");
        assert_eq!(conferir(&c.dados, &c.pacote, "1100", true, &f).unwrap(), [SEM_LEVANTAMENTO]);
        assert_eq!(conferir(&c.dados, &c.pacote, "7", true, &f).unwrap(), [SEM_LEVANTAMENTO]);
        grava(&c.pacote.join("RECEITAS-DESPESAS-SAO-TESTE-SP/saida/cidade.json"), &cidade("siconfi").to_string());
        assert_eq!(conferir(&c.dados, &c.pacote, "1099", true, &f).unwrap(), ["fonte errada: Tesouro/RREO (só vale o tribunal de contas)"]);
        let _ = std::fs::remove_dir_all(&c.raiz);
    }

    #[test]
    fn rele_o_tribunal_e_so_avisa_problema_novo() {
        let c = caso("fonte", "tribunal");
        let avisos: RefCell<Vec<String>> = RefCell::new(vec![]);
        let avisar = |t: &str| avisos.borrow_mut().push(t.to_string());
        let igual = |_: &str, c: &Value| Ok((c.clone(), setores()));
        let roda = |f: &Fora, t: f64| rodar(&c.dados, &c.pacote, None, t, true, f).unwrap();
        let conf = roda(&fora(&igual, &avisar), 100.0);
        assert_eq!(conf["1099"], json!({ "ok": true, "quando": 100.0, "problemas": [] }));
        assert!(conf.get("1100").is_none() && avisos.borrow().is_empty()); // sem levantamento nao entra
        // a fonte agora tem outro numero: divergencia anotada (com os dois valores), aviso uma vez so
        let mudou = |_: &str, c: &Value| {
            let (mut n, mut s) = (c.clone(), setores());
            n["anos"]["2025"]["gastou"] = json!(1.5e9 + 10.5);
            s["2024-12"]["Saúde"]["liquidado"] = json!(4e8);
            Ok((n, s))
        };
        let conf = roda(&fora(&mudou, &avisar), 200.0);
        assert_eq!(conf["1099"]["problemas"], json!(["2025 gastou: no vídeo 1,500,000,000.00, na fonte agora 1,500,000,010.50", "2024-12 Saúde: no vídeo 500,000,000.00, na fonte agora 400,000,000.00"]));
        assert!(conf["1099"]["ok"] == false && avisos.borrow().len() == 1 && avisos.borrow()[0].contains("1099 São Teste: 2025 gastou: no vídeo"));
        roda(&fora(&mudou, &avisar), 300.0);
        assert_eq!(avisos.borrow().len(), 1);
        // o tribunal publicou mes novo: nao compara periodo diferente
        let mes_novo = |_: &str, c: &Value| {
            let mut n = c.clone();
            (n["meses"]["2026"], n["anos"]["2026"]["gastou"]) = (json!("09"), json!(9e9));
            Ok((n, setores()))
        };
        assert_eq!(roda(&fora(&mes_novo, &avisar), 400.0)["1099"]["problemas"], json!([]));
        // fonte fora do ar ou leitura que nao fecha: fica em erro_fonte, o resultado anterior nao muda e ninguem e avisado
        let caiu = |_: &str, _: &Value| Err("2025: soma das ações 10.00 != total liquidado 12.00".to_string());
        let conf = roda(&fora(&caiu, &avisar), 500.0);
        assert_eq!(conf["1099"], json!({ "ok": true, "quando": 400.0, "problemas": [], "erro_fonte": "2025: soma das ações 10.00 != total liquidado 12.00" }));
        assert_eq!(avisos.borrow().len(), 1);
        // leitura nova sem um dos numeros nao e "bateu": tambem e erro de fonte
        let sem = |_: &str, c: &Value| {
            let mut n = c.clone();
            n["festas"]["2024"] = json!({});
            Ok((n, setores()))
        };
        assert!(roda(&fora(&sem, &avisar), 600.0)["1099"]["erro_fonte"].as_str().unwrap().contains("2024 festas prev (na fonte)"));
        // relogio: uma rodada a cada 3 h
        let f = fora(&igual, &avisar);
        assert!(tick(&c.dados, &c.pacote, 1000.0, &f).unwrap().is_ok() && tick(&c.dados, &c.pacote, 1000.0 + INTERVALO - 1.0, &f).is_none());
        assert_eq!(tick(&c.dados, &c.pacote, 1000.0 + INTERVALO, &f).unwrap().unwrap()["1099"]["quando"], json!(1000.0 + INTERVALO));
        let gravado: Value = json_de(&c.dados.join("magno/conferencia.json")).unwrap();
        assert!(gravado["1099"]["ok"] == true && gravado["1099"].get("erro_fonte").is_none());
        assert_eq!((us(1234567.891), us(-0.5)), ("1,234,567.89".to_string(), "-0.50".to_string()));
        let _ = std::fs::remove_dir_all(&c.raiz);
    }
}
