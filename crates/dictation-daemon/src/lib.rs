//! `dictation-daemon` — motor que liga o [`Pipeline`] (core) ao ASR e à injeção.
//!
//! O [`Engine`] é puro em relação a I/O de plataforma: recebe um [`AsrEngine`] e um
//! [`TextInjector`], então é testável com mocks. É aqui que os invariantes do core
//! viram comportamento observável: **uma injeção por sessão** e **texto monotônico**.

use dictation_core::{
    audio, AsrEngine, AsrOptions, Config, Corrections, Pipeline, SegmenterConfig, SessionId,
};
use dictation_platform::TextInjector;
use std::sync::Arc;

/// Orquestra captura → segmentação → transcrição → injeção para uma configuração.
pub struct Engine {
    pipeline: Pipeline,
    asr: Arc<dyn AsrEngine>,
    injector: Arc<dyn TextInjector>,
    opts: AsrOptions,
    corrections: Corrections,
}

impl Engine {
    pub fn new(cfg: &Config, asr: Arc<dyn AsrEngine>, injector: Arc<dyn TextInjector>) -> Self {
        let seg = SegmenterConfig::from_seconds(
            cfg.segment_seconds,
            cfg.overlap_seconds,
            cfg.min_segment_seconds,
            audio::SAMPLE_RATE,
        );
        let prompt = std::fs::read_to_string(&cfg.vocab)
            .ok()
            .map(|s| build_prompt(&s))
            .filter(|p| !p.is_empty());
        let corrections = Corrections::from_file(&cfg.corrections).unwrap_or_default();
        Self {
            pipeline: Pipeline::new(seg),
            asr,
            injector,
            opts: AsrOptions {
                model: cfg.model.clone(),
                language: Some(cfg.language.clone()),
                prompt,
            },
            corrections,
        }
    }

    pub fn is_recording(&self) -> bool {
        self.pipeline.is_recording()
    }

    pub fn session_id(&self) -> Option<SessionId> {
        self.pipeline.session_id()
    }

    /// Inicia uma sessão (falha se já houver gravação).
    pub fn start(&mut self) -> Result<SessionId, String> {
        self.pipeline.start().map_err(|e| e.to_string())
    }

    /// Recebe áudio: segmenta e transcreve o que estiver pronto. Retorna quantos
    /// segmentos foram transcritos.
    pub fn push_audio(&mut self, samples: &[i16]) -> usize {
        let ready = self.pipeline.push_audio(samples);
        let n = ready.len();
        for (id, seg) in ready {
            let wav = audio::raw_to_wav(
                &seg.pcm,
                audio::SAMPLE_RATE,
                audio::CHANNELS,
                audio::BITS_PER_SAMPLE,
            );
            if let Ok(text) = self.asr.transcribe(&wav, &self.opts) {
                self.pipeline.accept(id, &text);
            }
        }
        n
    }

    /// Encerra a sessão: transcreve a cauda, finaliza e **injeta uma única vez**.
    /// Retorna o texto injetado (ou `None` se vazio).
    pub fn stop_and_finish(&mut self) -> Option<String> {
        if let Ok(Some((id, seg))) = self.pipeline.stop() {
            let wav = audio::raw_to_wav(
                &seg.pcm,
                audio::SAMPLE_RATE,
                audio::CHANNELS,
                audio::BITS_PER_SAMPLE,
            );
            if let Ok(text) = self.asr.transcribe(&wav, &self.opts) {
                self.pipeline.accept(id, &text);
            }
        }
        match self.pipeline.finish() {
            Ok(Some(text)) => {
                let corrected = self.corrections.apply(&text);
                if let Err(e) = self.injector.inject(&corrected) {
                    tracing::error!(error = %e, "falha ao injetar texto");
                }
                Some(corrected)
            }
            _ => None,
        }
    }
}

/// Monta o `prompt` de vocabulário a partir do arquivo (linhas não-comentário, vírgula).
pub fn build_prompt(vocab: &str) -> String {
    let terms: Vec<&str> = vocab
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    let joined = terms.join(", ");
    joined.chars().take(800).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use dictation_core::MockAsr;
    use dictation_platform::mocks::MockInjector;

    fn cfg() -> Config {
        Config {
            segment_seconds: 1.0,
            overlap_seconds: 0.0,
            min_segment_seconds: 0.0,
            ..Config::default()
        }
    }

    #[test]
    fn long_audio_transcribes_all_segments_and_injects_once() {
        // 2.5s de áudio, segmentos de 1s => 2 segmentos + cauda = 3 chamadas
        let asr = Arc::new(MockAsr::new(vec![
            Ok("primeira".into()),
            Ok("segunda".into()),
            Ok("terceira".into()),
        ]));
        let inj = Arc::new(MockInjector::new());
        let mut engine = Engine::new(&cfg(), asr.clone(), inj.clone());
        engine.start().unwrap();

        let samples = vec![0i16; (audio::SAMPLE_RATE as usize * 5) / 2]; // 2,5s
        let n = engine.push_audio(&samples);
        assert_eq!(n, 2, "dois segmentos de 1s");

        let text = engine.stop_and_finish().unwrap();
        assert_eq!(text, "primeira segunda terceira");
        assert_eq!(inj.count(), 1, "injeção deve ocorrer exatamente uma vez");
        assert_eq!(asr.calls().len(), 3);
    }

    #[test]
    fn empty_session_does_not_inject() {
        let asr = Arc::new(MockAsr::new(vec![]));
        let inj = Arc::new(MockInjector::new());
        let mut engine = Engine::new(&cfg(), asr, inj.clone());
        engine.start().unwrap();
        assert!(engine.stop_and_finish().is_none());
        assert_eq!(inj.count(), 0);
    }

    #[test]
    fn applies_corrections_before_injecting() {
        let mut c = cfg();
        let path = std::env::temp_dir().join(format!(
            "dictation-corrections-test-{}.tsv",
            std::process::id()
        ));
        std::fs::write(&path, "open code\tOpenCode\n").unwrap();
        c.corrections = path.clone();

        let asr = Arc::new(MockAsr::new(vec![Ok("open code".into())]));
        let inj = Arc::new(MockInjector::new());
        let mut engine = Engine::new(&c, asr, inj.clone());
        engine.start().unwrap();
        engine.push_audio(&vec![0i16; audio::SAMPLE_RATE as usize + 1]);
        engine.stop_and_finish();
        assert_eq!(inj.injected(), vec!["OpenCode".to_string()]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn build_prompt_joins_terms() {
        let p = build_prompt("# comentário\nOpenCode\nTulinho\n\nGAQL\n");
        assert_eq!(p, "OpenCode, Tulinho, GAQL");
    }
}
