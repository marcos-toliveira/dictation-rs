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
    initial_delay_ms: u32,
    trailing_space: bool,
}

#[cfg(feature = "x11")]
impl XdotoolInjector {
    pub fn new() -> Self {
        Self {
            delay_ms: 12,
            initial_delay_ms: 150,
            trailing_space: true,
        }
    }

    pub fn with_delay_ms(mut self, ms: u32) -> Self {
        self.delay_ms = ms;
        self
    }

    /// Espera antes de começar a digitar (dá tempo ao app alvo ganhar foco/fica pronto).
    pub fn with_initial_delay_ms(mut self, ms: u32) -> Self {
        self.initial_delay_ms = ms;
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
impl XdotoolInjector {
    /// Digita um trecho via `xdotool type` (ASCII).
    fn type_ascii(&self, s: &str) -> Result<(), InjectError> {
        if s.is_empty() {
            return Ok(());
        }
        let status = std::process::Command::new("xdotool")
            .args([
                "type",
                "--clearmodifiers",
                "--delay",
                &self.delay_ms.to_string(),
                "--",
                s,
            ])
            .status()
            .map_err(|e| InjectError::Failed(format!("xdotool type: {e}")))?;
        if !status.success() {
            return Err(InjectError::Failed(format!(
                "xdotool type saiu com {status}"
            )));
        }
        Ok(())
    }

    /// Digita um caractere não-ASCII pelo keysym Unicode (`xdotool key Uxxxx`).
    fn key_unicode(&self, ch: char) -> Result<(), InjectError> {
        let key = format!("U{:04X}", ch as u32);
        let status = std::process::Command::new("xdotool")
            .args(["key", "--clearmodifiers", &key])
            .status()
            .map_err(|e| InjectError::Failed(format!("xdotool key: {e}")))?;
        if !status.success() {
            return Err(InjectError::Failed(format!(
                "xdotool key saiu com {status}"
            )));
        }
        Ok(())
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
        // Dá tempo ao app alvo ganhar foco/ficar pronto (evita perder o começo).
        if self.initial_delay_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(
                self.initial_delay_ms as u64,
            ));
        }
        // `xdotool type` PULA teclas mortas (á, ã, ç, ê) no layout `br`: enviamos
        // ASCII por `type` e cada acento por `key Uxxxx`.
        // Em apps Chromium/Electron isto ainda pode perder caractere — para esses,
        // o modo confiável é `inject = clipboard`.
        let mut ascii = String::new();
        for ch in payload.chars() {
            if ch.is_ascii() {
                ascii.push(ch);
            } else {
                self.type_ascii(&ascii)?;
                ascii.clear();
                self.key_unicode(ch)?;
            }
        }
        self.type_ascii(&ascii)?;
        Ok(())
    }
}

/// Injeta **colando da área de transferência** (`xclip` + Ctrl+V).
///
/// Mais confiável que digitar: o texto vai inteiro e exato (inclusive acentos),
/// sem depender das "teclas mortas" do layout. Use `inject = clipboard`.
#[cfg(feature = "x11")]
pub struct ClipboardInjector {
    paste_key: String,
    initial_delay_ms: u32,
    trailing_space: bool,
}

#[cfg(feature = "x11")]
impl ClipboardInjector {
    pub fn new() -> Self {
        Self {
            paste_key: "ctrl+v".into(),
            initial_delay_ms: 120,
            trailing_space: true,
        }
    }

    /// Tecla de colar (padrão `ctrl+v`; terminais costumam usar `ctrl+shift+v`).
    pub fn with_paste_key(mut self, key: impl Into<String>) -> Self {
        self.paste_key = key.into();
        self
    }

    pub fn with_trailing_space(mut self, on: bool) -> Self {
        self.trailing_space = on;
        self
    }
}

#[cfg(feature = "x11")]
impl Default for ClipboardInjector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "x11")]
impl TextInjector for ClipboardInjector {
    fn inject(&self, text: &str) -> Result<(), InjectError> {
        use std::io::Write;
        let payload = if self.trailing_space {
            format!("{text} ")
        } else {
            text.to_string()
        };
        // 1) copia o texto para a área de transferência
        let mut child = std::process::Command::new("xclip")
            .args(["-selection", "clipboard", "-in"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| InjectError::Failed(format!("xclip: {e}")))?;
        child
            .stdin
            .as_mut()
            .ok_or_else(|| InjectError::Failed("xclip sem stdin".into()))?
            .write_all(payload.as_bytes())
            .map_err(|e| InjectError::Failed(format!("xclip write: {e}")))?;
        drop(child.stdin.take());
        let _ = child.wait();

        // 2) cola no app em foco
        if self.initial_delay_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(
                self.initial_delay_ms as u64,
            ));
        }
        let status = std::process::Command::new("xdotool")
            .args(["key", "--clearmodifiers", &self.paste_key])
            .status()
            .map_err(|e| InjectError::Failed(format!("xdotool key: {e}")))?;
        if !status.success() {
            return Err(InjectError::Failed(format!(
                "xdotool key saiu com {status}"
            )));
        }
        Ok(())
    }
}

/// Digita o texto no aplicativo em foco via `SendInput` (com `KEYEVENTF_UNICODE`).
///
/// Não depende de teclas mortas do teclado e preserva acentos/Unicode exatamente.
#[cfg(windows)]
pub struct WindowsTypeInjector {
    delay_ms: u32,
    initial_delay_ms: u32,
    trailing_space: bool,
}

#[cfg(windows)]
impl WindowsTypeInjector {
    pub fn new() -> Self {
        Self {
            delay_ms: 6,
            initial_delay_ms: 100,
            trailing_space: true,
        }
    }

    pub fn with_delay_ms(mut self, ms: u32) -> Self {
        self.delay_ms = ms;
        self
    }

    pub fn with_initial_delay_ms(mut self, ms: u32) -> Self {
        self.initial_delay_ms = ms;
        self
    }

    pub fn with_trailing_space(mut self, on: bool) -> Self {
        self.trailing_space = on;
        self
    }
}

#[cfg(windows)]
impl Default for WindowsTypeInjector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(windows)]
impl TextInjector for WindowsTypeInjector {
    fn inject(&self, text: &str) -> Result<(), InjectError> {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
            KEYEVENTF_UNICODE, VIRTUAL_KEY,
        };

        let payload = if self.trailing_space {
            format!("{text} ")
        } else {
            text.to_string()
        };

        if self.initial_delay_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(
                self.initial_delay_ms as u64,
            ));
        }

        let mut utf16_buf = [0u16; 2];
        for ch in payload.chars() {
            let units = ch.encode_utf16(&mut utf16_buf);
            for &u in &*units {
                let inputs = [
                    INPUT {
                        r#type: INPUT_KEYBOARD,
                        Anonymous: INPUT_0 {
                            ki: KEYBDINPUT {
                                wVk: VIRTUAL_KEY(0),
                                wScan: u,
                                dwFlags: KEYEVENTF_UNICODE,
                                time: 0,
                                dwExtraInfo: 0,
                            },
                        },
                    },
                    INPUT {
                        r#type: INPUT_KEYBOARD,
                        Anonymous: INPUT_0 {
                            ki: KEYBDINPUT {
                                wVk: VIRTUAL_KEY(0),
                                wScan: u,
                                dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                                time: 0,
                                dwExtraInfo: 0,
                            },
                        },
                    },
                ];

                let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
                if sent != inputs.len() as u32 {
                    return Err(InjectError::Failed("SendInput falhou".into()));
                }

                if self.delay_ms > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(self.delay_ms as u64));
                }
            }
        }
        Ok(())
    }
}

/// Injeta colando da área de transferência (`arboard` + Ctrl+V via `SendInput`).
#[cfg(windows)]
pub struct WindowsClipboardInjector {
    initial_delay_ms: u32,
    trailing_space: bool,
}

#[cfg(windows)]
impl WindowsClipboardInjector {
    pub fn new() -> Self {
        Self {
            initial_delay_ms: 100,
            trailing_space: true,
        }
    }

    pub fn with_initial_delay_ms(mut self, ms: u32) -> Self {
        self.initial_delay_ms = ms;
        self
    }

    pub fn with_trailing_space(mut self, on: bool) -> Self {
        self.trailing_space = on;
        self
    }
}

#[cfg(windows)]
impl Default for WindowsClipboardInjector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(windows)]
impl TextInjector for WindowsClipboardInjector {
    fn inject(&self, text: &str) -> Result<(), InjectError> {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
            KEYEVENTF_KEYUP, VK_CONTROL, VK_V,
        };

        let payload = if self.trailing_space {
            format!("{text} ")
        } else {
            text.to_string()
        };

        let mut clipboard = arboard::Clipboard::new()
            .map_err(|e| InjectError::Failed(format!("falha ao acessar clipboard: {e}")))?;
        clipboard
            .set_text(payload)
            .map_err(|e| InjectError::Failed(format!("falha ao copiar para clipboard: {e}")))?;

        if self.initial_delay_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(
                self.initial_delay_ms as u64,
            ));
        }

        let make_key = |vk, keyup| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: if keyup {
                        KEYEVENTF_KEYUP
                    } else {
                        KEYBD_EVENT_FLAGS(0)
                    },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };

        let inputs = [
            make_key(VK_CONTROL, false),
            make_key(VK_V, false),
            make_key(VK_V, true),
            make_key(VK_CONTROL, true),
        ];

        let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
        if sent != inputs.len() as u32 {
            return Err(InjectError::Failed("SendInput Ctrl+V falhou".into()));
        }

        Ok(())
    }
}

#[cfg(windows)]
pub type PlatformTypeInjector = WindowsTypeInjector;
#[cfg(windows)]
pub type PlatformClipboardInjector = WindowsClipboardInjector;

#[cfg(all(not(windows), feature = "x11"))]
pub type PlatformTypeInjector = XdotoolInjector;
#[cfg(all(not(windows), feature = "x11"))]
pub type PlatformClipboardInjector = ClipboardInjector;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stdout_injector_is_ok() {
        let inj = StdoutInjector;
        assert!(inj.inject("teste").is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn windows_injectors_can_be_constructed() {
        let _type_inj = WindowsTypeInjector::new().with_delay_ms(1);
        let _clip_inj = WindowsClipboardInjector::new().with_initial_delay_ms(1);
    }
}
