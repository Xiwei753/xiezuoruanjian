package com.xiwei.sujian.feature.project.ui

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import com.xiwei.sujian.app.presentation.layout.AndroidLayoutRect
import com.xiwei.sujian.app.presentation.layout.AndroidWorkbenchLayoutPlan
import com.xiwei.sujian.app.presentation.layout.AndroidWorkbenchRole
import com.xiwei.sujian.app.presentation.layout.WorkspaceLayoutMode

/**
 * #640 A：EditorPresentationHost 的 placement 模式 — 由 [resolveEditorPresentationHostMode] 决定。
 *
 * host 由 SujianNavigationSuite 持有，和 SujianNavScaffoldContent 作为稳定 sibling，
 * 预热与显示共用同一 AndroidView。target null 不组合；target 非空从预热开始持有唯一编辑器。
 *
 * - [HIDDEN]：target null，不组合，不遮盖 ProjectList/ChapterTree；
 * - [COMPACT_EDITOR]：窄屏，SinglePaneEditorLayer，最终 Editor chrome，无 bottom NavigationBar；
 * - [WIDE_EDITOR_ONLY]：宽屏 EditorPane-only（预热或 plan 非 WORKBENCH），按 Rust Editor bounds；
 * - [WIDE_FULL_WORKBENCH]：宽屏完整 Workbench shell（presentationVisible=true 且 plan=WORKBENCH）。
 */
internal enum class EditorPresentationHostMode {
    HIDDEN,
    COMPACT_EDITOR,
    WIDE_EDITOR_ONLY,
    WIDE_FULL_WORKBENCH,
}

/**
 * #640 A：host placement 决策 — 纯函数，无 Compose 副作用，可单测。
 *
 * target null → HIDDEN。窄屏（!isWideLayout）→ COMPACT_EDITOR。
 * 宽屏复用 [resolveWideWorkspaceCompositionMode]：EDITOR_ONLY → WIDE_EDITOR_ONLY，
 * FULL_WORKBENCH → WIDE_FULL_WORKBENCH。
 */
internal fun resolveEditorPresentationHostMode(
    target: PreparedEditorTarget?,
    isWideLayout: Boolean,
    workbenchPlan: AndroidWorkbenchLayoutPlan?,
    presentationVisible: Boolean,
): EditorPresentationHostMode {
    if (target == null) return EditorPresentationHostMode.HIDDEN
    if (!isWideLayout) return EditorPresentationHostMode.COMPACT_EDITOR
    val compositionMode =
        resolveWideWorkspaceCompositionMode(
            workbenchPlan = workbenchPlan,
            presentationVisible = presentationVisible,
        )
    return when (compositionMode) {
        WideWorkspaceCompositionMode.EDITOR_ONLY -> EditorPresentationHostMode.WIDE_EDITOR_ONLY
        WideWorkspaceCompositionMode.FULL_WORKBENCH -> EditorPresentationHostMode.WIDE_FULL_WORKBENCH
    }
}

/**
 * #640 A：target handoff 导航守卫 — 纯函数，无 Compose 副作用，可单测。
 *
 * openChapter 成功提交 target 后，suite 在 awaitPresentationReady 返回后调用本函数：
 * 仅当 ready=true 且 [currentTarget] 仍是 [requestedTarget]（未被新请求替换/清空）才导航。
 * 旧请求的 target 已被替换时 currentTarget != requestedTarget，旧 await 不抢导航。
 */
internal fun shouldNavigateAfterReady(
    currentTarget: PreparedEditorTarget?,
    requestedTarget: PreparedEditorTarget,
    isReady: Boolean,
): Boolean = isReady && currentTarget == requestedTarget

/**
 * #640 A：窄屏 host 最终 chrome — 背景决策。
 *
 * - [TRANSPARENT]：presentationVisible=false，预热阶段不画 opaque editor surface；
 * - [SHARED_EDITOR_SURFACE]：presentationVisible=true，用共享 editorSurfaceBackgroundColor。
 */
internal enum class CompactEditorBackground {
    TRANSPARENT,
    SHARED_EDITOR_SURFACE,
}

/**
 * #640 A：窄屏 host 最终 chrome 决策 — 纯函数，可单测。
 *
 * host 在 Scaffold 外（sibling），绝不有 bottom primary NavigationBar，
 * 不使用 ChapterTree Scaffold 的 innerPadding。背景由 [background] 决定。
 */
internal data class CompactEditorChrome(
    val showsPrimaryNavigation: Boolean,
    val usesChapterTreeInnerPadding: Boolean,
    val background: CompactEditorBackground,
)

/**
 * #640 A：窄屏 host chrome 决策 — 纯函数，无 Compose 副作用，可单测。
 *
 * showsPrimaryNavigation 恒 false（host 不画 NavigationBar）；
 * usesChapterTreeInnerPadding 恒 false（host 在 Scaffold 外）；
 * background：hidden → TRANSPARENT，visible → SHARED_EDITOR_SURFACE。
 */
internal fun compactEditorChrome(presentationVisible: Boolean): CompactEditorChrome =
    CompactEditorChrome(
        showsPrimaryNavigation = false,
        usesChapterTreeInnerPadding = false,
        background =
            if (presentationVisible) {
                CompactEditorBackground.SHARED_EDITOR_SURFACE
            } else {
                CompactEditorBackground.TRANSPARENT
            },
    )

/**
 * #640 A：宽屏 host Editor bounds 解析 — 纯函数，可单测。
 *
 * 直接取 Rust [AndroidWorkbenchLayoutPlan] 的 EDITOR 角色 bounds；
 * 根 host 不经过 outer top bar/NavigationRail。plan null 或 Editor bounds 空 → null
 * （调用方回落 fillMaxSize，无遮挡信息）。
 */
internal fun resolveWideEditorBounds(workbenchPlan: AndroidWorkbenchLayoutPlan?): AndroidLayoutRect? {
    if (workbenchPlan == null) return null
    val bounds = workbenchPlan.placementFor(AndroidWorkbenchRole.EDITOR)?.bounds ?: return null
    if (bounds.isEmpty) return null
    return bounds
}

/**
 * #640 A：EditorPresentationCallbacks — 由 ProjectWorkspaceScreen 构造并上传给 suite，
 * 供 EditorPresentationHost 宽屏 WideWritingWorkspace 的章节树/toolbar 使用。
 *
 * 不新建第二 ViewModel、第二 requestOpenChapter 或第二状态源 — 回调源自 ProjectWorkspaceScreen
 * 的 openChapter/onChapterSwitchFailed，经 SujianNavContext 上传到 suite 再传给 host。
 */
internal data class EditorPresentationCallbacks(
    val onChapterSwitch: (volumeId: String, chapterId: String, chapterTitle: String) -> Unit,
    val onChapterSwitchFailed: (
        (oldProjectId: String, oldVolumeId: String?, oldChapterId: String?, oldChapterTitle: String) -> Unit
    )?,
)

/**
 * #640 A：稳定唯一 Editor presentation host。
 *
 * 由 [com.xiwei.sujian.app.navigation.SujianNavigationSuite] 持有，和
 * [com.xiwei.sujian.app.navigation.SujianNavScaffoldContent] 作为稳定 sibling（Box 内 host 后画，上层）。
 * 预热与显示共用同一 AndroidView：切换 location 只 INVISIBLE 到 VISIBLE，不重建 View。
 *
 * - target null → 不组合（HIDDEN）；
 * - 窄屏 visible → [compactTopBar] + [SinglePaneEditorLayer]（最终 Editor chrome，无 bottom NavigationBar）；
 *   host 在 Scaffold 外，自己画最终 top bar，不经过 ChapterTree Scaffold 的 innerPadding；
 * - 窄屏 hidden → [SinglePaneEditorLayer]（背景透明，不画 top bar，不遮盖 ProjectList/ChapterTree）；
 * - 宽屏 → [WideWritingWorkspace]（内部按 [resolveWideWorkspaceCompositionMode] 决定
 *   EDITOR_ONLY 或 FULL_WORKBENCH，按 Rust Editor bounds measure/place）。
 *
 * 不新建第二 ViewModel、第二 requestOpenChapter 或第二状态源 — wide 回调由调用方
 * （ProjectWorkspaceScreen 经 SujianNavContext 上传）提供。
 */
@Composable
internal fun EditorPresentationHost(
    target: PreparedEditorTarget?,
    isWideLayout: Boolean,
    presentationVisible: Boolean,
    workbenchPlan: AndroidWorkbenchLayoutPlan?,
    compactTopBar: @Composable () -> Unit,
    wideDeps: WideWorkspaceDeps?,
    wideLayoutState: WideWorkspaceLayoutState?,
    wideCallbacks: WideWorkspaceCallbacks?,
    onChapterSwitchFailed: (
        (oldProjectId: String, oldVolumeId: String?, oldChapterId: String?, oldChapterTitle: String) -> Unit
    )?,
    modifier: Modifier = Modifier,
) {
    val mode =
        resolveEditorPresentationHostMode(
            target = target,
            isWideLayout = isWideLayout,
            workbenchPlan = workbenchPlan,
            presentationVisible = presentationVisible,
        )
    if (mode == EditorPresentationHostMode.HIDDEN) return
    val currentTarget = target ?: return

    when (mode) {
        EditorPresentationHostMode.COMPACT_EDITOR -> {
            if (presentationVisible) {
                // #640 A：窄屏 visible — 最终 top bar + 正文。host 在 Scaffold 外自己画 top bar，
                // 不经过 ChapterTree Scaffold 的 innerPadding，绝不有 bottom NavigationBar。
                androidx.compose.foundation.layout.Column(modifier = modifier.fillMaxSize()) {
                    compactTopBar()
                    SinglePaneEditorLayer(
                        target = currentTarget,
                        presentationVisible = true,
                        onChapterSwitchFailed = onChapterSwitchFailed,
                        modifier = Modifier.fillMaxSize(),
                    )
                }
            } else {
                // #640 A.4：预热阶段背景透明，不画 top bar，不画 opaque editor surface/Compose shell。
                SinglePaneEditorLayer(
                    target = currentTarget,
                    presentationVisible = false,
                    onChapterSwitchFailed = onChapterSwitchFailed,
                    modifier = modifier.fillMaxSize(),
                )
            }
        }
        EditorPresentationHostMode.WIDE_EDITOR_ONLY,
        EditorPresentationHostMode.WIDE_FULL_WORKBENCH,
        -> {
            // 宽屏：WideWritingWorkspace 内部用 resolveWideWorkspaceCompositionMode 决定
            // EDITOR_ONLY（预热/plan 非 WORKBENCH）或 FULL_WORKBENCH（可见+WORKBENCH）。
            // host 只传 target 构造的 documentState + presentationVisible，不重建 View。
            // 宽屏 top bar 由 Scaffold 画（SinglePane Editor）或 Workbench toolbar 自己画（FULL_WORKBENCH）。
            val deps = wideDeps ?: return
            val layoutState = wideLayoutState ?: return
            val callbacks = wideCallbacks ?: return
            val documentState =
                WideWorkspaceDocumentState(
                    currentProjectId = currentTarget.projectId,
                    currentVolumeId = currentTarget.volumeId,
                    currentChapterId = currentTarget.chapterId,
                    currentChapterTitle = currentTarget.chapterTitle,
                    presentationVisible = presentationVisible,
                )
            WideWritingWorkspace(
                deps = deps,
                documentState = documentState,
                layoutState = layoutState,
                callbacks = callbacks,
                modifier = modifier.fillMaxSize(),
            )
        }
        EditorPresentationHostMode.HIDDEN -> return
    }
}

/**
 * #640 A：从 [WorkspaceLayoutMode] 推导 isWideLayout — 供 suite 调用 host 时转换。
 */
internal fun WorkspaceLayoutMode.isWideLayout(): Boolean = this != WorkspaceLayoutMode.SINGLE_PANE
