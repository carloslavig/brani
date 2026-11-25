#![allow(dead_code)]
use std::cmp::Ordering;

/// Representação de um rosto detectado.
#[derive(Debug, Clone, PartialEq)]
pub struct Rosto {
    pub x: f32,
    pub y: f32,
    pub l: f32,
    pub a: f32,
    pub pontos: [(f32, f32); 5],
    pub confianca: f32,
}

/// Decodifica as saídas do detector YuNet 2023mar.
///
/// Strides: 8, 16 e 32.
///
/// Layout de cada escala:
/// - cls: N valores.
/// - obj: N valores.
/// - bbox: N * 4 valores (x, y, w, h).
/// - kps: N * 10 valores (x1, y1, ..., x5, y5).
///
/// N = (tam / stride)².
///
/// Entradas incompletas ou valores não finitos são ignorados.
pub fn decodificar(
    cls: &[Vec<f32>; 3],
    obj: &[Vec<f32>; 3],
    bbox: &[Vec<f32>; 3],
    kps: &[Vec<f32>; 3],
    tam: u32,
    limiar: f32,
) -> Vec<Rosto> {
    let strides = [8u32, 16, 32];
    let mut rostos = Vec::new();

    if tam == 0 || !limiar.is_finite() {
        return rostos;
    }

    for escala in 0..3 {
        let stride = strides[escala];
        let cols = (tam / stride) as usize;
        let rows = cols;

        if cols == 0 {
            continue;
        }

        let Some(total) = rows.checked_mul(cols) else {
            continue;
        };

        let Some(bbox_len) = total.checked_mul(4) else {
            continue;
        };

        let Some(kps_len) = total.checked_mul(10) else {
            continue;
        };

        if cls[escala].len() < total
            || obj[escala].len() < total
            || bbox[escala].len() < bbox_len
            || kps[escala].len() < kps_len
        {
            continue;
        }

        let stride_f = stride as f32;

        for r in 0..rows {
            for c in 0..cols {
                let idx = r * cols + c;

                let classificacao = cls[escala][idx];
                let objetividade = obj[escala][idx];

                if !classificacao.is_finite()
                    || !objetividade.is_finite()
                {
                    continue;
                }

                let classificacao = classificacao.clamp(0.0, 1.0);
                let objetividade = objetividade.clamp(0.0, 1.0);

                // Confiança especificada: sqrt(clamp(cls) * clamp(obj)).
                let confianca = (classificacao * objetividade).sqrt();

                if !confianca.is_finite() || confianca < limiar {
                    continue;
                }

                let bi = idx * 4;

                let bx = bbox[escala][bi];
                let by = bbox[escala][bi + 1];
                let bw = bbox[escala][bi + 2];
                let bh = bbox[escala][bi + 3];

                if !bx.is_finite()
                    || !by.is_finite()
                    || !bw.is_finite()
                    || !bh.is_finite()
                {
                    continue;
                }

                let cx = (c as f32 + bx) * stride_f;
                let cy = (r as f32 + by) * stride_f;

                let w = bw.exp() * stride_f;
                let h = bh.exp() * stride_f;

                if !cx.is_finite()
                    || !cy.is_finite()
                    || !w.is_finite()
                    || !h.is_finite()
                    || w <= 0.0
                    || h <= 0.0
                {
                    continue;
                }

                let x = cx - w / 2.0;
                let y = cy - h / 2.0;

                if !x.is_finite() || !y.is_finite() {
                    continue;
                }

                let ki = idx * 10;
                let mut pontos = [(0.0f32, 0.0f32); 5];
                let mut pontos_validos = true;

                for i in 0..5 {
                    let px = kps[escala][ki + i * 2];
                    let py = kps[escala][ki + i * 2 + 1];

                    if !px.is_finite() || !py.is_finite() {
                        pontos_validos = false;
                        break;
                    }

                    let ponto_x = (px + c as f32) * stride_f;
                    let ponto_y = (py + r as f32) * stride_f;

                    if !ponto_x.is_finite() || !ponto_y.is_finite() {
                        pontos_validos = false;
                        break;
                    }

                    pontos[i] = (ponto_x, ponto_y);
                }

                if !pontos_validos {
                    continue;
                }

                rostos.push(Rosto {
                    x,
                    y,
                    l: w,
                    a: h,
                    pontos,
                    confianca,
                });
            }
        }
    }

    rostos
}

/// Calcula a interseção sobre união (IoU) entre dois rostos.
fn calcular_iou(a: &Rosto, b: &Rosto) -> f32 {
    if !a.x.is_finite()
        || !a.y.is_finite()
        || !a.l.is_finite()
        || !a.a.is_finite()
        || !b.x.is_finite()
        || !b.y.is_finite()
        || !b.l.is_finite()
        || !b.a.is_finite()
        || a.l <= 0.0
        || a.a <= 0.0
        || b.l <= 0.0
        || b.a <= 0.0
    {
        return 0.0;
    }

    let ax2 = a.x + a.l;
    let ay2 = a.y + a.a;
    let bx2 = b.x + b.l;
    let by2 = b.y + b.a;

    if !ax2.is_finite()
        || !ay2.is_finite()
        || !bx2.is_finite()
        || !by2.is_finite()
    {
        return 0.0;
    }

    let ix1 = a.x.max(b.x);
    let iy1 = a.y.max(b.y);
    let ix2 = ax2.min(bx2);
    let iy2 = ay2.min(by2);

    let iw = (ix2 - ix1).max(0.0);
    let ih = (iy2 - iy1).max(0.0);

    let intersecao = iw * ih;

    let area_a = a.l * a.a;
    let area_b = b.l * b.a;

    let uniao = area_a + area_b - intersecao;

    if !intersecao.is_finite()
        || !uniao.is_finite()
        || uniao <= 0.0
    {
        return 0.0;
    }

    intersecao / uniao
}

/// Non-Maximum Suppression.
///
/// Ordena os rostos por confiança decrescente e remove
/// detecções cujo IoU com uma detecção já mantida seja
/// maior que o limiar informado.
pub fn nms(v: Vec<Rosto>, iou: f32) -> Vec<Rosto> {
    let mut ordenados = v;

    ordenados.sort_by(|a, b| {
        b.confianca.total_cmp(&a.confianca)
    });

    if !iou.is_finite() {
        return ordenados;
    }

    let limiar = iou.clamp(0.0, 1.0);
    let mut resultado: Vec<Rosto> = Vec::new();

    for rosto in ordenados {
        let sobreposto = resultado
            .iter()
            .any(|mantido| calcular_iou(&rosto, mantido) > limiar);

        if !sobreposto {
            resultado.push(rosto);
        }
    }

    resultado
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entradas(
        classificacao: f32,
        objetividade: f32,
    ) -> (
        [Vec<f32>; 3],
        [Vec<f32>; 3],
        [Vec<f32>; 3],
        [Vec<f32>; 3],
    ) {
        let cls = [
            vec![classificacao],
            vec![],
            vec![],
        ];

        let obj = [
            vec![objetividade],
            vec![],
            vec![],
        ];

        // Centro relativo (0, 0), largura e altura iguais a 8.
        let bbox = [
            vec![0.0, 0.0, 0.0, 0.0],
            vec![],
            vec![],
        ];

        // Cinco pontos relativos.
        let kps = [
            vec![
                0.1, 0.2,
                0.3, 0.4,
                0.5, 0.6,
                0.7, 0.8,
                0.9, 1.0,
            ],
            vec![],
            vec![],
        ];

        (cls, obj, bbox, kps)
    }

    fn rosto(x: f32, confianca: f32) -> Rosto {
        Rosto {
            x,
            y: 0.0,
            l: 10.0,
            a: 10.0,
            pontos: [(0.0, 0.0); 5],
            confianca,
        }
    }

    #[test]
    fn decodifica_celula_unica() {
        let (cls, obj, bbox, kps) = entradas(0.81, 1.0);

        let rostos = decodificar(
            &cls,
            &obj,
            &bbox,
            &kps,
            8,
            0.5,
        );

        assert_eq!(rostos.len(), 1);

        let r = &rostos[0];

        // sqrt(0.81 * 1.0) = 0.9.
        assert!((r.confianca - 0.9).abs() < 0.0001);

        assert!((r.x + 4.0).abs() < 0.0001);
        assert!((r.y + 4.0).abs() < 0.0001);
        assert!((r.l - 8.0).abs() < 0.0001);
        assert!((r.a - 8.0).abs() < 0.0001);

        assert_eq!(
            r.pontos,
            [
                (0.8, 1.6),
                (2.4, 3.2),
                (4.0, 4.8),
                (5.6, 6.4),
                (7.2, 8.0),
            ]
        );
    }

    #[test]
    fn limiar_filtra_deteccoes() {
        let (cls, obj, bbox, kps) = entradas(0.81, 1.0);

        let rostos = decodificar(
            &cls,
            &obj,
            &bbox,
            &kps,
            8,
            0.95,
        );

        assert!(rostos.is_empty());
    }

    #[test]
    fn limiar_inclusivo_mantem_confianca_igual() {
        let (cls, obj, bbox, kps) = entradas(0.81, 1.0);

        let rostos = decodificar(
            &cls,
            &obj,
            &bbox,
            &kps,
            8,
            0.9,
        );

        assert_eq!(rostos.len(), 1);
        assert!((rostos[0].confianca - 0.9).abs() < 0.0001);
    }

    #[test]
    fn nms_remove_sobreposto_e_mantem_distante() {
        let rostos = vec![
            rosto(0.0, 0.9),
            rosto(1.0, 0.8),
            rosto(30.0, 0.7),
        ];

        let resultado = nms(rostos, 0.5);

        assert_eq!(resultado.len(), 2);

        assert!((resultado[0].confianca - 0.9).abs() < 0.0001);
        assert!((resultado[1].confianca - 0.7).abs() < 0.0001);

        assert!((resultado[0].x - 0.0).abs() < 0.0001);
        assert!((resultado[1].x - 30.0).abs() < 0.0001);
    }

    #[test]
    fn nms_ordena_por_confianca_decrescente() {
        let rostos = vec![
            rosto(0.0, 0.3),
            rosto(30.0, 0.9),
            rosto(60.0, 0.6),
        ];

        let resultado = nms(rostos, 0.5);

        assert_eq!(resultado.len(), 3);

        assert!((resultado[0].confianca - 0.9).abs() < 0.0001);
        assert!((resultado[1].confianca - 0.6).abs() < 0.0001);
        assert!((resultado[2].confianca - 0.3).abs() < 0.0001);
    }

    #[test]
    fn entradas_incompletas_sao_ignoradas() {
        let cls = [
            vec![0.9],
            vec![],
            vec![],
        ];

        let obj = [
            vec![1.0],
            vec![],
            vec![],
        ];

        let bbox = [
            vec![0.0, 0.0, 0.0],
            vec![],
            vec![],
        ];

        let kps = [
            vec![0.0; 10],
            vec![],
            vec![],
        ];

        let rostos = decodificar(
            &cls,
            &obj,
            &bbox,
            &kps,
            8,
            0.5,
        );

        assert!(rostos.is_empty());
    }

    #[test]
    fn valores_nao_finitos_sao_ignorados() {
        let (mut cls, obj, mut bbox, kps) = entradas(0.81, 1.0);

        cls[0][0] = f32::NAN;

        let rostos = decodificar(
            &cls,
            &obj,
            &bbox,
            &kps,
            8,
            0.5,
        );

        assert!(rostos.is_empty());

        cls[0][0] = 0.81;
        bbox[0][0] = f32::INFINITY;

        let rostos = decodificar(
            &cls,
            &obj,
            &bbox,
            &kps,
            8,
            0.5,
        );

        assert!(rostos.is_empty());
    }
}