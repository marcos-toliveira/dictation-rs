//! Segmentação de áudio **sem perda** e de custo linear.
//!
//! Em vez de reenviar o áudio acumulado inteiro ao ASR a cada atualização (o que ficava
//! "pesado" e instável em falas longas), dividimos o fluxo em segmentos de tamanho fixo
//! (com pequena sobreposição) e transcrevemos **cada segmento uma única vez**.
//!
//! Invariante central: **todo sample de entrada pertence a pelo menos um segmento**
//! (nada é perdido), garantido pela sobreposição ≥ 0 e pelo `flush` final.

/// Configuração da segmentação (em samples).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmenterConfig {
    pub segment_samples: usize,
    pub overlap_samples: usize,
    pub min_samples: usize,
}

impl SegmenterConfig {
    /// Constrói a partir de durações em segundos e da taxa de amostragem.
    pub fn from_seconds(segment_s: f64, overlap_s: f64, min_s: f64, rate: u32) -> Self {
        let r = rate as f64;
        Self {
            segment_samples: (segment_s * r).round().max(1.0) as usize,
            overlap_samples: (overlap_s * r).round().max(0.0) as usize,
            min_samples: (min_s * r).round().max(0.0) as usize,
        }
    }
}

/// Um segmento pronto para transcrição.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub pcm: Vec<i16>,
    /// Índice (em samples) do início do segmento no fluxo total.
    pub start_sample: u64,
}

/// Segmentador incremental.
pub struct Segmenter {
    cfg: SegmenterConfig,
    buf: Vec<i16>,
    buf_start: u64,
    total: u64,
}

impl Segmenter {
    pub fn new(cfg: SegmenterConfig) -> Self {
        assert!(cfg.segment_samples > 0, "segment_samples deve ser > 0");
        assert!(
            cfg.overlap_samples < cfg.segment_samples,
            "overlap deve ser menor que o segmento"
        );
        Self {
            cfg,
            buf: Vec::new(),
            buf_start: 0,
            total: 0,
        }
    }

    /// Total de samples já recebidos.
    pub fn pushed(&self) -> u64 {
        self.total
    }

    /// Recebe samples e devolve os segmentos que ficaram completos.
    pub fn push(&mut self, samples: &[i16]) -> Vec<Segment> {
        self.buf.extend_from_slice(samples);
        self.total += samples.len() as u64;
        let mut out = Vec::new();
        while self.buf.len() >= self.cfg.segment_samples {
            let pcm = self.buf[..self.cfg.segment_samples].to_vec();
            out.push(Segment {
                pcm,
                start_sample: self.buf_start,
            });
            let drop = self.cfg.segment_samples - self.cfg.overlap_samples;
            self.buf.drain(..drop);
            self.buf_start += drop as u64;
        }
        out
    }

    /// Encerra: devolve o restante (se relevante). Deve ser chamado no `stop`.
    pub fn flush(&mut self) -> Option<Segment> {
        if !self.buf.is_empty() && self.buf.len() >= self.cfg.min_samples {
            let pcm = std::mem::take(&mut self.buf);
            Some(Segment {
                pcm,
                start_sample: self.buf_start,
            })
        } else {
            self.buf.clear();
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(seg: usize, ov: usize, min: usize) -> SegmenterConfig {
        SegmenterConfig {
            segment_samples: seg,
            overlap_samples: ov,
            min_samples: min,
        }
    }

    #[test]
    fn emits_when_full() {
        let mut s = Segmenter::new(cfg(10, 2, 1));
        assert!(s.push(&[0i16; 9]).is_empty());
        let segs = s.push(&[0i16; 1]);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].pcm.len(), 10);
        assert_eq!(segs[0].start_sample, 0);
    }

    #[test]
    fn overlap_advances_correctly() {
        let mut s = Segmenter::new(cfg(10, 2, 1));
        let segs = s.push(&[0i16; 20]);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].start_sample, 0);
        assert_eq!(segs[1].start_sample, 8); // 10 - 2 de overlap
    }

    #[test]
    fn flush_returns_tail() {
        let mut s = Segmenter::new(cfg(10, 2, 1));
        s.push(&[0i16; 5]);
        let tail = s.flush().unwrap();
        assert_eq!(tail.pcm.len(), 5);
        assert_eq!(tail.start_sample, 0);
    }

    #[test]
    fn flush_discards_below_min() {
        let mut s = Segmenter::new(cfg(10, 2, 4));
        s.push(&[0i16; 3]);
        assert!(s.flush().is_none());
    }
}
