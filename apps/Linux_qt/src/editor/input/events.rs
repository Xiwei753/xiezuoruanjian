//! Layer 3: EditorInputEvent — 归一化输入事件类型
//!
//! Linux Qt 输入最终转换为这些事件类型。
//! EditorInputController 消费这些事件，调用 EditorInputHost 修改正文。
//!
//! Issue #701 评论 5699569220: IME commit/replace 归一化为 `ImeReplaceEvent`，
//! 直接携带 UTF-8 byte range 和插入文本。普通键盘输入仍归一化为 Insert/Delete。
//! 后续 controller / input_host / editing 不再关心"逗号"还是其他字符，
//! 也不再携带 Qt 的 replacementStart/replacementLength（UTF-16 QChar 偏移）。

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

/// 归一化的 IME commit/replace 事件 — 携带 UTF-8 byte range 和插入文本。
///
/// 由 `platform_ime` 结合当前 `CompositionSession` 把 Qt 的 `replacementStart`/
/// `replacementLength`（UTF-16 QChar 偏移，相对 preedit 起点）解析成 committed
/// text 的 byte range 后构造。进入 editor pipeline 后不再携带任何 Qt 坐标。
///
/// 坐标空间：
/// - `replace_byte_start`/`replace_byte_end`：committed text UTF-8 byte offset
///   （半开区间），由 `platform_ime` 把 base_text 坐标映射回 committed text 坐标
///   后填入。`replace_byte_start <= replace_byte_end`。
/// - `inserted_text`：即将插入的 commit 文本（已 UTF-16→UTF-8 解码）。
///
/// 普通键盘输入仍归一化为 Insert/Delete；IME commit 归一化为 Insert/Replace。
/// 后续代码不关心"逗号"还是其他字符。
pub(crate) struct ImeReplaceEvent {
    pub replace_byte_start: usize,
    pub replace_byte_end: usize,
    pub inserted_text: String,
}

impl ImeReplaceEvent {
    pub(crate) fn new(
        replace_byte_start: usize,
        replace_byte_end: usize,
        inserted_text: String,
    ) -> Self {
        let (start, end) = if replace_byte_start <= replace_byte_end {
            (replace_byte_start, replace_byte_end)
        } else {
            (replace_byte_end, replace_byte_start)
        };
        Self {
            replace_byte_start: start,
            replace_byte_end: end,
            inserted_text,
        }
    }
}
