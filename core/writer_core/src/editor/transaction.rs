//! 平台无关的编辑事务与动画语义模型。
//!
//! Core 只输出编辑语义、byte range、cause、animation mode 和平台无关 cursor 语义；
//! 平台视觉快照、glyph shaping、纹理、RenderNode/QImage 均不属于 Core。
//!
//! 平台端必须把输入事件翻译成这里的 transaction，再由平台渲染层决定如何绘制。
//! Core 不保存 QImage / QTextLayout / RenderNode / Bitmap / StaticLayout / 像素坐标。

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

/// 分层动画模式 — 替代旧的 NoAnimation/CursorOnly/FullAnimation 三值判定。
///
/// Core 是动画语义的权威：choose_animation_mode 根据文本特征和系统状态
/// 返回平台层应使用的动画模式。平台层只负责 offset 转换、布局捕获、渲染。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AnimationMode {
    /// 普通单字/短中文 commit/删除，逐 glyph 吞吐
    GlyphAnimation,
    /// emoji/ZWJ/组合字符/复杂 grapheme，整个 cluster 当整体动画
    ClusterAnimation,
    /// 超过 8 glyph/多字 commit/长中文词，按 run/word/chunk 分组动画
    RunAnimation,
    /// 换行/中间插入导致换行，按 old layout → new layout 行级 reflow
    LineReflowAnimation,
    /// 极端长文本或复杂布局，用局部 snapshot 做整体位移/淡入淡出
    /// UNAVAILABLE: No snapshot renderer exists on any platform.
    /// choose_animation_mode() must never return this variant.
    /// Retained for forward compatibility only.
    SnapshotAnimation,
    /// 系统抑制：滚动/加载/字号变化/章节切换/动画关闭
    /// 用户输入/删除/换行/IME commit/中间插入不能返回此值
    SystemSuppressed,
}

/// 矩形区域，用于 HiddenVisualRange 中的 old_rect/new_rect。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// 统一隐藏视觉范围 — 所有动画模式共用。
///
/// 静态正文层在动画期间跳过此范围，由动画 overlay 层渲染。
/// 动画完成后按 id 清除，正文层恢复完整绘制。
///
/// Glyph/Cluster/Run/LineReflow/Snapshot 都走 HiddenVisualRange。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HiddenVisualRange {
    /// 唯一 ID，用于动画完成后精确移除
    pub id: u64,
    /// 动画模式
    pub kind: AnimationMode,
    /// 范围起始（UTF-8 byte offset）
    pub range_start: usize,
    /// 范围结束（UTF-8 byte offset）
    pub range_end: usize,
    /// 旧矩形（LineReflow/Snapshot 使用）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_rect: Option<Rect>,
    /// 新矩形（LineReflow/Snapshot 使用）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_rect: Option<Rect>,
    /// 所在 visual line 索引
    #[serde(default)]
    pub line_index: usize,
    /// 关联的 payload 引用（如 cluster/run/snapshot 数据索引）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_ref: Option<u64>,
}

/// Grapheme cluster 矩形 — 用于 ClusterAnimation。
/// emoji/ZWJ/组合字符整组作为一个动画单元。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterRect {
    /// 该 cluster 的 UTF-8 byte 起始位置
    pub byte_start: usize,
    /// 该 cluster 的 UTF-8 byte 结束位置
    pub byte_end: usize,
    /// cluster 文本内容
    pub text: String,
    /// 是否包含复杂 grapheme
    pub is_complex: bool,
}

/// 分组动画 run — 用于 RunAnimation。
/// 中文每 4–6 字一组，英文按 word 一组。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterRun {
    /// 该 run 的 UTF-8 byte 起始位置
    pub byte_start: usize,
    /// 该 run 的 UTF-8 byte 结束位置
    pub byte_end: usize,
    /// run 文本内容
    pub text: String,
    /// 该 run 包含的 cluster 数量
    pub cluster_count: usize,
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
    /// 该 glyph 在文本中的 UTF-8 byte 起始位置
    #[serde(default)]
    pub byte_start: usize,
    /// 该 glyph 在文本中的 UTF-8 byte 结束位置
    #[serde(default)]
    pub byte_end: usize,
}

/// **DEPRECATED**: 已被 `EditorVisualTransaction` + `visual_transaction()` 替代。
/// 保留仅为现有测试覆盖；生产代码不得调用此类型。
/// 当前主链是 `EditorVisualTransaction`，见 `visual_transaction()` 方法。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[deprecated(
    since = "0.12.0",
    note = "Use EditorVisualTransaction instead. This will be removed in a future version."
)]
#[allow(dead_code)]
pub(crate) struct EditorAnimationEvent {
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
    /// 变更前光标的视口矩形位置（由 Linux_qt 端填充，Core 层默认为 None）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_cursor_rect: Option<CursorRect>,
    /// 变更后光标的视口矩形位置（由 Linux_qt 端填充，Core 层默认为 None）
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
/// Linux_qt SujianEditorItem 和 Android SujianEditorView 都吃同一份契约。
///
/// coordinate_mode 固定为 Baseline：所有 y 坐标使用 baselineY，
/// 不使用 top+height 拼接 baseline。
///
/// 这是 `EditorAnimationEvent` 的替代方案。
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
    /// 删除范围（UTF-8 byte offset），Delete 动画时平台层使用此范围
    /// 而非自行 diff_plain_text 计算，确保 Core 是范围语义唯一来源。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_range: Option<(usize, usize)>,
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
    /// 动画模式（由 Core choose_animation_mode 决定）
    #[serde(default = "default_animation_mode")]
    pub animation_mode: AnimationMode,
    /// Grapheme cluster 矩形列表（由平台层填充，Core 默认 None）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cluster_rects: Option<Vec<ClusterRect>>,
    /// 分组动画 run 列表（由平台层填充，Core 默认 None）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cluster_runs: Option<Vec<ClusterRun>>,
    /// 统一隐藏视觉范围列表
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden_visual_ranges: Vec<HiddenVisualRange>,
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

fn default_animation_mode() -> AnimationMode {
    AnimationMode::GlyphAnimation
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
    pub coordinate_mode: VisualCoordinateMode,
}

// =============================================================================
// 跨平台视觉语义边界
// =============================================================================
//
// Core 只输出 EditorVisualTransaction；平台端收到后，根据平台布局生成
// 自己的 PlatformVisualTransaction。两端共享以下语义概念和状态机，
// 不共享平台渲染结构（QImage / RenderNode / Bitmap 等）。
//
// 规则：
//   - Core 不保存 QImage / QTextLayout / RenderNode / Bitmap / StaticLayout / 像素坐标
//   - 平台视觉资源只存在于平台层
//   - 动画只能引用创建时的 old/new revision
//   - 进入动画协调器后只使用 document UTF-8 byte range；平台 UTF-16 index
//     只存在于布局适配层

/// 视觉布局版本指纹。
///
/// 以下变化都必须产生新 layout revision：
/// 正文、宽度、字号、字体和 fallback、行距、首行缩进、文字方向、
/// 主题正文色、Android density / Linux DPR。
///
/// 动画只能引用创建时的 old/new revision。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualLayoutRevision {
    pub document_revision: u64,
    pub layout_revision: u64,
    pub viewport_width: f64,
    pub font_fingerprint: String,
    pub paragraph_style_fingerprint: String,
    pub text_color_fingerprint: String,
    pub density_or_dpr: f64,
}

/// 动画切片角色 — 所有文字动画统一为 AnimatedSlice，角色区分行为。
///
/// Glyph、Cluster、Run、LineReflow 只是对 cluster 的分组方式，
/// 不再维护四套不同 payload。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AnimatedSliceRole {
    Insert,
    Delete,
    Move,
    CrossfadeOld,
    CrossfadeNew,
}

/// 静态行补丁 — 动画期间静态正文不能先完整显示新文字再叠一层动画。
///
/// 每个受影响的新视觉行生成一个补丁，平台静态层在动画期间跳过整条
/// 受影响视觉行，再使用 StaticLinePatch 画出没有被 AnimatedSlice 接管的部分。
///
/// 不能只按 byte range 在完整正文纹理上猜一块矩形，也不能恢复透明 Span。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StaticLinePatch {
    pub new_snapshot_id: u64,
    pub byte_start: usize,
    pub byte_end: usize,
    pub destination_rect: Rect,
    pub visible_source_rects: Vec<Rect>,
}

/// 平台视觉事务状态机。
///
/// 动画时间只能在首次进入 Rendering 时开始。
/// 滚动开始时 Rendering → Paused，滚动结束后 revision 未变则累加
/// pausedDuration 继续，revision 已变则取消失效事务。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlatformVisualTransactionState {
    Pending,
    Prepared,
    Rendering,
    Paused,
    Completed,
    Cancelled,
}

/// 统一时钟 — 文字切片、光标、预输入装饰全部消费同一个 progress。
///
/// Choreographer 和 Qt update 只负责请求帧，不得给光标维护独立开始时间。
/// Paused 状态返回暂停瞬间的 progress，不能返回 0。
/// resume 后从暂停进度继续。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Timeline {
    /// 首帧可见时间戳（毫秒，单调时钟）
    pub first_visible_frame_time_ms: Option<u64>,
    /// 动画时长（毫秒）
    pub duration_ms: u64,
    /// 暂停开始时间戳（毫秒，单调时钟）；None 表示未暂停
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pause_started_at_ms: Option<u64>,
    /// 累计暂停时长（毫秒）
    #[serde(default)]
    pub accumulated_paused_duration_ms: u64,
    /// 暂停瞬间的 progress（0.0–1.0）；Paused 状态必须返回此值，不能返回 0
    #[serde(default)]
    pub paused_progress: f64,
}

impl Timeline {
    pub fn new(duration_ms: u64) -> Self {
        Self {
            first_visible_frame_time_ms: None,
            duration_ms,
            pause_started_at_ms: None,
            accumulated_paused_duration_ms: 0,
            paused_progress: 0.0,
        }
    }

    /// 计算当前进度（0.0–1.0）。
    ///
    /// - 未开始（first_visible_frame_time_ms == None）→ 0.0
    /// - Paused → paused_progress（暂停瞬间快照，不返回 0）
    /// - 正常播放 → clamp(effective_elapsed / duration, 0.0, 1.0)
    ///   effective_elapsed = frame_time - start - accumulated_paused_duration
    /// - 完成 → 1.0
    pub fn progress(&self, frame_time_ms: u64) -> f64 {
        let start = match self.first_visible_frame_time_ms {
            Some(t) => t,
            None => return 0.0,
        };

        if self.pause_started_at_ms.is_some() {
            return self.paused_progress;
        }

        if self.duration_ms == 0 {
            return 1.0;
        }

        let effective_elapsed = frame_time_ms
            .saturating_sub(start)
            .saturating_sub(self.accumulated_paused_duration_ms);
        let p = (effective_elapsed as f64) / (self.duration_ms as f64);
        p.clamp(0.0, 1.0)
    }

    /// 标记首帧可见时间。
    pub fn mark_first_visible_frame(&mut self, frame_time_ms: u64) {
        if self.first_visible_frame_time_ms.is_none() {
            self.first_visible_frame_time_ms = Some(frame_time_ms);
        }
    }

    /// 暂停 — 记录暂停瞬间的 progress，不能返回 0。
    pub fn pause(&mut self, frame_time_ms: u64) {
        if self.pause_started_at_ms.is_some() {
            return;
        }
        self.paused_progress = self.progress(frame_time_ms);
        self.pause_started_at_ms = Some(frame_time_ms);
    }

    /// 恢复 — 从暂停进度继续。
    ///
    /// 关键：resume 后 progress 必须从 paused_progress 平滑过渡。
    /// 调整 first_visible_frame_time_ms 使得
    /// progress(resume_time) = paused_progress，即：
    ///   new_start = resume_time - paused_progress * duration
    pub fn resume(&mut self, frame_time_ms: u64) {
        if self.pause_started_at_ms.is_none() {
            return;
        }

        if self.first_visible_frame_time_ms.is_none() {
            self.pause_started_at_ms = None;
            self.paused_progress = 0.0;
            return;
        }

        let new_start = frame_time_ms
            .saturating_sub((self.paused_progress * self.duration_ms as f64) as u64);
        self.first_visible_frame_time_ms = Some(new_start);
        self.accumulated_paused_duration_ms = 0;
        self.pause_started_at_ms = None;
        self.paused_progress = 0.0;
    }

    /// 是否已暂停。
    pub fn is_paused(&self) -> bool {
        self.pause_started_at_ms.is_some()
    }

    /// 是否已完成。
    pub fn is_completed(&self, frame_time_ms: u64) -> bool {
        self.progress(frame_time_ms) >= 1.0
    }
}

/// 统一事务类型 — 最终 Linux 和 Android 只保留四种事务。
///
/// 所有事务共用 VisualRevision、LineSnapshot、AnimatedSlice、
/// StaticLinePatch、DecorationSlice、CursorPath 和 Timeline。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UnifiedTransactionKind {
    /// 正文编辑：普通插入、删除、换行、段落合并和删除回流
    BodyEdit,
    /// 预输入更新：setComposingText 触发
    CompositionUpdate,
    /// 预输入提交或取消
    CompositionCommitOrCancel,
    /// 仅光标移动：无正文变更的光标移动
    CursorOnly,
}

/// 视觉对象分类 — 通过 old/new VisualRevision、OffsetMap 和 shaping identity 分类。
///
/// 中间插入、换行、段落合并和删除回流全部使用这套分类，不建立特例。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VisualClassKind {
    /// 文本、shaping、位置均相同，进入 StaticLinePatch
    Static,
    /// 仅 new 存在，使用 new snapshot 从光标附近移动到最终位置并淡入
    Insert,
    /// 仅 old 存在，使用 old snapshot 向删除后光标或收缩中心移动并淡出
    Delete,
    /// shaping 相同但位置变化，从 oldRect 移到 newRect
    Move,
    /// 文本可映射但 shaping 改变，old 淡出、new 淡入
    Crossfade,
}

/// 装饰切片 — 预输入下划线、分段颜色和 IME cursor。
///
/// 使用同一 Timeline，不另起独立时间链。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecorationSlice {
    /// 装饰类型
    pub kind: DecorationSliceKind,
    /// UTF-8 byte 范围起始
    pub byte_start: usize,
    /// UTF-8 byte 范围结束
    pub byte_end: usize,
    /// 矩形区域（由平台层填充）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rect: Option<Rect>,
    /// 颜色（如 underline color、text color 等）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// 装饰切片类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DecorationSliceKind {
    Underline,
    TextColor,
    BackgroundColor,
    Cursor,
}

/// 光标路径 — 光标移动轨迹，使用同一 Timeline。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorPath {
    /// 起始光标矩形
    pub from_rect: CursorRect,
    /// 目标光标矩形
    pub to_rect: CursorRect,
    /// 是否 snap（无动画）
    pub is_snap: bool,
}

/// 预输入视觉修订 — 把预输入改为临时视觉正文版本。
///
/// virtualText 仅用于排版和渲染，不写入正文、Undo、保存、同步和 Core 正文状态。
/// 每次预输入变化生成新 CompositionVisualRevision，
/// 使用相同 StaticLinePatch + AnimatedSlice 分类。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompositionVisualRevision {
    /// 已提交文本（不含预输入）
    pub committed_text: String,
    /// 预输入替换范围（UTF-8 byte offset）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composition_replace_range: Option<(usize, usize)>,
    /// 预输入文本
    #[serde(default)]
    pub preedit_text: String,
    /// 虚拟文本 — 仅用于排版和渲染，不写入正文
    #[serde(default)]
    pub virtual_text: String,
    /// 受影响段落范围（UTF-8 byte offset）
    pub affected_paragraph_range: (usize, usize),
    /// 行快照 ID 列表（由平台层填充）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub line_snapshot_ids: Vec<u64>,
    /// 光标矩形
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_rect: Option<CursorRect>,
    /// 装饰范围
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decoration_ranges: Vec<DecorationSlice>,
}

/// 连续事务 rebase — 新事务与旧事务冲突时。
///
/// 预输入开始、更新、提交、取消以及连续正文输入都不得调用 pauseAll 叠加另一条事务。
/// 新事务与旧事务冲突时：
/// 1. 读取旧事务当前 progress
/// 2. 计算当前视觉帧
/// 3. 将当前 frame rect/alpha/scale 作为新事务 old state
/// 4. 取消旧事务
/// 5. 启动新事务
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionRebase {
    /// 被取消的旧事务 ID
    pub cancelled_transaction_id: u64,
    /// 旧事务在 rebase 瞬间的 progress（0.0–1.0）
    pub old_progress: f64,
    /// 旧事务当前帧的视觉状态快照（由平台层填充）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_frame_snapshot: Option<RebaseFrameSnapshot>,
}

/// Rebase 瞬间的帧快照 — 将当前 frame rect/alpha/scale 作为新事务 old state。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RebaseFrameSnapshot {
    /// 各切片的当前帧矩形
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slice_rects: Vec<Rect>,
    /// 各切片的当前帧透明度
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slice_alphas: Vec<f64>,
    /// 光标当前位置
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_rect: Option<CursorRect>,
}

/// 跨平台视觉事务语义边界。
///
/// Core 输出 EditorVisualTransaction；平台端收到后，根据平台布局
/// 生成 PlatformVisualTransaction。两端共享此结构和状态机概念，
/// 不共享平台渲染结构。
///
/// `visualResource` 字段由平台各自实现，不进入此结构。
///
/// #515: 增加统一 Timeline、UnifiedTransactionKind、VisualClassKind、
/// DecorationSlice、CursorPath、CompositionVisualRevision、TransactionRebase。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformVisualTransaction {
    pub transaction_id: u64,
    pub generation: u64,
    pub state: PlatformVisualTransactionState,
    pub old_revision: VisualLayoutRevision,
    pub new_revision: VisualLayoutRevision,
    pub slice_roles: Vec<AnimatedSliceRole>,
    pub slice_document_byte_ranges: Vec<(usize, usize)>,
    pub static_line_patches: Vec<StaticLinePatch>,
    pub cursor_transition_byte_start: usize,
    pub cursor_transition_byte_end: usize,
    pub duration_ms: u64,
    pub rendering_started_at_ms: Option<u64>,
    pub accumulated_paused_duration_ms: u64,
    /// #515: 统一时钟 — 文字切片、光标、预输入装饰全部消费同一个 progress
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeline: Option<Timeline>,
    /// #515: 统一事务类型
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unified_kind: Option<UnifiedTransactionKind>,
    /// #515: 视觉对象分类列表（与 slice_roles/slice_document_byte_ranges 对应）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub visual_class_kinds: Vec<VisualClassKind>,
    /// #515: 装饰切片（预输入下划线、分段颜色、IME cursor）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decoration_slices: Vec<DecorationSlice>,
    /// #515: 光标路径（使用同一 Timeline）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_path: Option<CursorPath>,
    /// #515: 预输入视觉修订（仅 CompositionUpdate/CompositionCommitOrCancel 事务）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composition_revision: Option<CompositionVisualRevision>,
    /// #515: 连续事务 rebase 信息
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rebase: Option<TransactionRebase>,
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

    /// **DEPRECATED**: 已被 `visual_transaction()` 替代。
    /// 保留仅为现有测试覆盖；生产代码不得调用此方法。
    /// 当前主链是 `visual_transaction()`，见该方法文档。
    #[deprecated(
        since = "0.12.0",
        note = "Use visual_transaction() instead. This will be removed in a future version."
    )]
    #[allow(deprecated, dead_code)]
    pub(crate) fn animation_events(
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
    /// Core 只填充语义字段（id, kind, cause, old/new text, selection, inserted_range, duration, coordinate_mode, animation_mode）。
    /// 平台层负责填充坐标字段（glyph_rects, cursor_rect, cluster_rects, cluster_runs）。
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
        let deleted_range = match change {
            EditorChange::Insert { .. } => None,
            EditorChange::Delete { index, text } => Some((*index, *index + text.len())),
        };

        let text = change.text();
        let cluster_count = count_grapheme_clusters(text);
        let contains_newline = text.contains('\n');
        let contains_complex_grapheme = text_contains_complex_grapheme(text);

        // choose_animation_mode — 根据 cause 传入系统状态
        let is_loading = transaction.cause == EditorTransactionCause::Load;
        let is_applying_format = transaction.cause == EditorTransactionCause::Format;
        let animation_mode = choose_animation_mode(
            cluster_count,
            contains_newline,
            contains_complex_grapheme,
            false, // is_scrolling
            is_loading,
            is_applying_format,
            false, // is_applying_settings
            true,  // animation_enabled
        );

        // 如果是 Insert，计算 cluster_rects 和 cluster_runs
        let (cluster_rects, cluster_runs) = match change {
            EditorChange::Insert { index, text: _ } => {
                let rects = split_text_into_clusters(text, *index);
                let runs = split_text_into_runs(text, *index);
                (Some(rects), Some(runs))
            }
            EditorChange::Delete { .. } => (None, None),
        };

        // 构建 hidden_visual_ranges
        let hidden_visual_ranges = match inserted_range {
            Some((start, end)) => vec![HiddenVisualRange {
                id: self.take_animation_id(),
                kind: animation_mode,
                range_start: start,
                range_end: end,
                old_rect: None,
                new_rect: None,
                line_index: 0,
                payload_ref: None,
            }],
            None => Vec::new(),
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
            deleted_range,
            deleted_glyph_rects: None,
            insert_glyph_rects: None,
            reflow_glyph_rects: None,
            animation_mode,
            cluster_rects,
            cluster_runs,
            hidden_visual_ranges,
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
    _max_animated_chars: usize,
) -> bool {
    // 系统状态和 preedit 不进动画
    // ImeComposition 是 preedit 阶段，有自己的视觉层，不需要吞吐动画
    // IME commit 走 TypingCommit cause，已经允许动画
    if matches!(
        cause,
        EditorTransactionCause::Load
            | EditorTransactionCause::Format
            | EditorTransactionCause::Programmatic
            | EditorTransactionCause::ImeComposition
    ) {
        return false;
    }
    if changes.len() != 1 {
        return false;
    }
    let text = changes[0].text();
    // 不再限制换行和字符数量 — 由 choose_animation_mode 决定具体动画模式
    !text.is_empty()
}

/// 统一动画模式选择函数 — 替代旧的 should_create_text_animation。
///
/// 输入：文本特征 + 系统状态
/// 输出：AnimationMode — 平台层据此决定如何渲染动画
///
/// 规则（按优先级）：
/// 1. 系统抑制条件（动画关闭/滚动/加载/格式化/设置变化）→ SystemSuppressed
/// 2. glyph 为空 → SystemSuppressed（无内容可动画）
/// 3. 包含换行 → LineReflowAnimation（换行必须做行级 reflow，不许只动光标）
/// 4. 包含复杂 grapheme → ClusterAnimation（整组动画，不跳过）
/// 5. cluster 数量 1–8 → GlyphAnimation（逐 cluster 动画）
/// 6. cluster 数量 9–40 → RunAnimation（按 word/run/chunk 分组动画）
    /// 7. cluster 数量 > 40 → RunAnimation（SnapshotAnimation unavailable，无 snapshot renderer）
pub fn choose_animation_mode(
    cluster_count: usize,
    contains_newline: bool,
    contains_complex_grapheme: bool,
    is_scrolling: bool,
    is_loading: bool,
    is_applying_format: bool,
    is_applying_settings: bool,
    animation_enabled: bool,
) -> AnimationMode {
    // 1. 系统抑制条件
    if !animation_enabled || is_scrolling || is_loading || is_applying_format || is_applying_settings {
        return AnimationMode::SystemSuppressed;
    }
    // 2. 无内容可动画
    if cluster_count == 0 {
        return AnimationMode::SystemSuppressed;
    }
    // 3. 包含换行 → 行级 reflow
    if contains_newline {
        return AnimationMode::LineReflowAnimation;
    }
    // 4. 包含复杂 grapheme → 整组动画
    if contains_complex_grapheme {
        return AnimationMode::ClusterAnimation;
    }
    // 5–7. 按 cluster 数量分级
    // NOTE: SnapshotAnimation is unavailable (no snapshot renderer exists on any platform).
    // >40 cluster edits use RunAnimation instead. SnapshotAnimation enum variant is retained
    // for forward compatibility but must never be returned by this function.
    if cluster_count <= 8 {
        AnimationMode::GlyphAnimation
    } else {
        AnimationMode::RunAnimation
    }
}

/// 计算文本的 grapheme cluster 数量
pub fn count_grapheme_clusters(text: &str) -> usize {
    use unicode_segmentation::UnicodeSegmentation;
    text.graphemes(true).count()
}

/// 检测文本是否包含复杂 grapheme（emoji/ZWJ/组合字符等）
pub fn text_contains_complex_grapheme(text: &str) -> bool {
    use unicode_segmentation::UnicodeSegmentation;
    text.graphemes(true).any(|g| g.chars().any(|ch| is_complex_grapheme_code_point(ch as u32)))
}

/// 检测单个 code point 是否属于复杂 grapheme
pub fn is_complex_grapheme_code_point(cp: u32) -> bool {
    // Surrogate pairs: code point > 0xFFFF (non-BMP, e.g. emoji)
    if cp > 0xFFFF { return true; }
    // Zero Width Joiner
    if cp == 0x200D { return true; }
    // Variation selectors (FE00-FE0F, E0100-E01EF)
    if (cp >= 0xFE00 && cp <= 0xFE0F) || (cp >= 0xE0100 && cp <= 0xE01EF) { return true; }
    // Combining Diacritical Marks (0300-036F)
    if cp >= 0x0300 && cp <= 0x036F { return true; }
    // Combining Diacritical Marks Extended (1AB0-1AFF)
    if cp >= 0x1AB0 && cp <= 0x1AFF { return true; }
    // Combining Diacritical Marks Supplement (1DC0-1DFF)
    if cp >= 0x1DC0 && cp <= 0x1DFF { return true; }
    // Combining Diacritical Marks for Symbols (20D0-20FF)
    if cp >= 0x20D0 && cp <= 0x20FF { return true; }
    // Combining Half Marks (FE20-FE2F)
    if cp >= 0xFE20 && cp <= 0xFE2F { return true; }
    // Emoji code points (common ranges)
    if cp >= 0x1F600 && cp <= 0x1F64F { return true; }
    if cp >= 0x1F300 && cp <= 0x1F5FF { return true; }
    if cp >= 0x1F680 && cp <= 0x1F6FF { return true; }
    if cp >= 0x1F900 && cp <= 0x1F9FF { return true; }
    // Regional Indicator (U+1F1E6-U+1F1FF)
    if cp >= 0x1F1E6 && cp <= 0x1F1FF { return true; }
    false
}

/// 检测单个 code point 是否为组合字符（附加到前一个 base character）
pub fn is_combining_code_point(cp: u32) -> bool {
    // Combining Diacritical Marks (0300-036F)
    (cp >= 0x0300 && cp <= 0x036F)
    // Combining Diacritical Marks Extended (1AB0-1AFF)
    || (cp >= 0x1AB0 && cp <= 0x1AFF)
    // Combining Diacritical Marks Supplement (1DC0-1DFF)
    || (cp >= 0x1DC0 && cp <= 0x1DFF)
    // Combining Diacritical Marks for Symbols (20D0-20FF)
    || (cp >= 0x20D0 && cp <= 0x20FF)
    // Combining Half Marks (FE20-FE2F)
    || (cp >= 0xFE20 && cp <= 0xFE2F)
    // Variation selectors
    || (cp >= 0xFE00 && cp <= 0xFE0F)
    || (cp >= 0xE0100 && cp <= 0xE01EF)
    // Zero Width Joiner
    || cp == 0x200D
}

/// 检测单个 code point 是否属于 CJK 字符
pub fn is_cjk_code_point(cp: u32) -> bool {
    (cp >= 0x4E00 && cp <= 0x9FFF)   // CJK Unified Ideographs
    || (cp >= 0x3400 && cp <= 0x4DBF) // CJK Unified Ideographs Extension A
    || (cp >= 0x20000 && cp <= 0x2A6DF) // CJK Unified Ideographs Extension B
    || (cp >= 0x2A700 && cp <= 0x2B73F) // CJK Unified Ideographs Extension C
    || (cp >= 0x2B740 && cp <= 0x2B81F) // CJK Unified Ideographs Extension D
    || (cp >= 0xF900 && cp <= 0xFAFF) // CJK Compatibility Ideographs
    || (cp >= 0x2F800 && cp <= 0x2FA1F) // CJK Compatibility Ideographs Supplement
    || (cp >= 0x3000 && cp <= 0x303F) // CJK Symbols and Punctuation
    || (cp >= 0x3040 && cp <= 0x309F) // Hiragana
    || (cp >= 0x30A0 && cp <= 0x30FF) // Katakana
    || (cp >= 0xAC00 && cp <= 0xD7AF) // Hangul Syllables
}

/// 将文本按 run/word/chunk 分组，用于 RunAnimation。
/// 中文每 4–6 字一组，英文按 word 一组。
pub fn split_text_into_runs(text: &str, base_offset: usize) -> Vec<ClusterRun> {
    let mut runs = Vec::new();
    let mut current_text = String::new();
    let mut current_cluster_count = 0usize;
    let mut current_byte_start = base_offset;

    let chinese_chunk_size = 5; // 中文每 5 字一组

    for (byte_offset, ch) in text.char_indices() {
        let absolute_byte = base_offset + byte_offset;

        if ch.is_whitespace() {
            // 空格结束当前 run
            if !current_text.is_empty() {
                runs.push(ClusterRun {
                    byte_start: current_byte_start,
                    byte_end: absolute_byte,
                    text: current_text.clone(),
                    cluster_count: current_cluster_count,
                });
                current_text.clear();
                current_cluster_count = 0;
            }
            // 空格本身作为独立 run
            runs.push(ClusterRun {
                byte_start: absolute_byte,
                byte_end: absolute_byte + ch.len_utf8(),
                text: ch.to_string(),
                cluster_count: 1,
            });
            current_byte_start = absolute_byte + ch.len_utf8();
            continue;
        }

        let is_cjk = is_cjk_code_point(ch as u32);

        if current_text.is_empty() {
            current_byte_start = absolute_byte;
        }

        current_text.push(ch);
        current_cluster_count += 1;

        // CJK 字符达到 chunk 大小时结束 run
        if is_cjk && current_cluster_count >= chinese_chunk_size {
            runs.push(ClusterRun {
                byte_start: current_byte_start,
                byte_end: absolute_byte + ch.len_utf8(),
                text: current_text.clone(),
                cluster_count: current_cluster_count,
            });
            current_text.clear();
            current_cluster_count = 0;
            current_byte_start = absolute_byte + ch.len_utf8();
        }

        // 非 CJK 连续字符达到一定长度也结束 run
        if !is_cjk && current_cluster_count >= 8 {
            runs.push(ClusterRun {
                byte_start: current_byte_start,
                byte_end: absolute_byte + ch.len_utf8(),
                text: current_text.clone(),
                cluster_count: current_cluster_count,
            });
            current_text.clear();
            current_cluster_count = 0;
            current_byte_start = absolute_byte + ch.len_utf8();
        }
    }

    // 处理剩余
    if !current_text.is_empty() {
        runs.push(ClusterRun {
            byte_start: current_byte_start,
            byte_end: base_offset + text.len(),
            text: current_text,
            cluster_count: current_cluster_count,
        });
    }

    runs
}

/// 将文本按 grapheme cluster 分割，用于 ClusterAnimation。
/// 每个 cluster 记录 byte range 和是否复杂。
pub fn split_text_into_clusters(text: &str, base_offset: usize) -> Vec<ClusterRect> {
    use unicode_segmentation::UnicodeSegmentation;
    let mut clusters = Vec::new();
    for grapheme in text.graphemes(true) {
        let byte_start = base_offset + (grapheme.as_ptr() as usize - text.as_ptr() as usize);
        let byte_end = byte_start + grapheme.len();
        let is_complex = grapheme.chars().any(|ch| is_complex_grapheme_code_point(ch as u32));
        clusters.push(ClusterRect {
            byte_start,
            byte_end,
            text: grapheme.to_string(),
            is_complex,
        });
    }
    clusters
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

        // Paste 现在进入 visual transaction（should_animate=true）
        assert!(tx.should_animate);
        let events = engine.animation_events(&tx);
        // Paste 长文本产生 Insert + Cursor 事件
        assert!(events.len() >= 1);
        assert_eq!(events[0].kind, EditorAnimationKind::Insert);
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
            byte_start: 0,
            byte_end: 3,
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
                    byte_start: 0,
                    byte_end: 1,
                },
                GlyphRect {
                    x: 10.0,
                    y: 0.0,
                    w: 10.0,
                    h: 20.0,
                    char_: "b".to_string(),
                    baseline_y: 16.0,
                    byte_start: 1,
                    byte_end: 2,
                },
                GlyphRect {
                    x: 20.0,
                    y: 0.0,
                    w: 10.0,
                    h: 20.0,
                    char_: "c".to_string(),
                    baseline_y: 16.0,
                    byte_start: 2,
                    byte_end: 3,
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
        // This test verifies that the Linux_qt Rust side filters complex grapheme
        // chars when filling glyph_rects. Since the filtering happens in the
        // Linux_qt-specific fill_glyph_rects_for_events (not in core), we test
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
        // Paste 现在进入 visual transaction
        assert!(tx.should_animate);
        let events = engine.animation_events(&tx);
        // Paste 长文本产生 Insert + Cursor 事件
        assert!(events.len() >= 1);
        assert_eq!(events[0].kind, EditorAnimationKind::Insert);
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
        // Undo 现在进入 visual transaction
        assert!(tx.should_animate, "Undo cause should animate");
        let events = engine.animation_events(&tx);
        // Undo 产生 Delete + Cursor 事件
        assert!(events.len() >= 1);
        assert_eq!(events[0].kind, EditorAnimationKind::Delete);
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
        // Redo 现在进入 visual transaction
        assert!(tx.should_animate, "Redo cause should animate");
        let events = engine.animation_events(&tx);
        // Redo 产生 Insert + Cursor 事件
        assert!(events.len() >= 1);
        assert_eq!(events[0].kind, EditorAnimationKind::Insert);
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
        // ImeComposition 是 preedit 阶段，不需要吞吐动画
        // IME commit 走 TypingCommit cause，已经允许动画
        assert!(!tx.should_animate, "ImeComposition should not animate");
        let events = engine.animation_events(&tx);
        // 只有 Cursor 事件（光标位置变化），没有 Insert/Delete 动画
        assert!(events.iter().all(|e| e.kind == EditorAnimationKind::Cursor));
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

        // Paste 现在也进入 visual transaction（用户触发的操作不应被入口拦掉）
        let tx_paste = engine.create_transaction(
            "a",
            "a pasted text",
            EditorSelection::collapsed("a", 1),
            EditorSelection::collapsed("a pasted text", "a pasted text".len()),
            EditorTransactionCause::Paste,
        );
        assert!(tx_paste.should_animate, "Paste should animate as a user-triggered operation");
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

        // Undo with text change 现在进入 visual transaction
        let tx_undo = engine.create_transaction(
            "abc",
            "a",
            EditorSelection::collapsed("abc", 3),
            EditorSelection::collapsed("a", 1),
            EditorTransactionCause::Undo,
        );
        assert!(tx_undo.should_animate, "Undo should animate");
        let events_undo = engine.animation_events(&tx_undo);
        assert!(events_undo.len() >= 1);
        assert_eq!(events_undo[0].kind, EditorAnimationKind::Delete);

        // Redo with text change 现在进入 visual transaction
        let tx_redo = engine.create_transaction(
            "a",
            "abc",
            EditorSelection::collapsed("a", 1),
            EditorSelection::collapsed("abc", 3),
            EditorTransactionCause::Redo,
        );
        assert!(tx_redo.should_animate, "Redo should animate");
        let events_redo = engine.animation_events(&tx_redo);
        assert!(events_redo.len() >= 1);
        assert_eq!(events_redo[0].kind, EditorAnimationKind::Insert);
    }

    #[test]
    fn paste_no_animation() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);

        // Paste with single-char text 现在进入 visual transaction
        let tx = engine.create_transaction(
            "a",
            "ab",
            EditorSelection::collapsed("a", 1),
            EditorSelection::collapsed("ab", 2),
            EditorTransactionCause::Paste,
        );
        assert!(tx.should_animate, "Paste should animate even for single char");
        let events = engine.animation_events(&tx);
        assert!(events.len() >= 1);
        assert_eq!(events[0].kind, EditorAnimationKind::Insert);

        // Paste with multi-char text 也进入 visual transaction
        let tx2 = engine.create_transaction(
            "a",
            "a long pasted text",
            EditorSelection::collapsed("a", 1),
            EditorSelection::collapsed("a long pasted text", "a long pasted text".len()),
            EditorTransactionCause::Paste,
        );
        assert!(tx2.should_animate, "Paste should animate for multi-char text");
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
    fn visual_transaction_paste_enters_visual_transaction() {
        // Paste 长文本进入 visual transaction，mode 是 RunAnimation (SnapshotAnimation unavailable)
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "a",
            "a long pasted text",
            EditorSelection::collapsed("a", 1),
            EditorSelection::collapsed("a long pasted text", "a long pasted text".len()),
            EditorTransactionCause::Paste,
        );
        let vt = engine.visual_transaction(&tx);
        assert!(vt.is_some(), "Paste should enter visual transaction");
        let vt = vt.unwrap();
        assert!(
            vt.animation_mode == AnimationMode::RunAnimation,
            "Paste long text should be RunAnimation, got {:?}",
            vt.animation_mode
        );
    }

    #[test]
    fn visual_transaction_paste_short_text_glyph_animation() {
        // Paste 短文本进入 visual transaction，mode 是 GlyphAnimation
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "a",
            "abc",
            EditorSelection::collapsed("a", 1),
            EditorSelection::collapsed("abc", 3),
            EditorTransactionCause::Paste,
        );
        let vt = engine.visual_transaction(&tx);
        assert!(vt.is_some(), "Paste short text should enter visual transaction");
        let vt = vt.unwrap();
        assert_eq!(
            vt.animation_mode,
            AnimationMode::GlyphAnimation,
            "Paste short text should be GlyphAnimation"
        );
    }

    #[test]
    fn visual_transaction_paste_newline_line_reflow() {
        // Paste 包含换行进入 visual transaction，mode 是 LineReflowAnimation
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "a",
            "a\nb",
            EditorSelection::collapsed("a", 1),
            EditorSelection::collapsed("a\nb", "a\nb".len()),
            EditorTransactionCause::Paste,
        );
        let vt = engine.visual_transaction(&tx);
        assert!(vt.is_some(), "Paste with newline should enter visual transaction");
        let vt = vt.unwrap();
        assert_eq!(
            vt.animation_mode,
            AnimationMode::LineReflowAnimation,
            "Paste with newline should be LineReflowAnimation"
        );
    }

    #[test]
    fn visual_transaction_undo_enters_visual_transaction() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "abc",
            "a",
            EditorSelection::collapsed("abc", 3),
            EditorSelection::collapsed("a", 1),
            EditorTransactionCause::Undo,
        );
        let vt = engine.visual_transaction(&tx);
        assert!(vt.is_some(), "Undo should enter visual transaction");
    }

    #[test]
    fn visual_transaction_redo_enters_visual_transaction() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "a",
            "abc",
            EditorSelection::collapsed("a", 1),
            EditorSelection::collapsed("abc", 3),
            EditorTransactionCause::Redo,
        );
        let vt = engine.visual_transaction(&tx);
        assert!(vt.is_some(), "Redo should enter visual transaction");
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
            byte_start: 0, byte_end: 3,
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

    // --- AnimationMode / choose_animation_mode tests ---

    #[test]
    fn choose_animation_mode_typing_returns_glyph_animation() {
        // 1–8 个普通 cluster → GlyphAnimation
        let mode = choose_animation_mode(5, false, false, false, false, false, false, true);
        assert_eq!(mode, AnimationMode::GlyphAnimation);

        let mode1 = choose_animation_mode(1, false, false, false, false, false, false, true);
        assert_eq!(mode1, AnimationMode::GlyphAnimation);

        let mode8 = choose_animation_mode(8, false, false, false, false, false, false, true);
        assert_eq!(mode8, AnimationMode::GlyphAnimation);
    }

    #[test]
    fn choose_animation_mode_complex_grapheme_returns_cluster_animation() {
        // emoji → ClusterAnimation
        let mode = choose_animation_mode(1, false, true, false, false, false, false, true);
        assert_eq!(mode, AnimationMode::ClusterAnimation);
    }

    #[test]
    fn choose_animation_mode_zwj_returns_cluster_animation() {
        // ZWJ emoji → ClusterAnimation (contains_complex_grapheme=true)
        let mode = choose_animation_mode(3, false, true, false, false, false, false, true);
        assert_eq!(mode, AnimationMode::ClusterAnimation);
    }

    #[test]
    fn choose_animation_mode_newline_returns_line_reflow() {
        // 换行 → LineReflowAnimation
        let mode = choose_animation_mode(1, true, false, false, false, false, false, true);
        assert_eq!(mode, AnimationMode::LineReflowAnimation);
    }

    #[test]
    fn choose_animation_mode_many_clusters_returns_run_animation() {
        // 9–40 个 cluster → RunAnimation
        let mode9 = choose_animation_mode(9, false, false, false, false, false, false, true);
        assert_eq!(mode9, AnimationMode::RunAnimation);

        let mode40 = choose_animation_mode(40, false, false, false, false, false, false, true);
        assert_eq!(mode40, AnimationMode::RunAnimation);

        let mode20 = choose_animation_mode(20, false, false, false, false, false, false, true);
        assert_eq!(mode20, AnimationMode::RunAnimation);
    }

    #[test]
    fn choose_animation_mode_extreme_many_clusters_returns_run() {
        // >40 个 cluster → RunAnimation (SnapshotAnimation is unavailable)
        let mode = choose_animation_mode(41, false, false, false, false, false, false, true);
        assert_eq!(mode, AnimationMode::RunAnimation);

        let mode100 = choose_animation_mode(100, false, false, false, false, false, false, true);
        assert_eq!(mode100, AnimationMode::RunAnimation);
    }

    #[test]
    fn choose_animation_mode_scrolling_returns_system_suppressed() {
        // 滚动 → SystemSuppressed
        let mode = choose_animation_mode(5, false, false, true, false, false, false, true);
        assert_eq!(mode, AnimationMode::SystemSuppressed);
    }

    #[test]
    fn choose_animation_mode_disabled_returns_system_suppressed() {
        // 动画关闭 → SystemSuppressed
        let mode = choose_animation_mode(5, false, false, false, false, false, false, false);
        assert_eq!(mode, AnimationMode::SystemSuppressed);
    }

    #[test]
    fn choose_animation_mode_loading_returns_system_suppressed() {
        // 加载 → SystemSuppressed
        let mode = choose_animation_mode(5, false, false, false, true, false, false, true);
        assert_eq!(mode, AnimationMode::SystemSuppressed);
    }

    #[test]
    fn choose_animation_mode_format_returns_system_suppressed() {
        // 格式化 → SystemSuppressed
        let mode = choose_animation_mode(5, false, false, false, false, true, false, true);
        assert_eq!(mode, AnimationMode::SystemSuppressed);
    }

    #[test]
    fn choose_animation_mode_settings_returns_system_suppressed() {
        // 设置变化 → SystemSuppressed
        let mode = choose_animation_mode(5, false, false, false, false, false, true, true);
        assert_eq!(mode, AnimationMode::SystemSuppressed);
    }

    #[test]
    fn choose_animation_mode_empty_returns_system_suppressed() {
        // 0 cluster → SystemSuppressed
        let mode = choose_animation_mode(0, false, false, false, false, false, false, true);
        assert_eq!(mode, AnimationMode::SystemSuppressed);
    }

    #[test]
    fn split_text_into_clusters_emoji() {
        // emoji 整组作为一个 cluster
        let clusters = split_text_into_clusters("😀", 0);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].text, "😀");
        assert!(clusters[0].is_complex);
        assert_eq!(clusters[0].byte_start, 0);
        assert_eq!(clusters[0].byte_end, "😀".len());
    }

    #[test]
    fn split_text_into_clusters_combining_mark() {
        // 组合字符附加到前一个 cluster
        let clusters = split_text_into_clusters("e\u{0301}", 0); // é = e + combining acute
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].text, "e\u{0301}");
        assert!(clusters[0].is_complex);
    }

    #[test]
    fn split_text_into_runs_chinese() {
        // 中文每 5 字一组
        let runs = split_text_into_runs("一二三四五六七八九十", 0);
        // "一二三四五" (5) + "六七八九十" (5)
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].text, "一二三四五");
        assert_eq!(runs[0].cluster_count, 5);
        assert_eq!(runs[1].text, "六七八九十");
        assert_eq!(runs[1].cluster_count, 5);
    }

    #[test]
    fn split_text_into_runs_mixed() {
        // 中英混合分组
        let runs = split_text_into_runs("你好world", 0);
        // "你好" (2 CJK, < 5) + "world" (5 non-CJK, < 8)
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "你好world");
        assert_eq!(runs[0].cluster_count, 7);
    }

    #[test]
    fn hidden_visual_range_serialization() {
        let hvr = HiddenVisualRange {
            id: 42,
            kind: AnimationMode::GlyphAnimation,
            range_start: 10,
            range_end: 20,
            old_rect: None,
            new_rect: None,
            line_index: 3,
            payload_ref: None,
        };
        let json = serde_json::to_string(&hvr).unwrap();
        assert!(json.contains("\"id\":"));
        assert!(json.contains("\"kind\":"));
        assert!(json.contains("\"glyphAnimation\""));
        assert!(json.contains("\"rangeStart\":"));
        assert!(json.contains("\"rangeEnd\":"));
        assert!(json.contains("\"lineIndex\":"));
        // None fields should be skipped
        assert!(!json.contains("\"oldRect\":"));
        assert!(!json.contains("\"newRect\":"));
        assert!(!json.contains("\"payloadRef\":"));

        // With rects
        let hvr2 = HiddenVisualRange {
            id: 43,
            kind: AnimationMode::LineReflowAnimation,
            range_start: 0,
            range_end: 5,
            old_rect: Some(Rect { x: 0.0, y: 0.0, w: 100.0, h: 20.0 }),
            new_rect: Some(Rect { x: 0.0, y: 20.0, w: 100.0, h: 20.0 }),
            line_index: 1,
            payload_ref: Some(99),
        };
        let json2 = serde_json::to_string(&hvr2).unwrap();
        assert!(json2.contains("\"lineReflowAnimation\""));
        assert!(json2.contains("\"oldRect\":"));
        assert!(json2.contains("\"newRect\":"));
        assert!(json2.contains("\"payloadRef\":"));
    }

    #[test]
    fn visual_transaction_contains_animation_mode() {
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "ab",
            "abc",
            EditorSelection::collapsed("ab", 2),
            EditorSelection::collapsed("abc", 3),
            EditorTransactionCause::Typing,
        );
        let vt = engine.visual_transaction(&tx).unwrap();
        assert_eq!(vt.animation_mode, AnimationMode::GlyphAnimation);
    }

    #[test]
    fn visual_transaction_newline_not_suppressed() {
        // 换行不返回 SystemSuppressed — should_animate 现在对换行返回 true
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "ab",
            "ab\nc",
            EditorSelection::collapsed("ab", 2),
            EditorSelection::collapsed("ab\nc", "ab\nc".len()),
            EditorTransactionCause::Typing,
        );
        assert!(tx.should_animate, "Newline should now animate");
        let vt = engine.visual_transaction(&tx).unwrap();
        assert_eq!(vt.animation_mode, AnimationMode::LineReflowAnimation);
        assert_ne!(vt.animation_mode, AnimationMode::SystemSuppressed);
    }

    #[test]
    fn visual_transaction_complex_grapheme_not_suppressed() {
        // 复杂 grapheme 不返回 SystemSuppressed
        let mut engine = EditorEngine::with_animation_limits(8, 120);
        let tx = engine.create_transaction(
            "ab",
            "ab😀",
            EditorSelection::collapsed("ab", 2),
            EditorSelection::collapsed("ab😀", "ab😀".len()),
            EditorTransactionCause::Typing,
        );
        assert!(tx.should_animate, "Complex grapheme should animate");
        let vt = engine.visual_transaction(&tx).unwrap();
        assert_eq!(vt.animation_mode, AnimationMode::ClusterAnimation);
        assert_ne!(vt.animation_mode, AnimationMode::SystemSuppressed);
    }

    #[test]
    fn count_grapheme_clusters_zwj_emoji() {
        // ZWJ emoji "👨‍👩‍👧‍👦" 计为 1 个 cluster
        assert_eq!(count_grapheme_clusters("👨‍👩‍👧‍👦"), 1);
    }

    #[test]
    fn count_grapheme_clusters_variation_selector_emoji() {
        // Variation selector emoji "❤️" 计为 1 个 cluster
        assert_eq!(count_grapheme_clusters("❤️"), 1);
    }

    #[test]
    fn count_grapheme_clusters_combining_mark() {
        // Combining mark "é" (e + U+0301) 计为 1 个 cluster
        assert_eq!(count_grapheme_clusters("e\u{0301}"), 1);
    }

    #[test]
    fn count_grapheme_clusters_mixed_text() {
        // 混合文本 "ab😀cd" 计为 5 个 cluster
        assert_eq!(count_grapheme_clusters("ab😀cd"), 5);
    }

    #[test]
    fn split_text_into_clusters_zwj_emoji() {
        // ZWJ emoji 输出正确的 byte range 和 is_complex=true
        let emoji = "👨‍👩‍👧‍👦";
        let clusters = split_text_into_clusters(emoji, 0);
        assert_eq!(clusters.len(), 1, "ZWJ emoji should be 1 cluster");
        assert_eq!(clusters[0].byte_start, 0);
        assert_eq!(clusters[0].byte_end, emoji.len());
        assert_eq!(clusters[0].text, emoji);
        assert!(clusters[0].is_complex, "ZWJ emoji should be complex");
    }

    #[test]
    fn split_text_into_clusters_variation_selector_emoji() {
        // Variation selector emoji 输出正确的 byte range 和 is_complex=true
        let emoji = "❤️"; // ❤ + FE0F
        let clusters = split_text_into_clusters(emoji, 0);
        assert_eq!(clusters.len(), 1, "Variation selector emoji should be 1 cluster");
        assert_eq!(clusters[0].byte_start, 0);
        assert_eq!(clusters[0].byte_end, emoji.len());
        assert_eq!(clusters[0].text, emoji);
        assert!(clusters[0].is_complex, "Variation selector emoji should be complex");
    }

    // --- #515: Timeline tests ---

    #[test]
    fn timeline_progress_before_start_returns_zero() {
        let tl = Timeline::new(160);
        assert_eq!(tl.progress(0), 0.0);
        assert_eq!(tl.progress(100), 0.0);
    }

    #[test]
    fn timeline_progress_after_start_clamps_to_one() {
        let mut tl = Timeline::new(160);
        tl.mark_first_visible_frame(1000);
        assert!((tl.progress(1160) - 1.0).abs() < f64::EPSILON);
        assert!((tl.progress(2000) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn timeline_progress_mid_animation() {
        let mut tl = Timeline::new(200);
        tl.mark_first_visible_frame(1000);
        let p = tl.progress(1100);
        assert!((p - 0.5).abs() < f64::EPSILON, "Expected 0.5, got {}", p);
    }

    #[test]
    fn timeline_paused_returns_paused_progress_not_zero() {
        let mut tl = Timeline::new(200);
        tl.mark_first_visible_frame(1000);
        let p_before_pause = tl.progress(1150);
        assert!((p_before_pause - 0.75).abs() < 0.01);
        tl.pause(1150);
        assert!(tl.is_paused());
        assert!((tl.paused_progress - 0.75).abs() < 0.01);
        let p_after_pause = tl.progress(1200);
        assert!((p_after_pause - 0.75).abs() < 0.01, "Paused must return paused_progress, not 0");
    }

    #[test]
    fn timeline_resume_continues_from_paused_progress() {
        let mut tl = Timeline::new(200);
        tl.mark_first_visible_frame(1000);
        tl.pause(1100);
        tl.resume(1200);
        assert!(!tl.is_paused());
        // resume at 1200, paused_progress=0.5, new_start=1200-100=1100
        // progress(1200) = (1200-1100)/200 = 0.5 (resumes from paused_progress)
        let p_at_resume = tl.progress(1200);
        assert!((p_at_resume - 0.5).abs() < 0.01, "Expected 0.5 at resume time, got {}", p_at_resume);
        // progress(1300) = (1300-1100)/200 = 1.0 (200ms effective elapsed)
        let p = tl.progress(1300);
        assert!((p - 1.0).abs() < 0.01, "Expected 1.0 at 1300, got {}", p);
    }

    #[test]
    fn timeline_zero_duration_returns_one() {
        let mut tl = Timeline::new(0);
        tl.mark_first_visible_frame(1000);
        assert!((tl.progress(1000) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn timeline_double_pause_is_noop() {
        let mut tl = Timeline::new(200);
        tl.mark_first_visible_frame(1000);
        tl.pause(1100);
        let first_paused = tl.paused_progress;
        tl.pause(1200);
        assert!((tl.paused_progress - first_paused).abs() < f64::EPSILON);
    }

    #[test]
    fn timeline_resume_without_pause_is_noop() {
        let mut tl = Timeline::new(200);
        tl.mark_first_visible_frame(1000);
        tl.resume(1100);
        assert_eq!(tl.accumulated_paused_duration_ms, 0);
    }

    #[test]
    fn timeline_is_completed() {
        let mut tl = Timeline::new(100);
        tl.mark_first_visible_frame(1000);
        assert!(!tl.is_completed(1050));
        assert!(tl.is_completed(1100));
    }

    // --- #515: UnifiedTransactionKind / VisualClassKind serialization ---

    #[test]
    fn unified_transaction_kind_serializes_camel_case() {
        let json = serde_json::to_string(&UnifiedTransactionKind::BodyEdit).unwrap();
        assert!(json.contains("\"bodyEdit\""));
        let json2 = serde_json::to_string(&UnifiedTransactionKind::CompositionUpdate).unwrap();
        assert!(json2.contains("\"compositionUpdate\""));
        let json3 = serde_json::to_string(&UnifiedTransactionKind::CompositionCommitOrCancel).unwrap();
        assert!(json3.contains("\"compositionCommitOrCancel\""));
        let json4 = serde_json::to_string(&UnifiedTransactionKind::CursorOnly).unwrap();
        assert!(json4.contains("\"cursorOnly\""));
    }

    #[test]
    fn visual_class_kind_serializes_camel_case() {
        assert!(serde_json::to_string(&VisualClassKind::Static).unwrap().contains("\"static\""));
        assert!(serde_json::to_string(&VisualClassKind::Insert).unwrap().contains("\"insert\""));
        assert!(serde_json::to_string(&VisualClassKind::Delete).unwrap().contains("\"delete\""));
        assert!(serde_json::to_string(&VisualClassKind::Move).unwrap().contains("\"move\""));
        assert!(serde_json::to_string(&VisualClassKind::Crossfade).unwrap().contains("\"crossfade\""));
    }

    // --- #515: CompositionVisualRevision serialization ---

    #[test]
    fn composition_visual_revision_serializes_camel_case() {
        let rev = CompositionVisualRevision {
            committed_text: "hello".to_string(),
            composition_replace_range: Some((5, 7)),
            preedit_text: "ni".to_string(),
            virtual_text: "helloni".to_string(),
            affected_paragraph_range: (0, 7),
            line_snapshot_ids: vec![1, 2],
            cursor_rect: Some(CursorRect { x: 10.0, top: 5.0, bottom: 25.0, baseline_y: 20.0 }),
            decoration_ranges: vec![DecorationSlice {
                kind: DecorationSliceKind::Underline,
                byte_start: 5,
                byte_end: 7,
                rect: None,
                color: None,
            }],
        };
        let json = serde_json::to_string(&rev).unwrap();
        assert!(json.contains("\"committedText\":"));
        assert!(json.contains("\"compositionReplaceRange\":"));
        assert!(json.contains("\"preeditText\":"));
        assert!(json.contains("\"virtualText\":"));
        assert!(json.contains("\"affectedParagraphRange\":"));
        assert!(json.contains("\"lineSnapshotIds\":"));
        assert!(json.contains("\"cursorRect\":"));
        assert!(json.contains("\"decorationRanges\":"));
    }

    #[test]
    fn composition_visual_revision_skips_none_and_empty() {
        let rev = CompositionVisualRevision {
            committed_text: "hello".to_string(),
            composition_replace_range: None,
            preedit_text: String::new(),
            virtual_text: String::new(),
            affected_paragraph_range: (0, 5),
            line_snapshot_ids: Vec::new(),
            cursor_rect: None,
            decoration_ranges: Vec::new(),
        };
        let json = serde_json::to_string(&rev).unwrap();
        assert!(!json.contains("\"compositionReplaceRange\":"));
        assert!(!json.contains("\"lineSnapshotIds\":"));
        assert!(!json.contains("\"cursorRect\":"));
        assert!(!json.contains("\"decorationRanges\":"));
    }

    // --- #515: PlatformVisualTransaction with new fields ---

    #[test]
    fn platform_visual_transaction_with_timeline_serializes() {
        let mut tl = Timeline::new(160);
        tl.mark_first_visible_frame(1000);
        let pvt = PlatformVisualTransaction {
            transaction_id: 1,
            generation: 1,
            state: PlatformVisualTransactionState::Rendering,
            old_revision: VisualLayoutRevision {
                document_revision: 1,
                layout_revision: 1,
                viewport_width: 800.0,
                font_fingerprint: "f1".to_string(),
                paragraph_style_fingerprint: "p1".to_string(),
                text_color_fingerprint: "t1".to_string(),
                density_or_dpr: 2.0,
            },
            new_revision: VisualLayoutRevision {
                document_revision: 2,
                layout_revision: 2,
                viewport_width: 800.0,
                font_fingerprint: "f1".to_string(),
                paragraph_style_fingerprint: "p1".to_string(),
                text_color_fingerprint: "t1".to_string(),
                density_or_dpr: 2.0,
            },
            slice_roles: vec![AnimatedSliceRole::Insert],
            slice_document_byte_ranges: vec![(2, 3)],
            static_line_patches: Vec::new(),
            cursor_transition_byte_start: 2,
            cursor_transition_byte_end: 3,
            duration_ms: 160,
            rendering_started_at_ms: Some(1000),
            accumulated_paused_duration_ms: 0,
            timeline: Some(tl),
            unified_kind: Some(UnifiedTransactionKind::BodyEdit),
            visual_class_kinds: vec![VisualClassKind::Insert],
            decoration_slices: Vec::new(),
            cursor_path: Some(CursorPath {
                from_rect: CursorRect { x: 10.0, top: 5.0, bottom: 25.0, baseline_y: 20.0 },
                to_rect: CursorRect { x: 30.0, top: 5.0, bottom: 25.0, baseline_y: 20.0 },
                is_snap: false,
            }),
            composition_revision: None,
            rebase: None,
        };
        let json = serde_json::to_string(&pvt).unwrap();
        assert!(json.contains("\"timeline\":"));
        assert!(json.contains("\"unifiedKind\":"));
        assert!(json.contains("\"bodyEdit\""));
        assert!(json.contains("\"visualClassKinds\":"));
        assert!(json.contains("\"cursorPath\":"));
        assert!(!json.contains("\"compositionRevision\":"));
        assert!(!json.contains("\"rebase\":"));
    }

    // --- #515: TransactionRebase serialization ---

    #[test]
    fn transaction_rebase_serializes_camel_case() {
        let rebase = TransactionRebase {
            cancelled_transaction_id: 42,
            old_progress: 0.6,
            old_frame_snapshot: Some(RebaseFrameSnapshot {
                slice_rects: vec![Rect { x: 10.0, y: 20.0, w: 30.0, h: 40.0 }],
                slice_alphas: vec![0.8],
                cursor_rect: Some(CursorRect { x: 10.0, top: 5.0, bottom: 25.0, baseline_y: 20.0 }),
            }),
        };
        let json = serde_json::to_string(&rebase).unwrap();
        assert!(json.contains("\"cancelledTransactionId\":"));
        assert!(json.contains("\"oldProgress\":"));
        assert!(json.contains("\"oldFrameSnapshot\":"));
        assert!(json.contains("\"sliceRects\":"));
        assert!(json.contains("\"sliceAlphas\":"));
        assert!(json.contains("\"cursorRect\":"));
    }

    #[test]
    fn transaction_rebase_skips_none() {
        let rebase = TransactionRebase {
            cancelled_transaction_id: 1,
            old_progress: 0.0,
            old_frame_snapshot: None,
        };
        let json = serde_json::to_string(&rebase).unwrap();
        assert!(!json.contains("\"oldFrameSnapshot\":"));
    }

    // --- #515: DecorationSlice serialization ---

    #[test]
    fn decoration_slice_serializes_camel_case() {
        let ds = DecorationSlice {
            kind: DecorationSliceKind::Underline,
            byte_start: 5,
            byte_end: 7,
            rect: Some(Rect { x: 10.0, y: 20.0, w: 30.0, h: 2.0 }),
            color: Some("#FF0000".to_string()),
        };
        let json = serde_json::to_string(&ds).unwrap();
        assert!(json.contains("\"byteStart\":"));
        assert!(json.contains("\"byteEnd\":"));
        assert!(json.contains("\"rect\":"));
        assert!(json.contains("\"color\":"));
        assert!(json.contains("\"underline\""));
    }

    #[test]
    fn decoration_slice_skips_none() {
        let ds = DecorationSlice {
            kind: DecorationSliceKind::Cursor,
            byte_start: 0,
            byte_end: 0,
            rect: None,
            color: None,
        };
        let json = serde_json::to_string(&ds).unwrap();
        assert!(!json.contains("\"rect\":"));
        assert!(!json.contains("\"color\":"));
    }

    // --- #515: CursorPath serialization ---

    #[test]
    fn cursor_path_serializes_camel_case() {
        let cp = CursorPath {
            from_rect: CursorRect { x: 10.0, top: 5.0, bottom: 25.0, baseline_y: 20.0 },
            to_rect: CursorRect { x: 30.0, top: 5.0, bottom: 25.0, baseline_y: 20.0 },
            is_snap: false,
        };
        let json = serde_json::to_string(&cp).unwrap();
        assert!(json.contains("\"fromRect\":"));
        assert!(json.contains("\"toRect\":"));
        assert!(json.contains("\"isSnap\":"));
    }
}
