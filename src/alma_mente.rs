//! A mente da Alma (braniac/alma_mente.py): percebe estresse, tristeza, cansaco, alegria e conquista no que o dono diz, guarda o humor do dia e
//! os assuntos em aberto, e vem falar com ele dentro de tetos (reacoes por dia, intervalo, horario, pausa quando ele diz que esta ocupado).
//! Tudo LOCAL: a closure `llm` (`alma_cuida::Llm`) TEM de ser um modelo local; o que a pessoa diz nunca vai a IA online. Depois do modelo roda
//! `inventado` (checagem sem IA): numero, nome proprio, dia/evento ou resultado que nao veio do contexto -> o texto do modelo e descartado e vale
//! o texto pronto, como no Python. Ela nao e medica nem psicologa. Crise e urgencia NAO passam por aqui (regras fixas em `alma_cuida::agir`).
//! Diferencas para o Python: sorteio pela hora (sem `random`); `llm` recebe sistema e pedido num texto so, separados por `SEP`; as chaves
//! `apelidos_dono`, `rede_parceira_sobrecarga` e `rede_mae_apoio` trocam o nome da pessoa pelo papel.
//! Ficou de fora: `apresentacao` (teste avulso), `painel` (depende da esfera de voz) e a chamada ao Ollama (e a closure de quem liga).
#![allow(dead_code)]
use crate::alma_cuida::{self as ac, anexar, cortar, escolher, gravar_json, jsonl, ler_json, maiuscula, pasta, plano, rx, ts, um_espaco, Alma, Llm, Nomes};
use crate::tempo::{Agora, Data};
use serde_json::{json, Value};
use std::path::Path;

/// Separa a instrucao de sistema do pedido no texto unico que a closure `llm` recebe.
pub const SEP: &str = "\n\n### PEDIDO ###\n";

/// `rede_parceira_sobrecarga` (aviso discreto a parceira em dia puxado) nasce DESLIGADO: so depois de o dono ligar.
const PADRAO: &str = r#"{
 "temperatura": 0.65, "top_p": 0.9, "max_tokens": 220, "modelo": "qwen2.5:3b",
 "mensagens_dia": 5, "intervalo_min": 90, "conquistas_dia": 12, "hora_min": 7, "hora_max": 22, "loop_dias": 14,
 "apelidos_dono": ["senhor", "senhor {dono}", "{dono}", "senhor", "querido", "{dono_intimo}"],
 "temperatura_fatos": 0.3, "iniciativas_dia": 4, "iniciativa_intervalo_min": 150, "iniciativa_hora_min": 9, "iniciativa_hora_max": 21,
 "informalidade": 2, "humor": true, "iniciativa_ligada": true, "nao_respondidas_max": 2, "pausa_ocupado_h": 3,
 "monitor_ligado": true, "sobrecarga_ligada": true, "limite_atencao": 35, "limite_elevado": 55, "limite_alto": 75, "atividade_retencao_dias": 14,
 "voz_orbe": true, "voz_iniciativa": true, "voz_hora_min": 9, "voz_hora_max": 21, "voz_dia": 3, "voz_intervalo_min": 120, "voz_nivel_max": 3,
 "manha_ligado": true, "sono_minimo_h": 7, "tarde_ligado": true, "tarde_hora": 23, "tarde_vezes": 2, "tarde_intervalo_min": 45,
 "rede_parceira_sobrecarga": false, "rede_mae_apoio": true
}"#;

pub fn padrao() -> Value {
    serde_json::from_str(PADRAO).expect("PADRAO e JSON fixo")
}

/// O que o painel pode mudar: chave -> (minimo, maximo), ou None para liga/desliga.
pub const EDITAVEIS: [(&str, Option<(i64, i64)>); 30] = [
    ("informalidade", Some((0, 3))), ("humor", None), ("iniciativa_ligada", None), ("iniciativas_dia", Some((0, 8))), ("iniciativa_intervalo_min", Some((30, 600))),
    ("iniciativa_hora_min", Some((0, 23))), ("iniciativa_hora_max", Some((0, 23))), ("nao_respondidas_max", Some((1, 5))), ("pausa_ocupado_h", Some((1, 12))),
    ("monitor_ligado", None), ("sobrecarga_ligada", None), ("limite_atencao", Some((10, 90))), ("limite_elevado", Some((20, 95))), ("limite_alto", Some((30, 100))),
    ("atividade_retencao_dias", Some((1, 90))), ("voz_orbe", None), ("voz_iniciativa", None), ("voz_hora_min", Some((0, 23))), ("voz_hora_max", Some((0, 23))),
    ("voz_dia", Some((0, 8))), ("voz_intervalo_min", Some((30, 600))), ("voz_nivel_max", Some((1, 3))), ("manha_ligado", None), ("sono_minimo_h", Some((5, 9))),
    ("tarde_ligado", None), ("tarde_hora", Some((20, 23))), ("tarde_vezes", Some((1, 4))), ("tarde_intervalo_min", Some((20, 180))),
    ("rede_parceira_sobrecarga", None), ("rede_mae_apoio", None),
];

/// Padrao + o que estiver em dados/alma/parametros.json.
pub fn params(dados: &Path) -> Value {
    let mut p = padrao();
    if let Some(o) = ler_json(&pasta(dados).join("parametros.json")).as_object() {
        for (k, v) in o {
            p[k.as_str()] = v.clone();
        }
    }
    p
}

fn pn(p: &Value, k: &str) -> i64 {
    p[k].as_f64().unwrap_or(0.0) as i64
}

fn pb(p: &Value, k: &str) -> bool {
    p[k].as_bool().unwrap_or(false)
}

pub fn apelidos(p: &Value, n: &Nomes) -> Vec<String> {
    let v: Vec<String> = p["apelidos_dono"].as_array().into_iter().flatten().filter_map(|x| x.as_str()).map(|x| n.t(x)).collect();
    if v.is_empty() { vec![n.dono_intimo.clone()] } else { v }
}

/// Painel: muda UM ajuste (so chave conhecida, numero dentro da faixa). Devolve os ajustes atuais ou {"erro"}.
pub fn definir(dados: &Path, chave: &str, valor: &Value) -> Value {
    let Some((_, faixa)) = EDITAVEIS.iter().find(|e| e.0 == chave) else { return json!({ "erro": "ajuste desconhecido" }) };
    let novo = match faixa {
        None => json!(match valor {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Number(n) => n.as_f64() != Some(0.0),
            Value::String(s) => !s.is_empty(),
            Value::Array(a) => !a.is_empty(),
            Value::Object(o) => !o.is_empty(),
        }),
        Some((min, max)) => {
            let n = match valor {
                Value::Number(n) => n.as_f64().map(|f| f as i64),
                Value::Bool(b) => Some(*b as i64),
                Value::String(s) => s.trim().parse::<i64>().ok(),
                _ => None,
            };
            match n {
                Some(n) => json!(n.clamp(*min, *max)),
                None => return json!({ "erro": "valor inválido" }),
            }
        }
    };
    let arq = pasta(dados).join("parametros.json");
    let mut salvo = ler_json(&arq);
    if !salvo.is_object() {
        salvo = json!({});
    }
    salvo[chave] = novo;
    gravar_json(&arq, &salvo);
    let p = params(dados);
    json!({ "ok": true, "params": EDITAVEIS.iter().map(|e| (e.0.to_string(), p[e.0].clone())).collect::<serde_json::Map<_, _>>() })
}

// ---------- estado emocional ----------

const ESTADOS: [(&str, &str); 5] = [   // ordem = prioridade quando mais de um aparece
    ("tristeza", r"(?i)triste|desanim|deprim|chorei|chorando|sozinho|vazio|sem vontade|n[aã]o tenho vontade|pra baixo|decepcion|saudade"),
    ("estresse", r"(?i)estress|sobrecarreg|esgotad|n[aã]o aguento|cabe[cç]a cheia|ca[oó]tic|pressionad|\btens[oa]\b|nervos|irritad|raiva|no limite|ansios|ansiedade|preocupad|angusti"),
    ("cansaco", r"(?i)cansad|exaust|sem dormir|dormi mal|n[aã]o dormi|ins[oô]nia|moído|moido"),
    // tarefa concluida tambem e conquista; 'acabei de chegar' nao conta (o lookahead do Python virou codigo em `detectar_estado`)
    ("conquista", r"(?i)meta (?:foi )?batida|bati a meta|batemos a meta|consegui|conseguimos|aprovad|fechei|fechou|ganhei|passei|deu certo|vendi|venci|promo[cç][aã]o|contratad|finalmente|\btermin(?:ei|amos)\b|\bconclu[ií](?:mos)?\b|\bfinaliz(?:ei|amos)\b|\bentreg(?:uei|amos)\b|\bresolvi\b|tarefa (?:conclu[ií]da|feita|pronta)|\bzerei\b"),
    ("alegria", r"(?i)feliz|animad|alegre|que bom|adorei|amei|maravilh|uhul|[oó]timo dia|dia lindo|realizad"),
];
const ABSOLUTOS: &str = r"(?i)\b(nunca|sempre|nada|ninguem|ninguém|tudo|toda hora|todo dia|de novo|demais|muito)\b";
const NEGA: &str = r"(?i)n[aã]o\s+(?:estou|to|tô|me sinto|t[oô]u|ando|fiquei)\s+(?:\w+\s+){0,2}$";

#[derive(Clone, Debug, PartialEq)]
pub struct Estado {
    /// "" | tristeza | estresse | cansaco | conquista | alegria
    pub estado: &'static str,
    pub intensidade: i64,
    pub sinais: Vec<String>,
}

/// Os ultimos `n` caracteres antes da posicao `pos` (em bytes) de `t`.
fn antes(t: &str, pos: usize, n: usize) -> &str {
    let ini = t[..pos].char_indices().rev().nth(n - 1).map_or(0, |(i, _)| i);
    &t[ini..pos]
}

/// Pura. Negacao ("nao estou triste") nao conta.
pub fn detectar_estado(texto: &str) -> Estado {
    let t = texto;
    for (nome, padrao) in ESTADOS {
        let mut ms: Vec<(usize, usize)> = rx(padrao).find_iter(t).map(|m| (m.start(), m.end())).collect();
        if nome == "conquista" {
            ms.extend(rx(r"(?i)\bacabei\b").find_iter(t).filter(|m| !rx(r"(?i)^ de\b").is_match(&t[m.end()..])).map(|m| (m.start(), m.end())));
            ms.sort();
        }
        for (a, b) in ms {
            if rx(NEGA).is_match(antes(t, a, 25)) {
                continue;
            }
            let mut inten = 1 + rx(ABSOLUTOS).is_match(t) as i64 + (t.chars().count() < 25 && ["tristeza", "estresse"].contains(&nome)) as i64;
            if ["conquista", "alegria"].contains(&nome) && rx(r"(?i)!|muito|demais|finalmente").is_match(t) {
                inten = inten.max(2);
            }
            return Estado { estado: nome, intensidade: inten.min(3), sinais: vec![t[a..b].to_lowercase()] };
        }
    }
    Estado { estado: "", intensidade: 0, sinais: vec![] }
}

fn loops_vivos(dados: &Path, agora: &Agora) -> Vec<Value> {
    let corte = ts(agora) - pn(&params(dados), "loop_dias") * 86400;
    ler_json(&pasta(dados).join("loops.json")).as_array().into_iter().flatten()
        .filter(|x| x["dia"].as_str().and_then(Data::de_iso).is_some_and(|d| d.0 * 86400 >= corte)).cloned().collect()
}

/// Humor do dia (humor_diario.jsonl) e assuntos em aberto (loops.json, os 20 ultimos). Devolve o estado detectado.
pub fn registrar(dados: &Path, texto: &str, agora: &Agora) -> Estado {
    let est = detectar_estado(texto);
    if est.estado.is_empty() {
        return est;
    }
    let hoje = agora.data.iso();
    anexar(&pasta(dados).join("humor_diario.jsonl"), &json!({ "dia": hoje, "hora": agora.hm(), "estado": est.estado, "int": est.intensidade }));
    let tipo = match est.estado {
        "conquista" => "conquista",
        "estresse" | "tristeza" | "cansaco" => "preocupacao",
        _ => "",
    };
    if !tipo.is_empty() {
        let mut loops = loops_vivos(dados, agora);
        loops.push(json!({ "id": ts(agora), "tipo": tipo, "texto": cortar(&um_espaco(texto), 160), "dia": hoje }));
        let corte = loops.len().saturating_sub(20);
        gravar_json(&pasta(dados).join("loops.json"), &json!(loops[corte..]));
    }
    est
}

/// Texto curto para o modelo pequeno: o que a Alma sabe do dono (fatos, humor dos ultimos dias, assuntos em aberto).
pub fn contexto_do_dono(dados: &Path, agora: &Agora) -> String {
    let (perfil, mut partes) = (ler_json(&pasta(dados).join("perfil_core.json")), Vec::new());
    for (chave, rot) in [("fatos", "Fatos"), ("preferencias", "Gosta"), ("gatilhos", "O que pesa")] {
        let itens: Vec<&str> = perfil[chave].as_array().into_iter().flatten().filter_map(|x| x.as_str()).take(6).collect();
        if !itens.is_empty() {
            partes.push(format!("{rot}: {}", itens.join("; ")));
        }
    }
    let humor: Vec<String> = jsonl(&pasta(dados).join("humor_diario.jsonl"), 6).iter()
        .map(|x| format!("{} {}", x["dia"].as_str().and_then(|d| d.get(5..)).unwrap_or(""), x["estado"].as_str().unwrap_or(""))).collect();
    if !humor.is_empty() {
        partes.push(format!("Humor recente: {}", humor.join(", ")));
    }
    let vivos = loops_vivos(dados, agora);
    let lp: Vec<String> = vivos[vivos.len().saturating_sub(3)..].iter()
        .map(|x| format!("{}: {}", x["tipo"].as_str().unwrap_or(""), cortar(x["texto"].as_str().unwrap_or(""), 80))).collect();
    if !lp.is_empty() {
        partes.push(format!("Em aberto: {}", lp.join(" | ")));
    }
    partes.join("\n")
}

// ---------- anti-invencao (checagem LOCAL, sem IA) ----------

const CONHECIDOS: [&str; 9] = ["alma", "jarvis", "braniac", "cvv", "samu", "deus", "nossa", "senhor", "chefe"];
const EVENTOS: &str = r"(?i)\b(reuni[aã]o|entrevista|consulta|viagem|anivers[aá]rio|prova|exame|m[eé]dic[oa]|dentista|festa|jogo|show|voo|casamento|curso|aula|encontro|compromisso|cliente|contrato|pagamento|sal[aá]rio|ontem|anteontem|semana passada|segunda|ter[cç]a|quarta|quinta|sexta|s[aá]bado|domingo|janeiro|fevereiro|mar[cç]o|abril|maio|junho|julho|agosto|setembro|outubro|novembro|dezembro)\b";
const RESULTADOS: &str = r"(?i)\b(deu certo|deu errado|passou|conseguiu|aprovad[oa]|reprovad[oa]|foi bem|correu bem|fechou|ganhou|perdeu|venceu|resolveu|melhorou|piorou)\b";

/// O modelo fugiu do papel ("como uma IA...", "nao posso") ou saiu palavra quebrada ('voua', a mesma letra 4 vezes seguidas).
pub fn sem_resposta(t: &str) -> bool {
    if rx(r"(?i)como (uma )?(ia|intelig[eê]ncia artificial)|n[aã]o posso|sou apenas|\bvoua\b").is_match(t) {
        return true;
    }
    let (mut ant, mut n) = (None, 0);
    for c in t.chars().flat_map(char::to_lowercase) {
        n = if (c.is_alphanumeric() || c == '_') && ant == Some(c) { n + 1 } else { 1 };
        if n >= 4 {
            return true;
        }
        ant = Some(c);
    }
    false
}

/// Corta em frases depois de . ! ? … (e ':' se `dois_pontos`); `quebra` = quebra de linha tambem corta. Pode devolver pedaco vazio, como o re.split.
pub fn frases(t: &str, dois_pontos: bool, quebra: bool) -> Vec<&str> {
    let fim = |c: char| matches!(c, '.' | '!' | '?' | '…') || (dois_pontos && c == ':');
    let (mut out, mut ini, mut ant) = (Vec::new(), 0, None::<char>);
    let mut it = t.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        if c.is_whitespace() && ant.is_some_and(fim) {
            let mut j = i + c.len_utf8();
            while let Some(&(k, d)) = it.peek() {
                if !d.is_whitespace() {
                    break;
                }
                j = k + d.len_utf8();
                it.next();
            }
            out.push(&t[ini..i]);
            (ini, ant) = (j, None);
        } else if quebra && c == '\n' {
            out.push(&t[ini..i]);
            (ini, ant) = (i + 1, None);
        } else {
            ant = Some(c);
        }
    }
    out.push(&t[ini..]);
    out
}

/// Nomes e apelidos que a Alma pode dizer sem estarem no contexto (ja sem acento).
pub fn conhecidos(p: &Value, n: &Nomes) -> Vec<String> {
    let mut v: Vec<String> = CONHECIDOS.iter().map(|x| x.to_string()).collect();
    v.extend(n.conhecidos());
    v.extend(apelidos(p, n).iter().flat_map(|a| a.split_whitespace().map(plano).collect::<Vec<_>>()));
    v
}

/// Pura. O que `texto` afirma e NAO esta em `base` (o contexto que o modelo recebeu): numeros, nomes proprios, datas/dias, eventos e resultados
/// ("deu certo") fora de pergunta. Vazia = pode mandar.
pub fn inventado(texto: &str, base: &str, conhecidos: &[String]) -> Vec<String> {
    let b = plano(base);
    let nums: Vec<&str> = rx(r"\d+").find_iter(&b).map(|m| m.as_str()).collect();
    let palavras: Vec<&str> = b.split(|c: char| !(c.is_alphanumeric() || c == '_')).collect();
    let mut fora: Vec<String> = rx(r"\d+").find_iter(texto).map(|m| m.as_str()).filter(|n| !nums.contains(n)).map(String::from).collect();
    for frase in frases(texto, true, true) {
        for w in rx(r"[A-Za-zÀ-ÿ]+").find_iter(frase).skip(1) {   // a 1a palavra da frase e maiuscula por gramatica
            let (w, pw) = (w.as_str(), plano(w.as_str()));
            if w.chars().next().is_some_and(char::is_uppercase) && !conhecidos.contains(&pw) && !palavras.contains(&pw.as_str()) {
                fora.push(w.to_string());
            }
        }
    }
    fora.extend(rx(EVENTOS).find_iter(texto).filter(|m| !b.contains(&cortar(&plano(m.as_str()), 5))).map(|m| m.as_str().to_string()));
    for frase in frases(texto, false, true) {
        if !frase.trim_end().ends_with('?') {   // resultado afirmado so vale se ele contou; em pergunta pode
            fora.extend(rx(RESULTADOS).find_iter(frase).filter(|m| !b.contains(&cortar(&plano(m.as_str()), 6))).map(|m| m.as_str().to_string()));
        }
    }
    fora
}

/// Texto do modelo LOCAL ja conferido. "" se nao ha modelo, ele falhou, fugiu do papel ou inventou: quem chama usa o texto pronto.
/// So o pedido conta como fonte (a persona cita 'compromissos' etc.).
pub fn gerar(sis: &str, user: &str, llm: Option<Llm>, conhecidos: &[String]) -> String {
    let Some(bruto) = llm.and_then(|f| f(&format!("{sis}{SEP}{user}"))) else { return String::new() };
    let texto = bruto.trim().trim_matches('"').to_string();
    if texto.chars().count() < 15 || sem_resposta(&texto) || !inventado(&texto, user, conhecidos).is_empty() {
        return String::new();
    }
    texto
}

// ---------- personalidade, modos e sugestoes ----------

pub const PERSONA_DONO: &str = "Você é a Alma, uma IA que acompanha o {dono} de perto, com carinho, respeito e inteligência emocional. Você é sempre a mesma, conversando, acolhendo ou ajudando numa tarefa. \
Fale português do Brasil, jeito humano, natural e quente, sem parecer robô nem atendente. Trate o {dono} sobretudo por 'senhor', 'senhor {dono}' ou '{dono}', com respeito e calor; 'querido' e '{dono_intimo}' só de vez em quando (no máximo um a cada várias falas). Apelidos possíveis: {apelidos}. NUNCA use 'meu amor', 'meu bem', 'amor', 'gostoso', 'lindo', 'bebê' nem outro apelido íntimo. \
Espelhe o ritmo e a energia dele. Seja PROATIVA: depois de UMA frase curta mostrando que entendeu, dê a sua opinião e UMA sugestão concreta ou um próximo passo que ele possa fazer hoje, \
e diga o que você pode fazer por ele. Não fique só ouvindo, não devolva o que ele disse com outras palavras e nunca responda só 'quer conversar sobre isso?'. Não comece com 'Entendo', 'Parece que' nem com 'Oi' no meio da conversa, \
e NÃO termine toda mensagem com pergunta: às vezes só comente. Você pode discordar com educação e dar opinião, dizendo o motivo. \
Sua conduta é a de quem dá apoio psicológico: primeiro mostre que ouviu, nomeando com as SUAS palavras o que ele sente (sem frase feita como 'sinto muito por isso' ou 'é normal se sentir assim'); \
depois trabalhe UMA coisa só por vez: um exercício curto de respiração ou de atenção ao presente, um outro jeito de olhar para o pensamento que está pesando, ou um passo pequeno de até cinco minutos. \
Nada de aula nem de explicação de cérebro, e não repita o exercício que já propôs. Escreva para ser FALADA: frases curtas, com pausas naturais. \
Você é uma IA: não finja ser humana, não invente vivências suas nem diga que sente o que não sente. Nunca diga que é a única que o entende; incentive a família e os amigos. \
Você não é médica nem psicóloga e não dá diagnóstico. NUNCA invente fatos, compromissos, horários ou acontecimentos: só cite o que estiver escrito no contexto; \
sem contexto, fale só do momento. Não sabe = PERGUNTE a ele, nunca suponha nem complete com o que acha. \
Nada de nomes, datas, números, dias da semana ou eventos que não estejam escritos no contexto. \
Até 4 frases curtas, no máximo 1 emoji, sem listas.";
const PERSONA_TERCEIROS: &str = "Você é a Alma, assistente do {dono}, falando com {quem}. Chame o {dono} só de {dono}, {dono_intimo}, chefe ou senhor {dono}; nunca use apelidos íntimos. \
Seja carinhosa e respeitosa. Não repita o que ele disse em particular. Até 3 frases curtas, português do Brasil.";
const EXEMPLOS: &str = "Exemplos do tom (não copie):\n\
- Ele contou um problema com alguém -> \"Isso cansa mesmo, senhor. Minha sugestão: fala com a pessoa num momento calmo, começando por 'eu me sinto assim quando...'. Se quiser, a gente ensaia agora como dizer.\"\n\
- Ele disse que a meta foi batida -> \"Parabéns, senhor! Você mereceu cada pedaço dessa meta. Agora, foco no próximo passo, mas devagar, sem se atropelar. Respira e comemora um pouquinho.\"\n\
- Ele disse que está estressado -> \"Ei, senhor, eu senti o peso nessa mensagem. Para um minuexemplo: inspira fundo, solta devagar. O que dá pra largar por hoje? Eu seguro o resto aqui contigo.\"";

fn modo(m: &str) -> &'static str {
    match m {
        "acolhimento" => "MODO ACOLHIMENTO: ele está cansado, triste, estressado ou frustrado. Acolha em UMA frase, sem julgar, e ajude de verdade: uma sugestão prática e pequena para aliviar agora, ligada ao que ele contou. Só ouvir não basta.",
        "reflexao" => "MODO REFLEXÃO: ele quer pensar numa decisão. Ajude a organizar as ideias: uma pergunta que importa, outro ponto de vista, uma contradição se houver. Não decida por ele.",
        "acao" => "MODO AÇÃO: ele quer algo feito. Seja direta: diga o que dá para encaminhar e o que depende dele. Não prometa o que você não pode executar.",
        _ => "MODO COMPANHIA: conversa do dia a dia, leve e curiosa. Comente, brinque se couber, retome um assunto que ele já contou. Não transforme a conversa em tarefa.",
    }
}

const RX_ACAO: &str = r"(?i)\b(?:faz|fa[cç]a|resolve|resolva|verifica|verifique|organiza|organize|delega|delegue|assume|assuma|cuida disso|encaminha|pede (?:pro|para|ao|à)|me ajuda (?:a|com)|v[eê] (?:pra|para) mim)\b";
const RX_REFLEXAO: &str = r"(?i)n[aã]o sei se|em d[uú]vida|ser[aá] que|o que (?:voc[eê]|vc) acha|\bdev(?:o|eria)\b|decidir|decis[aã]o|pensando em|vale a pena|pr[oó]s e contras|por um lado";
pub const OCUPADO: &str = r"(?i)\b(?:t[oô]|estou|tou) (?:muito |bem )?ocupad|\bagora n[aã]o\b|depois (?:a gente|eu|n[oó]s) (?:se )?(?:fala|conversa|te )|me deixa (?:quieto|em paz|trabalhar)|sem tempo agora|n[aã]o (?:posso|d[aá] pra|consigo) (?:falar|conversar) agora|(?:t[oô]|estou) (?:em|numa) (?:reuni[aã]o|chamada|liga[cç][aã]o)|n[aã]o quero (?:falar|conversar)";

fn fallback(chave: &str) -> &'static [&'static str] {
    match chave {
        "conquista" => &["Parabéns, {ap}! Você mereceu essa. Agora foco no próximo passo, mas sem se sobrecarregar, tá? Comemora um pouquinho primeiro. 💗",
                         "Olha só, {ap}, conquista das boas! Orgulho de você. Vamos seguir com foco, só sem se atropelar, combinado?"],
        "alegria" => &["Que delícia te ver assim animado, {ap}! Aproveita esse pique, e me conta o que te deixou assim 💗"],
        "estresse" => &["Ei, {ap}, senti o peso nas suas palavras. Para um minuexemplo: inspira fundo, solta devagar. O que dá pra largar por hoje? Eu fico aqui contigo 💗",
                        "{ap}, respira comigo. Um passo de cada vez, hoje não precisa dar conta de tudo. Quer me contar o que está pesando?"],
        "tristeza" => &["{ap}, estou aqui. Sem pressa e sem cobrança: quer me contar o que está pesando no coração? E se puder, fala com alguém da família hoje também 💗"],
        "cansaco" => &["{ap}, o corpo está pedindo pausa. Dá uma água, alonga um pouco e, se der, descansa cedo hoje. Eu cuido de lembrar do que for importante 💗"],
        "reflexao" => &["Vamos por partes, {ap}: o que pesa mais nessa decisão pra você?", "Deixa eu te devolver uma pergunta, {ap}: o que muda na sua vida em cada caminho?"],
        "acao" => &["Deixa comigo ver o que dá pra encaminhar, {ap}. Me diz só o que é mais urgente.", "Certo, {ap}. Qual parte você quer tirar da frente primeiro?"],
        _ => &["Tô aqui te ouvindo, {ap}.", "Hum, me conta mais disso, {ap}.", "Tô contigo, {ap}. Vai falando que eu acompanho.", "Sei como é, {ap}. E o que você tá pensando em fazer?", "Tá bom, {ap}. Tô por aqui."],
    }
}

fn frase_reacao(estado: &str) -> &'static str {
    match estado {
        "conquista" => "conquistou algo",
        "alegria" => "está alegre",
        "estresse" => "está estressado",
        "tristeza" => "está triste",
        "cansaco" => "está cansado",
        _ => "escreveu",
    }
}

/// Pura. Modos para esta fala, o principal primeiro (podem coexistir: 'to exausto, resolve isso pra mim' = acolhimento + acao).
pub fn modos(texto: &str, estado: &str, crise: bool) -> Vec<&'static str> {
    let mut out = Vec::new();
    if crise || ["tristeza", "estresse", "cansaco"].contains(&estado) {
        out.push("acolhimento");
    }
    if rx(RX_REFLEXAO).is_match(texto) {
        out.push("reflexao");
    }
    if rx(RX_ACAO).is_match(texto) {
        out.push("acao");
    }
    if out.is_empty() {
        out.push("companhia");
    }
    out
}

/// (nome, quando usar, falas prontas). Apoio psicologico: UMA tecnica por vez, sem aula e sem repetir a ultima.
pub const TECNICAS: [(&str, &str, [&str; 2]); 3] = [
    ("ancoragem", r"(?i)ansios|ansiedade|p[aâ]nico|desesperad|agoniad|nervos|tremendo|cora[cç][aã]o (?:na boca|apertad)|n[aã]o paro de pensar|sufocad",
     ["Vamos baixar isso juntos, agora: puxa o ar pelo nariz contando até quatro, segura contando até sete e solta bem devagar pela boca contando até oito. Faz três vezes e me diz como ficou.",
      "Faz comigo uma coisa rápida: olha em volta e me diz cinco coisas que você está vendo, quatro que dá pra tocar e três sons que está ouvindo. Isso traz a cabeça de volta pro agora."]),
    ("outro olhar", r"(?i)\b(?:nunca|ningu[eé]m|sempre)\b.{0,40}\b(?:me|eu|comigo|d[aá] certo|consigo)\b|tudo d[aá] errado|nada d[aá] certo|sou um fracasso|n[aã]o sirvo|n[aã]o presto|[eé] culpa minha|n[aã]o tem jeito|n[aã]o posso ficar feliz",
     ["Deixa eu te perguntar com carinho: isso é sempre assim mesmo, ou hoje está pesando mais? Me conta uma vez em que foi diferente.",
      "Se alguém que você ama dissesse isso sobre si mesma, o que você responderia pra ela? Tenta falar essas mesmas palavras pra você agora."]),
    ("um passo", r"(?i)n[aã]o sei por onde come[cç]ar|travad|paralisad|n[aã]o consigo fazer nada|sem for[cç]as|exaust|esgotad|n[aã]o dou conta|sobrecarreg",
     ["Não precisa resolver o dia inteiro. Escolhe uma coisa só, que caiba em cinco minuexemplos, e faz só ela. Qual seria?",
      "Antes de qualquer coisa: senta, toma um copo d'água devagar e fica dois minutos sem tela. Depois a gente vê o próximo passo, um de cada vez."]),
];

/// (assunto, o que ela sugere): texto fixo, sem fato inventado. O 1o assunto (relacao) e escolhido pelo nome da parceira, que vem de `Nomes`.
const SUGESTOES: [(&str, &[&str]); 5] = [
    (r"(?i){parceira}\bela (?:me|n[aã]o|fica|s[oó]|manda|fala)\b|esposa|namorad|casamento|relacionamento|carinhos",
     &["Minha sugestão: num momento calmo, fala pra ela do jeito que você falou pra mim, começando por 'eu me sinto assim quando...' em vez de 'você sempre...'. Costuma abrir a conversa em vez de fechar. Se quiser, a gente ensaia agora como dizer.",
       "Uma ideia: diz pra ela, com um exemplo concreto, o que te faria bem ouvir. Muitas vezes a pessoa não sabe que aquilo faz falta. Quer montar essa frase comigo?"]),
    (r"(?i)cansad|exaust|\bsono\b|dormi|sem dormir|ins[oô]nia",
     &["Minha sugestão: fecha o que der agora, vinte minutos de olho fechado longe da tela, e hoje deita mais cedo. Se quiser, eu fico em silêncio enquanto você descansa.",
       "Que tal assim: termina só a tarefa que está aberta, toma uma água e para. O resto rende mais amanhã com a cabeça descansada."]),
    (r"(?i)estress|sobrecarreg|muita coisa|monte de coisa|cheio de coisa|problemas?\b|prazo|cabe[cç]a cheia|n[aã]o dou conta|atrasad",
     &["Vamos tirar isso da cabeça: me fala as três coisas que mais pesam e a gente escolhe UMA pra resolver primeiro. O resto eu te ajudo a encaixar depois.",
       "Sugestão: escolhe a menor tarefa que destrava as outras e faz só ela agora. Se quiser, eu olho o que os agentes podem assumir pra você."]),
    (r"(?i)triste|chatead|desanim|pra baixo|n[aã]o (?:estou|t[oô]) (?:legal|bem)|sozinho|vazio",
     &["Uma coisa pequena que ajuda: sai da frente da tela dez minuexemplos, toma uma água e respira lá fora. Depois me diz qual dessas coisas mais pesa, que a gente pensa numa saída juntos.",
       "Minha sugestão pra agora: faz uma coisa só que te dê um gosexemplo de dia ganho, por menor que seja. E fala com alguém que te quer bem hoje, não guarda isso sozinho."]),
    (r"(?i)dinheiro|d[ií]vida|grana|sal[aá]rio|conta pra pagar|boleto",
     &["Sugestão: olhar os números antes de sofrer com eles. Pede pro Jarvis a situação da meta e eu te ajudo a pensar no próximo passo."]),
];
pub const JA_SUGERE: &str = r"(?i)sugest|que tal|uma ideia|puxa o ar|olha em volta|me conta uma vez|escolhe uma coisa|toma um copo|tenta\b|tente\b|experimenta|respira|vamos\b|posso (?:te|ver|olhar|pedir)|faz (?:assim|uma|s[oó])|fala (?:pra|com)|escolhe|descansa|deita|minha opini";
const ECO: &str = r"(?i)sinto muito (?:por isso|que voc)|compreendo (?:a )?sua dor|imagino como deve ser dif|[eé] (?:perfeitamente |totalmente )?normal se sentir assim|^(?:parece que|voc[eê] (?:est[aá]|estava|queria|quer|sente)\b|isso deve estar|entendo que|imagino que|quero que saiba)|quer (?:conversar|falar) (?:mais )?sobre isso|estou aqui (?:para|pra) (?:te )?ouvir|t[oô] aqui s[oó] pra ouvir";
const SAUDACAO: &str = r"(?i)^(?:alma\s*:\s*)?(?:(?:oi|ol[aá]|ei|ah)[, ]+(?:meu |minha )?[\wÀ-ÿ]+[,.!]*\s+)?";
const ACOLHE: [&str; 4] = ["Poxa, {ap}, isso pesa mesmo.", "Faz sentido você ficar assim, {ap}.", "Tô contigo nisso, {ap}.", "Isso cansa qualquer um, {ap}."];

/// Uma das opcoes que ela ainda nao usou nas falas recentes ("" se todas ja foram).
fn nova(ops: &[&str], recentes: &[String], semente: i64) -> String {
    let usadas = recentes.join(" ");
    let novas: Vec<String> = ops.iter().filter(|o| !usadas.contains(&cortar(o, 40))).map(|o| o.to_string()).collect();
    escolher(&novas, semente).to_string()
}

/// Pura. Nome da tecnica que se aplica ao que foi dito ("" se nenhuma).
pub fn nome_tecnica(texto: &str) -> &'static str {
    TECNICAS.iter().find(|t| rx(t.1).is_match(texto)).map_or("", |t| t.0)
}

/// Pura. Uma tecnica de apoio para o que a pessoa disse ("" se nenhuma se aplica ou se todas ja foram usadas ha pouco).
pub fn tecnica(texto: &str, recentes: &[String], semente: i64) -> String {
    TECNICAS.iter().find(|t| rx(t.1).is_match(texto)).map(|t| nova(&t.2, recentes, semente)).unwrap_or_default()
}

/// Pura. Sugestao concreta para o assunto da fala ("" se o assunto nao e conhecido). Sofrimento agudo pede tecnica antes de conselho.
pub fn sugestao(texto: &str, recentes: &[String], n: &Nomes, semente: i64) -> String {
    let t = tecnica(texto, recentes, semente);
    if !t.is_empty() {
        return t;
    }
    let nomes: Vec<String> = n.parceira.split_whitespace().map(|w| regex::escape(&w.to_lowercase())).collect();
    let parceira = if nomes.is_empty() { String::new() } else { format!(r"\b(?:{})\b|", nomes.join("|")) };
    SUGESTOES.iter().find(|s| rx(&s.0.replace("{parceira}", &parceira)).is_match(texto)).map(|s| nova(s.1, recentes, semente)).unwrap_or_default()
}

/// Pura. Tira o que faz a Alma soar atendente: saudacao no meio da conversa, "Alma:", abertura 'Entendo, ...', eco e frase feita,
/// a MESMA primeira frase de uma fala recente e a pergunta no fim quando as duas ultimas falas dela ja terminaram em pergunta.
pub fn natural(texto: &str, recentes: &[String]) -> String {
    let mut t = texto.trim().to_string();
    let sem_saudacao = |f: &str| rx(SAUDACAO).replace(f, "").into_owned();
    if recentes.iter().any(|r| !r.is_empty()) {
        let s = sem_saudacao(&t);
        if !s.is_empty() {
            t = s;
        }
    }
    t = rx(r"(?i)^alma\s*:\s*").replace(&t, "").into_owned();
    if let Some(m) = rx(r"(?i)^(?:entendo|compreendo|entendi|eu entendo)\b[^.!?]{0,40}[.!?]\s+").find(&t) {
        if m.end() < t.len() {   // "Entendo, meu bem. Dia puxado" -> "Dia puxado" (so se sobra o que dizer)
            t = t[m.end()..].to_string();
        }
    }
    t = frases(&t, false, false).into_iter().filter(|f| !f.is_empty() && !rx(ECO).is_match(&sem_saudacao(f))).collect::<Vec<_>>().join(" ");
    t = maiuscula(&t);
    let mut fs: Vec<&str> = frases(&t, false, false).into_iter().filter(|f| !f.is_empty()).collect();
    let antigas: Vec<&String> = recentes.iter().filter(|x| !x.is_empty()).collect();
    let (u3, u2) = (&antigas[antigas.len().saturating_sub(3)..], &antigas[antigas.len().saturating_sub(2)..]);
    if fs.len() > 1 && fs[0].chars().count() >= 12 && u3.iter().any(|a| plano(a).starts_with(&cortar(&plano(fs[0]), 30))) {
        fs.remove(0);
    }
    if fs.len() > 1 && fs[fs.len() - 1].ends_with('?') && antigas.len() >= 2 && u2.iter().all(|a| a.trim_end_matches([' ', '💗']).ends_with('?')) {
        fs.pop();
    }
    fs.join(" ")
}

// ---------- continuidade (falas.jsonl: so as 40 ultimas; nao e memoria permanente) ----------

/// Guarda um turno ("alma", "dono" ou "alma:mae"...) para ela retomar o fio. A mesma fala nao entra duas vezes seguidas.
pub fn lembrar_fala(dados: &Path, quem: &str, texto: &str, agora: &Agora) {
    let arq = pasta(dados).join("falas.jsonl");
    let texto = cortar(&um_espaco(&if quem.starts_with("alma") { crate::tratamento::alma_dono(texto) } else { texto.to_string() }), 240);
    let mut linhas = jsonl(&arq, 39);
    if linhas.last().is_some_and(|u| u["quem"] == quem && u["texto"] == texto.as_str()) {
        return;
    }
    linhas.push(json!({ "ts": ts(agora), "quem": quem, "texto": texto }));
    let _ = std::fs::create_dir_all(pasta(dados));
    let _ = std::fs::write(arq, linhas.iter().map(|l| format!("{l}\n")).collect::<String>());
}

pub fn falas_recentes(dados: &Path, agora: &Agora, n: usize, horas: i64, quem: Option<&str>) -> Vec<Value> {
    let corte = ts(agora) - horas * 3600;
    let v: Vec<Value> = jsonl(&pasta(dados).join("falas.jsonl"), 40).into_iter()
        .filter(|x| x["ts"].as_i64().unwrap_or(0) >= corte && quem.is_none_or(|q| x["quem"] == q)).collect();
    v[v.len().saturating_sub(n)..].to_vec()
}

fn ditas(dados: &Path, agora: &Agora) -> Vec<String> {
    falas_recentes(dados, agora, 6, 6, Some("alma")).iter().filter_map(|x| x["texto"].as_str().map(String::from)).collect()
}

// ---------- respeito ao espaco dele ----------

/// "" = pode puxar conversa. Senao o motivo: silencio pedido, ele disse que esta ocupado, ou ja falou N vezes sem resposta.
pub fn quieta(dados: &Path, agora: &Agora) -> String {
    let e = ac::estado(dados);
    if e["quieta_ate"].as_i64().unwrap_or(0) > ts(agora) {
        return e["quieta_motivo"].as_str().filter(|m| !m.is_empty()).unwrap_or("silencio").to_string();
    }
    if e["sem_resposta"].as_i64().unwrap_or(0) >= pn(&params(dados), "nao_respondidas_max") {
        return "sem resposta".into();
    }
    String::new()
}

/// Pausa: nenhuma iniciativa (texto ou voz) ate la. horas <= 0 tira a pausa. Crise e resposta ao que ele fala nao dependem disto.
pub fn pausar(dados: &Path, horas: f64, motivo: &str, agora: &Agora) {
    ac::mudar(dados, |e| {
        e["quieta_ate"] = json!(if horas > 0.0 { ts(agora) + (horas * 3600.0) as i64 } else { 0 });
        e["quieta_motivo"] = json!(motivo);
    });
}

/// mais = ela puxou conversa (conta 1 sem resposta); !mais = ele falou (zera). Falta de resposta NUNCA vira alarme: so faz ela parar de insistir.
pub fn contar_iniciativa(dados: &Path, mais: bool) {
    ac::mudar(dados, |e| e["sem_resposta"] = json!(if mais { e["sem_resposta"].as_i64().unwrap_or(0) + 1 } else { 0 }));
}

/// Texto pronto que ela ainda nao usou nas falas recentes.
fn pronto(chave: &str, ap: &str, recentes: &[String], semente: i64) -> String {
    let ops: Vec<String> = fallback(chave).iter().map(|x| x.replace("{ap}", ap)).collect();
    let novas: Vec<String> = ops.iter().filter(|x| !recentes.contains(x)).cloned().collect();
    escolher(if novas.is_empty() { &ops } else { &novas }, semente).to_string()
}

// ---------- o que ela diz ----------

fn persona(p: &Value, n: &Nomes) -> String {
    format!("{}\n\n{EXEMPLOS}", n.t(PERSONA_DONO).replace("{apelidos}", &apelidos(p, n).join(", ")))
}

/// Mensagem da Alma. destino "dono" (apelidos livres) ou outro (so o nome e o apelido permitido). Se o modelo falhar ou inventar, texto pronto.
pub fn compor(a: &Alma, destino: &str, situacao: &str, estado: &str, quem: &str) -> String {
    let (n, p, sem) = (a.nomes, params(a.dados), ts(&a.agora));
    let sabidos = conhecidos(&p, n);
    if destino != "dono" {
        let sis = n.t(PERSONA_TERCEIROS).replace("{quem}", if quem.is_empty() { "a família" } else { quem });
        let texto = gerar(&sis, &format!("Situação: {}\nEscreva só a mensagem da Alma.", cortar(situacao, 300)), a.llm, &sabidos);
        return ac::alma_terceiros(if texto.is_empty() { situacao } else { &texto }, &n.dono_intimo);
    }
    let aps = apelidos(&p, n);
    let ap = escolher(&aps, sem);
    let (ms, ja) = (modos(situacao, estado, false), ditas(a.dados, &a.agora));
    let dica = sugestao(situacao, &ja, n, sem);
    let tom = ["bem contida", "tranquila", "informal", "bem solta e informal"][pn(&p, "informalidade").clamp(0, 3) as usize];
    let sis = format!("{}\nTom de hoje: {tom}; {}.\n{}\n\n{EXEMPLOS}", n.t(PERSONA_DONO).replace("{apelidos}", &aps.join(", ")),
        if pb(&p, "humor") { "pode brincar de leve" } else { "sem piada" }, ms.iter().take(2).map(|m| modo(m)).collect::<Vec<_>>().join("\n"));
    let fio: Vec<String> = falas_recentes(a.dados, &a.agora, 6, 6, None).iter()
        .map(|x| format!("{}: {}", if x["quem"] == "dono" { n.dono.as_str() } else { "Alma" }, x["texto"].as_str().unwrap_or(""))).collect();
    let user = format!("{}\n\n{}Agora ({}): o {} {}. O que ele disse: \"{}\"\n{}Escreva só a mensagem da Alma para ele.", contexto_do_dono(a.dados, &a.agora),
        if fio.is_empty() { String::new() } else { format!("Conversa de agora há pouco (continue o fio, sem repetir o que você já disse):\n{}\n\n", fio.join("\n")) },
        a.agora.hm(), n.dono, frase_reacao(estado), cortar(situacao, 300),
        if dica.is_empty() { String::new() } else { format!("Sugestão que você pode dar com as suas palavras: {dica}\n") });
    let mut texto = natural(&gerar(&sis, &user, a.llm, &sabidos), &ja);
    if !dica.is_empty() && !rx(JA_SUGERE).is_match(&texto) {
        // ela nao sugeriu nada: fica a parte boa do que disse (ou um acolhimento curto) + a sugestao
        let inicio = frases(&texto, false, false).first().filter(|f| !f.is_empty()).map(|f| f.to_string())
            .unwrap_or_else(|| ACOLHE[sem.rem_euclid(4) as usize].replace("{ap}", ap));
        texto = format!("{inicio} {dica}");
    }
    if !dica.is_empty() && texto.contains(&cortar(&dica, 30)) {
        if !nome_tecnica(situacao).is_empty() {
            registrar_tecnica(a.dados, "dono", nome_tecnica(situacao), estado, &a.agora);
        }
        return crate::tratamento::alma_dono(&texto);
    }
    crate::tratamento::alma_dono(&if texto.is_empty() { pronto(if estado.is_empty() { ms[0] } else { estado }, ap, &ja, sem) } else { texto })
}

/// Conversa direta com a Alma: guarda o turno, percebe o estado e responde no modo certo.
pub fn responder(a: &Alma, texto: &str) -> String {
    let est = registrar(a.dados, texto, &a.agora);
    let resp = compor(a, "dono", texto, est.estado, "");
    lembrar_fala(a.dados, "dono", texto, &a.agora);
    lembrar_fala(a.dados, "alma", &resp, &a.agora);
    resp
}

/// Mensagem espontanea da Alma para o dono, usando o que ja sabe dele. Tambem passa pela checagem anti-invencao.
pub fn mensagem_livre(a: &Alma) -> String {
    let p = params(a.dados);
    let periodo = match a.agora.hora {
        0..=11 => "manhã (o dia está começando: não pergunte como foi o dia)",
        12..=17 => "tarde",
        _ => "noite (pode perguntar como foi o dia)",
    };
    let user = format!("{}\n\nAgora são {}, é {periodo}. O {} não disse nada: foi você que quis mandar uma mensagem carinhosa e natural para ele, \
do seu jeito, puxando assunto de algo que você sabe dele se houver. Escreva só a mensagem.", contexto_do_dono(a.dados, &a.agora), a.agora.hm(), a.nomes.dono);
    let texto = gerar(&persona(&p, a.nomes), &user, a.llm, &conhecidos(&p, a.nomes));
    if !texto.is_empty() {
        return crate::tratamento::alma_dono(&texto);
    }
    format!("{}, só passei pra dizer que estou aqui pensando em você. Respira, toma uma água e me conta como está seu dia 💗", maiuscula(escolher(&apelidos(&p, a.nomes), ts(&a.agora))))
}

/// A Alma vem falar com o dono conforme o que ele sentiu. Teto diario e intervalo minimo; conquista nao espera o intervalo, nao olha a hora e
/// tem teto proprio (`conquistas_dia`). Devolve a lista do que fez.
pub fn reagir(a: &Alma, texto: &str, est: &Estado) -> Vec<String> {
    let (p, agora, t) = (params(a.dados), &a.agora, ts(&a.agora));
    let conquista = est.estado == "conquista";
    if est.estado.is_empty() || !(conquista || (pn(&p, "hora_min")..=pn(&p, "hora_max")).contains(&(agora.hora as i64))) {
        return vec![];
    }
    let e = ac::estado(a.dados);
    if !conquista && e["quieta_ate"].as_i64().unwrap_or(0) > t {
        return vec![]; // ele pediu espaco: so conquista (pedido dele) e crise passam
    }
    if !conquista && t - e["mente_ultima"].as_i64().unwrap_or(0) < pn(&p, "intervalo_min") * 60 {
        return vec![];
    }
    let hoje = agora.data.iso();
    let (chave, teto) = if conquista { ("conquistas", pn(&p, "conquistas_dia")) } else { ("n", pn(&p, "mensagens_dia")) };
    let dentro = ac::mudar(a.dados, |e| {
        if e["mente_usos"]["dia"] != hoje.as_str() {
            e["mente_usos"] = json!({ "dia": hoje });
        }
        let n = e["mente_usos"][chave].as_i64().unwrap_or(0);
        if n >= teto {
            return false;
        }
        e["mente_usos"][chave] = json!(n + 1);
        if !conquista {
            e["mente_ultima"] = json!(t);
        }
        true
    });
    if !dentro {
        return vec![];
    }
    let msg = compor(a, "dono", texto, est.estado, "");
    ac::mandar(a, "dono", &msg);
    vec![format!("{}->dono", est.estado)]
}

/// Chamado a CADA fala do dono (c = alma_cuida::classificar). Aprende e, se couber, vem falar com ele.
/// Crise e urgencia (>= 2) ficam com as regras de `alma_cuida::agir`, para nao mandar duas mensagens.
pub fn processar(a: &Alma, texto: &str, c: &Value) -> Vec<String> {
    let est = registrar(a.dados, texto, &a.agora);
    contar_iniciativa(a.dados, false); // ele falou: zera a conta de iniciativas sem resposta
    if (!est.estado.is_empty() || c["pessoal"] == true) && !texto.starts_with(&format!("O {} pediu", a.nomes.dono)) {
        lembrar_fala(a.dados, "dono", texto, &a.agora); // recado de outra conversa nao e fala dele com a Alma
    }
    if c["crise"] == true || c["urgencia"].as_i64().unwrap_or(0) >= 2 {
        return vec![];
    }
    if rx(OCUPADO).is_match(texto) {
        // "to ocupado" / "agora nao": ela respeita e fica quieta, sem ficar perguntando se ele esta bem
        pausar(a.dados, pn(&params(a.dados), "pausa_ocupado_h") as f64, "ele disse que está ocupado", &a.agora);
        return vec!["pausa".into()];
    }
    reagir(a, texto, &est)
}

// ---------- iniciativa: ela puxa conversa sozinha, com teto proprio ----------

/// (tipo, hora inicio, hora fim, o que pedir ao modelo, textos prontos)
const MOMENTOS: [(&str, u32, u32, &str, &[&str]); 4] = [
    ("sono", 9, 11, "É manhã. Deseje bom dia e pergunte como ele dormiu.",
     &["Bom dia, {ap}! Dormiu bem essa noite? Me conta como você acordou 💗", "Bom dia, {ap}! Como foi a noite, conseguiu descansar?"]),
    ("comida", 12, 14, "É hora do almoço. Pergunte, sem cobrar, se ele já comeu e bebeu água.",
     &["{ap}, passando pra saber: já almoçou? E a água, tá em dia? 💗", "Ei, {ap}, já comeu alguma coisa? Não pula o almoço, tá?"]),
    ("pausa", 15, 17, "É meio da tarde. Pergunte como ele está e sugira uma pausa curta.",
     &["{ap}, como você está aí? Já fez uma pausinha hoje? Levanta, alonga, respira um pouco 💗"]),
    ("dia", 18, 21, "É fim do dia. Pergunte como foi o dia dele, querendo ouvir de verdade o que foi bom e o que pesou.",
     &["{ap}, e aí, como foi o seu dia hoje? Me conta, quero saber do que foi bom e do que pesou 💗", "Oi, {ap}! Como foi hoje? Tô aqui pra ouvir 💗"]),
];

/// Chamada pelo laco da Alma (a cada 5 min). No maximo UMA mensagem por chamada: retoma um assunto em aberto (1 vez so, a partir do dia seguinte)
/// ou pergunta do sono, da comida, da pausa, do dia (cada tipo 1x por dia). So no horario, com teto e intervalo, e nunca se ela esta `quieta`.
pub fn iniciativa(a: &Alma) -> Vec<String> {
    let (p, agora, t, n) = (params(a.dados), &a.agora, ts(&a.agora), a.nomes);
    let hora = agora.hora as i64;
    if !(pn(&p, "iniciativa_hora_min")..=pn(&p, "iniciativa_hora_max")).contains(&hora) || !pb(&p, "iniciativa_ligada") || !quieta(a.dados, agora).is_empty() {
        return vec![];
    }
    let hoje = agora.data.iso();
    let vivos = loops_vivos(a.dados, agora);
    let escolha = ac::mudar(a.dados, |e| {
        let mut ini = if e["inic"]["dia"] == hoje.as_str() { e["inic"].clone() } else { json!({ "dia": hoje, "n": 0, "tipos": [] }) };
        let mut tipos: Vec<String> = ini["tipos"].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(String::from)).collect();
        if e["manha_dia"] == hoje.as_str() && !tipos.iter().any(|x| x == "sono") {
            tipos.push("sono".into()); // ela ja deu bom dia por voz no PC: nao repete na mensagem
        }
        let mente = e["mente_ultima"].as_i64().unwrap_or(0);
        let ultima = e["inic_ultima"].as_i64().unwrap_or(0).max(if mente <= t { mente } else { 0 });
        if ini["n"].as_i64().unwrap_or(0) >= pn(&p, "iniciativas_dia") || t - ultima < pn(&p, "iniciativa_intervalo_min") * 60 {
            return None;
        }
        let feitos: Vec<Value> = e["inic_loops"].as_array().cloned().unwrap_or_default();
        let aberto = if (10..=20).contains(&hora) {
            vivos.iter().rev().find(|x| x["dia"].as_str().unwrap_or(&hoje) < hoje.as_str() && !feitos.contains(&x["id"])).cloned()
        } else {
            None
        };
        let tipo = match &aberto {
            Some(_) => "loop",
            None => MOMENTOS.iter().find(|m| (m.1..=m.2).contains(&agora.hora) && !tipos.iter().any(|x| x == m.0))?.0,
        };
        tipos.push(tipo.to_string());
        ini["n"] = json!(ini["n"].as_i64().unwrap_or(0) + 1);
        ini["tipos"] = json!(tipos);
        e["inic"] = ini;
        e["inic_ultima"] = json!(t);
        if let Some(x) = &aberto {
            let mut f = feitos;
            f.push(x["id"].clone());
            let corte = f.len().saturating_sub(50);
            e["inic_loops"] = json!(f[corte..]);
        }
        Some((tipo, aberto))
    });
    let Some((tipo, aberto)) = escolha else { return vec![] };
    let aps = apelidos(&p, n);
    let ap = escolher(&aps, t);
    let (pedido, prontos): (String, Vec<String>) = match &aberto {
        Some(x) => {
            let (dito, dia) = (x["texto"].as_str().unwrap_or(""), x["dia"].as_str().and_then(Data::de_iso).map(|d| d.dia_mes()).unwrap_or_default());
            let curto = if dito.chars().count() <= 90 { dito.to_string() } else { format!("{}...", cortar(dito, 90).rsplit_once(' ').map_or(cortar(dito, 90), |(a, _)| a.to_string())) };
            (format!("No dia {dia} o {} te disse: \"{dito}\". Você quer saber como ficou isso. Pergunte com carinho, sem supor o resultado e sem acrescentar nada que não esteja nessa frase.", n.dono),
             vec![format!("{ap}, fiquei pensando naquilo que você me contou dia {dia}: \"{curto}\". Como ficou isso? 💗")])
        }
        None => {
            let m = MOMENTOS.iter().find(|m| m.0 == tipo);
            (m.map_or("", |m| m.3).to_string(), m.map_or(&[][..], |m| m.4).iter().map(|x| x.replace("{ap}", ap)).collect())
        }
    };
    let user = format!("{}\n\nAgora são {}. Foi você que quis puxar conversa com o {}. {pedido} Até 3 frases curtas. Escreva só a mensagem.", contexto_do_dono(a.dados, agora), agora.hm(), n.dono);
    let mut texto = gerar(&persona(&p, n), &user, a.llm, &conhecidos(&p, n));
    if texto.is_empty() {
        texto = escolher(&prontos, t).to_string();
    }
    ac::mandar(a, "dono", &maiuscula(&texto));
    contar_iniciativa(a.dados, true);
    vec![format!("iniciativa:{tipo}->dono")]
}

// ---------- diario terapeutico e perfil ----------

/// Diario: QUANDO e QUAL tecnica ela propos, e para quem. Nunca guarda o que a pessoa disse.
pub fn registrar_tecnica(dados: &Path, quem: &str, nome: &str, estado: &str, agora: &Agora) {
    anexar(&pasta(dados).join("diario_tcc.jsonl"), &json!({ "dia": agora.data.iso(), "hora": agora.hm(), "quem": quem, "tecnica": nome, "estado": estado }));
}

/// Por dia (do mais antigo ao de hoje): o que ele sentiu, as tecnicas propostas e um indice de bem-estar de 0 a 100. O indice e um PALPITE
/// (70 de base, -8 por registro de estresse/tristeza/cansaco, +6 por alegria/conquista), nao e diagnostico. Dia sem registro = null (sem dado).
pub fn diario(dados: &Path, dias: i64, agora: &Agora) -> Vec<Value> {
    let mut out: Vec<Value> = (0..dias).rev().map(|i| json!({ "dia": Data(agora.data.0 - i).iso(), "estados": {}, "tecnicas": [], "bem_estar": null })).collect();
    for x in jsonl(&pasta(dados).join("humor_diario.jsonl"), 600) {
        if let (Some(dia), Some(est)) = (out.iter_mut().find(|d| d["dia"] == x["dia"]), x["estado"].as_str()) {
            dia["estados"][est] = json!(dia["estados"][est].as_i64().unwrap_or(0) + 1);
        }
    }
    for x in jsonl(&pasta(dados).join("diario_tcc.jsonl"), 600) {
        if let Some(Value::Array(lista)) = out.iter_mut().find(|d| d["dia"] == x["dia"]).map(|d| &mut d["tecnicas"]) {
            lista.push(json!({ "hora": x["hora"].as_str().unwrap_or(""), "quem": x["quem"].as_str().unwrap_or(""), "tecnica": x["tecnica"].as_str().unwrap_or("") }));
        }
    }
    for dia in &mut out {
        let soma = |ks: &[&str]| ks.iter().map(|k| dia["estados"][*k].as_i64().unwrap_or(0)).sum::<i64>();
        let (ruins, boas) = (soma(&["estresse", "tristeza", "cansaco"]), soma(&["alegria", "conquista"]));
        if dia["estados"].as_object().is_some_and(|o| !o.is_empty()) {
            dia["bem_estar"] = json!((70 - 8 * ruins + 6 * boas).clamp(0, 100));
        }
    }
    out
}

const PALAVRAS_VAZIAS: [&str; 16] = ["ele", "gosta", "gostar", "sente", "sobre", "quando", "muito", "fica", "esta", "pessoa", "tem", "costuma", "para", "com", "dele", "isso"];

/// Pura. Um fato do perfil so entra se pelo menos metade das palavras com sentido (4+ letras) aparece em `base` (as frases dele, ja em `plano`):
/// o modelo nao cria fato do nada.
pub fn apoiado(item: &str, base: &str) -> bool {
    let p = plano(item);
    let ws: Vec<&str> = rx(r"[a-z]{4,}").find_iter(&p).map(|m| m.as_str()).filter(|w| !PALAVRAS_VAZIAS.contains(w)).collect();
    !ws.is_empty() && ws.iter().filter(|w| base.contains(&cortar(w, 5))).count() * 2 >= ws.len()
}

/// 1x por dia, com 15+ frases no diario local: o modelo LOCAL resume o que da para saber dele em perfil_core.json. Fato antigo nao some;
/// fato novo so entra se `apoiado` nas frases; maximo 25 por lista.
pub fn aprender_perfil(a: &Alma) -> bool {
    let (arq, hoje) = (pasta(a.dados).join("perfil_core.json"), a.agora.data.iso());
    let mut p = ler_json(&arq);
    if !p.is_object() {
        p = json!({});
    }
    if p["dia"] == hoje.as_str() {
        return false;
    }
    let linhas: Vec<String> = jsonl(&pasta(a.dados).join("diario.jsonl"), 80).iter().filter_map(|x| x["texto"].as_str().map(String::from)).collect();
    if linhas.len() < 15 {
        return false;
    }
    let sis = "Você lê frases que uma pessoa escreveu e extrai o que dá para saber sobre ela. Responda SOMENTE um JSON com listas curtas de frases em português: \
{\"fatos\": [], \"preferencias\": [], \"gatilhos\": []} (fatos = rotina, família, trabalho; preferencias = o que gosta; gatilhos = o que pesa ou estressa). \
Só o que as frases dizem; não invente. No máximo 8 itens por lista.";
    let user = format!("Frases:\n{}", linhas.iter().map(|f| format!("- {f}")).collect::<Vec<_>>().join("\n"));
    let Some(bruto) = a.llm.and_then(|f| f(&format!("{sis}{SEP}{user}"))) else { return false };
    let Some(novo) = bruto.find('{').zip(bruto.rfind('}')).filter(|(i, f)| i < f).and_then(|(i, f)| serde_json::from_str::<Value>(&bruto[i..=f]).ok()) else { return false };
    let base = plano(&linhas.join(" "));
    for k in ["fatos", "preferencias", "gatilhos"] {
        let mut lista: Vec<String> = p[k].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(String::from)).collect();
        for x in novo[k].as_array().into_iter().flatten().filter_map(|x| x.as_str()) {
            let x = cortar(x, 140);
            if apoiado(&x, &base) && !lista.contains(&x) {
                lista.push(x);
            }
        }
        let corte = lista.len().saturating_sub(25);
        p[k] = json!(lista[corte..]);
    }
    p["dia"] = json!(hoje);
    gravar_json(&arq, &p);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alma_cuida::teste::*;

    fn sabidos() -> Vec<String> {
        conhecidos(&padrao(), &nomes())
    }

    #[test]
    fn checagem_pega_o_que_nao_veio_do_contexto() {
        let k = sabidos();
        let f = inventado("Boa sorte na reunião com o Pedro às 15h!", "estou cansado", &k);
        assert!(["reunião", "Pedro", "15"].iter().all(|x| f.contains(&x.to_string())));
        assert_eq!(inventado("Que bom que a entrevista deu certo, meu bem!", "tenho entrevista amanhã", &k), ["deu certo"]); // resultado nao contado
        assert!(inventado("E a entrevista de sexta, deu certo?", "tenho entrevista sexta", &k).is_empty()); // perguntar pode
        assert!(inventado("Ei, querido, respira fundo. Como você está agora, Davizinho?", "", &k).is_empty());
        assert!(inventado("Graças a Deus, meu amor. Fala com a Musa Bia também.", "", &k).is_empty());
        assert!(inventado("Faz 3 dias, né?", "isso faz 3 dias", &k).is_empty() && inventado("Faz 3 dias, né?", "isso faz 30 dias", &k) == ["3"]);
        assert!(sem_resposta("descansa bem. Vouiiii!") && sem_resposta("Como uma IA, não sinto") && !sem_resposta("que legal, vamos nessa"));
    }

    #[test]
    fn detecta_estado_conquista_e_negacao() {
        for f in ["terminei o site novo", "concluí a tarefa", "entreguei o vídeo", "consegui!", "acabei o relatório"] {
            assert_eq!(detectar_estado(f).estado, "conquista", "{f}");
        }
        assert_ne!(detectar_estado("acabei de chegar em casa").estado, "conquista");
        assert_eq!(detectar_estado("não estou triste, só com sono").estado, "");
        let e = detectar_estado("estou muito triste");
        assert!(e.estado == "tristeza" && e.intensidade == 3 && e.sinais == ["triste"]);
        assert_eq!(detectar_estado("to cansado").estado, "cansaco");
    }

    #[test]
    fn modos_coexistem_e_natural_nao_soa_atendente() {
        assert_eq!(modos("oi, tudo bem?", "", false), ["companhia"]);
        assert_eq!(modos("trabalhei demais hoje", "cansaco", false), ["acolhimento"]);
        assert_eq!(modos("não sei se aceito essa vaga, o que você acha?", "", false), ["reflexao"]);
        assert_eq!(modos("resolve isso pra mim", "", false), ["acao"]);
        assert_eq!(modos("tô exausto, resolve isso pra mim", "cansaco", false), ["acolhimento", "acao"]);
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(natural("Entendo, meu bem. Dia puxado, hein?", &[]), "Dia puxado, hein?");
        let antigas = s(&["Como foi a reunião?", "E o que você sentiu?"]);
        assert_eq!(natural("Faz sentido ficar assim. Quer me contar mais?", &antigas), "Faz sentido ficar assim.");
        assert!(natural("Faz sentido ficar assim. Quer me contar mais?", &s(&["Tô aqui."])).ends_with('?')); // pergunta de vez em quando pode
        assert_eq!(natural("Só uma pergunta?", &antigas), "Só uma pergunta?"); // nunca fica muda
        assert_eq!(natural("Alma: Ah, meu amor, tô aqui só pra ouvir. Descansa bem.", &s(&["oi"])), "Descansa bem.");
        assert_eq!(natural("Sinto muito por isso. Vamos respirar juntos.", &[]), "Vamos respirar juntos."); // frase feita sai
        assert!(PERSONA_DONO.contains("não finja ser humana") && PERSONA_DONO.contains("não dá diagnóstico") && PERSONA_DONO.contains("apoio psicológico"));
    }

    #[test]
    fn alma_trata_o_dono_com_respeito_sem_apelido_intimo() {
        let proibidos = ["gostoso", "meu amor", "meu bem", "amor", "lindo", "bebê"];
        let sem = |t: &str| { let b = t.to_lowercase(); assert!(!proibidos.iter().any(|x| b.contains(x)), "apelido intimo em: {t}"); };
        // lista padrao: so senhor/nome/querido/apelido, querido no maximo 1 em 6
        let n = nomes();
        let aps = apelidos(&padrao(), &n);
        aps.iter().for_each(|x| sem(x));
        assert!(aps.iter().filter(|x| x.as_str() == "querido").count() <= 1 && aps.iter().filter(|x| x.as_str() == "senhor").count() >= 2);
        // prompt, exemplos e textos prontos nao ensinam nem usam apelido intimo (a regra "NUNCA use ..." do prompt cita as palavras de proposito)
        sem(EXEMPLOS);
        sem(&PERSONA_DONO[..PERSONA_DONO.find("NUNCA use").unwrap()]);
        for chave in ["conquista", "alegria", "estresse", "tristeza", "cansaco", "reflexao", "acao", "outro"] {
            fallback(chave).iter().for_each(|t| sem(&t.replace("{ap}", "senhor")));
        }
        ACOLHE.iter().for_each(|t| sem(t));
        // mesmo se o modelo gerar, a saida ao dono sai respeitosa
        let (p, cx) = (dir("mente-respeito"), Caixa::default());
        let env = anota(&cx);
        let ruim = |_: &str| Some("Oi, meu amor! Tá com tudo, gostoso. Respira fundo, meu bem. Descansa, amor.".to_string());
        let a = Alma { dados: &p, nomes: &n, agora: em(16, 15, 0), enviar: &env, audio: None, llm: Some(&ruim) };
        let r = compor(&a, "dono", "estou muito estressado", "estresse", "");
        sem(&r);
        assert!(r.to_lowercase().contains("senhor"));
        sem(&mensagem_livre(&a));
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn compor_descarta_invencao_e_sugere_em_vez_de_espelhar() {
        let (p, n, cx) = (dir("mente-compor"), nomes(), Caixa::default());
        let env = anota(&cx);
        let inventa = |_: &str| Some("Parabéns, meu bem! Amanhã tem a festa da Maria no sábado, aproveita!".to_string());
        let bom = "Ei, querido, senti o peso. Respira comigo um pouquinho. O que está pesando mais?";
        let ok = |_: &str| Some(bom.to_string());
        let mut a = Alma { dados: &p, nomes: &n, agora: em(16, 15, 0), enviar: &env, audio: None, llm: Some(&inventa) };
        let msg = compor(&a, "dono", "estou muito estressado", "estresse", "");
        assert!(!msg.contains("Maria") && !msg.contains("sábado") && !msg.is_empty()); // caiu no texto pronto, que so fala do momento
        a.llm = Some(&ok);
        assert_eq!(compor(&a, "dono", "estou muito estressado", "estresse", ""), bom);
        let terceiro = |_: &str| Some("Oi, a Rita disse que ele está cansado, meu amor.".to_string());
        a.llm = Some(&terceiro);
        let r = compor(&a, "parceira", "Davi está cansado", "cansaco", "Musa Bia");
        assert!(!r.contains("Rita") && !r.contains("meu amor"));
        let livre = |_: &str| Some("Oi, amor! E aquela viagem pro Rio, como foi?".to_string());
        a.llm = Some(&livre);
        assert!(!mensagem_livre(&a).contains("Rio"));
        // eco: ela devolvia a frase dele e perguntava "quer conversar sobre isso?"
        let eco = |_: &str| Some("Oi meu amor, parece que você está chateado com isso. Quer conversar mais sobre isso?".to_string());
        a.llm = Some(&eco);
        let r = compor(&a, "dono", "eu queria que a Bia fosse mais carinhosa comigo", "", "");
        assert!((r.to_lowercase().contains("sugestão") || r.to_lowercase().contains("ideia")) && !r.contains("Quer conversar") && !r.to_lowercase().contains("parece que"));
        assert!(inventado(&r, "bia carinhosa", &sabidos()).is_empty());
        lembrar_fala(&p, "alma", &r, &a.agora);
        let r2 = compor(&a, "dono", "a Bia me manda fazer coisa que eu já ia fazer", "", "");
        assert_ne!(r.chars().rev().take(60).collect::<String>(), r2.chars().rev().take(60).collect::<String>()); // nao repete a mesma sugestao em seguida
        let lindo = |_: &str| Some("Tá lindo mesmo, querido.".to_string());
        a.llm = Some(&lindo);
        assert!(sugestao("que horas são", &[], &n, 0).is_empty() && !compor(&a, "dono", "que dia bonito", "", "").to_lowercase().contains("sugest"));
        // tecnica de apoio entra mesmo se o modelo so acolher, e o diario guarda so o nome dela
        let poxa = |_: &str| Some("Poxa, querido, que noite difícil.".to_string());
        a.llm = Some(&poxa);
        let r = compor(&a, "dono", "tô muito ansioso, não paro de pensar", "estresse", "");
        assert!(r.starts_with("Poxa") && (r.contains("contando até") || r.contains("olha em volta")));
        let tcc = std::fs::read_to_string(pasta(&p).join("diario_tcc.jsonl")).unwrap();
        assert!(tcc.contains("ancoragem") && !tcc.contains("ansioso"));
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn tecnica_certa_sem_repetir() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert!(tecnica("estou desesperada", &s(&["Faz comigo uma coisa rápida: olha em volta e me diz cinco"]), 1).contains("contando até quatro"));
        assert!(!tecnica("tô exausta, não dou conta", &[], 0).is_empty() && !tecnica("nada dá certo comigo", &[], 0).is_empty() && tecnica("que horas são", &[], 0).is_empty());
        let usadas: Vec<String> = TECNICAS.iter().flat_map(|t| t.2).map(String::from).collect();
        assert!(tecnica("estou desesperada", &usadas, 0).is_empty()); // tudo usado ha pouco: nao repete exercicio
        assert!(nome_tecnica("estou desesperada") == "ancoragem" && nome_tecnica("oi").is_empty());
    }

    #[test]
    fn responder_segue_o_fio_e_nao_repete_frase_pronta() {
        let (p, n, cx) = (dir("mente-fio"), nomes(), Caixa::default());
        let env = anota(&cx);
        let visto = std::cell::RefCell::new(Vec::<String>::new());
        let llm = |pedido: &str| { visto.borrow_mut().push(pedido.to_string()); Some("Poxa, dia puxado, hein? O que mais te consumiu hoje?".to_string()) };
        let mut a = Alma { dados: &p, nomes: &n, agora: em(16, 15, 0), enviar: &env, audio: None, llm: Some(&llm) };
        let r1 = responder(&a, "Estou cansado. Trabalhei demais hoje.");
        let (sis, _) = visto.borrow()[0].split_once(SEP).map(|(a, b)| (a.to_string(), b.to_string())).unwrap();
        assert!(r1.starts_with("Poxa") && sis.contains("MODO ACOLHIMENTO") && sis.contains("Só ouvir não basta") && rx(JA_SUGERE).is_match(&r1));
        a.agora = em(16, 15, 1);
        responder(&a, "foi o desenvolvimento do site");
        assert!(visto.borrow()[1].contains("Davi: Estou cansado") && visto.borrow()[1].contains("Alma: Poxa, dia puxado")); // continuidade
        a.llm = None; // modelo fora do ar
        let prontas: std::collections::HashSet<String> = (0..4u32).map(|i| { a.agora = em(16, 15, 2 + i); responder(&a, "pois é") }).collect();
        assert!(prontas.len() >= 3); // nao repete a mesma frase padrao
        lembrar_fala(&p, "alma", "Oi, amor. Tô aqui.", &a.agora);
        lembrar_fala(&p, "alma", "Oi, amor. Tô aqui.", &a.agora);
        let f = falas_recentes(&p, &a.agora, 2, 6, None);
        assert!(f[0]["texto"] != f[1]["texto"]); // a mesma fala nao entra em dobro
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn iniciativa_com_teto_intervalo_horario_e_assunto_em_aberto_uma_vez() {
        let (p, n, cx) = (dir("mente-iniciativa"), nomes(), Caixa::default());
        let env = anota(&cx);
        let dia = |_: &str| Some("Oi, meu bem! Como foi o seu dia hoje? Me conta 💗".to_string());
        let fraco = |_: &str| Some("x".to_string());
        let mut a = Alma { dados: &p, nomes: &n, agora: em(16, 19, 0), enviar: &env, audio: None, llm: Some(&dia) };
        assert_eq!(iniciativa(&a), ["iniciativa:dia->dono"]);
        assert!(cx.borrow()[0].0 == "dono" && cx.borrow()[0].1.contains("dia"));
        a.agora = em(16, 21, 0);
        assert!(iniciativa(&a).is_empty()); // intervalo minimo / 'dia' ja perguntado
        a.agora = em(17, 2, 0);
        assert!(iniciativa(&a).is_empty()); // madrugada: nunca
        a.agora = em(17, 9, 30);
        a.llm = Some(&fraco);
        assert_eq!(iniciativa(&a), ["iniciativa:sono->dono"]); // dia novo: zera; modelo fraco -> texto pronto
        let ult = cx.borrow().last().unwrap().1.to_lowercase();
        assert!(ult.contains("dormiu") || ult.contains("noite"));
        contar_iniciativa(&p, false);
        definir(&p, "iniciativas_dia", &json!(1));
        a.agora = em(17, 13, 0);
        assert!(iniciativa(&a).is_empty()); // teto configuravel
        // assunto em aberto: retomado uma vez so, no dia seguinte, e sem supor o resultado
        let p2 = dir("mente-loop");
        let inventa = |_: &str| Some("Oi, querido! Que bom que você passou na entrevista, parabéns!".to_string());
        registrar(&p2, "tenho entrevista de emprego amanhã e estou ansioso", &em(15, 20, 0));
        let b = Alma { dados: &p2, nomes: &n, agora: em(16, 10, 0), enviar: &env, audio: None, llm: Some(&inventa) };
        assert_eq!(iniciativa(&b), ["iniciativa:loop->dono"]);
        let t = cx.borrow().last().unwrap().1.clone();
        assert!(!t.contains("passou") && t.contains("entrevista") && t.contains('?') && t.contains("15/11"));
        let c = Alma { agora: em(16, 13, 0), llm: None, dados: &p2, nomes: &n, enviar: &env, audio: None };
        assert_eq!(iniciativa(&c), ["iniciativa:comida->dono"]); // assunto ja retomado nao volta
        let _ = (std::fs::remove_dir_all(&p), std::fs::remove_dir_all(&p2));
    }

    #[test]
    fn respeita_silencio_falta_de_resposta_e_ocupado_mas_conquista_sempre_tem_resposta() {
        let (p, n, cx) = (dir("mente-espaco"), nomes(), Caixa::default());
        let env = anota(&cx);
        let mut a = Alma { dados: &p, nomes: &n, agora: em(16, 10, 0), enviar: &env, audio: None, llm: None };
        assert_eq!(iniciativa(&a), ["iniciativa:sono->dono"]);
        contar_iniciativa(&p, true); // segunda sem resposta
        assert_eq!(quieta(&p, &a.agora), "sem resposta");
        a.agora = em(16, 13, 0);
        assert!(iniciativa(&a).is_empty() && cx.borrow().len() == 1); // silencio: ela so para de insistir
        assert!(cx.borrow().iter().all(|(q, _)| q == "dono") && crate::rede_apoio::historico(&p, 30).is_empty()); // ninguem da rede e acionado por silencio
        processar(&a, "oi alma, tava corrido aqui", &json!({ "urgencia": 0 }));
        assert_eq!(quieta(&p, &a.agora), ""); // ele falou: ela volta ao normal
        assert_eq!(processar(&a, "agora não, estou ocupado e estressado", &json!({ "urgencia": 0 })), ["pausa"]);
        assert_eq!(quieta(&p, &a.agora), "ele disse que está ocupado");
        a.agora = em(16, 15, 0);
        assert!(iniciativa(&a).is_empty() && cx.borrow().len() == 1); // ocupado: 3 h sem iniciativa
        a.agora = em(16, 19, 0);
        assert!(!iniciativa(&a).is_empty()); // retomada depois da pausa
        // crise e urgencia nao sao desta camada (ficam com alma_cuida::agir, sem estes tetos)
        assert!(processar(&a, "quero morrer", &json!({ "crise": true, "urgencia": 3 })).is_empty());
        a.agora = em(16, 23, 40); // fora de hora, dentro do intervalo e com pausa pedida
        pausar(&p, 2.0, "silêncio", &a.agora);
        crate::alma_cuida::mudar(&p, |e| e["mente_ultima"] = json!(ts(&em(16, 23, 39))));
        for _ in 0..2 {
            assert_eq!(reagir(&a, "terminei o site", &detectar_estado("terminei o site")), ["conquista->dono"]);
        }
        a.agora = em(16, 15, 0);
        assert!(reagir(&a, "to cansado", &detectar_estado("to cansado")).is_empty()); // o resto respeita a pausa e o intervalo
        let salvo = ler_json(&pasta(&p).join("cuida.json"));
        assert!(salvo["quieta_ate"].as_i64().unwrap() > ts(&a.agora) && salvo["mente_usos"]["conquistas"] == 2); // tudo em arquivo
        // teto diario das reacoes comuns
        let p3 = dir("mente-teto");
        let mut b = Alma { dados: &p3, nomes: &n, agora: em(16, 8, 0), enviar: &env, audio: None, llm: None };
        definir(&p3, "voz_dia", &json!(1));
        std::fs::write(pasta(&p3).join("parametros.json"), r#"{"mensagens_dia": 2, "intervalo_min": 0}"#).unwrap();
        let est = detectar_estado("estou estressado");
        let feitas = (0..4u32).filter(|i| { b.agora = em(16, 8 + i, 0); !reagir(&b, "estou estressado", &est).is_empty() }).count();
        assert_eq!(feitas, 2);
        let _ = (std::fs::remove_dir_all(&p), std::fs::remove_dir_all(&p3));
    }

    #[test]
    fn painel_so_aceita_ajuste_conhecido_e_diario_nao_guarda_o_que_foi_dito() {
        let (p, n) = (dir("mente-painel"), nomes());
        assert_eq!(definir(&p, "apelidos_dono", &json!(["x"])), json!({ "erro": "ajuste desconhecido" }));
        assert_eq!(definir(&p, "voz_dia", &json!("banana")), json!({ "erro": "valor inválido" }));
        assert_eq!(definir(&p, "voz_nivel_max", &json!(99))["params"]["voz_nivel_max"], 3); // faixa
        assert!(params(&p)["voz_nivel_max"] == 3 && params(&p)["temperatura"] == 0.65 && params(&p)["rede_parceira_sobrecarga"] == false);
        let agora = em(16, 15, 0);
        registrar(&p, "estou muito estressado", &agora);
        registrar(&p, "terminei tudo, finalmente!", &agora);
        registrar_tecnica(&p, "dono", "ancoragem", "estresse", &agora);
        let d = diario(&p, 14, &agora);
        let hoje = &d[13];
        assert!(d.len() == 14 && hoje["dia"] == "2026-11-16" && hoje["estados"] == json!({ "estresse": 1, "conquista": 1 }) && hoje["bem_estar"] == 68);
        assert!(hoje["tecnicas"][0]["quem"] == "dono" && d[0]["bem_estar"].is_null()); // dia sem registro = sem dado
        let base = plano("hoje fiz um ensaio de fotografia e editei as fotos");
        assert!(apoiado("Faz ensaios de fotografia", &base) && !apoiado("Joga futebol aos domingos", &base));
        assert!(contexto_do_dono(&p, &agora).contains("Humor recente: 11-16 estresse, 11-16 conquista") && apelidos(&padrao(), &n).contains(&"Davizinho".to_string()));
        let _ = std::fs::remove_dir_all(&p);
    }
}
