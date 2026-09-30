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

    /// 纯插值计算：根据"最终可见比例" `visible`（0..1）计算当前帧的 destination rect
    /// 和 source rect。
    ///
    /// Issue #808 核心语义: 文字本体固定在 canonical 最终/旧位置，动画只改变
    /// clip/mask。`visible` 由文字自己的时间线算出
    ///（`start_fraction + (target - start) * ease_out_quad(progress)`），
    /// 不再由 caret frame 驱动。caret 只决定遮罩的空间锚点/方向，不能决定文字
    /// 动画进度。
    ///
    /// InsertReveal: 遮罩从旧 caret 所在侧（`caret_anchor_x`）开始打开，字符像
    /// 从 caret 后面吐出来。跨行时按 visual_line_id 分段，每个 slice 带自己的
    /// caret 锚点，不能拿上一行的 x 去裁下一行。
    /// DeleteConceal: 遮罩向最终 caret 所在侧收拢，字符像被 caret 吞进去。
    /// Backspace/Delete 两个方向按 `conceal_to_left_edge` 决定收拢侧。
    pub fn compute_frame(&self, visible: f64) -> AnimatedSliceFrame {
        let visible = visible.clamp(0.0, 1.0);
        match self.kind {
            AnimatedSliceKind::InsertReveal => {
                // Issue #808 评论 5916391891 修改 1: 遮罩从 caret_anchor_x 开始打开。
                // is_caret_line=true 时用 caret_anchor_x 做锚点（文字从 caret 处吐出来）；
                // is_caret_line=false 时用行首 text_left 做锚点（跨行其他行从行首展开）。
                // caret 在文字左半：从 caret_anchor_x 向右展开。
                // caret 在文字右半：从 caret_anchor_x 向左展开。
                // frame_x/frame_w/src_x/src_w 都基于 anchor_x 计算，不是基于 text_left/text_right。
                let text_left = self.to_document_rect.x;
                let text_right = self.to_document_rect.x + self.to_document_rect.w;
                let text_w = self.to_document_rect.w;
                let frame_h = self.to_document_rect.h;

                let anchor_x = if self.is_caret_line {
                    self.caret_anchor_x.clamp(text_left, text_right)
                } else {
                    text_left
                };

                let (frame_x, frame_w, src_x, src_w) = if text_w <= 0.0 {
                    (text_left, 0.0, self.source_rect.x, 0.0)
                } else {
                    let reveal_from_right = anchor_x > text_left + text_w * 0.5;
                    if !reveal_from_right {
                        // 从 anchor_x 向右展开到 text_right
                        let full_extent = (text_right - anchor_x).max(0.0);
                        let w = full_extent * visible;
                        let sx = self.source_rect.x
                            + (anchor_x - text_left) / text_w * self.source_rect.w;
                        let sw = self.source_rect.w * (full_extent / text_w) * visible;
                        (anchor_x, w, sx, sw)
                    } else {
                        // 从 anchor_x 向左展开到 text_left
                        let full_extent = (anchor_x - text_left).max(0.0);
                        let w = full_extent * visible;
                        let x = anchor_x - w;
                        let sw = self.source_rect.w * (full_extent / text_w) * visible;
                        let sx = self.source_rect.x
                            + (full_extent - w) / text_w * self.source_rect.w;
                        (x, w, sx, sw)
                    }
                };
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
                // Issue #808 评论 5916391891 修改 1: 遮罩向 caret_anchor_x 收拢。
                // is_caret_line=true 时用 caret_anchor_x 做锚点（文字被 caret 吞进去）；
                // is_caret_line=false 时用行首/行尾做锚点（跨行其他行向行首/行尾收拢）。
                // conceal_to_left_edge=true：可见区域左边界固定在 from_left，
                //   右边界从 from_right 收向 anchor_x。
                // conceal_to_left_edge=false：可见区域右边界固定在 from_right，
                //   左边界从 from_left 收向 anchor_x。
                let from_left = self.from_document_rect.x;
                let from_right = self.from_document_rect.x + self.from_document_rect.w;
                let from_w = self.from_document_rect.w;
                let frame_h = self.from_document_rect.h;

                let anchor_x = if self.is_caret_line {
                    self.caret_anchor_x.clamp(from_left, from_right)
                } else if self.conceal_to_left_edge {
                    from_left
                } else {
                    from_right
                };

                let (frame_x, frame_w, src_x, src_w) = if from_w <= 0.0 {
                    (from_left, 0.0, self.source_rect.x, 0.0)
                } else if self.is_caret_line {
                    // Issue #808 评论 5917296533 问题2: 前向 Delete 遮罩公式修复。
                    // caret 在删除区域一侧时，整段遮罩向 caret 收拢，最终宽度归零。
                    // 旧公式 left_boundary = anchor_x + (from_left - anchor_x) * visible
                    // 在 anchor_x == from_left（Delete 键，新 caret 在被删字符左边）时
                    // 恒等于 from_left，frame_w 不变，文字几乎不缩。
                    //
                    // Issue #808 评论 5918236360 问题3: coordinated 模式下不能用
                    // old caret 推出来的 conceal_to_left_edge 决定收拢侧。Backspace 时
                    // old caret=160（靠右）→ conceal_to_left_edge=true，但 final caret=100
                    // （靠左）应该向左收，用 true 分支公式会导致 visible=1 时 fw=0
                    // （文字从第一帧就完全不可见）。
                    // 修复：用 anchor_x（final/new caret）相对 deleted extent 中点决定方向。
                    // - final caret 在左半（anchor_x <= from_left + from_w * 0.5）→ 向左收：
                    //   左边界固定在 anchor_x，右边界从 from_right 收向 anchor。
                    //   visible=1 → w=from_right-anchor，visible=0 → w=0。
                    // - final caret 在右半 → 向右收：
                    //   右边界固定在 anchor_x，左边界从 from_left 收向 anchor。
                    //   visible=1 → w=anchor-from_left，visible=0 → w=0。
                    let shrink_to_left = anchor_x <= from_left + from_w * 0.5;
                    if shrink_to_left {
                        // final caret 在左半 → 向左收
                        let left_boundary = anchor_x;
                        let right_boundary = anchor_x + (from_right - anchor_x) * visible;
                        let fw = (right_boundary - left_boundary).max(0.0);
                        let sx = self.source_rect.x
                            + (left_boundary - from_left) / from_w * self.source_rect.w;
                        let sw = self.source_rect.w * (fw / from_w);
                        (left_boundary, fw, sx, sw)
                    } else {
                        // final caret 在右半 → 向右收
                        let right_boundary = anchor_x;
                        let left_boundary = anchor_x - (anchor_x - from_left) * visible;
                        let fw = (right_boundary - left_boundary).max(0.0);
                        let sx = self.source_rect.x
                            + (left_boundary - from_left) / from_w * self.source_rect.w;
                        let sw = self.source_rect.w * (fw / from_w);
                        (left_boundary, fw, sx, sw)
                    }
                } else if self.conceal_to_left_edge {
                    // 跨行（is_caret_line=false）：向行首收，原公式保持。
                    // 可见区域左边界固定在 from_left，右边界从 from_right 收向 anchor_x
                    let right_boundary = anchor_x + (from_right - anchor_x) * visible;
                    let fw = (right_boundary - from_left).max(0.0);
                    let sx = self.source_rect.x;
                    let sw = self.source_rect.w * (fw / from_w);
                    (from_left, fw, sx, sw)
                } else {
                    // 跨行（is_caret_line=false）：向行尾收，原公式保持。
                    // 可见区域右边界固定在 from_right，左边界从 from_left 收向 anchor_x
                    let left_boundary = anchor_x + (from_left - anchor_x) * visible;
                    let fw = (from_right - left_boundary).max(0.0);
                    let sx = self.source_rect.x
                        + (left_boundary - from_left) / from_w * self.source_rect.w;
                    let sw = self.source_rect.w * (fw / from_w);
                    (left_boundary, fw, sx, sw)
                };
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
