//! Pure crosshair geometry in physical pixels, centered on the origin.
//!
//! No rendering or windowing types here so the math stays trivially testable.
//! Both the editor preview and the overlay build on these primitives.

use serde::{Deserialize, Serialize};

/// Crosshair shape selected in the editor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Shape {
    /// Four arms along the screen axes.
    #[default]
    Cross,
    /// Filled disc.
    Dot,
    /// Hollow ring.
    Circle,
    /// Horizontal bar with a downward stem (no top arm).
    #[serde(alias = "tee")]
    T,
}

/// Straight stroke from `from` to `to`, in pixels relative to the crosshair center.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    pub from: (f32, f32),
    pub to: (f32, f32),
}

/// Hollow circle centered on the crosshair center.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ring {
    pub radius: f32,
}

/// Filled disc centered on the crosshair center.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Disc {
    pub radius: f32,
}

/// All primitives for one crosshair, in pixels relative to its center.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Primitives {
    pub lines: Vec<Line>,
    pub rings: Vec<Ring>,
    pub discs: Vec<Disc>,
}

/// Build stroke/fill primitives for `shape`.
///
/// `size` is arm length (cross/T) or radius (circle/dot) in pixels.
/// `gap` is the distance from the center to the start of each arm (cross/T only).
/// Negative inputs are treated as zero.
pub fn crosshair_geometry(shape: Shape, size: f32, gap: f32) -> Primitives {
    let size = size.max(0.0);
    let gap = gap.max(0.0);
    let mut out = Primitives::default();
    let arm = |x0: f32, y0: f32, x1: f32, y1: f32| Line {
        from: (x0, y0),
        to: (x1, y1),
    };
    match shape {
        Shape::Cross => {
            let (a, b) = (gap, gap + size);
            out.lines.extend([
                arm(a, 0.0, b, 0.0),
                arm(-a, 0.0, -b, 0.0),
                arm(0.0, a, 0.0, b),
                arm(0.0, -a, 0.0, -b),
            ]);
        }
        Shape::T => {
            let (a, b) = (gap, gap + size);
            out.lines.extend([
                arm(a, 0.0, b, 0.0),
                arm(-a, 0.0, -b, 0.0),
                arm(0.0, a, 0.0, b),
            ]);
        }
        Shape::Circle => out.rings.push(Ring { radius: size }),
        Shape::Dot => out.discs.push(Disc { radius: size }),
    }
    out
}

/// Half-width/height of the smallest square containing the crosshair, in pixels.
/// Includes half the stroke width so antialiased edges are not clipped.
pub fn half_extent(shape: Shape, size: f32, gap: f32, thickness: f32) -> f32 {
    let size = size.max(0.0);
    let gap = gap.max(0.0);
    let half_stroke = thickness.max(0.0) / 2.0;
    match shape {
        Shape::Cross | Shape::T => gap + size + half_stroke,
        Shape::Circle => size + half_stroke,
        Shape::Dot => size,
    }
}

/// Overlay window size in physical pixels: a tight square around the crosshair
/// plus a 1px antialiasing margin on each side, clamped to 8..=4096px.
pub fn window_size_px(shape: Shape, size: f32, gap: f32, thickness: f32) -> (u32, u32) {
    let side = (half_extent(shape, size, gap, thickness) + 1.0) * 2.0;
    let side = side.ceil().clamp(8.0, 4096.0) as u32;
    (side, side)
}

/// Outer window position in logical points: centered on the monitor, shifted by
/// `offset_px`, converted with `pixels_per_point` (the OS DPI scale).
/// Non-finite or non-positive scales fall back to 1.0.
pub fn window_outer_pos_points(
    monitor_px: (f32, f32),
    window_px: (f32, f32),
    offset_px: (i32, i32),
    pixels_per_point: f32,
) -> (f32, f32) {
    let ppp = if pixels_per_point.is_finite() && pixels_per_point > 0.0 {
        pixels_per_point
    } else {
        1.0
    };
    let x = ((monitor_px.0 - window_px.0) / 2.0 + offset_px.0 as f32) / ppp;
    let y = ((monitor_px.1 - window_px.1) / 2.0 + offset_px.1 as f32) / ppp;
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cross_has_four_symmetric_arms() {
        let p = crosshair_geometry(Shape::Cross, 10.0, 4.0);
        assert!(p.rings.is_empty() && p.discs.is_empty());
        assert_eq!(
            p.lines,
            vec![
                Line {
                    from: (4.0, 0.0),
                    to: (14.0, 0.0)
                },
                Line {
                    from: (-4.0, 0.0),
                    to: (-14.0, 0.0)
                },
                Line {
                    from: (0.0, 4.0),
                    to: (0.0, 14.0)
                },
                Line {
                    from: (0.0, -4.0),
                    to: (0.0, -14.0)
                },
            ]
        );
    }

    #[test]
    fn t_has_no_top_arm() {
        let p = crosshair_geometry(Shape::T, 10.0, 4.0);
        assert_eq!(p.lines.len(), 3);
        assert!(p.lines.iter().all(|l| l.from.1 >= 0.0 && l.to.1 >= 0.0));
    }

    #[test]
    fn circle_and_dot_emit_single_centered_primitive() {
        let c = crosshair_geometry(Shape::Circle, 9.0, 4.0);
        assert!(c.lines.is_empty() && c.discs.is_empty());
        assert_eq!(c.rings, vec![Ring { radius: 9.0 }]);
        let d = crosshair_geometry(Shape::Dot, 2.0, 99.0);
        assert!(d.lines.is_empty() && d.rings.is_empty());
        assert_eq!(d.discs, vec![Disc { radius: 2.0 }]);
    }

    #[test]
    fn negative_inputs_clamp_to_zero() {
        let p = crosshair_geometry(Shape::Cross, -5.0, -2.0);
        assert!(
            p.lines
                .iter()
                .all(|l| l.from == (0.0, 0.0) || l.to == (0.0, 0.0))
        );
        assert_eq!(half_extent(Shape::Cross, -5.0, -2.0, -1.0), 0.0);
    }

    #[test]
    fn extents_include_half_stroke() {
        assert_eq!(half_extent(Shape::Cross, 10.0, 4.0, 2.0), 15.0);
        assert_eq!(half_extent(Shape::Circle, 9.0, 0.0, 2.0), 10.0);
        assert_eq!(half_extent(Shape::Dot, 3.0, 0.0, 8.0), 3.0);
    }

    #[test]
    fn window_size_has_margin_and_minimum() {
        assert_eq!(window_size_px(Shape::Cross, 10.0, 4.0, 2.0), (32, 32));
        assert_eq!(window_size_px(Shape::Dot, 0.0, 0.0, 0.0), (8, 8));
        assert_eq!(
            window_size_px(Shape::Cross, 99999.0, 0.0, 0.0),
            (4096, 4096)
        );
    }

    #[test]
    fn window_position_centers_and_offsets() {
        let (x, y) = window_outer_pos_points((1920.0, 1080.0), (100.0, 100.0), (0, 0), 1.0);
        assert_eq!((x, y), (910.0, 490.0));
        let (x, y) = window_outer_pos_points((1920.0, 1080.0), (100.0, 100.0), (10, -20), 1.0);
        assert_eq!((x, y), (920.0, 470.0));
    }

    #[test]
    fn window_position_scales_with_dpi() {
        let (x, y) = window_outer_pos_points((3840.0, 2160.0), (200.0, 200.0), (0, 0), 2.0);
        assert_eq!((x, y), (910.0, 490.0));
        for bad in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            let (x, y) = window_outer_pos_points((1920.0, 1080.0), (100.0, 100.0), (0, 0), bad);
            assert_eq!((x, y), (910.0, 490.0));
        }
    }
}
