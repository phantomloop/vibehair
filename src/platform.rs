//! OS-specific paths. Everything else stays platform-neutral.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_config_file_ends_with_filename() {
        if let Some(path) = user_config_file() {
            assert_eq!(path.file_name().and_then(|n| n.to_str()), Some(FILENAME));
        }
    }
}
