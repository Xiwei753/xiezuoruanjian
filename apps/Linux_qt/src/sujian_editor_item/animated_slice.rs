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
    /// 收进方向由 `conceal_from_left` 决定。
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
/// - `conceal_from_left`：仅对 `DeleteConceal` 有效。true 表示从左往右收（保留左段，
///   Backspace 场景——光标在文字右侧，文字向左消失）；false 表示从右往左收（保留右段，
///   Delete 键场景——光标在文字左侧，文字向右消失）。
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
    pub conceal_from_left: bool,
    /// Issue #690 评论 5675007226 步骤 3: 视觉单元的动画起始比例。
    ///
    /// InsertReveal：当前已吐出来的比例（0.0 = 未显示，1.0 = 完全显示）。
    /// DeleteConceal：当前还剩多少比例（1.0 = 完全可见，0.0 = 完全消失）。
    /// ReflowMove / ReflowCrossFade：不使用此字段（保持 0.0）。
    ///
    /// 快速连续输入时，旧 slice 被 rebase 后从 `start_fraction` 继续播放，
    /// 不再从 0 重新开始（避免文字"吐到一半被重启"）。
    pub start_fraction: f64,
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
            conceal_from_left: false,
            start_fraction: 0.0,
        }
    }

    /// 创建 Delete 吞字切片。
    ///
    /// 文字始终在 `from_document_rect` 位置，动画进度控制可见纹理宽度从 100% → 0%。
    /// `conceal_from_left` 决定收进方向：true 保留左段（Backspace），false 保留右段（Delete 键）。
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
        conceal_from_left: bool,
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
            conceal_from_left,
            start_fraction: 0.0,
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
            conceal_from_left: false,
            start_fraction: 0.0,
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
            conceal_from_left: false,
            start_fraction: 0.0,
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
            conceal_from_left: false,
            start_fraction: 0.0,
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
    /// Issue #690 评论 5675007226 步骤 3: `visible` 已经过 `current_visible_fraction`
    /// 由单元生命周期 + 单元视觉窗口（[`PreparedVisualUnit::start_fraction`,
    /// `PreparedVisualUnit::target_fraction`]）映射得到，并施加了同一条 ease-out quadratic
    /// 曲线。这里只做几何与透明度的线性插值，不再重复施加 easing 或 `start_fraction`，
    /// 避免与协调光标重复缓动。
    pub fn compute_frame(&self, visible: f64) -> AnimatedSliceFrame {
        let visible = visible.clamp(0.0, 1.0);
        match self.kind {
            AnimatedSliceKind::InsertReveal => {
                let frame_w = self.to_document_rect.w * visible;
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
                    w: frame_w,
                    h: frame_h,
                    opacity: 1.0,
                    source_rect: frame_source_rect,
                    snapshot_id: self.snapshot_id,
                }
            }
            AnimatedSliceKind::DeleteConceal => {
                let frame_w = self.from_document_rect.w * visible;
                let frame_h = self.from_document_rect.h;
                let (frame_x, src_x) = if self.conceal_from_left {
                    (self.from_document_rect.x, self.source_rect.x)
                } else {
                    (
                        self.from_document_rect.x + self.from_document_rect.w * (1.0 - visible),
                        self.source_rect.x + self.source_rect.w * (1.0 - visible),
                    )
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

    /// 把单元生命周期进度（`progress`，0..1）映射成"最终可见比例"（0..1）。
    ///
    /// Issue #690 评论 5675007226 步骤 2+3: 与协调光标共用同一条 ease-out quadratic；
    /// `start_fraction` 为单元在窗口内的起点（Reveal 当前已吐出比例 / Conceal 当前还剩比例），
    /// `DeleteConceal` 终点为 0、其余为 1，正好对应 [`PreparedVisualUnit::target_fraction`]。
    pub fn current_visible_fraction(&self, progress: f64) -> f64 {
        let eased = AnimatedSlice::ease_out_quad(progress);
        match self.kind {
            AnimatedSliceKind::InsertReveal => {
                self.start_fraction + (1.0 - self.start_fraction) * eased
            }
            AnimatedSliceKind::DeleteConceal => self.start_fraction * (1.0 - eased),
            AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade => eased,
        }
        .clamp(0.0, 1.0)
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
