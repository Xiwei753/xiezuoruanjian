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
 * #630 评论13/评论15: 行级 LazyColumn — 每个真实设置控件是独立 item，有稳定 key。
 * 使用 [SettingsFieldRowContainer] 的 isFirst/isLast 保持 M3 高色阶卡片视觉。
 * 每个 item 只 collect 自己的 row-level StateFlow，避免整分类重组。
 */
fun LazyListScope.saveSettingsItems(vm: SettingsViewModel) {
    // 自动保存开关
    item(key = "save.auto_save") {
        val checked by vm.autoSaveRow.collectAsStateWithLifecycle()
        SettingsGroupItemContainer(isLast = false) {
            SettingsFieldRowContainer(isFirst = true, isLast = false) {
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
    item(key = "save.auto_save_delay") {
        val delayMs by vm.autoSaveDelayRow.collectAsStateWithLifecycle()
        var autoSaveDelay by rememberSaveable(delayMs / 1000f) {
            mutableFloatStateOf(delayMs / 1000f)
        }
        SettingsGroupItemContainer(isLast = true) {
            SettingsFieldRowContainer(isFirst = false, isLast = true) {
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
