package com.xiwei.sujian.feature.settings.ui

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import com.xiwei.sujian.R
import com.xiwei.sujian.app.LocalWorkspaceAppState
import com.xiwei.sujian.app.RecoveryFromLocalButton

/**
 * #649 评论 5559763924：从本地恢复设置分类。
 *
 * 用户点击"选择源目录"后系统弹出 `OpenDocumentTree` 选择器，选中旧版共享存储
 *（`/Sujian/`、`/素笺/`）或 Download 镜像目录后，URI 交给
 * [com.xiwei.sujian.storage.recovery.StorageRecoveryCoordinator.recoverFromUri]，
 * 由协调器判断来源并完成迁移/恢复。不在 Composable 里做目录遍历/复制/解析。
 *
 * 恢复结果通过 Toast 提示；恢复成功后 [com.xiwei.sujian.storage.recovery.RecoveryChangeSink]
 * 负责刷新作品列表/星图/统计缓存。
 */
@Composable
fun RecoverySettingsContent() {
    val appState = LocalWorkspaceAppState.current
    SettingsInnerCard {
        SettingsFieldGroupTitle(title = stringResource(id = R.string.pref_category_recovery))
        RecoveryFromLocalButton(
            appState = appState,
            buttonTextResId = R.string.recovery_action_pick_source,
            modifier = Modifier.fillMaxWidth(),
        )
    }
}
