use writer_core::editor::CursorRect;
use super::animation_mode::AnimationMode;
use super::shaped_visual_run::{ShapedVisualRun, ReflowVisualSnapshot};

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
pub(crate) struct GlyphAnimationPayload {
    pub snapshot: ShapedVisualRun,
    pub old_cursor_rect: Option<CursorRect>,
    pub new_cursor_rect: Option<CursorRect>,
}

#[derive(Clone, Debug)]
pub(crate) struct ClusterAnimationPayload {
    pub snapshots: Vec<ShapedVisualRun>,
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
    pub insert_shaped_runs: Vec<ShapedVisualRun>,
    pub reflow_snapshot: ReflowVisualSnapshot,
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
        snapshot: ShapedVisualRun,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
        animation_mode: AnimationMode,
    ) -> Self {
        VisualPayload::GlyphPayload {
            payload: GlyphAnimationPayload {
                snapshot,
                old_cursor_rect,
                new_cursor_rect,
            },
            animation_mode,
        }
    }

    pub fn from_cluster_payload(
        snapshots: Vec<ShapedVisualRun>,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
        animation_mode: AnimationMode,
    ) -> Self {
        VisualPayload::ClusterPayload {
            payload: ClusterAnimationPayload {
                snapshots,
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
        insert_shaped_runs: Vec<ShapedVisualRun>,
        reflow_snapshot: ReflowVisualSnapshot,
        inserted_range: Option<(usize, usize)>,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
        animation_mode: AnimationMode,
    ) -> Self {
        VisualPayload::LineReflowPayload {
            payload: LineReflowAnimationPayload {
                insert_shaped_runs,
                reflow_snapshot,
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
            VisualPayload::ClusterPayload { payload, .. } => payload.snapshots.len(),
            VisualPayload::RunPayload { payload, .. } => payload.shaped_run.glyphs.len(),
            VisualPayload::LineReflowPayload { payload, .. } => {
                payload.insert_shaped_runs.iter().map(|r| r.glyphs.len()).sum::<usize>()
                    + payload.reflow_snapshot.new_shaped_runs.iter().map(|r| r.glyphs.len()).sum::<usize>()
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
        false
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
                payload.reflow_snapshot.new_shaped_runs.iter()
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
                runs.push(&payload.snapshot);
            }
            VisualPayload::ClusterPayload { payload, .. } => {
                for s in &payload.snapshots {
                    runs.push(s);
                }
            }
            VisualPayload::RunPayload { payload, .. } => {
                runs.push(&payload.shaped_run);
                if let Some(ref reflow_snapshot) = payload.reflow_snapshot {
                    for r in &reflow_snapshot.new_shaped_runs {
                        runs.push(r);
                    }
                }
            }
            VisualPayload::LineReflowPayload { payload, .. } => {
                for r in &payload.reflow_snapshot.new_shaped_runs {
                    runs.push(r);
                }
                for r in &payload.insert_shaped_runs {
                    runs.push(r);
                }
            }
            VisualPayload::CursorTransition { .. } => {}
        }
        runs
    }
}
