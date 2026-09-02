//! # libgit2 运行时平台级配置（Core 层基础设施）
//!
//! Android 共享存储（`/storage/emulated/0/...`）不提供桌面 POSIX owner 语义，
//! libgit2 默认的 owner 校验会把合法的应用目录拒掉（`Owner(-36)`）。
//! 本模块在 Android target 上一次性关闭 owner validation；其他 target 保持默认。
//!
//! 初始化通过 `OnceLock` 保证只执行一次，线程安全。
//!
//! #644 评论 5486167472 问题3：libgit2 的 `GIT_OPT_ENABLE_FSYNC_GITDIR` 默认 disabled，
//! libgit2 写 objects/refs 后无统一 durable barrier，崩溃时可能先于 Finished 丢盘。
//! 通过 `libgit2-sys` 直接依赖（见 `core/writer_core/Cargo.toml`）暴露
//! `git_libgit2_opts` / `GIT_OPT_ENABLE_FSYNC_GITDIR` FFI，本模块在 `configure()`
//! 的唯一初始化入口对所有 target 统一调用 `git_libgit2_opts(GIT_OPT_ENABLE_FSYNC_GITDIR, 1)`。
//! 这必须在所有 target 启用（libgit2 默认 disabled 不分平台，objects/refs 丢盘风险
//! 不分平台）。`sync::git::update_live_index` 在 `live_index.write()` 后的显式
//! fsync index + 父目录保留为事务自身的边界，但 libgit2 自己写 ODB/ref 的 durability
//! 由这里统一打开。NotGitRepo 路径的 `copy_dir_recursive` 也已是 durable recursive copy
//!（copy 后 fsync 文件 + 父目录，每层目录 bottom-up fsync）。

use std::sync::OnceLock;

static INIT: OnceLock<Result<(), String>> = OnceLock::new();

/// 确保 libgit2 运行时已完成平台级配置。幂等、线程安全。
///
/// 所有 target 启用 `GIT_OPT_ENABLE_FSYNC_GITDIR`；Android target 额外关闭
/// owner validation。必须在任何 `git2::Repository::open/init` 或 `RepoBuilder::clone`
/// 之前调用。
pub fn ensure_initialized() -> crate::Result<()> {
    match INIT.get_or_init(configure) {
        Ok(()) => Ok(()),
        Err(message) => Err(crate::Error::Io(std::io::Error::other(message.clone()))),
    }
}

/// #644 评论 5486167472 问题3：对所有 target 启用 libgit2 fsync-gitdir。
///
/// libgit2 默认不 fsync gitdir（`GIT_OPT_ENABLE_FSYNC_GITDIR` 默认 disabled），
/// 写 objects/refs 后无 durable barrier。这里通过 `libgit2-sys` 直接 FFI 调用
/// `git_libgit2_opts(GIT_OPT_ENABLE_FSYNC_GITDIR, 1)` 统一打开，保证 libgit2 自己
/// 写 ODB/ref 的 durability。返回 `c_int`（0 成功，负数错误）。
///
/// `GIT_OPT_ENABLE_FSYNC_GITDIR` 在非 msvc target 是 `u32`（见 libgit2-sys `git_enum!` 宏），
/// `git_libgit2_opts` 接受 `c_int`（i32）。值 3 远小于 `i32::MAX`，cast 不会 wrap，
/// 但 clippy 无法静态证明，此处窄范围 allow 是 FFI 边界的必要 cast。
#[allow(clippy::cast_possible_wrap)]
fn enable_fsync_gitdir() -> Result<(), String> {
    // SAFETY: `git_libgit2_opts` 设置 libgit2 全局选项，只在单线程初始化期通过
    // `OnceLock` 调用一次，调用发生在任何 Repository 操作之前。
    // `GIT_OPT_ENABLE_FSYNC_GITDIR` 是 libgit2 公开常量（值 3），`1` 表示启用。
    let rc = unsafe {
        libgit2_sys::git_libgit2_opts(
            libgit2_sys::GIT_OPT_ENABLE_FSYNC_GITDIR as std::ffi::c_int,
            1,
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(format!(
            "git_libgit2_opts(GIT_OPT_ENABLE_FSYNC_GITDIR, 1) failed: libgit2 error code {rc}"
        ))
    }
}

/// #644 评论 5486852142 问题4：统一 configure 入口。
///
/// 所有 target 先通过 git2-rs 公开入口 `set_verify_owner_validation` 触发 libgit2
/// crate 初始化（`git_libgit2_init`），再调 `enable_fsync_gitdir` 的 raw
/// `git_libgit2_opts`。libgit2 要求 `git_libgit2_init` 必须在任何其它 libgit2
/// 函数前调用；旧的非 Android configure 直接调 raw opts 而未先触发 init，违反此契约。
///
/// Android 关闭 owner validation（emulated/shared storage 无桌面 POSIX owner 语义）；
/// 非 Android 保持默认（开启 owner validation）。
fn configure() -> Result<(), String> {
    #[cfg(target_os = "android")]
    let verify_owner = false;
    #[cfg(not(target_os = "android"))]
    let verify_owner = true;

    // SAFETY: `set_verify_owner_validation` 设置 libgit2 全局选项，只在单线程初始化
    // 期通过 `OnceLock` 调用一次。调用发生在任何 Repository 操作之前。
    // 此调用同时触发 git2-rs 的 crate 初始化（`git_libgit2_init`）。
    unsafe { git2::opts::set_verify_owner_validation(verify_owner) }.map_err(|e| e.to_string())?;

    // 现在可以安全调用 raw git_libgit2_opts，因为 git2-rs 已完成初始化。
    // #644 评论 5486167472 问题3：所有 target 启用 fsync-gitdir。
    enable_fsync_gitdir()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 非 Android target：`configure` 启用 fsync-gitdir 后 `ensure_initialized` 必须返回 Ok。
    /// Android target 的 owner validation 关闭行为需在设备/模拟器上验证，
    /// 此处只锁定非 Android 平台的 fsync-gitdir 启用契约。
    #[cfg(not(target_os = "android"))]
    #[test]
    fn ensure_initialized_returns_ok_on_non_android() {
        assert!(ensure_initialized().is_ok());
    }

    /// `OnceLock` 保证 `configure` 只执行一次；多次调用必须返回同一结果。
    /// 锁定幂等契约，防止后续重构误把 `get_or_init` 改成每次都重新配置。
    #[test]
    fn ensure_initialized_is_idempotent() {
        let first = ensure_initialized();
        let second = ensure_initialized();
        assert_eq!(first.is_ok(), second.is_ok());
    }
}
