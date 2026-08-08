//! Linux 网络状态探测与缓存。
//!
//! 通过 `nmcli` 或 `/sys/class/net` 探测连通性与计费状态，并尊重系统代理环境变量。
//! 探测结果缓存到进程级 `OnceLock`，避免频繁调用外部命令。

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
    let (is_connected, is_metered) = detect_connectivity_and_metered();
    NetworkState {
        is_connected,
        is_metered,
        proxy_host: std::env::var("http_proxy")
            .or_else(|_| std::env::var("HTTP_PROXY"))
            .or_else(|_| std::env::var("https_proxy"))
            .or_else(|_| std::env::var("HTTPS_PROXY"))
            .ok()
            .and_then(|url| {
                let url = url
                    .strip_prefix("http://")
                    .or_else(|| url.strip_prefix("https://"))
                    .unwrap_or(&url);
                let without_auth = url.split('@').next_back().unwrap_or(url);
                let host = without_auth.split(':').next().unwrap_or("");
                let host = host.trim();
                if host.is_empty() {
                    None
                } else {
                    Some(host.to_string())
                }
            }),
        proxy_port: std::env::var("http_proxy")
            .or_else(|_| std::env::var("HTTP_PROXY"))
            .or_else(|_| std::env::var("https_proxy"))
            .or_else(|_| std::env::var("HTTPS_PROXY"))
            .ok()
            .and_then(|url| {
                let url = url
                    .strip_prefix("http://")
                    .or_else(|| url.strip_prefix("https://"))
                    .unwrap_or(&url);
                let without_auth = url.split('@').next_back().unwrap_or(url);
                without_auth
                    .split(':')
                    .nth(1)
                    .and_then(|s| s.trim().parse::<u16>().ok())
            }),
    }
}

fn detect_connectivity_and_metered() -> (bool, bool) {
    if let Ok(output) = std::process::Command::new("nmcli")
        .args(["-t", "-f", "STATE", "general", "status"])
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let is_connected = stdout
                .lines()
                .any(|line| line.trim() == "connected" || line.starts_with("connected"));
            let is_metered = check_nmcli_metered();
            return (is_connected, is_metered);
        }
    }
    (check_network_connectivity(), false)
}

fn check_nmcli_metered() -> bool {
    let output = match std::process::Command::new("nmcli")
        .args(["-t", "-f", "METERED", "dev", "show"])
        .output()
    {
        Ok(o) => o,
        Err(_) => return false,
    };
    if !output.status.success() {
        return false;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if let Some(value) = line.strip_prefix("METERED:") {
            let v = value.trim();
            return v == "yes" || v == "guess-yes";
        }
    }
    false
}

fn check_network_connectivity() -> bool {
    let entries = match std::fs::read_dir("/sys/class/net") {
        Ok(e) => e,
        Err(_) => return false,
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str == "lo" {
            continue;
        }
        let operstate_path = std::path::Path::new("/sys/class/net")
            .join(name_str.as_ref())
            .join("operstate");
        let Ok(state) = std::fs::read_to_string(&operstate_path) else {
            continue;
        };
        if state.trim() == "up" {
            return true;
        }
    }
    false
}
