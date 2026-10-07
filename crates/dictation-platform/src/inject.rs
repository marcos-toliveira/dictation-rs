//! Injeção de texto no aplicativo em foco.
//!
//! * [`StdoutInjector`] — escreve na saída (testes / modo `stdout`).
//! * [`XdotoolInjector`] — X11, usando o `xdotool` (feature `x11`).

use crate::{InjectError, TextInjector};

/// Escreve o texto em `stdout` (útil para testes e para `inject = "stdout"`).
#[derive(Default)]
pub struct StdoutInjector;

impl TextInjector for StdoutInjector {
    fn inject(&self, text: &str) -> Result<(), InjectError> {
        println!("{text}");
        Ok(())
    }
}

/// Digita o texto no app em foco via `xdotool` (X11).
#[cfg(feature = "x11")]
pub struct XdotoolInjector {
    delay_ms: u32,
    trailing_space: bool,
}

#[cfg(feature = "x11")]
impl XdotoolInjector {
    pub fn new() -> Self {
        Self {
            delay_ms: 6,
            trailing_space: true,
        }
    }

    pub fn with_delay_ms(mut self, ms: u32) -> Self {
        self.delay_ms = ms;
        self
    }

    pub fn with_trailing_space(mut self, on: bool) -> Self {
        self.trailing_space = on;
        self
    }
}

#[cfg(feature = "x11")]
impl Default for XdotoolInjector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "x11")]
impl TextInjector for XdotoolInjector {
    fn inject(&self, text: &str) -> Result<(), InjectError> {
        let payload = if self.trailing_space {
            format!("{text} ")
        } else {
            text.to_string()
        };
        let status = std::process::Command::new("xdotool")
            .args([
                "type",
                "--clearmodifiers",
                "--delay",
                &self.delay_ms.to_string(),
                "--",
                &payload,
            ])
            .status()
            .map_err(|e| InjectError::Failed(format!("xdotool: {e}")))?;
        if !status.success() {
            return Err(InjectError::Failed(format!("xdotool saiu com {status}")));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stdout_injector_is_ok() {
        let inj = StdoutInjector;
        assert!(inj.inject("teste").is_ok());
    }
}
