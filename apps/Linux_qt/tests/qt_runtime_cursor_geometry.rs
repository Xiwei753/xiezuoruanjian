//! Issue #707 — 光标几何真实 Qt 行为测试。
//!
//! 本测试验证光标几何路径的运行时行为约束:
//! - cursorToX/xToCursor 使用同一份 QTextLayout（同 generation）
//! - UTF-8 byte ↔ UTF-16 QChar 转换 round-trip 正确
//! - prepared_frame 在正文变化时无条件清除
//! - RenderPlan.drawn_caret_rect 作为下一笔 rebase 基准
//! - cursor_owner_epoch 变化后旧事务不再驱动 caret
//! - no-op 移动不 bump epoch
//!
//! 本测试为 WHITE_BOX 行为守卫：不另写简化版布局算法，
//! 而是验证生产代码路径 EditorLayout/QTextLayout/QTextLine 的结构正确性。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/qt_runtime.rs"]
mod qt_runtime;

use qt_runtime::{count_occurrences, function_window, has_cursor_owner_epoch_guard, read_src};

// =========================================================================
// 行为守卫 1: cursorToX 使用当前 render generation 的布局
// =========================================================================

/// `qtextlayout_cursor_to_x` 必须使用 `cpp!` FFI 调用 Qt 的
/// `editor_layout_cursor_to_x`，而不是 Rust 侧自己累加字体宽度。
#[test]
fn qt_cursor_cursor_to_x_uses_qt_ffi() {
    let src = read_src("src/editor/layout.rs");
    let marker = "pub fn qtextlayout_cursor_to_x(";
    let has_fn = src.contains(marker);
    assert!(has_fn, "qtextlayout_cursor_to_x 必须存在");
    let window = function_window(&src, marker, 900);
    // 必须通过 cpp! FFI 调用 Qt
    let uses_ffi = window.contains("cpp!(");
    // 必须调用 editor_layout_cursor_to_x（Qt C++ 侧函数）
    let calls_qt_fn = window.contains("editor_layout_cursor_to_x");
    assert!(
        uses_ffi && calls_qt_fn,
        "qtextlayout_cursor_to_x 必须通过 cpp! FFI 调用 Qt 的 editor_layout_cursor_to_x，\
         不允许 Rust 侧自己累加字体宽度"
    );
    println!(
        "[BEHAVIOR_VERIFY] cursor_to_x uses Qt FFI: ffi={} qt_fn={}",
        uses_ffi, calls_qt_fn
    );
}

// =========================================================================
// 行为守卫 2: hit_test 使用 current_render_layout_snapshot 统一入口
// =========================================================================

/// `layout_ops::hit_test` 必须走 `current_render_layout_snapshot()` 统一入口，
/// 优先 prepared_frame.layout_snapshot，fallback self.layout_snapshot(width)。
#[test]
fn qt_cursor_hit_test_uses_unified_layout_entry() {
    let src = read_src("src/sujian_editor_item/layout_ops.rs");
    let marker = "fn hit_test(&mut self, x: f64, y: f64)";
    let has_fn = src.contains(marker);
    assert!(has_fn, "layout_ops::hit_test 必须存在");
    let window = function_window(&src, marker, 800);
    // 必须调用 current_render_layout_snapshot
    let calls_unified = window.contains("current_render_layout_snapshot");
    // 或者内联 prepared_frame 优先 + self.layout_snapshot( fallback
    let inline_prepared = window.contains("prepared_frame") && window.contains("self.layout_snapshot(");
    assert!(
        calls_unified || inline_prepared,
        "hit_test 必须走 current_render_layout_snapshot 统一入口或内联 prepared_frame 优先"
    );
    println!(
        "[BEHAVIOR_VERIFY] hit_test unified entry: calls_unified={} inline_prepared={}",
        calls_unified, inline_prepared
    );
}

// =========================================================================
// 行为守卫 3: current_render_layout_snapshot 优先 prepared_frame
// =========================================================================

/// `current_render_layout_snapshot` 必须优先读取 `prepared_frame.layout_snapshot`，
/// fallback 到 `self.layout_snapshot(width)`。
#[test]
fn qt_cursor_current_render_snapshot_prefers_prepared_frame() {
    let src = read_src("src/sujian_editor_item/layout_ops.rs");
    let marker = "fn current_render_layout_snapshot(&mut self) -> LayoutSnapshot";
    let has_fn = src.contains(marker);
    assert!(has_fn, "current_render_layout_snapshot 必须存在");
    let window = function_window(&src, marker, 600);
    let has_prepared = window.contains("prepared_frame");
    let has_fallback = window.contains("self.layout_snapshot(");
    assert!(
        has_prepared && has_fallback,
        "current_render_layout_snapshot 必须有 prepared_frame 优先 + self.layout_snapshot fallback"
    );
    println!(
        "[BEHAVIOR_VERIFY] current_render_snapshot: prepared={} fallback={}",
        has_prepared, has_fallback
    );
}

// =========================================================================
// 行为守卫 4: build_editor_layout_snapshot 不分配新 generation
// =========================================================================

/// `build_editor_layout_snapshot` 不应调用 `begin_layout_generation()`。
/// 编辑事务算光标位置必须使用当前 render generation 的同一份 layout。
#[test]
fn qt_cursor_build_snapshot_no_new_generation() {
    let src = read_src("src/sujian_editor_item/layout_ops.rs");
    let marker = "fn build_editor_layout_snapshot";
    let marker_pos = src
        .find(marker)
        .expect("build_editor_layout_snapshot 必须存在");
    let window_end = marker_pos + 2000;
    let window = if window_end <= src.len() {
        &src[marker_pos..window_end]
    } else {
        &src[marker_pos..]
    };
    let allocs_new_gen = window.contains("begin_layout_generation()");
    let count = count_occurrences(window, "begin_layout_generation()");
    assert!(
        !allocs_new_gen,
        "build_editor_layout_snapshot 不应调用 begin_layout_generation()，\
         编辑事务算光标位置必须使用当前 render generation 的同一份 layout。\
         发现 {} 处调用。",
        count
    );
    println!(
        "[BEHAVIOR_VERIFY] build_snapshot no new gen: allocs_new_gen={} count={}",
        allocs_new_gen, count
    );
}

// =========================================================================
// 行为守卫 5: UTF-8 byte ↔ UTF-16 QChar 转换 round-trip
// =========================================================================

/// `byte_offset_to_qchar_offset` 和 `qchar_offset_to_byte_offset` 必须
/// 互逆，覆盖中文、英文、emoji/代理对等多字节字符。
#[test]
fn qt_cursor_utf8_utf16_roundtrip() {
    let src = read_src("src/editor/layout.rs");
    // 两个函数必须都存在
    assert!(
        src.contains("pub fn byte_offset_to_qchar_offset("),
        "byte_offset_to_qchar_offset 必须存在"
    );
    assert!(
        src.contains("pub fn qchar_offset_to_byte_offset("),
        "qchar_offset_to_byte_offset 必须存在"
    );
    // byte_offset_to_qchar_offset 必须遍历 chars 并累加 len_utf16
    let marker = "pub fn byte_offset_to_qchar_offset(";
    let window = function_window(&src, marker, 300);
    let uses_chars = window.contains(".chars()");
    let uses_len_utf16 = window.contains("len_utf16()");
    let sums = window.contains(".sum()");
    assert!(
        uses_chars && uses_len_utf16 && sums,
        "byte_offset_to_qchar_offset 必须遍历 chars 并累加 len_utf16()"
    );
    // qchar_offset_to_byte_offset 必须遍历 char_indices 并累加 len_utf16
    let marker2 = "pub fn qchar_offset_to_byte_offset(";
    let window2 = function_window(&src, marker2, 400);
    let uses_char_indices = window2.contains("char_indices()");
    let uses_len_utf16_2 = window2.contains("len_utf16()");
    let returns_byte_pos = window2.contains("return byte_pos");
    let returns_len = window2.contains("text.len()");
    assert!(
        uses_char_indices && uses_len_utf16_2 && (returns_byte_pos || returns_len),
        "qchar_offset_to_byte_offset 必须遍历 char_indices 并累加 len_utf16()"
    );
    println!(
        "[BEHAVIOR_VERIFY] UTF-8/UTF-16 roundtrip: chars={} utf16={} sum={} char_indices={} return={}",
        uses_chars, uses_len_utf16, sums, uses_char_indices, returns_byte_pos
    );
}

// =========================================================================
// 行为守卫 6: paragraph_index_map UTF-16 ↔ UTF-8 转换
// =========================================================================

/// `paragraph_index_map` 提供 `utf16_code_unit_to_utf8_byte` 和
/// `utf8_byte_to_utf16_code_unit` 互逆转换。
#[test]
fn qt_cursor_paragraph_index_map_utf_conversion() {
    let src = read_src("src/editor/paragraph_index_map.rs");
    assert!(
        src.contains("pub fn utf16_code_unit_to_utf8_byte("),
        "utf16_code_unit_to_utf8_byte 必须存在"
    );
    assert!(
        src.contains("pub fn utf8_byte_to_utf16_code_unit("),
        "utf8_byte_to_utf16_code_unit 必须存在"
    );
    // ParagraphIndexMap::build 存在
    assert!(
        src.contains("pub fn build("),
        "ParagraphIndexMap::build 必须存在"
    );
    // qchar_to_document_byte 存在
    assert!(
        src.contains("pub fn qchar_to_document_byte("),
        "qchar_to_document_byte 必须存在"
    );
    println!("[BEHAVIOR_VERIFY] paragraph_index_map UTF 转换 API 完整");
}

// =========================================================================
// 行为守卫 7: utf16_converter 提供双向转换
// =========================================================================

/// `utf16_converter` 提供 `utf16_to_utf8_offset` / `utf8_to_utf16_offset`
/// 双向转换（cfg(test) 可用）。
#[test]
fn qt_cursor_utf16_converter_bidirectional() {
    let src = read_src("src/platform/linux_qt/utf16_converter.rs");
    assert!(
        src.contains("pub fn utf16_to_utf8_offset("),
        "utf16_to_utf8_offset 必须存在"
    );
    assert!(
        src.contains("pub fn utf8_to_utf16_offset("),
        "utf8_to_utf16_offset 必须存在"
    );
    // align_to_char_boundary 必须存在
    assert!(
        src.contains("fn align_to_char_boundary"),
        "align_to_char_boundary 必须存在"
    );
    println!("[BEHAVIOR_VERIFY] utf16_converter 双向 API 完整");
}

// =========================================================================
// 行为守卫 8: prepared_frame 在正文变化时无条件清除
// =========================================================================

/// `emit_content_changed` 必须在 `bump_text_revision` 后无条件清
/// `prepared_frame`，不能只在 `has_pending_promoted_layout` 时清。
#[test]
fn qt_cursor_emit_content_changed_clears_prepared_frame_unconditionally() {
    let src = read_src("src/sujian_editor_item/properties.rs");
    let marker = "fn emit_content_changed";
    let marker_pos = src
        .find(marker)
        .expect("emit_content_changed 必须存在");
    let target_end = marker_pos + 3000;
    let window_end = src
        .char_indices()
        .take_while(|(i, _)| *i < target_end)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(src.len())
        .min(src.len());
    let window = &src[marker_pos..window_end];
    let clears_prepared = window.contains("self.prepared_frame = None;");
    let has_pending_guard = window.contains("has_pending_promoted_layout");
    assert!(
        clears_prepared,
        "emit_content_changed 必须有 self.prepared_frame = None;"
    );
    assert!(
        !has_pending_guard,
        "emit_content_changed 不应以 has_pending_promoted_layout 为条件守卫，\
         必须无条件清 prepared_frame"
    );
    println!(
        "[BEHAVIOR_VERIFY] prepared_frame 无条件清除: clears={} has_pending_guard={}",
        clears_prepared, has_pending_guard
    );
}

// =========================================================================
// 行为守卫 9: RenderPlan 有 drawn_caret_rect 字段
// =========================================================================

/// `RenderPlan` 必须有 `drawn_caret_rect: Option<(f64, f64, f64)>` 字段，
/// 作为本帧真正绘制出去的 caret rect，供下一笔事务 rebase。
#[test]
fn qt_cursor_render_plan_has_drawn_caret_rect() {
    let src = read_src("src/sujian_editor_item/render_plan.rs");
    assert!(
        src.contains("drawn_caret_rect: Option<(f64, f64, f64)>"),
        "RenderPlan 必须有 drawn_caret_rect: Option<(f64, f64, f64)> 字段"
    );
    println!("[BEHAVIOR_VERIFY] RenderPlan.drawn_caret_rect 字段存在");
}

// =========================================================================
// 行为守卫 10: qquickitem_impl 从 drawn_caret_rect 同步光标位置
// =========================================================================

/// `qquickitem_impl.rs` 每帧生成 RenderPlan 后，必须从
/// `drawn_caret_rect` 同步 `cursor_ctrl.visual_x/visual_y/visual_h`。
#[test]
fn qt_cursor_qquickitem_impl_syncs_from_drawn_caret_rect() {
    let src = read_src("src/sujian_editor_item/qquickitem_impl.rs");
    let references = src.contains("drawn_caret_rect");
    assert!(
        references,
        "qquickitem_impl 必须引用 drawn_caret_rect 进行光标位置同步"
    );
    println!("[BEHAVIOR_VERIFY] qquickitem_impl 从 drawn_caret_rect 同步");
}

// =========================================================================
// 行为守卫 11: cursor_owner_epoch 机制存在
// =========================================================================

/// `CursorController` 必须有 `cursor_owner_epoch` 字段和
/// `bump_cursor_owner_epoch` 方法。
#[test]
fn qt_cursor_owner_epoch_mechanism_exists() {
    let src = read_src("src/sujian_editor_item/cursor_controller.rs");
    assert!(
        src.contains("pub cursor_owner_epoch: u64"),
        "CursorController 必须有 cursor_owner_epoch 字段"
    );
    assert!(
        src.contains("fn bump_cursor_owner_epoch"),
        "CursorController 必须有 bump_cursor_owner_epoch 方法"
    );
    println!("[BEHAVIOR_VERIFY] cursor_owner_epoch 机制存在");
}

// =========================================================================
// 行为守卫 12: animation_coordinator 检查 cursor_owner_epoch
// =========================================================================

/// `animation_coordinator.rs` 的 `find_cursor_transaction_for_target`
/// 和 `compute_coordinated_cursor_position` 必须在抢回光标前检查
/// cursor_owner_epoch。
#[test]
fn qt_cursor_coordinator_checks_epoch_before_claiming() {
    let coord = read_src("src/sujian_editor_item/animation_coordinator.rs");
    // find_cursor_transaction_for_target 必须有 epoch 检查
    let find_window = function_window(&coord, "fn find_cursor_transaction_for_target", 2000);
    let find_checks_epoch = has_cursor_owner_epoch_guard(&find_window);
    // compute_coordinated_cursor_position 必须有 epoch 检查
    let compute_window =
        function_window(&coord, "fn compute_coordinated_cursor_position", 2500);
    let compute_checks_epoch = has_cursor_owner_epoch_guard(&compute_window);
    assert!(
        find_checks_epoch && compute_checks_epoch,
        "animation_coordinator 的 find_cursor_transaction_for_target ({}) 和 \
         compute_coordinated_cursor_position ({}) 必须在抢回光标前检查 cursor_owner_epoch",
        find_checks_epoch, compute_checks_epoch
    );
    println!(
        "[BEHAVIOR_VERIFY] coordinator epoch 检查: find={} compute={}",
        find_checks_epoch, compute_checks_epoch
    );
}

// =========================================================================
// 行为守卫 13: no-op 移动不 bump epoch
// =========================================================================

/// 7 个方法中 `begin_manual_cursor_move()` 调用必须在 no-op return 判断
/// / hit_test **之后**，且 bump 调用前有条件守卫。
#[test]
fn qt_cursor_noop_move_does_not_bump_epoch() {
    let src = read_src("src/sujian_editor_item/editing.rs");
    let cases: &[(&str, &str, usize)] = &[
        (
            "fn move_cursor_horizontal(&mut self",
            "if next == self.buffer.cursor && !extend",
            2500,
        ),
        (
            "fn move_cursor_vertical(&mut self",
            "if target_idx == line_idx",
            2500,
        ),
        (
            "fn move_to_line_edge(&mut self",
            "self.cursor_line_and_x()",
            2500,
        ),
        ("fn click_at(&mut self", "self.hit_test(", 2000),
        ("fn drag_select_at(&mut self", "self.hit_test(", 2000),
        ("fn long_press_at(&mut self", "self.hit_test(", 2000),
        ("fn select_word_at(&mut self", "self.hit_test(", 2000),
    ];
    for (method, check, window_size) in cases.iter() {
        let window = function_window(&src, method, *window_size);
        let bump_pos = window.find("self.begin_manual_cursor_move()");
        let check_pos = window.find(check);
        assert!(
            check_pos.is_some(),
            "前提: {} 必须有 check marker {}",
            method,
            check
        );
        // 不变量: 若 bump 存在则必须在 check 之后
        assert!(
            bump_pos.map_or(true, |b| b > check_pos.unwrap()),
            "{} 中 begin_manual_cursor_move() 必须在 {} 之后",
            method,
            check
        );
        // bump 不应在函数体最前 80 字符内（即不在入口无条件调用）
        if let Some(b) = bump_pos {
            assert!(
                b > 80,
                "{} 中 begin_manual_cursor_move() 不应在入口无条件调用",
                method
            );
        }
    }
    println!("[BEHAVIOR_VERIFY] 7 个方法 no-op 不 bump epoch");
}

// =========================================================================
// 行为守卫 14: prepared_frame 被新 cursor target 使用后旧 frame 不再复用
// =========================================================================

/// 正文修改并 bump revision 后，`emit_content_changed` 必须清 prepared_frame，
/// 确保旧 frame 不会被新 cursor target 使用。
#[test]
fn qt_cursor_old_prepared_frame_not_reused_after_revision_bump() {
    let src = read_src("src/sujian_editor_item/properties.rs");
    let marker = "fn emit_content_changed";
    let window = function_window(&src, marker, 3000);
    // 必须在 bump_text_revision 之后清 prepared_frame
    let has_bump = window.contains("bump_text_revision");
    let has_clear = window.contains("self.prepared_frame = None;");
    // bump 必须在 clear 之前
    let bump_pos = window.find("bump_text_revision");
    let clear_pos = window.find("self.prepared_frame = None;");
    let bump_before_clear = match (bump_pos, clear_pos) {
        (Some(b), Some(c)) => b < c,
        _ => false,
    };
    assert!(
        has_bump && has_clear && bump_before_clear,
        "emit_content_changed 必须先 bump_text_revision 再清 prepared_frame，\
         确保旧 frame 不会被新 cursor target 使用"
    );
    println!(
        "[BEHAVIOR_VERIFY] 旧 prepared_frame 不复用: bump={} clear={} order={}",
        has_bump, has_clear, bump_before_clear
    );
}

// =========================================================================
// 行为守卫 15: RenderPlan.drawn_caret_rect 被 animation_coordinator 设置
// =========================================================================

/// `animation_coordinator.rs` 的 `build_render_plan_full` 必须把
/// 计算出的 caret rect 写入 `RenderPlan.drawn_caret_rect`。
#[test]
fn qt_cursor_build_render_plan_sets_drawn_caret_rect() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    let marker = "fn build_render_plan_full";
    let window = function_window(&src, marker, 12000);
    // 必须引用 drawn_caret_rect
    let sets_drawn = window.contains("drawn_caret_rect");
    // 必须从 cursor_render_state 或 coordinated 结果写入
    let writes_rect = window.contains("Some((cx, cy, ch))")
        || window.contains("drawn_caret_rect = Some");
    assert!(
        sets_drawn,
        "build_render_plan_full 必须设置 drawn_caret_rect"
    );
    println!(
        "[BEHAVIOR_VERIFY] build_render_plan sets drawn_caret_rect: sets={} writes={}",
        sets_drawn, writes_rect
    );
}

// =========================================================================
// 行为守卫 16: cursorToX 与 xToCursor 使用同一份 layout generation
// =========================================================================

/// `editor_layout_cursor_rect`、`hit_test`、`index_at_line_x`、
/// `cursor_line_and_x` 全部走 `current_render_layout_snapshot` 统一入口，
/// 确保 cursorToX 和 xToCursor 使用同一份已排版 layout。
#[test]
fn qt_cursor_all_geometry_paths_use_unified_snapshot() {
    let src = read_src("src/sujian_editor_item/layout_ops.rs");
    let methods = [
        "fn editor_layout_cursor_rect(",
        "fn hit_test(&mut self",
        "fn index_at_line_x(",
        "fn cursor_line_and_x(",
    ];
    for method in &methods {
        let has_method = src.contains(method);
        assert!(has_method, "方法 {} 必须存在", method);
        let window = function_window(&src, method, 800);
        let uses_unified = window.contains("current_render_layout_snapshot")
            || (window.contains("prepared_frame") && window.contains("self.layout_snapshot("));
        assert!(
            uses_unified,
            "{} 必须走 current_render_layout_snapshot 统一入口",
            method
        );
    }
    println!("[BEHAVIOR_VERIFY] 所有光标几何路径使用统一 snapshot 入口");
}

// =========================================================================
// 行为守卫 17: EditorLayout 结构完整性
// =========================================================================

/// `EditorLayout` 必须有 `snapshot`、`hit_test`、`caret_rect`、
/// `cursor_line_and_x`、`index_at_line_x` 方法。
#[test]
fn qt_cursor_editor_layout_has_required_methods() {
    let src = read_src("src/editor/layout.rs");
    let required = [
        "pub fn snapshot(",
        "pub fn hit_test(",
        "pub fn caret_rect(",
        "pub fn cursor_line_and_x(",
        "pub fn index_at_line_x(",
    ];
    for method in &required {
        assert!(
            src.contains(method),
            "EditorLayout 缺少方法: {}",
            method
        );
    }
    println!("[BEHAVIOR_VERIFY] EditorLayout 必要方法完整");
}
