package com.xiwei.sujian.feature.editor.ui

/**
 * #624 评论14 第2项：EditorViewModel 测试共享辅助。
 *
 * [enterChapterForTest] 替代已删除的 initChapter — 直接设置 currentSession + 稳定
 * _uiState（loading=false），等价于 initChapter + 等待后台加载落定。生产代码不再
 * 有 initChapter 后台加载（切换事务改用 loadChapterForSwitch 纯读取 + switchCommit
 * commit 后发布）。
 */
internal fun EditorViewModel.enterChapterForTest(
    projectId: String,
    volumeId: String,
    chapterId: String,
    chapterTitle: String,
) {
    currentSession =
        EditorSession(
            sessionId = java.util.UUID.randomUUID().toString(),
            projectId = projectId,
            volumeId = volumeId,
            chapterId = chapterId,
        )
    syncMergeEmitDedup.reset()
    _uiState.value =
        _uiState.value.copy(
            loading = false,
            chapterTitle = chapterTitle,
            editorEnabled = true,
            saveStatus = SaveStatus.Idle,
        )
    startSaveActor()
    reloadSettings()
}
