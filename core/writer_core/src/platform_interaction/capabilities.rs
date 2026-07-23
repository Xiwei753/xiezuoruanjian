//! PlatformCapabilities — 平台能力报告
//!
//! 本模块从 `writer_platform_api` 重新导出平台能力契约，
//! 保持 `writer_core` 内部模块路径的兼容性。
//!
//! 平台能力定义已迁移至 `writer_platform_api::PlatformCapabilities`，
//! 新增平台时只需在 `writer_platform_api::PlatformKind` 添加变体
//! 并在 `PlatformCapabilitiesExt` 实现对应的 `default_capabilities`，
//! 无需修改项目、章节、星图、统计等业务模块。

pub use writer_platform_api::{PlatformCapabilities, PlatformCapabilitiesExt};

pub type PlatformKind = writer_platform_api::PlatformKind;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_capabilities_reflect_real_ime() {
        let caps = PlatformCapabilities::desktop();
        assert!(caps.supports_ime_preedit);
        assert!(caps.supports_replacement_commit);
        assert!(caps.supports_text_animation);
        assert!(caps.supports_smooth_cursor);
        assert!(caps.supports_reflow_animation);
        assert!(caps.supports_clipboard);
        assert!(caps.supports_context_menu);
        assert!(caps.supports_cursor_anchor);
    }

    #[test]
    fn harmony_has_no_animation() {
        let caps = PlatformCapabilities::harmony();
        assert!(!caps.supports_text_animation);
        assert!(!caps.supports_smooth_cursor);
        assert!(!caps.supports_reflow_animation);
        assert!(!caps.supports_ime_preedit);
        assert!(caps.supports_clipboard);
        assert!(caps.supports_context_menu);
    }

    #[test]
    fn android_capabilities_reflect_real_ime() {
        let caps = PlatformCapabilities::android();
        assert!(caps.supports_ime_preedit);
        assert!(caps.supports_cursor_anchor);
        assert!(caps.supports_replacement_commit);
        assert!(caps.has_any_animation_support());
    }

    #[test]
    fn windows_capabilities_reflect_real_ime() {
        let caps = PlatformCapabilities::windows();
        assert!(caps.supports_ime_preedit);
        assert!(caps.supports_cursor_anchor);
        assert!(!caps.supports_replacement_commit);
        assert!(!caps.has_any_animation_support());
        assert!(!caps.supports_context_menu);
    }

    #[test]
    fn platform_kind_default_capabilities() {
        use super::PlatformCapabilitiesExt;
        assert!(writer_platform_api::PlatformKind::Desktop.default_capabilities().supports_cursor_anchor);
        assert!(writer_platform_api::PlatformKind::Android.default_capabilities().supports_replacement_commit);
        assert!(!writer_platform_api::PlatformKind::Harmony.default_capabilities().supports_text_animation);
    }

    #[test]
    fn has_any_animation_support() {
        assert!(PlatformCapabilities::desktop().has_any_animation_support());
        assert!(!PlatformCapabilities::harmony().has_any_animation_support());
        assert!(!PlatformCapabilities::minimal().has_any_animation_support());
    }

    #[test]
    fn capabilities_serialize_camel_case() {
        let caps = PlatformCapabilities::desktop();
        let json = serde_json::to_string(&caps).unwrap();
        assert!(json.contains("\"supportsImePreedit\":"));
        assert!(json.contains("\"supportsCursorAnchor\":"));
        assert!(json.contains("\"supportsReplacementCommit\":"));
        assert!(json.contains("\"supportsTextAnimation\":"));
        assert!(json.contains("\"supportsSmoothCursor\":"));
        assert!(json.contains("\"supportsReflowAnimation\":"));
        assert!(json.contains("\"supportsClipboard\":"));
        assert!(json.contains("\"supportsContextMenu\":"));
    }
}
