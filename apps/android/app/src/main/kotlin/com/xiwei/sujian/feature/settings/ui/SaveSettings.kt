package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.xiwei.sujian.R
import com.xiwei.sujian.core.designsystem.component.SujianSlider
import com.xiwei.sujian.core.designsystem.component.SujianSwitchRow

/**
 * #630 R14 字段组模式：将原来的 2 个独立 item 合并为 1 个字段组 item。
 *
 * 自动保存分组: 开关 + 延迟
 *
 * 使用 [SettingsExpandedGroupContainer] 统一 16dp content padding、12dp 圆角。
 * 每个字段组一个 item，组内多个字段普通布局。
 * 每个 item 只 collect 自己需要的 row-level StateFlow，避免整分类重组。
 */
fun LazyListScope.saveSettingsItems(
    vm: SettingsViewModel,
    closeOuterGroup: Boolean,
) {
    // ── 自动保存分组（开关 + 延迟）— 一个字段组 item ──
    item(
        key = "save.auto_save_group",
        contentType = CONTENT_TYPE_EXPANDED_FIELD_GROUP,
    ) {
        val checked by vm.autoSaveRow.collectAsStateWithLifecycle()
        val delayMs by vm.autoSaveDelayRow.collectAsStateWithLifecycle()
        var autoSaveDelay by rememberSaveable(delayMs / 1000f) {
            mutableFloatStateOf(delayMs / 1000f)
        }
        SettingsExpandedGroupContainer(
            closeOuterGroup = closeOuterGroup,
            firstInGroup = true,
            lastInGroup = true,
        ) {
            SettingsFieldGroupTitle(title = stringResource(id = R.string.pref_category_save))
            SujianSwitchRow(
                title = stringResource(id = R.string.pref_auto_save),
                checked = checked,
                onCheckedChange = { c ->
                    vm.handleIntent(SettingsIntent.UpdateLocal { it.copy(autoSaveEnabled = c) })
                },
            )
            SujianSlider(
                title = stringResource(id = R.string.pref_auto_save_delay),
                value = autoSaveDelay,
                onValueChange = { autoSaveDelay = it },
                onValueChangeFinished = {
                    vm.handleIntent(
                        SettingsIntent.UpdateLocal { it.copy(autoSaveDelayMs = (autoSaveDelay * 1000).toLong()) },
                    )
                },
                valueRange = 1f..10f,
                steps = 8,
                valueLabel =
                    pluralStringResource(
                        id = R.plurals.auto_save_delay_seconds,
                        autoSaveDelay.toInt(),
                        autoSaveDelay.toInt(),
                    ),
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}
