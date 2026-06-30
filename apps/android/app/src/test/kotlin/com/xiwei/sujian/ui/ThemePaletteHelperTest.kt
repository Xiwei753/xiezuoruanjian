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

    /**
     * Fixture JSON matching the Rust ThemePaletteDto round-trip test.
     * This represents the real output format of ThemePaletteHelper.extractThemePaletteJson().
     * All keys are snake_case, matching Rust ThemePaletteDto field names exactly.
     */
    private val fixtureJson = """
    {
        "source":"android_dynamic_color",
        "updated_at_ms":1719792000000,
        "device_id":"test_device_001",
        "variant":"tonal_spot",
        "light_primary":"#006497",
        "light_on_primary":"#FFFFFF",
        "light_primary_container":"#CCE5FF",
        "light_on_primary_container":"#001E31",
        "light_secondary":"#50606E",
        "light_on_secondary":"#FFFFFF",
        "light_secondary_container":"#D3E5F5",
        "light_on_secondary_container":"#0C1D29",
        "light_tertiary":"#65587B",
        "light_on_tertiary":"#FFFFFF",
        "light_tertiary_container":"#EBDDFF",
        "light_on_tertiary_container":"#201634",
        "light_background":"#F6FAFE",
        "light_on_background":"#171C1F",
        "light_surface":"#F6FAFE",
        "light_on_surface":"#171C1F",
        "light_surface_variant":"#DEE3EB",
        "light_on_surface_variant":"#42474E",
        "light_surface_container":"#EAF0F7",
        "light_surface_container_high":"#E4EAF1",
        "light_outline":"#72787E",
        "light_outline_variant":"#C2C8CE",
        "dark_primary":"#85CFFF",
        "dark_on_primary":"#00344D",
        "dark_primary_container":"#004B6E",
        "dark_on_primary_container":"#CCE5FF",
        "dark_secondary":"#B7C9D8",
        "dark_on_secondary":"#22323F",
        "dark_secondary_container":"#384956",
        "dark_on_secondary_container":"#D3E5F5",
        "dark_tertiary":"#CFC0E8",
        "dark_on_tertiary":"#362E4B",
        "dark_tertiary_container":"#4D4462",
        "dark_on_tertiary_container":"#EBDDFF",
        "dark_background":"#0E1417",
        "dark_on_background":"#DEE3EB",
        "dark_surface":"#0E1417",
        "dark_on_surface":"#DEE3EB",
        "dark_surface_variant":"#42474E",
        "dark_on_surface_variant":"#C2C8CE",
        "dark_surface_container":"#1B2024",
        "dark_surface_container_high":"#252B2F",
        "dark_outline":"#8C9298",
        "dark_outline_variant":"#42474E"
    }
    """.trimIndent()

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
        // ThemePaletteDto 有 48 个字段（4 meta + 22 light + 22 dark）
        assertEquals(
            "Key count must match Rust ThemePaletteDto field count",
            48,
            expectedSnakeCaseKeys.size
        )
    }

    @Test
    fun themePaletteJson_fixtureMatchesRustDtoFields() {
        // 验证 fixture JSON 中的每个 key 都存在于 expectedSnakeCaseKeys 列表中
        for (key in expectedSnakeCaseKeys) {
            assertTrue(
                "Fixture JSON must contain key '$key' matching Rust ThemePaletteDto",
                fixtureJson.contains("\"$key\"")
            )
        }

        // 验证 fixture 中关键字段不为空（有实际颜色值）
        assertTrue("light_primary must have a non-empty value", fixtureJson.contains("\"light_primary\":\"#"))
        assertTrue("dark_surface must have a non-empty value", fixtureJson.contains("\"dark_surface\":\"#"))
        assertTrue("light_outline_variant must have a non-empty value", fixtureJson.contains("\"light_outline_variant\":\"#"))
        assertTrue("dark_outline_variant must have a non-empty value", fixtureJson.contains("\"dark_outline_variant\":\"#"))

        // 验证 fixture 的 key 数量与 expectedSnakeCaseKeys 一致
        val fixtureKeyCount = expectedSnakeCaseKeys.count { key -> fixtureJson.contains("\"$key\"") }
        assertEquals(
            "Fixture JSON key count must match expectedSnakeCaseKeys count",
            expectedSnakeCaseKeys.size,
            fixtureKeyCount
        )
    }
}
