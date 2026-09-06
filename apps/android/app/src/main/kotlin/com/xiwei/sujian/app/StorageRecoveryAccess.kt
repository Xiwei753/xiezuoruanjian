package com.xiwei.sujian.app

import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.runtime.Composable
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import com.xiwei.sujian.R
import com.xiwei.sujian.app.di.LocalSujianAppDependencies
import com.xiwei.sujian.core.designsystem.component.SujianOutlinedButton
import com.xiwei.sujian.core.platform.storage.documents.DocumentTreeReader
import com.xiwei.sujian.storage.mirror.ReadableMirrorStateStore
import com.xiwei.sujian.storage.recovery.DefaultRecoveryChangeSink
import com.xiwei.sujian.storage.recovery.RecoveryResult
import com.xiwei.sujian.storage.recovery.StorageRecoveryCoordinator
import kotlinx.coroutines.launch

/**
 * #649 评论 5559763924：进程级 [WorkspaceAppState] 的 CompositionLocal。
 *
 * [SujianApp] 在根 CompositionLocalProvider 里提供，设置页等无 appState 形参的
 * Composable 通过 [LocalWorkspaceAppState.current] 取到，用于构造恢复协调器。
 * 作品页已通过形参接收 [WorkspaceAppState]，可直接使用，不必读此 Local。
 */
val LocalWorkspaceAppState =
    compositionLocalOf<WorkspaceAppState> {
        error("No WorkspaceAppState provided. Wrap with CompositionLocalProvider.")
    }

/**
 * #649 评论 5559763924：在 Composable 层组装 [StorageRecoveryCoordinator]。
 *
 * [StorageRecoveryCoordinator] 依赖 [WorkspaceAppState]（Compose ViewModel 状态），
 * 不放进纯 DI 容器 [com.xiwei.sujian.app.di.AppServiceContainer]（后者不应依赖 Compose）。
 * 本 helper 在 Composable 层把 DI 提供的 Bridge/Repository 与 appState 粘合，
 * 用 [remember] 缓存，避免每次重组重建 Coordinator。
 *
 * 恢复入口（设置页"从本地恢复"、作品页空状态）用此 helper 取 Coordinator，
 * 配合 `rememberLauncherForActivityResult(ActivityResultContracts.OpenDocumentTree())`
 * 把用户选中的 URI 交给 [StorageRecoveryCoordinator.recoverFromUri]。
 */
@Composable
fun rememberStorageRecoveryCoordinator(appState: WorkspaceAppState): StorageRecoveryCoordinator {
    val context = LocalContext.current
    val deps = LocalSujianAppDependencies.current
    return remember(appState, deps) {
        val documentTreeReader = DocumentTreeReader(context.applicationContext.contentResolver)
        val stateStore = ReadableMirrorStateStore(context.applicationContext)
        val changeSink =
            DefaultRecoveryChangeSink(
                appState = appState,
                starMapRepository = deps.starmapRepository,
                writingStatsRepository = deps.statsRepository,
            )
        StorageRecoveryCoordinator(
            context = context.applicationContext,
            documentTreeReader = documentTreeReader,
            appServiceBridge = deps.appServiceBridge,
            stateStore = stateStore,
            changeSink = changeSink,
        )
    }
}

/**
 * #649 评论 5559763924：从本地恢复按钮 — 共享 Composable，设置页与作品页空状态复用。
 *
 * 点击后弹出 `OpenDocumentTree` 选择器，选中源目录后 URI 交给
 * [StorageRecoveryCoordinator.recoverFromUri]，结果通过 Toast 提示。
 * 不在按钮里做目录遍历/复制/解析。
 *
 * @param buttonTextResId 按钮文本资源 id（设置页用"选择源目录"，作品页用"从本地恢复"）。
 */
@Composable
fun RecoveryFromLocalButton(
    appState: WorkspaceAppState,
    buttonTextResId: Int,
    modifier: Modifier = Modifier,
) {
    val coordinator = rememberStorageRecoveryCoordinator(appState)
    val context = LocalContext.current
    val scope = rememberCoroutineScope()

    val legacyImportedText = stringResource(id = R.string.recovery_result_legacy_imported)
    val mirrorRestoredText = stringResource(id = R.string.recovery_result_mirror_restored)
    val nothingText = stringResource(id = R.string.recovery_result_nothing)
    val failedText = stringResource(id = R.string.recovery_result_failed)
    val cancelledText = stringResource(id = R.string.recovery_result_cancelled)

    val launcher =
        rememberLauncherForActivityResult(ActivityResultContracts.OpenDocumentTree()) { uri ->
            if (uri == null) {
                Toast.makeText(context, cancelledText, Toast.LENGTH_SHORT).show()
                return@rememberLauncherForActivityResult
            }
            try {
                context.contentResolver.takePersistableUriPermission(
                    uri,
                    android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION or
                        android.content.Intent.FLAG_GRANT_WRITE_URI_PERMISSION,
                )
            } catch (_: SecurityException) {
                // 持久化授权失败不阻断本次恢复：单次读取权限已由 launcher 授予。
            }
            scope.launch {
                val result = coordinator.recoverFromUri(uri)
                val message =
                    when (result) {
                        RecoveryResult.LegacyImported -> legacyImportedText
                        RecoveryResult.MirrorRestored -> mirrorRestoredText
                        RecoveryResult.NothingToRecover -> nothingText
                        is RecoveryResult.Failed -> String.format(failedText, result.reason)
                    }
                Toast.makeText(context, message, Toast.LENGTH_LONG).show()
            }
        }

    SujianOutlinedButton(
        text = stringResource(id = buttonTextResId),
        onClick = { launcher.launch(null) },
        modifier = modifier.fillMaxWidth(),
    )
}
