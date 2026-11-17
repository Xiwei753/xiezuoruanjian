//! Windows 网络状态探测与缓存。
//!
//! 使用 Windows Network List Manager COM 接口获取真实联网状态和计费信息。
//! COM 生命周期通过 RAII guard 管理，查询失败时保留缓存值而非伪造状态。

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

    match query_network_status() {
        Ok((is_connected, is_metered)) => {
            let state = NetworkState {
                is_connected,
                is_metered,
                proxy_host,
                proxy_port,
            };
            cache_network_state(&state);
            state
        }
        Err(_) => {
            let cached = get_cached_network_state();
            NetworkState {
                is_connected: cached.is_connected,
                is_metered: cached.is_metered,
                proxy_host,
                proxy_port,
            }
        }
    }
}

struct ComGuard;
impl Drop for ComGuard {
    fn drop(&mut self) {
        unsafe {
            windows::Win32::System::Com::CoUninitialize();
        }
    }
}

fn query_network_status() -> Result<(bool, bool), ()> {
    use windows::core::Interface;
    use windows::Win32::Networking::NetworkListManager::{
        INetworkCostManager, INetworkListManager, NetworkListManager,
    };
    use windows::Win32::System::Com::{
        CLSCTX_ALL, CoCreateInstance, CoInitializeEx, COINIT_MULTITHREADED,
    };

    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if hr.is_err() {
        return Err(());
    }

    let _com_guard = ComGuard;

    let manager: windows::core::Result<INetworkListManager> =
        unsafe { CoCreateInstance(&NetworkListManager, None, CLSCTX_ALL) };

    let manager = manager.map_err(|_| ())?;

    let is_connected = unsafe {
        manager.IsConnectedToInternet()
    }
    .map(|v| v.as_bool())
    .map_err(|_| ())?;

    let is_metered = manager
        .cast::<INetworkCostManager>()
        .ok()
        .and_then(|cost_manager| {
            let mut cost_raw: u32 = 0;
            let result = unsafe { cost_manager.GetCost(&mut cost_raw, std::ptr::null()) };
            if result.is_ok() {
                const NLM_COST_FLAG_UNRESTRICTED: u32 = 0x1;
                Some(cost_raw & NLM_COST_FLAG_UNRESTRICTED == 0)
            } else {
                None
            }
        })
        .unwrap_or(false);

    Ok((is_connected, is_metered))
}

fn parse_proxy(url_str: &str) -> (Option<String>, Option<u16>) {
    let parsed = match url::Url::parse(url_str) {
        Ok(u) => u,
        Err(_) => return (None, None),
    };

    let host = parsed.host_str().map(|h| h.to_string());
    let port = parsed.port();

    (host, port)
}
