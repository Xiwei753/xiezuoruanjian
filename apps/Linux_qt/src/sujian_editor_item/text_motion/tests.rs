//! text_motion.rs 的单元测试（从生产文件拆出，保持原有断言）。

//! Issue #919：把局部文字动画的核心不变量固化成回归测试。
//!
//! 这些测试只验证 `mask_rect` / `conceal_clip` / `SliceMotion::sample` 的
//! 几何语义，不构造 Qt 运行时。它们对应历次复核修正的实际显示错误：
//! 连续编辑从当前可见画面接续、遮罩按原始 clip 边界收拢、Shift 只位移不缩放。
//!
//! 复核 10 追加：直接驱动 `TextMotion::begin_or_retarget`，用构造出来的
//! `EditorEditResult` + 旧/新 `EditorLayoutSnapshot` 验证「按 patch 分类
//! Reveal / Conceal / Shift」「连续编辑从 last_drawn 接续」「同 byte_range
//! 不同 shaping 必须旧退场 + 新露出」，不构造 Qt 运行时。

use super::super::layout_snapshot::{LayoutRevision, LineClusterSnapshot, PreparedLineSnapshot};
use crate::editor::layout::chunked_vec::ChunkedSlotMap;
use crate::editor::layout::{CaretAffinity, ParagraphHeightIndex, ParagraphId};
use writer_core::editor::{
    DisplayPatch, EditorContentDelta, EditorEditResult, EditorOperationKind, EditorRevision,
    EditorSelection, EditorTransactionCause, Utf8ByteRange,
};

use super::*;

fn rect(x: f64, y: f64, w: f64, h: f64) -> LocalRect {
    LocalRect { x, y, w, h }
}

fn snap_ref(paragraph_id: u64, uv: LocalRect) -> SnapshotRef {
    SnapshotRef {
        snapshot_id: LineSnapshotId::new(1, paragraph_id, 0),
        source_uv: uv,
    }
}

fn identity() -> ShapingIdentity {
    ShapingIdentity::default()
}

fn make_motion(slices: Vec<SliceMotion>) -> TextMotion {
    TextMotion {
        slices,
        started_ms: 0.0,
        duration_ms: 100,
        session: 1,
        target_revision: 2,
        viewport: rect(0.0, 0.0, 1000.0, 1000.0),
        static_exclusions: Vec::new(),
        required_textures: Vec::new(),
        caret_from: None,
        caret_to: None,
        paragraph_moves: Vec::new(),
    }
}

fn visual(paragraph_id: u64, uv: LocalRect, rect: LocalRect) -> VisualSlice {
    VisualSlice {
        source: snap_ref(paragraph_id, uv),
        rect,
    }
}

#[test]
fn mask_rect_uses_direction_edge() {
    let at = rect(0.0, 5.0, 20.0, 10.0);
    assert_eq!(
        mask_rect(&at, ClipDirection::LeftToRight, 8.0),
        rect(0.0, 5.0, 8.0, 10.0)
    );
    assert_eq!(
        mask_rect(&at, ClipDirection::RightToLeft, 8.0),
        rect(12.0, 5.0, 8.0, 10.0)
    );
    assert_eq!(
        mask_rect(&at, ClipDirection::LeftToRight, 999.0),
        rect(0.0, 5.0, 20.0, 10.0)
    );
    assert_eq!(
        mask_rect(&at, ClipDirection::LeftToRight, -3.0),
        rect(0.0, 5.0, 0.0, 10.0)
    );
}

#[test]
fn conceal_clip_converges_from_start_clip_boundary() {
    let start = rect(10.0, 2.0, 8.0, 10.0);
    assert_eq!(conceal_clip(&start, ClipDirection::LeftToRight, 8.0), start);
    assert_eq!(
        conceal_clip(&start, ClipDirection::LeftToRight, 3.0),
        rect(10.0, 2.0, 3.0, 10.0)
    );
    assert_eq!(
        conceal_clip(&start, ClipDirection::LeftToRight, 0.0),
        rect(10.0, 2.0, 0.0, 10.0)
    );
    // RightToLeft 固定右边界 (x+w=18)，向左收拢。
    assert_eq!(
        conceal_clip(&start, ClipDirection::RightToLeft, 3.0),
        rect(15.0, 2.0, 3.0, 10.0)
    );
}

/// 旧字 target 与动画遮罩必须有实际非零交集，否则 `scene_graph_renderer`
/// 在 `iw <= 0` 时 `continue`，屏幕上没有任何退场像素，测试会把空画面
/// 误当正确动画。这里构造互相有覆盖、但遮罩左边缘不同于完整旧字左边缘的
/// 几何：旧图 target=40..60，上一帧原始 start_clip=50..58，交集宽 8。
fn conceal_with_start_clip(direction: ClipDirection) -> TextMotion {
    let old = visual(3, rect(0.0, 0.0, 20.0, 10.0), rect(40.0, 0.0, 20.0, 10.0));
    let start_clip = rect(50.0, 0.0, 8.0, 10.0);
    make_motion(vec![SliceMotion::Conceal {
        old,
        direction,
        start_clip,
        byte_range: (0, 3),
        shaping_identity: identity(),
    }])
}

/// target 与 mask 的水平交集宽度（>0 表示屏幕上确有退场像素）。
fn intersect_width(target_x: f64, target_w: f64, clip: &LocalRect) -> f64 {
    let x0 = clip.x.max(target_x);
    let x1 = (clip.x + clip.w).min(target_x + target_w);
    (x1 - x0).max(0.0)
}

#[test]
fn conceal_sample_continues_from_start_clip_not_old_rect() {
    // Backspace → LeftToRight：左边缘固定，从右向左吞。
    let motion = conceal_with_start_clip(ClipDirection::LeftToRight);

    // progress=0：遮罩完整继承 start_clip，与 target 有非零交集。
    let first = motion.sample(0.0);
    let overlay = first.overlays.first().expect("conceal overlay");
    assert_eq!((overlay.x, overlay.w), (40.0, 20.0)); // target=40..60
    assert_eq!((overlay.clip_rect.x, overlay.clip_rect.w), (50.0, 8.0)); // mask=50..58
    assert_eq!(
        intersect_width(overlay.x, overlay.w, &overlay.clip_rect),
        8.0
    );

    // progress=0.5：左边缘固定 50，宽度缩小，仍与 target 有实际非零交集。
    let mid = motion.sample(50.0);
    let overlay = mid.overlays.first().expect("conceal overlay");
    assert_eq!(overlay.clip_rect.x, 50.0);
    assert!(overlay.clip_rect.w > 0.0 && overlay.clip_rect.w < 8.0);
    assert!(intersect_width(overlay.x, overlay.w, &overlay.clip_rect) > 0.0);
}

#[test]
fn conceal_sample_right_to_left_continues_from_start_clip() {
    // Forward Delete → RightToLeft：右边缘固定，从左向右吞。
    let motion = conceal_with_start_clip(ClipDirection::RightToLeft);

    // progress=0：遮罩完整继承 start_clip，与 target 有非零交集。
    let first = motion.sample(0.0);
    let overlay = first.overlays.first().expect("conceal overlay");
    assert_eq!((overlay.x, overlay.w), (40.0, 20.0)); // target=40..60
    assert_eq!((overlay.clip_rect.x, overlay.clip_rect.w), (50.0, 8.0)); // mask=50..58
    assert_eq!(
        intersect_width(overlay.x, overlay.w, &overlay.clip_rect),
        8.0
    );

    // progress=0.5：右边缘固定 58，宽度缩小，仍与 target 有实际非零交集。
    let mid = motion.sample(50.0);
    let overlay = mid.overlays.first().expect("conceal overlay");
    assert_eq!(overlay.clip_rect.x + overlay.clip_rect.w, 58.0);
    assert!(overlay.clip_rect.w > 0.0 && overlay.clip_rect.w < 8.0);
    assert!(intersect_width(overlay.x, overlay.w, &overlay.clip_rect) > 0.0);
}

#[test]
fn conceal_sample_narrows_strictly_within_start_clip() {
    // Issue #919：Conceal 必须从上一帧可见 clip 持续收缩，且永不越过已显示边界。
    // 两种方向都保留各自固定的一侧边缘，避免下笔把可见部分瞬移到旧矩形另一边。
    let start = rect(50.0, 0.0, 8.0, 10.0);
    for direction in [ClipDirection::LeftToRight, ClipDirection::RightToLeft] {
        let motion = conceal_with_start_clip(direction);
        let mut prev_width = start.w;
        for step in 1..8 {
            let frame = motion.sample(step as f64 * 10.0);
            let overlay = frame.overlays.first().expect("conceal overlay");
            assert!(
                overlay.clip_rect.w < prev_width,
                "遮罩必须严格收缩: {} < {}",
                overlay.clip_rect.w,
                prev_width
            );
            assert!(overlay.clip_rect.w >= 0.0);
            prev_width = overlay.clip_rect.w;
            match direction {
                ClipDirection::LeftToRight => {
                    assert_eq!(overlay.clip_rect.x, start.x);
                    assert!(overlay.clip_rect.x + overlay.clip_rect.w <= start.x + start.w);
                }
                ClipDirection::RightToLeft => {
                    assert_eq!(overlay.clip_rect.x + overlay.clip_rect.w, start.x + start.w);
                    assert!(overlay.clip_rect.x >= start.x);
                }
            }
        }
    }
}

#[test]
fn reveal_sample_moves_from_start_clip_and_fixes_size() {
    let new = visual(9, rect(0.0, 0.0, 20.0, 10.0), rect(40.0, 0.0, 20.0, 10.0));
    let start_clip = rect(0.0, 0.0, 5.0, 10.0);
    let motion = make_motion(vec![SliceMotion::Reveal {
        new,
        direction: ClipDirection::LeftToRight,
        start_clip,
        byte_range: (0, 3),
        shaping_identity: identity(),
    }]);

    let first = motion.sample(0.0);
    let overlay = first.overlays.first().expect("reveal overlay");
    // 从上一帧已提交位置起步，不从 new.rect 直接开始。
    assert_eq!(overlay.x, 0.0);
    assert_eq!((overlay.w, overlay.h), (20.0, 10.0));
    assert_eq!(overlay.clip_rect.w, 5.0);

    let mid = motion.sample(50.0);
    let overlay = mid.overlays.first().expect("reveal overlay");
    assert!(overlay.x > 0.0 && overlay.x < 40.0);
    assert_eq!((overlay.w, overlay.h), (20.0, 10.0));
    assert!(overlay.clip_rect.w > 5.0 && overlay.clip_rect.w <= 20.0);
}

#[test]
fn shift_sample_keeps_new_size_and_only_moves() {
    let from = rect(100.0, 0.0, 5.0, 12.0);
    let to = rect(140.0, 3.0, 30.0, 20.0);
    let new = visual(7, rect(0.0, 0.0, 30.0, 20.0), to);
    let motion = make_motion(vec![SliceMotion::Shift {
        new,
        from,
        to,
        byte_range: (0, 3),
        shaping_identity: identity(),
    }]);

    let first = motion.sample(0.0);
    let overlay = first.overlays.first().expect("shift overlay");
    assert_eq!((overlay.x, overlay.y), (from.x, from.y));
    // 宽高固定为 new 版目标尺寸，绝不用 from/to 插值缩放字形。
    assert_eq!((overlay.w, overlay.h), (to.w, to.h));

    let mid = motion.sample(50.0);
    let overlay = mid.overlays.first().expect("shift overlay");
    assert_eq!((overlay.w, overlay.h), (to.w, to.h));
    assert!(overlay.x > from.x && overlay.x < to.x);
    assert!(overlay.y > from.y && overlay.y < to.y);
}

#[test]
fn paragraph_move_offset_is_zero_at_end() {
    let pm = ParagraphMove {
        paragraph_id: 3,
        from_y: 20.0,
        to_y: 0.0,
    };
    assert!((pm.offset_at(0.0) - 20.0).abs() < 1e-9);
    assert!(pm.offset_at(1.0).abs() < 1e-9);
}

#[test]
fn source_snapshot_ids_are_deduplicated() {
    let shared = LineSnapshotId::new(1, 1, 0);
    let slice = |uv: LocalRect| VisualSlice {
        source: SnapshotRef {
            snapshot_id: shared,
            source_uv: uv,
        },
        rect: rect(0.0, 0.0, 10.0, 10.0),
    };
    let motion = make_motion(vec![
        SliceMotion::Reveal {
            new: slice(rect(0.0, 0.0, 10.0, 10.0)),
            direction: ClipDirection::LeftToRight,
            start_clip: rect(0.0, 0.0, 0.0, 10.0),
            byte_range: (0, 1),
            shaping_identity: identity(),
        },
        SliceMotion::Conceal {
            old: slice(rect(10.0, 0.0, 10.0, 10.0)),
            direction: ClipDirection::LeftToRight,
            start_clip: rect(0.0, 0.0, 10.0, 10.0),
            byte_range: (1, 2),
            shaping_identity: identity(),
        },
    ]);
    assert_eq!(motion.source_snapshot_ids(), vec![shared]);
}

#[test]
fn motion_keeps_old_and_new_sources_distinct() {
    // Issue #919：同一 byte_range 的旧 Conceal 与新 Reveal 必须各自持有旧/新快照，
    // 不能被字节位置合并成一个源；这也是移除逐字身份匹配后仍不允许重影的前提。
    let old = visual(1, rect(0.0, 0.0, 10.0, 10.0), rect(40.0, 0.0, 10.0, 10.0));
    let new = visual(2, rect(0.0, 0.0, 10.0, 10.0), rect(40.0, 0.0, 10.0, 10.0));
    let motion = make_motion(vec![
        SliceMotion::Conceal {
            old,
            direction: ClipDirection::LeftToRight,
            start_clip: rect(40.0, 0.0, 10.0, 10.0),
            byte_range: (0, 1),
            shaping_identity: identity(),
        },
        SliceMotion::Reveal {
            new,
            direction: ClipDirection::LeftToRight,
            start_clip: rect(40.0, 0.0, 0.0, 10.0),
            byte_range: (0, 1),
            shaping_identity: identity(),
        },
    ]);
    let ids = motion.source_snapshot_ids();
    assert_eq!(ids.len(), 2, "旧/新画面必须各自持有快照");
    assert_ne!(ids[0], ids[1]);
}

// ── 复核 10：begin_or_retarget 端到端分类回归 ────────────────────────

fn byte_range(start: usize, end: usize) -> Utf8ByteRange {
    match Utf8ByteRange::from_values(start, end) {
        Some(range) => range,
        None => panic!("invalid byte range {start}..{end}"),
    }
}

fn cluster(byte_start: usize, byte_end: usize, x: f64, w: f64) -> LineClusterSnapshot {
    LineClusterSnapshot {
        byte_start,
        byte_end,
        // 行局部物理像素；dpr=1、visual_x=0 时文档 x 就等于此 x。
        source_rect: rect(x, 0.0, w, 10.0),
        shaping_identity: ShapingIdentity::default(),
    }
}

/// 造一条只含指定 cluster 的测试视觉行：段首文档 y = `top`，`visual_x = 0`。
fn test_line(
    ordinal: u32,
    byte_start: usize,
    clusters: Vec<LineClusterSnapshot>,
    top: f64,
) -> PreparedLineSnapshot {
    let mut line =
        PreparedLineSnapshot::stub_for_tests(ordinal as usize, top, byte_start, clusters);
    line.visual_x = 0.0;
    line
}

fn snapshot(lines: Vec<PreparedLineSnapshot>) -> EditorLayoutSnapshot {
    // Issue #919 复核 11：把 revision 写回每条 line.id，使 LineSnapshotId
    // 携带真实布局版本而非 stub_for_tests 的固定 (0,0,ordinal)。
    // ordinal=0 时旧值恰好等于 LineSnapshotId::none()（无纹理哨兵），
    // old/new 两版会被误认成同一 SnapshotRef；此处用 revision.0 作为
    // layout_revision、stable_paragraph_id 作为 paragraph_id、原 ordinal
    // 作为 visual_line_ordinal，保证 id.is_none()==false 且 old/new 不同。
    let revision = LayoutRevision::next();
    let line_snapshots = lines
        .into_iter()
        .map(|mut line| {
            line.id = LineSnapshotId::new(
                revision.0,
                line.stable_paragraph_id,
                line.id.visual_line_ordinal,
            );
            line
        })
        .collect();
    EditorLayoutSnapshot {
        revision,
        line_snapshots,
        paragraph_height_index: None,
        paragraph_slot_map: None,
        paragraph_padding: 0.0,
        caret_rect: None,
        caret_rect_doc: None,
        caret_affinity: CaretAffinity::Upstream,
    }
}

/// 造一个带真实段落几何索引的测试快照。
///
/// `snapshot()` 只填 `line_snapshots`，不含 `paragraph_height_index` /
/// `paragraph_slot_map`；整体段落位移 `collect_paragraph_moves` 依赖这两项
/// 按稳定 ParagraphId 查 `(段首 y, 段高)`。`heights[i]`/`ids[i]` 对应 slot i，
/// 段首 y = 前 i 段高度之和（`paragraph_padding = 0`）。
fn snapshot_with_paragraph_index(
    lines: Vec<PreparedLineSnapshot>,
    heights: Vec<f64>,
    ids: Vec<u64>,
) -> EditorLayoutSnapshot {
    let mut snapshot = snapshot(lines);
    snapshot.paragraph_height_index = Some(std::sync::Arc::new(
        ParagraphHeightIndex::from_heights(heights),
    ));
    snapshot.paragraph_slot_map = Some(std::sync::Arc::new(ChunkedSlotMap::from_ids(
        ids.into_iter().map(ParagraphId::from_u64).collect(),
    )));
    snapshot
}

#[allow(clippy::too_many_arguments)]
fn make_edit(
    old_text: &str,
    new_text: &str,
    old_head: usize,
    new_head: usize,
    replace: (usize, usize),
    inserted: (usize, usize),
    inserted_text: &str,
) -> EditorEditResult {
    let base = EditorRevision::initial();
    let new_rev = base.next();
    EditorEditResult {
        transaction_id: 1,
        base_revision: base,
        new_revision: new_rev,
        display_patches: vec![DisplayPatch {
            base_revision: base,
            new_revision: new_rev,
            replace_byte_range: byte_range(replace.0, replace.1),
            inserted_byte_range: byte_range(inserted.0, inserted.1),
            inserted_text: inserted_text.to_string(),
            resulting_selection_byte_range: byte_range(inserted.1, inserted.1),
        }],
        old_selection: EditorSelection::collapsed(old_text, old_head),
        new_selection: EditorSelection::collapsed(new_text, new_head),
        cause: EditorTransactionCause::Typing,
        operation_kind: EditorOperationKind::Insert,
        offset_map: None,
        content_delta: EditorContentDelta::default(),
        changes: Vec::new(),
        before_text_sha256: String::new(),
        after_text_sha256: String::new(),
    }
}

/// 造一个含多笔 `display_patches` 的事务。
///
/// 每笔为 `((旧起, 旧止), (新起, 新止), 新文本)`；`replace` 是旧坐标、
/// `inserted` 是新（最终）坐标，与 Core `DisplayPatch` 语义一致。
fn make_edit_from_patches(
    old_text: &str,
    new_text: &str,
    old_head: usize,
    new_head: usize,
    patches: Vec<((usize, usize), (usize, usize), &str)>,
) -> EditorEditResult {
    let base = EditorRevision::initial();
    let new_rev = base.next();
    let display_patches = patches
        .into_iter()
        .map(
            |((rep_start, rep_end), (ins_start, ins_end), text)| DisplayPatch {
                base_revision: base,
                new_revision: new_rev,
                replace_byte_range: byte_range(rep_start, rep_end),
                inserted_byte_range: byte_range(ins_start, ins_end),
                inserted_text: text.to_string(),
                resulting_selection_byte_range: byte_range(ins_end, ins_end),
            },
        )
        .collect();
    EditorEditResult {
        transaction_id: 1,
        base_revision: base,
        new_revision: new_rev,
        display_patches,
        old_selection: EditorSelection::collapsed(old_text, old_head),
        new_selection: EditorSelection::collapsed(new_text, new_head),
        cause: EditorTransactionCause::Typing,
        operation_kind: EditorOperationKind::Insert,
        offset_map: None,
        content_delta: EditorContentDelta::default(),
        changes: Vec::new(),
        before_text_sha256: String::new(),
        after_text_sha256: String::new(),
    }
}

fn viewport() -> LocalRect {
    rect(0.0, 0.0, 1000.0, 1000.0)
}

fn retarget(
    edit: &EditorEditResult,
    old_layout: Option<&EditorLayoutSnapshot>,
    new_layout: &EditorLayoutSnapshot,
    last_drawn: Option<&LocalDrawnFrame>,
    session: u64,
) -> TextMotion {
    match TextMotion::begin_or_retarget(
        edit,
        old_layout,
        new_layout,
        last_drawn,
        0.0,
        100,
        session,
        viewport(),
    ) {
        Some(motion) => motion,
        None => panic!("expected TextMotion, got None"),
    }
}

#[test]
fn cursor_only_edit_without_content_returns_none() {
    // 空 patch（无插入/替换）：只有光标移动，不做局部动画。
    let new_layout = snapshot(vec![]);
    let edit = make_edit("", "", 0, 0, (0, 0), (0, 0), "");
    assert!(
        TextMotion::begin_or_retarget(&edit, None, &new_layout, None, 0.0, 100, 1, viewport(),)
            .is_none()
    );
}

#[test]
fn inserted_patch_classifies_as_reveal_from_zero_width() {
    let new_layout = snapshot(vec![test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0)]);
    let edit = make_edit("", "甲", 0, 3, (0, 0), (0, 3), "甲");
    let motion = retarget(&edit, None, &new_layout, None, 7);

    assert_eq!(motion.slices.len(), 1);
    match &motion.slices[0] {
        SliceMotion::Reveal {
            new,
            direction,
            start_clip,
            ..
        } => {
            assert_eq!(new.rect, rect(0.0, 0.0, 20.0, 10.0));
            assert_eq!(*direction, ClipDirection::LeftToRight);
            // 无上一帧接续：全新字从零宽露出，位置就是自身 target。
            assert_eq!(*start_clip, rect(0.0, 0.0, 0.0, 10.0));
        }
        other => panic!("expected Reveal, got {other:?}"),
    }
    assert_eq!(
        motion.source_snapshot_ids(),
        vec![new_layout.line_snapshots[0].id]
    );
}

#[test]
fn backspace_conceals_with_left_edge_fixed() {
    let old_layout = snapshot(vec![test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0)]);
    let new_layout = snapshot(vec![]);
    // Backspace：光标从 3 左移到 0 → 左边缘固定、从右往左吞。
    let edit = make_edit("甲", "", 3, 0, (0, 3), (0, 0), "");
    let motion = retarget(&edit, Some(&old_layout), &new_layout, None, 7);

    assert_eq!(motion.slices.len(), 1);
    match &motion.slices[0] {
        SliceMotion::Conceal {
            old,
            direction,
            start_clip,
            ..
        } => {
            assert_eq!(old.rect, rect(0.0, 0.0, 20.0, 10.0));
            assert_eq!(*direction, ClipDirection::LeftToRight);
            // 无在途旧图：从旧字完整画面开始吞。
            assert_eq!(*start_clip, rect(0.0, 0.0, 20.0, 10.0));
        }
        other => panic!("expected Conceal, got {other:?}"),
    }
}

#[test]
fn forward_delete_conceals_with_right_edge_fixed() {
    let old_layout = snapshot(vec![test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0)]);
    let new_layout = snapshot(vec![]);
    // Forward Delete：光标停在 0（未左移）→ 右边缘固定、从左往右吞。
    let edit = make_edit("甲", "", 0, 0, (0, 3), (0, 0), "");
    let motion = retarget(&edit, Some(&old_layout), &new_layout, None, 7);

    match &motion.slices[0] {
        SliceMotion::Conceal { direction, .. } => {
            assert_eq!(*direction, ClipDirection::RightToLeft);
        }
        other => panic!("expected Conceal, got {other:?}"),
    }
}

#[test]
fn insert_before_existing_text_shifts_unchanged_cluster() {
    let old_layout = snapshot(vec![test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0)]);
    let new_layout = snapshot(vec![test_line(
        0,
        0,
        vec![cluster(0, 3, 0.0, 20.0), cluster(3, 6, 20.0, 20.0)],
        0.0,
    )]);
    // 在段首插入"乙"：新字 Reveal，"甲"未改但被挤到 x=20 → Shift。
    let edit = make_edit("甲", "乙甲", 0, 3, (0, 0), (0, 3), "乙");
    let motion = retarget(&edit, Some(&old_layout), &new_layout, None, 7);

    assert_eq!(motion.slices.len(), 2);
    assert!(matches!(motion.slices[0], SliceMotion::Reveal { .. }));
    match &motion.slices[1] {
        SliceMotion::Shift { from, to, .. } => {
            assert_eq!(*from, rect(0.0, 0.0, 20.0, 10.0));
            assert_eq!(*to, rect(20.0, 0.0, 20.0, 10.0));
        }
        other => panic!("expected Shift for unchanged 甲, got {other:?}"),
    }
}

fn drawn_reveal(
    line_id: LineSnapshotId,
    mask_clip: LocalRect,
    shaping_identity: ShapingIdentity,
) -> LocalDrawnFrame {
    LocalDrawnFrame {
        session: 7,
        revision: 1,
        slices: vec![DrawnSlice {
            slice: VisualSlice {
                source: SnapshotRef {
                    snapshot_id: line_id,
                    source_uv: rect(0.0, 0.0, 20.0, 10.0),
                },
                rect: rect(0.0, 0.0, 20.0, 10.0),
            },
            visible_width: mask_clip.w,
            clip_rect: mask_clip,
            mask_clip,
            motion_kind: MotionKind::Reveal,
            byte_range: (0, 3),
            shaping_identity,
            direction: ClipDirection::LeftToRight,
        }],
    }
}

#[test]
fn continuous_reveal_inherits_previous_mask_clip() {
    let new_layout = snapshot(vec![test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0)]);
    let line_id = new_layout.line_snapshots[0].id;
    // 上一帧同一字形吐到一半（遮罩 0..10）。
    let last = drawn_reveal(
        line_id,
        rect(0.0, 0.0, 10.0, 10.0),
        ShapingIdentity::default(),
    );
    let edit = make_edit("", "甲", 0, 3, (0, 0), (0, 3), "甲");
    let motion = retarget(&edit, None, &new_layout, Some(&last), 7);

    assert_eq!(motion.slices.len(), 1);
    match &motion.slices[0] {
        SliceMotion::Reveal { start_clip, .. } => {
            // 从上一帧原始 mask clip 继续吐，不退回零宽。
            assert_eq!(*start_clip, rect(0.0, 0.0, 10.0, 10.0));
        }
        other => panic!("expected Reveal, got {other:?}"),
    }
}

#[test]
fn consecutive_typing_continues_unchanged_cluster_reveal() {
    // Issue #919：连续输入——上一帧“甲”正吐到一半（遮罩 0..10），本帧在其后输入
    // “乙”。“甲”在本次编辑里是 patch 外未改区间，必须从上一帧已显示的
    // `mask_clip` 继续吐到满宽，而不是因为不在本次插入区间就退回零宽/重新露出。
    // 这覆盖 `begin_or_retarget` 中「未改区间接续上一帧 Reveal」的分支：用偏移
    // 映射定位旧 byte range，再从 last_drawn 的同一 shaping 画面接续。
    let old_layout = snapshot(vec![test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0)]);
    let new_layout = snapshot(vec![test_line(
        0,
        0,
        vec![cluster(0, 3, 0.0, 20.0), cluster(3, 6, 20.0, 20.0)],
        0.0,
    )]);
    let old_id = old_layout.line_snapshots[0].id;
    let new_id = new_layout.line_snapshots[0].id;
    // 上一帧“甲”Reveal 只露出左半 (0..10)，源是上一版排版行图。
    let last = drawn_reveal(
        old_id,
        rect(0.0, 0.0, 10.0, 10.0),
        ShapingIdentity::default(),
    );

    // 在“甲”之后输入“乙”：插入 patch 为 3..6，“甲”0..3 属未改区间。
    let edit = make_edit("甲", "甲乙", 3, 6, (3, 3), (3, 6), "乙");
    let motion = retarget(&edit, Some(&old_layout), &new_layout, Some(&last), 7);

    // “甲”未改但继续 Reveal（从半宽接续），“乙”新插入 Reveal（从零宽）。
    let reveal_jia = motion
        .slices
        .iter()
        .find(|s| s.byte_range() == (0, 3) && s.motion_kind() == MotionKind::Reveal)
        .expect("“甲”应继续 Reveal");
    match reveal_jia {
        SliceMotion::Reveal {
            new, start_clip, ..
        } => {
            assert_eq!(new.source.snapshot_id, new_id);
            assert_eq!(
                *start_clip,
                rect(0.0, 0.0, 10.0, 10.0),
                "必须从上一帧半宽继续吐，不退回零宽"
            );
        }
        other => panic!("expected Reveal for 甲, got {other:?}"),
    }
    let reveal_yi = motion
        .slices
        .iter()
        .find(|s| s.byte_range() == (3, 6) && s.motion_kind() == MotionKind::Reveal)
        .expect("“乙”应 Reveal");
    match reveal_yi {
        SliceMotion::Reveal {
            new, start_clip, ..
        } => {
            assert_eq!(new.source.snapshot_id, new_id);
            assert_eq!(*start_clip, rect(20.0, 0.0, 0.0, 10.0));
        }
        other => panic!("expected Reveal for 乙, got {other:?}"),
    }
    assert_eq!(motion.slices.len(), 2);
}

#[test]
fn same_byte_range_different_shaping_retires_old_and_reveals_new() {
    // Issue #919 复核 11：独立构造 old/new 排版画面，两份不同的行纹理 ID。
    // 上一帧旧字纹理来源（old_id）与本轮新字（new_id）各自独立，模拟真实
    // 旧/新可渲染视觉资源，而非共用同一份新行纹理。
    let old_layout = snapshot(vec![test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0)]);
    let new_layout = snapshot(vec![test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0)]);
    let old_id = old_layout.line_snapshots[0].id;
    let new_id = new_layout.line_snapshots[0].id;
    assert_ne!(old_id, new_id);
    // 上一帧露的是别的字形（glyph hash 不同），用旧排版画面的纹理 ID。
    let old_shaping = ShapingIdentity {
        glyph_indexes_hash: 42,
        ..ShapingIdentity::default()
    };
    let last = drawn_reveal(old_id, rect(0.0, 0.0, 10.0, 10.0), old_shaping);
    let edit = make_edit("", "甲", 0, 3, (0, 0), (0, 3), "甲");
    let motion = retarget(&edit, None, &new_layout, Some(&last), 7);

    assert_eq!(motion.slices.len(), 2);
    let source_ids = motion.source_snapshot_ids();
    assert!(source_ids.contains(&old_id));
    assert!(source_ids.contains(&new_id));
    match &motion.slices[0] {
        SliceMotion::Conceal {
            old,
            start_clip,
            direction,
            ..
        } => {
            assert_eq!(old.source.snapshot_id, old_id);
            // 旧图从当前可见遮罩继续退场，沿用原方向。
            assert_eq!(*start_clip, rect(0.0, 0.0, 10.0, 10.0));
            assert_eq!(*direction, ClipDirection::LeftToRight);
        }
        other => panic!("expected old Conceal first, got {other:?}"),
    }
    match &motion.slices[1] {
        SliceMotion::Reveal {
            new, start_clip, ..
        } => {
            assert_eq!(new.source.snapshot_id, new_id);
            // 新字形从零宽重新露出，不继承旧字宽度。
            assert_eq!(*start_clip, rect(0.0, 0.0, 0.0, 10.0));
        }
        other => panic!("expected new Reveal second, got {other:?}"),
    }
}

#[test]
fn replacement_patch_is_insert_and_replace_but_pure_insert_is_not_replace() {
    let insert = make_edit("", "甲", 0, 3, (0, 0), (0, 3), "甲");
    assert!(inserted_in_patch(&insert, (0, 3)));
    assert!(!replaced_in_patch(&insert, (0, 3)));

    let replace = make_edit("甲", "乙", 3, 3, (0, 3), (0, 3), "乙");
    assert!(inserted_in_patch(&replace, (0, 3)));
    assert!(replaced_in_patch(&replace, (0, 3)));
}

#[test]
fn new_to_old_byte_removes_insertion_delta_after_patch() {
    // "甲乙" → 段首插入"丙"：新坐标 3..6 映回旧坐标 0..3。
    let edit = make_edit("甲乙", "丙甲乙", 0, 3, (0, 0), (0, 3), "丙");
    assert_eq!(new_to_old_byte(&edit, 3), 0);
    assert_eq!(new_to_old_byte(&edit, 6), 3);
}

// ── 复核 12：旧图接续起点取上一帧“实际显示画面”的端到端回归 ──────────

/// 带非默认 shaping 指纹的 cluster：模拟同一 byte range 但字形/shaping 已变。
fn cluster_shaped(
    byte_start: usize,
    byte_end: usize,
    x: f64,
    w: f64,
    glyph_hash: u64,
) -> LineClusterSnapshot {
    LineClusterSnapshot {
        byte_start,
        byte_end,
        source_rect: rect(x, 0.0, w, 10.0),
        shaping_identity: ShapingIdentity {
            glyph_indexes_hash: glyph_hash,
            ..ShapingIdentity::default()
        },
    }
}

/// 上一帧真正提交显示的 `Shift` 片段：`slice.rect` 是中途插值位置，
/// `mask_clip` 是该位置的实际动画遮罩（Shift 不裁宽度，等于完整 rect）。
fn drawn_shift(line_id: LineSnapshotId, displayed: LocalRect) -> LocalDrawnFrame {
    LocalDrawnFrame {
        session: 7,
        revision: 1,
        slices: vec![DrawnSlice {
            slice: VisualSlice {
                source: SnapshotRef {
                    snapshot_id: line_id,
                    source_uv: rect(0.0, 0.0, 20.0, 10.0),
                },
                rect: displayed,
            },
            visible_width: displayed.w,
            clip_rect: displayed,
            mask_clip: displayed,
            motion_kind: MotionKind::Shift,
            byte_range: (0, 3),
            shaping_identity: identity(),
            direction: ClipDirection::LeftToRight,
        }],
    }
}

#[test]
fn shift_interrupted_by_shaping_change_conceals_from_last_shown_position() {
    // Issue #919 复核 7：上一帧“甲”正在 Shift（旧 0→新 20），本笔替换使
    // shaping 变化。退场必须从上一帧真正显示的中途位置 (x=10) 原地吞，
    // 而不是从旧 canonical (x=0) 或新目标 (x=20) 突然开始。
    let old_layout = snapshot(vec![test_line(
        0,
        0,
        vec![cluster_shaped(0, 3, 0.0, 20.0, 0)],
        0.0,
    )]);
    // 新布局含两块 cluster：
    // - “甲” byte 0..3：x=20、shaping 已变（glyph_hash=7）。
    // - “乙” byte 3..6：x=40、新插入字。
    // 注意：“甲”从旧 x=0 变成新 x=20 是人为构造的目标几何，用于验证中途
    // 位置接续；普通 LTR 文本在末尾追加“乙”不会把“甲”平移到 x=20。
    let new_layout = snapshot(vec![test_line(
        0,
        0,
        vec![
            cluster_shaped(0, 3, 20.0, 20.0, 7),
            cluster(3, 6, 40.0, 20.0),
        ],
        0.0,
    )]);
    let old_id = old_layout.line_snapshots[0].id;
    let new_id = new_layout.line_snapshots[0].id;
    assert_ne!(old_id, new_id);

    // 上一帧“甲”已经离开 x=0，插值显示在 x=10，源仍是旧排版行图。
    let last = drawn_shift(old_id, rect(10.0, 0.0, 20.0, 10.0));

    // 在“甲”之后插入“乙”，新排版把“甲”放到 x=20 且 shaping 已变。
    let edit = make_edit("甲", "甲乙", 3, 6, (3, 3), (3, 6), "乙");
    let motion = retarget(&edit, Some(&old_layout), &new_layout, Some(&last), 7);

    // 三条切片：旧“甲” Conceal + 新 shaping“甲” Reveal + 新插入“乙” Reveal。
    assert_eq!(motion.slices.len(), 3);
    let ids = motion.source_snapshot_ids();
    assert!(ids.contains(&old_id));
    assert!(ids.contains(&new_id));

    // 按 byte_range + motion_kind 定位，不依赖切片顺序。
    let conceal_jia = motion
        .slices
        .iter()
        .find(|s| s.byte_range() == (0, 3) && s.motion_kind() == MotionKind::Conceal)
        .expect("应有旧“甲” Conceal");
    let reveal_jia = motion
        .slices
        .iter()
        .find(|s| s.byte_range() == (0, 3) && s.motion_kind() == MotionKind::Reveal)
        .expect("应有新 shaping“甲” Reveal");
    let reveal_yi = motion
        .slices
        .iter()
        .find(|s| s.byte_range() == (3, 6) && s.motion_kind() == MotionKind::Reveal)
        .expect("应有新插入“乙” Reveal");

    match conceal_jia {
        SliceMotion::Conceal {
            old, start_clip, ..
        } => {
            assert_eq!(old.source.snapshot_id, old_id);
            // 起点是上一帧实际显示位置，不是 old_span.slice.rect (x=0)。
            assert_eq!(old.rect, rect(10.0, 0.0, 20.0, 10.0));
            assert_eq!(*start_clip, rect(10.0, 0.0, 20.0, 10.0));
        }
        other => panic!("expected Conceal, got {other:?}"),
    }
    match reveal_jia {
        SliceMotion::Reveal {
            new, start_clip, ..
        } => {
            assert_eq!(new.source.snapshot_id, new_id);
            assert_eq!(new.rect, rect(20.0, 0.0, 20.0, 10.0));
            // 新字形从零宽重新露出，不继承上一帧旧图位置。
            assert_eq!(*start_clip, rect(20.0, 0.0, 0.0, 10.0));
        }
        other => panic!("expected Reveal, got {other:?}"),
    }
    match reveal_yi {
        SliceMotion::Reveal {
            new, start_clip, ..
        } => {
            assert_eq!(new.source.snapshot_id, new_id);
            // 新插入的“乙”位于 x=40，从零宽开始露出。
            assert_eq!(new.rect, rect(40.0, 0.0, 20.0, 10.0));
            assert_eq!(*start_clip, rect(40.0, 0.0, 0.0, 10.0));
        }
        other => panic!("expected Reveal, got {other:?}"),
    }
}

#[test]
fn shift_continuation_uses_unclipped_current_rect() {
    // Issue #919 复核 15：上一帧 Shift 片段被视口裁过一部分时，`DrawnSlice`
    // 的 `clip_rect`（视口裁剪后）与 `mask_clip`（`sample()` 产生的完整插值
    // rect）不同。下一笔继续位移必须从 `mask_clip`（真实当前画面位置）接新
    // 目标，而不是从 `clip_rect` 的视口边缘——否则被裁掉的位移字形会把可见
    // 部分挪到视口边界，内容切片错位、在视口边缘跳变。
    let old_layout = snapshot(vec![test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0)]);
    let new_layout = snapshot(vec![test_line(
        0,
        0,
        vec![cluster(0, 3, 0.0, 20.0), cluster(3, 6, 20.0, 20.0)],
        0.0,
    )]);
    let old_id = old_layout.line_snapshots[0].id;

    // 上一帧“甲”已从 x=0 插值到 x=10（完整 rect 10..30），但被视口左边缘
    // 裁到 12..28：clip_rect != mask_clip。
    let last = frame_of(vec![DrawnSlice {
        slice: VisualSlice {
            source: SnapshotRef {
                snapshot_id: old_id,
                source_uv: rect(0.0, 0.0, 20.0, 10.0),
            },
            rect: rect(10.0, 0.0, 20.0, 10.0),
        },
        visible_width: 16.0,
        clip_rect: rect(12.0, 0.0, 16.0, 10.0),
        mask_clip: rect(10.0, 0.0, 20.0, 10.0),
        motion_kind: MotionKind::Shift,
        byte_range: (0, 3),
        shaping_identity: identity(),
        direction: ClipDirection::LeftToRight,
    }]);

    // 段首插入“乙”，未改的“甲”继续位移到 x=20。
    let edit = make_edit("甲", "乙甲", 0, 3, (0, 0), (0, 3), "乙");
    let motion = retarget(&edit, Some(&old_layout), &new_layout, Some(&last), 7);

    let shift = motion
        .slices
        .iter()
        .find(|s| s.motion_kind() == MotionKind::Shift)
        .expect("应有“甲” Shift 接续");
    match shift {
        SliceMotion::Shift { from, to, .. } => {
            assert_eq!(
                *from,
                rect(10.0, 0.0, 20.0, 10.0),
                "必须从上一帧完整插值 rect 接续，而不是视口裁剪后的 clip_rect"
            );
            assert_eq!(*to, rect(20.0, 0.0, 20.0, 10.0));
        }
        other => panic!("expected Shift, got {other:?}"),
    }
}

#[test]
fn deleting_half_revealed_glyph_continues_from_visible_clip() {
    // Issue #919 复核 6：刚吐到一半的字立即 Backspace，退场起点取上一帧
    // 已显示的 mask_clip（半宽），不先把半字补成整字再吞。
    let old_layout = snapshot(vec![test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0)]);
    let new_layout = snapshot(vec![]);
    let old_id = old_layout.line_snapshots[0].id;

    // 上一帧“甲”是 Reveal，只露出左半 (0..10)。
    let last = drawn_reveal(
        old_id,
        rect(0.0, 0.0, 10.0, 10.0),
        ShapingIdentity::default(),
    );

    // Backspace：光标从 3 左移到 0。
    let edit = make_edit("甲", "", 3, 0, (0, 3), (0, 0), "");
    let motion = retarget(&edit, Some(&old_layout), &new_layout, Some(&last), 7);

    assert_eq!(motion.slices.len(), 1);
    match &motion.slices[0] {
        SliceMotion::Conceal {
            old,
            start_clip,
            direction,
            ..
        } => {
            assert_eq!(old.source.snapshot_id, old_id);
            // 从已显示的半宽遮罩继续吞，而不是从整字 (w=20) 重新开始。
            assert_eq!(*start_clip, rect(0.0, 0.0, 10.0, 10.0));
            assert_eq!(*direction, ClipDirection::LeftToRight);
        }
        other => panic!("expected Conceal, got {other:?}"),
    }
}

// ── 复核 13：删除边界处未改文字不得被当成新字 Reveal ───────────────────

#[test]
fn backspace_does_not_reveal_unchanged_preceding_cluster() {
    // 正文“甲乙丙”，Backspace 删除“乙”（旧 3..6）。未改的“甲”仍占旧/新
    // 0..3，必须保持静态，不能被映成删除后坐标而重新露出。
    let old_layout = snapshot(vec![test_line(
        0,
        0,
        vec![
            cluster(0, 3, 0.0, 20.0),
            cluster(3, 6, 20.0, 20.0),
            cluster(6, 9, 40.0, 20.0),
        ],
        0.0,
    )]);
    let new_layout = snapshot(vec![test_line(
        0,
        0,
        vec![cluster(0, 3, 0.0, 20.0), cluster(3, 6, 20.0, 20.0)],
        0.0,
    )]);

    // Backspace：旧光标 6 → 新光标 3，删除“乙”3..6。
    let edit = make_edit("甲乙丙", "甲丙", 6, 3, (3, 6), (3, 3), "");
    let motion = retarget(&edit, Some(&old_layout), &new_layout, None, 7);

    assert!(
        motion
            .slices
            .iter()
            .all(|s| s.motion_kind() != MotionKind::Reveal),
        "未改的“甲”不应被 Reveal: {:?}",
        motion.slices
    );
    // 被删的“乙”退场；“丙”从左邻删除后左移到 x=20。
    let conceal = motion
        .slices
        .iter()
        .find(|s| s.motion_kind() == MotionKind::Conceal)
        .expect("应有“乙” Conceal");
    assert_eq!(conceal.byte_range(), (3, 6));
    let shift = motion
        .slices
        .iter()
        .find(|s| s.motion_kind() == MotionKind::Shift)
        .expect("应有“丙” Shift");
    match shift {
        SliceMotion::Shift { from, to, .. } => {
            assert_eq!(*from, rect(40.0, 0.0, 20.0, 10.0));
            assert_eq!(*to, rect(20.0, 0.0, 20.0, 10.0));
        }
        other => panic!("expected Shift, got {other:?}"),
    }
}

#[test]
fn forward_delete_does_not_reveal_unchanged_following_cluster() {
    // 正文“甲乙丙”，Forward Delete 删除“甲”（旧 0..3）。未改的“乙”“丙”
    // 左移，不得产生 Reveal。
    let old_layout = snapshot(vec![test_line(
        0,
        0,
        vec![
            cluster(0, 3, 0.0, 20.0),
            cluster(3, 6, 20.0, 20.0),
            cluster(6, 9, 40.0, 20.0),
        ],
        0.0,
    )]);
    let new_layout = snapshot(vec![test_line(
        0,
        0,
        vec![cluster(0, 3, 0.0, 20.0), cluster(3, 6, 20.0, 20.0)],
        0.0,
    )]);

    // Forward Delete：光标停在 0（未左移），删除“甲”0..3。
    let edit = make_edit("甲乙丙", "乙丙", 0, 0, (0, 3), (0, 0), "");
    let motion = retarget(&edit, Some(&old_layout), &new_layout, None, 7);

    assert!(
        motion
            .slices
            .iter()
            .all(|s| s.motion_kind() != MotionKind::Reveal),
        "删除后未改文字不应 Reveal: {:?}",
        motion.slices
    );
    let conceal = motion
        .slices
        .iter()
        .find(|s| s.motion_kind() == MotionKind::Conceal)
        .expect("应有“甲” Conceal");
    assert_eq!(conceal.byte_range(), (0, 3));
    // 「乙」「丙」未改、同视觉行、位移相同（均左移 20）、字节连续、源 UV 连续，
    // 按 #919 连续片段分组合并成一条 Shift（byte_range 0..6），不得产生 Reveal。
    let shifts: Vec<_> = motion
        .slices
        .iter()
        .filter(|s| s.motion_kind() == MotionKind::Shift)
        .collect();
    assert_eq!(
        shifts.len(),
        1,
        "乙丙应合并为一条连续 Shift: {:?}",
        motion.slices
    );
    match shifts[0] {
        SliceMotion::Shift {
            new,
            from,
            to,
            byte_range,
            ..
        } => {
            assert_eq!(*byte_range, (0, 6));
            assert_eq!((from.x, from.y, from.w, from.h), (20.0, 0.0, 40.0, 10.0));
            assert_eq!((to.x, to.y, to.w, to.h), (0.0, 0.0, 40.0, 10.0));
            assert_eq!(new.source.snapshot_id, new_layout.line_snapshots[0].id);
        }
        other => panic!("expected Shift, got {other:?}"),
    }
}

// ── 复核 14：在途旧图接续与旧/新画面归属的补充回归 ─────────────────────

fn frame_of(slices: Vec<DrawnSlice>) -> LocalDrawnFrame {
    LocalDrawnFrame {
        session: 7,
        revision: 1,
        slices,
    }
}

fn drawn_slice(
    source: SnapshotRef,
    displayed: LocalRect,
    mask_clip: LocalRect,
    motion_kind: MotionKind,
    direction: ClipDirection,
    byte_range: (usize, usize),
) -> DrawnSlice {
    DrawnSlice {
        slice: VisualSlice {
            source,
            rect: displayed,
        },
        visible_width: mask_clip.w,
        clip_rect: mask_clip,
        mask_clip,
        motion_kind,
        byte_range,
        shaping_identity: identity(),
        direction,
    }
}

#[test]
fn previous_deletion_conceal_continues_into_next_edit() {
    // Issue #919 复核 3：上一笔删除但仍在途的旧 Conceal 已不在当前 Core 正文，
    // 不会进入本轮 old_spans；必须从 last_drawn 直接沿用其画面继续吞到 0，
    // 否则连续按 Backspace 时，上一笔还可见的半个旧字会直接从新计划消失。
    let old_layout = snapshot(vec![test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0)]);
    let new_layout = snapshot(vec![]);
    let jia_id = old_layout.line_snapshots[0].id;

    // 上一帧：更早一笔删除的“乙”（独立旧排版行图，已不在本轮正文）正吞到一半。
    let yi_source = SnapshotRef {
        snapshot_id: LineSnapshotId::new(999, 1, 0),
        source_uv: rect(20.0, 0.0, 20.0, 10.0),
    };
    let last = frame_of(vec![drawn_slice(
        yi_source,
        rect(20.0, 0.0, 20.0, 10.0),
        rect(20.0, 0.0, 10.0, 10.0),
        MotionKind::Conceal,
        ClipDirection::LeftToRight,
        (3, 6),
    )]);

    // 本笔 Backspace 删除“甲”。
    let edit = make_edit("甲", "", 3, 0, (0, 3), (0, 0), "");
    let motion = retarget(&edit, Some(&old_layout), &new_layout, Some(&last), 7);

    let conceals: Vec<_> = motion
        .slices
        .iter()
        .filter(|s| s.motion_kind() == MotionKind::Conceal)
        .collect();
    assert_eq!(
        conceals.len(),
        2,
        "应有本轮“甲”和上一笔在途“乙”两条 Conceal: {:?}",
        motion.slices
    );

    // 本轮删除的“甲”：无同源在途旧图，从自身完整旧图开始吞。
    let jia = conceals
        .iter()
        .find(|s| s.byte_range() == (0, 3))
        .expect("应有“甲” Conceal");
    match jia {
        SliceMotion::Conceal {
            old,
            start_clip,
            direction,
            ..
        } => {
            assert_eq!(old.source.snapshot_id, jia_id);
            assert_eq!(*start_clip, rect(0.0, 0.0, 20.0, 10.0));
            assert_eq!(*direction, ClipDirection::LeftToRight);
        }
        other => panic!("expected Conceal, got {other:?}"),
    }

    // 上一笔在途“乙”：沿用原 source / 原遮罩 / 原方向继续吞，不与本轮合并。
    let yi = conceals
        .iter()
        .find(|s| s.byte_range() == (3, 6))
        .expect("应有在途“乙” Conceal");
    match yi {
        SliceMotion::Conceal {
            old,
            start_clip,
            direction,
            ..
        } => {
            assert_eq!(old.source, yi_source);
            assert_eq!(*start_clip, rect(20.0, 0.0, 10.0, 10.0));
            assert_eq!(*direction, ClipDirection::LeftToRight);
        }
        other => panic!("expected Conceal, got {other:?}"),
    }

    let ids = motion.source_snapshot_ids();
    assert!(ids.contains(&jia_id));
    assert!(ids.contains(&yi_source.snapshot_id));
}

#[test]
fn in_flight_deleted_glyph_not_swallowed_by_same_byte_range() {
    // Issue #919 复核 4：正文“甲乙丙”先删“乙”（旧 3..6）→“甲丙”，新“丙”恰好
    // 占据 3..6。在“乙”只吞一半时再 Forward Delete“甲”，不能因本轮 old_spans 的
    // 3..6 有“丙”就跳过在途“乙”的残留，也不能把“乙”的遮罩套到“丙”上。
    // 每张旧图按真实 source（snapshot_id + source_uv）区分，而不是字节位置。
    let old_layout = snapshot(vec![test_line(
        0,
        0,
        vec![cluster(0, 3, 0.0, 20.0), cluster(3, 6, 20.0, 20.0)],
        0.0,
    )]);
    let new_layout = snapshot(vec![test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0)]);
    let bing_id = old_layout.line_snapshots[0].id;
    let new_id = new_layout.line_snapshots[0].id;

    // 上一笔删除的“乙”来自更早的“甲乙丙”排版（不同 revision 的行图），正在吞。
    let yi_source = SnapshotRef {
        snapshot_id: LineSnapshotId::new(999, 1, 0),
        source_uv: rect(20.0, 0.0, 20.0, 10.0),
    };
    let last = frame_of(vec![drawn_slice(
        yi_source,
        rect(20.0, 0.0, 20.0, 10.0),
        rect(20.0, 0.0, 10.0, 10.0),
        MotionKind::Conceal,
        ClipDirection::LeftToRight,
        (3, 6),
    )]);

    // Forward Delete“甲”（旧 0..3）：“丙”应 Shift，在途“乙”继续退场。
    let edit = make_edit("甲丙", "丙", 0, 0, (0, 3), (0, 0), "");
    let motion = retarget(&edit, Some(&old_layout), &new_layout, Some(&last), 7);

    // “丙” 3..6 → 0..3 是 Shift（不因字节位置与在途“乙”相同而被判成退场/重影）。
    let shift = motion
        .slices
        .iter()
        .find(|s| s.motion_kind() == MotionKind::Shift)
        .expect("应有“丙” Shift");
    match shift {
        SliceMotion::Shift { new, from, to, .. } => {
            assert_eq!(new.source.snapshot_id, new_id);
            assert_eq!(*from, rect(20.0, 0.0, 20.0, 10.0));
            assert_eq!(*to, rect(0.0, 0.0, 20.0, 10.0));
        }
        other => panic!("expected Shift, got {other:?}"),
    }

    // 本轮删除的“甲”退场。
    let jia = motion
        .slices
        .iter()
        .find(|s| s.motion_kind() == MotionKind::Conceal && s.source_ref().snapshot_id == bing_id)
        .expect("应有“甲” Conceal");
    assert_eq!(jia.byte_range(), (0, 3));

    // 在途“乙”独立退场：保留原 source / 遮罩 / 方向，不与“丙”合并。
    let yi = motion
        .slices
        .iter()
        .find(|s| s.source_ref() == &yi_source)
        .expect("应有在途“乙” Conceal");
    match yi {
        SliceMotion::Conceal {
            start_clip,
            direction,
            ..
        } => {
            assert_eq!(*start_clip, rect(20.0, 0.0, 10.0, 10.0));
            assert_eq!(*direction, ClipDirection::LeftToRight);
        }
        other => panic!("expected Conceal, got {other:?}"),
    }

    // “丙”只位移，不应被当成 Conceal（否则 3..6 处会出现两张退场图）。
    assert!(
        !motion.slices.iter().any(|s| {
            s.motion_kind() == MotionKind::Conceal
                && s.source_ref().snapshot_id == bing_id
                && s.source_ref().source_uv == rect(20.0, 0.0, 20.0, 10.0)
        }),
        "未改的“丙”不应退场: {:?}",
        motion.slices
    );

    let ids = motion.source_snapshot_ids();
    assert!(ids.contains(&yi_source.snapshot_id));
    assert!(ids.contains(&bing_id));
}

#[test]
fn new_to_old_byte_end_maps_deletion_boundary_to_left_side() {
    // Issue #919 复核 13：删除“乙”（旧 3..6 → 新 3..3）时，同一个新偏移 3 既是
    // “甲”末尾（删除前坐标 3）也是“丙”开头（删除后坐标 3）。区间起点用
    // new_to_old_byte 取右侧（删除后）坐标，区间终点用 new_to_old_byte_end 取
    // 左侧（删除前）坐标，消除边界歧义。
    let edit = make_edit("甲乙丙", "甲丙", 6, 3, (3, 6), (3, 3), "");
    assert_eq!(new_to_old_byte(&edit, 3), 6);
    assert_eq!(new_to_old_byte_end(&edit, 3), 3);
}

#[test]
fn reveal_and_shift_targets_become_static_holes() {
    // Issue #919：静态层只挖 Reveal 的新位置 / Shift 的目标位置，Conceal 是旧字
    // 不挖孔；孔洞固定取自计划 target 排除集合，与 progress 无关。
    let old_layout = snapshot(vec![test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0)]);
    let new_layout = snapshot(vec![test_line(
        0,
        0,
        vec![cluster(0, 3, 0.0, 20.0), cluster(3, 6, 20.0, 20.0)],
        0.0,
    )]);
    // 段首插入“乙”：新字 Reveal(x=0)，“甲”未改但被挤到 x=20 → Shift(to x=20)。
    let edit = make_edit("甲", "乙甲", 0, 3, (0, 0), (0, 3), "乙");
    let motion = retarget(&edit, Some(&old_layout), &new_layout, None, 7);
    let new_id = new_layout.line_snapshots[0].id;

    let holes = motion.sample(0.0).static_holes;
    assert_eq!(holes.len(), 2, "Reveal + Shift 各挖一个孔: {holes:?}");
    assert!(holes.iter().any(|h| h.x == 0.0 && h.snapshot_id == new_id));
    assert!(holes.iter().any(|h| h.x == 20.0 && h.snapshot_id == new_id));

    // 与 progress 无关：中间帧孔洞不变。
    assert_eq!(motion.sample(50.0).static_holes.len(), 2);
    // 过渡结束后静态层恢复完整 canonical，不再挖孔。
    assert!(motion.sample(100.0).static_holes.is_empty());
}

#[test]
fn conceal_plan_does_not_create_static_hole() {
    // Issue #919：纯删除只有旧字 Conceal，静态层不挖孔（旧字形仍由静态/覆盖层
    // 唯一绘制，不能同时把相同像素画两次）。
    let old_layout = snapshot(vec![test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0)]);
    let new_layout = snapshot(vec![]);
    let old_id = old_layout.line_snapshots[0].id;
    let edit = make_edit("甲", "", 3, 0, (0, 3), (0, 0), "");
    let motion = retarget(&edit, Some(&old_layout), &new_layout, None, 7);

    assert!(motion.sample(0.0).static_holes.is_empty());
    assert_eq!(motion.required_textures, vec![old_id]);
}

#[test]
fn paragraph_with_content_change_does_not_also_get_paragraph_move() {
    // Issue #919 复核 2：同一段落只能由一种位移路径接管。段落既有真实内容变更
    // （display_patches 命中）又因上方插入新段而整体下移时，不能再叠加整体
    // ParagraphMove——否则该段同时采用“整段静态节点 y 位移”和“局部字形旧→新
    // 位移/遮罩”，两套画面坐标不一致，会跳行/重影。
    //
    // 构造：一次事务两笔 patch。旧正文“甲\n乙”两段（稳定 ID 6、7）；
    // 新正文“甲\n\n丙”三段——段 6“甲”不变、新增空段 8、段 7“乙”→“丙”。
    // 段 7 段首从 y=20 下移到 y=40（因中间插入空段 8），且段 7 内容已变，
    // 必须只走局部退场/露出。
    let mut old_p6 = test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0);
    old_p6.stable_paragraph_id = 6;
    let mut old_p7 = test_line(1, 4, vec![cluster(4, 7, 0.0, 20.0)], 20.0);
    old_p7.stable_paragraph_id = 7;
    let old_layout =
        snapshot_with_paragraph_index(vec![old_p6, old_p7], vec![20.0, 20.0], vec![6, 7]);

    let mut new_p6 = test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0);
    new_p6.stable_paragraph_id = 6;
    // 新增空段 8：位于两个换行之间（byte_start=4），无 cluster。
    let mut new_p8 = test_line(1, 4, vec![], 20.0);
    new_p8.stable_paragraph_id = 8;
    let mut new_p7 = test_line(2, 5, vec![cluster(5, 8, 0.0, 20.0)], 40.0);
    new_p7.stable_paragraph_id = 7;
    let new_layout = snapshot_with_paragraph_index(
        vec![new_p6, new_p8, new_p7],
        vec![20.0, 20.0, 20.0],
        vec![6, 8, 7],
    );

    // 直接验证段落锚点：旧段 7 在 y=20；新段 8 在 y=20、段 7 下移到 y=40。
    assert_eq!(
        old_layout.paragraph_anchor(7),
        Some((20.0, 20.0)),
        "旧段 7 段首 y=20"
    );
    assert_eq!(
        new_layout.paragraph_anchor(7),
        Some((40.0, 20.0)),
        "新段 7 段首 y=40"
    );
    assert_eq!(
        new_layout.paragraph_anchor(8),
        Some((20.0, 20.0)),
        "新段 8 段首 y=20"
    );

    // 旧正文“甲\n乙”（段 6 = 0..3，段 7 = 4..7）；新正文“甲\n\n丙”
    // （段 6 = 0..3，空段 8 = 4..4，段 7 = 5..8）。
    let edit = make_edit_from_patches(
        "甲\n乙",
        "甲\n\n丙",
        7,
        8,
        vec![((3, 3), (3, 4), "\n"), ((4, 7), (5, 8), "丙")],
    );
    let motion = retarget(&edit, Some(&old_layout), &new_layout, None, 7);

    // 段 7 的段首 y 确实变了，但内容也变了：只走局部路径，无整体位移。
    assert!(
        motion.paragraph_moves.is_empty(),
        "内容变更段落不应再有 ParagraphMove: {:?}",
        motion.paragraph_moves
    );
    // 本用例没有需要逐字位移的未改 cluster。
    assert!(
        !motion
            .slices
            .iter()
            .any(|s| s.motion_kind() == MotionKind::Shift),
        "本用例不应产生 Shift: {:?}",
        motion.slices
    );
    // 段 7 的旧“乙”退场、新“丙”露出仍保留。
    assert!(
        motion.slices.iter().any(|s| matches!(
            s,
            SliceMotion::Conceal { .. } if s.byte_range() == (4, 7)
        )),
        "旧“乙”应退场: {:?}",
        motion.slices
    );
    assert!(
        motion.slices.iter().any(|s| matches!(
            s,
            SliceMotion::Reveal { byte_range, .. } if *byte_range == (5, 8)
        )),
        "新“丙”应露出: {:?}",
        motion.slices
    );
}

#[test]
fn offscreen_edit_without_visible_slices_returns_none() {
    // Issue #919：完全落在视口外的编辑不产生局部动画——所有片段经
    // `slices.retain(|s| rects_intersect(s.motion_rect(), viewport))` 被滤掉后，
    // 若也没有整体段落位移，`begin_or_retarget` 应返回 None，让静态层直接画
    // canonical。屏幕外的字在滚动进入视口时由静态绘制呈现，不需要在空中预跑
    // 一段看不见的吞吐动画。
    let new_layout = snapshot(vec![test_line(
        0,
        0,
        vec![cluster(0, 3, 2000.0, 20.0)],
        0.0,
    )]);
    let edit = make_edit("", "甲", 0, 3, (0, 0), (0, 3), "甲");
    assert!(
        TextMotion::begin_or_retarget(&edit, None, &new_layout, None, 0.0, 100, 1, viewport(),)
            .is_none(),
        "视口外编辑不应创建局部动画"
    );
}

#[test]
fn newline_insert_moves_downstream_unchanged_paragraph_as_a_whole() {
    // Issue #919 复核 2：与「内容变更段落不再叠加整体位移」互为对照——内容未改的
    // 下游段落因上方插入换行而整体下移时，必须由唯一一条 `ParagraphMove` 接管，
    // 不再为段内每个字生成逐字切片，避免同段文字被静态位移和局部覆盖层画两次。
    //
    // 构造：旧正文“甲\n乙”两段（稳定 ID 6、7）；在“甲”末尾回车换行得到
    // “甲\n\n乙”——段 6“甲”不变、新增空段 8、段 7“乙”内容未改但整体下移。
    let mut old_p6 = test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0);
    old_p6.stable_paragraph_id = 6;
    let mut old_p7 = test_line(1, 4, vec![cluster(4, 7, 0.0, 20.0)], 20.0);
    old_p7.stable_paragraph_id = 7;
    let old_layout =
        snapshot_with_paragraph_index(vec![old_p6, old_p7], vec![20.0, 20.0], vec![6, 7]);

    let mut new_p6 = test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0);
    new_p6.stable_paragraph_id = 6;
    // 新增空段 8：位于两个换行之间（byte_start=4），无 cluster。
    let mut new_p8 = test_line(1, 4, vec![], 20.0);
    new_p8.stable_paragraph_id = 8;
    let mut new_p7 = test_line(2, 5, vec![cluster(5, 8, 0.0, 20.0)], 40.0);
    new_p7.stable_paragraph_id = 7;
    let new_layout = snapshot_with_paragraph_index(
        vec![new_p6, new_p8, new_p7],
        vec![20.0, 20.0, 20.0],
        vec![6, 8, 7],
    );

    // 段 7 内容未改，但段首因中间插入空段 8 从 y=20 下移到 y=40。
    assert_eq!(old_layout.paragraph_anchor(7), Some((20.0, 20.0)));
    assert_eq!(new_layout.paragraph_anchor(7), Some((40.0, 20.0)));

    // 在“甲”末尾（旧 3..3）插入换行，新插入区间 3..4。
    let edit = make_edit("甲\n乙", "甲\n\n乙", 3, 4, (3, 3), (3, 4), "\n");
    let motion = retarget(&edit, Some(&old_layout), &new_layout, None, 7);

    // 未改内容的段 7 只由唯一一条整体位移接管。
    assert_eq!(
        motion.paragraph_moves,
        vec![ParagraphMove {
            paragraph_id: 7,
            from_y: 20.0,
            to_y: 40.0,
        }],
        "段 7 应只走一条整体 ParagraphMove: {:?}",
        motion.paragraph_moves
    );
    // 段 7 的文字不产生逐字覆盖层（同段文字不能既静态位移又局部重画）。
    assert!(
        !motion
            .slices
            .iter()
            .any(|s| s.byte_range() == (4, 7) || s.byte_range() == (5, 8)),
        "整体位移的段 7 不应再产生逐字切片: {:?}",
        motion.slices
    );

    // 同一 progress 采样：起点为把目标位置补回旧 y 的完整偏移，结束回到 0。
    assert_eq!(motion.sample(0.0).paragraph_y_offsets, vec![(7, -20.0)]);
    assert!(motion.sample(100.0).paragraph_y_offsets.is_empty());
}

#[test]
fn newline_delete_moves_downstream_unchanged_paragraph_up_as_a_whole() {
    // Issue #919 复核 2：与「插入换行让下游段整体下移」对称——删除一个换行使
    // 上方的空段消失、下游内容未改的段整体上移时，同样只能由唯一一条
    // `ParagraphMove` 接管（`from_y` 大于 `to_y`，offset 起点为正），不再为
    // 段内每个字生成逐字切片，避免同段文字被静态位移和局部覆盖层画两次。
    //
    // 构造：旧正文“甲\n\n乙”三段（稳定 ID 6、8、7）——段 6“甲”、空段 8、
    // 段 7“乙”；删除段 6 末尾换行（旧 3..4）得到新正文“甲\n乙”，空段 8 消失、
    // 段 7“乙”内容未改但整体从 y=40 上移到 y=20。
    let mut old_p6 = test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0);
    old_p6.stable_paragraph_id = 6;
    // 空段 8：位于两个换行之间（byte_start=4），无 cluster。
    let mut old_p8 = test_line(1, 4, vec![], 20.0);
    old_p8.stable_paragraph_id = 8;
    let mut old_p7 = test_line(2, 5, vec![cluster(5, 8, 0.0, 20.0)], 40.0);
    old_p7.stable_paragraph_id = 7;
    let old_layout = snapshot_with_paragraph_index(
        vec![old_p6, old_p8, old_p7],
        vec![20.0, 20.0, 20.0],
        vec![6, 8, 7],
    );

    let mut new_p6 = test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0);
    new_p6.stable_paragraph_id = 6;
    let mut new_p7 = test_line(1, 4, vec![cluster(4, 7, 0.0, 20.0)], 20.0);
    new_p7.stable_paragraph_id = 7;
    let new_layout =
        snapshot_with_paragraph_index(vec![new_p6, new_p7], vec![20.0, 20.0], vec![6, 7]);

    // 段 7 内容未改，但段首因删除上方换行/空段从 y=40 上移到 y=20。
    assert_eq!(old_layout.paragraph_anchor(7), Some((40.0, 20.0)));
    assert_eq!(new_layout.paragraph_anchor(7), Some((20.0, 20.0)));
    // 空段 8 只存在于旧布局，新布局没有它的锚点。
    assert!(new_layout.paragraph_anchor(8).is_none());

    // 删除“甲”后的换行：旧 3..4 → 新插入区间 3..3（空）。
    let edit = make_edit_from_patches("甲\n\n乙", "甲\n乙", 3, 3, vec![((3, 4), (3, 3), "")]);
    let motion = retarget(&edit, Some(&old_layout), &new_layout, None, 7);

    // 未改内容的段 7 只由唯一一条整体位移接管（起点为正偏移，即当前画在旧 y）。
    assert_eq!(
        motion.paragraph_moves,
        vec![ParagraphMove {
            paragraph_id: 7,
            from_y: 40.0,
            to_y: 20.0,
        }],
        "段 7 应只走一条整体 ParagraphMove: {:?}",
        motion.paragraph_moves
    );
    // 段 7 的文字不产生逐字覆盖层（同段文字不能既静态位移又局部重画）。
    assert!(
        !motion
            .slices
            .iter()
            .any(|s| s.byte_range() == (5, 8) || s.byte_range() == (4, 7)),
        "整体位移的段 7 不应再产生逐字切片: {:?}",
        motion.slices
    );

    // 同一 progress 采样：起点为把目标位置补回旧 y 的完整正偏移，结束回到 0。
    assert_eq!(motion.sample(0.0).paragraph_y_offsets, vec![(7, 20.0)]);
    assert!(motion.sample(100.0).paragraph_y_offsets.is_empty());
}

#[test]
fn sample_clip_rect_is_viewport_intersected_but_mask_clip_is_original() {
    // Issue #919 复核 5：`sample()` 产生的原始动画遮罩 `mask_clip` 不被视口收缩，
    // 只有最终可见的 `clip_rect` 与视口求交。二者若混用，下一笔接续时会把视口
    // 裁剪边缘当成动画起点；`scene_graph_renderer` 也分别回执这两个矩形。
    let mut motion = conceal_with_start_clip(ClipDirection::LeftToRight);
    // start_clip = 50..58；把视口左边界移到 52，切掉左侧 2 像素。
    motion.viewport = rect(52.0, 0.0, 1000.0, 1000.0);

    let overlay = motion
        .sample(0.0)
        .overlays
        .into_iter()
        .next()
        .expect("conceal overlay");
    // mask_clip 保持整段原始遮罩 50..58。
    assert_eq!(
        (overlay.mask_clip.x, overlay.mask_clip.w),
        (50.0, 8.0),
        "mask_clip 不应被视口收缩"
    );
    // clip_rect 被视口裁到 52..58。
    assert_eq!((overlay.clip_rect.x, overlay.clip_rect.w), (52.0, 6.0));
    // 右边界相同但起点不同，clip_rect 不能代替 mask_clip。
    assert_eq!(
        overlay.clip_rect.x + overlay.clip_rect.w,
        overlay.mask_clip.x + overlay.mask_clip.w
    );
    assert_ne!(overlay.clip_rect, overlay.mask_clip);
}

#[test]
fn caret_sample_uses_same_shared_progress() {
    // Issue #919：唯一光标控制器与文字动画共用同一 progress。这里以纯几何
    // `TextMotion` 验证光标按同一条 ease-out 进度在真实布局 caret 之间插值，
    // 不新建第二条时间线。
    let mut motion = make_motion(Vec::new());
    motion.caret_from = Some((0.0, 0.0));
    motion.caret_to = Some((40.0, 20.0));

    assert_eq!(motion.sample(0.0).caret_sample, Some((0.0, 0.0)));

    // raw=0.5 → progress=ease_out_cubic(0.5)=0.875。
    let mid = motion.sample(50.0).caret_sample.expect("caret sample");
    assert!((mid.0 - 35.0).abs() < 1e-9, "x={}", mid.0);
    assert!((mid.1 - 17.5).abs() < 1e-9, "y={}", mid.1);

    // 结束时停在目标 caret，不残留中途位置。
    assert_eq!(motion.sample(100.0).caret_sample, Some((40.0, 20.0)));
}

#[test]
fn caret_sample_falls_back_to_present_endpoint() {
    // Issue #919：起点或终点 caret 缺失时（如首次布局、光标在视口外），保留
    // 可用的另一侧，不因缺一边而整体失去光标运动。
    let mut motion = make_motion(Vec::new());
    motion.caret_from = None;
    motion.caret_to = Some((10.0, 5.0));
    assert_eq!(motion.sample(0.0).caret_sample, Some((10.0, 5.0)));

    motion.caret_from = Some((3.0, 4.0));
    motion.caret_to = None;
    assert_eq!(motion.sample(0.0).caret_sample, Some((3.0, 4.0)));
}

#[test]
fn wrapped_unchanged_text_shifts_per_visual_line_without_merging_textures() {
    // Issue #919 复核 16：patch 外未改的连续文本按旧/新视觉行的连续片段分组，
    // 而不是每个 cluster 各一条 Shift。本用例构造同段「甲乙丙丁」在旧布局折成
    // 首行「甲乙丙」、次行「丁」（每行容纳 3 个等宽字）；段首插入「戊己」后折行
    // 点改变：新首行「戊己甲」、新次行「乙丙丁」。
    //
    // 位置映射：甲 (0,0)→(40,0)；乙 (20,0)→(0,20)；丙 (40,0)→(20,20)；
    // 丁 (0,20)→(40,20)。乙、丙位移同为 (-20,20) 且同属旧首行→新次行，源 UV
    // 连续，应合并成一条连续 Shift；甲（旧首行→新首行）和丁（旧次行→新次行）
    // 各为其他组合，不合并。最终 3 条 Shift，每条只用单一新行纹理。
    const PARA: u64 = 5;
    let line = |ordinal: u32, byte_start: usize, clusters, top: f64| {
        let mut l = test_line(ordinal, byte_start, clusters, top);
        l.stable_paragraph_id = PARA;
        l
    };

    // 旧：一段「甲乙丙丁」折两行——首行「甲乙丙」、次行「丁」（每行 3 字容量）。
    let old_layout = snapshot(vec![
        line(
            0,
            0,
            vec![
                cluster(0, 3, 0.0, 20.0),
                cluster(3, 6, 20.0, 20.0),
                cluster(6, 9, 40.0, 20.0),
            ],
            0.0,
        ),
        line(1, 9, vec![cluster(9, 12, 0.0, 20.0)], 20.0),
    ]);
    // 新：段首插入「戊己」——首行「戊己甲」、次行「乙丙丁」，折行点改变。
    let new_layout = snapshot(vec![
        line(
            0,
            0,
            vec![
                cluster(0, 3, 0.0, 20.0),
                cluster(3, 6, 20.0, 20.0),
                cluster(6, 9, 40.0, 20.0),
            ],
            0.0,
        ),
        line(
            1,
            9,
            vec![
                cluster(9, 12, 0.0, 20.0),
                cluster(12, 15, 20.0, 20.0),
                cluster(15, 18, 40.0, 20.0),
            ],
            20.0,
        ),
    ]);

    let old_line0_id = old_layout.line_snapshots[0].id;
    let old_line1_id = old_layout.line_snapshots[1].id;
    let new_line0_id = new_layout.line_snapshots[0].id;
    let new_line1_id = new_layout.line_snapshots[1].id;
    assert!(!old_line0_id.is_none() && !old_line1_id.is_none());
    assert!(!new_line0_id.is_none() && !new_line1_id.is_none());
    assert_ne!(old_line0_id, old_line1_id);
    assert_ne!(new_line0_id, new_line1_id);

    // 段首插入「戊己」：旧 0..0 → 新 0..6。
    let edit = make_edit("甲乙丙丁", "戊己甲乙丙丁", 0, 6, (0, 0), (0, 6), "戊己");
    let motion = retarget(&edit, Some(&old_layout), &new_layout, None, 1);

    let shifts: Vec<&SliceMotion> = motion
        .slices
        .iter()
        .filter(|s| s.motion_kind() == MotionKind::Shift)
        .collect();
    // 甲、乙丙（合并）、丁各一条，共 3 条。
    assert_eq!(
        shifts.len(),
        3,
        "甲/乙丙合并/丁应共 3 条 Shift: {:?}",
        motion.slices
    );

    // 每条 Shift 只使用单一新行纹理；两行纹理都被分别引用，不跨行硬拼同一纹理。
    let mut shift_source_ids: Vec<LineSnapshotId> = Vec::new();
    for s in &shifts {
        let id = s.source_snapshot_id();
        if !shift_source_ids.contains(&id) {
            shift_source_ids.push(id);
        }
    }
    assert_eq!(
        shift_source_ids.len(),
        2,
        "移位片段必须分别引用两行的 new 版纹理，实际 {shift_source_ids:?}"
    );
    assert!(shift_source_ids.contains(&new_line0_id));
    assert!(shift_source_ids.contains(&new_line1_id));
    // 旧行纹理绝不能被当作新画面来源（不能拿旧图冒充新字）。
    assert!(shifts
        .iter()
        .all(|s| s.source_snapshot_id() != old_line0_id && s.source_snapshot_id() != old_line1_id));

    // 「甲」留在新首行：旧首行 x=0 → 新首行 x=40，纹理取自新首行。
    match shifts
        .iter()
        .find(|s| s.byte_range() == (6, 9))
        .copied()
        .expect("甲 Shift")
    {
        SliceMotion::Shift { new, from, to, .. } => {
            assert_eq!(new.source.snapshot_id, new_line0_id);
            assert_eq!((from.x, from.y), (0.0, 0.0));
            assert_eq!((to.x, to.y), (40.0, 0.0));
        }
        other => panic!("甲应为 Shift: {other:?}"),
    }

    // 「乙丙」合并为一条连续 Shift：旧首行 x=20..60 → 新次行 x=0..40，
    // 纹理取自新次行，byte_range 连续 (9, 15)。
    match shifts
        .iter()
        .find(|s| s.byte_range() == (9, 15))
        .copied()
        .expect("乙丙合并 Shift")
    {
        SliceMotion::Shift { new, from, to, .. } => {
            assert_eq!(
                new.source.snapshot_id, new_line1_id,
                "乙丙的新画面必须取自新次行纹理"
            );
            assert_eq!(
                (from.x, from.y, from.w, from.h),
                (20.0, 0.0, 40.0, 10.0),
                "乙丙旧画面并集"
            );
            assert_eq!(
                (to.x, to.y, to.w, to.h),
                (0.0, 20.0, 40.0, 10.0),
                "乙丙新画面并集"
            );
            assert_eq!(
                (new.source.source_uv.x, new.source.source_uv.w),
                (0.0, 40.0),
                "乙丙源 UV 合并为连续区间"
            );
        }
        other => panic!("乙丙应为合并 Shift: {other:?}"),
    }

    // 「丁」从旧次行移到新次行末尾：旧次行 x=0 → 新次行 x=40。
    match shifts
        .iter()
        .find(|s| s.byte_range() == (15, 18))
        .copied()
        .expect("丁 Shift")
    {
        SliceMotion::Shift { new, from, to, .. } => {
            assert_eq!(new.source.snapshot_id, new_line1_id);
            assert_eq!((from.x, from.y), (0.0, 20.0), "丁的旧画面在旧次行");
            assert_eq!((to.x, to.y), (40.0, 20.0), "丁移入新次行末尾");
        }
        other => panic!("丁应为 Shift: {other:?}"),
    }
}

/// Issue #919 复核 16：合并 Shift 的连续重定向。
///
/// 第一笔编辑把"乙丙"从 (20,0) Shift 到 (0,10)，`merge_adjacent_shifts` 合为
/// 一条 `byte_range=(9,15)` 的 Shift。`sample(duration/2)` 后合并片段在插值
/// 位置 (2.5, 8.75, 40, 10)。第二笔编辑使"乙""丙"再次移动（从 y=10 到 y=20/30）。
///
/// 断言：第二笔的"乙""丙"各自 Shift 的 `from` 是上一帧该 cluster 的真实显示位置
/// （由 `restore_cluster_rect` 从合并 `mask_clip` 按 UV 比例还原），而非旧布局的
/// canonical 终点 (0,10)/(20,10)。此测试比只检查一次性 `shifts.len()==3` 更直接
/// 覆盖合并逻辑的完整生命周期。
#[test]
fn merged_shift_retargets_from_interpolated_position() {
    // --- 先构造第二笔 old_layout，取"乙丙"行 id 作为第一笔合并 Shift 的源 ---
    // Issue #919 复核 17：让第二笔 old_layout 与上一笔目标快照具备相同的源
    // revision / 源行映射。find_merged_shift 现在核对 snapshot_id + source_uv，
    // 若第一笔 snap 用与 old_layout 无关的硬编码 id，snapshot_id 核对会失败，
    // 测试无法证明合并 Shift 的接续真正生效。
    let old_layout = snapshot(vec![
        test_line(
            0,
            0,
            vec![
                cluster(0, 3, 0.0, 20.0),
                cluster(3, 6, 20.0, 20.0),
                cluster(6, 9, 40.0, 20.0),
            ],
            0.0,
        ),
        test_line(
            1,
            9,
            vec![cluster(9, 12, 0.0, 20.0), cluster(12, 15, 20.0, 20.0)],
            10.0,
        ),
    ]);
    // "乙丙"行是 old_layout.line_snapshots[1]，其 id 即第一笔合并 Shift 的源
    // snapshot_id。sample() 透传 new.source.snapshot_id 到 DrawnSlice，下一笔
    // retarget 时 old_spans 中"乙""丙"的 slice.source.snapshot_id 也取自此行 id，
    // find_merged_shift 的 snapshot_id + source_uv 核对才能通过。
    let merged_snap_id = old_layout.line_snapshots[1].id;

    // --- 第一笔：直接构造合并后的 TextMotion ---
    let merged_uv = rect(0.0, 0.0, 40.0, 10.0);
    let merged_new_rect = rect(0.0, 10.0, 40.0, 10.0);
    let merged_from = rect(20.0, 0.0, 40.0, 10.0);
    let merged_to = rect(0.0, 10.0, 40.0, 10.0);
    let snap = SnapshotRef {
        snapshot_id: merged_snap_id,
        source_uv: merged_uv,
    };
    let motion = TextMotion {
        slices: vec![SliceMotion::Shift {
            new: VisualSlice {
                source: snap,
                rect: merged_new_rect,
            },
            from: merged_from,
            to: merged_to,
            byte_range: (9, 15),
            shaping_identity: identity(),
        }],
        started_ms: 0.0,
        duration_ms: 100,
        session: 7,
        target_revision: 2,
        viewport: viewport(),
        static_exclusions: vec![],
        required_textures: vec![snap.snapshot_id],
        caret_from: None,
        caret_to: None,
        paragraph_moves: vec![],
    };

    // sample(duration/2)：progress = ease_out_cubic(0.5) = 0.875
    let frame = motion.sample(50.0);
    assert!(!frame.finished);
    assert_eq!(frame.overlays.len(), 1, "合并 Shift 应产出 1 条 overlay");
    let ov = &frame.overlays[0];
    assert_eq!(ov.motion_kind, MotionKind::Shift);
    assert_eq!(ov.byte_range, (9, 15));

    let p = 0.875_f64;
    let expected_x = 20.0 + (0.0 - 20.0) * p; // 2.5
    let expected_y = 0.0 + (10.0 - 0.0) * p; // 8.75
    assert!(
        (ov.x - expected_x).abs() < 0.01,
        "合并插值 x={} expected={}",
        ov.x,
        expected_x
    );
    assert!(
        (ov.y - expected_y).abs() < 0.01,
        "合并插值 y={} expected={}",
        ov.y,
        expected_y
    );

    // 从 overlay 构造 LocalDrawnFrame（模拟 scene_graph_renderer 的 DrawnSlice 构造）
    let drawn = DrawnSlice {
        slice: VisualSlice {
            source: SnapshotRef {
                snapshot_id: ov.snapshot_id,
                source_uv: ov.source_rect,
            },
            rect: rect(ov.x, ov.y, ov.w, ov.h),
        },
        visible_width: ov.clip_rect.w,
        clip_rect: ov.clip_rect,
        mask_clip: ov.mask_clip,
        motion_kind: ov.motion_kind,
        byte_range: ov.byte_range,
        shaping_identity: ov.shaping_identity.clone(),
        direction: ov.direction.unwrap_or(ClipDirection::LeftToRight),
    };
    let last_drawn = LocalDrawnFrame {
        session: 7,
        revision: 2,
        slices: vec![drawn],
    };

    // --- 第二笔编辑：在开头插入"戊"使"乙""丙"从 y=10 移到 y=20/y=30 ---
    // old_layout（已在上方构造）："甲乙'丙'" 一行(y=0) + "乙丙" 一行(y=10)
    //   甲(0,0,20,10) byte(0,3), 乙'(20,0,20,10) byte(3,6), 丙'(40,0,20,10) byte(6,9)
    //   乙(0,10,20,10) byte(9,12), 丙(20,10,20,10) byte(12,15)
    // new_layout："戊甲乙'丙'" 一行(y=0) + "乙" 一行(y=20) + "丙" 一行(y=30)
    //   戊(0,0,20,10) byte(0,3), 甲(20,0,20,10) byte(3,6),
    //   乙'(40,0,20,10) byte(6,9), 丙'(60,0,20,10) byte(9,12)
    //   乙(0,20,20,10) byte(12,15), 丙(0,30,20,10) byte(15,18)
    //
    // "乙""丙"在不同新行 → merge_adjacent_shifts 不会合并它们，
    // 可以单独断言各自的 from。
    let new_layout = snapshot(vec![
        test_line(
            0,
            0,
            vec![
                cluster(0, 3, 0.0, 20.0),
                cluster(3, 6, 20.0, 20.0),
                cluster(6, 9, 40.0, 20.0),
                cluster(9, 12, 60.0, 20.0),
            ],
            0.0,
        ),
        test_line(1, 12, vec![cluster(12, 15, 0.0, 20.0)], 20.0),
        test_line(2, 15, vec![cluster(15, 18, 0.0, 20.0)], 30.0),
    ]);

    // 编辑：在 byte 0 插入"戊"（3 字节 UTF-8 汉字）。replace=(0,0), inserted=(0,3)
    // "戊"是 3 字节，new_head=3 是合法 UTF-8 边界（戊 后 甲 前），
    // EditorSelection::collapsed 不会夹偏移。旧测试用 ASCII "X"（1 字节）
    // 当 3 字节，偏移 3 落在首字内部，selection 被夹到非法边界外。
    let edit = make_edit("甲乙丙乙丙", "戊甲乙丙乙丙", 0, 3, (0, 0), (0, 3), "戊");
    let motion2 = retarget(&edit, Some(&old_layout), &new_layout, Some(&last_drawn), 7);

    // "乙"的 new byte_range=(12,15)，old_byte_range=(9,12)
    // "丙"的 new byte_range=(15,18)，old_byte_range=(12,15)
    let yi_shift = motion2
        .slices
        .iter()
        .find(|s| s.motion_kind() == MotionKind::Shift && s.byte_range() == (12, 15))
        .expect("应有乙 Shift (byte 12..15)");
    let bing_shift = motion2
        .slices
        .iter()
        .find(|s| s.motion_kind() == MotionKind::Shift && s.byte_range() == (15, 18))
        .expect("应有丙 Shift (byte 15..18)");

    match yi_shift {
        SliceMotion::Shift { from, to, new, .. } => {
            // from = 上一帧"乙"的插值位置：(2.5, 8.75, 20, 10)
            // 而非旧布局 canonical 终点 (0, 10, 20, 10)
            assert!(
                (from.x - 2.5).abs() < 0.01,
                "乙 from.x={} expected=2.5 (上一帧插值位置)",
                from.x
            );
            assert!(
                (from.y - 8.75).abs() < 0.01,
                "乙 from.y={} expected=8.75 (上一帧插值位置)",
                from.y
            );
            assert_eq!(from.w, 20.0);
            assert_eq!(from.h, 10.0);
            // to = 新布局位置
            assert_eq!(*to, rect(0.0, 20.0, 20.0, 10.0));
            // 源 UV 正确（"乙"的 new 版 UV）
            assert_eq!(new.source.source_uv, rect(0.0, 0.0, 20.0, 10.0));
        }
        other => panic!("expected Shift for 乙, got {other:?}"),
    }

    match bing_shift {
        SliceMotion::Shift { from, to, new, .. } => {
            // from = 上一帧"丙"的插值位置：(22.5, 8.75, 20, 10)
            // 而非旧布局 canonical 终点 (20, 10, 20, 10)
            assert!(
                (from.x - 22.5).abs() < 0.01,
                "丙 from.x={} expected=22.5 (上一帧插值位置)",
                from.x
            );
            assert!(
                (from.y - 8.75).abs() < 0.01,
                "丙 from.y={} expected=8.75 (上一帧插值位置)",
                from.y
            );
            assert_eq!(from.w, 20.0);
            assert_eq!(from.h, 10.0);
            assert_eq!(*to, rect(0.0, 30.0, 20.0, 10.0));
            assert_eq!(new.source.source_uv, rect(0.0, 0.0, 20.0, 10.0));
        }
        other => panic!("expected Shift for 丙, got {other:?}"),
    }
}

/// Issue #919 复核 18：精确 Shift 接续必须核对来源 snapshot_id + UV，不能只凭
/// byte-range 恰好相同就认成同一张画面。构造 `old_span` 与 `last_drawn` 的
/// byte-range 完全相同，但 snapshot ID 不同且 `last.mask_clip` 位置很远；断言
/// 不会拿这条错误来源的 Shift 作为新计划起点，而是从 canonical 旧位置开始。
/// 与 `merged_shift_retargets_from_interpolated_position`（正向：合并来源正确
/// 时接续插值位置）组成一正一反两个用例。
#[test]
fn exact_shift_rejects_same_byte_range_different_snapshot() {
    // old_layout："甲乙" 一行，甲 byte(0,3) x=0，乙 byte(3,6) x=20
    let old_layout = snapshot(vec![test_line(
        0,
        0,
        vec![cluster(0, 3, 0.0, 20.0), cluster(3, 6, 20.0, 20.0)],
        0.0,
    )]);
    // old_layout 真实行 id（"乙"的源 snapshot_id）
    let correct_id = old_layout.line_snapshots[0].id;
    // 构造一个与 old_layout 不同 revision 的 snapshot_id，模拟上一帧来自不同
    // layout revision/行图的未合并 Shift。
    let wrong_id = LineSnapshotId::new(
        correct_id.layout_revision + 100,
        correct_id.paragraph_id,
        correct_id.visual_line_ordinal,
    );

    // last_drawn：一条 Shift，byte_range=(3,6) 与"乙"的 old_byte_range 完全相同，
    // 但 snapshot_id 不同，mask_clip 位置很远（500,500）。若精确匹配只按 byte-range
    // 不核对来源，会把 (500,500) 当作 from；修复后应被拒绝，改走 canonical 旧位置。
    let last_drawn = LocalDrawnFrame {
        session: 1,
        revision: 1,
        slices: vec![DrawnSlice {
            slice: VisualSlice {
                source: SnapshotRef {
                    snapshot_id: wrong_id,
                    source_uv: rect(20.0, 0.0, 20.0, 10.0),
                },
                rect: rect(500.0, 500.0, 20.0, 10.0),
            },
            visible_width: 20.0,
            clip_rect: rect(500.0, 500.0, 20.0, 10.0),
            mask_clip: rect(500.0, 500.0, 20.0, 10.0),
            motion_kind: MotionKind::Shift,
            byte_range: (3, 6),
            shaping_identity: identity(),
            direction: ClipDirection::LeftToRight,
        }],
    };

    // new_layout："甲丙乙" 一行，甲 x=0 不变，乙从 x=20 移到 x=40。
    // 在"甲"后插入"丙"："甲"位置不变不产生 Shift，只有"乙"Shift，避免
    // merge_adjacent_shifts 把"甲""乙"两条相邻 Shift 合并成一条而无法单独
    // 断言"乙"的 from。
    let new_layout = snapshot(vec![test_line(
        0,
        0,
        vec![
            cluster(0, 3, 0.0, 20.0),
            cluster(3, 6, 20.0, 20.0),
            cluster(6, 9, 40.0, 20.0),
        ],
        0.0,
    )]);

    // 编辑：在 byte 3（"甲"后"乙"前）插入"丙"（3 字节 UTF-8 汉字）
    let edit = make_edit("甲乙", "甲丙乙", 3, 6, (3, 3), (3, 6), "丙");
    let motion = retarget(&edit, Some(&old_layout), &new_layout, Some(&last_drawn), 1);

    // "乙"的 new byte_range=(6,9)，old_byte_range=(3,6)
    let yi_shift = motion
        .slices
        .iter()
        .find(|s| s.motion_kind() == MotionKind::Shift && s.byte_range() == (6, 9))
        .expect("应有乙 Shift (byte 6..9)");

    match yi_shift {
        SliceMotion::Shift { from, to, .. } => {
            // from 应为 old_span.slice.rect = (20, 0, 20, 10)（canonical 旧位置），
            // 而非 last.mask_clip = (500, 500, 20, 10)（远处错误来源）。
            assert!(
                (from.x - 20.0).abs() < 0.01,
                "乙 from.x={} expected=20.0 (canonical 旧位置)，\
                 不应沿用错误来源的 500.0",
                from.x
            );
            assert!(
                (from.y - 0.0).abs() < 0.01,
                "乙 from.y={} expected=0.0 (canonical 旧位置)，\
                 不应沿用错误来源的 500.0",
                from.y
            );
            assert_eq!(*to, rect(40.0, 0.0, 20.0, 10.0));
        }
        other => panic!("expected Shift for 乙, got {other:?}"),
    }
}

/// Issue #919 复核 17：合并 Shift 中途被替换（shaping 变化）且跨行位移距离
/// 大于字高时，退场 Conceal 的 target 与 clip 必须有非零交集。旧代码用
/// `old_span.slice.rect`（canonical 目标位置）作 Conceal `old.rect`，与
/// `start_clip`（插值位置）在 y 方向不相交，`scene_graph_renderer` 做交集
/// 会跳过整个 glyph，旧字从未开始吞就直接消失。
#[test]
fn merged_shift_shaping_change_conceal_has_nonzero_target_clip_overlap() {
    // --- 先构造第二笔 old_layout，取"乙丙"行 id 作为第一笔合并 Shift 的源 ---
    let old_layout = snapshot(vec![
        test_line(0, 0, vec![cluster(0, 3, 0.0, 20.0)], 0.0),
        test_line(
            1,
            3,
            vec![
                cluster_shaped(3, 6, 0.0, 20.0, 0),
                cluster(6, 9, 20.0, 20.0),
            ],
            100.0,
        ),
    ]);
    let merged_snap_id = old_layout.line_snapshots[1].id;

    // --- 第一笔：合并 Shift 把"乙丙"从 y=0 移到 y=100（跨行位移）---
    let merged_uv = rect(0.0, 0.0, 40.0, 10.0);
    let snap = SnapshotRef {
        snapshot_id: merged_snap_id,
        source_uv: merged_uv,
    };
    let motion = TextMotion {
        slices: vec![SliceMotion::Shift {
            new: VisualSlice {
                source: snap,
                rect: rect(0.0, 100.0, 40.0, 10.0),
            },
            from: rect(20.0, 0.0, 40.0, 10.0),
            to: rect(0.0, 100.0, 40.0, 10.0),
            byte_range: (3, 9),
            shaping_identity: identity(),
        }],
        started_ms: 0.0,
        duration_ms: 100,
        session: 7,
        target_revision: 2,
        viewport: viewport(),
        static_exclusions: vec![],
        required_textures: vec![snap.snapshot_id],
        caret_from: None,
        caret_to: None,
        paragraph_moves: vec![],
    };

    // sample(50ms)：progress = ease_out_cubic(0.5) = 0.875
    // 合并插值 y = 0 + 100*0.875 = 87.5，canonical 目标 y = 100，相距 12.5 > 字高 10。
    let frame = motion.sample(50.0);
    let ov = &frame.overlays[0];
    let drawn = DrawnSlice {
        slice: VisualSlice {
            source: SnapshotRef {
                snapshot_id: ov.snapshot_id,
                source_uv: ov.source_rect,
            },
            rect: rect(ov.x, ov.y, ov.w, ov.h),
        },
        visible_width: ov.clip_rect.w,
        clip_rect: ov.clip_rect,
        mask_clip: ov.mask_clip,
        motion_kind: ov.motion_kind,
        byte_range: ov.byte_range,
        shaping_identity: ov.shaping_identity.clone(),
        direction: ov.direction.unwrap_or(ClipDirection::LeftToRight),
    };
    let last_drawn = LocalDrawnFrame {
        session: 7,
        revision: 2,
        slices: vec![drawn],
    };

    // --- 第二笔：在"甲"后插入"戊"，"乙"shaping 变化（glyph_hash 0→999）---
    // old_layout："甲"一行(y=0) + "乙丙"一行(y=100)
    //   甲(0,0,20,10) byte(0,3)
    //   乙(0,100,20,10) byte(3,6) shaping hash=0, 丙(20,100,20,10) byte(6,9)
    // new_layout："甲戊"一行(y=0) + "乙丙"一行(y=110)
    //   甲(0,0,20,10) byte(0,3), 戊(20,0,20,10) byte(3,6)
    //   乙(0,110,20,10) byte(6,9) shaping hash=999, 丙(20,110,20,10) byte(9,12)
    let new_layout = snapshot(vec![
        test_line(
            0,
            0,
            vec![cluster(0, 3, 0.0, 20.0), cluster(3, 6, 20.0, 20.0)],
            0.0,
        ),
        test_line(
            1,
            6,
            vec![
                cluster_shaped(6, 9, 0.0, 20.0, 999),
                cluster(9, 12, 20.0, 20.0),
            ],
            110.0,
        ),
    ]);

    // 编辑：在 byte 3 插入"戊"（3 字节）。replace=(3,3), inserted=(3,6)
    let edit = make_edit("甲乙丙", "甲戊乙丙", 3, 6, (3, 3), (3, 6), "戊");
    let motion2 = retarget(&edit, Some(&old_layout), &new_layout, Some(&last_drawn), 7);

    // "乙"new byte_range=(6,9)，old_byte_range=(3,6)，在合并 Shift (3,9) 内。
    // shaping 变化 → Conceal + Reveal。Conceal 应用 shift_from（插值位置）。
    let conceal_yi = motion2
        .slices
        .iter()
        .find(|s| s.byte_range() == (3, 6) && s.motion_kind() == MotionKind::Conceal)
        .expect("应有旧\"乙\" Conceal (byte 3..6)");

    match conceal_yi {
        SliceMotion::Conceal {
            old, start_clip, ..
        } => {
            // old.rect = shift_from = 上一帧"乙"的插值位置 (2.5, 87.5, 20, 10)
            // 而非 canonical 目标 (0, 100, 20, 10)
            assert!(
                (old.rect.y - 87.5).abs() < 0.01,
                "old.rect.y={} expected=87.5 (上一帧插值位置)",
                old.rect.y
            );
            assert!(
                (old.rect.x - 2.5).abs() < 0.01,
                "old.rect.x={} expected=2.5 (上一帧插值位置)",
                old.rect.x
            );
            assert_eq!(old.rect.w, 20.0);
            assert_eq!(old.rect.h, 10.0);
            // start_clip = shift_from，与 old.rect 对齐
            assert_eq!(*start_clip, old.rect);
            // canonical old_span.slice.rect.y = 100，相距 12.5 > 字高 10
            assert!((100.0 - old.rect.y).abs() > 10.0);
        }
        other => panic!("expected Conceal, got {other:?}"),
    }

    // sample(0)：退场 overlay 的 target 与 clip 应有非零交集。
    // 旧代码 target=canonical(0,100), clip=shift_from(2.5,87.5) → y 不相交 → 空交集。
    let frame2 = motion2.sample(0.0);
    let conceal_overlay = frame2
        .overlays
        .iter()
        .find(|o| o.motion_kind == MotionKind::Conceal && o.byte_range == (3, 6))
        .expect("应有 Conceal overlay");
    let target = rect(
        conceal_overlay.x,
        conceal_overlay.y,
        conceal_overlay.w,
        conceal_overlay.h,
    );
    let overlap = intersect_rect(&target, &conceal_overlay.clip_rect);
    assert!(
        overlap.w > 0.01 && overlap.h > 0.01,
        "target {:?} 与 clip {:?} 交集为 {:?}，应有非零交集",
        target,
        conceal_overlay.clip_rect,
        overlap
    );

    // sample(50ms)：中间帧仍应有非零交集（遮罩从 shift_from 收拢，target 固定）
    let frame2_mid = motion2.sample(50.0);
    let conceal_overlay_mid = frame2_mid
        .overlays
        .iter()
        .find(|o| o.motion_kind == MotionKind::Conceal && o.byte_range == (3, 6))
        .expect("应有中间帧 Conceal overlay");
    let target_mid = rect(
        conceal_overlay_mid.x,
        conceal_overlay_mid.y,
        conceal_overlay_mid.w,
        conceal_overlay_mid.h,
    );
    let overlap_mid = intersect_rect(&target_mid, &conceal_overlay_mid.clip_rect);
    assert!(
        overlap_mid.w > 0.01 && overlap_mid.h > 0.01,
        "中间帧 target {:?} 与 clip {:?} 交集为 {:?}，应有非零交集",
        target_mid,
        conceal_overlay_mid.clip_rect,
        overlap_mid
    );
}
