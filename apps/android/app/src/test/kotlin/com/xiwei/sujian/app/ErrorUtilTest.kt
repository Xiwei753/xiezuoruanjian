package com.xiwei.sujian.app

import com.xiwei.sujian.core.interop.common.RepositoryException
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
    fun testSafeRunSuccess() {
        val context = RuntimeEnvironment.getApplication()
        val fallback = "fallback"
        val expected = "success"

        val result =
            ErrorUtil.safeRun(context, fallback) {
                expected
            }

        assertEquals(expected, result)
    }

    @Test
    fun testSafeRunExceptionFallback() {
        val context = RuntimeEnvironment.getApplication()
        val fallback = "fallback"

        val result =
            ErrorUtil.safeRun(context, fallback) {
                throw Exception("Test exception")
            }

        assertEquals(fallback, result)
    }

    @Test
    fun testSafeRunRepositoryExceptionFallback() {
        val context = RuntimeEnvironment.getApplication()
        val fallback = "fallback"

        val result =
            ErrorUtil.safeRun(context, fallback) {
                throw RepositoryException("Test RepositoryException")
            }

        assertEquals(fallback, result)
    }
}
