//! Lista os dispositivos de entrada do cpal e seus formatos.
//! Uso: `cargo run -p dictation-platform --example devices`

use cpal::traits::{DeviceTrait, HostTrait};

fn main() {
    let host = cpal::default_host();
    println!("host: {:?}", host.id());
    match host.input_devices() {
        Ok(devices) => {
            for d in devices {
                let name = d.name().unwrap_or_else(|_| "?".into());
                let cfg = d
                    .default_input_config()
                    .map(|c| {
                        format!(
                            "{:?} {}ch {}Hz",
                            c.sample_format(),
                            c.channels(),
                            c.sample_rate().0
                        )
                    })
                    .unwrap_or_else(|e| format!("erro: {e}"));
                println!("- {name}: {cfg}");
            }
        }
        Err(e) => eprintln!("erro ao enumerar: {e}"),
    }
    let default = host
        .default_input_device()
        .and_then(|d| d.name().ok())
        .unwrap_or_else(|| "<nenhum>".into());
    println!("default: {default}");
}
