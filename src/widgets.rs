//! Small shared UI pieces.

use std::time::Duration;

use eframe::egui::{self, Color32, RichText};

/// How long between animation frames of the wordmark.
const RAINBOW_FRAME: Duration = Duration::from_millis(40);

/// The `vibehair` wordmark: a rainbow band that travels from left to right.
///
/// Only windows that show the title request these repaints, so the overlay
/// itself stays event-driven.
pub fn rainbow_title(ui: &mut egui::Ui, text: &str) {
    let time = ui.ctx().input(|input| input.time) as f32;
    let chars: Vec<char> = text.chars().collect();
    let count = chars.len().max(1) as f32;
    ui.horizontal(|ui| {
        // Glyphs must sit next to each other to read as one word.
        ui.spacing_mut().item_spacing.x = 0.0;
        for (index, ch) in chars.iter().enumerate() {
            // Hue varies along X, and the whole band slides as time advances.
            let along = index as f32 / count;
            let hue = (along * 0.85 + time * 0.18).rem_euclid(1.0);
            let color = Color32::from(egui::ecolor::Hsva::new(hue, 0.95, 1.0, 1.0));
            ui.label(
                RichText::new(ch.to_string())
                    .size(26.0)
                    .strong()
                    .color(color),
            );
        }
    });
    ui.ctx().request_repaint_after(RAINBOW_FRAME);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wordmark must be visible and keep animating, and must not panic for
    /// any input (including empty strings).
    #[test]
    fn rainbow_title_emits_a_label_per_character_and_requests_repaints() {
        let ctx = egui::Context::default();
        let mut frame = eframe::Frame::_new_kittest();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            rainbow_title(ui, "vibehair");
            let _ = &mut frame;
        });
        output.textures_delta.clear();
        let text_shapes = output
            .shapes
            .iter()
            .filter(|clipped| matches!(clipped.shape, egui::Shape::Text(_)))
            .count();
        assert_eq!(text_shapes, "vibehair".chars().count());
        let delay = output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map(|out| out.repaint_delay)
            .unwrap_or(Duration::MAX);
        assert!(
            delay <= RAINBOW_FRAME,
            "the animation must keep repainting, got {delay:?}"
        );
    }

    /// Collect the glyph colours the widget emitted, in order.
    fn glyph_colors(ctx: &egui::Context, time: f64) -> Vec<Color32> {
        let mut frame = eframe::Frame::_new_kittest();
        let raw = egui::RawInput {
            time: Some(time),
            ..Default::default()
        };
        let mut output = ctx.run_ui(raw, |ui| {
            rainbow_title(ui, "vibehair");
            let _ = &mut frame;
        });
        output.textures_delta.clear();
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => text
                    .galley
                    .job
                    .sections
                    .first()
                    .map(|section| section.format.color),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn rainbow_title_paints_a_travelling_rainbow() {
        let ctx = egui::Context::default();
        let at_zero = glyph_colors(&ctx, 0.0);
        assert_eq!(at_zero.len(), 8);
        // Neighbouring letters differ: it really is a gradient, not one colour.
        assert_ne!(at_zero[0], at_zero[1]);
        assert_ne!(at_zero[3], at_zero[6]);
        // The band travels: the same letter changes colour as time advances.
        let later = glyph_colors(&ctx, 1.0);
        if std::env::var_os("VIBEHAIR_DUMP_COLORS").is_some() {
            let hex = |colors: &[Color32]| {
                colors
                    .iter()
                    .map(|c| format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b()))
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            println!("t=0.0: {}", hex(&at_zero));
            println!("t=1.0: {}", hex(&later));
        }
        assert_ne!(at_zero, later, "the rainbow must animate");
        // And it stays a rainbow (colours are not all equal).
        let distinct: std::collections::HashSet<_> = later.iter().map(|c| c.to_array()).collect();
        assert!(
            distinct.len() > 3,
            "expected a spread of colours, got {distinct:?}"
        );
    }

    #[test]
    fn rainbow_title_accepts_an_empty_string() {
        let ctx = egui::Context::default();
        let mut frame = eframe::Frame::_new_kittest();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            rainbow_title(ui, "");
            let _ = &mut frame;
        });
        output.textures_delta.clear();
    }
}
