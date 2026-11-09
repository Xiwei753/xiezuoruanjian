//! Windows 网络状态探测与缓存。
//!
//! Windows 的联网状态由系统 WLAN/以太网服务维护，桌面前端只需要知道是否在线、
//! 是否计费以及代理地址。这里使用环境代理与保守的在线判断，避免启动时调用
//! PowerShell/WinHTTP 造成额外延迟。

use std::sync::{Mutex, OnceLock};

use writer_platform_api::NetworkState;

static CACHED_NETWORK_STATE: OnceLock<Mutex<NetworkState>> = OnceLock::new();

fn get_or_init_cache() -> &'static Mutex<NetworkState> {
    CACHED_NETWORK_STATE.get_or_init(|| Mutex::new(NetworkState::default()))
}

pub(crate) fn cache_network_state(state: &NetworkState) {
    if let Ok(mut guard) = get_or_init_cache().lock() {
        *guard = state.clone();
    }
}

pub fn get_cached_network_state() -> NetworkState {
    CACHED_NETWORK_STATE
        .get()
        .and_then(|m| m.lock().ok())
        .map(|g| g.clone())
        .unwrap_or_default()
}

pub fn refresh_network_state() -> NetworkState {
    let state = detect_network_state();
    cache_network_state(&state);
    state
}

pub(crate) fn detect_network_state() -> NetworkState {
    let proxy_url = std::env::var("HTTPS_PROXY")
        .or_else(|_| std::env::var("https_proxy"))
        .or_else(|_| std::env::var("HTTP_PROXY"))
        .or_else(|_| std::env::var("http_proxy"))
        .ok();

    let (proxy_host, proxy_port) = proxy_url
        .as_deref()
        .map(parse_proxy)
        .unwrap_or((None, None));

    NetworkState {
        is_connected: std::env::var("SUJIAN_FORCE_OFFLINE").is_err(),
        is_metered: false,
        proxy_host,
        proxy_port,
    }
}

fn parse_proxy(url: &str) -> (Option<String>, Option<u16>) {
    let url = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .unwrap_or(url);
    let without_auth = url.split('@').next_back().unwrap_or(url);

    // IPv6 形如 [::1]:7890，普通主机形如 127.0.0.1:7890。
    if let Some(rest) = without_auth.strip_prefix('[') {
        if let Some((host, tail)) = rest.split_once(']') {
            let port = tail
                .strip_prefix(':')
                .and_then(|value| value.trim().parse::<u16>().ok());
            return (Some(host.to_string()), port);
        }
    }

    let mut parts = without_auth.split(':');
    let host = parts.next().unwrap_or("").trim();
    let port = parts
        .next()
        .and_then(|value| value.trim().parse::<u16>().ok());
    if host.is_empty() {
        (None, port)
    } else {
        (Some(host.to_string()), port)
    }
}
