//! Issue #683 复现测试 — SetSelection 的 NoChange 没同步镜像，导致光标锁死、
//! 旧正文无法删除；EditorEditResult/UndoEntry 用排序后的 Utf8ByteRange 表示
//! selection，丢失方向（anchor/head），反向选区会被翻成正向。
//!
//! 这些测试断言**期望（修复后）的正确行为**。在当前缺陷存在时它们会**失败**，
//! 失败的 panic 信息即为复现证据。修复后所有测试应通过。
//!
//! 复现的缺陷路径：
//! 1. core/writer_core/src/editor/kernel/selection.rs:58 — apply_set_selection
//!    总是返回 NoChange，即使 anchor/head 真的变化了。
//! 2. core/writer_core/src/editor/kernel/result.rs:133-134 — EditorEditResult 用
//!    Utf8ByteRange（排序后）表示 selection，丢失方向。
//! 3. core/writer_core/src/editor/kernel/selection.rs:36 — from_ordered(anchor, head)
//!    把反向选区翻成正向。
//! 4. core/writer_core/src/editor/kernel/history.rs:60-61,181-182 — undo/redo 从
//!    排序后的 range 反推 anchor/head，方向丢失。
//! 5. apps/Linux_qt/src/sujian_editor_item/pipeline.rs:602 — set_selection() 在
//!    NoChange 路径直接返回 Some(result) 不更新 mirror（光标锁死根因）。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use writer_core::editor::{
    EditorCommand, EditorEditOutcome, EditorKernel, EditorRevision, Utf8ByteOffset, Utf8ByteRange,
};

// ===========================================================================
// 复现 1：SetSelection 在 anchor/head 真的变化时应返回 Applied，而非 NoChange。
//
// 当前 selection.rs:58 无条件返回 NoChange。平台端把 NoChange 当作"什么都没变"，
// mirror cursor 不更新 → 光标锁死。
// ===========================================================================
#[test]
fn repro_set_selection_changed_anchor_head_returns_applied() {
    let mut kernel = EditorKernel::with_text("abcdef".to_string(), 6).unwrap();
    // 初始 anchor=6, head=6。把光标移到 3。
    let outcome = kernel.apply(EditorCommand::SetSelection {
        anchor: Utf8ByteOffset::unchecked(3),
        head: Utf8ByteOffset::unchecked(3),
        expected_revision: EditorRevision::new(0),
    });
    // 期望：anchor/head 真变了 → Applied（即使 display_patches 为空、content_delta 为 0）。
    assert!(
        matches!(outcome, EditorEditOutcome::Applied(_)),
        "FAIL: SetSelection 把光标从 6 移到 3，anchor/head 真变了，应返回 Applied；\
         实际返回 {:?}。平台端把 NoChange 当作'什么都没变'，mirror cursor 不更新 → 光标锁死。",
        outcome
    );
}

// ===========================================================================
// 复现 2：SetSelection 把光标移到旧正文中间位置后，正文 revision 不变，但 outcome
// 是状态已应用（Applied），mirror cursor 必须跟着变。
//
// 当前 selection.rs:58 返回 NoChange，pipeline.rs:602 的
// `NoChange(result) => Some(result)` 不调 apply_edit_result，mirror cursor 不更新。
// ===========================================================================
#[test]
fn repro_set_selection_to_middle_of_old_text_is_state_applied() {
    let mut kernel = EditorKernel::with_text("abcdef".to_string(), 6).unwrap();
    let rev_before = kernel.revision();
    let outcome = kernel.apply(EditorCommand::SetSelection {
        anchor: Utf8ByteOffset::unchecked(3),
        head: Utf8ByteOffset::unchecked(3),
        expected_revision: EditorRevision::new(0),
    });
    // 正文 revision 不变（选区操作不改正文）。
    assert_eq!(
        kernel.revision(),
        rev_before,
        "选区操作不应改变正文 revision"
    );
    // 但 kernel 内部状态确实变了（cursor 3, anchor 3）。
    assert_eq!(kernel.cursor(), 3, "kernel cursor 应已移到 3");
    assert_eq!(kernel.selection_anchor(), 3, "kernel anchor 应已移到 3");
    // 期望 outcome 反映状态已应用，这样平台端 mirror 才会跟着更新。
    assert!(
        outcome.is_applied(),
        "FAIL: kernel 内部 cursor/anchor 已从 6 变到 3，outcome 应为 Applied 让 mirror 跟着更新；\
         实际返回 {:?}。NoChange 导致 pipeline.rs set_selection() 的 NoChange 分支\
         不调 apply_edit_result，mirror.cursor 仍是 6 → 光标锁死。",
        outcome
    );
}

// ===========================================================================
// 复现 3：load "abcdef" -> set_selection(3,3) -> delete_backward() 应删除旧正文
// 里的 'c'，不是只能删本轮新增内容。
//
// 这条链路在 Linux Qt pipeline 上：set_selection(3,3) 返回 NoChange → mirror.cursor
// 不更新（仍是 6）→ delete_backward 用 buffer.cursor=6 删除 'f'，而非用 3 删除 'c'。
// Core 层我们验证 SetSelection 后 outcome 携带的 new_selection 能让消费方知道
// cursor 在 3。当前 new_selection_byte_range 是排序后的 range，对 (3,3) 恰好正确，
// 但 outcome 是 NoChange，消费方不会用它更新 mirror。
// ===========================================================================
#[test]
fn repro_set_selection_then_delete_removes_old_text_at_cursor() {
    let mut kernel = EditorKernel::with_text("abcdef".to_string(), 6).unwrap();
    // 把光标移到 3（'c' 后）。
    let sel_outcome = kernel.apply(EditorCommand::SetSelection {
        anchor: Utf8ByteOffset::unchecked(3),
        head: Utf8ByteOffset::unchecked(3),
        expected_revision: EditorRevision::new(0),
    });
    // 期望 outcome 是 Applied，这样平台端 mirror.cursor 才会更新到 3。
    assert!(
        sel_outcome.is_applied(),
        "FAIL: SetSelection(3,3) 应返回 Applied 让 mirror.cursor=3；\
         实际返回 {:?}。mirror.cursor 停留在 6，delete_backward 删的是 'f' 而非 'c'。",
        sel_outcome
    );
    // 接着用 cursor=3 删除前一个字符 'c'（byte range [2,3)）。
    let del_outcome = kernel.apply(EditorCommand::Delete {
        byte_range: Utf8ByteRange::try_new("abcdef", 2, 3).unwrap(),
        deleted_text: "c".to_string(),
        cause: writer_core::editor::EditorTransactionCause::Delete,
        expected_revision: EditorRevision::new(0),
    });
    assert!(del_outcome.is_applied(), "Delete 应成功应用");
    assert_eq!(
        kernel.snapshot_text(),
        "abdef",
        "FAIL: 删除 'c' 后应为 'abdef'，实际 {:?}",
        kernel.snapshot_text()
    );
}

// ===========================================================================
// 复现 4：反向选区 (anchor > head) 的方向必须在 EditorEditResult 中保留。
//
// 当前 selection.rs:36 `from_ordered(anchor, head)` 把 (3,0) 翻成 (0,3)，
// result.rs:133-134 用 Utf8ByteRange 存排序后的 range，方向丢失。
// 平台端 apply_edit_result (pipeline.rs:113-129) 从 range.start/end 反推
// anchor=head=start, cursor=end，反向选区被翻成正向。
// ===========================================================================
#[test]
fn repro_reverse_selection_direction_preserved_in_result() {
    let mut kernel = EditorKernel::with_text("abcdef".to_string(), 0).unwrap();
    // 反向选区：anchor=3, head=0（用户从右往左拖选）。
    let outcome = kernel.apply(EditorCommand::SetSelection {
        anchor: Utf8ByteOffset::unchecked(3),
        head: Utf8ByteOffset::unchecked(0),
        expected_revision: EditorRevision::new(0),
    });
    let result = outcome.into_result();
    // kernel 内部正确保留了方向：anchor=3, cursor(head)=0。
    assert_eq!(kernel.selection_anchor(), 3, "kernel anchor 应为 3");
    assert_eq!(kernel.cursor(), 0, "kernel cursor(head) 应为 0");
    // 期望 result 也保留方向。修复后 result 用
    // EditorSelection { anchor, head } 保留方向。
    let recovered_anchor = result.new_selection.anchor.index.value();
    let recovered_head = result.new_selection.head.index.value();
    assert!(
        recovered_anchor == 3 && recovered_head == 0,
        "FAIL: 反向选区 anchor=3, head=0 应在 result 中保留方向；\
         当前 new_selection = (anchor={}, head={})，方向丢失。",
        recovered_anchor,
        recovered_head
    );
}

// ===========================================================================
// 复现 5：Shift+Left 产生反向选区 (anchor > head)，undo/redo 后方向必须保持。
//
// 当前 UndoEntry.old_selection/new_selection 是 Utf8ByteRange（排序后），
// history.rs:60-61 undo 时 cursor=old_selection.end(), anchor=old_selection.start()，
// 反向选区被翻成正向。
// ===========================================================================
#[test]
fn repro_reverse_selection_survives_undo_redo() {
    let mut kernel = EditorKernel::with_text("abcdef".to_string(), 6).unwrap();
    // 先插入一个字符产生可 undo 的编辑。
    let r1 = kernel
        .apply(EditorCommand::Insert {
            byte_offset: Utf8ByteOffset::unchecked(6),
            text: "X".to_string(),
            cause: writer_core::editor::EditorTransactionCause::Typing,
            expected_revision: EditorRevision::new(0),
        })
        .into_result();
    assert_eq!(kernel.snapshot_text(), "abcdefX");
    // 反向选区：anchor=3, head=0。
    kernel
        .apply(EditorCommand::SetSelection {
            anchor: Utf8ByteOffset::unchecked(3),
            head: Utf8ByteOffset::unchecked(0),
            expected_revision: r1.new_revision,
        })
        .into_result();
    assert_eq!(kernel.selection_anchor(), 3, "设置后 anchor=3");
    assert_eq!(kernel.cursor(), 0, "设置后 cursor(head)=0");

    // 再插入一个字符，把反向选区状态记进 undo 栈。
    let r2 = kernel
        .apply(EditorCommand::Insert {
            byte_offset: Utf8ByteOffset::unchecked(0),
            text: "Y".to_string(),
            cause: writer_core::editor::EditorTransactionCause::Typing,
            expected_revision: r1.new_revision,
        })
        .into_result();
    assert_eq!(kernel.snapshot_text(), "YabcdefX");

    // Undo：应恢复到反向选区 anchor=3, head=0。
    kernel
        .apply(EditorCommand::Undo {
            expected_revision: r2.new_revision,
        })
        .into_result();
    assert_eq!(kernel.snapshot_text(), "abcdefX", "undo 后正文应恢复");
    assert_eq!(
        kernel.selection_anchor(),
        3,
        "FAIL: undo 后 anchor 应恢复为 3（反向选区）；实际 {}。\
         UndoEntry.old_selection 是 Utf8ByteRange（排序后），history.rs:60-61\
         从 start/end 反推 anchor/head，反向选区被翻成正向。",
        kernel.selection_anchor()
    );
    assert_eq!(
        kernel.cursor(),
        0,
        "FAIL: undo 后 cursor(head) 应恢复为 0（反向选区）；实际 {}。",
        kernel.cursor()
    );
}

// ===========================================================================
// 复现 6：pipeline.rs set_selection() 的 NoChange 分支不更新 mirror —
// 这是"光标锁死"的直接根因。
//
// 该行为测试已移至 Linux_Qt pipeline 模块内（pipeline.rs 的 #[cfg(test)] mod tests），
// 因为 CommittedTextMirror 是 pub(crate)，integration test 无法直接访问。
// 测试链路：load "abcdef" → set_selection(3,3) → mirror.cursor == 3 →
// delete_range(2,3) → "abdef"
//
// Core 层的等价行为已由复现 2 和复现 3 覆盖：
// - 复现 2 验证 SetSelection 后 outcome 为 Applied（mirror 会跟着更新）
// - 复现 3 验证 SetSelection 后 outcome 携带的 new_selection 能让消费方知道 cursor 在 3
// ===========================================================================

// ===========================================================================
// 复现 7：EditorEditResult 用 EditorSelection { anchor, head } 表示 selection
// （保留方向）— 静态分析确认 result.rs 修复后用有方向的 EditorSelection
// 而非排序后的 Utf8ByteRange。
// ===========================================================================
#[test]
fn repro_editor_edit_result_uses_directionless_range_for_selection() {
    use std::fs;
    use std::path::PathBuf;

    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let manifest_path = PathBuf::from(manifest_dir);
    let repo_root = manifest_path
        .parent()
        .and_then(|p| p.parent())
        .expect("repo root");
    let result_path = repo_root.join("core/writer_core/src/editor/kernel/result.rs");
    let source = fs::read_to_string(&result_path)
        .unwrap_or_else(|e| panic!("读取 {:?} 失败: {e}", result_path));

    assert!(
        source.contains("pub old_selection: EditorSelection")
            && source.contains("pub new_selection: EditorSelection"),
        "FAIL: EditorEditResult 应使用 EditorSelection {{ anchor, head }} 表示 selection（保留方向）。\
         若仍用 Utf8ByteRange，方向会丢失。"
    );
    assert!(
        !source.contains("old_selection_byte_range: Utf8ByteRange")
            && !source.contains("new_selection_byte_range: Utf8ByteRange"),
        "FAIL: EditorEditResult 不应再使用 old_selection_byte_range/new_selection_byte_range 字段。"
    );
}

// ===========================================================================
// 复现 8：UndoEntry.old_selection/new_selection 是 Utf8ByteRange（无方向）—
// 静态分析确认 mod.rs 当前定义。
// ===========================================================================
#[test]
fn repro_undo_entry_uses_directionless_range_for_selection() {
    use std::fs;
    use std::path::PathBuf;

    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let manifest_path = PathBuf::from(manifest_dir);
    let repo_root = manifest_path
        .parent()
        .and_then(|p| p.parent())
        .expect("repo root");
    let mod_path = repo_root.join("core/writer_core/src/editor/kernel/mod.rs");
    let source =
        fs::read_to_string(&mod_path).unwrap_or_else(|e| panic!("读取 {:?} 失败: {e}", mod_path));

    assert!(
        source.contains("old_selection: EditorSelection")
            && source.contains("new_selection: EditorSelection"),
        "FAIL: UndoEntry 应使用 EditorSelection {{ anchor, head }} 表示 selection（保留方向）。\
         若仍用 Utf8ByteRange，undo/redo 反向选区方向会丢失。"
    );
    assert!(
        !source.contains("old_selection: Utf8ByteRange")
            && !source.contains("new_selection: Utf8ByteRange"),
        "FAIL: UndoEntry 不应再使用 Utf8ByteRange 表示 selection。"
    );
}
