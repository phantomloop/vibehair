//! Startup mode selection: launcher, editor, or overlay.
//!
//! Pure logic, no windows: `cargo test` covers the whole state machine.

use std::ffi::OsString;

/// Which window the executable opens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// Small choice window: Open Editor / Launch Overlay.
    #[default]
    Launcher,
    /// Settings editor with live preview.
    Editor,
    /// Transparent click-through overlay.
    Overlay,
}

/// Parse CLI args (excluding argv[0]). Unknown flags are an error; `--help`/`-h` too.
pub fn parse_args(args: &[OsString]) -> Result<Mode, String> {
    let mut mode = Mode::Launcher;
    let mut seen = false;
    for arg in args {
        let text = arg.to_string_lossy();
        let flag = match text.as_ref() {
            "--editor" => Some(Mode::Editor),
            "--overlay" => Some(Mode::Overlay),
            "--launcher" => Some(Mode::Launcher),
            "--help" | "-h" => return Err(usage()),
            _ => None,
        };
        match flag {
            Some(next) if !seen => {
                mode = next;
                seen = true;
            }
            Some(_) => return Err("only one of --launcher, --editor, --overlay".to_owned()),
            None => return Err(format!("unknown argument: {text}\n{}", usage())),
        }
    }
    Ok(mode)
}

pub fn usage() -> String {
    "usage: vibehair [--launcher | --editor | --overlay]".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    #[test]
    fn no_args_opens_launcher() {
        assert_eq!(parse_args(&[]), Ok(Mode::Launcher));
        assert_eq!(parse_args(&args(&["--launcher"])), Ok(Mode::Launcher));
    }

    #[test]
    fn each_mode_selects_exactly() {
        assert_eq!(parse_args(&args(&["--editor"])), Ok(Mode::Editor));
        assert_eq!(parse_args(&args(&["--overlay"])), Ok(Mode::Overlay));
    }

    #[test]
    fn conflicting_or_unknown_args_fail() {
        assert!(parse_args(&args(&["--editor", "--overlay"])).is_err());
        assert!(parse_args(&args(&["--bogus"])).is_err());
        assert!(parse_args(&args(&["--help"])).is_err());
    }
}
