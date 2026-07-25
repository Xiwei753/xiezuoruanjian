package com.xiwei.sujian.ui

import androidx.test.ext.junit.runners.AndroidJUnit4
import com.xiwei.sujian.support.AndroidTestEnvironment
import com.xiwei.sujian.support.RestartableMainActivityRule
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.rules.RuleChain
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class MainActivitySmokeTest {

    private val activityRule = RestartableMainActivityRule { AndroidTestEnvironment.requireCurrentSession() }

    @get:Rule
    val ruleChain: RuleChain = RuleChain
        .outerRule(AndroidTestEnvironment.TestDependenciesRule())
        .around(activityRule)

    @Test
    fun testMainActivityLaunch() {
        val activity = activityRule.getActivity()
        assertNotNull("Activity should be launched via RestartableMainActivityRule", activity)
    }
}
