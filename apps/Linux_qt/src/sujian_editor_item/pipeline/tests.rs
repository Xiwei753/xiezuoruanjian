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
    snapshot_for_test_with_image(line_id, top, cluster_count, None)
}

/// Issue #826 评论 16：可以带真实行图的 fixture。
fn snapshot_for_test_with_image(
    line_id: LineSnapshotId,
    top: f64,
    cluster_count: usize,
    image: Option<qmetaobject::QImage>,
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
    let mut line = PreparedLineSnapshot::stub_for_tests(
        line_id.visual_line_ordinal as usize,
        top,
        0,
        clusters,
    );
    if let Some(image) = image {
        line = line.with_test_image(image);
    }
    crate::sujian_editor_item::layout_snapshot::EditorLayoutSnapshot::new(
        LayoutSnapshot::empty_for_tests(),
        vec![line],
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
    //
    // 这里 `ab` 的 current line 必须带**真实行图**：否则 source_lines 里的
    // image 仍是 None，测试只能验证 snapshot_id 对不对，验证不了
    // `prepare_frontier_textures` 真的把图插进了 TextureCache。
    let mut pipeline = LinuxEditorPipeline::new();
    let image = qmetaobject::QImage::new(
        qmetaobject::QSize {
            width: 2,
            height: 1,
        },
        qmetaobject::ImageFormat::ARGB32_Premultiplied,
    );
    let mid = now + Duration::from_millis(80);
    coord.begin_or_extend_edit_frontier(EditFrontierRequest {
        kind: EditorAnimationKind::Delete,
        base_snapshot: snapshot_for_test_with_image(current_id, 0.0, 2, Some(image)),
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
    assert!(
        sources
            .iter()
            .any(|source| source.snapshot_id == current_id && source.image.is_some()),
        "current line 必须真的带上行图（source_lines.image 不能是 None）"
    );

    // 关键断言：prepare_frontier_textures 之后 TextureCache 真的能拿到 line 9。
    pipeline.animation_coordinator = coord;
    pipeline.prepare_frontier_textures();
    assert!(
        pipeline.texture_cache().contains_line(&current_id),
        "prepare_frontier_textures 必须把 current old layout 的行图插进 TextureCache，\
         否则 renderer 的 get_line 会 miss，b 的 Conceal overlay 一像素都画不出来"
    );
}

/// Issue #826 评论 21 BLOCKER 1：carry 的贴图必须进纹理生命周期与准备链。
///
/// carry glyph 用的是**最新 target 行**的纹理，而那几行此时只有动画层在引用
/// —— 静态正文层还没有栅格化到它们。如果 carry 的 snapshot_id 不进
/// `collect_active_snapshot_ids`，`retain_active_snapshot_ids` 之后纹理可能被回收，
/// `prepare_frontier_textures` 也不会补插。renderer 里
/// `texture_cache.get_line(&glyph.snapshot_id)` 返回 None 就 `continue` 跳过 glyph，
/// 同时 carry 的 canonical clip 因为 `ReflowTarget` 的纹理守卫也被过滤掉 ——
/// 结果是「carry 不画 + canonical 不挖」，评论 20 修的旧行半个 X 直接整块跳到新行。
#[test]
fn reveal_carry_target_texture_is_retained_and_prepared() {
    use crate::editor::layout::{CaretAffinity, LayoutSnapshot};
    use crate::sujian_editor_item::animation::coordinator::{
        EditFrontierRequest, LinuxEditorAnimationCoordinator,
    };
    use crate::sujian_editor_item::animation::edit_frontier::ConcealDirection;
    use crate::sujian_editor_item::edit_motion::EditorAnimationKind;
    use crate::sujian_editor_item::layout_snapshot::{
        EditorLayoutSnapshot, LineClusterSnapshot, PreparedLineSnapshot, ShapingIdentity,
        SourceRect,
    };
    use crate::sujian_editor_item::qt_text_node::StaticClipKind;
    use std::time::{Duration, Instant};

    // carry 那一行的 id —— 断言全部围绕它。
    let carry_line_id = LineSnapshotId::new(0, 0, 42);

    let cluster = |start: usize, end: usize, x: f64| LineClusterSnapshot {
        byte_start: start,
        byte_end: end,
        source_rect: SourceRect {
            x,
            y: 0.0,
            w: 10.0,
            h: 20.0,
        },
        shaping_identity: ShapingIdentity {
            text_content_hash: 1,
            raw_font_fingerprint: String::from("test-font"),
            glyph_indexes_hash: 1,
            cluster_glyph_count: 1,
            direction_rtl: false,
            format_fingerprint: 1,
        },
    };
    let two_lines = |top_line: PreparedLineSnapshot, second: PreparedLineSnapshot| {
        EditorLayoutSnapshot::new(
            LayoutSnapshot::empty_for_tests(),
            vec![top_line, second],
            None,
            None,
            CaretAffinity::Downstream,
        )
    };

    let now = Instant::now();
    let mut coord = LinuxEditorAnimationCoordinator::new();

    // ── 第一笔：输入 X，正文 `a` -> `aX`，X 的文档矩形是 (90, 0, 10, 20) ──
    // 同一行里放一个 x=0 的 cluster 把 `visual_x` 钉在 0（`stub_for_tests`
    // 用 cluster 最小 source x 当 visual_x），X 的文档 x 才是 90。
    let first_target = EditorLayoutSnapshot::new(
        LayoutSnapshot::empty_for_tests(),
        vec![PreparedLineSnapshot::stub_for_tests(
            1,
            0.0,
            0,
            vec![cluster(0, 1, 0.0), cluster(1, 2, 90.0)],
        )],
        None,
        None,
        CaretAffinity::Downstream,
    );
    coord.begin_or_extend_edit_frontier(EditFrontierRequest {
        kind: EditorAnimationKind::Insert,
        base_snapshot: snapshot_for_test(LineSnapshotId::new(0, 0, 0), 0.0, 1),
        target_snapshot: first_target.clone(),
        inserted_ranges: vec![(1, 2)],
        deleted_ranges: Vec::new(),
        offset_map: writer_core::editor::OffsetMap::from_single_edit(1, (1, 1), 1),
        base_text: String::from("a"),
        target_text: String::from("aX"),
        conceal_direction: ConcealDirection::Forward,
        now,
    });

    // ── 第二笔：输入 Y，Qt 重排把 X 挪到第二行 → X 进入 reveal_carried ──
    // carry 的 snapshot_id 取自**这次 target_snapshot**，所以真实 QImage 必须挂
    // 在 X 所在的那一行上，`prepare_frontier_textures` 才插得进来。
    let image = qmetaobject::QImage::new(
        qmetaobject::QSize {
            width: 2,
            height: 1,
        },
        qmetaobject::ImageFormat::ARGB32_Premultiplied,
    );
    let second_target = two_lines(
        PreparedLineSnapshot::stub_for_tests(0, 0.0, 0, vec![cluster(0, 1, 0.0)]),
        PreparedLineSnapshot::stub_for_tests(
            carry_line_id.visual_line_ordinal as usize,
            20.0,
            1,
            vec![cluster(1, 2, 0.0), cluster(2, 3, 10.0)],
        )
        .with_test_image(image),
    );
    let mid = now + Duration::from_millis(80);
    coord.begin_or_extend_edit_frontier(EditFrontierRequest {
        kind: EditorAnimationKind::Insert,
        base_snapshot: first_target,
        target_snapshot: second_target,
        inserted_ranges: vec![(2, 3)],
        deleted_ranges: Vec::new(),
        offset_map: writer_core::editor::OffsetMap::from_single_edit(2, (2, 2), 1),
        base_text: String::from("aX"),
        target_text: String::from("aXY"),
        conceal_direction: ConcealDirection::Forward,
        now: mid,
    });

    let carry_ids = coord.active_reveal_carried_snapshot_ids();
    assert!(
        carry_ids.contains(&carry_line_id),
        "X 此刻应处于 reveal carry（半吐后被 rewrap 挪走），实际 carry ids = {carry_ids:?}"
    );
    assert!(
        coord.collect_active_snapshot_ids().contains(&carry_line_id),
        "carry 的 snapshot_id 必须进 collect_active_snapshot_ids，否则 retain 会把它回收"
    );

    // ── 走真实管线：retain 之后再 prepare ──
    let mut pipeline = LinuxEditorPipeline::new();
    pipeline.animation_coordinator = coord;
    let active = pipeline.animation_coordinator.collect_active_snapshot_ids();
    pipeline
        .texture_cache_mut()
        .retain_active_snapshot_ids(&active);
    pipeline.prepare_frontier_textures();

    assert!(
        pipeline.texture_cache().contains_line(&carry_line_id),
        "prepare_frontier_textures 必须把 carry 目标行的行图插进 TextureCache，\
         否则 renderer 的 get_line miss，carry glyph 一像素都画不出来"
    );

    // carry 的 canonical clip 必须能活过 renderer 的纹理守卫。
    let sample = pipeline
        .animation_coordinator
        .sample_edit_frontier(mid)
        .expect("carry 在跑时前沿必须还是活跃的");
    let clips = pipeline
        .animation_coordinator
        .reveal_carried_target_clip_rects(&sample);
    assert!(
        !clips.is_empty(),
        "carry 的 canonical 目标位置必须产出 clip，否则静态层会同时画一份（重影）"
    );
    for clip in &clips {
        assert_eq!(
            clip.kind,
            StaticClipKind::ReflowTarget,
            "carry overlay 真的要用那张纹理，必须用 ReflowTarget：只有它带纹理守卫，\
             纹理 miss 时会恢复 canonical；FrontierMask 会留下永久空洞"
        );
        assert!(
            clip.requires_animation_texture(),
            "ReflowTarget 必须参与纹理可用性过滤"
        );
        assert!(
            pipeline.texture_cache().contains_line(&clip.snapshot_id),
            "纹理已就绪，renderer 就不会把这个 clip 过滤掉"
        );
    }
}

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
/// 所以 `active_old_overlay_snapshot_ids()` 返回的就是第二笔 base 的行身份。
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
        let base_lines = pipeline
            .animation_coordinator()
            .active_old_overlay_snapshot_ids();
        assert!(
            !base_lines.is_empty(),
            "第二笔 Delete 必须登记 base 行（Conceal overlay 的纹理来源）"
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

/// 评论 33 要求② 评论 31 的生产级回归：不使用 `stub_for_tests` 的 revision 0，
/// 显式让第一笔 old R0 / target R1、第二笔 base **必须 R1** / target R2。
///
/// 第一笔形成 `f -> fi` shaping，40ms 后第二笔删整个 `fi`，断言：
/// - `conceal_handoffs` 命中（Conceal 层仍然画着正在被吞的 fi）；
/// - Conceal glyph 的 snapshot / source_rect / dest_rect 来自上一帧 fi；
/// - opacity 保留上一帧值，不回 1.0（revision 链断掉时会退回 canonical 的 1.0）。
#[test]
fn pipeline_shaping_new_to_conceal_handoff_survives_real_layout_revision_chain() {
    crate::editor::layout::run_on_qt_thread(|| {
        let (mut pipeline, mut layout, ctx) = chain_pipeline_ready(true);
        let r0 = pipeline.layout_revision;

        // 第一笔 old R0 / target R1：`af` -> `afi`，形成 f -> fi 的 shaping new atom。
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
        assert_eq!(first_target.revision, r1, "第一笔 target 必须是 R1");
        assert_ne!(r0, r1, "第一笔 old 必须是 R0、target 必须是 R1");
        let first_target_line = first_line_of(&first_target);

        // 吞字进行到 40ms：此时上一帧 fi 的 shaping opacity 还在 (0,1)。
        std::thread::sleep(std::time::Duration::from_millis(40));
        let shaping_now = std::time::Instant::now();
        let shaping_frames = pipeline
            .animation_coordinator()
            .shaping_transition_glyphs(shaping_now);
        assert!(
            !shaping_frames.is_empty(),
            "第一笔之后 shaping transition 必须还在跑，否则拿不到上一帧 fi"
        );
        let previous_fi = shaping_frames
            .iter()
            .flat_map(|frame| frame.new.iter())
            .filter(|side| side.rect.w > 0.0)
            .max_by(|a, b| a.rect.x.total_cmp(&b.rect.x))
            .expect("第一笔后 shaping new 侧必须有 glyph");
        let previous_opacity = previous_fi.opacity;
        assert!(
            previous_opacity > 0.0 && previous_opacity < 1.0,
            "上一帧 fi 的 opacity 必须在 (0,1)，实际 {previous_opacity}"
        );
        let previous_rect = previous_fi.rect.clone();

        // 第二笔 base 必须是 R1 / target R2：删整个 `fi` (1,3)。
        assert_prepared_created(
            "第二笔",
            &run_real_stroke(&mut pipeline, &mut layout, &ctx, |pipeline| {
                pipeline.delete_range(1, 3, EditorTransactionCause::Delete)
            }),
        );
        let r2 = pipeline.layout_revision;
        assert_ne!(r1, r2, "第二笔 target 必须是 R2");

        let coordinator = pipeline.animation_coordinator();
        let sample = coordinator
            .sample_edit_frontier(std::time::Instant::now())
            .expect("第二笔之后遮罩前沿必须活跃");
        let glyphs = coordinator.old_overlay_glyphs_for(&sample);
        let handed_off = glyphs
            .iter()
            .find(|glyph| glyph.range == (1, 3))
            .expect("conceal_handoffs 必须命中：Conceal 层还画着正在被吞的 fi");
        assert_eq!(
            handed_off.snapshot_id.layout_revision, r1.0,
            "Conceal glyph 必须来自上一帧 R1 行，实际 {:?}",
            handed_off.snapshot_id
        );

        let previous_line = first_target
            .line_snapshots
            .iter()
            .find(|line| line.id == handed_off.snapshot_id)
            .expect("Conceal glyph 的行必须是上一帧（第一笔 target）的行");
        let previous_cluster = previous_line
            .cluster_exact_for_range((1, 3))
            .expect("上一帧 target 必须有恰好 (1,3) 的 fi cluster");
        assert!(
            (handed_off.source_rect.x - previous_cluster.source_rect.x).abs() < 1e-6
                && (handed_off.source_rect.w - previous_cluster.source_rect.w).abs() < 1e-6,
            "Conceal glyph source_rect 必须来自上一帧 fi：期望 {:?}，实际 {:?}",
            previous_cluster.source_rect,
            handed_off.source_rect
        );
        assert!(
            (handed_off.dest_rect.x - previous_rect.x).abs() < 1e-6
                && (handed_off.dest_rect.w - previous_rect.w).abs() < 1e-6,
            "Conceal glyph dest_rect 必须是上一帧 fi 的屏幕位置：期望 {:?}，实际 {:?}",
            previous_rect,
            handed_off.dest_rect
        );

        // opacity 保留上一帧值，不回 1.0。
        assert!(
            (handed_off.opacity - previous_opacity).abs() < 1e-3,
            "Conceal glyph opacity 必须保留上一帧 fi 的 {previous_opacity}，实际 {}",
            handed_off.opacity
        );
        assert!(
            handed_off.opacity < 1.0 - 1e-6,
            "revision 链断掉时这里会退回 canonical 的 1.0，实际 {}",
            handed_off.opacity
        );
        assert!(
            coordinator.active_conceal_glyphs_for_test() >= 1,
            "第二笔必须至少持有一个 Conceal glyph"
        );
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

/// 补充测试 —— 对应评论 33 明确要求的 3 条测试**覆盖不到**的位置：
/// 要求②只断言 Conceal **glyph** 的 snapshot / source / rect / opacity（命中路径），
/// 没断言行图来源登记 `active_conceal_source_lines()` —— renderer 靠它把 base 行图
/// 插进 TextureCache，行图来源身份错了会直接 miss 纹理。
///
/// 与要求②不重复：这里断言的是**行图来源**这一层，不是 glyph 层。
#[test]
fn conceal_line_image_sources_come_from_previous_target_revision_line() {
    crate::editor::layout::run_on_qt_thread(|| {
        let (mut pipeline, mut layout, ctx) = chain_pipeline_ready(true);

        assert_prepared_created(
            "第一笔",
            &run_real_stroke(&mut pipeline, &mut layout, &ctx, |pipeline| {
                pipeline.insert_text(2, "i", EditorTransactionCause::Typing)
            }),
        );
        let first_target = pipeline
            .current_layout_snapshot()
            .clone()
            .expect("第一笔 target snapshot 必须安装");
        let r1 = pipeline.layout_revision;
        let first_target_line = first_line_of(&first_target);

        std::thread::sleep(std::time::Duration::from_millis(40));
        assert_prepared_created(
            "第二笔",
            &run_real_stroke(&mut pipeline, &mut layout, &ctx, |pipeline| {
                pipeline.delete_range(1, 3, EditorTransactionCause::Delete)
            }),
        );

        let sources = pipeline
            .animation_coordinator()
            .active_conceal_source_lines();
        assert!(
            !sources.is_empty(),
            "第二笔 Conceal 必须登记行图来源（否则 renderer 的 TextureCache 会 miss）"
        );
        for source in &sources {
            assert_eq!(
                source.snapshot_id.layout_revision, r1.0,
                "Conceal 行图来源必须是上一帧 R1 行，实际 {:?}",
                source.snapshot_id
            );
            assert!(
                first_target
                    .line_snapshots
                    .iter()
                    .any(|line| line.id == source.snapshot_id),
                "Conceal 行图来源必须能在上一帧 target snapshot 里找到，实际 {:?}",
                source.snapshot_id
            );
            assert_eq!(
                source.snapshot_id.layout_revision, first_target_line.layout_revision,
                "行图来源必须与上一帧 target 的稳定视觉行同 revision"
            );
        }
    });
}
