package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.xiwei.sujian.R

/**
 * #630 R14 字段组模式：将原来的 1 个 item 改为字段组模式。
 *
 * 关于分组: 应用名称 + 作者 + 许可 + 数据根路径 + 版本信息
 *
 * 使用 [SettingsExpandedGroupContainer] 统一 16dp content padding、12dp 圆角。
 * 字段在同一个 High Surface 内普通布局。
 */
fun LazyListScope.aboutSettingsItems(
    vm: SettingsViewModel,
    closeOuterGroup: Boolean,
) {
    item(
        key = "about.info_group",
        contentType = CONTENT_TYPE_EXPANDED_FIELD_GROUP,
    ) {
        val state by vm.aboutState.collectAsStateWithLifecycle()
        SettingsExpandedGroupContainer(
            closeOuterGroup = closeOuterGroup,
            firstInGroup = true,
            lastInGroup = true,
        ) {
            androidx.compose.foundation.layout.Column(
                modifier = Modifier.height(IntrinsicSize.Min),
            ) {
                Spacer(modifier = Modifier.height(12.dp))
                Text(
                    text = stringResource(id = R.string.about_app_name),
                    style = MaterialTheme.typography.headlineSmall,
                    modifier = Modifier.height(32.dp),
                )
                Spacer(modifier = Modifier.height(4.dp))
                Text(
                    text = stringResource(id = R.string.about_author),
                    style = MaterialTheme.typography.bodyMedium,
                )
                Spacer(modifier = Modifier.height(4.dp))
                Text(
                    text = stringResource(id = R.string.about_license),
                    style = MaterialTheme.typography.bodyMedium,
                )
                Spacer(modifier = Modifier.height(4.dp))
                Text(
                    text = stringResource(id = R.string.pref_data_root_path, state.dataRootPath),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                if (state.versionInfo.isNotEmpty()) {
                    Spacer(modifier = Modifier.height(4.dp))
                    Text(
                        text = state.versionInfo,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Spacer(modifier = Modifier.height(12.dp))
            }
        }
    }
}
