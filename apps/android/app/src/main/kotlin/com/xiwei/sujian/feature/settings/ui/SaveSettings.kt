package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.runtime.Composable
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
 * #633 评论 5379618506：保存设置 — 一个逻辑字段组 = 一张 High 内卡。
 *
 * 自动保存分组: 标题 + 开关 + 延迟（一张 SettingsInnerCard）
 */
@Composable
fun SaveSettingsContent(vm: SettingsViewModel) {
    val checked by vm.autoSaveRow.collectAsStateWithLifecycle()
    val delayMs by vm.autoSaveDelayRow.collectAsStateWithLifecycle()
    var autoSaveDelay by rememberSaveable(delayMs / 1000f) {
        mutableFloatStateOf(delayMs / 1000f)
    }

    SettingsInnerCard {
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
