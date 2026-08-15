//! # 共用布局尺寸 — 平台无关的共享数值（#628）
//!
//! 列表栏宽度、内容最大宽度等"产品语义尺寸"集中在此。
//! 平台端通过 `LayoutContract.metrics` 读取后做 `.dp` 映射，
//! 不再各自硬编码 `320.dp` 这类数值。

use serde::{Deserialize, Serialize};

/// 共用布局尺寸 — 由 Core 决定的产品级尺寸常量（dp 单位）。
///
/// 平台端只做 dp → 像素映射，不再自行决定列表栏宽度等数值。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LayoutMetrics {
    /// 列表栏（作品列表 / 章节树）宽度，dp。
    /// 三端共用，避免 Android 写 `320.dp`、Qt 写 `320px`、Harmony 写 `320vp` 的多源真相。
    pub list_pane_width_dp: f32,
}

impl Default for LayoutMetrics {
    fn default() -> Self {
        Self {
            list_pane_width_dp: DEFAULT_LIST_PANE_WIDTH_DP,
        }
    }
}

/// 列表栏默认宽度（dp）。原 Android 端写死的 `320.dp`。
pub const DEFAULT_LIST_PANE_WIDTH_DP: f32 = 320.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_metrics_has_list_pane_width() {
        let m = LayoutMetrics::default();
        assert_eq!(m.list_pane_width_dp, DEFAULT_LIST_PANE_WIDTH_DP);
        assert_eq!(m.list_pane_width_dp, 320.0);
    }

    #[test]
    fn test_metrics_serialization_roundtrip() {
        let m = LayoutMetrics {
            list_pane_width_dp: 360.0,
        };
        let json = serde_json::to_string(&m).expect("serialization must succeed");
        let back: LayoutMetrics =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(back, m);
    }
}
