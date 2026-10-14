//! Windows-only window setup for the overlay.
//!
//! Two framework behaviours have to be corrected here:
//!
//! 1. Click-through must not come from winit's `mouse_passthrough`: it adds
//!    `WS_EX_LAYERED` without any layer attributes, and Windows then stops
//!    compositing the window content at all, so the crosshair disappears
//!    (emilk/egui#2537, still open upstream).
//! 2. Transparency must not come from winit's `with_transparent` either: that
//!    path enables DWM blur-behind, which on Windows 11 renders the window as
//!    a glassy, shadowed blob instead of clean pixels.
//!
//! Instead the window keeps its own pixels and Windows makes one key color
//! transparent (`LWA_COLORKEY`), i.e. exactly the crosshair is visible. The
//! DWM shadow, Win11 rounded corners and non-client rendering are switched off
//! so nothing is drawn around or over those pixels.

use winit::raw_window_handle::{HasWindowHandle as _, RawWindowHandle};

use crate::platform::log_line;

const GWL_EXSTYLE: i32 = -20;
const WS_EX_TRANSPARENT: u32 = 0x0000_0020;
const WS_EX_LAYERED: u32 = 0x0008_0000;
const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
const WS_EX_NOACTIVATE: u32 = 0x0800_0000;
const LWA_COLORKEY: u32 = 0x0000_0001;

const DWMWA_NCRENDERING_POLICY: u32 = 2;
const DWMNCRP_DISABLED: u32 = 1;
const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
const DWMWCP_DONOTROUND: u32 = 1;

/// Win32 `COLORREF` is `0x00BBGGRR`.
fn colorref(r: u8, g: u8, b: u8) -> u32 {
    u32::from(r) | (u32::from(g) << 8) | (u32::from(b) << 16)
}

#[link(name = "user32")]
unsafe extern "system" {
    fn GetWindowLongPtrW(hwnd: isize, index: i32) -> isize;
    fn SetWindowLongPtrW(hwnd: isize, index: i32, value: isize) -> isize;
    fn SetLayeredWindowAttributes(hwnd: isize, key: u32, alpha: u8, flags: u32) -> i32;
}

#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmSetWindowAttribute(hwnd: isize, attribute: u32, value: *const u32, size: u32) -> i32;
}

/// Extended style for the overlay: composited (layered), keyed, click-through,
/// hidden from the taskbar and never activated.
pub fn overlay_style(ex_style: u32) -> u32 {
    ex_style | WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE
}

/// Make the overlay a keyed, click-through window. Failures are logged, never fatal.
pub fn configure_overlay_window(
    window: &winit::window::Window,
    crosshair_rgb: [u8; 3],
) -> Result<(), String> {
    let handle = window
        .window_handle()
        .map_err(|err| format!("no window handle: {err}"))?;
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return Err("unexpected window handle kind".to_owned());
    };
    let hwnd = win32.hwnd.get();

    // Safety: `hwnd` belongs to a live winit window on this thread; the calls
    // only read/write that window's styles and DWM attributes.
    unsafe {
        let current = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let wanted = overlay_style(current);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, wanted as isize);

        // The key color becomes fully transparent; alpha is unused with a key.
        let [r, g, b] = crate::platform::overlay_key_rgb(crosshair_rgb);
        if SetLayeredWindowAttributes(hwnd, colorref(r, g, b), 0, LWA_COLORKEY) == 0 {
            return Err("SetLayeredWindowAttributes(LWA_COLORKEY) failed".to_owned());
        }

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

        let applied = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        if applied & WS_EX_LAYERED == 0 || applied & WS_EX_TRANSPARENT == 0 {
            return Err(format!("overlay styles not applied: {applied:#010x}"));
        }
    }
    log_line("vibehair: overlay window configured (layered color key, click-through)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_style_is_layered_click_through_and_hidden() {
        let style = overlay_style(0);
        assert_ne!(
            style & WS_EX_LAYERED,
            0,
            "must be layered for the color key"
        );
        assert_ne!(style & WS_EX_TRANSPARENT, 0, "must be click-through");
        assert_ne!(style & WS_EX_TOOLWINDOW, 0, "must stay out of the taskbar");
        assert_ne!(style & WS_EX_NOACTIVATE, 0, "must not steal focus");
    }

    #[test]
    fn colorref_uses_bbggrr_order() {
        assert_eq!(colorref(0, 0, 0), 0x0000_0000);
        assert_eq!(colorref(255, 0, 255), 0x00FF_00FF);
        assert_eq!(colorref(0x12, 0x34, 0x56), 0x0056_3412);
    }

    #[test]
    fn overlay_style_keeps_unrelated_bits() {
        let existing = 0x0000_0008; // WS_EX_TOPMOST
        assert_eq!(overlay_style(existing) & 0x0000_0008, 0x0000_0008);
    }
}
