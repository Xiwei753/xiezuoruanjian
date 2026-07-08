//! Linux Qt 平台适配层
//!
//! 将 Qt InputMethod / fcitx5 / ibus / Wayland / QClipboard 等系统交互
//! 收敛到此模块，SujianEditorItem 和 QML 不直接关心平台细节。
//!
//! 实现 writer_core::platform_interaction 定义的五大适配器 trait：
//! - TextInputAdapter
//! - CursorAnchorAdapter
//! - AnimationDriver
//! - PlatformCapabilities
//! - ClipboardAndFocusAdapter

pub mod text_input_adapter;
pub mod cursor_anchor_adapter;
pub mod animation_driver_adapter;
pub mod capabilities_adapter;
pub mod clipboard_focus_adapter;
pub mod ime_platform;
pub mod utf16_converter;

pub use text_input_adapter::LinuxQtTextInputAdapter;
pub use cursor_anchor_adapter::LinuxQtCursorAnchorAdapter;
pub use animation_driver_adapter::LinuxQtAnimationDriver;
pub use capabilities_adapter::LinuxQtCapabilitiesAdapter;
pub use clipboard_focus_adapter::LinuxQtClipboardFocusAdapter;
