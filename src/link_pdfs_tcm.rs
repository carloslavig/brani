//! PDFs do tribunal de contas que vao junto do video (link/pdfs_tcm.py). Regras: so sobe documento de prefeitura com nome
//! AAAA-MM_NOME.pdf (outro PDF nunca sobe); so a serie 100-899 (a 900 e as capitais 1001+ ficam de fora); o que ja subiu fica em
//! dados/parceiro/pdfs_enviados.json e nao sobe de novo; para no primeiro erro (em geral o limite diario do site).
//! Daqui NADA sai para fora: copiar arquivo e mandar ao site sao closures (`copia`, `enviar`) que quem chama decide, por comando
//! de pessoa ou pela rotina. Ficou de fora: a linha de comando e os caminhos fixos do PC (viraram o parametro `Locais`).
#![allow(dead_code)]
use crate::texto_pt::sem_acento;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const SUB: &str = "pdfs-tcm";

pub struct Locais {
    /// Pasta do pacote de producao (tem videos/NNN/preparo.json e RECEITAS-DESPESAS-<CIDADE>/documentos_tcm).
    pub pacote: PathBuf,
    /// Onde ficam as pastas de entrega NNN-CIDADE.
    pub site: PathBuf,
    /// Onde montar a pasta temporaria de quem ainda nao tem video.
    pub tmp: PathBuf,
}

/// So documento da prefeitura no tribunal: AAAA-MM_NOME.pdf.
pub fn doc(nome: &str) -> bool {
    static DOC: OnceLock<Regex> = OnceLock::new();
    DOC.get_or_init(|| Regex::new(r"(?i)^[0-9]{4}-[0-9]{2}_[A-Za-z0-9_.-]{3,80}\.pdf$").expect("regex fixa")).is_match(nome)
}

/// "Barra do Rocha" -> "BARRA-DO-ROCHA".
pub fn asc(s: &str) -> String {
    let limpo: String = sem_acento(s).chars().filter(char::is_ascii).map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { '-' }).collect();
    limpo.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-")
}

/// A serie que tem PDF de tribunal para subir: 100 a 899.
pub fn na_serie(numero: &str) -> bool {
    !numero.is_empty() && numero.chars().all(|c| c.is_ascii_digit()) && numero.parse::<u32>().is_ok_and(|n| (100..900).contains(&n))
}

fn json_de(arq: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(arq).ok()?).ok()
}

/// Pasta documentos_tcm da cidade: o campo "dados" do preparo.json ou RECEITAS-DESPESAS-<CIDADE>.
pub fn pasta_pdfs(l: &Locais, numero: &str, cidade: &str) -> Option<PathBuf> {
    let dados = json_de(&l.pacote.join("videos").join(numero).join("preparo.json")).and_then(|p| p["dados"].as_str().filter(|d| !d.is_empty()).map(String::from));
    dados.into_iter().chain([format!("RECEITAS-DESPESAS-{}", asc(cidade))]).map(|nome| l.pacote.join(nome).join("documentos_tcm")).find(|p| p.is_dir())
}

/// A pasta NNN-CIDADE da entrega (sem as versoes -v2 etc., que sao sobras de refacao).
pub fn pasta_entrega(l: &Locais, numero: &str, cidade: &str) -> Option<PathBuf> {
    let exata = l.site.join(format!("{numero}-{}", asc(cidade)));
    if exata.is_dir() {
        return Some(exata);
    }
    let versao = Regex::new(r"-v[0-9]+$").expect("regex fixa");
    let mut achadas: Vec<PathBuf> = std::fs::read_dir(&l.site).ok()?.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    achadas.sort();
    achadas.into_iter().find(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(&format!("{numero}-")) && !versao.is_match(n)))
}

fn pdfs_validos(pasta: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(pasta).into_iter().flatten().flatten().map(|e| e.path())
        .filter(|p| p.is_file() && p.file_name().and_then(|n| n.to_str()).is_some_and(doc)).collect();
    v.sort();
    v
}

/// Copia os PDFs validos para <destino>/pdfs-tcm (os .txt sao extracao e nao vao). Devolve quantos PDFs validos ficaram la, ou None
/// se faltou a origem ou o destino. `destino` None = a pasta de entrega da cidade.
pub fn copiar(l: &Locais, numero: &str, cidade: &str, destino: Option<&Path>, copia: &dyn Fn(&Path, &Path) -> Result<(), String>) -> Option<usize> {
    let origem = pasta_pdfs(l, numero, cidade)?;
    let destino = destino.map(Path::to_path_buf).or_else(|| pasta_entrega(l, numero, cidade))?;
    let sub = destino.join(SUB);
    std::fs::create_dir_all(&sub).ok()?;
    for f in pdfs_validos(&origem) {
        let alvo = sub.join(f.file_name()?);
        let tam = |p: &Path| std::fs::metadata(p).map(|m| m.len()).ok();
        if tam(&alvo).is_none() || tam(&alvo) != tam(&f) {
            copia(&f, &alvo).ok()?;
        }
    }
    Some(pdfs_validos(&sub).len())
}

/// {numero: nome} da serie 100-899: os ja entregues (dados/parceiro/orquestrador.json, "enviados": ["NNN Nome"]) e os que so tem preparo.
pub fn cidades(dados: &Path, l: &Locais) -> BTreeMap<String, String> {
    let mut cid = BTreeMap::new();
    let o = json_de(&dados.join("parceiro").join("orquestrador.json")).unwrap_or(Value::Null);
    for e in o["enviados"].as_array().into_iter().flatten().filter_map(|e| e.as_str()) {
        if let Some((n, nome)) = e.split_once(' ') {
            cid.insert(n.to_string(), nome.to_string());
        }
    }
    for (n, nome) in [("143", "Itatim"), ("153", "Barra do Rocha")] {
        cid.insert(n.into(), nome.into()); // entregues antes da esteira de 50
    }
    for d in std::fs::read_dir(l.pacote.join("videos")).into_iter().flatten().flatten() {
        let n = d.file_name().to_string_lossy().to_string();
        if na_serie(&n) && !cid.contains_key(&n) {
            if let Some(nome) = json_de(&d.path().join("preparo.json")).and_then(|p| p["nome"].as_str().map(String::from)) {
                cid.insert(n, nome);
            }
        }
    }
    cid.retain(|n, _| na_serie(n)); // serie 900 e capitais nunca sobem, venham de onde vierem
    cid
}

fn titulo(nome: &str) -> String {
    let mut inicio = true;
    nome.chars().map(|c| {
        let r = if inicio { c.to_uppercase().next().unwrap_or(c) } else { c.to_lowercase().next().unwrap_or(c) };
        inicio = !c.is_alphabetic();
        r
    }).collect()
}

/// Sobe os PDFs de um video. Sem pasta de entrega (video ainda nao feito), monta uma temporaria com o meta do preparo.json.
pub fn enviar_numero(l: &Locais, n: &str, nome: &str, copia: &dyn Fn(&Path, &Path) -> Result<(), String>, enviar: &dyn Fn(&Path) -> bool) -> bool {
    if !na_serie(n) {
        return false;
    }
    if let Some(d) = pasta_entrega(l, n, nome) {
        return copiar(l, n, nome, None, copia).is_some_and(|q| q > 0) && enviar(&d);
    }
    let Some(prep) = json_de(&l.pacote.join("videos").join(n).join("preparo.json")) else { return false };
    let Some(projeto) = prep["projeto"].as_str() else { return false };
    let d = l.tmp.join(format!("{n}-{}", asc(nome)));
    let feito = (|| {
        std::fs::create_dir_all(&d).ok()?;
        let cidade = if nome.chars().any(char::is_alphabetic) && nome == nome.to_uppercase() { titulo(nome) } else { nome.to_string() };
        std::fs::write(d.join("meta.json"), json!({ "numero": n, "cidade": cidade, "projeto": projeto }).to_string()).ok()?;
        Some(copiar(l, n, nome, Some(&d), copia)? > 0 && enviar(&d))
    })();
    let _ = std::fs::remove_dir_all(&d);
    feito.unwrap_or(false)
}

/// Sobe os PDFs de toda cidade que ainda nao esta em pdfs_enviados.json. `so` vazio = todas. Devolve (numero, subiu?) de cada
/// tentativa; para no primeiro erro (roda de novo amanha).
pub fn enviar_faltando(dados: &Path, l: &Locais, so: &[&str], copia: &dyn Fn(&Path, &Path) -> Result<(), String>, enviar: &dyn Fn(&Path) -> bool) -> Vec<(String, bool)> {
    let reg = dados.join("parceiro").join("pdfs_enviados.json");
    let mut feitos: Vec<String> = json_de(&reg).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
    let mut todas: Vec<(String, String)> = cidades(dados, l).into_iter().collect();
    todas.sort_by_key(|(n, _)| n.parse::<u32>().unwrap_or(0));
    let mut out = vec![];
    for (n, nome) in todas {
        if (!so.is_empty() && !so.contains(&n.as_str())) || feitos.contains(&n) || pasta_pdfs(l, &n, &nome).is_none() {
            continue;
        }
        let ok = enviar_numero(l, &n, &nome, copia, enviar);
        if ok {
            feitos.push(n.clone());
            feitos.sort_by_key(|x| x.parse::<u32>().unwrap_or(0));
            // sem registro, o proximo ciclo subiria de novo: melhor parar aqui
            if std::fs::create_dir_all(dados.join("parceiro")).and_then(|_| std::fs::write(&reg, json!(feitos).to_string())).is_err() {
                out.push((n, false));
                break;
            }
        }
        out.push((n, ok));
        if !ok {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn locais(nome: &str) -> (PathBuf, Locais) {
        let p = std::env::temp_dir().join(format!("braniac-link_pdfs_tcm-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        for d in ["pacote/videos", "site", "tmp", "dados/parceiro"] {
            std::fs::create_dir_all(p.join(d)).unwrap();
        }
        let l = Locais { pacote: p.join("pacote"), site: p.join("site"), tmp: p.join("tmp") };
        (p, l)
    }

    fn copia(a: &Path, b: &Path) -> Result<(), String> {
        std::fs::copy(a, b).map(|_| ()).map_err(|e| e.to_string())
    }

    fn origem(l: &Locais, cidade: &str, nomes: &[&str]) {
        let o = l.pacote.join(format!("RECEITAS-DESPESAS-{cidade}")).join("documentos_tcm");
        std::fs::create_dir_all(&o).unwrap();
        for n in nomes {
            std::fs::write(o.join(n), b"%PDF").unwrap();
        }
    }

    fn nomes(p: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(p).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        v.sort();
        v
    }

    #[test]
    fn so_pdf_do_tribunal_e_copiado() {
        let (p, l) = locais("copiar");
        origem(&l, "ITATIM", &["2025-06_PCMGE015.pdf", "2025-06_A01.txt", "foto da camara.pdf", "contrato.pdf", "2025-6_X.pdf", "2025-06_ab.pdf"]);
        let dest = p.join("entrega");
        assert_eq!(copiar(&l, "143", "Itatim", Some(&dest), &copia), Some(1));
        assert_eq!(nomes(&dest.join(SUB)), ["2025-06_PCMGE015.pdf"]);
        assert_eq!(copiar(&l, "999", "Nada", Some(&dest), &copia), None); // sem origem
        assert_eq!(copiar(&l, "143", "Itatim", None, &copia), None); // sem pasta de entrega
        assert_eq!(copiar(&l, "143", "Itatim", Some(&dest), &|_, _| Err("nao era para copiar de novo".into())), Some(1)); // ja esta la, mesmo tamanho
        assert!(doc("2025-06_A01.pdf") && doc("2025-06_a.b-c.PDF") && !doc("2025-06_A01.pdf.exe") && !doc("../2025-06_A01.pdf") && !doc("2025-06_a b.pdf"));
        assert_eq!((asc("Barra do Rocha"), asc("São Félix do Coribe"), asc(" d'Ávila ")), ("BARRA-DO-ROCHA".into(), "SAO-FELIX-DO-CORIBE".into(), "D-AVILA".into()));
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn pasta_de_entrega_ignora_sobra_de_refacao() {
        let (p, l) = locais("entrega");
        std::fs::create_dir_all(l.site.join("150-OUTRO-NOME-v2")).unwrap();
        assert_eq!(pasta_entrega(&l, "150", "Cidade"), None);
        std::fs::create_dir_all(l.site.join("150-OUTRO-NOME")).unwrap();
        assert_eq!(pasta_entrega(&l, "150", "Cidade"), Some(l.site.join("150-OUTRO-NOME")));
        std::fs::create_dir_all(l.site.join("150-CIDADE")).unwrap();
        assert_eq!(pasta_entrega(&l, "150", "Cidade"), Some(l.site.join("150-CIDADE")));
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn sobe_so_o_que_falta_da_serie_certa_e_para_no_erro() {
        let (p, l) = locais("faltando");
        let dados = p.join("dados");
        std::fs::write(dados.join("parceiro/orquestrador.json"), json!({ "enviados": ["150 Alfa", "160 Beta", "901 Fora do Estado", "1001 Capital"] }).to_string()).unwrap();
        for (n, cidade) in [("150", "ALFA"), ("160", "BETA"), ("901", "FORA-DO-ESTADO"), ("1001", "CAPITAL")] {
            origem(&l, cidade, &["2025-06_A01.pdf", "outro.pdf"]);
            std::fs::create_dir_all(l.site.join(format!("{n}-{cidade}"))).unwrap();
        }
        // 170: ainda sem video (so preparo): vai por pasta temporaria, com meta
        std::fs::create_dir_all(l.pacote.join("videos/170")).unwrap();
        std::fs::write(l.pacote.join("videos/170/preparo.json"), json!({ "nome": "GAMA", "projeto": "170. gama" }).to_string()).unwrap();
        origem(&l, "GAMA", &["2026-01_B02.pdf"]);
        let enviados = RefCell::new(vec![]);
        let enviar = |d: &Path| {
            let meta = std::fs::read_to_string(d.join("meta.json")).unwrap_or_default();
            enviados.borrow_mut().push((d.file_name().unwrap().to_string_lossy().to_string(), nomes(&d.join(SUB)), meta));
            true
        };
        let r = enviar_faltando(&dados, &l, &[], &copia, &enviar);
        assert_eq!(r, [("150".to_string(), true), ("160".to_string(), true), ("170".to_string(), true)]); // 901 e 1001 nunca
        let e = enviados.borrow().clone();
        assert_eq!(e.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["150-ALFA", "160-BETA", "170-GAMA"]);
        assert!(e.iter().all(|x| x.1.len() == 1 && doc(&x.1[0])) && e[2].2.contains("\"cidade\":\"Gama\""));
        assert!(!l.tmp.join("170-GAMA").exists()); // a temporaria some
        assert_eq!(std::fs::read_to_string(dados.join("parceiro/pdfs_enviados.json")).unwrap(), "[\"150\",\"160\",\"170\"]");
        assert!(enviar_faltando(&dados, &l, &[], &copia, &|_| panic!("subiu de novo")).is_empty()); // o que ja subiu nao sobe de novo
        assert!(!enviar_numero(&l, "901", "Fora do Estado", &copia, &|_| panic!("serie 900 subiu")));

        std::fs::remove_file(dados.join("parceiro/pdfs_enviados.json")).unwrap();
        let tentou = RefCell::new(0);
        let r = enviar_faltando(&dados, &l, &[], &copia, &|_| { *tentou.borrow_mut() += 1; false });
        assert_eq!((r, *tentou.borrow()), (vec![("150".to_string(), false)], 1)); // para no primeiro erro
        let r = enviar_faltando(&dados, &l, &["160"], &copia, &|_| true);
        assert_eq!(r, [("160".to_string(), true)]);
        let _ = std::fs::remove_dir_all(&p);
    }
}
