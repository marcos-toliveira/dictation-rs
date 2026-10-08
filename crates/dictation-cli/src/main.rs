//! `dictation` — cliente fino: envia um comando ao `dictationd` pelo socket.
//!
//! Uso:
//!   dictation [toggle|start|stop|status]     (padrão: toggle)
//!   dictation teach <errado> <certo> [--vocab]
//!   dictation last
//!   dictation transcribe <arquivo.wav> [--to stdout|type]

use dictation_core::{AsrEngine, AsrOptions, Config, InjectMode};
use dictation_groq::GroqEngine;
use dictation_platform::inject::{PlatformClipboardInjector, PlatformTypeInjector, StdoutInjector};
use dictation_platform::TextInjector;
use std::io::{BufRead, BufReader, Write};
#[cfg(not(windows))]
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

/// Envia uma linha ao daemon e imprime a resposta (Unix domain socket).
#[cfg(not(windows))]
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

/// Envia uma linha ao daemon e imprime a resposta (Named pipe no Windows).
#[cfg(windows)]
fn send(cfg: &Config, line: &str) {
    use windows::core::HSTRING;
    use windows::Win32::System::Pipes::WaitNamedPipeW;

    let sock = cfg.socket_path();
    let pipe_path = sock.to_string_lossy();
    let wide_name = HSTRING::from(pipe_path.as_ref());

    let mut attempts = 0;
    let stream = loop {
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(pipe_path.as_ref())
        {
            Ok(s) => break s,
            Err(e) => {
                // os error 231 = ERROR_PIPE_BUSY (servidor está atendendo outra conexão ou reconectando)
                if e.raw_os_error() == Some(231) && attempts < 10 {
                    attempts += 1;
                    unsafe {
                        let _ = WaitNamedPipeW(&wide_name, 1000);
                    }
                    continue;
                }
                eprintln!("daemon indisponível em {}: {e}", sock.display());
                std::process::exit(1);
            }
        }
    };

    let mut stream = stream;
    let _ = stream.write_all(format!("{line}\n").as_bytes());
    let _ = stream.flush();
    let mut reply = String::new();
    let _ = BufReader::new(stream).read_line(&mut reply);
    print!("{reply}");
}

/// Sobe o daemon destacado se o socket ainda não existir.
fn ensure_daemon(cfg: &Config) {
    let sock = cfg.socket_path();
    #[cfg(not(windows))]
    if sock.exists() {
        return;
    }
    #[cfg(windows)]
    {
        use windows::core::HSTRING;
        use windows::Win32::Foundation::{GetLastError, ERROR_PIPE_BUSY};
        use windows::Win32::System::Pipes::WaitNamedPipeW;

        let wide_name = HSTRING::from(sock.to_string_lossy().as_ref());
        unsafe {
            if WaitNamedPipeW(&wide_name, 0).as_bool() || GetLastError() == ERROR_PIPE_BUSY {
                return;
            }
        }
    }

    let bin = std::env::var("DICTATIOND_BIN").unwrap_or_else(|_| "dictationd".into());

    #[cfg(not(windows))]
    let _ = Command::new("setsid")
        .arg(&bin)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x00000008;
        let _ = Command::new(&bin)
            .creation_flags(DETACHED_PROCESS)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }

    for _ in 0..80 {
        #[cfg(not(windows))]
        if sock.exists() {
            return;
        }
        #[cfg(windows)]
        {
            use windows::core::HSTRING;
            use windows::Win32::Foundation::{GetLastError, ERROR_PIPE_BUSY};
            use windows::Win32::System::Pipes::WaitNamedPipeW;

            let wide_name = HSTRING::from(sock.to_string_lossy().as_ref());
            unsafe {
                if WaitNamedPipeW(&wide_name, 0).as_bool() || GetLastError() == ERROR_PIPE_BUSY {
                    return;
                }
            }
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
        InjectMode::Clipboard => {
            Box::new(PlatformClipboardInjector::new().with_trailing_space(cfg.trailing_space))
        }
        InjectMode::Type => Box::new(
            PlatformTypeInjector::new()
                .with_delay_ms(cfg.type_delay_ms)
                .with_trailing_space(cfg.trailing_space),
        ),
    };
    let _ = injector.inject(&text);
}
