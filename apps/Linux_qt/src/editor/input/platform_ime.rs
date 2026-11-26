//! Layer 2: Linux PlatformImeAdapter — Rust FFI 回调桥接
//!
//! Linux/fcitx5/ibus: 直接插入，不延迟，按 Qt inputMethodEvent 语义
//!
//! C++ PlatformImeAdapter 类定义在 qt_surface.rs 的 cpp! 块中
//! （cpp! 宏将所有 cpp! 块合并到同一编译单元，类定义不能重复）。
//! 此文件包含 Rust 侧的 FFI 回调函数，以及只属于 GUI 线程的 IME composition
//! session 身份跟踪状态（generation / base_revision）。
//!
//! IME query 数据源迁移：
//! handle_input_method_query() 不再直接读 QML property，
//! 而是通过 sujian_get_ime_query_data / sujian_ime_query_text_* FFI 函数
//! 从 SujianEditorItem 内部状态读取，等价于 CursorAnchorAdapter 数据源。
//!
//! Issue #701 评论 5699569220: 所有 UTF-16/QChar offset → UTF-8 byte offset/range
//! 只在此文件转换一次，换算函数统一委托 Core `text_coordinates`。
//! `sujian_ime_replace_and_commit`
//! 结合当前 CompositionSession 把 Qt replacement 解析成最终 committed byte range，
//! 构造归一化的 `ImeReplaceEvent` 后交给 controller。进入 editor pipeline 后
//! 禁止继续携带 replacementStart/replacementLength 这种 Qt 坐标。
//!
//! Issue #1052 新评论第一点：Qt 事件层不再用固定时间窗、也不再按"上一条事件
//! 参数相同"猜测 IME 事件来源或重复投递。Qt 不提供原生唯一事件序号，因此
//! 连续两个合法的 `QInputMethodEvent` 一律按到达顺序交给 Core；不带 preedit
//! 的 delete-surrounding-text 与带 composition 提交的替换都照常按 Qt 两步
//! 协议进入 Core。
//!
//! 本文件只保留可推导的身份校验：composition session 的 generation 与建立时
//! 的已提交 revision。session 建立后正文被其他路径改动（快捷键/撤销/保存
//! 重载）时，replacement 坐标已与当前正文脱节，此时拒绝进入 Core。

use super::controller::*;
use super::events::{decode_utf16_ptr, ImeReplaceEvent};
use crate::editor::paragraph_index_map::{
    utf16_code_unit_range_to_utf8_byte_range, utf16_code_unit_to_utf8_byte,
};
use crate::sujian_editor_item::SujianEditorItem;
use std::cell::RefCell;
use std::ffi::c_void;
use writer_core::editor::text_coordinates::{
    clamp_to_char_boundary, utf16_len, utf16_to_utf8_clamped, utf8_to_utf16_clamped,
};

// ── Issue #1052：IME composition session 身份（generation / base_revision） ──
//
// Qt 的 `QInputMethodEvent` 不携带事件 id，也不提供原生唯一事件序号，且官方
// 文档明确规定空 commitString 的 replacementStart/replacementLength 同样按
// 合法替换/删除处理。因此本层不再猜“哪两条事件是同一原生动作的重复投递”：
// 连续两个合法事件一律按到达顺序交给 Core。
//
// 只保留真正可推导的状态：
// - 每次新 composition 开始时分配新的 generation，并记录 session 建立时的
//   已提交正文 revision（base_revision）。
// - composition 活跃期间，若当前 revision 与 base_revision 不符，说明正文
//   已被其他路径改动，Qt replacement 坐标不再锚在当前正文上 → 拒绝进入 Core、
//   清掉 preedit，让下一次 preedit 以新 revision 重新建立 session。
//
// 状态按 SujianEditorItem 指针隔离；所有入口都只在 Qt GUI 线程上调用，
// 因此用线程局部状态而不是全局锁。拿不到 session 锚点时一律放行：宁可不
// 拦截，也不吞合法输入。

/// 当前活跃 composition session 的身份。
#[derive(Clone, Copy)]
struct ActiveComposition {
    item: usize,
    generation: u64,
    /// session 建立时的已提交正文 revision；提交时用它判断 replacement
    /// 坐标是否还锚在当前正文上。
    base_revision: u64,
}

/// IME composition session 跟踪器（GUI 线程、按 item 指针隔离）。
#[derive(Default)]
struct ImeEventTracker {
    /// 已分配的 composition generation 计数。
    next_generation: u64,
    active_composition: Option<ActiveComposition>,
}

impl ImeEventTracker {
    fn start_composition(&mut self, item: usize, base_revision: u64) {
        self.next_generation = self.next_generation.wrapping_add(1);
        self.active_composition = Some(ActiveComposition {
            item,
            generation: self.next_generation,
            base_revision,
        });
    }

    fn finish_composition(&mut self) {
        self.active_composition = None;
    }

    /// composition session 建立时的 revision 是否已经与当前正文脱节。
    ///
    /// 只有 item 确实还有活跃 session 时才判定；脱节时返回该 session 的身份
    /// （generation/base_revision 只用于诊断，不含正文）。拿不到锚点一律按
    /// “未知”放行。
    fn stale_composition(
        &self,
        item: usize,
        composing: bool,
        revision: u64,
    ) -> Option<ActiveComposition> {
        if !composing {
            return None;
        }
        match self.active_composition {
            Some(active) if active.item == item && revision != active.base_revision => Some(active),
            _ => None,
        }
    }
}

thread_local! {
    /// IME 事件身份只由 GUI 线程的 FFI 入口读写（与 Qt 事件循环同线程）。
    static IME_EVENT_TRACKER: RefCell<ImeEventTracker> =
        RefCell::new(ImeEventTracker::default());
}

fn with_ime_event_tracker<R>(f: impl FnOnce(&mut ImeEventTracker) -> R) -> R {
    IME_EVENT_TRACKER.with(|tracker| {
        let mut state = tracker.borrow_mut();
        f(&mut state)
    })
}

/// 记录 IME preedit 事件开启/延续 composition session 的身份。
///
/// 只有 item 在本次 preedit 之前没有 composition session（或跟踪器里没有
/// 该 item 的 session 身份）时才分配新的 generation；同一次 composition
/// 内的 preedit 更新沿用原身份。
fn note_ime_preedit_activity(item: &SujianEditorItem, has_preedit_text: bool) {
    if !has_preedit_text {
        return;
    }
    let item_addr = item as *const SujianEditorItem as usize;
    let composing = item.has_active_composition();
    let revision = item.text_revision();
    with_ime_event_tracker(|tracker| {
        let needs_identity = tracker
            .active_composition
            .is_none_or(|active| active.item != item_addr);
        if !composing || needs_identity {
            tracker.start_composition(item_addr, revision);
        }
    });
}

/// 文档级重置：载入新正文/切章后，旧的 composition session 坐标不能继续使用。
///
/// 同一 EditorItem 切换到另一章时 revision/selection 可能恰好与旧 session 的
/// base_revision 相同，必须丢弃旧 session 身份，否则会漏掉真实的坐标脱节。
pub(crate) fn reset_ime_event_tracker_for_item(item: &SujianEditorItem) {
    let item_addr = item as *const SujianEditorItem as usize;
    with_ime_event_tracker(|tracker| {
        if tracker
            .active_composition
            .is_some_and(|active| active.item == item_addr)
        {
            tracker.active_composition = None;
        }
    });
}

/// 记录一笔因 composition session revision 脱节而被拒绝的 IME replace 事件。
///
/// 不修改正文，只记录事件参数、session generation/base_revision 与当前
/// revision（不记录正文内容）。
fn record_ime_stale_session(
    replace_start_utf16: i32,
    replace_length_utf16: i32,
    active: ActiveComposition,
    current_revision: u64,
) {
    let mut fields: std::collections::BTreeMap<String, serde_json::Value> =
        std::collections::BTreeMap::new();
    fields.insert(
        "reason".to_string(),
        serde_json::json!("composition_session_revision_mismatch"),
    );
    fields.insert(
        "replace_start_utf16".to_string(),
        serde_json::json!(replace_start_utf16),
    );
    fields.insert(
        "replace_length_utf16".to_string(),
        serde_json::json!(replace_length_utf16),
    );
    fields.insert(
        "composition_generation".to_string(),
        serde_json::json!(active.generation),
    );
    fields.insert(
        "composition_base_revision".to_string(),
        serde_json::json!(active.base_revision),
    );
    fields.insert(
        "text_revision".to_string(),
        serde_json::json!(current_revision),
    );
    writer_diagnostics::record_event(writer_diagnostics::DiagnosticEvent {
        timestamp_ms: chrono::Utc::now().timestamp_millis(),
        sequence: 0,
        session_id: String::new(),
        level: writer_diagnostics::DiagnosticLevel::Warn,
        origin: writer_diagnostics::DiagnosticOrigin::App,
        event: "editor.ime.replace_ignored_stale_session".to_string(),
        target: "editor.ime".to_string(),
        message: Some("composition_session_revision_mismatch".to_string()),
        fields,
    });
}

/// IME 查询数据 — 传递给 Qt InputMethodQuery 的光标和选区几何信息。
///
/// 坐标空间：相对于 SujianEditorItem 左上角的像素坐标（非 dp/vp）。
/// `cursor_char_pos` / `anchor_char_pos` 为 UTF-16 code unit 偏移量，
/// 与 Qt QTextDocument 内部编码一致，需通过 `utf16_code_unit_to_utf8_byte`
/// 转换为 UTF-8 byte offset 才能用于 Core 编辑操作。
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
    // SAFETY: rust_item is null-checked above; the C++ caller guarantees the pointer is a valid SujianEditorItem for the duration of the FFI call; single-threaded GUI access only.
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
    // SAFETY: item_from_ptr checks for null; the C++ caller guarantees the pointer is valid for the FFI call.
    let Some(item) = (unsafe { item_from_ptr(rust_item) }) else {
        return false;
    };
    let text = decode_utf16_ptr(text, text_len);
    // SAFETY: AssertUnwindSafe needed for FFI boundary catch_unwind; the closure only accesses the item through a mutable reference obtained from a null-checked pointer; on panic, the FFI caller discards the item state gracefully.
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
    // SAFETY: item_from_ptr checks for null; the C++ caller guarantees the pointer is valid for the FFI call.
    let Some(item) = (unsafe { item_from_ptr(rust_item) }) else {
        return;
    };
    let text = decode_utf16_ptr(text, text_len);
    // SAFETY: AssertUnwindSafe needed for FFI boundary catch_unwind; the closure only accesses the item through a mutable reference obtained from a null-checked pointer; on panic, the FFI caller discards the item state gracefully.
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
    // SAFETY: item_from_ptr checks for null; the C++ caller guarantees the pointer is valid for the FFI call.
    let Some(item) = (unsafe { item_from_ptr(rust_item) }) else {
        return;
    };
    let inserted_text = decode_utf16_ptr(text, text_len);
    let commit_utf16_len = u32::try_from(inserted_text.encode_utf16().count()).unwrap_or(u32::MAX);

    // 1. 真实 composition session 的 revision 锚点校验：session 建立后正文若被
    //    其他路径改动，Qt replacement 坐标不再锚在当前正文上 → 不解析、不应用。
    let item_addr = item as *const SujianEditorItem as usize;
    let composing = item.has_active_composition();
    let revision = item.text_revision();
    let stale =
        with_ime_event_tracker(|tracker| tracker.stale_composition(item_addr, composing, revision));
    if let Some(active) = stale {
        if replace_length > 0 && commit_utf16_len == 0 {
            // 沿用既有“被拒绝的纯删除 IME 事件”诊断口径（不记正文）。
            item.record_ignored_ime_replacement(replace_start, replace_length);
        }
        record_ime_stale_session(replace_start, replace_length, active, revision);
        with_ime_event_tracker(|tracker| tracker.finish_composition());
        item.input_clear_preedit();
        return;
    }

    // 2. 解析 Qt 两步语义坐标：Qt UTF-16 QChar → Core UTF-8 byte 只在这里做一次。
    let event = resolve_ime_replace_event(item, inserted_text, replace_start, replace_length);

    // 3. 每一条合法的 QInputMethodEvent 按到达顺序交给 Core 一次（正文唯一
    //    修改入口）。Qt 没有提供原生唯一事件序号，所以这里不按“上一条事件
    //    参数相同”推断重复投递：连续两次相同的 delete-surrounding-text、或
    //    两次相同的 composition commit 都是合法输入，必须各自成为一笔事务。
    //    这里不包 catch_unwind/AssertUnwindSafe：入口是 extern "C"，panic 会按
    //    Rust 的 FFI 约定 abort，不会展开穿过 C++ 栈；与其它 FFI 入口一致。
    ime_replace_and_commit(item, event);

    if !item.has_active_composition() {
        with_ime_event_tracker(|tracker| tracker.finish_composition());
    }
}

/// 删除 selection 后的虚拟正文（Qt 两步语义里的 `base_text`）。
///
/// Issue #1052: base_text 由 `prefix` / `suffix` 两段借用切片拼成：
/// `prefix = committed[..selection_start]`、`suffix = committed[selection_end..]`，
/// 普通 IME 输入不再为每次事件复制整章正文。UTF-16→UTF-8 换算只在实际经过的
/// 片段上遍历；replacement 跨越 prefix/suffix 边界时，用已走过片段的 UTF-16
/// 长度换算剩余步数，再在下一段继续。
struct BaseText<'a> {
    prefix: &'a str,
    suffix: &'a str,
}

impl<'a> BaseText<'a> {
    /// 从 committed 正文和要删除的 selection 范围构造虚拟正文。
    ///
    /// selection 端点 clamp 到 `[0, len]` 并对齐到字符起始边界，与
    /// Core `text_coordinates` 的 char boundary 规则一致，避免切片 panic。
    fn new(committed: &'a str, selection_start: usize, selection_end: usize) -> Self {
        let start = clamp_to_char_boundary(committed, selection_start);
        let end = clamp_to_char_boundary(committed, selection_end).max(start);
        Self {
            prefix: &committed[..start],
            suffix: &committed[end..],
        }
    }

    /// 把 base_text byte offset 对齐到字符起始边界。
    fn align(&self, byte_offset: usize) -> usize {
        if byte_offset <= self.prefix.len() {
            clamp_to_char_boundary(self.prefix, byte_offset)
        } else {
            self.prefix.len() + clamp_to_char_boundary(self.suffix, byte_offset - self.prefix.len())
        }
    }

    /// base_text 已对齐 byte offset → base_text UTF-16 code unit offset。
    fn byte_to_utf16(&self, byte_offset: usize) -> usize {
        if byte_offset <= self.prefix.len() {
            utf8_to_utf16_clamped(self.prefix, byte_offset)
        } else {
            utf16_len(self.prefix)
                + utf8_to_utf16_clamped(self.suffix, byte_offset - self.prefix.len())
        }
    }

    /// base_text UTF-16 code unit offset → base_text byte offset。
    ///
    /// 落在代理对中间时由 Core 映射到该码点起始；越界钳到 base_text 末尾。
    fn utf16_to_byte(&self, utf16_offset: usize) -> usize {
        let prefix_utf16 = utf16_len(self.prefix);
        if utf16_offset <= prefix_utf16 {
            utf16_to_utf8_clamped(self.prefix, utf16_offset)
        } else {
            self.prefix.len() + utf16_to_utf8_clamped(self.suffix, utf16_offset - prefix_utf16)
        }
    }

    /// 从 base_text 的 `byte_start` 向前走 `utf16_count` 个 UTF-16 code unit。
    fn utf16_forward_from(&self, byte_start: usize, utf16_count: usize) -> usize {
        let start = self.align(byte_start);
        let start_utf16 = self.byte_to_utf16(start);
        self.utf16_to_byte(start_utf16.saturating_add(utf16_count))
    }

    /// 从 base_text 的 `byte_start` 向后走 `utf16_count` 个 UTF-16 code unit。
    fn utf16_backward_from(&self, byte_start: usize, utf16_count: usize) -> usize {
        let start = self.align(byte_start);
        let start_utf16 = self.byte_to_utf16(start);
        self.utf16_to_byte(start_utf16.saturating_sub(utf16_count))
    }
}

/// 把 Qt `QInputMethodEvent` 的 replacementStart/replacementLength（UTF-16
/// QChar 偏移，相对 preedit 起点）解析成 Qt 两步语义的 `ImeReplaceEvent`。
///
/// Qt 官方语义：先删除当前 selection（committed text 上的 session replace
/// range），再在删完 selection 后的文本（base_text）上按
/// replacementStart/replacementLength 做 replacement/commit（replacement 时
/// 忽略 preedit 区域）。
///
/// Issue #701 评论 5702675971: 旧实现把两步硬合成一个连续 committed byte
/// range，在 selection 与 replacement 不相邻时会误删中间正文。本函数保持
/// 两步分开的事件模型：
/// - `selection_byte_range` = `Some((session_replace_start, session_replace_end))`
///   if `session_replace_start != session_replace_end` else `None`；
/// - `replacement_byte_range_after_selection` = base_text 坐标的 (del_start, del_end)，
///   不映射回 committed text 坐标。
///
/// Issue #1052: 不再 `String::with_capacity(committed_text.len())` 组装整章
/// base_text，而是用 `BaseText` 的两段借用切片表示删掉 selection 后的虚拟正文；
/// 普通直接插入（replacementLength == 0）只换算插入锚点，返回零长度范围
/// `(start, start)`。
///
/// 所有 UTF-16→UTF-8 换算只在此处做一次，`editing.rs` 不再二次换算。
fn resolve_ime_replace_event(
    item: &SujianEditorItem,
    inserted_text: String,
    replace_start: i32,
    replace_length: i32,
) -> ImeReplaceEvent {
    let (session_replace_start, session_replace_end, committed_text) =
        item.ime_replacement_context();

    // 第一步 selection 删除：仅当 session replace range 非零长度时存在。
    let selection_byte_range = if session_replace_start != session_replace_end {
        Some((session_replace_start, session_replace_end))
    } else {
        None
    };

    // 第二步的 base_text = committed_text 去掉
    // [session_replace_start, session_replace_end)，用两段借用切片表示，不复制正文。
    let base_text = BaseText::new(committed_text, session_replace_start, session_replace_end);

    // Qt replacement 相对 preedit 起点（= 删掉 selection 后 prefix 的末尾）解释。
    let anchor_in_base = base_text.prefix.len();

    // Qt replacementStart 可正可负：正值向前走，负值向后走。
    let rs_byte = if replace_start < 0 {
        base_text.utf16_backward_from(anchor_in_base, (-replace_start) as usize)
    } else {
        base_text.utf16_forward_from(anchor_in_base, replace_start as usize)
    };
    // replacementLength == 0 是普通直接插入（或无删除）：范围退化为零长度插入点，
    // 不再换算终点，也不触碰正文其余部分。
    let (del_start, del_end) = if replace_length > 0 {
        let re_byte = base_text.utf16_forward_from(rs_byte, replace_length as usize);
        if rs_byte <= re_byte {
            (rs_byte, re_byte)
        } else {
            (re_byte, rs_byte)
        }
    } else {
        (rs_byte, rs_byte)
    };

    // Issue #701 评论 5702675971: 不把 base_text 坐标映射回 committed text 坐标。
    // (del_start, del_end) 是 base_text 坐标，直接作为第二步 replacement range。
    // editing.rs 的 EditOp::ImeCommit 顺序执行两步 pipeline edit，
    // 第二步在删完 selection 后的 pipeline text（= base_text）上做 replacement。
    ImeReplaceEvent::new(selection_byte_range, (del_start, del_end), inserted_text)
}

#[no_mangle]
extern "C" fn sujian_ime_preedit(
    rust_item: *mut c_void,
    text: *const u16,
    text_len: i32,
    cursor: i32,
) {
    // SAFETY: item_from_ptr checks for null; the C++ caller guarantees the pointer is valid for the FFI call.
    let Some(item) = (unsafe { item_from_ptr(rust_item) }) else {
        return;
    };
    let text = decode_utf16_ptr(text, text_len);
    // Issue #1052: 记录本次（新）composition session 的 generation 与
    // 建立时的已提交 revision；后续 commit 用它们核验事件身份。
    note_ime_preedit_activity(item, text_len > 0);
    let cursor_byte = utf16_code_unit_to_utf8_byte(&text, cursor.max(0) as usize) as i32;
    // SAFETY: AssertUnwindSafe needed for FFI boundary catch_unwind; the closure only accesses the item through a mutable reference obtained from a null-checked pointer; on panic, the FFI caller discards the item state gracefully.
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
    // SAFETY: item_from_ptr checks for null; the C++ caller guarantees the pointer is valid for the FFI call.
    let Some(item) = (unsafe { item_from_ptr(rust_item) }) else {
        return;
    };
    let text = decode_utf16_ptr(text, text_len);

    // Issue #1052: 同 sujian_ime_preedit，preedit 活动绑定真实 session 身份。
    note_ime_preedit_activity(item, text_len > 0);

    let mut attributes = Vec::new();
    if !attr_types.is_null() && !attr_starts.is_null() && !attr_lengths.is_null() && attr_count > 0
    {
        // SAFETY: attr_types is null-checked above; attr_count is checked > 0 above; C++ caller guarantees valid pointers.
        let types_slice = unsafe { std::slice::from_raw_parts(attr_types, attr_count as usize) };
        // SAFETY: attr_starts is null-checked above; attr_count is checked > 0 above.
        let starts_slice = unsafe { std::slice::from_raw_parts(attr_starts, attr_count as usize) };
        // SAFETY: attr_lengths is null-checked above; attr_count is checked > 0 above.
        let lengths_slice =
            unsafe { std::slice::from_raw_parts(attr_lengths, attr_count as usize) };
        let formats_slice = if !attr_formats.is_null() {
            // SAFETY: attr_formats is null-checked above; attr_count is checked > 0 above.
            unsafe { std::slice::from_raw_parts(attr_formats, attr_count as usize) }
        } else {
            &vec![0i32; attr_count as usize]
        };

        for i in 0..attr_count as usize {
            let attr_type = types_slice[i];
            let qchar_start = starts_slice[i].max(0) as usize;
            let qchar_length = lengths_slice[i].max(0) as usize;
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

            let (byte_start, byte_end) =
                utf16_code_unit_range_to_utf8_byte_range(&text, qchar_start, qchar_length);
            let byte_length = byte_end.saturating_sub(byte_start);

            attributes.push(crate::sujian_editor_item::PreeditAttribute {
                start: byte_start,
                length: byte_length,
                kind,
            });
        }
    }

    let cursor_byte = utf16_code_unit_to_utf8_byte(&text, cursor.max(0) as usize) as i32;

    // SAFETY: AssertUnwindSafe needed for FFI boundary catch_unwind; the closure only accesses the item through a mutable reference obtained from a null-checked pointer; on panic, the FFI caller discards the item state gracefully.
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ime_preedit_with_attrs(item, text, cursor_byte, attributes);
    }));
}

#[no_mangle]
extern "C" fn sujian_ime_cancel(rust_item: *mut c_void) {
    // SAFETY: item_from_ptr checks for null; the C++ caller guarantees the pointer is valid for the FFI call.
    let Some(item) = (unsafe { item_from_ptr(rust_item) }) else {
        return;
    };
    // Issue #1052: composition 在没有 commit 的情况下结束（空 preedit + 无
    // replacement），session 身份到此结束；下一次 preedit 以当前 revision
    // 重新建立 session。
    with_ime_event_tracker(|tracker| tracker.finish_composition());
    // SAFETY: AssertUnwindSafe needed for FFI boundary catch_unwind; the closure only accesses the item through a mutable reference obtained from a null-checked pointer; on panic, the FFI caller discards the item state gracefully.
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ime_cancel(item);
    }));
}

#[no_mangle]
extern "C" fn sujian_request_repaint(rust_item: *mut c_void) {
    // SAFETY: item_from_ptr checks for null; the C++ caller guarantees the pointer is valid for the FFI call.
    let Some(item) = (unsafe { item_from_ptr(rust_item) }) else {
        return;
    };
    // SAFETY: AssertUnwindSafe needed for FFI boundary catch_unwind; the closure only accesses the item through a mutable reference obtained from a null-checked pointer; on panic, the FFI caller discards the item state gracefully.
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        item.input_request_repaint();
    }));
}

#[no_mangle]
extern "C" fn sujian_get_ime_query_data(
    rust_item: *mut c_void,
    out: *mut SujianImeQueryData,
) -> bool {
    // SAFETY: item_from_ptr checks for null; the C++ caller guarantees the pointer is valid for the FFI call.
    let Some(item) = (unsafe { item_from_ptr(rust_item) }) else {
        return false;
    };
    if out.is_null() {
        return false;
    }
    let data = SujianImeQueryData {
        cursor_rect_x: f64::from(item.cursor_rect_x()),
        cursor_rect_y: f64::from(item.cursor_rect_y()),
        cursor_rect_w: f64::from(item.cursor_rect_width()),
        cursor_rect_h: f64::from(item.cursor_rect_height()),
        has_anchor_rect: item.has_selection(),
        anchor_rect_x: f64::from(item.anchor_rect_x()),
        anchor_rect_y: f64::from(item.anchor_rect_y()),
        anchor_rect_w: f64::from(item.anchor_rect_width()),
        anchor_rect_h: f64::from(item.anchor_rect_height()),
        // Issue #668: cursor_char_pos / anchor_char_pos 是 UTF-16 code unit
        //（Qt QChar 位置），不是 Rust Unicode scalar 数。用专门的
        // cursor_position_utf16 / anchor_position_utf16 转换，遇到 emoji /
        // 非 BMP 字符（UTF-16 代理对）才不会错位。
        cursor_char_pos: item.cursor_position_utf16() as i32,
        anchor_char_pos: item.anchor_position_utf16() as i32,
        has_selection: item.has_selection(),
    };
    // SAFETY: out is null-checked above; the C++ caller guarantees the pointer is valid.
    unsafe { *out = data };
    true
}

#[no_mangle]
extern "C" fn sujian_ime_query_text_before_cursor(
    rust_item: *mut c_void,
    buf: *mut u16,
    buf_capacity: i32,
) -> i32 {
    // SAFETY: item_from_ptr checks for null; the C++ caller guarantees the pointer is valid for the FFI call.
    let Some(item) = (unsafe { item_from_ptr(rust_item) }) else {
        return 0;
    };
    if buf.is_null() || buf_capacity <= 0 {
        return 0;
    }
    let before_text = item.ime_query_text_before_cursor(100);
    let utf16: Vec<u16> = before_text.encode_utf16().collect();
    let copy_len = utf16.len().min(buf_capacity as usize);
    // SAFETY: buf is null-checked above; buf_capacity is checked > 0 above; copy_len <= buf_capacity; utf16 data is valid.
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
    // SAFETY: item_from_ptr checks for null; the C++ caller guarantees the pointer is valid for the FFI call.
    let Some(item) = (unsafe { item_from_ptr(rust_item) }) else {
        return 0;
    };
    if buf.is_null() || buf_capacity <= 0 {
        return 0;
    }
    let after_text = item.ime_query_text_after_cursor(100);
    let utf16: Vec<u16> = after_text.encode_utf16().collect();
    let copy_len = utf16.len().min(buf_capacity as usize);
    // SAFETY: buf is null-checked above; buf_capacity is checked > 0 above; copy_len <= buf_capacity; utf16 data is valid.
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
    // SAFETY: item_from_ptr checks for null; the C++ caller guarantees the pointer is valid for the FFI call.
    let Some(item) = (unsafe { item_from_ptr(rust_item) }) else {
        return 0;
    };
    if buf.is_null() || buf_capacity <= 0 {
        return 0;
    }
    let sel_text = item.ime_query_selected_text();
    let utf16: Vec<u16> = sel_text.encode_utf16().collect();
    let copy_len = utf16.len().min(buf_capacity as usize);
    // SAFETY: buf is null-checked above; buf_capacity is checked > 0 above; copy_len <= buf_capacity; utf16 data is valid.
    unsafe {
        std::ptr::copy_nonoverlapping(utf16.as_ptr(), buf, copy_len);
    }
    copy_len as i32
}
