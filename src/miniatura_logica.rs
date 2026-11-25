//! Miniatura de voz (Jarvis e Alma): SO a decisao, sem janela, rede nem audio. Duas casas usam este mesmo arquivo:
//! o servidor (rotas e testes) e o processo proprio `brani-mini` (via `#[path]`), que mostra a janelinha mesmo com o app fechado.
//! A miniatura so aparece quando alguem e chamado (Jarvis acordado, Alma na sessao) e some ao voltar a dormir ou sem atividade.
use serde_json::{json, Value};
use std::net::TcpListener;

pub const PORTA_TRAVA: u16 = 8768; // uma instancia so
pub const INATIVO_S: f64 = 120.0; // acordado e calado por tanto tempo: a miniatura some (o Jarvis nao dorme por isso)
pub const FIM_S: f64 = 4.0; // depois de dormir, ainda mostra "dormindo" por este tempo e some
pub const EXP_S: f64 = 300.0; // expandida e usada ha menos que isto: fica (mesmo calada ou com o agente dormindo)
pub const SEM_CONEXAO_S: f64 = 25.0; // com a miniatura aberta e o servidor mudo: mostra "sem conexao" e some depois disto

/// Trava de instancia unica: quem consegue a porta e o dono; a segunda copia recebe `None` e sai. Sem SO_REUSEADDR no Windows.
pub fn trava(porta: u16) -> Option<TcpListener> {
    TcpListener::bind(("127.0.0.1", porta)).ok()
}

/// Cor e nome por agente (a pagina traduz a cor em variaveis CSS).
pub fn agente_de(d: &Value) -> &'static str {
    if d["agente"] == "alma" || d["alma"]["ativa"] == true { "alma" } else { "jarvis" }
}

pub fn nome(agente: &str) -> &'static str {
    if agente == "alma" { "Alma" } else { "Jarvis" }
}

/// Alguem esta sendo chamado: o Jarvis acordado ou a Alma na sessao.
pub fn chamado(d: &Value) -> bool {
    d["estado"] == "acordado" || d["alma"]["ativa"] == true
}

fn curto(t: &str, n: usize) -> String {
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() <= n { t } else { format!("{}…", t.chars().take(n - 1).collect::<String>().trim_end()) }
}

/// Assinatura do historico (tamanho + ultimo texto): muda quando entra frase nova.
fn marca(d: &Value) -> (usize, String) {
    let h = d["historico"].as_array().map_or(&[][..], Vec::as_slice);
    (h.len(), h.last().and_then(|x| x[1].as_str()).unwrap_or("").to_string())
}

/// Corpo de uma resposta HTTP crua (depois da linha em branco). Resposta cortada pelo tempo limite ainda vale se ja veio inteira.
pub fn corpo_http(bruto: &str) -> Option<String> {
    bruto.split_once("\r\n\r\n").map(|x| x.1.to_string()).filter(|c| !c.trim().is_empty())
}

/// Itens do historico que sao novos: o `atual` menos o maior pedaco do comeco que ja era o fim do `anterior` (o historico da voz so guarda as ultimas 40).
pub fn novos<'a>(anterior: &[(String, String)], atual: &'a [(String, String)]) -> &'a [(String, String)] {
    for k in (0..=anterior.len().min(atual.len())).rev() {
        if anterior[anterior.len() - k..] == atual[..k] {
            return &atual[k..];
        }
    }
    atual
}

/// Janela expandida: cresce para CIMA se esta na metade de baixo da tela e para a ESQUERDA se esta na metade da direita (a ancora fica no canto
/// mais proximo da borda) e nunca sai da tela. Devolve (posicao nova, (cresceu_para_cima, cresceu_para_esquerda)). Tudo em pixels fisicos.
pub fn expandir(pos: (i64, i64), tela: (i64, i64), de: (i64, i64), para: (i64, i64)) -> ((i64, i64), (bool, bool)) {
    let (cima, esq) = (pos.1 + de.1 / 2 > tela.1 / 2, pos.0 + de.0 / 2 > tela.0 / 2);
    let x = if esq { pos.0 - (para.0 - de.0) } else { pos.0 };
    let y = if cima { pos.1 - (para.1 - de.1) } else { pos.1 };
    ((x.clamp(0, (tela.0 - para.0).max(0)), y.clamp(0, (tela.1 - para.1).max(0))), (cima, esq))
}

/// Volta ao tamanho de hoje desfazendo o crescimento (mesma ancora).
pub fn recolher(pos: (i64, i64), tela: (i64, i64), expandida: (i64, i64), normal: (i64, i64), ancora: (bool, bool)) -> (i64, i64) {
    let x = if ancora.1 { pos.0 + (expandida.0 - normal.0) } else { pos.0 };
    let y = if ancora.0 { pos.1 + (expandida.1 - normal.1) } else { pos.1 };
    (x.clamp(0, (tela.0 - normal.0).max(0)), y.clamp(0, (tela.1 - normal.1).max(0)))
}

#[derive(Default)]
pub struct Mini {
    expandida: bool,
    ultimo_toque: f64,
    log: Vec<(char, String)>,
    ant: Vec<(String, String)>,
    visivel: bool,
    /// fechou pelo X: fica escondida ate o agente dormir e ser chamado de novo
    dispensada: bool,
    ultimo_sinal: f64,
    fim: Option<f64>,
    sem_conexao_desde: Option<f64>,
    visto: (usize, String),
    falando_ate: f64,
    pensando_ate: f64,
    fala: String,
    chamado_antes: bool,
}

impl Mini {
    pub fn visivel(&self) -> bool {
        self.visivel
    }

    pub fn expandida(&self) -> bool {
        self.expandida
    }

    /// Expandir/recolher (botao). Mexer na janela conta como uso.
    pub fn expandir(&mut self, ligar: bool, agora: f64) {
        self.expandida = ligar;
        self.ultimo_toque = agora;
    }

    /// Clique, foco ou tecla na miniatura expandida: segura a janela na tela.
    pub fn tocar(&mut self, agora: f64) {
        self.ultimo_toque = agora;
    }

    fn empurrar(&mut self, papel: char, texto: &str) {
        let t = texto.trim();
        if t.is_empty() || self.log.iter().rev().take(6).any(|(p, x)| *p == papel && x == t) {
            return;
        }
        self.log.push((papel, t.chars().take(1500).collect()));
        if self.log.len() > 80 {
            self.log.remove(0);
        }
    }

    /// Aviso curto no historico (ex.: servidor sem resposta).
    pub fn aviso(&mut self, texto: &str, agora: f64) {
        self.empurrar('s', texto);
        self.pensando_ate = 0.0;
        self.ultimo_sinal = agora;
    }

    /// O dono digitou: entra no historico e o agente fica "pensando" ate a resposta.
    pub fn enviado(&mut self, texto: &str, agora: f64) {
        self.empurrar('u', texto);
        self.pensando_ate = agora + 90.0;
        self.ultimo_sinal = agora;
        self.ultimo_toque = agora;
    }

    /// A resposta escrita do agente ao que foi digitado.
    pub fn resposta(&mut self, agente: &str, texto: &str, agora: f64) {
        self.empurrar(if agente == "alma" { 'a' } else { 'j' }, texto);
        self.pensando_ate = 0.0;
        self.fala = curto(texto, 140);
        self.ultimo_sinal = agora;
    }

    /// O botao fechar: some agora e so volta numa nova chamada.
    pub fn dispensar(&mut self) {
        self.dispensada = true;
        self.visivel = false;
    }

    /// Um ciclo. `d` = resposta de /api/escuta-estado (None = servidor fora do ar), `agora` em segundos.
    /// Devolve o quadro que a pagina desenha; `visivel` diz se a janela deve estar na tela.
    pub fn passo(&mut self, d: Option<&Value>, agora: f64) -> Value {
        let Some(d) = d else {
            let desde = *self.sem_conexao_desde.get_or_insert(agora);
            if self.visivel && agora - desde > SEM_CONEXAO_S {
                self.visivel = false;
            }
            return self.quadro(&Value::Null, agora, true);
        };
        self.sem_conexao_desde = None;
        let ativo = chamado(d);
        // nova chamada (dormia, agora acordou): reabre, mesmo que tenha sido fechada pelo X
        if ativo && !self.chamado_antes {
            self.dispensada = false;
            self.ultimo_sinal = agora;
            self.fim = None;
            self.visto = marca(d); // a conversa antiga nao conta como fala nova
            self.fala.clear();
        }
        self.chamado_antes = ativo;
        self.observar(d, agora);
        if ativo {
            self.fim = None;
            let calado = agora - self.ultimo_sinal > INATIVO_S;
            self.visivel = !self.dispensada && (!calado || self.em_uso(agora));
        } else if self.visivel {
            let f = *self.fim.get_or_insert(agora);
            if agora - f > FIM_S && !self.em_uso(agora) {
                self.visivel = false;
            }
        }
        self.quadro(d, agora, false)
    }

    fn em_uso(&self, agora: f64) -> bool {
        self.expandida && agora - self.ultimo_toque < EXP_S
    }

    /// Le o historico: fala nova = "falando"; frase nova do dono = "pensando"; qualquer movimento zera o relogio de inatividade.
    fn observar(&mut self, d: &Value, agora: f64) {
        let h = d["historico"].as_array().cloned().unwrap_or_default();
        let ult = h.last().map(|x| (x[0].as_str().unwrap_or("").to_string(), x[1].as_str().unwrap_or("").to_string()));
        let atual: Vec<(String, String)> = h.iter().map(|x| (x[0].as_str().unwrap_or("").to_string(), x[1].as_str().unwrap_or("").to_string())).collect();
        let agente = agente_de(d);
        for (p, t) in novos(&self.ant, &atual).to_vec() {
            self.empurrar(if p == "j" && agente == "alma" { 'a' } else { p.chars().next().unwrap_or('s') }, &t);
        }
        self.ant = atual;
        let m = marca(d);
        if m != self.visto && !h.is_empty() {
            self.visto = m;
            self.ultimo_sinal = agora;
            if let Some((papel, texto)) = ult {
                if papel == "u" {
                    self.pensando_ate = agora + 20.0;
                    self.falando_ate = 0.0;
                } else if papel == "j" {
                    self.pensando_ate = 0.0;
                    self.falando_ate = agora + (texto.chars().count() as f64 / 14.0).clamp(1.5, 12.0);
                    self.fala = curto(&texto, 140);
                }
                if papel == "u" && self.fala.is_empty() {
                    self.fala = curto(&texto, 140);
                }
            }
        }
        if d["entrada"].as_f64().unwrap_or(0.0) > 0.25 && d["estado"] == "acordado" {
            self.ultimo_sinal = agora; // ele esta falando
        }
        if d["alma"]["ativa"] == true {
            if let Some(l) = d["alma"]["legenda"].as_str().filter(|l| !l.trim().is_empty()) {
                if self.fala != curto(l, 140) {
                    self.fala = curto(l, 140);
                    self.ultimo_sinal = agora;
                    self.falando_ate = agora + 3.0;
                }
            }
        }
    }

    fn quadro(&self, d: &Value, agora: f64, offline: bool) -> Value {
        let agente = agente_de(d);
        let ativo = chamado(d);
        let entrada = d["entrada"].as_f64().unwrap_or(0.0);
        let est = if offline {
            "offline"
        } else if !ativo {
            "dormindo"
        } else if agora < self.falando_ate {
            "falando"
        } else if agora < self.pensando_ate {
            "pensando"
        } else {
            "ouvindo"
        };
        let rotulo = match est {
            "offline" => "sem conexão",
            "dormindo" => "dormindo",
            "falando" => "falando",
            "pensando" => "pensando",
            _ => "ouvindo",
        };
        let energia = match est {
            "falando" => 0.55 + 0.35 * (agora * 9.0).sin().abs(),
            "ouvindo" => entrada.clamp(0.0, 1.0),
            "pensando" => 0.3,
            _ => 0.0,
        };
        json!({
            "visivel": self.visivel, "agente": agente, "nome": nome(agente), "est": est, "rotulo": rotulo,
            "fala": self.fala, "energia": energia, "offline": offline,
            "mic": d["microfone"]["microfone_ligado"].as_bool().unwrap_or(d["estado"] != "parada"),
            "mudo": d["mudo"]["mudo"] == true,
            "expandida": self.expandida,
            "chat": if self.expandida { json!(self.log.iter().map(|(p, t)| json!([p.to_string(), t])).collect::<Vec<_>>()) } else { json!([]) },
        })
    }
}

/// Posicao guardada: vale se ainda cabe na tela (monitor que sumiu = canto de baixo a direita).
pub fn posicao(salva: Option<(i64, i64)>, tela: (i64, i64), tam: (i64, i64)) -> (i64, i64) {
    match salva {
        Some((x, y)) if x > -tam.0 + 40 && y >= 0 && x < tela.0 - 40 && y < tela.1 - 40 => (x, y),
        _ => (tela.0 - tam.0 - 24, tela.1 - tam.1 - 72),
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn acordado(h: &[(&str, &str)]) -> Value {
        json!({ "estado": "acordado", "agente": "jarvis", "historico": h.iter().map(|(p, t)| json!([p, t])).collect::<Vec<_>>(), "entrada": 0.0, "alma": { "ativa": false, "legenda": "" } })
    }
    fn dormindo() -> Value {
        json!({ "estado": "dormindo", "agente": "jarvis", "historico": [], "entrada": 0.0, "alma": { "ativa": false, "legenda": "" } })
    }

    #[test]
    fn so_aparece_quando_chamado_e_some_ao_dormir() {
        let mut m = Mini::default();
        assert_eq!(m.passo(Some(&dormindo()), 0.0)["visivel"], false);
        assert_eq!(m.passo(Some(&acordado(&[])), 1.0)["visivel"], true);
        assert_eq!(m.passo(Some(&dormindo()), 2.0)["visivel"], true); // ainda mostra "dormindo" um instante
        assert_eq!(m.passo(Some(&dormindo()), 2.0)["est"], "dormindo");
        assert_eq!(m.passo(Some(&dormindo()), 2.0 + FIM_S + 1.0)["visivel"], false);
    }

    #[test]
    fn some_sozinha_sem_atividade_e_volta_com_fala_nova() {
        let mut m = Mini::default();
        m.passo(Some(&acordado(&[])), 0.0);
        assert!(m.visivel());
        assert_eq!(m.passo(Some(&acordado(&[])), INATIVO_S + 1.0)["visivel"], false);
        let q = m.passo(Some(&acordado(&[("u", "que horas sao")])), INATIVO_S + 2.0);
        assert_eq!((q["visivel"].as_bool(), q["est"].as_str()), (Some(true), Some("pensando")));
    }

    #[test]
    fn fala_do_agente_vira_falando_e_guarda_a_ultima_fala() {
        let mut m = Mini::default();
        m.passo(Some(&acordado(&[])), 0.0);
        let q = m.passo(Some(&acordado(&[("u", "oi"), ("j", "Boa noite, senhor.")])), 1.0);
        assert_eq!((q["est"].as_str(), q["fala"].as_str()), (Some("falando"), Some("Boa noite, senhor.")));
        assert_eq!(m.passo(Some(&acordado(&[("u", "oi"), ("j", "Boa noite, senhor.")])), 30.0)["est"], "ouvindo");
    }

    #[test]
    fn alma_muda_cor_e_nome() {
        let mut m = Mini::default();
        let d = json!({ "estado": "dormindo", "agente": "jarvis", "alma": { "ativa": true, "legenda": "Estou aqui com você." }, "historico": [] });
        let q = m.passo(Some(&d), 0.0);
        assert_eq!((q["agente"].as_str(), q["nome"].as_str(), q["visivel"].as_bool()), (Some("alma"), Some("Alma"), Some(true)));
        assert_eq!(q["fala"], "Estou aqui com você.");
    }

    #[test]
    fn fechar_esconde_ate_nova_chamada() {
        let mut m = Mini::default();
        m.passo(Some(&acordado(&[])), 0.0);
        m.dispensar();
        assert_eq!(m.passo(Some(&acordado(&[("j", "ainda aqui")])), 1.0)["visivel"], false);
        m.passo(Some(&dormindo()), 2.0);
        assert_eq!(m.passo(Some(&acordado(&[])), 3.0)["visivel"], true);
    }

    #[test]
    fn servidor_fora_mostra_sem_conexao_sem_quebrar_e_nunca_abre_sozinha() {
        let mut m = Mini::default();
        assert_eq!(m.passo(None, 0.0)["visivel"], false); // nunca apareceu: continua escondida
        m.passo(Some(&acordado(&[])), 1.0);
        let q = m.passo(None, 2.0);
        assert_eq!((q["visivel"].as_bool(), q["est"].as_str(), q["rotulo"].as_str()), (Some(true), Some("offline"), Some("sem conexão")));
        assert_eq!(m.passo(None, 2.0 + SEM_CONEXAO_S + 1.0)["visivel"], false);
    }

    fn h(p: &str, t: &str) -> (String, String) {
        (p.to_string(), t.to_string())
    }

    #[test]
    fn historico_novo_e_resposta_http_cortada() {
        let a = vec![h("u", "oi"), h("j", "ola")];
        let b = vec![h("u", "oi"), h("j", "ola"), h("u", "hora")];
        assert_eq!(novos(&a, &b), &b[2..]);
        assert_eq!(novos(&[], &b).len(), 3);
        assert_eq!(novos(&a, &[h("u", "outra")]).len(), 1); // historico trocado: tudo e novo
        assert_eq!(corpo_http("HTTP/1.0 200 OK\r\nX: y\r\n\r\n{\"ok\":true}").as_deref(), Some("{\"ok\":true}"));
        assert_eq!(corpo_http("HTTP/1.0 200 OK"), None);
    }

    #[test]
    fn expandida_cresce_para_cima_sem_sair_da_tela_e_volta_igual() {
        let tela = (1920, 1080);
        let (n, e) = ((308, 148), (358, 488));
        let (p, anc) = expandir((1596, 868), tela, n, e);
        assert_eq!((p, anc), ((1546, 528), (true, true))); // canto de baixo a direita: cresce para cima e para a esquerda
        assert_eq!(recolher(p, tela, e, n, anc), (1596, 868)); // recolher devolve ao lugar de antes
        let (p, anc) = expandir((10, 10), tela, n, e);
        assert_eq!((p, anc), ((10, 10), (false, false))); // canto de cima a esquerda: cresce para baixo
        assert!(expandir((1700, 900), tela, n, e).0 .0 + e.0 <= 1920);
    }

    #[test]
    fn expandida_nao_some_por_inatividade_enquanto_usada_e_guarda_o_que_foi_dito() {
        let mut m = Mini::default();
        m.passo(Some(&acordado(&[])), 0.0);
        m.expandir(true, 0.0);
        assert_eq!(m.passo(Some(&acordado(&[])), INATIVO_S + 10.0)["visivel"], true); // calada, mas expandida e usada
        m.enviado("que horas sao", INATIVO_S + 11.0);
        m.resposta("jarvis", "Sao 1h, senhor.", INATIVO_S + 12.0);
        let q = m.passo(Some(&dormindo()), INATIVO_S + 20.0); // dormiu, mas ele esta usando
        assert_eq!(q["visivel"], true);
        assert_eq!(q["chat"], json!([["u", "que horas sao"], ["j", "Sao 1h, senhor."]]));
        assert_eq!(m.passo(Some(&dormindo()), INATIVO_S + 11.0 + EXP_S + 20.0)["visivel"], false); // largou de vez: some
        m.expandir(false, 1.0);
        assert_eq!(m.passo(Some(&dormindo()), 2.0)["chat"], json!([]));
    }

    #[test]
    fn historico_da_voz_vira_balao_com_o_agente_certo() {
        let mut m = Mini::default();
        let d = json!({ "estado": "acordado", "agente": "alma", "alma": { "ativa": true, "legenda": "" }, "historico": [["u", "oi alma"], ["j", "oi, estou aqui"]], "entrada": 0.0 });
        m.expandir(true, 0.0);
        let q = m.passo(Some(&d), 0.0);
        assert_eq!(q["chat"], json!([["u", "oi alma"], ["a", "oi, estou aqui"]]));
        assert_eq!(m.passo(Some(&d), 1.0)["chat"].as_array().unwrap().len(), 2); // sem duplicar
    }

    #[test]
    fn uma_instancia_so_e_posicao_valida() {
        let a = trava(0).expect("porta livre");
        let porta = a.local_addr().unwrap().port();
        assert!(trava(porta).is_none()); // a segunda copia nao consegue a trava
        drop(a);
        assert!(trava(porta).is_some());
        assert_eq!(posicao(Some((100, 200)), (1920, 1080), (300, 140)), (100, 200));
        assert_eq!(posicao(Some((5000, 200)), (1920, 1080), (300, 140)), (1596, 868)); // monitor que sumiu
        assert_eq!(posicao(None, (1920, 1080), (300, 140)), (1596, 868));
    }
}
