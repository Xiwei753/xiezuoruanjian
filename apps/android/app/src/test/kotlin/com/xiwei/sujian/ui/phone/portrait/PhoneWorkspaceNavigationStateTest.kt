package com.xiwei.sujian.ui.phone.portrait

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PhoneWorkspaceNavigationStateTest {
    @Test
    fun initialLocation_isProjectList() {
        val state = PhoneWorkspaceNavigationState()
        assertTrue(state.currentLocation is WorkspaceLocation.ProjectList)
    }

    @Test
    fun navigateToChapterTree_updatesLocation() {
        val state = PhoneWorkspaceNavigationState()
        state.navigateToChapterTree("p1")
        assertTrue(state.currentLocation is WorkspaceLocation.ChapterTree)
        assertEquals("p1", (state.currentLocation as WorkspaceLocation.ChapterTree).projectId)
    }

    @Test
    fun navigateToEditor_updatesLocation() {
        val state = PhoneWorkspaceNavigationState()
        state.navigateToEditor("p1", "v1", "c1")
        val loc = state.currentLocation as WorkspaceLocation.Editor
        assertEquals("p1", loc.projectId)
        assertEquals("v1", loc.volumeId)
        assertEquals("c1", loc.chapterId)
    }

    @Test
    fun back_fromEditor_goesToChapterTree() {
        val state = PhoneWorkspaceNavigationState()
        state.navigateToEditor("p1", "v1", "c1")
        val handled = state.back()
        assertTrue(handled)
        assertTrue(state.currentLocation is WorkspaceLocation.ChapterTree)
        assertEquals("p1", (state.currentLocation as WorkspaceLocation.ChapterTree).projectId)
    }

    @Test
    fun back_fromChapterTree_goesToProjectList() {
        val state = PhoneWorkspaceNavigationState()
        state.navigateToChapterTree("p1")
        val handled = state.back()
        assertTrue(handled)
        assertTrue(state.currentLocation is WorkspaceLocation.ProjectList)
    }

    @Test
    fun back_fromProjectList_returnsFalse() {
        val state = PhoneWorkspaceNavigationState()
        val handled = state.back()
        assertFalse(handled)
    }

    @Test
    fun back_fullStack_returnsToProjectList() {
        val state = PhoneWorkspaceNavigationState()
        state.navigateToChapterTree("p1")
        state.navigateToEditor("p1", "v1", "c1")
        state.back()
        state.back()
        assertTrue(state.currentLocation is WorkspaceLocation.ProjectList)
    }

    @Test
    fun navigateToProjectList_resetsToProjectList() {
        val state = PhoneWorkspaceNavigationState()
        state.navigateToEditor("p1", "v1", "c1")
        state.navigateToProjectList()
        assertTrue(state.currentLocation is WorkspaceLocation.ProjectList)
    }
}
