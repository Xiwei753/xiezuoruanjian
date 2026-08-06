package com.xiwei.sujian.data

enum class SyncFailureKind {
    RetryableNetwork,
    RetryableIo,
    Authentication,
    Conflict,
    DirtyRepository,
    Protocol,
    NativeUnavailable,
    /** #595 三：同步前活动正文保存失败 — 本地输入未落盘，同步必须中止。 */
    DocumentSaveFailed,
    /** #595 三：flush 时屏幕正文 revision 与已保存 revision 不一致（保存后又输入）。 */
    DocumentRevisionChanged,
    /** #595 三：文档版本冲突 — 需重新读取/三方合并，禁止盲目覆盖。 */
    DocumentConflict,
    /** #595 三：无法读取/验证活动文档状态。 */
    DocumentUnavailable,
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
        DocumentSaveFailed -> "sync_document_save_failed"
        DocumentRevisionChanged -> "sync_document_revision_changed"
        DocumentConflict -> "sync_document_conflict"
        DocumentUnavailable -> "sync_document_unavailable"
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
        DocumentSaveFailed -> SyncOutcome.TerminalFailure(com.xiwei.sujian.model.SyncStatus.FatalError, DocumentSaveFailed)
        DocumentRevisionChanged -> SyncOutcome.TerminalFailure(com.xiwei.sujian.model.SyncStatus.FatalError, DocumentRevisionChanged)
        DocumentConflict -> SyncOutcome.TerminalFailure(com.xiwei.sujian.model.SyncStatus.Conflict, DocumentConflict)
        DocumentUnavailable -> SyncOutcome.TerminalFailure(com.xiwei.sujian.model.SyncStatus.FatalError, DocumentUnavailable)
        Fatal -> SyncOutcome.TerminalFailure(com.xiwei.sujian.model.SyncStatus.FatalError, Fatal)
    }

    /**
     * #595 四：同步配置快照读取失败对后台任务的映射 — 只有明确可重试的
     * 临时故障（网络/IO/原生库未加载）返回 true；Fatal/协议/凭据/冲突类
     * 失败重试没有意义，Worker 直接按确定性失败结束。
     */
    fun isTransientReadFailure(): Boolean = when (this) {
        RetryableNetwork, RetryableIo, NativeUnavailable -> true
        Authentication, Conflict, DirtyRepository, Protocol, Fatal,
        DocumentSaveFailed, DocumentRevisionChanged, DocumentConflict, DocumentUnavailable -> false
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

        /**
         * #592 七：从 Bridge 边界的类型化失败直接推导；未知/非同步错误默认 Fatal，
         * 只有明确网络或 IO 失败可以重试。不再维护 Android 字符串错误码表。
         */
        fun fromBridgeError(error: com.xiwei.sujian.data.BridgeResult.Error): SyncFailureKind =
            error.syncFailureKind ?: Fatal

        /**
         * #592 七：遗留字符串错误码映射 — 仅用于非 BridgeResult.Error 路径的兜底。
         * 主分类路径已改为 [com.xiwei.sujian.data.BridgeResult.Error.syncFailureKind]
         * （WriterException 变体直接推导），新错误码不再依赖字符串表。
         */
        fun fromLegacyErrorCode(code: String?): SyncFailureKind = when (code) {
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
