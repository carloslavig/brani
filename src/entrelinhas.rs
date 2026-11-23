//! Entrelinhas, fase 1 (braniac/entrelinhas.py): SO OBSERVA, NUNCA AGE. Le o que o dono diz e anota o que o Braniac ENTENDERIA e o
//! que PROPORIA fazer. Nada e enviado, buscado ou contatado: so grava em dados/entrelinhas.jsonl (so no PC; nunca vai a IA online).
//! Camadas: 1) regras (palavras + tom + ironia); 2) modelo LOCAL pequeno, por closure, so para confirmar quando ha gatilho ou ironia.
//! De fora: a chamada ao modelo local (closure `local`). O nome da pessoa a avisar saiu do texto do motivo ("uma pessoa proxima").
#![allow(dead_code)]
use crate::tempo::Data;
use crate::texto_pt::sem_acento;
use regex::Regex;
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;

const FINANCEIRO: &str = r"\b(sem grana|sem dinheiro|sem um real|duro|quebrado|contas?(?: \w+){0,2} (?:apertad\w*|atrasad\w*|pra pagar|para pagar|chegando)|apertad[oa] (?:de|com) (?:grana|dinheiro)|grana (?:curta|apertada|ta (?:curta|ruim))|preciso (?:de|juntar|ganhar) (?:grana|dinheiro|uma renda)|nao (?:tenho|to com) (?:dinheiro|grana)|dinheiro (?:acabou|curto|apertado)|(?:to|estou|ando) (?:falido|liso|na pindaiba)|boleto|fatura|aluguel atrasado|pagar as contas|renda extra|trabalho extra|bico|ganhar (?:mais )?(?:grana|dinheiro))\b";
const EMOCIONAL: &str = r"\b(nao (?:to|estou|tou) (?:muito )?bem|nao (?:to|estou) me sentindo bem|(?:to|estou|tou) (?:meio |muito |bem )?(?:mal|mal mesmo|pra baixo|triste|desanimad[oa]|deprimid[oa]|ansios[oa]|estressad[oa]|sobrecarregad[oa])|(?:to|estou|tou) (?:meio |muito |bem )?(?:sem animo|sem forcas)|nao aguento mais|cabeca (?:cheia|a mil)|me sentindo (?:mal|sozinho|perdid[oa]|pra baixo)|chorei|vontade de chorar|ta pesado|dia (?:ruim|pesado|dificil)|to na pior|desabaf)\b";
const RISCO: &str = r"\b(suicid|me matar|tirar (?:a )?minha vida|acabar com (?:tudo|a minha vida)|nao quero (?:mais )?viver|sumir pra sempre|me machucar|automutila|nao vejo saida|queria (?:morrer|desaparecer)|melhor (?:eu )?nao existir)\b";
const CANSACO: &str = r"\b(cansad[oa]|exaust[oa]|esgotad[oa]|sem energia|sem dormir|dormi mal|noite em claro|nao dormi)\b";
const PRESSA: &str = r"\b(urgente|rapido|o quanto antes|pra ontem|agora (?:mesmo)?|corre|depressa|hoje ainda|ja ja)\b";
const PREOCUPACAO: &str = r"\b(preocupad[oa]|aflit[oa]|nervos[oa]|com medo|receio|nao sei (?:o que fazer|como vou)|(?:to|estou) perdid[oa]|nao sei se vai dar)\b";
const PEDIDO_INDIRETO: &str = r"\b(seria bom se|queria (?:que|saber se)|sera que (?:da|voce consegue|tem como)|se (?:voce|vc) pudesse|tomara que|ia ser (?:bom|otimo|legal)|eu precisava (?:de|que))\b";
const IRONIA: &str = r"(?i)(kkk+|haha+|rs+\b|😂|🤣|😅|\bsq\b|so que nao|so brincando|brincadeira|zoeira|vou vender (?:meu|um) rim|\bmeu deus ne\b|claro que sim ne|que maravilha ne|parabens pra mim)";
const POSITIVO: &str = r"\b(tranquilo|de boa|tudo certo|tudo bem agora|ja passou|melhorei|resolvido|resolveu|passou|nao e nada)\b";
const NEGACAO_RECENTE: &str = r"\b(deixa pra la|esquece|nao precisa|nao quero (?:ajuda|isso)|me deixa|so desabafando|nao e nada)\b";

fn tem(padrao: &str, t: &str) -> bool {
    Regex::new(padrao).expect("regex fixa").is_match(t)
}

fn r2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

#[derive(Clone, Debug, PartialEq)]
pub struct Classe {
    pub categorias: Vec<String>,
    pub ironia: bool,
    pub risco: bool,
    /// recuou ("deixa pra la") ou disse que ja passou
    pub recuo: bool,
    /// 0 a 0.99
    pub confianca: f64,
    /// o que o modelo local respondeu, se foi consultado
    pub local: Option<Value>,
}

/// Camada 1 (sem rede).
pub fn classificar_regras(texto: &str) -> Classe {
    let t = sem_acento(texto);
    let mut cats: Vec<String> = [("financeiro", FINANCEIRO), ("emocional", EMOCIONAL), ("cansaco", CANSACO), ("preocupacao", PREOCUPACAO), ("pressa", PRESSA), ("pedido_indireto", PEDIDO_INDIRETO)]
        .iter()
        .filter(|c| tem(c.1, &t))
        .map(|c| c.0.to_string())
        .collect();
    let risco = tem(RISCO, &t);
    let ironia = tem(IRONIA, texto) || tem(IRONIA, &t);
    let (positivo, recuo) = (tem(POSITIVO, &t), tem(NEGACAO_RECENTE, &t));
    let mut conf = 0.0;
    if !cats.is_empty() {
        conf = 0.55 + 0.15 * cats.len().min(2) as f64;
        if texto.chars().count() > 25 {
            conf += 0.05; // frase com contexto e mais confiavel que palavra solta
        }
    }
    if ironia {
        conf -= 0.35; // piada/zoeira: derruba a confianca
    }
    if positivo || recuo {
        conf -= 0.25;
    }
    if risco {
        conf = conf.max(0.95);
        cats.push("risco".into());
    }
    Classe { categorias: cats, ironia, risco, recuo: recuo || positivo, confianca: r2(conf.clamp(0.0, 0.99)), local: None }
}

/// Classificacao completa. `local(texto, contexto)` = modelo LOCAL que confirma/ajusta (None = so as regras). Sinal de risco nunca espera o modelo.
pub fn classificar(texto: &str, ctx: &[String], local: Option<&dyn Fn(&str, &[String]) -> Option<Value>>) -> Classe {
    let mut r = classificar_regras(texto);
    let duvida = !r.categorias.is_empty() || r.ironia;
    let Some(d) = local.filter(|_| duvida && !r.risco).and_then(|l| l(texto, ctx)).filter(|d| d.as_object().is_some_and(|o| !o.is_empty())) else { return r };
    let c_local = match d.get("confianca") {
        None => 0.5,
        Some(c) => c.as_f64().or_else(|| c.as_str().and_then(|s| s.trim().parse().ok())).unwrap_or(0.5),
    };
    if (d["ironia"] == true || d["ironia"] == "true" || d["ironia"] == "True") && !r.ironia {
        r.ironia = true;
        r.confianca = r2((r.confianca - 0.3).max(0.0));
    }
    let intencao = d["intencao"].as_str().unwrap_or("");
    if intencao.starts_with("financ") || intencao.starts_with("emoc") {
        r.confianca = r2(((r.confianca + c_local) / 2.0 + 0.1).min(0.99));
    } else if (intencao == "conversa" || intencao == "comando") && r.confianca < 0.9 {
        r.confianca = r2((r.confianca - 0.25).max(0.0)); // o modelo local acha que e conversa comum: segura
    }
    r.local = Some(d);
    r
}

// quanto maior o impacto, maior a confianca exigida
const LIMIAR_VAGAS: f64 = 0.65;
const LIMIAR_PERGUNTAR: f64 = 0.6;
const LIMIAR_AVISAR: f64 = 0.8;

fn acao(tipo: &str, motivo: &str, c: f64, impacto: &str, consentimento: bool) -> Value {
    json!({ "tipo": tipo, "motivo": motivo, "confianca": c, "impacto": impacto, "requer_consentimento": consentimento })
}

/// Acoes que o Braniac PROPORIA (nao executa). `historico` = registros recentes ({categorias: [...]}).
pub fn propor(r: &Classe, historico: &[Value]) -> Vec<Value> {
    let c = r.confianca;
    let tem_cat = |x: &str| r.categorias.iter().any(|k| k == x);
    if tem_cat("risco") {
        return vec![acao("acolher_e_indicar_ajuda", "sinal de risco: acolher, indicar ajuda humana (CVV 188, pessoas proximas) e nao insistir", c, "alto", false)];
    }
    if r.ironia || r.recuo {
        return vec![];
    }
    // recorrencia: o tema ja apareceu antes? (reduz falso positivo de desabafo passageiro)
    let rec = historico.iter().filter(|h| h["categorias"].as_array().is_some_and(|a| a.iter().any(|k| k.as_str().is_some_and(tem_cat)))).count();
    let reforco = c + 0.05 * rec.min(2) as f64;
    let mut acoes = vec![];
    if tem_cat("financeiro") && reforco >= LIMIAR_VAGAS {
        acoes.push(acao("buscar_vagas", "reclamou de dinheiro/contas", c, "baixo", false));
    }
    if tem_cat("emocional") || tem_cat("cansaco") || tem_cat("preocupacao") {
        if reforco >= LIMIAR_PERGUNTAR {
            acoes.push(acao("perguntar_como_esta", "disse que nao esta bem/cansado/preocupado", c, "baixo", false));
        }
        if c >= LIMIAR_AVISAR {
            acoes.push(acao("oferecer_avisar_alguem", "emocional forte: oferecer avisar uma pessoa proxima (so se ele disser sim)", c, "medio", true));
        }
    }
    acoes
}

fn ler_log(dados: &Path, n: usize) -> Vec<Value> {
    let Ok(t) = std::fs::read_to_string(dados.join("entrelinhas.jsonl")) else { return vec![] };
    let linhas: Vec<&str> = t.lines().collect();
    linhas[linhas.len().saturating_sub(n)..].iter().filter_map(|l| serde_json::from_str::<Value>(l).ok()).collect()
}

/// Chamado a cada mensagem do dono. NUNCA age: so grava. Devolve o registro (ou None se nada a anotar). `agora` = segundos desde 1970.
pub fn observar(dados: &Path, texto: &str, ctx: &[String], local: Option<&dyn Fn(&str, &[String]) -> Option<Value>>, agora: f64) -> Option<Value> {
    let texto = texto.trim();
    if texto.chars().count() < 4 || texto.starts_with('/') {
        return None;
    }
    let r = classificar(texto, ctx, local);
    if r.categorias.is_empty() && !r.ironia {
        return None; // conversa comum: nao enche o log
    }
    let recentes: Vec<Value> = ler_log(dados, 30).into_iter().filter(|h| agora - h["ts"].as_f64().unwrap_or(0.0) < 48.0 * 3600.0).collect();
    // "executou" fica sempre vazio na fase 1
    let reg = json!({ "ts": agora, "texto": texto.chars().take(300).collect::<String>(), "categorias": r.categorias, "ironia": r.ironia, "risco": r.risco, "recuo": r.recuo,
                      "confianca": r.confianca, "local": r.local, "proporia": propor(&r, &recentes), "executou": [] });
    std::fs::create_dir_all(dados).ok()?;
    let mut f = std::fs::File::options().create(true).append(true).open(dados.join("entrelinhas.jsonl")).ok()?;
    writeln!(f, "{reg}").ok()?;
    Some(reg)
}

/// Texto legivel do que foi observado (para o dono conferir). `fuso_s` = segundos a leste de UTC (Brasilia = -10800).
pub fn revisar(dados: &Path, n: usize, fuso_s: i64) -> String {
    let reg = ler_log(dados, n);
    if reg.is_empty() {
        return "Ainda não anotei nada.".into();
    }
    reg.iter()
        .map(|h| {
            let s = h["ts"].as_f64().unwrap_or(0.0) as i64 + fuso_s;
            let (dia, resto) = (Data(s.div_euclid(86400)), s.rem_euclid(86400));
            let lista = |v: &Value, chave: Option<&str>| -> Vec<String> {
                v.as_array().map(|a| a.iter().filter_map(|x| chave.map_or(x, |c| &x[c]).as_str().map(str::to_string)).collect()).unwrap_or_default()
            };
            let (prop, cats) = (lista(&h["proporia"], Some("tipo")).join(", "), lista(&h["categorias"], None).join("+"));
            format!(
                "{} {:02}:{:02} | \"{}\" -> {}{} conf={} | proporia: {}",
                dia.dia_mes(),
                resto / 3600,
                resto / 60 % 60,
                h["texto"].as_str().unwrap_or("").chars().take(80).collect::<String>(),
                if cats.is_empty() { "ironia" } else { &cats },
                if h["ironia"] == true { " [ironia]" } else { "" },
                h["confianca"],
                if prop.is_empty() { "nada (seguro)" } else { &prop }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn dados(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-entrelinhas-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn tipos(frase: &str) -> Vec<String> {
        propor(&classificar(frase, &[], None), &[]).iter().map(|a| a["tipo"].as_str().unwrap().to_string()).collect()
    }

    #[test]
    fn reclamacao_financeira_propoe_buscar_vagas() {
        for f in ["as contas estão apertadas esse mês, não sei como vou pagar", "to sem grana, preciso juntar dinheiro pra pagar o aluguel", "preciso ganhar uma renda extra urgente", "estou sem dinheiro e o boleto vence amanhã"] {
            assert!(tipos(f).contains(&"buscar_vagas".to_string()), "{f}");
        }
    }

    #[test]
    fn piada_ou_recuo_nao_dispara() {
        for f in ["to sem grana kkkkk vou vender meu rim 😂", "haha tô duro, brincadeira", "to sem dinheiro mas já passou, tranquilo", "esquece, não precisa, tô sem grana mesmo"] {
            assert!(tipos(f).is_empty(), "{f}");
        }
    }

    #[test]
    fn nao_estar_bem_propoe_perguntar() {
        for f in ["não tô muito bem hoje", "estou muito cansado, sem energia", "tô meio pra baixo ultimamente", "to preocupado, não sei o que fazer"] {
            assert!(tipos(f).contains(&"perguntar_como_esta".to_string()), "{f}");
        }
    }

    #[test]
    fn avisar_alguem_so_com_confianca_alta_e_sempre_com_consentimento() {
        let acoes = propor(&classificar("não tô bem, tô muito cansado e esgotado, não aguento mais", &[], None), &[]);
        let av: Vec<&Value> = acoes.iter().filter(|a| a["tipo"] == "oferecer_avisar_alguem").collect();
        assert!(av.len() == 1 && av[0]["requer_consentimento"] == true);
        assert!(!tipos("tô cansado").contains(&"oferecer_avisar_alguem".to_string()));
    }

    #[test]
    fn sinal_de_risco_so_acolhe_e_indica_ajuda_e_nao_espera_o_modelo() {
        let r = classificar("às vezes queria desaparecer kkk", &[], Some(&|_, _| panic!("risco nao consulta o modelo")));
        let acoes = propor(&r, &[]);
        assert!(acoes.len() == 1 && acoes[0]["tipo"] == "acolher_e_indicar_ajuda" && acoes[0]["motivo"].as_str().unwrap().contains("CVV"));
        assert!(r.confianca >= 0.95);
    }

    #[test]
    fn conversa_normal_nao_gera_acao() {
        for f in ["bom dia, tudo bem?", "responde a Ana no airbnb", "manda mensagem pra Bia", "qual o horário do jogo?", "o dólar subiu hoje?"] {
            assert!(tipos(f).is_empty(), "{f}");
        }
        assert!(classificar("seria bom se você olhasse minha agenda amanhã", &[], None).categorias.contains(&"pedido_indireto".to_string()));
    }

    #[test]
    fn modelo_local_pode_segurar_ou_reforcar() {
        let base = classificar("to sem grana", &[], None).confianca;
        let seguro = classificar("to sem grana", &[], Some(&|_, _| Some(json!({"intencao": "conversa", "ironia": false, "confianca": 0.9}))));
        assert!(seguro.confianca < base);
        let reforco = classificar("to sem grana", &[], Some(&|_, _| Some(json!({"intencao": "financeira", "confianca": "0.9"}))));
        assert!(reforco.confianca > base);
        let ironia = classificar("to sem grana", &[], Some(&|_, _| Some(json!({"intencao": "financeira", "ironia": true, "confianca": 0.9}))));
        assert!(ironia.ironia && propor(&ironia, &[]).is_empty());
        let sem = classificar("as contas estão apertadas", &[], Some(&|_, _| None));
        assert!(sem.local.is_none() && sem.categorias == ["financeiro"]);
        // sem gatilho, o modelo nem e chamado
        classificar("bom dia", &[], Some(&|_, _| panic!("conversa comum nao consulta o modelo")));
    }

    #[test]
    fn recorrencia_reforca_o_tema() {
        let mut r = classificar("grana curta", &[], None);
        r.confianca = 0.56;
        let hist = [json!({"categorias": ["financeiro"]}), json!({"categorias": ["financeiro"]}), json!({"categorias": ["pressa"]})];
        assert!(propor(&r, &[]).is_empty() && propor(&r, &hist[2..]).is_empty());
        assert_eq!(propor(&r, &hist).len(), 1);
    }

    #[test]
    fn observar_so_grava_e_nunca_executa() {
        let d = dados("a");
        assert_eq!(revisar(&d, 15, 0), "Ainda não anotei nada.");
        let reg = observar(&d, "as contas estão apertadas, não sei como vou pagar", &[], None, 1_800_000_000.0).unwrap();
        assert!(reg["executou"] == json!([]) && reg["proporia"].as_array().unwrap().iter().any(|a| a["tipo"] == "buscar_vagas"));
        assert!(observar(&d, "bom dia", &[], None, 1_800_000_001.0).is_none() && observar(&d, "/status to sem grana", &[], None, 1_800_000_002.0).is_none());
        assert!(observar(&d, "kkk", &[], None, 1.0).is_none()); // curto demais
        assert!(observar(&d, "que maravilha né kkkk", &[], None, 1_800_000_003.0).is_some()); // ironia sem categoria tambem anota
        let log = std::fs::read_to_string(d.join("entrelinhas.jsonl")).unwrap();
        assert_eq!(log.lines().count(), 2);
        // so o arquivo do log existe: nada mais foi criado nem enviado
        assert_eq!(d.read_dir().unwrap().count(), 1);
        let rev = revisar(&d, 15, -10800);
        assert!(rev.starts_with("15/01 05:00 | \"as contas estão apertadas") && rev.contains("-> financeiro+preocupacao conf=") && rev.contains("proporia: buscar_vagas"), "{rev}");
        assert!(rev.lines().nth(1).unwrap().contains("-> ironia [ironia] conf=0.0 | proporia: nada (seguro)"), "{rev}");
    }
}
