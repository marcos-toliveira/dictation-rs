//! `dictation-ui` — overlay flutuante (REC + prévia) e bandeja do dictation-rs.
//!
//! Janela **frameless, translúcida, always-on-top, click-through e sem roubar foco**,
//! ancorada na janela em foco (multi-monitor), como no `dictation-indicator` do Python.

use eframe::egui;

pub mod anchor;
pub mod overlay;
pub mod state;
pub mod tray;
pub mod x11;

/// Opções da janela do overlay (frameless, translúcida, click-through, sempre no topo).
pub fn overlay_options(size: [f32; 2]) -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_mouse_passthrough(true)
            .with_window_type(egui::X11WindowType::Tooltip)
            // Janela NÃO gerenciada pelo WM: não rouba foco nem aparece na barra/taskbar.
            .with_override_redirect(true)
            .with_resizable(false)
            .with_taskbar(false)
            .with_inner_size(size),
        ..Default::default()
    }
}

/// Roda o overlay (deve ser chamado na **thread principal** — exigência do winit).
pub fn run_overlay(
    state: state::SharedUi,
    indicator_anchor: String,
    preview_anchor: String,
) -> eframe::Result<()> {
    let options = overlay_options([74.0, 38.0]);
    eframe::run_native(
        "dictation-rec",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(overlay::OverlayApp::new(
                state,
                indicator_anchor,
                preview_anchor,
            )))
        }),
    )
}
