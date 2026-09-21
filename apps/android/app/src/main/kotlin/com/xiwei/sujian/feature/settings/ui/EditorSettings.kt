package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.xiwei.sujian.R
import com.xiwei.sujian.core.designsystem.component.SujianSlider
import com.xiwei.sujian.core.designsystem.component.SujianSwitchRow
import com.xiwei.sujian.core.designsystem.testing.SujianSemanticIds

/**
 * #633 评论 5379618506：编辑器设置 — 一个逻辑字段组 = 一张 High 内卡。
 *
 * Issue #723 评论 5748592923：
 * - 字号/行距从外观页移回写作区设置，在自动首行缩进卡之前加"字体与排版"卡，
 *   直接复用现有保存入口（vm.fontSizeRow + SettingsIntent.UpdateFontSize，
 *   vm.lineSpacingRow + UpdateLocal { copy(editorLineSpacingMultiplier = ...) }）。
 * - 增加"协同动画（吞字/吐字）"开关，绑定 editorCoordinatedTextCursorAnimationEnabled。
 *   协同关闭时显示独立控件（输入动效 + 输入时长）；
 *   协同开启时隐藏独立控件，只显示一个"协同动画时长"，
 *   直接写 editorTypingAnimationDurationMs（EditorMotionPolicy 在 coordinated=true 时
 *   本来就是用 textDurationMillis 驱动整条文字+光标 timeline）。
 *   从关闭切到开启时，同时把 editorTypingAnimationEnabled 归一为 true，
 *   避免旧 false 值暗中把协同链拆掉。
 *
 * 自动缩进分组: 开关 + 宽度（一张 SettingsInnerCard）
 * 编辑器行为分组: 标题 + 打字动画开关 + 打字动画时长（一张 SettingsInnerCard）
 *
 * 8dp 间距由 [SettingsExpandedShell] 的 spacedBy 统一产生。
 * 每张内卡内部各自 collect 自己需要的 row-level StateFlow。
 */
@Composable
fun EditorSettingsContent(vm: SettingsViewModel) {
    // Issue #723 评论 5748592923：字体与排版卡 — 字号/行距入口回到写作区设置。
    val currentFontSize by vm.fontSizeRow.collectAsStateWithLifecycle()
    var fontSize by rememberSaveable(currentFontSize) { mutableFloatStateOf(currentFontSize) }
    val spacing by vm.lineSpacingRow.collectAsStateWithLifecycle()
    var lineSpacing by rememberSaveable(spacing) { mutableFloatStateOf(spacing) }

    SettingsInnerCard {
        SettingsFieldGroupTitle(title = stringResource(id = R.string.pref_category_font_layout))
        SujianSlider(
            title = stringResource(id = R.string.pref_font_size),
            value = fontSize,
            onValueChange = { fontSize = it },
            onValueChangeFinished = { vm.handleIntent(SettingsIntent.UpdateFontSize(fontSize)) },
            valueRange = 12f..72f,
            steps = 59,
            valueLabel = "${fontSize.toInt()}sp",
            semanticId = SujianSemanticIds.SettingsFontSize,
            modifier = Modifier.fillMaxWidth(),
        )
        SujianSlider(
            title = stringResource(id = R.string.pref_line_spacing),
            value = lineSpacing,
            onValueChange = { lineSpacing = it },
            onValueChangeFinished = {
                vm.handleIntent(
                    SettingsIntent.UpdateLocal { it.copy(editorLineSpacingMultiplier = lineSpacing) },
                )
            },
            valueRange = 1f..3f,
            steps = 19,
            valueLabel = String.format(java.util.Locale.ROOT, "%.1fx", lineSpacing),
            modifier = Modifier.fillMaxWidth(),
        )
    }

    val autoIndent by vm.autoIndentRow.collectAsStateWithLifecycle()
    val autoIndentWidth by vm.autoIndentWidthRow.collectAsStateWithLifecycle()
    var autoIndentWidthState by rememberSaveable(autoIndentWidth) { mutableFloatStateOf(autoIndentWidth) }

    SettingsInnerCard {
        SujianSwitchRow(
            title = stringResource(id = R.string.pref_auto_indent),
            checked = autoIndent,
            onCheckedChange = { c ->
                vm.handleIntent(SettingsIntent.UpdateLocal { it.copy(autoIndentEnabled = c) })
            },
        )
        SujianSlider(
            title = stringResource(id = R.string.pref_auto_indent_width),
            value = autoIndentWidthState,
            onValueChange = { autoIndentWidthState = it },
            onValueChangeFinished = {
                vm.handleIntent(
                    SettingsIntent.UpdateLocal { it.copy(autoIndentWidth = autoIndentWidthState) },
                )
            },
            valueRange = 0f..8f,
            steps = 15,
            valueLabel = stringResource(id = R.string.auto_indent_width_chars, autoIndentWidthState.toInt()),
            modifier = Modifier.fillMaxWidth(),
        )
    }

    // Issue #723 评论 5748592923：协同动画（吞字/吐字）开关。
    val coordinatedAnimationChecked by vm.coordinatedAnimationRow.collectAsStateWithLifecycle()
    val typingAnimationChecked by vm.typingAnimationRow.collectAsStateWithLifecycle()
    val typingAnimationDuration by vm.typingAnimationDurationRow.collectAsStateWithLifecycle()
    var typingDuration by rememberSaveable(typingAnimationDuration.toFloat()) {
        mutableFloatStateOf(typingAnimationDuration.toFloat())
    }

    SettingsInnerCard {
        SettingsFieldGroupTitle(title = stringResource(id = R.string.pref_category_editor_behavior))
        SujianSwitchRow(
            title = stringResource(id = R.string.pref_editor_coordinated_animation),
            checked = coordinatedAnimationChecked,
            onCheckedChange = { c ->
                // Issue #732 评论 5763493968 第4节：协同本身就是完整模式，
                // 不能靠改另一个隐藏设置才能成立 — 只写 editorCoordinatedTextCursorAnimationEnabled。
                vm.handleIntent(
                    SettingsIntent.UpdateLocal {
                        it.copy(editorCoordinatedTextCursorAnimationEnabled = c)
                    },
                )
            },
        )
        if (coordinatedAnimationChecked) {
            // 协同开启：只显示一个"协同动画时长"，直接写 editorTypingAnimationDurationMs。
            // EditorMotionPolicy 在 coordinated=true 时本来就是用 textDurationMillis 驱动
            // 整条文字+光标 timeline，cursorDurationMillis 对 Insert/Delete/Replace/Composition 不生效。
            SujianSlider(
                title = stringResource(id = R.string.pref_editor_coordinated_animation_duration),
                value = typingDuration,
                onValueChange = { typingDuration = it },
                onValueChangeFinished = {
                    vm.handleIntent(
                        SettingsIntent.UpdateLocal {
                            it.copy(editorTypingAnimationDurationMs = typingDuration.toInt())
                        },
                    )
                },
                valueRange = 30f..1000f,
                steps = 96,
                valueLabel = "${typingDuration.toInt()}ms",
                modifier = Modifier.fillMaxWidth(),
            )
        } else {
            // 协同关闭：显示独立控件（输入动效 + 输入时长）。
            SujianSwitchRow(
                title = stringResource(id = R.string.pref_editor_typing_animation),
                checked = typingAnimationChecked,
                onCheckedChange = { c ->
                    vm.handleIntent(SettingsIntent.UpdateLocal { it.copy(editorTypingAnimationEnabled = c) })
                },
                semanticId = SujianSemanticIds.SettingsTypingAnimation,
            )
            SujianSlider(
                title = stringResource(id = R.string.pref_editor_typing_animation_duration),
                value = typingDuration,
                onValueChange = { typingDuration = it },
                onValueChangeFinished = {
                    vm.handleIntent(
                        SettingsIntent.UpdateLocal {
                            it.copy(editorTypingAnimationDurationMs = typingDuration.toInt())
                        },
                    )
                },
                valueRange = 30f..1000f,
                steps = 96,
                valueLabel = "${typingDuration.toInt()}ms",
                enabled = typingAnimationChecked,
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}
