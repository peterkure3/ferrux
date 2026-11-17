//! Unicode text exchange with the system clipboard.
#[cfg(windows)]
mod platform {
    use std::{io, ptr, thread, time::Duration};
    use windows_sys::Win32::{
        Foundation::{GlobalFree, HWND},
        System::{
            DataExchange::{
                CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
                OpenClipboard, SetClipboardData,
            },
            Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE},
        },
        UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, HWND_MESSAGE},
    };

    // CF_UNICODETEXT; avoid enabling the unrelated OLE APIs for this constant.
    const UNICODE_TEXT: u32 = 13;

    struct Clipboard(HWND);

    impl Clipboard {
        fn open() -> io::Result<Self> {
            // A message-only STATIC window provides a valid clipboard owner.
            // It is created, used and destroyed on this same thread.
            let window = unsafe {
                CreateWindowExW(
                    0,
                    windows_sys::core::w!("STATIC"),
                    ptr::null(),
                    0,
                    0,
                    0,
                    0,
                    0,
                    HWND_MESSAGE,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null(),
                )
            };
            if window.is_null() {
                return Err(io::Error::last_os_error());
            }
            for _ in 0..10 {
                if unsafe { OpenClipboard(window) } != 0 {
                    return Ok(Self(window));
                }
                thread::sleep(Duration::from_millis(5));
            }
            let error = io::Error::last_os_error();
            unsafe {
                DestroyWindow(window);
            }
            Err(error)
        }
    }

    impl Drop for Clipboard {
        fn drop(&mut self) {
            unsafe {
                CloseClipboard();
                DestroyWindow(self.0);
            }
        }
    }

    pub fn read_text() -> io::Result<String> {
        let _clipboard = Clipboard::open()?;
        unsafe {
            if IsClipboardFormatAvailable(UNICODE_TEXT) == 0 {
                return Ok(String::new());
            }
            let handle = GetClipboardData(UNICODE_TEXT);
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            let data = GlobalLock(handle).cast::<u16>();
            if data.is_null() {
                return Err(io::Error::last_os_error());
            }
            // The clipboard owns this handle. Bound the read by its allocation.
            let units = std::slice::from_raw_parts(data, GlobalSize(handle) / 2);
            let end = units
                .iter()
                .position(|&unit| unit == 0)
                .unwrap_or(units.len());
            let text = String::from_utf16_lossy(&units[..end]);
            GlobalUnlock(handle);
            Ok(text)
        }
    }

    pub fn write_text(text: &str) -> io::Result<()> {
        let units: Vec<u16> = text
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .replace('\n', "\r\n")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let _clipboard = Clipboard::open()?;
        unsafe {
            let handle = GlobalAlloc(GMEM_MOVEABLE, units.len() * 2);
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            let data = GlobalLock(handle).cast::<u16>();
            if data.is_null() {
                let error = io::Error::last_os_error();
                GlobalFree(handle);
                return Err(error);
            }
            ptr::copy_nonoverlapping(units.as_ptr(), data, units.len());
            GlobalUnlock(handle);
            if EmptyClipboard() == 0 || SetClipboardData(UNICODE_TEXT, handle).is_null() {
                let error = io::Error::last_os_error();
                GlobalFree(handle);
                return Err(error);
            }
            // SetClipboardData succeeded: Windows now owns the allocation.
            Ok(())
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use std::io;
    pub fn read_text() -> io::Result<String> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Windows clipboard required",
        ))
    }
    pub fn write_text(_: &str) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Windows clipboard required",
        ))
    }
}

pub use platform::{read_text, write_text};

/// Match terminal paste semantics and child applications' bracketed-paste mode.
pub fn paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let text: String = text
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect();
    if text.is_empty() {
        return Vec::new();
    }
    let text = text.replace('\n', "\r");
    if bracketed {
        format!("\x1b[200~{text}\x1b[201~").into_bytes()
    } else {
        text.into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paste_normalizes_newlines_and_preserves_unicode_without_adding_enter() {
        assert_eq!(
            paste_bytes("a\r\n界e\u{301}\nb\rc\t", false),
            "a\r界e\u{301}\rb\rc\t".as_bytes()
        );
        assert_eq!(paste_bytes("command", false), b"command");
        assert!(paste_bytes("", true).is_empty());
    }

    #[test]
    fn bracketed_paste_cannot_be_terminated_by_clipboard_escape_codes() {
        assert_eq!(
            paste_bytes("hello\x1b[201~\0\x03world", true),
            b"\x1b[200~hello[201~world\x1b[201~"
        );
    }
}
