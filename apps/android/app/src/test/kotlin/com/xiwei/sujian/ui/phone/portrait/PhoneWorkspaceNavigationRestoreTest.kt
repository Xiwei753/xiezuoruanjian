package com.xiwei.sujian.ui.phone.portrait

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class PhoneWorkspaceNavigationRestoreTest {

    @Test
    fun initialHistory_noProject_projectListOnly() {
        val projectId: String? = null
        val volumeId: String? = null
        val chapterId: String? = null
        val chain = buildExpectedChain(projectId, volumeId, chapterId)
        assertEquals(1, chain.size)
        assertEquals(WorkspacePaneKey.ProjectList, chain[0])
    }

    @Test
    fun initialHistory_hasProjectNoChapter_projectListAndChapterTree() {
        val chain = buildExpectedChain("p1", null, null)
        assertEquals(2, chain.size)
        assertEquals(WorkspacePaneKey.ProjectList, chain[0])
        assertTrue(chain[1] is WorkspacePaneKey.ChapterTree)
        assertEquals("p1", (chain[1] as WorkspacePaneKey.ChapterTree).projectId)
    }

    @Test
    fun initialHistory_hasProjectAndChapter_fullChain() {
        val chain = buildExpectedChain("p1", "v1", "c1")
        assertEquals(3, chain.size)
        assertEquals(WorkspacePaneKey.ProjectList, chain[0])
        assertTrue(chain[1] is WorkspacePaneKey.ChapterTree)
        assertTrue(chain[2] is WorkspacePaneKey.Editor)
        val editor = chain[2] as WorkspacePaneKey.Editor
        assertEquals("p1", editor.projectId)
        assertEquals("v1", editor.volumeId)
        assertEquals("c1", editor.chapterId)
    }

    @Test
    fun initialHistory_volumeWithoutChapter_treatedAsProjectOnly() {
        val chain = buildExpectedChain("p1", "v1", null)
        assertEquals(2, chain.size)
        assertEquals(WorkspacePaneKey.ProjectList, chain[0])
        assertTrue(chain[1] is WorkspacePaneKey.ChapterTree)
    }

    @Test
    fun deriveLocation_consistentWithPaneKey_chainRoundTrip() {
        val keys = listOf(
            WorkspacePaneKey.ProjectList,
            WorkspacePaneKey.ChapterTree("p1"),
            WorkspacePaneKey.Editor("p1", "v1", "c1"),
        )
        val locations = keys.map { deriveWorkspaceLocation(it) }
        assertTrue(locations[0] is WorkspaceLocation.ProjectList)
        assertTrue(locations[1] is WorkspaceLocation.ChapterTree)
        assertTrue(locations[2] is WorkspaceLocation.Editor)
    }

    private fun buildExpectedChain(
        projectId: String?,
        volumeId: String?,
        chapterId: String?,
    ): List<WorkspacePaneKey> {
        val chain = mutableListOf<WorkspacePaneKey>(WorkspacePaneKey.ProjectList)
        if (projectId != null) {
            chain += WorkspacePaneKey.ChapterTree(projectId)
            if (volumeId != null && chapterId != null) {
                chain += WorkspacePaneKey.Editor(projectId, volumeId, chapterId)
            }
        }
        return chain
    }
}


class SessionRestoreStateTest {

    @Test
    fun loadingState_isDistinctFromReady() {
        val loading = SessionRestoreState.Loading
        val ready = SessionRestoreState.Ready(projectId = null, volumeId = null, chapterId = null)
        assert(loading != ready)
    }

    @Test
    fun readyState_holdsProjectId() {
        val ready = SessionRestoreState.Ready(projectId = "p1", volumeId = "v1", chapterId = "c1")
        assertEquals("p1", ready.projectId)
        assertEquals("v1", ready.volumeId)
        assertEquals("c1", ready.chapterId)
    }

    @Test
    fun readyState_canHaveNullIds() {
        val ready = SessionRestoreState.Ready(projectId = null, volumeId = null, chapterId = null)
        assertEquals(null, ready.projectId)
    }
}