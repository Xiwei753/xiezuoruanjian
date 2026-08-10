//! #610：布局契约纯函数测试（从 layout_contract.rs 拆出，避免生产文件测试膨胀）。
//!
//! 覆盖：窗口能力 → 壳层/面板模式/一级导航 的全部决策规则，
//! 以及分隔式折叠降级、键盘/触控隐藏一级导航等特殊规则。
//! 纯函数测试，不触碰 UI / 平台 API / 文件系统。

use super::layout_contract::*;

fn default_capabilities() -> WindowCapabilities {
    WindowCapabilities::default()
}

#[test]
fn test_single_pane_capabilities() {
    let caps = default_capabilities();
    let contract = resolve_layout(&caps);
    assert_eq!(contract.shell_mode, ShellMode::SinglePane);
    assert_eq!(contract.workspace_pane_mode, WorkspacePaneMode::SinglePane);
    assert!(contract.show_primary_navigation);
}

#[test]
fn test_two_pane_capabilities() {
    let caps = WindowCapabilities {
        available_pane_count: 2,
        ..default_capabilities()
    };
    let contract = resolve_layout(&caps);
    assert_eq!(contract.shell_mode, ShellMode::TwoPane);
    assert_eq!(contract.workspace_pane_mode, WorkspacePaneMode::ListDetail);
    assert!(contract.show_primary_navigation);
}

#[test]
fn test_three_pane_capabilities() {
    let caps = WindowCapabilities {
        available_pane_count: 3,
        ..default_capabilities()
    };
    let contract = resolve_layout(&caps);
    assert_eq!(contract.shell_mode, ShellMode::ThreePane);
    assert_eq!(contract.workspace_pane_mode, WorkspacePaneMode::ThreePane);
}

#[test]
fn test_separating_fold_downgrades_shell() {
    let caps = WindowCapabilities {
        available_pane_count: 2,
        has_separating_fold: true,
        ..default_capabilities()
    };
    let contract = resolve_layout(&caps);
    assert_eq!(contract.shell_mode, ShellMode::SupportingPane);
    // 折叠只影响壳层，不改变产品栏的角色组合。
    assert_eq!(contract.workspace_pane_mode, WorkspacePaneMode::ListDetail);
}

#[test]
fn test_keyboard_hides_primary_navigation_on_touch_single_pane() {
    let caps = WindowCapabilities {
        available_pane_count: 1,
        pointer_class: PointerClass::Touch,
        keyboard_visible: true,
        ..default_capabilities()
    };
    let contract = resolve_layout(&caps);
    assert!(!contract.show_primary_navigation);
}

#[test]
fn test_keyboard_keeps_primary_navigation_for_mouse() {
    let caps = WindowCapabilities {
        available_pane_count: 1,
        pointer_class: PointerClass::Mouse,
        keyboard_visible: true,
        ..default_capabilities()
    };
    let contract = resolve_layout(&caps);
    assert!(contract.show_primary_navigation);
}

#[test]
fn test_keyboard_keeps_primary_navigation_on_multi_pane() {
    let caps = WindowCapabilities {
        available_pane_count: 2,
        pointer_class: PointerClass::Touch,
        keyboard_visible: true,
        ..default_capabilities()
    };
    let contract = resolve_layout(&caps);
    assert!(contract.show_primary_navigation);
}

#[test]
fn test_zero_pane_count_falls_back_to_single() {
    let caps = WindowCapabilities {
        available_pane_count: 0,
        ..default_capabilities()
    };
    let contract = resolve_layout(&caps);
    assert_eq!(contract.shell_mode, ShellMode::SinglePane);
}

#[test]
fn test_default_capabilities() {
    let caps = WindowCapabilities::default();
    assert_eq!(caps.available_pane_count, 1);
    assert!(!caps.has_separating_fold);
    assert_eq!(caps.pointer_class, PointerClass::Touch);
    assert!(!caps.keyboard_visible);
}
