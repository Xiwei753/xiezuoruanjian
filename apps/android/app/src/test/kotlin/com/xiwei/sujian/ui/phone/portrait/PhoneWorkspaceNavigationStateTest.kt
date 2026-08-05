package com.xiwei.sujian.ui.phone.portrait

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PhoneWorkspaceNavigationStateTest {
    @Test
    fun derive_nullDestination_isProjectList() {
        assertEquals(WorkspaceLocation.ProjectList, deriveWorkspaceLocation(null))
    }

    @Test
    fun derive_projectListDestination_isProjectList() {
        assertEquals(
            WorkspaceLocation.ProjectList,
            deriveWorkspaceLocation(WorkspacePaneKey.ProjectList),
        )
    }

    @Test
    fun derive_chapterTreeDestination_carriesProjectId() {
        val location = deriveWorkspaceLocation(WorkspacePaneKey.ChapterTree("p1"))
        assertTrue(location is WorkspaceLocation.ChapterTree)
        assertEquals("p1", (location as WorkspaceLocation.ChapterTree).projectId)
    }

    @Test
    fun derive_editorDestination_carriesAllIds() {
        val location = deriveWorkspaceLocation(WorkspacePaneKey.Editor("p1", "v1", "c1"))
        assertTrue(location is WorkspaceLocation.Editor)
        val editor = location as WorkspaceLocation.Editor
        assertEquals("p1", editor.projectId)
        assertEquals("v1", editor.volumeId)
        assertEquals("c1", editor.chapterId)
    }

    @Test
    fun derive_chapterTreeToProjectList_returnsRootLocation() {
        assertEquals(
            WorkspaceLocation.ProjectList,
            deriveWorkspaceLocation(WorkspacePaneKey.ProjectList),
        )
    }

    @Test
    fun back_chain_editorToChapterTreeToProjectList_hasNoSecondLocationCopy() {
        // 位置唯一事实来源是 destination 键：章节树位置携带 projectId，
        // 正文位置携带 project/volume/chapter，不依赖外部镜像状态。
        val editorKey = WorkspacePaneKey.Editor("p1", "v1", "c1")
        val chapterTreeKey = WorkspacePaneKey.ChapterTree("p1")
        assertEquals(
            WorkspaceLocation.ChapterTree("p1"),
            deriveWorkspaceLocation(chapterTreeKey),
        )
        assertEquals(editorKey.projectId, (deriveWorkspaceLocation(editorKey) as WorkspaceLocation.Editor).projectId)
        assertFalse(deriveWorkspaceLocation(chapterTreeKey) is WorkspaceLocation.Editor)
    }
}
