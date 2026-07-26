//! Editor animation DTOs for cross-platform FFI.
//!
//! These DTOs mirror the Core `editor::transaction` types but are
//! stable API boundary types used by UniFFI and C-ABI FFI layers.
//! Platform clients (Android, Linux_qt, HarmonyOS) consume these
//! to decide whether and how to animate text changes.
//!
//! 边界契约：
//! - 所有 byte offset / byte range 字段均为 UTF-8 byte offset（半开区间 [start, end)）
//! - 平台端 UTF-16 index 只存在于平台 TextIndexMap 内，传入 Core 前必须转换
//! - DTO 的 u32 字段对应 Kotlin ULong/Int——平台端需注意溢出和符号
//! - `has_inserted_range` / `has_deleted_range` 必须先检查，不能依赖 start/end == 0 判断
//!   因为 0..0 在 Kotlin IntRange 中包含一个元素（0），不是空范围

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum AnimationModeDto {
    #[default]
    GlyphAnimation,
    ClusterAnimation,
    RunAnimation,
    LineReflowAnimation,
    /// UNAVAILABLE: No snapshot renderer exists. Core never returns this.
    /// 保留此变体是因为 DTO 与 Core 枚举一一对应，保证前向兼容——
    /// 未来实现 snapshot renderer 后无需新增变体或修改序列化格式。
    SnapshotAnimation,
    SystemSuppressed,
}

impl From<crate::editor::AnimationMode> for AnimationModeDto {
    fn from(mode: crate::editor::AnimationMode) -> Self {
        match mode {
            crate::editor::AnimationMode::GlyphAnimation => Self::GlyphAnimation,
            crate::editor::AnimationMode::ClusterAnimation => Self::ClusterAnimation,
            crate::editor::AnimationMode::RunAnimation => Self::RunAnimation,
            crate::editor::AnimationMode::LineReflowAnimation => Self::LineReflowAnimation,
            crate::editor::AnimationMode::SnapshotAnimation => Self::SnapshotAnimation,
            crate::editor::AnimationMode::SystemSuppressed => Self::SystemSuppressed,
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

impl From<crate::editor::CursorRect> for CursorRectDto {
    fn from(r: crate::editor::CursorRect) -> Self {
        Self {
            x: r.x,
            top: r.top,
            bottom: r.bottom,
            baseline_y: r.baseline_y,
        }
    }
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
/// Linux_qt SujianEditorItem 和 Android SujianEditorView 都吃同一份契约。
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
    /// 是否有插入范围 — 平台层必须先检查此标志，不能依赖 inserted_range_start/end == 0 判断
    /// 因为 0..0 在 Kotlin IntRange 中包含一个元素（0），不是空范围。
    /// Delete 事务此值为 false，Insert 事务此值为 true。
    pub has_inserted_range: bool,
    /// 删除范围起始（UTF-8 byte offset），无删除时为 0
    pub deleted_range_start: u32,
    /// 删除范围结束（UTF-8 byte offset），无删除时为 0
    pub deleted_range_end: u32,
    /// 是否有删除范围 — 平台层必须先检查此标志，不能依赖 deleted_range_start/end == 0 判断。
    /// Insert 事务此值为 false，Delete 事务此值为 true。
    /// 消除 Android 端用 0..0 表示空范围的歧义。
    pub has_deleted_range: bool,
    /// Core 决定的动画模式，是平台端唯一语义来源
    pub animation_mode: AnimationModeDto,
    /// 动画时长（毫秒）
    pub duration_ms: u64,
    /// 坐标模式
    pub coordinate_mode: VisualCoordinateModeDto,
}

// SAFETY: UTF-8 byte offset 转 u32 截断安全——文本长度受 u32::MAX 限制，
// 平台端和 Core 均不支持超过 4GB 的单章正文，实际 offset 远小于 u32::MAX。
#[allow(clippy::cast_possible_truncation)]
impl From<crate::editor::EditorVisualTransaction> for EditorVisualTransactionDto {
    fn from(vt: crate::editor::EditorVisualTransaction) -> Self {
        let (inserted_range_start, inserted_range_end) = vt
            .inserted_range
            .map(|(s, e)| (s as u32, e as u32))
            .unwrap_or((0, 0));
        let has_inserted_range = vt.inserted_range.is_some();
        let (deleted_range_start, deleted_range_end) = vt
            .deleted_range
            .map(|(s, e)| (s as u32, e as u32))
            .unwrap_or((0, 0));
        let has_deleted_range = vt.deleted_range.is_some();
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
            has_inserted_range,
            deleted_range_start,
            deleted_range_end,
            has_deleted_range,
            animation_mode: vt.animation_mode.into(),
            duration_ms: vt.duration_ms,
            coordinate_mode: vt.coordinate_mode.into(),
        }
    }
}

// ── #515: Unified Timeline DTO ──

/// 统一时钟 DTO — 文字切片、光标、预输入装饰全部消费同一个 progress。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_visible_frame_time_ms: Option<u64>,
    pub duration_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pause_started_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "u64_is_zero")]
    pub accumulated_paused_duration_ms: u64,
    #[serde(default, skip_serializing_if = "f64_is_zero")]
    pub paused_progress: f64,
}

fn u64_is_zero(v: &u64) -> bool {
    *v == 0
}

fn f64_is_zero(v: &f64) -> bool {
    *v == 0.0
}

impl From<crate::editor::Timeline> for TimelineDto {
    fn from(t: crate::editor::Timeline) -> Self {
        Self {
            first_visible_frame_time_ms: t.first_visible_frame_time_ms,
            duration_ms: t.duration_ms,
            pause_started_at_ms: t.pause_started_at_ms,
            accumulated_paused_duration_ms: t.accumulated_paused_duration_ms,
            paused_progress: t.paused_progress,
        }
    }
}

// ── #515: Unified Transaction Kind DTO ──

/// 统一事务类型 DTO — 最终 Linux 和 Android 只保留四种事务。
///
/// 所有事务共用 VisualRevision、LineSnapshot、AnimatedSlice、
/// StaticLinePatch、DecorationSlice、CursorPath 和 Timeline。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum UnifiedTransactionKindDto {
    #[default]
    BodyEdit,
    CompositionUpdate,
    CompositionCommitOrCancel,
    CursorOnly,
}

impl From<crate::editor::UnifiedTransactionKind> for UnifiedTransactionKindDto {
    fn from(k: crate::editor::UnifiedTransactionKind) -> Self {
        match k {
            crate::editor::UnifiedTransactionKind::BodyEdit => Self::BodyEdit,
            crate::editor::UnifiedTransactionKind::CompositionUpdate => Self::CompositionUpdate,
            crate::editor::UnifiedTransactionKind::CompositionCommitOrCancel => {
                Self::CompositionCommitOrCancel
            }
            crate::editor::UnifiedTransactionKind::CursorOnly => Self::CursorOnly,
        }
    }
}

// ── #515: Visual Class Kind DTO ──

/// 视觉对象分类 DTO — 通过 old/new VisualRevision、OffsetMap 和 shaping identity 分类。
///
/// 中间插入、换行、段落合并和删除回流全部使用这套分类，不建立特例。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum VisualClassKindDto {
    #[default]
    Static,
    Insert,
    Delete,
    Move,
    Crossfade,
}

impl From<crate::editor::VisualClassKind> for VisualClassKindDto {
    fn from(k: crate::editor::VisualClassKind) -> Self {
        match k {
            crate::editor::VisualClassKind::Static => Self::Static,
            crate::editor::VisualClassKind::Insert => Self::Insert,
            crate::editor::VisualClassKind::Delete => Self::Delete,
            crate::editor::VisualClassKind::Move => Self::Move,
            crate::editor::VisualClassKind::Crossfade => Self::Crossfade,
        }
    }
}

// ── #515: Decoration Slice DTO ──

/// 装饰切片类型 DTO — 预输入下划线、分段颜色、背景色和 IME 光标。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum DecorationSliceKindDto {
    #[default]
    Underline,
    TextColor,
    BackgroundColor,
    Cursor,
}

impl From<crate::editor::DecorationSliceKind> for DecorationSliceKindDto {
    fn from(k: crate::editor::DecorationSliceKind) -> Self {
        match k {
            crate::editor::DecorationSliceKind::Underline => Self::Underline,
            crate::editor::DecorationSliceKind::TextColor => Self::TextColor,
            crate::editor::DecorationSliceKind::BackgroundColor => Self::BackgroundColor,
            crate::editor::DecorationSliceKind::Cursor => Self::Cursor,
        }
    }
}

// ── #515: PlatformVisualTransaction DTO ──

/// 跨平台视觉事务 DTO — 包含统一时钟和分类。
///
/// Core 输出 EditorVisualTransaction；平台端收到后，根据平台布局
/// 生成 PlatformVisualTransaction。两端共享此 DTO 和状态机概念，
/// 不共享平台渲染结构。
///
/// `generation` 用于过期检测：平台端持有的 generation 与 Core 当前 generation
/// 不匹配时，后续操作被内核拒绝。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformVisualTransactionDto {
    pub transaction_id: u64,
    pub generation: u64,
    pub state: PlatformVisualTransactionStateDto,
    pub duration_ms: u64,
    pub unified_kind: UnifiedTransactionKindDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeline: Option<TimelineDto>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub visual_class_kinds: Vec<VisualClassKindDto>,
}

/// 事务状态 DTO — 动画时间只能在首次进入 Rendering 时开始。
/// 滚动开始时 Rendering → Paused，滚动结束后 revision 未变则累加
/// pausedDuration 继续，revision 已变则取消失效事务。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum PlatformVisualTransactionStateDto {
    #[default]
    Pending,
    Prepared,
    Rendering,
    Paused,
    Completed,
    Cancelled,
}

impl From<crate::editor::PlatformVisualTransactionState> for PlatformVisualTransactionStateDto {
    fn from(s: crate::editor::PlatformVisualTransactionState) -> Self {
        match s {
            crate::editor::PlatformVisualTransactionState::Pending => Self::Pending,
            crate::editor::PlatformVisualTransactionState::Prepared => Self::Prepared,
            crate::editor::PlatformVisualTransactionState::Rendering => Self::Rendering,
            crate::editor::PlatformVisualTransactionState::Paused => Self::Paused,
            crate::editor::PlatformVisualTransactionState::Completed => Self::Completed,
            crate::editor::PlatformVisualTransactionState::Cancelled => Self::Cancelled,
        }
    }
}

impl From<crate::editor::PlatformVisualTransaction> for PlatformVisualTransactionDto {
    fn from(t: crate::editor::PlatformVisualTransaction) -> Self {
        Self {
            transaction_id: t.transaction_id,
            generation: t.generation,
            state: t.state.into(),
            duration_ms: t.duration_ms,
            unified_kind: t.unified_kind.map(|k| k.into()).unwrap_or_default(),
            timeline: t.timeline.map(Into::into),
            visual_class_kinds: t.visual_class_kinds.into_iter().map(Into::into).collect(),
        }
    }
}

// ── #535: Editor V2 Kernel DTOs ──

/// 操作类型 DTO — 区分不同编辑操作的视觉语义。
/// Core 根据此枚举决定动画策略和光标行为。
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

/// 光标协同 DTO — 描述光标移动的视觉语义。
///
/// `old_byte_offset` / `new_byte_offset` 均为 UTF-8 byte offset。
/// `should_animate` 由 Core 根据动画开关、光标是否实际移动、
/// 是否处于加载/格式化状态综合决定。平台端不得自行判断。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoordinatedCursorDto {
    pub old_byte_offset: u32,
    pub new_byte_offset: u32,
    pub should_animate: bool,
}

// SAFETY: 同上，UTF-8 byte offset 截断安全
#[allow(clippy::cast_possible_truncation)]
impl From<crate::editor::CoordinatedCursor> for CoordinatedCursorDto {
    fn from(c: crate::editor::CoordinatedCursor) -> Self {
        Self {
            old_byte_offset: c.old_offset.value() as u32,
            new_byte_offset: c.new_offset.value() as u32,
            should_animate: c.should_animate,
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
            start: r.start.value() as u32,
            end_exclusive: r.end.value() as u32,
        }
    }
}

/// 视觉意图 DTO — Core 告诉平台层应该做什么动画。
///
/// Core 裁判动画语义（模式、时长、受影响范围）；
/// 平台层只负责 layout 坐标转换和绘制。
///
/// 所有 byte range 均为 UTF-8 byte offset 半开区间 `[start, end_exclusive)`。
/// 平台端 DisplayTextMirror 使用 UTF-16 时必须先通过 TextIndexMap 转换。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorVisualIntentDto {
    pub cause: EditorTransactionCauseDto,
    pub operation_kind: EditorOperationKindDto,
    pub old_affected_byte_ranges: Vec<EditorByteRangeDto>,
    pub new_affected_byte_ranges: Vec<EditorByteRangeDto>,
    pub animation_mode: AnimationModeDto,
    pub duration_ms: u64,
    pub coordinated_cursor: CoordinatedCursorDto,
}

impl EditorVisualIntentDto {
    pub fn default_fallback() -> Self {
        Self {
            cause: EditorTransactionCauseDto::Programmatic,
            operation_kind: EditorOperationKindDto::CursorOnly,
            old_affected_byte_ranges: vec![],
            new_affected_byte_ranges: vec![],
            animation_mode: AnimationModeDto::SystemSuppressed,
            duration_ms: 0,
            coordinated_cursor: CoordinatedCursorDto {
                old_byte_offset: 0,
                new_byte_offset: 0,
                should_animate: false,
            },
        }
    }
}

impl From<crate::editor::EditorVisualIntent> for EditorVisualIntentDto {
    fn from(vi: crate::editor::EditorVisualIntent) -> Self {
        Self {
            cause: match vi.cause {
                crate::editor::EditorTransactionCause::Typing => EditorTransactionCauseDto::Typing,
                crate::editor::EditorTransactionCause::Delete => EditorTransactionCauseDto::Delete,
                crate::editor::EditorTransactionCause::Paste => EditorTransactionCauseDto::Paste,
                crate::editor::EditorTransactionCause::Undo => EditorTransactionCauseDto::Undo,
                crate::editor::EditorTransactionCause::Redo => EditorTransactionCauseDto::Redo,
                crate::editor::EditorTransactionCause::Load => EditorTransactionCauseDto::Load,
                crate::editor::EditorTransactionCause::Format => EditorTransactionCauseDto::Format,
                crate::editor::EditorTransactionCause::ImeComposition => EditorTransactionCauseDto::ImeComposition,
                crate::editor::EditorTransactionCause::TypingCommit => EditorTransactionCauseDto::TypingCommit,
                crate::editor::EditorTransactionCause::Programmatic => EditorTransactionCauseDto::Programmatic,
            },
            operation_kind: vi.operation_kind.into(),
            old_affected_byte_ranges: vi.old_affected_byte_ranges.into_iter().map(Into::into).collect(),
            new_affected_byte_ranges: vi.new_affected_byte_ranges.into_iter().map(Into::into).collect(),
            animation_mode: vi.animation_mode.into(),
            duration_ms: vi.duration_ms,
            coordinated_cursor: vi.coordinated_cursor.into(),
        }
    }
}

/// 显示补丁 DTO — 平台显示镜像增量更新的最小单位。
///
/// `replace_byte_start..replace_byte_end_exclusive` 为半开区间（UTF-8 byte offset），
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
            replace_byte_start: p.replace_byte_range.start.value() as u32,
            replace_byte_end_exclusive: p.replace_byte_range.end.value() as u32,
            inserted_text: p.inserted_text,
            resulting_selection_start: p.resulting_selection_byte_range.start.value() as u32,
            resulting_selection_end: p.resulting_selection_byte_range.end.value() as u32,
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

/// 编辑结果 DTO — EditorKernel.apply() 的跨平台返回值。
///
/// 包含正文变化（display_patches）、选区变化、视觉意图和 composition 会话状态。
/// 平台端按此结果增量更新显示镜像、布局和动画。
///
/// 选区字段 `old_selection_start/end` 和 `new_selection_start/end` 均为
/// UTF-8 byte offset 半开区间 `[start, end)`。平台端使用 UTF-16 时
/// 必须通过 TextIndexMap 转换，不得直接用于 SpannableStringBuilder。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
 pub struct EditorEditResultDto {
    pub outcome: EditorEditOutcomeDto,
    pub transaction_id: u64,
    pub base_revision: u64,
    pub new_revision: u64,
    pub display_patches: Vec<DisplayPatchDto>,
    pub old_selection_start: u32,
    pub old_selection_end: u32,
    pub new_selection_start: u32,
    pub new_selection_end: u32,
    pub visual_intent: EditorVisualIntentDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composition_session: Option<CompositionSessionDto>,
}

impl EditorEditResultDto {
    pub fn stale_fallback() -> Self {
        Self {
            outcome: EditorEditOutcomeDto::StaleRevision,
            transaction_id: 0,
            base_revision: 0,
            new_revision: 0,
            display_patches: vec![],
            old_selection_start: 0,
            old_selection_end: 0,
            new_selection_start: 0,
            new_selection_end: 0,
            visual_intent: EditorVisualIntentDto::default_fallback(),
            composition_session: None,
        }
    }
}

/// 编辑结果分类 DTO — 平台必须区分不同结果走不同恢复路径。
///
/// - `Applied`：编辑成功应用，正文已变更
/// - `AppliedWithAdjustedSelection`：编辑成功，但平台传入的选区 offset 不在 char boundary 上，内核已自动对齐
/// - `NoChange`：命令无实际效果（如空替换、空选区变更），正文未变
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
            crate::editor::EditorEditOutcome::AppliedWithAdjustedSelection(r) => (EditorEditOutcomeDto::AppliedWithAdjustedSelection, r),
            crate::editor::EditorEditOutcome::NoChange(r) => (EditorEditOutcomeDto::NoChange, r),
            crate::editor::EditorEditOutcome::StaleRevision(r) => (EditorEditOutcomeDto::StaleRevision, r),
            crate::editor::EditorEditOutcome::InvalidOffset(r) => (EditorEditOutcomeDto::InvalidOffset, r),
            crate::editor::EditorEditOutcome::InvalidRange(r) => (EditorEditOutcomeDto::InvalidRange, r),
        };
        Self {
            outcome: outcome_dto,
            transaction_id: r.transaction_id,
            base_revision: r.base_revision.value(),
            new_revision: r.new_revision.value(),
            display_patches: r.display_patches.into_iter().map(Into::into).collect(),
            old_selection_start: r.old_selection_byte_range.start.value() as u32,
            old_selection_end: r.old_selection_byte_range.end.value() as u32,
            new_selection_start: r.new_selection_byte_range.start.value() as u32,
            new_selection_end: r.new_selection_byte_range.end.value() as u32,
            visual_intent: r.visual_intent.into(),
            composition_session: None,
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
            old_selection_start: r.old_selection_byte_range.start.value() as u32,
            old_selection_end: r.old_selection_byte_range.end.value() as u32,
            new_selection_start: r.new_selection_byte_range.start.value() as u32,
            new_selection_end: r.new_selection_byte_range.end.value() as u32,
            visual_intent: r.visual_intent.into(),
            composition_session: None,
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
}

// ── #516: VisualRevision DTO ──

/// 已提交正文的视觉修订 DTO。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualRevisionDto {
    pub revision_id: u64,
    pub full_text: String,
    pub affected_paragraph_range_start: usize,
    pub affected_paragraph_range_end: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_rect: Option<CursorRectDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caret_affinity: Option<CaretAffinityDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shaping_identity: Option<String>,
}

impl From<crate::editor::VisualRevision> for VisualRevisionDto {
    fn from(r: crate::editor::VisualRevision) -> Self {
        Self {
            revision_id: r.revision_id,
            full_text: r.full_text,
            affected_paragraph_range_start: r.affected_paragraph_range.0,
            affected_paragraph_range_end: r.affected_paragraph_range.1,
            cursor_rect: r.cursor_rect.map(Into::into),
            caret_affinity: r.caret_affinity.map(Into::into),
            shaping_identity: r.shaping_identity,
        }
    }
}

// ── #516: CaretAffinity DTO ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CaretAffinityDto {
    Upstream,
    Downstream,
}

impl From<crate::editor::CaretAffinity> for CaretAffinityDto {
    fn from(a: crate::editor::CaretAffinity) -> Self {
        match a {
            crate::editor::CaretAffinity::Upstream => Self::Upstream,
            crate::editor::CaretAffinity::Downstream => Self::Downstream,
        }
    }
}

// ── #516: TransactionCancelReason DTO ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransactionCancelReasonDto {
    Rebased,
    RevisionChanged,
    SystemSuppressed,
    UserCancelled,
    CompositionCommitted,
    CompositionCancelled,
}

impl From<crate::editor::TransactionCancelReason> for TransactionCancelReasonDto {
    fn from(r: crate::editor::TransactionCancelReason) -> Self {
        match r {
            crate::editor::TransactionCancelReason::Rebased => Self::Rebased,
            crate::editor::TransactionCancelReason::RevisionChanged => Self::RevisionChanged,
            crate::editor::TransactionCancelReason::SystemSuppressed => Self::SystemSuppressed,
            crate::editor::TransactionCancelReason::UserCancelled => Self::UserCancelled,
            crate::editor::TransactionCancelReason::CompositionCommitted => Self::CompositionCommitted,
            crate::editor::TransactionCancelReason::CompositionCancelled => Self::CompositionCancelled,
        }
    }
}

// ── #516: CompositionCommitOrCancelTransaction DTO ──

/// 预输入提交/取消事务 DTO。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompositionCommitOrCancelTransactionDto {
    pub id: u64,
    pub is_commit: bool,
    pub is_visual_same: bool,
    pub duration_ms: u64,
    pub unified_kind: UnifiedTransactionKindDto,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub visual_class_kinds: Vec<VisualClassKindDto>,
}

impl From<crate::editor::CompositionCommitOrCancelTransaction> for CompositionCommitOrCancelTransactionDto {
    fn from(t: crate::editor::CompositionCommitOrCancelTransaction) -> Self {
        Self {
            id: t.id,
            is_commit: t.is_commit,
            is_visual_same: t.is_visual_same,
            duration_ms: t.duration_ms,
            unified_kind: UnifiedTransactionKindDto::CompositionCommitOrCancel,
            visual_class_kinds: t.visual_class_kinds.into_iter().map(Into::into).collect(),
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
        assert!(dto.has_inserted_range);
        // Insert: deleted_range = None → (0, 0), has_deleted_range = false
        assert_eq!(dto.deleted_range_start, 0);
        assert_eq!(dto.deleted_range_end, 0);
        assert!(!dto.has_deleted_range);
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
        // Delete: inserted_range = None → (0, 0), has_inserted_range = false
        assert_eq!(dto.inserted_range_start, 0);
        assert_eq!(dto.inserted_range_end, 0);
        assert!(!dto.has_inserted_range);
        // Delete: deleted_range = Some((2, 3))
        assert_eq!(dto.deleted_range_start, 2);
        assert_eq!(dto.deleted_range_end, 3);
        assert!(dto.has_deleted_range);
        assert_eq!(dto.duration_ms, 120);
        assert_eq!(dto.coordinate_mode, VisualCoordinateModeDto::Baseline);
    }

    /// T1.8: Verify inserted_range None → (0, 0) + has_inserted_range=false explicitly.
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
        assert!(!dto.has_inserted_range);
    }

    /// T1.8: Verify inserted_range Some((s, e)) maps correctly + has_inserted_range=true.
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
        assert!(dto.has_inserted_range);
    }

    /// Verify deleted_range Some((s, e)) maps correctly + has_deleted_range=true.
    #[test]
    fn visual_transaction_dto_deleted_range_some_maps_correctly() {
        let mut engine = crate::editor::EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "abc",
            "ab",
            crate::editor::EditorSelection::collapsed("abc", 3),
            crate::editor::EditorSelection::collapsed("ab", 2),
            crate::editor::EditorTransactionCause::Delete,
        );
        let vt = engine.visual_transaction(&tx).unwrap();
        // Core layer: Delete has deleted_range = Some((2, 3))
        assert_eq!(vt.deleted_range, Some((2, 3)));
        let dto: EditorVisualTransactionDto = vt.into();
        assert_eq!(dto.deleted_range_start, 2);
        assert_eq!(dto.deleted_range_end, 3);
        assert!(dto.has_deleted_range);
    }

    /// Verify deleted_range None → (0, 0) + has_deleted_range=false for Insert.
    #[test]
    fn visual_transaction_dto_deleted_range_none_maps_to_zeros() {
        let mut engine = crate::editor::EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "ab",
            "abc",
            crate::editor::EditorSelection::collapsed("ab", 2),
            crate::editor::EditorSelection::collapsed("abc", 3),
            crate::editor::EditorTransactionCause::Typing,
        );
        let vt = engine.visual_transaction(&tx).unwrap();
        // Core layer: Insert has deleted_range = None
        assert!(vt.deleted_range.is_none());
        let dto: EditorVisualTransactionDto = vt.into();
        assert_eq!(dto.deleted_range_start, 0);
        assert_eq!(dto.deleted_range_end, 0);
        assert!(!dto.has_deleted_range);
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
        assert!(json.contains("\"hasInsertedRange\":"));
        assert!(json.contains("\"deletedRangeStart\":"));
        assert!(json.contains("\"deletedRangeEnd\":"));
        assert!(json.contains("\"hasDeletedRange\":"));
        assert!(json.contains("\"durationMs\":"));
        assert!(json.contains("\"coordinateMode\":"));
        // Verify snake_case fields are NOT present
        assert!(!json.contains("\"old_text\":"));
        assert!(!json.contains("\"new_text\":"));
        assert!(!json.contains("\"old_selection_anchor\":"));
        assert!(!json.contains("\"inserted_range_start\":"));
        assert!(!json.contains("\"inserted_range_end\":"));
        assert!(!json.contains("\"has_inserted_range\":"));
        assert!(!json.contains("\"deleted_range_start\":"));
        assert!(!json.contains("\"deleted_range_end\":"));
        assert!(!json.contains("\"has_deleted_range\":"));
        assert!(!json.contains("\"duration_ms\":"));
        assert!(!json.contains("\"coordinate_mode\":"));
    }

    // --- #515: TimelineDto tests ---

    #[test]
    fn timeline_dto_from_core_timeline() {
        let mut tl = crate::editor::Timeline::new(200);
        tl.mark_first_visible_frame(1000);
        tl.pause(1100);
        let dto: TimelineDto = tl.into();
        assert_eq!(dto.duration_ms, 200);
        assert_eq!(dto.first_visible_frame_time_ms, Some(1000));
        assert_eq!(dto.pause_started_at_ms, Some(1100));
        assert!((dto.paused_progress - 0.5).abs() < 0.01);
    }

    #[test]
    fn timeline_dto_serializes_camel_case() {
        let dto = TimelineDto {
            first_visible_frame_time_ms: Some(1000),
            duration_ms: 200,
            pause_started_at_ms: None,
            accumulated_paused_duration_ms: 50,
            paused_progress: 0.3,
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert!(json.contains("\"firstVisibleFrameTimeMs\":"));
        assert!(json.contains("\"durationMs\":"));
        assert!(json.contains("\"accumulatedPausedDurationMs\":"));
        assert!(json.contains("\"pausedProgress\":"));
        assert!(!json.contains("\"pauseStartedAtMs\":"));

        // Verify zero defaults are skipped
        let dto_zero = TimelineDto {
            first_visible_frame_time_ms: None,
            duration_ms: 200,
            pause_started_at_ms: None,
            accumulated_paused_duration_ms: 0,
            paused_progress: 0.0,
        };
        let json_zero = serde_json::to_string(&dto_zero).unwrap();
        assert!(!json_zero.contains("\"firstVisibleFrameTimeMs\":"));
        assert!(!json_zero.contains("\"pauseStartedAtMs\":"));
        assert!(!json_zero.contains("\"accumulatedPausedDurationMs\":"));
        assert!(!json_zero.contains("\"pausedProgress\":"));
    }

    // --- #515: UnifiedTransactionKindDto tests ---

    #[test]
    fn unified_transaction_kind_dto_from_core() {
        assert_eq!(
            UnifiedTransactionKindDto::from(crate::editor::UnifiedTransactionKind::BodyEdit),
            UnifiedTransactionKindDto::BodyEdit
        );
        assert_eq!(
            UnifiedTransactionKindDto::from(crate::editor::UnifiedTransactionKind::CompositionUpdate),
            UnifiedTransactionKindDto::CompositionUpdate
        );
        assert_eq!(
            UnifiedTransactionKindDto::from(crate::editor::UnifiedTransactionKind::CompositionCommitOrCancel),
            UnifiedTransactionKindDto::CompositionCommitOrCancel
        );
        assert_eq!(
            UnifiedTransactionKindDto::from(crate::editor::UnifiedTransactionKind::CursorOnly),
            UnifiedTransactionKindDto::CursorOnly
        );
    }

    #[test]
    fn unified_transaction_kind_dto_serializes_camel_case() {
        let json = serde_json::to_string(&UnifiedTransactionKindDto::CompositionUpdate).unwrap();
        assert!(json.contains("\"compositionUpdate\""));
    }

    // --- #515: VisualClassKindDto tests ---

    #[test]
    fn visual_class_kind_dto_from_core() {
        assert_eq!(VisualClassKindDto::from(crate::editor::VisualClassKind::Static), VisualClassKindDto::Static);
        assert_eq!(VisualClassKindDto::from(crate::editor::VisualClassKind::Insert), VisualClassKindDto::Insert);
        assert_eq!(VisualClassKindDto::from(crate::editor::VisualClassKind::Delete), VisualClassKindDto::Delete);
        assert_eq!(VisualClassKindDto::from(crate::editor::VisualClassKind::Move), VisualClassKindDto::Move);
        assert_eq!(VisualClassKindDto::from(crate::editor::VisualClassKind::Crossfade), VisualClassKindDto::Crossfade);
    }

    // --- #515: PlatformVisualTransactionDto tests ---

    #[test]
    fn platform_visual_transaction_dto_from_core() {
        let mut tl = crate::editor::Timeline::new(160);
        tl.mark_first_visible_frame(1000);
        let pvt = crate::editor::PlatformVisualTransaction {
            transaction_id: 1,
            generation: 1,
            state: crate::editor::PlatformVisualTransactionState::Rendering,
            old_revision: crate::editor::VisualLayoutRevision {
                document_revision: 1, layout_revision: 1,
                viewport_width: 800.0, font_fingerprint: "f1".into(),
                paragraph_style_fingerprint: "p1".into(),
                text_color_fingerprint: "t1".into(), density_or_dpr: 2.0,
            },
            new_revision: crate::editor::VisualLayoutRevision {
                document_revision: 2, layout_revision: 2,
                viewport_width: 800.0, font_fingerprint: "f1".into(),
                paragraph_style_fingerprint: "p1".into(),
                text_color_fingerprint: "t1".into(), density_or_dpr: 2.0,
            },
            slice_roles: vec![crate::editor::AnimatedSliceRole::Insert],
            slice_document_byte_ranges: vec![(2, 3)],
            static_line_patches: Vec::new(),
            cursor_transition_byte_start: 2,
            cursor_transition_byte_end: 3,
            duration_ms: 160,
            rendering_started_at_ms: Some(1000),
            accumulated_paused_duration_ms: 0,
            timeline: Some(tl),
            unified_kind: Some(crate::editor::UnifiedTransactionKind::BodyEdit),
            visual_class_kinds: vec![crate::editor::VisualClassKind::Insert],
            decoration_slices: Vec::new(),
            cursor_path: None,
            composition_revision: None,
            rebase: None,
            cancel_reason: None,
        };
        let dto: PlatformVisualTransactionDto = pvt.into();
        assert_eq!(dto.transaction_id, 1);
        assert_eq!(dto.state, PlatformVisualTransactionStateDto::Rendering);
        assert_eq!(dto.unified_kind, UnifiedTransactionKindDto::BodyEdit);
        assert!(dto.timeline.is_some());
        assert_eq!(dto.visual_class_kinds.len(), 1);
        assert_eq!(dto.visual_class_kinds[0], VisualClassKindDto::Insert);
    }

    #[test]
    fn platform_visual_transaction_dto_serializes_camel_case() {
        let dto = PlatformVisualTransactionDto {
            transaction_id: 1,
            generation: 1,
            state: PlatformVisualTransactionStateDto::Pending,
            duration_ms: 160,
            unified_kind: UnifiedTransactionKindDto::BodyEdit,
            timeline: None,
            visual_class_kinds: vec![VisualClassKindDto::Insert],
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert!(json.contains("\"transactionId\":"));
        assert!(json.contains("\"unifiedKind\":"));
        assert!(json.contains("\"bodyEdit\""));
        assert!(json.contains("\"visualClassKinds\":"));
        assert!(!json.contains("\"timeline\":"));
    }

    // --- #535: EditorKernel V2 DTO tests ---

    #[test]
    fn editor_operation_kind_dto_from_core() {
        assert_eq!(EditorOperationKindDto::from(crate::editor::EditorOperationKind::Insert), EditorOperationKindDto::Insert);
        assert_eq!(EditorOperationKindDto::from(crate::editor::EditorOperationKind::Delete), EditorOperationKindDto::Delete);
        assert_eq!(EditorOperationKindDto::from(crate::editor::EditorOperationKind::Replace), EditorOperationKindDto::Replace);
        assert_eq!(EditorOperationKindDto::from(crate::editor::EditorOperationKind::CursorOnly), EditorOperationKindDto::CursorOnly);
        assert_eq!(EditorOperationKindDto::from(crate::editor::EditorOperationKind::CompositionUpdate), EditorOperationKindDto::CompositionUpdate);
        assert_eq!(EditorOperationKindDto::from(crate::editor::EditorOperationKind::CompositionCommit), EditorOperationKindDto::CompositionCommit);
        assert_eq!(EditorOperationKindDto::from(crate::editor::EditorOperationKind::CompositionCancel), EditorOperationKindDto::CompositionCancel);
        assert_eq!(EditorOperationKindDto::from(crate::editor::EditorOperationKind::Load), EditorOperationKindDto::Load);
        assert_eq!(EditorOperationKindDto::from(crate::editor::EditorOperationKind::Format), EditorOperationKindDto::Format);
    }

    #[test]
    fn editor_edit_result_dto_from_kernel() {
        let mut kernel = crate::editor::EditorKernel::with_text("ab".to_string(), 2).unwrap();
        let result = kernel.apply(crate::editor::EditorCommand::Insert {
            byte_offset: crate::editor::strong_types::Utf8ByteOffset::unchecked(2),
            text: "c".to_string(),
            cause: crate::editor::EditorTransactionCause::Typing,
            expected_revision: crate::editor::strong_types::EditorRevision::new(0),
        });
        let dto: EditorEditResultDto = result.into();
        assert!(dto.transaction_id > 0);
        assert_eq!(dto.base_revision, 0);
        assert_eq!(dto.new_revision, 1);
        assert!(!dto.display_patches.is_empty());
        assert_eq!(dto.visual_intent.operation_kind, EditorOperationKindDto::Insert);
    }

    #[test]
    fn display_patch_dto_from_core() {
        let patch = crate::editor::DisplayPatch {
            base_revision: crate::editor::strong_types::EditorRevision::new(0),
            new_revision: crate::editor::strong_types::EditorRevision::new(1),
            replace_byte_range: crate::editor::strong_types::Utf8ByteRange::new(2, 2).unwrap(),
            inserted_text: "c".to_string(),
            resulting_selection_byte_range: crate::editor::strong_types::Utf8ByteRange::new(3, 3).unwrap(),
        };
        let dto: DisplayPatchDto = patch.into();
        assert_eq!(dto.base_revision, 0);
        assert_eq!(dto.new_revision, 1);
        assert_eq!(dto.replace_byte_start, 2);
        assert_eq!(dto.replace_byte_end_exclusive, 2);
        assert_eq!(dto.inserted_text, "c");
    }

    #[test]
    fn editor_visual_intent_dto_from_core() {
        let intent = crate::editor::EditorVisualIntent {
            cause: crate::editor::EditorTransactionCause::Typing,
            operation_kind: crate::editor::EditorOperationKind::Insert,
            old_affected_byte_ranges: vec![],
            new_affected_byte_ranges: vec![crate::editor::strong_types::Utf8ByteRange::new(2, 5).unwrap()],
            animation_mode: crate::editor::AnimationMode::GlyphAnimation,
            duration_ms: 160,
            coordinated_cursor: crate::editor::CoordinatedCursor {
                old_offset: crate::editor::strong_types::Utf8ByteOffset::unchecked(2),
                new_offset: crate::editor::strong_types::Utf8ByteOffset::unchecked(5),
                should_animate: true,
            },
        };
        let dto: EditorVisualIntentDto = intent.into();
        assert_eq!(dto.operation_kind, EditorOperationKindDto::Insert);
        assert_eq!(dto.duration_ms, 160);
        assert!(dto.coordinated_cursor.should_animate);
        assert_eq!(dto.new_affected_byte_ranges.len(), 1);
    }

    #[test]
    fn editor_edit_result_dto_json_camel_case() {
        let mut kernel = crate::editor::EditorKernel::with_text("ab".to_string(), 2).unwrap();
        let result = kernel.apply(crate::editor::EditorCommand::Insert {
            byte_offset: crate::editor::strong_types::Utf8ByteOffset::unchecked(2),
            text: "c".to_string(),
            cause: crate::editor::EditorTransactionCause::Typing,
            expected_revision: crate::editor::strong_types::EditorRevision::new(0),
        });
        let dto: EditorEditResultDto = result.into();
        let json = serde_json::to_string(&dto).unwrap();

        assert!(json.contains("\"transactionId\":"), "DTO JSON should use camelCase for transactionId, got: {}", json);
        assert!(json.contains("\"baseRevision\":"), "DTO JSON should use camelCase for baseRevision, got: {}", json);
        assert!(json.contains("\"newRevision\":"), "DTO JSON should use camelCase for newRevision, got: {}", json);
        assert!(json.contains("\"displayPatches\":"), "DTO JSON should use camelCase for displayPatches, got: {}", json);
        assert!(json.contains("\"oldSelectionStart\":"), "DTO JSON should use camelCase for oldSelectionStart, got: {}", json);
        assert!(json.contains("\"oldSelectionEnd\":"), "DTO JSON should use camelCase for oldSelectionEnd, got: {}", json);
        assert!(json.contains("\"newSelectionStart\":"), "DTO JSON should use camelCase for newSelectionStart, got: {}", json);
        assert!(json.contains("\"newSelectionEnd\":"), "DTO JSON should use camelCase for newSelectionEnd, got: {}", json);
        assert!(json.contains("\"visualIntent\":"), "DTO JSON should use camelCase for visualIntent, got: {}", json);
        assert!(json.contains("\"replaceByteStart\":"), "DTO JSON should use camelCase for replaceByteStart, got: {}", json);
        assert!(json.contains("\"replaceByteEndExclusive\":"), "DTO JSON should use camelCase for replaceByteEndExclusive, got: {}", json);
        assert!(json.contains("\"insertedText\":"), "DTO JSON should use camelCase for insertedText, got: {}", json);
        assert!(json.contains("\"resultingSelectionStart\":"), "DTO JSON should use camelCase for resultingSelectionStart, got: {}", json);
        assert!(json.contains("\"resultingSelectionEnd\":"), "DTO JSON should use camelCase for resultingSelectionEnd, got: {}", json);
        assert!(json.contains("\"operationKind\":"), "DTO JSON should use camelCase for operationKind, got: {}", json);
        assert!(json.contains("\"animationMode\":"), "DTO JSON should use camelCase for animationMode, got: {}", json);
        assert!(json.contains("\"durationMs\":"), "DTO JSON should use camelCase for durationMs, got: {}", json);
        assert!(json.contains("\"coordinatedCursor\":"), "DTO JSON should use camelCase for coordinatedCursor, got: {}", json);
        assert!(json.contains("\"oldByteOffset\":"), "DTO JSON should use camelCase for oldByteOffset, got: {}", json);
        assert!(json.contains("\"newByteOffset\":"), "DTO JSON should use camelCase for newByteOffset, got: {}", json);
        assert!(json.contains("\"shouldAnimate\":"), "DTO JSON should use camelCase for shouldAnimate, got: {}", json);
        assert!(json.contains("\"oldAffectedByteRanges\":"), "DTO JSON should use camelCase for oldAffectedByteRanges, got: {}", json);
        assert!(json.contains("\"newAffectedByteRanges\":"), "DTO JSON should use camelCase for newAffectedByteRanges, got: {}", json);
    }
}
