//! O app do overlay (egui/eframe): badge REC pulsante + bolha de prévia ao vivo.
//!
//! Uma única janela transparente, click-through e always-on-top, reancorada na
//! janela em foco (badge em `indicator_anchor`; bolha em `preview_anchor`).
//!
//! **Tamanho fixo** por estado (só-badge vs com-texto): a janela não redimensiona
//! a cada atualização da prévia, então o badge REC não "pula" (a borda superior
//! fica estável, já que a âncora é na base).

use crate::state::SharedUi;
use crate::{anchor, x11};
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
        }
    }
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

impl eframe::App for OverlayApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let (recording, transcribing, preview) = {
            let s = self.state.lock().unwrap();
            (s.recording, s.transcribing, s.preview.trim().to_string())
        };
        let has_text = !preview.is_empty();

        // Esconde só quando não há nada para mostrar.
        if !recording && !transcribing && !has_text {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            ctx.request_repaint_after(Duration::from_millis(250));
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));

        self.phase += 0.14;
        let pulse = 0.5 + 0.5 * self.phase.sin();
        let size = if has_text {
            BUBBLE
        } else if recording {
            BADGE
        } else {
            // "transcrevendo…" precisa de mais largura que "REC".
            egui::vec2(170.0, 38.0)
        };
        let shown = tail(&preview, PREVIEW_MAX);

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

        // Ajusta a janela ao tamanho fixo do estado (só quando muda).
        if (size.x - self.last_size.0).abs() > 0.5 || (size.y - self.last_size.1).abs() > 0.5 {
            self.last_size = (size.x, size.y);
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        }

        // Reancora periodicamente (sem depender do tamanho do conteúdo).
        let t = ctx.input(|i| i.time);
        if t - self.last_reposition > 0.7 {
            self.last_reposition = t;
            if let Some(win) = x11::active_window_rect() {
                self.last_win = Some(win);
            }
            if let Some(win) = self.last_win {
                let (cx, cy) = win.center();
                let monitor = x11::monitor_rect_for(cx, cy);
                let anchor = if has_text {
                    &self.preview_anchor
                } else {
                    &self.indicator_anchor
                };
                let (px, py) = anchor::anchor_pos(anchor, win, monitor, (size.x, size.y));
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(px, py)));
            }
        }

        ctx.request_repaint_after(Duration::from_millis(60));
    }
}

#[cfg(test)]
mod tests {
    use super::tail;

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
