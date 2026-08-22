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
 * #632 评论 5377052579：保存设置 — 每个重控件一个 Lazy item。
 *
 * 自动保存分组: 标题 + 开关 + 延迟
 *
 * 用 [SettingsExpandedFieldContainer] + [ExpandedFieldPosition] 让同一字段组的
 * 多个 item 视觉上连成一张大卡。每个 item 只 collect 自己需要的 row-level StateFlow。
 */
fun LazyListScope.saveSettingsItems(
    vm: SettingsViewModel,
    closeOuterGroup: Boolean,
) {
    // ── 自动保存分组（标题 + 开关 + 延迟）— 每个 item 独立 ──

    item(key = "save.auto_save.title", contentType = CONTENT_TYPE_FIELD_TITLE) {
        SettingsExpandedFieldContainer(
            position = ExpandedFieldPosition.First,
            closeOuterGroup = false,
        ) {
            SettingsFieldGroupTitle(title = stringResource(id = R.string.pref_category_save))
        }
    }

    item(key = "save.auto_save.switch", contentType = CONTENT_TYPE_SWITCH) {
        val checked by vm.autoSaveRow.collectAsStateWithLifecycle()
        SettingsExpandedFieldContainer(
            position = ExpandedFieldPosition.Middle,
            closeOuterGroup = false,
        ) {
            SujianSwitchRow(
                title = stringResource(id = R.string.pref_auto_save),
                checked = checked,
                onCheckedChange = { c ->
                    vm.handleIntent(SettingsIntent.UpdateLocal { it.copy(autoSaveEnabled = c) })
                },
            )
        }
    }

    item(key = "save.auto_save.delay", contentType = CONTENT_TYPE_SLIDER) {
        val delayMs by vm.autoSaveDelayRow.collectAsStateWithLifecycle()
        var autoSaveDelay by rememberSaveable(delayMs / 1000f) {
            mutableFloatStateOf(delayMs / 1000f)
        }
        SettingsExpandedFieldContainer(
            position = ExpandedFieldPosition.Last,
            closeOuterGroup = closeOuterGroup,
        ) {
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
