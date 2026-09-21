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
}

impl AnimatedSlice {
    /// Issue #690 评论 5675007226 步骤 2: 协同动画唯一 easing 函数。
    ///
    /// Reveal、Conceal、Reflow 的逐帧进度，以及协调光标（跟随文字吞吐边界）共用它，
    /// 保证文字和光标沿同一条 ease-out quadratic 曲线运动，不再出现"文字甩开光标"。
    /// 普通 CursorOnly（方向键、Home/End、鼠标点选后的平滑移动）仍保留自己的平滑曲线，
    /// 不调用本函数。
    pub(crate) fn ease_out_quad(p: f64) -> f64 {
        let p = p.clamp(0.0, 1.0);
        1.0 - (1.0 - p).powi(2)
    }

    /// 创建 Insert 吐字切片。
    ///
    /// 文字始终在 `to_document_rect` 位置，动画进度控制可见纹理宽度从 0 → 100%。
    /// `cursor_x`/`cursor_y` 保留在签名中以减少调用方改动，但不再用于动画起点。
    pub fn insert_reveal(
        _key: VisualTransactionKey,
        snapshot_id: LineSnapshotId,
        source_rect: SourceRect,
        to_document_rect: SourceRect,
        _cursor_x: f64,
        _cursor_y: f64,
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
            visual_line_id,
            start_fraction: 0.0,
            static_hidden_document_rects: Vec::new(),
        }
    }

    /// 创建 Delete 吞字切片。
    ///
    /// 文字始终在 `from_document_rect` 位置，动画进度控制可见纹理宽度从 100% → 0%。
    /// `conceal_to_left_edge` 决定收进方向：true 向左边缘收缩（保留左段，Backspace），
    /// false 向右边缘收缩（保留右段，Delete 键）。
    /// `cursor_x`/`cursor_y` 保留在签名中以减少调用方改动，但不再用于动画终点。
    pub fn delete_conceal(
        _key: VisualTransactionKey,
        snapshot_id: LineSnapshotId,
        source_rect: SourceRect,
        from_document_rect: SourceRect,
        _cursor_x: f64,
        _cursor_y: f64,
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
            visual_line_id,
            start_fraction: 0.0,
            static_hidden_document_rects: Vec::new(),
        }
    }

    /// 创建 ReflowMove 切片——shaping 不变，复用旧快照纹理做几何移动。
    ///
    /// `_new_snapshot_id`/`_new_source_rect` 当前未使用（Move 复用旧纹理），
    /// 保留参数签名与 ReflowCrossFade 对称，未来可能用于纹理缓存优化。
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
            visual_line_id: None,
            start_fraction: 0.0,
            static_hidden_document_rects: Vec::new(),
        }
    }

    pub fn reflow_crossfade_old(
        _key: VisualTransactionKey,
        snapshot_id: LineSnapshotId,
        source_rect: SourceRect,
        from_document_rect: SourceRect,
        to_document_rect: SourceRect,
        byte_start: usize,
        byte_end: usize,
    ) -> Self {
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
            shaping_identity: None,
            conceal_to_left_edge: false,
            visual_line_id: None,
            start_fraction: 0.0,
            static_hidden_document_rects: Vec::new(),
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
    ) -> Self {
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
            shaping_identity: None,
            conceal_to_left_edge: false,
            visual_line_id: None,
            start_fraction: 0.0,
            static_hidden_document_rects: Vec::new(),
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
    /// Issue #722 评论 5747719529 核心语义：光标本身就是吞字/吐字的视觉边界。
    /// 真正决定当前 reveal/conceal 截止位置的是这一帧的 caret geometry，不是文字 unit
    /// 自己的独立 timeline。此函数保留作为 reflow/crossfade 的纯几何插值入口；
    /// InsertReveal/DeleteConceal 的裁切边界应通过 `compute_frame_caret_driven`
    /// 直接消费本帧 coordinated caret 的位置（caret_driven_clip），caret 与文字使用
    /// 同一个 frame_now 和同一个 from→to 几何轨迹。文字不能再维护一套会和 caret
    /// 分叉的"自己什么时候完全出现/完全消失"的位置/可见度进度。
    pub fn compute_frame(&self, visible: f64) -> AnimatedSliceFrame {
        let visible = visible.clamp(0.0, 1.0);
        match self.kind {
            AnimatedSliceKind::InsertReveal => {
                // Issue #722 评论 5747719529: caret_driven_clip — 吐字时裁切边界
                // 由本帧 coordinated caret 位置决定。此回退入口用 visible 推导等效
                // caret 边界（caret_geometry_clip），保持向后兼容；主路径应调用
                // compute_frame_caret_driven 直接消费 caret geometry。
                let caret_clip_boundary =
                    self.to_document_rect.x + self.to_document_rect.w * visible;
                let reveal_from_caret = (caret_clip_boundary - self.to_document_rect.x)
                    .clamp(0.0, self.to_document_rect.w);
                let frame_h = self.to_document_rect.h;
                let frame_source_rect = SourceRect {
                    x: self.source_rect.x,
                    y: self.source_rect.y,
                    w: self.source_rect.w * visible,
                    h: self.source_rect.h,
                };
                AnimatedSliceFrame {
                    x: self.to_document_rect.x,
                    y: self.to_document_rect.y,
                    w: reveal_from_caret,
                    h: frame_h,
                    opacity: 1.0,
                    source_rect: frame_source_rect,
                    snapshot_id: self.snapshot_id,
                }
            }
            AnimatedSliceKind::DeleteConceal => {
                // Issue #722 评论 5747719529: caret_driven_clip — 吞字时裁切边界
                // 由本帧 coordinated caret 位置决定。此回退入口用 visible 推导等效
                // caret 边界（caret_geometry_clip），保持向后兼容；主路径应调用
                // compute_frame_caret_driven 直接消费 caret geometry。
                let frame_h = self.from_document_rect.h;
                let (caret_clip_boundary, frame_x, src_x) = if self.conceal_to_left_edge {
                    let caret_b = self.from_document_rect.x + self.from_document_rect.w * visible;
                    (caret_b, self.from_document_rect.x, self.source_rect.x)
                } else {
                    let caret_b =
                        self.from_document_rect.x + self.from_document_rect.w * (1.0 - visible);
                    (
                        caret_b,
                        self.from_document_rect.x + self.from_document_rect.w * (1.0 - visible),
                        self.source_rect.x + self.source_rect.w * (1.0 - visible),
                    )
                };
                let conceal_from_caret = if self.conceal_to_left_edge {
                    (caret_clip_boundary - self.from_document_rect.x)
                        .clamp(0.0, self.from_document_rect.w)
                } else {
                    (self.from_document_rect.x + self.from_document_rect.w - caret_clip_boundary)
                        .clamp(0.0, self.from_document_rect.w)
                };
                let frame_source_rect = SourceRect {
                    x: src_x,
                    y: self.source_rect.y,
                    w: self.source_rect.w * visible,
                    h: self.source_rect.h,
                };
                AnimatedSliceFrame {
                    x: frame_x,
                    y: self.from_document_rect.y,
                    w: conceal_from_caret,
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

    /// Issue #722 评论 5747719529 核心语义：caret 驱动裁切边界。
    ///
    /// 光标本身就是吞字/吐字的视觉边界。InsertReveal/DeleteConceal 的裁切边界直接
    /// 消费本帧 coordinated caret 的位置（`caret_clip_boundary`），不再由 unit 自己
    /// 的 visible fraction 驱动。caret 与文字使用同一个 frame_now 和同一个
    /// from→to 几何轨迹。
    ///
    /// - InsertReveal（吐字）：光标往前走到哪里，文字就显示到哪里。
    ///   `caret_clip_boundary` 是本帧 coordinated caret 的 x 坐标，裁切宽度 =
    ///   `caret_clip_boundary - to_document_rect.x`（已被光标"带出来"的部分）。
    /// - DeleteConceal（吞字）：光标往回走到哪里，文字就消失到哪里。
    ///   `caret_clip_boundary` 是本帧 coordinated caret 的 x 坐标，裁切宽度 =
    ///   `caret_clip_boundary - from_document_rect.x`（Backspace，conceal_to_left_edge）；
    ///   前向 Delete 时裁切宽度 = `from_document_rect.right - caret_clip_boundary`。
    /// - ReflowMove / ReflowCrossFade：不消费 caret 边界，回退到 `compute_frame(visible)`。
    ///
    /// 快速连续输入/删除时，新事务必须从当前这条视觉边界继续。上一帧光标已经扫过
    /// 的部分保持最终状态，尚未扫过的部分继续跟着新的光标边界走。快速 rebase 时
    /// 先采样当前 caret 边界，再把这个边界作为下一段动画起点。
    pub fn compute_frame_caret_driven(
        &self,
        caret_clip_boundary: f64,
        caret_clip_y: f64,
        caret_visual_line_id: Option<usize>,
        visible: f64,
    ) -> AnimatedSliceFrame {
        match self.kind {
            AnimatedSliceKind::InsertReveal => {
                // Issue #722 评论 5748596920 问题2: 跨软换行裁切按行判断。
                // caret 还没进入该 slice 所在视觉行 → InsertReveal 保持 0；
                // caret 已经越过该视觉行 → 该行已经吐出的部分保持完成；
                // 只有 caret 和 slice 在同一视觉行时，才用 caret.x 做横向裁切。
                // Issue #722 评论 5749164244 问题1: 用 Option<usize> 判断行身份。
                // None=未知走 y fallback；Some(id) 已知行按 id 比较。
                // y fallback 用半开区间 line_top <= caret_y < line_bottom，
                // 不用 abs(y - glyph_y) < glyph_h（相邻行会误判）。
                let same_line = match (caret_visual_line_id, self.visual_line_id) {
                    (Some(c_id), Some(s_id)) => c_id == s_id,
                    _ => {
                        let line_top = self.to_document_rect.y;
                        let line_bottom = self.to_document_rect.y + self.to_document_rect.h;
                        caret_clip_y >= line_top && caret_clip_y < line_bottom
                    }
                };
                let caret_above = match (caret_visual_line_id, self.visual_line_id) {
                    (Some(c_id), Some(s_id)) => c_id < s_id,
                    _ => caret_clip_y < self.to_document_rect.y,
                };
                let reveal_from_caret = if caret_above {
                    // caret 在该行上方，还没吐到该行
                    0.0
                } else if !same_line && !caret_above {
                    // caret 已经越过该行，该行完全吐出
                    self.to_document_rect.w
                } else {
                    // 同一行：用 caret.x 做横向裁切
                    (caret_clip_boundary - self.to_document_rect.x)
                        .clamp(0.0, self.to_document_rect.w)
                };
                let frame_w = reveal_from_caret;
                let frame_h = self.to_document_rect.h;
                let frame_source_rect = SourceRect {
                    x: self.source_rect.x,
                    y: self.source_rect.y,
                    w: (self.source_rect.w * (frame_w / self.to_document_rect.w.max(1.0)))
                        .clamp(0.0, self.source_rect.w),
                    h: self.source_rect.h,
                };
                AnimatedSliceFrame {
                    x: self.to_document_rect.x,
                    y: self.to_document_rect.y,
                    w: frame_w,
                    h: frame_h,
                    opacity: 1.0,
                    source_rect: frame_source_rect,
                    snapshot_id: self.snapshot_id,
                }
            }
            AnimatedSliceKind::DeleteConceal => {
                // Issue #722 评论 5748596920 问题2: 跨软换行裁切按行判断。
                // Issue #722 评论 5749164244 问题1: 用 Option<usize> 判断行身份，
                // y fallback 用半开区间 line_top <= caret_y < line_bottom。
                let frame_h = self.from_document_rect.h;
                let same_line = match (caret_visual_line_id, self.visual_line_id) {
                    (Some(c_id), Some(s_id)) => c_id == s_id,
                    _ => {
                        let line_top = self.from_document_rect.y;
                        let line_bottom = self.from_document_rect.y + self.from_document_rect.h;
                        caret_clip_y >= line_top && caret_clip_y < line_bottom
                    }
                };
                let caret_above = match (caret_visual_line_id, self.visual_line_id) {
                    (Some(c_id), Some(s_id)) => c_id < s_id,
                    _ => caret_clip_y < self.from_document_rect.y,
                };
                let (frame_w, frame_x, src_x) = if self.conceal_to_left_edge {
                    // Backspace：保留左段。
                    let conceal_from_caret = if caret_above {
                        // caret 在该行上方，已经吞完该行
                        0.0
                    } else if !same_line && !caret_above {
                        // caret 在该行下方，还没吞到该行
                        self.from_document_rect.w
                    } else {
                        // 同一行：裁切宽度 = caret 边界 - 文档起点
                        (caret_clip_boundary - self.from_document_rect.x)
                            .clamp(0.0, self.from_document_rect.w)
                    };
                    (
                        conceal_from_caret,
                        self.from_document_rect.x,
                        self.source_rect.x,
                    )
                } else {
                    // 前向 Delete：保留右段。
                    // Issue #722 评论 5749164244 问题2: 修正宽度方向。
                    // 语义：逻辑 caret 固定在左边，视觉吞字边界从被删内容右端向 caret 移动。
                    // frame_w = full_w * visible（visible 是当前剩余可见比例），
                    // frame_x = from_document_rect.x（左端固定），
                    // source_rect.x 保持原起点（self.source_rect.x）。
                    // visible: 1→0 是"完整文字 → 右边界往左收到光标 → 完全吞掉"。
                    // caret_above/!same_line 分支保持原逻辑（caret 在上方=还没吞到该行→满宽；
                    // caret 在下方=已吞完→0）。
                    if caret_above {
                        // caret 在该行上方，还没吞到该行 → 满宽
                        (
                            self.from_document_rect.w,
                            self.from_document_rect.x,
                            self.source_rect.x,
                        )
                    } else if !same_line && !caret_above {
                        // caret 在该行下方，已经吞完该行 → 0
                        (0.0, self.from_document_rect.x, self.source_rect.x)
                    } else {
                        // 同一行：frame_w = full_w * visible，
                        // 左端固定，source_rect.x 保持原起点。
                        let full_w = self.from_document_rect.w;
                        let frame_w = full_w * visible.clamp(0.0, 1.0);
                        (frame_w, self.from_document_rect.x, self.source_rect.x)
                    }
                };
                let frame_source_rect = SourceRect {
                    x: src_x,
                    y: self.source_rect.y,
                    w: (self.source_rect.w * (frame_w / self.from_document_rect.w.max(1.0)))
                        .clamp(0.0, self.source_rect.w),
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
            AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade => {
                // Reflow 不消费 caret 边界，回退到纯几何插值。
                let _ = (caret_clip_boundary, caret_clip_y, caret_visual_line_id);
                self.compute_frame(visible)
            }
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
