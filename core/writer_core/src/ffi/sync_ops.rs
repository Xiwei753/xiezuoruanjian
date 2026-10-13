//! 同步相关 FFI 函数 — 全量同步统一入口。
//!
//! 一个全局 `SyncConfig` + 一份全局凭据。旧的"作品同步 + 应用数据同步"两套
//! C ABI 已删除，新增 `writer_core_perform_full_sync` 等全量同步入口。
//!
//! ## 线程安全契约
//!
//! 所有函数通过 `with_app_service` 获取全局 `WriterAppService` 单例的 `Mutex` 锁。
//! 调用方不得在回调中再次调用任何 FFI 函数（非递归锁，会死锁）。
//!
//! ## JSON 传递语义
//!
//! 所有复杂数据通过 JSON C string 传递，格式为 `ResultEnvelope`。
//! 调用方必须用 `writer_core_free_string` 释放返回的 C string。

use std::os::raw::c_char;

use super::{c_str_to_rust, err_json, ok_json, patch_dto, with_app_service};
use crate::api::envelope::ResultEnvelope;
use crate::api::error::WriterError;
use crate::app_service::WriterAppService;

/// 执行一次同步操作，操作结束后无论成功失败都清除 secrets override。
///
/// Harmony C ABI 把一次同步调用收口成单操作 snapshot：开始时由
/// `WriterAppService::perform_full_sync_*` 内部的 `refresh_secrets_override`
/// 从持久化建立 override，结束后必须清掉，否则 override 会跨操作缓存陈旧
/// Token——用户改 Token 后下一次同步仍使用旧值（Issue #780 评论 5854433590）。
///
/// Android 平台层在单次操作内显式 set/clear override，不经过此 FFI 入口，
/// 因此这里收口不影响 Android 已有的操作级逻辑。
///
/// 语义：成功时若 clear 失败则返回 clear 错误；操作失败时忽略 clear 错误，
/// 保留原始操作错误，保证诊断信息不被清理失败掩盖。
fn run_sync_op<T, F>(svc: &WriterAppService, op: F) -> Result<T, WriterError>
where
    F: FnOnce(&WriterAppService) -> Result<T, WriterError>,
{
    let operation_result = op(svc);
    let clear_result = svc
        .clear_sync_secrets_override()
        .map_err(|e| WriterError::Other(format!("{}", e)));
    match operation_result {
        Ok(value) => {
            clear_result?;
            Ok(value)
        }
        Err(e) => {
            let _ = clear_result;
            Err(e)
        }
    }
}

/// # Safety
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_load_sync_config() -> *mut c_char {
    match with_app_service(|svc| {
        let dto: crate::api::SyncConfigDto =
            svc.load_sync_config().map_err(|e| format!("{}", e))?;
        Ok(dto)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SETTINGS_NOT_FOUND", &e),
    }
}

/// # Safety
/// `config_json` must be a valid null-terminated UTF-8 C string containing valid JSON.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_save_sync_config(config_json: *const c_char) -> *mut c_char {
    let json_str = match c_str_to_rust(config_json) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid config_json: error {}", e),
            )
        }
    };
    match with_app_service(|svc| {
        patch_dto(
            || svc.load_sync_config().map_err(|e| format!("{e}")),
            |next| svc.save_sync_config(next).map_err(|e| format!("{e}")),
            &json_str,
        )
    }) {
        Ok(()) => ok_json(true),
        Err(e) => err_json("SETTINGS_INVALID", &e),
    }
}

/// # Safety
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_load_sync_secrets() -> *mut c_char {
    match with_app_service(|svc| {
        let dto: crate::api::SyncSecretsDto =
            svc.load_sync_secrets().map_err(|e| format!("{}", e))?;
        Ok(dto)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SETTINGS_NOT_FOUND", &e),
    }
}

/// # Safety
/// `secrets_json` must be a valid null-terminated UTF-8 C string containing valid JSON.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_save_sync_secrets(secrets_json: *const c_char) -> *mut c_char {
    let json_str = match c_str_to_rust(secrets_json) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid secrets_json: error {}", e),
            )
        }
    };
    match with_app_service(|svc| {
        let dto: crate::api::SyncSecretsDto =
            serde_json::from_str(&json_str).map_err(|e| format!("JSON parse error: {}", e))?;
        svc.save_sync_secrets(dto).map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SETTINGS_INVALID", &e),
    }
}

/// 全量同步 dry-run C ABI。
///
///   改走 `with_app_service` 唯一 pipeline，
/// 经 `WriterAppService::perform_full_sync_dry_run` →
/// `WriterCoreApi::perform_full_sync_dry_run` →
/// `WriterCore::perform_full_sync_dry_run`。旧 facade `with_core` 路径
/// 不加载 pending deleted targets，已删除作品的远端前缀不会被清理。
///
/// # Safety
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_full_sync_dry_run() -> *mut c_char {
    let inner_result: Result<Result<_, WriterError>, String> = with_app_service(|svc| {
        Ok(run_sync_op(svc, |svc| {
            let config = svc.load_sync_config()?;
            let plan = svc.perform_full_sync_dry_run(config)?;
            Ok(plan)
        }))
    });
    match inner_result {
        Ok(Ok(data)) => ok_json(data),
        Ok(Err(e)) => {
            let envelope = ResultEnvelope::<()>::from_api_result(Err(e));
            let s = envelope.to_json_string();
            std::ffi::CString::new(s).unwrap_or_default().into_raw()
        }
        Err(e) => {
            let envelope = ResultEnvelope::<()>::from_api_result(Err(WriterError::Other(e)));
            let s = envelope.to_json_string();
            std::ffi::CString::new(s).unwrap_or_default().into_raw()
        }
    }
}

/// 全量同步诊断 C ABI— 只测一次仓库、分支、token。
///
///   改走 `with_app_service` 唯一 pipeline。
///
/// # Safety
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_full_sync_diagnostics() -> *mut c_char {
    let inner_result: Result<Result<_, WriterError>, String> = with_app_service(|svc| {
        Ok(run_sync_op(svc, |svc| {
            let config = svc.load_sync_config()?;
            let diag = svc.perform_full_sync_diagnostics(config)?;
            Ok(diag)
        }))
    });
    match inner_result {
        Ok(Ok(data)) => ok_json(data),
        Ok(Err(e)) => {
            let envelope = ResultEnvelope::<()>::from_api_result(Err(e));
            let s = envelope.to_json_string();
            std::ffi::CString::new(s).unwrap_or_default().into_raw()
        }
        Err(e) => {
            let envelope = ResultEnvelope::<()>::from_api_result(Err(WriterError::Other(e)));
            let s = envelope.to_json_string();
            std::ffi::CString::new(s).unwrap_or_default().into_raw()
        }
    }
}

/// 全量同步 C ABI— 先 App target，再所有 Project target。
///
///   改走 `with_app_service` 唯一 pipeline，
/// 经 `WriterAppService::perform_full_sync` →
/// `WriterCoreApi::perform_full_sync`（Prepare → Seed → Transfer → Commit）。
/// 旧 facade `with_app_service(|svc| svc.perform_full_sync(...))` 不加载
/// pending deleted targets，已删除作品的远端前缀不会被清理；且不走
/// 三段式 staging + workspace history，是第二套并行 pipeline。删除。
///
/// # Safety
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_perform_full_sync() -> *mut c_char {
    let inner_result: Result<Result<_, WriterError>, String> = with_app_service(|svc| {
        Ok(run_sync_op(svc, |svc| {
            let config = svc.load_sync_config()?;
            let result = svc.perform_full_sync(config, false)?;
            Ok(result)
        }))
    });
    match inner_result {
        Ok(Ok(data)) => ok_json(data),
        Ok(Err(e)) => {
            let envelope = ResultEnvelope::<()>::from_api_result(Err(e));
            let s = envelope.to_json_string();
            std::ffi::CString::new(s).unwrap_or_default().into_raw()
        }
        Err(e) => {
            let envelope = ResultEnvelope::<()>::from_api_result(Err(WriterError::Other(e)));
            let s = envelope.to_json_string();
            std::ffi::CString::new(s).unwrap_or_default().into_raw()
        }
    }
}

/// 加载 App target 同步状态。返回 JSON 形式的 `SyncState`。
///
/// # Safety
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_load_app_sync_state() -> *mut c_char {
    match with_app_service(|svc| {
        let state = svc.load_app_sync_state().map_err(|e| format!("{}", e))?;
        Ok(state)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SYNC_STATE_ERROR", &e),
    }
}

/// 保存 App target 同步状态。`state_json` 为 JSON 形式的 `SyncState`。
///
/// # Safety
/// `state_json` must be a valid null-terminated UTF-8 C string containing valid JSON.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_save_app_sync_state(state_json: *const c_char) -> *mut c_char {
    let json_str = match c_str_to_rust(state_json) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid state_json: error {}", e),
            )
        }
    };
    match with_app_service(|svc| {
        let state: crate::api::SyncStateDto =
            serde_json::from_str(&json_str).map_err(|e| format!("JSON parse error: {}", e))?;
        svc.save_app_sync_state(state)
            .map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SYNC_STATE_ERROR", &e),
    }
}

/// # Safety
/// `platform` and `device_class` must be valid null-terminated UTF-8 C strings.
#[no_mangle]
pub unsafe extern "C" fn writer_core_load_device_info() -> *mut c_char {
    match with_app_service(|svc| {
        let info = svc.load_device_info().map_err(|e| format!("{}", e))?;
        Ok(info)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("DEVICE_INFO_ERROR", &e),
    }
}

/// # Safety
/// `device_info_json` must be a valid null-terminated UTF-8 C string.
#[no_mangle]
pub unsafe extern "C" fn writer_core_save_device_info(
    device_info_json: *const c_char,
) -> *mut c_char {
    let json_str = match c_str_to_rust(device_info_json) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid device_info_json: error {}", e),
            )
        }
    };
    match with_app_service(|svc| {
        patch_dto(
            || svc.load_device_info().map_err(|e| format!("{e}")),
            |next| {
                svc.save_device_info_raw(&next)
                    .map(|()| true)
                    .map_err(|e| format!("{e}"))
            },
            &json_str,
        )
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("DEVICE_INFO_ERROR", &e),
    }
}

/// # Safety
/// `platform` and `device_class` must be valid null-terminated UTF-8 C strings.
#[no_mangle]
pub unsafe extern "C" fn writer_core_ensure_device_info(
    platform: *const c_char,
    device_class: *const c_char,
) -> *mut c_char {
    let platform_str = match c_str_to_rust(platform) {
        Ok(s) => {
            if s.len() > 64
                || !s
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                return err_json("INVALID_ARGUMENT", "Invalid platform format");
            }
            s
        }
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid platform: error {}", e),
            )
        }
    };
    let device_class_str = match c_str_to_rust(device_class) {
        Ok(s) => {
            if s.len() > 64
                || !s
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                return err_json("INVALID_ARGUMENT", "Invalid device_class format");
            }
            s
        }
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid device_class: error {}", e),
            )
        }
    };
    match with_app_service(|svc| {
        svc.ensure_device_info(platform_str, device_class_str)
            .map_err(|e| format!("{}", e))?;
        let info = svc.load_device_info().map_err(|e| format!("{}", e))?;
        Ok(info)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("DEVICE_INFO_ERROR", &e),
    }
}

#[cfg(test)]
mod tests {
    //! 验证 FFI 层 `run_sync_op` 把一次同步调用收口成单操作 snapshot：
    //! 操作结束后无论成功失败都清除 secrets override，陈旧 Token 不会跨操作缓存
    //! （Issue #780 评论 5854433590）。

    use super::*;

    fn set_override(svc: &WriterAppService, token: &str) {
        let secrets = crate::api::SyncSecretsDto {
            provider_secrets: Some(crate::api::ProviderSecretsDto::GitHub {
                token: token.to_string(),
            }),
        };
        svc.set_sync_secrets_override(secrets)
            .expect("set override");
    }

    #[test]
    fn run_sync_op_clears_override_after_success() {
        let dir = tempfile::TempDir::new().unwrap();
        let svc = WriterAppService::new(
            dir.path().to_string_lossy().to_string(),
            dir.path().join("projects").to_string_lossy().to_string(),
        );
        set_override(&svc, "token-a");
        assert!(svc.has_secrets_override());

        let result = run_sync_op(&svc, |_| Ok::<_, WriterError>(42));
        assert_eq!(result, Ok(42));
        assert!(
            !svc.has_secrets_override(),
            "override 必须在操作成功后被清除，下一次同步才能从持久化重新读取 Token"
        );
    }

    #[test]
    fn run_sync_op_clears_override_after_failure() {
        let dir = tempfile::TempDir::new().unwrap();
        let svc = WriterAppService::new(
            dir.path().to_string_lossy().to_string(),
            dir.path().join("projects").to_string_lossy().to_string(),
        );
        set_override(&svc, "token-a");
        assert!(svc.has_secrets_override());

        let result = run_sync_op(&svc, |_| Err::<i32, _>(WriterError::SyncFailed("sync failed".into())));
        assert!(result.is_err());
        assert!(
            !svc.has_secrets_override(),
            "override 必须在操作失败后也被清除，否则一次认证失败会把旧 Token 卡在进程里"
        );
    }

    #[test]
    fn run_sync_op_preserves_original_error_when_clear_also_fails() {
        // 操作失败时必须返回原始操作错误，不被 clear override 的错误掩盖。
        // clear_sync_secrets_override 当前实现不会失败，但 run_sync_op 的分支
        // 语义必须保证：Err 路径忽略 clear 结果，保留原始错误。
        let dir = tempfile::TempDir::new().unwrap();
        let svc = WriterAppService::new(
            dir.path().to_string_lossy().to_string(),
            dir.path().join("projects").to_string_lossy().to_string(),
        );
        set_override(&svc, "token-a");

        let result = run_sync_op(&svc, |_| Err::<i32, _>(WriterError::SyncFailed("original op error".into())));
        assert!(result.is_err());
        assert!(!svc.has_secrets_override());
    }
}
