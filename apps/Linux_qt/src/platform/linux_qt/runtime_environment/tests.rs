//! runtime_environment.rs 的单元测试。
//!
//! 按仓库结构门禁（`tools/check_source_structure.py` 的
//! production-test-bloat 规则）从生产文件拆出，测试逻辑保持不变。
use super::*;

fn inputs(xdg: Option<&str>, wayland_display: bool) -> EnvInputs {
    EnvInputs {
        xdg_session_type: xdg.map(String::from),
        wayland_display_present: wayland_display,
        user_qpa_platform: None,
        user_im_module: None,
        user_im_modules: None,
    }
}

// ── is_wayland_session ──

#[test]
fn wayland_detected_by_xdg_session_type() {
    assert!(is_wayland_session(&inputs(Some("wayland"), false)));
}

#[test]
fn wayland_detected_by_wayland_display_present() {
    assert!(is_wayland_session(&inputs(None, true)));
}

#[test]
fn wayland_detected_by_both_signals() {
    assert!(is_wayland_session(&inputs(Some("wayland"), true)));
}

#[test]
fn not_wayland_when_x11_session() {
    assert!(!is_wayland_session(&inputs(Some("x11"), false)));
}

#[test]
fn not_wayland_when_nothing_set() {
    assert!(!is_wayland_session(&inputs(None, false)));
}

// ── decide_qpa_platform ──

#[test]
fn qpa_wayland_when_wayland_session_and_unset() {
    let i = inputs(Some("wayland"), false);
    assert_eq!(decide_qpa_platform(&i, true), Some("wayland"));
}

#[test]
fn qpa_none_when_user_already_set() {
    let mut i = inputs(Some("wayland"), false);
    i.user_qpa_platform = Some("xcb".to_string());
    assert_eq!(decide_qpa_platform(&i, true), None);
}

#[test]
fn qpa_none_when_not_wayland() {
    let i = inputs(Some("x11"), false);
    assert_eq!(decide_qpa_platform(&i, false), None);
}

#[test]
fn qpa_none_when_wayland_but_user_set_wayland() {
    let mut i = inputs(Some("wayland"), false);
    i.user_qpa_platform = Some("wayland".to_string());
    assert_eq!(decide_qpa_platform(&i, true), None);
}

// ── decide_im_module ──

#[test]
fn im_wayland_fcitx5_uses_modules_with_wayland_first() {
    let i = inputs(Some("wayland"), false);
    assert_eq!(
        decide_im_module(&i, true, InputMethodFramework::Fcitx5),
        Some(ImModuleSetting::Modules("wayland;fcitx;ibus".to_string()))
    );
}

#[test]
fn im_wayland_ibus_uses_modules_with_wayland_first() {
    let i = inputs(Some("wayland"), false);
    assert_eq!(
        decide_im_module(&i, true, InputMethodFramework::Ibus),
        Some(ImModuleSetting::Modules("wayland;ibus".to_string()))
    );
}

#[test]
fn im_wayland_no_framework_relies_on_wayland_text_input() {
    let i = inputs(Some("wayland"), false);
    assert_eq!(decide_im_module(&i, true, InputMethodFramework::None), None);
}

#[test]
fn im_x11_fcitx5_uses_single_module() {
    let i = inputs(Some("x11"), false);
    assert_eq!(
        decide_im_module(&i, false, InputMethodFramework::Fcitx5),
        Some(ImModuleSetting::Module("fcitx".to_string()))
    );
}

#[test]
fn im_x11_ibus_uses_single_module() {
    let i = inputs(Some("x11"), false);
    assert_eq!(
        decide_im_module(&i, false, InputMethodFramework::Ibus),
        Some(ImModuleSetting::Module("ibus".to_string()))
    );
}

#[test]
fn im_none_when_user_already_set_module() {
    let mut i = inputs(Some("wayland"), false);
    i.user_im_module = Some("fcitx".to_string());
    assert_eq!(
        decide_im_module(&i, true, InputMethodFramework::Fcitx5),
        None
    );
}

#[test]
fn im_none_when_user_already_set_modules() {
    let mut i = inputs(Some("wayland"), false);
    i.user_im_modules = Some("wayland;fcitx".to_string());
    assert_eq!(
        decide_im_module(&i, true, InputMethodFramework::Fcitx5),
        None
    );
}

#[test]
fn im_none_when_no_framework_and_not_wayland() {
    let i = inputs(Some("x11"), false);
    assert_eq!(
        decide_im_module(&i, false, InputMethodFramework::None),
        None
    );
}

// ── user_forced_non_wayland_on_wayland ──

#[test]
fn user_forced_xcb_detected_on_wayland() {
    let mut i = inputs(Some("wayland"), false);
    i.user_qpa_platform = Some("xcb".to_string());
    assert!(user_forced_non_wayland_on_wayland(&i, true));
}

#[test]
fn user_forced_not_detected_when_wayland_set() {
    let mut i = inputs(Some("wayland"), false);
    i.user_qpa_platform = Some("wayland".to_string());
    assert!(!user_forced_non_wayland_on_wayland(&i, true));
}

#[test]
fn user_forced_not_detected_when_wayland_in_fallback_list() {
    let mut i = inputs(Some("wayland"), false);
    // QT_QPA_PLATFORM=wayland;xcb 包含 wayland，不算强制非 wayland
    i.user_qpa_platform = Some("wayland;xcb".to_string());
    assert!(!user_forced_non_wayland_on_wayland(&i, true));
}

#[test]
fn user_forced_not_detected_when_not_wayland_session() {
    let mut i = inputs(Some("x11"), false);
    i.user_qpa_platform = Some("xcb".to_string());
    assert!(!user_forced_non_wayland_on_wayland(&i, false));
}

#[test]
fn user_forced_not_detected_when_unset() {
    let i = inputs(Some("wayland"), false);
    assert!(!user_forced_non_wayland_on_wayland(&i, true));
}

// ── RuntimeEnvConfig::summary ──

#[test]
fn summary_contains_key_fields() {
    let config = RuntimeEnvConfig {
        wayland_session: true,
        detected_im_framework: InputMethodFramework::Fcitx5,
        set_qpa_platform: true,
        requested_qpa: None,
        configured_qpa: Some("wayland".to_string()),
        set_im_module: true,
        im_module_var: ImModuleVar::Modules,
        im_module_value: Some("wayland;fcitx;ibus".to_string()),
        user_forced_non_wayland_on_wayland: false,
    };
    let s = config.summary();
    assert!(s.contains("wayland=true"), "summary: {s}");
    assert!(s.contains("Fcitx5"), "summary: {s}");
    assert!(s.contains("setQpa=true"), "summary: {s}");
    assert!(s.contains("imVar=Modules"), "summary: {s}");
}

#[test]
fn default_config_summary_is_valid() {
    let config = RuntimeEnvConfig::default();
    let s = config.summary();
    assert!(s.contains("wayland=false"), "summary: {s}");
    assert!(s.contains("imFramework=None"), "summary: {s}");
}

// Issue #736 评论 5777408243 问题3: requested_qpa / configured_qpa 语义区分测试
#[test]
fn summary_distinguishes_requested_and_configured_qpa() {
    let config = RuntimeEnvConfig {
        wayland_session: true,
        detected_im_framework: InputMethodFramework::None,
        set_qpa_platform: false,
        requested_qpa: Some("xcb".to_string()),
        configured_qpa: None,
        set_im_module: false,
        im_module_var: ImModuleVar::None,
        im_module_value: None,
        user_forced_non_wayland_on_wayland: true,
    };
    let s = config.summary();
    assert!(s.contains("requestedQpa=Some(\"xcb\")"), "summary: {s}");
    assert!(s.contains("configuredQpa=None"), "summary: {s}");
    assert!(s.contains("userForcedNonWayland=true"), "summary: {s}");
}
