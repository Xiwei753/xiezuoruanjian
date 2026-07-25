package com.xiwei.sujian.ui

import androidx.test.ext.junit.runners.AndroidJUnit4
import com.xiwei.sujian.support.AndroidTestEnvironment
import com.xiwei.sujian.support.TestSession
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.rules.RuleChain
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class MainActivitySmokeTest {

    private val testRule = AndroidTestEnvironment.TestDependenciesRule()

    @get:Rule
    val ruleChain: RuleChain = RuleChain
        .outerRule(testRule)

    @Test
    fun testMainActivityLaunch() {
        val session = AndroidTestEnvironment.requireCurrentSession()
        session.launchActivity()
        val activity = session.withActivity { it }
        assertNotNull("Activity should be launched via TestSession", activity)
    }
}
