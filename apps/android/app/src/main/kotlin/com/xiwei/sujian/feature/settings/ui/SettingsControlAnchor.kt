package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.relocation.BringIntoViewRequester
import androidx.compose.foundation.relocation.bringIntoViewRequester
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.composed
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.unit.dp

/**
 * Android 设置页实际存在的控件键。
 *
 * 只登记真正有可见控件的 Core 键；已从 Android 设置页移除的键不伪造锚点，
 * 由 [SettingsRoute] 给出提示。
 */
internal object SettingsControlKeys {
    const val AppearanceMode = "appearanceMode"
    const val ColorSource = "colorSource"
    const val FontSize = "fontSize"
    const val LineSpacing = "editorLineSpacingMultiplier"
    const val AutoIndentEnabled = "autoIndentEnabled"
    const val AutoIndentWidth = "autoIndentWidth"
    const val TypingAnimationEnabled = "editorTypingAnimationEnabled"
    const val TypingAnimationDurationMs = "editorTypingAnimationDurationMs"
    const val CoordinatedAnimationEnabled = "editorCoordinatedTextCursorAnimationEnabled"
    const val AutoSaveEnabled = "autoSaveEnabled"
    const val AutoSaveDelayMs = "autoSaveDelayMs"
    const val AiEnabled = "aiEnabled"
    const val DiagnosticsEnabled = "diagnosticsEnabled"
    const val DiagnosticsVerbose = "diagnosticsVerbose"
}

private val settingsControlKeyByCoreKey: Map<String, String> =
    mapOf(
        "appearanceMode" to SettingsControlKeys.AppearanceMode,
        "themeMode" to SettingsControlKeys.AppearanceMode,
        "colorSource" to SettingsControlKeys.ColorSource,
        "dynamicColorEnabled" to SettingsControlKeys.ColorSource,
        "monetColor" to SettingsControlKeys.ColorSource,
        "themePalette" to SettingsControlKeys.ColorSource,
        "editorFontSize" to SettingsControlKeys.FontSize,
        "fontSize" to SettingsControlKeys.FontSize,
        "editorLineSpacingMultiplier" to SettingsControlKeys.LineSpacing,
        "autoIndentEnabled" to SettingsControlKeys.AutoIndentEnabled,
        "autoIndentWidth" to SettingsControlKeys.AutoIndentWidth,
        "editorTypingAnimationEnabled" to SettingsControlKeys.TypingAnimationEnabled,
        "editorTypingAnimationDurationMs" to SettingsControlKeys.TypingAnimationDurationMs,
        "editorCoordinatedTextCursorAnimationEnabled" to SettingsControlKeys.CoordinatedAnimationEnabled,
        "autoSaveEnabled" to SettingsControlKeys.AutoSaveEnabled,
        "autoSaveDelayMs" to SettingsControlKeys.AutoSaveDelayMs,
        "aiEnabled" to SettingsControlKeys.AiEnabled,
        "diagnosticsEnabled" to SettingsControlKeys.DiagnosticsEnabled,
        "diagnosticsVerbose" to SettingsControlKeys.DiagnosticsVerbose,
    )

/**
 * Core 搜索索引里的设置键 → Android 设置页控件键。
 *
 * 同一控件承载多个 Core 键时（例如旧版 themeMode 与 appearanceMode）映射到同一行；
 * 返回 null 表示当前 Android 设置页没有对应控件，调用方必须提示而不是只展开分类。
 */
internal fun settingsControlKeyFor(coreKey: String?): String? = coreKey?.let(settingsControlKeyByCoreKey::get)

internal data class SettingsControlAnchor(
    val bounds: Rect,
    val requester: BringIntoViewRequester,
)

/** 已注册控件锚点 — 设置页唯一持有，内容控件通过 [settingsControlAnchor] 登记。 */
@Stable
internal class SettingsControlRegistry {
    private val anchors = mutableStateMapOf<String, SettingsControlAnchor>()
    var highlightedKey by mutableStateOf<String?>(null)
        private set

    fun register(
        key: String,
        bounds: Rect,
        requester: BringIntoViewRequester,
    ) {
        anchors[key] = SettingsControlAnchor(bounds, requester)
    }

    fun unregister(
        key: String,
        requester: BringIntoViewRequester,
    ) {
        if (anchors[key]?.requester === requester) anchors.remove(key)
    }

    fun anchor(key: String): SettingsControlAnchor? = anchors[key]

    fun highlight(key: String) {
        highlightedKey = key
    }

    fun clearHighlight(key: String) {
        if (highlightedKey == key) highlightedKey = null
    }
}

internal val LocalSettingsControlRegistry =
    compositionLocalOf<SettingsControlRegistry?> { null }

/**
 * 给设置控件登记可定位边界，并在搜索结果定位后短暂高亮。
 *
 * 控件本身仍由现有设置组件绘制；这里只加透明布局监听与绘制层，
 * 不复制设置值或交互状态。
 */
internal fun Modifier.settingsControlAnchor(key: String): Modifier =
    composed {
        val registry = LocalSettingsControlRegistry.current
        if (registry == null) {
            this
        } else {
            val requester = remember { BringIntoViewRequester() }
            val highlighted = registry.highlightedKey == key
            val highlightFill = MaterialTheme.colorScheme.primaryContainer.copy(alpha = 0.32f)
            val highlightBorder = MaterialTheme.colorScheme.primary
            DisposableEffect(registry, key, requester) {
                onDispose { registry.unregister(key, requester) }
            }
            this
                .bringIntoViewRequester(requester)
                .onGloballyPositioned { coordinates ->
                    registry.register(key, coordinates.boundsInRoot(), requester)
                }
                .drawWithContent {
                    drawContent()
                    if (highlighted) {
                        val radius = CornerRadius(12.dp.toPx())
                        drawRoundRect(color = highlightFill, cornerRadius = radius)
                        drawRoundRect(
                            color = highlightBorder,
                            cornerRadius = radius,
                            style = Stroke(width = 2.dp.toPx()),
                        )
                    }
                }
        }
    }
