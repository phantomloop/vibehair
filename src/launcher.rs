//! Startup selector: Open Editor / Launch Overlay.
//!
//! The chosen mode is spawned as a child process: winit allows exactly one
//! event loop per process, so a second `run_native` in-process would fail.

use std::process::Command;

use eframe::egui::CentralPanel;
use eframe::{App, CreationContext, Frame as EframeFrame, NativeOptions};

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

/// Relaunch this exe with `flag`. Returns an error message for the UI on failure.
fn spawn_self(flag: &str) -> Option<String> {
    match std::env::current_exe() {
        Ok(exe) => match Command::new(exe).arg(flag).spawn() {
            Ok(_) => None,
            Err(err) => Some(format!("launch failed: {err}")),
        },
        Err(err) => Some(format!("cannot locate own exe: {err}")),
    }
}

pub struct LauncherApp {
    next: Next,
    error: Option<String>,
}

impl LauncherApp {
    pub fn new(_cc: &CreationContext<'_>) -> Self {
        Self {
            next: Next::None,
            error: None,
        }
    }
}

impl App for LauncherApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut EframeFrame) {
        CentralPanel::default().show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading("vibehair");
                ui.add_space(8.0);
                if ui.button("open editor").clicked() {
                    self.next = Next::Editor;
                }
                if ui.button("launch overlay").clicked() {
                    self.next = Next::Overlay;
                }
                if let Some(flag) = flag(&self.next) {
                    // Spawn first so a failure keeps the launcher open with an error.
                    if let Some(err) = spawn_self(flag) {
                        self.error = Some(err);
                        self.next = Next::None;
                    } else {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
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
}
