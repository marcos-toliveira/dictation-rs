//! Áudio: empacotamento PCM → WAV (para o ASR) e leitura de WAV (para testes).

/// Taxa de amostragem padrão do pipeline (Whisper usa 16 kHz mono).
pub const SAMPLE_RATE: u32 = 16_000;
pub const CHANNELS: u16 = 1;
pub const BITS_PER_SAMPLE: u16 = 16;

/// Empacota PCM `i16` (little-endian) num contêiner WAV mono.
pub fn raw_to_wav(pcm: &[i16], rate: u32, channels: u16, bits: u16) -> Vec<u8> {
    let bytes_per_sample = (bits / 8) as usize;
    let data_len = (pcm.len() * bytes_per_sample) as u32;
    let byte_rate = rate * channels as u32 * (bits as u32 / 8);
    let block_align = channels * (bits / 8);

    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // tamanho do chunk fmt
    out.extend_from_slice(&1u16.to_le_bytes()); // formato PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in pcm {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// Lê o chunk `data` de um WAV e devolve os samples `i16` (usado em testes).
pub fn wav_to_pcm(wav: &[u8]) -> Option<Vec<i16>> {
    if wav.len() < 12 || &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return None;
    }
    let mut i = 12;
    while i + 8 <= wav.len() {
        let id = &wav[i..i + 4];
        let size = u32::from_le_bytes([wav[i + 4], wav[i + 5], wav[i + 6], wav[i + 7]]) as usize;
        if id == b"data" {
            let start = i + 8;
            let end = (start + size).min(wav.len());
            let mut pcm = Vec::with_capacity((end - start) / 2);
            let mut j = start;
            while j + 2 <= end {
                pcm.push(i16::from_le_bytes([wav[j], wav[j + 1]]));
                j += 2;
            }
            return Some(pcm);
        }
        i += 8 + size + (size & 1);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_fields_are_correct() {
        let pcm = vec![0i16; 100];
        let wav = raw_to_wav(&pcm, SAMPLE_RATE, CHANNELS, BITS_PER_SAMPLE);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(
            u32::from_le_bytes([wav[4], wav[5], wav[6], wav[7]]),
            36 + 200
        );
        assert_eq!(
            u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]),
            200
        );
    }

    #[test]
    fn round_trip() {
        let pcm: Vec<i16> = (0..1000).map(|i| (i as i16).wrapping_mul(37)).collect();
        let wav = raw_to_wav(&pcm, SAMPLE_RATE, CHANNELS, BITS_PER_SAMPLE);
        assert_eq!(wav_to_pcm(&wav).unwrap(), pcm);
    }

    #[test]
    fn rejects_non_wav() {
        assert!(wav_to_pcm(b"not a wav").is_none());
    }
}
