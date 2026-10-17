use super::layout_snapshot::{LineSnapshotId, ShapingIdentity, SourceRect};
use super::qt_text_node::AnimationClipRect;
use super::text_motion::{ClipDirection, MotionKind};
use super::texture_cache::TextureCache;
use crate::editor::layout::LayoutSnapshot;

/// Issue #904：动画层本帧要画的一个局部文字覆盖层。
///
/// 由 `text_motion::TextMotionFrame.overlays` 生成，位置 / 源 UV / clip 都已算好。
/// 与静态层同一坐标系（文档坐标，不含滚动偏移），C++ 动画层统一施加一次
/// `translate(0, -scroll_y)`。
#[derive(Clone, Debug)]
pub(crate) struct TextAnimationGlyphInfo {
    /// 文档坐标 x（不含滚动偏移）。
    pub x: f64,
    /// 文档坐标 y（不含滚动偏移）。
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub opacity: f64,
    pub snapshot_id: LineSnapshotId,
    pub source_rect: SourceRect,
    /// 动态裁剪矩形（文档坐标）：渲染时只绘制 `target_rect ∩ clip_rect`。
    ///
    /// 按同一个 `progress` 更新（吐字/吞字据此推进），与静态层孔洞是两个概念：
    /// 静态孔洞覆盖目标字形完整范围（与 progress 无关），动态 clip 随 progress 变化。
    pub clip_rect: SourceRect,
    /// Issue #919 复核 5：`sample()` 产生的原始动画遮罩（视口裁剪前）。
    ///
    /// `scene_graph_renderer` 用 `clip_rect`（视口裁剪后）做最终可见裁剪，用
    /// `mask_clip` 回执给 `DrawnSlice`，供下一笔接续时继承裁剪起止位置。两者分开
    /// 保存：视口/上一动画局部裁过时 `clip_rect` 的位置与 `mask_clip` 不同，只用
    /// `clip_rect` 的宽度从源矩形边界重新造遮罩会把可见部分移到旧矩形另一边。
    pub mask_clip: SourceRect,
    /// Issue #919 复核 1：运动类别，供 `DrawnSlice` 记录本帧实际显示的运动类型。
    pub motion_kind: MotionKind,
    /// Issue #919 复核 1：所属字节范围，供连续编辑重定向时轻量片段关联。
    pub byte_range: (usize, usize),
    /// Issue #919 复核 3：本片段的 shaping 指纹，供 `DrawnSlice` 记录后用于
    /// 连续编辑接续时确认同一份字形，避免同 byte_range 不同字误继承遮罩宽度。
    pub shaping_identity: ShapingIdentity,
    /// Issue #919 复核 3：本片段的裁剪方向（Shift 无裁剪方向，为 None），
    /// 供 `DrawnSlice` 记录后用于旧 Conceal 继续吞时沿用原方向。
    pub direction: Option<ClipDirection>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SelectionPreeditPlan {
    pub has_selection: bool,
    pub selection_ranges: Vec<SelectionRange>,
    pub has_preedit: bool,
    pub preedit_ranges: Vec<PreeditRange>,
}

/// Issue #677 评论 5653944889: GUI 线程一次性准备好的不可变帧数据。
///
/// `layout_snapshot` 和 `selection_preedit` 来自同一次 `EditorLayout::snapshot()` 调用，
/// 确保 render thread（`update_paint_node()`）只读取已经准备好的数据，
/// 不再进入排版生命周期（`EditorLayout::snapshot()` /
/// `begin_layout_generation()` / `clear_layout_generation()`）。
///
/// 不变性：
/// - 只在 GUI 线程上由 `prepare_editor_frame()` 构造并一次性替换 `prepared_frame`。
/// - render thread 只读 `layout_snapshot` 和 `selection_preedit`，不调用任何排版方法。
/// - `prepared_frame = None` 表示需要 GUI 侧重新准备，render thread 跳过静态正文渲染。
///
/// Issue #677 评论 5654174714: `selection_preedit` 只保存文档坐标几何
/// （`SelectionRange.y` / `PreeditRange.y` 是文档坐标，不减 `scroll_y`）。
/// `scroll_y`、selection/preedit 颜色、cursor 颜色、动画进度属于每帧轻量状态，
/// 不进入 `PreparedEditorFrame`；由 `update_paint_node()` 读取当前值后通过
/// `RenderPlan` 的轻量 style 字段（`SelectionPreeditStyle` / `CursorStyle`）
/// 传给 renderer，renderer 在绘制时做 `screen_y = doc_y - scroll_y` 换算。
#[derive(Clone, Debug)]
pub(crate) struct PreparedEditorFrame {
    /// 当前已准备好的 canonical 排版快照。
    pub layout_snapshot: LayoutSnapshot,
    /// 从同一个 snapshot 算好的选区/preedit 文档坐标几何（不含颜色/scroll_y）。
    pub selection_preedit: SelectionPreeditPlan,
}

#[derive(Clone, Debug)]
pub(crate) struct SelectionRange {
    pub x: f64,
    /// 文档坐标 y（不减 scroll_y）。视口换算由 renderer 在绘制时完成。
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Clone, Debug)]
pub(crate) struct PreeditRange {
    pub x: f64,
    /// 文档坐标 y（不减 scroll_y）。视口换算由 renderer 在绘制时完成。
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub underline: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct CursorStyle {
    pub color: String,
    pub width: f64,
}

impl Default for CursorStyle {
    fn default() -> Self {
        Self {
            color: "#006497".to_string(),
            width: 2.0,
        }
    }
}

/// Issue #677 评论 5654174714: selection/preedit 的本帧轻量颜色状态。
/// 不进入排版，也不固化进 `PreparedEditorFrame`；由 `update_paint_node()` 读取当前
/// `current_selection_color` 后传给 renderer。preedit 透明度在 renderer 计算。
#[derive(Clone, Debug)]
pub(crate) struct SelectionPreeditStyle {
    pub selection_color: String,
}

impl Default for SelectionPreeditStyle {
    fn default() -> Self {
        Self {
            selection_color: "#3381D1".to_string(),
        }
    }
}

/// Issue #679 评论 5657313927: 纯显示数据 — render thread 直接画这个，
/// 不再理解 Snap/Tween/driver/Timestamp。由 qquickitem_impl 从 cursor_ctrl
/// 的 visual_x/y/h/visible 和 blink opacity 构造。
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CursorRenderState {
    pub visible: bool,
    pub x: f64,
    pub y: f64,
    pub h: f64,
    pub baseline_y: f64,
    pub opacity: f64,
}

/// Issue #904：本帧要画的局部文字覆盖层 + 静态层要挖掉的局部孔洞。
///
/// 取代 #901 的 `RenderOwnership`：不再有整屏 ownership/revision/handoff
/// 状态机，只有「这一帧动画层画哪些局部片段」和「静态层排除哪些局部矩形」。
#[derive(Clone, Debug, Default)]
pub(crate) struct LocalTextPlan {
    /// 动画层本帧的局部文字覆盖层（吐字/吞字/位移）。
    pub overlays: Vec<TextAnimationGlyphInfo>,
    /// 静态层本帧需要排除的局部矩形，与 `overlays` 原子提交。
    pub static_holes: Vec<AnimationClipRect>,
    /// 本帧接管所需的全部动画源纹理 id。
    ///
    /// 用「本计划全部片段要用到的源纹理」而不是「本帧可见的 overlay 纹理」判断
    /// 就绪：progress=0 时 Reveal 尚无可见像素，但它的目标字形已被静态孔洞排除，
    /// 若此时漏检纹理，新字会先消失。
    pub required_textures: Vec<LineSnapshotId>,
    /// 本帧仍活动的整体段落 y 偏移（复用已有 transform，不逐字复制纹理）。
    ///
    /// 元素为 `(paragraph_id, 相对目标位置的额外 y 偏移)`；动画结束为空，
    /// 段落直接留在目标矩阵。
    pub paragraph_y_offsets: Vec<(u64, f64)>,
}

impl LocalTextPlan {
    /// 检查本帧接管所需的动画源纹理是否已在 `texture_cache` 中就绪。
    ///
    /// 缺任一张就整体不接管：静态层画完整 canonical 正文，动画层不画。
    /// 静态挖孔与动画覆盖层用同一标志，避免"静态挖了空但动画没画"或反之。
    pub(crate) fn has_animation_resources(&self, texture_cache: &TextureCache) -> bool {
        self.required_textures
            .iter()
            .all(|id| texture_cache.contains_line(id))
    }

    /// Issue #904 评论 4：本帧真正缺失的源纹理 id。
    ///
    /// 资源缺失不是"正常成功路径"：上游必须生成本计划真正需要的源；这里把缺失
    /// 的 id 显式列出来，供日志写出具体失败原因，而不是让"已建立 TextMotion 结构"
    /// 被误报成"已经有动画"。
    pub(crate) fn missing_textures(&self, texture_cache: &TextureCache) -> Vec<LineSnapshotId> {
        self.required_textures
            .iter()
            .copied()
            .filter(|id| !texture_cache.contains_line(id))
            .collect()
    }
}

#[derive(Clone, Debug, Default)]
/// Issue #707 评论 5723616999: 改 `pub` 让集成测试能访问 `drawn_caret_rect` 字段。
/// 加 `Default` 让集成测试能构造实例验证字段可读写。
/// 一个 immutable RenderPlan 汇总本帧的局部文字效果、selection/preedit 和唯一 caret。
/// renderer 不读取动画状态，也不重新决定 owner。
pub struct RenderPlan {
    // 除 drawn_caret_rect 外全部收回 pub(crate)：字段类型都是平台端内部渲染
    // 状态（LocalTextPlan / CursorRenderState 等），集成测试只需要
    // drawn_caret_rect 这个 (x, y, h) 裸元组。
    //
    // 光标采样在 CursorController 内完成，RenderPlan 只保存最终要显示的位置。
    pub(crate) local_text: LocalTextPlan,
    pub(crate) selection_preedit: SelectionPreeditPlan,
    /// Issue #679 评论 5657313927: 改为纯显示数据 CursorRenderState，
    /// 不再携带 CursorAnimationPlan（Snap/Tween/driver 由 GUI 线程消费）。
    pub(crate) cursor: CursorRenderState,
    pub(crate) cursor_style: CursorStyle,
    /// Issue #677 评论 5654174714: selection/preedit 的本帧轻量颜色状态。
    pub(crate) selection_preedit_style: SelectionPreeditStyle,
    /// Issue #705: 本帧真正绘制出去的 caret rect `(x, y, h)`。
    ///
    /// 该 rect 是 CursorController 本帧采样后的实际光标位置；Snap 时就是目标位置。
    /// `qquickitem_impl` 每帧生成 RenderPlan 后,把
    /// `cursor_ctrl.visual_x/visual_y/visual_h` 同步成此值。下一次
    /// 下一次光标目标更新时从这个"上一帧真正画出来的位置" rebase。
    pub drawn_caret_rect: Option<(f64, f64, f64)>,
}
