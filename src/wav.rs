//! Cabecalho WAV para audio PCM cru, 16 bits mono (Otto, NexoDev T072)
pub fn wav(pcm: &[u8], taxa: u32) -> Vec<u8> {
    // 44 bytes de cabeçalho + dados PCM
    let mut v = Vec::with_capacity(44 + pcm.len());

    // RIFF
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36u32 + pcm.len() as u32).to_le_bytes());
    v.extend_from_slice(b"WAVE");

    // fmt subchunk
    v.extend_from_slice(b"fmt ");
    v.extend_from_slice(&16u32.to_le_bytes());          // tamanho do subchunk
    v.extend_from_slice(&1u16.to_le_bytes());           // PCM
    v.extend_from_slice(&1u16.to_le_bytes());           // mono
    v.extend_from_slice(&taxa.to_le_bytes());           // taxa de amostragem
    v.extend_from_slice(&(taxa * 2).to_le_bytes());     // byte rate (taxa * blockAlign)
    v.extend_from_slice(&2u16.to_le_bytes());           // block align (bytes por amostra)
    v.extend_from_slice(&16u16.to_le_bytes());          // bits por amostra

    // data subchunk
    v.extend_from_slice(b"data");
    v.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    v.extend_from_slice(pcm);

    v
}

pub fn segundos(wav: &[u8]) -> f64 {
    // validações básicas
    if wav.len() < 44 || &wav[0..4] != b"RIFF" {
        return 0.0;
    }

    // byte rate = bytes por segundo (offset 28..32)
    let byte_rate = wav.get(28..32).and_then(|b| <[u8; 4]>::try_from(b).ok()).map_or(0, u32::from_le_bytes);
    if byte_rate == 0 {
        return 0.0;
    }

    let data_len = wav.len() - 44;
    data_len as f64 / byte_rate as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic() {
        let w = wav(&[0u8; 48_000], 24_000);
        assert_eq!(w.len(), 48_044);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(&w[8..12], b"WAVE");
        assert_eq!(&w[36..40], b"data");
        assert_eq!(u32::from_le_bytes(w[4..8].try_into().unwrap()), 48_036);
        assert_eq!(u32::from_le_bytes(w[24..28].try_into().unwrap()), 24_000);
        assert_eq!(u32::from_le_bytes(w[40..44].try_into().unwrap()), 48_000);
        assert!((segundos(&w) - 1.0).abs() < 1e-9);
        assert_eq!(segundos(b"abc"), 0.0);
        assert_eq!(segundos(&[0u8; 100]), 0.0);
    }
}
