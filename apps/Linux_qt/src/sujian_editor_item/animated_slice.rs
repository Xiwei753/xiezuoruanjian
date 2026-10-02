use super::layout_snapshot::{LineSnapshotId, ShapingIdentity, SourceRect};
use super::transaction_key::VisualTransactionKey;

// ── 动画切片模块 ──
//
// 与 Core `AnimatedSliceRole` 的映射关系：
// - InsertReveal   ↔ Core Insert（新文字在最终位置从左向右逐步显出纹理）
// - DeleteConceal  ↔ Core Delete（旧文字在原位从一侧向另一侧逐步收进纹理）
// - ReflowMove     ↔ Core Move（shaping 不变，几何位移）
// - ReflowCrossFade ↔ Core CrossfadeOld/CrossfadeNew（shaping 变化，成对淡入淡出）
//
// Issue #686 评论 5664857575 领域1：吐字/吞字动画。
// 插入文字始终在最终排版位置，动画进度只控制可见纹理宽度（0% → 100%）。
// 删除文字始终在删除前位置，动画进度只控制可见纹理宽度（100% → 0%）。
// 不再从光标位置插值移动、不再缩放、不再淡入淡出。
//
// 线程安全：AnimatedSlice 仅在 Qt GUI 线程中使用，
// 不跨线程传递——动画帧计算和渲染都在 GUI 线程完成。

/// Issue #738 评论 5789470425 问题3: CrossFade old/new 两侧的共同组身份。
/// 一对 ReflowCrossFadeOld/New 共享同一个 `crossfade_group_id`，reconcile 时
/// 以 group 为单位成对重绑，不再把 old/new 两侧各自独立判死。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CrossFadeSide {
    Old,
    New,
}

/// Issue #738 评论 5789470425 问题2: merged Reflow 的逐 cluster anchor。
///
/// `merge_two` 合并相邻同方向 slice 时，不再把多个 cluster 框成一个大矩形，
/// 而是把每个原始 cluster 的身份（byte range、shaping identity、from/to document
/// rect、source_rect、snapshot_id、visual_line_id）保存为 `ReflowAnchor`。
/// reconcile 时逐 anchor 做 OffsetMap、找新 canonical cluster、校验 shaping；
/// 拆行或各 anchor 新移动向量不同时拆回多个 Timed unit。
#[derive(Clone, Debug)]
pub(crate) struct ReflowAnchor {
    pub byte_start: usize,
    pub byte_end: usize,
    pub shaping_identity: Option<ShapingIdentity>,
    pub from_document_rect: SourceRect,
    pub to_document_rect: SourceRect,
    pub source_rect: SourceRect,
    pub snapshot_id: LineSnapshotId,
    pub visual_line_id: Option<usize>,
}

/// 动画切片类型，决定视觉语义和插值行为。
///
/// 与 Core `AnimatedSliceRole` 一一对应（见模块文档映射表）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AnimatedSliceKind {
    /// 新文字在最终位置从左向右逐步显出纹理宽度（0% → 100%）。
    /// 对应 Core AnimatedSliceRole::Insert。
    InsertReveal,
    /// 旧文字在删除前位置逐步收进纹理宽度（100% → 0%）。
    /// 收进方向由 `conceal_to_left_edge` 决定。
    /// 对应 Core AnimatedSliceRole::Delete。
    DeleteConceal,
    /// old/new shaping identity 相同，复用旧视觉资源做几何移动。
    /// 对应 Core AnimatedSliceRole::Move。
    ReflowMove,
    /// shaping 发生变化，旧视觉淡出、新视觉淡入；同一逻辑对象由成对 slice 表达。
    /// 对应 Core AnimatedSliceRole::CrossfadeOld/CrossfadeNew。
    ReflowCrossFade,
}

/// Issue #815 评论 5946701331 问题1: 协同吞字/吐字的边界由谁驱动。
///
/// 协同模式**仍然只有一条 caret 运动轨迹**（`PreparedCursorVisualTrack`），
/// 文字边界只是这条轨迹的另一种读法。这里区分的是"边界坐标怎么从这份采样里读"，
/// 不是给文字新开一条独立时间线。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IngestBoundaryDriver {
    /// 边界就是本帧真实 caret.x。
    ///
    /// - InsertReveal：吐字从编辑前的 caret 向新 caret 展开。
    /// - Backspace：真实 caret 自己会往左走，吞字边界直接跟着它收拢。
    CaretPosition,
    /// Issue #815 评论 5946701331 问题1: Delete 键的吞字边界。
    ///
    /// Delete 键时 old caret 在被删文字**左侧**，删除后 caret 原地不动，
    /// 真实 caret track 从头到尾 progress 都是 0 增量。若边界无条件等于
    /// caret.x，`anchor == caret_x`，第一帧起裁切宽度就是 0——DeleteConceal
    /// 直接消失，根本没有"吞进去"的过程。
    ///
    /// 所以 Delete 需要自己的吞字边界：它从被删区间的**右端**
    /// （`ingest_boundary_from_x`，回退 `line_mask_right`）朝 caret.x 收拢。
    /// 收拢过程由同一笔事务 cursor track 的 progress 驱动（仍是一条轨迹），
    /// 只是这条轨迹驱动的量是"吞字边界"而不是"真实 caret 位置"。
    DeleteForwardBoundary,
}

/// Issue #815 评论 5946701331 问题2: 本帧吞字/吐字边界相对某一行所处阶段的判定结果。
///
/// 只在**同一份 canonical snapshot** 的行序内判定，不做 old/new 行号混比。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IngestLinePhase {
    /// 边界已经走过本行：Insert 全显 / Delete 全隐。
    Passed,
    /// 边界就在本行：按本帧边界 x 裁切。
    OnCurrentLine,
    /// 边界还没走到本行：Insert 全隐 / Delete 全保留。
    NotReached,
}

/// 一次动画切片的完整描述。
///
/// 坐标契约：
/// - `source_rect`：`snapshot_id` 对应视觉资源内的裁剪区域（行局部坐标，已乘 DPR）。
/// - `from_document_rect`/`to_document_rect`：文档坐标，不包含当前滚动偏移。
/// - `byte_start`/`byte_end`：用于事务冲突判断和静态层隐藏，不参与逐帧排版。
/// - `conceal_to_left_edge`：仅对 `DeleteConceal` 有效。true 表示向左边缘收缩（保留左段，
///   右段先消失——Backspace 场景，光标在文字右侧）；false 表示向右边缘收缩（保留右段，
///   左段先消失——Delete 键场景，光标在文字左侧）。
#[derive(Clone, Debug)]
pub(crate) struct AnimatedSlice {
    pub kind: AnimatedSliceKind,
    pub snapshot_id: LineSnapshotId,
    pub source_rect: SourceRect,
    pub from_document_rect: SourceRect,
    pub to_document_rect: SourceRect,
    pub opacity_from: f64,
    pub opacity_to: f64,
    pub scale_from: f64,
    pub scale_to: f64,
    pub byte_start: usize,
    pub byte_end: usize,
    pub shaping_identity: Option<ShapingIdentity>,
    pub conceal_to_left_edge: bool,
    /// Issue #808: 吞吐字遮罩的光标锚点（文档坐标）。
    ///
    /// `insert_reveal` 保存旧 caret 位置（吐字起点），`delete_conceal` 保存
    /// 新 caret 位置（吞字终点）。`compute_frame` 用这个锚点决定遮罩从哪一侧
    /// 打开/收拢，而不是简单从 `to_document_rect.x` / `from_document_rect.x`
    /// 开始——这样文字真正"从 caret 处吐出来 / 被 caret 吞进去"。
    ///
    /// 跨行时每个 slice 带自己的 caret 锚点（按 visual_line_id 分段），
    /// 不能拿上一行的 x 去裁下一行。ReflowMove/ReflowCrossFade 不使用此字段（0.0）。
    ///
    /// "caret 只决定遮罩的空间锚点/方向"，不能决定文字动画进度——文字 progress
    /// 仍来自文字自己的 timeline（`current_visible_fraction`）。
    pub caret_anchor_x: f64,
    pub caret_anchor_y: f64,
    /// Issue #808 评论 5919641249 修改 1: 行级共同吞吐边界（文档坐标）。
    ///
    /// 同一 `visual_line_id` 的 InsertReveal/DeleteConceal 在构造阶段由
    /// [`super::animation::transaction_builder::assign_shared_line_masks`] 分成
    /// 同一组，组内所有 slice 共享同一条 `line_mask_left/right`。
    /// `compute_frame` 先用文字自己的 progress 算这一帧的行级共同 boundary，
    /// 再让每个 slice 用自己的 rect 与 boundary 求交得到自己的 frame/source clip。
    ///
    /// 这样每个 slice 保留自己真实的 source/document rect（中间保留的字符绝不会
    /// 被卷入别人的大图块），但同一行只有一个吞吐边界——不再用 union `source_rect`
    /// 合并成一个大 slice 来表达共同 mask。
    ///
    /// 未分组的单 slice（含直接构造）默认等于自己的 rect（insert 用
    /// `to_document_rect`，delete 用 `from_document_rect`），行为与分组前一致。
    /// ReflowMove/ReflowCrossFade 不使用此字段（等于自己的 from rect）。
    pub line_mask_left: f64,
    pub line_mask_right: f64,
    /// Issue #808 评论 5916391891 修改 1: 该 slice 是否属于 caret 所在视觉行。
    ///
    /// `true` 表示该 slice 在 caret 所在视觉行上，`compute_frame` 用 `caret_anchor_x`
    /// 做吞吐遮罩锚点——文字真正"从 caret 处吐出来 / 被 caret 吞进去"。
    /// `false` 表示该 slice 不在 caret 所在行（跨行编辑的其他行），`compute_frame`
    /// 用行首/行尾做锚点——insert 时从行首展开，delete 时向行尾/行首收拢。
    ///
    /// 在 `build_insert_reveal_slices` / `build_delete_conceal_slices` 中，
    /// 比较 `new_line.visual_line_id` 与传入的 caret visual_line_id 来设置此字段。
    /// 一次编辑的 caret 只在一个位置，跨行时只有 caret 所在那行的 slice 为 true。
    /// ReflowMove/ReflowCrossFade 不使用此字段（false）。
    pub is_caret_line: bool,
    /// Issue #815 评论 5946701331 问题1: 本 slice 的吞字/吐字边界驱动。
    ///
    /// `InsertReveal` 与 Backspace 的 `DeleteConceal` 用 [`IngestBoundaryDriver::CaretPosition`]；
    /// Delete 键的 `DeleteConceal` 用 [`IngestBoundaryDriver::DeleteForwardBoundary`]。
    /// `build_delete_conceal_slices` 按既有 `conceal_to_left_edge` 语义区分：
    /// true（caret 在被删文字右侧 → Backspace）→ `CaretPosition`；
    /// false（caret 在被删文字左侧 → Delete 键）→ `DeleteForwardBoundary`。
    /// ReflowMove/ReflowCrossFade 不使用此字段（`CaretPosition`，不会走到该入口）。
    pub ingest_boundary_driver: IngestBoundaryDriver,
    /// Issue #815 评论 5946701331 问题1: Delete 键吞字边界的起点 x（文档坐标）。
    ///
    /// 即被删区间在 **old snapshot** 内的右端。Delete 时 caret 不动，吞字边界从
    /// 这里朝 caret.x 收拢。`None` 时回退 `line_mask_right`（该 slice 所在行的
    /// 被删文字右端，由 `assign_shared_line_masks` 计算）。
    pub ingest_boundary_from_x: Option<f64>,
    /// Issue #815 评论 5946701331 问题2: 本 slice 自己在**同一份 snapshot** 内的行序。
    ///
    /// `InsertReveal` 用 **new** snapshot 的 `line_snapshots` 下标；
    /// `DeleteConceal` 用 **old** snapshot 的 `line_snapshots` 下标。
    /// 绝不能拿 old 的行序和 new 的行序做大小比较——两次 canonical 排版的
    /// `VisualLine.id` 都从 0 重新编号，混着比必然判断反。
    pub ingest_line_ord: Option<usize>,
    /// Issue #815 评论 5946701331 问题2: 吞吐路径的起点行序（同一份 snapshot 内）。
    ///
    /// - InsertReveal（new snapshot）：`inserted_range.start` 所在视觉行，
    ///   即编辑前 caret 所在的新侧视觉行。
    /// - DeleteConceal（old snapshot）：删除前 old caret 所在视觉行。
    pub ingest_from_line_ord: Option<usize>,
    /// Issue #815 评论 5946701331 问题2: 吞吐路径的终点行序（同一份 snapshot 内）。
    ///
    /// - InsertReveal（new snapshot）：`inserted_range.end` 所在视觉行，
    ///   即新 caret 所在视觉行。
    /// - DeleteConceal（old snapshot）：`deleted_range.start` 所在视觉行。
    pub ingest_to_line_ord: Option<usize>,
    /// Issue #815 评论 5947728704 问题1: 本 slice 所在视觉行在**本侧** canonical
    /// 里的真实 y 范围。
    ///
    /// 跨行相位必须拿当前帧真实 `caret.y` 与这个范围比大小，而不是用 raw
    /// `progress` 重新推一遍行序——那样造出的位置和屏幕上的 caret 不是同一帧几何。
    pub ingest_line_top: Option<f64>,
    pub ingest_line_bottom: Option<f64>,
    /// Issue #722 评论 5748596920 问题2: 该 slice 所属视觉行的 id（来自 VisualLine.id）。
    ///
    /// 用于跨软换行裁切判断：caret 和 slice 在同一视觉行时才用 caret.x 做横向裁切；
    /// caret 还没进入该行时 InsertReveal 保持 0 / DeleteConceal 保持完整；
    /// caret 已经越过该行时 InsertReveal 保持完整 / DeleteConceal 保持 0。
    ///
    /// Issue #722 评论 5749164244 问题1: 改为 `Option<usize>`。
    /// `None` 表示未知（Reflow 或采样路径无法确定行身份），`Some(id)` 表示已知行。
    /// 不再用 0 当"未知"哨兵——0 是首行合法索引，不能同时当哨兵值。
    pub visual_line_id: Option<usize>,
    /// Issue #690 评论 5675007226 步骤 3: 视觉单元的动画起始比例。
    ///
    /// InsertReveal：当前已吐出来的比例（0.0 = 未显示，1.0 = 完全显示）。
    /// DeleteConceal：当前还剩多少比例（1.0 = 完全可见，0.0 = 完全消失）。
    /// ReflowMove / ReflowCrossFade：不使用此字段（保持 0.0）。
    ///
    /// 快速连续输入时，旧 slice 被 rebase 后从 `start_fraction` 继续播放，
    /// 不再从 0 重新开始（避免文字"吐到一半被重启"）。
    pub start_fraction: f64,
    /// Issue #727 评论 5755858583 问题2: 该 slice 在静态正文层需要隐藏的文档坐标矩形。
    ///
    /// InsertReveal/ReflowMove/ReflowCrossFadeNew 在创建 slice 时直接写自己的
    /// canonical 独占区域。RenderPlan 从 active units 收集这些 rect 作为裁剪区域，
    /// 不再从 StaticLinePatch 二次换算。AnimatedSlice 成为唯一事实源。
    pub static_hidden_document_rects: Vec<SourceRect>,
    /// Issue #738 评论 5789470425 问题3: CrossFade old/new 两侧的共同组身份。
    /// `None` 表示非 CrossFade 或未分组；`Some(id)` 表示属于该 group 的一侧。
    /// reconcile 以 group 为单位成对重绑 old/new。
    pub crossfade_group_id: Option<u64>,
    /// Issue #738 评论 5789470425 问题3: CrossFade 的 old/new 侧标记。
    /// `None` 表示非 CrossFade；`Some(Old/New)` 表示该 slice 是 group 的哪一侧。
    pub crossfade_side: Option<CrossFadeSide>,
    /// Issue #738 评论 5789470425 问题2: merged Reflow 的逐 cluster anchor 列表。
    /// 非 ReflowMove/ReflowCrossFade 时为空。merge_two 合并 anchors 列表，
    /// 不丢子 cluster 身份；rebind 逐 anchor 找新 canonical cluster。
    pub reflow_anchors: Vec<ReflowAnchor>,
}

impl AnimatedSlice {
    /// Issue #690 评论 5675007226 步骤 2: 文字动画的 easing 函数。
    ///
    /// Issue #808: 此 easing 只属于文字 reveal/conceal/reflow 的逐帧进度。
    /// 光标 track 不再共用这条曲线——光标用自己的 [`ease_out_cubic`]，
    /// duration 不同、曲线也允许不同。文字和光标各自按自己的时间线推进，
    /// 不再出现"文字甩开光标"或"两个动画共一套曲线"的旧耦合。
    pub(crate) fn ease_out_quad(p: f64) -> f64 {
        let p = p.clamp(0.0, 1.0);
        1.0 - (1.0 - p).powi(2)
    }

    /// Issue #808: 光标 track 专用 easing 函数。
    ///
    /// 光标轨迹从文字的 `ease_out_quad` 解开：cursor track 用自己的
    /// ease-out cubic 曲线，与文字 reveal/conceal 的 ease-out quadratic
    /// 区分开。文字 progress 只来自文字自己的 timeline，光标 progress
    /// 只来自 cursor track；duration 不同、曲线也允许不同。
    /// 普通 CursorOnly（方向键、Home/End、鼠标点选后的平滑移动）也保留
    /// 自己的平滑曲线，不调用本函数。
    pub(crate) fn ease_out_cubic(p: f64) -> f64 {
        let p = p.clamp(0.0, 1.0);
        1.0 - (1.0 - p).powi(3)
    }

    /// 创建 Insert 吐字切片。
    ///
    /// 文字始终在 `to_document_rect` 位置，动画进度控制可见纹理宽度从 0 → 100%。
    /// Issue #808: `cursor_x`/`cursor_y` 是旧 caret 位置（吐字起点），保存为
    /// `caret_anchor_x`/`caret_anchor_y`。`compute_frame` 用这个锚点决定遮罩
    /// 从哪一侧打开——文字真正"从 caret 处吐出来"，而不是简单从文字左边展开。
    /// 跨行时每个 slice 带自己的 caret 锚点，不能拿上一行的 x 去裁下一行。
    pub fn insert_reveal(
        _key: VisualTransactionKey,
        snapshot_id: LineSnapshotId,
        source_rect: SourceRect,
        to_document_rect: SourceRect,
        cursor_x: f64,
        cursor_y: f64,
        byte_start: usize,
        byte_end: usize,
        shaping_identity: Option<ShapingIdentity>,
        visual_line_id: Option<usize>,
    ) -> Self {
        Self {
            kind: AnimatedSliceKind::InsertReveal,
            snapshot_id,
            source_rect,
            from_document_rect: to_document_rect.clone(),
            // Issue #808 评论 5919641249: 单 slice 默认自己是整条行级 mask，
            // 分组由 assign_shared_line_masks 在构造阶段写回。
            line_mask_left: to_document_rect.x,
            line_mask_right: to_document_rect.x + to_document_rect.w,
            to_document_rect,
            opacity_from: 1.0,
            opacity_to: 1.0,
            scale_from: 1.0,
            scale_to: 1.0,
            byte_start,
            byte_end,
            shaping_identity,
            conceal_to_left_edge: false,
            caret_anchor_x: cursor_x,
            caret_anchor_y: cursor_y,
            // Issue #808 评论 5917296533 问题4: 构造函数默认 is_caret_line=false。
            // 调用方（build_insert_reveal_slices / build_composition_commit_crossfade_slices）
            // 按 coordinated 和 visual_line_id 显式设置此字段。
            // 默认 false 避免 Composition 路径偷偷进入协同 caret mask 模式。
            is_caret_line: false,
            // Issue #815 评论 5946701331 问题1/2: 默认按真实 caret 位置驱动 +
            // 无行序信息（退回同行 x 裁切）。InsertReveal / Backspace DeleteConceal
            // 保持这个默认值；Delete 键的 DeleteConceal 由
            // `build_delete_conceal_slices` 改成 `DeleteForwardBoundary`，
            // 行序由两侧各自的 canonical 快照填入。
            ingest_boundary_driver: IngestBoundaryDriver::CaretPosition,
            ingest_boundary_from_x: None,
            ingest_line_ord: None,
            ingest_from_line_ord: None,
            ingest_to_line_ord: None,
            ingest_line_top: None,
            ingest_line_bottom: None,
            visual_line_id,
            start_fraction: 0.0,
            static_hidden_document_rects: Vec::new(),
            crossfade_group_id: None,
            crossfade_side: None,
            reflow_anchors: Vec::new(),
        }
    }

    /// 创建 Delete 吞字切片。
    ///
    /// 文字始终在 `from_document_rect` 位置，动画进度控制可见纹理宽度从 100% → 0%。
    /// `conceal_to_left_edge` 决定收进方向：true 向左边缘收缩（保留左段，Backspace），
    /// false 向右边缘收缩（保留右段，Delete 键）。
    /// Issue #808: `cursor_x`/`cursor_y` 是新 caret 位置（吞字终点），保存为
    /// `caret_anchor_x`/`caret_anchor_y`。`compute_frame` 用这个锚点决定遮罩
    /// 向哪一侧收拢——文字真正"被 caret 吞进去"。Backspace/Delete 两个方向按
    /// 新旧 caret 与 glyph 的相对位置决定收拢侧。
    pub fn delete_conceal(
        _key: VisualTransactionKey,
        snapshot_id: LineSnapshotId,
        source_rect: SourceRect,
        from_document_rect: SourceRect,
        cursor_x: f64,
        cursor_y: f64,
        byte_start: usize,
        byte_end: usize,
        shaping_identity: Option<ShapingIdentity>,
        conceal_to_left_edge: bool,
        visual_line_id: Option<usize>,
    ) -> Self {
        Self {
            kind: AnimatedSliceKind::DeleteConceal,
            snapshot_id,
            source_rect,
            to_document_rect: from_document_rect.clone(),
            line_mask_left: from_document_rect.x,
            line_mask_right: from_document_rect.x + from_document_rect.w,
            from_document_rect,
            opacity_from: 1.0,
            opacity_to: 1.0,
            scale_from: 1.0,
            scale_to: 1.0,
            byte_start,
            byte_end,
            shaping_identity,
            conceal_to_left_edge,
            caret_anchor_x: cursor_x,
            caret_anchor_y: cursor_y,
            // Issue #808 评论 5917296533 问题4: 构造函数默认 is_caret_line=false。
            // 调用方（build_delete_conceal_slices / build_composition_commit_crossfade_slices）
            // 按 coordinated 和 visual_line_id 显式设置此字段。
            // 默认 false 避免 Composition 路径偷偷进入协同 caret mask 模式。
            is_caret_line: false,
            // Issue #815 评论 5946701331 问题1/2: 默认按真实 caret 位置驱动 +
            // 无行序信息（退回同行 x 裁切）。InsertReveal / Backspace DeleteConceal
            // 保持这个默认值；Delete 键的 DeleteConceal 由
            // `build_delete_conceal_slices` 改成 `DeleteForwardBoundary`，
            // 行序由两侧各自的 canonical 快照填入。
            ingest_boundary_driver: IngestBoundaryDriver::CaretPosition,
            ingest_boundary_from_x: None,
            ingest_line_ord: None,
            ingest_from_line_ord: None,
            ingest_to_line_ord: None,
            ingest_line_top: None,
            ingest_line_bottom: None,
            visual_line_id,
            start_fraction: 0.0,
            static_hidden_document_rects: Vec::new(),
            crossfade_group_id: None,
            crossfade_side: None,
            reflow_anchors: Vec::new(),
        }
    }

    /// 创建 ReflowMove 切片——shaping 不变，复用旧快照纹理做几何移动。
    ///
    /// `_new_snapshot_id`/`_new_source_rect` 当前未使用（Move 复用旧纹理），
    /// 保留参数签名与 ReflowCrossFade 对称，未来可能用于纹理缓存优化。
    /// Issue #738 评论 5789470425 问题2: 同时构造单个 ReflowAnchor 保存该 cluster
    /// 的完整身份，merge_two 合并后 anchors 列表保留所有子 cluster 身份。
    pub fn reflow_move(
        _key: VisualTransactionKey,
        old_snapshot_id: LineSnapshotId,
        old_source_rect: SourceRect,
        from_document_rect: SourceRect,
        _new_snapshot_id: LineSnapshotId,
        _new_source_rect: SourceRect,
        to_document_rect: SourceRect,
        byte_start: usize,
        byte_end: usize,
        shaping_identity: Option<ShapingIdentity>,
    ) -> Self {
        let anchor = ReflowAnchor {
            byte_start,
            byte_end,
            shaping_identity: shaping_identity.clone(),
            from_document_rect: from_document_rect.clone(),
            to_document_rect: to_document_rect.clone(),
            source_rect: old_source_rect.clone(),
            snapshot_id: old_snapshot_id,
            visual_line_id: None,
        };
        Self {
            kind: AnimatedSliceKind::ReflowMove,
            snapshot_id: old_snapshot_id,
            source_rect: old_source_rect,
            // Reflow 不使用行级 mask；保持等于自己的 from rect。
            line_mask_left: from_document_rect.x,
            line_mask_right: from_document_rect.x + from_document_rect.w,
            from_document_rect,
            to_document_rect,
            opacity_from: 1.0,
            opacity_to: 1.0,
            scale_from: 1.0,
            scale_to: 1.0,
            byte_start,
            byte_end,
            shaping_identity,
            conceal_to_left_edge: false,
            caret_anchor_x: 0.0,
            caret_anchor_y: 0.0,
            is_caret_line: false,
            // Issue #815 评论 5946701331 问题1/2: 默认按真实 caret 位置驱动 +
            // 无行序信息（退回同行 x 裁切）。InsertReveal / Backspace DeleteConceal
            // 保持这个默认值；Delete 键的 DeleteConceal 由
            // `build_delete_conceal_slices` 改成 `DeleteForwardBoundary`，
            // 行序由两侧各自的 canonical 快照填入。
            ingest_boundary_driver: IngestBoundaryDriver::CaretPosition,
            ingest_boundary_from_x: None,
            ingest_line_ord: None,
            ingest_from_line_ord: None,
            ingest_to_line_ord: None,
            ingest_line_top: None,
            ingest_line_bottom: None,
            visual_line_id: None,
            start_fraction: 0.0,
            static_hidden_document_rects: Vec::new(),
            crossfade_group_id: None,
            crossfade_side: None,
            reflow_anchors: vec![anchor],
        }
    }

    /// Issue #738 评论 5788513592 问题3: CrossFade old/new 必须写入真实的 shaping identity，
    /// 不能继续用 None。rebind_timed_units_to_canonical 用 shaping identity 判断能否按新布局
    /// 继续复用旧视觉；None 会导致 is_same_shaping 恒为 false → 必然 Remove。
    /// `shaping_identity` 传 old/new 侧各自真实的 cluster shaping identity。
    /// Issue #738 评论 5789470425 问题3: 增加 `crossfade_group_id` 参数，old/new 两侧
    /// 共享同一 group_id，reconcile 以 group 为单位成对重绑。
    /// Issue #738 评论 5789470425 问题2: 同时构造单个 ReflowAnchor 保存该 cluster 身份。
    pub fn reflow_crossfade_old(
        _key: VisualTransactionKey,
        snapshot_id: LineSnapshotId,
        source_rect: SourceRect,
        from_document_rect: SourceRect,
        to_document_rect: SourceRect,
        byte_start: usize,
        byte_end: usize,
        shaping_identity: Option<ShapingIdentity>,
        crossfade_group_id: Option<u64>,
    ) -> Self {
        let anchor = ReflowAnchor {
            byte_start,
            byte_end,
            shaping_identity: shaping_identity.clone(),
            from_document_rect: from_document_rect.clone(),
            to_document_rect: to_document_rect.clone(),
            source_rect: source_rect.clone(),
            snapshot_id,
            visual_line_id: None,
        };
        Self {
            kind: AnimatedSliceKind::ReflowCrossFade,
            snapshot_id,
            source_rect,
            // Reflow 不使用行级 mask；保持等于自己的 from rect。
            line_mask_left: from_document_rect.x,
            line_mask_right: from_document_rect.x + from_document_rect.w,
            from_document_rect,
            to_document_rect,
            opacity_from: 1.0,
            opacity_to: 0.0,
            scale_from: 1.0,
            scale_to: 1.0,
            byte_start,
            byte_end,
            shaping_identity,
            conceal_to_left_edge: false,
            caret_anchor_x: 0.0,
            caret_anchor_y: 0.0,
            is_caret_line: false,
            // Issue #815 评论 5946701331 问题1/2: 默认按真实 caret 位置驱动 +
            // 无行序信息（退回同行 x 裁切）。InsertReveal / Backspace DeleteConceal
            // 保持这个默认值；Delete 键的 DeleteConceal 由
            // `build_delete_conceal_slices` 改成 `DeleteForwardBoundary`，
            // 行序由两侧各自的 canonical 快照填入。
            ingest_boundary_driver: IngestBoundaryDriver::CaretPosition,
            ingest_boundary_from_x: None,
            ingest_line_ord: None,
            ingest_from_line_ord: None,
            ingest_to_line_ord: None,
            ingest_line_top: None,
            ingest_line_bottom: None,
            visual_line_id: None,
            start_fraction: 0.0,
            static_hidden_document_rects: Vec::new(),
            crossfade_group_id,
            crossfade_side: Some(CrossFadeSide::Old),
            reflow_anchors: vec![anchor],
        }
    }

    pub fn reflow_crossfade_new(
        _key: VisualTransactionKey,
        snapshot_id: LineSnapshotId,
        source_rect: SourceRect,
        from_document_rect: SourceRect,
        to_document_rect: SourceRect,
        byte_start: usize,
        byte_end: usize,
        shaping_identity: Option<ShapingIdentity>,
        crossfade_group_id: Option<u64>,
    ) -> Self {
        let anchor = ReflowAnchor {
            byte_start,
            byte_end,
            shaping_identity: shaping_identity.clone(),
            from_document_rect: from_document_rect.clone(),
            to_document_rect: to_document_rect.clone(),
            source_rect: source_rect.clone(),
            snapshot_id,
            visual_line_id: None,
        };
        Self {
            kind: AnimatedSliceKind::ReflowCrossFade,
            snapshot_id,
            source_rect,
            // Reflow 不使用行级 mask；保持等于自己的 from rect。
            line_mask_left: from_document_rect.x,
            line_mask_right: from_document_rect.x + from_document_rect.w,
            from_document_rect,
            to_document_rect,
            opacity_from: 0.0,
            opacity_to: 1.0,
            scale_from: 1.0,
            scale_to: 1.0,
            byte_start,
            byte_end,
            shaping_identity,
            conceal_to_left_edge: false,
            caret_anchor_x: 0.0,
            caret_anchor_y: 0.0,
            is_caret_line: false,
            // Issue #815 评论 5946701331 问题1/2: 默认按真实 caret 位置驱动 +
            // 无行序信息（退回同行 x 裁切）。InsertReveal / Backspace DeleteConceal
            // 保持这个默认值；Delete 键的 DeleteConceal 由
            // `build_delete_conceal_slices` 改成 `DeleteForwardBoundary`，
            // 行序由两侧各自的 canonical 快照填入。
            ingest_boundary_driver: IngestBoundaryDriver::CaretPosition,
            ingest_boundary_from_x: None,
            ingest_line_ord: None,
            ingest_from_line_ord: None,
            ingest_to_line_ord: None,
            ingest_line_top: None,
            ingest_line_bottom: None,
            visual_line_id: None,
            start_fraction: 0.0,
            static_hidden_document_rects: Vec::new(),
            crossfade_group_id,
            crossfade_side: Some(CrossFadeSide::New),
            reflow_anchors: vec![anchor],
        }
    }

    /// 接收当前已显示视觉帧的位置、透明度和可见比例，用于连续事务无跳变衔接。
    ///
    /// Issue #690 评论 5675007226 步骤 3: 对 InsertReveal/DeleteConceal，
    /// 传入的 `visible_fraction` 作为新 slice 的 `start_fraction`，
    /// 使快速连续输入时上一笔吐到一半的字从当前可见比例继续，而不是重新 0→1 / 1→0。
    /// ReflowMove/ReflowCrossFade 继续保存当前屏幕位置作为新的 from。
    pub fn rebase_from(
        &mut self,
        current_x: f64,
        current_y: f64,
        current_opacity: f64,
        visible_fraction: f64,
    ) {
        match self.kind {
            AnimatedSliceKind::InsertReveal => {
                self.start_fraction = visible_fraction.clamp(0.0, 1.0);
                let _ = (current_x, current_y, current_opacity);
            }
            AnimatedSliceKind::DeleteConceal => {
                self.start_fraction = visible_fraction.clamp(0.0, 1.0);
                let _ = (current_x, current_y, current_opacity);
            }
            AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade => {
                self.from_document_rect.x = current_x;
                self.from_document_rect.y = current_y;
                self.opacity_from = current_opacity;
                self.scale_from = 1.0;
                let _ = visible_fraction;
            }
        }
    }

    /// Issue #808 评论 5919641249 修改 5: 用自己的 rect 与行级共同 boundary 求交，
    /// 得到本 slice 这一帧的 frame rect 和 source clip。
    ///
    /// 关键点：boundary 是同一 `visual_line_id` 整行共同的（`line_mask_left/right`），
    /// 但参与求交的永远是这个 slice 自己的 `slice_rect`/`source_rect`——不是把同组
    /// 多个 cluster union 出来的大矩形。因此中间保留（不参与吞吐动画）的字符
    /// 永远不会被卷进动画层，也不会与 canonical 静态正文重影。
    fn clip_to_line_boundary(
        slice_rect: &SourceRect,
        source_rect: &SourceRect,
        boundary_left: f64,
        boundary_right: f64,
    ) -> (f64, f64, f64, f64) {
        let slice_left = slice_rect.x;
        let slice_right = slice_rect.x + slice_rect.w;
        let left = slice_left.max(boundary_left).min(slice_right);
        let right = slice_right.min(boundary_right).max(slice_left);
        let w = (right - left).max(0.0);
        let (src_x, src_w) = if slice_rect.w > 0.0 {
            (
                source_rect.x + (left - slice_left) / slice_rect.w * source_rect.w,
                source_rect.w * (w / slice_rect.w),
            )
        } else {
            (source_rect.x, 0.0)
        };
        (left, w, src_x, src_w)
    }

    /// 纯插值计算：根据"最终可见比例" `visible`（0..1）计算当前帧的 destination rect
    /// 和 source rect。
    ///
    /// Issue #808 核心语义: 文字本体固定在 canonical 最终/旧位置，动画只改变
    /// clip/mask。`visible` 由文字自己的时间线算出
    ///（`start_fraction + (target - start) * ease_out_quad(progress)`），
    /// 不再由 caret frame 驱动。caret 只决定遮罩的空间锚点/方向，不能决定文字
    /// 动画进度。
    ///
    /// Issue #810 评论 第10点 渲染层语义契约（本方法即该契约的实现）:
    /// - InsertReveal / DeleteConceal 采用真正的 clip/mask 语义：文字本体固定在
    ///   canonical 位置，动画只改变可见纹理宽度（clip），不做位移/缩放/淡入淡出。
    /// - caret 只提供空间锚点（`caret_anchor_x/y`）和方向（`conceal_to_left_edge`、
    ///   `is_caret_line`），**不提供 progress**。progress 由 unit 自己的 Timed timeline
    ///   算出后作为 `visible` 参数传入本方法。
    /// - Issue #815 评论 6042062633 修改 5：本入口只服务 `VisualUnitTiming::Timed` 的
    ///   unit——即非协同 InsertReveal/DeleteConceal 和全部 Reflow。协同模式的
    ///   InsertReveal/DeleteConceal 是 `VisualUnitTiming::CaretTrack`，**不准**把 caret
    ///   track 的进度换算成独立 0..1 再喂进这里，必须走
    ///   [`AnimatedSlice::compute_frame_by_caret_ingest`]。
    /// - 跨行按 visual line 分别处理遮罩锚点：每个 slice 带自己的 `visual_line_id`
    ///   和 `is_caret_line`，`is_caret_line=false` 时用本行级 extent 边缘做锚点，
    ///   不拿上一行的 x 裁下一行。
    ///
    /// Issue #808 评论 5919641249: InsertReveal/DeleteConceal 不再按每个 cluster
    /// 自己的矩形边缘缩放，也不 union 成大图块。同一行先按文字自己的 progress
    /// 算一条行级共同吞吐 boundary（由 `line_mask_left/right` + 锚点决定），
    /// 每个 slice 用自己的 rect 与这条 boundary 求交。
    ///
    /// InsertReveal: 锚点取旧 caret（caret 行）或行首（跨行其他行），字符像从
    /// caret 后面吐出来；跨行时按 visual_line_id 分段，每行一条自己的 boundary。
    /// DeleteConceal: 锚点取最终 caret（caret 行）或行级 extent 对应边缘，
    /// 字符像被 caret 吞进去；收拢侧由锚点相对行级 extent 的位置决定。
    pub fn compute_frame(&self, visible: f64) -> AnimatedSliceFrame {
        let visible = visible.clamp(0.0, 1.0);
        match self.kind {
            AnimatedSliceKind::InsertReveal => {
                // 先算行级共同 boundary，再与本 slice 自己的 to_document_rect 求交。
                // is_caret_line=true：锚点是真实 caret（clamp 到行级 extent）；
                // is_caret_line=false：锚点是行级 extent 左边缘（其他行从行首展开）。
                // 锚点在行级 extent 左半 → 从锚点向右展开；右半 → 从锚点向左展开。
                let frame_h = self.to_document_rect.h;
                let mask_left = self.line_mask_left;
                let mask_right = self.line_mask_right;
                let anchor_x = if self.is_caret_line {
                    self.caret_anchor_x.clamp(mask_left, mask_right)
                } else {
                    mask_left
                };
                let (boundary_left, boundary_right) =
                    if anchor_x > mask_left + (mask_right - mask_left) * 0.5 {
                        // 从锚点向左展开到行级 extent 左边缘
                        (anchor_x - (anchor_x - mask_left) * visible, anchor_x)
                    } else {
                        // 从锚点向右展开到行级 extent 右边缘
                        (anchor_x, anchor_x + (mask_right - anchor_x) * visible)
                    };
                let (frame_x, frame_w, src_x, src_w) = Self::clip_to_line_boundary(
                    &self.to_document_rect,
                    &self.source_rect,
                    boundary_left,
                    boundary_right,
                );
                let frame_source_rect = SourceRect {
                    x: src_x,
                    y: self.source_rect.y,
                    w: src_w,
                    h: self.source_rect.h,
                };
                AnimatedSliceFrame {
                    x: frame_x,
                    y: self.to_document_rect.y,
                    w: frame_w,
                    h: frame_h,
                    opacity: 1.0,
                    source_rect: frame_source_rect,
                    snapshot_id: self.snapshot_id,
                }
            }
            AnimatedSliceKind::DeleteConceal => {
                // 先算行级共同 boundary，再与本 slice 自己的 from_document_rect 求交。
                // is_caret_line=true：锚点是 final/new caret（clamp 到行级 extent）；
                // is_caret_line=false：锚点取行级 extent 对应边缘
                //（conceal_to_left_edge=true 取左边缘，false 取右边缘）。
                //
                // Issue #808 评论 5918236360 问题3: 收拢侧由锚点相对行级 extent 中点
                // 决定（Backspace 的 final caret 在左半 → 向左收），不再用 old caret
                // 推出来的 conceal_to_left_edge 选 caret 行分支：
                // - 锚点在左半：左边界固定在锚点，右边界从 extent 右边收向锚点。
                //   visible=1 → 完整 extent，visible=0 → w=0。
                // - 锚点在右半：右边界固定在锚点，左边界从 extent 左边收向锚点。
                //   visible=1 → 完整 extent，visible=0 → w=0。
                let frame_h = self.from_document_rect.h;
                let mask_left = self.line_mask_left;
                let mask_right = self.line_mask_right;
                let anchor_x = if self.is_caret_line {
                    self.caret_anchor_x.clamp(mask_left, mask_right)
                } else if self.conceal_to_left_edge {
                    mask_left
                } else {
                    mask_right
                };
                let (boundary_left, boundary_right) =
                    if anchor_x <= mask_left + (mask_right - mask_left) * 0.5 {
                        // 锚点在左半 → 向左收（右段先消失）
                        (anchor_x, anchor_x + (mask_right - anchor_x) * visible)
                    } else {
                        // 锚点在右半 → 向右收（左段先消失）
                        (anchor_x - (anchor_x - mask_left) * visible, anchor_x)
                    };
                let (frame_x, frame_w, src_x, src_w) = Self::clip_to_line_boundary(
                    &self.from_document_rect,
                    &self.source_rect,
                    boundary_left,
                    boundary_right,
                );
                let frame_source_rect = SourceRect {
                    x: src_x,
                    y: self.source_rect.y,
                    w: src_w,
                    h: self.source_rect.h,
                };
                AnimatedSliceFrame {
                    x: frame_x,
                    y: self.from_document_rect.y,
                    w: frame_w,
                    h: frame_h,
                    opacity: 1.0,
                    source_rect: frame_source_rect,
                    snapshot_id: self.snapshot_id,
                }
            }
            AnimatedSliceKind::ReflowMove => {
                let x = self.from_document_rect.x
                    + (self.to_document_rect.x - self.from_document_rect.x) * visible;
                let y = self.from_document_rect.y
                    + (self.to_document_rect.y - self.from_document_rect.y) * visible;
                AnimatedSliceFrame {
                    x,
                    y,
                    w: self.to_document_rect.w,
                    h: self.to_document_rect.h,
                    opacity: 1.0,
                    source_rect: self.source_rect.clone(),
                    snapshot_id: self.snapshot_id,
                }
            }
            AnimatedSliceKind::ReflowCrossFade => {
                let x = self.from_document_rect.x
                    + (self.to_document_rect.x - self.from_document_rect.x) * visible;
                let y = self.from_document_rect.y
                    + (self.to_document_rect.y - self.from_document_rect.y) * visible;
                let w = self.from_document_rect.w
                    + (self.to_document_rect.w - self.from_document_rect.w) * visible;
                let h = self.from_document_rect.h
                    + (self.to_document_rect.h - self.from_document_rect.h) * visible;
                let opacity = self.opacity_from + (self.opacity_to - self.opacity_from) * visible;
                AnimatedSliceFrame {
                    x,
                    y,
                    w,
                    h,
                    opacity,
                    source_rect: self.source_rect.clone(),
                    snapshot_id: self.snapshot_id,
                }
            }
        }
    }

    /// Issue #815 评论 6042062633 修改 5 / Issue #815 评论 5946701331 问题1+2:
    /// 按"当前 caret 帧"算吞吐 clip。
    ///
    /// 协同模式的 InsertReveal/DeleteConceal **没有自己的 0..1 visible fraction**。
    /// 本方法的边界是 cursor track 本帧采样出来的坐标，不是
    /// `anchor + extent * text_progress`。
    ///
    /// `caret_x` / `caret_progress` 必须来自同一笔事务 cursor track 的**同一次**
    /// 采样（`sample_caret_track_frame`），和光标层共用这一份，文字不自己再算时间。
    ///
    /// Issue #815 评论 5946701331 问题1（边界驱动）：
    /// - [`IngestBoundaryDriver::CaretPosition`]：边界就是本帧 `caret_x`。
    ///   InsertReveal 从编辑前的 caret 展开；Backspace 的真实 caret 自己会往左走，
    ///   吞字边界直接跟着它收拢。
    /// - [`IngestBoundaryDriver::DeleteForwardBoundary`]：Delete 键时 caret 原地不动，
    ///   若边界仍等于 `caret_x` 则 `anchor == caret_x`、第一帧宽度就是 0，吞字动画
    ///   直接消失。所以边界从被删区间右端（`ingest_boundary_from_x`，回退
    ///   `line_mask_right`）朝 `caret_x` 收拢，收拢量由同一笔事务 track 的
    ///   `caret_progress` 驱动。
    ///
    /// 两种驱动都用 `min/max` 归一化前后顺序，所以前删（终点在左）和前插
    /// （终点在右）都走同一条公式。
    ///
    /// Issue #815 评论 5946701331 问题2（跨行）：
    /// 跨软换行/跨段不再用 `slice_line < caret_line` / `>` 这种把"向前走"写死的
    /// 比较——Backspace 跨行时 caret 是从**较大的** old 行走到**较小的** final 行，
    /// 那种写法会把方向判断反。
    ///
    /// 改为在**同一份 snapshot 内**比较行序：起点行（`ingest_from_line_ord`）、
    /// 终点行（`ingest_to_line_ord`）和本 slice 自己的行（`ingest_line_ord`）都由
    /// 该侧的 canonical 快照算出——Insert 用 new snapshot，Delete 用 old snapshot。
    /// 方向由 `ingest_to_line_ord - ingest_from_line_ord` 的符号给出（forward /
    /// backward 都支持），本帧边界走到哪一行由 `caret_progress` 线性插值得到。
    /// 绝不把 old 的行序和 new 的行序做大小比较，也绝不用上一行的 caret.x
    /// 裁下一行。
    ///
    /// 行序缺失（拿不到同侧行）时防御性地按同一行处理，只用边界 x 裁本行——
    /// 这仍然比退化成独立 0..1 进度更接近协同语义。
    ///
    /// Issue #815 评论 5947728704 问题1：`caret_x` 是当前行的横向吞吐边界，
    /// `caret_y` 是当前帧真实 caret 的 y（用它判断"走到哪一条视觉行"），
    /// `caret_progress` **只**留给 `DeleteForwardBoundary` 这种真实 caret 不动、
    /// 必须从同一 cursor track 取推进量的特殊情况——progress 不再负责推导行序。
    pub(crate) fn compute_frame_by_caret_ingest(
        &self,
        caret_x: f64,
        caret_y: f64,
        caret_progress: f64,
    ) -> AnimatedSliceFrame {
        // ReflowMove/ReflowCrossFade 始终是独立 Timed（见
        // `VisualUnitTiming::default_for_kind_with_coordinated`），不会走到这个入口；
        // 这里按初始帧防御。
        let (slice_rect, fully_shown) = match self.kind {
            AnimatedSliceKind::InsertReveal => (&self.to_document_rect, true),
            AnimatedSliceKind::DeleteConceal => (&self.from_document_rect, false),
            AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade => {
                return self.compute_frame(0.0);
            }
        };
        let mask_left = self.line_mask_left;
        let mask_right = self.line_mask_right;

        // 本帧吞字/吐字边界 x。Issue #815 评论 5946701331 问题1。
        let boundary_x = match self.ingest_boundary_driver {
            IngestBoundaryDriver::CaretPosition => caret_x,
            IngestBoundaryDriver::DeleteForwardBoundary => {
                // Delete 键：caret 固定，边界从被删区间右端朝 caret.x 收拢。
                let from_x = self.ingest_boundary_from_x.unwrap_or(mask_right);
                let progress = caret_progress.clamp(0.0, 1.0);
                from_x + (caret_x - from_x) * progress
            }
        };
        // 吞吐路径的另一端（构造期写入）：
        // - CaretPosition：InsertReveal 是编辑前 caret；Backspace DeleteConceal 是
        //   删除后的最终 caret。
        // - DeleteForwardBoundary：Delete 键的最终 caret 固定在被删区间左端，
        //   所以另一端就是本帧的 `caret_x` 本身。边界从被删区间右端朝它收拢，
        //   收拢量由 `caret_progress` 驱动——不是把静止 caret 当进度，
        //   也不是回到独立文字 timeline（轨迹仍由本事务 cursor track 的 progress 定义）。
        let path_other_end_x = match self.ingest_boundary_driver {
            IngestBoundaryDriver::CaretPosition => self.caret_anchor_x,
            IngestBoundaryDriver::DeleteForwardBoundary => caret_x,
        };

        // Issue #815 评论 5946701331 问题2: 同侧行序 + 方向感知的跨行相位。
        let phase = self.ingest_line_phase(caret_y);
        let (boundary_left, boundary_right) = match phase {
            IngestLinePhase::Passed => {
                if fully_shown {
                    (mask_left, mask_right)
                } else {
                    (mask_left, mask_left)
                }
            }
            IngestLinePhase::NotReached => {
                if fully_shown {
                    (mask_left, mask_left)
                } else {
                    (mask_left, mask_right)
                }
            }
            IngestLinePhase::OnCurrentLine => (
                path_other_end_x.min(boundary_x),
                path_other_end_x.max(boundary_x),
            ),
        };
        self.clip_ingest_frame(slice_rect, boundary_left, boundary_right)
    }

    /// Issue #815 评论 5947728704 问题1: 本帧吞字/吐字边界相对本 slice 所在行的相位。
    ///
    /// **当前在哪一条视觉行，来自本帧真实 `caret_y`**——屏幕上的 caret 是
    /// `ease_out_cubic(progress)` 之后的几何，如果这里再用 raw `progress` 推一遍行序，
    /// 就会造出第二套和 caret 不一致的隐藏运动（上一轮修掉的老毛病换了个地方出现）。
    ///
    /// 同侧行序（`ingest_from_line_ord` / `ingest_to_line_ord`）只提供**方向**：
    /// - `to_ord > from_ord`（向下走，如 Insert / 跨行前进）
    /// - `to_ord < from_ord`（向上走，如 Backspace 跨行）
    ///
    /// 判定用的是本 slice 在**同一侧** canonical 里的真实行范围
    /// （`ingest_line_top` / `ingest_line_bottom`），所以任意一帧最多只有一行是
    /// `OnCurrentLine`——不会出现两行同时拿同一个 caret.x 裁切。
    ///
    /// 行序或行几何缺失、或 `from_ord == to_ord`（单行）时返回
    /// [`IngestLinePhase::OnCurrentLine`]，退回按本帧边界 x 裁本行。
    fn ingest_line_phase(&self, caret_y: f64) -> IngestLinePhase {
        let (Some(from_ord), Some(to_ord), Some(line_top), Some(line_bottom)) = (
            self.ingest_from_line_ord,
            self.ingest_to_line_ord,
            self.ingest_line_top,
            self.ingest_line_bottom,
        ) else {
            return IngestLinePhase::OnCurrentLine;
        };
        if from_ord == to_ord {
            return IngestLinePhase::OnCurrentLine;
        }
        let forward = to_ord > from_ord;
        if forward {
            if caret_y >= line_bottom {
                IngestLinePhase::Passed
            } else if caret_y < line_top {
                IngestLinePhase::NotReached
            } else {
                IngestLinePhase::OnCurrentLine
            }
        } else if caret_y < line_top {
            IngestLinePhase::Passed
        } else if caret_y >= line_bottom {
            IngestLinePhase::NotReached
        } else {
            IngestLinePhase::OnCurrentLine
        }
    }

    /// Issue #815 评论 6042062633 修改 5: 用行级 boundary 与本 slice 自己的 rect 求交，
    /// 得到本 slice 这一帧的 frame rect 与 source clip。
    ///
    /// 与 [`AnimatedSlice::clip_to_line_boundary`] 同样的语义（永远只裁自己的 rect，
    /// 不 union 同组 cluster），只是顺手组装出完整 `AnimatedSliceFrame`。
    fn clip_ingest_frame(
        &self,
        slice_rect: &SourceRect,
        boundary_left: f64,
        boundary_right: f64,
    ) -> AnimatedSliceFrame {
        let (frame_x, frame_w, src_x, src_w) = Self::clip_to_line_boundary(
            slice_rect,
            &self.source_rect,
            boundary_left,
            boundary_right,
        );
        AnimatedSliceFrame {
            x: frame_x,
            y: slice_rect.y,
            w: frame_w,
            h: slice_rect.h,
            opacity: 1.0,
            source_rect: SourceRect {
                x: src_x,
                y: self.source_rect.y,
                w: src_w,
                h: self.source_rect.h,
            },
            snapshot_id: self.snapshot_id,
        }
    }

    /// Issue #738 评论 5796693007 问题2: 按 `kind` 采 ReflowAnchor 当前帧 document rect
    /// 的共用 helper，与 `AnimatedSlice::compute_frame` 对 ReflowMove/ReflowCrossFade 的
    /// document rect 语义保持完全一致。
    ///
    /// - `ReflowMove`: x/y 按 `visible` 插值，**w/h 直接使用 `to.w`/`to.h`**
    ///   （对应 `compute_frame` 第 481-494 行 `w: self.to_document_rect.w, h: self.to_document_rect.h`）。
    /// - `ReflowCrossFade`: x/y/w/h 四项全部按 `visible` 从 from→to 插值
    ///   （对应 `compute_frame` 第 496-514 行）。
    /// - `InsertReveal`/`DeleteConceal`: 不走 reflow anchor 路径，防御性返回 `to.clone()`。
    ///
    /// `rebind_timed_units_to_canonical` 采 ReflowMove anchor current_rect 时必须用这个
    /// helper，不能把 ReflowCrossFade 的四项插值规则误套给 ReflowMove。否则 from/to 尺寸
    /// 不同时，rebind/split 的第一帧仍可能尺寸跳变（用户这一帧真正看到的 rect 与采出来的
    /// current_rect 不一致）。
    pub(crate) fn sample_current_document_rect(
        kind: AnimatedSliceKind,
        from: &SourceRect,
        to: &SourceRect,
        visible: f64,
    ) -> SourceRect {
        let visible = visible.clamp(0.0, 1.0);
        match kind {
            AnimatedSliceKind::ReflowMove => SourceRect {
                x: from.x + (to.x - from.x) * visible,
                y: from.y + (to.y - from.y) * visible,
                // 与 compute_frame(ReflowMove) 一致：w/h 直接用 to，不插值。
                w: to.w,
                h: to.h,
            },
            AnimatedSliceKind::ReflowCrossFade => SourceRect {
                x: from.x + (to.x - from.x) * visible,
                y: from.y + (to.y - from.y) * visible,
                w: from.w + (to.w - from.w) * visible,
                h: from.h + (to.h - from.h) * visible,
            },
            // InsertReveal/DeleteConceal 不走 reflow anchor 路径，防御性返回 to。
            AnimatedSliceKind::InsertReveal | AnimatedSliceKind::DeleteConceal => to.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct AnimatedSliceFrame {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub opacity: f64,
    pub source_rect: SourceRect,
    pub snapshot_id: LineSnapshotId,
}

#[cfg(test)]
mod ingest_tests;
