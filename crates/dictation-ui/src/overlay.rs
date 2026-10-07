//! O app do overlay (egui/eframe): badge REC pulsante + bolha de prévia ao vivo.
//!
//! Uma única janela transparente, click-through e always-on-top, reancorada na
//! janela em foco (badge em `indicator_anchor`; bolha em `preview_anchor`).

use crate::state::SharedUi;
use crate::{anchor, x11};
use eframe::egui;
use std::time::Duration;

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
            last_size: (74.0, 38.0),
            last_win: None,
        }
    }
}

impl eframe::App for OverlayApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let (recording, preview) = {
            let s = self.state.lock().unwrap();
            (s.recording, s.preview.trim().to_string())
        };
        let has_text = !preview.is_empty();

        // Sem gravação e sem prévia: esconde a janela (não ocupa a tela).
        if !recording && !has_text {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            ctx.request_repaint_after(Duration::from_millis(250));
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));

        self.phase += 0.14;
        let pulse = 0.5 + 0.5 * self.phase.sin();
        let width = if has_text { 640.0 } else { 74.0 };

        let frame = egui::Frame::NONE
            .fill(egui::Color32::from_rgba_unmultiplied(18, 18, 18, 205))
            .corner_radius(12.0)
            .inner_margin(egui::Margin::symmetric(12, 8))
            .shadow(egui::epaint::Shadow::NONE);

        let resp = frame.show(ui, |ui| {
            ui.set_max_width(width - 24.0);
            ui.horizontal(|ui| {
                let r = 5.0 + 3.0 * pulse;
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(r * 2.0, r * 2.0), egui::Sense::hover());
                ui.painter().circle_filled(
                    rect.center(),
                    r,
                    egui::Color32::from_rgba_unmultiplied(
                        235,
                        45,
                        45,
                        (180.0 + 75.0 * pulse) as u8,
                    ),
                );
                ui.label(egui::RichText::new("REC").strong().size(12.0));
            });
            if has_text {
                ui.add_space(2.0);
                ui.add(
                    egui::Label::new(egui::RichText::new(&preview).size(14.0))
                        .wrap()
                        .selectable(false),
                );
            }
        });
        let size = resp.response.rect.size();

        // Reancora periodicamente (como o `reposition()` do Python).
        let t = ctx.input(|i| i.time);
        if t - self.last_reposition > 0.7 {
            self.last_reposition = t;
            // Enquanto o próprio overlay é a "janela ativa", mantém a última janela real.
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

        // Ajusta a janela ao conteúdo (converge em 1-2 quadros).
        if (size.x - self.last_size.0).abs() > 0.5 || (size.y - self.last_size.1).abs() > 0.5 {
            self.last_size = (size.x, size.y);
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(size.x, size.y)));
        }

        ctx.request_repaint_after(Duration::from_millis(60));
    }
}
