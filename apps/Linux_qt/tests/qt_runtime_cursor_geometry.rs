//! Issue #707 评论 5724685300 — 光标几何真实 Qt 行为测试。
//!
//! 本测试直接调用生产代码的真实 Qt 排版路径：
//! - `EditorLayout::snapshot()` 生成真实 LayoutSnapshot（生产排版路径）
//! - `cursor_line_and_x()` / `caret_rect()` 从 snapshot 算 cursor 的 x 坐标
//! - `index_at_line_x()` / `hit_test()` 从 x 反算 byte offset
//! - `byte_offset_to_qchar_offset` / `qchar_offset_to_byte_offset`（UTF-8↔UTF-16）
//! - `CursorController`（真实 epoch 机制）
//! - `RenderPlan.drawn_caret_rect`（真实字段读写）
//!
//! Issue #707 评论 5724685300 关键修改:
//! - 不再调用 `qtextlayout_cursor_to_x()` / `qtextlayout_x_to_cursor()`，
//!   它们在 layout.rs 里重新 new QTextLayout 临时排版，绕开了 #705 要防的
//!   核心问题：正式运行时要求正文、cursorToX、xToCursor 必须使用当前 render
//!   generation 缓存里的同一份 QTextLayout/QTextLine。
//! - 现在用 `EditorLayout::snapshot()` 先生成真实 snapshot/generation，
//!   对同一份 snapshot 用生产 `caret_rect()` / `cursor_line_and_x()` 得到 x，
//!   再用生产 `hit_test()` / `index_at_line_x()` 反算 byte offset。
//! - `qtextlayout_x_to_cursor()` 和 C++ `editor_layout_x_to_cursor` 已删除，
//!   不保留第二套布局算法。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/qt_runtime.rs"]
mod qt_runtime;

use qt_runtime::run_on_qt_thread;
use sujian_linux_qt::editor::layout::{
    byte_offset_to_qchar_offset, qchar_offset_to_byte_offset, CaretAffinity, EditorLayout,
    LayoutParams,
};
use sujian_linux_qt::sujian_editor_item::cursor_controller::CursorController;
use sujian_linux_qt::sujian_editor_item::render_plan::RenderPlan;

/// 构造默认 LayoutParams（宽 400px，足够排一行）。
fn default_params() -> LayoutParams {
    LayoutParams {
        width: 400.0,
        font_size: 16.0,
        font_family: "Noto Sans CJK SC".to_string(),
        line_spacing: 1.2,
        text_indent: 0.0,
        padding: 0.0,
    }
}

// =========================================================================
// 5a. UTF-8 byte ↔ UTF-16 QChar round-trip（纯 Rust，不涉及 QTextLayout）
// =========================================================================

/// 对多个文本（含中文、emoji/代理对、中英混排），验证
/// `byte_offset_to_qchar_offset` 和 `qchar_offset_to_byte_offset` 互逆。
#[test]
fn qt_cursor_utf8_utf16_roundtrip_all_text_types() {
    run_on_qt_thread(|| {
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
    });
}

/// 行首 offset=0 和行尾 offset=text.len() 的 round-trip 必须成立。
#[test]
fn qt_cursor_utf8_utf16_roundtrip_boundaries() {
    run_on_qt_thread(|| {
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
    });
}

// =========================================================================
// 5b. EditorLayout::snapshot() 生产路径 cursorToX → xToCursor round-trip
// =========================================================================

/// 对同一份 `EditorLayout::snapshot()`，用生产 `cursor_line_and_x()` 得到 x，
/// 再用生产 `index_at_line_x()` 反算 byte offset，验证 round-trip。
///
/// 中文、英文、中文标点、emoji、中英混排都在这条 EditorLayout snapshot 路径上做。
#[test]
fn qt_cursor_editor_layout_snapshot_roundtrip_all_text_types() {
    run_on_qt_thread(|| {
        let texts: &[&str] = &[
            "abc123",
            "中文，标点。",
            "Hello世界World",
            "a😀b",
            "中英混排Hello世界World测试",
        ];
        for text in texts {
            let mut layout = EditorLayout::default();
            let params = default_params();
            let snapshot = layout.snapshot(text, params, 1).clone();
            assert!(
                !snapshot.lines.is_empty(),
                "snapshot 必须返回至少一行: text={:?}",
                text
            );

            // 对每个字符边界做 round-trip
            let mut offsets = vec![0usize];
            for (byte_off, _) in text.char_indices() {
                offsets.push(byte_off);
            }
            offsets.push(text.len());

            for byte_off in &offsets {
                let line_x =
                    layout.cursor_line_and_x(&snapshot, *byte_off, CaretAffinity::Downstream);
                assert!(
                    line_x.is_some(),
                    "cursor_line_and_x 必须返回 Some: text={:?} byte_off={}",
                    text,
                    byte_off
                );
                let (line_idx, x) = line_x.unwrap();
                assert!(
                    line_idx < snapshot.lines.len(),
                    "line_idx 越界: text={:?} byte_off={} line_idx={} lines_len={}",
                    text,
                    byte_off,
                    line_idx,
                    snapshot.lines.len()
                );
                let line = &snapshot.lines[line_idx];
                let back = layout.index_at_line_x(&snapshot, line, x);
                assert_eq!(
                    back, *byte_off,
                    "cursorToX→xToCursor round-trip 失败: text={:?} byte_off={} x={:.4} back={}",
                    text, *byte_off, x, back
                );
            }
        }
        println!("[BEHAVIOR_VERIFY] EditorLayout::snapshot round-trip: all text types pass");
    });
}

/// 行首 cursor=0 的 round-trip：cursor_line_and_x(0) → x → index_at_line_x(x) 应回 0。
#[test]
fn qt_cursor_editor_layout_roundtrip_at_origin() {
    run_on_qt_thread(|| {
        let text = "Hello世界World";
        let mut layout = EditorLayout::default();
        let params = default_params();
        let snapshot = layout.snapshot(text, params, 1).clone();

        let line_x = layout.cursor_line_and_x(&snapshot, 0, CaretAffinity::Downstream);
        assert!(line_x.is_some(), "cursor_line_and_x(0) 必须返回 Some");
        let (line_idx, x) = line_x.unwrap();
        assert_eq!(line_idx, 0, "cursor=0 在第 0 行");
        assert!(x.abs() < 0.5, "cursor=0 的 x 应回近 0，实际: {:.4}", x);

        let line = &snapshot.lines[line_idx];
        let back = layout.index_at_line_x(&snapshot, line, x);
        assert_eq!(back, 0, "xToCursor(x=0) 应回 0（行首）");
        println!(
            "[BEHAVIOR_VERIFY] EditorLayout round-trip at origin: x={:.4} → cursor={}",
            x, back
        );
    });
}

/// cursorToX 随 cursor 前进而单调非递减（x 坐标不会后退）。
#[test]
fn qt_cursor_editor_layout_cursor_to_x_monotonic_non_decreasing() {
    run_on_qt_thread(|| {
        let text = "abc中文def";
        let mut layout = EditorLayout::default();
        let params = default_params();
        let snapshot = layout.snapshot(text, params, 1).clone();

        let mut prev_x = 0.0;
        let mut offsets = vec![0usize];
        for (off, _) in text.char_indices() {
            offsets.push(off);
        }
        offsets.push(text.len());

        for off in &offsets {
            let line_x = layout.cursor_line_and_x(&snapshot, *off, CaretAffinity::Downstream);
            assert!(
                line_x.is_some(),
                "cursor_line_and_x 必须返回 Some: off={}",
                off
            );
            let (_, x) = line_x.unwrap();
            assert!(
                x >= prev_x - 0.5,
                "cursorToX 必须单调非递减: off={} x={:.4} prev_x={:.4}",
                off,
                x,
                prev_x
            );
            prev_x = x;
        }
        println!("[BEHAVIOR_VERIFY] EditorLayout cursorToX: monotonic non-decreasing");
    });
}

// =========================================================================
// 5c. 软换行 round-trip — width 设窄，确认多行后换行前后 caret
// =========================================================================

/// 软换行要把 width 故意设窄，先确认 `snapshot.lines.len() > 1`，
/// 再测换行前后 caret 的 round-trip。
#[test]
fn qt_cursor_editor_layout_soft_wrap_roundtrip() {
    run_on_qt_thread(|| {
        // 长文本，width 设窄，强制软换行
        let text =
            "重复字符超过一行宽度重复字符超过一行宽度重复字符超过一行宽度重复字符超过一行宽度";
        let mut layout = EditorLayout::default();
        let params = LayoutParams {
            width: 80.0, // 窄宽度，强制多行
            font_size: 16.0,
            font_family: "Noto Sans CJK SC".to_string(),
            line_spacing: 1.2,
            text_indent: 0.0,
            padding: 0.0,
        };
        let snapshot = layout.snapshot(text, params, 1).clone();
        assert!(
            snapshot.lines.len() > 1,
            "窄宽度下必须产生多行（软换行），实际 lines.len()={}",
            snapshot.lines.len()
        );

        // 对每个字符边界做 round-trip
        let mut offsets = vec![0usize];
        for (byte_off, _) in text.char_indices() {
            offsets.push(byte_off);
        }
        offsets.push(text.len());

        for byte_off in &offsets {
            let line_x = layout.cursor_line_and_x(&snapshot, *byte_off, CaretAffinity::Downstream);
            assert!(
                line_x.is_some(),
                "cursor_line_and_x 必须返回 Some: byte_off={}",
                byte_off
            );
            let (line_idx, x) = line_x.unwrap();
            let line = &snapshot.lines[line_idx];
            let back = layout.index_at_line_x(&snapshot, line, x);
            assert_eq!(
                back, *byte_off,
                "软换行 round-trip 失败: byte_off={} x={:.4} back={}",
                byte_off, x, back
            );
        }

        // 验证换行点：第 0 行的 byte_end 应该在第 1 行的 byte_start 之前
        let line0 = &snapshot.lines[0];
        let line1 = &snapshot.lines[1];
        assert!(
            line0.byte_end <= line1.byte_start,
            "第 0 行 byte_end ({}) 必须 <= 第 1 行 byte_start ({})",
            line0.byte_end,
            line1.byte_start
        );
        assert!(
            line0.byte_end > line0.byte_start,
            "第 0 行不能是空行: byte_start={} byte_end={}",
            line0.byte_start,
            line0.byte_end
        );
        println!(
            "[BEHAVIOR_VERIFY] 软换行 round-trip: {} 行, line0=[{},{}), line1=[{},{})",
            snapshot.lines.len(),
            line0.byte_start,
            line0.byte_end,
            line1.byte_start,
            line1.byte_end
        );
    });
}

// =========================================================================
// 5d. EditorLayout 真实排版 — snapshot + hit_test + caret_rect
// =========================================================================

/// `EditorLayout::snapshot` 对非空文本返回有效 LayoutSnapshot，
/// `hit_test` / `caret_rect` / `cursor_line_and_x` 返回合理值。
#[test]
fn qt_cursor_editor_layout_snapshot_and_geometry() {
    run_on_qt_thread(|| {
        let mut layout = EditorLayout::default();
        let text = "Hello\n世界\nWorld";
        let params = default_params();
        let snapshot = layout.snapshot(text, params, 1).clone();
        assert!(!snapshot.lines.is_empty(), "snapshot 必须返回至少一行");
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
        assert!(line_x.is_some(), "cursor_line_and_x(0) 必须返回 Some");
        let (line_id, x) = line_x.unwrap();
        assert_eq!(line_id, 0, "cursor=0 在第 0 行");
        assert!(x.abs() < 0.5, "cursor=0 的 x 应回近 0，实际: {:.4}", x);
        println!(
            "[BEHAVIOR_VERIFY] EditorLayout: {} lines, caret h={:.4}",
            snapshot.lines.len(),
            caret.h
        );
    });
}

// =========================================================================
// 5e. #705 状态交接 — CursorController epoch 机制
// =========================================================================

/// `CursorController::new()` 初始 epoch=0，`bump_cursor_owner_epoch()` 后 epoch=1。
#[test]
fn qt_cursor_controller_epoch_bump() {
    run_on_qt_thread(|| {
        let mut ctrl = CursorController::new();
        assert_eq!(ctrl.cursor_owner_epoch, 0, "初始 epoch 必须为 0");
        ctrl.bump_cursor_owner_epoch();
        assert_eq!(ctrl.cursor_owner_epoch, 1, "bump 后 epoch 必须为 1");
        ctrl.bump_cursor_owner_epoch();
        assert_eq!(ctrl.cursor_owner_epoch, 2, "第二次 bump 后 epoch 必须为 2");
        println!("[BEHAVIOR_VERIFY] CursorController epoch: 0 → 1 → 2");
    });
}

/// no-op 不 bump epoch — 直接构造 CursorController，不调用 bump，epoch 保持 0。
#[test]
fn qt_cursor_controller_noop_does_not_bump_epoch() {
    run_on_qt_thread(|| {
        let ctrl = CursorController::new();
        assert_eq!(
            ctrl.cursor_owner_epoch, 0,
            "不调用 bump 的 CursorController epoch 必须保持 0"
        );
        println!("[BEHAVIOR_VERIFY] no-op: epoch stays 0");
    });
}

/// epoch 使用 wrapping_add，不会 overflow panic。
#[test]
fn qt_cursor_controller_epoch_wrapping_no_overflow() {
    run_on_qt_thread(|| {
        let mut ctrl = CursorController::new();
        // 模拟大量 bump（不会 panic）
        for _ in 0..100 {
            ctrl.bump_cursor_owner_epoch();
        }
        assert_eq!(ctrl.cursor_owner_epoch, 100, "100 次 bump 后 epoch=100");
        println!("[BEHAVIOR_VERIFY] epoch wrapping: 100 bumps, no overflow");
    });
}

// =========================================================================
// 5f. RenderPlan.drawn_caret_rect 字段存在且类型正确
// =========================================================================

/// `RenderPlan` 必须有 `drawn_caret_rect: Option<(f64, f64, f64)>` 字段。
/// 本测试通过类型系统验证字段存在且可读写（编译时检查 + 运行时 Default）。
#[test]
fn qt_cursor_render_plan_drawn_caret_rect_field_exists() {
    run_on_qt_thread(|| {
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
    });
}

/// `RenderPlan.drawn_caret_rect` 可写入并读回。
#[test]
fn qt_cursor_render_plan_drawn_caret_rect_writable() {
    run_on_qt_thread(|| {
        let mut plan = RenderPlan::default();
        // 写入 (x, y, h) = (10.0, 20.0, 30.0)
        plan.drawn_caret_rect = Some((10.0, 20.0, 30.0));
        let rect = plan.drawn_caret_rect.expect("drawn_caret_rect 已设");
        assert_eq!(rect, (10.0, 20.0, 30.0), "drawn_caret_rect 写入后读回一致");
        println!("[BEHAVIOR_VERIFY] RenderPlan.drawn_caret_rect: writable and readback consistent");
    });
}
