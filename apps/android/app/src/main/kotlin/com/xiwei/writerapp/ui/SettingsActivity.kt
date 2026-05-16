package com.xiwei.writerapp.ui

import android.os.Bundle
import com.google.android.material.slider.Slider
import android.widget.ArrayAdapter

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
import android.widget.Toast
import androidx.appcompat.app.AlertDialog
import com.google.android.material.button.MaterialButton
import com.google.android.material.textfield.TextInputEditText
import com.xiwei.writerapp.model.SyncConfig
import com.xiwei.writerapp.model.SyncTransport
import com.xiwei.writerapp.model.SyncSecrets
import com.xiwei.writerapp.data.NativeResult
import com.xiwei.writerapp.model.FirstSyncMode


class SettingsActivity : AppCompatActivity() {

    private lateinit var settingsRepository: SettingsRepository
    private lateinit var currentSettings: LocalSettings

    private lateinit var sbFontSize: Slider
    private lateinit var sbLineSpacing: Slider
    private lateinit var switchAutoSave: MaterialSwitch
    private lateinit var sbAutoSaveDelay: Slider
    private lateinit var spinnerTheme: Spinner
    private lateinit var tvWorkspacePath: TextView

    // Sync Views
    private lateinit var switchEnableSync: MaterialSwitch
    private lateinit var etGithubRepo: TextInputEditText
    private lateinit var etBranch: TextInputEditText
    private lateinit var etHttpsToken: TextInputEditText
    private lateinit var tvTokenStatus: TextView
    private lateinit var switchAutoSync: MaterialSwitch
    private lateinit var sbSyncInterval: Slider

    private lateinit var tvFontSizeValue: TextView
    private lateinit var tvLineSpacingValue: TextView
    private lateinit var tvAutoSaveDelayValue: TextView
    private lateinit var tvSyncIntervalValue: TextView

    private lateinit var switchAutoIndent: MaterialSwitch
    private lateinit var sbAutoIndentWidth: Slider
    private lateinit var tvAutoIndentWidthValue: TextView

    private lateinit var switchTypingAnimation: MaterialSwitch
    private lateinit var switchSmoothCursor: MaterialSwitch

    private lateinit var btnDryRun: MaterialButton
    private lateinit var btnPerformSync: MaterialButton

    private lateinit var currentSyncConfig: SyncConfig
    private lateinit var currentSyncSecrets: SyncSecrets


    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_settings)

        ErrorUtil.safeRun(this) {
            settingsRepository = SettingsRepository(this)
        }
        currentSettings = ErrorUtil.safeRun(this, LocalSettings()) {
            if (::settingsRepository.isInitialized) {
                settingsRepository.getLocalSettings()
            } else {
                LocalSettings()
            }
        }

        loadSyncState()

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

        tvFontSizeValue = findViewById(R.id.tvFontSizeValue)
        tvLineSpacingValue = findViewById(R.id.tvLineSpacingValue)
        tvAutoSaveDelayValue = findViewById(R.id.tvAutoSaveDelayValue)
        tvSyncIntervalValue = findViewById(R.id.tvSyncIntervalValue)

        switchAutoIndent = findViewById(R.id.switchAutoIndent)
        sbAutoIndentWidth = findViewById(R.id.sbAutoIndentWidth)
        tvAutoIndentWidthValue = findViewById(R.id.tvAutoIndentWidthValue)
        switchTypingAnimation = findViewById(R.id.switchTypingAnimation)
        switchSmoothCursor = findViewById(R.id.switchSmoothCursor)

        tvWorkspacePath = findViewById(R.id.tvWorkspacePath)

        switchEnableSync = findViewById(R.id.switchEnableSync)
        etGithubRepo = findViewById(R.id.etGithubRepo)
        etBranch = findViewById(R.id.etBranch)
        etHttpsToken = findViewById(R.id.etHttpsToken)
        tvTokenStatus = findViewById(R.id.tvTokenStatus)
        switchAutoSync = findViewById(R.id.switchAutoSync)
        sbSyncInterval = findViewById(R.id.sbSyncInterval)
        btnDryRun = findViewById(R.id.btnDryRun)
        btnPerformSync = findViewById(R.id.btnPerformSync)


        // Live value update listeners
        sbFontSize.addOnChangeListener { _, value, _ ->
            tvFontSizeValue.text = "${value.toInt()}sp"
        }
        sbLineSpacing.addOnChangeListener { _, value, _ ->
            tvLineSpacingValue.text = "${String.format("%.1f", value)}x"
        }
        sbAutoSaveDelay.addOnChangeListener { _, value, _ ->
            tvAutoSaveDelayValue.text = "${value.toInt()}秒"
        }
        sbSyncInterval.addOnChangeListener { _, value, _ ->
            tvSyncIntervalValue.text = "${value.toInt()}秒"
        }
        sbAutoIndentWidth.addOnChangeListener { _, value, _ ->
            tvAutoIndentWidthValue.text = "${value}字符"
        }

        // Save on drag stop
        val saveSettingsListener = object : Slider.OnSliderTouchListener {
            override fun onStartTrackingTouch(slider: Slider) {}
            override fun onStopTrackingTouch(slider: Slider) {
                saveAndFinish(false)
            }
        }
        sbFontSize.addOnSliderTouchListener(saveSettingsListener)
        sbLineSpacing.addOnSliderTouchListener(saveSettingsListener)
        sbAutoSaveDelay.addOnSliderTouchListener(saveSettingsListener)
        sbSyncInterval.addOnSliderTouchListener(saveSettingsListener)
        sbAutoIndentWidth.addOnSliderTouchListener(saveSettingsListener)


        // Setup Theme Spinner
        val themeOptions = arrayOf(
            getString(R.string.theme_system),
            getString(R.string.theme_light),
            getString(R.string.theme_dark)
        )
        val adapter = ArrayAdapter(this, android.R.layout.simple_spinner_dropdown_item, themeOptions)
        spinnerTheme.adapter = adapter

        // Bind existing settings
        sbFontSize.value = currentSettings.editorFontSize
        sbLineSpacing.value = currentSettings.editorLineSpacingMultiplier
        switchAutoSave.isChecked = currentSettings.autoSaveEnabled
        sbAutoSaveDelay.value = (currentSettings.autoSaveDelayMs / 1000).toFloat()

        switchAutoIndent.isChecked = currentSettings.autoIndentEnabled
        sbAutoIndentWidth.value = currentSettings.autoIndentWidth

        switchTypingAnimation.isChecked = currentSettings.editorTypingAnimationEnabled
        switchSmoothCursor.isChecked = currentSettings.editorSmoothCursorEnabled

        // Initial texts
        tvFontSizeValue.text = "${currentSettings.editorFontSize.toInt()}sp"
        tvLineSpacingValue.text = "${String.format("%.1f", currentSettings.editorLineSpacingMultiplier)}x"
        tvAutoSaveDelayValue.text = "${(currentSettings.autoSaveDelayMs / 1000).toInt()}秒"
        tvAutoIndentWidthValue.text = "${currentSettings.autoIndentWidth}字符"

        switchAutoSave.setOnCheckedChangeListener { _, _ -> saveAndFinish(false) }
        switchAutoIndent.setOnCheckedChangeListener { _, _ -> saveAndFinish(false) }
        switchTypingAnimation.setOnCheckedChangeListener { _, _ -> saveAndFinish(false) }
        switchSmoothCursor.setOnCheckedChangeListener { _, _ -> saveAndFinish(false) }


        spinnerTheme.onItemSelectedListener = object : android.widget.AdapterView.OnItemSelectedListener {
            override fun onItemSelected(parent: android.widget.AdapterView<*>?, view: android.view.View?, position: Int, id: Long) {
                val themeStr = when (position) {
                    1 -> "light"
                    2 -> "dark"
                    else -> "system"
                }
                if (currentSettings.themeMode != themeStr) {
                    currentSettings = currentSettings.copy(themeMode = themeStr)
                    ErrorUtil.safeRun(this@SettingsActivity) {
                        if (::settingsRepository.isInitialized) {
                            settingsRepository.saveLocalSettings(currentSettings)
                        }
                    }
                    when (themeStr) {
                        "light" -> AppCompatDelegate.setDefaultNightMode(AppCompatDelegate.MODE_NIGHT_NO)
                        "dark" -> AppCompatDelegate.setDefaultNightMode(AppCompatDelegate.MODE_NIGHT_YES)
                        else -> AppCompatDelegate.setDefaultNightMode(AppCompatDelegate.MODE_NIGHT_FOLLOW_SYSTEM)
                    }
                }
            }

            override fun onNothingSelected(parent: android.widget.AdapterView<*>?) {}
        }

        when (currentSettings.themeMode) {
            "light" -> spinnerTheme.setSelection(1, false)
            "dark" -> spinnerTheme.setSelection(2, false)
            else -> spinnerTheme.setSelection(0, false)
        }

        tvWorkspacePath.text = WorkspaceManager.getWorkspaceDir(this).absolutePath

        btnDryRun.setOnClickListener {
            handleDryRun()
        }

        btnPerformSync.setOnClickListener {
            handlePerformSync()
        }


        // Bind Sync Settings
        switchEnableSync.isChecked = currentSyncConfig.enabled ?: false
        etGithubRepo.setText(currentSyncConfig.remoteUrl ?: "")
        etBranch.setText(currentSyncConfig.branch ?: "main")
        switchAutoSync.isChecked = currentSyncConfig.autoSync ?: false
        sbSyncInterval.value = (currentSyncConfig.syncIntervalSeconds ?: 300).toFloat()
        tvSyncIntervalValue.text = "${currentSyncConfig.syncIntervalSeconds ?: 300}秒"

        if (!currentSyncSecrets.token.isNullOrEmpty()) {
            tvTokenStatus.text = getString(R.string.token_configured)
            tvTokenStatus.setTextColor(getColor(com.google.android.material.R.color.material_dynamic_primary40))
        } else {
            tvTokenStatus.text = getString(R.string.token_not_configured)
            tvTokenStatus.setTextColor(getColor(com.google.android.material.R.color.design_default_color_error))
        }

    }



    private fun getUIConfig(): SyncConfig {
        return currentSyncConfig.copy(
            enabled = switchEnableSync.isChecked,
            remoteUrl = etGithubRepo.text?.toString() ?: "",
            transport = currentSyncConfig.transport ?: SyncTransport.HttpsToken,
            branch = etBranch.text?.toString()?.ifEmpty { "main" } ?: "main",
            autoSync = switchAutoSync.isChecked,
            syncIntervalSeconds = sbSyncInterval.value.toInt()
        )
    }

    private fun saveCurrentState() {
        val uiConfig = getUIConfig()
        val tokenInput = etHttpsToken.text?.toString() ?: ""
        val uiSecrets = if (tokenInput.isNotEmpty()) {
            currentSyncSecrets.copy(token = tokenInput)
        } else currentSyncSecrets

        ErrorUtil.safeRun(this) {
            if (::settingsRepository.isInitialized) {
                settingsRepository.saveSyncConfig(uiConfig)
                settingsRepository.saveSyncSecrets(uiSecrets)
            }
        }
        loadSyncState()
    }

    private fun handleDryRun() {
        saveCurrentState()

        Thread {
            val result = ErrorUtil.safeRun(this@SettingsActivity, NativeResult.Error("Exception during dry run")) {
                settingsRepository.performSyncDryRun(currentSyncConfig)
            }
            runOnUiThread {
                when (result) {
                    is NativeResult.Success -> {
                        val plan = result.data
                        if (plan != null) {
                            val msg = getString(R.string.sync_dry_run_result, plan.filesToUpload.size, plan.filesToDownload.size, plan.ignoredFiles.size)
                            Toast.makeText(this@SettingsActivity, msg, Toast.LENGTH_LONG).show()
                        }
                    }
                    is NativeResult.Error -> {
                        isSyncing = false
                        btnPerformSync.text = "立即同步"
                        btnPerformSync.isEnabled = true
                        Toast.makeText(this@SettingsActivity, result.message, Toast.LENGTH_LONG).show()
                    }
                    NativeResult.NotLoaded -> {
                        isSyncing = false
                        btnPerformSync.text = "立即同步"
                        btnPerformSync.isEnabled = true
                        Toast.makeText(this@SettingsActivity, getString(R.string.sync_error_not_loaded), Toast.LENGTH_SHORT).show()
                    }
                }
            }
        }.start()
    }

    private var isSyncing = false

    private fun handlePerformSync() {
        if (isSyncing) return
        isSyncing = true
        btnPerformSync.text = "同步中..."
        btnPerformSync.isEnabled = false
        saveCurrentState()

        if (currentSyncSecrets.token.isNullOrEmpty()) {
            isSyncing = false
            btnPerformSync.text = "立即同步"
            btnPerformSync.isEnabled = true
            Toast.makeText(this, getString(R.string.sync_error_no_token), Toast.LENGTH_SHORT).show()
            return
        }

        Thread {
            val result = ErrorUtil.safeRun(this@SettingsActivity, NativeResult.Error("Exception during sync")) {
                settingsRepository.performSync(currentSyncConfig)
            }
            runOnUiThread {
                when (result) {
                    is NativeResult.Success -> {
                        isSyncing = false
                        btnPerformSync.text = "立即同步"
                        btnPerformSync.isEnabled = true
                        val syncResult = result.data
                        if (syncResult != null) {
                            if (syncResult.userMessage != null) {
                                AlertDialog.Builder(this@SettingsActivity)
                                    .setMessage(syncResult.userMessage)
                                    .setPositiveButton(getString(R.string.action_ok), null)
                                    .show()
                            } else if (syncResult.firstSyncMode == FirstSyncMode.UnrelatedHistories || syncResult.firstSyncMode == FirstSyncMode.BlockedNonEmptyRemote) {
                                AlertDialog.Builder(this@SettingsActivity)
                                    .setMessage(getString(R.string.sync_error_unrelated))
                                    .setPositiveButton(getString(R.string.action_ok), null)
                                    .show()
                            } else if (syncResult.error != null) {
                                AlertDialog.Builder(this@SettingsActivity)
                                    .setMessage(syncResult.error)
                                    .setPositiveButton(getString(R.string.action_ok), null)
                                    .show()
                            } else if (syncResult.conflicts.isNotEmpty()) {
                                Toast.makeText(this@SettingsActivity, getString(R.string.sync_error_conflict), Toast.LENGTH_LONG).show()
                            } else {
                                Toast.makeText(this@SettingsActivity, getString(R.string.sync_success), Toast.LENGTH_SHORT).show()
                            }
                        }
                    }
                    is NativeResult.Error -> {
                        isSyncing = false
                        btnPerformSync.text = "立即同步"
                        btnPerformSync.isEnabled = true
                        Toast.makeText(this@SettingsActivity, result.message, Toast.LENGTH_LONG).show()
                    }
                    NativeResult.NotLoaded -> {
                        isSyncing = false
                        btnPerformSync.text = "立即同步"
                        btnPerformSync.isEnabled = true
                        Toast.makeText(this@SettingsActivity, getString(R.string.sync_error_not_loaded), Toast.LENGTH_SHORT).show()
                    }
                }
            }
        }.start()
    }

    private fun loadSyncState() {
        currentSyncConfig = ErrorUtil.safeRun(this, SyncConfig()) {
            if (::settingsRepository.isInitialized) {
                settingsRepository.loadSyncConfig()
            } else SyncConfig()
        }
        currentSyncSecrets = ErrorUtil.safeRun(this, SyncSecrets()) {
            if (::settingsRepository.isInitialized) {
                settingsRepository.loadSyncSecrets()
            } else SyncSecrets()
        }
    }

    override fun onBackPressed() {
        saveAndFinish(true)
        super.onBackPressed()
    }

    private fun saveAndFinish(finishActivity: Boolean = true) {
        val themeStr = when (spinnerTheme.selectedItemPosition) {
            1 -> "light"
            2 -> "dark"
            else -> "system"
        }

        val newSettings = currentSettings.copy(
            editorFontSize = sbFontSize.value,
            editorLineSpacingMultiplier = sbLineSpacing.value,
            autoSaveEnabled = switchAutoSave.isChecked,
            autoSaveDelayMs = sbAutoSaveDelay.value.toLong() * 1000L,
            autoIndentEnabled = switchAutoIndent.isChecked,
            autoIndentWidth = sbAutoIndentWidth.value,
            themeMode = themeStr,
            editorTypingAnimationEnabled = switchTypingAnimation.isChecked,
            editorSmoothCursorEnabled = switchSmoothCursor.isChecked
        )

        ErrorUtil.safeRun(this) {
            if (::settingsRepository.isInitialized) {
                settingsRepository.saveLocalSettings(newSettings)
            }
        }


        // Save Sync Config
        val newSyncConfig = currentSyncConfig.copy(
            enabled = switchEnableSync.isChecked,
            remoteUrl = etGithubRepo.text?.toString() ?: "",
            transport = currentSyncConfig.transport ?: SyncTransport.HttpsToken,
            branch = etBranch.text?.toString()?.ifEmpty { "main" } ?: "main",
            autoSync = switchAutoSync.isChecked,
            syncIntervalSeconds = sbSyncInterval.value.toInt()
        )

        val tokenInput = etHttpsToken.text?.toString() ?: ""
        val newSyncSecrets = if (tokenInput.isNotEmpty()) {
            currentSyncSecrets.copy(token = tokenInput)
        } else currentSyncSecrets

        ErrorUtil.safeRun(this) {
            if (::settingsRepository.isInitialized) {
                settingsRepository.saveSyncConfig(newSyncConfig)
                settingsRepository.saveSyncSecrets(newSyncSecrets)
            }
        }

        // Apply theme immediately if it changed
        if (currentSettings.themeMode != themeStr) {
            when (themeStr) {
                "light" -> AppCompatDelegate.setDefaultNightMode(AppCompatDelegate.MODE_NIGHT_NO)
                "dark" -> AppCompatDelegate.setDefaultNightMode(AppCompatDelegate.MODE_NIGHT_YES)
                else -> AppCompatDelegate.setDefaultNightMode(AppCompatDelegate.MODE_NIGHT_FOLLOW_SYSTEM)
            }
        }

        if (finishActivity) { finish() }
    }
}
