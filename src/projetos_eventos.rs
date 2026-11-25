use serde_json::{json, Value};
use std::collections::HashSet;

pub fn normalizar(s: &str) -> String {
    let mut resultado = String::new();
    let mut separador_pendente = false;

    for c in s.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            if separador_pendente && !resultado.is_empty() {
                resultado.push(' ');
            }

            separador_pendente = false;

            match c {
                'á' | 'à' | 'â' | 'ã' | 'ä' | 'å' => resultado.push('a'),
                'é' | 'è' | 'ê' | 'ë' => resultado.push('e'),
                'í' | 'ì' | 'î' | 'ï' => resultado.push('i'),
                'ó' | 'ò' | 'ô' | 'õ' | 'ö' => resultado.push('o'),
                'ú' | 'ù' | 'û' | 'ü' => resultado.push('u'),
                'ç' => resultado.push('c'),
                'ñ' => resultado.push('n'),
                'ý' | 'ÿ' => resultado.push('y'),
                'ß' => resultado.push_str("ss"),
                'æ' => resultado.push_str("ae"),
                'œ' => resultado.push_str("oe"),
                _ => resultado.push(c),
            }
        } else {
            separador_pendente = true;
        }
    }

    resultado
}

pub fn semelhanca(a: &str, b: &str) -> f64 {
    let a = normalizar(a);
    let b = normalizar(b);

    if a == b {
        return 1.0;
    }

    // contido no outro so vale com 8+ letras (tarefa curta como "tela" nao casa com tudo)
    if (a.len() >= 8 && b.contains(&a))
        || (b.len() >= 8 && a.contains(&b))
    {
        return 1.0;
    }

    let palavras_a: HashSet<&str> = a
        .split_whitespace()
        .filter(|palavra| {
            palavra.chars().filter(|c| c.is_alphabetic()).count() >= 3
        })
        .collect();

    let palavras_b: HashSet<&str> = b
        .split_whitespace()
        .filter(|palavra| {
            palavra.chars().filter(|c| c.is_alphabetic()).count() >= 3
        })
        .collect();

    let intersecao = palavras_a.intersection(&palavras_b).count();
    let uniao = palavras_a.union(&palavras_b).count();

    if uniao == 0 {
        0.0
    } else {
        intersecao as f64 / uniao as f64
    }
}

pub fn sensivel(s: &str) -> bool {
    let normalizado = normalizar(s);

    for palavra in normalizado.split_whitespace() {
        if ["login", "senha", "senhas", "token", "tokens", "cpf", "cartao", "pix"].contains(&palavra)
            || palavra.starts_with("passw")
        {
            return true;
        }
    }

    // Detecta "sk-" no início de um token, não no meio de uma palavra.
    let bytes = s.as_bytes();

    for i in 0..bytes.len().saturating_sub(2) {
        if bytes[i].eq_ignore_ascii_case(&b's')
            && bytes[i + 1].eq_ignore_ascii_case(&b'k')
            && bytes[i + 2] == b'-'
        {
            let inicio_token = i == 0
                || !bytes[i - 1].is_ascii_alphanumeric();

            if inicio_token {
                return true;
            }
        }
    }

    // Detecta sequências de 11 a 19 dígitos, permitindo pontos
    // ou hífens entre os dígitos.
    let mut i = 0;

    while i < bytes.len() {
        if !bytes[i].is_ascii_digit() {
            i += 1;
            continue;
        }

        let mut quantidade = 0;
        let mut j = i;

        while j < bytes.len() {
            if bytes[j].is_ascii_digit() {
                quantidade += 1;
                j += 1;
            } else if (bytes[j] == b'.' || bytes[j] == b'-')
                && j + 1 < bytes.len()
                && bytes[j + 1].is_ascii_digit()
            {
                j += 1;
            } else {
                break;
            }
        }

        if (11..=19).contains(&quantidade) {
            return true;
        }

        i = j.max(i + 1);
    }

    false
}

fn validar_estrutura(proj: &Value) -> Result<(), String> {
    let objeto = proj
        .as_object()
        .ok_or_else(|| "projeto deve ser um objeto JSON".to_string())?;

    let pendencias = objeto
        .get("pendencias")
        .and_then(Value::as_array)
        .ok_or_else(|| "pendencias deve ser uma lista".to_string())?;

    for pendencia in pendencias {
        if pendencia.get("texto").and_then(Value::as_str).is_none() {
            return Err("cada pendencia deve ter texto string".to_string());
        }

        if pendencia.get("feita").and_then(Value::as_bool).is_none() {
            return Err("cada pendencia deve ter feita boolean".to_string());
        }
    }

    objeto
        .get("proximos")
        .and_then(Value::as_array)
        .ok_or_else(|| "proximos deve ser uma lista".to_string())?
        .iter()
        .try_for_each(|item| {
            if item.as_str().is_some() {
                Ok(())
            } else {
                Err("proximos deve conter strings".to_string())
            }
        })?;

    objeto
        .get("bloqueios")
        .and_then(Value::as_array)
        .ok_or_else(|| "bloqueios deve ser uma lista".to_string())?
        .iter()
        .try_for_each(|item| {
            if item.as_str().is_some() {
                Ok(())
            } else {
                Err("bloqueios deve conter strings".to_string())
            }
        })?;

    Ok(())
}

fn indice_mais_parecido(
    pendencias: &[Value],
    tarefa: &str,
    limite: f64,
) -> Option<usize> {
    let mut melhor_indice = None;
    let mut melhor_score = limite;

    for (indice, pendencia) in pendencias.iter().enumerate() {
        if pendencia.get("feita").and_then(Value::as_bool) != Some(false) {
            continue;
        }

        let texto = pendencia
            .get("texto")
            .and_then(Value::as_str)?;

        let score = semelhanca(texto, tarefa);

        if score >= melhor_score {
            melhor_score = score;
            melhor_indice = Some(indice);
        }
    }

    melhor_indice
}

pub fn aplicar(
    proj: &mut Value,
    tarefa: &str,
    estado: &str,
    nota: Option<&str>,
    hoje: &str,
) -> Result<String, String> {
    let tarefa = tarefa.trim();

    if tarefa.is_empty() {
        return Err("tarefa vazia".to_string());
    }

    if tarefa.chars().count() > 300 {
        return Err("tarefa excede 300 caracteres".to_string());
    }

    if sensivel(tarefa) {
        return Err("tarefa contém informação sensível".to_string());
    }

    match estado {
        "feito" | "novo" | "falta" | "bloqueado" | "andando" => {}
        _ => return Err("estado inválido".to_string()),
    }

    // registro real pode nao ter todas as listas: completa o que falta antes de validar
    if let Some(o) = proj.as_object_mut() {
        for k in ["pendencias", "proximos", "bloqueios"] {
            o.entry(k).or_insert_with(|| json!([]));
        }
        for p in o["pendencias"].as_array_mut().into_iter().flatten().filter_map(Value::as_object_mut) {
            p.entry("feita").or_insert(json!(false));
        }
    }
    validar_estrutura(proj)?;

    let resultado = match estado {
        "feito" => {
            let pendencias = proj["pendencias"]
                .as_array_mut()
                .ok_or_else(|| "pendencias inválidas".to_string())?;

            let indice = match indice_mais_parecido(pendencias, tarefa, 0.5) {
                Some(indice) => indice,
                None => {
                    atualizar_atividade(proj, hoje, nota);
                    return Ok("sem_casamento".to_string());
                }
            };

            pendencias[indice]["feita"] = json!(true);

            let ainda_aberta = pendencias.iter().any(|pendencia| {
                pendencia.get("feita").and_then(Value::as_bool) == Some(false)
            });

            if ainda_aberta {
                "feita".to_string()
            } else {
                let proximo = proj["proximos"]
                    .as_array_mut()
                    .ok_or_else(|| "proximos inválidos".to_string())?;

                if !proximo.is_empty() {
                    let texto = proximo.remove(0);

                    proj["pendencias"]
                        .as_array_mut()
                        .ok_or_else(|| "pendencias inválidas".to_string())?
                        .push(json!({
                            "texto": texto,
                            "feita": false
                        }));

                    "promovida".to_string()
                } else {
                    proj["estado"] = json!("pronto");
                    "pronto".to_string()
                }
            }
        }

        "novo" | "falta" => {
            let pendencias = proj["pendencias"]
                .as_array_mut()
                .ok_or_else(|| "pendencias inválidas".to_string())?;

            if indice_mais_parecido(pendencias, tarefa, 0.8).is_none() {
                pendencias.push(json!({
                    "texto": tarefa,
                    "feita": false
                }));
            }

            "criada".to_string()
        }

        "bloqueado" => {
            let bloqueios = proj["bloqueios"]
                .as_array_mut()
                .ok_or_else(|| "bloqueios inválidos".to_string())?;

            if !bloqueios.iter().any(|item| item.as_str() == Some(tarefa)) {
                bloqueios.push(json!(tarefa));
            }

            "bloqueada".to_string()
        }

        "andando" => {
            proj["estado"] = json!("andando");
            "andando".to_string()
        }

        _ => unreachable!(),
    };

    atualizar_atividade(proj, hoje, nota);

    Ok(resultado)
}

fn atualizar_atividade(
    proj: &mut Value,
    hoje: &str,
    nota: Option<&str>,
) {
    let atividade = match nota {
        Some(nota) => format!("{} ({})", hoje, nota),
        None => hoje.to_string(),
    };

    proj["ultima_atividade"] = json!(atividade);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn projeto() -> Value {
        json!({
            "pendencias": [
                {
                    "texto": "Revisar documentação",
                    "feita": false
                }
            ],
            "proximos": [],
            "bloqueios": [],
            "estado": "andando",
            "ultima_atividade": ""
        })
    }

    #[test]
    fn casa_pendencia() {
        let mut proj = projeto();

        let resultado = aplicar(
            &mut proj,
            "revisar documentacao",
            "feito",
            None,
            "2026-11-24",
        )
        .unwrap();

        assert_eq!(resultado, "pronto");
        assert_eq!(proj["pendencias"][0]["feita"], true);
        assert_eq!(proj["estado"], "pronto");
    }

    #[test]
    fn novo_sem_duplicar() {
        let mut proj = projeto();

        aplicar(
            &mut proj,
            "Revisar documentação",
            "novo",
            None,
            "2026-11-24",
        )
        .unwrap();

        aplicar(
            &mut proj,
            "Revisar documentação",
            "novo",
            None,
            "2026-11-24",
        )
        .unwrap();

        assert_eq!(proj["pendencias"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn recusa_sensivel() {
        let mut proj = projeto();

        let resultado = aplicar(
            &mut proj,
            "usar login do sistema",
            "novo",
            None,
            "2026-11-24",
        );

        assert!(resultado.is_err());
    }

    #[test]
    fn promove_proximo() {
        let mut proj = projeto();

        proj["proximos"] = json!(["Preparar relatório"]);

        let resultado = aplicar(
            &mut proj,
            "Revisar documentação",
            "feito",
            None,
            "2026-11-24",
        )
        .unwrap();

        assert_eq!(resultado, "promovida");
        assert_eq!(proj["pendencias"][1]["texto"], "Preparar relatório");
        assert_eq!(proj["pendencias"][1]["feita"], false);
        assert_eq!(proj["proximos"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn retorna_pronto_sem_proximos() {
        let mut proj = projeto();

        let resultado = aplicar(
            &mut proj,
            "Revisar documentação",
            "feito",
            None,
            "2026-11-24",
        )
        .unwrap();

        assert_eq!(resultado, "pronto");
        assert_eq!(proj["estado"], "pronto");
    }

    #[test]
    fn sk_prefixo_detectado_sem_falso_positivo() {
        assert!(sensivel("sk-abc123"));
        assert!(sensivel("chave sk-abc123"));
        assert!(!sensivel("task-"));
    }

    #[test]
    fn normaliza_acento_e_espacos() {
        assert_eq!(normalizar("  AÇÃO   Rápida! "), "acao rapida");
    }
}