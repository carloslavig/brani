//! Cabo Tito: exames, EAF, cronograma e checklist de um processo seletivo do Exercito. Tudo local e sem IA: so responde
//! o pedaco que a pergunta pede, a partir do edital guardado em dados/edital_stt/edital.json.
//! No Python as listas do edital estao escritas dentro do exercito.py; aqui elas vem do arquivo (nada de data, local ou
//! situacao de pessoa real no codigo). Formato do arquivo: ver `Edital::de_json`. A situacao do candidato vem de
//! dados/pessoal.json ("exercito_situacao"); o checklist fica em dados/exercito_checklist.json ({"feitos": [indices]}).
//! Ficou de fora: `sincronizar` (gravar no banco, na agenda e avisar no barramento) virou `datas_para_agenda` + `fatos`,
//! que so devolvem o que gravar; e a chave do modulo (modulos.ativo) e conferida por quem chama, com `ativo`.
#![allow(dead_code)]
use crate::tempo::Data;
use crate::texto_pt::sem_acento;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::OnceLock;

const GRUPOS: [(&str, &str, &str); 4] = [("sangue", "Sangue", r"\bsangue\b|\bexames? de sangue"), ("laboratorio", "Outros laboratoriais", r"\blaboratori"),
    ("especialistas", "Especialistas", r"\bespecialist"), ("imagem", "Imagem", r"\bimagens?\b")];

fn rx(cel: &'static OnceLock<Regex>, padrao: &str) -> &'static Regex {
    cel.get_or_init(|| Regex::new(padrao).expect("regex fixa do modulo"))
}

fn tem(padrao: &str, texto: &str) -> bool {
    Regex::new(padrao).map_or(false, |r| r.is_match(texto))
}

fn chave() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    rx(&R, r"(?i)\bex[eé]rcito\b|\beaf\b|\bstt\b|\bsargento\b|\bincorpora|\binspe[cç][aã]o\s+de\s+sa[uú]de|\bexpress[aã]o\s+d[aeo]\s+sa[uú]de|\binspira[cç][aã]o\s+d[aeo]\s+sa[uú]de|\bexames?\s+(?:do|para|da|que)\b|\blista\s+de\s+exames|\bexames?\s+m[eé]dicos?|\b11\s?rm\b|\bedital\b|prazo.{0,30}(?:ex[eé]rcito|\bis\b|eaf)|\bcorrida\s+de\s+12\b")
}

fn pedido() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    rx(&R, r"(?i)\?|\b(?:qual|quais|quando|quanto|onde|que\s+dia|me\s+(?:manda|passa|fala|diz|envia|mostra)|manda|passa|lista|prazo|data|dia|hor[aá]rio|hor[ao]s?|falta|quero\s+saber|preciso\s+saber)\b")
}

fn verbo() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    rx(&R, r"(?i)\b(?:j[aá]\s+)?(?:fiz|fez|feito|feitos|conclu[ií]|terminei|peguei|busquei|tenho|marca(?:r)?|desmarca(?:r)?|n[aã]o\s+fiz|ainda\s+n[aã]o\s+fiz)\b")
}

fn pede_lista() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    rx(&R, r"(?i)\bchecklist\b|\bcheck\s*list\b|o\s+que\s+(?:falta|j[aá]\s+fiz)\b.{0,25}\bexames?\b|quais\s+exames\s+(?:faltam|j[aá]\s+fiz)")
}

/// O modulo "exercito" esta ligado? (mesma chave do Python: desligado, nada daqui responde.)
pub fn ativo(dados: &Path) -> bool {
    crate::modulos::ativos(dados).iter().any(|m| m == "exercito")
}

/// E pergunta para o Cabo Tito? Curta, fala do processo seletivo e pede algo; assunto de vaquinha nao e dele.
pub fn eh_pergunta(texto: &str) -> bool {
    texto.chars().count() < 400 && chave().is_match(texto) && pedido().is_match(texto) && !tem(r"(?i)vaquinha|vakinha|arrecad", texto)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Marco {
    pub data: Data,
    pub hora: String,
    pub titulo: String,
    pub so_se: String,
}

#[derive(Clone, Debug, Default)]
pub struct Edital {
    pub site: String,
    pub is_dia: Option<Data>,
    pub is_hora: String,
    pub is_local: String,
    pub exames_ate: Option<Data>,
    pub exames: Vec<(String, Vec<String>)>,
    pub regras_exames: String,
    pub eaf: String,
    pub cronograma: Vec<Marco>,
    /// (nome, palavras-chave sem acento) na ordem do edital; depois do ultimo grupo vem o que se leva no dia.
    pub itens: Vec<(String, Vec<String>)>,
    /// chave do grupo -> (inicio, fim) em `itens`, fim exclusivo.
    pub grupos: Vec<(String, usize, usize)>,
    pub situacao: String,
}

fn textos(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(|x| x.as_str().map(String::from)).collect()
}

impl Edital {
    /// {"site","is_dia":"AAAA-MM-DD","is_hora":"8h","is_local","exames_ate":"AAAA-MM-DD","regras_exames","eaf",
    ///  "exames":[["Grupo",["exame",...]]], "cronograma":[["AAAA-MM-DD","HH:MM ou vazio","titulo","condicao ou vazio"]],
    ///  "itens":[["nome",["palavra-chave",...]]], "grupos":{"sangue":[0,14],"laboratorio":[14,16],...}}
    /// Linha de cronograma com data que nao existe e grupo fora da lista de itens sao descartados (dado de arquivo nao derruba).
    pub fn de_json(v: &Value, situacao: &str) -> Edital {
        let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
        let par = |x: &Value| Some((x[0].as_str()?.to_string(), textos(&x[1])));
        let itens: Vec<(String, Vec<String>)> = v["itens"].as_array().into_iter().flatten().filter_map(par).collect();
        let mut cronograma: Vec<Marco> = v["cronograma"].as_array().into_iter().flatten().filter_map(|c| {
            Some(Marco { data: Data::de_iso(c[0].as_str()?)?, hora: c[1].as_str().unwrap_or("").into(), titulo: c[2].as_str()?.into(), so_se: c[3].as_str().unwrap_or("").into() })
        }).collect();
        cronograma.sort_by_key(|c| c.data);
        let grupos = GRUPOS.iter().filter_map(|(k, ..)| {
            let (a, b) = (v["grupos"][*k][0].as_u64()? as usize, v["grupos"][*k][1].as_u64()? as usize);
            (a <= b && b <= itens.len()).then(|| (k.to_string(), a, b))
        }).collect();
        Edital { site: s("site"), is_dia: v["is_dia"].as_str().and_then(Data::de_iso), is_hora: s("is_hora"), is_local: s("is_local"),
            exames_ate: v["exames_ate"].as_str().and_then(Data::de_iso), exames: v["exames"].as_array().into_iter().flatten().filter_map(par).collect(),
            regras_exames: s("regras_exames"), eaf: s("eaf"), cronograma, itens, grupos, situacao: situacao.to_string() }
    }

    /// Le dados/edital_stt/edital.json e a situacao em dados/pessoal.json. Sem o arquivo do edital: Err (o Tito nao chuta data).
    pub fn ler(dados: &Path) -> Result<Edital, String> {
        let t = std::fs::read_to_string(dados.join("edital_stt").join("edital.json")).map_err(|e| format!("sem o edital em dados/edital_stt/edital.json: {e}"))?;
        let v: Value = serde_json::from_str(t.trim_start_matches('\u{feff}')).map_err(|e| format!("edital.json ilegível: {e}"))?;
        let pessoal: Value = std::fs::read_to_string(dados.join("pessoal.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
        Ok(Edital::de_json(&v, pessoal["exercito_situacao"].as_str().unwrap_or("Situação do candidato não configurada (dados/pessoal.json: exercito_situacao).")))
    }

    fn aviso(&self) -> String {
        format!("Datas do cronograma inicial do edital; só vale o dia, hora e local da convocação específica. Acompanhar sempre em {}.", self.site)
    }

    pub fn lista_exames(&self) -> String {
        let mut l = vec!["Exames que o Exército pede na Inspeção de Saúde (edital item 12):".to_string()];
        l.extend(self.exames.iter().map(|(g, itens)| format!("• {g}: {}.", itens.join("; "))));
        format!("{}\n{}", l.join("\n"), self.regras_exames)
    }

    pub fn proximos(&self, hoje: Data, n: usize) -> Vec<&Marco> {
        self.cronograma.iter().filter(|c| c.data >= hoje).take(n).collect()
    }

    pub fn cronograma_texto(&self) -> String {
        self.cronograma.iter().map(linha).collect::<Vec<_>>().join("\n")
    }

    /// Primeiro marco (de hoje em diante, senao o ultimo) cujo titulo casa com `padrao`: uma linha so.
    fn marco(&self, padrao: &str, hoje: Data) -> String {
        let Ok(r) = Regex::new(&format!("(?i){padrao}")) else { return String::new() };
        let ach: Vec<&Marco> = self.cronograma.iter().filter(|c| r.is_match(&c.titulo)).collect();
        ach.iter().find(|c| c.data >= hoje).or(ach.last()).map(|c| linha(c)).unwrap_or_default()
    }

    /// Exames citados na pergunta (casa por palavra de 5+ letras do nome do exame).
    fn exame(&self, p: &str) -> Vec<&str> {
        let fora = ["exame", "exames", "total", "completo", "completa", "quais", "levar"];
        let pal: BTreeSet<String> = palavras5(p).into_iter().filter(|w| !fora.contains(&w.as_str())).collect();
        self.exames.iter().flat_map(|(_, itens)| itens).filter(|i| !pal.is_disjoint(&palavras5(i))).map(String::as_str).collect()
    }

    /// Resposta do Cabo Tito: SO o pedaco que a pergunta pede (hora, local, exame, EAF, resultado...). Devolve (texto, resumo curto).
    pub fn resposta(&self, pergunta: &str, hoje: Data) -> (String, String) {
        let p = pergunta;
        let prox = self.proximos(hoje, 1);
        let resumo = prox.first().map_or("processo sem marcos futuros no cronograma".to_string(), |c| format!("próximo marco: {} {}", fmt(c.data), c.titulo.chars().take(60).collect::<String>()));
        if tem(r"(?i)\btudo\b|todas as datas|cronograma|calend", p) {
            return (format!("{}\n{}", self.cronograma_texto(), self.aviso()), resumo);
        }
        // sem a data da IS no arquivo nada que depende dela e dito (falt = None)
        let falt = self.is_dia.map(|d| d.0 - hoje.0);
        let is_e = self.is_dia.map(|d| {
            let dia = match d.0 - hoje.0 { 0 => "hoje".to_string(), 1 => "amanhã".to_string(), _ => format!("em {}", fmt(d)) };
            format!("A Inspeção de Saúde é {dia} ({}){}.", fmt(d), if self.is_hora.is_empty() { String::new() } else { format!(", às {}", self.is_hora) })
        });
        let futuro = falt.map_or(false, |f| f >= 0);
        let mut partes: Vec<String> = Vec::new();
        let eh_eaf = tem(r"(?i)\beaf\b|f[ií]sic|corrida|flex[aã]o|abdominal|m[ií]nimo", p);
        if tem(r"(?i)hor[aá]rio|que\s+horas|\bhoras?\b|\bcome[cç]a", p) && futuro && !eh_eaf {
            partes.extend(is_e.clone());
        }
        if tem(r"(?i)\bonde\b|\blocal\b|endere[cç]o", p) && !self.is_local.is_empty() {
            partes.push(self.is_local.clone());
        }
        if tem(r"(?i)\bquando\b|que\s+dia|\bdata\b", p) && partes.is_empty() && !tem(r"(?i)eaf|resultado|recurso|incorpora|classifica", p) {
            partes.push(if futuro { is_e.clone().unwrap_or_default() } else { self.marco(r"Resultado da Inspe", hoje) });
        }
        if tem(r"(?i)\bfalta|quantos dias", p) && futuro && !eh_eaf {
            partes.push(match falt { Some(0) => "A Inspeção de Saúde é HOJE.".into(), f => format!("Faltam {} dia(s) para a Inspeção de Saúde.", f.unwrap_or(0)) });
        }
        if tem(r"(?i)\blevar|\bleve\b|trazer|documento|sunga|cnis|roupa de banho", p) {
            partes.push("Leve os exames (laudo original + cópia, de até 1 mês antes), RG, CNIS e sunga.".into());
        }
        if tem(r"(?i)resultado", p) {
            partes.push(self.marco(if eh_eaf { r"Resultado do EAF" } else { r"Resultado da Inspeção de Saúde \(" }, hoje));
        }
        if tem(r"(?i)recurso", p) {
            partes.push(self.marco(r"recurso da IS", hoje));
        }
        if tem(r"(?i)incorpora|classifica", p) {
            partes.push(self.marco(r"classifica|Incorpora", hoje));
        }
        if eh_eaf && partes.is_empty() {
            partes.push(if tem(r"(?i)quando|\bdata\b|\bdia\b|prazo", p) { self.marco(r"Período do EAF", hoje) } else { self.eaf.clone() });
        }
        if partes.is_empty() && tem(r"(?i)\bexames?\b|laudo|lista|sangue|preciso|pede", p) {
            let ex = self.exame(p);
            partes.push(if !ex.is_empty() && !tem(r"(?i)lista|quais|todos", p) { format!("Pede: {}.", ex[..ex.len().min(3)].join("; ")) } else { self.lista_exames() });
        }
        if tem(r"(?i)situa[cç][aã]o|meu caso|como estou", p) {
            partes.push(self.situacao.clone());
        }
        if partes.is_empty() && tem(r"(?i)\bprazos?\b|pr[oó]xim", p) {
            if let Some(c) = prox.first() {
                partes.push(linha(c));
                partes.extend(falt.filter(|f| *f > 0).map(|f| format!("Faltam {f} dia(s) para a Inspeção de Saúde.")));
            }
        }
        partes.retain(|x| !x.is_empty());
        if partes.is_empty() { // pergunta vaga: so o essencial, nunca o texto todo
            partes = vec![prox.first().map_or("Não há marcos futuros no cronograma.".to_string(), |c| linha(c)), "Quer saber de horário, local, exames, EAF ou resultado?".into()];
        }
        (partes.join(if partes.len() > 2 { "\n" } else { " " }), resumo)
    }

    /// Fatos que o Cabo Tito guarda no banco do assunto "exercito": (escopo, chave, texto). Quem chama grava (nivel local).
    pub fn fatos(&self) -> Vec<(String, String, String)> {
        let crono: Vec<String> = self.cronograma.iter().map(|c| format!("{} {}", fmt(c.data), c.titulo)).collect();
        [("situacao", self.situacao.clone()), ("exames", self.lista_exames().replace('\n', " ")), ("eaf", self.eaf.clone()),
         ("cronograma", format!("cronograma do edital: {}. {}", crono.join(" | "), self.aviso())),
         ("consulta", "Se perguntarem prazo, data, lista de exames ou EAF do Exército, o Jarvis consulta o Cabo Tito.".to_string())]
            .into_iter().map(|(k, t)| ("dossie".to_string(), format!("exercito:{k}"), format!("exercito: {t}"))).collect()
    }

    /// Datas futuras que ainda nao estao na agenda: json {titulo, quando, duracao, lembrar, notas} para quem liga a agenda.
    pub fn datas_para_agenda(&self, hoje: Data, ja_na_agenda: &[String]) -> Vec<Value> {
        self.cronograma.iter().filter(|c| c.data >= hoje).filter_map(|c| {
            let cond = if c.so_se.is_empty() { String::new() } else { format!(" ({})", c.so_se) };
            let titulo: String = format!("Exército: {}{cond}", c.titulo).chars().take(120).collect();
            let com_hora = !c.hora.is_empty();
            (!ja_na_agenda.contains(&titulo)).then(|| json!({ "titulo": titulo, "quando": format!("{} {}", c.data.iso(), c.hora).trim(), "duracao": if com_hora { 60 } else { 0 },
                "lembrar": if com_hora { 1440 } else { 0 }, "notas": self.aviso().chars().take(290).collect::<String>() }))
        }).collect()
    }

    // ---------- checklist: a pessoa responde o que ja fez e o Cabo Tito marca ----------

    /// Texto do checklist com ✅/⬜ numerado e a contagem. Sempre a lista toda.
    pub fn checklist(&self, feitos: &BTreeSet<usize>, hoje: Data) -> String {
        let mut out = vec!["Checklist da Inspeção de Saúde — responda 'fiz hemograma e audiometria' ou 'fiz 3 e 5' que eu marco:".to_string()];
        let fim = self.grupos.iter().map(|g| g.2).max().unwrap_or(0);
        let mut blocos: Vec<(&str, usize, usize)> = self.grupos.iter().map(|(k, a, b)| (GRUPOS.iter().find(|g| g.0 == k).map_or("", |g| g.1), *a, *b)).collect();
        blocos.push(("Levar no dia", fim, self.itens.len()));
        for (titulo, a, b) in blocos.into_iter().filter(|b| b.1 < b.2) {
            out.push(format!("\n{titulo}:"));
            out.extend((a..b).map(|i| format!("{} {}. {}", if feitos.contains(&i) { "✅" } else { "⬜" }, i + 1, self.itens[i].0)));
        }
        let falta = self.is_dia.filter(|d| *d >= hoje).map(|d| format!(" · faltam {} dia(s) para a IS ({})", d.0 - hoje.0, fmt(d))).unwrap_or_default();
        out.push(format!("\nFeitos: {}/{}{falta}", feitos.iter().filter(|i| **i < self.itens.len()).count(), self.itens.len()));
        if let Some(d) = self.exames_ate {
            out.push(format!("Lembrete: laudo original + cópia, com nome e CPF/RG, datado a partir de {}.", fmt(d)));
        }
        out.join("\n")
    }

    /// Indices citados no texto: por palavra-chave, por numero ("3 e 5", "1-4", "2 a 6") e por nome de grupo ("todos os de sangue").
    pub fn achar(&self, texto: &str) -> BTreeSet<usize> {
        static FAIXA: OnceLock<Regex> = OnceLock::new();
        static NUM: OnceLock<Regex> = OnceLock::new();
        let faixa = rx(&FAIXA, r"\b(\d{1,2})\s*(?:a|ate|-)\s*(\d{1,2})\b");
        let t = sem_acento(texto);
        let n = self.itens.len();
        let mut ach: BTreeSet<usize> = self.itens.iter().enumerate()
            .filter(|(_, (_, ks))| ks.iter().any(|k| !k.is_empty() && tem(&format!(r"\b{}", regex::escape(k)), &t))).map(|(i, _)| i).collect();
        for c in faixa.captures_iter(&t) {
            if let (Ok(a), Ok(b)) = (c[1].parse::<usize>(), c[2].parse::<usize>()) {
                ach.extend((a..=b).filter(|x| (1..=n).contains(x)).map(|x| x - 1));
            }
        }
        let sem_faixa = faixa.replace_all(&t, " ");
        ach.extend(rx(&NUM, r"\b(\d{1,2})\b").captures_iter(&sem_faixa).filter_map(|c| c[1].parse::<usize>().ok()).filter(|x| (1..=n).contains(x)).map(|x| x - 1));
        let mut grupos: Vec<&str> = GRUPOS.iter().filter(|g| tem(g.2, &t)).map(|g| g.0).collect();
        if tem(r"\btudo\b|\btodos\b|\btodas\b", &t) && grupos.is_empty() && tem(r"\bexames?\b|\btudo\b", &t) {
            grupos = GRUPOS.iter().map(|g| g.0).collect();
        }
        for (_, a, b) in self.grupos.iter().filter(|g| grupos.contains(&g.0.as_str())) {
            ach.extend(*a..*b);
        }
        ach
    }

    /// A frase e sobre o checklist? Pede a lista, ou diz que fez/nao fez algo que e exame.
    pub fn eh_checklist(&self, texto: &str) -> bool {
        static DIG: OnceLock<Regex> = OnceLock::new();
        if texto.chars().count() > 400 {
            return false;
        }
        pede_lista().is_match(texto) || (verbo().is_match(texto)
            && (!self.achar(&rx(&DIG, r"\d+").replace_all(texto, " ")).is_empty() || tem(r"(?i)\bexames?\b", texto) || chave().is_match(texto)))
    }

    /// Marca (ou desmarca, com "desmarca"/"nao fiz") o que a pessoa disse e devolve a resposta com o checklist atualizado.
    pub fn marcar(&self, dados: &Path, texto: &str, hoje: Data) -> Result<String, String> {
        let mut feitos = estado(dados);
        if pede_lista().is_match(texto) && !verbo().is_match(texto) {
            return Ok(self.checklist(&feitos, hoje));
        }
        let ach = self.achar(texto);
        if ach.is_empty() {
            return Ok(format!("Não entendi qual exame. Diga o nome (ex.: 'fiz hemograma e audiometria') ou o número da lista.\n\n{}", self.checklist(&feitos, hoje)));
        }
        let neg = tem(r"(?i)desmarca|n[aã]o\s+fiz|ainda\s+n[aã]o", texto);
        if neg { feitos.retain(|i| !ach.contains(i)) } else { feitos.extend(&ach) }
        std::fs::create_dir_all(dados).and_then(|_| std::fs::write(dados.join("exercito_checklist.json"), json!({ "feitos": feitos }).to_string()))
            .map_err(|e| format!("não gravei o checklist: {e}"))?;
        let nomes: Vec<&str> = ach.iter().map(|i| self.itens[*i].0.as_str()).collect();
        Ok(format!("{}{}.\n\n{}", if neg { "Desmarquei: " } else { "Marquei como feito: " }, nomes.join(", "), self.checklist(&feitos, hoje)))
    }
}

/// Itens ja feitos (dados/exercito_checklist.json). Arquivo ausente ou ilegivel = nada feito.
pub fn estado(dados: &Path) -> BTreeSet<usize> {
    let v: Value = std::fs::read_to_string(dados.join("exercito_checklist.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    v["feitos"].as_array().into_iter().flatten().filter_map(|x| x.as_u64().map(|n| n as usize)).collect()
}

fn fmt(d: Data) -> String {
    let (y, m, dia) = d.partes();
    format!("{dia:02}/{m:02}/{y:04}")
}

fn linha(c: &Marco) -> String {
    format!("• {}{}: {}{}", fmt(c.data), if c.hora.is_empty() { String::new() } else { format!(" {}", c.hora) }, c.titulo,
        if c.so_se.is_empty() { String::new() } else { format!(" ({})", c.so_se) })
}

fn palavras5(t: &str) -> BTreeSet<String> {
    static R: OnceLock<Regex> = OnceLock::new();
    rx(&R, r"\w{5,}").find_iter(&t.to_lowercase()).map(|m| m.as_str().to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // edital inventado, pequeno: nada daqui e de processo real
    fn edital() -> Edital {
        Edital::de_json(&json!({
            "site": "site.exemplo", "is_dia": "2030-03-10", "is_hora": "8h", "is_local": "No Quartel Modelo, Rua Um, Cidade Exemplo.", "exames_ate": "2030-02-10",
            "exames": [["Sangue", ["Hemograma completo", "Glicemia de jejum"]], ["Imagem", ["Ressonância dos joelhos", "MAPA (pressão arterial)"]]],
            "regras_exames": "Todos com laudo, original e cópia.",
            "eaf": "EAF: flexão 10, abdominal 20 e corrida de 12 minutos com 1.800 m.",
            "cronograma": [["2030-03-24", "", "Resultado da Inspeção de Saúde (no site)", ""], ["2030-03-10", "08:00", "Inspeção de Saúde: exames e documento", ""],
                ["2030-03-25", "", "Prazo de recurso da IS", "se for considerado inapto"], ["2030-04-08", "", "Período do EAF, 08 a 18/04", ""],
                ["2030-05-08", "", "Resultado do EAF (no site)", ""], ["2030-06-01", "", "Incorporação", ""], ["2030-02-30", "", "data que não existe", ""]],
            "itens": [["Hemograma completo", ["hemograma"]], ["Glicemia", ["glicemia", "glicose"]], ["HIV", ["hiv"]], ["Audiometria", ["audiometria"]],
                ["Ressonância dos joelhos", ["ressonancia", "joelho"]], ["MAPA", ["mapa", "pressao"]], ["Documento com foto", ["documento"]]],
            "grupos": {"sangue": [0, 3], "especialistas": [3, 4], "imagem": [4, 6], "laboratorio": [9, 99]}
        }), "Chamado para a inspeção.")
    }
    fn d(y: i64, m: i64, dia: i64) -> Data { Data::civil(y, m, dia).unwrap() }

    #[test]
    fn pergunta_do_exercito_vai_para_o_cabo_tito() {
        for q in ["Me manda a lista de exames que o exército pede pra minha vaga", "quando é o EAF?", "qual o prazo do exército?"] {
            assert!(eh_pergunta(q), "{q}");
        }
        assert!(!eh_pergunta("como está a vaquinha dos exames do exército?"));
        assert!(!eh_pergunta("o exército é grande") && !eh_pergunta("que dia é hoje?") && !eh_pergunta(&format!("quando é o EAF? {}", "x".repeat(400))));
    }

    #[test]
    fn resposta_tem_so_o_pedaco_pedido() {
        let e = edital();
        let hoje = d(2030, 2, 27);
        assert_eq!(e.cronograma.len(), 6); // a data que nao existe saiu; o resto ficou em ordem
        let exames = e.resposta("lista de exames", hoje).0;
        assert!(exames.contains("Hemograma") && exames.contains("Ressonância") && exames.contains("MAPA") && exames.contains("Todos com laudo"));
        let (datas, resumo) = e.resposta("qual o prazo?", hoje);
        assert!(datas.contains("10/03/2030 08:00") && datas.contains("Faltam 11 dia"), "{datas}");
        assert!(resumo.starts_with("próximo marco: 10/03/2030 Inspeção"));
        assert!(e.resposta("quais os mínimos do EAF?", hoje).0.contains("1.800 m"));
        let quando_eaf = e.resposta("quando é o EAF?", hoje).0;
        assert!(!quando_eaf.contains("1.800 m") && quando_eaf.contains("08/04/2030"), "{quando_eaf}");
        let onde = e.resposta("onde é a inspeção?", d(2030, 3, 9)).0;
        assert!(onde.contains("Quartel Modelo") && onde.chars().count() < 120);
        assert_eq!(e.resposta("que horas começa?", d(2030, 3, 9)).0, "A Inspeção de Saúde é amanhã (10/03/2030), às 8h.");
        assert_eq!(e.resposta("quantos dias falta?", d(2030, 3, 10)).0, "A Inspeção de Saúde é HOJE.");
        assert!(e.resposta("quando sai o resultado?", hoje).0.contains("24/03/2030") && e.resposta("resultado do EAF?", hoje).0.contains("08/05/2030"));
        assert!(e.resposta("e o recurso?", hoje).0.contains("(se for considerado inapto)"));
        assert_eq!(e.resposta("preciso de ressonância?", hoje).0, "Pede: Ressonância dos joelhos.");
        assert!(e.resposta("qual a minha situação?", hoje).0.contains("Chamado para a inspeção."));
        assert!(e.resposta("me passa todas as datas", hoje).0.ends_with("Acompanhar sempre em site.exemplo."));
        // pergunta vaga: o proximo marco e a oferta, nunca o texto todo
        assert_eq!(e.resposta("e ai?", hoje).0, "• 10/03/2030 08:00: Inspeção de Saúde: exames e documento Quer saber de horário, local, exames, EAF ou resultado?");
        // passou a IS: "quando" fala do resultado; depois de tudo, nao ha marco
        assert!(e.resposta("quando é?", d(2030, 3, 11)).0.contains("Resultado da Inspeção"));
        assert_eq!(e.resposta("oi", d(2031, 1, 1)), ("Não há marcos futuros no cronograma. Quer saber de horário, local, exames, EAF ou resultado?".into(), "processo sem marcos futuros no cronograma".into()));
        // edital sem a data da IS: nao inventa dia nem contagem
        let sem = Edital { is_dia: None, ..edital() };
        assert!(!sem.resposta("que horas começa? quantos dias falta?", hoje).0.contains("Faltam"));
    }

    #[test]
    fn checklist_marca_desmarca_e_grava() {
        let p = std::env::temp_dir().join(format!("braniac-exercito-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        let e = edital();
        let hoje = d(2030, 3, 1);
        assert_eq!(e.grupos.len(), 3); // grupo fora da lista de itens foi descartado
        assert!(e.marcar(&p, "fiz hemograma e audiometria", hoje).unwrap().contains("Marquei como feito: Hemograma completo, Audiometria."));
        assert_eq!(estado(&p), BTreeSet::from([0, 3]));
        e.marcar(&p, "fiz 2 e 5-6", hoje).unwrap();
        assert!(estado(&p).is_superset(&BTreeSet::from([1, 4, 5])));
        std::fs::write(p.join("exercito_checklist.json"), "{}").unwrap();
        e.marcar(&p, "fiz todos os exames de sangue", hoje).unwrap();
        assert_eq!(estado(&p), BTreeSet::from([0, 1, 2]));
        assert!(e.marcar(&p, "não fiz hiv", hoje).unwrap().starts_with("Desmarquei: HIV.") && !estado(&p).contains(&2));
        assert!(e.marcar(&p, "fiz o 40", hoje).unwrap().starts_with("Não entendi qual exame") && estado(&p).len() == 2); // numero fora da lista nao marca nada
        let c = e.marcar(&p, "me mostra o checklist", hoje).unwrap();
        assert!(c.contains("✅ 1. Hemograma completo") && c.contains("⬜ 3. HIV") && c.contains("Levar no dia:\n⬜ 7. Documento com foto")
            && c.contains("Feitos: 2/7 · faltam 9 dia(s) para a IS (10/03/2030)") && c.contains("datado a partir de 10/02/2030"), "{c}");
        assert!(e.eh_checklist("o que falta dos exames?") && e.eh_checklist("já fiz a glicose") && !e.eh_checklist("tenho 2 filhos") && !e.eh_checklist("o que falta?"));
        std::fs::write(p.join("exercito_checklist.json"), "lixo").unwrap();
        assert!(estado(&p).is_empty());
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn le_do_arquivo_e_entrega_datas_e_fatos() {
        let p = std::env::temp_dir().join(format!("braniac-exercito-ler-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        assert!(Edital::ler(&p).is_err()); // sem edital nao ha resposta inventada
        std::fs::create_dir_all(p.join("edital_stt")).unwrap();
        std::fs::write(p.join("edital_stt").join("edital.json"), json!({ "site": "s.exemplo", "is_dia": "2030-03-10", "eaf": "EAF x",
            "cronograma": [["2030-03-10", "08:00", "Inspeção de Saúde", ""], ["2030-03-25", "", "Prazo de recurso da IS", "se inapto"], ["2030-01-01", "", "passou", ""]] }).to_string()).unwrap();
        let e = Edital::ler(&p).unwrap();
        assert!(e.situacao.contains("não configurada"));
        std::fs::write(p.join("pessoal.json"), r#"{"exercito_situacao": "apto na primeira fase"}"#).unwrap();
        let e = Edital::ler(&p).unwrap();
        assert_eq!(e.situacao, "apto na primeira fase");
        let ag = e.datas_para_agenda(d(2030, 2, 1), &["Exército: Prazo de recurso da IS (se inapto)".to_string()]);
        assert_eq!(ag.len(), 1); // o que passou e o que ja esta na agenda nao voltam
        assert_eq!((ag[0]["titulo"].as_str(), ag[0]["quando"].as_str(), ag[0]["lembrar"].as_i64()), (Some("Exército: Inspeção de Saúde"), Some("2030-03-10 08:00"), Some(1440)));
        let f = e.fatos();
        assert!(f.len() == 5 && f.iter().all(|x| x.0 == "dossie" && x.1.starts_with("exercito:") && x.2.starts_with("exercito: ")) && f[0].2.ends_with("apto na primeira fase"));
        assert!(!ativo(&p) || ativo(&p)); // so confere que a chave do modulo e lida sem cair
        let _ = std::fs::remove_dir_all(&p);
    }
}
