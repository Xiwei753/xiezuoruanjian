use super::layout_snapshot::{LineSnapshotId, SourceRect};
use super::qt_text_node::AnimationClipRect;
use crate::editor::layout::LayoutSnapshot;

#[derive(Clone, Debug)]
pub(crate) struct TextAnimationGlyphInfo {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub opacity: f64,
    pub snapshot_id: LineSnapshotId,
    pub source_rect: SourceRect,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TextAnimationPlan {
    pub glyphs: Vec<TextAnimationGlyphInfo>,
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
#[derive(Clone, Debug, Default)]
pub(crate) struct CursorRenderState {
    pub visible: bool,
    pub x: f64,
    pub y: f64,
    pub h: f64,
    pub opacity: f64,
}

#[derive(Clone, Debug, Default)]
/// Issue #707 评论 5723616999: 改 `pub` 让集成测试能访问 `drawn_caret_rect` 字段。
/// 加 `Default` 让集成测试能构造实例验证字段可读写。
pub struct RenderPlan {
    // 除 drawn_caret_rect 外全部收回 pub(crate)：字段类型都是平台端内部渲染
    // 状态（TextAnimationPlan / CursorRenderState / FrameContext 等），
    // 集成测试只需要 drawn_caret_rect 这个 (x, y, h) 裸元组。
    //
    // Issue #727 约束 3 要求的"本帧统一采样的协同运动帧"不再挂在 RenderPlan 上：
    // build_render_plan_full 在入口处 sample 一次后全程用局部变量消费
    // （caret.is_some() 门禁 + owner_key 过滤），挂到帧上后没有任何读者，
    // 只是一份和局部变量重复的副本，所以这里不再冗余存一份。
    pub(crate) text_animation: TextAnimationPlan,
    pub(crate) selection_preedit: SelectionPreeditPlan,
    /// Issue #679 评论 5657313927: 改为纯显示数据 CursorRenderState，
    /// 不再携带 CursorAnimationPlan（Snap/Tween/driver 由 GUI 线程消费）。
    pub(crate) cursor: CursorRenderState,
    pub(crate) cursor_style: CursorStyle,
    /// Issue #677 评论 5654174714: selection/preedit 的本帧轻量颜色状态。
    pub(crate) selection_preedit_style: SelectionPreeditStyle,
    /// Issue #826: 吐字期间静态正文层需要隐藏的裁剪矩形。
    ///
    /// 直接存储文档坐标 x/y/w/h，全部来自遮罩前沿
    /// `hidden_canonical_rects`：最新 canonical 正文里还没被前沿打开的部分。
    /// 静态层只画 complement，因此"吐字只画一份正文"天然成立。
    pub(crate) clip_rects: Vec<AnimationClipRect>,
    /// Issue #705: 本帧真正绘制出去的 caret rect `(x, y, h)`。
    ///
    /// 正文协同动画时,这个 rect 就是同帧文字事务算出的实际光标位置;
    /// 纯光标动画时,就是该 Tween 本帧位置;Snap 时就是目标位置。
    /// `qquickitem_impl` 每帧生成 RenderPlan 后,把
    /// `cursor_ctrl.visual_x/visual_y/visual_h` 同步成此值。下一次
    /// 输入、删除、鼠标点击创建新事务时,只允许从这个"上一帧真正
    /// 画出来的位置" rebase。
    pub drawn_caret_rect: Option<(f64, f64, f64)>,
}
