package com.xiwei.sujian.feature.project.ui

import com.xiwei.sujian.app.presentation.layout.AndroidLayoutRect
import com.xiwei.sujian.app.presentation.layout.AndroidResolvedWorkspaceMode
import com.xiwei.sujian.app.presentation.layout.AndroidWorkbenchLayoutPlan
import com.xiwei.sujian.app.presentation.layout.AndroidWorkbenchPlacement
import com.xiwei.sujian.app.presentation.layout.AndroidWorkbenchRole
import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * #640 A：EditorPresentationHost placement 决策回归测试。
 *
 * host 必须由 SujianNavigationSuite 持有，和 SujianNavScaffoldContent 作为稳定 sibling。
 * placement 决策由纯函数 [resolveEditorPresentationHostMode] 统一表达，生产 host 与单测共用。
 *
 * - target null → HIDDEN（不组合，不遮盖 ProjectList/ChapterTree）；
 * - 窄屏 → COMPACT_EDITOR（SinglePaneEditorLayer，最终 Editor chrome，无 bottom NavigationBar）；
 * - 宽屏 + presentationVisible=false → WIDE_EDITOR_ONLY（EditorPane-only，不进完整 Workbench shell）；
 * - 宽屏 + presentationVisible=true + plan=WORKBENCH → WIDE_FULL_WORKBENCH；
 * - 宽屏 + plan=null/SINGLE_PANE → WIDE_EDITOR_ONLY。
 */
class EditorPresentationHostPlacementTest {
    private val target =
        PreparedEditorTarget(
            projectId = "p",
            projectTitle = "Title",
            volumeId = "v",
            chapterId = "c",
            chapterTitle = "C",
        )

    @Test
    fun nullTarget_returnsHidden() {
        assertEquals(
            EditorPresentationHostMode.HIDDEN,
            resolveEditorPresentationHostMode(
                target = null,
                isWideLayout = false,
                workbenchPlan = null,
                presentationVisible = false,
            ),
        )
    }

    @Test
    fun targetNotNull_wideAndVisible_workbenchPlan_returnsFullWorkbench() {
        assertEquals(
            EditorPresentationHostMode.WIDE_FULL_WORKBENCH,
            resolveEditorPresentationHostMode(
                target = target,
                isWideLayout = true,
                workbenchPlan = workbenchPlan(AndroidResolvedWorkspaceMode.WORKBENCH),
                presentationVisible = true,
            ),
        )
    }

    @Test
    fun compact_targetNotNull_returnsCompactEditor() {
        assertEquals(
            EditorPresentationHostMode.COMPACT_EDITOR,
            resolveEditorPresentationHostMode(
                target = target,
                isWideLayout = false,
                workbenchPlan = null,
                presentationVisible = false,
            ),
        )
    }

    @Test
    fun compact_targetNotNull_returnsCompactEditor_evenWhenVisible() {
        assertEquals(
            EditorPresentationHostMode.COMPACT_EDITOR,
            resolveEditorPresentationHostMode(
                target = target,
                isWideLayout = false,
                workbenchPlan = null,
                presentationVisible = true,
            ),
        )
    }

    @Test
    fun wide_hidden_presentationVisibleFalse_returnsEditorOnly() {
        assertEquals(
            EditorPresentationHostMode.WIDE_EDITOR_ONLY,
            resolveEditorPresentationHostMode(
                target = target,
                isWideLayout = true,
                workbenchPlan = workbenchPlan(AndroidResolvedWorkspaceMode.WORKBENCH),
                presentationVisible = false,
            ),
        )
    }

    @Test
    fun wide_visible_workbenchPlan_returnsFullWorkbench() {
        assertEquals(
            EditorPresentationHostMode.WIDE_FULL_WORKBENCH,
            resolveEditorPresentationHostMode(
                target = target,
                isWideLayout = true,
                workbenchPlan = workbenchPlan(AndroidResolvedWorkspaceMode.WORKBENCH),
                presentationVisible = true,
            ),
        )
    }

    @Test
    fun wide_visible_singlePanePlan_returnsEditorOnly() {
        assertEquals(
            EditorPresentationHostMode.WIDE_EDITOR_ONLY,
            resolveEditorPresentationHostMode(
                target = target,
                isWideLayout = true,
                workbenchPlan = workbenchPlan(AndroidResolvedWorkspaceMode.SINGLE_PANE),
                presentationVisible = true,
            ),
        )
    }

    @Test
    fun wide_visible_nullPlan_returnsEditorOnly() {
        assertEquals(
            EditorPresentationHostMode.WIDE_EDITOR_ONLY,
            resolveEditorPresentationHostMode(
                target = target,
                isWideLayout = true,
                workbenchPlan = null,
                presentationVisible = true,
            ),
        )
    }

    private fun workbenchPlan(mode: AndroidResolvedWorkspaceMode): AndroidWorkbenchLayoutPlan =
        AndroidWorkbenchLayoutPlan(
            placements =
                listOf(
                    AndroidWorkbenchPlacement(
                        role = AndroidWorkbenchRole.EDITOR,
                        bounds = AndroidLayoutRect(leftDp = 0f, topDp = 0f, rightDp = 100f, bottomDp = 100f),
                    ),
                ),
            mode = mode,
        )
}
