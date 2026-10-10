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

/// O executável `name` está no `PATH`?
#[cfg(target_os = "linux")]
fn command_exists(name: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
        .unwrap_or(false)
}

/// `true` quando `XDG_CURRENT_DESKTOP` indica KDE/Plasma (função pura).
#[cfg(target_os = "linux")]
fn is_kde_desktop_from(current: Option<&str>) -> bool {
    current.is_some_and(|v| v.to_ascii_uppercase().contains("KDE"))
}

/// `true` quando a sessão atual é KDE/Plasma.
#[cfg(target_os = "linux")]
fn is_kde_desktop() -> bool {
    is_kde_desktop_from(std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref())
}

/// Provider de geometria para a **sessão atual**.
///
/// No Linux, escolhe em **runtime**: **Wayland no KWin (Plasma) → [`crate::kwin::KWinGeometry`]**.
/// O backend KWin usa `kdotool` **e** `kscreen-doctor`, então exige **ambos** no `PATH` e o
/// desktop KDE; senão usa o backend nativo X11 (também vale no XWayland/GNOME). No Windows, o nativo.
pub fn provider() -> Box<dyn GeometryProvider> {
    #[cfg(target_os = "linux")]
    {
        // KWin (Plasma): só quando AMBOS os tools existem (senão `monitor_rect_for` cairia no
        // retângulo fixo 1920x1080) e o desktop é KDE. Sem isso (ex.: GNOME), o overlay roda em
        // XWayland e usa a geometria X11 (monitores/tela; a janela ativa fica `None`).
        if dictation_platform::session::is_wayland()
            && is_kde_desktop()
            && command_exists("kdotool")
            && command_exists("kscreen-doctor")
        {
            return Box::new(crate::kwin::KWinGeometry);
        }
    }
    Box::new(NativeGeometry)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::is_kde_desktop_from;

    #[test]
    fn detects_kde_desktop() {
        assert!(is_kde_desktop_from(Some("KDE")));
        assert!(is_kde_desktop_from(Some("kde-plasma")));
        assert!(!is_kde_desktop_from(Some("GNOME")));
        assert!(!is_kde_desktop_from(None));
    }
}
