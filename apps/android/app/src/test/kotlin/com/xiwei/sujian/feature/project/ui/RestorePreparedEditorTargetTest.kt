package com.xiwei.sujian.feature.project.ui

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #640：会话恢复/深链 restore target 单测。
 *
 * 冷启动/深链/进程恢复场景：rememberSujianWorkspaceNavState 可从 appState 三元 ID 恢复到
 * WorkspaceLocation.Editor，但此时没有 ProjectWorkspaceScreen.openChapter 点击来提交 target，
 * 于是 EditorPresentationHost target=null 不组合，直接空白。
 *
 * restorePreparedEditorTarget 从 appState 字段构造 PreparedEditorTarget，让稳定 host 组合；
 * 现有 WritingPane/SujianEditorHost 的正常 beginEdit/attach/loading 职责无预热恢复。
 *
 * - 完整恢复 IDs → 产生正确 target；
 * - 缺任一 ID → 返回 null；
 * - 普通 click handoff 语义不变（shouldNavigateAfterReady 仍按 currentTarget == requestedTarget 判定）。
 */
class RestorePreparedEditorTargetTest {
    @Test
    fun fullIds_producesTargetWithAllFields() {
        val target =
            restorePreparedEditorTarget(
                projectId = "p1",
                projectTitle = "我的作品",
                volumeId = "v1",
                chapterId = "c1",
                chapterTitle = "第一章",
            )
        checkNotNull(target)
        assertEquals("p1", target.projectId)
        assertEquals("我的作品", target.projectTitle)
        assertEquals("v1", target.volumeId)
        assertEquals("c1", target.chapterId)
        assertEquals("第一章", target.chapterTitle)
        assertEquals("chapter-body:p1:v1:c1", target.targetId)
    }

    @Test
    fun nullProject_returnsNull() {
        assertNull(
            restorePreparedEditorTarget(
                projectId = null,
                projectTitle = "",
                volumeId = "v1",
                chapterId = "c1",
                chapterTitle = "第一章",
            ),
        )
    }

    @Test
    fun nullVolume_returnsNull() {
        assertNull(
            restorePreparedEditorTarget(
                projectId = "p1",
                projectTitle = "我的作品",
                volumeId = null,
                chapterId = "c1",
                chapterTitle = "第一章",
            ),
        )
    }

    @Test
    fun nullChapter_returnsNull() {
        assertNull(
            restorePreparedEditorTarget(
                projectId = "p1",
                projectTitle = "我的作品",
                volumeId = "v1",
                chapterId = null,
                chapterTitle = "",
            ),
        )
    }

    @Test
    fun allNull_returnsNull() {
        assertNull(
            restorePreparedEditorTarget(
                projectId = null,
                projectTitle = "",
                volumeId = null,
                chapterId = null,
                chapterTitle = "",
            ),
        )
    }

    @Test
    fun emptyTitle_preservesEmptyTitle() {
        val target =
            restorePreparedEditorTarget(
                projectId = "p1",
                projectTitle = "",
                volumeId = "v1",
                chapterId = "c1",
                chapterTitle = "",
            )
        checkNotNull(target)
        assertEquals("", target.projectTitle)
        assertEquals("", target.chapterTitle)
    }

    /**
     * 普通 click handoff 语义不变 — restore target 与 click target 结构相同，
     * shouldNavigateAfterReady 仍按 currentTarget == requestedTarget 判定。
     * restore 路径直接设置 preparedEditorTargetState（不触发 await+navigate），
     * 若 restore target 后被 click 替换，旧 restore 不抢导航。
     */
    @Test
    fun restoreTarget_compatibleWithClickHandoffSemantics() {
        val restored =
            restorePreparedEditorTarget(
                projectId = "p1",
                projectTitle = "作品",
                volumeId = "v1",
                chapterId = "c1",
                chapterTitle = "章",
            )
        checkNotNull(restored)
        val clicked = restored.copy(chapterId = "c2")
        assertFalse(
            shouldNavigateAfterReady(
                currentTarget = clicked,
                requestedTarget = restored,
                isReady = true,
            ),
        )
        assertTrue(
            shouldNavigateAfterReady(
                currentTarget = clicked,
                requestedTarget = clicked,
                isReady = true,
            ),
        )
    }

    /**
     * restore target 与同字段 click target 等价 — 同 project/volume/chapter 产生同 targetId，
     * host 可无缝从 restore 切到 click（或反之）而不重建 View。
     */
    @Test
    fun restoreTarget_equivalentToClickTargetForSameChapter() {
        val restored =
            restorePreparedEditorTarget(
                projectId = "p1",
                projectTitle = "作品",
                volumeId = "v1",
                chapterId = "c1",
                chapterTitle = "章",
            )
        checkNotNull(restored)
        val clicked =
            PreparedEditorTarget(
                projectId = "p1",
                projectTitle = "作品",
                volumeId = "v1",
                chapterId = "c1",
                chapterTitle = "章",
            )
        assertEquals(restored, clicked)
        assertEquals(restored.targetId, clicked.targetId)
    }
}
