package com.xiwei.sujian.editor.v2.compose

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.viewinterop.AndroidView
import com.xiwei.sujian.editor.v2.coordinator.EditorWindowHost
import com.xiwei.sujian.editor.v2.coordinator.EditingState
import com.xiwei.sujian.editor.v2.host.SujianEditorView
import com.xiwei.sujian.ui.compose.theme.EditorThemeAdapter

/**
 * #595 一：正文编辑器宿主 — 在 [WritingPane] 的正文 Box 内直接持有
 * [AndroidView]([SujianEditorView])。
 *
 * AndroidView 的大小由父 Compose 布局直接决定（[Modifier.fillMaxSize]），
 * 使用局部坐标，不再通过 boundsInWindow()、全屏 slot、graphicsLayer 追踪正文。
 *
 * - 活动目标：显示 SujianEditorView（唯一视觉运行时）。
 * - 非活动目标：显示 ReadonlyChapterPreview（只读预览，不复用活动编辑器的动画 runtime）。
 */
@Composable
fun WritingEditorSurface(
    coordinator: EditorWindowHost,
    targetId: String,
    modifier: Modifier = Modifier,
) {
    val isActiveTarget = coordinator.activeTargetId == targetId
    val editingState = coordinator.editingState
    val isEditing = isActiveTarget &&
        (editingState == EditingState.BINDING || editingState == EditingState.EDITING)

    val themeColors = EditorThemeAdapter.extractColors()

    Box(modifier = modifier) {
        if (isEditing) {
            AndroidView(
                factory = { ctx ->
                    val view = coordinator.obtainSharedEditorView()
                    EditorThemeAdapter.applyToView(view, themeColors)
                    view
                },
                update = { view ->
                    EditorThemeAdapter.applyToView(view, themeColors)
                    view.visibility = android.view.View.VISIBLE
                    if (view.width > 0 && view.height > 0) {
                        coordinator.updateHostGeometry(view.width.toFloat(), view.height.toFloat())
                    }
                },
                onReset = { view ->
                    view.resetForReuse()
                },
                onRelease = { view ->
                    view.visibility = android.view.View.GONE
                },
                modifier = Modifier.fillMaxSize(),
            )
        } else {
            val projection = coordinator.getTargetProjection(targetId)
            if (projection != null && projection.getText().isNotEmpty()) {
                com.xiwei.sujian.ui.compose.editor.ReadonlyChapterPreview(projection = projection)
            }
        }
    }
}
