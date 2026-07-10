package com.xiwei.sujian.ui.compose.editor

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.viewinterop.AndroidView
import com.xiwei.sujian.editor.selfrender.SujianEditorView

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
