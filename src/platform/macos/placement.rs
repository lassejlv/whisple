use cocoa::appkit::{
    NSApplication, NSColor, NSView, NSViewLayerContentsPlacement, NSWindow, NSWindowStyleMask,
};
use cocoa::base::{id, nil, NO};
use cocoa::foundation::{NSPoint, NSRect, NSSize};
use objc::{class, msg_send, sel, sel_impl};

const MARGIN: f64 = 18.0;
/// `NSWindowAnimationBehaviorNone`. Popups default to utility-window
/// animation, which eases the frame and leaves the panel clipped.
const ANIMATION_NONE: isize = 2;

pub fn set_mapped(mapped: bool) {
    unsafe {
        let window = hud_window();
        if window.is_null() {
            return;
        }
        if mapped {
            let _: () = msg_send![window, orderFront: nil];
        } else {
            let _: () = msg_send![window, orderOut: nil];
        }
    }
}

pub fn anchor(width: f32, height: f32) {
    unsafe {
        let window = hud_window();
        if window.is_null() || window.isVisible() == NO {
            return;
        }
        let view = metal_view(window);
        // GPUI creates a titled window even without a titlebar, with a
        // faint background to support AppKit's shadow. That native frame
        // outlines the empty space while our HUD closes inside it. Whisp
        // paints its own rounded border, so the host must be truly clear
        // and borderless. Apply this once, before anchoring the content.
        let style = window.styleMask();
        if style.contains(NSWindowStyleMask::NSTitledWindowMask) {
            let was_key = window.isKeyWindow() != NO;
            let responder = window.firstResponder();
            window.setStyleMask_(style & !NSWindowStyleMask::NSTitledWindowMask);
            window.setHasShadow_(NO);
            window.setBackgroundColor_(NSColor::clearColor(nil));
            // AppKit resets keyboard focus when its style changes.
            if was_key {
                window.makeKeyWindow();
            }
            if !responder.is_null() {
                window.makeFirstResponder_(responder);
            }
        }
        let _: () = msg_send![window, setAnimationBehavior: ANIMATION_NONE];
        anchor_drawable(window, view);

        let screen: id = msg_send![window, screen];
        if screen.is_null() {
            return;
        }
        let visible: NSRect = msg_send![screen, visibleFrame];
        let content = NSRect {
            origin: NSPoint {
                x: visible.origin.x + (visible.size.width - f64::from(width)) * 0.5,
                y: visible.origin.y + MARGIN,
            },
            size: NSSize {
                width: f64::from(width),
                height: f64::from(height),
            },
        };
        let frame: NSRect = msg_send![window, frameRectForContentRect: content];
        let current: NSRect = msg_send![window, frame];
        if close(current.origin.x, frame.origin.x)
            && close(current.origin.y, frame.origin.y)
            && close(current.size.width, frame.size.width)
            && close(current.size.height, frame.size.height)
        {
            return;
        }
        // `display: NO` avoids a synchronous AppKit redraw while GPUI is
        // already drawing this frame. The view's resize callback and
        // `bounds_changed` pick up the new content size.
        let _: () = msg_send![window, setFrame: frame display: NO animate: NO];
    }
}

unsafe fn hud_window() -> id {
    let app = NSApplication::sharedApplication(nil);
    let windows: id = msg_send![app, windows];
    let count: usize = msg_send![windows, count];
    for index in 0..count {
        let window: id = msg_send![windows, objectAtIndex: index];
        let view = metal_view(window);
        if !view.is_null() && is_hud_width(NSView::frame(window.contentView()).size.width) {
            return window;
        }
    }
    nil
}

unsafe fn metal_view(window: id) -> id {
    let views: id = msg_send![window.contentView(), subviews];
    let count: usize = msg_send![views, count];
    for index in 0..count {
        let view: id = msg_send![views, objectAtIndex: index];
        let layer = view.layer();
        if layer.is_null() {
            continue;
        }
        let is_metal: bool = msg_send![layer, isKindOfClass: class!(CAMetalLayer)];
        if is_metal {
            return view;
        }
    }
    nil
}

unsafe fn anchor_drawable(window: id, view: id) {
    // Keep the last presented frame at its original size and pinned
    // to the bar while Metal prepares a drawable for the new bounds.
    // AppKit's default stretches it, producing a one-frame flash.
    let placement = NSViewLayerContentsPlacement::NSViewLayerContentsPlacementBottom;
    if view.layerContentsPlacement() != placement {
        view.setLayerContentsPlacement(placement);
    }
    let layer = view.layer();
    let scale = window.backingScaleFactor();
    let current_scale: f64 = msg_send![layer, contentsScale];
    if (current_scale - scale).abs() > f64::EPSILON {
        let _: () = msg_send![layer, setContentsScale: scale];
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.5
}

fn is_hud_width(width: f64) -> bool {
    close(width, f64::from(crate::app::WINDOW_WIDTH))
}

#[cfg(test)]
mod tests {
    use super::is_hud_width;

    #[test]
    fn settings_window_is_not_mistaken_for_voice_bar() {
        assert!(is_hud_width(f64::from(crate::app::WINDOW_WIDTH)));
        assert!(!is_hud_width(880.0));
    }
}
