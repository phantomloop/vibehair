//! OS-specific paths and process details. Everything else stays platform-neutral.

use std::path::PathBuf;

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
}
