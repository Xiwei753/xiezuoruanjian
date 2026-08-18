package com.xiwei.sujian.feature.editor.ui

import com.xiwei.sujian.feature.editor.presentation.EditorSettingsState
import com.xiwei.sujian.feature.editor.presentation.EditorUiState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #630 评论 5327560790: settingsReady 门槛 + 非默认持久化设置首帧一次排版。
 *
 * 测试3: 非默认持久化设置首帧一次排版 — settingsReady=true 后 beginEdit 用持久化设置
 *        而非默认值，首帧不二次重排。
 * 测试4: settingsReady 门槛 — settingsReady=false 时 BeginEdit 不触发；
 *        settingsReady=true 且 loading=false 时才触发。
 */
class EditorSettingsReadyGateTest {
    // ── 测试4: settingsReady 门槛 ──

    @Test
    fun shouldBeginEdit_loadingTrue_settingsReadyTrue_returnsFalse() {
        assertFalse(
            "loading=true 时不应触发 BeginEdit",
            shouldBeginEditForEditorAttach(loading = true, settingsReady = true),
        )
    }

    @Test
    fun shouldBeginEdit_loadingFalse_settingsReadyFalse_returnsFalse() {
        assertFalse(
            "settingsReady=false 时不应触发 BeginEdit",
            shouldBeginEditForEditorAttach(loading = false, settingsReady = false),
        )
    }

    @Test
    fun shouldBeginEdit_loadingTrue_settingsReadyFalse_returnsFalse() {
        assertFalse(
            "loading=true 且 settingsReady=false 时不应触发 BeginEdit",
            shouldBeginEditForEditorAttach(loading = true, settingsReady = false),
        )
    }

    @Test
    fun shouldBeginEdit_loadingFalse_settingsReadyTrue_returnsTrue() {
        assertTrue(
            "loading=false 且 settingsReady=true 时才触发 BeginEdit",
            shouldBeginEditForEditorAttach(loading = false, settingsReady = true),
        )
    }

    @Test
    fun editorUiState_settingsReady_defaultsToFalse() {
        // 不用"默认值恰好存在"冒充已加载完成 — settingsReady 默认为 false。
        val state = EditorUiState()
        assertFalse("EditorUiState.settingsReady 默认应为 false", state.settingsReady)
    }

    // ── 测试3: 非默认持久化设置首帧一次排版 ──

    @Test
    fun editorTypographyFromSettings_preservesNonDefaultFontSize() {
        // 持久化设置 fontSize=20f（非默认 16f）→ EditorTypography.fontSizeSp=20f
        val settings = EditorSettingsState(fontSize = 20f)
        val typography = editorTypographyFromSettings(settings)
        assertEquals(20f, typography.fontSizeSp, 0.001f)
    }

    @Test
    fun editorTypographyFromSettings_preservesNonDefaultLineSpacing() {
        val settings = EditorSettingsState(lineSpacingMultiplier = 2.0f)
        val typography = editorTypographyFromSettings(settings)
        assertEquals(2.0f, typography.lineSpacingMultiplier, 0.001f)
    }

    @Test
    fun editorTypographyFromSettings_preservesAutoIndentSettings() {
        val settings = EditorSettingsState(autoIndentEnabled = false, autoIndentWidth = 4f)
        val typography = editorTypographyFromSettings(settings)
        assertEquals(false, typography.autoIndentEnabled)
        assertEquals(4f, typography.autoIndentWidth, 0.001f)
    }

    @Test
    fun editorTypographyFromSettings_defaultSettings_matchesDefaultTypography() {
        // 默认设置 → 默认排版参数
        val settings = EditorSettingsState()
        val typography = editorTypographyFromSettings(settings)
        assertEquals(16f, typography.fontSizeSp, 0.001f)
        assertEquals(1.5f, typography.lineSpacingMultiplier, 0.001f)
        assertEquals(true, typography.autoIndentEnabled)
        assertEquals(2.0f, typography.autoIndentWidth, 0.001f)
    }

    @Test
    fun editorTypographyFromSettings_nonDefault_doesNotMatchDefault() {
        // 非默认持久化设置构造的 typography 不等于默认 typography —
        // 证明首帧用持久化设置而非默认值。
        val nonDefault = editorTypographyFromSettings(EditorSettingsState(fontSize = 24f))
        val default = editorTypographyFromSettings(EditorSettingsState())
        assertTrue("非默认设置应产生不同 typography", nonDefault != default)
    }
}
