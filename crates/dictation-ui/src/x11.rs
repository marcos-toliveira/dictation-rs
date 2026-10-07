//! Geometria X11 via ferramentas externas (`xdotool`/`xrandr`), como no Python.
//!
//! O overlay precisa saber onde está a **janela em foco** e qual **monitor** a contém
//! (multi-monitor: `xdotool` usa coordenadas do root; `xrandr` dá a geometria por monitor).

use crate::anchor::Rect;
use std::process::Command;

/// Geometria da janela ativa (coordenadas do root), via `xdotool`.
///
/// Retorna `None` quando a "janela ativa" é o **próprio overlay** (`dictation-rec`) —
/// senão ele se ancoraria em si mesmo (loop de reposicionamento).
pub fn active_window_rect() -> Option<Rect> {
    let name = Command::new("xdotool")
        .args(["getactivewindow", "getwindowname"])
        .output()
        .ok()?;
    if !name.status.success() {
        return None;
    }
    if String::from_utf8_lossy(&name.stdout).trim() == "dictation-rec" {
        return None;
    }
    let out = Command::new("xdotool")
        .args(["getactivewindow", "getwindowgeometry", "--shell"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut x = None;
    let mut y = None;
    let mut w = None;
    let mut h = None;
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let v = v.trim().parse::<f32>().ok();
        match k.trim() {
            "X" => x = v,
            "Y" => y = v,
            "WIDTH" => w = v,
            "HEIGHT" => h = v,
            _ => {}
        }
    }
    Some(Rect::new(x?, y?, w?, h?))
}

/// Tamanho da tela (root) via `xdotool getdisplaygeometry`.
pub fn display_size() -> Option<(f32, f32)> {
    let out = Command::new("xdotool")
        .arg("getdisplaygeometry")
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut it = text.split_whitespace();
    let w = it.next()?.parse().ok()?;
    let h = it.next()?.parse().ok()?;
    Some((w, h))
}

/// Parseia a saída de `xrandr --listactivemonitors` em retângulos (função pura, testável).
pub fn parse_active_monitors(text: &str) -> Vec<Rect> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("Monitors:") {
            continue;
        }
        // tokens: "0: +*eDP-1 1920/344x1080/193+0+0  eDP-1"
        let Some(geo) = line
            .split_whitespace()
            .find(|t| t.contains('x') && t.matches('+').count() >= 2)
        else {
            continue;
        };
        let Some((wh, xy)) = geo.split_once('+') else {
            continue;
        };
        let (wpart, hpart) = wh.split_once('x').unwrap_or((wh, "0"));
        let w: f32 = wpart
            .split('/')
            .next()
            .unwrap_or("0")
            .parse()
            .unwrap_or(0.0);
        let h: f32 = hpart
            .split('/')
            .next()
            .unwrap_or("0")
            .parse()
            .unwrap_or(0.0);
        let mut parts = xy.split('+');
        let x: f32 = parts.next().unwrap_or("0").parse().unwrap_or(0.0);
        let y: f32 = parts.next().unwrap_or("0").parse().unwrap_or(0.0);
        if w > 0.0 && h > 0.0 {
            out.push(Rect::new(x, y, w, h));
        }
    }
    out
}

/// Monitor que contém o ponto `(cx, cy)`; se não achar, devolve a tela inteira.
pub fn monitor_rect_for(cx: f32, cy: f32) -> Rect {
    if let Ok(out) = Command::new("xrandr").arg("--listactivemonitors").output() {
        if out.status.success() {
            let monitors = parse_active_monitors(&String::from_utf8_lossy(&out.stdout));
            if let Some(m) = monitors
                .iter()
                .find(|m| cx >= m.x && cx < m.x + m.w && cy >= m.y && cy < m.y + m.h)
            {
                return *m;
            }
        }
    }
    let (w, h) = display_size().unwrap_or((1920.0, 1080.0));
    Rect::new(0.0, 0.0, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_two_monitors() {
        let sample = "Monitors: 2\n \
             0: +*eDP-1 1920/344x1080/193+0+0  eDP-1\n \
             1: +HDMI-1 1280/286x720/161+1920+0  HDMI-1\n";
        let m = parse_active_monitors(sample);
        assert_eq!(m.len(), 2);
        assert_eq!(m[0], Rect::new(0.0, 0.0, 1920.0, 1080.0));
        assert_eq!(m[1], Rect::new(1920.0, 0.0, 1280.0, 720.0));
    }

    #[test]
    fn ignores_garbage() {
        assert!(parse_active_monitors("Monitors: 1\nnada\n").is_empty());
    }
}
