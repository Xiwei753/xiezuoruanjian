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
    assert_eq!(handoff.carried_units[0].stage_id, Some(a));
    let second = start(
        build_prepared_transaction(edit_spec(&mut coord, "AB", "A", handoff))
            .expect_created("second"),
        t1,
    );
    let b = IngestStageId(second.key.transaction_id);
    let track = second.cursor_visual_track.as_ref().unwrap();
    assert_eq!(track.duration_ms, 150);
    assert_eq!(track.segments.len(), 2, "no zero length layout handoff");
    assert_eq!(track.segments[0].duration_weight_ms, 50.0);
    assert_eq!(track.segments[1].duration_weight_ms, 100.0);
    assert_slice(
        slice(&sample_transaction_visual_state(&second, t1), a),
        slice(&sampled_first, a),
    );
    let mut width = slice(&sampled_first, a).dest_rect.w;
    for ms in 0..=50 {
        let frame = sample_transaction_visual_state(&second, t1 + Duration::from_millis(ms));
        let w = slice(&frame, a).dest_rect.w;
        assert!(w <= width + 1e-8, "C must continue closing");
        width = w;
        if ms < 50 {
            assert!((slice(&frame, b).dest_rect.w - 10.0).abs() < 1e-8);
        }
    }
    let t2 = t1 + Duration::from_millis(25);
    let sampled_second = sample_transaction_visual_state(&second, t2);
    let key = second.key;
    coord.prepared_queue.enqueue(second);
    let handoff = rebase(&mut coord, key, t2, "A");
    let stages: Vec<_> = handoff
        .carried_units
        .iter()
        .map(|u| u.stage_id.unwrap())
        .collect();
    assert!(stages.contains(&a) && stages.contains(&b));
    assert_eq!(
        handoff.caret_handoff.as_ref().unwrap().stage_id,
        a,
        "sampled stage differs from newest transaction stage"
    );
    let third = start(
        build_prepared_transaction(edit_spec(&mut coord, "A", "", handoff)).expect_created("third"),
        t2,
    );
    assert_eq!(third.cursor_visual_track.as_ref().unwrap().duration_ms, 225);
    let frame = sample_transaction_visual_state(&third, t2);
    assert_slice(slice(&frame, a), slice(&sampled_second, a));
    assert_slice(slice(&frame, b), slice(&sampled_second, b));
    for stage in [a, b] {
        assert!(
            third.units.iter().any(|u| u.stage_id == Some(stage)),
            "stage must survive third edit"
        );
    }
    // A 已完成而 B 还在播放时，下一轮只 carry B 和新 stage，不再留住 C 的纹理。
    let key = third.key;
    coord.prepared_queue.enqueue(third);
    let handoff = rebase(&mut coord, key, t2 + Duration::from_millis(50), "");
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
    let key = first.key;
    coord.prepared_queue.enqueue(first);
    let handoff = rebase(&mut coord, key, at, "AB");
    let second = start(
        build_prepared_transaction(edit_spec(&mut coord, "AB", "ABC", handoff))
            .expect_created("second insert"),
        at,
    );
    assert_slice(
        slice(&sample_transaction_visual_state(&second, at), a),
        slice(&old_frame, a),
    );
    let mut width = slice(&old_frame, a).dest_rect.w;
    for ms in 0..=50 {
        let frame = sample_transaction_visual_state(&second, at + Duration::from_millis(ms));
        let w = slice(&frame, a).dest_rect.w;
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
        slice(&sample_transaction_visual_state(&second, at), a),
        slice(&old_frame, a),
    );
    let mut width = slice(&old_frame, a).dest_rect.w;
    for ms in 0..=50 {
        let frame = sample_transaction_visual_state(&second, at + Duration::from_millis(ms));
        let w = slice(&frame, a).dest_rect.w;
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
                duration_weight_ms: [10.0, 30.0, 20.0, 40.0][i],
                ingest_start_progress: 0.0,
            })
            .collect(),
    );
    track
}

#[test]
fn issue819_comment5970185344_remaining_multiline_ingest_and_handoffs_keep_metadata() {
    let track = multi_stage_track();
    for (progress, index, expected_ms) in [
        (0.05, 0, 5.0),
        (0.25, 1, 15.0),
        (0.50, 2, 10.0),
        (0.80, 3, 20.0),
    ] {
        let remaining = track.remaining_segments_from(progress);
        let current = track.segments[index];
        assert_eq!(remaining.len(), track.segments.len() - index);
        assert_eq!(remaining[0].from, track.sampled_rect_at_progress(progress));
        assert_eq!(
            remaining[0].to, current.to,
            "must end at current segment, not entire route"
        );
        assert_eq!(remaining[0].kind, current.kind);
        assert_eq!(remaining[0].ingest_side, current.ingest_side);
        assert_eq!(remaining[0].ingest_stage_id, current.ingest_stage_id);
        assert_eq!(remaining[0].ingest_line_ord, current.ingest_line_ord);
        assert_eq!(remaining[0].visual_line_id, current.visual_line_id);
        assert!((remaining[0].duration_weight_ms - expected_ms).abs() < 1e-8);
        for pair in remaining.windows(2) {
            assert_eq!(pair[0].to, pair[1].from);
        }
        let mut rebased = track.clone();
        rebased.segments = remaining;
        assert_eq!(
            rebased.sampled_rect_at_progress(0.0),
            track.sampled_rect_at_progress(progress)
        );
    }
    assert!(track.remaining_segments_from(1.0).is_empty());
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
    assert_eq!(track.duration_ms, 150);
    let budget = |stage| {
        track
            .segments
            .iter()
            .filter(|s| s.ingest_stage_id == stage)
            .map(|s| s.duration_weight_ms)
            .sum::<f64>()
    };
    assert_eq!(budget(a), 50.0);
    assert!((budget(b) - 100.0).abs() < 1e-8);
    assert_eq!(
        sample_transaction_visual_state(&second, at + Duration::from_millis(49))
            .caret
            .unwrap()
            .ingest_stage_id,
        a
    );
    assert_eq!(
        sample_transaction_visual_state(&second, at + Duration::from_millis(50))
            .caret
            .unwrap()
            .ingest_stage_id,
        b
    );
    assert_eq!(
        second.timeline.duration_ms, 100,
        "transaction clock remains independent of the composed caret route"
    );
}

#[test]
fn issue819_comment5970185344_long_carried_route_does_not_expire_on_single_edit_clock() {
    let mut coord = LinuxEditorAnimationCoordinator::new();
    let mut old = "ABCDEFGHIJKLMNOP".to_string();
    let mut handoff = RebaseVisualState::default();
    let mut now = Instant::now();
    for index in 0..12 {
        let new = old[..old.len() - 1].to_string();
        let tx = start(
            build_prepared_transaction(edit_spec(&mut coord, &old, &new, handoff))
                .expect_created("repeated delete"),
            now,
        );
        let duration = tx.cursor_visual_track.as_ref().unwrap().duration_ms;
        assert!(
            !tx.is_expired(now + Duration::from_millis(duration)),
            "must survive the complete carried route"
        );
        if index == 11 {
            assert!(
                duration > 800,
                "exercise a route longer than the old timeout"
            );
            break;
        }
        let key = tx.key;
        coord.prepared_queue.enqueue(tx);
        now += Duration::from_millis(10);
        handoff = rebase(&mut coord, key, now, &new);
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
        let elapsed: f64 = track.segments[..index]
            .iter()
            .map(|s| s.duration_weight_ms)
            .sum::<f64>()
            + track.segments[index].duration_weight_ms / 2.0;
        let at = now + Duration::from_millis(elapsed as u64);
        let sampled = sample_transaction_visual_state(&first, at);
        let key = first.key;
        coord.prepared_queue.enqueue(first);
        let handoff = rebase(&mut coord, key, at, "");
        let remaining = &handoff
            .caret_handoff
            .as_ref()
            .unwrap()
            .remaining_ingest_segments;
        assert_eq!(remaining[0].kind, kind);
        let carried = handoff.carried_units.clone();
        let second = start(
            build_prepared_transaction(edit_spec(&mut coord, "", "E", handoff))
                .expect_created("after multiline delete"),
            at,
        );
        let frame = sample_transaction_visual_state(&second, at);
        assert_eq!(frame.caret.unwrap().rect, sampled.caret.unwrap().rect);
        for unit in carried {
            let actual = frame
                .slices
                .iter()
                .find(|s| {
                    s.unit_stage_id == unit.stage_id && s.snapshot_id == unit.slice.snapshot_id
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
    let elapsed: f64 = track.segments[..index]
        .iter()
        .map(|s| s.duration_weight_ms)
        .sum::<f64>()
        + track.segments[index].duration_weight_ms / 2.0;
    let at = now + Duration::from_millis(elapsed as u64);
    let sampled = sample_transaction_visual_state(&first, at);
    let key = first.key;
    coord.prepared_queue.enqueue(first);
    let handoff = rebase(&mut coord, key, at, "XY");
    assert_eq!(
        handoff
            .caret_handoff
            .as_ref()
            .unwrap()
            .remaining_ingest_segments[0]
            .ingest_side,
        Some(IngestSnapshotSide::Old)
    );
    let carried = handoff.carried_units.clone();
    let second = start(
        build_prepared_transaction(edit_spec(&mut coord, "XY", "XYZ", handoff))
            .expect_created("after mixed"),
        at,
    );
    let frame = sample_transaction_visual_state(&second, at);
    assert_eq!(
        frame.caret.unwrap().ingest_side,
        Some(IngestSnapshotSide::Old)
    );
    assert_eq!(frame.caret.unwrap().rect, sampled.caret.unwrap().rect);
    for unit in carried {
        let actual = frame
            .slices
            .iter()
            .find(|s| {
                s.unit_stage_id == unit.stage_id
                    && s.kind == unit.slice.kind
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
