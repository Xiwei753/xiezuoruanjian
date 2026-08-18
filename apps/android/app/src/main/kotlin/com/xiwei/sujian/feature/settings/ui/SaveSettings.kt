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
 * #630 评论13/评论15/评论5324547885项2: 行级 LazyColumn — 每个真实设置控件是独立 item，有稳定 key。
 * 使用 [SettingsExpandedRowContainer] 替代旧的 [SettingsGroupItemContainer] +
 * [SettingsFieldRowContainer] 嵌套；展开内容在外层 Low 内缩 High 表面里连续拼接。
 * 每个 item 只 collect 自己的 row-level StateFlow，避免整分类重组。
 * 展开字段使用 [SettingsExpandedItemContent] 统一 fadeIn100/fadeOut70/placement120。
 */
fun LazyListScope.saveSettingsItems(
    vm: SettingsViewModel,
    closeOuterGroup: Boolean,
) {
    // 自动保存开关
    item(key = "save.auto_save", contentType = CONTENT_TYPE_EXPANDED_FIELD) {
        val checked by vm.autoSaveRow.collectAsStateWithLifecycle()
        SettingsExpandedItemContent {
            SettingsExpandedRowContainer(
                closeOuterGroup = false,
                firstInCategory = true,
                lastInCategory = false,
                firstInGroup = true,
                lastInGroup = false,
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
    }

    // 自动保存延迟
    item(key = "save.auto_save_delay", contentType = CONTENT_TYPE_EXPANDED_FIELD) {
        val delayMs by vm.autoSaveDelayRow.collectAsStateWithLifecycle()
        var autoSaveDelay by rememberSaveable(delayMs / 1000f) {
            mutableFloatStateOf(delayMs / 1000f)
        }
        SettingsExpandedItemContent {
            SettingsExpandedRowContainer(
                closeOuterGroup = closeOuterGroup,
                firstInCategory = false,
                lastInCategory = true,
                firstInGroup = false,
                lastInGroup = true,
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
}
