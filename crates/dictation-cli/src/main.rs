//! `dictation` — cliente fino: envia um comando ao `dictationd` pelo socket e imprime a resposta.
//!
//! Uso: `dictation [toggle|start|stop|status]` (padrão: `toggle`).

use dictation_core::Config;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

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
    let cmd = std::env::args().nth(1).unwrap_or_else(|| "toggle".into());
    let cfg = Config::load_or_default(&config_path());
    let sock = cfg.socket_path();

    let mut stream = match UnixStream::connect(&sock) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("daemon indisponível em {}: {e}", sock.display());
            std::process::exit(1);
        }
    };
    let _ = stream.write_all(format!("{cmd}\n").as_bytes());
    let mut reply = String::new();
    let _ = BufReader::new(stream).read_line(&mut reply);
    print!("{reply}");
}
