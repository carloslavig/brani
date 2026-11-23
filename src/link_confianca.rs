//! Convites, pares conhecidos e permissoes do BRANIAC LINK (link/confianca.py). Nada e confiavel por padrao: par novo fica
//! "pendente" ate o dono conferir o codigo curto (SAS) com a outra pessoa e chamar `confirmar`. Revogar apaga o segredo do canal e
//! bloqueia o dispositivo. `decidir` responde aceitar | perguntar | recusar; desconhecido, pendente ou revogado: recusar.
//! Arquivo pares.json no formato do Python. O segredo do convite (16 bytes sorteados) entra por parametro: aqui nao ha sorteio.
#![allow(dead_code)]
use crate::link_identidade::{b64, hmac_sha256, id_de, iguais, sha256, unb64, Ident};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

pub const PREFIXO: &str = "BLINK1.";
pub const VALIDADE_CONVITE_S: i64 = 900;
pub const ACOES: [&str; 3] = ["msg", "tarefa", "arquivo"];
pub const NIVEIS: [&str; 3] = ["recusar", "perguntar", "aceitar"];

/// Tarefa remota nunca roda sem o dono liberar.
fn padrao(acao: &str) -> &'static str {
    match acao {
        "msg" => "aceitar",
        "arquivo" => "perguntar",
        _ => "recusar",
    }
}

/// Codigo de 6 digitos igual nos dois lados, para conferir por voz (detecta alguem no meio).
pub fn sas(pub_a: &str, pub_b: &str) -> Result<String, String> {
    let (mut a, mut b) = (unb64(pub_a)?, unb64(pub_b)?);
    if a > b {
        std::mem::swap(&mut a, &mut b);
    }
    let h = sha256(&[b"blink-sas".as_slice(), &a, &b].concat());
    Ok(format!("{:06}", u32::from_be_bytes([h[0], h[1], h[2], h[3]]) % 1_000_000))
}

/// Prova de que o convidado leu o convite (HMAC do segredo de uso unico).
pub fn prova(seg_b64: &str, id_convidante: &str, id_convidado: &str) -> Result<String, String> {
    Ok(b64(&hmac_sha256(&unb64(seg_b64)?, format!("{id_convidante}|{id_convidado}").as_bytes())))
}

/// Le o texto de um convite. Err para o que nao e convite, esta corrompido ou venceu.
pub fn ler_convite(texto: &str, agora: i64) -> Result<Value, String> {
    let Some(resto) = texto.trim().strip_prefix(PREFIXO) else { return Err("nao e um convite do BRANIAC LINK".into()) };
    let ruim = || "convite corrompido".to_string();
    let c: Value = serde_json::from_slice(&unb64(resto).map_err(|_| ruim())?).map_err(|_| ruim())?;
    let (Some(chave), Some(id), Some(_), Some(exp)) = (c["pub"].as_str(), c["id"].as_str(), c["seg"].as_str(), c["exp"].as_i64()) else { return Err(ruim()) };
    if id_de(chave).map_err(|_| ruim())? != id {
        return Err(ruim());
    }
    if exp < agora {
        return Err("convite vencido".into());
    }
    Ok(c)
}

/// pares.json: {pares: {id: {nome, pub, estado: pendente|confiavel|revogado, seg, perm}}, convites: {seg: {exp}}}.
pub struct Pares {
    pub arq: PathBuf,
}

impl Pares {
    pub fn new(pasta: &Path) -> Result<Pares, String> {
        std::fs::create_dir_all(pasta).map_err(|e| e.to_string())?;
        Ok(Pares { arq: pasta.join("pares.json") })
    }

    fn ler(&self) -> Value {
        let mut d: Value = std::fs::read_to_string(&self.arq).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
        for k in ["pares", "convites"] {
            if !d[k].is_object() {
                d[k] = json!({}); // arquivo estragado = ninguem e conhecido
            }
        }
        d
    }

    fn gravar(&self, d: &Value) -> Result<(), String> {
        std::fs::write(&self.arq, d.to_string()).map_err(|e| e.to_string())
    }

    // ---- convite ----
    /// Texto do convite para entregar ao outro dono (uso unico, vale 15 min). `segredo`: 16 bytes sorteados por quem chama.
    pub fn convidar(&self, ident: &Ident, agora: i64, segredo: &[u8; 16]) -> Result<String, String> {
        let seg = b64(segredo);
        let mut d = self.ler();
        if let Some(c) = d["convites"].as_object_mut() {
            c.retain(|_, v| v["exp"].as_i64().is_some_and(|e| e > agora));
            c.insert(seg.clone(), json!({ "exp": agora + VALIDADE_CONVITE_S }));
        }
        self.gravar(&d)?;
        let corpo = json!({ "v": 1, "id": ident.id, "pub": ident.chave_pub, "nome": ident.nome, "seg": seg, "exp": agora + VALIDADE_CONVITE_S });
        Ok(format!("{PREFIXO}{}", b64(corpo.to_string().as_bytes())))
    }

    /// Lado convidado: guarda o convidante como pendente. Devolve o convite lido (para conectar usando seg).
    pub fn aceitar_convite(&self, texto: &str, agora: i64) -> Result<Value, String> {
        let c = ler_convite(texto, agora)?;
        let s = |k: &str| c[k].as_str().unwrap_or("");
        self.salvar_par(s("id"), s("nome"), s("pub"), s("seg"), agora)?;
        let mut d = self.ler();
        d["pares"][s("id")]["conectar_ate"] = c["exp"].clone(); // tenta conectar sozinho ate o convite vencer
        self.gravar(&d)?;
        Ok(c)
    }

    /// Lado convidante: o convidado apareceu com a prova. O convite e queimado (uso unico) e o par entra como pendente.
    pub fn consumir_convite(&self, seg: &str, meu_id: &str, id_convidado: &str, pub_convidado: &str, nome: &str, prova_recebida: &str, agora: i64) -> Result<(), String> {
        let mut d = self.ler();
        if !d["convites"][seg]["exp"].as_i64().is_some_and(|e| e >= agora) {
            return Err("convite desconhecido, usado ou vencido".into());
        }
        let certa = prova(seg, meu_id, id_convidado);
        if id_de(pub_convidado).ok().as_deref() != Some(id_convidado) || !certa.is_ok_and(|p| iguais(p.as_bytes(), prova_recebida.as_bytes())) {
            return Err("prova do convite nao confere".into());
        }
        if let Some(c) = d["convites"].as_object_mut() {
            c.remove(seg);
        }
        self.gravar(&d)?;
        self.salvar_par(id_convidado, nome, pub_convidado, seg, agora)
    }

    // ---- pares ----
    /// Guarda ou atualiza um par SEM mudar o estado dele (novo entra pendente). Revogado nao volta.
    pub fn salvar_par(&self, pid: &str, nome: &str, chave_pub: &str, seg: &str, agora: i64) -> Result<(), String> {
        let mut d = self.ler();
        let atual = d["pares"][pid].clone();
        if atual["estado"] == "revogado" {
            return Err("dispositivo revogado".into());
        }
        let ou = |k: &str, padrao: Value| if atual[k].is_null() { padrao } else { atual[k].clone() };
        let mut novo = json!({ "nome": nome.chars().take(60).collect::<String>(), "pub": chave_pub, "seg": seg, "estado": ou("estado", json!("pendente")),
                               "perm": ou("perm", json!({})), "desde": ou("desde", json!(agora)) });
        if !atual["conectar_ate"].is_null() {
            novo["conectar_ate"] = atual["conectar_ate"].clone();
        }
        d["pares"][pid] = novo;
        self.gravar(&d)
    }

    pub fn marcar_conectado(&self, pid: &str) -> Result<(), String> {
        let mut d = self.ler();
        match d["pares"][pid].as_object_mut() {
            Some(p) => {
                p.remove("conectar_ate");
                self.gravar(&d)
            }
            None => Ok(()),
        }
    }

    pub fn get(&self, pid: &str) -> Option<Value> {
        self.ler()["pares"].get(pid).filter(|p| p.is_object()).cloned()
    }

    /// Todos os pares, sem o segredo do canal.
    pub fn listar(&self) -> Map<String, Value> {
        let mut pares = self.ler()["pares"].as_object().cloned().unwrap_or_default();
        for p in pares.values_mut() {
            if let Some(o) = p.as_object_mut() {
                o.remove("seg");
            }
        }
        pares
    }

    pub fn convites_abertos(&self, agora: i64) -> Map<String, Value> {
        let mut c = self.ler()["convites"].as_object().cloned().unwrap_or_default();
        c.retain(|_, v| v["exp"].as_i64().is_some_and(|e| e > agora));
        c
    }

    /// O dono conferiu o SAS com a outra pessoa.
    pub fn confirmar(&self, pid: &str) -> Result<(), String> {
        let mut d = self.ler();
        if !d["pares"][pid].is_object() || d["pares"][pid]["estado"] == "revogado" {
            return Err("par desconhecido ou revogado".into());
        }
        d["pares"][pid]["estado"] = json!("confiavel");
        self.gravar(&d)
    }

    pub fn revogar(&self, pid: &str) -> Result<(), String> {
        let mut d = self.ler();
        if !d["pares"][pid].is_object() {
            return Ok(());
        }
        d["pares"][pid]["estado"] = json!("revogado");
        d["pares"][pid]["seg"] = json!("");
        d["pares"][pid]["perm"] = json!({});
        self.gravar(&d)
    }

    // ---- permissoes ----
    pub fn permitir(&self, pid: &str, agente: &str, acao: &str, nivel: &str) -> Result<(), String> {
        if !ACOES.contains(&acao) || !NIVEIS.contains(&nivel) {
            return Err("acao ou nivel invalido".into());
        }
        let mut d = self.ler();
        if d["pares"][pid]["estado"] != "confiavel" {
            return Err("so par confiavel recebe permissoes".into());
        }
        if !d["pares"][pid]["perm"].is_object() {
            d["pares"][pid]["perm"] = json!({});
        }
        if !d["pares"][pid]["perm"][agente].is_object() {
            d["pares"][pid]["perm"][agente] = json!({});
        }
        d["pares"][pid]["perm"][agente][acao] = json!(nivel);
        self.gravar(&d)
    }

    /// aceitar | perguntar | recusar. Desconhecido, pendente ou revogado: recusar. `agente` "*" = qualquer.
    pub fn decidir(&self, pid: &str, acao: &str, agente: &str) -> &'static str {
        let Some(p) = self.get(pid).filter(|p| p["estado"] == "confiavel") else { return "recusar" };
        for a in [agente, "*"] {
            match &p["perm"][a][acao] {
                Value::Null => {}
                Value::String(s) if s.is_empty() => {}
                // nivel que nao existe gravado no arquivo nunca vira "aceitar"
                v => return NIVEIS.iter().find(|n| v.as_str() == Some(**n)).copied().unwrap_or("recusar"),
            }
        }
        if ACOES.contains(&acao) { padrao(acao) } else { "recusar" }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link_identidade::falso;

    fn pasta(nome: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("braniac-link_confianca-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    fn parear(base: &Path, a: &Ident, b: &Ident) -> (Pares, Pares) {
        let (pa, pb) = (Pares::new(&base.join("pa")).unwrap(), Pares::new(&base.join("pb")).unwrap());
        let conv = pa.convidar(a, 1000, &[7; 16]).unwrap();
        let lido = pb.aceitar_convite(&conv, 1001).unwrap();
        let seg = lido["seg"].as_str().unwrap();
        pa.consumir_convite(seg, &a.id, &b.id, &b.chave_pub, "B", &prova(seg, &a.id, &b.id).unwrap(), 1002).unwrap();
        (pa, pb)
    }

    #[test]
    fn convite_pareamento_sas_e_uso_unico() {
        let base = pasta("parear");
        let (a, b) = (falso::ident("A"), falso::ident("B"));
        let (pa, pb) = parear(&base, &a, &b);
        assert!(pa.get(&b.id).unwrap()["estado"] == "pendente" && pb.get(&a.id).unwrap()["estado"] == "pendente");
        assert_eq!(pb.get(&a.id).unwrap()["conectar_ate"], 1000 + VALIDADE_CONVITE_S);
        let codigo = sas(&a.chave_pub, &b.chave_pub).unwrap();
        assert!(codigo == sas(&b.chave_pub, &a.chave_pub).unwrap() && codigo.len() == 6 && codigo.chars().all(|c| c.is_ascii_digit()));
        assert_eq!(pa.decidir(&b.id, "msg", "*"), "recusar"); // pendente nao fala
        pa.confirmar(&b.id).unwrap();
        assert_eq!((pa.decidir(&b.id, "msg", "*"), pa.decidir(&b.id, "tarefa", "*"), pa.decidir(&b.id, "arquivo", "*")), ("aceitar", "recusar", "perguntar"));
        assert_eq!(pa.decidir(&b.id, "apagar", "*"), "recusar");
        assert_eq!(pa.decidir("bl1-desconhecido", "msg", "*"), "recusar"); // quem nao esta na lista nao e aceito
        let seg = pa.get(&b.id).unwrap()["seg"].as_str().unwrap().to_string();
        assert!(pa.consumir_convite(&seg, &a.id, &b.id, &b.chave_pub, "B", &prova(&seg, &a.id, &b.id).unwrap(), 1003).is_err()); // convite ja queimado
        assert!(pa.listar()[&b.id].get("seg").is_none() && pa.listar()[&b.id]["nome"] == "B");
        pb.marcar_conectado(&a.id).unwrap();
        assert!(pb.get(&a.id).unwrap().get("conectar_ate").is_none());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn convite_prova_errada_vencido_e_corrompido() {
        let base = pasta("convite");
        let (a, b) = (falso::ident("A"), falso::ident("B"));
        let pa = Pares::new(&base.join("pa")).unwrap();
        let conv = pa.convidar(&a, 1000, &[9; 16]).unwrap();
        let seg = ler_convite(&conv, 1000).unwrap()["seg"].as_str().unwrap().to_string();
        assert!(pa.consumir_convite(&seg, &a.id, &b.id, &b.chave_pub, "B", "errada", 1001).is_err());
        assert!(pa.consumir_convite(&seg, &a.id, &b.id, &a.chave_pub, "B", &prova(&seg, &a.id, &b.id).unwrap(), 1001).is_err()); // chave de outro
        assert!(pa.get(&b.id).is_none() && pa.convites_abertos(1001).len() == 1); // tentativa errada nao cria par
        assert!(pa.consumir_convite(&seg, &a.id, &b.id, &b.chave_pub, "B", &prova(&seg, &a.id, &b.id).unwrap(), 1000 + VALIDADE_CONVITE_S + 5).is_err());
        assert_eq!(ler_convite(&conv, 1000 + VALIDADE_CONVITE_S + 5).unwrap_err(), "convite vencido");
        assert!(pa.convites_abertos(1000 + VALIDADE_CONVITE_S + 5).is_empty());
        for ruim in ["BLINK1.lixo", "oi", "", &format!("{PREFIXO}{}", b64(b"{\"id\":\"bl1-x\",\"pub\":\"AAAA\",\"seg\":\"s\",\"exp\":9999}"))] {
            assert!(ler_convite(ruim, 1000).is_err(), "{ruim}");
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn permissao_por_agente_e_revogacao() {
        let base = pasta("perm");
        let (a, b) = (falso::ident("A"), falso::ident("B"));
        let (pa, _) = parear(&base, &a, &b);
        assert!(pa.permitir(&b.id, "dev", "tarefa", "aceitar").is_err()); // pendente nao recebe permissao
        pa.confirmar(&b.id).unwrap();
        assert!(pa.permitir(&b.id, "dev", "tarefa", "sempre").is_err() && pa.permitir(&b.id, "dev", "apagar", "aceitar").is_err());
        pa.permitir(&b.id, "dev", "tarefa", "perguntar").unwrap();
        assert_eq!((pa.decidir(&b.id, "tarefa", "dev"), pa.decidir(&b.id, "tarefa", "outro")), ("perguntar", "recusar"));
        pa.permitir(&b.id, "*", "tarefa", "aceitar").unwrap();
        assert_eq!((pa.decidir(&b.id, "tarefa", "dev"), pa.decidir(&b.id, "tarefa", "outro")), ("perguntar", "aceitar"));
        // nivel estranho escrito no arquivo nao vira permissao
        let mut d = pa.ler();
        d["pares"][&b.id]["perm"]["dev"]["msg"] = json!("talvez");
        pa.gravar(&d).unwrap();
        assert_eq!(pa.decidir(&b.id, "msg", "dev"), "recusar");
        pa.revogar(&b.id).unwrap();
        assert_eq!((pa.decidir(&b.id, "msg", "*"), pa.decidir(&b.id, "tarefa", "outro")), ("recusar", "recusar"));
        assert!(pa.get(&b.id).unwrap()["seg"] == "" && pa.confirmar(&b.id).is_err());
        assert!(pa.salvar_par(&b.id, "B", &b.chave_pub, "novo", 2000).is_err()); // revogado nao volta por convite novo
        assert!(pa.confirmar("bl1-ninguem").is_err());
        std::fs::write(&pa.arq, "lixo").unwrap();
        assert_eq!(pa.decidir(&b.id, "msg", "*"), "recusar");
        let _ = std::fs::remove_dir_all(&base);
    }
}
