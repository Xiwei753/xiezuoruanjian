use super::animation_mode::AnimationMode;
use super::line_snapshot::LineSnapshotId;
use super::texture_cache::TexturePhase;
use super::transaction_key::VisualTransactionKey;
use super::shaped_visual_run::ShapedVisualRun;

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
    pub line_snapshot_id: Option<LineSnapshotId>,
    pub source_x: f64,
    pub source_y: f64,
    pub source_w: f64,
    pub source_h: f64,
    pub target_x: f64,
    pub target_y: f64,
    pub target_w: f64,
    pub target_h: f64,
    pub baseline_in_quad: f64,
    pub animation_mode: AnimationMode,
    pub texture_phase: TexturePhase,
    pub run_identity: i32,
    pub byte_start: usize,
    pub byte_end: usize,
}

impl AnimatedSlice {
    pub fn insert_fade_in(
        key: VisualTransactionKey,
        run: &ShapedVisualRun,
        old_cursor_x: f64,
        old_cursor_y: f64,
        animation_mode: AnimationMode,
        line_snapshot_id: Option<LineSnapshotId>,
    ) -> Self {
        Self {
            key,
            kind: AnimatedSliceKind::InsertFadeIn,
            line_snapshot_id,
            source_x: old_cursor_x,
            source_y: old_cursor_y,
            source_w: run.visual_w,
            source_h: run.visual_h,
            target_x: run.visual_x,
            target_y: run.visual_y,
            target_w: run.visual_w,
            target_h: run.visual_h,
            baseline_in_quad: run.baseline_y - run.visual_y,
            animation_mode,
            texture_phase: TexturePhase::Insert,
            run_identity: run.qglyphrun_index,
            byte_start: run.source_string_start,
            byte_end: run.source_string_end,
        }
    }

    pub fn delete_fade_out(
        key: VisualTransactionKey,
        run: &ShapedVisualRun,
        new_cursor_x: f64,
        new_cursor_y: f64,
        animation_mode: AnimationMode,
        line_snapshot_id: Option<LineSnapshotId>,
    ) -> Self {
        Self {
            key,
            kind: AnimatedSliceKind::DeleteFadeOut,
            line_snapshot_id,
            source_x: run.visual_x,
            source_y: run.visual_y,
            source_w: run.visual_w,
            source_h: run.visual_h,
            target_x: new_cursor_x,
            target_y: new_cursor_y,
            target_w: run.visual_w * 0.7,
            target_h: run.visual_h * 0.7,
            baseline_in_quad: (run.baseline_y - run.visual_y) * 0.7,
            animation_mode,
            texture_phase: TexturePhase::DeleteOld,
            run_identity: run.qglyphrun_index,
            byte_start: run.source_string_start,
            byte_end: run.source_string_end,
        }
    }

    pub fn reflow_move(
        key: VisualTransactionKey,
        old_run: &ShapedVisualRun,
        new_run: &ShapedVisualRun,
        animation_mode: AnimationMode,
        line_snapshot_id: Option<LineSnapshotId>,
    ) -> Self {
        Self {
            key,
            kind: AnimatedSliceKind::ReflowMove,
            line_snapshot_id,
            source_x: old_run.visual_x,
            source_y: old_run.visual_y,
            source_w: old_run.visual_w,
            source_h: old_run.visual_h,
            target_x: new_run.visual_x,
            target_y: new_run.visual_y,
            target_w: new_run.visual_w,
            target_h: new_run.visual_h,
            baseline_in_quad: new_run.baseline_y - new_run.visual_y,
            animation_mode,
            texture_phase: TexturePhase::NewReflow,
            run_identity: new_run.qglyphrun_index,
            byte_start: new_run.source_string_start,
            byte_end: new_run.source_string_end,
        }
    }

    pub fn reflow_crossfade(
        key: VisualTransactionKey,
        old_run: &ShapedVisualRun,
        new_run: &ShapedVisualRun,
        animation_mode: AnimationMode,
        phase: TexturePhase,
        line_snapshot_id: Option<LineSnapshotId>,
    ) -> Self {
        let (src_x, src_y, src_w, src_h, bl) = if phase == TexturePhase::OldReflow {
            (old_run.visual_x, old_run.visual_y, old_run.visual_w, old_run.visual_h, old_run.baseline_y - old_run.visual_y)
        } else {
            (new_run.visual_x, new_run.visual_y, new_run.visual_w, new_run.visual_h, new_run.baseline_y - new_run.visual_y)
        };
        Self {
            key,
            kind: AnimatedSliceKind::ReflowCrossFade,
            line_snapshot_id,
            source_x: src_x,
            source_y: src_y,
            source_w: src_w,
            source_h: src_h,
            target_x: src_x,
            target_y: src_y,
            target_w: src_w,
            target_h: src_h,
            baseline_in_quad: bl,
            animation_mode,
            texture_phase: phase,
            run_identity: if phase == TexturePhase::OldReflow {
                old_run.qglyphrun_index
            } else {
                new_run.qglyphrun_index
            },
            byte_start: if phase == TexturePhase::OldReflow {
                old_run.source_string_start
            } else {
                new_run.source_string_start
            },
            byte_end: if phase == TexturePhase::OldReflow {
                old_run.source_string_end
            } else {
                new_run.source_string_end
            },
        }
    }

    pub fn compute_frame(&self, progress: f64) -> AnimatedSliceFrame {
        match self.kind {
            AnimatedSliceKind::InsertFadeIn => {
                let eased = 1.0 - (1.0 - progress).powi(3);
                let x = self.source_x + (self.target_x - self.source_x) * eased;
                let y = self.source_y + (self.target_y - self.source_y) * eased;
                AnimatedSliceFrame {
                    x,
                    y,
                    w: self.target_w,
                    h: self.target_h,
                    opacity: eased,
                    baseline_in_quad: self.baseline_in_quad + (self.target_y - y),
                }
            }
            AnimatedSliceKind::DeleteFadeOut => {
                let fade_out = 1.0 - progress;
                let x = self.source_x + (self.target_x - self.source_x) * progress;
                let y = self.source_y + (self.target_y - self.source_y) * progress;
                let scale = 1.0 - 0.3 * progress;
                AnimatedSliceFrame {
                    x,
                    y,
                    w: self.target_w / 0.7 * scale,
                    h: self.target_h / 0.7 * scale,
                    opacity: fade_out,
                    baseline_in_quad: self.baseline_in_quad / 0.7 * scale,
                }
            }
            AnimatedSliceKind::ReflowMove => {
                let eased = 1.0 - (1.0 - progress).powi(2);
                let x = self.source_x + (self.target_x - self.source_x) * eased;
                let y = self.source_y + (self.target_y - self.source_y) * eased;
                AnimatedSliceFrame {
                    x,
                    y,
                    w: self.target_w,
                    h: self.target_h,
                    opacity: 1.0,
                    baseline_in_quad: self.baseline_in_quad + (self.target_y - y),
                }
            }
            AnimatedSliceKind::ReflowCrossFade => {
                if self.texture_phase == TexturePhase::OldReflow {
                    let fade_out = 1.0 - progress;
                    AnimatedSliceFrame {
                        x: self.source_x,
                        y: self.source_y,
                        w: self.source_w,
                        h: self.source_h,
                        opacity: fade_out,
                        baseline_in_quad: self.baseline_in_quad,
                    }
                } else {
                    let eased = 1.0 - (1.0 - progress).powi(2);
                    AnimatedSliceFrame {
                        x: self.source_x,
                        y: self.source_y,
                        w: self.source_w,
                        h: self.source_h,
                        opacity: eased,
                        baseline_in_quad: self.baseline_in_quad,
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
    pub baseline_in_quad: f64,
}
