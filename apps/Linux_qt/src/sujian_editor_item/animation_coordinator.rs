use std::time::Instant;

use writer_core::editor::{
    CursorRect, EditorAnimationKind, EditorVisualTransaction,
};

pub(crate) use super::transaction_key::VisualTransactionKey;
pub(crate) use super::animation_mode::AnimationMode;
pub(crate) use super::cursor_animation::{CursorAnimationPlan, CursorBlinkMode, CursorTransition};
pub(crate) use super::render_plan::{
    HiddenClipRect, StaticTextPlan, TextAnimationPlan, TextAnimationGlyphInfo,
    SelectionPreeditPlan, SelectionRange, PreeditRange,
    ImeUpdateKind, ImeUpdatePlan, RenderPlan,
};
pub(crate) use super::texture_cache::TextureCache;
use super::animated_slice::{AnimatedSlice, AnimatedSliceFrame, AnimatedSliceKind};
use super::text_visual_transaction::{
    PreparedTextVisualTransaction, PreparedTransactionQueue,
    TextVisualTransactionState, TextVisualOperationKind,
};
use super::static_line_patch::StaticLinePatch;
use super::layout_revision::LayoutRevision;
use super::layout_snapshot::{EditorLayoutSnapshot, LineSnapshotId, SourceRect};

pub(crate) struct LinuxEditorAnimationCoordinator {
    next_key_id: u64,
    pub(crate) prepared_queue: PreparedTransactionQueue,
    layout_revision: LayoutRevision,
}

impl LinuxEditorAnimationCoordinator {
    pub fn new() -> Self {
        Self {
            next_key_id: 1,
            prepared_queue: PreparedTransactionQueue::new(),
            layout_revision: LayoutRevision::initial(),
        }
    }

    pub(crate) fn alloc_key(&mut self) -> VisualTransactionKey {
        let id = self.next_key_id;
        self.next_key_id += 1;
        VisualTransactionKey::new(id, id)
    }

    pub fn process_transaction(
        &mut self,
        vt: &EditorVisualTransaction,
        typing_animation_enabled: bool,
        is_scrolling: bool,
        is_loading: bool,
        is_applying_format: bool,
        is_applying_settings: bool,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
        old_snapshot: &EditorLayoutSnapshot,
        new_snapshot: &EditorLayoutSnapshot,
    ) -> Option<VisualTransactionKey> {
        if !typing_animation_enabled || is_scrolling || is_loading || is_applying_format || is_applying_settings {
            return None;
        }

        let mode = AnimationMode::from_core(vt.animation_mode);
        if !mode.should_create_transaction() {
            return None;
        }

        let new_revision = LayoutRevision::next();

        match vt.kind {
            EditorAnimationKind::Insert => {
                if let Some((range_start, range_end)) = vt.inserted_range {
                    let conflicting = self.prepared_queue.find_conflicting_insert(range_start, range_end);
                    let rebase_frames: Vec<(usize, usize, f64, f64, f64)> = if let Some(old_key) = conflicting {
                        let mut frames = Vec::new();
                        if let Some(old_tx) = self.prepared_queue.active_transactions().iter().find(|t| t.key == old_key) {
                            let old_progress = old_tx.progress(Instant::now());
                            if old_progress > 0.0 && old_progress < 1.0 {
                                for old_slice in &old_tx.slices {
                                    let frame = old_slice.compute_frame(old_progress);
                                    frames.push((old_slice.byte_start, old_slice.byte_end, frame.x, frame.y, frame.opacity));
                                }
                            }
                        }
                        self.prepared_queue.complete(old_key);
                        frames
                    } else {
                        Vec::new()
                    };

                    let key = self.alloc_key();
                    let mut slices = Vec::new();
                    let mut static_patches = Vec::new();

                    let old_cx = old_cursor_rect.as_ref().map(|c| c.x).unwrap_or(0.0);
                    let old_cy = old_cursor_rect.as_ref().map(|c| c.top).unwrap_or(0.0);

                    for new_line in new_snapshot.lines_in_byte_range(range_start, range_end) {
                        if let Some(source_rect) = new_line.source_rect_for_byte_range(range_start, range_end) {
                            slices.push(AnimatedSlice::insert_fade_in(
                                key,
                                new_line.id,
                                source_rect.clone(),
                                old_cx,
                                old_cy,
                                range_start,
                                range_end,
                                new_line.clusters.first().map(|c| c.shaping_identity.clone()),
                            ));

                            static_patches.push(StaticLinePatch::insert_patch(
                                key,
                                new_line.id,
                                vec![source_rect],
                                range_start,
                                range_end,
                            ));
                        }
                    }

                    let reflow_start = range_end;
                    for new_line in &new_snapshot.line_snapshots {
                        if new_line.byte_end <= range_end {
                            continue;
                        }
                        if new_line.byte_start < range_end {
                            continue;
                        }

                        let old_line = old_snapshot.line_for_byte(new_line.byte_start.saturating_sub(range_end - range_start));
                        if let Some(ol) = old_line {
                            let old_sr = ol.source_rect_for_byte_range(ol.byte_start, ol.byte_end);
                            let new_sr = new_line.source_rect_for_byte_range(new_line.byte_start, new_line.byte_end);

                            match (old_sr, new_sr) {
                                (Some(old_src), Some(new_src)) => {
                                    let same_shaping = ol.clusters.iter()
                                        .zip(new_line.clusters.iter())
                                        .take_while(|(oc, nc)| oc.shaping_identity.is_same_shaping(&nc.shaping_identity))
                                        .count() == ol.clusters.len().min(new_line.clusters.len());

                                    if same_shaping {
                                        slices.push(AnimatedSlice::reflow_move(
                                            key,
                                            ol.id,
                                            old_src,
                                            new_line.id,
                                            new_src.clone(),
                                            new_line.byte_start,
                                            new_line.byte_end,
                                            ol.clusters.first().map(|c| c.shaping_identity.clone()),
                                        ));
                                    } else {
                                        slices.push(AnimatedSlice::reflow_crossfade_old(
                                            key,
                                            ol.id,
                                            old_src.clone(),
                                            old_src,
                                            new_line.byte_start,
                                            new_line.byte_end,
                                        ));
                                        slices.push(AnimatedSlice::reflow_crossfade_new(
                                            key,
                                            new_line.id,
                                            new_src.clone(),
                                            new_src.clone(),
                                            new_line.byte_start,
                                            new_line.byte_end,
                                        ));
                                    }

                                    static_patches.push(StaticLinePatch::reflow_patch(
                                        key,
                                        new_line.id,
                                        vec![new_src],
                                        new_line.byte_start,
                                        new_line.byte_end,
                                    ));
                                }
                                _ => {}
                            }
                        }
                    }

                    let cursor_transition = if old_cursor_rect.is_some() && new_cursor_rect.is_some() {
                        CursorTransition::Tween {
                            old_rect: old_cursor_rect.clone().unwrap(),
                            new_rect: new_cursor_rect.clone().unwrap(),
                            duration_ms: vt.duration_ms,
                        }
                    } else {
                        CursorTransition::Snap
                    };

                    for (bs, be, fx, fy, fo) in &rebase_frames {
                        if let Some(new_slice) = slices.iter_mut().find(|ns| ns.byte_start == *bs && ns.byte_end == *be) {
                            new_slice.rebase_from(*fx, *fy, *fo);
                        }
                    }

                    let prepared = PreparedTextVisualTransaction {
                        key,
                        state: TextVisualTransactionState::Pending,
                        operation_kind: TextVisualOperationKind::Insert,
                        animation_mode: mode,
                        duration_ms: vt.duration_ms,
                        start_time: Instant::now(),
                        old_revision: self.layout_revision,
                        new_revision,
                        slices,
                        static_patches,
                        cursor_transition,
                        old_cursor_rect,
                        new_cursor_rect,
                        cancel_reason: None,
                        texture_prepared: false,
                        first_render_frame: None,
                        rendering_started_at: None,
                        accumulated_paused_duration_ms: 0,
                        pause_start: None,
                        old_snapshot: Some(old_snapshot.clone()),
                        new_snapshot: Some(new_snapshot.clone()),
                    };

                    self.layout_revision = new_revision;
                    self.prepared_queue.enqueue(prepared);

                    return Some(key);
                }
            }
            EditorAnimationKind::Delete => {
                let key = self.alloc_key();
                let changes = writer_core::editor::diff_plain_text(&vt.old_text, &vt.new_text);
                let mut deleted_ranges: Vec<(usize, usize)> = Vec::new();
                for change in &changes {
                    if let writer_core::editor::EditorChange::Delete { index, text } = change {
                        let range_start = *index;
                        let range_end = range_start + text.len();
                        deleted_ranges.push((range_start, range_end));
                    }
                }

                let mut slices = Vec::new();
                let mut static_patches = Vec::new();
                let new_cx = new_cursor_rect.as_ref().map(|c| c.x).unwrap_or(0.0);
                let new_cy = new_cursor_rect.as_ref().map(|c| c.top).unwrap_or(0.0);

                for (del_start, del_end) in &deleted_ranges {
                    for old_line in old_snapshot.lines_in_byte_range(*del_start, *del_end) {
                        if let Some(source_rect) = old_line.source_rect_for_byte_range(*del_start, *del_end) {
                            slices.push(AnimatedSlice::delete_fade_out(
                                key,
                                old_line.id,
                                source_rect,
                                new_cx,
                                new_cy,
                                *del_start,
                                *del_end,
                                old_line.clusters.first().map(|c| c.shaping_identity.clone()),
                            ));
                        }
                    }
                }

                for new_line in &new_snapshot.line_snapshots {
                    let old_line = old_snapshot.line_for_byte(new_line.byte_start);
                    if let Some(ol) = old_line {
                        let old_sr = ol.source_rect_for_byte_range(ol.byte_start, ol.byte_end);
                        let new_sr = new_line.source_rect_for_byte_range(new_line.byte_start, new_line.byte_end);

                        match (old_sr, new_sr) {
                            (Some(old_src), Some(new_src)) => {
                                let same_shaping = ol.clusters.iter()
                                    .zip(new_line.clusters.iter())
                                    .take_while(|(oc, nc)| oc.shaping_identity.is_same_shaping(&nc.shaping_identity))
                                    .count() == ol.clusters.len().min(new_line.clusters.len());

                                if same_shaping {
                                    slices.push(AnimatedSlice::reflow_move(
                                        key,
                                        ol.id,
                                        old_src,
                                        new_line.id,
                                        new_src.clone(),
                                        new_line.byte_start,
                                        new_line.byte_end,
                                        ol.clusters.first().map(|c| c.shaping_identity.clone()),
                                    ));
                                } else {
                                    slices.push(AnimatedSlice::reflow_crossfade_old(
                                        key,
                                        ol.id,
                                        old_src.clone(),
                                        old_src,
                                        new_line.byte_start,
                                        new_line.byte_end,
                                    ));
                                    slices.push(AnimatedSlice::reflow_crossfade_new(
                                        key,
                                        new_line.id,
                                        new_src.clone(),
                                        new_src.clone(),
                                        new_line.byte_start,
                                        new_line.byte_end,
                                    ));
                                }
                            }
                            _ => {}
                        }
                    }
                }

                let cursor_transition = if old_cursor_rect.is_some() && new_cursor_rect.is_some() {
                    CursorTransition::Tween {
                        old_rect: old_cursor_rect.clone().unwrap(),
                        new_rect: new_cursor_rect.clone().unwrap(),
                        duration_ms: vt.duration_ms,
                    }
                } else {
                    CursorTransition::Snap
                };

                let prepared = PreparedTextVisualTransaction {
                    key,
                    state: TextVisualTransactionState::Pending,
                    operation_kind: TextVisualOperationKind::Delete,
                    animation_mode: mode,
                    duration_ms: vt.duration_ms,
                    start_time: Instant::now(),
                    old_revision: self.layout_revision,
                    new_revision,
                    slices,
                    static_patches,
                    cursor_transition,
                    old_cursor_rect: old_cursor_rect.clone(),
                    new_cursor_rect: new_cursor_rect.clone(),
                    cancel_reason: None,
                    texture_prepared: false,
                    first_render_frame: None,
                    rendering_started_at: None,
                    accumulated_paused_duration_ms: 0,
                    pause_start: None,
                    old_snapshot: Some(old_snapshot.clone()),
                    new_snapshot: Some(new_snapshot.clone()),
                };

                self.layout_revision = new_revision;
                self.prepared_queue.enqueue(prepared);
                return Some(key);
            }
            EditorAnimationKind::Cursor => {}
        }
        None
    }

    pub fn finish_by_key(&mut self, key: VisualTransactionKey) -> bool {
        self.prepared_queue.complete(key)
    }

    pub fn cancel_by_key(&mut self, key: VisualTransactionKey, reason: &str) -> bool {
        self.prepared_queue.cancel(key, reason)
    }

    pub fn suppress_all(&mut self) -> bool {
        if self.prepared_queue.is_empty() {
            return false;
        }
        self.prepared_queue.cancel_all("suppress_all");
        true
    }

    pub fn tick(&mut self, now: Instant) -> bool {
        let expired = self.prepared_queue.tick(now);
        !expired.is_empty()
    }

    pub fn has_active_insert(&self) -> bool {
        self.prepared_queue.has_active_insert()
    }

    pub fn is_empty(&self) -> bool {
        self.prepared_queue.is_empty()
    }

    pub fn has_prepared_or_rendering(&self) -> bool {
        self.prepared_queue.active_transactions()
            .iter()
            .any(|t| t.state == TextVisualTransactionState::Prepared || t.state == TextVisualTransactionState::Rendering)
    }

    pub fn current_static_render_plan(&self) -> StaticTextPlan {
        self.build_static_render_plan()
    }

    pub fn insert_byte_ranges(&self) -> Vec<(usize, usize)> {
        self.prepared_queue.insert_byte_ranges()
    }

    pub fn reflow_byte_ranges(&self) -> Vec<(usize, usize)> {
        self.prepared_queue.active_transactions()
            .iter()
            .filter(|t| t.state != TextVisualTransactionState::Cancelled && t.state != TextVisualTransactionState::Completed)
            .flat_map(|t| t.reflow_byte_ranges())
            .collect()
    }

    fn build_static_render_plan(&self) -> StaticTextPlan {
        let mut hidden_clip_rects = Vec::new();

        for tx in self.prepared_queue.active_transactions() {
            if tx.state == TextVisualTransactionState::Cancelled || tx.state == TextVisualTransactionState::Completed {
                continue;
            }
            if !tx.texture_prepared {
                continue;
            }
            for patch in &tx.static_patches {
                let snapshot = if let Some(ref snap) = tx.new_snapshot {
                    snap.line_snapshots.iter().find(|l| l.id == patch.snapshot_id)
                } else {
                    None
                };
                if let Some(line_snap) = snapshot {
                    for sr in &patch.hidden_source_rects {
                        let doc_x = sr.x / line_snap.dpr + line_snap.visual_x;
                        let doc_y = line_snap.document_origin_y + sr.y / line_snap.dpr;
                        let doc_w = sr.w / line_snap.dpr;
                        let doc_h = sr.h / line_snap.dpr;
                        hidden_clip_rects.push(HiddenClipRect {
                            key: tx.key,
                            x: doc_x,
                            y: doc_y,
                            w: doc_w,
                            h: doc_h,
                            byte_start: patch.byte_start,
                            byte_end: patch.byte_end,
                        });
                    }
                } else {
                    hidden_clip_rects.push(HiddenClipRect {
                        key: tx.key,
                        x: 0.0,
                        y: 0.0,
                        w: 0.0,
                        h: 0.0,
                        byte_start: patch.byte_start,
                        byte_end: patch.byte_end,
                    });
                }
            }
        }

        StaticTextPlan { hidden_clip_rects }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build_cursor_plan(
        &self,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
        cursor_x: f64,
        cursor_y: f64,
        cursor_h: f64,
        editor_enabled: bool,
        has_selection: bool,
        viewport_height: f64,
        is_scrolling: bool,
        is_selecting: bool,
        is_preediting: bool,
        smooth_cursor_enabled: bool,
        smooth_cursor_duration_ms: u32,
        coordinated_enabled: bool,
        scroll_y: f64,
        old_scroll_y: f64,
        old_visible: bool,
        old_blink_visible: bool,
        old_visual_x: f64,
        old_visual_y: f64,
        force_snap_next: bool,
        cursor_animation: Option<&super::rendering::CursorAnimationState>,
    ) -> CursorAnimationPlan {
        let in_viewport = cursor_y + cursor_h > 0.0 && cursor_y < viewport_height;
        let should_be_visible = editor_enabled && !has_selection && in_viewport && !is_scrolling;

        let has_active = self.has_active_insert();
        let blink_mode = if coordinated_enabled && has_active {
            CursorBlinkMode::Suppressed
        } else {
            CursorBlinkMode::Normal
        };

        let scroll_changed = (old_scroll_y - scroll_y).abs() > 0.01;
        let should_snap = is_scrolling || is_selecting || !old_visible || scroll_changed;

        let dx = (cursor_x - old_visual_x).abs();
        let dy = (cursor_y - old_visual_y).abs();

        let large_distance = force_snap_next && (dx > 80.0 || dy > cursor_h * 1.5);
        let cross_line_snap = dy > cursor_h * 3.0;

        let transition = if !should_be_visible {
            CursorTransition::Snap
        } else if should_snap || !smooth_cursor_enabled || large_distance || cross_line_snap {
            if coordinated_enabled
                && has_active
                && old_cursor_rect.is_some()
                && new_cursor_rect.is_some()
            {
                CursorTransition::Tween {
                    old_rect: old_cursor_rect.clone().unwrap(),
                    new_rect: new_cursor_rect.clone().unwrap(),
                    duration_ms: smooth_cursor_duration_ms as u64,
                }
            } else {
                CursorTransition::Snap
            }
        } else if let Some(anim) = cursor_animation {
            if (anim.target_x - cursor_x).abs() > 0.01
                || (anim.target_y - cursor_y).abs() > 0.01
            {
                if coordinated_enabled
                    && has_active
                    && old_cursor_rect.is_some()
                    && new_cursor_rect.is_some()
                {
                    CursorTransition::Tween {
                        old_rect: old_cursor_rect.clone().unwrap(),
                        new_rect: new_cursor_rect.clone().unwrap(),
                        duration_ms: smooth_cursor_duration_ms as u64,
                    }
                } else {
                    CursorTransition::Tween {
                        old_rect: CursorRect {
                            x: anim.start_x,
                            top: anim.start_y,
                            bottom: anim.start_y + cursor_h,
                            baseline_y: anim.start_y + cursor_h * 0.8,
                        },
                        new_rect: CursorRect {
                            x: cursor_x,
                            top: cursor_y,
                            bottom: cursor_y + cursor_h,
                            baseline_y: cursor_y + cursor_h * 0.8,
                        },
                        duration_ms: smooth_cursor_duration_ms as u64,
                    }
                }
            } else {
                CursorTransition::Snap
            }
        } else if (old_visual_x - cursor_x).abs() > 0.01
            || (old_visual_y - cursor_y).abs() > 0.01
        {
            if coordinated_enabled
                && has_active
                && old_cursor_rect.is_some()
                && new_cursor_rect.is_some()
            {
                CursorTransition::Tween {
                    old_rect: old_cursor_rect.clone().unwrap(),
                    new_rect: new_cursor_rect.clone().unwrap(),
                    duration_ms: smooth_cursor_duration_ms as u64,
                }
            } else {
                CursorTransition::Tween {
                    old_rect: CursorRect {
                        x: old_visual_x,
                        top: old_visual_y,
                        bottom: old_visual_y + cursor_h,
                        baseline_y: old_visual_y + cursor_h * 0.8,
                    },
                    new_rect: CursorRect {
                        x: cursor_x,
                        top: cursor_y,
                        bottom: cursor_y + cursor_h,
                        baseline_y: cursor_y + cursor_h * 0.8,
                    },
                    duration_ms: smooth_cursor_duration_ms as u64,
                }
            }
        } else {
            CursorTransition::Snap
        };

        let _ = (is_preediting, old_blink_visible);

        CursorAnimationPlan {
            should_be_visible,
            blink_mode,
            transition,
            cursor_x,
            cursor_y,
            cursor_h,
        }
    }

    pub(crate) fn build_ime_plan(
        &self,
        position_changed: bool,
        scroll_changed: bool,
    ) -> ImeUpdatePlan {
        ImeUpdatePlan {
            kind: if position_changed || scroll_changed {
                ImeUpdateKind::QueryInput
            } else {
                ImeUpdateKind::None
            },
            cursor_changed: position_changed,
            anchor_changed: false,
        }
    }

    fn active_transaction_duration_ms(&self) -> Option<u64> {
        self.prepared_queue.active_transactions()
            .iter()
            .filter(|t| t.state != TextVisualTransactionState::Cancelled && t.state != TextVisualTransactionState::Completed)
            .map(|t| t.duration_ms)
            .min()
    }

    pub(crate) fn pause_all(&mut self) {
        for tx in self.prepared_queue.active_transactions_mut() {
            tx.pause();
        }
    }

    pub(crate) fn resume_all(&mut self) {
        for tx in self.prepared_queue.active_transactions_mut() {
            tx.resume();
        }
    }

    pub(crate) fn build_render_plan_full(
        &mut self,
        cursor_plan: CursorAnimationPlan,
        ime_plan: ImeUpdatePlan,
        selection_preedit: SelectionPreeditPlan,
        mut frame_context: super::render_plan::FrameContext,
        cursor_style: super::render_plan::CursorStyle,
    ) -> RenderPlan {
        let static_text = self.build_static_render_plan();
        let (text_animation, keys_to_complete) = self.build_text_animation_plan();
        frame_context.keys_to_complete = keys_to_complete;
        let active_keys: Vec<VisualTransactionKey> = self.prepared_queue.active_transactions()
            .iter().map(|t| t.key).collect();
        frame_context.active_transaction_keys = active_keys;
        RenderPlan {
            static_text,
            text_animation,
            selection_preedit,
            cursor: cursor_plan,
            ime: ime_plan,
            frame_context,
            cursor_style,
        }
    }

    fn build_text_animation_plan(&mut self) -> (TextAnimationPlan, Vec<VisualTransactionKey>) {
        let mut glyphs = Vec::new();
        let mut keys_to_complete = Vec::new();
        let now = Instant::now();

        for tx in self.prepared_queue.active_transactions_mut() {
            if tx.state == TextVisualTransactionState::Cancelled || tx.state == TextVisualTransactionState::Completed {
                continue;
            }

            if tx.state == TextVisualTransactionState::Prepared {
                tx.state = TextVisualTransactionState::Rendering;
                if tx.first_render_frame.is_none() {
                    tx.first_render_frame = Some(Instant::now());
                }
            }

            let progress = tx.progress(now);

            if progress >= 1.0 {
                keys_to_complete.push(tx.key);
                continue;
            }

            for slice in &tx.slices {
                let frame = slice.compute_frame(progress);
                glyphs.push(TextAnimationGlyphInfo {
                    key: tx.key,
                    x: frame.x,
                    y: frame.y,
                    w: frame.w,
                    h: frame.h,
                    opacity: frame.opacity,
                    animation_mode: tx.animation_mode,
                    is_delete: tx.is_delete(),
                    snapshot_id: frame.snapshot_id,
                    source_rect: frame.source_rect,
                });
            }
        }

        (TextAnimationPlan { glyphs }, keys_to_complete)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coordinator_suppress_all() {
        let mut coord = LinuxEditorAnimationCoordinator::new();
        assert!(!coord.has_active_insert());
        let suppressed = coord.suppress_all();
        assert!(!suppressed);
    }

    #[test]
    fn test_coordinator_finish_by_key() {
        let mut coord = LinuxEditorAnimationCoordinator::new();
        let key = VisualTransactionKey::new(1, 1);
        let removed = coord.finish_by_key(key);
        assert!(!removed);
    }

    #[test]
    fn test_ime_plan_position_changed() {
        let coord = LinuxEditorAnimationCoordinator::new();
        let plan = coord.build_ime_plan(true, false);
        assert_eq!(plan.kind, ImeUpdateKind::QueryInput);
        assert!(plan.cursor_changed);
    }

    #[test]
    fn test_ime_plan_no_change() {
        let coord = LinuxEditorAnimationCoordinator::new();
        let plan = coord.build_ime_plan(false, false);
        assert_eq!(plan.kind, ImeUpdateKind::None);
    }

    #[test]
    fn test_animation_mode_system_suppressed_no_transaction() {
        let mode = AnimationMode::SystemSuppressed;
        assert!(!mode.should_create_transaction());
    }

    #[test]
    fn test_animation_mode_glyph_creates_transaction() {
        let mode = AnimationMode::GlyphAnimation;
        assert!(mode.should_create_transaction());
    }
}
