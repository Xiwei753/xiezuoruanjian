package com.xiwei.sujian.storage.mirror

/**
 * 从 workspace backup 恢复旧正文到 Download 最终位置的结果。
 *
 * Issue #667：旧实现中 `ReadableMirrorStorage.restoreBackup()` 返回此类型，
 * 事务方法移到 [MirrorTransactionWorkspace] 后，由执行器内联组装此结果。
 */
internal sealed interface RestoreBackupResult {
    /** 旧正文已恢复到 final 位置。[ref] 是新创建文件的引用。 */
    data class Restored(val ref: MirrorFileRef) : RestoreBackupResult

    /** final 位置已存在且内容 hash 匹配预期，无需重复恢复。 */
    data class AlreadyRestored(val ref: MirrorFileRef) : RestoreBackupResult

    /** final 位置已存在但内容 hash 不匹配，状态冲突。 */
    data class Conflict(val ref: MirrorFileRef) : RestoreBackupResult

    /** 恢复失败（读取 backup 失败、创建文件失败等）。 */
    data class Failed(val cause: Throwable? = null) : RestoreBackupResult
}

/**
 * 章节回滚操作的结果。
 *
 * 由 [MirrorChapterRollbackExecutor.rollbackChapterToOldState] 返回，
 * 供 [MirrorRollbackExecutor] 和 [MirrorRecoveryExecutor] 消费。
 */
internal sealed interface RollbackItemResult {
    /**
     * 旧正文已恢复到 final 位置（#649 评论 5572554935 问题 2）。
     *
     * [ref] 是 restoreBackup 返回的真实 ref（URI 可能因 createText/createDocument 变化）。
     * 调用方必须把此 ref 写回 stateStore，否则 stateStore 仍保存失效 URI。
     */
    data class Restored(val ref: MirrorFileRef) : RollbackItemResult

    /**
     * 新建章节的本事务新文件已删除（无旧正文需要恢复）。
     */
    data object NewFileRemoved : RollbackItemResult

    data object StateUnknown : RollbackItemResult

    data object Failed : RollbackItemResult
}
