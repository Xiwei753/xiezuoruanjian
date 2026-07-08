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
import com.xiwei.sujian.ui.system.SystemBarsController
import com.xiwei.sujian.labs.ExperimentalSettingsRepository
import com.google.android.material.button.MaterialButton
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

class SettingsActivity : AppCompatActivity() {

    private lateinit var settingsRepository: SettingsRepository
    private lateinit var currentSettings: LocalSettings
    private lateinit var syncHelper: SyncSettingsHelper
    private var isRendering: Boolean = false

    private var systemBarsController: SystemBarsController? = null

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

    private lateinit var switchExperimentalFullscreen: MaterialSwitch
    private lateinit var switchDiagnosticsEnabled: MaterialSwitch
    private lateinit var switchDiagnosticsVerbose: MaterialSwitch
    private lateinit var btnExportDiagnostics: MaterialButton
    private lateinit var btnClearLogs: MaterialButton
    private lateinit var btnCopyDeviceInfo: MaterialButton


    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        ErrorUtil.safeRun(this) {
            settingsRepository = SettingsRepository(this)
            val settings = ErrorUtil.safeRun(this, LocalSettings()) {
                settingsRepository.getLocalSettings()
            }
            when (settings.themeMode) {
                "light" -> AppCompatDelegate.setDefaultNightMode(AppCompatDelegate.MODE_NIGHT_NO)
                "dark" -> AppCompatDelegate.setDefaultNightMode(AppCompatDelegate.MODE_NIGHT_YES)
                else -> AppCompatDelegate.setDefaultNightMode(AppCompatDelegate.MODE_NIGHT_FOLLOW_SYSTEM)
            }
        }

        setContentView(R.layout.activity_settings)

        systemBarsController = SystemBarsController(this)
        systemBarsController?.setupEdgeToEdge()

        currentSettings = ErrorUtil.safeRun(this, LocalSettings()) {
            if (::settingsRepository.isInitialized) {
                settingsRepository.getLocalSettings()
            } else {
                LocalSettings()
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

        val appBarLayout = findViewById<com.google.android.material.appbar.AppBarLayout>(R.id.appBarLayout)
        systemBarsController?.addAppBarTarget(appBarLayout)

        val nestedScrollView = findViewById<androidx.core.widget.NestedScrollView>(R.id.nestedScrollView)
        systemBarsController?.addBottomPaddingTarget(nestedScrollView)

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
        switchExperimentalFullscreen = findViewById(R.id.switchExperimentalFullscreen)
        btnExportDiagnostics = findViewById(R.id.btnExportDiagnostics)
        btnClearLogs = findViewById(R.id.btnClearLogs)
        btnCopyDeviceInfo = findViewById(R.id.btnCopyDeviceInfo)

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

        val saveSettingsListener = object : Slider.OnSliderTouchListener {
            override fun onStartTrackingTouch(slider: Slider) {}
            override fun onStopTrackingTouch(slider: Slider) {
                updateAndSaveFromSliders()
            }
        }
        sbFontSize.addOnSliderTouchListener(saveSettingsListener)
        sbLineSpacing.addOnSliderTouchListener(saveSettingsListener)
        sbAutoSaveDelay.addOnSliderTouchListener(saveSettingsListener)
        sbAutoIndentWidth.addOnSliderTouchListener(saveSettingsListener)
        sbTypingAnimationDuration.addOnSliderTouchListener(saveSettingsListener)
        sbSmoothCursorDuration.addOnSliderTouchListener(saveSettingsListener)

        val themeOptions = arrayOf(
            getString(R.string.theme_system),
            getString(R.string.theme_light),
            getString(R.string.theme_dark)
        )
        val themeAdapter = ArrayAdapter(this, android.R.layout.simple_dropdown_item_1line, themeOptions)
        actvTheme.setAdapter(themeAdapter)

        actvTheme.setOnItemClickListener { _, _, position, _ ->
            if (isRendering) return@setOnItemClickListener
            val themeStr = when (position) {
                1 -> "light"
                2 -> "dark"
                else -> "system"
            }
            updateLocalSettings { it.copy(themeMode = themeStr) }
            when (themeStr) {
                "light" -> AppCompatDelegate.setDefaultNightMode(AppCompatDelegate.MODE_NIGHT_NO)
                "dark" -> AppCompatDelegate.setDefaultNightMode(AppCompatDelegate.MODE_NIGHT_YES)
                else -> AppCompatDelegate.setDefaultNightMode(AppCompatDelegate.MODE_NIGHT_FOLLOW_SYSTEM)
            }
        }

        renderSettings(currentSettings, fromOnCreate = true)

        switchAutoSave.setOnCheckedChangeListener { _, isChecked ->
            if (isRendering) return@setOnCheckedChangeListener
            updateLocalSettings { it.copy(autoSaveEnabled = isChecked) }
        }
        switchAutoIndent.setOnCheckedChangeListener { _, isChecked ->
            if (isRendering) return@setOnCheckedChangeListener
            updateLocalSettings { it.copy(autoIndentEnabled = isChecked) }
        }
        switchTypingAnimation.setOnCheckedChangeListener { _, isChecked ->
            if (isRendering) return@setOnCheckedChangeListener
            sbTypingAnimationDuration.isEnabled = isChecked
            updateLocalSettings { it.copy(editorTypingAnimationEnabled = isChecked) }
        }
        switchSmoothCursor.setOnCheckedChangeListener { _, isChecked ->
            if (isRendering) return@setOnCheckedChangeListener
            sbSmoothCursorDuration.isEnabled = isChecked
            updateLocalSettings { it.copy(editorSmoothCursorEnabled = isChecked) }
        }

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

        switchDiagnosticsEnabled.setOnCheckedChangeListener { _, isChecked ->
            if (isRendering) return@setOnCheckedChangeListener
            var s = currentSettings.copy(diagnosticsEnabled = isChecked)
            switchDiagnosticsVerbose.isEnabled = isChecked
            DiagnosticsLogger.setEnabled(isChecked)
            EditorEventRingBuffer.setEnabled(isChecked)
            if (!isChecked) {
                s = s.copy(diagnosticsVerbose = false)
                switchDiagnosticsVerbose.isChecked = false
                DiagnosticsLogger.setVerbose(false)
            }
            currentSettings = s
            persistSettings()
        }
        switchDiagnosticsVerbose.setOnCheckedChangeListener { _, isChecked ->
            if (isRendering) return@setOnCheckedChangeListener
            currentSettings = currentSettings.copy(diagnosticsVerbose = isChecked)
            DiagnosticsLogger.setVerbose(isChecked)
            persistSettings()
        }

        val labsRepo = ExperimentalSettingsRepository(this)
        labsRepo.migrateFromLegacy(currentSettings.experimentalFullscreenMode)
        switchExperimentalFullscreen.setOnCheckedChangeListener { _, isChecked ->
            if (isRendering) return@setOnCheckedChangeListener
            labsRepo.setEnabled("fullscreen_immersive", isChecked)
            systemBarsController?.applyFullscreen(isChecked)
            updateLocalSettings { it.copy(experimentalFullscreenMode = isChecked) }
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

        syncHelper.bindSyncSettings()
    }

    private fun renderSettings(settings: LocalSettings, fromOnCreate: Boolean = false) {
        isRendering = true
        try {
            val effectiveFontSize = if (::settingsRepository.isInitialized && fromOnCreate) {
                settingsRepository.getEffectiveFontSize()
            } else {
                sbFontSize.value
            }
            if (fromOnCreate) {
                sbFontSize.value = effectiveFontSize
            }
            sbLineSpacing.value = settings.editorLineSpacingMultiplier
            switchAutoSave.isChecked = settings.autoSaveEnabled
            sbAutoSaveDelay.value = (settings.autoSaveDelayMs / 1000).toFloat()

            switchAutoIndent.isChecked = settings.autoIndentEnabled
            sbAutoIndentWidth.value = settings.autoIndentWidth

            switchTypingAnimation.isChecked = settings.editorTypingAnimationEnabled
            sbTypingAnimationDuration.isEnabled = settings.editorTypingAnimationEnabled
            switchSmoothCursor.isChecked = settings.editorSmoothCursorEnabled
            sbSmoothCursorDuration.isEnabled = settings.editorSmoothCursorEnabled
            sbTypingAnimationDuration.value = settings.editorTypingAnimationDurationMs.toFloat()
            sbSmoothCursorDuration.value = settings.editorSmoothCursorDurationMs.toFloat()

            val aiAvailable = try {
                if (::settingsRepository.isInitialized) settingsRepository.aiAvailable() else false
            } catch (e: Exception) { false }
            if (aiAvailable) {
                cardAi.visibility = android.view.View.VISIBLE
                switchAiEnabled.isChecked = settings.aiEnabled
                if (fromOnCreate) {
                    switchAiEnabled.setOnCheckedChangeListener { _, isChecked ->
                        if (isRendering) return@setOnCheckedChangeListener
                        updateLocalSettings { it.copy(aiEnabled = isChecked) }
                    }
                }
            } else {
                cardAi.visibility = android.view.View.GONE
            }

            tvFontSizeValue.text = "${sbFontSize.value.toInt()}sp"
            tvLineSpacingValue.text = "${String.format("%.1f", settings.editorLineSpacingMultiplier)}x"
            tvAutoSaveDelayValue.text = getString(R.string.auto_save_delay_seconds, (settings.autoSaveDelayMs / 1000).toInt())
            tvAutoIndentWidthValue.text = getString(R.string.auto_indent_width_chars, settings.autoIndentWidth)
            tvTypingAnimationDurationValue.text = "${settings.editorTypingAnimationDurationMs}ms"
            tvSmoothCursorDurationValue.text = "${settings.editorSmoothCursorDurationMs}ms"

            switchDiagnosticsEnabled.isChecked = settings.diagnosticsEnabled
            switchDiagnosticsVerbose.isChecked = settings.diagnosticsVerbose
            switchDiagnosticsVerbose.isEnabled = settings.diagnosticsEnabled
            switchExperimentalFullscreen.isChecked = settings.experimentalFullscreenMode
            DiagnosticsLogger.setEnabled(settings.diagnosticsEnabled)
            DiagnosticsLogger.setVerbose(settings.diagnosticsVerbose)
            EditorEventRingBuffer.setEnabled(settings.diagnosticsEnabled)

            actvTheme.setText(when (settings.themeMode) {
                "light" -> getString(R.string.theme_light)
                "dark" -> getString(R.string.theme_dark)
                else -> getString(R.string.theme_system)
            }, false)

            if (!fromOnCreate) {
                syncHelper.loadSyncState()
                syncHelper.bindSyncSettings()
            }
        } finally {
            isRendering = false
        }
    }

    private fun updateLocalSettings(transform: (LocalSettings) -> LocalSettings) {
        currentSettings = transform(currentSettings)
        persistSettings()
    }

    private fun updateAndSaveFromSliders() {
        currentSettings = currentSettings.copy(
            editorLineSpacingMultiplier = sbLineSpacing.value,
            autoSaveDelayMs = sbAutoSaveDelay.value.toLong() * 1000L,
            autoIndentWidth = sbAutoIndentWidth.value,
            editorTypingAnimationDurationMs = sbTypingAnimationDuration.value.toInt(),
            editorSmoothCursorDurationMs = sbSmoothCursorDuration.value.toInt(),
        )
        persistSettings()
    }

    private fun persistSettings() {
        ErrorUtil.safeRun(this) {
            if (::settingsRepository.isInitialized) {
                settingsRepository.saveLocalSettings(currentSettings)
                CoreSettingsEvents.markEditorChanged()
            }
        }
        val themeStr = currentSettings.themeMode
        val currentSyncable = settingsRepository.getSyncableSettings()
        val newSyncable = currentSyncable.copy(
            fontSize = sbFontSize.value.toDouble(),
            themeMode = themeStr ?: "system"
        )
        ErrorUtil.safeRun(this) {
            if (::settingsRepository.isInitialized) {
                settingsRepository.saveSyncableSettings(newSyncable)
            }
        }
    }

    override fun onResume() {
        super.onResume()
        if (CoreSettingsEvents.consumeChanged()) {
            currentSettings = settingsRepository.getLocalSettings()
            renderSettings(currentSettings)
        }
    }

    override fun onBackPressed() {
        saveAndFinish()
        super.onBackPressed()
    }

    private fun saveAndFinish() {
        persistSettings()

        val newSyncConfig = syncHelper.buildSaveSyncConfig()
        val newSyncSecrets = syncHelper.buildSaveSyncSecrets()

        ErrorUtil.safeRun(this) {
            if (::settingsRepository.isInitialized) {
                settingsRepository.saveSyncConfig(newSyncConfig)
                settingsRepository.saveSyncSecrets(newSyncSecrets)
            }
        }

        finish()
    }
}
