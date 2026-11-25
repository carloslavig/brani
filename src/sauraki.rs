//! SAURAKI, a camada VISUAL: monta o retrato (o JSON de GET /api/sauraki) do "escritorio", um personagem por agente real.
//! So LE o que ja existe: registro de agentes, barramento, HORA de modificacao dos arquivos de estado, propostas, equipes das empresas.
//! Estado so e afirmado com dado real; sem dado = null + "dado não disponível". Nunca devolve conteudo de mensagem de pessoa,
//! o que o dono diz ao Jarvis (recados `usuarios_disse`/`dono_disse`), segredo ou token. Nenhuma IA, nenhuma escrita.
//! Ficou de fora (entra pronto em `Extras`, quem chama monta): situacao do Compasso, leitura termica, resumo publico da esteira,
//! predios da rede, resumos das conversas da loja e do Link. Diferenca do Python: as mensagens dizem "dono" no lugar do nome.
#![allow(dead_code)]
use crate::empresa_dev::{corta, ler};
use crate::tempo::Agora;
use crate::{agentes, barramento, empresa_compras, empresa_cortes, empresa_dev, empresa_inv, empresa_mkt, modulos};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

pub const JANELA_EXECUTANDO_S: f64 = 180.0;
pub const JANELA_CONSULTANDO_S: f64 = 30.0;
pub const JANELA_ESTEIRA_S: f64 = 15.0 * 60.0;
pub const ESTADOS: [&str; 10] = ["ocioso", "analisando", "planejando", "executando", "consultando", "aguardando_autorizacao", "aguardando_servico", "concluido", "erro", "desconectado"];
pub const SEM_FONTE: &str = "sem registro de atividade para este agente (dado não disponível)";

/// Arquivos (relativos a dados/) cuja HORA de modificacao conta como atividade do agente. O conteudo nunca e usado.
const FONTES: &[(&str, &[&str])] = &[
    ("Jarvis", &["dono.log", "atendimentos.log", "vigia.log"]),
    ("Alma", &["alma/diario.jsonl"]),
    ("Forja", &["manutencao.json", "automelhoria.json"]),
    ("Jorge", &["airbnb.log"]),
    ("Claudinho", &["claudinho/achados.jsonl", "claudinho/estado.json"]),
    ("Tino", &["erros_jarvis.jsonl", "erros_detectados.json"]),
    ("Valério", &["financeiro.json"]),
    ("Nando", &["candidaturas.json"]),
    ("Cofre", &["financeiro.json"]),
    ("Compasso", &["agenda_envios.log", "compasso/situacao.json"]),
    ("Correio", &["correio.json"]),
    ("Garimpeiro", &["vagas.json"]),
    ("Monza", &["monza.json"]),
    ("Zelo", &["atendimentos.log"]),
    ("Vaquinha", &["vaquinha.json"]),
    ("Lupa", &["parceiro/lote.json"]),
];

/// O que o Braniac ainda nao sabe montar sozinho em Rust; vazio = "dado não disponível".
#[derive(Default)]
pub struct Extras {
    /// {agente: {"texto": ...}} do Compasso.
    pub situacao: Value,
    /// {"gpu_c", "cpu_pct", "ram_pct", "quente"}.
    pub recursos: Value,
    /// Resumo PUBLICO da esteira de videos ({"esteira", "cidades", "capitais"}); None = andar sem dado.
    pub esteira: Option<Value>,
    pub predios: Value,
    pub loja_conversa: Value,
    pub link_conversa: Value,
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}

fn mtime(dados: &Path, rel: &str) -> f64 {
    std::fs::metadata(dados.join(rel)).and_then(|m| m.modified()).ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0.0, |d| d.as_secs_f64())
}

fn arq(dados: &Path, rel: &str) -> Value {
    ler(&dados.join(rel)).unwrap_or(Value::Null)
}

/// O que o dono diz ao Jarvis nunca vira texto na tela.
fn privado(tipo: &str) -> bool {
    matches!(tipo, "usuarios_disse" | "dono_disse")
}

/// Propostas de melhoria esperando o dono (so conta; le o cabecalho do arquivo).
pub fn propostas_pendentes(dados: &Path) -> usize {
    let Ok(dir) = std::fs::read_dir(dados.join("propostas")) else { return 0 };
    dir.flatten().filter(|e| e.path().extension().is_some_and(|x| x == "md"))
        .filter(|e| std::fs::read(e.path()).is_ok_and(|b| corta(&String::from_utf8_lossy(&b), 600).to_uppercase().contains("AGUARDANDO APROVA")))
        .count()
}

/// [(nome tecnico, especialidade)] do registro real + o Jarvis.
fn registrados(dados: &Path) -> Vec<(String, String)> {
    let mut out = vec![("Jarvis".to_string(), "Entrada de todas as conversas; orquestra os agentes e as ferramentas".to_string())];
    for a in agentes::listar(dados) {
        let n = s(&a, "agente");
        if !n.is_empty() && !out.iter().any(|(x, _)| x == n) {
            out.push((n.to_string(), s(&a, "descricao").to_string()));
        }
    }
    out
}

struct Pessoa {
    nome: String,
    papel: String,
    andar: i64,
    sala: Option<String>,
}

/// Registro + equipes das empresas cujo modulo esta ligado. Nome repetido fica so no primeiro andar em que apareceu.
fn elenco(dados: &Path) -> Vec<Pessoa> {
    let mut out: Vec<Pessoa> = registrados(dados).into_iter().map(|(nome, papel)| Pessoa { nome, papel, andar: 1, sala: None }).collect();
    let ativos = modulos::ativos(dados);
    let equipes: [(i64, &str, Vec<Value>, &str); 5] = [
        (2, "programacao", empresa_dev::agentes(dados), ""),
        (3, "marketing", empresa_mkt::config(dados)["agentes"].as_array().cloned().unwrap_or_default(), "Marketing"),
        (4, "investimentos", empresa_inv::config(dados)["agentes"].as_array().cloned().unwrap_or_default(), "Mesa"),
        (5, "cortes_video", empresa_cortes::agentes(dados), "Mesa"),
        (6, "compras", empresa_compras::config(dados)["agentes"].as_array().cloned().unwrap_or_default(), "Mesa"),
    ];
    for (andar, modulo, equipe, sala_padrao) in equipes {
        if !ativos.iter().any(|m| m == modulo) {
            continue;
        }
        let usados: HashSet<String> = out.iter().map(|p| p.nome.clone()).collect();
        for a in equipe {
            let nome = s(&a, "nome");
            if nome.is_empty() || usados.contains(nome) {
                continue;
            }
            let sala = match s(&a, "sala") {
                "" if andar == 2 => empresa_dev::sala_de(&a),
                "" => sala_padrao,
                x => x,
            };
            out.push(Pessoa { nome: nome.to_string(), papel: s(&a, "papel").to_string(), andar, sala: Some(sala.to_string()) });
        }
    }
    out
}

fn fontes(p: &Pessoa) -> Vec<&'static str> {
    if p.andar == 5 && p.sala.as_deref() == Some("Estúdio") {
        return vec!["empresa_cortes/estudio.json"];
    }
    if let Some((_, f)) = FONTES.iter().find(|(n, _)| *n == p.nome) {
        return f.to_vec();
    }
    match p.andar {
        2 => vec!["empresa_dev/tickets.json"],
        3 => vec!["empresa_mkt/pedidos.json"],
        4 => vec!["empresa_inv/carteira.json"],
        5 => vec!["empresa_cortes/pedidos.json"],
        6 => vec!["empresa_compras/pedidos.json"],
        _ => vec![],
    }
}

/// ({agente: ultimo recado ENVIADO por ele}, {quem pediu algo agora: a quem}).
fn fala_do_barramento(dados: &Path, ts: f64) -> (BTreeMap<String, Value>, BTreeMap<String, String>) {
    let (mut ult, mut consultas) = (BTreeMap::new(), BTreeMap::new());
    for m in barramento::historico(dados, 120) {
        let (tipo, mts) = (s(&m, "tipo"), m["ts"].as_f64().unwrap_or(0.0));
        let texto = if privado(tipo) { String::new() } else { corta(s(&m, "texto"), 140) };
        ult.insert(s(&m, "de").to_string(), json!({ "ts": mts, "texto": texto, "tipo": tipo, "para": s(&m, "para") }));
        if (tipo == "pedido" || privado(tipo)) && ts - mts <= JANELA_CONSULTANDO_S {
            consultas.insert(s(&m, "de").to_string(), s(&m, "para").to_string());
        }
    }
    (ult, consultas)
}

/// {agente: os 8 registros mais novos}: recados do barramento + historico dos tickets e pedidos. O que o dono diz nunca entra.
fn logs(dados: &Path) -> BTreeMap<String, Vec<Value>> {
    let mut l: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let mut add = |nome: &str, ts: f64, onde: String, txt: &str| {
        if !nome.is_empty() && !txt.is_empty() {
            l.entry(if nome == "senior" { "Claude".into() } else { nome.to_string() }).or_default().push(json!({ "ts": ts, "onde": onde, "texto": corta(txt, 240) }));
        }
    };
    for m in barramento::historico(dados, 300) {
        if !privado(s(&m, "tipo")) {
            add(s(&m, "de"), m["ts"].as_f64().unwrap_or(0.0), format!("{} → {}", s(&m, "tipo"), s(&m, "para")), s(&m, "texto"));
        }
    }
    for (rel, rot) in [("empresa_dev/tickets.json", "ticket"), ("empresa_mkt/pedidos.json", "pedido")] {
        for t in arq(dados, rel).as_array().into_iter().flatten() {
            for h in t["historico"].as_array().into_iter().flatten() {
                add(s(h, "quem"), h["ts"].as_f64().unwrap_or(0.0), format!("{rot} {}", s(t, "id")), s(h, "txt"));
            }
        }
    }
    for v in l.values_mut() {
        v.sort_by(|a, b| a["ts"].as_f64().partial_cmp(&b["ts"].as_f64()).unwrap_or(std::cmp::Ordering::Equal));
        let n = v.len().saturating_sub(8);
        *v = v.split_off(n);
        v.reverse();
    }
    l
}

/// (estado ou None, motivo). Cada estado so aparece quando ha dado real por tras.
fn estado_agente(dados: &Path, nome: &str, ultima: Option<f64>, consultas: &BTreeMap<String, String>, ts: f64, tem_fonte: bool, propostas: usize) -> (Option<&'static str>, String) {
    if nome == "Vaquinha" && arq(dados, "vaquinha.json")["ativo"].as_bool() != Some(true) {
        return (Some("desconectado"), "agente ainda não ligado (falta logar o banco)".into());
    }
    if nome == "Forja" && arq(dados, "manutencao.json")["testes_ok"].as_bool() == Some(false) {
        return (Some("erro"), "a manutenção da madrugada reportou teste falhando".into());
    }
    if nome == "Vaquinha" && !s(&arq(dados, "vaquinha.json"), "ultimo_erro").is_empty() {
        return (Some("erro"), "último deploy do site falhou".into());
    }
    if nome == "Jorge" {
        let pend = arq(dados, "airbnb_estado.json")["consultas"].as_object().map_or(0, |o| o.values().filter(|c| c["estado"] == "consulta").count());
        if pend > 0 {
            return (Some("aguardando_autorizacao"), format!("{pend} reserva(s)/hóspede(s) esperando o OK de vocês"));
        }
    }
    if matches!(nome, "Forja" | "Alma") && propostas > 0 {
        return (Some("aguardando_autorizacao"), format!("{propostas} proposta(s) de melhoria esperando aprovação do dono"));
    }
    if let Some(para) = consultas.get(nome) {
        return (Some("consultando"), format!("pediu algo a {para} pelo barramento"));
    }
    if ultima.is_some_and(|u| ts - u <= JANELA_EXECUTANDO_S) {
        return (Some("executando"), "atividade registrada agora há pouco".into());
    }
    if !tem_fonte {
        return (None, SEM_FONTE.into());
    }
    (Some("ocioso"), "sem atividade recente".into())
}

fn contagem(ags: &[Value]) -> Map<String, Value> {
    ESTADOS.iter().map(|k| (k.to_string(), json!(ags.iter().filter(|a| a["estado"] == *k).count()))).collect()
}

/// Retrato completo para a tela. `status` = o /api/status ja montado pelo servidor.
pub fn snapshot(dados: &Path, status: &Value, ts: f64, agora: &Agora, x: &Extras) -> Value {
    let (falas, consultas) = fala_do_barramento(dados, ts);
    let lg = logs(dados);
    let propostas = propostas_pendentes(dados);
    let mut ags = Vec::new();
    for p in elenco(dados) {
        let f = fontes(&p);
        let ts_arq = f.iter().map(|r| mtime(dados, r)).fold(0.0, f64::max);
        let fala = falas.get(&p.nome);
        let ts_bus = fala.and_then(|m| m["ts"].as_f64()).unwrap_or(0.0);
        let ultima = Some(ts_arq.max(ts_bus)).filter(|u| *u > 0.0);
        let (est, motivo) = estado_agente(dados, &p.nome, ultima, &consultas, ts, !f.is_empty(), propostas);
        let tarefa = fala.map(|m| s(m, "texto")).filter(|t| !t.is_empty()).or_else(|| x.situacao[p.nome.as_str()]["texto"].as_str());
        let avisos: Vec<&str> = if matches!(est, Some("erro" | "aguardando_autorizacao")) { vec![&motivo] } else { vec![] };
        ags.push(json!({
            "nome": p.nome, "exibir": if p.nome == "Alma" { "A.L.M.A." } else { p.nome.as_str() }, "especialidade": p.papel, "estado": est, "motivo": motivo,
            "andar": p.andar, "sala": p.sala, "ultima_atividade": ultima, "tarefa": tarefa,
            "atividades": fala.filter(|m| !privado(s(m, "tipo"))).into_iter().collect::<Vec<_>>(), "inicio": null, "duracao_s": null,
            "log": lg.get(&p.nome).cloned().unwrap_or_default(), "avisos": avisos,
        }));
    }
    let mut cont = contagem(&ags);
    cont.insert("sem_dado".into(), json!(ags.iter().filter(|a| a["estado"].is_null()).count()));
    let ligado = |k: &str| status[k]["ligado"].as_bool().unwrap_or(false);
    let servicos = json!([
        { "nome": "Atendimento WhatsApp", "ok": ligado("atendimento_auto") },
        { "nome": "Airbnb", "ok": ligado("airbnb_auto") },
        { "nome": "Microfone", "ok": status["escuta_voz"]["estado"].as_str().is_some_and(|e| e != "parada") },
        { "nome": "Gmail", "ok": status["email"]["logado"].as_bool().unwrap_or(false) },
        { "nome": "Braniac", "ok": status["ligado"].as_bool().unwrap_or(true) },
    ]);
    let uso: Vec<Value> = status["provedores"].as_array().into_iter().flatten().map(|p| json!({ "nome": p["nome"], "ok": p["ok"], "hoje": p["hoje"] })).collect();
    let festa = arq(dados, "sauraki_festa.json")["ate"].as_f64().filter(|f| *f > ts);
    let aut = &status["autonomia"];
    let mut empresa = empresa_dev::situacao(dados, agora);
    if let Some(o) = empresa.as_object_mut() {
        o.insert("compras".into(), empresa_compras::situacao(dados, agora));
        o.insert("marketing".into(), empresa_mkt::situacao(dados, agora, ts));
        o.insert("investimentos".into(), empresa_inv::situacao(dados));
        o.insert("cortes".into(), empresa_cortes::situacao(dados));
    }
    let conversa: Vec<Value> = barramento::historico(dados, 40).into_iter().filter(|m| !privado(s(m, "tipo")))
        .map(|m| json!({ "id": m["id"], "ts": m["ts"], "de": m["de"], "para": m["para"], "tipo": m["tipo"], "texto": corta(s(&m, "texto"), 140) })).collect();
    let recursos: Map<String, Value> = ["gpu_c", "cpu_pct", "ram_pct", "quente"].iter().map(|k| (k.to_string(), x.recursos[*k].clone())).collect();
    json!({
        "ts": ts, "festa_ate": festa, "simulado": false, "total": ags.len(), "agentes": ags, "contagem": cont,
        "autonomia": { "armada": aut["armada"].as_bool().unwrap_or(false), "restante_min": aut["restante_min"], "indefinida": aut["indefinida"].as_bool().unwrap_or(false) },
        "servicos": servicos, "provedores": uso, "recursos": recursos,
        "predios": if x.predios.is_array() { x.predios.clone() } else { json!([]) },
        "esteira": x.esteira.as_ref().map(|p| esteira(p, ts)), "loja_conversa": x.loja_conversa, "link_conversa": x.link_conversa,
        "empresa": empresa, "conversa": conversa, "propostas_pendentes": propostas, "tokens": null,
        "nota": "Estados não observáveis (analisando, planejando, concluído, aguardando serviço) aparecem como 'dado não disponível' até haver eventos reais.",
    })
}

/// chave, agente, papel, etapas da cidade que contam para a estacao.
const ESTACOES: &[(&str, &str, &str, &[&str])] = &[
    ("busca", "Busca", "baixa os PDFs do tribunal", &["reservada", "baixando_tcm"]),
    ("fiscal", "Fiscal", "confere as contas com o Balanço", &["levantamento"]),
    ("pena", "Pena", "roteiro e cenas", &["roteiro"]),
    ("eco", "Eco", "voz e sincronia", &["voz", "sincronia"]),
    ("prisma", "Prisma", "cenas 3D e render", &["render"]),
    ("lupa", "Lupa", "confere o vídeo pronto", &["conferencia"]),
    ("entrega", "Entrega", "entrega o vídeo no site", &["pronto"]),
];

/// Andar da esteira a partir do resumo PUBLICO: so numero, cidade e etapa. Nada de caminho, contato, motivo ou log.
pub fn esteira(publico: &Value, ts: f64) -> Value {
    let e = &publico["esteira"];
    let desde = |c: &Value| c["desde"].as_f64().unwrap_or(0.0);
    let mut cidades: Vec<&Value> = publico["cidades"].as_object().map(|o| o.values().collect()).unwrap_or_default();
    cidades.sort_by(|a, b| desde(b).partial_cmp(&desde(a)).unwrap_or(std::cmp::Ordering::Equal));
    let atual = &e["atual"];
    let par = |c: &Value| json!({ "numero": c["numero"], "cidade": c["cidade"] });
    let est: Vec<Value> = ESTACOES.iter().map(|(chave, agente, papel, etapas)| {
        let aqui: Vec<&&Value> = cidades.iter().filter(|c| etapas.contains(&s(c, "etapa"))).collect();
        let agora_aqui = etapas.contains(&s(atual, "etapa")) || aqui.iter().any(|c| ts - desde(c) < JANELA_ESTEIRA_S);
        json!({ "chave": chave, "agente": agente, "papel": papel, "total": aqui.len(),
                "estado": if agora_aqui { "executando" } else if aqui.is_empty() { "ocioso" } else { "aguardando_servico" },
                "cidades": aqui.iter().take(4).map(|c| par(c)).collect::<Vec<_>>() })
    }).collect();
    let pend: Vec<&&Value> = cidades.iter().filter(|c| s(c, "etapa") == "pendencia").collect();
    let tem_atual = atual.as_object().is_some_and(|o| !o.is_empty());
    json!({
        "estacoes": est, "atual": if tem_atual { json!({ "numero": atual["numero"], "cidade": atual["cidade"], "etapa": atual["etapa"] }) } else { Value::Null },
        "prontos": e["prontos"].as_array().map_or(0, Vec::len), "meta": e["meta"], "pendencias": pend.len(),
        "pendentes": pend.iter().take(6).map(|c| par(c)).collect::<Vec<_>>(), "progresso_pct": e["progresso_pct"],
        "quadrados": e["slots"].as_array().into_iter().flatten().map(|q| q["estado"].as_str().filter(|x| !x.is_empty()).unwrap_or("fila")).collect::<Vec<_>>(),
        "capitais": capitais(&publico["capitais"]),
    })
}

/// Telao das capitais: so a etapa de cada uma e a fracao pronta (sem nome, fonte nem problema).
pub fn capitais(caps: &Value) -> Value {
    let q: Vec<&str> = caps.as_array().into_iter().flatten().map(|c| c["etapa"].as_str().filter(|x| !x.is_empty()).unwrap_or("fila")).collect();
    if q.is_empty() {
        return Value::Null;
    }
    let pct = (1000.0 * q.iter().filter(|x| **x == "pronto").count() as f64 / q.len() as f64).round() / 10.0;
    json!({ "quadrados": q, "progresso_pct": pct })
}

/// Copia do retrato SEM texto real: estados rotativos e falas ficticias, para gravar video. Marcada como simulada.
pub fn demo(snap: &Value) -> Value {
    const ROT: [&str; 6] = ["ocioso", "analisando", "executando", "consultando", "concluido", "planejando"];
    let ts = snap["ts"].as_f64().unwrap_or(0.0);
    let mut d = snap.clone();
    let ags: Vec<Value> = snap["agentes"].as_array().into_iter().flatten().enumerate().map(|(i, a)| {
        let mut a = a.clone();
        a["estado"] = json!(ROT[(i + (ts / 8.0) as usize) % ROT.len()]);
        a["motivo"] = json!("demonstração");
        a["tarefa"] = Value::Null;
        a["ultima_atividade"] = Value::Null;
        for k in ["atividades", "avisos", "log"] {
            a[k] = json!([]);
        }
        a
    }).collect();
    d["contagem"] = Value::Object(contagem(&ags));
    d["agentes"] = json!(ags);
    d["conversa"] = json!([
        { "id": 1, "ts": ts, "de": "Jarvis", "para": "Jorge", "tipo": "pedido", "texto": "(demo) hóspede pergunta o horário de check-in" },
        { "id": 2, "ts": ts, "de": "Jorge", "para": "Jarvis", "tipo": "resposta", "texto": "(demo) check-in a partir das 14h" },
    ]);
    d["loja_conversa"] = json!({ "ativa": false, "total": 0, "ultima": null });
    d["link_conversa"] = d["loja_conversa"].clone();
    d["empresa"] = json!({});
    d["esteira"] = Value::Null;
    d["servicos"] = json!(["Atendimento WhatsApp", "Airbnb", "Microfone", "Gmail", "Braniac"].map(|n| json!({ "nome": n, "ok": true })));
    d["provedores"] = json!([]);
    d["simulado"] = json!(true);
    d["festa_ate"] = Value::Null;
    d["propostas_pendentes"] = json!(0);
    d["nota"] = json!("Modo demonstração: nada aqui é real.");
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::empresa_dev::gravar;
    use crate::tempo::Data;
    use std::path::PathBuf;

    fn pasta(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-sauraki-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let ag = |id: &str, nome: &str| json!({ "id": id, "nome": id, "agente": nome, "descricao": "teste", "instrucao": "", "palavras": [] });
        gravar(&d.join("agentes.json"), &json!({ "agentes": [ag("correio", "Correio"), ag("vaquinha", "Vaquinha"), ag("airbnb", "Jorge"), ag("familia", "Alma")] })).unwrap();
        d
    }

    fn agora() -> Agora {
        Agora { data: Data::civil(2026, 11, 18).unwrap(), hora: 10, min: 0, seg: 0 }
    }

    fn ts() -> f64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64()
    }

    fn por_nome(d: &Path, status: &Value) -> (Value, BTreeMap<String, Value>) {
        let snap = snapshot(d, status, ts(), &agora(), &Extras::default());
        let m = snap["agentes"].as_array().unwrap().iter().map(|a| (a["nome"].as_str().unwrap().to_string(), a.clone())).collect();
        (snap, m)
    }

    #[test]
    fn descobre_agentes_do_registro_e_so_afirma_estado_com_dado() {
        let d = pasta("registro");
        let (snap, m) = por_nome(&d, &json!({}));
        for n in ["Jarvis", "Forja", "Ada", "Jorge", "Alma"] {
            assert!(m.contains_key(n), "{n}");
        }
        assert!(m["Ada"]["estado"].is_null() && m["Ada"]["motivo"].as_str().unwrap().contains("não disponível"));
        assert_eq!(m["Alma"]["exibir"], "A.L.M.A.");
        assert!(snap["simulado"] == false && snap["tokens"].is_null() && snap["esteira"].is_null());
        assert_eq!(snap["total"], snap["agentes"].as_array().unwrap().len());
        // equipe da empresa entra no andar dela, com sala, e so quando o modulo esta ligado
        assert!(snap["agentes"].as_array().unwrap().iter().any(|a| a["andar"] == 2 && a["sala"].is_string()));
        modulos::definir(&d, &[]).unwrap();
        assert!(por_nome(&d, &json!({})).0["agentes"].as_array().unwrap().iter().all(|a| a["andar"] == 1));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn executando_e_consultando_vem_dos_arquivos_e_do_barramento() {
        let d = pasta("estados");
        std::fs::write(d.join("correio.json"), "{}").unwrap();
        assert_eq!(por_nome(&d, &json!({})).1["Correio"]["estado"], "executando");
        barramento::enviar(&d, "Forja", "Ada", "pedido", "revisar o modulo X", 0, None).unwrap();
        let f = &por_nome(&d, &json!({})).1["Forja"];
        assert!(f["estado"] == "consultando" && f["motivo"].as_str().unwrap().contains("Ada"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn desconectado_erro_e_aguardando_sem_vazar_nome_nem_fala_do_dono() {
        let d = pasta("travas");
        assert_eq!(por_nome(&d, &json!({})).1["Vaquinha"]["estado"], "desconectado");
        gravar(&d.join("vaquinha.json"), &json!({ "ativo": true, "ultimo_erro": "x" })).unwrap();
        assert_eq!(por_nome(&d, &json!({})).1["Vaquinha"]["estado"], "erro");
        gravar(&d.join("airbnb_estado.json"), &json!({ "consultas": { "1": { "hospede": "Davi Teste", "estado": "consulta" } } })).unwrap();
        let j = por_nome(&d, &json!({})).1["Jorge"].clone();
        assert!(j["estado"] == "aguardando_autorizacao" && !j.to_string().contains("Davi") && j["avisos"].as_array().unwrap().len() == 1);
        barramento::enviar(&d, "Jarvis", "Alma", "usuarios_disse", "segredo pessoal do dono", 0, None).unwrap();
        barramento::enviar(&d, "Jarvis", "Alma", "dono_disse", "outro segredo pessoal", 0, None).unwrap();
        assert!(!por_nome(&d, &json!({})).0.to_string().contains("segredo"));
        std::fs::create_dir_all(d.join("propostas")).unwrap();
        std::fs::write(d.join("propostas/p1.md"), "# Proposta\nStatus: aguardando aprovação\n").unwrap();
        assert_eq!(propostas_pendentes(&d), 1);
        assert_eq!(por_nome(&d, &json!({})).1["Forja"]["estado"], "aguardando_autorizacao");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn autonomia_e_servicos_refletem_o_status() {
        let d = pasta("status");
        let st = json!({ "autonomia": { "armada": true, "restante_min": 30, "indefinida": false }, "airbnb_auto": { "ligado": false }, "atendimento_auto": { "ligado": true } });
        let snap = por_nome(&d, &st).0;
        assert_eq!(snap["autonomia"]["armada"], true);
        let ok = |n: &str| snap["servicos"].as_array().unwrap().iter().find(|x| x["nome"] == n).unwrap()["ok"].clone();
        assert!(ok("Airbnb") == false && ok("Atendimento WhatsApp") == true && ok("Microfone") == false && ok("Braniac") == true);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn demo_nao_vaza_texto_real() {
        let d = pasta("demo");
        let mut snap = por_nome(&d, &json!({})).0;
        snap["conversa"] = json!([{ "id": 9, "ts": 0, "de": "Cofre", "para": "Compasso", "tipo": "status", "texto": "saldo 9.876,54" }]);
        snap["agentes"][0]["log"] = json!([{ "ts": 0, "onde": "x", "texto": "saldo 9.876,54" }]);
        let dm = demo(&snap);
        assert!(dm["simulado"] == true && !dm.to_string().contains("9.876"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn esteira_mostra_so_numero_cidade_e_etapa() {
        let t = 1000.0;
        let lote = json!({
            "cidades": { "196": { "numero": "196", "cidade": "Cidade A", "etapa": "baixando_tcm", "desde": t - 30.0 },
                         "141": { "numero": "141", "cidade": "Cidade B", "etapa": "voz", "desde": t - 9000.0, "detalhe": "/Users/alguem/x" },
                         "152": { "numero": "152", "cidade": "Cidade C", "etapa": "pendencia", "desde": 1.0, "contato": "5500999" } },
            "esteira": { "meta": 50, "prontos": [{ "numero": "093" }], "atual": { "numero": "196", "cidade": "Cidade A", "etapa": "baixando_tcm", "pasta": "/Users/x" },
                         "slots": [{ "estado": "pronto" }, { "estado": "agora" }, {}] } });
        let e = esteira(&lote, t);
        let agentes: Vec<&str> = e["estacoes"].as_array().unwrap().iter().map(|x| x["agente"].as_str().unwrap()).collect();
        assert_eq!(agentes, ["Busca", "Fiscal", "Pena", "Eco", "Prisma", "Lupa", "Entrega"]);
        let est = |k: &str| e["estacoes"].as_array().unwrap().iter().find(|x| x["chave"] == k).unwrap()["estado"].clone();
        assert!(est("busca") == "executando" && est("eco") == "aguardando_servico" && est("prisma") == "ocioso");
        assert!(e["prontos"] == 1 && e["pendencias"] == 1 && e["atual"]["cidade"] == "Cidade A");
        assert!(!e.to_string().contains("5500999") && !e.to_string().contains("/Users"));
        assert_eq!(e["quadrados"], json!(["pronto", "agora", "fila"]));
        let c = capitais(&json!([{ "numero": "1001", "cidade": "X", "etapa": "pronto", "problemas": ["x"] }, { "numero": "1002", "etapa": "pendencia" }]));
        assert_eq!(c, json!({ "quadrados": ["pronto", "pendencia"], "progresso_pct": 50.0 }));
        assert!(capitais(&json!([])).is_null());
    }
}
