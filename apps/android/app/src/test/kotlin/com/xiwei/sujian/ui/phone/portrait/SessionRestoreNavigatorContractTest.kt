package com.xiwei.sujian.ui.phone.portrait

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class SessionRestoreNavigatorContractTest {

    @Test
    fun loadingState_noRestoredIds_navigatorNotCreated() {
        val state: SessionRestoreState = SessionRestoreState.Loading
        assertFalse(state is SessionRestoreState.Ready)
    }

    @Test
    fun readyState_withNullIds_navigatorCreatedWithProjectListOnly() {
        val state: SessionRestoreState = SessionRestoreState.Ready(projectId = null, volumeId = null, chapterId = null)
        val ready = state as SessionRestoreState.Ready
        val chain = buildChain(ready.projectId, ready.volumeId, ready.chapterId)
        assertEquals(1, chain.size)
        assertEquals(WorkspacePaneKey.ProjectList, chain[0])
    }

    @Test
    fun readyState_withProject_navigatorCreatedWithChapterTree() {
        val state: SessionRestoreState = SessionRestoreState.Ready(projectId = "p1", volumeId = null, chapterId = null)
        val ready = state as SessionRestoreState.Ready
        val chain = buildChain(ready.projectId, ready.volumeId, ready.chapterId)
        assertEquals(2, chain.size)
        assertTrue(chain[1] is WorkspacePaneKey.ChapterTree)
    }

    @Test
    fun readyState_withEditor_navigatorCreatedWithFullHistory() {
        val state: SessionRestoreState = SessionRestoreState.Ready(projectId = "p1", volumeId = "v1", chapterId = "c1")
        val ready = state as SessionRestoreState.Ready
        val chain = buildChain(ready.projectId, ready.volumeId, ready.chapterId)
        assertEquals(3, chain.size)
        assertTrue(chain[2] is WorkspacePaneKey.Editor)
    }

    @Test
    fun loadingToReady_transitionRebuildsHistory() {
        val loading: SessionRestoreState = SessionRestoreState.Loading
        assertFalse(loading is SessionRestoreState.Ready)
        val ready: SessionRestoreState = SessionRestoreState.Ready(projectId = "p1", volumeId = "v1", chapterId = "c1")
        assertTrue(ready is SessionRestoreState.Ready)
        val chain = buildChain((ready as SessionRestoreState.Ready).projectId, ready.volumeId, ready.chapterId)
        assertEquals(3, chain.size)
    }

    private fun buildChain(
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
