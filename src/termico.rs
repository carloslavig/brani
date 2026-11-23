//! Monitor termico do notebook (porte de braniac/termico.py): so o parse das leituras, os limites e o texto do aviso.
//! Ficou de fora (entra pronto, como `Leitura` ou texto): rodar nvidia-smi, PowerShell/WMI e medir CPU/RAM/processos, e o
//! cache em segundo plano da tela de voz. O aviso sai por closure, no maximo um por hora; a hora do ultimo aviso fica com
//! quem chama (`ultimo_alerta`), como a variavel de memoria do Python. Historico em dados/termico.csv.
#![allow(dead_code)]
use crate::tempo::Agora;
use std::io::Write;
use std::path::Path;

pub const GPU_ALERTA: f64 = 80.0; // C
pub const CPU_ALERTA: f64 = 90.0; // C (so com sensor)
pub const CARGA_ALERTA: f64 = 90.0; // % de CPU em 3 leituras seguidas quando nao ha sensor
pub const COOLDOWN_S: f64 = 3600.0;

#[derive(Clone, Debug, Default)]
pub struct Leitura {
    pub gpu_c: Option<f64>,
    pub cpu_c: Option<f64>,
    pub cpu_pct: f64,
    pub ram_pct: f64,
    /// Quem mais usa o processador (ate 3 nomes).
    pub top: Vec<String>,
}

/// Saida de `nvidia-smi --query-gpu=temperature.gpu --format=csv,noheader,nounits`: vale o primeiro numero.
pub fn parse_gpu(saida: &str) -> Option<f64> {
    saida.split_whitespace().next()?.parse().ok().filter(|v: &f64| v.is_finite())
}

/// Saida do sensor de CPU (LibreHardwareMonitor/OpenHardwareMonitor via WMI): um numero, com virgula ou ponto; vazio = sem sensor.
pub fn parse_cpu(saida: &str) -> Option<f64> {
    saida.trim().replace(',', ".").parse().ok().filter(|v: &f64| v.is_finite())
}

/// Texto do problema ou "". Pura. `recentes` = % de CPU das ultimas leituras do historico (a atual inclusive).
/// Com sensor e fresco, a carga sozinha nao alarma; leitura 0 conta como "sem leitura" (igual ao Python).
pub fn motivo(d: &Leitura, recentes: &[f64]) -> String {
    if let Some(g) = d.gpu_c.filter(|g| *g != 0.0 && *g >= GPU_ALERTA) {
        return format!("a placa de vídeo está em {g:.0} graus");
    }
    if let Some(c) = d.cpu_c.filter(|c| *c != 0.0 && *c >= CPU_ALERTA) {
        return format!("o processador está em {c:.0} graus");
    }
    if d.cpu_c.is_none() && recentes.len() >= 3 && recentes.iter().all(|p| *p >= CARGA_ALERTA) && d.cpu_pct >= CARGA_ALERTA {
        return format!("o processador está a {:.0}% de uso há muito tempo (sem sensor de temperatura, é o que dá para medir)", d.cpu_pct);
    }
    String::new()
}

pub fn quente(d: &Leitura) -> bool {
    !motivo(d, &[]).is_empty()
}

pub fn resumo(d: &Leitura) -> String {
    let gpu = d.gpu_c.filter(|g| *g != 0.0).map_or("placa de vídeo sem leitura".to_string(), |g| format!("placa de vídeo {g:.0} graus"));
    let cpu = d.cpu_c.filter(|c| *c != 0.0).map_or(format!("processador a {:.0}% de uso (sem sensor de temperatura)", d.cpu_pct), |c| format!("processador {c:.0} graus"));
    format!("{gpu}, {cpu}, memória {:.0}%. Quem mais usa: {}.", d.ram_pct, d.top.join(", "))
}

/// Acrescenta a leitura ao historico (cabecalho na primeira vez). Sem leitura = campo vazio.
pub fn registrar(dados: &Path, agora: &Agora, d: &Leitura) {
    let arq = dados.join("termico.csv");
    let novo = !arq.exists();
    let _ = std::fs::create_dir_all(dados);
    let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(arq) else { return };
    if novo {
        let _ = writeln!(f, "hora,gpu_c,cpu_c,cpu_pct,ram_pct");
    }
    let op = |v: Option<f64>| v.map(|x| x.to_string()).unwrap_or_default();
    let _ = writeln!(f, "{} {},{},{},{},{}", agora.data.iso(), agora.hm(), op(d.gpu_c), op(d.cpu_c), d.cpu_pct, d.ram_pct);
}

/// % de CPU das ultimas `n` leituras do historico (campo vazio ou torto conta 0).
pub fn recentes(dados: &Path, n: usize) -> Vec<f64> {
    let txt = std::fs::read_to_string(dados.join("termico.csv")).unwrap_or_default();
    let l: Vec<f64> = txt.lines().skip(1).filter(|l| !l.trim().is_empty()).map(|l| l.split(',').nth(3).and_then(|x| x.trim().parse().ok()).unwrap_or(0.0)).collect();
    l[l.len().saturating_sub(n)..].to_vec()
}

/// Chamado a cada ~10 min com a leitura pronta. Registra e, se esquentou e passou 1 h do ultimo aviso, avisa o dono.
/// Devolve o motivo quando avisou, senao "". `avisar` e quem decide o canal (deve passar pelo funil).
pub fn verificar(dados: &Path, agora: &Agora, ts: f64, d: &Leitura, nome_dono: &str, ultimo_alerta: &mut f64, avisar: &dyn Fn(&str)) -> String {
    registrar(dados, agora, d);
    let m = motivo(d, &recentes(dados, 3));
    if m.is_empty() || ts - *ultimo_alerta <= COOLDOWN_S {
        return String::new();
    }
    *ultimo_alerta = ts;
    avisar(&format!("🌡 {nome_dono}, o notebook está esquentando: {m}. Quem mais usa: {}. Se der, coloque o notebook em lugar mais ventilado ou numa base elevada.", d.top.join(", ")));
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;
    use std::cell::RefCell;

    fn d(gpu: f64, cpu_c: Option<f64>, pct: f64) -> Leitura {
        Leitura { gpu_c: Some(gpu), cpu_c, cpu_pct: pct, ram_pct: 40.0, top: vec!["jogo.exe".into(), "editor.exe".into()] }
    }

    #[test]
    fn motivo_respeita_os_limites() {
        assert!(motivo(&d(85.0, None, 10.0), &[]).contains("placa") && motivo(&d(80.0, None, 10.0), &[]).contains("80 graus"));
        assert!(motivo(&d(50.0, Some(95.0), 10.0), &[]).contains("processador está em 95 graus"));
        assert_eq!(motivo(&d(79.9, Some(89.9), 10.0), &[]), "");
        let alto = [95.0; 3];
        assert!(motivo(&d(50.0, None, 95.0), &alto).contains("95% de uso"));
        assert_eq!(motivo(&d(50.0, Some(60.0), 95.0), &alto), ""); // com sensor e fresco, a carga sozinha nao alarma
        assert_eq!(motivo(&d(50.0, None, 95.0), &[95.0, 95.0]), ""); // menos de 3 leituras
        assert_eq!(motivo(&d(50.0, None, 95.0), &[95.0, 20.0, 95.0]), "");
        assert_eq!(motivo(&d(50.0, None, 50.0), &alto), "");
        assert!(quente(&d(90.0, None, 0.0)) && !quente(&d(50.0, None, 99.0)));
    }

    #[test]
    fn parse_das_leituras_e_resumo() {
        assert_eq!((parse_gpu("67\n"), parse_gpu("71\n65\n"), parse_gpu(""), parse_gpu("N/A")), (Some(67.0), Some(71.0), None, None));
        assert_eq!((parse_cpu(" 72,5\r\n"), parse_cpu("80.25"), parse_cpu("  "), parse_cpu("erro"), parse_cpu("NaN")), (Some(72.5), Some(80.25), None, None, None));
        assert_eq!(resumo(&d(61.4, Some(70.0), 12.0)), "placa de vídeo 61 graus, processador 70 graus, memória 40%. Quem mais usa: jogo.exe, editor.exe.");
        let sem = Leitura { cpu_pct: 33.0, ram_pct: 51.0, ..Default::default() };
        assert_eq!(resumo(&sem), "placa de vídeo sem leitura, processador a 33% de uso (sem sensor de temperatura), memória 51%. Quem mais usa: .");
    }

    #[test]
    fn verificar_registra_e_avisa_no_maximo_uma_vez_por_hora() {
        let p = std::env::temp_dir().join(format!("braniac-termico-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        let agora = Agora { data: Data::civil(2026, 11, 16).unwrap(), hora: 10, min: 5, seg: 0 };
        let (avisos, mut ult, ts) = (RefCell::new(Vec::new()), 0.0, 1_800_000_000.0);
        let avisar = |t: &str| avisos.borrow_mut().push(t.to_string());
        assert_eq!(verificar(&p, &agora, ts, &d(50.0, None, 95.0), "Dono", &mut ult, &avisar), "");
        assert_eq!(verificar(&p, &agora, ts + 600.0, &Leitura { cpu_pct: 96.0, ..Default::default() }, "Dono", &mut ult, &avisar), "");
        // terceira leitura seguida acima de 90%: agora avisa
        assert!(verificar(&p, &agora, ts + 1200.0, &d(50.0, None, 97.0), "Dono", &mut ult, &avisar).contains("97% de uso"));
        assert_eq!(verificar(&p, &agora, ts + 1800.0, &d(88.0, None, 97.0), "Dono", &mut ult, &avisar), ""); // dentro de 1 h: calado
        assert!(verificar(&p, &agora, ts + 1200.0 + COOLDOWN_S + 1.0, &d(88.0, None, 10.0), "Dono", &mut ult, &avisar).contains("placa"));
        let a = avisos.borrow();
        assert!(a.len() == 2 && a[0].starts_with("🌡 Dono, o notebook está esquentando: o processador está a 97% de uso") && a[0].contains("Quem mais usa: jogo.exe, editor.exe."));
        let csv = std::fs::read_to_string(p.join("termico.csv")).unwrap();
        let l: Vec<&str> = csv.lines().collect();
        assert!(l.len() == 6 && l[0] == "hora,gpu_c,cpu_c,cpu_pct,ram_pct" && l[1] == "2026-11-16 10:05,50,,95,40" && l[2] == "2026-11-16 10:05,,,96,0");
        assert_eq!(recentes(&p, 3), vec![97.0, 97.0, 10.0]);
        let _ = std::fs::remove_dir_all(&p);
    }
}
