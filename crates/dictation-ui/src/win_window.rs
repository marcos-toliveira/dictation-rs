//! Geometria de janelas e monitores no Windows nativo (via Win32 API).
//!
//! Fornece:
//! * Coordenadas da janela em foco ([`active_window_rect`]).
//! * Geometria da área de trabalho do monitor ([`monitor_rect_for`]), respeitando a taskbar.
//! * Aplicação dos estilos `WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE`
//!   para garantir que o overlay nunca roube foco e seja click-through.

use crate::anchor::Rect;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowLongPtrW, GetWindowRect, GetWindowTextW, SetWindowLongPtrW,
    SetWindowPos, GWL_EXSTYLE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_NOZORDER, WS_EX_NOACTIVATE, WS_EX_TRANSPARENT,
};

/// Retorna o retângulo da janela em foco atual (excluindo o próprio overlay).
pub fn active_window_rect() -> Option<Rect> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0 == 0 {
            return None;
        }

        // Não se auto-ancorar no próprio overlay
        let mut title = [0u16; 64];
        let len = GetWindowTextW(hwnd, &mut title);
        let title_str = String::from_utf16_lossy(&title[..len as usize]);
        if title_str == "dictation-rec" {
            return None;
        }

        let mut r = windows::Win32::Foundation::RECT::default();
        if GetWindowRect(hwnd, &mut r).is_ok() {
            let w = (r.right - r.left).max(0) as f32;
            let h = (r.bottom - r.top).max(0) as f32;
            if w > 10.0 && h > 10.0 {
                return Some(Rect::new(r.left as f32, r.top as f32, w, h));
            }
        }
        None
    }
}

/// Retorna a área de trabalho do monitor que contém as coordenadas (cx, cy).
///
/// Usa `rcWork` do `MONITORINFO`, que já desconta a barra de tarefas do Windows.
pub fn monitor_rect_for(cx: f32, cy: f32) -> Rect {
    unsafe {
        let pt = POINT {
            x: cx as i32,
            y: cy as i32,
        };
        let hmon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        if hmon.0 != 0 {
            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if GetMonitorInfoW(hmon, &mut mi).as_bool() {
                let r = mi.rcWork;
                let w = (r.right - r.left).max(0) as f32;
                let h = (r.bottom - r.top).max(0) as f32;
                return Rect::new(r.left as f32, r.top as f32, w, h);
            }
        }
        Rect::new(0.0, 0.0, 1920.0, 1080.0)
    }
}

/// Aplica `WS_EX_TRANSPARENT | WS_EX_NOACTIVATE` na janela do overlay
/// (click-through e sem roubo de foco).
///
/// **Sem** `WS_EX_LAYERED`: numa janela *layered* o glow/OpenGL não apresenta
/// no Windows (a janela aparece, mas sem conteúdo).
pub fn apply_overlay_style(hwnd: isize) -> bool {
    unsafe {
        if hwnd == 0 {
            return false;
        }
        let hwnd = HWND(hwnd);
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let needed = (WS_EX_TRANSPARENT.0 | WS_EX_NOACTIVATE.0) as isize;
        if (ex_style & needed) != needed {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style | needed);
            let _ = SetWindowPos(
                hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
        }
        true
    }
}
