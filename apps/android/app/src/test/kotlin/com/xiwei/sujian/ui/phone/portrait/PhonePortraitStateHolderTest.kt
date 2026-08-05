package com.xiwei.sujian.ui.phone.portrait

import com.xiwei.sujian.ui.compose.navigation.SettingsSection
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class PhonePortraitStateHolderTest {
    private val savedSections = mutableListOf<Set<SettingsSection>>()
    private val holder = PhonePortraitStateHolder(
        onSaveExpandedSections = { savedSections.add(it) },
        initialExpandedSections = emptySet(),
    )

    @Test
    fun initialRoot_isWorks() {
        assertEquals(PhoneRoot.Works, holder.selectedRoot)
    }

    @Test
    fun selectRoot_works_updatesRoot() {
        holder.onEvent(PhonePortraitEvent.SelectRoot(PhoneRoot.Stats))
        assertEquals(PhoneRoot.Stats, holder.selectedRoot)
    }

    @Test
    fun selectRoot_starMap_isIgnored() {
        holder.onEvent(PhonePortraitEvent.SelectRoot(PhoneRoot.StarMap))
        assertEquals(PhoneRoot.Works, holder.selectedRoot)
    }

    @Test
    fun toggleSettingsSection_addsAndRemoves() {
        holder.onEvent(PhonePortraitEvent.ToggleSettingsSection(SettingsSection.Appearance))
        assertTrue(holder.expandedSettingsSections.contains(SettingsSection.Appearance))

        holder.onEvent(PhonePortraitEvent.ToggleSettingsSection(SettingsSection.Appearance))
        assertTrue(holder.expandedSettingsSections.isEmpty())
    }

    @Test
    fun toggleSettingsSection_persists() {
        holder.onEvent(PhonePortraitEvent.ToggleSettingsSection(SettingsSection.Sync))
        assertEquals(1, savedSections.size)
        assertTrue(savedSections.first().contains(SettingsSection.Sync))
    }
}


class PhonePortraitStateHolderRestoreTest {

    @Test
    fun initialRoot_defaultsToWorks() {
        val holder = PhonePortraitStateHolder(
            onSaveExpandedSections = { },
            initialExpandedSections = emptySet(),
        )
        assertEquals(PhoneRoot.Works, holder.selectedRoot)
    }

    @Test
    fun initialRoot_canBeRestoredToStats() {
        val holder = PhonePortraitStateHolder(
            initialRoot = PhoneRoot.Stats,
            onSaveExpandedSections = { },
            initialExpandedSections = emptySet(),
        )
        assertEquals(PhoneRoot.Stats, holder.selectedRoot)
    }

    @Test
    fun initialRoot_starMapFallsBackToWorks() {
        val holder = PhonePortraitStateHolder(
            initialRoot = PhoneRoot.StarMap,
            onSaveExpandedSections = { },
            initialExpandedSections = emptySet(),
        )
        assertEquals(PhoneRoot.Works, holder.selectedRoot)
    }
}