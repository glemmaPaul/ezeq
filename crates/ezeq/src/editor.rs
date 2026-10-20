use crate::dsp::{response_magnitude, SpectrumOutput, MIN_DB, SPECTRUM_BINS};
use crate::params::{
    EzeqParams, MAX_FREQUENCY_HZ, MAX_GAIN_DB, MIN_FREQUENCY_HZ, MIN_GAIN_DB, NUM_BANDS,
};
use nih_plug::prelude::Editor;
use nih_plug_egui::{
    create_egui_editor,
    egui::{self, Color32, Pos2, Rect, Shape, Stroke, Vec2},
    resizable_window::ResizableWindow,
    widgets,
};
use std::{sync::Arc, time::Duration};

const BACKGROUND: Color32 = Color32::from_rgb(17, 20, 24);
const PANEL: Color32 = Color32::from_rgb(24, 29, 35);
const GRID: Color32 = Color32::from_rgb(48, 56, 65);
const ACCENT: Color32 = Color32::from_rgb(76, 210, 172);
const SPECTRUM_FILL: Color32 = Color32::from_rgba_premultiplied(10, 29, 24, 35);
const AXIS_TEXT: Color32 = Color32::from_rgb(139, 149, 159);
const DISABLED_BAND: Color32 = Color32::from_rgb(91, 101, 111);
const RESPONSE: Color32 = Color32::from_rgb(241, 195, 89);

#[derive(Default)]
struct EditorState {
    selected_band: usize,
    dragging_band: Option<usize>,
    dragging_gain: bool,
}

pub(crate) fn create(
    params: Arc<EzeqParams>,
    spectrum: Arc<SpectrumOutput>,
) -> Option<Box<dyn Editor>> {
    let editor_state = params.editor_state.clone();

    create_egui_editor(
        params.editor_state.clone(),
        EditorState::default(),
        |context, _state| {
            let mut visuals = egui::Visuals::dark();
            visuals.panel_fill = BACKGROUND;
            visuals.window_fill = BACKGROUND;
            visuals.override_text_color = Some(Color32::from_rgb(224, 229, 234));
            context.set_visuals(visuals);
        },
        move |context, setter, state| {
            ResizableWindow::new("ezeq-editor")
                .min_size(Vec2::new(640.0, 400.0))
                .show(context, editor_state.as_ref(), |ui| {
                    ui.horizontal(|ui| {
                        ui.heading("EZEQ");
                        ui.label(egui::RichText::new("PARAMETRIC EQUALIZER").color(ACCENT));
                        ui.add_space((ui.available_width() - 72.0).max(0.0));
                        let bypassed = params.bypass.value();
                        let bypass = ui.selectable_label(bypassed, "BYPASS");
                        if bypass.clicked() {
                            setter.begin_set_parameter(&params.bypass);
                            setter.set_parameter(&params.bypass, !bypassed);
                            setter.end_set_parameter(&params.bypass);
                        }
                    });
                    ui.add_space(8.0);

                    egui::Frame::NONE
                        .fill(PANEL)
                        .inner_margin(16.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label("Input spectrum");
                                ui.label(
                                    egui::RichText::new("• EQ response").color(RESPONSE).small(),
                                );
                            });
                            ui.add_space(8.0);

                            let desired_size = Vec2::new(
                                ui.available_width(),
                                (ui.available_height() - 86.0).max(240.0),
                            );
                            let (response, painter) =
                                ui.allocate_painter(desired_size, egui::Sense::click_and_drag());
                            let rect = response.rect;
                            let plot_rect = Rect::from_min_max(
                                Pos2::new(rect.left() + 46.0, rect.top() + 6.0),
                                Pos2::new(rect.right() - 38.0, rect.bottom() - 26.0),
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

                            for gain_db in [-24, -12, 0, 12, 24] {
                                let y = gain_y(plot_rect, gain_db as f32);
                                painter.line_segment(
                                    [
                                        Pos2::new(plot_rect.left(), y),
                                        Pos2::new(plot_rect.right(), y),
                                    ],
                                    Stroke::new(
                                        if gain_db == 0 { 1.4_f32 } else { 0.7_f32 },
                                        if gain_db == 0 { AXIS_TEXT } else { GRID },
                                    ),
                                );
                                painter.text(
                                    Pos2::new(plot_rect.right() + 7.0, y),
                                    egui::Align2::LEFT_CENTER,
                                    format!("{gain_db:+}"),
                                    egui::FontId::monospace(10.0),
                                    AXIS_TEXT,
                                );
                            }

                            for db in [-90, -72, -54, -36, -18, 0] {
                                let y = db_y(plot_rect, db as f32);
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

                            let handle_positions = std::array::from_fn(|index| {
                                let band = params.bands()[index];
                                let gain_db = if band.band_type.value().uses_gain() {
                                    band.gain.value()
                                } else {
                                    0.0
                                };
                                Pos2::new(
                                    frequency_x(plot_rect, band.frequency.value()),
                                    gain_y(plot_rect, gain_db),
                                )
                            });

                            if (response.clicked() || response.drag_started())
                                && response.interact_pointer_pos().is_some()
                            {
                                let pointer = response.interact_pointer_pos().unwrap_or_default();
                                if let Some(index) = closest_handle(pointer, &handle_positions) {
                                    state.selected_band = index;

                                    if response.drag_started() {
                                        let band = params.bands()[index];
                                        state.dragging_band = Some(index);
                                        state.dragging_gain = band.band_type.value().uses_gain();
                                        if !band.enabled.value() {
                                            setter.begin_set_parameter(&band.enabled);
                                            setter.set_parameter(&band.enabled, true);
                                            setter.end_set_parameter(&band.enabled);
                                        }
                                        setter.begin_set_parameter(&band.frequency);
                                        if state.dragging_gain {
                                            setter.begin_set_parameter(&band.gain);
                                        }
                                    }
                                }
                            }

                            if response.dragged() {
                                if let (Some(index), Some(pointer)) =
                                    (state.dragging_band, response.interact_pointer_pos())
                                {
                                    let band = params.bands()[index];
                                    setter.set_parameter(
                                        &band.frequency,
                                        x_frequency(plot_rect, pointer.x),
                                    );
                                    if state.dragging_gain {
                                        setter.set_parameter(
                                            &band.gain,
                                            y_gain(plot_rect, pointer.y),
                                        );
                                    }
                                }
                            }

                            if response.drag_stopped() {
                                if let Some(index) = state.dragging_band.take() {
                                    let band = params.bands()[index];
                                    setter.end_set_parameter(&band.frequency);
                                    if state.dragging_gain {
                                        setter.end_set_parameter(&band.gain);
                                    }
                                }
                                state.dragging_gain = false;
                            }

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

                            let sample_rate = spectrum.sample_rate();
                            let mut response_points = Vec::with_capacity(SPECTRUM_BINS);
                            for index in 0..SPECTRUM_BINS {
                                let normalized = index as f32 / (SPECTRUM_BINS - 1) as f32;
                                let frequency = MIN_FREQUENCY_HZ
                                    * (MAX_FREQUENCY_HZ / MIN_FREQUENCY_HZ).powf(normalized);
                                let magnitude =
                                    response_magnitude(params.as_ref(), sample_rate, frequency);
                                let gain_db = (20.0 * magnitude.max(1.0e-6).log10())
                                    .clamp(MIN_GAIN_DB, MAX_GAIN_DB);
                                response_points.push(Pos2::new(
                                    frequency_x(plot_rect, frequency),
                                    gain_y(plot_rect, gain_db),
                                ));
                            }
                            painter
                                .add(Shape::line(response_points, Stroke::new(2.2_f32, RESPONSE)));

                            for (index, position) in handle_positions.into_iter().enumerate() {
                                let enabled = params.bands()[index].enabled.value();
                                let color = if enabled { ACCENT } else { DISABLED_BAND };
                                painter.circle_filled(position, 7.0, color);
                                painter.text(
                                    position,
                                    egui::Align2::CENTER_CENTER,
                                    format!("{}", index + 1),
                                    egui::FontId::monospace(9.0),
                                    BACKGROUND,
                                );
                                if state.selected_band == index {
                                    painter.circle_stroke(
                                        position,
                                        10.0,
                                        Stroke::new(1.5_f32, RESPONSE),
                                    );
                                }
                            }

                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                for index in 0..NUM_BANDS {
                                    let band = params.bands()[index];
                                    let text = egui::RichText::new(format!("B{}", index + 1))
                                        .color(if band.enabled.value() {
                                            ACCENT
                                        } else {
                                            AXIS_TEXT
                                        });
                                    if ui
                                        .selectable_label(state.selected_band == index, text)
                                        .clicked()
                                    {
                                        state.selected_band = index;
                                    }
                                }
                            });

                            let band = params.bands()[state.selected_band];
                            ui.horizontal(|ui| {
                                let mut enabled = band.enabled.value();
                                if ui.checkbox(&mut enabled, "Enabled").changed() {
                                    setter.begin_set_parameter(&band.enabled);
                                    setter.set_parameter(&band.enabled, enabled);
                                    setter.end_set_parameter(&band.enabled);
                                }

                                ui.label("Type");
                                ui.add_sized(
                                    [104.0, 20.0],
                                    widgets::ParamSlider::for_param(&band.band_type, setter),
                                );
                                ui.label("Frequency");
                                ui.add_sized(
                                    [118.0, 20.0],
                                    widgets::ParamSlider::for_param(&band.frequency, setter),
                                );
                                ui.label("Gain");
                                ui.add_enabled_ui(band.band_type.value().uses_gain(), |ui| {
                                    ui.add_sized(
                                        [100.0, 20.0],
                                        widgets::ParamSlider::for_param(&band.gain, setter),
                                    );
                                });
                                ui.label("Q");
                                ui.add_sized(
                                    [88.0, 20.0],
                                    widgets::ParamSlider::for_param(&band.q, setter),
                                );
                            });

                            context.request_repaint_after(Duration::from_millis(16));
                        });
                });
        },
    )
}

fn frequency_x(rect: Rect, frequency: f32) -> f32 {
    let normalized =
        (frequency / MIN_FREQUENCY_HZ).log10() / (MAX_FREQUENCY_HZ / MIN_FREQUENCY_HZ).log10();
    rect.left() + rect.width() * normalized
}

fn x_frequency(rect: Rect, x: f32) -> f32 {
    let normalized = ((x - rect.left()) / rect.width()).clamp(0.0, 1.0);
    MIN_FREQUENCY_HZ * (MAX_FREQUENCY_HZ / MIN_FREQUENCY_HZ).powf(normalized)
}

fn db_y(rect: Rect, db: f32) -> f32 {
    rect.bottom() - rect.height() * ((db - MIN_DB) / -MIN_DB).clamp(0.0, 1.0)
}

fn gain_y(rect: Rect, gain_db: f32) -> f32 {
    rect.bottom()
        - rect.height() * ((gain_db - MIN_GAIN_DB) / (MAX_GAIN_DB - MIN_GAIN_DB)).clamp(0.0, 1.0)
}

fn y_gain(rect: Rect, y: f32) -> f32 {
    let normalized = ((rect.bottom() - y) / rect.height()).clamp(0.0, 1.0);
    MIN_GAIN_DB + normalized * (MAX_GAIN_DB - MIN_GAIN_DB)
}

fn closest_handle(pointer: Pos2, handles: &[Pos2; NUM_BANDS]) -> Option<usize> {
    handles
        .iter()
        .enumerate()
        .map(|(index, position)| (index, pointer.distance(*position)))
        .filter(|(_, distance)| *distance <= 18.0)
        .min_by(|(_, left), (_, right)| left.total_cmp(right))
        .map(|(index, _)| index)
}
