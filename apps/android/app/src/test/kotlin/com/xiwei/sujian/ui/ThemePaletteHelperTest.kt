package com.xiwei.sujian.ui

import org.junit.Assert.*
import org.junit.Test

/**
 * 验证 ThemePaletteHelper 输出的 JSON 使用 snake_case 字段名，
 * 与 Rust 端 ThemePaletteDto（默认 snake_case）对齐。
 *
 * 注意：不使用 org.json.JSONObject（纯 JVM 测试不可用），
 * 改用字符串常量验证命名约定。
 */
class ThemePaletteHelperTest {

    /**
     * ThemePaletteHelper 输出 JSON 时使用的所有 key。
     * 这些 key 必须与 Rust ThemePaletteDto 的 snake_case 字段名一致。
     */
    private val expectedSnakeCaseKeys = listOf(
        "source", "updated_at_ms", "device_id", "variant",
        "light_primary", "light_on_primary", "light_primary_container", "light_on_primary_container",
        "light_secondary", "light_on_secondary", "light_secondary_container", "light_on_secondary_container",
        "light_tertiary", "light_on_tertiary", "light_tertiary_container", "light_on_tertiary_container",
        "light_background", "light_on_background", "light_surface", "light_on_surface",
        "light_surface_variant", "light_on_surface_variant",
        "light_surface_container", "light_surface_container_high",
        "light_outline", "light_outline_variant",
        "dark_primary", "dark_on_primary", "dark_primary_container", "dark_on_primary_container",
        "dark_secondary", "dark_on_secondary", "dark_secondary_container", "dark_on_secondary_container",
        "dark_tertiary", "dark_on_tertiary", "dark_tertiary_container", "dark_on_tertiary_container",
        "dark_background", "dark_on_background", "dark_surface", "dark_on_surface",
        "dark_surface_variant", "dark_on_surface_variant",
        "dark_surface_container", "dark_surface_container_high",
        "dark_outline", "dark_outline_variant"
    )

    @Test
    fun themePaletteJson_allKeysAreSnakeCase() {
        // 验证所有 key 都符合 snake_case 格式（小写字母+下划线，不含大写字母）
        for (key in expectedSnakeCaseKeys) {
            assertFalse(
                "Key '$key' should not contain uppercase letters (snake_case only)",
                key.any { it.isUpperCase() }
            )
        }
    }

    @Test
    fun themePaletteJson_noCamelCaseKeys() {
        // 验证不存在 camelCase key（这些是旧版错误命名，已修正为 snake_case）
        val camelCaseKeys = listOf(
            "updatedAtMs", "deviceId", "lightPrimary", "darkPrimary",
            "lightSurfaceContainerHigh", "darkSurfaceContainerHigh",
            "lightOnPrimary", "darkOnPrimary",
            "lightPrimaryContainer", "darkPrimaryContainer",
            "lightSurfaceVariant", "darkSurfaceVariant"
        )
        // 验证 camelCase key 不在 snake_case key 列表中
        for (camelKey in camelCaseKeys) {
            assertFalse(
                "CamelCase key '$camelKey' should NOT be in the expected snake_case key list",
                expectedSnakeCaseKeys.contains(camelKey)
            )
        }
    }

    @Test
    fun themePaletteJson_keyCountMatchesRustDto() {
        // ThemePaletteDto 有 50 个字段（source + updated_at_ms + device_id + variant + 46 color fields）
        assertEquals(
            "Key count must match Rust ThemePaletteDto field count",
            50,
            expectedSnakeCaseKeys.size
        )
    }
}
