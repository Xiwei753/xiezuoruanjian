use std::collections::HashMap;

use crate::error::Error;

/// FFI 边界错误类型 — 跨语言传递的唯一错误形式。
///
/// 平台端通过 `code()` 和 `message_key()` 做错误分类和 i18n 映射，
/// 不得依赖错误文案的包含关系作为主判断（见 AGENTS.md）。
///
/// 与 `crate::error::Error` 的区别：
/// - `Error` 是 Core 内部错误枚举，携带丰富上下文（如 SyncDocumentConflict 的 path/hash）
/// - `WriterError` 是 FFI 边界的简化版本，只保留平台端需要的分类信息
/// - 两者通过 `From<Error>` 转换，内部错误细节在转换时可能被折叠为通用 SyncConflict/SyncFailed
#[derive(Debug, thiserror::Error)]
pub enum WriterError {
    #[error("IO error: {0}")]
    Io(String),
    #[error("JSON parsing error: {0}")]
    Json(String),
    #[error("Project not found")]
    ProjectNotFound,
    #[error("Volume not found")]
    VolumeNotFound,
    #[error("Chapter not found")]
    ChapterNotFound,
    /// 安全降级：当保存内容为空但旧内容非空时拒绝覆盖，
    /// 防止因 bug 或损坏数据导致章节正文被意外清空。
    #[error("blocked_empty_overwrite: chapter_id={chapter_id}, old_len={old_len}, new_len={new_len}, reason={reason}")]
    EmptyOverwriteBlocked {
        chapter_id: String,
        old_len: u32,
        new_len: u32,
        reason: String,
    },
    #[error("Not implemented")]
    NotImplemented,
    #[error("Refuse to delete root")]
    RefuseToDeleteRoot,
    #[error("Invalid delete target: {0}")]
    InvalidDeleteTarget(String),
    /// 同步冲突——包含冲突详情描述，需用户干预解决
    #[error("Sync conflict: {0}")]
    SyncConflict(String),
    /// 同步失败——网络/认证/API 等非冲突性错误
    #[error("Sync failed: {0}")]
    SyncFailed(String),
    // --- #592 七：类型化同步失败 ---
    /// 明确网络失败（网络不可用/限流）——可重试
    #[error("Retryable network failure: {0}")]
    RetryableNetwork(String),
    /// 明确 I/O 失败（磁盘临时不可用等）——可重试
    #[error("Retryable IO failure: {0}")]
    RetryableIo(String),
    /// 认证失败（Token 无效/权限不足）——需用户干预
    #[error("Sync authentication failure: {0}")]
    Authentication(String),
    /// 同步冲突（正文/设置/checkout 冲突）——需用户决策
    #[error("Sync conflict (typed): {0}")]
    Conflict(String),
    /// 本地仓库脏（非白名单文件被修改）——需用户处理
    #[error("Dirty repository blocked: {0}")]
    DirtyRepository(String),
    /// 协议错误（non-fast-forward / unrelated histories / 不完整事务）
    #[error("Sync protocol error: {0}")]
    Protocol(String),
    /// 未知/程序错误——默认不可重试
    #[error("Fatal sync failure: {0}")]
    Fatal(String),
    #[error("Other error: {0}")]
    Other(String),
}

impl WriterError {
    /// 返回稳定错误码——这是跨端 API 契约，不可随意更改。
    /// 平台端通过此码做错误分类，不得依赖错误文案包含关系。
    pub fn code(&self) -> &'static str {
        match self {
            WriterError::Io(_) => "IO_ERROR",
            WriterError::Json(_) => "JSON_ERROR",
            WriterError::ProjectNotFound => "PROJECT_NOT_FOUND",
            WriterError::VolumeNotFound => "VOLUME_NOT_FOUND",
            WriterError::ChapterNotFound => "CHAPTER_NOT_FOUND",
            WriterError::EmptyOverwriteBlocked { .. } => "EMPTY_OVERWRITE_BLOCKED",
            WriterError::NotImplemented => "NOT_IMPLEMENTED",
            WriterError::RefuseToDeleteRoot => "REFUSE_DELETE_ROOT",
            WriterError::InvalidDeleteTarget(_) => "INVALID_DELETE_TARGET",
            WriterError::SyncConflict(_) => "SYNC_CONFLICT",
            WriterError::SyncFailed(_) => "SYNC_FAILED",
            WriterError::RetryableNetwork(_) => "RETRYABLE_NETWORK",
            WriterError::RetryableIo(_) => "RETRYABLE_IO",
            WriterError::Authentication(_) => "AUTHENTICATION",
            WriterError::Conflict(_) => "CONFLICT",
            WriterError::DirtyRepository(_) => "DIRTY_REPOSITORY",
            WriterError::Protocol(_) => "PROTOCOL",
            WriterError::Fatal(_) => "FATAL",
            WriterError::Other(_) => "OTHER",
        }
    }

    /// 返回稳定的 i18n message key，供 UI 层做本地化映射。
    ///
    /// 这些 key 是跨端 API 契约，不可随意更改。
    pub fn message_key(&self) -> &'static str {
        match self {
            WriterError::Io(_) => "error.io",
            WriterError::Json(_) => "error.json",
            WriterError::ProjectNotFound => "error.project_not_found",
            WriterError::VolumeNotFound => "error.volume_not_found",
            WriterError::ChapterNotFound => "error.chapter_not_found",
            WriterError::EmptyOverwriteBlocked { .. } => "error.empty_overwrite_blocked",
            WriterError::NotImplemented => "error.not_implemented",
            WriterError::RefuseToDeleteRoot => "error.refuse_delete_root",
            WriterError::InvalidDeleteTarget(_) => "error.invalid_delete_target",
            WriterError::SyncConflict(_) => "error.sync_conflict",
            WriterError::SyncFailed(_) => "error.sync_failed",
            WriterError::RetryableNetwork(_) => "error.sync_retryable_network",
            WriterError::RetryableIo(_) => "error.sync_retryable_io",
            WriterError::Authentication(_) => "error.sync_auth_failed",
            WriterError::Conflict(_) => "error.sync_conflict",
            WriterError::DirtyRepository(_) => "error.sync_dirty_repository",
            WriterError::Protocol(_) => "error.sync_protocol_error",
            WriterError::Fatal(_) => "error.sync_fatal",
            WriterError::Other(_) => "error.other",
        }
    }

    /// 结构化错误参数，供 UI 层做本地化模板插值。
    ///
    /// UI 根据 `message_key` + `params` 生成用户提示，不依赖硬编码中文文案。
    pub fn params(&self) -> HashMap<String, String> {
        let mut m = HashMap::new();
        match self {
            WriterError::EmptyOverwriteBlocked {
                chapter_id,
                old_len,
                new_len,
                reason,
            } => {
                m.insert("chapter_id".into(), chapter_id.clone());
                m.insert("old_len".into(), old_len.to_string());
                m.insert("new_len".into(), new_len.to_string());
                m.insert("reason".into(), reason.clone());
            }
            WriterError::InvalidDeleteTarget(detail) => {
                m.insert("detail".into(), detail.clone());
            }
            WriterError::SyncConflict(detail) => {
                m.insert("detail".into(), detail.clone());
            }
            WriterError::SyncFailed(detail) => {
                m.insert("detail".into(), detail.clone());
            }
            WriterError::RetryableNetwork(detail) => {
                m.insert("detail".into(), detail.clone());
            }
            WriterError::RetryableIo(detail) => {
                m.insert("detail".into(), detail.clone());
            }
            WriterError::Authentication(detail) => {
                m.insert("detail".into(), detail.clone());
            }
            WriterError::Conflict(detail) => {
                m.insert("detail".into(), detail.clone());
            }
            WriterError::DirtyRepository(detail) => {
                m.insert("detail".into(), detail.clone());
            }
            WriterError::Protocol(detail) => {
                m.insert("detail".into(), detail.clone());
            }
            WriterError::Fatal(detail) => {
                m.insert("detail".into(), detail.clone());
            }
            WriterError::Io(detail) => {
                m.insert("detail".into(), detail.clone());
            }
            WriterError::Json(detail) => {
                m.insert("detail".into(), detail.clone());
            }
            WriterError::Other(detail) => {
                m.insert("detail".into(), detail.clone());
            }
            _ => {}
        }
        m
    }
}

#[allow(clippy::cast_possible_truncation)]
impl From<crate::error::Error> for WriterError {
    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    fn from(e: crate::error::Error) -> Self {
        match e {
            Error::Io(e) => WriterError::Io(e.to_string()),
            Error::Json(e) => WriterError::Json(e.to_string()),
            Error::ProjectNotFound => WriterError::ProjectNotFound,
            Error::VolumeNotFound => WriterError::VolumeNotFound,
            Error::ChapterNotFound => WriterError::ChapterNotFound,
            Error::EmptyOverwriteBlocked {
                chapter_id,
                old_len,
                new_len,
                reason,
            } => WriterError::EmptyOverwriteBlocked {
                chapter_id,
                old_len: old_len as u32,
                new_len: new_len as u32,
                reason,
            },
            Error::NotImplemented => WriterError::NotImplemented,
            Error::RefuseToDeleteRoot => WriterError::RefuseToDeleteRoot,
            Error::InvalidDeleteTarget(s) => WriterError::InvalidDeleteTarget(s),
            Error::SyncAuthFailed { reason } => WriterError::Authentication(reason),
            Error::SyncNetworkUnavailable { reason } => WriterError::RetryableNetwork(reason),
            Error::SyncRateLimited { retry_after_secs } => WriterError::RetryableNetwork(format!(
                "rate_limited: retry_after={}",
                retry_after_secs
            )),
            Error::SyncDocumentConflict {
                path,
                local_hash,
                remote_hash,
            } => WriterError::Conflict(format!(
                "path={} local={} remote={}",
                path, local_hash, remote_hash
            )),
            Error::SyncIncompleteTransaction {
                transaction_id,
                missing_files,
            } => WriterError::Protocol(format!(
                "incomplete_transaction: tx_id={} missing={}",
                transaction_id,
                missing_files.join(",")
            )),
            Error::SyncCheckoutConflict { summary_json } => {
                WriterError::Conflict(format!("checkout_conflict: {}", summary_json))
            }
            Error::SyncConflictDetected => {
                WriterError::Conflict("SyncConflict_Detected".to_string())
            }
            Error::SyncNonFastForward { detail } => {
                WriterError::Protocol(format!("non_fast_forward: {}", detail))
            }
            Error::SyncUnrelatedHistories { detail } => {
                WriterError::Protocol(format!("unrelated_histories: {}", detail))
            }
            Error::SyncRemoteBranchNotFound { detail } => {
                WriterError::Protocol(format!("remote_branch_not_found: {}", detail))
            }
            Error::SyncRemoteError {
                category,
                context,
                status,
                body_preview,
            } => {
                // #592 七：远端 Provider 错误按类别映射为类型化失败；
                // 只有明确网络/认证类别进入对应类型，其余默认 Fatal。
                //
                // Issue #645 评论 5504296097 第1点：`SyncErrorCategory` 已收成
                // provider-neutral 分类，这里使用新变体。先查旧 GitHub/Git code
                // 兼容映射（legacy_category_compat），再回退 provider-neutral from_code。
                let cat =
                    crate::sync::types::legacy_category_compat(&category).unwrap_or_else(|| {
                        crate::sync::types::SyncErrorCategory::from_code(&category, "")
                    });
                match cat {
                    crate::sync::types::SyncErrorCategory::AuthFailed
                    | crate::sync::types::SyncErrorCategory::PermissionDenied => {
                        WriterError::Authentication(format!(
                            "remote_api_error: category={} context={} status={}",
                            category, context, status
                        ))
                    }
                    crate::sync::types::SyncErrorCategory::Network
                    | crate::sync::types::SyncErrorCategory::TemporaryUnavailable
                    | crate::sync::types::SyncErrorCategory::RateLimited => {
                        WriterError::RetryableNetwork(format!(
                            "remote_api_error: category={} context={} status={}",
                            category, context, status
                        ))
                    }
                    _ => WriterError::Fatal(format!(
                        "remote_api_error: category={} context={} status={} body={}",
                        category, context, status, body_preview
                    )),
                }
            }
            Error::DiskFull {
                path,
                required_bytes,
            } => WriterError::RetryableIo(format!(
                "disk_full: path={} required={}",
                path, required_bytes
            )),
            Error::StorageTransactionIncomplete { transaction_id } => WriterError::Other(format!(
                "storage_transaction_incomplete: tx_id={}",
                transaction_id
            )),
            Error::SaveQueueFlushIncomplete {
                failed_types,
                remaining_queue_len,
            } => WriterError::Other(format!(
                "save_queue_flush_incomplete: failed_types={:?} remaining={}",
                failed_types, remaining_queue_len
            )),
            Error::Other(s) => WriterError::Other(s),
        }
    }
}

impl WriterError {
    /// Create a sync conflict error with conflict details.
    pub fn sync_conflict(detail: String) -> Self {
        WriterError::SyncConflict(detail)
    }

    /// Create a sync failed error with error message and optional error category.
    pub fn sync_failed(detail: String) -> Self {
        WriterError::SyncFailed(detail)
    }
}

impl From<serde_json::Error> for WriterError {
    fn from(e: serde_json::Error) -> Self {
        WriterError::Json(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_params_empty_overwrite_blocked() {
        let err = WriterError::EmptyOverwriteBlocked {
            chapter_id: "ch1".into(),
            old_len: 100,
            new_len: 0,
            reason: "empty content".into(),
        };
        let p = err.params();
        assert_eq!(p.get("chapter_id").unwrap(), "ch1");
        assert_eq!(p.get("old_len").unwrap(), "100");
        assert_eq!(p.get("new_len").unwrap(), "0");
        assert_eq!(p.get("reason").unwrap(), "empty content");
    }

    #[test]
    fn test_params_sync_conflict() {
        let err = WriterError::SyncConflict("path conflict".into());
        let p = err.params();
        assert_eq!(p.get("detail").unwrap(), "path conflict");
    }

    #[test]
    fn test_params_sync_failed() {
        let err = WriterError::SyncFailed("network timeout".into());
        let p = err.params();
        assert_eq!(p.get("detail").unwrap(), "network timeout");
    }

    #[test]
    fn test_typed_sync_failure_codes() {
        // #592 七：类型化失败的错误码是跨端契约，不得落入 "OTHER"。
        assert_eq!(
            WriterError::RetryableNetwork("n".into()).code(),
            "RETRYABLE_NETWORK"
        );
        assert_eq!(WriterError::RetryableIo("n".into()).code(), "RETRYABLE_IO");
        assert_eq!(
            WriterError::Authentication("n".into()).code(),
            "AUTHENTICATION"
        );
        assert_eq!(WriterError::Conflict("n".into()).code(), "CONFLICT");
        assert_eq!(
            WriterError::DirtyRepository("n".into()).code(),
            "DIRTY_REPOSITORY"
        );
        assert_eq!(WriterError::Protocol("n".into()).code(), "PROTOCOL");
        assert_eq!(WriterError::Fatal("n".into()).code(), "FATAL");
    }

    #[test]
    fn test_from_error_maps_to_typed_kinds() {
        // #592 七：Core 内部错误映射到类型化失败，不再折叠为笼统 SyncFailed。
        use crate::error::Error;
        let auth = WriterError::from(Error::SyncAuthFailed {
            reason: "bad token".into(),
        });
        assert!(matches!(auth, WriterError::Authentication(_)));
        let net = WriterError::from(Error::SyncNetworkUnavailable {
            reason: "dns".into(),
        });
        assert!(matches!(net, WriterError::RetryableNetwork(_)));
        let conflict = WriterError::from(Error::SyncDocumentConflict {
            path: "a".into(),
            local_hash: "l".into(),
            remote_hash: "r".into(),
        });
        assert!(matches!(conflict, WriterError::Conflict(_)));
        let nff = WriterError::from(Error::SyncNonFastForward { detail: "x".into() });
        assert!(matches!(nff, WriterError::Protocol(_)));
        let incomplete = WriterError::from(Error::SyncIncompleteTransaction {
            transaction_id: "t".into(),
            missing_files: vec!["f".into()],
        });
        assert!(matches!(incomplete, WriterError::Protocol(_)));
        let remote_auth = WriterError::from(Error::SyncRemoteError {
            category: "token_invalid".into(),
            context: "c".into(),
            status: 401,
            body_preview: "b".into(),
        });
        assert!(matches!(remote_auth, WriterError::Authentication(_)));
        let remote_net = WriterError::from(Error::SyncRemoteError {
            category: "dns_failed".into(),
            context: "c".into(),
            status: 0,
            body_preview: "b".into(),
        });
        assert!(matches!(remote_net, WriterError::RetryableNetwork(_)));
    }
}
