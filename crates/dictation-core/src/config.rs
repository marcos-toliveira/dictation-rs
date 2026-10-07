//! Configuração do dictation-rs.
//!
//! Para a troca ser **drop-in** com o `dictation` em Python, o formato principal é o
//! mesmo INI (`~/.config/dictation/config.ini`, seções `[dictation]`/`[paths]`/`[local]`).
//! Um TOML equivalente também é aceito (extensão `.toml`).

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
    /// Backend de captura: `ffmpeg` | `cpal` | vazio = automático (Linux: ffmpeg se houver).
    pub capture: String,
    pub max_seconds: u64,
    pub inject: InjectMode,
    pub type_delay_ms: u32,
    pub trailing_space: bool,
    pub notify: bool,
    pub indicator: bool,
    pub indicator_anchor: String,
    pub preview: bool,
    pub preview_interval_ms: u64,
    pub preview_anchor: String,
    pub segment_seconds: f64,
    pub overlap_seconds: f64,
    pub min_segment_seconds: f64,
    pub vault: PathBuf,
    pub corrections: PathBuf,
    pub vocab: PathBuf,
    pub state_dir: PathBuf,
    pub socket: Option<PathBuf>,
    pub whisper_bin: PathBuf,
    pub whisper_model: PathBuf,
    pub whisper_threads: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            provider: Provider::Groq,
            language: "pt".into(),
            model: "whisper-large-v3".into(),
            device: None,
            capture: String::new(),
            max_seconds: 180,
            inject: InjectMode::Type,
            type_delay_ms: 6,
            trailing_space: true,
            notify: true,
            indicator: true,
            indicator_anchor: "bottom-right".into(),
            preview: true,
            preview_interval_ms: 1500,
            preview_anchor: "bottom-center".into(),
            segment_seconds: 10.0,
            overlap_seconds: 0.5,
            min_segment_seconds: 0.3,
            vault: expand("~/.config/anubis/groq.env"),
            corrections: expand("~/.config/dictation/corrections.tsv"),
            vocab: expand("~/.config/dictation/vocab.txt"),
            state_dir: expand("~/.local/state/dictation"),
            socket: None,
            whisper_bin: expand("~/.local/bin/whisper-cli"),
            whisper_model: expand("~/.local/share/whisper.cpp/models/ggml-small.bin"),
            whisper_threads: 4,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("falha ao ler config: {0}")]
    Io(#[from] std::io::Error),
    #[error("config inválida: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("valor inválido para '{key}': '{value}'")]
    Value { key: String, value: String },
}

impl Config {
    /// Caminho padrão (o mesmo do Python, para troca drop-in).
    pub fn default_path() -> PathBuf {
        if let Ok(p) = std::env::var("DICTATION_CONFIG") {
            return PathBuf::from(p);
        }
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
            });
        base.join("dictation").join("config.ini")
    }

    /// Parseia TOML (formato nativo do rewrite).
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        let mut cfg: Config = toml::from_str(text)?;
        cfg.expand_paths();
        Ok(cfg)
    }

    /// Parseia o INI do Python (`[dictation]`/`[paths]`/`[local]`).
    pub fn parse_ini(text: &str) -> Result<Self, ConfigError> {
        let mut cfg = Config::default();
        let mut section = String::from("dictation");
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                section = line[1..line.len() - 1].trim().to_lowercase();
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let key = k.trim().to_lowercase();
            let val = v.trim().trim_matches('"');
            let num = |key: &str| -> Result<f64, ConfigError> {
                val.parse::<f64>().map_err(|_| ConfigError::Value {
                    key: key.into(),
                    value: val.into(),
                })
            };
            match (section.as_str(), key.as_str()) {
                ("dictation", "provider") => cfg.provider = parse_provider(val)?,
                ("dictation", "language") => cfg.language = val.into(),
                ("dictation", "model") => cfg.model = val.into(),
                ("dictation", "device") => cfg.device = (!val.is_empty()).then(|| val.to_string()),
                ("dictation", "capture") => cfg.capture = val.into(),
                ("dictation", "max_seconds") => cfg.max_seconds = num(&key)? as u64,
                ("dictation", "inject") => cfg.inject = parse_inject(val)?,
                ("dictation", "type_delay_ms") => cfg.type_delay_ms = num(&key)? as u32,
                ("dictation", "trailing_space") => cfg.trailing_space = parse_bool(val),
                ("dictation", "notify") => cfg.notify = parse_bool(val),
                ("dictation", "indicator") => cfg.indicator = parse_bool(val),
                ("dictation", "indicator_anchor") => cfg.indicator_anchor = val.into(),
                ("dictation", "preview") => cfg.preview = parse_bool(val),
                ("dictation", "preview_interval_ms") => cfg.preview_interval_ms = num(&key)? as u64,
                ("dictation", "preview_anchor") => cfg.preview_anchor = val.into(),
                ("dictation", "segment_seconds") => cfg.segment_seconds = num(&key)?,
                ("dictation", "overlap_seconds") => cfg.overlap_seconds = num(&key)?,
                ("dictation", "min_segment_seconds") => cfg.min_segment_seconds = num(&key)?,
                ("paths", "vault") => cfg.vault = expand(val),
                ("paths", "vocab") => cfg.vocab = expand(val),
                ("paths", "corrections") => cfg.corrections = expand(val),
                ("paths", "state_dir") => cfg.state_dir = expand(val),
                ("paths", "socket") => cfg.socket = Some(expand(val)),
                ("local", "whisper_bin") => cfg.whisper_bin = expand(val),
                ("local", "model") => cfg.whisper_model = expand(val),
                ("local", "threads") => cfg.whisper_threads = num(&key)? as u32,
                _ => {}
            }
        }
        cfg.expand_paths();
        Ok(cfg)
    }

    /// Carrega por extensão (`.toml` → TOML; resto → INI).
    pub fn load_any(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path)?;
        if path.extension().map(|e| e == "toml").unwrap_or(false) {
            Self::parse(&text)
        } else {
            Self::parse_ini(&text)
        }
    }

    /// Carrega o caminho padrão; se ausente/inválido, usa o padrão (não falha o daemon).
    pub fn load_or_default(path: &Path) -> Self {
        if path.is_file() {
            Self::load_any(path).unwrap_or_default()
        } else {
            Config::default()
        }
    }

    /// Socket padrão (paridade com o Python: `${XDG_RUNTIME_DIR}/dictation.sock`).
    pub fn socket_path(&self) -> PathBuf {
        self.socket.clone().unwrap_or_else(default_socket)
    }

    fn expand_paths(&mut self) {
        self.vault = expand(&self.vault.to_string_lossy());
        self.corrections = expand(&self.corrections.to_string_lossy());
        self.vocab = expand(&self.vocab.to_string_lossy());
        self.state_dir = expand(&self.state_dir.to_string_lossy());
        self.whisper_bin = expand(&self.whisper_bin.to_string_lossy());
        self.whisper_model = expand(&self.whisper_model.to_string_lossy());
        if let Some(s) = self.socket.clone() {
            self.socket = Some(expand(&s.to_string_lossy()));
        }
    }
}

fn parse_provider(v: &str) -> Result<Provider, ConfigError> {
    match v.to_lowercase().as_str() {
        "groq" => Ok(Provider::Groq),
        "local" => Ok(Provider::Local),
        _ => Err(ConfigError::Value {
            key: "provider".into(),
            value: v.into(),
        }),
    }
}

fn parse_inject(v: &str) -> Result<InjectMode, ConfigError> {
    match v.to_lowercase().as_str() {
        "type" => Ok(InjectMode::Type),
        "clipboard" => Ok(InjectMode::Clipboard),
        "stdout" => Ok(InjectMode::Stdout),
        _ => Err(ConfigError::Value {
            key: "inject".into(),
            value: v.into(),
        }),
    }
}

fn parse_bool(v: &str) -> bool {
    matches!(
        v.trim().to_lowercase().as_str(),
        "true" | "yes" | "1" | "on" | "sim"
    )
}

fn default_socket() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("dictation.sock")
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
        assert!(c.trailing_space);
        assert!(c.preview);
    }

    #[test]
    fn parses_toml_overrides() {
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
    fn parses_ini_like_python() {
        let ini = "\
; comentário
[dictation]
provider = groq
language = pt
model = whisper-large-v3-turbo
device =
max_seconds = 120
inject = type
type_delay_ms = 6
trailing_space = true
notify = false
indicator = true
indicator_anchor = bottom-right
preview = true
preview_interval_ms = 1500
preview_anchor = bottom-center

[paths]
vault = ~/.config/anubis/groq.env
vocab = ~/.config/dictation/vocab.txt
corrections = ~/.config/dictation/corrections.tsv
state_dir = ~/.local/state/dictation

[local]
whisper_bin = ~/.local/bin/whisper-cli
model = ~/.local/share/whisper.cpp/models/ggml-small.bin
threads = 4
";
        let c = Config::parse_ini(ini).unwrap();
        assert_eq!(c.provider, Provider::Groq);
        assert_eq!(c.max_seconds, 120);
        assert_eq!(c.type_delay_ms, 6);
        assert!(!c.notify);
        assert!(c.indicator);
        assert_eq!(c.indicator_anchor, "bottom-right");
        assert_eq!(c.preview_interval_ms, 1500);
        assert_eq!(c.preview_anchor, "bottom-center");
        assert_eq!(c.device, None);
        assert_eq!(c.whisper_threads, 4);
        assert!(!c.vault.to_string_lossy().starts_with('~'));
        assert!(c
            .state_dir
            .to_string_lossy()
            .ends_with(".local/state/dictation"));
    }

    #[test]
    fn ini_invalid_value_errors() {
        let err = Config::parse_ini("[dictation]\nmax_seconds = abc\n").unwrap_err();
        assert!(matches!(err, ConfigError::Value { .. }));
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
