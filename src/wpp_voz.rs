//! Voz no WhatsApp: as REGRAS de enviar e ouvir audio (porte de braniac/wpp_voz.py sem os scripts de pagina).
//! - um audio por vez: tudo que mexe na pagina passa por `um_por_vez` (a MESMA trava para audio, arquivo, imagem e texto);
//! - a conversa aberta tem que ser a pedida (`abrir_chat` confere o nome no topo com `trava_destino` antes de qualquer envio);
//! - gravacao que ficou presa e cancelada (nada e enviado); rascunho apagado para gravar fica no registro;
//! - ouvir audio: espera maxima de 60 s pela pagina recarregada; qual audio e de quem; casar o audio da tela com o arquivo.
//! Ficou de fora: clicar/digitar/ler a pagina e a transcricao (entram como closures em `Pagina` e `Envio`).
#![allow(dead_code)]
use crate::contexto::Ctx;
use crate::jarvis_travas::{chave_busca, melhor_candidato, resolver_nome, trava_destino};
use std::sync::Mutex;

/// espera depois do audio acabar, antes de apertar Enviar
pub const FOLGA_S: f64 = 0.8;
/// silencio colado no fim do audio que vai para o microfone virtual
pub const SILENCIO_FINAL_S: f64 = 1.5;
/// teto de relogio para a pagina recarregada voltar: sem ele o laco segurava o WhatsApp por minutos e ninguem era atendido
pub const ESPERA_RECARGA_S: f64 = 60.0;
/// diferenca aceita entre a duracao mostrada na bolha e a do arquivo
pub const TOLERANCIA_S: f64 = 1.6;

/// A pagina do WhatsApp e o microfone virtual sao unicos.
pub static PAGINA: Mutex<()> = Mutex::new(());

pub fn um_por_vez<T>(f: impl FnOnce() -> T) -> T {
    let _g = PAGINA.lock().unwrap_or_else(|e| e.into_inner());
    f()
}

/// O que as regras precisam da pagina (quem integra liga ao navegador; nos testes, closures falsas).
pub struct Pagina<'a> {
    /// nomes das conversas visiveis agora
    pub listar: &'a dyn Fn() -> Vec<String>,
    /// digita na busca do aplicativo
    pub buscar: &'a dyn Fn(&str),
    /// abre a conversa com este nome exato
    pub abrir: &'a dyn Fn(&str) -> bool,
    /// nome que aparece NO TOPO da conversa aberta
    pub titulo: &'a dyn Fn() -> String,
    /// cancela uma gravacao presa, se houver (nada e enviado)
    pub cancelar_gravacao: &'a dyn Fn(),
}

/// Abre a conversa de `nome` e confere o destino. `pedido` = como a pessoa foi pedida (padrao: o proprio nome).
/// Ok(nome exato) ou Err(texto do problema; nada foi enviado).
pub fn abrir_chat(ctx: &Ctx, pg: &Pagina, nome: &str, pedido: Option<&str>) -> Result<String, String> {
    let mut alvos = resolver_nome(ctx, nome, &(pg.listar)());
    if alvos.is_empty() {
        // sem conversa recente na lista: acha pela busca
        (pg.buscar)(nome);
        alvos = resolver_nome(ctx, nome, &(pg.listar)());
    }
    // nao pergunta ao dono: nome exato, senao o que mais combina
    let Some(alvo) = (if alvos.len() > 1 { melhor_candidato(ctx, nome, &alvos) } else { alvos.first().cloned() }) else {
        return Err(format!("Não achei conversa com '{nome}' no WhatsApp."));
    };
    if !(pg.abrir)(&alvo) {
        (pg.buscar)(&chave_busca(&alvo));
        if !(pg.abrir)(&alvo) {
            return Err(format!("Não consegui abrir a conversa com {alvo}."));
        }
    }
    if let Some(bloqueio) = trava_destino(ctx, pedido.unwrap_or(nome), &(pg.titulo)()) {
        return Err(bloqueio);
    }
    (pg.cancelar_gravacao)();
    Ok(alvo)
}

/// Gravacao presa (o programa caiu no meio): o rodape mostra pausar/retomar gravacao e um Cancelar. Decide cancelar; nunca enviar.
pub fn gravacao_presa(rotulos_do_rodape: &[&str]) -> bool {
    rotulos_do_rodape.iter().any(|r| matches!(*r, "Pausar gravação" | "Retomar gravação")) && rotulos_do_rodape.contains(&"Cancelar")
}

pub const LOG_GRAVACAO_CANCELADA: &str = "gravacao de audio presa cancelada (nada enviado)";

/// Nada some em silencio: o rascunho apagado para gravar vai para o registro (so o comeco).
pub fn log_rascunho(rascunho: &str) -> String {
    format!("rascunho apagado para gravar audio | {:?}", rascunho.chars().take(120).collect::<String>())
}

fn u32le(b: &[u8], i: usize) -> Option<u32> {
    b.get(i..i + 4).and_then(|x| <[u8; 4]>::try_from(x).ok()).map(u32::from_le_bytes)
}

/// Audio do Jarvis -> (arquivo do microfone virtual: PCM 16 bits mono com silencio no fim, duracao em segundos do audio original).
pub fn preparar_mic(wav: &[u8]) -> Result<(Vec<u8>, f64), String> {
    if wav.len() < 12 || &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return Err("áudio inválido (não é WAV)".into());
    }
    let (mut i, mut taxa, mut bloco, mut dados) = (12usize, 0u32, 0u32, None);
    while let (Some(id), Some(tam)) = (wav.get(i..i + 4), u32le(wav, i + 4)) {
        let (ini, fim) = (i + 8, (i + 8 + tam as usize).min(wav.len()));
        match id {
            b"fmt " => {
                taxa = u32le(wav, ini + 4).unwrap_or(0);
                bloco = wav.get(ini + 12..ini + 14).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]) as u32);
            }
            b"data" => dados = Some(&wav[ini.min(wav.len())..fim]),
            _ => {}
        }
        i = fim + (tam as usize & 1);
    }
    let Some(dados) = dados.filter(|_| taxa > 0 && bloco > 0) else { return Err("áudio inválido (sem dados)".into()) };
    let quadros = dados.len() as u32 / bloco;
    let mut pcm = dados[..(quadros * bloco) as usize].to_vec();
    pcm.extend(std::iter::repeat(0u8).take((taxa as f64 * SILENCIO_FINAL_S) as usize * 2));
    Ok((crate::wav::wav(&pcm, taxa), quadros as f64 / taxa as f64))
}

/// O que dizer depois de clicar em enviar: so e sucesso se o audio APARECEU na conversa.
pub fn resultado_voz(alvo: &str, antes: usize, depois: usize) -> String {
    if depois > antes { format!("✅ Áudio enviado para {alvo}.") } else { format!("Cliquei em enviar para {alvo}, mas não vi o áudio aparecer; confira.") }
}

/// Passos do envio de voz que tocam a pagina.
pub struct Envio<'a> {
    pub pagina: Pagina<'a>,
    /// troca o audio do microfone virtual
    pub gravar_mic: &'a dyn Fn(&[u8]) -> Result<(), String>,
    pub fechar_menus: &'a dyn Fn(),
    /// quantas mensagens de voz a conversa mostra
    pub contar_voz: &'a dyn Fn() -> usize,
    /// apaga o que estiver escrito na caixa (com texto la o microfone vira "Enviar")
    pub limpar_rascunho: &'a dyn Fn(),
    pub clicar_microfone: &'a dyn Fn() -> bool,
    pub esperar: &'a dyn Fn(f64),
    pub clicar_enviar: &'a dyn Fn() -> bool,
    /// anota que o Jarvis acabou de mandar audio a essa pessoa (para nao confundir com comando de voz do dono)
    pub registrar_envio: &'a dyn Fn(&str),
}

/// Envia `wav` como MENSAGEM DE VOZ para `nome`, um por vez. Devolve o resultado em texto (comeca com ✅ se saiu).
pub fn enviar_voz(ctx: &Ctx, e: &Envio, nome: &str, wav: &[u8]) -> String {
    let r = um_por_vez(|| {
        let (mic, dur) = match preparar_mic(wav) {
            Ok(x) => x,
            Err(m) => return m,
        };
        if let Err(m) = (e.gravar_mic)(&mic) {
            return m;
        }
        (e.fechar_menus)();
        let alvo = match abrir_chat(ctx, &e.pagina, nome, None) {
            Ok(a) => a,
            Err(m) => return m,
        };
        let antes = (e.contar_voz)();
        (e.limpar_rascunho)();
        if !(e.clicar_microfone)() {
            return "Não achei o botão de mensagem de voz do WhatsApp.".into();
        }
        (e.esperar)(dur + FOLGA_S); // grava: o microfone virtual toca o audio
        if !(e.clicar_enviar)() {
            (e.fechar_menus)();
            return "A gravação não abriu (o navegador do agente foi iniciado sem o microfone virtual?).".into();
        }
        (e.esperar)(3.0);
        resultado_voz(&alvo, antes, (e.contar_voz)())
    });
    if r.starts_with('✅') {
        (e.registrar_envio)(nome);
    }
    r
}

/// Espera a lista de conversas voltar depois de recarregar a pagina, com teto de RELOGIO (60 s) e de tentativas (40).
pub fn esperar_recarga(pronto: &dyn Fn() -> bool, esperar: &dyn Fn(f64), relogio: &dyn Fn() -> f64) -> Result<(), String> {
    esperar(3.0);
    let limite = relogio() + ESPERA_RECARGA_S;
    for _ in 0..40 {
        if relogio() > limite {
            break;
        }
        esperar(1.5);
        if pronto() {
            return Ok(());
        }
    }
    Err("O WhatsApp não voltou a carregar.".into())
}

/// Audio mostrado na conversa: (e DELE?, duracao em segundos; 0 = a tela nao mostrou).
pub type Bolha = (bool, f64);

/// Quais audios ouvir: os ultimos `n` DELES ainda sem resposta nossa, em ordem (se nenhum, o ultimo dele).
/// `pular_nossos` recupera audios que ficaram para tras de uma resposta nossa; `so_nossos` = o mais recente do DONO.
pub fn escolher_audios(linhas: &[Bolha], n: usize, pular_nossos: bool, so_nossos: bool) -> Vec<Bolha> {
    if so_nossos {
        return linhas.iter().rev().find(|x| !x.0).copied().into_iter().collect();
    }
    let mut fim = Vec::new();
    for x in linhas.iter().rev() {
        if !x.0 {
            if pular_nossos {
                continue;
            }
            break;
        }
        fim.push(*x);
    }
    fim.reverse();
    let fim = fim[fim.len().saturating_sub(n)..].to_vec();
    if fim.is_empty() { linhas.iter().rev().find(|x| x.0).copied().into_iter().collect() } else { fim }
}

/// Casa cada audio escolhido com o arquivo capturado de mesma duracao (`durs`, na ordem de captura). Devolve os indices.
/// Sem par e so um audio pedido: se a tela mostrava a duracao, o audio ainda nao baixou (erro); senao vale o mais recente.
pub fn casar_duracoes(fim: &[Bolha], durs: &[f64], n: usize, capturados: usize) -> Result<Vec<usize>, String> {
    let mut idxs: Vec<usize> = Vec::new();
    for x in fim {
        let certo = durs.iter().enumerate().filter(|(i, d)| !idxs.contains(i) && **d > 0.0 && x.1 != 0.0 && (**d - x.1).abs() <= TOLERANCIA_S).map(|(i, _)| i).last();
        let i = match certo {
            Some(i) => i,
            None if n == 1 || idxs.is_empty() => {
                if x.1 != 0.0 {
                    return Err("O ultimo audio dele ainda nao baixou no WhatsApp Web (sem botao de reproduzir). Peca para reenviar ou mande por texto.".into());
                }
                capturados.max(1) - 1
            }
            None => continue,
        };
        idxs.push(i);
    }
    Ok(idxs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contexto::norm;
    use std::cell::{Cell, RefCell};

    fn ctx() -> Ctx {
        let mut c = Ctx { dono: "Davi".into(), ..Ctx::default() };
        c.contatos.insert(norm("bia"), "Beatriz Lima 🤍".into());
        c
    }

    fn nomes(l: &[&str]) -> Vec<String> {
        l.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn abre_o_contato_certo_e_barra_destino_errado() {
        let c = ctx();
        let aberto = RefCell::new(String::new());
        let cancelou = Cell::new(0);
        let buscas = RefCell::new(Vec::<String>::new());
        let abrir = |n: &str| { *aberto.borrow_mut() = n.replace(" 🤍", ""); true };
        let titulo = || aberto.borrow().clone();
        let listar = || nomes(&["Eu(SP)", "Beatriz Lima 🤍", "Caio Obras", "Caio Vendas Sul"]);
        let buscar = |t: &str| buscas.borrow_mut().push(t.to_string());
        let cancelar = || cancelou.set(cancelou.get() + 1);
        let pg = Pagina { listar: &listar, buscar: &buscar, abrir: &abrir, titulo: &titulo, cancelar_gravacao: &cancelar };
        assert_eq!(abrir_chat(&c, &pg, "bia", None).unwrap(), "Beatriz Lima 🤍"); // emoji no nome, apelido no pedido
        assert_eq!(abrir_chat(&c, &pg, "caio vendas", None).unwrap(), "Caio Vendas Sul");
        assert_eq!(cancelou.get(), 2);
        assert!(abrir_chat(&c, &pg, "zzz", None).unwrap_err().contains("Não achei conversa com 'zzz'") && buscas.borrow().as_slice() == ["zzz"]);
        // o aplicativo abriu OUTRA conversa: nada e enviado
        let errado = || "Caio Obras".to_string();
        let pg2 = Pagina { titulo: &errado, ..pg };
        let e = abrir_chat(&c, &pg2, "bia", None).unwrap_err();
        assert!(e.starts_with("🔒") && e.contains("Caio Obras") && cancelou.get() == 2);
        // nao conseguiu ler o topo: tambem nao envia
        let vazio = String::new;
        assert!(abrir_chat(&c, &Pagina { titulo: &vazio, ..pg }, "bia", None).is_err());
        // o `pedido` original confere mesmo quando o nome ja veio resolvido
        assert!(abrir_chat(&c, &pg, "Beatriz Lima 🤍", Some("bia")).is_ok());
        assert!(abrir_chat(&c, &pg, "Caio Obras", Some("bia")).unwrap_err().starts_with("🔒"));
        let nao_abre = |_: &str| false;
        assert!(abrir_chat(&c, &Pagina { abrir: &nao_abre, ..pg }, "bia", None).unwrap_err().starts_with("Não consegui abrir"));
    }

    #[test]
    fn gravacao_presa_e_cancelada_e_rascunho_fica_no_registro() {
        assert!(gravacao_presa(&["Cancelar", "Pausar gravação", "Enviar"]) && gravacao_presa(&["Retomar gravação", "Cancelar"]));
        assert!(!gravacao_presa(&["Mensagem de voz", "Anexar"]) && !gravacao_presa(&["Pausar gravação"]));
        assert!(log_rascunho(&"x".repeat(300)).len() < 180 && log_rascunho("oi").contains("\"oi\""));
    }

    #[test]
    fn mic_ganha_silencio_no_fim_e_a_duracao_e_do_audio() {
        let w = crate::wav::wav(&[1u8; 48_000], 24_000);
        let (mic, dur) = preparar_mic(&w).unwrap();
        assert!((dur - 1.0).abs() < 1e-9 && (crate::wav::segundos(&mic) - 2.5).abs() < 1e-9);
        assert!(mic[44..44 + 48_000].iter().all(|b| *b == 1) && mic[44 + 48_000..].iter().all(|b| *b == 0));
        assert!(preparar_mic(b"abc").is_err() && preparar_mic(&w[..40]).is_err());
    }

    #[test]
    fn um_audio_por_vez_e_so_diz_enviado_se_apareceu() {
        let c = ctx();
        let passos = RefCell::new(Vec::<String>::new());
        let voz = Cell::new(2usize);
        let anota = |s: &str| passos.borrow_mut().push(s.to_string());
        let listar = || nomes(&["Beatriz Lima 🤍"]);
        let buscar = |_: &str| ();
        let abrir = |_: &str| true;
        let titulo = || "Beatriz Lima".to_string();
        let cancelar = || ();
        let gravar = |_: &[u8]| Ok(());
        let fechar = || anota("fechar");
        let contar = || voz.get();
        let limpar = || anota("limpar");
        let mic = || { anota("mic"); assert!(PAGINA.try_lock().is_err()); true }; // dentro da trava da pagina
        let esperar = |s: f64| anota(&format!("espera {s:.1}"));
        let enviar = || { voz.set(voz.get() + 1); true };
        let registrar = |n: &str| anota(&format!("registrou {n}"));
        let e = Envio { pagina: Pagina { listar: &listar, buscar: &buscar, abrir: &abrir, titulo: &titulo, cancelar_gravacao: &cancelar },
            gravar_mic: &gravar, fechar_menus: &fechar, contar_voz: &contar, limpar_rascunho: &limpar, clicar_microfone: &mic, esperar: &esperar, clicar_enviar: &enviar, registrar_envio: &registrar };
        let w = crate::wav::wav(&[0u8; 48_000], 24_000);
        assert_eq!(enviar_voz(&c, &e, "bia", &w), "✅ Áudio enviado para Beatriz Lima 🤍.");
        assert_eq!(*passos.borrow(), ["fechar", "limpar", "mic", "espera 1.8", "espera 3.0", "registrou bia"]);
        // clicou mas o audio nao apareceu: nao diz que enviou, nao registra
        let nao_sai = || true;
        let r = enviar_voz(&c, &Envio { clicar_enviar: &nao_sai, pagina: Pagina { listar: &listar, buscar: &buscar, abrir: &abrir, titulo: &titulo, cancelar_gravacao: &cancelar }, ..e }, "bia", &w);
        assert!(r.starts_with("Cliquei em enviar") && passos.borrow().iter().filter(|p| p.starts_with("registrou")).count() == 1);
        // destino errado: nem chega no microfone
        let outro = || "Caio Obras".to_string();
        let antes = passos.borrow().len();
        let r = enviar_voz(&c, &Envio { pagina: Pagina { listar: &listar, buscar: &buscar, abrir: &abrir, titulo: &outro, cancelar_gravacao: &cancelar }, ..e }, "bia", &w);
        assert!(r.starts_with("🔒") && !passos.borrow()[antes..].contains(&"mic".to_string()));
        assert!(enviar_voz(&c, &e, "bia", b"lixo").contains("inválido"));
    }

    #[test]
    fn recarga_tem_teto_de_60_segundos() {
        let t = Cell::new(0.0);
        let esperar = |s: f64| t.set(t.get() + s);
        let relogio = || t.get();
        // cada conferida travada custa 20 s: sem o teto de relogio seriam 40 voltas (~14 min)
        let voltas = Cell::new(0);
        let travado = || { voltas.set(voltas.get() + 1); t.set(t.get() + 20.0); false };
        assert!(esperar_recarga(&travado, &esperar, &relogio).is_err());
        assert!(voltas.get() <= 3 && t.get() < 90.0);
        t.set(0.0);
        let n = Cell::new(0);
        let logo = || { n.set(n.get() + 1); n.get() == 2 };
        assert!(esperar_recarga(&logo, &esperar, &relogio).is_ok() && t.get() == 6.0);
        // pagina que nunca volta, mas responde rapido: para nas 40 tentativas
        t.set(0.0);
        let nunca = || false;
        assert!(esperar_recarga(&nunca, &esperar, &relogio).is_err() && t.get() <= 3.0 + 41.0 * 1.5);
    }

    #[test]
    fn escolhe_os_audios_dele_sem_resposta_e_casa_pela_duracao() {
        let l: Vec<Bolha> = vec![(true, 5.0), (false, 3.0), (true, 7.0), (true, 12.0)];
        assert_eq!(escolher_audios(&l, 3, false, false), [(true, 7.0), (true, 12.0)]); // para na nossa resposta
        assert_eq!(escolher_audios(&l, 1, false, false), [(true, 12.0)]);
        assert_eq!(escolher_audios(&l, 3, true, false), [(true, 5.0), (true, 7.0), (true, 12.0)]);
        assert_eq!(escolher_audios(&l, 3, false, true), [(false, 3.0)]); // o mais recente do dono
        assert_eq!(escolher_audios(&[(true, 5.0), (false, 3.0)], 3, false, false), [(true, 5.0)]); // nenhum sem resposta: o ultimo dele
        assert!(escolher_audios(&[], 3, false, false).is_empty());
        // arquivos capturados fora de ordem: casa pela duracao, sem repetir
        assert_eq!(casar_duracoes(&[(true, 7.0), (true, 12.0)], &[12.4, 3.0, 6.1], 3, 3), Ok(vec![2, 0]));
        assert_eq!(casar_duracoes(&[(true, 7.0), (true, 7.0)], &[7.0, 7.2], 3, 2), Ok(vec![1, 0]));
        // a tela mostrava a duracao mas o arquivo nao veio: ainda nao baixou
        assert!(casar_duracoes(&[(true, 30.0)], &[3.0], 1, 1).unwrap_err().contains("ainda nao baixou"));
        // sem duracao na tela: vale o capturado mais recente
        assert_eq!(casar_duracoes(&[(true, 0.0)], &[], 1, 4), Ok(vec![3]));
        assert_eq!(casar_duracoes(&[(true, 0.0)], &[], 1, 0), Ok(vec![0]));
        // varios pedidos: o que nao casou depois do primeiro e pulado
        assert_eq!(casar_duracoes(&[(true, 7.0), (true, 40.0)], &[7.0], 3, 1), Ok(vec![0]));
    }
}
