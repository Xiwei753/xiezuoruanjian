//! Issue #727 评论 5757225958 问题4: 统一深色主题链为 ResolvedThemeUiSnapshot。
//!
//! Rust 侧一次性生成最终 `colors`（包含所有主题颜色的最终 hex 字符串值），
//! `theme_state_json()` 只序列化这份 UI snapshot（不再手工拼 serde_json::Map）。
//! `DesignTokens.qml::applyThemeState()` 只读 `parsed.colors`，不再自己 fallback。
//!
//! 本模块定义 `ResolvedThemeUiSnapshot` 和 `ResolvedThemeUiColors` 结构，
//! 供 `linux_theme_controller.rs::theme_state_json()` 构造。
//! fallback 逻辑也在这里完成（scheme 为 None 时根据 is_dark 生成 fallback colors）。

use serde::Serialize;

/// Issue #727 评论 5757225958 问题4: 最终主题颜色值（hex 字符串 "#RRGGBB"）。
///
/// 所有字段为 hex 字符串，QML 侧经 `Qt.color(value)` 唯一边界转成 `color` 类型。
/// Rust 侧一次生成最终值（从 scheme 或 fallback），QML 侧只读不 fallback。
#[derive(Clone, Debug, Serialize)]
pub struct ResolvedThemeUiColors {
    pub primary: String,
    pub on_primary: String,
    pub primary_container: String,
    pub on_primary_container: String,
    pub secondary: String,
    pub on_secondary: String,
    pub secondary_container: String,
    pub on_secondary_container: String,
    pub tertiary: String,
    pub on_tertiary: String,
    pub tertiary_container: String,
    pub on_tertiary_container: String,
    pub background: String,
    pub on_background: String,
    pub surface: String,
    pub on_surface: String,
    pub surface_variant: String,
    pub on_surface_variant: String,
    pub surface_tint: String,
    pub surface_dim: String,
    pub surface_bright: String,
    pub surface_container_lowest: String,
    pub surface_container_low: String,
    pub surface_container: String,
    pub surface_container_high: String,
    pub surface_container_highest: String,
    pub inverse_surface: String,
    pub inverse_on_surface: String,
    pub inverse_primary: String,
    pub error: String,
    pub on_error: String,
    pub error_container: String,
    pub on_error_container: String,
    pub outline: String,
    pub outline_variant: String,
    pub scrim: String,
}

/// Issue #727 评论 5757225958 问题4: 统一主题 UI 快照。
///
/// Rust 侧一次性生成最终颜色值，QML 侧只读不 fallback。
/// `colors` 始终存在（scheme 为 None 时用 is_dark fallback），
/// 不再用 `Option<Value>`。
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
    /// 最终颜色值。始终存在（scheme 为 None 时用 is_dark fallback）。
    pub colors: ResolvedThemeUiColors,
}
