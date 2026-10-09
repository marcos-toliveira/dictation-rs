//! `dictationd` — daemon do dictation-rs com GUI.
//!
//! Thread principal: overlay (egui/eframe). Threads: socket local / named pipe, captura (`cpal`),
//! prévia ao vivo (Groq) e bandeja (Linux: `ksni` / Windows: `Shell_NotifyIconW`).
//!
//! Comandos do socket (uma linha): `start | stop | toggle | status | teach <e> <c> [--vocab] | last | quit`.
//! Config: `~/.config/dictation/config.ini` ou `%APPDATA%\dictation\config.ini` ou `$DICTATION_CONFIG`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use dictation_core::{AsrEngine, AsrOptions, Config, InjectMode, Provider};
use dictation_daemon::{read_last, save_last, teach, Engine, FallbackAsr, LocalWhisper, RetryAsr};
use dictation_groq::GroqEngine;
use dictation_platform::capture::CpalCapture;
#[cfg(target_os = "linux")]
use dictation_platform::capture::FfmpegCapture;
use dictation_platform::inject::{PlatformClipboardInjector, PlatformTypeInjector, StdoutInjector};
#[cfg(target_os = "linux")]
use dictation_platform::inject::{WlClipboardInjector, YdotoolTypeInjector};
use dictation_platform::{AudioCapture, TextInjector};
use dictation_ui::state::{self, SharedUi};
use dictation_ui::tray::{spawn_tray, TrayAction, TrayHandle};
use std::io::{BufRead, BufReader, Write};
#[cfg(not(windows))]
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
                    _ => {
                        // Sem argumentos: abre o diálogo (fora do lock).
                        let cfg = self.cfg.clone();
                        std::thread::spawn(move || teach_dialog(&cfg));
                        "aberto".into()
                    }
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
        let mode = self.cfg.capture.clone();
        self.capture = Some(std::thread::spawn(move || {
            let mut cap = build_capture(&mode, device);
            if let Err(e) = cap.start() {
                tracing::error!(error = %e, "falha ao iniciar captura");
                return;
            }
            while !stop.load(Ordering::SeqCst) {
                match cap.recv() {
                    // Bloco vazio = espera limitada expirou (só recheca o `stop`).
                    Some(block) if !block.is_empty() => {
                        engine.lock().unwrap().push_audio(&block);
                    }
                    Some(_) => {}
                    // Fonte encerrou (ex.: ffmpeg morreu) — sai do loop (o watchdog finaliza).
                    None => break,
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
        self.end_recording();
        self.finish_session();
        "transcrevendo".to_string()
    }

    /// Encerra o estado de gravação (captura + UI + bandeja).
    fn end_recording(&mut self) {
        self.recording = false;
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.capture.take() {
            let _ = h.join();
        }
        {
            let mut ui = self.ui.lock().unwrap();
            ui.recording = false;
            ui.transcribing = true; // mostra "transcrevendo…" até injetar
            ui.preview.clear();
        }
        self.update_tray(false);
    }

    /// Transcreve a cauda + finaliza + injeta (em thread separada).
    fn finish_session(&self) {
        let engine = Arc::clone(&self.engine);
        let cfg = self.cfg.clone();
        let ui = Arc::clone(&self.ui);
        std::thread::spawn(move || {
            let text = engine.lock().unwrap().stop_and_finish();
            if let Ok(mut ui) = ui.lock() {
                ui.transcribing = false;
            }
            match text {
                Some(t) => {
                    save_last(&cfg, &t);
                    notify(&cfg, "✅ dictation", &t);
                }
                None => notify(&cfg, "⚠️ dictation", "nada reconhecido"),
            }
        });
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
        // Retry no Groq (429) antes de cair para o local (lento).
        let groq_retry = Arc::new(RetryAsr::new(groq, 3));
        Arc::new(FallbackAsr::new(groq_retry, local()))
    }
}

/// Escolhe o backend de captura. No Linux, prefere `ffmpeg` (robusto); `cpal` fica
/// para o Windows e como opção (`capture = cpal`).
fn build_capture(_mode: &str, device: Option<String>) -> Box<dyn AudioCapture> {
    #[cfg(target_os = "linux")]
    {
        let use_ffmpeg = match _mode {
            "ffmpeg" => true,
            "cpal" => false,
            _ => which("ffmpeg"),
        };
        if use_ffmpeg {
            let mut c = FfmpegCapture::new();
            if let Some(d) = device {
                c = c.with_device(d);
            }
            return Box::new(c);
        }
    }
    let mut c = CpalCapture::new();
    if let Some(d) = device {
        c = c.with_device(d);
    }
    Box::new(c)
}

/// Escolhe o injetor de texto pela **sessão** (Linux: X11 vs Wayland) e pelo modo.
///
/// `inject = clipboard` é o caminho confiável para acentos/Unicode no Wayland
/// (`wl-copy` + Ctrl+V via `ydotool`); `inject = type` usa `ydotool type`.
fn build_injector(cfg: &Config) -> Arc<dyn TextInjector> {
    match cfg.inject {
        InjectMode::Stdout => Arc::new(StdoutInjector),
        InjectMode::Clipboard => {
            #[cfg(target_os = "linux")]
            if dictation_platform::session::is_wayland() {
                return Arc::new(
                    WlClipboardInjector::new().with_trailing_space(cfg.trailing_space),
                );
            }
            Arc::new(PlatformClipboardInjector::new().with_trailing_space(cfg.trailing_space))
        }
        InjectMode::Type => {
            #[cfg(target_os = "linux")]
            if dictation_platform::session::is_wayland() {
                return Arc::new(
                    YdotoolTypeInjector::new()
                        .with_delay_ms(cfg.type_delay_ms)
                        .with_trailing_space(cfg.trailing_space),
                );
            }
            Arc::new(
                PlatformTypeInjector::new()
                    .with_delay_ms(cfg.type_delay_ms)
                    .with_trailing_space(cfg.trailing_space),
            )
        }
    }
}

fn notify(cfg: &Config, title: &str, body: &str) {
    #[cfg(target_os = "linux")]
    if cfg.notify && which("notify-send") {
        let _ = Command::new("notify-send")
            .args(["-a", "dictation", title, body])
            .status();
    }
    #[cfg(windows)]
    if cfg.notify {
        tracing::info!(title = %title, body = %body, "notificação");
    }
}

#[allow(dead_code)]
fn which(prog: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| {
            std::env::split_paths(&p).any(|d| {
                d.join(prog).is_file() || (cfg!(windows) && d.join(format!("{prog}.exe")).is_file())
            })
        })
        .unwrap_or(false)
}

fn open(path: &std::path::Path) {
    #[cfg(target_os = "linux")]
    let _ = Command::new("xdg-open").arg(path).status();
    #[cfg(windows)]
    let _ = Command::new("cmd")
        .args(["/c", "start", "", &path.to_string_lossy()])
        .status();
}

/// Texto selecionado (primary/clipboard) — preenche o campo "Errado".
#[cfg(target_os = "linux")]
fn selection() -> Option<String> {
    for sel in ["primary", "clipboard"] {
        if let Ok(o) = Command::new("xclip")
            .args(["-o", "-selection", sel])
            .output()
        {
            if o.status.success() {
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if !s.is_empty() {
                    return Some(s.chars().take(200).collect());
                }
            }
        }
    }
    None
}

#[cfg(windows)]
fn selection() -> Option<String> {
    arboard::Clipboard::new()
        .ok()
        .and_then(|mut c| c.get_text().ok())
        .map(|s| s.trim().chars().take(200).collect())
        .filter(|s: &String| !s.is_empty())
}

/// Pede uma entrada de texto (zenity ou kdialog no Linux; PowerShell no Windows).
#[cfg(target_os = "linux")]
fn ask(title: &str, text: &str, default: &str) -> Option<String> {
    let out = if which("zenity") {
        Command::new("zenity")
            .args([
                "--entry",
                "--title",
                title,
                "--text",
                text,
                "--entry-text",
                default,
            ])
            .output()
            .ok()?
    } else if which("kdialog") {
        Command::new("kdialog")
            .args(["--title", title, "--inputbox", text, default])
            .output()
            .ok()?
    } else {
        return None;
    };
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(windows)]
fn ask(title: &str, text: &str, default: &str) -> Option<String> {
    let script = format!(
        "[void][System.Reflection.Assembly]::LoadWithPartialName('Microsoft.VisualBasic'); $res = [Microsoft.VisualBasic.Interaction]::InputBox('{}', '{}', '{}'); if ($res) {{ Write-Output $res }}",
        text.replace('\'', "''"),
        title.replace('\'', "''"),
        default.replace('\'', "''")
    );
    let out = Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Diálogo de ensinar correção (F9): preenche "Errado" com a seleção e salva.
fn teach_dialog(cfg: &Config) {
    let wrong = ask(
        "Ensinar correção",
        "Errado (o que o ASR escreveu):",
        &selection().unwrap_or_default(),
    )
    .unwrap_or_default();
    if wrong.trim().is_empty() {
        return;
    }
    let right = ask("Ensinar correção", "Certo (o que deveria ser):", "").unwrap_or_default();
    if right.trim().is_empty() {
        return;
    }
    match teach(cfg, wrong.trim(), right.trim(), false) {
        Ok(()) => notify(
            cfg,
            "✅ dictation",
            &format!("'{}' → '{}'", wrong.trim(), right.trim()),
        ),
        Err(e) => notify(cfg, "⚠️ dictation", &format!("erro: {e}")),
    }
}

fn main() {
    let cfg = Config::load_or_default(&config_path());
    let _ = std::fs::create_dir_all(&cfg.state_dir);

    let log_path = cfg.state_dir.join("dictationd.log");
    if let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        tracing_subscriber::fmt()
            .with_target(false)
            .with_writer(file)
            .with_ansi(false)
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
            )
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_target(false)
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
            )
            .init();
    }

    // Sessão gráfica (X11/Wayland): a escolha de backends ocorre em runtime.
    #[cfg(target_os = "linux")]
    tracing::info!(
        session = dictation_platform::session::session_label(),
        "sessão gráfica detectada"
    );

    let asr = build_asr(&cfg);
    let injector = build_injector(&cfg);
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

    // Bandeja + dispatcher de ações.
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

    // Socket local / named pipe.
    let sock = cfg.socket_path();
    {
        let d = Arc::clone(&daemon);
        std::thread::spawn(move || run_socket(d, sock));
    }

    // No Windows: atalhos globais nativos (F8 toggle / F9 teach) via RegisterHotKey.
    #[cfg(windows)]
    {
        let d = Arc::clone(&daemon);
        std::thread::spawn(move || run_global_hotkeys(d));
    }

    // Prévia ao vivo.
    {
        let d = Arc::clone(&daemon);
        std::thread::spawn(move || run_preview(d));
    }

    // Watchdog: se a captura morrer sozinha, finaliza (não fica preso em "gravando").
    {
        let d = Arc::clone(&daemon);
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(1000));
            let mut daemon = d.lock().unwrap();
            if daemon.recording
                && daemon
                    .capture
                    .as_ref()
                    .map(|h| h.is_finished())
                    .unwrap_or(false)
            {
                tracing::warn!("captura encerrou sozinha; finalizando");
                daemon.end_recording();
                daemon.finish_session();
            }
        });
    }

    tracing::info!("dictationd pronto");
    // Thread principal: overlay (exigência do winit). No Wayland (Linux), o overlay
    // usa layer-shell + egui/wgpu; senão, o eframe/winit.
    #[cfg(target_os = "linux")]
    {
        if dictation_platform::session::is_wayland() {
            dictation_ui::overlay_wayland::run_overlay(
                ui,
                cfg.indicator_anchor.clone(),
                cfg.preview_anchor.clone(),
            );
            return;
        }
    }
    let _ = dictation_ui::run_overlay(ui, cfg.indicator_anchor.clone(), cfg.preview_anchor.clone());
}

#[cfg(not(windows))]
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

#[cfg(windows)]
fn run_socket(daemon: Arc<Mutex<Daemon>>, pipe_path: PathBuf) {
    use std::os::windows::io::FromRawHandle;
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{GetLastError, ERROR_PIPE_CONNECTED, INVALID_HANDLE_VALUE};
    use windows::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
    use windows::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
        PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    };

    let pipe_name = pipe_path.to_string_lossy();
    let wide_name = HSTRING::from(pipe_name.as_ref());
    tracing::info!(pipe = %pipe_name, "dictationd ouvindo via named pipe");

    unsafe {
        loop {
            let handle = CreateNamedPipeW(
                &wide_name,
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                PIPE_UNLIMITED_INSTANCES,
                4096,
                4096,
                0,
                None,
            );

            if handle == INVALID_HANDLE_VALUE {
                tracing::error!(error = ?GetLastError(), "falha ao criar named pipe");
                std::thread::sleep(Duration::from_millis(500));
                continue;
            }

            let connected =
                ConnectNamedPipe(handle, None).is_ok() || GetLastError() == ERROR_PIPE_CONNECTED;

            if connected {
                let d = Arc::clone(&daemon);
                {
                    let mut file =
                        std::fs::File::from_raw_handle(handle.0 as *mut std::ffi::c_void);
                    let mut line = String::new();
                    if BufReader::new(&file).read_line(&mut line).is_ok() {
                        let cmd = line.trim();
                        if !cmd.is_empty() {
                            let reply = d.lock().unwrap().handle(cmd);
                            let _ = file.write_all(format!("{reply}\n").as_bytes());
                            let _ = file.flush();
                        }
                    }
                    // Desconecta ANTES do `file` (dono do handle) ser dropado/fechado.
                    let _ = DisconnectNamedPipe(handle);
                }
            }
        }
    }
}

#[cfg(windows)]
fn run_global_hotkeys(daemon: Arc<Mutex<Daemon>>) {
    use windows::Win32::Foundation::{HWND, WPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, MOD_NOREPEAT, VK_F8, VK_F9};
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, TranslateMessage, MSG, WM_HOTKEY,
    };

    unsafe {
        let ok1 = RegisterHotKey(HWND(0), 1, MOD_NOREPEAT, VK_F8.0 as u32).is_ok();
        let ok2 = RegisterHotKey(HWND(0), 2, MOD_NOREPEAT, VK_F9.0 as u32).is_ok();
        if ok1 {
            tracing::info!("Atalho global F8 registrado (ditar/toggle)");
        } else {
            tracing::warn!("Não foi possível registrar atalho global F8");
        }
        if ok2 {
            tracing::info!("Atalho global F9 registrado (ensinar/teach)");
        } else {
            tracing::warn!("Não foi possível registrar atalho global F9");
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, HWND(0), 0, 0).as_bool() {
            if msg.message == WM_HOTKEY {
                if msg.wParam == WPARAM(1) {
                    daemon.lock().unwrap().handle("toggle");
                } else if msg.wParam == WPARAM(2) {
                    let _ = daemon.lock().unwrap().handle("teach");
                }
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
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
            (e.preview_wav(), Arc::clone(&d.asr), d.opts.clone())
        };
        let (Some(wav), asr, opts) = snapshot else {
            continue;
        };
        if let Ok(t) = asr.transcribe(&wav, &opts) {
            daemon.lock().unwrap().ui.lock().unwrap().preview = t;
        }
    }
}
