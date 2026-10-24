use super::{ACCENT, AXIS_TEXT};
use crate::params::EqBandParams;
use nih_plug::prelude::ParamSetter;
use nih_plug_egui::{egui, widgets};

pub(super) fn controls(
    ui: &mut egui::Ui,
    setter: &ParamSetter,
    band: &EqBandParams,
    dynamic_gain_db: f32,
) {
    let supports_dynamics = band.band_type.value().uses_gain();

    ui.add_enabled_ui(supports_dynamics, |ui| {
        ui.horizontal_wrapped(|ui| {
            let mut enabled = band.dynamic_enabled.value();
            if ui.checkbox(&mut enabled, "Dynamic").changed() {
                setter.begin_set_parameter(&band.dynamic_enabled);
                setter.set_parameter(&band.dynamic_enabled, enabled);
                setter.end_set_parameter(&band.dynamic_enabled);
            }

            ui.add_enabled_ui(enabled, |ui| {
                ui.label("Trigger");
                ui.add_sized(
                    [82.0, 20.0],
                    widgets::ParamSlider::for_param(&band.dynamic_source, setter),
                );
                ui.label("Range");
                ui.add_sized(
                    [78.0, 20.0],
                    widgets::ParamSlider::for_param(&band.dynamic_range, setter),
                );
                ui.label("Threshold");
                ui.add_sized(
                    [82.0, 20.0],
                    widgets::ParamSlider::for_param(&band.dynamic_threshold, setter),
                );
                ui.label("Attack");
                ui.add_sized(
                    [72.0, 20.0],
                    widgets::ParamSlider::for_param(&band.dynamic_attack, setter),
                );
                ui.label("Release");
                ui.add_sized(
                    [72.0, 20.0],
                    widgets::ParamSlider::for_param(&band.dynamic_release, setter),
                );
            });

            let meter_color = if enabled && dynamic_gain_db.abs() >= 0.01 {
                ACCENT
            } else {
                AXIS_TEXT
            };
            ui.label(
                egui::RichText::new(format!("Δ {dynamic_gain_db:+.1} dB"))
                    .monospace()
                    .color(meter_color),
            );
        });
    });
}
