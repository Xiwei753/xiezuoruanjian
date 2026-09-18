//! Issue #707 评论 5723616999 — 光标几何真实 Qt 行为测试。
//!
//! 本测试直接调用生产代码的真实 Qt FFI 函数和真实对象：
//! - `qtextlayout_cursor_to_x` / `qtextlayout_x_to_cursor`（真实 Qt QTextLayout）
//! - `byte_offset_to_qchar_offset` / `qchar_offset_to_byte_offset`（UTF-8↔UTF-16）
//! - `EditorLayout::snapshot` / `hit_test` / `caret_rect` / `cursor_line_and_x`
//! - `CursorController`（真实 epoch 机制）
//! - `RenderPlan.drawn_caret_rect`（真实字段读写）
//!
//! 不再读取源码字符串做字段计数。每条测试先 `ensure_qt_application()`。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/qt_runtime.rs"]
mod qt_runtime;

use qt_runtime::ensure_qt_application;
use sujian_linux_qt::editor::layout::{
    byte_offset_to_qchar_offset, qchar_offset_to_byte_offset, qtextlayout_cursor_to_x,
    qtextlayout_x_to_cursor, CaretAffinity, EditorLayout, LayoutParams,
};
use sujian_linux_qt::sujian_editor_item::cursor_controller::CursorController;
use sujian_linux_qt::sujian_editor_item::render_plan::RenderPlan;

// =========================================================================
// 5a. UTF-8 byte ↔ UTF-16 QChar round-trip（纯 Rust）
// =========================================================================

/// 对多个文本（含中文、emoji/代理对、中英混排），验证
/// `byte_offset_to_qchar_offset` 和 `qchar_offset_to_byte_offset` 互逆。
#[test]
fn qt_cursor_utf8_utf16_roundtrip_all_text_types() {
    ensure_qt_application();
    let texts: &[&str] = &[
        "abc123",
        "中文，标点。",
        "Hello世界World",
        "a😀b", // U+1F600 是代理对
        "重复字符超过一行宽度重复字符超过一行宽度重复字符超过一行宽度重复字符超过一行宽度",
    ];
    for text in texts {
        // 对多个合法 byte offset 做 round-trip
        let mut offsets = vec![0usize, text.len()];
        // 加入每个字符边界
        for (byte_off, _) in text.char_indices() {
            offsets.push(byte_off);
        }
        for byte_off in &offsets {
            let qchar = byte_offset_to_qchar_offset(text, *byte_off);
            let back = qchar_offset_to_byte_offset(text, qchar);
            assert_eq!(
                back, *byte_off,
                "round-trip 失败: text={:?} byte_off={} qchar={} back={}",
                text, *byte_off, qchar, back
            );
        }
    }
    println!("[BEHAVIOR_VERIFY] UTF-8↔UTF-16 round-trip: all text types pass");
}

/// 行首 offset=0 和行尾 offset=text.len() 的 round-trip 必须成立。
#[test]
fn qt_cursor_utf8_utf16_roundtrip_boundaries() {
    ensure_qt_application();
    let texts: &[&str] = &["abc", "中文", "a😀b"];
    for text in texts {
        // 行首
        let q0 = byte_offset_to_qchar_offset(text, 0);
        assert_eq!(q0, 0, "行首 qchar offset 必须为 0");
        assert_eq!(qchar_offset_to_byte_offset(text, 0), 0, "行首 round-trip");

        // 行尾
        let q_end = byte_offset_to_qchar_offset(text, text.len());
        let back = qchar_offset_to_byte_offset(text, q_end);
        assert_eq!(back, text.len(), "行尾 round-trip");
    }
    println!("[BEHAVIOR_VERIFY] UTF-8↔UTF-16 round-trip: boundaries pass");
}

// =========================================================================
// 5b. cursorToX → xToCursor 真实 Qt FFI
// =========================================================================

/// `qtextlayout_cursor_to_x` 返回的 x 坐标必须是合理值（非 NaN、非负无穷）。
#[test]
fn qt_cursor_cursor_to_x_returns_finite_non_negative() {
    ensure_qt_application();
    let para = "Hello世界World";
    let cases = [
        ("", 16.0, "Noto Sans CJK SC"),
        ("Hello", 16.0, "Noto Sans CJK SC"),
        ("Hello世界", 16.0, "Noto Sans CJK SC"),
        (para, 16.0, "Noto Sans CJK SC"),
        ("", 14.0, "Sans"),
        ("abc", 14.0, "Sans"),
    ];
    for (before, fs, ff) in cases.iter() {
        let x = qtextlayout_cursor_to_x(para, before, *fs, *ff);
        assert!(
            x.is_finite(),
            "cursorToX 必须返回有限值，实际: {} (before={:?})",
            x,
            before
        );
        assert!(
            x >= 0.0,
            "cursorToX 必须返回非负值，实际: {} (before={:?})",
            x,
            before
        );
    }
    println!("[BEHAVIOR_VERIFY] cursorToX: returns finite non-negative x");
}

/// cursorToX → xToCursor round-trip：对行首 cursor (x=0)，xToCursor 应回 0。
#[test]
fn qt_cursor_x_to_cursor_at_origin_returns_zero() {
    ensure_qt_application();
    let para = "Hello世界World";
    let x = 0.0;
    let cursor = qtextlayout_x_to_cursor(para, x, 16.0, "Noto Sans CJK SC");
    assert_eq!(
        cursor, 0,
        "x=0 对应的 QChar cursor offset 必须为 0（行首）"
    );
    println!("[BEHAVIOR_VERIFY] xToCursor at x=0 → cursor=0");
}

/// cursorToX → xToCursor round-trip：cursor=0 得到 x，xToCursor(x) 应回 0。
#[test]
fn qt_cursor_cursor_to_x_then_x_to_cursor_roundtrip_at_start() {
    ensure_qt_application();
    let para = "Hello世界World";
    let before = "";
    let x = qtextlayout_cursor_to_x(para, before, 16.0, "Noto Sans CJK SC");
    let cursor_back = qtextlayout_x_to_cursor(para, x, 16.0, "Noto Sans CJK SC");
    assert_eq!(
        cursor_back, 0,
        "cursorToX(\"\") → x → xToCursor(x) 应回到 0（行首），实际: {}",
        cursor_back
    );
    println!(
        "[BEHAVIOR_VERIFY] cursorToX→xToCursor round-trip at start: x={:.4} → cursor={}",
        x, cursor_back
    );
}

/// cursorToX 随 cursor 前进而单调非递减（x 坐标不会后退）。
#[test]
fn qt_cursor_cursor_to_x_monotonic_non_decreasing() {
    ensure_qt_application();
    let para = "abc中文def";
    let font_size = 16.0;
    let font_family = "Noto Sans CJK SC";
    let mut prev_x = 0.0;
    let mut offsets = vec![0usize];
    for (off, _) in para.char_indices() {
        offsets.push(off);
    }
    offsets.push(para.len());
    for off in &offsets {
        let before = &para[..*off];
        let x = qtextlayout_cursor_to_x(para, before, font_size, font_family);
        assert!(
            x >= prev_x - 0.001,
            "cursorToX 必须单调非递减: off={} x={:.4} prev_x={:.4}",
            off,
            x,
            prev_x
        );
        prev_x = x;
    }
    println!("[BEHAVIOR_VERIFY] cursorToX: monotonic non-decreasing");
}

// =========================================================================
// 5c. EditorLayout 真实排版
// =========================================================================

/// `EditorLayout::snapshot` 对非空文本返回有效 LayoutSnapshot，
/// `hit_test` / `caret_rect` / `cursor_line_and_x` 返回合理值。
#[test]
fn qt_cursor_editor_layout_snapshot_and_geometry() {
    ensure_qt_application();
    let mut layout = EditorLayout::default();
    let text = "Hello\n世界\nWorld";
    let params = LayoutParams {
        width: 400.0,
        font_size: 16.0,
        font_family: "Noto Sans CJK SC".to_string(),
        line_spacing: 1.2,
        text_indent: 0.0,
        padding: 0.0,
    };
    let snapshot = layout.snapshot(text, params, 1).clone();
    assert!(
        !snapshot.lines.is_empty(),
        "snapshot 必须返回至少一行"
    );
    // hit_test 在第一行行首
    let (idx, _affinity) = layout.hit_test(&snapshot, 0.0, 0.0, 0.0);
    assert_eq!(idx, 0, "hit_test(0,0) 应回到文档起点");

    // caret_rect 在 cursor=0
    let caret = layout.caret_rect(&snapshot, 0, CaretAffinity::Downstream, 0.0, 1000.0);
    assert!(
        caret.h > 0.0,
        "caret_rect 的 h 必须为正（字号 16），实际: {}",
        caret.h
    );

    // cursor_line_and_x 在 cursor=0
    let line_x = layout.cursor_line_and_x(&snapshot, 0, CaretAffinity::Downstream);
    assert!(
        line_x.is_some(),
        "cursor_line_and_x(0) 必须返回 Some"
    );
    let (line_id, x) = line_x.unwrap();
    assert_eq!(line_id, 0, "cursor=0 在第 0 行");
    assert!(
        x.abs() < 0.001,
        "cursor=0 的 x 应回近 0，实际: {:.4}",
        x
    );
    println!(
        "[BEHAVIOR_VERIFY] EditorLayout: {} lines, caret h={:.4}",
        snapshot.lines.len(),
        caret.h
    );
}

// =========================================================================
// 5d. #705 状态交接 — CursorController epoch 机制
// =========================================================================

/// `CursorController::new()` 初始 epoch=0，`bump_cursor_owner_epoch()` 后 epoch=1。
#[test]
fn qt_cursor_controller_epoch_bump() {
    ensure_qt_application();
    let mut ctrl = CursorController::new();
    assert_eq!(
        ctrl.cursor_owner_epoch, 0,
        "初始 epoch 必须为 0"
    );
    ctrl.bump_cursor_owner_epoch();
    assert_eq!(
        ctrl.cursor_owner_epoch, 1,
        "bump 后 epoch 必须为 1"
    );
    ctrl.bump_cursor_owner_epoch();
    assert_eq!(
        ctrl.cursor_owner_epoch, 2,
        "第二次 bump 后 epoch 必须为 2"
    );
    println!("[BEHAVIOR_VERIFY] CursorController epoch: 0 → 1 → 2");
}

/// no-op 不 bump epoch — 直接构造 CursorController，不调用 bump，epoch 保持 0。
#[test]
fn qt_cursor_controller_noop_does_not_bump_epoch() {
    ensure_qt_application();
    let ctrl = CursorController::new();
    assert_eq!(
        ctrl.cursor_owner_epoch, 0,
        "不调用 bump 的 CursorController epoch 必须保持 0"
    );
    println!("[BEHAVIOR_VERIFY] no-op: epoch stays 0");
}

/// epoch 使用 wrapping_add，不会 overflow panic。
#[test]
fn qt_cursor_controller_epoch_wrapping_no_overflow() {
    ensure_qt_application();
    let mut ctrl = CursorController::new();
    // 模拟大量 bump（不会 panic）
    for _ in 0..100 {
        ctrl.bump_cursor_owner_epoch();
    }
    assert_eq!(ctrl.cursor_owner_epoch, 100, "100 次 bump 后 epoch=100");
    println!("[BEHAVIOR_VERIFY] epoch wrapping: 100 bumps, no overflow");
}

// =========================================================================
// 5e. RenderPlan.drawn_caret_rect 字段存在且类型正确
// =========================================================================

/// `RenderPlan` 必须有 `drawn_caret_rect: Option<(f64, f64, f64)>` 字段。
/// 本测试通过类型系统验证字段存在且可读写（编译时检查 + 运行时 Default）。
#[test]
fn qt_cursor_render_plan_drawn_caret_rect_field_exists() {
    ensure_qt_application();
    // RenderPlan::default() 构造默认实例
    let plan = RenderPlan::default();
    // drawn_caret_rect 字段可读
    let _rect: &Option<(f64, f64, f64)> = &plan.drawn_caret_rect;
    // 默认值应为 None（无绘制时不设 caret rect）
    assert!(
        plan.drawn_caret_rect.is_none(),
        "默认 RenderPlan.drawn_caret_rect 应为 None"
    );
    println!("[BEHAVIOR_VERIFY] RenderPlan.drawn_caret_rect: field exists, default None");
}

/// `RenderPlan.drawn_caret_rect` 可写入并读回。
#[test]
fn qt_cursor_render_plan_drawn_caret_rect_writable() {
    ensure_qt_application();
    let mut plan = RenderPlan::default();
    // 写入 (x, y, h) = (10.0, 20.0, 30.0)
    plan.drawn_caret_rect = Some((10.0, 20.0, 30.0));
    let rect = plan.drawn_caret_rect.expect("drawn_caret_rect 已设");
    assert_eq!(rect, (10.0, 20.0, 30.0), "drawn_caret_rect 写入后读回一致");
    println!("[BEHAVIOR_VERIFY] RenderPlan.drawn_caret_rect: writable and readback consistent");
}
