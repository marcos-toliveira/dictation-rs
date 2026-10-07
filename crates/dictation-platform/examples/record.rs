//! Exemplo: captura ~2s do microfone via cpal e informa quantos samples (16 kHz) vieram.
//!
//! Uso: `cargo run -p dictation-platform --example record`

use dictation_platform::capture::CpalCapture;
use dictation_platform::AudioCapture;
use std::time::{Duration, Instant};

fn main() {
    let dev = std::env::var("DICT_DEVICE").unwrap_or_else(|_| "pipewire".into());
    let mut capture = CpalCapture::new().with_device(dev);
    if let Err(e) = capture.start() {
        eprintln!("ERRO ao iniciar captura: {e}");
        std::process::exit(1);
    }
    let start = Instant::now();
    let mut total = 0usize;
    while start.elapsed() < Duration::from_secs(2) {
        if let Some(block) = capture.recv() {
            total += block.len();
        }
    }
    let _ = capture.stop();
    println!(
        "captured {total} samples (~{:.2}s @ 16 kHz)",
        total as f32 / 16_000.0
    );
}
