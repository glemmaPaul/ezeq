use crate::params::EzeqParams;
use nih_plug::prelude::Editor;
use nih_plug_egui::{
    create_egui_editor,
    egui::{self, Color32, Pos2, Stroke, Vec2},
    resizable_window::ResizableWindow,
};
use std::sync::Arc;

const BACKGROUND: Color32 = Color32::from_rgb(17, 20, 24);
const PANEL: Color32 = Color32::from_rgb(24, 29, 35);
const GRID: Color32 = Color32::from_rgb(48, 56, 65);
const ACCENT: Color32 = Color32::from_rgb(76, 210, 172);

pub(crate) fn create(params: Arc<EzeqParams>) -> Option<Box<dyn Editor>> {
    let editor_state = params.editor_state.clone();

    create_egui_editor(
        params.editor_state.clone(),
        (),
        |context, _state| {
            let mut visuals = egui::Visuals::dark();
            visuals.panel_fill = BACKGROUND;
            visuals.window_fill = BACKGROUND;
            visuals.override_text_color = Some(Color32::from_rgb(224, 229, 234));
            context.set_visuals(visuals);
        },
        move |context, _setter, _state| {
            ResizableWindow::new("ezeq-editor")
                .min_size(Vec2::new(640.0, 400.0))
                .show(context, editor_state.as_ref(), |ui| {
                    ui.horizontal(|ui| {
                        ui.heading("EZEQ");
                        ui.label(egui::RichText::new("PARAMETRIC EQUALIZER").color(ACCENT));
                    });
                    ui.add_space(8.0);

                    egui::Frame::NONE
                        .fill(PANEL)
                        .inner_margin(16.0)
                        .show(ui, |ui| {
                            ui.label("Spectrum and filter response");
                            ui.add_space(8.0);

                            let desired_size =
                                Vec2::new(ui.available_width(), ui.available_height().max(260.0));
                            let (response, painter) =
                                ui.allocate_painter(desired_size, egui::Sense::hover());
                            let rect = response.rect;

                            for step in 1..10 {
                                let x = rect.left() + rect.width() * step as f32 / 10.0;
                                painter.line_segment(
                                    [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
                                    Stroke::new(1.0_f32, GRID),
                                );
                            }
                            for step in 1..6 {
                                let y = rect.top() + rect.height() * step as f32 / 6.0;
                                painter.line_segment(
                                    [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
                                    Stroke::new(1.0_f32, GRID),
                                );
                            }

                            let center_y = rect.center().y;
                            painter.line_segment(
                                [
                                    Pos2::new(rect.left(), center_y),
                                    Pos2::new(rect.right(), center_y),
                                ],
                                Stroke::new(2.0_f32, ACCENT),
                            );
                            painter.text(
                                rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "Pass-through scaffold — DSP coming next",
                                egui::FontId::proportional(16.0),
                                Color32::from_rgb(150, 158, 166),
                            );
                        });
                });
        },
    )
}
