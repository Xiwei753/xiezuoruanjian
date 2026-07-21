use super::layout_snapshot::{LineSnapshotId, SourceRect, ShapingIdentity};
use super::transaction_key::VisualTransactionKey;

// ── 动画切片模块 ──
//
// 与 Core `AnimatedSliceRole` 的映射关系：
// - InsertFadeIn  ↔ Core Insert（新文字从光标位置淡入）
// - DeleteFadeOut ↔ Core Delete（旧文字向光标位置收缩淡出）
// - ReflowMove    ↔ Core Move（shaping 不变，几何位移）
// - ReflowCrossFade ↔ Core CrossfadeOld/CrossfadeNew（shaping 变化，成对淡入淡出）
//
// 线程安全：AnimatedSlice 仅在 Qt GUI 线程中使用，
// 不跨线程传递——动画帧计算和渲染都在 GUI 线程完成。

/// 动画切片类型，决定视觉语义和插值行为。
///
/// 与 Core `AnimatedSliceRole` 一一对应（见模块文档映射表）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AnimatedSliceKind {
    /// 新快照视觉从光标附近进入目标位置（透明→不透明）。
    /// 对应 Core AnimatedSliceRole::Insert。
    InsertFadeIn,
    /// 旧快照视觉向删除后的光标位置收缩并消失（不透明→透明，缩小 0.7）。
    /// 0.7 缩放因子为产品定义的视觉反馈强度，使删除动画有明显的收缩感。
    /// 对应 Core AnimatedSliceRole::Delete。
    DeleteFadeOut,
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
#[derive(Clone, Debug)]
pub(crate) struct AnimatedSlice {
    pub key: VisualTransactionKey,
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
}

impl AnimatedSlice {
    /// 创建 Insert 淡入切片。
    ///
    /// `cursor_x`/`cursor_y` 为文档坐标（不含滚动偏移），作为动画起始位置。
    /// 文字从光标位置淡入移动到 `to_document_rect`。
    pub fn insert_fade_in(
        key: VisualTransactionKey,
        snapshot_id: LineSnapshotId,
        source_rect: SourceRect,
        to_document_rect: SourceRect,
        cursor_x: f64,
        cursor_y: f64,
        byte_start: usize,
        byte_end: usize,
        shaping_identity: Option<ShapingIdentity>,
    ) -> Self {
        Self {
            key,
            kind: AnimatedSliceKind::InsertFadeIn,
            snapshot_id,
            source_rect,
            from_document_rect: SourceRect {
                x: cursor_x,
                y: cursor_y,
                w: to_document_rect.w,
                h: to_document_rect.h,
            },
            to_document_rect,
            opacity_from: 0.0,
            opacity_to: 1.0,
            scale_from: 1.0,
            scale_to: 1.0,
            byte_start,
            byte_end,
            shaping_identity,
        }
    }

    /// 创建 Delete 淡出切片。
    ///
    /// `cursor_x`/`cursor_y` 为文档坐标（不含滚动偏移），作为动画终止位置。
    /// 文字从 `from_document_rect` 收缩到光标位置并淡出。
    pub fn delete_fade_out(
        key: VisualTransactionKey,
        snapshot_id: LineSnapshotId,
        source_rect: SourceRect,
        from_document_rect: SourceRect,
        cursor_x: f64,
        cursor_y: f64,
        byte_start: usize,
        byte_end: usize,
        shaping_identity: Option<ShapingIdentity>,
    ) -> Self {
        let shrink_w = from_document_rect.w * 0.7;
        let shrink_h = from_document_rect.h * 0.7;
        Self {
            key,
            kind: AnimatedSliceKind::DeleteFadeOut,
            snapshot_id,
            source_rect,
            from_document_rect,
            to_document_rect: SourceRect {
                x: cursor_x,
                y: cursor_y,
                w: shrink_w,
                h: shrink_h,
            },
            opacity_from: 1.0,
            opacity_to: 0.0,
            scale_from: 1.0,
            scale_to: 0.7,
            byte_start,
            byte_end,
            shaping_identity,
        }
    }

    /// 创建 ReflowMove 切片——shaping 不变，复用旧快照纹理做几何移动。
    ///
    /// `_new_snapshot_id`/`_new_source_rect` 当前未使用（Move 复用旧纹理），
    /// 保留参数签名与 ReflowCrossFade 对称，未来可能用于纹理缓存优化。
    pub fn reflow_move(
        key: VisualTransactionKey,
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
            key,
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
        }
    }

    pub fn reflow_crossfade_old(
        key: VisualTransactionKey,
        snapshot_id: LineSnapshotId,
        source_rect: SourceRect,
        from_document_rect: SourceRect,
        to_document_rect: SourceRect,
        byte_start: usize,
        byte_end: usize,
    ) -> Self {
        Self {
            key,
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
        }
    }

    pub fn reflow_crossfade_new(
        key: VisualTransactionKey,
        snapshot_id: LineSnapshotId,
        source_rect: SourceRect,
        from_document_rect: SourceRect,
        to_document_rect: SourceRect,
        byte_start: usize,
        byte_end: usize,
    ) -> Self {
        Self {
            key,
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
        }
    }

    pub fn is_insert(&self) -> bool {
        matches!(self.kind, AnimatedSliceKind::InsertFadeIn)
    }

    pub fn is_delete(&self) -> bool {
        matches!(self.kind, AnimatedSliceKind::DeleteFadeOut)
    }

    /// 接收当前已显示视觉帧的位置和透明度，用于连续事务无跳变衔接。
    ///
    /// 触发条件：新事务开始时旧事务仍在播放中，rebase 使动画从当前视觉状态
    /// 平滑过渡到新目标，而不是从原始逻辑起点重新播放（避免跳变）。
    pub fn rebase_from(&mut self, current_x: f64, current_y: f64, current_opacity: f64) {
        self.from_document_rect.x = current_x;
        self.from_document_rect.y = current_y;
        self.opacity_from = current_opacity;
        self.scale_from = 1.0;
    }

    /// 纯插值计算：根据 progress 在 from/to 之间插值，不得查询布局或修改事务。
    ///
    /// 缓动函数：`1.0 - (1.0 - progress).powi(2)` 即 ease-out quadratic，
    /// 选择原因：快速启动、缓慢结束，适合文字位移的视觉反馈——
    /// 用户感知到即时响应，同时末段减速避免突兀停止。
    pub fn compute_frame(&self, progress: f64) -> AnimatedSliceFrame {
        match self.kind {
            AnimatedSliceKind::InsertFadeIn => {
                let eased = 1.0 - (1.0 - progress).powi(2);
                let x = self.from_document_rect.x + (self.to_document_rect.x - self.from_document_rect.x) * eased;
                let y = self.from_document_rect.y + (self.to_document_rect.y - self.from_document_rect.y) * eased;
                AnimatedSliceFrame {
                    x,
                    y,
                    w: self.to_document_rect.w,
                    h: self.to_document_rect.h,
                    opacity: eased,
                    source_rect: self.source_rect.clone(),
                    snapshot_id: self.snapshot_id,
                }
            }
            AnimatedSliceKind::DeleteFadeOut => {
                let eased = 1.0 - (1.0 - progress).powi(2);
                let x = self.from_document_rect.x + (self.to_document_rect.x - self.from_document_rect.x) * eased;
                let y = self.from_document_rect.y + (self.to_document_rect.y - self.from_document_rect.y) * eased;
                let scale = self.scale_from + (self.scale_to - self.scale_from) * eased;
                let opacity = self.opacity_from + (self.opacity_to - self.opacity_from) * eased;
                AnimatedSliceFrame {
                    x,
                    y,
                    w: self.from_document_rect.w * scale,
                    h: self.from_document_rect.h * scale,
                    opacity,
                    source_rect: self.source_rect.clone(),
                    snapshot_id: self.snapshot_id,
                }
            }
            AnimatedSliceKind::ReflowMove => {
                let eased = 1.0 - (1.0 - progress).powi(2);
                let x = self.from_document_rect.x + (self.to_document_rect.x - self.from_document_rect.x) * eased;
                let y = self.from_document_rect.y + (self.to_document_rect.y - self.from_document_rect.y) * eased;
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
                let eased = 1.0 - (1.0 - progress).powi(2);
                let x = self.from_document_rect.x + (self.to_document_rect.x - self.from_document_rect.x) * eased;
                let y = self.from_document_rect.y + (self.to_document_rect.y - self.from_document_rect.y) * eased;
                let w = self.from_document_rect.w + (self.to_document_rect.w - self.from_document_rect.w) * eased;
                let h = self.from_document_rect.h + (self.to_document_rect.h - self.from_document_rect.h) * eased;
                let opacity = self.opacity_from + (self.opacity_to - self.opacity_from) * eased;
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
