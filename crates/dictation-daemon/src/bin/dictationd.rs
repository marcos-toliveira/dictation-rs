//! `dictationd` — daemon do dictation-rs com GUI.
//!
//! Thread principal: overlay (egui/eframe). Threads: socket local, captura (`cpal`),
//! prévia ao vivo (Groq) e bandeja (`ksni`).
//!
//! Comandos do socket (uma linha): `start | stop | toggle | status | teach <e> <c> [--vocab] | last | quit`.
//! Config: `~/.config/dictation/config.ini` (o mesmo do Python) ou `$DICTATION_CONFIG`.

use dictation_core::{AsrEngine, AsrOptions, Config, InjectMode, Provider};
use dictation_daemon::{read_last, save_last, teach, Engine, FallbackAsr, LocalWhisper};
use dictation_groq::GroqEngine;
use dictation_platform::capture::CpalCapture;
use dictation_platform::inject::{StdoutInjector, XdotoolInjector};
use dictation_platform::{AudioCapture, TextInjector};
use dictation_ui::state::{self, SharedUi};
use dictation_ui::tray::{spawn_tray, TrayAction, TrayHandle};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// Estado do daemon, compartilhado entre as threads.
struct Daemon {
    cfg: Config,
    engine: Arc<Mutex<Engine>>,
    asr: Arc<dyn AsrEngine>,
    opts: AsrOptions,
    ui: SharedUi,
    recording: bool,
    stop: Arc<AtomicBool>,
    capture: Option<JoinHandle<()>>,
    tray: Option<TrayHandle>,
}

impl Daemon {
    fn handle(&mut self, line: &str) -> String {
        let mut parts = line.split_whitespace();
        let cmd = parts.next().unwrap_or("");
        match cmd {
            "start" => self.start(),
            "stop" => self.stop(),
            "toggle" => {
                if self.recording {
                    self.stop()
                } else {
                    self.start()
                }
            }
            "status" => {
                if self.recording {
                    "gravando".into()
                } else {
                    "ocioso".into()
                }
            }
            "teach" => {
                let wrong = parts.next();
                let right = parts.next();
                let vocab = parts.any(|p| p == "--vocab" || p == "vocab");
                match (wrong, right) {
                    (Some(w), Some(r)) => match teach(&self.cfg, w, r, vocab) {
                        Ok(()) => format!("ok: '{w}' → '{r}'"),
                        Err(e) => format!("erro: {e}"),
                    },
                    _ => "uso: dictation teach <errado> <certo> [--vocab]".into(),
                }
            }
            "last" => read_last(&self.cfg),
            "quit" => {
                std::process::exit(0);
            }
            other => format!("? ({other})"),
        }
    }

    fn start(&mut self) -> String {
        if self.recording {
            return "já gravando".into();
        }
        if let Err(e) = self.engine.lock().unwrap().start() {
            return format!("erro: {e}");
        }
        self.recording = true;
        self.stop.store(false, Ordering::SeqCst);
        {
            let mut ui = self.ui.lock().unwrap();
            ui.recording = true;
            ui.preview.clear();
        }
        self.update_tray(true);

        let engine = Arc::clone(&self.engine);
        let stop = Arc::clone(&self.stop);
        let device = self.cfg.device.clone();
        self.capture = Some(std::thread::spawn(move || {
            let mut cap = CpalCapture::new();
            if let Some(d) = device {
                cap = cap.with_device(d);
            }
            if let Err(e) = cap.start() {
                tracing::error!(error = %e, "falha ao iniciar captura");
                return;
            }
            while !stop.load(Ordering::SeqCst) {
                if let Some(block) = cap.recv() {
                    engine.lock().unwrap().push_audio(&block);
                }
            }
            let _ = cap.stop();
        }));
        "gravando".to_string()
    }

    fn stop(&mut self) -> String {
        if !self.recording {
            return "nada gravando".into();
        }
        self.recording = false;
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.capture.take() {
            let _ = h.join();
        }
        {
            let mut ui = self.ui.lock().unwrap();
            ui.recording = false;
            ui.preview.clear();
        }
        self.update_tray(false);

        // Finaliza em thread separada (transcrição + injeção) — resposta imediata.
        let engine = Arc::clone(&self.engine);
        let cfg = self.cfg.clone();
        std::thread::spawn(move || {
            let text = engine.lock().unwrap().stop_and_finish();
            match text {
                Some(t) => {
                    save_last(&cfg, &t);
                    notify(&cfg, "✅ dictation", &t);
                }
                None => notify(&cfg, "⚠️ dictation", "nada reconhecido"),
            }
        });
        "transcrevendo".to_string()
    }

    fn update_tray(&self, recording: bool) {
        if let Some(t) = &self.tray {
            let _ = t.update(|tray| tray.recording = recording);
        }
    }
}

fn config_path() -> PathBuf {
    Config::default_path()
}

fn build_asr(cfg: &Config) -> Arc<dyn AsrEngine> {
    let groq: Arc<dyn AsrEngine> = match GroqEngine::from_vault(&cfg.vault) {
        Ok(e) => Arc::new(e),
        Err(e) => {
            tracing::error!(error = %e, "não foi possível carregar a chave do Groq");
            std::process::exit(1);
        }
    };
    let local = || {
        Arc::new(LocalWhisper::new(
            cfg.whisper_bin.clone(),
            cfg.whisper_model.clone(),
            cfg.language.clone(),
            cfg.whisper_threads,
            cfg.state_dir.clone(),
        )) as Arc<dyn AsrEngine>
    };
    if cfg.provider == Provider::Local {
        local()
    } else {
        Arc::new(FallbackAsr::new(groq, local()))
    }
}

fn notify(cfg: &Config, title: &str, body: &str) {
    if cfg.notify && which("notify-send") {
        let _ = Command::new("notify-send")
            .args(["-a", "dictation", title, body])
            .status();
    }
}

fn which(prog: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(prog).is_file()))
        .unwrap_or(false)
}

fn open(path: &std::path::Path) {
    let _ = Command::new("xdg-open").arg(path).status();
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
    let _ = std::fs::create_dir_all(&cfg.state_dir);

    let asr = build_asr(&cfg);
    let injector: Arc<dyn TextInjector> = match cfg.inject {
        InjectMode::Stdout => Arc::new(StdoutInjector),
        _ => Arc::new(XdotoolInjector::new().with_delay_ms(cfg.type_delay_ms)),
    };
    let engine = Arc::new(Mutex::new(Engine::new(&cfg, Arc::clone(&asr), injector)));
    let opts = engine.lock().unwrap().options();
    let ui = state::shared();

    let daemon = Arc::new(Mutex::new(Daemon {
        cfg: cfg.clone(),
        engine,
        asr,
        opts,
        ui: Arc::clone(&ui),
        recording: false,
        stop: Arc::new(AtomicBool::new(false)),
        capture: None,
        tray: None,
    }));

    // Bandeja (ksni) + dispatcher de ações.
    let (tx, rx) = mpsc::channel::<TrayAction>();
    match spawn_tray(tx) {
        Ok(handle) => daemon.lock().unwrap().tray = Some(handle),
        Err(e) => tracing::warn!(error = %e, "bandeja indisponível"),
    }
    {
        let d = Arc::clone(&daemon);
        std::thread::spawn(move || {
            while let Ok(action) = rx.recv() {
                let cfg = d.lock().unwrap().cfg.clone();
                match action {
                    TrayAction::Toggle => {
                        d.lock().unwrap().handle("toggle");
                    }
                    TrayAction::Teach => {
                        let _ = d.lock().unwrap().handle("teach");
                    }
                    TrayAction::EditCorrections => open(&cfg.corrections),
                    TrayAction::EditVocab => open(&cfg.vocab),
                    TrayAction::Quit => std::process::exit(0),
                }
            }
        });
    }

    // Socket local.
    let sock = cfg.socket_path();
    {
        let d = Arc::clone(&daemon);
        std::thread::spawn(move || run_socket(d, sock));
    }

    // Prévia ao vivo.
    {
        let d = Arc::clone(&daemon);
        std::thread::spawn(move || run_preview(d));
    }

    tracing::info!("dictationd pronto");
    // Thread principal: overlay (exigência do winit).
    let _ = dictation_ui::run_overlay(ui, cfg.indicator_anchor.clone(), cfg.preview_anchor.clone());
}

fn run_socket(daemon: Arc<Mutex<Daemon>>, sock: PathBuf) {
    let _ = std::fs::remove_file(&sock);
    let listener = match UnixListener::bind(&sock) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(error = %e, path = %sock.display(), "falha ao abrir o socket");
            return;
        }
    };
    tracing::info!(socket = %sock.display(), "dictationd ouvindo");
    for stream in listener.incoming().flatten() {
        let mut line = String::new();
        if BufReader::new(stream.try_clone().unwrap())
            .read_line(&mut line)
            .is_ok()
        {
            let reply = daemon.lock().unwrap().handle(line.trim());
            let mut s = stream;
            let _ = s.write_all(format!("{reply}\n").as_bytes());
            let _ = s.flush();
        }
    }
}

fn run_preview(daemon: Arc<Mutex<Daemon>>) {
    loop {
        let interval = daemon.lock().unwrap().cfg.preview_interval_ms.max(300);
        std::thread::sleep(Duration::from_millis(interval));

        let snapshot = {
            let d = daemon.lock().unwrap();
            if !d.recording || !d.cfg.preview || d.cfg.provider != Provider::Groq {
                continue;
            }
            let e = d.engine.lock().unwrap();
            (
                e.pending_wav(),
                e.committed_text(),
                Arc::clone(&d.asr),
                d.opts.clone(),
            )
        };
        let (Some(wav), committed, asr, opts) = snapshot else {
            continue;
        };
        if let Ok(tail) = asr.transcribe(&wav, &opts) {
            let preview = if committed.is_empty() {
                tail
            } else {
                format!("{committed} {tail}")
            };
            daemon.lock().unwrap().ui.lock().unwrap().preview = preview;
        }
    }
}
