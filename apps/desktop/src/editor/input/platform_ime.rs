//! Layer 2: PlatformImeAdapter — 平台分支点（Rust FFI 回调桥接）
//!
//! Windows: pending key deferred insertion 防首字母泄漏
//! Linux/macOS: 直接插入，不延迟，按 Qt inputMethodEvent 语义
//!
//! C++ PlatformImeAdapter 类定义在 qt_surface.rs 的 cpp! 块中
//! （cpp! 宏将所有 cpp! 块合并到同一编译单元，类定义不能重复）。
//! 此文件只包含 Rust 侧的 FFI 回调函数。

use crate::sujian_editor_item::SujianEditorItem;
use super::controller::*;
use super::events::decode_utf16_ptr;
use std::ffi::c_void;

unsafe fn item_from_ptr<'a>(rust_item: *mut c_void) -> Option<&'a mut SujianEditorItem> {
    if rust_item.is_null() {
        return None;
    }
    Some(unsafe { &mut *(rust_item as *mut SujianEditorItem) })
}

#[no_mangle]
extern "C" fn sujian_handle_key_and_text(
    rust_item: *mut c_void,
    key: i32,
    modifiers: i32,
    text: *const u16,
    text_len: i32,
) -> bool {
    let Some(item) = (unsafe { item_from_ptr(rust_item) }) else {
        return false;
    };
    let text = decode_utf16_ptr(text, text_len);
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        handle_key_and_text(item, key, modifiers, text)
    })) {
        Ok(result) => result,
        Err(_) => {
            eprintln!(
                "[sujian_editor] panic in sujian_handle_key_and_text, caught at FFI boundary"
            );
            false
        }
    }
}

#[no_mangle]
extern "C" fn sujian_ime_commit(rust_item: *mut c_void, text: *const u16, text_len: i32) {
    let Some(item) = (unsafe { item_from_ptr(rust_item) }) else {
        return;
    };
    let text = decode_utf16_ptr(text, text_len);
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ime_commit(item, text);
    }));
}

#[no_mangle]
extern "C" fn sujian_ime_replace_and_commit(
    rust_item: *mut c_void,
    text: *const u16,
    text_len: i32,
    replace_start: i32,
    replace_length: i32,
) {
    let Some(item) = (unsafe { item_from_ptr(rust_item) }) else {
        return;
    };
    let text = decode_utf16_ptr(text, text_len);
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ime_replace_and_commit(item, text, replace_start, replace_length);
    }));
}

#[no_mangle]
extern "C" fn sujian_ime_preedit(
    rust_item: *mut c_void,
    text: *const u16,
    text_len: i32,
    cursor: i32,
) {
    let Some(item) = (unsafe { item_from_ptr(rust_item) }) else {
        return;
    };
    let text = decode_utf16_ptr(text, text_len);
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ime_preedit(item, text, cursor);
    }));
}

/// FFI callback for IME preedit with attributes.
/// attr_types: 0=TextFormat, 1=Cursor, 2=Language, 3=Ruby, 4=Selection
/// attr_starts/attr_lengths: character offsets and lengths for each attribute
/// attr_formats: format code for TextFormat attributes (0=underline, 1=textColor, 2=backgroundColor, 3=fontUnderline)
#[no_mangle]
extern "C" fn sujian_ime_preedit_attrs(
    rust_item: *mut c_void,
    text: *const u16,
    text_len: i32,
    cursor: i32,
    attr_types: *const i32,
    attr_starts: *const i32,
    attr_lengths: *const i32,
    attr_count: i32,
    attr_formats: *const i32,
) {
    let Some(item) = (unsafe { item_from_ptr(rust_item) }) else {
        return;
    };
    let text = decode_utf16_ptr(text, text_len);

    let mut attributes = Vec::new();
    if !attr_types.is_null() && !attr_starts.is_null() && !attr_lengths.is_null() && attr_count > 0 {
        let types_slice = unsafe { std::slice::from_raw_parts(attr_types, attr_count as usize) };
        let starts_slice = unsafe { std::slice::from_raw_parts(attr_starts, attr_count as usize) };
        let lengths_slice = unsafe { std::slice::from_raw_parts(attr_lengths, attr_count as usize) };
        let formats_slice = if !attr_formats.is_null() {
            unsafe { std::slice::from_raw_parts(attr_formats, attr_count as usize) }
        } else {
            &vec![0i32; attr_count as usize]
        };

        let char_offsets: Vec<usize> = {
            let mut offsets = Vec::new();
            let mut byte_pos = 0;
            offsets.push(0);
            for ch in text.chars() {
                byte_pos += ch.len_utf8();
                offsets.push(byte_pos);
            }
            offsets
        };

        for i in 0..attr_count as usize {
            let attr_type = types_slice[i];
            let char_start = starts_slice[i].max(0) as usize;
            let char_length = lengths_slice[i].max(0) as usize;
            let format_code = formats_slice[i];

            let kind = if attr_type == 0 {
                match format_code {
                    1 => crate::sujian_editor_item::PreeditAttributeKind::TextColor {
                        color: String::new(),
                    },
                    2 => crate::sujian_editor_item::PreeditAttributeKind::BackgroundColor {
                        color: String::new(),
                    },
                    3 => crate::sujian_editor_item::PreeditAttributeKind::FontUnderline,
                    _ => crate::sujian_editor_item::PreeditAttributeKind::Underline,
                }
            } else if attr_type == 1 {
                crate::sujian_editor_item::PreeditAttributeKind::Cursor
            } else {
                continue;
            };

            let byte_start = char_offsets.get(char_start).copied().unwrap_or(text.len());
            let char_end = char_start + char_length;
            let byte_end = char_offsets.get(char_end).copied().unwrap_or(text.len());
            let byte_length = byte_end.saturating_sub(byte_start);

            attributes.push(crate::sujian_editor_item::PreeditAttribute {
                start: byte_start,
                length: byte_length,
                kind,
            });
        }
    }

    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ime_preedit_with_attrs(item, text, cursor, attributes);
    }));
}

#[no_mangle]
extern "C" fn sujian_ime_cancel(rust_item: *mut c_void) {
    let Some(item) = (unsafe { item_from_ptr(rust_item) }) else {
        return;
    };
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ime_cancel(item);
    }));
}

#[no_mangle]
extern "C" fn sujian_request_repaint(rust_item: *mut c_void) {
    let Some(item) = (unsafe { item_from_ptr(rust_item) }) else {
        return;
    };
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        item.input_request_repaint();
    }));
}
