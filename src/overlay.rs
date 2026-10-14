//! Transparent, click-through overlay window.
//!
//! A small viewport sized to the crosshair, centered on the monitor plus the
//! manual offsets. Event-driven: viewport commands are sent only when the
//! desired geometry actually changes, and repaint requests are bounded.
//!
//! Platform notes: transparency, topmost and undecorated come from eframe/winit
//! viewport flags. Click-through is platform-specific: on Windows the Win32
//! style is applied directly (see [`click_through`]), elsewhere the viewport
//! `mouse_passthrough` flag is used. Actual on-screen behavior in Hunt:
//! Showdown is unverified (no Windows game machine here) and is marked so in
//! the release checklist.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

use eframe::egui::{CentralPanel, Frame};
use eframe::{App, CreationContext, Frame as EframeFrame, NativeOptions};

use crate::config::{self, Settings};
use crate::geometry::{window_outer_pos_points, window_size_px};
use crate::render::paint_crosshair;

/// Viewport flags for the overlay window. Shared so tests can assert intent.
///
/// On Windows neither `with_transparent` (DWM blur-behind renders a glassy,
/// shadowed blob) nor `mouse_passthrough` (adds `WS_EX_LAYERED` without layer
/// attributes, hiding the content; emilk/egui#2537) is used: the window keeps
/// its pixels and Windows makes one key color transparent - see
/// [`crate::platform_windows::configure_overlay_window`]. Other platforms use
/// the regular transparent, click-through viewport.
pub fn viewport_builder() -> egui::ViewportBuilder {
    let builder = egui::ViewportBuilder::default()
        .with_title("vibehair overlay")
        .with_decorations(false)
        .with_resizable(false)
        .with_always_on_top()
        .with_taskbar(false)
        .with_has_shadow(false)
        .with_active(false);
    if cfg!(target_os = "windows") {
        builder
    } else {
        builder.with_transparent(true).with_mouse_passthrough(true)
    }
}

/// Apply the platform window setup (Windows: keyed color + click-through).
///
/// Called once per process; failures are logged, never fatal.
fn configure_window(frame: &mut EframeFrame, settings: &Settings) {
    #[cfg(target_os = "windows")]
    {
        let Some(window) = frame.winit_window().cloned() else {
            crate::platform::log_line("vibehair: overlay window handle unavailable");
            return;
        };
        if let Err(err) = crate::platform_windows::configure_overlay_window(&window, settings.color)
        {
            crate::platform::log_line(&format!("vibehair: overlay window setup failed: {err}"));
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        // Other platforms configure click-through through the viewport flags.
        let _ = (frame, settings);
    }
}

/// Window side in physical pixels for these settings.
fn desired_side_px(settings: &Settings) -> u32 {
    window_size_px(
        settings.shape,
        settings.size,
        settings.gap,
        settings.thickness,
    )
    .0
}

/// Outer top-left position in logical points.
///
/// `monitor_points` is `ViewportInfo::monitor_size`, which egui reports in
/// logical points; it is converted to physical pixels here so the centering
/// math and the DPI division cancel out exactly once.
fn desired_outer_points(
    settings: &Settings,
    side_px: u32,
    monitor_points: egui::Vec2,
    pixels_per_point: f32,
) -> (f32, f32) {
    window_outer_pos_points(
        (
            monitor_points.x * pixels_per_point,
            monitor_points.y * pixels_per_point,
        ),
        (side_px as f32, side_px as f32),
        (settings.offset_x, settings.offset_y),
        pixels_per_point,
    )
}

/// Geometry the window has been told to use, to avoid resending commands.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Applied {
    side_px: u32,
    pixels_per_point: f32,
    pos: Option<(f32, f32)>,
}

/// Lock file path guarding against multiple simultaneous overlays.
pub fn overlay_lock_path() -> PathBuf {
    config::resolve_save_path().with_file_name("overlay.lock")
}

/// Exclusive, non-blocking lock on `path`.
///
/// `Ok(None)` means another overlay process holds the lock. Errors that are
/// not contention (e.g. unsupported filesystem) allow running rather than
/// silently disabling the overlay.
fn try_lock_file(path: &Path) -> std::io::Result<Option<File>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(err)) => Err(err),
    }
}

pub struct OverlayApp {
    settings: Settings,
    applied: Applied,
    /// Frames still to repaint after a resize so the new size takes effect.
    settle_frames: u8,
    /// Bounded frames waiting for the OS to report the monitor size.
    monitor_wait: u8,
    /// Holds the single-instance lock for the process lifetime, when available.
    _lock: Option<File>,
    /// Platform window setup runs once, as soon as a window handle exists.
    window_configured: bool,
}

impl OverlayApp {
    pub fn new(_cc: &CreationContext<'_>, lock: Option<File>) -> Self {
        let (settings, path, err) = config::load();
        if let Some(err) = err {
            crate::platform::log_line(&format!("vibehair: config {path:?}: {err}; using defaults"));
        }
        // No viewport commands here: `pixels_per_point` is still its default
        // 1.0 before the first update. The first `ui()` pass applies the real
        // size (`applied` starts at zero, so it always differs).
        Self {
            settings,
            applied: Applied::default(),
            settle_frames: 0,
            monitor_wait: 8,
            _lock: lock,
            window_configured: false,
        }
    }
}

impl App for OverlayApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut EframeFrame) {
        if !self.window_configured {
            self.window_configured = true;
            configure_window(frame, &self.settings);
        }
        let ctx = ui.ctx().clone();
        let ppp = ctx.pixels_per_point().max(0.5);
        let side = desired_side_px(&self.settings);

        let scale_changed = (self.applied.pixels_per_point - ppp).abs() > 0.01;
        if side != self.applied.side_px || scale_changed {
            self.applied.side_px = side;
            self.applied.pixels_per_point = ppp;
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                side as f32 / ppp,
                side as f32 / ppp,
            )));
            self.settle_frames = 2;
        }

        let monitor = ctx
            .input(|i| i.viewport().monitor_size)
            .unwrap_or(egui::Vec2::ZERO);
        if monitor.x > 1.0 && monitor.y > 1.0 {
            let want = desired_outer_points(&self.settings, side, monitor, ppp);
            if self.applied.pos != Some(want) {
                self.applied.pos = Some(want);
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
                    want.0, want.1,
                )));
            }
        } else if self.monitor_wait > 0 {
            // The OS reports the monitor size a frame or two in; retry briefly.
            self.monitor_wait -= 1;
            ctx.request_repaint();
        }

        if self.settle_frames > 0 {
            self.settle_frames -= 1;
            ctx.request_repaint();
        }

        CentralPanel::default().frame(Frame::NONE).show(ui, |ui| {
            let center = ui.max_rect().center();
            paint_crosshair(ui.painter(), center, &self.settings, ppp);
        });
    }

    /// Only the crosshair may be visible. Windows turns the key color into
    /// transparency (see [`crate::platform_windows::configure_overlay_window`]);
    /// elsewhere the window itself is transparent.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        crate::platform::overlay_clear_color_for(self.settings.color)
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
    let lock = match acquire_overlay_lock() {
        Ok(Some(lock)) => Some(lock),
        Ok(None) => {
            crate::platform::log_line("vibehair: an overlay is already running");
            return Ok(());
        }
        Err(err) => {
            // A broken/unsupported lock file must not disable the overlay.
            crate::platform::log_line(&format!(
                "vibehair: {err}; continuing without single-instance guard"
            ));
            None
        }
    };
    eframe::run_native(
        "vibehair overlay",
        native_options(),
        Box::new(move |cc| Ok(Box::new(OverlayApp::new(cc, lock)))),
    )
}

/// `true` when no other overlay process currently holds the single-instance lock.
pub fn overlay_lock_available() -> bool {
    matches!(try_lock_file(&overlay_lock_path()), Ok(Some(_)))
}

/// `Ok(None)` when another overlay process already holds the lock.
fn acquire_overlay_lock() -> Result<Option<File>, String> {
    let path = overlay_lock_path();
    try_lock_file(&path).map_err(|err| format!("cannot use lock file {path:?}: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn scratch(name: &str) -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-scratch")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn overlay_requests_transparent_click_through_topmost() {
        let vb = viewport_builder();
        assert_eq!(vb.transparent, Some(true));
        assert_eq!(vb.decorations, Some(false));
        #[cfg(not(target_os = "windows"))]
        {
            assert_eq!(vb.mouse_passthrough, Some(true));
            assert_eq!(vb.transparent, Some(true));
        }
        // Windows configures the native window itself instead.
        #[cfg(target_os = "windows")]
        {
            assert_eq!(vb.mouse_passthrough, None);
            assert_eq!(vb.transparent, None);
        }
        assert_eq!(vb.window_level, Some(egui::WindowLevel::AlwaysOnTop));
        assert_eq!(vb.resizable, Some(false));
    }

    #[test]
    fn overlay_starts_hidden_from_taskbar_without_focus() {
        let vb = viewport_builder();
        assert_eq!(vb.taskbar, Some(false));
        assert_eq!(vb.active, Some(false));
        assert_eq!(vb.has_shadow, Some(false));
    }

    #[test]
    fn overlay_background_is_fully_transparent() {
        let lock = try_lock_file(&scratch("clear").join("a.lock"))
            .unwrap()
            .unwrap();
        let cc = CreationContext::_new_kittest(egui::Context::default());
        let app = OverlayApp::new(&cc, Some(lock));
        assert_eq!(
            app.clear_color(&egui::Visuals::dark()),
            [0.0, 0.0, 0.0, 0.0]
        );
    }

    #[test]
    fn lock_excludes_second_holder_and_releases_on_drop() {
        let path = scratch("lock").join("overlay.lock");
        let first = try_lock_file(&path).unwrap().expect("first lock");
        assert!(
            try_lock_file(&path).unwrap().is_none(),
            "second lock must fail"
        );
        drop(first);
        assert!(
            try_lock_file(&path).unwrap().is_some(),
            "lock must be reusable"
        );
    }

    #[test]
    fn centering_is_correct_in_physical_pixels_at_200_percent_dpi() {
        let s = Settings {
            size: 48.0,
            gap: 0.0,
            thickness: 0.0,
            ..Settings::default()
        };
        let side = desired_side_px(&s);
        let side_f = side as f32;
        // Same monitor, reported as 1920x1080 points at 200% => 3840x2160 physical px.
        let (x, y) = desired_outer_points(&s, side, egui::vec2(1920.0, 1080.0), 2.0);
        // Points must be the physical center divided by the scale factor.
        assert_eq!(x, (3840.0 - side_f) / 4.0);
        assert_eq!(y, (2160.0 - side_f) / 4.0);
        // A points-vs-physical mix-up (the earlier bug) would produce these:
        assert_ne!(x, (1920.0 - side_f) / 4.0);
    }

    #[test]
    fn offsets_shift_position_by_physical_pixels() {
        let s = Settings {
            offset_x: 10,
            offset_y: -20,
            ..Settings::default()
        };
        let side = desired_side_px(&s);
        let side_f = side as f32;
        let (x, y) = desired_outer_points(&s, side, egui::vec2(1920.0, 1080.0), 1.0);
        assert_eq!(x, (1920.0 - side_f) / 2.0 + 10.0);
        assert_eq!(y, (1080.0 - side_f) / 2.0 - 20.0);
        // At 200% the same physical offsets are half as many logical points
        // (3840x2160 physical monitor).
        let (x2, y2) = desired_outer_points(&s, side, egui::vec2(1920.0, 1080.0), 2.0);
        assert_eq!(x2, (3840.0 - side_f) / 4.0 + 5.0);
        assert_eq!(y2, (2160.0 - side_f) / 4.0 - 10.0);
    }

    /// The overlay must emit the crosshair itself: opaque, centered, inside
    /// the viewport. Guards against "window is transparent but nothing shows".
    #[test]
    fn overlay_emits_an_opaque_centered_crosshair() {
        let ctx = egui::Context::default();
        let lock = try_lock_file(&scratch("emit").join("a.lock")).unwrap();
        let cc = CreationContext::_new_kittest(ctx.clone());
        let mut app = OverlayApp::new(&cc, lock);

        let mut frame = EframeFrame::_new_kittest();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.ui(ui, &mut frame);
        });
        output.textures_delta.clear(); // headless: nothing renders the textures
        let raw_input = output.viewport_output.keys().count();
        assert!(raw_input > 0);

        let mut segments = Vec::new();
        for clipped in &output.shapes {
            if let egui::Shape::LineSegment { points, stroke } = &clipped.shape {
                segments.push((points, stroke));
            }
        }
        assert_eq!(segments.len(), 4, "one crosshair = four arms");

        let rect = ctx.viewport_rect();
        let center = rect.center();
        for (points, stroke) in &segments {
            assert_eq!(stroke.color.a(), 255, "crosshair arms must be opaque");
            assert_eq!(stroke.color.g(), 255, "default crosshair color is green");
            for point in *points {
                assert!(rect.contains(*point), "{point:?} must be inside {rect:?}");
            }
        }
        // Defaults: size 8, gap 4 => arms run from 4px to 12px off-center.
        let (horizontal, _) = segments[0];
        assert_eq!(horizontal[0], center + egui::vec2(4.0, 0.0));
        assert_eq!(horizontal[1], center + egui::vec2(12.0, 0.0));
        let (vertical, _) = segments[2];
        assert_eq!(vertical[0], center + egui::vec2(0.0, 4.0));
        assert_eq!(vertical[1], center + egui::vec2(0.0, 12.0));
    }

    #[test]
    fn applied_defaults_force_first_frame_apply() {
        let applied = Applied::default();
        let s = Settings::default();
        assert_ne!(applied.side_px, desired_side_px(&s));
        assert_eq!(applied.pos, None);
    }

    /// Drive one headless egui pass; returns the viewport commands and the
    /// repaint delay the app requested.
    fn run_pass(
        ctx: &egui::Context,
        app: &mut OverlayApp,
    ) -> (Vec<egui::ViewportCommand>, std::time::Duration) {
        let mut frame = EframeFrame::_new_kittest();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.ui(ui, &mut frame);
        });
        output.textures_delta.clear(); // headless: nothing renders the textures
        match output.viewport_output.get(&egui::ViewportId::ROOT) {
            Some(out) => (out.commands.clone(), out.repaint_delay),
            None => (Vec::new(), std::time::Duration::MAX),
        }
    }

    /// Regression test for the overlay burning CPU: the window size used to be
    /// re-applied on every frame, which kept triggering resize events forever.
    #[test]
    fn steady_state_applies_size_once_and_stops_repainting() {
        let ctx = egui::Context::default();
        let lock = try_lock_file(&scratch("steady").join("a.lock")).unwrap();
        let cc = CreationContext::_new_kittest(ctx.clone());
        let mut app = OverlayApp::new(&cc, lock);

        let mut size_sends = 0;
        let mut repaint_requests = 0;
        let mut last_delay = Duration::ZERO;
        for _ in 0..16 {
            let (commands, delay) = run_pass(&ctx, &mut app);
            size_sends += commands
                .iter()
                .filter(|cmd| matches!(cmd, egui::ViewportCommand::InnerSize(_)))
                .count();
            if delay < Duration::MAX {
                repaint_requests += 1;
            }
            last_delay = delay;
        }
        assert_eq!(
            size_sends, 1,
            "window size must be applied once, not every frame"
        );
        assert!(
            repaint_requests <= 12,
            "repaint requests must be bounded during startup, got {repaint_requests}"
        );
        assert_eq!(
            last_delay,
            Duration::MAX,
            "overlay must idle without repainting"
        );
    }
}
