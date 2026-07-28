package com.xiwei.sujian.ui.compose.workbench

import com.xiwei.sujian.ui.compose.workbench.state.LayoutStorageKey
import com.xiwei.sujian.ui.compose.workbench.state.WindowWidthBucket
import org.junit.Assert.assertEquals
import org.junit.Test

class WindowWidthBucketTest {

    @Test
    fun fromDp_compact() {
        assertEquals(WindowWidthBucket.Compact, WindowWidthBucket.fromDp(400))
    }

    @Test
    fun fromDp_medium() {
        assertEquals(WindowWidthBucket.Medium, WindowWidthBucket.fromDp(700))
    }

    @Test
    fun fromDp_expanded() {
        assertEquals(WindowWidthBucket.Expanded, WindowWidthBucket.fromDp(1000))
    }

    @Test
    fun fromDp_large() {
        assertEquals(WindowWidthBucket.Large, WindowWidthBucket.fromDp(1400))
    }

    @Test
    fun fromDp_boundaryCompactMedium() {
        assertEquals(WindowWidthBucket.Compact, WindowWidthBucket.fromDp(599))
        assertEquals(WindowWidthBucket.Medium, WindowWidthBucket.fromDp(600))
    }

    @Test
    fun fromDp_boundaryMediumExpanded() {
        assertEquals(WindowWidthBucket.Medium, WindowWidthBucket.fromDp(839))
        assertEquals(WindowWidthBucket.Expanded, WindowWidthBucket.fromDp(840))
    }

    @Test
    fun fromDp_boundaryExpandedLarge() {
        assertEquals(WindowWidthBucket.Expanded, WindowWidthBucket.fromDp(1199))
        assertEquals(WindowWidthBucket.Large, WindowWidthBucket.fromDp(1200))
    }

    @Test
    fun storageKey_format() {
        val key = LayoutStorageKey(
            deviceId = "dev1",
            orientation = "landscape",
            windowWidthBucket = WindowWidthBucket.Expanded,
            windowMode = "standard",
        )
        assertEquals("dev1|landscape|expanded|standard", key.toStorageKey())
    }
}
