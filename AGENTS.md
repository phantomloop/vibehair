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
- Staying above the game (`platform_windows::enforce_overlay`, called once per second from the
  overlay): a borderless game re-asserts its own `HWND_TOPMOST` after us and Windows can drop
  window styles, so the watchdog re-asserts `SetWindowPos(HWND_TOPMOST, .., SWP_NOACTIVATE)` and
  repairs styles if they changed. `WATCHDOG_INTERVAL` is the cadence: one egui pass per second is
  the whole cost (16 passes measure <10 ms in release). The overlay NEVER touches another process
  or another process's window - only its own HWND; the monitor to place the crosshair on is picked
  by *querying* the foreground window's monitor (`GetForegroundWindow` + `MonitorFromWindow`), so a
  game on a secondary screen still gets the crosshair.
- Positioning is open loop and intentionally simple: the window is placed at
  `monitor center + offsets` and re-sent only when the wanted value changes. A measured
  closed-loop correction was tried in v0.1.6 and removed again: reading `inner_rect` in the same
  pass that the position was requested measures a stale rect, and the resulting "corrections"
  marched the window across the screen. Do not re-add self-correcting placement without a verified
  settling delay, a large-error escape (re-send the absolute position) and a hard stop.
- One overlay per *user*, not per executable copy: the instance lock lives in
  `platform::control_dir()` (`%LOCALAPPDATA%\vibehair`, `$XDG_RUNTIME_DIR`/`~/.cache/vibehair`), so
  stale overlays from older downloads cannot each draw their own crosshair.
- Stopping an overlay: `platform::request_overlay_stop()` writes `overlay.stop` in the control
  directory; the overlay's 1 Hz watchdog consumes it and closes. The launcher stays open as a
  control panel (start/stop) and kills the overlay it started on exit; the editor has a stop button.
- The `vibehair` wordmark is an animated rainbow (`widgets::rainbow_title`): hue varies along X and
  advances with time. Only the launcher/editor pay for the animation repaints; the overlay does not.
- Visual checks without a display: `cargo test --features preview-render write_previews -- --nocapture`
  renders the launcher states and the editor to `target/test-scratch/preview-*.png` (wgpu; on this
  server set `VK_ICD_FILENAMES` to a manifest pointing at `libvulkan_lvp.so`). The same feature
  provides `memory_report`, which drives each window for 2000 frames and prints resident memory
  growth per window plus egui's galley-cache size - run it *alone*, since a wgpu renderer in the
  same process allocates enough to look like a leak.
- The wordmark lays out each glyph once and caches the galleys in egui temp data; per-frame it only
  paints them with fresh colours, and it stops requesting repaints when the window loses focus.
  Do not call `fonts_mut` inside `data_mut`: both take the context write lock and deadlock.
- Memory: each window is its own process (winit allows one event loop per process) and pays for one
  GL context + DWM-composited window on top of the egui context, so the *number of resident
  processes* is the dominant term, not our heap. Measured app heap: launcher ~0.37 MiB, editor
  ~0.43 MiB, overlay ~0.32 MiB, 0 bytes/frame over 1800 frames each. The launcher hands over to the
  editor (and can also exit after launching the overlay, "close after launching") so a menu-like
  window never stays resident next to the window it opened; detached children are dropped handles,
  not kills - see `spawn_self` and `closes_after` in `launcher.rs`.
- Memory diagnostics on a user's machine: every process logs its own working set once at startup
  (`platform::working_set_mb`, one line per process in `vibehair.log`), so `Launcher`/`Editor`/
  `Overlay` lines are directly comparable. `--stop` requests an overlay stop without opening a
  window, which is how an overlay started in close-after-launch mode is stopped.
- Fonts: `widgets::install_fonts` drops the emoji fonts egui embeds (nothing here draws one). UI
  strings stay ASCII; non-ASCII text such as a user path is covered by the proportional font.
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
