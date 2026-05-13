package com.xiwei.writerapp.ui

import android.os.Bundle
import android.widget.ArrayAdapter
import android.widget.SeekBar
import android.widget.Spinner
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity
import androidx.appcompat.app.AppCompatDelegate
import com.google.android.material.appbar.MaterialToolbar
import com.google.android.material.materialswitch.MaterialSwitch
import com.xiwei.writerapp.R
import com.xiwei.writerapp.data.SettingsRepository
import com.xiwei.writerapp.data.WorkspaceManager
import com.xiwei.writerapp.model.LocalSettings

class SettingsActivity : AppCompatActivity() {

    private lateinit var settingsRepository: SettingsRepository
    private lateinit var currentSettings: LocalSettings

    private lateinit var sbFontSize: SeekBar
    private lateinit var sbLineSpacing: SeekBar
    private lateinit var switchAutoSave: MaterialSwitch
    private lateinit var sbAutoSaveDelay: SeekBar
    private lateinit var spinnerTheme: Spinner
    private lateinit var tvWorkspacePath: TextView

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_settings)

        try {
            settingsRepository = SettingsRepository(this)
            currentSettings = settingsRepository.getLocalSettings()
        } catch (e: Throwable) {
            e.printStackTrace()
            currentSettings = LocalSettings()
            android.widget.Toast.makeText(this, "设置加载失败，使用默认值", android.widget.Toast.LENGTH_LONG).show()
        }

        val toolbar: MaterialToolbar = findViewById(R.id.toolbar)
        setSupportActionBar(toolbar)
        toolbar.setNavigationOnClickListener {
            saveAndFinish()
        }

        sbFontSize = findViewById(R.id.sbFontSize)
        sbLineSpacing = findViewById(R.id.sbLineSpacing)
        switchAutoSave = findViewById(R.id.switchAutoSave)
        sbAutoSaveDelay = findViewById(R.id.sbAutoSaveDelay)
        spinnerTheme = findViewById(R.id.spinnerTheme)
        tvWorkspacePath = findViewById(R.id.tvWorkspacePath)

        // Setup Theme Spinner
        val themeOptions = arrayOf(
            getString(R.string.theme_system),
            getString(R.string.theme_light),
            getString(R.string.theme_dark)
        )
        val adapter = ArrayAdapter(this, android.R.layout.simple_spinner_dropdown_item, themeOptions)
        spinnerTheme.adapter = adapter

        // Bind existing settings
        sbFontSize.progress = currentSettings.editorFontSize.toInt()
        sbLineSpacing.progress = (currentSettings.editorLineSpacingMultiplier * 10).toInt()
        switchAutoSave.isChecked = currentSettings.autoSaveEnabled
        sbAutoSaveDelay.progress = (currentSettings.autoSaveDelayMs / 1000).toInt()

        when (currentSettings.themeMode) {
            "light" -> spinnerTheme.setSelection(1)
            "dark" -> spinnerTheme.setSelection(2)
            else -> spinnerTheme.setSelection(0)
        }

        tvWorkspacePath.text = WorkspaceManager.getWorkspaceDir(this).absolutePath
    }

    override fun onBackPressed() {
        saveAndFinish()
        super.onBackPressed()
    }

    private fun saveAndFinish() {
        val themeStr = when (spinnerTheme.selectedItemPosition) {
            1 -> "light"
            2 -> "dark"
            else -> "system"
        }

        val newSettings = currentSettings.copy(
            editorFontSize = sbFontSize.progress.toFloat(),
            editorLineSpacingMultiplier = sbLineSpacing.progress / 10f,
            autoSaveEnabled = switchAutoSave.isChecked,
            autoSaveDelayMs = sbAutoSaveDelay.progress * 1000L,
            themeMode = themeStr
        )

        try {
            if (this::settingsRepository.isInitialized) {
                settingsRepository.saveLocalSettings(newSettings)
            }
        } catch (e: Throwable) {
            e.printStackTrace()
            android.widget.Toast.makeText(this, "设置保存失败", android.widget.Toast.LENGTH_LONG).show()
        }

        // Apply theme immediately if it changed
        if (currentSettings.themeMode != themeStr) {
            when (themeStr) {
                "light" -> AppCompatDelegate.setDefaultNightMode(AppCompatDelegate.MODE_NIGHT_NO)
                "dark" -> AppCompatDelegate.setDefaultNightMode(AppCompatDelegate.MODE_NIGHT_YES)
                else -> AppCompatDelegate.setDefaultNightMode(AppCompatDelegate.MODE_NIGHT_FOLLOW_SYSTEM)
            }
        }

        finish()
    }
}
