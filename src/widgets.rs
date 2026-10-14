//! Small shared UI pieces.

use std::time::Duration;

use eframe::egui::{
    self, Color32, CursorIcon, FontFamily, FontId, Sense, TextFormat, TextStyle, Vec2,
    text::LayoutJob,
};

/// Point size of the wordmark.
pub const TITLE_SIZE: f32 = 34.0;
/// How long between animation frames of the wordmark. Slow enough to be almost
/// free, fast enough for a smooth sweep.
const RAINBOW_FRAME: Duration = Duration::from_millis(66);
/// Hue span across the word.
const BAND_SPREAD: f32 = 0.85;
/// How fast the band travels, in hues per second, towards +X.
const TRAVEL_PER_SECOND: f32 = 0.25;

/// Shared look for every window: larger text, roomier click targets and a
/// pointing hand over anything clickable (egui keeps the native arrow unless
/// asked, which is why hovering a button used to show no hand cursor).
pub fn apply_style(ctx: &egui::Context) {
    // Both themes are kept identical so the look cannot change under the app.
    ctx.all_styles_mut(|style| {
        let proportional = |size: f32| FontId::new(size, FontFamily::Proportional);
        style
            .text_styles
            .insert(TextStyle::Small, proportional(14.0));
        style
            .text_styles
            .insert(TextStyle::Body, proportional(17.0));
        style
            .text_styles
            .insert(TextStyle::Button, proportional(17.0));
        style
            .text_styles
            .insert(TextStyle::Heading, proportional(21.0));
        style.text_styles.insert(
            TextStyle::Monospace,
            FontId::new(15.0, FontFamily::Monospace),
        );

        style.spacing.item_spacing = Vec2::new(9.0, 9.0);
        style.spacing.button_padding = Vec2::new(16.0, 9.0);
        style.spacing.interact_size = Vec2::new(48.0, 32.0);
        style.spacing.slider_width = 140.0;

        style.visuals.interact_cursor = Some(CursorIcon::PointingHand);
    });
}

/// One glyph, laid out once and reused: the colour is left as
/// [`Color32::PLACEHOLDER`] so it can be supplied per frame when painting.
fn glyph_job(ch: char) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY; // a wordmark never wraps
    job.append(
        &ch.to_string(),
        0.0,
        TextFormat {
            font_id: FontId::proportional(TITLE_SIZE),
            color: Color32::PLACEHOLDER,
            ..Default::default()
        },
    );
    job
}

/// The `vibehair` wordmark: a centred rainbow that travels from left to right.
///
/// Glyph layouts are cached, so animating costs no text layout: each frame only
/// paints the cached galleys with fresh colours. Repaints are requested only
/// while the window has focus, so an idle launcher costs nothing.
pub fn rainbow_title(ui: &mut egui::Ui, text: &str) {
    let (time, focused) = ui.ctx().input(|input| (input.time as f32, input.focused));
    let pixels_per_point = ui.ctx().pixels_per_point();
    // Layout only depends on the text and the scale factor.
    let key = ui.make_persistent_id(("vibehair-wordmark", text, (pixels_per_point * 100.0) as u32));
    // Read the cached layouts, and lay out only on the first frame (or when the
    // scale factor changed). Laying out *inside* `data_mut` would deadlock: both
    // take the context's write lock.
    let cached = ui.data(|data| data.get_temp::<Vec<std::sync::Arc<egui::Galley>>>(key));
    let galleys = match cached {
        Some(galleys) => galleys,
        None => {
            let laid_out: Vec<std::sync::Arc<egui::Galley>> = text
                .chars()
                .map(|ch| ui.ctx().fonts_mut(|fonts| fonts.layout_job(glyph_job(ch))))
                .collect();
            ui.data_mut(|data| data.insert_temp(key, laid_out.clone()));
            laid_out
        }
    };
    if galleys.is_empty() {
        return;
    }

    let widths: Vec<f32> = galleys.iter().map(|galley| galley.size().x).collect();
    let total: f32 = widths.iter().sum();
    let height = galleys
        .iter()
        .map(|galley| galley.size().y)
        .fold(0.0_f32, f32::max);
    let (rect, _response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let mut x = rect.center().x - total / 2.0;
    let y = rect.center().y - height / 2.0;

    // Faux bold: stamp each glyph with sub-pixel offsets, which thickens the
    // strokes without bundling a second font file.
    let weight = (TITLE_SIZE * 0.028).max(0.5);
    let count = galleys.len() as f32;
    for (index, galley) in galleys.iter().enumerate() {
        let along = index as f32 / count;
        // Patches of equal hue sit at `x = (hue + travel * t) / spread`, so the
        // rainbow moves towards increasing x - left to right - as time passes.
        let hue = (along * BAND_SPREAD - time * TRAVEL_PER_SECOND).rem_euclid(1.0);
        let color = Color32::from(egui::ecolor::Hsva::new(hue, 0.95, 1.0, 1.0));
        let origin = egui::pos2(x, y);
        for offset in [
            Vec2::new(-weight, 0.0),
            Vec2::new(weight, 0.0),
            Vec2::new(0.0, weight),
        ] {
            ui.painter().galley(origin + offset, galley.clone(), color);
        }
        ui.painter().galley(origin, galley.clone(), color);
        x += widths[index];
    }

    if focused {
        ui.ctx().request_repaint_after(RAINBOW_FRAME);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// The four bold stamps of every glyph, in painting order.
    fn glyph_stamps(ctx: &egui::Context, time: f64) -> Vec<egui::epaint::TextShape> {
        let (_output, _rect, shapes) = render(ctx, time);
        shapes
            .into_iter()
            .filter_map(|shape| match shape {
                egui::Shape::Text(text) => Some(text),
                _ => None,
            })
            .collect()
    }

    /// The colour each glyph was painted with, one entry per letter.
    fn glyph_colors(ctx: &egui::Context, time: f64) -> Vec<Color32> {
        glyph_stamps(ctx, time)
            .chunks(4)
            .map(|stamps| stamps[0].fallback_color)
            .collect()
    }

    /// Render the wordmark once and return the output plus its screen rectangle.
    fn render(ctx: &egui::Context, time: f64) -> (egui::FullOutput, egui::Rect, Vec<egui::Shape>) {
        let mut frame = eframe::Frame::_new_kittest();
        let raw = egui::RawInput {
            time: Some(time),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(280.0, 220.0),
            )),
            ..Default::default()
        };
        let mut output = ctx.run_ui(raw, |ui| {
            rainbow_title(ui, "vibehair");
            let _ = &mut frame;
        });
        output.textures_delta.clear();
        let shapes: Vec<egui::Shape> = output
            .shapes
            .iter()
            .map(|clipped| clipped.shape.clone())
            .collect();
        let rect = ctx.viewport_rect();
        (output, rect, shapes)
    }

    fn close(a: Color32, b: Color32) -> bool {
        let (a, b) = (a.to_array(), b.to_array());
        (0..3).all(|i| a[i].abs_diff(b[i]) <= 2) && a[3].abs_diff(b[3]) <= 2
    }

    /// The requested look: bigger text and buttons, and a pointing hand over
    /// anything clickable (egui keeps the native arrow by default).
    #[test]
    fn style_makes_text_bigger_and_clickables_point() {
        let ctx = egui::Context::default();
        let dark = egui::Theme::Dark;
        let default_body = egui::TextStyle::Body.resolve(&ctx.style_of(dark));
        apply_style(&ctx);
        let style = ctx.style_of(dark);
        assert!(egui::TextStyle::Body.resolve(&style).size > default_body.size);
        assert_eq!(egui::TextStyle::Button.resolve(&style).size, 17.0);
        assert_eq!(egui::TextStyle::Body.resolve(&style).size, 17.0);
        assert_eq!(
            style.visuals.interact_cursor,
            Some(CursorIcon::PointingHand),
            "hovering a button must show the pointing hand"
        );
        assert!(style.spacing.interact_size.y >= 32.0, "bigger hit areas");
        assert!(style.spacing.button_padding.y >= 8.0, "roomier buttons");
    }

    #[test]
    fn wordmark_paints_a_gradient_and_animates() {
        let ctx = egui::Context::default();
        let at_zero = glyph_colors(&ctx, 0.0);
        assert_eq!(at_zero.len(), "vibehair".chars().count());
        assert!(
            !close(at_zero[0], at_zero[1]),
            "neighbouring letters differ"
        );
        let later = glyph_colors(&ctx, 1.0);
        assert_ne!(at_zero, later, "the rainbow must animate");
        let distinct: HashSet<_> = later.iter().map(|c| c.to_array()).collect();
        assert!(
            distinct.len() > 3,
            "expected a spread of colours: {distinct:?}"
        );
    }

    /// The band must travel towards +X (left to right): the colour a letter
    /// shows now reaches its right neighbour a moment later.
    #[test]
    fn rainbow_travels_left_to_right() {
        let ctx = egui::Context::default();
        let letters = "vibehair".chars().count() as f32;
        let step = BAND_SPREAD / (letters * TRAVEL_PER_SECOND);
        let now = glyph_colors(&ctx, 0.0);
        let later = glyph_colors(&ctx, step as f64);
        for index in 0..now.len() - 1 {
            assert!(
                close(later[index + 1], now[index]),
                "letter {} at t={step:.3} ({:?}) should match letter {index} at t=0 ({:?})",
                index + 1,
                later[index + 1],
                now[index]
            );
        }
    }

    #[test]
    fn wordmark_is_centred_and_stamped_bold() {
        let ctx = egui::Context::default();
        let (_output, rect, _shapes) = render(&ctx, 0.0);
        let stamps = glyph_stamps(&ctx, 0.0);
        let glyphs = "vibehair".chars().count();
        // Four stamps per glyph: three offsets plus the plain one -> faux bold.
        assert_eq!(stamps.len(), glyphs * 4, "expected the bold stamps");
        // The last stamp of each glyph is the plain, un-offset one.
        let plain: Vec<&egui::epaint::TextShape> =
            stamps.chunks(4).map(|stamps| &stamps[3]).collect();

        // The run of glyphs must be horizontally centred in the widget's rect.
        let first_x = plain[0].pos.x;
        let last = plain[plain.len() - 1];
        let run_right = last.pos.x + last.galley.size().x;
        let centre = (first_x + run_right) / 2.0;
        assert!(
            (centre - rect.width() / 2.0).abs() <= 1.0,
            "wordmark centre {centre} should be near {}",
            rect.width() / 2.0
        );

        // Glyphs advance left to right without gaps or overlaps.
        let mut x = first_x;
        for (index, text) in plain.iter().enumerate() {
            assert!(
                (text.pos.x - x).abs() < 0.01,
                "glyph {index} should start at {x}, is at {}",
                text.pos.x
            );
            x += text.galley.size().x;
        }
        assert!((x - run_right).abs() < 0.01);
    }
}
