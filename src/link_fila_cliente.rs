//! Cliente da FILA do BRANIAC LINK (link/fila_cliente.py): monta e assina os pedidos (mesma identidade ja confirmada no par), entrega
//! arquivos e pega o resultado. A rede entra por closure (`http`); a assinatura Ed25519 tambem (`assinar`).
//! Ficou de fora: servir_painel/servir_predio (servidor HTTP local que so repassa GET) e o laco infinito espelhar_predio
//! (quem chama repete `olhando_predio` + `enviar_predio` no proprio relogio).
#![allow(dead_code)]
use crate::link_fila::assinar_pedido;
use crate::link_identidade::{hex, sha256, Ident};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub enum Corpo<'a> {
    Bytes(Vec<u8>),
    Arquivo(&'a Path),
}

/// http(metodo, url, cabecalhos, corpo) -> (status, corpo da resposta). Err = nao conectou.
pub type Http<'a> = &'a dyn Fn(&str, &str, &HashMap<String, String>, &Corpo) -> Result<(u16, Vec<u8>), String>;

pub struct Cliente<'a> {
    pub ident: &'a Ident,
    pub assinar: &'a dyn Fn(&[u8]) -> String,
    pub url: String,
    pub http: Http<'a>,
    pub agora: &'a dyn Fn() -> i64,
}

/// urllib.parse.quote: letras, digitos, "_.-~" e "/" ficam; o resto vira %XX.
pub fn quote(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"_.-~/".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

impl<'a> Cliente<'a> {
    pub fn new(ident: &'a Ident, assinar: &'a dyn Fn(&[u8]) -> String, url: &str, http: Http<'a>, agora: &'a dyn Fn() -> i64) -> Cliente<'a> {
        Cliente { ident, assinar, url: url.trim_end_matches('/').to_string(), http, agora }
    }

    /// Pedido assinado. Resposta 4xx/5xx vira Err("HTTP <codigo>: <erro>").
    pub fn pedido(&self, metodo: &str, caminho: &str, corpo: Corpo) -> Result<Vec<u8>, String> {
        // shortcut: o arquivo e lido inteiro para o SHA-256 (o sha256 do projeto nao e aos pedacos); trocar se passar de centenas de MB
        let (sha, tam) = match &corpo {
            Corpo::Bytes(b) => (hex(&sha256(b)), b.len()),
            Corpo::Arquivo(p) => {
                let b = fs::read(p).map_err(|e| format!("nao li {}: {e}", p.display()))?;
                (hex(&sha256(&b)), b.len())
            }
        };
        let mut cab = assinar_pedido(self.ident, self.assinar, metodo, caminho, &sha, (self.agora)());
        cab.insert("Content-Length".into(), tam.to_string());
        cab.insert("User-Agent".into(), "braniac-link-cliente/1".into());
        let (st, resp) = (self.http)(metodo, &format!("{}{caminho}", self.url), &cab, &corpo).map_err(|e| format!("sem conexao com a fila: {}", e.chars().take(80).collect::<String>()))?;
        if st >= 400 {
            let msg = serde_json::from_slice::<Value>(&resp).ok().and_then(|v| v["erro"].as_str().map(str::to_string)).unwrap_or_default();
            return Err(format!("HTTP {st}: {msg}"));
        }
        Ok(resp)
    }

    fn json(&self, metodo: &str, caminho: &str, corpo: Corpo) -> Result<Value, String> {
        serde_json::from_slice(&self.pedido(metodo, caminho, corpo)?).map_err(|_| "resposta da fila nao e JSON".to_string())
    }

    fn vazio() -> Corpo<'static> {
        Corpo::Bytes(Vec::new())
    }

    pub fn pedir(&self, meta: &Value) -> Result<String, String> {
        self.json("POST", "/pedido", Corpo::Bytes(meta.to_string().into_bytes()))?["id"].as_str().map(str::to_string).ok_or_else(|| "a fila nao devolveu o id".into())
    }

    pub fn enviar(&self, jid: &str, caminho: &Path, nome: Option<&str>) -> Result<String, String> {
        let nome = nome.map(str::to_string).or_else(|| caminho.file_name().map(|n| n.to_string_lossy().to_string())).unwrap_or_default();
        Ok(self.json("PUT", &format!("/job/{jid}/arquivo/{nome}"), Corpo::Arquivo(caminho))?["nome"].as_str().unwrap_or(&nome).to_string())
    }

    pub fn pronto(&self, jid: &str) -> Result<Value, String> {
        self.json("POST", &format!("/job/{jid}/pronto"), Self::vazio())
    }

    pub fn status(&self, jid: &str) -> Result<Value, String> {
        self.json("GET", &format!("/job/{jid}"), Self::vazio())
    }

    /// Sem cidade: resumo (pronto/producao/falta). Com cidade: a situacao dela.
    pub fn municipios(&self, cidade: &str) -> Result<Value, String> {
        self.json("GET", &if cidade.is_empty() { "/municipios".to_string() } else { format!("/municipios/{}", quote(cidade)) }, Self::vazio())
    }

    /// acao: reservar | concluir | liberar.
    pub fn reservar(&self, cidade: &str, acao: &str, nota: &str) -> Result<Value, String> {
        if !["reservar", "concluir", "liberar"].contains(&acao) {
            return Err("acao desconhecida".into());
        }
        self.json("POST", &format!("/municipios/{}/{acao}", quote(cidade)), Corpo::Bytes(json!({ "nota": nota }).to_string().into_bytes()))
    }

    /// Caixa de avisos: {"avisos": [{seq, quando, texto}], "ultimo": N}. Guarde o maior seq e passe como `desde` na proxima vez.
    pub fn avisos(&self, desde: u64) -> Result<Value, String> {
        self.json("GET", &format!("/avisos?desde={desde}"), Self::vazio())
    }

    pub fn mensagem(&self, texto: &str) -> Result<Value, String> {
        self.json("POST", "/mensagem", Corpo::Bytes(json!({ "texto": texto }).to_string().into_bytes()))
    }

    pub fn painel(&self) -> Result<Value, String> {
        self.json("GET", "/painel.json", Self::vazio())
    }

    pub fn baixar(&self, jid: &str, destino: &Path) -> Result<PathBuf, String> {
        let dados = self.pedido("GET", &format!("/job/{jid}/resultado"), Self::vazio())?;
        fs::write(destino, dados).map_err(|e| e.to_string())?;
        Ok(destino.to_path_buf())
    }

    /// Sobe a pasta do predio deste lado, so o que mudou desde a ultima vez (`ja` = caminho -> data de modificacao). Devolve o que foi enviado.
    pub fn enviar_predio(&self, pasta: &Path, ja: &mut HashMap<String, u128>) -> Result<Vec<String>, String> {
        let (mut arquivos, mut foi) = (Vec::new(), Vec::new());
        listar(pasta, pasta, &mut arquivos);
        arquivos.sort();
        for (rel, f) in arquivos {
            let mudou = fs::metadata(&f).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
            if ja.get(&rel) == Some(&mudou) {
                continue;
            }
            self.json("PUT", &format!("/predio-par/{}", rel.split('/').map(quote).collect::<Vec<_>>().join("/")), Corpo::Arquivo(&f))?;
            ja.insert(rel.clone(), mudou);
            foi.push(rel);
        }
        Ok(foi)
    }

    /// True quando o dono do outro lado esta com o predio aberto (so entao vale mandar dados novos).
    pub fn olhando_predio(&self) -> Result<bool, String> {
        Ok(self.json("GET", "/predio-par/olhando", Self::vazio())?["olhando"].as_bool().unwrap_or(false))
    }

    /// Ciclo inteiro: pedido, arquivos, "pronto", espera (ate `consultas` vezes, com `esperar()` entre elas) e baixa o resultado.
    pub fn enviar_tudo(&self, meta: &Value, arquivos: &[&Path], consultas: usize, esperar: &dyn Fn(), destino: Option<&Path>) -> Result<PathBuf, String> {
        let jid = self.pedir(meta)?;
        for a in arquivos {
            self.enviar(&jid, a, None)?;
        }
        self.pronto(&jid)?;
        for _ in 0..consultas {
            let st = self.status(&jid)?;
            match st["estado"].as_str() {
                Some("erro") => return Err(format!("a fila recusou ou falhou: {}", st["detalhe"].as_str().unwrap_or(""))),
                Some("concluido") => return self.baixar(&jid, destino.unwrap_or(Path::new(&format!("{jid}.mp4")))),
                _ => esperar(),
            }
        }
        Err("tempo esgotado esperando o render".into())
    }
}

fn listar(raiz: &Path, pasta: &Path, out: &mut Vec<(String, PathBuf)>) {
    for e in fs::read_dir(pasta).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            listar(raiz, &p, out);
        } else if !e.file_name().to_string_lossy().starts_with('.') {
            if let Ok(rel) = p.strip_prefix(raiz) {
                out.push((rel.to_string_lossy().replace('\\', "/"), p));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link_identidade::falso;
    use std::cell::RefCell;

    type Visto = RefCell<Vec<(String, String, HashMap<String, String>, usize)>>;

    fn servidor<'a>(visto: &'a Visto, respostas: &'a RefCell<Vec<(u16, &'static str)>>) -> impl Fn(&str, &str, &HashMap<String, String>, &Corpo) -> Result<(u16, Vec<u8>), String> + 'a {
        move |m, url, cab, corpo| {
            let tam = match corpo {
                Corpo::Bytes(b) => b.len(),
                Corpo::Arquivo(p) => fs::read(p).unwrap().len(),
            };
            visto.borrow_mut().push((m.to_string(), url.to_string(), cab.clone(), tam));
            let (st, txt) = respostas.borrow_mut().remove(0);
            Ok((st, txt.as_bytes().to_vec()))
        }
    }

    #[test]
    fn pedido_vai_assinado_com_sha_do_corpo_e_erro_http_vira_mensagem() {
        let i = falso::ident("Ana");
        let assinar = |d: &[u8]| falso::assinar(&i, d);
        let (visto, resp) = (Visto::default(), RefCell::new(vec![(200, r#"{"id":"abc123"}"#), (403, r#"{"erro":"sem permissao"}"#), (200, r#"{"avisos":[],"ultimo":4}"#)]));
        let http = servidor(&visto, &resp);
        let c = Cliente::new(&i, &assinar, "https://fila.exemplo/", &http, &|| 1000);
        let meta = json!({"titulo": "Teste"});
        assert_eq!(c.pedir(&meta).unwrap(), "abc123");
        assert_eq!(c.pronto("abc123").unwrap_err(), "HTTP 403: sem permissao");
        assert_eq!(c.avisos(3).unwrap()["ultimo"], 4);
        let v = visto.borrow();
        assert_eq!((v[0].0.as_str(), v[0].1.as_str()), ("POST", "https://fila.exemplo/pedido"));
        let cab = &v[0].2;
        assert_eq!(cab["X-Link-Sha"], hex(&sha256(meta.to_string().as_bytes())));
        assert_eq!(cab["X-Link-Id"], i.id);
        assert_eq!(cab["X-Link-Ts"], "1000");
        assert_eq!(cab["Content-Length"], meta.to_string().len().to_string());
        assert!(!cab["X-Link-Sig"].is_empty() && cab["User-Agent"].starts_with("braniac-link-cliente"));
        assert_eq!(v[1].2["X-Link-Sha"], hex(&sha256(b""))); // corpo vazio assina o SHA do vazio
        assert_eq!(v[2].1, "https://fila.exemplo/avisos?desde=3");
    }

    #[test]
    fn sem_rede_acao_estranha_e_quote() {
        let i = falso::ident("Ana");
        let assinar = |d: &[u8]| falso::assinar(&i, d);
        let fora = |_: &str, _: &str, _: &HashMap<String, String>, _: &Corpo| -> Result<(u16, Vec<u8>), String> { Err("recusou".into()) };
        let c = Cliente::new(&i, &assinar, "http://x", &fora, &|| 1);
        assert!(c.status("j").unwrap_err().starts_with("sem conexao com a fila"));
        assert_eq!(c.reservar("Cidade", "apagar", "").unwrap_err(), "acao desconhecida");
        assert_eq!(quote("São João/x y"), "S%C3%A3o%20Jo%C3%A3o/x%20y");
    }

    #[test]
    fn ciclo_inteiro_e_predio_so_manda_o_que_mudou() {
        let p = std::env::temp_dir().join(format!("braniac-link_fila_cliente-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(p.join("predio/dados")).unwrap();
        fs::write(p.join("audio.mp3"), b"som").unwrap();
        fs::write(p.join("predio/index.html"), b"<html>").unwrap();
        fs::write(p.join("predio/dados/a b.json"), b"{}").unwrap();
        fs::write(p.join("predio/.oculto"), b"x").unwrap();
        let i = falso::ident("Ana");
        let assinar = |d: &[u8]| falso::assinar(&i, d);
        let (visto, resp) = (
            Visto::default(),
            RefCell::new(vec![(200, r#"{"id":"j1"}"#), (200, r#"{"nome":"audio.mp3"}"#), (200, "{}"), (200, r#"{"estado":"processando"}"#), (200, r#"{"estado":"concluido"}"#), (200, "VIDEO"), (200, "{}"), (200, "{}"), (200, r#"{"olhando":true}"#)]),
        );
        let http = servidor(&visto, &resp);
        let c = Cliente::new(&i, &assinar, "http://x", &http, &|| 5);
        let esperas = RefCell::new(0);
        let saida = c.enviar_tudo(&json!({"titulo": "T"}), &[&p.join("audio.mp3")], 5, &|| *esperas.borrow_mut() += 1, Some(&p.join("saida.mp4"))).unwrap();
        assert_eq!(fs::read(saida).unwrap(), b"VIDEO");
        assert_eq!(*esperas.borrow(), 1);
        assert_eq!(visto.borrow()[1].1, "http://x/job/j1/arquivo/audio.mp3");
        let mut ja = HashMap::new();
        assert_eq!(c.enviar_predio(&p.join("predio"), &mut ja).unwrap(), vec!["dados/a b.json", "index.html"]);
        assert_eq!(visto.borrow()[6].1, "http://x/predio-par/dados/a%20b.json");
        assert!(c.enviar_predio(&p.join("predio"), &mut ja).unwrap().is_empty()); // nada mudou: nada sobe (nem gasta resposta)
        assert!(c.olhando_predio().unwrap());
        // erro no render vira Err com o detalhe
        resp.borrow_mut().extend([(200, r#"{"id":"j2"}"#), (200, "{}"), (200, r#"{"estado":"erro","detalhe":"audio ilegivel"}"#)]);
        assert!(c.enviar_tudo(&json!({}), &[], 3, &|| (), None).unwrap_err().contains("audio ilegivel"));
        let _ = fs::remove_dir_all(p);
    }
}
