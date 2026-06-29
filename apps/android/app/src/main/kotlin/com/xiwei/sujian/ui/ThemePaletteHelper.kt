package com.xiwei.sujian.ui

import android.content.Context
import android.os.Build
import android.util.Log
import org.json.JSONObject

/**
 * Helper to extract Android 12+ Dynamic Color (Material You) palette
 * and format it as a cross-platform ThemePalette JSON object.
 *
 * Non-Android clients only consume this; they never produce it.
 */
object ThemePaletteHelper {

    private const val TAG = "ThemePaletteHelper"

    /**
     * Extract the full Material You palette from system resources (Android 12+).
     * Returns a JSON string compatible with Rust core's ThemePalette struct.
     * Returns null if not available (pre-Android 12 or no dynamic color).
     */
    fun extractThemePaletteJson(context: Context): String? {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) return null

        return try {
            val palette = JSONObject()
            palette.put("source", "android_dynamic_color")
            palette.put("updatedAtMs", System.currentTimeMillis())
            palette.put("deviceId", "")  // Will be filled by settings layer
            palette.put("variant", "tonal_spot")

            // Accent colors from system resources
            // Android 12+ provides system_accent1, system_accent2, system_accent3
            // and system_neutral1, system_neutral2 in 10 shades (0-999)
            // We use key shades that map to Material3 semantic tokens

            // Light palette
            putSystemColor(palette, "lightPrimary", context,
                android.R.color.system_accent1_500)
            putSystemColor(palette, "lightOnPrimary", context,
                android.R.color.system_accent1_100)
            putSystemColor(palette, "lightPrimaryContainer", context,
                android.R.color.system_accent1_100)
            putSystemColor(palette, "lightOnPrimaryContainer", context,
                android.R.color.system_accent1_900)
            putSystemColor(palette, "lightSecondary", context,
                android.R.color.system_accent2_500)
            putSystemColor(palette, "lightOnSecondary", context,
                android.R.color.system_accent2_100)
            putSystemColor(palette, "lightSecondaryContainer", context,
                android.R.color.system_accent2_100)
            putSystemColor(palette, "lightOnSecondaryContainer", context,
                android.R.color.system_accent2_900)
            putSystemColor(palette, "lightTertiary", context,
                android.R.color.system_accent3_500)
            putSystemColor(palette, "lightOnTertiary", context,
                android.R.color.system_accent3_100)
            putSystemColor(palette, "lightTertiaryContainer", context,
                android.R.color.system_accent3_100)
            putSystemColor(palette, "lightOnTertiaryContainer", context,
                android.R.color.system_accent3_900)
            putSystemColor(palette, "lightBackground", context,
                android.R.color.system_neutral1_50)
            putSystemColor(palette, "lightOnBackground", context,
                android.R.color.system_neutral1_900)
            putSystemColor(palette, "lightSurface", context,
                android.R.color.system_neutral1_50)
            putSystemColor(palette, "lightOnSurface", context,
                android.R.color.system_neutral1_900)
            putSystemColor(palette, "lightSurfaceVariant", context,
                android.R.color.system_neutral2_200)
            putSystemColor(palette, "lightOnSurfaceVariant", context,
                android.R.color.system_neutral2_700)
            putSystemColor(palette, "lightSurfaceContainer", context,
                android.R.color.system_neutral1_100)
            putSystemColor(palette, "lightSurfaceContainerHigh", context,
                android.R.color.system_neutral1_200)
            putSystemColor(palette, "lightOutline", context,
                android.R.color.system_neutral2_500)
            putSystemColor(palette, "lightOutlineVariant", context,
                android.R.color.system_neutral2_200)

            // Dark palette
            putSystemColor(palette, "darkPrimary", context,
                android.R.color.system_accent1_200)
            putSystemColor(palette, "darkOnPrimary", context,
                android.R.color.system_accent1_800)
            putSystemColor(palette, "darkPrimaryContainer", context,
                android.R.color.system_accent1_700)
            putSystemColor(palette, "darkOnPrimaryContainer", context,
                android.R.color.system_accent1_100)
            putSystemColor(palette, "darkSecondary", context,
                android.R.color.system_accent2_200)
            putSystemColor(palette, "darkOnSecondary", context,
                android.R.color.system_accent2_800)
            putSystemColor(palette, "darkSecondaryContainer", context,
                android.R.color.system_accent2_700)
            putSystemColor(palette, "darkOnSecondaryContainer", context,
                android.R.color.system_accent2_100)
            putSystemColor(palette, "darkTertiary", context,
                android.R.color.system_accent3_200)
            putSystemColor(palette, "darkOnTertiary", context,
                android.R.color.system_accent3_800)
            putSystemColor(palette, "darkTertiaryContainer", context,
                android.R.color.system_accent3_700)
            putSystemColor(palette, "darkOnTertiaryContainer", context,
                android.R.color.system_accent3_100)
            putSystemColor(palette, "darkBackground", context,
                android.R.color.system_neutral1_900)
            putSystemColor(palette, "darkOnBackground", context,
                android.R.color.system_neutral1_100)
            putSystemColor(palette, "darkSurface", context,
                android.R.color.system_neutral1_900)
            putSystemColor(palette, "darkOnSurface", context,
                android.R.color.system_neutral1_100)
            putSystemColor(palette, "darkSurfaceVariant", context,
                android.R.color.system_neutral2_700)
            putSystemColor(palette, "darkOnSurfaceVariant", context,
                android.R.color.system_neutral2_200)
            putSystemColor(palette, "darkSurfaceContainer", context,
                android.R.color.system_neutral1_800)
            putSystemColor(palette, "darkSurfaceContainerHigh", context,
                android.R.color.system_neutral1_700)
            putSystemColor(palette, "darkOutline", context,
                android.R.color.system_neutral2_500)
            putSystemColor(palette, "darkOutlineVariant", context,
                android.R.color.system_neutral2_700)

            palette.toString()
        } catch (e: Exception) {
            Log.w(TAG, "Failed to extract theme palette", e)
            null
        }
    }

    private fun putSystemColor(json: JSONObject, key: String, context: Context, colorResId: Int) {
        try {
            val colorInt = context.resources.getColor(colorResId, context.theme)
            val hexColor = String.format("#%06X", 0xFFFFFF and colorInt)
            json.put(key, hexColor)
        } catch (e: Exception) {
            // Color resource may not exist on all devices
            json.put(key, "")
        }
    }
}
