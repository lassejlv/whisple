use std::sync::mpsc;
use std::time::Duration;

use block::ConcreteBlock;
use cocoa::base::{id, nil, BOOL, NO};
use cocoa::foundation::NSString;
use objc::runtime::Class;
use objc::{msg_send, sel, sel_impl};

#[link(name = "AVFoundation", kind = "framework")]
extern "C" {}

fn with_device<T>(f: impl FnOnce(&Class, id) -> T) -> Result<T, String> {
    let class = Class::get("AVCaptureDevice").ok_or("AVFoundation is unavailable")?;
    // AVMediaTypeAudio is the Objective-C string `soun`.
    let media_type = unsafe { NSString::alloc(nil).init_str("soun") };
    let result = f(class, media_type);
    unsafe {
        let _: () = msg_send![media_type, release];
    }
    Ok(result)
}

pub(crate) fn is_allowed() -> bool {
    with_device(|class, media_type| unsafe {
        let status: isize = msg_send![class, authorizationStatusForMediaType: media_type];
        status == 3 // AVAuthorizationStatusAuthorized
    })
    .unwrap_or(false)
}

pub(crate) fn request() -> Result<(), String> {
    with_device(|class, media_type| unsafe {
        let status: isize = msg_send![class, authorizationStatusForMediaType: media_type];
        if status == 3 {
            return Ok(());
        }
        if status == 1 || status == 2 {
            return Err("Access is denied. Enable Whisple in System Settings › Privacy & Security › Microphone, then try again.".into());
        }
        if status != 0 {
            return Err("macOS could not determine microphone permission.".into());
        }
        let (sender, receiver) = mpsc::channel();
        let block = ConcreteBlock::new(move |granted: BOOL| {
            let _ = sender.send(granted != NO);
        })
        .copy();
        let _: () =
            msg_send![class, requestAccessForMediaType: media_type completionHandler: &*block];
        match receiver.recv_timeout(Duration::from_secs(120)) {
            Ok(true) => Ok(()),
            Ok(false) => Err("Access is denied. Enable Whisple in System Settings › Privacy & Security › Microphone, then try again.".into()),
            Err(_) => Err("macOS did not return a microphone permission result. Try again.".into()),
        }
    })?
}
