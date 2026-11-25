//! O Jarvis fala quando o Dono ABRE o aplicativo (pedido de 24/11/2026).
//! Fase 1 (so se o sistema ainda nao estava de pe): "Boa tarde" + pausa de 0,5 s + "senhor."; se a carga passa de 5 s, "Sistemas carregando." uma vez.
//! Fase 3 (a tela de abertura chega a 100%, ou o painel avisa que o Inicio apareceu; vale o primeiro): resumo curto do PC e, se houver pendencia aberta, "O senhor tem algumas pendencias para visualizar." (sem dizer quais).
//! Quem chama: o app Tauri (POST /api/saudacao/abrir, so na abertura do app; F5, reinicio do servidor e miniatura nao passam por aqui) e o painel (POST /api/saudacao/pronto).
//! Chave: `saudacao_voz` em dados/saudacao.json (padrao ligada). Erro de audio nunca aparece nem se repete. A decisao e feita por funcoes puras.
use crate::roteador::Roteador;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const INTERVALO_S: f64 = 600.0; // nao repete se o app abriu ha menos de 10 min
pub const LIMITE_CARGA_S: f64 = 5.0; // demorou mais que isso sem o painel pronto: "Sistemas carregando."
const VALIDADE_S: f64 = 180.0; // o "pronto" so vale ate 3 min depois da abertura
const PAUSA_MS: u64 = 500;

pub fn saudacao_hora(hora: u32) -> &'static str {
    match hora {
        0..=4 => "Boa madrugada",
        5..=11 => "Bom dia",
        12..=17 => "Boa tarde",
        _ => "Boa noite",
    }
}

/// Fase 1: as duas frases curtas (tocadas com pausa entre elas). None = sistema ja estava carregado: nao diz nada.
pub fn frases_abertura(ja_estava: bool, hora: u32) -> Option<[String; 2]> {
    (!ja_estava).then(|| [saudacao_hora(hora).to_string(), "senhor.".to_string()])
}

/// "Sistemas carregando." so quando o sistema nao estava pronto, a carga passou do limite, o painel ainda nao abriu e ainda nao foi dito.
pub fn avisar_carregando(ja_estava: bool, decorrido_s: f64, pronto: bool, ja_dito: bool) -> bool {
    !ja_estava && !pronto && !ja_dito && decorrido_s >= LIMITE_CARGA_S
}

/// Tudo o que pode calar a saudacao.
pub struct Portoes {
    pub ligada: bool,
    pub mudo: bool,
    pub servidor_pausado: bool,
    pub mic_ligado: bool,
    pub falando: bool,
    pub ultima_abertura: f64,
    pub agora: f64,
}

pub fn pode_falar(p: &Portoes) -> bool {
    p.ligada && !p.mudo && !p.servidor_pausado && p.mic_ligado && !p.falando
}

/// A abertura conta (e arma o resumo) se nao e repeticao de menos de 10 min. Se a fala for calada por outro motivo, a abertura ainda vale.
pub fn abertura_nova(ultima: f64, agora: f64) -> bool {
    ultima <= 0.0 || agora - ultima >= INTERVALO_S || agora < ultima
}

/// Frase do estado do PC a partir de /api/pc/metricas (CPU, RAM, temperaturas, disco). Limites sensatos; no maximo dois problemas.
pub fn frase_pc(m: &Value) -> String {
    let n = |v: &Value| v.as_f64();
    let mut ruins: Vec<&str> = Vec::new();
    if n(&m["gpu"]["temp_c"]).is_some_and(|t| t >= 80.0) {
        ruins.push("A GPU está quente");
    }
    if n(&m["cpu_c"]).is_some_and(|t| t >= 85.0) {
        ruins.push("O processador está quente");
    }
    if n(&m["ram_pct"]).is_some_and(|r| r >= 85.0) {
        ruins.push("A memória está alta");
    }
    if n(&m["cpu_pct"]).is_some_and(|c| c >= 90.0) {
        ruins.push("O processador está sobrecarregado");
    }
    if m["discos"].as_array().is_some_and(|d| d.iter().any(|x| n(&x["usado_pct"]).is_some_and(|u| u >= 92.0))) {
        ruins.push("O disco está quase cheio");
    }
    ruins.truncate(2);
    if ruins.is_empty() { "O computador está bem.".into() } else { format!("{}.", ruins.join(". ")) }
}

/// Quantas pendencias nao feitas existem em dados/projetos.json.
pub fn pendencias_abertas(projetos: &Value) -> usize {
    projetos["projetos"].as_array().map_or(0, |ps| ps.iter().map(|p| p["pendencias"].as_array().map_or(0, |l| l.iter().filter(|x| x["feita"] != true).count())).sum())
}

/// Fase 3: o resumo inteiro (2 frases no maximo). Sem pendencias, nao menciona; com, nunca diz quais.
pub fn resumo(m: &Value, pendencias: usize) -> String {
    let mut t = frase_pc(m);
    if pendencias > 0 {
        t.push_str(" O senhor tem algumas pendências para visualizar.");
    }
    t
}

// ---------- estado e rotas ----------

#[derive(Default)]
struct Estado {
    armado: bool,
    abriu: f64,
    ja_estava: bool,
    pronto: bool,
    carregando_dito: bool,
    resumo_feito: bool,
}
static E: Mutex<Estado> = Mutex::new(Estado { armado: false, abriu: 0.0, ja_estava: false, pronto: false, carregando_dito: false, resumo_feito: false });
static FALA: Mutex<()> = Mutex::new(()); // uma saudacao por vez, na ordem

fn ler(dados: &Path) -> Value {
    std::fs::read_to_string(dados.join("saudacao.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}))
}

fn gravar(dados: &Path, campo: &str, v: Value) {
    let mut d = ler(dados);
    d[campo] = v;
    let _ = std::fs::write(dados.join("saudacao.json"), d.to_string());
}

fn portoes(dados: &Path, ts: f64) -> Portoes {
    let c = ler(dados);
    Portoes {
        ligada: c["saudacao_voz"] != false,
        mudo: crate::mudo::ativo(dados, ts),
        servidor_pausado: crate::power::pausado(dados),
        mic_ligado: crate::aparelho::MIC_LIGADO.load(std::sync::atomic::Ordering::SeqCst),
        falando: crate::aparelho::estado()["falando"] == true,
        ultima_abertura: c["ultima_abertura"].as_f64().unwrap_or(0.0),
        agora: ts,
    }
}

fn agora_s() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

/// Fala uma ou mais frases (a voz escolhida para o Jarvis), gerando tudo antes para a pausa ser so de 0,5 s. Falha: so registra no log.
fn dizer(dados: &Path, rot: &Roteador, frases: &[String]) {
    let wavs: Result<Vec<_>, _> = frases.iter().map(|f| crate::aparelho::gerar(dados, rot, f, "jarvis")).collect();
    match wavs {
        Ok(ws) => {
            for (i, w) in ws.into_iter().enumerate() {
                if i > 0 {
                    std::thread::sleep(Duration::from_millis(PAUSA_MS));
                }
                if crate::aparelho::tocar_wav(w).is_err() {
                    return;
                }
            }
        }
        Err(e) => eprintln!("saudacao: sem voz ({e})"),
    }
}

fn fase1(dados: &Path, rot: &Roteador, ja_estava: bool, esperou_s: f64) {
    let _g = crate::travar(&FALA);
    if let Some(f) = frases_abertura(ja_estava, crate::tempo::agora().hora) {
        dizer(dados, rot, &f);
    }
    // espera o painel ate o limite; passou sem "pronto": avisa uma vez
    let t0 = agora_s();
    loop {
        let (pronto, dito) = { let e = crate::travar(&E); (e.pronto, e.carregando_dito) };
        let decorrido = esperou_s + (agora_s() - t0);
        if avisar_carregando(ja_estava, decorrido, pronto, dito) {
            crate::travar(&E).carregando_dito = true;
            if pode_falar(&portoes(dados, agora_s())) {
                dizer(dados, rot, &["Sistemas carregando.".to_string()]);
            }
            return;
        }
        if ja_estava || pronto || dito || decorrido > 60.0 {
            return;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn fase3(dados: &Path, rot: &Roteador) {
    let _g = crate::travar(&FALA);
    let ts = agora_s();
    if !pode_falar(&Portoes { falando: false, ..portoes(dados, ts) }) {
        return;
    }
    let m = crate::pc_metricas::metricas(ts);
    let p = std::fs::read_to_string(dados.join("projetos.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).map_or(0, |v| pendencias_abertas(&v));
    dizer(dados, rot, &[resumo(&m, p)]);
}

// ---------- boot do Windows ----------
// O atalho da pasta Inicializar chama o lancador com /logon, que cria dados/logon.flag. A hora do arquivo e o "boot": reiniciar so o servidor
// nao mexe nele (nao fala); reboot/logon novo cria de novo. Vale o uptime do Windows nao (Inicializacao rapida nao zera o contador).
pub const BOOT_JANELA_S: f64 = 900.0; // flag com mais de 15 min: o Dono ja esta usando o PC, nada de "inicializando"
const BOOT_ESPERA_S: u64 = 20; // tempo entre "inicializando" e "Sistemas no ar"

pub fn deve_saudar_boot(ligada: bool, flag_ts: f64, ultimo_boot: f64, agora: f64) -> bool {
    ligada && flag_ts > 0.0 && flag_ts != ultimo_boot && agora >= flag_ts && agora - flag_ts <= BOOT_JANELA_S
}

pub fn frases_boot(hora: u32) -> [String; 2] {
    [saudacao_hora(hora).to_string(), "senhor Dono.".to_string()]
}

pub fn fechamento(resumo: &str) -> String {
    format!("Sistemas no ar. {resumo}")
}

fn flag_ts(dados: &Path) -> f64 {
    std::fs::metadata(dados.join("logon.flag")).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0.0, |d| d.as_secs_f64())
}

/// Chamado uma vez quando o servidor sobe. So fala se houver um boot novo ainda nao saudado.
pub fn boot(dados: &Path, rot: Arc<Roteador>) {
    let ts = agora_s();
    let (c, flag) = (ler(dados), flag_ts(dados));
    if !deve_saudar_boot(c["saudacao_boot"] != false, flag, c["ultimo_boot"].as_f64().unwrap_or(0.0), ts) {
        return;
    }
    gravar(dados, "ultimo_boot", json!(flag)); // marcado de antemao: nem silencio nem falha fazem repetir
    gravar(dados, "ultima_abertura", json!(ts)); // abrir o app logo depois nao saúda de novo
    let d = dados.to_path_buf();
    std::thread::spawn(move || {
        let livre = |d: &Path| pode_falar(&Portoes { ligada: true, ..portoes(d, agora_s()) });
        let mut espera = 0;
        while !livre(&d) && espera < 60 {
            std::thread::sleep(Duration::from_secs(1)); // outro audio em curso: espera a vez
            espera += 1;
        }
        if !livre(&d) {
            return;
        }
        {
            let _g = crate::travar(&FALA);
            dizer(&d, &rot, &frases_boot(crate::tempo::agora().hora));
            dizer(&d, &rot, &["Estou inicializando os processos.".to_string()]);
        }
        std::thread::sleep(Duration::from_secs(BOOT_ESPERA_S));
        let _g = crate::travar(&FALA);
        if !livre(&d) {
            return;
        }
        let ts = agora_s();
        let m = crate::pc_metricas::metricas(ts);
        let p = std::fs::read_to_string(d.join("projetos.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).map_or(0, |v| pendencias_abertas(&v));
        dizer(&d, &rot, &[fechamento(&resumo(&m, p))]);
    });
}

pub fn post(dados: &Path, rot: &Arc<Roteador>, caminho: &str, v: &Value) -> Option<crate::Resp> {
    let ts = agora_s();
    Some(match caminho {
        "/api/saudacao/abrir" => {
            let p = portoes(dados, ts);
            let nova = abertura_nova(p.ultima_abertura, ts);
            let ja_estava = v["ja_estava"] == true;
            let esperou = v["esperou_s"].as_f64().unwrap_or(0.0).max(0.0);
            let arma = nova && p.ligada && !p.servidor_pausado && p.mic_ligado && !p.mudo;
            if nova {
                gravar(dados, "ultima_abertura", json!(ts));
            }
            *crate::travar(&E) = Estado { armado: arma, abriu: ts, ja_estava, ..Default::default() };
            let falar = arma && pode_falar(&p);
            if arma {
                let (d, r) = (dados.to_path_buf(), rot.clone());
                std::thread::spawn(move || fase1(&d, &r, ja_estava || !falar, esperou));
            }
            crate::dado(json!({ "armado": arma, "fala_abertura": falar && !ja_estava }))
        }
        "/api/saudacao/pronto" => {
            let vai = {
                let mut e = crate::travar(&E);
                e.pronto = true;
                let vai = e.armado && !e.resumo_feito && ts - e.abriu < VALIDADE_S;
                if vai {
                    e.resumo_feito = true; // uma vez por abertura: F5 e novas leituras do painel nao repetem
                }
                vai
            };
            if vai {
                let (d, r) = (dados.to_path_buf(), rot.clone());
                std::thread::spawn(move || fase3(&d, &r));
            }
            crate::dado(json!({ "resumo": vai }))
        }
        _ => return None,
    })
}

#[cfg(test)]
mod testes {
    use super::*;

    fn ok() -> Portoes {
        Portoes { ligada: true, mudo: false, servidor_pausado: false, mic_ligado: true, falando: false, ultima_abertura: 0.0, agora: 10_000.0 }
    }

    #[test]
    fn hora_vira_saudacao() {
        assert_eq!(saudacao_hora(5), "Bom dia");
        assert_eq!(saudacao_hora(11), "Bom dia");
        assert_eq!(saudacao_hora(12), "Boa tarde");
        assert_eq!(saudacao_hora(17), "Boa tarde");
        assert_eq!(saudacao_hora(18), "Boa noite");
        assert_eq!(saudacao_hora(23), "Boa noite");
        assert_eq!(saudacao_hora(0), "Boa madrugada");
        assert_eq!(saudacao_hora(4), "Boa madrugada");
    }

    #[test]
    fn abertura_diz_duas_frases_e_sistema_ja_carregado_fica_mudo() {
        assert_eq!(frases_abertura(false, 15), Some(["Boa tarde".to_string(), "senhor.".to_string()]));
        assert_eq!(frases_abertura(true, 15), None);
    }

    #[test]
    fn demora_diz_sistemas_carregando_uma_vez() {
        assert!(!avisar_carregando(false, 3.0, false, false));
        assert!(avisar_carregando(false, 5.0, false, false));
        assert!(!avisar_carregando(false, 9.0, false, true)); // so uma vez
        assert!(!avisar_carregando(false, 9.0, true, false)); // painel ja abriu
        assert!(!avisar_carregando(true, 9.0, false, false)); // ja estava carregado
    }

    #[test]
    fn resumo_do_pc_usa_limites() {
        let bom = json!({ "cpu_pct": 20, "ram_pct": 50, "cpu_c": 60, "gpu": { "temp_c": 55 }, "discos": [{ "usado_pct": 70 }] });
        assert_eq!(frase_pc(&bom), "O computador está bem.");
        assert_eq!(frase_pc(&json!({ "ram_pct": 90 })), "A memória está alta.");
        assert_eq!(frase_pc(&json!({ "gpu": { "temp_c": 83 } })), "A GPU está quente.");
        assert_eq!(frase_pc(&json!({ "gpu": { "temp_c": 83 }, "ram_pct": 90, "cpu_c": 95 })), "A GPU está quente. O processador está quente.");
        assert_eq!(frase_pc(&json!({ "gpu": null })), "O computador está bem."); // sem leitura nao inventa problema
    }

    #[test]
    fn resumo_avisa_pendencias_sem_listar_quais() {
        let proj = json!({ "projetos": [{ "pendencias": [{ "texto": "SEGREDO-X", "feita": false }, { "texto": "b", "feita": true }] }, { "pendencias": [] }] });
        assert_eq!(pendencias_abertas(&proj), 1);
        let t = resumo(&json!({}), 1);
        assert_eq!(t, "O computador está bem. O senhor tem algumas pendências para visualizar.");
        assert!(!t.contains("SEGREDO"));
        assert_eq!(resumo(&json!({}), 0), "O computador está bem."); // sem pendencia nao menciona
        assert_eq!(pendencias_abertas(&json!({ "projetos": [{ "pendencias": [{ "feita": true }] }] })), 0);
    }

    #[test]
    fn intervalo_minimo_de_dez_minutos() {
        assert!(abertura_nova(0.0, 1000.0));
        assert!(!abertura_nova(1000.0, 1000.0 + 599.0));
        assert!(abertura_nova(1000.0, 1000.0 + 600.0));
    }

    #[test]
    fn respeita_silencio_pausa_microfone_chave_e_fala_em_curso() {
        assert!(pode_falar(&ok()));
        assert!(!pode_falar(&Portoes { mudo: true, ..ok() }));
        assert!(!pode_falar(&Portoes { servidor_pausado: true, ..ok() }));
        assert!(!pode_falar(&Portoes { mic_ligado: false, ..ok() }));
        assert!(!pode_falar(&Portoes { ligada: false, ..ok() }));
        assert!(!pode_falar(&Portoes { falando: true, ..ok() }));
    }

    #[test]
    fn chave_padrao_ligada_e_desliga_com_false() {
        let d = std::env::temp_dir().join(format!("braniac-saud-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        let _ = std::fs::remove_file(d.join("saudacao.json"));
        assert!(portoes(&d, 1.0).ligada);
        gravar(&d, "saudacao_voz", json!(false));
        assert!(!portoes(&d, 1.0).ligada);
    }

    #[test]
    fn boot_novo_fala_repetido_nao_e_servidor_reiniciado_nao_e_flag_velha_nao() {
        let agora = 100_000.0;
        assert!(deve_saudar_boot(true, agora - 60.0, 0.0, agora)); // boot novo fala
        assert!(!deve_saudar_boot(true, agora - 60.0, agora - 60.0, agora)); // mesmo boot ja saudado (servidor reiniciado)
        assert!(!deve_saudar_boot(true, agora - 3600.0, 0.0, agora)); // flag velha: o Dono ja esta usando o PC, sem "inicializando"
        assert!(!deve_saudar_boot(true, 0.0, 0.0, agora)); // sem flag (reinicio do servidor por sessao/bat): nunca
        assert!(!deve_saudar_boot(false, agora - 60.0, 0.0, agora)); // chave saudacao_boot desligada
        assert_eq!(frases_boot(0), ["Boa madrugada".to_string(), "senhor Dono.".to_string()]);
        assert_eq!(fechamento("O computador está bem."), "Sistemas no ar. O computador está bem.");
    }
}
