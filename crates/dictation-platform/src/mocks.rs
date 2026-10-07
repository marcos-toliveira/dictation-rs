//! Implementações de teste (mocks) para os traits de plataforma.

use crate::{AudioCapture, CaptureError, InjectError, TextInjector};
use std::sync::{Arc, Mutex};

/// Captura "de mentira": entrega blocos pré-programados.
#[derive(Default)]
pub struct MockCapture {
    started: bool,
    blocks: std::collections::VecDeque<Vec<i16>>,
}

impl MockCapture {
    pub fn new(blocks: Vec<Vec<i16>>) -> Self {
        Self {
            started: false,
            blocks: blocks.into(),
        }
    }
}

impl AudioCapture for MockCapture {
    fn start(&mut self) -> Result<(), CaptureError> {
        self.started = true;
        Ok(())
    }

    fn recv(&mut self) -> Option<Vec<i16>> {
        self.blocks.pop_front()
    }

    fn stop(&mut self) -> Result<(), CaptureError> {
        self.started = false;
        Ok(())
    }
}

/// Injetor "de mentira": registra o que foi injetado (para asserts).
#[derive(Default, Clone)]
pub struct MockInjector {
    injected: Arc<Mutex<Vec<String>>>,
}

impl MockInjector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Textos injetados, na ordem.
    pub fn injected(&self) -> Vec<String> {
        self.injected.lock().unwrap().clone()
    }

    pub fn count(&self) -> usize {
        self.injected.lock().unwrap().len()
    }
}

impl TextInjector for MockInjector {
    fn inject(&self, text: &str) -> Result<(), InjectError> {
        self.injected.lock().unwrap().push(text.to_string());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_capture_yields_blocks() {
        let mut c = MockCapture::new(vec![vec![1, 2], vec![3]]);
        c.start().unwrap();
        assert_eq!(c.recv(), Some(vec![1, 2]));
        assert_eq!(c.recv(), Some(vec![3]));
        assert_eq!(c.recv(), None);
    }

    #[test]
    fn mock_injector_records() {
        let inj = MockInjector::new();
        inj.inject("olá").unwrap();
        assert_eq!(inj.count(), 1);
        assert_eq!(inj.injected(), vec!["olá".to_string()]);
    }
}
