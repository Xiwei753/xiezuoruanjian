//! Issue #705 结构守卫 — 旧字段不能重新出现。
//!
//! 本测试只保留"旧字段不能重新出现"的结构守卫。能通过真实行为覆盖的
//! 测试已迁移到 qt_runtime_theme_cursor.rs 和 qt_runtime_cursor_geometry.rs。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

fn linux_qt_root() -> PathBuf {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set by cargo");
    PathBuf::from(manifest_dir)
}

fn read_src(rel: &str) -> String {
    let path = linux_qt_root().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e))
}

// =========================================================================
// 问题 1: 旧主题字段不能重新出现 — 结构守卫
// =========================================================================

/// 守卫 1a: Core `LocalSettings` 运行时不应再持有 `theme_mode` 字段。
/// `theme_mode` 只能用于加载旧设置时的一次性迁移。
#[test]
fn issue705_guard_1a_local_settings_no_runtime_theme_mode() {
    let src = read_src("../../core/writer_core/src/settings/mod.rs");
    let has_theme_mode = src.contains("pub theme_mode: Option<String>");
    assert!(
        !has_theme_mode,
        "LocalSettings 不应再持有运行时 theme_mode 字段"
    );
}

/// 守卫 1b: SettingsBackend 不应再暴露 `setting_theme_mode` QML 属性。
#[test]
fn issue705_guard_1b_settings_backend_no_theme_mode_property() {
    let src = read_src("src/backend/settings_backend.rs");
    let exposes = src.contains("setting_theme_mode: qt_property!");
    assert!(
        !exposes,
        "SettingsBackend 不应再暴露 setting_theme_mode QML 属性"
    );
}

/// 守卫 1c: AppBackend 不应再持有独立的 `current_setting_theme_mode` 字段。
#[test]
fn issue705_guard_1c_app_backend_no_independent_theme_mode_field() {
    let src = read_src("src/backend/app_backend.rs");
    let has_theme_mode = src.contains("current_setting_theme_mode: String");
    assert!(
        !has_theme_mode,
        "AppBackend 不应再持有独立的 current_setting_theme_mode 字段"
    );
}

// =========================================================================
// 问题 2: 旧光标结构不能重新出现 — 结构守卫
// =========================================================================

/// 守卫 2e: 鼠标点击路径不应有多个 per-method force_snap 分支。
#[test]
fn issue705_guard_2e_click_path_no_per_method_force_snap() {
    let src = read_src("src/sujian_editor_item/editing.rs");
    let force_snap_count =
        src.matches("self.cursor_ctrl.force_snap_next = true;").count();
    assert!(
        force_snap_count <= 1,
        "鼠标点击路径不应有多个 per-method force_snap 分支（发现 {} 处）",
        force_snap_count
    );
}
