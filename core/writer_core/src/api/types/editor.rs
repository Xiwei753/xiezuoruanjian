//! Editor DTOs for cross-platform FFI.
//!
//! These DTOs mirror the Core `editor` types but are
//! stable API boundary types used by UniFFI and C-ABI FFI layers.
//! Platform clients (Android, Linux_qt, HarmonyOS) consume these
//! to apply edit results and derive platform-local animation.
//!
//! 边界契约：
//! - 所有 byte offset / byte range 字段均为 UTF-8 byte offset（半开区间 [start, end)）
//! - 平台端 UTF-16 index 只存在于平台 TextIndexMap 内，传入 Core 前必须转换
//! - DTO 的 u32 字段对应 Kotlin ULong/Int——平台端需注意溢出和符号
//! - `has_inserted_range` / `has_deleted_range` 必须先检查，不能依赖 start/end == 0 判断
//!   因为 0..0 在 Kotlin IntRange 中包含一个元素（0），不是空范围
//!
//! Issue #735：动画下沉平台后，Core 只保留编辑事实（cause / operation_kind / offset_map）。
//! 平台要做动画时从 cause 推导动画策略，不再拿 Core 的 Visual DTO。

/// Cause of an editor transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum EditorTransactionCauseDto {
    #[default]
    Typing,
    Delete,
    ImeComposition,
    TypingCommit,
    Paste,
    Undo,
    Redo,
    Load,
    Format,
    Programmatic,
}

impl From<EditorTransactionCauseDto> for crate::editor::EditorTransactionCause {
    fn from(c: EditorTransactionCauseDto) -> Self {
        match c {
            EditorTransactionCauseDto::Typing => Self::Typing,
            EditorTransactionCauseDto::Delete => Self::Delete,
            EditorTransactionCauseDto::ImeComposition => Self::ImeComposition,
            EditorTransactionCauseDto::TypingCommit => Self::TypingCommit,
            EditorTransactionCauseDto::Paste => Self::Paste,
            EditorTransactionCauseDto::Undo => Self::Undo,
            EditorTransactionCauseDto::Redo => Self::Redo,
            EditorTransactionCauseDto::Load => Self::Load,
            EditorTransactionCauseDto::Format => Self::Format,
            EditorTransactionCauseDto::Programmatic => Self::Programmatic,
        }
    }
}

#[allow(clippy::cast_possible_truncation)]
impl From<crate::editor::EditorTransactionCause> for EditorTransactionCauseDto {
    fn from(c: crate::editor::EditorTransactionCause) -> Self {
        match c {
            crate::editor::EditorTransactionCause::Typing => Self::Typing,
            crate::editor::EditorTransactionCause::Delete => Self::Delete,
            crate::editor::EditorTransactionCause::ImeComposition => Self::ImeComposition,
            crate::editor::EditorTransactionCause::TypingCommit => Self::TypingCommit,
            crate::editor::EditorTransactionCause::Paste => Self::Paste,
            crate::editor::EditorTransactionCause::Undo => Self::Undo,
            crate::editor::EditorTransactionCause::Redo => Self::Redo,
            crate::editor::EditorTransactionCause::Load => Self::Load,
            crate::editor::EditorTransactionCause::Format => Self::Format,
            crate::editor::EditorTransactionCause::Programmatic => Self::Programmatic,
        }
    }
}

// ── #535: Editor V2 Kernel DTOs ──

/// 操作类型 DTO — 区分不同编辑操作的语义类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum EditorOperationKindDto {
    #[default]
    Insert,
    Delete,
    Replace,
    CursorOnly,
    CompositionUpdate,
    CompositionCommit,
    CompositionCancel,
    Load,
    Format,
}

impl From<crate::editor::EditorOperationKind> for EditorOperationKindDto {
    fn from(k: crate::editor::EditorOperationKind) -> Self {
        match k {
            crate::editor::EditorOperationKind::Insert => Self::Insert,
            crate::editor::EditorOperationKind::Delete => Self::Delete,
            crate::editor::EditorOperationKind::Replace => Self::Replace,
            crate::editor::EditorOperationKind::CursorOnly => Self::CursorOnly,
            crate::editor::EditorOperationKind::CompositionUpdate => Self::CompositionUpdate,
            crate::editor::EditorOperationKind::CompositionCommit => Self::CompositionCommit,
            crate::editor::EditorOperationKind::CompositionCancel => Self::CompositionCancel,
            crate::editor::EditorOperationKind::Load => Self::Load,
            crate::editor::EditorOperationKind::Format => Self::Format,
        }
    }
}

/// UTF-8 byte 范围 DTO — 半开区间 `[start, end_exclusive)`。
///
/// `start` 和 `end_exclusive` 均为 UTF-8 byte offset，保证 char boundary。
/// 空范围表示为 `start == end_exclusive`（不是 0..0，因为 0..0 在 Kotlin IntRange 中包含一个元素）。
/// 平台端应先检查 `has_inserted_range` / `has_deleted_range` 标志，
/// 不能依赖 `start == 0 && end_exclusive == 0` 判断空范围。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorByteRangeDto {
    pub start: u32,
    pub end_exclusive: u32,
}

// SAFETY: 同上，UTF-8 byte offset 截断安全
#[allow(clippy::cast_possible_truncation)]
impl From<(usize, usize)> for EditorByteRangeDto {
    fn from((start, end): (usize, usize)) -> Self {
        Self {
            start: start as u32,
            end_exclusive: end as u32,
        }
    }
}

#[allow(clippy::cast_possible_truncation)]
impl From<crate::editor::strong_types::Utf8ByteRange> for EditorByteRangeDto {
    fn from(r: crate::editor::strong_types::Utf8ByteRange) -> Self {
        Self {
            start: r.start().value() as u32,
            end_exclusive: r.end().value() as u32,
        }
    }
}

/// #606: 偏移映射类型 DTO — 与 Core `OffsetMapKind` 一一对应。
///
/// 平台端 AffectedLayoutPlanner 直接消费此字段决定 cluster 身份映射策略，
/// 不再在 Kotlin 中独立推导 offset mapping。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OffsetMapKindDto {
    Identity,
    Shifted,
}

impl From<crate::editor::OffsetMapKind> for OffsetMapKindDto {
    fn from(k: crate::editor::OffsetMapKind) -> Self {
        match k {
            crate::editor::OffsetMapKind::Identity => Self::Identity,
            crate::editor::OffsetMapKind::Shifted => Self::Shifted,
        }
    }
}

impl From<OffsetMapKindDto> for crate::editor::OffsetMapKind {
    fn from(k: OffsetMapKindDto) -> Self {
        match k {
            OffsetMapKindDto::Identity => Self::Identity,
            OffsetMapKindDto::Shifted => Self::Shifted,
        }
    }
}

/// #606: 单个偏移映射条目 DTO — 与 Core `OffsetMapEntry` 一一对应。
///
/// `old_byte_offset` / `new_byte_offset` 均为 UTF-8 byte offset。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OffsetMapEntryDto {
    pub old_byte_offset: u32,
    pub new_byte_offset: u32,
    pub length: u32,
    pub kind: OffsetMapKindDto,
}

// SAFETY: UTF-8 byte offset 截断安全（正文长度受 u32 范围约束）
#[allow(clippy::cast_possible_truncation)]
impl From<crate::editor::OffsetMapEntry> for OffsetMapEntryDto {
    fn from(e: crate::editor::OffsetMapEntry) -> Self {
        Self {
            old_byte_offset: e.old_byte_offset.value() as u32,
            new_byte_offset: e.new_byte_offset.value() as u32,
            length: e.length as u32,
            kind: e.kind.into(),
        }
    }
}

// SAFETY: UTF-8 byte offset 截断安全（正文长度受 u32 范围约束）
#[allow(clippy::cast_possible_truncation)]
impl From<OffsetMapEntryDto> for crate::editor::OffsetMapEntry {
    fn from(e: OffsetMapEntryDto) -> Self {
        Self {
            old_byte_offset: crate::editor::strong_types::Utf8ByteOffset::unchecked(
                e.old_byte_offset as usize,
            ),
            new_byte_offset: crate::editor::strong_types::Utf8ByteOffset::unchecked(
                e.new_byte_offset as usize,
            ),
            length: e.length as usize,
            kind: e.kind.into(),
        }
    }
}

/// #606: 偏移映射 DTO — 与 Core `OffsetMap` 一一对应。
///
/// 记录 old 正文 → new 正文的字符身份映射，用于后续正文 cluster 保持身份
/// 并生成 Move 动画，而不是全部 Crossfade/Insert。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OffsetMapDto {
    pub entries: Vec<OffsetMapEntryDto>,
}

impl From<crate::editor::OffsetMap> for OffsetMapDto {
    fn from(m: crate::editor::OffsetMap) -> Self {
        Self {
            entries: m.entries.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<OffsetMapDto> for crate::editor::OffsetMap {
    fn from(m: OffsetMapDto) -> Self {
        Self {
            entries: m.entries.into_iter().map(Into::into).collect(),
        }
    }
}

/// DisplayPatch DTO — 正文增量补丁。
///
/// `replace_byte_start..replace_byte_end_exclusive` 在旧正文中
/// 表示要被替换的范围。`inserted_text` 为替换后的新文本。
/// `resulting_selection_start..resulting_selection_end` 为替换完成后的选区（半开区间）。
///
/// 平台端 DisplayTextMirror 按 DisplayPatch 增量更新 SpannableStringBuilder，
/// 不得根据 old/new 全文重新 diff，也不得先本地改 Buffer 再通知 Core。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayPatchDto {
    pub base_revision: u64,
    pub new_revision: u64,
    pub replace_byte_start: u32,
    pub replace_byte_end_exclusive: u32,
    pub inserted_text: String,
    pub resulting_selection_start: u32,
    pub resulting_selection_end: u32,
}

// SAFETY: 同上，UTF-8 byte offset 截断安全
#[allow(clippy::cast_possible_truncation)]
impl From<crate::editor::DisplayPatch> for DisplayPatchDto {
    fn from(p: crate::editor::DisplayPatch) -> Self {
        Self {
            base_revision: p.base_revision.value(),
            new_revision: p.new_revision.value(),
            replace_byte_start: p.replace_byte_range.start().value() as u32,
            replace_byte_end_exclusive: p.replace_byte_range.end().value() as u32,
            inserted_text: p.inserted_text,
            resulting_selection_start: p.resulting_selection_byte_range.start().value() as u32,
            resulting_selection_end: p.resulting_selection_byte_range.end().value() as u32,
        }
    }
}

/// Composition 会话 DTO — 跨平台传递当前 IME composition 会话状态。
///
/// `generation` 用于过期检测：平台端持有的 generation 与 Core 当前 generation
/// 不匹配时，后续 UpdateComposition/FinishComposition/CancelComposition 被内核拒绝。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompositionSessionDto {
    pub session_id: u64,
    pub base_revision: u64,
    pub generation: u64,
}

/// Composition 完整状态 DTO — 暴露 preedit 文本和 replace range
/// 给平台静态写作区，使其能在不复制业务状态机的前提下显示预输入文本和下划线。
///
/// 字段语义：
/// - `session_id` / `base_revision` / `generation`：与 [CompositionSessionDto] 同义，
///   平台端用过期检测守卫后续 UpdateComposition/FinishComposition/CancelComposition。
/// - `replace_byte_start` / `replace_byte_end_exclusive`：UTF-8 byte offset 半开区间，
///   指明 committed text 中将被 preedit_text 替换的范围。平台端据此构造临时显示文本。
/// - `preedit_text`：当前预输入文本（未提交到正文）。Core 不把它写进正文持久化。
/// - `preedit_cursor_utf16`：preedit 内部光标的 UTF-16 code unit offset（IME 协议要求）。
///   Core 内部用 `Utf16CodeUnitOffset` 强类型承载，DTO 边界仍 u32。
///
/// 平台端构造临时显示文本时：把 committed text 的
/// `[replace_byte_start, replace_byte_end_exclusive)` 替换为 `preedit_text`，
/// composition 下划线范围 = `[replace_byte_start, replace_byte_start + preedit_text.len_utf8())`。
/// 保存正文仍只取 committed text（snapshot.text），不把 preedit 写进文件。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorCompositionStateDto {
    pub session_id: u64,
    pub base_revision: u64,
    pub generation: u64,
    pub replace_byte_start: u32,
    pub replace_byte_end_exclusive: u32,
    pub preedit_text: String,
    pub preedit_cursor_utf16: u32,
}

/// 内容增量 DTO — 本次编辑实际插入/删除的字符统计。
///
/// `_chars` 按 Unicode scalar 计数（非 UTF-8 byte、非 UTF-16 code unit）；
/// Cursor/selection/composition-update 没有 committed 正文变化时为全 0；
/// composition commit、Undo/Redo 按实际 delta 统计。Android 直接消费此字段，
/// 不再用 UTF-8 byte 长度或全文重算。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EditorContentDeltaDto {
    pub inserted_chars: u32,
    pub deleted_chars: u32,
    pub inserted_non_whitespace_chars: u32,
    pub deleted_non_whitespace_chars: u32,
}

impl From<crate::editor::EditorContentDelta> for EditorContentDeltaDto {
    fn from(d: crate::editor::EditorContentDelta) -> Self {
        Self {
            inserted_chars: d.inserted_chars,
            deleted_chars: d.deleted_chars,
            inserted_non_whitespace_chars: d.inserted_non_whitespace_chars,
            deleted_non_whitespace_chars: d.deleted_non_whitespace_chars,
        }
    }
}

/// 编辑结果 DTO — EditorKernel.apply() 的跨平台返回值。
///
/// 包含正文变化（display_patches）、选区变化、编辑事实和 composition 会话状态。
/// 平台端按此结果增量更新显示镜像、布局，并自行推导动画策略。
///
/// 选区字段 `old_selection_anchor/head` 和 `new_selection_anchor/head` 均为
/// UTF-8 byte offset，**保留 anchor/head 方向**（Issue #683）。
/// 平台端不得把 anchor/head 当作排序后的 range start/end——反向选区方向会丢失。
/// 需要无方向的实际覆盖范围时由 `min(anchor,head)..max(anchor,head)` 派生。
/// 平台端使用 UTF-16 时必须通过 TextIndexMap 转换，不得直接用于 SpannableStringBuilder。
///
/// Issue #735：`cause` / `operation_kind` / `offset_map` 直接暴露编辑事实。
/// 平台要做动画时从 cause 推导动画策略，不再拿 Core 的 Visual DTO。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorEditResultDto {
    pub outcome: EditorEditOutcomeDto,
    pub transaction_id: u64,
    pub base_revision: u64,
    pub new_revision: u64,
    pub display_patches: Vec<DisplayPatchDto>,
    /// 编辑前选区 anchor（UTF-8 byte offset，保留方向）。
    pub old_selection_anchor: u32,
    /// 编辑前选区 head（UTF-8 byte offset，保留方向）。
    pub old_selection_head: u32,
    /// 编辑后选区 anchor（UTF-8 byte offset，保留方向）。
    pub new_selection_anchor: u32,
    /// 编辑后选区 head（UTF-8 byte offset，保留方向）。
    pub new_selection_head: u32,
    /// 编辑事实：本次事务的原因。
    pub cause: EditorTransactionCauseDto,
    /// 编辑事实：本次操作的语义类别。
    pub operation_kind: EditorOperationKindDto,
    /// 编辑事实：old 正文 → new 正文的字符身份映射。纯选区/光标操作为 None。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset_map: Option<OffsetMapDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composition_session: Option<CompositionSessionDto>,
    /// 本次编辑的字符增量（正文无变化时为全 0）。
    #[serde(default)]
    pub content_delta: EditorContentDeltaDto,
    /// 当前 composition 完整状态（preedit 文本 replace range cursor）。
    /// 仅在 composition 活跃（begin/update 成功）时非 None；finish/cancel/普通编辑后为 None。
    /// 平台端据此构造临时显示文本和下划线范围，不复制 Core 的 composition 状态机。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composition: Option<EditorCompositionStateDto>,
}

impl EditorEditResultDto {
    pub fn stale_fallback() -> Self {
        Self {
            outcome: EditorEditOutcomeDto::StaleRevision,
            transaction_id: 0,
            base_revision: 0,
            new_revision: 0,
            display_patches: vec![],
            old_selection_anchor: 0,
            old_selection_head: 0,
            new_selection_anchor: 0,
            new_selection_head: 0,
            cause: EditorTransactionCauseDto::Programmatic,
            operation_kind: EditorOperationKindDto::CursorOnly,
            offset_map: None,
            composition_session: None,
            content_delta: EditorContentDeltaDto::default(),
            composition: None,
        }
    }

    pub fn invalid_offset_fallback() -> Self {
        Self {
            outcome: EditorEditOutcomeDto::InvalidOffset,
            transaction_id: 0,
            base_revision: 0,
            new_revision: 0,
            display_patches: vec![],
            old_selection_anchor: 0,
            old_selection_head: 0,
            new_selection_anchor: 0,
            new_selection_head: 0,
            cause: EditorTransactionCauseDto::Programmatic,
            operation_kind: EditorOperationKindDto::CursorOnly,
            offset_map: None,
            composition_session: None,
            content_delta: EditorContentDeltaDto::default(),
            composition: None,
        }
    }

    pub fn invalid_range_fallback() -> Self {
        Self {
            outcome: EditorEditOutcomeDto::InvalidRange,
            transaction_id: 0,
            base_revision: 0,
            new_revision: 0,
            display_patches: vec![],
            old_selection_anchor: 0,
            old_selection_head: 0,
            new_selection_anchor: 0,
            new_selection_head: 0,
            cause: EditorTransactionCauseDto::Programmatic,
            operation_kind: EditorOperationKindDto::CursorOnly,
            offset_map: None,
            composition_session: None,
            content_delta: EditorContentDeltaDto::default(),
            composition: None,
        }
    }
}

/// 编辑结果分类 DTO — 平台必须区分不同结果走不同恢复路径。
///
/// - `Applied`：编辑器状态发生了实际变化（可能只是 selection/cursor 变化，此时 new_revision 可以不变、display_patches 可以为空）
/// - `AppliedWithAdjustedSelection`：编辑成功，但平台传入的选区 offset 不在 char boundary 上，内核已自动对齐
/// - `NoChange`：正文和 selection/cursor 都没有变化
/// - `StaleRevision`：expected_revision 与当前 revision 不匹配，平台需用结果中的最新 revision 重试
/// - `InvalidOffset`：offset 不在 UTF-8 char boundary 上或超出文本范围
/// - `InvalidRange`：range 语义非法（如 start ≥ end 对于 delete）
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EditorEditOutcomeDto {
    Applied,
    AppliedWithAdjustedSelection,
    NoChange,
    StaleRevision,
    InvalidOffset,
    InvalidRange,
}

// SAFETY: 同上，UTF-8 byte offset 截断安全
#[allow(clippy::cast_possible_truncation)]
impl From<crate::editor::EditorEditOutcome> for EditorEditResultDto {
    fn from(outcome: crate::editor::EditorEditOutcome) -> Self {
        let (outcome_dto, r) = match outcome {
            crate::editor::EditorEditOutcome::Applied(r) => (EditorEditOutcomeDto::Applied, r),
            crate::editor::EditorEditOutcome::AppliedWithAdjustedSelection(r) => {
                (EditorEditOutcomeDto::AppliedWithAdjustedSelection, r)
            }
            crate::editor::EditorEditOutcome::NoChange(r) => (EditorEditOutcomeDto::NoChange, r),
            crate::editor::EditorEditOutcome::StaleRevision(r) => {
                (EditorEditOutcomeDto::StaleRevision, r)
            }
            crate::editor::EditorEditOutcome::InvalidOffset(r) => {
                (EditorEditOutcomeDto::InvalidOffset, r)
            }
            crate::editor::EditorEditOutcome::InvalidRange(r) => {
                (EditorEditOutcomeDto::InvalidRange, r)
            }
        };
        Self {
            outcome: outcome_dto,
            transaction_id: r.transaction_id,
            base_revision: r.base_revision.value(),
            new_revision: r.new_revision.value(),
            display_patches: r.display_patches.into_iter().map(Into::into).collect(),
            old_selection_anchor: r.old_selection.anchor.index.value() as u32,
            old_selection_head: r.old_selection.head.index.value() as u32,
            new_selection_anchor: r.new_selection.anchor.index.value() as u32,
            new_selection_head: r.new_selection.head.index.value() as u32,
            cause: r.cause.into(),
            operation_kind: r.operation_kind.into(),
            offset_map: r.offset_map.map(Into::into),
            composition_session: None,
            content_delta: r.content_delta.into(),
            composition: None,
        }
    }
}

// SAFETY: 同上，UTF-8 byte offset 截断安全
#[allow(clippy::cast_possible_truncation)]
impl From<crate::editor::EditorEditResult> for EditorEditResultDto {
    fn from(r: crate::editor::EditorEditResult) -> Self {
        Self {
            outcome: EditorEditOutcomeDto::Applied,
            transaction_id: r.transaction_id,
            base_revision: r.base_revision.value(),
            new_revision: r.new_revision.value(),
            display_patches: r.display_patches.into_iter().map(Into::into).collect(),
            old_selection_anchor: r.old_selection.anchor.index.value() as u32,
            old_selection_head: r.old_selection.head.index.value() as u32,
            new_selection_anchor: r.new_selection.anchor.index.value() as u32,
            new_selection_head: r.new_selection.head.index.value() as u32,
            cause: r.cause.into(),
            operation_kind: r.operation_kind.into(),
            offset_map: r.offset_map.map(Into::into),
            composition_session: None,
            content_delta: r.content_delta.into(),
            composition: None,
        }
    }
}

/// 编辑器会话快照 DTO — 用于跨平台传递编辑器完整状态。
///
/// `cursor` 和 `selection_anchor` 均为 UTF-8 byte offset，保证 char boundary。
/// 平台端使用 UTF-16 时必须通过 TextIndexMap 转换。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorSessionSnapshotDto {
    pub text: String,
    pub revision: u64,
    pub cursor: u32,
    pub selection_anchor: u32,
    pub generation: u64,
    pub chapter_id: String,
    /// 当前 composition 完整状态。composition 活跃时非 None，
    /// 平台端据此构造临时显示文本和下划线范围。无 composition 时为 None。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composition: Option<EditorCompositionStateDto>,
}

#[cfg(test)]
mod tests {
    use super::*;

    ///  : EditorEditResultDto 映射 content_delta（插入/删除/空白统计）。
    #[test]
    fn edit_result_dto_maps_content_delta() {
        use crate::editor::strong_types::{EditorRevision, Utf8ByteRange};
        use crate::editor::EditorCommand;
        use crate::editor::EditorKernel;

        let mut kernel = EditorKernel::with_text("你好世界".to_string(), 12).unwrap();
        let result = kernel
            .apply(EditorCommand::Delete {
                byte_range: Utf8ByteRange::from_ordered(6, 12),
                deleted_text: "世界".to_string(),
                cause: crate::editor::EditorTransactionCause::Delete,
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        assert_eq!(result.content_delta.deleted_chars, 2);
        assert_eq!(result.content_delta.deleted_non_whitespace_chars, 2);
        let dto: EditorEditResultDto = result.into();
        // Unicode scalar 计数：删除 2 个 CJK char，非 UTF-8 byte 数（6）
        assert_eq!(dto.content_delta.deleted_chars, 2u32);
        assert_eq!(dto.content_delta.inserted_chars, 0u32);
        assert_eq!(dto.content_delta.deleted_non_whitespace_chars, 2u32);
        assert_eq!(dto.content_delta.inserted_non_whitespace_chars, 0u32);
    }

    ///  : selection-only 结果 content_delta 为全 0，DTO 默认值正确。
    #[test]
    fn edit_result_dto_selection_only_has_zero_content_delta() {
        use crate::editor::strong_types::{EditorRevision, Utf8ByteOffset};
        use crate::editor::EditorCommand;
        use crate::editor::EditorKernel;

        let mut kernel = EditorKernel::with_text("hello".to_string(), 5).unwrap();
        let result = kernel
            .apply(EditorCommand::SetSelection {
                anchor: Utf8ByteOffset::unchecked(0),
                head: Utf8ByteOffset::unchecked(3),
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        let dto: EditorEditResultDto = result.into();
        assert_eq!(dto.content_delta, EditorContentDeltaDto::default());
    }

    ///  : EditorContentDeltaDto 序列化 camelCase（Android 直接消费）。
    #[test]
    fn content_delta_dto_serializes_camel_case() {
        let dto = EditorContentDeltaDto {
            inserted_chars: 1,
            deleted_chars: 2,
            inserted_non_whitespace_chars: 3,
            deleted_non_whitespace_chars: 4,
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert!(json.contains("\"insertedChars\":1"));
        assert!(json.contains("\"deletedChars\":2"));
        assert!(json.contains("\"insertedNonWhitespaceChars\":3"));
        assert!(json.contains("\"deletedNonWhitespaceChars\":4"));
    }

    #[test]
    fn editor_operation_kind_dto_from_core() {
        assert_eq!(
            EditorOperationKindDto::from(crate::editor::EditorOperationKind::Insert),
            EditorOperationKindDto::Insert
        );
        assert_eq!(
            EditorOperationKindDto::from(crate::editor::EditorOperationKind::Delete),
            EditorOperationKindDto::Delete
        );
        assert_eq!(
            EditorOperationKindDto::from(crate::editor::EditorOperationKind::Replace),
            EditorOperationKindDto::Replace
        );
        assert_eq!(
            EditorOperationKindDto::from(crate::editor::EditorOperationKind::CursorOnly),
            EditorOperationKindDto::CursorOnly
        );
        assert_eq!(
            EditorOperationKindDto::from(crate::editor::EditorOperationKind::CompositionUpdate),
            EditorOperationKindDto::CompositionUpdate
        );
        assert_eq!(
            EditorOperationKindDto::from(crate::editor::EditorOperationKind::CompositionCommit),
            EditorOperationKindDto::CompositionCommit
        );
        assert_eq!(
            EditorOperationKindDto::from(crate::editor::EditorOperationKind::CompositionCancel),
            EditorOperationKindDto::CompositionCancel
        );
        assert_eq!(
            EditorOperationKindDto::from(crate::editor::EditorOperationKind::Load),
            EditorOperationKindDto::Load
        );
        assert_eq!(
            EditorOperationKindDto::from(crate::editor::EditorOperationKind::Format),
            EditorOperationKindDto::Format
        );
    }

    #[test]
    fn editor_edit_result_dto_from_kernel() {
        let mut kernel = crate::editor::EditorKernel::with_text("ab".to_string(), 2).unwrap();
        let result = kernel.apply(crate::editor::EditorCommand::Insert {
            byte_offset: crate::editor::strong_types::Utf8ByteOffset::try_new("ab", 2).unwrap(),
            text: "c".to_string(),
            cause: crate::editor::EditorTransactionCause::Typing,
            expected_revision: crate::editor::strong_types::EditorRevision::new(0),
        });
        let dto: EditorEditResultDto = result.into();
        assert!(dto.transaction_id > 0);
        assert_eq!(dto.base_revision, 0);
        assert_eq!(dto.new_revision, 1);
        assert!(!dto.display_patches.is_empty());
        assert_eq!(dto.operation_kind, EditorOperationKindDto::Insert);
        assert_eq!(dto.cause, EditorTransactionCauseDto::Typing);
    }

    #[test]
    fn display_patch_dto_from_core() {
        let patch = crate::editor::DisplayPatch {
            base_revision: crate::editor::strong_types::EditorRevision::new(0),
            new_revision: crate::editor::strong_types::EditorRevision::new(1),
            replace_byte_range: crate::editor::strong_types::Utf8ByteRange::try_new("abc", 2, 2)
                .unwrap(),
            inserted_text: "c".to_string(),
            resulting_selection_byte_range: crate::editor::strong_types::Utf8ByteRange::try_new(
                "abc", 3, 3,
            )
            .unwrap(),
        };
        let dto: DisplayPatchDto = patch.into();
        assert_eq!(dto.base_revision, 0);
        assert_eq!(dto.new_revision, 1);
        assert_eq!(dto.replace_byte_start, 2);
        assert_eq!(dto.replace_byte_end_exclusive, 2);
        assert_eq!(dto.inserted_text, "c");
    }

    #[test]
    fn editor_edit_result_dto_json_camel_case() {
        let mut kernel = crate::editor::EditorKernel::with_text("ab".to_string(), 2).unwrap();
        let result = kernel.apply(crate::editor::EditorCommand::Insert {
            byte_offset: crate::editor::strong_types::Utf8ByteOffset::try_new("ab", 2).unwrap(),
            text: "c".to_string(),
            cause: crate::editor::EditorTransactionCause::Typing,
            expected_revision: crate::editor::strong_types::EditorRevision::new(0),
        });
        let dto: EditorEditResultDto = result.into();
        let json = serde_json::to_string(&dto).unwrap();

        assert!(
            json.contains("\"transactionId\":"),
            "DTO JSON should use camelCase for transactionId, got: {}",
            json
        );
        assert!(
            json.contains("\"baseRevision\":"),
            "DTO JSON should use camelCase for baseRevision, got: {}",
            json
        );
        assert!(
            json.contains("\"newRevision\":"),
            "DTO JSON should use camelCase for newRevision, got: {}",
            json
        );
        assert!(
            json.contains("\"displayPatches\":"),
            "DTO JSON should use camelCase for displayPatches, got: {}",
            json
        );
        assert!(
            json.contains("\"oldSelectionAnchor\":"),
            "DTO JSON should use camelCase for oldSelectionAnchor, got: {}",
            json
        );
        assert!(
            json.contains("\"oldSelectionHead\":"),
            "DTO JSON should use camelCase for oldSelectionHead, got: {}",
            json
        );
        assert!(
            json.contains("\"newSelectionAnchor\":"),
            "DTO JSON should use camelCase for newSelectionAnchor, got: {}",
            json
        );
        assert!(
            json.contains("\"newSelectionHead\":"),
            "DTO JSON should use camelCase for newSelectionHead, got: {}",
            json
        );
        assert!(
            json.contains("\"cause\":"),
            "DTO JSON should use camelCase for cause, got: {}",
            json
        );
        assert!(
            json.contains("\"operationKind\":"),
            "DTO JSON should use camelCase for operationKind, got: {}",
            json
        );
        assert!(
            json.contains("\"replaceByteStart\":"),
            "DTO JSON should use camelCase for replaceByteStart, got: {}",
            json
        );
        assert!(
            json.contains("\"replaceByteEndExclusive\":"),
            "DTO JSON should use camelCase for replaceByteEndExclusive, got: {}",
            json
        );
        assert!(
            json.contains("\"insertedText\":"),
            "DTO JSON should use camelCase for insertedText, got: {}",
            json
        );
        assert!(
            json.contains("\"resultingSelectionStart\":"),
            "DTO JSON should use camelCase for resultingSelectionStart, got: {}",
            json
        );
        assert!(
            json.contains("\"resultingSelectionEnd\":"),
            "DTO JSON should use camelCase for resultingSelectionEnd, got: {}",
            json
        );
    }
}
