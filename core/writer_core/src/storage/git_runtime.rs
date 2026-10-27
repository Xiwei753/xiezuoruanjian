//! # libgit2 运行时平台级配置（Core 层基础设施）
//!
//! Android 共享存储（`/storage/emulated/0/...`）不提供桌面 POSIX owner 语义，
//! libgit2 默认的 owner 校验会把合法的应用目录拒掉（`Owner(-36)`）。
//! 本模块在 Android target 上一次性关闭 owner validation；其他 target 保持默认。
//!
//! 初始化通过 `OnceLock` 保证只执行一次，线程安全。
//!
//!   libgit2 的 `GIT_OPT_ENABLE_FSYNC_GITDIR` 默认 disabled，
//! libgit2 写 objects/refs 后无统一 durable barrier，崩溃时可能先于 Finished 丢盘。
//! 通过 `libgit2-sys` 直接依赖（见 `core/writer_core/Cargo.toml`）暴露
//! `git_libgit2_opts` / `GIT_OPT_ENABLE_FSYNC_GITDIR` FFI，本模块在 `configure()`
//! 的唯一初始化入口对所有 target 统一调用 `git_libgit2_opts(GIT_OPT_ENABLE_FSYNC_GITDIR, 1)`。
//! 这必须在所有 target 启用（libgit2 默认 disabled 不分平台，objects/refs 丢盘风险
//! 不分平台）。workspace git finalize 在 `live_index.write()` 后的显式
//! fsync index + 父目录保留为事务自身的边界，但 libgit2 自己写 ODB/ref 的 durability
//! 由这里统一打开。NotGitRepo 路径的 `copy_dir_recursive` 也已是 durable recursive copy
//!（copy 后 fsync 文件 + 父目录，每层目录 bottom-up fsync）。

use std::path::PathBuf;
use std::sync::OnceLock;

static INIT: OnceLock<Result<(), String>> = OnceLock::new();
static TLS_TRUST: OnceLock<(Option<PathBuf>, Option<PathBuf>)> = OnceLock::new();
static TLS_INIT: OnceLock<Result<(), String>> = OnceLock::new();

/// 注入平台解析出的 CA bundle/目录。路径由平台适配层保证真实存在且可读；
/// 重复调用不覆盖首次注入。必须在首次 Git HTTPS 连接前调用。
pub fn set_platform_tls_trust(bundle: Option<PathBuf>, dir: Option<PathBuf>) {
    let _ = TLS_TRUST.set((bundle, dir));
}

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

/// 在首次 Git HTTPS 连接前配置平台 CA。与 libgit2 全局初始化分开，避免
/// 本地 Git 操作或应用启动因 CA 暂不可读而失败。
pub fn ensure_tls_initialized() -> crate::Result<()> {
    ensure_initialized()?;
    match TLS_INIT.get_or_init(configure_tls_trust) {
        Ok(()) => Ok(()),
        Err(message) => Err(crate::Error::Io(std::io::Error::other(message.clone()))),
    }
}

/// 为 vendored OpenSSL 配置平台 CA。未注入时不假设任何路径；配置失败返回
/// 稳定前缀 `ca_store_unavailable`，供 Provider 层区分于普通运行时初始化失败。
fn configure_tls_trust() -> Result<(), String> {
    let (bundle, dir) = TLS_TRUST.get().cloned().unwrap_or((None, None));
    if bundle.is_none() && dir.is_none() {
        return Ok(());
    }
    if let Some(path) = bundle.as_deref() {
        if !path.is_file() || std::fs::File::open(path).is_err() {
            return Err(format!(
                "ca_store_unavailable: configured CA bundle is not readable: {}",
                path.display()
            ));
        }
        // SAFETY: libgit2 全局 SSL 选项只在 OnceLock 初始化期、任何连接开始前设置。
        unsafe { git2::opts::set_ssl_cert_file(path) }
            .map_err(|err| format!("ca_store_unavailable: set CA bundle failed: {err}"))?;
    }
    if let Some(path) = dir.as_deref() {
        if !path.is_dir() || std::fs::read_dir(path).is_err() {
            return Err(format!(
                "ca_store_unavailable: configured CA directory is not readable: {}",
                path.display()
            ));
        }
        // SAFETY: 同上，一次性设置 libgit2 全局 SSL 目录。
        unsafe { git2::opts::set_ssl_cert_dir(path) }
            .map_err(|err| format!("ca_store_unavailable: set CA directory failed: {err}"))?;
    }
    Ok(())
}

///   对所有 target 启用 libgit2 fsync-gitdir。
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

///   统一 configure 入口。
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

    // libgit2 的服务器超时也是无同步保护的 C 全局状态，官方要求只在启动阶段、
    // 其他线程开始 Git 操作前设置。这里与 owner validation、fsync-gitdir 一起由
    // `OnceLock` 只配置一次，绝不在每次 open/fetch 时重复写入。
    //
    // SAFETY: 同一 `OnceLock` 初始化路径，调用发生在任何同步线程使用 libgit2 之前。
    unsafe {
        git2::opts::set_server_connect_timeout_in_milliseconds(30_000)
            .map_err(|e| format!("set git connect timeout failed: {e}"))?;
        git2::opts::set_server_timeout_in_milliseconds(120_000)
            .map_err(|e| format!("set git server timeout failed: {e}"))?;
    }

    // 现在可以安全调用 raw git_libgit2_opts，因为 git2-rs 已完成初始化。
    //   所有 target 启用 fsync-gitdir。
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
