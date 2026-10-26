package com.xiwei.sujian.app.theme.interop

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import com.xiwei.sujian.app.theme.model.ThemeColorScheme
import com.xiwei.sujian.app.theme.model.ThemePaletteRecord
import com.xiwei.sujian.core.platform.storage.AndroidPrivateDataRoot
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import java.io.File

/** Issue #1005：旧 Android 本地 Core catalog 升级导入测试。 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class LocalThemePaletteStoreTest {
    private lateinit var context: Context
    private lateinit var store: LocalThemePaletteStore

    @Before
    fun setUp() {
        context = ApplicationProvider.getApplicationContext()
        context.filesDir.deleteRecursively()
        store = LocalThemePaletteStore(context)
    }

    @Test
    fun legacyCoreCatalog_isImportedWithoutDeletingOldFile() {
        val oldFile =
            File(
                AndroidPrivateDataRoot.root(context),
                "themes/palettes/device001/legacyfp.json",
            )
        oldFile.parentFile?.mkdirs()
        oldFile.writeText(ThemePaletteRecordJson.encode(record("legacyfp")), Charsets.UTF_8)

        val records = store.list()

        assertEquals(1, records.size)
        assertEquals("device001:legacyfp", records.single().paletteId)
        assertTrue(oldFile.exists())
        val imported =
            File(
                AndroidPrivateDataRoot.localThemePalettes(context),
                "device001/legacyfp.json",
            )
        assertTrue(imported.isFile)
    }

    @Test
    fun delete_removesLegacyAndImportedSnapshot() {
        val oldFile =
            File(
                AndroidPrivateDataRoot.root(context),
                "themes/palettes/device001/legacyfp.json",
            )
        oldFile.parentFile?.mkdirs()
        oldFile.writeText(ThemePaletteRecordJson.encode(record("legacyfp")), Charsets.UTF_8)

        assertTrue(store.delete("device001", "legacyfp"))

        assertFalse(oldFile.exists())
        val imported =
            File(
                AndroidPrivateDataRoot.localThemePalettes(context),
                "device001/legacyfp.json",
            )
        assertFalse(imported.exists())
        assertTrue(store.list().isEmpty())
    }

    @Test
    fun save_rejectsSameColorSnapshot() {
        val readable = record("low-contrast")
        val unreadableScheme =
            readable.lightScheme.copy(
                onSurface = readable.lightScheme.surface,
                onBackground = readable.lightScheme.background,
                onSurfaceVariant = readable.lightScheme.surface,
            )
        val unreadable = readable.copy(lightScheme = unreadableScheme, darkScheme = unreadableScheme)

        assertFalse(ThemePaletteRecordJson.isRecordValid(unreadable))
        assertFalse(store.save(unreadable))
        assertTrue(store.list().isEmpty())
    }

    private fun record(fingerprint: String): ThemePaletteRecord {
        val scheme =
            ThemeColorScheme(
                primary = "#6750A4",
                onPrimary = "#FFFFFF",
                primaryContainer = "#EADDFF",
                onPrimaryContainer = "#21005D",
                inversePrimary = "#D0BCFF",
                secondary = "#625B71",
                onSecondary = "#FFFFFF",
                secondaryContainer = "#E8DEF8",
                onSecondaryContainer = "#1D192B",
                tertiary = "#7D5260",
                onTertiary = "#FFFFFF",
                tertiaryContainer = "#FFD8E4",
                onTertiaryContainer = "#31111D",
                background = "#FFFBFE",
                onBackground = "#1C1B1F",
                surface = "#FFFBFE",
                onSurface = "#1C1B1F",
                surfaceVariant = "#E7E0EC",
                onSurfaceVariant = "#49454F",
                surfaceTint = "#6750A4",
                surfaceDim = "#DED8E1",
                surfaceBright = "#FFFBFE",
                surfaceContainerLowest = "#FFFFFF",
                surfaceContainerLow = "#F7F2FA",
                surfaceContainer = "#F3EDF7",
                surfaceContainerHigh = "#ECE6F0",
                surfaceContainerHighest = "#E6E0E9",
                inverseSurface = "#313033",
                inverseOnSurface = "#F4EFF4",
                error = "#B3261E",
                onError = "#FFFFFF",
                errorContainer = "#F9DEDC",
                onErrorContainer = "#410E0B",
                outline = "#79747E",
                outlineVariant = "#C4C7C5",
                scrim = "#000000",
                primaryFixed = "#EADDFF",
                primaryFixedDim = "#D0BCFF",
                onPrimaryFixed = "#21005D",
                onPrimaryFixedVariant = "#4F378B",
                secondaryFixed = "#E8DEF8",
                secondaryFixedDim = "#CCC2DC",
                onSecondaryFixed = "#1D192B",
                onSecondaryFixedVariant = "#4A4458",
                tertiaryFixed = "#FFD8E4",
                tertiaryFixedDim = "#EFB8C8",
                onTertiaryFixed = "#31111D",
                onTertiaryFixedVariant = "#633B48",
            )
        return ThemePaletteRecord(
            schemaVersion = 1u,
            paletteId = "device001:$fingerprint",
            paletteFingerprint = fingerprint,
            source = "android_dynamic_color",
            sourcePlatform = "android",
            sourceDeviceId = "device001",
            sourceDeviceClass = "phone",
            capturedAtMs = 1719792000000L,
            variant = "system_selected",
            lightScheme = scheme,
            darkScheme = scheme,
        )
    }
}
