package com.xiwei.sujian.ui.compose.editor

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import com.xiwei.sujian.editor.v2.host.SujianEditorView

@Composable
fun SujianEditorHost(
    projectId: String,
    volumeId: String,
    chapterId: String,
    chapterTitle: String,
    modifier: Modifier = Modifier
) {
    WritingPane(
        projectId = projectId,
        volumeId = volumeId,
        chapterId = chapterId,
        chapterTitle = chapterTitle,
        modifier = modifier.fillMaxSize()
    )
}
