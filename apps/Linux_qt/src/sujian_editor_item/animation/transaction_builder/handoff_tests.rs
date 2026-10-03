//! #819 最新评论：采真实事务的文字帧，覆盖两次和三次交棒。
use super::*;
use crate::sujian_editor_item::animation::frame_state::{
    SampledEditVisualState, SampledSliceFrame,
};
use crate::sujian_editor_item::animation::rebase::RebaseVisualState;
use crate::sujian_editor_item::animation::sample::sample_transaction_visual_state;
use crate::sujian_editor_item::animation::transaction::types::{
    CaretTrackSegment, CaretTrackSegmentKind, IngestSnapshotSide, PreparedCursorVisualTrack,
};
use crate::sujian_editor_item::layout_snapshot::{LineSnapshotId, ShapingIdentity};
use std::time::{Duration, Instant};

fn caret(x: f64, y: f64) -> CursorRect {
    CursorRect {
        x,
        top: y,
        bottom: y + 20.0,
        baseline_y: y + 16.0,
    }
}

fn snapshot(text: &str, revision: u64) -> EditorLayoutSnapshot {
    let clusters = text
        .char_indices()
        .map(|(i, c)| {
            (
                i,
                i + c.len_utf8(),
                i as f64 * 10.0,
                0.0,
                ShapingIdentity {
                    text_content_hash: u64::from(c),
                    raw_font_fingerprint: "font".into(),
                    glyph_indexes_hash: u64::from(c),
                    cluster_glyph_count: 1,
                    direction_rtl: false,
                    format_fingerprint: 0,
                },
            )
        })
        .collect();
    let mut snapshot = super::tests::make_test_snapshot(text, clusters);
    snapshot.line_snapshots[0].id = LineSnapshotId::new(revision, 0, 0);
    snapshot
}

fn edit_spec(
    coord: &mut LinuxEditorAnimationCoordinator,
    old: &str,
    new: &str,
    visual_state: RebaseVisualState,
) -> VisualEditSpec {
    let key = coord.alloc_key();
    let inserting = new.len() > old.len();
    VisualEditSpec {
        key,
        operation_kind: if inserting {
            TextVisualOperationKind::Insert
        } else {
            TextVisualOperationKind::Delete
        },
        // Issue #824: 正文动画种类来自 patch 事实。
        patch_kind: if inserting {
            crate::sujian_editor_item::edit_motion::EditorAnimationKind::Insert
        } else {
            crate::sujian_editor_item::edit_motion::EditorAnimationKind::Delete
        },
        old_snapshot: snapshot(old, key.transaction_id * 2),
        new_snapshot: snapshot(new, key.transaction_id * 2 + 1),
        inserted_ranges: if inserting {
            vec![(old.len(), new.len())]
        } else {
            vec![]
        },
        deleted_ranges: if inserting {
            vec![]
        } else {
            vec![(new.len(), old.len())]
        },
        offset_map: OffsetMap::build(old, new),
        old_cursor_rect: Some(caret(old.len() as f64 * 10.0, 0.0)),
        new_cursor_rect: Some(caret(new.len() as f64 * 10.0, 0.0)),
        old_cursor_visual_line_id: Some(0),
        new_cursor_visual_line_id: Some(0),
        old_cursor_line_top: 0.0,
        old_cursor_line_bottom: 20.0,
        new_cursor_line_top: 0.0,
        new_cursor_line_bottom: 20.0,
        cursor_owner_epoch: 1,
        layout_basis_revision: LayoutRevision::initial(),
        visual_state,
        visual_affected_byte_range_old: Some((0, old.len())),
        visual_affected_byte_range_new: Some((0, new.len())),
        text_duration_ms: 100,
        caret_duration_ms: 100,
        text_animation_enabled: true,
        caret_animation_enabled: true,
        coordinated_animation_enabled: true,
        composition_commit_crossfade: None,
    }
}

fn start(mut tx: PreparedTextVisualTransaction, now: Instant) -> PreparedTextVisualTransaction {
    tx.state = TextVisualTransactionState::Rendering;
    tx.texture_prepared = true;
    tx.timeline.mark_first_frame(now);
    // Issue #824：carried 吞吐字改由 Timed（本笔 motion 时长）继续收口，必须像
    // 生产路径 `begin_rendering_transactions` 一样给它们打上同一起跑时间。
    for unit in tx.units.iter_mut() {
        unit.timing.mark_started(now);
    }
    tx.cursor_visual_track
        .as_mut()
        .expect("caret track")
        .started_at = Some(now);
    tx
}

fn slice(state: &SampledEditVisualState, stage: IngestStageId) -> &SampledSliceFrame {
    state
        .slices
        .iter()
        .find(|s| s.unit_stage_id == Some(stage))
        .expect("stage slice")
}

/// Issue #824 评论 5971089641 第 5 节：carried 旧 glyph 不再保留旧 stage
/// （`stage_id=None`，只保留当前可见几何），身份改用纹理来源 snapshot + kind。
fn carried_slice(
    state: &SampledEditVisualState,
    snapshot_id: LineSnapshotId,
    kind: AnimatedSliceKind,
) -> &SampledSliceFrame {
    state
        .slices
        .iter()
        .find(|s| s.snapshot_id == snapshot_id && s.kind == kind)
        .expect("carried slice by snapshot")
}

fn assert_rect(actual: &SourceRect, expected: &SourceRect) {
    for (a, b) in [
        (actual.x, expected.x),
        (actual.y, expected.y),
        (actual.w, expected.w),
        (actual.h, expected.h),
    ] {
        assert!(
            (a - b).abs() < 1e-8,
            "rect mismatch: {actual:?} != {expected:?}"
        );
    }
}

fn assert_slice(actual: &SampledSliceFrame, expected: &SampledSliceFrame) {
    assert_rect(&actual.dest_rect, &expected.dest_rect);
    assert_rect(&actual.source_rect, &expected.source_rect);
    assert_eq!(actual.snapshot_id, expected.snapshot_id);
    assert_eq!(actual.opacity, expected.opacity);
}

/// Issue #824 评论 5971089641 第 4 节：把「某段中点的路程比例」换算成采样时刻。
///
/// 全局 easing 只应用一次：`distance_fraction = 1 - (1 - p)^3`，
/// 所以 `p = 1 - cbrt(1 - fraction)`，`elapsed = p * duration`。
fn mid_segment_elapsed_ms(track: &PreparedCursorVisualTrack, index: usize) -> u64 {
    let total: f64 = track.segments.iter().map(|s| s.distance_weight).sum();
    let before: f64 = track.segments[..index]
        .iter()
        .map(|s| s.distance_weight)
        .sum();
    let fraction = (before + track.segments[index].distance_weight / 2.0) / total;
    let progress = 1.0 - (1.0 - fraction).cbrt();
    (progress * track.duration_ms as f64) as u64
}

fn rebase(
    coord: &mut LinuxEditorAnimationCoordinator,
    key: VisualTransactionKey,
    now: Instant,
    old: &str,
) -> RebaseVisualState {
    let state = coord.take_rebase_frames(&[key], "test_handoff", now, None, old, 1);
    assert!(
        !state.carried_units.is_empty(),
        "must exercise carried text"
    );
    assert!(coord.prepared_queue.active_transactions().is_empty());
    state
}

#[test]
fn issue819_comment5970185344_real_three_deletes_preserve_frames_stages_and_time() {
    let mut coord = LinuxEditorAnimationCoordinator::new();
    let t0 = Instant::now();
    let first = start(
        build_prepared_transaction(edit_spec(
            &mut coord,
            "ABC",
            "AB",
            RebaseVisualState::default(),
        ))
        .expect_created("first"),
        t0,
    );
    let a = IngestStageId(first.key.transaction_id);
    let t1 = t0 + Duration::from_millis(50);
    let sampled_first = sample_transaction_visual_state(&first, t1);
    let key = first.key;
    coord.prepared_queue.enqueue(first);
    let handoff = rebase(&mut coord, key, t1, "AB");
    // 交棒时仍记录旧 motion id 供诊断，但旧 stage 不再排进下一笔动画。
    assert_eq!(handoff.caret_handoff.as_ref().unwrap().stage_id, a);
    assert_eq!(handoff.carried_units[0].stage_id, Some(a));
    let sampled_caret = handoff.caret_handoff.as_ref().unwrap().sampled;
    let second = start(
        build_prepared_transaction(edit_spec(&mut coord, "AB", "A", handoff))
            .expect_created("second"),
        t1,
    );
    let b = IngestStageId(second.key.transaction_id);
    let track = second.cursor_visual_track.as_ref().unwrap();
    // Issue #824 评论 5971089641 第 6 节：retarget 后的 motion 用**本笔**单一时长，
    // 不再是「旧 route 剩余 50ms + 新 route 100ms」的 150ms。
    assert_eq!(track.duration_ms, 100);
    assert_eq!(track.segments.len(), 2, "no zero length layout handoff");
    // 新 route 的起点就是 retarget 采样到的当前屏幕 caret——绝不把旧 route 剩余段
    // 拼在它前面（旧模型下这里会先接旧路线剩余段）。
    assert_eq!(track.segments[0].from, sampled_caret);
    assert_eq!(track.segments[0].kind, CaretTrackSegmentKind::LayoutHandoff);
    assert_eq!(track.segments[1].kind, CaretTrackSegmentKind::IngestLine);
    // 段权重只表达几何路程，不再是「每段独立动画的时长」。
    for segment in &track.segments {
        let expected = ((segment.to.x - segment.from.x).powi(2)
            + (segment.to.top - segment.from.top).powi(2))
        .sqrt()
        .max(1e-3);
        assert!((segment.distance_weight - expected).abs() < 1e-8);
    }
    // 所有新段都属于本笔 stage，route 不再混入旧 stage 剩余段。
    assert!(track
        .segments
        .iter()
        .all(|segment| segment.ingest_stage_id == b));
    let first_carried = slice(&sampled_first, a).clone();
    assert_slice(
        carried_slice(
            &sample_transaction_visual_state(&second, t1),
            first_carried.snapshot_id,
            first_carried.kind,
        ),
        &first_carried,
    );
    let mut width = first_carried.dest_rect.w;
    // Issue #824：新 motion 一建立就开始向最新目标扫，B 不再排队等旧 route 播完。
    let mut b_width = slice(&sample_transaction_visual_state(&second, t1), b)
        .dest_rect
        .w;
    assert!(
        (b_width - 10.0).abs() < 1e-8,
        "第二笔自己的 B 从完整宽度起步"
    );
    for ms in 0..=50 {
        let frame = sample_transaction_visual_state(&second, t1 + Duration::from_millis(ms));
        let w = carried_slice(&frame, first_carried.snapshot_id, first_carried.kind)
            .dest_rect
            .w;
        assert!(w <= width + 1e-8, "C must continue closing");
        width = w;
        let current_b = slice(&frame, b).dest_rect.w;
        assert!(
            current_b <= b_width + 1e-8,
            "B 必须随本笔 motion 单调收口，不能先恢复再吞"
        );
        b_width = current_b;
    }
    let t2 = t1 + Duration::from_millis(25);
    let sampled_second = sample_transaction_visual_state(&second, t2);
    let key = second.key;
    coord.prepared_queue.enqueue(second);
    let handoff = rebase(&mut coord, key, t2, "A");
    // 交棒采到的是第二笔的 motion id（诊断 replaced_motion_id）。
    assert_eq!(handoff.caret_handoff.as_ref().unwrap().stage_id, b);
    let third = start(
        build_prepared_transaction(edit_spec(&mut coord, "A", "", handoff)).expect_created("third"),
        t2,
    );
    let c = IngestStageId(third.key.transaction_id);
    // 第三笔仍然是单一时长；不随按键次数增长（Issue #824 的验收目标）。
    assert_eq!(third.cursor_visual_track.as_ref().unwrap().duration_ms, 100);
    // 旧 carried 旧 glyph 全部收口到本笔 active motion；旧 stage 不再存在。
    assert!(
        third
            .units
            .iter()
            .all(|u| u.timing.is_caret_track() == (u.stage_id == Some(c))),
        "协同吞吐字必须全部属于本笔 stage"
    );
    assert!(third.units.iter().any(|u| u.stage_id == Some(c)));
    assert!(!third.units.iter().any(|u| u.stage_id == Some(a)));
    assert!(!third.units.iter().any(|u| u.stage_id == Some(b)));
    let frame = sample_transaction_visual_state(&third, t2);
    // 交棒瞬间：第二笔仍可见的所有旧 glyph（含 carried）几何不变——无跳变 retarget。
    for unit in &sampled_second.slices {
        assert_slice(carried_slice(&frame, unit.snapshot_id, unit.kind), unit);
    }
    // A 已完成而 B 还在播放时，下一轮只 carry B 和新 stage，不再留住 C 的纹理。
    // Issue #824：retarget 后的 motion 用本笔时长（100ms），所以把 rebase 放在
    // 第三笔 motion 播完之后，C 已到终态、不该再被 carry。此时没有可见旧 glyph，
    // 不走要求 carried 非空的 `rebase` helper。
    let key = third.key;
    coord.prepared_queue.enqueue(third);
    let handoff = coord.take_rebase_frames(
        &[key],
        "test_handoff",
        t2 + Duration::from_millis(150),
        None,
        "",
        1,
    );
    assert!(coord.prepared_queue.active_transactions().is_empty());
    assert!(!handoff.carried_units.iter().any(|u| u.stage_id == Some(a)));
    assert!(!handoff
        .carried_snapshot_ids
        .contains(&slice(&sampled_first, a).snapshot_id));
}

#[test]
fn issue819_comment5970185344_real_insert_carries_visible_text_without_replay() {
    let mut coord = LinuxEditorAnimationCoordinator::new();
    let now = Instant::now();
    let first = start(
        build_prepared_transaction(edit_spec(
            &mut coord,
            "A",
            "AB",
            RebaseVisualState::default(),
        ))
        .expect_created("first insert"),
        now,
    );
    let a = IngestStageId(first.key.transaction_id);
    let at = now + Duration::from_millis(50);
    let old_frame = sample_transaction_visual_state(&first, at);
    let carried = slice(&old_frame, a).clone();
    let key = first.key;
    coord.prepared_queue.enqueue(first);
    let handoff = rebase(&mut coord, key, at, "AB");
    let second = start(
        build_prepared_transaction(edit_spec(&mut coord, "AB", "ABC", handoff))
            .expect_created("second insert"),
        at,
    );
    // 交棒瞬间：carried B 的几何从采样帧继续，不重播。
    assert_slice(
        carried_slice(
            &sample_transaction_visual_state(&second, at),
            carried.snapshot_id,
            carried.kind,
        ),
        &carried,
    );
    let mut width = carried.dest_rect.w;
    for ms in 0..=50 {
        let frame = sample_transaction_visual_state(&second, at + Duration::from_millis(ms));
        let w = carried_slice(&frame, carried.snapshot_id, carried.kind)
            .dest_rect
            .w;
        assert!(w >= width - 1e-8, "B must continue revealing");
        width = w;
    }
}

#[test]
fn issue819_comment5970185344_forward_delete_preserves_local_boundary_phase() {
    let mut coord = LinuxEditorAnimationCoordinator::new();
    let now = Instant::now();
    let mut spec = edit_spec(&mut coord, "AB", "B", RebaseVisualState::default());
    spec.deleted_ranges = vec![(0, 1)];
    spec.old_cursor_rect = Some(caret(0.0, 0.0));
    spec.new_cursor_rect = Some(caret(0.0, 0.0));
    let first = start(
        build_prepared_transaction(spec).expect_created("forward delete"),
        now,
    );
    let a = IngestStageId(first.key.transaction_id);
    let at = now + Duration::from_millis(50);
    let old_frame = sample_transaction_visual_state(&first, at);
    let carried = slice(&old_frame, a).clone();
    let key = first.key;
    coord.prepared_queue.enqueue(first);
    let handoff = rebase(&mut coord, key, at, "B");
    let mut spec = edit_spec(&mut coord, "B", "", handoff);
    spec.old_cursor_rect = Some(caret(0.0, 0.0));
    spec.new_cursor_rect = Some(caret(0.0, 0.0));
    let second = start(
        build_prepared_transaction(spec).expect_created("second forward delete"),
        at,
    );
    assert_slice(
        carried_slice(
            &sample_transaction_visual_state(&second, at),
            carried.snapshot_id,
            carried.kind,
        ),
        &carried,
    );
    let mut width = carried.dest_rect.w;
    for ms in 0..=50 {
        let frame = sample_transaction_visual_state(&second, at + Duration::from_millis(ms));
        let w = carried_slice(&frame, carried.snapshot_id, carried.kind)
            .dest_rect
            .w;
        assert!(w <= width + 1e-8);
        width = w;
    }
}

fn multi_stage_track() -> PreparedCursorVisualTrack {
    let mut track = PreparedCursorVisualTrack::new_first(
        caret(0.0, 0.0),
        caret(30.0, 20.0),
        Some(0),
        Some(1),
        0.0,
        20.0,
        20.0,
        40.0,
        100,
        IngestStageId(7),
    );
    let kinds = [
        CaretTrackSegmentKind::LayoutHandoff,
        CaretTrackSegmentKind::IngestLine,
        CaretTrackSegmentKind::RowHandoff,
        CaretTrackSegmentKind::IngestLine,
    ];
    let points = [
        caret(0.0, 0.0),
        caret(10.0, 0.0),
        caret(0.0, 0.0),
        caret(10.0, 20.0),
        caret(30.0, 20.0),
    ];
    track.set_segments(
        kinds
            .into_iter()
            .enumerate()
            .map(|(i, kind)| CaretTrackSegment {
                kind,
                from: points[i],
                to: points[i + 1],
                ingest_line_ord: if i == 0 { None } else { Some(i / 2) },
                ingest_side: Some(if i < 3 {
                    IngestSnapshotSide::Old
                } else {
                    IngestSnapshotSide::New
                }),
                visual_line_id: Some(i / 2),
                ingest_stage_id: IngestStageId(7),
                distance_weight: [10.0, 30.0, 20.0, 40.0][i],
            })
            .collect(),
    );
    track
}

/// Issue #824 评论 5971089641 第 4 节：整条 motion 只有**一份**连续
/// progress/velocity。全局 progress 只做一次 easing，再按 `distance_weight`
/// 把路程映射到段；段内线性插值——不再对每段二次 `ease_out_cubic`。
#[test]
fn issue824_comment5971089641_single_global_easing_maps_distance_across_segments() {
    let track = multi_stage_track();
    let total: f64 = track.segments.iter().map(|s| s.distance_weight).sum();
    assert_eq!(total, 100.0);

    let expected_rect = |progress: f64| -> CursorRect {
        let eased = AnimatedSlice::ease_out_cubic(progress);
        let travelled = eased * total;
        let mut start = 0.0;
        for segment in &track.segments {
            let end = start + segment.distance_weight;
            if travelled < end {
                let local = (travelled - start) / segment.distance_weight;
                let top = segment.from.top + (segment.to.top - segment.from.top) * local;
                return CursorRect {
                    x: segment.from.x + (segment.to.x - segment.from.x) * local,
                    top,
                    bottom: top + (segment.to.bottom - segment.to.top),
                    baseline_y: segment.from.baseline_y
                        + (segment.to.baseline_y - segment.from.baseline_y) * local,
                };
            }
            start = end;
        }
        track.segments.last().unwrap().to
    };

    for progress in [0.0, 0.05, 0.25, 0.5, 0.8, 1.0] {
        let expected = expected_rect(progress);
        let actual = track.sampled_rect_at_progress(progress);
        for (a, b) in [
            (actual.x, expected.x),
            (actual.top, expected.top),
            (actual.bottom, expected.bottom),
            (actual.baseline_y, expected.baseline_y),
        ] {
            assert!(
                (a - b).abs() < 1e-8,
                "progress={progress}: {actual:?} != {expected:?}"
            );
        }
    }

    // 全局 easing 只应用一次：0.5 时 eased=0.875，路程 87.5 落在最后一段
    // 的 0.6875 处 → x = 10 + 20*0.6875 = 23.75。若仍按每段二次 easing，
    // 段内进度会变成 ease_out_cubic(0.6875)≈0.969，x 会跳到 ≈29.4。
    let at_half = track.sampled_rect_at_progress(0.5);
    assert!(
        (at_half.x - 23.75).abs() < 1e-8,
        "segment must map global distance linearly, got x={}",
        at_half.x
    );

    // 跨 segment 时位置连续：边界前后采样收敛到同一个交接点。
    let boundary_progress = {
        // 前三段路程 10+30+20 = 60 → eased = 0.6 的全局 progress。
        // 反解 ease_out_cubic(p) = 0.6：p = 1 - cbrt(0.4)。
        let p = 1.0 - (0.4f64).cbrt();
        p
    };
    let before = track.sampled_rect_at_progress(boundary_progress - 1e-6);
    let after = track.sampled_rect_at_progress(boundary_progress + 1e-6);
    assert!(
        (before.x - after.x).abs() < 1e-3 && (before.top - after.top).abs() < 1e-3,
        "velocity must stay continuous across segment boundary: {before:?} vs {after:?}"
    );
    assert_eq!(track.segments[2].to, track.segments[3].from);
}

#[test]
fn issue819_comment5970185344_new_multisegment_stage_keeps_full_time_budget() {
    let mut coord = LinuxEditorAnimationCoordinator::new();
    let now = Instant::now();
    let first = start(
        build_prepared_transaction(edit_spec(
            &mut coord,
            "ABC",
            "AB",
            RebaseVisualState::default(),
        ))
        .expect_created("delete"),
        now,
    );
    let a = IngestStageId(first.key.transaction_id);
    let at = now + Duration::from_millis(50);
    let key = first.key;
    coord.prepared_queue.enqueue(first);
    let handoff = rebase(&mut coord, key, at, "AB");
    let mut spec = edit_spec(&mut coord, "AB", "XY", handoff);
    spec.operation_kind = TextVisualOperationKind::CompositionCommitOrCancel;
    spec.inserted_ranges.clear();
    spec.deleted_ranges.clear();
    spec.composition_commit_crossfade = Some(CompositionCommitCrossfadeSpec {
        preedit_byte_start: 0,
        preedit_byte_end: 2,
        candidate_byte_start: 0,
        candidate_byte_end: 2,
    });
    let second = start(
        build_prepared_transaction(spec).expect_created("mixed IME"),
        at,
    );
    let b = IngestStageId(second.key.transaction_id);
    let track = second.cursor_visual_track.as_ref().unwrap();
    assert!(
        track
            .segments
            .iter()
            .filter(|s| s.ingest_stage_id == b)
            .count()
            > 1
    );
    // Issue #824 评论 5971089641 第 3/6 节：route 不再合成旧剩余段 + 新段，
    // 也不再有「旧段 50ms + 新段 100ms」的时间预算拼接——整条 motion 只有
    // 本笔的单一时长；旧 stage 一段都不留在新 route 里。
    assert_eq!(track.duration_ms, 100);
    assert!(track.segments.iter().all(|s| s.ingest_stage_id == b));
    assert!(track.segments.iter().all(|s| s.ingest_stage_id != a));
    // 段权重只表达路程：所有权重都是几何长度。
    let total: f64 = track.segments.iter().map(|s| s.distance_weight).sum();
    assert!(total > 0.0);
    // 采样到的 stage 始终是本笔 stage——carried 旧 unit 已挂到本笔 active motion。
    assert_eq!(
        sample_transaction_visual_state(&second, at + Duration::from_millis(49))
            .caret
            .unwrap()
            .ingest_stage_id,
        b
    );
    assert_eq!(
        sample_transaction_visual_state(&second, at + Duration::from_millis(50))
            .caret
            .unwrap()
            .ingest_stage_id,
        b
    );
}

/// Issue #824 评论 5971089641 的验收目标：连续删除无论按多快，活动正文运动
/// 都不能随按键次数无限增加。
///
/// 旧模型每按一次退格都会把旧 route 剩余段拼进新 route，track 时长单调增长
/// （100 → 150 → 375 → …）；新模型每次 retarget 只用本笔单一时长并替换旧 target。
#[test]
fn issue824_comment5971089641_repeated_delete_keeps_single_motion_duration() {
    let mut coord = LinuxEditorAnimationCoordinator::new();
    let mut old = "ABCDEFGHIJKLMNOP".to_string();
    let mut handoff = RebaseVisualState::default();
    let mut now = Instant::now();
    for _ in 0..12 {
        let new = old[..old.len() - 1].to_string();
        let tx = start(
            build_prepared_transaction(edit_spec(&mut coord, &old, &new, handoff))
                .expect_created("repeated delete"),
            now,
        );
        let duration = tx.cursor_visual_track.as_ref().unwrap().duration_ms;
        // 单一时长：不随按键次数增长。
        assert_eq!(
            duration, 100,
            "motion duration must stay at the single edit budget"
        );
        assert!(
            !tx.is_expired(now + Duration::from_millis(duration)),
            "must survive the complete motion"
        );
        let key = tx.key;
        coord.prepared_queue.enqueue(tx);
        // 同一时刻只有一笔活动正文事务（旧事务在 rebase 时已 cancel）。
        assert_eq!(coord.prepared_queue.active_transactions().len(), 1);
        now += Duration::from_millis(10);
        handoff = rebase(&mut coord, key, now, &new);
        assert!(coord.prepared_queue.active_transactions().is_empty());
        old = new;
    }
}

#[test]
fn issue819_comment5970185344_real_multiline_rebase_keeps_ingest_and_row_handoff_first_frames() {
    for kind in [
        CaretTrackSegmentKind::IngestLine,
        CaretTrackSegmentKind::RowHandoff,
    ] {
        let mut coord = LinuxEditorAnimationCoordinator::new();
        let now = Instant::now();
        let mut spec = edit_spec(&mut coord, "ABCD", "", RebaseVisualState::default());
        spec.old_snapshot = super::tests::make_multiline_snapshot(
            "ABCD",
            &[(0, 0, 2, 0.0, 20.0), (1, 2, 4, 20.0, 40.0)],
        );
        spec.old_cursor_rect = Some(caret(20.0, 20.0));
        spec.old_cursor_visual_line_id = Some(1);
        spec.old_cursor_line_top = 20.0;
        spec.old_cursor_line_bottom = 40.0;
        let first = start(
            build_prepared_transaction(spec).expect_created("multiline delete"),
            now,
        );
        let track = first.cursor_visual_track.as_ref().unwrap();
        let index = track.segments.iter().position(|s| s.kind == kind).unwrap();
        let elapsed = mid_segment_elapsed_ms(track, index);
        let at = now + Duration::from_millis(elapsed);
        let sampled = sample_transaction_visual_state(&first, at);
        let key = first.key;
        coord.prepared_queue.enqueue(first);
        let handoff = rebase(&mut coord, key, at, "");
        // Issue #824：交棒只保留当前屏幕采样点；不再提取旧 route 的剩余段。
        assert!(handoff.caret_handoff.is_some());
        let carried = handoff.carried_units.clone();
        let second = start(
            build_prepared_transaction(edit_spec(&mut coord, "", "E", handoff))
                .expect_created("after multiline delete"),
            at,
        );
        let new_stage = IngestStageId(second.key.transaction_id);
        let second_track = second.cursor_visual_track.as_ref().unwrap();
        // 新 route 从本帧采样点直接面向最新 target，且只属于本笔 stage。
        assert!(second_track
            .segments
            .iter()
            .all(|s| s.ingest_stage_id == new_stage));
        let frame = sample_transaction_visual_state(&second, at);
        assert_eq!(frame.caret.unwrap().rect, sampled.caret.unwrap().rect);
        // carried 旧 glyph 的几何在交棒瞬间保持不变（无跳变 retarget）。
        // 它们的 stage 已经重挂到本笔 motion，所以按 snapshot/kind 定位。
        for unit in carried {
            let actual = frame
                .slices
                .iter()
                .find(|s| {
                    s.snapshot_id == unit.slice.snapshot_id
                        && s.kind == unit.slice.kind
                        && s.shaping_identity == unit.slice.shaping_identity
                })
                .unwrap();
            assert_slice(actual, &unit.sampled_frame);
        }
    }
}

#[test]
fn issue819_comment5970185344_real_mixed_old_side_survives_midsegment_rebase() {
    let mut coord = LinuxEditorAnimationCoordinator::new();
    let now = Instant::now();
    let mut spec = edit_spec(&mut coord, "AB", "XY", RebaseVisualState::default());
    spec.operation_kind = TextVisualOperationKind::CompositionCommitOrCancel;
    spec.inserted_ranges.clear();
    spec.deleted_ranges.clear();
    spec.composition_commit_crossfade = Some(CompositionCommitCrossfadeSpec {
        preedit_byte_start: 0,
        preedit_byte_end: 2,
        candidate_byte_start: 0,
        candidate_byte_end: 2,
    });
    let first = start(
        build_prepared_transaction(spec).expect_created("mixed"),
        now,
    );
    let track = first.cursor_visual_track.as_ref().unwrap();
    let index = track
        .segments
        .iter()
        .position(|s| {
            s.kind == CaretTrackSegmentKind::IngestLine
                && s.ingest_side == Some(IngestSnapshotSide::Old)
        })
        .unwrap();
    let elapsed = mid_segment_elapsed_ms(track, index);
    let at = now + Duration::from_millis(elapsed);
    let sampled = sample_transaction_visual_state(&first, at);
    let key = first.key;
    coord.prepared_queue.enqueue(first);
    let handoff = rebase(&mut coord, key, at, "XY");
    // Issue #824：交棒不再携带旧 route 剩余段，只保留当前屏幕采样。
    assert!(handoff.caret_handoff.is_some());
    assert_eq!(
        handoff.caret_handoff.as_ref().unwrap().sampled,
        sampled.caret.unwrap().rect
    );
    let carried = handoff.carried_units.clone();
    let second = start(
        build_prepared_transaction(edit_spec(&mut coord, "XY", "XYZ", handoff))
            .expect_created("after mixed"),
        at,
    );
    let new_stage = IngestStageId(second.key.transaction_id);
    let second_track = second.cursor_visual_track.as_ref().unwrap();
    // 新 route 全部属于本笔 stage；旧 stage 一段都不再排进新动画。
    assert!(second_track
        .segments
        .iter()
        .all(|s| s.ingest_stage_id == new_stage));
    let frame = sample_transaction_visual_state(&second, at);
    // retarget 后的第一帧 caret 仍然等于交棒采样点：无跳变。
    assert_eq!(frame.caret.unwrap().rect, sampled.caret.unwrap().rect);
    for unit in carried {
        let actual = frame
            .slices
            .iter()
            .find(|s| {
                s.kind == unit.slice.kind
                    && s.shaping_identity == unit.slice.shaping_identity
                    && s.snapshot_id == unit.slice.snapshot_id
            })
            .unwrap();
        assert_slice(actual, &unit.sampled_frame);
    }
}

#[test]
fn issue819_comment5970185344_composition_commit_retains_earlier_texture_until_new_owner() {
    use crate::sujian_editor_item::pipeline::LinuxEditorPipeline;
    let mut pipeline = LinuxEditorPipeline::new();
    let now = Instant::now();
    let mut spec = edit_spec(
        pipeline.animation_coordinator_mut(),
        "A",
        "AB",
        RebaseVisualState::default(),
    );
    spec.operation_kind = TextVisualOperationKind::CompositionUpdate;
    let first = start(
        build_prepared_transaction(spec).expect_created("preedit update"),
        now,
    );
    let carried_id = first.units[0].slice.snapshot_id;
    pipeline
        .texture_cache_mut()
        .insert_line(carried_id, qmetaobject::QImage::default());
    pipeline
        .animation_coordinator_mut()
        .prepared_queue
        .enqueue(first);
    let at = now + Duration::from_millis(50);
    // 新提交两份快照均不含先前动画的纹理 id，后续 prepare 不能重新生成它。
    let old = snapshot("AB", 1000);
    let new = snapshot("XY", 1001);
    assert_ne!(carried_id, old.line_snapshots[0].id);
    assert_ne!(carried_id, new.line_snapshots[0].id);
    let handoff = pipeline
        .animation_coordinator_mut()
        .prepare_composition_commit_handoff(&old, &new, 0, 2, true, 0, 2, 0, 1, 1, at);
    assert!(pipeline
        .animation_coordinator()
        .prepared_queue
        .active_transactions()
        .is_empty());
    assert!(handoff
        .visual_state
        .carried_snapshot_ids
        .contains(&carried_id));
    pipeline.retain_handoff_textures(&handoff.visual_state.carried_snapshot_ids);
    assert!(pipeline.texture_cache().contains_line(&carried_id));
    let outcome = pipeline
        .animation_coordinator_mut()
        .handle_composition_commit_or_cancel(
            &old,
            &new,
            0,
            2,
            true,
            false,
            crate::sujian_editor_item::animation::composition::CompositionCommitBodyRanges {
                inserted: vec![(0, 2)],
                deleted: vec![(0, 2)],
            },
            0,
            2,
            0,
            1,
            Some(caret(20.0, 0.0)),
            Some(caret(20.0, 0.0)),
            Some(0),
            Some(0),
            0.0,
            20.0,
            0.0,
            20.0,
            1,
            LayoutRevision::initial(),
            at,
            Some(handoff),
            true,
            true,
            true,
        );
    let HandoffTransactionOutcome::Created(key) = outcome else {
        panic!("commit must create transaction: {outcome:?}")
    };
    let tx = pipeline
        .animation_coordinator()
        .prepared_queue
        .active_transactions()
        .iter()
        .find(|t| t.key == key)
        .unwrap();
    assert!(tx.snapshot_ids().contains(&carried_id));
    pipeline.retain_handoff_textures(&[]);
    assert!(pipeline.texture_cache().contains_line(&carried_id));
    pipeline
        .animation_coordinator_mut()
        .prepared_queue
        .cancel(key, "test completed");
    pipeline.retain_handoff_textures(&[]);
    assert!(!pipeline.texture_cache().contains_line(&carried_id));
}

#[test]
fn issue819_comment5970185344_composition_commit_preserves_empty_builder_skip_reason() {
    let mut coord = LinuxEditorAnimationCoordinator::new();
    let empty = snapshot("", 1);
    let outcome = coord.handle_composition_commit_or_cancel(
        &empty,
        &empty,
        0,
        0,
        true,
        true,
        crate::sujian_editor_item::animation::composition::CompositionCommitBodyRanges {
            inserted: vec![(0, 0)],
            deleted: vec![(0, 0)],
        },
        0,
        0,
        0,
        0,
        None,
        None,
        None,
        None,
        0.0,
        20.0,
        0.0,
        20.0,
        1,
        LayoutRevision::initial(),
        Instant::now(),
        None,
        true,
        false,
        false,
    );
    assert_eq!(
        outcome,
        HandoffTransactionOutcome::Skipped(
            crate::sujian_editor_item::edit_flow::EditVisualSkipReason::BuilderEmptyTransaction
        )
    );
    assert!(coord.prepared_queue.active_transactions().is_empty());
}
