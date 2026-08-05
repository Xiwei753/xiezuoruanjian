package com.xiwei.sujian.ui.phone.portrait

import com.xiwei.sujian.platform.api.DeviceCategory
import com.xiwei.sujian.platform.api.FoldPosture
import com.xiwei.sujian.platform.api.WindowSizeClass
import com.xiwei.sujian.ui.compose.navigation.resolvePhonePortraitPolicy
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PhoneLayoutPolicyTest {
    @Test
    fun regularPhonePortrait_entersPhoneShell() {
        assertTrue(
            resolvePhonePortraitPolicy(
                windowSizeClass = WindowSizeClass.Compact,
                screenWidthDp = 411,
                screenHeightDp = 891,
                foldPosture = FoldPosture.None,
                deviceCategory = DeviceCategory.Phone,
            ),
        )
    }

    @Test
    fun landscapePhone_doesNotEnterPhoneShell() {
        assertFalse(
            resolvePhonePortraitPolicy(
                windowSizeClass = WindowSizeClass.Compact,
                screenWidthDp = 891,
                screenHeightDp = 411,
                foldPosture = FoldPosture.None,
                deviceCategory = DeviceCategory.Phone,
            ),
        )
    }

    @Test
    fun widePhone_doesNotEnterPhoneShell() {
        assertFalse(
            resolvePhonePortraitPolicy(
                windowSizeClass = WindowSizeClass.Medium,
                screenWidthDp = 700,
                screenHeightDp = 900,
                foldPosture = FoldPosture.None,
                deviceCategory = DeviceCategory.Phone,
            ),
        )
    }

    @Test
    fun foldableWithPosture_doesNotEnterPhoneShell() {
        assertFalse(
            resolvePhonePortraitPolicy(
                windowSizeClass = WindowSizeClass.Compact,
                screenWidthDp = 411,
                screenHeightDp = 891,
                foldPosture = FoldPosture.Flat,
                deviceCategory = DeviceCategory.Foldable,
            ),
        )
    }

    @Test
    fun tabletPortrait_doesNotEnterPhoneShell() {
        assertFalse(
            resolvePhonePortraitPolicy(
                windowSizeClass = WindowSizeClass.Medium,
                screenWidthDp = 800,
                screenHeightDp = 1280,
                foldPosture = FoldPosture.None,
                deviceCategory = DeviceCategory.Tablet,
            ),
        )
    }

    @Test
    fun compactPortraitWithFoldableDeviceCategory_doesNotEnterPhoneShell() {
        assertFalse(
            resolvePhonePortraitPolicy(
                windowSizeClass = WindowSizeClass.Compact,
                screenWidthDp = 411,
                screenHeightDp = 891,
                foldPosture = FoldPosture.None,
                deviceCategory = DeviceCategory.Foldable,
            ),
        )
    }
}
