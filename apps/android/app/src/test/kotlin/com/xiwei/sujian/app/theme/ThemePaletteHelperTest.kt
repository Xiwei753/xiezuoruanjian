package com.xiwei.sujian.app.theme

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertSame
import org.junit.Test

class ThemePaletteHelperTest {
    @Test
    fun themeColorSchemeDto_allFieldsAreCamelCase() {
        val dto =
            com.xiwei.sujian.app.theme.model.ThemeColorScheme(
                primary = "#006497",
                onPrimary = "#FFFFFF",
                primaryContainer = "#CCE5FF",
                onPrimaryContainer = "#001E31",
                inversePrimary = "#87CEFF",
                secondary = "#50606E",
                onSecondary = "#FFFFFF",
                secondaryContainer = "#D3E5F5",
                onSecondaryContainer = "#0C1D29",
                tertiary = "#65587B",
                onTertiary = "#FFFFFF",
                tertiaryContainer = "#EBDDFF",
                onTertiaryContainer = "#201634",
                background = "#F6FAFE",
                onBackground = "#171C1F",
                surface = "#F6FAFE",
                onSurface = "#171C1F",
                surfaceVariant = "#DEE3EB",
                onSurfaceVariant = "#42474E",
                surfaceTint = "#006497",
                surfaceDim = "#D7DADE",
                surfaceBright = "#F6FAFE",
                surfaceContainerLowest = "#FFFFFF",
                surfaceContainerLow = "#F0F4F8",
                surfaceContainer = "#EBEEF2",
                surfaceContainerHigh = "#E5E8EC",
                surfaceContainerHighest = "#DFE3E7",
                inverseSurface = "#2C3134",
                inverseOnSurface = "#ECF0F4",
                error = "#BA1A1A",
                onError = "#FFFFFF",
                errorContainer = "#FFDAD6",
                onErrorContainer = "#410002",
                outline = "#72787E",
                outlineVariant = "#C2C8CE",
                scrim = "#000000",
                primaryFixed = "#C9E6FF",
                primaryFixedDim = "#A5CCF0",
                onPrimaryFixed = "#001E2F",
                onPrimaryFixedVariant = "#004B6E",
                secondaryFixed = "#D3E5F5",
                secondaryFixedDim = "#B7C9D8",
                onSecondaryFixed = "#0C1D29",
                onSecondaryFixedVariant = "#384956",
                tertiaryFixed = "#EBDDFF",
                tertiaryFixedDim = "#CFC0E7",
                onTertiaryFixed = "#201634",
                onTertiaryFixedVariant = "#4D4462",
            )
        assertEquals("#006497", dto.primary)
        assertEquals("#FFFFFF", dto.onPrimary)
        assertEquals("#CCE5FF", dto.primaryContainer)
        assertEquals("#001E31", dto.onPrimaryContainer)
        assertEquals("#87CEFF", dto.inversePrimary)
        assertEquals("#50606E", dto.secondary)
        assertEquals("#FFFFFF", dto.onSecondary)
        assertEquals("#D3E5F5", dto.secondaryContainer)
        assertEquals("#0C1D29", dto.onSecondaryContainer)
        assertEquals("#65587B", dto.tertiary)
        assertEquals("#FFFFFF", dto.onTertiary)
        assertEquals("#EBDDFF", dto.tertiaryContainer)
        assertEquals("#201634", dto.onTertiaryContainer)
        assertEquals("#F6FAFE", dto.background)
        assertEquals("#171C1F", dto.onBackground)
        assertEquals("#F6FAFE", dto.surface)
        assertEquals("#171C1F", dto.onSurface)
        assertEquals("#DEE3EB", dto.surfaceVariant)
        assertEquals("#42474E", dto.onSurfaceVariant)
        assertEquals("#006497", dto.surfaceTint)
        assertEquals("#D7DADE", dto.surfaceDim)
        assertEquals("#F6FAFE", dto.surfaceBright)
        assertEquals("#FFFFFF", dto.surfaceContainerLowest)
        assertEquals("#F0F4F8", dto.surfaceContainerLow)
        assertEquals("#EBEEF2", dto.surfaceContainer)
        assertEquals("#E5E8EC", dto.surfaceContainerHigh)
        assertEquals("#DFE3E7", dto.surfaceContainerHighest)
        assertEquals("#2C3134", dto.inverseSurface)
        assertEquals("#ECF0F4", dto.inverseOnSurface)
        assertEquals("#BA1A1A", dto.error)
        assertEquals("#FFFFFF", dto.onError)
        assertEquals("#FFDAD6", dto.errorContainer)
        assertEquals("#410002", dto.onErrorContainer)
        assertEquals("#72787E", dto.outline)
        assertEquals("#C2C8CE", dto.outlineVariant)
        assertEquals("#000000", dto.scrim)
        assertEquals("#C9E6FF", dto.primaryFixed)
        assertEquals("#A5CCF0", dto.primaryFixedDim)
        assertEquals("#001E2F", dto.onPrimaryFixed)
        assertEquals("#004B6E", dto.onPrimaryFixedVariant)
        assertEquals("#D3E5F5", dto.secondaryFixed)
        assertEquals("#B7C9D8", dto.secondaryFixedDim)
        assertEquals("#0C1D29", dto.onSecondaryFixed)
        assertEquals("#384956", dto.onSecondaryFixedVariant)
        assertEquals("#EBDDFF", dto.tertiaryFixed)
        assertEquals("#CFC0E7", dto.tertiaryFixedDim)
        assertEquals("#201634", dto.onTertiaryFixed)
        assertEquals("#4D4462", dto.onTertiaryFixedVariant)
    }

    @Test
    fun themePaletteRecordDto_allFieldsAreCamelCase() {
        val scheme =
            com.xiwei.sujian.app.theme.model.ThemeColorScheme(
                primary = "#006497",
                onPrimary = "#FFFFFF",
                primaryContainer = "#CCE5FF",
                onPrimaryContainer = "#001E31",
                inversePrimary = "#87CEFF",
                secondary = "#50606E",
                onSecondary = "#FFFFFF",
                secondaryContainer = "#D3E5F5",
                onSecondaryContainer = "#0C1D29",
                tertiary = "#65587B",
                onTertiary = "#FFFFFF",
                tertiaryContainer = "#EBDDFF",
                onTertiaryContainer = "#201634",
                background = "#F6FAFE",
                onBackground = "#171C1F",
                surface = "#F6FAFE",
                onSurface = "#171C1F",
                surfaceVariant = "#DEE3EB",
                onSurfaceVariant = "#42474E",
                surfaceTint = "#006497",
                surfaceDim = "#D7DADE",
                surfaceBright = "#F6FAFE",
                surfaceContainerLowest = "#FFFFFF",
                surfaceContainerLow = "#F0F4F8",
                surfaceContainer = "#EBEEF2",
                surfaceContainerHigh = "#E5E8EC",
                surfaceContainerHighest = "#DFE3E7",
                inverseSurface = "#2C3134",
                inverseOnSurface = "#ECF0F4",
                error = "#BA1A1A",
                onError = "#FFFFFF",
                errorContainer = "#FFDAD6",
                onErrorContainer = "#410002",
                outline = "#72787E",
                outlineVariant = "#C2C8CE",
                scrim = "#000000",
                primaryFixed = "#C9E6FF",
                primaryFixedDim = "#A5CCF0",
                onPrimaryFixed = "#001E2F",
                onPrimaryFixedVariant = "#004B6E",
                secondaryFixed = "#D3E5F5",
                secondaryFixedDim = "#B7C9D8",
                onSecondaryFixed = "#0C1D29",
                onSecondaryFixedVariant = "#384956",
                tertiaryFixed = "#EBDDFF",
                tertiaryFixedDim = "#CFC0E7",
                onTertiaryFixed = "#201634",
                onTertiaryFixedVariant = "#4D4462",
            )
        val record =
            com.xiwei.sujian.app.theme.model.ThemePaletteRecord(
                schemaVersion = 1u,
                paletteId = "test-palette",
                paletteFingerprint = "fp123",
                source = "android_dynamic_color",
                sourcePlatform = "android",
                sourceDeviceId = "device001",
                sourceDeviceClass = "phone",
                capturedAtMs = 1719792000000L,
                variant = "tonal_spot",
                lightScheme = scheme,
                darkScheme = scheme,
            )
        assertEquals(1u, record.schemaVersion)
        assertEquals("test-palette", record.paletteId)
        assertEquals("fp123", record.paletteFingerprint)
        assertEquals("android_dynamic_color", record.source)
        assertEquals("android", record.sourcePlatform)
        assertEquals("device001", record.sourceDeviceId)
        assertEquals("phone", record.sourceDeviceClass)
        assertEquals(1719792000000L, record.capturedAtMs)
        assertEquals("tonal_spot", record.variant)
        assertNotNull(record.lightScheme)
        assertNotNull(record.darkScheme)
    }

    @Test
    fun dynamicColorResult_holdsBothSchemes() {
        val scheme =
            com.xiwei.sujian.app.theme.model.ThemeColorScheme(
                primary = "#006497",
                onPrimary = "#FFFFFF",
                primaryContainer = "#CCE5FF",
                onPrimaryContainer = "#001E31",
                inversePrimary = "#87CEFF",
                secondary = "#50606E",
                onSecondary = "#FFFFFF",
                secondaryContainer = "#D3E5F5",
                onSecondaryContainer = "#0C1D29",
                tertiary = "#65587B",
                onTertiary = "#FFFFFF",
                tertiaryContainer = "#EBDDFF",
                onTertiaryContainer = "#201634",
                background = "#F6FAFE",
                onBackground = "#171C1F",
                surface = "#F6FAFE",
                onSurface = "#171C1F",
                surfaceVariant = "#DEE3EB",
                onSurfaceVariant = "#42474E",
                surfaceTint = "#006497",
                surfaceDim = "#D7DADE",
                surfaceBright = "#F6FAFE",
                surfaceContainerLowest = "#FFFFFF",
                surfaceContainerLow = "#F0F4F8",
                surfaceContainer = "#EBEEF2",
                surfaceContainerHigh = "#E5E8EC",
                surfaceContainerHighest = "#DFE3E7",
                inverseSurface = "#2C3134",
                inverseOnSurface = "#ECF0F4",
                error = "#BA1A1A",
                onError = "#FFFFFF",
                errorContainer = "#FFDAD6",
                onErrorContainer = "#410002",
                outline = "#72787E",
                outlineVariant = "#C2C8CE",
                scrim = "#000000",
                primaryFixed = "#C9E6FF",
                primaryFixedDim = "#A5CCF0",
                onPrimaryFixed = "#001E2F",
                onPrimaryFixedVariant = "#004B6E",
                secondaryFixed = "#D3E5F5",
                secondaryFixedDim = "#B7C9D8",
                onSecondaryFixed = "#0C1D29",
                onSecondaryFixedVariant = "#384956",
                tertiaryFixed = "#EBDDFF",
                tertiaryFixedDim = "#CFC0E7",
                onTertiaryFixed = "#201634",
                onTertiaryFixedVariant = "#4D4462",
            )
        val result =
            ThemePaletteHelper.DynamicColorResult(
                lightScheme = scheme,
                darkScheme = scheme,
            )
        assertSame(scheme, result.lightScheme)
        assertSame(scheme, result.darkScheme)
    }
}
