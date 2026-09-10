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
    assert!(ProviderError::Other { reason: "x".into() }.is_retryable());
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

/// 验证 ProviderError::is_retryable() 与 crate::Error::from(err).recoverable()
/// 对每个变体都返回相同值——provider 层与 core 层可恢复性语义必须一致，
/// engine 才能放心用 e.recoverable() 决定是否重试。
#[test]
fn retryable_matches_core_recoverable() {
    let cases: Vec<(ProviderError, bool)> = vec![
        (
            ProviderError::AuthFailed {
                reason: "bad token".into(),
            },
            false,
        ),
        (
            ProviderError::PermissionDenied {
                reason: "no write".into(),
            },
            false,
        ),
        (ProviderError::NotFound { path: "a/b".into() }, false),
        (
            ProviderError::PreconditionFailed {
                path: "a/b".into(),
                reason: "sha mismatch".into(),
            },
            false,
        ),
        (
            ProviderError::RateLimited {
                retry_after_secs: 30,
            },
            true,
        ),
        (
            ProviderError::Network {
                reason: "dns".into(),
            },
            true,
        ),
        (
            ProviderError::TemporaryUnavailable {
                reason: "5xx".into(),
            },
            true,
        ),
        (ProviderError::Other { reason: "x".into() }, true),
    ];

    for (provider_err, expected) in cases {
        assert_eq!(
            provider_err.is_retryable(),
            expected,
            "ProviderError is_retryable mismatch: {provider_err:?}"
        );
        let core_err = crate::Error::from(provider_err.clone());
        assert_eq!(
            core_err.recoverable(),
            expected,
            "core Error recoverable mismatch for {core_err:?}"
        );
        // 两层语义必须一致
        assert_eq!(
            provider_err.is_retryable(),
            core_err.recoverable(),
            "provider/core retryable semantic mismatch"
        );
    }
}
