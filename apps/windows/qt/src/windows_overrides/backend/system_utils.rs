//! Windows Qt 平台系统工具。
//!
//! 系统深浅色直接读取 Windows 个性化注册表；剪贴板使用 arboard。
//! Linux 的 gdbus/kreadconfig/gsettings 探测不进入 Windows 包。

use std::ffi::c_void;
use std::sync::Mutex;

const HKEY_CURRENT_USER: *mut c_void = 0x8000_0001_usize as *mut c_void;
const RRF_RT_REG_DWORD: u32 = 0x0000_0010;

#[cfg_attr(windows, link(name = "advapi32"))]
extern "system" {
    fn RegGetValueW(
        hkey: *mut c_void,
        sub_key: *const u16,
        value_name: *const u16,
        flags: u32,
        value_type: *mut u32,
        data: *mut c_void,
        data_size: *mut u32,
    ) -> i32;
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn system_prefers_dark() -> bool {
    let sub_key = wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    let value_name = wide("AppsUseLightTheme");
    let mut light: u32 = 1;
    let mut value_type: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: sub_key/value_name 是以 NUL 结尾的 UTF-16；light 是有效 u32 缓冲区，
    // size 与实际缓冲区长度一致，注册表 API 只在本调用期间写入它们。
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            sub_key.as_ptr(),
            value_name.as_ptr(),
            RRF_RT_REG_DWORD,
            &mut value_type,
            (&mut light as *mut u32).cast::<c_void>(),
            &mut size,
        )
    };
    result == 0 && light == 0
}

pub(crate) fn detect_system_theme_from_platform() -> String {
    if system_prefers_dark() {
        "dark".to_string()
    } else {
        "light".to_string()
    }
}

static ARBOARD_CLIPBOARD: Mutex<Option<arboard::Clipboard>> = Mutex::new(None);

fn get_or_create_arboard() -> Option<std::sync::MutexGuard<'static, Option<arboard::Clipboard>>> {
    let mut guard = ARBOARD_CLIPBOARD.lock().ok()?;
    if guard.is_none() {
        match arboard::Clipboard::new() {
            Ok(clip) => {
                *guard = Some(clip);
            }
            Err(_) => return None,
        }
    }
    Some(guard)
}

#[allow(dead_code)]
pub(crate) enum ClipboardBackend {
    WlCopy,
    Xclip,
    Xsel,
    Arboard,
}

pub(crate) enum ClipboardCopyResult {
    Success { backend: ClipboardBackend },
    Unavailable,
}

impl ClipboardCopyResult {
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            ClipboardCopyResult::Success { backend } => {
                let backend_str = match backend {
                    ClipboardBackend::WlCopy => "wl-copy",
                    ClipboardBackend::Xclip => "xclip",
                    ClipboardBackend::Xsel => "xsel",
                    ClipboardBackend::Arboard => "arboard",
                };
                serde_json::json!({
                    "success": true,
                    "data": { "backend": backend_str },
                    "warnings": [],
                    "changedPaths": [],
                    "changedEntities": [],
                })
            }
            ClipboardCopyResult::Unavailable => {
                serde_json::json!({
                    "success": false,
                    "errorCode": "CLIPBOARD_UNAVAILABLE",
                    "messageKey": "error.clipboard_unavailable",
                    "messageArgs": {},
                    "rawError": "No clipboard backend available",
                    "warnings": [],
                    "changedPaths": [],
                    "changedEntities": [],
                })
            }
        }
    }
}

pub(crate) fn copy_text_to_clipboard_impl(text_str: &str) -> ClipboardCopyResult {
    if let Some(mut guard) = get_or_create_arboard() {
        if let Some(ref mut clip) = *guard {
            if clip.set_text(text_str.to_string()).is_ok() {
                return ClipboardCopyResult::Success {
                    backend: ClipboardBackend::Arboard,
                };
            }
        }
    }
    ClipboardCopyResult::Unavailable
}
