//! Abstração da geometria do overlay: **janela em foco** + **monitor**.
//!
//! O overlay depende só desta trait — nunca do backend concreto. Há um backend por
//! SO/sessão: Linux/X11 → [`crate::x11`], Linux/Wayland → [`crate::kwin`],
//! Windows → [`crate::win_window`]. O [`provider`] escolhe em runtime pela sessão
//! (`dictation_platform::session::is_wayland`).
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
/// No Linux, escolhe em **runtime**: **Wayland → [`crate::kwin::KWinGeometry`]**
/// (`kdotool` + `kscreen-doctor`); senão o backend nativo X11. No Windows, o nativo.
pub fn provider() -> Box<dyn GeometryProvider> {
    #[cfg(target_os = "linux")]
    {
        if dictation_platform::session::is_wayland() {
            return Box::new(crate::kwin::KWinGeometry);
        }
    }
    Box::new(NativeGeometry)
}
