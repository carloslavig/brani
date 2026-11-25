//! Roteador de IAs: tenta os provedores disponiveis na ordem da cadeia, com e sem ferramentas.
//! Porte do nucleo de braniac/router.py (ainda sem disputa em paralelo e ordenacao por latencia).
use crate::banco::Banco;
use crate::provedores::ErroProvedor;
use crate::store::Store;
use crate::{parecer, texto_ferramenta, travas};
use serde_json::{json, Value};
use std::collections::HashMap;

/// (provedor, mensagens, chave, timeout em segundos) -> texto
pub type Chamador = Box<dyn Fn(&Value, &Value, &str, u64) -> Result<String, ErroProvedor> + Send + Sync>;
/// (provedor, mensagens, chave, timeout, ferramentas) -> mensagem do assistente no formato OpenAI
pub type ChamadorTools = Box<dyn Fn(&Value, &Value, &str, u64, &Value) -> Result<Value, ErroProvedor> + Send + Sync>;
/// (nome, argumentos, tipo do provedor que pediu) -> (resultado, trouxe dado da camada local)
pub type Executor<'a> = &'a dyn Fn(&str, &Value, &str) -> (String, bool);

const SEM_RESPOSTA: &str = "Não consegui fechar uma resposta agora. Pode me dizer em uma frase o que precisa?";
const TIREI_NUMERO: &str = "(Tirei da resposta um número que não consegui confirmar em nenhuma fonte.)";
const REVISOR: &str = "Voce e um REVISOR rigoroso. Recebe as FONTES (tudo que o assistente tinha: conversa, BANCO, resultados de ferramenta) e a RESPOSTA dele. \
Diga se a RESPOSTA afirma algum FATO ESPECIFICO que NAO esta nas FONTES: nome de pessoa, numero, valor, data, horario, lugar, compromisso, pedido, algo que alguem disse, \
ou algo que o assistente diz que FEZ (enviou, abriu, guardou, agendou) sem resultado de ferramenta confirmando. \
NAO conta como invencao: cumprimento, pergunta, oferta de ajuda, dizer que nao sabe, explicacao de conhecimento geral sem dado especifico da pessoa, e conta pedida pela pessoa. \
Responda SO com JSON, sem mais nada: {\"inventou\": false} ou {\"inventou\": true, \"trecho\": \"<copie da RESPOSTA, igualzinho, o trecho inventado>\"}";
const NAO_CONCLUI: &str = "Não consegui concluir isso agora. Pode pedir de novo?";

pub struct Roteador {
    pub cfg: Value,
    pub store: Store,
    pub banco: Banco,
    pub chamar: Chamador,
    pub chamar_tools: ChamadorTools,
    pub ler_chave: Box<dyn Fn(&str) -> String + Send + Sync>,
    /// (fala de agora, tipo do provedor) -> bloco extra do prompt (o agente do assunto e os fatos dele); "" = nada
    pub contexto: Box<dyn Fn(&str, &str) -> String + Send + Sync>,
    /// (tipo, detalhe): registro de cada coisa que o verificador de respostas pegou
    pub vigia: Box<dyn Fn(&str, &str) + Send + Sync>,
}

/// 11 ou mais digitos seguidos (ignorando espaco, ponto e traco entre eles): CPF, cartao, conta.
/// Mais largo que o regex do Python de proposito: na duvida o texto nao vai a IA online.
fn tem_numero_longo(texto: &str) -> bool {
    let mut seguidos = 0;
    for c in texto.chars() {
        if c.is_ascii_digit() {
            seguidos += 1;
            if seguidos >= 11 {
                return true;
            }
        } else if !matches!(c, ' ' | '.' | '-') {
            seguidos = 0;
        }
    }
    false
}

fn corta(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn txt(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}

impl Roteador {
    /// Trava: texto sensivel nunca vai a provedor online.
    pub fn sensivel(&self, texto: &str) -> bool {
        let t = texto.to_lowercase();
        tem_numero_longo(texto)
            || self.cfg["sensivel"].as_array().into_iter().flatten()
                .filter_map(Value::as_str).any(|p| t.contains(&p.to_lowercase()))
    }

    fn chave(&self, p: &Value) -> String {
        (self.ler_chave)(txt(&p["chave_env"])).trim().to_string()
    }

    fn timeout(&self) -> u64 {
        self.cfg["timeout_s"].as_u64().unwrap_or(45)
    }

    pub fn disponivel(&self, p: &Value) -> (bool, String) {
        let (nome, tipo) = (txt(&p["nome"]), txt(&p["tipo"]));
        if tipo == "online" && self.chave(p).is_empty() {
            return (false, "sem chave".into());
        }
        if tipo == "claude" && self.store.uso_hoje("claude") >= self.cfg["claude_max_por_dia"].as_i64().unwrap_or(20) {
            return (false, "teto diario do Claude atingido".into());
        }
        // limite_dia vem como numero ou texto no json
        let limite = p["limite_dia"].as_f64().or_else(|| p["limite_dia"].as_str().and_then(|s| s.parse().ok()));
        if limite.is_some_and(|l| self.store.uso_hoje(nome) as f64 >= l * 0.9) {
            return (false, "limite diario do plano gratis".into());
        }
        let espera = self.store.cooldown(nome);
        if espera > 0.0 {
            return (false, format!("em descanso ({}s): {}", espera as u64, self.store.erro(nome)));
        }
        (true, String::new())
    }

    pub fn cadeia_para(&self, sensivel: bool) -> Vec<Value> {
        self.cfg["cadeia"].as_array().into_iter().flatten()
            .filter(|p| !(sensivel && p["tipo"] == "online")).cloned().collect()
    }

    /// Tenta cada provedor disponivel na ordem; quem falha com descanso sai da fila pelo tempo certo.
    fn tentar<T>(&self, cadeia: Vec<Value>, f: impl Fn(&Value, &str) -> Result<T, ErroProvedor>) -> Result<(T, Value), String> {
        let mut motivos = Vec::new();
        for p in cadeia {
            let nome = txt(&p["nome"]).to_string();
            let (ok, motivo) = self.disponivel(&p);
            if !ok {
                motivos.push(format!("{nome}: {motivo}"));
                continue;
            }
            match f(&p, &self.chave(&p)) {
                Ok(r) => {
                    let _ = self.store.add_uso(&nome);
                    return Ok((r, p));
                }
                Err(e) => {
                    if e.descanso > 0 {
                        let _ = self.store.set_cooldown(&nome, e.descanso as f64, &e.msg);
                    }
                    motivos.push(format!("{nome}: {}", e.msg));
                }
            }
        }
        Err(format!("nenhuma IA respondeu ({})", motivos.join("; ")))
    }

    /// Cada provedor recebe o banco da SUA camada: online so ve o que e publico.
    fn sistema_para(&self, p: &Value, texto: &str) -> Value {
        let (bloco, _) = self.banco.consultar(texto, txt(&p["tipo"]), None, 8);
        json!({ "role": "system", "content": format!("{}{}\n\nBANCO:\n{bloco}", txt(&self.cfg["system"]), (self.contexto)(texto, txt(&p["tipo"]))) })
    }

    /// sistema + historico + fala de agora + mensagens deste turno. Provedor online nao recebe do historico
    /// os turnos que ficaram so no PC (marcados "local/") nem fala antiga com texto sensivel.
    fn msgs_para(&self, p: &Value, hist: &[Value], texto: &str, turno: &[Value]) -> Value {
        let online = p["tipo"] == "online";
        let mut msgs = vec![self.sistema_para(p, texto)];
        msgs.extend(hist.iter()
            .filter(|m| !online || !(txt(&m["provider"]).starts_with("local/") || self.sensivel(txt(&m["content"]))))
            .map(|m| json!({ "role": m["role"], "content": m["content"] })));
        msgs.push(json!({ "role": "user", "content": texto }));
        if self.cfg["poda_contexto"].as_bool().unwrap_or(false) { msgs.extend(crate::poda::podar(turno, 2, 600)); } else { msgs.extend(turno.iter().cloned()); }
        Value::Array(msgs)
    }

    fn historico(&self, conv: &str) -> Vec<Value> {
        self.store.historico(conv, self.cfg["historico_max_msgs"].as_u64().unwrap_or(20) as usize)
    }

    /// Grava o turno (pergunta e resposta juntas). `local` = passou por dado que nao pode ir a IA online.
    fn fechar(&self, conv: &str, texto: &str, resp: &str, nome: &str, local: bool) -> Result<(String, String), String> {
        let marca = if local { format!("local/{nome}") } else { nome.to_string() };
        self.store.add_msg(conv, "user", texto, Some(&marca))?;
        self.store.add_msg(conv, "assistant", resp, Some(&marca))?;
        Ok((resp.to_string(), nome.to_string()))
    }

    #[cfg(test)]
    pub fn responder(&self, conv: &str, texto: &str) -> Result<(String, String), String> {
        self.responder_com(conv, texto, "")
    }

    /// `extra` = bloco que vai junto do pedido so neste turno (a IA le; o historico guarda so o que a pessoa disse).
    pub fn responder_com(&self, conv: &str, dito: &str, extra: &str) -> Result<(String, String), String> {
        let pedido = format!("{dito}{extra}");
        let texto = pedido.as_str();
        let sensivel = self.sensivel(texto);
        let hist = self.historico(conv);
        let (mut resp, p) = self.tentar(self.cadeia_para(sensivel), |p, chave| (self.chamar)(p, &self.msgs_para(p, &hist, texto, &[]), chave, self.timeout()))?;
        // IA local recebeu o banco inteiro: a resposta dela pode citar dado que nao sai do PC
        let local = sensivel || p["tipo"] != "online";
        let (base, conferir) = (self.base(&p, &hist, texto, &[]), !travas::eh_conta(texto));
        let (dica, suspeito) = self.vigiar(&resp, &base, conferir, &hist, texto, &[], local, txt(&p["nome"]));
        if let Some(dica) = dica {
            // refaz UMA vez, com o defeito dito ao modelo
            let turno = [json!({ "role": "assistant", "content": if resp.is_empty() { "(vazio)" } else { &resp } }), json!({ "role": "user", "content": dica })];
            if let Ok((r2, _)) = self.tentar(self.cadeia_para(local), |p, chave| (self.chamar)(p, &self.msgs_para(p, &hist, texto, &turno), chave, self.timeout())) {
                resp = r2;
            }
        }
        self.fechar(conv, dito, &self.acabar(&resp, &base, conferir, &suspeito), txt(&p["nome"]), local)
    }

    /// Tudo que o modelo teve na mao neste pedido: fonte dos numeros que ele pode citar. A dica de refazer e a
    /// resposta reprovada (a anterior a dica) nao contam, senao o numero inventado se "confirmava" sozinho.
    fn base(&self, p: &Value, hist: &[Value], texto: &str, turno: &[Value]) -> String {
        let dica = |i: usize| turno.get(i).is_some_and(|m| txt(&m["content"]).starts_with("[ATENCAO:"));
        let mut partes = vec![texto.to_string(), txt(&self.sistema_para(p, texto)["content"]).to_string()];
        partes.extend(hist.iter().map(|m| txt(&m["content"]).to_string()));
        partes.extend(turno.iter().enumerate().filter(|(i, _)| !dica(*i) && !dica(i + 1)).map(|(_, m)| txt(&m["content"]).to_string()));
        partes.join("\n")
    }

    /// `houve_ferramenta` aqui = CONFERIR os numeros: sempre, menos quando a pessoa pediu uma conta e nenhuma ferramenta rodou.
    /// Numero de 2+ digitos que nao esta em nenhuma fonte (conversa, BANCO, ferramenta) e tratado como inventado.
    fn inventados(&self, fim: &str, base: &str, houve_ferramenta: bool) -> Vec<String> {
        if houve_ferramenta { travas::numeros_inventados(fim, base, &self.store.agora_partes()) } else { Vec::new() }
    }

    fn defeito(&self, fim: &str, base: &str, houve_ferramenta: bool) -> Option<String> {
        if travas::resposta_crua(fim) {
            return Some("[ATENCAO: sua resposta anterior veio vazia ou com o resultado cru da ferramenta. Responda em uma ou duas frases, com suas palavras, dizendo o que foi feito ou o que o dado mostra.]".into());
        }
        let fora = self.inventados(fim, base, houve_ferramenta);
        if !fora.is_empty() {
            (self.vigia)("numero_sem_fonte", &fora.join(", "));
        }
        (!fora.is_empty()).then(|| format!("[ATENCAO: voce citou {} e esse(s) numero(s) NAO aparece(m) em nenhuma fonte (conversa, BANCO, resultado de ferramenta). Confirme com uma ferramenta (consultar_banco, pesquisar_web) ou refaca dizendo que nao sabe. Nao chute.]", fora.join(", ")))
    }

    /// Ultima barreira, sem IA: resposta crua vira pedido de repeticao; frase com numero inventado sai.
    fn consertar(&self, fim: &str, base: &str, houve_ferramenta: bool) -> String {
        if travas::resposta_crua(fim) {
            return SEM_RESPOSTA.to_string();
        }
        let fora = self.inventados(fim, base, houve_ferramenta);
        if fora.is_empty() {
            return fim.to_string();
        }
        (self.vigia)("numero_removido", &fora.join(", "));
        let limpo = travas::sem_frases_com(fim, &fora);
        // quando sobra resposta, a pessoa fica sabendo que algo foi tirado (quando nao sobra, o texto ja diz que nao ha o numero)
        if limpo.starts_with("Não tenho esse número") { limpo } else { format!("{limpo}\n\n{TIREI_NUMERO}") }
    }

    /// REVISOR: uma segunda IA confere a resposta contra as fontes. Some(trecho) = afirmou algo sem fonte.
    /// Revisor fora do ar ou parecer ilegivel = None: a resposta nunca fica presa por causa dele.
    /// A revisora recebe as fontes da camada DELA (online so ve o que e publico); resposta de IA local so e revista por IA local.
    #[allow(clippy::too_many_arguments)]
    fn revisar(&self, hist: &[Value], texto: &str, turno: &[Value], resposta: &str, local: bool, autor: &str) -> Option<String> {
        if resposta.chars().count() < 40 && !resposta.chars().any(|c| c.is_ascii_digit()) {
            return None; // cumprimento ou frase curta sem numero: nao ha fato para conferir
        }
        let dica = |i: usize| turno.get(i).is_some_and(|m| txt(&m["content"]).starts_with("[ATENCAO:"));
        let limpo: Vec<Value> = turno.iter().enumerate().filter(|(i, _)| !dica(*i) && !dica(i + 1)).map(|(_, m)| m.clone()).collect();
        let rapidas: Vec<Value> = self.cadeia_para(local).into_iter().filter(|p| p["tipo"] != "claude").collect(); // o Claude leva 30 s: nao serve de revisor
        let outras: Vec<Value> = rapidas.iter().filter(|p| p["nome"] != autor).cloned().collect();
        let saida = self.tentar(if outras.is_empty() { rapidas } else { outras }, |p, chave| {
            let fontes: Vec<String> = self.msgs_para(p, hist, texto, &limpo).as_array().into_iter().flatten().map(|m| txt(&m["content"]).to_string()).collect();
            let c: Vec<char> = fontes.join("\n").chars().collect();
            // fontes longas: fica o comeco (regras e BANCO) e o fim (a fala e os resultados de ferramenta)
            let fontes: String = if c.len() > 7000 { c[..2500].iter().chain(&c[c.len() - 4500..]).collect() } else { c.iter().collect() };
            (self.chamar)(p, &json!([{ "role": "system", "content": REVISOR }, { "role": "user", "content": format!("FONTES:\n{fontes}\n\nRESPOSTA:\n{resposta}") }]), chave, self.timeout())
        });
        let Ok((saida, _)) = saida else {
            (self.vigia)("sem_revisor", "");
            return None;
        };
        let trecho = parecer::ler_parecer(&saida, resposta)?;
        (self.vigia)("revisor_sem_fonte", &corta(&trecho, 160));
        Some(trecho)
    }

    /// Vigia uma resposta pronta: (dica para o modelo refazer, trecho que o revisor nao confirmou).
    #[allow(clippy::too_many_arguments)]
    fn vigiar(&self, fim: &str, base: &str, conferir: bool, hist: &[Value], texto: &str, turno: &[Value], local: bool, autor: &str) -> (Option<String>, Option<String>) {
        if let Some(d) = self.defeito(fim, base, conferir) {
            return (Some(d), None);
        }
        match self.revisar(hist, texto, turno, fim, local, autor) {
            Some(t) => (Some(format!("[ATENCAO: um revisor nao achou fonte para este trecho da sua resposta: «{t}». Confirme com uma ferramenta (consultar_banco, pesquisar_web) ou refaca sem esse trecho, dizendo que nao sabe. Nao chute.]")), Some(t)),
            None => (None, None),
        }
    }

    /// Ultimo passo: tira numero sem fonte e, se o trecho que o revisor apontou continua la, avisa a pessoa em vez de esconder.
    fn acabar(&self, fim: &str, base: &str, conferir: bool, suspeito: &Option<String>) -> String {
        let resp = self.consertar(fim, base, conferir);
        match suspeito {
            Some(t) if resp.to_lowercase().contains(&t.to_lowercase()) => {
                (self.vigia)("aviso_sem_confirmacao", &corta(t, 160));
                format!("{resp}\n\n⚠ Não consegui confirmar este trecho em nenhuma fonte: «{t}».")
            }
            _ => resp,
        }
    }

    /// Agente: a IA escolhe ferramentas ate dar a resposta final.
    pub fn responder_agente(&self, conv: &str, texto: &str, tools: &Value, executor: Executor, max_passos: usize) -> Result<(String, String), String> {
        self.responder_agente_com(conv, texto, "", tools, executor, max_passos)
    }

    /// Como `responder_agente`, com o bloco `extra` (ver `responder_com`).
    pub fn responder_agente_com(&self, conv: &str, dito: &str, extra: &str, tools: &Value, executor: Executor, max_passos: usize) -> Result<(String, String), String> {
        let pedido_do_turno = format!("{dito}{extra}");
        let texto = pedido_do_turno.as_str();
        let mut sensivel = self.sensivel(texto);
        let mut local = sensivel;
        let hist = self.historico(conv);
        let nomes: Vec<&str> = tools.as_array().into_iter().flatten().filter_map(|t| t["function"]["name"].as_str()).collect();
        let com_tools = |p: &Value| matches!(p["api"].as_str(), Some("openai" | "ollama" | "cli"));
        let mut turno: Vec<Value> = Vec::new();
        let mut feitas: HashMap<String, String> = HashMap::new();
        let (mut refeita, mut em_texto) = (false, 0);
        let mut suspeito: Option<String> = None;
        let conta = travas::eh_conta(texto);
        for _ in 0..max_passos {
            let cadeia = self.cadeia_para(sensivel).into_iter().filter(com_tools).collect();
            let (m, p) = self.tentar(cadeia, |p, chave| (self.chamar_tools)(p, &self.msgs_para(p, &hist, texto, &turno), chave, self.timeout(), tools))?;
            let (nome, tipo) = (txt(&p["nome"]).to_string(), txt(&p["tipo"]).to_string());
            local |= tipo != "online";
            let mut calls: Vec<Value> = m["tool_calls"].as_array().cloned().unwrap_or_default();
            let mut do_assistente = m.clone();
            if calls.is_empty() {
                let fim = txt(&m["content"]).trim().to_string();
                match texto_ferramenta::chamada_em_texto(&fim, Some(&nomes)) {
                    // a IA escreveu a ferramenta como texto: executa de verdade em vez de mandar o JSON a pessoa
                    Some((n, a)) if em_texto < 2 || p["api"] == "cli" => {
                        em_texto += 1;
                        calls = vec![json!({ "id": "txt", "type": "function", "function": { "name": n, "arguments": a.to_string() } })];
                        do_assistente = json!({ "role": "assistant", "content": null, "tool_calls": calls });
                    }
                    Some(_) => return self.fechar(conv, dito, NAO_CONCLUI, &nome, local),
                    None if texto_ferramenta::resposta_e_json_de_ferramenta(&fim) => return self.fechar(conv, dito, NAO_CONCLUI, &nome, local),
                    None => {
                        let base = self.base(&p, &hist, texto, &turno);
                        let conferir = !feitas.is_empty() || !conta;
                        let dica = if refeita { None } else {
                            let (d, s) = self.vigiar(&fim, &base, conferir, &hist, texto, &turno, local, &nome);
                            suspeito = s;
                            d
                        };
                        if let Some(dica) = dica {
                            // refaz UMA vez, com o defeito dito ao modelo (a dica fica so neste pedido, nao no historico)
                            refeita = true;
                            turno.push(json!({ "role": "assistant", "content": if fim.is_empty() { "(vazio)" } else { &fim } }));
                            turno.push(json!({ "role": "user", "content": dica }));
                            continue;
                        }
                        return self.fechar(conv, dito, &self.acabar(&fim, &base, conferir, &suspeito), &nome, local);
                    }
                }
            }
            turno.push(do_assistente);
            for c in &calls {
                let ferramenta = txt(&c["function"]["name"]);
                let args: Value = serde_json::from_str(txt(&c["function"]["arguments"])).ok().filter(Value::is_object).unwrap_or_else(|| json!({}));
                let id = format!("{ferramenta}{args}");
                // a IA repetia a mesma ferramenta ate esgotar os passos: devolve o guardado e manda responder
                let resultado = match feitas.get(&id) {
                    Some(r) => format!("{}\n(Ja consultado. Responda AGORA com o que voce tem.)", corta(r, 3500)),
                    None => {
                        let (res, usou_local) = executor(ferramenta, &args, &tipo);
                        if usou_local {
                            // o resultado trouxe dado local: dai em diante so IA local, nunca online
                            sensivel = true;
                            local = true;
                        }
                        feitas.insert(id, res.clone());
                        corta(&res, 4000)
                    }
                };
                turno.push(json!({ "role": "tool", "tool_call_id": c["id"], "content": resultado }));
            }
        }
        // os passos acabaram: uma ultima chamada SEM ferramentas, com o que ja foi apurado
        let mut linhas = vec![format!("PESSOA: {}", corta(texto, 1500))];
        linhas.extend(turno.iter().filter(|m| !txt(&m["content"]).is_empty()).map(|m| {
            let quem = match txt(&m["role"]) { "tool" => "RESULTADO DE FERRAMENTA: ", "user" => "PESSOA: ", _ => "VOCE: " };
            format!("{quem}{}", corta(txt(&m["content"]), 1500))
        }));
        let ultimas = linhas[linhas.len().saturating_sub(12)..].join("\n");
        let pedido = format!("{ultimas}\n\nResponda AGORA a ultima mensagem da pessoa, em portugues, curto, usando so o que esta acima. Se faltar algo, diga o que conseguiu e pergunte UMA coisa objetiva.");
        let cadeia = self.cadeia_para(sensivel).into_iter().filter(com_tools).collect();
        let fim = self.tentar(cadeia, |p, chave| {
            (self.chamar)(p, &json!([self.sistema_para(p, texto), { "role": "user", "content": pedido }]), chave, self.timeout())
        });
        let resp = match fim {
            Ok((r, p)) => self.consertar(r.trim(), &self.base(&p, &hist, texto, &turno), !feitas.is_empty() || !conta),
            Err(_) => SEM_RESPOSTA.to_string(),
        };
        self.fechar(conv, dito, &resp, "agente", local)
    }

    /// Pergunta solta a UMA IA pelo nome: so sistema + pergunta (sem banco e sem historico) e nada e guardado.
    pub fn perguntar_a(&self, nome: &str, sistema: &str, pergunta: &str) -> Result<String, String> {
        let p: Vec<Value> = self.cfg["cadeia"].as_array().into_iter().flatten().filter(|p| p["nome"] == nome).cloned().collect();
        let msgs = json!([{ "role": "system", "content": sistema }, { "role": "user", "content": pergunta }]);
        self.tentar(p, |p, chave| (self.chamar)(p, &msgs, chave, self.timeout())).map(|(r, _)| r)
    }

    /// Texto neutro (sintese, resumo): primeira IA pronta, sem banco e sem historico; texto sensivel so vai a IA local.
    pub fn neutro(&self, sistema: &str, texto: &str) -> Result<String, String> {
        self.neutro_em(sistema, texto, false)
    }

    /// `so_local` = o texto pode trazer dado da camada local: nenhuma IA online entra, mesmo sem palavra sensivel.
    pub fn neutro_em(&self, sistema: &str, texto: &str, so_local: bool) -> Result<String, String> {
        let msgs = json!([{ "role": "system", "content": sistema }, { "role": "user", "content": texto }]);
        let fechado = so_local || self.sensivel(texto) || self.sensivel(sistema);
        self.tentar(self.cadeia_para(fechado), |p, chave| (self.chamar)(p, &msgs, chave, self.timeout())).map(|(r, _)| r)
    }

    pub fn status(&self) -> Value {
        self.cfg["cadeia"].as_array().into_iter().flatten().map(|p| {
            let (ok, motivo) = self.disponivel(p);
            json!({ "nome": p["nome"], "tipo": p["tipo"], "modelo": p["modelo"], "ok": ok, "motivo": motivo })
        }).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    type Visto = Arc<Mutex<Vec<String>>>;

    /// Roteador com 3 provedores: a (online, sem chave), b (online), c (local). `visto` guarda "nome|sistema|ultima mensagem".
    /// `roteiro` = respostas do passo com ferramentas, na ordem.
    fn rot(falha_b: bool, roteiro: Vec<Value>) -> (Roteador, Visto) {
        let visto: Visto = Arc::new(Mutex::new(Vec::new()));
        let (v1, v2) = (visto.clone(), visto.clone());
        let fila = Mutex::new(roteiro.into_iter());
        let nota = |v: &Visto, p: &Value, msgs: &Value| {
            let ult = msgs.as_array().and_then(|a| a.last()).map(|m| m["content"].to_string()).unwrap_or_default();
            v.lock().unwrap().push(format!("{}|{}|{ult}|{}", txt(&p["nome"]), txt(&msgs[0]["content"]), msgs.as_array().map_or(0, Vec::len)));
        };
        let r = Roteador {
            cfg: json!({ "system": "S", "sensivel": ["senha"], "cadeia": [
                {"nome": "a", "tipo": "online", "api": "openai", "chave_env": "FALTA"},
                {"nome": "b", "tipo": "online", "api": "openai", "chave_env": "TEM"},
                {"nome": "c", "tipo": "local", "api": "ollama"}]}),
            store: Store::em_memoria().unwrap(),
            banco: Banco::em_memoria().unwrap(),
            chamar: Box::new(move |p, msgs, _, _| {
                // revisor de mentira: so acha invencao no trecho combinado nos testes (e nao entra no `visto`)
                if txt(&msgs[0]["content"]).starts_with("Voce e um REVISOR") {
                    let resp = txt(&msgs[1]["content"]).rsplit("RESPOSTA:\n").next().unwrap_or("");
                    if resp.contains("parecer quebrado") {
                        return Ok("acho que sim {".into());
                    }
                    let achou = resp.contains("Paulo da contabilidade");
                    return Ok(if achou { r#"Parecer: {"inventou": true, "trecho": "Paulo da contabilidade"}"#.into() } else { r#"{"inventou": false}"#.into() });
                }
                nota(&v1, p, msgs);
                if p["nome"] == "b" && falha_b {
                    return Err(ErroProvedor { msg: "429 limite de uso".into(), descanso: 900 });
                }
                Ok(format!("resposta de {}", txt(&p["nome"])))
            }),
            chamar_tools: Box::new(move |p, msgs, _, _, _| {
                nota(&v2, p, msgs);
                fila.lock().unwrap().next().ok_or(ErroProvedor { msg: "roteiro acabou".into(), descanso: 0 })
            }),
            contexto: Box::new(|_, _| String::new()),
            vigia: Box::new(|_, _| {}),
            ler_chave: Box::new(|k| if k == "TEM" { "k".into() } else { String::new() }),
        };
        (r, visto)
    }

    fn chama(nome: &str, args: &str) -> Value {
        json!({ "role": "assistant", "content": null, "tool_calls": [{ "id": "1", "type": "function", "function": { "name": nome, "arguments": args } }] })
    }

    fn diz(t: &str) -> Value {
        json!({ "role": "assistant", "content": t })
    }

    #[test]
    fn pula_sem_chave_e_guarda_a_conversa() {
        let (r, _) = rot(false, vec![]);
        assert_eq!(r.responder("c1", "oi").unwrap(), ("resposta de b".into(), "b".into()));
        assert_eq!(r.store.historico("c1", 10).len(), 2);
        assert_eq!(r.store.uso_hoje("b"), 1);
    }

    #[test]
    fn quem_falha_descansa_e_o_proximo_responde() {
        let (r, _) = rot(true, vec![]);
        assert_eq!(r.responder("c1", "oi").unwrap().1, "c");
        assert!(r.store.cooldown("b") > 800.0);
        assert!(!r.disponivel(&r.cfg["cadeia"][1]).0);
    }

    #[test]
    fn sensivel_e_banco_local_nunca_vao_a_ia_online() {
        let (r, visto) = rot(false, vec![]);
        r.banco.gravar_fato("pessoa", "x", "fato privado da familia", "local", "t", None);
        r.banco.gravar_fato("regra", "y", "fato publico da familia", "publico", "t", None);
        r.responder("c1", "fala da familia").unwrap();
        r.responder("c2", "a senha da familia").unwrap();
        r.responder("c3", "cpf 123.456.789-01").unwrap();
        let v = visto.lock().unwrap();
        assert!(v[0].starts_with("b|") && v[0].contains("fato publico") && !v[0].contains("fato privado"));
        assert!(v[1].starts_with("c|") && v[2].starts_with("c|"));
        assert!(v[1].contains("fato privado"));
    }

    #[test]
    fn turno_que_ficou_no_pc_nao_volta_no_historico_da_ia_online() {
        let (r, visto) = rot(false, vec![]);
        r.responder("c1", "minha senha e abacaxi").unwrap(); // sensivel: so o c viu e respondeu
        r.responder("c1", "bom dia").unwrap(); // normal: vai ao b
        let v = visto.lock().unwrap();
        // sistema + "bom dia": o turno da senha (pergunta e resposta) ficou de fora
        assert!(v[1].starts_with("b|") && v[1].ends_with("|2"), "{}", v[1]);
        assert_eq!(r.store.historico("c1", 10).len(), 4);
    }

    #[test]
    fn todos_falham_vira_erro_com_motivos() {
        let (mut r, _) = rot(false, vec![]);
        r.chamar = Box::new(|_, _, _, _| Err(ErroProvedor { msg: "fora".into(), descanso: 0 }));
        let e = r.responder("c1", "oi").unwrap_err();
        assert!(e.contains("a: sem chave") && e.contains("b: fora") && e.contains("c: fora"));
        assert!(r.store.historico("c1", 10).is_empty());
    }

    #[test]
    fn agente_usa_a_ferramenta_e_responde() {
        let (r, _) = rot(false, vec![chama("soma", "{\"a\":2}"), diz("Deu 44, pronto.")]);
        let pedidos = Mutex::new(Vec::new());
        let exec = |n: &str, a: &Value, tipo: &str| {
            pedidos.lock().unwrap().push(format!("{n}{a}{tipo}"));
            ("o total e 44".to_string(), false)
        };
        assert_eq!(r.responder_agente("c1", "quanto da?", &json!([{"function": {"name": "soma"}}]), &exec, 6).unwrap(), ("Deu 44, pronto.".into(), "b".into()));
        assert_eq!(*pedidos.lock().unwrap(), vec!["soma{\"a\":2}online"]);
    }

    #[test]
    fn dado_local_da_ferramenta_tira_a_ia_online_do_resto_do_turno() {
        let (r, visto) = rot(false, vec![chama("consultar_banco", "{}"), diz("A Maria pediu o video.")]);
        let exec = |_: &str, _: &Value, _: &str| ("Maria pediu o video".to_string(), true);
        assert_eq!(r.responder_agente("c1", "o que a Maria pediu?", &json!([]), &exec, 6).unwrap().1, "c");
        let v = visto.lock().unwrap();
        assert!(v[0].starts_with("b|") && v[1].starts_with("c|") && v[1].contains("Maria pediu o video"));
        assert_eq!(r.store.historico("c1", 10)[1]["provider"], "local/c");
    }

    #[test]
    fn numero_inventado_refaz_uma_vez_e_depois_corta_a_frase() {
        let (r, _) = rot(false, vec![chama("saldo", "{}"), diz("O saldo e 999 reais."), diz("O saldo e 888 reais. Quer mais algo?")]);
        let exec = |_: &str, _: &Value, _: &str| ("saldo: 150 reais".to_string(), false);
        assert_eq!(r.responder_agente("c1", "qual o saldo?", &json!([]), &exec, 6).unwrap().0, format!("Quer mais algo?\n\n{TIREI_NUMERO}"));
        let (r, _) = rot(false, vec![chama("saldo", "{}"), diz("O saldo e 999 reais."), diz("O saldo e 150 reais.")]);
        assert_eq!(r.responder_agente("c1", "qual o saldo?", &json!([]), &exec, 6).unwrap().0, "O saldo e 150 reais.");
    }

    #[test]
    fn numero_sem_fonte_e_barrado_mesmo_sem_ferramenta_mas_conta_pedida_passa() {
        let exec = |_: &str, _: &Value, _: &str| (String::new(), false);
        // sem ferramenta nenhuma: o numero nao esta na conversa nem no banco -> refaz; refez certo -> passa
        let (r, _) = rot(false, vec![diz("A pagina custa 7431 reais."), diz("Nao sei o preco: nao achei em nenhuma fonte.")]);
        assert_eq!(r.responder_agente("c1", "quanto custa a pagina?", &json!([]), &exec, 6).unwrap().0, "Nao sei o preco: nao achei em nenhuma fonte.");
        // insistiu no numero: a frase sai e a pessoa e avisada
        let (r, _) = rot(false, vec![diz("A pagina custa 7431 reais."), diz("Custa 7431 reais. Posso ajudar em algo mais?")]);
        assert_eq!(r.responder_agente("c1", "quanto custa a pagina?", &json!([]), &exec, 6).unwrap().0, format!("Posso ajudar em algo mais?\n\n{TIREI_NUMERO}"));
        // numero que esta no banco e fonte
        let (r, _) = rot(false, vec![diz("A pagina custa 7431 reais.")]);
        r.banco.gravar_fato("regra", "preco", "a pagina custa 7431 reais", "publico", "t", None);
        assert_eq!(r.responder_agente("c1", "quanto custa a pagina?", &json!([]), &exec, 6).unwrap().0, "A pagina custa 7431 reais.");
        // conta pedida pela pessoa: o resultado e numero novo e legitimo
        let (r, _) = rot(false, vec![diz("12 vezes 12 da 144.")]);
        assert_eq!(r.responder_agente("c1", "quanto e 12 vezes 12?", &json!([]), &exec, 6).unwrap().0, "12 vezes 12 da 144.");
    }

    #[test]
    fn revisor_manda_refazer_e_se_o_trecho_continua_a_pessoa_e_avisada() {
        let exec = |_: &str, _: &Value, _: &str| (String::new(), false);
        let inventada = "A reuniao e com o Paulo da contabilidade, ja esta combinado.";
        // o revisor nao achou fonte: o modelo refaz e a resposta honesta passa
        let (r, visto) = rot(false, vec![diz(inventada), diz("Nao sei com quem e a reuniao: nao achei em nenhuma fonte.")]);
        assert_eq!(r.responder_agente("c1", "com quem e a reuniao?", &json!([]), &exec, 6).unwrap().0, "Nao sei com quem e a reuniao: nao achei em nenhuma fonte.");
        assert!(visto.lock().unwrap()[1].contains("um revisor nao achou fonte"), "o modelo recebe o defeito dito");
        // insistiu: a resposta sai com o aviso, sem esconder
        let (r, _) = rot(false, vec![diz(inventada), diz(inventada)]);
        let fim = r.responder_agente("c1", "com quem e a reuniao?", &json!([]), &exec, 6).unwrap().0;
        assert!(fim.starts_with(inventada) && fim.ends_with("⚠ Não consegui confirmar este trecho em nenhuma fonte: «Paulo da contabilidade»."), "{fim}");
        // o nome esta na conversa: o revisor de verdade nao apontaria; aqui a resposta curta sem numero nem passa por ele
        let (r, _) = rot(false, vec![diz("Tudo certo por aqui!")]);
        assert_eq!(r.responder_agente("c1", "tudo bem?", &json!([]), &exec, 6).unwrap().0, "Tudo certo por aqui!");
        // parecer ilegivel: a resposta nao fica presa por causa do revisor
        let longa = "Segue a resposta completa, com parecer quebrado do outro lado.";
        let (r, _) = rot(false, vec![diz(longa)]);
        assert_eq!(r.responder_agente("c1", "e ai?", &json!([]), &exec, 6).unwrap().0, longa);
    }

    #[test]
    fn ferramenta_escrita_como_texto_e_executada_e_json_nunca_vira_resposta() {
        let tools = json!([{"function": {"name": "soma"}}]);
        let (r, _) = rot(false, vec![diz("{\"tool\": \"soma\", \"arguments\": {\"a\": 1}}"), diz("Pronto, somei.")]);
        let n = Mutex::new(0);
        let exec = |_: &str, _: &Value, _: &str| {
            *n.lock().unwrap() += 1;
            ("ok".to_string(), false)
        };
        assert_eq!(r.responder_agente("c1", "soma ai", &tools, &exec, 6).unwrap().0, "Pronto, somei.");
        assert_eq!(*n.lock().unwrap(), 1);
        let (r, _) = rot(false, vec![diz("{\"name\": \"outra\", \"arguments\": {}}")]);
        assert_eq!(r.responder_agente("c1", "faz", &tools, &exec, 6).unwrap().0, NAO_CONCLUI);
    }

    #[test]
    fn ferramenta_repetida_nao_roda_duas_vezes_e_passos_esgotados_fecham_sem_ferramenta() {
        let (r, visto) = rot(false, vec![chama("x", "{}"), chama("x", "{}"), chama("x", "{}")]);
        let n = Mutex::new(0);
        let exec = |_: &str, _: &Value, _: &str| {
            *n.lock().unwrap() += 1;
            ("achei".to_string(), false)
        };
        assert_eq!(r.responder_agente("c1", "procura", &json!([]), &exec, 3).unwrap(), ("resposta de b".into(), "agente".into()));
        assert_eq!(*n.lock().unwrap(), 1);
        assert!(visto.lock().unwrap().last().unwrap().contains("RESULTADO DE FERRAMENTA: achei"));
    }

    #[test]
    fn extra_vai_a_ia_mas_nao_ao_historico_e_neutro_local_nao_usa_online() {
        let (r, visto) = rot(false, vec![diz("certo, combinado então")]);
        assert_eq!(r.responder_com("c1", "oi", "

[FICHA]").unwrap().1, "b");
        r.responder_agente_com("c1", "tudo bem?", "

[FICHA 2]", &json!([]), &|_, _, _| (String::new(), false), 3).unwrap();
        let v = visto.lock().unwrap().clone();
        assert!(v[0].contains("[FICHA]") && v[1].contains("[FICHA 2]"), "{v:?}");
        let h = r.store.historico("c1", 10);
        assert!(h.len() == 4 && h.iter().all(|m| !txt(&m["content"]).contains("FICHA")) && h[2]["content"] == "tudo bem?", "{h:?}");
        // texto sem palavra sensivel, mas marcado como local: so a IA local responde
        assert_eq!(r.neutro_em("S", "resuma isto", true).unwrap(), "resposta de c");
        assert_eq!(r.neutro_em("S", "resuma isto", false).unwrap(), "resposta de b");
        assert_eq!(r.neutro_em("a senha e x", "resuma isto", false).unwrap(), "resposta de c");
    }
}
