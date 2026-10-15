//! 从 CanonicalDocumentVisualSnapshot 构建 EditorLayoutSnapshot。
//!
//! 输入是 CanonicalDocumentVisualSnapshot（由 layout 模块完成 QTextLayout 排版后产出）。
//! PreparedLineSnapshot 的 image、cluster、byte range、visual line 和 shaping identity
//! 均来自同一批 canonical 排版结果，动画阶段只消费快照，不再次排版。
//!
//! source_rect 坐标单位为像素，已乘 DPR；消费端直接用于 QImage 裁剪。
//!
//! canonical 排版使用 QChar/UTF-16 offset（QTextLayout 原生坐标），
//! 此处通过 CanonicalClusterSnapshot 的 document_byte_start/document_byte_end
//! 将 UTF-16 范围转换为 UTF-8 document byte range，供跨平台事务使用。
//!
//! 职责边界：
//! - build_from_canonical_document()：从整个文档快照构建 EditorLayoutSnapshot，
//!   包含视口裁剪、段落 ID 分配、baseline 计算和空行处理。
//! - build_clusters_from_canonical()：从单条 canonical line 的 cluster 列表
//!   构建 LineClusterSnapshot，负责 source_rect 裁剪和 shaping identity 哈希。

use super::layout_revision::LayoutRevision;
use super::layout_snapshot::{
    EditorLayoutSnapshot, LineClusterSnapshot, LineSnapshotId, ParagraphAnchor,
    PreparedLineSnapshot, ShapingIdentity, SourceRect,
};
use crate::editor::layout::ParagraphHeightIndex;
use crate::editor::layout::{
    cursor_rect_for_line, CanonicalDocumentVisualSnapshot, CanonicalLineSnapshot, VisualLine,
};
use crate::editor::paragraph_index_map::ParagraphIndexMap;

/// Issue #904 复核 9：真实段首文档 y 与段落高度。
///
/// 两种构建路线都按视口裁行，不能用"快照第一条行的 y"反推段首；这里统一从该版
/// canonical 的权威 `height_index` 取（`logical_slot` 是物化后按稳定 ParagraphId
/// 映射出的当前逻辑序号，不是物理 cache_slot）。
fn paragraph_top_and_height(
    logical_slot: i32,
    doc_snapshot: &CanonicalDocumentVisualSnapshot,
) -> (f64, f64) {
    let slot = logical_slot.max(0) as usize;
    (
        f64::from(doc_snapshot.padding) + doc_snapshot.height_index.paragraph_top_y(slot),
        doc_snapshot.height_index.height_at(slot),
    )
}

/// Issue #904 复核 10：本次编辑受影响的**段落 slot** 集合（按真实 byte 范围判定）。
///
/// 编辑发生在可见光标处，但一次粘贴/换行/删除多行会把受影响段落的行推到视口之外。
/// 这些行的 cluster 几何与该版本图片必须一并保留：否则屏幕外进入/离开的字在
/// `text_motion` 里拿不到完整 old/new 几何，会被误判成 Reveal/Conceal 或整段缺席。
fn affected_paragraph_slots(
    doc_snapshot: &CanonicalDocumentVisualSnapshot,
    affected_range: Option<(usize, usize)>,
) -> Vec<usize> {
    let Some((start, end)) = affected_range else {
        return Vec::new();
    };
    let index = doc_snapshot.paragraph_offset_index.as_ref();
    let paragraph_count = index.len();
    if paragraph_count == 0 {
        return Vec::new();
    }
    // 零长度的插入（光标处）用 start 自身定位段落。
    let last_byte = end.saturating_sub(1).max(start);
    let first = index.byte_to_paragraph(start).min(paragraph_count - 1);
    let last = index.byte_to_paragraph(last_byte).min(paragraph_count - 1);
    (first..=last).collect()
}

/// Issue #904 复核 10：该版本**全部段落**的几何锚点（不含任何 QImage）。
///
/// `visual_lines` 按段落顺序排列、同段连续，因此只与上一个锚点比较即可去重，O(行数)。
fn paragraph_anchors(doc_snapshot: &CanonicalDocumentVisualSnapshot) -> Vec<ParagraphAnchor> {
    let mut anchors: Vec<ParagraphAnchor> = Vec::new();
    for line in doc_snapshot.resolved_visual_lines().iter() {
        let paragraph_id = line.paragraph_id.as_u64();
        if anchors.last().map(|a| a.paragraph_id) == Some(paragraph_id) {
            continue;
        }
        let (top_y, height) = paragraph_top_and_height(line.logical_slot, doc_snapshot);
        anchors.push(ParagraphAnchor {
            paragraph_id,
            top_y,
            height,
        });
    }
    anchors
}

pub(crate) struct LineSnapshotBuilder;

impl LineSnapshotBuilder {
    /// Issue #810 评论 5932233052 问题2: 返回 `Result<EditorLayoutSnapshot, String>`。
    ///
    /// 可见正文出现空 clusters 时返回 `Err`，包含诊断信息（revision、para_start、
    /// qtextline_idx 等），不再产出带空 cluster 的成功对象。调用方据此 fallback
    /// 到空 snapshot 或跳过本次动画事务，不 panic、不伪造 cluster。
    pub fn build_from_canonical_document(
        revision: LayoutRevision,
        doc_snapshot: &CanonicalDocumentVisualSnapshot,
        scroll_y: f64,
        viewport_h: f64,
        affected_range: Option<(usize, usize)>,
    ) -> Result<EditorLayoutSnapshot, String> {
        let mut line_snapshots = Vec::new();
        let mut prev_cache_slot: Option<i32> = None;
        let mut visual_line_ordinal: u32 = 0;

        // Issue #904 复核 10：本次编辑受影响段落 —— 它们的行即使被推到视口外也必须
        // 保留（屏幕外进入/离开的字要按真实路径生成 Move）。
        let affected_slots = affected_paragraph_slots(doc_snapshot, affected_range);

        // Issue #901: 只物化视口内行（加边距）∪ 本次编辑受影响段落的行，
        // 避免对长篇文档物化整章所有视觉行。返回的行已是物化后的绝对坐标。
        // 当 height_index 为空时退化为物化所有行（与原 resolved_visual_lines 一致）。
        let resolved_lines: Vec<VisualLine> = if doc_snapshot.height_index.is_empty() {
            doc_snapshot.resolved_visual_lines().into_owned()
        } else {
            let margin = viewport_h.max(100.0);
            let (vp_first, vp_last_excl) = doc_snapshot
                .height_index
                .paragraphs_in_y_range(scroll_y - margin, scroll_y + viewport_h + margin);
            let mut first = vp_first;
            let mut last_excl = vp_last_excl;
            for &slot in &affected_slots {
                first = first.min(slot);
                last_excl = last_excl.max(slot + 1);
            }
            let first = first.min(doc_snapshot.paragraphs.len());
            let last_excl = last_excl.min(doc_snapshot.paragraphs.len());
            if first < last_excl {
                let (start_idx, end_idx) =
                    doc_snapshot.paragraph_visual_line_range(first, last_excl);
                doc_snapshot.resolve_visual_line_range(start_idx, end_idx)
            } else {
                Vec::new()
            }
        };
        for line in &resolved_lines {
            // Issue #901: 用 logical_slot 作为 paragraph_id（段落索引），
            // 遇到新 logical_slot 时重置 visual_line_ordinal 为 0。
            // Issue #901 第二十轮阻塞3: 用 logical_slot（文档次序）而非 cache_slot（Qt 物理槽）。
            let paragraph_id = line.logical_slot as u64;
            if prev_cache_slot != Some(line.logical_slot) {
                visual_line_ordinal = 0;
                prev_cache_slot = Some(line.logical_slot);
            }

            if line.para_text.is_empty() {
                let line_top = line.y - scroll_y;
                let line_bottom = line_top + line.height;
                // Issue #904 复核 10：受影响段落的行即使被推出视口也要保留几何。
                if (line_bottom < -line.height || line_top > viewport_h + line.height)
                    && !affected_slots.contains(&(line.logical_slot as usize))
                {
                    visual_line_ordinal += 1;
                    continue;
                }

                let id = LineSnapshotId::new(revision.0, paragraph_id, visual_line_ordinal);

                // Issue #826 评论 42：真实 drawn caret top/height（不贴行顶）。
                let (caret_top, caret_height) =
                    cursor_rect_for_line(&line, doc_snapshot.font_size, &doc_snapshot.font_family);

                let (para_top, para_h) = paragraph_top_and_height(line.logical_slot, doc_snapshot);

                line_snapshots.push(PreparedLineSnapshot {
                    id,

                    // Issue #904 复核 8: 段落位移身份 = 稳定 ParagraphId，与静态 QSG 同源。
                    stable_paragraph_id: line.paragraph_id.as_u64(),
                    paragraph_top_document_y: para_top,
                    paragraph_height: para_h,
                    image: None,
                    clusters: Vec::new(),
                    document_origin_y: line.y,
                    dpr: doc_snapshot.dpr,
                    byte_start: line.byte_start,
                    byte_end: line.byte_end,
                    visual_x: line.x,
                    visual_line_top: line.y,
                    visual_line_bottom: line.y + line.height,
                    caret_top,
                    caret_height,
                });

                visual_line_ordinal += 1;
                continue;
            }

            let line_top = line.y - scroll_y;
            let line_bottom = line_top + line.height;
            // Issue #904 复核 10：受影响段落的行即使被推出视口也要保留几何。
            if (line_bottom < -line.height || line_top > viewport_h + line.height)
                && !affected_slots.contains(&(line.logical_slot as usize))
            {
                visual_line_ordinal += 1;
                continue;
            }

            // Issue #901 第二十轮阻塞3: 用 logical_slot 直接索引 paragraphs，
            // 不再用 cache_slot（Qt 物理槽，与 paragraphs 数组索引不对应）。
            // cluster 的 byte_start/end 也需加上段首绝对 byte 偏移。
            let canonical_para = {
                let slot = line.logical_slot as usize;
                doc_snapshot.paragraphs.get(slot)
            };
            let para_byte_start = canonical_para
                .map(|_p| {
                    let slot = line.logical_slot as usize;
                    doc_snapshot
                        .paragraph_offset_index
                        .paragraph_byte_start(slot)
                })
                .unwrap_or(0);

            let (image, clusters) = if let Some(canonical) = canonical_para {
                let canonical_line = canonical.lines.get(line.qtextline_idx as usize);
                let img = canonical_line.and_then(|cl| cl.image.clone());
                let cls = canonical_line
                    .map(|cl| {
                        Self::build_clusters_from_canonical(
                            cl,
                            &line,
                            &canonical.index_map,
                            para_byte_start,
                        )
                    })
                    .unwrap_or_default();
                (img, cls)
            } else {
                (None, Vec::new())
            };

            // Issue #810 评论 5932233052 问题2: cluster 数据流重构——canonical 排版现在
            // 直接产出 cluster 几何（engine.rs 中 cluster 提取始终执行），"有可见正文但
            // clusters 为空"不再是合法 canonical 状态。此处返回 Err，不再产出带空 cluster
            // 的半残 snapshot 往下传。由于 canonical 排版保证产出 cluster，此分支理论上
            // 不应触发；若触发说明 engine.rs 排版路径有 bug 需要排查。
            if clusters.is_empty()
                && line
                    .para_text
                    .chars()
                    .any(|c| !c.is_whitespace() && !c.is_control())
            {
                crate::backend::app_backend::debug_error_static(
                    "line_snapshot_builder",
                    "canonical_line_clusters_empty_invariant_failure",
                    &format!(
                        "revision={} para_start={} qtextline_idx={} byte_start={} byte_end={} \
                         image={} — clusters empty and line contains visible chars, \
                         this is an invariant failure: canonical layout should always produce \
                         clusters (Issue #810). Returning Err to surface the failure upstream, \
                         not producing a half-built snapshot.",
                        revision.0,
                        line.para_start,
                        line.qtextline_idx,
                        line.byte_start,
                        line.byte_end,
                        image.is_some(),
                    ),
                );
                return Err(format!(
                    "canonical_line_clusters_empty_invariant_failure: \
                     revision={} para_start={} qtextline_idx={} byte_start={} byte_end={}",
                    revision.0, line.para_start, line.qtextline_idx, line.byte_start, line.byte_end,
                ));
            }

            let id = LineSnapshotId::new(revision.0, paragraph_id, visual_line_ordinal);

            // Issue #826 评论 42：真实 drawn caret top/height（不贴行顶）。
            let (caret_top, caret_height) =
                cursor_rect_for_line(line, doc_snapshot.font_size, &doc_snapshot.font_family);

            let (para_top, para_h) = paragraph_top_and_height(line.logical_slot, doc_snapshot);

            line_snapshots.push(PreparedLineSnapshot {
                id,

                // Issue #904 复核 8: 段落位移身份 = 稳定 ParagraphId，与静态 QSG 同源。
                stable_paragraph_id: line.paragraph_id.as_u64(),
                paragraph_top_document_y: para_top,
                paragraph_height: para_h,
                image,
                clusters,
                document_origin_y: line.y,
                dpr: doc_snapshot.dpr,
                byte_start: line.byte_start,
                byte_end: line.byte_end,
                visual_x: line.x,
                visual_line_top: line.y,
                visual_line_bottom: line.y + line.height,
                caret_top,
                caret_height,
            });

            visual_line_ordinal += 1;
        }

        Ok(EditorLayoutSnapshot {
            revision,
            line_snapshots,
            // Issue #904 复核 10：整章段落锚点（与视口裁剪无关），供跨视口位移使用。
            paragraph_anchors: paragraph_anchors(doc_snapshot),
            caret_rect: None,
            caret_rect_doc: None,
            caret_affinity: crate::editor::layout::CaretAffinity::Downstream,
        })
    }

    /// Issue #901 第五轮阻塞5: 通过段落高度索引直接枚举 viewport+边距内行。
    ///
    /// 与 `build_from_canonical_document` 的区别：用 `ParagraphHeightIndex`
    /// O(log n) 定位视口内的段落范围，再用二分查找在 visual_lines 中定位
    /// 这些段落的行范围，只遍历视口内的行构建 cluster。
    /// 不在视口内的行只做轻量 paragraph_id/visual_line_ordinal 推算（用高度索引
    /// 直接计算，不遍历前面的行）。
    ///
    /// # 参数
    ///
    /// - `height_index`：段落高度索引，用于 O(log n) 定位视口内段落。
    ///   必须与 `doc_snapshot` 的段落数一致。
    pub fn build_from_canonical_document_with_height_index(
        revision: LayoutRevision,
        doc_snapshot: &CanonicalDocumentVisualSnapshot,
        scroll_y: f64,
        viewport_h: f64,
        height_index: &ParagraphHeightIndex,
        affected_range: Option<(usize, usize)>,
    ) -> Result<EditorLayoutSnapshot, String> {
        let mut line_snapshots = Vec::new();

        if doc_snapshot.visual_lines.is_empty() {
            return Ok(EditorLayoutSnapshot {
                revision,
                line_snapshots,
                paragraph_anchors: Vec::new(),
                caret_rect: None,
                caret_rect_doc: None,
                caret_affinity: crate::editor::layout::CaretAffinity::Downstream,
            });
        }

        // Issue #904 复核 10: 用高度索引 O(log n) 定位视口内段落范围。
        // 视口在文档坐标系中的 y 范围：[scroll_y, scroll_y + viewport_h]。
        // 加边距确保边缘行不被遗漏。
        let affected_slots = affected_paragraph_slots(doc_snapshot, affected_range);
        let max_line_h = doc_snapshot.font_size * doc_snapshot.line_spacing * 2.0 + 8.0;
        let viewport_top = scroll_y - max_line_h;
        let viewport_bottom = scroll_y + viewport_h + max_line_h;

        // 用高度索引找到视口顶部和底部对应的段落索引
        let first_para_idx = height_index.y_to_paragraph(viewport_top.max(0.0));
        let last_para_idx = height_index.y_to_paragraph(viewport_bottom);

        // 在 visual_lines 中定位视口段落范围的行索引。
        // visual_lines 按段落顺序排列，同一段落的所有行连续。
        // Issue #901 第二十轮阻塞1: 用 paragraph_id_to_line_range 索引查行范围，
        // 替换 raw partition_point(logical_slot)。
        // 插段后复用后缀行的 raw logical_slot 没有更新，partition_point 二分仍会找错；
        // paragraph_id_to_line_range 按 visual_lines 实际行顺序构建，通过 ParagraphId
        // → (start, end) 直接定位行范围，索引为空时退化为 partition_point(logical_slot)。
        let (start_idx, end_idx) = {
            let para_len = doc_snapshot.paragraphs.len();
            let mut first = first_para_idx.min(para_len);
            let mut last_excl = if first_para_idx < para_len && last_para_idx >= first_para_idx {
                (last_para_idx + 1).min(para_len)
            } else {
                first
            };
            // Issue #904 复核 10：受影响段落的行一并枚举（可能整段被推到视口之外）。
            for &slot in &affected_slots {
                first = first.min(slot);
                last_excl = last_excl.max(slot + 1);
            }
            if first < last_excl {
                doc_snapshot.paragraph_visual_line_range(first, last_excl)
            } else {
                (
                    doc_snapshot.visual_lines.len(),
                    doc_snapshot.visual_lines.len(),
                )
            }
        };

        // Issue #901 第五轮阻塞5: paragraph_id 直接用段落索引（从高度索引推算），
        // 不遍历前面的行。visual_line_ordinal 在段落内从 0 递增。
        let mut prev_para_start: Option<usize> = None;
        let mut paragraph_id: u64 = first_para_idx as u64;
        let mut visual_line_ordinal: u32 = 0;

        // Issue #901 第十四轮问题1: 用 resolve_visual_line 物化绝对坐标。
        // partition_point 仍在 raw visual_lines 上做（para_start 为段落索引，
        // 与 paragraph_document_byte_start 一致），但行的 y/byte 坐标需物化为绝对。
        for idx in start_idx..end_idx {
            let line = doc_snapshot.resolve_visual_line(idx);
            if line.para_text.is_empty() {
                if prev_para_start != Some(line.para_start) && prev_para_start.is_some() {
                    paragraph_id = paragraph_id.wrapping_add(1);
                }
                visual_line_ordinal = 0;
                prev_para_start = Some(line.para_start);

                let line_top = line.y - scroll_y;
                let line_bottom = line_top + line.height;
                // Issue #904 复核 10：受影响段落的行即使被推出视口也要保留几何。
                if (line_bottom < -line.height || line_top > viewport_h + line.height)
                    && !affected_slots.contains(&(line.logical_slot as usize))
                {
                    visual_line_ordinal += 1;
                    continue;
                }

                let id = LineSnapshotId::new(revision.0, paragraph_id, visual_line_ordinal);
                let (caret_top, caret_height) =
                    cursor_rect_for_line(&line, doc_snapshot.font_size, &doc_snapshot.font_family);

                let (para_top, para_h) = paragraph_top_and_height(line.logical_slot, doc_snapshot);

                line_snapshots.push(PreparedLineSnapshot {
                    id,

                    // Issue #904 复核 8: 段落位移身份 = 稳定 ParagraphId，与静态 QSG 同源。
                    stable_paragraph_id: line.paragraph_id.as_u64(),
                    paragraph_top_document_y: para_top,
                    paragraph_height: para_h,
                    image: None,
                    clusters: Vec::new(),
                    document_origin_y: line.y,
                    dpr: doc_snapshot.dpr,
                    byte_start: line.byte_start,
                    byte_end: line.byte_end,
                    visual_x: line.x,
                    visual_line_top: line.y,
                    visual_line_bottom: line.y + line.height,
                    caret_top,
                    caret_height,
                });

                visual_line_ordinal += 1;
                continue;
            }

            if prev_para_start != Some(line.para_start) {
                if prev_para_start.is_some() {
                    paragraph_id = paragraph_id.wrapping_add(1);
                }
                visual_line_ordinal = 0;
                prev_para_start = Some(line.para_start);
            }

            let line_top = line.y - scroll_y;
            let line_bottom = line_top + line.height;
            // Issue #904 复核 10：受影响段落的行即使被推出视口也要保留几何。
            if (line_bottom < -line.height || line_top > viewport_h + line.height)
                && !affected_slots.contains(&(line.logical_slot as usize))
            {
                visual_line_ordinal += 1;
                continue;
            }

            // Issue #901 第二十轮阻塞3: 用 logical_slot 直接索引 paragraphs，
            // 不再用 cache_slot（Qt 物理槽，与 paragraphs 数组索引不对应）。
            // cluster 的 byte_start/end 也需加上段首绝对 byte 偏移。
            let canonical_para = {
                let slot = line.logical_slot as usize;
                doc_snapshot.paragraphs.get(slot)
            };
            let para_byte_start = canonical_para
                .map(|_p| {
                    let slot = line.logical_slot as usize;
                    doc_snapshot
                        .paragraph_offset_index
                        .paragraph_byte_start(slot)
                })
                .unwrap_or(0);

            let (image, clusters) = if let Some(canonical) = canonical_para {
                let canonical_line = canonical.lines.get(line.qtextline_idx as usize);
                let img = canonical_line.and_then(|cl| cl.image.clone());
                let cls = canonical_line
                    .map(|cl| {
                        Self::build_clusters_from_canonical(
                            cl,
                            &line,
                            &canonical.index_map,
                            para_byte_start,
                        )
                    })
                    .unwrap_or_default();
                (img, cls)
            } else {
                (None, Vec::new())
            };

            if clusters.is_empty()
                && line
                    .para_text
                    .chars()
                    .any(|c| !c.is_whitespace() && !c.is_control())
            {
                crate::backend::app_backend::debug_error_static(
                    "line_snapshot_builder",
                    "canonical_line_clusters_empty_invariant_failure",
                    &format!(
                        "revision={} para_start={} qtextline_idx={} byte_start={} byte_end={} \
                         image={} — clusters empty and line contains visible chars, \
                         this is an invariant failure: canonical layout should always produce \
                         clusters (Issue #810). Returning Err to surface the failure upstream, \
                         not producing a half-built snapshot.",
                        revision.0,
                        line.para_start,
                        line.qtextline_idx,
                        line.byte_start,
                        line.byte_end,
                        image.is_some(),
                    ),
                );
                return Err(format!(
                    "canonical_line_clusters_empty_invariant_failure: \
                     revision={} para_start={} qtextline_idx={} byte_start={} byte_end={}",
                    revision.0, line.para_start, line.qtextline_idx, line.byte_start, line.byte_end,
                ));
            }

            let id = LineSnapshotId::new(revision.0, paragraph_id, visual_line_ordinal);
            let (caret_top, caret_height) =
                cursor_rect_for_line(&line, doc_snapshot.font_size, &doc_snapshot.font_family);

            let (para_top, para_h) = paragraph_top_and_height(line.logical_slot, doc_snapshot);

            line_snapshots.push(PreparedLineSnapshot {
                id,

                // Issue #904 复核 8: 段落位移身份 = 稳定 ParagraphId，与静态 QSG 同源。
                stable_paragraph_id: line.paragraph_id.as_u64(),
                paragraph_top_document_y: para_top,
                paragraph_height: para_h,
                image,
                clusters,
                document_origin_y: line.y,
                dpr: doc_snapshot.dpr,
                byte_start: line.byte_start,
                byte_end: line.byte_end,
                visual_x: line.x,
                visual_line_top: line.y,
                visual_line_bottom: line.y + line.height,
                caret_top,
                caret_height,
            });

            visual_line_ordinal += 1;
        }

        Ok(EditorLayoutSnapshot {
            revision,
            line_snapshots,
            // Issue #904 复核 10：整章段落锚点（与视口裁剪无关），供跨视口位移使用。
            paragraph_anchors: paragraph_anchors(doc_snapshot),
            caret_rect: None,
            caret_rect_doc: None,
            caret_affinity: crate::editor::layout::CaretAffinity::Downstream,
        })
    }

    /// Issue #810 评论 5932233052 问题2: 返回
    /// `Result<(EditorLayoutSnapshot, EditorLayoutSnapshot), String>`。
    ///
    /// old 或 new 任一构建失败时返回 `Err`，调用方据此 fallback。
    pub fn build_old_new_from_canonical(
        old_doc: &CanonicalDocumentVisualSnapshot,
        new_doc: &CanonicalDocumentVisualSnapshot,
        old_revision: LayoutRevision,
        new_revision: LayoutRevision,
        scroll_y: f64,
        viewport_h: f64,
    ) -> Result<(EditorLayoutSnapshot, EditorLayoutSnapshot), String> {
        let old_layout =
            Self::build_from_canonical_document(old_revision, old_doc, scroll_y, viewport_h, None)?;
        let new_layout =
            Self::build_from_canonical_document(new_revision, new_doc, scroll_y, viewport_h, None)?;
        Ok((old_layout, new_layout))
    }

    /// Issue #901 第十五轮问题3: 增加 `para_byte_start` 参数，将 cluster 的
    /// `document_byte_start/end`（段内相对偏移）转换为全文绝对 byte 坐标。
    /// LineSnapshot 外层 `byte_start/end` 是全文绝对值，cluster 也必须一致。
    fn build_clusters_from_canonical(
        canonical_line: &CanonicalLineSnapshot,
        _line: &VisualLine,
        _index_map: &ParagraphIndexMap,
        para_byte_start: usize,
    ) -> Vec<LineClusterSnapshot> {
        canonical_line
            .clusters
            .iter()
            .map(|cc| {
                let shaping_identity = ShapingIdentity {
                    text_content_hash: Self::hash_str(&cc.cluster_text),
                    raw_font_fingerprint: cc.raw_font_fingerprint.clone(),
                    glyph_indexes_hash: Self::hash_u32(&[cc.first_glyph_index]),
                    cluster_glyph_count: cc.glyph_count,
                    direction_rtl: cc.is_rtl,
                    format_fingerprint: 0,
                };

                LineClusterSnapshot {
                    byte_start: cc.document_byte_start + para_byte_start,
                    byte_end: cc.document_byte_end + para_byte_start,
                    source_rect: SourceRect {
                        x: cc.source_rect_x,
                        y: cc.source_rect_y,
                        w: cc.source_rect_w,
                        h: cc.source_rect_h,
                    },
                    shaping_identity,
                }
            })
            .collect()
    }

    fn hash_u32(data: &[u32]) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        data.hash(&mut hasher);
        hasher.finish()
    }

    fn hash_str(data: &str) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        data.hash(&mut hasher);
        hasher.finish()
    }
}
