package com.xiwei.sujian.ui

import android.widget.TextView
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.app.AppCompatActivity
import com.google.android.material.button.MaterialButton
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
        }
        currentSyncConfig = uiConfig
        currentSyncSecrets = uiSecrets
        updateTokenStatusUI()
    }

    fun handleDryRun() {
        if (!SyncSession.lock.compareAndSet(false, true)) return
        val taskId = SyncSession.currentTaskId.incrementAndGet()
        btnDryRun.text = "检查中..."
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
                btnDryRun.text = "检查同步计划"
                btnDryRun.isEnabled = true
                btnPerformSync.isEnabled = true
                btnTestConnection.isEnabled = true
                when (result) {
                    is BridgeResult.Success -> {
                        val plan = result.data
                        val msg = "同步计划检查完成: " + activity.getString(R.string.sync_dry_run_result, plan.filesToUpload.size, plan.filesToDownload.size, plan.filesToDeleteRemote.size, plan.filesToDeleteLocal.size, plan.ignoredFiles.size)
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
        btnTestConnection.text = "检查中..."
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
                                    "ok" -> "正常"
                                    "failed" -> "失败"
                                    "skipped" -> "已跳过"
                                    else -> if (s.startsWith("failed")) "失败 ($s)" else "未检查 ($s)"
                                }
                            }

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

                            if (!diag.androidHasInternetPermission) {
                                msgBuilder.append("\n${diag.userMessage}")
                                AlertDialog.Builder(activity)
                                    .setTitle("诊断失败")
                                    .setMessage(msgBuilder.toString())
                                    .setPositiveButton(activity.getString(R.string.action_ok), null)
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
                            }

                            msgBuilder.append(diag.userMessage)

                            if (diag.rawError != null && diag.rawError.isNotEmpty()) {
                                msgBuilder.append("\n\n原始错误:\n${diag.rawError}")
                            }

                        AlertDialog.Builder(activity)
                            .setTitle(if (diag.success) "诊断成功" else "诊断失败")
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
        btnPerformSync.text = "同步中..."
        btnPerformSync.isEnabled = false
        btnDryRun.isEnabled = false
        btnTestConnection.isEnabled = false
        saveCurrentState()

        if (currentSyncSecrets.token.isNullOrEmpty()) {
            SyncSession.lock.set(false)
            btnPerformSync.text = "立即同步"
            btnPerformSync.isEnabled = true
            btnDryRun.isEnabled = true
            btnTestConnection.isEnabled = true
            android.widget.Toast.makeText(activity, activity.getString(R.string.sync_error_no_token), android.widget.Toast.LENGTH_SHORT).show()
            return
        }

        Thread {
            val result = ErrorUtil.safeRun(activity, BridgeResult.Error(ResultEnvelope.error("UNKNOWN", "Exception during sync"))) {
                settingsRepository.performSync(currentSyncConfig)
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
                btnPerformSync.text = "立即同步"
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
                                        msgBuilder.append("冲突文件:\n")
                                        for (file in summary.conflictedFiles) {
                                            msgBuilder.append("  - ").append(file).append("\n")
                                        }
                                        msgBuilder.append("\n")
                                    }
                                } else {
                                    msgBuilder.append("同步中检测到冲突。\n\n")
                                    if (syncResult.conflicts.isNotEmpty()) {
                                        msgBuilder.append("冲突文件:\n")
                                        for (c in syncResult.conflicts) {
                                            msgBuilder.append("  - ").append(c.localPath).append("\n")
                                        }
                                        msgBuilder.append("\n")
                                    }
                                }

                                if (!settingConflicts.isNullOrEmpty()) {
                                    msgBuilder.append("具体设置冲突:\n")
                                    for (sc in settingConflicts) {
                                        msgBuilder.append("  • 键名: ").append(sc.key)
                                            .append(", 本地值: ").append(sc.localValue)
                                            .append(", 远程值: ").append(sc.remoteValue).append("\n")
                                    }
                                    msgBuilder.append("\n")
                                }

                                if (summary != null && summary.safeNextSteps.isNotEmpty()) {
                                    msgBuilder.append("安全建议:\n")
                                    for (step in summary.safeNextSteps) {
                                        msgBuilder.append("• ").append(step).append("\n")
                                    }
                                } else {
                                    msgBuilder.append("安全建议:\n")
                                    msgBuilder.append("• 备份当前工作区。\n")
                                    msgBuilder.append("• 确认诊断状态或手动合并后重新同步。")
                                }

                                AlertDialog.Builder(activity)
                                    .setTitle("同步冲突")
                                    .setMessage(msgBuilder.toString().trim())
                                    .setPositiveButton(activity.getString(R.string.action_ok), null)
                                    .show()
                            } else if (syncResult.status == com.xiwei.sujian.model.SyncStatus.Success ||
                                       syncResult.status == com.xiwei.sujian.model.SyncStatus.BranchMissingRecovered ||
                                       syncResult.status == com.xiwei.sujian.model.SyncStatus.NoChanges ||
                                       syncResult.status == com.xiwei.sujian.model.SyncStatus.LatestWinsApplied) {
                                val successMsg = syncResult.userMessage ?: if (syncResult.status == com.xiwei.sujian.model.SyncStatus.NoChanges) {
                                    "同步完成：本地和远端均已是最新状态。"
                                } else {
                                    val title = if (syncResult.status == com.xiwei.sujian.model.SyncStatus.BranchMissingRecovered) {
                                        "同步成功 (已关联并恢复缺失的分支)"
                                    } else {
                                        activity.getString(R.string.sync_success)
                                    }
                                    "$title\n上传: ${syncResult.uploadedFiles.size} 下载: ${syncResult.downloadedFiles.size} 本地删除: ${syncResult.localDeletes.size} 远端删除: ${syncResult.remoteDeletes.size} 覆盖: ${syncResult.overwrittenFiles.size}"
                                }
                                android.widget.Toast.makeText(activity, successMsg, android.widget.Toast.LENGTH_LONG).show()
                            } else if (syncResult.status == com.xiwei.sujian.model.SyncStatus.DirtyRepoBlocked) {
                                AlertDialog.Builder(activity)
                                    .setTitle("同步被阻止")
                                    .setMessage("同步被阻止：本地仓库有未提交的改动，且非安全设置文件，请先提交或备份改动后再试。")
                                    .setPositiveButton(activity.getString(R.string.action_ok), null)
                                    .show()
                            } else if (syncResult.status == com.xiwei.sujian.model.SyncStatus.RecoverableError || syncResult.status == com.xiwei.sujian.model.SyncStatus.FatalError || syncResult.status == com.xiwei.sujian.model.SyncStatus.Error) {
                                val title = if (syncResult.status == com.xiwei.sujian.model.SyncStatus.RecoverableError) "可恢复错误" else "同步失败"
                                val errMsg = syncResult.userMessage ?: syncResult.error ?: "未知同步错误"
                                AlertDialog.Builder(activity)
                                    .setTitle(title)
                                    .setMessage(errMsg)
                                    .setPositiveButton(activity.getString(R.string.action_ok), null)
                                    .show()
                            } else if (syncResult.userMessage != null) {
                                AlertDialog.Builder(activity)
                                    .setMessage(syncResult.userMessage)
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
                        android.widget.Toast.makeText(activity, result.message, android.widget.Toast.LENGTH_LONG).show()
                    }
                    BridgeResult.NotLoaded -> {
                        android.widget.Toast.makeText(activity, activity.getString(R.string.sync_error_not_loaded), android.widget.Toast.LENGTH_SHORT).show()
                    }
                }
            }
        }.start()
    }

    fun formatSyncIntervalText(seconds: Int): String {
        val minutes = seconds / 60
        return if (minutes < 15) {
            "${minutes}分钟 (有效后台间隔: 15分钟)"
        } else {
            "${minutes}分钟"
        }
    }

    fun updateTokenStatusUI() {
        val input = etHttpsToken.text?.toString() ?: ""
        if (input.isNotEmpty()) {
            tvTokenStatus.text = activity.getString(R.string.token_input_active)
            tvTokenStatus.setTextColor(activity.getColor(com.google.android.material.R.color.material_dynamic_primary40))
        } else {
            if (currentSyncSecrets.token.isNullOrEmpty()) {
                tvTokenStatus.text = activity.getString(R.string.token_not_configured)
                tvTokenStatus.setTextColor(activity.getColor(com.google.android.material.R.color.design_default_color_error))
            } else {
                tvTokenStatus.text = activity.getString(R.string.token_configured)
                tvTokenStatus.setTextColor(activity.getColor(com.google.android.material.R.color.material_dynamic_primary40))
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
}