//! Transparent, click-through overlay window.
//!
//! A small viewport sized to the crosshair, centered on the primary monitor
//! plus the manual offsets. Event-driven: repaints only on config/resize
//! changes or when the OS asks (first frames, DPI change). No idle loop.
//!
//! Platform notes: transparency, topmost, undecorated and mouse-passthrough
//! are requested through eframe/winit viewport flags, which is the documented
//! path on Windows. Actual on-screen/Hunt behavior is unverified (no Windows
//! machine here) and is marked so in the release checklist.

use std::time::Duration;

use eframe::egui::{CentralPanel, Frame};
use eframe::{App, CreationContext, Frame as EframeFrame, NativeOptions};

use crate::config::{self, Settings};
use crate::geometry::{window_outer_pos_points, window_size_px};
use crate::render::paint_crosshair;

/// Viewport flags for the overlay window. Shared so tests can assert intent.
pub fn viewport_builder() -> egui::ViewportBuilder {
    egui::ViewportBuilder::default()
        .with_title("vibehair overlay")
        .with_transparent(true)
        .with_decorations(false)
        .with_resizable(false)
        .with_always_on_top()
        .with_mouse_passthrough(true)
        .with_taskbar(false)
        .with_has_shadow(false)
        .with_active(false)
}

pub struct OverlayApp {
    settings: Settings,
    /// Window side in physical pixels, to detect resize needs.
    window_px: u32,
    /// Frames left that must still repaint unconditionally (startup/DPI settle).
    settle_frames: u8,
}

impl OverlayApp {
    pub fn new(cc: &CreationContext<'_>) -> Self {
        let (settings, path, err) = config::load();
        if let Some(err) = err {
            eprintln!("vibehair: config {path:?}: {err}; using defaults");
        }
        let (side, _) = window_size_px(
            settings.shape,
            settings.size,
            settings.gap,
            settings.thickness,
        );
        cc.egui_ctx
            .send_viewport_cmd(egui::ViewportCommand::InnerSize(
                egui::vec2(side as f32, side as f32) / cc.egui_ctx.pixels_per_point().max(1.0),
            ));
        Self {
            settings,
            window_px: side,
            settle_frames: 3,
        }
    }
}

impl App for OverlayApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut EframeFrame) {
        let ctx = ui.ctx().clone();
        let ppp = ctx.pixels_per_point().max(0.5);
        let (side, _) = window_size_px(
            self.settings.shape,
            self.settings.size,
            self.settings.gap,
            self.settings.thickness,
        );
        if side != self.window_px {
            self.window_px = side;
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                side as f32 / ppp,
                side as f32 / ppp,
            )));
            self.settle_frames = self.settle_frames.max(2);
        }
        // Re-center (cheap, idempotent): handles monitor/DPI/offset changes.
        let monitor = ctx
            .input(|i| i.viewport().monitor_size)
            .unwrap_or(egui::vec2(0.0, 0.0));
        if monitor.x > 1.0 && monitor.y > 1.0 {
            let (x, y) = window_outer_pos_points(
                (monitor.x, monitor.y),
                (side as f32, side as f32),
                (self.settings.offset_x, self.settings.offset_y),
                ppp,
            );
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(x, y)));
        }
        if self.settle_frames > 0 {
            self.settle_frames -= 1;
            ctx.request_repaint_after(Duration::from_millis(100));
        }

        CentralPanel::default().frame(Frame::NONE).show(ui, |ui| {
            let center = ui.max_rect().center();
            paint_crosshair(ui.painter(), center, &self.settings);
        });
    }
}

/// Native options for the overlay window.
pub fn native_options() -> NativeOptions {
    NativeOptions {
        viewport: viewport_builder().with_inner_size([64.0, 64.0]),
        ..Default::default()
    }
}

pub fn run() -> eframe::Result {
    eframe::run_native(
        "vibehair overlay",
        native_options(),
        Box::new(|cc| Ok(Box::new(OverlayApp::new(cc)))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_requests_transparent_click_through_topmost() {
        let vb = viewport_builder();
        assert_eq!(vb.transparent, Some(true));
        assert_eq!(vb.decorations, Some(false));
        assert_eq!(vb.mouse_passthrough, Some(true));
        assert_eq!(vb.window_level, Some(egui::WindowLevel::AlwaysOnTop));
        assert_eq!(vb.resizable, Some(false));
    }

    #[test]
    fn overlay_starts_hidden_from_taskbar_without_focus() {
        let vb = viewport_builder();
        assert_eq!(vb.taskbar, Some(false));
        assert_eq!(vb.active, Some(false));
    }
}
