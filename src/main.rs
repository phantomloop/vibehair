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
mod render;
mod state;

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
    let result = match mode {
        state::Mode::Launcher => launcher::run(),
        state::Mode::Editor => editor::run(),
        state::Mode::Overlay => overlay::run(),
    };
    if let Err(err) = &result {
        platform::log_line(&format!("vibehair: {mode:?} failed: {err}"));
    }
    result
}
