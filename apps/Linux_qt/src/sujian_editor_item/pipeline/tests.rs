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

// ── Issue #808 评论 5918236360 问题1: 失败 InsertReveal 行诊断 + 重取验证 ──

fn test_cluster(
    byte_start: usize,
    byte_end: usize,
    text: &str,
) -> crate::editor::layout::CanonicalClusterSnapshot {
    crate::editor::layout::CanonicalClusterSnapshot {
        document_byte_start: byte_start,
        document_byte_end: byte_end,
        source_rect_x: byte_start as f64 * 10.0,
        source_rect_y: 0.0,
        source_rect_w: (byte_end - byte_start) as f64 * 10.0,
        source_rect_h: 20.0,
        glyph_count: 1,
        raw_font_fingerprint: "font".to_string(),
        is_rtl: false,
        first_glyph_index: 0,
        cluster_text: text.to_string(),
    }
}

/// 构造 canonical 行（可含非空 clusters）。
fn test_canonical_line(
    para_start: usize,
    qtextline_idx: i32,
    byte_start: usize,
    byte_end: usize,
    clusters: Vec<crate::editor::layout::CanonicalClusterSnapshot>,
) -> crate::editor::layout::CanonicalLineSnapshot {
    crate::editor::layout::CanonicalLineSnapshot {
        qchar_start: 0,
        qchar_end: 0,
        document_byte_start: byte_start,
        document_byte_end: byte_end,
        x_pos: 0.0,
        width: (byte_end - byte_start) as f64 * 10.0,
        ascent: 16.0,
        descent: 4.0,
        x_end_trailing: 0.0,
        image: None,
        clusters,
        cursor_x_map: Vec::new(),
        paragraph_document_byte_start: para_start,
        qtextline_idx,
    }
}

/// 构造含单段落、单视觉行的 canonical document snapshot。
fn test_doc_snapshot(
    text: &str,
    para_start: usize,
    lines: Vec<crate::editor::layout::CanonicalLineSnapshot>,
) -> crate::editor::layout::CanonicalDocumentVisualSnapshot {
    crate::editor::layout::CanonicalDocumentVisualSnapshot {
        text_revision: 0,
        font_size: 16.0,
        font_family: "sans-serif".to_string(),
        line_spacing: 1.5,
        text_indent: 0.0,
        padding: 0.0,
        width: 800.0,
        dpr: 1.0,
        paragraphs: vec![crate::editor::layout::CanonicalParagraphSnapshot {
            paragraph_text: text.to_string(),
            paragraph_document_byte_start: para_start,
            lines,
            index_map: crate::editor::paragraph_index_map::ParagraphIndexMap::build(
                text, para_start,
            ),
        }],
        visual_lines: Vec::new(),
    }
}

fn test_visual_line(
    para_start: usize,
    qtextline_idx: i32,
    byte_start: usize,
    byte_end: usize,
    para_text: &str,
) -> crate::editor::layout::VisualLine {
    crate::editor::layout::VisualLine {
        id: 0,
        byte_start,
        byte_end,
        qchar_start: 0,
        qchar_end: 0,
        hard_break: false,
        x: 0.0,
        y: 0.0,
        width: 800.0,
        height: 20.0,
        para_text: para_text.to_string(),
        para_start,
        qtextline_idx,
        para_qchar_start: 0,
        para_qchar_end: 0,
        line_wrap_width: 800.0,
        line_indent_x: 0.0,
        para_indent: 0.0,
        x_end_trailing: 800.0,
        qt_ascent: 16.0,
        qt_descent: 4.0,
        cache_slot: 0,
    }
}

/// Issue #810 评论 5932233052 问题1: 构造 AnimationRasterVisual（raster-only，
/// 不携带 cluster）。
fn test_raster_visual(
    para_start: usize,
    qtextline_idx: i32,
) -> crate::editor::layout::AnimationRasterVisual {
    crate::editor::layout::AnimationRasterVisual {
        image: None,
        paragraph_document_byte_start: para_start,
        qtextline_idx,
    }
}

/// Issue #808 评论 5918236360 问题1: prepare 成功且 clusters 非空，但注入未命中目标行时，
/// 该行必须带准确 `visual_line_idx / para_start / qtextline_idx` 进入 `failed_lines`
/// （reason=InjectMiss），让上游能对它做第二次精确重取。
///
/// Issue #810 评论 5932233052 问题1: prepare_animation_visuals_from_layout 改为
/// raster-only 后，clusters 检查改为直接读回 new_doc_snapshot 的 clusters
///（基础排版产出）。本测试验证：canonical 行 clusters 为空时，该行进入 failed_lines
///（reason=ClustersEmpty），让上游知道这是 invariant failure。
#[test]
fn issue808_comment5918236360_inject_miss_line_is_collected_for_retry() {
    // 目标 snapshot 里只有一个段落 (para_start=999)，且其第 7 行没有 clusters；
    // inserted visible line 的稳定身份也是 (para_start=999, qtextline_idx=7)，
    // prepare 命中但 canonical_line_has_clusters 返回 false → ClustersEmpty。
    let mut doc = test_doc_snapshot(
        "ab",
        999,
        vec![test_canonical_line(999, 7, 0, 2, Vec::new())],
    );
    doc.visual_lines = vec![test_visual_line(999, 7, 0, 2, "ab")];
    let prepared = vec![test_raster_visual(999, 7)];

    let status = inject_new_animation_visuals_with_diagnostics(
        &mut doc,
        prepared,
        Some(Utf8ByteRange::from_ordered(0, 2)),
    );

    assert_eq!(
        status.checked_line_count, 1,
        "应检查 1 个 inserted visible line"
    );
    assert_eq!(status.prepare_miss_count, 0, "prepare 命中（按稳定行身份）");
    assert_eq!(
        status.clusters_empty_count, 1,
        "canonical 行 clusters 为空 → ClustersEmpty"
    );
    assert_eq!(status.ok_count, 0);
    assert!(
        status.has_unavailable_lines,
        "clusters empty 行必须让 has_unavailable_lines=true，否则上游不会重取"
    );
    assert_eq!(
        status.failed_lines.len(),
        1,
        "clusters empty 行必须进入 failed_lines"
    );
    let fl = &status.failed_lines[0];
    assert_eq!(fl.visual_line_idx, 0);
    assert_eq!(fl.para_start, 999);
    assert_eq!(fl.qtextline_idx, 7);
    assert_eq!(fl.reason, AnimationVisualsFailedReason::ClustersEmpty);
    assert!(
        !canonical_line_has_clusters(&doc, 999, 7),
        "canonical 行仍然没有 clusters"
    );
}

/// Issue #808 评论 5918236360 问题1: prepare 未返回该行 snapshot 时，
/// failed_lines 必须带准确 `visual_line_idx / para_start / qtextline_idx`（reason=PrepareMiss）。
#[test]
fn issue808_comment5918236360_prepare_miss_line_is_collected_for_retry() {
    let mut doc = test_doc_snapshot("ab", 0, vec![test_canonical_line(0, 0, 0, 2, Vec::new())]);
    doc.visual_lines = vec![test_visual_line(0, 0, 0, 2, "ab")];

    // prepare 结果为空（该行没有被提取到动画视觉）。
    let status = inject_new_animation_visuals_with_diagnostics(
        &mut doc,
        Vec::new(),
        Some(Utf8ByteRange::from_ordered(0, 2)),
    );

    assert_eq!(status.prepare_miss_count, 1);
    assert!(status.has_unavailable_lines);
    assert_eq!(status.failed_lines.len(), 1);
    let fl = &status.failed_lines[0];
    assert_eq!(fl.visual_line_idx, 0);
    assert_eq!(fl.para_start, 0);
    assert_eq!(fl.qtextline_idx, 0);
    assert_eq!(fl.byte_start, 0);
    assert_eq!(fl.byte_end, 2);
    assert_eq!(fl.reason, AnimationVisualsFailedReason::PrepareMiss);
}

/// Issue #808 评论 5918236360 问题1: 注入命中且 clusters 非空时没有任何失败行，
/// 且重取后验证用的 `canonical_line_has_clusters` 必须能读回真实 clusters。
///
/// Issue #810 评论 5932233052 问题1: clusters 由基础 canonical 排版直接产出，
/// prepared 改为 AnimationRasterVisual（不携带 cluster）。canonical 行的 clusters
/// 在 doc_snapshot 中自带，`canonical_line_has_clusters` 直接读回。
#[test]
fn issue808_comment5918236360_successful_inject_has_no_failed_lines() {
    let mut doc = test_doc_snapshot(
        "ab",
        0,
        vec![test_canonical_line(
            0,
            0,
            0,
            2,
            vec![test_cluster(0, 1, "a"), test_cluster(1, 2, "b")],
        )],
    );
    doc.visual_lines = vec![test_visual_line(0, 0, 0, 2, "ab")];
    let prepared = vec![test_raster_visual(0, 0)];

    let status = inject_new_animation_visuals_with_diagnostics(
        &mut doc,
        prepared,
        Some(Utf8ByteRange::from_ordered(0, 2)),
    );

    assert_eq!(status.ok_count, 1);
    assert_eq!(status.actual_inject_count, 1);
    assert!(
        !status.has_unavailable_lines,
        "全部行注入成功时不应有 unavailable lines"
    );
    assert!(status.failed_lines.is_empty());
    assert!(
        canonical_line_has_clusters(&doc, 0, 0),
        "注入成功后 canonical 行必须带真实 clusters"
    );
}

/// Issue #808 评论 5918236360 问题1: 空白字符的 inserted line 不参与检查，
/// 不产生 failed_lines（空格/tab/换行本来就没有 InsertReveal）。
#[test]
fn issue808_comment5918236360_whitespace_line_is_not_checked() {
    let mut doc = test_doc_snapshot(" ", 0, vec![test_canonical_line(0, 0, 0, 1, Vec::new())]);
    doc.visual_lines = vec![test_visual_line(0, 0, 0, 1, " ")];

    let status = inject_new_animation_visuals_with_diagnostics(
        &mut doc,
        Vec::new(),
        Some(Utf8ByteRange::from_ordered(0, 1)),
    );

    assert_eq!(status.checked_line_count, 0, "纯空白行不检查");
    assert!(status.failed_lines.is_empty());
    assert!(!status.has_unavailable_lines);
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

fn chain_ctx(
    typing_animation_enabled: bool,
    smooth_cursor_enabled: bool,
) -> VisualTransactionContext {
    VisualTransactionContext {
        typing_animation_enabled,
        smooth_cursor_enabled,
        coordinated_animation_enabled: false,
        is_scrolling: false,
        is_loading: false,
        is_applying_format: false,
        bounding_width: 400.0,
        font_pixel_size: 16.0,
        font_family: String::from("sans-serif"),
        scroll_y: 0.0,
        viewport_height: 600.0,
        text_indent: 0.0,
        line_spacing: 1.2,
        padding: 8.0,
        text_color: String::from("#000000"),
        dpr: 1.0,
    }
}

/// 构造「章节 load 完成、第一笔编辑还没发生」的 pipeline：
/// 真实 QTextLayout 已排版 + `current_canonical_snapshot` 已就位（内容/revision 一致）。
///
/// `typing_animation_enabled=false` 时仍然打开平滑光标，使 `prepare_edit_motion`
/// 继续走到 `LineSnapshotBuilder::build_old_new_from_canonical`（文字动画关、光标动画开）。
fn chain_pipeline_ready(
    typing_animation_enabled: bool,
) -> (
    LinuxEditorPipeline,
    crate::editor::layout::EditorLayout,
    VisualTransactionContext,
) {
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
    let ctx = chain_ctx(typing_animation_enabled, true);
    let canonical = pipeline.build_canonical_snapshot_for_current_layout(&ctx, &layout);
    pipeline.set_current_canonical_snapshot(Some(canonical));
    pipeline.set_typing_animation_duration_ms(300);
    (pipeline, layout, ctx)
}

/// 一笔**真实** Core 编辑 + **真实** `prepare_edit_motion`，
/// 并把 pending promoted layout 提升成下一笔的 prepared handle
/// （生产由 `editing.rs` / `layout_ops.rs` 做同一件事）。
fn run_real_stroke<F>(
    pipeline: &mut LinuxEditorPipeline,
    layout: &mut crate::editor::layout::EditorLayout,
    ctx: &VisualTransactionContext,
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
    let prepared = pipeline.prepare_edit_motion(ctx, &result, &old, &new, layout);
    // 生产由 `emit_content_changed` 做同一件事：先 bump text_revision
    //（使 new canonical 的 text_revision == pipeline.text_revision()），再提升 pending layout。
    pipeline.bump_text_revision();
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
fn consecutive_pipeline_edits_reuse_previous_target_revision_as_next_base_revision() {
    crate::editor::layout::run_on_qt_thread(|| {
        let (mut pipeline, mut layout, ctx) = chain_pipeline_ready(true);
        let r0 = pipeline.layout_revision;

        // 第一笔：`af` -> `afi`（真实 Core 编辑 + 真实 LineSnapshotBuilder）。
        assert_prepared_created(
            "第一笔",
            &run_real_stroke(&mut pipeline, &mut layout, &ctx, |pipeline| {
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
            &run_real_stroke(&mut pipeline, &mut layout, &ctx, |pipeline| {
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

        // second.base_snapshot 的行身份 = 第二笔 Delete 的 base 行。
        let base_lines: Vec<_> = pipeline
            .animation_coordinator()
            .active_snapshot_ids()
            .into_iter()
            .filter(|id| id.layout_revision == first_target_line.layout_revision)
            .collect();
        assert!(
            !base_lines.is_empty(),
            "第二笔 Delete 必须保留上一帧 source 行纹理"
        );
        for id in &base_lines {
            assert_eq!(
                id.layout_revision, first_target_line.layout_revision,
                "second.base_snapshot 行的 layout_revision 必须等于 \
                 first.target_line.id.layout_revision（R1 沿用），实际 base={:?} first_target={:?}",
                id, first_target_line
            );
            assert_eq!(
                (id.paragraph_id, id.visual_line_ordinal),
                (
                    first_target_line.paragraph_id,
                    first_target_line.visual_line_ordinal
                ),
                "必须是同一稳定视觉行"
            );
        }
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
fn layout_revision_commits_even_when_text_animation_is_disabled() {
    crate::editor::layout::run_on_qt_thread(|| {
        let (mut pipeline, mut layout, ctx) = chain_pipeline_ready(false);
        let r0 = pipeline.layout_revision;

        let prepared = run_real_stroke(&mut pipeline, &mut layout, &ctx, |pipeline| {
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
