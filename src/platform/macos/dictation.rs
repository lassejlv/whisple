//! Type a transcript into the editor that had focus before Whisple opened.
//! The target is retained through recording so the HUD can safely take focus.

use std::ffi::c_void;
use std::ptr;
use std::thread;
use std::time::{Duration, Instant};

use cocoa::base::{id, nil, BOOL, YES};
use objc::{class, msg_send, sel, sel_impl};

type Ref = *const c_void;
pub(crate) type Element = Ref;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> u8;
    fn AXIsProcessTrustedWithOptions(options: Ref) -> u8;
    static kAXTrustedCheckOptionPrompt: Ref;
    fn AXUIElementCreateSystemWide() -> Element;
    fn AXUIElementCreateApplication(pid: i32) -> Element;
    fn AXUIElementCopyAttributeValue(element: Element, attribute: Ref, value: *mut Ref) -> i32;
    fn AXUIElementSetAttributeValue(element: Element, attribute: Ref, value: Ref) -> i32;
    fn AXUIElementGetPid(element: Element, pid: *mut i32) -> i32;
    fn CGEventCreateKeyboardEvent(source: Ref, keycode: u16, down: u8) -> Ref;
    fn CGEventKeyboardSetUnicodeString(event: Ref, length: usize, text: *const u16);
    fn CGEventPost(tap: u32, event: Ref);
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFStringCreateWithBytes(
        allocator: Ref,
        bytes: *const u8,
        length: isize,
        encoding: u32,
        external: u8,
    ) -> Ref;
    fn CFStringCompare(a: Ref, b: Ref, options: u32) -> isize;
    fn CFEqual(a: Ref, b: Ref) -> u8;
    fn CFRelease(value: Ref);
    fn CFGetTypeID(value: Ref) -> usize;
    fn CFStringGetTypeID() -> usize;
    static kCFBooleanTrue: Ref;
}

/// A Core Foundation object this code owns and releases.
pub(crate) struct Owned(pub(crate) Ref);

impl Owned {
    fn string(value: &str) -> Option<Self> {
        let reference = unsafe {
            CFStringCreateWithBytes(
                ptr::null(),
                value.as_ptr(),
                value.len() as isize,
                0x0800_0100, // kCFStringEncodingUTF8
                0,
            )
        };
        (!reference.is_null()).then_some(Self(reference))
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) };
    }
}

pub(crate) struct Target {
    element: Owned,
    pid: i32,
}

impl Target {
    pub(crate) fn focused() -> Option<Self> {
        if unsafe { AXIsProcessTrusted() } == 0 {
            return None;
        }
        let system = Owned(unsafe { AXUIElementCreateSystemWide() });
        if system.0.is_null() {
            return None;
        }
        let element = attribute(system.0, "AXFocusedUIElement")?;
        let mut pid = 0;
        if unsafe { AXUIElementGetPid(element.0, &mut pid) } != 0
            || pid == std::process::id() as i32
        {
            return None;
        }
        let role = attribute(element.0, "AXRole")?;
        if !["AXTextField", "AXTextArea", "AXComboBox", "AXSearchField"]
            .iter()
            .any(|name| {
                Owned::string(name)
                    .is_some_and(|name| unsafe { CFStringCompare(role.0, name.0, 0) } == 0)
            })
        {
            return None;
        }
        Some(Self { element, pid })
    }

    pub(crate) fn insert(self, text: &str) -> Result<(), String> {
        if text.is_empty() {
            return Ok(());
        }
        if unsafe { AXIsProcessTrusted() } == 0 {
            return Err("Enable Accessibility access to type into other apps.".into());
        }
        let mut pid = 0;
        if unsafe { AXUIElementGetPid(self.element.0, &mut pid) } != 0 || pid != self.pid {
            return Err("The original text field is no longer available.".into());
        }
        unsafe {
            let app: id = msg_send![class!(NSRunningApplication), runningApplicationWithProcessIdentifier: self.pid];
            if app == nil {
                return Err("The original app is no longer open.".into());
            }
            let activated: BOOL = msg_send![app, activateWithOptions: 2usize];
            if activated != YES {
                return Err("Could not switch to the original app.".into());
            }
        }

        // Activation is asynchronous. Never send keystrokes unless the editor's
        // process is actually frontmost, or they could land in another app.
        let deadline = Instant::now() + Duration::from_millis(500);
        while frontmost_pid() != self.pid {
            if Instant::now() >= deadline {
                return Err("Could not focus the original app.".into());
            }
            thread::sleep(Duration::from_millis(10));
        }
        let focused = Owned::string("AXFocused").expect("static accessibility name");
        while !self.is_focused() {
            unsafe { AXUIElementSetAttributeValue(self.element.0, focused.0, kCFBooleanTrue) };
            if Instant::now() >= deadline {
                return Err("Could not focus the original text field.".into());
            }
            thread::sleep(Duration::from_millis(10));
        }

        let utf16: Vec<u16> = text.encode_utf16().collect();
        let mut start = 0;
        while start < utf16.len() {
            let mut end = (start + 64).min(utf16.len());
            // Keep a UTF-16 surrogate pair together at the event boundary.
            if end < utf16.len() && (0xD800..=0xDBFF).contains(&utf16[end - 1]) {
                end -= 1;
            }
            let chunk = &utf16[start..end];
            if frontmost_pid() != self.pid || !self.is_focused() {
                return Err("Typing stopped because the active app changed.".into());
            }
            unsafe {
                let down = Owned(CGEventCreateKeyboardEvent(ptr::null(), 0, 1));
                let up = Owned(CGEventCreateKeyboardEvent(ptr::null(), 0, 0));
                if down.0.is_null() || up.0.is_null() {
                    return Err("Could not create a typing event.".into());
                }
                CGEventKeyboardSetUnicodeString(down.0, chunk.len(), chunk.as_ptr());
                CGEventKeyboardSetUnicodeString(up.0, chunk.len(), chunk.as_ptr());
                CGEventPost(0, down.0); // kCGHIDEventTap
                CGEventPost(0, up.0);
            }
            start = end;
        }
        // Event posting queues the text in the target process. Let it handle
        // the final keystroke before the caller updates the clipboard.
        thread::sleep(Duration::from_millis(50));
        Ok(())
    }

    fn is_focused(&self) -> bool {
        let system = Owned(unsafe { AXUIElementCreateSystemWide() });
        if system.0.is_null() {
            return false;
        }
        attribute(system.0, "AXFocusedUIElement")
            .is_some_and(|focused| unsafe { CFEqual(focused.0, self.element.0) != 0 })
    }
}

pub(crate) fn request_access() {
    if unsafe { AXIsProcessTrusted() } != 0 {
        return;
    }
    unsafe {
        let key = kAXTrustedCheckOptionPrompt as id;
        let value: id = msg_send![class!(NSNumber), numberWithBool: YES];
        let options: id = msg_send![class!(NSDictionary), dictionaryWithObject: value forKey: key];
        AXIsProcessTrustedWithOptions(options as Ref);
    }
}

pub(crate) fn is_trusted() -> bool {
    unsafe { AXIsProcessTrusted() != 0 }
}

/// The accessibility element for a running app.
pub(crate) fn application(pid: i32) -> Option<Owned> {
    let element = unsafe { AXUIElementCreateApplication(pid) };
    (!element.is_null()).then_some(Owned(element))
}

/// A string attribute's value. Other types, such as a missing title
/// reported as a number, read as `None`.
pub(crate) fn string_value(value: &Owned) -> Option<String> {
    unsafe {
        if CFGetTypeID(value.0) != CFStringGetTypeID() {
            return None;
        }
        // CFString and NSString are toll-free bridged.
        let bytes: *const std::os::raw::c_char = msg_send![value.0 as id, UTF8String];
        (!bytes.is_null()).then(|| {
            std::ffi::CStr::from_ptr(bytes)
                .to_string_lossy()
                .into_owned()
        })
    }
}

pub(crate) fn attribute(element: Element, name: &str) -> Option<Owned> {
    let name = Owned::string(name)?;
    let mut value = ptr::null();
    let status = unsafe { AXUIElementCopyAttributeValue(element, name.0, &mut value) };
    (status == 0 && !value.is_null()).then_some(Owned(value))
}

fn frontmost_pid() -> i32 {
    unsafe {
        let workspace: id = msg_send![class!(NSWorkspace), sharedWorkspace];
        let app: id = msg_send![workspace, frontmostApplication];
        if app == nil {
            return 0;
        }
        msg_send![app, processIdentifier]
    }
}
