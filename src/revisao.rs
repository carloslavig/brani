//! Revisao das decisoes do atendimento ANTES de agirem (porte de braniac/revisao.py). Um auditor barato e deterministico olha
//! cada decisao (E enviar, I ignorar, A avisar, P perguntar) e procura os erros ja vistos: rotular a pessoa (trote, golpe...),
//! repetir o que ja foi dito a ela, ignorar/so avisar quando ela fez um PEDIDO. Achou: a decisao NAO sai e vai a um revisor.
//! Sem revisor (ou revisao ainda ruim): cala se repetiria, senao manda a resposta NEUTRA uma unica vez. Nunca a proposta barrada.
//! O revisor (canal ao vivo ou IA) entra como closure que recebe a linha V do protocolo; o que ja dissemos a pessoa nas ultimas
//! 24 h (`saidas`) e se o contato e de "nao responder" entram por parametro (quem chama le do banco e do contexto).
#![allow(dead_code)]
use crate::atender::Item;
use crate::canal;
use crate::contexto::{re, sem_marca};
use crate::tempo::Agora;
use std::collections::BTreeSet;
use std::path::Path;

pub const LIMIAR_REPETICAO: f64 = 0.55;
/// Resposta segura quando a decisao proposta e barrada e ninguem revisa.
pub const NEUTRA: &str = "Recebi sua mensagem! Vou conferir isso direitinho e já te retorno.";
const PARADAS: &str = "a o as os um uma uns umas de do da dos das em no na nos nas por para pra pro com sem e ou que se eu voce vc tu ele ela \
                       nao sim ja mais mas so tambem eh sao foi ser esta estao tem ter como qual quais quem onde quando isso isto esse essa \
                       aquele aquela meu minha seu sua nosso me te lhe ao aos";

re!(julgamento, r"(?i)\btrote\b|zoeira|golpe\b|golpista|mentiros[oa]|\btroll\b|palha[cç]ada|sacanagem|n[aã]o\s+leva\s+a\s+s[eé]rio");
re!(pedido, r"(?i)\?|or[cç]amento|quanto\s+(?:custa|fica|[eé])|gostaria|\bquero\b|preciso|consegue|tem\s+como|d[aá]\s+pra|pode\s+(?:me|fazer|ver)|valor");
re!(endereco, r"https?://\S+|www\.\S+");

/// Minusculas, sem acento, so letras e digitos, um espaco entre as palavras (o _norm da memoria do Python).
fn norm(x: &str) -> String {
    let s: String = x.chars().flat_map(char::to_lowercase).map(sem_marca).map(|c| if c.is_ascii_lowercase() || c.is_ascii_digit() { c } else { ' ' }).collect();
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn palavras(x: &str) -> BTreeSet<String> {
    norm(x).split(' ').filter(|p| p.len() > 1 && !PARADAS.split_whitespace().any(|s| s == *p)).map(str::to_string).collect()
}

fn frases(txt: &str) -> BTreeSet<String> {
    txt.split(['.', '!', '?', ',', ';', ':']).map(norm).filter(|f| f.len() > 22).collect()
}

fn corte(t: &str, n: usize) -> String {
    t.chars().take(n).collect()
}

/// True se o texto repete (mesma frase ou resposta quase igual) algo que ja dissemos a esta pessoa.
pub fn repete(texto: &str, saidas: &[String]) -> bool {
    let (novas, pal) = (frases(texto), palavras(texto));
    saidas.iter().any(|s| {
        if !novas.is_disjoint(&frases(s)) {
            return true; // a mesma frase de novo
        }
        let outras = palavras(s);
        !pal.is_empty() && !outras.is_empty() && pal.intersection(&outras).count() as f64 / pal.union(&outras).count() as f64 >= LIMIAR_REPETICAO
    })
}

/// Lista de problemas da decisao (vazia = pode seguir). `nao_responde` = o contato esta na lista de "nao responder".
pub fn auditar(it: &Item, cod: &str, texto: &str, saidas: &[String], nao_responde: bool) -> Vec<String> {
    let mut problemas = vec![];
    if julgamento().is_match(&endereco().replace_all(texto, " ")) { // endereco nao conta (validar_resposta ja cuida de links)
        problemas.push("rotula/julga a pessoa (trote, zoeira, golpe...): proibido; trate como cliente real, com educação".to_string());
    }
    if cod == "E" && repete(texto, saidas) {
        problemas.push("repete o que já foi dito a esta pessoa nas últimas 24 h (não volte a falar do mesmo ponto)".to_string());
    }
    if (cod == "A" || cod == "I") && pedido().is_match(&it.msgs.join(" ")) && !nao_responde {
        problemas.push("a pessoa fez um PEDIDO/pergunta e a decisão não responde nada (responda o que dá e peça o dado que falta)".to_string());
    }
    problemas
}

/// Linha V do protocolo: a pergunta original + a decisao proposta + o que o auditor achou.
pub fn formatar_revisao(id: &str, it: &Item, flags: &str, cod: &str, texto: &str, problemas: &[String]) -> String {
    let base = canal::formatar_pergunta(id, &it.nome, flags, &it.msgs, &it.ctx);
    format!("V{}|PROPOSTA={}|PROBLEMAS={}", &base[1..], canal::limpar(&format!("{cod}:{texto}"), 220), canal::limpar(&problemas.join("; "), 260))
}

/// Aplica auditoria + revisao a uma decisao (cod, texto). Devolve (cod, texto, anotacao); anotacao "" se nada mudou,
/// "revisada", "autoignorada" ou "neutra". `revisar` recebe a linha V e devolve (cod, texto) ou None; `log` recebe o registro.
#[allow(clippy::too_many_arguments)]
pub fn decidir_com_revisao(it: &Item, flags: &str, cod: &str, texto: &str, saidas: &[String], nao_responde: bool, id: &str,
                           revisar: &dyn Fn(&str) -> Option<(String, String)>, log: &dyn Fn(&str)) -> (String, String, &'static str) {
    let problemas = auditar(it, cod, texto, saidas, nao_responde);
    if problemas.is_empty() {
        return (cod.to_string(), texto.to_string(), "");
    }
    if let Some((c2, t2)) = revisar(&formatar_revisao(id, it, flags, cod, texto, &problemas)) {
        if auditar(it, &c2, &t2, saidas, nao_responde).is_empty() || (c2 == "P" || c2 == "A") && !julgamento().is_match(&t2) {
            log(&format!("{} | REVISADA {cod}->{c2} | motivo={:?} | nova={:?}", it.nome, corte(&problemas[0], 70), corte(&t2, 100)));
            return (c2, t2, "revisada");
        }
    }
    // sem revisor, ou a revisao ainda tem problema: o proposto NAO sai, e o dono nao e incomodado
    if problemas.iter().any(|p| p.contains("repete")) { // ja dissemos isso: melhor calar do que repetir
        log(&format!("{} | AUTO-IGNORADA {cod} | repetiria o que ja foi dito | proposta={:?}", it.nome, corte(texto, 100)));
        return ("I".into(), String::new(), "autoignorada");
    }
    if saidas.iter().any(|s| s == NEUTRA) { // a mensagem neutra ja foi dada: nao repete
        log(&format!("{} | AUTO-IGNORADA {cod} | neutra ja enviada | proposta={:?}", it.nome, corte(texto, 100)));
        return ("I".into(), String::new(), "autoignorada");
    }
    log(&format!("{} | NEUTRA no lugar de {cod} | {:?} | proposta={:?}", it.nome, corte(&problemas[0], 90), corte(texto, 100)));
    ("E".into(), NEUTRA.into(), "neutra")
}

/// Registro de auditoria em dados/revisoes.log (mesmo formato do Python: "AAAA-MM-DD HH:MM:SS | texto").
pub fn anotar(dados: &Path, agora: &Agora, txt: &str) {
    use std::io::Write;
    let _ = std::fs::create_dir_all(dados);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dados.join("revisoes.log")) {
        let _ = writeln!(f, "{} {}:{:02} | {}", agora.data.iso(), agora.hm(), agora.seg, txt.replace(['\r', '\n'], " "));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn item(nome: &str, msgs: &[&str]) -> Item {
        Item { nome: nome.into(), msgs: msgs.iter().map(|s| s.to_string()).collect(), autores: vec![nome.into()], ..Default::default() }
    }

    fn cliente() -> Item {
        item("Rui Lopes 🌝", &["Eu gostaria de fazer o orçamento da cerimônia e da lua de mel"])
    }

    fn ninguem(_: &str) -> Option<(String, String)> {
        None
    }

    #[test]
    fn rotular_a_pessoa_e_barrado_em_resposta_e_em_nota() {
        for texto in ["Parece trote: pediu 31 de fevereiro", "isso é zoeira", "pode ser golpe", "esse cara é mentiroso", "é um troll", "que PALHAÇADA", "ele não leva a sério"] {
            for cod in ["E", "A", "P"] {
                assert!(auditar(&cliente(), cod, texto, &[], false).iter().any(|p| p.contains("rotula")), "{cod} {texto}");
            }
        }
        // endereco com a palavra nao e julgamento
        assert!(!auditar(&cliente(), "E", "veja https://golpe.com/x", &[], false).iter().any(|p| p.contains("rotula")));
        assert!(auditar(&cliente(), "E", "Rui, claro! Me diz a data, a cidade e quantos convidados.", &[], false).is_empty());
    }

    #[test]
    fn repetir_a_mesma_observacao_e_barrado() {
        let antes = vec!["Rui, 31 de fevereiro não existe, rs. Me passa a data certinha e o horário do casamento que eu anoto tudo".to_string()];
        let tem = |t: &str| auditar(&cliente(), "E", t, &antes, false).iter().any(|p| p.contains("repete"));
        assert!(tem("Opa, Rui! Só um detalhe: 31 de fevereiro não existe, rs. Me confirma a data certinha?")); // a mesma frase
        assert!(tem("Rui, 31 de fevereiro não existe, rs. Me passa a data certinha e o horário do casamento que eu anoto")); // quase igual
        assert!(auditar(&cliente(), "E", "Rui, os pacotes de casamento vão de mil a três mil reais. Me diz a cidade e os convidados.", &antes, false).is_empty());
        // so resposta ENVIADA conta como repeticao
        assert!(!auditar(&cliente(), "P", &antes[0], &antes, false).iter().any(|p| p.contains("repete")));
        assert!(!repete("", &antes) && !repete("oi", &[]));
    }

    #[test]
    fn ignorar_ou_so_avisar_um_pedido_de_cliente_e_barrado() {
        for cod in ["I", "A"] {
            assert!(auditar(&cliente(), cod, "avisar o dono", &[], false).iter().any(|p| p.contains("PEDIDO")));
        }
        assert!(auditar(&item("Eva", &["bom dia pessoal"]), "I", "", &[], false).is_empty());
        assert!(auditar(&item("CobraBot", &["quanto custa?"]), "I", "", &[], true).is_empty()); // conta automatica: sem revisao
    }

    #[test]
    fn decisao_limpa_passa_direto_sem_incomodar_ninguem() {
        let revisar = |_: &str| -> Option<(String, String)> { panic!("nao devia revisar") };
        let r = decidir_com_revisao(&cliente(), "-", "E", "Claro, Rui! Me passa a data e a cidade.", &[], false, "ab12", &revisar, &|_| panic!("nada a registrar"));
        assert_eq!(r, ("E".into(), "Claro, Rui! Me passa a data e a cidade.".into(), ""));
    }

    #[test]
    fn revisor_e_perguntado_com_a_linha_v_e_a_revisao_dele_vale() {
        let (linhas, logs) = (RefCell::new(vec![]), RefCell::new(vec![]));
        let revisar = |l: &str| {
            linhas.borrow_mut().push(l.to_string());
            Some(("E".to_string(), "Rui, os pacotes vão de mil a três mil reais.".to_string()))
        };
        let r = decidir_com_revisao(&cliente(), "-", "A", "Rui Lopes parece trote", &[], false, "ab12", &revisar, &|l| logs.borrow_mut().push(l.to_string()));
        assert_eq!(r, ("E".into(), "Rui, os pacotes vão de mil a três mil reais.".into(), "revisada"));
        let l = &linhas.borrow()[0];
        assert!(l.starts_with("V|ab12|Rui Lopes 🌝|-|Eu gostaria") && l.contains("|PROPOSTA=A:Rui Lopes parece trote|PROBLEMAS=rotula") && !l.contains('\n'));
        assert!(logs.borrow()[0].contains("REVISADA A->E"));
        // nota ao dono (A/P) sem rotulo vale mesmo que o auditor ainda aponte o pedido sem resposta
        let nota = |_: &str| Some(("A".to_string(), "Rui pediu orçamento de casamento".to_string()));
        assert_eq!(decidir_com_revisao(&cliente(), "-", "E", "É zoeira", &[], false, "ab12", &nota, &|_| ()).2, "revisada");
    }

    #[test]
    fn sem_revisor_o_proposto_nao_sai_neutra_uma_vez_e_depois_cala() {
        let logs = RefCell::new(vec![]);
        let log = |l: &str| logs.borrow_mut().push(l.to_string());
        let r = decidir_com_revisao(&cliente(), "-", "E", "Esse pedido é uma zoeira, rs", &[], false, "ab12", &ninguem, &log);
        assert_eq!(r, ("E".into(), NEUTRA.into(), "neutra"));
        assert!(logs.borrow()[0].contains("NEUTRA no lugar de E"));
        let ja = vec![NEUTRA.to_string()];
        assert_eq!(decidir_com_revisao(&cliente(), "-", "E", "Esse pedido é uma zoeira, rs", &ja, false, "ab12", &ninguem, &log), ("I".into(), String::new(), "autoignorada"));
        // a revisao ainda tem problema: tambem segura
        let ruim = |_: &str| Some(("E".to_string(), "Isso me parece trote mesmo.".to_string()));
        assert_eq!(decidir_com_revisao(&cliente(), "-", "E", "É zoeira", &[], false, "ab12", &ruim, &log), ("E".into(), NEUTRA.into(), "neutra"));
        // resposta que repete, sem revisor: cala (ninguem recebe de novo a mesma coisa)
        let antes = vec!["Rui, 31 de fevereiro não existe, rs. Me passa a data certinha e o horário do casamento".to_string()];
        let rep = "Opa, Rui! Só um detalhe: 31 de fevereiro não existe, rs. Me passa a data certinha e o horário do casamento";
        assert_eq!(decidir_com_revisao(&cliente(), "-", "E", rep, &antes, false, "ab12", &ninguem, &log), ("I".into(), String::new(), "autoignorada"));
        assert!(logs.borrow().iter().any(|l| l.contains("repetiria")));
    }

    #[test]
    fn auditoria_fica_registrada_em_arquivo() {
        let d = std::env::temp_dir().join(format!("braniac-revisao-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let a = Agora { data: crate::tempo::Data::civil(2026, 11, 18).unwrap(), hora: 9, min: 5, seg: 7 };
        decidir_com_revisao(&cliente(), "-", "E", "é zoeira", &[], false, "ab12", &ninguem, &|l| anotar(&d, &a, l));
        let t = std::fs::read_to_string(d.join("revisoes.log")).unwrap();
        assert!(t.starts_with("2026-11-18 09:05:07 | Rui Lopes 🌝 | NEUTRA no lugar de E") && t.ends_with('\n'));
        let _ = std::fs::remove_dir_all(&d);
    }
}
