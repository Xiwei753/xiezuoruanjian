//! Windows Qt 运行环境准备。
//!
//! Windows 平台由 Qt 的 `windows` QPA 插件和 TSF/IME 集成负责输入法，
//! 不需要像 Linux 那样在启动前设置 `QT_QPA_PLATFORM` / `QT_IM_MODULE`。
//! 这里保留与 Linux 完全一致的返回结构，便于共享 main/日志代码复用。

/// 输入法框架探测结果（Windows 固定使用 TSF/IME）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMethodFramework {
    #[default]
    None,
    Fcitx5,
    Ibus,
}

/// IM module 设置方式（Windows 不使用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImModuleSetting {
    Module(String),
    Modules(String),
}

/// 写入的 IM 环境变量名（Windows 不使用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImModuleVar {
    #[default]
    None,
    Module,
    Modules,
}

/// 运行环境配置结果，字段语义与 Linux 版本保持一致。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeEnvConfig {
    pub wayland_session: bool,
    pub detected_im_framework: InputMethodFramework,
    pub set_qpa_platform: bool,
    pub requested_qpa: Option<String>,
    pub configured_qpa: Option<String>,
    pub set_im_module: bool,
    pub im_module_var: ImModuleVar,
    pub im_module_value: Option<String>,
    pub user_forced_non_wayland_on_wayland: bool,
}

impl RuntimeEnvConfig {
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

/// Windows 不需要在启动前改写 Qt 平台或输入法环境变量。
pub fn configure_qpa_and_input_method() -> RuntimeEnvConfig {
    RuntimeEnvConfig::default()
}
