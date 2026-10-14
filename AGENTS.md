# Project instructions

- Write all file content in English.
- Keep vibehair a minimalist crosshair game overlay for Windows and Linux.
- Prioritize low CPU usage, low memory usage, and small binaries.
- Keep dependencies and features minimal; avoid unnecessary abstractions.
- Isolate platform-specific code and preserve support for both platforms.
- Keep README.md extremely short.
- Check Rust changes with `cargo fmt --check` and `cargo check`.

## Architecture (v0.1.0)

- Single crate/binary `vibehair`; modes: launcher (default), `--editor`, `--overlay`.
- GUI: `eframe` + `egui` 0.36 with `glow` backend only (no wgpu). Settings: `serde` + `toml`.
- Modules: `state` (CLI/mode), `config` (TOML load/save/clamp), `geometry` (pure pixel math),
  `render` (ONE shared painter used by editor preview AND overlay), `launcher`, `editor`,
  `overlay` (transparent/topmost/click-through), `platform` (exe-dir/user-config paths).
- Overlay window: small square sized to the crosshair, centered via `monitor_size` + manual
  X/Y offsets, event-driven repaint (no idle loop). Game offset is user-calibrated; never guess it.
- Crosshair sizes are **physical pixels**: `render::paint_crosshair` takes `pixels_per_point` and
  converts to points, so the editor preview and the overlay show the same pixel size at any DPI.
  Tested by `render::tests` (physical size, stroke width, and a no-clipping check per DPI scale).
- Windows overlay (`platform_windows.rs`): neither winit `mouse_passthrough` (adds `WS_EX_LAYERED`
  without layer attributes → content is not composited, crosshair invisible; emilk/egui#2537) nor
  `with_transparent` (DWM blur-behind → glassy, shadowed blob on Win11) is used by default. Instead
  the window is opaque, `SetLayeredWindowAttributes(.., LWA_COLORKEY)` makes one key color
  transparent, and DWM shadow/rounded corners are disabled. Click-through comes from
  `WS_EX_TRANSPARENT`. A style change only takes effect after `SetWindowPos(.., SWP_FRAMECHANGED)`,
  so window setup is retried once. `Settings::transparency` (`key` | `alpha`) selects the mechanism
  and is user-settable in `vibehair.toml`; `alpha` requests per-pixel transparency through the
  viewport builder and must NOT be layered. `window.set_undecorated_shadow(false)` is mandatory:
  egui-winit enables winit's drop shadow for borderless windows, which draws a 1px line on top.
  Verify with `cargo clippy --target x86_64-pc-windows-msvc --all-targets -- -D warnings`.
- Diagnostics: `platform::log_line` writes `vibehair.log` next to the settings file, because a
  `windows_subsystem = "windows"` process has no visible stderr.
- Windows release builds link the CRT statically (`.cargo/config.toml`); `release.yml` fails if
  `VCRUNTIME140`/`MSVCP140` shows up in the exe, so users need no VC++ redistributable.
- No game injection/hooks/drivers; plain user-space window. No telemetry.

## Working rules

- Server is headless (no X11/Wayland): never run GUI windows here; cover logic with unit tests.
- All agent scratch goes under ignored `.agent/` (`tmp/`, `reports/`, `coordination/`, `state.md`,
  `plan.md`, `decisions.md`, `issues.md`, `release-checklist.md`). Never use `/tmp` for own files.
- Do not edit `README.md`. Do not rename the project/package.
- GitHub: no `gh` CLI; push via git, read CI via public REST + `curl` into `.agent/reports/ci/`.
- Release gates: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test`, `cargo build --release`, plus green Linux + Windows CI on the tagged SHA.
  Never claim Windows/Hunt compatibility without on-machine verification.
