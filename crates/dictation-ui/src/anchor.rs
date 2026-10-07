//! Cálculo de ancoragem do overlay (puro e testável, sem display).

/// Retângulo em coordenadas do root (X11).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}

/// Calcula o canto superior-esquerdo de uma janela de tamanho `size`, ancorada em
/// `anchor` dentro de `win` (a janela ativa), limitada por `monitor` (availableGeometry).
///
/// Âncoras: `bottom-right` (padrão), `bottom-center`, `top-right`, `top-left`.
pub fn anchor_pos(anchor: &str, win: Rect, monitor: Rect, size: (f32, f32)) -> (f32, f32) {
    let (sw, sh) = size;
    let margin = 18.0;
    let (px, py) = match anchor {
        "top-right" => (win.x + win.w - sw - margin, win.y + margin),
        "top-left" => (win.x + margin, win.y + margin),
        "bottom-center" => (win.x + win.w / 2.0 - sw / 2.0, win.y + win.h - sh - margin),
        // bottom-right (padrão)
        _ => (win.x + win.w - sw - margin, win.y + win.h - sh - margin),
    };
    clamp_to_monitor(px, py, size, monitor)
}

/// Limita a posição para caber no monitor (como `availableGeometry` do Qt).
pub fn clamp_to_monitor(px: f32, py: f32, size: (f32, f32), monitor: Rect) -> (f32, f32) {
    let (sw, sh) = size;
    let min_x = monitor.x;
    let max_x = (monitor.x + monitor.w - sw).max(min_x);
    let min_y = monitor.y;
    let max_y = (monitor.y + monitor.h - sh).max(min_y);
    (px.clamp(min_x, max_x), py.clamp(min_y, max_y))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win() -> Rect {
        Rect::new(100.0, 100.0, 800.0, 600.0)
    }
    fn mon() -> Rect {
        Rect::new(0.0, 0.0, 1920.0, 1080.0)
    }

    #[test]
    fn bottom_right_anchors_inside_window() {
        let (x, y) = anchor_pos("bottom-right", win(), mon(), (74.0, 38.0));
        assert_eq!(x, 100.0 + 800.0 - 74.0 - 18.0);
        assert_eq!(y, 100.0 + 600.0 - 38.0 - 18.0);
    }

    #[test]
    fn bottom_center_is_horizontally_centered() {
        let (x, _) = anchor_pos("bottom-center", win(), mon(), (300.0, 40.0));
        assert_eq!(x, 100.0 + 400.0 - 150.0);
    }

    #[test]
    fn top_left_uses_margin() {
        let (x, y) = anchor_pos("top-left", win(), mon(), (74.0, 38.0));
        assert_eq!((x, y), (118.0, 118.0));
    }

    #[test]
    fn clamps_when_window_is_off_monitor() {
        // janela quase toda fora da tela: a âncora é puxada para dentro do monitor
        let w = Rect::new(-500.0, -500.0, 200.0, 200.0);
        let (x, y) = anchor_pos("bottom-right", w, mon(), (74.0, 38.0));
        assert!(
            x >= 0.0 && y >= 0.0,
            "deve ficar dentro do monitor: {x},{y}"
        );
    }

    #[test]
    fn clamps_to_second_monitor() {
        // monitor à direita (1920..3840): janela nesse monitor não deve vazar p/ o 1º
        let m2 = Rect::new(1920.0, 0.0, 1920.0, 1080.0);
        let w = Rect::new(2000.0, 100.0, 800.0, 600.0);
        let (x, _) = anchor_pos("bottom-right", w, m2, (74.0, 38.0));
        assert!(x >= 1920.0, "deve ficar no monitor 2: {x}");
    }
}
