//! The window in front: which program owns it and where it is.

/// Program path and on-screen rectangle (physical px: left, top, right, bottom)
/// of the foreground window, or `None` if there isn't one we can read.
#[cfg(windows)]
pub fn window() -> Option<(String, [i32; 4])> {
    use windows::Win32::Foundation::{CloseHandle, RECT};
    use windows::Win32::Graphics::Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId, IsIconic,
    };
    use windows::core::PWSTR;

    // SAFETY: read-only Win32 queries; the process handle is closed below.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() || IsIconic(hwnd).as_bool() {
            return None;
        }
        let mut r = RECT::default();
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut r as *mut RECT as *mut _,
            std::mem::size_of::<RECT>() as u32,
        )
        .ok()?;
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(process);
        ok.ok()?;
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        Some((path, [r.left, r.top, r.right, r.bottom]))
    }
}

#[cfg(not(windows))]
pub fn window() -> Option<(String, [i32; 4])> {
    None
}
