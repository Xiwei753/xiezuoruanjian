//! 从 Qt 排版结果组装 Canonical document visual snapshot。
//!
//! 对外三条入口 + 动画纹理注入；实际组装在 `prepare/full.rs`。

use super::ffi::*;
use super::*;

/// Issue #658 评论 5624570557 问题 1+2: 从已有 generation 的 QTextLine 提取动画视觉资源。
/// 按 (generation, cache_slot, qtextline_idx) 读取现成 QTextLine，
/// 不重新排版，直接 line.draw() 到 QImage。
///
/// Issue #810 评论 5932233052 问题1: 本函数现在是 **raster-only** 提取，
/// 返回 `Vec<AnimationRasterVisual>`，只携带 QImage 和稳定行身份
///（paragraph_document_byte_start + qtextline_idx），**不携带 cluster**。
///
/// cluster 几何由基础 canonical 排版直接产出（engine.rs 中 cluster 提取始终执行），
/// 本函数不再承担"给 canonical 补 cluster"的职责。
/// `inject_animation_visuals_into_snapshot` 也只注入 image，不再覆盖 cluster。
///
/// Issue #658 评论 5625515748 问题 1: 移除统一的 `paragraph_text` / `paragraph_document_byte_start`
/// 参数，改为从每个 VisualLine 自身的 `para_text` / `para_start` 取段落级文本与文档起点。
/// qchar_start/qchar_end 来自各段落自己的 QTextLayout，是段落内 QChar offset，
/// 必须用对应段落的 para_text 做 QChar→byte 转换，再加该段落的 para_start 得到文档级 byte range。
/// SAFETY: GUI thread only; gen/slot 对应的 layout 由 EditorLayout 生命周期管理。
pub fn prepare_animation_visuals_from_layout(
    handle: &super::super::types::PreparedLayoutHandle<'_>,
    line_ids: &[usize],
    dpr: f64,
    text_color: &str,
) -> Vec<super::AnimationRasterVisual> {
    let color = qmetaobject::QColor::from_name(text_color);
    let mut snapshots = Vec::new();

    for &line_id in line_ids {
        if line_id >= handle.lines.len() {
            continue;
        }
        let line = &handle.lines[line_id];
        let slot = line.cache_slot;
        let qtextline_idx = line.qtextline_idx;
        let gen = handle.generation;
        // Issue #658 评论 5625515748 问题 1: 每行用自己的 para_text/para_start 做
        // QChar→byte 转换。qchar_start/qchar_end 是段落内 QChar offset，
        // 必须对应该段落的 para_text，再加 para_start 得到文档级 byte offset。
        let para_start = line.para_start;

        // SAFETY: GUI thread only; gen/slot 对应的 layout 由 EditorLayout 生命周期管理。
        let success = cpp::cpp!(unsafe [
            gen as "uint64_t",
            slot as "int",
            qtextline_idx as "int",
            dpr as "double",
            color as "QColor"
        ] -> bool as "bool" {
            extract_animation_visuals_from_existing_line(gen, slot, qtextline_idx, dpr, color);
            return !g_canonical_line_buf.empty();
        });

        if !success {
            continue;
        }

        // Issue #810 评论 5932233052 问题1: 只提取 image，不再提取 clusters/cursor_x_map。
        // cluster 几何由基础 canonical 排版直接产出，本函数只负责可延迟的 QImage/纹理。
        let image_phys_w = get_canonical_line_image_phys_w(0);
        let image_phys_h = get_canonical_line_image_phys_h(0);

        let image = if image_phys_w > 0 && image_phys_h > 0 {
            let mut img = qmetaobject::QImage::new(
                qmetaobject::QSize {
                    width: 1,
                    height: 1,
                },
                qmetaobject::ImageFormat::ARGB32_Premultiplied,
            );
            let img_ptr = &mut img as *mut qmetaobject::QImage;
            cpp::cpp!(unsafe [img_ptr as "QImage*"] {
                if (!g_canonical_line_images.empty()) {
                    *img_ptr = g_canonical_line_images[0];
                }
            });
            Some(img)
        } else {
            None
        };

        snapshots.push(super::AnimationRasterVisual {
            image,
            paragraph_document_byte_start: para_start,
            qtextline_idx,
        });
    }

    snapshots
}

pub fn prepare_document_visual_snapshot(
    text: &str,
    text_revision: u64,
    font_size: f64,
    font_family: &str,
    line_spacing: f64,
    padding: f64,
    indent: f64,
    width: f64,
    dpr: f64,
    generation: u64,
    generate_animation_visuals: bool,
) -> CanonicalDocumentVisualSnapshot {
    // Issue #658 评论 5623746506 问题 2a: 收口到受影响范围生成动画视觉。
    // affected_byte_start >= affected_byte_end 表示全篇生成（保持原语义）。
    // Issue #688: 静态布局路径不接收颜色参数；只有 generate_animation_visuals=true 时才需要
    prepare_document_visual_snapshot_impl(
        text,
        text_revision,
        font_size,
        font_family,
        line_spacing,
        padding,
        indent,
        width,
        dpr,
        None,
        generation,
        generate_animation_visuals,
        0,
        0,
    )
}

/// Issue #658 评论 5623746506 问题 2a: 按受影响字节范围生成动画视觉的排版入口。
///
/// 与 `prepare_document_visual_snapshot` 相同，但只有与 `[affected_byte_start,
/// affected_byte_end)` 有交集的段落才以 `generate_animation_visuals=true` 排版
/// （生成 QImage/glyphRuns/cluster）；其他段落只做基础排版
/// （`generate_animation_visuals=false`），保留 QTextLayout/VisualLine 供静态
/// QSGTextNode 消费。基础 canonical 排版仍一次生成完整 new text 的所有段落。
///
/// Issue #658 评论 5624570557 问题 2: 分离基础排版与动画视觉生成。
/// 本函数只做基础排版（QTextLayout + VisualLine + cursor map），不生成 QImage。
/// 调用方需随后调用 `prepare_animation_visuals_from_layout` 对受影响行提取动画资源。
pub fn prepare_document_visual_snapshot_scoped(
    text: &str,
    text_revision: u64,
    font_size: f64,
    font_family: &str,
    line_spacing: f64,
    padding: f64,
    indent: f64,
    width: f64,
    dpr: f64,
    text_color: Option<&str>,
    generation: u64,
    affected_byte_start: usize,
    affected_byte_end: usize,
) -> CanonicalDocumentVisualSnapshot {
    // Issue #658 评论 5624570557 问题 2: 基础排版不生成动画视觉，
    // 只得到 QTextLayout/VisualLine/cursor 几何。
    // 动画视觉由 prepare_animation_visuals_from_layout 单独提取。
    // Issue #688: 静态布局路径传 None；动画路径传 Some(text_color)
    prepare_document_visual_snapshot_impl(
        text,
        text_revision,
        font_size,
        font_family,
        line_spacing,
        padding,
        indent,
        width,
        dpr,
        text_color,
        generation,
        false,
        affected_byte_start,
        affected_byte_end,
    )
}

// 实际组装逻辑（整篇 impl / 从 VisualLine 组装 / 局部受影响段落）移到 full.rs。
mod full;

use full::prepare_document_visual_snapshot_impl;
pub use full::{
    assemble_document_visual_snapshot_from_lines, prepare_affected_paragraphs_visual_snapshot,
};

/// Issue #658 评论 5624570557 问题 1: 把从已有 layout 提取的动画 raster 视觉注入到 doc snapshot。
///
/// Issue #810 评论 5932233052 问题1: 本函数现在是 **raster-only** 注入，
/// 接收 `Vec<AnimationRasterVisual>`，**只注入 image**，不再覆盖 cluster。
///
/// cluster 几何由基础 canonical 排版直接产出（engine.rs 中 cluster 提取始终执行），
/// doc_snapshot 在调用本函数前已自带完整 clusters。本函数只把可延迟的 QImage/纹理
/// 注入到对应行，使动画纹理可用。
///
/// Issue #785 评论 5857873894 修改 2b: 按 (paragraph_document_byte_start, qtextline_idx)
/// 稳定行身份匹配目标行，不再只按 document_byte_start 猜。返回成功注入的行数，
/// 找不到目标行时通过 debug_warn 明确报告，不静默跳过，让调用方知道注入失败。
pub fn inject_animation_visuals_into_snapshot(
    doc_snapshot: &mut CanonicalDocumentVisualSnapshot,
    animation_visuals: Vec<super::AnimationRasterVisual>,
) -> usize {
    let mut injected_count: usize = 0;
    for anim_line in animation_visuals {
        // Issue #785 评论 5857873894 修改 2b: 优先按稳定行身份
        // (paragraph_document_byte_start, qtextline_idx) 精确匹配。
        // 这是 prepare_animation_visuals_from_layout 提取时记录的段落起点 + 段落内
        // 视觉行索引，与 doc_snapshot.paragraphs[].lines[].qtextline_idx 同源，
        // 不会因 document_byte_start 在 reflow 后漂移而误匹配。
        let mut matched = false;
        for para in &mut doc_snapshot.paragraphs {
            if para.paragraph_document_byte_start != anim_line.paragraph_document_byte_start {
                continue;
            }
            // 段落匹配，按 qtextline_idx 精确匹配行。
            // qtextline_idx 是段落内视觉行索引，与 para.lines 的索引一致。
            let line_idx = anim_line.qtextline_idx as usize;
            if let Some(line) = para.lines.get_mut(line_idx) {
                // Issue #810 评论 5932233052 问题1: 只注入可延迟的 image/texture，
                // 不覆盖基础 canonical 排版已产出的 cluster。
                line.image = anim_line.image.clone();
                injected_count += 1;
                matched = true;
            }
            break;
        }
        if !matched {
            crate::backend::app_backend::debug_warn_static(
                "canonical_snapshot",
                "inject_animation_visuals_line_not_found",
                &format!(
                    "paragraph_document_byte_start={} qtextline_idx={} — \
                     target line not found in doc_snapshot, animation raster visual dropped",
                    anim_line.paragraph_document_byte_start,
                    anim_line.qtextline_idx,
                ),
            );
        }
    }
    injected_count
}
