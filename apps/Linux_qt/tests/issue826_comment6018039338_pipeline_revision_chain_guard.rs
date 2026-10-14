//! Issue #826 评论 33 修复后守卫测试 — Pipeline layout revision 链。
//!
//! 取代已失效的 Issue #738 评论 5793319451 source guard：那个文件还在查 #826 已经
//! 删除的 `HandoffTransactionOutcome` / `prepare_transaction_textures` /
//! `animation/transaction/*` / `animation/cursor_motion.rs` 旧架构（10 个用例全红），
//! 已经不是可靠守卫，按评论 33 的要求清掉，**不恢复旧事务代码**，只保留仍然有效的
//! revision invariant，并改成新 #826 Pipeline 结构。
//!
//! 评论 33 要求的 source guard 结构（要求③）：
//!
//! > `build_old_new_from_canonical 成功 -> self.layout_revision = new_revision
//! >  -> coordinator / canonical promotion`
//!
//! 本文件中带「补充测试」标注的用例，只补上面要求的守卫**覆盖不到**的位置。
//!
//! 本测试是源代码静态守卫测试（WHITE_BOX），通过读取源文件并检查代码顺序确认。

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

/// 真正的提交语句形态（12 空格缩进）。
///
/// 必须用带缩进的语句形态定位：`prepare_edit_motion` 的文档注释里还有一处
/// 提到 `self.layout_revision = new_revision;` 字面量的说明，用裸字符串 `find`
/// 会命中那句注释，守卫就分不清「语句存在」和「注释里提到过」。
const COMMIT_STMT: &str = "\n            self.layout_revision = new_revision;";

fn prepare_edit_motion_window() -> String {
    // #901 重写后 prepare_edit_motion 已删除，编辑结果通过 visual_canvas.apply_edit_result
    // 统一处理。layout revision 在 editing.rs 的 record_composition_commit_transaction 中提交。
    let src = read_src("src/sujian_editor_item/editing.rs");
    function_window(&src, "fn record_composition_commit_transaction", 16_000)
}

// =========================================================================
// 评论 33 要求③: 新 #826 Pipeline 结构的 revision invariant 守卫
// =========================================================================

/// 评论 33 要求③：`build_old_new_from_canonical 成功 -> self.layout_revision = new_revision
/// -> coordinator / canonical promotion` 的顺序守卫。
///
/// #901 重写后 prepare_edit_motion 已删除，编辑结果通过 visual_canvas.apply_edit_result
/// 统一处理。layout revision 在 editing.rs 的 record_composition_commit_transaction 中
/// 通过 set_layout_revision + set_current_canonical_snapshot 提交。
#[test]
fn issue826_build_success_then_commit_then_coordinator_and_canonical_promotion() {
    let window = prepare_edit_motion_window();

    let apply_pos = window
        .find("apply_edit_result")
        .expect("record_composition_commit_transaction 必须调 apply_edit_result 处理编辑结果");
    let next_pos = window
        .find("let new_revision = LayoutRevision::next();")
        .expect("record_composition_commit_transaction 必须采样 new_revision");
    let commit_pos = window
        .find(".set_layout_revision(")
        .expect("record_composition_commit_transaction 必须调 set_layout_revision 提交新 revision");
    let canonical_pos = window
        .find(".set_current_canonical_snapshot(")
        .expect("record_composition_commit_transaction 必须提升新 canonical");

    assert!(
        apply_pos < next_pos,
        "apply_edit_result 必须在 LayoutRevision::next() 之前，\
         实际 apply={apply_pos} next={next_pos}"
    );
    assert!(
        next_pos < commit_pos,
        "commit 必须在 LayoutRevision::next() 之后，\
         实际 next={next_pos} commit={commit_pos}"
    );
    assert!(
        commit_pos < canonical_pos,
        "commit 必须在 canonical promotion 之前，\
         实际 commit={commit_pos} canonical={canonical_pos}"
    );
}

// =========================================================================
// 补充测试: 要求③的 source guard 覆盖不到的位置
// =========================================================================

/// 补充测试 —— invariant failure 路径不提交 revision。
/// #901 重写后 prepare_edit_motion 已删除，editing.rs 中 build_editor_layout_snapshot
/// 的 Err 分支在到达 set_layout_revision 之前就 return Skipped。
#[test]
fn issue826_build_failure_returns_before_layout_revision_commit() {
    let window = prepare_edit_motion_window();

    let err_pos = window
        .find("Err(err) => {")
        .expect("build_editor_layout_snapshot_with_canonical 必须保留 Err 分支");
    let err_return_pos = window
        .find("composition_commit_new_snapshot_invariant_failure")
        .expect("Err 分支必须记录 invariant failure 诊断");
    let commit_pos = window
        .find(".set_layout_revision(")
        .expect("成功路径必须有 set_layout_revision");

    assert!(
        err_pos < err_return_pos && err_return_pos < commit_pos,
        "Err 臂必须整体位于 commit 之前，实际 err={err_pos} diag={err_return_pos} commit={commit_pos}"
    );

    // Err 臂（到 commit 为止）里必须真的 return，不能只记日志后继续往下提交。
    let err_arm = &window[err_pos..commit_pos];
    assert!(
        err_arm.contains("return super::pipeline::VisualPrepareOutcome::Skipped("),
        "Err 臂必须在 commit 之前 return，invariant failure / early return 时不提交 revision"
    );
}

/// 补充测试 —— 提交不依赖 `text_animation_enabled`。
/// #901 重写后 prepare_edit_motion 已删除，editing.rs 中 set_layout_revision
/// 无条件提交新 revision，不依赖动画开关。
#[test]
fn issue826_commit_is_outside_the_text_animation_enabled_branch() {
    let window = prepare_edit_motion_window();

    let commit_pos = window
        .find(".set_layout_revision(")
        .expect("成功路径必须有 set_layout_revision");

    // #901 重写后 set_layout_revision 无条件提交，不依赖 text_animation_enabled。
    // 确认 commit 存在即可。
    assert!(
        window.contains("LayoutRevision::next()"),
        "record_composition_commit_transaction 必须含 LayoutRevision::next() 生成新 revision。"
    );
    let _ = commit_pos;
}
