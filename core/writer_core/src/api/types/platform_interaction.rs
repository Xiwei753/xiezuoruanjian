//! Platform capabilities DTO for cross-platform FFI.

use serde::{Deserialize, Serialize};

/// 平台能力集合 DTO — 启动时由平台适配层一次性报告
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformCapabilitiesDto {
    pub supports_ime_preedit: bool,
    pub supports_cursor_anchor: bool,
    pub supports_replacement_commit: bool,
    pub supports_text_animation: bool,
    pub supports_smooth_cursor: bool,
    pub supports_reflow_animation: bool,
    pub supports_clipboard: bool,
    pub supports_context_menu: bool,
}

impl From<crate::platform_interaction::PlatformCapabilities> for PlatformCapabilitiesDto {
    fn from(caps: crate::platform_interaction::PlatformCapabilities) -> Self {
        Self {
            supports_ime_preedit: caps.supports_ime_preedit,
            supports_cursor_anchor: caps.supports_cursor_anchor,
            supports_replacement_commit: caps.supports_replacement_commit,
            supports_text_animation: caps.supports_text_animation,
            supports_smooth_cursor: caps.supports_smooth_cursor,
            supports_reflow_animation: caps.supports_reflow_animation,
            supports_clipboard: caps.supports_clipboard,
            supports_context_menu: caps.supports_context_menu,
        }
    }
}

/// 平台标识 DTO
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlatformKindDto {
    LinuxQt,
    Android,
    Windows,
    Harmony,
    Unknown,
}

impl From<crate::platform_interaction::PlatformKind> for PlatformKindDto {
    fn from(kind: crate::platform_interaction::PlatformKind) -> Self {
        match kind {
            crate::platform_interaction::PlatformKind::LinuxQt => Self::LinuxQt,
            crate::platform_interaction::PlatformKind::Android => Self::Android,
            crate::platform_interaction::PlatformKind::Windows => Self::Windows,
            crate::platform_interaction::PlatformKind::Harmony => Self::Harmony,
            crate::platform_interaction::PlatformKind::Unknown => Self::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dto_from_linux_qt_capabilities() {
        let caps = crate::platform_interaction::PlatformCapabilities::linux_qt();
        let dto: PlatformCapabilitiesDto = caps.into();
        assert!(dto.supports_ime_preedit);
        assert!(dto.supports_cursor_anchor);
        assert!(dto.supports_replacement_commit);
        assert!(dto.supports_text_animation);
    }

    #[test]
    fn dto_from_harmony_capabilities() {
        let caps = crate::platform_interaction::PlatformCapabilities::harmony();
        let dto: PlatformCapabilitiesDto = caps.into();
        assert!(!dto.supports_text_animation);
        assert!(!dto.supports_smooth_cursor);
        assert!(dto.supports_clipboard);
    }

    #[test]
    fn dto_serializes_camel_case() {
        let caps = crate::platform_interaction::PlatformCapabilities::linux_qt();
        let dto: PlatformCapabilitiesDto = caps.into();
        let json = serde_json::to_string(&dto).unwrap();
        assert!(json.contains("\"supportsImePreedit\":"));
        assert!(json.contains("\"supportsTextAnimation\":"));
    }
}
