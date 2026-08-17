package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.xiwei.sujian.R
import com.xiwei.sujian.core.designsystem.component.SujianSlider
import com.xiwei.sujian.core.designsystem.component.SujianSwitchRow

/**
 * #630 评论13 项2: 扁平 LazyColumn — 向父 [LazyListScope] 注册行，
 * 每个 [SettingsFieldGroup] 是独立 item，有稳定 key。
 */
fun LazyListScope.saveSettingsItems(vm: SettingsViewModel) {
    item(key = "save.auto_group") {
        val state by vm.saveState.collectAsStateWithLifecycle()
        SettingsGroupItemContainer(isLast = true, isFirst = true) {
            var autoSaveDelay by rememberSaveable(state.autoSaveDelayMs / 1000f) {
                mutableFloatStateOf(state.autoSaveDelayMs / 1000f)
            }

            SettingsFieldGroup(title = stringResource(id = R.string.pref_category_save)) {
                SujianSwitchRow(
                    title = stringResource(id = R.string.pref_auto_save),
                    checked = state.autoSaveEnabled,
                    onCheckedChange = { checked ->
                        vm.handleIntent(SettingsIntent.UpdateLocal { it.copy(autoSaveEnabled = checked) })
                    },
                )
                Spacer(modifier = Modifier.height(8.dp))
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
