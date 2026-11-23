//! Deixa uma linha do relatorio PRONUNCIAVEL, para virar audio: sem link, caminho, codigo, emoji ou numero enorme; so a primeira frase.
//! (ticket T071 do NexoDev veio cortado; escrito pelo senior)

fn descarta(w: &str) -> bool {
    let miolo = w.trim_matches(|c: char| !c.is_alphanumeric());
    w.starts_with("http://") || w.starts_with("https://") || w.contains(":\\") || w.contains(":/") || (w.contains('/') && w.contains('.'))
        || (miolo.chars().count() >= 6 && miolo.chars().all(|c| c.is_ascii_digit()))
}

pub fn para_fala(texto: &str, maximo: usize) -> String {
    // trecho entre crases e codigo: sai inteiro
    let sem_codigo: String = texto.split('`').step_by(2).collect::<Vec<_>>().join(" ");
    let t = sem_codigo.split_whitespace().filter(|w| !descarta(w)).collect::<Vec<_>>().join(" ");
    let t = t.replace('_', " ").replace('—', ",").replace('/', " de ");
    let t: String = t.chars().filter(|c| c.is_ascii() || c.is_alphanumeric() || *c == '·').collect();
    let c: Vec<char> = t.split_whitespace().collect::<Vec<_>>().join(" ").chars().collect();
    // primeira frase: o menor trecho de 40 a `maximo` caracteres seguido de . ! ou ? e depois espaco ou fim
    let fim = (40..=maximo.min(c.len().saturating_sub(1))).find(|&i| matches!(c[i], '.' | '!' | '?') && c.get(i + 1).map_or(true, |p| *p == ' '));
    let corte = fim.map_or(maximo.min(c.len()), |i| i + 1);
    c[..corte].iter().collect::<String>().trim_matches(|x| matches!(x, ' ' | '-' | '·')).to_string()
}

#[cfg(test)]
mod tests {
    use super::para_fala;

    #[test]
    fn tira_o_que_nao_se_fala_e_fica_na_primeira_frase() {
        for (entra, sai) in [
            ("Veja https://exemplo.com/x agora", "Veja agora"),
            ("☀️ Site novo no ar", "Site novo no ar"),
            ("Rodei `cargo test` e passou", "Rodei e passou"),
            ("Backup_diario feito 12/11", "Backup diario feito 12 de 11"),
            ("Pedido 1234567 enviado", "Pedido enviado"),
            ("Arquivo C:\\dados\\x.txt e src/main.rs lidos", "Arquivo e lidos"),
            ("A manutencao da madrugada rodou todos os testes. Depois limpou o disco.", "A manutencao da madrugada rodou todos os testes."),
            ("Feito. Tudo certo com o backup de hoje, sem erro nenhum.", "Feito. Tudo certo com o backup de hoje, sem erro nenhum."),
            ("- ação concluída ·", "ação concluída"),
            ("", ""),
        ] {
            assert_eq!(para_fala(entra, 230), sai);
        }
        assert_eq!(para_fala(&"á".repeat(300), 230).chars().count(), 230); // corta por caractere, nao por byte
    }
}
