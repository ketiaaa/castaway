//! Gives UxPlay's mirror window the Castaway look: title, icon and a dark
//! title bar. UxPlay creates the window itself (class `Gst...Hwnd`), so a
//! background thread finds it and restyles it from outside. Purely cosmetic:
//! every Windows call may fail and the window then just keeps its default look.

use std::collections::HashSet;
use std::ffi::c_void;
use std::path::Path;
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;
use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM};
use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateIconFromResourceEx, EnumWindows, GWL_STYLE, GetClassNameW, GetWindowLongW,
    GetWindowTextW, GetWindowThreadProcessId, ICON_BIG, ICON_SMALL, IsWindowVisible,
    LR_DEFAULTCOLOR, SendMessageW, SetWindowTextW, WM_SETICON,
};

pub const TITLE: &str = "Castaway";

const WS_CAPTION: u32 = 0x00C0_0000;
const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
const DWMWA_CAPTION_COLOR: u32 = 35;
const DWMWA_TEXT_COLOR: u32 = 36;
/// 0x00BBGGRR: the dark tile colour of the logo (#0a1020) and a soft white.
const CAPTION_COLOR: u32 = 0x0020_100A;
const TEXT_COLOR: u32 = 0x00F5_EDE6;

const ICON_LARGE_PNG: &[u8] = include_bytes!("../icons/128x128.png");
const ICON_SMALL_PNG: &[u8] = include_bytes!("../icons/32x32.png");

/// True for UxPlay's visible, framed top-level video window.
pub fn is_mirror_window(class: &str, visible: bool, style: u32) -> bool {
    visible
        && class.starts_with("Gst")
        && class.ends_with("Hwnd")
        && style & WS_CAPTION == WS_CAPTION
}

/// True when `image` (a process image path) is a `uxplay.exe`.
pub fn is_uxplay_image(image: &str) -> bool {
    Path::new(image)
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.eq_ignore_ascii_case("uxplay.exe"))
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn text_of(f: impl Fn(*mut u16, i32) -> i32) -> String {
    let mut buf = [0u16; 256];
    let n = f(buf.as_mut_ptr(), buf.len() as i32);
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn class_name(hwnd: HWND) -> String {
    // SAFETY: buffer pointer and length come from `text_of`.
    text_of(|p, n| unsafe { GetClassNameW(hwnd, p, n) })
}

fn window_title(hwnd: HWND) -> String {
    // SAFETY: as above.
    text_of(|p, n| unsafe { GetWindowTextW(hwnd, p, n) })
}

fn process_image(pid: u32) -> Option<String> {
    // SAFETY: the handle is closed on every path; the buffer outlives the call.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(h);
        (ok != 0).then(|| String::from_utf16_lossy(&buf[..len as usize]))
    }
}

unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> i32 {
    // SAFETY: `lparam` is the `&mut Vec<isize>` passed by `candidates`.
    let out = unsafe { &mut *(lparam as *mut Vec<isize>) };
    let (visible, style) = unsafe { (IsWindowVisible(hwnd) != 0, GetWindowLongW(hwnd, GWL_STYLE) as u32) };
    if is_mirror_window(&class_name(hwnd), visible, style) {
        out.push(hwnd as isize);
    }
    1
}

/// Visible, framed `Gst...Hwnd` windows that belong to a `uxplay.exe`.
fn candidates() -> Vec<isize> {
    let mut found: Vec<isize> = Vec::new();
    // SAFETY: `found` outlives the synchronous EnumWindows call.
    unsafe { EnumWindows(Some(collect), &mut found as *mut Vec<isize> as LPARAM) };
    found.retain(|&h| {
        let mut pid = 0u32;
        // SAFETY: plain query with a valid out pointer.
        unsafe { GetWindowThreadProcessId(h as HWND, &mut pid) };
        process_image(pid).is_some_and(|p| is_uxplay_image(&p))
    });
    found
}

/// HICONs live for the whole process, so they are created once.
fn icons() -> (isize, isize) {
    static ICONS: OnceLock<(isize, isize)> = OnceLock::new();
    *ICONS.get_or_init(|| {
        let make = |png: &[u8], size: i32| -> isize {
            // SAFETY: PNG bytes are valid for the call; Windows copies them.
            unsafe {
                CreateIconFromResourceEx(png.as_ptr(), png.len() as u32, 1, 0x0003_0000, size, size, LR_DEFAULTCOLOR)
                    as isize
            }
        };
        (make(ICON_LARGE_PNG, 128), make(ICON_SMALL_PNG, 32))
    })
}

fn dwm_set(hwnd: HWND, attr: u32, value: u32) {
    // SAFETY: `value` is a 4-byte DWORD that lives across the call. Failure
    // (older Windows without this attribute) is ignored on purpose.
    unsafe {
        DwmSetWindowAttribute(hwnd, attr, &value as *const u32 as *const c_void, 4);
    }
}

fn style_window(hwnd: HWND) {
    let (big, small) = icons();
    // SAFETY: valid window handle from EnumWindows; icons are process-lifetime.
    unsafe {
        if big != 0 {
            SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, big);
        }
        if small != 0 {
            SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, small);
        }
    }
    dwm_set(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, 1);
    dwm_set(hwnd, DWMWA_CAPTION_COLOR, CAPTION_COLOR);
    dwm_set(hwnd, DWMWA_TEXT_COLOR, TEXT_COLOR);
}

/// Start the watcher thread. It runs until the process exits.
pub fn spawn() {
    thread::spawn(|| {
        let mut styled: HashSet<isize> = HashSet::new();
        loop {
            let now = candidates();
            styled.retain(|h| now.contains(h));
            for h in now {
                let hwnd = h as HWND;
                if window_title(hwnd) != TITLE {
                    let t = wide(TITLE);
                    // SAFETY: `t` is NUL-terminated and outlives the call.
                    unsafe { SetWindowTextW(hwnd, t.as_ptr()) };
                }
                if styled.insert(h) {
                    style_window(hwnd);
                }
            }
            thread::sleep(Duration::from_millis(500));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAMED: u32 = 0x16CF_0000; // style seen on a real UxPlay window

    #[test]
    fn recognises_the_d3d_mirror_window() {
        assert!(is_mirror_window("GstD3D12Hwnd", true, FRAMED));
        assert!(is_mirror_window("GstD3D11Hwnd", true, FRAMED));
        assert!(is_mirror_window("GstGLWin32Hwnd", true, FRAMED));
    }

    #[test]
    fn ignores_hidden_borderless_and_unrelated_windows() {
        assert!(!is_mirror_window("GstD3D12Hwnd", false, FRAMED));
        assert!(!is_mirror_window("GstD3D12Hwnd", true, 0x9000_0000)); // popup, no caption
        assert!(!is_mirror_window("IME", true, FRAMED));
        assert!(!is_mirror_window("MSCTFIME UI", true, FRAMED));
        assert!(!is_mirror_window("Chrome_WidgetWin_1", true, FRAMED));
    }

    #[test]
    fn matches_only_uxplay_images() {
        assert!(is_uxplay_image(r"F:\test dir\Castaway\resources\uxplay\uxplay.exe"));
        assert!(is_uxplay_image(r"C:\X\UXPLAY.EXE"));
        assert!(!is_uxplay_image(r"C:\Windows\explorer.exe"));
        assert!(!is_uxplay_image(r"C:\X\uxplay.exe.bak"));
        assert!(!is_uxplay_image(""));
    }
}
