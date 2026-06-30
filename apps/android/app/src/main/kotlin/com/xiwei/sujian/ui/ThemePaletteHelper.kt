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
            palette.put("updated_at_ms", System.currentTimeMillis())
            palette.put("device_id", "")  // Will be filled by settings layer
            palette.put("variant", "tonal_spot")

            // Accent colors from system resources
            // Android 12+ provides system_accent1, system_accent2, system_accent3
            // and system_neutral1, system_neutral2 in 10 shades (0-999)
            // We use key shades that map to Material3 semantic tokens

            // Light palette
            putSystemColor(palette, "light_primary", context,
                android.R.color.system_accent1_500)
            putSystemColor(palette, "light_on_primary", context,
                android.R.color.system_accent1_100)
            putSystemColor(palette, "light_primary_container", context,
                android.R.color.system_accent1_100)
            putSystemColor(palette, "light_on_primary_container", context,
                android.R.color.system_accent1_900)
            putSystemColor(palette, "light_secondary", context,
                android.R.color.system_accent2_500)
            putSystemColor(palette, "light_on_secondary", context,
                android.R.color.system_accent2_100)
            putSystemColor(palette, "light_secondary_container", context,
                android.R.color.system_accent2_100)
            putSystemColor(palette, "light_on_secondary_container", context,
                android.R.color.system_accent2_900)
            putSystemColor(palette, "light_tertiary", context,
                android.R.color.system_accent3_500)
            putSystemColor(palette, "light_on_tertiary", context,
                android.R.color.system_accent3_100)
            putSystemColor(palette, "light_tertiary_container", context,
                android.R.color.system_accent3_100)
            putSystemColor(palette, "light_on_tertiary_container", context,
                android.R.color.system_accent3_900)
            putSystemColor(palette, "light_background", context,
                android.R.color.system_neutral1_50)
            putSystemColor(palette, "light_on_background", context,
                android.R.color.system_neutral1_900)
            putSystemColor(palette, "light_surface", context,
                android.R.color.system_neutral1_50)
            putSystemColor(palette, "light_on_surface", context,
                android.R.color.system_neutral1_900)
            putSystemColor(palette, "light_surface_variant", context,
                android.R.color.system_neutral2_200)
            putSystemColor(palette, "light_on_surface_variant", context,
                android.R.color.system_neutral2_700)
            putSystemColor(palette, "light_surface_container", context,
                android.R.color.system_neutral1_100)
            putSystemColor(palette, "light_surface_container_high", context,
                android.R.color.system_neutral1_200)
            putSystemColor(palette, "light_outline", context,
                android.R.color.system_neutral2_500)
            putSystemColor(palette, "light_outline_variant", context,
                android.R.color.system_neutral2_200)

            // Dark palette
            putSystemColor(palette, "dark_primary", context,
                android.R.color.system_accent1_200)
            putSystemColor(palette, "dark_on_primary", context,
                android.R.color.system_accent1_800)
            putSystemColor(palette, "dark_primary_container", context,
                android.R.color.system_accent1_700)
            putSystemColor(palette, "dark_on_primary_container", context,
                android.R.color.system_accent1_100)
            putSystemColor(palette, "dark_secondary", context,
                android.R.color.system_accent2_200)
            putSystemColor(palette, "dark_on_secondary", context,
                android.R.color.system_accent2_800)
            putSystemColor(palette, "dark_secondary_container", context,
                android.R.color.system_accent2_700)
            putSystemColor(palette, "dark_on_secondary_container", context,
                android.R.color.system_accent2_100)
            putSystemColor(palette, "dark_tertiary", context,
                android.R.color.system_accent3_200)
            putSystemColor(palette, "dark_on_tertiary", context,
                android.R.color.system_accent3_800)
            putSystemColor(palette, "dark_tertiary_container", context,
                android.R.color.system_accent3_700)
            putSystemColor(palette, "dark_on_tertiary_container", context,
                android.R.color.system_accent3_100)
            putSystemColor(palette, "dark_background", context,
                android.R.color.system_neutral1_900)
            putSystemColor(palette, "dark_on_background", context,
                android.R.color.system_neutral1_100)
            putSystemColor(palette, "dark_surface", context,
                android.R.color.system_neutral1_900)
            putSystemColor(palette, "dark_on_surface", context,
                android.R.color.system_neutral1_100)
            putSystemColor(palette, "dark_surface_variant", context,
                android.R.color.system_neutral2_700)
            putSystemColor(palette, "dark_on_surface_variant", context,
                android.R.color.system_neutral2_200)
            putSystemColor(palette, "dark_surface_container", context,
                android.R.color.system_neutral1_800)
            putSystemColor(palette, "dark_surface_container_high", context,
                android.R.color.system_neutral1_700)
            putSystemColor(palette, "dark_outline", context,
                android.R.color.system_neutral2_500)
            putSystemColor(palette, "dark_outline_variant", context,
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
