//! `dictationd` — daemon do dictation-rs.
//!
//! Servidor de socket local + thread de captura. Comandos (uma linha):
//! `start | stop | toggle | status | quit`.
//!
//! Config: `~/.config/dictation-rs/config.toml` (ou `$DICTATION_CONFIG`).

use dictation_core::{Config, InjectMode};
use dictation_daemon::Engine;
use dictation_groq::GroqEngine;
use dictation_platform::capture::CpalCapture;
use dictation_platform::inject::{StdoutInjector, XdotoolInjector};
use dictation_platform::{AudioCapture, TextInjector};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

fn config_path() -> PathBuf {
    if let Ok(p) = std::env::var("DICTATION_CONFIG") {
        return PathBuf::from(p);
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        });
    base.join("dictation-rs").join("config.toml")
}

fn main() {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cfg = Config::load_or_default(&config_path());
    let asr = match GroqEngine::from_vault(&cfg.vault) {
        Ok(e) => e,
        Err(e) => {
            tracing::error!(error = %e, "não foi possível carregar a chave do Groq");
            std::process::exit(1);
        }
    };
    let asr: Arc<dyn dictation_core::AsrEngine> = Arc::new(asr);
    let injector: Arc<dyn TextInjector> = match cfg.inject {
        InjectMode::Stdout => Arc::new(StdoutInjector),
        _ => Arc::new(XdotoolInjector::new()),
    };

    let engine = Arc::new(Mutex::new(Engine::new(&cfg, asr, injector)));
    let stop = Arc::new(AtomicBool::new(false));
    let mut capture: Option<std::thread::JoinHandle<()>> = None;

    let sock = cfg.socket_path();
    let _ = std::fs::remove_file(&sock);
    let listener = match UnixListener::bind(&sock) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(error = %e, path = %sock.display(), "falha ao abrir o socket");
            std::process::exit(1);
        }
    };
    tracing::info!(socket = %sock.display(), "dictationd ouvindo");

    for conn in listener.incoming() {
        match conn {
            Ok(stream) => {
                if let Err(e) = handle(stream, &engine, &cfg, &stop, &mut capture) {
                    tracing::warn!(error = %e, "erro ao tratar conexão");
                }
            }
            Err(e) => tracing::warn!(error = %e, "accept"),
        }
    }
    let _ = std::fs::remove_file(&sock);
}

fn handle(
    mut stream: UnixStream,
    engine: &Arc<Mutex<Engine>>,
    cfg: &Config,
    stop: &Arc<AtomicBool>,
    capture: &mut Option<std::thread::JoinHandle<()>>,
) -> std::io::Result<()> {
    let mut line = String::new();
    BufReader::new(stream.try_clone()?).read_line(&mut line)?;
    let reply = match line.trim() {
        "start" => start(engine, cfg, stop, capture),
        "stop" => stop_recording(engine, stop, capture),
        "toggle" => {
            if engine.lock().unwrap().is_recording() {
                stop_recording(engine, stop, capture)
            } else {
                start(engine, cfg, stop, capture)
            }
        }
        "status" => {
            if engine.lock().unwrap().is_recording() {
                "gravando".to_string()
            } else {
                "ocioso".to_string()
            }
        }
        "quit" => {
            stream.write_all(b"bye\n")?;
            stream.flush()?;
            std::process::exit(0);
        }
        other => format!("? ({other})"),
    };
    stream.write_all(format!("{reply}\n").as_bytes())?;
    stream.flush()
}

fn start(
    engine: &Arc<Mutex<Engine>>,
    cfg: &Config,
    stop: &Arc<AtomicBool>,
    capture: &mut Option<std::thread::JoinHandle<()>>,
) -> String {
    if engine.lock().unwrap().is_recording() {
        return "já gravando".to_string();
    }
    if let Err(e) = engine.lock().unwrap().start() {
        return format!("erro: {e}");
    }
    stop.store(false, Ordering::SeqCst);
    let engine2 = Arc::clone(engine);
    let stop2 = Arc::clone(stop);
    let device = cfg.device.clone();
    *capture = Some(std::thread::spawn(move || {
        let mut cap = CpalCapture::new();
        if let Some(d) = device {
            cap = cap.with_device(d);
        }
        if let Err(e) = cap.start() {
            tracing::error!(error = %e, "falha ao iniciar captura");
            return;
        }
        while !stop2.load(Ordering::SeqCst) {
            if let Some(block) = cap.recv() {
                engine2.lock().unwrap().push_audio(&block);
            }
        }
        let _ = cap.stop();
    }));
    "gravando".to_string()
}

fn stop_recording(
    engine: &Arc<Mutex<Engine>>,
    stop: &Arc<AtomicBool>,
    capture: &mut Option<std::thread::JoinHandle<()>>,
) -> String {
    if !engine.lock().unwrap().is_recording() {
        return "nada gravando".to_string();
    }
    stop.store(true, Ordering::SeqCst);
    if let Some(h) = capture.take() {
        let _ = h.join();
    }
    match engine.lock().unwrap().stop_and_finish() {
        Some(text) => format!("transcrevendo: {text}"),
        None => "nada reconhecido".to_string(),
    }
}
