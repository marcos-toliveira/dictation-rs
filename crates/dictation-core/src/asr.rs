//! Abstração do motor de transcrição (ASR) + um mock para testes.
//!
//! O `dictation-core` não conhece HTTP nem Groq: ele só define o contrato. A
//! implementação concreta (Groq) vive em `dictation-groq`, e o mock permite testar
//! o pipeline inteiro sem rede.

use std::collections::VecDeque;
use std::sync::Mutex;

#[derive(Debug, Clone, Default)]
pub struct AsrOptions {
    pub model: String,
    pub language: Option<String>,
    pub prompt: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum AsrError {
    #[error("falha de rede: {0}")]
    Network(String),
    #[error("HTTP {status}: {body}")]
    Http { status: u16, body: String },
    #[error("resposta inválida: {0}")]
    Decode(String),
}

/// Contrato do motor de transcrição.
pub trait AsrEngine: Send + Sync {
    /// Transcreve um WAV completo e devolve o texto.
    fn transcribe(&self, wav: &[u8], opts: &AsrOptions) -> Result<String, AsrError>;
}

/// ASR de teste: devolve respostas pré-programadas e registra o tamanho de cada áudio recebido.
pub struct MockAsr {
    queue: Mutex<VecDeque<Result<String, AsrError>>>,
    calls: Mutex<Vec<usize>>,
}

impl MockAsr {
    pub fn new(responses: Vec<Result<String, AsrError>>) -> Self {
        Self {
            queue: Mutex::new(responses.into()),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// Tamanhos (em bytes) de cada WAV transcrito, na ordem.
    pub fn calls(&self) -> Vec<usize> {
        self.calls.lock().unwrap().clone()
    }
}

impl AsrEngine for MockAsr {
    fn transcribe(&self, wav: &[u8], _opts: &AsrOptions) -> Result<String, AsrError> {
        self.calls.lock().unwrap().push(wav.len());
        self.queue
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Err(AsrError::Decode("sem resposta programada".into())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_returns_queued_and_records_sizes() {
        let m = MockAsr::new(vec![Ok("um".into()), Err(AsrError::Network("x".into()))]);
        let o = AsrOptions::default();
        assert_eq!(m.transcribe(b"abc", &o).unwrap(), "um");
        assert!(matches!(m.transcribe(b"de", &o), Err(AsrError::Network(_))));
        assert_eq!(m.calls(), vec![3, 2]);
    }
}
