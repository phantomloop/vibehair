// Hide the console window on Windows: vibehair is a GUI app, not a CLI tool.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod config;
mod editor;
mod geometry;
mod launcher;
mod overlay;
mod platform;
#[cfg(target_os = "windows")]
mod platform_windows;
#[cfg(all(test, feature = "preview-render"))]
mod preview;
mod render;
mod state;
mod widgets;

use std::ffi::OsString;

fn main() -> eframe::Result {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let mode = match state::parse_args(&args) {
        Ok(mode) => mode,
        Err(msg) => {
            platform::log_line(&msg);
            std::process::exit(2);
        }
    };

    if mode == state::Mode::Stop {
        // No window and no graphics stack: this must work from a terminal too.
        match platform::request_overlay_stop() {
            Ok(()) => platform::log_line("vibehair: stop requested"),
            Err(err) => {
                platform::log_line(&format!("vibehair: cannot request stop: {err}"));
                return Ok(());
            }
        }
        return Ok(());
    }

    // One line per process, so a user can measure where their memory goes:
    // this is the whole resident set (winit, glow and the driver included).
    match platform::working_set_mb() {
        Some(mb) => platform::log_line(&format!(
            "vibehair: {mode:?} process, working set {mb:.0} MB"
        )),
        None => platform::log_line(&format!(
            "vibehair: {mode:?} process, working set unavailable"
        )),
    }

    let result = match mode {
        state::Mode::Launcher => launcher::run(),
        state::Mode::Editor => editor::run(),
        state::Mode::Overlay => overlay::run(),
        // Handled above, before any window exists.
        state::Mode::Stop => Ok(()),
    };
    if let Err(err) = &result {
        platform::log_line(&format!("vibehair: {mode:?} failed: {err}"));
    }
    result
}
