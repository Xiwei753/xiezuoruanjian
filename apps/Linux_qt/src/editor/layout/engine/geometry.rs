//! 排版结果的 Rust 侧几何 helper 与坐标转换。
//!
//! 与 `engine.rs` 分离的原因：`engine.rs` 里那 705 行是**必须保持单块**的
//! `cpp! {{ }}` C++ 段 —— `cpp_build` 按 mod 顺序把它拼进同一个 C++ 文件，
//! 拆开会让 `EditorLayoutEntry`、thread_local 缓冲与 qt_cache 的前向声明顺序
//! 失效。Rust 侧这一半没有这个约束，单独成文件后 `engine.rs` 才降到门禁线以下。

use super::super::canonical_snapshot::{
    CanonicalClusterSnapshot, CanonicalLineSnapshot, CanonicalParagraphSnapshot, CursorXMapEntry,
};
use super::super::types::VisualLine;
use cpp::cpp;
use qmetaobject::QString;

pub fn byte_offset_to_qchar_offset(text: &str, byte_offset: usize) -> usize {
    text[..byte_offset.min(text.len())]
        .chars()
        .map(|c| c.len_utf16())
        .sum()
}

pub fn qchar_offset_to_byte_offset(text: &str, qchar_offset: usize) -> usize {
    let mut qchar_count: usize = 0;
    for (byte_pos, ch) in text.char_indices() {
        if qchar_count >= qchar_offset {
            return byte_pos;
        }
        qchar_count += ch.len_utf16();
    }
    text.len()
}

pub fn get_font_ascent(font_family: &str, font_size: f32) -> f64 {
    let family = QString::from(font_family);
    // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
    cpp!(unsafe [family as "QString", font_size as "float"] -> f64 as "double" {
        QFont font(family);
        font.setPixelSize(font_size);
        QFontMetricsF metrics(font);
        return metrics.ascent();
    })
}

pub fn get_font_descent(font_family: &str, font_size: f32) -> f64 {
    let family = QString::from(font_family);
    // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
    cpp!(unsafe [family as "QString", font_size as "float"] -> f64 as "double" {
        QFont font(family);
        font.setPixelSize(font_size);
        QFontMetricsF metrics(font);
        return metrics.descent();
    })
}

pub fn cursor_rect_for_line(line: &VisualLine, font_size: f64, font_family: &str) -> (f64, f64) {
    let ascent = if line.qt_ascent > 0.0 {
        line.qt_ascent
    } else {
        get_font_ascent(font_family, font_size as f32)
    };
    let descent = if line.qt_descent > 0.0 {
        line.qt_descent
    } else {
        get_font_descent(font_family, font_size as f32)
    };
    let baseline = text_baseline_y(line, font_size, font_family);
    let h = (ascent + descent).min(line.height);
    let mut top_y = baseline - ascent;
    if top_y < line.y {
        top_y = line.y;
    }
    if top_y + h > line.y + line.height {
        top_y = line.y + line.height - h;
    }
    (top_y, h)
}

pub fn text_baseline_y(line: &VisualLine, font_size: f64, font_family: &str) -> f64 {
    let ascent = if line.qt_ascent > 0.0 {
        line.qt_ascent
    } else {
        get_font_ascent(font_family, font_size as f32)
    };
    let descent = if line.qt_descent > 0.0 {
        line.qt_descent
    } else {
        get_font_descent(font_family, font_size as f32)
    };
    let top_padding = (line.height - (ascent + descent)).max(0.0) / 2.0;
    line.y + top_padding + ascent
}

/// Issue #748: 用 QFontMetricsF::horizontalAdvance 测量文本宽度（纯 QFont 测量），
/// 不创建 QTextLayout，符合"正式路径只从 PreparedLayoutHandle/QTextLine 走"的要求。
/// 供 EditorLayout::text_width 测量 preedit/IME 文本宽度。
/// SAFETY: GUI thread only; QFont/QFontMetricsF 不依赖 QTextLayout 生命周期。
pub fn text_width(text: &str, font_size: f64, font_family: &str) -> f64 {
    let qtext: QString = text.to_string().into();
    let fs = font_size as f32;
    let ff: QString = font_family.to_string().into();
    // SAFETY: GUI thread only; QFont/QFontMetricsF 不依赖 QTextLayout 生命周期。
    cpp!(unsafe [qtext as "QString", fs as "float", ff as "QString"] -> f64 as "double" {
        QFont font(ff);
        font.setPixelSize(static_cast<int>(fs));
        QFontMetricsF metrics(font);
        return metrics.horizontalAdvance(qtext);
    })
}

pub fn prepare_paragraph_visual_snapshot(
    paragraph_text: &str,
    paragraph_document_byte_start: usize,
    font_size: f64,
    font_family: &str,
    wrap_w: f64,
    indent_w: f64,
    dpr: f64,
    text_color: Option<&str>,
    cache_slot: i32,
    line_spacing: f64,
    generation: u64,
    generate_animation_visuals: bool,
) -> CanonicalParagraphSnapshot {
    let index_map = crate::editor::paragraph_index_map::ParagraphIndexMap::build(
        paragraph_text,
        paragraph_document_byte_start,
    );

    // Issue #688: 静态布局路径不接收颜色参数；只有 generate_animation_visuals=true 时才使用真实颜色
    let color = if generate_animation_visuals {
        text_color.map_or_else(
            || qmetaobject::QColor::from_name(""),
            |c| qmetaobject::QColor::from_name(c),
        )
    } else {
        qmetaobject::QColor::from_name("")
    };

    if paragraph_text.is_empty() {
        // Issue #658: 空段落也调用 C++ 占 null slot，保持 cache_slot 与文档段落一一对应。
        let para: QString = paragraph_text.to_string().into();
        let fs = font_size as f32;
        let ff: QString = font_family.to_string().into();
        let ls = line_spacing;
        // SAFETY: GUI thread only; cache_slot 是文档段落索引，由调用方保证有效。
        cpp!(unsafe [
            para as "QString",
            fs as "float",
            ff as "QString",
            wrap_w as "double",
            indent_w as "double",
            dpr as "double",
            color as "QColor",
            cache_slot as "int",
            ls as "double",
            generation as "uint64_t",
            generate_animation_visuals as "bool"
        ] {
            editor_prepare_paragraph_visual_snapshot(para, fs, ff, wrap_w, indent_w, dpr, color, cache_slot, ls, generation, generate_animation_visuals);
        });
        return CanonicalParagraphSnapshot {
            paragraph_text: paragraph_text.to_string(),
            paragraph_document_byte_start,
            lines: Vec::new(),
            index_map,
        };
    }

    let para: QString = paragraph_text.to_string().into();
    let fs = font_size as f32;
    let ff: QString = font_family.to_string().into();
    let ls = line_spacing;

    // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
    let line_count = cpp!(unsafe [
        para as "QString",
        fs as "float",
        ff as "QString",
        wrap_w as "double",
        indent_w as "double",
        dpr as "double",
        color as "QColor",
        cache_slot as "int",
        ls as "double",
        generation as "uint64_t",
        generate_animation_visuals as "bool"
    ] -> i32 as "int" {
        editor_prepare_paragraph_visual_snapshot(para, fs, ff, wrap_w, indent_w, dpr, color, cache_slot, ls, generation, generate_animation_visuals);
        return static_cast<int>(g_canonical_line_buf.size());
    });

    let mut lines = Vec::with_capacity(line_count as usize);

    for line_idx in 0..line_count {
        let idx = line_idx;

        // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
        let qchar_start = cpp!(unsafe [idx as "int"] -> usize as "qulonglong" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return static_cast<qulonglong>(g_canonical_line_buf[idx].qcharStart);
            return 0;
        });
        // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
        let qchar_end = cpp!(unsafe [idx as "int"] -> usize as "qulonglong" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return static_cast<qulonglong>(g_canonical_line_buf[idx].qcharEnd);
            return 0;
        });
        // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
        let x_pos = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].xPos;
            return 0.0;
        });
        // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
        let width = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].width;
            return 0.0;
        });
        // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
        let _height = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].height;
            return 0.0;
        });
        // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
        let ascent = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].ascent;
            return 0.0;
        });
        // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
        let descent = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].descent;
            return 0.0;
        });
        // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
        let _y = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].y;
            return 0.0;
        });
        // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
        let _x_end_leading = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].xEndLeading;
            return 0.0;
        });
        // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
        let x_end_trailing = cpp!(unsafe [idx as "int"] -> f64 as "double" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].xEndTrailing;
            return 0.0;
        });
        // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
        let cluster_start = cpp!(unsafe [idx as "int"] -> i32 as "int" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].clusterStartIndex;
            return 0;
        });
        // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
        let cluster_count = cpp!(unsafe [idx as "int"] -> i32 as "int" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].clusterCount;
            return 0;
        });
        // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
        let image_phys_w = cpp!(unsafe [idx as "int"] -> i32 as "int" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].imagePhysW;
            return 0;
        });
        // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
        let image_phys_h = cpp!(unsafe [idx as "int"] -> i32 as "int" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].imagePhysH;
            return 0;
        });

        let image = if image_phys_w > 0 && image_phys_h > 0 {
            let mut img = qmetaobject::QImage::new(
                qmetaobject::QSize {
                    width: 1,
                    height: 1,
                },
                qmetaobject::ImageFormat::ARGB32_Premultiplied,
            );
            let img_ptr = &mut img as *mut qmetaobject::QImage;
            // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
            cpp!(unsafe [img_ptr as "QImage*", idx as "int"] {
                editor_copy_canonical_line_image(idx, img_ptr);
            });
            Some(img)
        } else {
            None
        };

        let doc_byte_start = index_map.qchar_to_document_byte(qchar_start);
        let doc_byte_end = index_map.qchar_to_document_byte(qchar_end);

        let mut clusters = Vec::with_capacity(cluster_count as usize);
        for ci in 0..cluster_count {
            let cidx = cluster_start + ci;

            // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
            let c_qchar_start = cpp!(unsafe [cidx as "int"] -> usize as "qulonglong" {
                if (cidx >= 0 && cidx < (int)g_canonical_cluster_buf.size())
                    return static_cast<qulonglong>(g_canonical_cluster_buf[cidx].qcharStart);
                return 0;
            });
            // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
            let c_qchar_end = cpp!(unsafe [cidx as "int"] -> usize as "qulonglong" {
                if (cidx >= 0 && cidx < (int)g_canonical_cluster_buf.size())
                    return static_cast<qulonglong>(g_canonical_cluster_buf[cidx].qcharEnd);
                return 0;
            });
            // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
            let c_src_x = cpp!(unsafe [cidx as "int"] -> f64 as "double" {
                if (cidx >= 0 && cidx < (int)g_canonical_cluster_buf.size())
                    return g_canonical_cluster_buf[cidx].sourceRectX;
                return 0.0;
            });
            // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
            let c_src_y = cpp!(unsafe [cidx as "int"] -> f64 as "double" {
                if (cidx >= 0 && cidx < (int)g_canonical_cluster_buf.size())
                    return g_canonical_cluster_buf[cidx].sourceRectY;
                return 0.0;
            });
            // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
            let c_src_w = cpp!(unsafe [cidx as "int"] -> f64 as "double" {
                if (cidx >= 0 && cidx < (int)g_canonical_cluster_buf.size())
                    return g_canonical_cluster_buf[cidx].sourceRectW;
                return 0.0;
            });
            // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
            let c_src_h = cpp!(unsafe [cidx as "int"] -> f64 as "double" {
                if (cidx >= 0 && cidx < (int)g_canonical_cluster_buf.size())
                    return g_canonical_cluster_buf[cidx].sourceRectH;
                return 0.0;
            });
            // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
            let c_glyph_count = cpp!(unsafe [cidx as "int"] -> i32 as "int" {
                if (cidx >= 0 && cidx < (int)g_canonical_cluster_buf.size())
                    return g_canonical_cluster_buf[cidx].glyphCount;
                return 0;
            });
            // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
            let c_raw_font: QString = cpp!(unsafe [cidx as "int"] -> QString as "QString" {
                if (cidx >= 0 && cidx < (int)g_canonical_cluster_buf.size())
                    return QString::fromUtf8(g_canonical_cluster_buf[cidx].rawFontFingerprint);
                return QString();
            });
            // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
            let c_is_rtl = cpp!(unsafe [cidx as "int"] -> bool as "bool" {
                if (cidx >= 0 && cidx < (int)g_canonical_cluster_buf.size())
                    return g_canonical_cluster_buf[cidx].isRTL;
                return false;
            });
            // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
            let c_first_glyph = cpp!(unsafe [cidx as "int"] -> u32 as "quint32" {
                if (cidx >= 0 && cidx < (int)g_canonical_cluster_buf.size())
                    return g_canonical_cluster_buf[cidx].firstGlyphIndex;
                return 0;
            });

            let c_doc_byte_start = index_map.qchar_to_document_byte(c_qchar_start);
            let c_doc_byte_end = index_map.qchar_to_document_byte(c_qchar_end);

            let (c_byte_start, c_byte_end) =
                index_map.qchar_range_to_document_byte_range(c_qchar_start, c_qchar_end);
            let c_cluster_text: String = if c_byte_start <= c_byte_end
                && c_byte_end <= paragraph_text.len() + paragraph_document_byte_start
            {
                let local_start = c_byte_start.saturating_sub(paragraph_document_byte_start);
                let local_end = c_byte_end.saturating_sub(paragraph_document_byte_start);
                if local_start <= local_end && local_end <= paragraph_text.len() {
                    paragraph_text[local_start..local_end].to_string()
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            clusters.push(CanonicalClusterSnapshot {
                document_byte_start: c_doc_byte_start,
                document_byte_end: c_doc_byte_end,
                source_rect_x: c_src_x,
                source_rect_y: c_src_y,
                source_rect_w: c_src_w,
                source_rect_h: c_src_h,
                glyph_count: c_glyph_count as usize,
                raw_font_fingerprint: c_raw_font.to_string(),
                is_rtl: c_is_rtl,
                first_glyph_index: c_first_glyph,
                cluster_text: c_cluster_text,
            });
        }

        // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
        let cursor_x_map_start = cpp!(unsafe [idx as "int"] -> i32 as "int" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].cursorXMapStart;
            return 0;
        });
        // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
        let cursor_x_map_count = cpp!(unsafe [idx as "int"] -> i32 as "int" {
            if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
                return g_canonical_line_buf[idx].cursorXMapCount;
            return 0;
        });

        let mut cursor_x_map = Vec::with_capacity(cursor_x_map_count as usize);
        for mi in 0..cursor_x_map_count {
            let midx = cursor_x_map_start + mi;
            // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
            let m_qchar = cpp!(unsafe [midx as "int"] -> usize as "qulonglong" {
                if (midx >= 0 && midx < (int)g_cursor_x_map_buf.size())
                    return static_cast<qulonglong>(g_cursor_x_map_buf[midx].qcharPos);
                return 0;
            });
            // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
            let m_x_leading = cpp!(unsafe [midx as "int"] -> f64 as "double" {
                if (midx >= 0 && midx < (int)g_cursor_x_map_buf.size())
                    return g_cursor_x_map_buf[midx].xLeading;
                return 0.0;
            });
            // SAFETY: pointer from Qt scene graph/QML engine; valid while owning QQuickItem/node alive; GUI thread only; null-checked or guaranteed non-null by caller.
            let m_x_trailing = cpp!(unsafe [midx as "int"] -> f64 as "double" {
                if (midx >= 0 && midx < (int)g_cursor_x_map_buf.size())
                    return g_cursor_x_map_buf[midx].xTrailing;
                return 0.0;
            });
            cursor_x_map.push(CursorXMapEntry {
                qchar_pos: m_qchar,
                x_leading: m_x_leading,
                x_trailing: m_x_trailing,
            });
        }

        lines.push(CanonicalLineSnapshot {
            qchar_start,
            qchar_end,
            document_byte_start: doc_byte_start,
            document_byte_end: doc_byte_end,
            x_pos,
            width,
            ascent,
            descent,
            x_end_trailing,
            image,
            clusters,
            cursor_x_map,
            // Issue #785 评论 5857873894 修改 2a: 填充稳定行身份。
            // paragraph_document_byte_start 是函数参数（段落文档 byte 起始），
            // idx = line_idx 是段落内 qtextline 索引。
            paragraph_document_byte_start,
            qtextline_idx: idx,
        });
    }

    CanonicalParagraphSnapshot {
        paragraph_text: paragraph_text.to_string(),
        paragraph_document_byte_start,
        lines,
        index_map,
    }
}
