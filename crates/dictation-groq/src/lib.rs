//! `dictation-groq` — implementação do [`AsrEngine`] para o Groq.
//!
//! Envia o WAV por multipart para `/openai/v1/audio/transcriptions` (compatível com
//! OpenAI), com `model`, `language` e `prompt` (viés de vocabulário). Nenhuma lógica de
//! estado aqui — apenas a tradução "WAV → texto".

use dictation_core::{AsrEngine, AsrError, AsrOptions};
use std::path::Path;
use std::time::Duration;

const DEFAULT_ENDPOINT: &str = "https://api.groq.com/openai/v1/audio/transcriptions";

/// Cliente do Groq.
pub struct GroqEngine {
    endpoint: String,
    api_key: String,
    timeout: Duration,
}

impl GroqEngine {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            endpoint: DEFAULT_ENDPOINT.to_string(),
            api_key: api_key.into(),
            timeout: Duration::from_secs(90),
        }
    }

    /// Sobrescreve o endpoint (usado em testes com um servidor mock).
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Lê `GROQ_API_KEY` (e `GROQ_MODEL`) do ambiente ou de um cofre `KEY=VALUE` (ex.: `~/.config/anubis/groq.env`).
    pub fn from_vault(path: &Path) -> Result<Self, AsrError> {
        if let Ok(key) = std::env::var("GROQ_API_KEY") {
            let key = key.trim().to_string();
            if !key.is_empty() {
                return Ok(Self::new(key));
            }
        }
        let text = if path.is_file() {
            std::fs::read_to_string(path)
                .map_err(|e| AsrError::Network(format!("vault {}: {e}", path.display())))?
        } else {
            let alt1 = std::env::var_os("USERPROFILE").map(|p| {
                std::path::PathBuf::from(p)
                    .join(".config")
                    .join("anubis")
                    .join("groq.env")
            });
            let alt2 = std::env::var_os("APPDATA")
                .map(|p| std::path::PathBuf::from(p).join("anubis").join("groq.env"));
            let text_opt = alt1
                .and_then(|p| std::fs::read_to_string(p).ok())
                .or_else(|| alt2.and_then(|p| std::fs::read_to_string(p).ok()));
            match text_opt {
                Some(t) => t,
                None => {
                    return Err(AsrError::Network(format!(
                        "vault {} não encontrado (defina GROQ_API_KEY ou configure groq.env)",
                        path.display()
                    )));
                }
            }
        };
        let key = parse_vault_key(&text).ok_or_else(|| {
            AsrError::Network(format!("GROQ_API_KEY ausente em {}", path.display()))
        })?;
        Ok(Self::new(key))
    }

    /// Monta o corpo multipart e o `Content-Type` correspondente.
    fn multipart(&self, wav: &[u8], opts: &AsrOptions) -> (String, Vec<u8>) {
        let boundary = format!(
            "----dictationrs{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let mut body: Vec<u8> = Vec::with_capacity(wav.len() + 512);
        let mut field = |name: &str, value: &str| {
            body.extend_from_slice(
                format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")
                    .as_bytes(),
            );
        };
        field("model", &opts.model);
        field("response_format", "json");
        if let Some(lang) = opts
            .language
            .as_deref()
            .filter(|l| *l != "auto" && !l.is_empty())
        {
            field("language", lang);
        }
        if let Some(prompt) = opts.prompt.as_deref().filter(|p| !p.is_empty()) {
            field("prompt", prompt);
        }
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\n")
                .as_bytes(),
        );
        body.extend_from_slice(wav);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        (format!("multipart/form-data; boundary={boundary}"), body)
    }
}

impl AsrEngine for GroqEngine {
    fn transcribe(&self, wav: &[u8], opts: &AsrOptions) -> Result<String, AsrError> {
        let (content_type, body) = self.multipart(wav, opts);
        let response = ureq::post(&self.endpoint)
            .set("Authorization", &format!("Bearer {}", self.api_key))
            .set("Content-Type", &content_type)
            .timeout(self.timeout)
            .send_bytes(&body)
            .map_err(|e| match e {
                ureq::Error::Status(code, resp) => AsrError::Http {
                    status: code,
                    body: resp
                        .into_string()
                        .unwrap_or_default()
                        .chars()
                        .take(300)
                        .collect(),
                },
                other => AsrError::Network(other.to_string()),
            })?;
        let value: serde_json::Value = response
            .into_json()
            .map_err(|e| AsrError::Decode(e.to_string()))?;
        value
            .get("text")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .ok_or_else(|| AsrError::Decode("resposta sem campo 'text'".into()))
    }
}

/// Extrai `GROQ_API_KEY` de um texto no formato `CHAVE=valor`.
pub fn parse_vault_key(text: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("GROQ_API_KEY=") {
            let v = v.trim().trim_matches(['"', '\'']);
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_vault_key() {
        let text = "# cofre\nGROQ_API_KEY=gsk_abc123\nGROQ_MODEL=whisper-large-v3-turbo\n";
        assert_eq!(parse_vault_key(text).as_deref(), Some("gsk_abc123"));
        assert_eq!(parse_vault_key("nada aqui"), None);
        assert_eq!(parse_vault_key("GROQ_API_KEY=  "), None);
    }

    #[test]
    fn multipart_contains_fields_and_audio() {
        let engine = GroqEngine::new("gsk_x");
        let opts = AsrOptions {
            model: "whisper-large-v3-turbo".into(),
            language: Some("pt".into()),
            prompt: Some("OpenCode, Tulinho".into()),
        };
        let (ct, body) = engine.multipart(b"RIFFDATA", &opts);
        assert!(ct.starts_with("multipart/form-data; boundary="));
        let s = String::from_utf8_lossy(&body);
        assert!(s.contains("name=\"model\""));
        assert!(s.contains("whisper-large-v3-turbo"));
        assert!(s.contains("name=\"language\""));
        assert!(s.contains("name=\"prompt\""));
        assert!(s.contains("OpenCode, Tulinho"));
        assert!(body.windows(8).any(|w| w == b"RIFFDATA"));
    }

    #[test]
    fn multipart_skips_auto_language() {
        let engine = GroqEngine::new("gsk_x");
        let opts = AsrOptions {
            model: "m".into(),
            language: Some("auto".into()),
            prompt: None,
        };
        let (_, body) = engine.multipart(b"x", &opts);
        assert!(!String::from_utf8_lossy(&body).contains("name=\"language\""));
    }
}
