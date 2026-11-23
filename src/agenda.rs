//! Agenda da pessoa: tarefas, compromissos e rotina. Porte de agenda.py, no mesmo formato de arquivo (dados/agenda.json).
//! Tudo local e sem IA no caminho: o modelo so chama a ferramenta com os campos; aqui se entende a data em portugues
//! ("amanha 14h", "15/10 9:30", "segunda 10h", ISO), a repeticao, os horarios livres e a hora de lembrar.
use crate::tempo::{Agora, Data};
use crate::texto_pt::{sem_acento, titulo_vazio};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

static TRAVA: Mutex<()> = Mutex::new(());
const DIAS_SEMANA: [&str; 7] = ["segunda", "terca", "quarta", "quinta", "sexta", "sabado", "domingo"];
const DIAS_BONITOS: [&str; 7] = ["segunda", "terça", "quarta", "quinta", "sexta", "sábado", "domingo"];
const MESES: [&str; 12] = ["jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez"];
const REPETICOES: [&str; 5] = ["", "diario", "dias_uteis", "semanal", "mensal"];
const DIA_COMECA: i64 = 8; // janela usada para achar horarios livres
const DIA_TERMINA: i64 = 22;
const LEMBRAR_PADRAO: i64 = 15; // minutos antes

fn trava() -> MutexGuard<'static, ()> {
    TRAVA.lock().unwrap_or_else(|e| e.into_inner())
}

/// Arquivo que nao existe = agenda vazia. Arquivo ilegivel = erro: gravar por cima apagaria a agenda inteira.
fn ler(arq: &Path) -> Result<Vec<Value>, String> {
    match std::fs::read_to_string(arq) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!("Não consegui ler a agenda: {e}")),
        Ok(t) => serde_json::from_str(&t).map_err(|_| "O arquivo da agenda está ilegível; não mexi em nada.".to_string()),
    }
}

fn gravar(arq: &Path, itens: &[Value]) -> bool {
    if let Some(p) = arq.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let tmp = arq.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(itens).unwrap_or_default()).and_then(|_| std::fs::rename(&tmp, arq)).is_ok()
}

const NAO_GRAVOU: &str = "Não consegui gravar a agenda; nada mudou.";

// ---------- datas em portugues ----------

fn palavra(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn pula(c: &[char], mut i: usize) -> usize {
    while i < c.len() && c[i] == ' ' {
        i += 1;
    }
    i
}

fn comeca(c: &[char], i: usize, s: &str) -> bool {
    i <= c.len() && c[i..].iter().copied().take(s.len()).eq(s.chars())
}

/// Numero de 1 ou 2 digitos que comeca em i, sem letra nem digito colado antes: (valor, onde termina).
fn numero(c: &[char], i: usize) -> Option<(u32, usize)> {
    if i >= c.len() || !c[i].is_ascii_digit() || (i > 0 && palavra(c[i - 1])) {
        return None;
    }
    let f = (i..c.len()).find(|&j| !c[j].is_ascii_digit()).unwrap_or(c.len());
    (f - i <= 2).then(|| (c[i..f].iter().fold(0, |n, d| n * 10 + d.to_digit(10).unwrap_or(0)), f))
}

/// Aplica "da tarde"/"da noite" e recusa hora que nao existe.
fn fecha(c: &[char], h: u32, m: u32, fim: usize) -> Option<(u32, u32)> {
    let j = pula(c, fim);
    let tarde = (comeca(c, j, "da ") || comeca(c, j, "de ")) && {
        let k = pula(c, j + 2);
        comeca(c, k, "tarde") || comeca(c, k, "noite")
    };
    let h = if tarde && h < 12 { h + 12 } else { h };
    (h < 24 && m < 60).then_some((h, m))
}

/// "14h", "14:30", "14h30", "10 horas", "as 8 da noite" -> (h, m). O texto ja vem sem acento.
fn hora(t: &str) -> Option<(u32, u32)> {
    let c: Vec<char> = t.chars().collect();
    for i in 0..c.len() {
        let Some((h, f)) = numero(&c, i) else { continue };
        let j = pula(&c, f);
        let (k, dois_pontos) = if comeca(&c, j, "horas") {
            (j + 5, false)
        } else if comeca(&c, j, "hora") {
            (j + 4, false)
        } else if comeca(&c, j, "h") {
            (j + 1, false)
        } else if comeca(&c, j, ":") {
            (j + 1, true)
        } else {
            continue;
        };
        let k2 = pula(&c, k);
        let dig = |x: usize| c.get(x).and_then(|d| d.to_digit(10));
        let min = match (dig(k2), dig(k2 + 1)) {
            (Some(a), Some(b)) if c.get(k2 + 2).map_or(true, |x| !palavra(*x)) => Some(a * 10 + b),
            _ => None,
        };
        match min {
            Some(m) => return fecha(&c, h, m, k2 + 2),
            // "14h" sozinho; "14hoje" ou "9:" solto nao sao hora
            None if !dois_pontos && c.get(k).map_or(true, |x| !x.is_alphabetic()) => return fecha(&c, h, 0, k),
            None => continue,
        }
    }
    for i in 0..c.len() {
        if comeca(&c, i, "as ") && (i == 0 || !palavra(c[i - 1])) {
            if let Some((h, f)) = numero(&c, pula(&c, i + 2)) {
                if c.get(f).map_or(true, |x| !palavra(*x)) {
                    return fecha(&c, h, 0, f);
                }
            }
        }
    }
    None
}

/// Exatamente n digitos a partir de i.
fn digitos(b: &[u8], i: usize, n: usize) -> Option<i64> {
    let s = b.get(i..i + n)?;
    (n > 0 && s.iter().all(u8::is_ascii_digit)).then(|| s.iter().fold(0, |a, d| a * 10 + (d - b'0') as i64))
}

/// "AAAA-MM-DD" com " HH:MM" opcional: (data, hora, onde termina).
fn iso(t: &str) -> Option<(Data, Option<(u32, u32)>, usize)> {
    let b = t.as_bytes();
    for i in 0..b.len() {
        let (Some(y), Some(m), Some(d)) = (digitos(b, i, 4), digitos(b, i + 5, 2), digitos(b, i + 8, 2)) else { continue };
        if b[i + 4] != b'-' || b[i + 7] != b'-' {
            continue;
        }
        let data = Data::civil(y, m, d)?;
        let hm = match (b.get(i + 10), digitos(b, i + 11, 2), b.get(i + 13), digitos(b, i + 14, 2)) {
            (Some(b' ' | b't'), Some(h), Some(b':'), Some(mi)) if h < 24 && mi < 60 => Some((h as u32, mi as u32)),
            _ => None,
        };
        return Some((data, hm, i + 10 + if hm.is_some() { 6 } else { 0 }));
    }
    None
}

/// "15/10" ou "15/10/2026": (dia, mes, ano).
fn barra(t: &str) -> Option<(i64, i64, Option<i64>)> {
    let b = t.as_bytes();
    let fim = |i: usize| (i..b.len()).find(|&j| !b[j].is_ascii_digit()).unwrap_or(b.len());
    for i in 0..b.len() {
        if !b[i].is_ascii_digit() || (i > 0 && b[i - 1].is_ascii_alphanumeric()) {
            continue;
        }
        let f = fim(i);
        if f - i > 2 || b.get(f) != Some(&b'/') {
            continue;
        }
        let f2 = fim(f + 1);
        if f2 == f + 1 || f2 - f - 1 > 2 {
            continue;
        }
        let ano = (b.get(f2) == Some(&b'/')).then(|| (f2 + 1, fim(f2 + 1))).filter(|(a, z)| (2..=4).contains(&(z - a))).and_then(|(a, z)| digitos(b, a, z - a));
        return Some((digitos(b, i, f - i)?, digitos(b, f + 1, f2 - f - 1)?, ano));
    }
    None
}

/// "20 de dezembro" ou "20 de dez de 2027": (dia, mes, ano).
fn por_extenso(p: &[&str]) -> Option<(i64, i64, Option<i64>)> {
    let num = |w: &str, max: usize| (w.len() <= max).then(|| digitos(w.as_bytes(), 0, w.len())).flatten();
    (0..p.len().saturating_sub(2)).find_map(|i| {
        let (dia, mes) = (num(p[i], 2)?, MESES.iter().position(|m| p[i + 2].starts_with(m))?);
        let ano = (p.get(i + 3) == Some(&"de")).then(|| p.get(i + 4).and_then(|a| num(a, 4))).flatten().filter(|a| *a > 999);
        (p[i + 1] == "de").then_some((dia, mes as i64 + 1, ano))
    })
}

/// "seg", "terca", "sextas" -> 0..6. So aceita o comeco do NOME do dia ("quando" e "terminar" nao contam).
fn dia_da_semana(w: &str) -> Option<usize> {
    let w = if w.len() > 4 { w.strip_suffix('s').unwrap_or(w) } else { w };
    (w.len() >= 3).then(|| DIAS_SEMANA.iter().position(|n| n.starts_with(w))).flatten()
}

/// Texto livre -> (data, hora). Sem data e sem hora = tarefa sem data.
pub fn interpretar_quando(texto: &str, agora: &Agora) -> (Option<Data>, Option<(u32, u32)>) {
    let t = sem_acento(texto);
    if t.is_empty() {
        return (None, None);
    }
    if let Some((d, hm, fim)) = iso(&t) {
        return (Some(d), hm.or_else(|| hora(&t[fim..])));
    }
    let h = hora(&t);
    let p: Vec<&str> = t.split(|c: char| !c.is_alphanumeric()).filter(|x| !x.is_empty()).collect();
    let (hoje, relogio) = (agora.data, (agora.hora, agora.min));
    let d = if let Some((dia, mes, ano)) = barra(&t).or_else(|| por_extenso(&p)) {
        let a = ano.map(|a| if a < 100 { a + 2000 } else { a }).unwrap_or(hoje.partes().0);
        // sem ano e ja passou: e a do ano que vem
        Data::civil(a, mes, dia).and_then(|d| if ano.is_none() && d < hoje { Data::civil(a + 1, mes, dia) } else { Some(d) })
    } else if p.windows(3).any(|w| w == ["depois", "de", "amanha"]) {
        Some(Data(hoje.0 + 2))
    } else if p.contains(&"amanha") {
        Some(Data(hoje.0 + 1))
    } else if p.contains(&"hoje") {
        Some(hoje)
    } else {
        p.iter().find_map(|w| dia_da_semana(w)).map(|i| {
            let mut delta = (i as i64 - hoje.semana() as i64).rem_euclid(7);
            if delta == 0 && h.map_or(true, |x| x <= relogio) {
                delta = 7;
            }
            Data(hoje.0 + delta)
        })
    };
    // so a hora: hoje se ainda nao passou, senao amanha
    (d.or_else(|| h.map(|x| if x > relogio { hoje } else { Data(hoje.0 + 1) })), h)
}

fn dia_de(texto: &str, agora: &Agora) -> Data {
    interpretar_quando(texto, agora).0.unwrap_or(agora.data)
}

// ---------- itens e ocorrencias ----------

fn s<'a>(i: &'a Value, k: &str) -> &'a str {
    i[k].as_str().unwrap_or("")
}

fn n(i: &Value, k: &str) -> i64 {
    i[k].as_i64().or_else(|| i[k].as_f64().map(|x| x as i64)).unwrap_or(0)
}

fn tem(i: &Value, k: &str, chave: &str) -> bool {
    i[k].as_array().is_some_and(|a| a.iter().any(|x| x == chave))
}

/// Poe a chave no fim da lista e guarda so as ultimas `max`.
fn anota(i: &mut Value, k: &str, chave: &str, max: usize) {
    let mut a: Vec<Value> = i[k].as_array().cloned().unwrap_or_default();
    a.push(json!(chave));
    let corte = a.len().saturating_sub(max);
    i[k] = json!(a[corte..]);
}

fn ocorre(item: &Value, d: Data) -> bool {
    let Some(ini) = Data::de_iso(s(item, "data")) else { return false };
    if d < ini || Data::de_iso(s(item, "ate")).is_some_and(|ate| d > ate) {
        return false;
    }
    match s(item, "repetir") {
        "diario" => true,
        "dias_uteis" => d.semana() < 5,
        "semanal" => d.semana() == ini.semana(),
        "mensal" => d.partes().2 == ini.partes().2,
        _ => d == ini,
    }
}

/// Posicoes dos itens que acontecem no dia d, por hora (dia inteiro primeiro).
fn ocorrencias(d: Data, itens: &[Value]) -> Vec<usize> {
    let mut oc: Vec<usize> = (0..itens.len()).filter(|&i| ocorre(&itens[i], d)).collect();
    oc.sort_by_key(|&i| s(&itens[i], "hora").to_string());
    oc
}

fn chave_feito(item: &Value, d: Data) -> String {
    if s(item, "data").is_empty() { "sem-data".into() } else { d.iso() }
}

fn feito(item: &Value, d: Data) -> bool {
    tem(item, "feitos", &chave_feito(item, d))
}

fn minutos(hhmm: &str) -> i64 {
    let mut p = hhmm.split(':').map(|x| x.parse::<i64>().unwrap_or(0));
    p.next().unwrap_or(0) * 60 + p.next().unwrap_or(0)
}

fn hm(m: i64) -> String {
    format!("{:02}:{:02}", m / 60, m % 60)
}

fn duracao_ou_30(i: &Value) -> i64 {
    match n(i, "duracao") {
        0 => 30,
        x => x.max(1),
    }
}

fn fmt(item: &Value, d: Option<Data>) -> String {
    let h = s(item, "hora");
    let fim = if !h.is_empty() && n(item, "duracao") > 0 { format!("–{}", hm((minutos(h) + n(item, "duracao")) % 1440)) } else { String::new() };
    let rep = match s(item, "repetir") {
        "diario" => " 🔁 todo dia",
        "dias_uteis" => " 🔁 dias úteis",
        "semanal" => " 🔁 toda semana",
        "mensal" => " 🔁 todo mês",
        _ => "",
    };
    let marca = if d.is_some_and(|d| feito(item, d)) { "✅" } else { "▫️" };
    let quando = if h.is_empty() { String::new() } else { format!("{h}{fim} ") };
    format!("{marca} {quando}{}{rep}  [{}]", s(item, "titulo"), s(item, "id"))
}

fn sobrepoe(a: &Value, b: &Value) -> bool {
    let (a0, b0) = (minutos(s(a, "hora")), minutos(s(b, "hora")));
    a0 < b0 + duracao_ou_30(b) && b0 < a0 + duracao_ou_30(a)
}

fn data_txt(d: Data, agora: &Agora) -> String {
    let base = format!("{} {}", DIAS_BONITOS[d.semana()], d.dia_mes());
    match d.0 - agora.data.0 {
        0 => format!("hoje ({base})"),
        1 => format!("amanhã ({base})"),
        -1 => format!("ontem ({base})"),
        _ => base,
    }
}

fn novo_id(itens: &[Value]) -> String {
    let mut x = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
    loop {
        let id = format!("{:05x}", (x ^ (x >> 20)) & 0xfffff);
        if !itens.iter().any(|i| s(i, "id") == id) {
            return id;
        }
        x = x.wrapping_add(7919);
    }
}

fn num(a: &Value, k: &str, padrao: i64) -> i64 {
    a[k].as_f64().or_else(|| a[k].as_str().and_then(|x| x.trim().parse().ok())).map(|x| x as i64).unwrap_or(padrao)
}

fn txt(a: &Value, k: &str) -> String {
    a[k].as_str().map(|x| x.trim().to_string()).or_else(|| a[k].as_f64().map(|x| x.to_string())).unwrap_or_default()
}

fn corta(t: &str, max: usize) -> String {
    t.chars().take(max).collect()
}

fn texto_repetir(r: &str) -> &'static str {
    match r {
        "diario" => ", todo dia",
        "dias_uteis" => ", dias úteis",
        "semanal" => ", toda semana",
        "mensal" => ", todo mês",
        _ => "",
    }
}

fn adicionar(arq: &Path, a: &Value, quando: &str, agora: &Agora) -> String {
    let titulo = txt(a, "titulo");
    if titulo.is_empty() {
        return "Faltou o que é o compromisso/tarefa.".into();
    }
    let rep = sem_acento(&txt(a, "repetir")).replace(' ', "_");
    let rep = match rep.as_str() {
        "todo_dia" | "diaria" | "diariamente" => "diario",
        "semanalmente" | "toda_semana" => "semanal",
        "mensalmente" | "todo_mes" => "mensal",
        "nao" | "nenhum" => "",
        x if x.starts_with("dias_ut") => "dias_uteis",
        x => x,
    };
    if !REPETICOES.contains(&rep) {
        return format!("Repetição '{rep}' não existe. Use: diario, dias_uteis, semanal ou mensal.");
    }
    let (d, h) = interpretar_quando(quando, agora);
    let d = d.or((!rep.is_empty()).then_some(agora.data));
    let hora_txt = h.map(|(h, m)| format!("{h:02}:{m:02}")).unwrap_or_default();
    let lembrar = num(a, "lembrar", LEMBRAR_PADRAO);
    let ate = txt(a, "ate");
    let _g = trava();
    let mut itens = match ler(arq) {
        Ok(i) => i,
        Err(e) => return e,
    };
    let item = json!({
        "id": novo_id(&itens), "titulo": corta(&titulo, 120), "data": d.map(Data::iso).unwrap_or_default(), "hora": hora_txt,
        "duracao": if h.is_some() { num(a, "duracao", 60).max(0) } else { 0 }, "repetir": rep, "lembrar": lembrar,
        "notas": corta(&txt(a, "notas"), 300), "ate": if ate.is_empty() { String::new() } else { dia_de(&ate, agora).iso() },
        "feitos": [], "avisos": [],
        "criado": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|x| x.as_secs_f64()).unwrap_or(0.0),
    });
    if let (Some(d), true, true) = (d, h.is_some(), rep.is_empty()) {
        // "me lembre antes" logo depois de marcar criava um 2o item: mesmo dia/hora e titulo parecido atualiza o que ja existe
        let novo = sem_acento(&titulo);
        let parecido = |o: &Value| {
            let velho = sem_acento(s(o, "titulo"));
            novo.split_whitespace().filter(|w| !["de", "da", "do", "para", "com"].contains(w)).any(|w| velho.split_whitespace().any(|v| v == w))
        };
        if let Some(o) = itens.iter_mut().find(|o| s(o, "data") == s(&item, "data") && s(o, "hora") == hora_txt && s(o, "repetir").is_empty() && parecido(o)) {
            if s(&item, "titulo").chars().count() > s(o, "titulo").chars().count() {
                o["titulo"] = item["titulo"].clone();
            }
            if n(&item, "duracao") > 0 {
                o["duracao"] = item["duracao"].clone();
            }
            o["lembrar"] = json!(lembrar);
            if !s(&item, "notas").is_empty() {
                o["notas"] = item["notas"].clone();
            }
            let fala = format!("📅 Atualizei o que já estava marcado: {} — {} às {hora_txt}. Te lembro {lembrar} min antes. [{}]", s(o, "titulo"), data_txt(d, agora), s(o, "id"));
            return if gravar(arq, &itens) { fala } else { NAO_GRAVOU.into() };
        }
    }
    let conflito: Vec<String> = match d {
        Some(d) if h.is_some() => ocorrencias(d, &itens).into_iter().map(|i| &itens[i]).filter(|o| !s(o, "hora").is_empty() && sobrepoe(o, &item)).map(|o| s(o, "titulo").to_string()).collect(),
        _ => Vec::new(),
    };
    let id = s(&item, "id").to_string();
    itens.push(item);
    if !gravar(arq, &itens) {
        return NAO_GRAVOU.into();
    }
    let quando_txt = match d {
        Some(d) if h.is_some() => format!("{} às {hora_txt}", data_txt(d, agora)),
        Some(d) => format!("{} (dia todo)", data_txt(d, agora)),
        None => "sem data (lista de tarefas)".into(),
    };
    let aviso = if conflito.is_empty() { String::new() } else { format!(" ⚠️ Bate com: {}.", conflito.join(", ")) };
    let lembrete = if h.is_some() && lembrar > 0 { format!(" Te lembro {lembrar} min antes.") } else { String::new() };
    format!("📅 Anotado: {titulo} — {quando_txt}{}.{lembrete}{aviso} [{id}]", texto_repetir(rep))
}

/// Pelo id entre colchetes ou por palavras do titulo.
fn achar(alvo: &str, itens: &[Value]) -> Vec<usize> {
    let a = sem_acento(alvo);
    let a = a.trim_matches(|c| c == '[' || c == ']');
    if a.is_empty() {
        return Vec::new();
    }
    let por_id: Vec<usize> = (0..itens.len()).filter(|&i| s(&itens[i], "id") == a).collect();
    if !por_id.is_empty() {
        return por_id;
    }
    (0..itens.len()).filter(|&i| { let t = sem_acento(s(&itens[i], "titulo")); a.split_whitespace().all(|p| t.contains(p)) }).collect()
}

/// Ok(posicao) quando o alvo aponta para um item so; Err(fala) quando nao achou ou achou varios.
fn um_so(alvo: &str, itens: &[Value], pergunta: &str) -> Result<usize, String> {
    match achar(alvo, itens).as_slice() {
        [] => Err(format!("Não achei '{alvo}' na agenda.")),
        [i] => Ok(*i),
        varios => Err(format!("{pergunta} {}", varios.iter().take(6).map(|&i| format!("{} [{}]", s(&itens[i], "titulo"), s(&itens[i], "id"))).collect::<Vec<_>>().join("; "))),
    }
}

fn concluir(arq: &Path, alvo: &str, dia: &str, agora: &Agora) -> String {
    let _g = trava();
    let mut itens = match ler(arq) {
        Ok(i) => i,
        Err(e) => return e,
    };
    let i = match um_so(alvo, &itens, "Achei mais de um, qual?") {
        Ok(i) => i,
        Err(e) => return e,
    };
    let chave = chave_feito(&itens[i], dia_de(dia, agora));
    if !tem(&itens[i], "feitos", &chave) {
        anota(&mut itens[i], "feitos", &chave, 120);
    }
    if !gravar(arq, &itens) {
        return NAO_GRAVOU.into();
    }
    let rep = if s(&itens[i], "repetir").is_empty() { String::new() } else { format!(" ({chave})") };
    format!("✅ Feito: {}{rep}.", s(&itens[i], "titulo"))
}

fn remover(arq: &Path, alvo: &str) -> String {
    let _g = trava();
    let mut itens = match ler(arq) {
        Ok(i) => i,
        Err(e) => return e,
    };
    let i = match um_so(alvo, &itens, "Achei mais de um, qual remover?") {
        Ok(i) => i,
        Err(e) => return e,
    };
    let fora = itens.remove(i);
    if gravar(arq, &itens) { format!("🗑️ Removido da agenda: {}.", s(&fora, "titulo")) } else { NAO_GRAVOU.into() }
}

fn sem_data(itens: &[Value]) -> Vec<&Value> {
    itens.iter().filter(|i| s(i, "data").is_empty() && !tem(i, "feitos", "sem-data")).collect()
}

fn maiuscula(t: &str) -> String {
    let mut c = t.chars();
    c.next().map(|p| p.to_uppercase().chain(c).collect()).unwrap_or_default()
}

fn listar(itens: &[Value], d0: Data, dias: i64, agora: &Agora) -> String {
    let mut linhas = Vec::new();
    for k in 0..dias.clamp(1, 31) {
        let d = Data(d0.0 + k);
        let oc = ocorrencias(d, itens);
        if !oc.is_empty() || dias <= 1 {
            linhas.push(format!("*{}*", maiuscula(&data_txt(d, agora))));
            if oc.is_empty() {
                linhas.push("(dia livre)".into());
            }
            linhas.extend(oc.iter().map(|&i| fmt(&itens[i], Some(d))));
        }
    }
    let pend = sem_data(itens);
    if !pend.is_empty() {
        linhas.push("*Tarefas sem data:*".into());
        linhas.extend(pend.iter().take(15).map(|i| fmt(i, None)));
    }
    if linhas.is_empty() { "Agenda vazia nesse período.".into() } else { linhas.join("\n") }
}

/// Horarios livres do dia entre 8h e 22h (hoje: a partir de agora), em minutos.
fn livres(itens: &[Value], d: Data, agora: &Agora, minimo: i64) -> Vec<(i64, i64)> {
    let mut cur = DIA_COMECA * 60;
    if d == agora.data {
        cur = cur.max((agora.hora * 60 + agora.min) as i64);
    }
    let mut ocupado: Vec<(i64, i64)> = ocorrencias(d, itens).into_iter().map(|i| &itens[i]).filter(|i| !s(i, "hora").is_empty())
        .map(|i| (minutos(s(i, "hora")), minutos(s(i, "hora")) + duracao_ou_30(i))).collect();
    ocupado.sort();
    let mut buracos = Vec::new();
    for (a, b) in ocupado {
        if a - cur >= minimo {
            buracos.push((cur, a));
        }
        cur = cur.max(b);
    }
    if DIA_TERMINA * 60 - cur >= minimo {
        buracos.push((cur, DIA_TERMINA * 60));
    }
    buracos
}

fn livres_txt(l: &[(i64, i64)], vazio: &str) -> String {
    if l.is_empty() { format!("Horários livres: {vazio}") } else { format!("Horários livres: {}", l.iter().map(|(a, b)| format!("{}–{}", hm(*a), hm(*b))).collect::<Vec<_>>().join(", ")) }
}

// ---------- lembretes ----------

/// Chamado pelo relogio (1 min). Avisa `lembrar` minutos antes de cada compromisso com hora, uma vez por ocorrencia.
/// `enviar` devolve o resultado do envio; so conta como avisado se comecar com "✅".
/// Uma frase para o bom dia: compromissos de hoje (os nao concluidos) e quantas tarefas sem data. "" = nada.
pub fn resumo_falado(arq: &Path, agora: &Agora) -> String {
    let _t = trava();
    let itens = ler(arq).unwrap_or_default();
    let oc: Vec<String> = ocorrencias(agora.data, &itens).into_iter().map(|i| &itens[i]).filter(|i| !feito(i, agora.data)).take(6)
        .map(|i| {
            let h = s(i, "hora");
            let quando = if h.is_empty() { String::new() } else { format!("às {}, ", h.strip_suffix(":00").map_or_else(|| h.replace(':', " e "), |cheia| format!("{cheia} horas"))) };
            format!("{quando}{}", s(i, "titulo"))
        }).collect();
    let pend = sem_data(&itens).len();
    let mut partes = Vec::new();
    if !oc.is_empty() {
        partes.push(format!("Na sua agenda de hoje: {}.", oc.join("; ")));
    }
    if pend > 0 {
        partes.push(format!("E você tem {pend} tarefa{} sem data esperando um horário.", if pend > 1 { "s" } else { "" }));
    }
    partes.join(" ")
}

pub fn lembretes(arq: &Path, agora: &Agora, enviar: &dyn Fn(&str) -> String) -> Vec<String> {
    let _g = trava();
    let Ok(mut itens) = ler(arq) else { return Vec::new() };
    let (hoje, mut enviados) = (agora.data, Vec::new());
    for i in ocorrencias(hoje, &itens) {
        let (hora, lembrar) = (s(&itens[i], "hora").to_string(), n(&itens[i], "lembrar"));
        if hora.is_empty() || lembrar <= 0 || feito(&itens[i], hoje) {
            continue;
        }
        let falta_s = minutos(&hora) * 60 - agora.segundos();
        let chave = format!("{} {hora}", hoje.iso());
        // janela: de `lembrar` minutos antes ate 10 minutos depois da hora
        if tem(&itens[i], "avisos", &chave) || falta_s > lembrar * 60 || falta_s <= -600 {
            continue;
        }
        let falta = falta_s.div_euclid(60);
        let quando = if falta > 0 { format!("daqui a {falta} min ({hora})") } else { format!("agora ({hora})") };
        // reserva ANTES de enviar: sem conseguir anotar nao envia, senao o mesmo lembrete sairia a cada minuto
        anota(&mut itens[i], "avisos", &chave, 60);
        if !gravar(arq, &itens) {
            continue;
        }
        let notas = if s(&itens[i], "notas").is_empty() { String::new() } else { format!("\n{}", s(&itens[i], "notas")) };
        if enviar(&format!("⏰ Lembrete: {} — {quando}.{notas}", s(&itens[i], "titulo"))).starts_with('✅') {
            enviados.push(s(&itens[i], "titulo").to_string());
        } else {
            let resto: Vec<Value> = itens[i]["avisos"].as_array().into_iter().flatten().filter(|c| *c != chave.as_str()).cloned().collect();
            itens[i]["avisos"] = json!(resto);
            gravar(arq, &itens);
        }
    }
    enviados
}

// ---------- ferramenta do Jarvis ----------

pub fn ferramenta(arq: &Path, args: &Value, agora: &Agora) -> String {
    let acao = txt(args, "acao");
    let alvo = || Some(txt(args, "alvo")).filter(|x| !x.is_empty()).unwrap_or_else(|| txt(args, "titulo"));
    match acao.as_str() {
        "adicionar" => {
            // modelos as vezes mandam a data em outros campos (data/dia + hora/horario): junta tudo em `quando`
            let quando = Some(txt(args, "quando")).filter(|x| !x.is_empty())
                .unwrap_or_else(|| ["data", "dia", "hora", "horario"].iter().map(|k| txt(args, k)).collect::<Vec<_>>().join(" ").trim().to_string());
            if titulo_vazio(&txt(args, "titulo")) {
                return "Falta dizer do que lembrar (e a que horas). NÃO crie nada ainda: pergunte 'lembrar de quê?' e espere a resposta.".into();
            }
            adicionar(arq, args, &quando, agora)
        }
        "concluir" => concluir(arq, &alvo(), &txt(args, "dia"), agora),
        "remover" => remover(arq, &alvo()),
        _ => {
            let itens = match ler(arq) {
                Ok(i) => i,
                Err(e) => return e,
            };
            let d = dia_de(&txt(args, "dia"), agora);
            match acao.as_str() {
                "livres" => livres_txt(&livres(&itens, d, agora, 30), "nenhum."),
                "organizar" => {
                    let mut partes = vec![listar(&itens, d, 1, agora), livres_txt(&livres(&itens, d, agora, 30), "nenhum (dia cheio)")];
                    if !sem_data(&itens).is_empty() {
                        partes.push("Para encaixar, use a ação 'adicionar' com hora (e depois 'concluir' a tarefa sem data pelo id), uma de cada vez, respeitando os horários livres; proponha antes se a pessoa não pediu para já marcar.".into());
                    }
                    partes.join("\n")
                }
                _ => listar(&itens, d, num(args, "dias", 1), agora),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // terca, 17/11/2026, 10:00
    fn agora() -> Agora {
        Agora { data: Data::civil(2026, 11, 17).unwrap(), hora: 10, min: 0, seg: 0 }
    }

    fn arq(nome: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("braniac-agenda-{}-{nome}.json", std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    fn quando(t: &str) -> (String, Option<(u32, u32)>) {
        let (d, h) = interpretar_quando(t, &agora());
        (d.map(Data::iso).unwrap_or_default(), h)
    }

    #[test]
    fn entende_data_e_hora_em_portugues() {
        for (texto, data, hora) in [
            ("amanhã 14h", "2026-11-18", Some((14, 0))),
            ("depois de amanhã às 9", "2026-11-19", Some((9, 0))),
            ("15/10 9:30", "2027-10-15", Some((9, 30))), // ja passou este ano
            ("25/12/26", "2026-12-25", None),
            ("segunda 10h", "2026-11-23", Some((10, 0))),
            ("terça 9h", "2026-11-24", Some((9, 0))), // hoje e terca e 9h ja passou
            ("terça 15h30", "2026-11-17", Some((15, 30))),
            ("sexta-feira", "2026-11-20", None),
            ("2026-12-01 08:15", "2026-12-01", Some((8, 15))),
            ("2026-12-01 as 8 da noite", "2026-12-01", Some((20, 0))),
            ("20 de dezembro às 10 horas", "2026-12-20", Some((10, 0))),
            ("5 de jan de 2028", "2028-01-05", None),
            ("hoje 18:45", "2026-11-17", Some((18, 45))),
            ("às 8 da noite", "2026-11-17", Some((20, 0))),
            ("9h", "2026-11-18", Some((9, 0))), // so a hora, que ja passou: amanha
            ("", "", None),
            ("quando terminar", "", None), // "qua" e "ter" dentro de outras palavras nao sao dia
            ("31/02", "", None),
            ("25h", "", None),
        ] {
            assert_eq!(quando(texto), (data.to_string(), hora), "{texto}");
        }
    }

    #[test]
    fn adiciona_lista_conclui_e_remove() {
        let (a, ag) = (arq("ciclo"), agora());
        let f = |v: Value| ferramenta(&a, &v, &ag);
        let r = f(json!({"acao": "adicionar", "titulo": "Dentista", "quando": "amanhã 14h"}));
        assert!(r.starts_with("📅 Anotado: Dentista — amanhã (quarta 18/11) às 14:00. Te lembro 15 min antes. ["), "{r}");
        // modelo que manda a data em campos separados, numero como texto, e bate com o dentista
        let r = f(json!({"acao": "adicionar", "titulo": "Reunião", "dia": "amanhã", "hora": "14:30", "duracao": "30"}));
        assert!(r.contains("⚠️ Bate com: Dentista."), "{r}");
        // "me lembre antes" do mesmo compromisso atualiza, nao duplica
        let r = f(json!({"acao": "adicionar", "titulo": "ir ao dentista", "quando": "amanhã 14h", "lembrar": 30}));
        assert!(r.starts_with("📅 Atualizei o que já estava marcado: ir ao dentista") && r.contains("30 min antes"), "{r}");
        assert!(f(json!({"acao": "adicionar", "titulo": "Lembrar"})).starts_with("Falta dizer do que lembrar"));
        assert!(f(json!({"acao": "adicionar", "titulo": "Comprar pão"})).contains("sem data (lista de tarefas)"));
        assert!(f(json!({"acao": "adicionar", "titulo": "Academia", "quando": "7h", "repetir": "dias úteis"})).contains("às 07:00, dias úteis"));
        assert!(f(json!({"acao": "adicionar", "titulo": "X y z", "repetir": "anual"})).contains("não existe"));

        let lista = f(json!({"acao": "listar", "dia": "amanhã"}));
        let linhas: Vec<&str> = lista.lines().collect();
        assert_eq!(linhas[0], "*Amanhã (quarta 18/11)*");
        assert!(linhas[1].starts_with("▫️ 07:00–08:00 Academia 🔁 dias úteis") && linhas[2].starts_with("▫️ 14:00–15:00 ir ao dentista") && linhas[3].starts_with("▫️ 14:30–15:00 Reunião"), "{lista}");
        assert_eq!((linhas[4], linhas[5].starts_with("▫️ Comprar pão  [")), ("*Tarefas sem data:*", true));
        // sabado nao tem academia; semana inteira so mostra dia com coisa
        assert!(f(json!({"acao": "listar", "dia": "sábado"})).contains("(dia livre)"));
        assert_eq!(f(json!({"acao": "listar", "dias": 7})).matches("Academia").count(), 4);

        assert_eq!(f(json!({"acao": "livres", "dia": "amanhã"})), "Horários livres: 08:00–14:00, 15:00–22:00");
        assert!(f(json!({"acao": "organizar", "dia": "amanhã"})).contains("Para encaixar"));

        assert_eq!(f(json!({"acao": "concluir", "alvo": "academia", "dia": "amanhã"})), "✅ Feito: Academia (2026-11-18).");
        assert!(f(json!({"acao": "listar", "dia": "amanhã"})).contains("✅ 07:00–08:00 Academia"));
        assert_eq!(f(json!({"acao": "concluir", "alvo": "pao"})), "✅ Feito: Comprar pão.");
        assert!(!f(json!({"acao": "listar"})).contains("Comprar pão"));
        assert!(f(json!({"acao": "remover", "alvo": "i"})).starts_with("Achei mais de um, qual remover?"));
        assert_eq!(f(json!({"acao": "remover", "alvo": "reuniao"})), "🗑️ Removido da agenda: Reunião.");
        assert_eq!(f(json!({"acao": "remover", "alvo": "reuniao"})), "Não achei 'reuniao' na agenda.");
        let _ = std::fs::remove_file(&a);
    }

    #[test]
    fn lembra_uma_vez_na_janela_e_tenta_de_novo_se_o_envio_falhar() {
        let a = arq("lembretes");
        ferramenta(&a, &json!({"acao": "adicionar", "titulo": "Reunião", "quando": "hoje 10:30", "notas": "sala 2"}), &agora());
        ferramenta(&a, &json!({"acao": "adicionar", "titulo": "Sem aviso", "quando": "hoje 10:30", "lembrar": 0}), &agora());
        let ditos = std::cell::RefCell::new(Vec::new());
        let ok = |t: &str| { ditos.borrow_mut().push(t.to_string()); "✅ foi".to_string() };
        let as_ = |h, m| Agora { hora: h, min: m, ..agora() };
        assert!(lembretes(&a, &as_(10, 14), &ok).is_empty()); // 16 min antes: cedo
        assert!(lembretes(&a, &as_(10, 20), &|_| "falhou".into()).is_empty()); // envio falhou: nao conta
        assert_eq!(lembretes(&a, &as_(10, 20), &ok), ["Reunião"]);
        assert!(lembretes(&a, &as_(10, 25), &ok).is_empty()); // ja avisou
        assert_eq!(*ditos.borrow(), ["⏰ Lembrete: Reunião — daqui a 10 min (10:30).\nsala 2"]);
        // concluido nao lembra; passou mais de 10 min da hora tambem nao
        ferramenta(&a, &json!({"acao": "adicionar", "titulo": "Almoço", "quando": "hoje 12h"}), &agora());
        assert!(lembretes(&a, &as_(12, 10), &ok).is_empty());
        ferramenta(&a, &json!({"acao": "adicionar", "titulo": "Banco", "quando": "hoje 15h"}), &agora());
        ferramenta(&a, &json!({"acao": "concluir", "alvo": "banco"}), &agora());
        assert!(lembretes(&a, &as_(14, 50), &ok).is_empty());
        let _ = std::fs::remove_file(&a);
    }

    #[test]
    fn arquivo_ilegivel_nao_e_sobrescrito() {
        let a = arq("ilegivel");
        std::fs::write(&a, "{ quebrado").unwrap();
        assert!(ferramenta(&a, &json!({"acao": "adicionar", "titulo": "Dentista", "quando": "amanhã 14h"}), &agora()).contains("ilegível"));
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "{ quebrado");
        let _ = std::fs::remove_file(&a);
    }
}
