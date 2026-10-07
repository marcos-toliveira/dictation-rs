//! Estado compartilhado entre o daemon (threads) e o overlay (thread da UI).

use std::sync::{Arc, Mutex};

/// O que o overlay precisa saber a cada quadro.
#[derive(Debug, Default, Clone)]
pub struct UiState {
    /// Está gravando? (mostra o badge REC)
    pub recording: bool,
    /// Está transcrevendo (entre o F8 e a injeção)? (mostra "transcrevendo…")
    pub transcribing: bool,
    /// Texto da prévia ao vivo (comprometido + cauda).
    pub preview: String,
}

/// Handle compartilhado (`Arc<Mutex<..>>`).
pub type SharedUi = Arc<Mutex<UiState>>;

/// Cria o estado compartilhado.
pub fn shared() -> SharedUi {
    Arc::new(Mutex::new(UiState::default()))
}
