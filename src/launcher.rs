//! Startup selector and overlay control panel.
//!
//! The chosen mode is spawned as a child process: winit allows exactly one
//! event loop per process, so a second `run_native` in-process would fail.
//! This window stays open so the overlay can be started *and stopped*: without
//! it, a click-through overlay that covers the screen had to be killed from the
//! task manager (reported).
//!
//! Stopping works across processes through [`crate::platform::request_overlay_stop`],
//! so an overlay started by another copy of the executable is stopped too.

use std::process::{Child, Command};
use std::time::{Duration, Instant};

use eframe::egui::CentralPanel;
use eframe::{App, CreationContext, Frame as EframeFrame, NativeOptions};

use crate::platform;
use crate::widgets::rainbow_title;

/// Relaunch this exe in `mode`. Returns the error message on failure.
fn spawn_self(mode: crate::state::Mode, child: &mut Option<Child>) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|err| format!("cannot locate own exe: {err}"))?;
    let flag = match mode {
        crate::state::Mode::Editor => "--editor",
        crate::state::Mode::Overlay => "--overlay",
        crate::state::Mode::Launcher => return Ok(()),
    };
    let mut cmd = Command::new(exe);
    cmd.arg(flag);
    platform::hide_console(&mut cmd);
    let spawned = cmd.spawn().map_err(|err| format!("launch failed: {err}"))?;
    if mode == crate::state::Mode::Overlay {
        *child = Some(spawned);
    }
    Ok(())
}

pub struct LauncherApp {
    /// The overlay this window started, if any.
    overlay: Option<Child>,
    status: String,
    error: Option<String>,
    /// Cached "is an overlay running" answer, probed at most twice a second
    /// (this window repaints continuously for the wordmark animation).
    running: bool,
    running_checked_at: Instant,
}

/// How long a running-state probe is reused.
const RUNNING_PROBE_INTERVAL: Duration = Duration::from_millis(500);

impl LauncherApp {
    pub fn new(cc: &CreationContext<'_>) -> Self {
        crate::widgets::apply_style(&cc.egui_ctx);
        Self {
            overlay: None,
            status: String::new(),
            error: None,
            running: !crate::overlay::overlay_lock_available(),
            running_checked_at: Instant::now(),
        }
    }

    /// `true` when an overlay runs, whoever started it.
    fn overlay_running(&mut self) -> bool {
        if self.running_checked_at.elapsed() >= RUNNING_PROBE_INTERVAL {
            self.running_checked_at = Instant::now();
            self.running = !crate::overlay::overlay_lock_available();
        }
        self.running
    }

    /// Force the next `overlay_running` call to probe again.
    fn refresh_running(&mut self) {
        self.running_checked_at = Instant::now() - RUNNING_PROBE_INTERVAL;
    }

    fn start_overlay(&mut self) {
        self.error = None;
        // A leftover request from a previous session must not kill the overlay
        // we are about to start.
        let _ = platform::take_overlay_stop_request();
        match spawn_self(crate::state::Mode::Overlay, &mut self.overlay) {
            Ok(()) => {
                self.status = "overlay started".to_owned();
                self.refresh_running();
            }
            Err(err) => self.error = Some(err),
        }
    }

    fn stop_overlay(&mut self) {
        self.error = None;
        if let Some(mut child) = self.overlay.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        match platform::request_overlay_stop() {
            Ok(()) => {
                self.status = "overlay stop requested".to_owned();
                self.refresh_running();
            }
            Err(err) => self.error = Some(format!("cannot request stop: {err}")),
        }
    }
}

/// What the user asked for from the panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Nothing clicked this frame.
    None,
    OpenEditor,
    StartOverlay,
    StopOverlay,
}

/// The launcher's contents. Shared with the head-less preview so the images
/// show exactly what the application draws.
pub fn panel(ui: &mut egui::Ui, running: bool, status: &str, error: Option<&str>) -> Action {
    let mut action = Action::None;
    /// Buttons share one width so the panel looks deliberate, not ragged.
    const BUTTON_WIDTH: f32 = 210.0;
    const BUTTON_HEIGHT: f32 = 38.0;

    CentralPanel::default().show(ui, |ui| {
        ui.vertical_centered(|ui| {
            // Push the block towards the middle of the (fixed size) window.
            ui.add_space(ui.available_height() * 0.18);
            rainbow_title(ui, "vibehair");
            ui.add_space(22.0);

            let button = |ui: &mut egui::Ui, label: &str| {
                ui.add_sized([BUTTON_WIDTH, BUTTON_HEIGHT], egui::Button::new(label))
                    .clicked()
            };
            if button(ui, "open editor") {
                action = Action::OpenEditor;
            }
            ui.add_space(8.0);
            if running {
                if button(ui, "stop overlay") {
                    action = Action::StopOverlay;
                }
            } else if button(ui, "launch overlay") {
                action = Action::StartOverlay;
            }

            ui.add_space(18.0);
            let state = if running {
                "overlay: running"
            } else {
                "overlay: stopped"
            };
            ui.colored_label(
                if running {
                    egui::Color32::from_rgb(120, 220, 120)
                } else {
                    egui::Color32::from_gray(160)
                },
                egui::RichText::new(state).size(19.0),
            );
            if !status.is_empty() {
                ui.small(status);
            }
            if let Some(err) = error {
                ui.colored_label(egui::Color32::from_rgb(220, 90, 90), err);
            }
        });
    });
    action
}

impl App for LauncherApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut EframeFrame) {
        let running = self.overlay_running();
        let action = panel(ui, running, &self.status, self.error.as_deref());
        match action {
            Action::None => {}
            Action::OpenEditor => {
                let mut none = None;
                if let Err(err) = spawn_self(crate::state::Mode::Editor, &mut none) {
                    self.error = Some(err);
                }
            }
            Action::StartOverlay => self.start_overlay(),
            Action::StopOverlay => self.stop_overlay(),
        }
    }

    /// Closing this window also stops the overlay it started.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(mut child) = self.overlay.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

pub fn run() -> eframe::Result {
    eframe::run_native(
        "vibehair",
        NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("vibehair")
                .with_inner_size([340.0, 300.0])
                .with_resizable(false),
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(LauncherApp::new(cc)))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launcher() -> LauncherApp {
        LauncherApp::new(&CreationContext::_new_kittest(egui::Context::default()))
    }

    #[test]
    fn launcher_starts_idle_without_an_overlay_child() {
        let app = launcher();
        assert!(app.overlay.is_none());
        assert!(app.error.is_none());
    }

    /// The stop request must be consumable, so a stale request cannot stop the
    /// next overlay the user starts.
    #[test]
    fn stop_requests_are_consumed_once() {
        let mut launcher = launcher();
        launcher.start_overlay();
        assert!(platform::request_overlay_stop().is_ok());
        assert!(platform::take_overlay_stop_request());
        assert!(!platform::take_overlay_stop_request());
        launcher.stop_overlay();
    }
}
