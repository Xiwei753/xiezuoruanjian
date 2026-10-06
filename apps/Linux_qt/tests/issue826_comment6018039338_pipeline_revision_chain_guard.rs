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
    let src = read_src("src/sujian_editor_item/pipeline.rs");
    // 评论 33 要求的守卫要一直看到 canonical promotion（函数尾部），窗口必须覆盖整段。
    function_window(&src, "fn prepare_edit_motion", 60_000)
}

// =========================================================================
// 评论 33 要求③: 新 #826 Pipeline 结构的 revision invariant 守卫
// =========================================================================

/// 评论 33 要求③：`build_old_new_from_canonical 成功 -> self.layout_revision = new_revision
/// -> coordinator / canonical promotion` 的顺序守卫。
///
/// 同时守住评论 33 给出的位置约束：只提交一次、不再调 `LayoutRevision::next()` 造第三个
/// revision、`old_revision` 必须采样自 `self.layout_revision`、提交不依赖
/// `text_animation_enabled`。
#[test]
fn issue826_build_success_then_commit_then_coordinator_and_canonical_promotion() {
    let window = prepare_edit_motion_window();

    let old_rev_pos = window
        .find("let old_revision = self.layout_revision;")
        .expect("prepare_edit_motion 必须先读当前 self.layout_revision 当 old_revision");
    let next_pos = window
        .find("let new_revision = LayoutRevision::next();")
        .expect("prepare_edit_motion 必须只在这里采样一次 new_revision");
    let build_pos = window
        .find("let (old_snap, new_snap) = match LineSnapshotBuilder::build_old_new_from_canonical(")
        .expect("prepare_edit_motion 必须走真实 LineSnapshotBuilder");
    let commit_pos = window
        .find(COMMIT_STMT)
        .expect("build_old_new_from_canonical 成功后必须提交 self.layout_revision = new_revision");
    let coordinator_pos = window
        .find(".begin_or_extend_edit_frontier(EditFrontierRequest")
        .expect("prepare_edit_motion 必须把真实 old/new snapshot 交给 coordinator");
    let canonical_pos = window
        .find("self.current_canonical_snapshot = Some(new_doc_snapshot);")
        .expect("prepare_edit_motion 成功后必须提升新 canonical");

    assert!(
        old_rev_pos < next_pos && next_pos < build_pos,
        "顺序必须是 读 old_revision -> 采样 new_revision -> build_old_new_from_canonical，\
         实际 old_rev={old_rev_pos} next={next_pos} build={build_pos}"
    );
    assert!(
        build_pos < commit_pos,
        "commit 必须在 build_old_new_from_canonical 成功之后，\
         实际 build={build_pos} commit={commit_pos}"
    );
    assert!(
        commit_pos < coordinator_pos,
        "commit 必须在 coordinator 之前，实际 commit={commit_pos} coordinator={coordinator_pos}"
    );
    assert!(
        commit_pos < canonical_pos,
        "commit 必须在 canonical promotion 之前，\
         实际 commit={commit_pos} canonical={canonical_pos}"
    );

    // 只提交一次：没有第二处、也没有藏在别的分支里的提交。
    assert_eq!(
        window.matches(COMMIT_STMT).count(),
        1,
        "prepare_edit_motion 里只能有唯一一处 self.layout_revision = new_revision;"
    );
    // 不再造第三个 revision。
    // 同样用缩进语句形态，排除注释里提到该调用的字面量。
    let next_stmt = "\n            let new_revision = LayoutRevision::next();";
    assert_eq!(
        window.matches(next_stmt).count(),
        1,
        "prepare_edit_motion 里只能采样一次 LayoutRevision::next()，不得再造第三个 revision"
    );
    assert_eq!(
        window
            .matches("let old_revision = self.layout_revision;")
            .count(),
        1,
        "old_revision 只能采样一次"
    );
}

// =========================================================================
// 补充测试: 要求③的 source guard 覆盖不到的位置
// =========================================================================

/// 补充测试 —— 对应评论 33 要求的守卫**覆盖不到**的位置：
/// 要求③只查「成功链」的顺序，没有查
/// 「invariant failure / early return 时不提交」这条位置约束。
///
/// 这里断言 `build_old_new_from_canonical` 的 `Err` 臂在到达 commit 之前就
/// `return VisualPrepareOutcome::Skipped(...)`（并释放 new_generation），
/// 所以失败路径永远走不到 `self.layout_revision = new_revision;`。
#[test]
fn issue826_build_failure_returns_before_layout_revision_commit() {
    let window = prepare_edit_motion_window();

    let err_pos = window
        .find("Err(err) => {")
        .expect("build_old_new_from_canonical 必须保留 Err 分支");
    let err_return_pos = window
        .find("build_old_new_from_canonical_invariant_failure")
        .expect("Err 分支必须记录 invariant failure 诊断");
    let commit_pos = window
        .find(COMMIT_STMT)
        .expect("成功路径必须有 self.layout_revision = new_revision;");

    assert!(
        err_pos < err_return_pos && err_return_pos < commit_pos,
        "Err 臂必须整体位于 commit 之前，实际 err={err_pos} diag={err_return_pos} commit={commit_pos}"
    );

    // Err 臂（到 commit 为止）里必须真的 return，不能只记日志后继续往下提交。
    let err_arm = &window[err_pos..commit_pos];
    assert!(
        err_arm.contains("return VisualPrepareOutcome::Skipped("),
        "Err 臂必须在 commit 之前 return，invariant failure / early return 时不提交 revision"
    );
    assert!(
        err_arm.contains("layout::clear_layout_generation(new_generation);"),
        "Err 臂必须释放 new_generation（成功路径由 pending_promoted_layout 接管）"
    );
    assert!(
        !err_arm.contains("self.layout_revision = new_revision;"),
        "commit 不得出现在 Err 臂内"
    );
}

/// 补充测试 —— 对应评论 33 要求的守卫**覆盖不到**的位置：
/// 要求③没有查「提交不依赖 `text_animation_enabled`」这条位置约束
///（评论 33：动画开关关闭时新 canonical 仍然是新 revision）。
///
/// 断言 commit 落在 `if text_animation_enabled {` 的**条件块之外**（之前）。
#[test]
fn issue826_commit_is_outside_the_text_animation_enabled_branch() {
    let window = prepare_edit_motion_window();

    let commit_pos = window
        .find(COMMIT_STMT)
        .expect("成功路径必须有 self.layout_revision = new_revision;");
    let branch_pos = window
        .find("if text_animation_enabled {")
        .expect("prepare_edit_motion 必须有 if text_animation_enabled { 分支");

    assert_eq!(
        window.matches("if text_animation_enabled {").count(),
        1,
        "只应有一处 if text_animation_enabled {{ 分支"
    );
    assert!(
        commit_pos < branch_pos,
        "commit 必须在 if text_animation_enabled {{ 之外（之前），否则动画关闭时不提交，\
         实际 commit={commit_pos} branch={branch_pos}"
    );
}
