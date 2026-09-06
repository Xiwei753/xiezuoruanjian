package com.xiwei.sujian.app

import android.os.Bundle
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.R
import com.xiwei.sujian.app.theme.SujianTheme
import com.xiwei.sujian.core.diagnostics.JankStatsController
import com.xiwei.sujian.core.designsystem.component.SujianOutlinedButton
import com.xiwei.sujian.core.platform.storage.AndroidPrivateDataRoot
import com.xiwei.sujian.core.platform.storage.documents.DocumentTreeReader
import com.xiwei.sujian.storage.recovery.ImportResult
import com.xiwei.sujian.storage.recovery.LegacySharedStorageImporter
import com.xiwei.sujian.storage.recovery.LegacyStorageMigrationGate
import kotlinx.coroutines.launch

/**
 * MainActivity — 应用唯一 Activity。
 *
 * #649 评论 5559763924 / 5560685734 要求 3：数据根目录已改为应用私有
 * `filesDir/sujian`，不再需要共享外部存储权限。`onCreate` 启动顺序：
 * 1. 用 [LegacyStorageMigrationGate] 检测 `filesDir/sujian-git/workspace` 是否存在。
 * 2. 若存在（旧版本升级且未迁移）：**不**调用 [AndroidPrivateDataRoot.ensure]，
 *    只显示迁移入口 Composable（`OpenDocumentTree` + [LegacySharedStorageImporter]）。
 *    迁移成功后才调用 `ensure` 并进入 [SujianApp]。
 * 3. 若不存在（新安装或已迁移）：正常调用 `ensure` 并进入 [SujianApp]。
 *
 * 顺序门控的关键：迁移前不能创建 `filesDir/sujian/`，否则
 * [LegacySharedStorageImporter.renameToFinal] 会因目标目录已存在而失败。
 *
 * 旧版共享存储/Download 镜像的常规恢复入口（设置页/作品页空状态）仍由
 * `rememberLauncherForActivityResult(ActivityResultContracts.OpenDocumentTree())`
 * 触发，URI 交给 [com.xiwei.sujian.storage.recovery.StorageRecoveryCoordinator]，
 * 不在 Activity 里做目录遍历/复制/解析。
 */
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        if (LegacyStorageMigrationGate.legacyGitWorkspaceExists(this)) {
            // 旧结构待迁移：不创建私有根，只显示迁移入口。
            setContent {
                SujianTheme {
                    LegacyStorageMigrationScreen(
                        onMigrationSucceeded = { proceedWithUi() },
                    )
                }
            }
        } else {
            // 新安装或已迁移：正常进入 UI。
            proceedWithUi()
        }
    }

    private fun proceedWithUi() {
        // #649 评论 5559763924：应用私有 filesDir 无需运行时权限，直接确保目录存在。
        // 仅在不需要迁移或迁移成功后才调用，避免迁移前创建目标目录导致 renameToFinal 失败。
        AndroidPrivateDataRoot.ensure(this)
        val initialDestination = intent?.getStringExtra("navigateTo")
        setContent {
            SujianApp(initialDestination = initialDestination)
        }
        // Issue #612 四：JankStats 跟踪当前窗口帧；onResume 开启，onPause 关闭。
        JankStatsController.track(window)
    }

    override fun onResume() {
        super.onResume()
        JankStatsController.enable()
    }

    override fun onPause() {
        super.onPause()
        JankStatsController.disable()
    }
}

/**
 * 旧版本升级迁移入口 Composable。
 *
 * 居中显示标题、说明与"从旧版本迁移数据"按钮。点击后弹出 `OpenDocumentTree`
 * 选择旧 `Sujian` 子目录，选中 URI 后调用 [LegacySharedStorageImporter.import]，
 * 迁移在 IO 协程执行。成功后调用 [onMigrationSucceeded] 进入正常 UI；
 * 失败/取消通过 Toast 提示，用户可重试。
 *
 * 不依赖 [com.xiwei.sujian.core.interop.app.AppServiceBridge]——迁移完成前 Core 未初始化。
 */
@Composable
private fun LegacyStorageMigrationScreen(onMigrationSucceeded: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    // 迁移进行中标记，避免用户重复点击。
    var migrating by remember { mutableStateOf(false) }

    val titleText = stringResource(id = R.string.migration_gate_title)
    val messageText = stringResource(id = R.string.migration_gate_message)
    val actionText = stringResource(id = R.string.migration_gate_action)
    val successText = stringResource(id = R.string.migration_gate_success)
    val failedText = stringResource(id = R.string.migration_gate_failed)
    val cancelledText = stringResource(id = R.string.migration_gate_cancelled)

    val importer = remember { LegacySharedStorageImporter() }
    val documentTreeReader = remember {
        DocumentTreeReader(context.applicationContext.contentResolver)
    }

    val launcher =
        rememberLauncherForActivityResult(ActivityResultContracts.OpenDocumentTree()) { uri ->
            if (uri == null) {
                migrating = false
                Toast.makeText(context, cancelledText, Toast.LENGTH_SHORT).show()
                return@rememberLauncherForActivityResult
            }
            try {
                context.contentResolver.takePersistableUriPermission(
                    uri,
                    android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION,
                )
            } catch (_: SecurityException) {
                // 持久化授权失败不阻断本次迁移：单次读取权限已由 launcher 授予。
            }
            scope.launch {
                val result =
                    importer.import(
                        context.applicationContext,
                        uri,
                        documentTreeReader,
                    )
                when (result) {
                    is ImportResult.Success -> {
                        Toast.makeText(context, successText, Toast.LENGTH_LONG).show()
                        onMigrationSucceeded()
                    }
                    is ImportResult.NoLegacyGitFound -> {
                        migrating = false
                        Toast.makeText(context, cancelledText, Toast.LENGTH_SHORT).show()
                    }
                    is ImportResult.CopyFailed -> {
                        migrating = false
                        Toast.makeText(
                            context,
                            String.format(failedText, result.reason),
                            Toast.LENGTH_LONG,
                        ).show()
                    }
                }
            }
        }

    Column(
        modifier = Modifier.fillMaxSize().padding(24.dp),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(text = titleText)
        Spacer(modifier = Modifier.height(12.dp))
        Text(text = messageText)
        Spacer(modifier = Modifier.height(24.dp))
        SujianOutlinedButton(
            text = actionText,
            onClick = {
                if (migrating) return@SujianOutlinedButton
                migrating = true
                launcher.launch(null)
            },
            loading = migrating,
            enabled = !migrating,
        )
    }
}
