//! Linux Qt 平台适配层
//!
//! 将 Qt InputMethod / fcitx5 / ibus / Wayland / QClipboard 等系统交互
//! 收敛到此模块，SujianEditorItem 和 QML 不直接关心平台细节。
//!
//! 实现 writer_core::platform_interaction 定义的适配器 trait：
//! - ClipboardAndFocusAdapter
//!
//! CursorAnchorAdapter 和 AnimationDriver 已被直接实现替代：
//! - IME query 数据通过 FFI 函数 sujian_get_ime_query_data 从 SujianEditorItem 内部状态读取
//! - 动画帧驱动通过 QQuickItem::update() 直接实现
//!
//! TextInputAdapter、ime_platform 和 capabilities_adapter 已被 SujianEditorItem 直接处理替代，
//! 不再需要独立的适配器层。

pub mod clipboard_focus_adapter;
pub mod utf16_converter;


pub use clipboard_focus_adapter::LinuxQtClipboardFocusAdapter;
