package com.xiwei.sujian.ui

import android.widget.TextView
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.app.AppCompatActivity
import com.google.android.material.button.MaterialButton
import com.google.android.material.R.attr as M3Attr
import com.google.android.material.color.MaterialColors
import com.google.android.material.materialswitch.MaterialSwitch
import com.google.android.material.textfield.TextInputEditText
import com.xiwei.sujian.R
import com.xiwei.sujian.data.BridgeResult
import com.xiwei.sujian.data.ResultEnvelope
import com.xiwei.sujian.data.SettingsRepository
import com.xiwei.sujian.data.SyncSession
import com.xiwei.sujian.model.FirstSyncMode
import com.xiwei.sujian.model.SyncConfig
import com.xiwei.sujian.model.SyncSecrets

internal class SyncSettingsHelper(
    private val activity: AppCompatActivity,
    private val settingsRepository: SettingsRepository
) {
    lateinit var switchEnableSync: MaterialSwitch
    lateinit var etGithubRepo: TextInputEditText
    lateinit var etBranch: TextInputEditText
    lateinit var etHttpsToken: TextInputEditText
    lateinit var tvTokenStatus: TextView
    lateinit var switchAutoSync: MaterialSwitch
    lateinit var sbSyncInterval: com.google.android.material.slider.Slider
    lateinit var tvSyncIntervalValue: TextView
    lateinit var btnDryRun: MaterialButton
    lateinit var btnTestConnection: MaterialButton
    lateinit var btnPerformSync: MaterialButton

    var currentSyncConfig: SyncConfig = SyncConfig()
        private set
    var currentSyncSecrets: SyncSecrets = SyncSecrets()
        private set

    fun initViews() {
        switchEnableSync = activity.findViewById(R.id.switchEnableSync)
        etGithubRepo = activity.findViewById(R.id.etGithubRepo)
        etBranch = activity.findViewById(R.id.etBranch)
        etHttpsToken = activity.findViewById(R.id.etHttpsToken)
        tvTokenStatus = activity.findViewById(R.id.tvTokenStatus)
        switchAutoSync = activity.findViewById(R.id.switchAutoSync)
        sbSyncInterval = activity.findViewById(R.id.sbSyncInterval)
        tvSyncIntervalValue = activity.findViewById(R.id.tvSyncIntervalValue)
        btnDryRun = activity.findViewById(R.id.btnDryRun)
        btnTestConnection = activity.findViewById(R.id.btnTestConnection)
        btnPerformSync = activity.findViewById(R.id.btnPerformSync)

        etHttpsToken.addTextChangedListener(object : android.text.TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {}
            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {
                updateTokenStatusUI()
            }
            override fun afterTextChanged(s: android.text.Editable?) {}
        })

        sbSyncInterval.addOnChangeListener { _, value, _ ->
            tvSyncIntervalValue.text = formatSyncIntervalText(value.toInt())
        }
    }

    fun loadSyncState() {
        currentSyncConfig = ErrorUtil.safeRun(activity, SyncConfig()) {
            settingsRepository.loadSyncConfig()
        }
        currentSyncSecrets = ErrorUtil.safeRun(activity, SyncSecrets()) {
            settingsRepository.loadSyncSecrets()
        }
    }

    fun bindSyncSettings() {
        switchEnableSync.isChecked = currentSyncConfig.enabled ?: false
        etGithubRepo.setText(currentSyncConfig.remoteUrl ?: "")
        etBranch.setText(currentSyncConfig.branch ?: "main")
        switchAutoSync.isChecked = currentSyncConfig.autoSync ?: false
        sbSyncInterval.value = (currentSyncConfig.syncIntervalSeconds ?: 300).toFloat()
        tvSyncIntervalValue.text = formatSyncIntervalText(currentSyncConfig.syncIntervalSeconds ?: 300)
        updateTokenStatusUI()
    }

    fun getUIConfig(): SyncConfig {
        return currentSyncConfig.copy(
            enabled = switchEnableSync.isChecked,
            backendType = com.xiwei.sujian.model.BackendType.GithubApi,
            remoteUrl = etGithubRepo.text?.toString() ?: "",
            transport = currentSyncConfig.transport ?: com.xiwei.sujian.model.SyncTransport.HttpsToken,
            branch = etBranch.text?.toString()?.ifEmpty { "main" } ?: "main",
            autoSync = switchAutoSync.isChecked,
            syncIntervalSeconds = sbSyncInterval.value.toInt()
        )
    }

    fun saveCurrentState() {
        val uiConfig = getUIConfig()
        val tokenInput = etHttpsToken.text?.toString() ?: ""
        val uiSecrets = if (tokenInput.isNotEmpty()) {
            currentSyncSecrets.copy(token = tokenInput)
        } else currentSyncSecrets

        ErrorUtil.safeRun(activity) {
            settingsRepository.saveSyncConfig(uiConfig)
            settingsRepository.saveSyncSecrets(uiSecrets)
            // 保存设备信息
            val deviceInfo = mapOf(
                "deviceId" to getDeviceId(),
                "deviceClass" to determineDeviceClass(),
                "platform" to "android"
            )
            settingsRepository.saveDeviceInfo(deviceInfo)
            // 同时通过 Core 层写入 current_device.json
            settingsRepository.ensureDeviceInfo("android", determineDeviceClass())
        }
        currentSyncConfig = uiConfig
        currentSyncSecrets = uiSecrets
        updateTokenStatusUI()
    }

    fun handleDryRun() {
        if (!SyncSession.lock.compareAndSet(false, true)) return
        val taskId = SyncSession.currentTaskId.incrementAndGet()
        btnDryRun.text = activity.getString(R.string.sync_checking)
        btnDryRun.isEnabled = false
        btnPerformSync.isEnabled = false
        btnTestConnection.isEnabled = false
        saveCurrentState()

        Thread {
            val result = ErrorUtil.safeRun(activity, BridgeResult.Error(ResultEnvelope.error("UNKNOWN", "Exception during dry run"))) {
                settingsRepository.performSyncDryRun(currentSyncConfig)
            }
            if (activity.isDestroyed || activity.isFinishing || SyncSession.currentTaskId.get() != taskId) {
                SyncSession.lock.set(false)
                return@Thread
            }
            activity.runOnUiThread {
                if (activity.isDestroyed || activity.isFinishing || SyncSession.currentTaskId.get() != taskId) {
                    SyncSession.lock.set(false)
                    return@runOnUiThread
                }
                SyncSession.lock.set(false)
                btnDryRun.text = activity.getString(R.string.btn_dry_run)
                btnDryRun.isEnabled = true
                btnPerformSync.isEnabled = true
                btnTestConnection.isEnabled = true
                when (result) {
                    is BridgeResult.Success -> {
                        val plan = result.data
                        val msg = activity.getString(R.string.sync_plan_check_complete, activity.getString(R.string.sync_dry_run_result, plan.filesToUpload.size, plan.filesToDownload.size, plan.filesToDeleteRemote.size, plan.filesToDeleteLocal.size, plan.ignoredFiles.size))
                        android.widget.Toast.makeText(activity, msg, android.widget.Toast.LENGTH_LONG).show()
                    }
                    is BridgeResult.Error -> {
                        android.widget.Toast.makeText(activity, result.message, android.widget.Toast.LENGTH_LONG).show()
                    }
                    BridgeResult.NotLoaded -> {
                        android.widget.Toast.makeText(activity, activity.getString(R.string.sync_error_not_loaded), android.widget.Toast.LENGTH_SHORT).show()
                    }
                }
            }
        }.start()
    }

    fun handleTestConnection() {
        if (!SyncSession.lock.compareAndSet(false, true)) return
        val taskId = SyncSession.currentTaskId.incrementAndGet()
        btnTestConnection.text = activity.getString(R.string.sync_checking)
        btnTestConnection.isEnabled = false
        btnDryRun.isEnabled = false
        btnPerformSync.isEnabled = false
        saveCurrentState()

        Thread {
            val result = ErrorUtil.safeRun(activity, BridgeResult.Error(ResultEnvelope.error("UNKNOWN", "Exception during diagnostic run"))) {
                settingsRepository.performSyncDiagnostics(currentSyncConfig)
            }
            if (activity.isDestroyed || activity.isFinishing || SyncSession.currentTaskId.get() != taskId) {
                SyncSession.lock.set(false)
                return@Thread
            }
            activity.runOnUiThread {
                if (activity.isDestroyed || activity.isFinishing || SyncSession.currentTaskId.get() != taskId) {
                    SyncSession.lock.set(false)
                    return@runOnUiThread
                }
                SyncSession.lock.set(false)
                btnTestConnection.text = activity.getString(R.string.btn_test_connection)
                btnTestConnection.isEnabled = true
                btnDryRun.isEnabled = true
                btnPerformSync.isEnabled = true
                when (result) {
                    is BridgeResult.Success -> {
                        val diag = result.data
                        val msgBuilder = StringBuilder()
                            val mapStatus = { s: String ->
                                when (s) {
                                    "ok" -> activity.getString(R.string.diag_status_ok)
                                    "failed" -> activity.getString(R.string.diag_status_failed)
                                    "skipped" -> activity.getString(R.string.diag_status_skipped)
                                    else -> if (s.startsWith("failed")) activity.getString(R.string.diag_status_failed_detail, s) else activity.getString(R.string.diag_status_unchecked, s)
                                }
                            }

                            msgBuilder.append(activity.getString(R.string.diag_permission_section)).append("\n")
                            msgBuilder.append(activity.getString(R.string.diag_internet_permission, if (diag.androidHasInternetPermission) activity.getString(R.string.diag_permission_granted) else activity.getString(R.string.diag_permission_missing))).append("\n")
                            msgBuilder.append(activity.getString(R.string.diag_network_state_permission, if (diag.androidHasAccessNetworkStatePermission) activity.getString(R.string.diag_permission_granted) else activity.getString(R.string.diag_permission_missing))).append("\n")
                            msgBuilder.append(when (diag.androidNetworkState) {
                                "permission_granted" -> activity.getString(R.string.diag_network_state_granted)
                                "unknown_no_permission" -> activity.getString(R.string.diag_network_state_unknown)
                                "failed_no_internet_permission" -> activity.getString(R.string.diag_network_state_no_internet)
                                else -> diag.androidNetworkState
                            }).append("\n\n")

                            if (!diag.androidHasInternetPermission) {
                                msgBuilder.append("\n${errorMessageFromCategory(diag.errorCategory)}")
                                AlertDialog.Builder(activity)
                                    .setTitle(activity.getString(R.string.diag_title_failed))
                                    .setMessage(msgBuilder.toString())
                                    .setPositiveButton(activity.getString(R.string.action_ok), null)
                                    .show()
                                return@runOnUiThread
                            }

                            msgBuilder.append(activity.getString(R.string.diag_network_connection, mapStatus(diag.networkStatus))).append("\n")
                            msgBuilder.append(activity.getString(R.string.diag_auth_status, mapStatus(diag.authStatus))).append("\n")
                            msgBuilder.append(activity.getString(R.string.diag_repo_access, mapStatus(diag.repoStatus))).append("\n")
                            msgBuilder.append(activity.getString(R.string.diag_branch_exists, mapStatus(diag.branchStatus))).append("\n\n")

                            if (!diag.networkProbeSummary.isNullOrEmpty()) {
                                msgBuilder.append(activity.getString(R.string.diag_auto_probe_section)).append("\n")
                                msgBuilder.append(activity.getString(R.string.diag_chosen_mode, diag.chosenNetworkMode ?: activity.getString(R.string.diag_unknown_mode))).append("\n")
                                diag.networkProbeSummary.forEach { probe ->
                                    val mark = if (probe.success) "✅" else "❌"
                                    msgBuilder.append("$mark ${probe.mode}: ${probe.message}\n")
                                }
                                msgBuilder.append("\n")
                            }

                            msgBuilder.append(errorMessageFromCategory(diag.errorCategory))

                            if (diag.rawError != null && diag.rawError.isNotEmpty()) {
                                msgBuilder.append("\n\n").append(activity.getString(R.string.diag_raw_error_section, diag.rawError))
                            }

                        AlertDialog.Builder(activity)
                            .setTitle(if (diag.success) activity.getString(R.string.diag_title_success) else activity.getString(R.string.diag_title_failed))
                            .setMessage(msgBuilder.toString())
                            .setPositiveButton(activity.getString(R.string.action_ok), null)
                            .show()
                    }
                    is BridgeResult.Error -> {
                        android.widget.Toast.makeText(activity, result.message, android.widget.Toast.LENGTH_LONG).show()
                    }
                    BridgeResult.NotLoaded -> {
                        android.widget.Toast.makeText(activity, activity.getString(R.string.sync_error_not_loaded), android.widget.Toast.LENGTH_SHORT).show()
                    }
                }
            }
        }.start()
    }

    fun handlePerformSync() {
        if (!SyncSession.lock.compareAndSet(false, true)) return
        val taskId = SyncSession.currentTaskId.incrementAndGet()
        btnPerformSync.text = activity.getString(R.string.sync_syncing)
        btnPerformSync.isEnabled = false
        btnDryRun.isEnabled = false
        btnTestConnection.isEnabled = false
        saveCurrentState()

        if (currentSyncSecrets.token.isNullOrEmpty()) {
            SyncSession.lock.set(false)
            btnPerformSync.text = activity.getString(R.string.btn_perform_sync)
            btnPerformSync.isEnabled = true
            btnDryRun.isEnabled = true
            btnTestConnection.isEnabled = true
            android.widget.Toast.makeText(activity, activity.getString(R.string.sync_error_no_token), android.widget.Toast.LENGTH_SHORT).show()
            return
        }

        // 确保写作统计已 flush
        ErrorUtil.safeRun(activity) {
            settingsRepository.flushWritingStats()
        }

        Thread {
            val result = ErrorUtil.safeRun(activity, BridgeResult.Error(ResultEnvelope.error("UNKNOWN", "Exception during sync"))) {
                settingsRepository.performSync(currentSyncConfig, true)
            }
            if (activity.isDestroyed || activity.isFinishing || SyncSession.currentTaskId.get() != taskId) {
                SyncSession.lock.set(false)
                return@Thread
            }
            activity.runOnUiThread {
                if (activity.isDestroyed || activity.isFinishing || SyncSession.currentTaskId.get() != taskId) {
                    SyncSession.lock.set(false)
                    return@runOnUiThread
                }
                SyncSession.lock.set(false)
                btnPerformSync.text = activity.getString(R.string.btn_perform_sync)
                btnPerformSync.isEnabled = true
                btnDryRun.isEnabled = true
                btnTestConnection.isEnabled = true
                when (result) {
                    is BridgeResult.Success -> {
                        val syncResult = result.data
                        if (syncResult.status == com.xiwei.sujian.model.SyncStatus.Conflict || syncResult.status == com.xiwei.sujian.model.SyncStatus.PartialConflict) {
                                val summary = syncResult.conflictSummary
                                val settingConflicts = syncResult.settingsConflicts
                                val msgBuilder = StringBuilder()
                                
                                if (summary != null) {
                                    msgBuilder.append(summary.blockedReason).append("\n\n")
                                    if (summary.conflictedFiles.isNotEmpty()) {
                                        msgBuilder.append(activity.getString(R.string.sync_conflict_files)).append("\n")
                                        for (file in summary.conflictedFiles) {
                                            msgBuilder.append("  - ").append(file as CharSequence).append("\n")
                                        }
                                        msgBuilder.append("\n")
                                    }
                                } else {
                                    msgBuilder.append(activity.getString(R.string.sync_conflict_detected))
                                    if (syncResult.conflicts.isNotEmpty()) {
                                        msgBuilder.append(activity.getString(R.string.sync_conflict_files)).append("\n")
                                        for (c in syncResult.conflicts) {
                                            msgBuilder.append("  - ").append(c.localPath).append("\n")
                                        }
                                        msgBuilder.append("\n")
                                    }
                                }

                                if (!settingConflicts.isNullOrEmpty()) {
                                    msgBuilder.append(activity.getString(R.string.sync_setting_conflicts)).append("\n")
                                    for (sc in settingConflicts) {
                                        msgBuilder.append(activity.getString(R.string.sync_setting_conflict_detail, sc.key, sc.localValue, sc.remoteValue)).append("\n")
                                    }
                                    msgBuilder.append("\n")
                                }

                                if (summary != null && summary.safeNextSteps.isNotEmpty()) {
                                    msgBuilder.append(activity.getString(R.string.sync_safe_suggestions)).append("\n")
                                    for (step in summary.safeNextSteps) {
                                        msgBuilder.append("• ").append(step as CharSequence).append("\n")
                                    }
                                } else {
                                    msgBuilder.append(activity.getString(R.string.sync_safe_suggestions)).append("\n")
                                    msgBuilder.append(activity.getString(R.string.sync_suggestion_backup)).append("\n")
                                    msgBuilder.append(activity.getString(R.string.sync_suggestion_merge))
                                }

                                AlertDialog.Builder(activity)
                                    .setTitle(activity.getString(R.string.sync_conflict_title))
                                    .setMessage(msgBuilder.toString().trim())
                                    .setPositiveButton(activity.getString(R.string.action_ok), null)
                                    .show()
                            } else if (syncResult.status == com.xiwei.sujian.model.SyncStatus.Success ||
                                       syncResult.status == com.xiwei.sujian.model.SyncStatus.BranchMissingRecovered ||
                                       syncResult.status == com.xiwei.sujian.model.SyncStatus.NoChanges ||
                                       syncResult.status == com.xiwei.sujian.model.SyncStatus.LatestWinsApplied) {
                                val successMsg = if (syncResult.status == com.xiwei.sujian.model.SyncStatus.NoChanges) {
                                    activity.getString(R.string.sync_complete_no_changes)
                                } else {
                                    val title = if (syncResult.status == com.xiwei.sujian.model.SyncStatus.BranchMissingRecovered) {
                                        activity.getString(R.string.sync_success_recovered)
                                    } else {
                                        activity.getString(R.string.sync_success)
                                    }
                                    activity.getString(R.string.sync_success_detail, title, syncResult.uploadedFiles.size, syncResult.downloadedFiles.size, syncResult.localDeletes.size, syncResult.remoteDeletes.size, syncResult.overwrittenFiles.size)
                                }
                                android.widget.Toast.makeText(activity, successMsg, android.widget.Toast.LENGTH_LONG).show()
                            } else if (syncResult.status == com.xiwei.sujian.model.SyncStatus.DirtyRepoBlocked) {
                                AlertDialog.Builder(activity)
                                    .setTitle(activity.getString(R.string.sync_blocked_title))
                                    .setMessage(activity.getString(R.string.sync_blocked_message))
                                    .setPositiveButton(activity.getString(R.string.action_ok), null)
                                    .show()
                            } else if (syncResult.status == com.xiwei.sujian.model.SyncStatus.RecoverableError || syncResult.status == com.xiwei.sujian.model.SyncStatus.FatalError || syncResult.status == com.xiwei.sujian.model.SyncStatus.Error) {
                                val title = if (syncResult.status == com.xiwei.sujian.model.SyncStatus.RecoverableError) activity.getString(R.string.sync_recoverable_error_title) else activity.getString(R.string.sync_fatal_error_title)
                                val errMsg = syncResult.error ?: errorMessageFromCategory(syncResult.errorCategory)
                                AlertDialog.Builder(activity)
                                    .setTitle(title)
                                    .setMessage(errMsg)
                                    .setPositiveButton(activity.getString(R.string.action_ok), null)
                                    .show()
                            } else if (syncResult.errorCategory != null) {
                                AlertDialog.Builder(activity)
                                    .setMessage(errorMessageFromCategory(syncResult.errorCategory))
                                    .setPositiveButton(activity.getString(R.string.action_ok), null)
                                    .show()
                            } else if (syncResult.firstSyncMode == FirstSyncMode.UnrelatedHistories || syncResult.firstSyncMode == FirstSyncMode.BlockedNonEmptyRemote) {
                                AlertDialog.Builder(activity)
                                    .setMessage(activity.getString(R.string.sync_error_unrelated))
                                    .setPositiveButton(activity.getString(R.string.action_ok), null)
                                    .show()
                            } else if (syncResult.error != null) {
                                AlertDialog.Builder(activity)
                                    .setMessage(syncResult.error)
                                    .setPositiveButton(activity.getString(R.string.action_ok), null)
                                    .show()
                        }
                    }
                    is BridgeResult.Error -> {
                        AlertDialog.Builder(activity)
                            .setTitle(activity.getString(R.string.sync_fatal_error_title))
                            .setMessage(result.message)
                            .setPositiveButton(activity.getString(R.string.action_ok), null)
                            .show()
                    }
                    BridgeResult.NotLoaded -> {
                        AlertDialog.Builder(activity)
                            .setTitle(activity.getString(R.string.sync_fatal_error_title))
                            .setMessage(activity.getString(R.string.sync_error_not_loaded))
                            .setPositiveButton(activity.getString(R.string.action_ok), null)
                            .show()
                    }
                }
            }
        }.start()
    }

    fun formatSyncIntervalText(seconds: Int): String {
        val minutes = seconds / 60
        return if (minutes < 15) {
            activity.getString(R.string.sync_interval_minutes_effective, minutes)
        } else {
            activity.getString(R.string.sync_interval_minutes, minutes)
        }
    }

    fun updateTokenStatusUI() {
        val input = etHttpsToken.text?.toString() ?: ""
        if (input.isNotEmpty()) {
            tvTokenStatus.text = activity.getString(R.string.token_input_active)
            tvTokenStatus.setTextColor(MaterialColors.getColor(tvTokenStatus, M3Attr.colorPrimary))
        } else {
            if (currentSyncSecrets.token.isNullOrEmpty()) {
                tvTokenStatus.text = activity.getString(R.string.token_not_configured)
                tvTokenStatus.setTextColor(MaterialColors.getColor(tvTokenStatus, M3Attr.colorError))
            } else {
                tvTokenStatus.text = activity.getString(R.string.token_configured)
                tvTokenStatus.setTextColor(MaterialColors.getColor(tvTokenStatus, M3Attr.colorPrimary))
            }
        }
    }

    fun buildSaveSyncConfig(): SyncConfig {
        return currentSyncConfig.copy(
            enabled = switchEnableSync.isChecked,
            backendType = com.xiwei.sujian.model.BackendType.GithubApi,
            remoteUrl = etGithubRepo.text?.toString() ?: "",
            transport = currentSyncConfig.transport ?: com.xiwei.sujian.model.SyncTransport.HttpsToken,
            branch = etBranch.text?.toString()?.ifEmpty { "main" } ?: "main",
            autoSync = switchAutoSync.isChecked,
            syncIntervalSeconds = sbSyncInterval.value.toInt()
        )
    }

    fun buildSaveSyncSecrets(): SyncSecrets {
        val tokenInput = etHttpsToken.text?.toString() ?: ""
        return if (tokenInput.isNotEmpty()) {
            currentSyncSecrets.copy(token = tokenInput)
        } else currentSyncSecrets
    }

    /**
     * 判断设备类型：smallestScreenWidthDp >= 600 为 tablet，否则为 phone。
     */
    fun determineDeviceClass(): String {
        val config = activity.resources.configuration
        return if (config.smallestScreenWidthDp >= 600) {
            "tablet"
        } else {
            "phone"
        }
    }

    /**
     * 获取本地持久化的设备 UUID，首次访问时生成并保存。
     */
    private fun getDeviceId(): String {
        val prefs = activity.getSharedPreferences("sujian_device", android.content.Context.MODE_PRIVATE)
        var id = prefs.getString("deviceId", null)
        if (id == null) {
            id = java.util.UUID.randomUUID().toString()
            prefs.edit().putString("deviceId", id).apply()
        }
        return id
    }

    /**
     * 根据 errorCategory 映射到对应的 string resource。
     * Core 层已完成 user_message 去中文化，UI 层不再依赖 userMessage 直接展示。
     */
    private fun errorMessageFromCategory(errorCategory: String?): String {
        return when (errorCategory) {
            // Core 细粒度分类
            "token_missing" -> activity.getString(R.string.sync_token_missing)
            "token_invalid" -> activity.getString(R.string.sync_auth_failed)
            "token_permission_denied" -> activity.getString(R.string.sync_auth_failed)
            "repo_not_found_or_no_permission" -> activity.getString(R.string.sync_repo_not_found)
            "remote_branch_missing" -> activity.getString(R.string.sync_branch_missing)
            "github_network_failed" -> activity.getString(R.string.sync_network_failed)
            "dns_failed" -> activity.getString(R.string.sync_network_failed)
            "tls_failed" -> activity.getString(R.string.sync_network_failed)
            "network_probe_failed" -> activity.getString(R.string.sync_network_failed)
            "conflict" -> activity.getString(R.string.error_sync_conflict)
            // WriterError 级别（兼容旧值）
            "SYNC_CONFLICT" -> activity.getString(R.string.error_sync_conflict)
            "SYNC_FAILED" -> activity.getString(R.string.error_sync_failed)
            "IO_ERROR" -> activity.getString(R.string.error_io)
            "JSON_ERROR" -> activity.getString(R.string.error_json)
            else -> activity.getString(R.string.sync_unknown_error)
        }
    }
}