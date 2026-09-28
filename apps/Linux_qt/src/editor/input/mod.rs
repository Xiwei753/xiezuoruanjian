//! Linux Qt 输入层三层架构
//!
//! ┌──────────────────────────────────────────────────────────────────────┐
//! │ Layer 1: QtInputSurface (C++ SujianEventFilter)                     │
//! │   - Qt 官方事件入口：keyPressEvent / inputMethodEvent / query       │
//! │   - 不写正文业务，不写动画逻辑                                      │
//! │   - 委托给 Linux PlatformImeAdapter 处理 fcitx5/ibus 语义             │
//! ├──────────────────────────────────────────────────────────────────────┤
//! │ Layer 2: Linux PlatformImeAdapter (C++ 内嵌)                         │
//! │   - LinuxImeAdapter: 直接插入，不延迟，按 Qt inputMethodEvent 语义  │
//! ├──────────────────────────────────────────────────────────────────────┤
//! │ Layer 3: EditorInputController (Rust)                               │
//! │   - 接收归一化输入事件：PlainText / Shortcut / Preedit / Commit 等  │
//! │   - 调用 SujianEditorItem / EditorEngine 修改正文和生成视觉事务     │
//! │   - Linux IME 语义只存在 Layer 2，正文编辑和动画不关心具体输入法    │
//! └──────────────────────────────────────────────────────────────────────┘

pub mod controller;
pub mod events;
pub mod platform;
pub mod platform_ime;
pub mod qt_surface;

pub(crate) use controller::{
    cancel_preedit, commit_preedit_text, handle_key, insert_preedit_text, EditorInputHost,
};
pub(crate) use qt_surface::{focus_item, install_event_filter};

#[cfg(test)]
mod tests;
