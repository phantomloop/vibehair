//! Windows-only window setup for the overlay.
//!
//! Three framework/OS behaviours have to be corrected here:
//!
//! 1. Click-through must not come from winit's `mouse_passthrough`: it adds
//!    `WS_EX_LAYERED` without any layer attributes, and Windows then stops
//!    compositing the window content at all, so the crosshair disappears
//!    (emilk/egui#2537, still open upstream).
//! 2. Transparency must not come from winit's `with_transparent` either: that
//!    path enables DWM blur-behind, which renders the tiny window as a glassy,
//!    shadowed blob instead of clean pixels.
//! 3. egui-winit enables winit's undecorated drop shadow for borderless
//!    windows; per winit's own documentation that "causes a thin 1px line to
//!    appear on the top of the window", which is exactly the artifact users
//!    see above the crosshair.
//!
//! Instead the window keeps its own pixels and Windows makes one key color
//! transparent (`LWA_COLORKEY`), i.e. exactly the crosshair is visible. The
//! shadow, Win11 rounded corners and non-client rendering are switched off so
//! nothing is drawn around or over those pixels.

use winit::platform::windows::WindowExtWindows as _;
use winit::raw_window_handle::{HasWindowHandle as _, RawWindowHandle};

use crate::platform::log_line;

const GWL_EXSTYLE: i32 = -20;

const WS_EX_TRANSPARENT: u32 = 0x0000_0020;
const WS_EX_LAYERED: u32 = 0x0008_0000;
const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
const WS_EX_NOACTIVATE: u32 = 0x0800_0000;

const SWP_NOSIZE: u32 = 0x0001;
const SWP_NOMOVE: u32 = 0x0002;
const SWP_NOZORDER: u32 = 0x0004;
const SWP_NOACTIVATE: u32 = 0x0010;
const SWP_FRAMECHANGED: u32 = 0x0020;
const SWP_NOOWNERZORDER: u32 = 0x0200;

/// `SetWindowPos` insert-after value that puts a window in the topmost band.
const HWND_TOPMOST: isize = -1;
const MONITOR_DEFAULTTONEAREST: u32 = 2;

const LWA_COLORKEY: u32 = 0x0000_0001;

const DWMWA_NCRENDERING_POLICY: u32 = 2;
const DWMNCRP_DISABLED: u32 = 1;
const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
const DWMWCP_DONOTROUND: u32 = 1;

/// Win32 `COLORREF` is `0x00BBGGRR`.
fn colorref(r: u8, g: u8, b: u8) -> u32 {
    u32::from(r) | (u32::from(g) << 8) | (u32::from(b) << 16)
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct WinRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
#[derive(Default)]
struct MonitorInfo {
    cb_size: u32,
    rc_monitor: WinRect,
    rc_work: WinRect,
    dw_flags: u32,
}

#[link(name = "user32")]
unsafe extern "system" {
    fn GetForegroundWindow() -> isize;
    fn GetWindowRect(hwnd: isize, rect: *mut WinRect) -> i32;
    fn MonitorFromWindow(hwnd: isize, flags: u32) -> isize;
    fn GetMonitorInfoW(monitor: isize, info: *mut MonitorInfo) -> i32;
    fn GetWindowLongPtrW(hwnd: isize, index: i32) -> isize;
    fn SetWindowLongPtrW(hwnd: isize, index: i32, value: isize) -> isize;
    fn SetLayeredWindowAttributes(hwnd: isize, key: u32, alpha: u8, flags: u32) -> i32;
    fn SetWindowPos(
        hwnd: isize,
        insert_after: isize,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        flags: u32,
    ) -> i32;
}

#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmSetWindowAttribute(hwnd: isize, attribute: u32, value: *const u32, size: u32) -> i32;
}

/// Extended style for the overlay: click-through, hidden from the taskbar,
/// never activated, plus `WS_EX_LAYERED` in key mode only.
///
/// In alpha mode the window must *not* be layered: a layered window without
/// layer attributes stops Windows from compositing the content at all, which
/// is the invisible-crosshair bug (emilk/egui#2537).
pub fn overlay_style(ex_style: u32, mode: crate::config::Transparency) -> u32 {
    let base = ex_style | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
    match mode.effective() {
        crate::config::Transparency::Key => base | WS_EX_LAYERED,
        crate::config::Transparency::Alpha => base & !WS_EX_LAYERED,
    }
}

/// Monitor rectangle `(x, y, width, height)` in physical pixels that contains
/// `hwnd`. Reading another window's monitor is a plain query: the overlay never
/// touches or modifies the game's window or process.
fn monitor_rect_of(hwnd: isize) -> Option<(f32, f32, f32, f32)> {
    if hwnd == 0 {
        return None;
    }
    // Safety: only queries the monitor of an existing window handle.
    unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        if monitor == 0 {
            return None;
        }
        let mut info = MonitorInfo {
            cb_size: size_of::<MonitorInfo>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return None;
        }
        let r = info.rc_monitor;
        Some((
            r.left as f32,
            r.top as f32,
            (r.right - r.left) as f32,
            (r.bottom - r.top) as f32,
        ))
    }
}

/// Monitor of the foreground window, but only when that window **fills** it.
///
/// Following the focused window's monitor unconditionally made the crosshair
/// jump to the other screen whenever the user alt-tabbed (reported: a second
/// monitor at 2560,357 was picked while the game ran on the first one). A
/// window that covers its monitor is a game in borderless fullscreen, which is
/// the case the overlay wants to follow; anything else leaves the crosshair
/// where it is.
pub fn foreground_fullscreen_monitor_rect() -> Option<(f32, f32, f32, f32)> {
    // Safety: handles are queried, never modified; null is handled.
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd == 0 {
        return None;
    }
    let mut rect = WinRect::default();
    // Safety: `rect` is a valid out-parameter and the handle was just obtained.
    if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
        return None;
    }
    let monitor = monitor_rect_of(hwnd)?;
    let covers =
        |span: i32, monitor_span: f32| monitor_span > 1.0 && span as f32 >= monitor_span * 0.95;
    if covers(rect.right - rect.left, monitor.2) && covers(rect.bottom - rect.top, monitor.3) {
        Some(monitor)
    } else {
        None
    }
}

fn hwnd_of(window: &winit::window::Window) -> Result<isize, String> {
    let handle = window
        .window_handle()
        .map_err(|err| format!("no window handle: {err}"))?;
    match handle.as_raw() {
        RawWindowHandle::Win32(win32) => Ok(win32.hwnd.get()),
        _ => Err("unexpected window handle kind".to_owned()),
    }
}

/// Keep the overlay above the game.
///
/// A borderless game re-asserts `HWND_TOPMOST` on its own window (and Windows
/// can drop styles when a window is re-shown), which leaves the overlay behind
/// the game even though it was created topmost. Re-asserting our own topmost
/// state at a low, fixed cadence wins that race without ever touching the
/// game's window or process.
///
/// Returns `true` when the extended styles had to be repaired.
pub fn enforce_overlay(
    window: &winit::window::Window,
    crosshair_rgb: [u8; 3],
    mode: crate::config::Transparency,
) -> Result<bool, String> {
    let hwnd = hwnd_of(window)?;

    // Safety: `hwnd` belongs to our own live window; the calls only change that
    // window's Z-order position flags and (if needed) its styles. `SWP_NOACTIVATE`
    // keeps focus with the game.
    unsafe {
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
        );

        let current = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let wanted = overlay_style(current, mode);
        if current == wanted {
            return Ok(false);
        }
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, wanted as isize);
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_FRAMECHANGED,
        );
        if matches!(mode.effective(), crate::config::Transparency::Key) {
            let [r, g, b] = crate::platform::overlay_key_rgb(crosshair_rgb);
            let _ = SetLayeredWindowAttributes(hwnd, colorref(r, g, b), 0, LWA_COLORKEY);
        }
    }
    Ok(true)
}

/// Make the overlay a keyed, click-through window. Failures are logged, never fatal.
///
/// Safe to call repeatedly: the caller may retry once, because a style change
/// only takes effect after the window manager processes a frame change.
pub fn configure_overlay_window(
    window: &winit::window::Window,
    crosshair_rgb: [u8; 3],
    mode: crate::config::Transparency,
) -> Result<(), String> {
    // Removes the 1px shadow line winit draws above undecorated windows.
    window.set_undecorated_shadow(false);

    let handle = window
        .window_handle()
        .map_err(|err| format!("no window handle: {err}"))?;
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return Err("unexpected window handle kind".to_owned());
    };
    let hwnd = win32.hwnd.get();
    let key = match mode.effective() {
        crate::config::Transparency::Key => {
            let [r, g, b] = crate::platform::overlay_key_rgb(crosshair_rgb);
            Some(colorref(r, g, b))
        }
        crate::config::Transparency::Alpha => None,
    };

    // Safety: `hwnd` belongs to a live winit window on this thread; the calls
    // only read and write that window's styles, position flags and DWM
    // attributes.
    unsafe {
        let before = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, overlay_style(before, mode) as isize);

        // Style changes are only applied on a frame change; the layer
        // attributes below are rejected until then.
        let _ = SetWindowPos(
            hwnd,
            0,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );

        if let Some(key) = key {
            // The key color becomes fully transparent; alpha is unused with a key.
            if SetLayeredWindowAttributes(hwnd, key, 0, LWA_COLORKEY) == 0 {
                return Err(format!(
                    "SetLayeredWindowAttributes(LWA_COLORKEY, {key:#010x}) failed"
                ));
            }
        }
        // In alpha mode the window keeps the per-pixel transparency requested
        // through the viewport builder (`with_transparent`); nothing to do here.

        // No DWM shadow, no rounded corners, nothing drawn around our pixels.
        let ncrp = DWMNCRP_DISABLED;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_NCRENDERING_POLICY,
            &raw const ncrp,
            size_of::<u32>() as u32,
        );
        let corners = DWMWCP_DONOTROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &raw const corners,
            size_of::<u32>() as u32,
        );

        let after = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let layered = after & WS_EX_LAYERED != 0;
        if after & WS_EX_TRANSPARENT == 0 {
            return Err(format!("click-through style not applied: {after:#010x}"));
        }
        let want_layered = matches!(mode.effective(), crate::config::Transparency::Key);
        if layered != want_layered {
            return Err(format!(
                "window layer mode is wrong: layered={layered}, want={want_layered} ({after:#010x})"
            ));
        }
        log_line(&format!(
            "vibehair: overlay window configured (styles {before:#010x} -> {after:#010x}, key {}, no shadow)",
            match key {
                Some(key) => format!("{key:#010x}"),
                None => "none (alpha mode)".to_owned(),
            }
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colorref_uses_bbggrr_order() {
        assert_eq!(colorref(0, 0, 0), 0x0000_0000);
        assert_eq!(colorref(255, 0, 255), 0x00FF_00FF);
        assert_eq!(colorref(0x12, 0x34, 0x56), 0x0056_3412);
    }

    #[test]
    fn key_mode_is_layered_and_click_through() {
        use crate::config::Transparency;
        let style = overlay_style(0, Transparency::Key);
        assert_ne!(
            style & WS_EX_LAYERED,
            0,
            "must be layered for the color key"
        );
        assert_ne!(style & WS_EX_TRANSPARENT, 0, "must be click-through");
        assert_ne!(style & WS_EX_TOOLWINDOW, 0, "must stay out of the taskbar");
        assert_ne!(style & WS_EX_NOACTIVATE, 0, "must not steal focus");
    }

    /// A layered window without layer attributes is exactly the bug that made
    /// the crosshair invisible, so alpha mode must actively drop the flag.
    #[test]
    fn alpha_mode_drops_layered_and_keeps_click_through() {
        use crate::config::Transparency;
        let style = overlay_style(WS_EX_LAYERED | WS_EX_TOOLWINDOW, Transparency::Alpha);
        assert_eq!(style & WS_EX_LAYERED, 0, "alpha mode must not be layered");
        assert_ne!(style & WS_EX_TRANSPARENT, 0, "click-through stays");
        assert_ne!(style & WS_EX_TOOLWINDOW, 0, "taskbar hiding stays");
    }

    #[test]
    fn overlay_style_keeps_unrelated_bits_and_is_idempotent() {
        use crate::config::Transparency;
        let existing = 0x0000_0008; // WS_EX_TOPMOST
        for mode in [Transparency::Key, Transparency::Alpha] {
            let once = overlay_style(existing, mode);
            assert_eq!(once & 0x0000_0008, 0x0000_0008, "{mode:?}");
            assert_eq!(overlay_style(once, mode), once, "retries must be safe");
        }
    }
}
