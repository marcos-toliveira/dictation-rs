//! `dictation-daemon` — motor que liga o [`Pipeline`] (core) ao ASR e à injeção.
//!
//! O [`Engine`] é puro em relação a I/O de plataforma: recebe um [`AsrEngine`] e um
//! [`TextInjector`], então é testável com mocks. É aqui que os invariantes do core
//! viram comportamento observável: **uma injeção por sessão** e **texto monotônico**.

use dictation_core::{
    audio, AsrEngine, AsrError, AsrOptions, Config, Corrections, Pipeline, SegmenterConfig,
    SessionId,
};
use dictation_platform::TextInjector;
use std::path::PathBuf;
use std::process::Command;
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

    /// Opções de ASR em uso (para a prévia reusar o mesmo modelo/idioma/prompt).
    pub fn options(&self) -> AsrOptions {
        self.opts.clone()
    }

    /// Texto já comprometido (segmentos transcritos).
    pub fn committed_text(&self) -> String {
        self.pipeline.committed_text()
    }

    /// Cauda de áudio (WAV) ainda não transcrita — para a prévia ao vivo.
    /// Só devolve algo a partir de ~0,5s (evita chamadas inúteis ao ASR).
    pub fn pending_wav(&self) -> Option<Vec<u8>> {
        let pcm = self.pipeline.pending_pcm();
        if pcm.len() < (audio::SAMPLE_RATE as usize) / 2 {
            return None;
        }
        Some(audio::raw_to_wav(
            &pcm,
            audio::SAMPLE_RATE,
            audio::CHANNELS,
            audio::BITS_PER_SAMPLE,
        ))
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

/// ASR local (fallback offline) via `whisper.cpp` (`whisper-cli`).
pub struct LocalWhisper {
    bin: PathBuf,
    model: PathBuf,
    language: String,
    threads: u32,
    work_dir: PathBuf,
}

impl LocalWhisper {
    pub fn new(
        bin: PathBuf,
        model: PathBuf,
        language: String,
        threads: u32,
        work_dir: PathBuf,
    ) -> Self {
        Self {
            bin,
            model,
            language,
            threads,
            work_dir,
        }
    }
}

impl AsrEngine for LocalWhisper {
    fn transcribe(&self, wav: &[u8], opts: &AsrOptions) -> Result<String, AsrError> {
        std::fs::create_dir_all(&self.work_dir).map_err(|e| AsrError::Network(e.to_string()))?;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = self
            .work_dir
            .join(format!("tmp-{}-{nanos}.wav", std::process::id()));
        std::fs::write(&path, wav).map_err(|e| AsrError::Network(e.to_string()))?;

        let lang = if self.language.is_empty() {
            "auto".to_string()
        } else {
            self.language.clone()
        };
        let mut cmd = Command::new(&self.bin);
        cmd.args([
            "-m",
            &self.model.to_string_lossy(),
            "-f",
            &path.to_string_lossy(),
            "-l",
            &lang,
            "-t",
            &self.threads.to_string(),
            "-nt",
            "-np",
            "-bs",
            "1",
        ]);
        if let Some(prompt) = opts.prompt.as_deref().filter(|p| !p.is_empty()) {
            cmd.args(["--prompt", prompt]);
        }
        let out = cmd.output();
        let _ = std::fs::remove_file(&path);

        match out {
            Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
                .trim()
                .to_string()),
            Ok(o) => Err(AsrError::Network(format!(
                "whisper-cli falhou: {}",
                String::from_utf8_lossy(&o.stderr)
                    .chars()
                    .take(200)
                    .collect::<String>()
            ))),
            Err(e) => Err(AsrError::Network(e.to_string())),
        }
    }
}

/// ASR com fallback: tenta o primário (Groq) e, em erro, usa o secundário (local).
pub struct FallbackAsr {
    primary: Arc<dyn AsrEngine>,
    fallback: Arc<dyn AsrEngine>,
}

impl FallbackAsr {
    pub fn new(primary: Arc<dyn AsrEngine>, fallback: Arc<dyn AsrEngine>) -> Self {
        Self { primary, fallback }
    }
}

impl AsrEngine for FallbackAsr {
    fn transcribe(&self, wav: &[u8], opts: &AsrOptions) -> Result<String, AsrError> {
        match self.primary.transcribe(wav, opts) {
            Ok(t) => Ok(t),
            Err(e) => {
                tracing::warn!(error = %e, "ASR primário falhou; caindo para o local");
                self.fallback.transcribe(wav, opts)
            }
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

/// Extrai a origem de uma regra de correção (`errado<TAB>certo` ou `errado=>certo`).
fn rule_source(line: &str) -> Option<&str> {
    if let Some((a, _)) = line.split_once('\t') {
        Some(a.trim())
    } else {
        line.split_once("=>").map(|(a, _)| a.trim())
    }
}

/// Ensina uma correção: grava `errado<TAB>certo` (substituindo regra anterior) e,
/// opcionalmente, adiciona o termo correto ao vocabulário.
pub fn teach(cfg: &Config, wrong: &str, right: &str, add_vocab: bool) -> std::io::Result<()> {
    if let Some(parent) = cfg.corrections.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut lines: Vec<String> = if cfg.corrections.is_file() {
        std::fs::read_to_string(&cfg.corrections)?
            .lines()
            .map(String::from)
            .collect()
    } else {
        Vec::new()
    };
    lines.retain(|l| {
        rule_source(l)
            .map(|s| !s.eq_ignore_ascii_case(wrong))
            .unwrap_or(true)
    });
    lines.push(format!("{wrong}\t{right}"));
    std::fs::write(&cfg.corrections, lines.join("\n") + "\n")?;

    if add_vocab {
        let mut terms: Vec<String> = if cfg.vocab.is_file() {
            std::fs::read_to_string(&cfg.vocab)?
                .lines()
                .map(String::from)
                .collect()
        } else {
            Vec::new()
        };
        if !terms.iter().any(|t| t.trim() == right) {
            terms.push(right.to_string());
            std::fs::write(&cfg.vocab, terms.join("\n") + "\n")?;
        }
    }
    Ok(())
}

/// Salva a última transcrição em `state_dir/last.txt`.
pub fn save_last(cfg: &Config, text: &str) {
    let _ = std::fs::create_dir_all(&cfg.state_dir);
    let _ = std::fs::write(cfg.state_dir.join("last.txt"), text);
}

/// Lê a última transcrição.
pub fn read_last(cfg: &Config) -> String {
    std::fs::read_to_string(cfg.state_dir.join("last.txt")).unwrap_or_default()
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
        let samples = [0i16; audio::SAMPLE_RATE as usize + 1];
        engine.push_audio(&samples);
        engine.stop_and_finish();
        assert_eq!(inj.injected(), vec!["OpenCode".to_string()]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn pending_wav_only_after_half_second() {
        let asr = Arc::new(MockAsr::new(vec![]));
        let inj = Arc::new(MockInjector::new());
        let mut engine = Engine::new(&cfg(), asr, inj);
        engine.start().unwrap();
        engine.push_audio(&[0i16; 100]); // pouca coisa
        assert!(engine.pending_wav().is_none());
        let half = [0i16; (audio::SAMPLE_RATE as usize) / 2 + 1];
        engine.push_audio(&half);
        assert!(engine.pending_wav().is_some());
    }

    #[test]
    fn fallback_uses_secondary_on_primary_error() {
        let primary = Arc::new(MockAsr::new(vec![Err(AsrError::Network(
            "sem rede".into(),
        ))]));
        let fallback = Arc::new(MockAsr::new(vec![Ok("local".into())]));
        let asr = FallbackAsr::new(primary, fallback);
        let text = asr.transcribe(b"x", &AsrOptions::default()).unwrap();
        assert_eq!(text, "local");
    }

    #[test]
    fn teach_writes_and_replaces_rule() {
        let dir = std::env::temp_dir().join(format!("dictation-teach-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let c = Config {
            corrections: dir.join("corrections.tsv"),
            vocab: dir.join("vocab.txt"),
            ..Config::default()
        };
        teach(&c, "teh", "the", true).unwrap();
        teach(&c, "teh", "THE", false).unwrap(); // substitui
        let content = std::fs::read_to_string(&c.corrections).unwrap();
        assert_eq!(
            content.matches("teh").count(),
            1,
            "regra deve ser substituída"
        );
        assert!(content.contains("teh\tTHE"));
        let vocab = std::fs::read_to_string(&c.vocab).unwrap();
        assert!(vocab.contains("the"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_and_read_last() {
        let dir = std::env::temp_dir().join(format!("dictation-last-{}", std::process::id()));
        let c = Config {
            state_dir: dir.clone(),
            ..Config::default()
        };
        save_last(&c, "olá mundo");
        assert_eq!(read_last(&c), "olá mundo");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn build_prompt_joins_terms() {
        let p = build_prompt("# comentário\nOpenCode\nTulinho\n\nGAQL\n");
        assert_eq!(p, "OpenCode, Tulinho, GAQL");
    }
}
