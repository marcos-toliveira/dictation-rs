//! Demo: overlay com estado simulado (REC + prévia ao vivo).
//! Uso: `cargo run -p dictation-ui --example overlay`

use std::time::Duration;

fn main() -> eframe::Result<()> {
    let state = dictation_ui::state::shared();
    let s2 = state.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(600));
        s2.lock().unwrap().recording = true;
        std::thread::sleep(Duration::from_millis(1600));
        s2.lock().unwrap().preview =
            "olá, isto é uma prévia ao vivo do ditado por voz funcionando no Rust, com quebra de linha automática e ancoragem na janela em foco".into();
        std::thread::sleep(Duration::from_secs(5));
        {
            let mut s = s2.lock().unwrap();
            s.recording = false;
            s.preview.clear();
        }
    });
    dictation_ui::run_overlay(state, "bottom-right".into(), "bottom-center".into())
}
