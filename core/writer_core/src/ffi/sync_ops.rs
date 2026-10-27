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
use crate::api::SyncConfigDto;

struct SyncFailureContext {
    operation_id: String,
    stage: String,
    category: String,
}

struct SyncOperationFailure {
    error: WriterError,
    context: SyncFailureContext,
}

fn sync_failure_category(error: &WriterError, stage: &str) -> String {
    match error {
        WriterError::Io(detail) => {
            let subtype = [
                ("sync_config_file_read", "config_file_read"),
                ("sync_config_file_decode", "config_file_decode"),
                ("sync_config_migration_write", "config_migration_write"),
                ("sync_secure_storage_read", "secure_storage_read"),
                ("sync_secure_storage_decode", "secure_storage_decode"),
                ("sync_credentials_file_read", "credentials_file_read"),
                ("sync_credentials_file_decode", "credentials_file_decode"),
                ("sync_credential_migration_", "credential_migration_io"),
            ]
            .iter()
            .find(|(marker, _)| detail.contains(marker))
            .map(|(_, category)| *category);
            subtype
                .unwrap_or(if stage == "config_load" {
                    "config_local_io"
                } else {
                    "credentials_local_io"
                })
                .to_string()
        }
        WriterError::Json(_) => "local_decode_error".to_string(),
        WriterError::RetryableIo(_) => "local_io_error".to_string(),
        WriterError::RetryableNetwork(_) => "network_error".to_string(),
        WriterError::Authentication(_) => "authentication_error".to_string(),
        WriterError::SyncFailure { category, .. } => category.clone(),
        WriterError::Other(_) if stage == "core_enter" => "sync_core_error".to_string(),
        _ => "sync_error".to_string(),
    }
}

fn sync_config_has_remote_url(config: &SyncConfigDto) -> bool {
    config
        .provider_config
        .as_ref()
        .is_some_and(|provider| match provider {
            crate::api::ProviderConfigDto::GitHttps { remote_url, .. }
            | crate::api::ProviderConfigDto::GitHub { remote_url, .. } => {
                !remote_url.trim().is_empty()
            }
        })
}

fn record_sync_preflight_event(
    operation_id: &str,
    stage: &str,
    outcome: &str,
    error: Option<&WriterError>,
    error_category: Option<&str>,
) {
    let mut fields = std::collections::BTreeMap::new();
    fields.insert(
        "operation_id".to_string(),
        serde_json::Value::String(operation_id.to_string()),
    );
    fields.insert(
        "phase".to_string(),
        serde_json::Value::String(stage.to_string()),
    );
    fields.insert(
        "outcome".to_string(),
        serde_json::Value::String(outcome.to_string()),
    );
    if let Some(error) = error {
        fields.insert(
            "error_code".to_string(),
            serde_json::Value::String(error.code().to_string()),
        );
    }
    if let Some(category) = error_category {
        fields.insert(
            "error_category".to_string(),
            serde_json::Value::String(category.to_string()),
        );
    }
    let level = if outcome == "error" {
        writer_diagnostics::DiagnosticLevel::Error
    } else {
        writer_diagnostics::DiagnosticLevel::Info
    };
    writer_diagnostics::record_event(writer_diagnostics::DiagnosticEvent {
        timestamp_ms: chrono::Utc::now().timestamp_millis(),
        sequence: 0,
        session_id: String::new(),
        level,
        origin: writer_diagnostics::DiagnosticOrigin::App,
        event: if outcome == "started" {
            "sync.preflight.start".to_string()
        } else {
            "sync.preflight.end".to_string()
        },
        target: "sync.preflight".to_string(),
        message: None,
        fields,
    });
}

fn persist_sync_preflight_failure(svc: &WriterAppService, stage: &str, category: &str) {
    if let Err(error) = svc.record_full_sync_preflight_failure_typed(
        "recoverable_error".to_string(),
        "preflight".to_string(),
        category.to_string(),
        stage.to_string(),
        stage.to_string(),
    ) {
        log::warn!(
            "[sync] preflight state write failed: stage={} error_code={}",
            stage,
            error.code()
        );
    }
}

fn perform_full_sync_with_preflight_diagnostics(
    svc: &WriterAppService,
    operation_id: &str,
) -> Result<crate::api::FullSyncResultDto, SyncOperationFailure> {
    record_sync_preflight_event(operation_id, "config_load", "started", None, None);
    let config = match svc.load_sync_config() {
        Ok(config) => {
            record_sync_preflight_event(operation_id, "config_load", "success", None, None);
            config
        }
        Err(error) => {
            let category = sync_failure_category(&error, "config_load");
            record_sync_preflight_event(
                operation_id,
                "config_load",
                "error",
                Some(&error),
                Some(&category),
            );
            persist_sync_preflight_failure(svc, "config_load", &category);
            return Err(SyncOperationFailure {
                error,
                context: SyncFailureContext {
                    operation_id: operation_id.to_string(),
                    stage: "config_load".to_string(),
                    category,
                },
            });
        }
    };

    record_sync_preflight_event(operation_id, "secrets_load", "started", None, None);
    if let Err(error) = svc.refresh_secrets_override() {
        let category = sync_failure_category(&error, "secrets_load");
        record_sync_preflight_event(
            operation_id,
            "secrets_load",
            "error",
            Some(&error),
            Some(&category),
        );
        persist_sync_preflight_failure(svc, "secrets_load", &category);
        return Err(SyncOperationFailure {
            error,
            context: SyncFailureContext {
                operation_id: operation_id.to_string(),
                stage: "secrets_load".to_string(),
                category,
            },
        });
    }
    record_sync_preflight_event(operation_id, "secrets_load", "success", None, None);

    record_sync_preflight_event(operation_id, "core_enter", "started", None, None);
    let result = svc.perform_full_sync_after_preflight(config.clone(), false, operation_id.to_string());
    match result {
        Ok(result) => {
            record_sync_preflight_event(operation_id, "core_enter", "success", None, None);
            Ok(result)
        }
        Err(error) => {
            let category = if config.enabled && !sync_config_has_remote_url(&config) {
                "repository_unconfigured".to_string()
            } else {
                sync_failure_category(&error, "core_enter")
            };
            let stage = match &error {
                WriterError::SyncFailure { stage, .. } => stage.clone(),
                _ => "core_enter".to_string(),
            };
            record_sync_preflight_event(
                operation_id,
                &stage,
                "error",
                Some(&error),
                Some(&category),
            );
            Err(SyncOperationFailure {
                error,
                context: SyncFailureContext {
                    operation_id: operation_id.to_string(),
                    stage,
                    category,
                },
            })
        }
    }
}

fn sync_error_envelope_json(
    error: WriterError,
    context: SyncFailureContext,
) -> String {
    let mut envelope = ResultEnvelope::<()>::error(error);
    envelope.diagnostic_operation_id = Some(context.operation_id);
    envelope.failure_stage = Some(context.stage.to_string());
    envelope.failure_category = Some(context.category);
    envelope.to_json_string()
}

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
    let operation_id = uuid::Uuid::new_v4().to_string();
    record_sync_preflight_event(&operation_id, "operation", "started", None, None);
    let mut failure_context: Option<SyncFailureContext> = None;
    let inner_result: Result<Result<_, WriterError>, String> = with_app_service(|svc| {
        Ok(run_sync_op(svc, |svc| {
            match perform_full_sync_with_preflight_diagnostics(svc, &operation_id) {
                Ok(result) => Ok(result),
                Err(failure) => {
                    failure_context = Some(failure.context);
                    Err(failure.error)
                }
            }
        }))
    });
    match inner_result {
        Ok(Ok(data)) => {
            record_sync_preflight_event(&operation_id, "operation", "success", None, None);
            let mut envelope = ResultEnvelope::success(data);
            envelope.diagnostic_operation_id = Some(operation_id);
            std::ffi::CString::new(envelope.to_json_string())
                .unwrap_or_default()
                .into_raw()
        }
        Ok(Err(e)) => {
            let stage_already_recorded = failure_context.is_some();
            let context = failure_context.unwrap_or_else(|| SyncFailureContext {
                operation_id: operation_id.clone(),
                stage: "cleanup".to_string(),
                category: sync_failure_category(&e, "cleanup"),
            });
            if !stage_already_recorded {
                record_sync_preflight_event(
                    &context.operation_id,
                    &context.stage,
                    "error",
                    Some(&e),
                    Some(&context.category),
                );
            }
            record_sync_preflight_event(
                &operation_id,
                "operation",
                "error",
                Some(&e),
                Some(&context.category),
            );
            std::ffi::CString::new(sync_error_envelope_json(e, context))
                .unwrap_or_default()
                .into_raw()
        }
        Err(e) => {
            let error = WriterError::Other(e);
            let category = sync_failure_category(&error, "app_service_entry");
            let context = SyncFailureContext {
                operation_id: operation_id.clone(),
                stage: "app_service_entry".to_string(),
                category: category.clone(),
            };
            record_sync_preflight_event(
                &operation_id,
                "app_service_entry",
                "error",
                Some(&error),
                Some(&category),
            );
            record_sync_preflight_event(
                &operation_id,
                "operation",
                "error",
                Some(&error),
                Some(&category),
            );
            std::ffi::CString::new(sync_error_envelope_json(error, context))
                .unwrap_or_default()
                .into_raw()
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

/// 加载全量同步持久状态（`<app_data_root>/app-meta/sync/full_state.local.json`）。
///
/// 与 per-target `state.local.json` 分层：这里只返回"最近一次全量同步事务"的
/// 总体结果。Core 尚未写入任何结果时 `data` 为 `null` —— 这是"从未成功同步"，
/// 不是查询失败，平台端不得把两者混为一谈。
///
/// # Safety
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_load_full_sync_state() -> *mut c_char {
    match with_app_service(|svc| {
        let state = svc.load_full_sync_state().map_err(|e| format!("{}", e))?;
        Ok(state)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SYNC_STATE_ERROR", &e),
    }
}

/// 加载最近一次完整同步报告（失败组、路径、阶段和脱敏后的类型化原因）。
///
/// Core 尚未持久化报告时 `data` 为 `null`，不是查询失败。
///
/// # Safety
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_load_latest_full_sync_report() -> *mut c_char {
    match with_app_service(|svc| {
        let report = svc
            .load_latest_full_sync_report()
            .map_err(|e| format!("{}", e))?;
        Ok(report)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SYNC_STATE_ERROR", &e),
    }
}

/// 列出 App target + 所有 Project target 的未解决冲突摘要。
///
/// 只返回 `AllSyncConflictEntryDto` 摘要（不含正文/快照），让平台端展示冲突
/// 总览；单条内容用 `writer_core_load_sync_conflict_preview` 获取。
///
/// # Safety
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_list_all_sync_conflicts() -> *mut c_char {
    match with_app_service(|svc| {
        let entries = svc
            .list_all_sync_conflicts()
            .map_err(|e| format!("{}", e))?;
        Ok(entries)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SYNC_CONFLICT", &e),
    }
}

/// 读取必填的 UTF-8 C 字符串参数；null 或空串都视为缺失。
fn required_c_str_arg(value: *const c_char, name: &str) -> Result<String, String> {
    match c_str_to_rust(value) {
        Ok(s) if !s.is_empty() => Ok(s),
        Ok(_) => Err(format!("{name} must not be empty")),
        Err(code) => Err(format!("Invalid {name}: error {code}")),
    }
}

/// 读取可选的 UTF-8 C 字符串参数；null 或空串都映射为 `None`。
///
/// App target 的冲突不属于任何作品，平台端传 null / "" 均可。
fn optional_c_str_arg(value: *const c_char, name: &str) -> Result<Option<String>, String> {
    if value.is_null() {
        return Ok(None);
    }
    match c_str_to_rust(value) {
        Ok(s) if s.is_empty() => Ok(None),
        Ok(s) => Ok(Some(s)),
        Err(code) => Err(format!("Invalid {name}: error {code}")),
    }
}

/// 加载一条冲突的本地/远端内容预览。
///
/// `target_kind` 为 `"app"` / `"project"`；`project_id` 允许 null 或空串。
/// `remote_deleted == true` 时 `remote_content` 为 `None`，不伪造空字符串。
///
/// # Safety
/// All string arguments must be valid null-terminated UTF-8 C strings.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_load_sync_conflict_preview(
    target_kind: *const c_char,
    project_id: *const c_char,
    path: *const c_char,
) -> *mut c_char {
    let target_kind_str = match required_c_str_arg(target_kind, "target_kind") {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &e),
    };
    let project_id_str = match optional_c_str_arg(project_id, "project_id") {
        Ok(v) => v,
        Err(e) => return err_json("INVALID_ARGUMENT", &e),
    };
    let path_str = match required_c_str_arg(path, "path") {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &e),
    };
    match with_app_service(|svc| {
        svc.load_sync_conflict_preview(target_kind_str, project_id_str, path_str)
            .map_err(|e| format!("{}", e))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SYNC_CONFLICT", &e),
    }
}

/// 冲突解决动作 — `decision` 字符串到现有 Core API 的映射。
///
/// FFI 只做映射，不实现合并策略：三种动作分别转发到
/// `WriterAppService::resolve_conflict_keep_local / _take_remote / _mark_merged`。
enum SyncConflictDecision {
    KeepLocal,
    TakeRemote,
    MarkMerged,
}

/// 按用户决定解决一条同步冲突。
///
/// `decision` 只接受 `"keep_local"` / `"take_remote"` / `"mark_merged"`；
/// `target_kind == "project"` 时 `project_id` 必填。返回值透传 Core：
/// `take_remote` 的 `false` 表示仅排队 pending（未立即替换 live 正文），
/// 平台端据此决定是否重载编辑器。
///
/// # Safety
/// All string arguments must be valid null-terminated UTF-8 C strings.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_resolve_sync_conflict(
    target_kind: *const c_char,
    project_id: *const c_char,
    path: *const c_char,
    decision: *const c_char,
) -> *mut c_char {
    let target_kind_str = match required_c_str_arg(target_kind, "target_kind") {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &e),
    };
    let project_id_str = match optional_c_str_arg(project_id, "project_id") {
        Ok(v) => v,
        Err(e) => return err_json("INVALID_ARGUMENT", &e),
    };
    let path_str = match required_c_str_arg(path, "path") {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &e),
    };
    let decision_str = match required_c_str_arg(decision, "decision") {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &e),
    };
    if target_kind_str == "project" && project_id_str.is_none() {
        return err_json(
            "INVALID_ARGUMENT",
            "project_id is required for target_kind \"project\"",
        );
    }
    let decision_kind = match decision_str.as_str() {
        "keep_local" => SyncConflictDecision::KeepLocal,
        "take_remote" => SyncConflictDecision::TakeRemote,
        "mark_merged" => SyncConflictDecision::MarkMerged,
        other => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!(
                    "Invalid decision '{other}': expected keep_local | take_remote | mark_merged"
                ),
            )
        }
    };
    match with_app_service(|svc| {
        let applied = match decision_kind {
            SyncConflictDecision::KeepLocal => {
                svc.resolve_conflict_keep_local(target_kind_str, project_id_str, path_str)
            }
            SyncConflictDecision::TakeRemote => {
                svc.resolve_conflict_take_remote(target_kind_str, project_id_str, path_str)
            }
            SyncConflictDecision::MarkMerged => {
                svc.resolve_conflict_mark_merged(target_kind_str, project_id_str, path_str)
            }
        }
        .map_err(|e| format!("{}", e))?;
        Ok(applied)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SYNC_CONFLICT", &e),
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
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
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

        let result = run_sync_op(&svc, |_| {
            Err::<i32, _>(WriterError::SyncFailed("sync failed".into()))
        });
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

        let result = run_sync_op(&svc, |_| {
            Err::<i32, _>(WriterError::SyncFailed("original op error".into()))
        });
        assert!(result.is_err());
        assert!(!svc.has_secrets_override());
    }
}
