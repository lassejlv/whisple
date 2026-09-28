//! A single in-memory screen capture. ScreenCaptureKit scales before delivery;
//! older systems use CoreGraphics without launching tools or writing files.
use std::ffi::{c_void, CString};
use std::sync::{mpsc, OnceLock};
use std::time::Duration;

use block::ConcreteBlock;
use cocoa::base::{id, nil, NO, YES};
use cocoa::foundation::{NSPoint, NSRect, NSSize};
use objc::{class, msg_send, sel, sel_impl};

type Ref = *const c_void;
const LIMIT: usize = 1440;

extern "C" {
    fn dlopen(path: *const i8, flags: i32) -> *mut c_void;
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGMainDisplayID() -> u32;
    fn CGDisplayPixelsWide(display: u32) -> usize;
    fn CGDisplayPixelsHigh(display: u32) -> usize;
    fn CGDisplayCreateImage(display: u32) -> Ref;
    fn CGImageRelease(image: Ref);
    fn CGColorSpaceCreateDeviceRGB() -> Ref;
    fn CGColorSpaceRelease(space: Ref);
    fn CGBitmapContextCreate(
        data: *mut c_void,
        width: usize,
        height: usize,
        bits: usize,
        stride: usize,
        space: Ref,
        flags: u32,
    ) -> Ref;
    fn CGContextSetInterpolationQuality(context: Ref, quality: i32);
    fn CGContextDrawImage(context: Ref, rect: NSRect, image: Ref);
    fn CGContextRelease(context: Ref);
}

struct Obj(id);
impl Drop for Obj {
    fn drop(&mut self) {
        unsafe {
            let _: () = msg_send![self.0, release];
        }
    }
}

fn dimensions(width: usize, height: usize) -> (usize, usize) {
    let longest = width.max(height).max(1);
    if longest <= LIMIT {
        return (width.max(1), height.max(1));
    }
    (
        (width * LIMIT / longest).max(1),
        (height * LIMIT / longest).max(1),
    )
}

fn permission_error() -> String {
    "Could not capture the screen. Allow Whisple under Privacy & Security › Screen Recording."
        .into()
}

/// Runs on the assistant worker, never on the UI thread. Only owned PNG bytes
/// cross the callback boundary; image and Objective-C references stay inside it.
pub(crate) fn capture_png() -> Result<Vec<u8>, String> {
    if !unsafe { CGPreflightScreenCaptureAccess() } {
        return Err(permission_error());
    }
    let display_id = unsafe { CGMainDisplayID() };
    let (width, height) = dimensions(unsafe { CGDisplayPixelsWide(display_id) }, unsafe {
        CGDisplayPixelsHigh(display_id)
    });
    static LOADED: OnceLock<bool> = OnceLock::new();
    LOADED.get_or_init(|| {
        let path =
            CString::new("/System/Library/Frameworks/ScreenCaptureKit.framework/ScreenCaptureKit")
                .unwrap();
        // Keep the framework loaded for the lifetime of its registered classes.
        !unsafe { dlopen(path.as_ptr(), 1) }.is_null()
    });
    let Some(manager) = objc::runtime::Class::get("SCScreenshotManager") else {
        let image = unsafe { CGDisplayCreateImage(display_id) };
        if image.is_null() {
            return Err(permission_error());
        }
        let result = encode(image, width, height);
        unsafe { CGImageRelease(image) };
        return result;
    };
    let (sender, receiver) = mpsc::sync_channel(1);
    let available = ConcreteBlock::new(move |content: id, error: id| {
        let _pool = Obj(unsafe { msg_send![class!(NSAutoreleasePool), new] });
        if content == nil || error != nil {
            let _ = sender.send(Err(permission_error()));
            return;
        }
        unsafe {
            let displays: id = msg_send![content, displays];
            let count: usize = msg_send![displays, count];
            let display = (0..count).find_map(|index| {
                let display: id = msg_send![displays, objectAtIndex: index];
                let found: u32 = msg_send![display, displayID];
                (found == display_id).then_some(display)
            });
            let Some(display) = display else {
                let _ = sender.send(Err("Could not find the main display.".into()));
                return;
            };
            let empty: id = msg_send![class!(NSArray), array];
            let filter: id = msg_send![class!(SCContentFilter), alloc];
            let filter = Obj(msg_send![filter, initWithDisplay: display excludingWindows: empty]);
            let config = Obj(msg_send![class!(SCStreamConfiguration), new]);
            let _: () = msg_send![config.0, setWidth: width];
            let _: () = msg_send![config.0, setHeight: height];
            let _: () = msg_send![config.0, setShowsCursor: NO];
            let _: () = msg_send![config.0, setScalesToFit: YES];
            let filter_ref = filter.0;
            let config_ref = config.0;
            let sender = sender.clone();
            let captured = ConcreteBlock::new(move |image: Ref, error: id| {
                let _keep_alive = (&filter, &config);
                let result = if image.is_null() || error != nil { Err(permission_error()) } else { encode(image, width, height) };
                let _ = sender.send(result);
            }).copy();
            let _: () = msg_send![manager, captureImageWithFilter: filter_ref configuration: config_ref completionHandler: &*captured];
        }
    }).copy();
    unsafe {
        let _: () = msg_send![class!(SCShareableContent), getShareableContentExcludingDesktopWindows: NO onScreenWindowsOnly: YES completionHandler: &*available];
    }
    receiver
        .recv_timeout(Duration::from_secs(10))
        .map_err(|_| "Screen capture timed out.".to_string())?
}

fn encode(image: Ref, width: usize, height: usize) -> Result<Vec<u8>, String> {
    let mut rgba = vec![0u8; width * height * 4];
    unsafe {
        let space = CGColorSpaceCreateDeviceRGB();
        if space.is_null() {
            return Err("Could not prepare the screenshot colors.".into());
        }
        let context = CGBitmapContextCreate(
            rgba.as_mut_ptr().cast(),
            width,
            height,
            8,
            width * 4,
            space,
            1 | (4 << 12),
        );
        CGColorSpaceRelease(space);
        if context.is_null() {
            return Err("Could not prepare the screenshot pixels.".into());
        }
        CGContextSetInterpolationQuality(context, 3);
        CGContextDrawImage(
            context,
            NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(width as f64, height as f64),
            ),
            image,
        );
        CGContextRelease(context);
    }
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width as u32, height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        let mut writer = encoder.write_header().map_err(|err| err.to_string())?;
        writer
            .write_image_data(&rgba)
            .map_err(|err| err.to_string())?;
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_fits_portrait_and_landscape_without_upscaling() {
        assert_eq!(dimensions(3840, 2160), (1440, 810));
        assert_eq!(dimensions(2160, 3840), (810, 1440));
        assert_eq!(dimensions(800, 600), (800, 600));
    }
}
