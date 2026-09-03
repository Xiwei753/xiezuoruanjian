//! Provider-neutral 远端错误 — 所有 SyncProvider 实现共用的错误类型。
//!
//! [`ProviderError`] 是 provider 无关的远端错误枚举，不携带任何 GitHub 特定名字。
//! 通过 [`ProviderError::to_sync_error_category`] 映射到通用 [`SyncErrorCategory`]，
//! 通过 [`ProviderError::is_retryable`] 让 engine 判断是否可重试。
//!
//! GitHub HTTP 状态码到 `ProviderError` 的映射由 `github/error.rs` 完成，
//! 本模块只定义通用错误形态。

use crate::sync::types::SyncErrorCategory;

/// Provider-neutral 远端错误。
///
/// 每个变体携带足够上下文（reason/path），便于 engine 决策和日志诊断。
/// 不包含任何 GitHub/Git 特定名字，保持通用层强类型纯净。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProviderError {
    /// 认证失败（token 无效/过期）。不可重试，需用户干预。
    #[error("auth failed: {reason}")]
    AuthFailed { reason: String },

    /// 权限不足（token 有效但无对应资源写/读权限）。不可重试，需用户干预。
    #[error("permission denied: {reason}")]
    PermissionDenied { reason: String },

    /// 远端对象不存在。
    #[error("not found: path={path}")]
    NotFound { path: String },

    /// 前置条件失败（乐观并发冲突：远端版本与 IfMatch 不一致，或 CreateNew 时对象已存在）。
    /// engine 应重新拉取远端版本后重试，或上报冲突让用户决策。
    #[error("precondition failed: path={path}, reason={reason}")]
    PreconditionFailed { path: String, reason: String },

    /// 速率限制。可重试，等待 `retry_after_secs` 后重试。
    #[error("rate limited: retry_after_secs={retry_after_secs}")]
    RateLimited { retry_after_secs: u64 },

    /// 网络错误（DNS/TLS/连接失败/超时）。可重试。
    #[error("network error: {reason}")]
    Network { reason: String },

    /// 远端临时不可用（5xx 但非网络层错误）。可重试。
    #[error("temporary unavailable: {reason}")]
    TemporaryUnavailable { reason: String },

    /// 其他未分类错误。engine 按不可重试处理（除非上下文明确可重试）。
    #[error("provider error: {reason}")]
    Other { reason: String },
}

impl ProviderError {
    /// 是否可重试。
    ///
    /// - `Network` / `RateLimited` / `TemporaryUnavailable` → 可重试。
    /// - `AuthFailed` / `PermissionDenied` / `NotFound` / `PreconditionFailed` / `Other` → 不可重试。
    ///
    /// engine 据此决定是否进入退避重试循环，还是直接上报给用户。
    pub fn is_retryable(&self) -> bool {
        match self {
            ProviderError::Network { .. }
            | ProviderError::RateLimited { .. }
            | ProviderError::TemporaryUnavailable { .. } => true,
            ProviderError::AuthFailed { .. }
            | ProviderError::PermissionDenied { .. }
            | ProviderError::NotFound { .. }
            | ProviderError::PreconditionFailed { .. }
            | ProviderError::Other { .. } => false,
        }
    }

    /// 映射到通用同步错误分类 [`SyncErrorCategory`]。
    ///
    /// 该映射让现有 UI/状态机继续工作，同时把 GitHub 特定错误名隔离在 Provider 实现内。
    /// Phase 6 会精简 `SyncErrorCategory`（移除 `Github*` 变体），届时本映射同步更新。
    pub fn to_sync_error_category(&self) -> SyncErrorCategory {
        match self {
            ProviderError::AuthFailed { .. } => SyncErrorCategory::AuthError,
            ProviderError::PermissionDenied { .. } => SyncErrorCategory::TokenPermissionDenied,
            ProviderError::NotFound { .. } => SyncErrorCategory::FileNotFound,
            ProviderError::PreconditionFailed { .. } => SyncErrorCategory::Conflict,
            ProviderError::RateLimited { .. } => SyncErrorCategory::ApiRateLimited,
            ProviderError::Network { .. } | ProviderError::TemporaryUnavailable { .. } => {
                SyncErrorCategory::NetworkFailed
            }
            ProviderError::Other { .. } => SyncErrorCategory::Other,
        }
    }
}

/// 将 [`ProviderError`] 转为 Core 统一 [`crate::Error`]。
///
/// 映射规则（见 Issue #645 评论 5504296097）：
/// - `AuthFailed` / `PermissionDenied` → `SyncAuthFailed`
/// - `Network` / `TemporaryUnavailable` → `SyncNetworkUnavailable`
/// - `RateLimited` → `SyncRateLimited`
/// - `NotFound` → `SyncRemoteApiError { category: "file_not_found" }`
/// - `PreconditionFailed` → `SyncRemoteApiError { category: "remote_sha_conflict" }`
/// - `Other` → `Other`
///
/// `SyncRemoteApiError` 在 Phase 6 后仍保留作为通用远端 API 错误载体，
/// 其 `category` 字段是稳定 API 契约，不随 Provider 实现变化。
impl From<ProviderError> for crate::Error {
    fn from(err: ProviderError) -> Self {
        match err {
            ProviderError::AuthFailed { reason } => crate::Error::SyncAuthFailed { reason },
            ProviderError::PermissionDenied { reason } => crate::Error::SyncAuthFailed { reason },
            ProviderError::NotFound { path } => crate::Error::SyncRemoteApiError {
                category: "file_not_found".to_string(),
                context: "read".to_string(),
                status: 404,
                body_preview: path,
            },
            ProviderError::PreconditionFailed { path, reason } => {
                crate::Error::SyncRemoteApiError {
                    category: "remote_sha_conflict".to_string(),
                    context: "conditional_write".to_string(),
                    status: 409,
                    body_preview: format!("path={path}, reason={reason}"),
                }
            }
            ProviderError::RateLimited { retry_after_secs } => {
                crate::Error::SyncRateLimited { retry_after_secs }
            }
            ProviderError::Network { reason } => crate::Error::SyncNetworkUnavailable { reason },
            ProviderError::TemporaryUnavailable { reason } => {
                crate::Error::SyncNetworkUnavailable { reason }
            }
            ProviderError::Other { reason } => crate::Error::Other(reason),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryable_classification() {
        assert!(!ProviderError::AuthFailed {
            reason: "bad".into()
        }
        .is_retryable());
        assert!(!ProviderError::PermissionDenied {
            reason: "no".into()
        }
        .is_retryable());
        assert!(!ProviderError::NotFound { path: "x".into() }.is_retryable());
        assert!(!ProviderError::PreconditionFailed {
            path: "x".into(),
            reason: "r".into()
        }
        .is_retryable());
        assert!(ProviderError::RateLimited {
            retry_after_secs: 1
        }
        .is_retryable());
        assert!(ProviderError::Network {
            reason: "dns".into()
        }
        .is_retryable());
        assert!(ProviderError::TemporaryUnavailable {
            reason: "5xx".into()
        }
        .is_retryable());
        assert!(!ProviderError::Other { reason: "x".into() }.is_retryable());
    }

    #[test]
    fn maps_to_core_error_codes() {
        let e = crate::Error::from(ProviderError::AuthFailed { reason: "t".into() });
        assert_eq!(e.code(), "SYNC_AUTH_FAILED");

        let e = crate::Error::from(ProviderError::RateLimited {
            retry_after_secs: 30,
        });
        assert_eq!(e.code(), "SYNC_RATE_LIMITED");

        let e = crate::Error::from(ProviderError::Network {
            reason: "dns".into(),
        });
        assert_eq!(e.code(), "SYNC_NETWORK_UNAVAILABLE");

        let e = crate::Error::from(ProviderError::NotFound { path: "a/b".into() });
        assert_eq!(e.code(), "SYNC_REMOTE_API_ERROR");
    }
}
