//! Base de conhecimento do Airbnb: politicas da plataforma + manual de atendimento, para o dossie e o banco unificado
//! (escopo "airbnb"). Porte de braniac/airbnb_conhecimento.py com duas diferencas de proposito:
//! 1) nomes de pessoas e de anuncios sairam do texto (viraram "o dono", "os responsaveis", "anuncio de quarto");
//! 2) o bloco RESERVAS do Python (07/10) dizia que o sistema PODE pre-aprovar/aceitar dentro do padrao. Essa regra foi
//!    revogada (26/10): aqui o texto diz que NENHUMA reserva e aceita ou pre-aprovada sem o OK de uma pessoa.
//! O dossie (contexto.json) nao existe no Rust ainda: `gravar` recebe a closure que registra la.
#![allow(dead_code)]
use crate::banco::Banco;

pub const POLITICAS: &str = "POLITICAS DO AIRBNB (curadas em out/2026). \
CONSULTA de reserva = pergunta antes de reservar; responda em ate 24 h (taxa de resposta); a pre-aprovacao nao bloqueia calendario e vale 24 h. \
PEDIDO de reserva = o hospede ja pediu; datas ficam bloqueadas; 24 h para aceitar ou recusar; deixar expirar derruba taxa de resposta/Superhost, nunca ignore: avise os responsaveis. \
CANCELAMENTO: o hospede cancela pelo proprio Airbnb e o reembolso segue a politica DA RESERVA (flexivel: integral ate 24 h antes; moderada: ate 5 dias); nunca diga 'sem direito a reembolso' sem checar, nunca prometa reembolso/excecao, nunca cancele reserva confirmada nem peca ao hospede que cancele. \
REGRA DE OURO: tudo dentro do Airbnb ate a reserva confirmada: nao peca nem passe WhatsApp, telefone, e-mail, Pix, link de pagamento, cartao, CVV, senha. Cobranca extra/diferenca so por 'Alterar reserva' ou Central de Resolucoes, nunca por fora. \
PROIBIDO: discriminar (raca, religiao, nacionalidade, genero, orientacao, deficiencia, idade); trocar avaliacao por desconto/dinheiro ou pedir nota (pedido de avaliacao e neutro, sem '5 estrelas'); inventar comodidade; esconder limitacao do imovel; cobrar taxa nao anunciada. \
DANO: nao cobrar informalmente; fotos/videos, orcamento/recibo, pedido pela Central de Resolucoes em ate 14 dias apos o checkout; AirCover para anfitrioes cobre danos e responsabilidade civil. Avise o dono. \
ALTERACAO de reserva (datas, pessoas, preco) so pelo fluxo 'Alterar reserva'; o Airbnb calcula a diferenca. Hospede extra: confira a capacidade do anuncio e peca a alteracao, nao aceite 'de graca'. \
Duvida sobre politica: nao invente, diga que vai confirmar e consulte o dono.";

pub const ATENDIMENTO: &str = "MANUAL DE ATENDIMENTO AIRBNB. Tom: humano, caloroso, direto, pelo primeiro nome; sem 'Prezado hospede' nem 'estamos a disposicao'; emoji raro (no maximo 1). \
Estrutura: acolher -> responder exatamente o que perguntou (+ a proxima duvida provavel) -> proximo passo. Respostas ate ~400 caracteres. Rapidez: consulta em minutos; problema na estadia (chave, agua, luz, acesso, seguranca) tem prioridade maxima. \
Nao sabe? 'Nao quero te passar informacao errada, vou confirmar e ja te retorno' e CONSULTE os responsaveis; nunca invente. Nunca prometa o que nao esta confirmado (check-in cedo, late checkout). \
Desconto: nao ofereca nem negue por conta propria (e decisao de dinheiro do dono); diga que vai verificar; se for aprovado vira oferta especial pelo proprio Airbnb. \
Check-in cedo / late checkout: 'vou verificar a programacao de limpeza e te aviso' e consulte; se houver cobranca extra, so pelo Airbnb. \
Reclamacao: ouvir -> pedir foto se preciso -> resolver -> registrar; peca desculpa uma vez ('sinto muito pelo transtorno, vou verificar agora'); nunca culpe o hospede ('voce nao leu o anuncio'). \
Barulho: lembrar com educacao as regras da casa e o horario de silencio; reincidencia: avisar o dono. Cancelamento: orientar a cancelar pelo app (reembolso conforme a politica da reserva). \
Pos-estadia: agradecer e pedir avaliacao de forma NEUTRA (sem pedir nota nem prometer retribuicao). \
QUARTO em casa: sempre deixar claro que e um quarto PRIVATIVO, areas comuns compartilhadas com outros hospedes e moradores, SEM cafe da manha; nunca sugerir casa inteira. \
Antes de responder pergunte-se: 'o hospede fica sabendo exatamente o que vai acontecer?'";

pub const FIRSTCONTACT: &str = "PRIMEIRO CONTATO EM QUARTO: quando alguem entra em contato num anuncio de QUARTO e ainda nao recebeu nenhuma mensagem nossa, \
vai PRIMEIRO a resposta rapida salva no Airbnb chamada 'firstcontact' (explica que e um quarto privativo numa casa com areas compartilhadas) e so depois a resposta a duvida. \
NAO vale para apartamento, casa inteira nem studio. Nao repita o conteudo da firstcontact na resposta seguinte.";

pub const RESERVAS: &str = "RESERVAS: o sistema NUNCA aceita pedido nem pre-aprova consulta de reserva, nem 'dentro do padrao'. Toda reserva vai para os responsaveis decidirem \
(quem responder primeiro 'sim'/'nao' decide) e sao ELES que aceitam, pre-aprovam ou recusam no Airbnb. Sinal de festa/evento/som/pet/visitas, muitos hospedes ou muitas noites entram no aviso como motivo de atencao. \
Valor/preco/desconto: so o dono. Nenhum anuncio e alterado pelo sistema.";

/// chave do dossie -> (texto, gatilhos que fazem o texto entrar no contexto).
pub fn entradas() -> Vec<(&'static str, &'static str, Vec<&'static str>)> {
    vec![
        ("airbnb_politicas", POLITICAS, vec!["airbnb", "politica", "política", "cancelar", "cancelamento", "reembolso", "estorno", "pix", "pagamento", "cartao", "cartão", "avaliacao", "avaliação", "dano", "quebrou", "aircover", "discrimin", "alterar reserva", "taxa"]),
        ("airbnb_atendimento", ATENDIMENTO, vec!["airbnb", "check-in", "checkin", "checkout", "check-out", "cedo", "mais tarde", "barulho", "desconto", "reclama", "wifi", "wi-fi", "cafe", "café", "chegar", "chave", "pessoas extras", "disponibilidade", "vaga"]),
        ("airbnb_firstcontact", FIRSTCONTACT, vec!["airbnb", "firstcontact", "primeiro contato", "quarto", "suite", "suíte"]),
        ("airbnb_reservas", RESERVAS, vec!["airbnb", "reserva", "pedido de reserva", "consulta de reserva", "pre-aprovar", "pré-aprovar", "aceitar", "recusar", "convite"]),
    ]
}

/// Textos cujo gatilho aparece na frase (sem repetir), na ordem de `entradas`.
pub fn para(texto: &str) -> Vec<&'static str> {
    let t = texto.to_lowercase();
    entradas().into_iter().filter(|(_, _, g)| g.iter().any(|x| t.contains(x))).map(|e| e.1).collect()
}

/// Grava a base no banco (nivel publico: e politica da plataforma, sem dado pessoal) e entrega cada entrada ao dossie pela closure
/// `dossie(chave, texto, gatilhos)`. Devolve as chaves gravadas.
pub fn gravar(banco: &Banco, dossie: &dyn Fn(&str, &str, &[&str])) -> Vec<&'static str> {
    entradas().into_iter().map(|(chave, texto, gatilhos)| {
        dossie(chave, texto, &gatilhos);
        banco.gravar_fato("airbnb", chave, texto, "publico", "curadoria", None);
        chave
    }).collect()
}

/// Respostas rapidas lidas do Airbnb (podem ter endereco e senha do Wi-Fi: camada LOCAL, nunca vai a IA online). Devolve quantas gravou.
pub fn gravar_modelos(banco: &Banco, modelos: &[(&str, &str)]) -> usize {
    modelos.iter().filter(|(titulo, corpo)| banco.gravar_fato("airbnb_modelos", titulo, corpo, "local", "airbnb-respostas-rapidas", None)).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn a_base_nunca_autoriza_aceitar_reserva() {
        let tudo = entradas().iter().map(|e| e.1.to_lowercase()).collect::<Vec<_>>().join(" ");
        assert!(RESERVAS.contains("NUNCA aceita pedido nem pre-aprova") && !tudo.contains("pode pre-aprovar") && !tudo.contains("pode aceitar"));
        assert!(POLITICAS.contains("nao peca nem passe WhatsApp") && ATENDIMENTO.contains("nunca invente"));
    }

    #[test]
    fn gatilho_escolhe_o_texto_e_gravar_separa_publico_de_local() {
        assert_eq!(para("o hóspede quer REEMBOLSO"), vec![POLITICAS]);
        assert_eq!(para("chegou um pedido de reserva no airbnb").len(), 4);
        assert!(para("bom dia").is_empty());
        let b = Banco::em_memoria().unwrap();
        let visto = RefCell::new(Vec::new());
        let chaves = gravar(&b, &|k, t, g| visto.borrow_mut().push((k.to_string(), t.len(), g.len())));
        assert_eq!(chaves, vec!["airbnb_politicas", "airbnb_atendimento", "airbnb_firstcontact", "airbnb_reservas"]);
        assert!(visto.borrow().len() == 4 && visto.borrow().iter().all(|v| v.1 > 100 && v.2 > 3));
        assert!(b.consultar("reembolso cancelamento", "online", None, 5).0.contains("POLITICAS DO AIRBNB")); // politica e publica
        assert_eq!(gravar_modelos(&b, &[("chegada", "A rede se chama Exemplo e a chave fica na portaria"), ("vazio", "  ")]), 1);
        assert!(!b.consultar("rede exemplo portaria", "online", None, 5).0.contains("portaria")); // modelo e local: IA online nao ve
        assert!(b.consultar("rede exemplo portaria", "local", None, 5).0.contains("portaria"));
    }
}
