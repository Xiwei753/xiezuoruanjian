package com.xiwei.sujian.ui

import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [33])
class ErrorUtilTest {

    @Test
    fun testSafeRun_success() {
        val context = RuntimeEnvironment.getApplication()
        val result = ErrorUtil.safeRun(context, "fallback") {
            "success"
        }
        assertEquals("success", result)
    }

    @Test
    fun testSafeRun_exceptionReturnsFallback() {
        val context = RuntimeEnvironment.getApplication()
        val result = ErrorUtil.safeRun(context, "fallback") {
            throw Exception("Test exception")
        }
        assertEquals("fallback", result)
    }
}
