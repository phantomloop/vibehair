//! The single shared crosshair painter.
//!
//! Both the editor preview and the overlay call [`paint_crosshair`]; there is
//! no second renderer to drift out of sync.

use egui::epaint::{CircleShape, Shape as EguiShape};
use egui::{Color32, Painter, Pos2, Stroke};

use crate::config::Settings;
use crate::geometry::crosshair_geometry;

/// sRGB + opacity as an egui color. Uses unmultiplied input: the glow painter
/// blends premultiplied, and `Color32` premultiplies on conversion.
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
/// Fractional `thickness` values (e.g. 0.25 steps) produce antialiased
/// subpixel strokes, not physically fractional display pixels.
pub fn paint_crosshair(painter: &Painter, center: Pos2, settings: &Settings) {
    let color = crosshair_color(settings);
    if settings.opacity <= 0.0 {
        return;
    }
    let stroke = Stroke::new(settings.thickness.max(0.0), color);
    let prims = crosshair_geometry(settings.shape, settings.size, settings.gap);
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
pub fn paint_shapes(center: Pos2, settings: &Settings) -> Vec<EguiShape> {
    let mut shapes = Vec::new();
    if settings.opacity <= 0.0 {
        return shapes;
    }
    let color = crosshair_color(settings);
    let stroke = Stroke::new(settings.thickness.max(0.0), color);
    let prims = crosshair_geometry(settings.shape, settings.size, settings.gap);
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
        let s = Settings {
            shape: Shape::Cross,
            ..Settings::default()
        };
        assert_eq!(paint_shapes(Pos2::ZERO, &s).len(), 4);
        let s = Settings {
            shape: Shape::T,
            ..Settings::default()
        };
        assert_eq!(paint_shapes(Pos2::ZERO, &s).len(), 3);
        let s = Settings {
            shape: Shape::Circle,
            ..Settings::default()
        };
        assert!(matches!(
            paint_shapes(Pos2::ZERO, &s)[..],
            [EguiShape::Circle(_)]
        ));
        let s = Settings {
            shape: Shape::Dot,
            ..Settings::default()
        };
        assert!(matches!(
            paint_shapes(Pos2::ZERO, &s)[..],
            [EguiShape::Circle(_)]
        ));
    }

    #[test]
    fn zero_opacity_paints_nothing() {
        let s = Settings {
            opacity: 0.0,
            ..Settings::default()
        };
        assert!(paint_shapes(Pos2::ZERO, &s).is_empty());
    }

    #[test]
    fn shapes_are_centered_on_given_point() {
        let center = Pos2::new(50.0, 60.0);
        let s = Settings {
            size: 10.0,
            gap: 4.0,
            ..Settings::default()
        };
        let shapes = paint_shapes(center, &s);
        let EguiShape::LineSegment { points, .. } = shapes[0] else {
            panic!("expected line segment");
        };
        assert_eq!(points, [Pos2::new(54.0, 60.0), Pos2::new(64.0, 60.0)]);
    }

    #[test]
    fn half_opacity_premultiplies_rgb() {
        // egui_glow blends (ONE, ONE_MINUS_SRC_ALPHA): stored rgb must scale with a.
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
}
