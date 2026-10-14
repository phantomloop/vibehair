//! Startup selector: Open Editor / Launch Overlay.
//!
//! The chosen mode is spawned as a child process: winit allows exactly one
//! event loop per process, so a second `run_native` in-process would fail.
//! Spawning happens exactly once; the launcher then closes (bug: repeated
//! spawns while the close request was still in flight).

use std::process::Command;

use eframe::egui::CentralPanel;
use eframe::{App, CreationContext, Frame as EframeFrame, NativeOptions};

use crate::platform;

enum Next {
    None,
    Editor,
    Overlay,
}

fn flag(next: &Next) -> Option<&'static str> {
    match next {
        Next::None => None,
        Next::Editor => Some("--editor"),
        Next::Overlay => Some("--overlay"),
    }
}

/// Relaunch this exe with `flag`. Returns the error message on failure.
fn spawn_self(flag: &str) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|err| format!("cannot locate own exe: {err}"))?;
    let mut cmd = Command::new(exe);
    cmd.arg(flag);
    platform::hide_console(&mut cmd);
    cmd.spawn().map_err(|err| format!("launch failed: {err}"))?;
    Ok(())
}

pub struct LauncherApp {
    next: Next,
    /// Set once a mode has been spawned, so a pending close cannot spawn twice.
    launched: bool,
    error: Option<String>,
}

impl LauncherApp {
    pub fn new(_cc: &CreationContext<'_>) -> Self {
        Self {
            next: Next::None,
            launched: false,
            error: None,
        }
    }

    /// The flag to spawn, taken at most once. Returns `None` on every later
    /// call, so the frames between requesting a close and the close arriving
    /// cannot spawn the chosen mode a second time.
    fn take_pending_spawn(&mut self) -> Option<&'static str> {
        let wanted = flag(&self.next);
        self.next = Next::None;
        if self.launched { None } else { wanted }
    }
}

impl App for LauncherApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut EframeFrame) {
        CentralPanel::default().show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading("vibehair");
                ui.add_space(8.0);
                if ui.button("open editor").clicked() && !self.launched {
                    self.next = Next::Editor;
                }
                if ui.button("launch overlay").clicked() && !self.launched {
                    self.next = Next::Overlay;
                }
                if let Some(flag) = self.take_pending_spawn() {
                    // Spawn exactly once; keep the window open on failure.
                    match spawn_self(flag) {
                        Ok(()) => {
                            self.launched = true;
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        Err(err) => self.error = Some(err),
                    }
                }
                if let Some(err) = &self.error {
                    ui.small(err);
                }
            });
        });
    }
}

pub fn run() -> eframe::Result {
    eframe::run_native(
        "vibehair",
        NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("vibehair")
                .with_inner_size([260.0, 180.0])
                .with_resizable(false),
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(LauncherApp::new(cc)))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_map_to_modes() {
        assert_eq!(flag(&Next::Editor), Some("--editor"));
        assert_eq!(flag(&Next::Overlay), Some("--overlay"));
        assert_eq!(flag(&Next::None), None);
    }

    #[test]
    fn launcher_starts_idle_and_allows_a_single_spawn() {
        let app = LauncherApp::new(&CreationContext::_new_kittest(egui::Context::default()));
        assert!(!app.launched);
        assert_eq!(flag(&app.next), None);
        assert!(app.error.is_none());
    }

    /// Regression test: the click used to be re-processed on the frames before
    /// the close request took effect, spawning two editors.
    #[test]
    fn a_choice_spawns_exactly_once() {
        let mut app = LauncherApp::new(&CreationContext::_new_kittest(egui::Context::default()));
        app.next = Next::Editor;
        assert_eq!(app.take_pending_spawn(), Some("--editor"));
        // Later frames before the window closes must not spawn again.
        assert_eq!(app.take_pending_spawn(), None);
        assert_eq!(app.take_pending_spawn(), None);
        // After a successful spawn, a fresh choice is ignored too.
        app.launched = true;
        app.next = Next::Overlay;
        assert_eq!(app.take_pending_spawn(), None);
    }

    #[test]
    fn failed_spawn_leaves_the_launcher_open_for_retry() {
        let mut app = LauncherApp::new(&CreationContext::_new_kittest(egui::Context::default()));
        app.next = Next::Editor;
        assert_eq!(app.take_pending_spawn(), Some("--editor"));
        app.error = Some("launch failed".to_owned());
        assert!(
            !app.launched,
            "a failure must not latch the launcher closed"
        );
        // The user can try again.
        app.next = Next::Overlay;
        assert_eq!(app.take_pending_spawn(), Some("--overlay"));
    }
}
