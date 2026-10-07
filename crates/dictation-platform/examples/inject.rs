//! Injeta um texto no campo em foco.
//! Uso: cargo run -p dictation-platform --example inject -- [--clipboard] "texto"

use dictation_platform::inject::{ClipboardInjector, XdotoolInjector};
use dictation_platform::TextInjector;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mode, text) = if args.first().map(|s| s == "--clipboard").unwrap_or(false) {
        (
            "clipboard",
            args.get(1).cloned().unwrap_or_else(|| "você não".into()),
        )
    } else {
        (
            "type",
            args.first()
                .cloned()
                .unwrap_or_else(|| "você não você não".into()),
        )
    };
    let inj: Box<dyn TextInjector> = if mode == "clipboard" {
        Box::new(ClipboardInjector::new().with_trailing_space(false))
    } else {
        Box::new(XdotoolInjector::new().with_trailing_space(false))
    };
    inj.inject(&text).expect("falha ao injetar");
}
