package com.xiwei.sujian.editor.v2.compose

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.lifecycle.compose.collectAsStateWithLifecycle
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
    val activeTargetId = coordinator.activeTargetIdFlow.collectAsStateWithLifecycle().value
    val editingState = coordinator.editingStateFlow.collectAsStateWithLifecycle().value
    val isActiveTarget = activeTargetId == targetId
    val isEditing = isActiveTarget &&
        (editingState == EditingState.BINDING || editingState == EditingState.EDITING)

    // #595 八：消费 EditorAttachmentState 决定渲染策略。
    // Attached/Attaching/Paused → 显示编辑器；Detached/Idle → 显示预览。
    // attachmentState 从规范 windowBindingState 派生，叠加窗口级暂停标志。
    val attachmentState = coordinator.attachmentState

    val themeColors = EditorThemeAdapter.extractColors()

    Box(modifier = modifier) {
        if (isEditing) {
            // #595 三：AndroidView 正式拥有 View 生命周期 —
            // factory 用传入的 Context 创建 View（Compose 官方模型），
            // 不返回宿主提前创建、长期缓存的 View。
            // 普通正文 Surface 不是 Lazy 列表 View 池复用场景，删除 onReset。
            // onRelease 完整解绑双向引用、InputConnection、FrameClock 和 callback。
            AndroidView(
                factory = { ctx ->
                    val view = coordinator.createWindowView(ctx)
                    coordinator.attachView(coordinator.windowId, targetId, view)
                    EditorThemeAdapter.applyToView(view, themeColors)
                    view
                },
                update = { view ->
                    coordinator.updateView(view, themeColors)
                },
                onRelease = { view ->
                    coordinator.detachView(coordinator.windowId, targetId, view)
                    view.release()
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
