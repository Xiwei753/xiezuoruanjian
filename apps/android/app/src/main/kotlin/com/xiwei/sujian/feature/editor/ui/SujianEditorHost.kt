package com.xiwei.sujian.feature.editor.ui

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier

@Composable
@Suppress("LongParameterList")
fun SujianEditorHost(
    projectId: String,
    volumeId: String,
    chapterId: String,
    chapterTitle: String,
    modifier: Modifier = Modifier,
    /** #595 一：章节切换事务失败回滚回调 — 透传给 [WritingPane]。 */
    onChapterSwitchFailed: (
        (oldProjectId: String, oldVolumeId: String?, oldChapterId: String?, oldChapterTitle: String) -> Unit
    )? = null,
) {
    WritingPane(
        projectId = projectId,
        volumeId = volumeId,
        chapterId = chapterId,
        chapterTitle = chapterTitle,
        modifier = modifier.fillMaxSize(),
        onChapterSwitchFailed = onChapterSwitchFailed,
    )
}
