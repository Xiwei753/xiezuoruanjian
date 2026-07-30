package com.xiwei.sujian.data

import androidx.test.ext.junit.runners.AndroidJUnit4
import com.xiwei.sujian.model.LocalSettings
import com.xiwei.sujian.support.AndroidTestEnvironment
import com.xiwei.sujian.support.SujianSmallTest
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
@SujianSmallTest
class SettingsRepositoryInstrumentedTest {

    @get:Rule
    val rule = AndroidTestEnvironment.TestDependenciesRule(seedProject = false)

    private fun getSettingsRepo(): SettingsRepository =
        AndroidTestEnvironment.requireCurrentSession().deps.settingsRepository

    @Test
    fun getLocalSettings_returnsDefaults() {
        val repo = getSettingsRepo()
        val settings = repo.getLocalSettings()
        assertEquals("Default theme mode should be system", "system", settings.themeMode)
        assertEquals("Default font size should be 16f", 16f, settings.editorFontSize, 0.01f)
        assertTrue("Typing animation should be enabled by default", settings.editorTypingAnimationEnabled)
    }

    @Test
    fun saveAndRead_localSettingsRoundtrips() {
        val repo = getSettingsRepo()
        val original = repo.getLocalSettings()
        val modified = original.copy(
            editorFontSize = 22f,
            editorTypingAnimationEnabled = !original.editorTypingAnimationEnabled,
            autoSaveDelayMs = 3000L
        )
        val saveResult = repo.saveLocalSettings(modified)
        assertTrue("Save should succeed", saveResult is SettingsSaveResult.Success)
        val retrieved = repo.getLocalSettings()
        assertEquals("Font size should persist", 22f, retrieved.editorFontSize, 0.01f)
        assertEquals(
            "Typing animation toggle should persist",
            modified.editorTypingAnimationEnabled,
            retrieved.editorTypingAnimationEnabled
        )
        assertEquals("Auto-save delay should persist", 3000L, retrieved.autoSaveDelayMs)
    }

    @Test
    fun setFontSize_getEffectiveFontSize_reflectsChange() {
        val repo = getSettingsRepo()
        val result = repo.setFontSize(20f)
        assertTrue("setFontSize should succeed", result is SettingsSaveResult.Success)
        val effective = repo.getEffectiveFontSize()
        assertEquals("Effective font size should be 20f", 20f, effective, 0.01f)
    }

    @Test
    fun getSyncableSettings_returnsDefaults() {
        val repo = getSettingsRepo()
        val syncable = repo.getSyncableSettings()
        assertEquals("Default syncable fontSize should be 0.0", 0.0, syncable.fontSize, 0.01)
    }

    @Test
    fun saveSyncableSettings_persistsFontSize() {
        val repo = getSettingsRepo()
        val original = repo.getSyncableSettings()
        val modified = original.copy(fontSize = 24.0)
        val saveResult = repo.saveSyncableSettings(modified)
        assertTrue("Save syncable settings should succeed", saveResult is SettingsSaveResult.Success)
        val retrieved = repo.getSyncableSettings()
        assertEquals("Syncable fontSize should persist", 24.0, retrieved.fontSize, 0.01)
    }

    @Test
    fun fontSizeChange_affectsGetEffectiveFontSize() {
        val repo = getSettingsRepo()
        val defaultSize = repo.getEffectiveFontSize()
        repo.setFontSize(defaultSize + 4f)
        val newSize = repo.getEffectiveFontSize()
        assertEquals("Effective fontSize should be 4 more than default", defaultSize + 4f, newSize, 0.01f)
    }

    @Test
    fun toggleTypingAnimation_savesAndReadsCorrectly() {
        val repo = getSettingsRepo()
        val before = repo.getLocalSettings()
        val toggled = before.copy(editorTypingAnimationEnabled = !before.editorTypingAnimationEnabled)
        repo.saveLocalSettings(toggled)
        val after = repo.getLocalSettings()
        assertEquals(
            "Typing animation toggle should change",
            !before.editorTypingAnimationEnabled,
            after.editorTypingAnimationEnabled
        )
    }
}
