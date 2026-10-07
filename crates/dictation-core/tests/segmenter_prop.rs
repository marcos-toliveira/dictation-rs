//! Propriedade central da segmentação: **nenhum sample é perdido**.
//!
//! Para qualquer combinação de tamanho de segmento, sobreposição e fluxo de entrada,
//! a união dos intervalos dos segmentos deve cobrir todo o áudio recebido.

use dictation_core::{Segment, Segmenter, SegmenterConfig};
use proptest::prelude::*;

proptest! {
    #[test]
    fn segmentation_is_lossless(
        seg in 1usize..200,
        ov in 0usize..50,
        chunks in prop::collection::vec(0usize..100, 1..20),
    ) {
        prop_assume!(ov < seg);
        // min = 0 garante que o flush emite o restante (sem descartar cauda)
        let cfg = SegmenterConfig { segment_samples: seg, overlap_samples: ov, min_samples: 0 };
        let mut s = Segmenter::new(cfg);
        let mut total = 0u64;
        let mut segments: Vec<Segment> = Vec::new();
        for c in &chunks {
            let data = vec![0i16; *c];
            total += *c as u64;
            segments.extend(s.push(&data));
        }
        if let Some(t) = s.flush() {
            segments.push(t);
        }

        let mut covered = vec![false; total as usize];
        for seg in &segments {
            for k in 0..seg.pcm.len() {
                let idx = seg.start_sample as usize + k;
                if idx < covered.len() {
                    covered[idx] = true;
                }
            }
        }
        prop_assert!(covered.iter().all(|c| *c), "todo sample deve pertencer a algum segmento");
    }
}
