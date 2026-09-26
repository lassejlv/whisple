//! Types dictation into the text field that had focus when recording began,
//! like the macOS accessibility target. UI Automation finds the field, and the
//! text is pasted: Unicode keystrokes from `SendInput` race in apps that
//! handle input late, which garbles or drops characters. The clipboard is put
//! back afterwards.

use std::thread;
use std::time::{Duration, Instant};

use windows::core::w;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
    GetClipboardSequenceNumber, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};
use windows::Win32::UI::Accessibility::{
    IUIAutomation, IUIAutomationElement, UIA_ComboBoxControlTypeId, UIA_DocumentControlTypeId,
    UIA_EditControlTypeId,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    VIRTUAL_KEY, VK_CONTROL, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, GetForegroundWindow, GetWindowThreadProcessId, IsWindow,
    SetForegroundWindow, HWND_MESSAGE, WINDOW_EX_STYLE, WINDOW_STYLE,
};

const CF_UNICODETEXT: u32 = 13;
/// How long the target app gets to read the pasted text before the previous
/// clipboard comes back.
const RESTORE_AFTER: Duration = Duration::from_millis(1500);

pub(crate) struct Target {
    automation: IUIAutomation,
    element: IUIAutomationElement,
    window: HWND,
}

impl Target {
    pub(crate) fn focused() -> Option<Self> {
        let window = unsafe { GetForegroundWindow() };
        let pid = process_of(window)?;
        // Asking UI Automation about our own window would wait on this
        // thread, which is the one answering it.
        if pid == std::process::id() {
            return None;
        }
        let automation = super::automation()?;
        let element = unsafe { automation.GetFocusedElement() }.ok()?;
        if unsafe { element.CurrentProcessId() }.ok()? as u32 != pid {
            return None;
        }
        let control = unsafe { element.CurrentControlType() }.ok()?;
        if ![
            UIA_EditControlTypeId,
            UIA_DocumentControlTypeId,
            UIA_ComboBoxControlTypeId,
        ]
        .contains(&control)
        {
            return None;
        }
        Some(Self {
            automation,
            element,
            window,
        })
    }

    pub(crate) fn insert(self, text: &str) -> Result<(), String> {
        if text.is_empty() {
            return Ok(());
        }
        if !unsafe { IsWindow(Some(self.window)) }.as_bool() {
            return Err("The original app is no longer open.".into());
        }
        // Windows only lets the foreground app hand focus on, and switching
        // can take a moment. Never paste until the original window is in
        // front, or the text could land in another app.
        let deadline = Instant::now() + Duration::from_millis(500);
        while unsafe { GetForegroundWindow() } != self.window {
            let _ = unsafe { SetForegroundWindow(self.window) };
            if Instant::now() >= deadline {
                return Err("Could not switch to the original app.".into());
            }
            thread::sleep(Duration::from_millis(10));
        }
        while !self.is_focused() {
            let _ = unsafe { self.element.SetFocus() };
            if Instant::now() >= deadline {
                return Err("Could not focus the original text field.".into());
            }
            thread::sleep(Duration::from_millis(10));
        }

        let previous = Clipboard::open()?.take_text(text)?;
        let sequence = unsafe { GetClipboardSequenceNumber() };
        let inputs = [
            key(VK_CONTROL, false),
            key(VK_V, false),
            key(VK_V, true),
            key(VK_CONTROL, true),
        ];
        let sent = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
        // The app reads the clipboard whenever it handles the paste, so the
        // old contents return later, and only if nothing has replaced the
        // dictation meanwhile, such as the copy-to-clipboard preference.
        thread::spawn(move || {
            thread::sleep(RESTORE_AFTER);
            if unsafe { GetClipboardSequenceNumber() } == sequence {
                if let Ok(clipboard) = Clipboard::open() {
                    clipboard.restore(previous);
                }
            }
        });
        if sent as usize != inputs.len() {
            return Err("Windows blocked pasting into the original app.".into());
        }
        Ok(())
    }

    fn is_focused(&self) -> bool {
        unsafe {
            self.automation
                .GetFocusedElement()
                .and_then(|focused| self.automation.CompareElements(&focused, &self.element))
                .is_ok_and(|same| same.as_bool())
        }
    }
}

/// Windows has no permission prompt for typing into other apps.
pub(crate) fn request_access() {}

/// The clipboard, opened with a hidden window as its owner. Windows refuses
/// new clipboard data from an ownerless clipboard.
struct Clipboard {
    owner: HWND,
}

impl Clipboard {
    fn open() -> Result<Self, String> {
        let owner = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                None,
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
        }
        .map_err(|err| format!("Could not use the clipboard: {err}"))?;
        // Another app may hold the clipboard for a moment.
        let deadline = Instant::now() + Duration::from_millis(250);
        while unsafe { OpenClipboard(Some(owner)) }.is_err() {
            if Instant::now() >= deadline {
                let _ = unsafe { DestroyWindow(owner) };
                return Err("Another app is using the clipboard.".into());
            }
            thread::sleep(Duration::from_millis(10));
        }
        Ok(Self { owner })
    }

    /// Replaces the clipboard with `text`, kept out of clipboard history, and
    /// returns what it held before.
    fn take_text(self, text: &str) -> Result<Vec<(u32, Vec<u8>)>, String> {
        let previous = self.contents();
        unsafe {
            EmptyClipboard().map_err(|err| format!("Could not use the clipboard: {err}"))?;
            let utf16: Vec<u8> = text
                .encode_utf16()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect();
            set(CF_UNICODETEXT, &utf16)?;
            let private =
                RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing"));
            if private != 0 {
                let _ = set(private, &[0]);
            }
        }
        Ok(previous)
    }

    fn restore(self, contents: Vec<(u32, Vec<u8>)>) {
        unsafe {
            if EmptyClipboard().is_err() {
                return;
            }
            for (format, bytes) in contents {
                let _ = set(format, &bytes);
            }
        }
    }

    /// Copies each format stored in global memory. Formats held as GDI or
    /// metafile handles are skipped; Windows derives bitmaps again from the
    /// device-independent copy.
    fn contents(&self) -> Vec<(u32, Vec<u8>)> {
        let mut contents = Vec::new();
        let mut format = 0;
        loop {
            format = unsafe { EnumClipboardFormats(format) };
            if format == 0 {
                break;
            }
            if is_handle_format(format) {
                continue;
            }
            let Ok(handle) = (unsafe { GetClipboardData(format) }) else {
                continue;
            };
            let memory = HGLOBAL(handle.0);
            unsafe {
                let size = GlobalSize(memory);
                let data = GlobalLock(memory);
                if data.is_null() {
                    continue;
                }
                contents.push((
                    format,
                    std::slice::from_raw_parts(data as *const u8, size).to_vec(),
                ));
                let _ = GlobalUnlock(memory);
            }
        }
        contents
    }
}

impl Drop for Clipboard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
            let _ = DestroyWindow(self.owner);
        }
    }
}

/// Clipboard formats whose data is a handle rather than global memory.
fn is_handle_format(format: u32) -> bool {
    matches!(
        format,
        2 | 3 | 9 | 14 // bitmap, metafile picture, palette, enhanced metafile
            | 0x80 | 0x82 | 0x83 | 0x8E // owner display and its display formats
            | 0x300..=0x3FF // private GDI objects
    )
}

/// Hands `bytes` to the clipboard, which then owns the memory.
unsafe fn set(format: u32, bytes: &[u8]) -> Result<(), String> {
    let memory = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1))
        .map_err(|err| format!("Could not use the clipboard: {err}"))?;
    let data = GlobalLock(memory);
    if data.is_null() {
        let _ = GlobalFree(Some(memory));
        return Err("Could not use the clipboard.".into());
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), data as *mut u8, bytes.len());
    let _ = GlobalUnlock(memory);
    if let Err(err) = SetClipboardData(format, Some(HANDLE(memory.0))) {
        let _ = GlobalFree(Some(memory));
        return Err(format!("Could not use the clipboard: {err}"));
    }
    Ok(())
}

fn process_of(window: HWND) -> Option<u32> {
    if window.is_invalid() {
        return None;
    }
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
    (pid != 0).then_some(pid)
}

fn key(code: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: code,
                wScan: 0,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}
