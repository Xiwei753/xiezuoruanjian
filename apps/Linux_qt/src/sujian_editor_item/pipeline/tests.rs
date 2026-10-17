//! `pipeline` 模块的单元测试。
//!
//! 从 pipeline.rs 的内嵌 `#[cfg(test)] mod tests` 拆出（Issue #808 评论 5918236360 的
//! 失败行重取/注入诊断测试把内嵌测试模块推到 100 行以上，超过生产文件结构上限）。
//! 本模块在 `pipeline` 内部声明（`#[cfg(test)] mod tests;`），可直接访问其私有条目。
//!
//! Issue #919：原先在本模块按 `prepare_edit_motion` + 整章 `current_layout_snapshot`
//! 驱动的「连续编辑 revision 链」「文字动画关闭仍提交 layout_revision」两条用例
//! （#826 评论 33）已删除。原因：这两条用例的 `run_real_stroke` 现在只是直接调
//! `LinuxEditorPipeline::insert_text` / `delete_range` 并绑定 Core revision，不再
//! 经过 `prepare_edit_motion`；普通编辑路径已由 #919 收敛到唯一的
//! `TextMotion` 局部计划（`transaction.rs::record_transaction` / `editing.rs`），
//! pipeline 内部既不安装整章动画 basis，也不再于该 helper 里推进 `layout_revision`。
//! 两条用例因此长期 `#[ignore]`、放开即失败。它们验证的语义（连续编辑的 canonical
//! revision 链、关闭正文动画仍提交 canonical）已用当前生产入口改写在
//! `super::runtime_tests` 中，避免继续保留会失败的旧脚手架。

use super::*;

/// Issue #683 复现 6（行为测试版）：验证 set_selection 后 mirror.cursor 正确更新，
/// 后续 delete_range 能删除旧正文中的字符，而非因光标锁死删除错误位置。
///
/// 链路：load "abcdef" → set_selection(3,3) → mirror.cursor == 3 →
/// delete_range(2,3) → "abdef"
#[test]
fn set_selection_updates_mirror_cursor_then_delete_removes_correct_char() {
    let mut pipeline = LinuxEditorPipeline::new();
    assert!(pipeline.load_text("abcdef".to_string(), 6));

    // 光标初始在末尾 (6)。
    assert_eq!(pipeline.mirror().cursor(), 6);

    // 把光标移到 3（'c' 后面）。
    pipeline.set_selection(3, 3);

    // mirror.cursor 必须跟着更新到 3，否则光标锁死。
    assert_eq!(
        pipeline.mirror().cursor(),
        3,
        "set_selection(3,3) 后 mirror.cursor 应为 3，实际 {} — 光标锁死",
        pipeline.mirror().cursor()
    );

    // 删除 [2,3) 即 'c'，应得到 "abdef"。
    pipeline.delete_range(2, 3, EditorTransactionCause::Delete);
    assert_eq!(
        pipeline.mirror().text(),
        "abdef",
        "删除 'c' 后应为 'abdef'，实际 {:?}",
        pipeline.mirror().text()
    );
}
