package com.xiwei.sujian.ui.phone.portrait

import androidx.compose.runtime.saveable.SaverScope
import com.xiwei.sujian.ui.compose.navigation.SettingsSection
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class PhonePortraitStateHolderTest {
    private val savedSections = mutableListOf<Set<SettingsSection>>()
    private val savedRoots = mutableListOf<String>()
    private val holder = PhonePortraitStateHolder(
        onSaveSelectedRoot = { savedRoots.add(it) },
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
    fun selectRoot_starMap_updatesRoot() {
        holder.onEvent(PhonePortraitEvent.SelectRoot(PhoneRoot.StarMap))
        assertEquals(PhoneRoot.StarMap, holder.selectedRoot)
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

    @Test
    fun selectRoot_notifiesSaverCallbackSynchronously() {
        holder.onEvent(PhonePortraitEvent.SelectRoot(PhoneRoot.Stats))
        assertEquals(listOf("Stats"), savedRoots)
    }

    @Test
    fun selectRoot_starMap_writesStarMap() {
        holder.onEvent(PhonePortraitEvent.SelectRoot(PhoneRoot.StarMap))
        assertEquals(listOf("StarMap"), savedRoots)
        assertEquals(PhoneRoot.StarMap, holder.selectedRoot)
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
    fun initialRoot_canBeRestoredToStarMap() {
        val holder = PhonePortraitStateHolder(
            initialRoot = PhoneRoot.StarMap,
            onSaveExpandedSections = { },
            initialExpandedSections = emptySet(),
        )
        assertEquals(PhoneRoot.StarMap, holder.selectedRoot)
    }
}


/**
 * Saver 契约：配置变化/进程恢复时 save pass 读取状态当前值，restore 原样恢复；
 * 统计页停留、折叠分类、星图恢复都必须符合真实持久化语义。
 *
 * 测试类实现 [SaverScope]，按框架内部相同方式（with(saver) { save(value) }）
 * 驱动 Saver 的扩展成员 save。
 */
class PhonePortraitStateHolderSaverTest : SaverScope {

    override fun canBeSaved(value: Any): Boolean = true

    private fun roundTrip(holder: PhonePortraitStateHolder): PhonePortraitStateHolder {
        val saver = PhonePortraitStateHolder.saver()
        val saved = with(saver) { save(holder) }
        assertTrue("saver.save must produce a value", saved != null)
        val restored = saver.restore(saved!!)
        assertTrue("saver.restore must produce a value", restored != null)
        return restored!!
    }

    @Test
    fun saver_saveStats_restoresStats() {
        val holder = PhonePortraitStateHolder(
            initialRoot = PhoneRoot.Works,
            onSaveSelectedRoot = { },
            onSaveExpandedSections = { },
        )
        holder.onEvent(PhonePortraitEvent.SelectRoot(PhoneRoot.Stats))
        holder.onEvent(PhonePortraitEvent.ToggleSettingsSection(SettingsSection.Sync))

        val restored = roundTrip(holder)

        assertEquals(PhoneRoot.Stats, restored.selectedRoot)
        assertEquals(setOf(SettingsSection.Sync), restored.expandedSettingsSections)
    }

    @Test
    fun saver_saveWorks_expandedSectionsRoundTrip() {
        val holder = PhonePortraitStateHolder(
            initialRoot = PhoneRoot.Works,
            onSaveSelectedRoot = { },
            onSaveExpandedSections = { },
        )
        holder.onEvent(PhonePortraitEvent.ToggleSettingsSection(SettingsSection.Appearance))
        holder.onEvent(PhonePortraitEvent.ToggleSettingsSection(SettingsSection.About))

        val restored = roundTrip(holder)

        assertEquals(PhoneRoot.Works, restored.selectedRoot)
        assertEquals(
            setOf(SettingsSection.Appearance, SettingsSection.About),
            restored.expandedSettingsSections,
        )
    }

    @Test
    fun saver_restoreUnknownRoot_fallsBackToWorks() {
        val restored = PhonePortraitStateHolder.saver().restore(
            listOf("NotARoot", SettingsSection.Sync.name),
        )
        assertTrue(restored != null)
        assertEquals(PhoneRoot.Works, restored!!.selectedRoot)
        assertEquals(setOf(SettingsSection.Sync), restored.expandedSettingsSections)
    }

    @Test
    fun saver_restoreStarMap_restoresStarMap() {
        val restored = PhonePortraitStateHolder.saver().restore(listOf(PhoneRoot.StarMap.name))
        assertTrue(restored != null)
        assertEquals(PhoneRoot.StarMap, restored!!.selectedRoot)
    }

    @Test
    fun saver_restorePreservesCallbacks() {
        val savedRoots = mutableListOf<String>()
        val restored = PhonePortraitStateHolder.saver(
            onSaveSelectedRoot = { savedRoots.add(it) },
        ).restore(listOf(PhoneRoot.Stats.name))
        assertTrue(restored != null)
        restored!!.onEvent(PhonePortraitEvent.SelectRoot(PhoneRoot.Works))
        assertEquals(listOf("Works"), savedRoots)
    }
}
