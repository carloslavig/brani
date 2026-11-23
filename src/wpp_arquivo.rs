//! Enviar ARQUIVO pelo WhatsApp: as REGRAS (porte de braniac/wpp_arquivo.py sem os scripts de pagina).
//! - o que pode sair: qualquer arquivo que o dono pedir, MENOS o que guarda segredo ou dado pessoal (chaves, cookies, bancos,
//!   perfil do navegador, AppData, legado, pastas do sistema e a pasta de dados do proprio Braniac);
//! - tamanho maximo (100 MB); tipo que o WhatsApp bloqueia (executavel, script) vira .zip antes, nunca vai cru;
//! - pre-visualizacao travada: recarrega a pagina UMA vez e tenta de novo;
//! - texto igual mandado ha menos de 20 min na mesma conversa nao e repetido.
//! Ficou de fora: anexar/clicar/esperar o envio na pagina (closure `enviar`), e a expansao de variaveis de ambiente no caminho.
//! Diferencas do Python: pastas e atalhos vem em `Pastas` (nada de caminho da pessoa no codigo); o .zip sai sem compressao
//! (nao ha crate de zip), o que continua sendo um .zip valido.
#![allow(dead_code)]
use crate::contexto::re;
use crate::tempo::{Agora, Data};
use crate::wpp_voz::um_por_vez;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// conservador: upload longo demais trava a pagina unica
pub const LIMITE_MB: u64 = 100;
pub const BLOQUEADAS: [&str; 13] = [".exe", ".bat", ".cmd", ".msi", ".scr", ".com", ".vbs", ".js", ".jar", ".ps1", ".dll", ".lnk", ".apk"];
pub const ESPERA_ENVIO_S: u64 = 240;
pub const PREVIA_NAO_ABRIU: &str = "A pré-visualização do arquivo não abriu (o botão Enviar não apareceu). Nada foi enviado.";
pub const JANELA_DUPLICADA_MIN: i64 = 20;
const BLOQUEIO: [&str; 21] = [".env", ".pem", ".key", ".pfx", ".kdbx", "id_rsa", ".ssh", ".db", ".sqlite", "cookies", "login data", "cofre", "chromeagente", "appdata",
    "bravoseguranca", "\\legado", "\\gustavo", "\\braniac\\data\\", "\\windows\\", "\\program files", "\\programdata"];
const LIBERADO_DENTRO_DE_DATA: [&str; 2] = ["\\braniac\\data\\imagens", "\\braniac\\data\\cortes\\saida"];
/// dentro de uma pasta protegida, so estas subpastas podem sair (imagens geradas e cortes prontos)
const LIBERADO_NA_PROTEGIDA: [&str; 2] = ["imagens\\", "cortes\\saida\\"];
const IGNORAR_NA_BUSCA: [&str; 7] = [".git", "node_modules", "target", "__pycache__", ".pytest_cache", "pyi", "binaries"];

/// Onde procurar e o que nunca sai. Tudo vem de quem integra (configuracao), nada fica no codigo.
#[derive(Default, Clone)]
pub struct Pastas {
    /// palavra -> arquivo (ex.: o instalador mais recente)
    pub atalhos: Vec<(String, PathBuf)>,
    /// pastas de busca, as mais importantes primeiro (as 4 primeiras tem prioridade na ordem do resultado)
    pub raizes: Vec<PathBuf>,
    /// pastas de dados do proprio sistema: nada de dentro delas e enviado (fora imagens e cortes prontos)
    pub protegidas: Vec<PathBuf>,
}

fn baixo(p: &Path) -> String {
    let s = p.to_string_lossy().to_lowercase().replace('/', "\\");
    format!("{}\\", s.strip_prefix("\\\\?\\").unwrap_or(&s).trim_end_matches('\\'))
}

/// TRAVA: o arquivo guarda segredo ou dado pessoal? (confere o caminho inteiro, sem abrir o arquivo)
pub fn bloqueado(p: &Path, protegidas: &[PathBuf]) -> bool {
    let b = baixo(p);
    for d in protegidas {
        if let Some(resto) = b.strip_prefix(&baixo(d)) {
            return !LIBERADO_NA_PROTEGIDA.iter().any(|l| resto.starts_with(l));
        }
    }
    !LIBERADO_DENTRO_DE_DATA.iter().any(|m| b.contains(m)) && BLOQUEIO.iter().any(|x| b.contains(x))
}

re!(tokens, r"[\wÀ-ÿ.]+");

/// Procura arquivos pelo nome (todas as palavras) nas `raizes`, na ordem, por no maximo `tempo`. Mais relevantes primeiro:
/// nome exato, depois pastas de prioridade, depois o mais recente. Nunca entra em pasta bloqueada.
pub fn buscar(consulta: &str, pastas: &Pastas, limite: usize, tempo: Duration) -> Vec<PathBuf> {
    let c = consulta.to_lowercase();
    let toks: Vec<&str> = tokens().find_iter(&c).map(|m| m.as_str()).filter(|t| t.chars().count() > 1).collect();
    if toks.is_empty() {
        return vec![];
    }
    let fim = Instant::now() + tempo;
    let mut achados: Vec<PathBuf> = Vec::new();
    for (i, raiz) in pastas.raizes.iter().enumerate() {
        let mut fila = vec![raiz.clone()];
        while let Some(dir) = fila.pop() {
            let Ok(itens) = std::fs::read_dir(&dir) else { continue };
            for e in itens.flatten() {
                let (p, nome) = (e.path(), e.file_name().to_string_lossy().to_lowercase());
                if p.is_dir() {
                    if !IGNORAR_NA_BUSCA.contains(&nome.as_str()) && !bloqueado(&p, &pastas.protegidas) {
                        fila.push(p);
                    }
                } else if toks.iter().all(|t| nome.contains(t)) && !achados.contains(&p) && !bloqueado(&p, &pastas.protegidas) {
                    achados.push(p);
                }
            }
            if Instant::now() > fim || achados.len() >= limite * 4 {
                break;
            }
        }
        if (!achados.is_empty() && Instant::now() > fim) || (achados.len() >= limite && i < 4) {
            break;
        }
    }
    let prioridade: Vec<String> = pastas.raizes.iter().take(4).map(|r| baixo(r)).collect();
    let chave = |p: &PathBuf| {
        let exato = p.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase() == c.trim());
        let quando = p.metadata().and_then(|m| m.modified()).ok();
        (!exato, !prioridade.iter().any(|r| baixo(p).starts_with(r.as_str())), std::cmp::Reverse(quando))
    };
    achados.sort_by_key(chave);
    achados.truncate(limite);
    achados
}

/// Atalho, caminho completo ou parte do nome (acha o mais recente, pastas de prioridade primeiro) -> arquivo que PODE ser enviado.
/// Err = a frase do motivo (nao achou, ou area protegida).
pub fn resolver_caminho(texto: &str, pastas: &Pastas) -> Result<PathBuf, String> {
    let t = texto.trim().trim_matches('"');
    if t.is_empty() {
        return Err("Faltou dizer qual arquivo.".into());
    }
    let p = match pastas.atalhos.iter().find(|(k, _)| k.to_lowercase() == t.to_lowercase()) {
        Some((_, a)) => a.clone(),
        None => {
            let (cand, tem_barra) = (PathBuf::from(t), t.contains('\\') || t.contains('/'));
            if (cand.is_absolute() || tem_barra) && (cand.exists() || cand.is_absolute()) {
                cand
            } else if tem_barra {
                // a IA devolve caminho relativo ("pasta/arquivo.pdf"): procura pelo NOME do arquivo
                let nome = t.replace('\\', "/").rsplit('/').next().unwrap_or("").to_string();
                buscar(&nome, pastas, 1, Duration::from_secs(6)).into_iter().next().ok_or(format!("Não achei '{t}' no PC."))?
            } else {
                buscar(t, pastas, 1, Duration::from_secs(6)).into_iter().next().ok_or(format!("Não achei nenhum arquivo com '{t}' no nome."))?
            }
        }
    };
    // o caminho de verdade (atalho de pasta e ".." resolvidos) e que passa pela trava
    let p = std::fs::canonicalize(&p).map(|c| PathBuf::from(c.to_string_lossy().strip_prefix("\\\\?\\").map(str::to_string).unwrap_or_else(|| c.to_string_lossy().into_owned()))).unwrap_or(p);
    if bloqueado(&p, &pastas.protegidas) {
        return Err("Esse arquivo guarda segredos ou dados pessoais protegidos (chaves, bancos, legado, sistema): não envio.".into());
    }
    if !p.is_file() {
        return Err(format!("Arquivo não encontrado: {}", p.display()));
    }
    Ok(p)
}

fn extensao(p: &Path) -> String {
    p.extension().map(|e| format!(".{}", e.to_string_lossy().to_lowercase())).unwrap_or_default()
}

pub fn crc32(dados: &[u8]) -> u32 {
    let mut tabela = [0u32; 256];
    for (i, t) in tabela.iter_mut().enumerate() {
        *t = (0..8).fold(i as u32, |c, _| if c & 1 == 1 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 });
    }
    !dados.iter().fold(!0u32, |c, b| tabela[((c ^ *b as u32) & 0xFF) as usize] ^ (c >> 8))
}

/// Um .zip com UM arquivo, sem compressao.
fn zip_de_um(nome: &str, dados: &[u8]) -> Vec<u8> {
    let (crc, tam, n) = (crc32(dados), dados.len() as u32, nome.len() as u16);
    let comum = |v: &mut Vec<u8>| {
        v.extend(20u16.to_le_bytes()); // versao necessaria
        v.extend(0x0800u16.to_le_bytes()); // nome em UTF-8
        v.extend([0u8; 2]); // sem compressao
        v.extend(0u16.to_le_bytes()); // hora
        v.extend(0x21u16.to_le_bytes()); // data: 01/01/1980
        v.extend(crc.to_le_bytes());
        v.extend(tam.to_le_bytes());
        v.extend(tam.to_le_bytes());
        v.extend(n.to_le_bytes());
        v.extend(0u16.to_le_bytes()); // sem campo extra
    };
    let mut v = Vec::with_capacity(dados.len() + 2 * nome.len() + 98);
    v.extend(0x0403_4b50u32.to_le_bytes());
    comum(&mut v);
    v.extend(nome.as_bytes());
    v.extend(dados);
    let central = v.len() as u32;
    v.extend(0x0201_4b50u32.to_le_bytes());
    v.extend(20u16.to_le_bytes());
    comum(&mut v);
    v.extend([0u8; 10]); // comentario, disco, atributos
    v.extend(0u32.to_le_bytes()); // onde comeca o arquivo
    v.extend(nome.as_bytes());
    let tam_central = v.len() as u32 - central;
    v.extend(0x0605_4b50u32.to_le_bytes());
    v.extend([0u8; 4]);
    v.extend(1u16.to_le_bytes());
    v.extend(1u16.to_le_bytes());
    v.extend(tam_central.to_le_bytes());
    v.extend(central.to_le_bytes());
    v.extend(0u16.to_le_bytes());
    v
}

fn grande_demais(bytes: u64) -> Option<String> {
    let mb = bytes as f64 / 1_048_576.0;
    (mb > LIMITE_MB as f64).then(|| format!("Arquivo grande demais ({mb:.0} MB; o limite aqui é {LIMITE_MB} MB)."))
}

/// Se o arquivo e de um tipo que o WhatsApp bloqueia, devolve o caminho de um .zip com ele (em `destino`); senao o proprio caminho.
pub fn compactar(caminho: &Path, destino: &Path) -> Result<PathBuf, String> {
    if !caminho.is_file() {
        return Err(format!("Arquivo não encontrado: {}", caminho.display()));
    }
    if !BLOQUEADAS.contains(&extensao(caminho).as_str()) {
        return Ok(caminho.to_path_buf());
    }
    if let Some(m) = caminho.metadata().ok().and_then(|m| grande_demais(m.len())) {
        return Err(m);
    }
    let nome = caminho.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let dados = std::fs::read(caminho).map_err(|e| format!("Não consegui ler o arquivo: {e}"))?;
    let zip = destino.join(format!("{}.zip", caminho.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
    std::fs::write(&zip, zip_de_um(&nome, &dados)).map_err(|e| format!("Não consegui criar o .zip: {e}"))?;
    Ok(zip)
}

/// Pode ir? Existe, cabe no limite e nao e de um tipo bloqueado.
pub fn validar(caminho: &Path) -> Result<(), String> {
    let m = caminho.metadata().ok().filter(|m| m.is_file()).ok_or(format!("Arquivo não encontrado: {}", caminho.display()))?;
    if let Some(e) = grande_demais(m.len()) {
        return Err(e);
    }
    if BLOQUEADAS.contains(&extensao(caminho).as_str()) {
        return Err("O WhatsApp bloqueia esse tipo de arquivo: compacte em .zip antes.".into());
    }
    Ok(())
}

/// Envia `caminho` como documento. Executavel vira .zip sozinho. `enviar(arquivo, legenda)` faz o envio na pagina (e confere o
/// destino); `recarregar` recarrega o WhatsApp. Devolve o texto do resultado (comeca com ✅ se saiu). Um envio por vez.
pub fn enviar_arquivo(caminho: &Path, legenda: &str, tmp: &Path, enviar: &dyn Fn(&Path, &str) -> String, recarregar: &dyn Fn()) -> String {
    let arq = match compactar(caminho, tmp).and_then(|z| validar(&z).map(|_| z)) {
        Ok(a) => a,
        Err(m) => return m,
    };
    um_por_vez(|| {
        let r = enviar(&arq, legenda);
        if r != PREVIA_NAO_ABRIU {
            return r;
        }
        // a aba travou e descartava todo anexo ate recarregar: UMA recarga por envio
        recarregar();
        enviar(&arq, legenda)
    })
}

re!(carimbo, r"\[(\d{1,2}):(\d{2}),\s*(\d{1,2})/(\d{1,2})/(\d{4})\]");

fn norm_msg(t: &str) -> String {
    t.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Uma mensagem NOSSA igual (ou a mesma com/sem a nota do final) saiu nos ultimos 20 min nessa conversa?
/// `recentes` = (cabecalho "[HH:MM, DD/MM/AAAA] Nome:", texto) das nossas ultimas mensagens. Texto curto (<12) nunca conta.
pub fn duplicada(recentes: &[(String, String)], texto: &str, agora: &Agora) -> bool {
    let novo = norm_msg(texto);
    if novo.chars().count() < 12 {
        return false;
    }
    let seg_agora = agora.data.0 * 86_400 + agora.segundos();
    recentes.iter().any(|(h, t)| {
        let Some(c) = carimbo().captures(h) else { return false };
        let n = |i: usize| c[i].parse::<i64>().unwrap_or(-1);
        let Some(dia) = Data::civil(n(5), n(4), n(3)).filter(|_| (0..24).contains(&n(1)) && (0..60).contains(&n(2))) else { return false };
        let passou = seg_agora - (dia.0 * 86_400 + n(1) * 3600 + n(2) * 60);
        let velho = norm_msg(t);
        (0..=JANELA_DUPLICADA_MIN * 60).contains(&passou) && !velho.is_empty() && (velho == novo || velho.starts_with(&novo) || novo.starts_with(&velho))
    })
}

/// O que responder quando a mensagem ja estava la: conta como enviada, sem repetir.
pub fn ja_enviada(alvo: &str) -> String {
    format!("✅ Mensagem enviada para {alvo}. (ja estava enviada ha instantes: nao repeti)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn pasta(n: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("braniac-wpp_arquivo-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn executavel_vira_zip_e_arquivo_comum_nao_e_alterado() {
        let d = pasta("zip");
        let exe = d.join("Instalador.exe");
        std::fs::write(&exe, [b"MZ".as_slice(), &[0u8; 100]].concat()).unwrap();
        let z = compactar(&exe, &d).unwrap();
        assert!(z.ends_with("Instalador.zip"));
        let b = std::fs::read(&z).unwrap();
        assert!(b.starts_with(b"PK\x03\x04") && &b[30..44] == b"Instalador.exe" && &b[44..46] == b"MZ" && b[b.len() - 22..].starts_with(b"PK\x05\x06"));
        assert_eq!(u32::from_le_bytes(b[18..22].try_into().unwrap()), 102); // tamanho guardado
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        let foto = d.join("foto.png");
        std::fs::write(&foto, b"x").unwrap();
        assert_eq!(compactar(&foto, &d).unwrap(), foto);
        assert!(validar(&foto).is_ok() && validar(&z).is_ok());
        assert!(validar(&exe).unwrap_err().contains("bloqueia")); // executavel nunca vai cru
        assert!(compactar(&d.join("nao_existe.zip"), &d).unwrap_err().contains("não encontrado"));
        assert!(grande_demais(101 * 1_048_576).unwrap().contains("grande demais") && grande_demais(100 * 1_048_576).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn envio_usa_o_zip_e_nada_sai_se_o_arquivo_nao_serve() {
        let d = pasta("envio");
        let exe = d.join("Setup.exe");
        std::fs::write(&exe, b"MZ").unwrap();
        let visto = RefCell::new(Vec::<String>::new());
        let enviar = |p: &Path, l: &str| { visto.borrow_mut().push(format!("{}|{l}", p.file_name().unwrap().to_string_lossy())); "✅ ok".to_string() };
        assert_eq!(enviar_arquivo(&exe, "oi", &d, &enviar, &|| panic!("sem recarga")), "✅ ok");
        assert_eq!(*visto.borrow(), ["Setup.zip|oi"]);
        let nunca = |_: &Path, _: &str| -> String { panic!("nao deveria abrir o WhatsApp") };
        assert!(enviar_arquivo(&d.join("nao_existe.zip"), "", &d, &nunca, &|| ()).contains("não encontrado"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn previa_travada_recarrega_e_tenta_uma_vez() {
        let d = pasta("previa");
        let pdf = d.join("guia.pdf");
        std::fs::write(&pdf, b"%PDF").unwrap();
        let respostas = RefCell::new(vec![PREVIA_NAO_ABRIU.to_string(), "✅ ok".to_string()]);
        let recargas = RefCell::new(0);
        let enviar = |_: &Path, _: &str| respostas.borrow_mut().remove(0);
        let recarregar = || *recargas.borrow_mut() += 1;
        assert_eq!(enviar_arquivo(&pdf, "", &d, &enviar, &recarregar), "✅ ok");
        assert_eq!(*recargas.borrow(), 1);
        *respostas.borrow_mut() = vec![PREVIA_NAO_ABRIU.to_string(), PREVIA_NAO_ABRIU.to_string()];
        assert_eq!(enviar_arquivo(&pdf, "", &d, &enviar, &recarregar), PREVIA_NAO_ABRIU);
        assert_eq!(*recargas.borrow(), 2); // so uma recarga por envio
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn manda_qualquer_arquivo_menos_os_que_guardam_segredo() {
        let dados = [PathBuf::from("E:/sistema/dados")];
        for ruim in ["C:/Users/ana/.env", "C:/Users/ana/Projetos/app/braniac/data/jarvis.db", "C:/Windows/system32/notepad.exe", "C:/Users/ana/AppData/Local/x.zip",
                     "C:/Users/ana/Projetos/legado/plano.md", "C:/Users/ana/.ssh/id_rsa", "C:/Users/ana/Documents/chave.pem", "D:/Perfil/ChromeAgente/Default/Cookies",
                     "E:/sistema/dados/pessoal.json", "E:/sistema/dados/recebidos/ana/doc.pdf", "E:\\SISTEMA\\DADOS\\memoria.txt", "C:/Program Files/x/y.txt"] {
            assert!(bloqueado(Path::new(ruim), &dados), "{ruim}");
        }
        for bom in ["C:/Users/ana/Documents/guia.pdf", "C:/Users/ana/Projetos/app/braniac/data/imagens/arte.png", "C:/Users/ana/Projetos/app/braniac/data/cortes/saida/v.mp4",
                    "E:/sistema/dados/imagens/arte.png", "E:/sistema/dados/cortes/saida/v.mp4", "E:/sistema/dados-publicos/a.txt", "E:/entregas/video.mp4"] {
            assert!(!bloqueado(Path::new(bom), &dados), "{bom}");
        }
    }

    #[test]
    fn resolve_por_caminho_nome_ou_atalho_e_recusa_o_protegido() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let aqui = src.join("wpp_arquivo.rs");
        let pastas = Pastas { atalhos: vec![("manual".into(), src.join("main.rs"))], raizes: vec![src.clone()], protegidas: vec![] };
        let igual = |a: &Path, b: &Path| baixo(a) == baixo(b);
        assert!(igual(&resolver_caminho(&aqui.to_string_lossy(), &pastas).unwrap(), &aqui));
        assert!(igual(&resolver_caminho("wpp_arquivo.rs", &pastas).unwrap(), &aqui)); // por parte do nome
        assert!(igual(&resolver_caminho("coisas/wpp_arquivo.rs", &pastas).unwrap(), &aqui)); // caminho relativo inventado: vale o nome
        assert!(igual(&resolver_caminho("MANUAL", &pastas).unwrap(), &src.join("main.rs")));
        assert_eq!(resolver_caminho("  ", &pastas).unwrap_err(), "Faltou dizer qual arquivo.");
        assert!(resolver_caminho("arquivo-que-nao-existe-xyz", &pastas).unwrap_err().starts_with("Não achei nenhum arquivo"));
        assert!(resolver_caminho("C:/pasta-que-nao-existe/a.pdf", &pastas).unwrap_err().starts_with("Arquivo não encontrado"));
        for ruim in ["C:/Users/ana/.env", "C:/Windows/system32/notepad.exe", "C:/Users/ana/AppData/Local/x.zip"] {
            assert!(resolver_caminho(ruim, &pastas).unwrap_err().contains("guarda segredos"), "{ruim}");
        }
        // a pasta de dados do sistema fica de fora ate da busca por nome
        let fechado = Pastas { protegidas: vec![src.clone()], ..pastas.clone() };
        assert!(resolver_caminho(&aqui.to_string_lossy(), &fechado).unwrap_err().contains("guarda segredos"));
        assert!(buscar("wpp_arquivo.rs", &fechado, 3, Duration::from_secs(3)).is_empty());
        assert!(buscar("a", &pastas, 3, Duration::from_secs(3)).is_empty()); // consulta sem palavra util
    }

    #[test]
    fn mensagem_igual_ha_pouco_nao_e_repetida() {
        let agora = Agora { data: Data::civil(2026, 10, 25).unwrap(), hora: 10, min: 20, seg: 0 };
        let r = |h: &str, t: &str| vec![(h.to_string(), t.to_string())];
        let lembrete = "Lembrete: reunião com o cliente às 11h";
        assert!(duplicada(&r("[10:05, 25/10/2026] Davi: ", lembrete), lembrete, &agora));
        assert!(duplicada(&r("[10:00, 25/10/2026] Davi: ", "lembrete:  reunião com o cliente às 11h (enviado pelo Jarvis)"), lembrete, &agora)); // com a nota do final
        assert!(!duplicada(&r("[9:59, 25/10/2026] Davi: ", lembrete), lembrete, &agora)); // mais de 20 min
        assert!(!duplicada(&r("[10:30, 25/10/2026] Davi: ", lembrete), lembrete, &agora)); // carimbo no futuro
        assert!(!duplicada(&r("[10:05, 24/10/2026] Davi: ", lembrete), lembrete, &agora));
        assert!(!duplicada(&r("[10:05, 25/10/2026] Davi: ", "outra coisa bem diferente"), lembrete, &agora));
        assert!(!duplicada(&r("[10:05, 25/10/2026] Davi: ", "ok"), "ok", &agora)); // curto nunca conta
        assert!(!duplicada(&r("sem carimbo", lembrete), lembrete, &agora) && !duplicada(&r("[10:05, 31/02/2026] x", lembrete), lembrete, &agora));
        // virada do dia
        let meia = Agora { data: Data::civil(2026, 10, 26).unwrap(), hora: 0, min: 5, seg: 0 };
        assert!(duplicada(&r("[23:50, 25/10/2026] Davi: ", lembrete), lembrete, &meia));
        assert!(ja_enviada("Ana").starts_with("✅"));
    }
}
