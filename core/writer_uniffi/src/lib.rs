//! # Writer UniFFI — 跨平台绑定入口与最终库组装
//!
//! 本 crate 是 UniFFI 对外的唯一入口，负责：
//! - 重新导出 `writer_core` 的稳定 API（包括 UniFFI scaffolding）
//! - 产生最终 `cdylib` 供 Android/HarmonyOS 等平台链接
//! - 提供 `uniffi-bindgen` 二进制用于生成 Kotlin/Swift 绑定
//!
//! ## 依赖方向
//!
//! ```text
//! 原生端 → writer_uniffi (cdylib) → writer_core + writer_platform_api
//! ```
//!
//! `writer_core` 自身是 `rlib`，不直接产生 `cdylib`。
//! 所有平台的最终库都通过 `writer_uniffi` 或平台 crate 组装。

pub use writer_core::*;
