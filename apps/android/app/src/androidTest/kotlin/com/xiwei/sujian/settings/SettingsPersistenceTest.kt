package com.xiwei.sujian.settings

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsOff
import androidx.compose.ui.test.assertIsOn
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.geometry.Offset
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.xiwei.sujian.ui.MainActivity
import com.xiwei.sujian.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.support.AndroidTestEnvironment
import com.xiwei.sujian.support.ComposeWait
import com.xiwei.sujian.support.TestSession
import org.junit.Rule
import org.junit.Test
import org.junit.rules.RuleChain
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class SettingsPersistenceTest {

    private val _composeTestRule = createAndroidComposeRule<MainActivity>()

    @get:Rule
    val ruleChain: RuleChain = RuleChain
        .outerRule(AndroidTestEnvironment.TestDependenciesRule())
        .around(_composeTestRule)

    private val composeTestRule get() = _composeTestRule

    private fun getSession(): TestSession = AndroidTestEnvironment.requireCurrentSession()

    private fun restartRuntime() {
        val session = getSession()
        composeTestRule.activityRule.scenario.close()
        session.restartRuntime()
        com.xiwei.sujian.runtime.SujianAppDependencies.setTestProvider { _ -> session.deps }
        androidx.test.core.app.ActivityScenario.launch(MainActivity::class.java)
    }

    private fun navigateToEditorSettings() {
        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.NavigationSettings)
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationSettings).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.SettingsScreen)

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.SettingsNavEditor)
        composeTestRule.onNodeWithTag(SujianSemanticIds.SettingsNavEditor).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.SettingsTypingAnimation)
    }

    @Test
    fun typingAnimationToggle_persistsAfterRestart() {
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

        if (wasOn) {
            toggleNode.assertIsOff()
        } else {
            toggleNode.assertIsOn()
        }

        ComposeWait.waitUntil(composeTestRule, {
            try {
                if (wasOn) {
                    composeTestRule.onNodeWithTag(SujianSemanticIds.SettingsTypingAnimation).assertIsOff()
                } else {
                    composeTestRule.onNodeWithTag(SujianSemanticIds.SettingsTypingAnimation).assertIsOn()
                }
                true
            } catch (e: AssertionError) {
                throw AssertionError("Typing animation toggle did not reflect expected state after click: ${e.message}")
            }
        }, timeoutMs = 5_000)

        restartRuntime()

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
    fun fontSizeSlider_persistsAfterRestart() {
        val session = getSession()
        val initialFontSize = session.deps.settingsRepository.getEffectiveFontSize()
        val targetFontSize = 22f

        assert(initialFontSize != targetFontSize) {
            "Initial font size ($initialFontSize) should differ from target ($targetFontSize) for a meaningful test"
        }

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.NavigationSettings)
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationSettings).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.SettingsScreen)

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.SettingsNavAppearance)
        composeTestRule.onNodeWithTag(SujianSemanticIds.SettingsNavAppearance).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.SettingsFontSize)
        val sliderNode = composeTestRule.onNodeWithTag(SujianSemanticIds.SettingsFontSize)
        sliderNode.assertIsDisplayed()

        val valueRange = 12f..72f
        val targetProgress = (targetFontSize - valueRange.start) / (valueRange.endInclusive - valueRange.start)

        sliderNode.performTouchInput {
            val targetX = width * targetProgress
            down(Offset(targetX, height / 2f))
            up()
        }

        ComposeWait.waitUntil(composeTestRule, {
            val currentFontSize = session.deps.settingsRepository.getEffectiveFontSize()
            currentFontSize == targetFontSize
        }, timeoutMs = 10_000, message = "Font size did not update to $targetFontSize after Slider interaction")

        restartRuntime()

        val newSession = AndroidTestEnvironment.requireCurrentSession()
        val restoredFontSize = newSession.deps.settingsRepository.getEffectiveFontSize()
        assert(restoredFontSize == targetFontSize) {
            "Font size after restart should be $targetFontSize but was $restoredFontSize"
        }

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
        assert(restoredStateDesc.contains("22")) {
            "Slider state description should contain '22' after restart but was '$restoredStateDesc'"
        }
    }
}
