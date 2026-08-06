package com.xiwei.sujian.ui.phone.portrait

import com.xiwei.sujian.model.SyncIndicatorState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PhoneChromePolicyTest {
    @Test
    fun settingsRoute_showsBackHidesActionsAndBottomBar() {
        val spec = PhoneChromePolicy.resolve(
            route = PhoneSettingsRoute.Settings,
            selectedRoot = PhoneRoot.Works,
            workspaceLocation = WorkspaceLocation.ProjectList,
            syncState = SyncIndicatorState.Synced,
        )
        assertEquals(null, spec.title)
        assertTrue(spec.showBack)
        assertFalse(spec.appBarTransparent)
        assertFalse(spec.showSync)
        assertFalse(spec.showSearch)
        assertFalse(spec.showSettings)
        assertFalse(spec.showBottomBar)
    }

    @Test
    fun worksProjectList_showsTitleNoBackShowsActionsAndBottomBar() {
        val spec = PhoneChromePolicy.resolve(
            route = PhoneRootRoute.Root,
            selectedRoot = PhoneRoot.Works,
            workspaceLocation = WorkspaceLocation.ProjectList,
            syncState = SyncIndicatorState.Synced,
        )
        assertEquals("素笺写作", spec.title)
        assertFalse(spec.showBack)
        assertFalse(spec.appBarTransparent)
        assertTrue(spec.showSync)
        assertTrue(spec.showSearch)
        assertTrue(spec.showSettings)
        assertTrue(spec.showBottomBar)
    }

    @Test
    fun worksEditor_transparentAppBarHidesBottomBar() {
        val spec = PhoneChromePolicy.resolve(
            route = PhoneRootRoute.Root,
            selectedRoot = PhoneRoot.Works,
            workspaceLocation = WorkspaceLocation.Editor("p1", "v1", "c1"),
            syncState = SyncIndicatorState.Syncing,
        )
        assertEquals(null, spec.title)
        assertTrue(spec.showBack)
        assertTrue(spec.appBarTransparent)
        assertFalse(spec.showBottomBar)
    }

    @Test
    fun worksChapterTree_showsBackNoTitle() {
        val spec = PhoneChromePolicy.resolve(
            route = PhoneRootRoute.Root,
            selectedRoot = PhoneRoot.Works,
            workspaceLocation = WorkspaceLocation.ChapterTree("p1"),
            syncState = SyncIndicatorState.Synced,
        )
        assertEquals(null, spec.title)
        assertTrue(spec.showBack)
        assertFalse(spec.appBarTransparent)
        assertTrue(spec.showBottomBar)
    }

    @Test
    fun starMap_showsStarMapTitleNoBack() {
        val spec = PhoneChromePolicy.resolve(
            route = PhoneRootRoute.Root,
            selectedRoot = PhoneRoot.StarMap,
            workspaceLocation = WorkspaceLocation.ProjectList,
            syncState = SyncIndicatorState.Synced,
        )
        assertEquals("星图", spec.title)
        assertFalse(spec.showBack)
        assertFalse(spec.appBarTransparent)
        assertTrue(spec.showSync)
        assertTrue(spec.showSearch)
        assertTrue(spec.showSettings)
        assertTrue(spec.showBottomBar)
    }

    @Test
    fun stats_doesNotInheritWorkspaceChrome() {
        val spec = PhoneChromePolicy.resolve(
            route = PhoneRootRoute.Root,
            selectedRoot = PhoneRoot.Stats,
            workspaceLocation = WorkspaceLocation.Editor("p1", "v1", "c1"),
            syncState = SyncIndicatorState.Failed,
        )
        assertEquals("素笺写作", spec.title)
        assertFalse(spec.showBack)
        assertFalse(spec.appBarTransparent)
        assertTrue(spec.showBottomBar)
    }
}
