//! WhatsApp de verdade, parte 2: AUDIO (enviar e ouvir) e ARQUIVO (enviar), na mesma aba da VM. Seletores e passos vieram de
//! braniac/wpp_voz.py e braniac/wpp_arquivo.py (nada inventado). Tudo passa pelo trait `Pagina`, entao os testes usam uma pagina de mentira.
//! ENVIAR AUDIO: o WAV falado pelo B.R.A.N.I vira dados/mic.wav (mono, 16 bits, +1,5 s de silencio) e o Chromium da VM o le como
//! MICROFONE FALSO (flags em padrao/ligar.sh; o arquivo e visto pela VM em /mnt/<disco>/...). Clica no microfone, espera a duracao,
//! clica em enviar e confere que o audio apareceu na conversa. Travas: um envio por vez (TRAVA), nome no topo conferido ANTES de gravar,
//! limite de duracao, rascunho na caixa barra, gravacao presa e cancelada, e `gravando()` avisa quem abriria aba nova (o audio sai cortado).
//! OUVIR: gancho em URL.createObjectURL guarda os blobs de audio da pagina; se nenhum veio, aperta o play do ultimo audio DELE; teto de
//! 60 s; falhou = o contato fica 20 min sem nova tentativa (mesmo arquivo de pausas do atender_laco). A transcricao e LOCAL (ouvir.rs).
//! ARQUIVO: botao Anexar -> Documento (ou Fotos e videos) -> seletor de arquivo interceptado por CDP (`Pagina::escolher_arquivo`) ->
//! legenda -> enviar -> confere "enviada". Executaveis barrados, ate 100 MB, o pai do dono nao recebe arquivo (so audio).
use crate::alma_cuida::Nomes;
use crate::atender_laco::{marcar_pausa, pausa, AUDIO_PAUSA_S};
use crate::contexto::Ctx;
use crate::wpp_vivo::{self as wv, json_de, Pagina};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

static TRAVA: Mutex<()> = Mutex::new(());
static GRAVANDO: AtomicBool = AtomicBool::new(false);

pub const MAX_AUDIO_S: f64 = 150.0;
const FOLGA_S: f64 = 0.8;
const SILENCIO_S: f64 = 1.5;
pub const ESPERA_OUVIR_S: u64 = 60;
const LIMITE_MB: u64 = 100;
const BLOQUEADAS: [&str; 13] = ["exe", "bat", "cmd", "msi", "scr", "com", "vbs", "js", "jar", "ps1", "dll", "lnk", "apk"];

/// Tem um audio sendo gravado agora? Quem abriria/fecharia abas do navegador deve esperar (o audio sai cortado).
pub fn gravando() -> bool {
    GRAVANDO.load(Ordering::Relaxed)
}

const JS_RECT: &str = r##"(function(escopo, rot){
  var els=[].slice.call(document.querySelectorAll(escopo)); var alvo=null;
  for (var e of els){ var a=(e.getAttribute('aria-label')||'').toLowerCase(); var r=e.getBoundingClientRect();
    if (new RegExp(rot,'i').test(a) && r.width>0) alvo=e; }
  if(!alvo) return null; var r=alvo.getBoundingClientRect(); return JSON.stringify({x:r.x+r.width/2,y:r.y+r.height/2}); })(@ESCOPO@, @ROT@)"##;
// Assinatura dos audios da conversa (quantidade + marca do ultimo): a lista do WhatsApp e cortada, entao so contar nao percebe o audio novo.
const JS_CONTA_VOZ: &str = r##"(function(){ var v=[].slice.call(document.querySelectorAll('#main [aria-label="Reproduzir mensagem de voz"], #main [aria-label="Pausar mensagem de voz"]')); var u=v[v.length-1]; var r=u&&(u.closest('[role="row"]')||u.parentElement); var t=r?(r.innerText||''):''; var h=0; for(var i=0;i<t.length;i++){h=(h*31+t.charCodeAt(i))%1000000;} return v.length*1000000+h; })()"##;
const JS_RASCUNHO: &str = r##"(function(){var c=document.querySelector('footer [contenteditable="true"]'); return c ? (c.innerText||'').trim().length : 0})()"##;
const JS_GANCHO: &str = r##"(function(){ if(window.__hkp2){window.__aud=[]; return true;} window.__hkp2=true; window.__aud=[];
  var guarda=function(b){ try{ if(b && b.size>0 && (!b.type || b.type.indexOf('audio')===0) && !window.__aud.some(function(x){return x.size===b.size;})){ window.__aud.push(b); if(window.__aud.length>30) window.__aud.shift(); } }catch(e){} };
  var pl=HTMLMediaElement.prototype.play; // o WhatsApp guarda o blob e reusa a URL: so o play entrega o audio de novo
  HTMLMediaElement.prototype.play=function(){ try{ var s=this.src; if(s && s.indexOf('blob:')===0) fetch(s).then(function(r){return r.blob();}).then(guarda).catch(function(){}); }catch(e){} return pl.apply(this,arguments); };
  return true; })()"##;
const JS_CONTA_BLOBS: &str = "(window.__aud||[]).length";
const JS_BLOB: &str = r##"(async function(i){ var bs=window.__aud||[]; var b=bs[i]; if(!b) return null; var buf=new Uint8Array(await b.arrayBuffer()); var bin='';
  for (var k=0;k<buf.length;k+=0x8000) bin+=String.fromCharCode.apply(null,buf.subarray(k,k+0x8000));
  return JSON.stringify({tipo:b.type,b64:btoa(bin)}); })(@I@)"##;
/// Botao de play do ultimo audio que NAO e nosso (a mesma regra de "nossa" do wpp_msgs.js).
const JS_PLAY_DELE: &str = r##"(function(){ var main=document.querySelector('#main'); if(!main) return null; var alvo=null;
  [].slice.call(main.querySelectorAll('[aria-label="Reproduzir mensagem de voz"]')).forEach(function(b){
    var r=b.closest('[role="row"]')||b.parentElement;
    var nossa=!!(r.className.indexOf('message-out')>=0||r.querySelector('[class*="message-out"],[data-icon="tail-out"],[data-icon^="msg-check"],[data-icon^="msg-dblcheck"],[data-icon^="msg-time"],[data-icon*="status-dblcheck"],[data-icon*="status-check"]')||[].slice.call(r.querySelectorAll('[aria-label]')).some(function(e){return /^\s*(Entregue|Lida|Enviada|Pendente|Delivered|Read|Sent|Pending)\s*$|^Você:$/i.test(e.getAttribute('aria-label'));}));
    if(!nossa) alvo=b; });
  if(!alvo) return null; var q=alvo.getBoundingClientRect(); return JSON.stringify({x:q.x+q.width/2,y:q.y+q.height/2}); })()"##;
const JS_ITEM_DOC: &str = r##"(function(rx){ var els=[...document.querySelectorAll('[role=menuitem], li[role=button], [data-animate-dropdown-item], li, button')];
  var alvo=els.find(e=>new RegExp(rx,'i').test((e.innerText||'').trim()) && e.getBoundingClientRect().width>0);
  if(!alvo) return null; var r=alvo.getBoundingClientRect(); return JSON.stringify({x:r.x+r.width/2,y:r.y+r.height/2}); })(@RX@)"##;
const JS_ANEXAR_CLICK: &str = r##"(function(){var b=[...document.querySelectorAll('footer button')].find(e=>/anexar|attach/i.test(e.getAttribute('aria-label')||'')); if(b) b.click();})()"##;
const JS_ICONE: &str = r##"(function(ic){ var i=[...document.querySelectorAll('[data-icon="'+ic+'"]')].find(e=>e.getBoundingClientRect().width>0);
  if(!i) return null; var b=i.closest('button,[role=button]')||i; var r=b.getBoundingClientRect();
  return JSON.stringify({x:r.x+r.width/2,y:r.y+r.height/2}); })(@IC@)"##;
const JS_ENVIAR_ARIA: &str = r##"(function(){ var b=[...document.querySelectorAll('[aria-label]')].find(e=>/^Enviar \d+ ite/i.test(e.getAttribute('aria-label')||'') && e.getBoundingClientRect().width>0);
  if(!b) return null; var r=b.getBoundingClientRect(); return JSON.stringify({x:r.x+r.width/2,y:r.y+r.height/2}); })()"##;
const JS_X_PREVIA: &str = r##"(function(){ var n=document.querySelector('[data-icon="wds-ic-send-filled"]');
  for(var k=0;k<14&&n;k++){ var x=n.querySelector&&n.querySelector('[data-icon="x-alt"]'); if(x){var r=x.getBoundingClientRect(); if(r.width>0) return JSON.stringify({x:r.x+r.width/2,y:r.y+r.height/2});} n=n.parentElement;}
  var e=[...document.querySelectorAll('[aria-label="Remover anexo"]')].find(x=>x.getBoundingClientRect().width>0);
  if(!e) return null; var q=e.getBoundingClientRect(); return JSON.stringify({x:q.x+q.width/2,y:q.y+q.height/2}); })()"##;
const JS_LEGENDA: &str = r##"(function(){ var c=[...document.querySelectorAll('[contenteditable="true"]')].find(e=>!e.closest('#side') && !e.closest('footer') && e.getBoundingClientRect().width>0);
  if(!c) return false; c.focus(); return true; })()"##;
const JS_ULTIMA: &str = r##"(function(nome){ var main=document.querySelector('#main'); if(!main) return null;
  var rows=[].slice.call(main.querySelectorAll('[role="row"]')); var u=rows[rows.length-1]; if(!u) return JSON.stringify({achou:false});
  var tx=(u.innerText||''); var lab=[].slice.call(u.querySelectorAll('[aria-label]')).map(e=>(e.getAttribute('aria-label')||'').toLowerCase());
  var pend=lab.some(l=>/pendente/.test(l)); var ok=lab.some(l=>/^(enviada|entregue|lida)$/.test(l.trim()));
  return JSON.stringify({achou: tx.indexOf(nome)>=0, pendente: pend, enviada: ok}); })(@NOME@)"##;

fn aspas(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

fn ponto(p: &mut dyn Pagina, js: &str) -> Option<(f64, f64)> {
    let v = json_de(p.js(js).ok()?);
    Some((v["x"].as_f64()?, v["y"].as_f64()?))
}

fn rect(p: &mut dyn Pagina, escopo: &str, rot: &str) -> Option<(f64, f64)> {
    ponto(p, &JS_RECT.replace("@ESCOPO@", &aspas(escopo)).replace("@ROT@", &aspas(rot)))
}

fn icone(p: &mut dyn Pagina, ic: &str) -> Option<(f64, f64)> {
    ponto(p, &JS_ICONE.replace("@IC@", &aspas(ic))).or_else(|| ponto(p, JS_ENVIAR_ARIA))
}

fn clicar(p: &mut dyn Pagina, x: f64, y: f64) {
    let _ = p.mouse("mouseMoved", x, y);
    p.dormir(200);
    let _ = p.mouse("mousePressed", x, y);
    let _ = p.mouse("mouseReleased", x, y);
}

// ---------- WAV -> microfone falso ----------

/// Pura. (taxa, amostras MONO de 16 bits) de um WAV PCM; estereo vira mono pela media. Outro formato = erro.
pub fn ler_wav(wav: &[u8]) -> Result<(u32, Vec<i16>), String> {
    if wav.len() < 12 || &wav[..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return Err("o audio nao e um WAV".into());
    }
    let (mut i, mut fmt, mut dados) = (12usize, None, None);
    while i + 8 <= wav.len() {
        let id = &wav[i..i + 4];
        let tam = u32::from_le_bytes([wav[i + 4], wav[i + 5], wav[i + 6], wav[i + 7]]) as usize;
        let fim = (i + 8).saturating_add(tam).min(wav.len()); // WAV de fluxo traz tamanho 0 ou 0xFFFFFFFF
        if id == b"fmt " && fim - (i + 8) >= 16 {
            let f = &wav[i + 8..fim];
            fmt = Some((u16::from_le_bytes([f[0], f[1]]), u16::from_le_bytes([f[2], f[3]]), u32::from_le_bytes([f[4], f[5], f[6], f[7]]), u16::from_le_bytes([f[14], f[15]])));
        } else if id == b"data" {
            dados = Some(&wav[i + 8..if tam == 0 { wav.len() } else { fim }]);
            break;
        }
        i = fim + (tam & 1);
    }
    let (Some((tag, canais, taxa, bits)), Some(d)) = (fmt, dados) else { return Err("WAV sem fmt ou data".into()) };
    if !(tag == 1 || tag == 0xFFFE) || bits != 16 || !(1..=2).contains(&canais) || taxa < 8000 {
        return Err(format!("WAV nao suportado (formato {tag}, {canais} canais, {bits} bits)"));
    }
    let a: Vec<i16> = d.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
    let m = if canais == 2 { a.chunks_exact(2).map(|c| ((c[0] as i32 + c[1] as i32) / 2) as i16).collect() } else { a };
    Ok((taxa, m))
}

/// Pura. WAV mono de 16 bits.
pub fn escrever_wav(taxa: u32, amostras: &[i16]) -> Vec<u8> {
    let n = (amostras.len() * 2) as u32;
    let mut o = Vec::with_capacity(44 + n as usize);
    o.extend(b"RIFF");
    o.extend((36 + n).to_le_bytes());
    o.extend(b"WAVEfmt ");
    o.extend(16u32.to_le_bytes());
    o.extend(1u16.to_le_bytes());
    o.extend(1u16.to_le_bytes());
    o.extend(taxa.to_le_bytes());
    o.extend((taxa * 2).to_le_bytes());
    o.extend(2u16.to_le_bytes());
    o.extend(16u16.to_le_bytes());
    o.extend(b"data");
    o.extend(n.to_le_bytes());
    o.extend(amostras.iter().flat_map(|a| a.to_le_bytes()));
    o
}

/// Grava dados/mic.wav (a voz + 1,5 s de silencio) e devolve a DURACAO da fala. Acima de MAX_AUDIO_S ou vazio = erro, nada e gravado.
pub fn preparar_mic(dados: &Path, wav: &[u8]) -> Result<f64, String> {
    let (taxa, mut a) = ler_wav(wav)?;
    let dur = a.len() as f64 / taxa as f64;
    if dur < 0.3 {
        return Err("audio vazio".into());
    }
    if dur > MAX_AUDIO_S {
        return Err(format!("audio de {dur:.0} s passa do limite de {MAX_AUDIO_S:.0} s"));
    }
    a.extend(std::iter::repeat(0i16).take((taxa as f64 * SILENCIO_S) as usize));
    let (tmp, alvo) = (dados.join("mic.wav.tmp"), dados.join("mic.wav"));
    std::fs::write(&tmp, escrever_wav(taxa, &a)).and_then(|_| std::fs::rename(&tmp, &alvo)).map_err(|e| format!("nao gravei o microfone falso: {e}"))?;
    Ok(dur)
}

/// Caminho do Windows -> o mesmo arquivo visto pela VM (WSL monta os discos em /mnt/<letra>). Ja Linux (comeca com /) fica como esta.
pub fn caminho_vm(p: &str) -> Result<String, String> {
    if p.starts_with('/') {
        return Ok(p.to_string());
    }
    let b = p.as_bytes();
    if b.len() < 3 || !b[0].is_ascii_alphabetic() || b[1] != b':' || !(b[2] == b'\\' || b[2] == b'/') {
        return Err(format!("caminho que a VM nao enxerga: {p}"));
    }
    Ok(format!("/mnt/{}/{}", (b[0] as char).to_ascii_lowercase(), p[3..].replace('\\', "/")))
}

// ---------- enviar audio ----------

fn contar_audios(p: &mut dyn Pagina) -> i64 {
    p.js(JS_CONTA_VOZ).ok().and_then(|v| v.as_i64()).unwrap_or(0)
}

/// Grava e envia `wav` como mensagem de voz para `nome`. "✅ ..." = enviou; qualquer outra coisa = nada saiu.
pub fn enviar_audio(p: &mut dyn Pagina, dados: &Path, ctx: &Ctx, nomes: &Nomes, nome: &str, wav: &[u8]) -> String {
    let Ok(_g) = TRAVA.lock() else { return "Áudio não enviado: trava interna quebrada.".into() };
    let dur = match preparar_mic(dados, wav) {
        Ok(d) => d,
        Err(e) => return format!("⛔ Áudio barrado: {e}. Nada foi enviado."),
    };
    let alvo = match wv::abrir_destino(p, ctx, nomes, nome, true) {
        Ok(a) => a,
        Err(m) => return m,
    };
    wv::cancelar_gravacao_presa(p);
    if p.js(JS_RASCUNHO).ok().and_then(|v| v.as_i64()).unwrap_or(0) > 0 {
        return format!("A caixa de mensagem de {alvo} tem um rascunho; não gravei para não misturar. Nada foi enviado.");
    }
    let antes = contar_audios(p);
    let Some((x, y)) = rect(p, "footer [aria-label]", "mensagem de voz") else { return "Não achei o botão de mensagem de voz do WhatsApp. Nada foi enviado.".into() };
    GRAVANDO.store(true, Ordering::Relaxed);
    clicar(p, x, y);
    p.dormir(((dur + FOLGA_S) * 1000.0) as u64);
    let r = (|| {
        let Some((x, y)) = rect(p, "footer [aria-label]", "^enviar") else {
            wv::cancelar_gravacao_presa(p);
            return "A gravação não abriu (o microfone falso pode estar desligado). Nada foi enviado.".to_string();
        };
        clicar(p, x, y);
        p.dormir(3000);
        if contar_audios(p) != antes { format!("✅ Áudio enviado para {alvo}.") } else { format!("Cliquei em enviar, mas não vi o áudio aparecer na conversa com {alvo}.") }
    })();
    GRAVANDO.store(false, Ordering::Relaxed);
    r
}

// ---------- ouvir audio ----------

fn base64(s: &str) -> Vec<u8> {
    let (mut saida, mut acc, mut bits) = (Vec::with_capacity(s.len() * 3 / 4), 0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => continue,
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            saida.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    saida
}

/// Os `n` audios mais recentes da conversa JA ABERTA, como bytes (OGG/Opus do WhatsApp). Instala o gancho, e se nenhum blob apareceu
/// aperta o play do ultimo audio dele. Teto de ESPERA_OUVIR_S no total.
pub fn baixar_audios(p: &mut dyn Pagina, n: usize) -> Result<Vec<Vec<u8>>, String> {
    let _g = TRAVA.lock().map_err(|_| "trava interna quebrada".to_string())?;
    let inicio = Instant::now();
    let passou = || inicio.elapsed().as_secs() >= ESPERA_OUVIR_S;
    p.js(JS_GANCHO)?;
    let conta = |p: &mut dyn Pagina| p.js(JS_CONTA_BLOBS).ok().and_then(|v| v.as_i64()).unwrap_or(0);
    // sempre aperta o play do ultimo audio dele: o gancho pega o blob que toca (mesmo o que o WhatsApp ja tinha guardado)
    let Some((x, y)) = ponto(p, JS_PLAY_DELE) else { return Err("não achei o botão de reproduzir do áudio dele".into()) };
    clicar(p, x, y);
    for _ in 0..12 {
        p.dormir(1000);
        if conta(p) > 0 || passou() {
            break;
        }
    }
    let total = conta(p);
    if total == 0 {
        return Err("o WhatsApp não entregou o áudio a tempo".into());
    }
    let mut out = vec![];
    for i in (total - (n.max(1) as i64).min(total))..total {
        let v = json_de(p.js(&JS_BLOB.replace("@I@", &i.to_string()))?);
        let b = base64(v["b64"].as_str().unwrap_or(""));
        if !b.is_empty() {
            out.push(b);
        }
        if passou() {
            break;
        }
    }
    if out.is_empty() { Err("o áudio veio vazio".into()) } else { Ok(out) }
}

/// Abre a conversa de `nome` (nome conferido), baixa os `n` ultimos audios e transcreve com `transcrever` (LOCAL). Falhou = o contato
/// fica AUDIO_PAUSA_S (20 min) sem nova tentativa, para nao mexer no WhatsApp de novo. Nunca devolve texto vazio como sucesso.
#[allow(clippy::too_many_arguments)]
pub fn ouvir_contato(p: &mut dyn Pagina, dados: &Path, ctx: &Ctx, nomes: &Nomes, nome: &str, n: usize, ts: f64, transcrever: &dyn Fn(&[u8]) -> Result<String, String>) -> Result<String, String> {
    if ts - pausa(dados, "audio", nome) < AUDIO_PAUSA_S {
        return Err("ouvir falhou há pouco: sem mexer no WhatsApp de novo".into());
    }
    let r = (|| {
        wv::abrir_destino(p, ctx, nomes, nome, true).map_err(|m| m.chars().take(120).collect::<String>())?;
        let textos = baixar_audios(p, n)?.iter().map(|b| transcrever(b)).collect::<Result<Vec<_>, _>>()?;
        let t = textos.join(" ").trim().to_string();
        if t.is_empty() { Err("não entendi o áudio".to_string()) } else { Ok(t) }
    })();
    if r.is_err() {
        marcar_pausa(dados, "audio", nome, ts);
    }
    r
}

// ---------- enviar arquivo ----------

/// Barra o que nao deve ir: nao existe, pasta, executavel, passa de 100 MB. Devolve o caminho como a VM enxerga.
pub fn validar_arquivo(caminho: &str) -> Result<String, String> {
    let c = Path::new(caminho);
    let meta = std::fs::metadata(c).map_err(|_| format!("arquivo não encontrado: {caminho}"))?;
    if !meta.is_file() {
        return Err("isso não é um arquivo".into());
    }
    let ext = c.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if BLOQUEADAS.contains(&ext.as_str()) {
        return Err(format!("arquivo .{ext} não é enviado (executável ou script); compacte antes"));
    }
    if meta.len() > LIMITE_MB * 1024 * 1024 {
        return Err(format!("arquivo de {} MB passa do limite de {LIMITE_MB} MB", meta.len() / 1024 / 1024));
    }
    caminho_vm(caminho)
}

fn fechar_previa(p: &mut dyn Pagina) {
    if let Some((x, y)) = ponto(p, JS_X_PREVIA) {
        clicar(p, x, y);
        p.dormir(800);
    }
}

fn esperar_icone(p: &mut dyn Pagina, seg: u32) -> Option<(f64, f64)> {
    for _ in 0..seg {
        if let Some(pt) = icone(p, "wds-ic-send-filled") {
            return Some(pt);
        }
        p.dormir(1000);
    }
    None
}

/// Envia o arquivo a `nome`, com legenda opcional. `como_video` manda pelo item "Fotos e vídeos" (o resto vai como Documento).
/// "✅ ..." = enviou; qualquer outra coisa = nada saiu (a previa que sobrar e fechada sem enviar).
#[allow(clippy::too_many_arguments)]
pub fn enviar_arquivo(p: &mut dyn Pagina, ctx: &Ctx, nomes: &Nomes, nome: &str, caminho: &str, legenda: &str, como_video: bool) -> String {
    let vm = match validar_arquivo(caminho) {
        Ok(v) => v,
        Err(e) => return format!("⛔ Arquivo barrado: {e}. Nada foi enviado."),
    };
    let Ok(_g) = TRAVA.lock() else { return "Arquivo não enviado: trava interna quebrada.".into() };
    let alvo = match wv::abrir_destino(p, ctx, nomes, nome, false) {
        Ok(a) => a,
        Err(m) => return m.replace("Texto barrado", "Arquivo barrado"),
    };
    wv::cancelar_gravacao_presa(p);
    fechar_previa(p);
    let Some((x, y)) = rect(p, "footer [aria-label], footer button, footer [role=button]", "^anexar|^attach") else { return "Não achei o botão Anexar. Nada foi enviado.".into() };
    clicar(p, x, y);
    let rx = if como_video { "^(fotos e v[ií]deos|photos (&|and) videos)$" } else { "^documento$" };
    let mut item = None;
    for tent in 0..8 {
        p.dormir(600);
        if let Some(pt) = ponto(p, &JS_ITEM_DOC.replace("@RX@", &aspas(rx))) {
            item = Some(pt);
            break;
        }
        if tent == 2 {
            let _ = p.js(JS_ANEXAR_CLICK);
        }
    }
    let Some((ix, iy)) = item else { return "Não achei a opção Documento/Fotos e vídeos no menu Anexar. Nada foi enviado.".into() };
    if let Err(e) = p.escolher_arquivo(ix, iy, &vm) {
        return format!("Não consegui entregar o arquivo ao WhatsApp ({e}). Nada foi enviado.");
    }
    if esperar_icone(p, 15).is_none() {
        fechar_previa(p);
        return "A pré-visualização do arquivo não abriu. Nada foi enviado.".into();
    }
    if !legenda.trim().is_empty() && p.js(JS_LEGENDA).is_ok_and(|v| v == true) {
        let _ = p.inserir(legenda.trim());
        p.dormir(400);
    }
    let Some((sx, sy)) = icone(p, "wds-ic-send-filled") else {
        fechar_previa(p);
        return "Não achei o botão de enviar da pré-visualização. Nada foi enviado.".into();
    };
    clicar(p, sx, sy);
    p.dormir(2000);
    let curto: String = Path::new(caminho).file_name().and_then(|f| f.to_str()).unwrap_or("").chars().take(30).collect();
    for _ in 0..120 {
        let v = p.js(&JS_ULTIMA.replace("@NOME@", &aspas(&curto))).map(json_de).unwrap_or_default();
        if v["achou"] == true && v["pendente"] != true && v["enviada"] == true {
            return format!("✅ Arquivo enviado para {alvo}.");
        }
        p.dormir(2000);
    }
    format!("O arquivo foi para {alvo}, mas o WhatsApp não confirmou a entrega em 4 minutos.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wpp_vivo::tests::{lista_de, tmp};
    use serde_json::{json, Value};

    /// Pagina de mentira com o rodape e o menu de anexo. Anota cada gesto.
    #[derive(Default)]
    struct Falsa {
        lista: String,
        titulo: String,
        rascunho: i64,
        tem_mic: bool,
        /// audios na conversa antes do envio; sobe 1 depois de 3 cliques (abrir, microfone, enviar) se `grava_ok`
        audios: i64,
        grava_ok: bool,
        cliques: usize,
        gestos: Vec<String>,
        blobs: Vec<String>,
        tem_play: bool,
        previa: bool,
        arquivos: Vec<String>,
        ultima_enviada: bool,
    }

    impl Pagina for Falsa {
        fn js(&mut self, e: &str) -> Result<Value, String> {
            let t = |s: &str| e.contains(s);
            if t("out.push({nome:t.getAttribute('title'), nl:n") {
                return Ok(json!(self.lista));
            }
            if t("scrollIntoView({block:'nearest'})") {
                return Ok(json!(r#"{"x":10,"y":20}"#));
            }
            if e.starts_with("[...document.querySelectorAll('header')]") {
                return Ok(json!(true));
            }
            if t("span[dir=auto]") {
                return Ok(json!(self.titulo));
            }
            if t("Pausar gravação") {
                return Ok(json!(false));
            }
            if t("return c ? (c.innerText||'').trim().length : 0") {
                return Ok(json!(self.rascunho));
            }
            if e == JS_CONTA_VOZ {
                return Ok(json!(self.audios + i64::from(self.grava_ok && self.cliques >= 3)));
            }
            if t("(function(escopo, rot)") {
                let achou = (e.contains("mensagem de voz\"") && self.tem_mic) || (e.contains("\"^enviar\"") && self.cliques >= 2) || (e.contains("^anexar") && self.tem_mic);
                return Ok(if achou { json!(r#"{"x":1,"y":2}"#) } else { Value::Null });
            }
            if e == JS_GANCHO {
                return Ok(json!(true));
            }
            if e == JS_CONTA_BLOBS {
                return Ok(json!(self.blobs.len()));
            }
            if t("var bs=window.__aud||[]; var b=bs[i];") {
                let i: usize = e.rsplit('(').next().unwrap().trim_end_matches(')').parse().unwrap();
                return Ok(json!(json!({"tipo": "audio/ogg", "b64": self.blobs[i]}).to_string()));
            }
            if t("[aria-label=\"Reproduzir mensagem de voz\"]')).forEach") {
                return Ok(if self.tem_play { json!(r#"{"x":5,"y":5}"#) } else { Value::Null });
            }
            if t("(function(rx)") {
                return Ok(json!(r#"{"x":3,"y":4}"#));
            }
            if t("(function(ic)") {
                return Ok(if self.previa { json!(r#"{"x":7,"y":8}"#) } else { Value::Null });
            }
            if t("data-icon=\"wds-ic-send-filled\"]');\n  for(var k=0") {
                return Ok(Value::Null);
            }
            if t("!e.closest('#side') && !e.closest('footer')") {
                return Ok(json!(true));
            }
            if t("var u=rows[rows.length-1]") {
                return Ok(json!(json!({"achou": true, "pendente": false, "enviada": self.ultima_enviada}).to_string()));
            }
            if t("if(b) b.click();})()") {
                return Ok(Value::Null);
            }
            Err(format!("fora do roteiro: {}", e.chars().take(40).collect::<String>()))
        }
        fn mouse(&mut self, tipo: &str, _: f64, _: f64) -> Result<(), String> {
            if tipo == "mousePressed" {
                self.cliques += 1;
            }
            self.gestos.push(format!("mouse:{tipo}"));
            Ok(())
        }
        fn inserir(&mut self, t: &str) -> Result<(), String> {
            self.gestos.push(format!("inserir:{t}"));
            Ok(())
        }
        fn enter(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn frente(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn escolher_arquivo(&mut self, _: f64, _: f64, c: &str) -> Result<(), String> {
            self.arquivos.push(c.to_string());
            self.previa = true;
            Ok(())
        }
        fn dormir(&mut self, _: u64) {}
    }

    fn wav(seg: f64, taxa: u32) -> Vec<u8> {
        escrever_wav(taxa, &vec![1000i16; (taxa as f64 * seg) as usize])
    }

    fn base(nome: &str) -> (std::path::PathBuf, Ctx, Nomes) {
        let d = tmp(nome);
        let (c, n) = (Ctx::ler(&d), Nomes::ler(&d));
        (d, c, n)
    }

    fn ana(titulo: &str) -> Falsa {
        Falsa { lista: lista_de(&["Ana"]), titulo: titulo.into(), tem_mic: true, ..Default::default() }
    }

    #[test]
    fn wav_vira_microfone_mono_com_silencio_e_limite() {
        let (d, _, _) = base("mic");
        let dur = preparar_mic(&d, &wav(2.0, 24000)).unwrap();
        assert!((dur - 2.0).abs() < 0.01);
        let (taxa, a) = ler_wav(&std::fs::read(d.join("mic.wav")).unwrap()).unwrap();
        assert_eq!((taxa, a.len()), (24000, 24000 * 2 + 36000)); // fala + 1,5 s de silencio
        assert!(a[a.len() - 1] == 0 && a[0] == 1000);
        // estereo vira mono pela media; longo demais e vazio sao barrados
        let mut w = escrever_wav(24000, &[1000i16, 3000].repeat(2400));
        w[22] = 2; // canais = 2
        assert_eq!(ler_wav(&w).unwrap().1[0], 2000);
        assert!(preparar_mic(&d, &wav(151.0, 8000)).unwrap_err().contains("limite"));
        assert!(preparar_mic(&d, &wav(0.1, 8000)).is_err() && preparar_mic(&d, b"lixo").is_err());
    }

    #[test]
    fn caminho_do_windows_para_a_vm() {
        assert_eq!(caminho_vm(r"E:\braniac-rs\dados\mic.wav").unwrap(), "/mnt/e/braniac-rs/dados/mic.wav");
        assert_eq!(caminho_vm("C:/Users/x/a.pdf").unwrap(), "/mnt/c/Users/x/a.pdf");
        assert_eq!(caminho_vm("/tmp/a").unwrap(), "/tmp/a");
        assert!(caminho_vm(r"\\servidor\pasta\a").is_err() && caminho_vm("relativo.txt").is_err());
        // o Chromium da VM tem de subir com o microfone falso apontando para o mic.wav daqui
        let sh = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/padrao/ligar.sh")).unwrap();
        for f in ["--use-fake-device-for-media-stream", "--use-fake-ui-for-media-stream", "--use-file-for-fake-audio-capture=/mnt/e/braniac-rs/dados/mic.wav%noloop"] {
            assert!(sh.contains(f), "{f}");
        }
    }

    #[test]
    fn audio_com_nome_errado_no_topo_nao_grava() {
        let (d, ctx, nomes) = base("audio-nome");
        let mut p = ana("Outra Pessoa");
        let r = enviar_audio(&mut p, &d, &ctx, &nomes, "Ana", &wav(1.0, 24000));
        assert!(r.starts_with("🔒"), "{r}");
        assert_eq!(p.cliques, 1, "so o clique que abre a conversa: {:?}", p.gestos); // nao clicou no microfone
        assert!(!gravando());
    }

    #[test]
    fn audio_envia_confere_e_um_por_vez() {
        let (d, ctx, nomes) = base("audio-ok");
        let mut p = Falsa { grava_ok: true, ..ana("Ana") };
        let r = enviar_audio(&mut p, &d, &ctx, &nomes, "Ana", &wav(1.0, 24000));
        assert!(r.starts_with('✅'), "{r}");
        // sem o audio aparecer na conversa: nao diz que enviou
        assert!(!enviar_audio(&mut ana("Ana"), &d, &ctx, &nomes, "Ana", &wav(1.0, 24000)).starts_with('✅'));
        // sem botao de microfone / com rascunho: nada gravado
        let mut r1 = Falsa { tem_mic: false, ..ana("Ana") };
        assert!(enviar_audio(&mut r1, &d, &ctx, &nomes, "Ana", &wav(1.0, 24000)).contains("Nada foi enviado") && r1.cliques == 1);
        let mut r2 = Falsa { rascunho: 5, ..ana("Ana") };
        assert!(enviar_audio(&mut r2, &d, &ctx, &nomes, "Ana", &wav(1.0, 24000)).contains("rascunho") && r2.cliques == 1);
        // um por vez: com a trava segura por outro envio, este espera (nao entra junto)
        let g = TRAVA.lock().unwrap();
        let (d2, ctx2, nomes2) = base("audio-trava");
        let h = std::thread::spawn(move || {
            let mut p = Falsa { grava_ok: true, ..ana("Ana") };
            (enviar_audio(&mut p, &d2, &ctx2, &nomes2, "Ana", &wav(1.0, 24000)), p.cliques)
        });
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(!h.is_finished(), "o segundo envio nao pode entrar com o primeiro em andamento");
        drop(g);
        let (r, cliques) = h.join().unwrap();
        assert!(r.starts_with('✅') && cliques == 3, "{r}");
    }

    #[test]
    fn pai_recebe_audio_e_nunca_texto_nem_arquivo() {
        let (d, ctx, nomes) = base("pai");
        let pai = || Falsa { lista: lista_de(&["Pai"]), titulo: "Pai".into(), tem_mic: true, ..Default::default() };
        let mut p = Falsa { grava_ok: true, ..pai() };
        assert!(enviar_audio(&mut p, &d, &ctx, &nomes, "Pai", &wav(1.0, 24000)).starts_with('✅'));
        let f = d.join("a.txt");
        std::fs::write(&f, "oi").unwrap();
        let mut q = pai();
        let r = enviar_arquivo(&mut q, &ctx, &nomes, "Pai", f.to_str().unwrap(), "", false);
        assert!(r.starts_with("⛔") && q.arquivos.is_empty() && q.gestos.is_empty(), "{r}");
        assert!(wv::enviar_texto(&mut q, &d, &ctx, &nomes, "Pai", "Oi pai, tudo bem?").starts_with("⛔"));
    }

    #[test]
    fn base64_decodifica() {
        assert_eq!(base64("T2dnUw=="), b"OggS");
        assert_eq!(base64("aGVsbG8gd29ybGQ="), b"hello world");
    }

    #[test]
    fn transcricao_que_falha_entra_na_pausa_de_20_minutos() {
        let (d, ctx, nomes) = base("pausa");
        let mut p = Falsa { blobs: vec!["T2dnUw==".into()], tem_play: true, ..ana("Ana") };
        let falha = |_: &[u8]| Err::<String, _>("transcritor fora do ar".to_string());
        let ts = 1.8e9;
        assert!(ouvir_contato(&mut p, &d, &ctx, &nomes, "Ana", 1, ts, &falha).is_err());
        // dentro de 20 min nem mexe na pagina; depois da pausa tenta de novo e, dessa vez, ouve
        let cliques = p.cliques;
        assert!(ouvir_contato(&mut p, &d, &ctx, &nomes, "Ana", 1, ts + 600.0, &|_| Ok("oi".into())).unwrap_err().contains("há pouco"));
        assert_eq!(p.cliques, cliques);
        let boa = |b: &[u8]| Ok::<String, String>(format!("{} bytes ouvidos", b.len()));
        assert_eq!(ouvir_contato(&mut p, &d, &ctx, &nomes, "Ana", 1, ts + AUDIO_PAUSA_S + 1.0, &boa).unwrap(), "4 bytes ouvidos");
    }

    #[test]
    fn sem_audio_na_pagina_aperta_o_play_e_sem_play_falha() {
        let mut p = Falsa::default();
        assert!(baixar_audios(&mut p, 1).unwrap_err().contains("reproduzir"));
        p.tem_play = true;
        let c = p.cliques;
        assert!(baixar_audios(&mut p, 1).is_err()); // nenhum blob aparece: erro, mas o play foi apertado
        assert_eq!(p.cliques, c + 1);
    }

    #[test]
    fn arquivo_valida_envia_com_legenda_e_confere() {
        let (d, ctx, nomes) = base("arquivo");
        let f = d.join("relatorio.pdf");
        std::fs::write(&f, "x").unwrap();
        let mut p = Falsa { ultima_enviada: true, ..ana("Ana") };
        let r = enviar_arquivo(&mut p, &ctx, &nomes, "Ana", f.to_str().unwrap(), "segue o relatório", false);
        assert!(r.starts_with('✅'), "{r}");
        assert!(p.arquivos[0].starts_with("/mnt/") && p.arquivos[0].ends_with("relatorio.pdf"), "{:?}", p.arquivos);
        assert!(p.gestos.contains(&"inserir:segue o relatório".to_string()));
        // executavel e inexistente: barrados sem tocar na pagina; nome errado no topo: nao anexa
        let exe = d.join("a.exe");
        std::fs::write(&exe, "x").unwrap();
        let mut q = ana("Ana");
        assert!(enviar_arquivo(&mut q, &ctx, &nomes, "Ana", exe.to_str().unwrap(), "", false).starts_with("⛔"));
        assert!(enviar_arquivo(&mut q, &ctx, &nomes, "Ana", "nao-existe.pdf", "", false).starts_with("⛔"));
        assert!(q.gestos.is_empty());
        let mut z = ana("Bia");
        assert!(enviar_arquivo(&mut z, &ctx, &nomes, "Ana", f.to_str().unwrap(), "", false).starts_with("🔒") && z.arquivos.is_empty());
    }
}
