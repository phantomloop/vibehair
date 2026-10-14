//! Startup selector and overlay control panel.
//!
//! The chosen mode is spawned as a child process: winit allows exactly one
//! event loop per process, so a second `run_native` in-process would fail.
//! This window stays open so the overlay can be started *and stopped*: without
//! it, a click-through overlay that covers the screen had to be killed from the
//! task manager (reported). For the smallest possible memory use the panel
//! offers "close after launching": the window then exits and leaves only the
//! overlay process running, and a later run of the executable shows the same
//! menu with "stop overlay" because the overlay holds the single-instance lock.
//! Opening the editor always hands over the same way, so at most one menu-like
//! process is resident at a time.
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
///
/// The caller decides whether to keep the returned handle: dropping it lets the
/// child outlive this window (nothing kills it on exit), which is what the
/// "close after launching" mode needs.
fn spawn_self(mode: crate::state::Mode) -> Result<Child, String> {
    let exe = std::env::current_exe().map_err(|err| format!("cannot locate own exe: {err}"))?;
    let flag = match mode {
        crate::state::Mode::Editor => "--editor",
        crate::state::Mode::Overlay => "--overlay",
        crate::state::Mode::Launcher | crate::state::Mode::Stop => {
            return Err("that is not a window mode".to_owned());
        }
    };
    let mut cmd = Command::new(exe);
    cmd.arg(flag);
    platform::hide_console(&mut cmd);
    cmd.spawn().map_err(|err| format!("launch failed: {err}"))
}

pub struct LauncherApp {
    /// The overlay this window started and still owns, if any.
    overlay: Option<Child>,
    /// Stored settings: only `close_after_launch` is used here, and toggling it
    /// is saved immediately so the choice survives the window closing itself.
    settings: crate::config::Settings,
    /// How this window starts another mode. A function pointer so tests can
    /// substitute a harmless process: a real overlay would take the
    /// single-instance lock and make unrelated tests race for it.
    spawn: fn(crate::state::Mode) -> Result<Child, String>,
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
        // A read failure is not fatal here: the editor reports it.
        let (settings, _path, _err) = crate::config::load();
        Self {
            overlay: None,
            settings,
            spawn: spawn_self,
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
        match (self.spawn)(crate::state::Mode::Overlay) {
            Ok(child) => {
                if self.settings.close_after_launch {
                    // Dropping the handle leaves the overlay running on its own.
                    drop(child);
                } else {
                    self.overlay = Some(child);
                }
                self.status = "overlay started".to_owned();
                self.refresh_running();
            }
            Err(err) => self.error = Some(err),
        }
    }

    /// Whether this window exits after `action`.
    ///
    /// Opening the editor always hands over (the editor replaces this menu), and
    /// starting the overlay hands over only in "close after launching" mode and
    /// only if it really started.
    fn closes_after(&self, action: Action) -> bool {
        match action {
            Action::OpenEditor => true,
            Action::StartOverlay => self.settings.close_after_launch && self.error.is_none(),
            Action::None | Action::StopOverlay => false,
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
pub fn panel(
    ui: &mut egui::Ui,
    running: bool,
    status: &str,
    error: Option<&str>,
    close_after_launch: &mut bool,
) -> Action {
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

            ui.add_space(14.0);
            ui.checkbox(close_after_launch, "close after launching");
            if *close_after_launch {
                ui.small("the menu exits; run vibehair again to stop the overlay");
            }

            ui.add_space(14.0);
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
        let was = self.settings.close_after_launch;
        let action = panel(
            ui,
            running,
            &self.status,
            self.error.as_deref(),
            &mut self.settings.close_after_launch,
        );
        if self.settings.close_after_launch != was
            && let Err(err) = crate::config::save(&self.settings)
        {
            self.error = Some(format!("cannot save the setting: {err}"));
        }
        match action {
            Action::None => {}
            Action::OpenEditor => {
                // Handing over: the editor is a full window of its own, and this
                // menu has nothing left to do once it is open. Exiting here keeps
                // a single process resident while editing. An overlay this menu
                // started is detached first, so it keeps running and the editor
                // can still stop it through the stop file.
                if self.overlay.take().is_some() {
                    crate::platform::log_line("vibehair: overlay handed over to the editor");
                }
                if let Err(err) = (self.spawn)(crate::state::Mode::Editor) {
                    self.error = Some(err);
                }
            }
            Action::StartOverlay => self.start_overlay(),
            Action::StopOverlay => self.stop_overlay(),
        }
        if self.closes_after(action) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
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
                .with_inner_size([340.0, 340.0])
                .with_resizable(false),
            // The crosshair is flat 2D: no depth, stencil or MSAA buffers.
            depth_buffer: 0,
            stencil_buffer: 0,
            multisampling: 0,
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(LauncherApp::new(cc)))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Starts the test binary itself with libtest's `--list`: it exits at once
    /// and never touches the overlay lock, so the tests stay independent.
    fn quiet_spawn(_mode: crate::state::Mode) -> Result<Child, String> {
        let exe = std::env::current_exe().map_err(|err| err.to_string())?;
        let mut cmd = Command::new(exe);
        cmd.arg("--list")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        cmd.spawn().map_err(|err| err.to_string())
    }

    fn launcher() -> LauncherApp {
        let mut app = LauncherApp::new(&CreationContext::_new_kittest(egui::Context::default()));
        app.spawn = quiet_spawn;
        app
    }

    #[test]
    fn launcher_starts_idle_without_an_overlay_child() {
        let app = launcher();
        assert!(app.overlay.is_none());
        assert!(app.error.is_none());
    }

    /// "close after launching" must not keep the overlay as a child: a child
    /// handle would be killed when this window exits.
    #[test]
    fn close_after_launch_does_not_track_the_overlay() {
        let mut keeping = launcher();
        keeping.settings.close_after_launch = false;
        keeping.start_overlay();
        assert!(keeping.overlay.is_some(), "default mode keeps the handle");
        keeping.stop_overlay();

        let mut detached = launcher();
        detached.settings.close_after_launch = true;
        detached.start_overlay();
        assert!(detached.error.is_none(), "{:?}", detached.error);
        assert!(
            detached.overlay.is_none(),
            "detached overlay must not be owned"
        );
    }

    /// Opening the editor must not take a running overlay down with it: the
    /// child is forgotten so `on_exit` finds nothing to kill, and the editor
    /// stops it through the cross-process stop file instead.
    #[test]
    fn handing_over_to_the_editor_detaches_the_overlay() {
        let mut app = launcher();
        app.start_overlay();
        assert!(app.overlay.is_some(), "the menu owns its overlay first");
        // Same step the OpenEditor arm performs before spawning the editor.
        app.overlay.take();
        assert!(
            app.overlay.is_none(),
            "a detached overlay survives the menu closing"
        );
        let _ = platform::take_overlay_stop_request();
    }

    /// Which clicks make the window exit: the editor always hands over, the
    /// overlay only in "close after launching" mode, and a click that failed
    /// must keep the window (and its error message) on screen.
    #[test]
    fn close_rules_follow_the_setting_and_the_outcome() {
        let mut app = launcher();
        assert!(app.closes_after(Action::OpenEditor));
        assert!(!app.closes_after(Action::None));
        assert!(!app.closes_after(Action::StopOverlay));

        assert!(!app.closes_after(Action::StartOverlay));
        app.settings.close_after_launch = true;
        assert!(app.closes_after(Action::StartOverlay));
        app.error = Some("launch failed: access denied".to_owned());
        assert!(!app.closes_after(Action::StartOverlay));
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
