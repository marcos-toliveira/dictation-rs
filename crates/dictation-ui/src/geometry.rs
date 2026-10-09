//! Abstração da geometria do overlay: **janela em foco** + **monitor**.
//!
//! O overlay depende só desta trait — nunca do backend concreto. Hoje há um backend
//! por SO (Linux→[`crate::x11`], Windows→[`crate::win_window`]); a fase F3 do port
//! Wayland adiciona um `KWinGeometry` (KWin D-Bus) e o [`provider`] passa a escolhê-lo
//! quando a sessão for Wayland (`dictation_platform::session::is_wayland`).
//!
//! Isolar agora evita que o `overlay.rs` fique acoplado ao backend — que é o ponto do F0.

use crate::anchor::Rect;

/// Fonte de geometria (janela em foco e monitores).
///
/// `Send` para poder ser movido para a thread principal do overlay (exigência do winit).
pub trait GeometryProvider: Send {
    /// Retângulo da **janela em foco** (coordenadas do root do compositor).
    ///
    /// Retorna `None` quando a "janela ativa" é o **próprio overlay** (`dictation-rec`)
    /// — senão ele se ancoraria em si mesmo (loop de reposicionamento) — ou quando a
    /// consulta falha.
    fn active_window_rect(&self) -> Option<Rect>;

    /// Monitor que contém o ponto `(cx, cy)`; a tela inteira como fallback.
    fn monitor_rect_for(&self, cx: f32, cy: f32) -> Rect;
}

/// Backend de geometria nativo do SO atual.
#[cfg(target_os = "linux")]
#[derive(Default)]
pub struct NativeGeometry;

#[cfg(target_os = "linux")]
impl GeometryProvider for NativeGeometry {
    fn active_window_rect(&self) -> Option<Rect> {
        crate::x11::active_window_rect()
    }

    fn monitor_rect_for(&self, cx: f32, cy: f32) -> Rect {
        crate::x11::monitor_rect_for(cx, cy)
    }
}

/// Backend de geometria nativo do SO atual.
#[cfg(windows)]
#[derive(Default)]
pub struct NativeGeometry;

#[cfg(windows)]
impl GeometryProvider for NativeGeometry {
    fn active_window_rect(&self) -> Option<Rect> {
        crate::win_window::active_window_rect()
    }

    fn monitor_rect_for(&self, cx: f32, cy: f32) -> Rect {
        crate::win_window::monitor_rect_for(cx, cy)
    }
}

/// Provider de geometria para a **sessão atual**.
///
/// **F0**: o backend nativo do SO. A fase F3 insere, aqui, a seleção por
/// `dictation_platform::session::is_wayland()` devolvendo o backend KWin no Linux.
pub fn provider() -> Box<dyn GeometryProvider> {
    Box::new(NativeGeometry)
}
