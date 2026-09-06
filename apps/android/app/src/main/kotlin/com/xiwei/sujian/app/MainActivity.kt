package com.xiwei.sujian.app

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import com.xiwei.sujian.core.diagnostics.JankStatsController
import com.xiwei.sujian.core.platform.storage.AndroidDataRoot

/**
 * MainActivity — 应用唯一 Activity。
 *
 * #649 评论 5559763924：数据根目录已改为应用私有 `filesDir/Sujian`，不再需要
 * 共享外部存储权限。`onCreate` 直接初始化私有目录并进入 UI。
 *
 * 旧版共享存储/Download 镜像的恢复入口由设置页与作品页空状态通过
 * `rememberLauncherForActivityResult(ActivityResultContracts.OpenDocumentTree())`
 * 触发，URI 交给 [com.xiwei.sujian.storage.recovery.StorageRecoveryCoordinator]，
 * 不在 Activity 里做目录遍历/复制/解析。
 */
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        proceedWithUi()
    }

    private fun proceedWithUi() {
        // #649 评论 5559763924：应用私有 filesDir 无需运行时权限，直接确保目录存在。
        AndroidDataRoot.ensureDirectories(this)
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
