//! Head-less rendering of the GUI to PNG files, for looking at instead of
//! guessing. Enabled only with `--features preview-render`:
//!
//! ```text
//! cargo test --features preview-render write_previews -- --nocapture
//! ```
//!
//! Images land in `target/test-scratch/preview-*.png`. The renderer is wgpu,
//! which uses the software Vulkan driver (lavapipe) on machines without a GPU -
//! on this project's build server the Vulkan ICD manifest is not installed, so
//! `VK_ICD_FILENAMES` is pointed at `libvulkan_lvp.so` when running the test.

use std::path::PathBuf;

use eframe::egui;
use egui_kittest::Harness;

/// Output directory for the previews (inside the ignored build tree).
fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test-scratch");
    std::fs::create_dir_all(&dir).expect("create preview output dir");
    dir
}

/// Render one screen of `size` and save it as `name`.png.
fn capture(name: &str, size: egui::Vec2, ui: impl FnMut(&mut egui::Ui) + 'static) {
    let mut harness = Harness::builder().with_size(size).build_ui(ui);
    // The wordmark animates forever, so `run` (which waits for the UI to settle)
    // would spin; take a fixed number of frames instead.
    harness.run_steps(2);
    let image = harness.render().expect("render the UI");
    let path = out_dir().join(format!("preview-{name}.png"));
    image.save(&path).expect("save the preview png");
    println!("wrote {}", path.display());
}

mod capture_screens {
    use super::*;
    use crate::launcher;

    #[test]
    fn write_previews() {
        // The launcher at two points in the rainbow animation, plus the state
        // where an overlay is running (so the stop button shows).
        for (name, time) in [("launcher", 0.0_f64), ("launcher-animated", 1.0)] {
            capture(name, egui::vec2(340.0, 300.0), move |ui| {
                set_time(ui, time);
                launcher::panel(ui, false, "ready", None);
            });
        }
        capture("launcher-running", egui::vec2(340.0, 300.0), |ui| {
            launcher::panel(ui, true, "overlay started", None);
        });
        capture("launcher-error", egui::vec2(340.0, 300.0), |ui| {
            launcher::panel(ui, false, "", Some("launch failed: access denied"));
        });

        // The editor is a whole window of controls; check that the wider fonts
        // still fit the control column.
        let ctx = egui::Context::default();
        crate::widgets::apply_style(&ctx);
        let cc = eframe::CreationContext::_new_kittest(ctx);
        let mut editor = crate::editor::EditorApp::new(&cc);
        let mut frame = eframe::Frame::_new_kittest();
        let mut harness = Harness::builder()
            .with_size(egui::vec2(680.0, 520.0))
            .build_ui(move |ui| {
                // `App::ui` is the trait method that draws the whole window.
                eframe::App::ui(&mut editor, ui, &mut frame);
            });
        harness.run_steps(2);
        let image = harness.render().expect("render the editor");
        let path = out_dir().join("preview-editor.png");
        image.save(&path).expect("save the editor preview");
        println!("wrote {}", path.display());
    }
}

/// Freeze egui's clock at `time` so a frame of the animation can be captured.
fn set_time(ui: &mut egui::Ui, time: f64) {
    ui.ctx().input_mut(|input| input.time = time);
}

/// Resident memory of this process, in KiB (Linux only).
#[cfg(target_os = "linux")]
fn rss_kib() -> u64 {
    let statm = std::fs::read_to_string("/proc/self/statm").unwrap_or_default();
    let pages: u64 = statm
        .split_whitespace()
        .nth(1)
        .and_then(|field| field.parse().ok())
        .unwrap_or(0);
    pages * 4 // 4 KiB pages
}

#[cfg(not(target_os = "linux"))]
fn rss_kib() -> u64 {
    0
}

/// Memory report: drive each window for many frames and report resident memory
/// growth and egui's galley cache. Run with
/// `--features preview-render memory_report -- --nocapture`.
#[test]
fn memory_report() {
    const WARMUP: usize = 200;
    const FRAMES: usize = 2000; // ~80 s at the 25 fps the animation requests
    println!("process rss at start: {} KiB", rss_kib());

    for window in ["launcher", "editor", "overlay"] {
        let size = match window {
            "editor" => egui::vec2(680.0, 520.0),
            "overlay" => egui::vec2(64.0, 64.0),
            _ => egui::vec2(340.0, 300.0),
        };
        let ctx = egui::Context::default();
        crate::widgets::apply_style(&ctx);
        let mut frame = eframe::Frame::_new_kittest();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app: Option<Box<dyn eframe::App>> = match window {
            "editor" => Some(Box::new(crate::editor::EditorApp::new(&cc))),
            "overlay" => Some(Box::new(crate::overlay::OverlayApp::new(&cc, None))),
            _ => None,
        };

        for index in 0..WARMUP {
            run_frame(&ctx, &mut frame, &mut app, size, index as f64 / 25.0);
        }
        let start = rss_kib();
        for index in WARMUP..FRAMES {
            run_frame(&ctx, &mut frame, &mut app, size, index as f64 / 25.0);
        }
        let end = rss_kib();
        let frames = (FRAMES - WARMUP) as u64;
        println!(
            "{window:>8}: rss {start} -> {end} KiB over {frames} frames ({} bytes/frame), galleys {}",
            end.saturating_sub(start) * 1024 / frames,
            ctx.fonts(|fonts| fonts.num_galleys_in_cache())
        );
    }
}

/// One frame of one window.
fn run_frame(
    ctx: &egui::Context,
    frame: &mut eframe::Frame,
    app: &mut Option<Box<dyn eframe::App>>,
    size: egui::Vec2,
    time: f64,
) {
    let raw = egui::RawInput {
        time: Some(time),
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
        ..Default::default()
    };
    let mut output = ctx.run_ui(raw, |ui| match app {
        Some(app) => app.ui(ui, frame),
        None => {
            crate::launcher::panel(ui, false, "ready", None);
        }
    });
    output.textures_delta.clear();
}
