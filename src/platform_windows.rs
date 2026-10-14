//! Windows-only window tweaks.
//!
//! We cannot use winit's `mouse_passthrough` here: it adds `WS_EX_LAYERED`,
//! which makes Windows stop compositing our per-pixel-alpha content, so the
//! overlay window appears empty (emilk/egui#2537, open upstream). Instead we
//! set `WS_EX_TRANSPARENT` ourselves, which is all that click-through needs.

use winit::raw_window_handle::{HasWindowHandle as _, RawWindowHandle};

use crate::platform::{WS_EX_TRANSPARENT, click_through_style, log_line};

const GWL_EXSTYLE: i32 = -20;

#[link(name = "user32")]
unsafe extern "system" {
    fn GetWindowLongPtrW(hwnd: isize, index: i32) -> isize;
    fn SetWindowLongPtrW(hwnd: isize, index: i32, value: isize) -> isize;
}

/// Make `window` click-through without breaking DWM composition.
///
/// Returns an error string when the window handle is unavailable or the style
/// could not be set; callers log it and carry on.
pub fn make_click_through(window: &winit::window::Window) -> Result<(), String> {
    let handle = window
        .window_handle()
        .map_err(|err| format!("no window handle: {err}"))?;
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return Err("unexpected window handle kind".to_owned());
    };
    let hwnd = win32.hwnd.get();

    // Safety: `hwnd` comes from a live winit window on this thread; the two
    // user32 calls only read and write that window's extended style bits.
    unsafe {
        let current = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let wanted = click_through_style(current);
        if current == wanted {
            return Ok(());
        }
        let previous = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, wanted as isize) as u32;
        if previous == 0 && current != 0 {
            return Err(format!(
                "SetWindowLongPtrW(GWL_EXSTYLE) failed for {current:#010x}"
            ));
        }
        let applied = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        if applied & WS_EX_TRANSPARENT == 0 {
            return Err(format!("click-through style not applied: {applied:#010x}"));
        }
    }
    log_line("vibehair: overlay click-through enabled (WS_EX_TRANSPARENT)");
    Ok(())
}
