//! Layer 3: EditorInputController — 归一化输入事件分发
//!
//! 接收归一化 EditorInputEvent，调用 EditorInputHost 修改正文和生成视觉事务。
//! 不包含任何平台分支逻辑。动画状态不影响文本正确性。
//!
//! Issue #701 评论 5699569220: controller 只分发归一化后的编辑事件，
//! 不再向 EditorInputHost 传 Qt UTF-16 replacement 参数。IME commit/replace
//! 的 UTF-16→UTF-8 坐标换算已在 platform_ime 完成。

use super::events::*;
use crate::sujian_editor_item::PreeditAttribute;

/// 编辑器输入宿主 trait — 平台端必须实现。
///
/// 线程安全：所有方法在 GUI 线程上调用，实现方不得跨线程访问。
/// Qt 对象（QWidget/QQuickItem）只能在主线程使用，后台线程只能发送强类型命令。
///
/// `input_set_suppress_next_ime_commit` / `input_take_suppress_next_ime_commit`
/// 用于处理 ESC 取消 preedit 后延迟到达的 commit 事件。
///
/// Issue #704: `suppress_next_ime_commit` 的语义已收窄为"刚刚取消过一个真实
/// composition，允许忽略它可能迟到的一次 commit"。它不再表示"最近按过 ESC"。
/// 只有 `input_cancel_preedit_for_escape` 在确认当前确实存在活跃
/// composition/preedit 并真的执行了取消后，才会武装一次该标记。普通 ESC（无
/// composition）不会武装，避免吞掉下一次直接 IME commit。
/// 这是一个一次性消费标记（take-and-clear），防止多次 commit 被误抑制。
pub(crate) trait EditorInputHost {
    fn input_enabled(&self) -> bool;
    fn input_emit_explicit_clear_requested(&mut self);
    fn input_clipboard_copy(&mut self) -> bool;
    fn input_clipboard_paste(&mut self);
    fn input_undo(&mut self);
    fn input_redo(&mut self);
    fn input_select_all(&mut self);
    fn input_delete_selection(&mut self);
    fn input_delete_backward(&mut self);
    fn input_delete_forward(&mut self);
    fn input_insert_text(&mut self, text: String);
    /// IME commit/replace — 接收 Qt 两步语义的 `ImeReplaceEvent`。
    ///
    /// 事件由 `platform_ime` 结合当前 `CompositionSession` 把 Qt 的
    /// `replacementStart`/`replacementLength`（UTF-16 QChar 偏移）解析后构造，
    /// 携带：
    /// - `selection_byte_range`：第一步删除当前 selection 的 committed text
    ///   UTF-8 byte range（半开区间），`None` 表示无 selection 删除；
    /// - `replacement_byte_range_after_selection`：第二步在删完 selection 后的
    ///   文本（base_text）上做 replacement/commit 的 byte range（半开区间）；
    /// - `inserted_text`：第二步插入的 commit 文本（已 UTF-16→UTF-8 解码）。
    ///
    /// 进入此 trait 方法后不再携带任何 Qt 坐标。
    fn input_ime_replace_and_commit(&mut self, event: ImeReplaceEvent);
    fn input_move_cursor_horizontal(&mut self, forward: bool, extend: bool);
    fn input_move_cursor_vertical(&mut self, down: bool, extend: bool);
    fn input_move_to_line_edge(&mut self, end: bool, extend: bool);
    fn input_clear_preedit(&mut self);
    /// Issue #704: 用户按 ESC 请求取消当前输入法组合态。
    ///
    /// 实现方必须：先判断当前是否存在活跃 composition / 非空 preedit
    /// （`is_composing()`），沿用 `input_clear_preedit` 的动画清理逻辑执行取消；
    /// 仅当确实取消过一次真实 composition 时，才武装一次
    /// `suppress_next_ime_commit`（用于忽略该 composition 可能迟到的一次 commit）。
    /// 当前没有 composition 时按 ESC 不能改变下一次 IME commit 的处理结果。
    fn input_cancel_preedit_for_escape(&mut self);
    fn input_set_preedit(&mut self, text: String, cursor: usize);
    fn input_set_preedit_with_attrs(
        &mut self,
        text: String,
        cursor: usize,
        attributes: Vec<PreeditAttribute>,
    );
    fn input_set_suppress_next_ime_commit(&mut self, value: bool);
    fn input_take_suppress_next_ime_commit(&mut self) -> bool;
    fn input_request_repaint(&mut self);
}

pub(crate) fn handle_key<H: EditorInputHost + ?Sized>(
    host: &mut H,
    key: i32,
    modifiers: i32,
) -> bool {
    if !host.input_enabled() {
        return false;
    }
    let ctrl = has_ctrl(modifiers);
    let shift = has_shift(modifiers);
    if is_copy_shortcut(key, modifiers) {
        host.input_clipboard_copy();
        return true;
    }
    if is_paste_shortcut(key, modifiers) {
        host.input_clipboard_paste();
        return true;
    }
    if is_redo_shortcut(key, modifiers) {
        host.input_redo();
        return true;
    }
    if ctrl {
        match key {
            KEY_A => {
                host.input_select_all();
                return true;
            }
            KEY_X => {
                host.input_clipboard_copy();
                host.input_delete_selection();
                return true;
            }
            KEY_Z => {
                host.input_undo();
                return true;
            }
            _ => return false,
        }
    }

    match key {
        KEY_ESCAPE => {
            // Issue #704: controller 只表达"用户按 ESC 请求取消输入法组合态"，
            // 由 host 判断当前是否确有活跃 composition 并决定是否武装一次
            // late-commit guard。不再无条件 input_clear_preedit + 武装 suppress。
            host.input_cancel_preedit_for_escape();
        }
        KEY_BACKSPACE => host.input_delete_backward(),
        KEY_DELETE => host.input_delete_forward(),
        KEY_RETURN | KEY_ENTER => host.input_insert_text("\n".to_string()),
        KEY_TAB => host.input_insert_text("\t".to_string()),
        KEY_LEFT => host.input_move_cursor_horizontal(false, shift),
        KEY_RIGHT => host.input_move_cursor_horizontal(true, shift),
        KEY_UP => host.input_move_cursor_vertical(false, shift),
        KEY_DOWN => host.input_move_cursor_vertical(true, shift),
        KEY_HOME => host.input_move_to_line_edge(false, shift),
        KEY_END => host.input_move_to_line_edge(true, shift),
        _ => return false,
    }
    true
}

pub(crate) fn handle_key_and_text<H: EditorInputHost + ?Sized>(
    host: &mut H,
    key: i32,
    modifiers: i32,
    text: String,
) -> bool {
    if !host.input_enabled() {
        return false;
    }

    let ctrl = has_ctrl(modifiers);
    if is_destructive_key(key, modifiers) {
        host.input_emit_explicit_clear_requested();
    }

    if handle_key(host, key, modifiers) {
        return true;
    }

    if !ctrl && !has_alt(modifiers) && !has_meta(modifiers) && !text.is_empty() {
        host.input_insert_text(text);
        return true;
    }

    false
}

pub(crate) fn insert_preedit_text<H: EditorInputHost + ?Sized>(host: &mut H, text: String) {
    if !host.input_enabled() {
        return;
    }
    let cursor = text.len();
    host.input_set_preedit(text, cursor);
    host.input_request_repaint();
}

pub(crate) fn commit_preedit_text<H: EditorInputHost + ?Sized>(host: &mut H, text: String) {
    if !host.input_enabled() {
        return;
    }
    if !text.is_empty() {
        host.input_insert_text(text);
    } else {
        host.input_clear_preedit();
    }
}

pub(crate) fn cancel_preedit<H: EditorInputHost + ?Sized>(host: &mut H) {
    host.input_clear_preedit();
    host.input_request_repaint();
}

/// IME commit 处理。
///
/// 检查 suppress-next-ime-commit 标记：如果被设置（ESC 取消 preedit 后），
/// 仅清除 preedit 而不插入文本；否则正常插入。
/// 这防止了 fcitx5/ibus 在 cancel 后仍发送 commit 的问题。
pub(crate) fn ime_commit<H: EditorInputHost + ?Sized>(host: &mut H, text: String) {
    if !host.input_enabled() || text.is_empty() {
        return;
    }
    if host.input_take_suppress_next_ime_commit() {
        host.input_clear_preedit();
        return;
    }
    host.input_insert_text(text);
}

pub(crate) fn ime_replace_and_commit<H: EditorInputHost + ?Sized>(
    host: &mut H,
    event: ImeReplaceEvent,
) {
    if !host.input_enabled() {
        return;
    }
    // Issue #701 评论 5702675971: 不再因 inserted_text.is_empty() 直接 return。
    // 改为：既无删除（selection 与 replacement range 都零长度）又无插入时 return。
    // 这允许"空 commit + replacement"（纯删除）进入事务。
    if !event.has_any_deletion() && event.inserted_text.is_empty() {
        return;
    }
    if host.input_take_suppress_next_ime_commit() {
        host.input_clear_preedit();
        return;
    }
    host.input_ime_replace_and_commit(event);
}

/// IME preedit 更新。
///
/// `cursor` 参数为 UTF-16 code unit 偏移量（来自 Qt InputMethodEvent），
/// 此处已由 platform_ime 转换为 UTF-8 byte offset。
/// 非空 preedit 会清除 suppress-next-ime-commit 标记（用户重新开始输入）。
pub(crate) fn ime_preedit<H: EditorInputHost + ?Sized>(host: &mut H, text: String, cursor: i32) {
    if !host.input_enabled() {
        return;
    }
    if !text.is_empty() {
        host.input_set_suppress_next_ime_commit(false);
    }
    let cursor = (cursor.max(0) as usize).min(text.len());
    host.input_set_preedit(text, cursor);
}

/// IME preedit 更新（带属性）。
///
/// `attributes` 中的 start/length 已由 platform_ime 从 UTF-16 code unit
/// 转换为 UTF-8 byte offset。`cursor` 同理。
pub(crate) fn ime_preedit_with_attrs<H: EditorInputHost + ?Sized>(
    host: &mut H,
    text: String,
    cursor: i32,
    attributes: Vec<PreeditAttribute>,
) {
    if !host.input_enabled() {
        return;
    }
    if !text.is_empty() {
        host.input_set_suppress_next_ime_commit(false);
    }
    let cursor = (cursor.max(0) as usize).min(text.len());
    host.input_set_preedit_with_attrs(text, cursor, attributes);
}

pub(crate) fn ime_cancel<H: EditorInputHost + ?Sized>(host: &mut H) {
    host.input_clear_preedit();
}
