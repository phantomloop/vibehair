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

/// Color the overlay clears to.
///
/// In `key` mode this exact color is made transparent by the layer key, so it
/// must be one the crosshair never draws by accident: black, or magenta when
/// the crosshair itself is black. In `alpha` mode the window is genuinely
/// transparent and the clear color is fully transparent.
pub fn overlay_clear_color_for(crosshair: [u8; 3], mode: crate::config::Transparency) -> [f32; 4] {
    use crate::config::Transparency;
    match mode.effective() {
        Transparency::Key => {
            let [r, g, b] = overlay_key_rgb(crosshair);
            [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
        }
        Transparency::Alpha => [0.0, 0.0, 0.0, 0.0],
    }
}

/// The color keyed out on Windows: black, or magenta for a black crosshair.
pub fn overlay_key_rgb(crosshair: [u8; 3]) -> [u8; 3] {
    if crosshair == [0, 0, 0] {
        [255, 0, 255]
    } else {
        [0, 0, 0]
    }
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

    #[test]
    fn overlay_clear_color_follows_the_transparency_mode() {
        use crate::config::Transparency;
        // Alpha mode is genuinely transparent everywhere.
        assert_eq!(
            overlay_clear_color_for([0, 255, 0], Transparency::Alpha),
            [0.0, 0.0, 0.0, 0.0]
        );
        // Key mode needs an opaque key color; only Windows honours it.
        let keyed = overlay_clear_color_for([0, 255, 0], Transparency::Key);
        assert_eq!(&keyed[..3], &[0.0, 0.0, 0.0]);
        #[cfg(target_os = "windows")]
        assert_eq!(keyed[3], 1.0, "the key color must be opaque to be visible");
        #[cfg(not(target_os = "windows"))]
        assert_eq!(keyed[3], 0.0, "key mode resolves to alpha off Windows");
    }

    #[test]
    fn a_black_crosshair_is_never_the_key_color() {
        assert_eq!(overlay_key_rgb([0, 255, 0]), [0, 0, 0]);
        assert_eq!(overlay_key_rgb([255, 255, 255]), [0, 0, 0]);
        // A black crosshair would vanish if black were keyed out:
        assert_eq!(overlay_key_rgb([0, 0, 0]), [255, 0, 255]);
        // The keyed color is only used where a key is actually applied.
        #[cfg(target_os = "windows")]
        assert_ne!(
            overlay_clear_color_for([0, 0, 0], crate::config::Transparency::Key)[..3],
            [0.0, 0.0, 0.0]
        );
    }
}
