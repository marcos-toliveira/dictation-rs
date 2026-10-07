//! Configuração (TOML) do dictation-rs.

use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    #[default]
    Groq,
    Local,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum InjectMode {
    #[default]
    Type,
    Clipboard,
    Stdout,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub provider: Provider,
    pub language: String,
    pub model: String,
    pub device: Option<String>,
    pub max_seconds: u64,
    pub inject: InjectMode,
    pub segment_seconds: f64,
    pub overlap_seconds: f64,
    pub min_segment_seconds: f64,
    pub vault: PathBuf,
    pub corrections: PathBuf,
    pub vocab: PathBuf,
    pub socket: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            provider: Provider::Groq,
            language: "pt".into(),
            model: "whisper-large-v3-turbo".into(),
            device: None,
            max_seconds: 180,
            inject: InjectMode::Type,
            segment_seconds: 10.0,
            overlap_seconds: 0.5,
            min_segment_seconds: 0.3,
            vault: expand("~/.config/anubis/groq.env"),
            corrections: expand("~/.config/dictation/corrections.tsv"),
            vocab: expand("~/.config/dictation/vocab.txt"),
            socket: None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("falha ao ler config: {0}")]
    Io(#[from] std::io::Error),
    #[error("config inválida: {0}")]
    Parse(#[from] toml::de::Error),
}

impl Config {
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        let mut cfg: Config = toml::from_str(text)?;
        cfg.expand_paths();
        Ok(cfg)
    }

    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        Self::parse(&std::fs::read_to_string(path)?)
    }

    /// Carrega o arquivo; se ausente/inválido, usa o padrão (não falha o daemon).
    pub fn load_or_default(path: &Path) -> Self {
        if path.is_file() {
            Self::load(path).unwrap_or_default()
        } else {
            Config::default()
        }
    }

    pub fn socket_path(&self) -> PathBuf {
        self.socket.clone().unwrap_or_else(default_socket)
    }

    fn expand_paths(&mut self) {
        self.vault = expand(&self.vault.to_string_lossy());
        self.corrections = expand(&self.corrections.to_string_lossy());
        self.vocab = expand(&self.vocab.to_string_lossy());
        if let Some(s) = self.socket.clone() {
            self.socket = Some(expand(&s.to_string_lossy()));
        }
    }
}

fn default_socket() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("dictation-rs.sock")
}

/// Expande um `~` inicial para `$HOME`.
fn expand(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let c = Config::default();
        assert_eq!(c.provider, Provider::Groq);
        assert_eq!(c.inject, InjectMode::Type);
        assert_eq!(c.language, "pt");
        assert!(c.segment_seconds > 0.0);
        assert!(c.overlap_seconds < c.segment_seconds);
    }

    #[test]
    fn parses_overrides() {
        let c = Config::parse(
            "provider = \"local\"\nlanguage = \"en\"\ninject = \"stdout\"\nsegment_seconds = 8.0\n",
        )
        .unwrap();
        assert_eq!(c.provider, Provider::Local);
        assert_eq!(c.language, "en");
        assert_eq!(c.inject, InjectMode::Stdout);
        assert_eq!(c.segment_seconds, 8.0);
    }

    #[test]
    fn expands_home() {
        let c = Config::parse("vault = \"~/.config/anubis/groq.env\"\n").unwrap();
        assert!(!c.vault.to_string_lossy().starts_with('~'));
        assert!(c
            .vault
            .to_string_lossy()
            .ends_with(".config/anubis/groq.env"));
    }
}
