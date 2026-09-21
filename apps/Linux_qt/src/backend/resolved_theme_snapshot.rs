//! Issue #727 评论 5755858583 问题3: 统一深色主题链为 ResolvedThemeUiSnapshot。
//!
//! Rust 侧一次性生成最终 `colors`（包含所有主题颜色的最终 RGBA 值），
//! `theme_state_json()` 只序列化这份 UI snapshot（不再序列化 scheme DTO）。
//! `DesignTokens.qml::applyThemeState()` 只读 `parsed.colors`，不再自己 fallback。
//!
//! 本模块定义 `ResolvedThemeUiSnapshot` 结构，供 `theme_state_json()` 构造。
//! 实际的颜色解析逻辑在 `linux_theme_controller.rs::theme_state_json()` 中，
//! 直接从 `ThemeColorSchemeDto` 序列化生成 `colors` 对象。

use serde::Serialize;

/// Issue #727 评论 5755858583 问题3: 统一主题 UI 快照。
///
/// Rust 侧一次性生成最终颜色值，QML 侧只读不 fallback。
/// `colors` 为 `None` 时 QML 侧用 `is_dark` 派生固定色。
#[derive(Clone, Debug, Serialize)]
pub struct ResolvedThemeUiSnapshot {
    pub appearance_mode: String,
    pub system_is_dark: bool,
    pub is_dark: bool,
    pub color_source: String,
    pub selected_builtin_theme_id: String,
    pub selected_palette_id: String,
    pub resolved_source: String,
    pub resolved_scheme_kind: String,
    /// 最终颜色值（ThemeColorSchemeDto 序列化结果）。
    /// `None` 时 QML 侧用 `is_dark` fallback。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub colors: Option<serde_json::Value>,
}
