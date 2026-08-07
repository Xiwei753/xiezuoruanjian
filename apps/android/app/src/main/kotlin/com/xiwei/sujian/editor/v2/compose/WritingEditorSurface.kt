package com.xiwei.sujian.editor.v2.compose

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.viewinterop.AndroidView
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.xiwei.sujian.editor.v2.coordinator.EditorWindowHost
import com.xiwei.sujian.editor.v2.coordinator.WindowBindingState
import com.xiwei.sujian.ui.compose.theme.EditorThemeAdapter

/**
 * #595 一：正文编辑器宿主 — 在 [WritingPane] 的正文 Box 内直接持有
 * [AndroidView]([SujianEditorView])。
 *
 * AndroidView 的大小由父 Compose 布局直接决定（[Modifier.fillMaxSize]），
 * 使用局部坐标，不再通过 boundsInWindow()、全屏 slot、graphicsLayer 追踪正文。
 *
 * - 活动目标：显示 SujianEditorView（唯一视觉运行时）。
 * - 非活动目标：显示 ReadonlyChapterPreview（只读预览，纯静态 ChapterPreviewState，
 *   不复用活动编辑器的动画 runtime）。
 *
 * #595 八/十一：直接消费规范窗口绑定状态机 [WindowBindingState]（会话层唯一事实源），
 * 用生命周期感知收集 [collectAsStateWithLifecycle] 观察 bindingStateFlow；
 * 不再存在第二套 EditorAttachmentState 派生类型。临时失焦（动画暂停）不会改变
 * binding 状态 — Attached 时编辑器始终显示，暂停/恢复由 View 内部处理。
 */
@Composable
fun WritingEditorSurface(
    coordinator: EditorWindowHost,
    targetId: String,
    modifier: Modifier = Modifier,
) {
    // #595 三：只收集会话层唯一 [sessionStateFlow]，从同一个快照读取 bindingState。
    // 三个独立 stateIn 派生流已删除 — 同一帧内 activeTargetId / editingState /
    // bindingState / sessionId 永远来自同一个不可变快照，不会读到跨帧组合。
    val sessionState by coordinator.sessionStateFlow.collectAsStateWithLifecycle()
    val bindingState = sessionState.bindingState
    val showEditor = shouldShowEditor(bindingState, targetId)

    val themeColors = EditorThemeAdapter.extractColors()

    Box(modifier = modifier) {
        if (showEditor) {
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
                modifier =
                    Modifier
                        .fillMaxSize()
                        // #597 九：正文出现的稳定语义 ID（页面测试不靠文本找正文）。
                        .testTag(com.xiwei.sujian.designsystem.testing.SujianSemanticIds.EditorContent),
            )
        } else {
            // #595 九：预览用纯静态 ChapterPreviewState，不经 TargetDisplayRuntime。
            val previewState = coordinator.getChapterPreviewState(targetId)
            if (previewState != null && previewState.text.isNotEmpty()) {
                com.xiwei.sujian.ui.compose.editor.ReadonlyChapterPreview(previewState = previewState)
            }
        }
    }
}

/**
 * #595 八：正文 Surface 的渲染决策 — 窗口绑定状态机到"显示编辑器/预览"的纯函数。
 *
 * - [WindowBindingState.Attaching]/[Attached]：窗口已绑定该 target → 编辑器。
 * - [WindowBindingState.Committing]/[Cancelling]：编辑事务收尾中，编辑器保持显示。
 * - [WindowBindingState.Idle]/[Detaching]/[Detached]：未绑定/已解绑 → 预览。
 */
fun shouldShowEditor(
    bindingState: WindowBindingState,
    targetId: String,
): Boolean =
    when (bindingState) {
        is WindowBindingState.Attaching -> bindingState.targetId == targetId
        is WindowBindingState.Attached -> bindingState.targetId == targetId
        is WindowBindingState.Committing -> bindingState.targetId == targetId
        is WindowBindingState.Cancelling -> bindingState.targetId == targetId
        WindowBindingState.Idle -> false
        is WindowBindingState.Detaching -> false
        is WindowBindingState.Detached -> false
    }
