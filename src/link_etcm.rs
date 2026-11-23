//! e-TCM, a Consulta Publica do TCM-BA (link/etcm.py): quais PDFs o metodo do auditor pede de cada cidade e a ordem de baixar.
//! Prefeitura: dez/2024, dez/2025 e o ultimo mes entregue de 2026 -> PCMGE015/016/056/065/066. Camara: os mesmos meses ->
//! PCMGE015/018. Anual: PCAGO015 de 2024 e 2025. Nome: AAAA-MM_PCMGExxx.pdf, AAAA-MM_CAMARA_PCMGExxx.pdf, AAAA-13_ANUAL_PCAGO015.pdf.
//! REGRAS: um PDF por vez, com pausa de 3 a 5 s depois de cada um (`PAUSA_PDF`); uma baixada por vez em TODO o PC (porta-trava:
//! duas sessoes no mesmo navegador ja cruzaram PDFs de duas cidades); a lista devolvida tem de ser da unidade pedida e a
//! prestacao aberta tambem (o site as vezes devolve a de OUTRA cidade); o que o tribunal nao entregou vira PENDENTE no log e no
//! `_pendencias-tcm.md`, nunca e completado com outra coisa; rodar de novo so baixa o que falta. Bloqueio/captcha = o `Site`
//! devolve Err e a baixada para.
//! Entra por parametro: o `Site` (cada passo no navegador: listas, busca, abrir a prestacao, filtro, link, download e a pausa)
//! e `texto` (pdftotext -layout do PDF para o .txt ao lado). Este arquivo nao fala com a rede.
//! Diferenca do Python: no ultimo mes de 2026 (descoberto na hora) so e pedido o que ainda nao esta na pasta (la pedia as 5
//! classes de novo).
//! Ficou de fora: o JS de cada passo na pagina (PrimeFaces) e as esperas internas dele, a trava do WhatsApp no mesmo navegador
//! e a linha de comando.
#![allow(dead_code)]
use crate::tempo::Agora;
use crate::texto_pt::sem_acento;
use std::collections::{BTreeMap, BTreeSet};
use std::net::TcpListener;
use std::path::Path;

pub const PREF: [&str; 5] = ["PCMGE015", "PCMGE016", "PCMGE056", "PCMGE065", "PCMGE066"];
pub const CAM: [&str; 2] = ["PCMGE015", "PCMGE018"];
pub const PORTA_TRAVA: u16 = 8786;
/// 3 a 5 s entre PDFs (regra do metodo).
pub const PAUSA_PDF: f64 = 4.0;

/// Os passos no site do tribunal. Cada chamada e UM pedido; a ordem e as pausas sao decididas aqui, nunca por quem implementa.
pub trait Site {
    /// Unidades jurisdicionadas: (valor do campo, nome).
    fn unidades(&self) -> Result<Vec<(String, String)>, String>;
    /// Anos de competencia: (valor do campo, texto "2024").
    fn anos(&self) -> Result<Vec<(String, String)>, String>;
    /// Pesquisa (periodicidade "2" mensal, "1" anual): linhas (competencia "12/2024" ou "2024", nome da unidade).
    fn buscar(&self, per: &str, ano: &str, uni: &str) -> Result<Vec<(String, String)>, String>;
    /// Abre a prestacao daquela competencia. Some(comeco do texto da pagina) ou None se a lista de documentos nao abriu.
    fn abrir(&self, competencia: &str) -> Result<Option<String>, String>;
    /// Filtra a classe do documento: "ok", "sem filtro" ou "sem classe".
    fn filtrar(&self, cod: &str) -> Result<String, String>;
    /// Ids dos documentos daquela classe na lista filtrada.
    fn documentos(&self, cod: &str) -> Result<Vec<String>, String>;
    /// Endereco do PDF de um documento (None = o site nao deu o link).
    fn link(&self, id: &str) -> Result<Option<String>, String>;
    /// Baixa UM PDF com aquele nome; devolve o tamanho em bytes.
    fn baixar(&self, url: &str, nome: &str) -> Result<u64, String>;
    fn pausa(&self, segundos: f64);
}

fn norm(s: &str) -> String {
    sem_acento(s).chars().filter(char::is_ascii).collect::<String>().to_uppercase().trim().to_string()
}

/// Segura a porta-trava (o sistema solta sozinho se o processo morrer). Outra baixada rodando: espera ate `espera_s` e desiste.
pub fn trava(porta: u16, espera_s: f64, dormir: &dyn Fn(f64)) -> Result<TcpListener, String> {
    let mut esperou = 0.0;
    loop {
        match TcpListener::bind(("127.0.0.1", porta)) {
            Ok(s) => return Ok(s),
            Err(_) if esperou >= espera_s => return Err("outra baixada do e-TCM continua rodando; tente depois".into()),
            Err(_) => {
                dormir(15.0);
                esperou += 15.0;
            }
        }
    }
}

/// "12/2024" -> ("2024", "12"). Competencia fora do formato e erro (viraria nome de arquivo errado).
fn partes(comp: &str) -> Result<(&str, &str), String> {
    match comp.split_once('/') {
        Some((m, a)) if m.len() == 2 && a.len() == 4 && comp.bytes().all(|b| b.is_ascii_digit() || b == b'/') => Ok((a, m)),
        _ => Err(format!("competência fora do formato MM/AAAA: {comp:?}")),
    }
}

struct Baixada<'a> {
    site: &'a dyn Site,
    nomes: BTreeMap<String, String>, // valor da unidade -> nome
    log: Vec<String>,
    ja: BTreeSet<String>, // PDFs que ja estao na pasta
}

impl Baixada<'_> {
    fn buscar(&self, per: &str, ano: &str, uni: &str) -> Result<Vec<String>, String> {
        // o site as vezes devolve a lista de OUTRA unidade: so vale a linha da unidade pedida
        let alvo = norm(self.nomes.get(uni).map_or("", String::as_str));
        Ok(self.site.buscar(per, ano, uni)?.into_iter().filter(|(_, u)| alvo.is_empty() || norm(u) == alvo).map(|x| x.0).collect())
    }

    /// Os documentos de cada classe, UM por vez, com a pausa depois de cada PDF.
    fn classes(&mut self, prefixo: &str, codigos: &[&str]) -> Result<(), String> {
        for cod in codigos {
            let r = self.site.filtrar(cod)?;
            if r != "ok" {
                self.log.push(format!("PENDENTE {prefixo}{cod}: {r}"));
                continue;
            }
            let ids = self.site.documentos(cod)?;
            if ids.is_empty() {
                self.log.push(format!("PENDENTE {prefixo}{cod}: nao entregue"));
            }
            for (i, id) in ids.iter().enumerate() {
                let nome = format!("{prefixo}{cod}{}.pdf", if i > 0 { format!("_{}", i + 1) } else { String::new() });
                let Some(url) = self.site.link(id)? else {
                    self.log.push(format!("PENDENTE {nome}: sem link"));
                    continue;
                };
                let tam = self.site.baixar(&url, &nome)?;
                self.log.push(format!("ok {nome} ({} KB)", tam / 1024));
                self.site.pausa(PAUSA_PDF);
            }
        }
        Ok(())
    }

    /// Uma competencia de uma unidade. `comp` None = a mais recente (primeira linha). Devolve a competencia aberta.
    fn competencia(&mut self, per: &str, ano: &str, uni: &str, comp: Option<&str>, prefixo: &str, codigos: &[&str]) -> Result<Option<String>, String> {
        let mut comps = self.buscar(per, ano, uni)?;
        if comps.is_empty() || comp.is_some_and(|c| !comps.iter().any(|x| x == c)) {
            self.site.pausa(5.0); // o site as vezes devolve a lista anterior: uma segunda busca
            comps = self.buscar(per, ano, uni)?;
        }
        let alvo = comp.map(String::from).or_else(|| comps.first().cloned()).filter(|a| comps.contains(a));
        let Some(alvo) = alvo else {
            self.log.push(format!("PENDENTE {}: competencia {} nao entregue", if prefixo.is_empty() { ano } else { prefixo }, comp.unwrap_or(ano)));
            return Ok(None);
        };
        let (prefixo, codigos) = if prefixo.is_empty() {
            // mes descoberto agora: do que ja esta na pasta com esse nome so se pede o que falta
            let p = partes(&alvo).map(|(a, m)| format!("{a}-{m}_"))?;
            let falta: Vec<&str> = codigos.iter().copied().filter(|c| !self.ja.contains(&format!("{p}{c}.pdf"))).collect();
            if falta.is_empty() {
                return Ok(Some(alvo));
            }
            (p, falta)
        } else {
            (prefixo.to_string(), codigos.to_vec())
        };
        let Some(pagina) = self.site.abrir(&alvo)? else {
            self.log.push(format!("PENDENTE {prefixo}: a lista de documentos nao abriu"));
            return Ok(Some(alvo));
        };
        // segunda trava: a prestacao aberta e mesmo da unidade pedida
        if let Some(nome) = self.nomes.get(uni).filter(|n| !n.is_empty() && !norm(&pagina).contains(&norm(n))) {
            self.log.push(format!("PENDENTE {prefixo}: a prestacao aberta nao e de {nome}"));
            return Ok(None);
        }
        self.classes(&prefixo, &codigos)?;
        Ok(Some(alvo))
    }
}

/// Baixa o que falta dos documentos de `municipio` para `pasta`/documentos_tcm e devolve o log ("ok ...", "PENDENTE ...", "AVISO ...").
/// `comp` "11/2024": so aquele mes (prefeitura + Camara). `outra` "AGUA E ESGOTO:SAAE": so o demonstrativo de outra unidade do
/// municipio (dez/2024 e dez/2025, ou o mes de `comp`).
#[allow(clippy::too_many_arguments)]
pub fn baixar(site: &dyn Site, porta: u16, municipio: &str, pasta: &Path, comp: Option<&str>, outra: Option<&str>, agora: &Agora, texto: &dyn Fn(&Path, &Path) -> Result<(), String>) -> Result<Vec<String>, String> {
    let docs = pasta.join("documentos_tcm");
    std::fs::create_dir_all(&docs).map_err(|e| e.to_string())?;
    let nome = norm(municipio);
    let pdfs = || -> Vec<String> { std::fs::read_dir(&docs).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".pdf")).collect() };
    {
        let _uma_por_vez = trava(porta, 1800.0, &|s| site.pausa(s))?;
        let unidades = site.unidades()?;
        let ja: BTreeSet<String> = pdfs().into_iter().collect(); // rodar de novo so baixa o que falta
        let mut b = Baixada { site, nomes: unidades.iter().cloned().collect(), log: vec![], ja: ja.clone() };
        let acha = |alvo: String| unidades.iter().find(|(_, t)| norm(t) == alvo).map(|x| x.0.clone());
        let pref = acha(format!("PREFEITURA MUNICIPAL DE {nome}"));
        let mut cam = acha(format!("CAMARA MUNICIPAL DE {nome}"));
        if cam.is_none() {
            // o tribunal grafa errado as vezes: pega a Camara de nome mais parecido
            let perto = unidades.iter().filter_map(|(v, t)| norm(t).strip_prefix("CAMARA MUNICIPAL DE ").map(|c| (crate::escuta::semelhanca(&nome, c), c.to_string(), v))).filter(|x| x.0 >= 0.85).fold(None, |m: Option<(f64, String, &String)>, x| if m.as_ref().is_none_or(|m| x.0 > m.0) { Some(x) } else { m });
            if let Some((_, grafia, v)) = perto {
                cam = Some(v.clone());
                b.log.push(format!("AVISO Camara de {municipio} no e-TCM com outro nome: {grafia}"));
            }
        }
        let pref = pref.ok_or_else(|| format!("municipio {municipio} nao achado no e-TCM"))?;
        let anos: BTreeMap<String, String> = site.anos()?.into_iter().map(|(v, t)| (t, v)).collect();
        let ano = |a: &str| anos.get(a).cloned().ok_or_else(|| format!("ano {a} não está na lista do e-TCM"));
        let falta = |pre: &str, cods: &[&'static str]| -> Vec<&'static str> { cods.iter().copied().filter(|c| !ja.contains(&format!("{pre}{c}.pdf"))).collect() };
        if let Some(outra) = outra {
            // outra unidade do municipio (autarquia que presta contas separada)
            let (termo, sig) = outra.split_once(':').unwrap_or((outra, ""));
            b.log.push(format!("unidades de {municipio}: {}", unidades.iter().filter(|(_, t)| norm(t).contains(&nome)).map(|x| x.1.as_str()).collect::<Vec<_>>().join("; ")));
            match unidades.iter().find(|(_, t)| norm(t).contains(&norm(termo)) && norm(t).contains(&nome)) {
                None => b.log.push(format!("PENDENTE unidade '{termo}' de {municipio}: nao achada no e-TCM")),
                Some((uni, _)) => {
                    for cp in comp.map_or(vec!["12/2024", "12/2025"], |c| vec![c]) {
                        let (a, m) = partes(cp)?;
                        let pre = format!("{a}-{m}_{}_", if sig.is_empty() { "OUTRA" } else { sig });
                        let f = falta(&pre, &["PCMGE015", "PCMGE065"]);
                        if !f.is_empty() {
                            b.competencia("2", &ano(a)?, uni, Some(cp), &pre, &f)?;
                        }
                    }
                }
            }
        } else if let Some(comp) = comp {
            let (a, m) = partes(comp)?;
            for (uni, pre, cods) in [(Some(&pref), format!("{a}-{m}_"), &PREF[..]), (cam.as_ref(), format!("{a}-{m}_CAMARA_"), &CAM[..])] {
                let f = falta(&pre, cods);
                if let (Some(uni), false) = (uni, f.is_empty()) {
                    b.competencia("2", &ano(a)?, uni, Some(comp), &pre, &f)?;
                }
            }
        } else {
            let mut m26: Vec<&str> = ja.iter().filter(|n| n.starts_with("2026-") && n.as_bytes().get(7) == Some(&b'_') && !n.contains("CAMARA")).map(|n| &n[..7]).collect();
            m26.sort();
            let ult26 = match m26.last() {
                Some(m) if falta(&format!("{m}_"), &PREF).is_empty() => Some(format!("{}/2026", &m[5..7])),
                _ => b.competencia("2", &ano("2026")?, &pref, None, "", &PREF)?,
            };
            for a in ["2024", "2025"] {
                let f = falta(&format!("{a}-12_"), &PREF);
                if !f.is_empty() {
                    b.competencia("2", &ano(a)?, &pref, Some(&format!("12/{a}")), &format!("{a}-12_"), &f)?;
                }
            }
            match &cam {
                Some(cam) => {
                    for (a, cp) in [("2024", Some("12/2024".to_string())), ("2025", Some("12/2025".to_string())), ("2026", ult26)] {
                        let Some(cp) = cp else { continue };
                        let (ac, m) = partes(&cp)?;
                        let pre = format!("{ac}-{m}_CAMARA_");
                        let f = falta(&pre, &CAM);
                        if !f.is_empty() {
                            b.competencia("2", &ano(a)?, cam, Some(&cp), &pre, &f)?;
                        }
                    }
                }
                None => b.log.push(format!("PENDENTE Camara de {municipio}: unidade nao achada no e-TCM")),
            }
            for a in ["2024", "2025"] {
                if !falta(&format!("{a}-13_ANUAL_"), &["PCAGO015"]).is_empty() {
                    b.competencia("1", &ano(a)?, &pref, None, &format!("{a}-13_ANUAL_"), &["PCAGO015"])?;
                }
            }
        }
        site.pausa(5.0);
        let log = b.log;
        // texto ao lado, para ler sem abrir o PDF
        for pdf in pdfs() {
            let (p, t) = (docs.join(&pdf), docs.join(&pdf).with_extension("txt"));
            if !t.exists() {
                texto(&p, &t)?;
            }
        }
        let pend: Vec<&String> = log.iter().filter(|x| x.starts_with("PENDENTE")).collect();
        if !pend.is_empty() {
            use std::io::Write;
            let (y, m, d) = agora.data.partes();
            let linhas: String = pend.iter().map(|x| format!("- {municipio}: {} ({d:02}/{m:02}/{y:04})\n", &x[9..])).collect();
            std::fs::OpenOptions::new().append(true).create(true).open(pasta.join("_pendencias-tcm.md")).and_then(|mut f| f.write_all(linhas.as_bytes())).map_err(|e| e.to_string())?;
        }
        Ok(log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;
    use std::cell::{Cell, RefCell};

    /// Um e-TCM de mentira: anota cada pedido e reclama se um PDF for pedido antes da pausa do anterior.
    #[derive(Default)]
    struct Falso {
        pedidos: RefCell<Vec<String>>,
        baixando: Cell<bool>,
        /// a 1a busca devolve a lista de outra cidade
        lista_trocada: Cell<bool>,
        prestacao_de_outra: bool,
        sem_2026: bool,
        bloqueado: bool,
        docs: std::path::PathBuf,
    }

    impl Falso {
        fn anota(&self, p: String) {
            self.pedidos.borrow_mut().push(p);
        }
    }

    impl Site for Falso {
        fn unidades(&self) -> Result<Vec<(String, String)>, String> {
            if self.bloqueado {
                return Err("captcha na Consulta Publica".into());
            }
            Ok([("10", "Prefeitura Municipal de ITATIM"), ("11", "Camara Municipal de ITATIMM"), ("12", "Serviço Autônomo de Água e Esgoto de Itatim"), ("20", "Prefeitura Municipal de ITAGIMIRIM"), ("21", "Camara Municipal de ITAGIMIRIM")].map(|(v, t)| (v.to_string(), t.to_string())).to_vec())
        }
        fn anos(&self) -> Result<Vec<(String, String)>, String> {
            Ok([("a24", "2024"), ("a25", "2025"), ("a26", "2026")].map(|(v, t)| (v.to_string(), t.to_string())).to_vec())
        }
        fn buscar(&self, per: &str, ano: &str, uni: &str) -> Result<Vec<(String, String)>, String> {
            assert!(!self.baixando.get(), "busca no meio de um PDF");
            self.anota(format!("buscar {per} {ano} {uni}"));
            let nome = self.unidades()?.into_iter().find(|x| x.0 == uni).unwrap().1;
            if self.lista_trocada.replace(false) {
                return Ok(vec![("09/2026".into(), "Prefeitura Municipal de ITAGIMIRIM".into())]);
            }
            let a = &ano[1..];
            Ok(match (per, a) {
                ("1", _) => vec![(format!("20{a}"), nome)],
                (_, "26") if self.sem_2026 => vec![],
                (_, "26") => vec![("09/2026".into(), nome.clone()), ("08/2026".into(), nome)],
                _ => (1..=12).rev().map(|m| (format!("{m:02}/20{a}"), nome.clone())).collect(),
            })
        }
        fn abrir(&self, competencia: &str) -> Result<Option<String>, String> {
            self.anota(format!("abrir {competencia}"));
            let ultima = self.pedidos.borrow().iter().rev().find(|p| p.starts_with("buscar")).unwrap().clone();
            let uni = ultima.rsplit(' ').next().unwrap().to_string();
            let nome = self.unidades()?.into_iter().find(|x| x.0 == uni).unwrap().1;
            Ok(Some(if self.prestacao_de_outra { "Prestação de Contas - Prefeitura Municipal de ITAGIMIRIM".into() } else { format!("Prestação de Contas\n{nome}\n{competencia}") }))
        }
        fn filtrar(&self, cod: &str) -> Result<String, String> {
            Ok(if cod == "PCMGE056" { "sem classe" } else { "ok" }.into())
        }
        fn documentos(&self, cod: &str) -> Result<Vec<String>, String> {
            Ok(match cod {
                "PCMGE066" => vec![],
                "PCMGE065" => vec!["65a".into(), "65b".into()],
                c => vec![c.to_lowercase()],
            })
        }
        fn link(&self, id: &str) -> Result<Option<String>, String> {
            assert!(!self.baixando.get(), "link pedido antes da pausa do PDF anterior");
            Ok((id != "pcmge018").then(|| format!("https://tribunal.invalid/{id}")))
        }
        fn baixar(&self, url: &str, nome: &str) -> Result<u64, String> {
            assert!(!self.baixando.replace(true), "dois PDFs ao mesmo tempo");
            self.anota(format!("baixar {nome}"));
            std::fs::write(self.docs.join(nome), url).map_err(|e| e.to_string())?;
            Ok(5000)
        }
        fn pausa(&self, s: f64) {
            self.baixando.set(false);
            self.anota(format!("pausa {s}"));
        }
    }

    fn pasta(nome: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("braniac-link_etcm-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(p.join("documentos_tcm")).unwrap();
        p
    }

    fn agora() -> Agora {
        Agora { data: Data::civil(2026, 11, 18).unwrap(), hora: 9, min: 0, seg: 0 }
    }

    fn com_txt(_: &Path, txt: &Path) -> Result<(), String> {
        std::fs::write(txt, "texto").map_err(|e| e.to_string())
    }

    #[test]
    fn uma_baixada_do_etcm_por_vez() {
        let dormidas = Cell::new(0);
        let dormir = |_: f64| dormidas.set(dormidas.get() + 1);
        let primeira = trava(18786, 0.0, &dormir).unwrap();
        assert!(trava(18786, 0.0, &dormir).unwrap_err().contains("outra baixada"));
        assert!(trava(18786, 30.0, &dormir).is_err() && dormidas.get() == 2); // espera de 15 em 15 s e desiste
        drop(primeira);
        trava(18786, 0.0, &dormir).unwrap(); // soltou: a proxima entra
        assert!((3.0..=5.0).contains(&PAUSA_PDF));
    }

    #[test]
    fn baixa_um_pdf_por_vez_com_pausa_e_anota_o_que_falta() {
        let p = pasta("ordem");
        let site = Falso { docs: p.join("documentos_tcm"), ..Default::default() };
        let log = baixar(&site, 18787, "Itatim", &p, None, None, &agora(), &com_txt).unwrap();
        let ped = site.pedidos.borrow().clone();
        // depois de CADA PDF vem a pausa de 3 a 5 s, antes de qualquer outro pedido
        let baixados: Vec<usize> = ped.iter().enumerate().filter(|x| x.1.starts_with("baixar")).map(|x| x.0).collect();
        assert!(baixados.len() == 17 && baixados.iter().all(|i| ped[i + 1] == "pausa 4"), "{ped:?}");
        // prefeitura: o mes mais recente de 2026 (primeira linha), dez/2024 e dez/2025; Camara nos mesmos meses; anual
        assert_eq!(ped.iter().filter(|x| x.starts_with("abrir")).collect::<Vec<_>>(), ["abrir 09/2026", "abrir 12/2024", "abrir 12/2025", "abrir 12/2024", "abrir 12/2025", "abrir 09/2026", "abrir 2024", "abrir 2025"]);
        assert_eq!(ped.iter().filter(|x| x.starts_with("baixar 2026")).collect::<Vec<_>>(), ["baixar 2026-09_PCMGE015.pdf", "baixar 2026-09_PCMGE016.pdf", "baixar 2026-09_PCMGE065.pdf", "baixar 2026-09_PCMGE065_2.pdf", "baixar 2026-09_CAMARA_PCMGE015.pdf"]);
        assert!(ped.contains(&"baixar 2025-13_ANUAL_PCAGO015.pdf".to_string()) && ped.last().unwrap() == "pausa 5");
        assert_eq!(log[0], "AVISO Camara de Itatim no e-TCM com outro nome: ITATIMM");
        for esperado in ["ok 2024-12_PCMGE015.pdf (4 KB)", "PENDENTE 2024-12_PCMGE056: sem classe", "PENDENTE 2025-12_PCMGE066: nao entregue", "PENDENTE 2026-09_CAMARA_PCMGE018.pdf: sem link"] {
            assert!(log.contains(&esperado.to_string()), "{esperado}: {log:?}");
        }
        let pend = std::fs::read_to_string(p.join("_pendencias-tcm.md")).unwrap();
        assert!(pend.contains("- Itatim: 2024-12_PCMGE056: sem classe (18/11/2026)\n") && pend.lines().count() == 9);
        assert!(p.join("documentos_tcm/2026-09_PCMGE065_2.txt").exists());

        // rodar de novo: so o que falta (as classes que o tribunal nao entregou); nenhum PDF que ja esta na pasta
        let de_novo = Falso { docs: p.join("documentos_tcm"), ..Default::default() };
        let log = baixar(&de_novo, 18787, "Itatim", &p, None, None, &agora(), &com_txt).unwrap();
        assert!(!de_novo.pedidos.borrow().iter().any(|x| x.starts_with("baixar") || x == "abrir 2024"));
        assert!(log.iter().filter(|x| x.starts_with("PENDENTE")).count() == 9 && std::fs::read_to_string(p.join("_pendencias-tcm.md")).unwrap().lines().count() == 18);
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn um_mes_so_outra_unidade_e_as_travas_de_unidade() {
        let p = pasta("mes");
        let site = Falso { docs: p.join("documentos_tcm"), lista_trocada: Cell::new(true), ..Default::default() };
        // a 1a lista veio de outra cidade: nao serve, busca de novo
        let log = baixar(&site, 18788, "Itatim", &p, Some("11/2024"), None, &agora(), &com_txt).unwrap();
        let ped = site.pedidos.borrow().clone();
        assert_eq!(&ped[..4], ["buscar 2 a24 10", "pausa 5", "buscar 2 a24 10", "abrir 11/2024"]);
        assert!(log.contains(&"ok 2024-11_CAMARA_PCMGE015.pdf (4 KB)".to_string()) && !ped.iter().any(|x| x.contains("2026") || x.contains("ANUAL")));
        // outra unidade do municipio: so o demonstrativo dela
        let site = Falso { docs: p.join("documentos_tcm"), ..Default::default() };
        let log = baixar(&site, 18788, "Itatim", &p, None, Some("AGUA E ESGOTO:SAAE"), &agora(), &com_txt).unwrap();
        assert!(log[1].starts_with("unidades de Itatim: Prefeitura Municipal de ITATIM; Camara") && log.contains(&"ok 2025-12_SAAE_PCMGE065_2.pdf (4 KB)".to_string()));
        assert!(site.pedidos.borrow().iter().filter(|x| x.starts_with("buscar")).all(|x| x.ends_with(" 12")));
        let log = baixar(&site, 18788, "Itatim", &p, None, Some("TRANSPORTE"), &agora(), &com_txt).unwrap();
        assert_eq!(log[2], "PENDENTE unidade 'TRANSPORTE' de Itatim: nao achada no e-TCM");
        // a prestacao aberta e de outra cidade: nada e baixado
        let q = pasta("outra");
        let site = Falso { docs: q.join("documentos_tcm"), prestacao_de_outra: true, sem_2026: true, ..Default::default() };
        let log = baixar(&site, 18788, "Itatim", &q, None, None, &agora(), &com_txt).unwrap();
        assert!(!site.pedidos.borrow().iter().any(|x| x.starts_with("baixar")));
        assert!(log.contains(&"PENDENTE a26: competencia a26 nao entregue".to_string()) && log.contains(&"PENDENTE 2024-12_: a prestacao aberta nao e de Prefeitura Municipal de ITATIM".to_string()));
        assert!(!log.iter().any(|x| x.contains("2026-"))); // sem 2026 da prefeitura, a Camara de 2026 nao e pedida
        // cidade que nao existe, competencia torta e bloqueio do site param a baixada
        assert!(baixar(&site, 18788, "Itaju", &q, None, None, &agora(), &com_txt).unwrap_err().contains("municipio Itaju nao achado"));
        assert!(baixar(&site, 18788, "Itatim", &q, Some("2024-11"), None, &agora(), &com_txt).unwrap_err().contains("fora do formato"));
        assert!(baixar(&site, 18788, "Itatim", &q, Some("11/2019"), None, &agora(), &com_txt).unwrap_err().contains("ano 2019 não está"));
        let fora = Falso { bloqueado: true, ..Default::default() };
        assert!(baixar(&fora, 18788, "Itatim", &q, None, None, &agora(), &com_txt).unwrap_err().contains("captcha"));
        let (_, _) = (std::fs::remove_dir_all(&p), std::fs::remove_dir_all(&q));
    }
}
