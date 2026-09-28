//! Temporarily use the pasteboard for editor-compatible insertion.

use cocoa::base::{id, nil, BOOL, YES};
use objc::{class, msg_send, sel, sel_impl};

use super::dictation::Owned;
use crate::i18n::t;

pub(super) struct TemporaryText {
    board: id,
    items: Vec<id>,
    change_count: isize,
}

impl TemporaryText {
    pub(super) fn new(text: &str) -> Result<Self, String> {
        unsafe {
            let board: id = msg_send![class!(NSPasteboard), generalPasteboard];
            Self::on_board(board, text)
        }
    }

    unsafe fn on_board(board: id, text: &str) -> Result<Self, String> {
        let previous: id = msg_send![board, pasteboardItems];
        let count: usize = msg_send![previous, count];
        let mut snapshot = Self {
            board,
            items: Vec::new(),
            change_count: -1,
        };
        for index in 0..count {
            let item: id = msg_send![previous, objectAtIndex: index];
            let copy: id = msg_send![class!(NSPasteboardItem), new];
            snapshot.items.push(copy);
            let types: id = msg_send![item, types];
            let type_count: usize = msg_send![types, count];
            for type_index in 0..type_count {
                let kind: id = msg_send![types, objectAtIndex: type_index];
                let data: id = msg_send![item, dataForType: kind];
                if data == nil {
                    continue;
                }
                let copied: BOOL = msg_send![copy, setData: data forType: kind];
                if copied != YES {
                    return Err(t("Could not preserve the clipboard before typing.").into());
                }
            }
        }
        let text = Owned::string(text)
            .ok_or_else(|| t("Could not prepare text for typing.").to_string())?;
        let kind = Owned::string("public.utf8-plain-text").expect("static pasteboard type");
        snapshot.change_count = msg_send![board, clearContents];
        let written: BOOL = msg_send![board, setString: text.0 as id forType: kind.0 as id];
        snapshot.change_count = msg_send![board, changeCount];
        if written != YES {
            return Err(t("Could not prepare the clipboard for typing.").into());
        }
        Ok(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    struct TestBoard {
        board: id,
        pool: id,
    }

    impl TestBoard {
        fn new() -> Self {
            unsafe {
                let pool: id = msg_send![class!(NSAutoreleasePool), new];
                let board: id = msg_send![class!(NSPasteboard), pasteboardWithUniqueName];
                Self { board, pool }
            }
        }

        fn write(&self, kind: &str, value: &str) {
            unsafe {
                let kind = Owned::string(kind).unwrap();
                let value = Owned::string(value).unwrap();
                let ok: BOOL =
                    msg_send![self.board, setString: value.0 as id forType: kind.0 as id];
                assert_eq!(ok, YES);
            }
        }

        fn read(&self, kind: &str) -> Option<String> {
            unsafe {
                let kind = Owned::string(kind).unwrap();
                let value: id = msg_send![self.board, stringForType: kind.0 as id];
                if value == nil {
                    return None;
                }
                let bytes: *const std::os::raw::c_char = msg_send![value, UTF8String];
                Some(CStr::from_ptr(bytes).to_string_lossy().into_owned())
            }
        }
    }

    impl Drop for TestBoard {
        fn drop(&mut self) {
            unsafe {
                let _: () = msg_send![self.board, releaseGlobally];
                let _: () = msg_send![self.pool, drain];
            }
        }
    }

    #[test]
    fn inserting_unicode_restores_all_previous_clipboard_formats() {
        let board = TestBoard::new();
        board.write("public.utf8-plain-text", "original");
        board.write("public.html", "<b>original</b>");
        let temporary = unsafe { TemporaryText::on_board(board.board, "Hej æøå 👋") }.unwrap();
        assert_eq!(
            board.read("public.utf8-plain-text").as_deref(),
            Some("Hej æøå 👋")
        );
        drop(temporary);
        assert_eq!(
            board.read("public.utf8-plain-text").as_deref(),
            Some("original")
        );
        assert_eq!(
            board.read("public.html").as_deref(),
            Some("<b>original</b>")
        );
    }

    #[test]
    fn a_new_copy_during_insertion_is_never_overwritten() {
        let board = TestBoard::new();
        board.write("public.utf8-plain-text", "original");
        let temporary = unsafe { TemporaryText::on_board(board.board, "dictation") }.unwrap();
        unsafe {
            let _: isize = msg_send![board.board, clearContents];
        }
        board.write("public.utf8-plain-text", "new user copy");
        drop(temporary);
        assert_eq!(
            board.read("public.utf8-plain-text").as_deref(),
            Some("new user copy")
        );
    }

    #[test]
    fn an_empty_clipboard_is_empty_after_insertion() {
        let board = TestBoard::new();
        let temporary = unsafe { TemporaryText::on_board(board.board, "dictation") }.unwrap();
        drop(temporary);
        assert_eq!(board.read("public.utf8-plain-text"), None);
    }
}

impl Drop for TemporaryText {
    fn drop(&mut self) {
        unsafe {
            let current: isize = msg_send![self.board, changeCount];
            // A copy made by the user or another app during insertion wins.
            if current == self.change_count {
                let _: isize = msg_send![self.board, clearContents];
                if !self.items.is_empty() {
                    let items: id = msg_send![class!(NSArray), arrayWithObjects: self.items.as_ptr() count: self.items.len()];
                    let _: BOOL = msg_send![self.board, writeObjects: items];
                }
            }
            for item in self.items.drain(..) {
                let _: () = msg_send![item, release];
            }
        }
    }
}
