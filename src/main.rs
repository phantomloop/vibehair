mod config;
mod editor;
mod geometry;
mod launcher;
mod overlay;
mod platform;
mod render;
mod state;

use std::ffi::OsString;

fn main() -> eframe::Result {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let mode = match state::parse_args(&args) {
        Ok(mode) => mode,
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(2);
        }
    };
    match mode {
        state::Mode::Launcher => launcher::run(),
        state::Mode::Editor => editor::run(),
        state::Mode::Overlay => overlay::run(),
    }
}
