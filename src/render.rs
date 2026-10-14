//! The single shared crosshair painter.
//!
//! Both the editor preview and the overlay call [`paint_crosshair`], so the
//! preview cannot drift from the overlay: same geometry, same colors, same
//! physical pixel size.

use egui::epaint::{CircleShape, Shape as EguiShape};
use egui::{Color32, Painter, Pos2, Stroke};

use crate::config::Settings;
use crate::geometry::crosshair_geometry;

/// Settings expressed in physical pixels; the crosshair is _drawn_ in egui
/// points, so every length is divided by this scale before painting.
pub fn to_points(settings: &Settings, pixels_per_point: f32) -> Settings {
    let scale = if pixels_per_point.is_finite() && pixels_per_point > 0.0 {
        1.0 / pixels_per_point
    } else {
        1.0
    };
    Settings {
        size: settings.size * scale,
        gap: settings.gap * scale,
        thickness: settings.thickness * scale,
        ..settings.clone()
    }
}

/// sRGB + opacity as an egui color. Uses unmultiplied input: the painter blends
/// premultiplied, and `Color32` premultiplies on conversion.
fn crosshair_color(settings: &Settings) -> Color32 {
    Color32::from_rgba_unmultiplied(
        settings.color[0],
        settings.color[1],
        settings.color[2],
        (settings.opacity.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

/// Draw the crosshair described by `settings` with `center` as its middle.
///
/// `settings` lengths are physical pixels (as stored in the config and used to
/// size the overlay window); `pixels_per_point` converts them to the points
/// egui paints in, so the crosshair covers exactly as many display pixels as
/// it does in the overlay.
pub fn paint_crosshair(
    painter: &Painter,
    center: Pos2,
    settings: &Settings,
    pixels_per_point: f32,
) {
    let points = to_points(settings, pixels_per_point);
    if points.opacity <= 0.0 {
        return;
    }
    let color = crosshair_color(&points);
    let stroke = Stroke::new(points.thickness.max(0.0), color);
    let prims = crosshair_geometry(points.shape, points.size, points.gap);
    for line in &prims.lines {
        painter.line_segment(
            [
                center + egui::vec2(line.from.0, line.from.1),
                center + egui::vec2(line.to.0, line.to.1),
            ],
            stroke,
        );
    }
    for ring in &prims.rings {
        painter.add(EguiShape::Circle(CircleShape::stroke(
            center,
            ring.radius,
            stroke,
        )));
    }
    for disc in &prims.discs {
        painter.circle_filled(center, disc.radius, color);
    }
}

/// Same shapes as [`paint_crosshair`] but as data, for headless tests.
#[cfg(test)]
pub fn paint_shapes(center: Pos2, settings: &Settings, pixels_per_point: f32) -> Vec<EguiShape> {
    let points = to_points(settings, pixels_per_point);
    let mut shapes = Vec::new();
    if points.opacity <= 0.0 {
        return shapes;
    }
    let color = crosshair_color(&points);
    let stroke = Stroke::new(points.thickness.max(0.0), color);
    let prims = crosshair_geometry(points.shape, points.size, points.gap);
    for line in &prims.lines {
        shapes.push(EguiShape::line_segment(
            [
                center + egui::vec2(line.from.0, line.from.1),
                center + egui::vec2(line.to.0, line.to.1),
            ],
            stroke,
        ));
    }
    for ring in &prims.rings {
        shapes.push(EguiShape::Circle(CircleShape::stroke(
            center,
            ring.radius,
            stroke,
        )));
    }
    for disc in &prims.discs {
        shapes.push(EguiShape::Circle(CircleShape::filled(
            center,
            disc.radius,
            color,
        )));
    }
    shapes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Shape;

    #[test]
    fn shapes_cover_all_kinds() {
        for (shape, expected) in [
            (Shape::Cross, 4),
            (Shape::T, 3),
            (Shape::Circle, 1),
            (Shape::Dot, 1),
        ] {
            let s = Settings {
                shape,
                ..Settings::default()
            };
            assert_eq!(
                paint_shapes(Pos2::ZERO, &s, 1.0).len(),
                expected,
                "{shape:?}"
            );
        }
    }

    #[test]
    fn zero_opacity_paints_nothing() {
        let s = Settings {
            opacity: 0.0,
            ..Settings::default()
        };
        assert!(paint_shapes(Pos2::ZERO, &s, 1.0).is_empty());
    }

    #[test]
    fn shapes_are_centered_on_given_point() {
        let center = Pos2::new(50.0, 60.0);
        let s = Settings {
            size: 10.0,
            gap: 4.0,
            ..Settings::default()
        };
        let shapes = paint_shapes(center, &s, 1.0);
        let EguiShape::LineSegment { points, .. } = shapes[0] else {
            panic!("expected line segment");
        };
        assert_eq!(points, [Pos2::new(54.0, 60.0), Pos2::new(64.0, 60.0)]);
    }

    #[test]
    fn half_opacity_premultiplies_rgb() {
        // egui's glow painter blends (ONE, ONE_MINUS_SRC_ALPHA): stored rgb must scale with a.
        let s = Settings {
            color: [200, 100, 50],
            opacity: 0.5,
            ..Settings::default()
        };
        let c = crosshair_color(&s);
        assert_eq!((c.a(), c.r(), c.g(), c.b()), (128, 100, 50, 25));
        let s = Settings {
            color: [200, 100, 50],
            opacity: 1.0,
            ..Settings::default()
        };
        let c = crosshair_color(&s);
        assert_eq!((c.a(), c.r(), c.g(), c.b()), (255, 200, 100, 50));
    }

    /// The crosshair must occupy the same number of display pixels in the
    /// editor preview and in the overlay, whatever the DPI scaling is.
    #[test]
    fn physical_size_is_identical_at_every_dpi_scale() {
        let s = Settings {
            shape: Shape::Dot,
            size: 3.0,
            thickness: 2.0,
            ..Settings::default()
        };
        for ppp in [1.0, 1.25, 1.5, 2.0, 3.0] {
            let shapes = paint_shapes(Pos2::ZERO, &s, ppp);
            let [EguiShape::Circle(circle)] = &shapes[..] else {
                panic!("expected one disc at {ppp}");
            };
            // Radius in points * scale = radius in physical pixels.
            assert!(
                (circle.radius * ppp - s.size).abs() < 1e-3,
                "at {ppp}x: drew {} px instead of {} px",
                circle.radius * ppp,
                s.size
            );
        }
    }

    #[test]
    fn stroke_width_is_preserved_in_physical_pixels() {
        let s = Settings {
            shape: Shape::Cross,
            size: 8.0,
            gap: 4.0,
            thickness: 2.0,
            ..Settings::default()
        };
        for ppp in [1.0, 1.5, 2.0] {
            let shapes = paint_shapes(Pos2::ZERO, &s, ppp);
            let EguiShape::LineSegment { points, stroke } = &shapes[0] else {
                panic!("expected line segment");
            };
            assert!((stroke.width * ppp - s.thickness).abs() < 1e-3);
            // Arm length (gap..gap+size) in physical pixels is unchanged.
            assert!(((points[1].x - points[0].x) * ppp - s.size).abs() < 1e-3);
            assert!((points[0].x * ppp - s.gap).abs() < 1e-3);
        }
    }

    /// Everything drawn must fit inside the window the overlay creates for it,
    /// otherwise the crosshair is clipped (the reported "broken" look).
    #[test]
    fn crosshair_fits_inside_its_overlay_window_at_every_dpi_scale() {
        use crate::geometry::window_size_px;
        for shape in [Shape::Cross, Shape::T, Shape::Circle, Shape::Dot] {
            for (size, gap, thickness) in [
                (3.0f32, 0.0f32, 0.25f32),
                (3.0, 34.0, 32.0),
                (8.0, 4.0, 2.0),
                (40.0, 10.0, 6.0),
            ] {
                let s = Settings {
                    shape,
                    size,
                    gap,
                    thickness,
                    ..Settings::default()
                };
                let (side_px, _) = window_size_px(shape, size, gap, thickness);
                for ppp in [1.0, 1.25, 1.5, 2.0] {
                    let side_points = side_px as f32 / ppp;
                    for egui_shape in paint_shapes(Pos2::ZERO, &s, ppp) {
                        let rect = egui_shape.visual_bounding_rect();
                        let drawn_half = rect.width().max(rect.height()) / 2.0;
                        assert!(
                            drawn_half <= side_points / 2.0 + 1e-3,
                            "{shape:?} size {size} gap {gap} thickness {thickness} at {ppp}x: \
                             drawn half-size {drawn_half} points exceeds window half {side_points} / 2"
                        );
                    }
                }
            }
        }
    }
}
