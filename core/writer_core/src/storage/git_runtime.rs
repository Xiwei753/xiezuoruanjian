//! # libgit2 运行时平台级配置（Core 层基础设施）
//!
//! Android 共享存储（`/storage/emulated/0/...`）不提供桌面 POSIX owner 语义，
//! libgit2 默认的 owner 校验会把合法的应用目录拒掉（`Owner(-36)`）。
//! 本模块在 Android target 上一次性关闭 owner validation；其他 target 保持默认。
//!
//! 初始化通过 `OnceLock` 保证只执行一次，线程安全。

use std::sync::OnceLock;

static INIT: OnceLock<Result<(), String>> = OnceLock::new();

/// 确保 libgit2 运行时已完成平台级配置。幂等、线程安全。
///
/// Android target 关闭 owner validation；其他 target 为 no-op。
/// 必须在任何 `git2::Repository::open/init` 或 `RepoBuilder::clone` 之前调用。
pub fn ensure_initialized() -> crate::Result<()> {
    match INIT.get_or_init(configure) {
        Ok(()) => Ok(()),
        Err(message) => Err(crate::Error::Io(std::io::Error::other(message.clone()))),
    }
}

#[cfg(target_os = "android")]
fn configure() -> Result<(), String> {
    // Android emulated/shared storage 不提供可用于 libgit2 owner 校验的桌面 POSIX
    // 所有权语义。关闭 owner validation 让应用管理的共享存储目录合法。
    //
    // SAFETY: `set_verify_owner_validation` 设置 libgit2 全局选项，只在单线程初始化
    // 期通过 `OnceLock` 调用一次。调用发生在任何 Repository 操作之前。
    unsafe { git2::opts::set_verify_owner_validation(false) }.map_err(|error| error.to_string())
}

#[cfg(not(target_os = "android"))]
fn configure() -> Result<(), String> {
    Ok(())
}
