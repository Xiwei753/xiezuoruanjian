package com.xiwei.sujian.ui.compose.editor

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.viewinterop.AndroidView
import com.xiwei.sujian.editor.selfrender.SujianEditorView

@Composable
fun SujianEditorHost(
    modifier: Modifier = Modifier
) {
    AndroidView(
        factory = { context ->
            SujianEditorView(context).apply {
                layoutParams = android.widget.FrameLayout.LayoutParams(
                    android.widget.FrameLayout.LayoutParams.MATCH_PARENT,
                    android.widget.FrameLayout.LayoutParams.MATCH_PARENT
                )
            }
        },
        modifier = modifier.fillMaxSize()
    )
}
