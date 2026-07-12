use std::time::Instant;

use writer_core::editor::{
    CursorRect, EditorAnimationKind, EditorVisualTransaction,
};

pub(crate) use super::transaction_key::VisualTransactionKey;
pub(crate) use super::transaction_queue::{ActiveVisualTransaction, ActiveVisualTransactionQueue, VisualTransactionState, VisualOperationKind};
pub(crate) use super::visual_payload::VisualPayload;
pub(crate) use super::animation_mode::AnimationMode;
pub(crate) use super::cursor_animation::{CursorAnimationPlan, CursorBlinkMode, CursorTransition};
pub(crate) use super::render_plan::{
    HiddenRangeInfo, StaticTextPlan, TextAnimationPlan, TextAnimationGlyphInfo,
    SelectionPreeditPlan, SelectionRange, PreeditRange,
    ImeUpdateKind, ImeUpdatePlan, RenderPlan,

};
pub(crate) use super::texture_cache::{TextureCache, TexturePhase};
pub(crate) use super::shaped_visual_run::{ShapedVisualRun, ShapedGlyph, ShapedCluster, ReflowVisualSnapshot};
use super::visual_payload::ShapedGlyphInfo;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextAnimationKind {
    Insert,
    Delete,
}

#[derive(Clone, Debug)]
pub(crate) struct AnimationRangeEntry {
    pub key: VisualTransactionKey,
    pub core_range_id: Option<u64>,
    pub kind: TextAnimationKind,
    pub byte_range: (usize, usize),
    pub reflow_hidden_ranges: Vec<ReflowHiddenRangeEntry>,
    pub animation_mode: AnimationMode,
    pub start_time: Instant,
    pub duration_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReflowHiddenRangeEntry {
    pub byte_range: (usize, usize),
}

pub(crate) struct AnimationRangeRegistry {
    entries: Vec<AnimationRangeEntry>,
}

impl AnimationRangeRegistry {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn start_insert(
        &mut self,
        key: VisualTransactionKey,
        core_range_id: Option<u64>,
        byte_range: (usize, usize),
        reflow_byte_ranges: Vec<(usize, usize)>,
        animation_mode: AnimationMode,
        duration_ms: u64,
    ) {
        let reflow_hidden_ranges = reflow_byte_ranges
            .into_iter()
            .map(|br| ReflowHiddenRangeEntry {
                byte_range: br,
            })
            .collect();
        self.entries.push(AnimationRangeEntry {
            key,
            core_range_id,
            kind: TextAnimationKind::Insert,
            byte_range,
            reflow_hidden_ranges,
            animation_mode,
            start_time: Instant::now(),
            duration_ms,
        });
    }

    pub fn start_delete(
        &mut self,
        key: VisualTransactionKey,
        byte_range: (usize, usize),
        animation_mode: AnimationMode,
        duration_ms: u64,
    ) {
        self.entries.push(AnimationRangeEntry {
            key,
            core_range_id: None,
            kind: TextAnimationKind::Delete,
            byte_range,
            reflow_hidden_ranges: Vec::new(),
            animation_mode,
            start_time: Instant::now(),
            duration_ms,
        });
    }

    pub fn finish_by_key(&mut self, key: VisualTransactionKey) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.key != key);
        self.entries.len() < before
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn tick(&mut self, now: Instant) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        let before = self.entries.len();
        self.entries.retain(|e| {
            let elapsed = now.duration_since(e.start_time).as_millis() as u64;
            elapsed < e.duration_ms * 2 + 200
        });
        self.entries.len() != before
    }

    pub fn has_active_insert(&self) -> bool {
        self.entries
            .iter()
            .any(|e| e.kind == TextAnimationKind::Insert)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn insert_byte_ranges(&self) -> Vec<(usize, usize)> {
        self.entries
            .iter()
            .filter(|e| e.kind == TextAnimationKind::Insert)
            .map(|e| e.byte_range)
            .collect()
    }

    pub fn reflow_byte_ranges(&self) -> Vec<(usize, usize)> {
        self.entries
            .iter()
            .filter(|e| e.kind == TextAnimationKind::Insert)
            .flat_map(|e| e.reflow_hidden_ranges.iter().map(|r| r.byte_range))
            .collect()
    }

    pub fn map_ranges_for_insert(&mut self, pos: usize, len: usize) {
        let new_ranges: Vec<Option<(usize, usize)>> = self
            .entries
            .iter()
            .map(|e| map_range_for_insert(e.byte_range, pos, len))
            .collect();

        for (i, e) in self.entries.iter_mut().enumerate() {
            if let Some(nr) = new_ranges[i] {
                e.byte_range = nr;
            }
        }
        let mut i = 0;
        self.entries.retain(|_| {
            let keep = new_ranges[i].is_some();
            i += 1;
            keep
        });

        for e in &mut self.entries {
            e.reflow_hidden_ranges = e
                .reflow_hidden_ranges
                .iter()
                .filter_map(|r| {
                    map_range_for_insert(r.byte_range, pos, len).map(|br| ReflowHiddenRangeEntry {
                        byte_range: br,
                    })
                })
                .collect();
        }
    }

    pub fn map_ranges_for_delete(&mut self, pos: usize, len: usize) {
        let new_ranges: Vec<Option<(usize, usize)>> = self
            .entries
            .iter()
            .map(|e| map_range_for_delete(e.byte_range, pos, len))
            .collect();

        for (i, e) in self.entries.iter_mut().enumerate() {
            if let Some(nr) = new_ranges[i] {
                e.byte_range = nr;
            }
        }
        let mut i = 0;
        self.entries.retain(|_| {
            let keep = new_ranges[i].is_some();
            i += 1;
            keep
        });

        for e in &mut self.entries {
            e.reflow_hidden_ranges = e
                .reflow_hidden_ranges
                .iter()
                .filter_map(|r| {
                    map_range_for_delete(r.byte_range, pos, len).map(|br| ReflowHiddenRangeEntry {
                        byte_range: br,
                    })
                })
                .collect();
        }
    }
}

fn map_range_for_insert(range: (usize, usize), pos: usize, len: usize) -> Option<(usize, usize)> {
    if pos <= range.0 {
        Some((range.0 + len, range.1 + len))
    } else if pos >= range.1 {
        Some(range)
    } else {
        None
    }
}

fn map_range_for_delete(range: (usize, usize), pos: usize, len: usize) -> Option<(usize, usize)> {
    let delete_end = pos + len;
    if delete_end <= range.0 {
        Some((range.0 - len, range.1 - len))
    } else if pos >= range.1 {
        Some(range)
    } else {
        None
    }
}

pub(crate) struct LinuxEditorAnimationCoordinator {
    registry: AnimationRangeRegistry,
    next_key_id: u64,
    pub(crate) vt_queue: ActiveVisualTransactionQueue,
}

impl LinuxEditorAnimationCoordinator {
    pub fn new() -> Self {
        Self {
            registry: AnimationRangeRegistry::new(),
            next_key_id: 1,
            vt_queue: ActiveVisualTransactionQueue::new(),
        }
    }

    fn alloc_key(&mut self) -> VisualTransactionKey {
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
        _cursor_x: f64,
        _cursor_y: f64,
        _cursor_h: f64,
        _editor_enabled: bool,
        _has_selection: bool,
        _viewport_height: f64,
        _is_selecting: bool,
        _is_preediting: bool,
        _smooth_cursor_enabled: bool,
        _smooth_cursor_duration_ms: u32,
        _coordinated_enabled: bool,
        _scroll_y: f64,
        _old_scroll_y: f64,
        _old_visible: bool,
        _old_blink_visible: bool,
        _old_visual_x: f64,
        _old_visual_y: f64,
        _force_snap_next: bool,
        _cursor_animation: Option<&super::rendering::CursorAnimationState>,
        font_family: &str,
        shaped_glyphs: &[ShapedGlyphInfo],
        font_size: f64,
        old_layout_runs: &[ShapedVisualRun],
        new_layout_runs: &[ShapedVisualRun],
        insert_runs: &[ShapedVisualRun],
        reflow_old_runs: &[ShapedVisualRun],
        reflow_new_runs: &[ShapedVisualRun],
    ) -> Option<VisualTransactionKey> {
        if !typing_animation_enabled || is_scrolling {
            return None;
        }

        let mode = AnimationMode::from_core(vt.animation_mode);
        if is_scrolling || is_loading || is_applying_format || is_applying_settings || !typing_animation_enabled {
            return None;
        }
        if !mode.should_create_transaction() {
            return None;
        }

        match vt.kind {
            EditorAnimationKind::Insert => {
                if let Some((range_start, range_end)) = vt.inserted_range {
                    let insert_len = range_end - range_start;
                    self.registry.map_ranges_for_insert(range_start, insert_len);

                    let key = self.alloc_key();
                    let reflow_ranges: Vec<(usize, usize)> = reflow_new_runs
                        .iter()
                        .map(|r| r.string_range())
                        .collect();
                    let core_range_id = vt.hidden_visual_ranges.first().map(|r| r.id);

                    let has_reflow = !reflow_new_runs.is_empty();

                    let payload = match mode {
                        AnimationMode::GlyphAnimation => {
                            let run = match insert_runs.first().cloned() {
                                Some(r) => r,
                                None => return None,
                            };
                            let glyph = run.glyphs.first().cloned().unwrap_or(ShapedGlyph {
                                glyph_index: 0, glyph_position_x: run.visual_x,
                                glyph_position_y: run.baseline_y, string_index: run.source_string_start,
                                advance_width: run.visual_w,
                            });
                            let glyph_bounds = super::visual_payload::GlyphBounds {
                                x: run.visual_x, y: run.visual_y, w: run.visual_w, h: run.visual_h,
                            };
                            let texture_region = super::visual_payload::TextureRegion {
                                x: 0.0, y: 0.0, w: run.visual_w, h: run.visual_h,
                            };
                            VisualPayload::from_glyph_payload(
                                run.qglyphrun_index,
                                0,
                                glyph,
                                glyph_bounds,
                                texture_region,
                                run.clone(),
                                old_cursor_rect,
                                new_cursor_rect,
                                mode,
                            )
                        }
                        AnimationMode::ClusterAnimation => {
                            if insert_runs.is_empty() {
                                return None;
                            }
                            let run = &insert_runs[0];
                            let cluster = run.clusters.first().cloned().unwrap_or(ShapedCluster {
                                string_start: run.source_string_start,
                                string_end: run.source_string_end,
                                glyph_start: 0,
                                glyph_end: run.glyphs.len(),
                            });
                            let cluster_bounds = super::visual_payload::GlyphBounds {
                                x: run.visual_x, y: run.visual_y, w: run.visual_w, h: run.visual_h,
                            };
                            let texture_region = super::visual_payload::TextureRegion {
                                x: 0.0, y: 0.0, w: run.visual_w, h: run.visual_h,
                            };
                            VisualPayload::from_cluster_payload(
                                run.qglyphrun_index,
                                cluster,
                                (0, run.glyphs.len()),
                                (run.source_string_start, run.source_string_end),
                                cluster_bounds,
                                texture_region,
                                run.clone(),
                                old_cursor_rect,
                                new_cursor_rect,
                                mode,
                            )
                        }
                        AnimationMode::RunAnimation => {
                            let run = match insert_runs.first().cloned() {
                                Some(r) => r,
                                None => return None,
                            };
                            let reflow_snapshot = if has_reflow && !reflow_old_runs.is_empty() && !reflow_new_runs.is_empty() {
                                Some(ReflowVisualSnapshot::new(
                                    reflow_old_runs.to_vec(),
                                    reflow_new_runs.to_vec(),
                                ))
                            } else {
                                None
                            };
                            VisualPayload::from_run_payload(
                                run,
                                reflow_snapshot,
                                old_cursor_rect,
                                new_cursor_rect,
                                mode,
                            )
                        }
                        AnimationMode::LineReflowAnimation => {
                            if insert_runs.is_empty() {
                                return None;
                            }
                            let old_runs = if !reflow_old_runs.is_empty() {
                                reflow_old_runs.to_vec()
                            } else {
                                old_layout_runs.to_vec()
                            };
                            let new_runs = if !reflow_new_runs.is_empty() {
                                reflow_new_runs.to_vec()
                            } else {
                                new_layout_runs.to_vec()
                            };
                            let reflow_snapshot = ReflowVisualSnapshot::new(old_runs.clone(), new_runs.clone());
                            VisualPayload::from_line_reflow_payload(
                                old_runs,
                                new_runs,
                                reflow_snapshot.run_mapping.clone(),
                                insert_runs.to_vec(),
                                Some((range_start, range_end)),
                                old_cursor_rect,
                                new_cursor_rect,
                                mode,
                            )
                        }
                        AnimationMode::SystemSuppressed => return None,
                    };

                    self.registry.start_insert(
                        key,
                        core_range_id,
                        (range_start, range_end),
                        reflow_ranges,
                        mode,
                        vt.duration_ms,
                    );
                    self.vt_queue.enqueue(key, VisualOperationKind::Insert, payload, mode, vt.duration_ms);
                    return Some(key);
                }
            }
            EditorAnimationKind::Delete => {
                let key = self.alloc_key();
                let changes = writer_core::editor::diff_plain_text(&vt.old_text, &vt.new_text);
                for change in &changes {
                    if let writer_core::editor::EditorChange::Delete { index, text } = change {
                        let range_start = *index;
                        let delete_len = text.len();
                        self.registry.map_ranges_for_delete(range_start, delete_len);
                    }
                }

                let mut deleted_ranges: Vec<(usize, usize)> = Vec::new();
                for change in &changes {
                    if let writer_core::editor::EditorChange::Delete { index, text } = change {
                        let range_start = *index;
                        let range_end = range_start + text.len();
                        deleted_ranges.push((range_start, range_end));
                        self.registry.start_delete(
                            key,
                            (range_start, range_end),
                            mode,
                            vt.duration_ms,
                        );
                    }
                }

                let delete_runs: Vec<ShapedVisualRun> = old_layout_runs.iter()
                    .filter(|run| {
                        deleted_ranges.iter().any(|(ds, de)| {
                            run.source_string_end > *ds && run.source_string_start < *de
                        })
                    })
                    .cloned()
                    .collect();

                if delete_runs.is_empty() && old_layout_runs.is_empty() {
                    return None;
                }

                let payload = match mode {
                    AnimationMode::GlyphAnimation => {
                        let run = match delete_runs.first().cloned() {
                            Some(r) => r,
                            None => return None,
                        };
                        let glyph = run.glyphs.first().cloned().unwrap_or(ShapedGlyph {
                            glyph_index: 0, glyph_position_x: run.visual_x,
                            glyph_position_y: run.baseline_y, string_index: run.source_string_start,
                            advance_width: run.visual_w,
                        });
                        let glyph_bounds = super::visual_payload::GlyphBounds {
                            x: run.visual_x, y: run.visual_y, w: run.visual_w, h: run.visual_h,
                        };
                        let texture_region = super::visual_payload::TextureRegion {
                            x: 0.0, y: 0.0, w: run.visual_w, h: run.visual_h,
                        };
                        VisualPayload::from_glyph_payload(
                            run.qglyphrun_index,
                            0,
                            glyph,
                            glyph_bounds,
                            texture_region,
                            run,
                            old_cursor_rect,
                            new_cursor_rect,
                            mode,
                        )
                    }
                    AnimationMode::ClusterAnimation => {
                        if delete_runs.is_empty() {
                            return None;
                        }
                        let run = &delete_runs[0];
                        let cluster = run.clusters.first().cloned().unwrap_or(ShapedCluster {
                            string_start: run.source_string_start,
                            string_end: run.source_string_end,
                            glyph_start: 0,
                            glyph_end: run.glyphs.len(),
                        });
                        let cluster_bounds = super::visual_payload::GlyphBounds {
                            x: run.visual_x, y: run.visual_y, w: run.visual_w, h: run.visual_h,
                        };
                        let texture_region = super::visual_payload::TextureRegion {
                            x: 0.0, y: 0.0, w: run.visual_w, h: run.visual_h,
                        };
                        VisualPayload::from_cluster_payload(
                            run.qglyphrun_index,
                            cluster,
                            (0, run.glyphs.len()),
                            (run.source_string_start, run.source_string_end),
                            cluster_bounds,
                            texture_region,
                            run.clone(),
                            old_cursor_rect,
                            new_cursor_rect,
                            mode,
                        )
                    }
                    AnimationMode::RunAnimation => {
                        let run = match delete_runs.first().cloned() {
                            Some(r) => r,
                            None => return None,
                        };
                        VisualPayload::from_run_payload(
                            run,
                            None,
                            old_cursor_rect,
                            new_cursor_rect,
                            mode,
                        )
                    }
                    AnimationMode::LineReflowAnimation => {
                        let old_runs = if !delete_runs.is_empty() {
                            delete_runs
                        } else if !old_layout_runs.is_empty() {
                            old_layout_runs.to_vec()
                        } else {
                            return None;
                        };
                        let new_runs = if !new_layout_runs.is_empty() {
                            new_layout_runs.to_vec()
                        } else {
                            old_runs.clone()
                        };
                        let reflow_snapshot = ReflowVisualSnapshot::new(old_runs.clone(), new_runs.clone());
                        VisualPayload::from_line_reflow_payload(
                            old_runs,
                            new_runs,
                            reflow_snapshot.run_mapping.clone(),
                            Vec::new(),
                            None,
                            old_cursor_rect,
                            new_cursor_rect,
                            mode,
                        )
                    }
                    AnimationMode::SystemSuppressed => return None,
                };
                self.vt_queue.enqueue(key, VisualOperationKind::Delete, payload, mode, vt.duration_ms);
                return Some(key);
            }
            EditorAnimationKind::Cursor => {}
        }
        None
    }

    pub fn finish_by_key(&mut self, key: VisualTransactionKey) -> bool {
        let registry_result = self.registry.finish_by_key(key);
        if registry_result {
            self.vt_queue.complete(key);
        }
        registry_result
    }

    pub fn cancel_by_key(&mut self, key: VisualTransactionKey, reason: &str) -> bool {
        let registry_result = self.registry.finish_by_key(key);
        if registry_result {
            self.vt_queue.cancel(key, reason);
        }
        registry_result
    }

    pub fn suppress_all(&mut self) -> bool {
        if self.registry.is_empty() && self.vt_queue.is_empty() {
            return false;
        }
        self.registry.clear();
        self.vt_queue.cancel_all("suppress_all");
        true
    }

    pub fn tick(&mut self, now: Instant) -> bool {
        let registry_changed = self.registry.tick(now);
        let queue_changed = self.vt_queue.tick(now);
        registry_changed || queue_changed
    }

    pub fn has_active_insert(&self) -> bool {
        self.registry.has_active_insert() || self.vt_queue.has_active_insert()
    }

    pub fn is_empty(&self) -> bool {
        self.registry.is_empty() && self.vt_queue.is_empty()
    }

    pub fn current_static_render_plan(&self) -> StaticTextPlan {
        self.build_static_render_plan()
    }

    pub fn insert_byte_ranges(&self) -> Vec<(usize, usize)> {
        self.registry.insert_byte_ranges()
    }

    pub fn reflow_byte_ranges(&self) -> Vec<(usize, usize)> {
        self.registry.reflow_byte_ranges()
    }

    fn build_static_render_plan(&self) -> StaticTextPlan {
        let mut hidden_ranges = Vec::new();

        for e in self.registry.entries.iter() {
            if e.kind == TextAnimationKind::Insert {
                let tx = self.vt_queue.active_transactions().iter().find(|t| t.key == e.key);
                let texture_prepared = tx.map(|t| t.texture_prepared).unwrap_or(false);
                if !texture_prepared {
                    continue;
                }
                hidden_ranges.push(HiddenRangeInfo {
                    key: e.key,
                    byte_range: e.byte_range,
                });
                for r in &e.reflow_hidden_ranges {
                    hidden_ranges.push(HiddenRangeInfo {
                        key: e.key,
                        byte_range: r.byte_range,
                    });
                }
            }
        }

        StaticTextPlan { hidden_ranges }
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
        let safety_snap = dy > cursor_h * 3.0;

        let transition = if !should_be_visible {
            CursorTransition::Snap
        } else if should_snap || !smooth_cursor_enabled || large_distance || safety_snap {
            if coordinated_enabled
                && has_active
                && old_cursor_rect.is_some()
                && new_cursor_rect.is_some()
            {
                CursorTransition::Tween {
                    old_rect: old_cursor_rect.unwrap(),
                    new_rect: new_cursor_rect.unwrap(),
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
                        old_rect: old_cursor_rect.unwrap(),
                        new_rect: new_cursor_rect.unwrap(),
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
                    old_rect: old_cursor_rect.unwrap(),
                    new_rect: new_cursor_rect.unwrap(),
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

    pub(crate) fn build_render_plan_full(
        &self,
        cursor_plan: CursorAnimationPlan,
        ime_plan: ImeUpdatePlan,
        selection_preedit: SelectionPreeditPlan,
        mut frame_context: super::render_plan::FrameContext,
        cursor_style: super::render_plan::CursorStyle,
    ) -> RenderPlan {
        let static_text = self.build_static_render_plan();
        let (text_animation, keys_to_complete) = self.build_text_animation_plan();
        frame_context.keys_to_complete = keys_to_complete;
        frame_context.active_transaction_keys = self.vt_queue.active_transactions()
            .iter().map(|t| t.key).collect();
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

    pub(crate) fn build_render_plan(&self, selection_preedit: SelectionPreeditPlan) -> RenderPlan {
        let static_text = self.build_static_render_plan();
        let (text_animation, _) = self.build_text_animation_plan();
        RenderPlan {
            static_text,
            text_animation,
            selection_preedit,
            cursor: CursorAnimationPlan::default(),
            ime: ImeUpdatePlan::default(),
            frame_context: super::render_plan::FrameContext::default(),
            cursor_style: super::render_plan::CursorStyle::default(),
        }
    }

    fn build_text_animation_plan(&self) -> (TextAnimationPlan, Vec<VisualTransactionKey>) {
        let mut glyphs = Vec::new();
        let mut keys_to_complete = Vec::new();
        let now = Instant::now();

        for tx in self.vt_queue.active_transactions() {
            if tx.state == VisualTransactionState::Cancelled {
                continue;
            }

            let elapsed_ms = now.duration_since(tx.start_time).as_millis() as f64;
            let progress = (elapsed_ms / tx.duration_ms as f64).min(1.0);

            if progress >= 1.0 {
                keys_to_complete.push(tx.key);
                continue;
            }

            let mode = tx.animation_mode;
            let is_delete = tx.is_delete();

            match &tx.payload {
                VisualPayload::CursorTransition { .. } => {}
                VisualPayload::GlyphPayload { payload, .. } => {
                    let run = &payload.parent_run;
                    if is_delete {
                        let fade_out = 1.0 - progress;
                        let new_cx = payload.new_cursor_rect.as_ref().map(|c| c.x).unwrap_or(run.visual_x);
                        let new_cy = payload.new_cursor_rect.as_ref().map(|c| c.top).unwrap_or(run.visual_y);
                        let dx = new_cx - run.visual_x;
                        let dy = new_cy - run.visual_y;
                        let gx = run.visual_x + dx * progress;
                        let gy = run.visual_y + dy * progress;
                        let scale = 1.0 - 0.3 * progress;

                        glyphs.push(TextAnimationGlyphInfo {
                            key: tx.key,
                            x: gx,
                            y: gy,
                            w: run.visual_w * scale,
                            h: run.visual_h * scale,
                            opacity: fade_out,
                            baseline_in_quad: (run.baseline_y - run.visual_y) * scale,
                            animation_mode: mode,
                            is_delete: true,
                            texture_phase: if is_delete { TexturePhase::DeleteOld } else { TexturePhase::Insert },
                            run_identity: run.qglyphrun_index,
                        });
                    } else {
                        let eased = 1.0 - (1.0 - progress).powi(3);
                        let old_cx = payload.old_cursor_rect.as_ref().map(|c| c.x).unwrap_or(run.visual_x);
                        let old_cy = payload.old_cursor_rect.as_ref().map(|c| c.top).unwrap_or(run.visual_y);
                        let dx = run.visual_x - old_cx;
                        let dy = run.visual_y - old_cy;
                        let gx = old_cx + dx * eased;
                        let gy = old_cy + dy * eased;

                        glyphs.push(TextAnimationGlyphInfo {
                            key: tx.key,
                            x: gx,
                            y: gy,
                            w: run.visual_w,
                            h: run.visual_h,
                            opacity: eased,
                            baseline_in_quad: run.baseline_y - gy,
                            animation_mode: mode,
                            is_delete: false,
                            texture_phase: if is_delete { TexturePhase::DeleteOld } else { TexturePhase::Insert },
                            run_identity: run.qglyphrun_index,
                        });
                    }
                }
                VisualPayload::ClusterPayload { payload, .. } => {
                    let run = &payload.parent_run;
                    if is_delete {
                        let fade_out = 1.0 - progress;
                        let new_cx = payload.new_cursor_rect.as_ref().map(|c| c.x).unwrap_or(run.visual_x);
                        let new_cy = payload.new_cursor_rect.as_ref().map(|c| c.top).unwrap_or(run.visual_y);
                        let dx = new_cx - run.visual_x;
                        let dy = new_cy - run.visual_y;
                        let gx = run.visual_x + dx * progress;
                        let gy = run.visual_y + dy * progress;
                        let scale = 1.0 - 0.3 * progress;

                        glyphs.push(TextAnimationGlyphInfo {
                            key: tx.key,
                            x: gx,
                            y: gy,
                            w: run.visual_w * scale,
                            h: run.visual_h * scale,
                            opacity: fade_out,
                            baseline_in_quad: (run.baseline_y - run.visual_y) * scale,
                            animation_mode: mode,
                            is_delete: true,
                            texture_phase: TexturePhase::DeleteOld,
                            run_identity: payload.run_identity,
                        });
                    } else {
                        let eased = 1.0 - (1.0 - progress).powi(3);
                        let old_cx = payload.old_cursor_rect.as_ref().map(|c| c.x).unwrap_or(run.visual_x);
                        let old_cy = payload.old_cursor_rect.as_ref().map(|c| c.top).unwrap_or(run.visual_y);
                        let dx = run.visual_x - old_cx;
                        let dy = run.visual_y - old_cy;
                        let gx = old_cx + dx * eased;
                        let gy = old_cy + dy * eased;

                        glyphs.push(TextAnimationGlyphInfo {
                            key: tx.key,
                            x: gx,
                            y: gy,
                            w: run.visual_w,
                            h: run.visual_h,
                            opacity: eased,
                            baseline_in_quad: run.baseline_y - gy,
                            animation_mode: mode,
                            is_delete: false,
                            texture_phase: TexturePhase::Insert,
                            run_identity: payload.run_identity,
                        });
                    }
                }
                VisualPayload::RunPayload { payload, .. } => {
                    let run = &payload.shaped_run;
                    if is_delete {
                        let fade_out = 1.0 - progress;
                        let new_cx = payload.new_cursor_rect.as_ref().map(|c| c.x).unwrap_or(run.visual_x);
                        let new_cy = payload.new_cursor_rect.as_ref().map(|c| c.top).unwrap_or(run.visual_y);
                        let dx = new_cx - run.visual_x;
                        let dy = new_cy - run.visual_y;
                        let gx = run.visual_x + dx * progress;
                        let gy = run.visual_y + dy * progress;
                        let scale = 1.0 - 0.3 * progress;

                        glyphs.push(TextAnimationGlyphInfo {
                            key: tx.key,
                            x: gx,
                            y: gy,
                            w: run.visual_w * scale,
                            h: run.visual_h * scale,
                            opacity: fade_out,
                            baseline_in_quad: (run.baseline_y - run.visual_y) * scale,
                            animation_mode: mode,
                            is_delete: true,
                            texture_phase: if is_delete { TexturePhase::DeleteOld } else { TexturePhase::Insert },
                            run_identity: run.qglyphrun_index,
                        });
                    } else {
                        let eased = 1.0 - (1.0 - progress).powi(3);
                        let old_cx = payload.old_cursor_rect.as_ref().map(|c| c.x).unwrap_or(run.visual_x);
                        let old_cy = payload.old_cursor_rect.as_ref().map(|c| c.top).unwrap_or(run.visual_y);
                        let dx = run.visual_x - old_cx;
                        let dy = run.visual_y - old_cy;
                        let gx = old_cx + dx * eased;
                        let gy = old_cy + dy * eased;

                        glyphs.push(TextAnimationGlyphInfo {
                            key: tx.key,
                            x: gx,
                            y: gy,
                            w: run.visual_w,
                            h: run.visual_h,
                            opacity: eased,
                            baseline_in_quad: run.baseline_y - gy,
                            animation_mode: mode,
                            is_delete: false,
                            texture_phase: if is_delete { TexturePhase::DeleteOld } else { TexturePhase::Insert },
                            run_identity: run.qglyphrun_index,
                        });
                    }

                    if let Some(ref reflow_snapshot) = payload.reflow_snapshot {
                        for (run_idx, new_run) in reflow_snapshot.new_shaped_runs.iter().enumerate() {
                            let can_reuse = reflow_snapshot.can_reuse_texture_for_run(run_idx);
                            let needs_crossfade = reflow_snapshot.run_needs_crossfade(run_idx);

                            let (frame_x, frame_y, frame_opacity) = if can_reuse && !needs_crossfade {
                                let old_pos = reflow_snapshot.old_positions.get(run_idx);
                                let new_pos = reflow_snapshot.new_positions.get(run_idx);
                                let eased_pos = 1.0 - (1.0 - progress).powi(2);
                                match (old_pos, new_pos) {
                                    (Some((ox, oy, _)), Some((nx, ny, _))) => {
                                        (ox + (nx - ox) * eased_pos, oy + (ny - oy) * eased_pos, 1.0)
                                    }
                                    _ => (new_run.visual_x, new_run.visual_y, 1.0),
                                }
                            } else if needs_crossfade {
                                let eased_fade = 1.0 - (1.0 - progress).powi(2);
                                (new_run.visual_x, new_run.visual_y, eased_fade)
                            } else {
                                (new_run.visual_x, new_run.visual_y, 1.0)
                            };

                            glyphs.push(TextAnimationGlyphInfo {
                                key: tx.key,
                                x: frame_x,
                                y: frame_y,
                                w: new_run.visual_w,
                                h: new_run.visual_h,
                                opacity: frame_opacity,
                                baseline_in_quad: new_run.baseline_y - frame_y,
                                animation_mode: mode,
                                is_delete: false,
                                texture_phase: TexturePhase::NewReflow,
                                run_identity: new_run.qglyphrun_index,
                            });
                        }
                    }
                }
                VisualPayload::LineReflowPayload { payload, .. } => {
                    for (run_idx, new_run) in payload.new_runs.iter().enumerate() {
                        let mapping = payload.run_mapping.iter().find(|m| m.new_run_index == run_idx);
                        let can_reuse = mapping.map(|m| m.same_shaping).unwrap_or(false);
                        let needs_crossfade = !can_reuse;

                        let old_pos = mapping.and_then(|m| {
                            payload.old_runs.get(m.old_run_index)
                                .map(|r| (r.visual_x, r.visual_y, r.baseline_y))
                        });

                        let (frame_x, frame_y, frame_opacity, phase) = if can_reuse && !needs_crossfade {
                            let new_pos = (new_run.visual_x, new_run.visual_y, new_run.baseline_y);
                            let eased = 1.0 - (1.0 - progress).powi(2);
                            if let Some((ox, oy, _)) = old_pos {
                                (ox + (new_pos.0 - ox) * eased, oy + (new_pos.1 - oy) * eased, 1.0, TexturePhase::NewReflow)
                            } else {
                                (new_run.visual_x, new_run.visual_y, 1.0, TexturePhase::NewReflow)
                            }
                        } else if needs_crossfade {
                            if let Some((ox, oy, _)) = old_pos {
                                let fade_out = 1.0 - progress;
                                let old_run = mapping.and_then(|m| payload.old_runs.get(m.old_run_index));
                                glyphs.push(TextAnimationGlyphInfo {
                                    key: tx.key,
                                    x: ox,
                                    y: oy,
                                    w: old_run.map(|r| r.visual_w).unwrap_or(new_run.visual_w),
                                    h: old_run.map(|r| r.visual_h).unwrap_or(new_run.visual_h),
                                    opacity: fade_out,
                                    baseline_in_quad: old_run.map(|r| r.baseline_y - oy).unwrap_or(new_run.baseline_y - oy),
                                    animation_mode: mode,
                                    is_delete: false,
                                    texture_phase: TexturePhase::OldReflow,
                                    run_identity: old_run.map(|r| r.qglyphrun_index + (mapping.unwrap().old_run_index as i32)).unwrap_or(new_run.qglyphrun_index + (run_idx as i32)),
                                });
                            }
                            let eased = 1.0 - (1.0 - progress).powi(2);
                            (new_run.visual_x, new_run.visual_y, eased, TexturePhase::NewReflow)
                        } else {
                            (new_run.visual_x, new_run.visual_y, 1.0, TexturePhase::NewReflow)
                        };

                        glyphs.push(TextAnimationGlyphInfo {
                            key: tx.key,
                            x: frame_x,
                            y: frame_y,
                            w: new_run.visual_w,
                            h: new_run.visual_h,
                            opacity: frame_opacity,
                            baseline_in_quad: new_run.baseline_y - frame_y,
                            animation_mode: mode,
                            is_delete: false,
                            texture_phase: phase,
                            run_identity: new_run.qglyphrun_index + (run_idx as i32),
                        });
                    }

                    for insert_run in &payload.insert_runs {
                        let eased = 1.0 - (1.0 - progress).powi(3);
                        let old_cx = payload.old_cursor_rect.as_ref().map(|c| c.x).unwrap_or(insert_run.visual_x);
                        let old_cy = payload.old_cursor_rect.as_ref().map(|c| c.top).unwrap_or(insert_run.visual_y);
                        let dx = insert_run.visual_x - old_cx;
                        let dy = insert_run.visual_y - old_cy;
                        let gx = old_cx + dx * eased;
                        let gy = old_cy + dy * eased;

                        glyphs.push(TextAnimationGlyphInfo {
                            key: tx.key,
                            x: gx,
                            y: gy,
                            w: insert_run.visual_w,
                            h: insert_run.visual_h,
                            opacity: eased,
                            baseline_in_quad: (insert_run.baseline_y - insert_run.visual_y) + (insert_run.visual_y - gy),
                            animation_mode: mode,
                            is_delete: false,
                            texture_phase: TexturePhase::Insert,
                            run_identity: insert_run.qglyphrun_index,
                        });
                    }
                }
            }
        }

        (TextAnimationPlan { glyphs }, keys_to_complete)
    }
}

#[cfg(test)]
 mod tests {
     use super::*;
     use super::super::shaped_visual_run::{RawFontCacheKey, RunFlags};
     use std::time::Duration;
     use writer_core::editor::AnimationMode as CoreAnimationMode;
     use writer_core::editor::{GlyphRect, ReflowGlyphRect};

    fn make_key(id: u64) -> VisualTransactionKey {
        VisualTransactionKey::new(id, id)
    }

    #[test]
    fn test_registry_insert_creates_hidden_range() {
        let mut reg = AnimationRangeRegistry::new();
        reg.start_insert(make_key(1), None, (10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        assert_eq!(reg.insert_byte_ranges(), vec![(10, 20)]);
        assert!(reg.has_active_insert());
        assert!(!reg.is_empty());
    }

    #[test]
    fn test_registry_delete_no_hidden_range() {
        let mut reg = AnimationRangeRegistry::new();
        reg.start_delete(make_key(1), (5, 15), AnimationMode::GlyphAnimation, 100);
        assert!(reg.insert_byte_ranges().is_empty());
        assert!(!reg.has_active_insert());
        assert!(!reg.is_empty());
    }

    #[test]
    fn test_registry_clear() {
        let mut reg = AnimationRangeRegistry::new();
        reg.start_insert(make_key(1), None, (10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        reg.start_delete(make_key(2), (30, 40), AnimationMode::GlyphAnimation, 100);
        assert!(!reg.is_empty());
        reg.clear();
        assert!(reg.is_empty());
    }

    #[test]
    fn test_registry_finish_by_key() {
        let mut reg = AnimationRangeRegistry::new();
        let key = make_key(1);
        reg.start_insert(key, None, (10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        assert!(reg.has_active_insert());
        let removed = reg.finish_by_key(key);
        assert!(removed);
        assert!(reg.is_empty());
    }

    #[test]
    fn test_registry_finish_wrong_key_no_effect() {
        let mut reg = AnimationRangeRegistry::new();
        reg.start_insert(make_key(1), None, (10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        let removed = reg.finish_by_key(make_key(999));
        assert!(!removed);
        assert!(reg.has_active_insert());
    }

    #[test]
    fn test_registry_timeout() {
        let mut reg = AnimationRangeRegistry::new();
        reg.start_insert(make_key(1), None, (10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        let now = Instant::now() + Duration::from_millis(401);
        assert!(reg.tick(now));
        assert!(reg.is_empty());
    }

    #[test]
    fn test_registry_map_ranges_for_insert() {
        let mut reg = AnimationRangeRegistry::new();
        reg.start_insert(make_key(1), None, (10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        reg.map_ranges_for_insert(5, 3);
        assert_eq!(reg.insert_byte_ranges(), vec![(13, 23)]);
    }

    #[test]
    fn test_registry_map_ranges_insert_inside_cancels() {
        let mut reg = AnimationRangeRegistry::new();
        reg.start_insert(make_key(1), None, (10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        reg.map_ranges_for_insert(15, 3);
        assert!(reg.is_empty());
    }

    #[test]
    fn test_registry_map_ranges_for_delete() {
        let mut reg = AnimationRangeRegistry::new();
        reg.start_insert(make_key(1), None, (10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        reg.map_ranges_for_delete(5, 3);
        assert_eq!(reg.insert_byte_ranges(), vec![(7, 17)]);
    }

    #[test]
    fn test_registry_reflow_ranges() {
        let mut reg = AnimationRangeRegistry::new();
        reg.start_insert(make_key(1), None, (10, 20), vec![(20, 25), (25, 30)], AnimationMode::GlyphAnimation, 100);
        assert_eq!(reg.reflow_byte_ranges(), vec![(20, 25), (25, 30)]);
    }

    #[test]
    fn test_concurrent_inserts_different_keys() {
        let mut reg = AnimationRangeRegistry::new();
        reg.start_insert(make_key(1), None, (5, 6), vec![], AnimationMode::GlyphAnimation, 100);
        reg.start_insert(make_key(2), None, (15, 16), vec![], AnimationMode::GlyphAnimation, 100);
        assert_eq!(reg.insert_byte_ranges().len(), 2);
        let removed = reg.finish_by_key(make_key(2));
        assert!(removed);
        assert!(reg.has_active_insert());
        assert_eq!(reg.insert_byte_ranges(), vec![(5, 6)]);
    }

    #[test]
    fn test_visual_transaction_key_uniqueness() {
        let key1 = make_key(1);
        let key2 = make_key(2);
        assert_ne!(key1, key2);
        let key1_dup = make_key(1);
        assert_eq!(key1, key1_dup);
    }

    #[test]
    fn test_static_render_plan_merged_byte_ranges() {
        let plan = StaticTextPlan {
            hidden_ranges: vec![
                HiddenRangeInfo {
                    key: make_key(1),
                    byte_range: (10, 20),
                },
                HiddenRangeInfo {
                    key: make_key(1),
                    byte_range: (18, 25),
                },
                HiddenRangeInfo {
                    key: make_key(2),
                    byte_range: (30, 40),
                },
            ],
        };
        let merged = plan.merged_byte_ranges();
        assert_eq!(merged, vec![(10, 25), (30, 40)]);
    }

    #[test]
    fn test_coordinator_suppress_all() {
        let mut coord = LinuxEditorAnimationCoordinator::new();
        coord.registry.start_insert(make_key(1), None, (5, 6), vec![], AnimationMode::GlyphAnimation, 100);
        assert!(coord.has_active_insert());
        let suppressed = coord.suppress_all();
        assert!(suppressed);
        assert!(!coord.has_active_insert());
        assert!(coord.is_empty());
    }

    #[test]
    fn test_coordinator_finish_by_key() {
        let mut coord = LinuxEditorAnimationCoordinator::new();
        let key = make_key(1);
        coord.registry.start_insert(key, None, (5, 6), vec![], AnimationMode::GlyphAnimation, 100);
        let removed = coord.finish_by_key(key);
        assert!(removed);
        assert!(coord.is_empty());
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

    #[test]
    fn test_glyph_payload_creation() {
        let font_key = RawFontCacheKey::new("Sans", "", 50, 16);
        let run = ShapedVisualRun {
            glyphs: vec![ShapedGlyph { glyph_index: 0, glyph_position_x: 10.0, glyph_position_y: 20.0, string_index: 0, advance_width: 12.0 }],
            clusters: vec![ShapedCluster { string_start: 0, string_end: 1, glyph_start: 0, glyph_end: 1 }],
            raw_font_key: font_key,
            flags: RunFlags::empty(),
            source_string_start: 0,
            source_string_end: 1,
            baseline_y: 20.0,
            visual_x: 10.0,
            visual_y: 5.0,
            visual_w: 12.0,
            visual_h: 20.0,
            texture_atlas_x: 0.0,
            texture_atlas_y: 0.0,
            texture_atlas_w: 12.0,
            texture_atlas_h: 20.0,
            texture_translate_x: 0.0,
            texture_translate_y: 0.0,
            qglyphrun_index: 0,
            para_text: None,
            qtextline_idx: None,
            paragraph_wrap_w: None,
            para_indent: None,
            line_y: 0.0,
        };
        let glyph = run.glyphs.first().cloned().unwrap();
        let glyph_bounds = crate::sujian_editor_item::visual_payload::GlyphBounds {
            x: run.visual_x, y: run.visual_y, w: run.visual_w, h: run.visual_h,
        };
        let texture_region = crate::sujian_editor_item::visual_payload::TextureRegion {
            x: 0.0, y: 0.0, w: run.visual_w, h: run.visual_h,
        };
        let payload = VisualPayload::from_glyph_payload(
            run.qglyphrun_index, 0, glyph, glyph_bounds, texture_region, run, None, None, AnimationMode::GlyphAnimation,
        );
        assert!(matches!(payload, VisualPayload::GlyphPayload { .. }));
        assert!(payload.is_insert());
        assert_eq!(payload.total_glyph_count(), 1);
    }

    #[test]
    fn test_line_reflow_payload_creation() {
        let font_key = RawFontCacheKey::new("Test", "", 50, 16);
        let old_run = ShapedVisualRun {
            glyphs: vec![ShapedGlyph { glyph_index: 1, glyph_position_x: 0.0, glyph_position_y: 0.0, string_index: 0, advance_width: 10.0 }],
            clusters: vec![ShapedCluster { string_start: 0, string_end: 1, glyph_start: 0, glyph_end: 1 }],
            raw_font_key: font_key.clone(),
            flags: RunFlags::empty(),
            source_string_start: 0,
            source_string_end: 1,
            baseline_y: 12.0,
            visual_x: 0.0,
            visual_y: 0.0,
            visual_w: 10.0,
            visual_h: 16.0,
            texture_atlas_x: 0.0,
            texture_atlas_y: 0.0,
            texture_atlas_w: 10.0,
            texture_atlas_h: 16.0,
            texture_translate_x: 0.0,
            texture_translate_y: 0.0,
            qglyphrun_index: 0,
            para_text: None,
            qtextline_idx: None,
            paragraph_wrap_w: None,
            para_indent: None,
            line_y: 0.0,
        };
        let mut new_run = old_run.clone();
        new_run.visual_x = 10.0;
        let snapshot = ReflowVisualSnapshot::new(vec![old_run], vec![new_run]);
        let payload = VisualPayload::from_line_reflow_payload(
            snapshot.old_shaped_runs.clone(),
            snapshot.new_shaped_runs.clone(),
            snapshot.run_mapping.clone(),
            vec![],
            Some((0, 1)),
            None,
            None,
            AnimationMode::LineReflowAnimation,
        );
        assert!(matches!(payload, VisualPayload::LineReflowPayload { .. }));
        assert!(payload.is_insert());
    }

    #[test]
    fn test_scroll_suppresses_insert_animation() {
        let mut coord = LinuxEditorAnimationCoordinator::new();
        let vt = EditorVisualTransaction {
            id: 1,
            kind: EditorAnimationKind::Insert,
            cause: writer_core::editor::EditorTransactionCause::Typing,
            old_text: "a".into(),
            new_text: "ab".into(),
            old_selection: writer_core::editor::EditorSelection::collapsed("a", 1),
            new_selection: writer_core::editor::EditorSelection::collapsed("ab", 2),
            inserted_range: Some((1, 2)),
            deleted_glyph_rects: None,
            insert_glyph_rects: Some(vec![]),
            reflow_glyph_rects: None,
            animation_mode: CoreAnimationMode::GlyphAnimation,
            cluster_rects: None,
            cluster_runs: None,
            hidden_visual_ranges: vec![],
            old_cursor_rect: None,
            new_cursor_rect: None,
            duration_ms: 160,
            coordinate_mode: writer_core::editor::VisualCoordinateMode::Baseline,
        };
        coord.process_transaction(
            &vt,
            true,
            true,
            false, false, false,
            None, None,
            50.0, 5.0, 20.0,
            true, false, 1000.0,
            false, false, true,
            120,
            true,
            0.0, 0.0,
            true, true,
            50.0, 5.0,
            false, None,
            "",
            &[],
            16.0,
            &[],
            &[],
            &[],
            &[],
            &[],
        );
        assert!(coord.is_empty());
    }

    #[test]
    fn test_loading_suppresses_insert_animation() {
        let mut coord = LinuxEditorAnimationCoordinator::new();
        let vt = EditorVisualTransaction {
            id: 1,
            kind: EditorAnimationKind::Insert,
            cause: writer_core::editor::EditorTransactionCause::Typing,
            old_text: "a".into(),
            new_text: "ab".into(),
            old_selection: writer_core::editor::EditorSelection::collapsed("a", 1),
            new_selection: writer_core::editor::EditorSelection::collapsed("ab", 2),
            inserted_range: Some((1, 2)),
            deleted_glyph_rects: None,
            insert_glyph_rects: Some(vec![]),
            reflow_glyph_rects: None,
            animation_mode: CoreAnimationMode::GlyphAnimation,
            cluster_rects: None,
            cluster_runs: None,
            hidden_visual_ranges: vec![],
            old_cursor_rect: None,
            new_cursor_rect: None,
            duration_ms: 160,
            coordinate_mode: writer_core::editor::VisualCoordinateMode::Baseline,
        };
        coord.process_transaction(
            &vt,
            true,
            false,
            true, false, false,
            None, None,
            50.0, 5.0, 20.0,
            true, false, 1000.0,
            false, false, true,
            120,
            true,
            0.0, 0.0,
            true, true,
            50.0, 5.0,
            false, None,
            "",
            &[],
            16.0,
            &[],
            &[],
            &[],
            &[],
            &[],
        );
        assert!(coord.is_empty());
    }
}
