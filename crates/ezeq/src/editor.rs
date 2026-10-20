use crate::dsp::{SpectrumOutput, MIN_DB, SPECTRUM_BINS};
use crate::params::EzeqParams;
use nih_plug::prelude::Editor;
use nih_plug_egui::{
    create_egui_editor,
    egui::{self, Color32, Pos2, Rect, Shape, Stroke, Vec2},
    resizable_window::ResizableWindow,
};
use std::{sync::Arc, time::Duration};

const BACKGROUND: Color32 = Color32::from_rgb(17, 20, 24);
const PANEL: Color32 = Color32::from_rgb(24, 29, 35);
const GRID: Color32 = Color32::from_rgb(48, 56, 65);
const ACCENT: Color32 = Color32::from_rgb(76, 210, 172);
const SPECTRUM_FILL: Color32 = Color32::from_rgba_premultiplied(10, 29, 24, 35);
const AXIS_TEXT: Color32 = Color32::from_rgb(139, 149, 159);

pub(crate) fn create(
    params: Arc<EzeqParams>,
    spectrum: Arc<SpectrumOutput>,
) -> Option<Box<dyn Editor>> {
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
                            ui.label("Input spectrum");
                            ui.add_space(8.0);

                            let desired_size =
                                Vec2::new(ui.available_width(), ui.available_height().max(260.0));
                            let (response, painter) =
                                ui.allocate_painter(desired_size, egui::Sense::hover());
                            let rect = response.rect;
                            let plot_rect = Rect::from_min_max(
                                Pos2::new(rect.left() + 46.0, rect.top() + 6.0),
                                Pos2::new(rect.right() - 8.0, rect.bottom() - 26.0),
                            );

                            for &(frequency, label) in &[
                                (20.0, "20"),
                                (50.0, "50"),
                                (100.0, "100"),
                                (200.0, "200"),
                                (500.0, "500"),
                                (1_000.0, "1k"),
                                (2_000.0, "2k"),
                                (5_000.0, "5k"),
                                (10_000.0, "10k"),
                                (20_000.0, "20k"),
                            ] {
                                let x = frequency_x(plot_rect, frequency);
                                painter.line_segment(
                                    [
                                        Pos2::new(x, plot_rect.top()),
                                        Pos2::new(x, plot_rect.bottom()),
                                    ],
                                    Stroke::new(1.0_f32, GRID),
                                );
                                painter.text(
                                    Pos2::new(x, plot_rect.bottom() + 7.0),
                                    egui::Align2::CENTER_TOP,
                                    label,
                                    egui::FontId::monospace(10.0),
                                    AXIS_TEXT,
                                );
                            }

                            for db in [-90, -72, -54, -36, -18, 0] {
                                let y = db_y(plot_rect, db as f32);
                                painter.line_segment(
                                    [
                                        Pos2::new(plot_rect.left(), y),
                                        Pos2::new(plot_rect.right(), y),
                                    ],
                                    Stroke::new(1.0_f32, GRID),
                                );
                                painter.text(
                                    Pos2::new(plot_rect.left() - 7.0, y),
                                    egui::Align2::RIGHT_CENTER,
                                    format!("{db}"),
                                    egui::FontId::monospace(10.0),
                                    AXIS_TEXT,
                                );
                            }

                            painter.text(
                                Pos2::new(rect.left() + 7.0, plot_rect.center().y),
                                egui::Align2::CENTER_CENTER,
                                "dB",
                                egui::FontId::monospace(10.0),
                                AXIS_TEXT,
                            );

                            let mut levels = [MIN_DB; SPECTRUM_BINS];
                            spectrum.read(&mut levels);
                            let mut spectrum_points = Vec::with_capacity(SPECTRUM_BINS);
                            for (index, level) in levels.iter().enumerate() {
                                let x = plot_rect.left()
                                    + plot_rect.width() * index as f32 / (SPECTRUM_BINS - 1) as f32;
                                spectrum_points.push(Pos2::new(x, db_y(plot_rect, *level)));
                            }

                            let mut fill_mesh = egui::Mesh::default();
                            for points in spectrum_points.windows(2) {
                                let first_vertex = fill_mesh.vertices.len() as u32;
                                fill_mesh.colored_vertex(points[0], SPECTRUM_FILL);
                                fill_mesh.colored_vertex(points[1], SPECTRUM_FILL);
                                fill_mesh.colored_vertex(
                                    Pos2::new(points[1].x, plot_rect.bottom()),
                                    Color32::TRANSPARENT,
                                );
                                fill_mesh.colored_vertex(
                                    Pos2::new(points[0].x, plot_rect.bottom()),
                                    Color32::TRANSPARENT,
                                );
                                fill_mesh.add_triangle(
                                    first_vertex,
                                    first_vertex + 1,
                                    first_vertex + 2,
                                );
                                fill_mesh.add_triangle(
                                    first_vertex,
                                    first_vertex + 2,
                                    first_vertex + 3,
                                );
                            }
                            painter.add(Shape::mesh(fill_mesh));
                            painter.add(Shape::line(spectrum_points, Stroke::new(1.8_f32, ACCENT)));
                            painter.line_segment(
                                [
                                    Pos2::new(plot_rect.left(), plot_rect.bottom()),
                                    Pos2::new(plot_rect.right(), plot_rect.bottom()),
                                ],
                                Stroke::new(1.0_f32, AXIS_TEXT),
                            );

                            context.request_repaint_after(Duration::from_millis(16));
                        });
                });
        },
    )
}

fn frequency_x(rect: Rect, frequency: f32) -> f32 {
    let normalized = (frequency / 20.0).log10() / (20_000.0_f32 / 20.0).log10();
    rect.left() + rect.width() * normalized
}

fn db_y(rect: Rect, db: f32) -> f32 {
    rect.bottom() - rect.height() * ((db - MIN_DB) / -MIN_DB).clamp(0.0, 1.0)
}
