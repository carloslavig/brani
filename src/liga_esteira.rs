//! Ligacao (liga_esteira): a esteira dos videos de prefeitura ate onde da sem render, sem voz e sem site: o LEVANTAMENTO dos
//! numeros no tribunal de contas (so sob pedido, em thread, um por vez) e a AUDITORIA do levantamento gravado.
//! Numero de video vem SO do tribunal de contas. Qualquer falha de rede, de arquivo ou de conta = PENDENCIA gravada; nada e
//! aproximado e a pasta `saida` do levantamento que falhou e apagada (o que foi baixado do tribunal fica, para nao baixar de novo).
//! Rede ligada (leitura de dado publico, uma consulta por vez no programa inteiro, com pausa e tempo limite):
//!   CE (API do SIM), RS (CSV, so a capital), TO (Portal do Cidadao + planilhas, so a capital), ES (CSV em zip do CidadES),
//!   PR (XML em zip do PIT, so Curitiba), SP (IRIS do TCM-SP, so a capital), MA (API do SINC + Painel de Envio, so Sao Luis),
//!   GO (Portal do Cidadao: o token do relatorio vem da propria pagina publica, so Goiania), RJ e RN (paineis Power BI publicos).
//! Setores: a base dos moldes chama o script de setores do PACOTE de producao (de terceiro: nao e copiado nem reescrito) como
//! processo externo, sem janela e com tempo limite. Sem a pasta do pacote, o pedido para antes de consultar o tribunal.
//! Depois do levantamento, tudo sob pedido (POST) e SEM voz, render ou site:
//!   ROTEIRO: os geradores do pacote (gera_cidade.py e gera_legenda.py, de terceiro) rodam como processo, mas eles tem a pasta do
//!     pacote escrita dentro do proprio codigo e GRAVAM nela. Aqui nada e gravado no pacote: o processo carrega o script com essa
//!     unica linha apontada para <levantamento>/roteiro (uma "sombra" do pacote so com o que o gerador le: os dois moldes, copiados,
//!     e um atalho de uma linha para o fontes.py), e roda com --teste (nao registra nada no Root.tsx). O arquivo do script nao e
//!     copiado nem alterado. O gerador busca sozinho o contorno do municipio na API publica do IBGE.
//!   AUDITORIA: os PDFs de documentos_tcm/ ou documentos/ sao lidos de novo pelo pdftotext a cada conferencia (o .txt ao lado e
//!     so rascunho: .txt sem PDF nao e prova); roteiro e legenda conferidos sao os da sombra, nunca os do pacote.
//!   RELEITURA: levantamento sem PDF (dados abertos) so tem a origem provada quando o tribunal, lido de novo numa pasta limpa,
//!     da o mesmo numero (link_conferir_capitais). A sigla do tribunal vem do guia do estado (BRANIAC_GUIAS ou dados/parceiro/guias).
//!   PORTA: so informa se poderia seguir para a voz. DOCUMENTOS: so o Ceara (unico molde cujo cache serve ao adaptador como esta).
//! NAO ligado aqui: releitura do tribunal na triagem, voz, render, site, envio de PDFs, fila do Link, tunel, o e-TCM da Bahia
//! e os moldes que dependem de navegador. O relogio nao faz nada.
use crate::link_auditar as auditar;
use crate::link_conferir_capitais as cc;
use crate::link_docs_cap as cap;
use crate::link_docs_capitais as docs;
use crate::link_tribunais_base as base;
use crate::link_tribunais_ce as ce;
use crate::roteador::Roteador;
use crate::tempo::Agora;
use crate::Resp;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const AVISO: &str = "Número de vídeo vem só do tribunal de contas. Qualquer dúvida vira pendência.";
const SEM_PACOTE: &str = "falta apontar a pasta do pacote de produção (variável BRANIAC_PACOTE ou a pasta dados/parceiro/pacote, com scripts-braniac/setores.py dentro)";
const NAVEGADOR: &str = "Portado, leitura pelo navegador ainda não ligada";
const LIMITE: u64 = 400 * 1024 * 1024;
/// Teto do que sai de dentro de um zip do tribunal (os CSV e XML de um ano inteiro sao grandes, mas nao tanto).
const ZIP_TETO: u64 = 1024 * 1024 * 1024;
const TEMPO_SETORES: Duration = Duration::from_secs(300);
const TEMPO_ROTEIRO: Duration = Duration::from_secs(300);
const SEM_PDFTOTEXT: &str = "não ligado: falta o pdftotext (instale o Poppler e ponha no PATH, ou aponte o programa na variável BRANIAC_PDFTOTEXT)";
const DAQUI: &str = "Daqui para frente (voz, vídeo e site) ainda é no Braniac atual.";
/// A "sombra" do pacote dentro da pasta do levantamento: tudo o que os geradores gravam cai aqui.
const SOMBRA: &str = "roteiro";
const MOLDE_TSX: [&str; 5] = ["estudio-3d", "src", "videos", "orcamentosalvadortcm", "OrcamentoSalvadorTcm.tsx"];
const MOLDE_KIT: [&str; 4] = ["kit-mapas", "src", "videos", "orcamento-salvadortcm"];
/// Carrega o script do pacote com a linha `PAC = Path("...")` apontada para a sombra e roda como se fosse ele. Sem essa linha
/// unica o processo para (o script mudou): nunca roda gravando no pacote.
const LANCADOR: &str = r"import re,sys;s,d=sys.argv[1],sys.argv[2];t,n=re.subn(r'(?m)^PAC = Path\(r?\x22[^\x22\n]*\x22\)',lambda m:'PAC = Path('+repr(d)+')',open(s,encoding='utf-8').read());n==1 or sys.exit('o script do pacote mudou: esperava uma linha PAC = Path(...), achei '+str(n));sys.argv=[s]+sys.argv[3:];exec(compile(t,s,'exec'),{'__name__':'__main__','__file__':s})";

/// (UF, tribunal, tipo, nota). Tipo: R = rede ligada; N = precisa de navegador/painel; P = pendente no tribunal (motivo em
/// link_tribunais::pendente, ou a nota); X = sem molde.
const MOLDES: [(&str, &str, char, &str); 27] = [
    ("AC", "TCE-AC", 'N', "painel Qlik"),
    ("AL", "TCE-AL", 'P', ""),
    ("AM", "TCE-AM", 'N', "o mês de corte sai de uma tela JSF"),
    ("AP", "TCE-AP", 'P', ""),
    ("BA", "TCM-BA", 'N', "e-TCM, um PDF por vez"),
    ("CE", "TCE-CE", 'R', "qualquer município do Ceará"),
    ("DF", "TCDF", 'X', "o Distrito Federal não tem municípios"),
    ("ES", "TCE-ES", 'R', "qualquer município do Espírito Santo, com o nome como o tribunal escreve; precisa da lista do IBGE baixada"),
    ("GO", "TCM-GO", 'R', "só Goiânia"),
    ("MA", "TCE-MA", 'R', "só São Luís; são milhares de consultas, leva horas"),
    ("MG", "TCE-MG", 'N', "download pela tela dos Dados Abertos, em zip"),
    ("MS", "TCE-MS", 'N', "motor do Qlik por WebSocket"),
    ("MT", "TCE-MT", 'N', "Radar do TCE-MT, painel Qlik"),
    ("PA", "TCM-PA", 'P', ""),
    ("PB", "TCE-PB", 'N', "SAGRES Online atrás de proteção de navegador, mais arquivos zip"),
    ("PE", "TCE-PE", 'N', "consulta pública do e-TCEPE e PDFs"),
    ("PI", "TCE-PI", 'N', "painel e PDF"),
    ("PR", "TCE-PR", 'R', "só Curitiba"),
    ("RJ", "TCM-RJ", 'R', "só a capital"),
    ("RN", "TCE-RN", 'R', "só Natal"),
    ("RO", "TCE-RO", 'P', "nenhuma base pública do TCE-RO entrega 2024 a 2026 por ação sem login ou captcha (nada estimado)"),
    ("RR", "TCE-RR", 'P', ""),
    ("RS", "TCE-RS", 'R', "só Porto Alegre"),
    ("SC", "TCE-SC", 'N', "painel Qlik"),
    ("SE", "TCE-SE", 'P', ""),
    ("SP", "TCM-SP", 'R', "só a capital"),
    ("TO", "TCE-TO", 'R', "só Palmas"),
];

/// O despacho por UF: tribunal, situacao em palavras e se da para levantar por aqui.
pub fn despacho(uf: &str) -> Value {
    let uf = uf.trim().to_uppercase();
    let Some((u, tribunal, tipo, nota)) = MOLDES.iter().find(|m| m.0 == uf) else {
        return json!({ "uf": uf, "situacao": "sem molde", "texto": "UF que não existe", "levantar": false });
    };
    let (situacao, texto) = match tipo {
        'R' => ("rede ligada", format!("Portado, com a leitura do tribunal ligada ({nota})")),
        'N' => ("sem rede", format!("{NAVEGADOR} ({nota})")),
        'P' => ("pendente", format!("Pendente: {}", crate::link_tribunais::pendente(u).unwrap_or(nota))),
        _ => ("sem molde", format!("Sem molde: {nota}")),
    };
    json!({ "uf": u, "tribunal": tribunal, "situacao": situacao, "texto": texto, "levantar": *tipo == 'R' })
}

/// Um pedido HTTP: (endereco, cabecalhos, corpo; com corpo e POST) -> (status, resposta).
pub type Http<'a> = &'a dyn Fn(&str, &[(&str, &str)], Option<&str>) -> Result<(u16, Vec<u8>), String>;

/// O que sai para fora, por parametro. Nos testes, tudo falso.
pub struct Fora<'a> {
    pub http: Http<'a>,
    /// Pausa entre pedidos, em segundos.
    pub dorme: &'a dyn Fn(f64),
    /// Roda o script de setores: (script, pasta `saida`) -> (codigo de saida, fim do que ele escreveu de erro). Err = nao rodou.
    pub script: &'a dyn Fn(&Path, &Path) -> Result<(i32, String), String>,
    /// Pasta do pacote de producao (de terceiro; so e usado o script de setores, como processo).
    pub pacote: &'a Path,
    /// Codigo do IBGE de (UF, cidade), pela lista oficial ja baixada.
    pub ibge: &'a dyn Fn(&str, &str) -> Option<i64>,
}

impl Fora<'_> {
    fn get(&self, url: &str) -> Result<(u16, Vec<u8>), String> {
        (self.http)(url, &[], None)
    }
}

/// Leitura em site publico: uma consulta por vez no programa inteiro, tempo limite, 3 tentativas. 404 volta como status.
/// O erro so leva o endereco (nunca cabecalho: o token do relatorio de GO vai neles).
fn rede_real(url: &str, cab: &[(&str, &str)], corpo: Option<&str>) -> Result<(u16, Vec<u8>), String> {
    static UMA: Mutex<()> = Mutex::new(());
    let _t = UMA.lock().map_err(|e| e.to_string())?;
    let mut erro = String::new();
    for t in 0..3 {
        if t > 0 {
            std::thread::sleep(Duration::from_secs(10));
        }
        let mut p = if corpo.is_some() { ureq::post(url) } else { ureq::get(url) }.set("User-Agent", "Mozilla/5.0").timeout(Duration::from_secs(300));
        for (k, v) in cab {
            p = p.set(k, v);
        }
        match corpo.map_or_else(|| p.clone().call(), |c| p.clone().send_string(c)) {
            Ok(r) => {
                let mut b = vec![];
                match r.into_reader().take(LIMITE + 1).read_to_end(&mut b) {
                    Ok(n) if n as u64 <= LIMITE => return Ok((200, b)),
                    Ok(_) => return Err(format!("{url}: resposta grande demais")),
                    Err(e) => erro = e.to_string(), // corpo cortado no meio: nunca usar pela metade
                }
            }
            Err(ureq::Error::Status(404, _)) => return Ok((404, vec![])),
            Err(e) => erro = e.to_string(),
        }
    }
    Err(format!("{url}: {erro}"))
}

fn dorme_real(s: f64) {
    std::thread::sleep(Duration::from_secs_f64(s));
}

/// Roda um programa sem janela e com tempo limite. Devolve (codigo de saida, o que ele escreveu de erro).
pub fn rodar(programa: &str, args: &[OsString], limite: Duration) -> Result<(i32, String), String> {
    use std::process::Stdio;
    let mut c = crate::oculto::cmd(programa);
    c.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut filho = c.spawn().map_err(|e| format!("não consegui rodar {programa}: {e}"))?;
    let mut err = filho.stderr.take();
    // le o erro em paralelo: se o cano enche, o programa trava esperando alguem ler
    let leitor = std::thread::spawn(move || {
        let mut t = vec![];
        if let Some(e) = err.as_mut() {
            let _ = e.take(1 << 20).read_to_end(&mut t);
        }
        String::from_utf8_lossy(&t).into_owned()
    });
    let comeco = std::time::Instant::now();
    let cod = loop {
        match filho.try_wait() {
            Ok(Some(s)) => break s.code().unwrap_or(-1),
            Ok(None) if comeco.elapsed() < limite => std::thread::sleep(Duration::from_millis(100)),
            fim => {
                let _ = filho.kill();
                let _ = filho.wait();
                let _ = leitor.join();
                return Err(fim.err().map_or_else(|| format!("tempo esgotado ({} s)", limite.as_secs()), |e| e.to_string()));
            }
        }
    };
    Ok((cod, leitor.join().unwrap_or_default()))
}

/// O script de setores do pacote, como o Python chama: python isolado (-I), UTF-8, o script e a pasta `saida`.
fn script_real(script: &Path, saida: &Path) -> Result<(i32, String), String> {
    let python = std::env::var("BRANIAC_PYTHON").unwrap_or_else(|_| "python".into());
    rodar(&python, &["-I".into(), "-X".into(), "utf8".into(), script.into(), saida.into()], TEMPO_SETORES)
}

/// O fim do que o script escreveu de erro (e onde esta o motivo).
fn fim(err: &str) -> String {
    err.trim().chars().rev().take(300).collect::<Vec<_>>().into_iter().rev().collect()
}

fn script_de_setores(pacote: &Path) -> PathBuf {
    pacote.join("scripts-braniac").join("setores.py")
}

/// A closure `setores` da base dos moldes: roda o script do pacote sobre a pasta `saida`. Pacote ausente, script que nao
/// rodou, que saiu com erro ou que nao gravou o setores.json = Err (pendencia).
pub fn setores(f: &Fora, saida: &Path) -> Result<(), String> {
    let script = script_de_setores(f.pacote);
    if !script.is_file() {
        return Err(SEM_PACOTE.into());
    }
    let feito = saida.join("setores.json");
    let _ = std::fs::remove_file(&feito); // o de uma rodada antiga nao pode passar por resultado desta
    let (cod, err) = (f.script)(&script, saida).map_err(|e| format!("script de setores do pacote: {e}"))?;
    if cod != 0 {
        return Err(format!("o script de setores do pacote falhou (código {cod}): {}", fim(&err)));
    }
    if !feito.is_file() {
        return Err("o script de setores do pacote terminou sem gravar o setores.json".into());
    }
    Ok(())
}

fn pct(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

fn ok200(url: &str, r: (u16, Vec<u8>)) -> Result<Vec<u8>, String> {
    if r.0 == 200 { Ok(r.1) } else { Err(format!("{url}: o tribunal respondeu {}", r.0)) }
}

fn utf8(b: Vec<u8>, o_que: &str) -> Result<String, String> {
    String::from_utf8(b).map(|t| t.trim_start_matches('\u{feff}').to_string()).map_err(|_| format!("{o_que}: texto que não é UTF-8"))
}

const CP1252_ALTO: &str = "€\0‚ƒ„…†‡ˆ‰Š‹Œ\0Ž\0\0‘’“”•–—˜™š›œ\0žŸ"; // 0x80 a 0x9F; \0 = byte sem letra
const CP850_ALTO: &str = "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜø£Ø×ƒáíóúñÑªº¿®¬½¼¡«»░▒▓│┤ÁÂÀ©╣║╗╝¢¥┐└┴┬├─┼ãÃ╚╔╩╦╠═╬¤ðÐÊËÈıÍÎÏ┘┌█▄¦Ì▀ÓßÔÒõÕµþÞÚÛÙýÝ¯´\u{ad}±‗¾¶§÷¸°¨·¹³²■\u{a0}";

/// Texto em Windows-1252 (CSV do CidadES). Byte que a tabela nao tem = Err, como no Python.
fn cp1252(b: &[u8], o_que: &str) -> Result<String, String> {
    let alto: Vec<char> = CP1252_ALTO.chars().collect();
    b.iter()
        .map(|&c| match c {
            0x80..=0x9F => Some(alto[(c - 0x80) as usize]).filter(|x| *x != '\0').ok_or_else(|| format!("{o_que}: texto que não é Windows-1252")),
            _ => Ok(c as char),
        })
        .collect()
}

/// Texto em cp850 (CSV do IRIS do TCM-SP).
fn cp850(b: &[u8]) -> String {
    let alto: Vec<char> = CP850_ALTO.chars().collect();
    b.iter().map(|&c| if c < 0x80 { c as char } else { alto[(c - 0x80) as usize] }).collect()
}

/// XML do tribunal: UTF-8, ou Latin-1 quando o proprio cabecalho do arquivo diz.
fn xml_texto(b: Vec<u8>, o_que: &str) -> Result<String, String> {
    let latin = String::from_utf8_lossy(&b[..b.len().min(200)]).to_lowercase().contains("8859-1");
    if latin { Ok(b.iter().map(|c| *c as char).collect()) } else { utf8(b, o_que) }
}

/// Abre um zip na memoria e devolve (nome, conteudo) dos arquivos que `quer` (pelo nome sem a pasta), em ordem de nome.
/// Zip que nao abre (download cortado), entrada com `..` ou caminho absoluto, ou conteudo acima do teto = Err.
pub fn do_zip(b: &[u8], quer: &dyn Fn(&str) -> bool, teto: u64, o_que: &str) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(b)).map_err(|e| format!("{o_que}: zip que não abre ({e})"))?;
    let (mut out, mut total) = (vec![], 0u64);
    for i in 0..z.len() {
        let mut e = z.by_index(i).map_err(|e| format!("{o_que}: zip com entrada ilegível ({e})"))?;
        let nome = e.name().replace('\\', "/");
        if nome.starts_with('/') || nome.contains(':') || nome.split('/').any(|p| p == "..") {
            return Err(format!("{o_que}: zip recusado, caminho suspeito dentro dele"));
        }
        if e.is_dir() || !quer(nome.rsplit('/').next().unwrap_or("")) {
            continue;
        }
        let mut c = vec![];
        // o tamanho declarado no zip nao vale como limite: conta o que sai de verdade
        total += (&mut e).take(teto - total + 1).read_to_end(&mut c).map_err(|e| format!("{o_que}: zip cortado ({e})"))? as u64;
        if total > teto {
            return Err(format!("{o_que}: o conteúdo do zip passa do teto"));
        }
        out.push((nome, c));
    }
    out.sort();
    Ok(out)
}

/// O unico (ou o primeiro, por nome) arquivo que `quer` dentro do zip; nenhum = Err.
fn um_do_zip(b: &[u8], quer: &dyn Fn(&str) -> bool, o_que: &str) -> Result<Vec<u8>, String> {
    do_zip(b, quer, ZIP_TETO, o_que)?.into_iter().next().map(|x| x.1).ok_or_else(|| format!("{o_que}: zip sem o arquivo esperado"))
}

/// O arquivo ja baixado ainda vale? Ano fechado vale sempre; 2026 vale `dias`.
fn fresco(arq: &Path, ano: &str, dias: f64, agora_s: f64) -> bool {
    let mexido = std::fs::metadata(arq).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs_f64());
    mexido.is_some_and(|t| ano != "2026" || agora_s - t < dias * 86400.0)
}

fn pasta_de_cache(pasta: &Path, nome: &str) -> Result<PathBuf, String> {
    let d = pasta.join(nome);
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    Ok(d)
}

/// Arquivo do tribunal que e refeito de tempos em tempos (vale `dias`, ate o de ano fechado): baixa se o guardado venceu,
/// confere com `abre` e so entao guarda. Arquivo que nao abre nunca fica no cache.
fn baixado<T>(arq: &Path, dias: f64, agora_s: f64, baixa: &dyn Fn() -> Result<Vec<u8>, String>, abre: &dyn Fn(&[u8]) -> Result<T, String>) -> Result<T, String> {
    if fresco(arq, "2026", dias, agora_s) {
        return abre(&std::fs::read(arq).map_err(|e| e.to_string())?);
    }
    let b = baixa()?;
    let t = abre(&b)?;
    std::fs::write(arq, &b).map_err(|e| e.to_string())?;
    Ok(t)
}

/// Endereco que veio de dentro de uma resposta (catalogo): so https de orgao publico brasileiro.
fn de_orgao_publico(url: &str) -> bool {
    url.strip_prefix("https://").and_then(|r| r.split(['/', '?', '#']).next()).is_some_and(|h| !h.contains(['@', ':']) && (h.ends_with(".gov.br") || h.ends_with(".tc.br")))
}

/// POST de consulta a um painel Power BI publico; devolve a resposta crua (quem decodifica e confere e o molde).
fn painel(f: &Fora, url: &str, cab: &[(&str, &str)], corpo: &Value, pausa: f64, o_que: &str) -> Result<String, String> {
    let mut c = cab.to_vec();
    c.push(("Content-Type", "application/json"));
    let r = utf8(ok200(url, (f.http)(url, &c, Some(&corpo.to_string()))?)?, o_que)?;
    (f.dorme)(pausa);
    Ok(r)
}

/// O primeiro modelo do relatorio (id e, em GO, o nome do banco).
fn modelo(f: &Fora, url: &str, cab: &[(&str, &str)], o_que: &str) -> Result<Value, String> {
    let v: Value = serde_json::from_slice(&ok200(url, (f.http)(url, cab, None)?)?).map_err(|e| format!("{o_que}: resposta que não é JSON ({e})"))?;
    Some(v["models"][0].clone()).filter(|m| !m["id"].is_null()).ok_or_else(|| format!("{o_que}: o painel não informou o modelo do relatório"))
}

const CE_API: &str = "https://api-dados-abertos.tce.ce.gov.br/sim/";
const RS_URL: &str = "https://dados.tce.rs.gov.br/dados/municipal";
const TO_PORTAL: &str = "https://portaldocidadao.tceto.tc.br/estadomunicipios";
const TO_PLAN: &str = "https://www.tceto.tc.br/sicap/contabilx/relatorios/planilhas";
const ES_CKAN: &str = "https://dados.es.gov.br/api/3/action/package_show?id=";
const PR_URL: &str = "https://pit.tce.pr.gov.br/Arquivos";
const SP_URL: &str = "https://iris.tcm.sp.gov.br/assets/data";
const MA_API: &str = "https://tce-api.apps.tcema.tc.br/sincfiscal/";
/// Painel de Envio do SINC 2026, aba "Ranking Sinc-Fiscal" (status de cada remessa bimestral), e o id do filtro de periodo.
const MA_ENVIO: (&str, &str) = ("https://metabase.apps.tcema.tc.br/api/public/dashboard/45e43e2a-5066-4702-a493-2e39d1b9e256/dashcard/1733/card/1792", "d2d291e6");
const GO_PORTAL: &str = "https://portalcidadao.tcmgo.tc.br/new-portal-cidadao/pages/visualizador.php";
const GO_CLUSTER: &str = "https://WABI-BRAZIL-SOUTH-redirect.analysis.windows.net";
/// Chaves PUBLICAS dos relatorios (estao no link que o proprio tribunal publica).
const RJ_CHAVE: &str = "6cb89d2f-8748-4d3f-8a1c-33c9f269b1ba";
const RJ_API: &str = "https://wabi-brazil-south-b-primary-api.analysis.windows.net/public/reports";
const RN_CHAVE: &str = "dc705690-d721-4b4c-ac5b-f99206773f0c";
const RN_API: &str = "https://wabi-brazil-south-api.analysis.windows.net/public/reports/querydata?synchronous=true";

/// Roda o molde da UF (sincrono). Err = pendencia: a pasta `saida` e apagada, nenhum numero fica.
pub fn levantar(pasta: &Path, uf: &str, cidade: &str, numero: &str, f: &Fora, agora_s: f64) -> Result<Value, String> {
    std::fs::create_dir_all(pasta).map_err(|e| e.to_string())?;
    if despacho(uf)["levantar"] == true && !script_de_setores(f.pacote).is_file() {
        return Err(SEM_PACOTE.into()); // sem o script o levantamento nunca fecha: nem gasta o tribunal
    }
    let setores = |saida: &Path| setores(f, saida);
    let r = match uf {
        "CE" => {
            let pagina = |met: &str, p: &ce::Params| -> Result<Vec<Value>, String> {
                (f.dorme)(0.5);
                let mut q = p.clone();
                q.entry("$count".into()).or_insert_with(|| ce::PAGINA.to_string());
                let url = format!("{CE_API}{met}?{}", q.iter().map(|(k, v)| format!("{}={}", pct(k), pct(v))).collect::<Vec<_>>().join("&"));
                let b = ok200(&url, f.get(&url)?)?;
                let mut r: Value = serde_json::from_slice(&b).map_err(|e| format!("TCE-CE: {met}: resposta que não é JSON ({e})"))?;
                match r["elements"].take() {
                    Value::Array(l) => Ok(l),
                    _ => Err(format!("API do TCE-CE respondeu sem dados em {met}")),
                }
            };
            ce::preparar(&pagina, &setores, numero, cidade, pasta, agora_s)
        }
        "RS" => {
            let texto = |t: &str, a: &str, o: &str| -> Result<Option<String>, String> {
                let arq = pasta_de_cache(pasta, "tce-rs")?.join(format!("{t}_{a}_{o}.csv"));
                if !fresco(&arq, a, 3.0, agora_s) {
                    (f.dorme)(3.0);
                    let url = format!("{RS_URL}/balancete-{t}/{a}/{o}.csv");
                    let (c, b) = f.get(&url)?;
                    if c == 404 && t == "receita" && o == crate::link_tribunais_rs::CAMARA {
                        return Ok(None); // a Camara nao tem receita orcamentaria
                    }
                    std::fs::write(&arq, ok200(&url, (c, b))?).map_err(|e| e.to_string())?;
                }
                utf8(std::fs::read(&arq).map_err(|e| e.to_string())?, "TCE-RS").map(Some)
            };
            crate::link_tribunais_rs::preparar(&texto, &setores, numero, cidade, pasta)
        }
        "TO" => {
            let busca = |rota: &str, ug: &str, a: &str, rem: i64| -> Result<String, String> {
                // `ug` vem do JSON do tribunal e vira nome de arquivo e pedaco de endereco: so digitos
                if !ug.bytes().all(|b| b.is_ascii_digit()) || !a.bytes().all(|b| b.is_ascii_digit()) {
                    return Err("TCE-TO: unidade gestora ou ano fora do formato".into());
                }
                let json = |url: String| -> Result<String, String> {
                    (f.dorme)(0.5);
                    utf8(ok200(&url, f.get(&url)?)?, "TCE-TO")
                };
                match rota {
                    "getUnidades" => json(format!("{TO_PORTAL}/getUnidades?municipio={}&ajax=true", crate::link_tribunais_to::IBGE)),
                    "getRemessas" => json(format!("{TO_PORTAL}/getRemessas?exercicio={a}&unidade={ug}&ajax=true")),
                    "balanceteDespesa" | "balanceteReceita" => {
                        let arq = pasta_de_cache(pasta, "tce-to")?.join(format!("{rota}-{a}-{rem:02}-{ug}.xls"));
                        if !fresco(&arq, a, 7.0, agora_s) {
                            (f.dorme)(1.5);
                            let url = format!("{TO_PLAN}/{rota}.php?unidade={ug}&exercicio={a}&bimestre={rem}");
                            std::fs::write(&arq, ok200(&url, f.get(&url)?)?).map_err(|e| e.to_string())?;
                        }
                        Ok(std::fs::read(&arq).map_err(|e| e.to_string())?.iter().map(|b| *b as char).collect()) // latin-1
                    }
                    _ => Err(format!("TCE-TO: consulta desconhecida ({rota})")),
                }
            };
            crate::link_tribunais_to::preparar(&busca, &setores, numero, cidade, pasta)
        }
        "ES" => {
            let d = pasta_de_cache(pasta, "tce-es")?;
            let texto = |tipo: &str, a: &str| -> Result<String, String> {
                // o TCE-ES republica os arquivos todo mes e troca o endereco: o endereco sai do catalogo, pelo nome do recurso
                let (pac, nome, arq) = match tipo {
                    "pcm" => ("obrigacoes".to_string(), "Prestação de Contas Municipal (PCM)".to_string(), d.join("pcm.csv")),
                    "despesas" | "receitas" if base::ANOS.contains(&a) => (format!("{tipo}-municipios"), format!("municipios-{tipo}-{a}.zip"), d.join(format!("{tipo}{a}.zip"))),
                    _ => return Err(format!("TCE-ES: arquivo desconhecido ({tipo} {a})")),
                };
                let baixa = || {
                    (f.dorme)(1.0);
                    let cat = format!("{ES_CKAN}{pac}");
                    let url = crate::link_tribunais_es::recurso(&utf8(ok200(&cat, f.get(&cat)?)?, "TCE-ES")?, &pac, &nome)?;
                    if !de_orgao_publico(&url) {
                        return Err(format!("TCE-ES: o catálogo apontou '{nome}' para um endereço que não é de órgão público"));
                    }
                    (f.dorme)(3.0);
                    ok200(&url, f.get(&url)?)
                };
                let abre = |b: &[u8]| if tipo == "pcm" { cp1252(b, "TCE-ES") } else { cp1252(&um_do_zip(b, &|_| true, &nome)?, "TCE-ES") };
                baixado(&arq, 20.0, agora_s, &baixa, &abre)
            };
            let ibge = (f.ibge)("ES", cidade).ok_or("ES: não achei o código do IBGE dessa cidade (baixe a lista do IBGE e escreva o nome como está nela)")?;
            crate::link_tribunais_es::preparar(&texto, &setores, numero, cidade, pasta, ibge)
        }
        "PR" => {
            use crate::link_tribunais_pr as pr;
            let d = pasta_de_cache(pasta, "tce-pr")?;
            let textos = |tipo: &str, a: &str| -> Result<Vec<String>, String> {
                if !base::ANOS.contains(&a) {
                    return Err(format!("TCE-PR: ano fora do molde ({a})"));
                }
                let t = if tipo == "receita" { "Receita" } else { "Despesa" };
                let nome = format!("{a}_{}_{t}.zip", pr::IBGE6);
                let url = format!("{PR_URL}/{a}/{nome}");
                let liq = regex::Regex::new(r"_EmpenhoLiquidacao(_\d+)?\.xml$").expect("regex fixa");
                let quer = |n: &str| match tipo {
                    "receita" => n.ends_with("ReceitasConsolidado.xml"),
                    "empenho" => n == format!("{a}_{}_Empenho.xml", pr::IBGE6),
                    _ => liq.is_match(n),
                };
                let baixa = || {
                    (f.dorme)(3.0);
                    ok200(&url, f.get(&url)?)
                };
                let abre = |b: &[u8]| do_zip(b, &quer, ZIP_TETO, &nome)?.into_iter().map(|(n, c)| xml_texto(c, &n)).collect::<Result<Vec<_>, _>>();
                // o PIT refaz os arquivos toda semana
                baixado(&d.join(format!("{a}_{t}.zip")), 6.0, agora_s, &baixa, &abre)
            };
            pr::preparar(&textos, &setores, numero, cidade, pasta)
        }
        "SP" => {
            let d = pasta_de_cache(pasta, "tcm-sp")?;
            let texto = |tipo: &str, a: &str| -> Result<String, String> {
                if !base::ANOS.contains(&a) {
                    return Err(format!("TCM-SP: ano fora do molde ({a})"));
                }
                let (rel, nome) = if tipo == "receita" { (format!("receita/csvReceita{a}.zip"), format!("csvReceita{a}.zip")) } else { (format!("dotacao/Dotacao{a}.csv"), format!("Dotacao{a}.csv")) };
                let url = format!("{SP_URL}/{rel}");
                let baixa = || {
                    (f.dorme)(3.0);
                    ok200(&url, f.get(&url)?)
                };
                let abre = |b: &[u8]| if tipo == "receita" { um_do_zip(b, &|n| n.to_lowercase().ends_with(".csv"), &nome).map(|c| cp850(&c)) } else { Ok(cp850(b)) };
                // o IRIS atualiza todo mes
                baixado(&d.join(&nome), 20.0, agora_s, &baixa, &abre)
            };
            crate::link_tribunais_sp::preparar(&texto, &setores, numero, cidade, pasta)
        }
        "MA" => {
            use crate::link_tribunais_ma as ma;
            let d = pasta_de_cache(pasta, "tce-ma")?;
            let q = |a: &str, met: &str, size: usize, page: usize, fl: &ma::Filtros| -> Result<String, String> {
                let mut url = format!("{MA_API}{a}/{met}?enteId={}&size={size}&page={page}", ma::IBGE_SAO_LUIS);
                for (k, v) in fl {
                    url += &format!("&{}={}", pct(k), pct(v));
                }
                // shortcut: o nome do arquivo e o hash padrao do Rust, que pode mudar numa versao nova do compilador (so perde o cache)
                let mut h = std::collections::hash_map::DefaultHasher::new();
                std::hash::Hash::hash(&url, &mut h);
                let arq = d.join(format!("{a}-{met}-{:016x}.json", std::hash::Hasher::finish(&h)));
                if fresco(&arq, a, 3.0, agora_s) {
                    return utf8(std::fs::read(&arq).map_err(|e| e.to_string())?, "TCE-MA");
                }
                (f.dorme)(1.0);
                let t = utf8(ok200(&url, f.get(&url)?)?, "TCE-MA")?;
                let v: Value = serde_json::from_str(&t).map_err(|_| format!("TCE-MA não respondeu: {url}"))?;
                let Some(linhas) = v["content"].as_array() else { return Err(format!("TCE-MA não respondeu: {url}")) };
                // a API nao ordena as paginas: so a fatia que veio INTEIRA numa consulta fica guardada (da para retomar de onde parou);
                // pagina de varredura e pergunta de total sao sempre refeitas
                if v["totalElements"].as_u64() == Some(linhas.len() as u64) && v["numberOfElements"] == v["totalElements"] {
                    std::fs::write(&arq, &t).map_err(|e| e.to_string())?;
                }
                Ok(t)
            };
            let envio = |b: u8| -> Result<String, String> {
                (f.dorme)(2.0);
                let p = json!([{ "id": MA_ENVIO.1, "type": "number/=", "value": [b], "target": ["variable", ["template-tag", "periodo"]] }]).to_string();
                let url = format!("{}?parameters={}", MA_ENVIO.0, pct(&p));
                utf8(ok200(&url, f.get(&url)?)?, "Painel de Envio do SINC")
            };
            ma::preparar(&q, &envio, &setores, numero, cidade, pasta)
        }
        "GO" => {
            // cada pagina do portal e um relatorio: o token e o numero do relatorio vem na propria pagina publica (pag -> token, relatorio, modelo, banco)
            let paineis: RefCell<BTreeMap<String, (String, String, Value, Value)>> = RefCell::default();
            let consulta = |pag: &str, comando: &Value| -> Result<String, String> {
                if !paineis.borrow().contains_key(pag) {
                    (f.dorme)(1.0);
                    let url = format!("{GO_PORTAL}?pag={}&municipio={}", pct(pag), pct(cidade));
                    let h = String::from_utf8_lossy(&ok200(&url, f.get(&url)?)?).into_owned();
                    let acha = |rx: &str| regex::Regex::new(rx).expect("regex fixa").captures(&h).map(|c| c[1].to_string());
                    let (Some(tok), Some(rid)) = (acha(r#"accessToken:\s*"([A-Za-z0-9._~+/=-]+)""#), acha(r"reportId=([0-9a-f-]+)")) else {
                        return Err(format!("Portal do Cidadão do TCM-GO: a página '{pag}' não trouxe o token do relatório (a página mudou?)"));
                    };
                    let m = modelo(f, &format!("{GO_CLUSTER}/explore/reports/{rid}/modelsAndExploration?preferReadOnlySession=true"), &[("Authorization", &format!("Bearer {tok}"))], "Portal do Cidadão do TCM-GO")?;
                    (f.dorme)(3.0);
                    paineis.borrow_mut().insert(pag.into(), (tok, rid, m["id"].clone(), m["dbName"].clone()));
                }
                let (tok, rid, mid, db) = paineis.borrow()[pag].clone();
                let corpo = json!({ "version": "1.0.0", "queries": [{ "Query": { "Commands": [comando] }, "ApplicationContext": { "DatasetId": db, "Sources": [{ "ReportId": rid }] } }], "cancelQueries": [], "modelId": mid });
                painel(f, &format!("{GO_CLUSTER}/explore/querydata?synchronous=true"), &[("Authorization", &format!("Bearer {tok}"))], &corpo, 3.0, "Portal do Cidadão do TCM-GO")
            };
            crate::link_tribunais_go::preparar(&consulta, &setores, numero, cidade, pasta)
        }
        "RJ" => {
            let chave = [("X-PowerBI-ResourceKey", RJ_CHAVE)];
            let mid: RefCell<Option<Value>> = RefCell::default();
            let consulta = |comando: &Value| -> Result<String, String> {
                if mid.borrow().is_none() {
                    let m = modelo(f, &format!("{RJ_API}/{RJ_CHAVE}/modelsAndExploration?preferReadOnlySession=true"), &chave, "Radar Carioca do TCM-RJ")?;
                    (f.dorme)(3.0);
                    *mid.borrow_mut() = Some(m["id"].clone());
                }
                let corpo = json!({ "version": "1.0.0", "queries": [{ "Query": { "Commands": [comando] } }], "cancelQueries": [], "modelId": *mid.borrow() });
                painel(f, &format!("{RJ_API}/querydata?synchronous=true"), &chave, &corpo, 3.0, "Radar Carioca do TCM-RJ")
            };
            crate::link_tribunais_rj::preparar(&consulta, &setores, numero, cidade, pasta)
        }
        "RN" => {
            let consulta = |corpo: &Value| painel(f, RN_API, &[("X-PowerBI-ResourceKey", RN_CHAVE)], corpo, 1.0, "Painel Fiscal do TCE-RN");
            crate::link_tribunais_rn::preparar(&consulta, &setores, numero, cidade, pasta)
        }
        _ => Err(despacho(uf)["texto"].as_str().unwrap_or("UF sem leitura ligada").to_string()),
    };
    r.inspect_err(|_| {
        let _ = std::fs::remove_dir_all(pasta.join("saida")); // pendencia nao deixa numero para tras
    })
}

fn raiz(dados: &Path) -> PathBuf {
    dados.join("parceiro").join("levantamentos")
}

fn numero_ok(n: &str) -> bool {
    (1..=4).contains(&n.len()) && n.bytes().all(|b| b.is_ascii_digit())
}

/// Pasta do levantamento daquele numero (a mais nova, se houver mais de uma cidade com o mesmo numero).
fn pasta_do_numero(dados: &Path, numero: &str) -> Option<PathBuf> {
    if !numero_ok(numero) {
        return None;
    }
    let pre = format!("{numero}-");
    std::fs::read_dir(raiz(dados)).ok()?.flatten().filter(|d| d.file_name().to_string_lossy().starts_with(&pre) && d.path().is_dir())
        .max_by_key(|d| d.metadata().and_then(|m| m.modified()).ok()).map(|d| d.path())
}

fn ler_json(arq: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(arq).ok()?).ok()
}

fn gravar_json(arq: &Path, v: &Value) -> Result<(), String> {
    std::fs::write(arq, v.to_string()).map_err(|e| e.to_string())
}

/// Numero do levantamento que esta rodando agora (um por vez).
static RODANDO: Mutex<Option<String>> = Mutex::new(None);

fn rodando() -> Option<String> {
    RODANDO.lock().ok().and_then(|g| g.clone())
}

/// Confere o pedido e devolve (uf, cidade, numero, pasta). Recusa: outra fonte que nao o tribunal, UF sem rede ligada, dado torto.
pub fn pedido(dados: &Path, v: &Value) -> Result<(String, String, String, PathBuf), String> {
    let txt = |k: &str| v[k].as_str().map(|s| s.trim().to_string()).or_else(|| v[k].as_i64().map(|n| n.to_string())).unwrap_or_default();
    let (uf, cidade, numero, fonte) = (txt("uf").to_uppercase(), txt("cidade"), txt("numero"), txt("fonte").to_lowercase());
    if !fonte.is_empty() && fonte != "tribunal" {
        // qualquer outra fonte cai na trava do SICONFI: recusa e explica
        return Err(crate::link_fonte_siconfi::preparar(&numero, &uf, &cidade, &raiz(dados)).err().unwrap_or_else(|| crate::link_fonte_siconfi::MOTIVO.into()));
    }
    let d = despacho(&uf);
    if d["levantar"] != true {
        return Err(format!("{uf}: {}", d["texto"].as_str().unwrap_or("")));
    }
    if !numero_ok(&numero) {
        return Err("numero: de 1 a 4 algarismos".into());
    }
    let slug = base::slug(&cidade).to_lowercase();
    if slug.is_empty() || cidade.chars().count() > 60 || cidade.chars().any(|c| c.is_control() || "/\\<>:\"|?*".contains(c)) {
        return Err("cidade: escreva o nome do município".into());
    }
    let pasta = raiz(dados).join(format!("{numero}-{slug}"));
    Ok((uf, cidade, numero, pasta))
}

/// Uma consulta longa ao tribunal por vez (levantamento ou releitura).
fn ocupa(numero: &str) -> Result<(), String> {
    let mut g = RODANDO.lock().map_err(|e| e.to_string())?;
    if let Some(n) = g.as_ref() {
        return Err(format!("já tem um levantamento ou uma releitura rodando (número {n}); é um por vez"));
    }
    *g = Some(numero.to_string());
    Ok(())
}

fn solta() {
    if let Ok(mut g) = RODANDO.lock() {
        *g = None;
    }
}

/// O de fora de verdade: rede publica com pausa, script de setores do pacote, lista do IBGE gravada.
fn com_fora<R>(dados: &Path, f: impl FnOnce(&Fora) -> R) -> R {
    let ibge = |uf: &str, cidade: &str| ibge_de(dados, uf, cidade);
    f(&Fora { http: &rede_real, dorme: &dorme_real, script: &script_real, pacote: &pacote(dados), ibge: &ibge })
}

/// Comeca a releitura do tribunal em thread e devolve na hora. O estado fica em <pasta>/releitura.json.
fn comecar_releitura(dados: &Path, numero: &str) -> Result<Value, String> {
    let e = estado_de(dados, numero)?;
    let pasta = pasta_do_numero(dados, numero).ok_or("não há levantamento com esse número")?;
    if e["estado"] != "pronto" || e["roteiro"]["estado"] != "pronto" {
        return Err("a releitura do tribunal vem depois do levantamento e do roteiro prontos".into());
    }
    ocupa(numero)?;
    if let Err(e) = gravar_json(&pasta.join("releitura.json"), &json!({ "estado": "fazendo", "comecou": hora() })) {
        solta();
        return Err(e);
    }
    let (dados, numero) = (dados.to_path_buf(), numero.to_string());
    let resposta = json!({ "resultado": "comecei", "numero": numero, "aviso": AVISO });
    std::thread::spawn(move || {
        let sigla = |uf: &str| sigla_do_guia(&dados, uf);
        let v = match com_fora(&dados, |f| reler(&pasta, &numero, f, &sigla, crate::fatos::agora_ts())) {
            Ok(p) => json!({ "estado": "pronto", "problemas": p, "terminou": hora() }),
            Err(m) => json!({ "estado": "pendencia", "motivo": m, "terminou": hora() }),
        };
        let _ = gravar_json(&pasta.join("releitura.json"), &v);
        solta();
    });
    Ok(resposta)
}

/// Comeca o levantamento em thread e devolve na hora. O estado fica em <pasta>/estado.json.
fn comecar(dados: &Path, v: &Value) -> Result<Value, String> {
    let (uf, cidade, numero, pasta) = pedido(dados, v)?;
    ocupa(&numero)?;
    let mut estado = json!({ "numero": numero, "uf": uf, "cidade": cidade, "estado": "fazendo", "comecou": hora() });
    if let Err(e) = std::fs::create_dir_all(&pasta).map_err(|e| e.to_string()).and_then(|_| gravar_json(&pasta.join("estado.json"), &estado)) {
        solta();
        return Err(e);
    }
    // roteiro, auditoria, releitura e documentos de um levantamento antigo nao valem para o novo
    for f in ["auditoria.json", "roteiro.json", "releitura.json"] {
        let _ = std::fs::remove_file(pasta.join(f));
    }
    apaga_pasta(&pasta.join(SOMBRA));
    apaga_pasta(&pasta.join("documentos-capital"));
    let resposta = json!({ "resultado": "comecei", "numero": numero, "aviso": AVISO });
    let dados = dados.to_path_buf();
    std::thread::spawn(move || {
        let r = com_fora(&dados, |f| levantar(&pasta, &uf, &cidade, &numero, f, crate::fatos::agora_ts()));
        estado["terminou"] = json!(hora());
        match r {
            Ok(_) => estado["estado"] = json!("pronto"),
            Err(e) => {
                estado["estado"] = json!("pendencia");
                estado["motivo"] = json!(e);
            }
        }
        let _ = gravar_json(&pasta.join("estado.json"), &estado);
        solta();
    });
    Ok(resposta)
}

/// Estado do levantamento daquele numero. "fazendo" sem thread viva = o programa reiniciou no meio.
fn estado_de(dados: &Path, numero: &str) -> Result<Value, String> {
    let pasta = pasta_do_numero(dados, numero).ok_or("não há levantamento com esse número")?;
    let mut e = ler_json(&pasta.join("estado.json")).unwrap_or_else(|| json!({ "numero": numero, "estado": "sem registro" }));
    if e["estado"] == "fazendo" && rodando().as_deref() != Some(numero) {
        e["estado"] = json!("pendencia");
        e["motivo"] = json!("o programa reiniciou no meio do levantamento; peça de novo");
    }
    e["tem_levantamento"] = json!(pasta.join("saida").join("cidade.json").exists());
    for k in ["auditoria", "roteiro"] {
        e[k] = ler_json(&pasta.join(format!("{k}.json"))).unwrap_or(Value::Null);
    }
    e["releitura"] = releitura_de(&pasta, numero);
    e["documentos"] = ler_json(&pasta.join("documentos-capital").join("relatorio.json")).unwrap_or(Value::Null);
    Ok(e)
}

fn pacote(dados: &Path) -> PathBuf {
    std::env::var_os("BRANIAC_PACOTE").map(PathBuf::from).unwrap_or_else(|| dados.join("parceiro").join("pacote"))
}

/// Codigo do IBGE da cidade, pela lista oficial ja baixada (nome igual, sem contar acento e caixa).
fn ibge_de(dados: &Path, uf: &str, cidade: &str) -> Option<i64> {
    let igual = |n: &str| crate::texto_pt::sem_acento(n).to_lowercase() == crate::texto_pt::sem_acento(cidade).to_lowercase();
    crate::ibge_cidades::procurar(&dados.join("ibge"), uf, cidade).iter().find(|c| c["nome"].as_str().is_some_and(igual)).and_then(|c| c["ibge"].as_i64())
}

/// A auditoria so olha o que foi gerado AQUI: roteiro, legenda e preparo estao na sombra, nunca no pacote nem em copia antiga.
fn locais(pasta: &Path) -> auditar::Locais {
    let sombra = pasta.join(SOMBRA);
    auditar::Locais { backup: sombra.join("sem-copia"), pacote: sombra }
}

fn junta(base: &Path, pedacos: &[&str]) -> PathBuf {
    pedacos.iter().fold(base.to_path_buf(), |p, x| p.join(x))
}

fn apaga_pasta(p: &Path) {
    let _ = std::fs::remove_dir_all(p);
}

fn lista_de(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(|p| p.as_str().map(String::from)).collect()
}

fn hora() -> String {
    let a = crate::tempo::agora();
    format!("{} {}", a.data.iso(), a.hm())
}

/// pdftotext: (PDF, .txt que ele grava) -> Ok, ou o motivo.
pub type Pdf<'a> = &'a dyn Fn(&Path, &Path) -> Result<(), String>;

fn pdftotext_real(pdf: &Path, txt: &Path) -> Result<(), String> {
    let prog = std::env::var("BRANIAC_PDFTOTEXT").unwrap_or_else(|_| "pdftotext".into());
    match rodar(&prog, &["-layout".into(), pdf.into(), txt.into()], Duration::from_secs(300)) {
        Ok((0, _)) => Ok(()),
        Ok((cod, err)) => Err(format!("o pdftotext falhou (código {cod}): {}", fim(&err))),
        Err(e) if e.starts_with("não consegui rodar") => Err(SEM_PDFTOTEXT.into()),
        Err(e) => Err(format!("pdftotext: {e}")),
    }
}

/// (nome, texto) de cada PDF do tribunal da pasta ("AAAA-....pdf" em documentos_tcm ou documentos), lido AGORA pelo pdftotext.
/// O .txt ao lado e so rascunho desta rodada: o antigo e apagado antes, e .txt sem PDF nao entra (nao e prova).
fn textos(pasta: &Path, pdf: Pdf) -> Result<Vec<(String, String)>, String> {
    let mut out = vec![];
    for sub in ["documentos_tcm", "documentos"] {
        let mut pdfs: Vec<PathBuf> = std::fs::read_dir(pasta.join(sub)).into_iter().flatten().flatten().map(|d| d.path()).filter(|p| {
            let n = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            n.len() > 4 && n.as_bytes()[..4].iter().all(u8::is_ascii_digit) && n.ends_with(".pdf")
        }).collect();
        pdfs.sort();
        for p in pdfs {
            let (nome, txt) = (p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), p.with_extension("txt"));
            let _ = std::fs::remove_file(&txt);
            pdf(&p, &txt).map_err(|e| if e == SEM_PDFTOTEXT { e } else { format!("{nome}: {e}") })?;
            let b = std::fs::read(&txt).map_err(|_| format!("{nome}: o pdftotext terminou sem gravar o texto"))?;
            out.push((nome, String::from_utf8_lossy(&b).into_owned()));
        }
    }
    Ok(out)
}

/// Roda um gerador do pacote: (script, sombra, pasta do levantamento) -> (codigo de saida, fim do que escreveu de erro). Err = nao rodou.
pub type Gerador<'a> = &'a dyn Fn(&Path, &Path, &Path) -> Result<(i32, String), String>;

/// Python isolado (-I), UTF-8, sem janela, com tempo limite. --refazer: a sombra e sempre desta cidade; --teste: nao registra
/// nada no Root.tsx (o registro grava no pacote).
fn gerador_real(script: &Path, sombra: &Path, pasta: &Path) -> Result<(i32, String), String> {
    let python = std::env::var("BRANIAC_PYTHON").unwrap_or_else(|_| "python".into());
    rodar(&python, &["-I".into(), "-X".into(), "utf8".into(), "-c".into(), LANCADOR.into(), script.into(), sombra.into(), pasta.into(), "--refazer".into(), "--teste".into()], TEMPO_ROTEIRO)
}

fn copia_pasta(de: &Path, para: &Path) -> Result<(), String> {
    std::fs::create_dir_all(para).map_err(|e| format!("{}: {e}", para.display()))?;
    for d in std::fs::read_dir(de).map_err(|e| format!("{}: {e}", de.display()))?.flatten() {
        let (o, n) = (d.path(), para.join(d.file_name()));
        if o.is_dir() {
            copia_pasta(&o, &n)?;
        } else {
            std::fs::copy(&o, &n).map_err(|e| format!("{}: {e}", o.display()))?;
        }
    }
    Ok(())
}

/// Monta a sombra do pacote: so o que o gerador LE (os dois moldes, copiados; o fontes.py por um atalho que roda o do pacote,
/// que so grava no arquivo que recebe por argumento). O que faltar no pacote sai dito pelo caminho.
fn prepara_sombra(pacote: &Path, sombra: &Path) -> Result<(), String> {
    let (tsx, kit, fontes) = (junta(pacote, &MOLDE_TSX), junta(pacote, &MOLDE_KIT), pacote.join("scripts-braniac").join("fontes.py"));
    if let Some(f) = [&tsx, &fontes].into_iter().find(|p| !p.is_file()).or(Some(&kit).filter(|k| !k.is_dir())) {
        return Err(format!("falta no pacote de produção: {}", f.display()));
    }
    let cria = |p: PathBuf| std::fs::create_dir_all(&p).map(|_| p.clone()).map_err(|e| format!("{}: {e}", p.display()));
    cria(sombra.join("videos"))?;
    let d = cria(junta(sombra, &MOLDE_TSX[..4]))?;
    std::fs::copy(&tsx, d.join(MOLDE_TSX[4])).map_err(|e| format!("{}: {e}", tsx.display()))?;
    copia_pasta(&kit, &junta(sombra, &MOLDE_KIT))?;
    let atalho = format!("import runpy\nrunpy.run_path({}, run_name='__main__')\n", Value::from(fontes.to_string_lossy().into_owned()));
    std::fs::write(cria(sombra.join("scripts-braniac"))?.join("fontes.py"), atalho).map_err(|e| e.to_string())
}

/// Texto do levantamento que vira nome de pasta ou de arquivo na mao do gerador.
fn nome_de_pasta(s: &str) -> bool {
    !s.is_empty() && s.chars().count() <= 80 && !s.starts_with('.') && !s.contains("..") && s.chars().all(|c| c.is_alphanumeric() || "-_. '".contains(c))
}

fn gera(pasta: &Path, numero: &str, pacote: &Path, sombra: &Path, gerador: Gerador) -> Result<Value, String> {
    let s = pacote.join("scripts-braniac");
    let (g_rot, g_leg) = (s.join("gera_cidade.py"), s.join("gera_legenda.py"));
    if !g_rot.is_file() {
        return Err(format!("falta no pacote de produção: {} (a pasta do pacote vem da variável BRANIAC_PACOTE ou de dados/parceiro/pacote)", g_rot.display()));
    }
    let cid = ler_json(&pasta.join("saida").join("cidade.json")).ok_or("levantamento com o cidade.json ilegível")?;
    if cid["numero"].as_str() != Some(numero) {
        return Err("o cidade.json do levantamento é de outro número".into());
    }
    if let Some(k) = ["nome", "codigo", "comp", "kit_pasta", "arquivo_mapa"].into_iter().find(|k| !cid[*k].as_str().is_some_and(nome_de_pasta)) {
        return Err(format!("o cidade.json do levantamento está sem '{k}' ou com um '{k}' que não serve de nome de pasta"));
    }
    prepara_sombra(pacote, sombra)?;
    let roda = |script: &Path, o_que: &str| -> Result<(), String> {
        let (cod, err) = gerador(script, sombra, pasta).map_err(|e| format!("{o_que}: {e}"))?;
        if cod == 0 { Ok(()) } else { Err(format!("{o_que} falhou (código {cod}): {}", fim(&err))) }
    };
    roda(&g_rot, "o gerador do roteiro (gera_cidade.py do pacote)")?;
    let prep = ler_json(&sombra.join("videos").join(numero).join("preparo.json")).ok_or("o gerador do roteiro terminou sem gravar o preparo.json")?;
    let (Some(projeto), Some(nome)) = (prep["projeto"].as_str().filter(|p| nome_de_pasta(p)), prep["nome"].as_str().filter(|p| nome_de_pasta(p))) else {
        return Err("o preparo.json que o gerador gravou está sem projeto ou nome".into());
    };
    let estudio = sombra.join("estudio-3d");
    let rot = std::fs::read_to_string(estudio.join("projetos").join(projeto).join("audio").join("roteiro-falado.txt")).unwrap_or_default();
    if rot.trim().is_empty() {
        return Err("o gerador do roteiro terminou sem gravar o roteiro".into());
    }
    let mut notas = vec![];
    let legenda = g_leg.is_file();
    if legenda {
        roda(&g_leg, "a legenda (gera_legenda.py do pacote)")?;
        if !estudio.join("edicoes").join(projeto).join(format!("{nome} - legenda.txt")).is_file() {
            return Err("o gerador da legenda terminou sem gravar a legenda".into());
        }
    } else {
        notas.push("o pacote não tem o gera_legenda.py: ficou sem legenda".to_string());
    }
    Ok(json!({ "numero": numero, "palavras": rot.split_whitespace().count(), "legenda": legenda, "notas": notas, "pasta": sombra.display().to_string() }))
}

/// ROTEIRO sob pedido: so de levantamento pronto. O resultado fica em <levantamento>/roteiro e o estado em roteiro.json.
/// Qualquer falha = pendencia gravada e nenhum roteiro para tras. Roteiro novo derruba a conferencia, a releitura e os
/// documentos feitos com o antigo.
pub fn roteiro(dados: &Path, numero: &str, pacote: &Path, gerador: Gerador, quando: &str) -> Result<Value, String> {
    let e = estado_de(dados, numero)?;
    let pasta = pasta_do_numero(dados, numero).ok_or("não há levantamento com esse número")?;
    if e["estado"] != "pronto" || e["tem_levantamento"] != true {
        return Err(format!("o roteiro só sai de levantamento pronto (este está: {}{})", e["estado"].as_str().unwrap_or("sem registro"), e["motivo"].as_str().map(|m| format!(", {m}")).unwrap_or_default()));
    }
    let sombra = pasta.join(SOMBRA);
    for f in ["roteiro.json", "auditoria.json", "releitura.json"] {
        let _ = std::fs::remove_file(pasta.join(f));
    }
    apaga_pasta(&sombra);
    apaga_pasta(&pasta.join("documentos-capital"));
    let r = gera(&pasta, numero, pacote, &sombra, gerador);
    let v = match &r {
        Ok(v) => {
            let mut v = v.clone();
            v["estado"] = json!("pronto");
            v["quando"] = json!(quando);
            v
        }
        Err(m) => {
            apaga_pasta(&sombra);
            json!({ "estado": "pendencia", "motivo": m, "quando": quando })
        }
    };
    gravar_json(&pasta.join("roteiro.json"), &v)?;
    r.map(|_| v)
}

/// Auditoria sob pedido do levantamento gravado (e do roteiro e da legenda, se ja foram gerados aqui). Com PDF na pasta, o
/// pdftotext le cada um de novo; sem PDF so passa levantamento de dados abertos do tribunal, e a origem fica para a releitura.
pub fn conferir(dados: &Path, numero: &str, pdf: Pdf) -> Result<Value, String> {
    let pasta = pasta_do_numero(dados, numero).ok_or("não há levantamento gravado com esse número")?;
    let Some(cid) = ler_json(&pasta.join("saida").join("cidade.json")) else {
        let motivo = ler_json(&pasta.join("estado.json")).and_then(|e| e["motivo"].as_str().map(String::from)).unwrap_or_default();
        return Err(format!("o levantamento não terminou, não há o que auditar{}", if motivo.is_empty() { String::new() } else { format!(" (pendência: {motivo})") }));
    };
    let t = textos(&pasta, pdf)?;
    if t.is_empty() && cid["fonte"] != "tribunal" {
        return Err(format!("faltam os PDFs do tribunal (arquivos AAAA-MM_NOME.pdf em {})", pasta.join("documentos_tcm").display()));
    }
    let (d, l) = (auditar::documentos(&t), locais(&pasta));
    let problemas = auditar::porta(&l, &pasta, &d, &|_| None);
    let notas = auditar::conferir(&l, &pasta, &d).map(|x| x.1).unwrap_or_default();
    let r = json!({ "numero": numero, "ok": problemas.is_empty(), "problemas": problemas, "notas": notas, "documentos": t.len(), "precisa_reler": t.is_empty(), "quando": hora() });
    gravar_json(&pasta.join("auditoria.json"), &r)?;
    Ok(r)
}

/// A releitura gravada. "fazendo" sem thread viva = o programa reiniciou no meio.
fn releitura_de(pasta: &Path, numero: &str) -> Value {
    let mut r = ler_json(&pasta.join("releitura.json")).unwrap_or(Value::Null);
    if r["estado"] == "fazendo" && rodando().as_deref() != Some(numero) {
        r = json!({ "estado": "pendencia", "motivo": "o programa reiniciou no meio da releitura; peça de novo" });
    }
    r
}

/// O que a releitura do tribunal diz para a porta: so a lista vazia de uma releitura que fechou deixa passar.
fn da_releitura(pasta: &Path, numero: &str) -> Vec<String> {
    let r = releitura_de(pasta, numero);
    match r["estado"].as_str() {
        Some("pronto") => lista_de(&r["problemas"]),
        Some("fazendo") => vec!["a releitura do tribunal ainda está rodando".into()],
        Some(_) => vec![format!("a releitura do tribunal não fechou: {}", r["motivo"].as_str().unwrap_or("sem motivo anotado"))],
        None => vec!["falta reler o tribunal: sem PDF no PC, a prova da origem é o tribunal dar o mesmo número de novo".into()],
    }
}

/// PORTA: o video PODERIA seguir para a voz? So informa; nada segue daqui. Passo que falta, pendencia ou numero sem prova = barrado.
pub fn porta(dados: &Path, numero: &str) -> Result<Value, String> {
    let e = estado_de(dados, numero)?;
    let pasta = pasta_do_numero(dados, numero).ok_or("não há levantamento com esse número")?;
    let motivo = |v: &Value| v["motivo"].as_str().unwrap_or("sem motivo anotado").to_string();
    let (rot, aud) = (&e["roteiro"], &e["auditoria"]);
    let prob = if e["estado"] != "pronto" || e["tem_levantamento"] != true {
        vec![format!("o levantamento não está pronto ({})", e["motivo"].as_str().or(e["estado"].as_str()).unwrap_or("sem registro"))]
    } else if rot["estado"] != "pronto" {
        vec![if rot.is_null() { "o roteiro ainda não foi gerado".into() } else { format!("o roteiro ficou com pendência: {}", motivo(rot)) }]
    } else if aud.is_null() {
        vec!["a conferência (auditoria) ainda não foi feita".into()]
    } else if aud["documentos"] == 0 {
        // sem PDF: a conta e refeita agora (so arquivo local) e a origem vem da releitura do tribunal
        auditar::porta(&locais(&pasta), &pasta, &auditar::Docs::new(), &|_| Some(da_releitura(&pasta, numero)))
    } else {
        lista_de(&aud["problemas"]) // com PDF vale a ultima conferencia: o pdftotext so roda por pedido
    };
    Ok(json!({ "numero": numero, "liberado": prob.is_empty(), "situacao": if prob.is_empty() { "liberado" } else { "barrado" }, "problemas": prob, "aviso": format!("A porta só informa. {DAQUI}") }))
}

fn guias(dados: &Path) -> PathBuf {
    std::env::var_os("BRANIAC_GUIAS").map(PathBuf::from).unwrap_or_else(|| dados.join("parceiro").join("guias"))
}

/// A sigla do tribunal que o guia do estado manda citar.
// shortcut: sempre le o guia como capital (tribunal proprio do municipio, onde ha); os moldes de estado com tribunal
// municipal so para a capital (SP, RJ) so levantam a capital. Molde novo para o interior desses estados: passar o caso certo.
fn sigla_do_guia(dados: &Path, uf: &str) -> Result<String, String> {
    if uf.len() != 2 || !uf.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err("cidade.json com a UF fora do formato".into());
    }
    let arq = guias(dados).join(format!("{uf}.json"));
    let g = ler_json(&arq).ok_or_else(|| format!("falta o guia do estado ({}); aponte a pasta dos guias na variável BRANIAC_GUIAS ou ponha o arquivo em dados/parceiro/guias", arq.display()))?;
    crate::link_tribunais::guia(uf, &g, true)?["sigla"].as_str().map(String::from).ok_or_else(|| format!("guia de {uf} sem a sigla do tribunal"))
}

/// O jeito que `link_conferir_capitais` espera achar a cidade, montado dentro da sombra: o registro da capital e uma copia
/// (feita agora) do cidade.json e do setores.json na pasta com o nome que ele procura.
fn arranjo(pasta: &Path, numero: &str) -> Result<PathBuf, String> {
    let (arr, saida) = (pasta.join(SOMBRA), pasta.join("saida"));
    let cid = ler_json(&saida.join("cidade.json")).ok_or("levantamento com o cidade.json ilegível")?;
    if !cid["nome"].as_str().is_some_and(nome_de_pasta) || !cid["uf"].as_str().is_some_and(|u| u.len() == 2 && u.bytes().all(|b| b.is_ascii_uppercase())) {
        return Err("cidade.json sem nome ou UF".into());
    }
    std::fs::create_dir_all(arr.join("parceiro")).map_err(|e| e.to_string())?;
    gravar_json(&arr.join("parceiro").join("capitais.json"), &json!({ numero: { "cidade": cid["nome"], "uf": cid["uf"] } }))?;
    let copia = cc::pasta_de(&arr, &arr, numero).ok_or("cidade.json sem nome ou UF")?.0.join("saida");
    std::fs::create_dir_all(&copia).map_err(|e| e.to_string())?;
    for n in ["cidade.json", "setores.json"] {
        std::fs::copy(saida.join(n), copia.join(n)).map_err(|e| format!("{n}: {e}"))?;
    }
    Ok(arr)
}

/// RELEITURA (sincrona; a rota roda em thread): o mesmo molde le o tribunal de novo numa pasta limpa e cada numero tem de bater
/// no centavo; confere tambem o tribunal do guia e o texto do roteiro, da tela e da legenda. Ok(lista) = problemas (vazia = bate);
/// Err = nao deu para conferir (tribunal fora do ar, guia que falta): nao prova nada.
pub fn reler(pasta: &Path, numero: &str, f: &Fora, sigla: &dyn Fn(&str) -> Result<String, String>, agora_s: f64) -> Result<Vec<String>, String> {
    let arr = arranjo(pasta, numero)?;
    let relido = |n: &str, c: &Value| -> Result<(Value, Value), String> {
        let limpa = pasta.join("releitura-tmp");
        apaga_pasta(&limpa);
        let r = match (c["uf"].as_str(), c["nome"].as_str()) {
            (Some(uf), Some(nome)) => levantar(&limpa, uf, nome, n, f, agora_s).and_then(|novo| Ok((novo, ler_json(&limpa.join("saida").join("setores.json")).ok_or("a releitura terminou sem os setores")?))),
            _ => Err("cidade.json sem nome ou UF".into()),
        };
        apaga_pasta(&limpa);
        r
    };
    let p = cc::conferir(&arr, &arr, numero, true, &cc::Fora { reler: &relido, sigla_do_guia: sigla, voz: &|_| vec![], avisar: &|_| {} })?;
    Ok(p.into_iter().filter(|x| !x.starts_with("voz:")).collect())
}

/// Quem desenha o PDF-extrato: (destino, titulo, secoes).
pub type Desenha<'a> = &'a dyn Fn(&Path, &str, &[(String, Vec<String>)]) -> Result<(), String>;

/// PDF-extrato so de texto: Courier 7 em A4 deitado, 186 caracteres por linha, com a MESMA conta de pagina de
/// `link_docs_capitais::paginas` (o fontes.md cita a pagina). O texto ja chega em latin-1 e sem nome de pessoa.
pub fn pdf_extrato(destino: &Path, titulo: &str, secoes: &[(String, Vec<String>)]) -> Result<(), String> {
    const W: f64 = 297.0 * 72.0 / 25.4;
    const H: f64 = 210.0 * 72.0 / 25.4;
    const LARG: usize = 186;
    fn texto(p: &mut Vec<u8>, fonte: &str, x: f64, y: f64, s: &str) {
        p.extend(format!("BT /{fonte} Tf {x:.2} {y:.2} Td (").bytes());
        for c in s.chars().take(LARG) {
            match c {
                '\\' | '(' | ')' => p.extend([b'\\', c as u8]),
                c if (c as u32) < 256 && !c.is_control() => p.push(c as u8),
                _ => p.push(b'?'),
            }
        }
        p.extend(b") Tj ET\n");
    }
    let nova = |pags: &mut Vec<Vec<u8>>| {
        let mut p = vec![];
        texto(&mut p, "F2 8", 24.0, H - 22.0, titulo);
        texto(&mut p, "F1 7", W - 60.0, 18.0, &format!("p. {}", pags.len() + 1));
        pags.push(p);
        H - 40.0
    };
    let mut pags: Vec<Vec<u8>> = vec![];
    let mut y = nova(&mut pags);
    for (st, linhas) in secoes {
        if y < 80.0 {
            y = nova(&mut pags);
        }
        texto(pags.last_mut().ok_or("pdf sem página")?, "F2 8", 24.0, y, st);
        y -= 12.0;
        for ln in linhas {
            let cs: Vec<char> = ln.chars().collect();
            for i in 0..cs.len().max(1).div_ceil(LARG) {
                if y < 30.0 {
                    y = nova(&mut pags);
                }
                let pedaco: String = cs.iter().skip(i * LARG).take(LARG).collect();
                texto(pags.last_mut().ok_or("pdf sem página")?, "F1 7", 24.0, y, &pedaco);
                y -= 8.4;
            }
        }
        y -= 8.0;
    }
    // objetos: 1 catalogo, 2 paginas, 3 e 4 fontes, depois (pagina, conteudo) de cada pagina
    let mut objs: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!("<< /Type /Pages /Count {} /Kids [{}] >>", pags.len(), (0..pags.len()).map(|i| format!("{} 0 R", 5 + 2 * i)).collect::<Vec<_>>().join(" ")).into_bytes(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Courier /Encoding /WinAnsiEncoding >>".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Courier-Bold /Encoding /WinAnsiEncoding >>".to_vec(),
    ];
    for (i, p) in pags.iter().enumerate() {
        objs.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {W:.2} {H:.2}] /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /Contents {} 0 R >>", 6 + 2 * i).into_bytes());
        let mut c = format!("<< /Length {} >>\nstream\n", p.len()).into_bytes();
        c.extend(p);
        c.extend(b"endstream");
        objs.push(c);
    }
    let (mut out, mut onde) = (b"%PDF-1.4\n".to_vec(), vec![]);
    for (i, o) in objs.iter().enumerate() {
        onde.push(out.len());
        out.extend(format!("{} 0 obj\n", i + 1).bytes());
        out.extend(o);
        out.extend(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).bytes());
    for o in onde {
        out.extend(format!("{o:010} 00000 n \n").bytes());
    }
    out.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).bytes());
    std::fs::write(destino, out).map_err(|e| format!("{}: {e}", destino.display()))
}

/// O que falta para montar os documentos do tribunal daquela UF (None = da para montar).
fn falta_para_documentos(uf: &str) -> Option<String> {
    let preso = "o adaptador de documentos desse estado só aceita o número e a pasta do vídeo do sistema atual (dentro do pacote)";
    match uf {
        "CE" => None,
        "ES" => Some(format!("ES: o adaptador lê tce-es/despesasAAAA.json e receitasAAAA.json (só as linhas do município) e o mes26.txt, e o molde em Rust guarda os zips do CidadES e o pcm.csv; além disso, {preso}")),
        "RJ" | "GO" => Some(format!("{uf}: o adaptador lê as respostas do painel guardadas em JSON (despesas, receitas, orçamento ou origens, e o mes26.txt) e o molde em Rust não guarda nenhuma resposta do painel; além disso, {preso}")),
        "TO" | "RS" => Some(format!("{uf}: os arquivos do tribunal estão guardados, mas {preso} e pede o mês ou a remessa de corte por fora")),
        "PR" => Some(format!("PR: o adaptador lê os XML já abertos (AAAA_Despesa/..., AAAA_Receita/...) e o molde em Rust guarda os zips do PIT fechados; além disso, {preso}")),
        "SP" | "MA" | "RN" => Some(format!("{uf}: ainda não existe adaptador de documentos para esse tribunal")),
        _ => Some(format!("{uf}: sem leitura do tribunal ligada, não há de onde montar os documentos")),
    }
}

/// Os documentos do TCE-CE a partir do que o levantamento guardou em tce-ce/ (SIM pela API de Dados Abertos): as mesmas tabelas
/// e colunas de `link_docs_cap::ce`, que so serve ao video 997 dentro do pacote; aqui valem o numero e a pasta do levantamento.
fn docs_ce(pasta: &Path, site: &Path, numero: &str, cidade: &str, desenha: Desenha) -> Result<(PathBuf, Value), String> {
    const API: &str = "https://api-dados-abertos.tce.ce.gov.br/sim/";
    const CAMPOS: [&str; 8] = ["codigo_orgao", "codigo_unidade_orcamentaria", "codigo_funcao", "codigo_subfuncao", "codigo_programa", "codigo_projeto_atividade", "numero_projeto_atividade", "numero_subprojeto_atividade"];
    let src = pasta.join("tce-ce");
    let falta = |n: &str| format!("falta o arquivo de trabalho tce-ce/{n}, que o levantamento guarda; levante de novo");
    let mes26 = std::fs::read_to_string(src.join("mes26.txt")).map_err(|_| falta("mes26.txt"))?.trim().to_string();
    if mes26.len() != 2 || !mes26.parse::<u8>().is_ok_and(|n| (1..=12).contains(&n)) {
        return Err(format!("tce-ce/mes26.txt: mês de corte inválido {mes26:?}"));
    }
    let p = docs::nova_pasta(site, numero, cidade)?;
    let (mut arqs, mut mun) = (vec![], String::new());
    for ano in cap::ANOS {
        let mes = cap::mes_de(ano, &mes26);
        let rf = format!("{ano}{mes}");
        for (tipo, arq, met, rot) in [("despesa", format!("despesa{rf}"), "balancetes_despesas_orcamentarias", "SIM-BALANCETE-DESPESA"), ("receita", format!("receita{rf}"), "balancetes_receitas_orcamentarias", "SIM-BALANCETE-RECEITA"), ("outro", format!("orcamento{ano}"), "orcamento_despesas_projetos_atividades", "SIM-ORCAMENTO-NOMES-ACOES")] {
            let v = ler_json(&src.join(format!("{arq}.json"))).ok_or_else(|| falta(&format!("{arq}.json")))?;
            if mun.is_empty() {
                mun = v[0]["codigo_municipio"].as_str().unwrap_or("").to_string();
            }
            let mut t = docs::tabela_json(&v);
            if tipo == "despesa" {
                // o codigo inteiro numa coluna: os pedacos soltos ("1", "2"...) casam com outros campos e puxam linha de acao vizinha
                let i = cap::posicoes(&t[0], &CAMPOS, &arq)?;
                let cods = t[1..].iter().map(|r| i.iter().map(|k| r[*k].trim()).collect::<Vec<_>>().join(".")).collect();
                t = cap::com_coluna("codigo_acao_video", cods, t);
            }
            let f = docs::gravar(&p, &format!("{ano}-{mes}_CE-{rot}.txt"), &t)?;
            let filtro = if tipo == "outro" { String::new() } else { format!("&data_referencia_doc={rf}") };
            arqs.push(docs::Arq::novo(ano, tipo, f, format!("TCE-CE SIM, API de Dados Abertos: {API}{met}?codigo_municipio={mun}&exercicio_orcamento={ano}00{filtro} (todos os órgãos, um por consulta: a paginação da API não é estável){}{}",
                if tipo == "outro" { " - só o nome de cada projeto/atividade" } else { "" }, if tipo == "despesa" { "; 1a coluna acrescentada = código da ação do vídeo, tirado das colunas da própria linha" } else { "" })));
        }
    }
    let mut ad = docs::Adaptador { pasta: p, tribunal: "TCE-CE".into(), arquivos: arqs, partes: &cap::inteiro, codigos: None, faixa_mx: 6,
        col: cap::colunas("'valor_liquidado_ate_mes' menos 'valor_estornos_liquidacao_ate_mes'", "'valor_fixado_orcamento_bal_despesa' (dotação inicial)", "'valor_empenhado_ate_mes' menos 'valor_anulacoes_empenhos_ate_mes'", "'valor_arrecadacao_ate_mes' menos 'valor_anulacoes_ate_mes' (rubricas 95... = deduções do Fundeb, negativas)"),
        filtro: cap::filtros(&|a, _| format!("todas as linhas (todos os órgãos), balancete acumulado até {}/{a}", cap::mes_de(a, &mes26))) };
    docs::montar(site, numero, cidade, "CE", &mut ad, pasta, desenha)
}

/// DOCUMENTOS DO TRIBUNAL sob pedido, em <levantamento>/documentos-capital. Parte do fontes.md que o gerador do roteiro escreveu
/// e do que o levantamento guardou do tribunal. Nada e enviado. Falha = nenhuma pasta para tras.
pub fn documentos(dados: &Path, numero: &str, desenha: Desenha) -> Result<Value, String> {
    let e = estado_de(dados, numero)?;
    let pasta = pasta_do_numero(dados, numero).ok_or("não há levantamento com esse número")?;
    let cid = ler_json(&pasta.join("saida").join("cidade.json")).filter(|_| e["estado"] == "pronto").ok_or("os documentos só saem de levantamento pronto")?;
    let (Some(uf), Some(cidade)) = (cid["uf"].as_str(), cid["nome"].as_str().filter(|n| nome_de_pasta(n))) else { return Err("cidade.json sem nome ou UF".into()) };
    if let Some(f) = falta_para_documentos(uf) {
        return Err(format!("documentos não ligados para {f}"));
    }
    let sombra = pasta.join(SOMBRA);
    let projeto = ler_json(&sombra.join("videos").join(numero).join("preparo.json")).filter(|_| e["roteiro"]["estado"] == "pronto").and_then(|p| p["projeto"].as_str().map(String::from));
    let fontes = projeto.map(|p| sombra.join("estudio-3d").join("projetos").join(p).join("audio").join("fontes.md")).filter(|f| f.is_file())
        .ok_or("gere o roteiro primeiro: os documentos partem do fontes.md que o gerador do roteiro escreve")?;
    let site = pasta.join("documentos-capital");
    apaga_pasta(&site);
    // o motor procura o fontes.md na pasta do video dentro do "site": aqui o site e a propria pasta dos documentos
    let video = site.join(format!("{numero}-{}", docs::slug(cidade)));
    let r = std::fs::create_dir_all(&video).and_then(|_| std::fs::copy(&fontes, video.join("fontes.md"))).map_err(|e| e.to_string())
        .and_then(|_| docs_ce(&pasta, &site, numero, cidade, desenha))
        .and_then(|(p, rel)| {
            let v = json!({ "numero": numero, "uf": uf, "pasta": p.display().to_string(), "relatorio": rel, "quando": hora(), "aviso": "Nada foi enviado." });
            gravar_json(&site.join("relatorio.json"), &v).map(|_| v)
        });
    r.inspect_err(|_| apaga_pasta(&site))
}

/// O que esta errado, pela ultima conferencia gravada. A triagem nao rele o tribunal: o que depender disso vai para pessoa.
pub fn triagem(dados: &Path, numero: &str) -> Result<Value, String> {
    let pasta = pasta_do_numero(dados, numero).ok_or("não há levantamento gravado com esse número")?;
    let a = ler_json(&pasta.join("auditoria.json")).ok_or("ainda não houve conferência desse número; peça a conferência primeiro")?;
    let prob: Vec<String> = a["problemas"].as_array().into_iter().flatten().filter_map(|p| p.as_str().map(String::from)).collect();
    if prob.is_empty() {
        return Ok(json!({ "numero": numero, "categoria": null, "explicacao": "sem problema na última conferência: nada a triar" }));
    }
    let reler = |_: &Value, _: &Path| Err("a triagem ainda não relê o tribunal por aqui".to_string());
    let (cat, explicacao, nova) = auditar::triagem(&locais(&pasta), &pasta, &prob, &reler, &std::env::temp_dir())?;
    if let Some(n) = nova {
        let _ = std::fs::remove_dir_all(n);
    }
    let titulo = auditar::NOMES.iter().find(|n| n.0 == cat).map_or(cat, |n| n.1);
    Ok(json!({ "numero": numero, "categoria": cat, "titulo": titulo, "explicacao": explicacao, "problemas": prob }))
}

/// Rota GET deste grupo; None = nao e minha.
pub fn get(dados: &Path, _rot: &Arc<Roteador>, url: &str) -> Option<Resp> {
    let numero = || crate::parametro(url, "numero").unwrap_or_default();
    let ou404 = |r: Result<Value, String>| r.map(crate::dado).unwrap_or_else(|e| crate::erro(e, 404));
    Some(match url.split('?').next().unwrap_or("") {
        "/tribunais" => crate::pagina(include_str!("tribunais.html")),
        "/api/tribunais" => crate::dado(json!({ "aviso": AVISO, "daqui": DAQUI, "rodando": rodando(), "pacote": script_de_setores(&pacote(dados)).is_file(), "falta_pacote": SEM_PACOTE, "estados": MOLDES.iter().map(|m| despacho(m.0)).collect::<Vec<_>>() })),
        "/api/tribunais/levantamento" => ou404(estado_de(dados, &numero())),
        "/api/tribunais/porta" => ou404(porta(dados, &numero())),
        "/api/auditoria/triagem" => ou404(triagem(dados, &numero())),
        "/api/esteira/processo" => {
            let ts = crate::fatos::agora_ts();
            crate::feito(crate::link_processo::painel(dados, &crate::link_producao::pedidos(dados), ts, ts - crate::tempo::agora().segundos() as f64))
        }
        "/api/esteira/orq" => crate::dado(crate::esteira_orq::estado_http(dados)),
        "/api/esteira/producao" => crate::dado(json!({ "pedidos": crate::link_producao::pedidos(dados) })),
        "/api/esteira/pdfs" => {
            let l = crate::link_pdfs_tcm::Locais { pacote: pacote(dados), site: dados.join("parceiro").join("site"), tmp: std::env::temp_dir() };
            crate::dado(json!({ "cidades": crate::link_pdfs_tcm::cidades(dados, &l) }))
        }
        "/api/ibge" => {
            let (pasta, uf) = (dados.join("ibge"), crate::parametro(url, "uf").unwrap_or_default());
            match crate::ibge_cidades::estado(&pasta, &uf) {
                None => crate::erro("lista do IBGE desse estado ainda não foi baixada (ou UF errada)", 404),
                Some(e) => {
                    let cidades = match crate::parametro(url, "q").filter(|q| !q.trim().is_empty()) {
                        Some(q) => crate::ibge_cidades::procurar(&pasta, &uf, &q),
                        None => e["cidades"].as_array().cloned().unwrap_or_default(),
                    };
                    crate::dado(json!({ "uf": e["uf"], "estado": e["estado"], "capital": e["capital"], "baixado_em": e["baixado_em"], "total": cidades.len(), "cidades": cidades }))
                }
            }
        }
        _ => return None,
    })
}

/// Rota POST deste grupo (ja passou pelo cabecalho X-Braniac); None = nao e minha.
pub fn post(dados: &Path, _rot: &Arc<Roteador>, caminho: &str, v: &Value) -> Option<Resp> {
    let numero = v["numero"].as_str().map(String::from).or_else(|| v["numero"].as_i64().map(|n| n.to_string())).unwrap_or_default();
    let ou400 = |r: Result<Value, String>| r.map(crate::dado).unwrap_or_else(|e| crate::erro(e, 400));
    Some(match caminho {
        "/api/tribunais/levantar" => ou400(comecar(dados, v)),
        "/api/esteira/orq" => ou400(crate::esteira_orq::ligar_http(dados, v)),
        "/api/auditoria/conferir" => ou400(conferir(dados, &numero, &pdftotext_real)),
        "/api/auditoria/reler" => ou400(comecar_releitura(dados, &numero)),
        "/api/tribunais/roteiro" => {
            // um gerador por vez: dois ao mesmo tempo no mesmo numero apagariam a sombra um do outro
            static UM: Mutex<()> = Mutex::new(());
            match UM.try_lock() {
                Ok(_t) => ou400(roteiro(dados, &numero, &pacote(dados), &gerador_real, &hora())),
                Err(_) => crate::erro("já tem um roteiro sendo gerado; espere terminar", 400),
            }
        }
        "/api/tribunais/documentos" => ou400(documentos(dados, &numero, &pdf_extrato)),
        // lista oficial do IBGE (API publica, sem chave): 29 consultas, uma por vez
        "/api/ibge/baixar" => crate::feito(crate::ibge_cidades::baixar(&dados.join("ibge"), crate::tempo::agora().data, &|c| {
            dorme_real(0.3);
            let url = format!("{}/{c}", crate::ibge_cidades::API);
            serde_json::from_slice(&ok200(&url, rede_real(&url, &[], None)?)?).map_err(|e| format!("IBGE: {e}"))
        })),
        _ => return None,
    })
}

/// Relogio de 60 s: so garante as linhas do maestro da esteira no ar se `esteira_ligada` existir (nasce desligado).
/// Levantamento, roteiro, auditoria, releitura, documentos e IBGE continuam so por pedido da pessoa.
pub fn tick(dados: &Path, _rot: &Roteador, _agora: &Agora, _ts: f64, _minuto: u32) {
    crate::esteira_orq::tick(dados);
    crate::esteira_vigias::tick(dados); // painel, mp4 e esteira: cada um na sua thread, nao bloqueia o relogio
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp(nome: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("braniac-liga_esteira-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// Uma pasta de pacote so com o lugar do script de setores (o script de verdade e de terceiro e nao entra em teste).
    fn pacote_falso(d: &Path) -> PathBuf {
        let p = d.join("pacote");
        std::fs::create_dir_all(p.join("scripts-braniac")).unwrap();
        for s in ["setores.py", "gera_cidade.py", "gera_legenda.py", "fontes.py"] {
            std::fs::write(p.join("scripts-braniac").join(s), "").unwrap();
        }
        // os dois moldes que o gerador do roteiro le
        std::fs::create_dir_all(junta(&p, &MOLDE_TSX[..4])).unwrap();
        std::fs::write(junta(&p, &MOLDE_TSX), "molde").unwrap();
        std::fs::create_dir_all(junta(&p, &MOLDE_KIT).join("dados")).unwrap();
        std::fs::write(junta(&p, &MOLDE_KIT).join("dados").join("salvador.json"), "{}").unwrap();
        p
    }

    /// Faz o papel do processo do script: festas = soma do liquidado das acoes com FESTA no nome, por arquivo acoes-AAAA-MM.json.
    fn script_falso(_: &Path, saida: &Path) -> Result<(i32, String), String> {
        let mut st = serde_json::Map::new();
        for d in std::fs::read_dir(saida).unwrap().flatten() {
            let n = d.file_name().to_string_lossy().to_string();
            if let Some(k) = n.strip_prefix("acoes-").and_then(|x| x.strip_suffix(".json")) {
                let v = ler_json(&d.path()).unwrap();
                let s: f64 = v.as_array().unwrap().iter().filter(|x| x["acao"].as_str().unwrap().to_uppercase().contains("FESTA")).map(|x| x["liquidado"].as_f64().unwrap()).sum();
                st.insert(k.into(), json!({ "Festas e eventos": { "liquidado": s } }));
            }
        }
        std::fs::write(saida.join("setores.json"), Value::Object(st).to_string()).unwrap();
        Ok((0, String::new()))
    }

    fn sem_ibge(_: &str, _: &str) -> Option<i64> {
        None
    }

    fn sem_pausa(_: f64) {}

    fn nunca_rede(u: &str, _: &[(&str, &str)], _: Option<&str>) -> Result<(u16, Vec<u8>), String> {
        panic!("nao era para consultar nada: {u}")
    }

    /// O de fora dos testes: rede dada, sem pausa, script de mentira, sem lista do IBGE.
    fn fora<'a>(http: Http<'a>, pacote: &'a Path) -> Fora<'a> {
        Fora { http, dorme: &sem_pausa, script: &script_falso, pacote, ibge: &sem_ibge }
    }

    #[test]
    fn despacho_por_uf() {
        for uf in ["CE", "RS", "TO", "ES", "PR", "SP", "MA", "GO", "RJ", "RN"] {
            assert!(despacho(uf)["levantar"] == true && despacho(uf)["situacao"] == "rede ligada", "{uf}");
        }
        for uf in ["SC", "AC", "MS", "AM", "PE", "PI", "MT", "BA", "MG", "PB"] {
            let d = despacho(uf);
            assert!(d["levantar"] == false && d["situacao"] == "sem rede" && d["texto"].as_str().unwrap().starts_with(NAVEGADOR), "{uf}");
        }
        assert_eq!(despacho("ce")["tribunal"], "TCE-CE");
        assert_eq!(despacho("XX")["situacao"], "sem molde");
        assert_eq!(despacho("DF")["situacao"], "sem molde");
        // as 27 UFs, cada uma uma vez
        let mut ufs: Vec<&str> = MOLDES.iter().map(|m| m.0).collect();
        ufs.dedup();
        assert_eq!(ufs.len(), 27);
    }

    #[test]
    fn uf_pendente_devolve_o_motivo() {
        for uf in ["AL", "AP", "PA", "RR", "SE", "RO"] {
            let d = despacho(uf);
            assert!(d["situacao"] == "pendente" && d["levantar"] == false && d["texto"].as_str().unwrap().len() > 60, "{uf}");
        }
        assert!(despacho("PA")["texto"].as_str().unwrap().contains("TCM-PA não publica"));
        let d = tmp("pendente");
        let e = pedido(&d, &json!({ "uf": "SE", "cidade": "Aracaju", "numero": 1005 })).unwrap_err();
        assert!(e.starts_with("SE: Pendente: TCE-SE sem base pública"), "{e}");
        let e = pedido(&d, &json!({ "uf": "SC", "cidade": "Florianópolis", "numero": 1006 })).unwrap_err();
        assert!(e.contains(NAVEGADOR), "{e}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn pedido_confere_e_siconfi_recusa() {
        let d = tmp("pedido");
        let (uf, cidade, numero, pasta) = pedido(&d, &json!({ "uf": "ce", "cidade": " Fortaleza ", "numero": 997 })).unwrap();
        assert_eq!((uf.as_str(), cidade.as_str(), numero.as_str()), ("CE", "Fortaleza", "997"));
        assert!(pasta.ends_with("parceiro/levantamentos/997-fortaleza"));
        for ruim in [json!({ "uf": "CE", "cidade": "Fortaleza", "numero": "../1" }), json!({ "uf": "CE", "cidade": "..\\x", "numero": "1" }), json!({ "uf": "CE", "cidade": "", "numero": "1" }), json!({ "uf": "CE", "cidade": "Fortaleza", "numero": "12345" })] {
            assert!(pedido(&d, &ruim).is_err(), "{ruim}");
        }
        // RREO/Tesouro nunca vale como fonte
        assert!(!crate::link_fonte_siconfi::serve_de_fonte());
        let e = pedido(&d, &json!({ "uf": "CE", "cidade": "Fortaleza", "numero": 997, "fonte": "siconfi" })).unwrap_err();
        assert!(e.contains("tribunal de contas"), "{e}");
        assert!(!raiz(&d).exists());
        // a lista do IBGE gravada da o codigo pelo nome, sem contar acento e caixa
        std::fs::create_dir_all(d.join("ibge")).unwrap();
        std::fs::write(d.join("ibge").join("ES.json"), json!({ "cidades": [{ "nome": "Vitória", "ibge": 3205309 }, { "nome": "Vitória Nova", "ibge": 1 }] }).to_string()).unwrap();
        assert_eq!((ibge_de(&d, "ES", "vitoria"), ibge_de(&d, "ES", "Vit"), ibge_de(&d, "RJ", "Vitória")), (Some(3205309), None, None));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Tribunal de mentira: a lista de municipios vem certa, mas o orcamento vem com uma linha repetida no lugar de outra
    /// (o defeito da paginacao instavel do SIM).
    #[test]
    fn rede_que_perde_linha_vira_pendencia_sem_levantamento() {
        let d = tmp("perde");
        let pac = pacote_falso(&d);
        let pasta = raiz(&d).join("997-cidadeteste");
        let pedidos = RefCell::new(vec![]);
        let http = |url: &str, _: &[(&str, &str)], _: Option<&str>| -> Result<(u16, Vec<u8>), String> {
            pedidos.borrow_mut().push(url.to_string());
            let corpo = if url.contains("/sim/municipios?") {
                json!({ "elements": [{ "nome_municipio": "Cidade Teste", "codigo_municipio": "001", "codigo_municipio_ibge": "2300000" }] })
            } else {
                let l = json!({ "codigo_orgao": "01", "codigo_unidade_orcamentaria": "101", "codigo_funcao": "04", "codigo_subfuncao": "122", "nome_projeto_atividade": "A" });
                json!({ "elements": [l.clone(), l] })
            };
            Ok((200, corpo.to_string().into_bytes()))
        };
        let pausas = RefCell::new(0.0);
        let f = Fora { dorme: &|s| *pausas.borrow_mut() += s, ..fora(&http, &pac) };
        let e = levantar(&pasta, "CE", "Cidade Teste", "997", &f, 1.0e9).unwrap_err();
        assert!(e.contains("repetidas"), "{e}");
        assert!(!pasta.join("saida").exists() && !pasta.join("tce-ce").join("orcamento2024.json").exists());
        {
            let p = pedidos.borrow();
            assert!(p[0].starts_with("https://api-dados-abertos.tce.ce.gov.br/sim/municipios?%24count=1000") && p.len() == 2, "{p:?}");
            assert!(p[1].contains("exercicio_orcamento=202400") && *pausas.borrow() >= 1.0); // pausa antes de cada consulta
        }
        // tribunal fora do ar, resposta sem dados e UF sem rede tambem sao pendencia
        assert!(levantar(&pasta, "CE", "Cidade Teste", "997", &fora(&|u, _, _| Err(format!("{u}: tempo esgotado")), &pac), 1.0e9).unwrap_err().contains("tempo esgotado"));
        assert!(levantar(&pasta, "CE", "Cidade Teste", "997", &fora(&|_, _, _| Ok((200, b"{\"erro\":1}".to_vec())), &pac), 1.0e9).unwrap_err().contains("sem dados"));
        let nunca = fora(&nunca_rede, &pac);
        assert!(levantar(&pasta, "SC", "Florianópolis", "1006", &nunca, 1.0e9).unwrap_err().starts_with(NAVEGADOR));
        // molde de uma cidade so recusa as outras sem consultar
        for (uf, cidade, diz) in [("RS", "Pelotas", "só com a capital"), ("TO", "Araguaína", "Palmas"), ("PR", "Londrina", "só com Curitiba"), ("SP", "Campinas", "só da capital"), ("MA", "Imperatriz", "só com São Luís"), ("GO", "Anápolis", "só com Goiânia"), ("RJ", "Niterói", "só da capital")] {
            assert!(levantar(&pasta, uf, cidade, "500", &nunca, 1.0e9).unwrap_err().contains(diz), "{uf}");
        }
        // ES sem o codigo do IBGE da cidade: pendencia antes de consultar
        assert!(levantar(&pasta, "ES", "Vitória", "500", &nunca, 1.0e9).unwrap_err().contains("código do IBGE"));
        // 404 fora da excecao da Camara e erro, nunca arquivo vazio
        assert!(levantar(&pasta, "RS", "Porto Alegre", "502", &fora(&|_, _, _| Ok((404, vec![])), &pac), 1.0e9).unwrap_err().contains("404"));
        assert!(!pasta.join("saida").exists());
        // sem a pasta do pacote o pedido para antes de gastar o tribunal
        let fora_do_lugar = d.join("nao-existe");
        let sem = fora(&nunca_rede, &fora_do_lugar);
        assert_eq!(levantar(&pasta, "CE", "Cidade Teste", "997", &sem, 1.0e9).unwrap_err(), SEM_PACOTE);
        assert!(SEM_PACOTE.starts_with("falta apontar a pasta do pacote de produção"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn setores_roda_o_script_do_pacote_como_processo() {
        let d = tmp("setores");
        let (pac, saida) = (pacote_falso(&d), d.join("saida"));
        std::fs::create_dir_all(&saida).unwrap();
        let com = |script: &dyn Fn(&Path, &Path) -> Result<(i32, String), String>| setores(&Fora { script, ..fora(&nunca_rede, &pac) }, &saida);
        // ok: o processo recebe o script do pacote e a pasta saida, e grava o setores.json
        let ok = |script: &Path, saida: &Path| {
            assert!(script.ends_with("pacote/scripts-braniac/setores.py") && saida.ends_with("saida"));
            std::fs::write(saida.join("setores.json"), "{}").map(|_| (0, String::new())).map_err(|e| e.to_string())
        };
        com(&ok).unwrap();
        // erro: codigo de saida diferente de zero, com o fim do que o script escreveu
        let e = com(&|_, _| Ok((1, format!("{}\nKeyError: 'liquidado'\n", "x".repeat(900))))).unwrap_err();
        assert!(e.contains("falhou (código 1)") && e.ends_with("KeyError: 'liquidado'") && e.len() < 400, "{e}");
        assert!(!saida.join("setores.json").exists()); // o setores.json da rodada boa nao fica valendo
        // tempo esgotado e python que nao existe
        assert!(com(&|_, _| Err("tempo esgotado (300 s)".into())).unwrap_err().contains("script de setores do pacote: tempo esgotado"));
        // terminou "bem" sem gravar nada
        assert!(com(&|_, _| Ok((0, String::new()))).unwrap_err().contains("sem gravar o setores.json"));
        // pacote ausente: a frase clara, sem rodar nada
        let outro = d.join("outro");
        assert!(setores(&Fora { script: &|_, _| panic!("sem pacote nao roda nada"), ..fora(&nunca_rede, &outro) }, &saida).unwrap_err().starts_with("falta apontar a pasta do pacote de produção"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(windows)]
    #[test]
    fn processo_de_verdade_tem_codigo_de_saida_e_tempo_limite() {
        assert_eq!(rodar("cmd", &["/c".into(), "exit 3".into()], Duration::from_secs(30)).unwrap().0, 3);
        // `waitfor` fica parado esperando um sinal que nunca vem: o tempo limite mata
        let t = std::time::Instant::now();
        assert!(rodar("waitfor", &["/t".into(), "60".into(), "braniacteste".into()], Duration::from_millis(400)).unwrap_err().contains("tempo esgotado"));
        assert!(t.elapsed() < Duration::from_secs(20));
        assert!(rodar("programa-que-nao-existe-braniac", &[], Duration::from_secs(5)).unwrap_err().contains("não consegui rodar"));
    }

    fn zipa(arqs: &[(&str, &[u8])]) -> Vec<u8> {
        let mut z = zip::ZipWriter::new(std::io::Cursor::new(vec![]));
        for (n, c) in arqs {
            z.start_file(*n, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(c).unwrap();
        }
        z.finish().unwrap().into_inner()
    }

    #[test]
    fn zip_so_entrega_o_arquivo_esperado() {
        let grande = vec![b'a'; 5000];
        let z = zipa(&[("leia.txt", b"x"), ("dados/2025_Empenho.xml", b"<A/>"), ("b.csv", &grande), ("a.csv", b"um;dois")]);
        // arquivo certo, pelo nome sem a pasta, em ordem de nome
        assert_eq!(do_zip(&z, &|n| n == "2025_Empenho.xml", 100, "z").unwrap(), [("dados/2025_Empenho.xml".to_string(), b"<A/>".to_vec())]);
        assert_eq!(um_do_zip(&z, &|n| n.ends_with(".csv"), "z").unwrap(), b"um;dois");
        // zip sem o arquivo esperado
        assert!(do_zip(&z, &|n| n == "Receita.xml", 100, "z").unwrap().is_empty());
        assert!(um_do_zip(&z, &|n| n == "Receita.xml", "z").unwrap_err().contains("zip sem o arquivo esperado"));
        // teto: conta o que sai de verdade; o que nao foi pedido nao conta
        assert!(do_zip(&z, &|n| n.ends_with(".csv"), 100, "z").unwrap_err().contains("passa do teto"));
        assert_eq!(do_zip(&z, &|n| n.ends_with(".csv"), 5007, "z").unwrap().len(), 2);
        // caminho com `..` (ou absoluto) recusa o zip inteiro, mesmo que a entrada nao seja a pedida
        for ruim in ["../fora.csv", "a/../../fora.csv", "..\\fora.csv", "/etc/x.csv", "C:/x.csv"] {
            let z = zipa(&[("a.csv", b"um"), (ruim, b"dois")]);
            assert!(do_zip(&z, &|n| n == "a.csv", 100, "z").unwrap_err().contains("caminho suspeito"), "{ruim}");
        }
        // download cortado no meio e o que nao e zip
        assert!(do_zip(&z[..z.len() / 2], &|_| true, 100, "z").unwrap_err().contains("zip que não abre"));
        assert!(do_zip(b"<html>erro</html>", &|_| true, 100, "z").is_err());
    }

    #[test]
    fn textos_do_tribunal_em_outras_codificacoes() {
        assert_eq!((CP1252_ALTO.chars().count(), CP850_ALTO.chars().count()), (32, 128));
        assert_eq!(cp1252(b"Munic\xedpio \x80 \x93a\x94", "x").unwrap(), "Município € “a”");
        assert!(cp1252(b"a\x81", "x").unwrap_err().contains("Windows-1252"));
        assert_eq!(cp850(b"DOTA\x80\xc7O, a\x87\xc6o e \xa1ndice"), "DOTAÇÃO, ação e índice");
        assert_eq!(xml_texto(b"<?xml version='1.0' encoding='ISO-8859-1'?><A n='\xe7'/>".to_vec(), "x").unwrap(), "<?xml version='1.0' encoding='ISO-8859-1'?><A n='ç'/>");
        assert!(xml_texto(b"<A n='\xe7'/>".to_vec(), "x").is_err());
        for (url, vale) in [("https://dados.es.gov.br/a.zip", true), ("https://x.tcees.tc.br/a", true), ("http://dados.es.gov.br/a.zip", false), ("https://127.0.0.1/a.gov.br", false), ("https://gov.br.exemplo.com/a", false), ("https://a.gov.br:8443/x", false), ("https://u@a.gov.br/x", false)] {
            assert_eq!(de_orgao_publico(url), vale, "{url}");
        }
    }

    /// A cidade de teste, igual em todos os tribunais de mentira: (acao, funcao, subfuncao, codigo, orcado, liquidado).
    const ACOES: [(&str, &str, &str, &str, f64, f64); 3] = [("FESTAS POPULARES", "13", "392", "2100", 1e6, 3e6), ("BENEFICIOS EVENTUAIS", "08", "244", "2200", 2e6, 2e6), ("ENSINO FUNDAMENTAL", "12", "361", "2300", 5e7, 5e7)];

    type Rede<'a> = &'a dyn Fn(&str, &[(&str, &str)], Option<&str>) -> Vec<u8>;

    /// Resposta boa -> levantamento pronto, com a saida gravada. A mesma rede com a resposta `corta` pela metade -> pendencia
    /// e nenhuma saida.
    fn confere(uf: &str, cidade: &str, rede: Rede, corta: &str) -> PathBuf {
        let d = tmp(&format!("ponta-{uf}"));
        let pac = pacote_falso(&d);
        let ibge = |_: &str, _: &str| Some(1234567);
        let boa = |u: &str, c: &[(&str, &str)], b: Option<&str>| Ok((200, rede(u, c, b)));
        let pasta = raiz(&d).join("900-teste");
        let c = levantar(&pasta, uf, cidade, "900", &Fora { ibge: &ibge, ..fora(&boa, &pac) }, 1.0e9).unwrap_or_else(|e| panic!("{uf}: {e}"));
        assert_eq!((c["anos"]["2024"]["gastou"].as_f64(), c["anos"]["2026"]["arrec"].as_f64(), c["festas"]["2025"]["gasto"].as_f64(), c["benef"]["2026"].as_f64()), (Some(5.5e7), Some(6e7), Some(3e6), Some(2e6)), "{uf}");
        assert!(c["fonte"] == "tribunal" && c["uf"] == uf, "{uf}");
        for arq in ["cidade.json", "setores.json", "receita-2026.json", "acoes-2025-12.json", "festas-cods.json"] {
            assert!(pasta.join("saida").join(arq).is_file(), "{uf}: {arq}");
        }
        let meia = |u: &str, c: &[(&str, &str)], b: Option<&str>| {
            let r = rede(u, c, b);
            Ok((200, if u.contains(corta) { r[..r.len() / 2].to_vec() } else { r }))
        };
        let outra = raiz(&d).join("901-teste");
        let e = levantar(&outra, uf, cidade, "901", &Fora { ibge: &ibge, ..fora(&meia, &pac) }, 1.0e9).unwrap_err();
        assert!(!outra.join("saida").exists(), "{uf}: {e}");
        d
    }

    fn latin(s: &str) -> Vec<u8> {
        s.chars().map(|c| c as u8).collect()
    }

    fn rede_es(u: &str, _: &[(&str, &str)], _: Option<&str>) -> Vec<u8> {
        if let Some(p) = u.strip_prefix(ES_CKAN) {
            let rec: Vec<Value> = if p == "obrigacoes" {
                vec![json!({ "name": "Prestação de Contas Municipal (PCM)", "url": "https://dados.es.gov.br/x/pcm.csv" })]
            } else {
                base::ANOS.iter().map(|a| json!({ "name": format!("Municipios-{}-{a}.zip", p.trim_end_matches("-municipios")), "url": format!("https://dados.es.gov.br/x/{p}-{a}.zip") })).collect()
            };
            return json!({ "result": { "organization": { "name": "tcees" }, "resources": rec } }).to_string().into_bytes();
        }
        if u.ends_with("pcm.csv") {
            return latin("Município;Ano;Mes;UnidadeGestora;DataHomologacao\nVitória;2026;1;Prefeitura;01/02/2026\nVitória;2026;2;Prefeitura;01/03/2026\n");
        }
        let t = if u.contains("/despesas-municipios-") {
            let mut t = "EsferaAdministrativa;Mes;CodigoUnidadeGestora;CodigoOrgao;CodigoUnidadeOrcamentaria;CodigoFuncao;CodigoSubFuncao;CodigoPrograma;CodigoAcao;NomeAcao;PrevisaoInicial;PrevisaoAtualizada;Empenhada;Liquidada".to_string();
            for x in ACOES {
                t += &format!("\nVitória;1;077E0700001;10;01;{};{};0001;{};{};{:.2};{:.2};{:.2};{:.2}", x.1, x.2, x.3, x.0, x.4, x.4, x.5, x.5).replace('.', ",");
            }
            t + "\nSerra;1;069E0700001;10;01;12;361;0003;2004;ESCOLAS;0;0;999,00;999,00\n"
        } else {
            assert!(u.contains("/receitas-municipios-"), "pedido inesperado: {u}");
            "EsferaAdministrativa;Mes;ClassificacaoReceita;Arrecadada\nVitória;1;1112;40.000.000,00\nVitória;1;1751;20.000.000,00\nSerra;1;1112;9,00\n".to_string()
        };
        zipa(&[("dados.csv", &latin(&t))])
    }

    #[test]
    fn es_levanta_pelo_zip_do_cidades() {
        let d = confere("ES", "Vitória", &rede_es, "despesas-municipios-2025.zip");
        // o zip conferido fica guardado; o que veio cortado nao
        assert!(raiz(&d).join("900-teste").join("tce-es").join("despesas2025.zip").is_file() && !raiz(&d).join("901-teste").join("tce-es").join("despesas2025.zip").exists());
        // catalogo que aponta para fora de orgao publico: pendencia, sem ir buscar
        let pac = pacote_falso(&d);
        let desvia = |u: &str, c: &[(&str, &str)], b: Option<&str>| {
            assert!(u.starts_with("https://dados.es.gov.br/"), "foi buscar em {u}");
            Ok((200, String::from_utf8(rede_es(u, c, b)).map(|t| t.replace("https://dados.es.gov.br/x/pcm.csv", "http://127.0.0.1:8765/pcm.csv")).unwrap().into_bytes()))
        };
        let e = levantar(&raiz(&d).join("902-teste"), "ES", "Vitória", "902", &Fora { ibge: &|_, _| Some(3205309), ..fora(&desvia, &pac) }, 1.0e9).unwrap_err();
        assert!(e.contains("não é de órgão público"), "{e}");
        let _ = std::fs::remove_dir_all(&d);
    }

    fn rede_pr(u: &str, _: &[(&str, &str)], _: Option<&str>) -> Vec<u8> {
        let nome = u.strip_prefix(PR_URL).and_then(|r| r.rsplit('/').next()).unwrap_or_else(|| panic!("pedido inesperado: {u}"));
        let a = &nome[..4];
        if nome == format!("{a}_410690_Receita.zip") {
            let item = |m: u32, id: &str, v: &str| format!("<ReceitasConsolidado nrMes=\"{m}\" idSumarioItem=\"{id}\" dsItem=\"ITEM {id}\" vlRealizadoAteOMes=\"{v}\"/>");
            let l: String = (1..=12).map(|m| item(m, "5010", "60000000") + &item(m, "5030", "40000000") + &item(m, "5220", "20000000")).collect();
            return zipa(&[("leia.txt", b"x"), (&format!("{a}_410690_ReceitasConsolidado.xml"), format!("<R>{l}</R>").as_bytes())]);
        }
        assert_eq!(nome, format!("{a}_410690_Despesa.zip"));
        let emp: String = ACOES.iter().enumerate().map(|(i, x)| format!("<Empenho idEmpenho=\"{i}\" nmEntidade=\"MUNICIPIO DE CURITIBA\" idPessoa=\"100\" cdOrgao=\"10\" cdUnidade=\"01\" cdFuncao=\"{}\" cdSubFuncao=\"{}\" cdPrograma=\"0005\" cdProjetoAtividade=\"{}\" dsProjetoAtividade=\"{}\" dtEmpenho=\"{a}-01-10\" vlEmpenho=\"{}\" ultimoEnvioSIMAMNesteExercicio=\"{a}/8\" />\n", x.1, x.2, x.3, x.0, x.5)).collect();
        let liq: String = ACOES.iter().enumerate().map(|(i, x)| format!("<EmpenhoLiquidacao idEmpenho=\"{i}\" dtLiquidacao=\"{a}-02-01\" vlLiquidacaoLiquido=\"{}\"/>", x.5)).collect();
        zipa(&[(&format!("{a}_410690_Empenho.xml"), format!("<SIM-AM>\n{emp}</SIM-AM>").as_bytes()), (&format!("xml/{a}_410690_EmpenhoLiquidacao_1.xml"), format!("<L>{liq}</L>").as_bytes()), (&format!("{a}_410690_EmpenhoPagamento.xml"), b"<P/>")])
    }

    #[test]
    fn pr_levanta_pelos_zips_do_pit() {
        let d = confere("PR", "Curitiba", &rede_pr, "2025_410690_Despesa.zip");
        // zip que veio inteiro mas sem o arquivo de receita: pendencia
        let pac = pacote_falso(&d);
        let sem = |u: &str, c: &[(&str, &str)], b: Option<&str>| Ok((200, if u.ends_with("Receita.zip") { zipa(&[("leia.txt", b"x")]) } else { rede_pr(u, c, b) }));
        let pasta = raiz(&d).join("902-teste");
        let e = levantar(&pasta, "PR", "Curitiba", "902", &fora(&sem, &pac), 1.0e9).unwrap_err();
        assert!(e.contains("não veio no zip") && !pasta.join("saida").exists(), "{e}");
        let _ = std::fs::remove_dir_all(&d);
    }

    fn linha(n: usize, campos: &[(usize, &str)]) -> String {
        let mut c = vec![String::new(); n];
        for (i, v) in campos {
            c[*i] = format!("\"{v}\"");
        }
        c.join(",")
    }

    fn rede_sp(u: &str, _: &[(&str, &str)], _: Option<&str>) -> Vec<u8> {
        let nome = u.strip_prefix(SP_URL).and_then(|r| r.rsplit('/').next()).unwrap_or_else(|| panic!("pedido inesperado: {u}"));
        if nome.starts_with("csvReceita") {
            let t = format!("cab\n{}\n{}\n", linha(19, &[(1, "1"), (4, "1 - IMPOSTOS"), (15, "01"), (18, "40000000")]), linha(19, &[(1, "1"), (4, "7 - TRANSFERENCIAS"), (15, "01"), (18, "20000000")]));
            return zipa(&[("leia.txt", b"x"), ("receita.csv", t.as_bytes())]);
        }
        assert!(nome.starts_with("Dotacao") && nome.ends_with(".csv"), "pedido inesperado: {u}");
        let mut t = "cab".to_string();
        for x in ACOES {
            let (orc, liq) = (x.4.to_string(), x.5.to_string());
            t += &format!("\n{}", linha(35, &[(3, "25"), (4, "Secretaria"), (5, "10"), (7, x.1), (9, x.2), (11, "3001"), (13, x.3), (14, x.0), (29, &orc), (30, &orc), (33, &liq), (34, &liq)]));
        }
        t.into_bytes()
    }

    #[test]
    fn sp_levanta_pelo_iris() {
        let d = confere("SP", "São Paulo", &rede_sp, "csvReceita2025.zip");
        // zip da receita sem nenhum CSV dentro
        let pac = pacote_falso(&d);
        let sem = |u: &str, c: &[(&str, &str)], b: Option<&str>| Ok((200, if u.ends_with(".zip") { zipa(&[("leia.txt", b"x")]) } else { rede_sp(u, c, b) }));
        let e = levantar(&raiz(&d).join("902-teste"), "SP", "São Paulo", "902", &fora(&sem, &pac), 1.0e9).unwrap_err();
        assert!(e.contains("zip sem o arquivo esperado"), "{e}");
        let _ = std::fs::remove_dir_all(&d);
    }

    fn tabela_ma(a: &str, met: &str) -> Vec<Value> {
        let k = |i: usize, mais: Value| {
            let x = ACOES[i];
            let mut o = json!({ "cnpjUnidadeGestora": "06", "codigoUnidadeOrcamentaria": "1301", "codigoFuncao": x.1, "codigoSubfuncao": x.2, "codigoPrograma": "0020", "codigoAcao": x.3, "fonteRecurso": "1500" });
            o.as_object_mut().unwrap().extend(mais.as_object().unwrap().clone());
            o
        };
        let todas = |f: &dyn Fn(usize) -> Value| (0..ACOES.len()).map(f).collect::<Vec<_>>();
        match met {
            "acao" => todas(&|i| json!({ "codigoPrograma": "0020", "codigoAcao": ACOES[i].3, "tituloAcao": ACOES[i].0 })),
            "unidadeorcamentaria" => vec![json!({ "codigoUnidadeOrcamentaria": "1301" })],
            "dotacao" => todas(&|i| k(i, json!({ "valor": ACOES[i].4 }))),
            "atualizacaoorcamentaria" => vec![k(0, json!({ "valor": 10.0, "tipoAlteracao": 13, "dataAto": format!("{a}-01-10") }))],
            "empenho" => todas(&|i| k(i, json!({ "numeroEmpenho": format!("000{i}/{a}"), "dataEmpenho": format!("{a}-01-10"), "tipoEmpenho": "O", "valor": ACOES[i].5, "codigoNaturezaDespesa": "339039" }))),
            "liquidacao" => todas(&|i| k(i, json!({ "numeroLiquidacao": format!("L{i}"), "numeroEmpenhoOriginal": format!("000{i}/{a}"), "dataLiquidacao": format!("{a}-02-01"), "tipoLiquidacao": "O", "valor": ACOES[i].5, "codNaturezaDespesaCompleto": "33903900" }))),
            "receitaorcamentaria" => [("11125001", 4e7), ("17115001", 2e7)].iter().map(|(c, v)| json!({ "cnpjUnidadeGestora": "06", "competencia": format!("01{a}"), "tipoLancamentoReceitaOrcamentaria": "O", "fonteRecurso": "1500", "codigoReceitaOrcamentaria": c, "valor": v })).collect(),
            x => panic!("método inesperado {x}"),
        }
    }

    /// A API do SINC de mentira (filtra pelo comeco do valor e pagina) e o Painel de Envio com so o 1o bimestre completo.
    fn rede_ma(u: &str, _: &[(&str, &str)], _: Option<&str>) -> Vec<u8> {
        if u.starts_with(MA_ENVIO.0) {
            let st = if u.contains("%22value%22%3A%5B1%5D") { "Remessa completa" } else { "Remessa incompleta" };
            return json!({ "data": { "rows": [["1-São Luís-MA", 0, 0, 0, 0, 0, st]] } }).to_string().into_bytes();
        }
        let (caminho, consulta) = u.strip_prefix(MA_API).and_then(|r| r.split_once('?')).unwrap_or_else(|| panic!("pedido inesperado: {u}"));
        let (a, met) = caminho.split_once('/').unwrap();
        let q: BTreeMap<&str, &str> = consulta.split('&').filter_map(|p| p.split_once('=')).collect();
        assert_eq!(q["enteId"], "2111300");
        let (size, page): (usize, usize) = (q["size"].parse().unwrap(), q["page"].parse().unwrap());
        let t: Vec<Value> = tabela_ma(a, met).into_iter().filter(|x| q.iter().all(|(k, v)| ["enteId", "size", "page"].contains(k) || crate::link_tribunais_util::pystr(&x[*k]).starts_with(v))).collect();
        let pg: Vec<Value> = t.iter().skip(page * size).take(size).cloned().collect();
        json!({ "content": pg, "numberOfElements": pg.len(), "totalElements": t.len() }).to_string().into_bytes()
    }

    #[test]
    fn ma_levanta_pela_api_do_sinc() {
        let d = confere("MA", "São Luís", &rede_ma, "2025/liquidacao");
        // so a fatia que veio inteira fica guardada: a pergunta de total (1 linha de 3) nao
        let guardados: Vec<String> = std::fs::read_dir(raiz(&d).join("900-teste").join("tce-ma")).unwrap().flatten().map(|x| x.file_name().to_string_lossy().to_string()).collect();
        assert!(guardados.iter().any(|n| n.starts_with("2025-empenho-")) && guardados.iter().filter(|n| n.starts_with("2025-empenho-")).count() == 1, "{guardados:?}");
        // com o que ficou guardado, a segunda rodada so volta ao tribunal para o que nao veio inteiro
        let pac = pacote_falso(&d);
        let pedidos = RefCell::new(0);
        let conta = |u: &str, c: &[(&str, &str)], b: Option<&str>| {
            *pedidos.borrow_mut() += 1;
            assert!(!u.contains("/liquidacao?"), "liquidacao ja estava guardada: {u}");
            Ok((200, rede_ma(u, c, b)))
        };
        levantar(&raiz(&d).join("900-teste"), "MA", "São Luís", "900", &fora(&conta, &pac), 1.0e9).unwrap();
        assert!(*pedidos.borrow() > 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Resposta de painel Power BI (formato DSR), cada linha com todos os valores.
    fn dsr(linhas: &[Value]) -> Vec<u8> {
        let n = linhas.first().and_then(Value::as_array).map_or(1, Vec::len);
        let s: Vec<Value> = (0..n).map(|i| json!({ "N": format!("G{i}") })).collect();
        let dm: Vec<Value> = linhas.iter().enumerate().map(|(i, l)| if i == 0 { json!({ "S": s, "C": l }) } else { json!({ "C": l }) }).collect();
        json!({ "results": [{ "result": { "data": { "dsr": { "DS": [{ "PH": [{ "DM0": dm }] }] } } } }] }).to_string().into_bytes()
    }

    /// (tabela, primeira coluna, quantas colunas) da consulta Power BI que veio no corpo do POST, e o corpo.
    fn consulta_pbi(corpo: Option<&str>) -> (String, String, usize, Value) {
        let v: Value = serde_json::from_str(corpo.expect("consulta ao painel e POST")).unwrap();
        let q = v["queries"][0]["Query"]["Commands"][0]["SemanticQueryDataShapeCommand"]["Query"].clone();
        (q["From"][0]["Entity"].as_str().unwrap().into(), q["Select"][0]["Column"]["Property"].as_str().unwrap_or("").into(), q["Select"].as_array().unwrap().len(), v)
    }

    const GO_NOMES: [(&str, &str, &str); 3] = [("Secretaria de Festas", "Cultura", "Difusão Cultural"), ("Fundo de Beneficios Eventuais", "Assistência social", "Assistência Comunitária"), ("Secretaria de Ensino", "Educação", "Ensino Fundamental")];

    fn rede_go(u: &str, cab: &[(&str, &str)], corpo: Option<&str>) -> Vec<u8> {
        if u.starts_with(GO_PORTAL) {
            assert!(u.contains("municipio=Goi%C3%A2nia") && cab.is_empty(), "{u}");
            return br#"<script>powerbi.embed(x, { accessToken: "tok-1.x", embedUrl: "https://app.powerbi.com/reportEmbed?reportId=abc-123&groupId=9" })</script>"#.to_vec();
        }
        assert!(u.starts_with(GO_CLUSTER) && cab.contains(&("Authorization", "Bearer tok-1.x")), "{u}");
        if u.contains("/explore/reports/abc-123/modelsAndExploration") {
            return json!({ "models": [{ "id": 5, "dbName": "db" }] }).to_string().into_bytes();
        }
        assert!(u.ends_with("/explore/querydata?synchronous=true"), "pedido inesperado: {u}");
        let (tabela, _, n, v) = consulta_pbi(corpo);
        assert!(v["modelId"] == 5 && v["queries"][0]["ApplicationContext"]["DatasetId"] == "db" && v["queries"][0]["ApplicationContext"]["Sources"][0]["ReportId"] == "abc-123");
        let por_acao = |f: &dyn Fn(usize) -> Value| (0..ACOES.len()).map(f).collect::<Vec<_>>();
        dsr(&match (tabela.as_str(), n) {
            ("chat_despesa", 3) => (1..=8).map(|m| json!(["Prefeitura", m, 100])).collect(),
            ("chat receita", 2) => (1..=8).map(|m| json!([m, 100])).collect(),
            ("chat_despesa", 7) => por_acao(&|i| json!(["Executivo", "Prefeitura", GO_NOMES[i].0, GO_NOMES[i].1, GO_NOMES[i].2, ACOES[i].5, ACOES[i].5])),
            ("chat receita", 3) => vec![json!(["Receitas Correntes", "1112", 4e7]), json!(["Receitas Correntes", "1751", 2e7])],
            ("chat orcamento", 7) => por_acao(&|i| json!(["Prefeitura", GO_NOMES[i].0, GO_NOMES[i].1, GO_NOMES[i].2, "Programa", "Ação", ACOES[i].4])),
            x => panic!("consulta inesperada {x:?}"),
        })
    }

    #[test]
    fn go_levanta_com_o_token_da_pagina_publica() {
        let d = confere("GO", "Goiânia", &rede_go, "querydata");
        // pagina sem o token: pendencia dita, sem consultar o painel
        let pac = pacote_falso(&d);
        let sem = |u: &str, _: &[(&str, &str)], _: Option<&str>| {
            assert!(u.starts_with(GO_PORTAL), "foi consultar {u}");
            Ok((200, b"<html>em manutencao</html>".to_vec()))
        };
        let e = levantar(&raiz(&d).join("902-teste"), "GO", "Goiânia", "902", &fora(&sem, &pac), 1.0e9).unwrap_err();
        assert!(e.contains("não trouxe o token") && !e.contains("tok-"), "{e}");
        let _ = std::fs::remove_dir_all(&d);
    }

    fn rede_rj(u: &str, cab: &[(&str, &str)], corpo: Option<&str>) -> Vec<u8> {
        assert!(u.starts_with(RJ_API) && cab.contains(&("X-PowerBI-ResourceKey", RJ_CHAVE)), "{u}");
        if u.contains("modelsAndExploration") {
            return json!({ "models": [{ "id": 7 }] }).to_string().into_bytes();
        }
        let (tabela, coluna, n, v) = consulta_pbi(corpo);
        assert_eq!(v["modelId"], 7);
        dsr(&match (tabela.as_str(), coluna.as_str(), n) {
            ("Despesas_Mensal", _, 3) => (1..=8).map(|m| json!(["Prefeitura", m, 100])).collect(),
            ("Receitas", "Mês", _) => base::ABR[..8].iter().map(|m| json!([m, 100])).collect(),
            ("Origem Receita", ..) => vec![json!(["11", "Impostos"]), json!(["17", "Transferências correntes"])],
            ("Despesas_Mensal", _, 6) => ACOES.iter().flat_map(|x| [("Dotação Inicial", x.4), ("Despesas Empenhadas", x.5), ("Despesas Liquidadas", x.5)].map(|(t, v)| json!(["Prefeitura", format!("{} - Função", x.1), format!("{} - Subfunção", x.2), format!("{:0>20}{} - {}", "1", x.3, x.0), t, v]))).collect(),
            ("Receitas", "ORIGEM", _) => vec![json!(["11", 4e7]), json!(["17", 2e7])],
            x => panic!("consulta inesperada {x:?}"),
        })
    }

    #[test]
    fn rj_levanta_pelo_radar_carioca() {
        let _ = std::fs::remove_dir_all(confere("RJ", "Rio de Janeiro", &rede_rj, "querydata"));
    }

    fn rede_rn(u: &str, cab: &[(&str, &str)], corpo: Option<&str>) -> Vec<u8> {
        assert!(u == RN_API && cab.contains(&("X-PowerBI-ResourceKey", RN_CHAVE)) && cab.contains(&("Content-Type", "application/json")), "{u}");
        dsr(&match consulta_pbi(corpo).0.as_str() {
            "Dim_Orgao" => vec![json!([77, "Prefeitura Municipal de Natal"]), json!([78, "Câmara Municipal de Natal"])],
            "Dim_ArquivoLRF" => vec![json!([2024, 6, 246]), json!([2025, 6, 256]), json!([2026, 3, 263])],
            "Fato_Despesa" => ACOES.iter().map(|x| json!([x.1, x.2, x.0, x.4, x.4, x.5, x.5])).collect(),
            "Fato_Receita" => vec![json!(["1112", 4e7]), json!(["1751", 2e7])],
            "Fato_Receita_Deducoes" => vec![json!([263, 0])],
            x => panic!("consulta inesperada {x}"),
        })
    }

    #[test]
    fn rn_levanta_pelo_painel_fiscal() {
        let d = confere("RN", "Natal", &rede_rn, "querydata");
        // RELEITURA: o mesmo molde le o tribunal de novo numa pasta limpa e tem de dar o mesmo numero
        let (pac, pasta) = (d.join("pacote"), raiz(&d).join("900-teste"));
        let boa = |u: &str, c: &[(&str, &str)], b: Option<&str>| Ok((200, rede_rn(u, c, b)));
        let f = fora(&boa, &pac);
        let arq = pasta.join("saida").join("cidade.json");
        let mut cid = ler_json(&arq).unwrap();
        let certa = cid["tribunal"]["sigla"].as_str().unwrap().to_string();
        let do_guia = |_: &str| Ok(certa.clone());
        assert_eq!(reler(&pasta, "900", &f, &do_guia, 1.0e9).unwrap(), Vec::<String>::new());
        assert!(!pasta.join("releitura-tmp").exists());
        // o guia manda outro tribunal
        let p = reler(&pasta, "900", &f, &|_| Ok("TCM-XX".into()), 1.0e9).unwrap();
        assert!(p.len() == 1 && p[0].contains("tribunal fora do guia"), "{p:?}");
        // tribunal fora do ar ou guia que falta: nao deu para conferir (nao prova nada)
        assert!(reler(&pasta, "900", &fora(&|u, _, _| Err(format!("{u}: tempo esgotado")), &pac), &do_guia, 1.0e9).unwrap_err().contains("tempo esgotado"));
        assert!(reler(&pasta, "900", &f, &|uf| Err(format!("falta o guia do estado ({uf})")), 1.0e9).unwrap_err().contains("falta o guia"));
        // o numero guardado no PC nao e o que o tribunal da agora
        cid["anos"]["2026"]["arrec"] = json!(cid["anos"]["2026"]["arrec"].as_f64().unwrap() + 1.0);
        std::fs::write(&arq, cid.to_string()).unwrap();
        let p = reler(&pasta, "900", &f, &do_guia, 1.0e9).unwrap();
        assert!(p.len() == 1 && p[0].starts_with("2026 arrec: no vídeo 60,000,001.00, na fonte agora 60,000,000.00"), "{p:?}");
        let _ = std::fs::remove_dir_all(&d);
    }

    const COD: &str = "2.2120.13.392.10.2.2117";
    const ROTEIRO: &str = "Em 2026 a prefeitura arrecadou 10 milhões e 460. Festas, 718 mil. Gastar não é crime.";
    const FONTES: &str = "| Valor dito | Valor exato | Arquivo | Página / linha |\n|---|---|---|---|\n| Gastou (2026) | R$ 8.718.313,42 | SIM | soma do liquidado |\n| Festas gastou (2026) | R$ 718.313,42 | SIM | ação FESTA (2.2120.13.392.10.2.2117.0) · liquidado |\n\n## Setores 2026\n- Festas e eventos: R$ 718.313,42 (2.2120.13.392.10.2.2117.0, p. 0)\n";

    /// Um levantamento pronto (formato do gerador) com o documento do tribunal em PDF; `falta` tira um valor do documento.
    fn levantamento(d: &Path, falta: bool) -> PathBuf {
        let pasta = raiz(d).join("777-cidadeteste");
        let saida = pasta.join("saida");
        std::fs::create_dir_all(&saida).unwrap();
        std::fs::create_dir_all(pasta.join("documentos_tcm")).unwrap();
        let acoes = json!([{ "cod": "1.1", "previsto": 9000000.0, "empenhado": 8500000.0, "liquidado": 8000000.0 },
            { "cod": COD, "previsto": 980000.0, "empenhado": 800000.0, "liquidado": 718313.42 }]);
        let cid = json!({ "numero": "777", "nome": "Cidade Teste", "uf": "CE", "codigo": "orcamentocidadetestece", "comp": "OrcamentoCidadetesteCe", "kit_pasta": "orcamento-cidadeteste-ce", "arquivo_mapa": "cidadeteste.mp4",
            "meses": { "2026": "09" }, "anos": { "2026": { "arrec": 10463552.21, "gastou": 8718313.42, "emp": 9300000.0 } },
            "festas": { "2026": { "prev": 980000.0, "gasto": 718313.42 } }, "festas_fonte": { "2026": [{ "acao": COD }] }, "benef": { "2026": 718313.42 } });
        let setores = json!({ "2026-09": { "Educação": { "liquidado": 8000000.0, "acoes": [["Ensino", 8000000.0, "1.1", 3]] }, "Festas e eventos": { "liquidado": 718313.42, "acoes": [["Festas", 718313.42, COD, 24]] } } });
        for (nome, v) in [("cidade.json", &cid), ("acoes-2026-09.json", &acoes), ("receita-2026.json", &json!([["Impostos", 1463552.21, 1], ["Transferências Correntes", 9000000.0, 5]])), ("setores.json", &setores)] {
            std::fs::write(saida.join(nome), v.to_string()).unwrap();
        }
        std::fs::write(pasta.join("estado.json"), json!({ "numero": "777", "estado": "pronto" }).to_string()).unwrap();
        let doc = format!("9.000.000,00 8.500.000,00 8.000.000,00 980.000,00 800.000,00 {} 1.463.552,21", if falta { "" } else { "718.313,42" });
        std::fs::write(pasta.join("documentos_tcm").join("2026-09_BALANCETE.pdf"), doc).unwrap();
        // .txt sem PDF nao e prova, com ou sem ano no nome
        std::fs::write(pasta.join("documentos_tcm").join("2026-09_SOLTO.txt"), "718.313,42").unwrap();
        std::fs::write(pasta.join("documentos_tcm").join("anotacao.txt"), "718.313,42").unwrap();
        pasta
    }

    /// Faz o papel do pdftotext: o "PDF" de teste ja e o texto.
    fn pdf_falso(pdf: &Path, txt: &Path) -> Result<(), String> {
        std::fs::copy(pdf, txt).map(|_| ()).map_err(|e| e.to_string())
    }

    /// Faz o papel dos geradores do pacote: grava na SOMBRA o que eles gravam (roteiro, fontes e preparo; depois a legenda).
    fn gerador_falso(rot: &'static str) -> impl Fn(&Path, &Path, &Path) -> Result<(i32, String), String> {
        move |script, sombra, pasta| {
            let cid = ler_json(&pasta.join("saida").join("cidade.json")).unwrap();
            let (n, nome) = (cid["numero"].as_str().unwrap(), cid["nome"].as_str().unwrap());
            let (projeto, estudio) = (format!("{n}. orçamento de {}", nome.to_lowercase()), sombra.join("estudio-3d"));
            if script.ends_with("pacote/scripts-braniac/gera_cidade.py") {
                // a sombra chega com o que o gerador le: os moldes, a pasta dos videos e o atalho para o fontes.py do pacote
                assert!(sombra.ends_with("777-cidadeteste/roteiro") && junta(sombra, &MOLDE_TSX).is_file() && junta(sombra, &MOLDE_KIT).join("dados").join("salvador.json").is_file() && sombra.join("videos").is_dir());
                let atalho = std::fs::read_to_string(sombra.join("scripts-braniac").join("fontes.py")).unwrap();
                assert!(atalho.contains("runpy.run_path(") && atalho.contains("fontes.py") && atalho.contains("pacote"), "{atalho}");
                let audio = estudio.join("projetos").join(&projeto).join("audio");
                std::fs::create_dir_all(&audio).unwrap();
                std::fs::write(audio.join("roteiro-falado.txt"), rot).unwrap();
                std::fs::write(audio.join("fontes.md"), FONTES).unwrap();
                std::fs::create_dir_all(sombra.join("videos").join(n)).unwrap();
                std::fs::write(sombra.join("videos").join(n).join("preparo.json"), json!({ "projeto": projeto, "nome": nome.to_uppercase(), "codigo": cid["codigo"], "comp": cid["comp"] }).to_string()).unwrap();
            } else {
                assert!(script.ends_with("pacote/scripts-braniac/gera_legenda.py"));
                let ed = estudio.join("edicoes").join(&projeto);
                std::fs::create_dir_all(&ed).unwrap();
                std::fs::write(ed.join(format!("{} - legenda.txt", nome.to_uppercase())), "Festas: R$ 718 mil. \u{2696}\u{fe0f} AVISO: art. 5 da lei 12.527").unwrap();
            }
            Ok((0, String::new()))
        }
    }

    fn nunca_gera(s: &Path, _: &Path, _: &Path) -> Result<(i32, String), String> {
        panic!("nao era para rodar gerador nenhum: {}", s.display())
    }

    #[test]
    fn roteiro_roda_os_geradores_do_pacote_na_sombra() {
        let d = tmp("roteiro");
        let pac = pacote_falso(&d);
        assert!(roteiro(&d, "777", &pac, &nunca_gera, "q").unwrap_err().contains("não há levantamento"));
        let pasta = levantamento(&d, false);
        let sombra = pasta.join(SOMBRA);
        // levantamento que nao esta pronto: nem prepara nada
        std::fs::write(pasta.join("estado.json"), json!({ "estado": "pendencia", "motivo": "tribunal fora do ar" }).to_string()).unwrap();
        let e = roteiro(&d, "777", &pac, &nunca_gera, "q").unwrap_err();
        assert!(e.contains("só sai de levantamento pronto") && e.contains("tribunal fora do ar") && !sombra.exists() && !pasta.join("roteiro.json").exists(), "{e}");
        levantamento(&d, false);
        // ok: roteiro e legenda ficam dentro da pasta do levantamento; a conferencia antiga cai
        std::fs::write(pasta.join("auditoria.json"), "{\"ok\":true}").unwrap();
        let r = roteiro(&d, "777", &pac, &gerador_falso(ROTEIRO), "2026-11-18 10:00").unwrap();
        assert!(r["estado"] == "pronto" && r["palavras"] == 16 && r["legenda"] == true && r["quando"] == "2026-11-18 10:00", "{r}");
        let e = estado_de(&d, "777").unwrap();
        assert!(e["roteiro"]["estado"] == "pronto" && e["auditoria"].is_null() && sombra.join("videos").join("777").join("preparo.json").is_file());
        // nada foi gravado no pacote
        assert!(!pac.join("videos").exists() && !pac.join("estudio-3d").join("projetos").exists() && !pac.join("estudio-3d").join("edicoes").exists());
        // erro do script: pendencia com o fim da mensagem, e nenhum roteiro para tras (nem o da rodada boa)
        let e = roteiro(&d, "777", &pac, &|_, _, _| Ok((1, format!("{}\nAssertionError: palavra proibida no roteiro: %\n", "x".repeat(900)))), "q").unwrap_err();
        assert!(e.contains("gera_cidade.py do pacote) falhou (código 1)") && e.ends_with("palavra proibida no roteiro: %") && e.len() < 420, "{e}");
        let e = estado_de(&d, "777").unwrap();
        assert!(!sombra.exists() && e["roteiro"]["estado"] == "pendencia" && e["roteiro"]["motivo"].as_str().unwrap().contains("palavra proibida"));
        let p = porta(&d, "777").unwrap();
        assert!(p["liberado"] == false && p["problemas"][0].as_str().unwrap().starts_with("o roteiro ficou com pendência"), "{p}");
        // tempo esgotado, python que nao existe
        assert!(roteiro(&d, "777", &pac, &|_, _, _| Err("tempo esgotado (300 s)".into()), "q").unwrap_err().contains("gera_cidade.py do pacote): tempo esgotado"));
        // terminou "bem" sem gravar nada
        assert!(roteiro(&d, "777", &pac, &|_, _, _| Ok((0, String::new())), "q").unwrap_err().contains("sem gravar o preparo.json"));
        // a legenda que nao gera tambem e pendencia (legenda velha nunca vai junto)
        let bom = gerador_falso(ROTEIRO);
        let so_roteiro = |s: &Path, a: &Path, b: &Path| if s.ends_with("gera_legenda.py") { Ok((1, "KeyError: 'achado'".to_string())) } else { bom(s, a, b) };
        let e = roteiro(&d, "777", &pac, &so_roteiro, "q").unwrap_err();
        assert!(e.contains("a legenda (gera_legenda.py do pacote) falhou") && e.ends_with("KeyError: 'achado'") && !sombra.exists(), "{e}");
        // nome que viraria caminho para fora da sombra: para antes de rodar
        let arq = pasta.join("saida").join("cidade.json");
        let mut cid = ler_json(&arq).unwrap();
        cid["kit_pasta"] = json!("../../fora");
        std::fs::write(&arq, cid.to_string()).unwrap();
        assert!(roteiro(&d, "777", &pac, &nunca_gera, "q").unwrap_err().contains("'kit_pasta' que não serve de nome de pasta"));
        levantamento(&d, false);
        // pacote sem o molde, sem o gerador e sem a legenda
        std::fs::remove_file(junta(&pac, &MOLDE_TSX)).unwrap();
        let e = roteiro(&d, "777", &pac, &nunca_gera, "q").unwrap_err();
        assert!(e.starts_with("falta no pacote de produção: ") && e.ends_with("OrcamentoSalvadorTcm.tsx"), "{e}");
        let pac = pacote_falso(&d);
        std::fs::remove_file(pac.join("scripts-braniac").join("gera_legenda.py")).unwrap();
        let r = roteiro(&d, "777", &pac, &gerador_falso(ROTEIRO), "q").unwrap();
        assert!(r["legenda"] == false && r["notas"][0].as_str().unwrap().contains("sem legenda"), "{r}");
        std::fs::remove_file(pac.join("scripts-braniac").join("gera_cidade.py")).unwrap();
        assert!(roteiro(&d, "777", &pac, &nunca_gera, "q").unwrap_err().contains("gera_cidade.py (a pasta do pacote vem"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn lancador_so_troca_a_linha_do_pacote() {
        // sem aspas duplas (o argumento vai inteiro ao processo) e parando se a linha do pacote nao for unica
        assert!(!LANCADOR.contains('"') && !LANCADOR.contains('\n') && LANCADOR.contains(r"^PAC = Path\(") && LANCADOR.contains("n==1 or sys.exit("));
        assert!(nome_de_pasta("Olho d'Água") && nome_de_pasta("fortaleza.mp4") && !nome_de_pasta("a/b") && !nome_de_pasta("..") && !nome_de_pasta("C:x") && !nome_de_pasta(""));
    }

    #[test]
    fn auditoria_e_porta_barram_numero_sem_prova() {
        let d = tmp("auditoria");
        let pac = pacote_falso(&d);
        assert!(conferir(&d, "777", &pdf_falso).unwrap_err().contains("não há levantamento"));
        assert!(triagem(&d, "777").is_err() && porta(&d, "777").is_err());
        let pasta = levantamento(&d, false);
        assert!(triagem(&d, "777").unwrap_err().contains("conferência primeiro"));
        // a porta barra cada passo que falta
        assert_eq!(porta(&d, "777").unwrap()["problemas"], json!(["o roteiro ainda não foi gerado"]));
        roteiro(&d, "777", &pac, &gerador_falso(ROTEIRO), "q").unwrap();
        assert_eq!(porta(&d, "777").unwrap()["problemas"], json!(["a conferência (auditoria) ainda não foi feita"]));
        // tudo com prova: o PDF lido agora, o roteiro e a legenda gerados aqui
        let r = conferir(&d, "777", &pdf_falso).unwrap();
        assert!(r["ok"] == true && r["documentos"] == 1 && r["precisa_reler"] == false && r["notas"].as_array().unwrap().iter().all(|n| !n.as_str().unwrap().contains("roteiro") && !n.as_str().unwrap().contains("legenda")), "{r}");
        assert!(pasta.join("documentos_tcm").join("2026-09_BALANCETE.txt").is_file());
        assert_eq!(triagem(&d, "777").unwrap()["categoria"], Value::Null);
        let p = porta(&d, "777").unwrap();
        assert!(p["liberado"] == true && p["situacao"] == "liberado" && p["aviso"].as_str().unwrap().contains("ainda é no Braniac atual"), "{p}");
        // sem pdftotext nao audita; pdftotext que falha num PDF tambem nao (e o texto antigo nao fica valendo)
        assert_eq!(conferir(&d, "777", &|_, _| Err(SEM_PDFTOTEXT.into())).unwrap_err(), SEM_PDFTOTEXT);
        assert!(SEM_PDFTOTEXT.starts_with("não ligado: falta o pdftotext") && !pasta.join("documentos_tcm").join("2026-09_BALANCETE.txt").exists());
        assert!(conferir(&d, "777", &|_, _| Err("o pdftotext falhou (código 1): Syntax Error".into())).unwrap_err().starts_with("2026-09_BALANCETE.pdf: o pdftotext falhou"));
        assert!(conferir(&d, "777", &|_, _| Ok(())).unwrap_err().contains("sem gravar o texto"));
        // roteiro com numero inventado: a conferencia acusa e a porta barra
        roteiro(&d, "777", &pac, &gerador_falso("Festas, 918 mil. Gastar não é crime."), "q").unwrap();
        let r = conferir(&d, "777", &pdf_falso).unwrap();
        assert!(r["ok"] == false && r["problemas"][0].as_str().unwrap().contains("roteiro fala número sem prova"), "{r}");
        let p = porta(&d, "777").unwrap();
        assert!(p["liberado"] == false && p["situacao"] == "barrado" && p["problemas"][0].as_str().unwrap().contains("918000"), "{p}");
        // o valor das festas some do PDF do tribunal (continua no .txt solto, que nao e prova): barra
        levantamento(&d, true);
        roteiro(&d, "777", &pac, &gerador_falso(ROTEIRO), "q").unwrap();
        let r = conferir(&d, "777", &pdf_falso).unwrap();
        assert!(r["ok"] == false && r["problemas"][0].as_str().unwrap().contains("não estão no documento do tribunal"), "{r}");
        assert!(estado_de(&d, "777").unwrap()["auditoria"]["ok"] == false && porta(&d, "777").unwrap()["liberado"] == false);
        // com roteiro feito, dizer QUEM esta errado pede reler o tribunal, que a triagem ainda nao faz por aqui: vai para pessoa
        let t = triagem(&d, "777").unwrap();
        assert!(t["categoria"] == "pessoa" && t["explicacao"].as_str().unwrap().contains("a triagem ainda não relê o tribunal"), "{t}");
        // sem roteiro (so o levantamento): numero sem prova e para refazer
        std::fs::remove_file(pasta.join("roteiro.json")).unwrap();
        apaga_pasta(&pasta.join(SOMBRA));
        conferir(&d, "777", &pdf_falso).unwrap();
        let t = triagem(&d, "777").unwrap();
        assert!(t["categoria"] == "refazer" && t["explicacao"].as_str().unwrap().contains("sem prova"), "{t}");
        // sem PDF nao audita (so .txt solto na pasta); levantamento que nao terminou tambem nao
        std::fs::remove_file(pasta.join("documentos_tcm").join("2026-09_BALANCETE.pdf")).unwrap();
        assert!(conferir(&d, "777", &pdf_falso).unwrap_err().starts_with("faltam os PDFs do tribunal"));
        std::fs::remove_dir_all(pasta.join("saida")).unwrap();
        std::fs::write(pasta.join("estado.json"), json!({ "estado": "pendencia", "motivo": "tribunal fora do ar" }).to_string()).unwrap();
        assert!(conferir(&d, "777", &pdf_falso).unwrap_err().contains("tribunal fora do ar"));
        let e = estado_de(&d, "777").unwrap();
        assert!(e["estado"] == "pendencia" && e["tem_levantamento"] == false);
        assert!(porta(&d, "777").unwrap()["problemas"][0].as_str().unwrap().contains("o levantamento não está pronto (tribunal fora do ar)"));
        // "fazendo" sem thread viva = reiniciou no meio
        std::fs::write(pasta.join("estado.json"), json!({ "estado": "fazendo" }).to_string()).unwrap();
        assert!(estado_de(&d, "777").unwrap()["motivo"].as_str().unwrap().contains("reiniciou"));
        assert!(estado_de(&d, "../7").is_err() && estado_de(&d, "778").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Levantamento de dados abertos (sem PDF): a conta e conferida, mas a porta so abre com a releitura do tribunal sem problema.
    #[test]
    fn porta_de_dados_abertos_espera_a_releitura_do_tribunal() {
        let d = tmp("porta-aberta");
        let pac = pacote_falso(&d);
        let pasta = levantamento(&d, false);
        std::fs::remove_dir_all(pasta.join("documentos_tcm")).unwrap();
        let arq = pasta.join("saida").join("cidade.json");
        let mut cid = ler_json(&arq).unwrap();
        cid["fonte"] = json!("tribunal");
        std::fs::write(&arq, cid.to_string()).unwrap();
        roteiro(&d, "777", &pac, &gerador_falso(ROTEIRO), "q").unwrap();
        let r = conferir(&d, "777", &|p, _| panic!("sem PDF nao roda pdftotext: {}", p.display())).unwrap();
        assert!(r["ok"] == true && r["documentos"] == 0 && r["precisa_reler"] == true, "{r}");
        let barrado = |diz: &str| {
            let p = porta(&d, "777").unwrap();
            assert!(p["liberado"] == false && p["problemas"].as_array().unwrap().len() == 1 && p["problemas"][0].as_str().unwrap().contains(diz), "{p}");
        };
        barrado("falta reler o tribunal");
        let grava = |v: Value| std::fs::write(pasta.join("releitura.json"), v.to_string()).unwrap();
        grava(json!({ "estado": "fazendo" }));
        barrado("reiniciou no meio da releitura");
        grava(json!({ "estado": "pendencia", "motivo": "falta o guia do estado" }));
        barrado("não fechou: falta o guia do estado");
        grava(json!({ "estado": "pronto", "problemas": ["2026 gastou: no vídeo 8,718,313.42, na fonte agora 8,800,000.00"] }));
        barrado("na fonte agora");
        grava(json!({ "estado": "pronto", "problemas": [] }));
        assert_eq!(porta(&d, "777").unwrap()["liberado"], true);
        assert_eq!(estado_de(&d, "777").unwrap()["releitura"]["estado"], "pronto");
        // roteiro novo derruba a conferencia e a releitura: a porta fecha de novo
        roteiro(&d, "777", &pac, &gerador_falso(ROTEIRO), "q").unwrap();
        assert!(!pasta.join("releitura.json").exists() && porta(&d, "777").unwrap()["liberado"] == false);
        // a releitura so comeca com levantamento e roteiro prontos
        std::fs::remove_file(pasta.join("roteiro.json")).unwrap();
        assert!(comecar_releitura(&d, "777").unwrap_err().contains("depois do levantamento e do roteiro"));
        // guia do estado que falta: dito pelo caminho
        assert!(sigla_do_guia(&d, "CE").unwrap_err().starts_with("falta o guia do estado") && sigla_do_guia(&d, "../x").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn documentos_do_ceara_saem_do_cache_do_levantamento() {
        let d = tmp("documentos");
        let pac = pacote_falso(&d);
        let pasta = levantamento(&d, false);
        let nunca = |p: &Path, _: &str, _: &[(String, Vec<String>)]| -> Result<(), String> { panic!("nao era para desenhar {}", p.display()) };
        // sem roteiro nao ha fontes.md de onde partir
        assert!(documentos(&d, "777", &nunca).unwrap_err().starts_with("gere o roteiro primeiro"));
        roteiro(&d, "777", &pac, &gerador_falso(ROTEIRO), "q").unwrap();
        let site = pasta.join("documentos-capital");
        // cache ausente: diz o arquivo e nao deixa pasta
        assert!(documentos(&d, "777", &nunca).unwrap_err().contains("falta o arquivo de trabalho tce-ce/mes26.txt") && !site.exists());
        let src = pasta.join("tce-ce");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("mes26.txt"), "09\n").unwrap();
        let desp = |org: &str| json!({ "codigo_municipio": "999", "codigo_orgao": org, "codigo_unidade_orcamentaria": "2120 ", "codigo_funcao": "13", "codigo_subfuncao": "392", "codigo_programa": "10", "codigo_projeto_atividade": "2", "numero_projeto_atividade": "2117", "numero_subprojeto_atividade": "0", "valor_liquidado_ate_mes": 718313.42 });
        for (a, m) in [("2024", "12"), ("2025", "12"), ("2026", "09")] {
            std::fs::write(src.join(format!("despesa{a}{m}.json")), json!([desp("2"), desp("3")]).to_string()).unwrap();
            std::fs::write(src.join(format!("receita{a}{m}.json")), json!([{ "codigo_rubrica": "1112", "valor_arrecadacao_ate_mes": 10.0 }]).to_string()).unwrap();
            std::fs::write(src.join(format!("orcamento{a}.json")), json!([{ "nome_projeto_atividade": "FESTA" }]).to_string()).unwrap();
        }
        std::fs::remove_file(src.join("receita202512.json")).unwrap();
        assert!(documentos(&d, "777", &nunca).unwrap_err().contains("falta o arquivo de trabalho tce-ce/receita202512.json") && !site.exists());
        std::fs::write(src.join("receita202512.json"), json!([{ "codigo_rubrica": "1112", "valor_arrecadacao_ate_mes": 10.0 }]).to_string()).unwrap();
        // cache presente: as tabelas do tribunal, o extrato em PDF e o fontes.md com arquivo e linha, tudo dentro do levantamento
        let r = documentos(&d, "777", &pdf_extrato).unwrap();
        let p = site.join("docs").join(format!("777-{}-docs", docs::slug("Cidade Teste")));
        assert!(r["aviso"] == "Nada foi enviado." && r["uf"] == "CE" && r["relatorio"]["arquivos"].as_array().unwrap().len() == 9 && Path::new(r["pasta"].as_str().unwrap()) == p, "{r}");
        let t = docs::ler_linhas(&p.join("2026-09_CE-SIM-BALANCETE-DESPESA.txt"), None).unwrap();
        assert_eq!((t[0][0].as_str(), t[1][0].as_str(), t[2][0].as_str(), t.len()), ("codigo_acao_video", "2.2120.13.392.10.2.2117.0", "3.2120.13.392.10.2.2117.0", 3));
        let md = std::fs::read_to_string(p.join("fontes.md")).unwrap();
        assert!(md.contains("2026-09_CE-SIM-BALANCETE-DESPESA.txt linhas 2 |") && md.contains("(2.2120.13.392.10.2.2117.0, 2026-09_CE-SIM-BALANCETE-DESPESA.txt linhas 2)"), "{md}");
        let pdf = std::fs::read(p.join("2026-09_CE-EXTRATO-TRIBUNAL-SN.pdf")).unwrap();
        assert!(pdf.starts_with(b"%PDF-1.4") && pdf.ends_with(b"%%EOF\n"));
        assert!(std::fs::read_to_string(p.join("2026-09_CE-SIM-BALANCETE-RECEITA.txt")).unwrap().contains("1112") && estado_de(&d, "777").unwrap()["documentos"]["uf"] == "CE");
        // a origem cita o municipio que esta no proprio arquivo do tribunal
        assert!(String::from_utf8_lossy(&pdf).contains("codigo_municipio=999"));
        // estados cujo molde nao guarda o que o adaptador le: diz o que falta
        let arq = pasta.join("saida").join("cidade.json");
        let mut cid = ler_json(&arq).unwrap();
        for (uf, diz) in [("RJ", "não guarda nenhuma resposta do painel"), ("ES", "guarda os zips do CidadES"), ("PR", "zips do PIT fechados"), ("TO", "só aceita o número e a pasta do vídeo do sistema atual"), ("SP", "não existe adaptador")] {
            cid["uf"] = json!(uf);
            std::fs::write(&arq, cid.to_string()).unwrap();
            let e = documentos(&d, "777", &nunca).unwrap_err();
            assert!(e.starts_with(&format!("documentos não ligados para {uf}: ")) && e.contains(diz), "{e}");
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn pdf_do_extrato_tem_as_paginas_da_conta() {
        let d = tmp("pdf");
        let secoes = vec![("Sobre (este) extrato".to_string(), vec!["a\\b".to_string(), "x".repeat(400)]), ("2026: números".to_string(), (0..150).map(|i| format!("linha {i} ação")).collect())];
        pdf_extrato(&d.join("e.pdf"), "777. orçamento", &secoes).unwrap();
        let b = std::fs::read(d.join("e.pdf")).unwrap();
        let t: String = b.iter().map(|c| *c as char).collect();
        // a segunda secao comeca na pagina 1 e o arquivo tem as 3 paginas que a conta do fontes.md preve
        assert_eq!(docs::paginas(&secoes, 186), [("Sobre (este) extrato".to_string(), 1), ("2026: números".to_string(), 1)]);
        assert_eq!((t.matches("/Type /Page ").count(), t.contains("/Count 3 "), t.contains("(p. 3) Tj")), (3, true, true));
        assert!(t.contains("(Sobre \\(este\\) extrato) Tj") && t.contains("(a\\\\b) Tj") && t.contains("(linha 149 a\u{e7}\u{e3}o) Tj") && t.contains(&format!("({}) Tj", "x".repeat(186))) && t.contains(&format!("({}) Tj", "x".repeat(28))));
        // a tabela de enderecos aponta para o comeco de cada objeto
        let xref: usize = t.rsplit("startxref\n").next().unwrap().lines().next().unwrap().parse().unwrap();
        let cauda = std::str::from_utf8(&b[xref..]).unwrap();
        assert!(cauda.starts_with("xref\n0 11\n"));
        let quinto: usize = cauda.lines().nth(7).unwrap()[..10].parse().unwrap();
        assert!(b[quinto..].starts_with(b"5 0 obj\n<< /Type /Page "));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(windows)]
    #[test]
    fn pdftotext_que_nao_existe_e_nao_ligado() {
        // sem mexer no ambiente do processo: o programa que nao existe da o mesmo erro de "nao consegui rodar"
        assert!(rodar("pdftotext-que-nao-existe-braniac", &[], Duration::from_secs(5)).unwrap_err().starts_with("não consegui rodar"));
    }

    /// Ao vivo, so a pedido: `cargo test ao_vivo -- --ignored --nocapture`, com BRANIAC_TESTE_LEVANTAMENTO = pasta de um
    /// levantamento pronto (e COPIADA para a pasta temporaria; a original nao e tocada) e BRANIAC_PACOTE = o pacote de producao.
    /// Roda os geradores de verdade na sombra, a conferencia, a porta e os documentos. Nada e gravado no pacote.
    #[test]
    #[ignore]
    fn ao_vivo_roteiro_com_o_pacote_de_verdade() {
        let (Some(orig), Some(pac)) = (std::env::var_os("BRANIAC_TESTE_LEVANTAMENTO").map(PathBuf::from), std::env::var_os("BRANIAC_PACOTE").map(PathBuf::from)) else { return };
        let d = tmp("aovivo");
        let nome = orig.file_name().unwrap().to_string_lossy().to_string();
        let pasta = raiz(&d).join(&nome);
        copia_pasta(&orig, &pasta).unwrap();
        for sobra in ["roteiro", "documentos-capital", "releitura-tmp"] {
            apaga_pasta(&pasta.join(sobra));
        }
        let numero = nome.split('-').next().unwrap().to_string();
        let r = roteiro(&d, &numero, &pac, &gerador_real, "ao vivo");
        println!("roteiro: {r:?}\nconferir: {:?}\nporta: {:?}", conferir(&d, &numero, &pdftotext_real), porta(&d, &numero));
        println!("documentos: {:?}\npasta: {}", documentos(&d, &numero, &pdf_extrato), pasta.display());
        r.unwrap();
    }
}
