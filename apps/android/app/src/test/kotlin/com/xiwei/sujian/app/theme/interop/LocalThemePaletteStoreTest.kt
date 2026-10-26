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

    private fun record(fingerprint: String): ThemePaletteRecord {
        val scheme =
            ThemeColorScheme(
                primary = "#123456",
                onPrimary = "#123456",
                primaryContainer = "#123456",
                onPrimaryContainer = "#123456",
                inversePrimary = "#123456",
                secondary = "#123456",
                onSecondary = "#123456",
                secondaryContainer = "#123456",
                onSecondaryContainer = "#123456",
                tertiary = "#123456",
                onTertiary = "#123456",
                tertiaryContainer = "#123456",
                onTertiaryContainer = "#123456",
                background = "#123456",
                onBackground = "#123456",
                surface = "#123456",
                onSurface = "#123456",
                surfaceVariant = "#123456",
                onSurfaceVariant = "#123456",
                surfaceTint = "#123456",
                surfaceDim = "#123456",
                surfaceBright = "#123456",
                surfaceContainerLowest = "#123456",
                surfaceContainerLow = "#123456",
                surfaceContainer = "#123456",
                surfaceContainerHigh = "#123456",
                surfaceContainerHighest = "#123456",
                inverseSurface = "#123456",
                inverseOnSurface = "#123456",
                error = "#123456",
                onError = "#123456",
                errorContainer = "#123456",
                onErrorContainer = "#123456",
                outline = "#123456",
                outlineVariant = "#123456",
                scrim = "#123456",
                primaryFixed = "#123456",
                primaryFixedDim = "#123456",
                onPrimaryFixed = "#123456",
                onPrimaryFixedVariant = "#123456",
                secondaryFixed = "#123456",
                secondaryFixedDim = "#123456",
                onSecondaryFixed = "#123456",
                onSecondaryFixedVariant = "#123456",
                tertiaryFixed = "#123456",
                tertiaryFixedDim = "#123456",
                onTertiaryFixed = "#123456",
                onTertiaryFixedVariant = "#123456",
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
