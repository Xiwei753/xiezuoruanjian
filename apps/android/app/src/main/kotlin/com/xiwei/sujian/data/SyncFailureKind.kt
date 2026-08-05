package com.xiwei.sujian.data

enum class SyncFailureKind {
    RetryableNetwork,
    RetryableIo,
    Authentication,
    Conflict,
    DirtyRepository,
    Protocol,
    NativeUnavailable,
    Fatal;

    /**
     * #592 四：统一用户提示映射 — 正式同步、试运行、连接诊断全部使用同一 messageKey，
     * 不再各自拼接 dry_run_error / diagnostics_error。
     */
    fun messageKey(): String = when (this) {
        RetryableNetwork -> "sync_retryable_network"
        RetryableIo -> "sync_retryable_io"
        Authentication -> "sync_auth_failed"
        Conflict -> "sync_conflict"
        DirtyRepository -> "sync_dirty_repository"
        Protocol -> "sync_protocol_error"
        NativeUnavailable -> "sync_native_unavailable"
        Fatal -> "sync_fatal"
    }

    fun toOutcome(): SyncOutcome = when (this) {
        RetryableNetwork -> SyncOutcome.RetryableFailure(com.xiwei.sujian.model.SyncStatus.RecoverableError)
        RetryableIo -> SyncOutcome.RetryableFailure(com.xiwei.sujian.model.SyncStatus.RecoverableError)
        Authentication -> SyncOutcome.TerminalFailure(com.xiwei.sujian.model.SyncStatus.FatalError)
        Conflict -> SyncOutcome.TerminalFailure(com.xiwei.sujian.model.SyncStatus.Conflict)
        DirtyRepository -> SyncOutcome.TerminalFailure(com.xiwei.sujian.model.SyncStatus.DirtyRepoBlocked)
        Protocol -> SyncOutcome.TerminalFailure(com.xiwei.sujian.model.SyncStatus.FatalError)
        NativeUnavailable -> SyncOutcome.TerminalFailure(com.xiwei.sujian.model.SyncStatus.FatalError)
        Fatal -> SyncOutcome.TerminalFailure(com.xiwei.sujian.model.SyncStatus.FatalError)
    }

    companion object {
        fun fromErrorCode(code: String?): SyncFailureKind = when (code) {
            "SYNC_NETWORK_UNAVAILABLE", "SYNC_RATE_LIMITED" -> RetryableNetwork
            "IO_ERROR" -> RetryableIo
            "NATIVE_NOT_LOADED" -> NativeUnavailable
            "SYNC_AUTH_FAILED" -> Authentication
            "SYNC_CONFLICT", "SYNC_DOCUMENT_CONFLICT", "SYNC_CHECKOUT_CONFLICT",
            "SYNC_SETTINGS_CONFLICT", "SYNC_CONFLICT_DETECTED" -> Conflict
            "SYNC_NON_FAST_FORWARD", "SYNC_UNRELATED_HISTORIES",
            "SYNC_INCOMPLETE_TRANSACTION" -> Protocol
            else -> Fatal
        }
    }
}
