//! OS-specific paths, process details and diagnostics. Everything else stays
//! platform-neutral.

use std::path::PathBuf;
use std::sync::LazyLock;

use crate::config::FILENAME;

/// Directory holding the running executable, if it can be determined.
pub fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(std::path::Path::to_path_buf)
}

/// Per-user config file path, used when the exe directory is not writable.
#[cfg(target_os = "windows")]
pub fn user_config_file() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|dir| PathBuf::from(dir).join("vibehair").join(FILENAME))
}

/// Per-user config file path, used when the exe directory is not writable.
#[cfg(not(target_os = "windows"))]
pub fn user_config_file() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(dir).join("vibehair").join(FILENAME));
    }
    std::env::var_os("HOME").map(|dir| {
        PathBuf::from(dir)
            .join(".config")
            .join("vibehair")
            .join(FILENAME)
    })
}

/// Stop a spawned child from flashing a console window on Windows.
pub fn hide_console(cmd: &mut std::process::Command) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(target_os = "windows"))]
    let _ = cmd;
}

/// Extended window style bit that makes a window click-through.
#[cfg(target_os = "windows")]
pub const WS_EX_TRANSPARENT: u32 = 0x0000_0020;
/// Extended window style bit that hands compositing to the app; on Windows it
/// stops DWM from compositing our per-pixel-alpha content, which makes the
/// window invisible (winit sets it for `mouse_passthrough`, see emilk/egui#2537).
#[cfg(target_os = "windows")]
pub const WS_EX_LAYERED: u32 = 0x0008_0000;

/// Extended window style for a click-through overlay that still composites.
#[cfg(target_os = "windows")]
pub fn click_through_style(ex_style: u32) -> u32 {
    (ex_style | WS_EX_TRANSPARENT) & !WS_EX_LAYERED
}

/// Diagnostics file, resolved once next to the settings file.
static LOG_PATH: LazyLock<PathBuf> = LazyLock::new(|| {
    let mut path = crate::config::resolve_save_path();
    path.set_file_name("vibehair.log");
    path
});

/// Report a diagnostic. A Windows GUI process has no visible stderr, so the
/// message is also written to `vibehair.log` next to the settings file.
/// Never panics and never blocks on I/O errors.
pub fn log_line(msg: &str) {
    use std::io::Write as _;

    let _ = writeln!(std::io::stderr(), "{msg}");

    static FIRST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
    let truncate = FIRST.swap(false, std::sync::atomic::Ordering::Relaxed);
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(truncate)
        .append(!truncate)
        .open(&*LOG_PATH)
    {
        let _ = writeln!(file, "{msg}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_config_file_ends_with_filename() {
        if let Some(path) = user_config_file() {
            assert_eq!(path.file_name().and_then(|n| n.to_str()), Some(FILENAME));
        }
    }

    #[test]
    fn hide_console_is_callable_on_this_platform() {
        let mut cmd = std::process::Command::new("vibehair-nonexistent-probe");
        hide_console(&mut cmd);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn click_through_style_adds_transparent_and_drops_layered() {
        // Window created without any extended styles:
        assert_eq!(click_through_style(0), WS_EX_TRANSPARENT);
        // winit's mouse_passthrough variant must lose WS_EX_LAYERED:
        let layered_and_click_through = WS_EX_TRANSPARENT | WS_EX_LAYERED;
        assert_eq!(
            click_through_style(layered_and_click_through),
            WS_EX_TRANSPARENT
        );
        // Unrelated styles (topmost, no-activate, toolwindow) survive:
        let others = 0x0000_0008 | 0x0800_0000 | 0x0000_0080 | WS_EX_LAYERED;
        assert_eq!(
            click_through_style(others),
            (others | WS_EX_TRANSPARENT) & !WS_EX_LAYERED
        );
    }
}
