//! Transferencia de arquivo em blocos, com retomada e verificacao (link/arquivos.py). Quadro binario: fid(8) | indice(4) | hash16 do bloco | dados.
//! O recebido fica na pasta de quarentena e NUNCA e aberto nem executado por aqui. O BLAKE2b esta escrito aqui (nao ha crate) para o
//! quadro bater com o do Python. Ficou de fora: sortear o fid (quem chama passa 16 hex) e o envio pelo canal (e do no).
#![allow(dead_code)]
use crate::link_fila::nome_seguro;
use crate::link_identidade::hex;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub const BLOCO: u64 = 16 * 1024;
pub const MAX_TAMANHO: u64 = 2 * 1024 * 1024 * 1024;
pub const CAB: usize = 8 + 4 + 16;
pub const PERIGOSOS: [&str; 13] = [".exe", ".bat", ".cmd", ".ps1", ".vbs", ".js", ".msi", ".scr", ".lnk", ".com", ".sh", ".jar", ".dll"];

const IV: [u64; 8] = [0x6a09e667f3bcc908, 0xbb67ae8584caa73b, 0x3c6ef372fe94f82b, 0xa54ff53a5f1d36f1, 0x510e527fade682d1, 0x9b05688c2b3e6c1f, 0x1f83d9abfb41bd6b, 0x5be0cd19137e2179];
const SIGMA: [[usize; 16]; 10] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
    [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
    [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
    [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
    [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
    [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
    [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10],
    [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
    [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
];

/// BLAKE2b sem chave, com saida de `n` bytes (1 a 64), aos pedacos (arquivo grande nao cabe na memoria).
pub struct Blake2b {
    h: [u64; 8],
    t: u128,
    buf: [u8; 128],
    cheio: usize,
    n: usize,
}

impl Blake2b {
    pub fn new(n: usize) -> Blake2b {
        let n = n.clamp(1, 64);
        let mut h = IV;
        h[0] ^= 0x0101_0000 ^ n as u64;
        Blake2b { h, t: 0, buf: [0; 128], cheio: 0, n }
    }

    fn comprimir(&mut self, ultimo: bool) {
        let mut m = [0u64; 16];
        for (i, p) in self.buf.chunks(8).enumerate() {
            m[i] = u64::from_le_bytes([p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7]]);
        }
        let mut v = [0u64; 16];
        v[..8].copy_from_slice(&self.h);
        v[8..].copy_from_slice(&IV);
        v[12] ^= self.t as u64;
        v[13] ^= (self.t >> 64) as u64;
        if ultimo {
            v[14] = !v[14];
        }
        for r in 0..12 {
            let s = &SIGMA[r % 10];
            for (i, [a, b, c, d]) in [[0, 4, 8, 12], [1, 5, 9, 13], [2, 6, 10, 14], [3, 7, 11, 15], [0, 5, 10, 15], [1, 6, 11, 12], [2, 7, 8, 13], [3, 4, 9, 14]].into_iter().enumerate() {
                v[a] = v[a].wrapping_add(v[b]).wrapping_add(m[s[2 * i]]);
                v[d] = (v[d] ^ v[a]).rotate_right(32);
                v[c] = v[c].wrapping_add(v[d]);
                v[b] = (v[b] ^ v[c]).rotate_right(24);
                v[a] = v[a].wrapping_add(v[b]).wrapping_add(m[s[2 * i + 1]]);
                v[d] = (v[d] ^ v[a]).rotate_right(16);
                v[c] = v[c].wrapping_add(v[d]);
                v[b] = (v[b] ^ v[c]).rotate_right(63);
            }
        }
        for i in 0..8 {
            self.h[i] ^= v[i] ^ v[i + 8];
        }
    }

    pub fn update(&mut self, mut dados: &[u8]) {
        while !dados.is_empty() {
            if self.cheio == 128 {
                // so comprime quando sabe que vem mais: o ultimo bloco leva a marca de fim
                self.t += 128;
                self.comprimir(false);
                self.cheio = 0;
            }
            let k = (128 - self.cheio).min(dados.len());
            self.buf[self.cheio..self.cheio + k].copy_from_slice(&dados[..k]);
            self.cheio += k;
            dados = &dados[k..];
        }
    }

    pub fn fim(mut self) -> Vec<u8> {
        self.t += self.cheio as u128;
        self.buf[self.cheio..].fill(0);
        self.comprimir(true);
        self.h.iter().flat_map(|x| x.to_le_bytes()).take(self.n).collect()
    }
}

pub fn blake2b(dados: &[u8], n: usize) -> Vec<u8> {
    let mut h = Blake2b::new(n);
    h.update(dados);
    h.fim()
}

fn hash_do_arquivo(caminho: &Path) -> Result<String, String> {
    let mut f = fs::File::open(caminho).map_err(|e| e.to_string())?;
    let (mut h, mut parte) = (Blake2b::new(32), vec![0u8; 1024 * 1024]);
    loop {
        let k = f.read(&mut parte).map_err(|e| e.to_string())?;
        if k == 0 {
            return Ok(hex(&h.fim()));
        }
        h.update(&parte[..k]);
    }
}

fn n_blocos(tam: u64, bloco: u64) -> u64 {
    tam.div_ceil(bloco).max(1)
}

fn hex16(s: &str, n: usize) -> bool {
    s.len() == n && s.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

pub struct Envio {
    pub caminho: PathBuf,
    pub bloco: u64,
    pub tam: u64,
    pub hash: String,
    pub fid: String,
    pub blocos: u64,
}

impl Envio {
    /// `fid`: 16 hex sorteados por quem chama (identifica esta transferencia).
    pub fn new(caminho: &Path, bloco: u64, fid: &str) -> Result<Envio, String> {
        if !hex16(fid, 16) || bloco == 0 {
            return Err("fid ou bloco invalido".into());
        }
        let tam = fs::metadata(caminho).map_err(|e| e.to_string())?.len();
        if tam > MAX_TAMANHO {
            return Err("arquivo maior que o limite".into());
        }
        Ok(Envio { caminho: caminho.to_path_buf(), bloco, tam, hash: hash_do_arquivo(caminho)?, fid: fid.to_string(), blocos: n_blocos(tam, bloco) })
    }

    pub fn oferta(&self) -> Value {
        let nome = self.caminho.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        json!({ "fid": self.fid, "nome": nome_seguro(&nome), "tam": self.tam, "bloco": self.bloco, "blocos": self.blocos, "hash": self.hash })
    }

    pub fn quadro(&self, i: u32) -> Result<Vec<u8>, String> {
        let mut f = fs::File::open(&self.caminho).map_err(|e| e.to_string())?;
        f.seek(SeekFrom::Start(i as u64 * self.bloco)).map_err(|e| e.to_string())?;
        let mut dados = Vec::new();
        f.take(self.bloco).read_to_end(&mut dados).map_err(|e| e.to_string())?;
        let mut q: Vec<u8> = (0..8).map(|k| u8::from_str_radix(&self.fid[2 * k..2 * k + 2], 16).unwrap_or(0)).collect();
        q.extend_from_slice(&i.to_be_bytes());
        q.extend_from_slice(&blake2b(&dados, 16));
        q.extend_from_slice(&dados);
        Ok(q)
    }
}

/// pasta/<fid>.parcial + <fid>.estado.json (indices ja recebidos): se a conexao cair, criar de novo com a mesma oferta e pedir `faltando()`.
pub struct Recebimento {
    pub fid: String,
    pub tam: u64,
    pub bloco: u64,
    pub blocos: u64,
    pub hash: String,
    pub nome: String,
    pasta: PathBuf,
    parcial: PathBuf,
    estado: PathBuf,
    tenho: BTreeSet<u64>,
}

fn inteiro(v: &Value) -> Option<u64> {
    v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

impl Recebimento {
    pub fn new(pasta: &Path, oferta: &Value) -> Result<Recebimento, String> {
        let ruim = || "oferta invalida".to_string();
        let texto = |k: &str| oferta[k].as_str().map(str::to_string).ok_or_else(ruim);
        let (fid, hash, nome) = (texto("fid")?, texto("hash")?, nome_seguro(&texto("nome")?));
        let (tam, bloco, blocos) = (inteiro(&oferta["tam"]).ok_or_else(ruim)?, inteiro(&oferta["bloco"]).ok_or_else(ruim)?, inteiro(&oferta["blocos"]).ok_or_else(ruim)?);
        if !hex16(&fid, 16) || tam > MAX_TAMANHO || !(1024..=1024 * 1024).contains(&bloco) || blocos != n_blocos(tam, bloco) || !hex16(&hash, 64) {
            return Err("oferta com numeros invalidos".into());
        }
        fs::create_dir_all(pasta).map_err(|e| e.to_string())?;
        let (parcial, estado) = (pasta.join(format!("{fid}.parcial")), pasta.join(format!("{fid}.estado.json")));
        let mut tenho: BTreeSet<u64> = fs::read_to_string(&estado)
            .ok()
            .and_then(|t| serde_json::from_str::<Vec<u64>>(&t).ok())
            .map(|v| v.into_iter().filter(|i| *i < blocos).collect())
            .unwrap_or_default();
        if !parcial.exists() {
            fs::write(&parcial, b"").map_err(|e| e.to_string())?;
            tenho.clear();
        }
        Ok(Recebimento { fid, tam, bloco, blocos, hash, nome, pasta: pasta.to_path_buf(), parcial, estado, tenho })
    }

    /// Grava um bloco conferido (fid, tamanho e hash do bloco). Devolve o indice.
    pub fn receber(&mut self, quadro: &[u8]) -> Result<u64, String> {
        if quadro.len() < CAB || hex(&quadro[..8]) != self.fid {
            return Err("quadro de outro arquivo".into());
        }
        let i = u32::from_be_bytes([quadro[8], quadro[9], quadro[10], quadro[11]]) as u64;
        let dados = &quadro[CAB..];
        if i >= self.blocos {
            return Err(format!("bloco {i} corrompido ou fora do esperado"));
        }
        let esperado = if i < self.blocos - 1 { self.bloco } else { self.tam - self.bloco * (self.blocos - 1) };
        if dados.len() as u64 != esperado || blake2b(dados, 16) != quadro[12..CAB] {
            return Err(format!("bloco {i} corrompido ou fora do esperado"));
        }
        let mut f = fs::OpenOptions::new().write(true).open(&self.parcial).map_err(|e| e.to_string())?;
        f.seek(SeekFrom::Start(i * self.bloco)).map_err(|e| e.to_string())?;
        f.write_all(dados).map_err(|e| e.to_string())?;
        self.tenho.insert(i);
        if self.tenho.len() % 64 == 0 || self.tenho.len() as u64 == self.blocos {
            fs::write(&self.estado, json!(self.tenho).to_string()).map_err(|e| e.to_string())?;
        }
        Ok(i)
    }

    pub fn faltando(&self) -> Vec<u64> {
        (0..self.blocos).filter(|i| !self.tenho.contains(i)).collect()
    }

    /// Confere o hash do arquivo inteiro e solta o nome final (sem sobrescrever). Hash errado: descarta tudo.
    pub fn finalizar(&mut self) -> Result<PathBuf, String> {
        if !self.faltando().is_empty() {
            return Err("ainda faltam blocos".into());
        }
        if hash_do_arquivo(&self.parcial)? != self.hash {
            let _ = fs::remove_file(&self.parcial);
            let _ = fs::remove_file(&self.estado);
            self.tenho.clear();
            return Err("hash final nao confere: arquivo descartado".into());
        }
        let (raiz, ext) = match self.nome.rfind('.').filter(|i| *i > 0) {
            Some(i) => (&self.nome[..i], &self.nome[i..]),
            None => (self.nome.as_str(), ""),
        };
        let (mut final_, mut n) = (self.nome.clone(), 1);
        while self.pasta.join(&final_).exists() {
            final_ = format!("{raiz}-{n}{ext}");
            n += 1;
        }
        if PERIGOSOS.contains(&ext.to_lowercase().as_str()) {
            final_.push_str(".recebido");
        }
        let destino = self.pasta.join(final_);
        fs::rename(&self.parcial, &destino).map_err(|e| e.to_string())?;
        let _ = fs::remove_file(&self.estado);
        Ok(destino)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(nome: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("braniac-link_arquivos-{}-{nome}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn origem(p: &Path, nome: &str, tam: usize) -> PathBuf {
        let f = p.join(nome);
        fs::write(&f, (0..tam).map(|i| (i * 7 % 251) as u8).collect::<Vec<u8>>()).unwrap();
        f
    }

    #[test]
    fn blake2b_bate_com_o_python() {
        assert_eq!(hex(&blake2b(b"", 16)), "cae66941d9efbd404e4d88758ea67670");
        assert_eq!(hex(&blake2b(b"abc", 16)), "cf4ab791c62b8d2b2109c90275287816");
        assert_eq!(hex(&blake2b(b"abc", 32)), "bddd813c634239723171ef3fee98579b94964e3bb1cb3e427262c8c068d52319");
        assert_eq!(hex(&blake2b(&[b'a'; 300], 32)), "3c1292de00a518e36823f9ff908ac2da46be38718c018713403461df077e15f6");
        let mut h = Blake2b::new(16); // aos pedacos da o mesmo, inclusive com bloco de 128 exato
        h.update(&[b'a'; 128]);
        h.update(&[b'a'; 172]);
        assert_eq!(hex(&h.fim()), "ccc6f168e83a9fcfde99f7d9ddeeae49");
    }

    #[test]
    fn envia_recebe_e_retoma_depois_da_queda() {
        let p = tmp("retoma");
        let env = Envio::new(&origem(&p, "foto.png", 5000), 1024, "00112233aabbccdd").unwrap();
        assert_eq!(env.blocos, 5);
        let pasta = p.join("quarentena");
        let mut rec = Recebimento::new(&pasta, &env.oferta()).unwrap();
        for i in [0, 3] {
            rec.receber(&env.quadro(i).unwrap()).unwrap();
        }
        fs::write(pasta.join("00112233aabbccdd.estado.json"), "[0,3]").unwrap(); // o estado so e gravado de 64 em 64: simula a gravacao
        let mut rec = Recebimento::new(&pasta, &env.oferta()).unwrap(); // conexao caiu: mesma oferta de novo
        assert_eq!(rec.faltando(), vec![1, 2, 4]);
        assert!(rec.finalizar().unwrap_err().contains("faltam"));
        for i in rec.faltando() {
            rec.receber(&env.quadro(i as u32).unwrap()).unwrap();
        }
        let destino = rec.finalizar().unwrap();
        assert_eq!(destino, pasta.join("foto.png"));
        assert_eq!(fs::read(&destino).unwrap(), fs::read(p.join("foto.png")).unwrap());
        assert!(!pasta.join("00112233aabbccdd.estado.json").exists() && !pasta.join("00112233aabbccdd.parcial").exists());
        // o mesmo nome de novo nao sobrescreve
        let mut rec = Recebimento::new(&pasta, &env.oferta()).unwrap();
        for i in 0..5 {
            rec.receber(&env.quadro(i).unwrap()).unwrap();
        }
        assert_eq!(rec.finalizar().unwrap(), pasta.join("foto-1.png"));
        let _ = fs::remove_dir_all(p);
    }

    #[test]
    fn bloco_corrompido_de_outro_arquivo_ou_fora_do_tamanho_e_recusado() {
        let p = tmp("corrompido");
        let env = Envio::new(&origem(&p, "a.txt", 2500), 1024, "0000000000000001").unwrap();
        let mut rec = Recebimento::new(&p.join("q"), &env.oferta()).unwrap();
        let mut q = env.quadro(1).unwrap();
        q[CAB + 5] ^= 1;
        assert!(rec.receber(&q).unwrap_err().contains("bloco 1 corrompido"));
        let mut q = env.quadro(0).unwrap();
        q[0] = 9;
        assert_eq!(rec.receber(&q).unwrap_err(), "quadro de outro arquivo");
        let mut q = env.quadro(0).unwrap();
        q[11] = 7; // indice que nao existe
        assert!(rec.receber(&q).is_err());
        assert!(rec.receber(&env.quadro(2).unwrap()[..CAB + 10]).is_err()); // ultimo bloco cortado
        assert!(rec.receber(&[1, 2, 3]).is_err());
        assert_eq!(rec.faltando(), vec![0, 1, 2]);
        let _ = fs::remove_dir_all(p);
    }

    #[test]
    fn hash_final_errado_descarta_e_oferta_ruim_nao_entra() {
        let p = tmp("hash");
        let env = Envio::new(&origem(&p, "virus.EXE", 100), 1024, "0000000000000002").unwrap();
        let mut o = env.oferta();
        o["hash"] = json!("0".repeat(64));
        let mut rec = Recebimento::new(&p.join("q"), &o).unwrap();
        rec.receber(&env.quadro(0).unwrap()).unwrap();
        assert!(rec.finalizar().unwrap_err().contains("descartado"));
        assert!(!p.join("q").join("0000000000000002.parcial").exists());
        // executavel fica com .recebido no fim
        let mut rec = Recebimento::new(&p.join("q"), &env.oferta()).unwrap();
        rec.receber(&env.quadro(0).unwrap()).unwrap();
        assert_eq!(rec.finalizar().unwrap(), p.join("q").join("virus.EXE.recebido"));
        for (k, v) in [("fid", json!("XYZ")), ("bloco", json!(10)), ("blocos", json!(3)), ("tam", json!(MAX_TAMANHO + 1)), ("hash", json!("abc")), ("nome", json!(5))] {
            let mut o = env.oferta();
            o[k] = v;
            assert!(Recebimento::new(&p.join("q"), &o).is_err(), "{k}");
        }
        let mut o = env.oferta();
        o["nome"] = json!("..\\..\\fora.txt");
        assert_eq!(Recebimento::new(&p.join("q"), &o).unwrap().nome, "fora.txt");
        // arquivo vazio: um bloco de tamanho zero
        let vazio = Envio::new(&origem(&p, "vazio.txt", 0), 1024, "0000000000000003").unwrap();
        let mut rec = Recebimento::new(&p.join("q"), &vazio.oferta()).unwrap();
        rec.receber(&vazio.quadro(0).unwrap()).unwrap();
        assert!(rec.finalizar().is_ok());
        let _ = fs::remove_dir_all(p);
    }
}
