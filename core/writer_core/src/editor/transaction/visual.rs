use serde::{Deserialize, Serialize};

#[cfg(test)]
use super::types::EditorCursor;
use super::types::{AnimationMode, EditorAnimationKind, EditorSelection, EditorTransactionCause};
use crate::editor::strong_types::{EditorRevision, Utf8ByteOffset, Utf8ByteRange};

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
    /// 范围（UTF-8 byte offset，半开区间）
    #[serde(
        serialize_with = "crate::editor::strong_types::ser_range",
        deserialize_with = "crate::editor::strong_types::de_range"
    )]
    pub range: Utf8ByteRange,
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
    /// 该 cluster 的 UTF-8 byte 起始位置（半开区间左端）
    #[serde(
        serialize_with = "crate::editor::strong_types::ser_offset",
        deserialize_with = "crate::editor::strong_types::de_offset"
    )]
    pub byte_start: Utf8ByteOffset,
    /// 该 cluster 的 UTF-8 byte 结束位置（半开区间右端，即 `[byte_start, byte_end)`）
    #[serde(
        serialize_with = "crate::editor::strong_types::ser_offset",
        deserialize_with = "crate::editor::strong_types::de_offset"
    )]
    pub byte_end: Utf8ByteOffset,
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
    /// 该 run 的 UTF-8 byte 起始位置（半开区间左端）
    #[serde(
        serialize_with = "crate::editor::strong_types::ser_offset",
        deserialize_with = "crate::editor::strong_types::de_offset"
    )]
    pub byte_start: Utf8ByteOffset,
    /// 该 run 的 UTF-8 byte 结束位置（半开区间右端，即 `[byte_start, byte_end)`）
    #[serde(
        serialize_with = "crate::editor::strong_types::ser_offset",
        deserialize_with = "crate::editor::strong_types::de_offset"
    )]
    pub byte_end: Utf8ByteOffset,
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
    #[serde(
        serialize_with = "crate::editor::strong_types::ser_offset",
        deserialize_with = "crate::editor::strong_types::de_offset"
    )]
    pub byte_start: Utf8ByteOffset,
    /// 该 glyph 在新文本中的 UTF-8 byte 结束位置
    #[serde(
        serialize_with = "crate::editor::strong_types::ser_offset",
        deserialize_with = "crate::editor::strong_types::de_offset"
    )]
    pub byte_end: Utf8ByteOffset,
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
    #[serde(
        default,
        serialize_with = "crate::editor::strong_types::ser_offset",
        deserialize_with = "crate::editor::strong_types::de_offset"
    )]
    pub byte_start: Utf8ByteOffset,
    /// 该 glyph 在文本中的 UTF-8 byte 结束位置
    #[serde(
        default,
        serialize_with = "crate::editor::strong_types::ser_offset",
        deserialize_with = "crate::editor::strong_types::de_offset"
    )]
    pub byte_end: Utf8ByteOffset,
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
    pub range_start: Utf8ByteOffset,
    pub range_len: Utf8ByteOffset,
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::editor::strong_types::ser_opt_range",
        deserialize_with = "crate::editor::strong_types::de_opt_range"
    )]
    pub inserted_range: Option<Utf8ByteRange>,
    /// 删除范围（UTF-8 byte offset），Delete 动画时平台层使用此范围
    /// 而非自行 diff_plain_text 计算，确保 Core 是范围语义唯一来源。
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::editor::strong_types::ser_opt_range",
        deserialize_with = "crate::editor::strong_types::de_opt_range"
    )]
    pub deleted_range: Option<Utf8ByteRange>,
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
    pub revision_id: EditorRevision,
    /// 完整正文文本
    pub full_text: String,
    /// 受影响段落范围（UTF-8 byte offset）
    #[serde(
        serialize_with = "crate::editor::strong_types::ser_range",
        deserialize_with = "crate::editor::strong_types::de_range"
    )]
    pub affected_paragraph_range: Utf8ByteRange,
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
            let mut result = String::with_capacity(committed_text.len() + preedit_text.len());
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
    pub document_revision: EditorRevision,
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
///
/// 配对规则：CrossfadeOld 和 CrossfadeNew 必须成对出现，
/// 共享同一个 Timeline。Old 淡出、New 淡入，视觉上表现为文字变形过渡。
/// Insert/Delete/Move 不需要配对，各自独立动画。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AnimatedSliceRole {
    /// 新插入的文字——从光标附近位移到最终位置并淡入
    Insert,
    /// 被删除的文字——从当前位置向删除后光标或收缩中心位移并淡出
    Delete,
    /// 位置变化的文字——从 oldRect 移到 newRect（shaping 不变）
    Move,
    /// Crossfade 旧侧——shaping 变化的旧文字淡出
    CrossfadeOld,
    /// Crossfade 新侧——shaping 变化的新文字淡入，与 CrossfadeOld 配对
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
    /// 补丁覆盖范围起始（new_text UTF-8 byte offset，半开区间左端）
    #[serde(
        serialize_with = "crate::editor::strong_types::ser_offset",
        deserialize_with = "crate::editor::strong_types::de_offset"
    )]
    pub byte_start: Utf8ByteOffset,
    /// 补丁覆盖范围结束（new_text UTF-8 byte offset，半开区间右端，即 `[byte_start, byte_end)`）
    #[serde(
        serialize_with = "crate::editor::strong_types::ser_offset",
        deserialize_with = "crate::editor::strong_types::de_offset"
    )]
    pub byte_end: Utf8ByteOffset,
    /// 补丁在视口中的目标矩形（文档坐标，不含滚动偏移）
    pub destination_rect: Rect,
    /// 从 new_text 快照纹理中裁剪的可见子区域列表（纹理坐标）
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
    // SAFETY: paused_progress ∈ [0.0, 1.0]（由 progress() clamp 保证），
    // duration_ms 为正整数，乘积 ≤ duration_ms ≤ u64::MAX，截断安全。
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn resume(&mut self, frame_time_ms: u64) {
        if self.pause_started_at_ms.is_none() {
            return;
        }

        if self.first_visible_frame_time_ms.is_none() {
            self.pause_started_at_ms = None;
            self.paused_progress = 0.0;
            return;
        }

        let new_start =
            frame_time_ms.saturating_sub((self.paused_progress * self.duration_ms as f64) as u64);
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
///
/// 分类规则（按优先级）：
/// 1. 仅 new 存在 → Insert（新文字淡入）
/// 2. 仅 old 存在 → Delete（旧文字淡出）
/// 3. shaping identity 相同但位置变化 → Move（位移）
/// 4. 文本可映射但 shaping 改变 → Crossfade（旧淡出+新淡入）
/// 5. 完全相同 → Static（不需要动画，走 StaticLinePatch）
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
///
/// byte_start/byte_end 为半开区间 [byte_start, byte_end)（UTF-8 byte offset），
/// 与 Core 其余范围语义一致。装饰范围不得超出所属 visual revision 的正文范围。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecorationSlice {
    /// 装饰类型
    pub kind: DecorationSliceKind,
    /// UTF-8 byte 范围起始（半开区间，含）
    #[serde(
        serialize_with = "crate::editor::strong_types::ser_offset",
        deserialize_with = "crate::editor::strong_types::de_offset"
    )]
    pub byte_start: Utf8ByteOffset,
    /// UTF-8 byte 范围结束（半开区间，不含）
    #[serde(
        serialize_with = "crate::editor::strong_types::ser_offset",
        deserialize_with = "crate::editor::strong_types::de_offset"
    )]
    pub byte_end: Utf8ByteOffset,
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
