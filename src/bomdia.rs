//! Bom dia diario: uma vez por dia, a partir das 6h, o Jarvis fala o resumo: o que foi feito (relatorio), a agenda de hoje e o disco.
//! O relatorio (dados/relatorio.md) recebe uma linha de cada tarefa terminada; depois de entregue vai para o historico e recomeca.
use crate::para_fala::para_fala;
use crate::tempo::Agora;
use crate::agenda;
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;

const HORA: u32 = 6;

/// Anota uma linha no relatorio do proximo bom dia.
pub fn registrar(dados: &Path, agora: &Agora, texto: &str) -> bool {
    let (_, m, d) = agora.data.partes();
    let linha = format!("- {d:02}/{m:02} {} · {}", agora.hm(), texto.trim().replace('\n', " "));
    !texto.trim().is_empty()
        && std::fs::OpenOptions::new().create(true).append(true).open(dados.join("relatorio.md")).and_then(|mut f| writeln!(f, "{linha}")).is_ok()
}

#[cfg(windows)]
fn gigas_livres(letra: char) -> Option<u64> {
    extern "system" {
        fn GetDiskFreeSpaceExW(dir: *const u16, livre: *mut u64, total: *mut u64, livre_total: *mut u64) -> i32;
    }
    let caminho: Vec<u16> = format!("{letra}:\\").encode_utf16().chain(Some(0)).collect();
    let mut livre = 0u64;
    // SAFETY: caminho termina em zero e a funcao so preenche `livre`; os outros ponteiros podem ser nulos.
    let ok = unsafe { GetDiskFreeSpaceExW(caminho.as_ptr(), &mut livre, std::ptr::null_mut(), std::ptr::null_mut()) };
    (ok != 0).then_some(livre / 1_000_000_000)
}

#[cfg(not(windows))]
fn gigas_livres(_: char) -> Option<u64> {
    None
}

/// O bom dia como FALA: texto corrido e curto, sem link nem lista. O nome da pessoa vem de dados/pessoal.json ("nome").
pub fn fala(dados: &Path, agora: &Agora) -> String {
    let pessoal: Value = std::fs::read_to_string(dados.join("pessoal.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let nome = pessoal["nome"].as_str().map(|n| format!(", {n}")).unwrap_or_default();
    let mut partes = vec![format!("Bom dia{nome}! Aqui é o Jarvis com o resumo da madrugada.")];
    let rel = std::fs::read_to_string(dados.join("relatorio.md")).unwrap_or_default();
    // cada linha e "- dd/mm HH:MM · texto": a fala fica so com o texto, as 12 ultimas
    let feitos: Vec<String> = rel.lines().filter_map(|l| l.split_once(" · ").map(|(_, t)| para_fala(t, 230))).filter(|t| !t.is_empty())
        .map(|t| if t.ends_with(['.', '!', '?']) { t } else { format!("{t}.") }).collect(); // sem ponto, a linha emendava na frase seguinte
    let ultimos = &feitos[feitos.len().saturating_sub(12)..];
    partes.push(if ultimos.is_empty() { "De madrugada não houve nada novo para registrar.".into() } else { format!("O que foi feito: {}", ultimos.join(" ")) });
    let ag = agenda::resumo_falado(&dados.join("agenda.json"), agora);
    if !ag.is_empty() {
        partes.push(ag);
    }
    let discos: Vec<String> = ['C', 'D', 'E'].iter().filter_map(|l| gigas_livres(*l).map(|g| format!("o disco {l} tem {g} gigas livres"))).collect();
    if !discos.is_empty() {
        partes.push(format!("No computador, {}.", discos.join(" e ")));
    }
    partes.push("Tenha um ótimo dia. Qualquer coisa, é só me chamar.".into());
    partes.join(" ").chars().take(3500).collect()
}

/// Uma vez por dia, a partir das 6h. So marca como feito (e recomeca o relatorio) se o envio confirmar com "✅".
pub fn tick(dados: &Path, agora: &Agora, enviar: &dyn Fn(&str) -> String) -> bool {
    let arq = dados.join("rotina.json");
    let mut est: Value = std::fs::read_to_string(&arq).ok().and_then(|t| serde_json::from_str(&t).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}));
    let hoje = agora.data.iso();
    if agora.hora < HORA || est["bom_dia"] == hoje.as_str() || !enviar(&fala(dados, agora)).starts_with('✅') {
        return false;
    }
    est["bom_dia"] = json!(hoje);
    let _ = std::fs::write(&arq, est.to_string());
    // guarda no historico antes de zerar
    let rel = std::fs::read_to_string(dados.join("relatorio.md")).unwrap_or_default();
    let guardou = rel.is_empty()
        || std::fs::OpenOptions::new().create(true).append(true).open(dados.join("relatorio_historico.md")).and_then(|mut f| write!(f, "\n## entregue em {hoje}\n{rel}")).is_ok();
    if guardou {
        let _ = std::fs::write(dados.join("relatorio.md"), "");
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;
    use std::cell::RefCell;

    #[test]
    fn uma_vez_por_dia_depois_das_seis_e_o_relatorio_recomeca() {
        let d = std::env::temp_dir().join(format!("braniac-bomdia-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let hora = |dia, hora| Agora { data: Data::civil(2026, 11, dia).unwrap(), hora, min: 5, seg: 0 };
        std::fs::write(d.join("pessoal.json"), r#"{"nome": "Maria"}"#).unwrap();
        assert!(registrar(&d, &hora(17, 2), "Backup_diario feito, veja https://exemplo.com/x") && !registrar(&d, &hora(17, 2), "  "));
        agenda::ferramenta(&d.join("agenda.json"), &json!({"acao": "adicionar", "titulo": "Dentista", "quando": "2026-11-17 14:00"}), &hora(17, 2));
        let ditos = RefCell::new(Vec::new());
        let ok = |t: &str| { ditos.borrow_mut().push(t.to_string()); "✅ ok".to_string() };
        assert!(!tick(&d, &hora(17, 5), &ok)); // antes das 6h
        assert!(!tick(&d, &hora(17, 6), &|_| "falhou".into())); // envio falhou: tenta de novo depois
        assert!(tick(&d, &hora(17, 6), &ok) && !tick(&d, &hora(17, 9), &ok)); // uma vez por dia
        let f = ditos.borrow()[0].clone();
        assert!(f.starts_with("Bom dia, Maria! Aqui é o Jarvis") && f.contains("O que foi feito: Backup diario feito, veja. Na sua agenda") && !f.contains("http"), "{f}");
        assert!(f.contains("Na sua agenda de hoje: às 14 horas, Dentista."), "{f}");
        assert!(std::fs::read_to_string(d.join("relatorio.md")).unwrap().is_empty());
        assert!(std::fs::read_to_string(d.join("relatorio_historico.md")).unwrap().contains("## entregue em 2026-11-17\n- 17/11 02:05 · Backup_diario"));
        assert!(tick(&d, &hora(18, 7), &ok)); // dia seguinte, relatorio vazio
        assert!(ditos.borrow()[1].contains("De madrugada não houve nada novo") && ditos.borrow().len() == 2);
        let _ = std::fs::remove_dir_all(&d);
    }
}
