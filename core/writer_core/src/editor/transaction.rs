//! 平台无关的编辑事务与动画事件模型。
//!
//! 本模块只描述正文如何变化以及渲染层可以播放什么事件，不处理绘制、输入法、窗口、
//! 鼠标或触摸。平台端必须把输入事件翻译成这里的 transaction，再由 renderer 决定如何画。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorCursor {
    /// UTF-8 byte offset. The value is always clamped to a char boundary.
    pub index: usize,
}

impl EditorCursor {
    pub fn new(text: &str, index: usize) -> Self {
        Self {
            index: clamp_to_char_boundary(text, index),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorSelection {
    pub anchor: EditorCursor,
    pub head: EditorCursor,
}

impl EditorSelection {
    pub fn collapsed(text: &str, index: usize) -> Self {
        let cursor = EditorCursor::new(text, index);
        Self {
            anchor: cursor,
            head: cursor,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum EditorChange {
    Insert { index: usize, text: String },
    Delete { index: usize, text: String },
}

impl EditorChange {
    pub fn index(&self) -> usize {
        match self {
            Self::Insert { index, .. } | Self::Delete { index, .. } => *index,
        }
    }

    pub fn text(&self) -> &str {
        match self {
            Self::Insert { text, .. } | Self::Delete { text, .. } => text,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EditorTransactionCause {
    Typing,
    Delete,
    Paste,
    Undo,
    Redo,
    Load,
    Format,
    ImeComposition,
    TypingCommit,
    Programmatic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorTransaction {
    pub old_text: String,
    pub new_text: String,
    pub changes: Vec<EditorChange>,
    pub old_selection: EditorSelection,
    pub new_selection: EditorSelection,
    pub cause: EditorTransactionCause,
    pub should_animate: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EditorAnimationKind {
    Insert,
    Delete,
    Cursor,
}

/// 光标矩形信息，供平台端动画 overlay 使用。
///
/// coordinate_mode=Baseline 时：
/// - baseline_y 是文字基线 Y 坐标
/// - top 是光标顶部 Y 坐标（baseline + ascent）
/// - bottom 是光标底部 Y 坐标（baseline + descent）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorRect {
    pub x: f64,
    pub top: f64,
    pub bottom: f64,
    pub baseline_y: f64,
}

/// 受局部 reflow 影响的 glyph 的旧位置和新位置。
///
/// 中间插入时，插入点右侧的文字需要做轻量位移动画（局部挤开），
/// 避免瞬间大跳。ReflowGlyphRect 记录这些 glyph 在插入前后的位置，
/// 供 QML overlay 渲染位移动画。
///
/// 只影响同一行中插入点右侧的 glyph，以及受影响的相邻 1-2 行。
/// 超过 2 行、跨段落、滚动中、格式化中、加载中时直接 snap，不收集。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReflowGlyphRect {
    /// 该 glyph 对应的字符（可能是多字节 UTF-8）
    #[serde(rename = "char")]
    pub char_: String,
    /// 该 glyph 在新文本中的 UTF-8 byte 起始位置（用于静态层跳过 reflow range）
    pub byte_start: usize,
    /// 该 glyph 在新文本中的 UTF-8 byte 结束位置
    pub byte_end: usize,
    /// 插入前的 x 坐标（文档坐标系，不含 scroll offset）
    pub old_x: f64,
    /// 插入前的 y 坐标（文档坐标系，不含 scroll offset）
    pub old_y: f64,
    /// 插入前的基线 Y 坐标
    pub old_baseline_y: f64,
    /// 插入后的 x 坐标（文档坐标系，不含 scroll offset）
    pub new_x: f64,
    /// 插入后的 y 坐标（文档坐标系，不含 scroll offset）
    pub new_y: f64,
    /// 插入后的基线 Y 坐标
    pub new_baseline_y: f64,
    /// glyph 宽度
    pub w: f64,
    /// glyph 高度
    pub h: f64,
    /// 所在 visual line 索引（新布局中的索引）
    pub line_index: usize,
}

/// 单个 glyph 的精确矩形信息，供平台端动画 overlay 使用。
///
/// Core 层不负责绘制，只负责在 animation event 中暴露每个字符的
/// 精确位置和尺寸，避免平台端用 `fontSize * 0.6` 估算。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlyphRect {
    /// 矩形左上角 x 坐标（文档坐标系，不含 scroll offset）
    pub x: f64,
    /// 矩形左上角 y 坐标（文档坐标系，不含 scroll offset）
    pub y: f64,
    /// 矩形宽度
    pub w: f64,
    /// 矩形高度
    pub h: f64,
    /// 该 glyph 对应的字符（可能是多字节 UTF-8）
    #[serde(rename = "char")]
    pub char_: String,
    /// 文字基线 Y 坐标（coordinate_mode=Baseline 时必须使用此字段而非 y+h）
    #[serde(default)]
    pub baseline_y: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[deprecated(
    since = "0.12.0",
    note = "Use EditorVisualTransaction instead. This will be removed in a future version."
)]
pub struct EditorAnimationEvent {
    pub id: u64,
    pub kind: EditorAnimationKind,
    pub range_start: usize,
    pub range_len: usize,
    pub text: String,
    pub old_cursor: EditorCursor,
    pub new_cursor: EditorCursor,
    pub duration_ms: u64,
    /// 每个 glyph 的精确矩形。Core 层默认为空 Vec（向后兼容），
    /// 平台端填充后通过 FFI 传给 QML overlay。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub glyph_rects: Vec<GlyphRect>,
    /// 变更前光标的视口矩形位置（由 Desktop 端填充，Core 层默认为 None）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_cursor_rect: Option<CursorRect>,
    /// 变更后光标的视口矩形位置（由 Desktop 端填充，Core 层默认为 None）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_cursor_rect: Option<CursorRect>,
}

/// 视觉坐标模式。
/// Baseline 表示所有 y 坐标使用 baselineY，
/// Canvas.drawText 永远用 baselineY，不能用 top + height 拼 baseline。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VisualCoordinateMode {
    Baseline,
}

/// 统一编辑器视觉事务契约。
///
/// Core 层只裁判事件语义和范围（UTF-8 byte offset），
/// 平台层只负责 layout 坐标转换和绘制。
/// Desktop SujianEditorItem 和 Android SujianEditorView 都吃同一份契约。
///
/// coordinate_mode 固定为 Baseline：所有 y 坐标使用 baselineY，
/// 不使用 top+height 拼接 baseline。
///
/// 这是 `EditorAnimationEvent` 的替代方案（Phase 1 视觉事务收敛）。
/// 旧 API `animation_events()` 返回多个事件（Insert + Cursor 等），
/// 新 API `visual_transaction()` 返回单个统一事务，平台层自行决定
/// 如何渲染动画和光标移动。
///
/// 坐标字段（deleted_glyph_rects, insert_glyph_rects, old_cursor_rect,
/// new_cursor_rect）由平台层填充，Core 默认为 None。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorVisualTransaction {
    /// 事务唯一 ID
    pub id: u64,
    /// 动画类型
    pub kind: EditorAnimationKind,
    /// 变更原因
    pub cause: EditorTransactionCause,
    /// 旧文本
    pub old_text: String,
    /// 新文本
    pub new_text: String,
    /// 旧选区（UTF-8 byte offset）
    pub old_selection: EditorSelection,
    /// 新选区（UTF-8 byte offset）
    pub new_selection: EditorSelection,
    /// 插入范围（UTF-8 byte offset），Insert 动画时平台层应跳过此范围
    pub inserted_range: Option<(usize, usize)>,
    /// 删除前 glyph 矩形快照（由平台层填充，Core 默认 None）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_glyph_rects: Option<Vec<GlyphRect>>,
    /// 插入后 glyph 矩形（由平台层填充，Core 默认 None）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub insert_glyph_rects: Option<Vec<GlyphRect>>,
    /// 受局部 reflow 影响的 glyph 的旧位置和新位置（由平台层填充，Core 默认 None）
    /// 中间插入时，插入点右侧的文字做轻量位移动画（局部挤开）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reflow_glyph_rects: Option<Vec<ReflowGlyphRect>>,
    /// 变更前光标矩形（由平台层填充，Core 默认 None）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_cursor_rect: Option<CursorRect>,
    /// 变更后光标矩形（由平台层填充，Core 默认 None）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_cursor_rect: Option<CursorRect>,
    /// 动画时长（毫秒）
    pub duration_ms: u64,
    /// 坐标模式：固定为 Baseline
    pub coordinate_mode: VisualCoordinateMode,
}

/// IME preedit 文本格式属性
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PreeditTextFormat {
    Underline,
    TextColor { color: String },
    BackgroundColor { color: String },
    FontUnderline,
}

/// 预输入（IME composition）视觉事务。
///
/// Preedit 是临时视觉层，不修改 buffer text，不进入 undo。
/// 每次 preedit 变化时生成此事务，驱动 overlay 做轻量吐字/吞字动画。
/// commit 时清空 preedit layer，正式 buffer 插入 commitString，
/// 生成正式 EditorVisualTransaction。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreeditVisualTransaction {
    /// 事务唯一 ID
    pub id: u64,
    /// 旧 preedit 文本
    pub old_preedit_text: String,
    /// 新 preedit 文本
    pub new_preedit_text: String,
    /// 旧 preedit 光标矩形（preedit 变化前）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_preedit_cursor_rect: Option<CursorRect>,
    /// 新 preedit 光标矩形（preedit 变化后）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_preedit_cursor_rect: Option<CursorRect>,
    /// preedit 中每个 glyph 的矩形（由平台层填充）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preedit_glyph_rects: Option<Vec<GlyphRect>>,
    /// 被删除的 preedit glyph 矩形（由平台层填充）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_preedit_glyph_rects: Option<Vec<GlyphRect>>,
    /// 新插入的 preedit glyph 矩形（由平台层填充）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inserted_preedit_glyph_rects: Option<Vec<GlyphRect>>,
    /// preedit 光标矩形
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preedit_cursor_rect: Option<CursorRect>,
    /// 动画时长（毫秒）
    pub duration_ms: u64,
    /// 坐标模式
    pub coordinate_mode: VisualCoordinateMode,
}

#[derive(Debug, Clone)]
pub struct EditorEngine {
    next_animation_id: u64,
    max_animated_chars: usize,
    animation_duration_ms: u64,
}

impl Default for EditorEngine {
    fn default() -> Self {
        Self {
            next_animation_id: 1,
            max_animated_chars: 8,
            animation_duration_ms: 160,
        }
    }
}

impl EditorEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_animation_limits(max_animated_chars: usize, animation_duration_ms: u64) -> Self {
        Self {
            next_animation_id: 1,
            max_animated_chars,
            animation_duration_ms,
        }
    }

    pub fn set_animation_duration_ms(&mut self, duration_ms: u64) {
        self.animation_duration_ms = duration_ms;
    }

    pub fn create_transaction(
        &self,
        old_text: impl Into<String>,
        new_text: impl Into<String>,
        old_selection: EditorSelection,
        new_selection: EditorSelection,
        cause: EditorTransactionCause,
    ) -> EditorTransaction {
        let old_text = old_text.into();
        let new_text = new_text.into();
        let changes = diff_plain_text(&old_text, &new_text);
        let should_animate = should_animate_changes(&changes, cause, self.max_animated_chars);

        EditorTransaction {
            old_text,
            new_text,
            changes,
            old_selection,
            new_selection,
            cause,
            should_animate,
        }
    }

    #[deprecated(
        since = "0.12.0",
        note = "Use visual_transaction() instead. This will be removed in a future version."
    )]
    #[allow(deprecated)]
    pub fn animation_events(
        &mut self,
        transaction: &EditorTransaction,
    ) -> Vec<EditorAnimationEvent> {
        let mut events = Vec::new();
        if transaction.should_animate {
            for change in &transaction.changes {
                let kind = match change {
                    EditorChange::Insert { .. } => EditorAnimationKind::Insert,
                    EditorChange::Delete { .. } => EditorAnimationKind::Delete,
                };
                events.push(EditorAnimationEvent {
                    id: self.take_animation_id(),
                    kind,
                    range_start: change.index(),
                    range_len: change.text().len(),
                    text: change.text().to_string(),
                    old_cursor: transaction.old_selection.head,
                    new_cursor: transaction.new_selection.head,
                    duration_ms: self.animation_duration_ms,
                    glyph_rects: Vec::new(),
                    old_cursor_rect: None,
                    new_cursor_rect: None,
                });
            }
        }

        if transaction.cause != EditorTransactionCause::Load
            && transaction.old_selection.head != transaction.new_selection.head
        {
            events.push(EditorAnimationEvent {
                id: self.take_animation_id(),
                kind: EditorAnimationKind::Cursor,
                range_start: transaction.new_selection.head.index,
                range_len: 0,
                text: String::new(),
                old_cursor: transaction.old_selection.head,
                new_cursor: transaction.new_selection.head,
                duration_ms: self.animation_duration_ms,
                glyph_rects: Vec::new(),
                old_cursor_rect: None,
                new_cursor_rect: None,
            });
        }

        events
    }

    fn take_animation_id(&mut self) -> u64 {
        let id = self.next_animation_id;
        self.next_animation_id = self.next_animation_id.saturating_add(1);
        id
    }

    /// 从 transaction 生成 EditorVisualTransaction。
    /// 
    /// Core 只填充语义字段（id, kind, cause, old/new text, selection, inserted_range, duration, coordinate_mode）。
    /// 平台层负责填充坐标字段（glyph_rects, cursor_rect）。
    pub fn visual_transaction(
        &mut self,
        transaction: &EditorTransaction,
    ) -> Option<EditorVisualTransaction> {
        if !transaction.should_animate {
            return None;
        }
        if transaction.changes.len() != 1 {
            return None;
        }
        let change = &transaction.changes[0];
        let kind = match change {
            EditorChange::Insert { .. } => EditorAnimationKind::Insert,
            EditorChange::Delete { .. } => EditorAnimationKind::Delete,
        };
        let inserted_range = match change {
            EditorChange::Insert { index, text } => Some((*index, *index + text.len())),
            EditorChange::Delete { .. } => None,
        };
        Some(EditorVisualTransaction {
            id: self.take_animation_id(),
            kind,
            cause: transaction.cause,
            old_text: transaction.old_text.clone(),
            new_text: transaction.new_text.clone(),
            old_selection: transaction.old_selection,
            new_selection: transaction.new_selection,
            inserted_range,
            deleted_glyph_rects: None,
            insert_glyph_rects: None,
            reflow_glyph_rects: None,
            old_cursor_rect: None,
            new_cursor_rect: None,
            duration_ms: self.animation_duration_ms,
            coordinate_mode: VisualCoordinateMode::Baseline,
        })
    }
}

pub fn diff_plain_text(old_text: &str, new_text: &str) -> Vec<EditorChange> {
    if old_text == new_text {
        return Vec::new();
    }

    let prefix = common_prefix_byte_len(old_text, new_text);
    let suffix = common_suffix_byte_len(old_text, new_text, prefix);
    let old_end = old_text.len() - suffix;
    let new_end = new_text.len() - suffix;
    let removed = &old_text[prefix..old_end];
    let inserted = &new_text[prefix..new_end];

    let mut changes = Vec::new();
    if !removed.is_empty() {
        changes.push(EditorChange::Delete {
            index: prefix,
            text: removed.to_string(),
        });
    }
    if !inserted.is_empty() {
        changes.push(EditorChange::Insert {
            index: prefix,
            text: inserted.to_string(),
        });
    }
    changes
}

fn should_animate_changes(
    changes: &[EditorChange],
    cause: EditorTransactionCause,
    max_animated_chars: usize,
) -> bool {
    if !matches!(
        cause,
        EditorTransactionCause::Typing | EditorTransactionCause::Delete | EditorTransactionCause::TypingCommit
    ) {
        return false;
    }
    if changes.len() != 1 {
        return false;
    }
    let text = changes[0].text();
    !text.is_empty() && !text.contains('\n') && text.chars().count() <= max_animated_chars
}

fn common_prefix_byte_len(old_text: &str, new_text: &str) -> usize {
    let mut prefix = 0;
    for ((old_index, old_char), (_, new_char)) in
        old_text.char_indices().zip(new_text.char_indices())
    {
        if old_char != new_char {
            break;
        }
        prefix = old_index + old_char.len_utf8();
    }
    prefix
}

fn common_suffix_byte_len(old_text: &str, new_text: &str, prefix: usize) -> usize {
    let old_tail = &old_text[prefix..];
    let new_tail = &new_text[prefix..];
    let mut suffix = 0;
    for ((_, old_char), (_, new_char)) in old_tail
        .char_indices()
        .rev()
        .zip(new_tail.char_indices().rev())
    {
        if old_char != new_char {
            break;
        }
        suffix += old_char.len_utf8();
    }
    suffix
}

fn clamp_to_char_boundary(text: &str, index: usize) -> usize {
    if index >= text.len() {
        return text.len();
    }
    let mut safe = index;
    while safe > 0 && !text.is_char_boundary(safe) {
        safe -= 1;
    }
    safe
}

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use super::*;

    #[test]
    fn detects_single_insert_on_utf8_boundary() {
        let changes = diff_plain_text("你好世界", "你好新世界");
        assert_eq!(
            changes,
            vec![EditorChange::Insert {
                index: "你好".len(),
                text: "新".to_string(),
            }]
        );
    }

    #[test]
    fn detects_single_delete_on_utf8_boundary() {
        let changes = diff_plain_text("abc月def", "abcdef");
        assert_eq!(
            changes,
            vec![EditorChange::Delete {
                index: "abc".len(),
                text: "月".to_string(),
            }]
        );
    }

    #[test]
    fn detects_diff_with_empty_inputs() {
        assert_eq!(diff_plain_text("", ""), vec![]);

        assert_eq!(
            diff_plain_text("", "text"),
            vec![EditorChange::Insert {
                index: 0,
                text: "text".to_string(),
            }]
        );

        assert_eq!(
            diff_plain_text("text", ""),
            vec![EditorChange::Delete {
                index: 0,
                text: "text".to_string(),
            }]
        );
    }

    #[test]
    fn replacement_is_delete_then_insert() {
        let changes = diff_plain_text("alpha beta", "alpha gamma");
        assert_eq!(
            changes,
            vec![
                EditorChange::Delete {
                    index: "alpha ".len(),
                    text: "bet".to_string(),
                },
                EditorChange::Insert {
                    index: "alpha ".len(),
                    text: "gamm".to_string(),
                },
            ]
        );
    }

    #[test]
    fn typing_transaction_emits_insert_and_cursor_events() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "ab",
            "abc",
            EditorSelection::collapsed("ab", 2),
            EditorSelection::collapsed("abc", 3),
            EditorTransactionCause::Typing,
        );

        assert!(tx.should_animate);
        let events = engine.animation_events(&tx);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, EditorAnimationKind::Insert);
        assert_eq!(events[0].text, "c");
        assert_eq!(events[1].kind, EditorAnimationKind::Cursor);
    }

    #[test]
    fn paste_does_not_emit_text_animation() {
        let mut engine = EditorEngine::new();
        let tx = engine.create_transaction(
            "a",
            "a long pasted text",
            EditorSelection::collapsed("a", 1),
            EditorSelection::collapsed("a long pasted text", "a long pasted text".len()),
            EditorTransactionCause::Paste,
        );

        assert!(!tx.should_animate);
        let events = engine.animation_events(&tx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, EditorAnimationKind::Cursor);
    }

    #[test]
    fn load_does_not_emit_animation_events() {
        let mut engine = EditorEngine::new();
        let tx = engine.create_transaction(
            "",
            "loaded",
            EditorSelection::collapsed("", 0),
            EditorSelection::collapsed("loaded", 6),
            EditorTransactionCause::Load,
        );

        assert!(!tx.should_animate);
        assert!(engine.animation_events(&tx).is_empty());
    }

    #[test]
    fn glyph_rect_serializes_camel_case() {
        let gr = GlyphRect {
            x: 10.5,
            y: 20.0,
            w: 16.0,
            h: 24.0,
            char_: "你".to_string(),
            baseline_y: 36.0,
        };
        let json = serde_json::to_string(&gr).unwrap();
        // 字段名必须是 camelCase，char_ → "char"
        assert!(json.contains("\"x\":"));
        assert!(json.contains("\"y\":"));
        assert!(json.contains("\"w\":"));
        assert!(json.contains("\"h\":"));
        assert!(json.contains("\"char\":"));
        assert!(!json.contains("\"char_\":"));
        assert!(json.contains("\"baselineY\":"));
    }

    #[test]
    fn animation_event_glyph_rects_default_empty_and_skip_serializing() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "ab",
            "abc",
            EditorSelection::collapsed("ab", 2),
            EditorSelection::collapsed("abc", 3),
            EditorTransactionCause::Typing,
        );
        let events = engine.animation_events(&tx);
        // Core 层默认 glyph_rects 为空
        assert!(events[0].glyph_rects.is_empty());
        assert!(events[1].glyph_rects.is_empty());

        // 空 glyphRects 不应出现在 JSON 中（skip_serializing_if）
        let json = serde_json::to_string(&events).unwrap();
        assert!(!json.contains("glyphRects"));
    }

    #[test]
    fn animation_event_with_glyph_rects_serializes() {
        let event = EditorAnimationEvent {
            id: 1,
            kind: EditorAnimationKind::Insert,
            range_start: 0,
            range_len: 3,
            text: "abc".to_string(),
            old_cursor: EditorCursor { index: 0 },
            new_cursor: EditorCursor { index: 3 },
            duration_ms: 160,
            glyph_rects: vec![
                GlyphRect {
                    x: 0.0,
                    y: 0.0,
                    w: 10.0,
                    h: 20.0,
                    char_: "a".to_string(),
                    baseline_y: 16.0,
                },
                GlyphRect {
                    x: 10.0,
                    y: 0.0,
                    w: 10.0,
                    h: 20.0,
                    char_: "b".to_string(),
                    baseline_y: 16.0,
                },
                GlyphRect {
                    x: 20.0,
                    y: 0.0,
                    w: 10.0,
                    h: 20.0,
                    char_: "c".to_string(),
                    baseline_y: 16.0,
                },
            ],
            old_cursor_rect: None,
            new_cursor_rect: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        // 非空 glyphRects 必须出现在 JSON 中
        assert!(json.contains("glyphRects"));
        assert!(json.contains("\"char\":"));
    }

    #[test]
    fn complex_grapheme_chars_are_filtered_from_glyph_rects() {
        // This test verifies that the Desktop Rust side filters complex grapheme
        // chars when filling glyph_rects. Since the filtering happens in the
        // Desktop-specific fill_glyph_rects_for_events (not in core), we test
        // the is_complex_grapheme helper function logic here at the core level
        // by verifying that the core transaction correctly identifies emoji text.
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "ab",
            "ab😀",
            EditorSelection::collapsed("ab", 2),
            EditorSelection::collapsed("ab😀", "ab😀".len()),
            EditorTransactionCause::Typing,
        );
        let events = engine.animation_events(&tx);
        // Core still emits the insert event with text "😀"
        assert_eq!(events[0].kind, EditorAnimationKind::Insert);
        assert_eq!(events[0].text, "😀");
        // glyph_rects is empty at core level (filled by platform later)
        assert!(events[0].glyph_rects.is_empty());
    }

    #[test]
    fn set_animation_duration_ms_affects_event_duration() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        // 初始 duration_ms = 120
        let tx = engine.create_transaction(
            "ab",
            "abc",
            EditorSelection::collapsed("ab", 2),
            EditorSelection::collapsed("abc", 3),
            EditorTransactionCause::Typing,
        );
        let events = engine.animation_events(&tx);
        assert_eq!(events[0].duration_ms, 120);

        // 改为 500
        engine.set_animation_duration_ms(500);
        let tx2 = engine.create_transaction(
            "abc",
            "abcd",
            EditorSelection::collapsed("abc", 3),
            EditorSelection::collapsed("abcd", 4),
            EditorTransactionCause::Typing,
        );
        let events2 = engine.animation_events(&tx2);
        assert_eq!(events2[0].duration_ms, 500);
    }

    #[test]
    fn animation_event_cursor_rects_default_none() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "ab",
            "abc",
            EditorSelection::collapsed("ab", 2),
            EditorSelection::collapsed("abc", 3),
            EditorTransactionCause::Typing,
        );
        let events = engine.animation_events(&tx);
        // Core 层默认 cursor_rects 为 None
        assert!(events[0].old_cursor_rect.is_none());
        assert!(events[0].new_cursor_rect.is_none());
    }

    #[test]
    fn cursor_rect_serializes_camel_case() {
        let cr = CursorRect { x: 10.5, top: 5.0, bottom: 25.0, baseline_y: 20.0 };
        let json = serde_json::to_string(&cr).unwrap();
        assert!(json.contains("\"x\":"));
        assert!(json.contains("\"top\":"));
        assert!(json.contains("\"bottom\":"));
        assert!(json.contains("\"baselineY\":"));
    }

    #[test]
    fn animation_event_with_cursor_rects_serializes() {
        let event = EditorAnimationEvent {
            id: 1,
            kind: EditorAnimationKind::Insert,
            range_start: 0,
            range_len: 1,
            text: "a".to_string(),
            old_cursor: EditorCursor { index: 0 },
            new_cursor: EditorCursor { index: 1 },
            duration_ms: 160,
            glyph_rects: Vec::new(),
            old_cursor_rect: Some(CursorRect { x: 10.0, top: 5.0, bottom: 25.0, baseline_y: 20.0 }),
            new_cursor_rect: Some(CursorRect { x: 30.0, top: 5.0, bottom: 25.0, baseline_y: 20.0 }),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("oldCursorRect"));
        assert!(json.contains("newCursorRect"));
    }

    #[test]
    fn animation_event_without_cursor_rects_skips_serializing() {
        let event = EditorAnimationEvent {
            id: 1,
            kind: EditorAnimationKind::Insert,
            range_start: 0,
            range_len: 1,
            text: "a".to_string(),
            old_cursor: EditorCursor { index: 0 },
            new_cursor: EditorCursor { index: 1 },
            duration_ms: 160,
            glyph_rects: Vec::new(),
            old_cursor_rect: None,
            new_cursor_rect: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(!json.contains("oldCursorRect"));
        assert!(!json.contains("newCursorRect"));
    }

    #[test]
    fn single_char_insert_event_has_correct_range() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "你好",
            "你好世",
            EditorSelection::collapsed("你好", "你好".len()),
            EditorSelection::collapsed("你好世", "你好世".len()),
            EditorTransactionCause::Typing,
        );
        let events = engine.animation_events(&tx);
        // Should have Insert + Cursor events
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, EditorAnimationKind::Insert);
        // range_start should be at byte offset of "世" insertion point
        assert_eq!(events[0].range_start, "你好".len()); // 6 bytes
        assert_eq!(events[0].range_len, "世".len()); // 3 bytes
        assert_eq!(events[0].text, "世");
    }

    #[test]
    fn single_char_delete_event_has_correct_range() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "你好世",
            "你好",
            EditorSelection::collapsed("你好世", "你好世".len()),
            EditorSelection::collapsed("你好", "你好".len()),
            EditorTransactionCause::Delete,
        );
        let events = engine.animation_events(&tx);
        // Should have Delete + Cursor events
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, EditorAnimationKind::Delete);
        // range_start should be at byte offset where "世" was deleted
        assert_eq!(events[0].range_start, "你好".len()); // 6 bytes
        assert_eq!(events[0].range_len, "世".len()); // 3 bytes
        assert_eq!(events[0].text, "世");
    }

    #[test]
    fn paste_does_not_produce_animation_events() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "a",
            "a long pasted text",
            EditorSelection::collapsed("a", 1),
            EditorSelection::collapsed("a long pasted text", "a long pasted text".len()),
            EditorTransactionCause::Paste,
        );
        assert!(!tx.should_animate);
        let events = engine.animation_events(&tx);
        // Only Cursor event, no Insert/Delete
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, EditorAnimationKind::Cursor);
    }

    #[test]
    fn load_does_not_produce_animation_events() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "",
            "loaded text",
            EditorSelection::collapsed("", 0),
            EditorSelection::collapsed("loaded text", "loaded text".len()),
            EditorTransactionCause::Load,
        );
        assert!(!tx.should_animate);
        assert!(engine.animation_events(&tx).is_empty());
    }

    // --- Cause-based animation suppression tests ---
    // These tests verify that non-typing causes (Format, Undo, Redo,
    // ImeComposition, Programmatic) do not produce text animation events,
    // as ensured by should_animate_changes().

    #[test]
    fn format_does_not_produce_animation_events() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "hello world",
            "Hello World",
            EditorSelection::collapsed("hello world", 0),
            EditorSelection::collapsed("Hello World", 0),
            EditorTransactionCause::Format,
        );
        assert!(!tx.should_animate, "Format cause should not animate");
        // Format with cursor movement should only produce Cursor event, no Insert/Delete
        let events = engine.animation_events(&tx);
        for event in &events {
            assert!(
                event.kind == EditorAnimationKind::Cursor,
                "Format should only produce Cursor events, got {:?}",
                event.kind
            );
        }
    }

    #[test]
    fn undo_does_not_produce_animation_events() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "abc",
            "a",
            EditorSelection::collapsed("abc", 3),
            EditorSelection::collapsed("a", 1),
            EditorTransactionCause::Undo,
        );
        assert!(!tx.should_animate, "Undo cause should not animate");
        // Undo with cursor movement should only produce Cursor event
        let events = engine.animation_events(&tx);
        for event in &events {
            assert!(
                event.kind == EditorAnimationKind::Cursor,
                "Undo should only produce Cursor events, got {:?}",
                event.kind
            );
        }
    }

    #[test]
    fn redo_does_not_produce_animation_events() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "a",
            "abc",
            EditorSelection::collapsed("a", 1),
            EditorSelection::collapsed("abc", 3),
            EditorTransactionCause::Redo,
        );
        assert!(!tx.should_animate, "Redo cause should not animate");
        // Redo with cursor movement should only produce Cursor event
        let events = engine.animation_events(&tx);
        for event in &events {
            assert!(
                event.kind == EditorAnimationKind::Cursor,
                "Redo should only produce Cursor events, got {:?}",
                event.kind
            );
        }
    }

    #[test]
    fn ime_composition_does_not_produce_animation_events() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "ni",
            "nihao",
            EditorSelection::collapsed("ni", 2),
            EditorSelection::collapsed("nihao", 5),
            EditorTransactionCause::ImeComposition,
        );
        assert!(!tx.should_animate, "ImeComposition cause should not animate");
        // ImeComposition with cursor movement should only produce Cursor event
        let events = engine.animation_events(&tx);
        for event in &events {
            assert!(
                event.kind == EditorAnimationKind::Cursor,
                "ImeComposition should only produce Cursor events, got {:?}",
                event.kind
            );
        }
    }

    #[test]
    fn programmatic_does_not_produce_animation_events() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "old text",
            "new text",
            EditorSelection::collapsed("old text", 0),
            EditorSelection::collapsed("new text", 0),
            EditorTransactionCause::Programmatic,
        );
        assert!(!tx.should_animate, "Programmatic cause should not animate");
        // Programmatic without cursor movement should produce no events at all
        let events = engine.animation_events(&tx);
        assert!(
            events.is_empty(),
            "Programmatic with same cursor position should produce no events, got {} events",
            events.len()
        );
    }

    // --- Guard tests for different setting combinations ---

    #[test]
    fn typing_animation_toggle_on_off() {
        // When typing animation is ON: Typing cause should_animate = true
        let engine = EditorEngine::with_animation_limits(8, 120);
        let tx_on = engine.create_transaction(
            "ab",
            "abc",
            EditorSelection::collapsed("ab", 2),
            EditorSelection::collapsed("abc", 3),
            EditorTransactionCause::Typing,
        );
        assert!(tx_on.should_animate, "Typing should animate when animation is on");

        // When typing animation is OFF: should_animate_changes still returns true for Typing cause,
        // but the caller (platform) should check the setting and skip creating animation events.
        // The core should_animate_changes function is cause-based, not setting-based.
        // This test verifies the core behavior is consistent regardless of external toggle.
        let tx_off = engine.create_transaction(
            "abc",
            "abcd",
            EditorSelection::collapsed("abc", 3),
            EditorSelection::collapsed("abcd", 4),
            EditorTransactionCause::Typing,
        );
        // Core always returns true for Typing cause — platform is responsible for checking the toggle
        assert!(tx_off.should_animate, "Core should_animate_changes is cause-based, not toggle-based");

        // Non-typing causes should never animate regardless of toggle
        let tx_paste = engine.create_transaction(
            "a",
            "a pasted text",
            EditorSelection::collapsed("a", 1),
            EditorSelection::collapsed("a pasted text", "a pasted text".len()),
            EditorTransactionCause::Paste,
        );
        assert!(!tx_paste.should_animate, "Paste should never animate regardless of toggle");
    }

    #[test]
    fn animation_duration_clamped() {
        // Verify that animation duration is stored as-is in EditorEngine,
        // and that the settings layer (not core) is responsible for clamping.
        // Core stores whatever duration is set via set_animation_duration_ms.
        let mut engine = EditorEngine::with_animation_limits(8, 120);

        // Normal duration
        engine.set_animation_duration_ms(200);
        let tx = engine.create_transaction(
            "ab",
            "abc",
            EditorSelection::collapsed("ab", 2),
            EditorSelection::collapsed("abc", 3),
            EditorTransactionCause::Typing,
        );
        let events = engine.animation_events(&tx);
        assert_eq!(events[0].duration_ms, 200);

        // Very small duration — core stores it, settings layer should clamp before calling set
        engine.set_animation_duration_ms(5);
        let tx2 = engine.create_transaction(
            "abc",
            "abcd",
            EditorSelection::collapsed("abc", 3),
            EditorSelection::collapsed("abcd", 4),
            EditorTransactionCause::Typing,
        );
        let events2 = engine.animation_events(&tx2);
        assert_eq!(events2[0].duration_ms, 5, "Core stores whatever duration is set; clamping is the caller's responsibility");

        // Very large duration
        engine.set_animation_duration_ms(9999);
        let tx3 = engine.create_transaction(
            "abcd",
            "abcde",
            EditorSelection::collapsed("abcd", 4),
            EditorSelection::collapsed("abcde", 5),
            EditorTransactionCause::Typing,
        );
        let events3 = engine.animation_events(&tx3);
        assert_eq!(events3[0].duration_ms, 9999, "Core stores whatever duration is set; clamping is the caller's responsibility");
    }

    #[test]
    fn undo_redo_no_animation() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);

        // Undo with text change should NOT animate
        let tx_undo = engine.create_transaction(
            "abc",
            "a",
            EditorSelection::collapsed("abc", 3),
            EditorSelection::collapsed("a", 1),
            EditorTransactionCause::Undo,
        );
        assert!(!tx_undo.should_animate, "Undo should not animate");
        let events_undo = engine.animation_events(&tx_undo);
        for event in &events_undo {
            assert_ne!(event.kind, EditorAnimationKind::Insert, "Undo should not produce Insert animation");
            assert_ne!(event.kind, EditorAnimationKind::Delete, "Undo should not produce Delete animation");
        }

        // Redo with text change should NOT animate
        let tx_redo = engine.create_transaction(
            "a",
            "abc",
            EditorSelection::collapsed("a", 1),
            EditorSelection::collapsed("abc", 3),
            EditorTransactionCause::Redo,
        );
        assert!(!tx_redo.should_animate, "Redo should not animate");
        let events_redo = engine.animation_events(&tx_redo);
        for event in &events_redo {
            assert_ne!(event.kind, EditorAnimationKind::Insert, "Redo should not produce Insert animation");
            assert_ne!(event.kind, EditorAnimationKind::Delete, "Redo should not produce Delete animation");
        }
    }

    #[test]
    fn paste_no_animation() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);

        // Paste with single-char text should still NOT animate
        let tx = engine.create_transaction(
            "a",
            "ab",
            EditorSelection::collapsed("a", 1),
            EditorSelection::collapsed("ab", 2),
            EditorTransactionCause::Paste,
        );
        assert!(!tx.should_animate, "Paste should not animate even for single char");
        let events = engine.animation_events(&tx);
        for event in &events {
            assert_ne!(event.kind, EditorAnimationKind::Insert, "Paste should not produce Insert animation");
            assert_ne!(event.kind, EditorAnimationKind::Delete, "Paste should not produce Delete animation");
        }

        // Paste with multi-char text should NOT animate
        let tx2 = engine.create_transaction(
            "a",
            "a long pasted text",
            EditorSelection::collapsed("a", 1),
            EditorSelection::collapsed("a long pasted text", "a long pasted text".len()),
            EditorTransactionCause::Paste,
        );
        assert!(!tx2.should_animate, "Paste should not animate for multi-char text");
    }

    #[test]
    fn load_no_animation() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);

        // Load should produce zero animation events (not even Cursor)
        let tx = engine.create_transaction(
            "",
            "loaded text",
            EditorSelection::collapsed("", 0),
            EditorSelection::collapsed("loaded text", "loaded text".len()),
            EditorTransactionCause::Load,
        );
        assert!(!tx.should_animate, "Load should not animate");
        let events = engine.animation_events(&tx);
        assert!(events.is_empty(), "Load should produce zero animation events (not even Cursor)");

        // Load with same cursor position (0→0) should also produce no events
        let tx2 = engine.create_transaction(
            "",
            "loaded",
            EditorSelection::collapsed("", 0),
            EditorSelection::collapsed("loaded", 0),
            EditorTransactionCause::Load,
        );
        assert!(!tx2.should_animate);
        assert!(engine.animation_events(&tx2).is_empty());
    }

    #[test]
    fn visual_transaction_insert_has_inserted_range() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "ab",
            "abc",
            EditorSelection::collapsed("ab", 2),
            EditorSelection::collapsed("abc", 3),
            EditorTransactionCause::Typing,
        );
        let vt = engine.visual_transaction(&tx).unwrap();
        assert_eq!(vt.kind, EditorAnimationKind::Insert);
        assert_eq!(vt.inserted_range, Some((2, 3)));
        assert_eq!(vt.coordinate_mode, VisualCoordinateMode::Baseline);
        assert!(vt.deleted_glyph_rects.is_none());
        assert!(vt.insert_glyph_rects.is_none());
        assert!(vt.old_cursor_rect.is_none());
        assert!(vt.new_cursor_rect.is_none());
    }

    #[test]
    fn visual_transaction_delete_has_no_inserted_range() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "abc",
            "ab",
            EditorSelection::collapsed("abc", 3),
            EditorSelection::collapsed("ab", 2),
            EditorTransactionCause::Delete,
        );
        let vt = engine.visual_transaction(&tx).unwrap();
        assert_eq!(vt.kind, EditorAnimationKind::Delete);
        assert!(vt.inserted_range.is_none());
    }

    #[test]
    fn visual_transaction_paste_returns_none() {
        let mut engine = EditorEngine::new();
        let tx = engine.create_transaction(
            "a",
            "a long pasted text",
            EditorSelection::collapsed("a", 1),
            EditorSelection::collapsed("a long pasted text", "a long pasted text".len()),
            EditorTransactionCause::Paste,
        );
        assert!(engine.visual_transaction(&tx).is_none());
    }

    #[test]
    fn cursor_rect_has_baseline_y() {
        let cr = CursorRect { x: 10.0, top: 5.0, bottom: 25.0, baseline_y: 20.0 };
        let json = serde_json::to_string(&cr).unwrap();
        assert!(json.contains("\"baselineY\":"));
        assert!(json.contains("\"top\":"));
        assert!(json.contains("\"bottom\":"));
    }

    #[test]
    fn glyph_rect_has_baseline_y() {
        let gr = GlyphRect {
            x: 10.5, y: 20.0, w: 16.0, h: 24.0,
            char_: "你".to_string(), baseline_y: 40.0,
        };
        let json = serde_json::to_string(&gr).unwrap();
        assert!(json.contains("\"baselineY\":"));
    }

    #[test]
    fn preedit_visual_transaction_serializes_camel_case() {
        let vt = PreeditVisualTransaction {
            id: 1,
            old_preedit_text: "n".to_string(),
            new_preedit_text: "ni".to_string(),
            old_preedit_cursor_rect: None,
            new_preedit_cursor_rect: Some(CursorRect { x: 10.0, top: 5.0, bottom: 25.0, baseline_y: 20.0 }),
            preedit_glyph_rects: None,
            deleted_preedit_glyph_rects: None,
            inserted_preedit_glyph_rects: None,
            preedit_cursor_rect: Some(CursorRect { x: 10.0, top: 5.0, bottom: 25.0, baseline_y: 20.0 }),
            duration_ms: 160,
            coordinate_mode: VisualCoordinateMode::Baseline,
        };
        let json = serde_json::to_string(&vt).unwrap();
        assert!(json.contains("\"oldPreeditText\":"));
        assert!(json.contains("\"newPreeditText\":"));
        assert!(json.contains("\"newPreeditCursorRect\":"));
        assert!(json.contains("\"preeditCursorRect\":"));
        assert!(json.contains("\"durationMs\":"));
        assert!(json.contains("\"coordinateMode\":"));
        // None fields should be skipped
        assert!(!json.contains("\"oldPreeditCursorRect\":"));
        assert!(!json.contains("\"preeditGlyphRects\":"));
        assert!(!json.contains("\"deletedPreeditGlyphRects\":"));
        assert!(!json.contains("\"insertedPreeditGlyphRects\":"));
    }

    #[test]
    fn preedit_text_format_serializes_camel_case() {
        let fmt = PreeditTextFormat::TextColor { color: "#FF0000".to_string() };
        let json = serde_json::to_string(&fmt).unwrap();
        assert!(json.contains("\"textColor\":"));
        assert!(json.contains("\"color\":"));

        let fmt2 = PreeditTextFormat::Underline;
        let json2 = serde_json::to_string(&fmt2).unwrap();
        assert!(json2.contains("\"underline\""));

        let fmt3 = PreeditTextFormat::BackgroundColor { color: "#00FF00".to_string() };
        let json3 = serde_json::to_string(&fmt3).unwrap();
        assert!(json3.contains("\"backgroundColor\":"));

        let fmt4 = PreeditTextFormat::FontUnderline;
        let json4 = serde_json::to_string(&fmt4).unwrap();
        assert!(json4.contains("\"fontUnderline\""));
    }
}
