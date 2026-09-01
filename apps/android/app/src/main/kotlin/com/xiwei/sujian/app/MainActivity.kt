package com.xiwei.sujian.app

import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.provider.Settings
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.ActivityResultLauncher
import androidx.activity.result.contract.ActivityResultContracts
import com.xiwei.sujian.core.diagnostics.JankStatsController
import com.xiwei.sujian.core.platform.storage.AndroidDataRoot

class MainActivity : ComponentActivity() {
    private lateinit var storageAccessLauncher: ActivityResultLauncher<Intent>

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        enableEdgeToEdge()

        storageAccessLauncher =
            registerForActivityResult(ActivityResultContracts.StartActivityForResult()) {
                if (AndroidDataRoot.hasStorageAccess()) {
                    proceedWithUi()
                }
                // 未授权时不初始化 UI，等待用户再次进入设置授权。
            }

        if (AndroidDataRoot.hasStorageAccess()) {
            proceedWithUi()
        } else {
            requestStorageAccess()
        }
    }

    private fun proceedWithUi() {
        // #609 二：取得存储权限后、初始化 Core 前，先迁移旧版中文数据目录
        // （/素笺/ → /Sujian/），再建立新目录；迁移幂等且不覆盖新数据。
        AndroidDataRoot.migrateLegacyChineseDataRoot()
        AndroidDataRoot.ensureDirectories()
        // #644 评论 5490799656 问题1：创建 Git metadata 私有目录（filesDir/sujian-git/）。
        // 此目录位于应用私有存储，不需要共享存储权限。
        AndroidDataRoot.ensureGitMetadataDirectories(this)
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

    private fun requestStorageAccess() {
        val intent =
            Intent(
                Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION,
                Uri.parse("package:$packageName"),
            )
        storageAccessLauncher.launch(intent)
    }
}
