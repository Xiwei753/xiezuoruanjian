use writer_core::editor::CursorRect;
use super::animation_mode::AnimationMode;
use super::shaped_visual_run::{ShapedVisualRun, ShapedGlyph, ShapedCluster, ReflowVisualSnapshot, RunMapping};
use super::transaction_queue::VisualOperationKind;
use super::texture_cache::{TexturePhase, TextureCacheKey};
use super::transaction_key::VisualTransactionKey;

#[derive(Clone, Debug)]
pub(crate) struct ShapedGlyphInfo {
    pub font_id: String,
    pub glyph_index: u32,
    pub glyph_position_x: f64,
    pub glyph_position_y: f64,
    pub string_index: usize,
    pub raw_font_family: String,
}

#[derive(Clone, Debug)]
pub(crate) struct GlyphBounds {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Clone, Debug)]
pub(crate) struct TextureRegion {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Clone, Debug)]
pub(crate) struct GlyphAnimationPayload {
    pub run_identity: i32,
    pub glyph_index_in_run: usize,
    pub glyph: ShapedGlyph,
    pub glyph_bounds: GlyphBounds,
    pub texture_region: TextureRegion,
    pub parent_run: ShapedVisualRun,
    pub old_cursor_rect: Option<CursorRect>,
    pub new_cursor_rect: Option<CursorRect>,
}

#[derive(Clone, Debug)]
pub(crate) struct ClusterAnimationPayload {
    pub run_identity: i32,
    pub cluster: ShapedCluster,
    pub glyph_range: (usize, usize),
    pub string_range: (usize, usize),
    pub cluster_bounds: GlyphBounds,
    pub texture_region: TextureRegion,
    pub parent_run: ShapedVisualRun,
    pub old_cursor_rect: Option<CursorRect>,
    pub new_cursor_rect: Option<CursorRect>,
}

#[derive(Clone, Debug)]
pub(crate) struct RunAnimationPayload {
    pub shaped_run: ShapedVisualRun,
    pub reflow_snapshot: Option<ReflowVisualSnapshot>,
    pub old_cursor_rect: Option<CursorRect>,
    pub new_cursor_rect: Option<CursorRect>,
}

#[derive(Clone, Debug)]
pub(crate) struct LineReflowAnimationPayload {
    pub old_runs: Vec<ShapedVisualRun>,
    pub new_runs: Vec<ShapedVisualRun>,
    pub run_mapping: Vec<RunMapping>,
    pub insert_runs: Vec<ShapedVisualRun>,
    pub inserted_range: Option<(usize, usize)>,
    pub old_cursor_rect: Option<CursorRect>,
    pub new_cursor_rect: Option<CursorRect>,
}

#[derive(Clone, Debug)]
pub(crate) enum VisualPayload {
    CursorTransition {
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
    },
    GlyphPayload {
        payload: GlyphAnimationPayload,
        animation_mode: AnimationMode,
    },
    ClusterPayload {
        payload: ClusterAnimationPayload,
        animation_mode: AnimationMode,
    },
    RunPayload {
        payload: RunAnimationPayload,
        animation_mode: AnimationMode,
    },
    LineReflowPayload {
        payload: LineReflowAnimationPayload,
        animation_mode: AnimationMode,
    },
}

impl VisualPayload {
    pub fn from_glyph_payload(
        run_identity: i32,
        glyph_index_in_run: usize,
        glyph: ShapedGlyph,
        glyph_bounds: GlyphBounds,
        texture_region: TextureRegion,
        parent_run: ShapedVisualRun,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
        animation_mode: AnimationMode,
    ) -> Self {
        VisualPayload::GlyphPayload {
            payload: GlyphAnimationPayload {
                run_identity,
                glyph_index_in_run,
                glyph,
                glyph_bounds,
                texture_region,
                parent_run,
                old_cursor_rect,
                new_cursor_rect,
            },
            animation_mode,
        }
    }

    pub fn from_cluster_payload(
        run_identity: i32,
        cluster: ShapedCluster,
        glyph_range: (usize, usize),
        string_range: (usize, usize),
        cluster_bounds: GlyphBounds,
        texture_region: TextureRegion,
        parent_run: ShapedVisualRun,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
        animation_mode: AnimationMode,
    ) -> Self {
        VisualPayload::ClusterPayload {
            payload: ClusterAnimationPayload {
                run_identity,
                cluster,
                glyph_range,
                string_range,
                cluster_bounds,
                texture_region,
                parent_run,
                old_cursor_rect,
                new_cursor_rect,
            },
            animation_mode,
        }
    }

    pub fn from_run_payload(
        shaped_run: ShapedVisualRun,
        reflow_snapshot: Option<ReflowVisualSnapshot>,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
        animation_mode: AnimationMode,
    ) -> Self {
        VisualPayload::RunPayload {
            payload: RunAnimationPayload {
                shaped_run,
                reflow_snapshot,
                old_cursor_rect,
                new_cursor_rect,
            },
            animation_mode,
        }
    }

    pub fn from_line_reflow_payload(
        old_runs: Vec<ShapedVisualRun>,
        new_runs: Vec<ShapedVisualRun>,
        run_mapping: Vec<RunMapping>,
        insert_runs: Vec<ShapedVisualRun>,
        inserted_range: Option<(usize, usize)>,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
        animation_mode: AnimationMode,
    ) -> Self {
        VisualPayload::LineReflowPayload {
            payload: LineReflowAnimationPayload {
                old_runs,
                new_runs,
                run_mapping,
                insert_runs,
                inserted_range,
                old_cursor_rect,
                new_cursor_rect,
            },
            animation_mode,
        }
    }

    pub fn cursor_rects(&self) -> (Option<&CursorRect>, Option<&CursorRect>) {
        match self {
            VisualPayload::CursorTransition { old_cursor_rect, new_cursor_rect } => (old_cursor_rect.as_ref(), new_cursor_rect.as_ref()),
            VisualPayload::GlyphPayload { payload, .. } => (payload.old_cursor_rect.as_ref(), payload.new_cursor_rect.as_ref()),
            VisualPayload::ClusterPayload { payload, .. } => (payload.old_cursor_rect.as_ref(), payload.new_cursor_rect.as_ref()),
            VisualPayload::RunPayload { payload, .. } => (payload.old_cursor_rect.as_ref(), payload.new_cursor_rect.as_ref()),
            VisualPayload::LineReflowPayload { payload, .. } => (payload.old_cursor_rect.as_ref(), payload.new_cursor_rect.as_ref()),
        }
    }

    pub fn total_glyph_count(&self) -> usize {
        match self {
            VisualPayload::CursorTransition { .. } => 0,
            VisualPayload::GlyphPayload { .. } => 1,
            VisualPayload::ClusterPayload { payload, .. } => payload.cluster.glyph_end - payload.cluster.glyph_start,
            VisualPayload::RunPayload { payload, .. } => payload.shaped_run.glyphs.len(),
            VisualPayload::LineReflowPayload { payload, .. } => {
                payload.insert_runs.iter().map(|r| r.glyphs.len()).sum::<usize>()
                    + payload.new_runs.iter().map(|r| r.glyphs.len()).sum::<usize>()
            }
        }
    }

    pub fn animation_mode(&self) -> AnimationMode {
        match self {
            VisualPayload::CursorTransition { .. } => AnimationMode::GlyphAnimation,
            VisualPayload::GlyphPayload { animation_mode, .. } => *animation_mode,
            VisualPayload::ClusterPayload { animation_mode, .. } => *animation_mode,
            VisualPayload::RunPayload { animation_mode, .. } => *animation_mode,
            VisualPayload::LineReflowPayload { animation_mode, .. } => *animation_mode,
        }
    }

    pub fn is_insert(&self) -> bool {
        matches!(self, VisualPayload::GlyphPayload { .. } | VisualPayload::ClusterPayload { .. } | VisualPayload::RunPayload { .. } | VisualPayload::LineReflowPayload { .. })
    }

    pub fn is_delete(&self) -> bool {
        matches!(self, VisualPayload::GlyphPayload { .. } | VisualPayload::ClusterPayload { .. } | VisualPayload::RunPayload { .. } | VisualPayload::LineReflowPayload { .. })
    }

    pub fn inserted_range(&self) -> Option<(usize, usize)> {
        match self {
            VisualPayload::LineReflowPayload { payload, .. } => payload.inserted_range,
            _ => None,
        }
    }

    pub fn reflow_byte_ranges(&self) -> Vec<(usize, usize)> {
        match self {
            VisualPayload::LineReflowPayload { payload, .. } => {
                payload.new_runs.iter()
                    .map(|r| r.string_range())
                    .collect()
            }
            VisualPayload::RunPayload { payload, .. } => {
                payload.reflow_snapshot.as_ref()
                    .map(|rs| rs.new_shaped_runs.iter().map(|r| r.string_range()).collect())
                    .unwrap_or_default()
            }
            _ => Vec::new(),
        }
    }

    pub fn shaped_runs_for_texture(&self) -> Vec<&ShapedVisualRun> {
        let mut runs = Vec::new();
        match self {
            VisualPayload::GlyphPayload { payload, .. } => {
                runs.push(&payload.parent_run);
            }
            VisualPayload::ClusterPayload { payload, .. } => {
                runs.push(&payload.parent_run);
            }
            VisualPayload::RunPayload { payload, .. } => {
                runs.push(&payload.shaped_run);
                if let Some(ref reflow_snapshot) = payload.reflow_snapshot {
                    for r in &reflow_snapshot.new_shaped_runs {
                        runs.push(r);
                    }
                    for r in &reflow_snapshot.old_shaped_runs {
                        if r.raw_font_key != *reflow_snapshot.new_shaped_runs.first().map(|nr| &nr.raw_font_key).unwrap_or(&r.raw_font_key) {
                            runs.push(r);
                        }
                    }
                }
            }
            VisualPayload::LineReflowPayload { payload, .. } => {
                for r in &payload.new_runs {
                    runs.push(r);
                }
                for r in &payload.old_runs {
                    runs.push(r);
                }
                for r in &payload.insert_runs {
                    runs.push(r);
                }
            }
            VisualPayload::CursorTransition { .. } => {}
        }
        runs
    }

    pub fn texture_cache_keys(&self, transaction_key: VisualTransactionKey, operation_kind: VisualOperationKind) -> Vec<TextureCacheKey> {
        let mut keys = Vec::new();
        match self {
            VisualPayload::GlyphPayload { payload, .. } => {
                let phase = if operation_kind == VisualOperationKind::Delete {
                    TexturePhase::DeleteOld
                } else {
                    TexturePhase::Insert
                };
                keys.push(TextureCacheKey::new(transaction_key, phase, payload.run_identity));
            }
            VisualPayload::ClusterPayload { payload, .. } => {
                let phase = if operation_kind == VisualOperationKind::Delete {
                    TexturePhase::DeleteOld
                } else {
                    TexturePhase::Insert
                };
                keys.push(TextureCacheKey::new(transaction_key, phase, payload.run_identity));
            }
            VisualPayload::RunPayload { payload, .. } => {
                let insert_phase = if operation_kind == VisualOperationKind::Delete {
                    TexturePhase::DeleteOld
                } else {
                    TexturePhase::Insert
                };
                keys.push(TextureCacheKey::new(transaction_key, insert_phase, payload.shaped_run.qglyphrun_index));
                if let Some(ref reflow_snapshot) = payload.reflow_snapshot {
                    for (i, new_run) in reflow_snapshot.new_shaped_runs.iter().enumerate() {
                        keys.push(TextureCacheKey::new(transaction_key, TexturePhase::NewReflow, new_run.qglyphrun_index + (i as i32)));
                    }
                }
            }
            VisualPayload::LineReflowPayload { payload, .. } => {
                for (i, new_run) in payload.new_runs.iter().enumerate() {
                    keys.push(TextureCacheKey::new(transaction_key, TexturePhase::NewReflow, new_run.qglyphrun_index + (i as i32)));
                }
                for (i, old_run) in payload.old_runs.iter().enumerate() {
                    keys.push(TextureCacheKey::new(transaction_key, TexturePhase::OldReflow, old_run.qglyphrun_index + (i as i32)));
                }
                for (i, insert_run) in payload.insert_runs.iter().enumerate() {
                    keys.push(TextureCacheKey::new(transaction_key, TexturePhase::Insert, insert_run.qglyphrun_index + (i as i32)));
                }
            }
            VisualPayload::CursorTransition { .. } => {}
        }
        keys
    }
}
