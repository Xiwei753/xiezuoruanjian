package com.xiwei.sujian.feature.editor.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.viewinterop.AndroidView
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.xiwei.sujian.feature.editor.session.WindowBindingState
import com.xiwei.sujian.feature.editor.ui.theme.EditorThemeAdapter
import com.xiwei.sujian.feature.editor.window.EditorWindowHost

/**
 * #640 A.3：活动 target 从首帧组合唯一 AndroidView，
 * 用 View.INVISIBLE（不是 GONE/alpha/AnimatedVisibility）控制可见性。
 * 非活动 target 显示只读预览。
 */
enum class EditorSurfaceMode {
    /** 当前窗口绑定该 target 且状态为 Attaching/Attached/Committing/Cancelling → 真实编辑器。 */
    EditorHost,

    /** 非活动章节 → 只读预览（ReadonlyChapterPreview）。 */
    Preview,
}

/**
 * #640 A.3：正文 Surface 渲染决策 — 纯函数。
 *
 * 活动 target 始终组合 AndroidView（EditorSurfaceMode.EditorHost），
 * 用 View.INVISIBLE 控制可见性，不画第二套正文 renderer。
 * 非活动 target 显示只读预览（EditorSurfaceMode.Preview）。
 */
fun editorSurfaceMode(
    bindingState: WindowBindingState,
    windowId: String,
    targetId: String,
    isActivePane: Boolean,
): EditorSurfaceMode {
    val editorMatch =
        when (bindingState) {
            is WindowBindingState.Attaching ->
                bindingState.windowId == windowId && bindingState.targetId == targetId
            is WindowBindingState.Attached ->
                bindingState.windowId == windowId && bindingState.targetId == targetId
            is WindowBindingState.Committing -> bindingState.targetId == targetId
            is WindowBindingState.Cancelling -> bindingState.targetId == targetId
            WindowBindingState.Idle,
            is WindowBindingState.Detaching,
            is WindowBindingState.Detached,
            -> false
        }
    return when {
        editorMatch -> EditorSurfaceMode.EditorHost
        isActivePane -> EditorSurfaceMode.EditorHost // #640：活动 target 始终组合 AndroidView
        else -> EditorSurfaceMode.Preview
    }
}

/**
 * #595 一：正文编辑器宿主 — 在 [WritingPane] 的正文 Box 内直接持有
 * [AndroidView]([SujianEditorView])。
 *
 * AndroidView 的大小由父 Compose 布局直接决定（[Modifier.fillMaxSize]）,
 * 使用局部坐标，不再通过 boundsInWindow()、全屏 slot、graphicsLayer 追踪正文。
 *
 * #630 R12：渲染决策改为消费 [EditorSurfaceMode]，同时看 bindingState 和业务层传入的 isActivePane：
 * - [EditorSurfaceMode.EditorHost]：显示 SujianEditorView（唯一正文 renderer）。
 * - [EditorSurfaceMode.Preview]：非活动 target → 显示 ReadonlyChapterPreview（只读预览）。
 *
 * #595 八/十一：直接消费规范窗口绑定状态机 [WindowBindingState]（会话层唯一事实源），
 * 用生命周期感知收集 [collectAsStateWithLifecycle] 观察 bindingStateFlow；
 * 不再存在第二套 EditorAttachmentState 派生类型。临时失焦（动画暂停）不会改变
 * binding 状态 — Attached 时编辑器始终显示，暂停/恢复由 View 内部处理。
 *
 * #640 A.5：新增 presentationVisible — 控制编辑器 View 可见性，不参与 session 业务判断。
 */
@Composable
fun WritingEditorSurface(
    coordinator: EditorWindowHost,
    targetId: String,
    isActivePane: Boolean,
    modifier: Modifier = Modifier,
    /** #640 A.5：presentationVisible — 控制编辑器 View 的可见性，不参与 session 业务判断。 */
    presentationVisible: Boolean = true,
) {
    // #595 三：只收集会话层唯一 [sessionStateFlow]，从同一个快照读取 bindingState。
    // 三个独立 stateIn 派生流已删除 — 同一帧内 activeTargetId / editingState /
    // bindingState / sessionId 永远来自同一个不可变快照，不会读到跨帧组合。
    val sessionState by coordinator.sessionStateFlow.collectAsStateWithLifecycle()
    val bindingState = sessionState.bindingState
    // #640 A.7：收集 presentationReadyTargetId — StateFlow 更新触发 AndroidView.update 的可见性重组
    val readyTargetId by coordinator.presentationReadyTargetId.collectAsStateWithLifecycle()
    // #630 R12：渲染决策同时消费 bindingState 和业务层传入的 isActivePane —
    // 活动章节从进入页面到稳定显示必须只有 SujianEditorView 一套正文 renderer。
    val surfaceMode = editorSurfaceMode(bindingState, coordinator.windowId, targetId, isActivePane)

    val themeColors = EditorThemeAdapter.extractColors()

    Box(modifier = modifier) {
        when (surfaceMode) {
            EditorSurfaceMode.EditorHost -> {
                // #595 三：AndroidView 正式拥有 View 生命周期 —
                // factory 用传入的 Context 创建 View（Compose 官方模型），
                // 不返回宿主提前创建、长期缓存的 View。
                // 普通正文 Surface 不是 Lazy 列表 View 池复用场景，删除 onReset。
                // onRelease 完整解绑双向引用、InputConnection、FrameClock 和 callback。
                // #640 A.3：活动 target 始终组合 AndroidView，
                // 用 View.INVISIBLE（不是 GONE/alpha/AnimatedVisibility）控制可见性。
                AndroidView(
                    factory = { ctx ->
                        val view = coordinator.createWindowView(ctx)
                        coordinator.attachView(coordinator.windowId, targetId, view)
                        EditorThemeAdapter.applyToView(view, themeColors)
                        view
                    },
                    update = { view ->
                        coordinator.updateView(view, themeColors)
                        // #640 A.3：统一主题和 visibility = VISIBLE 仅当 presentationVisible && isPresentationReady
                        // 否则 INVISIBLE；绝不用 alpha/动画假隐藏。
                        // presentationVisible 不参与 session 业务判断（#640 A.5）。
                        // #640 A.7：用 readyTargetId（已收集为 State）触发 recomposition，
                        // 避免 StateFlow 更新不触发 AndroidView.update 的可见性重组。
                        val isReady = readyTargetId == targetId
                        view.visibility =
                            if (presentationVisible && isReady) {
                                android.view.View.VISIBLE
                            } else {
                                android.view.View.INVISIBLE
                            }
                    },
                    onRelease = { view ->
                        coordinator.detachView(coordinator.windowId, targetId, view)
                        view.release()
                    },
                    modifier =
                        Modifier
                            .fillMaxSize()
                            // #597 九：正文出现的稳定语义 ID（页面测试不靠文本找正文）。
                            .testTag(com.xiwei.sujian.core.designsystem.testing.SujianSemanticIds.EditorContent),
                )
            }
            EditorSurfaceMode.Preview -> {
                // #595 九 / #630 R12：非活动章节 → 只读预览（ReadonlyChapterPreview）。
                val previewState = coordinator.getChapterPreviewState(targetId)
                if (previewState != null && previewState.text.isNotEmpty()) {
                    com.xiwei.sujian.feature.editor.ui.ReadonlyChapterPreview(previewState = previewState)
                }
            }
        }
    }
}

/**
 * #624 评论16 问题3：confirmEditorAttached 的决策 — 只有 [WindowBindingState.Attached]
 * 且 windowId + targetId 都匹配才返回 true。
 *
 * - Attached 且匹配 → true（真正 View 已绑定，解除输入冻结）；
 * - Attaching → false（等待 AndroidView factory/attachView() 推进到 Attached，不解除冻结）；
 * - Idle/Detached → false（beginEdit 发起绑定，不解除冻结）；
 * - Attached 但 windowId/targetId 不匹配 → false（残留自其他窗口的绑定）。
 */
fun shouldConfirmEditorAttached(
    bindingState: WindowBindingState,
    windowId: String,
    targetId: String,
): Boolean =
    bindingState is WindowBindingState.Attached &&
        bindingState.windowId == windowId &&
        bindingState.targetId == targetId
