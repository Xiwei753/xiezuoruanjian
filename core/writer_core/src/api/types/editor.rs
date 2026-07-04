//! Editor animation DTOs for cross-platform FFI.
//!
//! These DTOs mirror the Core `editor::transaction` types but are
//! stable API boundary types used by UniFFI and C-ABI FFI layers.
//! Platform clients (Android, Desktop, HarmonyOS) consume these
//! to decide whether and how to animate text changes.

/// Kind of animation event emitted by the editor engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum EditorAnimationKindDto {
    #[default]
    Insert,
    Delete,
    Cursor,
}

impl From<crate::editor::EditorAnimationKind> for EditorAnimationKindDto {
    fn from(k: crate::editor::EditorAnimationKind) -> Self {
        match k {
            crate::editor::EditorAnimationKind::Insert => Self::Insert,
            crate::editor::EditorAnimationKind::Delete => Self::Delete,
            crate::editor::EditorAnimationKind::Cursor => Self::Cursor,
        }
    }
}

/// Cause of an editor transaction, used by Core to decide `should_animate`.
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

/// A single animation event produced by the Core EditorEngine.
///
/// The platform side uses `range_start`, `range_len`, `text` to locate
/// the affected text in its own Layout, then computes glyph coordinates
/// and submits them to its renderer (e.g., Android `EditorRenderLayer`).
///
/// **Deprecated**: Use `EditorVisualTransactionDto` instead. This DTO
/// will be removed in a future version.
#[deprecated(
    since = "0.12.0",
    note = "Use EditorVisualTransactionDto instead. This will be removed in a future version."
)]
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorAnimationEventDto {
    pub id: u64,
    pub kind: EditorAnimationKindDto,
    pub range_start: u32,
    pub range_len: u32,
    pub text: String,
    pub old_cursor_index: u32,
    pub new_cursor_index: u32,
    pub duration_ms: u64,
}

#[allow(deprecated)]
impl From<crate::editor::EditorAnimationEvent> for EditorAnimationEventDto {
    fn from(e: crate::editor::EditorAnimationEvent) -> Self {
        Self {
            id: e.id,
            kind: e.kind.into(),
            range_start: e.range_start as u32,
            range_len: e.range_len as u32,
            text: e.text,
            old_cursor_index: e.old_cursor.index as u32,
            new_cursor_index: e.new_cursor.index as u32,
            duration_ms: e.duration_ms,
        }
    }
}

// ── T1.1 VisualCoordinateModeDto ──

/// 视觉坐标模式 DTO。
/// Baseline 表示所有 y 坐标使用 baselineY，
/// Canvas.drawText 永远用 baselineY，不能用 top + height 拼 baseline。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum VisualCoordinateModeDto {
    #[default]
    Baseline,
}

impl From<crate::editor::VisualCoordinateMode> for VisualCoordinateModeDto {
    fn from(m: crate::editor::VisualCoordinateMode) -> Self {
        match m {
            crate::editor::VisualCoordinateMode::Baseline => Self::Baseline,
        }
    }
}

// ── T1.2 CursorRectDto and GlyphRectDto ──

/// 光标矩形信息 DTO，供平台端动画 overlay 使用。
///
/// coordinate_mode=Baseline 时：
/// - baseline_y 是文字基线 Y 坐标
/// - top 是光标顶部 Y 坐标（baseline + ascent）
/// - bottom 是光标底部 Y 坐标（baseline + descent）
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorRectDto {
    pub x: f64,
    pub top: f64,
    pub bottom: f64,
    pub baseline_y: f64,
}

/// 单个 glyph 的精确矩形信息 DTO，供平台端动画 overlay 使用。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlyphRectDto {
    /// 矩形左上角 x 坐标
    pub x: f64,
    /// 矩形左上角 y 坐标
    pub y: f64,
    /// 矩形宽度
    pub w: f64,
    /// 矩形高度
    pub h: f64,
    /// 该 glyph 对应的字符
    #[serde(rename = "char")]
    pub char_: String,
    /// 文字基线 Y 坐标
    #[serde(default)]
    pub baseline_y: f64,
    /// 该 glyph 在文本中的 UTF-8 byte 起始位置
    #[serde(default)]
    pub byte_start: u64,
    /// 该 glyph 在文本中的 UTF-8 byte 结束位置
    #[serde(default)]
    pub byte_end: u64,
}

// ── T1.3 EditorVisualTransactionDto ──

/// 统一编辑器视觉事务 DTO。
///
/// Core 层只裁判事件语义和范围（UTF-8 byte offset），
/// 平台层只负责 layout 坐标转换和绘制。
/// Desktop SujianEditorItem 和 Android SujianEditorView 都吃同一份契约。
///
/// 坐标字段（old_cursor_rect, new_cursor_rect, deleted_glyph_rects, insert_glyph_rects）
/// 不在 FFI DTO 中——由平台层自行填充。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorVisualTransactionDto {
    /// 事务唯一 ID
    pub id: u64,
    /// 动画类型
    pub kind: EditorAnimationKindDto,
    /// 变更原因
    pub cause: EditorTransactionCauseDto,
    /// 旧文本
    pub old_text: String,
    /// 新文本
    pub new_text: String,
    /// 旧选区 anchor（UTF-8 byte offset）
    pub old_selection_anchor: u32,
    /// 旧选区 head（UTF-8 byte offset）
    pub old_selection_head: u32,
    /// 新选区 anchor（UTF-8 byte offset）
    pub new_selection_anchor: u32,
    /// 新选区 head（UTF-8 byte offset）
    pub new_selection_head: u32,
    /// 插入范围起始（UTF-8 byte offset），无插入时为 0
    pub inserted_range_start: u32,
    /// 插入范围结束（UTF-8 byte offset），无插入时为 0
    pub inserted_range_end: u32,
    /// 动画时长（毫秒）
    pub duration_ms: u64,
    /// 坐标模式
    pub coordinate_mode: VisualCoordinateModeDto,
}

impl From<crate::editor::EditorVisualTransaction> for EditorVisualTransactionDto {
    fn from(vt: crate::editor::EditorVisualTransaction) -> Self {
        let (inserted_range_start, inserted_range_end) = vt
            .inserted_range
            .map(|(s, e)| (s as u32, e as u32))
            .unwrap_or((0, 0));
        Self {
            id: vt.id,
            kind: vt.kind.into(),
            cause: match vt.cause {
                crate::editor::EditorTransactionCause::Typing => EditorTransactionCauseDto::Typing,
                crate::editor::EditorTransactionCause::Delete => EditorTransactionCauseDto::Delete,
                crate::editor::EditorTransactionCause::ImeComposition => {
                    EditorTransactionCauseDto::ImeComposition
                }
                crate::editor::EditorTransactionCause::TypingCommit => {
                    EditorTransactionCauseDto::TypingCommit
                }
                crate::editor::EditorTransactionCause::Paste => EditorTransactionCauseDto::Paste,
                crate::editor::EditorTransactionCause::Undo => EditorTransactionCauseDto::Undo,
                crate::editor::EditorTransactionCause::Redo => EditorTransactionCauseDto::Redo,
                crate::editor::EditorTransactionCause::Load => EditorTransactionCauseDto::Load,
                crate::editor::EditorTransactionCause::Format => EditorTransactionCauseDto::Format,
                crate::editor::EditorTransactionCause::Programmatic => {
                    EditorTransactionCauseDto::Programmatic
                }
            },
            old_text: vt.old_text,
            new_text: vt.new_text,
            old_selection_anchor: vt.old_selection.anchor.index as u32,
            old_selection_head: vt.old_selection.head.index as u32,
            new_selection_anchor: vt.new_selection.anchor.index as u32,
            new_selection_head: vt.new_selection.head.index as u32,
            inserted_range_start,
            inserted_range_end,
            duration_ms: vt.duration_ms,
            coordinate_mode: vt.coordinate_mode.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T1.8: Verify all fields are correctly mapped for an Insert visual transaction.
    #[test]
    fn visual_transaction_dto_insert_maps_all_fields() {
        let mut engine = crate::editor::EditorEngine::with_animation_limits(8, 160);
        let tx = engine.create_transaction(
            "ab",
            "abc",
            crate::editor::EditorSelection::collapsed("ab", 2),
            crate::editor::EditorSelection::collapsed("abc", 3),
            crate::editor::EditorTransactionCause::Typing,
        );
        let vt = engine.visual_transaction(&tx).unwrap();
        let dto: EditorVisualTransactionDto = vt.into();

        assert!(dto.id > 0);
        assert_eq!(dto.kind, EditorAnimationKindDto::Insert);
        assert_eq!(dto.cause, EditorTransactionCauseDto::Typing);
        assert_eq!(dto.old_text, "ab");
        assert_eq!(dto.new_text, "abc");
        assert_eq!(dto.old_selection_anchor, 2);
        assert_eq!(dto.old_selection_head, 2);
        assert_eq!(dto.new_selection_anchor, 3);
        assert_eq!(dto.new_selection_head, 3);
        // Insert: inserted_range = Some((2, 3))
        assert_eq!(dto.inserted_range_start, 2);
        assert_eq!(dto.inserted_range_end, 3);
        assert_eq!(dto.duration_ms, 160);
        assert_eq!(dto.coordinate_mode, VisualCoordinateModeDto::Baseline);
    }

    /// T1.8: Verify all fields are correctly mapped for a Delete visual transaction.
    #[test]
    fn visual_transaction_dto_delete_maps_all_fields() {
        let mut engine = crate::editor::EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "abc",
            "ab",
            crate::editor::EditorSelection::collapsed("abc", 3),
            crate::editor::EditorSelection::collapsed("ab", 2),
            crate::editor::EditorTransactionCause::Delete,
        );
        let vt = engine.visual_transaction(&tx).unwrap();
        let dto: EditorVisualTransactionDto = vt.into();

        assert!(dto.id > 0);
        assert_eq!(dto.kind, EditorAnimationKindDto::Delete);
        assert_eq!(dto.cause, EditorTransactionCauseDto::Delete);
        assert_eq!(dto.old_text, "abc");
        assert_eq!(dto.new_text, "ab");
        assert_eq!(dto.old_selection_anchor, 3);
        assert_eq!(dto.old_selection_head, 3);
        assert_eq!(dto.new_selection_anchor, 2);
        assert_eq!(dto.new_selection_head, 2);
        // Delete: inserted_range = None → (0, 0)
        assert_eq!(dto.inserted_range_start, 0);
        assert_eq!(dto.inserted_range_end, 0);
        assert_eq!(dto.duration_ms, 120);
        assert_eq!(dto.coordinate_mode, VisualCoordinateModeDto::Baseline);
    }

    /// T1.8: Verify inserted_range None → (0, 0) explicitly.
    #[test]
    fn visual_transaction_dto_inserted_range_none_maps_to_zeros() {
        let mut engine = crate::editor::EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "abc",
            "ab",
            crate::editor::EditorSelection::collapsed("abc", 3),
            crate::editor::EditorSelection::collapsed("ab", 2),
            crate::editor::EditorTransactionCause::Delete,
        );
        let vt = engine.visual_transaction(&tx).unwrap();
        // Core layer: Delete has inserted_range = None
        assert!(vt.inserted_range.is_none());
        let dto: EditorVisualTransactionDto = vt.into();
        assert_eq!(dto.inserted_range_start, 0);
        assert_eq!(dto.inserted_range_end, 0);
    }

    /// T1.8: Verify inserted_range Some((s, e)) maps correctly.
    #[test]
    fn visual_transaction_dto_inserted_range_some_maps_correctly() {
        let mut engine = crate::editor::EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "ab",
            "abc",
            crate::editor::EditorSelection::collapsed("ab", 2),
            crate::editor::EditorSelection::collapsed("abc", 3),
            crate::editor::EditorTransactionCause::Typing,
        );
        let vt = engine.visual_transaction(&tx).unwrap();
        // Core layer: Insert has inserted_range = Some((2, 3))
        assert_eq!(vt.inserted_range, Some((2, 3)));
        let dto: EditorVisualTransactionDto = vt.into();
        assert_eq!(dto.inserted_range_start, 2);
        assert_eq!(dto.inserted_range_end, 3);
    }

    /// T1.8: Verify cause mapping for all variants.
    #[test]
    fn visual_transaction_dto_cause_mapping() {
        // We test Delete cause specifically since Typing is already covered
        let mut engine = crate::editor::EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "abc",
            "ab",
            crate::editor::EditorSelection::collapsed("abc", 3),
            crate::editor::EditorSelection::collapsed("ab", 2),
            crate::editor::EditorTransactionCause::Delete,
        );
        let vt = engine.visual_transaction(&tx).unwrap();
        let dto: EditorVisualTransactionDto = vt.into();
        assert_eq!(dto.cause, EditorTransactionCauseDto::Delete);
    }

    /// Verify camelCase serialization of EditorVisualTransactionDto.
    #[test]
    fn visual_transaction_dto_serializes_camel_case() {
        let mut engine = crate::editor::EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "ab",
            "abc",
            crate::editor::EditorSelection::collapsed("ab", 2),
            crate::editor::EditorSelection::collapsed("abc", 3),
            crate::editor::EditorTransactionCause::Typing,
        );
        let vt = engine.visual_transaction(&tx).unwrap();
        let dto: EditorVisualTransactionDto = vt.into();
        let json = serde_json::to_string(&dto).unwrap();

        // Verify camelCase field names
        assert!(json.contains("\"oldText\":"));
        assert!(json.contains("\"newText\":"));
        assert!(json.contains("\"oldSelectionAnchor\":"));
        assert!(json.contains("\"oldSelectionHead\":"));
        assert!(json.contains("\"newSelectionAnchor\":"));
        assert!(json.contains("\"newSelectionHead\":"));
        assert!(json.contains("\"insertedRangeStart\":"));
        assert!(json.contains("\"insertedRangeEnd\":"));
        assert!(json.contains("\"durationMs\":"));
        assert!(json.contains("\"coordinateMode\":"));
        // Verify snake_case fields are NOT present
        assert!(!json.contains("\"old_text\":"));
        assert!(!json.contains("\"new_text\":"));
        assert!(!json.contains("\"old_selection_anchor\":"));
        assert!(!json.contains("\"inserted_range_start\":"));
        assert!(!json.contains("\"inserted_range_end\":"));
        assert!(!json.contains("\"duration_ms\":"));
        assert!(!json.contains("\"coordinate_mode\":"));
    }
}
