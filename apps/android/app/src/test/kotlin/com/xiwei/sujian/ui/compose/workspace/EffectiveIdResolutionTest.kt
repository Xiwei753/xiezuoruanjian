package com.xiwei.sujian.ui.compose.workspace

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class EffectiveIdResolutionTest {

    @Test
    fun routeDriven_nullOverride_returnsNull() {
        assertNull(resolveEffectiveId(overrideValue = null, appStateValue = "app-project", isRouteDriven = true))
    }

    @Test
    fun routeDriven_setOverride_returnsOverride() {
        assertEquals("route-project", resolveEffectiveId("route-project", "app-project", isRouteDriven = true))
    }

    @Test
    fun routeDriven_nullOverride_nullAppState_returnsNull() {
        assertNull(resolveEffectiveId(null, null, isRouteDriven = true))
    }

    @Test
    fun localMode_nullOverride_returnsAppStateValue() {
        assertEquals("app-project", resolveEffectiveId(null, "app-project", isRouteDriven = false))
    }

    @Test
    fun localMode_setOverride_returnsOverride() {
        assertEquals("override", resolveEffectiveId("override", "app-project", isRouteDriven = false))
    }

    @Test
    fun localMode_nullOverride_nullAppState_returnsNull() {
        assertNull(resolveEffectiveId(null, null, isRouteDriven = false))
    }
}
