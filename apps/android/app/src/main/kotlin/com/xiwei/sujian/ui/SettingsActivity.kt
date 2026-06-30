package com.xiwei.sujian.ui

import android.os.Bundle
import com.google.android.material.slider.Slider
import android.widget.ArrayAdapter
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity
import androidx.appcompat.app.AppCompatDelegate
import com.google.android.material.appbar.MaterialToolbar
import com.google.android.material.materialswitch.MaterialSwitch
import com.google.android.material.textfield.MaterialAutoCompleteTextView
import com.xiwei.sujian.R
import com.xiwei.sujian.data.SettingsRepository
import com.xiwei.sujian.data.CoreSettingsEvents
import com.xiwei.sujian.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.diagnostics.DiagnosticsExporter
import com.xiwei.sujian.diagnostics.EditorEventRingBuffer
import com.xiwei.sujian.model.LocalSettings
import com.google.android.material.button.MaterialButton
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

class SettingsActivity : AppCompatActivity() {

    private lateinit var settingsRepository: SettingsRepository
    private lateinit var currentSettings: LocalSettings
    private lateinit var syncHelper: SyncSettingsHelper

    private lateinit var sbFontSize: Slider
    private lateinit var sbLineSpacing: Slider
    private lateinit var switchAutoSave: MaterialSwitch
    private lateinit var sbAutoSaveDelay: Slider
    private lateinit var actvTheme: MaterialAutoCompleteTextView
    private lateinit var tvWorkspacePath: TextView
    private lateinit var tvVersionInfo: TextView

    private lateinit var tvFontSizeValue: TextView
    private lateinit var tvLineSpacingValue: TextView
    private lateinit var tvAutoSaveDelayValue: TextView
    private lateinit var tvSyncIntervalValue: TextView

    private lateinit var switchAutoIndent: MaterialSwitch
    private lateinit var sbAutoIndentWidth: Slider
    private lateinit var tvAutoIndentWidthValue: TextView

    private lateinit var switchTypingAnimation: MaterialSwitch
    private lateinit var switchSmoothCursor: MaterialSwitch
    private lateinit var sbTypingAnimationDuration: Slider
    private lateinit var tvTypingAnimationDurationValue: TextView
    private lateinit var sbSmoothCursorDuration: Slider
    private lateinit var tvSmoothCursorDurationValue: TextView

    private lateinit var tvAiSection: TextView
    private lateinit var switchAiEnabled: MaterialSwitch
    private lateinit var cardAi: com.google.android.material.card.MaterialCardView

    private lateinit var btnActionRegistry: MaterialButton
    private lateinit var btnAbout: MaterialButton

    private lateinit var switchDiagnosticsEnabled: MaterialSwitch
    private lateinit var switchDiagnosticsVerbose: MaterialSwitch
    private lateinit var btnExportDiagnostics: MaterialButton
    private lateinit var btnClearLogs: MaterialButton
    private lateinit var btnCopyDeviceInfo: MaterialButton


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

        val effectiveFontSize = ErrorUtil.safeRun(this, 16f) {
            if (::settingsRepository.isInitialized) {
                settingsRepository.getEffectiveFontSize()
            } else {
                16f
            }
        }

        syncHelper = SyncSettingsHelper(this, settingsRepository)
        syncHelper.initViews()
        syncHelper.loadSyncState()

        val toolbar: MaterialToolbar = findViewById(R.id.toolbar)
        setSupportActionBar(toolbar)
        toolbar.setNavigationOnClickListener {
            saveAndFinish()
        }

        sbFontSize = findViewById(R.id.sbFontSize)
        sbLineSpacing = findViewById(R.id.sbLineSpacing)
        switchAutoSave = findViewById(R.id.switchAutoSave)
        sbAutoSaveDelay = findViewById(R.id.sbAutoSaveDelay)
        actvTheme = findViewById(R.id.actvTheme)

        tvFontSizeValue = findViewById(R.id.tvFontSizeValue)
        tvLineSpacingValue = findViewById(R.id.tvLineSpacingValue)
        tvAutoSaveDelayValue = findViewById(R.id.tvAutoSaveDelayValue)
        tvSyncIntervalValue = findViewById(R.id.tvSyncIntervalValue)

        switchAutoIndent = findViewById(R.id.switchAutoIndent)
        sbAutoIndentWidth = findViewById(R.id.sbAutoIndentWidth)
        tvAutoIndentWidthValue = findViewById(R.id.tvAutoIndentWidthValue)
        switchTypingAnimation = findViewById(R.id.switchTypingAnimation)
        switchSmoothCursor = findViewById(R.id.switchSmoothCursor)
        sbTypingAnimationDuration = findViewById(R.id.sbTypingAnimationDuration)
        tvTypingAnimationDurationValue = findViewById(R.id.tvTypingAnimationDurationValue)
        sbSmoothCursorDuration = findViewById(R.id.sbSmoothCursorDuration)
        tvSmoothCursorDurationValue = findViewById(R.id.tvSmoothCursorDurationValue)

        tvAiSection = findViewById(R.id.tvAiSection)
        switchAiEnabled = findViewById(R.id.switchAiEnabled)
        cardAi = findViewById(R.id.cardAi)

        tvWorkspacePath = findViewById(R.id.tvWorkspacePath)
        tvVersionInfo = findViewById(R.id.tvVersionInfo)

        btnActionRegistry = findViewById(R.id.btnActionRegistry)
        btnAbout = findViewById(R.id.btnAbout)

        switchDiagnosticsEnabled = findViewById(R.id.switchDiagnosticsEnabled)
        switchDiagnosticsVerbose = findViewById(R.id.switchDiagnosticsVerbose)
        btnExportDiagnostics = findViewById(R.id.btnExportDiagnostics)
        btnClearLogs = findViewById(R.id.btnClearLogs)
        btnCopyDeviceInfo = findViewById(R.id.btnCopyDeviceInfo)


        // Live value update listeners
        sbFontSize.addOnChangeListener { _, value, _ ->
            tvFontSizeValue.text = "${value.toInt()}sp"
        }
        sbLineSpacing.addOnChangeListener { _, value, _ ->
            tvLineSpacingValue.text = "${String.format("%.1f", value)}x"
        }
        sbAutoSaveDelay.addOnChangeListener { _, value, _ ->
            tvAutoSaveDelayValue.text = getString(R.string.auto_save_delay_seconds, value.toInt())
        }
        sbAutoIndentWidth.addOnChangeListener { _, value, _ ->
            tvAutoIndentWidthValue.text = getString(R.string.auto_indent_width_chars, value)
        }
        sbTypingAnimationDuration.addOnChangeListener { _, value, _ ->
            tvTypingAnimationDurationValue.text = "${value.toInt()}ms"
        }
        sbSmoothCursorDuration.addOnChangeListener { _, value, _ ->
            tvSmoothCursorDurationValue.text = "${value.toInt()}ms"
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
        sbAutoIndentWidth.addOnSliderTouchListener(saveSettingsListener)
        sbTypingAnimationDuration.addOnSliderTouchListener(saveSettingsListener)
        sbSmoothCursorDuration.addOnSliderTouchListener(saveSettingsListener)


        // Setup Theme AutoCompleteTextView
        val themeOptions = arrayOf(
            getString(R.string.theme_system),
            getString(R.string.theme_light),
            getString(R.string.theme_dark)
        )
        val themeAdapter = ArrayAdapter(this, android.R.layout.simple_dropdown_item_1line, themeOptions)
        actvTheme.setAdapter(themeAdapter)

        // Set current selection
        actvTheme.setText(when (currentSettings.themeMode) {
            "light" -> getString(R.string.theme_light)
            "dark" -> getString(R.string.theme_dark)
            else -> getString(R.string.theme_system)
        }, false)

        actvTheme.setOnItemClickListener { _, _, position, _ ->
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

        // Bind existing settings
        sbFontSize.value = effectiveFontSize
        sbLineSpacing.value = currentSettings.editorLineSpacingMultiplier
        switchAutoSave.isChecked = currentSettings.autoSaveEnabled
        sbAutoSaveDelay.value = (currentSettings.autoSaveDelayMs / 1000).toFloat()

        switchAutoIndent.isChecked = currentSettings.autoIndentEnabled
        sbAutoIndentWidth.value = currentSettings.autoIndentWidth

        switchTypingAnimation.isChecked = currentSettings.editorTypingAnimationEnabled
        sbTypingAnimationDuration.isEnabled = switchTypingAnimation.isChecked
        switchSmoothCursor.isChecked = currentSettings.editorSmoothCursorEnabled
        sbTypingAnimationDuration.value = currentSettings.editorTypingAnimationDurationMs.toFloat()
        sbSmoothCursorDuration.value = currentSettings.editorSmoothCursorDurationMs.toFloat()

        // AI settings - hide section if AI not available (compile-time)
        val aiAvailable = try {
            if (::settingsRepository.isInitialized) {
                settingsRepository.aiAvailable()
            } else false
        } catch (e: Exception) { false }
        if (aiAvailable) {
            cardAi.visibility = android.view.View.VISIBLE
            switchAiEnabled.isChecked = currentSettings.aiEnabled
            switchAiEnabled.setOnCheckedChangeListener { _, isChecked ->
                currentSettings = currentSettings.copy(aiEnabled = isChecked)
                saveAndFinish(false)
            }
        } else {
            cardAi.visibility = android.view.View.GONE
        }

        // Initial texts
        tvFontSizeValue.text = "${effectiveFontSize.toInt()}sp"
        tvLineSpacingValue.text = "${String.format("%.1f", currentSettings.editorLineSpacingMultiplier)}x"
        tvAutoSaveDelayValue.text = getString(R.string.auto_save_delay_seconds, (currentSettings.autoSaveDelayMs / 1000).toInt())
        tvAutoIndentWidthValue.text = getString(R.string.auto_indent_width_chars, currentSettings.autoIndentWidth)
        tvTypingAnimationDurationValue.text = "${currentSettings.editorTypingAnimationDurationMs}ms"
        tvSmoothCursorDurationValue.text = "${currentSettings.editorSmoothCursorDurationMs}ms"

        switchAutoSave.setOnCheckedChangeListener { _, _ -> saveAndFinish(false) }
        switchAutoIndent.setOnCheckedChangeListener { _, _ -> saveAndFinish(false) }
        switchTypingAnimation.setOnCheckedChangeListener { _, isChecked ->
            sbTypingAnimationDuration.isEnabled = isChecked
            saveAndFinish(false)
        }
        switchSmoothCursor.setOnCheckedChangeListener { _, _ -> saveAndFinish(false) }





        tvWorkspacePath.text = if (::settingsRepository.isInitialized) {
            settingsRepository.workspaceDir()
        } else {
            ""
        }

        try {
            val packageInfo = packageManager.getPackageInfo(packageName, 0)
            val vName = packageInfo.versionName
            val vCode = if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.P) {
                packageInfo.longVersionCode
            } else {
                @Suppress("DEPRECATION")
                packageInfo.versionCode.toLong()
            }
            tvVersionInfo.text = getString(R.string.version_info, vName, vCode)
        } catch (e: Exception) {
            tvVersionInfo.text = getString(R.string.version_info_fallback)
        }

        syncHelper.btnDryRun.setOnClickListener {
            syncHelper.handleDryRun()
        }
        syncHelper.btnTestConnection.setOnClickListener {
            syncHelper.handleTestConnection()
        }
        syncHelper.btnPerformSync.setOnClickListener {
            syncHelper.handlePerformSync()
        }

        btnActionRegistry.setOnClickListener {
            val intent = android.content.Intent(this, ActionRegistryActivity::class.java)
            startActivity(intent)
        }

        btnAbout.setOnClickListener {
            val intent = android.content.Intent(this, AboutActivity::class.java)
            startActivity(intent)
        }

        switchDiagnosticsEnabled.isChecked = currentSettings.diagnosticsEnabled
        switchDiagnosticsVerbose.isChecked = currentSettings.diagnosticsVerbose
        switchDiagnosticsVerbose.isEnabled = currentSettings.diagnosticsEnabled
        DiagnosticsLogger.init(this, currentSettings.diagnosticsEnabled, currentSettings.diagnosticsVerbose)
        EditorEventRingBuffer.setEnabled(currentSettings.diagnosticsEnabled)

        switchDiagnosticsEnabled.setOnCheckedChangeListener { _, isChecked ->
            currentSettings = currentSettings.copy(diagnosticsEnabled = isChecked)
            switchDiagnosticsVerbose.isEnabled = isChecked
            DiagnosticsLogger.setEnabled(isChecked)
            EditorEventRingBuffer.setEnabled(isChecked)
            if (!isChecked) {
                currentSettings = currentSettings.copy(diagnosticsVerbose = false)
                switchDiagnosticsVerbose.isChecked = false
                DiagnosticsLogger.setVerbose(false)
            }
            saveAndFinish(false)
        }
        switchDiagnosticsVerbose.setOnCheckedChangeListener { _, isChecked ->
            currentSettings = currentSettings.copy(diagnosticsVerbose = isChecked)
            DiagnosticsLogger.setVerbose(isChecked)
            saveAndFinish(false)
        }

        btnExportDiagnostics.setOnClickListener {
            btnExportDiagnostics.isEnabled = false
            lifecycleScope.launch {
                val zipFile = withContext(Dispatchers.IO) {
                    DiagnosticsLogger.flush()
                    DiagnosticsExporter.export(applicationContext)
                }
                btnExportDiagnostics.isEnabled = true
                if (zipFile != null) {
                    DiagnosticsExporter.shareZip(this@SettingsActivity, zipFile)
                } else {
                    android.widget.Toast.makeText(this@SettingsActivity, getString(R.string.diagnostics_export_failed), android.widget.Toast.LENGTH_SHORT).show()
                }
            }
        }

        btnClearLogs.setOnClickListener {
            DiagnosticsLogger.clearLogs()
            EditorEventRingBuffer.clear()
            android.widget.Toast.makeText(this, getString(R.string.diagnostics_cleared), android.widget.Toast.LENGTH_SHORT).show()
        }

        btnCopyDeviceInfo.setOnClickListener {
            val deviceInfoJson = DiagnosticsExporter.getDeviceInfoJson(this)
            val clipboard = getSystemService(android.content.Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager
            clipboard.setPrimaryClip(android.content.ClipData.newPlainText("device_info", deviceInfoJson))
            android.widget.Toast.makeText(this, getString(R.string.diagnostics_device_info_copied), android.widget.Toast.LENGTH_SHORT).show()
        }

        // Bind Sync Settings
        syncHelper.bindSyncSettings()
    }



    override fun onResume() {
        super.onResume()
        if (CoreSettingsEvents.consumeChanged()) {
            reloadSettings()
        }
    }

    private fun reloadSettings() {
        if (!::settingsRepository.isInitialized) return

        currentSettings = settingsRepository.getLocalSettings()
        val effectiveFontSize = settingsRepository.getEffectiveFontSize()

        sbFontSize.value = effectiveFontSize
        sbLineSpacing.value = currentSettings.editorLineSpacingMultiplier
        switchAutoSave.isChecked = currentSettings.autoSaveEnabled
        sbAutoSaveDelay.value = (currentSettings.autoSaveDelayMs / 1000).toFloat()
        switchAutoIndent.isChecked = currentSettings.autoIndentEnabled
        sbAutoIndentWidth.value = currentSettings.autoIndentWidth
        switchTypingAnimation.isChecked = currentSettings.editorTypingAnimationEnabled
        sbTypingAnimationDuration.isEnabled = currentSettings.editorTypingAnimationEnabled
        switchSmoothCursor.isChecked = currentSettings.editorSmoothCursorEnabled
        sbTypingAnimationDuration.value = currentSettings.editorTypingAnimationDurationMs.toFloat()
        sbSmoothCursorDuration.value = currentSettings.editorSmoothCursorDurationMs.toFloat()

        tvFontSizeValue.text = "${effectiveFontSize.toInt()}sp"
        tvLineSpacingValue.text = "${String.format("%.1f", currentSettings.editorLineSpacingMultiplier)}x"
        tvAutoSaveDelayValue.text = getString(R.string.auto_save_delay_seconds, (currentSettings.autoSaveDelayMs / 1000).toInt())
        tvAutoIndentWidthValue.text = getString(R.string.auto_indent_width_chars, currentSettings.autoIndentWidth)
        tvTypingAnimationDurationValue.text = "${currentSettings.editorTypingAnimationDurationMs}ms"
        tvSmoothCursorDurationValue.text = "${currentSettings.editorSmoothCursorDurationMs}ms"

        switchDiagnosticsEnabled.isChecked = currentSettings.diagnosticsEnabled
        switchDiagnosticsVerbose.isChecked = currentSettings.diagnosticsVerbose
        switchDiagnosticsVerbose.isEnabled = currentSettings.diagnosticsEnabled
        DiagnosticsLogger.setEnabled(currentSettings.diagnosticsEnabled)
        DiagnosticsLogger.setVerbose(currentSettings.diagnosticsVerbose)
        EditorEventRingBuffer.setEnabled(currentSettings.diagnosticsEnabled)

        val syncable = settingsRepository.getSyncableSettings()
        if (syncable.themeMode.isNotEmpty()) {
            currentSettings = currentSettings.copy(themeMode = syncable.themeMode)
        }
        actvTheme.setText(when (currentSettings.themeMode) {
            "light" -> getString(R.string.theme_light)
            "dark" -> getString(R.string.theme_dark)
            else -> getString(R.string.theme_system)
        }, false)

        syncHelper.loadSyncState()
        syncHelper.bindSyncSettings()
    }

    override fun onBackPressed() {
        saveAndFinish(true)
        super.onBackPressed()
    }

    private fun saveAndFinish(finishActivity: Boolean = true) {
        // Derive theme from actvTheme text
        val currentThemeText = actvTheme.text.toString()
        val themeStr = when {
            currentThemeText == getString(R.string.theme_light) -> "light"
            currentThemeText == getString(R.string.theme_dark) -> "dark"
            else -> "system"
        }

        val newSettings = currentSettings.copy(
            editorLineSpacingMultiplier = sbLineSpacing.value,
            autoSaveEnabled = switchAutoSave.isChecked,
            autoSaveDelayMs = sbAutoSaveDelay.value.toLong() * 1000L,
            autoIndentEnabled = switchAutoIndent.isChecked,
            autoIndentWidth = sbAutoIndentWidth.value,
            themeMode = themeStr,
            editorTypingAnimationEnabled = switchTypingAnimation.isChecked,
            editorSmoothCursorEnabled = switchSmoothCursor.isChecked,
            editorTypingAnimationDurationMs = sbTypingAnimationDuration.value.toInt(),
            editorSmoothCursorDurationMs = sbSmoothCursorDuration.value.toInt(),
            aiEnabled = switchAiEnabled.isChecked,
            diagnosticsEnabled = switchDiagnosticsEnabled.isChecked,
            diagnosticsVerbose = switchDiagnosticsVerbose.isChecked
        )

        ErrorUtil.safeRun(this) {
            if (::settingsRepository.isInitialized) {
                settingsRepository.saveLocalSettings(newSettings)
            }
        }

        val currentSyncable = settingsRepository.getSyncableSettings()
        val newSyncable = currentSyncable.copy(
            fontSize = sbFontSize.value.toDouble(),
            themeMode = themeStr
        )
        ErrorUtil.safeRun(this) {
            if (::settingsRepository.isInitialized) {
                settingsRepository.saveSyncableSettings(newSyncable)
            }
        }


        // Save Sync Config
        val newSyncConfig = syncHelper.buildSaveSyncConfig()
        val newSyncSecrets = syncHelper.buildSaveSyncSecrets()

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
