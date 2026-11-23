//! Leads do Google Maps (braniac/leads_maps.py): lista de empresas de um tipo de negocio numa cidade, pelo kit local de raspagem.
//! Aqui ficam as travas e o parse: pedido valido, sem palavra de segredo, ate 40 por busca, 3 buscas por dia (dados/leads_maps_uso.json),
//! SEM e-mails nem redes sociais (so dado comercial), sem proxy. Rodar o kit (processo) entra como closure que recebe os argumentos.
//! Nao envia mensagem para ninguem: quem decide o contato e o dono, e so quando ele pede a prospeccao.
#![allow(dead_code)]
use crate::tempo::Agora;
use regex::Regex;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

pub const MAX_POR_BUSCA: i64 = 40;
pub const MAX_BUSCAS_DIA: i64 = 3;

fn limpo(t: &str) -> String {
    t.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(60).collect()
}

/// (tipo, cidade) limpos, ou o motivo da recusa.
pub fn validar(tipo: &str, cidade: &str) -> Result<(String, String), String> {
    let (tipo, cidade) = (limpo(tipo), limpo(cidade));
    if tipo.chars().count() < 3 || cidade.chars().count() < 3 {
        return Err("preciso do tipo de negocio e da cidade (ex.: 'padarias', 'Cidade, UF')".into());
    }
    if Regex::new(r"(?i)senha|token|cpf|cart[aã]o|pix|chave de acesso").expect("regex fixa").is_match(&format!("{tipo}{cidade}")) {
        return Err("pedido com palavra de segredo: recusado".into());
    }
    Ok((tipo, cidade))
}

fn usos_hoje(dados: &Path, hoje: &str) -> i64 {
    let d = std::fs::read_to_string(dados.join("leads_maps_uso.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).unwrap_or(Value::Null);
    if d["dia"] == hoje { d["n"].as_i64().unwrap_or(0) } else { 0 }
}

pub fn slug(tipo: &str, cidade: &str) -> String {
    let ascii: String = format!("{tipo}-{cidade}").to_lowercase().chars().filter(char::is_ascii).collect();
    Regex::new("[^a-z0-9]+").expect("regex fixa").replace_all(&ascii, "-").trim_matches('-').chars().take(50).collect()
}

/// Argumentos do scrape.py do kit. Os tetos e os "--no-email --no-socials" sao fixos: nao ha como pedir mais.
pub fn argumentos(tipo: &str, cidade: &str, limite: i64, csv: &Path) -> Vec<String> {
    let limite = (if limite == 0 { 20 } else { limite }).clamp(1, MAX_POR_BUSCA);
    let (q, lim, out) = (format!("{tipo} em {cidade}"), limite.to_string(), csv.display().to_string());
    [q.as_str(), "--city", cidade, "--depth", "2", "--lang", "pt", "--country", "br", "--no-email", "--no-socials", "--limit", lim.as_str(), "--out", out.as_str()].iter().map(|s| s.to_string()).collect()
}

/// CSV (com aspas, virgula e quebra de linha dentro de campo) -> uma lista de {coluna: valor}.
pub fn ler_csv(texto: &str) -> Vec<Map<String, Value>> {
    let (mut linhas, mut linha, mut campo, mut aspas) = (vec![], vec![], String::new(), false);
    let mut cs = texto.trim_start_matches('\u{feff}').chars().peekable();
    while let Some(c) = cs.next() {
        match c {
            '"' if aspas && cs.peek() == Some(&'"') => {
                campo.push('"');
                cs.next();
            }
            '"' => aspas = !aspas,
            ',' if !aspas => linha.push(std::mem::take(&mut campo)),
            '\r' if !aspas => {}
            '\n' if !aspas => {
                linha.push(std::mem::take(&mut campo));
                linhas.push(std::mem::take(&mut linha));
            }
            _ => campo.push(c),
        }
    }
    if !campo.is_empty() || !linha.is_empty() {
        linha.push(campo);
        linhas.push(linha);
    }
    let mut it = linhas.into_iter().filter(|l| l.iter().any(|c| !c.is_empty()));
    let Some(cab) = it.next() else { return vec![] };
    it.map(|l| cab.iter().cloned().zip(l.into_iter().map(Value::String)).collect()).collect()
}

/// Devolve (caminho do csv, linhas). `kit` = pasta do raspador; `saida` = onde o CSV fica (fora do Git).
/// `raspar(argumentos)` sobe o motor do kit, roda o scrape.py com esses argumentos e para o motor; Err = a busca nao terminou.
pub fn buscar(tipo: &str, cidade: &str, limite: i64, dados: &Path, kit: &Path, saida: &Path, agora: &Agora, raspar: &dyn Fn(&[String]) -> Result<(), String>) -> Result<(PathBuf, Vec<Map<String, Value>>), String> {
    let (tipo, cidade) = validar(tipo, cidade)?;
    if !kit.join("scripts").join("scrape.py").exists() {
        return Err(format!("kit do scraper nao encontrado em {}", kit.display()));
    }
    let hoje = agora.data.iso();
    let n = usos_hoje(dados, &hoje);
    if n >= MAX_BUSCAS_DIA {
        return Err(format!("teto de {MAX_BUSCAS_DIA} buscas por dia (para nao sobrecarregar nem irritar o Google)"));
    }
    // conta a busca antes de rodar: busca que falha tambem gasta o dia
    std::fs::create_dir_all(dados).and_then(|_| std::fs::write(dados.join("leads_maps_uso.json"), json!({ "dia": hoje, "n": n + 1 }).to_string())).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(saida).map_err(|e| e.to_string())?;
    let csv = saida.join(format!("{}-{}.csv", agora.carimbo().chars().take(13).collect::<String>(), slug(&tipo, &cidade)));
    raspar(&argumentos(&tipo, &cidade, limite, &csv)).map_err(|e| format!("a busca nao terminou: {}", e.chars().rev().take(250).collect::<Vec<_>>().into_iter().rev().collect::<String>()))?;
    let texto = std::fs::read_to_string(&csv).map_err(|_| "a busca nao terminou: o kit nao gravou o CSV".to_string())?;
    Ok((csv, ler_csv(&texto)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;

    fn tmp(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-leads_maps-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("kit").join("scripts")).unwrap();
        std::fs::write(d.join("kit").join("scripts").join("scrape.py"), "").unwrap();
        d
    }

    fn agora(dia: i64) -> Agora {
        Agora { data: Data::civil(2026, 11, dia).unwrap(), hora: 9, min: 5, seg: 7 }
    }

    #[test]
    fn pedido_invalido_segredo_e_kit_ausente_nao_rodam_nada() {
        let t = tmp("a");
        let nunca = |_: &[String]| -> Result<(), String> { panic!("nao raspa") };
        let b = |tipo: &str, cidade: &str, kit: &str| buscar(tipo, cidade, 20, &t.join("dados"), &t.join(kit), &t.join("leads"), &agora(18), &nunca).unwrap_err();
        assert!(b("", "Vila Azul", "kit").contains("tipo de negocio") && b("padarias", " a ", "kit").contains("tipo de negocio"));
        assert!(b("padarias com senha", "Vila Azul, SP", "kit").contains("segredo") && b("padarias", "Vila do Cartão", "kit").contains("segredo"));
        assert!(b("padarias", "Vila Azul, SP", "nao-existe").starts_with("kit do scraper nao encontrado"));
        assert!(!t.join("dados").exists() && !t.join("leads").exists()); // recusa nao gasta o dia nem cria pasta
    }

    #[test]
    fn teto_de_tres_buscas_por_dia_e_de_quarenta_por_busca() {
        let t = tmp("b");
        let pedidos = std::cell::RefCell::new(vec![]);
        let raspa = |args: &[String]| -> Result<(), String> {
            pedidos.borrow_mut().push(args.to_vec());
            std::fs::write(args.last().unwrap(), "\u{feff}title,phone,address\r\nPadaria Sol,(11) 3043-0000,\"Rua A, 1\"\r\n\"Pão \"\"Bom\"\"\",,\"Rua B\nfundos\"\r\n").map_err(|e| e.to_string())
        };
        let b = |dia: i64, limite: i64| buscar("  Padarias   artesanais ", "Vila Açu, SP", limite, &t.join("dados"), &t.join("kit"), &t.join("leads"), &agora(dia), &raspa);
        let (csv, linhas) = b(18, 500).unwrap();
        assert_eq!(csv.file_name().unwrap().to_string_lossy(), "20261118-0905-padarias-artesanais-vila-au-sp.csv");
        assert_eq!(linhas.len(), 2);
        assert_eq!((linhas[0]["title"].as_str(), linhas[0]["address"].as_str(), linhas[1]["title"].as_str(), linhas[1]["address"].as_str()), (Some("Padaria Sol"), Some("Rua A, 1"), Some("Pão \"Bom\""), Some("Rua B\nfundos")));
        let a = pedidos.borrow()[0].clone();
        assert_eq!(a[0], "Padarias artesanais em Vila Açu, SP");
        assert!(a.contains(&"--no-email".to_string()) && a.contains(&"--no-socials".to_string()) && !a.iter().any(|x| x.contains("proxy")));
        assert_eq!(a[a.iter().position(|x| x == "--limit").unwrap() + 1], "40");
        assert!(b(18, 0).is_ok() && b(18, -5).is_ok());
        assert_eq!(pedidos.borrow()[1][12], "20");
        assert_eq!(pedidos.borrow()[2][12], "1");
        assert!(b(18, 20).unwrap_err().starts_with("teto de 3 buscas por dia"));
        assert_eq!(pedidos.borrow().len(), 3);
        assert!(b(19, 20).is_ok()); // dia novo zera
    }

    #[test]
    fn busca_que_falha_avisa_e_gasta_o_dia() {
        let t = tmp("c");
        let b = |r: &dyn Fn(&[String]) -> Result<(), String>| buscar("padarias", "Vila Azul", 20, &t.join("dados"), &t.join("kit"), &t.join("leads"), &agora(18), r);
        assert_eq!(b(&|_| Err("motor nao subiu".into())).unwrap_err(), "a busca nao terminou: motor nao subiu");
        assert_eq!(b(&|_| Ok(())).unwrap_err(), "a busca nao terminou: o kit nao gravou o CSV");
        assert_eq!(usos_hoje(&t.join("dados"), "2026-11-18"), 2);
        assert!(ler_csv("").is_empty() && ler_csv("title,phone\n").is_empty());
    }
}
