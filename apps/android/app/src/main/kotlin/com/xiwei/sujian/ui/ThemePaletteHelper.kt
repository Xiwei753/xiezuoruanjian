package com.xiwei.sujian.ui

import android.content.Context
import android.os.Build
import android.util.Log
import com.google.android.material.R.attr as M3Attr
import com.google.android.material.color.MaterialColors

/**
 * Helper to extract Android 12+ Dynamic Color (Material You) palette
 * and format it as a cross-platform ThemePalette JSON object.
 *
 * Non-Android clients only consume this; they never produce it.
 *
 * ## Color resolution priority
 *
 * 1. **Primary route**: Read semantic attr (e.g. `R.attr.colorPrimary`) from the
 *    currently-applied DynamicColors theme via `MaterialColors.getColor()`.
 *    This yields the true Material3 token value, including any customisation
 *    applied by the DynamicColors engine.
 *
 * 2. **Fallback route**: When the semantic attr is unavailable (pre-Android 12,
 *    theme without DynamicColors, or API-33-only attrs on older devices),
 *    fall back to `system_accent1/2/3` / `system_neutral1/2` tone mapping.
 */
object ThemePaletteHelper {

    private const val TAG = "ThemePaletteHelper"

    // ------------------------------------------------------------------ //
    //  ColorResolver – injectable colour-resolution strategy for testing  //
    // ------------------------------------------------------------------ //

    /**
     * Abstraction over colour resolution so that unit tests can inject
     * deterministic mocks without needing an Android Context or MaterialColors.
     */
    interface ColorResolver {
        /**
         * Attempt to resolve a semantic theme-attr colour.
         *
         * @return the colour as a hex string (e.g. `"#006497"`), or `null`
         *         if the attr is not available in the current theme.
         */
        fun resolveThemeAttrColor(attrResId: Int): String?

        /**
         * Resolve a system-resource colour (fallback route).
         *
         * @return the colour as a hex string, or `null` if unavailable.
         */
        fun resolveSystemColor(colorResId: Int): String?
    }

    /**
     * Production implementation that reads colours from a real Android [Context].
     */
    class DefaultColorResolver(private val context: Context) : ColorResolver {

        override fun resolveThemeAttrColor(attrResId: Int): String? {
            // Semantic attrs require Android 12+ with DynamicColors applied.
            if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) return null
            return try {
                val colorInt = MaterialColors.getColor(context, attrResId, /* fallback= */ 0)
                // MaterialColors.getColor returns the fallback value (0) when the attr
                // is not resolved, which means the colour is fully transparent black.
                // Treat that as "not available".
                if (colorInt == 0) null
                else String.format("#%06X", 0xFFFFFF and colorInt)
            } catch (e: Exception) {
                Log.d(TAG, "Semantic attr 0x${attrResId.toString(16)} not available", e)
                null
            }
        }

        override fun resolveSystemColor(colorResId: Int): String? {
            return try {
                val colorInt = context.resources.getColor(colorResId, context.theme)
                String.format("#%06X", 0xFFFFFF and colorInt)
            } catch (e: Exception) {
                null
            }
        }
    }

    // ----------------------------------------------------------- //
    //  Semantic attr definitions (light + dark)                    //
    // ----------------------------------------------------------- //

    /**
     * A colour entry to be written into the JSON output.
     *
     * @param jsonKey   snake_case key in the output JSON.
     * @param attrResId Material3 semantic attr (e.g. `M3Attr.colorPrimary`).
     *                  `null` for attrs that only exist on API 33+.
     * @param attrResIdV33 Same attr but only available on API 33+.
     *                  If non-null, takes precedence over [attrResId] on API 33+.
     * @param systemColorResId  Fallback system-resource colour ID.
     */
    private data class ColorEntry(
        val jsonKey: String,
        val attrResId: Int?,
        val attrResIdV33: Int? = null,
        val systemColorResId: Int
    )

    /** Light-palette colour entries. */
    private val lightColorEntries = listOf(
        // Primary
        ColorEntry("light_primary",              M3Attr.colorPrimary,              systemColorResId = android.R.color.system_accent1_500),
        ColorEntry("light_on_primary",           M3Attr.colorOnPrimary,            systemColorResId = android.R.color.system_accent1_100),
        ColorEntry("light_primary_container",    M3Attr.colorPrimaryContainer,     systemColorResId = android.R.color.system_accent1_100),
        ColorEntry("light_on_primary_container", M3Attr.colorOnPrimaryContainer,   systemColorResId = android.R.color.system_accent1_900),
        // Secondary
        ColorEntry("light_secondary",              M3Attr.colorSecondary,              systemColorResId = android.R.color.system_accent2_500),
        ColorEntry("light_on_secondary",           M3Attr.colorOnSecondary,            systemColorResId = android.R.color.system_accent2_100),
        ColorEntry("light_secondary_container",    M3Attr.colorSecondaryContainer,     systemColorResId = android.R.color.system_accent2_100),
        ColorEntry("light_on_secondary_container", M3Attr.colorOnSecondaryContainer,   systemColorResId = android.R.color.system_accent2_900),
        // Tertiary
        ColorEntry("light_tertiary",              M3Attr.colorTertiary,              systemColorResId = android.R.color.system_accent3_500),
        ColorEntry("light_on_tertiary",           M3Attr.colorOnTertiary,            systemColorResId = android.R.color.system_accent3_100),
        ColorEntry("light_tertiary_container",    M3Attr.colorTertiaryContainer,     systemColorResId = android.R.color.system_accent3_100),
        ColorEntry("light_on_tertiary_container", M3Attr.colorOnTertiaryContainer,   systemColorResId = android.R.color.system_accent3_900),
        // Background / Surface
        // Note: colorBackground is in android.R.attr, not Material R.attr.
        // colorOnBackground is not available as a standard attr, so we use null
        // and fall back to system tone mapping.
        ColorEntry("light_background",           android.R.attr.colorBackground,    systemColorResId = android.R.color.system_neutral1_50),
        ColorEntry("light_on_background",        null,                              systemColorResId = android.R.color.system_neutral1_900),
        ColorEntry("light_surface",              M3Attr.colorSurface,              systemColorResId = android.R.color.system_neutral1_50),
        ColorEntry("light_on_surface",           M3Attr.colorOnSurface,            systemColorResId = android.R.color.system_neutral1_900),
        ColorEntry("light_surface_variant",      M3Attr.colorSurfaceVariant,       systemColorResId = android.R.color.system_neutral2_200),
        ColorEntry("light_on_surface_variant",   M3Attr.colorOnSurfaceVariant,     systemColorResId = android.R.color.system_neutral2_700),
        // SurfaceContainer (API 33+)
        ColorEntry("light_surface_container_lowest",  null, M3Attr.colorSurfaceContainerLowest,  systemColorResId = android.R.color.system_neutral1_50),
        ColorEntry("light_surface_container_low",     null, M3Attr.colorSurfaceContainerLow,     systemColorResId = android.R.color.system_neutral1_100),
        ColorEntry("light_surface_container",         null, M3Attr.colorSurfaceContainer,        systemColorResId = android.R.color.system_neutral1_100),
        ColorEntry("light_surface_container_high",    null, M3Attr.colorSurfaceContainerHigh,    systemColorResId = android.R.color.system_neutral1_200),
        ColorEntry("light_surface_container_highest", null, M3Attr.colorSurfaceContainerHighest, systemColorResId = android.R.color.system_neutral2_100),
        // Outline
        ColorEntry("light_outline",          M3Attr.colorOutline,          systemColorResId = android.R.color.system_neutral2_500),
        ColorEntry("light_outline_variant",  M3Attr.colorOutlineVariant,   systemColorResId = android.R.color.system_neutral2_200),
    )

    /** Dark-palette colour entries. */
    private val darkColorEntries = listOf(
        // Primary
        ColorEntry("dark_primary",              M3Attr.colorPrimary,              systemColorResId = android.R.color.system_accent1_200),
        ColorEntry("dark_on_primary",           M3Attr.colorOnPrimary,            systemColorResId = android.R.color.system_accent1_800),
        ColorEntry("dark_primary_container",    M3Attr.colorPrimaryContainer,     systemColorResId = android.R.color.system_accent1_700),
        ColorEntry("dark_on_primary_container", M3Attr.colorOnPrimaryContainer,   systemColorResId = android.R.color.system_accent1_100),
        // Secondary
        ColorEntry("dark_secondary",              M3Attr.colorSecondary,              systemColorResId = android.R.color.system_accent2_200),
        ColorEntry("dark_on_secondary",           M3Attr.colorOnSecondary,            systemColorResId = android.R.color.system_accent2_800),
        ColorEntry("dark_secondary_container",    M3Attr.colorSecondaryContainer,     systemColorResId = android.R.color.system_accent2_700),
        ColorEntry("dark_on_secondary_container", M3Attr.colorOnSecondaryContainer,   systemColorResId = android.R.color.system_accent2_100),
        // Tertiary
        ColorEntry("dark_tertiary",              M3Attr.colorTertiary,              systemColorResId = android.R.color.system_accent3_200),
        ColorEntry("dark_on_tertiary",           M3Attr.colorOnTertiary,            systemColorResId = android.R.color.system_accent3_800),
        ColorEntry("dark_tertiary_container",    M3Attr.colorTertiaryContainer,     systemColorResId = android.R.color.system_accent3_700),
        ColorEntry("dark_on_tertiary_container", M3Attr.colorOnTertiaryContainer,   systemColorResId = android.R.color.system_accent3_100),
        // Background / Surface
        ColorEntry("dark_background",           android.R.attr.colorBackground,    systemColorResId = android.R.color.system_neutral1_900),
        ColorEntry("dark_on_background",        null,                              systemColorResId = android.R.color.system_neutral1_100),
        ColorEntry("dark_surface",              M3Attr.colorSurface,              systemColorResId = android.R.color.system_neutral1_900),
        ColorEntry("dark_on_surface",           M3Attr.colorOnSurface,            systemColorResId = android.R.color.system_neutral1_100),
        ColorEntry("dark_surface_variant",      M3Attr.colorSurfaceVariant,       systemColorResId = android.R.color.system_neutral2_700),
        ColorEntry("dark_on_surface_variant",   M3Attr.colorOnSurfaceVariant,     systemColorResId = android.R.color.system_neutral2_200),
        // SurfaceContainer (API 33+)
        ColorEntry("dark_surface_container_lowest",  null, M3Attr.colorSurfaceContainerLowest,  systemColorResId = android.R.color.system_neutral1_900),
        ColorEntry("dark_surface_container_low",     null, M3Attr.colorSurfaceContainerLow,     systemColorResId = android.R.color.system_neutral1_800),
        ColorEntry("dark_surface_container",         null, M3Attr.colorSurfaceContainer,        systemColorResId = android.R.color.system_neutral1_800),
        ColorEntry("dark_surface_container_high",    null, M3Attr.colorSurfaceContainerHigh,    systemColorResId = android.R.color.system_neutral1_700),
        ColorEntry("dark_surface_container_highest", null, M3Attr.colorSurfaceContainerHighest, systemColorResId = android.R.color.system_neutral2_700),
        // Outline
        ColorEntry("dark_outline",          M3Attr.colorOutline,          systemColorResId = android.R.color.system_neutral2_500),
        ColorEntry("dark_outline_variant",  M3Attr.colorOutlineVariant,   systemColorResId = android.R.color.system_neutral2_700),
    )

    // ----------------------------------------------------------- //
    //  Public API                                                   //
    // ----------------------------------------------------------- //

    /**
     * Extract the full Material You palette.
     * Returns a JSON string compatible with Rust core's ThemePalette struct.
     * Returns null if not available (pre-Android 12 or no dynamic color).
     */
    fun extractThemePaletteJson(context: Context): String? {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) return null
        return extractThemePaletteJson(DefaultColorResolver(context))
    }

    /**
     * Testable overload that accepts a [ColorResolver] instead of a [Context].
     *
     * Uses [StringBuilder] instead of [JSONObject] so that pure-JVM unit tests
     * (which lack the Android stub `org.json`) can execute this path.
     */
    fun extractThemePaletteJson(resolver: ColorResolver): String? {
        return try {
            val sb = StringBuilder()
            sb.append("{\"source\":\"android_dynamic_color\",")
            sb.append("\"updated_at_ms\":${System.currentTimeMillis()},")
            sb.append("\"device_id\":\"\",")
            sb.append("\"variant\":\"tonal_spot\",")

            val allEntries = lightColorEntries + darkColorEntries
            for ((i, entry) in allEntries.withIndex()) {
                val value = resolveColorValue(entry, resolver)
                sb.append("\"${entry.jsonKey}\":\"$value\"")
                if (i < allEntries.size - 1) sb.append(",")
            }

            sb.append("}")
            sb.toString()
        } catch (e: Exception) {
            Log.w(TAG, "Failed to extract theme palette", e)
            null
        }
    }

    /**
     * Resolve the colour value for a single [ColorEntry] using the priority chain:
     * 1. API-33+ semantic attr → 2. Standard semantic attr → 3. System tone fallback → 4. ""
     */
    private fun resolveColorValue(entry: ColorEntry, resolver: ColorResolver): String {
        // 1. Try API-33+ semantic attr
        val v33Attr = entry.attrResIdV33
        if (v33Attr != null && Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            val hex = resolver.resolveThemeAttrColor(v33Attr)
            if (hex != null) return hex
        }

        // 2. Try standard semantic attr
        val attr = entry.attrResId
        if (attr != null) {
            val hex = resolver.resolveThemeAttrColor(attr)
            if (hex != null) return hex
        }

        // 3. Fallback to system tone
        val systemHex = resolver.resolveSystemColor(entry.systemColorResId)
        if (systemHex != null) return systemHex

        // 4. Nothing available
        return ""
    }
}
