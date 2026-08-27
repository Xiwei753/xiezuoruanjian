package com.xiwei.sujian.feature.project.ui

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #640 A：target handoff 决策回归测试。
 *
 * PreparedEditorTarget 从 ProjectWorkspaceScreen 上移到 SujianNavigationSuite 后，
 * openChapter 成功只提交 target；suite 在 awaitPresentationReady 成功且 target 仍当前
 * 才 selectChapter + navigateToEditor。导航守卫由纯函数 [shouldNavigateAfterReady]
 * 统一表达，生产 suite 与单测共用。
 *
 * - ready=false → 永不导航（即使 target 仍当前）；
 * - ready=true 但 currentTarget 已变（被新请求替换）→ 不导航（旧请求不抢导航）；
 * - ready=true 且 currentTarget 仍是 requestedTarget → 导航。
 */
class EditorPresentationTargetHandoffTest {
    private val target =
        PreparedEditorTarget(
            projectId = "p",
            projectTitle = "Title",
            volumeId = "v",
            chapterId = "c",
            chapterTitle = "C",
        )

    @Test
    fun readyFalse_neverNavigates_evenWhenTargetCurrent() {
        assertFalse(
            shouldNavigateAfterReady(
                currentTarget = target,
                requestedTarget = target,
                isReady = false,
            ),
        )
    }

    @Test
    fun readyTrue_targetReplaced_doesNotNavigate() {
        val newer = target.copy(chapterId = "c2")
        assertFalse(
            shouldNavigateAfterReady(
                currentTarget = newer,
                requestedTarget = target,
                isReady = true,
            ),
        )
    }

    @Test
    fun readyTrue_targetCleared_doesNotNavigate() {
        assertFalse(
            shouldNavigateAfterReady(
                currentTarget = null,
                requestedTarget = target,
                isReady = true,
            ),
        )
    }

    @Test
    fun readyTrue_targetCurrent_navigates() {
        assertTrue(
            shouldNavigateAfterReady(
                currentTarget = target,
                requestedTarget = target,
                isReady = true,
            ),
        )
    }

    @Test
    fun preparedEditorTarget_carriesProjectTitleForNavigate() {
        val withTitle = target.copy(projectTitle = "新标题")
        assertEquals("新标题", withTitle.projectTitle)
        assertEquals(target.targetId, withTitle.targetId)
    }
}
