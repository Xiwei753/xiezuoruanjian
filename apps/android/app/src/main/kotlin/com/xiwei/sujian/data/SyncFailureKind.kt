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
        RetryableNetwork -> SyncOutcome.RetryableFailure(com.xiwei.sujian.model.SyncStatus.RecoverableError, RetryableNetwork)
        RetryableIo -> SyncOutcome.RetryableFailure(com.xiwei.sujian.model.SyncStatus.RecoverableError, RetryableIo)
        Authentication -> SyncOutcome.TerminalFailure(com.xiwei.sujian.model.SyncStatus.FatalError, Authentication)
        Conflict -> SyncOutcome.TerminalFailure(com.xiwei.sujian.model.SyncStatus.Conflict, Conflict)
        DirtyRepository -> SyncOutcome.TerminalFailure(com.xiwei.sujian.model.SyncStatus.DirtyRepoBlocked, DirtyRepository)
        Protocol -> SyncOutcome.TerminalFailure(com.xiwei.sujian.model.SyncStatus.FatalError, Protocol)
        NativeUnavailable -> SyncOutcome.TerminalFailure(com.xiwei.sujian.model.SyncStatus.FatalError, NativeUnavailable)
        Fatal -> SyncOutcome.TerminalFailure(com.xiwei.sujian.model.SyncStatus.FatalError, Fatal)
    }

    companion object {
        /**
         * #592 三：Core 返回的 SyncResult.status 到失败类型的映射。
         * 用于正式同步成功路径中 Core 报告的失败状态，与 Bridge 错误码路径共用同一类型。
         */
        fun fromSyncStatus(status: com.xiwei.sujian.model.SyncStatus): SyncFailureKind = when (status) {
            com.xiwei.sujian.model.SyncStatus.RecoverableError -> RetryableNetwork
            com.xiwei.sujian.model.SyncStatus.Error -> Fatal
            com.xiwei.sujian.model.SyncStatus.Conflict,
            com.xiwei.sujian.model.SyncStatus.PartialConflict -> Conflict
            com.xiwei.sujian.model.SyncStatus.DirtyRepoBlocked -> DirtyRepository
            com.xiwei.sujian.model.SyncStatus.FatalError -> Fatal
            else -> Fatal
        }

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
