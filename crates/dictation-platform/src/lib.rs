//! `dictation-platform` — fronteiras de plataforma.
//!
//! O núcleo não conhece SO. Aqui ficam os **traits** e as implementações concretas,
//! atrás de *feature flags* (`audio-cpal`, `x11`; `wayland`/`windows` virão depois).
//!
//! * [`AudioCapture`] — captura de áudio (cpal: Linux + Windows; independente do display server).
//! * [`TextInjector`] — digita o texto no app em foco (X11 via `xdotool`; Wayland/Windows depois).
//! * [`OverlayHost`]/[`TrayHost`]/[`HotkeyProvider`] — pontos de extensão para a UI e atalhos.

pub mod inject;
pub mod mocks;

#[cfg(feature = "audio-cpal")]
pub mod capture;

/// Erros de captura de áudio.
#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("dispositivo de áudio indisponível: {0}")]
    Device(String),
    #[error("erro de captura: {0}")]
    Stream(String),
}

/// Fonte de áudio: entrega blocos mono `i16` a 16 kHz.
///
/// Obs.: não é `Send` de propósito — o `cpal::Stream` deve ser criado e usado na
/// **mesma thread** (o daemon sobe uma thread dedicada à captura).
pub trait AudioCapture {
    /// Abre o dispositivo e começa a capturar.
    fn start(&mut self) -> Result<(), CaptureError>;
    /// Bloqueia até o próximo bloco de samples (ou `None` no fim).
    fn recv(&mut self) -> Option<Vec<i16>>;
    /// Encerra a captura.
    fn stop(&mut self) -> Result<(), CaptureError>;
}

/// Erros de injeção de texto.
#[derive(Debug, thiserror::Error)]
pub enum InjectError {
    #[error("falha ao injetar texto: {0}")]
    Failed(String),
}

/// Digita o texto no aplicativo em foco.
pub trait TextInjector: Send + Sync {
    fn inject(&self, text: &str) -> Result<(), InjectError>;
}

/// Ponto de extensão: indicador/prévia flutuante (X11 hoje; Wayland/Windows depois).
pub trait OverlayHost: Send {
    fn show_recording(&mut self);
    fn show_preview(&mut self, text: &str);
    fn hide(&mut self);
}

/// Ponto de extensão: ícone na bandeja.
pub trait TrayHost: Send {
    fn set_recording(&mut self, recording: bool);
}

/// Ponto de extensão: atalho global nativo.
pub trait HotkeyProvider: Send {
    fn register(&mut self, hotkey: &str) -> Result<(), String>;
}
