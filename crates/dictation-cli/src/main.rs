//! `dictation` — cliente fino: envia um comando ao `dictationd` pelo socket.
//!
//! Uso:
//!   dictation [toggle|start|stop|status]     (padrão: toggle)
//!   dictation teach <errado> <certo> [--vocab]
//!   dictation last
//!   dictation transcribe <arquivo.wav> [--to stdout|type]

use dictation_core::{AsrEngine, AsrOptions, Config, InjectMode};
use dictation_groq::GroqEngine;
use dictation_platform::inject::{StdoutInjector, XdotoolInjector};
use dictation_platform::TextInjector;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::{Command, Stdio};
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).cloned().unwrap_or_else(|| "toggle".into());
    let cfg = Config::load_or_default(&Config::default_path());

    match cmd.as_str() {
        "transcribe" => transcribe(&cfg, &args),
        "teach" => {
            let rest = &args[2..];
            let line = if rest.is_empty() {
                "teach".to_string()
            } else {
                format!("teach {}", rest.join(" "))
            };
            ensure_daemon(&cfg);
            send(&cfg, &line);
        }
        "last" => send(&cfg, "last"),
        other => {
            ensure_daemon(&cfg);
            send(&cfg, other);
        }
    }
}

/// Envia uma linha ao daemon e imprime a resposta.
fn send(cfg: &Config, line: &str) {
    let sock = cfg.socket_path();
    match UnixStream::connect(&sock) {
        Ok(mut stream) => {
            let _ = stream.write_all(format!("{line}\n").as_bytes());
            let mut reply = String::new();
            let _ = BufReader::new(stream).read_line(&mut reply);
            print!("{reply}");
        }
        Err(e) => {
            eprintln!("daemon indisponível em {}: {e}", sock.display());
            std::process::exit(1);
        }
    }
}

/// Sobe o daemon destacado se o socket ainda não existir.
fn ensure_daemon(cfg: &Config) {
    let sock = cfg.socket_path();
    if sock.exists() {
        return;
    }
    let bin = std::env::var("DICTATIOND_BIN").unwrap_or_else(|_| "dictationd".into());
    let _ = Command::new("setsid")
        .arg(&bin)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    for _ in 0..80 {
        if sock.exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Transcreve um WAV local (debug/offline) e injeta.
fn transcribe(cfg: &Config, args: &[String]) {
    let Some(file) = args.get(2) else {
        eprintln!("uso: dictation transcribe <arquivo.wav> [--to stdout|type]");
        std::process::exit(2);
    };
    let data = match std::fs::read(file) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("não consegui ler {file}: {e}");
            std::process::exit(1);
        }
    };
    let asr = match GroqEngine::from_vault(&cfg.vault) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let opts = AsrOptions {
        model: cfg.model.clone(),
        language: Some(cfg.language.clone()),
        prompt: None,
    };
    let text = match asr.transcribe(&data, &opts) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("erro ao transcrever: {e}");
            std::process::exit(1);
        }
    };
    let injector: Box<dyn TextInjector> = match cfg.inject {
        InjectMode::Stdout => Box::new(StdoutInjector),
        _ => Box::new(XdotoolInjector::new().with_delay_ms(cfg.type_delay_ms)),
    };
    let _ = injector.inject(&text);
}
