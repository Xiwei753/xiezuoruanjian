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

    @Test
    fun typingAnimationToggle_persistsAcrossRecreate() {
        val scenario = composeTestRule.activityRule.scenario

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

        scenario.recreate()

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
    fun fontSizeSlider_persistsAcrossRecreate() {
        val scenario = composeTestRule.activityRule.scenario

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.NavigationSettings)
        composeTestRule.onNodeWithTag(SujianSemanticIds.NavigationSettings).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.SettingsScreen)

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.SettingsNavAppearance)
        composeTestRule.onNodeWithTag(SujianSemanticIds.SettingsNavAppearance).performClick()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.SettingsFontSize)
        composeTestRule.onNodeWithTag(SujianSemanticIds.SettingsFontSize).assertIsDisplayed()

        scenario.recreate()

        ComposeWait.waitForTag(composeTestRule, SujianSemanticIds.SettingsFontSize, timeoutMs = 15_000)
        composeTestRule.onNodeWithTag(SujianSemanticIds.SettingsFontSize).assertIsDisplayed()
    }
}
