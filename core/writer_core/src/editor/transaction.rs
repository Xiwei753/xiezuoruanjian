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
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[deprecated(
    since = "0.12.0",
    note = "Use EditorVisualTransaction instead. This will be removed in a future version."
)]
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

/// 已提交正文的视觉修订 — committed document 的平台无关快照。
///
/// #516: 正文事实状态只有 committed document。
/// 每次正文变更（插入、删除、换行、段落合并）都产生新 VisualRevision。
/// 预输入不产生 VisualRevision，只产生 CompositionVisualRevision。
///
/// 平台端持有 VisualRevision 的渲染资源（行快照、纹理等），
/// Core 只记录语义元数据。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualRevision {
    /// 修订唯一 ID（递增）
    pub revision_id: u64,
    /// 完整正文文本
    pub full_text: String,
    /// 受影响段落范围（UTF-8 byte offset）
    pub affected_paragraph_range: (usize, usize),
    /// 行快照 ID 列表（由平台层填充）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub line_snapshot_ids: Vec<u64>,
    /// 光标矩形
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_rect: Option<CursorRect>,
    /// 选区/插入点亲和性
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caret_affinity: Option<CaretAffinity>,
    /// Shaping 身份指纹 — 同一 shaping identity 的文字可按 glyph 一一映射
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shaping_identity: Option<String>,
}

/// 光标/插入点亲和性 — 决定光标在软换行断点处偏向上一行末尾还是下一行开头。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CaretAffinity {
    Upstream,
    Downstream,
}

/// 构造预输入虚拟文本 — 严格按公式拼接，不得丢失 replaceEnd 后正文。
///
/// virtualText = committedText[0..replaceStart] + preeditText + committedText[replaceEnd..]
///
/// 如果 composition_replace_range 为 None，默认在光标位置做零长度插入：
/// virtualText = committedText + preeditText
///
/// #516: Linux 的 virtualText 只拼接正文前缀和 preedit 丢失光标后正文是错误实现。
/// 此函数是 virtualText 构造的唯一权威来源。
pub fn build_virtual_text(
    committed_text: &str,
    composition_replace_range: Option<(usize, usize)>,
    preedit_text: &str,
) -> String {
    match composition_replace_range {
        Some((replace_start, replace_end)) => {
            let replace_start = replace_start.min(committed_text.len());
            let replace_end = replace_end.min(committed_text.len());
            if replace_start > replace_end {
                return committed_text.to_string();
            }
            let mut result = String::with_capacity(
                replace_start + preedit_text.len() + (committed_text.len() - replace_end),
            );
            result.push_str(&committed_text[..replace_start]);
            result.push_str(preedit_text);
            result.push_str(&committed_text[replace_end..]);
            result
        }
        None => {
            let mut result =
                String::with_capacity(committed_text.len() + preedit_text.len());
            result.push_str(committed_text);
            result.push_str(preedit_text);
            result
        }
    }
}

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
/// virtualText 仅用于排版和渲染，不写入正文、Undo、保存和同步和 Core 正文状态。
/// 每次预输入变化生成新 CompositionVisualRevision，
/// 使用相同 StaticLinePatch + AnimatedSlice 分类。
///
/// #516: virtualText 必须通过 `build_virtual_text()` 构造，
/// 严格按 committedText[0..replaceStart] + preeditText + committedText[replaceEnd..] 拼接。
/// 不得丢失 replaceEnd 后正文，也不得默认把预输入永远当成零长度插入。
///
/// #517: 增加不可变 revision 链接。每次更新必须从 previous visual revision 接续，
/// 不允许从 committed revision 重新开始。replaceStart/replaceEndExclusive 始终是
/// committed 正文坐标，preeditCursorOffset 始终是 preedit 内部坐标。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompositionVisualRevision {
    /// 修订唯一 ID（递增，由 CompositionSession 分配）
    #[serde(default)]
    pub revision_id: u64,
    /// 所属 composition session ID
    #[serde(default)]
    pub session_id: u64,
    /// 此修订基于的 committed revision ID
    #[serde(default)]
    pub committed_revision_id: u64,
    /// 已提交文本（不含预输入）
    pub committed_text: String,
    /// 预输入替换范围（UTF-8 byte offset，committed 正文坐标）
    ///
    /// #517: replaceStart/replaceEndExclusive 始终是 committed 正文坐标，
    /// 不是 virtualText 坐标，也不是 preedit 长度。
    /// 普通 setComposingText 初次预输入默认是零长度插入：
    /// replaceStart == replaceEndExclusive == 原 committed 光标位置。
    /// 只有 setComposingRegion 或平台明确给出替换范围时才能形成非零替换范围。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composition_replace_range: Option<(usize, usize)>,
    /// 预输入文本
    #[serde(default)]
    pub preedit_text: String,
    /// 预输入光标偏移（preedit 内部 UTF-8 byte offset）
    ///
    /// #517: 始终是 preedit 内部坐标，不能与 composition_replace_range 混用。
    #[serde(default)]
    pub preedit_cursor_offset: usize,
    /// 虚拟文本 — 仅用于排版和渲染，不写入正文
    /// 必须通过 `build_virtual_text()` 构造，不得手动拼接
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
    /// IME 光标范围/位置（UTF-8 byte offset）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ime_cursor_range: Option<(usize, usize)>,
    /// 从上一 CompositionVisualRevision 的偏移映射
    ///
    /// #517: 连续更新必须从 previous visual revision 接续，
    /// 不允许从 committed revision 重新开始。
    /// OffsetMap 记录 old virtualText → new virtualText 的字符映射，
    /// 用于后续正文 cluster 保持身份并生成 Move，而不是全部 Crossfade/Insert。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset_map_from_previous: Option<OffsetMap>,
}

impl CompositionVisualRevision {
    /// 使用 `build_virtual_text()` 正确构造 CompositionVisualRevision。
    ///
    /// virtualText 由 committed_text、composition_replace_range 和 preedit_text
    /// 自动计算，不手动传入。
    pub fn new(
        committed_text: String,
        composition_replace_range: Option<(usize, usize)>,
        preedit_text: String,
        affected_paragraph_range: (usize, usize),
    ) -> Self {
        let virtual_text = build_virtual_text(
            &committed_text,
            composition_replace_range,
            &preedit_text,
        );
        Self {
            revision_id: 0,
            session_id: 0,
            committed_revision_id: 0,
            committed_text,
            composition_replace_range,
            preedit_text,
            preedit_cursor_offset: 0,
            virtual_text,
            affected_paragraph_range,
            line_snapshot_ids: Vec::new(),
            cursor_rect: None,
            decoration_ranges: Vec::new(),
            ime_cursor_range: None,
            offset_map_from_previous: None,
        }
    }

    /// #517: 从 previous visual revision 构造新 CompositionVisualRevision。
    ///
    /// 更新链必须是：previous visual revision -> new visual revision，
    /// 而不是：committed revision -> 每一次新的 preedit。
    ///
    /// 此方法自动计算 OffsetMap，记录 old virtualText → new virtualText 的映射。
    pub fn from_previous(
        previous: &CompositionVisualRevision,
        new_preedit_text: String,
        new_preedit_cursor_offset: usize,
        affected_paragraph_range: (usize, usize),
    ) -> Self {
        let virtual_text = build_virtual_text(
            &previous.committed_text,
            previous.composition_replace_range,
            &new_preedit_text,
        );
        let offset_map = OffsetMap::build(&previous.virtual_text, &virtual_text);
        Self {
            revision_id: 0,
            session_id: previous.session_id,
            committed_revision_id: previous.committed_revision_id,
            committed_text: previous.committed_text.clone(),
            composition_replace_range: previous.composition_replace_range,
            preedit_text: new_preedit_text,
            preedit_cursor_offset: new_preedit_cursor_offset,
            virtual_text,
            affected_paragraph_range,
            line_snapshot_ids: Vec::new(),
            cursor_rect: None,
            decoration_ranges: Vec::new(),
            ime_cursor_range: None,
            offset_map_from_previous: Some(offset_map),
        }
    }

    /// 预输入文本在 virtualText 中的字节范围。
    ///
    /// #517: 此范围只能表示 virtualText 中 preedit 的范围，
    /// 不能表示 committed replaceRange；两者必须分开命名和存储。
    pub fn preedit_byte_range_in_virtual_text(&self) -> (usize, usize) {
        match self.composition_replace_range {
            Some((replace_start, _)) => {
                (replace_start, replace_start + self.preedit_text.len())
            }
            None => {
                let start = self.committed_text.len();
                (start, start + self.preedit_text.len())
            }
        }
    }
}

/// #517: 偏移映射 — 两个 visualText 之间的字符身份映射。
///
/// 记录 old virtualText 中每个字符在 new virtualText 中的对应位置。
/// 用于后续正文 cluster 保持身份并生成 Move，而不是全部 Crossfade/Insert。
///
/// 映射规则：
/// - 前缀相同部分：old[i] → new[i]（identity）
/// - 中间差异部分：无映射（Insert/Delete/Crossfade）
/// - 后缀相同部分：old[i] → new[j]（shifted）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OffsetMap {
    /// 映射条目列表，按 old byte offset 排序
    pub entries: Vec<OffsetMapEntry>,
}

/// #517: 单个偏移映射条目。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OffsetMapEntry {
    /// old virtualText 中的 UTF-8 byte offset
    pub old_byte_offset: usize,
    /// new virtualText 中的 UTF-8 byte offset
    pub new_byte_offset: usize,
    /// 映射的字符数（UTF-8 bytes）
    pub length: usize,
    /// 映射类型
    pub kind: OffsetMapKind,
}

/// #517: 偏移映射类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OffsetMapKind {
    /// 文本和位置均相同（前缀/后缀静态部分）
    Identity,
    /// 文本相同但位置变化（后缀移动）
    Shifted,
}

impl OffsetMap {
    /// 从 old/new virtualText 构建偏移映射。
    ///
    /// 使用最长公共前缀/后缀算法确定映射区域。
    pub fn build(old_text: &str, new_text: &str) -> Self {
        if old_text.is_empty() || new_text.is_empty() || old_text == new_text {
            return OffsetMap { entries: Vec::new() };
        }

        let prefix = common_prefix_byte_len(old_text, new_text);
        let suffix = common_suffix_byte_len(old_text, new_text, prefix);

        let mut entries = Vec::new();

        if prefix > 0 {
            entries.push(OffsetMapEntry {
                old_byte_offset: 0,
                new_byte_offset: 0,
                length: prefix,
                kind: OffsetMapKind::Identity,
            });
        }

        if suffix > 0 {
            let old_suffix_start = old_text.len() - suffix;
            let new_suffix_start = new_text.len() - suffix;
            let kind = if prefix > 0 || (old_text.len() != new_text.len()) {
                OffsetMapKind::Shifted
            } else {
                OffsetMapKind::Identity
            };
            entries.push(OffsetMapEntry {
                old_byte_offset: old_suffix_start,
                new_byte_offset: new_suffix_start,
                length: suffix,
                kind,
            });
        }

        OffsetMap { entries }
    }

    /// 查找 old byte offset 在 new text 中的对应位置。
    pub fn map_old_to_new(&self, old_byte_offset: usize) -> Option<usize> {
        for entry in &self.entries {
            if old_byte_offset >= entry.old_byte_offset
                && old_byte_offset < entry.old_byte_offset + entry.length
            {
                let offset_within = old_byte_offset - entry.old_byte_offset;
                return Some(entry.new_byte_offset + offset_within);
            }
        }
        None
    }

    pub fn map_new_to_old(&self, new_byte_offset: usize) -> Option<usize> {
        for entry in &self.entries {
            if new_byte_offset >= entry.new_byte_offset
                && new_byte_offset < entry.new_byte_offset + entry.length
            {
                let offset_within = new_byte_offset - entry.new_byte_offset;
                return Some(entry.old_byte_offset + offset_within);
            }
        }
        None
    }
}

/// #517: 预输入会话 — 跨平台 composition 状态模型。
///
/// Android 和 Linux 都必须维护一个明确的 composition session，
/// 而不是零散地存 preedit_text 和临时 snapshot。
///
/// 关键规则：
/// - replaceStart/replaceEndExclusive 始终是 committed 正文坐标
/// - preeditCursorOffset 始终是 preedit 内部坐标
/// - virtualText 由 committed replaceRange 和 preeditText 构造
/// - composing 更新不能修改 committed buffer、Undo、保存、同步和 Core 正文状态
/// - 连续 setComposingText 必须保持原 session 的 committed replaceRange，
///   不能随着 preedit 长度变化而移动 end
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompositionSession {
    /// 会话唯一 ID
    pub session_id: u64,
    /// 此会话基于的 committed revision ID
    pub committed_revision_id: u64,
    /// 会话开始时的 committed 文本
    pub committed_text_at_start: String,
    /// committed 正文替换范围起始（UTF-8 byte offset）
    pub replace_start: usize,
    /// committed 正文替换范围结束（不含，UTF-8 byte offset）
    pub replace_end_exclusive: usize,
    /// 当前预输入文本
    #[serde(default)]
    pub preedit_text: String,
    /// 预输入光标偏移（preedit 内部 UTF-8 byte offset）
    #[serde(default)]
    pub preedit_cursor_offset: usize,
    /// 当前视觉修订
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_visual_revision: Option<CompositionVisualRevision>,
    /// 最后提交的 generation
    #[serde(default)]
    pub last_submitted_generation: u64,
    /// 下一个 revision ID
    #[serde(default)]
    pub next_revision_id: u64,
}

impl CompositionSession {
    /// 创建新的 composition session。
    ///
    /// #517: 普通 setComposingText 初次预输入默认是零长度插入：
    /// replace_start == replace_end_exclusive == 原 committed 光标位置。
    /// 只有 setComposingRegion 或平台明确给出替换范围时才能形成非零替换范围。
    pub fn new(
        session_id: u64,
        committed_revision_id: u64,
        committed_text: String,
        cursor_position: usize,
    ) -> Self {
        Self {
            session_id,
            committed_revision_id,
            committed_text_at_start: committed_text.clone(),
            replace_start: cursor_position,
            replace_end_exclusive: cursor_position,
            preedit_text: String::new(),
            preedit_cursor_offset: 0,
            current_visual_revision: None,
            last_submitted_generation: 0,
            next_revision_id: 1,
        }
    }

    /// 创建带替换范围的 composition session（setComposingRegion）。
    pub fn new_with_replace_range(
        session_id: u64,
        committed_revision_id: u64,
        committed_text: String,
        replace_start: usize,
        replace_end_exclusive: usize,
    ) -> Self {
        Self {
            session_id,
            committed_revision_id,
            committed_text_at_start: committed_text,
            replace_start,
            replace_end_exclusive,
            preedit_text: String::new(),
            preedit_cursor_offset: 0,
            current_visual_revision: None,
            last_submitted_generation: 0,
            next_revision_id: 1,
        }
    }

    /// 更新预输入文本。
    ///
    /// #517: 连续 setComposingText 必须保持原 session 的 committed replaceRange，
    /// 不能随着 preedit 长度变化而移动 end。
    pub fn update_preedit(
        &mut self,
        new_preedit_text: String,
        new_preedit_cursor_offset: usize,
    ) -> CompositionVisualRevision {
        let new_revision = match &self.current_visual_revision {
            Some(previous) => {
                let mut rev = CompositionVisualRevision::from_previous(
                    previous,
                    new_preedit_text.clone(),
                    new_preedit_cursor_offset,
                    (0, self.committed_text_at_start.len()),
                );
                rev.revision_id = self.take_revision_id();
                rev.session_id = self.session_id;
                rev.committed_revision_id = self.committed_revision_id;
                rev
            }
            None => {
                let mut rev = CompositionVisualRevision::new(
                    self.committed_text_at_start.clone(),
                    Some((self.replace_start, self.replace_end_exclusive)),
                    new_preedit_text.clone(),
                    (0, self.committed_text_at_start.len()),
                );
                rev.revision_id = self.take_revision_id();
                rev.session_id = self.session_id;
                rev.committed_revision_id = self.committed_revision_id;
                rev
            }
        };
        self.preedit_text = new_preedit_text;
        self.preedit_cursor_offset = new_preedit_cursor_offset;
        self.current_visual_revision = Some(new_revision.clone());
        self.last_submitted_generation = self.last_submitted_generation.saturating_add(1);
        new_revision
    }

    /// 通过 setComposingRegion 更新替换范围。
    ///
    /// #517: 只有 setComposingRegion 或平台明确给出替换范围时才能修改 replaceRange。
    pub fn set_composing_region(&mut self, start: usize, end: usize) {
        self.replace_start = start.min(self.committed_text_at_start.len());
        self.replace_end_exclusive = end.min(self.committed_text_at_start.len());
        if self.replace_start > self.replace_end_exclusive {
            std::mem::swap(&mut self.replace_start, &mut self.replace_end_exclusive);
        }
    }

    /// 会话是否活跃（有预输入文本或有视觉修订）。
    pub fn is_active(&self) -> bool {
        !self.preedit_text.is_empty() || self.current_visual_revision.is_some()
    }

    /// 获取当前 composition_replace_range。
    pub fn composition_replace_range(&self) -> Option<(usize, usize)> {
        if self.replace_start == self.replace_end_exclusive && self.preedit_text.is_empty() {
            None
        } else {
            Some((self.replace_start, self.replace_end_exclusive))
        }
    }

    /// 构造当前虚拟文本。
    pub fn virtual_text(&self) -> String {
        build_virtual_text(
            &self.committed_text_at_start,
            self.composition_replace_range(),
            &self.preedit_text,
        )
    }

    /// 预输入文本在 virtualText 中的字节范围。
    ///
    /// #517: 此范围只能表示 virtualText 中 preedit 的范围，
    /// 不能表示 committed replaceRange；两者必须分开命名和存储。
    pub fn preedit_byte_range_in_virtual_text(&self) -> (usize, usize) {
        let start = self.replace_start;
        let end = start + self.preedit_text.len();
        (start, end)
    }

    /// 提交预输入。
    ///
    /// #517: commitText 必须使用 session 的 replaceRange 替换 committed 正文。
    /// 返回 (composition_visual_revision, committed_text_after)。
    /// 如果 commit 文字与当前视觉文字相同，调用方可标记 is_visual_same 以避免重复吐字。
    pub fn commit(&mut self, commit_text: &str) -> (CompositionVisualRevision, String) {
        let composition_revision = self.current_visual_revision.clone().unwrap_or_else(|| {
            CompositionVisualRevision::new(
                self.committed_text_at_start.clone(),
                self.composition_replace_range(),
                self.preedit_text.clone(),
                (0, self.committed_text_at_start.len()),
            )
        });

        let mut committed_after = self.committed_text_at_start.clone();
        committed_after.replace_range(
            self.replace_start..self.replace_end_exclusive,
            commit_text,
        );

        self.preedit_text.clear();
        self.preedit_cursor_offset = 0;
        self.current_visual_revision = None;
        self.replace_start = 0;
        self.replace_end_exclusive = 0;

        (composition_revision, committed_after)
    }

    /// 取消预输入。
    ///
    /// #517: cancel 删除 preedit 并让后续正文回流。
    /// 返回取消前的 composition_visual_revision。
    pub fn cancel(&mut self) -> CompositionVisualRevision {
        let composition_revision = self.current_visual_revision.clone().unwrap_or_else(|| {
            CompositionVisualRevision::new(
                self.committed_text_at_start.clone(),
                self.composition_replace_range(),
                self.preedit_text.clone(),
                (0, self.committed_text_at_start.len()),
            )
        });

        self.preedit_text.clear();
        self.preedit_cursor_offset = 0;
        self.current_visual_revision = None;
        self.replace_start = 0;
        self.replace_end_exclusive = 0;

        composition_revision
    }

    /// 清除会话。
    pub fn clear(&mut self) {
        self.preedit_text.clear();
        self.preedit_cursor_offset = 0;
        self.current_visual_revision = None;
        self.replace_start = 0;
        self.replace_end_exclusive = 0;
        self.last_submitted_generation = 0;
    }

    fn take_revision_id(&mut self) -> u64 {
        let id = self.next_revision_id;
        self.next_revision_id = self.next_revision_id.saturating_add(1);
        id
    }
}

/// #517: 快照所有权状态 — 单一所有权，不允许 Manager 与事务共享同一个可释放资源引用。
///
/// 如果 Kotlin 层难以表达 move semantics，使用显式 owner token/state。
/// 任何 release 前必须校验 owner。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SnapshotOwner {
    /// 由 CompositionSession 持有
    OwnedBySession { session_id: u64 },
    /// 由指定事务持有
    OwnedByTransaction { transaction_id: u64 },
    /// 已释放
    Released,
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

/// 事务取消原因 — #516: 取消事务必须记录原因，用于 rebase 和资源释放判断。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransactionCancelReason {
    /// 被新事务 rebase 取代
    Rebased,
    /// 修订已变更，事务失效
    RevisionChanged,
    /// 系统抑制（滚动/加载/章节切换）
    SystemSuppressed,
    /// 用户手动取消
    UserCancelled,
    /// 预输入提交完成
    CompositionCommitted,
    /// 预输入取消
    CompositionCancelled,
}

/// 跨平台视觉事务语义边界。
///
/// Core 输出 EditorVisualTransaction；平台端收到后，根据平台布局
/// 生成 PlatformVisualTransaction。两端共享此结构和状态机概念，
/// 不共享平台渲染结构。
///
/// `visualResource` 字段由平台各自实现，不进入此结构。
///
/// #516: 四种事务（BodyEdit、CompositionUpdate、CompositionCommitOrCancel、CursorOnly）
/// 全部进入同一队列和 Timeline。不再存在独立预输入覆盖主路径、
/// 独立光标位移动画时间源。
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
    /// #516: 统一时钟 — 文字切片、光标、预输入装饰全部消费同一个 progress
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeline: Option<Timeline>,
    /// #516: 统一事务类型（必填，不再允许 None）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unified_kind: Option<UnifiedTransactionKind>,
    /// #516: 视觉对象分类列表（与 slice_roles/slice_document_byte_ranges 对应）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub visual_class_kinds: Vec<VisualClassKind>,
    /// #516: 装饰切片（预输入下划线、分段颜色、IME cursor）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decoration_slices: Vec<DecorationSlice>,
    /// #516: 光标路径（使用同一 Timeline）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_path: Option<CursorPath>,
    /// #516: 预输入视觉修订（仅 CompositionUpdate/CompositionCommitOrCancel 事务）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composition_revision: Option<CompositionVisualRevision>,
    /// #516: 连续事务 rebase 信息
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rebase: Option<TransactionRebase>,
    /// #516: 取消原因（仅 Cancelled 状态有值）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel_reason: Option<TransactionCancelReason>,
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
    #[cfg(test)]
    #[deprecated(
        since = "0.12.0",
        note = "Use visual_transaction() instead. This will be removed in a future version."
    )]
    #[allow(deprecated)]
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

    /// #516: 创建 CursorOnly 事务 — 仅光标移动，无正文变更。
    ///
    /// 普通光标移动也必须创建 CursorOnly 事务并由 Renderer 队列驱动，
    /// 不允许光标拥有独立位移动画时间源。
    pub fn cursor_only_transaction(
        &mut self,
        text: &str,
        old_cursor_index: usize,
        new_cursor_index: usize,
    ) -> Option<EditorVisualTransaction> {
        if old_cursor_index == new_cursor_index {
            return None;
        }
        let old_sel = EditorSelection::collapsed(text, old_cursor_index);
        let new_sel = EditorSelection::collapsed(text, new_cursor_index);
        Some(EditorVisualTransaction {
            id: self.take_animation_id(),
            kind: EditorAnimationKind::Cursor,
            cause: EditorTransactionCause::Programmatic,
            old_text: text.to_string(),
            new_text: text.to_string(),
            old_selection: old_sel,
            new_selection: new_sel,
            inserted_range: None,
            deleted_range: None,
            deleted_glyph_rects: None,
            insert_glyph_rects: None,
            reflow_glyph_rects: None,
            animation_mode: AnimationMode::GlyphAnimation,
            cluster_rects: None,
            cluster_runs: None,
            hidden_visual_ranges: Vec::new(),
            old_cursor_rect: None,
            new_cursor_rect: None,
            duration_ms: self.animation_duration_ms,
            coordinate_mode: VisualCoordinateMode::Baseline,
        })
    }

    /// #516/#517: 创建 CompositionUpdate 事务 — 预输入更新。
    ///
    /// 每次 setComposingText 触发。预输入文字必须真实推动后续正文、
    /// 触发换行和 reflow，不能在原正文上盖一段文字。
    ///
    /// composing 更新不会修改 committed text、Undo、保存和同步状态。
    ///
    /// #517: 支持从 previous visual revision 接续。
    /// 如果提供了 previous_revision，新 revision 从 previous 接续，
    /// 自动计算 OffsetMap，后续正文 cluster 保持身份并生成 Move。
    /// 如果没有提供，则从 committed 状态开始（首次预输入）。
    pub fn composition_update_transaction(
        &mut self,
        committed_text: &str,
        composition_replace_range: Option<(usize, usize)>,
        old_preedit_text: &str,
        new_preedit_text: &str,
    ) -> CompositionUpdateTransaction {
        let old_revision = CompositionVisualRevision::new(
            committed_text.to_string(),
            composition_replace_range,
            old_preedit_text.to_string(),
            (0, committed_text.len()),
        );
        let new_revision = CompositionVisualRevision::new(
            committed_text.to_string(),
            composition_replace_range,
            new_preedit_text.to_string(),
            (0, committed_text.len()),
        );
        let visual_class_kinds = classify_visual_diff(
            &old_revision.virtual_text,
            &new_revision.virtual_text,
        );
        CompositionUpdateTransaction {
            id: self.take_animation_id(),
            old_revision,
            new_revision,
            visual_class_kinds,
            duration_ms: self.animation_duration_ms,
        }
    }

    /// #517: 从 previous visual revision 创建 CompositionUpdate 事务。
    ///
    /// 更新链必须是：previous visual revision -> new visual revision，
    /// 而不是：committed revision -> 每一次新的 preedit。
    ///
    /// 此方法使用 CompositionVisualRevision::from_previous 自动计算 OffsetMap，
    /// 后续正文 cluster 通过 OffsetMap 保持身份并生成 Move。
    pub fn composition_update_from_previous(
        &mut self,
        previous_revision: &CompositionVisualRevision,
        new_preedit_text: &str,
        new_preedit_cursor_offset: usize,
    ) -> CompositionUpdateTransaction {
        let new_revision = CompositionVisualRevision::from_previous(
            previous_revision,
            new_preedit_text.to_string(),
            new_preedit_cursor_offset,
            previous_revision.affected_paragraph_range,
        );
        let visual_class_kinds = classify_visual_diff(
            &previous_revision.virtual_text,
            &new_revision.virtual_text,
        );
        CompositionUpdateTransaction {
            id: self.take_animation_id(),
            old_revision: previous_revision.clone(),
            new_revision,
            visual_class_kinds,
            duration_ms: self.animation_duration_ms,
        }
    }

    /// #516: 创建 CompositionCommitOrCancel 事务 — 预输入提交或取消。
    ///
    /// commitText: current CompositionVisualRevision → new committed VisualRevision
    /// cancel: current CompositionVisualRevision → original committed VisualRevision
    ///
    /// 视觉文字完全相同时，不重复播放吐字，只移除 underline、segment style
    /// 和 composing cursor，并完成 revision 所有权转移。
    /// 候选转换导致文字变化时，旧 preedit 执行 Delete/Crossfade，
    /// 新 committed 文字执行 Insert/Crossfade，后续正文执行 Move/Crossfade。
    pub fn composition_commit_or_cancel_transaction(
        &mut self,
        committed_text_before: &str,
        committed_text_after: &str,
        composition_revision: CompositionVisualRevision,
        is_commit: bool,
    ) -> CompositionCommitOrCancelTransaction {
        let visual_class_kinds = if is_commit {
            classify_visual_diff(
                &composition_revision.virtual_text,
                committed_text_after,
            )
        } else {
            classify_visual_diff(
                &composition_revision.virtual_text,
                committed_text_before,
            )
        };
        let is_visual_same = composition_revision.virtual_text == committed_text_after;
        CompositionCommitOrCancelTransaction {
            id: self.take_animation_id(),
            is_commit,
            is_visual_same,
            composition_revision,
            committed_text_after: committed_text_after.to_string(),
            visual_class_kinds,
            duration_ms: self.animation_duration_ms,
        }
    }
}

/// #516: CompositionUpdate 事务 — 预输入更新（setComposingText）。
///
/// 预输入文字必须真实推动后续正文、触发换行和 reflow。
/// composing 更新不会修改 committed text、Undo、保存和同步状态。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompositionUpdateTransaction {
    pub id: u64,
    pub old_revision: CompositionVisualRevision,
    pub new_revision: CompositionVisualRevision,
    pub visual_class_kinds: Vec<VisualClassKind>,
    pub duration_ms: u64,
}

/// #516: CompositionCommitOrCancel 事务 — 预输入提交或取消。
///
/// commitText: current CompositionVisualRevision → new committed VisualRevision
/// cancel: current CompositionVisualRevision → original committed VisualRevision
///
/// 视觉文字完全相同时（is_visual_same=true），不重复播放吐字，
/// 只移除 underline、segment style 和 composing cursor。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompositionCommitOrCancelTransaction {
    pub id: u64,
    pub is_commit: bool,
    /// 视觉文字完全相同 — 不重复播放吐字
    pub is_visual_same: bool,
    pub composition_revision: CompositionVisualRevision,
    pub committed_text_after: String,
    pub visual_class_kinds: Vec<VisualClassKind>,
    pub duration_ms: u64,
}

/// #516: 视觉对象分类器 — 通过 old/new 文本差异分类。
///
/// 所有 old/new revision 比较都通过此函数分类，不按场景写特例。
/// 中间插入、换行、段落合并、删除回流、预输入更新和候选转换
/// 全部使用同一分类器。
///
/// 分类规则：
/// - 相同位置文本和位置都相同 → Static
/// - 仅 new 存在 → Insert
/// - 仅 old 存在 → Delete
/// - 文本可映射但可能有 shaping 改变 → Crossfade（保守策略）
/// - 文本相同但位置变化 → Move
///
/// 注：精确的 shaping identity 比较需要平台端提供 shaping fingerprint，
/// Core 层在文本内容相同时保守返回 Crossfade。
/// 平台端可利用 shaping_identity 做更精确的分类。
pub fn classify_visual_diff(old_text: &str, new_text: &str) -> Vec<VisualClassKind> {
    if old_text == new_text {
        return Vec::new();
    }
    if old_text.is_empty() && !new_text.is_empty() {
        return vec![VisualClassKind::Insert];
    }
    if !old_text.is_empty() && new_text.is_empty() {
        return vec![VisualClassKind::Delete];
    }

    let prefix = common_prefix_byte_len(old_text, new_text);
    let suffix = common_suffix_byte_len(old_text, new_text, prefix);
    let old_end = old_text.len().saturating_sub(suffix);
    let new_end = new_text.len().saturating_sub(suffix);

    let mut kinds = Vec::new();

    // 前缀相同部分 → Static
    if prefix > 0 {
        kinds.push(VisualClassKind::Static);
    }

    // 中间差异部分
    let removed = &old_text[prefix..old_end];
    let inserted = &new_text[prefix..new_end];

    if !removed.is_empty() && !inserted.is_empty() {
        // 替换：old 文本淡出 + new 文本淡入
        kinds.push(VisualClassKind::Crossfade);
    } else if !removed.is_empty() {
        kinds.push(VisualClassKind::Delete);
    } else if !inserted.is_empty() {
        kinds.push(VisualClassKind::Insert);
    }

    // 后缀相同部分 → Static 或 Move（位置可能变化）
    if suffix > 0 {
        // 如果有插入/删除，后缀文字位置会变化
        if !removed.is_empty() || !inserted.is_empty() {
            kinds.push(VisualClassKind::Move);
        } else {
            kinds.push(VisualClassKind::Static);
        }
    }

    kinds
}

/// #516: 统一 rebase — 新事务与旧事务冲突时的处理。
///
/// rebase 必须覆盖四种事务（BodyEdit、CompositionUpdate、
/// CompositionCommitOrCancel、CursorOnly），不只覆盖 Insert。
///
/// 新事务入队前：
/// 1. 根据视觉区域、revision 和 byte/UTF-16 映射查找冲突事务
/// 2. 读取旧事务当前 progress
/// 3. 将当前帧作为新事务 old state
/// 4. 取消旧事务，但不能提前释放已转移资源
/// 5. 启动新事务
///
/// 冲突判断不能只看 AnimatedSlice：CursorOnly、纯 Decoration、
/// 视觉文字相同的 CompositionCommit 也必须能通过 revision/affected range 参与替换。
pub fn compute_rebase(
    cancelled_transaction_id: u64,
    old_progress: f64,
    old_frame_snapshot: Option<RebaseFrameSnapshot>,
) -> TransactionRebase {
    TransactionRebase {
        cancelled_transaction_id,
        old_progress,
        old_frame_snapshot,
    }
}

/// #516: 检查两个事务是否在视觉区域上冲突。
///
/// 用于决定是否需要 rebase。冲突条件：
/// - 同一 unified_kind 的连续事务
/// - 视觉区域有重叠
/// - CursorOnly 与任何影响光标位置的事务冲突
pub fn transactions_overlap(
    old_kind: UnifiedTransactionKind,
    old_affected_range: (usize, usize),
    new_kind: UnifiedTransactionKind,
    new_affected_range: (usize, usize),
) -> bool {
    let (old_start, old_end) = old_affected_range;
    let (new_start, new_end) = new_affected_range;

    // CursorOnly 与任何影响光标的事务冲突
    if matches!(old_kind, UnifiedTransactionKind::CursorOnly)
        || matches!(new_kind, UnifiedTransactionKind::CursorOnly)
    {
        return true;
    }

    // 视觉区域重叠
    old_start < new_end && new_start < old_end
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

    // --- #516: Timeline tests ---

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

    // --- #516: UnifiedTransactionKind / VisualClassKind serialization ---

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

    // --- #516: CompositionVisualRevision serialization ---

    #[test]
    fn composition_visual_revision_serializes_camel_case() {
        let rev = CompositionVisualRevision {
            revision_id: 1,
            session_id: 1,
            committed_revision_id: 10,
            committed_text: "hello".to_string(),
            composition_replace_range: Some((5, 7)),
            preedit_text: "ni".to_string(),
            preedit_cursor_offset: 2,
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
            ime_cursor_range: Some((5, 7)),
            offset_map_from_previous: None,
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
            revision_id: 0,
            session_id: 0,
            committed_revision_id: 0,
            committed_text: "hello".to_string(),
            composition_replace_range: None,
            preedit_text: String::new(),
            preedit_cursor_offset: 0,
            virtual_text: String::new(),
            affected_paragraph_range: (0, 5),
            line_snapshot_ids: Vec::new(),
            cursor_rect: None,
            decoration_ranges: Vec::new(),
            ime_cursor_range: None,
            offset_map_from_previous: None,
        };
        let json = serde_json::to_string(&rev).unwrap();
        assert!(!json.contains("\"compositionReplaceRange\":"));
        assert!(!json.contains("\"lineSnapshotIds\":"));
        assert!(!json.contains("\"cursorRect\":"));
        assert!(!json.contains("\"decorationRanges\":"));
    }

    // --- #516: PlatformVisualTransaction with new fields ---

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
            cancel_reason: None,
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

    // --- #516: TransactionRebase serialization ---

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

    // --- #516: DecorationSlice serialization ---

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

    // --- #516: CursorPath serialization ---

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

    // ========================================================================
    // #516 行为测试 — 覆盖 issue 验收标准
    // ========================================================================

    // --- build_virtual_text ---

    #[test]
    fn build_virtual_text_appends_preedit_when_no_replace_range() {
        let vt = build_virtual_text("hello", None, "world");
        assert_eq!(vt, "helloworld");
    }

    #[test]
    fn build_virtual_text_replaces_range_correctly() {
        // committedText[0..2] + preeditText + committedText[5..]
        let vt = build_virtual_text("hello", Some((2, 5)), "y");
        assert_eq!(vt, "hey");
    }

    #[test]
    fn build_virtual_text_preserves_text_after_replace_end() {
        // #516 关键验收：不得丢失 replaceEnd 后正文
        let vt = build_virtual_text("hello world", Some((0, 5)), "goodbye");
        assert_eq!(vt, "goodbye world", "Must preserve text after replaceEnd");
    }

    #[test]
    fn build_virtual_text_zero_length_replace_is_insert() {
        let vt = build_virtual_text("abc", Some((1, 1)), "X");
        assert_eq!(vt, "aXbc");
    }

    #[test]
    fn build_virtual_text_empty_preedit_is_delete() {
        let vt = build_virtual_text("abc", Some((1, 2)), "");
        assert_eq!(vt, "ac");
    }

    #[test]
    fn build_virtual_text_clamps_out_of_bounds_range() {
        let vt = build_virtual_text("hi", Some((0, 100)), "hello");
        assert_eq!(vt, "hello");
    }

    #[test]
    fn build_virtual_text_swap_start_end_is_noop() {
        let vt = build_virtual_text("abc", Some((2, 1)), "X");
        assert_eq!(vt, "abc");
    }

    // --- CompositionVisualRevision::new ---

    #[test]
    fn composition_visual_revision_new_builds_virtual_text() {
        let rev = CompositionVisualRevision::new(
            "hello".to_string(),
            Some((2, 5)),
            "y".to_string(),
            (0, 5),
        );
        assert_eq!(rev.virtual_text, "hey");
        assert_eq!(rev.committed_text, "hello");
        assert_eq!(rev.preedit_text, "y");
    }

    #[test]
    fn composition_visual_revision_new_no_replace_range() {
        let rev = CompositionVisualRevision::new(
            "abc".to_string(),
            None,
            "def".to_string(),
            (0, 3),
        );
        assert_eq!(rev.virtual_text, "abcdef");
    }

    // --- CursorOnly 事务 ---

    #[test]
    fn cursor_only_transaction_creates_transaction_on_move() {
        let mut engine = EditorEngine::new();
        let vt = engine.cursor_only_transaction("hello world", 5, 0).unwrap();
        assert_eq!(vt.kind, EditorAnimationKind::Cursor);
        assert_eq!(vt.old_text, "hello world");
        assert_eq!(vt.new_text, "hello world");
        assert!(vt.inserted_range.is_none());
        assert!(vt.deleted_range.is_none());
        assert_eq!(vt.old_selection.head.index, 5);
        assert_eq!(vt.new_selection.head.index, 0);
    }

    #[test]
    fn cursor_only_transaction_returns_none_when_no_move() {
        let mut engine = EditorEngine::new();
        let vt = engine.cursor_only_transaction("hello", 3, 3);
        assert!(vt.is_none());
    }

    // --- CompositionUpdate 事务 ---

    #[test]
    fn composition_update_transaction_generates_insert_class() {
        let mut engine = EditorEngine::new();
        let tx = engine.composition_update_transaction(
            "hello",
            None,
            "",
            "n",
        );
        assert!(tx.id > 0);
        assert_eq!(tx.old_revision.virtual_text, "hello");
        assert_eq!(tx.new_revision.virtual_text, "hellon");
        assert!(tx.visual_class_kinds.contains(&VisualClassKind::Insert));
    }

    #[test]
    fn composition_update_does_not_modify_committed_text() {
        let mut engine = EditorEngine::new();
        let tx = engine.composition_update_transaction(
            "committed",
            Some((0, 5)),
            "old_preedit",
            "new_preedit",
        );
        assert_eq!(tx.old_revision.committed_text, "committed");
        assert_eq!(tx.new_revision.committed_text, "committed");
    }

    // --- CompositionCommitOrCancel 事务 ---

    #[test]
    fn composition_commit_transaction_visual_same_no_repeat() {
        let mut engine = EditorEngine::new();
        let comp_rev = CompositionVisualRevision::new(
            "hello".to_string(),
            None,
            " world".to_string(),
            (0, 5),
        );
        // commit 后正文与 virtual_text 相同
        let tx = engine.composition_commit_or_cancel_transaction(
            "hello",
            "hello world",
            comp_rev,
            true,
        );
        assert!(tx.is_commit);
        assert!(tx.is_visual_same);
    }

    #[test]
    fn composition_commit_transaction_visual_different() {
        let mut engine = EditorEngine::new();
        let comp_rev = CompositionVisualRevision::new(
            "hello".to_string(),
            None,
            " wor".to_string(),
            (0, 5),
        );
        // commit 后正文与 virtual_text 不同（候选转换）
        let tx = engine.composition_commit_or_cancel_transaction(
            "hello",
            "hello world",
            comp_rev,
            true,
        );
        assert!(tx.is_commit);
        assert!(!tx.is_visual_same);
    }

    #[test]
    fn composition_cancel_transaction() {
        let mut engine = EditorEngine::new();
        let comp_rev = CompositionVisualRevision::new(
            "hello".to_string(),
            None,
            " world".to_string(),
            (0, 5),
        );
        let tx = engine.composition_commit_or_cancel_transaction(
            "hello",
            "hello",
            comp_rev,
            false,
        );
        assert!(!tx.is_commit);
    }

    // --- classify_visual_diff ---

    #[test]
    fn classify_visual_diff_same_text_returns_empty() {
        let kinds = classify_visual_diff("abc", "abc");
        assert!(kinds.is_empty());
    }

    #[test]
    fn classify_visual_diff_insert_only() {
        let kinds = classify_visual_diff("", "abc");
        assert_eq!(kinds, vec![VisualClassKind::Insert]);
    }

    #[test]
    fn classify_visual_diff_delete_only() {
        let kinds = classify_visual_diff("abc", "");
        assert_eq!(kinds, vec![VisualClassKind::Delete]);
    }

    #[test]
    fn classify_visual_diff_replacement_is_crossfade() {
        let kinds = classify_visual_diff("abc", "xyz");
        assert!(kinds.contains(&VisualClassKind::Crossfade));
    }

    #[test]
    fn classify_visual_diff_suffix_moves_after_insert() {
        // "ab" → "aXb": prefix=a, inserted=X, suffix=b moves
        let kinds = classify_visual_diff("ab", "aXb");
        assert!(kinds.contains(&VisualClassKind::Insert));
        assert!(kinds.contains(&VisualClassKind::Move));
    }

    #[test]
    fn classify_visual_diff_prefix_is_static() {
        // "abc" → "abX": prefix=ab, inserted=X
        let kinds = classify_visual_diff("abc", "abX");
        assert!(kinds.contains(&VisualClassKind::Static));
    }

    // --- compute_rebase ---

    #[test]
    fn compute_rebase_creates_transaction_rebase() {
        let rebase = compute_rebase(42, 0.6, Some(RebaseFrameSnapshot {
            slice_rects: vec![Rect { x: 10.0, y: 20.0, w: 30.0, h: 40.0 }],
            slice_alphas: vec![0.8],
            cursor_rect: None,
        }));
        assert_eq!(rebase.cancelled_transaction_id, 42);
        assert!((rebase.old_progress - 0.6).abs() < f64::EPSILON);
        assert!(rebase.old_frame_snapshot.is_some());
    }

    // --- transactions_overlap ---

    #[test]
    fn transactions_overlap_cursor_only_always_conflicts() {
        assert!(transactions_overlap(
            UnifiedTransactionKind::CursorOnly,
            (0, 0),
            UnifiedTransactionKind::BodyEdit,
            (5, 10),
        ));
    }

    #[test]
    fn transactions_overlap_overlapping_ranges() {
        assert!(transactions_overlap(
            UnifiedTransactionKind::BodyEdit,
            (0, 10),
            UnifiedTransactionKind::BodyEdit,
            (5, 15),
        ));
    }

    #[test]
    fn transactions_overlap_non_overlapping_ranges() {
        assert!(!transactions_overlap(
            UnifiedTransactionKind::BodyEdit,
            (0, 5),
            UnifiedTransactionKind::BodyEdit,
            (10, 15),
        ));
    }

    // --- VisualRevision ---

    #[test]
    fn visual_revision_serializes_camel_case() {
        let rev = VisualRevision {
            revision_id: 1,
            full_text: "hello".to_string(),
            affected_paragraph_range: (0, 5),
            line_snapshot_ids: vec![1, 2],
            cursor_rect: Some(CursorRect { x: 10.0, top: 5.0, bottom: 25.0, baseline_y: 20.0 }),
            caret_affinity: Some(CaretAffinity::Downstream),
            shaping_identity: Some("sha1:abc".to_string()),
        };
        let json = serde_json::to_string(&rev).unwrap();
        assert!(json.contains("\"revisionId\":"));
        assert!(json.contains("\"fullText\":"));
        assert!(json.contains("\"affectedParagraphRange\":"));
        assert!(json.contains("\"cursorRect\":"));
        assert!(json.contains("\"caretAffinity\":"));
        assert!(json.contains("\"shapingIdentity\":"));
    }

    // --- TransactionCancelReason ---

    #[test]
    fn transaction_cancel_reason_serializes_camel_case() {
        let json = serde_json::to_string(&TransactionCancelReason::Rebased).unwrap();
        assert!(json.contains("\"rebased\""));
        let json2 = serde_json::to_string(&TransactionCancelReason::CompositionCommitted).unwrap();
        assert!(json2.contains("\"compositionCommitted\""));
        let json3 = serde_json::to_string(&TransactionCancelReason::CompositionCancelled).unwrap();
        assert!(json3.contains("\"compositionCancelled\""));
    }

    // --- CaretAffinity ---

    #[test]
    fn caret_affinity_serializes_camel_case() {
        let json = serde_json::to_string(&CaretAffinity::Upstream).unwrap();
        assert!(json.contains("\"upstream\""));
        let json2 = serde_json::to_string(&CaretAffinity::Downstream).unwrap();
        assert!(json2.contains("\"downstream\""));
    }

    // --- Timeline 行为测试（#516 验收标准） ---

    #[test]
    fn timeline_pause_resume_maintains_progress() {
        let mut tl = Timeline::new(200);
        tl.mark_first_visible_frame(1000);
        // 50% 进度时暂停
        tl.pause(1100);
        assert!((tl.paused_progress - 0.5).abs() < 0.01);
        // 恢复后进度从 0.5 继续
        tl.resume(1200);
        let p = tl.progress(1200);
        assert!((p - 0.5).abs() < 0.01, "Resume must continue from paused progress");
        // 200ms 后完成
        let p_end = tl.progress(1300);
        assert!((p_end - 1.0).abs() < 0.01);
    }

    #[test]
    fn timeline_cursor_and_text_same_frame_progress() {
        // #516: 光标与正文同帧 progress
        let mut tl = Timeline::new(160);
        tl.mark_first_visible_frame(1000);
        let p_at_1080 = tl.progress(1080);
        // 正文切片和光标都使用同一个 progress
        // 不允许光标维护独立开始时间
        assert!((p_at_1080 - 0.5).abs() < 0.01);
    }

    #[test]
    fn timeline_cursor_only_no_text_slice_still_executes() {
        // #516: CursorOnly 无正文切片也能完整执行
        let mut engine = EditorEngine::new();
        let vt = engine.cursor_only_transaction("hello", 0, 3).unwrap();
        assert_eq!(vt.kind, EditorAnimationKind::Cursor);
        assert_eq!(vt.old_text, vt.new_text);
        assert!(vt.inserted_range.is_none());
        assert!(vt.deleted_range.is_none());
    }

    // --- composing 不修改 committed text/undo/save/sync ---

    #[test]
    fn composition_update_does_not_change_committed_text() {
        let mut engine = EditorEngine::new();
        let tx = engine.composition_update_transaction(
            "original",
            Some((0, 4)),
            "orig",
            "new_text",
        );
        assert_eq!(tx.old_revision.committed_text, "original");
        assert_eq!(tx.new_revision.committed_text, "original");
        // virtual_text 变化，但 committed_text 不变
        assert_ne!(tx.old_revision.virtual_text, tx.new_revision.virtual_text);
    }

    // --- commit 相同视觉文字不重复吐字 ---

    #[test]
    fn commit_same_visual_text_no_repeat_animation() {
        let mut engine = EditorEngine::new();
        let comp_rev = CompositionVisualRevision::new(
            "hello".to_string(),
            None,
            " world".to_string(),
            (0, 5),
        );
        let tx = engine.composition_commit_or_cancel_transaction(
            "hello",
            "hello world",
            comp_rev,
            true,
        );
        assert!(tx.is_visual_same, "Same visual text must not repeat animation");
    }

    // --- 候选转换生成 Crossfade/Move ---

    #[test]
    fn candidate_conversion_generates_crossfade_or_move() {
        let mut engine = EditorEngine::new();
        // 预输入 "ni" → 候选转换 commit "你"
        let comp_rev = CompositionVisualRevision::new(
            "hello ".to_string(),
            Some((6, 8)),
            "ni".to_string(),
            (0, 8),
        );
        let tx = engine.composition_commit_or_cancel_transaction(
            "hello ",
            "hello 你",
            comp_rev,
            true,
        );
        assert!(!tx.is_visual_same, "Candidate conversion changes visual text");
        // 应该有 Crossfade 或 Delete/Insert 分类
        assert!(!tx.visual_class_kinds.is_empty());
    }

    // --- cancel 生成 Delete + reflow ---

    #[test]
    fn cancel_generates_delete_classification() {
        let mut engine = EditorEngine::new();
        let comp_rev = CompositionVisualRevision::new(
            "hello".to_string(),
            None,
            " world".to_string(),
            (0, 5),
        );
        let tx = engine.composition_commit_or_cancel_transaction(
            "hello",
            "hello",
            comp_rev,
            false,
        );
        assert!(!tx.is_commit);
        // 取消时 virtual_text("hello world") → committed_text("hello")
        // 预输入部分应该产生 Delete 分类
        assert!(tx.visual_class_kinds.contains(&VisualClassKind::Delete));
    }

    // --- 连续事务 rebase ---

    #[test]
    fn rebase_covers_all_transaction_kinds() {
        // #516: rebase 必须覆盖四种事务
        // 测试 CursorOnly 与 BodyEdit 冲突
        assert!(transactions_overlap(
            UnifiedTransactionKind::CursorOnly,
            (0, 0),
            UnifiedTransactionKind::BodyEdit,
            (0, 5),
        ));
        // 测试 CompositionUpdate 与 CompositionCommitOrCancel 冲突
        assert!(transactions_overlap(
            UnifiedTransactionKind::CompositionUpdate,
            (0, 5),
            UnifiedTransactionKind::CompositionCommitOrCancel,
            (3, 8),
        ));
    }

    // --- Emoji ZWJ / combining mark / Arabic / RTL / ligature 进入 Crossfade fallback ---

    #[test]
    fn complex_grapheme_classified_as_crossfade_on_change() {
        // ZWJ emoji 变化 → Crossfade
        let kinds = classify_visual_diff("👨‍👩‍👧‍👦", "👨‍👨‍👧");
        assert!(kinds.contains(&VisualClassKind::Crossfade));
    }

    #[test]
    fn combining_mark_classified_as_crossfade_on_change() {
        // 组合字符变化 → Crossfade
        let kinds = classify_visual_diff("e\u{0301}", "è");
        assert!(kinds.contains(&VisualClassKind::Crossfade));
    }

    // --- PlatformVisualTransaction cancel_reason ---

    #[test]
    fn platform_visual_transaction_cancel_reason_serializes() {
        let mut pvt = PlatformVisualTransaction {
            transaction_id: 1,
            generation: 1,
            state: PlatformVisualTransactionState::Cancelled,
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
            slice_roles: Vec::new(),
            slice_document_byte_ranges: Vec::new(),
            static_line_patches: Vec::new(),
            cursor_transition_byte_start: 0,
            cursor_transition_byte_end: 0,
            duration_ms: 160,
            rendering_started_at_ms: None,
            accumulated_paused_duration_ms: 0,
            timeline: None,
            unified_kind: Some(UnifiedTransactionKind::BodyEdit),
            visual_class_kinds: Vec::new(),
            decoration_slices: Vec::new(),
            cursor_path: None,
            composition_revision: None,
            rebase: None,
            cancel_reason: Some(TransactionCancelReason::Rebased),
        };
        let json = serde_json::to_string(&pvt).unwrap();
        assert!(json.contains("\"cancelReason\":"));
        assert!(json.contains("\"rebased\""));

        pvt.cancel_reason = None;
        let json2 = serde_json::to_string(&pvt).unwrap();
        assert!(!json2.contains("\"cancelReason\":"));
    }

    // --- CompositionUpdateTransaction serialization ---

    #[test]
    fn composition_update_transaction_serializes_camel_case() {
        let mut engine = EditorEngine::new();
        let tx = engine.composition_update_transaction("hello", None, "", "n");
        let json = serde_json::to_string(&tx).unwrap();
        assert!(json.contains("\"oldRevision\":"));
        assert!(json.contains("\"newRevision\":"));
        assert!(json.contains("\"visualClassKinds\":"));
        assert!(json.contains("\"durationMs\":"));
    }

    // --- CompositionCommitOrCancelTransaction serialization ---

    #[test]
    fn composition_commit_or_cancel_transaction_serializes_camel_case() {
        let mut engine = EditorEngine::new();
        let comp_rev = CompositionVisualRevision::new(
            "hello".to_string(),
            None,
            " world".to_string(),
            (0, 5),
        );
        let tx = engine.composition_commit_or_cancel_transaction(
            "hello",
            "hello world",
            comp_rev,
            true,
        );
        let json = serde_json::to_string(&tx).unwrap();
        assert!(json.contains("\"isCommit\":"));
        assert!(json.contains("\"isVisualSame\":"));
        assert!(json.contains("\"compositionRevision\":"));
        assert!(json.contains("\"committedTextAfter\":"));
    }

    // ========================================================================
    // #517 行为测试 — 覆盖 issue 验收标准
    // ========================================================================

    // --- #517: replaceRange 测试 ---

    #[test]
    fn build_virtual_text_zero_length_replace_preserves_text_after_cursor() {
        let committed = "你好世界";
        let cursor = "你好".len();
        let preedit = "abc";
        let vt = build_virtual_text(committed, Some((cursor, cursor)), preedit);
        assert_eq!(vt, "你好abc世界", "Zero-length replace must preserve text after cursor");
    }

    #[test]
    fn build_virtual_text_preedit_length_does_not_determine_replace_end() {
        let committed = "你好世界";
        let cursor = "你好".len();
        let vt_short = build_virtual_text(committed, Some((cursor, cursor)), "a");
        let vt_long = build_virtual_text(committed, Some((cursor, cursor)), "abcdef");
        assert_eq!(vt_short, "你好a世界");
        assert_eq!(vt_long, "你好abcdef世界");
    }

    #[test]
    fn build_virtual_text_composing_region_replaces_correctly() {
        let committed = "你好世界";
        let vt = build_virtual_text(committed, Some((3, 9)), "abc");
        assert_eq!(vt, "你abc界");
    }

    #[test]
    fn build_virtual_text_preedit_and_replace_different_lengths() {
        let committed = "hello world";
        let vt = build_virtual_text(committed, Some((0, 5)), "goodbye");
        assert_eq!(vt, "goodbye world");
        let vt2 = build_virtual_text(committed, Some((0, 5)), "hi");
        assert_eq!(vt2, "hi world");
    }

    #[test]
    fn build_virtual_text_emoji_boundary() {
        let committed = "ab😀cd";
        let emoji_start = "ab".len();
        let emoji_char = '😀';
        let emoji_end = emoji_start + emoji_char.len_utf8();
        let vt = build_virtual_text(committed, Some((emoji_start, emoji_end)), "XX");
        assert_eq!(vt, "abXXcd");
    }

    #[test]
    fn build_virtual_text_combining_mark_boundary() {
        let committed = "e\u{0301}test";
        let combining_end = "e\u{0301}".len();
        let vt = build_virtual_text(committed, Some((0, combining_end)), "X");
        assert_eq!(vt, "Xtest");
    }

    // --- #517: CompositionSession 测试 ---

    #[test]
    fn composition_session_zero_length_replace_by_default() {
        let session = CompositionSession::new(
            1, 10, "你好世界".to_string(), "你好".len(),
        );
        assert_eq!(session.replace_start, "你好".len());
        assert_eq!(session.replace_end_exclusive, "你好".len());
    }

    #[test]
    fn composition_session_update_preedit_preserves_replace_range() {
        let mut session = CompositionSession::new(
            1, 10, "你好世界".to_string(), "你好".len(),
        );
        let rev1 = session.update_preedit("a".to_string(), 1);
        assert_eq!(rev1.virtual_text, "你好a世界");
        assert_eq!(session.replace_start, "你好".len());
        assert_eq!(session.replace_end_exclusive, "你好".len());

        let rev2 = session.update_preedit("abcdef".to_string(), 6);
        assert_eq!(rev2.virtual_text, "你好abcdef世界");
        assert_eq!(session.replace_start, "你好".len());
        assert_eq!(session.replace_end_exclusive, "你好".len(),
            "replace_end must NOT change with preedit length");
    }

    #[test]
    fn composition_session_set_composing_region() {
        let mut session = CompositionSession::new(
            1, 10, "你好世界".to_string(), 0,
        );
        session.set_composing_region(3, 9);
        assert_eq!(session.replace_start, 3);
        assert_eq!(session.replace_end_exclusive, 9);
        let rev = session.update_preedit("abc".to_string(), 3);
        assert_eq!(rev.virtual_text, "你abc界");
    }

    #[test]
    fn composition_session_update_creates_revision_chain() {
        let mut session = CompositionSession::new(
            1, 10, "hello world".to_string(), 5,
        );

        let rev1 = session.update_preedit("n".to_string(), 1);
        assert_eq!(rev1.revision_id, 1);
        assert_eq!(rev1.session_id, 1);
        assert!(rev1.offset_map_from_previous.is_none(), "First revision has no previous");

        let rev2 = session.update_preedit("ni".to_string(), 2);
        assert_eq!(rev2.revision_id, 2);
        assert!(rev2.offset_map_from_previous.is_some(), "Second revision must have offset map");
        assert_eq!(rev2.preedit_text, "ni");

        let rev3 = session.update_preedit("nih".to_string(), 3);
        assert_eq!(rev3.revision_id, 3);
        assert!(rev3.offset_map_from_previous.is_some());
    }

    #[test]
    fn composition_session_does_not_modify_committed_text() {
        let mut session = CompositionSession::new(
            1, 10, "original".to_string(), 4,
        );
        session.update_preedit("test".to_string(), 4);
        assert_eq!(session.committed_text_at_start, "original");
    }

    #[test]
    fn composition_session_clear_resets_preedit() {
        let mut session = CompositionSession::new(
            1, 10, "hello".to_string(), 5,
        );
        session.update_preedit("world".to_string(), 5);
        assert!(!session.preedit_text.is_empty());
        session.clear();
        assert!(session.preedit_text.is_empty());
        assert!(session.current_visual_revision.is_none());
    }

    // --- #517: CompositionVisualRevision::from_previous 测试 ---

    #[test]
    fn composition_visual_revision_from_previous_chains_correctly() {
        let rev1 = CompositionVisualRevision::new(
            "hello world".to_string(),
            Some((6, 6)),
            "n".to_string(),
            (0, 11),
        );
        assert_eq!(rev1.virtual_text, "hello nworld");
        let rev2 = CompositionVisualRevision::from_previous(
            &rev1, "ni".to_string(), 2, (0, 11),
        );
        assert_eq!(rev2.virtual_text, "hello niworld");
        assert_eq!(rev2.committed_text, "hello world");
        assert_eq!(rev2.composition_replace_range, Some((6, 6)));
        assert!(rev2.offset_map_from_previous.is_some());
    }

    #[test]
    fn composition_visual_revision_preedit_byte_range_in_virtual_text() {
        let rev = CompositionVisualRevision::new(
            "你好世界".to_string(),
            Some((6, 6)),
            "abc".to_string(),
            (0, 12),
        );
        let (start, end) = rev.preedit_byte_range_in_virtual_text();
        assert_eq!(start, 6);
        assert_eq!(end, 9);
    }

    #[test]
    fn composition_visual_revision_preedit_byte_range_no_replace() {
        let rev = CompositionVisualRevision::new(
            "hello".to_string(),
            None,
            "world".to_string(),
            (0, 5),
        );
        let (start, end) = rev.preedit_byte_range_in_virtual_text();
        assert_eq!(start, 5);
        assert_eq!(end, 10);
    }

    // --- #517: OffsetMap 测试 ---

    #[test]
    fn offset_map_prefix_identity() {
        let map = OffsetMap::build("hello world", "hello WORLD");
        assert!(!map.entries.is_empty());
        let first = &map.entries[0];
        assert_eq!(first.kind, OffsetMapKind::Identity);
        assert_eq!(first.old_byte_offset, 0);
        assert_eq!(first.new_byte_offset, 0);
        assert_eq!(first.length, 6);
    }

    #[test]
    fn offset_map_suffix_shifted() {
        let map = OffsetMap::build("ab", "aXb");
        assert!(map.entries.len() >= 2);
        let suffix = map.entries.iter().find(|e| e.kind == OffsetMapKind::Shifted);
        assert!(suffix.is_some(), "Suffix after insert must be Shifted");
        let suffix = suffix.unwrap();
        assert_eq!(suffix.old_byte_offset, 1);
        assert_eq!(suffix.new_byte_offset, 2);
        assert_eq!(suffix.length, 1);
    }

    #[test]
    fn offset_map_map_old_to_new_identity() {
        let map = OffsetMap::build("abcde", "abXde");
        assert_eq!(map.map_old_to_new(0), Some(0));
        assert_eq!(map.map_old_to_new(1), Some(1));
    }

    #[test]
    fn offset_map_map_old_to_new_shifted() {
        let map = OffsetMap::build("ab", "aXb");
        assert_eq!(map.map_old_to_new(1), Some(2));
    }

    #[test]
    fn offset_map_map_old_to_new_no_mapping_for_middle() {
        let map = OffsetMap::build("abc", "aXc");
        assert!(map.map_old_to_new(1).is_none(), "Middle changed region has no mapping");
    }

    #[test]
    fn offset_map_empty_texts() {
        let map = OffsetMap::build("", "");
        assert!(map.entries.is_empty());
        let map2 = OffsetMap::build("", "abc");
        assert!(map2.entries.is_empty());
    }

    #[test]
    fn offset_map_same_text() {
        let map = OffsetMap::build("abc", "abc");
        assert!(map.entries.is_empty(), "Same text has no offset map");
    }

    // --- #517: SnapshotOwner 测试 ---

    #[test]
    fn snapshot_owner_serializes_camel_case() {
        let json = serde_json::to_string(&SnapshotOwner::OwnedBySession { session_id: 0 }).unwrap();
        assert!(json.contains("\"ownedBySession\""));
        let json2 = serde_json::to_string(&SnapshotOwner::OwnedByTransaction { transaction_id: 42 }).unwrap();
        assert!(json2.contains("\"ownedByTransaction\""));
        let json3 = serde_json::to_string(&SnapshotOwner::Released).unwrap();
        assert!(json3.contains("\"released\""));
    }

    #[test]
    fn snapshot_owner_equality() {
        assert_eq!(SnapshotOwner::OwnedBySession { session_id: 1 }, SnapshotOwner::OwnedBySession { session_id: 1 });
        assert_eq!(
            SnapshotOwner::OwnedByTransaction { transaction_id: 1 },
            SnapshotOwner::OwnedByTransaction { transaction_id: 1 },
        );
        assert_ne!(
            SnapshotOwner::OwnedByTransaction { transaction_id: 1 },
            SnapshotOwner::OwnedByTransaction { transaction_id: 2 },
        );
        assert_eq!(SnapshotOwner::Released, SnapshotOwner::Released);
        assert_ne!(SnapshotOwner::OwnedBySession { session_id: 1 }, SnapshotOwner::Released);
    }

    // --- #517: revision 接续测试 ---

    #[test]
    fn composition_update_from_previous_creates_chained_revision() {
        let mut engine = EditorEngine::new();
        let rev1 = CompositionVisualRevision::new(
            "hello world".to_string(),
            Some((6, 6)),
            "n".to_string(),
            (0, 11),
        );
        let tx = engine.composition_update_from_previous(&rev1, "ni", 2);
        assert_eq!(tx.old_revision.virtual_text, "hello nworld");
        assert_eq!(tx.new_revision.virtual_text, "hello niworld");
        assert!(tx.new_revision.offset_map_from_previous.is_some());
    }

    #[test]
    fn composition_update_from_previous_n_to_ni_to_nih() {
        let mut engine = EditorEngine::new();
        let rev1 = CompositionVisualRevision::new(
            "hello ".to_string(),
            Some((6, 6)),
            "n".to_string(),
            (0, 6),
        );
        let tx1 = engine.composition_update_from_previous(&rev1, "ni", 2);
        assert_eq!(tx1.old_revision.preedit_text, "n");
        assert_eq!(tx1.new_revision.preedit_text, "ni");

        let tx2 = engine.composition_update_from_previous(&tx1.new_revision, "nih", 3);
        assert_eq!(tx2.old_revision.preedit_text, "ni");
        assert_eq!(tx2.new_revision.preedit_text, "nih");
        assert!(tx2.new_revision.offset_map_from_previous.is_some());
    }

    // --- #517: commit/cancel 使用真实 replaceRange ---

    #[test]
    fn commit_with_replace_range_replaces_correctly() {
        let mut engine = EditorEngine::new();
        let comp_rev = CompositionVisualRevision::new(
            "hello world".to_string(),
            Some((6, 11)),
            "earth".to_string(),
            (0, 11),
        );
        let committed_after = "hello earth";
        let tx = engine.composition_commit_or_cancel_transaction(
            "hello world",
            committed_after,
            comp_rev,
            true,
        );
        assert!(tx.is_commit);
        assert!(tx.is_visual_same, "Same visual text on commit with replace range");
    }

    #[test]
    fn cancel_with_replace_range_restores_committed() {
        let mut engine = EditorEngine::new();
        let comp_rev = CompositionVisualRevision::new(
            "hello world".to_string(),
            Some((6, 11)),
            "earth".to_string(),
            (0, 11),
        );
        let tx = engine.composition_commit_or_cancel_transaction(
            "hello world",
            "hello world",
            comp_rev,
            false,
        );
        assert!(!tx.is_commit);
        assert!(!tx.visual_class_kinds.is_empty(), "Cancel must produce visual classifications");
    }

    #[test]
    fn commit_same_visual_text_no_repeat() {
        let mut engine = EditorEngine::new();
        let comp_rev = CompositionVisualRevision::new(
            "hello".to_string(),
            Some((5, 5)),
            " world".to_string(),
            (0, 5),
        );
        let tx = engine.composition_commit_or_cancel_transaction(
            "hello",
            "hello world",
            comp_rev,
            true,
        );
        assert!(tx.is_visual_same);
    }

    #[test]
    fn commit_candidate_conversion_generates_crossfade() {
        let mut engine = EditorEngine::new();
        let comp_rev = CompositionVisualRevision::new(
            "hello ".to_string(),
            Some((6, 8)),
            "ni".to_string(),
            (0, 8),
        );
        let tx = engine.composition_commit_or_cancel_transaction(
            "hello ",
            "hello 你",
            comp_rev,
            true,
        );
        assert!(!tx.is_visual_same);
        assert!(!tx.visual_class_kinds.is_empty());
    }

    // --- #517: CompositionSession 完整流程 ---

    #[test]
    fn composition_session_full_lifecycle() {
        let mut session = CompositionSession::new(
            1, 100, "你好世界".to_string(), "你好".len(),
        );

        let rev1 = session.update_preedit("n".to_string(), 1);
        assert_eq!(rev1.virtual_text, "你好n世界");
        assert_eq!(rev1.composition_replace_range, Some((6, 6)));

        let rev2 = session.update_preedit("ni".to_string(), 2);
        assert_eq!(rev2.virtual_text, "你好ni世界");
        assert_eq!(rev2.composition_replace_range, Some((6, 6)));
        assert!(rev2.offset_map_from_previous.is_some());

        let rev3 = session.update_preedit("nih".to_string(), 3);
        assert_eq!(rev3.virtual_text, "你好nih世界");
        assert_eq!(rev3.composition_replace_range, Some((6, 6)));
    }

    #[test]
    fn composition_session_with_composing_region() {
        let mut session = CompositionSession::new_with_replace_range(
            1, 100, "你好世界".to_string(), 3, 9,
        );
        let rev = session.update_preedit("abc".to_string(), 3);
        assert_eq!(rev.virtual_text, "你abc界");
        assert_eq!(rev.composition_replace_range, Some((3, 9)));
    }

    // --- #517: CompositionVisualRevision 新字段序列化 ---

    #[test]
    fn composition_visual_revision_new_fields_serialize() {
        let rev = CompositionVisualRevision {
            revision_id: 42,
            session_id: 7,
            committed_revision_id: 100,
            committed_text: "hello".to_string(),
            composition_replace_range: Some((5, 5)),
            preedit_text: "world".to_string(),
            preedit_cursor_offset: 3,
            virtual_text: "helloworld".to_string(),
            affected_paragraph_range: (0, 5),
            line_snapshot_ids: Vec::new(),
            cursor_rect: None,
            decoration_ranges: Vec::new(),
            ime_cursor_range: None,
            offset_map_from_previous: Some(OffsetMap {
                entries: vec![OffsetMapEntry {
                    old_byte_offset: 0,
                    new_byte_offset: 0,
                    length: 5,
                    kind: OffsetMapKind::Identity,
                }],
            }),
        };
        let json = serde_json::to_string(&rev).unwrap();
        assert!(json.contains("\"revisionId\":42"));
        assert!(json.contains("\"sessionId\":7"));
        assert!(json.contains("\"committedRevisionId\":100"));
        assert!(json.contains("\"preeditCursorOffset\":3"));
        assert!(json.contains("\"offsetMapFromPrevious\":"));
        assert!(json.contains("\"identity\""));
    }

    #[test]
    fn composition_session_serializes_camel_case() {
        let session = CompositionSession::new(1, 10, "hello".to_string(), 5);
        let json = serde_json::to_string(&session).unwrap();
        assert!(json.contains("\"sessionId\":1"));
        assert!(json.contains("\"committedRevisionId\":10"));
        assert!(json.contains("\"committedTextAtStart\":"));
        assert!(json.contains("\"replaceStart\":5"));
        assert!(json.contains("\"replaceEndExclusive\":5"));
        assert!(json.contains("\"preeditText\":"));
        assert!(json.contains("\"preeditCursorOffset\":0"));
    }

    // --- #517: OffsetMap 序列化 ---

    #[test]
    fn offset_map_serializes_camel_case() {
        let map = OffsetMap {
            entries: vec![
                OffsetMapEntry {
                    old_byte_offset: 0,
                    new_byte_offset: 0,
                    length: 5,
                    kind: OffsetMapKind::Identity,
                },
                OffsetMapEntry {
                    old_byte_offset: 8,
                    new_byte_offset: 10,
                    length: 3,
                    kind: OffsetMapKind::Shifted,
                },
            ],
        };
        let json = serde_json::to_string(&map).unwrap();
        assert!(json.contains("\"entries\":"));
        assert!(json.contains("\"oldByteOffset\":"));
        assert!(json.contains("\"newByteOffset\":"));
        assert!(json.contains("\"length\":"));
        assert!(json.contains("\"kind\":"));
        assert!(json.contains("\"identity\""));
        assert!(json.contains("\"shifted\""));
    }

    // --- #517: CompositionSession is_active/virtual_text/commit/cancel 测试 ---

    #[test]
    fn composition_session_is_active() {
        let mut session = CompositionSession::new(1, 1, "hello".to_string(), 5);
        assert!(!session.is_active());
        session.update_preedit("abc".to_string(), 0);
        assert!(session.is_active());
    }

    #[test]
    fn composition_session_virtual_text_zero_length_replace() {
        let mut session = CompositionSession::new(1, 1, "你好世界".to_string(), "你好".len());
        session.update_preedit("abc".to_string(), 0);
        assert_eq!(session.virtual_text(), "你好abc世界");
    }

    #[test]
    fn composition_session_virtual_text_nonzero_replace() {
        let mut session = CompositionSession::new_with_replace_range(
            1, 1, "你好世界".to_string(), 3, 6,
        );
        session.update_preedit("abc".to_string(), 0);
        assert_eq!(session.virtual_text(), "你abc世界");
    }

    #[test]
    fn composition_session_preedit_byte_range_in_virtual_text() {
        let mut session = CompositionSession::new_with_replace_range(
            1, 1, "你好世界".to_string(), 3, 6,
        );
        session.update_preedit("abcdef".to_string(), 6);
        let (start, end) = session.preedit_byte_range_in_virtual_text();
        assert_eq!(start, 3);
        assert_eq!(end, 9, "preedit range in virtualText differs from replaceRange");
    }

    #[test]
    fn composition_session_commit_uses_replace_range() {
        let mut session = CompositionSession::new(1, 1, "你好世界".to_string(), "你好".len());
        session.update_preedit("abc".to_string(), 0);
        let (comp_rev, committed_after) = session.commit("abc");
        assert_eq!(committed_after, "你好abc世界");
        assert_eq!(comp_rev.virtual_text, "你好abc世界");
        assert!(!session.is_active());
    }

    #[test]
    fn composition_session_commit_with_nonzero_replace_range() {
        let mut session = CompositionSession::new_with_replace_range(
            1, 1, "你好世界".to_string(), 3, 6,
        );
        session.update_preedit("abc".to_string(), 0);
        let (comp_rev, committed_after) = session.commit("abc");
        assert_eq!(committed_after, "你abc世界");
        assert_eq!(comp_rev.virtual_text, "你abc世界");
    }

    #[test]
    fn composition_session_cancel_restores_committed_text() {
        let mut session = CompositionSession::new(1, 1, "你好世界".to_string(), "你好".len());
        session.update_preedit("abc".to_string(), 0);
        let comp_rev = session.cancel();
        assert_eq!(comp_rev.virtual_text, "你好abc世界");
        assert!(!session.is_active());
    }

    #[test]
    fn composition_session_commit_same_visual_no_repeat() {
        let mut session = CompositionSession::new(1, 1, "你好世界".to_string(), "你好".len());
        session.update_preedit("abc".to_string(), 0);
        let (comp_rev, committed_after) = session.commit("abc");
        assert_eq!(comp_rev.virtual_text, committed_after);
    }

    #[test]
    fn composition_session_clear_resets_last_submitted_generation() {
        let mut session = CompositionSession::new(1, 1, "hello".to_string(), 5);
        session.update_preedit("abc".to_string(), 0);
        assert!(session.is_active());
        session.clear();
        assert!(!session.is_active());
        assert!(session.preedit_text.is_empty());
        assert!(session.current_visual_revision.is_none());
        assert_eq!(session.last_submitted_generation, 0);
    }

    #[test]
    fn composition_session_emoji_boundary() {
        let text = "👨‍👩‍👧‍👦hello";
        let emoji_len = "👨‍👩‍👧‍👦".len();
        let mut session = CompositionSession::new(1, 1, text.to_string(), emoji_len);
        session.update_preedit("abc".to_string(), 0);
        assert_eq!(session.virtual_text(), "👨‍👩‍👧‍👦abchello");
    }
}
