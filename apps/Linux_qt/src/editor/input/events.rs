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

/// 归一化的 IME commit/replace 事件 — Qt `QInputMethodEvent` 两步语义。
///
/// Qt 官方 `QInputMethodEvent` 语义是两步：
/// 1. 先删除当前 selection（committed text 上的 byte range）；
/// 2. 再按 `replacementStart`/`replacementLength` 做 replacement/commit，
///    replacement 时忽略 preedit 区域。
///
/// 旧实现把这两步硬合成一个连续 committed byte range，在 selection 与
/// replacement 不相邻时会误删中间正文。本结构改成两步分开的事件模型，
/// `editing.rs` 的 `EditOp::ImeCommit` 顺序执行两步 pipeline edit，
/// 但只在 `record_edit_transaction` 末尾做一次 committed 投影同步 + 一次 snapshot +
/// 一次视觉事务。
///
/// 坐标空间（全部 UTF-8 byte offset，半开区间）：
/// - `selection_byte_range`：第一步删除的 committed text byte range。
///   `None` 表示无 selection 删除（session replace range 零长度）。
/// - `replacement_byte_range_after_selection`：第二步在删完 selection 后的
///   文本（base_text）上做 replacement/commit 的 byte range。`(start, end)`，
///   `start <= end`。这是 base_text 坐标，不是 committed text 坐标。
/// - `inserted_text`：第二步插入的 commit 文本（已 UTF-16→UTF-8 解码）。
///   可以为空（纯删除场景）。
///
/// 普通键盘输入仍归一化为 Insert/Delete；IME commit 归一化为 ImeCommit。
/// 后续代码不关心"逗号"还是其他字符。
#[derive(Clone)]
pub(crate) struct ImeReplaceEvent {
    /// 第一步：删除当前 selection（committed text UTF-8 byte range，半开区间）。
    /// `None` 表示无 selection 删除。
    pub selection_byte_range: Option<(usize, usize)>,
    /// 第二步：在删完 selection 后的文本（base_text）上做 replacement/commit。
    /// `(start, end)` 是 base_text UTF-8 byte offset（半开区间），`start <= end`。
    pub replacement_byte_range_after_selection: (usize, usize),
    /// 插入文本（已 UTF-16→UTF-8 解码）。可以为空（纯删除）。
    pub inserted_text: String,
}

impl ImeReplaceEvent {
    pub(crate) fn new(
        selection_byte_range: Option<(usize, usize)>,
        replacement_byte_range: (usize, usize),
        inserted_text: String,
    ) -> Self {
        // 归一化 replacement range 使 start <= end。
        let (rep_start, rep_end) = replacement_byte_range;
        let replacement_byte_range_after_selection = if rep_start <= rep_end {
            (rep_start, rep_end)
        } else {
            (rep_end, rep_start)
        };
        Self {
            selection_byte_range,
            replacement_byte_range_after_selection,
            inserted_text,
        }
    }

    /// 返回 true 如果事件包含任何删除操作：
    /// - selection 删除（`selection_byte_range` 存在且 start != end），或
    /// - replacement 删除（`replacement_byte_range_after_selection` 的 start != end）。
    ///
    /// 用于 controller 判断"既无删除也无插入"的纯 noop 场景（直接 return），
    /// 但允许"空 commit + replacement"（纯删除）进入事务。
    pub(crate) fn has_any_deletion(&self) -> bool {
        if let Some((sel_start, sel_end)) = self.selection_byte_range {
            if sel_start != sel_end {
                return true;
            }
        }
        let (rep_start, rep_end) = self.replacement_byte_range_after_selection;
        rep_start != rep_end
    }
}
