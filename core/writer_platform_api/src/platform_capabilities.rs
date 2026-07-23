//! PlatformCapabilities — 平台能力报告
//!
//! 启动时报告平台能力，设置页和前端按钮按 capabilities 显示/禁用。
//! 鸿蒙未实现动画就禁用动画，Android 不支持 replacement commit 就不暴露。
//!
//! 本模块属于平台能力契约，由 `writer_platform_api` 统一定义，
//! `writer_core` 和平台适配层只消费这些契约。

use crate::PlatformKind;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformCapabilities {
    pub supports_ime_preedit: bool,
    pub supports_cursor_anchor: bool,
    pub supports_replacement_commit: bool,
    pub supports_text_animation: bool,
    pub supports_smooth_cursor: bool,
    pub supports_reflow_animation: bool,
    pub supports_clipboard: bool,
    pub supports_context_menu: bool,
}

impl PlatformCapabilities {
    pub fn desktop() -> Self {
        Self {
            supports_ime_preedit: true,
            supports_cursor_anchor: true,
            supports_replacement_commit: true,
            supports_text_animation: true,
            supports_smooth_cursor: true,
            supports_reflow_animation: true,
            supports_clipboard: true,
            supports_context_menu: true,
        }
    }

    pub fn android() -> Self {
        Self {
            supports_ime_preedit: true,
            supports_cursor_anchor: true,
            supports_replacement_commit: true,
            supports_text_animation: true,
            supports_smooth_cursor: true,
            supports_reflow_animation: true,
            supports_clipboard: true,
            supports_context_menu: true,
        }
    }

    pub fn windows() -> Self {
        Self {
            supports_ime_preedit: true,
            supports_cursor_anchor: true,
            supports_replacement_commit: false,
            supports_text_animation: false,
            supports_smooth_cursor: false,
            supports_reflow_animation: false,
            supports_clipboard: true,
            supports_context_menu: false,
        }
    }

    pub fn harmony() -> Self {
        Self {
            supports_ime_preedit: false,
            supports_cursor_anchor: false,
            supports_replacement_commit: false,
            supports_text_animation: false,
            supports_smooth_cursor: false,
            supports_reflow_animation: false,
            supports_clipboard: true,
            supports_context_menu: true,
        }
    }

    pub fn minimal() -> Self {
        Self {
            supports_ime_preedit: false,
            supports_cursor_anchor: false,
            supports_replacement_commit: false,
            supports_text_animation: false,
            supports_smooth_cursor: false,
            supports_reflow_animation: false,
            supports_clipboard: false,
            supports_context_menu: false,
        }
    }

    pub fn has_any_animation_support(&self) -> bool {
        self.supports_text_animation
            || self.supports_smooth_cursor
            || self.supports_reflow_animation
    }

    pub fn has_any_ime_support(&self) -> bool {
        self.supports_ime_preedit
            || self.supports_cursor_anchor
            || self.supports_replacement_commit
    }
}

pub trait PlatformCapabilitiesExt {
    fn default_capabilities(&self) -> PlatformCapabilities;
}

impl PlatformCapabilitiesExt for PlatformKind {
    fn default_capabilities(&self) -> PlatformCapabilities {
        match self {
            Self::Desktop => PlatformCapabilities::desktop(),
            Self::Android => PlatformCapabilities::android(),
            Self::Windows => PlatformCapabilities::windows(),
            Self::Harmony => PlatformCapabilities::harmony(),
            Self::Apple => PlatformCapabilities::minimal(),
        }
    }
}
