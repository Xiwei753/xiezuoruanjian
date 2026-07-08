//! IME 平台检测 — fcitx5/ibus/Wayland 环境判断收敛到此
//!
//! QML 和 SujianEditorItem 不直接关心 fcitx/ibus，
//! 只通过 LinuxQtTextInputAdapter 获取 IME 状态。

use std::cell::Cell;

/// Linux IME 平台检测结果
#[derive(Debug, Clone, Default)]
pub struct ImePlatformInfo {
    pub is_wayland: bool,
    pub is_fcitx5: bool,
    pub is_ibus: bool,
    pub ime_detected: bool,
}

impl ImePlatformInfo {
    /// 平台名称字符串
    pub fn platform_name(&self) -> &'static str {
        if self.is_wayland && self.is_fcitx5 {
            return "wayland_fcitx5";
        }
        if self.is_wayland && self.is_ibus {
            return "wayland_ibus";
        }
        if self.is_fcitx5 {
            return "x11_fcitx5";
        }
        if self.is_ibus {
            return "x11_ibus";
        }
        if self.is_wayland && !self.ime_detected {
            return "wayland_unknown";
        }
        if self.is_wayland {
            return "wayland";
        }
        if !self.ime_detected {
            return "linux_unknown";
        }
        "linux"
    }
}

/// IME 平台检测状态
pub struct ImePlatformDetector {
    info: ImePlatformInfo,
    ime_composing: Cell<bool>,
    detected: Cell<bool>,
}

impl Default for ImePlatformDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl ImePlatformDetector {
    pub fn new() -> Self {
        Self {
            info: ImePlatformInfo::default(),
            ime_composing: Cell::new(false),
            detected: Cell::new(false),
        }
    }

    /// 执行平台检测（从环境变量读取）
    ///
    /// 实际的 C++ PlatformImeAdapter::detect_platform() 通过 FFI 调用，
    /// 此处提供 Rust 侧的检测结果缓存。
    pub fn detect_from_env(&mut self) {
        if self.detected.get() {
            return;
        }
        let xdg_session = std::env::var("XDG_SESSION_TYPE")
            .unwrap_or_default()
            .to_lowercase();
        let qt_im_module = std::env::var("QT_IM_MODULE")
            .unwrap_or_default()
            .to_lowercase();
        let qt_im_modules = std::env::var("QT_IM_MODULES")
            .unwrap_or_default()
            .to_lowercase();

        self.info.is_wayland = xdg_session == "wayland";
        self.info.is_fcitx5 = qt_im_module.contains("fcitx") || qt_im_modules.contains("fcitx");
        self.info.is_ibus = qt_im_module.contains("ibus") || qt_im_modules.contains("ibus");
        self.info.ime_detected = self.info.is_fcitx5 || self.info.is_ibus;
        self.detected.set(true);
    }

    /// 从 C++ PlatformImeAdapter 同步检测结果
    pub fn set_detected_info(&mut self, info: ImePlatformInfo) {
        self.info = info;
        self.detected.set(true);
    }

    pub fn info(&self) -> &ImePlatformInfo {
        &self.info
    }

    pub fn is_ime_composing(&self) -> bool {
        self.ime_composing.get()
    }

    pub fn set_ime_composing(&self, composing: bool) {
        self.ime_composing.set(composing);
    }

    pub fn can_accept_plain_text_key(&self) -> bool {
        !self.ime_composing.get()
    }
}
