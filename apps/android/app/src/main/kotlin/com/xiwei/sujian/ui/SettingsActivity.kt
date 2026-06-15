package com.xiwei.sujian.ui

import android.os.Bundle
import com.google.android.material.slider.Slider
import android.widget.ArrayAdapter
import android.widget.Spinner
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity
import androidx.appcompat.app.AppCompatDelegate
import com.google.android.material.appbar.MaterialToolbar
import com.google.android.material.materialswitch.MaterialSwitch
import com.xiwei.sujian.R
import com.xiwei.sujian.data.SettingsRepository
import com.xiwei.sujian.data.CoreSettingsEvents
import com.xiwei.sujian.model.LocalSettings
import com.google.android.material.button.MaterialButton

class SettingsActivity : AppCompatActivity() {

    private lateinit var settingsRepository: SettingsRepository
    private lateinit var currentSettings: LocalSettings
    private lateinit var syncHelper: SyncSettingsHelper

    private lateinit var sbFontSize: Slider
    private lateinit var sbLineSpacing: Slider
    private lateinit var switchAutoSave: MaterialSwitch
    private lateinit var sbAutoSaveDelay: Slider
    private lateinit var spinnerTheme: Spinner
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

    private lateinit var btnActionRegistry: MaterialButton


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
        sbTypingAnimationDuration = findViewById(R.id.sbTypingAnimationDuration)
        tvTypingAnimationDurationValue = findViewById(R.id.tvTypingAnimationDurationValue)
        sbSmoothCursorDuration = findViewById(R.id.sbSmoothCursorDuration)
        tvSmoothCursorDurationValue = findViewById(R.id.tvSmoothCursorDurationValue)

        tvAiSection = findViewById(R.id.tvAiSection)
        switchAiEnabled = findViewById(R.id.switchAiEnabled)

        tvWorkspacePath = findViewById(R.id.tvWorkspacePath)
        tvVersionInfo = findViewById(R.id.tvVersionInfo)

        btnActionRegistry = findViewById(R.id.btnActionRegistry)


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
        sbAutoIndentWidth.addOnChangeListener { _, value, _ ->
            tvAutoIndentWidthValue.text = "${value}字符"
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


        // Setup Theme Spinner
        val themeOptions = arrayOf(
            getString(R.string.theme_system),
            getString(R.string.theme_light),
            getString(R.string.theme_dark)
        )
        val adapter = ArrayAdapter(this, android.R.layout.simple_spinner_dropdown_item, themeOptions)
        spinnerTheme.adapter = adapter

        // Bind existing settings
        sbFontSize.value = effectiveFontSize
        sbLineSpacing.value = currentSettings.editorLineSpacingMultiplier
        switchAutoSave.isChecked = currentSettings.autoSaveEnabled
        sbAutoSaveDelay.value = (currentSettings.autoSaveDelayMs / 1000).toFloat()

        switchAutoIndent.isChecked = currentSettings.autoIndentEnabled
        sbAutoIndentWidth.value = currentSettings.autoIndentWidth

        switchTypingAnimation.isChecked = currentSettings.editorTypingAnimationEnabled
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
            tvAiSection.visibility = android.view.View.VISIBLE
            switchAiEnabled.visibility = android.view.View.VISIBLE
            switchAiEnabled.isChecked = currentSettings.aiEnabled
            switchAiEnabled.setOnCheckedChangeListener { _, isChecked ->
                currentSettings = currentSettings.copy(aiEnabled = isChecked)
                saveAndFinish(false)
            }
        } else {
            tvAiSection.visibility = android.view.View.GONE
            switchAiEnabled.visibility = android.view.View.GONE
        }

        // Initial texts
        tvFontSizeValue.text = "${effectiveFontSize.toInt()}sp"
        tvLineSpacingValue.text = "${String.format("%.1f", currentSettings.editorLineSpacingMultiplier)}x"
        tvAutoSaveDelayValue.text = "${(currentSettings.autoSaveDelayMs / 1000).toInt()}秒"
        tvAutoIndentWidthValue.text = "${currentSettings.autoIndentWidth}字符"
        tvTypingAnimationDurationValue.text = "${currentSettings.editorTypingAnimationDurationMs}ms"
        tvSmoothCursorDurationValue.text = "${currentSettings.editorSmoothCursorDurationMs}ms"

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
            tvVersionInfo.text = "v$vName (Build $vCode) Android Native 客户端"
        } catch (e: Exception) {
            tvVersionInfo.text = "Android Native 客户端"
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
        switchSmoothCursor.isChecked = currentSettings.editorSmoothCursorEnabled
        sbTypingAnimationDuration.value = currentSettings.editorTypingAnimationDurationMs.toFloat()
        sbSmoothCursorDuration.value = currentSettings.editorSmoothCursorDurationMs.toFloat()

        tvFontSizeValue.text = "${effectiveFontSize.toInt()}sp"
        tvLineSpacingValue.text = "${String.format("%.1f", currentSettings.editorLineSpacingMultiplier)}x"
        tvAutoSaveDelayValue.text = "${(currentSettings.autoSaveDelayMs / 1000).toInt()}秒"
        tvAutoIndentWidthValue.text = "${currentSettings.autoIndentWidth}字符"
        tvTypingAnimationDurationValue.text = "${currentSettings.editorTypingAnimationDurationMs}ms"
        tvSmoothCursorDurationValue.text = "${currentSettings.editorSmoothCursorDurationMs}ms"

        val syncable = settingsRepository.getSyncableSettings()
        if (syncable.themeMode.isNotEmpty()) {
            currentSettings = currentSettings.copy(themeMode = syncable.themeMode)
        }
        when (currentSettings.themeMode) {
            "light" -> spinnerTheme.setSelection(1, false)
            "dark" -> spinnerTheme.setSelection(2, false)
            else -> spinnerTheme.setSelection(0, false)
        }

        syncHelper.loadSyncState()
        syncHelper.bindSyncSettings()
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
            aiEnabled = switchAiEnabled.isChecked
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
