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
