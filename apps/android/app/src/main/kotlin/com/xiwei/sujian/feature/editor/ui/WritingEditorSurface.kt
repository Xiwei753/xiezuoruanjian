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
 * #630 R12：正文 Surface 的渲染模式。
 *
 * 活动章节从进入页面到稳定显示必须只有一套正文 renderer（SujianEditorView）。
 * - [Editor]：真实编辑器 View，窗口已绑定且状态正确。
 * - [Preview]：非活动章节只读预览（BasicText），不进入编辑器 Surface。
 * - [Pending]：活动 target 但尚未进入真实 View（等待 settingsReady / beginEdit / attach），
 *   保持空白，绝不用第二套正文 renderer 顶一帧。
 */
enum class EditorSurfaceMode {
    /** 当前窗口绑定该 target 且状态为 Attaching/Attached/Committing/Cancelling → 真实编辑器。 */
    Editor,

    /** 非活动章节 → 只读预览（ReadonlyChapterPreview）。 */
    Preview,

    /** 活动 target 但尚未进入真实 View → 保持空白，不画第二套正文 renderer。 */
    Pending,
}

/**
 * #630 R12：正文 Surface 渲染决策 — 纯函数。
 *
 * 同时消费 bindingState 和 activeTargetId，规则：
 * - 当前窗口绑定该 target 且状态为 Attaching/Attached/Committing/Cancelling → [EditorSurfaceMode.Editor]。
 * - 非 activeTarget → [EditorSurfaceMode.Preview]。
 * - 是 activeTarget 但未进入真实 View（Idle/Detached/Detaching）→ [EditorSurfaceMode.Pending]，绝不 Preview。
 *
 * 旧 window 的 Attached 不得冒充当前 Editor：若 target 仍是 active → Pending，不画 Preview。
 */
fun editorSurfaceMode(
    bindingState: WindowBindingState,
    windowId: String,
    targetId: String,
    activeTargetId: String?,
): EditorSurfaceMode {
    val isActiveTarget = activeTargetId == targetId
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
        editorMatch -> EditorSurfaceMode.Editor
        isActiveTarget -> EditorSurfaceMode.Pending
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
 * #630 R12：渲染决策改为消费 [EditorSurfaceMode]，同时看 bindingState 和 activeTargetId：
 * - [EditorSurfaceMode.Editor]：显示 SujianEditorView（唯一正文 renderer）。
 * - [EditorSurfaceMode.Preview]：非活动 target → 显示 ReadonlyChapterPreview（只读预览）。
 * - [EditorSurfaceMode.Pending]：活动 target 但尚未进入真实 View → 保持空白，不用第二套 renderer。
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
    val activeTargetId = sessionState.activeTargetId
    // #630 R12：渲染决策同时消费 bindingState 和 activeTargetId —
    // 活动章节从进入页面到稳定显示必须只有 SujianEditorView 一套正文 renderer。
    val surfaceMode = editorSurfaceMode(bindingState, coordinator.windowId, targetId, activeTargetId)

    val themeColors = EditorThemeAdapter.extractColors()

    Box(modifier = modifier) {
        when (surfaceMode) {
            EditorSurfaceMode.Editor -> {
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
            EditorSurfaceMode.Pending -> {
                // #630 R12：活动 target 但尚未进入真实 View —
                // 保持空白，绝不用第二套正文 renderer（ReadonlyChapterPreview）顶一帧。
                // 防止活动章节在 settingsReady / beginEdit / attach 期间先画一遍预览排版，
                // 再切换到 SujianEditorView 的真实排版，造成正文乱跳。
            }
        }
    }
}

/**
 * #595 八 / #630 R12: 正文 Surface 的渲染决策 — 窗口绑定状态机到 [EditorSurfaceMode] 的纯函数。
 *
 * 已被 [editorSurfaceMode] 取代，保留为向后兼容别名（内部调用 [editorSurfaceMode]）。
 *
 * - [WindowBindingState.Attaching]/[Attached]：窗口已绑定该 target → [EditorSurfaceMode.Editor]。
 *   #623 评论5：必须同时匹配 windowId + targetId — 残留自其他窗口的绑定
 *   （旧窗口 release 与新窗口附着之间的竞态）对新窗口不算已绑定。
 * - [WindowBindingState.Committing]/[Cancelling]：编辑事务收尾中，编辑器保持显示。
 * - [WindowBindingState.Idle]/[Detaching]/[Detached]：未绑定/已解绑。
 */
fun shouldShowEditor(
    bindingState: WindowBindingState,
    windowId: String,
    targetId: String,
): Boolean =
    editorSurfaceMode(
        bindingState,
        windowId,
        targetId,
        activeTargetId = null,
    ) == EditorSurfaceMode.Editor

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
