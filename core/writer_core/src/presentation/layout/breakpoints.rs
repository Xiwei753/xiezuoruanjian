//! # 窗口尺寸断点 — 素笺自己的窗口分类（#628）
//!
//! 本文件集中所有"窗口宽度/高度 → 窗口 class"的常量与分类函数。
//! 不引入 Android `WindowWidthSizeClass` 等平台类型；Core 自定义平台无关枚举。
//!
//! 断点表（与 Issue #628 评论一致）：
//!
//! ```text
//! width  < 600          -> Narrow
//! 600 <= width  < 840   -> Medium
//! 840 <= width  < 1200  -> Wide
//! 1200 <= width < 1600  -> Large
//! width  >= 1600        -> ExtraLarge
//!
//! height < 480          -> Compact
//! 480 <= height < 900   -> Medium
//! height >= 900         -> Tall
//! ```

use serde::{Deserialize, Serialize};

// ========== 宽度断点常量 ==========

/// Narrow 与 Medium 的分界（含）。
pub const WIDTH_MEDIUM_MIN: f32 = 600.0;
/// Medium 与 Wide 的分界（含）。
pub const WIDTH_WIDE_MIN: f32 = 840.0;
/// Wide 与 Large 的分界（含）。
pub const WIDTH_LARGE_MIN: f32 = 1200.0;
/// Large 与 ExtraLarge 的分界（含）。
pub const WIDTH_EXTRA_LARGE_MIN: f32 = 1600.0;

// ========== 高度断点常量 ==========

/// Compact 与 Medium 的分界（含）。
pub const HEIGHT_MEDIUM_MIN: f32 = 480.0;
/// Medium 与 Tall 的分界（含）。
pub const HEIGHT_TALL_MIN: f32 = 900.0;

// ========== 宽度 class ==========

/// 窗口宽度分类 — 平台无关，不引入 Android `WindowWidthSizeClass`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowWidthClass {
    /// `width < 600`
    Narrow,
    /// `600 <= width < 840`
    Medium,
    /// `840 <= width < 1200`
    Wide,
    /// `1200 <= width < 1600`
    Large,
    /// `width >= 1600`
    ExtraLarge,
}

/// 根据宽度（dp）计算窗口宽度分类。纯函数。
pub fn classify_width(width_dp: f32) -> WindowWidthClass {
    if width_dp < WIDTH_MEDIUM_MIN {
        WindowWidthClass::Narrow
    } else if width_dp < WIDTH_WIDE_MIN {
        WindowWidthClass::Medium
    } else if width_dp < WIDTH_LARGE_MIN {
        WindowWidthClass::Wide
    } else if width_dp < WIDTH_EXTRA_LARGE_MIN {
        WindowWidthClass::Large
    } else {
        WindowWidthClass::ExtraLarge
    }
}

// ========== 高度 class ==========

/// 窗口高度分类 — 平台无关。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowHeightClass {
    /// `height < 480`
    Compact,
    /// `480 <= height < 900`
    Medium,
    /// `height >= 900`
    Tall,
}

/// 根据高度（dp）计算窗口高度分类。纯函数。
pub fn classify_height(height_dp: f32) -> WindowHeightClass {
    if height_dp < HEIGHT_MEDIUM_MIN {
        WindowHeightClass::Compact
    } else if height_dp < HEIGHT_TALL_MIN {
        WindowHeightClass::Medium
    } else {
        WindowHeightClass::Tall
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_width_boundaries() {
        assert_eq!(classify_width(0.0), WindowWidthClass::Narrow);
        assert_eq!(classify_width(599.9), WindowWidthClass::Narrow);
        assert_eq!(classify_width(600.0), WindowWidthClass::Medium);
        assert_eq!(classify_width(839.9), WindowWidthClass::Medium);
        assert_eq!(classify_width(840.0), WindowWidthClass::Wide);
        assert_eq!(classify_width(1199.9), WindowWidthClass::Wide);
        assert_eq!(classify_width(1200.0), WindowWidthClass::Large);
        assert_eq!(classify_width(1599.9), WindowWidthClass::Large);
        assert_eq!(classify_width(1600.0), WindowWidthClass::ExtraLarge);
        assert_eq!(classify_width(3000.0), WindowWidthClass::ExtraLarge);
    }

    #[test]
    fn test_classify_height_boundaries() {
        assert_eq!(classify_height(0.0), WindowHeightClass::Compact);
        assert_eq!(classify_height(479.9), WindowHeightClass::Compact);
        assert_eq!(classify_height(480.0), WindowHeightClass::Medium);
        assert_eq!(classify_height(899.9), WindowHeightClass::Medium);
        assert_eq!(classify_height(900.0), WindowHeightClass::Tall);
        assert_eq!(classify_height(2000.0), WindowHeightClass::Tall);
    }

    #[test]
    fn test_negative_width_falls_into_narrow() {
        // 防御性：负数不应进入更高级别。
        assert_eq!(classify_width(-100.0), WindowWidthClass::Narrow);
        assert_eq!(classify_height(-100.0), WindowHeightClass::Compact);
    }
}
