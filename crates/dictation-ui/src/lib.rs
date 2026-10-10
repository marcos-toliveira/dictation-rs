//! `dictation-ui` — overlay flutuante (REC + prévia) e bandeja do dictation-rs.
//!
//! Janela **frameless, translúcida, always-on-top, click-through e sem roubar foco**,
//! ancorada na janela em foco (multi-monitor), como no `dictation-indicator` do Python.

use eframe::egui;

pub mod anchor;
pub mod geometry;
#[cfg(target_os = "linux")]
pub mod kwin;
pub mod overlay;
#[cfg(all(target_os = "linux", feature = "wayland"))]
pub mod overlay_wayland;
pub mod state;
pub mod tray;
#[cfg(windows)]
pub mod win_icon;
#[cfg(windows)]
pub mod win_window;
#[cfg(target_os = "linux")]
pub mod x11;

/// Opções da janela do overlay (frameless, translúcida, click-through, sempre no topo).
pub fn overlay_options(size: [f32; 2]) -> eframe::NativeOptions {
    #[allow(unused_mut)]
    let mut builder = egui::ViewportBuilder::default()
        .with_decorations(false)
        .with_transparent(true)
        .with_always_on_top()
        .with_resizable(false)
        .with_taskbar(false)
        .with_inner_size(size);

    // No Linux, o click-through vem do winit/egui. No Windows,
    // `with_mouse_passthrough` força `WS_EX_LAYERED` — e o glow/OpenGL **não
    // apresenta** numa janela *layered*. Lá aplicamos `WS_EX_TRANSPARENT`
    // diretamente no `HWND` (ver `win_window::apply_overlay_style`).
    #[cfg(target_os = "linux")]
    {
        builder = builder
            .with_mouse_passthrough(true)
            .with_window_type(egui::X11WindowType::Tooltip)
            // Janela NÃO gerenciada pelo WM: não rouba foco nem aparece na barra/taskbar.
            .with_override_redirect(true);
    }

    eframe::NativeOptions {
        viewport: builder,
        ..Default::default()
    }
}

/// Roda o overlay (deve ser chamado na **thread principal** — exigência do winit).
pub fn run_overlay(
    state: state::SharedUi,
    indicator_anchor: String,
    preview_anchor: String,
) -> eframe::Result<()> {
    run_overlay_with(state, indicator_anchor, preview_anchor, false)
}

/// Overlay forçado em **X11/XWayland**, mesmo numa sessão Wayland.
///
/// Para compositores sem `wlr-layer-shell` (GNOME/Mutter): a janela `override_redirect`
/// não é gerenciada pelo compositor, então **não rouba foco** (medido no GNOME 46). O
/// `WAYLAND_DISPLAY` do processo fica intacto (`wl-copy`/`ydotool` dependem dele).
/// Exige `DISPLAY` (XWayland). Deve ser chamado na **thread principal**.
#[cfg(target_os = "linux")]
pub fn run_overlay_xwayland(
    state: state::SharedUi,
    indicator_anchor: String,
    preview_anchor: String,
) -> eframe::Result<()> {
    run_overlay_with(state, indicator_anchor, preview_anchor, true)
}

#[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
fn run_overlay_with(
    state: state::SharedUi,
    indicator_anchor: String,
    preview_anchor: String,
    force_x11: bool,
) -> eframe::Result<()> {
    #[allow(unused_mut)]
    let mut options = overlay_options([74.0, 38.0]);
    #[cfg(target_os = "linux")]
    if force_x11 {
        options.event_loop_builder = Some(Box::new(|builder| {
            use winit::platform::x11::EventLoopBuilderExtX11;
            builder.with_x11();
        }));
    }
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
