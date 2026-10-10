//! Geometria para a sessão Wayland do **KWin** (Plasma 6).
//!
//! No Wayland, `xdotool`/`xrandr` não valem. Usamos *shell-out* (como o resto do
//! projeto faz com `xdotool`/`xrandr`/`ffmpeg`):
//!
//! - **`kdotool`** (clone do `xdotool` para KWin, via API de scripting) para a
//!   **janela em foco**;
//! - **`kscreen-doctor -j`** (Plasma) para os **monitores**.
//!
//! Degradação graciosa: se faltarem, a janela ativa vira `None` (o overlay mantém a
//! última posição conhecida) e o monitor cai para a tela inteira.

use crate::anchor::Rect;
use crate::geometry::GeometryProvider;

/// Backend de geometria para a sessão Wayland (KWin).
#[derive(Default)]
pub struct KWinGeometry;

impl GeometryProvider for KWinGeometry {
    fn active_window_rect(&self) -> Option<Rect> {
        // Guarda anti-loop: a "janela ativa" não pode ser o próprio overlay.
        let name = run("kdotool", &["getactivewindow", "getwindowname"])?;
        if name.trim() == "dictation-rec" {
            return None;
        }
        let geometry = run("kdotool", &["getactivewindow", "getwindowgeometry"])?;
        parse_kdotool_geometry(&geometry)
    }

    fn monitor_rect_for(&self, cx: f32, cy: f32) -> Rect {
        let monitors = kscreen_monitors().unwrap_or_default();
        if let Some(m) = monitors.iter().find(|m| contains(m, cx, cy)) {
            return *m;
        }
        bounding_box(&monitors).unwrap_or(Rect::new(0.0, 0.0, 1920.0, 1080.0))
    }
}

/// Executa `cmd args` e devolve o `stdout` (só quando o processo termina com sucesso).
fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(cmd).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// O ponto `(x, y)` está dentro do retângulo `r`?
fn contains(r: &Rect, x: f32, y: f32) -> bool {
    x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h
}

/// Menor retângulo que contém todos (tela virtual), ou `None` se a lista for vazia.
fn bounding_box(rects: &[Rect]) -> Option<Rect> {
    let first = rects.first()?;
    let mut min_x = first.x;
    let mut min_y = first.y;
    let mut max_x = first.x + first.w;
    let mut max_y = first.y + first.h;
    for r in rects.iter().skip(1) {
        min_x = min_x.min(r.x);
        min_y = min_y.min(r.y);
        max_x = max_x.max(r.x + r.w);
        max_y = max_y.max(r.y + r.h);
    }
    Some(Rect::new(min_x, min_y, max_x - min_x, max_y - min_y))
}

/// Parseia a saída do `kdotool getactivewindow getwindowgeometry`:
///
/// ```text
/// Window {xxxxxxxx-xxxx-...}
///   Position: 100,200
///   Geometry: 800x600
/// ```
///
/// (Função pura — testável sem KWin.)
pub fn parse_kdotool_geometry(text: &str) -> Option<Rect> {
    let mut x = None;
    let mut y = None;
    let mut w = None;
    let mut h = None;
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Position:") {
            let (a, b) = rest.trim().split_once(',')?;
            x = a.trim().parse().ok();
            y = b.trim().parse().ok();
        } else if let Some(rest) = line.strip_prefix("Geometry:") {
            let (a, b) = rest.trim().split_once('x')?;
            w = a.trim().parse().ok();
            h = b.trim().parse().ok();
        }
    }
    Some(Rect::new(x?, y?, w?, h?))
}

#[derive(serde::Deserialize)]
struct KscreenRoot {
    outputs: Vec<KscreenOutput>,
}

#[derive(serde::Deserialize)]
struct KscreenOutput {
    enabled: bool,
    connected: bool,
    pos: KscreenPoint,
    /// Tamanho do **modo** (pixels de dispositivo). A área lógica é `size / scale`.
    #[serde(default)]
    size: Option<KscreenSize>,
    #[serde(default)]
    scale: Option<f32>,
    /// Alguns `kscreen` expõem a geometria lógica pronta; quando existe, tem prioridade.
    #[serde(default)]
    geometry: Option<KscreenGeometry>,
}

#[derive(serde::Deserialize)]
struct KscreenPoint {
    x: f32,
    y: f32,
}

#[derive(serde::Deserialize)]
struct KscreenSize {
    width: f32,
    height: f32,
}

#[derive(serde::Deserialize)]
struct KscreenGeometry {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl KscreenOutput {
    /// Geometria **lógica** (área de trabalho) do output, em coordenadas do compositor.
    ///
    /// `kscreen-doctor` não tem `geometry` em todas as versões; nesse caso usa `size`
    /// (pixels do modo) dividido por `scale` — `size` **não** é a medida física em mm
    /// (essa é `sizeMM`).
    fn rect(&self) -> Option<Rect> {
        if let Some(g) = &self.geometry {
            if g.width > 0.0 && g.height > 0.0 {
                return Some(Rect::new(g.x, g.y, g.width, g.height));
            }
        }
        let s = self.size.as_ref()?;
        let scale = self.scale.filter(|s| *s > 0.0).unwrap_or(1.0);
        let (w, h) = (s.width / scale, s.height / scale);
        (w > 0.0 && h > 0.0).then(|| Rect::new(self.pos.x, self.pos.y, w, h))
    }
}

/// Monitores ativos via `kscreen-doctor -j`.
fn kscreen_monitors() -> Option<Vec<Rect>> {
    let json = run("kscreen-doctor", &["-j"])?;
    parse_kscreen_monitors(&json)
}

/// Parseia o JSON do `kscreen-doctor -j`, mantendo só outputs **enabled** e **connected**.
///
/// (Função pura — testável sem Plasma.)
pub fn parse_kscreen_monitors(json: &str) -> Option<Vec<Rect>> {
    let root: KscreenRoot = serde_json::from_str(json).ok()?;
    Some(
        root.outputs
            .into_iter()
            .filter(|o| o.enabled && o.connected)
            .filter_map(|o| o.rect())
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_kdotool_geometry() {
        let s = "Window {abc-def}\n  Position: 100,200\n  Geometry: 800x600\n";
        assert_eq!(
            parse_kdotool_geometry(s),
            Some(Rect::new(100.0, 200.0, 800.0, 600.0))
        );
    }

    #[test]
    fn rejects_incomplete_geometry() {
        // Sem "Geometry:" não há retângulo.
        assert_eq!(
            parse_kdotool_geometry("Window {x}\n  Position: 1,2\n"),
            None
        );
        assert_eq!(parse_kdotool_geometry(""), None);
    }

    #[test]
    fn parses_kscreen_outputs_filters_inactive() {
        let json = r#"{"outputs":[
            {"enabled":true,"connected":true,"pos":{"x":0,"y":0},"size":{"width":1920,"height":1080},"scale":1},
            {"enabled":false,"connected":true,"pos":{"x":1920,"y":0},"size":{"width":1280,"height":720},"scale":1},
            {"enabled":true,"connected":true,"pos":{"x":1920,"y":0},"size":{"width":1280,"height":720},"scale":1}
        ]}"#;
        let m = parse_kscreen_monitors(json).unwrap();
        assert_eq!(m.len(), 2);
        assert_eq!(m[0], Rect::new(0.0, 0.0, 1920.0, 1080.0));
        assert_eq!(m[1], Rect::new(1920.0, 0.0, 1280.0, 720.0));
    }

    #[test]
    fn kscreen_scale_divides_size() {
        // HiDPI: modo 3840x2160 com scale 2 → área lógica 1920x1080.
        let json = r#"{"outputs":[{"enabled":true,"connected":true,"pos":{"x":0,"y":0},
            "size":{"width":3840,"height":2160},"scale":2}]}"#;
        assert_eq!(
            parse_kscreen_monitors(json),
            Some(vec![Rect::new(0.0, 0.0, 1920.0, 1080.0)])
        );
    }

    #[test]
    fn kscreen_geometry_field_wins() {
        // Quando `geometry` existe, ele tem prioridade sobre `size`/`scale`.
        let json = r#"{"outputs":[{"enabled":true,"connected":true,"pos":{"x":0,"y":0},
            "size":{"width":3840,"height":2160},"scale":2,
            "geometry":{"x":10,"y":20,"width":800,"height":600}}]}"#;
        assert_eq!(
            parse_kscreen_monitors(json),
            Some(vec![Rect::new(10.0, 20.0, 800.0, 600.0)])
        );
    }

    #[test]
    fn invalid_json_is_none() {
        assert_eq!(parse_kscreen_monitors("not json"), None);
    }

    #[test]
    fn bounding_box_spans_all() {
        let r = vec![
            Rect::new(0.0, 0.0, 100.0, 100.0),
            Rect::new(200.0, 50.0, 100.0, 100.0),
        ];
        assert_eq!(bounding_box(&r), Some(Rect::new(0.0, 0.0, 300.0, 150.0)));
        assert_eq!(bounding_box(&[]), None);
    }
}
