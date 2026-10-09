//! Detecção de sessão gráfica (X11 vs Wayland).
//!
//! A escolha de *backend* (injeção, geometria, overlay) é feita em **runtime**: um
//! mesmo binário precisa funcionar tanto numa sessão X11 quanto numa sessão Wayland
//! do Plasma. A detecção fica aqui — pequena, pura e testável — para que os crates
//! superiores não repliquem a lógica de ambiente.
//!
//! Heurística (padrão do ecossistema):
//! - `WAYLAND_DISPLAY` não vazio ⇒ Wayland (mesmo com XWayland presente);
//! - senão `XDG_SESSION_TYPE == "wayland"`.

/// Decide a sessão a partir dos valores das variáveis de ambiente (função **pura**).
///
/// Recebe as variáveis já resolvidas para permitir teste hermético (sem tocar no
/// ambiente global do processo).
pub fn is_wayland_from(wayland_display: Option<&str>, session_type: Option<&str>) -> bool {
    if wayland_display.is_some_and(|wd| !wd.is_empty()) {
        return true;
    }
    session_type.is_some_and(|t| t.eq_ignore_ascii_case("wayland"))
}

/// `true` quando a sessão gráfica atual é **Wayland**.
pub fn is_wayland() -> bool {
    let wayland_display = std::env::var("WAYLAND_DISPLAY").ok();
    let session_type = std::env::var("XDG_SESSION_TYPE").ok();
    is_wayland_from(wayland_display.as_deref(), session_type.as_deref())
}

/// Rótulo curto da sessão atual, para logs/telemetria.
pub fn session_label() -> &'static str {
    if is_wayland() {
        "wayland"
    } else {
        "x11"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wayland_display_set_is_wayland() {
        assert!(is_wayland_from(Some("wayland-0"), None));
        // Wayland com XWayland também exporta DISPLAY, mas isso não é passado aqui.
        assert!(is_wayland_from(Some("wayland-1"), Some("x11")));
    }

    #[test]
    fn session_type_wayland_is_wayland() {
        assert!(is_wayland_from(None, Some("wayland")));
        assert!(is_wayland_from(Some(""), Some("wayland")));
    }

    #[test]
    fn x11_is_not_wayland() {
        assert!(!is_wayland_from(None, Some("x11")));
        assert!(!is_wayland_from(Some(""), Some("x11")));
    }

    #[test]
    fn nothing_set_is_not_wayland() {
        assert!(!is_wayland_from(None, None));
        assert!(!is_wayland_from(Some(""), Some("")));
    }

    #[test]
    fn session_type_is_case_insensitive() {
        assert!(is_wayland_from(None, Some("Wayland")));
        assert!(is_wayland_from(None, Some("WAYLAND")));
    }
}
