//! GET /api/pc/metricas: estado do PC para o Início do painel (so leitura, sem janela de console).
//! CPU% e RAM% vem do cache de `liga_telas::termico`; GPU (temp, uso, memoria) do nvidia-smi; disco por GetDiskFreeSpaceExW.
//! Sensor ausente = null, nunca numero inventado. Rede: sem rota, fica de fora.
use crate::{dado, Resp};
use serde_json::{json, Value};
use std::sync::Mutex;

static CACHE: Mutex<Option<(f64, Value)>> = Mutex::new(None);

/// Saida de `nvidia-smi --query-gpu=temperature.gpu,utilization.gpu,memory.used,memory.total --format=csv,noheader,nounits`.
pub fn parse_nvidia(saida: &str) -> Option<[f64; 4]> {
    let n: Vec<f64> = saida.lines().next()?.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    (n.len() == 4 && n.iter().all(|v| v.is_finite())).then(|| [n[0], n[1], n[2], n[3]])
}

#[cfg(windows)]
fn disco(letra: char) -> Option<(f64, f64)> {
    extern "system" {
        fn GetDiskFreeSpaceExW(dir: *const u16, livre: *mut u64, total: *mut u64, livre_total: *mut u64) -> i32;
    }
    let dir: Vec<u16> = format!("{letra}:\\").encode_utf16().chain([0]).collect();
    let (mut livre, mut total, mut lt) = (0u64, 0u64, 0u64);
    let ok = unsafe { GetDiskFreeSpaceExW(dir.as_ptr(), &mut livre, &mut total, &mut lt) };
    (ok != 0 && total > 0).then(|| (livre as f64 / 1e9, total as f64 / 1e9))
}
#[cfg(not(windows))]
fn disco(_: char) -> Option<(f64, f64)> {
    None
}

fn arred(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

fn medir() -> Value {
    let t = crate::liga_telas::termico();
    let gpu = crate::oculto::cmd("nvidia-smi")
        .args(["--query-gpu=temperature.gpu,utilization.gpu,memory.used,memory.total", "--format=csv,noheader,nounits"])
        .output().ok().and_then(|o| parse_nvidia(&String::from_utf8_lossy(&o.stdout)));
    let discos: Vec<Value> = ['C', 'E'].iter().filter_map(|&l| disco(l).map(|(livre, total)| json!({
        "letra": format!("{l}:"), "livre_gb": arred(livre), "total_gb": arred(total), "usado_pct": arred(100.0 * (1.0 - livre / total)) }))).collect();
    json!({
        "pronto": true, "cpu_pct": t["cpu_pct"], "ram_pct": t["ram_pct"], "cpu_c": t["cpu_c"],
        "gpu": gpu.map(|g| json!({ "temp_c": g[0], "uso_pct": g[1], "mem_usada_mb": g[2], "mem_total_mb": g[3] })),
        "discos": discos, "rede": null,
    })
}

/// Leitura com cache de 4 s: abas e pedidos seguidos nao abrem varios nvidia-smi.
pub fn metricas(agora: f64) -> Value {
    let mut g = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((t, v)) = &*g {
        if agora - t < 4.0 {
            return v.clone();
        }
    }
    let v = medir();
    *g = Some((agora, v.clone()));
    v
}

pub fn get(url: &str) -> Option<Resp> {
    (url.split('?').next() == Some("/api/pc/metricas")).then(|| dado(metricas(crate::fatos::agora_ts())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_a_saida_do_nvidia_smi_e_recusa_lixo() {
        assert_eq!(parse_nvidia("51, 7, 215, 4096\n"), Some([51.0, 7.0, 215.0, 4096.0]));
        assert_eq!(parse_nvidia("[N/A], 7, 215, 4096"), None);
        assert_eq!(parse_nvidia(""), None);
    }

    #[test]
    fn rota_de_metricas_devolve_cpu_ram_gpu_e_discos_sem_inventar() {
        let r = metricas(1.0e12);
        for k in ["cpu_pct", "ram_pct", "gpu", "discos", "rede"] {
            assert!(r.get(k).is_some(), "{k}");
        }
        assert!(r["discos"].is_array());
        assert!(get("/api/pc/metricas?x=1").is_some() && get("/api/pc/outra").is_none());
    }
}
