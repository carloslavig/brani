//! Rotina de candidaturas: 3 janelas por dia (manha 7-12, tarde 12-18, noite 18-23), ate 5 vagas em cada, no maximo 2 tentativas por janela
//! com 1 h entre elas. Aqui so se conta o que ja foi registrado como "enviada" em candidaturas.json e se decide se a janela dispara.
//! Estado em dados/candidatura_rotina.json; o arquivo dados/candidatura_rotina.pausa segura tudo ate ser apagado.
//! Nunca resolve captcha, nao cria conta, nao grava audio: isso esta no pedido entregue ao agente e o que exigir fica "aguardando_voce".
//! Ficou de fora: acionar o agente com navegador (closure `rodar`, devolve (codigo, texto)) e o envio do resumo (closure).
//! Diferenca do Python: `tick` roda a closure na hora (la, numa thread); quem chama decide se poe em segundo plano.
#![allow(dead_code)]
use crate::candidaturas;
use crate::empresa_dev::{corta, ler};
use crate::tempo::Agora;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

pub const META: usize = 5;
const JANELAS: [(&str, u32, u32); 3] = [("manha", 7, 12), ("tarde", 12, 18), ("noite", 18, 23)];
const TENTATIVAS: i64 = 2;
const ESPERA: f64 = 3600.0;
static RODANDO: AtomicBool = AtomicBool::new(false);

pub fn janela(hora: u32) -> Option<&'static str> {
    JANELAS.iter().find(|(_, a, b)| (*a..*b).contains(&hora)).map(|(n, _, _)| *n)
}

/// O pedido entregue ao agente. `quem` = nome do candidato (vem da configuracao).
pub fn pedido(nome: &str, falta: usize, quem: &str) -> String {
    format!("Rotina de candidaturas, janela da {nome}. Siga a skill candidatura-vagas e as regras do projeto. Candidate {quem} a até {falta} vaga(s) de desenvolvedor estágio/júnior REMOTAS (varie a busca: python, javascript, typescript, node, developer, software, suporte, QA; nível 0 e 1), só se o CV cobrir o essencial (sem mentir) e sem áudio/teste/presença/residência fora do Brasil. Pule vagas que já estão em candidaturas.json. Registre cada vaga como enviada, pulada ou aguardando_voce, com título, empresa, link e motivo. Só registre 'enviada' quando a URL tiver ?success ou aparecer a mensagem de sucesso. Nunca clique em captcha, não crie conta e não digite senha. Termine com 1 linha: enviadas, puladas e o que depende de {quem}.")
}

/// Chamado a cada ~10 min. Dispara a janela atual se ainda faltam vagas, sobram tentativas e a espera passou. Devolve o nome da janela disparada.
pub fn tick(dados: &Path, agora: &Agora, ts: f64, quem: &str, rodar: &dyn Fn(&str) -> (i32, String), enviar: &dyn Fn(&str)) -> Option<&'static str> {
    let (nome, ini_h, fim_h) = *JANELAS.iter().find(|(_, a, b)| (*a..*b).contains(&agora.hora))?;
    if dados.join("candidatura_rotina.pausa").exists() {
        return None; // pausa pedida pelo dono: apague o arquivo para retomar
    }
    let arq = dados.join("candidatura_rotina.json");
    let hoje = agora.data.iso();
    let mut dia = ler(&arq).map(|e| e[&hoje].clone()).filter(Value::is_object).unwrap_or_else(|| json!({}));
    let (tentativas, ultima) = (dia[nome]["tentativas"].as_i64().unwrap_or(0), dia[nome]["ultima"].as_f64().unwrap_or(0.0));
    let ini = ts - agora.segundos() as f64 + (ini_h * 3600) as f64;
    let feitas = candidaturas::enviadas_entre(dados, ini, ini + ((fim_h - ini_h) * 3600) as f64);
    if feitas >= META || tentativas >= TENTATIVAS || ts - ultima < ESPERA {
        return None;
    }
    if RODANDO.swap(true, Ordering::SeqCst) {
        return None; // outra janela ainda esta rodando neste processo
    }
    dia[nome] = json!({ "tentativas": tentativas + 1, "ultima": ts });
    // guarda so o dia de hoje; sem gravar nao dispara (senao repetiria a cada tick)
    if std::fs::create_dir_all(dados).and_then(|_| std::fs::write(&arq, json!({ hoje: dia }).to_string())).is_ok() {
        let (cod, texto) = rodar(&pedido(nome, META - feitas, quem));
        let total = candidaturas::enviadas_entre(dados, ini, ts + 86400.0);
        let resumo = match texto.trim().lines().last() {
            Some(l) if cod == 0 => corta(l, 300),
            _ => format!("não consegui rodar (código {cod})"),
        };
        enviar(&format!("💼 Candidaturas da {nome}: {total} enviada(s) até agora. {resumo}"));
    }
    RODANDO.store(false, Ordering::SeqCst);
    Some(nome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::Data;
    use std::cell::RefCell;

    #[test]
    fn janelas_do_dia() {
        assert_eq!([6, 7, 11, 12, 17, 18, 22, 23].map(janela), [None, Some("manha"), Some("manha"), Some("tarde"), Some("tarde"), Some("noite"), Some("noite"), None]);
        let p = pedido("tarde", 3, "Ana");
        assert!(p.contains("janela da tarde") && p.contains("Candidate Ana a até 3 vaga(s)") && p.contains("Nunca clique em captcha, não crie conta e não digite senha"));
    }

    #[test]
    fn tick_dispara_conta_enviadas_respeita_espera_tentativas_meta_e_pausa() {
        let d = std::env::temp_dir().join(format!("braniac-candidatura_rotina-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let a = |dia: i64, h: u32| Agora { data: Data::civil(2026, 11, dia).unwrap(), hora: h, min: 0, seg: 0 };
        let base = 1_000_000.0; // 2026-11-04 08:00 neste teste
        let pedidos = RefCell::new(vec![]);
        let avisos = RefCell::new(vec![]);
        let rodar = |p: &str| -> (i32, String) {
            pedidos.borrow_mut().push(p.to_string());
            candidaturas::registrar(&d, "enviada", "Dev Irmao", &format!("Empresa {}", pedidos.borrow().len()), &format!("https://exemplo.test/{}", pedidos.borrow().len()), "", base + 60.0).unwrap();
            (0, "log\n1 enviada, 0 puladas".into())
        };
        let enviar = |m: &str| avisos.borrow_mut().push(m.to_string());
        assert_eq!(tick(&d, &a(4, 6), base, "Ana", &rodar, &enviar), None); // fora de janela
        std::fs::write(d.join("candidatura_rotina.pausa"), "").unwrap();
        assert_eq!(tick(&d, &a(4, 8), base, "Ana", &rodar, &enviar), None); // pausada
        std::fs::remove_file(d.join("candidatura_rotina.pausa")).unwrap();
        assert_eq!(tick(&d, &a(4, 8), base, "Ana", &rodar, &enviar), Some("manha"));
        assert!(pedidos.borrow()[0].contains("até 5 vaga(s)"));
        assert_eq!(avisos.borrow()[0], "💼 Candidaturas da manha: 1 enviada(s) até agora. 1 enviada, 0 puladas");
        assert_eq!(tick(&d, &a(4, 8), base + 600.0, "Ana", &rodar, &enviar), None); // espera de 1 h
        assert_eq!(tick(&d, &a(4, 9), base + 3600.0, "Ana", &rodar, &enviar), Some("manha"));
        assert!(pedidos.borrow()[1].contains("até 4 vaga(s)")); // so o que falta
        assert_eq!(tick(&d, &a(4, 11), base + 3.0 * 3600.0, "Ana", &rodar, &enviar), None); // 2 tentativas por janela
        // tarde: janela nova; agente que falha conta a tentativa e avisa
        let falha = |_: &str| -> (i32, String) { (1, String::new()) };
        assert_eq!(tick(&d, &a(4, 13), base + 5.0 * 3600.0, "Ana", &falha, &enviar), Some("tarde"));
        assert!(avisos.borrow()[2].contains("não consegui rodar (código 1)"));
        // meta batida na janela: nao dispara
        for i in 0..5 {
            candidaturas::registrar(&d, "enviada", "Dev", &format!("Outra {i}"), &format!("https://exemplo.test/n{i}"), "", base + 10.5 * 3600.0).unwrap();
        }
        assert_eq!(tick(&d, &a(4, 19), base + 11.0 * 3600.0, "Ana", &rodar, &enviar), None);
        // dia novo: so o dia de hoje fica no arquivo
        assert_eq!(tick(&d, &a(5, 8), base + 86400.0, "Ana", &rodar, &enviar), Some("manha"));
        let est: Value = serde_json::from_str(&std::fs::read_to_string(d.join("candidatura_rotina.json")).unwrap()).unwrap();
        assert_eq!(est.as_object().unwrap().keys().collect::<Vec<_>>(), ["2026-11-05"]);
    }
}
