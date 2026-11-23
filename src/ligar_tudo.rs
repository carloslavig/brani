//! Liga TUDO do Braniac (ligar_tudo.py): navegador do Braniac com as abas, o servidor e a interface. Aqui so sai a LISTA DE PASSOS,
//! na ordem, a partir do que ja esta de pe: nada e executado nem aberto (quem chama roda cada passo e confere de novo).
//! Pode ser pedido varias vezes: so entra o que ainda nao subiu. Um servidor so: se a porta ja responde, nao sobe outro.
//! Diferenca: o endereco de cada aba vem de configuracao (`abas`); o padrao aqui e a pagina inicial de cada servico.
#![allow(dead_code)]

/// (dominio que identifica a aba, endereco a abrir).
pub fn abas_padrao() -> Vec<(String, String)> {
    [("web.whatsapp.com", "https://web.whatsapp.com/"), ("chatgpt.com", "https://chatgpt.com/"), ("gemini.google.com", "https://gemini.google.com/app"), ("grok.com", "https://grok.com/"), ("chat.deepseek.com", "https://chat.deepseek.com/"), ("kimi.com", "https://www.kimi.com/"), ("meta.ai", "https://www.meta.ai/")]
        .iter()
        .map(|(d, u)| (d.to_string(), u.to_string()))
        .collect()
}

#[derive(Debug, PartialEq)]
pub enum Passo {
    /// Subir o navegador do Braniac e esperar ate `espera_s` a porta de depuracao responder.
    LigarNavegador { espera_s: u32 },
    AbrirAba { dominio: String, url: String },
    /// Subir o servidor (UMA vez) e esperar ate `espera_s`.
    LigarServidor { espera_s: u32 },
    /// Tirar o Braniac da pausa (modo jogo).
    Acordar,
    /// Mostrar a tela: o programa proprio se existir, senao o navegador.
    AbrirInterface { programa: bool },
}

pub struct Situacao<'a> {
    pub navegador_ok: bool,
    /// Servidor respondeu OU a porta esta aberta (servidor ocupado demora a responder: porta aberta = ja esta de pe).
    pub servidor_ok: bool,
    /// Enderecos das abas abertas agora.
    pub abas_abertas: &'a [String],
    pub abrir_interface: bool,
    pub programa_existe: bool,
}

/// Abas que ainda faltam abrir.
pub fn abas_faltando(abas: &[(String, String)], abertas: &[String]) -> Vec<(String, String)> {
    abas.iter().filter(|(d, _)| !abertas.iter().any(|a| a.contains(d.as_str()))).cloned().collect()
}

/// Os passos, na ordem de subida. Com o navegador ainda fora, todas as abas entram (ele sobe vazio).
pub fn passos(s: &Situacao, abas: &[(String, String)]) -> Vec<Passo> {
    let mut p = Vec::new();
    if !s.navegador_ok {
        p.push(Passo::LigarNavegador { espera_s: 45 });
    }
    p.extend(abas_faltando(abas, if s.navegador_ok { s.abas_abertas } else { &[] }).into_iter().map(|(dominio, url)| Passo::AbrirAba { dominio, url }));
    if !s.servidor_ok {
        p.push(Passo::LigarServidor { espera_s: 40 });
    }
    p.push(Passo::Acordar);
    if s.abrir_interface {
        p.push(Passo::AbrirInterface { programa: s.programa_existe });
    }
    p
}

/// Depois de rodar: so segue para Acordar/AbrirInterface com o servidor de pe; as abas, so com o navegador de pe.
pub fn pode(passo: &Passo, navegador_ok: bool, servidor_ok: bool) -> bool {
    match passo {
        Passo::AbrirAba { .. } => navegador_ok,
        Passo::Acordar | Passo::AbrirInterface { .. } => servidor_ok,
        _ => true,
    }
}

/// Resultado final e codigo de saida (0 = tudo de pe).
pub fn resultado(navegador_ok: bool, servidor_ok: bool) -> (String, i32) {
    let t = |ok: bool| if ok { "OK" } else { "NAO SUBIU" };
    (format!("Navegador do Braniac: {}\nBraniac/Jarvis: {}", t(navegador_ok), t(servidor_ok)), if navegador_ok && servidor_ok { 0 } else { 1 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn so_sobe_o_que_ainda_nao_esta_de_pe_e_na_ordem() {
        let abas = abas_padrao();
        let tudo_fora = Situacao { navegador_ok: false, servidor_ok: false, abas_abertas: &[], abrir_interface: true, programa_existe: true };
        let p = passos(&tudo_fora, &abas);
        assert_eq!(p.first(), Some(&Passo::LigarNavegador { espera_s: 45 }));
        assert_eq!(p.iter().filter(|x| matches!(x, Passo::AbrirAba { .. })).count(), abas.len());
        assert_eq!(p[p.len() - 3..], [Passo::LigarServidor { espera_s: 40 }, Passo::Acordar, Passo::AbrirInterface { programa: true }]);
        let abertas = vec!["https://web.whatsapp.com/".to_string(), "https://chatgpt.com/c/123".to_string()];
        let de_pe = Situacao { navegador_ok: true, servidor_ok: true, abas_abertas: &abertas, abrir_interface: false, programa_existe: false };
        let p = passos(&de_pe, &abas);
        assert!(!p.iter().any(|x| matches!(x, Passo::LigarNavegador { .. } | Passo::LigarServidor { .. } | Passo::AbrirInterface { .. }))); // nunca um segundo servidor
        assert_eq!(p.iter().filter(|x| matches!(x, Passo::AbrirAba { .. })).count(), abas.len() - 2);
        assert!(!p.contains(&Passo::AbrirAba { dominio: "chatgpt.com".into(), url: "https://chatgpt.com/".into() }));
        assert_eq!(p.last(), Some(&Passo::Acordar));
        let tudo = abas.iter().map(|(_, u)| u.clone()).collect::<Vec<_>>();
        assert_eq!(passos(&Situacao { abas_abertas: &tudo, ..de_pe }, &abas), [Passo::Acordar]);
    }

    #[test]
    fn passo_que_depende_do_que_nao_subiu_nao_roda() {
        let aba = Passo::AbrirAba { dominio: "x".into(), url: "y".into() };
        assert!(!pode(&aba, false, true) && pode(&aba, true, false));
        assert!(!pode(&Passo::Acordar, true, false) && !pode(&Passo::AbrirInterface { programa: false }, true, false) && pode(&Passo::LigarServidor { espera_s: 40 }, false, false));
        assert_eq!(resultado(true, true).1, 0);
        assert_eq!(resultado(true, false), ("Navegador do Braniac: OK\nBraniac/Jarvis: NAO SUBIU".to_string(), 1));
    }
}
