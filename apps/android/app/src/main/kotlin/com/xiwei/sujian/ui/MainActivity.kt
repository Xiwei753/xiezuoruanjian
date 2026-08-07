package com.xiwei.sujian.ui

import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.provider.Settings
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.ActivityResultLauncher
import androidx.activity.result.contract.ActivityResultContracts
import com.xiwei.sujian.platform.AndroidDataRoot
import com.xiwei.sujian.ui.compose.SujianApp

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
        AndroidDataRoot.ensureDirectories()
        val initialDestination = intent?.getStringExtra("navigateTo")
        setContent {
            SujianApp(initialDestination = initialDestination)
        }
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
