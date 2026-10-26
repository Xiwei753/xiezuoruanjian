package com.xiwei.sujian.app.theme.interop

import com.xiwei.sujian.app.theme.model.ThemeColorScheme
import com.xiwei.sujian.app.theme.model.ThemePaletteRecord
import org.json.JSONObject
import java.security.MessageDigest
import java.util.Locale
import kotlin.math.pow

/**
 * Android 本地调色板快照 JSON 编解码（Issue #1005）。
 *
 * 设备配色只留在当前 Android 设备，不再通过 Core 调色板目录跨端读写。
 * 新存储沿用旧 Core catalog 的 camelCase 结构，升级时可无转换导入旧快照；
 * 指纹算法也与旧 Core 保持一致，旧 `selectedPaletteId` 仍能定位到迁移后的记录。
 */
internal object ThemePaletteRecordJson {
    fun encode(record: ThemePaletteRecord): String =
        JSONObject()
            .put("schemaVersion", record.schemaVersion.toInt())
            .put("paletteId", record.paletteId)
            .put("paletteFingerprint", record.paletteFingerprint)
            .put("source", record.source)
            .put("sourcePlatform", record.sourcePlatform)
            .put("sourceDeviceId", record.sourceDeviceId)
            .put("sourceDeviceClass", record.sourceDeviceClass)
            .put("capturedAtMs", record.capturedAtMs)
            .put("variant", record.variant)
            .put("lightScheme", schemeToJson(record.lightScheme))
            .put("darkScheme", schemeToJson(record.darkScheme))
            .toString()

    fun decode(json: String): ThemePaletteRecord? =
        try {
            val root = JSONObject(json)
            val light = root.optJSONObject("lightScheme") ?: return null
            val dark = root.optJSONObject("darkScheme") ?: return null
            ThemePaletteRecord(
                schemaVersion = root.optInt("schemaVersion", 0).toUInt(),
                paletteId = root.optString("paletteId"),
                paletteFingerprint = root.optString("paletteFingerprint"),
                source = root.optString("source"),
                sourcePlatform = root.optString("sourcePlatform"),
                sourceDeviceId = root.optString("sourceDeviceId"),
                sourceDeviceClass = root.optString("sourceDeviceClass"),
                capturedAtMs = root.optLong("capturedAtMs", 0L),
                variant = root.optString("variant"),
                lightScheme = schemeFromJson(light),
                darkScheme = schemeFromJson(dark),
            )
        } catch (_: Exception) {
            null
        }

    /** 与旧 Core 的 SHA-256(lightJson + darkJson) 取前 8 字节一致。 */
    fun fingerprint(
        light: ThemeColorScheme,
        dark: ThemeColorScheme,
    ): String {
        val digest = MessageDigest.getInstance("SHA-256")
        digest.update(canonicalSchemeJson(light).toByteArray(Charsets.UTF_8))
        digest.update(canonicalSchemeJson(dark).toByteArray(Charsets.UTF_8))
        val hash = digest.digest()
        return buildString(16) {
            for (index in 0 until 8) {
                append(String.format(Locale.ROOT, "%02x", hash[index]))
            }
        }
    }

    fun isRecordValid(record: ThemePaletteRecord): Boolean =
        record.schemaVersion > 0u &&
            record.paletteId.isNotEmpty() &&
            record.paletteFingerprint.isNotEmpty() &&
            isSchemeComplete(record.lightScheme) &&
            isSchemeComplete(record.darkScheme)

    private fun schemeToJson(scheme: ThemeColorScheme): JSONObject =
        JSONObject()
            .put("primary", scheme.primary)
            .put("onPrimary", scheme.onPrimary)
            .put("primaryContainer", scheme.primaryContainer)
            .put("onPrimaryContainer", scheme.onPrimaryContainer)
            .put("inversePrimary", scheme.inversePrimary)
            .put("secondary", scheme.secondary)
            .put("onSecondary", scheme.onSecondary)
            .put("secondaryContainer", scheme.secondaryContainer)
            .put("onSecondaryContainer", scheme.onSecondaryContainer)
            .put("tertiary", scheme.tertiary)
            .put("onTertiary", scheme.onTertiary)
            .put("tertiaryContainer", scheme.tertiaryContainer)
            .put("onTertiaryContainer", scheme.onTertiaryContainer)
            .put("background", scheme.background)
            .put("onBackground", scheme.onBackground)
            .put("surface", scheme.surface)
            .put("onSurface", scheme.onSurface)
            .put("surfaceVariant", scheme.surfaceVariant)
            .put("onSurfaceVariant", scheme.onSurfaceVariant)
            .put("surfaceTint", scheme.surfaceTint)
            .put("surfaceDim", scheme.surfaceDim)
            .put("surfaceBright", scheme.surfaceBright)
            .put("surfaceContainerLowest", scheme.surfaceContainerLowest)
            .put("surfaceContainerLow", scheme.surfaceContainerLow)
            .put("surfaceContainer", scheme.surfaceContainer)
            .put("surfaceContainerHigh", scheme.surfaceContainerHigh)
            .put("surfaceContainerHighest", scheme.surfaceContainerHighest)
            .put("inverseSurface", scheme.inverseSurface)
            .put("inverseOnSurface", scheme.inverseOnSurface)
            .put("error", scheme.error)
            .put("onError", scheme.onError)
            .put("errorContainer", scheme.errorContainer)
            .put("onErrorContainer", scheme.onErrorContainer)
            .put("outline", scheme.outline)
            .put("outlineVariant", scheme.outlineVariant)
            .put("scrim", scheme.scrim)
            .put("primaryFixed", scheme.primaryFixed)
            .put("primaryFixedDim", scheme.primaryFixedDim)
            .put("onPrimaryFixed", scheme.onPrimaryFixed)
            .put("onPrimaryFixedVariant", scheme.onPrimaryFixedVariant)
            .put("secondaryFixed", scheme.secondaryFixed)
            .put("secondaryFixedDim", scheme.secondaryFixedDim)
            .put("onSecondaryFixed", scheme.onSecondaryFixed)
            .put("onSecondaryFixedVariant", scheme.onSecondaryFixedVariant)
            .put("tertiaryFixed", scheme.tertiaryFixed)
            .put("tertiaryFixedDim", scheme.tertiaryFixedDim)
            .put("onTertiaryFixed", scheme.onTertiaryFixed)
            .put("onTertiaryFixedVariant", scheme.onTertiaryFixedVariant)

    private fun schemeFromJson(value: JSONObject): ThemeColorScheme =
        ThemeColorScheme(
            primary = value.optString("primary"),
            onPrimary = value.optString("onPrimary"),
            primaryContainer = value.optString("primaryContainer"),
            onPrimaryContainer = value.optString("onPrimaryContainer"),
            inversePrimary = value.optString("inversePrimary"),
            secondary = value.optString("secondary"),
            onSecondary = value.optString("onSecondary"),
            secondaryContainer = value.optString("secondaryContainer"),
            onSecondaryContainer = value.optString("onSecondaryContainer"),
            tertiary = value.optString("tertiary"),
            onTertiary = value.optString("onTertiary"),
            tertiaryContainer = value.optString("tertiaryContainer"),
            onTertiaryContainer = value.optString("onTertiaryContainer"),
            background = value.optString("background"),
            onBackground = value.optString("onBackground"),
            surface = value.optString("surface"),
            onSurface = value.optString("onSurface"),
            surfaceVariant = value.optString("surfaceVariant"),
            onSurfaceVariant = value.optString("onSurfaceVariant"),
            surfaceTint = value.optString("surfaceTint"),
            surfaceDim = value.optString("surfaceDim"),
            surfaceBright = value.optString("surfaceBright"),
            surfaceContainerLowest = value.optString("surfaceContainerLowest"),
            surfaceContainerLow = value.optString("surfaceContainerLow"),
            surfaceContainer = value.optString("surfaceContainer"),
            surfaceContainerHigh = value.optString("surfaceContainerHigh"),
            surfaceContainerHighest = value.optString("surfaceContainerHighest"),
            inverseSurface = value.optString("inverseSurface"),
            inverseOnSurface = value.optString("inverseOnSurface"),
            error = value.optString("error"),
            onError = value.optString("onError"),
            errorContainer = value.optString("errorContainer"),
            onErrorContainer = value.optString("onErrorContainer"),
            outline = value.optString("outline"),
            outlineVariant = value.optString("outlineVariant"),
            scrim = value.optString("scrim"),
            primaryFixed = value.optString("primaryFixed"),
            primaryFixedDim = value.optString("primaryFixedDim"),
            onPrimaryFixed = value.optString("onPrimaryFixed"),
            onPrimaryFixedVariant = value.optString("onPrimaryFixedVariant"),
            secondaryFixed = value.optString("secondaryFixed"),
            secondaryFixedDim = value.optString("secondaryFixedDim"),
            onSecondaryFixed = value.optString("onSecondaryFixed"),
            onSecondaryFixedVariant = value.optString("onSecondaryFixedVariant"),
            tertiaryFixed = value.optString("tertiaryFixed"),
            tertiaryFixedDim = value.optString("tertiaryFixedDim"),
            onTertiaryFixed = value.optString("onTertiaryFixed"),
            onTertiaryFixedVariant = value.optString("onTertiaryFixedVariant"),
        )

    private fun isSchemeComplete(scheme: ThemeColorScheme): Boolean =
        isHex(scheme.primary) &&
            isHex(scheme.onPrimary) &&
            isHex(scheme.background) &&
            isHex(scheme.onBackground) &&
            isHex(scheme.surface) &&
            isHex(scheme.onSurface) &&
            isHex(scheme.surfaceVariant) &&
            isHex(scheme.onSurfaceVariant) &&
            isReadablePair(scheme.onSurface, scheme.surface) &&
            isReadablePair(scheme.onBackground, scheme.background) &&
            isReadablePair(scheme.onSurfaceVariant, scheme.surface)

    private fun isHex(color: String): Boolean {
        val hex = color.removePrefix("#")
        return (hex.length == 6 || hex.length == 8) && hex.all { it.isDigit() || it.lowercaseChar() in 'a'..'f' }
    }

    /**
     * 与旧 Core 调色板校验保持一致的 WCAG 对比度检查。
     * `#AARRGGBB` 与旧实现一样只取后 6 位 RGB。
     */
    private fun isReadablePair(
        foreground: String,
        background: String,
    ): Boolean {
        val foregroundLuminance = relativeLuminance(foreground) ?: return false
        val backgroundLuminance = relativeLuminance(background) ?: return false
        val lighter = maxOf(foregroundLuminance, backgroundLuminance)
        val darker = minOf(foregroundLuminance, backgroundLuminance)
        return (lighter + 0.05) / (darker + 0.05) >= MIN_READABLE_CONTRAST_RATIO
    }

    private fun relativeLuminance(color: String): Double? {
        val hex = color.removePrefix("#")
        val rgb =
            when (hex.length) {
                6 -> hex
                8 -> hex.substring(2)
                else -> return null
            }
        val red = rgb.substring(0, 2).toIntOrNull(16) ?: return null
        val green = rgb.substring(2, 4).toIntOrNull(16) ?: return null
        val blue = rgb.substring(4, 6).toIntOrNull(16) ?: return null
        return 0.2126 * linearizeChannel(red) +
            0.7152 * linearizeChannel(green) +
            0.0722 * linearizeChannel(blue)
    }

    private fun linearizeChannel(channel: Int): Double {
        val normalized = channel / 255.0
        return if (normalized <= 0.04045) {
            normalized / 12.92
        } else {
            ((normalized + 0.055) / 1.055).pow(2.4)
        }
    }

    private const val MIN_READABLE_CONTRAST_RATIO = 3.0

    /**
     * 手写字段顺序，保证与 Rust serde 生成的紧凑 camelCase JSON 一致；
     * `JSONObject` 不保证 key 顺序，不能用于指纹计算。
     */
    private fun canonicalSchemeJson(scheme: ThemeColorScheme): String =
        buildString {
            append('{')
            appendField("primary", scheme.primary, first = true)
            appendField("onPrimary", scheme.onPrimary)
            appendField("primaryContainer", scheme.primaryContainer)
            appendField("onPrimaryContainer", scheme.onPrimaryContainer)
            appendField("inversePrimary", scheme.inversePrimary)
            appendField("secondary", scheme.secondary)
            appendField("onSecondary", scheme.onSecondary)
            appendField("secondaryContainer", scheme.secondaryContainer)
            appendField("onSecondaryContainer", scheme.onSecondaryContainer)
            appendField("tertiary", scheme.tertiary)
            appendField("onTertiary", scheme.onTertiary)
            appendField("tertiaryContainer", scheme.tertiaryContainer)
            appendField("onTertiaryContainer", scheme.onTertiaryContainer)
            appendField("background", scheme.background)
            appendField("onBackground", scheme.onBackground)
            appendField("surface", scheme.surface)
            appendField("onSurface", scheme.onSurface)
            appendField("surfaceVariant", scheme.surfaceVariant)
            appendField("onSurfaceVariant", scheme.onSurfaceVariant)
            appendField("surfaceTint", scheme.surfaceTint)
            appendField("surfaceDim", scheme.surfaceDim)
            appendField("surfaceBright", scheme.surfaceBright)
            appendField("surfaceContainerLowest", scheme.surfaceContainerLowest)
            appendField("surfaceContainerLow", scheme.surfaceContainerLow)
            appendField("surfaceContainer", scheme.surfaceContainer)
            appendField("surfaceContainerHigh", scheme.surfaceContainerHigh)
            appendField("surfaceContainerHighest", scheme.surfaceContainerHighest)
            appendField("inverseSurface", scheme.inverseSurface)
            appendField("inverseOnSurface", scheme.inverseOnSurface)
            appendField("error", scheme.error)
            appendField("onError", scheme.onError)
            appendField("errorContainer", scheme.errorContainer)
            appendField("onErrorContainer", scheme.onErrorContainer)
            appendField("outline", scheme.outline)
            appendField("outlineVariant", scheme.outlineVariant)
            appendField("scrim", scheme.scrim)
            appendField("primaryFixed", scheme.primaryFixed)
            appendField("primaryFixedDim", scheme.primaryFixedDim)
            appendField("onPrimaryFixed", scheme.onPrimaryFixed)
            appendField("onPrimaryFixedVariant", scheme.onPrimaryFixedVariant)
            appendField("secondaryFixed", scheme.secondaryFixed)
            appendField("secondaryFixedDim", scheme.secondaryFixedDim)
            appendField("onSecondaryFixed", scheme.onSecondaryFixed)
            appendField("onSecondaryFixedVariant", scheme.onSecondaryFixedVariant)
            appendField("tertiaryFixed", scheme.tertiaryFixed)
            appendField("tertiaryFixedDim", scheme.tertiaryFixedDim)
            appendField("onTertiaryFixed", scheme.onTertiaryFixed)
            appendField("onTertiaryFixedVariant", scheme.onTertiaryFixedVariant)
            append('}')
        }

    private fun StringBuilder.appendField(
        name: String,
        value: String,
        first: Boolean = false,
    ) {
        if (!first) append(',')
        append(JSONObject.quote(name))
        append(':')
        append(JSONObject.quote(value))
    }
}
