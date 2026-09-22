//! Platform Interaction — 前端 UI 与系统交互的统一边界
//!
//! 本模块定义了前端写作区与平台系统服务之间的统一接口。
//! 前端只依赖这些 trait 和 DTO，不直接碰 Qt InputMethod、Android Input Connection、
//! Windows 输入服务、Harmony TextArea，也不直接判断 fcitx、ibus、Wayland、IMM、
//! 系统剪贴板、候选框位置。
//!
//! ## 四大适配器
//!
//! 1. `TextInputAdapter` — 归一化输入事件
//! 2. `CursorAnchorAdapter` — 光标/锚点/候选框定位
//! 3. `PlatformCapabilities` — 平台能力报告
//! 4. `ClipboardAndFocusAdapter` — 剪贴板/焦点/菜单
//!
//! 动画驱动已下沉各平台端，Core 不再提供 AnimationDriver。

pub mod capabilities;
pub mod clipboard_focus;
pub mod cursor_anchor;
pub mod text_input;

pub use capabilities::*;
pub use clipboard_focus::*;
pub use cursor_anchor::*;
pub use text_input::*;
