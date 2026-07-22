package com.xiwei.sujian.ui

import android.os.Bundle
import androidx.appcompat.app.AppCompatActivity
import androidx.appcompat.app.AppCompatDelegate
import androidx.activity.compose.setContent
import com.xiwei.sujian.data.BridgeProvider
import com.xiwei.sujian.data.SettingsRepository
import com.xiwei.sujian.model.LocalSettings
import com.xiwei.sujian.ui.compose.SujianApp

class MainActivity : AppCompatActivity() {

    val textEditorCoordinator by lazy {
        com.xiwei.sujian.editor.v2.coordinator.AnimatedTextEditorCoordinator(this, BridgeProvider.getAppServiceBridge(this))
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        ErrorUtil.safeRun(this) {
            val settingsRepository = SettingsRepository(this)
            val settings = ErrorUtil.safeRun(this, LocalSettings()) {
                settingsRepository.getLocalSettings()
            }
            when (settings.appearanceMode) {
                "light" -> AppCompatDelegate.setDefaultNightMode(AppCompatDelegate.MODE_NIGHT_NO)
                "dark" -> AppCompatDelegate.setDefaultNightMode(AppCompatDelegate.MODE_NIGHT_YES)
                else -> AppCompatDelegate.setDefaultNightMode(AppCompatDelegate.MODE_NIGHT_FOLLOW_SYSTEM)
            }
        }

        setContent {
            SujianApp()
        }
    }
}
