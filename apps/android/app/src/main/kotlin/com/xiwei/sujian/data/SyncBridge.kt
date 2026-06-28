package com.xiwei.sujian.data

import com.xiwei.sujian.model.*

class SyncBridge(private val appService: AppServiceBridge) {
    fun loadSyncConfig(): BridgeResult<SyncConfig> = appService.loadSyncConfig()
    fun saveSyncConfig(config: SyncConfig): BridgeResult<Boolean> = appService.saveSyncConfig(config)
    fun loadSyncSecrets(): BridgeResult<SyncSecrets> = appService.loadSyncSecrets()
    fun saveSyncSecrets(secrets: SyncSecrets): BridgeResult<Boolean> = appService.saveSyncSecrets(secrets)
    fun loadSyncState(): BridgeResult<SyncState> = appService.loadSyncState()
    fun performSyncDiagnostics(config: SyncConfig): BridgeResult<SyncDiagnosticsResult> = appService.performSyncDiagnostics(config)
    fun performSyncDryRun(config: SyncConfig): BridgeResult<SyncPlan> = appService.performSyncDryRun(config)
    fun performSync(config: SyncConfig): BridgeResult<SyncResult> = appService.performSync(config)
}