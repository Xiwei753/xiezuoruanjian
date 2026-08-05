package com.xiwei.sujian.ui.phone.portrait

import com.xiwei.sujian.model.SyncIndicatorState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PhoneChromePolicyContractTest {

    @Test
    fun syncState_doesNotAffectChromeSpec() {
        val states = SyncIndicatorState.entries
        val specs = states.map { state ->
            PhoneChromePolicy.resolve(
                route = PhoneRootRoute.Root,
                selectedRoot = PhoneRoot.Works,
                workspaceLocation = WorkspaceLocation.ProjectList,
                syncState = state,
            )
        }
        val first = specs.first()
        assertTrue(specs.all { it.showSync == first.showSync })
        assertTrue(specs.all { it.showSearch == first.showSearch })
        assertTrue(specs.all { it.showSettings == first.showSettings })
        assertTrue(specs.all { it.showBottomBar == first.showBottomBar })
        assertTrue(specs.all { it.appBarTransparent == first.appBarTransparent })
    }

    @Test
    fun workspaceLocation_onlyAffectsWorksRoot() {
        val editorSpec = PhoneChromePolicy.resolve(
            route = PhoneRootRoute.Root,
            selectedRoot = PhoneRoot.StarMap,
            workspaceLocation = WorkspaceLocation.Editor("p1", "v1", "c1"),
            syncState = SyncIndicatorState.Synced,
        )
        assertFalse(editorSpec.appBarTransparent)
        assertTrue(editorSpec.showBottomBar)

        val statsSpec = PhoneChromePolicy.resolve(
            route = PhoneRootRoute.Root,
            selectedRoot = PhoneRoot.Stats,
            workspaceLocation = WorkspaceLocation.Editor("p1", "v1", "c1"),
            syncState = SyncIndicatorState.Synced,
        )
        assertFalse(statsSpec.appBarTransparent)
        assertTrue(statsSpec.showBottomBar)
    }

    @Test
    fun worksEditor_onlyTopBarTransparent_notFullScaffold() {
        val spec = PhoneChromePolicy.resolve(
            route = PhoneRootRoute.Root,
            selectedRoot = PhoneRoot.Works,
            workspaceLocation = WorkspaceLocation.Editor("p1", "v1", "c1"),
            syncState = SyncIndicatorState.Syncing,
        )
        assertTrue(spec.appBarTransparent)
        assertFalse(spec.showBottomBar)
        assertTrue(spec.showSync)
        assertTrue(spec.showSearch)
        assertTrue(spec.showSettings)
    }

    @Test
    fun settingsRoute_noActionsNoBottomBar() {
        val spec = PhoneChromePolicy.resolve(
            route = PhoneSettingsRoute.Settings,
            selectedRoot = PhoneRoot.Works,
            workspaceLocation = WorkspaceLocation.ProjectList,
            syncState = SyncIndicatorState.Synced,
        )
        assertFalse(spec.showSync)
        assertFalse(spec.showSearch)
        assertFalse(spec.showSettings)
        assertFalse(spec.showBottomBar)
        assertTrue(spec.showBack)
    }

    @Test
    fun starMap_noBackButton() {
        val spec = PhoneChromePolicy.resolve(
            route = PhoneRootRoute.Root,
            selectedRoot = PhoneRoot.StarMap,
            workspaceLocation = WorkspaceLocation.ProjectList,
            syncState = SyncIndicatorState.Synced,
        )
        assertFalse(spec.showBack)
        assertEquals("素笺写作", spec.title)
    }

    @Test
    fun stats_noBackButton() {
        val spec = PhoneChromePolicy.resolve(
            route = PhoneRootRoute.Root,
            selectedRoot = PhoneRoot.Stats,
            workspaceLocation = WorkspaceLocation.ProjectList,
            syncState = SyncIndicatorState.Synced,
        )
        assertFalse(spec.showBack)
        assertEquals("素笺写作", spec.title)
    }
}
