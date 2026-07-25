package com.xiwei.sujian.settings

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsOff
import androidx.compose.ui.test.assertIsOn
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
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
        session.deps.coordinator.releaseHost()
        val ctx = androidx.test.platform.app.InstrumentationRegistry.getInstrumentation().targetContext
        val newSession = AndroidTestEnvironment.createSession(ctx)
        com.xiwei.sujian.runtime.SujianAppDependencies.setTestProvider { _ -> newSession.deps }
        androidx.test.core.app.ActivityScenario.launch(MainActivity::class.java)
    }

    @Test
    fun typingAnimationToggle_persistsAfterRestart() {
        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.NavigationSettings)
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationSettings).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.SettingsScreen)

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.SettingsNavEditor)
        composeTestRule.onNodeWithTag(SujianSemanticIds.SettingsNavEditor).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.SettingsTypingAnimation)
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
            } catch (_: AssertionError) {
                false
            }
        }, timeoutMs = 5_000)

        restartRuntime()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.NavigationSettings, timeoutMs = 15_000)
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationSettings).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.SettingsScreen)

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.SettingsNavEditor)
        composeTestRule.onNodeWithTag(SujianSemanticIds.SettingsNavEditor).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.SettingsTypingAnimation, timeoutMs = 15_000)
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

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.NavigationSettings)
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationSettings).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.SettingsScreen)

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.SettingsNavAppearance)
        composeTestRule.onNodeWithTag(SujianSemanticIds.SettingsNavAppearance).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.SettingsFontSize)
        composeTestRule.onNodeWithTag(SujianSemanticIds.SettingsFontSize).assertIsDisplayed()

        val targetFontSize = 22f
        assert(initialFontSize != targetFontSize) {
            "Initial font size ($initialFontSize) should differ from target ($targetFontSize) for a meaningful test"
        }

        session.deps.settingsRepository.setFontSize(targetFontSize)

        ComposeWait.waitUntil(composeTestRule, {
            val currentFontSize = session.deps.settingsRepository.getEffectiveFontSize()
            currentFontSize == targetFontSize
        }, timeoutMs = 5_000)

        restartRuntime()

        val newSession = AndroidTestEnvironment.requireCurrentSession()
        val restoredFontSize = newSession.deps.settingsRepository.getEffectiveFontSize()
        assert(restoredFontSize == targetFontSize) {
            "Font size after restart should be $targetFontSize but was $restoredFontSize"
        }
    }
}
