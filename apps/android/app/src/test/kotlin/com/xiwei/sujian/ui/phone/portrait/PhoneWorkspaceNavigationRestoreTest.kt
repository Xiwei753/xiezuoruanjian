package com.xiwei.sujian.ui.phone.portrait

import androidx.compose.material3.adaptive.ExperimentalMaterial3AdaptiveApi
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

@OptIn(ExperimentalMaterial3AdaptiveApi::class)
class PhoneWorkspaceNavigationRestoreTest {

    @Test
    fun initialHistory_noProject_projectListOnly() {
        val chain = buildInitialHistory(SessionRestoreState.Destination.ProjectList)
        assertEquals(1, chain.size)
        assertEquals(WorkspacePaneKey.ProjectList, chain[0].contentKey)
    }

    @Test
    fun initialHistory_hasProjectNoChapter_projectListAndChapterTree() {
        val chain = buildInitialHistory(SessionRestoreState.Destination.ChapterTree("p1"))
        assertEquals(2, chain.size)
        assertEquals(WorkspacePaneKey.ProjectList, chain[0].contentKey)
        assertTrue(chain[1].contentKey is WorkspacePaneKey.ChapterTree)
        assertEquals("p1", (chain[1].contentKey as WorkspacePaneKey.ChapterTree).projectId)
    }

    @Test
    fun initialHistory_hasProjectAndChapter_fullChain() {
        val chain = buildInitialHistory(SessionRestoreState.Destination.Editor("p1", "v1", "c1"))
        assertEquals(3, chain.size)
        assertEquals(WorkspacePaneKey.ProjectList, chain[0].contentKey)
        assertTrue(chain[1].contentKey is WorkspacePaneKey.ChapterTree)
        assertTrue(chain[2].contentKey is WorkspacePaneKey.Editor)
        val editor = chain[2].contentKey as WorkspacePaneKey.Editor
        assertEquals("p1", editor.projectId)
        assertEquals("v1", editor.volumeId)
        assertEquals("c1", editor.chapterId)
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
}


class SessionRestoreStateTest {

    @Test
    fun loadingState_isDistinctFromReady() {
        val loading: SessionRestoreState = SessionRestoreState.Loading
        val ready: SessionRestoreState = SessionRestoreState.Ready(SessionRestoreState.Destination.ProjectList)
        assert(loading != ready)
    }

    @Test
    fun readyEditor_holdsAllIds() {
        val ready = SessionRestoreState.Ready(SessionRestoreState.Destination.Editor("p1", "v1", "c1"))
        val destination = (ready as SessionRestoreState.Ready).destination as SessionRestoreState.Destination.Editor
        assertEquals("p1", destination.projectId)
        assertEquals("v1", destination.volumeId)
        assertEquals("c1", destination.chapterId)
    }

    @Test
    fun readyProjectList_holdsNoIds() {
        val ready = SessionRestoreState.Ready(SessionRestoreState.Destination.ProjectList)
        assertTrue((ready as SessionRestoreState.Ready).destination is SessionRestoreState.Destination.ProjectList)
    }
}
