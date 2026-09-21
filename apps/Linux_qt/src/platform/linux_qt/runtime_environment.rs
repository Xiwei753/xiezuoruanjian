//! Issue #729 评论 5762596831 第 1 部分：RPM 收口到原生 Wayland 运行环境。
//!
//! 统一的 QPA / 输入法环境设置逻辑，在 Qt 初始化前调用一次。
//!
//! ## 设计
//! - 检测 Wayland 会话（`XDG_SESSION_TYPE=wayland` 或 `WAYLAND_DISPLAY` 非空）。
//! - Wayland 可用且用户未显式设置 `QT_QPA_PLATFORM` 时，设为 `wayland`，
//!   拒绝默认回退到 xcb/XWayland。
//! - 检测 fcitx5 / ibus 输入法框架，设置 `QT_IM_MODULE` / `QT_IM_MODULES`。
//!   Wayland 会话下优先用 Wayland text-input 协议
//!   （`QT_IM_MODULES=wayland;fcitx;ibus`，wayland 在前）。
//! - 幂等：只在环境变量未设置时补默认，不覆盖用户显式设置。
//!
//! ## 可测性
//! 决策逻辑（`is_wayland_session` / `decide_*`）是纯函数，接受 [`EnvInputs`]
//! 和 [`InputMethodFramework`]，不直接读 env、不调外部命令，可在单测中覆盖。
//! env 采集（[`EnvInputs::from_env`]）和 IM 框架探测
//! （[`detect_im_framework`]）是副作用函数，不在单测中覆盖。

use std::process::Command;

/// 输入法框架探测结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMethodFramework {
    /// 未检测到 fcitx5 / ibus
    #[default]
    None,
    /// fcitx5
    Fcitx5,
    /// ibus
    Ibus,
}

/// IM module 设置方式（决定写哪个环境变量）。
#[derive(Debug, Clone, PartialEq, Eq)]
enum ImModuleSetting {
    /// 写 `QT_IM_MODULE`（单值，非 Wayland 会话）
    Module(String),
    /// 写 `QT_IM_MODULES`（分号分隔多值，Wayland 会话，优先 Wayland text-input 协议）
    Modules(String),
}

/// 写入的 IM 环境变量名（供日志区分）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImModuleVar {
    /// 未设置
    #[default]
    None,
    /// `QT_IM_MODULE`
    Module,
    /// `QT_IM_MODULES`
    Modules,
}

/// 运行环境配置结果，供 main.rs 记日志。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeEnvConfig {
    /// 是否检测到 Wayland 会话
    pub wayland_session: bool,
    /// 检测到的输入法框架
    pub detected_im_framework: InputMethodFramework,
    /// 是否由本函数设置了 `QT_QPA_PLATFORM`（true=本次设置，false=用户已设或未设）
    pub set_qpa_platform: bool,
    /// 最终生效的 `QT_QPA_PLATFORM` 值（用户设置或本次设置）
    pub qpa_platform_value: Option<String>,
    /// 是否由本函数设置了 IM module
    pub set_im_module: bool,
    /// 写入的 IM 环境变量
    pub im_module_var: ImModuleVar,
    /// 最终生效的 IM module 值
    pub im_module_value: Option<String>,
    /// Wayland 会话下用户显式设了非 wayland 平台（警告，未覆盖）
    pub user_forced_non_wayland_on_wayland: bool,
}

impl RuntimeEnvConfig {
    /// 简短摘要，供 `debug_log_static` 记一行日志。
    pub fn summary(&self) -> String {
        format!(
            "wayland={} imFramework={:?} setQpa={} qpa={:?} setIm={} imVar={:?} im={:?} userForcedNonWayland={}",
            self.wayland_session,
            self.detected_im_framework,
            self.set_qpa_platform,
            self.qpa_platform_value,
            self.set_im_module,
            self.im_module_var,
            self.im_module_value,
            self.user_forced_non_wayland_on_wayland,
        )
    }
}

/// 从环境变量采集的输入（纯函数决策的输入）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct EnvInputs {
    xdg_session_type: Option<String>,
    wayland_display_present: bool,
    user_qpa_platform: Option<String>,
    user_im_module: Option<String>,
    user_im_modules: Option<String>,
}

impl EnvInputs {
    /// 从进程环境变量采集。只在 [`configure_qpa_and_input_method`] 中调用一次。
    fn from_env() -> Self {
        Self {
            xdg_session_type: std::env::var("XDG_SESSION_TYPE").ok(),
            wayland_display_present: std::env::var("WAYLAND_DISPLAY")
                .ok()
                .filter(|s| !s.is_empty())
                .is_some(),
            user_qpa_platform: std::env::var("QT_QPA_PLATFORM").ok(),
            user_im_module: std::env::var("QT_IM_MODULE").ok(),
            user_im_modules: std::env::var("QT_IM_MODULES").ok(),
        }
    }
}

/// 判断是否为 Wayland 会话（纯函数）。
fn is_wayland_session(inputs: &EnvInputs) -> bool {
    inputs.xdg_session_type.as_deref() == Some("wayland") || inputs.wayland_display_present
}

/// 决定 `QT_QPA_PLATFORM` 设置值。
///
/// 返回 `Some("wayland")` 表示应设置（Wayland 会话且用户未显式设置）；
/// `None` 表示不设置（非 Wayland，或用户已显式设置）。
fn decide_qpa_platform(inputs: &EnvInputs, wayland: bool) -> Option<&'static str> {
    if wayland && inputs.user_qpa_platform.is_none() {
        Some("wayland")
    } else {
        None
    }
}

/// 判断用户是否在 Wayland 会话下显式强制了非 wayland 平台（如 xcb/XWayland）。
///
/// 仅用于日志告警，不覆盖用户设置。
fn user_forced_non_wayland_on_wayland(inputs: &EnvInputs, wayland: bool) -> bool {
    if !wayland {
        return false;
    }
    match &inputs.user_qpa_platform {
        Some(v) => !v.split(';').any(|p| p == "wayland"),
        None => false,
    }
}

/// 决定 IM module 设置值（纯函数）。
///
/// 用户已显式设置 `QT_IM_MODULE` 或 `QT_IM_MODULES` 时返回 `None`（不覆盖）。
fn decide_im_module(
    inputs: &EnvInputs,
    wayland: bool,
    framework: InputMethodFramework,
) -> Option<ImModuleSetting> {
    // 幂等：用户已显式设置任一变量则不覆盖
    if inputs.user_im_module.is_some() || inputs.user_im_modules.is_some() {
        return None;
    }
    match (wayland, framework) {
        (true, InputMethodFramework::Fcitx5) => {
            Some(ImModuleSetting::Modules("wayland;fcitx;ibus".to_string()))
        }
        (true, InputMethodFramework::Ibus) => {
            Some(ImModuleSetting::Modules("wayland;ibus".to_string()))
        }
        (false, InputMethodFramework::Fcitx5) => Some(ImModuleSetting::Module("fcitx".to_string())),
        (false, InputMethodFramework::Ibus) => Some(ImModuleSetting::Module("ibus".to_string())),
        // Wayland 但无 IM 框架：依赖 Qt Wayland 原生 text-input 协议，不设 IM module。
        // 非 Wayland 且无 IM 框架：不设。
        _ => None,
    }
}

/// 探测当前可用的输入法框架。先 fcitx5 再 ibus。
///
/// 通过 `pgrep -x` 检测进程是否在运行（与 start.sh 逻辑一致）。
/// `pgrep` 不存在或执行失败时视为未检测到，返回 [`InputMethodFramework::None`]。
fn detect_im_framework() -> InputMethodFramework {
    if process_running("fcitx5") {
        return InputMethodFramework::Fcitx5;
    }
    if process_running("ibus-daemon") {
        return InputMethodFramework::Ibus;
    }
    InputMethodFramework::None
}

/// 检测指定进程是否在运行（`pgrep -x <name>`）。
///
/// 命令不存在或非零退出均视为未运行。不使用 `unwrap`/`expect` 处理外部命令错误。
fn process_running(name: &str) -> bool {
    Command::new("pgrep")
        .arg("-x")
        .arg(name)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// 配置 QPA 平台和输入法环境变量。在 Qt 初始化前调用一次。
///
/// - 幂等：只在环境变量未设置时补默认，不覆盖用户显式设置。
/// - Wayland 可用时设 `QT_QPA_PLATFORM=wayland`，拒绝默认回退 xcb/XWayland。
/// - 检测 fcitx5/ibus 设 `QT_IM_MODULE`/`QT_IM_MODULES`，Wayland 下优先
///   Wayland text-input 协议。
///
/// 返回 [`RuntimeEnvConfig`]，供 main.rs 记日志。
pub fn configure_qpa_and_input_method() -> RuntimeEnvConfig {
    let inputs = EnvInputs::from_env();
    let wayland = is_wayland_session(&inputs);
    let framework = detect_im_framework();
    let forced_non_wayland = user_forced_non_wayland_on_wayland(&inputs, wayland);

    let mut config = RuntimeEnvConfig {
        wayland_session: wayland,
        detected_im_framework: framework,
        user_forced_non_wayland_on_wayland: forced_non_wayland,
        ..Default::default()
    };

    // QPA platform：Wayland 会话且用户未设时补 wayland，拒绝默认回退 xcb
    if let Some(platform) = decide_qpa_platform(&inputs, wayland) {
        // SAFETY: 在 main 最早期、单线程、Qt 初始化前调用，无并发读 env 风险。
        std::env::set_var("QT_QPA_PLATFORM", platform);
        config.set_qpa_platform = true;
        config.qpa_platform_value = Some(platform.to_string());
    } else if let Some(user) = &inputs.user_qpa_platform {
        // 用户显式设置，记录但不覆盖
        config.qpa_platform_value = Some(user.clone());
    }

    // IM module
    if let Some(setting) = decide_im_module(&inputs, wayland, framework) {
        match setting {
            ImModuleSetting::Module(v) => {
                // SAFETY: 同上，main 最早期单线程调用。
                std::env::set_var("QT_IM_MODULE", &v);
                config.set_im_module = true;
                config.im_module_var = ImModuleVar::Module;
                config.im_module_value = Some(v);
            }
            ImModuleSetting::Modules(v) => {
                // SAFETY: 同上，main 最早期单线程调用。
                std::env::set_var("QT_IM_MODULES", &v);
                config.set_im_module = true;
                config.im_module_var = ImModuleVar::Modules;
                config.im_module_value = Some(v);
            }
        }
    } else {
        // 用户已设或无需设置：记录用户已设的值供日志
        if let Some(v) = &inputs.user_im_module {
            config.im_module_var = ImModuleVar::Module;
            config.im_module_value = Some(v.clone());
        } else if let Some(v) = &inputs.user_im_modules {
            config.im_module_var = ImModuleVar::Modules;
            config.im_module_value = Some(v.clone());
        }
    }

    config
}

#[cfg(test)]
mod tests {
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
            qpa_platform_value: Some("wayland".to_string()),
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
}
