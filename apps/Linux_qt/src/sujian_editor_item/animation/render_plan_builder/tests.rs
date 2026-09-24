use super::super::coordinator::LinuxEditorAnimationCoordinator;
use super::*;
use crate::sujian_editor_item::animated_slice::{AnimatedSlice, AnimatedSliceKind};
use crate::sujian_editor_item::animation::cursor_motion::sample_coordinated_cursor_rect_at;
use crate::sujian_editor_item::animation::rebase::match_rebase_frames;
use crate::sujian_editor_item::animation::transaction_builder::build_delete_conceal_slices;
use crate::sujian_editor_item::animation::{
    PreparedCursorVisualTrack, PreparedTextVisualTransaction, PreparedVisualUnit, RebaseFrame,
    TransactionTimeline, VisualUnitTiming,
};
use crate::sujian_editor_item::animation_mode::AnimationMode;
use crate::sujian_editor_item::edit_motion::CursorRect;
use crate::sujian_editor_item::layout_snapshot::{LineSnapshotId, ShapingIdentity, SourceRect};
use crate::sujian_editor_item::render_plan::{
    CoordinatedMotionFrame, CursorRenderState, CursorStyle, FrameContext, SampledCaretFrame,
    SelectionPreeditPlan, SelectionPreeditStyle,
};
use crate::sujian_editor_item::transaction_key::VisualTransactionKey;
use std::time::Duration;
use writer_core::editor::{OffsetMap, Utf8ByteOffset};

/// `match_rebase_frames` 现在作用在视觉单元上（Issue #690 评论 5675007226 步骤 3）。
fn wrap_units(slices: Vec<AnimatedSlice>) -> Vec<PreparedVisualUnit> {
    slices
        .into_iter()
        .map(|s| PreparedVisualUnit::wrap(s, 100))
        .collect()
}

fn reveal_slice(byte_start: usize, byte_end: usize, x: f64, w: f64) -> AnimatedSlice {
    AnimatedSlice::insert_reveal(
        VisualTransactionKey::new(1, 1),
        LineSnapshotId::new(1, 0, 0),
        SourceRect {
            x: 0.0,
            y: 0.0,
            w,
            h: 20.0,
        },
        SourceRect {
            x,
            y: 0.0,
            w,
            h: 20.0,
        },
        x,
        0.0,
        byte_start,
        byte_end,
        None,
        None,
    )
}

fn conceal_slice(
    byte_start: usize,
    byte_end: usize,
    x: f64,
    w: f64,
    conceal_to_left_edge: bool,
) -> AnimatedSlice {
    AnimatedSlice::delete_conceal(
        VisualTransactionKey::new(1, 1),
        LineSnapshotId::new(1, 0, 0),
        SourceRect {
            x: 0.0,
            y: 0.0,
            w,
            h: 20.0,
        },
        SourceRect {
            x,
            y: 0.0,
            w,
            h: 20.0,
        },
        x,
        0.0,
        byte_start,
        byte_end,
        None,
        conceal_to_left_edge,
        None,
    )
}

fn reflow_slice(byte_start: usize, byte_end: usize, from_x: f64, to_x: f64) -> AnimatedSlice {
    AnimatedSlice::reflow_move(
        VisualTransactionKey::new(1, 1),
        LineSnapshotId::new(1, 0, 0),
        SourceRect {
            x: 0.0,
            y: 0.0,
            w: 30.0,
            h: 20.0,
        },
        SourceRect {
            x: from_x,
            y: 0.0,
            w: 30.0,
            h: 20.0,
        },
        LineSnapshotId::new(1, 0, 0),
        SourceRect {
            x: 0.0,
            y: 0.0,
            w: 30.0,
            h: 20.0,
        },
        SourceRect {
            x: to_x,
            y: 0.0,
            w: 30.0,
            h: 20.0,
        },
        byte_start,
        byte_end,
        None,
    )
}

fn caret(x: f64) -> CursorRect {
    CursorRect {
        x,
        top: 0.0,
        bottom: 20.0,
        baseline_y: 16.0,
    }
}

/// 已经演进了 `elapsed_ms` 的视觉单元（单元自己的时间线，与事务 timeline 无关）。
fn elapsed_unit(
    slice: AnimatedSlice,
    elapsed_ms: u64,
    duration_ms: u64,
    now: Instant,
) -> PreparedVisualUnit {
    let mut unit = PreparedVisualUnit::wrap(slice, duration_ms);
    // Issue #727 约束 2: 通过 VisualUnitTiming 设置 started_at / start_fraction。
    // CaretDriven unit 无 started_at，通过 start_fraction 模拟已吐/吞比例。
    // Timed unit 通过 started_at 设置独立时间线。
    let fraction = if duration_ms > 0 {
        (elapsed_ms as f64 / duration_ms as f64).clamp(0.0, 1.0)
    } else {
        0.0
    };
    match &mut unit.timing {
        VisualUnitTiming::Timed { started_at, .. } => {
            *started_at = Some(now - Duration::from_millis(elapsed_ms));
        }
        VisualUnitTiming::CaretDriven { .. } => {
            // Issue #727 约束 2: CaretDriven unit 的 visible_fraction 从 caret track
            // progress 推导（start + (target - start) * ease_out_quad(progress)），
            // 不需要通过 start_fraction 模拟已演进状态。
            // start_fraction 保持 fresh unit 的初始值（0 for InsertReveal, 1 for DeleteConceal）。
            // 测试中 caret track 的 started_at 由 rendering_tx 设置，反映已演进状态。
        }
    }
    unit
}

/// 手工装配一笔处于 Rendering 的正文事务。事务 timeline 从 `tx_elapsed_ms` 起算，
/// 与单元各自的 `elapsed_ms` 故意取不同值，用来验证两者不再互相顶替。
/// Issue #727 约束 3: 自动创建 `cursor_visual_track`，使 CaretDriven unit 可以
/// 从 caret track progress 推导可见比例。`started_at` 与事务 timeline 同步。
fn rendering_tx(
    key: VisualTransactionKey,
    operation_kind: TextVisualOperationKind,
    units: Vec<PreparedVisualUnit>,
    old_cursor: CursorRect,
    new_cursor: CursorRect,
    now: Instant,
    tx_elapsed_ms: u64,
) -> PreparedTextVisualTransaction {
    let mut timeline = TransactionTimeline::new(100);
    timeline.rendering_started_at = Some(now - Duration::from_millis(tx_elapsed_ms));
    // Issue #727 约束 3: CaretDriven unit 依赖 caret motion track。
    // 测试辅助函数自动创建 cursor_visual_track，使 CaretDriven unit 可以生成 glyph。
    let cursor_visual_track = Some(PreparedCursorVisualTrack {
        from: old_cursor.clone(),
        to: new_cursor.clone(),
        from_visual_line_id: None,
        to_visual_line_id: None,
        from_line_top: 0.0,
        from_line_bottom: 20.0,
        to_line_top: 0.0,
        to_line_bottom: 20.0,
        started_at: Some(now - Duration::from_millis(tx_elapsed_ms)),
        duration_ms: 100,
        pause_start: None,
    });
    PreparedTextVisualTransaction {
        key,
        state: TextVisualTransactionState::Rendering,
        operation_kind,
        timeline,
        units,
        old_cursor_rect: Some(old_cursor),
        new_cursor_rect: Some(new_cursor),
        cursor_visual_track,
        cancel_reason: None,
        texture_prepared: true,
        old_snapshot: None,
        new_snapshot: None,
        cursor_owner_epoch: 0,
        caret_motion_retired: false,
        coordinated: false,
        visual_affected_byte_range_old: None,
        visual_affected_byte_range_new: None,
        layout_basis_revision: LayoutRevision::initial(),
    }
}

fn stale_cursor_state() -> CursorRenderState {
    CursorRenderState {
        visible: true,
        x: 1234.5,
        y: 999.0,
        h: 20.0,
        opacity: 0.0,
    }
}

#[test]
fn issue690_render_plan_cursor_sits_on_reveal_boundary_of_same_frame() {
    let now = Instant::now();
    let mut coord = LinuxEditorAnimationCoordinator::new();
    coord.prepared_queue.enqueue(rendering_tx(
        VisualTransactionKey::new(3, 3),
        TextVisualOperationKind::Insert,
        vec![elapsed_unit(reveal_slice(0, 3, 100.0, 60.0), 50, 100, now)],
        caret(100.0),
        caret(160.0),
        now,
        50,
    ));

    let plan = coord.build_render_plan_full(
        stale_cursor_state(),
        SelectionPreeditPlan::default(),
        FrameContext::default(),
        CursorStyle::default(),
        SelectionPreeditStyle::default(),
        now,
        None,
        0,
        0.0,
    );

    assert_eq!(plan.text_animation.glyphs.len(), 1);
    let glyph = &plan.text_animation.glyphs[0];
    let text_right_edge = glyph.x + glyph.w;
    assert!(
        (text_right_edge - 145.0).abs() < 1e-6,
        "本帧文字右边界应为 100 + 60*0.75，got {}",
        text_right_edge
    );
    assert!(
        (plan.cursor.x - text_right_edge).abs() < 1e-6,
        "光标必须落在同一帧的文字吞吐边界上，got cursor={} text_right={}",
        plan.cursor.x,
        text_right_edge
    );
    assert!(
        (plan.cursor.x - 1234.5).abs() > 1.0,
        "正文事务期间不再把 GUI 线程留下的 visual_x 当最终屏幕坐标"
    );
    assert_eq!(plan.cursor.y, glyph.y);
    assert!(
        (plan.cursor.opacity - 1.0).abs() < 1e-6,
        "Insert 期间光标闪烁抑制，恒为不透明"
    );
}

#[test]
fn issue690_render_plan_keeps_cursor_only_state_when_coordinated_disabled() {
    // Issue #727 约束 6: 删除 coordinated_text_cursor_animation_enabled 独立开关。
    // 是否有吞吐字直接由"本帧有没有有效 caret motion"决定。
    // 无 cursor_visual_track = 无 caret motion = 走 CursorOnly 自己的平滑曲线。
    let now = Instant::now();
    let mut coord = LinuxEditorAnimationCoordinator::new();
    // 手工装配一笔无 cursor_visual_track 的 Rendering 事务（模拟 caret motion 丢失）。
    let mut tx = rendering_tx(
        VisualTransactionKey::new(3, 3),
        TextVisualOperationKind::Insert,
        vec![elapsed_unit(reveal_slice(0, 3, 100.0, 60.0), 50, 100, now)],
        caret(100.0),
        caret(160.0),
        now,
        50,
    );
    tx.cursor_visual_track = None;
    coord.prepared_queue.enqueue(tx);

    let plan = coord.build_render_plan_full(
        stale_cursor_state(),
        SelectionPreeditPlan::default(),
        FrameContext::default(),
        CursorStyle::default(),
        SelectionPreeditStyle::default(),
        now,
        None,
        0,
        0.0,
    );

    assert!(
        (plan.cursor.x - 1234.5).abs() < 1e-6,
        "无 caret motion 时走 CursorOnly 自己的平滑曲线，位置不由事务改写"
    );
    assert!(
        plan.cursor.opacity < 1e-6,
        "CursorOnly 链保留 caller 传入的 blink opacity"
    );
}

#[test]
fn issue690_fresh_conceal_unit_runs_from_fully_visible() {
    let now = Instant::now();
    let fresh = PreparedVisualUnit::wrap(conceal_slice(0, 3, 100.0, 60.0, true), 100);
    assert!(
        (fresh.current_visible_fraction(now) - 1.0).abs() < 1e-6,
        "吞字单元第一帧必须完整可见，否则被删的字一帧都不出现",
    );

    // Issue #727 约束 2: CaretDriven unit 的 visible_fraction 从 caret track progress 推导。
    // start_fraction=1.0, target_fraction=0.0, progress=0.5
    // → visible = 1 + (0-1)*ease_out_quad(0.5) = 1 - 0.75 = 0.25
    let half = elapsed_unit(conceal_slice(0, 3, 100.0, 60.0, true), 50, 100, now);
    let visible = 1.0 + (0.0 - 1.0) * AnimatedSlice::ease_out_quad(0.5);
    assert!(
        (visible - 0.25).abs() < 1e-6,
        "吞字比例必须走与吐字同一条曲线（镜像），got {}",
        visible
    );
    let frame = half.slice.compute_frame(visible);
    assert!(
        (frame.w - 15.0).abs() < 1e-6,
        "演到一半时可见宽度 = 60 * 0.25，got {}",
        frame.w
    );
    // Backspace 保留左段：右边界 160 → 115，前半程已扫过 45px（ease-out 减速）。
    assert!(
        (160.0 - (frame.x + frame.w)) > (frame.x + frame.w - 100.0),
        "吞字边界应先快后慢地逼近终点，got edge={}",
        frame.x + frame.w
    );

    // caret track progress=1.0 → visible = 1 + (0-1)*ease_out_quad(1.0) = 0
    let done = elapsed_unit(conceal_slice(0, 3, 100.0, 60.0, true), 200, 100, now);
    let done_visible = 1.0 + (0.0 - 1.0) * AnimatedSlice::ease_out_quad(1.0);
    let frame = done.slice.compute_frame(done_visible);
    assert!(frame.w.abs() < 1e-6, "播完后旧字彻底消失，got {}", frame.w);
}

#[test]
fn issue690_backspace_cursor_tracks_shrinking_conceal_edge() {
    let now = Instant::now();
    let mut coord = LinuxEditorAnimationCoordinator::new();
    // Backspace：保留左段，可见宽度 60 → 15，右边界 160 → 115 往左走。
    coord.prepared_queue.enqueue(rendering_tx(
        VisualTransactionKey::new(4, 4),
        TextVisualOperationKind::Delete,
        vec![elapsed_unit(
            conceal_slice(100, 103, 100.0, 60.0, true),
            50,
            100,
            now,
        )],
        caret(160.0),
        caret(100.0),
        now,
        50,
    ));

    let plan = coord.build_render_plan_full(
        stale_cursor_state(),
        SelectionPreeditPlan::default(),
        FrameContext::default(),
        CursorStyle::default(),
        SelectionPreeditStyle::default(),
        now,
        None,
        0,
        0.0,
    );
    assert!(
        (plan.cursor.x - 115.0).abs() < 1e-6,
        "Backspace 光标跟着正在被吞掉的右边界（100 + 60*0.25），got {}",
        plan.cursor.x
    );
    let glyph = &plan.text_animation.glyphs[0];
    assert!(
        (plan.cursor.x - (glyph.x + glyph.w)).abs() < 1e-6,
        "光标与文字帧来自同一个采样点"
    );
    assert!(
        plan.cursor.opacity < 1e-6,
        "Delete 不抑制闪烁，blink 状态由 caller 决定"
    );
}

#[test]
fn issue690_forward_delete_cursor_stays_pinned_at_new_caret() {
    let now = Instant::now();
    let mut coord = LinuxEditorAnimationCoordinator::new();
    // 前向 Delete：保留右段，逻辑光标本来不动，右侧文字向光标收。
    coord.prepared_queue.enqueue(rendering_tx(
        VisualTransactionKey::new(5, 5),
        TextVisualOperationKind::Delete,
        vec![elapsed_unit(
            conceal_slice(100, 103, 100.0, 60.0, false),
            50,
            100,
            now,
        )],
        caret(100.0),
        caret(100.0),
        now,
        50,
    ));

    let plan = coord.build_render_plan_full(
        stale_cursor_state(),
        SelectionPreeditPlan::default(),
        FrameContext::default(),
        CursorStyle::default(),
        SelectionPreeditStyle::default(),
        now,
        None,
        0,
        0.0,
    );
    assert!(
        (plan.cursor.x - 100.0).abs() < 1e-6,
        "前向 Delete 光标固定在 new caret，不回抽，got {}",
        plan.cursor.x
    );
}

#[test]
fn issue690_cursor_without_boundary_glyph_uses_reflow_easing() {
    let now = Instant::now();
    let mut coord = LinuxEditorAnimationCoordinator::new();
    // 跨行/软换行 reflow：没有可直接当边界的 reveal/conceal 单元，
    // 走 caret track 插值，easing 与 ReflowMove 同为二次曲线。
    // Issue #690 评论 5681206040: caret track 自带 started_at/duration_ms，
    // 不再借 reflow unit 的 progress。
    let mut tx = rendering_tx(
        VisualTransactionKey::new(6, 6),
        TextVisualOperationKind::Insert,
        vec![elapsed_unit(reflow_slice(0, 3, 100.0, 200.0), 50, 100, now)],
        CursorRect {
            x: 100.0,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        },
        CursorRect {
            x: 200.0,
            top: 40.0,
            bottom: 60.0,
            baseline_y: 56.0,
        },
        now,
        10,
    );
    // caret track 与 reflow unit 同一条时间线：started_at = now - 50ms, duration = 100ms
    // Issue #690 评论 5682867529: new_first 不再接受 now 参数（始终 started_at = None）。
    // 测试要模拟"已经播了 50ms"的场景，直接用结构体字面量设置 started_at = Some(...)。
    tx.cursor_visual_track = Some(PreparedCursorVisualTrack {
        from: CursorRect {
            x: 100.0,
            top: 0.0,
            bottom: 20.0,
            baseline_y: 16.0,
        },
        to: CursorRect {
            x: 200.0,
            top: 40.0,
            bottom: 60.0,
            baseline_y: 56.0,
        },
        from_visual_line_id: None,
        to_visual_line_id: None,
        from_line_top: 0.0,
        from_line_bottom: 0.0,
        to_line_top: 0.0,
        to_line_bottom: 0.0,
        started_at: Some(now - Duration::from_millis(50)),
        duration_ms: 100,
        pause_start: None,
    });
    coord.prepared_queue.enqueue(tx);

    let plan = coord.build_render_plan_full(
        stale_cursor_state(),
        SelectionPreeditPlan::default(),
        FrameContext::default(),
        CursorStyle::default(),
        SelectionPreeditStyle::default(),
        now,
        None,
        0,
        0.0,
    );
    // caret track 演了 50/100ms → progress 0.5 → ease_out_quad = 0.75
    // → x = 100 + 100*0.75 = 175
    assert!(
        (plan.cursor.x - 175.0).abs() < 1e-6,
        "无边界 glyph 时用 caret track 的 progress 插值，got {}",
        plan.cursor.x
    );
    assert!(
        (plan.cursor.y - 30.0).abs() < 1e-6,
        "y 同一条曲线（0 + 40*0.75 = 30），got {}",
        plan.cursor.y
    );
    assert!(
        (plan.cursor.h - 20.0).abs() < 1e-6,
        "光标高度取 new caret 行高"
    );
}

#[test]
fn issue690_comment5680276931_rebase_reflow_cursor_starts_from_screen_cursor_not_logical_old_caret()
{
    // 评论 5680276931 指出：take_rebase_frames() 交棒时只采集文字视觉单元，
    // 没有把旧事务这一帧正在屏幕上显示的 coordinated cursor rect 带给新事务。
    // 新事务的 old_cursor_rect 仍来自 pipeline 对旧正文做的权威布局 caret
    // （逻辑 old caret），不是旧动画当前显示到的位置。
    // compute_coordinated_cursor_position() 在 Enter/删除换行/纯 reflow 这类
    // 没有 InsertReveal/DeleteConceal glyph 当边界的场景，走 old/new caret 插值，
    // rebase 后第一帧 progress=0 → cursor.x = old_cursor_rect.x（逻辑 old caret），
    // 而文字 reflow unit 的 from_document_rect 已被 rebase 成屏幕位置 → 文字不跳。
    // 结果：文字保持在屏幕位置，光标却瞬间跳回逻辑 old caret 再往新位置走。
    let now = Instant::now();
    let mut coord = LinuxEditorAnimationCoordinator::new();

    // ── 旧事务：一个正在播放的 reflow unit，屏幕光标已离开 old_cursor_rect ──
    // old_cursor_rect = 100（逻辑 old caret），new_cursor_rect = 220
    // reflow unit 演了 50/100ms → progress 0.5 → ease_out_quad(0.5) = 0.75
    // 屏幕光标 = 100 + (220-100)*0.75 = 190
    // 用 Insert 操作（对应 Enter 产生换行：有 reflow 但无 InsertReveal glyph），
    // 这样 active_text_transaction_key() 才会返回本事务。
    let old_key = VisualTransactionKey::new(1, 1);
    coord.prepared_queue.enqueue(rendering_tx(
        old_key,
        TextVisualOperationKind::Insert,
        vec![elapsed_unit(reflow_slice(0, 3, 100.0, 220.0), 50, 100, now)],
        caret(100.0),
        caret(220.0),
        now,
        50,
    ));

    // 前置断言：旧事务当前屏幕光标 = 190
    let mut old_sample = AnimationFrameSample::new(now);
    old_sample.set_progress(old_key, 0.5);
    let (cx_old, _, _) = coord
        .compute_coordinated_cursor_position(&old_sample, 0)
        .expect("旧事务应能算出协同光标");
    assert!(
        (cx_old - 190.0).abs() < 1e-6,
        "前置：旧事务屏幕光标应在 190（reflow progress 0.5 → eased 0.75），got {}",
        cx_old
    );

    // ── rebase 交棒：take_rebase_frames 现在同时采集文字单元和屏幕光标 ──
    let (rebase_frames, sampled_cursor) =
        coord.take_rebase_frames(&[old_key], "rebased_by_enter", now, None, "abc", 0);
    assert_eq!(
        rebase_frames.len(),
        1,
        "应采集到一个正在播放的 reflow unit 帧"
    );
    assert!(
        (rebase_frames[0].x - 190.0).abs() < 1e-6,
        "rebase 帧应携带旧 unit 当前屏幕位置 190，got {}",
        rebase_frames[0].x
    );
    let sampled_cursor = sampled_cursor.expect("rebase 交棒应采样到旧事务屏幕光标");
    assert!(
        (sampled_cursor.sampled.x - 190.0).abs() < 1e-6,
        "sampled_cursor_rect 应为旧事务屏幕光标 190，got {}",
        sampled_cursor.sampled.x
    );

    // ── 新事务：Enter/纯 reflow，没有 InsertReveal/DeleteConceal glyph 当边界 ──
    // old_cursor_rect = 100：pipeline.record_visual_transaction() 对旧正文做的
    //   权威布局 caret（逻辑 old caret），不等于旧事务屏幕光标 190。
    // new_cursor_rect = 20：下一行最终 caret。
    // 修复后 cursor_visual_from = sampled_cursor（190），cursor_visual_to = new_cursor_rect（20）。
    let new_key = VisualTransactionKey::new(2, 2);
    let mut new_units = wrap_units(vec![reflow_slice(0, 3, 100.0, 20.0)]);
    let offset_map = OffsetMap::build("abc", "abc");
    match_rebase_frames(&rebase_frames, &mut new_units, &offset_map);
    // rebase 后新 reflow unit：from_document_rect.x = 190（屏幕位置），progress(now) = 0
    assert!(
        (new_units[0].slice.from_document_rect.x - 190.0).abs() < 1e-6,
        "rebase 后新 reflow unit 的 from 应为屏幕位置 190，got {}",
        new_units[0].slice.from_document_rect.x
    );
    assert!(
        new_units[0].progress(now).abs() < 1e-9,
        "rebase 后新 unit progress 从 0 开始，got {}",
        new_units[0].progress(now)
    );

    let mut new_tx = rendering_tx(
        new_key,
        TextVisualOperationKind::Insert,
        new_units,
        caret(100.0), // ← 逻辑 old caret（pipeline 权威布局），≠屏幕光标 190
        caret(20.0),
        now,
        0,
    );
    // 修复后：rebase 交棒时把采样到的旧事务屏幕光标作为 cursor_visual_from，
    // new_cursor_rect 作为 cursor_visual_to，compute_coordinated_cursor_position
    // 消费这条同帧 caret track，不再用裸 old_cursor_rect 当 reflow 光标起点。
    // Issue #690 评论 5682867529: new_first 不再接受 now 参数，用结构体字面量设置 started_at。
    new_tx.cursor_visual_track = Some(PreparedCursorVisualTrack {
        from: sampled_cursor.sampled,
        to: caret(20.0),
        from_visual_line_id: None,
        to_visual_line_id: None,
        from_line_top: 0.0,
        from_line_bottom: 0.0,
        to_line_top: 0.0,
        to_line_bottom: 0.0,
        started_at: Some(now),
        duration_ms: 100,
        pause_start: None,
    });
    coord.prepared_queue.enqueue(new_tx);

    // ── 新事务第一帧（frame_now = now，reflow unit progress = 0）──
    let plan = coord.build_render_plan_full(
        stale_cursor_state(),
        SelectionPreeditPlan::default(),
        FrameContext::default(),
        CursorStyle::default(),
        SelectionPreeditStyle::default(),
        now,
        None,
        0,
        0.0,
    );

    // 文字 reflow：from=190，progress=0 → frame.x = 190（不跳）
    assert!(!plan.text_animation.glyphs.is_empty(), "新事务应产出文字帧");
    let text_x = plan.text_animation.glyphs[0].x;
    assert!(
        (text_x - 190.0).abs() < 1e-6,
        "文字 reflow 应从屏幕位置 190 起步不跳，got {}",
        text_x
    );

    // 光标 reflow：修复后从 cursor_visual_from.x = 190 起步（屏幕光标不跳）。
    let cx_new = plan.cursor.x;
    assert!(
        (cx_new - 190.0).abs() < 1e-6,
        "Issue #690 评论 5680276931: rebase 交棒后 reflow 光标应从上一帧屏幕光标 190 起步，\
             修复后 cursor_visual_from 同步 rebase，第一帧光标不跳（got cursor.x={}）",
        cx_new
    );
}

/// 评论 5681206040 问题 1：`sample_coordinated_cursor_rect_at()` 没有采样旧事务
/// 自己的 visual caret track（`tx.cursor_visual_from` / `tx.cursor_visual_to`），
/// 仍然固定用 `old_cursor_rect / new_cursor_rect`。连续交棒（第二次 rebase）时
/// 采样到的光标会回到逻辑 old caret 起算，与旧事务当前屏幕光标不一致，
/// 新事务拿错误的 sampled cursor 当起点，连续快速操作时光标跳变。
#[test]
fn issue690_comment5681206040_continuous_handoff_sample_uses_tx_visual_caret_track() {
    let now = Instant::now();
    let mut coord = LinuxEditorAnimationCoordinator::new();

    // ── 第一次交棒后的新事务 B（手工装配，模拟第一次 rebase 后的状态）──
    // cursor_visual_from = 190：第一次 rebase 采样的旧事务屏幕光标。
    // cursor_visual_to   = 20 ：new_cursor_rect 镜像。
    // old_cursor_rect    = 100：pipeline 对旧正文做的权威布局 caret（逻辑 old caret），
    //                          ≠ 旧事务屏幕光标 190。
    // new_cursor_rect    = 20。
    // reflow unit 已播 50/100ms → progress 0.5 → ease_out_quad(0.5) = 0.75。
    // 事务 B 当前屏幕光标 = 190 + (20 - 190) * 0.75 = 62.5。
    let key_b = VisualTransactionKey::new(2, 2);
    let mut tx_b = rendering_tx(
        key_b,
        TextVisualOperationKind::Insert,
        vec![elapsed_unit(reflow_slice(0, 3, 190.0, 20.0), 50, 100, now)],
        caret(100.0),
        caret(20.0),
        now,
        50,
    );
    // Issue #690 评论 5682867529: new_first 不再接受 now 参数，用结构体字面量设置 started_at。
    tx_b.cursor_visual_track = Some(PreparedCursorVisualTrack {
        from: caret(190.0),
        to: caret(20.0),
        from_visual_line_id: None,
        to_visual_line_id: None,
        from_line_top: 0.0,
        from_line_bottom: 0.0,
        to_line_top: 0.0,
        to_line_bottom: 0.0,
        started_at: Some(now - Duration::from_millis(50)),
        duration_ms: 100,
        pause_start: None,
    });
    coord.prepared_queue.enqueue(tx_b);

    // 前置断言：compute_coordinated_cursor_position 已修复用 visual track，
    // 事务 B 当前屏幕光标 = 62.5。
    let expected_screen_cursor = 190.0 + (20.0 - 190.0) * AnimatedSlice::ease_out_quad(0.5);
    let mut sample_b = AnimationFrameSample::new(now);
    sample_b.set_progress(key_b, 0.5);
    let (cx_b, _, _) = coord
        .compute_coordinated_cursor_position(&sample_b, 0)
        .expect("事务 B 应能算出协同光标");
    assert!(
        (cx_b - expected_screen_cursor).abs() < 1e-6,
        "前置：事务 B 屏幕光标应在 {}（visual track 190→20, progress 0.5），got {}",
        expected_screen_cursor,
        cx_b
    );

    // ── 第二次 rebase：take_rebase_frames 采样事务 B 的屏幕光标 ──
    // sample_coordinated_cursor_rect_at(B, now) 应返回事务 B 当前屏幕光标 62.5。
    // 当前缺陷：reflow 分支用 old_cursor_rect=100, new_cursor_rect=20
    //   → 100 + (20-100)*0.75 = 40，而非屏幕上的 62.5。
    let (_rebase_frames, sampled_cursor) =
        coord.take_rebase_frames(&[key_b], "rebased_by_second_input", now, None, "abc", 0);
    let sampled_cursor = sampled_cursor.expect("第二次 rebase 应采样到事务 B 的屏幕光标");

    let buggy_value = 100.0 + (20.0 - 100.0) * AnimatedSlice::ease_out_quad(0.5);
    assert!(
        (sampled_cursor.sampled.x - expected_screen_cursor).abs() < 1e-6,
        "Issue #690 评论 5681206040 问题1: 连续交棒第二次 sampled_cursor 应为事务 B \
             屏幕光标 {} (用 cursor_visual_track)，但当前实现用 \
             old_cursor_rect/new_cursor_rect 算出 {} (got sampled_cursor.sampled.x={})",
        expected_screen_cursor,
        buggy_value,
        sampled_cursor.sampled.x
    );
}

/// 评论 5681206040 问题 2：cursor reflow 仍然"随便拿第一个 reflow unit 的 progress"。
/// `sample_coordinated_cursor_rect_at()` 和 `compute_coordinated_cursor_position()` 里
/// `reflow_progress()` 遍历 `tx.units`，遇到第一个 `ReflowMove/ReflowCrossFade` 就直接
/// 返回它的 progress。视觉单元各自持有 `started_at / duration_ms`，rebase 后不同 unit
/// 可能有不同剩余时长。第一个 reflow unit 可能已经到 1.0，另一个与当前 caret 更相关的
/// reflow unit 还在 0.4，光标提前冲到目标，与实际正在移动的文字不同步。
#[test]
fn issue690_comment5681206040_reflow_progress_should_not_take_first_unit() {
    let now = Instant::now();
    let mut coord = LinuxEditorAnimationCoordinator::new();

    // 事务 C：Insert 纯 reflow（无 InsertReveal glyph），首次事务（无 visual track）。
    // old_cursor_rect = 0, new_cursor_rect = 200。
    // 两个 ReflowMove unit：
    //   unit1: duration=100ms, 已播 100ms → progress=1.0（已播完）
    //   unit2: duration=200ms, 已播 40ms → progress=0.2（仍在播）
    // reflow_progress() 遇到 unit1 直接返回 1.0 → eased=1.0 → 光标 = 200（目标）。
    // 但 unit2 还在 0.2，事务整体未完成，光标不应已到目标。
    let key_c = VisualTransactionKey::new(3, 3);
    let tx_c = rendering_tx(
        key_c,
        TextVisualOperationKind::Insert,
        vec![
            elapsed_unit(reflow_slice(0, 3, 0.0, 100.0), 100, 100, now),
            elapsed_unit(reflow_slice(3, 6, 50.0, 150.0), 40, 200, now),
        ],
        caret(0.0),
        caret(200.0),
        now,
        40,
    );
    coord.prepared_queue.enqueue(tx_c);

    let mut sample_c = AnimationFrameSample::new(now);
    sample_c.set_progress(key_c, 0.2);
    let (cx_c, _, _) = coord
        .compute_coordinated_cursor_position(&sample_c, 0)
        .expect("事务 C 应能算出协同光标");

    // 期望：unit2 还在 progress=0.2，事务未完成，光标不应已到 new_cursor_rect.x=200。
    // 当前缺陷：reflow_progress 取 unit1.progress=1.0 → 光标 = 200（提前冲到目标）。
    let new_cx = 200.0;
    assert!(
        (cx_c - new_cx).abs() > 1e-6,
        "Issue #690 评论 5681206040 问题2: unit2 还在 progress=0.2，事务未完成，\
             光标不应已到 new_cursor_rect.x={}，但 reflow_progress 取第一个 unit1.progress=1.0 \
             导致光标提前冲到目标 (got cursor.x={})",
        new_cx,
        cx_c
    );
}

/// 评论 5681206040 要求：测试补真实连续交棒，不要只测一次。
/// 旧事务 `100 -> 220` 播到中间 -> 第一次 rebase 成 `190 -> 20` -> 再播一段 ->
/// 第二次 rebase；断言第二次 sampled caret 精确等于第二次 rebase 前
/// `compute_coordinated_cursor_position()` 的屏幕结果，而不是按逻辑 old/new caret 重算。
#[test]
fn issue690_comment5681206040_real_continuous_handoff_two_rebases() {
    let now = Instant::now();
    let mut coord = LinuxEditorAnimationCoordinator::new();

    // ── 旧事务 A：caret 100→220，reflow unit 播到中间 ──
    // reflow unit: from_x=100, to_x=220, duration=100ms, 已播 50ms → progress 0.5
    // ease_out_quad(0.5) = 0.75 → 屏幕光标 = 100 + (220-100)*0.75 = 190
    let key_a = VisualTransactionKey::new(1, 1);
    let mut tx_a = rendering_tx(
        key_a,
        TextVisualOperationKind::Insert,
        vec![elapsed_unit(reflow_slice(0, 3, 100.0, 220.0), 50, 100, now)],
        caret(100.0),
        caret(220.0),
        now,
        50,
    );
    // Issue #690 评论 5682867529: new_first 不再接受 now 参数，用结构体字面量设置 started_at。
    tx_a.cursor_visual_track = Some(PreparedCursorVisualTrack {
        from: caret(100.0),
        to: caret(220.0),
        from_visual_line_id: None,
        to_visual_line_id: None,
        from_line_top: 0.0,
        from_line_bottom: 0.0,
        to_line_top: 0.0,
        to_line_bottom: 0.0,
        started_at: Some(now - Duration::from_millis(50)),
        duration_ms: 100,
        pause_start: None,
    });
    coord.prepared_queue.enqueue(tx_a);

    // 验证事务 A 当前屏幕光标 = 190
    let expected_a = 100.0 + (220.0 - 100.0) * AnimatedSlice::ease_out_quad(0.5);
    let mut sample_a = AnimationFrameSample::new(now);
    sample_a.set_progress(key_a, 0.5);
    let (cx_a, _, _) = coord
        .compute_coordinated_cursor_position(&sample_a, 0)
        .expect("事务 A 应能算出协同光标");
    assert!(
        (cx_a - expected_a).abs() < 1e-6,
        "事务 A 屏幕光标应为 {}，got {}",
        expected_a,
        cx_a
    );

    // ── 第一次 rebase：take_rebase_frames 采集事务 A 的屏幕光标 ──
    let (rebase_frames_a, handoff_a) =
        coord.take_rebase_frames(&[key_a], "first_rebase", now, None, "abc", 0);
    let handoff_a = handoff_a.expect("第一次 rebase 应采样到事务 A 的屏幕光标");
    assert!(
        (handoff_a.sampled.x - expected_a).abs() < 1e-6,
        "第一次 rebase sampled caret 应为 {}，got {}",
        expected_a,
        handoff_a.sampled.x
    );

    // ── 新事务 B：用 handoff_a 构造 cursor_visual_track ──
    // from = 190（sampled），to = 20（new_cursor_rect），duration = handoff_a.remaining_duration_ms
    let key_b = VisualTransactionKey::new(2, 2);
    let mut new_units_b = wrap_units(vec![reflow_slice(0, 3, 190.0, 20.0)]);
    let offset_map = OffsetMap::build("abc", "abc");
    match_rebase_frames(&rebase_frames_a, &mut new_units_b, &offset_map);
    let mut tx_b = rendering_tx(
        key_b,
        TextVisualOperationKind::Insert,
        new_units_b,
        caret(100.0),
        caret(20.0),
        now,
        0,
    );
    tx_b.cursor_visual_track = Some(PreparedCursorVisualTrack {
        from: handoff_a.sampled,
        to: caret(20.0),
        from_visual_line_id: None,
        to_visual_line_id: None,
        from_line_top: 0.0,
        from_line_bottom: 0.0,
        to_line_top: 0.0,
        to_line_bottom: 0.0,
        started_at: Some(now),
        duration_ms: handoff_a.remaining_duration_ms,
        pause_start: None,
    });
    coord.prepared_queue.enqueue(tx_b);

    // ── 事务 B 播一段：50ms 后 ──
    // caret track: from=190, to=20, started_at=now, duration=50ms（handoff remaining）
    // progress = 50/50 = 1.0 → eased = 1.0 → caret = 20
    // 但我们要测"再播一段"不是"播完"，所以用 25ms → progress = 25/50 = 0.5
    // ease_out_quad(0.5) = 0.75 → 屏幕光标 = 190 + (20-190)*0.75 = 62.5
    let now_after_b = now + Duration::from_millis(25);
    let expected_b = 190.0 + (20.0 - 190.0) * AnimatedSlice::ease_out_quad(0.5);
    let mut sample_b = AnimationFrameSample::new(now_after_b);
    sample_b.set_progress(key_b, 0.5);
    let (cx_b, _, _) = coord
        .compute_coordinated_cursor_position(&sample_b, 0)
        .expect("事务 B 应能算出协同光标");
    assert!(
        (cx_b - expected_b).abs() < 1e-6,
        "事务 B 屏幕光标应为 {}（visual track 190→20, progress 0.5），got {}",
        expected_b,
        cx_b
    );

    // ── 第二次 rebase：take_rebase_frames 采样事务 B 的屏幕光标 ──
    let (_rebase_frames_b, handoff_b) =
        coord.take_rebase_frames(&[key_b], "second_rebase", now_after_b, None, "abc", 0);
    let handoff_b = handoff_b.expect("第二次 rebase 应采样到事务 B 的屏幕光标");

    // 断言：第二次 sampled caret 精确等于第二次 rebase 前
    // compute_coordinated_cursor_position() 的屏幕结果
    assert!(
        (handoff_b.sampled.x - expected_b).abs() < 1e-6,
        "Issue #690 评论 5681206040: 连续交棒第二次 sampled caret 应为事务 B 屏幕光标 {}，\
             但 got {}（如果按逻辑 old/new caret 重算会得到不同值）",
        expected_b,
        handoff_b.sampled.x
    );

    // 额外验证：第二次 sampled caret 不等于按逻辑 old/new caret 重算的值
    let logical_recalc = 100.0 + (20.0 - 100.0) * AnimatedSlice::ease_out_quad(0.5);
    assert!(
        (handoff_b.sampled.x - logical_recalc).abs() > 1e-6,
        "第二次 sampled caret 不应等于按逻辑 old/new caret 重算的值 {}",
        logical_recalc
    );
}

/// 评论 5681206040 要求：再补两个不同 `started_at/duration_ms` 的 reflow unit，
/// 确认 caret 不依赖 `units` 顺序。
#[test]
fn issue690_comment5681206040_caret_track_independent_of_units_order() {
    let now = Instant::now();

    // 事务 D：有两个不同 started_at/duration_ms 的 reflow unit，有 cursor_visual_track。
    // caret track: from=0, to=200, started_at=now-40ms, duration=200ms
    // 已播 40ms → progress = 40/200 = 0.2 → ease_out_quad(0.2) = 0.36
    // 屏幕光标 = 0 + (200-0)*0.36 = 72
    let expected_d = 0.0 + (200.0 - 0.0) * AnimatedSlice::ease_out_quad(0.2);

    // unit1: duration=100ms, 已播 100ms → progress=1.0（已播完）
    // unit2: duration=200ms, 已播 40ms → progress=0.2（仍在播）
    // 如果 caret 依赖 units 顺序（取第一个 reflow unit 的 progress），
    // 会用 unit1.progress=1.0 → eased=1.0 → caret=200（错误）。
    // 正确行为：caret track 自带 started_at/duration_ms，不依赖任何 unit 的 progress。

    // ── 顺序 1：unit1 在前，unit2 在后 ──
    let mut coord1 = LinuxEditorAnimationCoordinator::new();
    let key_d1 = VisualTransactionKey::new(4, 4);
    let mut tx_d1 = rendering_tx(
        key_d1,
        TextVisualOperationKind::Insert,
        vec![
            elapsed_unit(reflow_slice(0, 3, 0.0, 100.0), 100, 100, now),
            elapsed_unit(reflow_slice(3, 6, 50.0, 150.0), 40, 200, now),
        ],
        caret(0.0),
        caret(200.0),
        now,
        40,
    );
    // Issue #690 评论 5682867529: new_first 不再接受 now 参数，用结构体字面量设置 started_at。
    tx_d1.cursor_visual_track = Some(PreparedCursorVisualTrack {
        from: caret(0.0),
        to: caret(200.0),
        from_visual_line_id: None,
        to_visual_line_id: None,
        from_line_top: 0.0,
        from_line_bottom: 0.0,
        to_line_top: 0.0,
        to_line_bottom: 0.0,
        started_at: Some(now - Duration::from_millis(40)),
        duration_ms: 200,
        pause_start: None,
    });
    coord1.prepared_queue.enqueue(tx_d1);

    let mut sample_d1 = AnimationFrameSample::new(now);
    sample_d1.set_progress(key_d1, 0.2);
    let (cx_d1, _, _) = coord1
        .compute_coordinated_cursor_position(&sample_d1, 0)
        .expect("事务 D1 应能算出协同光标");

    assert!(
        (cx_d1 - expected_d).abs() < 1e-6,
        "事务 D1（unit1在前）屏幕光标应为 {}（caret track 0→200, progress 0.2），\
             got {} — caret 不应依赖 units 顺序",
        expected_d,
        cx_d1
    );

    // ── 顺序 2：unit2 在前，unit1 在后（交换 units 顺序）──
    let mut coord2 = LinuxEditorAnimationCoordinator::new();
    let key_d2 = VisualTransactionKey::new(5, 5);
    let mut tx_d2 = rendering_tx(
        key_d2,
        TextVisualOperationKind::Insert,
        vec![
            elapsed_unit(reflow_slice(3, 6, 50.0, 150.0), 40, 200, now),
            elapsed_unit(reflow_slice(0, 3, 0.0, 100.0), 100, 100, now),
        ],
        caret(0.0),
        caret(200.0),
        now,
        40,
    );
    // Issue #690 评论 5682867529: new_first 不再接受 now 参数，用结构体字面量设置 started_at。
    tx_d2.cursor_visual_track = Some(PreparedCursorVisualTrack {
        from: caret(0.0),
        to: caret(200.0),
        from_visual_line_id: None,
        to_visual_line_id: None,
        from_line_top: 0.0,
        from_line_bottom: 0.0,
        to_line_top: 0.0,
        to_line_bottom: 0.0,
        started_at: Some(now - Duration::from_millis(40)),
        duration_ms: 200,
        pause_start: None,
    });
    coord2.prepared_queue.enqueue(tx_d2);

    let mut sample_d2 = AnimationFrameSample::new(now);
    sample_d2.set_progress(key_d2, 0.2);
    let (cx_d2, _, _) = coord2
        .compute_coordinated_cursor_position(&sample_d2, 0)
        .expect("事务 D2 应能算出协同光标");

    assert!(
        (cx_d2 - expected_d).abs() < 1e-6,
        "事务 D2（unit2在前）屏幕光标应为 {}（caret track 0→200, progress 0.2），\
             got {} — caret 不应依赖 units 顺序",
        expected_d,
        cx_d2
    );

    // ── 关键断言：两种 units 顺序的 caret 结果完全相同 ──
    assert!(
        (cx_d1 - cx_d2).abs() < 1e-6,
        "Issue #690 评论 5681206040: 不同 units 顺序的 caret 结果应完全相同，\
             但 got cx_d1={} vs cx_d2={} — caret track 不应依赖 units 顺序",
        cx_d1,
        cx_d2
    );

    // ── 额外验证：sample_coordinated_cursor_rect_at 也不依赖 units 顺序 ──
    let sampled1 = sample_coordinated_cursor_rect_at(
        coord1
            .prepared_queue
            .active_transactions()
            .iter()
            .find(|t| t.key == key_d1)
            .unwrap(),
        now,
    )
    .expect("事务 D1 应能采样到光标");
    let sampled2 = sample_coordinated_cursor_rect_at(
        coord2
            .prepared_queue
            .active_transactions()
            .iter()
            .find(|t| t.key == key_d2)
            .unwrap(),
        now,
    )
    .expect("事务 D2 应能采样到光标");

    assert!(
        (sampled1.x - sampled2.x).abs() < 1e-6,
        "Issue #690 评论 5681206040: sample_coordinated_cursor_rect_at 也不应依赖 units 顺序，\
             但 got sampled1.x={} vs sampled2.x={}",
        sampled1.x,
        sampled2.x
    );
    assert!(
        (sampled1.x - expected_d).abs() < 1e-6,
        "sample_coordinated_cursor_rect_at 结果应为 {}，got {}",
        expected_d,
        sampled1.x
    );
}

/// Issue #690 评论 5682867529: caret track 跟文字 unit 共用同一个"开始播放时刻"。
///
/// 真正经过 Pending → Prepared → Rendering 生命周期的行为测试：
/// - 创建纯 reflow 事务，caret track 此时尚未开始（started_at = None）；
/// - 模拟在 Pending/Prepared 阶段过去 40ms；
/// - 第一帧进入 Rendering；
/// - 断言同一个 frame_now 下文字 reflow unit progress == 0，caret track progress == 0；
/// - 再推进 50ms，断言二者从同一个起点同时前进。
#[test]
fn issue690_comment5682867529_caret_track_starts_with_text_unit_at_rendering() {
    let create_now = Instant::now();
    let mut coord = LinuxEditorAnimationCoordinator::new();
    let key = VisualTransactionKey::new(100, 100);

    // 构造一笔纯 reflow 事务（无 InsertReveal/DeleteConceal，只有 ReflowMove）。
    // caret track: from=caret(0), to=caret(200), duration=200ms。
    // 事务创建时 started_at = None（尚未开始）。
    let reflow_unit = PreparedVisualUnit::wrap(reflow_slice(0, 3, 0.0, 100.0), 200);
    let cursor_visual_track = PreparedCursorVisualTrack::new_first(
        caret(0.0),
        caret(200.0),
        None,
        None,
        0.0,
        0.0,
        0.0,
        0.0,
        200,
    );
    let tx = PreparedTextVisualTransaction {
        key,
        state: TextVisualTransactionState::Pending,
        operation_kind: TextVisualOperationKind::Insert,
        timeline: TransactionTimeline::new(200),
        units: vec![reflow_unit],
        old_cursor_rect: Some(caret(0.0)),
        new_cursor_rect: Some(caret(200.0)),
        cursor_visual_track: Some(cursor_visual_track),
        cancel_reason: None,
        texture_prepared: false,
        old_snapshot: None,
        new_snapshot: None,
        cursor_owner_epoch: 0,
        caret_motion_retired: false,
        coordinated: false,
        visual_affected_byte_range_old: None,
        visual_affected_byte_range_new: None,
        layout_basis_revision: LayoutRevision::initial(),
    };
    coord.prepared_queue.enqueue(tx);

    // 断言 1: 事务创建时 caret track started_at = None，progress = 0。
    {
        let tx_ref = coord
            .prepared_queue
            .active_transactions()
            .iter()
            .find(|t| t.key == key)
            .expect("事务应在队列中");
        let track = tx_ref
            .cursor_visual_track
            .as_ref()
            .expect("应有 caret track");
        assert!(
            track.started_at.is_none(),
            "事务创建时 caret track started_at 应为 None，got {:?}",
            track.started_at
        );
        assert!(
            (track.progress(create_now) - 0.0).abs() < 1e-9,
            "started_at=None 时 progress 应为 0"
        );
        assert!(
                tx_ref.units[0].timing.is_caret_driven()
                    || matches!(
                        &tx_ref.units[0].timing,
                        VisualUnitTiming::Timed {
                            started_at: None,
                            ..
                        }
                    ),
                "事务创建时文字 unit started_at 应为 None（CaretDriven 无 started_at，Timed 应为 None）"
            );
        assert!(
            (tx_ref.units[0].progress(create_now) - 0.0).abs() < 1e-9,
            "文字 unit progress 应为 0"
        );
    }

    // 模拟 Pending → Prepared 阶段过去 40ms（纹理准备等）。
    let prepared_now = create_now + Duration::from_millis(40);
    coord.prepared_queue.mark_prepared(key);

    // 断言 2: Prepared 阶段过去 40ms 后，caret track 仍未开始。
    {
        let tx_ref = coord
            .prepared_queue
            .active_transactions()
            .iter()
            .find(|t| t.key == key)
            .expect("事务应在队列中");
        let track = tx_ref
            .cursor_visual_track
            .as_ref()
            .expect("应有 caret track");
        assert!(
            track.started_at.is_none(),
            "Prepared 阶段过去 40ms 后 caret track started_at 仍应为 None \
                （Pending/Prepared 等待时间不算进动画播放时间），got {:?}",
            track.started_at
        );
        assert!(
            (track.progress(prepared_now) - 0.0).abs() < 1e-9,
            "Prepared 阶段 progress 仍应为 0（未开始计时），got {}",
            track.progress(prepared_now)
        );
        assert!(
            (tx_ref.units[0].progress(prepared_now) - 0.0).abs() < 1e-9,
            "Prepared 阶段文字 unit progress 仍应为 0"
        );
    }

    // 第一帧进入 Rendering。
    let frame_now_0 = prepared_now + Duration::from_millis(16);
    // Issue #727 评论 5760020833 问题2: Prepared→Rendering 现由 begin_rendering_transactions
    // 在采样前完成，build_text_animation_plan_with_sample 不再做状态切换。
    coord.begin_rendering_transactions(frame_now_0);
    let mut sample_0 = AnimationFrameSample::new(frame_now_0);
    sample_0.set_progress(key, 0.0);
    let (plan_0, _, _) =
        coord.build_text_animation_plan_with_sample(&sample_0, 0, LayoutRevision::initial());

    // 断言 3: 同一个 frame_now_0 下 progress 都 == 0。
    {
        let tx_ref = coord
            .prepared_queue
            .active_transactions()
            .iter()
            .find(|t| t.key == key)
            .expect("事务应在队列中");
        assert_eq!(
            tx_ref.state,
            TextVisualTransactionState::Rendering,
            "第一帧后事务应进入 Rendering"
        );
        let track = tx_ref
            .cursor_visual_track
            .as_ref()
            .expect("应有 caret track");
        assert!(
            track.started_at.is_some(),
            "进入 Rendering 后 caret track started_at 应被设置"
        );
        assert_eq!(
            track.started_at,
            Some(frame_now_0),
            "caret track started_at 应等于第一帧 frame_now"
        );
        let track_progress = track.progress(frame_now_0);
        assert!(
            (track_progress - 0.0).abs() < 1e-9,
            "第一帧 caret track progress 应为 0（刚启动），got {}",
            track_progress
        );
        let unit_progress = tx_ref.units[0].progress(frame_now_0);
        assert!(
            (unit_progress - 0.0).abs() < 1e-9,
            "第一帧文字 unit progress 应为 0（刚启动），got {}",
            unit_progress
        );
        assert!(!plan_0.glyphs.is_empty(), "应有文字 glyph 输出");
    }

    // 推进 50ms，断言二者从同一个起点同时前进。
    let frame_now_1 = frame_now_0 + Duration::from_millis(50);
    let mut sample_1 = AnimationFrameSample::new(frame_now_1);
    sample_1.set_progress(key, 0.25);
    let (plan_1, _, _) =
        coord.build_text_animation_plan_with_sample(&sample_1, 0, LayoutRevision::initial());

    {
        let tx_ref = coord
            .prepared_queue
            .active_transactions()
            .iter()
            .find(|t| t.key == key)
            .expect("事务应在队列中");
        let track = tx_ref
            .cursor_visual_track
            .as_ref()
            .expect("应有 caret track");
        let track_progress = track.progress(frame_now_1);
        let unit_progress = tx_ref.units[0].progress(frame_now_1);
        assert!(
            (track_progress - 0.25).abs() < 1e-9,
            "推进 50ms 后 caret track progress 应为 0.25（50/200），got {}",
            track_progress
        );
        assert!(
            (unit_progress - 0.25).abs() < 1e-9,
            "推进 50ms 后文字 unit progress 应为 0.25（50/200），got {}",
            unit_progress
        );
        assert!(
            (track_progress - unit_progress).abs() < 1e-9,
            "caret track 和文字 unit 的 progress 应完全相同（从同一帧起跑），\
                 got track={} unit={}",
            track_progress,
            unit_progress
        );
        assert!(!plan_1.glyphs.is_empty(), "推进 50ms 后应有文字 glyph 输出");
    }

    println!("[BUGFIX_690_VERIFY] 评论5682867529 caret track 与文字 unit 同帧起跑 (FIXED)");
}

/// Issue #690 评论 5683759796: rebased 文字 unit 和 caret track 在 Rendering 阶段同帧起跑。
///
/// 真正经过 rebase 交棒 + Pending → Prepared → Rendering 生命周期的行为测试：
/// - 构造旧事务（Rendering），含一个播到 50% 的 InsertReveal unit 和已播 50ms 的 caret track；
/// - 用 collect_rebase_frames 采集旧事务的 rebase frames；
/// - 构造新事务的 units（fresh wrap），用 match_rebase_frames rebase；
/// - 构造新事务的 caret track（rebase_to，started_at = None）；
/// - 新事务以 Pending 入队，模拟 Pending → Prepared 过去 40ms；
/// - 断言 Prepared 阶段：rebased text unit started_at == None 且 progress == 0；
///   caret track started_at == None 且 progress == 0；
/// - 第一帧进入 Rendering，断言 rebased text unit progress == 0 且 caret track progress == 0
///   （修复前 rebased text unit progress 会 > 0，因为 started_at = Some(旧事务交棒时刻)）；
/// - 推进 25ms / 50ms，断言二者一起前进（progress 相同）。
#[test]
fn issue690_comment5683759796_rebased_unit_and_caret_track_start_together_at_rendering() {
    let now = Instant::now();
    let mut coord = LinuxEditorAnimationCoordinator::new();

    // ── 1. 构造旧事务（Rendering 状态） ──
    // 文字 unit: InsertReveal, elapsed 50ms / duration 100ms → progress 0.5
    // → ease_out_quad(0.5) = 0.75 → visible_fraction = 0.75
    let old_unit = elapsed_unit(reveal_slice(0, 3, 100.0, 60.0), 50, 100, now);
    // caret track: from=caret(100), to=caret(160), 已播 50ms / duration 100ms → progress 0.5
    let old_caret_track = PreparedCursorVisualTrack {
        from: caret(100.0),
        to: caret(160.0),
        from_visual_line_id: None,
        to_visual_line_id: None,
        from_line_top: 0.0,
        from_line_bottom: 0.0,
        to_line_top: 0.0,
        to_line_bottom: 0.0,
        started_at: Some(now - Duration::from_millis(50)),
        duration_ms: 100,
        pause_start: None,
    };
    let old_key = VisualTransactionKey::new(7, 7);
    let mut old_tx = rendering_tx(
        old_key,
        TextVisualOperationKind::Insert,
        vec![old_unit],
        caret(100.0),
        caret(160.0),
        now,
        50,
    );
    old_tx.cursor_visual_track = Some(old_caret_track.clone());

    // ── 2. 用 collect_rebase_frames(now) 采集旧事务的 rebase frames ──
    let rebase_frames = old_tx.collect_rebase_frames(now);
    assert_eq!(
        rebase_frames.len(),
        1,
        "应采集到一个 rebase frame（旧 unit 未播完）"
    );
    let rebase_frame = &rebase_frames[0];
    assert_eq!(rebase_frame.sampled_at, now);
    assert_eq!(rebase_frame.remaining_duration_ms, 50);

    // ── 3. 构造新事务的 units（fresh wrap），用 match_rebase_frames rebase ──
    let mut new_units = wrap_units(vec![reveal_slice(0, 3, 100.0, 60.0)]);
    let offset_map = OffsetMap::build("abc", "abc");
    match_rebase_frames(&rebase_frames, &mut new_units, &offset_map);

    // rebased text unit: start_fraction = 0.75（旧 unit 当前可见比例）。
    // Issue #690 评论 5683759796 关键断言: started_at 必须是 None（不是 Some(sampled_at)），
    // 这样才不会从旧事务交棒时刻提前计时。
    // Issue #727 约束 2: CaretDriven unit 无独立 duration_ms，剩余时长由 caret track 管理。
    // CaretDriven unit 的 start_fraction 是 rebase 交棒时的载体（0.75）。
    let (new_started_at_is_none, new_duration_ms) = match &new_units[0].timing {
        VisualUnitTiming::CaretDriven { start_fraction, .. } => {
            // CaretDriven unit 无 duration_ms 字段，剩余时长在 caret track 中断言。
            assert!(
                (start_fraction - 0.75).abs() < 1e-9,
                "rebase 后 CaretDriven unit start_fraction 应为 0.75，got {}",
                start_fraction
            );
            (true, 0u64)
        }
        VisualUnitTiming::Timed {
            started_at,
            duration_ms,
            ..
        } => (started_at.is_none(), *duration_ms),
    };
    assert!(
        new_started_at_is_none,
        "Issue #690 评论 5683759796: rebase 后文字 unit started_at 应为 None\
             （等 Rendering 再启动）"
    );
    // CaretDriven unit 无独立 duration_ms，剩余时长在 caret track 中断言（见下方）。
    // Timed unit 的 duration_ms 应为剩余时长 50。
    if !matches!(&new_units[0].timing, VisualUnitTiming::CaretDriven { .. }) {
        assert_eq!(
            new_duration_ms, 50,
            "rebase 后 Timed unit duration_ms 应为剩余时长 50"
        );
    }

    // ── 4. 构造新事务的 caret track（rebase_to，started_at = None） ──
    let new_caret_track = old_caret_track.rebase_to(caret(220.0), now);
    assert!(
        new_caret_track.started_at.is_none(),
        "rebase_to 后 caret track started_at 应为 None"
    );
    assert_eq!(
        new_caret_track.duration_ms, 50,
        "rebase_to 后 caret track duration_ms 应为剩余时长 50"
    );

    // ── 5. 把新事务以 Pending 状态入队 ──
    let new_key = VisualTransactionKey::new(8, 8);
    let new_tx = PreparedTextVisualTransaction {
        key: new_key,
        state: TextVisualTransactionState::Pending,
        operation_kind: TextVisualOperationKind::Insert,
        timeline: TransactionTimeline::new(50),
        units: new_units,
        old_cursor_rect: Some(caret(100.0)),
        new_cursor_rect: Some(caret(220.0)),
        cursor_visual_track: Some(new_caret_track),
        cancel_reason: None,
        texture_prepared: false,
        old_snapshot: None,
        new_snapshot: None,
        cursor_owner_epoch: 0,
        caret_motion_retired: false,
        coordinated: false,
        visual_affected_byte_range_old: None,
        visual_affected_byte_range_new: None,
        layout_basis_revision: LayoutRevision::initial(),
    };
    coord.prepared_queue.enqueue(new_tx);

    // ── 6. 模拟 Pending → Prepared 过去 40ms ──
    let prepared_now = now + Duration::from_millis(40);
    coord.prepared_queue.mark_prepared(new_key);

    // ── 7. 断言 Prepared 阶段 ──
    {
        let tx_ref = coord
            .prepared_queue
            .active_transactions()
            .iter()
            .find(|t| t.key == new_key)
            .expect("新事务应在队列中");
        assert_eq!(
            tx_ref.state,
            TextVisualTransactionState::Prepared,
            "mark_prepared 后事务应处于 Prepared"
        );
        assert!(
                tx_ref.units[0].timing.is_caret_driven()
                    || matches!(
                        &tx_ref.units[0].timing,
                        VisualUnitTiming::Timed {
                            started_at: None,
                            ..
                        }
                    ),
                "Prepared 阶段 rebased text unit started_at 应为 None（CaretDriven 无 started_at，Timed 应为 None）"
            );
        assert!(
            (tx_ref.units[0].progress(prepared_now) - 0.0).abs() < 1e-9,
            "Prepared 阶段 rebased text unit progress 应为 0，got {}",
            tx_ref.units[0].progress(prepared_now)
        );
        let track = tx_ref
            .cursor_visual_track
            .as_ref()
            .expect("应有 caret track");
        assert!(
            track.started_at.is_none(),
            "Prepared 阶段 caret track started_at 应为 None，got {:?}",
            track.started_at
        );
        assert!(
            (track.progress(prepared_now) - 0.0).abs() < 1e-9,
            "Prepared 阶段 caret track progress 应为 0，got {}",
            track.progress(prepared_now)
        );
    }

    // ── 8. 第一帧进入 Rendering ──
    let frame_now_0 = prepared_now + Duration::from_millis(16);
    // Issue #727 评论 5760020833 问题2: Prepared→Rendering 现由 begin_rendering_transactions
    // 在采样前完成，build_text_animation_plan_with_sample 不再做状态切换。
    coord.begin_rendering_transactions(frame_now_0);
    let mut sample_0 = AnimationFrameSample::new(frame_now_0);
    sample_0.set_progress(new_key, 0.0);
    // Issue #727 约束 3: InsertReveal/DeleteConceal 需要 CoordinatedMotionFrame.caret
    // 不为 None 才能生成 glyph。第一帧 caret track progress = 0，caret 在 from = (100, 0)。
    let coordinated_frame_0 = CoordinatedMotionFrame {
        caret: Some(SampledCaretFrame {
            x: 100.0,
            y: 0.0,
            visual_line_id: None,
            progress: 0.0,
        }),
        owner_key: Some(new_key),
    };
    let (plan_0, _, _) =
        coord.build_text_animation_plan_with_sample(&sample_0, 0, LayoutRevision::initial());

    // ── 9. 断言第一帧 Rendering：二者 progress == 0（同帧起跑，没有错拍） ──
    {
        let tx_ref = coord
            .prepared_queue
            .active_transactions()
            .iter()
            .find(|t| t.key == new_key)
            .expect("新事务应在队列中");
        assert_eq!(
            tx_ref.state,
            TextVisualTransactionState::Rendering,
            "第一帧后事务应进入 Rendering"
        );
        let track = tx_ref
            .cursor_visual_track
            .as_ref()
            .expect("应有 caret track");
        assert_eq!(
            track.started_at,
            Some(frame_now_0),
            "进入 Rendering 后 caret track started_at 应等于第一帧 frame_now"
        );
        // Issue #727 约束 2: CaretDriven unit 无 started_at，progress 总是 0.0。
        // Timed unit 的 started_at 应等于第一帧 frame_now。
        match &tx_ref.units[0].timing {
            VisualUnitTiming::CaretDriven { .. } => {
                // CaretDriven: 无独立时间线，progress 由 caret track 驱动。
            }
            VisualUnitTiming::Timed { started_at, .. } => {
                assert_eq!(
                    *started_at,
                    Some(frame_now_0),
                    "Issue #690 评论 5683759796: 进入 Rendering 后 Timed text unit started_at \
                         应等于第一帧 frame_now"
                );
            }
        }
        let track_progress = track.progress(frame_now_0);
        let unit_progress = tx_ref.units[0].progress(frame_now_0);
        assert!(
            (track_progress - 0.0).abs() < 1e-9,
            "第一帧 caret track progress 应为 0（刚启动），got {}",
            track_progress
        );
        assert!(
            (unit_progress - 0.0).abs() < 1e-9,
            "Issue #690 评论 5683759796: 第一帧 rebased text unit progress 应为 0\
                 （刚启动，不再从旧事务交棒时刻提前计时），got {}",
            unit_progress
        );
        assert!(
            (track_progress - unit_progress).abs() < 1e-9,
            "第一帧 caret track 和 rebased text unit 的 progress 应完全相同（同帧起跑），\
                 got track={} unit={}",
            track_progress,
            unit_progress
        );
        assert!(!plan_0.glyphs.is_empty(), "第一帧应有文字 glyph 输出");
    }

    // ── 10. 推进 25ms，断言 caret track 前进到 0.5 ──
    // rebased text unit 是 CaretDriven（InsertReveal），progress 总是 0.0。
    // CaretDriven unit 的 current_visible_fraction 返回 start_fraction（rebase 交棒载体），
    // 不随时间变化。真正的可见比例推导在 build_text_animation_plan_with_sample 中
    // 从 caret track progress 计算。
    let frame_now_mid = frame_now_0 + Duration::from_millis(25);
    let mut sample_mid = AnimationFrameSample::new(frame_now_mid);
    sample_mid.set_progress(new_key, 0.5);
    // 推进 25ms 后 caret track progress = 0.5，eased = 0.75，
    // caret 在 100 + (220-100)*0.75 = 190。
    let coordinated_frame_mid = CoordinatedMotionFrame {
        caret: Some(SampledCaretFrame {
            x: 190.0,
            y: 0.0,
            visual_line_id: None,
            progress: 0.5,
        }),
        owner_key: Some(new_key),
    };
    let (_plan_mid, _, _) =
        coord.build_text_animation_plan_with_sample(&sample_mid, 0, LayoutRevision::initial());
    {
        let tx_ref = coord
            .prepared_queue
            .active_transactions()
            .iter()
            .find(|t| t.key == new_key)
            .expect("新事务应在队列中");
        let track = tx_ref
            .cursor_visual_track
            .as_ref()
            .expect("应有 caret track");
        let track_progress = track.progress(frame_now_mid);
        let unit_progress = tx_ref.units[0].progress(frame_now_mid);
        // CaretDriven unit 的 progress 总是 0.0（无独立时间线）。
        assert!(
            (unit_progress - 0.0).abs() < 1e-9,
            "Issue #727 约束 2: CaretDriven unit progress 总是 0.0，got {}",
            unit_progress
        );
        assert!(
            (track_progress - 0.5).abs() < 1e-9,
            "推进 25ms 后 caret track progress 应为 0.5（25/50），got {}",
            track_progress
        );
        // CaretDriven unit 的 current_visible_fraction 返回 start_fraction（rebase 交棒载体），
        // 不随时间变化。这是正确的——真正的可见比例推导在 build_text_animation_plan_with_sample 中
        // 从 caret track progress 计算。
        let unit_visible = tx_ref.units[0].current_visible_fraction(frame_now_mid);
        assert!(
                (unit_visible - 0.75).abs() < 1e-9,
                "Issue #727 约束 2: CaretDriven unit current_visible_fraction 返回 start_fraction（0.75），got {}",
                unit_visible
            );
    }

    // ── 11. 推进到 50ms，断言 caret track 到 1.0 ──
    let frame_now_1 = frame_now_0 + Duration::from_millis(50);
    let mut sample_1 = AnimationFrameSample::new(frame_now_1);
    sample_1.set_progress(new_key, 1.0);
    // 推进 50ms 后 caret track progress = 1.0，caret 在 to = (220, 0)。
    let coordinated_frame_1 = CoordinatedMotionFrame {
        caret: Some(SampledCaretFrame {
            x: 220.0,
            y: 0.0,
            visual_line_id: None,
            progress: 1.0,
        }),
        owner_key: Some(new_key),
    };
    let (_plan_1, _, _) =
        coord.build_text_animation_plan_with_sample(&sample_1, 0, LayoutRevision::initial());
    {
        let tx_ref = coord
            .prepared_queue
            .active_transactions()
            .iter()
            .find(|t| t.key == new_key)
            .expect("新事务应在队列中");
        let track = tx_ref
            .cursor_visual_track
            .as_ref()
            .expect("应有 caret track");
        let track_progress = track.progress(frame_now_1);
        let unit_progress = tx_ref.units[0].progress(frame_now_1);
        // CaretDriven unit 的 progress 总是 0.0（无独立时间线）。
        assert!(
            (unit_progress - 0.0).abs() < 1e-9,
            "Issue #727 约束 2: CaretDriven unit progress 总是 0.0，got {}",
            unit_progress
        );
        assert!(
            (track_progress - 1.0).abs() < 1e-9,
            "推进 50ms 后 caret track progress 应为 1.0（50/50），got {}",
            track_progress
        );
        // CaretDriven unit 的 current_visible_fraction 返回 start_fraction（rebase 交棒载体）。
        let unit_visible = tx_ref.units[0].current_visible_fraction(frame_now_1);
        assert!(
                (unit_visible - 0.75).abs() < 1e-9,
                "Issue #727 约束 2: CaretDriven unit current_visible_fraction 返回 start_fraction（0.75），got {}",
                unit_visible
            );
    }

    println!("[BUGFIX_690_VERIFY] 评论5683759796 rebased 文字 unit 和 caret track 在 Rendering 同帧起跑 (FIXED)");
}

/// Issue #727 评论 5760650874 方案 A 回归测试：旧 CaretDriven 事务失去 owner 后
/// **永远**不能再重新获得 owner（即使 new tx 完成移除、即使 old tx 仍在 active queue
/// 因 Timed Reflow 未播完）。
///
/// 修复前（评论 5760431554 问题1）只在"本帧"把非 owner 的 CaretDriven 事务视为
/// caret 部分完成：`caret_track_complete = !has_caret_driven_units || !owns_caret || caret_track_done`。
/// 但旧事务若同时还有 ReflowMove/ReflowCrossFade 没播完，`all_units_done == false`，
/// 旧事务仍留在 active queue。新事务完成并从队列移除后，下一帧
/// `active_text_transaction_key_with_epoch()` 会再次倒序选中这个旧事务，
/// `sample_coordinated_motion_frame()` 又会给它 `owner_key = old_tx.key`，
/// 已经 Snap 回 canonical 的旧 caret / 吞吐字轨迹会重新接管，造成 caret 回跳。
///
/// 方案 A 修复：给 `PreparedTextVisualTransaction` 增加 `caret_motion_retired: bool`，
/// 在 `build_text_animation_plan_with_sample` 发现 `has_caret_driven_units && !owns_caret`
/// 时置 true，`active_text_transaction_key_with_epoch` / `active_text_transaction_key`
/// 永远跳过 retired 事务。
///
/// 本测试是跨两帧的完整回归测试：
/// - old tx：CaretDriven (InsertReveal) + 仍未完成的 Timed Reflow (ReflowMove duration=1000ms)；
/// - new tx：只有 CaretDriven，会成为 owner；
/// - 第 1 帧确认 old tx 失去 owner 但因 Reflow 仍留队列，且 caret_motion_retired 被置 true；
/// - 移除/完成 new tx；
/// - 第 2 帧确认 `active_text_transaction_key_with_epoch()` **不能**重新返回 old tx，
///   `CoordinatedMotionFrame.owner_key` 也不能重新变成 old key。
#[test]
fn issue727_comment5760650874_old_tx_regains_owner_next_frame() {
    let create_now = Instant::now();
    let mut coord = LinuxEditorAnimationCoordinator::new();
    let old_key = VisualTransactionKey::new(100, 100);
    let new_key = VisualTransactionKey::new(200, 200);
    let epoch = 1u64;

    // ── 1. 构造 old tx：含 CaretDriven (InsertReveal) + 未完成的 Timed Reflow ──
    // caret track: from=caret(0), to=caret(100), duration=100ms
    // ReflowMove: duration=1000ms（很长，确保在测试时间窗口内未完成）
    let old_insert_unit = PreparedVisualUnit::wrap(reveal_slice(0, 3, 0.0, 60.0), 100);
    let old_reflow_unit = PreparedVisualUnit::wrap(reflow_slice(3, 6, 60.0, 120.0), 1000);
    let old_cursor_track = PreparedCursorVisualTrack::new_first(
        caret(0.0),
        caret(100.0),
        None,
        None,
        0.0,
        20.0,
        0.0,
        20.0,
        100,
    );
    let old_tx = PreparedTextVisualTransaction {
        key: old_key,
        state: TextVisualTransactionState::Pending,
        operation_kind: TextVisualOperationKind::Insert,
        timeline: TransactionTimeline::new(100),
        units: vec![old_insert_unit, old_reflow_unit],
        old_cursor_rect: Some(caret(0.0)),
        new_cursor_rect: Some(caret(100.0)),
        cursor_visual_track: Some(old_cursor_track),
        cancel_reason: None,
        texture_prepared: false,
        old_snapshot: None,
        new_snapshot: None,
        cursor_owner_epoch: epoch,
        caret_motion_retired: false,
        coordinated: false,
        visual_affected_byte_range_old: None,
        visual_affected_byte_range_new: None,
        layout_basis_revision: LayoutRevision::initial(),
    };
    coord.prepared_queue.enqueue(old_tx);

    // ── 2. 构造 new tx：只有 CaretDriven (InsertReveal)，会成为 owner ──
    // active_text_transaction_key_with_epoch 倒序选最后一个 → new tx
    let new_insert_unit = PreparedVisualUnit::wrap(reveal_slice(0, 3, 100.0, 60.0), 100);
    let new_cursor_track = PreparedCursorVisualTrack::new_first(
        caret(100.0),
        caret(200.0),
        None,
        None,
        0.0,
        20.0,
        0.0,
        20.0,
        100,
    );
    let new_tx = PreparedTextVisualTransaction {
        key: new_key,
        state: TextVisualTransactionState::Pending,
        operation_kind: TextVisualOperationKind::Insert,
        timeline: TransactionTimeline::new(100),
        units: vec![new_insert_unit],
        old_cursor_rect: Some(caret(100.0)),
        new_cursor_rect: Some(caret(200.0)),
        cursor_visual_track: Some(new_cursor_track),
        cancel_reason: None,
        texture_prepared: false,
        old_snapshot: None,
        new_snapshot: None,
        cursor_owner_epoch: epoch,
        caret_motion_retired: false,
        coordinated: false,
        visual_affected_byte_range_old: None,
        visual_affected_byte_range_new: None,
        layout_basis_revision: LayoutRevision::initial(),
    };
    coord.prepared_queue.enqueue(new_tx);

    // ── 3. mark_prepared 两个事务 ──
    coord.prepared_queue.mark_prepared(old_key);
    coord.prepared_queue.mark_prepared(new_key);

    // ── 4. 第 1 帧：begin_rendering_transactions 让两个事务进入 Rendering ──
    let frame_now_0 = create_now + Duration::from_millis(16);
    coord.begin_rendering_transactions(frame_now_0);

    // 构造 sample_0
    let mut sample_0 = AnimationFrameSample::new(frame_now_0);
    sample_0.set_progress(old_key, 0.0);
    sample_0.set_progress(new_key, 0.0);

    // 采样 coordinated motion frame → owner_key 应为 new_key（倒序选最后一个）
    let coordinated_frame_0 =
        coord.sample_coordinated_motion_frame(&sample_0, epoch, LayoutRevision::initial());
    assert_eq!(
        coordinated_frame_0.owner_key,
        Some(new_key),
        "第 1 帧 owner_key 应为 new tx（active_text_transaction_key_with_epoch 倒序选最后一个）"
    );

    // 调用 build_text_animation_plan_with_sample —— 此处应把 old tx 的 caret_motion_retired 置 true
    let (_plan_0, keys_to_complete_0, _) =
        coord.build_text_animation_plan_with_sample(&sample_0, epoch, LayoutRevision::initial());

    // 验证 old tx 不在 keys_to_complete（因为 ReflowMove 未完成，all_units_done == false）
    assert!(
        !keys_to_complete_0.contains(&old_key),
        "第 1 帧 old tx 不应完成：ReflowMove 未播完，all_units_done == false"
    );

    // 验证 old tx 仍在 active queue（因 ReflowMove 未完成）
    let old_tx_still_active = coord
        .prepared_queue
        .active_transactions()
        .iter()
        .any(|t| t.key == old_key);
    assert!(
        old_tx_still_active,
        "第 1 帧 old tx 应仍在 active queue（ReflowMove 未完成）"
    );

    // 方案 A 核心断言：old tx 的 caret_motion_retired 应已被置 true
    {
        let old_tx_ref = coord
            .prepared_queue
            .active_transactions()
            .iter()
            .find(|t| t.key == old_key)
            .expect("old tx 应仍在队列中");
        assert!(
                old_tx_ref.caret_motion_retired,
                "第 1 帧 build_text_animation_plan_with_sample 应把 old tx 的 caret_motion_retired 置 true\
                 （has_caret_driven_units && !owns_caret）"
            );
    }

    // ── 5. 完成 new tx（从队列移除）──
    let removed = coord.prepared_queue.complete(new_key);
    assert!(removed.is_some(), "new tx 应能被 complete");

    // ── 6. 第 2 帧：确认修复——old tx 不能重新成为 owner ──
    // 推进一小段时间（远小于 ReflowMove 的 1000ms，确保 old tx 的 ReflowMove 仍未完成）
    let frame_now_1 = frame_now_0 + Duration::from_millis(50);
    // old tx 的 ReflowMove duration=1000ms，elapsed≈66ms，progress≈0.066 < 1.0 → 未完成

    let mut sample_1 = AnimationFrameSample::new(frame_now_1);
    sample_1.set_progress(old_key, 0.5);

    // 修复后：active_text_transaction_key_with_epoch 跳过 retired 事务，返回 None
    let active_key_1 =
        coord.active_text_transaction_key_with_epoch(epoch, LayoutRevision::initial());
    assert_eq!(
        active_key_1, None,
        "修复后：*不应重新返回 old tx\
             （caret_motion_retired == true，被跳过）"
    );

    // 修复后：sample_coordinated_motion_frame 的 owner_key 为 None（不重新变成 old_key）
    let coordinated_frame_1 =
        coord.sample_coordinated_motion_frame(&sample_1, epoch, LayoutRevision::initial());
    assert_eq!(
        coordinated_frame_1.owner_key, None,
        "修复后：第 2 帧 owner_key 不应重新变成 old tx\
             —— 旧 caret/吞吐字轨迹不会重新接管，不会造成 caret 回跳"
    );

    // 验证 old tx 仍在 active queue（ReflowMove 仍未完成，Timed Reflow 继续播完）
    {
        let old_tx_ref = coord
            .prepared_queue
            .active_transactions()
            .iter()
            .find(|t| t.key == old_key)
            .expect("old tx 应仍在队列中（ReflowMove 未完成）");
        assert!(
            old_tx_ref.caret_motion_retired,
            "第 2 帧 old tx 的 caret_motion_retired 仍应为 true（永久退休，不会重置）"
        );
        // old tx 的 caret track 仍原封不动（from=caret(0), to=caret(100)），
        // 但因为 retired，不会再被选为 owner，不会重新接管。
        let track = old_tx_ref
            .cursor_visual_track
            .as_ref()
            .expect("old tx 应有 caret track");
        assert_eq!(
            (track.from.x, track.to.x),
            (0.0, 100.0),
            "old tx 的 caret track 仍原封不动（from=0, to=100），\
                 但因 caret_motion_retired == true 不会被重新选为 owner"
        );
    }

    // 额外验证：active_text_transaction_key()（无 epoch 版本）也跳过 retired 事务
    let active_key_no_epoch = coord.active_text_transaction_key();
    assert_eq!(
        active_key_no_epoch, None,
        "修复后：active_text_transaction_key()（无 epoch 版本）也应跳过 retired 事务，\
             find_cursor_transaction_for_target / compute_coordinated_cursor_position\
             不会再用 old tx 驱动 caret"
    );

    // 额外验证：再调一次 build_text_animation_plan_with_sample，
    // old tx 的 caret_motion_retired 不会被重置（已经是 true 就保持 true）
    let (_plan_1, _keys_to_complete_1, _) =
        coord.build_text_animation_plan_with_sample(&sample_1, epoch, LayoutRevision::initial());
    {
        let old_tx_ref = coord
            .prepared_queue
            .active_transactions()
            .iter()
            .find(|t| t.key == old_key)
            .expect("第 2 帧 build 后 old tx 应仍在队列中");
        assert!(
            old_tx_ref.caret_motion_retired,
            "第 2 帧 build_text_animation_plan_with_sample 后 old tx 的 caret_motion_retired\
                 仍应为 true（永久退休，不会因再次进入循环而重置）"
        );
    }

    println!(
        "[BUGFIX_VERIFY] Issue #727 评论 5760650874 方案 A: \
             旧 CaretDriven 事务失去 owner 后永远不能再重新获得 owner FIXED"
    );
}
