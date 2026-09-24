//! Issue #738 评论 5796693007 修复后守卫测试 — 验证 2 个并发视觉事务结构缺陷
//! 已被正确修复。
//!
//! 本测试是源代码静态守卫测试（WHITE_BOX），通过读取源文件并检查代码模式来确认
//! 评论 5796693007 指出的 2 个结构缺陷的修复后正确结构在当前代码中确实存在。
//! 测试 PASS = 修复后结构正确。
//!
//! 2 个问题的修复后正确结构：
//! 1. 正文编辑路径先采 rebase frame/handoff 再 retire CaretDriven。
//!    `prepare_edit_motion` 的顺序改为：构造 new_doc_snapshot →
//!    `prepare_rebase_handoff_for_edit`（采 rebase frame + caret handoff，旧事务还活着）→
//!    `reconcile_active_transactions_with_canonical`（retire + rebind）→
//!    `create_transaction_from_prepared_handoff`（用保存的 handoff 创建新事务）。
//!    用统一的 `edit_now` 传给 prepare 和 reconcile。`process_transaction` 保留原内联
//!    match 结构供 issue687/issue702 白盒测试锚点。
//! 2. ReflowMove 的 current_rect.w/h 等于 to_document_rect.w/h（与 compute_frame(ReflowMove)
//!    一致），不是 from→to 插值。`animated_slice.rs` 新增 `sample_current_document_rect`
//!    共用 helper 按 kind 采 current rect，`text_visual_transaction.rs` 的 ReflowMove
//!    current_rect 计算处调用这个 helper。CrossFade 保持四项插值不变。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

fn linux_qt_root() -> PathBuf {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set by cargo");
    PathBuf::from(manifest_dir)
}

fn read_src(rel: &str) -> String {
    let path = linux_qt_root().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e))
}

/// 在 `src` 中定位 `fn_marker`，返回从该处起 `window_size` 字符的函数体窗口。
/// 窗口结束位置回退到最近的 UTF-8 字符边界。
fn function_window(src: &str, fn_marker: &str, window_size: usize) -> String {
    let pos = src
        .find(fn_marker)
        .unwrap_or_else(|| panic!("{} 必须存在", fn_marker));
    let target_end = pos + window_size;
    let window_end = src
        .char_indices()
        .take_while(|(i, _)| *i < target_end)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(src.len())
        .min(src.len());
    src[pos..window_end].to_string()
}

// =========================================================================
// 问题1守卫: 正文编辑路径先采 rebase frame/handoff 再 retire CaretDriven
// =========================================================================

/// 守卫1a: `PreparedRebaseHandoff` 枚举存在，含 Insert/Delete 两个变体，
/// 每个变体携带 rebase_frames、caret_handoff、offset_map、visual_affected_byte_range。
#[test]
fn fix1a_prepared_rebase_handoff_enum_exists() {
    let src = read_src("src/sujian_editor_item/animation/rebase.rs");
    assert!(
        src.contains("enum PreparedRebaseHandoff"),
        "修复后应有 PreparedRebaseHandoff 枚举（prepare 阶段中间状态）。"
    );
    assert!(
        src.contains("PreparedRebaseHandoff::Insert"),
        "修复后 PreparedRebaseHandoff 应有 Insert 变体。"
    );
    assert!(
        src.contains("PreparedRebaseHandoff::Delete"),
        "修复后 PreparedRebaseHandoff 应有 Delete 变体。"
    );
    // 每个变体应携带 rebase_frames 和 caret_handoff。
    assert!(
        src.contains("rebase_frames: Vec<RebaseFrame>"),
        "修复后 PreparedRebaseHandoff 变体应携带 rebase_frames: Vec<RebaseFrame>。"
    );
    assert!(
        src.contains("caret_handoff: Option<RebaseCaretHandoff>"),
        "修复后 PreparedRebaseHandoff 变体应携带 caret_handoff: Option<RebaseCaretHandoff>。"
    );
}

/// 守卫1b: `prepare_rebase_handoff_for_edit` 方法存在，签名含 `now: Instant` 参数
///（用外层统一 now 采样，不内部 Instant::now()）。
#[test]
fn fix1b_prepare_rebase_handoff_for_edit_exists_with_outer_now() {
    let src = read_src("src/sujian_editor_item/animation/rebase.rs");
    // Issue #756: prepare_rebase_handoff_for_edit 增加 coordinated_animation_enabled 参数，
    // 函数体变长，窗口从 4000 增到 5000 以覆盖 take_rebase_frames 调用。
    let window = function_window(&src, "fn prepare_rebase_handoff_for_edit", 5000);
    assert!(
        window.contains("now: Instant"),
        "修复后 prepare_rebase_handoff_for_edit 签名应含 now: Instant 参数（用外层统一 now 采样）。"
    );
    assert!(
        window.contains("take_rebase_frames"),
        "修复后 prepare_rebase_handoff_for_edit 应调 take_rebase_frames 采 rebase frame。"
    );
    // 不应内部重新 Instant::now()（prepare 阶段用外层传入的 now）。
    // 注意：take_rebase_frames 内部不调 Instant::now()，prepare 也不应调。
    // 这里检查 prepare 函数体内不含 `let now = Instant::now();`（用外层 now）。
    assert!(
        !window.contains("let now = Instant::now();"),
        "修复后 prepare_rebase_handoff_for_edit 不应内部 let now = Instant::now();，应用外层传入的 now。"
    );
}

/// 守卫1c: `create_transaction_from_prepared_handoff` 方法存在，接收
/// `Option<PreparedRebaseHandoff>` 并用保存的 handoff 创建新事务。
#[test]
fn fix1c_create_transaction_from_prepared_handoff_exists() {
    let src = read_src("src/sujian_editor_item/animation/transaction_builder.rs");
    let window = function_window(&src, "fn create_transaction_from_prepared_handoff", 8000);
    assert!(
        window.contains("prepared: Option<PreparedRebaseHandoff>"),
        "修复后 create_transaction_from_prepared_handoff 应接收 prepared: Option<PreparedRebaseHandoff>。"
    );
    assert!(
        window.contains("build_cluster_reflow_slices"),
        "修复后 create_transaction_from_prepared_handoff 应调 build_cluster_reflow_slices 创建切片。"
    );
    assert!(
        window.contains("self.prepared_queue.enqueue"),
        "修复后 create_transaction_from_prepared_handoff 应 enqueue 新事务。"
    );
}

/// 守卫1d: `prepare_edit_motion` 中 `prepare_rebase_handoff_for_edit` 调用在
/// `reconcile_active_transactions_with_canonical` 调用之前（先采样再 retire）。
#[test]
fn fix1d_prepare_called_before_reconcile_in_prepare_edit_motion() {
    let src = read_src("src/sujian_editor_item/pipeline.rs");
    let window = function_window(&src, "fn prepare_edit_motion", 32000);

    let prepare_marker = "prepare;"; // .prepare_rebase_handoff_for_edit( 调用
    let reconcile_marker = "reconcile_active_transactions_with_canonical";
    let create_marker = "create_transaction_from_prepared_handoff";

    // 用更精确的锚点：方法调用链。
    let prepare_call = ".prepare_rebase_handoff_for_edit(";
    let create_call = ".create_transaction_from_prepared_handoff(";

    assert!(
        window.contains(prepare_call),
        "修复后 prepare_edit_motion 应调 .prepare_rebase_handoff_for_edit( 采 rebase frame。"
    );
    assert!(
        window.contains(reconcile_marker),
        "修复后 prepare_edit_motion 应调 reconcile_active_transactions_with_canonical。"
    );
    assert!(
        window.contains(create_call),
        "修复后 prepare_edit_motion 应调 .create_transaction_from_prepared_handoff( 创建新事务。"
    );

    let prepare_pos = window.find(prepare_call).expect("prepare 调用已确认存在");
    let reconcile_pos = window
        .find(reconcile_marker)
        .expect("reconcile 调用已确认存在");
    let create_pos = window.find(create_call).expect("create 调用已确认存在");

    assert!(
        prepare_pos < reconcile_pos,
        "修复后 prepare_rebase_handoff_for_edit 调用 ({}) 必须在 reconcile_active_transactions_with_canonical 调用 ({}) 之前，\
         否则旧事务 CaretDriven 会被提前推到终态，采到的 rebase frame 不是真实当前帧。",
        prepare_pos, reconcile_pos
    );
    assert!(
        reconcile_pos < create_pos,
        "修复后 reconcile_active_transactions_with_canonical 调用 ({}) 必须在 create_transaction_from_prepared_handoff 调用 ({}) 之前，\
         保证 retire + rebind 在创建新事务之前完成。",
        reconcile_pos, create_pos
    );
    // 抑制未使用变量警告。
    let _ = (prepare_marker, create_marker);
}

/// 守卫1e: `prepare_edit_motion` 用统一的 `edit_now` 传给 prepare 和 reconcile
///（同一时刻采样，避免 prepare 和 reconcile 用不同 now 导致帧不一致）。
#[test]
fn fix1e_unified_edit_now_passed_to_prepare_and_reconcile() {
    let src = read_src("src/sujian_editor_item/pipeline.rs");
    let window = function_window(&src, "fn prepare_edit_motion", 32000);

    assert!(
        window.contains("let edit_now = Instant::now();"),
        "修复后 prepare_edit_motion 应有 let edit_now = Instant::now(); 统一时间采样。"
    );
    // edit_now 传给 prepare_rebase_handoff_for_edit。
    assert!(
        window.contains("cursor_owner_epoch,\n                edit_now,\n            );"),
        "修复后 edit_now 应传给 prepare_rebase_handoff_for_edit。"
    );
    // edit_now 传给 reconcile_active_transactions_with_canonical。
    assert!(
        window.contains("new_revision,\n                    edit_now,\n                );"),
        "修复后 edit_now 应传给 reconcile_active_transactions_with_canonical。"
    );
}

/// 守卫1f: `prepare_rebase_handoff_for_edit` 不 enqueue 新事务（只采 rebase frame + handoff），
/// 新事务由 `create_transaction_from_prepared_handoff` 创建。
#[test]
fn fix1f_prepare_does_not_enqueue_new_transaction() {
    let src = read_src("src/sujian_editor_item/animation/rebase.rs");
    let window = function_window(&src, "fn prepare_rebase_handoff_for_edit", 5000);

    // prepare 阶段不应 enqueue 新事务（enqueue 在 create 阶段）。
    assert!(
        !window.contains("self.prepared_queue.enqueue"),
        "修复后 prepare_rebase_handoff_for_edit 不应 enqueue 新事务（enqueue 由 create_transaction_from_prepared_handoff 完成）。"
    );
    // prepare 阶段不应 alloc_key（alloc_key 在 create 阶段）。
    assert!(
        !window.contains("self.alloc_key()"),
        "修复后 prepare_rebase_handoff_for_edit 不应 alloc_key（alloc_key 由 create_transaction_from_prepared_handoff 完成）。"
    );
}

// =========================================================================
// 问题2守卫: ReflowMove current_rect.w/h 等于 to_document_rect.w/h
// =========================================================================

/// 守卫2a: `animated_slice.rs` 新增 `sample_current_document_rect` 共用 helper，
/// 按 kind 采 current rect，与 `compute_frame` 对 ReflowMove/ReflowCrossFade 的语义一致。
#[test]
fn fix2a_sample_current_document_rect_helper_exists() {
    let src = read_src("src/sujian_editor_item/animated_slice.rs");
    let window = function_window(&src, "fn sample_current_document_rect", 3000);

    assert!(
        window.contains("kind: AnimatedSliceKind"),
        "修复后 sample_current_document_rect 应接收 kind: AnimatedSliceKind 参数。"
    );
    assert!(
        window.contains("from: &SourceRect"),
        "修复后 sample_current_document_rect 应接收 from: &SourceRect 参数。"
    );
    assert!(
        window.contains("to: &SourceRect"),
        "修复后 sample_current_document_rect 应接收 to: &SourceRect 参数。"
    );
    assert!(
        window.contains("visible: f64"),
        "修复后 sample_current_document_rect 应接收 visible: f64 参数。"
    );
}

/// 守卫2b: helper 的 ReflowMove 分支 w/h 直接用 `to.w`/`to.h`（不插值），
/// 与 `compute_frame(ReflowMove)` 第 481-494 行一致。
#[test]
fn fix2b_helper_reflow_move_uses_to_wh_not_interpolated() {
    let src = read_src("src/sujian_editor_item/animated_slice.rs");
    let window = function_window(&src, "fn sample_current_document_rect", 3000);

    // ReflowMove 分支应含 `w: to.w` 和 `h: to.h`。
    assert!(
        window.contains("w: to.w"),
        "修复后 helper 的 ReflowMove 分支应 w: to.w（与 compute_frame(ReflowMove) 一致，不插值）。"
    );
    assert!(
        window.contains("h: to.h"),
        "修复后 helper 的 ReflowMove 分支应 h: to.h（与 compute_frame(ReflowMove) 一致，不插值）。"
    );
}

/// 守卫2c: helper 的 ReflowCrossFade 分支保持四项插值（x/y/w/h 全部 from→to 插值），
/// 与 `compute_frame(ReflowCrossFade)` 第 496-514 行一致。
#[test]
fn fix2c_helper_reflow_crossfade_keeps_four_way_interpolation() {
    let src = read_src("src/sujian_editor_item/animated_slice.rs");
    let window = function_window(&src, "fn sample_current_document_rect", 3000);

    // CrossFade 分支应含 w/h 的 from→to 插值表达式。
    assert!(
        window.contains("(to.w - from.w)"),
        "修复后 helper 的 ReflowCrossFade 分支应保持 w 的 from→to 插值：(to.w - from.w)。"
    );
    assert!(
        window.contains("(to.h - from.h)"),
        "修复后 helper 的 ReflowCrossFade 分支应保持 h 的 from→to 插值：(to.h - from.h)。"
    );
}

/// 守卫2d: `rebind_timed_units_to_canonical` 中 ReflowMove current_rect 计算处调用
/// `AnimatedSlice::sample_current_document_rect` helper，不再手写四项插值。
#[test]
fn fix2d_rebind_reflow_move_current_rect_uses_helper() {
    let src = read_src("src/sujian_editor_item/animation/transaction/rebind.rs");
    let window = function_window(&src, "fn rebind_timed_units_to_canonical", 30000);

    assert!(
        window.contains("AnimatedSlice::sample_current_document_rect"),
        "修复后 rebind_timed_units_to_canonical 中 ReflowMove current_rect 应调 AnimatedSlice::sample_current_document_rect helper。"
    );
}

/// 守卫2e: ReflowMove current_rect 处不再手写 `(anchor.to_document_rect.w - anchor.from_document_rect.w)`
/// 插值（该表达式只应出现在 CrossFade 的两处）。验证 ReflowMove current_rect 调用 helper 后，
/// helper 的 ReflowMove 分支不含该插值表达式（已在守卫2b 覆盖）。这里额外验证
/// `rebind_timed_units_to_canonical` 中 ReflowMove 区域（调用 helper 处）附近不含
/// `(anchor.to_document_rect.w - anchor.from_document_rect.w)`。
#[test]
fn fix2e_reflow_move_current_rect_no_manual_wh_interpolation() {
    let src = read_src("src/sujian_editor_item/animation/transaction/rebind.rs");
    let window = function_window(&src, "fn rebind_timed_units_to_canonical", 30000);

    // 定位 helper 调用处，检查调用处前后 200 字符不含手写 w 插值。
    let helper_call = "AnimatedSlice::sample_current_document_rect";
    let call_pos = window.find(helper_call).expect("helper 调用已确认存在");
    let region_start = call_pos.saturating_sub(200);
    let region_end = (call_pos + 400).min(window.len());
    let region = &window[region_start..region_end];

    assert!(
        !region.contains("(anchor.to_document_rect.w - anchor.from_document_rect.w)"),
        "修复后 ReflowMove current_rect 调用 helper 处附近不应再手写 (anchor.to_document_rect.w - anchor.from_document_rect.w) 插值。"
    );
    assert!(
        !region.contains("(anchor.to_document_rect.h - anchor.from_document_rect.h)"),
        "修复后 ReflowMove current_rect 调用 helper 处附近不应再手写 (anchor.to_document_rect.h - anchor.from_document_rect.h) 插值。"
    );
}

/// 守卫2f: CrossFade 的两处 current_rect 保持四项插值不变（仍手写 SourceRect 构造，
/// 未改成 helper）。两处分别用 `anchor.from/to_document_rect`（有 reflow_anchors 路径）
/// 和 `new_unit.slice.from/to_document_rect`（无 reflow_anchors fallback 路径）。
#[test]
fn fix2f_crossfade_current_rect_keeps_four_way_interpolation() {
    let src = read_src("src/sujian_editor_item/animation/transaction/rebind.rs");
    let window = function_window(&src, "fn rebind_timed_units_to_canonical", 30000);

    // 有 reflow_anchors 路径：用 anchor.from/to_document_rect 的 w 插值。
    assert!(
        window.contains("(anchor.to_document_rect.w - anchor.from_document_rect.w)"),
        "修复后 CrossFade 有 reflow_anchors 路径应保持 (anchor.to_document_rect.w - anchor.from_document_rect.w) 四项插值。"
    );
    // 无 reflow_anchors fallback 路径：仍手写 SourceRect 构造，用 new_unit.slice.from_document_rect.w
    // 作为 w 插值起点（表达式跨多行，检查起点存在即可证明未改成 helper）。
    assert!(
        window.contains("new_unit.slice.from_document_rect.w"),
        "修复后 CrossFade 无 reflow_anchors fallback 路径应保持手写 SourceRect 构造（含 new_unit.slice.from_document_rect.w），未改成 helper。"
    );
    assert!(
        window.contains("new_unit.slice.to_document_rect.w"),
        "修复后 CrossFade 无 reflow_anchors fallback 路径应保持手写 SourceRect 构造（含 new_unit.slice.to_document_rect.w），未改成 helper。"
    );
}
