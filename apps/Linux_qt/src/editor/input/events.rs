//! Layer 3: EditorInputEvent — 归一化输入事件类型
//!
//! Linux Qt 输入最终转换为这些事件类型。
//! EditorInputController 消费这些事件，调用 EditorInputHost 修改正文。

use crate::sujian_editor_item::PreeditAttribute;

/// 归一化输入事件 — Layer 2 → Layer 3 的唯一数据流
#[derive(Clone, Debug)]
#[allow(dead_code)] // SAFETY: enum variants used by EditorInputController dispatch
pub(crate) enum EditorInputEvent {
    /// 普通文本插入（键盘直接输入、Linux 符号）
    PlainText { text: String },
    /// 快捷键（Ctrl+A/C/V/X/Z/Y 等）
    Shortcut { key: i32, modifiers: i32 },
    /// Preedit 文本变化（IME 组合输入）
    PreeditChanged { text: String, cursor: usize, attributes: Vec<PreeditAttribute> },
    /// IME commit 上屏
    ImeCommit { text: String },
    /// IME commit 带替换语义（fcitx5 拼音修正等）
    ImeReplacementCommit { text: String, replace_start: i32, replace_length: i32 },
    /// IME 取消
    ImeCancel,
}

pub(crate) const KEY_BACKSPACE: i32 = 0x0100_0003;
pub(crate) const KEY_TAB: i32 = 0x0100_0001;
pub(crate) const KEY_ENTER: i32 = 0x0100_0005;
pub(crate) const KEY_INSERT: i32 = 0x0100_0006;
pub(crate) const KEY_RETURN: i32 = 0x0100_0004;
pub(crate) const KEY_DELETE: i32 = 0x0100_0007;
pub(crate) const KEY_LEFT: i32 = 0x0100_0012;
pub(crate) const KEY_UP: i32 = 0x0100_0013;
pub(crate) const KEY_RIGHT: i32 = 0x0100_0014;
pub(crate) const KEY_DOWN: i32 = 0x0100_0015;
pub(crate) const KEY_HOME: i32 = 0x0100_0010;
pub(crate) const KEY_END: i32 = 0x0100_0011;
pub(crate) const KEY_ESCAPE: i32 = 0x0100_0000;
pub(crate) const KEY_A: i32 = 0x41;
pub(crate) const KEY_C: i32 = 0x43;
pub(crate) const KEY_V: i32 = 0x56;
pub(crate) const KEY_X: i32 = 0x58;
pub(crate) const KEY_Y: i32 = 0x59;
pub(crate) const KEY_Z: i32 = 0x5a;
pub(crate) const CTRL_MODIFIER: i32 = 0x0400_0000;
pub(crate) const SHIFT_MODIFIER: i32 = 0x0200_0000;
pub(crate) const ALT_MODIFIER: i32 = 0x0800_0000;
pub(crate) const META_MODIFIER: i32 = 0x1000_0000;

pub(crate) fn has_ctrl(modifiers: i32) -> bool {
    modifiers & CTRL_MODIFIER != 0
}

pub(crate) fn has_shift(modifiers: i32) -> bool {
    modifiers & SHIFT_MODIFIER != 0
}

pub(crate) fn has_alt(modifiers: i32) -> bool {
    modifiers & ALT_MODIFIER != 0
}

pub(crate) fn has_meta(modifiers: i32) -> bool {
    modifiers & META_MODIFIER != 0
}

pub(crate) fn is_copy_shortcut(key: i32, modifiers: i32) -> bool {
    has_ctrl(modifiers) && (key == KEY_C || key == KEY_INSERT)
}

pub(crate) fn is_paste_shortcut(key: i32, modifiers: i32) -> bool {
    (has_ctrl(modifiers) && key == KEY_V) || (has_shift(modifiers) && key == KEY_INSERT)
}

pub(crate) fn is_redo_shortcut(key: i32, modifiers: i32) -> bool {
    has_ctrl(modifiers) && (key == KEY_Y || (has_shift(modifiers) && key == KEY_Z))
}

pub(crate) fn is_destructive_key(key: i32, modifiers: i32) -> bool {
    key == KEY_BACKSPACE || key == KEY_DELETE || (has_ctrl(modifiers) && key == KEY_X)
}

pub(crate) fn decode_utf16_lossy(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

pub(crate) fn decode_utf16_ptr(text: *const u16, text_len: i32) -> String {
    if text.is_null() || text_len <= 0 {
        return String::new();
    }
    // SAFETY: text is checked for null above; text_len is checked > 0 above; the C++ caller guarantees the pointer is valid for text_len elements.
    let slice = unsafe { std::slice::from_raw_parts(text, text_len as usize) };
    decode_utf16_lossy(slice)
}
