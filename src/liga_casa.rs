//! Ligacao (liga_casa): casa, voz e sistema. Chamado por main.rs nos tres ganchos (get, post, tick).
//! Regra central: o Braniac em Rust nao envia nada para fora; todo envio vai para crate::saida.
//! - Escuta por voz SEM microfone nativo: a tela /escuta grava e transcreve pelo navegador e manda a frase; aqui roda a maquina
//!   de estados (`escuta::Escuta`) e a sessao da Alma (`alma_voz::Sessao`). O que seria falado volta para a tela tocar.
//! - Cuidar do PC (organizar Downloads, limpeza, seguranca, guarda), vagas, freelas e avaliacao: so por pedido (POST).
//! - Rede, portal, porta LAN, remoto, par, elo, MCP e taxa: nada sobe aqui; so se diz que esta desligado.
//! - Rodada 5: o que a Alma aprende (consentimento e "esquecer"), conhecimento do Airbnb, ensaios (projeto, comando do dono, faxina de
//!   abas, barra do Claude: devolvem o que fariam e nao fazem), sonda de video (ffprobe, so leitura) e a lista do que falta (/falta).
use crate::alma_cuida::{Alma, Nomes};
use crate::alma_voz::{Sessao, Voz};
use crate::escuta::{Escuta, Mundo};
use crate::roteador::Roteador;
use crate::tempo::Agora;
use crate::{agente_dev, alma_mente, alma_voz, avaliar_jarvis, fatos, freelas, funil, gratis, guarda, limpeza, manutencao, mostrar, mudo, organizador, power, saida, seguranca, sobrecarga, tempo, vagas, Resp};
use crate::{airbnb_conhecimento, alma_aprende, alma_orbe, celular, faxina_abas, fornalha_estudio, habilidades_terceiros, monitor_dono, orbe, organiza_claude, projeto_consulta, sentinela, video_analise, vm};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, TryLockError};

/// A escuta e a sessao da esfera da Alma. Valem ate o programa fechar.
pub static VOZ: LazyLock<Mutex<(Escuta, Sessao)>> = LazyLock::new(Default::default);
/// Trabalhos de fundo em andamento (um de cada por vez).
static FUNDO: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

/// Tem trabalho longo de fundo em curso (usado pelo estado "parando" do interruptor do servidor).
pub fn fundo_em_curso() -> bool {
    !crate::travar(&FUNDO).is_empty()
}

const SO_O_DONO: &str = "desligado: só o dono liga";
const LOGIN: &str = "precisa do seu login";
const APARELHO: &str = "precisa de aparelho (microfone, janela, render)";
const EXPOE: &str = "expõe o computador: só você liga";
const TROCA: &str = "espera a troca do Braniac atual pelo Rust";
const GRUPOS: [&str; 4] = [LOGIN, APARELHO, EXPOE, TROCA];
/// (grupo, modulo, o que falta): tudo que tem regra pronta e continua sem ligar.
const NAO_LIGADOS: [(&str, &str, &str); 43] = [
    (LOGIN, "gmail", "falta você autorizar a leitura do Gmail (o login é seu)"),
    (LOGIN, "gmail_web", "falta a aba do Gmail aberta e logada por você no navegador"),
    (LOGIN, "correio", "falta o Gmail ligado: ele só resume os e-mails que o Gmail entrega"),
    (LOGIN, "drive_pasta", "falta a aba do Drive logada por você no navegador"),
    (LOGIN, "spotify", "falta o seu login no Spotify"),
    (LOGIN, "ia_web", "faltam as abas das IAs logadas por você no navegador"),
    (LOGIN, "ia_img", "falta a aba do Gemini logada por você no navegador"),
    (TROCA, "telegram_bot", "o laço existe (telegram_laco, chave \"telegram\" desligada, zero chamadas à API); só você liga, depois de desligar o robô do Braniac em Python (dois robôs com o mesmo token brigam)"),
    (LOGIN, "telegram_ex", "falta o Telegram Web logado por você (aba telegram_web na VM); a leitura das conversas ainda não foi ligada"),
    (LOGIN, "wpp_direto", "falta você parear o WhatsApp (QR code)"),
    (LOGIN, "wpp_img", "falta o WhatsApp Web logado por você, com a conversa aberta para ler a foto"),
    (LOGIN, "grupo_ia", "falta o WhatsApp Web logado por você e as abas do conselho de IAs"),
    (LOGIN, "candidatura_rotina", "falta o navegador logado por você nos sites de vaga e o agente de programação"),
    (APARELHO, "sentinela", "o microfone nativo e o alto-falante existem (aparelho.rs) atrás da chave \"voz\", desligada: só você liga, depois de desligar a escuta do Braniac em Python (dois ouvindo e falando juntos é desastre)"),
    (APARELHO, "orbe", "sem janela flutuante, por pedido seu: só ícone do Jarvis na bandeja do Windows (anel laranja)"),
    (APARELHO, "alma_orbe", "sem janela flutuante, por pedido seu: só ícone da Alma na bandeja (anel roxo); a sessão e o bom dia por voz funcionam sem janela, falando no alto-falante só com a chave \"voz\" ligada"),
    (APARELHO, "celular", "falta o aplicativo do celular ligado a este computador"),
    (APARELHO, "tela", "falta olhar e controlar a tela do computador (mouse e teclado)"),
    (APARELHO, "tela_viva", "falta o navegador do Braniac aberto para mostrar a aba ao vivo"),
    (APARELHO, "link_render_estudio", "falta o estúdio de vídeo (render): a ponte do Link recusa pedido de render com \"ainda não disponível\""),
    (APARELHO, "link_render_parceiro", "falta o pacote de vídeos instalado e o render (um render por vez no computador)"),
    (APARELHO, "link_preparar_parceiro", "falta rodar o ffmpeg, a transcrição e a voz da narração"),
    (APARELHO, "link_conferir", "falta tirar os quadros do vídeo (ffmpeg) e alguém conferir a folha"),
    (APARELHO, "artes_loja", "falta o desenho das artes e o ffmpeg do reel"),
    (EXPOE, "rede", "conversaria com outros Braniacs pela rede"),
    (EXPOE, "rede_jarvis", "as regras existem; a conexão só sai pela ponte temporária com o Python (chave \"loja\", desligada; só você liga)"),
    (EXPOE, "par_webrtc", "o canal em si (WebRTC) só existe na ponte temporária com o Python (chave \"loja\", desligada); o B.R.A.N.I não abre canal sozinho"),
    (EXPOE, "remoto", "deixaria o celular usar este Braniac de fora de casa"),
    (EXPOE, "portal", "abriria a porta de entrada com senha para usar o Braniac de fora"),
    (EXPOE, "porta_lan", "abriria uma porta na rede de casa para receber arquivos"),
    (EXPOE, "porta_lan_cliente", "mandaria arquivos para a porta de outro computador da casa"),
    (EXPOE, "mcp_servidor", "deixaria uma ferramenta de fora consultar os agentes"),
    (EXPOE, "taxa_sepique", "atenderia o aplicativo do taxímetro e leria o extrato"),
    (EXPOE, "elo_loja", "conversa pelo elo só pela ponte temporária com o Python (chave \"loja\", desligada; só você liga, com o servidor antigo parado)"),
    (EXPOE, "link_predio", "o prédio do par só passa pela ponte temporária com o Python (chave \"link\", desligada); a tela do nosso prédio ainda não sai pela ponte"),
    (EXPOE, "link_fila_cliente", "mandaria pedidos e arquivos para a fila de outro Braniac"),
    (EXPOE, "habilidades_terceiros (instalar)", "baixaria habilidades da internet; o catálogo já aparece, instalar só com o seu OK"),
    (TROCA, "ligar_tudo", "sobe o navegador e o servidor do Braniac; só faz sentido quando o Rust for o único"),
    (TROCA, "fcc", "sobe a ponte das IAs grátis; hoje quem sobe é o Braniac atual"),
    (TROCA, "dev_livre", "mexe no código sozinho; fica com a automelhoria do Braniac atual até a troca"),
    (TROCA, "organiza_claude (gravar)", "aqui só simula; gravar na barra do Claude continua com o Braniac atual (dois gravando o mesmo arquivo brigam)"),
    (TROCA, "link_radar_mt", "usa o navegador do Braniac atual para consultar o painel do tribunal, uma consulta por vez"),
    (TROCA, "link_etcm", "baixa os PDFs do tribunal um por vez no computador inteiro; essa trava hoje é do Braniac atual"),
];
const REDE: [(&str, &str); 9] = [
    ("rede_jarvis", "conversaria com outros Braniacs pela rede"),
    ("par_webrtc", "abriria um canal direto com outro Braniac por código de pareamento"),
    ("remoto", "deixaria o celular usar este Braniac de fora de casa"),
    ("portal", "abriria a porta de entrada com senha para usar o Braniac de fora"),
    ("porta_lan", "abriria uma porta na rede de casa para receber arquivos"),
    ("porta_lan_cliente", "mandaria arquivos para a porta de outro PC da casa"),
    ("elo_loja", "conversaria com a assistente da loja pelo elo"),
    ("mcp_servidor", "deixaria uma ferramenta de fora consultar os agentes"),
    ("taxa_sepique", "atenderia o aplicativo do taxímetro e leria o extrato"),
];

/// O relogio de cada aba para a faxina (desde quando esta na mesma pagina). Vale ate o programa fechar.
static VISTAS: LazyLock<Mutex<faxina_abas::Vistas>> = LazyLock::new(Default::default);
/// Dia em que o perfil da Alma foi refeito pela ultima vez (1 vez por dia).
static PERFIL_DIA: Mutex<String> = Mutex::new(String::new());
static AIRBNB: std::sync::Once = std::sync::Once::new();

/// (hora local, segundos desde 1970): o `post` nao recebe o relogio.
fn relogio() -> (Agora, f64) {
    (tempo::agora(), fatos::agora_ts())
}

fn ler(arq: &Path) -> Value {
    std::fs::read_to_string(arq).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null)
}

fn anexar(arq: &Path, linha: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(arq) {
        let _ = writeln!(f, "{linha}");
    }
}

/// Aviso automatico ao dono: passa pelo funil (critico/urgente sai ja; o resto espera o resumo) e so chega a caixa de saida.
fn alertar(dados: &Path, origem: &str, texto: &str, agora: &Agora, ts: f64) -> String {
    let dono = crate::dono_de(dados);
    funil::filtrar(dados, &dono, &dono, texto, origem, agora, ts, &|para, t| saida::registrar(dados, origem, para, t))
}

// ---------- escuta por voz ----------

/// O que vem de fora numa frase: o cerebro do Jarvis, o modelo LOCAL da Alma e o retrato do escritorio.
struct Fora<'a> {
    responder: &'a dyn Fn(&str) -> String,
    llm: &'a dyn Fn(&str) -> Option<String>,
    retrato: &'a dyn Fn() -> Result<Value, String>,
}

/// (agente, texto) que a tela deve falar.
type Falas = RefCell<Vec<(&'static str, String)>>;

/// A Alma com as saidas presas na caixa de saida; a "fala em voz alta" dela so e anotada em `falas` (e nunca no mudo).
fn com_voz<R>(dados: &Path, agora: &Agora, ts: f64, jarvis_acordado: bool, fora: &Fora, falas: &Falas, f: impl FnOnce(&Voz) -> R) -> R {
    let nomes = Nomes::ler(dados);
    let enviar = |para: &str, t: &str| saida::registrar(dados, "alma", para, t);
    let audio = |para: &str, t: &str| saida::registrar(dados, "alma-audio", para, t);
    let alma = Alma { dados, nomes: &nomes, agora: *agora, enviar: &enviar, audio: Some(&audio), llm: Some(fora.llm) };
    let dizer = |t: &str| {
        if !mudo::ativo(dados, ts) {
            falas.borrow_mut().push(("alma", t.to_string()));
        }
    };
    // sem microfone nativo nao ha como saber se outro programa esta em chamada; sem time de programacao ligado, nao abre ticket
    f(&Voz { alma: &alma, relogio: ts, dizer: &dizer, jarvis_em_conversa: jarvis_acordado, em_chamada: false, abrir_orbe: &|| crate::janela::mostrar(dados, crate::janela::alma(crate::PORTA)), retrato: fora.retrato, abrir_ticket: None, semente: ts as i64 })
}

/// O que a tela /escuta le.
pub fn estado_voz(dados: &Path, (esc, ses): &(Escuta, Sessao), ts: f64) -> Value {
    let h: Vec<Value> = esc.historico.iter().rev().take(40).rev().map(|(p, t)| json!([p.to_string(), t])).collect();
    json!({ "estado": esc.estado, "agente": esc.agente, "ultimo": esc.ultimo.chars().take(80).collect::<String>(), "historico": h,
            "mudo": mudo::ativo(dados, ts), "tela": mostrar::tela_atual(&esc.tela, ts).map(|(id, url)| json!({ "id": id, "url": url })),
            "alma": { "ativa": ses.ativa, "nivel": ses.nivel, "legenda": ses.legenda } })
}

/// Uma frase vinda da tela. Devolve o texto da resposta, o que falar em voz alta (null no mudo) e quem fala.
fn frase(dados: &Path, par: &mut (Escuta, Sessao), texto: &str, agora: &Agora, ts: f64, fora: &Fora) -> Value {
    let (esc, ses) = &mut *par;
    esc.ligar(); // sem microfone nativo, a escuta "liga" na primeira frase que a tela manda
    let (falas, dono) = (Falas::default(), crate::dono_de(dados));
    let dito = com_voz(dados, agora, ts, esc.estado == "acordado", fora, &falas, |v| {
        ses.expirar(v);
        let ativa = ses.ativa;
        let ses = RefCell::new(ses);
        let ouvir = |t: &str| ses.borrow_mut().ouvir(v, t);
        let falar = |t: &str, agente: &str| falas.borrow_mut().push((if agente == "alma" { "alma" } else { "jarvis" }, t.to_string()));
        let alma = |t: &str| Ok(alma_mente::responder(v.alma, t));
        let m = Mundo {
            dados, ts, porta: crate::PORTA, apelidos: &[], falar: &falar, responder: fora.responder, alma: &alma,
            sessao: if ativa { Some(&ouvir) } else { None },
            fechar_sessao: &|motivo| ses.borrow_mut().fechar(v, motivo),
            abrir_janela: &|| crate::janela::mostrar(dados, crate::janela::jarvis(crate::PORTA)),
            minimizar_janela: &|| crate::janela::minimizar(crate::janela::jarvis(crate::PORTA)),
            ler_tela: &|_| Err("não ligado".into()),
            abrir_link: &|u| drop(crate::abrir_no_pc(u)),
            mandar_whatsapp: &|t| drop(saida::registrar(dados, "escuta", &dono, t)),
            mandar_telegram: &|t| saida::registrar(dados, "escuta-telegram", &dono, t).starts_with('✅'),
            log: &|l| anexar(&dados.join("escuta.log"), &format!("{} {} | {l}", agora.data.iso(), agora.hm())),
        };
        esc.tratar_frase(&m, texto)
    });
    let falas = falas.into_inner();
    let fala = falas.iter().map(|f| f.1.as_str()).collect::<Vec<_>>().join(" ");
    let alma_falou = par.1.ativa || par.0.agente == "alma";
    json!({ "texto": dito, "fala": (!fala.is_empty()).then_some(fala), "agente": falas.last().map_or(if alma_falou { "alma" } else { "jarvis" }, |f| f.0),
            "estado": estado_voz(dados, par, ts) })
}

/// Relogio da voz da Alma: bom dia por voz, "ate tarde" e a esfera na sobrecarga. Devolve o que ela falaria.
/// A amostra de atividade e a sobrecarga sao gravadas pelo relogio do main.rs: aqui so se le (por isso `tick_com`, e nao `alma_voz::tick`,
/// que amostraria o mesmo minuto duas vezes).
fn tick_voz(dados: &Path, par: &mut (Escuta, Sessao), agora: &Agora, ts: f64) -> Vec<String> {
    if alma_mente::params(dados)["monitor_ligado"] != true || power::pausado(dados) {
        return Vec::new();
    }
    let ((esc, ses), falas) = (&mut *par, Falas::default());
    let av = sobrecarga::avaliar(dados, agora, false);
    // os textos do relogio sao fixos: nenhum modelo e chamado
    let fora = Fora { responder: &|_| String::new(), llm: &|_| None, retrato: &|| Err("não ligado".into()) };
    com_voz(dados, agora, ts, esc.estado == "acordado", &fora, &falas, |v| {
        ses.expirar(v);
        alma_voz::tick_com(v, ses, &av);
    });
    falas.into_inner().into_iter().map(|f| f.1).collect()
}

/// A trava da voz sem esperar: None = a tela esta no meio de uma frase (a IA pode levar muitos segundos).
pub fn voz_livre() -> Option<MutexGuard<'static, (Escuta, Sessao)>> {
    match VOZ.try_lock() {
        Ok(g) => Some(g),
        Err(TryLockError::Poisoned(e)) => Some(e.into_inner()),
        Err(TryLockError::WouldBlock) => None,
    }
}

// ---------- cuidar do PC ----------

fn casa() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from)
}

fn sem_janela(c: &mut std::process::Command) {
    c.stdin(std::process::Stdio::null());
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(c, 0x0800_0000); // CREATE_NO_WINDOW
}

/// PowerShell so de leitura: sem perfil, sem pergunta, sem janela. A saida vem em UTF-8.
fn powershell(script: &str) -> Result<String, String> {
    if !cfg!(windows) {
        return Err("só no Windows".into());
    }
    let mut c = crate::oculto::cmd("powershell.exe");
    c.args(["-NoProfile", "-NonInteractive", "-Command", &format!("[Console]::OutputEncoding=[Text.Encoding]::UTF8;{script}")]);
    sem_janela(&mut c);
    let o = c.output().map_err(|e| format!("não consegui rodar o PowerShell: {e}"))?;
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}

/// So MOVE arquivo solto para subpasta por tipo. Simulando, nada e tocado nem avisado.
fn organizar_pc(pasta: &Path, dados: &Path, simular: bool, agora: &Agora, ts: f64) -> Value {
    let movidos = if simular {
        organizador::organizar(pasta, dados, agora, ts as u64, true)
    } else {
        organizador::rodar(pasta, dados, agora, ts as u64, false, &|t| drop(alertar(dados, "organizador", t, agora, ts)))
    };
    json!({ "simular": simular, "pasta": pasta.to_string_lossy(), "movidos": movidos.iter().map(|(de, para)| json!({ "de": de, "para": para })).collect::<Vec<_>>() })
}

fn seguranca_pc(dados: &Path, agora: &Agora, ts: f64, ps: &dyn Fn(&str) -> Result<String, String>) -> Result<Value, String> {
    // sem varrer pastas atras de client_secret nesta rodada; o .env daqui e criptografado (nao e "chave em texto puro")
    let (achados, arq) = seguranca::rodar(&ps(seguranca::PS)?, &[], false, dados, &dados.join("relatorios"), agora, &|t| drop(alertar(dados, "seguranca", t, agora, ts)))?;
    Ok(json!({ "arquivo": arq.file_name().map(|n| n.to_string_lossy().into_owned()),
               "achados": achados.iter().map(|(g, o, c)| json!({ "gravidade": g, "achado": o, "correcao": c })).collect::<Vec<_>>() }))
}

fn guarda_pc(dados: &Path, agora: &Agora, ts: f64, ps: &dyn Fn(&str) -> Result<String, String>) -> Result<Value, String> {
    let saidas = [guarda::PS_PORTAS, guarda::PS_FIREWALL, guarda::PS_RDP, guarda::PS_DEFENDER, guarda::PS_FALHAS].map(|s| ps(s).unwrap_or_default());
    if saidas.iter().all(|s| s.trim().is_empty()) {
        return Err("nenhuma leitura do PowerShell respondeu: nada foi avaliado".into());
    }
    // sem IP publico: consultar ele seria falar com um site de fora
    let est = guarda::montar(ts, None, &saidas[0], &saidas[1], &saidas[2], &saidas[3], &saidas[4]);
    let alertas = guarda::verificar(dados, &est, ts, &|t| drop(alertar(dados, "guarda", t, agora, ts)));
    Ok(json!({ "alertas": alertas, "resumo": guarda::resumo(dados), "leitura": est }))
}

// ---------- trabalho: leitura de lista publica, so por pedido ----------

fn baixar(url: &str) -> Result<String, String> {
    // identificacao generica: nada do dono nem do projeto vai no pedido
    ureq::get(url).set("User-Agent", "leitor-de-feed/1.0").timeout(std::time::Duration::from_secs(25)).call().map_err(|e| e.to_string())?.into_string().map_err(|e| e.to_string())
}

fn json_de(texto: &str) -> Result<Value, String> {
    serde_json::from_str(texto).map_err(|e| format!("resposta ilegível: {e}"))
}

/// O Garimpeiro a pedido. O proprio modulo segura: 1 vez por dia, a partir das 8h, no maximo 5 vagas.
fn vagas_buscar(dados: &Path, agora: &Agora, ts: f64, baixar: &dyn Fn(&str) -> Result<String, String>) -> Result<Value, String> {
    // shortcut: os termos sao fixos e so tem espaco; termo livre pediria um codificador de URL de verdade
    let buscar = |termo: &str| Ok(json_de(&baixar(&format!("https://remotive.com/api/remote-jobs?limit=25&search={}", termo.replace(' ', "%20")))?)?["jobs"].as_array().cloned().unwrap_or_default());
    let n = vagas::verificar(dados, agora, ts, &buscar, &|t| {
        alertar(dados, "vagas", t, agora, ts);
        Ok(())
    })?;
    Ok(json!({ "novas": n, "obs": if n == 0 { "Nenhuma vaga nova. A busca roda 1 vez por dia, a partir das 8h." } else { "As novas foram para a fila de candidaturas e para a caixa de saída." } }))
}

/// A busca de freelas pode rodar agora? O mesmo intervalo do `freelas::tick` (6 h, das 8h as 21h), conferido antes de abrir a thread.
fn freelas_pode(dados: &Path, agora: &Agora, ts: f64) -> bool {
    (8..21).contains(&agora.hora) && ts - ler(&dados.join("freelas").join("estado.json"))["ultimo"].as_f64().unwrap_or(0.0) >= 6.0 * 3600.0
}

/// Baixa os feeds publicos, tria (IA local; sem ela a nota fica 0) e avisa dos bons pelo funil. Devolve se rodou.
fn freelas_buscar(dados: &Path, agora: &Agora, ts: f64, baixar: &dyn Fn(&str) -> Result<String, String>, llm: &dyn Fn(&str) -> Value) -> bool {
    let remoteok = || Ok(freelas::itens_remoteok(&json_de(&baixar("https://remoteok.com/api")?)?));
    let reddit = || Ok(freelas::itens_reddit(&baixar("https://www.reddit.com/r/forhire/new/.rss")?));
    freelas::tick(dados, agora, ts, &|t| drop(alertar(dados, "freelas", t, agora, ts)), llm, &[("remoteok", &remoteok), ("reddit", &reddit)])
}

/// O primeiro objeto JSON dentro da resposta de um modelo; ilegivel = {} (a triagem da nota 0).
fn objeto_em(texto: &str) -> Value {
    texto.find('{').zip(texto.rfind('}')).filter(|(a, b)| a < b).and_then(|(a, b)| serde_json::from_str(&texto[a..=b]).ok()).unwrap_or_else(|| json!({}))
}

// ---------- rodada 5: Alma aprende, Airbnb, ensaios, video, aparelhos ----------

const PARTES: [&str; 4] = ["temas", "sinais", "exemplos", "pausado"];
const ESCOPOS: [&str; 6] = ["sinais", "exemplos", "rotulos", "perfil", "diario", "tudo"];

/// O que a Alma aprendeu, so em numeros, e o que o dono deixou ligado.
fn aprende_estado(dados: &Path, agora: &Agora) -> Value {
    json!({ "consentimento": alma_aprende::consentimento(dados), "sabe": alma_aprende::sabe(dados, agora), "resumo": alma_aprende::resumo_semanal(dados, agora),
            "tom_mais_calmo": !alma_aprende::dica_tom(dados, agora, "o dono").is_empty() })
}

/// Muda o consentimento ({"sinais": false}, {"pausado": true}...) ou esquece ({"esquecer": "tudo", "confirmar": "ESQUECER"}).
fn aprende_mudar(dados: &Path, agora: &Agora, v: &Value) -> Result<Value, String> {
    if let Some(escopo) = v["esquecer"].as_str() {
        if !ESCOPOS.contains(&escopo) && !escopo.strip_prefix("tema:").is_some_and(|t| !t.is_empty()) {
            return Err(format!("não sei esquecer \"{escopo}\"; use {} ou tema:<nome>. Nada foi apagado.", ESCOPOS.join(", ")));
        }
        if v["confirmar"] != "ESQUECER" {
            return Err("Apagar não tem volta. Para confirmar, mande também {\"confirmar\": \"ESQUECER\"}. Nada foi apagado.".into());
        }
        if !alma_aprende::apagar(dados, agora, escopo) {
            return Err("não consegui abrir o que a Alma guardou; nada foi apagado".into());
        }
        return Ok(json!({ "esquecido": escopo, "estado": aprende_estado(dados, agora) }));
    }
    let mudancas: Vec<(&str, bool)> = PARTES.iter().filter_map(|k| Some((*k, v[*k].as_bool()?))).collect();
    if mudancas.is_empty() {
        return Err("mande {\"temas\"|\"sinais\"|\"exemplos\"|\"pausado\": true ou false} ou {\"esquecer\": \"tudo\", \"confirmar\": \"ESQUECER\"}".into());
    }
    alma_aprende::definir_consentimento(dados, &mudancas);
    Ok(aprende_estado(dados, agora))
}

/// Os textos do Airbnb que entrariam no contexto para a frase (sem frase: todos). Sempre que algum fala em aceitar ou pre-aprovar
/// (a politica da plataforma fala), a regra das RESERVAS vai junto: o sistema nunca aceita nem pre-aprova.
fn airbnb_para(q: &str) -> Vec<&'static str> {
    let mut textos = if q.trim().is_empty() { airbnb_conhecimento::entradas().into_iter().map(|e| e.1).collect() } else { airbnb_conhecimento::para(q) };
    let fala_de_aceitar = |t: &&str| t.to_lowercase().contains("aceit") || t.to_lowercase().contains("pre-aprov");
    if textos.iter().any(fala_de_aceitar) && !textos.contains(&airbnb_conhecimento::RESERVAS) {
        textos.push(airbnb_conhecimento::RESERVAS);
    }
    textos
}

/// ENSAIO do "projeto sem registro": devolve a decisao e nao dispara nada. A marca de "ja perguntei nas ultimas 24 h" e gravada
/// numa copia (apagada no fim) e o fato que a pasta ensinaria so e mostrado.
fn projeto_verificar(dados: &Path, texto: &str, pastas: &[PathBuf], ts: f64, sensivel: bool, banco: &dyn Fn(&str) -> String) -> Value {
    let (p, dono) = (ler(&dados.join("pessoal.json")), crate::dono_de(dados));
    let quem = projeto_consulta::Quem { nome: &dono, descricao: p["descricao"].as_str().unwrap_or(""), onde_procurar: p["onde_procurar"].as_str().unwrap_or("as pastas de projetos do PC") };
    let copia = std::env::temp_dir().join(format!("braniac-projeto-{}-{ts}", std::process::id()));
    let _ = std::fs::create_dir_all(&copia);
    let _ = std::fs::copy(dados.join("projetos_consultados.json"), copia.join("projetos_consultados.json"));
    // frase com dado sensivel: a regra que casa com tudo faz o modulo tirar a frase da pergunta que iria a IA online
    let tudo = regex::Regex::new("").ok().filter(|_| sensivel);
    let aprenderia = RefCell::new(Value::Null);
    let d = projeto_consulta::verificar(texto, &copia, pastas, ts, &quem, tudo.as_ref(), banco, &|chave, fato, fonte| *aprenderia.borrow_mut() = json!({ "chave": chave, "texto": fato, "fonte": fonte }));
    let _ = std::fs::remove_dir_all(&copia);
    let mut v = match d {
        projeto_consulta::Decisao::Nada => json!({ "decisao": "nada" }),
        projeto_consulta::Decisao::Instrucao(i) => json!({ "decisao": "instrucao", "instrucao": i }),
        projeto_consulta::Decisao::Consultar { nome, instrucao, pergunta_gpt, tarefa_claude } => json!({ "decisao": "consultar", "nome": nome, "instrucao": instrucao, "pergunta_gpt": pergunta_gpt, "tarefa_claude": tarefa_claude }),
    };
    v["aprenderia"] = aprenderia.into_inner();
    v["disparou"] = json!(false);
    v
}

const VIDEO_EXT: [&str; 6] = ["mp4", "mov", "mkv", "webm", "avi", "m4v"];
const PROTEGIDAS: [&str; 8] = [".git", ".ssh", ".gnupg", "appdata", "windows", "programdata", "program files", "program files (x86)"];

fn extensao_de_video(p: &Path) -> bool {
    p.extension().is_some_and(|e| VIDEO_EXT.contains(&e.to_string_lossy().to_lowercase().as_str()))
}

/// O caminho que a sonda pode ler: completo, de video, existente, deste PC e fora da pasta de dados e das pastas protegidas.
/// Confere o arquivo REAL (atalho "video.mp4" apontando para outra coisa nao passa).
fn video_valido(arquivo: &str, dados: &Path) -> Result<PathBuf, String> {
    let p = Path::new(arquivo.trim());
    if !p.is_absolute() {
        return Err("mande o caminho completo do vídeo".into());
    }
    if !extensao_de_video(p) {
        return Err(format!("só analiso vídeo ({})", VIDEO_EXT.join(", ")));
    }
    let real = p.canonicalize().ok().filter(|r| r.is_file()).ok_or("esse arquivo não existe")?;
    let dados = dados.canonicalize().unwrap_or_else(|_| dados.to_path_buf());
    let protegida = real.components().any(|c| PROTEGIDAS.contains(&c.as_os_str().to_string_lossy().to_lowercase().as_str()));
    if !extensao_de_video(&real) || real.starts_with(&dados) || protegida || real.to_string_lossy().starts_with(r"\\?\UNC") {
        return Err("esse arquivo fica numa pasta protegida (ou fora deste computador): não analiso".into());
    }
    Ok(real)
}

/// Roda o ffprobe (so le o arquivo). `args` vem do `fornalha_estudio::args_sondar`: o caminho e UM argumento, nunca passa por shell.
fn ffprobe(args: &[String]) -> Result<String, String> {
    let exe = which("ffprobe").ok_or("não ligado: falta o ffmpeg")?;
    let mut c = crate::oculto::cmd(exe);
    c.args(&args[1..]);
    sem_janela(&mut c);
    // shortcut: sem tempo limite; o ffprobe so le o cabecalho de um arquivo deste PC. Por um prazo se um dia aceitar caminho de rede.
    let o = c.output().map_err(|e| format!("não consegui rodar o ffprobe: {e}"))?;
    if !o.status.success() {
        return Err(format!("o ffprobe não leu o vídeo: {}", String::from_utf8_lossy(&o.stderr).chars().take(200).collect::<String>()));
    }
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}

/// Placar da Fornalha (0-100, regua de reel: 3 a 20 s, 9:16) e o resumo da sonda. Nao tira quadro nem transcreve.
fn analisar_video(video: &Path, sondar: &dyn Fn(&[String]) -> Result<String, String>) -> Result<Value, String> {
    let bytes = video.metadata().map_err(|e| e.to_string())?.len();
    let caminho = video.to_string_lossy();
    let saida = sondar(&fornalha_estudio::args_sondar(caminho.trim_start_matches(r"\\?\")))?;
    let sonda = fornalha_estudio::sonda_de_json(&saida);
    let (nota, avisos) = fornalha_estudio::placar_video(sonda.as_ref(), bytes);
    let dur = video_analise::duracao(&saida);
    Ok(json!({ "arquivo": video.file_name().map(|n| n.to_string_lossy().into_owned()), "nota": nota, "avisos": avisos,
               "resumo": { "duracao_s": dur, "largura": sonda.map(|s| s.w), "altura": sonda.map(|s| s.h), "tem_audio": sonda.map(|s| s.audio), "bytes": bytes,
                           "quadros_que_olharia_s": video_analise::quadros(4, dur).iter().map(|q| q.0).collect::<Vec<_>>() },
               "obs": "Só a sonda do arquivo: não tirei quadros nem transcrevi a fala." }))
}

/// Catalogo de habilidades de terceiros e quais estao instaladas. Instalar nao tem rota: baixa de fora, so com o OK do dono.
fn habilidades(dados: &Path) -> Value {
    let cat = dados.join("habilidades_catalogo.json");
    if !cat.is_file() {
        return json!({ "perguntado": false, "itens": [], "obs": "Sem catálogo (dados/habilidades_catalogo.json). Instalar não é feito por aqui." });
    }
    match habilidades_terceiros::status(&cat, &dados.join("skills"), dados) {
        Ok(mut s) => {
            s["obs"] = json!("Instalar não é feito por aqui: baixa da internet, só com o seu OK.");
            s
        }
        Err(e) => json!({ "itens": [], "erro": e }),
    }
}

/// Faxina de abas da VM. Aba de agente (a reservada do WhatsApp e uma delas) nunca entra na escolha; o resto segue a regra do
/// `faxina_abas` (parada ha 1 h na mesma pagina e fora da lista de essenciais). Simulando, nada fecha e o relogio de cada aba continua.
fn faxina(abas: &Value, vistas: &mut faxina_abas::Vistas, ts: f64, simular: bool, fechar: &dyn Fn(&str) -> Result<Value, String>) -> Value {
    let todas = abas.as_array().map_or(0, Vec::len);
    let livres: Vec<(String, String)> = abas.as_array().into_iter().flatten().filter(|a| a["agente"].as_str().unwrap_or("").is_empty())
        .filter_map(|a| Some((a["id"].as_str()?.to_string(), a["url"].as_str()?.to_string()))).collect();
    let antes = vistas.clone();
    let (escolhidas, mut fechadas) = (faxina_abas::escolher(&livres, vistas, ts, faxina_abas::PARADA_S), Vec::new());
    for id in &escolhidas {
        if !simular && fechar(id).is_ok() {
            fechadas.push(id.clone());
        } else if let Some(v) = antes.get(id) {
            vistas.insert(id.clone(), v.clone()); // nao fechou: ela continua contando como parada
        }
    }
    let fecharia: Vec<Value> = livres.iter().filter(|a| escolhidas.contains(&a.0)).map(|(id, url)| json!({ "id": id, "url": url })).collect();
    json!({ "simular": simular, "fecharia": fecharia, "fechadas": fechadas, "olhadas": livres.len(), "poupadas_de_agente": todas - livres.len(),
            "obs": "Só entra aba sem agente, parada há 1 hora na mesma página, contada desde a primeira faxina pedida." })
}

/// O que cada aparelho faria. Nenhum esta ligado: nao ha microfone nativo, janela nativa nem aplicativo de celular aqui.
fn aparelhos() -> Value {
    let item = |m: &str, faria: &str, mostraria: Value| json!({ "modulo": m, "estado": "sem aparelho ligado", "ligado": false, "faria": faria, "mostraria": mostraria });
    json!({ "itens": [
        item("celular", "ouviria você pelo aplicativo do celular: dormindo não escuta; acordado conversa por voz e descansa sozinho depois de 3 minutos de silêncio",
             json!({ "acordado": celular::Estado::default().acordado, "descansa_em_s": celular::IDLE_S })),
        item("sentinela", "com o Braniac fechado, ouviria só a frase de acordar e subiria o Braniac; com ele no ar, solta o microfone",
             json!({ "microfone_com_o_braniac_no_ar": sentinela::microfone(true, "ouvindo") })),
        item("orbe", "mostraria numa janelinha se o Jarvis está dormindo, ouvindo ou falando", orbe::dados_padrao()),
        item("alma_orbe", "mostraria a esfera da Alma num canto da tela", json!({ "estado": alma_orbe::offline(), "rotulo": alma_orbe::rotulo("offline") })),
    ] })
}

/// ENSAIO da organizacao da barra do Claude: as regras rodam numa COPIA dos arquivos (config + fichas das conversas), apagada no fim.
/// Os arquivos do app so sao lidos.
fn claude_simular(local: &Path, agora: &Agora) -> Value {
    let filhos = |p: &Path| std::fs::read_dir(p).into_iter().flatten().flatten().map(|e| e.path()).collect::<Vec<_>>();
    let nome = |p: &Path| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let base = std::env::temp_dir().join(format!("braniac-claude-ensaio-{}-{}", std::process::id(), agora.carimbo()));
    let (perfis, n, mut mudaria) = (organiza_claude::perfis(local), std::cell::Cell::new(0), Vec::new());
    for (i, perfil) in perfis.iter().enumerate() {
        let copia = base.join(i.to_string()).join(nome(perfil));
        let _ = std::fs::create_dir_all(&copia);
        let _ = std::fs::copy(perfil.join("claude_desktop_config.json"), copia.join("claude_desktop_config.json"));
        for ficha in filhos(&perfil.join("claude-code-sessions")).iter().flat_map(|a| filhos(a)).flat_map(|b| filhos(&b)).filter(|f| nome(f).starts_with("local_") && nome(f).ends_with(".json")) {
            let Some((b, a)) = ficha.parent().zip(ficha.parent().and_then(Path::parent)) else { continue };
            let destino = copia.join("claude-code-sessions").join(nome(a)).join(nome(b));
            let _ = std::fs::create_dir_all(&destino);
            let _ = std::fs::copy(&ficha, destino.join(nome(&ficha)));
        }
        let novo_id = || {
            n.set(n.get() + 1);
            format!("ensaio{}", n.get())
        };
        match organiza_claude::organizar_perfil(&copia, &base.join("copias"), &organiza_claude::secoes_padrao(), agora, &novo_id) {
            Ok(f) => mudaria.extend(f),
            Err(e) => mudaria.push(format!("erro em {}: {e}", nome(perfil))),
        }
    }
    let _ = std::fs::remove_dir_all(&base);
    json!({ "simular": true, "gravado": false, "perfis": perfis.len(), "mudaria": mudaria })
}

// ---------- fundo, IAs gratis, agente de programacao ----------

/// Tira o nome da lista quando o trabalho acaba, mesmo se ele quebrar.
struct Vez(&'static str);
impl Drop for Vez {
    fn drop(&mut self) {
        crate::travar(&FUNDO).retain(|n| *n != self.0);
    }
}

fn fundo(nome: &'static str, f: impl FnOnce() + Send + 'static) -> Resp {
    {
        let mut rodando = crate::travar(&FUNDO);
        if rodando.contains(&nome) {
            return crate::erro(format!("{nome}: já está rodando"), 409);
        }
        rodando.push(nome);
    }
    std::thread::spawn(move || {
        let _vez = Vez(nome);
        f();
    });
    crate::dado(json!({ "ok": true, "rodando": nome }))
}

fn which(nome: &str) -> Option<String> {
    std::env::split_paths(&std::env::var_os("PATH")?).flat_map(|p| ["", ".exe", ".cmd"].map(|e| p.join(format!("{nome}{e}")))).find(|p| p.is_file()).map(|p| p.to_string_lossy().into_owned())
}

fn lista(itens: &[(&str, &str)], chave: &str) -> Vec<Value> {
    itens.iter().map(|(m, f)| json!({ "modulo": m, chave: f, "ligado": false })).collect()
}

/// Os quatro grupos da tela /falta (e a lista corrida em "itens").
fn nao_ligados() -> Value {
    let item = |(g, m, f): &(&str, &str, &str)| json!({ "grupo": g, "modulo": m, "falta": f, "ligado": false });
    json!({ "grupos": GRUPOS.map(|g| json!({ "grupo": g, "itens": NAO_LIGADOS.iter().filter(|i| i.0 == g).map(item).collect::<Vec<_>>() })),
            "itens": NAO_LIGADOS.iter().map(item).collect::<Vec<_>>() })
}

/// Nada de rede sobe aqui: so o que cada um faria.
fn rede_estado() -> Value {
    json!({ "estado": SO_O_DONO, "servidores_no_ar": 0, "itens": lista(&REDE, "faria") })
}

/// Rotas GET que nao precisam do roteador.
fn get_local(dados: &Path, caminho: &str, ts: f64) -> Option<Value> {
    Some(match caminho {
        "/api/escuta-estado" => voz_livre().map_or_else(|| json!({ "pensando": true }), |par| estado_voz(dados, &par, ts)),
        "/api/pc/organizar" => json!({ "ultimo": ler(&dados.join("organizador.json")) }),
        "/api/pc/limpeza" => {
            let rel = limpeza::ultimo(dados);
            json!({ "varrendo": crate::travar(&FUNDO).contains(&"limpeza"), "resumo": rel.as_ref().map(|r| limpeza::resumo(r, 5)), "relatorio": rel })
        }
        "/api/pc/seguranca" => crate::ultimo_relatorio_seguranca(dados),
        "/api/pc/guarda" => json!({ "resumo": guarda::resumo(dados), "ultima": ler(&dados.join("guarda.json"))["ultima"] }),
        "/api/nao-ligados" => nao_ligados(),
        "/api/habilidades" => habilidades(dados),
        "/api/aparelhos" => aparelhos(),
        "/api/rede-estado" => rede_estado(),
        "/api/agente-dev" => {
            let s = agente_dev::Sistema { env: &|k| std::env::var(k).ok(), which: &which, executar: &|_| Err(agente_dev::Falha::Outra("não ligado".into())), guia: Path::new(""), raiz: Path::new("") };
            agente_dev::status(&s, dados)
        }
        "/api/manutencao" => json!({ "ultimo": manutencao::ultimo(dados), "testes": "não rodam pelo Braniac em Rust nesta rodada" }),
        "/api/avaliar" => json!({ "rodando": crate::travar(&FUNDO).contains(&"avaliar"), "ultimo": ler(&dados.join("avaliacao.json")) }),
        _ => return None,
    })
}

/// Rota GET deste grupo; None = nao e minha.
pub fn get(dados: &Path, rot: &Arc<Roteador>, url: &str) -> Option<Resp> {
    let caminho = url.split('?').next().unwrap_or("");
    match caminho {
        "/escuta" => Some(crate::pagina(include_str!("escuta.html"))),
        "/falta" => Some(crate::pagina(include_str!("falta.html"))),
        "/api/alma/aprende" => Some(crate::dado(aprende_estado(dados, &tempo::agora()))),
        "/api/airbnb/conhecimento" => {
            let q = crate::parametro(url, "q").unwrap_or_default();
            Some(crate::dado(json!({ "q": q, "textos": airbnb_para(&q) })))
        }
        // sem rede: nao confere quais modelos o provedor oferece agora. So diz se ha chave; a chave nunca aparece
        "/api/gratis" => {
            let cat = gratis::catalogo(&dados.join("catalogo_gratis.json"));
            let diagnostico = gratis::diagnostico(&cat, &|nome| (rot.ler_chave)(nome), &|_, _| None);
            Some(crate::dado(json!({ "catalogo": cat, "diagnostico": diagnostico })))
        }
        _ => get_local(dados, caminho, fatos::agora_ts()).map(crate::dado),
    }
}

/// Rota POST deste grupo (ja passou pelo cabecalho X-Braniac); None = nao e minha.
pub fn post(dados: &Path, rot: &Arc<Roteador>, caminho: &str, v: &Value) -> Option<Resp> {
    let (agora, ts) = relogio();
    Some(match caminho {
        "/api/escuta/frase" => {
            let texto = v["texto"].as_str().unwrap_or("").trim();
            if texto.is_empty() {
                return Some(crate::erro("mande {\"texto\": \"...\"}", 400));
            }
            // o mesmo caminho da conversa escrita: responde e, ligado, guarda na memoria e deixa a Alma observar
            let responder = |t: &str| {
                let r = crate::conversar(dados, rot, crate::jarvis::CONV_UNICA, t, true, &agora, ts)["resposta"].as_str().unwrap_or("").to_string();
                if !power::pausado(dados) {
                    let (rot, d, t, r) = (rot.clone(), dados.to_path_buf(), t.to_string(), r.clone());
                    std::thread::spawn(move || crate::depois(&d, &rot, crate::jarvis::CONV_UNICA, &t, &r, &agora, ts));
                }
                r
            };
            let fora = Fora { responder: &responder, llm: &|p| crate::ia_local(rot, p), retrato: &|| Ok(crate::retrato(dados, rot, &agora, ts)) };
            crate::dado(frase(dados, &mut crate::travar(&VOZ), texto, &agora, ts, &fora))
        }
        "/api/pc/organizar" => match casa() {
            Some(c) => crate::dado(organizar_pc(&c.join("Downloads"), dados, v["simular"] != false, &agora, ts)),
            None => crate::erro("não achei a pasta do usuário", 500),
        },
        // so analisa; pode levar muitos minutos, entao roda fora do pedido
        "/api/pc/limpeza" => {
            let pedidas: Vec<PathBuf> = v["raizes"].as_array().into_iter().flatten().filter_map(|r| r.as_str().map(PathBuf::from)).collect();
            let raizes = if pedidas.is_empty() { casa().into_iter().collect() } else { pedidas };
            let d = dados.to_path_buf();
            fundo("limpeza", move || {
                let (agora, ts) = relogio();
                let rel = limpeza::varrer(&d, &raizes, &agora);
                alertar(&d, "limpeza", &limpeza::resumo(&rel, 5), &agora, ts);
            })
        }
        "/api/pc/seguranca" => crate::feito(seguranca_pc(dados, &agora, ts, &powershell)),
        "/api/pc/guarda" => crate::feito(guarda_pc(dados, &agora, ts, &powershell)),
        "/api/vagas/buscar" => crate::feito(vagas_buscar(dados, &agora, ts, &baixar)),
        "/api/freelas/buscar" if !freelas_pode(dados, &agora, ts) => crate::erro("A busca de freelas roda no máximo a cada 6 horas, das 8h às 21h.", 429),
        "/api/freelas/buscar" => {
            let (rot, d) = (rot.clone(), dados.to_path_buf());
            fundo("freelas", move || {
                freelas_buscar(&d, &agora, ts, &baixar, &|p| crate::ia_local(&rot, p).map_or_else(|| json!({}), |t| objeto_em(&t)));
            })
        }
        "/api/alma/aprende" => aprende_mudar(dados, &agora, v).map_or_else(|e| crate::erro(e, 400), crate::dado),
        "/api/projeto/verificar" => {
            let (texto, p) = (v["texto"].as_str().unwrap_or(""), ler(&dados.join("pessoal.json")));
            let config: Vec<PathBuf> = p["pastas_projetos"].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(PathBuf::from)).collect();
            let pastas = if config.is_empty() { casa().map(|c| c.join("Projetos")).into_iter().collect() } else { config };
            crate::dado(projeto_verificar(dados, texto, &pastas, ts, rot.sensivel(texto), &|nome| rot.banco.consultar(nome, "local", None, 8).0))
        }
        "/api/monitor/interpretar" => {
            let (acao, conteudo) = monitor_dono::interpretar(v["texto"].as_str().unwrap_or(""));
            crate::dado(json!({ "acao": acao, "conteudo": conteudo, "executado": false }))
        }
        "/api/video/analisar" => match video_valido(v["arquivo"].as_str().unwrap_or(""), dados).and_then(|p| analisar_video(&p, &ffprobe)) {
            Ok(r) => crate::dado(r),
            Err(e) => crate::erro(&e, if e.starts_with("não ligado") { 501 } else { 400 }),
        },
        "/api/vm/faxina" => match vm::abas(dados) {
            Ok(abas) => crate::dado(faxina(&abas, &mut crate::travar(&VISTAS), ts, v["simular"] != false, &|id| vm::fechar_aba(dados, id))),
            Err(e) => crate::erro(format!("não consegui ler as abas da VM (ela está ligada?): {e}"), 502),
        },
        "/api/claude/organizar" if v["simular"] == false => crate::erro("Gravar de verdade não entra nesta rodada: mexe em arquivo de outro programa. Nada foi feito.", 501),
        "/api/claude/organizar" => match std::env::var_os("LOCALAPPDATA") {
            Some(local) => crate::dado(claude_simular(Path::new(&local), &agora)),
            None => crate::erro("não achei a pasta de dados dos programas (LOCALAPPDATA)", 500),
        },
        "/api/manutencao/testes" => crate::erro("Os testes não rodam pelo Braniac em Rust nesta rodada. Nada foi feito.", 501),
        "/api/avaliar" if !dados.join("avaliacao_casos.json").exists() => crate::erro("não há dados/avaliacao_casos.json: nada para avaliar", 404),
        "/api/avaliar" => {
            let (rot, d) = (rot.clone(), dados.to_path_buf());
            fundo("avaliar", move || {
                // cada caso numa conversa a parte ("b2:..."), para nao sujar a conversa do dono; a duvida pendente dele e devolvida no fim
                let pendente = crate::travar(&crate::PENDENTE).clone();
                let perguntar = |msg: &str, conv: &str| Ok(crate::conversar(&d, &rot, &format!("b2:{conv}"), msg, true, &agora, ts)["resposta"].as_str().unwrap_or("").to_string());
                let r = avaliar_jarvis::avaliar(&perguntar, &avaliar_jarvis::casos(&d), ts);
                *crate::travar(&crate::PENDENTE) = pendente;
                let casos: Vec<Value> = r.iter().map(|(nome, ok, motivo, resposta)| json!({ "caso": nome, "ok": ok, "motivo": motivo, "resposta": resposta })).collect();
                let rel = json!({ "quando": format!("{} {}", agora.data.iso(), agora.hm()), "passaram": r.iter().filter(|c| c.1).count(), "total": r.len(), "casos": casos });
                let _ = std::fs::write(d.join("avaliacao.json"), rel.to_string());
            })
        }
        _ => return None,
    })
}

/// Relogio de 60 s. Nada de IA online, rede ou processo externo aqui sem pedido da pessoa.
/// A voz da Alma (sem esfera nativa, o que ela falaria vai para a caixa de saida, canal alma-voz); 1 vez por dia, o perfil da Alma
/// (apaga sinal com mais de 90 dias e refaz as medias); 1 vez por subida, o conhecimento do Airbnb no banco. Tudo arquivo local.
pub fn tick(dados: &Path, rot: &Roteador, agora: &Agora, ts: f64, _minuto: u32) {
    // o dossie (contexto.json) ainda nao existe no Rust: so o banco recebe
    AIRBNB.call_once(|| drop(airbnb_conhecimento::gravar(&rot.banco, &|_, _, _| {})));
    let hoje = agora.data.iso();
    if *crate::travar(&PERFIL_DIA) != hoje {
        *crate::travar(&PERFIL_DIA) = hoje;
        alma_aprende::atualizar_perfil(dados, agora);
    }
    let Some(mut par) = voz_livre() else { return };
    let falas = tick_voz(dados, &mut par, agora, ts);
    drop(par); // falar demora: a trava da escuta nao fica presa
    if !falas.is_empty() {
        let dono = crate::dono_de(dados);
        let tocar = crate::aparelho::chave(dados) && !mudo::ativo(dados, ts); // alto-falante so com a chave "voz"
        for t in falas {
            saida::registrar(dados, "alma-voz", &dono, &t);
            if tocar {
                drop(crate::aparelho::falar(dados, rot, &t, "alma"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;

    fn dir(nome: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("braniac-liga-casa-{}-{nome}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn ag(hora: u32) -> Agora {
        Agora { data: Data::civil(2030, 5, 6).unwrap(), hora, min: 0, seg: 0 }
    }

    const T: f64 = 1_900_000_000.0;

    fn diz(d: &Path, par: &mut (Escuta, Sessao), texto: &str) -> Value {
        let fora = Fora { responder: &|t| format!("resposta para: {t}"), llm: &|_| Some("Tô aqui com você.".into()), retrato: &|| Err("sem retrato".into()) };
        frase(d, par, texto, &ag(10), T, &fora)
    }

    #[test]
    fn escuta_acorda_troca_de_agente_e_dorme_por_frase() {
        let (d, mut par) = (dir("escuta"), <(Escuta, Sessao)>::default());
        // dormindo, conversa da casa nao vai ao Jarvis
        let r = diz(&d, &mut par, "que horas a gente almoça hoje");
        assert!(r["texto"].is_null() && r["fala"].is_null() && r["estado"]["estado"] == "dormindo");
        let r = diz(&d, &mut par, "Jarvis, tá acordado?");
        assert!(r["estado"]["estado"] == "acordado" && r["agente"] == "jarvis" && r["fala"] == crate::escuta::SAUDACAO);
        let r = diz(&d, &mut par, "qual é a capital da Bahia");
        assert!(r["fala"] == "resposta para: qual é a capital da Bahia" && r["agente"] == "jarvis");
        let r = diz(&d, &mut par, "quero falar com a Alma");
        assert!(r["estado"]["agente"] == "alma" && r["agente"] == "alma" && r["fala"] == crate::escuta::ALMA_ENTRA);
        let r = diz(&d, &mut par, "hoje o dia foi puxado");
        assert!(r["agente"] == "alma" && r["texto"].as_str().is_some_and(|t| !t.is_empty() && !t.starts_with("resposta para")));
        let r = diz(&d, &mut par, "quero falar com o Jarvis");
        assert!(r["estado"]["agente"] == "jarvis" && r["fala"] == crate::escuta::JARVIS_VOLTA);
        let r = diz(&d, &mut par, "pode descansar");
        assert!(r["estado"]["estado"] == "dormindo" && r["fala"] == crate::escuta::DESPEDIDA);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn no_mudo_a_resposta_vem_escrita_e_nada_e_falado() {
        let (d, mut par) = (dir("mudo"), <(Escuta, Sessao)>::default());
        diz(&d, &mut par, "Jarvis, tá acordado?");
        mudo::mutar(&d, None, T);
        let r = diz(&d, &mut par, "qual é a capital da Bahia");
        assert!(r["fala"].is_null() && r["texto"] == "resposta para: qual é a capital da Bahia" && r["estado"]["mudo"] == true);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn relogio_da_voz_nao_fala_com_o_braniac_pausado_e_expira_a_sessao() {
        let (d, mut par) = (dir("tick"), <(Escuta, Sessao)>::default());
        par.1 = Sessao { ativa: true, nivel: 1, espera_ate: T - 1.0, ..Default::default() };
        assert!(tick_voz(&d, &mut par, &ag(10), T).is_empty());
        assert!(!par.1.ativa); // passou o prazo sem resposta: ela se recolhe
        let (d2, mut par2) = (dir("tick-pausado"), <(Escuta, Sessao)>::default());
        power::desligar(&d2, "teste", "");
        par2.1 = Sessao { ativa: true, nivel: 1, espera_ate: T - 1.0, ..Default::default() };
        assert!(tick_voz(&d2, &mut par2, &ag(8), T).is_empty() && par2.1.ativa); // pausado, o relogio da voz nem roda
        for p in [d, d2] {
            let _ = std::fs::remove_dir_all(&p);
        }
    }

    #[test]
    fn organizar_so_move_e_nao_sobrescreve() {
        let (d, pasta) = (dir("org-dados"), dir("org-pasta"));
        std::fs::write(pasta.join("conta.pdf"), "nova").unwrap();
        std::fs::create_dir_all(pasta.join("Documentos")).unwrap();
        std::fs::write(pasta.join("Documentos").join("conta.pdf"), "antiga").unwrap();
        let ts = fatos::agora_ts() + 3600.0; // o arquivo precisa estar parado ha 10 min
        let r = organizar_pc(&pasta, &d, true, &ag(10), ts);
        assert!(r["movidos"].as_array().unwrap().len() == 1 && pasta.join("conta.pdf").exists()); // simulando, nada sai do lugar
        assert!(!d.join("saida.jsonl").exists() && !d.join("organizador.json").exists());
        let r = organizar_pc(&pasta, &d, false, &ag(10), ts);
        assert_eq!(r["movidos"].as_array().unwrap().len(), 1);
        assert!(!pasta.join("conta.pdf").exists());
        assert_eq!(std::fs::read_to_string(pasta.join("Documentos").join("conta.pdf")).unwrap(), "antiga");
        assert_eq!(std::fs::read_to_string(pasta.join("Documentos").join("conta (1).pdf")).unwrap(), "nova");
        assert!(get_local(&d, "/api/pc/organizar", T).unwrap()["ultimo"]["movidos"].as_array().is_some_and(|m| m.len() == 1));
        for p in [d, pasta] {
            let _ = std::fs::remove_dir_all(&p);
        }
    }

    /// Tudo que foi "avisado": a caixa de saida e a fila do funil (o resumo sai nas horas marcadas).
    fn avisado(d: &Path) -> String {
        std::fs::read_to_string(d.join("saida.jsonl")).unwrap_or_default() + &std::fs::read_to_string(d.join("funil_dono.json")).unwrap_or_default()
    }

    #[test]
    fn alerta_cai_na_caixa_de_saida() {
        let d = dir("alerta");
        // urgente, de dia: sai na hora, e so para a caixa de saida
        assert!(alertar(&d, "guarda", "Possível ataque: muitas falhas de login na última hora.", &ag(10), T).starts_with('✅'));
        let caixa = saida::ultimas(&d, 10);
        assert!(caixa[0]["canal"] == "guarda" && caixa[0]["texto"].as_str().unwrap().contains("ataque"));
        // firewall desligado visto pela guarda: o alerta fica avisado (caixa ou fila do resumo), nunca some
        let ps = |s: &str| if s == guarda::PS_FIREWALL { Ok(r#"[{"Name":"Public","Enabled":0}]"#.to_string()) } else { Err("sem leitura".to_string()) };
        let r = guarda_pc(&d, &ag(10), T, &ps).unwrap();
        assert!(!r["alertas"].as_array().unwrap().is_empty() && avisado(&d).to_lowercase().contains("firewall"));
        // sem leitura nenhuma nada e avaliado; coleta de seguranca ilegivel e erro, nunca "tudo certo"
        assert!(guarda_pc(&d, &ag(10), T, &|_| Err("x".into())).is_err());
        assert!(seguranca_pc(&d, &ag(10), T, &|_| Ok("nada".into())).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn vagas_e_freelas_leem_a_lista_e_avisam_pela_caixa() {
        let d = dir("trabalho");
        let pedidos = RefCell::new(Vec::<String>::new());
        let baixar = |url: &str| {
            pedidos.borrow_mut().push(url.to_string());
            if url.contains("remotive.com") {
                Ok(json!({ "jobs": [{ "id": 1, "title": "Suporte de TI", "company_name": "Empresa Exemplo", "url": "https://vagas.example/1", "candidate_required_location": "Worldwide", "salary": "" }] }).to_string())
            } else if url.contains("remoteok.com") {
                Ok(json!([{ "legal": "aviso" }, { "id": "7", "position": "Build a landing page website", "company": "Loja Exemplo", "description": "Need a simple landing page, paid, fixed price.", "url": "https://freelas.example/7" }]).to_string())
            } else {
                Err("fora do ar".to_string())
            }
        };
        assert_eq!(vagas_buscar(&d, &ag(7), T, &baixar).unwrap()["novas"], 0); // antes das 8h nem busca
        assert!(pedidos.borrow().is_empty());
        assert_eq!(vagas_buscar(&d, &ag(9), T, &baixar).unwrap()["novas"], 1);
        assert!(pedidos.borrow().iter().all(|u| u.starts_with("https://remotive.com/api/remote-jobs?limit=25&search=") && !u.contains(' ')));
        assert!(avisado(&d).contains("Vagas remotas novas"));
        // freelas: so de 6 em 6 horas; uma fonte fora do ar nao derruba a outra
        assert!(freelas_pode(&d, &ag(9), T) && !freelas_pode(&d, &ag(22), T));
        assert!(freelas_buscar(&d, &ag(9), T, &baixar, &|_| json!({ "nota": 9, "motivo": "pequeno e claro", "proposta": "Faço a página." })));
        assert!(!freelas_pode(&d, &ag(10), T + 3600.0) && avisado(&d).contains("Achei freela"));
        assert_eq!(freelas::listar(&d, 7).len(), 1);
        assert_eq!(objeto_em("claro: {\"nota\": 8} pronto")["nota"], 8);
        assert_eq!(objeto_em("sem json"), json!({}));
        let _ = std::fs::remove_dir_all(&d);
    }

    fn rot() -> Arc<Roteador> {
        Arc::new(Roteador {
            cfg: json!({ "cadeia": [] }),
            store: crate::store::Store::em_memoria().unwrap(),
            banco: crate::banco::Banco::em_memoria().unwrap(),
            chamar: Box::new(|_, _, _, _| Ok(String::new())),
            chamar_tools: Box::new(|_, _, _, _, _| Ok(json!({}))),
            contexto: Box::new(|_, _| String::new()),
            vigia: Box::new(|_, _| {}),
            ler_chave: Box::new(|_| String::new()),
        })
    }

    #[test]
    fn nenhuma_rota_de_rede_sobe_servidor() {
        let (d, r) = (dir("rede"), rot());
        let e = rede_estado();
        assert!(e["servidores_no_ar"] == 0 && e["estado"] == SO_O_DONO && e["itens"].as_array().unwrap().len() == REDE.len());
        assert!(e["itens"].as_array().unwrap().iter().all(|i| i["ligado"] == false));
        // nao existe rota POST para ligar nada disso: o pedido nem e deste grupo
        for m in ["rede", "rede-estado", "portal", "porta-lan", "remoto", "par", "elo", "mcp", "taxa"] {
            for acao in ["", "/ligar", "/iniciar"] {
                assert!(post(&d, &r, &format!("/api/{m}{acao}"), &json!({ "ligar": true })).is_none());
            }
        }
        // testes e avaliacao sem arquivo de casos nao rodam nada
        assert_eq!(post(&d, &r, "/api/manutencao/testes", &Value::Null).unwrap().status_code().0, 501);
        assert_eq!(post(&d, &r, "/api/avaliar", &Value::Null).unwrap().status_code().0, 404);
        assert_eq!(post(&d, &r, "/api/escuta/frase", &json!({})).unwrap().status_code().0, 400);
        // leituras sem dado nenhum respondem (e a lista do que falta nao promete nada)
        for rota in ["/falta", "/api/alma/aprende", "/api/airbnb/conhecimento?q=reserva", "/api/habilidades", "/api/aparelhos", "/escuta", "/api/gratis", "/api/nao-ligados", "/api/rede-estado", "/api/agente-dev", "/api/manutencao", "/api/avaliar", "/api/pc/limpeza", "/api/pc/guarda", "/api/pc/seguranca", "/api/pc/organizar"] {
            assert_eq!(get(&d, &r, rota).unwrap().status_code().0, 200, "{rota}");
        }
        assert!(get(&d, &r, "/api/portal").is_none());
        assert!(get_local(&d, "/api/nao-ligados", T).unwrap()["itens"].as_array().unwrap().iter().all(|i| i["ligado"] == false && i["falta"].as_str().is_some_and(|f| !f.is_empty())));
        assert!(crate::travar(&FUNDO).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_lista_do_que_falta_tem_os_quatro_grupos_e_nada_ligado() {
        let (d, r) = (dir("falta"), rot());
        let n = nao_ligados();
        let grupos = n["grupos"].as_array().unwrap();
        assert_eq!(grupos.iter().map(|g| g["grupo"].as_str().unwrap()).collect::<Vec<_>>(), GRUPOS);
        assert!(grupos.iter().all(|g| !g["itens"].as_array().unwrap().is_empty()));
        assert_eq!(grupos.iter().map(|g| g["itens"].as_array().unwrap().len()).sum::<usize>(), NAO_LIGADOS.len());
        assert!(aparelhos()["itens"].as_array().unwrap().iter().all(|i| i["ligado"] == false && i["estado"] == "sem aparelho ligado"));
        assert_eq!(habilidades(&d)["itens"], json!([]));
        // gravar na barra do Claude nao existe nesta rodada; comando do dono so e lido; video sem caminho nao roda nada
        assert_eq!(post(&d, &r, "/api/claude/organizar", &json!({ "simular": false })).unwrap().status_code().0, 501);
        assert_eq!(post(&d, &r, "/api/video/analisar", &json!({})).unwrap().status_code().0, 400);
        assert_eq!(post(&d, &r, "/api/monitor/interpretar", &json!({ "texto": "bom dia" })).unwrap().status_code().0, 200);
        assert!(post(&d, &r, "/api/habilidades", &json!({ "ids": ["x"] })).is_none() && post(&d, &r, "/api/habilidades/instalar", &json!({ "ids": ["x"] })).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn alma_aprende_muda_consentimento_e_so_esquece_com_confirmacao() {
        let (d, a) = (dir("aprende"), ag(10));
        assert_eq!(aprende_estado(&d, &a)["consentimento"], alma_aprende::consentimento(&d));
        assert_eq!(aprende_mudar(&d, &a, &json!({ "sinais": false, "outra": true })).unwrap()["consentimento"]["sinais"], false);
        assert!(!alma_aprende::registrar_sinais(&d, &a, "oi, tudo bem por aqui")); // sem consentimento nada e guardado
        aprende_mudar(&d, &a, &json!({ "sinais": true })).unwrap();
        assert!(alma_aprende::registrar_sinais(&d, &a, "oi, tudo bem por aqui"));
        assert!(aprende_mudar(&d, &a, &json!({})).is_err());
        assert!(aprende_mudar(&d, &a, &json!({ "esquecer": "tudo" })).unwrap_err().contains("Nada foi apagado"));
        assert!(aprende_mudar(&d, &a, &json!({ "esquecer": "o resto", "confirmar": "ESQUECER" })).is_err());
        assert_eq!(aprende_estado(&d, &a)["sabe"]["sinais"], 1);
        let r = aprende_mudar(&d, &a, &json!({ "esquecer": "tudo", "confirmar": "ESQUECER" })).unwrap();
        assert!(r["esquecido"] == "tudo" && r["estado"]["sabe"]["sinais"] == 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn conhecimento_do_airbnb_nunca_fala_em_aceitar_sem_a_regra_das_reservas() {
        let reservas = airbnb_conhecimento::RESERVAS;
        assert!(reservas.contains("NUNCA aceita pedido nem pre-aprova"));
        for q in ["", "airbnb", "o hóspede quer cancelar", "posso aceitar a reserva?", "tem wifi?", "é um quarto?"] {
            let t = airbnb_para(q);
            let junto = t.join(" ").to_lowercase();
            assert!(!t.is_empty() && !junto.contains("pode aceitar") && !junto.contains("pode pre-aprovar"), "{q}");
            assert!(t.contains(&reservas) || !(junto.contains("aceit") || junto.contains("pre-aprov")), "{q}");
        }
        // a politica da plataforma fala no prazo "para aceitar ou recusar": a regra das reservas vai junto
        assert!(!airbnb_conhecimento::para("o hóspede quer cancelar").contains(&reservas) && airbnb_para("o hóspede quer cancelar").contains(&reservas));
        assert!(airbnb_para("bom dia").is_empty());
    }

    #[test]
    fn ensaio_do_projeto_e_do_comando_nao_disparam_nem_gravam() {
        let (d, pastas) = (dir("projeto"), dir("projeto-pastas"));
        let nada = |_: &str| String::new();
        for _ in 0..2 {
            // duas vezes: o ensaio nao gasta a marca de "ja perguntei nas ultimas 24 h"
            let r = projeto_verificar(&d, "como anda o projeto Zafira?", &[pastas.clone()], T, false, &nada);
            assert!(r["decisao"] == "consultar" && r["nome"] == "Zafira" && r["disparou"] == false && r["aprenderia"].is_null());
            assert!(r["pergunta_gpt"].as_str().unwrap().contains("como anda o projeto Zafira?"));
        }
        // frase com dado sensivel nao entra na pergunta que iria a IA online
        let r = projeto_verificar(&d, "o projeto Zafira usa a senha 1234", &[pastas.clone()], T, true, &nada);
        assert!(r["decisao"] == "consultar" && !r["pergunta_gpt"].as_str().unwrap().contains("1234"));
        // a pasta ensinaria o projeto: o fato so e mostrado
        std::fs::create_dir_all(pastas.join("Zafira")).unwrap();
        std::fs::write(pastas.join("Zafira").join("README.md"), "Loja de exemplo.").unwrap();
        let r = projeto_verificar(&d, "como anda o projeto Zafira?", &[pastas.clone()], T, false, &nada);
        assert!(r["decisao"] == "instrucao" && r["aprenderia"]["fonte"] == "pasta" && r["disparou"] == false);
        assert_eq!(projeto_verificar(&d, "bom dia", &[pastas.clone()], T, false, &nada)["decisao"], "nada");
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 0); // nada foi gravado na pasta de dados
        assert_eq!(monitor_dono::interpretar("bom dia"), (None, String::new()));
        for p in [d, pastas] {
            let _ = std::fs::remove_dir_all(&p);
        }
    }

    #[test]
    fn video_so_passa_caminho_completo_de_video_fora_de_pasta_protegida() {
        // fora da pasta temporaria: no Windows ela fica dentro de AppData, que e protegida
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join(format!("braniac-liga-casa-video-{}", std::process::id()));
        let (d, chaves) = (base.join("dados"), base.join(".ssh"));
        for p in [&d, &chaves] {
            std::fs::create_dir_all(p).unwrap();
        }
        for arq in [base.join("v.mp4"), base.join("nota.txt"), d.join("v.MP4"), chaves.join("v.mp4")] {
            std::fs::write(arq, vec![0u8; 200_000]).unwrap();
        }
        let s = |p: PathBuf| p.to_string_lossy().into_owned();
        assert!(video_valido("v.mp4", &d).unwrap_err().contains("caminho completo"));
        assert!(video_valido(&s(base.join("nota.txt")), &d).unwrap_err().contains("só analiso vídeo"));
        assert!(video_valido(&s(base.join("nao-existe.mp4")), &d).unwrap_err().contains("não existe"));
        assert!(video_valido(&s(d.join("v.MP4")), &d).unwrap_err().contains("protegida"));
        assert!(video_valido(&s(chaves.join("v.mp4")), &d).unwrap_err().contains("protegida"));
        assert!(video_valido(&s(d.join("..").join(".ssh").join("v.mp4")), &d).unwrap_err().contains("protegida")); // o ".." nao engana
        let ok = video_valido(&s(base.join("v.mp4")), &d).unwrap();
        let pedido = RefCell::new(Vec::new());
        let r = analisar_video(&ok, &|a| {
            *pedido.borrow_mut() = a.to_vec();
            Ok(r#"{"streams": [{"codec_type": "video", "width": 1080, "height": 1920}, {"codec_type": "audio"}], "format": {"duration": "10.0"}}"#.into())
        })
        .unwrap();
        assert!(r["nota"] == 100 && r["resumo"]["altura"] == 1920 && r["resumo"]["tem_audio"] == true && r["resumo"]["duracao_s"] == 10.0 && r["arquivo"] == "v.mp4");
        let pedido = pedido.into_inner();
        assert!(pedido[0] == "ffprobe" && pedido.last().unwrap().ends_with("v.mp4") && !pedido.last().unwrap().starts_with(r"\\?"));
        assert_eq!(analisar_video(&ok, &|_| Err("não ligado: falta o ffmpeg".into())).unwrap_err(), "não ligado: falta o ffmpeg");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn faxina_nunca_escolhe_aba_de_agente_e_simular_nao_fecha() {
        let abas = json!([
            { "id": "W", "url": "https://web.whatsapp.com/", "agente": "whatsapp" },
            { "id": "W2", "url": "https://web.whatsapp.com/", "agente": "" },
            { "id": "A", "url": "https://exemplo.test/pesquisa", "agente": "garimpeiro" },
            { "id": "L", "url": "https://exemplo.test/parada", "agente": "" },
        ]);
        let (mut vistas, pedidos) = (faxina_abas::Vistas::new(), RefCell::new(Vec::<String>::new()));
        let fechar = |id: &str| {
            pedidos.borrow_mut().push(id.to_string());
            Ok(json!({ "ok": true }))
        };
        // primeira olhada: so comeca a contar
        let r = faxina(&abas, &mut vistas, T, false, &fechar);
        assert!(r["fecharia"] == json!([]) && r["poupadas_de_agente"] == 2 && r["olhadas"] == 2);
        // duas horas depois, simulando (duas vezes): mostra a mesma aba, nao fecha e nao zera o relogio dela
        for _ in 0..2 {
            let antes = vistas.clone();
            let r = faxina(&abas, &mut vistas, T + 7200.0, true, &fechar);
            assert!(r["fecharia"] == json!([{ "id": "L", "url": "https://exemplo.test/parada" }]) && r["fechadas"] == json!([]));
            assert!(pedidos.borrow().is_empty() && vistas == antes);
        }
        // de verdade: so a aba livre e parada; a do WhatsApp (com ou sem agente no mapa) e a do agente ficam
        let r = faxina(&abas, &mut vistas, T + 7200.0, false, &fechar);
        assert!(r["fechadas"] == json!(["L"]) && *pedidos.borrow() == ["L"]);
        // se fechar falha, a aba nao some da conta
        let mut v2 = faxina_abas::Vistas::new();
        faxina(&abas, &mut v2, T, true, &fechar);
        let r = faxina(&abas, &mut v2, T + 7200.0, false, &|_| Err("VM fora do ar".into()));
        assert!(r["fechadas"] == json!([]) && v2.contains_key("L"));
    }

    #[test]
    fn ensaio_da_barra_do_claude_nao_muda_nenhum_arquivo() {
        let local = dir("claude");
        let perfil = local.join("Claude-Teste");
        let (cfg, ficha) = (perfil.join("claude_desktop_config.json"), perfil.join("claude-code-sessions").join("a").join("b").join("local_1.json"));
        std::fs::create_dir_all(ficha.parent().unwrap()).unwrap();
        std::fs::write(&cfg, "{}").unwrap();
        std::fs::write(&ficha, r#"{"sessionId": "s1", "title": "Arrumar o site", "cwd": "C:/x"}"#).unwrap();
        let r = claude_simular(&local, &ag(10));
        let mudaria = r["mudaria"].as_array().unwrap();
        assert!(r["perfis"] == 1 && r["gravado"] == false && mudaria.len() == 2, "{r}");
        assert!(mudaria[0].as_str().unwrap().contains(organiza_claude::PARADA) && mudaria[1].as_str().unwrap().starts_with("seção "));
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), "{}");
        assert_eq!(std::fs::read_to_string(&ficha).unwrap(), r#"{"sessionId": "s1", "title": "Arrumar o site", "cwd": "C:/x"}"#);
        assert_eq!(std::fs::read_dir(&perfil).unwrap().count(), 2); // nenhuma copia, nenhum .tmp
        assert_eq!(std::fs::read_dir(&local).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&local);
    }
}
