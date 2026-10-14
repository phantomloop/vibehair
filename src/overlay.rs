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
use std::time::{Duration, Instant};

use eframe::egui::{CentralPanel, Frame};
use eframe::{App, CreationContext, Frame as EframeFrame, NativeOptions};

use crate::config::{self, Settings};
use crate::geometry::{window_outer_pos_points, window_pos_on_monitor_px, window_size_px};
use crate::render::paint_crosshair;

/// Viewport flags for the overlay window. Shared so tests can assert intent.
///
/// On Windows neither `with_transparent` (DWM blur-behind renders a glassy,
/// shadowed blob) nor `mouse_passthrough` (adds `WS_EX_LAYERED` without layer
/// attributes, hiding the content; emilk/egui#2537) is used: the window keeps
/// its pixels and Windows makes one key color transparent - see
/// [`crate::platform_windows::configure_overlay_window`]. Other platforms use
/// the regular transparent, click-through viewport.
pub fn viewport_builder(mode: crate::config::Transparency) -> egui::ViewportBuilder {
    let builder = egui::ViewportBuilder::default()
        .with_title("vibehair overlay")
        .with_decorations(false)
        .with_resizable(false)
        .with_always_on_top()
        .with_taskbar(false)
        .with_has_shadow(false)
        .with_active(false);
    match (mode.effective(), cfg!(target_os = "windows")) {
        // Windows key mode: an opaque window whose key color is switched off
        // by Windows, so no windowing transparency is requested at all.
        (crate::config::Transparency::Key, true) => builder,
        // Alpha mode uses the windowing system's transparency.
        (_, true) => builder.with_transparent(true),
        (_, false) => builder.with_transparent(true).with_mouse_passthrough(true),
    }
}

/// Re-assert the overlay's Z-order and repair its styles.
///
/// `Some(styles_repaired)` on Windows, `None` where there is nothing to do.
fn enforce_overlay(frame: &mut EframeFrame, settings: &Settings) -> Option<bool> {
    #[cfg(target_os = "windows")]
    {
        let window = frame.winit_window().cloned()?;
        match crate::platform_windows::enforce_overlay(
            &window,
            settings.color,
            settings.transparency,
        ) {
            Ok(repaired) => Some(repaired),
            Err(err) => {
                crate::platform::log_line(&format!("vibehair: topmost watchdog failed: {err}"));
                None
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (frame, settings);
        None
    }
}

/// Monitor (physical pixels) that currently has focus, when the platform can say.
fn foreground_monitor() -> Option<(f32, f32, f32, f32)> {
    #[cfg(target_os = "windows")]
    {
        crate::platform_windows::foreground_fullscreen_monitor_rect()
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

/// Apply the platform window setup (Windows: keyed color + click-through).
///
/// Called once per process (retried once); failures are logged, never fatal.
/// Returns `true` when nothing further needs doing.
fn configure_window(frame: &mut EframeFrame, settings: &Settings) -> bool {
    #[cfg(target_os = "windows")]
    {
        let Some(window) = frame.winit_window().cloned() else {
            crate::platform::log_line("vibehair: overlay window handle unavailable");
            return false;
        };
        match crate::platform_windows::configure_overlay_window(
            &window,
            settings.color,
            settings.transparency,
        ) {
            Ok(()) => true,
            Err(err) => {
                crate::platform::log_line(&format!("vibehair: overlay window setup failed: {err}"));
                false
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        // Other platforms configure click-through through the viewport flags.
        let _ = (frame, settings);
        true
    }
}

/// How often the overlay re-asserts its topmost position and checks that its
/// window styles are intact. A borderless game re-asserts its own `HWND_TOPMOST`
/// after us, which would leave the crosshair behind the game; one cheap Win32
/// call per second wins that race and costs a fraction of one egui pass.
const WATCHDOG_INTERVAL: Duration = Duration::from_secs(1);

/// How many "styles were reset" lines are written before the log goes quiet.
const MAX_LOGGED_STYLE_REPAIRS: u8 = 3;

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
///
/// It lives in the shared control directory rather than next to the settings,
/// because every copy of the executable (old downloads, different folders) must
/// contend for the *same* lock - otherwise each copy happily draws its own
/// crosshair on top of the others.
pub fn overlay_lock_path() -> PathBuf {
    crate::platform::control_dir().join("overlay.lock")
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
    /// Platform window setup attempts so far (a style change needs a frame
    /// change, so the first attempt may not stick; one retry is allowed).
    configure_attempts: u8,
    /// How often the watchdog had to repair our window styles. A foreign window
    /// can reset them repeatedly, so only the first few repairs are logged.
    style_repairs: u8,
    /// When the next topmost/style watchdog tick is due.
    next_watchdog: Instant,
    /// Monitor (physical pixels) the crosshair is placed on: the one showing the
    /// foreground window, so a game on a secondary screen still gets it.
    target_monitor: Option<(f32, f32, f32, f32)>,
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
            configure_attempts: 0,
            style_repairs: 0,
            next_watchdog: Instant::now(),
            target_monitor: None,
        }
    }
}

impl App for OverlayApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut EframeFrame) {
        if self.configure_attempts < 2 {
            self.configure_attempts += 1;
            let ok = configure_window(frame, &self.settings);
            if ok {
                self.configure_attempts = 2;
            }
        }

        // Keep the crosshair above the game and wake up once a second to do so.
        let now = Instant::now();
        if now >= self.next_watchdog {
            self.next_watchdog = now + WATCHDOG_INTERVAL;
            if crate::platform::take_overlay_stop_request() {
                crate::platform::log_line("vibehair: stop requested; closing the overlay");
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
            if let Some(repair) = enforce_overlay(frame, &self.settings) {
                if repair && self.style_repairs < MAX_LOGGED_STYLE_REPAIRS {
                    self.style_repairs += 1;
                    crate::platform::log_line(
                        "vibehair: overlay styles were reset by another window; repaired",
                    );
                }
                match foreground_monitor() {
                    Some(rect) if self.target_monitor != Some(rect) => {
                        crate::platform::log_line(&format!(
                            "vibehair: crosshair follows the focused monitor at {},{} {}x{} px",
                            rect.0, rect.1, rect.2, rect.3
                        ));
                        self.target_monitor = Some(rect);
                    }
                    _ => {}
                }
            }
        }
        ui.ctx().request_repaint_after(WATCHDOG_INTERVAL);

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
        if let Some(rect) = self.target_monitor {
            // Position on the monitor that has focus, in physical pixels, then
            // convert to the points `OuterPosition` expects.
            let (x_px, y_px) = window_pos_on_monitor_px(
                rect,
                (side as f32, side as f32),
                (self.settings.offset_x, self.settings.offset_y),
            );
            let want = (x_px / ppp, y_px / ppp);
            if self.applied.pos != Some(want) {
                self.applied.pos = Some(want);
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
                    want.0, want.1,
                )));
            }
        } else if monitor.x > 1.0 && monitor.y > 1.0 {
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
        crate::platform::overlay_clear_color_for(self.settings.color, self.settings.transparency)
    }
}

/// Native options for the overlay window.
pub fn native_options(settings: &Settings) -> NativeOptions {
    NativeOptions {
        // Tiny first size: the real size (crosshair extent + antialiasing
        // margin) is applied on the first frames, in physical pixels.
        viewport: viewport_builder(settings.transparency).with_inner_size([64.0, 64.0]),
        // Flat 2D content: no depth, stencil or MSAA buffers.
        depth_buffer: 0,
        stencil_buffer: 0,
        multisampling: 0,
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
    // The transparency mode decides how the window is created, and the
    // settings are loaded again inside the app; both must agree.
    let (settings, _path, _err) = config::load();
    eframe::run_native(
        "vibehair overlay",
        native_options(&settings),
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
        use crate::config::Transparency;
        let vb = viewport_builder(Transparency::Key);
        assert_eq!(vb.decorations, Some(false));
        #[cfg(not(target_os = "windows"))]
        {
            assert_eq!(vb.mouse_passthrough, Some(true));
            assert_eq!(vb.transparent, Some(true));
        }
        // Windows configures the native window itself instead: keyed color for
        // transparency and WS_EX_TRANSPARENT for click-through.
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
        use crate::config::Transparency;
        let vb = viewport_builder(Transparency::Key);
        assert_eq!(vb.taskbar, Some(false));
        assert_eq!(vb.active, Some(false));
        assert_eq!(vb.has_shadow, Some(false));
    }

    /// The window background must never show: transparent with alpha on most
    /// platforms, an opaque key color on Windows (which Windows makes
    /// transparent via `LWA_COLORKEY`).
    #[test]
    fn overlay_background_is_invisible_but_never_shows_a_box() {
        let lock = try_lock_file(&scratch("clear").join("a.lock"))
            .unwrap()
            .unwrap();
        let cc = CreationContext::_new_kittest(egui::Context::default());
        let app = OverlayApp::new(&cc, Some(lock));
        let color = app.clear_color(&egui::Visuals::dark());
        #[cfg(target_os = "windows")]
        {
            assert_eq!(color, [0.0, 0.0, 0.0, 1.0], "the key color must be opaque");
        }
        #[cfg(not(target_os = "windows"))]
        {
            assert_eq!(color, [0.0, 0.0, 0.0, 0.0], "must clear fully transparent");
        }
    }

    /// The single-instance guarantee matters *between processes* (each overlay
    /// is its own process). Same-process exclusion is only asserted on Unix,
    /// where std's file locks are per file description; Windows byte-range
    /// locks are owned per process, so a second handle there may succeed.
    #[test]
    fn lock_is_exclusive_where_guaranteed_and_always_reusable() {
        let path = scratch("lock").join("overlay.lock");
        let first = try_lock_file(&path).unwrap().expect("first lock");

        #[cfg(not(target_os = "windows"))]
        assert!(
            try_lock_file(&path).unwrap().is_none(),
            "second holder in this process must be refused"
        );

        drop(first);
        assert!(
            try_lock_file(&path).unwrap().is_some(),
            "lock must be reusable after release"
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
        let mut immediate_repaints = 0;
        let mut last_delay = Duration::MAX;
        for _ in 0..16 {
            let (commands, delay) = run_pass(&ctx, &mut app);
            size_sends += commands
                .iter()
                .filter(|cmd| matches!(cmd, egui::ViewportCommand::InnerSize(_)))
                .count();
            if delay == Duration::ZERO {
                immediate_repaints += 1;
            } else {
                // Between the startup passes and the watchdog cadence nothing
                // may schedule a faster redraw - that is what a busy loop is.
                assert!(
                    delay >= WATCHDOG_INTERVAL - Duration::from_millis(100),
                    "intermediate repaint cadence: {delay:?}"
                );
            }
            last_delay = delay;
        }
        assert_eq!(
            size_sends, 1,
            "window size must be applied once, not every frame"
        );
        assert!(
            immediate_repaints <= 12,
            "only startup may repaint immediately, got {immediate_repaints} of 16 passes"
        );
        // egui rounds the requested delay to frame boundaries, so allow a frame.
        assert!(
            last_delay <= WATCHDOG_INTERVAL
                && last_delay >= WATCHDOG_INTERVAL - Duration::from_millis(100),
            "steady state must sleep at the watchdog cadence, got {last_delay:?}"
        );
    }
}
