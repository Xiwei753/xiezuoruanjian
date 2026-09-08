//! GitHub HTTP 状态码到 [`ProviderError`] 的映射。
//!
//! 本模块把 GitHub 特定的 HTTP 响应翻译为 provider-neutral 的 [`ProviderError`]，
//! 让通用层不出现 `Github*` 名字。`context` 仅用于错误 reason 拼装，不影响分类。

use crate::sync::provider::error::ProviderError;

/// 把 GitHub HTTP 响应映射为 [`ProviderError`]。
///
/// 映射规则（见 Issue #645 评论 5504296097）：
/// - 401 → `AuthFailed`
/// - 403 + body 含 "Resource not accessible" → `PermissionDenied`
/// - 403 其他 → `AuthFailed`
/// - 404 → `NotFound`
/// - 409 → `PreconditionFailed`
/// - 429 → `RateLimited { retry_after_secs: 0 }`
/// - 5xx → `Network`
/// - 其他 → `Other`
///
/// `context` 用于 `PreconditionFailed`/`Other` 的 reason 拼装，便于日志定位。
pub fn map_http_error(context: &str, status: u16, body: String) -> ProviderError {
    match status {
        401 => ProviderError::AuthFailed {
            reason: format!("http 401: {}", truncate(&body, 200)),
        },
        403 => {
            if body.contains("Resource not accessible") {
                ProviderError::PermissionDenied {
                    reason: format!("http 403: {}", truncate(&body, 200)),
                }
            } else {
                ProviderError::AuthFailed {
                    reason: format!("http 403: {}", truncate(&body, 200)),
                }
            }
        }
        404 => ProviderError::NotFound {
            path: context.to_string(),
        },
        409 => ProviderError::PreconditionFailed {
            path: context.to_string(),
            reason: format!("http 409 sha conflict: {}", truncate(&body, 200)),
        },
        429 => ProviderError::RateLimited {
            retry_after_secs: 0,
        },
        s if (500..600).contains(&s) => ProviderError::Network {
            reason: format!("http {}: {}", s, truncate(&body, 200)),
        },
        s => ProviderError::Other {
            reason: format!("{} http {}: {}", context, s, truncate(&body, 200)),
        },
    }
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_known_statuses() {
        assert!(matches!(
            map_http_error("c", 401, "b".into()),
            ProviderError::AuthFailed { .. }
        ));
        assert!(matches!(
            map_http_error("c", 403, "Resource not accessible".into()),
            ProviderError::PermissionDenied { .. }
        ));
        assert!(matches!(
            map_http_error("c", 403, "other".into()),
            ProviderError::AuthFailed { .. }
        ));
        assert!(matches!(
            map_http_error("path", 404, "b".into()),
            ProviderError::NotFound { .. }
        ));
        assert!(matches!(
            map_http_error("path", 409, "b".into()),
            ProviderError::PreconditionFailed { .. }
        ));
        assert!(matches!(
            map_http_error("c", 429, "b".into()),
            ProviderError::RateLimited {
                retry_after_secs: 0
            }
        ));
        assert!(matches!(
            map_http_error("c", 500, "b".into()),
            ProviderError::Network { .. }
        ));
        assert!(matches!(
            map_http_error("c", 418, "b".into()),
            ProviderError::Other { .. }
        ));
    }
}
