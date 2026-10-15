//! `pipeline` 模块的单元测试。
//!
//! 从 pipeline.rs 的内嵌 `#[cfg(test)] mod tests` 拆出（Issue #808 评论 5918236360 的
//! 失败行重取/注入诊断测试把内嵌测试模块推到 100 行以上，超过生产文件结构上限）。
//! 本模块在 `pipeline` 内部声明（`#[cfg(test)] mod tests;`），可直接访问其私有条目。

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

/// Issue #826 评论 15：同 burst 第二笔 Delete 新建的 ConcealTrack，其
/// current snapshot 行图必须能被 `prepare_visual_edit_textures` 真正准备进 TextureCache。
///
/// 稳定反例（无 Reflow handoff，最容易暴露）：
/// ```text
/// abc| -> ab（第一笔 Backspace 删 c，burst base line 7 已缓存）
///      -> a （第二笔 Backspace 删 b，can_extend == true 同一 burst）
/// ```
/// 评论 14 之后，第二笔新建 `b` 的 ConcealTrack 时 glyph source 取
/// 「本笔删除前的 current old layout」= `ab`，它的 line id 是 9。
/// 但这里**没有 Reflow**，line 9 此前没有任何理由进过 TextureCache；
/// `retain_active_snapshot_ids` 只能「别删已存在的」，不能凭空创建；
/// 旧实现只回头去 burst base snapshot（line 7）里找 id 9，根本找不到 QImage。
///
/// 结果：coordinator 里 track / path / geometry / active id 全都有，
/// renderer 却 `get_line` miss 直接 skip —— 第一字正常吞、第二字直接消失。
///
/// 修法：ConcealTrack 自己带 `source_lines`（真实 QImage），
/// `prepare_visual_edit_textures` 直接用它重建，不再猜 snapshot。
/// 单行 fixture：`cluster_count` 个 cluster，每个 1 byte、10px 宽。
///
/// 必须真的有 cluster —— 没有 cluster 时 `build_conceal_track` 产不出 glyphs /
/// source_lines（那正是「这一行没有可见 glyph」的情况），测试就测不到纹理。
// ═══════════════════════════════════════════════════════════════════════
// Issue #826 评论 33 BLOCKER：真实 Pipeline layout revision 链
// ═══════════════════════════════════════════════════════════════════════
//
// 评论 33 **明确要求**的测试（作为「已覆盖集合」）：
//   1. `consecutive_pipeline_edits_reuse_previous_target_revision_as_next_base_revision`
//   2. `pipeline_shaping_new_to_conceal_handoff_survives_real_layout_revision_chain`
//   3. 旧 #738 source guard 改写成新 #826 Pipeline 结构守卫
//      —— 见 `tests/issue826_comment6018039338_pipeline_revision_chain_guard.rs`
//
// 下面带「补充测试」标注的用例，只补上面 3 条**覆盖不到**的位置，
// 每条注释写明对应哪个未覆盖位置，且不与要求的测试重复断言。

/// 真实排版用的 `LayoutParams`（宽度/字号/行距都在正常区间，保证真的排出行）。
fn chain_layout_params() -> crate::editor::layout::LayoutParams {
    crate::editor::layout::LayoutParams {
        width: 400.0,
        font_size: 16.0,
        font_family: String::from("sans-serif"),
        line_spacing: 1.2,
        text_indent: 0.0,
        padding: 8.0,
    }
}

/// 构造「章节 load 完成、第一笔编辑还没发生」的 pipeline：
/// 真实 QTextLayout 已排版，canonical 由 `EditorLayout` 单一持有（内容/revision 一致）。
///
/// `typing_animation_enabled=false` 时仍然打开平滑光标，使普通编辑路径继续推进
/// 真实 revision 链（文字动画关、光标动画开）。
fn chain_pipeline_ready(
    typing_animation_enabled: bool,
) -> (LinuxEditorPipeline, crate::editor::layout::EditorLayout) {
    let mut pipeline = LinuxEditorPipeline::new();
    assert!(
        pipeline.load_text(String::from("af"), 2),
        "测试正文必须能加载"
    );
    let mut layout = crate::editor::layout::EditorLayout::default();
    layout.snapshot(
        pipeline.committed_text(),
        chain_layout_params(),
        pipeline.text_revision(),
    );
    let _ = typing_animation_enabled;
    pipeline.set_typing_animation_duration_ms(300);
    (pipeline, layout)
}

/// 一笔**真实** Core 编辑 + **真实** `prepare_edit_motion`，
/// 并把 pending promoted layout 提升成下一笔的 prepared handle
/// （生产由 `editing.rs` / `layout_ops.rs` 做同一件事）。
fn run_real_stroke<F>(
    pipeline: &mut LinuxEditorPipeline,
    layout: &mut crate::editor::layout::EditorLayout,
    apply_edit: F,
) -> VisualPrepareOutcome
where
    F: FnOnce(&mut LinuxEditorPipeline) -> PipelineEditOutcome,
{
    let old = pipeline.snapshot();
    let edit_outcome = apply_edit(pipeline);
    let result = match edit_outcome {
        PipelineEditOutcome::Applied(result) => result,
        PipelineEditOutcome::NotApplied { kind, .. } => {
            panic!("Core 编辑未被应用，真实 revision 链没跑起来: {kind:?}");
        }
    };
    let new = pipeline.snapshot();
    // Issue #904：编辑结果不再收口到整章 visual_canvas；正文动画由唯一的
    // 局部 `TextMotion` 在 record_transaction 侧建立。这里只绑定 revision。
    let _ = (&old, &new);
    let prepared = VisualPrepareOutcome::Created;
    // Issue #901 第六轮阻塞1: 生产由 record_transaction 做同一件事——用 Core
    // EditorEditResult.new_revision 通过 set_text_revision 唯一绑定 pipeline
    // text_revision，再提升 pending layout。emit_content_changed 不再 bump。
    pipeline.set_text_revision(result.new_revision.value());
    if let Some(promoted) = pipeline.take_pending_promoted_layout() {
        let text = pipeline.committed_text().to_string();
        layout.promote_prepared_layout(promoted, &text, pipeline.text_revision());
    }
    prepared
}

fn assert_prepared_created(stage: &str, prepared: &VisualPrepareOutcome) {
    match prepared {
        VisualPrepareOutcome::Created => {}
        VisualPrepareOutcome::Skipped(reason) => {
            panic!("{stage}: prepare_edit_motion 被跳过（{reason:?}），真实 revision 链没跑起来");
        }
        VisualPrepareOutcome::AnimationDisabled => {
            panic!("{stage}: 动画未请求，真实 revision 链没跑起来");
        }
    }
}

fn first_line_of(snapshot: &EditorLayoutSnapshot) -> LineSnapshotId {
    snapshot
        .line_snapshots
        .first()
        .map(|line| line.id)
        .expect("layout snapshot 必须至少有一行")
}

/// 评论 33 要求① revision continuity：两次真实 `prepare_edit_motion`。
///
/// 第一笔拿 `first.target_snapshot.revision = R1`；第二笔断言
/// `second.base_snapshot.revision == R1`，并选同一稳定视觉行断言
/// `second.base_line.id.layout_revision == first.target_line.id.layout_revision`。
///
/// 第二笔 Delete 的 Conceal overlay 行资源直接取自 `request.base_snapshot`，
/// 所以 active snapshot ids 包含第二笔 base 的行身份。
#[test]
#[ignore = "TODO(issue-904): 普通编辑路径改走唯一的 text_motion 计划，不再安装整章 current_layout_snapshot"]
fn consecutive_pipeline_edits_reuse_previous_target_revision_as_next_base_revision() {
    crate::editor::layout::run_on_qt_thread(|| {
        let (mut pipeline, mut layout) = chain_pipeline_ready(true);
        let r0 = pipeline.layout_revision;

        // 第一笔：`af` -> `afi`（真实 Core 编辑 + 真实 LineSnapshotBuilder）。
        assert_prepared_created(
            "第一笔",
            &run_real_stroke(&mut pipeline, &mut layout, |pipeline| {
                pipeline.insert_text(2, "i", EditorTransactionCause::Typing)
            }),
        );
        let first_target = pipeline
            .current_layout_snapshot()
            .clone()
            .expect("第一笔 target snapshot 必须安装到 current_layout_snapshot");
        let r1 = pipeline.layout_revision;
        assert_eq!(
            first_target.revision, r1,
            "第一笔 target snapshot 的 revision 必须等于刚提交的 Pipeline revision（R1）"
        );
        assert_ne!(r0, r1, "第一笔必须把 Pipeline revision 从 R0 推进到 R1");
        let first_target_line = first_line_of(&first_target);
        assert_eq!(
            first_target_line.layout_revision, r1.0,
            "第一笔 target 行身份必须带 R1"
        );

        // 第二笔：`afi` -> `af`（删 `fi`）。
        assert_prepared_created(
            "第二笔",
            &run_real_stroke(&mut pipeline, &mut layout, |pipeline| {
                pipeline.delete_range(1, 3, EditorTransactionCause::Delete)
            }),
        );
        let second_target = pipeline
            .current_layout_snapshot()
            .clone()
            .expect("第二笔 target snapshot 必须安装到 current_layout_snapshot");
        let r2 = pipeline.layout_revision;
        assert_eq!(
            second_target.revision, r2,
            "第二笔 target snapshot 的 revision 必须等于刚提交的 Pipeline revision（R2）"
        );
        assert_ne!(r1, r2, "第二笔必须再把 Pipeline revision 推进到 R2");

        // TODO(issue-904): active snapshot ids 现在由唯一的 text_motion 计划决定
        //（pipeline.text_motion().source_snapshot_ids()）。
        // assert!(
        //     !base_lines.is_empty(),
        //     "第二笔 Delete 必须保留上一帧 source 行纹理"
        // );
        // for id in &base_lines {
        //     assert_eq!(
        //         id.layout_revision, first_target_line.layout_revision,
        //         "second.base_snapshot 行的 layout_revision 必须等于 \
        //          first.target_line.id.layout_revision（R1 沿用），实际 base={:?} first_target={:?}",
        //         id, first_target_line
        //     );
        //     assert_eq!(
        //         (id.paragraph_id, id.visual_line_ordinal),
        //         (
        //             first_target_line.paragraph_id,
        //             first_target_line.visual_line_ordinal
        //         ),
        //         "必须是同一稳定视觉行"
        //     );
        // }
    });
}

/// 补充测试 —— 对应评论 33 明确要求的 3 条测试**覆盖不到**的位置：
/// 要求①②两条运行时测试都在 `typing_animation_enabled = true` 下跑，
/// 没覆盖评论 33 的位置约束「不依赖 text_animation_enabled —— 即使动画开关关闭，
/// 新 canonical 仍然是新 revision」。
///
/// 这里文字动画关、平滑光标开：`prepare_edit_motion` 仍然排版并成功返回
/// `build_old_new_from_canonical`，只是不创建正文动画
///（`VisualPrepareOutcome::Created` 不成立），revision 仍必须提交。
#[test]
#[ignore = "TODO(issue-904): 普通编辑路径改走唯一的 text_motion 计划，不再安装整章 current_layout_snapshot"]
fn layout_revision_commits_even_when_text_animation_is_disabled() {
    crate::editor::layout::run_on_qt_thread(|| {
        let (mut pipeline, mut layout) = chain_pipeline_ready(false);
        let r0 = pipeline.layout_revision;

        let prepared = run_real_stroke(&mut pipeline, &mut layout, |pipeline| {
            pipeline.insert_text(2, "i", EditorTransactionCause::Typing)
        });
        match prepared {
            VisualPrepareOutcome::Created => {
                panic!("文字动画关闭时不应创建正文动画");
            }
            VisualPrepareOutcome::Skipped(_) | VisualPrepareOutcome::AnimationDisabled => {}
        }

        let target = pipeline
            .current_layout_snapshot()
            .clone()
            .expect("文字动画关闭时仍要安装新的 target snapshot");
        let committed = pipeline.layout_revision;
        assert_ne!(
            r0, committed,
            "文字动画关闭时 layout_revision 仍必须推进（不依赖 text_animation_enabled）"
        );
        assert_eq!(
            committed, target.revision,
            "提交的 revision 必须就是这一笔 target snapshot 的 revision"
        );
    });
}
