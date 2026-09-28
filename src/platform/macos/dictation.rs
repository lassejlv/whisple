//! Type a transcript into the editor that had focus before Whisple opened.
//! The target is retained through recording so the HUD can safely take focus.

use std::ffi::c_void;
use std::ptr;
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
    fn CGEventSetFlags(event: Ref, flags: u64);
    fn CGPreflightPostEventAccess() -> bool;
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
    fn CFEqual(a: Ref, b: Ref) -> u8;
    fn CFRelease(value: Ref);
    fn CFGetTypeID(value: Ref) -> usize;
    fn CFStringGetTypeID() -> usize;
    static kCFBooleanTrue: Ref;
}

/// A Core Foundation object this code owns and releases.
pub(crate) struct Owned(pub(crate) Ref);

impl Owned {
    pub(super) fn string(value: &str) -> Option<Self> {
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
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) };
        }
    }
}

pub(crate) struct Target {
    element: Option<Owned>,
    pid: i32,
}

impl Target {
    pub(crate) fn focused() -> Option<Self> {
        let pid = frontmost_pid();
        let focused = is_trusted().then(focused_element).flatten();
        let (pid, element) = capture_destination(pid, std::process::id() as i32, focused)?;
        Some(Self { element, pid })
    }

    pub(crate) async fn insert(
        self,
        text: &str,
        executor: &gpui_kit::BackgroundExecutor,
    ) -> Result<(), String> {
        if text.is_empty() {
            return Ok(());
        }
        if !unsafe { CGPreflightPostEventAccess() } {
            return Err("Enable Accessibility access to type into other apps.".into());
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
            executor.timer(Duration::from_millis(10)).await;
        }
        if let Some(element) = &self.element {
            let focused = Owned::string("AXFocused").expect("static accessibility name");
            while !self.is_focused() {
                let status =
                    unsafe { AXUIElementSetAttributeValue(element.0, focused.0, kCFBooleanTrue) };
                // Web composers may replace their AX node while recording.
                // If the retained node is gone, use this app's current input.
                if status != 0 || Instant::now() >= deadline {
                    break;
                }
                executor.timer(Duration::from_millis(10)).await;
            }
        }

        // Paste uses the editor's normal input handling, including rich-text
        // composers that ignore synthetic Unicode keystrokes.
        let clipboard = super::clipboard::TemporaryText::new(text)?;
        if frontmost_pid() != self.pid {
            return Err("Typing stopped because the active app changed.".into());
        }
        unsafe {
            let down = Owned(CGEventCreateKeyboardEvent(ptr::null(), 9, 1)); // V
            let up = Owned(CGEventCreateKeyboardEvent(ptr::null(), 9, 0));
            if down.0.is_null() || up.0.is_null() {
                return Err("Could not create a typing event.".into());
            }
            CGEventSetFlags(down.0, 1 << 20); // Command
            CGEventSetFlags(up.0, 1 << 20);
            CGEventPost(0, down.0);
            CGEventPost(0, up.0);
        }
        // The receiving process reads the pasteboard asynchronously.
        executor.timer(Duration::from_millis(200)).await;
        drop(clipboard);
        Ok(())
    }

    fn is_focused(&self) -> bool {
        self.element.as_ref().is_some_and(|element| {
            focused_element().is_some_and(|(pid, focused)| {
                pid == self.pid && unsafe { CFEqual(focused.0, element.0) != 0 }
            })
        })
    }
}

// Missing AX text roles do not mean an app has no editor. Browser composers
// and custom editors may expose generic roles or no AX node at all.
fn capture_destination<T>(
    frontmost: i32,
    own_pid: i32,
    focused: Option<(i32, T)>,
) -> Option<(i32, Option<T>)> {
    if frontmost <= 0 || frontmost == own_pid {
        return None;
    }
    Some((
        frontmost,
        focused
            .filter(|(pid, _)| *pid == frontmost)
            .map(|(_, element)| element),
    ))
}

fn focused_element() -> Option<(i32, Owned)> {
    let system = Owned(unsafe { AXUIElementCreateSystemWide() });
    if system.0.is_null() {
        return None;
    }
    let element = attribute(system.0, "AXFocusedUIElement")?;
    let mut pid = 0;
    (unsafe { AXUIElementGetPid(element.0, &mut pid) } == 0).then_some((pid, element))
}

/// Only call from an explicit permission action, never from recording or
/// focus capture. macOS may keep reporting an old grant as untrusted.
pub(crate) fn request_access_from_settings() {
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

#[cfg(test)]
mod tests {
    use super::capture_destination;

    #[test]
    fn an_editor_without_an_accessibility_text_node_keeps_its_app() {
        assert_eq!(capture_destination::<()>(42, 7, None), Some((42, None)));
    }

    #[test]
    fn rich_composers_are_not_rejected_by_accessibility_role() {
        assert_eq!(
            capture_destination(42, 7, Some((42, "AXGroup"))),
            Some((42, Some("AXGroup")))
        );
    }

    #[test]
    fn a_stale_accessibility_node_cannot_redirect_the_destination() {
        assert_eq!(
            capture_destination(42, 7, Some((21, "old editor"))),
            Some((42, None))
        );
    }

    #[test]
    fn whisple_and_missing_apps_are_never_dictation_destinations() {
        assert_eq!(capture_destination(7, 7, Some((42, "editor"))), None);
        assert_eq!(capture_destination::<()>(0, 7, None), None);
    }
}
