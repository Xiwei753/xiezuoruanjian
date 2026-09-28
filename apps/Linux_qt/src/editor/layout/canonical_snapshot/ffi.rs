//! 排版结果 thread_local 缓冲的取值 shim。
//!
//! C++ 侧把一次排版的 line / cluster / cursor_x_map 结果写进 thread_local 缓冲，
//! 这 26 个函数是 Rust 读回这些数据的唯一入口，签名与缓冲下标一一对应。
//! 全部 `pub(super)`：消费者只有 `canonical_snapshot.rs` 的 snapshot 组装方法
//! 和 `prepare.rs`。

use super::*;

pub(super) fn get_canonical_line_qchar_start(idx: i32) -> usize {
    cpp::cpp!(unsafe [idx as "int"] -> usize as "qulonglong" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return static_cast<qulonglong>(g_canonical_line_buf[idx].qcharStart);
        return 0;
    })
}

pub(super) fn get_canonical_line_qchar_end(idx: i32) -> usize {
    cpp::cpp!(unsafe [idx as "int"] -> usize as "qulonglong" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return static_cast<qulonglong>(g_canonical_line_buf[idx].qcharEnd);
        return 0;
    })
}

pub(super) fn get_canonical_line_x_pos(idx: i32) -> f64 {
    cpp::cpp!(unsafe [idx as "int"] -> f64 as "double" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return g_canonical_line_buf[idx].xPos;
        return 0.0;
    })
}

pub(super) fn get_canonical_line_width(idx: i32) -> f64 {
    cpp::cpp!(unsafe [idx as "int"] -> f64 as "double" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return g_canonical_line_buf[idx].width;
        return 0.0;
    })
}

pub(super) fn get_canonical_line_ascent(idx: i32) -> f64 {
    cpp::cpp!(unsafe [idx as "int"] -> f64 as "double" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return g_canonical_line_buf[idx].ascent;
        return 0.0;
    })
}

pub(super) fn get_canonical_line_descent(idx: i32) -> f64 {
    cpp::cpp!(unsafe [idx as "int"] -> f64 as "double" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return g_canonical_line_buf[idx].descent;
        return 0.0;
    })
}

pub(super) fn get_canonical_line_x_end_trailing(idx: i32) -> f64 {
    cpp::cpp!(unsafe [idx as "int"] -> f64 as "double" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return g_canonical_line_buf[idx].xEndTrailing;
        return 0.0;
    })
}

pub(super) fn get_canonical_line_image_phys_w(idx: i32) -> i32 {
    cpp::cpp!(unsafe [idx as "int"] -> i32 as "int" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return g_canonical_line_buf[idx].imagePhysW;
        return 0;
    })
}

pub(super) fn get_canonical_line_image_phys_h(idx: i32) -> i32 {
    cpp::cpp!(unsafe [idx as "int"] -> i32 as "int" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return g_canonical_line_buf[idx].imagePhysH;
        return 0;
    })
}

pub(super) fn get_canonical_line_cluster_start(idx: i32) -> i32 {
    cpp::cpp!(unsafe [idx as "int"] -> i32 as "int" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return g_canonical_line_buf[idx].clusterStartIndex;
        return 0;
    })
}

pub(super) fn get_canonical_line_cluster_count(idx: i32) -> i32 {
    cpp::cpp!(unsafe [idx as "int"] -> i32 as "int" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return g_canonical_line_buf[idx].clusterCount;
        return 0;
    })
}

pub(super) fn get_canonical_line_cursor_x_map_start(idx: i32) -> i32 {
    cpp::cpp!(unsafe [idx as "int"] -> i32 as "int" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return g_canonical_line_buf[idx].cursorXMapStart;
        return 0;
    })
}

pub(super) fn get_canonical_line_cursor_x_map_count(idx: i32) -> i32 {
    cpp::cpp!(unsafe [idx as "int"] -> i32 as "int" {
        if (idx >= 0 && idx < (int)g_canonical_line_buf.size())
            return g_canonical_line_buf[idx].cursorXMapCount;
        return 0;
    })
}

pub(super) fn get_canonical_cluster_qchar_start(idx: i32) -> usize {
    cpp::cpp!(unsafe [idx as "int"] -> usize as "qulonglong" {
        if (idx >= 0 && idx < (int)g_canonical_cluster_buf.size())
            return static_cast<qulonglong>(g_canonical_cluster_buf[idx].qcharStart);
        return 0;
    })
}

pub(super) fn get_canonical_cluster_qchar_end(idx: i32) -> usize {
    cpp::cpp!(unsafe [idx as "int"] -> usize as "qulonglong" {
        if (idx >= 0 && idx < (int)g_canonical_cluster_buf.size())
            return static_cast<qulonglong>(g_canonical_cluster_buf[idx].qcharEnd);
        return 0;
    })
}

pub(super) fn get_canonical_cluster_src_x(idx: i32) -> f64 {
    cpp::cpp!(unsafe [idx as "int"] -> f64 as "double" {
        if (idx >= 0 && idx < (int)g_canonical_cluster_buf.size())
            return g_canonical_cluster_buf[idx].sourceRectX;
        return 0.0;
    })
}

pub(super) fn get_canonical_cluster_src_y(idx: i32) -> f64 {
    cpp::cpp!(unsafe [idx as "int"] -> f64 as "double" {
        if (idx >= 0 && idx < (int)g_canonical_cluster_buf.size())
            return g_canonical_cluster_buf[idx].sourceRectY;
        return 0.0;
    })
}

pub(super) fn get_canonical_cluster_src_w(idx: i32) -> f64 {
    cpp::cpp!(unsafe [idx as "int"] -> f64 as "double" {
        if (idx >= 0 && idx < (int)g_canonical_cluster_buf.size())
            return g_canonical_cluster_buf[idx].sourceRectW;
        return 0.0;
    })
}

pub(super) fn get_canonical_cluster_src_h(idx: i32) -> f64 {
    cpp::cpp!(unsafe [idx as "int"] -> f64 as "double" {
        if (idx >= 0 && idx < (int)g_canonical_cluster_buf.size())
            return g_canonical_cluster_buf[idx].sourceRectH;
        return 0.0;
    })
}

pub(super) fn get_canonical_cluster_glyph_count(idx: i32) -> i32 {
    cpp::cpp!(unsafe [idx as "int"] -> i32 as "int" {
        if (idx >= 0 && idx < (int)g_canonical_cluster_buf.size())
            return g_canonical_cluster_buf[idx].glyphCount;
        return 0;
    })
}

pub(super) fn get_canonical_cluster_raw_font(idx: i32) -> QString {
    cpp::cpp!(unsafe [idx as "int"] -> QString as "QString" {
        if (idx >= 0 && idx < (int)g_canonical_cluster_buf.size())
            return QString::fromUtf8(g_canonical_cluster_buf[idx].rawFontFingerprint);
        return QString();
    })
}

pub(super) fn get_canonical_cluster_is_rtl(idx: i32) -> bool {
    cpp::cpp!(unsafe [idx as "int"] -> bool as "bool" {
        if (idx >= 0 && idx < (int)g_canonical_cluster_buf.size())
            return g_canonical_cluster_buf[idx].isRTL;
        return false;
    })
}

pub(super) fn get_canonical_cluster_first_glyph(idx: i32) -> u32 {
    cpp::cpp!(unsafe [idx as "int"] -> u32 as "quint32" {
        if (idx >= 0 && idx < (int)g_canonical_cluster_buf.size())
            return g_canonical_cluster_buf[idx].firstGlyphIndex;
        return 0;
    })
}

pub(super) fn get_cursor_x_map_qchar(idx: i32) -> usize {
    cpp::cpp!(unsafe [idx as "int"] -> usize as "qulonglong" {
        if (idx >= 0 && idx < (int)g_cursor_x_map_buf.size())
            return static_cast<qulonglong>(g_cursor_x_map_buf[idx].qcharPos);
        return 0;
    })
}

pub(super) fn get_cursor_x_map_x_leading(idx: i32) -> f64 {
    cpp::cpp!(unsafe [idx as "int"] -> f64 as "double" {
        if (idx >= 0 && idx < (int)g_cursor_x_map_buf.size())
            return g_cursor_x_map_buf[idx].xLeading;
        return 0.0;
    })
}

pub(super) fn get_cursor_x_map_x_trailing(idx: i32) -> f64 {
    cpp::cpp!(unsafe [idx as "int"] -> f64 as "double" {
        if (idx >= 0 && idx < (int)g_cursor_x_map_buf.size())
            return g_cursor_x_map_buf[idx].xTrailing;
        return 0.0;
    })
}
