//! 排版结果 thread_local 缓冲的取值 shim。
//!
//! C++ 侧把一次排版的 line / cluster / cursor_x_map 结果写进 thread_local 缓冲，
//! 这些函数是 Rust 读回这些数据的唯一入口。
//! 全部 `pub(super)`：消费者只有 `prepare.rs`。
//!
//! Issue #810 评论 5932233052 问题1: prepare_animation_visuals_from_layout 改为
//! raster-only 后，只保留 image 物理尺寸读取 shim。cluster / cursor_x_map / line
//! 几何的读取 shim 已删除（cluster 几何由基础 canonical 排版直接产出，不再需要
//! 从 C++ buffer 二次提取）。被新实现替代的旧入口直接删除（AGENTS.md）。

use super::*;

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
