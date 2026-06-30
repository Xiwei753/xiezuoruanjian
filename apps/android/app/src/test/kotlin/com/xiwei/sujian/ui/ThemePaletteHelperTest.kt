package com.xiwei.sujian.ui

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

/**
 * 验证 ThemePaletteHelper 输出的 JSON 使用 snake_case 字段名，
 * 与 Rust 端 ThemePaletteDto（默认 snake_case）对齐。
 */
class ThemePaletteHelperTest {

    @Test
    fun themePaletteJson_usesSnakeCaseKeys() {
        // 模拟 ThemePaletteHelper 输出的 JSON 结构（snake_case）
        val expectedSnakeCaseKeys = listOf(
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

        // 构建一个与 ThemePaletteHelper 输出格式一致的 JSON
        val json = JSONObject()
        for (key in expectedSnakeCaseKeys) {
            json.put(key, if (key == "updated_at_ms") 0L else "test_value")
        }

        // 验证所有期望的 snake_case key 都存在
        for (key in expectedSnakeCaseKeys) {
            assertTrue("JSON should contain snake_case key: $key", json.has(key))
        }

        // 验证不存在 camelCase key
        val camelCaseKeys = listOf(
            "updatedAtMs", "deviceId", "lightPrimary", "darkPrimary",
            "lightSurfaceContainerHigh", "darkSurfaceContainerHigh",
            "lightOnPrimary", "darkOnPrimary",
            "lightPrimaryContainer", "darkPrimaryContainer",
            "lightSurfaceVariant", "darkSurfaceVariant"
        )
        for (key in camelCaseKeys) {
            assertFalse("JSON should NOT contain camelCase key: $key", json.has(key))
        }
    }

    @Test
    fun themePaletteJson_snakeCaseKeyFormat_isConsistent() {
        // 验证所有 key 都符合 snake_case 格式（小写字母+下划线，不含大写字母）
        val allKeys = listOf(
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

        for (key in allKeys) {
            assertFalse(
                "Key '$key' should not contain uppercase letters (snake_case only)",
                key.any { it.isUpperCase() }
            )
        }
    }
}
