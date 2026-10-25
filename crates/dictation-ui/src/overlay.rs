//! O app do overlay (egui/eframe): badge REC pulsante + bolha de prévia ao vivo.
//!
//! Uma única janela transparente, click-through e always-on-top, reancorada na
//! janela em foco (badge em `indicator_anchor`; bolha em `preview_anchor`).
//!
//! **Tamanho fixo** por estado (só-badge vs com-texto): a janela não redimensiona
//! a cada atualização da prévia, então o badge REC não "pula" (a borda superior
//! fica estável, já que a âncora é na base).

use crate::anchor;
use crate::geometry::{self, GeometryProvider};
use crate::state::SharedUi;
#[cfg(windows)]
use crate::win_window as window_helper;
use eframe::egui;
use std::time::Duration;

/// Tamanho da janela só-badge.
const BADGE: egui::Vec2 = egui::vec2(74.0, 38.0);
/// Tamanho da janela com prévia (fixo, para o layout não oscilar).
const BUBBLE: egui::Vec2 = egui::vec2(640.0, 110.0);
/// Máximo de caracteres exibidos na prévia (mostra o fim, mais recente).
const PREVIEW_MAX: usize = 300;

pub struct OverlayApp {
    pub state: SharedUi,
    pub indicator_anchor: String,
    pub preview_anchor: String,
    phase: f32,
    last_reposition: f64,
    last_size: (f32, f32),
    last_win: Option<anchor::Rect>,
    /// Último estado de visibilidade enviado (evita comandos redundantes).
    visible: Option<bool>,
    /// Fonte de geometria (janela em foco + monitor) da sessão atual.
    geom: Box<dyn GeometryProvider>,
}

impl OverlayApp {
    pub fn new(state: SharedUi, indicator_anchor: String, preview_anchor: String) -> Self {
        Self {
            state,
            indicator_anchor,
            preview_anchor,
            phase: 0.0,
            last_reposition: -1.0,
            last_size: (0.0, 0.0),
            last_win: None,
            visible: None,
            geom: geometry::provider(),
        }
    }
}

/// Retângulo onde o overlay se ancora: a janela ativa, se conhecida; senão a tela.
fn anchor_target(win: Option<anchor::Rect>, screen: impl FnOnce() -> anchor::Rect) -> anchor::Rect {
    win.unwrap_or_else(screen)
}

/// Mostra o fim do texto (o mais recente), limitado a `max` caracteres.
fn tail(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        return text.to_string();
    }
    let skip = count - max;
    format!("…{}", text.chars().skip(skip).collect::<String>())
}

/// Tamanho da janela para o estado atual (só-badge / "transcrevendo…" / com prévia).
pub(crate) fn overlay_size(recording: bool, preview: &str) -> egui::Vec2 {
    if !preview.is_empty() {
        BUBBLE
    } else if recording {
        BADGE
    } else {
        // "transcrevendo…" precisa de mais largura que "REC".
        egui::vec2(170.0, 38.0)
    }
}

/// Desenha o conteúdo do overlay (badge REC pulsante / bolha de prévia) num `Ui` e
/// devolve o **tamanho** do estado atual.
///
/// Compartilhado pelo backend `eframe` (X11/Windows) e pelo layer-shell (Wayland),
/// para os dois renderizarem exatamente a mesma UI.
pub(crate) fn content(ui: &mut egui::Ui, recording: bool, preview: &str, pulse: f32) -> egui::Vec2 {
    let has_text = !preview.is_empty();
    let size = overlay_size(recording, preview);
    let shown = tail(preview, PREVIEW_MAX);

    let frame = egui::Frame::NONE
        .fill(egui::Color32::from_rgba_unmultiplied(18, 18, 18, 205))
        .corner_radius(12.0)
        .inner_margin(egui::Margin::symmetric(12, 8))
        .shadow(egui::epaint::Shadow::NONE);

    frame.show(ui, |ui| {
        let w = size.x - 24.0;
        ui.set_min_width(w);
        ui.set_max_width(w);
        let (label, color) = if recording {
            ("REC", egui::Color32::from_rgb(235, 45, 45))
        } else {
            ("transcrevendo…", egui::Color32::from_rgb(240, 180, 60))
        };
        ui.horizontal(|ui| {
            // Alocação FIXA do ponto (o raio pulsante não muda o layout).
            let d = 16.0;
            let (rect, _) = ui.allocate_exact_size(egui::vec2(d, d), egui::Sense::hover());
            let r = 5.0 + 3.0 * pulse;
            ui.painter().circle_filled(
                rect.center(),
                r,
                egui::Color32::from_rgba_unmultiplied(
                    color.r(),
                    color.g(),
                    color.b(),
                    (180.0 + 75.0 * pulse) as u8,
                ),
            );
            ui.label(egui::RichText::new(label).strong().size(12.0));
        });
        if has_text {
            ui.add_space(2.0);
            ui.add(
                egui::Label::new(egui::RichText::new(&shown).size(14.0))
                    .wrap()
                    .selectable(false),
            );
        }
    });
    size
}

impl eframe::App for OverlayApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    // `native_frame` só é usado no Windows (aplicação dos estilos da janela).
    #[cfg_attr(not(windows), allow(unused_variables))]
    fn ui(&mut self, ui: &mut egui::Ui, native_frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let (recording, transcribing, preview) = {
            let s = self.state.lock().unwrap();
            (s.recording, s.transcribing, s.preview.trim().to_string())
        };
        let has_text = !preview.is_empty();

        // Esconde só quando não há nada para mostrar.
        if !recording && !transcribing && !has_text {
            if self.visible != Some(false) {
                self.visible = Some(false);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
            ctx.request_repaint_after(Duration::from_millis(250));
            return;
        }
        if self.visible != Some(true) {
            self.visible = Some(true);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        }

        self.phase += 0.14;
        let pulse = 0.5 + 0.5 * self.phase.sin();
        let size = content(ui, recording, &preview, pulse);

        // Ajusta a janela ao tamanho fixo do estado (só quando muda).
        if (size.x - self.last_size.0).abs() > 0.5 || (size.y - self.last_size.1).abs() > 0.5 {
            self.last_size = (size.x, size.y);
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        }

        // No Windows, garante o click-through/sem-foco a cada quadro: o winit
        // reaplica os estilos da janela ao processar comandos de viewport.
        #[cfg(windows)]
        {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if let Ok(handle) = native_frame.window_handle() {
                if let RawWindowHandle::Win32(w) = handle.as_raw() {
                    window_helper::apply_overlay_style(w.hwnd.get());
                }
            }
        }

        // Reancora periodicamente (sem depender do tamanho do conteúdo).
        let t = ctx.input(|i| i.time);
        if t - self.last_reposition > 0.7 {
            self.last_reposition = t;
            if let Some(win) = self.geom.active_window_rect() {
                self.last_win = Some(win);
            }
            // Sem janela ativa conhecida (GNOME/Wayland não a expõe), ancora na tela.
            let win = anchor_target(self.last_win, || self.geom.monitor_rect_for(0.0, 0.0));
            let (cx, cy) = win.center();
            let monitor = self.geom.monitor_rect_for(cx, cy);
            let anchor = if has_text {
                &self.preview_anchor
            } else {
                &self.indicator_anchor
            };
            let (px, py) = anchor::anchor_pos(anchor, win, monitor, (size.x, size.y));
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(px, py)));
        }

        ctx.request_repaint_after(Duration::from_millis(60));
    }
}

#[cfg(test)]
mod tests {
    use super::{anchor_target, tail};
    use crate::anchor::Rect;

    #[test]
    fn anchor_prefers_active_window() {
        let win = Rect::new(10.0, 20.0, 800.0, 600.0);
        let t = anchor_target(Some(win), || panic!("nao deve consultar a tela"));
        assert_eq!(t, win);
    }

    #[test]
    fn anchor_falls_back_to_screen() {
        let screen = Rect::new(0.0, 0.0, 1920.0, 1080.0);
        assert_eq!(anchor_target(None, || screen), screen);
    }

    #[test]
    fn tail_keeps_short_text() {
        assert_eq!(tail("olá", 10), "olá");
    }

    #[test]
    fn tail_shows_the_end() {
        let t = tail("abcdefghij", 4);
        assert_eq!(t, "…ghij");
    }
}
