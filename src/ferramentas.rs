//! Ferramentas que o Jarvis pode usar. Cada modulo portado ganha a sua aqui (o Braniac atual tem ~55).
//! Trava: o que AGE (liga, abre, fecha, grava) so roda quando o pedido veio do proprio PC.
use crate::executor::{self, Ambiente};
use crate::jarvis_travas::Pedido;
use crate::roteador::Roteador;
use crate::{abas, agenda, agentes, barramento, compasso, fatos, conselho, jarvis, modulos, skills, tempo, vm};
use serde_json::{json, Value};
use std::path::Path;

fn fn_(nome: &str, desc: &str, props: Value, obrig: &[&str]) -> Value {
    json!({ "type": "function", "function": { "name": nome, "description": desc,
            "parameters": { "type": "object", "properties": props, "required": obrig } } })
}

pub fn lista() -> Value {
    json!([
        fn_("consultar_banco", "Consulta o BANCO do Jarvis (fonte unica da verdade: pessoas, regras, negocio, mensagens, decisoes). Use antes de afirmar qualquer fato.",
            json!({ "busca": { "type": "string", "description": "assunto ou nome" }, "horas": { "type": "number", "description": "olhar so as ultimas N horas (opcional)" } }), &["busca"]),
        fn_("lembrar", "Guarda um FATO que a pessoa acabou de ensinar, para nunca mais perguntar.",
            json!({ "assunto": { "type": "string" }, "fato": { "type": "string" } }), &["assunto", "fato"]),
        fn_("listar_agentes", "AGENTES DO BRANIAC: um por assunto (nome, assunto e o que cuida) e, com recados=true, os ultimos recados que eles trocaram no barramento. Use quando a pessoa perguntar quais sao os agentes, quem cuida de um assunto ou o que os agentes combinaram.",
            json!({ "recados": { "type": "boolean" } }), &[]),
        fn_("status_braniac", "Mostra quais IAs estao prontas, quais modulos estao ligados e se a VM esta ligada.", json!({}), &[]),
        fn_("conselho_de_ias", "Convoca o CONSELHO DE IAs: varias IAs opinam sobre um tema, cada uma com um papel, e sai uma sintese. Use quando pedirem o conselho ou para decisao que merece mais de uma opiniao. So o tema vai para as IAs: nunca ponha nome de pessoa nem dado pessoal.",
            json!({ "tema": { "type": "string", "description": "o assunto completo, em uma ou duas frases" } }), &["tema"]),
        fn_("pesquisar_web", "PESQUISA na internet (na aba do agente Pesquisa, dentro da VM) e devolve o texto dos resultados. Use para fato atual ou que voce nao sabe. Nunca ponha dado pessoal na consulta.",
            json!({ "consulta": { "type": "string" } }), &["consulta"]),
        fn_("vm_navegador", "VM do Braniac (Linux com navegador; cada agente tem a SUA aba). acao: 'estado', 'ligar', 'desligar', 'abas' (lista), 'navegar' (o agente vai a uma url http/https na aba dele e devolve o texto da pagina), 'ler' (texto que a aba do agente mostra agora), 'elementos' (lista numerada do que da para clicar ou digitar na aba do agente), 'clicar' (agente e n da lista), 'digitar' (agente, n, texto; enter=true aperta Enter no fim), 'mostrar' (traz a aba do id para a frente) ou 'fechar' (precisa de id). Antes de clicar ou digitar, peca 'elementos'. Pagar, comprar, entrar em conta e digitar segredo sao recusados: isso e sempre a pessoa.",
            json!({ "acao": { "type": "string", "enum": ["estado", "ligar", "desligar", "abas", "navegar", "ler", "elementos", "clicar", "digitar", "mostrar", "fechar"] },
                    "n": { "type": "integer", "description": "numero do elemento na lista" }, "texto": { "type": "string" }, "enter": { "type": "boolean" },
                    "agente": { "type": "string" }, "url": { "type": "string" }, "id": { "type": "string" } }), &["acao"]),
        fn_("agenda", "AGENDA/ROTINA DA PESSOA (local, no PC): 'adicionar' compromisso ou tarefa (quando = 'amanha 14h', '15/10 9:30', 'segunda 10h'; vazio = tarefa sem data; repetir = diario/dias_uteis/semanal/mensal para rotina), 'listar' (dia e dias), 'concluir' e 'remover' (alvo = id entre colchetes ou parte do titulo), 'livres' (horarios vagos) e 'organizar' (marcado + livres + tarefas sem data, para montar a rotina do dia). O Braniac lembra a pessoa antes de cada horario. Use SEMPRE que ela falar de compromisso, tarefa, lembrete, rotina ou do que tem pra fazer. So diga que anotou se o resultado confirmar.",
            json!({ "acao": { "type": "string", "enum": ["adicionar", "listar", "concluir", "remover", "livres", "organizar"] },
                    "titulo": { "type": "string" }, "quando": { "type": "string", "description": "Data/hora em portugues ou AAAA-MM-DD HH:MM" },
                    "duracao": { "type": "number", "description": "Minutos (padrao 60)" }, "repetir": { "type": "string", "enum": ["", "diario", "dias_uteis", "semanal", "mensal"] },
                    "lembrar": { "type": "number", "description": "Minutos antes para lembrar (padrao 15; 0 = sem lembrete)" }, "notas": { "type": "string" },
                    "ate": { "type": "string", "description": "Ultimo dia de uma repeticao (opcional)" }, "dia": { "type": "string", "description": "Dia para listar/livres/organizar/concluir (padrao hoje)" },
                    "dias": { "type": "number", "description": "Quantos dias listar a partir de 'dia' (padrao 1; 7 = semana)" }, "alvo": { "type": "string" } }), &["acao"]),
        fn_("compasso_rotinas", "ROTINAS GUARDADAS PELO COMPASSO, o agente de rotina (ele aciona o Jarvis na hora certa: perguntar a um familiar, lembrar a pessoa). acao 'listar' mostra todas; 'ativar'/'pausar' (alvo = id da rotina) SO quando a pessoa pedir.",
            json!({ "acao": { "type": "string", "enum": ["listar", "ativar", "pausar"] }, "alvo": { "type": "string", "description": "id da rotina" } }), &["acao"]),
    ])
}

/// Ferramentas oferecidas NESTA mensagem: o nucleo e os grupos do assunto (`jarvis::ferramentas_para`; `falas` = as 2 ultimas
/// falas da conversa). As que ja moravam em `lista` ficam com a descricao daqui, que e a que vale neste Braniac; as que so
/// existem aqui entram sempre.
pub fn lista_para(amb: &Ambiente, texto: &str, falas: &[String]) -> Value {
    let nome = |f: &Value| f["function"]["name"].as_str().unwrap_or("").to_string();
    let daqui = lista().as_array().cloned().unwrap_or_default();
    let todas = jarvis::ferramentas(&jarvis::Catalogos { skills: skills::catalogo(&amb.locais.skills, 80), ..Default::default() });
    let mut l: Vec<Value> = jarvis::ferramentas_para(&todas, texto, falas, &[]).into_iter().map(|mut f| {
        if let Some(m) = daqui.iter().find(|m| nome(m) == nome(&f)) {
            return m.clone();
        }
        // catalogo que ainda nao existe aqui deixa o enum vazio, e ha provedor que recusa a lista inteira por isso
        if let Some(acao) = f["function"]["parameters"]["properties"]["acao"].as_object_mut().filter(|a| a.get("enum") == Some(&json!([]))) {
            acao.remove("enum");
        }
        f
    }).collect();
    l.extend(daqui.iter().filter(|m| !todas.iter().any(|f| nome(f) == nome(m))).cloned());
    Value::Array(l)
}

/// O trabalho das empresas roda IA e processo: fica para outra rodada. Abrir, ver o quadro e aprovar seguem pelo executor.
const EMPRESAS: [&str; 5] = ["nexodev", "marketing", "investimentos", "compras", "cortes_video"];

/// Executa qualquer ferramenta oferecida por `lista_para`: as novas pelo `executor` (travas primeiro; envio so na caixa de saida),
/// as antigas por `executar`. Quem chama esvazia `amb.saida_direta` depois de cada chamada.
pub fn executar_tudo(rot: &Roteador, amb: &Ambiente, p: &Pedido, nome: &str, args: &Value, tipo: &str) -> (String, bool) {
    if EMPRESAS.contains(&nome) && matches!(args["acao"].as_str(), Some("trabalhar" | "planejar")) {
        return ("O trabalho das empresas (IA e processos) ainda não foi ligado no Braniac em Rust: nada foi feito. Por aqui dá para abrir pedido, ver o quadro e aprovar.".into(), false);
    }
    executor::executar(amb, p, nome, args).unwrap_or_else(|| executar(rot, &amb.dados, p.origem, nome, args, tipo))
}

/// Mensagem de recusa quando a acao nao pode vir daquela origem; None = liberado.
pub fn trava(origem: &str, o_que: &str) -> Option<String> {
    (origem != "pc").then(|| format!("Por seguranca, de fora do computador eu so converso e consulto. Para {o_que} o pedido precisa vir do proprio PC."))
}

fn texto(r: Result<Value, String>) -> String {
    r.map(|v| v.to_string()).unwrap_or_else(|e| format!("ERRO na ferramenta: {e}"))
}

/// Devolve (resultado para a IA, trouxe dado da camada local). `tipo` = tipo do provedor que pediu:
/// IA online consultando o banco so recebe a camada publica.
pub fn executar(rot: &Roteador, dados: &Path, origem: &str, nome: &str, args: &Value, _tipo: &str) -> (String, bool) {
    let arg = |k: &str| args[k].as_str().unwrap_or("").trim().to_string();
    match nome {
        // Sempre a camada completa: se vier dado local, o `true` faz o roteador tirar as IAs online do resto do
        // turno (o resultado so entra no proximo pedido, que ja vai a IA local). Consultar so a camada de quem
        // pediu fazia a IA online responder "nao ha registro" de fato que existe.
        "consultar_banco" => rot.banco.consultar(&arg("busca"), "local", args["horas"].as_f64(), 8),
        "lembrar" => {
            if let Some(t) = trava(origem, "guardar um fato") {
                return (t, false);
            }
            // fato ensinado em conversa e pessoal ate prova em contrario: camada local
            // "so hoje" vence sozinho; o fato tambem vai para o banco do assunto dele
            let val = fatos::validade_do_texto(&arg("fato"), &tempo::agora(), fatos::agora_ts());
            let ok = rot.banco.gravar_fato("aprendido", &arg("assunto"), &arg("fato"), "local", "jarvis", val);
            if ok {
                fatos::espelhar(dados, "aprendido", &arg("assunto"), &arg("fato"), "local", "jarvis", val);
            }
            (if ok { "Guardado.".into() } else { "Nada guardado: faltou o fato.".into() }, false)
        }
        "agenda" => (agenda::ferramenta(&dados.join("agenda.json"), args, &tempo::agora()), false),
        "compasso_rotinas" => {
            if let Some(t) = Some(arg("acao")).filter(|a| a == "ativar" || a == "pausar").and_then(|_| trava(origem, "ligar ou pausar rotinas do Compasso")) {
                return (t, false);
            }
            (compasso::ferramenta(dados, &arg("acao"), &arg("alvo")), false)
        }
        "conselho_de_ias" => (conselho::convocar(rot, &arg("tema")).map(|v| v["relatorio"].as_str().unwrap_or("").to_string()).unwrap_or_else(|e| e), false),
        "pesquisar_web" => {
            // a consulta vira endereco num site de fora: texto sensivel nao sai do PC
            if rot.sensivel(&arg("consulta")) {
                return ("Nao pesquisei: a consulta tem dado sensivel, e isso nao pode sair do computador.".into(), false);
            }
            (abas::pesquisar(dados, &arg("consulta")).map(|l| abas::para_ia(&l)).unwrap_or_else(|e| format!("ERRO na ferramenta: {e}")), false)
        }
        "listar_agentes" if args["recados"] == true => {
            let l: Vec<String> = barramento::historico(dados, 15).iter().map(|m| format!("{} → {} ({}): {}", m["de"].as_str().unwrap_or(""), m["para"].as_str().unwrap_or(""), m["tipo"].as_str().unwrap_or(""), m["texto"].as_str().unwrap_or(""))).collect();
            (format!("{}\n\nÚltimos recados:\n{}", agentes::resumo(dados), if l.is_empty() { "(nenhum)".into() } else { l.join("\n") }), false)
        }
        "listar_agentes" => (agentes::resumo(dados), false),
        "status_braniac" => {
            let ias = rot.status();
            let prontas: Vec<&str> = ias.as_array().into_iter().flatten().filter(|i| i["ok"] == true).filter_map(|i| i["nome"].as_str()).collect();
            (json!({ "ias_prontas": prontas, "ias_total": ias.as_array().map_or(0, Vec::len),
                     "modulos_ligados": modulos::ativos(dados), "vm": vm::estado().unwrap_or_else(|e| json!({ "erro": e })) }).to_string(), false)
        }
        "vm_navegador" => {
            let acao = arg("acao");
            if matches!(acao.as_str(), "ligar" | "desligar" | "navegar" | "clicar" | "digitar" | "mostrar" | "fechar") {
                if let Some(t) = trava(origem, "mexer na VM") {
                    return (t, false);
                }
            }
            if crate::whatsapp::reservado(&arg("agente")) {
                return ("ERRO na ferramenta: a aba do WhatsApp e reservada; nenhuma IA le nem mexe nela".into(), false);
            }
            let n = args["n"].as_u64().unwrap_or(0) as u32;
            let r = match acao.as_str() {
                "estado" => vm::estado(),
                "ligar" => vm::ligar(),
                "desligar" => vm::desligar(),
                "abas" => vm::abas(dados),
                // aba pode estar numa conta logada da pessoa: o que se le dela nunca vai a IA online
                "navegar" => return (abas::navegar(dados, &arg("agente"), &arg("url")).map(|l| abas::para_ia(&l)).unwrap_or_else(|e| format!("ERRO na ferramenta: {e}")), true),
                "ler" => return (abas::ler(dados, &arg("agente")).map(|l| abas::para_ia(&l)).unwrap_or_else(|e| format!("ERRO na ferramenta: {e}")), true),
                "elementos" => return (abas::elementos(dados, &arg("agente")).map(|l| l.to_string()).unwrap_or_else(|e| format!("ERRO na ferramenta: {e}")), true),
                "clicar" => return (abas::clicar(dados, &arg("agente"), n).map(|l| abas::para_ia(&l)).unwrap_or_else(|e| format!("ERRO na ferramenta: {e}")), true),
                // o texto vai para dentro de um site: segredo e documento nunca sao digitados pelo agente
                "digitar" if rot.sensivel(&arg("texto")) => Err("nao digitei: o texto tem dado sensivel, e isso e sempre a pessoa".into()),
                "digitar" => return (abas::digitar(dados, &arg("agente"), n, &arg("texto"), args["enter"] == true).map(|l| abas::para_ia(&l)).unwrap_or_else(|e| format!("ERRO na ferramenta: {e}")), true),
                "mostrar" => vm::mostrar_aba(&arg("id")),
                "fechar" => vm::fechar_aba(dados, &arg("id")),
                outra => Err(format!("acao desconhecida: {outra}")),
            };
            (texto(r), false)
        }
        outro => (format!("ERRO na ferramenta: '{outro}' nao existe"), false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{banco::Banco, store::Store};

    fn rot() -> Roteador {
        Roteador {
            cfg: json!({ "cadeia": [] }),
            store: Store::em_memoria().unwrap(),
            banco: Banco::em_memoria().unwrap(),
            chamar: Box::new(|_, _, _, _| Ok(String::new())),
            chamar_tools: Box::new(|_, _, _, _, _| Ok(json!({}))),
            contexto: Box::new(|_, _| String::new()),
            vigia: Box::new(|_, _| {}),
            ler_chave: Box::new(|_| String::new()),
        }
    }

    #[test]
    fn toda_ferramenta_da_lista_existe_e_tem_nome_unico() {
        let (r, d) = (rot(), std::env::temp_dir());
        let nomes: Vec<String> = lista().as_array().unwrap().iter().map(|t| t["function"]["name"].as_str().unwrap().to_string()).collect();
        for n in &nomes {
            // origem de fora: nada que age roda de verdade neste teste
            assert!(!executar(&r, &d, "telegram", n, &json!({"acao": "fechar"}), "online").0.contains("nao existe"), "{n}");
            assert_eq!(nomes.iter().filter(|x| *x == n).count(), 1);
        }
        assert!(executar(&r, &d, "pc", "inventada", &json!({}), "local").0.starts_with("ERRO na ferramenta"));
    }

    #[test]
    fn de_fora_do_pc_nao_age_e_ia_online_nao_ve_o_local() {
        let (r, d) = (rot(), std::env::temp_dir());
        for acao in ["ligar", "desligar", "navegar", "clicar", "digitar", "mostrar", "fechar"] {
            assert!(executar(&r, &d, "telegram", "vm_navegador", &json!({ "acao": acao }), "local").0.starts_with("Por seguranca"));
        }
        assert!(executar(&r, &d, "whatsapp", "lembrar", &json!({"assunto": "a", "fato": "b"}), "local").0.starts_with("Por seguranca"));
        let mut r = r;
        r.cfg = json!({ "cadeia": [], "sensivel": ["senha"] });
        assert!(executar(&r, &d, "pc", "pesquisar_web", &json!({"consulta": "minha senha 1234"}), "online").0.starts_with("Nao pesquisei"));
        assert!(executar(&r, &d, "pc", "vm_navegador", &json!({"acao": "digitar", "agente": "x", "n": 1, "texto": "a senha e abc"}), "local").0.contains("dado sensivel"));
        assert_eq!(executar(&r, &d, "pc", "lembrar", &json!({"assunto": "gato", "fato": "o gato se chama Mingau"}), "local").0, "Guardado.");
        // fato local: quem pediu foi IA online, mas a marca `true` tira as online do resto do turno
        let (t, local) = executar(&r, &d, "pc", "consultar_banco", &json!({"busca": "gato"}), "online");
        assert!(t.contains("Mingau") && local);
        // so fato publico: nada de local, a IA online pode seguir
        r.banco.gravar_fato("regra", "p", "o preco da pagina e 150", "publico", "t", None);
        let (t, local) = executar(&r, &d, "pc", "consultar_banco", &json!({"busca": "preco pagina"}), "online");
        assert!(t.contains("150") && !local);
    }

    #[test]
    fn lista_por_assunto_e_execucao_das_novas_pela_caixa_de_saida() {
        let d = std::env::temp_dir().join(format!("braniac-ferramentas-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let (r, amb) = (rot(), Ambiente { modulo_ativo: Box::new(|_| true), ..Ambiente::seguro(&d) });
        let nomes = |t: &str| -> Vec<String> { lista_para(&amb, t, &[]).as_array().unwrap().iter().map(|f| f["function"]["name"].as_str().unwrap().to_string()).collect() };
        let (simples, reserva) = (nomes("bom dia"), nomes("aceita a reserva do hóspede João"));
        assert!(simples.contains(&"consultar_banco".to_string()) && !simples.contains(&"airbnb_reserva".to_string()) && reserva.contains(&"airbnb_reserva".to_string()));
        // toda ferramenta daqui que o Python nao conhece continua oferecida; nenhuma repetida; nenhum enum vazio
        let todas = jarvis::ferramentas(&jarvis::Catalogos::default());
        for f in lista().as_array().unwrap().iter().filter(|f| !todas.iter().any(|t| t["function"]["name"] == f["function"]["name"])) {
            assert!(simples.contains(&f["function"]["name"].as_str().unwrap().to_string()));
        }
        assert!(simples.iter().all(|n| simples.iter().filter(|x| *x == n).count() == 1));
        assert!(!lista_para(&amb, "edita a foto e vê o ipca acumulado na lei", &[]).to_string().contains("\"enum\":[]"));
        // a descricao que vale e a daqui
        let banco = lista_para(&amb, "oi", &[]).as_array().unwrap().iter().find(|f| f["function"]["name"] == "consultar_banco").cloned().unwrap();
        assert!(lista().as_array().unwrap().contains(&banco));
        let ctx = crate::contexto::Ctx::default();
        let p = Pedido { origem: "pc", conv: "jarvis", original: "x", armada: false, ctx: &ctx, familia: &[] };
        let (t, _) = executar_tudo(&r, &amb, &p, "whatsapp_enviar", &json!({ "nome": "Ana", "texto": "cheguei" }), "online");
        assert!(t.starts_with('✅') && crate::saida::ultimas(&d, 5)[0]["canal"] == "pedido", "{t}");
        // leitura privada: a IA so recebe a nota; o conteudo fica em saida_direta para quem chamou
        assert!(executar_tudo(&r, &amb, &p, "resumo_whatsapp", &json!({}), "online").1 && amb.saida_direta.borrow().len() == 1);
        assert!(executar_tudo(&r, &amb, &p, "agenda", &json!({ "acao": "listar" }), "local").0.len() > 3);
        for n in EMPRESAS {
            assert!(executar_tudo(&r, &amb, &p, n, &json!({ "acao": "trabalhar" }), "local").0.contains("ainda não foi ligado"), "{n}");
        }
        assert!(!executar_tudo(&r, &amb, &p, "nexodev", &json!({ "acao": "quadro" }), "local").0.contains("ainda não foi ligado"));
        assert!(executar_tudo(&r, &amb, &p, "inventada", &json!({}), "local").0.starts_with("ERRO na ferramenta"));
        // de fora do PC a trava do executor continua valendo
        let fora = Pedido { origem: "telegram", ..p };
        assert_eq!(executar_tudo(&r, &amb, &fora, "whatsapp_enviar", &json!({ "nome": "Ana", "texto": "oi" }), "online").0, crate::jarvis_travas::BLOQUEADO);
        assert_eq!(crate::saida::ultimas(&d, 5).as_array().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }
}
