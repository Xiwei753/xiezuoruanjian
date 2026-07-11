use std::time::Instant;

use writer_core::editor::{
    AnimationMode as CoreAnimationMode, CursorRect, EditorAnimationKind, EditorVisualTransaction,
    GlyphRect, ReflowGlyphRect,
};

pub(crate) use super::transaction_key::VisualTransactionKey;
pub(crate) use super::transaction_queue::{ActiveVisualTransaction, ActiveVisualTransactionQueue, VisualTransactionState};
pub(crate) use super::visual_payload::{VisualPayload, VisualRunSnapshot, ReflowRunSnapshot};
pub(crate) use super::animation_mode::AnimationMode;
pub(crate) use super::cursor_animation::{CursorAnimationPlan, CursorBlinkMode, CursorTransition};
pub(crate) use super::render_plan::{
    HiddenRangeInfo, StaticTextPlan, TextAnimationPlan, TextAnimationGlyphInfo,
    SelectionPreeditPlan, SelectionRange, PreeditRange,
    ImeUpdateKind, ImeUpdatePlan, RenderPlan,
};
pub(crate) use super::texture_cache::TextureCache;
pub(crate) use super::{insert_animation, delete_animation, reflow_animation};

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
        cursor_x: f64,
        cursor_y: f64,
        cursor_h: f64,
        editor_enabled: bool,
        has_selection: bool,
        viewport_height: f64,
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
    ) {
        if !typing_animation_enabled || is_scrolling {
            return;
        }

        let mode = AnimationMode::from_core(vt.animation_mode);
        if is_scrolling || is_loading || is_applying_format || is_applying_settings || !typing_animation_enabled {
            return;
        }
        if !mode.should_create_transaction() {
            return;
        }

        match vt.kind {
            EditorAnimationKind::Insert => {
                if let Some((range_start, range_end)) = vt.inserted_range {
                    let insert_len = range_end - range_start;
                    self.registry.map_ranges_for_insert(range_start, insert_len);

                    let key = self.alloc_key();
                    let reflow_ranges: Vec<(usize, usize)> = vt
                        .reflow_glyph_rects
                        .as_ref()
                        .map(|rects| {
                            rects.iter().map(|r| (r.byte_start, r.byte_end)).collect()
                        })
                        .unwrap_or_default();
                    let core_range_id = vt.hidden_visual_ranges.first().map(|r| r.id);

                    let has_reflow = !reflow_ranges.is_empty();
                    let payload = VisualPayload::from_insert_transaction(
                        vt.insert_glyph_rects.as_deref().unwrap_or(&[]),
                        vt.reflow_glyph_rects.as_deref().unwrap_or(&[]),
                        vt.inserted_range,
                        vt.old_cursor_rect.clone(),
                        vt.new_cursor_rect.clone(),
                        has_reflow,
                    );

                    self.registry.start_insert(
                        key,
                        core_range_id,
                        (range_start, range_end),
                        reflow_ranges,
                        mode,
                        vt.duration_ms,
                    );
                    self.vt_queue.enqueue(key, payload, mode, vt.duration_ms);
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
                for change in &changes {
                    if let writer_core::editor::EditorChange::Delete { index, text } = change {
                        let range_start = *index;
                        let range_end = range_start + text.len();
                        self.registry.start_delete(
                            key,
                            (range_start, range_end),
                            mode,
                            vt.duration_ms,
                        );
                    }
                }

                let payload = VisualPayload::from_delete_transaction(
                    vt.deleted_glyph_rects.as_deref().unwrap_or(&[]),
                    vt.old_cursor_rect.clone(),
                    vt.new_cursor_rect.clone(),
                );
                self.vt_queue.enqueue(key, payload, mode, vt.duration_ms);
            }
            EditorAnimationKind::Cursor => {}
        }
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use writer_core::editor::AnimationMode as CoreAnimationMode;

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
    fn test_visual_payload_insert_dispatch() {
        let payload = VisualPayload::from_insert_transaction(
            &[GlyphRect { x: 10.0, y: 5.0, w: 12.0, h: 20.0, char_: "a".into(), baseline_y: 20.0, byte_start: 0, byte_end: 1 }],
            &[],
            Some((0, 1)),
            None,
            None,
            false,
        );
        assert!(matches!(payload, VisualPayload::InsertRuns { .. }));
    }

    #[test]
    fn test_visual_payload_reflow_dispatch() {
        let payload = VisualPayload::from_insert_transaction(
            &[GlyphRect { x: 10.0, y: 5.0, w: 12.0, h: 20.0, char_: "a".into(), baseline_y: 20.0, byte_start: 0, byte_end: 1 }],
            &[ReflowGlyphRect { char_: "b".into(), byte_start: 1, byte_end: 2, old_x: 10.0, old_y: 5.0, old_baseline_y: 20.0, new_x: 22.0, new_y: 5.0, new_baseline_y: 20.0, w: 12.0, h: 20.0, line_index: 0 }],
            Some((0, 1)),
            None,
            None,
            true,
        );
        assert!(matches!(payload, VisualPayload::ReflowRuns { .. }));
    }

    #[test]
    fn test_visual_payload_delete_dispatch() {
        let payload = VisualPayload::from_delete_transaction(
            &[GlyphRect { x: 10.0, y: 5.0, w: 12.0, h: 20.0, char_: "a".into(), baseline_y: 20.0, byte_start: 0, byte_end: 1 }],
            None,
            None,
        );
        assert!(matches!(payload, VisualPayload::DeleteRuns { .. }));
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
    fn test_cancel_by_key() {
        let mut coord = LinuxEditorAnimationCoordinator::new();
        let key = make_key(1);
        coord.registry.start_insert(key, None, (5, 6), vec![], AnimationMode::GlyphAnimation, 100);
        let payload = VisualPayload::from_insert_transaction(&[], &[], Some((5, 6)), None, None, false);
        coord.vt_queue.enqueue(key, payload, AnimationMode::GlyphAnimation, 100);
        let cancelled = coord.cancel_by_key(key, "test_cancel");
        assert!(cancelled);
        assert!(coord.is_empty());
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
        );
        assert!(coord.is_empty());
    }
}

