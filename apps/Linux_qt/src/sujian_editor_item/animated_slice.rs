use super::layout_snapshot::{LineSnapshotId, SourceRect, ShapingIdentity};
use super::transaction_key::VisualTransactionKey;

/// 动画切片类型，决定视觉语义和插值行为。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AnimatedSliceKind {
    /// 新快照视觉从光标附近进入目标位置（透明→不透明）。
    InsertFadeIn,
    /// 旧快照视觉向删除后的光标位置收缩并消失（不透明→透明，缩小 0.7）。
    DeleteFadeOut,
    /// old/new shaping identity 相同，复用旧视觉资源做几何移动。
    ReflowMove,
    /// shaping 发生变化，旧视觉淡出、新视觉淡入；同一逻辑对象由成对 slice 表达。
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
    pub fn insert_fade_in(
        key: VisualTransactionKey,
        snapshot_id: LineSnapshotId,
        source_rect: SourceRect,
        cursor_x: f64,
        cursor_y: f64,
        byte_start: usize,
        byte_end: usize,
        shaping_identity: Option<ShapingIdentity>,
    ) -> Self {
        let dest = source_rect.clone();
        Self {
            key,
            kind: AnimatedSliceKind::InsertFadeIn,
            snapshot_id,
            source_rect: source_rect.clone(),
            from_document_rect: SourceRect {
                x: cursor_x,
                y: cursor_y,
                w: source_rect.w,
                h: source_rect.h,
            },
            to_document_rect: dest,
            opacity_from: 0.0,
            opacity_to: 1.0,
            scale_from: 1.0,
            scale_to: 1.0,
            byte_start,
            byte_end,
            shaping_identity,
        }
    }

    pub fn delete_fade_out(
        key: VisualTransactionKey,
        snapshot_id: LineSnapshotId,
        source_rect: SourceRect,
        cursor_x: f64,
        cursor_y: f64,
        byte_start: usize,
        byte_end: usize,
        shaping_identity: Option<ShapingIdentity>,
    ) -> Self {
        Self {
            key,
            kind: AnimatedSliceKind::DeleteFadeOut,
            snapshot_id,
            source_rect: source_rect.clone(),
            from_document_rect: source_rect.clone(),
            to_document_rect: SourceRect {
                x: cursor_x,
                y: cursor_y,
                w: source_rect.w * 0.7,
                h: source_rect.h * 0.7,
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

    pub fn reflow_move(
        key: VisualTransactionKey,
        old_snapshot_id: LineSnapshotId,
        old_source_rect: SourceRect,
        new_snapshot_id: LineSnapshotId,
        new_dest_rect: SourceRect,
        byte_start: usize,
        byte_end: usize,
        shaping_identity: Option<ShapingIdentity>,
    ) -> Self {
        Self {
            key,
            kind: AnimatedSliceKind::ReflowMove,
            snapshot_id: old_snapshot_id,
            source_rect: old_source_rect.clone(),
            from_document_rect: old_source_rect,
            to_document_rect: new_dest_rect,
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
        dest_rect: SourceRect,
        byte_start: usize,
        byte_end: usize,
    ) -> Self {
        Self {
            key,
            kind: AnimatedSliceKind::ReflowCrossFade,
            snapshot_id,
            source_rect: source_rect.clone(),
            from_document_rect: source_rect.clone(),
            to_document_rect: dest_rect,
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
        dest_rect: SourceRect,
        byte_start: usize,
        byte_end: usize,
    ) -> Self {
        Self {
            key,
            kind: AnimatedSliceKind::ReflowCrossFade,
            snapshot_id,
            source_rect: source_rect.clone(),
            from_document_rect: source_rect.clone(),
            to_document_rect: dest_rect,
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
    /// rebase 后 slice 从当前视觉状态开始，而不是从原始逻辑起点重新播放。
    pub fn rebase_from(&mut self, current_x: f64, current_y: f64, current_opacity: f64) {
        self.from_document_rect.x = current_x;
        self.from_document_rect.y = current_y;
        self.opacity_from = current_opacity;
        self.scale_from = 1.0;
    }

    /// 纯插值计算：根据 progress 在 from/to 之间插值，不得查询布局或修改事务。
    pub fn compute_frame(&self, progress: f64) -> AnimatedSliceFrame {
        match self.kind {
            AnimatedSliceKind::InsertFadeIn => {
                let eased = 1.0 - (1.0 - progress).powi(3);
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
                let fade_out = 1.0 - progress;
                let x = self.from_document_rect.x + (self.to_document_rect.x - self.from_document_rect.x) * progress;
                let y = self.from_document_rect.y + (self.to_document_rect.y - self.from_document_rect.y) * progress;
                let scale = 1.0 - 0.3 * progress;
                AnimatedSliceFrame {
                    x,
                    y,
                    w: self.from_document_rect.w * scale,
                    h: self.from_document_rect.h * scale,
                    opacity: fade_out,
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
                if self.opacity_to < self.opacity_from {
                    let fade_out = 1.0 - progress;
                    AnimatedSliceFrame {
                        x: self.from_document_rect.x,
                        y: self.from_document_rect.y,
                        w: self.from_document_rect.w,
                        h: self.from_document_rect.h,
                        opacity: fade_out,
                        source_rect: self.source_rect.clone(),
                        snapshot_id: self.snapshot_id,
                    }
                } else {
                    let eased = 1.0 - (1.0 - progress).powi(2);
                    AnimatedSliceFrame {
                        x: self.from_document_rect.x,
                        y: self.from_document_rect.y,
                        w: self.from_document_rect.w,
                        h: self.from_document_rect.h,
                        opacity: eased,
                        source_rect: self.source_rect.clone(),
                        snapshot_id: self.snapshot_id,
                    }
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
