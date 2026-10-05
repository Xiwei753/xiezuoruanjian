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
/// current snapshot 行图必须能被 `prepare_frontier_textures` 真正准备进 TextureCache。
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
/// `prepare_frontier_textures` 直接用它重建，不再猜 snapshot。
/// 单行 fixture：`cluster_count` 个 cluster，每个 1 byte、10px 宽。
///
/// 必须真的有 cluster —— 没有 cluster 时 `build_conceal_track` 产不出 glyphs /
/// source_lines（那正是「这一行没有可见 glyph」的情况），测试就测不到纹理。
fn snapshot_for_test(
    line_id: LineSnapshotId,
    top: f64,
    cluster_count: usize,
) -> crate::sujian_editor_item::layout_snapshot::EditorLayoutSnapshot {
    use crate::editor::layout::{CaretAffinity, LayoutSnapshot};
    use crate::sujian_editor_item::layout_snapshot::{
        LineClusterSnapshot, PreparedLineSnapshot, ShapingIdentity, SourceRect,
    };
    let shaping = ShapingIdentity {
        text_content_hash: 1,
        raw_font_fingerprint: String::from("test-font"),
        glyph_indexes_hash: 1,
        cluster_glyph_count: 1,
        direction_rtl: false,
        format_fingerprint: 1,
    };
    let clusters: Vec<LineClusterSnapshot> = (0..cluster_count)
        .map(|i| LineClusterSnapshot {
            byte_start: i,
            byte_end: i + 1,
            source_rect: SourceRect {
                x: (i as f64) * 20.0,
                y: 0.0,
                w: 10.0,
                h: 20.0,
            },
            shaping_identity: shaping.clone(),
        })
        .collect();
    crate::sujian_editor_item::layout_snapshot::EditorLayoutSnapshot::new(
        LayoutSnapshot::empty_for_tests(),
        vec![PreparedLineSnapshot::stub_for_tests(
            line_id.visual_line_ordinal as usize,
            top,
            0,
            clusters,
        )],
        None,
        None,
        CaretAffinity::Downstream,
    )
}

#[test]
fn same_burst_second_delete_can_prepare_current_snapshot_overlay_texture() {
    use crate::sujian_editor_item::animation::coordinator::{
        EditFrontierRequest, LinuxEditorAnimationCoordinator,
    };
    use crate::sujian_editor_item::animation::edit_frontier::ConcealDirection;
    use crate::sujian_editor_item::edit_motion::EditorAnimationKind;
    use crate::sujian_editor_item::layout_snapshot::LineSnapshotId;
    use std::time::{Duration, Instant};

    let now = Instant::now();
    let base_id = LineSnapshotId::new(0, 0, 7);
    let current_id = LineSnapshotId::new(0, 0, 9);

    let mut coord = LinuxEditorAnimationCoordinator::new();

    // ── 第一笔 Backspace：`abc` -> `ab`，删 c ──
    // burst base 是 `abc`（line 7）；先只把 line 7 当作已缓存。
    coord.begin_or_extend_edit_frontier(EditFrontierRequest {
        kind: EditorAnimationKind::Delete,
        base_snapshot: snapshot_for_test(base_id, 0.0, 3),
        target_snapshot: snapshot_for_test(current_id, 0.0, 2),
        deleted_ranges: vec![(2, 3)],
        inserted_ranges: Vec::new(),
        offset_map: writer_core::editor::OffsetMap::from_single_edit(3, (2, 3), 0),
        base_text: String::from("abc"),
        target_text: String::from("ab"),
        conceal_direction: ConcealDirection::Backward,
        now,
    });
    let active = coord.active_old_overlay_snapshot_ids();
    assert!(
        active.contains(&base_id),
        "第一笔的 overlay 纹理应来自 burst base line 7"
    );

    // ── 第二笔 Backspace（同一 burst，can_extend == true）：`ab` -> `a`，删 b ──
    let mid = now + Duration::from_millis(80);
    coord.begin_or_extend_edit_frontier(EditFrontierRequest {
        kind: EditorAnimationKind::Delete,
        base_snapshot: snapshot_for_test(current_id, 0.0, 2),
        target_snapshot: snapshot_for_test(LineSnapshotId::new(0, 0, 11), 0.0, 1),
        deleted_ranges: vec![(1, 2)],
        inserted_ranges: Vec::new(),
        offset_map: writer_core::editor::OffsetMap::from_single_edit(2, (1, 2), 0),
        base_text: String::from("ab"),
        target_text: String::from("a"),
        conceal_direction: ConcealDirection::Backward,
        now: mid,
    });

    // 第二笔的 overlay 资源必须来自 current snapshot（line 9），不是 burst base。
    let sources = coord.active_conceal_source_lines();
    assert!(
        sources
            .iter()
            .any(|source| source.snapshot_id == current_id),
        "第二笔的 overlay 纹理必须来自 current old layout（line 9），实际 {:?}",
        sources
            .iter()
            .map(|source| source.snapshot_id)
            .collect::<Vec<_>>()
    );
}
