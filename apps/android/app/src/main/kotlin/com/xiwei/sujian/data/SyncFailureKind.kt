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
