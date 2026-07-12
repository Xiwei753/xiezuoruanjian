use super::layout_snapshot::{LineSnapshotId, SourceRect, ShapingIdentity};
use super::transaction_key::VisualTransactionKey;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AnimatedSliceKind {
    InsertFadeIn,
    DeleteFadeOut,
    ReflowMove,
    ReflowCrossFade,
}

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
