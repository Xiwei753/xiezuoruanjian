package com.xiwei.sujian.feature.editor.session

/**
 * #624 评论17 问题5：test-only helper — 生产 markSaved(targetId, savedVersion) 已删除
 * （正常保存和切章保存都走 commitSavedLease）。测试用此 helper 直接写 store 记录的
 * committedVersion/sessionBaseVersion/lastSavedVersion/localDirty 设置"已保存"前置状态，
 * 与旧 markSaved 语义等价，供版本因果/dirty 冲突/保存回执等测试复用。
 */
@Suppress("TopLevelFunctionName") // 与已删除的生产入口同名，仅测试可见
internal fun EditorSessionCoordinator.markSaved(
    targetId: String,
    savedVersion: DocumentVersion,
) {
    if (savedVersion.isEmpty) return
    mutateSession {
        val rec = record(targetId)
        if (rec != null) {
            putRecord(
                rec.withDocumentState {
                    it.copy(
                        committedVersion = savedVersion,
                        sessionBaseVersion = savedVersion,
                        lastSavedVersion = savedVersion,
                        localDirty = false,
                    )
                },
            )
        }
        if (sessionState.targetId == targetId) {
            sessionState =
                sessionState.copy(
                    committedVersion = savedVersion,
                    sessionBaseVersion = savedVersion,
                    localDirty = false,
                )
        }
    }
}
