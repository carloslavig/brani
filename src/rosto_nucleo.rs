#![allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub struct Pessoa {
    pub nome: String,
    pub vetores: Vec<Vec<f32>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Veredito {
    Conhecido {
        nome: String,
        pontuacao: f32,
    },
    Incerto {
        melhor: String,
        pontuacao: f32,
    },
    Desconhecido,
}

/// Calcula a similaridade de cosseno entre dois vetores.
pub fn cosseno(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let mut produto = 0.0_f64;
    let mut norma_a = 0.0_f64;
    let mut norma_b = 0.0_f64;

    for (&x, &y) in a.iter().zip(b.iter()) {
        let x = x as f64;
        let y = y as f64;

        if !x.is_finite() || !y.is_finite() {
            return 0.0;
        }

        produto += x * y;
        norma_a += x * x;
        norma_b += y * y;
    }

    if norma_a == 0.0 || norma_b == 0.0 {
        return 0.0;
    }

    (produto / (norma_a.sqrt() * norma_b.sqrt()))
        .clamp(-1.0, 1.0) as f32
}

/// Identifica a pessoa com maior similaridade.
pub fn identificar(
    v: &[f32],
    gente: &[Pessoa],
    limiar: f32,
    margem: f32,
) -> Veredito {
    if v.is_empty()
        || gente.is_empty()
        || !limiar.is_finite()
        || !margem.is_finite()
    {
        return Veredito::Desconhecido;
    }

    let mut pontuacoes: Vec<(&Pessoa, f32)> = gente
        .iter()
        .filter_map(|pessoa| {
            let melhor = pessoa
                .vetores
                .iter()
                .map(|vetor| cosseno(v, vetor))
                .fold(f32::NEG_INFINITY, f32::max);

            if melhor == f32::NEG_INFINITY {
                None
            } else {
                Some((pessoa, melhor))
            }
        })
        .collect();

    if pontuacoes.is_empty() {
        return Veredito::Desconhecido;
    }

    pontuacoes.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let (melhor_pessoa, melhor_pontuacao) = pontuacoes[0];

    let segunda_pontuacao = pontuacoes
        .get(1)
        .map(|(_, pontuacao)| *pontuacao)
        .unwrap_or(f32::NEG_INFINITY);

    if melhor_pontuacao >= limiar
        && melhor_pontuacao - segunda_pontuacao >= margem
    {
        Veredito::Conhecido {
            nome: melhor_pessoa.nome.clone(),
            pontuacao: melhor_pontuacao,
        }
    } else if melhor_pontuacao >= limiar - 0.1 {
        Veredito::Incerto {
            melhor: melhor_pessoa.nome.clone(),
            pontuacao: melhor_pontuacao,
        }
    } else {
        Veredito::Desconhecido
    }
}

const TAM_ALINHADO: usize = 112;
const CANAIS: usize = 3;

const DESTINO: [(f32, f32); 5] = [
    (38.2946, 51.6963),
    (73.5318, 51.5014),
    (56.0252, 71.7366),
    (41.5493, 92.3655),
    (70.7299, 92.2041),
];

/// Transformação de similaridade 2D.
///
/// x' = a*x - b*y + tx
/// y' = b*x + a*y + ty
///
/// A transformação é estimada por mínimos quadrados.
#[derive(Debug, Clone, Copy)]
struct Transformacao {
    a: f64,
    b: f64,
    tx: f64,
    ty: f64,
}

impl Transformacao {
    /// Estima a transformação dos pontos de origem para destino.
    fn estimar(
        origem: &[(f32, f32); 5],
        destino: &[(f32, f32); 5],
    ) -> Option<Self> {
        let mut mx = 0.0_f64;
        let mut my = 0.0_f64;
        let mut mu = 0.0_f64;
        let mut mv = 0.0_f64;

        for i in 0..5 {
            let (x, y) = origem[i];
            let (u, v) = destino[i];

            if !x.is_finite()
                || !y.is_finite()
                || !u.is_finite()
                || !v.is_finite()
            {
                return None;
            }

            mx += x as f64;
            my += y as f64;
            mu += u as f64;
            mv += v as f64;
        }

        mx /= 5.0;
        my /= 5.0;
        mu /= 5.0;
        mv /= 5.0;

        let mut denominador = 0.0_f64;
        let mut numerador_a = 0.0_f64;
        let mut numerador_b = 0.0_f64;

        for i in 0..5 {
            let (x, y) = origem[i];
            let (u, v) = destino[i];

            let x = x as f64 - mx;
            let y = y as f64 - my;
            let u = u as f64 - mu;
            let v = v as f64 - mv;

            denominador += x * x + y * y;

            numerador_a += x * u + y * v;
            numerador_b += x * v - y * u;
        }

        if !denominador.is_finite()
            || denominador <= f64::EPSILON
        {
            return None;
        }

        let a = numerador_a / denominador;
        let b = numerador_b / denominador;

        let tx = mu - a * mx + b * my;
        let ty = mv - b * mx - a * my;

        let escala_quadrada = a * a + b * b;

        if !a.is_finite()
            || !b.is_finite()
            || !tx.is_finite()
            || !ty.is_finite()
            || !escala_quadrada.is_finite()
            || escala_quadrada <= f64::EPSILON
        {
            return None;
        }

        Some(Self { a, b, tx, ty })
    }

    /// Calcula o ponto de origem correspondente a um ponto de destino.
    ///
    /// Usa a transformação inversa para evitar lacunas na imagem.
    fn inversa(&self, x: f64, y: f64) -> Option<(f64, f64)> {
        let dx = x - self.tx;
        let dy = y - self.ty;

        let escala_quadrada = self.a * self.a + self.b * self.b;

        if !escala_quadrada.is_finite()
            || escala_quadrada <= f64::EPSILON
        {
            return None;
        }

        let sx = (self.a * dx + self.b * dy)
            / escala_quadrada;

        let sy = (-self.b * dx + self.a * dy)
            / escala_quadrada;

        if !sx.is_finite() || !sy.is_finite() {
            return None;
        }

        Some((sx, sy))
    }
}

/// Amostra um canal usando interpolação bilinear.
///
/// Pixels fora da imagem contribuem com zero.
fn bilinear(
    imagem: &[u8],
    largura: usize,
    altura: usize,
    canal: usize,
    x: f64,
    y: f64,
) -> f32 {
    if !x.is_finite() || !y.is_finite() {
        return 0.0;
    }

    let x0 = x.floor();
    let y0 = y.floor();

    let fx = x - x0;
    let fy = y - y0;

    let x0i = x0 as i64;
    let y0i = y0 as i64;

    let x1i = x0i.saturating_add(1);
    let y1i = y0i.saturating_add(1);

    let obter = |px: i64, py: i64| -> f64 {
        if px < 0
            || py < 0
            || px >= largura as i64
            || py >= altura as i64
        {
            return 0.0;
        }

        let indice = (
            py as usize * largura + px as usize
        ) * CANAIS + canal;

        imagem[indice] as f64
    };

    let p00 = obter(x0i, y0i);
    let p10 = obter(x1i, y0i);
    let p01 = obter(x0i, y1i);
    let p11 = obter(x1i, y1i);

    let superior = p00 * (1.0 - fx) + p10 * fx;
    let inferior = p01 * (1.0 - fx) + p11 * fx;

    (superior * (1.0 - fy) + inferior * fy) as f32
}

/// Alinha um rosto para 112x112 usando os cinco pontos faciais.
///
/// Entrada:
/// - rgb: imagem RGB intercalada HWC.
/// - larg: largura da imagem.
/// - alt: altura da imagem.
/// - pontos: olhos, nariz e cantos da boca, nessa ordem.
///
/// Saída:
/// - 112x112x3 floats.
/// - Ordem BGR intercalada HWC.
/// - Valores entre 0 e 255, sem normalização.
///
/// Imagens inválidas ou pontos degenerados produzem uma imagem zerada.
pub fn alinhar(
    rgb: &[u8],
    larg: usize,
    alt: usize,
    pontos: [(f32, f32); 5],
) -> Vec<f32> {
    let total_saida = TAM_ALINHADO * TAM_ALINHADO * CANAIS;

    let mut saida = vec![0.0_f32; total_saida];

    let tamanho_entrada = match larg
        .checked_mul(alt)
        .and_then(|n| n.checked_mul(CANAIS))
    {
        Some(n) => n,
        None => return saida,
    };

    if larg == 0 || alt == 0 || rgb.len() < tamanho_entrada {
        return saida;
    }

    let transformacao = match Transformacao::estimar(
        &pontos,
        &DESTINO,
    ) {
        Some(t) => t,
        None => return saida,
    };

    for y in 0..TAM_ALINHADO {
        for x in 0..TAM_ALINHADO {
            let (sx, sy) = match transformacao.inversa(
                x as f64,
                y as f64,
            ) {
                Some(p) => p,
                None => continue,
            };

            let indice = (y * TAM_ALINHADO + x) * CANAIS;

            // Entrada RGB; saída BGR.
            saida[indice] = bilinear(
                rgb,
                larg,
                alt,
                2,
                sx,
                sy,
            );

            saida[indice + 1] = bilinear(
                rgb,
                larg,
                alt,
                1,
                sx,
                sy,
            );

            saida[indice + 2] = bilinear(
                rgb,
                larg,
                alt,
                0,
                sx,
                sy,
            );
        }
    }

    saida
}

/// Normaliza um vetor pela norma L2.
///
/// Vetores vazios, com norma zero ou valores não finitos
/// são preservados sem divisão inválida.
pub fn normalizar_l2(v: &mut Vec<f32>) {
    if v.is_empty() {
        return;
    }

    if v.iter().any(|x| !x.is_finite()) {
        return;
    }

    let norma_quadrada: f64 = v
        .iter()
        .map(|&x| {
            let x = x as f64;
            x * x
        })
        .sum();

    if !norma_quadrada.is_finite()
        || norma_quadrada <= f64::EPSILON
    {
        return;
    }

    let norma = norma_quadrada.sqrt();

    for valor in v.iter_mut() {
        *valor = (*valor as f64 / norma) as f32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn imagem_teste() -> Vec<u8> {
        let mut imagem = vec![0u8; 112 * 112 * 3];

        for y in 0..112 {
            for x in 0..112 {
                let i = (y * 112 + x) * 3;

                imagem[i] = (x % 256) as u8;
                imagem[i + 1] = (y % 256) as u8;
                imagem[i + 2] = ((x + y) % 256) as u8;
            }
        }

        imagem
    }

    fn pontos_destino() -> [(f32, f32); 5] {
        DESTINO
    }

    fn distancia(a: f32, b: f32) -> f32 {
        (a - b).abs()
    }

    #[test]
    fn alinhamento_identidade_converte_rgb_para_bgr() {
        let rgb = imagem_teste();

        let resultado = alinhar(
            &rgb,
            112,
            112,
            pontos_destino(),
        );

        assert_eq!(resultado.len(), 112 * 112 * 3);

        for y in 0..112 {
            for x in 0..112 {
                let i = (y * 112 + x) * 3;

                assert!(
                    distancia(resultado[i], rgb[i + 2] as f32)
                        < 0.001
                );

                assert!(
                    distancia(
                        resultado[i + 1],
                        rgb[i + 1] as f32
                    ) < 0.001
                );

                assert!(
                    distancia(resultado[i + 2], rgb[i] as f32)
                        < 0.001
                );
            }
        }
    }

    #[test]
    fn alinhamento_rotacao_conhecida() {
        let rgb = imagem_teste();

        let angulo = std::f32::consts::FRAC_PI_2;
        let cos = angulo.cos();
        let sin = angulo.sin();

        let cx = 56.0;
        let cy = 56.0;

        // Origem rotacionada em relação aos pontos de destino.
        let origem = DESTINO.map(|(x, y)| {
            let dx = x - cx;
            let dy = y - cy;

            (
                cx + cos * dx - sin * dy,
                cy + sin * dx + cos * dy,
            )
        });

        let resultado = alinhar(
            &rgb,
            112,
            112,
            origem,
        );

        assert_eq!(resultado.len(), 112 * 112 * 3);

        // O ponto central permanece próximo do centro.
        let centro = (56 * 112 + 56) * 3;

        assert!(resultado[centro].is_finite());
        assert!(resultado[centro + 1].is_finite());
        assert!(resultado[centro + 2].is_finite());

        // A imagem precisa conter pixels amostrados.
        assert!(resultado.iter().any(|&v| v > 0.0));
    }

    #[test]
    fn normalizacao_l2_produz_norma_um() {
        let mut v = vec![3.0, 4.0];

        normalizar_l2(&mut v);

        assert!((v[0] - 0.6).abs() < 1e-6);
        assert!((v[1] - 0.8).abs() < 1e-6);

        let norma = v.iter()
            .map(|x| x * x)
            .sum::<f32>()
            .sqrt();

        assert!((norma - 1.0).abs() < 1e-6);
    }

    #[test]
    fn normalizacao_nao_divide_por_zero() {
        let mut v = vec![0.0, 0.0, 0.0];

        normalizar_l2(&mut v);

        assert_eq!(v, vec![0.0, 0.0, 0.0]);
    }

    #[test]
    fn imagem_invalida_retorna_zeros() {
        let resultado = alinhar(
            &[1, 2, 3],
            112,
            112,
            pontos_destino(),
        );

        assert_eq!(resultado.len(), 112 * 112 * 3);
        assert!(resultado.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn pontos_degenerados_retorna_zeros() {
        let pontos = [(10.0, 10.0); 5];

        let resultado = alinhar(
            &imagem_teste(),
            112,
            112,
            pontos,
        );

        assert!(resultado.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn cosseno_identico() {
        let a = vec![1.0, 2.0, 3.0];

        assert!((cosseno(&a, &a) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosseno_tamanhos_diferentes() {
        assert_eq!(
            cosseno(&[1.0, 2.0], &[1.0]),
            0.0
        );
    }

    #[test]
    fn identificar_pessoa_conhecida() {
        let gente = vec![
            Pessoa {
                nome: "Ana".to_string(),
                vetores: vec![vec![1.0, 0.0]],
            },
            Pessoa {
                nome: "Bruno".to_string(),
                vetores: vec![vec![0.0, 1.0]],
            },
        ];

        let resultado = identificar(
            &[1.0, 0.0],
            &gente,
            0.8,
            0.1,
        );

        assert_eq!(
            resultado,
            Veredito::Conhecido {
                nome: "Ana".to_string(),
                pontuacao: 1.0,
            }
        );
    }
}