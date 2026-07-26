package com.xiwei.sujian.settings

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsOff
import androidx.compose.ui.test.assertIsOn
import androidx.compose.ui.test.junit4.AndroidComposeTestRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.semantics.SemanticsActions
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.xiwei.sujian.ui.MainActivity
import com.xiwei.sujian.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.support.AndroidTestEnvironment
import com.xiwei.sujian.support.ComposeWait
import com.xiwei.sujian.support.RestartableMainActivityRule
import com.xiwei.sujian.support.TestSession
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.RuleChain
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class SettingsPersistenceTest {

    private val activityRule = RestartableMainActivityRule { AndroidTestEnvironment.requireCurrentSession() }

    private val _composeTestRule = AndroidComposeTestRule(
        activityRule,
        activityRule.composeActivityProvider
    ).also { activityRule.setComposeTestRule(it) }

    @get:Rule
    val ruleChain: RuleChain = RuleChain
        .outerRule(AndroidTestEnvironment.TestDependenciesRule())
        .around(_composeTestRule)

    private val composeTestRule get() = _composeTestRule

    private fun getSession(): TestSession = AndroidTestEnvironment.requireCurrentSession()

    private fun navigateToEditorSettings() {
        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.NavigationSettings)
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationSettings).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.SettingsScreen)

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.SettingsNavEditor)
        composeTestRule.onNodeWithTag(SujianSemanticIds.SettingsNavEditor).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.SettingsTypingAnimation)
    }

    @Test
    fun typingAnimationToggle_persistsAfterColdRestart() {
        val session = getSession()

        navigateToEditorSettings()

        val toggleNode = composeTestRule.onNodeWithTag(SujianSemanticIds.SettingsTypingAnimation)
        toggleNode.assertIsDisplayed()

        val wasOn = try {
            toggleNode.assertIsOn()
            true
        } catch (_: AssertionError) {
            false
        }

        toggleNode.performClick()

        val expectedEnabled = !wasOn
        ComposeWait.waitUntil(composeTestRule, {
            try {
                if (wasOn) {
                    composeTestRule.onNodeWithTag(SujianSemanticIds.SettingsTypingAnimation).assertIsOff()
                } else {
                    composeTestRule.onNodeWithTag(SujianSemanticIds.SettingsTypingAnimation).assertIsOn()
                }
                true
            } catch (_: AssertionError) {
                false
            }
        }, timeoutMs = 5_000, message = { "Typing animation toggle did not reflect expected state after click" })

        ComposeWait.waitUntil(composeTestRule, {
            session.deps.settingsRepository.getLocalSettings().editorTypingAnimationEnabled == expectedEnabled
        }, timeoutMs = 10_000, message = { "Settings repository did not reflect typing animation change to $expectedEnabled" })

        activityRule.restartRuntimeAndActivity()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.NavigationSettings, timeoutMs = 15_000)
        navigateToEditorSettings()

        val restoredNode = composeTestRule.onNodeWithTag(SujianSemanticIds.SettingsTypingAnimation)
        restoredNode.assertIsDisplayed()
        if (wasOn) {
            restoredNode.assertIsOff()
        } else {
            restoredNode.assertIsOn()
        }
    }

    @Test
    fun fontSizeSlider_persistsAfterColdRestart() {
        val session = getSession()
        val targetFontSize = 22f

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.NavigationSettings)
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationSettings).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.SettingsScreen)

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.SettingsNavAppearance)
        composeTestRule.onNodeWithTag(SujianSemanticIds.SettingsNavAppearance).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.SettingsFontSize)
        val sliderNode = composeTestRule.onNodeWithTag(SujianSemanticIds.SettingsFontSize)
        sliderNode.assertIsDisplayed()

        sliderNode.performSemanticsAction(SemanticsActions.SetProgress) {
            it(targetFontSize)
        }

        ComposeWait.waitUntil(composeTestRule, {
            val currentFontSize = session.deps.settingsRepository.getEffectiveFontSize()
            kotlin.math.abs(currentFontSize - targetFontSize) < 0.5f
        }, timeoutMs = 10_000, message = { "Font size did not update to $targetFontSize after Slider interaction" })

        activityRule.restartRuntimeAndActivity()

        val newSession = AndroidTestEnvironment.requireCurrentSession()
        val restoredFontSize = newSession.deps.settingsRepository.getEffectiveFontSize()
        assertEquals(
            "Font size after cold restart should be $targetFontSize but was $restoredFontSize",
            targetFontSize, restoredFontSize, 0.5f
        )

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.NavigationSettings, timeoutMs = 15_000)
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationSettings).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.SettingsScreen)

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.SettingsNavAppearance)
        composeTestRule.onNodeWithTag(SujianSemanticIds.SettingsNavAppearance).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.SettingsFontSize, timeoutMs = 15_000)
        val restoredSliderNode = composeTestRule.onNodeWithTag(SujianSemanticIds.SettingsFontSize)
        restoredSliderNode.assertIsDisplayed()
        val restoredStateDesc = restoredSliderNode.fetchSemanticsNode().config[
            androidx.compose.ui.semantics.SemanticsProperties.StateDescription
        ]
        assertTrue(
            "Slider state description should contain '22' after cold restart but was '$restoredStateDesc'",
            restoredStateDesc.contains("22")
        )
    }
}
