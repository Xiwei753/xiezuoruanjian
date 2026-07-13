//! Layer 2: Linux PlatformImeAdapter — Rust FFI 回调桥接
//!
//! Linux/fcitx5/ibus: 直接插入，不延迟，按 Qt inputMethodEvent 语义
//!
//! C++ PlatformImeAdapter 类定义在 qt_surface.rs 的 cpp! 块中
//! （cpp! 宏将所有 cpp! 块合并到同一编译单元，类定义不能重复）。
//! 此文件只包含 Rust 侧的 FFI 回调函数。
//!
//! IME query 数据源迁移：
//! handle_input_method_query() 不再直接读 QML property，
//! 而是通过 sujian_get_ime_query_data / sujian_ime_query_text_* FFI 函数
//! 从 SujianEditorItem 内部状态读取，等价于 CursorAnchorAdapter 数据源。

use crate::sujian_editor_item::SujianEditorItem;
use super::controller::*;
use super::events::decode_utf16_ptr;
use std::ffi::c_void;

#[repr(C)]
pub struct SujianImeQueryData {
    pub cursor_rect_x: f64,
    pub cursor_rect_y: f64,
    pub cursor_rect_w: f64,
    pub cursor_rect_h: f64,
    pub has_anchor_rect: bool,
    pub anchor_rect_x: f64,
    pub anchor_rect_y: f64,
    pub anchor_rect_w: f64,
    pub anchor_rect_h: f64,
    pub cursor_char_pos: i32,
    pub anchor_char_pos: i32,
    pub has_selection: bool,
}

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
    let cursor_byte = char_offsets.get(cursor.max(0) as usize).copied().unwrap_or(text.len()) as i32;
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ime_preedit(item, text, cursor_byte);
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
    let cursor_byte = char_offsets.get(cursor.max(0) as usize).copied().unwrap_or(text.len()) as i32;

    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ime_preedit_with_attrs(item, text, cursor_byte, attributes);
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

#[no_mangle]
extern "C" fn sujian_get_ime_query_data(
    rust_item: *mut c_void,
    out: *mut SujianImeQueryData,
) -> bool {
    let Some(item) = (unsafe { item_from_ptr(rust_item) }) else {
        return false;
    };
    if out.is_null() {
        return false;
    }
    let data = SujianImeQueryData {
        cursor_rect_x: item.cursor_rect_x() as f64,
        cursor_rect_y: item.cursor_rect_y() as f64,
        cursor_rect_w: item.cursor_rect_width() as f64,
        cursor_rect_h: item.cursor_rect_height() as f64,
        has_anchor_rect: item.has_selection(),
        anchor_rect_x: item.anchor_rect_x() as f64,
        anchor_rect_y: item.anchor_rect_y() as f64,
        anchor_rect_w: item.anchor_rect_width() as f64,
        anchor_rect_h: item.anchor_rect_height() as f64,
        cursor_char_pos: item.cursor_position() as i32,
        anchor_char_pos: item.anchor_position() as i32,
        has_selection: item.has_selection(),
    };
    unsafe { *out = data };
    true
}

#[no_mangle]
extern "C" fn sujian_ime_query_text_before_cursor(
    rust_item: *mut c_void,
    buf: *mut u16,
    buf_capacity: i32,
) -> i32 {
    let Some(item) = (unsafe { item_from_ptr(rust_item) }) else {
        return 0;
    };
    if buf.is_null() || buf_capacity <= 0 {
        return 0;
    }
    let before_text = item.ime_query_text_before_cursor(100);
    let utf16: Vec<u16> = before_text.encode_utf16().collect();
    let copy_len = utf16.len().min(buf_capacity as usize);
    unsafe {
        std::ptr::copy_nonoverlapping(utf16.as_ptr(), buf, copy_len);
    }
    copy_len as i32
}

#[no_mangle]
extern "C" fn sujian_ime_query_text_after_cursor(
    rust_item: *mut c_void,
    buf: *mut u16,
    buf_capacity: i32,
) -> i32 {
    let Some(item) = (unsafe { item_from_ptr(rust_item) }) else {
        return 0;
    };
    if buf.is_null() || buf_capacity <= 0 {
        return 0;
    }
    let after_text = item.ime_query_text_after_cursor(100);
    let utf16: Vec<u16> = after_text.encode_utf16().collect();
    let copy_len = utf16.len().min(buf_capacity as usize);
    unsafe {
        std::ptr::copy_nonoverlapping(utf16.as_ptr(), buf, copy_len);
    }
    copy_len as i32
}

#[no_mangle]
extern "C" fn sujian_ime_query_selection_text(
    rust_item: *mut c_void,
    buf: *mut u16,
    buf_capacity: i32,
) -> i32 {
    let Some(item) = (unsafe { item_from_ptr(rust_item) }) else {
        return 0;
    };
    if buf.is_null() || buf_capacity <= 0 {
        return 0;
    }
    let sel_text = item.ime_query_selected_text();
    let utf16: Vec<u16> = sel_text.encode_utf16().collect();
    let copy_len = utf16.len().min(buf_capacity as usize);
    unsafe {
        std::ptr::copy_nonoverlapping(utf16.as_ptr(), buf, copy_len);
    }
    copy_len as i32
}
