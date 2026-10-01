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
    ///   `is_caret_line`），**不提供 progress**。progress 由文字自己的 Timed timeline
    ///   算出后作为 `visible` 参数传入本方法。
    /// - **不允许因当前没有 caret Tween 就取消文字动画**。本方法是纯函数，不检查
    ///   caret Tween 是否存在；消费方（render_plan_builder）对所有 Timed unit 一律
    ///   调用 `compute_frame(unit.current_visible_fraction(now))`，文字动画独立于
    ///   caret ownership/epoch 推进。
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
