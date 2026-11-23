//! Layer 2: Linux PlatformImeAdapter — Rust FFI 回调桥接
//!
//! Linux/fcitx5/ibus: 直接插入，不延迟，按 Qt inputMethodEvent 语义
//!
//! C++ PlatformImeAdapter 类定义在 qt_surface.rs 的 cpp! 块中
//! （cpp! 宏将所有 cpp! 块合并到同一编译单元，类定义不能重复）。
//! 此文件包含 Rust 侧的 FFI 回调函数，以及只属于 GUI 线程的 IME 事件身份
//! 跟踪状态（composition session generation / 已提交事件指纹）。
//!
//! IME query 数据源迁移：
//! handle_input_method_query() 不再直接读 QML property，
//! 而是通过 sujian_get_ime_query_data / sujian_ime_query_text_* FFI 函数
//! 从 SujianEditorItem 内部状态读取，等价于 CursorAnchorAdapter 数据源。
//!
//! Issue #701 评论 5699569220: 所有 UTF-16/QChar offset → UTF-8 byte offset/range
//! 只在此文件（和 utf16_converter.rs）转换一次。`sujian_ime_replace_and_commit`
//! 结合当前 CompositionSession 把 Qt replacement 解析成最终 committed byte range，
//! 构造归一化的 `ImeReplaceEvent` 后交给 controller。进入 editor pipeline 后
//! 禁止继续携带 replacementStart/replacementLength 这种 Qt 坐标。
//!
//! Issue #1052 新评论第一点：Qt 事件层不再用固定时间窗猜测 IME 事件来源。
//! IME replace 事件在这里绑定到真实的 composition session generation、
//! 已提交正文 revision 与 selection/replacement 坐标；同一原生输入动作的
//! 重复投递只用事件指纹去重。合法的不带 preedit 的 delete-surrounding-text
//! 照常按 Qt 两步协议进入 Core，不做任何时间关联过滤。

use super::controller::*;
use super::events::{decode_utf16_ptr, ImeReplaceEvent};
use crate::editor::paragraph_index_map::{
    utf16_code_unit_range_to_utf8_byte_range, utf16_code_unit_to_utf8_byte,
};
use crate::platform::linux_qt::utf16_converter::{
    utf16_backward_from_byte, utf16_forward_from_byte,
};
use crate::sujian_editor_item::SujianEditorItem;
use std::cell::RefCell;
use std::ffi::c_void;

// ── Issue #1052：IME 事件身份（composition session / revision / selection） ──
//
// Qt 的 `QInputMethodEvent` 不携带事件 id，也没有可信的时间戳。旧实现用
// “剪贴板快捷键后 500ms 内、且没有 active composition”来猜“这条空 commit
// replacement 是快捷键附带的误事件”，会把两次无关键作间隔小于 500ms 的真实
// IME 删除也吞掉。这里改成只认事件身份：
//
// - 每条 IME replace 事件在进入 Core 前捕获当前已提交正文的 revision 与
//   selection/cursor（UTF-16 QChar 坐标），并绑定它所属的 composition
//   session generation（每次新 composition 开始时递增）。
// - 事件提交成功后记录“提交前锚点 + replacementStart/Length + commit 文本
//   长度/FNV 指纹 + 提交后 revision/selection”的完整指纹。
// - 后续事件只有与这份指纹完全重合、且锚点仍停在“提交后 revision/selection”
//   上，才判定为同一原生输入动作的重复投递并忽略；否则一律放行。
// - 指纹不存 commit 正文，只存长度与 64 位哈希，诊断日志同样不记正文。
//
// 状态按 SujianEditorItem 指针隔离；所有入口都只在 Qt GUI 线程上调用，
// 因此用线程局部状态而不是全局锁。拿不到指纹（例如该 item 还没有提交过
// IME 事件）时一律放行：宁可不拦截，也不吞合法输入。

/// 一次 IME 事件进入 Core 前/后的文档锚点（不含正文内容）。
#[derive(Clone, Copy, Debug)]
struct ImeDocumentAnchor {
    /// 已提交正文的 Core revision。
    revision: u64,
    /// 光标 UTF-16 QChar 绝对位置（与 Qt IME 协议一致）。
    cursor_utf16: u32,
    /// selection anchor UTF-16 QChar 绝对位置。
    anchor_utf16: u32,
    has_selection: bool,
    /// 正文 UTF-8 字节长度，额外用于区分指针复用后的不同文档。
    body_bytes: u32,
    /// 捕获锚点时 item 是否真的存在 composition session。
    composing: bool,
}

impl ImeDocumentAnchor {
    fn capture(item: &SujianEditorItem) -> Self {
        Self {
            revision: item.text_revision(),
            cursor_utf16: item.cursor_position_utf16(),
            anchor_utf16: item.anchor_position_utf16(),
            has_selection: item.has_selection(),
            body_bytes: item.plain_text_length(),
            composing: item.has_active_composition(),
        }
    }

    /// 两个锚点是否落在同一正文 revision 的同一 selection 上。
    ///
    /// revision 本身就是正文内容的权威版本号；body_bytes 只用于在 item 指针
    /// 被复用且 revision 恰好相同时再多一道区分。
    fn same_document_position(&self, other: &Self) -> bool {
        self.revision == other.revision
            && self.body_bytes == other.body_bytes
            && self.cursor_utf16 == other.cursor_utf16
            && self.anchor_utf16 == other.anchor_utf16
            && self.has_selection == other.has_selection
    }
}

/// 最近一次已提交 IME 事件的指纹。
#[derive(Clone, Debug)]
struct CommittedImeEvent {
    /// 所属 SujianEditorItem 指针（按 item 隔离去重窗口）。
    item: usize,
    /// composition session generation；无 session 的事件为 `None`。
    generation: Option<u64>,
    /// 提交前的正文 revision / selection / composition 状态。
    before: ImeDocumentAnchor,
    replace_start_utf16: i32,
    replace_length_utf16: i32,
    /// commit 文本的 UTF-16 长度与 FNV-1a 指纹（不保存正文）。
    commit_utf16_len: u32,
    commit_hash: u64,
    /// 归一化后的两步 Core 坐标；只用于诊断取证，不参与重复投递判定
    /// （重复投递是在“提交后”正文/selection 上重新解析，坐标必然变化）。
    selection_byte_range: Option<(usize, usize)>,
    replacement_byte_range: (usize, usize),
    /// 提交产生事务后的 revision / selection。
    after: ImeDocumentAnchor,
}

/// 当前活跃 composition session 的身份。
#[derive(Clone, Copy)]
struct ActiveComposition {
    item: usize,
    generation: u64,
    /// session 建立时的已提交正文 revision；提交时用它判断 replacement
    /// 坐标是否还锚在当前正文上。
    base_revision: u64,
}

/// IME 事件身份跟踪器（GUI 线程、按 item 指针隔离）。
#[derive(Default)]
struct ImeEventTracker {
    /// 已分配的 composition generation 计数。
    next_generation: u64,
    active_composition: Option<ActiveComposition>,
    last_committed: Option<CommittedImeEvent>,
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

    /// 当前事件所属的 composition generation；没有活跃 session 时为 `None`。
    fn current_generation(&self, item: usize, composing: bool) -> Option<u64> {
        if !composing {
            return None;
        }
        match self.active_composition {
            Some(active) if active.item == item => Some(active.generation),
            _ => None,
        }
    }

    /// composition session 建立时的 revision 是否已经与当前正文脱节。
    ///
    /// 只有 item 确实还有活跃 session、且跟踪器记录过同一 item 的 session
    /// 锚点时才判定；拿不到锚点一律按“未知”放行。
    fn composition_is_stale(&self, item: usize, before: &ImeDocumentAnchor) -> bool {
        if !before.composing {
            return false;
        }
        match self.active_composition {
            Some(active) if active.item == item => before.revision != active.base_revision,
            _ => false,
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

/// 文档级重置：载入新正文/切章后，上一章的事件指纹不能参与去重。
///
/// 指纹按 item 指针隔离；同一 EditorItem 切换到另一章时 revision/selection
/// 可能恰好回到旧事件提交后的状态，必须丢弃旧指纹，否则极端情况下会误判
/// 为重复投递。
pub(crate) fn reset_ime_event_tracker_for_item(item: &SujianEditorItem) {
    let item_addr = item as *const SujianEditorItem as usize;
    with_ime_event_tracker(|tracker| {
        if tracker
            .last_committed
            .as_ref()
            .is_some_and(|last| last.item == item_addr)
        {
            tracker.last_committed = None;
        }
        if tracker
            .active_composition
            .is_some_and(|active| active.item == item_addr)
        {
            tracker.active_composition = None;
        }
    });
}

/// 一次 IME replace 事件的原始参数快照（Qt 坐标 + 当前文档锚点）。
#[derive(Clone, Copy)]
struct ImeReplaceEvidence {
    replace_start_utf16: i32,
    replace_length_utf16: i32,
    commit_utf16_len: u32,
    commit_hash: u64,
    generation: Option<u64>,
    before: ImeDocumentAnchor,
}

/// IME replace 事件的处理分类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ImeReplaceGate {
    /// 正常进入 Core（包含合法的无 preedit delete-surrounding-text）。
    Dispatch,
    /// 与刚提交事件完全重合：同一原生输入动作的重复投递，忽略。
    DuplicateDelivery,
    /// 与刚提交事件完全重合，但属于无 session 的直接插入 commit：
    /// 与用户连续两次输入相同文本无法区分，按“宁可不拦截”放行并记录诊断。
    DirectInsertRepeat,
}

/// 64 位 FNV-1a：只用于事件指纹比对，不是内容校验或安全用途。
fn ime_commit_text_hash(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// 半开 byte range 的长度；`None` 视为 0 长度。
fn byte_range_len(range: Option<(usize, usize)>) -> usize {
    range.map_or(0, |(start, end)| end.saturating_sub(start))
}

/// 按事件身份判断本次 IME replace 事件是否为同一原生动作的重复投递。
///
/// 判定顺序：
/// 1. 没有上一条已提交 IME 指纹，或指纹属于别的 item → 放行。
/// 2. 新一轮 composition 已经开始（generation 不同）→ 放行。
/// 3. 当前文档锚点必须与上一条事件的“提交后 revision/selection”完全重合，
///    且 Qt replacementStart/Length 与 commit 长度/哈希完全一致；否则放行。
/// 4. 上一条事件来自 composition session（该 session 已在提交时结束，没有新的
///    generation 就不再可能来自新的 composition 动作），或本次事件本身带有
///    删除作用域 → 判定为同一原生动作的重复投递并忽略；
///    无 session 的直接插入 commit 与连续合法输入无法区分 → 放行。
fn classify_ime_replace(
    tracker: &ImeEventTracker,
    item: usize,
    evidence: &ImeReplaceEvidence,
    event: &ImeReplaceEvent,
) -> ImeReplaceGate {
    let before = &evidence.before;
    let Some(last) = tracker.last_committed.as_ref() else {
        return ImeReplaceGate::Dispatch;
    };
    if last.item != item {
        return ImeReplaceGate::Dispatch;
    }
    // 新一轮 composition 已经开始（generation 不同）：不是上一笔已提交动作的重复投递。
    if !evidence
        .generation
        .is_none_or(|current| last.generation == Some(current))
    {
        return ImeReplaceGate::Dispatch;
    }
    if !before.same_document_position(&last.after) {
        return ImeReplaceGate::Dispatch;
    }
    if evidence.replace_start_utf16 != last.replace_start_utf16
        || evidence.replace_length_utf16 != last.replace_length_utf16
        || evidence.commit_utf16_len != last.commit_utf16_len
        || evidence.commit_hash != last.commit_hash
    {
        return ImeReplaceGate::Dispatch;
    }
    // 这里不能再比较两步归一化后的 byte range：重复投递是在“提交后”的
    // 正文/selection 上重新解析同一 Qt payload，锚点移动后解析结果必然不同。
    // 事件身份由 raw replacementStart/Length + commit 指纹 + 已提交事务
    // revision/selection 共同确定。
    if last.generation.is_some() || event.has_any_deletion() {
        return ImeReplaceGate::DuplicateDelivery;
    }
    ImeReplaceGate::DirectInsertRepeat
}

/// IME replace 事件自身的参数与当前文档锚点字段（不记录正文）。
fn ime_replace_evidence_fields(
    evidence: &ImeReplaceEvidence,
    resolved_event: Option<&ImeReplaceEvent>,
) -> std::collections::BTreeMap<String, serde_json::Value> {
    use serde_json::json;

    let mut fields: std::collections::BTreeMap<String, serde_json::Value> =
        std::collections::BTreeMap::new();
    fields.insert(
        "replace_start_utf16".to_string(),
        json!(evidence.replace_start_utf16),
    );
    fields.insert(
        "replace_length_utf16".to_string(),
        json!(evidence.replace_length_utf16),
    );
    fields.insert(
        "commit_utf16_len".to_string(),
        json!(evidence.commit_utf16_len),
    );
    fields.insert("commit_hash".to_string(), json!(evidence.commit_hash));
    fields.insert(
        "composition_generation".to_string(),
        json!(evidence.generation),
    );
    fields.insert(
        "composition_active".to_string(),
        json!(evidence.before.composing),
    );
    fields.insert("text_revision".to_string(), json!(evidence.before.revision));
    fields.insert(
        "cursor_utf16".to_string(),
        json!(evidence.before.cursor_utf16),
    );
    fields.insert(
        "anchor_utf16".to_string(),
        json!(evidence.before.anchor_utf16),
    );
    fields.insert(
        "has_selection".to_string(),
        json!(evidence.before.has_selection),
    );
    fields.insert("body_bytes".to_string(), json!(evidence.before.body_bytes));
    if let Some(event) = resolved_event {
        fields.insert("insert_bytes".to_string(), json!(event.inserted_text.len()));
        fields.insert(
            "selection_delete_bytes".to_string(),
            json!(byte_range_len(event.selection_byte_range)),
        );
        fields.insert(
            "replacement_delete_bytes".to_string(),
            json!(byte_range_len(Some(
                event.replacement_byte_range_after_selection
            ))),
        );
    }
    fields
}

/// 最近一笔已提交 IME 事件的指纹字段（不记录正文）。
fn ime_commit_fingerprint_fields(
    last: &CommittedImeEvent,
) -> std::collections::BTreeMap<String, serde_json::Value> {
    use serde_json::json;

    let mut fields: std::collections::BTreeMap<String, serde_json::Value> =
        std::collections::BTreeMap::new();
    fields.insert("last_pre_revision".to_string(), json!(last.before.revision));
    fields.insert(
        "last_pre_cursor_utf16".to_string(),
        json!(last.before.cursor_utf16),
    );
    fields.insert(
        "last_pre_anchor_utf16".to_string(),
        json!(last.before.anchor_utf16),
    );
    fields.insert(
        "last_pre_has_selection".to_string(),
        json!(last.before.has_selection),
    );
    fields.insert("last_post_revision".to_string(), json!(last.after.revision));
    fields.insert(
        "last_post_cursor_utf16".to_string(),
        json!(last.after.cursor_utf16),
    );
    fields.insert(
        "last_post_anchor_utf16".to_string(),
        json!(last.after.anchor_utf16),
    );
    fields.insert(
        "last_post_has_selection".to_string(),
        json!(last.after.has_selection),
    );
    fields.insert(
        "last_replace_start_utf16".to_string(),
        json!(last.replace_start_utf16),
    );
    fields.insert(
        "last_replace_length_utf16".to_string(),
        json!(last.replace_length_utf16),
    );
    fields.insert(
        "last_commit_utf16_len".to_string(),
        json!(last.commit_utf16_len),
    );
    fields.insert("last_commit_hash".to_string(), json!(last.commit_hash));
    fields.insert(
        "last_selection_delete_bytes".to_string(),
        json!(byte_range_len(last.selection_byte_range)),
    );
    fields.insert(
        "last_replacement_delete_bytes".to_string(),
        json!(byte_range_len(Some(last.replacement_byte_range))),
    );
    fields
}

/// 记录被事件身份判定拒绝的 IME replace 事件（不记录正文）。
fn record_ime_replace_decision(
    event_name: &str,
    reason: &str,
    evidence: &ImeReplaceEvidence,
    resolved_event: Option<&ImeReplaceEvent>,
    last: Option<&CommittedImeEvent>,
) {
    let mut fields = ime_replace_evidence_fields(evidence, resolved_event);
    fields.insert("reason".to_string(), serde_json::json!(reason));
    if let Some(last) = last {
        fields.extend(ime_commit_fingerprint_fields(last));
    }
    writer_diagnostics::record_event(writer_diagnostics::DiagnosticEvent {
        timestamp_ms: chrono::Utc::now().timestamp_millis(),
        sequence: 0,
        session_id: String::new(),
        level: writer_diagnostics::DiagnosticLevel::Warn,
        origin: writer_diagnostics::DiagnosticOrigin::App,
        event: event_name.to_string(),
        target: "editor.ime".to_string(),
        message: Some(reason.to_string()),
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
    let commit_hash = ime_commit_text_hash(&inserted_text);

    // 1. 在解析 Qt 坐标前捕获本次事件的真实文档锚点与 composition 身份。
    let item_addr = item as *const SujianEditorItem as usize;
    let before = ImeDocumentAnchor::capture(item);
    let (generation, stale) = with_ime_event_tracker(|tracker| {
        (
            tracker.current_generation(item_addr, before.composing),
            tracker.composition_is_stale(item_addr, &before),
        )
    });
    let evidence = ImeReplaceEvidence {
        replace_start_utf16: replace_start,
        replace_length_utf16: replace_length,
        commit_utf16_len,
        commit_hash,
        generation,
        before,
    };

    // 2. 脱节的 composition session 坐标不可信：不解析、不应用。
    if stale {
        ignore_stale_ime_composition(item, &evidence, replace_length, commit_utf16_len);
        return;
    }

    // 3. 解析 Qt 两步语义坐标，再按事件身份决定放行/去重。
    let event = resolve_ime_replace_event(item, inserted_text, replace_start, replace_length);
    let gate = with_ime_event_tracker(|tracker| {
        classify_ime_replace(tracker, item_addr, &evidence, &event)
    });

    match gate {
        ImeReplaceGate::Dispatch | ImeReplaceGate::DirectInsertRepeat => {
            if gate == ImeReplaceGate::DirectInsertRepeat {
                // 指纹与刚提交事件完全重合，但属于无 session 的直接插入 commit：
                // 可能是重复投递，也可能是用户连续输入了相同文本。Qt 层没有
                // 事件 id 可继续区分，按“宁可不拦截”放行，只留取证日志。
                with_ime_event_tracker(|tracker| {
                    record_ime_replace_decision(
                        "editor.ime.replace_possible_duplicate_passthrough",
                        "direct_insert_repeat_passthrough",
                        &evidence,
                        Some(&event),
                        tracker.last_committed.as_ref(),
                    );
                });
            }
            dispatch_ime_replace(item, item_addr, &evidence, event);
        }
        ImeReplaceGate::DuplicateDelivery => {
            ignore_ime_replace_duplicate(item, &evidence, &event, replace_length, commit_utf16_len);
        }
    }
}

/// 把一笔记号明确的 IME replace 事件交给 Core（正文唯一修改入口）。
fn dispatch_ime_replace(
    item: &mut SujianEditorItem,
    item_addr: usize,
    evidence: &ImeReplaceEvidence,
    event: ImeReplaceEvent,
) {
    let before = evidence.before;
    let selection_byte_range = event.selection_byte_range;
    let replacement_byte_range = event.replacement_byte_range_after_selection;
    // 所有正文修改仍只经 Core `EditorCommand`：合法的不带 preedit
    // delete-surrounding-text 也在这里按 Qt 两步协议进入一笔 ImeCommit 事务。
    // 这里不包 catch_unwind/AssertUnwindSafe：入口是 extern "C"，panic 会按
    // Rust 的 FFI 约定 abort，不会展开穿过 C++ 栈；与其它 FFI 入口一致。
    ime_replace_and_commit(item, event);
    let after = ImeDocumentAnchor::capture(item);
    let committed = after.revision != before.revision;
    with_ime_event_tracker(|tracker| {
        if !after.composing {
            tracker.finish_composition();
        }
        if committed {
            tracker.last_committed = Some(CommittedImeEvent {
                item: item_addr,
                generation: evidence.generation,
                before,
                replace_start_utf16: evidence.replace_start_utf16,
                replace_length_utf16: evidence.replace_length_utf16,
                commit_utf16_len: evidence.commit_utf16_len,
                commit_hash: evidence.commit_hash,
                selection_byte_range,
                replacement_byte_range,
                after,
            });
        }
    });
}

/// 忽略一笔与刚提交事件完全重合的重复投递，并留下取证日志（不记正文）。
fn ignore_ime_replace_duplicate(
    item: &SujianEditorItem,
    evidence: &ImeReplaceEvidence,
    event: &ImeReplaceEvent,
    replace_length: i32,
    commit_utf16_len: u32,
) {
    if replace_length > 0 && commit_utf16_len == 0 {
        // 沿用既有“被拒绝的纯删除 IME 事件”诊断口径（不记正文）。
        item.record_ignored_ime_replacement(evidence.replace_start_utf16, replace_length);
    }
    with_ime_event_tracker(|tracker| {
        record_ime_replace_decision(
            "editor.ime.replace_ignored_duplicate",
            "same_native_action_redelivery",
            evidence,
            Some(event),
            tracker.last_committed.as_ref(),
        );
    });
}

/// 忽略一笔坐标已与当前正文脱节的 composition 提交，并清理临时 preedit。
///
/// 不修改正文；清掉 session 是为了让下一次 IME preedit 以当前 cursor/revision
/// 重新建立 session，避免在同一份脱节坐标上反复忽略。
fn ignore_stale_ime_composition(
    item: &mut SujianEditorItem,
    evidence: &ImeReplaceEvidence,
    replace_length: i32,
    commit_utf16_len: u32,
) {
    if replace_length > 0 && commit_utf16_len == 0 {
        item.record_ignored_ime_replacement(evidence.replace_start_utf16, replace_length);
    }
    with_ime_event_tracker(|tracker| {
        record_ime_replace_decision(
            "editor.ime.replace_ignored_stale_session",
            "composition_session_revision_mismatch",
            evidence,
            None,
            tracker.last_committed.as_ref(),
        );
        tracker.finish_composition();
    });
    item.input_clear_preedit();
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
/// range，在 selection 与 replacement 不相邻时会误删中间正文。本函数改成
/// 两步分开的事件模型：
/// - `selection_byte_range` = `Some((session_replace_start, session_replace_end))`
///   if `session_replace_start != session_replace_end` else `None`；
/// - `replacement_byte_range_after_selection` = base_text 坐标的 (del_start, del_end)，
///   不再映射回 committed text 坐标。
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

    // base_text = committed_text 去掉 [session_replace_start, session_replace_end) 段。
    // Qt replacement 相对 preedit 起点（= session_replace_start in base_text）解释。
    let mut base_text = String::with_capacity(committed_text.len());
    base_text.push_str(&committed_text[..session_replace_start]);
    base_text.push_str(&committed_text[session_replace_end..]);

    let anchor_in_base = session_replace_start;

    // Qt replacementStart 可正可负：正值向前走，负值向后走。
    let rs_byte = if replace_start < 0 {
        utf16_backward_from_byte(&base_text, anchor_in_base, (-replace_start) as usize)
    } else {
        utf16_forward_from_byte(&base_text, anchor_in_base, replace_start as usize)
    };
    // replacementLength 总是非负，从 rs_byte 向前走。
    let re_byte = if replace_length > 0 {
        utf16_forward_from_byte(&base_text, rs_byte, replace_length as usize)
    } else {
        rs_byte
    };
    let (del_start, del_end) = if rs_byte <= re_byte {
        (rs_byte, re_byte)
    } else {
        (re_byte, rs_byte)
    };

    // Issue #701 评论 5702675971: 不再把 base_text 坐标映射回 committed text 坐标。
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
    // replacement），session 身份到此结束；保留最近一笔已提交事件指纹，
    // 供随后可能到达的重复投递继续按事件身份比对。
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
