//! Testa o injetor X11 com acentos (digita num campo em foco).
//! Uso: `cargo run -p dictation-platform --example inject`

use dictation_platform::inject::XdotoolInjector;
use dictation_platform::TextInjector;

fn main() {
    let inj = XdotoolInjector::new().with_trailing_space(false);
    inj.inject("fácil você não relatório pontuação aí ção")
        .expect("falha ao injetar");
}
