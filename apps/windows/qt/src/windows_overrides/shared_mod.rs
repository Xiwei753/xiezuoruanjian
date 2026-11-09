//! Windows 共享源码镜像根。
//!
//! 由 `build.rs` 从 `apps/Linux_qt/src` 生成到 `src/shared`，这里只声明模块。
//! 平台差异文件由 `build.rs` 用 `src/windows_overrides` 覆盖。

pub mod agent_cli;
pub mod agent_runtime;
pub mod app_main_cpp;
pub mod backend;
pub mod editor;
pub mod native;
pub mod platform;
pub mod platform_utils;
pub mod starmap_bridge;
pub mod starmap_view;
pub mod sujian_editor_item;
pub mod sync_bridge;
pub mod writing_bridge;
