//! Git HTTPS 凭据回调与传输选项。
//!
//! 只实现标准 HTTP Basic 认证：用户名可显式填写，也可以在只提供令牌时使用
//! 通用非空用户名 `git`。不按域名识别厂商、不盲试多组用户名、不覆盖用户输入。
//! 密码/令牌不写入 URL、不进入日志。

use git2::{Cred, CredentialType, ProxyOptions, RemoteCallbacks};

/// 构造带凭据回调的 `RemoteCallbacks`。
///
/// - `username` 非空：始终使用该用户名。
/// - `username` 为空：优先使用 URL 中出现的用户名，否则在提供 secret 时使用 `git`。
/// - 无 secret：匿名访问，返回 `Cred::default()`。
pub(crate) fn callbacks(username: &str, secret: Option<&str>) -> RemoteCallbacks<'static> {
    let explicit_username = username.trim().to_string();
    let secret = secret.map(str::to_string).filter(|value| !value.is_empty());

    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(move |_url, username_from_url, allowed| {
        if let Some(secret) = secret.as_deref() {
            let user = if !explicit_username.is_empty() {
                explicit_username.as_str()
            } else {
                username_from_url.unwrap_or("git")
            };
            return Cred::userpass_plaintext(user, secret);
        }

        if allowed.contains(CredentialType::USERNAME) {
            let user = if !explicit_username.is_empty() {
                explicit_username.as_str()
            } else {
                username_from_url.unwrap_or("git")
            };
            return Cred::username(user);
        }

        Cred::default()
    });
    callbacks
}

/// 使用平台/环境代理设置（`auto`），不硬编码代理地址。
pub(crate) fn proxy_options() -> ProxyOptions<'static> {
    let mut proxy = ProxyOptions::new();
    proxy.auto();
    proxy
}
