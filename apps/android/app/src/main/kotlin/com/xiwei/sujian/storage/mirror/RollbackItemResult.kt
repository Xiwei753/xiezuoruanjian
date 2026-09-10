package com.xiwei.sujian.storage.mirror

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
