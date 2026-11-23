//! Regras da FILA de pedidos do BRANIAC LINK (link/fila.py), por onde um par confirmado entrega arquivos (audio, roteiro) e pega
//! o resultado. Sem senha: cada pedido e ASSINADO pelo dispositivo e so passa se o par for confiavel e tiver a permissao "tarefa"
//! = "aceitar" para o agente da fila ("perguntar" nao basta). Nada vindo de fora e executado: o `processador` de quem chama
//! recebe SO dados (meta.json + arquivos de tipos permitidos) e devolve o arquivo pronto.
//! Ficou de fora: o servidor HTTP e as rotas (municipios, predio, painel, avisos, mensagem), o trabalhador em segundo plano e o
//! tunel. O tunel NASCE DESLIGADO e nenhum endereco e anunciado sozinho: aqui so existe a leitura da chave (`tunel_ligado`);
//! ligar e botao do dono. Verificar assinatura, conferir zip, processar e avisar entram como closure.
#![allow(dead_code)]
use crate::link_confianca::Pares;
use crate::link_identidade::{id_novo, Ident};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const TOLERANCIA_S: f64 = 300.0;
pub const EXT_OK: [&str; 14] = [".json", ".txt", ".md", ".mp3", ".wav", ".m4a", ".ogg", ".aac", ".flac", ".png", ".jpg", ".jpeg", ".webp", ".zip"];
/// So no tipo producao_completa (pacote de pesquisa + modelo de video).
pub const EXT_SO_PRODUCAO: [&str; 2] = [".pdf", ".mp4"];
pub const ESTADOS: [&str; 6] = ["criado", "pronto", "processando", "concluido", "erro", "producao"];
pub const CABECALHOS: [&str; 5] = ["X-Link-Id", "X-Link-Ts", "X-Link-Sha", "X-Link-Sig", "X-Link-Nonce"];

/// O tunel da fila so esta ligado se o dono ligou (o arquivo dados/link/tunel_ligado existe). Nasce desligado.
pub fn tunel_ligado(dados: &Path) -> bool {
    dados.join("link").join("tunel_ligado").exists()
}

/// Codigo HTTP de um erro da fila: 403 para quem nao pode, 400 para pedido errado.
pub fn codigo_http(erro: &str) -> u16 {
    if ["permissao", "assinatura", "repetido", "data"].iter().any(|p| erro.contains(p)) { 403 } else { 400 }
}

/// Nome de arquivo sem caminho e sem caractere estranho, com ate 120 caracteres.
pub fn nome_seguro(nome: &str) -> String {
    let limpo = nome.replace('\\', "/");
    let base = limpo.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    let s: String = base.chars().map(|c| if c.is_alphanumeric() || "_.- ".contains(c) { c } else { '_' }).collect();
    let s = s.trim_matches([' ', '.']);
    if s.is_empty() { "arquivo".into() } else { s.chars().take(120).collect() }
}

fn extensao(nome: &str) -> String {
    nome.rfind('.').filter(|i| *i > 0).map(|i| nome[i..].to_lowercase()).unwrap_or_default()
}

fn texto_assinado(metodo: &str, caminho: &str, ts: &str, sha: &str, id: &str, nonce: &str) -> Vec<u8> {
    [metodo, caminho, ts, sha, id, nonce].join("\n").into_bytes()
}

/// Cabecalhos de um pedido assinado (lado cliente).
pub fn assinar_pedido(ident: &Ident, assinar: &dyn Fn(&[u8]) -> String, metodo: &str, caminho: &str, corpo_sha: &str, agora: i64) -> HashMap<String, String> {
    let (ts, nonce) = (agora.to_string(), id_novo(16));
    let sig = assinar(&texto_assinado(metodo, caminho, &ts, corpo_sha, &ident.id, &nonce));
    CABECALHOS.iter().map(|k| k.to_string()).zip([ident.id.clone(), ts, corpo_sha.to_string(), sig, nonce]).collect()
}

// "x or ''" do Python seguido de strip(): vazio, nulo, zero e so espaco nao contam
fn preenchido(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.trim().is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

pub struct Fila<'a> {
    pub pares: &'a Pares,
    pub pasta: PathBuf,
    /// Agente cuja permissao "tarefa" libera a fila.
    pub agente: String,
    pub max_dia: usize,
    pub max_bytes: u64,
    usados: Mutex<HashMap<String, f64>>,
    trava: Mutex<()>,
}

impl<'a> Fila<'a> {
    pub fn new(pares: &'a Pares, pasta: &Path, agente: &str, max_dia: usize, max_bytes: u64) -> Result<Fila<'a>, String> {
        std::fs::create_dir_all(pasta.join("jobs")).map_err(|e| e.to_string())?;
        Ok(Fila { pares, pasta: pasta.to_path_buf(), agente: agente.to_string(), max_dia, max_bytes, usados: Mutex::new(HashMap::new()), trava: Mutex::new(()) })
    }

    // ---------- autenticacao ----------
    /// `h`: cabecalhos do pedido. Devolve o id do par ou o motivo da recusa. Confere ANTES de qualquer leitura do corpo.
    /// `verificar(chave publica, dados, assinatura)`.
    pub fn autenticar(&self, metodo: &str, caminho: &str, h: &HashMap<String, String>, verificar: &dyn Fn(&str, &[u8], &str) -> bool, agora: f64) -> Result<String, String> {
        let de = |k: &str| h.get(k).map_or("", |v| v.as_str());
        let (pid, ts, sha, sig, nonce) = (de("X-Link-Id"), de("X-Link-Ts"), de("X-Link-Sha"), de("X-Link-Sig"), de("X-Link-Nonce"));
        let par = self.pares.get(pid).filter(|p| p["estado"] == "confiavel" && self.pares.decidir(pid, "tarefa", &self.agente) == "aceitar").ok_or("par sem permissao")?;
        let no_prazo = !ts.is_empty() && ts.bytes().all(|b| b.is_ascii_digit()) && ts.parse::<i64>().is_ok_and(|t| (agora - t as f64).abs() <= TOLERANCIA_S);
        if !no_prazo {
            return Err("data fora da tolerancia".into());
        }
        if sig.is_empty() || !verificar(par["pub"].as_str().unwrap_or(""), &texto_assinado(metodo, caminho, ts, sha, pid, nonce), sig) {
            return Err("assinatura invalida".into());
        }
        let mut usados = self.usados.lock().map_err(|e| e.to_string())?;
        usados.retain(|_, t| agora - *t < 2.0 * TOLERANCIA_S);
        if usados.insert(sig.to_string(), agora).is_some() {
            return Err("pedido repetido".into());
        }
        Ok(pid.to_string())
    }

    // ---------- jobs ----------
    fn dir(&self, jid: &str) -> Result<PathBuf, String> {
        if jid.len() != 12 || !jid.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
            return Err("id de job invalido".into());
        }
        Ok(self.pasta.join("jobs").join(jid))
    }

    fn json_de(arq: &Path) -> Option<Value> {
        serde_json::from_str::<Value>(&std::fs::read_to_string(arq).ok()?).ok().filter(Value::is_object)
    }

    fn estado(&self, jid: &str) -> Result<Value, String> {
        Self::json_de(&self.dir(jid)?.join("estado.json")).ok_or_else(|| "job nao encontrado".to_string())
    }

    fn gravar_estado(&self, jid: &str, campos: Value, agora: i64) -> Result<Value, String> {
        let _t = self.trava.lock().map_err(|e| e.to_string())?;
        let arq = self.dir(jid)?.join("estado.json");
        let mut e = Self::json_de(&arq).unwrap_or_else(|| json!({}));
        if let (Some(e), Some(c)) = (e.as_object_mut(), campos.as_object()) {
            e.extend(c.clone());
        }
        e["atualizado"] = json!(agora);
        std::fs::write(&arq, e.to_string()).map_err(|x| x.to_string())?;
        Ok(e)
    }

    fn do_dono(&self, jid: &str, pid: &str) -> Result<Value, String> {
        let e = self.estado(jid)?;
        if e["par"].as_str() != Some(pid) {
            return Err("job de outro par".into());
        }
        Ok(e)
    }

    fn meta(&self, jid: &str) -> Value {
        self.dir(jid).ok().and_then(|d| Self::json_de(&d.join("meta.json"))).unwrap_or_else(|| json!({}))
    }

    fn todos(&self) -> Vec<(String, Value)> {
        std::fs::read_dir(self.pasta.join("jobs")).into_iter().flatten().flatten()
            .filter_map(|d| Some((d.file_name().to_string_lossy().to_string(), Self::json_de(&d.path().join("estado.json"))?))).collect()
    }

    /// Abre um pedido. `hoje`: a data local "AAAA-MM-DD" (o limite e por par e por dia).
    pub fn criar(&self, pid: &str, meta: &Value, hoje: &str, agora: i64) -> Result<String, String> {
        if self.todos().iter().filter(|(_, e)| e["par"] == pid && e["dia"] == hoje).count() >= self.max_dia {
            return Err(format!("limite de {} pedidos por dia", self.max_dia));
        }
        if !meta.is_object() {
            return Err("meta invalida".into());
        }
        if meta["tipo"] == "producao_completa" && !(preenchido(&meta["numero"]) && preenchido(&meta["cidade"])) {
            return Err("producao_completa precisa de numero e cidade".into());
        }
        let jid = id_novo(12);
        let d = self.dir(&jid)?;
        std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
        std::fs::write(d.join("meta.json"), meta.to_string()).map_err(|e| e.to_string())?;
        self.gravar_estado(&jid, json!({ "par": pid, "estado": "criado", "dia": hoje, "criado": agora, "bytes": 0 }), agora)?;
        Ok(jid)
    }

    /// Guarda um arquivo recebido (ja em `caminho_tmp`) no job. So enquanto o job esta "criado", so extensao da lista, so dentro do
    /// tamanho do job. `conferir_zip`: o zip do pacote de producao e conferido por quem chama antes de entrar.
    pub fn receber_arquivo(&self, jid: &str, pid: &str, nome: &str, caminho_tmp: &Path, conferir_zip: &dyn Fn(&Path) -> Result<(), String>, agora: i64) -> Result<String, String> {
        let e = self.do_dono(jid, pid)?;
        if e["estado"] != "criado" {
            return Err("job nao aceita mais arquivos".into());
        }
        let seguro = nome_seguro(nome);
        let producao = self.meta(jid)["tipo"] == "producao_completa";
        let ext = extensao(&seguro);
        if !(EXT_OK.contains(&ext.as_str()) || (producao && EXT_SO_PRODUCAO.contains(&ext.as_str()))) {
            return Err("tipo de arquivo nao permitido".into());
        }
        let tam = std::fs::metadata(caminho_tmp).map_err(|x| x.to_string())?.len();
        let ja = e["bytes"].as_u64().unwrap_or(0);
        if ja + tam > self.max_bytes {
            return Err("job grande demais".into());
        }
        if producao && ext == ".zip" {
            conferir_zip(caminho_tmp)?;
        }
        let alvo = self.dir(jid)?.join(&seguro);
        if std::fs::rename(caminho_tmp, &alvo).is_err() {
            std::fs::copy(caminho_tmp, &alvo).map_err(|x| x.to_string())?; // outro disco: copia e apaga
            let _ = std::fs::remove_file(caminho_tmp);
        }
        self.gravar_estado(jid, json!({ "bytes": ja + tam }), agora)?;
        Ok(seguro)
    }

    /// O par terminou de mandar. Pedido comum vira "pronto" (entra na fila do processador). Pedido producao_completa vai para
    /// `ao_producao(par, job, meta, pasta)` e devolve a posicao que ela der; sem ela, e recusado.
    pub fn pronto(&self, jid: &str, pid: &str, ao_producao: Option<&dyn Fn(&str, &str, &Value, &Path) -> Result<Value, String>>, agora: i64) -> Result<Option<Value>, String> {
        self.do_dono(jid, pid)?;
        let meta = self.meta(jid);
        if meta["tipo"] == "producao_completa" {
            let Some(ao) = ao_producao else { return Err("fila de producao desligada".into()) };
            self.gravar_estado(jid, json!({ "estado": "producao" }), agora)?;
            return ao(pid, jid, &meta, &self.dir(jid)?).map(Some);
        }
        self.gravar_estado(jid, json!({ "estado": "pronto" }), agora)?;
        Ok(None)
    }

    fn concluir(&self, jid: &str, arquivo: &Path, agora: i64) -> Result<(), String> {
        let alvo = self.dir(jid)?.join("resultado.mp4");
        let tam = std::fs::copy(arquivo, &alvo).map_err(|e| e.to_string())?;
        self.gravar_estado(jid, json!({ "estado": "concluido", "detalhe": "", "resultado_bytes": tam }), agora).map(|_| ())
    }

    /// Pedido de producao entregue: o arquivo vira o resultado do job.
    pub fn concluir_externo(&self, jid: &str, arquivo: &Path, agora: i64) -> Result<(), String> {
        self.estado(jid)?;
        self.concluir(jid, arquivo, agora)
    }

    pub fn status(&self, jid: &str, pid: &str) -> Result<Value, String> {
        let e = self.do_dono(jid, pid)?;
        Ok(json!({ "estado": e["estado"], "detalhe": e["detalhe"], "criado": e["criado"], "atualizado": e["atualizado"], "bytes": e["bytes"], "resultado_bytes": e["resultado_bytes"] }))
    }

    /// Caminho do resultado, so para o dono do job e so depois de concluido.
    pub fn resultado(&self, jid: &str, pid: &str) -> Result<PathBuf, String> {
        let e = self.do_dono(jid, pid)?;
        let arq = self.dir(jid)?.join("resultado.mp4");
        if e["estado"] != "concluido" || !arq.exists() {
            return Err("resultado ainda nao existe".into());
        }
        Ok(arq)
    }

    // ---------- trabalhador ----------
    fn proximo(&self) -> Option<(String, Value)> {
        self.todos().into_iter().filter(|(_, e)| e["estado"] == "pronto").min_by_key(|(jid, e)| (e["criado"].as_i64().unwrap_or(0), jid.clone()))
    }

    /// Processa o job pronto mais antigo (se houver) e devolve o id. `processador(pasta do job, meta)` devolve o arquivo pronto;
    /// `avisar(par, job, estado, detalhe)`. Erro do processador vira estado "erro", com o motivo cortado em 300 caracteres.
    pub fn processar_um(&self, processador: &dyn Fn(&Path, &Value) -> Result<PathBuf, String>, avisar: &dyn Fn(&str, &str, &str, &str), agora: i64) -> Option<String> {
        let (jid, e) = self.proximo()?;
        let par = e["par"].as_str().unwrap_or("").to_string();
        self.gravar_estado(&jid, json!({ "estado": "processando", "detalhe": "" }), agora).ok()?;
        avisar(&par, &jid, "processando", "");
        let feito = self.dir(&jid).and_then(|d| {
            let meta = Self::json_de(&d.join("meta.json")).ok_or("meta invalida")?;
            self.concluir(&jid, &processador(&d, &meta)?, agora)
        });
        match feito {
            Ok(()) => avisar(&par, &jid, "concluido", ""),
            Err(x) => {
                let detalhe: String = x.chars().take(300).collect();
                let _ = self.gravar_estado(&jid, json!({ "estado": "erro", "detalhe": detalhe }), agora);
                avisar(&par, &jid, "erro", &detalhe);
            }
        }
        Some(jid)
    }

    /// Ao ligar: job que ficou "processando" (o programa reiniciou no meio) volta para a fila. Devolve quantos.
    pub fn retomar(&self, agora: i64) -> usize {
        self.todos().iter().filter(|(jid, e)| e["estado"] == "processando" && self.gravar_estado(jid, json!({ "estado": "pronto" }), agora).is_ok()).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link_identidade::falso;
    use std::cell::RefCell;

    struct Cena {
        tmp: PathBuf,
        pares: Pares,
        par: Ident,
        intruso: Ident,
    }

    fn cena(nome: &str) -> Cena {
        let tmp = std::env::temp_dir().join(format!("braniac-link_fila-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let pares = Pares::new(&tmp.join("pares")).unwrap();
        let (par, intruso) = (falso::ident("Par"), falso::ident("Intruso"));
        pares.salvar_par(&par.id, "Par", &par.chave_pub, "seg", 1).unwrap();
        pares.confirmar(&par.id).unwrap();
        Cena { tmp, pares, par, intruso }
    }

    fn fila(c: &Cena) -> Fila<'_> {
        Fila::new(&c.pares, &c.tmp.join("fila"), "render", 3, 100_000).unwrap()
    }

    fn cab(i: &Ident, metodo: &str, caminho: &str, agora: i64) -> HashMap<String, String> {
        assinar_pedido(i, &|d| falso::assinar(i, d), metodo, caminho, "sha-do-corpo", agora)
    }

    fn arquivo(c: &Cena, nome: &str, n: usize) -> PathBuf {
        let p = c.tmp.join(format!("_rec-{nome}"));
        std::fs::write(&p, vec![b'x'; n]).unwrap();
        p
    }

    fn sem_zip(_: &Path) -> Result<(), String> {
        Ok(())
    }

    #[test]
    fn sem_permissao_ou_par_desconhecido_e_barrado() {
        let c = cena("permissao");
        let f = fila(&c);
        let entra = |i: &Ident| f.autenticar("POST", "/pedido", &cab(i, "POST", "/pedido", 1000), &falso::verificar, 1000.0);
        assert_eq!(entra(&c.par).unwrap_err(), "par sem permissao"); // confiavel, mas sem permissao para o agente render
        c.pares.permitir(&c.par.id, "render", "tarefa", "perguntar").unwrap();
        assert_eq!(entra(&c.par).unwrap_err(), "par sem permissao"); // "perguntar" tambem nao basta para a fila automatica
        c.pares.permitir(&c.par.id, "render", "tarefa", "aceitar").unwrap();
        assert_eq!(entra(&c.intruso).unwrap_err(), "par sem permissao"); // desconhecido
        assert_eq!(entra(&c.par), Ok(c.par.id.clone()));
        c.pares.revogar(&c.par.id).unwrap();
        assert_eq!(entra(&c.par).unwrap_err(), "par sem permissao");
        assert_eq!((codigo_http("par sem permissao"), codigo_http("assinatura invalida"), codigo_http("pedido repetido"), codigo_http("data fora da tolerancia"), codigo_http("job grande demais")), (403, 403, 403, 403, 400));
        assert!(!tunel_ligado(&c.tmp)); // o tunel nasce desligado
        let _ = std::fs::remove_dir_all(&c.tmp);
    }

    #[test]
    fn assinatura_adulterada_repeticao_e_data() {
        let c = cena("assinatura");
        c.pares.permitir(&c.par.id, "render", "tarefa", "aceitar").unwrap();
        let f = fila(&c);
        let h = cab(&c.par, "GET", "/job/aaaaaaaaaaaa", 1000);
        f.autenticar("GET", "/job/aaaaaaaaaaaa", &h, &falso::verificar, 1000.0).unwrap();
        assert_eq!(f.autenticar("GET", "/job/aaaaaaaaaaaa", &h, &falso::verificar, 1001.0).unwrap_err(), "pedido repetido");
        let h2 = cab(&c.par, "GET", "/job/aaaaaaaaaaaa", 1000);
        assert_eq!(f.autenticar("GET", "/job/bbbbbbbbbbbb", &h2, &falso::verificar, 1000.0).unwrap_err(), "assinatura invalida"); // caminho diferente do assinado
        assert_eq!(f.autenticar("POST", "/job/aaaaaaaaaaaa", &h2, &falso::verificar, 1000.0).unwrap_err(), "assinatura invalida");
        let mut outro_corpo = h2.clone();
        outro_corpo.insert("X-Link-Sha".into(), "outro".into());
        assert_eq!(f.autenticar("GET", "/job/aaaaaaaaaaaa", &outro_corpo, &falso::verificar, 1000.0).unwrap_err(), "assinatura invalida");
        let mut sem_sig = h2.clone();
        sem_sig.insert("X-Link-Sig".into(), String::new());
        assert_eq!(f.autenticar("GET", "/job/aaaaaaaaaaaa", &sem_sig, &|_, _, _| true, 1000.0).unwrap_err(), "assinatura invalida");
        let velho = cab(&c.par, "GET", "/x", 1000);
        assert_eq!(f.autenticar("GET", "/x", &velho, &falso::verificar, 5000.0).unwrap_err(), "data fora da tolerancia");
        let mut ts_ruim = velho.clone();
        ts_ruim.insert("X-Link-Ts".into(), "-1000".into());
        assert_eq!(f.autenticar("GET", "/x", &ts_ruim, &falso::verificar, 1000.0).unwrap_err(), "data fora da tolerancia");
        assert_eq!(f.autenticar("GET", "/x", &HashMap::new(), &falso::verificar, 1000.0).unwrap_err(), "par sem permissao");
        let _ = std::fs::remove_dir_all(&c.tmp);
    }

    #[test]
    fn ciclo_completo_e_limites() {
        let c = cena("ciclo");
        let f = fila(&c);
        let p = c.par.id.as_str();
        let jid = f.criar(p, &json!({ "titulo": "t", "cenas": [] }), "2026-11-18", 100).unwrap();
        assert_eq!(f.receber_arquivo(&jid, p, "C:\\pasta\\audio.mp3", &arquivo(&c, "a", 11), &sem_zip, 101), Ok("audio.mp3".to_string()));
        assert_eq!(f.resultado(&jid, p).unwrap_err(), "resultado ainda nao existe");
        assert_eq!(f.pronto(&jid, p, None, 102), Ok(None));
        assert_eq!(f.receber_arquivo(&jid, p, "b.mp3", &arquivo(&c, "b", 1), &sem_zip, 103).unwrap_err(), "job nao aceita mais arquivos");
        let avisos = RefCell::new(vec![]);
        let avisar = |par: &str, _: &str, estado: &str, _: &str| avisos.borrow_mut().push(format!("{par} {estado}"));
        let proc = |pasta: &Path, meta: &Value| -> Result<PathBuf, String> {
            assert_eq!(meta["titulo"], "t");
            let saida = pasta.join("feito.bin");
            let audio = std::fs::read(pasta.join("audio.mp3")).map_err(|e| e.to_string())?;
            std::fs::write(&saida, [b"MP4".as_slice(), &audio].concat()).map_err(|e| e.to_string())?;
            Ok(saida)
        };
        assert_eq!(f.processar_um(&proc, &avisar, 104), Some(jid.clone()));
        assert_eq!(f.processar_um(&proc, &avisar, 105), None);
        assert_eq!(*avisos.borrow(), [format!("{p} processando"), format!("{p} concluido")]);
        assert_eq!(std::fs::read(f.resultado(&jid, p).unwrap()).unwrap(), b"MP4xxxxxxxxxxx");
        let st = f.status(&jid, p).unwrap();
        assert!(st["estado"] == "concluido" && st["bytes"] == 11 && st["resultado_bytes"] == 14 && st.get("par").is_none());

        let jid = f.criar(p, &json!({}), "2026-11-18", 110).unwrap();
        for ruim in ["x.exe", "x.bat", "x.ps1", "x", "x.mp3.exe", "video.mp4", "doc.pdf", ".."] {
            assert_eq!(f.receber_arquivo(&jid, p, ruim, &arquivo(&c, "r", 2), &sem_zip, 111).unwrap_err(), "tipo de arquivo nao permitido", "{ruim}");
        }
        assert_eq!(f.receber_arquivo(&jid, p, "g.mp3", &arquivo(&c, "g", 200_000), &sem_zip, 112).unwrap_err(), "job grande demais");
        assert_eq!(f.receber_arquivo(&jid, p, "g1.mp3", &arquivo(&c, "g1", 60_000), &sem_zip, 112), Ok("g1.mp3".to_string()));
        assert_eq!(f.receber_arquivo(&jid, p, "g2.mp3", &arquivo(&c, "g2", 60_000), &sem_zip, 112).unwrap_err(), "job grande demais"); // soma do job
        f.criar(p, &json!({}), "2026-11-18", 113).unwrap();
        assert_eq!(f.criar(p, &json!({}), "2026-11-18", 114).unwrap_err(), "limite de 3 pedidos por dia"); // 4o pedido do dia
        assert!(f.criar(p, &json!({}), "2026-11-19", 115).is_ok()); // no outro dia volta
        assert_eq!(f.criar(&c.intruso.id, &json!([1]), "2026-11-18", 116).unwrap_err(), "meta invalida");
        let _ = std::fs::remove_dir_all(&c.tmp);
    }

    #[test]
    fn job_de_outro_par_id_invalido_e_erro_do_processador() {
        let c = cena("outro");
        let f = fila(&c);
        let p = c.par.id.as_str();
        let jid = f.criar(p, &json!({}), "2026-11-18", 100).unwrap();
        let outro = c.intruso.id.as_str();
        assert_eq!(f.status(&jid, outro).unwrap_err(), "job de outro par");
        assert_eq!(f.resultado(&jid, outro).unwrap_err(), "job de outro par");
        assert_eq!(f.pronto(&jid, outro, None, 101).unwrap_err(), "job de outro par");
        assert_eq!(f.receber_arquivo(&jid, outro, "a.mp3", &arquivo(&c, "a", 1), &sem_zip, 101).unwrap_err(), "job de outro par");
        for ruim in ["../../etc", "AAAAAAAAAAAA", "abc", "aaaaaaaaaaa/"] {
            assert_eq!(f.status(ruim, p).unwrap_err(), "id de job invalido");
        }
        assert_eq!(f.status("aaaaaaaaaaaa", p).unwrap_err(), "job nao encontrado");
        f.pronto(&jid, p, None, 102).unwrap();
        let avisos = RefCell::new(vec![]);
        let avisar = |_: &str, _: &str, estado: &str, detalhe: &str| avisos.borrow_mut().push(format!("{estado} {}", detalhe.len()));
        assert_eq!(f.processar_um(&|_, _| Err("e".repeat(999)), &avisar, 103), Some(jid.clone())); // o processador falha
        assert_eq!(*avisos.borrow(), ["processando 0", "erro 300"]);
        assert_eq!(f.status(&jid, p).unwrap()["estado"], "erro");
        assert!(f.resultado(&jid, p).is_err());
        // reinicio no meio de um processamento: o job volta para a fila
        let j2 = f.criar(p, &json!({}), "2026-11-18", 104).unwrap();
        f.gravar_estado(&j2, json!({ "estado": "processando" }), 105).unwrap();
        assert_eq!((f.retomar(106), f.status(&j2, p).unwrap()["estado"].as_str()), (1, Some("pronto")));
        let _ = std::fs::remove_dir_all(&c.tmp);
    }

    #[test]
    fn producao_completa_tem_regras_proprias() {
        let c = cena("producao");
        let f = fila(&c);
        let p = c.par.id.as_str();
        for meta in [json!({ "tipo": "producao_completa" }), json!({ "tipo": "producao_completa", "numero": "155", "cidade": "  " }), json!({ "tipo": "producao_completa", "numero": 0, "cidade": "X" })] {
            assert_eq!(f.criar(p, &meta, "2026-11-18", 100).unwrap_err(), "producao_completa precisa de numero e cidade");
        }
        let jid = f.criar(p, &json!({ "tipo": "producao_completa", "numero": "155", "cidade": "Jussiape" }), "2026-11-18", 100).unwrap();
        assert_eq!(f.receber_arquivo(&jid, p, "fontes.pdf", &arquivo(&c, "pdf", 5), &sem_zip, 101), Ok("fontes.pdf".to_string())); // pdf so neste tipo
        assert_eq!(f.receber_arquivo(&jid, p, "pacote.zip", &arquivo(&c, "zip", 5), &|_| Err("zip com arquivo proibido".into()), 101).unwrap_err(), "zip com arquivo proibido");
        assert_eq!(f.receber_arquivo(&jid, p, "x.exe", &arquivo(&c, "exe", 5), &sem_zip, 101).unwrap_err(), "tipo de arquivo nao permitido");
        assert_eq!(f.pronto(&jid, p, None, 102).unwrap_err(), "fila de producao desligada");
        let ao = |_: &str, _: &str, meta: &Value, _: &Path| Ok(json!({ "posicao": 1, "cidade": meta["cidade"] }));
        assert_eq!(f.pronto(&jid, p, Some(&ao), 103).unwrap().unwrap()["cidade"], "Jussiape");
        assert_eq!(f.status(&jid, p).unwrap()["estado"], "producao");
        assert_eq!(f.processar_um(&|_, _| panic!("producao nao vai para o render"), &|_, _, _, _| (), 104), None);
        let mp4 = arquivo(&c, "mp4", 9);
        f.concluir_externo(&jid, &mp4, 105).unwrap();
        assert!(f.resultado(&jid, p).is_ok() && f.concluir_externo("aaaaaaaaaaaa", &mp4, 106).is_err());
        assert_eq!((nome_seguro("../../a b.TXT"), nome_seguro("c:\\x\\..\\"), nome_seguro("vírus<>.mp3"), nome_seguro(&"a".repeat(300)).len()), ("a b.TXT".into(), "arquivo".into(), "vírus__.mp3".into(), 120));
        let _ = std::fs::remove_dir_all(&c.tmp);
    }
}
