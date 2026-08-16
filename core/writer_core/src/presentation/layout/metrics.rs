//! # 共用布局尺寸 — 平台无关的共享数值（#628）
//!
//! 列表栏宽度、内容最大宽度等"产品语义尺寸"集中在此。
//! 平台端通过 `LayoutContract.metrics` 读取后做 `.dp` 映射，
//! 不再各自硬编码 `320.dp` 这类数值。
//!
//! #628 评论 5301021120 问题 3：`editor_min_width_dp` / `toolbar_height_dp` /
//! `toolbar_leading_width_dp` / `toolbar_trailing_width_dp` 以及左右 pane 的最小宽度
//! 全部收回此处，避免 `resolver.rs` 散落 `64 / 200 / 200 / 1` 字面量。

use serde::{Deserialize, Serialize};

/// 共用布局尺寸 — 由 Core 决定的产品级尺寸常量（dp 单位）。
///
/// 平台端只做 dp → 像素映射，不再自行决定列表栏宽度等数值。
///
/// #628 验收点 4：把新出现的结构尺寸继续收回 `LayoutMetrics`，
/// 避免平台端各自硬编码 `180.dp` / `240.dp` / `56.dp`。
///
/// #628 评论 5301021120 问题 3：再收回 `editor_min_width_dp` /
/// `toolbar_height_dp` / `toolbar_leading_width_dp` / `toolbar_trailing_width_dp`，
/// 以及左右 pane 的最小压缩宽度。`resolver.rs` 不再出现 `1.0` / `64.0` / `200.0`
/// 这类散落字面量。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LayoutMetrics {
    /// 列表栏（作品列表 / 章节树）宽度，dp。
    /// 三端共用，避免 Android 写 `320.dp`、Qt 写 `320px`、Harmony 写 `320vp` 的多源真相。
    pub list_pane_width_dp: f32,
    /// 作品卡最小宽度，dp。原 Android 端写死的 `180.dp`。
    pub project_card_min_width_dp: f32,
    /// 工作台工具栏宽度，dp。原 Android 端写死的 `240.dp`。
    pub tool_pane_width_dp: f32,
    /// 工具 rail 宽度，dp。原 Android 端写死的 `56.dp`。
    pub tool_rail_width_dp: f32,
    /// 正文编辑器最小可编辑宽度，dp（#628 评论 5301021120 问题 3）。
    ///
    /// 不再用"非零就算可用"的 `1.0`：低于此宽度时正文已无法正常编辑，
    /// Rust 直接判定本次布局语义失效（`WorkbenchLayoutPlan.valid = false`），
    /// 退化为 Editor 单栏占满最大可用 free region，而不是把侧栏压成细线
    /// 只给正文留 1dp。初始值 `240.0 dp` 起步——这是真正能放下一段正文
    /// （含行号 + 一行可读字符）的最小宽度。
    pub editor_min_width_dp: f32,
    /// 工作台顶栏高度，dp。原 `resolver.rs` 散落的 `64.0`。
    pub toolbar_height_dp: f32,
    /// 顶栏左组（返回/撤销/重做/收起）宽度，dp。原 `resolver.rs` 散落的 `200.0`。
    pub toolbar_leading_width_dp: f32,
    /// 顶栏右组（同步/搜索/设置）宽度，dp。原 `resolver.rs` 散落的 `200.0`。
    pub toolbar_trailing_width_dp: f32,
    /// 列表栏最小压缩宽度，dp（#628 评论 5301021120 问题 3）。
    ///
    /// 空间紧张时列表栏可在 `list_pane_width_dp` 与此值之间压缩，
    /// 但不会无限压成细线。低于此值时由 visibility 决定是否完全收起。
    pub list_pane_min_width_dp: f32,
    /// 工具面板最小压缩宽度，dp（#628 评论 5301021120 问题 3）。
    ///
    /// 空间紧张时工具面板可在 `tool_pane_width_dp` 与此值之间压缩，
    /// 但不会无限压成细线。低于此值时由 visibility 决定是否完全收起。
    pub tool_pane_min_width_dp: f32,
}

impl Default for LayoutMetrics {
    fn default() -> Self {
        Self {
            list_pane_width_dp: DEFAULT_LIST_PANE_WIDTH_DP,
            project_card_min_width_dp: DEFAULT_PROJECT_CARD_MIN_WIDTH_DP,
            tool_pane_width_dp: DEFAULT_TOOL_PANE_WIDTH_DP,
            tool_rail_width_dp: DEFAULT_TOOL_RAIL_WIDTH_DP,
            editor_min_width_dp: DEFAULT_EDITOR_MIN_WIDTH_DP,
            toolbar_height_dp: DEFAULT_TOOLBAR_HEIGHT_DP,
            toolbar_leading_width_dp: DEFAULT_TOOLBAR_LEADING_WIDTH_DP,
            toolbar_trailing_width_dp: DEFAULT_TOOLBAR_TRAILING_WIDTH_DP,
            list_pane_min_width_dp: DEFAULT_LIST_PANE_MIN_WIDTH_DP,
            tool_pane_min_width_dp: DEFAULT_TOOL_PANE_MIN_WIDTH_DP,
        }
    }
}

/// 列表栏默认宽度（dp）。原 Android 端写死的 `320.dp`。
pub const DEFAULT_LIST_PANE_WIDTH_DP: f32 = 320.0;
/// 作品卡最小默认宽度（dp）。原 Android 端写死的 `180.dp`（#628 验收点 4）。
pub const DEFAULT_PROJECT_CARD_MIN_WIDTH_DP: f32 = 180.0;
/// 工作台工具栏默认宽度（dp）。原 Android 端写死的 `240.dp`（#628 验收点 4）。
pub const DEFAULT_TOOL_PANE_WIDTH_DP: f32 = 240.0;
/// 工具 rail 默认宽度（dp）。原 Android 端写死的 `56.dp`（#628 验收点 4）。
pub const DEFAULT_TOOL_RAIL_WIDTH_DP: f32 = 56.0;
/// 正文编辑器最小可编辑宽度（dp）。#628 评论 5301021120 问题 3。
///
/// 240 dp 是真正能放下"行号 + 一行可读字符"的最小宽度，低于此值正文
/// 已不可正常编辑——不再用"非零就算可用"的 1dp。
pub const DEFAULT_EDITOR_MIN_WIDTH_DP: f32 = 240.0;
/// 工作台顶栏高度（dp）。原 `resolver.rs` 散落的 `64.0`。
pub const DEFAULT_TOOLBAR_HEIGHT_DP: f32 = 64.0;
/// 顶栏左组宽度（dp）。原 `resolver.rs` 散落的 `200.0`。
pub const DEFAULT_TOOLBAR_LEADING_WIDTH_DP: f32 = 200.0;
/// 顶栏右组宽度（dp）。原 `resolver.rs` 散落的 `200.0`。
pub const DEFAULT_TOOLBAR_TRAILING_WIDTH_DP: f32 = 200.0;
/// 列表栏最小压缩宽度（dp）。空间紧张时下限，避免压成细线。
pub const DEFAULT_LIST_PANE_MIN_WIDTH_DP: f32 = 200.0;
/// 工具面板最小压缩宽度（dp）。空间紧张时下限，避免压成细线。
pub const DEFAULT_TOOL_PANE_MIN_WIDTH_DP: f32 = 200.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_metrics_has_all_fields() {
        let m = LayoutMetrics::default();
        assert_eq!(m.list_pane_width_dp, DEFAULT_LIST_PANE_WIDTH_DP);
        assert_eq!(m.list_pane_width_dp, 320.0);
        assert_eq!(
            m.project_card_min_width_dp,
            DEFAULT_PROJECT_CARD_MIN_WIDTH_DP
        );
        assert_eq!(m.project_card_min_width_dp, 180.0);
        assert_eq!(m.tool_pane_width_dp, DEFAULT_TOOL_PANE_WIDTH_DP);
        assert_eq!(m.tool_pane_width_dp, 240.0);
        assert_eq!(m.tool_rail_width_dp, DEFAULT_TOOL_RAIL_WIDTH_DP);
        assert_eq!(m.tool_rail_width_dp, 56.0);
        // #628 评论 5301021120 问题 3：新字段默认值。
        assert_eq!(m.editor_min_width_dp, DEFAULT_EDITOR_MIN_WIDTH_DP);
        assert_eq!(m.editor_min_width_dp, 240.0);
        assert_eq!(m.toolbar_height_dp, DEFAULT_TOOLBAR_HEIGHT_DP);
        assert_eq!(m.toolbar_height_dp, 64.0);
        assert_eq!(m.toolbar_leading_width_dp, DEFAULT_TOOLBAR_LEADING_WIDTH_DP);
        assert_eq!(m.toolbar_leading_width_dp, 200.0);
        assert_eq!(
            m.toolbar_trailing_width_dp,
            DEFAULT_TOOLBAR_TRAILING_WIDTH_DP
        );
        assert_eq!(m.toolbar_trailing_width_dp, 200.0);
        assert_eq!(m.list_pane_min_width_dp, DEFAULT_LIST_PANE_MIN_WIDTH_DP);
        assert_eq!(m.list_pane_min_width_dp, 200.0);
        assert_eq!(m.tool_pane_min_width_dp, DEFAULT_TOOL_PANE_MIN_WIDTH_DP);
        assert_eq!(m.tool_pane_min_width_dp, 200.0);
    }

    #[test]
    fn test_metrics_serialization_roundtrip() {
        let m = LayoutMetrics {
            list_pane_width_dp: 360.0,
            project_card_min_width_dp: 200.0,
            tool_pane_width_dp: 280.0,
            tool_rail_width_dp: 64.0,
            editor_min_width_dp: 280.0,
            toolbar_height_dp: 72.0,
            toolbar_leading_width_dp: 220.0,
            toolbar_trailing_width_dp: 220.0,
            list_pane_min_width_dp: 240.0,
            tool_pane_min_width_dp: 240.0,
        };
        let json = serde_json::to_string(&m).expect("serialization must succeed");
        let back: LayoutMetrics =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(back, m);
    }

    #[test]
    fn test_editor_min_width_is_real_editable_not_one_dp() {
        // #628 评论 5301021120 问题 3：editor_min_width_dp 必须是真正可编辑宽度，不是 1dp。
        let m = LayoutMetrics::default();
        assert!(
            m.editor_min_width_dp >= 240.0,
            "editor_min_width_dp 应 >= 240dp，实际 = {}",
            m.editor_min_width_dp
        );
        assert_ne!(m.editor_min_width_dp, 1.0);
    }

    #[test]
    fn test_pane_min_widths_are_reasonable_lower_bounds() {
        // pane 最小压缩宽度应在 (0, preferred] 之间，避免压成细线。
        let m = LayoutMetrics::default();
        assert!(m.list_pane_min_width_dp > 0.0);
        assert!(m.list_pane_min_width_dp <= m.list_pane_width_dp);
        assert!(m.tool_pane_min_width_dp > 0.0);
        assert!(m.tool_pane_min_width_dp <= m.tool_pane_width_dp);
    }
}
