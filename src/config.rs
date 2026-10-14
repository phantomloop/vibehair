//! TOML settings: validated defaults, clamping, portable load/save.
//!
//! A corrupt or out-of-range file never panics: unknown failures fall back to
//! [`Settings::default`], out-of-range fields are clamped on load.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::geometry::Shape;
use crate::platform::{exe_dir, user_config_file};

/// Settings file name, shared by the portable and user-config locations.
pub const FILENAME: &str = "vibehair.toml";

pub const MIN_SIZE: f32 = 0.0;
pub const MAX_SIZE: f32 = 200.0;
pub const MIN_GAP: f32 = 0.0;
pub const MAX_GAP: f32 = 200.0;
pub const MIN_THICKNESS: f32 = 0.25;
pub const MAX_THICKNESS: f32 = 32.0;
pub const MIN_OPACITY: f32 = 0.0;
pub const MAX_OPACITY: f32 = 1.0;
pub const MAX_OFFSET: i32 = 2000;

/// Clamp-or-fallback helper for finite floats.
fn clamp_finite(value: f32, fallback: f32, min: f32, max: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}

/// How the overlay window is made transparent.
///
/// `Key` is the deterministic Windows default: the window keeps its own pixels
/// and one key color is made fully transparent. `Alpha` uses per-pixel alpha
/// (DWM blur-behind on Windows). Only `Alpha` is meaningful off Windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Transparency {
    /// Opaque window with one transparent key color (Windows).
    Key,
    /// Per-pixel alpha through the windowing system.
    Alpha,
}

impl Default for Transparency {
    fn default() -> Self {
        if cfg!(target_os = "windows") {
            Self::Key
        } else {
            Self::Alpha
        }
    }
}

impl Transparency {
    /// The mode actually used on the running platform.
    pub fn effective(self) -> Self {
        if cfg!(target_os = "windows") {
            self
        } else {
            Self::Alpha
        }
    }
}

/// All user-adjustable crosshair settings. Field names are the TOML keys.
///
/// `default` here means a *missing* key keeps its default while every other key
/// in the file is still read: a hand-edited file that lost a line - or one
/// written by an older version - degrades per field instead of being discarded
/// whole.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub shape: Shape,
    /// Arm length (cross/T) or radius (circle/dot), in physical pixels.
    pub size: f32,
    /// Gap between center and arm start (cross/T), in physical pixels.
    pub gap: f32,
    /// Stroke width in pixels; quarter-pixel steps allowed (antialiased).
    pub thickness: f32,
    /// Crosshair opacity, 0 = invisible, 1 = solid.
    pub opacity: f32,
    /// sRGB color as `#rrggbb`, parsed case-insensitively.
    #[serde(
        default = "default_color",
        deserialize_with = "de_color",
        serialize_with = "se_color"
    )]
    pub color: [u8; 3],
    /// Overlay transparency mechanism; defaults to `key` on Windows.
    #[serde(default)]
    pub transparency: Transparency,
    /// Manual X offset from screen center, in physical pixels.
    pub offset_x: i32,
    /// Manual Y offset from screen center, in physical pixels (down is positive).
    pub offset_y: i32,
    /// Close the launcher/editor window as soon as it has started the overlay,
    /// so only the overlay process stays resident. Off by default: keeping the
    /// window open is what lets the menu stop the overlay.
    #[serde(default)]
    pub close_after_launch: bool,
}

fn default_color() -> [u8; 3] {
    [0, 255, 0]
}

fn se_color<S: serde::Serializer>(color: &[u8; 3], ser: S) -> Result<S::Ok, S::Error> {
    ser.serialize_str(&format!(
        "#{:02x}{:02x}{:02x}",
        color[0], color[1], color[2]
    ))
}

fn de_color<'de, D: serde::Deserializer<'de>>(de: D) -> Result<[u8; 3], D::Error> {
    let raw = String::deserialize(de)?;
    parse_hex_color(&raw).ok_or_else(|| serde::de::Error::custom("expected \"#rrggbb\""))
}

/// Parse `#rrggbb` (optional `#`, ASCII hex, case-insensitive).
pub fn parse_hex_color(raw: &str) -> Option<[u8; 3]> {
    let hex = raw.strip_prefix('#').unwrap_or(raw);
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            shape: Shape::Cross,
            size: 8.0,
            gap: 4.0,
            thickness: 2.0,
            opacity: 1.0,
            color: default_color(),
            transparency: Transparency::default(),
            offset_x: 0,
            offset_y: 0,
            close_after_launch: false,
        }
    }
}

impl Settings {
    /// Clamp every field into its valid range in place.
    pub fn clamp(&mut self) {
        let d = Self::default();
        self.size = clamp_finite(self.size, d.size, MIN_SIZE, MAX_SIZE);
        self.gap = clamp_finite(self.gap, d.gap, MIN_GAP, MAX_GAP);
        self.thickness = clamp_finite(self.thickness, d.thickness, MIN_THICKNESS, MAX_THICKNESS);
        self.opacity = clamp_finite(self.opacity, d.opacity, MIN_OPACITY, MAX_OPACITY);
        self.offset_x = self.offset_x.clamp(-MAX_OFFSET, MAX_OFFSET);
        self.offset_y = self.offset_y.clamp(-MAX_OFFSET, MAX_OFFSET);
    }

    /// Validated copy of `self`.
    pub fn clamped(mut self) -> Self {
        self.clamp();
        self
    }
}

/// Read settings from `path`. Missing file → defaults (not an error).
/// Malformed TOML or invalid values → defaults, plus the error for logging.
pub fn load_from(path: &Path) -> (Settings, Option<String>) {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return (Settings::default(), None);
        }
        Err(err) => return (Settings::default(), Some(err.to_string())),
    };
    match toml::from_str::<Settings>(&text) {
        Ok(settings) => (settings.clamped(), None),
        Err(err) => (Settings::default(), Some(err.to_string())),
    }
}

/// Write settings to `path`, creating parent directories, via atomic rename.
pub fn save_to(path: &Path, settings: &Settings) -> std::io::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    let text = toml::to_string_pretty(settings)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// Resolve the load path: portable file beside the exe wins if present,
/// otherwise the per-user config file.
pub fn resolve_load_path() -> PathBuf {
    if let Some(dir) = exe_dir() {
        let portable = dir.join(FILENAME);
        if portable.is_file() {
            return portable;
        }
    }
    user_config_file().unwrap_or_else(|| PathBuf::from(FILENAME))
}

/// Resolve the save path: beside the exe when that directory is writable,
/// otherwise the per-user config file.
pub fn resolve_save_path() -> PathBuf {
    if let Some(dir) = exe_dir() {
        let probe = dir.join(".vibehair-write-probe");
        if std::fs::write(&probe, []).is_ok() {
            let _ = std::fs::remove_file(&probe);
            return dir.join(FILENAME);
        }
    }
    user_config_file().unwrap_or_else(|| PathBuf::from(FILENAME))
}

/// Load from the resolved path.
pub fn load() -> (Settings, PathBuf, Option<String>) {
    let path = resolve_load_path();
    let (settings, err) = load_from(&path);
    (settings, path, err)
}

/// Save to the resolved path.
pub fn save(settings: &Settings) -> std::io::Result<PathBuf> {
    let path = resolve_save_path();
    save_to(&path, settings)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Scratch dir inside the (git-ignored) build tree; never the OS temp dir.
    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-scratch")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn defaults_are_within_bounds() {
        let mut s = Settings::default();
        let before = s.clone();
        s.clamp();
        assert_eq!(s, before);
    }

    #[test]
    fn clamp_repairs_out_of_range_and_non_finite() {
        let s = Settings {
            size: 9999.0,
            gap: -5.0,
            thickness: f32::NAN,
            opacity: 2.0,
            offset_x: i32::MAX,
            offset_y: i32::MIN,
            ..Settings::default()
        }
        .clamped();
        assert_eq!(s.size, MAX_SIZE);
        assert_eq!(s.gap, MIN_GAP);
        assert_eq!(s.thickness, Settings::default().thickness);
        assert_eq!(s.opacity, MAX_OPACITY);
        assert_eq!(s.offset_x, MAX_OFFSET);
        assert_eq!(s.offset_y, -MAX_OFFSET);
        let s = Settings {
            thickness: f32::INFINITY,
            ..Settings::default()
        }
        .clamped();
        assert_eq!(s.thickness, Settings::default().thickness);
    }

    #[test]
    fn color_round_trip_and_parse_variants() {
        assert_eq!(parse_hex_color("#00ff00"), Some([0, 255, 0]));
        assert_eq!(parse_hex_color("00FF00"), Some([0, 255, 0]));
        assert_eq!(parse_hex_color("#a0b1c2"), Some([0xa0, 0xb1, 0xc2]));
        for bad in ["", "#", "#fff", "#gggggg", "#1234567", "##123456"] {
            assert_eq!(parse_hex_color(bad), None, "input: {bad:?}");
        }
        let s = Settings {
            color: [171, 205, 239],
            ..Settings::default()
        };
        let text = toml::to_string(&s).unwrap();
        let back: Settings = toml::from_str(&text).unwrap();
        assert_eq!(back.color, [171, 205, 239]);
    }

    #[test]
    fn toml_round_trip_preserves_everything() {
        let s = Settings {
            shape: Shape::Circle,
            size: 12.5,
            gap: 3.0,
            thickness: 1.25,
            opacity: 0.8,
            color: [255, 0, 128],
            transparency: Transparency::Alpha,
            offset_x: -40,
            offset_y: 120,
            close_after_launch: true,
        };
        let text = toml::to_string(&s).unwrap();
        assert!(text.contains("close_after_launch = true"), "{text}");
        let back: Settings = toml::from_str(&text).unwrap();
        assert_eq!(back, s);
    }

    /// A file written before `close_after_launch` existed, and one that lost a
    /// line to hand editing, must keep every value they do contain.
    #[test]
    fn missing_keys_keep_their_defaults_and_the_rest_survives() {
        let dir = scratch("old-file");
        let file = dir.join("old.toml");
        // No `gap`, no `close_after_launch`: both must fall back, the rest stays.
        std::fs::write(
            &file,
            "shape = \"circle\"\nsize = 21.5\ntransparency = \"alpha\"\noffset_y = -12\n",
        )
        .unwrap();
        let (settings, err) = load_from(&file);
        assert!(err.is_none(), "{err:?}");
        assert_eq!(settings.shape, Shape::Circle);
        assert_eq!(settings.size, 21.5);
        assert_eq!(settings.transparency, Transparency::Alpha);
        assert_eq!(settings.offset_y, -12);
        assert_eq!(
            settings.gap,
            Settings::default().gap,
            "missing key defaulted"
        );
        assert!(!settings.close_after_launch, "new key defaults to off");
    }

    /// A wrong type is still a hard error for the whole file: silently mixing
    /// a broken value with defaults would hide the mistake.
    #[test]
    fn wrong_type_still_falls_back_to_defaults_with_a_message() {
        let dir = scratch("wrong-type");
        let file = dir.join("bad.toml");
        std::fs::write(&file, "size = \"not a number\"\n").unwrap();
        let (settings, err) = load_from(&file);
        assert!(err.is_some(), "a type error must be reported");
        assert_eq!(settings.size, Settings::default().size);
    }

    #[test]
    fn malformed_toml_falls_back_to_defaults() {
        let dir = scratch("malformed");
        let file = dir.join("bad.toml");
        std::fs::write(&file, "shape = [unclosed\nsize = ").unwrap();
        let (s, err) = load_from(&file);
        assert_eq!(s, Settings::default());
        assert!(err.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_then_load_round_trip() {
        let dir = scratch("roundtrip");
        let file = dir.join("sub").join("vibehair.toml");
        let s = Settings {
            offset_y: 64,
            ..Settings::default()
        };
        save_to(&file, &s).unwrap();
        assert!(!file.with_extension("toml.tmp").exists());
        let (back, err) = load_from(&file);
        assert!(err.is_none());
        assert_eq!(back, s);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn transparency_has_a_platform_default_and_round_trips() {
        assert_eq!(Settings::default().transparency, Transparency::default());
        if cfg!(target_os = "windows") {
            assert_eq!(Transparency::default(), Transparency::Key);
        } else {
            assert_eq!(Transparency::default(), Transparency::Alpha);
        }
        // A config file written before this option existed still loads.
        let full = toml::to_string(&Settings::default()).unwrap();
        let without = full
            .lines()
            .filter(|line| !line.starts_with("transparency"))
            .collect::<Vec<_>>()
            .join("\n");
        let upgraded: Settings = toml::from_str(&without).unwrap();
        assert_eq!(upgraded.transparency, Transparency::default());
        // Both values survive a write/read cycle.
        for mode in [Transparency::Key, Transparency::Alpha] {
            let s = Settings {
                transparency: mode,
                ..Settings::default()
            };
            let text = toml::to_string(&s).unwrap();
            let back: Settings = toml::from_str(&text).unwrap();
            assert_eq!(back.transparency, mode);
        }
        // Off Windows, `key` is meaningless and resolves to alpha.
        #[cfg(not(target_os = "windows"))]
        assert_eq!(Transparency::Key.effective(), Transparency::Alpha);
        #[cfg(target_os = "windows")]
        assert_eq!(Transparency::Key.effective(), Transparency::Key);
    }

    #[test]
    fn missing_file_loads_defaults_without_error() {
        let (s, err) = load_from(Path::new("definitely-not-here-vibehair.toml"));
        assert_eq!(s, Settings::default());
        assert!(err.is_none());
    }
}
