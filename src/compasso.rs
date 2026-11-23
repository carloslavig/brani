//! Compasso, o agente de rotina. Porte de compasso.py: guarda as rotinas e, na hora certa, ACIONA o Jarvis.
//! Protocolo em 4 passos, tudo local e sem IA: 1) rotinas em dados/compasso/rotinas.json; 2) o Compasso monta um
//! PEDIDO e registra em pedidos.jsonl; 3) o Jarvis CONFERE (acao permitida, contato permitido, rotina ativa, horario,
//! 1 vez por dia, teto diario, texto sem assunto proibido); 4) executa so a acao pedida e grava o resultado.
//! Quem pode receber rotina e dado da pessoa: fica em dados/pessoal.json ({"contatos": {"apelido": "nome no chat"}}).
//! Falta portar: acompanhar os outros agentes (barramento), audio de verdade e a acao verificar_youtube.
use crate::agenda;
use crate::tempo::Agora;
use crate::texto_pt::sem_acento;
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

static TRAVA: Mutex<()> = Mutex::new(());
const HORA_MAX: u32 = 21; // ninguem recebe mensagem de rotina de madrugada
const TETO_DIARIO: usize = 10; // pedidos EXECUTADOS por dia, somando todas as rotinas
const DIAS: [&str; 7] = ["seg", "ter", "qua", "qui", "sex", "sab", "dom"];
const ACOES: [&str; 3] = ["perguntar_contato", "lembrar_dono", "audio_contato"];
const DONO: &str = "dono";
const PROIBIDOS: [&str; 15] = ["cartao", "cpf", "senha", "codigo", "token", "pix", "transfer", "pagamento", "pagar", "boleto", "conta banc", "agencia", "chave", "segredo", "login"];
const FORA_DE_HORA: &str = "fora do horário permitido";

/// Como o envio sai: (contato, texto) -> resultado; so conta como feito se comecar com "✅".
pub type Enviar<'a> = &'a dyn Fn(&str, &str) -> String;

fn trava() -> MutexGuard<'static, ()> {
    TRAVA.lock().unwrap_or_else(|e| e.into_inner())
}

fn arq(dados: &Path) -> PathBuf {
    dados.join("compasso").join("rotinas.json")
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}

fn textos(r: &Value) -> Vec<&str> {
    r["textos"].as_array().into_iter().flatten().filter_map(Value::as_str).collect()
}

/// "lembrar_carlos" e o nome antigo (arquivo vindo do Braniac em Python).
fn acao(r: &Value) -> &str {
    match s(r, "acao") {
        "lembrar_carlos" => "lembrar_dono",
        a => a,
    }
}

/// O audio de bom dia pode sair mais cedo; o resto, das 8h as 21h.
fn hora_min(acao: &str) -> u32 {
    if acao == "audio_contato" { 6 } else { 8 }
}

fn texto_max(acao: &str) -> usize {
    if acao == "audio_contato" { 1500 } else { 500 }
}

fn proibido(t: &str) -> bool {
    let t = sem_acento(t);
    PROIBIDOS.iter().any(|p| t.contains(p))
}

fn norm(t: &str) -> String {
    sem_acento(t).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// apelido (sem acento) -> nome no aplicativo de mensagem. So quem a pessoa pos em dados/pessoal.json.
fn contatos(dados: &Path) -> Vec<(String, String)> {
    let v: Value = std::fs::read_to_string(dados.join("pessoal.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    v["contatos"].as_object().into_iter().flatten().map(|(k, n)| (norm(k), n.as_str().unwrap_or(k.as_str()).to_string())).collect()
}

/// Arquivo ilegivel = erro: gravar por cima apagaria as rotinas.
fn ler(dados: &Path) -> Result<Vec<Value>, String> {
    match std::fs::read_to_string(arq(dados)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!("não consegui ler as rotinas: {e}")),
        Ok(t) => serde_json::from_str::<Value>(&t).ok().and_then(|v| v["rotinas"].as_array().cloned()).ok_or_else(|| "o arquivo das rotinas está ilegível; não mexi em nada".to_string()),
    }
}

fn gravar(dados: &Path, rotinas: &[Value]) -> Result<(), String> {
    let (a, tmp) = (arq(dados), arq(dados).with_extension("tmp"));
    std::fs::create_dir_all(dados.join("compasso"))
        .and_then(|_| std::fs::write(&tmp, serde_json::to_string_pretty(&json!({ "rotinas": rotinas })).unwrap_or_default()))
        .and_then(|_| std::fs::rename(&tmp, &a))
        .map_err(|e| format!("não consegui gravar as rotinas: {e}"))
}

pub fn listar(dados: &Path) -> Vec<Value> {
    ler(dados).unwrap_or_default()
}

/// Hora de um "HH:MM" valido.
fn hora_de(horario: &str) -> Option<u32> {
    let b = horario.as_bytes();
    if b.len() != 5 || b[2] != b':' || ![0, 1, 3, 4].iter().all(|&i| b[i].is_ascii_digit()) {
        return None;
    }
    let (h, m) = (horario[..2].parse::<u32>().ok()?, horario[3..].parse::<u32>().ok()?);
    (h < 24 && m < 60).then_some(h)
}

/// Recusa a rotina que foge do protocolo. Vale na criacao e de novo antes de cada execucao.
fn validar(dados: &Path, r: &Value) -> Result<(), String> {
    let a = acao(r);
    if !ACOES.contains(&a) {
        return Err(format!("ação '{a}' fora da lista permitida ({})", ACOES.join(", ")));
    }
    if a != "lembrar_dono" && !contatos(dados).iter().any(|(k, _)| *k == norm(s(r, "para"))) {
        return Err(format!("contato '{}' não pode receber rotina (só quem está em pessoal.json)", s(r, "para")));
    }
    let h = hora_de(s(r, "horario")).ok_or("horário precisa ser HH:MM")?;
    if !(hora_min(a)..=HORA_MAX).contains(&h) {
        return Err(format!("horário fora da janela {:02}h-{HORA_MAX:02}h", hora_min(a)));
    }
    if r["dias"].as_array().into_iter().flatten().any(|d| !DIAS.contains(&d.as_str().unwrap_or(""))) {
        return Err(format!("dias válidos: {}", DIAS.join(", ")));
    }
    let ts = textos(r);
    if ts.is_empty() {
        return Err("essa ação precisa de pelo menos um texto".into());
    }
    for t in ts {
        if proibido(t) {
            return Err("texto da rotina não pode falar de pagamento, senha, código ou dados bancários".into());
        }
        if t.chars().count() > texto_max(a) {
            return Err(format!("texto da rotina muito longo (máx. {})", texto_max(a)));
        }
    }
    let c = s(r, "complemento");
    if proibido(c) || c.chars().count() > 4500 {
        return Err("complemento escrito não pode falar de pagamento, senha, código ou dados bancários (máx. 4500)".into());
    }
    Ok(())
}

/// Guarda uma rotina NOVA. Nasce DESATIVADA: so a pessoa ativa.
pub fn criar(dados: &Path, v: &Value, agora: &Agora) -> Result<Value, String> {
    let id: String = norm(s(v, "id")).replace(' ', "-").chars().take(40).collect();
    if id.is_empty() || s(v, "titulo").trim().is_empty() {
        return Err("a rotina precisa de id e título".into());
    }
    let lista = |k: &str| -> Vec<String> { v[k].as_array().into_iter().flatten().filter_map(Value::as_str).map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect() };
    let r = json!({
        "id": id, "titulo": s(v, "titulo").trim(), "acao": s(v, "acao"), "para": s(v, "para"), "horario": s(v, "horario"), "dias": lista("dias"),
        "textos": lista("textos"), "motivo": s(v, "motivo"), "ativa": false, "ate": s(v, "ate"), "complemento": s(v, "complemento"),
        "criada_por": Some(s(v, "criada_por")).filter(|x| !x.is_empty()).unwrap_or("pessoa"), "criada_em": format!("{} {}", agora.data.iso(), agora.hm()), "ultima": "",
    });
    validar(dados, &r)?;
    let _g = trava();
    let mut rs = ler(dados)?;
    if rs.iter().any(|x| s(x, "id") == id) {
        return Err(format!("já existe rotina '{id}'"));
    }
    rs.push(r.clone());
    gravar(dados, &rs).map(|_| r)
}

/// Liga ou pausa uma rotina. So quando a pessoa pede.
pub fn ativar(dados: &Path, id: &str, ligado: bool) -> Result<Value, String> {
    let _g = trava();
    let mut rs = ler(dados)?;
    let r = rs.iter_mut().find(|r| s(r, "id") == id).ok_or(format!("rotina '{id}' não existe"))?;
    r["ativa"] = json!(ligado);
    let r = r.clone();
    gravar(dados, &rs).map(|_| r)
}

pub fn remover(dados: &Path, id: &str) -> Result<(), String> {
    let _g = trava();
    let mut rs = ler(dados)?;
    let antes = rs.len();
    rs.retain(|r| s(r, "id") != id);
    if rs.len() == antes {
        return Err(format!("rotina '{id}' não existe"));
    }
    gravar(dados, &rs)
}

pub fn resumo(dados: &Path) -> String {
    let rs = listar(dados);
    if rs.is_empty() {
        return "O Compasso ainda não tem rotinas guardadas.".into();
    }
    rs.iter().map(|r| {
        let dias: Vec<&str> = r["dias"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        format!("{} {} · {} {} · {}{}{}", if r["ativa"] == true { "🟢" } else { "⏸" }, s(r, "id"), s(r, "horario"), if dias.is_empty() { "todo dia".into() } else { dias.join("/") }, s(r, "titulo"),
            if s(r, "para").is_empty() { String::new() } else { format!(" → {}", s(r, "para")) }, if s(r, "ate").is_empty() { String::new() } else { format!(" (até {})", s(r, "ate")) })
    }).collect::<Vec<_>>().join("\n")
}

// ---------- protocolo de acionamento ----------

fn pedidos_hoje(dados: &Path, agora: &Agora) -> Vec<Value> {
    let hoje = agora.data.iso().replace('-', "");
    std::fs::read_to_string(dados.join("compasso").join("pedidos.jsonl")).unwrap_or_default().lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok()).filter(|p| s(p, "id").starts_with(&hoje)).collect()
}

fn anotar_pedido(dados: &Path, p: &Value) -> bool {
    let _ = std::fs::create_dir_all(dados.join("compasso"));
    std::fs::OpenOptions::new().create(true).append(true).open(dados.join("compasso").join("pedidos.jsonl")).and_then(|mut f| writeln!(f, "{p}")).is_ok()
}

/// Rotinas ativas cujo horario de hoje ja passou e que ainda nao rodaram hoje.
fn vencidas(dados: &Path, agora: &Agora) -> Vec<Value> {
    let hoje = agora.data.iso();
    listar(dados).into_iter().filter(|r| {
        let dias: Vec<&str> = r["dias"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        r["ativa"] == true && s(r, "ultima") != hoje && (s(r, "ate").is_empty() || hoje.as_str() <= s(r, "ate"))
            && (dias.is_empty() || dias.contains(&DIAS[agora.data.semana()])) && agora.hm().as_str() >= s(r, "horario")
    }).collect()
}

/// Passo 2: o pedido ao Jarvis. Texto = uma das variacoes aprovadas (roda pelo dia do ano), nunca gerado por IA.
fn montar_pedido(r: &Value, agora: &Agora) -> Value {
    let ts = textos(r);
    let texto = if ts.is_empty() { "" } else { ts[agora.data.dia_do_ano() as usize % ts.len()] };
    json!({ "id": format!("{}-{}", agora.carimbo(), s(r, "id")), "de": "Compasso", "para_agente": "Jarvis", "rotina": s(r, "id"), "acao": acao(r),
            "contato": Some(s(r, "para")).filter(|x| !x.is_empty()).unwrap_or(DONO), "texto": texto, "status": "pedido", "complemento": s(r, "complemento") })
}

/// Passo 3: o Jarvis confere o pedido. None = pode executar; senao, o motivo da recusa.
fn checar(dados: &Path, p: &Value, r: &Value, agora: &Agora) -> Option<String> {
    if let Err(e) = validar(dados, r) {
        return Some(e);
    }
    if r["ativa"] != true {
        return Some("rotina desativada".into());
    }
    if !(hora_min(acao(r))..=HORA_MAX).contains(&agora.hora) {
        return Some(FORA_DE_HORA.into());
    }
    if s(p, "acao") != acao(r) || !textos(r).contains(&s(p, "texto")) {
        return Some("pedido diferente da rotina guardada".into());
    }
    let feitos: Vec<Value> = pedidos_hoje(dados, agora).into_iter().filter(|x| s(x, "status") == "feito").collect();
    if feitos.iter().any(|x| s(x, "rotina") == s(r, "id")) {
        return Some("essa rotina já rodou hoje".into());
    }
    (feitos.len() >= TETO_DIARIO).then(|| format!("teto diário de {TETO_DIARIO} execuções atingido"))
}

/// Passo 4: SO a acao do pedido.
fn executar(dados: &Path, p: &Value, enviar: Enviar) -> String {
    let nome = contatos(dados).into_iter().find(|(k, _)| *k == norm(s(p, "contato"))).map(|(_, n)| n).unwrap_or_else(|| DONO.into());
    match s(p, "acao") {
        "perguntar_contato" => enviar(&nome, s(p, "texto")),
        // shortcut: sem voz portada o audio vai por escrito (no Python esse ja e o plano B quando o audio falha); trocar quando voz.rs existir
        "audio_contato" => enviar(&nome, &[s(p, "texto"), s(p, "complemento")].iter().filter(|x| !x.is_empty()).copied().collect::<Vec<_>>().join("\n\n")),
        "lembrar_dono" => enviar(DONO, &format!("💗 Lembrete do Compasso: {}", s(p, "texto"))),
        _ => "acao desconhecida".into(),
    }
}

/// Chamado pelo relogio do servidor (1 min): lembretes da agenda e rotinas vencidas. Devolve os pedidos processados.
pub fn tick(dados: &Path, agora: &Agora, enviar: Enviar) -> Vec<Value> {
    agenda::lembretes(&dados.join("agenda.json"), agora, &|t| enviar(DONO, t));
    let (hoje, mut feitos) = (agora.data.iso(), Vec::new());
    for r in vencidas(dados, agora) {
        let mut p = montar_pedido(&r, agora);
        match checar(dados, &p, &r, agora) {
            // tenta de novo no proximo minuto dentro da janela, sem poluir o registro
            Some(m) if m == FORA_DE_HORA => continue,
            Some(m) => {
                p["status"] = json!("recusado");
                p["motivo"] = json!(m);
            }
            None => {
                let res = executar(dados, &p, enviar);
                p["status"] = json!(if res.starts_with('✅') { "feito" } else { "falhou" });
                p["resultado"] = json!(res.chars().take(200).collect::<String>());
            }
        }
        // sem conseguir registrar, a rotina fica para o proximo minuto (o registro e o que impede rodar duas vezes)
        if !anotar_pedido(dados, &p) {
            continue;
        }
        let falhas = pedidos_hoje(dados, agora).iter().filter(|x| s(x, "rotina") == s(&r, "id") && s(x, "status") == "falhou").count();
        // "falhou" tenta de novo, no maximo 3 vezes no dia
        if s(&p, "status") != "falhou" || falhas >= 3 {
            let _g = trava();
            if let Ok(mut rs) = ler(dados) {
                if let Some(x) = rs.iter_mut().find(|x| s(x, "id") == s(&r, "id")) {
                    x["ultima"] = json!(hoje);
                }
                let _ = gravar(dados, &rs);
            }
        }
        feitos.push(p);
    }
    feitos
}

/// Ferramenta do Jarvis: listar, ativar e pausar. Criar rotina e pela tela (a pessoa confere os textos).
pub fn ferramenta(dados: &Path, acao: &str, alvo: &str) -> String {
    match acao {
        "ativar" | "pausar" => match ativar(dados, alvo, acao == "ativar") {
            Ok(r) => format!("{} a rotina «{}» ({}).", if acao == "ativar" { "🟢 Ativei" } else { "⏸ Pausei" }, s(&r, "titulo"), s(&r, "horario")),
            Err(e) => format!("🚫 {e}"),
        },
        _ => format!("💗 Rotinas do Compasso:\n{}", resumo(dados)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;
    use std::cell::RefCell;

    fn pasta(nome: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("braniac-compasso-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(p.join("pessoal.json"), r#"{"contatos": {"Tia Lúcia": "Lucia Zap"}}"#).unwrap();
        p
    }

    // terca, 17/11/2026
    fn as_(h: u32, m: u32) -> Agora {
        Agora { data: Data::civil(2026, 11, 17).unwrap(), hora: h, min: m, seg: 0 }
    }

    fn rotina() -> Value {
        json!({"id": "Braço da Tia", "titulo": "Perguntar do braço", "acao": "perguntar_contato", "para": "tia lucia", "horario": "09:00", "textos": ["Oi tia, e o braço?", "Tia, melhorou?"]})
    }

    #[test]
    fn recusa_rotina_fora_do_protocolo() {
        let d = pasta("validar");
        let com = |k: &str, v: Value| { let mut r = rotina(); r[k] = v; criar(&d, &r, &as_(8, 0)).unwrap_err() };
        assert!(com("acao", json!("pagar_boleto")).contains("fora da lista"));
        assert!(com("para", json!("um lead qualquer")).contains("não pode receber rotina"));
        assert!(com("horario", json!("9h")).contains("HH:MM"));
        assert!(com("horario", json!("05:00")).contains("fora da janela 08h-21h"));
        assert!(com("dias", json!(["segunda"])).contains("dias válidos"));
        assert!(com("textos", json!([])).contains("pelo menos um texto"));
        assert!(com("textos", json!(["me passa o código do cartão"])).contains("não pode falar"));
        assert!(com("textos", json!(["a".repeat(501)])).contains("muito longo"));
        assert!(listar(&d).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn rotina_nasce_pausada_roda_uma_vez_por_dia_e_insiste_ate_tres_falhas() {
        let d = pasta("ciclo");
        let enviados = RefCell::new(Vec::new());
        let ok = |para: &str, t: &str| { enviados.borrow_mut().push(format!("{para}: {t}")); "✅ foi".to_string() };
        let r = criar(&d, &rotina(), &as_(8, 0)).unwrap();
        assert_eq!((s(&r, "id"), &r["ativa"]), ("braco-da-tia", &json!(false)));
        assert!(criar(&d, &rotina(), &as_(8, 0)).unwrap_err().contains("já existe"));
        assert!(tick(&d, &as_(9, 5), &ok).is_empty()); // pausada
        assert!(ferramenta(&d, "listar", "").contains("⏸ braco-da-tia · 09:00 todo dia · Perguntar do braço → tia lucia"));
        assert_eq!(ferramenta(&d, "ativar", "braco-da-tia"), "🟢 Ativei a rotina «Perguntar do braço» (09:00).");
        assert!(ferramenta(&d, "ativar", "nao-existe").starts_with("🚫"));
        assert!(tick(&d, &as_(8, 59), &ok).is_empty()); // ainda nao deu a hora
        // envio falhando: registra "falhou" e tenta de novo; na 3a desiste por hoje
        for n in 1..=4 {
            let p = tick(&d, &as_(9, n), &|_, _| "sem conexao".into());
            assert_eq!(p.len(), usize::from(n <= 3), "tentativa {n}");
        }
        assert!(tick(&d, &as_(9, 30), &ok).is_empty());
        // dia seguinte: sai uma vez so, para o nome do contato, com o texto da vez
        let amanha = |h, m| Agora { data: Data::civil(2026, 11, 18).unwrap(), ..as_(h, m) };
        let p = tick(&d, &amanha(9, 0), &ok);
        assert_eq!((s(&p[0], "status"), s(&p[0], "rotina")), ("feito", "braco-da-tia"));
        assert!(tick(&d, &amanha(9, 1), &ok).is_empty() && tick(&d, &amanha(15, 0), &ok).is_empty());
        assert_eq!(enviados.borrow().len(), 1);
        assert!(enviados.borrow()[0].starts_with("Lucia Zap: ") && rotina()["textos"].as_array().unwrap().iter().any(|t| enviados.borrow()[0].ends_with(t.as_str().unwrap())));
        // rotina mexida a mao para um texto proibido e recusada na hora de executar
        let mut rs = ler(&d).unwrap();
        rs[0]["textos"] = json!(["manda o pix"]);
        gravar(&d, &rs).unwrap();
        let p = tick(&d, &Agora { data: Data::civil(2026, 11, 19).unwrap(), ..as_(9, 0) }, &ok);
        assert_eq!((s(&p[0], "status"), enviados.borrow().len()), ("recusado", 1));
        remover(&d, "braco-da-tia").unwrap();
        assert!(remover(&d, "braco-da-tia").is_err() && listar(&d).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn tick_tambem_manda_os_lembretes_da_agenda_ao_dono() {
        let d = pasta("agenda");
        agenda::ferramenta(&d.join("agenda.json"), &json!({"acao": "adicionar", "titulo": "Dentista", "quando": "hoje 14h"}), &as_(10, 0));
        let enviados = RefCell::new(Vec::new());
        tick(&d, &as_(13, 50), &|para, t| { enviados.borrow_mut().push(format!("{para}: {t}")); "✅".into() });
        tick(&d, &as_(13, 51), &|_, _| panic!("lembrete repetido"));
        assert_eq!(*enviados.borrow(), ["dono: ⏰ Lembrete: Dentista — daqui a 10 min (14:00)."]);
        let _ = std::fs::remove_dir_all(&d);
    }
}
