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
import com.xiwei.writerapp.data.SettingsChangeBus
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

    private lateinit var spinnerProxyType: Spinner
    private lateinit var etProxyHost: TextInputEditText
    private lateinit var etProxyPort: TextInputEditText

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

    private lateinit var btnDryRun: MaterialButton
    private lateinit var btnTestConnection: MaterialButton
    private lateinit var btnPerformSync: MaterialButton
    private lateinit var btnActionRegistry: MaterialButton

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

        val effectiveFontSize = ErrorUtil.safeRun(this, 16f) {
            if (::settingsRepository.isInitialized) {
                settingsRepository.getEffectiveFontSize()
            } else {
                16f
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
        sbTypingAnimationDuration = findViewById(R.id.sbTypingAnimationDuration)
        tvTypingAnimationDurationValue = findViewById(R.id.tvTypingAnimationDurationValue)
        sbSmoothCursorDuration = findViewById(R.id.sbSmoothCursorDuration)
        tvSmoothCursorDurationValue = findViewById(R.id.tvSmoothCursorDurationValue)

        tvWorkspacePath = findViewById(R.id.tvWorkspacePath)

        switchEnableSync = findViewById(R.id.switchEnableSync)
        etGithubRepo = findViewById(R.id.etGithubRepo)
        etBranch = findViewById(R.id.etBranch)
        etHttpsToken = findViewById(R.id.etHttpsToken)
        tvTokenStatus = findViewById(R.id.tvTokenStatus)
        switchAutoSync = findViewById(R.id.switchAutoSync)
        sbSyncInterval = findViewById(R.id.sbSyncInterval)

        etHttpsToken.addTextChangedListener(object : android.text.TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {}
            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {
                updateTokenStatusUI()
            }
            override fun afterTextChanged(s: android.text.Editable?) {}
        })
        spinnerProxyType = findViewById(R.id.spinnerProxyType)
        etProxyHost = findViewById(R.id.etProxyHost)
        etProxyPort = findViewById(R.id.etProxyPort)
        btnDryRun = findViewById(R.id.btnDryRun)
        btnTestConnection = findViewById(R.id.btnTestConnection)
        btnPerformSync = findViewById(R.id.btnPerformSync)
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
        sbSyncInterval.addOnChangeListener { _, value, _ ->
            tvSyncIntervalValue.text = "${value.toInt()}秒"
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
        sbSyncInterval.addOnSliderTouchListener(saveSettingsListener)
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

        spinnerProxyType.onItemSelectedListener = object : android.widget.AdapterView.OnItemSelectedListener {
            override fun onItemSelected(parent: android.widget.AdapterView<*>?, view: android.view.View?, position: Int, id: Long) {
                val enabled = position == 2 || position == 3
                etProxyHost.isEnabled = enabled
                etProxyPort.isEnabled = enabled

                val currentPortStr = etProxyPort.text?.toString() ?: ""
                val currentPort = currentPortStr.toIntOrNull()

                if (position == 3) { // SOCKS5
                    if (currentPortStr.isEmpty() || currentPort == 7890) {
                        etProxyPort.setText("7891")
                    }
                } else if (position == 2) { // HTTP
                    if (currentPortStr.isEmpty() || currentPort == 7891) {
                        etProxyPort.setText("7890")
                    }
                }
            }
            override fun onNothingSelected(parent: android.widget.AdapterView<*>?) {}
        }

        tvWorkspacePath.text = WorkspaceManager.getWorkspaceDir(this).absolutePath

        btnDryRun.setOnClickListener {
            handleDryRun()
        }
        btnTestConnection.setOnClickListener {
            handleTestConnection()
        }

        btnPerformSync.setOnClickListener {
            handlePerformSync()
        }

        btnActionRegistry.setOnClickListener {
            val intent = android.content.Intent(this, ActionRegistryActivity::class.java)
            startActivity(intent)
        }


        // Bind Sync Settings
        switchEnableSync.isChecked = currentSyncConfig.enabled ?: false
        etGithubRepo.setText(currentSyncConfig.remoteUrl ?: "")
        etBranch.setText(currentSyncConfig.branch ?: "main")
        switchAutoSync.isChecked = currentSyncConfig.autoSync ?: false
        sbSyncInterval.value = (currentSyncConfig.syncIntervalSeconds ?: 300).toFloat()
        tvSyncIntervalValue.text = "${currentSyncConfig.syncIntervalSeconds ?: 300}秒"

        val proxyType = currentSyncConfig.proxyType ?: "none"
        spinnerProxyType.setSelection(when (proxyType) {
            "none" -> 0
            "auto" -> 1
            "http" -> 2
            "socks5" -> 3
            else -> 0
        })
        etProxyHost.setText(currentSyncConfig.proxyHost ?: "127.0.0.1")
        val defaultPort = if (proxyType == "socks5") 7891 else 7890
        etProxyPort.setText((currentSyncConfig.proxyPort ?: defaultPort).toString())

        updateTokenStatusUI()

    }



    private fun getUIConfig(): SyncConfig {
        return currentSyncConfig.copy(
            enabled = switchEnableSync.isChecked,
            backendType = com.xiwei.writerapp.model.BackendType.GithubApi,
            remoteUrl = etGithubRepo.text?.toString() ?: "",
            transport = currentSyncConfig.transport ?: SyncTransport.HttpsToken,
            branch = etBranch.text?.toString()?.ifEmpty { "main" } ?: "main",
            autoSync = switchAutoSync.isChecked,
            syncIntervalSeconds = sbSyncInterval.value.toInt(),
            proxyEnabled = spinnerProxyType.selectedItemPosition >= 1,
            proxyType = when (spinnerProxyType.selectedItemPosition) {
                0 -> "none"
                1 -> "auto"
                2 -> "http"
                3 -> "socks5"
                else -> "none"
            },
            proxyHost = etProxyHost.text?.toString()?.ifEmpty { "127.0.0.1" } ?: "127.0.0.1",
            proxyPort = etProxyPort.text?.toString()?.toIntOrNull() ?: if (spinnerProxyType.selectedItemPosition == 3) 7891 else 7890
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
        currentSyncConfig = uiConfig
        currentSyncSecrets = uiSecrets
        updateTokenStatusUI()
    }

    private fun handleDryRun() {
        if (isDryRunning || isSyncing || isTesting) return
        isDryRunning = true
        btnDryRun.text = "检查中..."
        btnDryRun.isEnabled = false
        btnPerformSync.isEnabled = false
        btnTestConnection.isEnabled = false
        saveCurrentState()

        Thread {
            val result = ErrorUtil.safeRun(this@SettingsActivity, NativeResult.Error("Exception during dry run")) {
                settingsRepository.performSyncDryRun(currentSyncConfig)
            }
            if (isDestroyed || isFinishing) return@Thread
            runOnUiThread {
                isDryRunning = false
                btnDryRun.text = "检查同步计划"
                btnDryRun.isEnabled = true
                btnPerformSync.isEnabled = true
                btnTestConnection.isEnabled = true
                when (result) {
                    is NativeResult.Success -> {
                        val plan = result.data
                        if (plan != null) {
                            val msg = "同步计划检查完成: " + getString(R.string.sync_dry_run_result, plan.filesToUpload.size, plan.filesToDownload.size, plan.ignoredFiles.size)
                            Toast.makeText(this@SettingsActivity, msg, Toast.LENGTH_LONG).show()
                        }
                    }
                    is NativeResult.Error -> {
                        Toast.makeText(this@SettingsActivity, result.message, Toast.LENGTH_LONG).show()
                    }
                    NativeResult.NotLoaded -> {
                        Toast.makeText(this@SettingsActivity, getString(R.string.sync_error_not_loaded), Toast.LENGTH_SHORT).show()
                    }
                }
            }
        }.start()
    }


    private var isTesting = false
    private fun handleTestConnection() {
        if (isTesting || isDryRunning || isSyncing) return
        isTesting = true
        btnTestConnection.text = "检查中..."
        btnTestConnection.isEnabled = false
        btnDryRun.isEnabled = false
        btnPerformSync.isEnabled = false
        btnTestConnection.isEnabled = false
        saveCurrentState()

        Thread {
            val result = ErrorUtil.safeRun(this@SettingsActivity, NativeResult.Error("Exception during diagnostic run")) {
                settingsRepository.performSyncDiagnostics(currentSyncConfig)
            }
            if (isDestroyed || isFinishing) return@Thread
            runOnUiThread {
                isTesting = false
                btnTestConnection.text = getString(R.string.btn_test_connection)
                btnTestConnection.isEnabled = true
                btnDryRun.isEnabled = true
                btnPerformSync.isEnabled = true
                btnTestConnection.isEnabled = true
                when (result) {
                    is NativeResult.Success -> {
                        val diag = result.data
                        if (diag != null) {
                            val msgBuilder = StringBuilder()
                            val mapStatus = { s: String ->
                                when (s) {
                                    "ok" -> "正常"
                                    "failed" -> "失败"
                                    "skipped" -> "已跳过"
                                    else -> if (s.startsWith("failed")) "失败 ($s)" else "未检查 ($s)"
                                }
                            }

                            // Android permission status
                            msgBuilder.append("=== 权限状态 ===\n")
                            msgBuilder.append("INTERNET 权限: ${if (diag.androidHasInternetPermission) "已授予" else "缺失"}\n")
                            msgBuilder.append("网络状态权限: ${if (diag.androidHasAccessNetworkStatePermission) "已授予" else "缺失"}\n")
                            msgBuilder.append("网络状态: ${
                                when (diag.androidNetworkState) {
                                    "permission_granted" -> "权限已授予，可检测网络"
                                    "unknown_no_permission" -> "未知（缺少 ACCESS_NETWORK_STATE 权限）"
                                    "failed_no_internet_permission" -> "无 INTERNET 权限，无法联网"
                                    else -> diag.androidNetworkState
                                }
                            }\n\n")

                            // If INTERNET permission is missing, stop here
                            if (!diag.androidHasInternetPermission) {
                                msgBuilder.append("\n${diag.userMessage}")
                                AlertDialog.Builder(this@SettingsActivity)
                                    .setTitle("诊断失败")
                                    .setMessage(msgBuilder.toString())
                                    .setPositiveButton(getString(R.string.action_ok), null)
                                    .show()
                                return@runOnUiThread
                            }

                            msgBuilder.append("网络连接: ${mapStatus(diag.networkStatus)}\n")
                            msgBuilder.append("身份认证: ${mapStatus(diag.authStatus)}\n")
                            msgBuilder.append("仓库访问: ${mapStatus(diag.repoStatus)}\n")
                            msgBuilder.append("分支存在: ${mapStatus(diag.branchStatus)}\n\n")

                            if (!diag.networkProbeSummary.isNullOrEmpty()) {
                                msgBuilder.append("=== 自动网络探测 ===\n")
                                msgBuilder.append("最终选择模式: ${diag.chosenNetworkMode ?: "未知"}\n")
                                diag.networkProbeSummary.forEach { probe ->
                                    val mark = if (probe.success) "✅" else "❌"
                                    msgBuilder.append("$mark ${probe.mode}: ${probe.message}\n")
                                }
                                msgBuilder.append("\n")
                            } else {
                                val proxyType = diag.proxyType
                                if (diag.proxyUsed && proxyType != "none") {
                                    val protocol = if (proxyType == "socks5") "socks5h" else if (proxyType == "auto") "auto" else "http"
                                    if (protocol == "auto") {
                                        msgBuilder.append("代理配置: auto\n")
                                    } else {
                                        msgBuilder.append("代理配置: ${protocol}://${diag.proxyHost}:${diag.proxyPort}\n")
                                        if (protocol == "http" || protocol == "socks5h") {
                                            msgBuilder.append("  TCP 连通: ${if (diag.tcpProbeOk) "成功" else "失败"} (${diag.tcpProbeStatus})\n")
                                            if (protocol == "http") {
                                                msgBuilder.append("  HTTP CONNECT: ${if (diag.httpConnectProbeOk) "成功" else "失败"} (${diag.httpConnectProbeStatus})\n")
                                            }
                                        }
                                    }
                                    msgBuilder.append("  libgit2 访问: ${if (diag.libgit2ProbeOk) "成功" else "失败"} (${diag.libgit2ProbeStatus})\n\n")
                                } else {
                                    msgBuilder.append("代理配置: 未使用显式代理\n\n")
                                    msgBuilder.append("提示：当前同步底层没有使用显式代理。系统代理/TUN 是否接管取决于系统路由和 Clash，本应用不能保证 libgit2 自动读取。如果同步失败，建议启用 HTTP 代理 127.0.0.1:7890。\n\n")
                                }
                            }

                            msgBuilder.append(diag.userMessage)

                            if (diag.rawError != null && diag.rawError.isNotEmpty()) {
                                msgBuilder.append("\n\n原始错误:\n${diag.rawError}")
                            }

                            AlertDialog.Builder(this@SettingsActivity)
                                .setTitle(if (diag.success) "诊断成功" else "诊断失败")
                                .setMessage(msgBuilder.toString())
                                .setPositiveButton(getString(R.string.action_ok), null)
                                .show()
                        }
                    }
                    is NativeResult.Error -> {
                        Toast.makeText(this@SettingsActivity, result.message, Toast.LENGTH_LONG).show()
                    }
                    NativeResult.NotLoaded -> {
                        Toast.makeText(this@SettingsActivity, getString(R.string.sync_error_not_loaded), Toast.LENGTH_SHORT).show()
                    }
                }
            }
        }.start()
    }

    private var isSyncing = false
    private var isDryRunning = false

    private fun handlePerformSync() {
        if (isSyncing || isDryRunning || isTesting) return
        isSyncing = true
        btnPerformSync.text = "同步中..."
        btnPerformSync.isEnabled = false
        btnDryRun.isEnabled = false
        btnTestConnection.isEnabled = false
        saveCurrentState()

        if (currentSyncSecrets.token.isNullOrEmpty()) {
            isSyncing = false
            btnPerformSync.text = "立即同步"
            btnPerformSync.isEnabled = true
            btnDryRun.isEnabled = true
            btnTestConnection.isEnabled = true
            Toast.makeText(this, getString(R.string.sync_error_no_token), Toast.LENGTH_SHORT).show()
            return
        }

        Thread {
            val result = ErrorUtil.safeRun(this@SettingsActivity, NativeResult.Error("Exception during sync")) {
                settingsRepository.performSync(currentSyncConfig)
            }
            if (isDestroyed || isFinishing) return@Thread
            runOnUiThread {
                isSyncing = false
                btnPerformSync.text = "立即同步"
                btnPerformSync.isEnabled = true
                btnDryRun.isEnabled = true
                btnTestConnection.isEnabled = true
                when (result) {
                    is NativeResult.Success -> {
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
                            } else if (syncResult.status == com.xiwei.writerapp.model.SyncStatus.Success) {
                                Toast.makeText(this@SettingsActivity, getString(R.string.sync_success), Toast.LENGTH_SHORT).show()
                            }
                        }
                    }
                    is NativeResult.Error -> {
                        Toast.makeText(this@SettingsActivity, result.message, Toast.LENGTH_LONG).show()
                    }
                    NativeResult.NotLoaded -> {
                        Toast.makeText(this@SettingsActivity, getString(R.string.sync_error_not_loaded), Toast.LENGTH_SHORT).show()
                    }
                }
            }
        }.start()
    }

    private fun updateTokenStatusUI() {
        val input = etHttpsToken.text?.toString() ?: ""
        if (input.isNotEmpty()) {
            tvTokenStatus.text = getString(R.string.token_input_active)
            tvTokenStatus.setTextColor(getColor(com.google.android.material.R.color.material_dynamic_primary40))
        } else {
            if (currentSyncSecrets.token.isNullOrEmpty()) {
                tvTokenStatus.text = getString(R.string.token_not_configured)
                tvTokenStatus.setTextColor(getColor(com.google.android.material.R.color.design_default_color_error))
            } else {
                tvTokenStatus.text = getString(R.string.token_configured)
                tvTokenStatus.setTextColor(getColor(com.google.android.material.R.color.material_dynamic_primary40))
            }
        }
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

    override fun onResume() {
        super.onResume()
        if (SettingsChangeBus.consumeChanged()) {
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

        loadSyncState()
        switchEnableSync.isChecked = currentSyncConfig.enabled ?: false
        etGithubRepo.setText(currentSyncConfig.remoteUrl ?: "")
        etBranch.setText(currentSyncConfig.branch ?: "main")
        switchAutoSync.isChecked = currentSyncConfig.autoSync ?: false
        sbSyncInterval.value = (currentSyncConfig.syncIntervalSeconds ?: 300).toFloat()
        tvSyncIntervalValue.text = "${currentSyncConfig.syncIntervalSeconds ?: 300}秒"
        updateTokenStatusUI()
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
            editorSmoothCursorDurationMs = sbSmoothCursorDuration.value.toInt()
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
        val newSyncConfig = currentSyncConfig.copy(
            enabled = switchEnableSync.isChecked,
            backendType = com.xiwei.writerapp.model.BackendType.GithubApi,
            remoteUrl = etGithubRepo.text?.toString() ?: "",
            transport = currentSyncConfig.transport ?: SyncTransport.HttpsToken,
            branch = etBranch.text?.toString()?.ifEmpty { "main" } ?: "main",
            autoSync = switchAutoSync.isChecked,
            syncIntervalSeconds = sbSyncInterval.value.toInt(),
            proxyEnabled = spinnerProxyType.selectedItemPosition >= 1,
            proxyType = when (spinnerProxyType.selectedItemPosition) {
                0 -> "none"
                1 -> "auto"
                2 -> "http"
                3 -> "socks5"
                else -> "none"
            },
            proxyHost = etProxyHost.text?.toString()?.ifEmpty { "127.0.0.1" } ?: "127.0.0.1",
            proxyPort = etProxyPort.text?.toString()?.toIntOrNull() ?: if (spinnerProxyType.selectedItemPosition == 3) 7891 else 7890
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
