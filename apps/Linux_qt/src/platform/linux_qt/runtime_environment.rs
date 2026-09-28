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
///
/// Issue #736 评论 5777408243 问题3: 字段语义明确区分 requested/configured/actual：
/// - `requested_qpa`：用户显式设置的环境变量值（inputs.user_qpa_platform）
/// - `configured_qpa`：本函数设置的值（decide_qpa_platform 的结果）
/// - actual_qpa：不在这个模块设置（只能在 GUI application 建立以后从 Qt 读取），
///   留给 main.rs 的 DesktopRuntimeProfile。
///
/// 这个模块只负责"启动前环境决策"，不让任何字段冒充 Qt 最终实际平台。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeEnvConfig {
    /// 是否检测到 Wayland 会话
    pub wayland_session: bool,
    /// 检测到的输入法框架
    pub detected_im_framework: InputMethodFramework,
    /// 是否由本函数设置了 `QT_QPA_PLATFORM`（true=本次设置，false=用户已设或未设）
    pub set_qpa_platform: bool,
    /// 用户显式设置的环境变量值（inputs.user_qpa_platform）
    pub requested_qpa: Option<String>,
    /// 本函数设置的值（decide_qpa_platform 的结果）
    pub configured_qpa: Option<String>,
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
            "wayland={} imFramework={:?} setQpa={} requestedQpa={:?} configuredQpa={:?} setIm={} imVar={:?} im={:?} userForcedNonWayland={}",
            self.wayland_session,
            self.detected_im_framework,
            self.set_qpa_platform,
            self.requested_qpa,
            self.configured_qpa,
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
        // Issue #736 评论 5777408243 问题3: requested_qpa 记录用户显式设置值
        requested_qpa: inputs.user_qpa_platform.clone(),
        ..Default::default()
    };

    // QPA platform：Wayland 会话且用户未设时补 wayland，拒绝默认回退 xcb
    if let Some(platform) = decide_qpa_platform(&inputs, wayland) {
        // SAFETY: 在 main 最早期、单线程、Qt 初始化前调用，无并发读 env 风险。
        std::env::set_var("QT_QPA_PLATFORM", platform);
        config.set_qpa_platform = true;
        config.configured_qpa = Some(platform.to_string());
    }
    // Issue #736 评论 5777408243 问题3: requested_qpa 已在构造时记录用户显式设置值，
    // 不再把用户值混进 configured_qpa。configured_qpa 只记本函数设置的值。

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
mod tests;
